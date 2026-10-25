//! Port of `MOUSE_TRAIL_TRACER` (`pcbnew/router/pns_mouse_trail_tracer.cpp`): the posture solver of the line placer. It keeps the path the cursor took
//! since the route (or the last fixed leg) started and decides, from how that path bends, whether the 45-degree elbow of the head is a straight
//! leg first or a diagonal one first (`GetPosture`): the person who moves the mouse along a diagonal and then along a straight run gets a
//! diagonal-first head, one who moves the other way gets a straight-first one. "Use mouse path to set track posture" (`m_autoPosture`) is its
//! switch (`SetMouseDisabled`): off, the posture is the one the route started with, or the one `/` flipped it to.
//!
//! The trail and every distance and area are in nanometres, the crate's unit and KiCad's, because the
//! solver's tests are integer ones (`SEG::SquaredDistance( SEG ) <= tolerance^2`, `TestSegmentHit` to the nanometre, `refLength` truncated to an
//! integer, `ratio = areaS / ( areaDiag + 1.0 )`) and the same arithmetic on the same numbers gives KiCad's answers. `SHAPE_LINE_CHAIN`'s `Append`,
//! `Simplify` (the 2024 version: "points must be exactly co-linear to be removed"), `Slice`, `Reverse` and `Area` are the few functions of it the
//! tracer calls; they are ported here in the plain-polyline form (a trail has no arcs).

use crate::direction45::{AngleType, CornerMode, Direction45};
use eda_drc::kimath::Seg;
use eda_model::ir::{Point, Nm};

/// `PNS::MOUSE_TRAIL_TRACER`.
#[derive(Debug, Clone)]
pub struct MouseTrailTracer {
    /// `m_trail`: an open chain, nanometres.
    trail: Vec<Point>,
    /// `m_tolerance`, nanometres.
    tolerance: Nm,
    /// `m_direction`.
    direction: Direction45,
    /// `m_lastSegDirection`.
    last_seg_direction: Direction45,
    /// `m_forced`: the solution is locked in.
    forced: bool,
    /// `m_disableMouse`.
    disable_mouse: bool,
    /// `m_manuallyForced`: `/` was pressed.
    manually_forced: bool,
}

impl Default for MouseTrailTracer {
    /// `MOUSE_TRAIL_TRACER::MOUSE_TRAIL_TRACER`: tolerance 0, mouse enabled, `Clear()`; both directions `UNDEFINED`.
    fn default() -> Self {
        MouseTrailTracer { trail: Vec::new(), tolerance: Nm::ZERO, direction: Direction45::Undefined, last_seg_direction: Direction45::Undefined, forced: false, disable_mouse: false, manually_forced: false }
    }
}

impl MouseTrailTracer {
    pub fn new() -> Self {
        Self::default()
    }

    /// `Clear`.
    pub fn clear(&mut self) {
        self.forced = false;
        self.manually_forced = false;
        self.trail.clear();
    }

    /// `SetTolerance`: the track width.
    pub fn set_tolerance(&mut self, tolerance: Nm) {
        self.tolerance = tolerance;
    }

    /// `SetDefaultDirections( aInitDirection, aLastSegDir )`.
    pub fn set_default_directions(&mut self, init_direction: Direction45, last_seg_dir: Direction45) {
        self.direction = init_direction;
        self.last_seg_direction = last_seg_dir;
    }

    /// `SetMouseDisabled`: leaves only the manual posture switch and the previous-segment posture.
    pub fn set_mouse_disabled(&mut self, disabled: bool) {
        self.disable_mouse = disabled;
    }

    /// `IsManuallyForced`.
    pub fn is_manually_forced(&self) -> bool {
        self.manually_forced
    }

    /// The posture the tracer holds now (`m_direction`), without solving anything.
    pub fn direction(&self) -> Direction45 {
        self.direction
    }

    /// `GetTrailLeadVector`: from the first point of the trail to its last, micrometres.
    pub fn trail_lead_vector(&self) -> (Nm, Nm) {
        if self.trail.len() < 2 {
            (Nm::ZERO, Nm::ZERO)
        } else {
            let (first, last) = (self.trail[0], *self.trail.last().unwrap());
            (last.x - first.x, last.y - first.y)
        }
    }

    /// The points of the trail, micrometres.
    pub fn trail(&self) -> Vec<Point> {
        self.trail.clone()
    }

    /// `AddTrailPoint`: the cursor moved to `p`. A new leg of the trail that comes within the tolerance of an older one cuts the loop out of it.
    pub fn add_trail_point(&mut self, p: Point) {
        if segment_count(&self.trail) == 0 {
            append(&mut self.trail, p);
        } else {
            let s_new = Seg::new(*self.trail.last().unwrap(), p);
            if segment_count(&self.trail) > 2 {
                let limit = self.tolerance.area(self.tolerance);
                for i in 0..segment_count(&self.trail) - 2 {
                    let s_trail = Seg::new(self.trail[i], self.trail[i + 1]);
                    if s_trail.sq_distance_to_seg(&s_new) <= limit {
                        self.trail.truncate(i + 1); // `m_trail.Slice( 0, i )`
                        break;
                    }
                }
            }
            append(&mut self.trail, p);
        }
        simplify(&mut self.trail, false);
    }

    /// `FlipPosture`: the next octant, locked in.
    pub fn flip_posture(&mut self) {
        self.direction = self.direction.right();
        self.forced = true;
        self.manually_forced = true;
    }

    /// `GetPosture( aP )`: the posture of the head that ends at `p` (micrometres), given the trail so far.
    pub fn get_posture(&mut self, p: Point) -> Direction45 {
        // Tuning factor for how good the "fit" of the trail must be to the posture
        const AREA_RATIO_THRESHOLD: f64 = 1.3;
        // Tuning factor to minimize flutter
        const AREA_RATIO_EPSILON: f64 = 0.25;
        // Minimum distance factor of the trail before the min area test is used to lock the solver
        const MIN_AREA_CUTOFF_DISTANCE_FACTOR: f64 = 6.0;
        // Adjusts how far away from p0 we get before whatever posture we solved is locked in
        const LOCK_DISTANCE_FACTOR: i64 = 30;
        // Adjusts how close to p0 we unlock the posture again if one was locked already
        const UNLOCK_DISTANCE_FACTOR: i64 = 10;

        if self.trail.len() < 2 || self.manually_forced {
            // If mouse trail detection is enabled; using the last seg direction as a starting point will give the best results. Otherwise, just
            // assume that we switch postures every segment.
            if !self.manually_forced && self.last_seg_direction != Direction45::Undefined {
                self.direction = if self.disable_mouse { self.last_seg_direction.right() } else { self.last_seg_direction };
            }
            return self.direction;
        }

        let p0 = self.trail[0];
        // `double refLength = SEG( p0, aP ).Length()`: the length is an integer (`EuclideanNorm` truncates)
        let ref_length = ((((p.x - p0.x).as_f64()).powi(2) + ((p.y - p0.y).as_f64()).powi(2)).sqrt() as i64) as f64;
        let reversed: Vec<Point> = self.trail.iter().rev().copied().collect();

        // `SHAPE_LINE_CHAIN straight( DIRECTION_45().BuildInitialTrace( p0, aP, false ) ); straight.SetClosed( true ); straight.Append( m_trail.Reverse() );
        // straight.Simplify();`
        let mut straight = initial_trace(p0, p, false);
        append_chain(&mut straight, &reversed);
        merge_first_last_if_needed(&mut straight); // a closed chain: `Append` merges the first and last points
        simplify(&mut straight, true);
        let area_s = area(&straight);

        // `SHAPE_LINE_CHAIN diag( DIRECTION_45().BuildInitialTrace( p0, aP, true ) ); diag.Append( m_trail.Reverse() ); diag.SetClosed( true ); diag.Simplify();`
        let mut diag = initial_trace(p0, p, true);
        append_chain(&mut diag, &reversed);
        simplify(&mut diag, true);
        let area_diag = area(&diag);
        let ratio = area_s / (area_diag + 1.0);

        // heuristic to detect that the user dragged back the cursor to the beginning of the trace: in this case, we cancel any forced posture and
        // restart the trail
        if self.forced && ref_length < (UNLOCK_DISTANCE_FACTOR * self.tolerance).as_f64() {
            self.forced = false;
            self.trail.clear();
            self.trail.push(p0);
        }

        let mut area_ok = false;

        // Check the actual trail area against the cutoff. This prevents flutter when the trail is very close to a straight line.
        if !self.forced && ref_length > MIN_AREA_CUTOFF_DISTANCE_FACTOR * self.tolerance.as_f64() {
            let area_cutoff = self.tolerance.as_f64() * ref_length;
            // `SHAPE_LINE_CHAIN trail( m_trail ); trail.SetClosed( true ); trail.Area()`
            if area(&self.trail) > area_cutoff {
                area_ok = true;
            }
        }

        let straight_direction = first_segment_direction(&straight);
        let diag_direction = first_segment_direction(&diag);
        let new_direction;

        if !self.forced && area_ok && ratio > AREA_RATIO_THRESHOLD + AREA_RATIO_EPSILON {
            new_direction = diag_direction;
        } else if !self.forced && area_ok && ratio < (1.0 / AREA_RATIO_THRESHOLD) - AREA_RATIO_EPSILON {
            new_direction = straight_direction;
        } else {
            new_direction = if self.direction.is_diagonal() { diag_direction } else { straight_direction };
        }

        if !self.disable_mouse && new_direction != self.direction {
            self.direction = new_direction;
        }

        // If we have a last segment, correct the direction relative to it. For segment exit, we want to correct to the least obtuse
        if !self.manually_forced && !self.disable_mouse && self.last_seg_direction != Direction45::Undefined {
            if straight_direction == self.last_seg_direction {
                self.direction = straight_direction;
            } else if diag_direction == self.last_seg_direction {
                self.direction = diag_direction;
            } else {
                match self.direction.angle_to(&self.last_seg_direction) {
                    AngleType::HalfFull => {
                        // Force a better (acute) connection
                        self.direction = if self.direction.is_diagonal() { straight_direction } else { diag_direction };
                    }
                    AngleType::Acute => {
                        // Force a better connection by flipping if possible
                        let candidate = if self.direction.is_diagonal() { straight_direction } else { diag_direction };
                        if candidate.angle_to(&self.last_seg_direction) == AngleType::Right {
                            self.direction = candidate;
                        }
                    }
                    AngleType::Right => {
                        // Force a better connection by flipping if possible
                        let candidate = if self.direction.is_diagonal() { straight_direction } else { diag_direction };
                        if candidate.angle_to(&self.last_seg_direction) == AngleType::Obtuse {
                            self.direction = candidate;
                        }
                    }
                    _ => {}
                }
            }
        }

        // If we get far away from the initial point, lock in the current solution to prevent flutter
        if !self.forced && ref_length > (LOCK_DISTANCE_FACTOR * self.tolerance).as_f64() {
            self.forced = true;
        }

        self.direction
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// the few `SHAPE_LINE_CHAIN` functions the tracer stands on, for a chain without arcs (nanometres)
// ---------------------------------------------------------------------------------------------------------------------------------------------

/// `DIRECTION_45().BuildInitialTrace( p0, p1, aStartDiagonal )`, its points appended one by one (`Append( point )` leaves out a repeated one).
fn initial_trace(p0: Point, p1: Point, start_diagonal: bool) -> Vec<Point> {
    let mut out = Vec::new();
    for q in Direction45::Undefined.build_initial_trace(p0, p1, start_diagonal, CornerMode::Mitered45) {
        append(&mut out, q);
    }
    out
}

/// `SHAPE_LINE_CHAIN::SegmentCount` of an open chain.
fn segment_count(chain: &[Point]) -> usize {
    chain.len().saturating_sub(1)
}

/// `SHAPE_LINE_CHAIN::Append( const VECTOR2I&, false )`: a point equal to the last one is not added.
fn append(chain: &mut Vec<Point>, p: Point) {
    if chain.last() != Some(&p) {
        chain.push(p);
    }
}

/// `SHAPE_LINE_CHAIN::Append( const SHAPE_LINE_CHAIN& )`: the first point of `other` is left out when it is the last point of the chain.
fn append_chain(chain: &mut Vec<Point>, other: &[Point]) {
    if other.is_empty() {
        return;
    }
    if chain.is_empty() || other[0] != *chain.last().unwrap() {
        chain.push(other[0]);
    }
    chain.extend(other[1..].iter().copied());
}

/// `SHAPE_LINE_CHAIN::mergeFirstLastPointIfNeeded` of a closed chain: it does not repeat its first point at the end.
fn merge_first_last_if_needed(chain: &mut Vec<Point>) {
    if chain.len() > 1 && chain.first() == chain.last() {
        chain.pop();
    }
}

/// `SHAPE_LINE_CHAIN::Area` of a closed chain (shoelace, absolute), square nanometres.
fn area(chain: &[Point]) -> f64 {
    let size = chain.len();
    let mut sum = 0.0f64;
    let mut j = size.wrapping_sub(1);
    for i in 0..size {
        sum += (chain[j].x.as_f64() + chain[i].x.as_f64()) * (chain[j].y.as_f64() - chain[i].y.as_f64());
        j = i;
    }
    (sum * 0.5).abs()
}

/// `DIRECTION_45( chain.CSegment( 0 ) )`.
fn first_segment_direction(chain: &[Point]) -> Direction45 {
    if chain.len() < 2 {
        return Direction45::Undefined;
    }
    Direction45::from_seg(chain[0], chain[1])
}

/// `TestSegmentHit( p, a, b, 0 )` (`libs/kimath/src/trigo.cpp`): `p` is on the segment `a`-`b` to the nanometre.
fn test_segment_hit(p: Point, a: Point, b: Point) -> bool {
    let (xmin, xmax) = (a.x.min(b.x), a.x.max(b.x));
    let (ymin, ymax) = (a.y.min(b.y), a.y.max(b.y));
    // outside of the bounding box
    if ymin - p.y > Nm::ZERO || p.y - ymax > Nm::ZERO {
        return false;
    }
    if xmin - p.x > Nm::ZERO || p.x - xmax > Nm::ZERO {
        return false;
    }
    // eliminate easy cases
    if a.x == b.x && p.y > ymin && p.y < ymax {
        return (a.x - p.x).abs() <= Nm::ZERO;
    }
    if a.y == b.y && p.x > xmin && p.x < xmax {
        return (a.y - p.y).abs() <= Nm::ZERO;
    }
    Seg::new(a, b).sq_distance_to_point(p) < 1
}

/// `SHAPE_LINE_CHAIN::Simplify( 0 )`: the points that lie exactly on the line between the ones around them, and the duplicates, go.
fn simplify(chain: &mut Vec<Point>, closed: bool) {
    let n = chain.len();
    if n < 3 {
        return;
    }
    let mut new_points: Vec<Point> = Vec::with_capacity(n);
    let mut start_idx = 0usize;
    while start_idx < n {
        new_points.push(chain[start_idx]);
        // If the line is not closed, we need at least 3 points before simplifying
        if !closed && start_idx == n - 2 {
            break;
        }
        // Initialize the end index to be two points ahead of start
        let mut end_idx = (start_idx + 2) % n;
        let mut can_simplify = true;
        while can_simplify && end_idx != start_idx && (end_idx > start_idx || closed) {
            // Test all points between start_idx and end_idx
            let mut test_idx = (start_idx + 1) % n;
            while test_idx != end_idx {
                // Test if the point is within the allowed error
                if !test_segment_hit(chain[test_idx], chain[start_idx], chain[end_idx]) {
                    can_simplify = false;
                    break;
                }
                test_idx = (test_idx + 1) % n;
            }
            if can_simplify {
                // If we can simplify, move end_idx one further
                end_idx = (end_idx + 1) % n;
            }
        }
        // If we couldn't simplify at all, move to the next point
        if end_idx == (start_idx + 2) % n {
            start_idx += 1;
        } else {
            // Otherwise, jump to the last point we could include in the simplification
            let new_start_idx = (end_idx + n - 1) % n;
            // If we looped all the way around, we're done
            if new_start_idx <= start_idx {
                break;
            }
            start_idx = new_start_idx;
        }
    }
    // If we have only one point, we need to add a second point to make a line
    if new_points.len() == 1 {
        new_points.push(*chain.last().unwrap());
    }
    // If we are not closed, then the start and end points of the original line need to be the start and end points of the new line.
    if !closed && chain.last() != new_points.last() {
        new_points.push(*chain.last().unwrap());
    }
    *chain = new_points;
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;

    fn p(x: Nm, y: Nm) -> Point {
        Point { x, y }
    }

    fn tracer(tolerance: Nm, init: Direction45) -> MouseTrailTracer {
        // `LINE_PLACER::Start`
        let mut t = MouseTrailTracer::new();
        t.clear();
        t.add_trail_point(p(Nm::ZERO, Nm::ZERO));
        t.set_tolerance(tolerance);
        t.set_default_directions(init, Direction45::Undefined);
        t
    }

    /// The line placer asks for the posture while it routes the head to the cursor, and adds the cursor to the trail afterwards (`Move`).
    fn move_to(t: &mut MouseTrailTracer, at: Point) -> Direction45 {
        let d = t.get_posture(at);
        t.add_trail_point(at);
        d
    }

    #[test]
    fn simplify_drops_only_points_exactly_between_their_neighbours() {
        let mut c = vec![p(Nm::ZERO, Nm::ZERO), p(mm(0.01), Nm::ZERO), p(mm(0.02), Nm::ZERO), p(mm(0.02), mm(0.01)), p(mm(0.02), Nm::new(5000))];
        simplify(&mut c, false);
        // (10,0) is between (0,0) and (20,0); (20,10) is a corner and (20,5) is back along the leg, so both stay
        assert_eq!(c, vec![p(Nm::ZERO, Nm::ZERO), p(mm(0.02), Nm::ZERO), p(mm(0.02), mm(0.01)), p(mm(0.02), Nm::new(5000))]);
        let mut dup = vec![p(Nm::ZERO, Nm::ZERO), p(Nm::ZERO, Nm::ZERO), p(mm(0.01), mm(0.01))];
        simplify(&mut dup, false);
        assert_eq!(dup, vec![p(Nm::ZERO, Nm::ZERO), p(mm(0.01), mm(0.01))]);
    }

    #[test]
    fn a_closed_square_with_a_point_on_a_side_loses_that_point_and_keeps_its_corners() {
        let mut c = vec![p(Nm::ZERO, Nm::ZERO), p(Nm::new(5000), Nm::ZERO), p(mm(0.01), Nm::ZERO), p(mm(0.01), mm(0.01)), p(Nm::ZERO, mm(0.01))];
        simplify(&mut c, true);
        assert_eq!(c, vec![p(Nm::ZERO, Nm::ZERO), p(mm(0.01), Nm::ZERO), p(mm(0.01), mm(0.01)), p(Nm::ZERO, mm(0.01))]);
        assert_eq!(area(&c), 100.0 * 1.0e6);
    }

    #[test]
    fn the_trail_is_the_simplified_path_of_the_cursor_and_a_loop_back_across_it_cuts_the_loop_out() {
        let mut t = tracer(mm(0.2), Direction45::E);
        for at in [p(mm(1.0), Nm::ZERO), p(mm(2.0), Nm::ZERO), p(mm(3.0), mm(1.0))] {
            t.add_trail_point(at);
        }
        assert_eq!(t.trail(), vec![p(Nm::ZERO, Nm::ZERO), p(mm(2.0), Nm::ZERO), p(mm(3.0), mm(1.0))]);
        // up, back left and down again across the first leg: the part of the trail after the crossing is cut off
        for at in [p(mm(3.0), mm(3.0)), p(mm(1.0), mm(3.0)), p(mm(1.0), mm(-0.5))] {
            t.add_trail_point(at);
        }
        assert_eq!(t.trail().first(), Some(&p(Nm::ZERO, Nm::ZERO)));
        assert_eq!(t.trail().last(), Some(&p(mm(1.0), mm(-0.5))));
        assert!(!t.trail().contains(&p(mm(3.0), mm(3.0))), "the loop is out of the trail: {:?}", t.trail());
    }

    #[test]
    fn with_one_point_in_the_trail_the_posture_is_the_one_the_route_started_with() {
        let mut t = tracer(mm(0.2), Direction45::NE);
        assert_eq!(t.get_posture(p(mm(5.0), mm(1.0))), Direction45::NE);
        let mut t = tracer(mm(0.2), Direction45::E);
        assert_eq!(t.get_posture(p(mm(5.0), mm(1.0))), Direction45::E);
    }

    #[test]
    fn moving_the_mouse_along_a_diagonal_and_then_straight_makes_the_head_diagonal_first_and_the_other_way_round_straight_first() {
        // a trail that goes up-right at 45 degrees for 6 mm and then flat for 6 mm: the head from the start to the cursor wants the diagonal first
        let mut t = tracer(mm(0.2), Direction45::E);
        let mut d = Direction45::E;
        for i in 1..=12 {
            d = move_to(&mut t, p(mm(0.5) * i, mm(-0.5) * i));
        }
        for i in 1..=12 {
            d = move_to(&mut t, p(mm(6.0) + mm(0.5) * i, mm(-6.0)));
        }
        assert!(d.is_diagonal(), "diagonal first: {d:?}");

        // flat first, then the diagonal
        let mut t = tracer(mm(0.2), Direction45::NE);
        for i in 1..=12 {
            d = move_to(&mut t, p(mm(0.5) * i, Nm::ZERO));
        }
        for i in 1..=12 {
            d = move_to(&mut t, p(mm(6.0) + mm(0.5) * i, mm(-0.5) * i));
        }
        assert!(!d.is_diagonal(), "straight first: {d:?}");
    }

    #[test]
    fn a_flip_locks_the_posture_in_and_the_cursor_cannot_change_it() {
        let mut t = tracer(mm(0.2), Direction45::E);
        t.flip_posture();
        assert!(t.is_manually_forced());
        assert_eq!(t.direction(), Direction45::SE);
        for i in 1..=20 {
            assert_eq!(move_to(&mut t, p(mm(0.5) * i, mm(-0.5) * i)), Direction45::SE);
        }
        t.clear();
        assert!(!t.is_manually_forced());
    }

    #[test]
    fn with_the_mouse_disabled_the_cursor_never_changes_the_posture() {
        let mut t = tracer(mm(0.2), Direction45::E);
        t.set_mouse_disabled(true);
        for i in 1..=20 {
            assert_eq!(move_to(&mut t, p(mm(0.5) * i, mm(-0.5) * i)), Direction45::E);
        }
    }

    #[test]
    fn a_posture_locked_by_distance_is_unlocked_when_the_cursor_comes_back_to_the_start() {
        let mut t = tracer(mm(0.2), Direction45::E);
        for i in 1..=30 {
            move_to(&mut t, p(mm(0.4) * i, mm(-0.4) * i));
        }
        assert!(t.forced, "30 tolerances from the start locks the solution in");
        // back to within 10 tolerances of the start: unlocked, the trail restarts from the start point
        let _ = move_to(&mut t, p(mm(1.0), mm(-1.0)));
        assert!(!t.forced);
        assert_eq!(t.trail(), vec![p(Nm::ZERO, Nm::ZERO), p(mm(1.0), mm(-1.0))]);
    }

    #[test]
    fn the_direction_of_the_last_segment_decides_the_posture_for_the_first_move_after_a_fix() {
        // `UnfixRoute`: `SetDefaultDirections( m_initial_direction, m_direction )` -- the last segment's direction is set
        let mut t = MouseTrailTracer::new();
        t.add_trail_point(p(Nm::ZERO, Nm::ZERO));
        t.set_tolerance(mm(0.2));
        t.set_default_directions(Direction45::E, Direction45::NE);
        assert_eq!(t.get_posture(p(mm(5.0), Nm::ZERO)), Direction45::NE, "one point in the trail: the last segment's direction");
        t.set_mouse_disabled(true);
        assert_eq!(t.get_posture(p(mm(5.0), Nm::ZERO)), Direction45::E, "mouse off: the next octant after it");
    }
}
