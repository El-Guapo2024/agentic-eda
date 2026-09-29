//! Vias: placing one where a connection changes layer, pushing aside what
//! is in a pad's way, and moving a via to shorten the traces it joins.
//! Ported from FreeRouting's `ForcedViaAlgo.check` and `insert`,
//! `ForcedPadAlgo.forced_pad` and `calc_from_side`, `MoveDrillItemAlgo.check`
//! and `insert`, `DrillItem.move_by`, `BasicBoard.insert_via` and
//! `OptViaAlgo`.
//!
//! As in [`super::shove`], pushing a trace or via of another net aside is
//! not ported yet; these stop, naming it, where the Java would.

use crate::geometry::{Circle, FloatLine, FloatPoint, IntPoint, Line, Point, Polyline, Side, TileShape};
use crate::model::{AreaKind, FixedState, Item, ItemKind};

use super::pull_tight::PullTight;
use super::shove::{DrillCheck, FromSide};
use super::{Pick, RoutingBoard};

impl FromSide {
    /// The side of `shape` nearest `from`, where its nearest border point
    /// lies. `CalcFromSide(Point, TileShape)`.
    pub fn of_point(from: IntPoint, shape: &TileShape) -> FromSide {
        let projection = shape.nearest_border_point(from);
        let no = shape.contains_on_border_line_no(&projection).map_or(-1, |n| n as i32);
        FromSide { no, border_intersection: Some(FloatPoint::from_point(&projection)) }
    }
}

/// A thin shape from `center` to the border line, square to it: the way a
/// trace would leave a pad through that side.
/// `ForcedPadAlgo.calc_check_shape_for_from_side`.
fn check_shape_for_from_side(center: IntPoint, border_line: &Line) -> TileShape {
    let projection = FloatPoint::from_int(center).projection_approx(border_line);
    let dir = border_line.direction();
    let lines = [Line::through(center, dir), Line::through(center, dir.turn_45_degree(2)), Line::through(projection.round(), dir)];
    Polyline::from_lines(&lines).offset_shape(1, 0).expect("a check shape (the Java fails without one)")
}

/// `IntVector.is_multiple_of_45_degree`.
fn is_multiple_of_45_degree(dx: i64, dy: i64) -> bool {
    dx == 0 || dy == 0 || dx.abs() == dy.abs()
}

/// The sign of `a * b - c * d`, exactly, as a side: positive left. The
/// products of rational coordinates can pass 128 bits, so they are taken
/// in 256.
fn side_of_det(a: i128, b: i128, c: i128, d: i128) -> Side {
    /// `a * b` as its sign and its magnitude's high and low halves.
    fn product(a: i128, b: i128) -> (bool, u128, u128) {
        const M: u128 = u64::MAX as u128;
        let (x, y) = (a.unsigned_abs(), b.unsigned_abs());
        let (x1, x0, y1, y0) = (x >> 64, x & M, y >> 64, y & M);
        let (p00, p01, p10, p11) = (x0 * y0, x0 * y1, x1 * y0, x1 * y1);
        let mid = (p00 >> 64) + (p01 & M) + (p10 & M);
        let lo = (p00 & M) | (mid << 64);
        let hi = p11 + (p01 >> 64) + (p10 >> 64) + (mid >> 64);
        ((a < 0) != (b < 0) && (hi, lo) != (0, 0), hi, lo)
    }
    let (p, q) = (product(a, b), product(c, d));
    let order = match (p.0, q.0) {
        (false, true) => std::cmp::Ordering::Greater,
        (true, false) => std::cmp::Ordering::Less,
        (false, false) => (p.1, p.2).cmp(&(q.1, q.2)),
        (true, true) => (q.1, q.2).cmp(&(p.1, p.2)),
    };
    match order {
        std::cmp::Ordering::Greater => Side::Left,
        std::cmp::Ordering::Less => Side::Right,
        std::cmp::Ordering::Equal => Side::Collinear,
    }
}

impl RoutingBoard {
    /// A via of padstack `padstack_no` at `center`, the traces of its nets
    /// through it split there. `BasicBoard.insert_via`.
    pub fn insert_via_item(&mut self, padstack_no: usize, center: IntPoint, nets: &[i32], clearance_class: i32, fixed: FixedState, attach_allowed: bool) -> usize {
        let ps = self.board.rules.padstack(padstack_no).expect("the via's padstack").clone();
        let pads = (ps.from_layer..=ps.to_layer).map(|l| ps.shapes.get(l as usize).cloned().flatten().map(|s| s.translate(center.x, center.y))).collect();
        let item = Item {
            id: 0,
            kind: ItemKind::Via { center, padstack: padstack_no, pads, attach_allowed },
            first_layer: ps.from_layer,
            last_layer: ps.to_layer,
            clearance_class,
            fixed,
            component: 0,
            nets: nets.to_vec(),
        };
        let via = self.insert_new(item);
        for layer in ps.from_layer..ps.to_layer {
            for &net in nets {
                self.split_traces(&Point::Int(center), layer, net);
            }
        }
        via
    }

    /// The pad shapes of via rule entry `via_info` placed at `location`,
    /// layer by layer, as bounding octagons. `None` where the stack has no
    /// shape.
    fn via_pad_octagons(&self, via_info: usize, location: IntPoint) -> Vec<(i32, TileShape)> {
        let vi = &self.board.rules.via_infos[via_info];
        let ps = self.board.rules.padstack(vi.padstack).expect("the via's padstack");
        let mut result = Vec::new();
        for layer in ps.from_layer..=ps.to_layer {
            let Some(shape) = ps.shapes.get(layer as usize).cloned().flatten() else { continue };
            let pad = shape.translate(location.x, location.y);
            result.push((layer, TileShape::Octagon(pad.bounding_octagon().expect("a bounded via pad"))));
        }
        result
    }

    /// Whether a via of rule entry `via_info` fits at `location`, pushing
    /// aside what can be pushed. `ForcedViaAlgo.check`.
    pub fn forced_via_check(&self, via_info: usize, location: IntPoint, nets: &[i32], max_recursion_depth: i32, max_via_recursion_depth: i32) -> bool {
        let vi = self.board.rules.via_infos[via_info].clone();
        let offset = self.min_trace_half_width;
        for (layer, tile) in self.via_pad_octagons(via_info, location) {
            let from_side = self.pad_from_side(&tile, location, layer, offset, vi.clearance_class);
            if self.check_forced_pad(&tile, Some(from_side), layer, nets, vi.clearance_class, vi.attach_smd_allowed, max_recursion_depth, max_via_recursion_depth, &[], false) == DrillCheck::NotDrillable {
                return false;
            }
        }
        true
    }

    /// Place a via of rule entry `via_info` at `location`, pushing aside
    /// what is in the way, with room to start a trace of each layer's half
    /// width: false if that failed. `ForcedViaAlgo.insert`.
    #[allow(clippy::too_many_arguments)]
    pub fn forced_via_insert(&mut self, via_info: usize, location: IntPoint, nets: &[i32], trace_clearance_class: i32, trace_half_widths: &[i64], max_recursion_depth: i32, max_via_recursion_depth: i32) -> bool {
        let vi = self.board.rules.via_infos[via_info].clone();
        let offset = self.min_trace_half_width;
        for (layer, tile) in self.via_pad_octagons(via_info, location) {
            let hw = trace_half_widths[layer as usize];
            let start_trace_shape = (hw > 0).then(|| TileShape::Octagon(Circle::new(location, hw).bounding_octagon()));
            let from_side = self.pad_from_side(&tile, location, layer, offset, vi.clearance_class);
            if !self.forced_pad(&tile, from_side, layer, nets, vi.clearance_class, vi.attach_smd_allowed, &[], max_recursion_depth, max_via_recursion_depth) {
                return false;
            }
            if let Some(start) = start_trace_shape {
                // In case the trace is wider than the pad.
                if !self.forced_pad(&start, from_side, layer, nets, trace_clearance_class, true, &[], max_recursion_depth, max_via_recursion_depth) {
                    return false;
                }
            }
        }
        self.insert_via_item(vi.padstack, location, nets, vi.clearance_class, FixedState::Unfixed, vi.attach_smd_allowed);
        true
    }

    /// The first side of `shape`, grown by `offset`, a trace could leave
    /// its centre through without meeting anything -- first keeping class
    /// `cl_class`'s clearances, then none; not found, the side is left for
    /// the push to work out. `ForcedPadAlgo.calc_from_side`.
    fn pad_from_side(&self, shape: &TileShape, center: IntPoint, layer: i32, offset: i64, cl_class: i32) -> FromSide {
        let offset_shape = shape.offset(offset as f64);
        for class in [cl_class, 0] {
            for i in 0..offset_shape.border_line_count() {
                let check = check_shape_for_from_side(center, &offset_shape.border_line(i));
                if self.check_trace_shape(&check, layer, &[], class, None) {
                    return FromSide { no: i as i32, border_intersection: None };
                }
            }
        }
        FromSide { no: -1, border_intersection: None }
    }

    /// Put a pad of `shape` on `layer`, pushing aside the traces and vias in
    /// its way: false if that failed. `ForcedPadAlgo.forced_pad`.
    #[allow(clippy::too_many_arguments)]
    pub fn forced_pad(&mut self, shape: &TileShape, from_side: FromSide, layer: i32, nets: &[i32], cl_class: i32, copper_sharing_allowed: bool, ignore: &[usize], max_recursion_depth: i32, max_via_recursion_depth: i32) -> bool {
        if shape.is_empty() {
            return true;
        }
        if !shape.is_contained_in_box(&self.board.bounds) {
            return false;
        }
        if !self.shove_vias_aside(shape, Some(from_side), layer, nets, cl_class, ignore, max_recursion_depth, max_via_recursion_depth, false) {
            return false;
        }
        let mut obstacles = self.overlapping_items_with_clearance(shape, layer, &[], cl_class);
        obstacles.retain(|i| !ignore.contains(i));
        let mut entries = self.pad_entries(shape, Some(from_side), layer, nets, cl_class);
        let Some(trace_piece_count) = self.store_pad_entries(&mut entries, &obstacles, copper_sharing_allowed) else { return false };
        if trace_piece_count == 0 {
            return true;
        }
        if max_recursion_depth <= 0 {
            return false;
        }
        let tails_exist_before = self.contains_trace_tails(&obstacles, nets);
        self.cutout_traces(&obstacles, shape, nets, cl_class);
        self.insert_substitute_pieces(&mut entries, layer, ignore, tails_exist_before, max_recursion_depth, max_via_recursion_depth, 0, true)
    }

    /// Whether the drill item could move by `(dx, dy)`, pushing aside what
    /// can be pushed. `MoveDrillItemAlgo.check`.
    pub fn move_drill_check(&self, item: usize, dx: i64, dy: i64, max_recursion_depth: i32, max_via_recursion_depth: i32, ignore: &[usize]) -> bool {
        let it = &self.board.items[item];
        if it.is_shove_fixed(&self.board.rules) {
            return false;
        }
        // Only traces and pours may connect to it.
        for c in self.normal_contacts(item) {
            if !(self.is_trace(c) || matches!(self.board.items[c].kind, ItemKind::Area { kind: AreaKind::Conduction { .. }, .. })) {
                return false;
            }
        }
        let mut ignore = ignore.to_vec();
        ignore.push(item);
        let attach_allowed = matches!(it.kind, ItemKind::Via { attach_allowed: true, .. });
        let center = it.center().expect("a drill item");
        for layer in it.first_layer..=it.last_layer {
            let Some(curr_shape) = self.tree.get_shape(item, (layer - it.first_layer) as u32) else { continue };
            let tile = TileShape::Octagon(curr_shape.translate_by(dx, dy).bounding_octagon().expect("a bounded pad"));
            let from_side = FromSide::of_point(center, &tile);
            if self.check_forced_pad(&tile, Some(from_side), layer, &it.nets, it.clearance_class, attach_allowed, max_recursion_depth, max_via_recursion_depth, &ignore, true) == DrillCheck::NotDrillable {
                return false;
            }
        }
        true
    }

    /// Move the drill item by `(dx, dy)`, pushing aside what is in the way:
    /// false if that failed. `MoveDrillItemAlgo.insert`.
    pub fn move_drill_insert(&mut self, item: usize, dx: i64, dy: i64, max_recursion_depth: i32, max_via_recursion_depth: i32) -> bool {
        let it = self.board.items[item].clone();
        if it.is_shove_fixed(&self.board.rules) {
            return false;
        }
        let attach_allowed = matches!(it.kind, ItemKind::Via { attach_allowed: true, .. });
        let center = it.center().expect("a drill item");
        let ignore = [item];
        for layer in it.first_layer..=it.last_layer {
            let Some(curr_shape) = self.tree.get_shape(item, (layer - it.first_layer) as u32).cloned() else { continue };
            let tile = TileShape::Octagon(curr_shape.translate_by(dx, dy).bounding_octagon().expect("a bounded pad"));
            let from_side = FromSide::of_point(center, &tile);
            if !self.forced_pad(&tile, from_side, layer, &it.nets, it.clearance_class, attach_allowed, &ignore, max_recursion_depth, max_via_recursion_depth) {
                return false;
            }
            let b = curr_shape.bounding_box();
            for j in 0..4 {
                let c = b.corner(j);
                self.join_changed_area(FloatPoint::from_int(c), layer);
            }
        }
        self.move_drill_by(item, dx, dy);
        true
    }

    /// Move the drill item, and join its old and new centres with a trace
    /// on each layer a trace met it on, as the first such trace there.
    /// `DrillItem.move_by`.
    fn move_drill_by(&mut self, item: usize, dx: i64, dy: i64) {
        let old_center = self.board.items[item].center().expect("a drill item");
        // By layer descending, the first trace on each layer.
        let mut infos: Vec<(i32, i64, i32)> = Vec::new();
        for c in self.normal_contacts(item) {
            if let ItemKind::Trace { layer, half_width, .. } = self.board.items[c].kind {
                if !infos.iter().any(|i| i.0 == layer) {
                    infos.push((layer, half_width, self.board.items[c].clearance_class));
                }
            }
        }
        infos.sort_by(|a, b| b.0.cmp(&a.0));
        // `Item.move_by`.
        self.save_for_undo(item);
        self.trees_remove(item);
        match &mut self.board.items[item].kind {
            ItemKind::Via { center, pads, .. } | ItemKind::Pin { center, pads, .. } => {
                *center = IntPoint::new(center.x + dx, center.y + dy);
                for p in pads.iter_mut().flatten() {
                    *p = p.translate(dx, dy);
                }
            }
            _ => unreachable!("only drill items move"),
        }
        self.trees_insert(item);
        let new_center = IntPoint::new(old_center.x + dx, old_center.y + dy);
        let mut points = vec![old_center];
        if let Some(add) = old_center.fortyfive_degree_corner(new_center, true) {
            points.push(add);
        }
        points.push(new_center);
        let nets = self.board.items[item].nets.clone();
        for (layer, half_width, cl) in infos {
            self.insert_trace(Polyline::from_points(&points), layer, half_width, &nets, cl, FixedState::Unfixed);
        }
    }

    /// Pull a trace tight, alone: of its own nets only if `own_net_only`.
    /// `PolylineTrace.pull_tight(boolean, int, Stoppable)`.
    pub fn pull_tight_alone(&mut self, item: usize, own_net_only: bool, accuracy: i32) -> bool {
        let nets = if own_net_only { self.board.items[item].nets.clone() } else { Vec::new() };
        let mut algo = PullTight::new(&nets, None, accuracy, None, -1);
        self.pull_tight_trace(item, &mut algo)
    }

    /// Move the via to shorten the traces it joins, weighing each layer's
    /// trace costs: true if it moved. `OptViaAlgo.opt_via_location`.
    pub fn opt_via_location(&mut self, via: usize, trace_costs: Option<&[(f64, f64)]>, accuracy: i32, max_recursion_depth: i32) -> bool {
        if self.board.items[via].is_shove_fixed(&self.board.rules) || max_recursion_depth <= 0 {
            return false;
        }
        let contacts = self.normal_contacts(via);
        let mut is_plane_or_fanout = contacts.len() == 1;
        let (mut first, mut second) = (None, None);
        if !is_plane_or_fanout {
            if contacts.len() != 2 {
                return false;
            }
            for (k, &c) in contacts.iter().enumerate() {
                if self.board.items[c].is_shove_fixed(&self.board.rules) || !self.is_trace(c) {
                    if matches!(self.board.items[c].kind, ItemKind::Area { kind: AreaKind::Conduction { .. }, .. }) {
                        is_plane_or_fanout = true;
                    } else {
                        return false;
                    }
                } else if k == 0 {
                    first = Some(c);
                } else {
                    second = Some(c);
                }
            }
        }
        if is_plane_or_fanout {
            return self.opt_plane_or_fanout_via(via, accuracy, max_recursion_depth);
        }
        let (first, second) = (first.expect("two traces"), second.expect("two traces"));
        let via_center = self.board.items[via].center().expect("a via");
        let from_corner = |rb: &RoutingBoard, t: usize| -> Option<Point> {
            let (polyline, _, _) = rb.trace(t);
            let c = Point::Int(via_center);
            if polyline.first_corner().java_equals(&c) {
                Some(polyline.corner(1))
            } else if polyline.last_corner().java_equals(&c) {
                Some(polyline.corner(polyline.corner_count() - 2))
            } else {
                None
            }
        };
        let Some(first_from) = from_corner(self, first) else { return false };
        let Some(second_from) = from_corner(self, second) else { return false };
        let (_, first_layer, first_hw) = self.trace(first);
        let (_, second_layer, second_hw) = self.trace(second);
        let (first_cl, second_cl) = (self.board.items[first].clearance_class, self.board.items[second].clearance_class);
        let costs = |layer: i32| trace_costs.map_or((1.0, 1.0), |c| c[layer as usize]);
        let first_leg = Leg { half_width: first_hw, cl: first_cl, layer: first_layer, costs: costs(first_layer), from_corner: first_from };
        let second_leg = Leg { half_width: second_hw, cl: second_cl, layer: second_layer, costs: costs(second_layer), from_corner: second_from };
        let Some(new_location) = self.reposition_via_legs(via, &first_leg, &second_leg) else { return false };
        if new_location == via_center {
            return false;
        }
        if !self.move_drill_insert(via, new_location.x - via_center.x, new_location.y - via_center.y, 9, 9) {
            return false;
        }
        let at = Point::Int(new_location);
        for layer in [first_layer, second_layer] {
            for t in self.pick_items(&at, layer, Pick::Traces) {
                self.pull_tight_alone(t, true, accuracy);
            }
        }
        if let Some(&next) = self.pick_items(&at, first_layer, Pick::Vias).first() {
            self.opt_via_location(next, trace_costs, accuracy, max_recursion_depth - 1);
        }
        true
    }

    /// A via on one trace, or on one trace and a plane: move it along the
    /// trace to its next corner, or square onto the segment after.
    /// `OptViaAlgo.opt_plane_or_fanout_via`.
    fn opt_plane_or_fanout_via(&mut self, via: usize, accuracy: i32, max_recursion_depth: i32) -> bool {
        if max_recursion_depth <= 0 {
            return false;
        }
        let contacts = self.normal_contacts(via);
        if contacts.is_empty() {
            return false;
        }
        let (mut plane, mut trace) = (None, None);
        for c in contacts {
            let item = &self.board.items[c];
            if matches!(item.kind, ItemKind::Area { kind: AreaKind::Conduction { .. }, .. }) {
                if plane.is_some() {
                    return false;
                }
                plane = Some(c);
            } else if self.is_trace(c) {
                if item.is_shove_fixed(&self.board.rules) || trace.is_some() {
                    return false;
                }
                trace = Some(c);
            } else {
                return false;
            }
        }
        let Some(trace) = trace else { return false };
        let via_center = self.board.items[via].center().expect("a via");
        let (polyline, layer, half_width) = self.trace(trace);
        let polyline = polyline.clone();
        let cl = self.board.items[trace].clearance_class;
        let center = Point::Int(via_center);
        let at_first = if polyline.first_corner().java_equals(&center) {
            true
        } else if polyline.last_corner().java_equals(&center) {
            false
        } else {
            return false;
        };
        let check_corner = if at_first { polyline.corner(1) } else { polyline.corner(polyline.corner_count() - 2) };
        let rounded_check_corner = FloatPoint::from_point(&check_corner).round();
        let mut new_location = self.reposition_via_towards(via, rounded_check_corner, half_width, layer, cl);
        if new_location.is_none() && polyline.corner_count() >= 3 {
            // Try the square onto the segment before.
            let prev_corner = if at_first { polyline.corner(2) } else { polyline.corner(polyline.corner_count() - 3) };
            let float_check = FloatPoint::from_point(&check_corner);
            let float_via = FloatPoint::from_int(via_center);
            let float_prev = FloatPoint::from_point(&prev_corner);
            if float_check.scalar_product(&float_via, &float_prev) != 0.0 {
                let projection = FloatLine::new(float_check, float_prev).perpendicular_projection(&float_via).round();
                let (dx, dy) = (projection.x - via_center.x, projection.y - via_center.y);
                let projection_ok = projection != via_center && is_multiple_of_45_degree(dx, dy);
                if projection_ok && self.move_drill_check(via, dx, dy, 0, 0, &[]) {
                    let nets = self.board.items[via].nets.clone();
                    let ok_length = self.check_trace_segment(via_center, projection, layer, &nets, half_width, cl, false);
                    if ok_length >= i32::MAX as f64 {
                        new_location = Some(projection);
                    }
                }
            }
        }
        let Some(new_location) = new_location else { return false };
        let at = Point::Int(new_location);
        if let Some(plane) = plane {
            // The new place must be on the plane.
            let ItemKind::Area { layer: plane_layer, .. } = self.board.items[plane].kind else { unreachable!() };
            if !self.pick_items(&at, plane_layer, Pick::Conduction).contains(&plane) {
                return false;
            }
        }
        if !self.move_drill_insert(via, new_location.x - via_center.x, new_location.y - via_center.y, 9, 9) {
            return false;
        }
        for t in self.pick_items(&at, layer, Pick::Traces) {
            self.pull_tight_alone(t, true, accuracy);
        }
        if at.java_equals(&check_corner) {
            self.opt_plane_or_fanout_via(via, accuracy, max_recursion_depth - 1);
        }
        true
    }

    /// Move the via towards `to` as far as the trace on `layer` and the via
    /// itself fit, halving the step; `None` if it cannot move.
    /// `OptViaAlgo.reposition_via(board, via, IntPoint, int, int, int)`.
    fn reposition_via_towards(&self, via: usize, to: IntPoint, half_width: i64, layer: i32, cl: i32) -> Option<IntPoint> {
        let from = self.board.items[via].center().expect("a via");
        if from == to {
            return None;
        }
        let nets = self.board.items[via].nets.clone();
        let mut ok_length = self.check_trace_segment(from, to, layer, &nets, half_width, cl, false);
        if ok_length <= 0.0 {
            return None;
        }
        let (float_from, float_to) = (FloatPoint::from_int(from), FloatPoint::from_int(to));
        let new_float_to = if ok_length >= i32::MAX as f64 { float_to } else { float_from.change_length(&float_to, ok_length) };
        let new_to = new_float_to.round();
        if self.move_drill_check(via, new_to.x - from.x, new_to.y - from.y, 0, 0, &[]) {
            return Some(new_to);
        }
        let min_length = 0.3 * half_width as f64 + 1.0;
        ok_length = ok_length.min(float_from.distance(&float_to));
        let mut curr_length = ok_length / 2.0;
        ok_length = 0.0;
        let mut result = None;
        while curr_length >= min_length {
            let check_point = float_from.change_length(&float_to, ok_length + curr_length).round();
            if self.move_drill_check(via, check_point.x - from.x, check_point.y - from.y, 0, 0, &[]) {
                ok_length += curr_length;
                result = Some(check_point);
            }
            curr_length /= 2.0;
        }
        result
    }

    /// Whether the via can move to `to`, with the one trace running there
    /// straight and the other on from there to `connect`.
    /// `OptViaAlgo.reposition_via(board, via, IntPoint, ..., IntPoint, ...)`.
    fn reposition_via_to(&self, via: usize, to: IntPoint, leg_1: (i64, i32, i32), connect: IntPoint, leg_2: (i64, i32, i32)) -> bool {
        let from = self.board.items[via].center().expect("a via");
        if from == to {
            return false;
        }
        let nets = self.board.items[via].nets.clone();
        let (hw_1, layer_1, cl_1) = leg_1;
        let (hw_2, layer_2, cl_2) = leg_2;
        if self.check_trace_segment(from, to, layer_1, &nets, hw_1, cl_1, false) < i32::MAX as f64 {
            return false;
        }
        if self.check_trace_segment(to, connect, layer_2, &nets, hw_2, cl_2, false) < i32::MAX as f64 {
            return false;
        }
        self.move_drill_check(via, to.x - from.x, to.y - from.y, 0, 0, &[])
    }

    /// A better place for a via joining two traces, weighing each layer's
    /// costs: along either trace, across an acute angle, or at the corner
    /// of an axis-parallel detour. `OptViaAlgo.reposition_via` for two
    /// traces.
    fn reposition_via_legs(&self, via: usize, first: &Leg, second: &Leg) -> Option<IntPoint> {
        let via_location = self.board.items[via].center().expect("a via");
        // The corners from the via, exactly: numerators over a positive
        // denominator, 1 for a corner on the grid. `Point.difference_by`.
        let delta = |p: &Point| -> (i128, i128, i128) {
            match p {
                Point::Int(q) => ((q.x - via_location.x) as i128, (q.y - via_location.y) as i128, 1),
                Point::Rational(r) => {
                    let (x, y, z) = (r.x - via_location.x as i128 * r.z, r.y - via_location.y as i128 * r.z, r.z);
                    if z < 0 {
                        (-x, -y, -z)
                    } else {
                        (x, y, z)
                    }
                }
            }
        };
        let (first_delta, second_delta) = (delta(&first.from_corner), delta(&second.from_corner));
        // `Vector.scalar_product`: exact for two grid vectors, else of
        // their floating point approximations.
        let scalar = if matches!((&first.from_corner, &second.from_corner), (Point::Int(_), Point::Int(_))) {
            first_delta.0 as f64 * second_delta.0 as f64 + first_delta.1 as f64 * second_delta.1 as f64
        } else {
            let approx = |d: (i128, i128, i128)| (d.0 as f64 / d.2 as f64, d.1 as f64 / d.2 as f64);
            let (a, b) = (approx(first_delta), approx(second_delta));
            a.0 * b.0 + a.1 * b.1
        };
        let float_via = FloatPoint::from_int(via_location);
        let (float_first, float_second) = (FloatPoint::from_point(&first.from_corner), FloatPoint::from_point(&second.from_corner));
        let first_distance = float_via.distance(&float_first);
        let second_distance = float_via.distance(&float_second);
        let (rounded_first, rounded_second) = (float_first.round(), float_second.round());
        let first_trace = (first.half_width, first.layer, first.cl);
        let second_trace = (second.half_width, second.layer, second.cl);
        let towards = |to: IntPoint, leg: (i64, i32, i32)| self.reposition_via_towards(via, to, leg.0, leg.1, leg.2);
        // The traces overlapping first.
        // Point.side_of, exactly: the via's side of the line from the first
        // corner to the second is the sign of first x second, from the via.
        if side_of_det(first_delta.0, second_delta.1, first_delta.1, second_delta.0) == Side::Collinear && scalar > 0.0 {
            if second_distance < first_distance {
                return towards(rounded_second, first_trace);
            }
            return towards(rounded_first, second_trace);
        }
        let wd = |a: &FloatPoint, b: &FloatPoint, c: (f64, f64)| a.weighted_distance(b, c.0, c.1);
        if wd(&float_via, &float_first, first.costs) > wd(&float_via, &float_first, second.costs) {
            // Towards the first trace's corner.
            if let Some(r) = towards(rounded_first, second_trace) {
                return Some(r);
            }
        }
        if wd(&float_via, &float_second, second.costs) > wd(&float_via, &float_second, first.costs) {
            // Towards the second trace's corner.
            if let Some(r) = towards(rounded_second, first_trace) {
                return Some(r);
            }
        }
        if scalar > 0.0 {
            // An acute angle.
            let (to_1, float_to_1, to_2, float_to_2);
            if first_distance < second_distance {
                to_1 = rounded_first;
                float_to_1 = float_first;
                float_to_2 = float_via.change_length(&float_second, first_distance);
                to_2 = float_to_2.round();
            } else {
                float_to_1 = float_via.change_length(&float_first, second_distance);
                to_1 = float_to_1.round();
                to_2 = rounded_second;
                float_to_2 = float_second;
            }
            let result = if wd(&float_to_1, &float_to_2, first.costs) > wd(&float_to_1, &float_to_2, second.costs) {
                towards(to_1, second_trace).or_else(|| towards(to_2, first_trace))
            } else {
                towards(to_2, first_trace).or_else(|| towards(to_1, second_trace))
            };
            if result.is_some() {
                return result;
            }
        }
        // Axis-parallel detours.
        for (delta, this_leg, other_leg, float_from, rounded_from) in [
            (first_delta, first, second, float_first, rounded_first),
            (second_delta, second, first, float_second, rounded_second),
        ] {
            // Vector.is_orthogonal.
            if delta.0 == 0 || delta.1 == 0 {
                continue;
            }
            let d_1 = wd(&float_via, &float_from, this_leg.costs);
            for float_check in [FloatPoint::new(float_via.x, float_from.y), FloatPoint::new(float_from.x, float_via.y)] {
                let d_2 = wd(&float_via, &float_check, other_leg.costs);
                let d_3 = wd(&float_check, &float_from, this_leg.costs);
                if d_1 > d_2 + d_3 {
                    let check = float_check.round();
                    if self.reposition_via_to(via, check, (other_leg.half_width, other_leg.layer, other_leg.cl), rounded_from, (this_leg.half_width, this_leg.layer, this_leg.cl)) {
                        return Some(check);
                    }
                }
            }
        }
        None
    }
}

/// One of the two traces a via joins: its half width, clearance class,
/// layer, cost factors, and its corner next to the via.
struct Leg {
    half_width: i64,
    cl: i32,
    layer: i32,
    costs: (f64, f64),
    from_corner: Point,
}
