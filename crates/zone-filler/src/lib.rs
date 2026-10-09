//! A port of KiCad's `ZONE_FILLER` (`pcbnew/zone_filler.cpp`/`.h`, vendored
//! snapshot commit `8303b2ad`), adapted to this workspace's simpler board
//! model. Builds on `eda_shape_poly_set` (stage 2) and `eda_clipper2`
//! (stage 1).
//!
//! ## Scope
//!
//! Ported, following `fillCopperZone`'s own pipeline order:
//!
//! * the zone outline with its corner smoothing, the same-net zones it merges
//!   with, the board outline and the min-width apron (`ZONE::BuildSmoothedPoly`,
//!   [`smoothed`]);
//! * knockouts of pads, tracks, vias, text, other-net zones and keepouts by
//!   clearance;
//! * thermal reliefs (`knockoutThermalReliefs`): the connection a pad gets is
//!   the pad's own override, else its footprint's, else the zone's
//!   (`DRC_ENGINE::EvalZoneConnection`), with the pad's own relief gap and spoke
//!   width over the zone's, and the four spokes of `buildThermalSpokes`
//!   ([`spokes`]) -- turned with the pad, at the pad's spoke angle, kept only if
//!   they reach copper;
//! * the hatch fill (`addHatchFillTypeOnZone`, `buildHatchZoneThermalRings`,
//!   [`hatch`]), with thermal rings round pads and vias;
//! * min-width deflate/prune/reinflate, the iterative refill's pre-knockout
//!   cache (`m_preKnockoutFillCache`) and `postKnockoutMinWidthPrune`, the
//!   same-net higher-priority subtraction, and a final `Fracture()`;
//! * geometric island removal (`ISLAND_REMOVAL_MODE`).
//!
//! Not ported, each a documented, bounded cut: `connect_nearby_polys` (a
//! robustness pass for concave near-touching geometry); per-item DRC-rule
//! dispatch (`DRC_ENGINE::EvalRules`) -- clearance resolution is a
//! caller-supplied `net, net -> gap` callback instead, meant to be
//! `eda_drc::constraints::clearance`; teardrop areas' special cases beyond the
//! connection type, backdrill/post-machining knockouts, conditional pad
//! flashing, and custom pad shapes (this workspace's pads are always
//! `Rect`/`RoundRect`/`Circle`/`Oval`, the same simplification
//! `eda_drc::kimath` already documents).

pub mod corner;
pub mod hatch;
pub mod shape;
pub mod smoothed;
pub mod spokes;

use eda_clipper2::Point64;
use eda_model::ir::{FillMode, IslandRemovalMode, PadConnection, Point, Zone};
use eda_shape_poly_set::{CornerStrategy, LineChain, Polygon, ShapePolySet};
use shape::Shape;
use std::sync::Arc;

pub fn to_point64(p: Point) -> Point64 {
    Point64::new(p.x, p.y)
}

pub fn chain_from_ir(pts: &[Point]) -> LineChain {
    pts.iter().map(|&p| to_point64(p)).collect()
}

/// What `buildThermalSpokes` needs to know about a pad beyond its copper: where its shape sits, how big it is before
/// the board orientation is applied, and which way its spokes point.
#[derive(Debug, Clone, Copy)]
pub struct PadGeometry {
    /// `PAD::ShapePos`.
    pub center: Point64,
    /// `PAD::GetSize` (before any rotation).
    pub size: (i64, i64),
    /// `CIRCLE`, or `OVAL` with equal sides: spokes are built at 0 degrees and turned afterwards.
    pub circular: bool,
    /// `PADSTACK::DefaultThermalSpokeAngleForShape`, millidegrees: 90 degrees for an oval or (rounded) rectangle, 45 otherwise.
    pub default_spoke_angle_mdeg: i64,
    /// `PAD::GetOrientation`, millidegrees, KiCad's sign convention.
    pub orientation_mdeg: i64,
}

#[derive(Debug, Clone)]
pub struct FillPad {
    pub net: Option<String>,
    pub layers: Vec<String>,
    pub copper: Shape,
    pub hole: Option<Shape>,
    /// `None`: the spokes are cut from `copper`'s bounding box as an unrotated rectangle (the older, simpler model).
    pub geometry: Option<PadGeometry>,
    /// `PAD::GetLocalZoneConnection` (`None` is `INHERITED`).
    pub zone_connection: Option<PadConnection>,
    /// `FOOTPRINT::GetLocalZoneConnection` of the pad's footprint.
    pub footprint_zone_connection: Option<PadConnection>,
    /// `PAD::GetClearanceOverrides`: the pad's own clearance, else its footprint's. Replaces the net class's and the zone's
    /// for this pad, but never goes below the board's minimum clearance; 0 is no override.
    pub clearance_override: Option<i64>,
    /// `PAD::GetLocalThermalGapOverride`.
    pub thermal_gap: Option<i64>,
    /// `PAD::GetLocalThermalSpokeWidthOverride`.
    pub spoke_width: Option<i64>,
    /// `PAD::GetThermalSpokeAngle` when set explicitly, millidegrees.
    pub spoke_angle_mdeg: Option<i64>,
    /// `PAD_ATTRIB::PTH`: what `ZONE_CONNECTION::THT_THERMAL` thermal-relieves.
    pub plated_through_hole: bool,
}

impl Default for FillPad {
    fn default() -> Self {
        FillPad {
            net: None,
            layers: Vec::new(),
            copper: Shape::Polygon { pts: Vec::new() },
            hole: None,
            geometry: None,
            zone_connection: None,
            footprint_zone_connection: None,
            clearance_override: None,
            thermal_gap: None,
            spoke_width: None,
            spoke_angle_mdeg: None,
            plated_through_hole: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FillTrack {
    pub net: Option<String>,
    pub layer: String,
    pub a: Point64,
    pub b: Point64,
    pub width: i64,
}

#[derive(Debug, Clone)]
pub struct FillVia {
    pub net: Option<String>,
    pub at: Point64,
    pub diameter: i64,
    /// Drill diameter (0 = unknown): knocked out at the hole clearance.
    pub drill: i64,
    pub from_layer: String,
    pub to_layer: String,
    pub layer_order: Vec<String>,
}

impl FillVia {
    /// Whether the via spans `layer` (inclusive of both ends), matching
    /// `BOARD_ITEM::IsOnLayer` for a through via: on every copper layer
    /// between (and including) its start and end in stack order.
    pub fn is_on_layer(&self, layer: &str) -> bool {
        let (Some(i0), Some(i1), Some(il)) =
            (self.layer_order.iter().position(|l| l == &self.from_layer), self.layer_order.iter().position(|l| l == &self.to_layer), self.layer_order.iter().position(|l| l == layer))
        else {
            return self.from_layer == layer || self.to_layer == layer;
        };
        let (lo, hi) = (i0.min(i1), i0.max(i1));
        il >= lo && il <= hi
    }
}

/// The other zones a fill has to know about. `fill` is the zone's own computed copper on `layer`, when it has one yet:
/// a higher-priority zone of another net knocks this zone out by its *fill* (`ZONE::TransformShapeToPolygon`), not by its
/// outline; without one the outline stands in.
#[derive(Debug, Clone, Default)]
pub struct FillZoneRef {
    pub id: String,
    pub net: Option<String>,
    pub layer: String,
    pub outline: LineChain,
    pub priority: u32,
    /// `ZONE::IsTeardropArea()`: never subtracted as a higher-priority
    /// same-net zone (`subtractHigherPriorityZones`).
    pub teardrop: bool,
    /// `ZONE::GetLocalClearance`: maxed into the clearance between this zone and another (`EvalRules`).
    pub clearance: i64,
    pub fill: Option<Arc<ShapePolySet>>,
}

impl FillZoneRef {
    /// `ZONE::HigherPriority`: teardrops first, then the assigned priority, then (KiCad: the UUID) the id.
    pub fn higher_priority_than(&self, other_teardrop: bool, other_priority: u32, other_id: &str) -> bool {
        if self.teardrop != other_teardrop {
            return self.teardrop;
        }
        if self.priority != other_priority {
            return self.priority > other_priority;
        }
        self.id.as_str() > other_id
    }
}

/// `ZONE::HigherPriority( other )` for the zone being filled against another.
pub fn zone_higher_priority(zone: &Zone, other: &FillZoneRef) -> bool {
    if zone.teardrop != other.teardrop {
        return zone.teardrop;
    }
    if zone.priority != other.priority {
        return zone.priority > other.priority;
    }
    zone.id.as_str() > other.id.as_str()
}

/// A rule area (keepout) that disallows copper pours under it
/// (`ZONE::GetDoNotAllowZoneFills`, task item 3) -- the one keepout
/// restriction the filler itself needs to know about; the other four
/// (tracks/vias/pads/footprints) are a placement/routing/DRC concern, not
/// a fill one, and are checked by `eda_drc`'s disallow provider instead.
#[derive(Debug, Clone)]
pub struct FillKeepout {
    pub layer: String,
    pub outline: LineChain,
}

#[derive(Debug, Clone, Default)]
pub struct FillInput {
    pub pads: Vec<FillPad>,
    pub tracks: Vec<FillTrack>,
    pub vias: Vec<FillVia>,
    pub other_zones: Vec<FillZoneRef>,
    pub board_outline: Option<LineChain>,
    /// Copper-pour keepouts on this layer. Unconditional knockout, no net/
    /// priority test (`other_zones`' own gate) -- a rule area wins against
    /// every zone regardless of net or priority, matching
    /// `ZONE_FILLER::fillCopperZone`'s own unconditional keepout
    /// subtraction.
    pub keepouts: Vec<FillKeepout>,
    /// `HOLE_CLEARANCE_CONSTRAINT` (the board's hole clearance): a
    /// different-net pad or via hole is knocked out at `max( gap, this )`.
    pub hole_clearance: i64,
    /// `BOARD::GetMaxClearanceValue`: how far outside a zone's bounding box an item can still reach into it.
    pub worst_clearance: i64,
    /// `EDGE_CLEARANCE_CONSTRAINT` (the board's copper-to-edge clearance): the fill stays this far from every segment of
    /// `board_outline` (`knockoutGraphicClearance` on the Edge.Cuts items, line widths ignored). 0 = no edge knockout.
    pub edge_clearance: i64,
    /// `BOARD_DESIGN_SETTINGS::m_MinClearance`: the floor a pad's own clearance override cannot go below.
    pub min_clearance: i64,
}

/// `max_error`: the polygon-approximation tolerance for circles/arcs
/// (`BOARD_DESIGN_SETTINGS::m_MaxError`), in the same unit as every other
/// coordinate here (micrometers). 5 (0.005 mm) matches KiCad's own default.
pub const DEFAULT_MAX_ERROR: i64 = 5;

/// `ADVANCED_CFG::m_ExtraClearance` (0.0005 mm), rounded up to this IR's
/// whole µm: `zone_filler.cpp` knocks out every item at `gap + extra_margin`.
/// (A 2 µm margin was measured too: it hides real near-misses KiCad itself
/// reports on issue22475's board, so 1 it is.)
pub const EXTRA_MARGIN: i64 = 1;

/// `addKnockout( pad, aLayer, aGap, holes )` / `TransformShapeToPolygon( ..., aGap, m_maxError, ERROR_OUTSIDE )`: the
/// polygonal approximation's error lands *outside* the true boundary (`GetCircleToPolyCorrection` grows the radius by
/// `aMaxError`), so the hole never comes closer than the gap anywhere along a curve.
fn add_knockout_exact(holes: &mut ShapePolySet, shape: &Shape, gap: i64, max_error: i64) {
    let poly = shape::shape_to_polygon_outside(shape, gap, max_error);
    if poly.len() >= 3 {
        holes.add_outline(poly);
    }
}

/// The same with the fill's extra margin (`gap + extra_margin`), used for every electrical clearance.
fn add_knockout(holes: &mut ShapePolySet, shape: &Shape, gap: i64, max_error: i64) {
    add_knockout_exact(holes, shape, gap + EXTRA_MARGIN, max_error);
}

fn bbox_of(shape: &Shape) -> (i64, i64, i64, i64) {
    shape::bounds(shape)
}

fn bboxes_intersect(a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)) -> bool {
    a.0.max(b.0) <= a.2.min(b.2) && a.1.max(b.1) <= a.3.min(b.3)
}

fn inflate_bbox(b: (i64, i64, i64, i64), by: i64) -> (i64, i64, i64, i64) {
    (b.0 - by, b.1 - by, b.2 + by, b.3 + by)
}

fn chain_bbox(chain: &LineChain) -> (i64, i64, i64, i64) {
    bbox_of(&Shape::Polygon { pts: chain.clone() })
}

/// `DRC_ENGINE::EvalZoneConnection( pad, zone )`: the pad's own override, else its footprint's, else the zone's;
/// `THT_THERMAL` is a thermal relief on a plated through-hole pad and a solid connection on anything else.
pub fn eval_zone_connection(pad: &FillPad, zone: &Zone) -> PadConnection {
    let connection = pad.zone_connection.or(pad.footprint_zone_connection).unwrap_or(zone.pad_connection);
    if connection == PadConnection::ThtThermal {
        return if pad.plated_through_hole { PadConnection::Thermal } else { PadConnection::Full };
    }
    connection
}

/// `THERMAL_RELIEF_GAP_CONSTRAINT` for a pad: its own override when set (and positive), else the zone's.
pub fn thermal_gap_of(pad: &FillPad, zone: &Zone) -> i64 {
    pad.thermal_gap.filter(|&g| g > 0).unwrap_or(zone.thermal_gap)
}

/// `THERMAL_SPOKE_WIDTH_CONSTRAINT` for a pad: its own override when set (and positive; never below the zone's minimum
/// thickness), else the zone's.
pub fn spoke_width_of(pad: &FillPad, zone: &Zone) -> i64 {
    match pad.spoke_width.filter(|&w| w > 0) {
        Some(w) => w.max(zone.min_thickness),
        None => zone.thermal_spoke_width,
    }
}

/// A pad or hatch-zone via held in thermal relief: knocked out at its gap, then connected back by spokes (and rings).
struct ThermalItem {
    /// The exact copper outline of the item (a via is a circle).
    shape: Shape,
    /// A pad's drill (`addHoleKnockout( pad, 0, clearanceHoles )` keeps it open after the spokes went in).
    hole: Option<Shape>,
    circular: bool,
    /// `padRadius` of `buildHatchZoneThermalRings`.
    radius: i64,
    position: Point64,
    gap: i64,
    /// Spoke width, already clamped to the item and checked against the zone's minimum thickness; `None` when the item
    /// gets no spokes (`spoke_w < GetMinThickness()`).
    spoke_width: Option<i64>,
    source: spokes::SpokeSource,
}

/// The result of one `fillCopperZone`: the fill, fractured, and the same fill before the other zones' knockouts
/// (`m_preKnockoutFillCache`, what an iterative refill starts from).
#[derive(Debug, Clone)]
pub struct ZoneFillStage1 {
    pub fill: ShapePolySet,
    pub pre_knockout: ShapePolySet,
}

/// `ZONE_FILLER::fillCopperZone`, then geometric island removal: the whole fill of one zone, with whichever other zones
/// of `input.other_zones` already have a fill (or an outline) knocking it out.
pub fn fill_zone<F>(zone: &Zone, layer: &str, input: &FillInput, clearance_fn: F, max_error: i64) -> ShapePolySet
where
    F: Fn(Option<&str>, Option<&str>) -> i64,
{
    let mut fill = fill_copper_zone(zone, layer, input, clearance_fn, max_error).fill;
    apply_island_removal(&mut fill, zone, input, layer);
    fill
}

/// `ZONE_FILLER::fillCopperZone`.
pub fn fill_copper_zone<F>(zone: &Zone, layer: &str, input: &FillInput, clearance_fn: F, max_error: i64) -> ZoneFillStage1
where
    F: Fn(Option<&str>, Option<&str>) -> i64,
{
    let half_min_width = zone.min_thickness / 2;
    let epsilon = 1; // pcbIUScale.mmToIU(0.001) in um
    let fast_corner = CornerStrategy::ChamferAllCorners;
    let round_corner = CornerStrategy::RoundAllCorners;
    let hatch = zone.fill_mode == FillMode::HatchPattern;

    let zone_net: Option<&str> = if zone.net.is_empty() { None } else { Some(zone.net.as_str()) };
    let zone_outline = chain_from_ir(&zone.outline);
    let zone_bbox = chain_bbox(&zone_outline);
    // Items outside the zone's bounding box inflated by the largest clearance cannot reach into it.
    let worst = input.worst_clearance.max(zone.clearance).max(input.hole_clearance);
    let probe_bbox = inflate_bbox(zone_bbox, worst + EXTRA_MARGIN);

    // `ZONE::BuildSmoothedPoly`: `aSmoothedOutline` (with the min-width apron) and `aMaxExtents`.
    let smoothed = smoothed::build_smoothed_poly(zone, layer, input, max_error);
    // The hatch rings are clipped to the same smoothed outline the fill starts from.
    let outline_for_rings = if hatch { Some(smoothed.with_apron.clone()) } else { None };
    let mut fill = smoothed.with_apron;
    let max_extents = smoothed.max_extents;

    // -------------------------------------------------------------------
    // Knockout thermal reliefs (`knockoutThermalReliefs`): a pad of the zone's net per its resolved connection either
    // gets a thermal-gap hole (and is recorded for spokes), no hole at all (`FULL`), or a clearance hole (`NONE`);
    // everything else (another net, or no net) is left for `buildCopperItemClearances` below.
    // -------------------------------------------------------------------
    let mut thermal_holes = ShapePolySet::new();
    let mut thermal_items: Vec<ThermalItem> = Vec::new();
    let mut no_connection_pads: Vec<usize> = Vec::new();
    let mut clearance_holes = ShapePolySet::new();

    for (i, pad) in input.pads.iter().enumerate() {
        if !pad.layers.iter().any(|l| l == layer) {
            continue;
        }
        let pad_bbox = bbox_of(&pad.copper);
        if !bboxes_intersect(pad_bbox, probe_bbox) {
            continue;
        }

        // `bool noConnection = pad->GetNetCode() != aZone->GetNetCode();
        //  if( !aZone->IsTeardropArea() && aZone->GetNetCode() == 0 ) noConnection = true;`
        let same_net = pad.net.as_deref() == zone_net && (zone_net.is_some() || zone.teardrop);
        if !same_net {
            no_connection_pads.push(i);
            continue;
        }

        // A teardrop connects solidly to everything of its net.
        let connection = if zone.teardrop { PadConnection::Full } else { eval_zone_connection(pad, zone) };
        match connection {
            PadConnection::Thermal | PadConnection::ThtThermal => {
                // `aFill.Collide( padShape, 0 )`: a relief only where the pad touches the zone.
                if !shape_touches_fill(&fill, &pad.copper, max_error) {
                    continue;
                }
                let gap = thermal_gap_of(pad, zone);
                // `addKnockout( pad, aLayer, padClearance, holes )` -- the bare thermal gap, no extra margin.
                add_knockout_exact(&mut thermal_holes, &pad.copper, gap, max_error);
                thermal_items.push(thermal_item_of_pad(pad, zone, gap, hatch, epsilon));
            }
            PadConnection::Full => { /* no knockout: connects directly */ }
            PadConnection::None => {
                // `PHYSICAL_CLEARANCE` (null here: 0), maxed with the zone's local clearance; no extra margin.
                let gap = zone.clearance;
                add_knockout_exact(&mut thermal_holes, &pad.copper, gap, max_error);
                if let Some(hole) = &pad.hole {
                    add_knockout_exact(&mut thermal_holes, hole, gap, max_error);
                }
            }
        }
    }

    // A via in a hatch zone is thermal-relieved the same way (a solid pour connects to it directly); its connection is the
    // zone's own (`THT_THERMAL` is a solid connection to a via).
    let mut via_items: Vec<usize> = Vec::new();
    if hatch {
        for (i, via) in input.vias.iter().enumerate() {
            if !via.is_on_layer(layer) {
                continue;
            }
            let shape = Shape::Circle { c: via.at, r: via.diameter / 2 };
            if !bboxes_intersect(bbox_of(&shape), probe_bbox) {
                continue;
            }
            if via.net.as_deref() != zone_net {
                continue;
            }
            if zone.pad_connection == PadConnection::Thermal && zone.thermal_gap > 0 {
                add_knockout_exact(&mut thermal_holes, &shape, zone.thermal_gap, max_error);
                thermal_items.push(thermal_item_of_via(via, zone, epsilon));
                via_items.push(i);
            }
        }
    }
    fill.boolean_subtract(&thermal_holes);

    // -------------------------------------------------------------------
    // For hatch zones, thermal rings round pads and vias: the webbing connects to those instead of to the pad itself.
    // -------------------------------------------------------------------
    let mut thermal_rings = ShapePolySet::new();
    if let Some(outline) = &outline_for_rings {
        hatch::build_thermal_rings(&thermal_items_for_rings(&thermal_items), outline, &mut fill, &mut thermal_rings, max_error);
    }

    // -------------------------------------------------------------------
    // Knockout electrical clearances (`buildCopperItemClearances`): pads that are not connected, tracks, vias and text of
    // another net; same-net items do not knock the zone out. Zone-to-zone clearances come later (iterative refill).
    // -------------------------------------------------------------------
    for &i in &no_connection_pads {
        let pad = &input.pads[i];
        // `knockoutPadClearance`: the copper at the clearance, the drill at the larger hole clearance. A pad's own clearance
        // (or its footprint's) replaces the net class's and the zone's -- `EvalRules`: a local override takes precedence
        // over everything except the board minimum (and 0 is not an override).
        let (gap, hole_gap) = match pad.clearance_override.filter(|&c| c != 0) {
            Some(c) => (c.max(input.min_clearance), c.max(input.hole_clearance)),
            None => {
                let g = clearance_fn(zone_net, pad.net.as_deref()).max(zone.clearance);
                (g, g.max(input.hole_clearance))
            }
        };
        add_knockout(&mut clearance_holes, &pad.copper, gap, max_error);
        if let Some(hole) = &pad.hole {
            add_knockout(&mut clearance_holes, hole, hole_gap, max_error);
        }
    }

    for track in &input.tracks {
        if track.layer != layer {
            continue;
        }
        let shape = Shape::Stadium { a: track.a, b: track.b, r: track.width / 2 };
        if !bboxes_intersect(bbox_of(&shape), probe_bbox) {
            continue;
        }
        // `if( !aZone->IsTeardropArea() && aZone->GetNetCode() == 0 ) sameNet = false;`
        let same_net = track.net.as_deref() == zone_net && (zone_net.is_some() || zone.teardrop);
        if !same_net {
            let gap = clearance_fn(zone_net, track.net.as_deref()).max(zone.clearance);
            add_knockout(&mut clearance_holes, &shape, gap, max_error);
        }
    }

    for via in &input.vias {
        if !via.is_on_layer(layer) {
            continue;
        }
        let shape = Shape::Circle { c: via.at, r: via.diameter / 2 };
        if !bboxes_intersect(bbox_of(&shape), probe_bbox) {
            continue;
        }
        let same_net = via.net.as_deref() == zone_net && (zone_net.is_some() || zone.teardrop);
        if !same_net {
            let gap = clearance_fn(zone_net, via.net.as_deref()).max(zone.clearance);
            add_knockout(&mut clearance_holes, &shape, gap, max_error);
            // `knockoutTrackClearance`: the drill at max( gap, HOLE_CLEARANCE ).
            if via.drill > 0 {
                add_knockout(&mut clearance_holes, &Shape::Circle { c: via.at, r: via.drill / 2 }, gap.max(input.hole_clearance), max_error);
            }
        }
    }

    // Rule area (keepout) copper-pour exclusion: a hole cut from `fill` before the min-width pass, so a keepout-adjacent
    // sliver is pruned by the same geometry those passes apply to every other knockout (iterative refill adds the keepouts
    // to `clearanceHoles` for exactly this reason: issue 23515).
    for keepout in &input.keepouts {
        if keepout.layer != layer {
            continue;
        }
        if !bboxes_intersect(chain_bbox(&keepout.outline), zone_bbox) {
            continue;
        }
        clearance_holes.add_outline(keepout.outline.clone());
    }

    // Board edge: `knockoutGraphicClearance` of the Edge.Cuts items at the board's copper-to-edge clearance, the line's
    // own width ignored (a zero-width stadium round each outline segment).
    if let (Some(outline), true) = (&input.board_outline, input.edge_clearance > 0) {
        let n = outline.len();
        for i in 0..n {
            let (a, b) = (outline[i], outline[(i + 1) % n]);
            let seg = Shape::Stadium { a, b, r: 0 };
            if bboxes_intersect(bbox_of(&seg), inflate_bbox(zone_bbox, input.edge_clearance + EXTRA_MARGIN + max_error)) {
                add_knockout(&mut clearance_holes, &seg, input.edge_clearance, max_error);
            }
        }
    }

    // -------------------------------------------------------------------
    // Zone clearances: higher-priority zones of another net (`buildDifferentNetZoneClearances`), kept apart from
    // `clearance_holes` so the fill can be cached before they apply.
    // -------------------------------------------------------------------
    let zone_clearances = different_net_zone_clearances(zone, layer, input, zone_net, &clearance_fn, max_error, probe_bbox);

    // -------------------------------------------------------------------
    // Thermal relief spokes (`buildThermalSpokes` + the spoke-keep loop).
    // -------------------------------------------------------------------
    let mut test_areas = fill.clone();
    test_areas.boolean_subtract(&clearance_holes);
    if zone_clearances.outline_count() > 0 {
        test_areas.boolean_subtract(&zone_clearances);
    }
    if half_min_width - epsilon > epsilon {
        test_areas.deflate(half_min_width - epsilon, fast_corner, max_error as i32);
        test_areas.inflate(half_min_width - epsilon, fast_corner, max_error as i32, false);
    }

    let zone_half_width = if hatch { zone.hatch_thickness / 2 } else { half_min_width };
    let mut all_spokes: Vec<spokes::Spoke> = Vec::new();
    for item in &thermal_items {
        if item.spoke_width.is_some() {
            all_spokes.extend(spokes::build_spokes(&item.source, zone_half_width, max_error));
        }
    }
    for i in spokes::keep_spokes(&all_spokes, &test_areas) {
        fill.add_outline(all_spokes[i].outline.clone());
    }

    fill.boolean_subtract(&clearance_holes);

    // -------------------------------------------------------------------
    // Prune features that don't meet minimum-width criteria.
    // -------------------------------------------------------------------
    if half_min_width - epsilon > epsilon {
        fill.deflate(half_min_width - epsilon, fast_corner, max_error as i32);
        // Also deflate thermal rings to match, for correct hatch hole notching.
        if thermal_rings.outline_count() > 0 {
            thermal_rings.deflate(half_min_width - epsilon, fast_corner, max_error as i32);
        }
    }

    remove_sub_min_thickness_slivers(&mut fill, zone.min_thickness);

    if hatch {
        // Rings and the clearance holes of unconnected pads both keep the hatch holes off them.
        let mut protect = thermal_rings.clone();
        protect.boolean_add(&clearance_holes);
        hatch::add_hatch_fill(zone, &mut fill, &protect, max_error);
    } else {
        fill.fracture(true);
        // `connect_nearby_polys` is not ported (see the module doc comment).
    }

    if half_min_width - epsilon > epsilon {
        fill.inflate(half_min_width - epsilon, round_corner, max_error as i32, true);
    }

    // The deflation/inflation process can leave notches in the outline. Remove these by doing a union with the original ring.
    fill.boolean_add(&thermal_rings);

    // Additive changes (thermal stubs, inflated acute corners) must not add copper outside the zone boundary, inside the
    // clearance holes, or inside the pad drills.
    for item in &thermal_items {
        if let Some(hole) = &item.hole {
            add_knockout_exact(&mut clearance_holes, hole, 0, max_error);
        }
    }
    fill.boolean_intersection(&max_extents);
    fill.boolean_subtract(&clearance_holes);

    // The iterative refill's cache: the fill before the other zones' knockouts, so it can be reclaimed when a
    // higher-priority zone loses islands.
    let pre_knockout = fill.clone();
    let mut knockouts_applied = false;
    if zone_clearances.outline_count() > 0 {
        fill.boolean_subtract(&zone_clearances);
        knockouts_applied = true;
    }

    // Re-prune minimum-width violations introduced by the different-net zone knockouts, before the same-net knockout:
    // the fill still extends into overlapping same-net zones, a buffer that keeps the deflate/inflate cycle from cutting
    // divots at their boundaries.
    if knockouts_applied {
        post_knockout_min_width_prune(zone, &mut fill, max_error, half_min_width, epsilon);
    }

    // Lastly give any same-net but higher-priority zones control over their own area.
    let same_net_higher = same_net_higher_priority_outlines(zone, layer, input, zone_net, zone_bbox);
    if same_net_higher.outline_count() > 0 {
        fill.boolean_subtract(&same_net_higher);
    }

    fill.fracture(true);
    ZoneFillStage1 { fill, pre_knockout }
}

/// `ZONE_FILLER::refillZoneFromCache`: a zone's cached pre-knockout fill with the *current* fills of the higher-priority
/// zones taken out again -- their copper, now that islands have been removed from it, rather than their outlines.
pub fn refill_zone_from_cache<F>(zone: &Zone, layer: &str, cache: &ShapePolySet, input: &FillInput, clearance_fn: F, max_error: i64) -> ShapePolySet
where
    F: Fn(Option<&str>, Option<&str>) -> i64,
{
    let half_min_width = zone.min_thickness / 2;
    let epsilon = 1;
    let zone_net: Option<&str> = if zone.net.is_empty() { None } else { Some(zone.net.as_str()) };
    let zone_bbox = chain_bbox(&chain_from_ir(&zone.outline));
    let worst = input.worst_clearance.max(zone.clearance).max(input.hole_clearance);
    let probe_bbox = inflate_bbox(zone_bbox, worst + EXTRA_MARGIN);

    let mut fill = cache.clone();
    let mut diff_net = ShapePolySet::new();
    let mut same_net = ShapePolySet::new();
    let mut knockouts_applied = false;
    for other in &input.other_zones {
        if other.layer != layer || (!other.id.is_empty() && other.id == zone.id) {
            continue;
        }
        let other_same_net = other.net.as_deref() == zone_net;
        if other.teardrop && other_same_net {
            continue;
        }
        if !zone_higher_priority_inverse(zone, other) {
            continue;
        }
        if !bboxes_intersect(chain_bbox(&other.outline), probe_bbox) {
            continue;
        }
        let Some(other_fill) = other.fill.as_ref().filter(|f| f.outline_count() > 0) else { continue };
        if other_same_net {
            for poly in &other_fill.polys {
                same_net.add_polygon(poly.clone());
            }
        } else {
            let gap = clearance_fn(zone_net, other.net.as_deref()).max(zone.clearance).max(other.clearance);
            let mut inflated = (**other_fill).clone();
            inflated.inflate_with_linked_holes(gap + EXTRA_MARGIN + max_error, CornerStrategy::RoundAllCorners, max_error as i32);
            for poly in inflated.polys {
                diff_net.add_polygon(poly);
            }
            knockouts_applied = true;
        }
    }
    if diff_net.outline_count() > 0 {
        fill.boolean_subtract(&diff_net);
    }
    if knockouts_applied {
        post_knockout_min_width_prune(zone, &mut fill, max_error, half_min_width, epsilon);
    }
    if same_net.outline_count() > 0 {
        fill.boolean_subtract(&same_net);
    }
    fill.fracture(true);
    fill
}

/// `otherZone->HigherPriority( aZone )`.
fn zone_higher_priority_inverse(zone: &Zone, other: &FillZoneRef) -> bool {
    other.higher_priority_than(zone.teardrop, zone.priority, &zone.id)
}

fn thermal_item_of_pad(pad: &FillPad, zone: &Zone, gap: i64, hatch: bool, epsilon: i64) -> ThermalItem {
    let _ = (hatch, epsilon);
    let (b0, b1, b2, b3) = bbox_of(&pad.copper);
    let geometry = pad.geometry.unwrap_or(PadGeometry {
        center: Point64::new((b0 + b2) / 2, (b1 + b3) / 2),
        size: (b2 - b0, b3 - b1),
        circular: false,
        default_spoke_angle_mdeg: 90_000,
        orientation_mdeg: 0,
    });
    let circular = geometry.circular;
    // Spoke width: never wider than the pad's minor axis (otherwise the relief is no relief and the spoke count test can
    // fail); a spoke narrower than the zone's minimum thickness is not built at all.
    let spoke_w = spoke_width_of(pad, zone).min(geometry.size.0.min(geometry.size.1));
    let spoke_width = (spoke_w >= zone.min_thickness).then_some(spoke_w);
    ThermalItem {
        shape: pad.copper.clone(),
        hole: pad.hole.clone(),
        circular,
        radius: geometry.size.0.max(geometry.size.1) / 2,
        position: geometry.center,
        gap,
        spoke_width,
        source: spokes::SpokeSource {
            position: geometry.center,
            size: geometry.size,
            circular,
            angle_mdeg: pad.spoke_angle_mdeg.unwrap_or(geometry.default_spoke_angle_mdeg),
            orientation_mdeg: geometry.orientation_mdeg,
            thermal_gap: gap,
            spoke_width: spoke_w,
        },
    }
}

fn thermal_item_of_via(via: &FillVia, zone: &Zone, _epsilon: i64) -> ThermalItem {
    let gap = zone.thermal_gap;
    let spoke_w = zone.thermal_spoke_width.min(via.diameter);
    let spoke_width = (spoke_w >= zone.min_thickness).then_some(spoke_w);
    ThermalItem {
        shape: Shape::Circle { c: via.at, r: via.diameter / 2 },
        hole: None,
        circular: true,
        radius: via.diameter / 2,
        position: via.at,
        gap,
        spoke_width,
        source: spokes::SpokeSource { position: via.at, size: (via.diameter, via.diameter), circular: true, angle_mdeg: 45_000, orientation_mdeg: 0, thermal_gap: gap, spoke_width: spoke_w },
    }
}

fn thermal_items_for_rings(items: &[ThermalItem]) -> Vec<hatch::RingItem> {
    items
        .iter()
        .filter_map(|it| it.spoke_width.map(|w| hatch::RingItem { shape: it.shape.clone(), circular: it.circular, radius: it.radius, position: it.position, gap: it.gap, spoke_width: w }))
        .collect()
}

/// Whether `shape` touches the copper of `fill` (`SHAPE_POLY_SET::Collide( shape, 0 )`).
fn shape_touches_fill(fill: &ShapePolySet, shape: &Shape, max_error: i64) -> bool {
    let sb = bbox_of(shape);
    let fb = {
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for poly in &fill.polys {
            let b = chain_bbox(&poly[0]);
            x0 = x0.min(b.0);
            y0 = y0.min(b.1);
            x1 = x1.max(b.2);
            y1 = y1.max(b.3);
        }
        (x0, y0, x1, y1)
    };
    if fill.polys.is_empty() || !bboxes_intersect(sb, fb) {
        return false;
    }
    let centre = Point64::new((sb.0 + sb.2) / 2, (sb.1 + sb.3) / 2);
    if spokes::poly_set_contains(fill, centre) {
        return true;
    }
    let pad_poly = shape::exact_polygon(shape, max_error);
    if pad_poly.len() < 3 {
        return false;
    }
    let mut probe = fill.clone();
    probe.boolean_intersection(&ShapePolySet::from_outline(pad_poly));
    !probe.is_empty()
}

/// `buildDifferentNetZoneClearances`: higher-priority zones of another net, at their clearance. Their fill when they have
/// one (`ZONE::TransformShapeToPolygon`: the fill inflated by `gap + extra_margin + max_error`), else their outline.
fn different_net_zone_clearances<F>(zone: &Zone, layer: &str, input: &FillInput, zone_net: Option<&str>, clearance_fn: &F, max_error: i64, probe_bbox: (i64, i64, i64, i64)) -> ShapePolySet
where
    F: Fn(Option<&str>, Option<&str>) -> i64,
{
    let mut holes = ShapePolySet::new();
    for other in &input.other_zones {
        if other.layer != layer || (!other.id.is_empty() && other.id == zone.id) {
            continue;
        }
        if other.net.as_deref() == zone_net {
            continue;
        }
        if !bboxes_intersect(chain_bbox(&other.outline), probe_bbox) {
            continue;
        }
        // `aKnockout->HigherPriority( aZone ) && !aKnockout->SameNet( aZone )`.
        if !zone_higher_priority_inverse(zone, other) {
            continue;
        }
        let gap = clearance_fn(zone_net, other.net.as_deref()).max(zone.clearance).max(other.clearance);
        let mut inflated = match &other.fill {
            Some(f) => {
                if f.outline_count() == 0 {
                    continue;
                }
                (**f).clone()
            }
            None => ShapePolySet::from_outline(other.outline.clone()),
        };
        if other.fill.is_some() {
            inflated.inflate_with_linked_holes(gap + EXTRA_MARGIN + max_error, CornerStrategy::RoundAllCorners, max_error as i32);
        } else if gap > 0 {
            inflated.inflate(gap, CornerStrategy::RoundAllCorners, max_error as i32, false);
        }
        for poly in inflated.polys {
            holes.add_polygon(poly);
        }
    }
    holes
}

/// `subtractHigherPriorityZones`: the outlines of same-net zones of strictly higher priority.
fn same_net_higher_priority_outlines(zone: &Zone, layer: &str, input: &FillInput, zone_net: Option<&str>, zone_bbox: (i64, i64, i64, i64)) -> ShapePolySet {
    let mut knockouts = ShapePolySet::new();
    for other in &input.other_zones {
        if other.layer != layer || (!other.id.is_empty() && other.id == zone.id) {
            continue;
        }
        // `ZONE::SameNet`: net-code equality, so two netless zones match.
        if other.net.as_deref() != zone_net || other.priority <= zone.priority || other.teardrop {
            continue;
        }
        if !bboxes_intersect(chain_bbox(&other.outline), zone_bbox) {
            continue;
        }
        knockouts.add_outline(other.outline.clone());
    }
    knockouts
}

/// `postKnockoutMinWidthPrune`: re-prune min-width violations introduced by the different-net zone knockout.
fn post_knockout_min_width_prune(zone: &Zone, fill: &mut ShapePolySet, max_error: i64, half_min_width: i64, epsilon: i64) {
    if half_min_width - epsilon <= epsilon {
        return;
    }
    let pre_deflate = fill.clone();
    fill.deflate(half_min_width - epsilon, CornerStrategy::ChamferAllCorners, max_error as i32);
    fill.fracture(true);
    remove_sub_min_thickness_slivers(fill, zone.min_thickness);
    fill.inflate(half_min_width - epsilon, CornerStrategy::RoundAllCorners, max_error as i32, true);
    fill.boolean_intersection(&pre_deflate);
}

/// The "web vs. blob" min-thickness check from `fillCopperZone`: an island
/// whose bounding-box longest side is still under `min_thickness` after
/// deflating can't be a useful min-thickness-wide web, so it's dropped.
fn remove_sub_min_thickness_slivers(fill: &mut ShapePolySet, min_thickness: i64) {
    let mut i = fill.polys.len();
    while i > 0 {
        i -= 1;
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for p in &fill.polys[i][0] {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        if (x1 - x0).max(y1 - y0) < min_thickness {
            fill.polys.remove(i);
        }
    }
}

/// `ISLAND_REMOVAL_MODE`: geometric connectivity (does this fragment
/// contain a same-net pad center, track endpoint, or via center on this
/// layer) stands in for upstream's `CONNECTIVITY_DATA`-driven island
/// detection, since this crate deliberately has no dependency on the
/// connectivity crate (which is itself meant to consume this crate's fills
/// -- see the module doc comment).
pub fn apply_island_removal(fill: &mut ShapePolySet, zone: &Zone, input: &FillInput, layer: &str) {
    if matches!(zone.island_removal_mode, IslandRemovalMode::Never) {
        return;
    }
    // Island removal is a *relative* notion -- it prunes fragments that
    // broke off from the (rest of the) fill, never the entire result: with
    // only one fragment there is nothing for it to be disconnected *from*,
    // so it stands on its own regardless of whether this net happens to
    // have any pads/tracks/vias elsewhere on the board yet.
    if fill.polys.len() <= 1 {
        return;
    }
    let zone_net: Option<&str> = if zone.net.is_empty() { None } else { Some(zone.net.as_str()) };

    let islands: Vec<bool> = fill.polys.iter().map(|p| !touches_same_net_item(&p[0], zone_net, input, layer, zone)).collect();
    // `ZONE_FILLER::Fill`: "If *all* the polygons are islands, do not
    // remove any of them".
    if islands.iter().all(|&i| i) {
        return;
    }

    let mut i = fill.polys.len();
    while i > 0 {
        i -= 1;
        let outline = &fill.polys[i][0];
        if !islands[i] {
            continue;
        }
        let remove = match zone.island_removal_mode {
            IslandRemovalMode::Always => true,
            IslandRemovalMode::Never => false,
            IslandRemovalMode::Area => eda_clipper2::area(outline).abs() < zone.min_island_area as f64,
        };
        if remove {
            fill.polys.remove(i);
        }
    }
}

/// Whether `outline`'s (exact) bounding box comes within reach of some
/// same-net pad/track/via/zone on `layer`. A bounding-box proximity test
/// (rather than exact point-in-polygon against the pad's own center, which
/// the pad's own thermal-relief knockout can carve a hole straight through)
/// stands in for `CONNECTIVITY_DATA`'s real "does this fragment touch
/// anything" test, which this crate deliberately has no connectivity
/// dependency to call into -- see the module doc comment.
///
/// The "reach" is added to each *item's* bbox, not the fragment's: only a
/// thermal-relief-connected pad or via needs one at all (`zone.thermal_gap`,
/// since its own knockout hole separates its copper from the fragment by
/// exactly that much, bridged only by a spoke neither this nor
/// `eda_drc`/`eda_connectivity` resolve into exact geometry here) --
/// `FULL`/`NONE`-connected pads, tracks and other zones connect (or don't)
/// with zero gap, so inflating the *fragment* itself by a blanket margin
/// would wrongly call a small, genuinely disconnected fragment "connected"
/// just for having *any* same-net item within reach, regardless of size.
fn touches_same_net_item(outline: &LineChain, zone_net: Option<&str>, input: &FillInput, layer: &str, zone: &Zone) -> bool {
    if zone_net.is_none() {
        return false;
    }
    let frag_bbox = shape::bounds(&Shape::Polygon { pts: outline.clone() });
    let pad_margin = if matches!(zone.pad_connection, PadConnection::Thermal | PadConnection::ThtThermal) { zone.thermal_gap } else { 0 };

    for pad in &input.pads {
        if pad.net.as_deref() == zone_net && pad.layers.iter().any(|l| l == layer) {
            let b = inflate_bbox(bbox_of(&pad.copper), pad_margin);
            if bboxes_intersect(frag_bbox, b) {
                return true;
            }
        }
    }
    for track in &input.tracks {
        if track.net.as_deref() == zone_net && track.layer == layer {
            let shape = Shape::Stadium { a: track.a, b: track.b, r: track.width / 2 };
            if bboxes_intersect(frag_bbox, bbox_of(&shape)) {
                return true;
            }
        }
    }
    for via in &input.vias {
        if via.net.as_deref() == zone_net && via.is_on_layer(layer) {
            let shape = Shape::Circle { c: via.at, r: via.diameter / 2 };
            if bboxes_intersect(frag_bbox, inflate_bbox(bbox_of(&shape), pad_margin)) {
                return true;
            }
        }
    }
    for other in &input.other_zones {
        if other.net.as_deref() == zone_net && other.layer == layer {
            let shape = Shape::Polygon { pts: other.outline.clone() };
            if bboxes_intersect(frag_bbox, bbox_of(&shape)) {
                return true;
            }
        }
    }
    false
}

/// Flattens a fill's outlines+holes into the `Polygon` list the `.kicad_pcb`
/// exporter writes as `filled_polygon` (post-`Fracture`, each polygon is
/// already a single slitted outline with no separate holes).
pub fn to_filled_polygons(fill: &ShapePolySet) -> Vec<Polygon> {
    fill.polys.clone()
}
