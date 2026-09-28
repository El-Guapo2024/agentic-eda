//! Whether a via or a stretch of trace could go somewhere, if what can be
//! pushed aside were pushed: the checks the maze search makes before it
//! opens a via or squeezes through a thin room. Ported from FreeRouting's
//! `ForcedViaAlgo.check_layer`, `ForcedPadAlgo.check_forced_pad`,
//! `RoutingBoard.check_forced_trace_polyline`, `ShoveTraceAlgo.check`, and
//! the part of `ShapeTraceEntries.store_items` they run on the default tree.
//!
//! Only what boards without traces reach is ported so far: pins, keepouts,
//! pours and the outline, which are pushed aside by nothing. Where a trace
//! or via would have to be shoved, these checks stop with a message naming
//! the missing piece.

use crate::geometry::{Circle, FloatPoint, IntPoint, Point, Polyline, Simplex, TileShape};
use crate::model::{AreaKind, ItemKind};

use super::engine::Engine;

/// `ForcedPadAlgo.CheckDrillResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrillCheck {
    NotDrillable,
    Drillable,
    DrillableWithAttachSmd,
}

impl Engine<'_> {
    /// Items touching `shape` on `layer` once each keeps its clearance from
    /// class `cl_class`: both shapes grown by half that clearance, safety
    /// margin included, and tested exactly. Items of `ignore_nets` are
    /// skipped. In FreeRouting's order, by item number descending.
    /// `ShapeSearchTree.overlapping_items_with_clearance` on the default
    /// tree, which is not clearance-compensated.
    pub fn overlapping_items_with_clearance(&self, shape: &TileShape, layer: i32, ignore_nets: &[i32], cl_class: i32) -> Vec<usize> {
        let board = self.board;
        let Some(bounds) = shape.bounding_octagon() else { return Vec::new() };
        // Every candidate within the largest clearance of the class: the
        // bounds grown by 1.2 times it, as enlarging octagons is not
        // symmetric.
        let max_clearance = (1.2 * board.rules.max_clearance(cl_class, layer) as f64) as i64;
        let mut result: Vec<usize> = Vec::new();
        for (i, k, l) in self.default_tree.candidates(board, &bounds.offset(max_clearance as f64)) {
            let item = &board.items[i];
            if (layer >= 0 && l != layer) || ignore_nets.iter().any(|&net| !item.is_obstacle(net)) {
                continue;
            }
            let clearance = board.rules.clearance.get(cl_class, item.clearance_class, layer) + CLEARANCE_SAFETY_MARGIN;
            let half = (clearance / 2) as f64;
            let grown = shape.enlarge(half);
            let other = self.default_tree.shape(i, k).enlarge(half);
            if grown.intersects(&other) && !result.contains(&i) {
                result.push(i);
            }
        }
        result.sort_by(|a, b| board.items[*b].id.cmp(&board.items[*a].id));
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
        let board = self.board;
        let via_shape = Circle::new(location, via_radius.ceil() as i64);
        let clearance = board.rules.clearance.get(cl_class, cl_class, layer) + CLEARANCE_SAFETY_MARGIN;
        let check_radius = via_radius + 0.5 * clearance as f64 + board.rules.min_trace_half_width as f64;
        let tile_shape = TileShape::Octagon(via_shape.bounding_octagon());
        let room = match room_shape {
            TileShape::Simplex(s) => s.clone(),
            TileShape::Octagon(o) => o.to_simplex(),
            TileShape::Box(b) => b.to_simplex(),
        };
        if !has_from_side(&FloatPoint::from_int(location), &tile_shape, &room, check_radius) {
            return DrillCheck::NotDrillable;
        }
        self.check_forced_pad(&tile_shape, layer, nets, cl_class, attach_smd_allowed, max_recursion_depth, max_via_recursion_depth)
    }

    /// Whether a pad `shape` fits on `layer`, pushing aside what can be
    /// pushed; with copper sharing, touching the net's own SMD pins is
    /// allowed and reported. `ForcedPadAlgo.check_forced_pad`.
    #[allow(clippy::too_many_arguments)]
    pub fn check_forced_pad(
        &self,
        shape: &TileShape,
        layer: i32,
        nets: &[i32],
        cl_class: i32,
        copper_sharing_allowed: bool,
        _max_recursion_depth: i32,
        max_via_recursion_depth: i32,
    ) -> DrillCheck {
        let board = self.board;
        if !shape.is_contained_in_box(&board.bounds) {
            return DrillCheck::NotDrillable;
        }
        let obstacles = self.overlapping_items_with_clearance(shape, layer, &[], cl_class);
        let Some(stored) = self.store_items(&obstacles, nets, true, copper_sharing_allowed) else {
            return DrillCheck::NotDrillable;
        };
        if !stored.shove_vias.is_empty() {
            if max_via_recursion_depth <= 0 {
                return DrillCheck::NotDrillable;
            }
            unimplemented!("pushing a via aside for a new via (MoveDrillItemAlgo) is not ported yet");
        }
        let mut result = DrillCheck::Drillable;
        if copper_sharing_allowed && obstacles.iter().any(|&i| matches!(board.items[i].kind, ItemKind::Pin { .. })) {
            result = DrillCheck::DrillableWithAttachSmd;
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
        for shape in polyline.offset_shapes(half_width, 0, n - 1) {
            // CalcFromSide(polyline, i + 1, shape) only steers the shoving of
            // traces, which is not ported; it has no other effect.
            if !self.check_trace_shape(&shape, layer, nets, cl_class, max_recursion_depth, max_via_recursion_depth, max_spring_over_recursion_depth) {
                return false;
            }
        }
        true
    }

    /// Whether a trace piece of shape `shape` fits on `layer`, pushing
    /// aside what can be pushed. `ShoveTraceAlgo.check`.
    #[allow(clippy::too_many_arguments)]
    fn check_trace_shape(&self, shape: &TileShape, layer: i32, nets: &[i32], cl_class: i32, _max_recursion_depth: i32, _max_via_recursion_depth: i32, _max_spring_over_recursion_depth: i32) -> bool {
        if shape.is_empty() {
            return true;
        }
        if !shape.is_contained_in_box(&self.board.bounds) {
            return false;
        }
        let obstacles = self.overlapping_items_with_clearance(shape, layer, &[], cl_class);
        // get_ignore_items_at_tie_pins: what touches a pin of the net; with
        // no traces or vias on the board, nothing does.
        let Some(stored) = self.store_items(&obstacles, nets, false, true) else {
            return false;
        };
        if stored.shove_vias.iter().any(|&v| !self.board.items[v].shares_net_no(nets)) {
            unimplemented!("pushing a via aside for a trace (MoveDrillItemAlgo) is not ported yet");
        }
        true
    }

    /// Sort `obstacles` into what could be pushed aside and what blocks:
    /// `None` at the first blocker. `ShapeTraceEntries.store_items`, for
    /// everything but traces.
    fn store_items(&self, obstacles: &[usize], own_nets: &[i32], is_pad_check: bool, copper_sharing_allowed: bool) -> Option<Stored> {
        let board = self.board;
        let mut stored = Stored { shove_vias: Vec::new() };
        for &i in obstacles {
            let item = &board.items[i];
            let area_kind = match &item.kind {
                ItemKind::Area { kind, .. } => Some(*kind),
                _ => None,
            };
            if (!is_pad_check && area_kind == Some(AreaKind::ViaKeepout)) || area_kind == Some(AreaKind::ComponentKeepout) {
                continue;
            }
            let contains_own_net = item.shares_net_no(own_nets);
            if let Some(AreaKind::Conduction { is_obstacle }) = area_kind {
                if contains_own_net || !is_obstacle {
                    continue;
                }
            }
            if item.is_shove_fixed(&board.rules) && !contains_own_net {
                return None;
            }
            match &item.kind {
                ItemKind::Via { .. } => {
                    if is_pad_check || !contains_own_net {
                        stored.shove_vias.push(i);
                    }
                }
                ItemKind::Trace { .. } => unimplemented!("pushing a trace aside (ShapeTraceEntries.store_trace) is not ported yet"),
                _ => {
                    if contains_own_net {
                        if !copper_sharing_allowed {
                            return None;
                        }
                        if is_pad_check && !item.drill_allowed() {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }
            }
        }
        // search_from_side, resort and calculate_stack_levels: with no trace
        // entries they change nothing and succeed.
        Some(stored)
    }
}

/// What `store_items` kept for pushing aside.
struct Stored {
    shove_vias: Vec<usize>,
}

/// `ClearanceMatrix.clearance_safety_margin`, which `board.clearance_value`
/// and the clearance searches add.
pub const CLEARANCE_SAFETY_MARGIN: i64 = 16;

/// Whether a via at `location` can be entered from the room: a point
/// `dist` away from it square or diagonally, tried in FreeRouting's order,
/// lies inside the room. `ForcedViaAlgo.calculate_from_side`, whose side
/// and border point only steer the pushing of traces.
fn has_from_side(location: &FloatPoint, via_shape: &TileShape, room: &Simplex, dist: f64) -> bool {
    let room = TileShape::Simplex(room.clone());
    let square = [(0.0, -dist), (dist, 0.0), (0.0, dist), (-dist, 0.0)];
    if square.iter().any(|&(dx, dy)| room.contains_float(&FloatPoint::new(location.x + dx, location.y + dy))) {
        return true;
    }
    let _ = via_shape;
    let d = dist / std::f64::consts::SQRT_2;
    let diagonal = [(d, -d), (d, d), (-d, d), (-d, -d)];
    diagonal.iter().any(|&(dx, dy)| room.contains_float(&FloatPoint::new(location.x + dx, location.y + dy)))
}

/// A two-point polyline: the segment with a line square to it at each
/// end. `Polyline(Point[])`, through `Polygon`, for two distinct points.
pub fn two_point_polyline(from: IntPoint, to: IntPoint) -> Polyline {
    use crate::geometry::{Direction, Line};
    let _ = Point::Int(from);
    let dir = Direction::of(to.x - from.x, to.y - from.y);
    let back = Direction::of(from.x - to.x, from.y - to.y);
    Polyline { lines: vec![Line::through(from, dir.turn_45_degree(2)), Line::new(from, to), Line::through(to, back.turn_45_degree(2))] }
}
