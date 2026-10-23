//! Arcs in the router's world: the geometry behind `PNS::ARC` (`pcbnew/router/pns_arc.{h,cpp}`), `ArcHull` (`pns_utils.cpp`),
//! `LINE_PLACER::SplitAdjacentArcs` (`pns_line_placer.cpp`) and the way `SHAPE_LINE_CHAIN::Append( SHAPE_ARC )` turns an arc into the
//! points of a line.
//!
//! An arc is held as the three points of it the IR holds ([`ArcPts`]: start, a point on it, end -- `PCB_ARC`'s `(start) (mid) (end)`),
//! in micrometres. KiCad's own `SHAPE_ARC` is computed on the nanometre grid, so everything that needs its arithmetic goes through
//! [`crate::kigeom::ShapeArc`] (micrometres in, nanometres inside, micrometres out), and the polyline of an arc is KiCad's
//! `ConvertToPolyline` at its 1 um polygonisation error ([`ArcPts::polyline`]), which is what a `LINE` holds for an arc and what
//! [`crate::line::Line::arc_runs`] finds again in a line's points.

use crate::kigeom::{angle_of, normalize, rotate_about, to_nm, to_um, ShapeArc, ARC_POLYGONIZATION_MAX_ERROR_NM};
use eda_drc::kimath::{Seg, Shape};
use eda_model::ir::{Point, Nm};

/// `ARC_HIGH_DEF` (0.005 mm) in nanometres: `SHAPE_ARC::DefaultAccuracyForPCB()`, the slack KiCad adds to an arc's hull and, in a shove, to the
/// hull of each arc segment of the pushing line.
pub const ARC_HIGH_DEF_NM: i64 = 5_000;

/// How far an arc's collision shape is fattened (um) past the track's half width. None: the polyline it is made of is held to half a micrometre of the
/// curve (vertices a little outside it, chord midpoints a little inside, `ConvertToPolyline`), inside the 1 um the router's clearance epsilon allows, and
/// the segments of a line that goes through the arc are the very same chords -- so an arc as an item and as part of a line are judged alike.
pub const ARC_SHAPE_PAD_NM: Nm = 0;

/// An arc by three of its points, micrometres: where it starts, a point on it, where it ends (`PCB_ARC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArcPts {
    pub start: Point,
    pub mid: Point,
    pub end: Point,
}

fn cross(o: Point, a: Point, b: Point) -> i128 {
    (a.x - o.x) as i128 * (b.y - o.y) as i128 - (a.y - o.y) as i128 * (b.x - o.x) as i128
}

fn dist(a: Point, b: Point) -> f64 {
    (((a.x - b.x) as f64).powi(2) + ((a.y - b.y) as f64).powi(2)).sqrt()
}

impl ArcPts {
    pub fn new(start: Point, mid: Point, end: Point) -> Self {
        ArcPts { start, mid, end }
    }

    /// The same arc walked the other way (`SHAPE_ARC::Reverse`: the ends swap, the middle stays).
    pub fn reversed(&self) -> ArcPts {
        ArcPts { start: self.end, mid: self.mid, end: self.start }
    }

    /// `SHAPE_ARC` of it on the nanometre grid, width 0.
    pub fn shape_nm(&self) -> ShapeArc {
        ShapeArc::new(to_nm(self.start), to_nm(self.mid), to_nm(self.end), 0)
    }

    /// No circle through the three points (they are one point, or in a line): the arc is its chord.
    pub fn is_degenerate(&self) -> bool {
        self.start == self.end || cross(self.start, self.mid, self.end) == 0
    }

    /// The polyline of the arc at the error a `LINE` polygonises an arc to (`SHAPE_LINE_CHAIN::Append( SHAPE_ARC )`: 1 um).
    pub fn polyline(&self) -> Vec<Point> {
        self.polyline_with_error(ARC_POLYGONIZATION_MAX_ERROR_NM)
    }

    /// `SHAPE_ARC::ConvertToPolyline( aMaxError )` in micrometres (`max_error_nm` is KiCad's own unit): the arc's own ends, and between them a point
    /// in the middle of each slice, a little outside the arc. A degenerate arc, or one that is flatter than the error, is its chord.
    pub fn polyline_with_error(&self, max_error_nm: i64) -> Vec<Point> {
        if self.is_degenerate() {
            return if self.start == self.end { vec![self.start] } else { vec![self.start, self.end] };
        }
        let mut out: Vec<Point> = Vec::new();
        for p in self.shape_nm().convert_to_polyline(max_error_nm) {
            let q = to_um(p);
            if out.last() != Some(&q) {
                out.push(q);
            }
        }
        if out.first() != Some(&self.start) {
            out.insert(0, self.start);
        }
        if out.last() != Some(&self.end) {
            out.push(self.end);
        }
        out
    }

    /// The circle's centre, micrometres (`SHAPE_ARC::GetCenter`, rounded to a micrometre).
    pub fn center(&self) -> Point {
        to_um(self.shape_nm().center())
    }

    /// The circle's centre in micrometres, as KiCad computes it (nanometre grid, no rounding to a micrometre).
    pub fn center_f(&self) -> (f64, f64) {
        let c = self.shape_nm().center();
        (c.x as f64 / 1000.0, c.y as f64 / 1000.0)
    }

    /// `SHAPE_ARC::GetRadius`, micrometres.
    pub fn radius(&self) -> f64 {
        self.shape_nm().radius() / 1000.0
    }

    /// `SHAPE_ARC::GetCentralAngle`, degrees; positive counter-clockwise in KiCad's sense, negative the other way, 360 for a circle.
    pub fn central_angle(&self) -> f64 {
        self.shape_nm().central_angle()
    }

    /// `SHAPE_ARC::GetLength`, micrometres.
    pub fn length(&self) -> f64 {
        if self.is_degenerate() {
            return dist(self.start, self.end);
        }
        self.shape_nm().length() / 1000.0
    }

    /// `SHAPE_ARC::GetChord().Length()`, micrometres.
    pub fn chord(&self) -> f64 {
        dist(self.start, self.end)
    }

    /// Is the direction `p` is seen from the centre inside the arc's sweep?
    fn within_sweep(&self, p: Point) -> bool {
        let a = self.shape_nm();
        let c = a.center();
        let (px, py) = (p.x as f64 * 1000.0 - c.x as f64, p.y as f64 * 1000.0 - c.y as f64);
        let ap = normalize(angle_of(px, py));
        let ca = a.central_angle();
        let sa = a.start_angle();
        let t = if ca >= 0.0 { normalize(ap - sa) } else { normalize(sa - ap) };
        t <= ca.abs() + 1e-9
    }

    /// The point of the arc nearest to `p` (rounded to a micrometre): along the line from the centre through `p`, or an end when `p` is out
    /// past the sweep -- what `GRID_HELPER::AlignToArc` snaps the cursor to when a route starts on an arc.
    pub fn nearest_point(&self, p: Point) -> Point {
        if self.is_degenerate() {
            return Seg::new(self.start, self.end).nearest_point(p);
        }
        if self.within_sweep(p) {
            let (cx, cy) = self.center_f();
            let (dx, dy) = (p.x as f64 - cx, p.y as f64 - cy);
            let l = dx.hypot(dy);
            if l > 0.0 {
                let r = self.radius();
                return Point { x: (cx + dx / l * r).round() as Nm, y: (cy + dy / l * r).round() as Nm };
            }
        }
        if dist(p, self.start) <= dist(p, self.end) {
            self.start
        } else {
            self.end
        }
    }

    /// How far `p` is from the arc's centre line, micrometres.
    pub fn distance_to(&self, p: Point) -> f64 {
        if self.is_degenerate() {
            return (Seg::new(self.start, self.end).sq_distance_to_point(p) as f64).sqrt();
        }
        if self.within_sweep(p) {
            let (cx, cy) = self.center_f();
            ((p.x as f64 - cx).hypot(p.y as f64 - cy) - self.radius()).abs()
        } else {
            dist(p, self.start).min(dist(p, self.end))
        }
    }

    /// `LINE_PLACER::SplitAdjacentArcs`: the arc cut in two at `at`, both halves on the same circle (`ConstructFromStartEndCenter( .., o_arc.GetCenter(),
    /// o_arc.IsClockwise() )`), the first from the start to `at` and the second from `at` to the end. `None` for an arc that is no arc, or a
    /// point that is one of its ends.
    pub fn split(&self, at: Point) -> Option<(ArcPts, ArcPts)> {
        if self.is_degenerate() || at == self.start || at == self.end {
            return None;
        }
        let orig = self.shape_nm();
        let (center, clockwise) = (orig.center(), orig.is_clockwise());
        let half = |s: Point, e: Point| -> ArcPts {
            let a = ShapeArc::from_start_end_center(to_nm(s), to_nm(e), center, clockwise);
            ArcPts { start: s, mid: to_um(a.mid), end: e }
        };
        Some((half(self.start, at), half(at, self.end)))
    }

    /// The arc around `center` from `start` to `end` the short way, with the middle `CalcArcMid` finds (what `EDIT_TOOL::DragArcTrack` makes of a
    /// dragged arc).
    pub fn from_start_end_center(start: Point, end: Point, center: Point) -> ArcPts {
        ArcPts { start, mid: calc_arc_mid(start, end, center), end }
    }
}

/// `CalcArcMid( aStart, aEnd, aCenter, aMinArcAngle = true )` (`libs/kimath/src/trigo.cpp:209`): the start turned about the centre by half the angle
/// from the end to it, which is the middle of the shorter arc between the two. Micrometres in and out; the turn is done on the nanometre grid.
pub fn calc_arc_mid(start: Point, end: Point, center: Point) -> Point {
    let (s, e, c) = (to_nm(start), to_nm(end), to_nm(center));
    let start_angle = angle_of((s.x - c.x) as f64, (s.y - c.y) as f64);
    let end_angle = angle_of((e.x - c.x) as f64, (e.y - c.y) as f64);
    // `( startAngle - endAngle ).Normalize180() / 2`
    let mut d = start_angle - end_angle;
    while d <= -180.0 {
        d += 360.0;
    }
    while d > 180.0 {
        d -= 360.0;
    }
    to_um(rotate_about(s, c, d / 2.0))
}

/// The collision shape of an arc track: the stroke of its polyline, fattened by [`ARC_SHAPE_PAD_NM`] so that it holds the stroke of the true arc.
pub fn arc_shape(poly: &[Point], width: Nm) -> Shape {
    Shape::Strokes { segs: poly.windows(2).map(|w| Seg::new(w[0], w[1])).collect(), r: width / 2 + ARC_SHAPE_PAD_NM }
}

/// `ArcHull( aArc, aClearance, aWalkaroundThickness )` (`pns_utils.cpp:68`): the closed ring, clockwise, round an arc track of width `width` that a
/// line of width `walk` must stay outside of to keep `clearance` from it. An arc the line cannot get through (more than half a circle, with a chord
/// shorter than the clearance) -- or one so tight that the inner side of the ring would fold over itself -- is hulled as the circle it is part of.
///
/// KiCad builds it in nanometres from a polygonisation at `ARC_LOW_DEF` (0.02 mm) and pads it by `ARC_HIGH_DEF`; the polygonisation here is held to
/// `ARC_HIGH_DEF` (0.005 mm), so the ring's inner and outer sides are never closer to the curve than the pad allows for.
pub fn arc_hull(arc: &ArcPts, width: Nm, clearance: Nm, walk: Nm) -> Vec<Point> {
    let cl = clearance + (walk + 1) / 2;
    let (cl_nm, width_nm) = (cl * 1000, width * 1000);
    let shape = arc.shape_nm();
    let radius = shape.radius();
    let d = width_nm / 2 + cl_nm + ARC_HIGH_DEF_NM;
    if arc.is_degenerate() {
        return crate::hull::make_clockwise(crate::hull::segment_hull(arc.start, arc.end, width, clearance, walk));
    }
    // "If we can't route through the arc, we might as well treat it as a circle" (plus: the inner offset of a tight arc folds over itself)
    if (shape.central_angle().abs() > 180.0 && arc.chord() * 1000.0 < cl_nm as f64) || radius <= d as f64 {
        let c = shape.center();
        let r = radius as i64;
        let hull_nm = crate::hull::octagonal_hull(Point { x: c.x - r, y: c.y - r }, (2 * r, 2 * r), cl_nm, (2.0 * (1.0 - std::f64::consts::FRAC_1_SQRT_2) * (r + cl_nm) as f64) as i64);
        return crate::hull::make_clockwise(hull_nm.into_iter().map(to_um).collect());
    }
    let x = ((2.0 / (1.0 + std::f64::consts::SQRT_2) * d as f64) as i64) / 2;
    let line = shape.convert_to_polyline(ARC_HIGH_DEF_NM);
    let seg = |i: usize| (line[i], line[i + 1]);
    let n_seg = line.len() - 1;
    let add = |p: Point, v: (i64, i64)| Point { x: p.x + v.0, y: p.y + v.1 };
    let neg = |v: (i64, i64)| (-v.0, -v.1);
    let sum = |u: (i64, i64), v: (i64, i64)| (u.0 + v.0, u.1 + v.1);
    let sub = |u: (i64, i64), v: (i64, i64)| (u.0 - v.0, u.1 - v.1);
    let perp = |s: (Point, Point)| (-(s.1.y - s.0.y), s.1.x - s.0.x);
    let dirv = |s: (Point, Point)| (s.1.x - s.0.x, s.1.y - s.0.y);
    let mut s: Vec<Point> = Vec::with_capacity(2 * n_seg + 8);
    let mut reverse_line: Vec<Point> = Vec::with_capacity(n_seg);

    // the first curve: a rounded cap round the start
    let first = seg(0);
    let (dir, pr) = (dirv(first), perp(first));
    let p0 = neg(crate::hull::resize(pr.0, pr.1, d));
    let ds = neg(crate::hull::resize(pr.0, pr.1, x));
    let pd = crate::hull::resize(dir.0, dir.1, x);
    let dp = crate::hull::resize(dir.0, dir.1, d);
    s.push(add(first.0, sub(p0, pd)));
    s.push(add(first.0, sum(neg(dp), ds)));
    s.push(add(first.0, sub(neg(dp), ds)));
    s.push(add(first.0, sub(neg(p0), pd)));

    for i in 1..n_seg {
        // the vertex normal is the average of the two segment normals: the offset lines of both segments meet
        let (sa, sb) = (seg(i - 1), seg(i));
        let (pp, pp2) = (perp(sa), perp(sb));
        let (pp, pp2) = (crate::hull::resize(pp.0, pp.1, d), crate::hull::resize(pp2.0, pp2.1, d));
        let shifted = |sg: (Point, Point), v: (i64, i64)| (add(sg.0, v), add(sg.1, v));
        let (sa_out, sb_out) = (shifted(sa, pp), shifted(sb, pp2));
        let (sa_in, sb_in) = (shifted(sa, neg(pp)), shifted(sb, neg(pp2)));
        let ip_out = crate::hull::intersect_lines(sa_out, sb_out).unwrap_or(sb_out.0);
        let ip_in = crate::hull::intersect_lines(sa_in, sb_in).unwrap_or(sb_in.0);
        s.push(ip_out);
        reverse_line.push(ip_in);
    }

    // the last curve: a rounded cap round the end
    let last = seg(n_seg - 1);
    let (dir, pr) = (dirv(last), perp(last));
    let p0 = neg(crate::hull::resize(pr.0, pr.1, d));
    let ds = neg(crate::hull::resize(pr.0, pr.1, x));
    let pd = crate::hull::resize(dir.0, dir.1, x);
    let dp = crate::hull::resize(dir.0, dir.1, d);
    s.push(add(last.1, sum(neg(p0), pd)));
    s.push(add(last.1, sub(dp, ds)));
    s.push(add(last.1, sum(dp, ds)));
    s.push(add(last.1, sum(p0, pd)));

    for p in reverse_line.into_iter().rev() {
        s.push(p);
    }
    // "make sure the hull outline is always clockwise": the first side has the arc's start on its inside
    let side = cross(s[0], s[1], line[0]);
    if side < 0 {
        s.reverse();
    }
    crate::hull::make_clockwise(s.into_iter().map(to_um).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i64, y: i64) -> Point {
        Point { x, y }
    }

    /// A quarter circle of radius 5 mm round (0, 0) from (5000, 0) to (0, 5000).
    fn quarter() -> ArcPts {
        let h = (5000.0 * std::f64::consts::FRAC_1_SQRT_2).round() as i64;
        ArcPts::new(p(5000, 0), p(h, h), p(0, 5000))
    }

    #[test]
    fn the_polyline_of_an_arc_starts_and_ends_on_it_and_stays_within_a_micrometre_and_a_half() {
        let a = quarter();
        let pl = a.polyline();
        assert_eq!((pl.first(), pl.last()), (Some(&a.start), Some(&a.end)));
        assert!(pl.len() > 20, "{}", pl.len());
        let c = a.center_f();
        for q in &pl {
            let d = ((q.x as f64 - c.0).powi(2) + (q.y as f64 - c.1).powi(2)).sqrt();
            assert!((d - a.radius()).abs() <= 1.6, "{q:?} is {d} from the centre, radius {}", a.radius());
        }
        // reading it again gives the same points: a line finds its arcs by them
        assert_eq!(pl, a.polyline());
    }

    #[test]
    fn a_straight_or_one_point_arc_is_its_chord() {
        assert_eq!(ArcPts::new(p(0, 0), p(500, 0), p(1000, 0)).polyline(), vec![p(0, 0), p(1000, 0)]);
        assert!(ArcPts::new(p(0, 0), p(500, 0), p(1000, 0)).is_degenerate());
    }

    #[test]
    fn splitting_an_arc_gives_two_arcs_on_its_circle_that_meet_at_the_point() {
        let a = quarter();
        let at = a.nearest_point(p(3000, 3000));
        let (l, r) = a.split(at).expect("a point inside the arc splits it");
        assert_eq!((l.start, l.end, r.start, r.end), (a.start, at, at, a.end));
        let c = a.center_f();
        for half in [l, r] {
            let (hc, hr) = (half.center_f(), half.radius());
            assert!((hc.0 - c.0).abs() < 8.0 && (hc.1 - c.1).abs() < 8.0, "{hc:?} vs {c:?}");
            assert!((hr - 5000.0).abs() < 8.0, "{hr}");
        }
        assert!(a.split(a.start).is_none() && a.split(a.end).is_none());
    }

    #[test]
    fn the_nearest_point_of_an_arc_is_on_the_arc_or_an_end() {
        let a = quarter();
        let n = a.nearest_point(p(6000, 6000));
        assert!(a.distance_to(n) < 1.5, "{n:?}");
        assert!(n.x > 3000 && n.y > 3000, "the middle of the arc: {n:?}");
        // behind the start, off the sweep: the start
        assert_eq!(a.nearest_point(p(5000, -3000)), a.start);
        assert!((a.distance_to(p(0, 0)) - 5000.0).abs() < 1.0);
    }

    #[test]
    fn the_hull_of_an_arc_encloses_the_clearance_round_it_and_not_the_arc_centre() {
        let a = quarter();
        let hull = arc_hull(&a, 250, 200, 250);
        assert!(hull.len() > 8);
        let poly_contains = |q: Point| crate::walkaround::point_in_polygon(&hull, q);
        // points of the arc, and points up to clearance + both half widths either side of it, are inside
        for &t in &[0.05f64, 0.3, 0.5, 0.8, 0.95] {
            let ang = t * std::f64::consts::FRAC_PI_2;
            for off in [-(125.0 + 200.0 + 125.0 - 3.0), 0.0, 125.0 + 200.0 + 125.0 - 3.0] {
                let (x, y) = ((5000.0 + off) * ang.cos(), (5000.0 + off) * ang.sin());
                assert!(poly_contains(p(x.round() as i64, y.round() as i64)), "t {t} off {off}");
            }
        }
        // the middle of the circle is nowhere near
        assert!(!poly_contains(p(0, 0)));
        // and a point a whole millimetre off it is not in it
        assert!(!poly_contains(p((6200.0 * 0.7071) as i64, (6200.0 * 0.7071) as i64)));
        // clockwise like every hull
        let area2: i128 = (0..hull.len()).map(|i| hull[i].x as i128 * hull[(i + 1) % hull.len()].y as i128 - hull[(i + 1) % hull.len()].x as i128 * hull[i].y as i128).sum();
        assert!(area2 > 0, "{area2}");
    }

    #[test]
    fn a_tight_arc_is_hulled_as_its_circle() {
        let tight = ArcPts::new(p(300, 0), p(212, 212), p(0, 300));
        let hull = arc_hull(&tight, 250, 200, 250);
        assert_eq!(hull.len(), 8, "an octagon round the circle");
    }
}
