//! Adapts this crate's `DrcBoard` into `eda_zone_filler::FillInput`, and
//! runs the real filler. This is the one place board-level geometry
//! (pads/tracks/vias/other zones) gets adapted into `eda_zone_filler`'s
//! input shapes, so `eda_connectivity`, `eda_kicad` and this crate's own
//! `copper_clearance` provider all call into it rather than duplicating the
//! adapter (see the task's stage 4: zone fills power both connectivity and
//! DRC, plus the `.kicad_pcb` export).
//!
//! Takes a `&DrcBoard` (already built by the caller) rather than
//! `&Design`/`&ConstraintModel` directly, for two reasons: it avoids a
//! second, redundant `board::build` pass when the caller already has one
//! (every caller does), and it sidesteps a real recursion risk --
//! `copper_clearance::check` takes a `&DrcBoard` and wants to call in here
//! too, and `board::build` must never (even transitively) call back into
//! itself. `DrcZone` therefore carries the handful of `ZONE_SETTINGS`
//! fields (`clearance`, `min_thickness`, ...) `fill_zone` needs, alongside
//! its usual `net`/`layer`/`outline`.

use crate::board::DrcBoard;
use crate::kimath::Shape as DrcShape;
use eda_clipper2::Point64;
use eda_model::ir::{Point, Zone};
use eda_model::BoardRules;
use eda_shape_poly_set::ShapePolySet;
use eda_zone_filler::shape::Shape as FillShape;
use eda_zone_filler::{fill_zone, FillInput, FillPad, FillTrack, FillVia, FillZoneRef, DEFAULT_MAX_ERROR};
use std::collections::HashMap;

#[inline]
fn pt(p: Point) -> Point64 {
    Point64::new(p.x, p.y)
}

fn convert_shape(s: &DrcShape) -> FillShape {
    match s {
        DrcShape::Circle { c, r } => FillShape::Circle { c: pt(*c), r: *r },
        DrcShape::Stadium { a, b, r } => FillShape::Stadium { a: pt(*a), b: pt(*b), r: *r },
        DrcShape::Rect { x0, y0, x1, y1 } => FillShape::Rect { x0: *x0, y0: *y0, x1: *x1, y1: *y1 },
        DrcShape::RoundRect { x0, y0, x1, y1, r } => FillShape::RoundRect { x0: *x0, y0: *y0, x1: *x1, y1: *y1, r: *r },
        DrcShape::Polygon { pts } => FillShape::Polygon { pts: pts.iter().map(|&p| pt(p)).collect() },
        // Only ever a `SilkItem` shape (stroked text) in this workspace's
        // model -- never a pad/track/via's copper or hole -- so this arm is
        // unreachable in practice; an empty polygon is a harmless knockout
        // no-op if it's ever hit.
        DrcShape::Strokes { .. } => FillShape::Polygon { pts: Vec::new() },
    }
}

/// One zone's computed fill.
pub struct ZoneFill {
    pub fill: ShapePolySet,
}

pub struct FillResults {
    /// Keyed by `Zone::id` (every zone filled here is expected to already
    /// have one -- see `RoutingSection::assign_missing_ids`).
    pub zones: HashMap<String, ZoneFill>,
}

impl FillResults {
    pub fn get(&self, zone_id: &str) -> Option<&ShapePolySet> {
        self.zones.get(zone_id).map(|z| &z.fill)
    }

    /// Every disjoint fragment of `zone_id`'s fill as a `Shape::Polygon`,
    /// for a clearance/collision test -- `fallback` (typically the zone's
    /// raw outline) when there's no fill on record for it (no stable id, or
    /// the zone wasn't included in this `FillResults`).
    pub fn fragments_or(&self, zone_id: &str, fallback: DrcShape) -> Vec<DrcShape> {
        match self.get(zone_id) {
            Some(fill) if !fill.polys.is_empty() => {
                fill.polys.iter().filter_map(|poly| poly.first()).filter(|c| c.len() >= 3).map(|chain| DrcShape::Polygon { pts: chain.iter().map(|p| Point { x: p.x, y: p.y }).collect() }).collect()
            }
            _ => vec![fallback],
        }
    }
}

/// `ZONE_FILLER::Fill`, for every zone in `board.zones`: each zone's own
/// layer (this model has no multi-layer zones) via `eda_zone_filler::fill_zone`.
pub fn fill_all_zones(board: &DrcBoard, rules: &BoardRules) -> FillResults {
    let board_outline: Option<Vec<Point64>> = if board.outline.len() >= 3 { Some(board.outline.iter().map(|&p| pt(p)).collect()) } else { None };

    let pads: Vec<FillPad> =
        board.pads.iter().map(|p| FillPad { net: p.net.clone(), layers: p.layers.clone(), copper: convert_shape(&p.copper), hole: p.hole.as_ref().map(convert_shape) }).collect();
    let tracks: Vec<FillTrack> = board.tracks.iter().map(|t| FillTrack { net: t.net.clone(), layer: t.layer.clone(), a: pt(t.a), b: pt(t.b), width: t.width }).collect();
    let vias: Vec<FillVia> =
        board.vias.iter().map(|v| FillVia { net: v.net.clone(), at: pt(v.at), diameter: v.diameter, from_layer: v.from_layer.clone(), to_layer: v.to_layer.clone(), layer_order: board.layers.clone() }).collect();

    let mut out = FillResults { zones: HashMap::new() };
    for z in &board.zones {
        let zone = Zone {
            id: z.id.clone(),
            net: z.net.clone().unwrap_or_default(),
            layer: z.layer.clone(),
            outline: z.outline.clone(),
            clearance: z.clearance,
            min_thickness: z.min_thickness,
            thermal_gap: z.thermal_gap,
            thermal_spoke_width: z.thermal_spoke_width,
            pad_connection: z.pad_connection,
            priority: z.priority,
            island_removal_mode: z.island_removal_mode,
            min_island_area: z.min_island_area,
            ..Zone::default()
        };

        let other_zones: Vec<FillZoneRef> = board
            .zones
            .iter()
            .filter(|o| !std::ptr::eq(*o, z))
            .map(|o| FillZoneRef { net: o.net.clone(), layer: o.layer.clone(), outline: o.outline.iter().map(|&p| pt(p)).collect(), priority: o.priority })
            .collect();

        let input = FillInput { pads: pads.clone(), tracks: tracks.clone(), vias: vias.clone(), other_zones, board_outline: board_outline.clone() };
        let clearance_fn = |a: Option<&str>, b: Option<&str>| crate::constraints::clearance(rules, a, b);
        let fill = fill_zone(&zone, &zone.layer, &input, clearance_fn, DEFAULT_MAX_ERROR);
        out.zones.insert(z.id.clone(), ZoneFill { fill });
    }
    out
}
