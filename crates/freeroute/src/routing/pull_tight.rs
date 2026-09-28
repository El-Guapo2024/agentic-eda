//! Pulling traces tight: corners cut and lines moved in to shorten a trace
//! as far as clearances allow, sharp corners smoothed to 45 degrees, and
//! trace ends turned to leave pins the ways their pads allow. Ported from
//! FreeRouting's `PullTightAlgo` and `PullTightAlgo45`, `PolylineTrace.pull_tight`
//! with its pin exit corrections, and `BasicBoard.check_trace_shape`.
//!
//! The Java returns a polyline unchanged as the same object and tests for
//! change by identity; here "unchanged" is `None`.
//!
//! The algorithm keeps the trace it last worked on in its fields -- its
//! layer, half width, nets, class and the pins at its ends -- and some
//! steps read them after a later trace has left them stale; they are kept
//! the same way here.

use crate::geometry::{Direction, FloatPoint, IntOctagon, IntPoint, Line, Point, Polyline, Side, TileShape, CRIT};
use crate::model::{FixedState, ItemKind};

use super::{nets_equal, surrounding_octagon, Pick, RoutingBoard};

/// `PullTightAlgo.c_min_corner_dist_square`.
const MIN_CORNER_DIST_SQUARE: f64 = 0.9;

/// `Limits.sqrt2`.
const SQRT2: f64 = std::f64::consts::SQRT_2;

/// The pull-tight algorithm and the trace it is working on.
/// `PullTightAlgo45`.
pub struct PullTight {
    /// Only traces of exactly these nets are pulled, if any are given.
    only_nets: Vec<i32>,
    clip: Option<IntOctagon>,
    min_translate_dist: i32,
    keep_point: Option<Point>,
    keep_point_layer: i32,
    curr_layer: i32,
    curr_half_width: i64,
    curr_nets: Vec<i32>,
    curr_cl_type: i32,
    contact_pins: Option<Vec<usize>>,
}

/// `Signum.of`.
fn signum(v: f64) -> i32 {
    if v > 0.0 {
        1
    } else if v < 0.0 {
        -1
    } else {
        0
    }
}

/// `Direction.projection`: the sign of the scalar product.
fn projection(a: &Direction, b: &Direction) -> i32 {
    signum(a.x as f64 * b.x as f64 + a.y as f64 * b.y as f64)
}

/// `Line.equals`: the same points, the same way.
fn line_equals(a: &Line, b: &Line) -> bool {
    a.side_of(&Point::Int(b.a)) == Side::Collinear && a.direction().same_as(&b.direction())
}

/// `Point.side_of(Line)`.
fn point_side_of_line(p: &Point, line: &Line) -> Side {
    line.side_of(p).negate()
}

/// `FloatPoint.round`.
fn round(p: (f64, f64)) -> IntPoint {
    FloatPoint::new(p.0, p.1).round()
}

impl PullTight {
    /// `PullTightAlgo.get_instance`, in 45 degree mode.
    pub fn new(only_nets: &[i32], clip: Option<IntOctagon>, min_translate_dist: i32, keep_point: Option<Point>, keep_point_layer: i32) -> Self {
        PullTight {
            only_nets: only_nets.to_vec(),
            clip,
            min_translate_dist: min_translate_dist.max(100),
            keep_point,
            keep_point_layer,
            curr_layer: 0,
            curr_half_width: 0,
            curr_nets: Vec::new(),
            curr_cl_type: 0,
            contact_pins: None,
        }
    }

    fn is_outside_clip(&self, p: &Point) -> bool {
        match &self.clip {
            None => false,
            Some(c) => TileShape::Octagon(*c).is_outside(p),
        }
    }

    fn check(&self, rb: &RoutingBoard, shape: &TileShape) -> bool {
        rb.check_trace_shape(shape, self.curr_layer, &self.curr_nets, self.curr_cl_type, self.contact_pins.as_deref())
    }

    fn join_changed(&self, rb: &mut RoutingBoard, p: FloatPoint) {
        rb.join_changed_area(p, self.curr_layer);
    }

    /// Pull tight the traces in the changed area, layer by layer, until
    /// nothing changes, and move its vias where the trace costs say; the
    /// changed area is used up. `opt_changed_area`.
    pub fn opt_changed_area(&mut self, rb: &mut RoutingBoard, trace_costs: Option<&[(f64, f64)]>) {
        if rb.changed_area.is_none() {
            return;
        }
        let mut something_changed = true;
        while something_changed {
            something_changed = false;
            for layer in 0..rb.board.layer_count() as i32 {
                let changed_region = rb.changed_area.as_ref().expect("a changed area").get_area(layer);
                if changed_region.is_empty() {
                    continue;
                }
                rb.changed_area.as_mut().expect("a changed area").set_empty(layer);
                let offset = 1.5 * (rb.board.rules.max_clearance_on_layer(layer) + 2 * rb.board.rules.max_trace_half_width) as f64;
                let region = TileShape::Octagon(changed_region.offset(offset));
                for item in rb.overlapping_objects(&region, layer) {
                    match rb.board.items[item].kind {
                        ItemKind::Trace { .. } => {
                            if rb.pull_tight_trace(item, self) {
                                something_changed = true;
                                if self.split_traces_at_keep_point(rb) {
                                    break;
                                }
                            } else if self.smoothen_end_corners_at_trace_1(rb, item) {
                                something_changed = true;
                                // Items may have been removed.
                                break;
                            }
                        }
                        ItemKind::Via { .. } if trace_costs.is_some() => {
                            if rb.opt_via_location(item, trace_costs, self.min_translate_dist, 10) {
                                something_changed = true;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Pull `polyline` tight as a trace of the given properties, with
    /// `contact_pins` the pins at its ends; other pins, even of its nets,
    /// are obstacles. `None` if nothing changed. `PullTightAlgo.pull_tight`.
    #[allow(clippy::too_many_arguments)]
    pub fn pull_tight_polyline(&mut self, rb: &mut RoutingBoard, polyline: &Polyline, layer: i32, half_width: i64, nets: &[i32], cl_type: i32, contact_pins: Vec<usize>) -> Option<Polyline> {
        self.curr_layer = layer;
        // The default tree compensates no clearance.
        self.curr_half_width = half_width;
        self.curr_nets = nets.to_vec();
        self.curr_cl_type = cl_type;
        self.contact_pins = Some(contact_pins);
        self.pull_tight(rb, polyline)
    }

    /// `PullTightAlgo45.pull_tight(Polyline)`: cut corners, smooth them,
    /// move lines in, until none of these changes anything.
    fn pull_tight(&mut self, rb: &mut RoutingBoard, polyline: &Polyline) -> Option<Polyline> {
        let mut current: Option<Polyline> = None;
        loop {
            let mut changed = false;
            let prev = current.clone().unwrap_or_else(|| polyline.clone());
            let tmp1 = match self.reduce_corners(rb, &prev) {
                Some(p) => {
                    changed = true;
                    p
                }
                None => prev,
            };
            let tmp2 = match self.smoothen_corners(rb, &tmp1) {
                Some(p) => {
                    changed = true;
                    p
                }
                None => tmp1,
            };
            let new = match self.reposition_lines(rb, &tmp2) {
                Some(p) => {
                    changed = true;
                    p
                }
                None => tmp2,
            };
            if !changed {
                return current;
            }
            current = Some(new);
        }
    }

    /// Remove corners by moving a segment parallel onto its neighbour's
    /// corner, where the moved segment clears everything.
    /// `PullTightAlgo45.reduce_corners`.
    fn reduce_corners(&mut self, rb: &mut RoutingBoard, p: &Polyline) -> Option<Polyline> {
        let n = p.lines.len();
        if n <= 4 {
            return None;
        }
        let mut curr_corner: [IntPoint; 4] = [IntPoint::new(0, 0); 4];
        for (i, c) in curr_corner.iter_mut().enumerate() {
            *c = p.corner(i).as_int()?;
        }
        let mut in_clip = [false; 4];
        for i in 0..4 {
            in_clip[i] = !self.is_outside_clip(&Point::Int(curr_corner[i]));
        }
        let mut polyline_changed = false;
        let mut new_corners: Vec<IntPoint> = Vec::with_capacity(n - 3);
        new_corners.push(curr_corner[0]);
        let mut new_corner = IntPoint::new(0, 0);
        let mut corner_no = 3;
        while corner_no < n - 1 {
            let mut corner_removed = false;
            curr_corner[3] = p.corner(corner_no).as_int()?;
            if curr_corner[1] == curr_corner[2] || (corner_no < n - 2 && curr_corner[3].side_of(curr_corner[1], curr_corner[2]) == Side::Collinear) {
                // A corner in the middle of a line can go.
                corner_no += 1;
                curr_corner[2] = curr_corner[3];
                in_clip[2] = in_clip[3];
                if corner_no < n - 1 {
                    curr_corner[3] = p.corner(corner_no).as_int()?;
                }
                polyline_changed = true;
            }
            in_clip[3] = !self.is_outside_clip(&Point::Int(curr_corner[3]));
            if in_clip[1] && in_clip[2] && in_clip[3] {
                // Move the line from corner 2 to corner 1 onto corner 3.
                let (dx, dy) = (curr_corner[3].x - curr_corner[2].x, curr_corner[3].y - curr_corner[2].y);
                new_corner = IntPoint::new(curr_corner[1].x + dx, curr_corner[1].y + dy);
                if curr_corner[3] == curr_corner[2] {
                    corner_removed = true;
                } else if new_corner.side_of(curr_corner[0], curr_corner[1]) == Side::Collinear {
                    let check = Polyline::from_points(&[new_corner, curr_corner[1]]);
                    if check.lines.len() == 3 {
                        let shape = check.offset_shape(self.curr_half_width, 0).expect("a segment");
                        if self.check(rb, &shape) {
                            if new_corner == curr_corner[3] {
                                corner_removed = true;
                            } else {
                                let check = Polyline::from_points(&[new_corner, curr_corner[3]]);
                                if check.lines.len() == 3 {
                                    let shape = check.offset_shape(self.curr_half_width, 0).expect("a segment");
                                    corner_removed = self.check(rb, &shape);
                                } else {
                                    corner_removed = true;
                                }
                            }
                        }
                    } else {
                        corner_removed = true;
                    }
                }
            }
            if !corner_removed && in_clip[0] && in_clip[1] && in_clip[2] {
                // Else move the line from corner 2 to corner 1 onto corner 0.
                let (dx, dy) = (curr_corner[0].x - curr_corner[1].x, curr_corner[0].y - curr_corner[1].y);
                new_corner = IntPoint::new(curr_corner[2].x + dx, curr_corner[2].y + dy);
                if curr_corner[0] == curr_corner[1] {
                    corner_removed = true;
                } else if new_corner.side_of(curr_corner[2], curr_corner[3]) == Side::Collinear {
                    let check = Polyline::from_points(&[new_corner, curr_corner[0]]);
                    if check.lines.len() == 3 {
                        let shape = check.offset_shape(self.curr_half_width, 0).expect("a segment");
                        if self.check(rb, &shape) {
                            let check = Polyline::from_points(&[new_corner, curr_corner[2]]);
                            if check.lines.len() == 3 {
                                let shape = check.offset_shape(self.curr_half_width, 0).expect("a segment");
                                corner_removed = self.check(rb, &shape);
                            } else {
                                corner_removed = true;
                            }
                        }
                    } else {
                        corner_removed = true;
                    }
                }
            }
            if corner_removed {
                polyline_changed = true;
                curr_corner[1] = new_corner;
                in_clip[1] = !self.is_outside_clip(&Point::Int(curr_corner[1]));
                if rb.changed_area.is_some() {
                    self.join_changed(rb, FloatPoint::from_int(new_corner));
                    self.join_changed(rb, FloatPoint::from_int(curr_corner[1]));
                    self.join_changed(rb, FloatPoint::from_int(curr_corner[2]));
                }
            } else {
                new_corners.push(curr_corner[1]);
                curr_corner[0] = curr_corner[1];
                curr_corner[1] = curr_corner[2];
                in_clip[0] = in_clip[1];
                in_clip[1] = in_clip[2];
            }
            curr_corner[2] = curr_corner[3];
            in_clip[2] = in_clip[3];
            corner_no += 1;
        }
        if !polyline_changed {
            return None;
        }
        new_corners.push(curr_corner[1]);
        new_corners.push(curr_corner[2]);
        Some(Polyline::from_points(&new_corners))
    }

    /// Cut every right or sharper angle between lines at multiples of 45
    /// degrees with a short 45 degree line. `PullTightAlgo45.smoothen_corners`.
    fn smoothen_corners(&mut self, rb: &mut RoutingBoard, p: &Polyline) -> Option<Polyline> {
        let mut result: Option<Polyline> = None;
        let mut polyline_changed = true;
        while polyline_changed {
            let curr = result.as_ref().unwrap_or(p);
            if curr.lines.len() < 4 {
                return result;
            }
            polyline_changed = false;
            let mut line_arr = curr.lines.clone();
            let mut i = 1;
            while i + 2 < line_arr.len() {
                let d1 = line_arr[i].direction();
                let d2 = line_arr[i + 1].direction();
                if d1.is_multiple_of_45_degree() && d2.is_multiple_of_45_degree() && projection(&d1, &d2) != 1 {
                    // A right or sharper angle.
                    let mut new_line = self.smoothen_corner(rb, &line_arr, i);
                    if new_line.is_none() {
                        new_line = self.smoothen_sharp_corner(rb, &line_arr, i);
                    }
                    if let Some(l) = new_line {
                        polyline_changed = true;
                        line_arr.insert(i + 1, l);
                        i += 1;
                    }
                }
                i += 1;
            }
            if polyline_changed {
                result = Some(Polyline::from_lines(&line_arr));
            }
        }
        result
    }

    /// A 45 degree line cutting the corner after line `no` by so little that
    /// no check is needed. `smoothen_sharp_corner`.
    fn smoothen_sharp_corner(&mut self, rb: &mut RoutingBoard, lines: &[Line], no: usize) -> Option<Line> {
        let curr_corner = lines[no].intersection_approx(&lines[no + 1]);
        if curr_corner.0 != (curr_corner.0 as i32) as f64 {
            // Two diagonals meeting off the grid.
            if let Some(l) = self.smoothen_non_integer_corner(lines, no) {
                return Some(l);
            }
        }
        let prev_corner = lines[no].intersection_approx(&lines[no - 1]);
        let next_corner = lines[no + 1].intersection_approx(&lines[no + 2]);
        let prev_dir = lines[no].direction();
        let next_dir = lines[no + 1].direction();
        let new_line_dir = Direction::of(prev_dir.x + next_dir.x, prev_dir.y + next_dir.y);
        let translate_line = Line::through(round(curr_corner), new_line_dir);
        let mut translate_dist = (SQRT2 - 1.0) * self.curr_half_width as f64;
        let prev_dist = translate_line.signed_distance(prev_corner).abs();
        let next_dist = translate_line.signed_distance(next_corner).abs();
        translate_dist = translate_dist.min(prev_dist);
        translate_dist = translate_dist.min(next_dist);
        if translate_dist < 0.99 {
            return None;
        }
        translate_dist = (translate_dist - 1.0).max(1.0);
        if translate_line.side_of_float(next_corner, 0.0) == Side::Left {
            translate_dist = -translate_dist;
        }
        let result = translate_line.translate(translate_dist);
        if rb.changed_area.is_some() {
            self.join_changed(rb, FloatPoint::new(curr_corner.0, curr_corner.1));
        }
        Some(result)
    }

    /// A short axis-parallel line cutting two diagonals' corner off the
    /// grid. `smoothen_non_integer_corner`.
    fn smoothen_non_integer_corner(&self, lines: &[Line], no: usize) -> Option<Line> {
        let prev_line = lines[no];
        let next_line = lines[no + 1];
        if prev_line.is_equal_or_opposite(&next_line) {
            return None;
        }
        let diagonal = |l: &Line| {
            let d = l.direction();
            d.x.abs() == d.y.abs()
        };
        if !(diagonal(&prev_line) && diagonal(&next_line)) {
            return None;
        }
        let curr = prev_line.intersection_approx(&next_line);
        let prev = prev_line.intersection_approx(&lines[no - 1]);
        let next = next_line.intersection_approx(&lines[no + 2]);
        let (new_x, new_y, vertical);
        if prev.0 > curr.0 && next.0 > curr.0 {
            (new_x, new_y, vertical) = (curr.0.ceil() as i64, curr.1.ceil() as i64, true);
        } else if prev.0 < curr.0 && next.0 < curr.0 {
            (new_x, new_y, vertical) = (curr.0.floor() as i64, curr.1.floor() as i64, true);
        } else if prev.1 > curr.1 && next.1 > curr.1 {
            (new_x, new_y, vertical) = (curr.0.ceil() as i64, curr.1.ceil() as i64, false);
        } else if prev.1 < curr.1 && next.1 < curr.1 {
            (new_x, new_y, vertical) = (curr.0.floor() as i64, curr.1.floor() as i64, false);
        } else {
            return None;
        }
        let dir = if vertical {
            if prev.1 < next.1 {
                Direction::UP
            } else {
                Direction::DOWN
            }
        } else if prev.0 < next.0 {
            Direction::RIGHT
        } else {
            Direction::LEFT
        };
        Some(Line::through(IntPoint::new(new_x, new_y), dir))
    }

    /// A 45 degree line cutting the corner after line `no` as far in as
    /// clearances allow, found by halving. `smoothen_corner`.
    fn smoothen_corner(&mut self, rb: &mut RoutingBoard, lines: &[Line], no: usize) -> Option<Line> {
        let prev_corner = lines[no].intersection_approx(&lines[no - 1]);
        let curr_corner = lines[no].intersection_approx(&lines[no + 1]);
        let next_corner = lines[no + 1].intersection_approx(&lines[no + 2]);
        let prev_dir = lines[no].direction();
        let next_dir = lines[no + 1].direction();
        let new_line_dir = Direction::of(prev_dir.x + next_dir.x, prev_dir.y + next_dir.y);
        let translate_line = Line::through(round(curr_corner), new_line_dir);
        let prev_dist = translate_line.signed_distance(prev_corner).abs();
        let next_dist = translate_line.signed_distance(next_corner).abs();
        if prev_dist == 0.0 || next_dist == 0.0 {
            return None;
        }
        let (mut max_translate_dist, nearest_corner) = if prev_dist <= next_dist { (prev_dist, prev_corner) } else { (next_dist, next_corner) };
        if max_translate_dist < 1.0 {
            return None;
        }
        max_translate_dist = (max_translate_dist - 1.0).max(1.0);
        if translate_line.side_of_float(next_corner, 0.0) == Side::Left {
            max_translate_dist = -max_translate_dist;
        }
        let mut check_lines = [lines[no], lines[no], lines[no + 1]];
        let mut translate_dist = max_translate_dist;
        let mut delta_dist = max_translate_dist;
        let side_of_nearest_corner = translate_line.side_of_float(nearest_corner, 0.0);
        let sign = signum(max_translate_dist) as f64;
        let mut result: Option<Line> = None;
        while delta_dist.abs() > self.min_translate_dist as f64 {
            let mut check_ok = false;
            let new_line = translate_line.translate(translate_dist);
            let new_side = new_line.side_of_float(nearest_corner, 0.0);
            if new_side == side_of_nearest_corner || new_side == Side::Collinear {
                check_lines[1] = new_line;
                let tmp = Polyline::from_lines(&check_lines);
                if tmp.lines.len() == 3 {
                    let shape = tmp.offset_shape(self.curr_half_width, 0).expect("a segment");
                    check_ok = self.check(rb, &shape);
                }
                delta_dist /= 2.0;
                if check_ok {
                    result = Some(check_lines[1]);
                    if translate_dist == max_translate_dist {
                        // The biggest possible change.
                        break;
                    }
                    translate_dist += delta_dist;
                } else {
                    translate_dist -= delta_dist;
                }
            } else {
                // Moved a little too far, by rounding.
                let shorten_value = sign * 0.5;
                max_translate_dist -= shorten_value;
                translate_dist -= shorten_value;
                delta_dist -= shorten_value;
            }
        }
        if let Some(r) = result {
            if rb.changed_area.is_some() {
                let a = check_lines[0].intersection_approx(&r);
                let b = check_lines[2].intersection_approx(&r);
                self.join_changed(rb, FloatPoint::new(a.0, a.1));
                self.join_changed(rb, FloatPoint::new(b.0, b.1));
                self.join_changed(rb, FloatPoint::new(curr_corner.0, curr_corner.1));
            }
        }
        result
    }

    /// Move the first inner line that can be moved in to shorten the
    /// polyline. `PullTightAlgo.reposition_lines`.
    fn reposition_lines(&mut self, rb: &mut RoutingBoard, p: &Polyline) -> Option<Polyline> {
        if p.lines.len() < 5 {
            return None;
        }
        for i in 2..p.lines.len() - 2 {
            if let Some(new_line) = self.reposition_line(rb, &p.lines, i) {
                let mut lines = p.lines.clone();
                lines[i] = new_line;
                let result = Polyline::from_lines(&lines);
                return Some(self.skip_segments_of_length_0(rb, &result).unwrap_or(result));
            }
        }
        None
    }

    /// Line `no` moved parallel towards its neighbours' corners as far as
    /// clearances allow, found by halving; `None` if it cannot move.
    /// `PullTightAlgo.reposition_line`.
    fn reposition_line(&mut self, rb: &mut RoutingBoard, lines: &[Line], no: usize) -> Option<Line> {
        if lines.len() - no < 3 {
            return None;
        }
        if self.clip.is_some() {
            for i in [-1i64, 0] {
                let j = (no as i64 + i) as usize;
                let corner = lines[j].intersection(&lines[j + 1]);
                if self.is_outside_clip(&corner) {
                    return None;
                }
            }
        }
        let translate_line = lines[no];
        let prev_corner = lines[no - 2].intersection(&lines[no - 1]);
        let next_corner = lines[no + 1].intersection(&lines[no + 2]);
        let prev_dist = translate_line.signed_distance(prev_corner.to_float());
        let next_dist = translate_line.signed_distance(next_corner.to_float());
        if signum(prev_dist) != signum(next_dist) {
            // The corners lie either side of the line.
            return None;
        }
        let (nearest_point, mut max_translate_dist) = if prev_dist.abs() < next_dist.abs() { (prev_corner, prev_dist) } else { (next_corner, next_dist) };
        let mut translate_dist = max_translate_dist;
        let mut delta_dist = max_translate_dist;
        let side_of_nearest_point = translate_line.side_of(&nearest_point);
        let sign = signum(max_translate_dist) as f64;
        let mut new_line: Option<Line> = None;
        let mut check_lines = [lines[no - 1], lines[no], lines[no + 1]];
        let mut first_time = true;
        while first_time || delta_dist.abs() > self.min_translate_dist as f64 {
            let mut check_ok = false;
            check_lines[1] = match (first_time, nearest_point.as_int()) {
                (true, Some(q)) => Line::through(q, translate_line.direction()),
                _ => translate_line.translate(-translate_dist),
            };
            if line_equals(&check_lines[1], &translate_line) {
                return None;
            }
            let new_side = check_lines[1].side_of(&nearest_point);
            if new_side != side_of_nearest_point && new_side != Side::Collinear {
                // Moved a little too far the first time, by rounding.
                let shorten_value = sign * 0.5;
                max_translate_dist -= shorten_value;
                translate_dist -= shorten_value;
                delta_dist -= shorten_value;
                continue;
            }
            let tmp = Polyline::from_lines(&check_lines);
            if tmp.lines.len() == 3 {
                let shape = tmp.offset_shape(self.curr_half_width, 0).expect("a segment");
                check_ok = self.check(rb, &shape);
            }
            delta_dist /= 2.0;
            if check_ok {
                new_line = Some(check_lines[1]);
                if first_time {
                    // The biggest possible change.
                    break;
                }
                translate_dist += delta_dist;
            } else {
                translate_dist -= delta_dist;
            }
            first_time = false;
        }
        let _ = max_translate_dist;
        if let Some(l) = new_line {
            if rb.changed_area.is_some() {
                for (a, b) in [(check_lines[0], l), (check_lines[2], l), (lines[no - 1], lines[no]), (lines[no], lines[no + 1])] {
                    let p = a.intersection_approx(&b);
                    self.join_changed(rb, FloatPoint::new(p.0, p.1));
                }
            }
        }
        new_line
    }

    /// Drop segments of no length, where that makes no dog ears: checked
    /// unless the line is at a multiple of 45 degrees. The first and last
    /// corners stay exactly where they are.
    /// `PullTightAlgo.skip_segments_of_length_0`.
    fn skip_segments_of_length_0(&mut self, rb: &mut RoutingBoard, p: &Polyline) -> Option<Polyline> {
        let mut changed = false;
        let mut curr = p.clone();
        let mut i = 1;
        while i + 1 < curr.lines.len() {
            let try_skip = if i == 1 || i == curr.lines.len() - 2 {
                curr.corner(i).java_equals(&curr.corner(i - 1))
            } else {
                curr.corner_float(i).distance_square(&curr.corner_float(i - 1)) < MIN_CORNER_DIST_SQUARE
            };
            if try_skip {
                let mut lines = curr.lines.clone();
                lines.remove(i);
                let tmp = Polyline::from_lines(&lines);
                let mut check_ok = tmp.lines.len() == lines.len();
                if check_ok && !curr.lines[i].is_multiple_of_45_degree() {
                    if i > 1 {
                        let shape = tmp.offset_shape(self.curr_half_width, i - 2).expect("a segment");
                        check_ok = self.check(rb, &shape);
                    }
                    if check_ok && i < curr.lines.len() - 2 {
                        let shape = tmp.offset_shape(self.curr_half_width, i - 1).expect("a segment");
                        check_ok = self.check(rb, &shape);
                    }
                }
                if check_ok {
                    changed = true;
                    curr = tmp;
                    i -= 1;
                }
            }
            i += 1;
        }
        changed.then_some(curr)
    }

    /// Smooth sharp angles with the traces at either end of `trace`, again
    /// and again. `smoothen_end_corners_at_trace_1`.
    fn smoothen_end_corners_at_trace_1(&mut self, rb: &mut RoutingBoard, trace: usize) -> bool {
        if rb.board.items[trace].is_shove_fixed(&rb.board.rules) {
            return false;
        }
        let saved_contact_pins = self.contact_pins.take();
        let mut result = false;
        let mut improved = true;
        let mut curr_trace = trace;
        while improved {
            improved = false;
            let Some(adjusted) = self.smoothen_end_corners_at_trace_2(rb, curr_trace) else { continue };
            let (_, layer, _) = rb.trace(curr_trace);
            let it = &rb.board.items[curr_trace];
            let (cl, fixed, nets) = (it.clearance_class, it.fixed, it.nets.clone());
            rb.remove_item(curr_trace);
            let first = adjusted.first_corner();
            let last = adjusted.last_corner();
            if let Some(new_trace) = rb.insert_trace_without_cleaning(adjusted, layer, self.curr_half_width, &nets, cl, fixed) {
                result = true;
                improved = true;
                rb.remove_item(curr_trace);
                curr_trace = new_trace;
                for net in nets {
                    rb.split_traces(&first, layer, net);
                    rb.split_traces(&last, layer, net);
                    let _ = rb.normalize_traces(net);
                    if self.split_traces_at_keep_point(rb) {
                        return true;
                    }
                }
            }
        }
        self.contact_pins = saved_contact_pins;
        result
    }

    /// `smoothen_end_corners_at_trace_2`.
    fn smoothen_end_corners_at_trace_2(&mut self, rb: &mut RoutingBoard, trace: usize) -> Option<Polyline> {
        if !rb.is_on_board(trace) {
            return None;
        }
        let mut result = self.smoothen_start_corner_at_trace(rb, trace);
        match &result {
            None => {
                result = self.smoothen_end_corner_at_trace(rb, trace);
                if let Some(r) = &result {
                    if rb.changed_area.is_some() {
                        let p = r.corner_float(r.corner_count() - 1);
                        self.join_changed(rb, p);
                    }
                }
            }
            Some(r) => {
                if rb.changed_area.is_some() {
                    let p = r.corner_float(0);
                    self.join_changed(rb, p);
                }
            }
        }
        let r = result?;
        self.contact_pins = Some(rb.touching_pins_at_end_corners(trace));
        Some(self.skip_segments_of_length_0(rb, &r).unwrap_or(r))
    }

    /// Smooth an acute angle or a bend with the trace at the start of
    /// `trace`. `PullTightAlgo45.smoothen_start_corner_at_trace`.
    fn smoothen_start_corner_at_trace(&mut self, rb: &mut RoutingBoard, trace: usize) -> Option<Polyline> {
        self.smoothen_corner_at_trace(rb, trace, true)
    }

    /// `PullTightAlgo45.smoothen_end_corner_at_trace`.
    fn smoothen_end_corner_at_trace(&mut self, rb: &mut RoutingBoard, trace: usize) -> Option<Polyline> {
        self.smoothen_corner_at_trace(rb, trace, false)
    }

    /// Both ends' smoothing, which the Java writes out twice.
    fn smoothen_corner_at_trace(&mut self, rb: &mut RoutingBoard, trace: usize, at_start: bool) -> Option<Polyline> {
        let trace_polyline = rb.trace(trace).0.clone();
        let arr = &trace_polyline.lines;
        let n = arr.len();
        let curr_end_corner = if at_start { trace_polyline.corner(0) } else { trace_polyline.last_corner() };
        if self.is_outside_clip(&curr_end_corner) {
            return None;
        }
        let curr_prev_end_corner = if at_start { trace_polyline.corner(1) } else { trace_polyline.corner(trace_polyline.corner_count() - 2) };
        let (line_direction, prev_line_direction) = if at_start {
            (arr[1].direction(), arr[2].direction())
        } else {
            (arr[n - 2].direction().opposite(), arr[n - 3].direction().opposite())
        };
        let contacts = if at_start { rb.start_contacts(trace) } else { rb.end_contacts(trace) };
        let mut acute_angle = false;
        let mut bend = false;
        let mut other_trace_corner_approx = (0.0, 0.0);
        let mut other_trace_line: Option<Line> = None;
        let mut other_prev_trace_line: Option<Line> = None;
        let mut prev_corner_side = Side::Collinear;
        for c in contacts {
            if !rb.is_trace(c) || rb.board.items[c].is_shove_fixed(&rb.board.rules) {
                return None;
            }
            let contact_polyline = &rb.trace(c).0;
            let (corner_approx, line, prev_line) = if contact_polyline.first_corner().java_equals(&curr_end_corner) {
                (contact_polyline.corner_approx(1), contact_polyline.lines[1], contact_polyline.lines[2])
            } else {
                let k = contact_polyline.corner_count() - 2;
                (contact_polyline.corner_approx(k), contact_polyline.lines[k + 1].opposite(), contact_polyline.lines[k])
            };
            let side = point_side_of_line(&curr_prev_end_corner, &line);
            let proj = projection(&line_direction, &line.direction());
            let mut found = false;
            if proj == 1 && side != Side::Collinear {
                if line.direction().is_orthogonal() {
                    acute_angle = true;
                    found = true;
                }
            } else if proj == 0 && trace_polyline.corner_count() > 2 && projection(&prev_line_direction, &line.direction()) == 1 {
                bend = true;
                found = true;
            }
            if found {
                other_trace_corner_approx = corner_approx;
                other_trace_line = Some(line);
                prev_corner_side = side;
                other_prev_trace_line = Some(prev_line);
            }
        }
        if acute_angle {
            let other_line = other_trace_line.expect("set with the angle");
            let turn = if (prev_corner_side == Side::Left) == at_start { 2 } else { 6 };
            let new_line_dir = other_line.direction().turn_45_degree(turn);
            let translate_line = Line::through(round(curr_end_corner.to_float()), new_line_dir);
            let mut translate_dist = (SQRT2 - 1.0) * self.curr_half_width as f64;
            let prev_corner_dist = translate_line.signed_distance(curr_prev_end_corner.to_float()).abs();
            let other_dist = translate_line.signed_distance(other_trace_corner_approx).abs();
            translate_dist = translate_dist.min(prev_corner_dist);
            translate_dist = translate_dist.min(other_dist);
            if translate_dist >= 0.99 {
                translate_dist = (translate_dist - 1.0).max(1.0);
                if translate_line.side_of(&curr_prev_end_corner) == Side::Left {
                    translate_dist = -translate_dist;
                }
                let add_line = translate_line.translate(translate_dist);
                let new_lines: Vec<Line> = if at_start {
                    let mut v = vec![other_line, add_line];
                    v.extend_from_slice(&arr[1..]);
                    v
                } else {
                    let mut v = arr[..n - 1].to_vec();
                    v.push(add_line);
                    v.push(other_line);
                    v
                };
                return Some(Polyline::from_lines(&new_lines));
            }
        } else if bend {
            let other_line = other_trace_line.expect("set with the bend");
            let other_prev = other_prev_trace_line.expect("set with the bend");
            if at_start {
                let mut check = vec![other_prev, other_line];
                check.extend_from_slice(&arr[1..]);
                if let Some(new_line) = self.reposition_line(rb, &check, 2) {
                    let mut new_lines = vec![other_line, new_line];
                    new_lines.extend_from_slice(&arr[2..]);
                    return Some(Polyline::from_lines(&new_lines));
                }
            } else {
                let mut check = arr[..n - 1].to_vec();
                check.push(other_line);
                check.push(other_prev);
                if let Some(new_line) = self.reposition_line(rb, &check, n - 2) {
                    let mut new_lines = arr[..n - 2].to_vec();
                    new_lines.push(new_line);
                    new_lines.push(other_line);
                    return Some(Polyline::from_lines(&new_lines));
                }
            }
        }
        None
    }

    /// Split the traces through the keep point, if there is one: true if
    /// one was split. `split_traces_at_keep_point`.
    pub fn split_traces_at_keep_point(&self, rb: &mut RoutingBoard) -> bool {
        let Some(p) = self.keep_point else { return false };
        for i in rb.pick_items(&p, self.keep_point_layer, Pick::Traces) {
            if rb.split_at_point(i, &p).is_some() {
                return true;
            }
        }
        false
    }
}

impl RoutingBoard {
    /// Whether a trace piece of shape `shape` fits on `layer` as it is. With
    /// `contact_pins`, every other pin is an obstacle, even of the trace's
    /// nets. `BasicBoard.check_trace_shape`.
    pub fn check_trace_shape(&self, shape: &TileShape, layer: i32, nets: &[i32], cl_class: i32, contact_pins: Option<&[usize]>) -> bool {
        if !shape.is_contained_in_box(&self.board.bounds) {
            return false;
        }
        for (i, _) in self.overlapping_entries_with_clearance(shape, layer, &[], cl_class) {
            let item = &self.board.items[i];
            if let Some(pins) = contact_pins {
                if pins.contains(&i) {
                    continue;
                }
                if matches!(item.kind, ItemKind::Pin { .. }) {
                    // Other pins are obstacles, against acid traps.
                    return false;
                }
            }
            let mut is_obstacle = true;
            for &net in nets {
                if !item.is_trace_obstacle(net) {
                    is_obstacle = false;
                }
            }
            if is_obstacle && self.is_trace(i) {
                if let Some(pins) = contact_pins {
                    if pins.iter().any(|&p| self.board.items[p].nets.len() > 1 && self.board.items[p].shares_net(item)) {
                        unimplemented!("traces of another net at a tie pin (check_trace_shape) are not ported yet");
                    }
                }
            }
            if is_obstacle {
                return false;
            }
        }
        true
    }

    /// Whether a trace along `polyline` fits as it is.
    /// `BasicBoard.check_polyline_trace`, whose temporary trace draws an
    /// item number.
    pub fn check_polyline_trace(&mut self, polyline: &Polyline, layer: i32, half_width: i64, nets: &[i32], cl_class: i32) -> bool {
        self.skip_id();
        let contact_pins = self.touching_pins_at_polyline_ends(polyline, layer, half_width, nets, cl_class);
        for shape in polyline.offset_shapes(half_width, 0, polyline.lines.len().saturating_sub(1)) {
            if !self.check_trace_shape(&shape, layer, nets, cl_class, Some(&contact_pins)) {
                return false;
            }
        }
        true
    }

    /// The pins of `nets` within `half_width` of either end of `polyline`,
    /// clearance included. `Trace.touching_pins_at_end_corners`.
    pub fn touching_pins_at_polyline_ends(&self, polyline: &Polyline, layer: i32, half_width: i64, nets: &[i32], cl_class: i32) -> Vec<usize> {
        let mut result = Vec::new();
        for p in [polyline.first_corner(), polyline.last_corner()] {
            let Some(o) = surrounding_octagon(&p) else { continue };
            let shape = TileShape::Octagon(o.offset(half_width as f64));
            for i in self.overlapping_items_with_clearance(&shape, layer, &[], cl_class) {
                let other = &self.board.items[i];
                if matches!(other.kind, ItemKind::Pin { .. }) && other.shares_net_no(nets) {
                    result.push(i);
                }
            }
        }
        self.sort_items(&mut result);
        result
    }

    /// Insert a trace and normalize it within the changed area.
    /// `BasicBoard.insert_trace(Polyline, ...)`.
    pub fn insert_trace(&mut self, polyline: Polyline, layer: i32, half_width: i64, nets: &[i32], cl_class: i32, fixed: FixedState) {
        let Some(new_trace) = self.insert_trace_without_cleaning(polyline, layer, half_width, nets, cl_class, fixed) else { return };
        let clip = self.changed_area_on(layer);
        let _ = self.normalize(new_trace, clip.as_ref());
    }

    /// Shorten the trace as clearances allow, then fix how its ends leave
    /// pins: true if it changed. `PolylineTrace.pull_tight(PullTightAlgo)`.
    pub fn pull_tight_trace(&mut self, item: usize, algo: &mut PullTight) -> bool {
        if !self.is_on_board(item) {
            return false;
        }
        let it = &self.board.items[item];
        if it.is_shove_fixed(&self.board.rules) || !self.nets_normal(&it.nets) {
            return false;
        }
        if !algo.only_nets.is_empty() && !nets_equal(&it.nets, &algo.only_nets) {
            return false;
        }
        if let Some(&net) = it.nets.first() {
            if !self.board.rules.net_class(net).is_some_and(|c| c.pull_tight) {
                return false;
            }
        }
        let (polyline, layer, half_width) = self.trace(item);
        let polyline = polyline.clone();
        let (nets, cl) = (it.nets.clone(), it.clearance_class);
        let contact_pins = self.touching_pins_at_end_corners(item);
        if let Some(new_lines) = algo.pull_tight_polyline(self, &polyline, layer, half_width, &nets, cl, contact_pins) {
            self.change(item, new_lines);
            return true;
        }
        if self.board.rules.pin_edge_to_turn_dist > 0.0 {
            for at_start in [true, false] {
                if self.swap_connection_to_pin(item, at_start) {
                    self.pull_tight_trace(item, algo);
                    return true;
                }
            }
            // The trace could not be improved: remove acid traps.
            for at_start in [true, false] {
                if self.correct_connection_to_pin(item, at_start) {
                    self.pull_tight_trace(item, algo);
                    return true;
                }
            }
        }
        false
    }

    /// The first pin among the trace's contacts at one end.
    fn contact_pin(&self, item: usize, at_start: bool) -> Option<usize> {
        let contacts = if at_start { self.start_contacts(item) } else { self.end_contacts(item) };
        contacts.into_iter().find(|&c| matches!(self.board.items[c].kind, ItemKind::Pin { .. }))
    }

    /// A pin's exit rules and pad on `layer`, if the pad is not round.
    fn pin_exits(&self, pin: usize, layer: i32) -> Option<(IntPoint, TileShape, Vec<crate::model::ExitRestriction>)> {
        let p = &self.board.items[pin];
        let ItemKind::Pin { center, pads, exits, .. } = &p.kind else { return None };
        let index = (layer - p.first_layer) as usize;
        let exits = exits.get(index)?.clone();
        if exits.is_empty() {
            return None;
        }
        let shape = match pads.get(index)?.as_ref()? {
            crate::board::PadShape::Box(b) => TileShape::Box(*b),
            crate::board::PadShape::Octagon(o) => TileShape::Octagon(*o),
            crate::board::PadShape::Polygon(s) => TileShape::Simplex(s.clone()),
            crate::board::PadShape::Circle(_) => return None,
        };
        Some((*center, shape, exits))
    }

    /// Whether the trace leaves a pin at one end one of the ways the pin
    /// allows, straight far enough. `PolylineTrace.check_connection_to_pin`.
    fn check_connection_to_pin(&self, item: usize, at_start: bool) -> bool {
        let (polyline, layer, half_width) = self.trace(item);
        if polyline.corner_count() < 2 {
            return true;
        }
        let Some(pin) = self.contact_pin(item, at_start) else { return true };
        let ItemKind::Pin { exits, .. } = &self.board.items[pin].kind else { return true };
        let exits = &exits[(layer - self.board.items[pin].first_layer) as usize];
        if exits.is_empty() {
            return true;
        }
        let (end_corner, prev_end_corner) = if at_start { (polyline.first_corner(), polyline.corner(1)) } else { (polyline.last_corner(), polyline.corner(polyline.corner_count() - 2)) };
        // Direction.get_instance(end, prev), exact for corners off the grid
        // too; a direction too fine for an int one is no exit's.
        let homogeneous = |p: &Point| match p {
            Point::Int(q) => (q.x as i128, q.y as i128, 1i128),
            Point::Rational(r) => (r.x, r.y, r.z),
        };
        let ((ex, ey, ez), (px, py, pz)) = (homogeneous(&end_corner), homogeneous(&prev_end_corner));
        let (mut dx, mut dy) = (px * ez - ex * pz, py * ez - ey * pz);
        if pz * ez < 0 {
            (dx, dy) = (-dx, -dy);
        }
        if dx == 0 && dy == 0 {
            return true;
        }
        let g = gcd_i128(dx.unsigned_abs(), dy.unsigned_abs()) as i128;
        (dx, dy) = (dx / g, dy / g);
        if dx.abs() > CRIT as i128 || dy.abs() > CRIT as i128 {
            return false;
        }
        let trace_end_direction = Direction::of(dx as i64, dy as i64);
        let Some(matching) = exits.iter().find(|x| x.direction.same_as(&trace_end_direction)) else { return false };
        let edge_to_turn_dist = self.board.rules.pin_edge_to_turn_dist;
        if edge_to_turn_dist < 0.0 {
            return false;
        }
        let end_line_length = FloatPoint::from_point(&end_corner).distance(&FloatPoint::from_point(&prev_end_corner));
        let clearance = self.clearance_value(self.board.items[item].clearance_class, self.board.items[pin].clearance_class, layer);
        let add_width = edge_to_turn_dist.max(clearance as f64 + 1.0);
        let preserve_length = matching.min_length + half_width as f64 + add_width;
        preserve_length <= end_line_length
    }

    /// Where the trace leaves a pin at one end the wrong way, bend its end
    /// round the pad to the nearest allowed exit and fix the exit stub
    /// against pushing: true if changed.
    /// `PolylineTrace.correct_connection_to_pin`.
    fn correct_connection_to_pin(&mut self, item: usize, at_start: bool) -> bool {
        if self.check_connection_to_pin(item, at_start) {
            return false;
        }
        let (polyline, layer, half_width) = self.trace(item);
        let trace_polyline = if at_start { polyline.clone() } else { polyline.reverse() };
        let Some(pin) = self.contact_pin(item, at_start) else { return false };
        let Some((pin_center, pin_shape, exits)) = self.pin_exits(pin, layer) else { return false };
        let edge_to_turn_dist = self.board.rules.pin_edge_to_turn_dist;
        if edge_to_turn_dist < 0.0 {
            return false;
        }
        let it = &self.board.items[item];
        let (nets, cl) = (it.nets.clone(), it.clearance_class);
        let clearance = self.clearance_value(cl, self.board.items[pin].clearance_class, layer);
        let add_width = edge_to_turn_dist.max(clearance as f64 + 1.0);
        let offset_pin_shape = pin_shape.offset(half_width as f64 + add_width);
        let offset_pin_shape = if offset_pin_shape.is_int_box() {
            TileShape::Box(offset_pin_shape.bounding_box())
        } else {
            TileShape::Octagon(offset_pin_shape.bounding_octagon().expect("a bounded pad"))
        };
        let entries = offset_pin_shape.entrance_points(&trace_polyline);
        let Some(&(entry_line, entry_side)) = entries.last() else { return false };
        let entry = trace_polyline.lines[entry_line].intersection_approx(&offset_pin_shape.border_line(entry_side));
        let trace_entry_location = FloatPoint::new(entry.0, entry.1);
        let (exit_ray, exit_border_line_no, exit_direction) = nearest_exit(&offset_pin_shape, pin_center, &exits, &trace_entry_location, &trace_polyline);
        // The border of the grown pad from the exit round to the entry.
        let corner_count = offset_pin_shape.border_line_count() as i64;
        let (nb, es) = (exit_border_line_no as i64, entry_side as i64);
        let clockwise_diff = (nb - es + corner_count) % corner_count;
        let counter_clockwise_diff = (es - nb + corner_count) % corner_count;
        let mut curr_lines: Vec<Line> = vec![exit_ray];
        let mut curr_no = nb;
        if counter_clockwise_diff <= clockwise_diff {
            for _ in 0..=counter_clockwise_diff {
                curr_lines.push(offset_pin_shape.border_line(curr_no as usize));
                curr_no = (curr_no + 1) % corner_count;
            }
        } else {
            for _ in 0..=clockwise_diff {
                curr_lines.push(offset_pin_shape.border_line(curr_no as usize));
                curr_no = (curr_no - 1 + corner_count) % corner_count;
            }
        }
        curr_lines.push(trace_polyline.lines[entry_line]);
        let border_polyline = Polyline::from_lines(&curr_lines);
        if !self.check_polyline_trace(&border_polyline, layer, half_width, &nets, cl) {
            return false;
        }
        let mut cut_lines = vec![curr_lines[curr_lines.len() - 2]];
        cut_lines.extend_from_slice(&trace_polyline.lines[entry_line..]);
        let cut_polyline = Polyline::from_lines(&cut_lines);
        let mut changed = if cut_polyline.first_corner().java_equals(&cut_polyline.last_corner()) {
            border_polyline.clone()
        } else {
            border_polyline.combine(&cut_polyline).unwrap_or_else(|| border_polyline.clone())
        };
        if !at_start {
            changed = changed.reverse();
        }
        self.change(item, changed);
        // A shove-fixed exit stub.
        let stub = Polyline::from_lines(&[Line::through(pin_center, exit_direction.turn_45_degree(2)), exit_ray, offset_pin_shape.border_line(exit_border_line_no)]);
        self.insert_trace(stub, layer, half_width, &nets, cl, FixedState::ShoveFixed);
        true
    }

    /// Where the trace meets a shove-fixed exit stub at a sharp angle, let
    /// the stub go if another exit of the pin fits better: true if changed.
    /// `PolylineTrace.swap_connection_to_pin`.
    fn swap_connection_to_pin(&mut self, item: usize, at_start: bool) -> bool {
        let (polyline, layer, half_width) = self.trace(item);
        let trace_polyline = if at_start { polyline.clone() } else { polyline.reverse() };
        let contacts = if at_start { self.start_contacts(item) } else { self.end_contacts(item) };
        if contacts.len() != 1 {
            return false;
        }
        let contact = contacts[0];
        if !(self.board.items[contact].fixed == FixedState::ShoveFixed && self.is_trace(contact)) {
            return false;
        }
        let contact_polyline = self.trace(contact).0.clone();
        let contact_last_line = contact_polyline.lines[contact_polyline.lines.len() - 2];
        let first_line = trace_polyline.lines[1];
        let mut check_swap = projection(&contact_last_line.direction(), &first_line.direction()) == -1;
        if !check_swap {
            let hw = half_width as f64;
            if trace_polyline.lines.len() > 3 && trace_polyline.corner_float(0).distance_square(&trace_polyline.corner_float(1)) <= hw * hw {
                check_swap = projection(&contact_last_line.direction(), &trace_polyline.lines[2].direction()) == -1;
            }
        }
        if !check_swap {
            return false;
        }
        let Some(pin) = self.contact_pin(contact, true) else { return false };
        let combined = contact_polyline.combine(&trace_polyline).unwrap_or_else(|| contact_polyline.clone());
        let Some(direction) = self.nearest_exit_restriction_direction(pin, &combined, half_width, layer) else { return false };
        if direction.same_as(&contact_polyline.lines[1].direction()) {
            return false;
        }
        self.board.items[contact].fixed = self.board.items[item].fixed;
        self.combine(item);
        true
    }

    /// The exit direction of the pin nearest where `polyline`, starting at
    /// its centre, leaves the grown pad. `Pin.calc_nearest_exit_restriction_direction`.
    fn nearest_exit_restriction_direction(&self, pin: usize, polyline: &Polyline, half_width: i64, layer: i32) -> Option<Direction> {
        let (pin_center, pin_shape, exits) = self.pin_exits(pin, layer)?;
        let edge_to_turn_dist = self.board.rules.pin_edge_to_turn_dist;
        if edge_to_turn_dist < 0.0 {
            return None;
        }
        let offset_pin_shape = pin_shape.offset(edge_to_turn_dist + half_width as f64);
        let entries = offset_pin_shape.entrance_points(polyline);
        let &(entry_line, entry_side) = entries.last()?;
        let entry = polyline.lines[entry_line].intersection_approx(&offset_pin_shape.border_line(entry_side));
        let (_, _, direction) = nearest_exit(&offset_pin_shape, pin_center, &exits, &FloatPoint::new(entry.0, entry.1), polyline);
        Some(direction)
    }
}

/// The pin exit nearest `entry`, near ties going to the one nearer the
/// polyline's corners: the ray out of the centre, the grown pad's border
/// line it crosses, and its direction.
fn nearest_exit(offset_pin_shape: &TileShape, pin_center: IntPoint, exits: &[crate::model::ExitRestriction], entry: &FloatPoint, polyline: &Polyline) -> (Line, usize, Direction) {
    const TOLERANCE: f64 = 1.0;
    let mut min_distance = f64::MAX;
    let mut best: Option<(Line, usize, Direction, FloatPoint)> = None;
    for exit in exits {
        let border_line_no = offset_pin_shape
            .intersecting_border_line_no(&pin_center, &exit.direction)
            .unwrap_or_else(|| panic!("no border line of the grown pad crosses the exit (the Java fails here)"));
        let ray = Line::through(pin_center, exit.direction);
        let c = ray.intersection_approx(&offset_pin_shape.border_line(border_line_no));
        let corner = FloatPoint::new(c.0, c.1);
        let distance = corner.distance_square(entry);
        let mut nearer = false;
        if distance + TOLERANCE < min_distance {
            nearer = true;
        } else if distance < min_distance + TOLERANCE {
            let old = best.as_ref().expect("a nearest corner").3;
            for i in 1..polyline.corner_count() {
                let trace_corner = polyline.corner_float(i);
                let curr = trace_corner.distance_square(&corner);
                let prev = trace_corner.distance_square(&old);
                if curr + TOLERANCE < prev {
                    nearer = true;
                    break;
                } else if curr > prev + TOLERANCE {
                    break;
                }
            }
        }
        if nearer {
            min_distance = distance;
            best = Some((ray, border_line_no, exit.direction, corner));
        }
    }
    let (ray, no, direction, _) = best.expect("an exit");
    (ray, no, direction)
}

/// The greatest common divisor, for exact directions.
fn gcd_i128(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}
