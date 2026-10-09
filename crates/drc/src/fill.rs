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
use eda_zone_filler::{fill_zone, FillInput, FillKeepout, FillPad, FillTrack, FillVia, FillZoneRef, PadGeometry, DEFAULT_MAX_ERROR};
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

/// A pad as the filler sees it: the exact copper and drill, and what a thermal relief needs -- the pad's own size,
/// orientation and spoke angle, and its zone-connection overrides.
fn fill_pad(p: &crate::board::DrcPad) -> FillPad {
    use eda_model::{PadKind, PadShape};
    let circular = p.shape == PadShape::Circle || (p.shape == PadShape::Oval && p.size.0 == p.size.1);
    // `PADSTACK::DefaultThermalSpokeAngleForShape`: 90 degrees for an oval or (rounded) rectangle, 45 for a circle.
    let default_angle = if matches!(p.shape, PadShape::Oval | PadShape::Rect | PadShape::RoundRect) { 90_000 } else { 45_000 };
    FillPad {
        net: p.net.clone(),
        layers: p.layers.clone(),
        copper: convert_shape(&p.copper),
        hole: p.hole.as_ref().map(convert_shape),
        geometry: Some(PadGeometry { center: pt(p.center), size: p.size, circular, default_spoke_angle_mdeg: default_angle, orientation_mdeg: p.orientation_mdeg }),
        zone_connection: p.zone.connection,
        footprint_zone_connection: p.zone.footprint_connection,
        thermal_gap: p.zone.thermal_gap,
        spoke_width: p.zone.thermal_spoke_width,
        spoke_angle_mdeg: p.zone.thermal_spoke_angle_mdeg.map(i64::from),
        plated_through_hole: p.kind == PadKind::ThroughHole,
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
        // A zone that *was* filled but came out empty (every island removed,
        // or fully knocked out) has no copper to collide with -- `ZONE::
        // GetFill()` is an empty `SHAPE_POLY_SET`, never the outline.
        match self.get(zone_id) {
            Some(fill) => fill.polys.iter().filter_map(|poly| poly.first()).filter(|c| c.len() >= 3).map(|chain| DrcShape::Polygon { pts: chain.iter().map(|p| Point { x: p.x, y: p.y }).collect() }).collect(),
            None => vec![fallback],
        }
    }
}

/// `ZONE_FILLER::Fill`, for every zone in `board.zones`: each zone's own
/// layer (this model has no multi-layer zones) via `eda_zone_filler::fill_zone`.
pub fn fill_all_zones(board: &DrcBoard, rules: &BoardRules) -> FillResults {
    let board_outline: Option<Vec<Point64>> = if board.outline.len() >= 3 { Some(board.outline.iter().map(|&p| pt(p)).collect()) } else { None };

    let pads: Vec<FillPad> = board.pads.iter().map(fill_pad).collect();
    // An arc knocks out its true curve: one `FillTrack` per chord of its
    // `ARC_HIGH_DEF` polyline (`PCB_ARC::TransformShapeToPolygon`).
    let tracks: Vec<FillTrack> = board
        .tracks
        .iter()
        .flat_map(|t| {
            let pts = match t.arc_mid {
                Some(mid) => crate::board::arc_polyline(t.a, mid, t.b, crate::board::ARC_HIGH_DEF),
                None => vec![t.a, t.b],
            };
            pts.windows(2).map(|w| FillTrack { net: t.net.clone(), layer: t.layer.clone(), a: pt(w[0]), b: pt(w[1]), width: t.width }).collect::<Vec<_>>()
        })
        .chain(board.copper_graphics.iter().flat_map(|g| {
            // `knockoutGraphicClearance`: a text's strokes, each inflated by
            // its pen half-width plus the zone clearance -- a netless track.
            match &g.shape {
                crate::kimath::Shape::Strokes { segs, r } => segs.iter().map(|s| FillTrack { net: None, layer: g.layer.clone(), a: pt(s.a), b: pt(s.b), width: 2 * r }).collect::<Vec<_>>(),
                _ => Vec::new(),
            }
        }))
        .collect();
    let vias: Vec<FillVia> =
        board.vias.iter().map(|v| FillVia { net: v.net.clone(), at: pt(v.at), diameter: v.diameter, drill: v.drill, from_layer: v.from_layer.clone(), to_layer: v.to_layer.clone(), layer_order: board.layers.clone() }).collect();

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
            teardrop: z.teardrop,
            fill_mode: z.fill_mode,
            hatch_thickness: z.hatch_thickness,
            hatch_gap: z.hatch_gap,
            hatch_orientation_mdeg: z.hatch_orientation_mdeg,
            hatch_smoothing_level: z.hatch_smoothing_level,
            hatch_smoothing_value: z.hatch_smoothing_value,
            hatch_hole_min_area: z.hatch_hole_min_area,
            hatch_border_algorithm: z.hatch_border_algorithm,
            smoothing: z.smoothing,
            corner_radius: z.corner_radius,
            ..Zone::default()
        };

        let other_zones: Vec<FillZoneRef> = board
            .zones
            .iter()
            .filter(|o| !std::ptr::eq(*o, z))
            .map(|o| FillZoneRef { id: o.id.clone(), net: o.net.clone(), layer: o.layer.clone(), outline: o.outline.iter().map(|&p| pt(p)).collect(), priority: o.priority, teardrop: o.teardrop, clearance: o.clearance, fill: None })
            .collect();

        // Copper-pour keepouts on this zone's own layer (task item 3) --
        // the filler's own unconditional knockout, see `FillKeepout`'s doc.
        let keepouts: Vec<FillKeepout> =
            board.keepouts.iter().filter(|k| k.no_copper_pour && k.layer == z.layer).map(|k| FillKeepout { layer: k.layer.clone(), outline: k.outline.iter().map(|&p| pt(p)).collect() }).collect();

        let input = FillInput {
            pads: pads.clone(),
            tracks: tracks.clone(),
            vias: vias.clone(),
            other_zones,
            board_outline: board_outline.clone(),
            keepouts,
            hole_clearance: crate::constraints::hole_clearance_min(rules),
            worst_clearance: crate::constraints::worst_case_clearance(rules),
        };
        let clearance_fn = |a: Option<&str>, b: Option<&str>| crate::constraints::clearance(rules, a, b);
        let fill = fill_zone(&zone, &zone.layer, &input, clearance_fn, DEFAULT_MAX_ERROR);
        out.zones.insert(z.id.clone(), ZoneFill { fill });
    }
    out
}
