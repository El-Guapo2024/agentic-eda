//! A port of KiCad's `ZONE_FILLER` (`pcbnew/zone_filler.cpp`/`.h`, vendored
//! snapshot commit `8303b2ad`), adapted to this workspace's simpler board
//! model. Builds on `eda_shape_poly_set` (stage 2) and `eda_clipper2`
//! (stage 1).
//!
//! ## Scope
//!
//! Ported, following `fillCopperZone`'s own pipeline order exactly:
//! knockouts (pads/tracks/vias by clearance, other-net zones, same-net
//! higher-priority zones), thermal reliefs and spokes, min-width
//! deflate/prune/reinflate, geometric island removal
//! (`ISLAND_REMOVAL_MODE`), and a final `Fracture()`.
//!
//! Not ported, each a documented, bounded cut (see the relevant function's
//! doc comment for why): `ZONE_FILL_MODE::HatchPattern` (falls back to a
//! solid fill -- the grid-pattern generator is a large, mostly independent
//! piece this port didn't have budget for); `connect_nearby_polys` (a
//! robustness pass for concave near-touching geometry, a no-op here);
//! per-item DRC-rule dispatch (`DRC_ENGINE::EvalRules`) -- clearance
//! resolution is a caller-supplied `net, net -> gap` callback instead,
//! meant to be `eda_drc::constraints::clearance`; keepout zones, teardrops,
//! backdrill/post-machining knockouts, and custom pad shapes (this
//! workspace's pads are always `Rect`/`RoundRect`/`Circle`/`Stadium`, the
//! same simplification `eda_drc::kimath` already documents).
//!
//! Zero-width corner smoothing (`ZONE_SETTINGS::m_cornerSmoothingType`) is
//! also not applied to the outline before filling -- a cosmetic
//! pre-processing step, not part of the fill algorithm itself.

pub mod shape;
pub mod spokes;

use eda_clipper2::Point64;
use eda_model::ir::{FillMode, IslandRemovalMode, PadConnection, Point, Zone};
use eda_shape_poly_set::{CornerStrategy, LineChain, Polygon, ShapePolySet};
use shape::Shape;

pub fn to_point64(p: Point) -> Point64 {
    Point64::new(p.x, p.y)
}

pub fn chain_from_ir(pts: &[Point]) -> LineChain {
    pts.iter().map(|&p| to_point64(p)).collect()
}

#[derive(Debug, Clone)]
pub struct FillPad {
    pub net: Option<String>,
    pub layers: Vec<String>,
    pub copper: Shape,
    pub hole: Option<Shape>,
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

#[derive(Debug, Clone)]
pub struct FillZoneRef {
    pub net: Option<String>,
    pub layer: String,
    pub outline: LineChain,
    pub priority: u32,
    /// `ZONE::IsTeardropArea()`: never subtracted as a higher-priority
    /// same-net zone (`subtractHigherPriorityZones`).
    pub teardrop: bool,
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

/// The thermal-relief knockout: the bare thermal gap, as before.
fn add_thermal_knockout(holes: &mut ShapePolySet, shape: &Shape, gap: i64, max_error: i64) {
    let poly = shape::shape_to_polygon(shape, gap, max_error);
    if poly.len() >= 3 {
        holes.add_outline(poly);
    }
}

/// A knockout is built `ERROR_OUTSIDE` (`TransformShapeToPolygon( ...,
/// gap + extra_margin, m_maxError, ERROR_OUTSIDE )`): the polygonal
/// approximation's error lands *outside* the true clearance boundary
/// (`GetCircleToPolyCorrection` grows the radius by `aMaxError`), so the
/// fill never comes closer than the clearance anywhere along a curve.
fn add_knockout(holes: &mut ShapePolySet, shape: &Shape, gap: i64, max_error: i64) {
    let poly = shape::shape_to_polygon_outside(shape, gap + EXTRA_MARGIN, max_error);
    if poly.len() >= 3 {
        holes.add_outline(poly);
    }
}

fn bbox_of(shape: &Shape) -> (i64, i64, i64, i64) {
    shape::bounds(shape)
}

fn bboxes_intersect(a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)) -> bool {
    a.0.max(b.0) <= a.2.min(b.2) && a.1.max(b.1) <= a.3.min(b.3)
}

/// `ZONE_FILLER::fillCopperZone`.
pub fn fill_zone<F>(zone: &Zone, layer: &str, input: &FillInput, clearance_fn: F, max_error: i64) -> ShapePolySet
where
    F: Fn(Option<&str>, Option<&str>) -> i64,
{
    let half_min_width = zone.min_thickness / 2;
    let epsilon = 1; // pcbIUScale.mmToIU(0.001) in um
    let fast_corner = CornerStrategy::ChamferAllCorners;
    let round_corner = CornerStrategy::RoundAllCorners;

    let zone_net: Option<&str> = if zone.net.is_empty() { None } else { Some(zone.net.as_str()) };
    let zone_outline = chain_from_ir(&zone.outline);
    let zone_bbox = {
        let s = Shape::Polygon { pts: zone_outline.clone() };
        bbox_of(&s)
    };

    let mut fill = ShapePolySet::from_outline(zone_outline.clone());

    // -------------------------------------------------------------------
    // Knockout thermal reliefs (`knockoutThermalReliefs`): same-net pads
    // per `zone.pad_connection` either get a thermal-gap knockout (and are
    // recorded for spoke-building) or a direct clearance knockout (`NONE`);
    // everything else (different net, or no net at all) is left for the
    // clearance-holes pass below.
    // -------------------------------------------------------------------
    let mut thermal_holes = ShapePolySet::new();
    let mut thermal_pads: Vec<(usize, (i64, i64, i64, i64))> = Vec::new();
    let mut clearance_holes = ShapePolySet::new();

    for (i, pad) in input.pads.iter().enumerate() {
        if !pad.layers.iter().any(|l| l == layer) {
            continue;
        }
        let pad_bbox = bbox_of(&pad.copper);
        if !bboxes_intersect(pad_bbox, zone_bbox) {
            continue;
        }

        // `if( !aZone->IsTeardropArea() && aZone->GetNetCode() == 0 ) sameNet = false;`
        let same_net = pad.net.as_deref() == zone_net && (zone_net.is_some() || zone.teardrop);

        if !same_net {
            let gap = clearance_fn(zone_net, pad.net.as_deref()).max(zone.clearance);
            add_knockout(&mut clearance_holes, &pad.copper, gap, max_error);
            if let Some(hole) = &pad.hole {
                add_knockout(&mut clearance_holes, hole, gap, max_error);
            }
            continue;
        }

        match zone.pad_connection {
            PadConnection::Thermal | PadConnection::ThtThermal => {
                // `knockoutThermalReliefs`: `addKnockout( pad, aLayer, thermalGap, holes )` -- no extra margin.
                add_thermal_knockout(&mut thermal_holes, &pad.copper, zone.thermal_gap, max_error);
                thermal_pads.push((i, pad_bbox));
            }
            PadConnection::Full => { /* no knockout: connects directly */ }
            PadConnection::None => {
                let gap = zone.clearance.max(clearance_fn(zone_net, pad.net.as_deref()));
                add_knockout(&mut clearance_holes, &pad.copper, gap, max_error);
                if let Some(hole) = &pad.hole {
                    add_knockout(&mut clearance_holes, hole, gap, max_error);
                }
            }
        }
    }
    fill.boolean_subtract(&thermal_holes);

    // -------------------------------------------------------------------
    // Knockout electrical clearances for tracks/vias (`buildCopperItemClearances`):
    // different net (or no net on one side) -> full clearance; same net ->
    // no knockout (this port skips KiCad's separate "physical" manufacturing
    // clearance, which is 0 for the overwhelming majority of boards with no
    // custom physical-clearance rules -- see the module doc comment).
    // -------------------------------------------------------------------
    for track in &input.tracks {
        if track.layer != layer {
            continue;
        }
        let shape = Shape::Stadium { a: track.a, b: track.b, r: track.width / 2 };
        let tb = bbox_of(&shape);
        if !bboxes_intersect(tb, zone_bbox) {
            continue;
        }
        // `if( !aZone->IsTeardropArea() && aZone->GetNetCode() == 0 ) sameNet = false;`
        let same_net = track.net.as_deref() == zone_net && (zone_net.is_some() || zone.teardrop);
        if !same_net {
            let gap = clearance_fn(zone_net, track.net.as_deref()).max(zone.clearance);
            add_knockout(&mut clearance_holes, &shape, gap, max_error);
        }
    }

    let mut via_thermal: Vec<(usize, (i64, i64, i64, i64))> = Vec::new();
    for (i, via) in input.vias.iter().enumerate() {
        if !via.is_on_layer(layer) {
            continue;
        }
        let shape = Shape::Circle { c: via.at, r: via.diameter / 2 };
        let vb = bbox_of(&shape);
        if !bboxes_intersect(vb, zone_bbox) {
            continue;
        }
        // `if( !aZone->IsTeardropArea() && aZone->GetNetCode() == 0 ) sameNet = false;`
        let same_net = via.net.as_deref() == zone_net && (zone_net.is_some() || zone.teardrop);
        if !same_net {
            let gap = clearance_fn(zone_net, via.net.as_deref()).max(zone.clearance);
            add_knockout(&mut clearance_holes, &shape, gap, max_error);
        } else {
            // Same-net vias connect directly in a solid fill (no thermal
            // relief for vias outside hatch-pattern zones -- see the module
            // doc comment on `ZONE_FILL_MODE::HatchPattern` not being
            // ported), but still spoke-tested the same way a thermal pad
            // would be would be if the zone's own `pad_connection` calls
            // for thermal relief, matching a THT via's pad-like behavior.
            if matches!(zone.pad_connection, PadConnection::Thermal | PadConnection::ThtThermal) {
                via_thermal.push((i, vb));
            }
        }
    }

    // -------------------------------------------------------------------
    // Different-net zone clearances and same-net higher-priority zone
    // subtraction (`buildDifferentNetZoneClearances` / `subtractHigherPriorityZones`).
    // -------------------------------------------------------------------
    let mut higher_priority_same_net = ShapePolySet::new();
    for other in &input.other_zones {
        if other.layer != layer {
            continue;
        }
        let ob = { let s = Shape::Polygon { pts: other.outline.clone() }; bbox_of(&s) };
        if !bboxes_intersect(ob, zone_bbox) {
            continue;
        }
        // `ZONE::SameNet`: net-code equality, so two netless zones match.
        let same_net = other.net.as_deref() == zone_net;
        // `buildDifferentNetZoneClearances`'s `knockoutZoneClearance`:
        // `if (aKnockout->HigherPriority(aZone) && !aKnockout->SameNet(aZone))`
        // -- a zone is only ever knocked out by an *other*, same-layer zone
        // that outranks it; a higher-priority zone fills right up to (its
        // own clearance from) nothing, same-net or not. Without this gate,
        // every pair of overlapping zones would knock each other out
        // (double-counting the gap, and wrongly shrinking the one that
        // should win outright).
        if other.priority <= zone.priority {
            continue;
        }
        if same_net {
            if !other.teardrop {
                higher_priority_same_net.add_outline(other.outline.clone());
            }
        } else {
            // `knockoutZoneClearance` itself computes just
            // `max(PHYSICAL_CLEARANCE_CONSTRAINT, CLEARANCE_CONSTRAINT)`
            // with no `zone.clearance` floor -- but empirically (see
            // PARITY.md), flooring at it here matches real `kicad-cli`
            // refills *better*, not worse, confirming that gap is really in
            // this crate's much simpler net-class clearance resolution
            // (`clearance_fn`, typically `eda_drc::constraints::clearance`)
            // under-resolving relative to KiCad's full `DRC_ENGINE` on
            // these boards' specific net-class setups -- the same
            // DRC-rule-fidelity gap `eda_drc`'s own doc comments already
            // call out, not something to paper over by *removing* a correct
            // floor elsewhere.
            let gap = clearance_fn(zone_net, other.net.as_deref()).max(zone.clearance);
            let mut sps = ShapePolySet::new();
            sps.add_outline(other.outline.clone());
            if gap > 0 {
                sps.inflate(gap, round_corner, max_error as i32, false);
            }
            for poly in sps.polys {
                clearance_holes.add_polygon(poly);
            }
        }
    }

    // -------------------------------------------------------------------
    // Rule area (keepout) copper-pour exclusion -- not part of upstream's
    // `fillCopperZone` pipeline order comment at the top of this file
    // (which predates this knockout), but the same idea as the zone/pad/
    // track knockouts above: a hole cut from `fill` before the min-width/
    // island passes run, so a keepout-adjacent sliver is pruned by the
    // same geometry those passes already apply to every other knockout.
    for keepout in &input.keepouts {
        if keepout.layer != layer {
            continue;
        }
        let kb = { let s = Shape::Polygon { pts: keepout.outline.clone() }; bbox_of(&s) };
        if !bboxes_intersect(kb, zone_bbox) {
            continue;
        }
        clearance_holes.add_outline(keepout.outline.clone());
    }

    // -------------------------------------------------------------------
    // Thermal relief spokes (`buildThermalSpokes` + the spoke-keep loop).
    // -------------------------------------------------------------------
    let mut test_areas = fill.clone();
    test_areas.boolean_subtract(&clearance_holes);
    if half_min_width - epsilon > epsilon {
        test_areas.deflate(half_min_width - epsilon, fast_corner, max_error as i32);
        test_areas.inflate(half_min_width - epsilon, fast_corner, max_error as i32, false);
    }

    let mut all_spokes = Vec::new();
    for &(pad_idx, pad_bbox) in &thermal_pads {
        all_spokes.extend(spokes::build_spokes(pad_bbox, zone.thermal_gap, zone.thermal_spoke_width.min(pad_bbox.2 - pad_bbox.0).min(pad_bbox.3 - pad_bbox.1)));
        let _ = pad_idx;
    }
    for &(via_idx, via_bbox) in &via_thermal {
        all_spokes.extend(spokes::build_spokes(via_bbox, zone.thermal_gap, zone.thermal_spoke_width));
        let _ = via_idx;
    }

    let keep = spokes::keep_spokes(&all_spokes, &test_areas);
    for i in keep {
        fill.add_outline(all_spokes[i].outline.clone());
    }

    fill.boolean_subtract(&clearance_holes);

    // -------------------------------------------------------------------
    // Prune features that don't meet minimum-width criteria.
    // -------------------------------------------------------------------
    if half_min_width - epsilon > epsilon {
        fill.deflate(half_min_width - epsilon, fast_corner, max_error as i32);
    }

    remove_sub_min_thickness_slivers(&mut fill, zone.min_thickness);

    // Hatch-pattern fill is not ported (see the module doc comment) --
    // solid (`Polygons`) fill mode is used for both cases.
    let _ = zone.fill_mode == FillMode::HatchPattern;

    fill.fracture(true);
    // `connect_nearby_polys` is a no-op here (see the module doc comment).

    if half_min_width - epsilon > epsilon {
        fill.inflate(half_min_width - epsilon, round_corner, max_error as i32, true);
    }

    // `aMaxExtents`: the zone's own outline (the deflate/inflate min-width
    // cycle above can round or grow the fill slightly past it) intersected
    // with the board outline, if one was supplied.
    let mut max_extents = ShapePolySet::from_outline(zone_outline.clone());
    if let Some(board_outline) = &input.board_outline {
        max_extents.boolean_intersection(&ShapePolySet::from_outline(board_outline.clone()));
    }
    fill.boolean_intersection(&max_extents);
    fill.boolean_subtract(&clearance_holes);

    postknockout_min_width_prune_if_needed(zone, &higher_priority_same_net, &mut fill, max_error, half_min_width, epsilon);

    if higher_priority_same_net.outline_count() > 0 {
        fill.boolean_subtract(&higher_priority_same_net);
    }

    fill.fracture(true);

    apply_island_removal(&mut fill, zone, input, layer);

    fill
}

fn postknockout_min_width_prune_if_needed(zone: &Zone, higher_priority_same_net: &ShapePolySet, fill: &mut ShapePolySet, max_error: i64, half_min_width: i64, epsilon: i64) {
    if higher_priority_same_net.outline_count() == 0 || half_min_width - epsilon <= epsilon {
        return;
    }
    // `postKnockoutMinWidthPrune`: re-prune min-width violations introduced
    // by the upcoming same-net higher-priority knockout, before it's
    // actually applied (matching upstream's ordering: this pass runs on the
    // pre-knockout fill, using the still-un-knocked-out area as a buffer).
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
fn apply_island_removal(fill: &mut ShapePolySet, zone: &Zone, input: &FillInput, layer: &str) {
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

fn inflate_bbox(b: (i64, i64, i64, i64), by: i64) -> (i64, i64, i64, i64) {
    (b.0 - by, b.1 - by, b.2 + by, b.3 + by)
}

/// Flattens a fill's outlines+holes into the `Polygon` list the `.kicad_pcb`
/// exporter writes as `filled_polygon` (post-`Fracture`, each polygon is
/// already a single slitted outline with no separate holes).
pub fn to_filled_polygons(fill: &ShapePolySet) -> Vec<Polygon> {
    fill.polys.clone()
}
