//! Putting a found connection on the board: trace by trace and segment by
//! segment, pushing aside what can be pushed, wrapping round what cannot,
//! necking down at pins where the full width does not fit, and cleaning
//! up after. Ported from FreeRouting's `InsertFoundConnectionAlgo` and
//! `RoutingBoard.insert_forced_trace_polyline`, `insert_forced_trace_segment`
//! and `check_trace_segment`.
//!
//! The Java tells how far an insertion got by returning one of the points
//! it was given, compared by identity; [`Reached`] names which.

use crate::autoroute::locate::{fortyfive_degree_corner, Located, LocatedTrace};
use crate::autoroute::Control;
use crate::geometry::{FloatPoint, IntPoint, LineSegment, Point, Polyline};
use crate::model::{FixedState, ItemKind};

use super::pull_tight::PullTight;
use super::shove::{FromSide, Spring};
use super::{nets_equal, surrounding_octagon, Pick, RoutingBoard};

/// How far an insertion from one point towards another got: the Java's
/// point returned, by identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reached {
    /// Not past the first point.
    From,
    /// All the way.
    To,
    /// Up to another point short of the end.
    Other(IntPoint),
    /// The push failed after its check passed: the board may be damaged.
    Failed,
}

/// The shove limits and trace settings an insertion runs with, from the
/// autoroute control.
#[derive(Debug, Clone, Copy)]
pub struct ShoveLimits {
    pub max_recursion_depth: i32,
    pub max_via_recursion_depth: i32,
    pub max_spring_over_recursion_depth: i32,
    pub pull_tight_accuracy: i32,
}

impl ShoveLimits {
    pub fn of(ctrl: &Control) -> Self {
        ShoveLimits {
            max_recursion_depth: ctrl.max_shove_trace_recursion_depth,
            max_via_recursion_depth: ctrl.max_shove_via_recursion_depth,
            max_spring_over_recursion_depth: ctrl.max_spring_over_recursion_depth,
            pull_tight_accuracy: ctrl.pull_tight_accuracy,
        }
    }
}

impl RoutingBoard {
    /// Insert a trace along `polyline`, pushing aside what is in the way,
    /// and pull it tight within `tidy_width` of where it ends (everywhere
    /// for `i32::MAX`). How far it got. `insert_forced_trace_polyline`.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_forced_trace_polyline(&mut self, polyline: &Polyline, half_width: i64, layer: i32, nets: &[i32], cl_class: i32, limits: ShoveLimits, tidy_width: i32, with_check: bool) -> Reached {
        let from_corner = polyline.first_corner();
        let to_corner = polyline.last_corner();
        if from_corner.java_equals(&to_corner) {
            return Reached::To;
        }
        if from_corner.as_int().is_none() || to_corner.as_int().is_none() {
            return Reached::From;
        }
        self.start_marking_changed_area();
        // A trace of the net ending where this starts trims the dog ears of
        // the check shapes.
        let mut picked_trace = None;
        let picked = self.pick_items(&from_corner, layer, Pick::Traces);
        if picked.len() == 1 {
            let t = picked[0];
            let item = &self.board.items[t];
            if nets_equal(&item.nets, nets) && self.trace(t).2 == half_width && item.clearance_class == cl_class {
                picked_trace = Some(t);
            }
        }
        // The default tree compensates no clearance.
        let compensated_half_width = half_width;
        let mut new_polyline = match self.spring_over_obstacles(polyline, compensated_half_width, layer, nets, cl_class, None) {
            Spring::Fail => return Reached::From,
            Spring::Same => polyline.clone(),
            Spring::New(p) => p,
        };
        let combine = |rb: &RoutingBoard, p: &Polyline| match picked_trace {
            None => p.clone(),
            Some(t) => p.combine(rb.trace(t).0).unwrap_or_else(|| p.clone()),
        };
        let mut combined = combine(self, &new_polyline);
        if combined.lines.len() < 3 {
            return Reached::From;
        }
        let start_shape_no = combined.lines.len() - new_polyline.lines.len();
        let trace_shapes = combined.offset_shapes(compensated_half_width, start_shape_no, combined.lines.len() - 1);
        let mut last_shape_no = trace_shapes.len();
        for (i, shape) in trace_shapes.iter().enumerate() {
            let from_side = FromSide::of_polyline(&combined, combined.corner_count() - trace_shapes.len() - 1 + i, shape);
            if with_check && !self.shove_check(shape, Some(from_side), None, layer, nets, cl_class, limits.max_recursion_depth, limits.max_via_recursion_depth, limits.max_spring_over_recursion_depth) {
                last_shape_no = i;
                break;
            }
            if !self.shove_insert(shape, Some(from_side), layer, nets, cl_class, limits.max_recursion_depth, limits.max_via_recursion_depth, limits.max_spring_over_recursion_depth) {
                return Reached::Failed;
            }
        }
        let mut new_corner = Reached::To;
        if last_shape_no < trace_shapes.len() {
            // The push of shape `last_shape_no` failed: try it shortened.
            let mut last_trace_shape = trace_shapes[last_shape_no].clone();
            let sample_width = 2 * self.min_trace_half_width;
            let last_corner = new_polyline.corner_float(last_shape_no + 1);
            let prev_last_corner = new_polyline.corner_float(last_shape_no);
            let last_segment_length = last_corner.distance(&prev_last_corner);
            if last_segment_length > 100.0 * sample_width as f64 {
                // Too many steps to sample.
                return Reached::From;
            }
            let mut shape_index = combined.corner_count() - trace_shapes.len() - 1 + last_shape_no;
            if last_segment_length > sample_width as f64 {
                new_polyline = new_polyline.shorten(new_polyline.lines.len() - (trace_shapes.len() - last_shape_no - 1), sample_width as f64);
                let Some(curr_last_corner) = new_polyline.last_corner().as_int() else { return Reached::From };
                new_corner = Reached::Other(curr_last_corner);
                combined = combine(self, &new_polyline);
                if combined.lines.len() < 3 {
                    return new_corner;
                }
                shape_index = combined.lines.len() - 3;
                last_trace_shape = combined.offset_shape(compensated_half_width, shape_index).expect("a last segment");
            }
            let from_side = FromSide::of_polyline(&combined, shape_index, &last_trace_shape);
            if !self.shove_check(&last_trace_shape, Some(from_side), None, layer, nets, cl_class, limits.max_recursion_depth, limits.max_via_recursion_depth, limits.max_spring_over_recursion_depth) {
                return Reached::From;
            }
            if !self.shove_insert(&last_trace_shape, Some(from_side), layer, nets, cl_class, limits.max_recursion_depth, limits.max_via_recursion_depth, limits.max_spring_over_recursion_depth) {
                return Reached::Failed;
            }
        }
        for i in 0..new_polyline.corner_count() {
            let p = new_polyline.corner_float(i);
            self.join_changed_area(p, layer);
        }
        let Some(mut new_trace) = self.insert_trace_without_cleaning(new_polyline, layer, half_width, nets, cl_class, FixedState::Unfixed) else {
            // The Java fails on the missing trace here.
            panic!("insert_forced_trace_polyline: the new trace could not be inserted (the Java fails here)");
        };
        self.combine(new_trace);
        let new_corner_point = match new_corner {
            Reached::Other(p) => Point::Int(p),
            _ => to_corner,
        };
        let tidy_region = if tidy_width < i32::MAX { surrounding_octagon(&new_corner_point).map(|o| o.offset(tidy_width as f64)) } else { None };
        let opt_nets: Vec<i32> = if limits.max_recursion_depth <= 0 { nets.to_vec() } else { Vec::new() };
        let mut algo = PullTight::new(&opt_nets, tidy_region, limits.pull_tight_accuracy, Some(new_corner_point), layer);
        // Remove cycles made, or pulling tight may not work.
        let clip = self.changed_area_on(layer).expect("a changed area");
        let mut new_trace_opt = Some(new_trace);
        match self.normalize(new_trace, Some(&clip)) {
            Ok(true) => {
                algo.split_traces_at_keep_point(self);
                // Else the new corner may not be on the trace after pulling.
                new_trace_opt = self.pick_items(&new_corner_point, layer, Pick::Traces).first().copied();
            }
            Ok(false) => {}
            Err(_) => {}
        }
        if tidy_width > 0 {
            if let Some(t) = new_trace_opt {
                new_trace = t;
                self.pull_tight_trace(new_trace, &mut algo);
            }
        }
        new_corner
    }

    /// Insert the segment from `from` to `to`: how far it got.
    /// `insert_forced_trace_segment`.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_forced_trace_segment(&mut self, from: IntPoint, to: IntPoint, half_width: i64, layer: i32, nets: &[i32], cl_class: i32, limits: ShoveLimits, tidy_width: i32, with_check: bool) -> Reached {
        if from == to {
            return Reached::To;
        }
        let polyline = Polyline::from_two_points(from, to);
        self.insert_forced_trace_polyline(&polyline, half_width, layer, nets, cl_class, limits, tidy_width, with_check)
    }

    /// How far a straight trace from `from` to `to` gets before it comes
    /// too close to something, `i32::MAX` if it does not. With
    /// `only_not_shovable`, traces and vias that could be pushed do not
    /// count. `RoutingBoard.check_trace_segment`.
    #[allow(clippy::too_many_arguments)]
    pub fn check_trace_segment(&self, from: IntPoint, to: IntPoint, layer: i32, nets: &[i32], half_width: i64, cl_class: i32, only_not_shovable: bool) -> f64 {
        if from == to {
            return 0.0;
        }
        let polyline = Polyline::from_two_points(from, to);
        let segment = LineSegment::of(&polyline, 1);
        let check = segment.to_polyline();
        if check.lines.len() != 3 {
            return 0.0;
        }
        let shape_to_check = check.offset_shape(half_width, 0).expect("a segment");
        let from_point = segment.start_point_approx();
        let to_point = segment.end_point_approx();
        let line_length = to_point.distance(&from_point);
        let mut ok_length = i32::MAX as f64;
        for (i, k) in self.overlapping_entries_with_clearance(&shape_to_check, layer, nets, cl_class) {
            let obstacle = &self.board.items[i];
            if only_not_shovable && obstacle.is_routable() && !obstacle.is_shove_fixed(&self.board.rules) {
                continue;
            }
            let obstacle_shape = self.tree.shape(i, k);
            let clearance = self.clearance_value(obstacle.clearance_class, cl_class, layer);
            let offset_shape = shape_to_check.offset(clearance as f64);
            let shorten_value = (half_width + clearance) as f64;
            let intersection = obstacle_shape.intersection(&offset_shape);
            if intersection.is_empty() {
                continue;
            }
            let Some(nearest) = intersection.nearest_point_approx(&from_point) else { continue };
            let projection = from_point.scalar_product(&to_point, &nearest) / line_length;
            let projection = (projection - shorten_value - 1.0).max(0.0);
            if projection < ok_length {
                ok_length = projection;
                if ok_length <= 0.0 {
                    return 0.0;
                }
            }
        }
        ok_length
    }
}

/// Inserting one found connection. `InsertFoundConnectionAlgo`.
struct InsertConnection<'c> {
    ctrl: &'c Control,
    first_corner: Option<IntPoint>,
    last_corner: Option<IntPoint>,
}

/// Put the located connection on the board: its traces, vias between
/// them, the joins onto traces it ends at, then the net normalized. False
/// where the Java returns null, the board left as far as it got.
/// `InsertFoundConnectionAlgo.get_instance`.
pub fn insert_found_connection(rb: &mut RoutingBoard, located: &Located, ctrl: &Control) -> bool {
    let mut algo = InsertConnection { ctrl, first_corner: None, last_corner: None };
    let mut curr_layer = located.target_layer;
    for item in &located.traces {
        if !algo.insert_via(rb, item.corners[0], curr_layer, item.layer) {
            return false;
        }
        curr_layer = item.layer;
        if !algo.insert_trace(rb, item) {
            return false;
        }
    }
    let last = algo.last_corner.expect("a last corner");
    if !algo.insert_via(rb, last, curr_layer, located.start_layer) {
        return false;
    }
    if rb.is_trace(located.target_item) {
        let first = algo.first_corner.expect("a first corner (the Java fails without one)");
        rb.connect_to_trace(first, located.target_item, ctrl.trace_half_width[located.start_layer as usize], ctrl.trace_clearance_class);
    }
    if rb.is_trace(located.start_item) {
        rb.connect_to_trace(last, located.start_item, ctrl.trace_half_width[located.target_layer as usize], ctrl.trace_clearance_class);
    }
    let _ = rb.normalize_traces(ctrl.net_no);
    true
}

impl RoutingBoard {
    /// Join `from` onto the trace `to_trace` with a short square line, if
    /// it is not on it already, and drop the trace's tails left over:
    /// false if no line fits. `RoutingBoard.connect_to_trace`.
    pub fn connect_to_trace(&mut self, from: IntPoint, to_trace: usize, half_width: i64, cl_type: i32) -> bool {
        let (polyline, layer, _) = self.trace(to_trace);
        let polyline = polyline.clone();
        let (first_corner, last_corner) = (polyline.first_corner(), polyline.last_corner());
        let nets = self.board.items[to_trace].nets.clone();
        if polyline.contains(from) {
            return true;
        }
        let Some(projection_line) = polyline.projection_line(from) else { return false };
        let connection_line = projection_line.to_polyline();
        if connection_line.lines.len() != 3 {
            return false;
        }
        if !self.check_polyline_trace(&connection_line, layer, half_width, &nets, cl_type) {
            return false;
        }
        if self.changed_area.is_some() {
            for i in 0..connection_line.corner_count() {
                let p = connection_line.corner_float(i);
                self.join_changed_area(p, layer);
            }
        }
        self.insert_trace(connection_line, layer, half_width, &nets, cl_type, FixedState::Unfixed);
        let from = Point::Int(from);
        for corner in [first_corner, last_corner] {
            if !from.java_equals(&corner) {
                if let Some(tail) = self.get_trace_tail(&corner, layer, &nets) {
                    if !self.board.items[tail].is_user_fixed() {
                        self.remove_item(tail);
                    }
                }
            }
        }
        true
    }
}

impl InsertConnection<'_> {
    /// A via from `from_layer` to `to_layer` at `location`, if the layers
    /// differ. `InsertFoundConnectionAlgo.insert_via`.
    fn insert_via(&mut self, _rb: &mut RoutingBoard, _location: IntPoint, from_layer: i32, to_layer: i32) -> bool {
        if from_layer == to_layer {
            return true;
        }
        unimplemented!("inserting a via (ForcedViaAlgo.check and insert) is not ported yet");
    }

    /// Insert one trace of the connection segment by segment, necking down
    /// at pins where needed, then remove the stubs left. False if it did
    /// not all go in. `InsertFoundConnectionAlgo.insert_trace`.
    fn insert_trace(&mut self, rb: &mut RoutingBoard, trace: &LocatedTrace) -> bool {
        let corners = &trace.corners;
        if corners.len() == 1 {
            self.last_corner = Some(corners[0]);
            return true;
        }
        let ctrl = self.ctrl;
        let layer = trace.layer;
        let mut result = true;
        // No pin exit corrections while inserting segment by segment.
        let saved_edge_to_turn_dist = rb.board.rules.pin_edge_to_turn_dist;
        rb.board.rules.pin_edge_to_turn_dist = -1.0;
        let (mut start_pin, mut end_pin) = (None, None);
        if ctrl.with_neckdown {
            for (i, corner) in [corners[0], corners[corners.len() - 1]].into_iter().enumerate() {
                for pin in rb.pick_items(&Point::Int(corner), layer, Pick::Pins) {
                    let p = &rb.board.items[pin];
                    if p.contains_net(ctrl.net_no) && p.center() == Some(corner) {
                        if i == 0 {
                            start_pin = Some(pin);
                        } else {
                            end_pin = Some(pin);
                        }
                    }
                }
            }
        }
        let nets = [ctrl.net_no];
        let limits = ShoveLimits::of(ctrl);
        let half_width = ctrl.trace_half_width[layer as usize];
        let mut from_corner_no = 0;
        for i in 1..corners.len() {
            let curr_corner_arr = &corners[from_corner_no..=i];
            let insert_polyline = Polyline::from_points(curr_corner_arr);
            let ok_point = rb.insert_forced_trace_polyline(&insert_polyline, half_width, layer, &nets, ctrl.trace_clearance_class, limits, i32::MAX, true);
            let mut neckdown_inserted = false;
            if ok_point != Reached::Failed && ok_point != Reached::To && ctrl.with_neckdown && curr_corner_arr.len() == 2 {
                let ok = match ok_point {
                    Reached::From => insert_polyline.first_corner().as_int().expect("an integer corner"),
                    Reached::Other(p) => p,
                    _ => unreachable!(),
                };
                neckdown_inserted = self.insert_neckdown(rb, ok, curr_corner_arr[1], layer, start_pin, end_pin);
            }
            if ok_point == Reached::To || neckdown_inserted {
                from_corner_no = i;
            } else if ok_point == Reached::From && i != corners.len() - 1 {
                // The spring over may have failed; more distant corners
                // may let it through.
                if from_corner_no > 0 && curr_corner_arr.len() < 3 {
                    from_corner_no -= 1;
                }
            } else {
                result = false;
                break;
            }
        }
        for &corner in &corners[..corners.len() - 1] {
            if let Some(stub) = rb.get_trace_tail(&Point::Int(corner), layer, &nets) {
                rb.remove_item(stub);
            }
        }
        rb.board.rules.pin_edge_to_turn_dist = saved_edge_to_turn_dist;
        if self.first_corner.is_none() {
            self.first_corner = Some(corners[0]);
        }
        self.last_corner = Some(corners[corners.len() - 1]);
        result
    }

    /// Finish a segment that stopped short of a pin with a narrower trace:
    /// from the start pin, or to the end pin. `insert_neckdown`.
    fn insert_neckdown(&mut self, rb: &mut RoutingBoard, from: IntPoint, to: IntPoint, layer: i32, start_pin: Option<usize>, end_pin: Option<usize>) -> bool {
        if let Some(pin) = start_pin {
            if self.try_neck_down(rb, to, from, layer, pin) == Reached::To {
                return true;
            }
        }
        if let Some(pin) = end_pin {
            return self.try_neck_down(rb, from, to, layer, pin) == Reached::To;
        }
        false
    }

    /// From `from` towards `to` at a pin: at full width as far as fits,
    /// then necked down to the pin. How far it got. `try_neck_down`, which
    /// returns null where this returns `Failed` too.
    fn try_neck_down(&mut self, rb: &mut RoutingBoard, from: IntPoint, to: IntPoint, layer: i32, pin: usize) -> Reached {
        let ctrl = self.ctrl;
        let p = &rb.board.items[pin];
        if layer < p.first_layer || layer > p.last_layer {
            return Reached::Failed;
        }
        let ItemKind::Pin { center, neckdown, max_width, .. } = &p.kind else { return Reached::Failed };
        let index = (layer - p.first_layer) as usize;
        let pin_center = FloatPoint::from_int(*center);
        let clearance = rb.clearance_value(ctrl.trace_clearance_class, p.clearance_class, layer);
        let pin_neck_down_distance = 2.0 * (0.5 * max_width[index] + clearance as f64);
        if pin_center.distance(&FloatPoint::from_int(to)) >= pin_neck_down_distance {
            return Reached::Failed;
        }
        let neck_down_halfwidth = neckdown[index];
        let half_width = ctrl.trace_half_width[layer as usize];
        if neck_down_halfwidth >= half_width {
            return Reached::Failed;
        }
        let float_from = FloatPoint::from_int(from);
        let float_to = FloatPoint::from_int(to);
        const TOLERANCE: f64 = 2.0;
        let nets = [ctrl.net_no];
        let limits = ShoveLimits::of(ctrl);
        let cl = ctrl.trace_clearance_class;
        let mut ok_length = rb.check_trace_segment(from, to, layer, &nets, half_width, cl, true);
        if ok_length >= i32::MAX as f64 {
            return Reached::From;
        }
        ok_length -= TOLERANCE;
        // `None`: the neck-down starts at `from` itself.
        let mut neck_down_end_point: Option<IntPoint> = None;
        if ok_length > TOLERANCE {
            let float_end = float_from.change_length(&float_to, ok_length);
            let end = float_end.round();
            neck_down_end_point = Some(end);
            // A corner in case the end point is not on the line.
            let horizontal_first = (float_from.x - float_end.x).abs() >= (float_from.y - float_end.y).abs();
            let add_corner = fortyfive_degree_corner(&float_from, &float_end, horizontal_first).round();
            if rb.insert_forced_trace_segment(from, add_corner, half_width, layer, &nets, cl, limits, i32::MAX, true) != Reached::To {
                return Reached::From;
            }
            if rb.insert_forced_trace_segment(add_corner, end, half_width, layer, &nets, cl, limits, i32::MAX, true) != Reached::To {
                return Reached::From;
            }
            let add_corner = fortyfive_degree_corner(&float_end, &float_to, !horizontal_first).round();
            if add_corner != to {
                if rb.insert_forced_trace_segment(end, add_corner, half_width, layer, &nets, cl, limits, i32::MAX, true) != Reached::To {
                    return Reached::From;
                }
                neck_down_end_point = Some(add_corner);
            }
        }
        let start = neck_down_end_point.unwrap_or(from);
        match rb.insert_forced_trace_segment(start, to, neck_down_halfwidth, layer, &nets, cl, limits, i32::MAX, true) {
            Reached::To => Reached::To,
            Reached::From if neck_down_end_point.is_none() => Reached::From,
            Reached::From => Reached::Other(start),
            other => other,
        }
    }
}
