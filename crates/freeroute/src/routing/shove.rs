//! Whether a trace or via fits somewhere if what can be pushed aside is
//! pushed, and pushing it: the checks the maze search makes before it opens
//! a via or squeezes through a thin room, and what inserting a found
//! connection runs. Ported from FreeRouting's `ShoveTraceAlgo`,
//! `ShapeTraceEntries`, `CalcFromSide`, `ForcedViaAlgo.check_layer`,
//! `ForcedPadAlgo.check_forced_pad` and `RoutingBoard.check_forced_trace_polyline`.
//!
//! What pushes a trace or via of another net aside -- the substitute
//! traces `ShapeTraceEntries` builds round the shape, and `MoveDrillItemAlgo`
//! -- is not ported yet; where the Java would do that, these stop with a
//! message naming the missing piece. Traces of the net being routed are
//! passed over as in the Java.

use crate::geometry::{Circle, Cutout, Direction, FloatPoint, IntBox, IntPoint, Line, LineSegment, Polyline, Side, Simplex, TileShape};
use crate::model::{AreaKind, FixedState, Item, ItemKind};

use super::trace::StopConnection;
use super::{nets_equal, RoutingBoard};

/// `ForcedPadAlgo.CheckDrillResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrillCheck {
    NotDrillable,
    Drillable,
    DrillableWithAttachSmd,
}

/// The side of a shape a push comes from: the number of its border line,
/// and where it is crossed. `CalcFromSide`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FromSide {
    pub no: i32,
    pub border_intersection: Option<FloatPoint>,
}

impl FromSide {
    /// Where `polyline` enters `shape`: the border line its segment `no`,
    /// or the last one before it that crosses the border, crosses; failing
    /// that, the border line whose crossing with the first segment's line
    /// is nearest the first corner. `CalcFromSide(Polyline, int, TileShape)`.
    pub fn of_polyline(polyline: &Polyline, no: i64, shape: &TileShape) -> FromSide {
        let mut curr_no = no;
        while curr_no > 0 {
            let segment = LineSegment::of(polyline, curr_no as usize);
            let intersections = segment.border_intersections(shape);
            if let Some(&side) = intersections.first() {
                let (x, y) = segment.middle.intersection_approx(&shape.border_line(side));
                return FromSide { no: side as i32, border_intersection: Some(FloatPoint::new(x, y)) };
            }
            curr_no -= 1;
        }
        let from_point = polyline.corner_float(0);
        let check_line = polyline.lines[1];
        let mut min_dist = f64::MAX;
        let mut result = FromSide { no: -1, border_intersection: None };
        for i in 0..shape.border_line_count() {
            let (x, y) = check_line.intersection_approx(&shape.border_line(i));
            let curr = FloatPoint::new(x, y);
            let dist = curr.distance(&from_point).abs();
            if dist < min_dist {
                result = FromSide { no: i as i32, border_intersection: Some(curr) };
                min_dist = dist;
            }
        }
        result
    }
}

impl FromSide {
    /// The side of `shape` a push along `segment` comes from: the border
    /// line the segment's line crosses nearer its start, turned two sides
    /// on, left or right as the push goes; `no` -1 if none is crossed.
    /// `CalcFromSide(LineSegment, TileShape, boolean)`.
    pub fn of_segment(segment: &LineSegment, shape: &TileShape, shove_to_the_left: bool) -> FromSide {
        let start_corner = segment.start_point_approx();
        let end_corner = segment.end_point_approx();
        let count = shape.border_line_count();
        let check_line = segment.middle;
        let first_corner = shape.corner_approx(0);
        let mut prev_side = check_line.side_of_float((first_corner.x, first_corner.y), 0.0);
        let mut front_side_no = None;
        for i in 1..=count {
            let next_corner = if i == count { first_corner } else { shape.corner_approx(i) };
            let next_side = check_line.side_of_float((next_corner.x, next_corner.y), 0.0);
            if prev_side != next_side {
                let (x, y) = shape.border_line(i - 1).intersection_approx(&check_line);
                let crossing = FloatPoint::new(x, y);
                if crossing.distance_square(&start_corner) < crossing.distance_square(&end_corner) {
                    front_side_no = Some(i - 1);
                    break;
                }
            }
            prev_side = next_side;
        }
        let Some(front) = front_side_no else {
            return FromSide { no: -1, border_intersection: None };
        };
        let no = if shove_to_the_left { (front + 2) % count } else { (front + count - 2) % count };
        let border_intersection = shape.corner_approx(no).middle_point(&shape.corner_approx((no + 1) % count));
        FromSide { no: no as i32, border_intersection: Some(border_intersection) }
    }
}

/// A piece of trace a push would lay round the shape pushed into, in
/// place of the piece inside: not on the board. `ShapeTraceEntries`
/// makes a `PolylineTrace` of it, which draws an item number.
#[derive(Debug, Clone)]
pub(crate) struct SubstituteTrace {
    /// The number it drew.
    pub id: u32,
    pub polyline: Polyline,
    pub layer: i32,
    pub half_width: i64,
    pub nets: Vec<i32>,
    pub cl_class: i32,
}

/// What spring-over made of a polyline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spring {
    /// Nothing in the way: the polyline itself, the same object in the
    /// Java.
    Same,
    /// A new polyline round the obstacles.
    New(Polyline),
    /// No way round.
    Fail,
}

impl RoutingBoard {
    /// Items touching `shape` on `layer` once each keeps its clearance from
    /// class `cl_class`: both shapes grown by half that clearance, safety
    /// margin included, and tested exactly. Items no obstacle to one of
    /// `ignore_nets` are skipped. By item number descending.
    /// `ShapeSearchTree.overlapping_items_with_clearance` on the default
    /// tree, which is not clearance-compensated.
    pub fn overlapping_items_with_clearance(&self, shape: &TileShape, layer: i32, ignore_nets: &[i32], cl_class: i32) -> Vec<usize> {
        let mut result: Vec<usize> = self.overlapping_entries_with_clearance(shape, layer, ignore_nets, cl_class).into_iter().map(|(i, _)| i).collect();
        self.sort_items(&mut result);
        result
    }

    /// The tree entries behind [`overlapping_items_with_clearance`], by
    /// clearance, then item number descending and shape index.
    /// `ShapeSearchTree.overlapping_tree_entries_with_clearance`.
    ///
    /// [`overlapping_items_with_clearance`]: Self::overlapping_items_with_clearance
    pub fn overlapping_entries_with_clearance(&self, shape: &TileShape, layer: i32, ignore_nets: &[i32], cl_class: i32) -> Vec<(usize, u32)> {
        let board = &self.board;
        let bounds = shape.bounding_octagon().unwrap_or_else(|| board.bounds.to_octagon());
        // Every candidate within the largest clearance of the class: the
        // bounds grown by 1.2 times it, as enlarging octagons is not
        // symmetric.
        let max_clearance = (1.2 * board.rules.max_clearance(cl_class, layer) as f64) as i64;
        let mut sorted: Vec<(i64, usize, u32)> = Vec::new();
        for (i, k, l) in self.tree.candidates(board, &bounds.offset(max_clearance as f64)) {
            let item = &board.items[i];
            if (layer >= 0 && l != layer) || ignore_nets.iter().any(|&net| !item.is_obstacle(net)) {
                continue;
            }
            let clearance = board.rules.clearance.get_with_margin(cl_class, item.clearance_class, layer);
            sorted.push((clearance, i, k));
        }
        // A stable sort keeps the candidates' order among equal clearances.
        sorted.sort_by_key(|e| e.0);
        let mut result = Vec::new();
        let mut curr_half = 0;
        let mut grown = shape.clone();
        for (clearance, i, k) in sorted {
            let half = clearance / 2;
            if half != curr_half {
                curr_half = half;
                grown = shape.enlarge(half as f64);
            }
            let other = self.tree.shape(i, k).enlarge(curr_half as f64);
            if grown.intersects(&other) {
                result.push((i, k));
            }
        }
        result
    }

    /// Whether a via of radius `via_radius` and clearance class `cl_class`
    /// fits at `location` on `layer` within the room `room_shape`, pushing
    /// aside what can be pushed. `ForcedViaAlgo.check_layer`.
    #[allow(clippy::too_many_arguments)]
    pub fn check_via_layer(
        &self,
        via_radius: f64,
        cl_class: i32,
        attach_smd_allowed: bool,
        room_shape: &TileShape,
        location: IntPoint,
        layer: i32,
        nets: &[i32],
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
    ) -> DrillCheck {
        if via_radius <= 0.0 {
            return DrillCheck::Drillable;
        }
        let via_shape = Circle::new(location, via_radius.ceil() as i64);
        let clearance = self.clearance_value(cl_class, cl_class, layer);
        let check_radius = via_radius + 0.5 * clearance as f64 + self.min_trace_half_width as f64;
        let tile_shape = TileShape::Octagon(via_shape.bounding_octagon());
        let room = match room_shape {
            TileShape::Simplex(s) => TileShape::Simplex(s.clone()),
            TileShape::Octagon(o) => TileShape::Simplex(o.to_simplex()),
            TileShape::Box(b) => TileShape::Simplex(b.to_simplex()),
        };
        let Some(from_side) = via_from_side(&FloatPoint::from_int(location), &tile_shape, &room, check_radius) else {
            return DrillCheck::NotDrillable;
        };
        self.check_forced_pad(&tile_shape, Some(from_side), layer, nets, cl_class, attach_smd_allowed, max_recursion_depth, max_via_recursion_depth, &[])
    }

    /// Whether a pad `shape` fits on `layer`, pushing aside what can be
    /// pushed; with copper sharing, touching the net's own SMD pins is
    /// allowed and reported. `ForcedPadAlgo.check_forced_pad`.
    #[allow(clippy::too_many_arguments)]
    pub fn check_forced_pad(
        &self,
        shape: &TileShape,
        from_side: Option<FromSide>,
        layer: i32,
        nets: &[i32],
        cl_class: i32,
        copper_sharing_allowed: bool,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
        ignore: &[usize],
    ) -> DrillCheck {
        let board = &self.board;
        if !shape.is_contained_in_box(&board.bounds) {
            return DrillCheck::NotDrillable;
        }
        let mut obstacles = self.overlapping_items_with_clearance(shape, layer, &[], cl_class);
        obstacles.retain(|i| !ignore.contains(i));
        let mut entries = ShapeTraceEntries::new(shape, layer, nets, cl_class, from_side);
        if !entries.store_items(self, &obstacles, true, copper_sharing_allowed) {
            return DrillCheck::NotDrillable;
        }
        if !entries.shove_vias.is_empty() {
            if max_via_recursion_depth <= 0 {
                return DrillCheck::NotDrillable;
            }
            unimplemented!("pushing a via aside for a new via (MoveDrillItemAlgo) is not ported yet");
        }
        let mut result = DrillCheck::Drillable;
        if copper_sharing_allowed && obstacles.iter().any(|&i| matches!(board.items[i].kind, ItemKind::Pin { .. })) {
            result = DrillCheck::DrillableWithAttachSmd;
        }
        if entries.trace_piece_count == 0 {
            return result;
        }
        if max_recursion_depth <= 0 || entries.max_stack_level > 1 {
            return DrillCheck::NotDrillable;
        }
        if entries.next_substitute_trace_piece(self).is_some() {
            unimplemented!("pushing a trace aside for a new via (ForcedPadAlgo with substitute traces) is not ported yet");
        }
        result
    }

    /// Whether a trace of half width `half_width` along `polyline` fits on
    /// `layer`, pushing aside what can be pushed.
    /// `RoutingBoard.check_forced_trace_polyline`.
    #[allow(clippy::too_many_arguments)]
    pub fn check_forced_trace_polyline(
        &self,
        polyline: &Polyline,
        half_width: i64,
        layer: i32,
        nets: &[i32],
        cl_class: i32,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
        max_spring_over_recursion_depth: i32,
    ) -> bool {
        // The default tree is not clearance-compensated: its compensation is
        // 0 and the trace keeps its own half width.
        let n = polyline.lines.len();
        if n < 3 {
            return true;
        }
        for (i, shape) in polyline.offset_shapes(half_width, 0, n - 1).into_iter().enumerate() {
            let from_side = FromSide::of_polyline(polyline, i as i64 + 1, &shape);
            if !self.shove_check(&shape, Some(from_side), None, layer, nets, cl_class, max_recursion_depth, max_via_recursion_depth, max_spring_over_recursion_depth) {
                return false;
            }
        }
        true
    }

    /// Whether a trace piece of shape `shape` fits on `layer`, pushing
    /// aside what can be pushed; `dir` keeps a recursive check from
    /// bouncing back. `ShoveTraceAlgo.check`.
    #[allow(clippy::too_many_arguments)]
    pub fn shove_check(
        &self,
        shape: &TileShape,
        from_side: Option<FromSide>,
        dir: Option<Direction>,
        layer: i32,
        nets: &[i32],
        cl_class: i32,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
        max_spring_over_recursion_depth: i32,
    ) -> bool {
        if shape.is_empty() {
            return true;
        }
        if !shape.is_contained_in_box(&self.board.bounds) {
            return false;
        }
        let mut entries = ShapeTraceEntries::new(shape, layer, nets, cl_class, from_side);
        let mut obstacles = self.overlapping_items_with_clearance(shape, layer, &[], cl_class);
        let ignore = self.ignore_items_at_tie_pins(shape, layer, nets);
        obstacles.retain(|i| !ignore.contains(i));
        if !entries.store_items(self, &obstacles, false, true) {
            return false;
        }
        let trace_piece_count = entries.trace_piece_count;
        if entries.max_stack_level > 1 {
            return false;
        }
        for &via in &entries.shove_vias {
            if self.board.items[via].shares_net_no(nets) {
                continue;
            }
            if max_via_recursion_depth <= 0 {
                return false;
            }
            unimplemented!("pushing a via aside for a trace (MoveDrillItemAlgo) is not ported yet");
        }
        if trace_piece_count == 0 {
            return true;
        }
        if max_recursion_depth <= 0 {
            return false;
        }
        // Each piece a push would lay round the shape must fit in turn,
        // sprung over what it can be, its segments facing the push checked
        // the same way.
        let mut spring_depth = max_spring_over_recursion_depth;
        while let Some(mut piece) = entries.next_substitute_trace_piece(self) {
            if spring_depth > 0 {
                let compensated_half_width = piece.half_width + self.plain_compensation(piece.cl_class, layer);
                match self.spring_over(&piece.polyline, compensated_half_width, layer, &piece.nets, piece.cl_class, false, spring_depth, None) {
                    Spring::Fail => return false,
                    Spring::Same => {}
                    Spring::New(polyline) => {
                        spring_depth -= 1;
                        piece.polyline = polyline;
                    }
                }
            }
            for i in 0..piece.polyline.lines.len().saturating_sub(2) {
                let curr_dir = piece.polyline.lines[i + 1].direction();
                if dir.is_none_or(|d| d == curr_dir) {
                    let (shape, from_side) = self.substitute_shape_and_from_side(&piece, i, true);
                    if !self.shove_check(&shape, from_side, Some(curr_dir), layer, &piece.nets, piece.cl_class, max_recursion_depth - 1, max_via_recursion_depth, spring_depth) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// `ShapeSearchTree.clearance_compensation_value` of the plain tree,
    /// which compensates for no class.
    fn plain_compensation(&self, cl_class: i32, layer: i32) -> i64 {
        crate::board::clearance_offset(&self.board.rules.clearance, cl_class, 0, layer)
    }

    /// The shape of a substitute trace's segment `index` in the plain
    /// tree, cut off where the trace's first or last line is near so it
    /// grows no dog ears, and the side a push through it comes from; left
    /// open in a shove check where no line was cut off.
    /// `CalcShapeAndFromSide`, not orthogonal.
    fn substitute_shape_and_from_side(&self, piece: &SubstituteTrace, index: usize, in_shove_check: bool) -> (TileShape, Option<FromSide>) {
        let lines = &piece.polyline;
        let compensated_half_width = piece.half_width + self.plain_compensation(piece.cl_class, piece.layer);
        let tree_shape = lines.offset_shape(compensated_half_width, index).expect("a segment has a shape");
        let mut curr = match &tree_shape {
            TileShape::Box(b) => b.to_simplex(),
            TileShape::Octagon(o) => o.to_simplex(),
            TileShape::Simplex(s) => s.clone(),
        };
        let len = lines.lines.len();
        let cut_line = |line: Line, inside: FloatPoint| if line.side_of_float((inside.x, inside.y), 0.0) == Side::Left { line.opposite() } else { line };
        let end_cutline = (index == len - 3 || lines.corner_float(len - 2).distance(&lines.corner_float(index + 1)) < compensated_half_width as f64)
            .then(|| cut_line(lines.lines[len - 1], lines.corner_float(len - 3)));
        let mut cut_off_at_end = false;
        if let Some(cut) = end_cutline {
            let tmp = Simplex::from_lines(&[cut]).intersection(&curr);
            if !tmp.is_empty() {
                curr = tmp;
                cut_off_at_end = true;
            }
        }
        let start_cutline = (index == 0 || lines.corner_float(0).distance(&lines.corner_float(index)) < compensated_half_width as f64).then(|| cut_line(lines.lines[0], lines.corner_float(1)));
        let mut cut_off_at_start = false;
        if let Some(cut) = start_cutline {
            let tmp = Simplex::from_lines(&[cut]).intersection(&curr);
            if !tmp.is_empty() {
                curr = tmp;
                cut_off_at_start = true;
            }
        }
        let index_of = |line: &Line| curr.lines.iter().position(|l| l == line);
        let mut found = None;
        if cut_off_at_start {
            let cut = start_cutline.expect("cut off at the start");
            found = index_of(&cut).map(|no| (no, cut));
        }
        if found.is_none() && cut_off_at_end {
            let cut = end_cutline.expect("cut off at the end");
            found = index_of(&cut).map(|no| (no, cut));
        }
        let mut from_side = found.map(|(no, cut)| {
            let (x, y) = cut.intersection_approx(&curr.lines[no]);
            FromSide { no: no as i32, border_intersection: Some(FloatPoint::new(x, y)) }
        });
        let shape = TileShape::Simplex(curr);
        if from_side.is_none() && !in_shove_check {
            from_side = Some(FromSide::of_polyline(lines, index as i64, &shape));
        }
        (shape, from_side)
    }

    /// How far a trace segment can be pushed along `segment`, to the left
    /// or right of it, pushing aside the traces in the way: `i32::MAX` if
    /// nothing limits it, 0 if it cannot be pushed at all.
    /// `ShoveTraceAlgo.check(RoutingBoard, LineSegment, ...)`.
    #[allow(clippy::too_many_arguments)]
    pub fn shove_check_segment(&self, segment: &LineSegment, shove_to_the_left: bool, layer: i32, nets: &[i32], half_width: i64, cl_class: i32, max_recursion_depth: i32, max_via_recursion_depth: i32) -> f64 {
        let polyline = segment.to_polyline();
        if polyline.lines.len() != 3 {
            return 0.0;
        }
        let Some(trace_shape) = polyline.offset_shape(half_width, 0) else { return 0.0 };
        if trace_shape.is_empty() || !trace_shape.is_contained_in_box(&self.board.bounds) {
            return 0.0;
        }
        let from_side = FromSide::of_segment(segment, &trace_shape, shove_to_the_left);
        let mut entries = ShapeTraceEntries::new(&trace_shape, layer, nets, cl_class, Some(from_side));
        let obstacles = self.overlapping_items_with_clearance(&trace_shape, layer, &[], cl_class);
        if !entries.store_items(self, &obstacles, false, true) || entries.shape_contains_trace_tails {
            return 0.0;
        }
        let trace_piece_count = entries.trace_piece_count;
        if entries.max_stack_level > 1 {
            return 0.0;
        }
        let start_corner = segment.start_point_approx();
        let end_corner = segment.end_point_approx();
        let segment_length = end_corner.distance(&start_corner);
        let cm = &self.board.rules.clearance;
        let mut result = i32::MAX as f64;
        for &via in &entries.shove_vias {
            let item = &self.board.items[via];
            if item.shares_net_no(nets) {
                continue;
            }
            if max_via_recursion_depth > 0 {
                unimplemented!("pushing a via aside for a pushed trace (MoveDrillItemAlgo.try_shove_via_points) is not ported yet");
            }
            // The via stays: the push goes as far as its projection less
            // its radius and the clearance.
            let ItemKind::Via { center, .. } = item.kind else { unreachable!("a via") };
            let projection = start_corner.scalar_product(&end_corner, &FloatPoint::from_int(center)) / segment_length;
            let via_box = self.tree.shape(via, (layer - item.first_layer) as u32).bounding_box();
            let via_radius = 0.5 * box_max_width(&via_box);
            let ok_length = projection - via_radius - half_width as f64 - cm.get_with_margin(cl_class, item.clearance_class, layer) as f64;
            if ok_length <= 0.0 {
                return 0.0;
            }
            result = result.min(ok_length);
        }
        if trace_piece_count == 0 {
            return result;
        }
        if max_recursion_depth <= 0 {
            return 0.0;
        }
        let line_direction = segment.middle.direction();
        while let Some(piece) = entries.next_substitute_trace_piece(self) {
            for i in 0..piece.polyline.lines.len().saturating_sub(2) {
                let mut curr_segment = LineSegment::of(&piece.polyline, i + 1);
                if shove_to_the_left {
                    // Swapped, to get the right length where it is shorter
                    // than the whole segment.
                    curr_segment = curr_segment.opposite();
                }
                if curr_segment.middle.direction() != line_direction {
                    continue;
                }
                let shove_ok_length = self.shove_check_segment(&curr_segment, shove_to_the_left, layer, &piece.nets, piece.half_width, piece.cl_class, max_recursion_depth - 1, max_via_recursion_depth);
                if shove_ok_length < i32::MAX as f64 {
                    if shove_ok_length <= 0.0 {
                        return 0.0;
                    }
                    let projection = start_corner.scalar_product(&end_corner, &curr_segment.start_point_approx()).min(start_corner.scalar_product(&end_corner, &curr_segment.end_point_approx())) / segment_length;
                    let ok_length = shove_ok_length + projection - half_width as f64 - piece.half_width as f64 - cm.get_with_margin(cl_class, piece.cl_class, layer) as f64;
                    if ok_length <= 0.0 {
                        return 0.0;
                    }
                    result = result.min(ok_length);
                }
                break;
            }
        }
        result
    }

    /// Put in a trace piece of shape `shape`, pushing what is in the way
    /// aside; false where that fails. `ShoveTraceAlgo.insert`.
    #[allow(clippy::too_many_arguments)]
    pub fn shove_insert(
        &mut self,
        shape: &TileShape,
        from_side: Option<FromSide>,
        layer: i32,
        nets: &[i32],
        cl_class: i32,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
        max_spring_over_recursion_depth: i32,
    ) -> bool {
        if shape.is_empty() {
            return true;
        }
        if !shape.is_contained_in_box(&self.board.bounds) {
            return false;
        }
        if !self.shove_vias_aside(shape, from_side, layer, nets, cl_class, &[], max_via_recursion_depth, true) {
            return false;
        }
        let mut entries = ShapeTraceEntries::new(shape, layer, nets, cl_class, from_side);
        let mut obstacles = self.overlapping_items_with_clearance(shape, layer, &[], cl_class);
        let ignore = self.ignore_items_at_tie_pins(shape, layer, nets);
        obstacles.retain(|i| !ignore.contains(i));
        let obstacles_shovable = entries.store_items(self, &obstacles, false, true);
        if !entries.shove_vias.is_empty() {
            return false;
        }
        if !obstacles_shovable {
            return false;
        }
        if entries.trace_piece_count == 0 {
            return true;
        }
        if max_recursion_depth <= 0 {
            return false;
        }
        let tails_exist_before = self.contains_trace_tails(&obstacles, nets);
        self.cutout_traces(&obstacles, shape, nets, cl_class);
        if matches!(shape, TileShape::Box(_)) {
            unimplemented!("pushing traces aside in 90-degree mode is not ported");
        }
        // Lay each piece round the shape, pushing in turn what is in its
        // way, then put it on the board and tidy it.
        let mut spring_depth = max_spring_over_recursion_depth;
        while let Some(mut piece) = entries.next_substitute_trace_piece(self) {
            if piece.polyline.first_corner().java_equals(&piece.polyline.last_corner()) {
                continue;
            }
            if spring_depth > 0 {
                let compensated_half_width = piece.half_width + self.plain_compensation(piece.cl_class, layer);
                match self.spring_over(&piece.polyline, compensated_half_width, layer, &piece.nets, piece.cl_class, false, spring_depth, None) {
                    Spring::Fail => return false,
                    Spring::Same => {}
                    Spring::New(polyline) => {
                        spring_depth -= 1;
                        piece.polyline = polyline;
                    }
                }
            }
            for i in 0..piece.polyline.lines.len() - 2 {
                let (shape, from_side) = self.substitute_shape_and_from_side(&piece, i, false);
                if !self.shove_insert(&shape, from_side, layer, &piece.nets, piece.cl_class, max_recursion_depth - 1, max_via_recursion_depth, spring_depth) {
                    return false;
                }
            }
            for i in 0..piece.polyline.corner_count() {
                self.join_changed_area(piece.polyline.corner_float(i), layer);
            }
            let end_corners = [piece.polyline.first_corner(), piece.polyline.last_corner()];
            let item = self.insert_numbered(Item {
                id: piece.id,
                kind: ItemKind::Trace { layer, half_width: piece.half_width, polyline: piece.polyline },
                first_layer: layer,
                last_layer: layer,
                clearance_class: piece.cl_class,
                fixed: FixedState::Unfixed,
                component: 0,
                nets: piece.nets.clone(),
            });
            // The Java normalizes in the changed area, and would throw, and
            // pass over the throw, without one.
            if let Some(clip) = self.changed_area_on(layer) {
                let _ = self.normalize(item, Some(&clip));
            }
            if !tails_exist_before {
                for corner in &end_corners {
                    if let Some(tail) = self.get_trace_tail(corner, layer, &piece.nets) {
                        let connection = self.connection_items(tail, StopConnection::Via);
                        self.remove_items(&connection, false);
                        for &net in &piece.nets {
                            self.combine_traces(net);
                        }
                    }
                }
            }
        }
        true
    }

    /// Whether a trace among `items` not of `nets` ends at nothing.
    /// `BasicBoard.contains_trace_tails`.
    fn contains_trace_tails(&self, items: &[usize], nets: &[i32]) -> bool {
        items.iter().any(|&i| self.is_trace(i) && !nets_equal(&self.board.items[i].nets, nets) && self.is_tail(i))
    }

    /// Cut the traces of other nets among `items` out of `shape`, grown by
    /// each one's half width and clearance. `ShapeTraceEntries.cutout_traces`.
    fn cutout_traces(&mut self, items: &[usize], shape: &TileShape, own_nets: &[i32], cl_class: i32) {
        for &i in items {
            if self.is_trace(i) && !self.board.items[i].shares_net_no(own_nets) {
                self.cutout_trace(i, shape, cl_class);
            }
        }
    }

    /// `ShapeTraceEntries.cutout_trace`.
    fn cutout_trace(&mut self, trace: usize, shape: &TileShape, cl_class: i32) {
        if !self.is_on_board(trace) {
            return;
        }
        let (polyline, layer, half_width) = self.trace(trace);
        let polyline = polyline.clone();
        let it = &self.board.items[trace];
        let (nets, class) = (it.nets.clone(), it.clearance_class);
        // Grown in two steps, for symmetry.
        let cl_offset = self.clearance_value(class, cl_class, layer) as f64 + OFFSET_ADD;
        let offset_shape = shape.offset(half_width as f64).offset(cl_offset);
        let Cutout::Pieces(pieces) = offset_shape.cutout_polyline(&polyline) else { return };
        if pieces.len() == 2 && offset_shape.is_outside(&pieces[0].first_corner()) && offset_shape.is_outside(&pieces[1].last_corner()) {
            let [start, end]: [Polyline; 2] = pieces.try_into().expect("two pieces");
            self.fast_cutout_trace(trace, start, end);
        } else {
            self.remove_item(trace);
            for piece in pieces {
                self.insert_trace_without_cleaning(piece, layer, half_width, &nets, class, FixedState::Unfixed);
            }
        }
    }

    /// The common cut: the trace's middle out, its two ends left as new
    /// traces keeping its tree entries. `ShapeTraceEntries.fast_cutout_trace`.
    fn fast_cutout_trace(&mut self, trace: usize, start_piece: Polyline, end_piece: Polyline) {
        let (_, layer, half_width) = self.trace(trace);
        let it = &self.board.items[trace];
        let (nets, class) = (it.nets.clone(), it.clearance_class);
        let piece_item = |rb: &mut RoutingBoard, polyline: Polyline| {
            let id = rb.skip_id();
            rb.push_item(Item {
                id,
                kind: ItemKind::Trace { layer, half_width, polyline },
                first_layer: layer,
                last_layer: layer,
                clearance_class: class,
                fixed: FixedState::Unfixed,
                component: 0,
                nets: nets.clone(),
            })
        };
        let start = piece_item(self, start_piece);
        let end = piece_item(self, end_piece);
        self.reuse_entries_after_cutout(trace, start, end);
        self.remove_item(trace);
    }

    /// `MoveDrillItemAlgo.shove_vias`, for when nothing needs pushing: the
    /// vias of other nets in the way are pushed aside, which is not ported.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn shove_vias_aside(&self, shape: &TileShape, from_side: Option<FromSide>, layer: i32, nets: &[i32], cl_class: i32, ignore: &[usize], max_via_recursion_depth: i32, copper_sharing_allowed: bool) -> bool {
        let mut entries = ShapeTraceEntries::new(shape, layer, nets, cl_class, from_side);
        let obstacles = self.overlapping_items_with_clearance(shape, layer, &[], cl_class);
        if !entries.store_items(self, &obstacles, false, copper_sharing_allowed) {
            return true;
        }
        entries.shove_vias.retain(|v| !ignore.contains(v));
        for &via in &entries.shove_vias {
            if self.board.items[via].shares_net_no(nets) {
                continue;
            }
            if max_via_recursion_depth <= 0 {
                return true;
            }
            unimplemented!("pushing a via aside (MoveDrillItemAlgo.shove_vias) is not ported yet");
        }
        true
    }

    /// The trace pieces a pad of `shape` would push round it, for
    /// `ForcedPadAlgo.forced_pad`: `None` if something cannot be pushed or
    /// a via is in the way.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn stored_pad_entries(&self, shape: &TileShape, from_side: Option<FromSide>, layer: i32, nets: &[i32], cl_class: i32, obstacles: &[usize], copper_sharing_allowed: bool) -> Option<i32> {
        let mut entries = ShapeTraceEntries::new(shape, layer, nets, cl_class, from_side);
        if !(entries.store_items(self, obstacles, true, copper_sharing_allowed) && entries.shove_vias.is_empty()) {
            return None;
        }
        Some(entries.trace_piece_count)
    }

    /// What touches a pin of the net on `layer` within `shape`: exempt from
    /// the push, as it shares the pin's copper.
    /// `ShoveTraceAlgo.get_ignore_items_at_tie_pins`.
    fn ignore_items_at_tie_pins(&self, shape: &TileShape, layer: i32, nets: &[i32]) -> Vec<usize> {
        let mut result = Vec::new();
        for i in self.overlapping_objects(shape, layer) {
            let item = &self.board.items[i];
            if matches!(item.kind, ItemKind::Pin { .. }) && item.shares_net_no(nets) {
                result.extend(self.all_contacts_on(i, layer));
            }
        }
        self.sort_items(&mut result);
        result
    }

    /// Wrap `polyline` round the first obstacle in its way that cannot be
    /// pushed, counter-clockwise, and on round the next, up to `depth`
    /// times. With `contact_pins`, pins of the net other than those count
    /// as obstacles too. `ShoveTraceAlgo.spring_over`.
    #[allow(clippy::too_many_arguments)]
    pub fn spring_over(&self, polyline: &Polyline, half_width: i64, layer: i32, nets: &[i32], cl_class: i32, over_connected_pins: bool, depth: i32, contact_pins: Option<&[usize]>) -> Spring {
        let mut current = polyline.clone();
        let mut changed = false;
        let mut depth = depth;
        loop {
            match self.spring_over_once(&current, half_width, layer, nets, cl_class, over_connected_pins, depth, contact_pins) {
                SpringStep::NoObstacle => return if changed { Spring::New(current) } else { Spring::Same },
                SpringStep::Fail => return Spring::Fail,
                SpringStep::Wrapped(p) => {
                    current = p;
                    changed = true;
                    depth -= 1;
                }
            }
        }
    }

    /// One step of [`spring_over`](Self::spring_over).
    #[allow(clippy::too_many_arguments)]
    fn spring_over_once(&self, polyline: &Polyline, half_width: i64, layer: i32, nets: &[i32], cl_class: i32, over_connected_pins: bool, depth: i32, contact_pins: Option<&[usize]>) -> SpringStep {
        let board = &self.board;
        let check_nets: &[i32] = if contact_pins.is_none() { nets } else { &[] };
        let mut found: Option<(usize, IntBox)> = None;
        for i in 0..polyline.lines.len().saturating_sub(2) {
            let Some(shape) = polyline.offset_shape(half_width, i) else { continue };
            for obstacle in self.overlapping_items_with_clearance(&shape, layer, check_nets, cl_class) {
                let item = &board.items[obstacle];
                let is_obstacle = if item.shares_net_no(nets) {
                    matches!(item.kind, ItemKind::Pin { .. }) && contact_pins.is_some_and(|p| !p.contains(&obstacle))
                } else {
                    match &item.kind {
                        ItemKind::Area { kind: AreaKind::Conduction { is_obstacle }, .. } => *is_obstacle,
                        ItemKind::Area { kind: AreaKind::ViaKeepout | AreaKind::ComponentKeepout, .. } => false,
                        ItemKind::Trace { .. } => {
                            if item.is_shove_fixed(&board.rules) {
                                // A shove-fixed exit stub is passed over at a
                                // tie pin.
                                !self.normal_contacts(obstacle).iter().any(|&c| board.items[c].shares_net_no(nets))
                            } else {
                                false
                            }
                        }
                        _ => !item.is_routable(),
                    }
                };
                if !is_obstacle {
                    continue;
                }
                match found {
                    None => found = Some((obstacle, self.item_bounding_box(obstacle))),
                    Some((f, fbox)) if f != obstacle => {
                        // Of two obstacles one inside the other, the bigger
                        // one counts: fixed vias inside pins.
                        let cbox = self.item_bounding_box(obstacle);
                        if boxes_intersect(&fbox, &cbox) {
                            if box_contains(&cbox, &fbox) {
                                found = Some((obstacle, cbox));
                            } else if !box_contains(&fbox, &cbox) {
                                return SpringStep::Fail;
                            }
                        }
                    }
                    _ => {}
                }
            }
            if found.is_some() {
                break;
            }
        }
        let Some((obstacle, _)) = found else { return SpringStep::NoObstacle };
        let item = &board.items[obstacle];
        if depth <= 0 || matches!(item.kind, ItemKind::Outline { .. }) || (matches!(item.kind, ItemKind::Trace { .. }) && !item.is_shove_fixed(&board.rules)) {
            return SpringStep::Fail;
        }
        let mut try_spring_over = true;
        if !over_connected_pins && self.all_contacts_on(obstacle, layer).iter().any(|&c| self.is_trace(c)) {
            try_spring_over = false;
        }
        let mut obstacle_shape = None;
        if try_spring_over {
            match &item.kind {
                ItemKind::Area { .. } | ItemKind::Trace { .. } => {
                    if self.tree.shape_count(obstacle) == 1 {
                        obstacle_shape = self.tree.get_shape(obstacle, 0).cloned();
                    } else {
                        try_spring_over = false;
                    }
                }
                ItemKind::Pin { .. } | ItemKind::Via { .. } => {
                    obstacle_shape = self.tree.get_shape(obstacle, (layer - item.first_layer) as u32).cloned();
                }
                _ => {}
            }
        }
        if !try_spring_over {
            return SpringStep::Fail;
        }
        let Some(obstacle_shape) = obstacle_shape else { return SpringStep::Fail };
        // Grown in two steps, for symmetry.
        let offset = (half_width + 1) as f64;
        let half_cl_offset = 0.5 * self.clearance_value(item.clearance_class, cl_class, layer) as f64;
        let offset_shape = obstacle_shape.enlarge(offset + half_cl_offset).enlarge(half_cl_offset);
        let offset_shape = TileShape::Octagon(offset_shape.bounding_octagon().expect("a bounded obstacle"));
        if offset_shape.contains_inside(&polyline.first_corner()) || offset_shape.contains_inside(&polyline.last_corner()) {
            return SpringStep::Fail;
        }
        let entries = offset_shape.entrance_points(polyline);
        if entries.is_empty() {
            return SpringStep::NoObstacle;
        }
        if entries.len() < 2 {
            return SpringStep::Fail;
        }
        let pieces = match offset_shape.cutout_polyline(polyline) {
            Cutout::Untouched => vec![polyline.clone()],
            Cutout::Pieces(p) => p,
        };
        let edge_count = offset_shape.border_line_count() as i64;
        let (first_line_no, first_side_no) = entries[0];
        let (last_line_no, last_side_no) = entries[entries.len() - 1];
        let mut side_diff = last_side_no as i64 - first_side_no as i64;
        if side_diff < 0 {
            side_diff += edge_count;
        } else if side_diff == 0 {
            let compare_corner = offset_shape.corner_approx(first_side_no);
            let (x1, y1) = polyline.lines[first_line_no].intersection_approx(&offset_shape.border_line(first_side_no));
            let (x2, y2) = polyline.lines[last_line_no].intersection_approx(&offset_shape.border_line(last_side_no));
            if compare_corner.distance(&FloatPoint::new(x2, y2)) < compare_corner.distance(&FloatPoint::new(x1, y1)) {
                side_diff += edge_count;
            }
        }
        let mut substitute: Vec<Line> = Vec::with_capacity(side_diff as usize + 3);
        substitute.push(polyline.lines[first_line_no]);
        let mut curr_edge = first_side_no;
        for _ in 1..=side_diff + 1 {
            substitute.push(offset_shape.border_line(curr_edge));
            curr_edge = if curr_edge == edge_count as usize - 1 { 0 } else { curr_edge + 1 };
        }
        substitute.push(polyline.lines[last_line_no]);
        let substitute_polyline = Polyline::from_lines(&substitute);
        let mut result = substitute_polyline.clone();
        if !pieces.is_empty() {
            result = pieces[0].combine(&substitute_polyline).unwrap_or_else(|| pieces[0].clone());
        }
        if pieces.len() > 1 {
            result = result.combine(&pieces[1]).unwrap_or(result);
        }
        SpringStep::Wrapped(result)
    }

    /// Wrap `polyline` round what is in its way both ways, and keep the
    /// shorter: [`Spring::Same`] if nothing is in the way.
    /// `ShoveTraceAlgo.spring_over_obstacles`.
    pub fn spring_over_obstacles(&self, polyline: &Polyline, half_width: i64, layer: i32, nets: &[i32], cl_class: i32, contact_pins: Option<&[usize]>) -> Spring {
        const MAX_SPRING_OVER_RECURSION_DEPTH: i32 = 20;
        let counter_clockwise = self.spring_over(polyline, half_width, layer, nets, cl_class, true, MAX_SPRING_OVER_RECURSION_DEPTH, contact_pins);
        if counter_clockwise == Spring::Same {
            return Spring::Same;
        }
        let reversed = polyline.reverse();
        let clockwise = match self.spring_over(&reversed, half_width, layer, nets, cl_class, true, MAX_SPRING_OVER_RECURSION_DEPTH, contact_pins) {
            Spring::Same => Some(reversed),
            Spring::New(p) => Some(p),
            Spring::Fail => None,
        };
        let counter_clockwise = match counter_clockwise {
            Spring::New(p) => Some(p),
            _ => None,
        };
        match (clockwise, counter_clockwise) {
            (Some(cw), Some(ccw)) => {
                if cw.length_approx() <= ccw.length_approx() {
                    Spring::New(cw.reverse())
                } else {
                    Spring::New(ccw)
                }
            }
            (Some(cw), None) => Spring::New(cw.reverse()),
            (None, Some(ccw)) => Spring::New(ccw),
            (None, None) => Spring::Fail,
        }
    }

    /// `Item.bounding_box`, per kind: a pin's or via's pads, a trace's
    /// corners grown by its half width, an area's outline.
    pub fn item_bounding_box(&self, item: usize) -> IntBox {
        let it = &self.board.items[item];
        match &it.kind {
            ItemKind::Pin { pads, .. } | ItemKind::Via { pads, .. } => {
                let mut result = EMPTY_BOX;
                for pad in pads.iter().flatten() {
                    result = box_union(&result, &pad.bounding_box());
                }
                result
            }
            ItemKind::Trace { polyline, half_width, .. } => polyline.bounding_box().offset(*half_width),
            ItemKind::Area { shape, .. } => shape.bounding_box(),
            ItemKind::Outline { shapes, .. } => {
                let mut result = EMPTY_BOX;
                for lines in shapes {
                    result = box_union(&result, &TileShape::from_lines(lines).bounding_box());
                }
                result
            }
            ItemKind::ComponentOutline => EMPTY_BOX,
        }
    }
}

enum SpringStep {
    NoObstacle,
    Wrapped(Polyline),
    Fail,
}

/// `IntBox.EMPTY`.
const EMPTY_BOX: IntBox = IntBox::new(i32::MAX as i64, i32::MAX as i64, i32::MIN as i64, i32::MIN as i64);

fn box_union(a: &IntBox, b: &IntBox) -> IntBox {
    IntBox::new(a.ll.x.min(b.ll.x), a.ll.y.min(b.ll.y), a.ur.x.max(b.ur.x), a.ur.y.max(b.ur.y))
}

fn boxes_intersect(a: &IntBox, b: &IntBox) -> bool {
    a.ll.x <= b.ur.x && b.ll.x <= a.ur.x && a.ll.y <= b.ur.y && b.ll.y <= a.ur.y
}

/// `IntBox.contains(IntBox)`.
fn box_contains(a: &IntBox, b: &IntBox) -> bool {
    b.ll.x >= a.ll.x && b.ll.y >= a.ll.y && b.ur.x <= a.ur.x && b.ur.y <= a.ur.y
}

pub use crate::rules::CLEARANCE_SAFETY_MARGIN;

/// The side of the via's octagon the room reaches it from: the first of
/// the points `dist` away from it, square then diagonally in FreeRouting's
/// order, that lies inside the room, with the matching point on the via's
/// border. `None` if none does. `ForcedViaAlgo.calculate_from_side`, in
/// 45 degree mode.
fn via_from_side(location: &FloatPoint, via_shape: &TileShape, room: &TileShape, dist: f64) -> Option<FromSide> {
    let via_box = via_shape.bounding_box();
    let (x, y) = (location.x, location.y);
    let square = [
        ((x, y - dist), (x, via_box.ll.y as f64)),
        ((x + dist, y), (via_box.ur.x as f64, y)),
        ((x, y + dist), (x, via_box.ur.y as f64)),
        ((x - dist, y), (via_box.ll.x as f64, y)),
    ];
    for (i, ((cx, cy), (bx, by))) in square.into_iter().enumerate() {
        if room.contains_float(&FloatPoint::new(cx, cy)) {
            return Some(FromSide { no: 2 * i as i32, border_intersection: Some(FloatPoint::new(bx, by)) });
        }
    }
    let d = dist / std::f64::consts::SQRT_2;
    let b = box_max_width(&via_box) / (2.0 * std::f64::consts::SQRT_2);
    let diagonal = [((x + d, y - d), (x + b, y - b)), ((x + d, y + d), (x + b, y + b)), ((x - d, y + d), (x - b, y + b)), ((x - d, y - d), (x - b, y - b))];
    for (i, ((cx, cy), (bx, by))) in diagonal.into_iter().enumerate() {
        if room.contains_float(&FloatPoint::new(cx, cy)) {
            return Some(FromSide { no: 2 * i as i32 + 1, border_intersection: Some(FloatPoint::new(bx, by)) });
        }
    }
    None
}

/// `IntBox.max_width`.
fn box_max_width(b: &IntBox) -> f64 {
    ((b.ur.x - b.ll.x) as f64).max((b.ur.y - b.ll.y) as f64)
}

/// A point where a trace crosses into the shape being pushed into.
/// `ShapeTraceEntries.EntryPoint`.
#[derive(Debug, Clone)]
struct EntryPoint {
    trace: usize,
    trace_line_no: usize,
    entry_approx: FloatPoint,
    edge_no: usize,
    stack_level: i32,
    next: Option<usize>,
}

/// The traces crossing a shape to be pushed into, sorted round its border,
/// and the vias in it: what a push would have to move.
/// `ShapeTraceEntries`, its linked list kept as indices into `nodes`.
struct ShapeTraceEntries<'a> {
    shape: &'a TileShape,
    layer: i32,
    own_nets: &'a [i32],
    cl_class: i32,
    from_side: Option<FromSide>,
    nodes: Vec<EntryPoint>,
    anchor: Option<usize>,
    trace_piece_count: i32,
    max_stack_level: i32,
    shape_contains_trace_tails: bool,
    shove_vias: Vec<usize>,
}

impl<'a> ShapeTraceEntries<'a> {
    fn new(shape: &'a TileShape, layer: i32, own_nets: &'a [i32], cl_class: i32, from_side: Option<FromSide>) -> Self {
        ShapeTraceEntries {
            shape,
            layer,
            own_nets,
            cl_class,
            from_side,
            nodes: Vec::new(),
            anchor: None,
            trace_piece_count: 0,
            max_stack_level: 0,
            shape_contains_trace_tails: false,
            shove_vias: Vec::new(),
        }
    }

    fn nets<'r>(&self, rb: &'r RoutingBoard, node: usize) -> &'r [i32] {
        &rb.board.items[self.nodes[node].trace].nets
    }

    /// Sort `items` into what could be pushed aside and what blocks: false
    /// at the first blocker. `store_items`.
    fn store_items(&mut self, rb: &RoutingBoard, items: &[usize], is_pad_check: bool, copper_sharing_allowed: bool) -> bool {
        let board = &rb.board;
        for &i in items {
            let item = &board.items[i];
            let area_kind = match &item.kind {
                ItemKind::Area { kind, .. } => Some(*kind),
                _ => None,
            };
            if (!is_pad_check && area_kind == Some(AreaKind::ViaKeepout)) || area_kind == Some(AreaKind::ComponentKeepout) {
                continue;
            }
            let contains_own_net = item.shares_net_no(self.own_nets);
            if let Some(AreaKind::Conduction { is_obstacle }) = area_kind {
                if contains_own_net || !is_obstacle {
                    continue;
                }
            }
            if item.is_shove_fixed(&board.rules) && !contains_own_net {
                return false;
            }
            match &item.kind {
                ItemKind::Via { .. } => {
                    if is_pad_check || !contains_own_net {
                        self.shove_vias.push(i);
                    }
                }
                ItemKind::Trace { .. } => {
                    if !self.store_trace(rb, i) {
                        return false;
                    }
                }
                _ => {
                    if contains_own_net {
                        if !copper_sharing_allowed {
                            return false;
                        }
                        if is_pad_check && !item.drill_allowed() {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
            }
        }
        self.search_from_side(rb);
        self.resort(rb);
        self.calculate_stack_levels(rb)
    }

    /// Where the trace crosses the shape grown by its half width and
    /// clearance, sorted into the list round the border. `store_trace`.
    fn store_trace(&mut self, rb: &RoutingBoard, trace: usize) -> bool {
        let item = &rb.board.items[trace];
        let (polyline, trace_layer, half_width) = rb.trace(trace);
        let cl_offset = rb.clearance_value(item.clearance_class, self.cl_class, trace_layer) as f64 + OFFSET_ADD;
        let offset_shape = self.shape.offset(half_width as f64).offset(cl_offset);
        for (line_no, edge_no) in offset_shape.entrance_points(polyline) {
            let (x, y) = polyline.lines[line_no].intersection_approx(&offset_shape.border_line(edge_no));
            self.insert_entry_point(trace, line_no, edge_no, FloatPoint::new(x, y));
        }
        if !item.shares_net_no(self.own_nets) {
            if !rb.nets_normal(&item.nets) {
                return false;
            }
            for i in 0..2 {
                let end_corner = if i == 0 { polyline.first_corner() } else { polyline.last_corner() };
                if !offset_shape.contains_point(&end_corner) {
                    continue;
                }
                let contacts = if i == 0 { rb.start_contacts(trace) } else { rb.end_contacts(trace) };
                let mut contact_count = 0;
                let store_end_corner = true;
                for &c in &contacts {
                    let contact = &rb.board.items[c];
                    if !contact.is_routable() {
                        return false;
                    }
                    match &contact.kind {
                        ItemKind::Trace { half_width: chw, .. } => {
                            if (contact.is_shove_fixed(&rb.board.rules) || *chw != half_width || contact.clearance_class != item.clearance_class) && offset_shape.contains_inside(&end_corner) {
                                return false;
                            }
                        }
                        ItemKind::Via { .. } => unimplemented!("a trace of another net ending at a via inside a pushed shape is not ported yet"),
                        _ => {}
                    }
                    contact_count += 1;
                }
                if contact_count == 1 && store_end_corner {
                    unimplemented!("a trace of another net ending inside a pushed shape (nearest_border_point) is not ported yet");
                } else if contact_count == 0 && offset_shape.contains_inside(&end_corner) {
                    self.shape_contains_trace_tails = true;
                }
            }
        }
        true
    }

    fn insert_entry_point(&mut self, trace: usize, trace_line_no: usize, edge_no: usize, entry_approx: FloatPoint) {
        let mut prev: Option<usize> = None;
        let mut next = self.anchor;
        while let Some(n) = next {
            if self.nodes[n].edge_no > edge_no {
                break;
            }
            if self.nodes[n].edge_no == edge_no {
                let prev_corner = self.shape.corner_approx(edge_no);
                let next_corner = if edge_no == self.shape.border_line_count() - 1 { self.shape.corner_approx(0) } else { self.shape.corner_approx(edge_no + 1) };
                if prev_corner.scalar_product(&entry_approx, &next_corner) <= prev_corner.scalar_product(&self.nodes[n].entry_approx, &next_corner) {
                    break;
                }
            }
            prev = next;
            next = self.nodes[n].next;
        }
        self.nodes.push(EntryPoint { trace, trace_line_no, entry_approx, edge_no, stack_level: -1, next });
        let new = self.nodes.len() - 1;
        match prev {
            Some(p) => self.nodes[p].next = Some(new),
            None => self.anchor = Some(new),
        }
    }

    fn search_from_side(&mut self, rb: &RoutingBoard) {
        if self.from_side.is_some_and(|f| f.no >= 0) {
            return;
        }
        let mut curr = self.anchor;
        let mut no = 0;
        let mut entry = None;
        while let Some(c) = curr {
            if rb.board.items[self.nodes[c].trace].shares_net_no(self.own_nets) {
                no = self.nodes[c].edge_no as i32;
                entry = Some(self.nodes[c].entry_approx);
                break;
            }
            curr = self.nodes[c].next;
        }
        self.from_side = Some(FromSide { no, border_intersection: entry });
    }

    /// Start the list in the middle of the side the push comes from, and
    /// drop the entries between the first and last of each run of the same
    /// nets, and the own net's at either end. `resort`.
    fn resort(&mut self, rb: &RoutingBoard) {
        let edge_count = self.shape.border_line_count();
        let Some(mut from_side) = self.from_side else { return };
        if from_side.no < 0 || from_side.no as usize >= edge_count {
            return;
        }
        let fs_no = from_side.no as usize;
        let compare_corner_1 = self.shape.corner_approx(fs_no);
        let compare_corner_2 = if fs_no == edge_count - 1 { self.shape.corner_approx(0) } else { self.shape.corner_approx(fs_no + 1) };
        let mut from_point_dist = 0.0;
        let mut from_point_projection = None;
        if let Some(bi) = from_side.border_intersection {
            let projection = bi.projection_approx(&self.shape.border_line(fs_no));
            from_point_dist = projection.distance_square(&compare_corner_1);
            if from_point_dist >= compare_corner_1.distance_square(&compare_corner_2) {
                from_side = FromSide { no: from_side.no, border_intersection: None };
                self.from_side = Some(from_side);
            }
            from_point_projection = Some(projection);
        }
        let mut curr = self.anchor;
        while let Some(c) = curr {
            let node = &self.nodes[c];
            if node.edge_no > fs_no {
                break;
            }
            if node.edge_no == fs_no {
                if from_side.border_intersection.is_some() {
                    let projection = node.entry_approx.projection_approx(&self.shape.border_line(fs_no));
                    let fpp = from_point_projection.expect("set with the border intersection");
                    if projection.distance_square(&compare_corner_1) >= from_point_dist && projection.distance_square(&fpp) <= projection.distance_square(&compare_corner_1) {
                        break;
                    }
                } else if node.entry_approx.distance_square(&compare_corner_2) <= node.entry_approx.distance_square(&compare_corner_1) {
                    break;
                }
            }
            curr = node.next;
        }
        if let Some(new_anchor) = curr {
            if Some(new_anchor) != self.anchor {
                let mut last = new_anchor;
                while let Some(n) = self.nodes[last].next {
                    last = n;
                }
                self.nodes[last].next = self.anchor;
                let mut c = self.anchor.expect("a list");
                let mut prev = last;
                while c != new_anchor {
                    self.nodes[c].edge_no += edge_count;
                    prev = c;
                    c = self.nodes[c].next.expect("the list runs round to the new anchor");
                }
                self.nodes[prev].next = None;
                self.anchor = Some(new_anchor);
            }
        }
        // Drop the entries between the first and last of each run.
        let Some(anchor) = self.anchor else { return };
        let mut prev = anchor;
        let mut prev_nets = self.nets(rb, prev).to_vec();
        let mut curr = self.nodes[anchor].next;
        let (mut curr_nets, mut next) = match curr {
            Some(c) => (self.nets(rb, c).to_vec(), self.nodes[c].next),
            None => (Vec::new(), None),
        };
        let mut before_prev: Option<usize> = None;
        while let Some(n) = next {
            let next_nets = self.nets(rb, n).to_vec();
            if net_nos_equal(&prev_nets, &curr_nets) && net_nos_equal(&curr_nets, &next_nets) {
                self.nodes[prev].next = Some(n);
            } else {
                before_prev = Some(prev);
                prev = curr.expect("curr precedes next");
                prev_nets = curr_nets;
            }
            curr_nets = next_nets;
            curr = Some(n);
            next = self.nodes[n].next;
        }
        // Drop the own net's entries at the end, then at the start.
        if curr.is_some() && net_nos_equal(&curr_nets, self.own_nets) {
            self.nodes[prev].next = None;
            if net_nos_equal(&prev_nets, self.own_nets) {
                match before_prev {
                    Some(b) => self.nodes[b].next = None,
                    None => self.anchor = None,
                }
            }
        }
        if let Some(a) = self.anchor {
            if nets_equal(self.nets(rb, a), self.own_nets) {
                self.anchor = self.nodes[a].next;
                if let Some(a) = self.anchor {
                    if nets_equal(self.nets(rb, a), self.own_nets) {
                        self.anchor = self.nodes[a].next;
                    }
                }
            }
        }
    }

    /// How deep each crossing trace is stacked in the push; false where the
    /// traces do not nest. `calculate_stack_levels`.
    fn calculate_stack_levels(&mut self, rb: &RoutingBoard) -> bool {
        let Some(anchor) = self.anchor else { return true };
        let mut curr_entry = Some(anchor);
        let mut curr_nets = self.nets(rb, anchor).to_vec();
        let mut curr_level = if net_nos_equal(&curr_nets, self.own_nets) { 0 } else { 1 };
        while let Some(ce) = curr_entry {
            if self.nodes[ce].stack_level < 0 {
                self.trace_piece_count += 1;
                self.nodes[ce].stack_level = curr_level;
                if curr_level > self.max_stack_level {
                    self.max_stack_level = curr_level;
                }
            }
            let mut check = self.nodes[ce].next;
            let mut index_of_next_foreign_set = 0;
            let mut index_of_last_occurrence_of_set = 0;
            let mut next_index = 0;
            let mut last_own_entry = None;
            let mut first_foreign_entry = None;
            while let Some(ck) = check {
                next_index += 1;
                if net_nos_equal(self.nets(rb, ck), &curr_nets) {
                    index_of_last_occurrence_of_set = next_index;
                    last_own_entry = Some(ck);
                    self.nodes[ck].stack_level = self.nodes[ce].stack_level;
                } else if index_of_next_foreign_set == 0 {
                    index_of_next_foreign_set = next_index;
                    first_foreign_entry = Some(ck);
                }
                check = self.nodes[ck].next;
            }
            if next_index == 0 {
                curr_entry = None;
                continue;
            }
            let next_entry;
            if index_of_next_foreign_set != 0 && index_of_next_foreign_set < index_of_last_occurrence_of_set {
                next_entry = first_foreign_entry.expect("a foreign entry");
                if self.nodes[next_entry].stack_level >= 0 {
                    return false;
                }
                curr_level += 1;
            } else if index_of_last_occurrence_of_set != 0 {
                next_entry = last_own_entry.expect("an own entry");
            } else {
                next_entry = first_foreign_entry.expect("a foreign entry");
                if self.nodes[next_entry].stack_level >= 0 {
                    curr_level -= 1;
                    if self.nodes[next_entry].stack_level != curr_level {
                        return false;
                    }
                }
            }
            curr_nets = self.nets(rb, next_entry).to_vec();
            self.nodes[ce].next = Some(next_entry);
            curr_entry = Some(next_entry);
        }
        curr_level == 1
    }

    /// The next run of entries of the deepest level, as the trace piece to
    /// go round the shape, the own net's skipped. `pop_piece`.
    fn pop_piece(&mut self, rb: &RoutingBoard) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        let mut prev_first = None;
        let mut first = Some(anchor);
        while let Some(f) = first {
            if self.nodes[f].stack_level == self.max_stack_level {
                break;
            }
            prev_first = first;
            first = self.nodes[f].next;
        }
        let first = first?;
        let mut last = first;
        let mut after_last = self.nodes[first].next;
        while let Some(a) = after_last {
            if self.nodes[a].stack_level == self.max_stack_level && nets_equal(self.nets(rb, a), self.nets(rb, first)) {
                last = a;
                after_last = self.nodes[a].next;
            } else {
                break;
            }
        }
        match prev_first {
            Some(p) => self.nodes[p].next = after_last,
            None => self.anchor = after_last,
        }
        self.max_stack_level = 0;
        let mut c = self.anchor;
        while let Some(n) = c {
            self.max_stack_level = self.max_stack_level.max(self.nodes[n].stack_level);
            c = self.nodes[n].next;
        }
        self.trace_piece_count -= 1;
        if nets_equal(self.nets(rb, first), self.own_nets) {
            return self.pop_piece(rb);
        }
        Some((first, last))
    }

    /// The next piece of trace to be pushed round the shape: from the
    /// trace's line where it enters, along the shape grown by the trace's
    /// half width and clearance, to its line where it leaves. Pieces that
    /// come out empty are passed over. `next_substitute_trace_piece`.
    fn next_substitute_trace_piece(&mut self, rb: &RoutingBoard) -> Option<SubstituteTrace> {
        loop {
            let (first, last) = self.pop_piece(rb)?;
            let trace = self.nodes[first].trace;
            let item = &rb.board.items[trace];
            let (_, _, half_width) = rb.trace(trace);
            // Grown in two steps, for symmetry.
            let cl_offset = rb.clearance_value(item.clearance_class, self.cl_class, self.layer) as f64 + OFFSET_ADD;
            let offset_shape = self.shape.offset(half_width as f64).offset(cl_offset);
            let edge_count = self.shape.border_line_count();
            let edge_diff = self.nodes[last].edge_no as i64 - self.nodes[first].edge_no as i64;
            let len = usize::try_from(edge_diff + 3).expect("a piece of at least no lines");
            let mut lines: Vec<Option<Line>> = vec![None; len];
            if len > 0 {
                lines[0] = Some(rb.trace(trace).0.lines[self.nodes[first].trace_line_no]);
                lines[len - 1] = Some(rb.trace(self.nodes[last].trace).0.lines[self.nodes[last].trace_line_no]);
            }
            let mut curr_edge = self.nodes[first].edge_no % edge_count;
            for line in lines.iter_mut().take(len.saturating_sub(1)).skip(1) {
                *line = Some(offset_shape.border_line(curr_edge));
                curr_edge = if curr_edge == edge_count - 1 { 0 } else { curr_edge + 1 };
            }
            let lines: Vec<Line> = lines.into_iter().map(|l| l.expect("every line set")).collect();
            let polyline = Polyline::from_lines(&lines);
            if polyline.is_empty() {
                continue;
            }
            let id = rb.skip_id();
            return Some(SubstituteTrace { id, polyline, layer: self.layer, half_width, nets: item.nets.clone(), cl_class: item.clearance_class });
        }
    }
}

/// `ShapeTraceEntries.c_offset_add`.
const OFFSET_ADD: f64 = 1.0;

/// `ShapeTraceEntries.net_nos_equal`: the same nets, by set.
fn net_nos_equal(a: &[i32], b: &[i32]) -> bool {
    a.len() == b.len() && a.iter().all(|n| b.contains(n))
}
