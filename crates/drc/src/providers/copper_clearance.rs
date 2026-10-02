//! Ported from `pcbnew/drc/drc_test_provider_copper_clearance.cpp`: copper
//! clearance between pads, tracks, vias and zones on different nets, per
//! layer. Zone-vs-item and zone-vs-zone checks test against each zone's
//! real, computed fill (`eda_drc::fill::fill_all_zones`) -- one check per
//! disjoint fragment, keeping the worst (closest) -- falling back to the
//! zone's raw outline only when it has no stable id to look a fill up by.
//!
//! Generated: `DRCE_CLEARANCE`, `DRCE_HOLE_CLEARANCE`, `DRCE_TRACKS_CROSSING`,
//! `DRCE_SHORTING_ITEMS`, `DRCE_ZONES_INTERSECT`.

use crate::board::{DrcBoard, DrcPad, DrcTrackSeg, DrcVia, DrcZone};
use crate::constraints;
use crate::fill::FillResults;
use crate::item::{format_um, DrcRefItem, DrcViolation, ErrorType};
use crate::kimath::Shape;
use crate::pcbexpr::Facts;
use eda_model::{BoardRules, PadKind};
use std::collections::HashMap;

/// Every footprint whose courtyard contains each pad/track/via's own
/// representative point, precomputed once per board -- `Facts::
/// inside_courtyards`'s only data source (`A.insideCourtyard('NAME')`, task
/// item 4; see that field's doc comment). Built once and looked up per item
/// rather than recomputed per *pair*: `check()` resolves clearance for
/// O(items^2) pairs on a real board, and this containment test is itself
/// O(footprints), so computing it inline per pair would be
/// O(items^2 * footprints) -- the same reasoning `CompiledClearanceRules`
/// documents for compiling a rule's condition once instead of per pair.
pub(crate) struct CourtyardMembership {
    pads: HashMap<String, Vec<String>>,
    tracks: HashMap<String, Vec<String>>,
    vias: HashMap<String, Vec<String>>,
}

const NO_COURTYARDS: &[String] = &[];

impl CourtyardMembership {
    pub(crate) fn new(board: &DrcBoard) -> Self {
        let containing = |x: i64, y: i64| -> Vec<String> {
            board.footprints.iter().filter(|f| x > f.courtyard.0 && x < f.courtyard.2 && y > f.courtyard.1 && y < f.courtyard.3).map(|f| f.id.clone()).collect()
        };
        CourtyardMembership {
            pads: board.pads.iter().map(|p| (p.id.clone(), containing(p.center.x, p.center.y))).collect(),
            tracks: board.tracks.iter().map(|t| (t.id.clone(), containing((t.a.x + t.b.x) / 2, (t.a.y + t.b.y) / 2))).collect(),
            vias: board.vias.iter().map(|v| (v.id.clone(), containing(v.at.x, v.at.y))).collect(),
        }
    }
    fn pad(&self, id: &str) -> &[String] {
        self.pads.get(id).map(Vec::as_slice).unwrap_or(NO_COURTYARDS)
    }
    fn track(&self, id: &str) -> &[String] {
        self.tracks.get(id).map(Vec::as_slice).unwrap_or(NO_COURTYARDS)
    }
    fn via(&self, id: &str) -> &[String] {
        self.vias.get(id).map(Vec::as_slice).unwrap_or(NO_COURTYARDS)
    }
}

/// This item's resolved net-class name, for a `.kicad_dru` condition's
/// `A.NetClass`/`B.NetClass`/`hasNetclass()` (task item 4) -- `"Default"`
/// for an unclassed net, matching KiCad's own convention that every net
/// belongs to at least the implicit default class.
pub(crate) fn net_class_name<'a>(rules: &'a BoardRules, net: Option<&str>) -> &'a str {
    net.and_then(|n| rules.class_of(n)).map(|c| c.name.as_str()).unwrap_or("Default")
}

fn facts_of_pad<'a>(rules: &'a BoardRules, p: &'a DrcPad, courtyards: &'a CourtyardMembership) -> Facts<'a> {
    Facts { item_type: "Pad", net_class: net_class_name(rules, p.net.as_deref()), net_name: p.net.as_deref().unwrap_or(""), reference: &p.footprint_ref, inside_courtyards: courtyards.pad(&p.id) }
}
/// Also used by `providers::track_width` to build the `A`-only `Facts` a
/// `.kicad_dru` `track_width` rule's condition is evaluated against (task
/// item 1) -- see `constraints::CompiledWidthRules`'s doc comment for why a
/// track/arc built the same way a clearance-family rule's condition sees it
/// is exactly what KiCad's own one-item `TRACK_WIDTH_CONSTRAINT` evaluation
/// needs.
pub(crate) fn facts_of_track<'a>(rules: &'a BoardRules, t: &'a DrcTrackSeg, courtyards: &'a CourtyardMembership) -> Facts<'a> {
    Facts { item_type: "Track", net_class: net_class_name(rules, t.net.as_deref()), net_name: t.net.as_deref().unwrap_or(""), reference: "", inside_courtyards: courtyards.track(&t.id) }
}
fn facts_of_via<'a>(rules: &'a BoardRules, v: &'a DrcVia, courtyards: &'a CourtyardMembership) -> Facts<'a> {
    Facts { item_type: "Via", net_class: net_class_name(rules, v.net.as_deref()), net_name: v.net.as_deref().unwrap_or(""), reference: "", inside_courtyards: courtyards.via(&v.id) }
}
fn facts_of_zone<'a>(rules: &'a BoardRules, z: &'a DrcZone) -> Facts<'a> {
    // Not resolved against any footprint's courtyard -- a zone's own
    // outline is in no way a point, and `insideCourtyard` scoped to a zone
    // is not a pattern that's shown up in any sampled `.kicad_dru` file.
    Facts { item_type: "Zone", net_class: net_class_name(rules, z.net.as_deref()), net_name: z.net.as_deref().unwrap_or(""), reference: "", inside_courtyards: NO_COURTYARDS }
}

/// A non-plated hole has no copper at all (it is a mechanical hole only),
/// so it never participates in copper clearance/shorting -- exactly
/// `testPadAgainstItem`'s `pad->GetAttribute() == PAD_ATTRIB::NPTH &&
/// !pad->FlashLayer(aLayer) -> testClearance = testShorting = false`
/// (this model has no per-layer NPTH "flashing" override, so the
/// un-flashed case is the only one that applies). Its *hole* still needs
/// clearance from foreign copper -- that path is untouched.
fn flashed(p: &DrcPad) -> bool {
    p.kind != PadKind::NonPlatedHole
}

fn ref_item(desc: String, pos: eda_model::ir::Point, id: String) -> DrcRefItem {
    DrcRefItem { description: desc, pos: (pos.x, pos.y), id }
}

fn pad_ref(p: &DrcPad) -> DrcRefItem {
    ref_item(format!("Pad {} [{}] of {}", p.number, p.net.as_deref().unwrap_or("<no net>"), p.footprint_ref), p.center, p.id.clone())
}
fn track_ref(t: &DrcTrackSeg) -> DrcRefItem {
    ref_item(format!("Track [{}] on {}", t.net.as_deref().unwrap_or("<no net>"), t.layer), t.a, t.id.clone())
}
fn via_ref(v: &DrcVia) -> DrcRefItem {
    ref_item(format!("Via [{}] on {}-{}", v.net.as_deref().unwrap_or("<no net>"), v.from_layer, v.to_layer), v.at, v.id.clone())
}
fn zone_ref(z: &DrcZone) -> DrcRefItem {
    ref_item(format!("Zone [{}] on {}", z.net.as_deref().unwrap_or("<no net>"), z.layer), z.outline[0], z.id.clone())
}

/// `shape`'s closest collision against any of `zone`'s real-fill fragments
/// (or its raw outline, if it has none on record), i.e. the same
/// `Option<(actual, pos)>` a single `.collides()` call would give against
/// one shape -- `fill_zone`'s islands can leave a zone's net nowhere near
/// where its outline alone would suggest, so every fragment needs its own
/// test rather than one test against the outline as a whole.
fn collides_zone(shape: &Shape, zone: &DrcZone, fills: &FillResults, clearance: i64) -> Option<(i64, eda_model::ir::Point)> {
    fills.fragments_or(&zone.id, zone.shape()).iter().filter_map(|frag| shape.collides(frag, clearance)).min_by_key(|(actual, _)| *actual)
}

/// Same idea, both sides a zone's own fragments.
fn collides_zone_zone(a: &DrcZone, b: &DrcZone, fills: &FillResults, clearance: i64) -> Option<(i64, eda_model::ir::Point)> {
    let frags_a = fills.fragments_or(&a.id, a.shape());
    let frags_b = fills.fragments_or(&b.id, b.shape());
    frags_a.iter().flat_map(|fa| frags_b.iter().filter_map(move |fb| fa.collides(fb, clearance))).min_by_key(|(actual, _)| *actual)
}

fn same_logical_pad(a: &DrcPad, b: &DrcPad) -> bool {
    a.footprint_ref == b.footprint_ref && a.number == b.number
}

/// `GetDRCEpsilon()` (`ADVANCED_CFG::m_DRCEpsilon`, 0.0005 mm). With this
/// IR's whole-µm distances, `actual < clearance - 0.5` is exactly
/// `actual < clearance`, so the half-micron rounds to 0 here.
const DRC_EPSILON: i64 = 0;

/// `DRC_TEST_PROVIDER_COPPER_CLEARANCE::sub_e`: every `Collide` against a
/// clearance uses `max( 0, clearance - epsilon )`.
fn sub_e(clearance: i64) -> i64 {
    (clearance - DRC_EPSILON).max(0)
}

/// `DRC_ENGINE::m_errorLimits` for the types this provider reports:
/// `RunTests` sets `EXTENDED_ERROR_LIMIT` (499) for `DRCE_CLEARANCE`,
/// `ERROR_LIMIT` (199) for the rest, and every test gates on
/// `IsErrorLimitExceeded` *before* it runs.
struct Limits {
    clearance: i64,
    shorting: i64,
    hole: i64,
    crossing: i64,
}

impl Limits {
    fn new() -> Self {
        Limits { clearance: 499, shorting: 199, hole: 199, crossing: 199 }
    }
    fn report(&mut self, v: DrcViolation, out: &mut Vec<DrcViolation>) {
        match v.error_type {
            t if t == ErrorType::Clearance.key() => self.clearance -= 1,
            t if t == ErrorType::ShortingItems.key() => self.shorting -= 1,
            t if t == ErrorType::HoleClearance.key() => self.hole -= 1,
            t if t == ErrorType::TracksCrossing.key() => self.crossing -= 1,
            _ => {}
        }
        out.push(v);
    }
}

/// One copper item a track/via/pad query can hit (`m_CopperItemRTreeCache`'s
/// contents, minus copper graphics, which this provider doesn't test yet).
#[derive(Clone, Copy)]
enum Item<'a> {
    Track(usize, &'a DrcTrackSeg),
    Via(usize, &'a DrcVia),
    Pad(usize, &'a DrcPad),
}

impl<'a> Item<'a> {
    fn net(&self) -> Option<&'a str> {
        match self {
            Item::Track(_, t) => t.net.as_deref(),
            Item::Via(_, v) => v.net.as_deref(),
            Item::Pad(_, p) => p.net.as_deref(),
        }
    }
    /// Position in `m_board->Tracks()` (tracks then vias) for the
    /// pointer-order pair dedupe, or the pad's own index.
    fn order(&self, n_tracks: usize) -> usize {
        match self {
            Item::Track(i, _) => *i,
            Item::Via(i, _) => n_tracks + *i,
            Item::Pad(i, _) => *i,
        }
    }
    fn is_track_like(&self) -> bool {
        !matches!(self, Item::Pad(..))
    }
    fn reference(&self) -> DrcRefItem {
        match self {
            Item::Track(_, t) => track_ref(t),
            Item::Via(_, v) => via_ref(v),
            Item::Pad(_, p) => pad_ref(p),
        }
    }
    fn facts(&self, rules: &'a BoardRules, courtyards: &'a CourtyardMembership) -> Facts<'a> {
        match self {
            Item::Track(_, t) => facts_of_track(rules, t, courtyards),
            Item::Via(_, v) => facts_of_via(rules, v, courtyards),
            Item::Pad(_, p) => facts_of_pad(rules, p, courtyards),
        }
    }
    /// `GetEffectiveHoleShape()`, if this item has a hole.
    fn hole(&self) -> Option<Shape> {
        match self {
            Item::Track(..) => None,
            Item::Via(_, v) => Some(v.hole()),
            Item::Pad(_, p) => p.hole.clone(),
        }
    }
}

/// `PCB_VIA::IsOnLayer`: inside the via's own layer span.
fn via_on_layer(board: &DrcBoard, v: &DrcVia, layer: &str) -> bool {
    let idx = |l: &str| board.layers.iter().position(|x| x == l);
    match (idx(&v.from_layer), idx(&v.to_layer), idx(layer)) {
        (Some(a), Some(b), Some(l)) => l >= a.min(b) && l <= a.max(b),
        _ => true,
    }
}

struct Ctx<'a> {
    rules: &'a BoardRules,
    courtyards: &'a CourtyardMembership,
    compiled: &'a constraints::CompiledClearanceRules,
}

/// `DRC_TEST_PROVIDER_COPPER_CLEARANCE::testSingleLayerItemAgainstItem`
/// (`item` is always a track, arc or via here). Returns `false` once it
/// has reported a violation -- the caller then stops visiting candidates
/// for this item and layer (`GetReportAllTrackErrors()` is off for
/// kicad-cli's default run).
fn test_single_layer_item_against_item(ctx: &Ctx, layer: &str, item: Item, item_shape: &Shape, other: Item, limits: &mut Limits, out: &mut Vec<DrcViolation>) -> bool {
    let mut test_clearance = limits.clearance > 0;
    let mut test_shorting = limits.shorting > 0;
    let test_holes = limits.hole > 0;
    if item.net() == other.net() {
        test_clearance = false;
        test_shorting = false;
    }
    // A pad this layer doesn't flash (an NPTH here) is tested as its hole.
    let other_shape = match other {
        Item::Pad(_, p) if !flashed(p) => {
            test_clearance = false;
            test_shorting = false;
            p.hole.clone().unwrap_or_else(|| p.copper.clone())
        }
        Item::Pad(_, p) => p.copper.clone(),
        Item::Track(_, t) => t.shape(),
        Item::Via(_, v) => v.shape(),
    };

    let mut clearance = -1;
    if test_clearance || test_shorting {
        clearance = constraints::clearance_with_custom_rules(ctx.rules, item.net(), other.net(), layer, &item.facts(ctx.rules, ctx.courtyards), &other.facts(ctx.rules, ctx.courtyards), ctx.compiled);
    }
    if clearance > 0 {
        // Special processing for track:track intersections (`PCB_TRACE_T`
        // only -- an arc never takes this path).
        if let (Item::Track(_, a), Item::Track(_, b)) = (item, other) {
            if !a.is_arc() && !b.is_arc() && limits.crossing > 0 {
                if let Some(pt) = crate::kimath::Seg::new(a.a, a.b).intersect(&crate::kimath::Seg::new(b.a, b.b)) {
                    limits.report(DrcViolation::new(ErrorType::TracksCrossing, "", vec![track_ref(a), track_ref(b), ref_item("crossing point".into(), pt, String::new())]), out);
                    return false;
                }
            }
        }
        if let Some((actual, _pos)) = item_shape.collides(&other_shape, sub_e(clearance)) {
            if actual == 0 && test_shorting {
                let name = |n: Option<&str>| n.unwrap_or("<no net>").to_string();
                limits.report(DrcViolation::new(ErrorType::ShortingItems, format!("(nets {} and {})", name(item.net()), name(other.net())), vec![item.reference(), other.reference()]), out);
                return false;
            } else if test_clearance {
                limits.report(DrcViolation::new(ErrorType::Clearance, format!("(clearance {}; actual {})", format_um(clearance), format_um(actual)), vec![item.reference(), other.reference()]), out);
                return false;
            }
        }
    }

    if test_holes && (item.hole().is_some() || other.hole().is_some()) {
        // `a[ii]`'s copper against `b[ii]`'s hole, both ways round.
        for (a, a_shape, b) in [(item, item_shape.clone(), other), (other, other_shape.clone(), item)] {
            let Some(hole) = b.hole() else { continue };
            // `HOLE_CLEARANCE_CONSTRAINT`, tested "even if clearance is 0,
            // because the item cannot be inside (or intersect) the hole".
            let clearance = constraints::hole_clearance_min(ctx.rules).max(0);
            if let Some((actual, _)) = a_shape.collides(&hole, sub_e(clearance)) {
                limits.report(DrcViolation::new(ErrorType::HoleClearance, format!("(clearance {}; actual {})", format_um(clearance), format_um(actual)), vec![a.reference(), b.reference()]), out);
                return false;
            }
        }
    }
    true
}

/// `DRC_TEST_PROVIDER_COPPER_CLEARANCE::testPadAgainstItem`. Tracks and
/// vias only get the hole half here (their clearance is
/// `testTrackClearances`' job); the pad's copper is tested against the
/// *other* item's hole.
fn test_pad_against_item(ctx: &Ctx, layer: &str, pad: &DrcPad, pad_i: usize, other: Item, limits: &mut Limits, out: &mut Vec<DrcViolation>) {
    let mut test_clearance = limits.clearance > 0;
    let mut test_shorting = limits.shorting > 0;
    let mut test_holes = limits.hole > 0;
    let other_pad = if let Item::Pad(_, p) = other { Some(p) } else { None };
    let other_via = if let Item::Via(_, v) = other { Some(v) } else { None };

    if let Some(op) = other_pad {
        if op.footprint_ref == pad.footprint_ref && same_logical_pad(pad, op) {
            test_holes = false;
        }
    }
    // A NPTH has no cylinder.
    if !flashed(pad) || other_pad.is_some_and(|op| !flashed(op)) {
        test_clearance = false;
        test_shorting = false;
    }
    // Track clearances are tested in testTrackClearances().
    if other.is_track_like() {
        test_clearance = false;
        test_shorting = false;
    }
    // Other objects of the same (defined) net get a waiver on clearance and hole tests.
    if other.net().is_some() && other.net() == pad.net.as_deref() {
        return;
    }
    let pad_drilled = pad.drill_round.is_some_and(|d| d > 0) || pad.drill_slot.is_some();
    let other_drilled = other_pad.is_some_and(|op| op.drill_round.is_some_and(|d| d > 0) || op.drill_slot.is_some()) || other_via.is_some_and(|v| v.drill > 0);
    if !pad_drilled && !other_drilled {
        test_holes = false;
    }
    if !test_clearance && !test_shorting && !test_holes {
        return;
    }

    if let Some(op) = other_pad {
        if same_logical_pad(pad, op) {
            // Equivalent pads must carry the same net.
            if test_shorting && pad.net.is_some() && pad.net != op.net {
                limits.report(DrcViolation::new(ErrorType::ShortingItems, format!("(nets {} and {})", pad.net.as_deref().unwrap_or(""), op.net.as_deref().unwrap_or("")), vec![pad_ref(pad), pad_ref(op)]), out);
            }
            return;
        }
    }

    if test_clearance || test_shorting {
        let c = constraints::clearance_with_custom_rules(ctx.rules, pad.net.as_deref(), other.net(), layer, &facts_of_pad(ctx.rules, pad, ctx.courtyards), &other.facts(ctx.rules, ctx.courtyards), ctx.compiled);
        let other_shape = match other {
            Item::Pad(_, p) => p.copper.clone(),
            _ => unreachable!("tracks/vias never reach the clearance half"),
        };
        if c > 0 {
            if let Some((actual, _)) = pad.copper.collides(&other_shape, sub_e(c)) {
                if actual == 0 && pad.net.is_some() && other.net().is_some() && test_shorting {
                    limits.report(DrcViolation::new(ErrorType::ShortingItems, format!("(nets {} and {})", pad.net.as_deref().unwrap_or(""), other.net().unwrap_or("")), vec![pad_ref(pad), other.reference()]), out);
                    test_holes = false;
                } else if test_clearance {
                    limits.report(DrcViolation::new(ErrorType::Clearance, format!("(clearance {}; actual {})", format_um(c), format_um(actual)), vec![pad_ref(pad), other.reference()]), out);
                    test_holes = false;
                }
            }
        }
    }

    if test_holes {
        let c = constraints::hole_clearance_min(ctx.rules);
        let test_hole = |hole: Shape, clearance: i64, limits: &mut Limits, out: &mut Vec<DrcViolation>| {
            if clearance > 0 {
                if let Some((actual, _)) = pad.copper.collides(&hole, sub_e(clearance)) {
                    limits.report(DrcViolation::new(ErrorType::HoleClearance, format!("(clearance {}; actual {})", format_um(clearance), format_um(actual)), vec![pad_ref(pad), other.reference()]), out);
                    return true;
                }
            }
            false
        };
        let mut done = false;
        if let Some(op) = other_pad {
            if let Some(h) = op.hole.clone() {
                // `if( !pad->FlashLayer( aLayer ) ) clearance = 0;`
                done = test_hole(h, if flashed(pad) { c } else { 0 }, limits, out);
            }
        }
        if !done {
            if let Some(v) = other_via {
                test_hole(v.hole(), c, limits, out);
            }
        }
    }
    let _ = pad_i;
}

/// `DRC_TEST_PROVIDER_COPPER_CLEARANCE::testItemAgainstZone`.
#[allow(clippy::too_many_arguments)]
fn test_item_against_zone(ctx: &Ctx, layer: &str, item: Item, item_shape: &Shape, zone: &DrcZone, fills: &FillResults, limits: &mut Limits, out: &mut Vec<DrcViolation>) {
    if zone.net.is_some() && zone.net.as_deref() == item.net() {
        return;
    }
    let mut test_clearance = limits.clearance > 0;
    let test_holes = limits.hole > 0;
    if let Item::Pad(_, p) = item {
        let plated_hole = p.hole.is_some() && p.kind == PadKind::ThroughHole;
        if !flashed(p) && !plated_hole {
            test_clearance = false;
        }
    }
    if test_clearance {
        let c = constraints::clearance_with_custom_rules(ctx.rules, item.net(), zone.net.as_deref(), layer, &item.facts(ctx.rules, ctx.courtyards), &facts_of_zone(ctx.rules, zone), ctx.compiled);
        if c > 0 {
            if let Some((actual, _)) = collides_zone(item_shape, zone, fills, sub_e(c)) {
                limits.report(DrcViolation::new(ErrorType::Clearance, format!("(clearance {}; actual {})", format_um(c), format_um(actual)), vec![item.reference(), zone_ref(zone)]), out);
            }
        }
    }
    if test_holes {
        if let Some(hole) = item.hole() {
            let c = constraints::hole_clearance_min(ctx.rules);
            if c > 0 {
                if let Some((actual, _)) = collides_zone(&hole, zone, fills, sub_e(c)) {
                    limits.report(DrcViolation::new(ErrorType::HoleClearance, format!("(clearance {}; actual {})", format_um(c), format_um(actual)), vec![item.reference(), zone_ref(zone)]), out);
                }
            }
        }
    }
}

pub fn check(board: &DrcBoard, rules: &BoardRules) -> Vec<DrcViolation> {
    let mut out = Vec::new();
    let mut limits = Limits::new();
    // `DRC_RTREE` queries are bounded by one run-wide worst-case clearance
    // (`BOARD::m_DRCMaxClearance`, see `crate::constraints::
    // worst_case_clearance`'s doc comment) -- the exact-pair clearance is
    // still resolved per pair.
    let worst_clearance = constraints::worst_case_clearance(rules);
    let fills = crate::fill::fill_all_zones(board, rules);
    // Compiled once per board, reused for every pair -- see
    // `constraints::CompiledClearanceRules`'s doc comment.
    let compiled_rules = constraints::CompiledClearanceRules::new(rules);
    let courtyards = CourtyardMembership::new(board);
    let ctx = Ctx { rules, courtyards: &courtyards, compiled: &compiled_rules };
    let n_tracks = board.tracks.len();

    // Per-layer copper item index: `m_CopperItemRTreeCache`.
    struct LayerIndex<'a> {
        items: Vec<Item<'a>>,
        tree: crate::rtree::DrcRTree,
    }
    let layer_index: HashMap<&str, LayerIndex> = board
        .layers
        .iter()
        .map(|layer| {
            let mut items: Vec<Item> = Vec::new();
            items.extend(board.tracks.iter().enumerate().filter(|(_, t)| &t.layer == layer).map(|(i, t)| Item::Track(i, t)));
            items.extend(board.vias.iter().enumerate().filter(|(_, v)| via_on_layer(board, v, layer)).map(|(i, v)| Item::Via(i, v)));
            items.extend(board.pads.iter().enumerate().filter(|(_, p)| p.layers.iter().any(|l| l == layer)).map(|(i, p)| Item::Pad(i, p)));
            let mut tree = crate::rtree::DrcRTree::new(worst_clearance.max(1));
            for (k, it) in items.iter().enumerate() {
                let bbox = match it {
                    Item::Track(_, t) => t.shape().bbox(0),
                    Item::Via(_, v) => v.shape().bbox(0),
                    Item::Pad(_, p) => p.copper.bbox(0),
                };
                tree.insert(k, bbox);
            }
            (layer.as_str(), LayerIndex { items, tree })
        })
        .collect();
    fn zones_on<'b>(board: &'b DrcBoard, layer: &'b str) -> impl Iterator<Item = &'b DrcZone> {
        board.zones.iter().filter(move |z| z.layer == layer)
    }

    // ---- testTrackClearances: every track, arc and via, per copper layer ----
    let track_items = board.tracks.iter().enumerate().map(|(i, t)| Item::Track(i, t)).chain(board.vias.iter().enumerate().map(|(i, v)| Item::Via(i, v)));
    for item in track_items {
        let item_shape = match item {
            Item::Track(_, t) => t.shape(),
            Item::Via(_, v) => v.shape(),
            Item::Pad(..) => unreachable!(),
        };
        let layers: Vec<&str> = match item {
            Item::Track(_, t) => vec![t.layer.as_str()],
            Item::Via(_, v) => board.layers.iter().map(String::as_str).filter(|l| via_on_layer(board, v, l)).collect(),
            Item::Pad(..) => unreachable!(),
        };
        for layer in layers {
            let Some(idx) = layer_index.get(layer) else { continue };
            for k in idx.tree.query(item_shape.bbox(worst_clearance)) {
                let other = idx.items[k];
                // Filter: same netcode (net 0 included), and each
                // track/arc/via pair only once (pointer order).
                if other.net() == item.net() {
                    continue;
                }
                if other.is_track_like() && item.order(n_tracks) > other.order(n_tracks) {
                    continue;
                }
                if other.is_track_like() && item.order(n_tracks) == other.order(n_tracks) {
                    continue;
                }
                if !test_single_layer_item_against_item(&ctx, layer, item, &item_shape, other, &mut limits, &mut out) {
                    break;
                }
            }
            for zone in zones_on(board, layer) {
                test_item_against_zone(&ctx, layer, item, &item_shape, zone, &fills, &mut limits, &mut out);
            }
        }
    }

    // ---- testPadClearances: every pad, per copper layer it's on ----
    for (pi, pad) in board.pads.iter().enumerate() {
        for layer in &pad.layers {
            let Some(idx) = layer_index.get(layer.as_str()) else { continue };
            for k in idx.tree.query(pad.copper.bbox(worst_clearance)) {
                let other = idx.items[k];
                if let Item::Pad(oi, _) = other {
                    if pi >= oi {
                        continue;
                    }
                }
                test_pad_against_item(&ctx, layer, pad, pi, other, &mut limits, &mut out);
            }
            for zone in zones_on(board, layer) {
                test_item_against_zone(&ctx, layer, Item::Pad(pi, pad), &pad.copper, zone, &fills, &mut limits, &mut out);
            }
        }
    }

    // ---- testZonesToZones ----
    // Teardrop areas are tested as tracks, not zones; rule areas never
    // reach `board.zones`. Same-net pairs only matter at equal priority and
    // compare *outlines*; different-net pairs compare *fills*.
    for layer in &board.layers {
        let zones: Vec<&DrcZone> = zones_on(board, layer).filter(|z| !z.teardrop).collect();
        for i in 0..zones.len() {
            for j in (i + 1)..zones.len() {
                let (a, b) = (zones[i], zones[j]);
                let same_net = a.net == b.net;
                if same_net && a.priority != b.priority {
                    continue;
                }
                if same_net {
                    if a.shape().collides(&b.shape(), 0).is_some() {
                        out.push(DrcViolation::new(ErrorType::ZonesIntersect, "(intersecting zones must have distinct priorities)", vec![zone_ref(a), zone_ref(b)]));
                    }
                } else if limits.clearance > 0 {
                    let c = constraints::clearance_with_custom_rules(rules, a.net.as_deref(), b.net.as_deref(), layer, &facts_of_zone(rules, a), &facts_of_zone(rules, b), &compiled_rules);
                    if c > 0 {
                        if let Some((actual, _)) = collides_zone_zone(a, b, &fills, sub_e(c)) {
                            limits.report(DrcViolation::new(ErrorType::Clearance, format!("(clearance {}; actual {})", format_um(c), format_um(actual)), vec![zone_ref(a), zone_ref(b)]), &mut out);
                        }
                    }
                }
            }
        }
    }

    // ---- testTeardropClearances: each teardrop against every other zone on its layer ----
    for (ti, td) in board.zones.iter().enumerate().filter(|(_, z)| z.teardrop) {
        let td_shapes = fills.fragments_or(&td.id, td.shape());
        for (zi, zone) in board.zones.iter().enumerate().filter(|(_, z)| z.layer == td.layer) {
            if zi == ti || (zone.teardrop && zi < ti) {
                continue;
            }
            // `testItemAgainstZone( teardrop, zone, layer )`: a teardrop has no hole.
            if zone.net.is_some() && zone.net == td.net {
                continue;
            }
            if limits.clearance <= 0 {
                break;
            }
            let c = constraints::clearance_with_custom_rules(rules, td.net.as_deref(), zone.net.as_deref(), &td.layer, &facts_of_zone(rules, td), &facts_of_zone(rules, zone), &compiled_rules);
            if c > 0 {
                if let Some((actual, _)) = td_shapes.iter().filter_map(|s| collides_zone(s, zone, &fills, sub_e(c))).min_by_key(|(a, _)| *a) {
                    limits.report(DrcViolation::new(ErrorType::Clearance, format!("(clearance {}; actual {})", format_um(c), format_um(actual)), vec![zone_ref(td), zone_ref(zone)]), &mut out);
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{Point, Side};

    fn empty_board(tracks: Vec<DrcTrackSeg>, pads: Vec<DrcPad>) -> DrcBoard {
        DrcBoard { layers: vec!["F.Cu".into()], outline: vec![], pads, tracks, vias: vec![], zones: vec![], keepouts: vec![], footprints: vec![], shapes: vec![], texts: vec![], silk_items: vec![] }
    }

    fn seg(id: &str, net: Option<&str>, a: (i64, i64), b: (i64, i64)) -> DrcTrackSeg {
        DrcTrackSeg { id: id.into(), net: net.map(String::from), layer: "F.Cu".into(), width: 200, a: Point { x: a.0, y: a.1 }, b: Point { x: b.0, y: b.1 }, arc_mid: None }
    }

    fn pad_with_hole(id: &str, net: Option<&str>, center: (i64, i64), hole_r: i64) -> DrcPad {
        let c = Point { x: center.0, y: center.1 };
        DrcPad {
            id: id.into(),
            footprint_ref: id.into(),
            number: "1".into(),
            net: net.map(String::from),
            center: c,
            side: Side::Top,
            kind: PadKind::ThroughHole,
            layers: vec!["F.Cu".into()],
            copper: Shape::Circle { c, r: 300 },
            hole: Some(Shape::Circle { c, r: hole_r }),
            drill_round: Some(hole_r * 2),
            drill_slot: None,
        }
    }

    #[test]
    fn crossing_different_net_tracks_is_flagged() {
        let board = empty_board(vec![seg("t1", Some("A"), (0, 0), (1000, 1000)), seg("t2", Some("B"), (0, 1000), (1000, 0))], vec![]);
        let v = check(&board, &BoardRules::default());
        assert!(v.iter().any(|v| v.error_type == ErrorType::TracksCrossing.key()), "{v:#?}");
    }

    #[test]
    fn crossing_same_net_tracks_is_not_flagged() {
        let board = empty_board(vec![seg("t1", Some("A"), (0, 0), (1000, 1000)), seg("t2", Some("A"), (0, 1000), (1000, 0))], vec![]);
        let v = check(&board, &BoardRules::default());
        assert!(!v.iter().any(|v| v.error_type == ErrorType::TracksCrossing.key()), "{v:#?}");
    }

    /// Regression for the `a.net.is_some() && a.net == b.net` bug: a
    /// netless (`None`) track must be treated the same as KiCad treats net
    /// code 0 -- "same net as any other netless item" -- not as "always a
    /// different net from everything, including another netless item".
    #[test]
    fn crossing_two_netless_tracks_is_not_flagged() {
        let board = empty_board(vec![seg("t1", None, (0, 0), (1000, 1000)), seg("t2", None, (0, 1000), (1000, 0))], vec![]);
        let v = check(&board, &BoardRules::default());
        assert!(v.is_empty(), "{v:#?}");
    }

    /// Regression for the missing net-equality guard on pad-vs-pad hole
    /// clearance: two same-net pads with overlapping holes (e.g. two pins
    /// of a plane-tied connector, or stitched vias) must not be flagged --
    /// hole clearance is a foreign-copper check, same as `testPadAgainstItem`.
    #[test]
    fn hole_clearance_same_net_pads_not_flagged() {
        let board = empty_board(vec![], vec![pad_with_hole("P1", Some("GND"), (0, 0), 150), pad_with_hole("P2", Some("GND"), (300, 0), 150)]);
        let v = check(&board, &BoardRules::default());
        assert!(!v.iter().any(|v| v.error_type == ErrorType::HoleClearance.key()), "{v:#?}");
    }

    /// Overlapping different-net pads: `testPadAgainstItem` reports the
    /// short and then sets `testHoles = false` ("No need for multiple
    /// violations") -- one violation for the pair, not a short plus a hole
    /// clearance.
    #[test]
    fn overlapping_different_net_pads_short_without_a_hole_violation() {
        let board = empty_board(vec![], vec![pad_with_hole("P1", Some("GND"), (0, 0), 150), pad_with_hole("P2", Some("VCC"), (300, 0), 150)]);
        let v = check(&board, &BoardRules::default());
        assert_eq!(v.iter().filter(|v| v.error_type == ErrorType::ShortingItems.key()).count(), 1, "{v:#?}");
        assert!(!v.iter().any(|v| v.error_type == ErrorType::HoleClearance.key()), "{v:#?}");
    }

    /// Copper clear of the 0.2 mm clearance but inside the 0.25 mm hole
    /// clearance of the other pad's drill: a `hole_clearance` only.
    #[test]
    fn hole_clearance_different_net_pads_is_flagged() {
        let mut p1 = pad_with_hole("P1", Some("GND"), (0, 0), 90);
        p1.copper = Shape::Circle { c: Point { x: 0, y: 0 }, r: 100 };
        let mut p2 = pad_with_hole("P2", Some("VCC"), (520, 0), 180);
        p2.copper = Shape::Circle { c: Point { x: 520, y: 0 }, r: 200 };
        let board = empty_board(vec![], vec![p1, p2]);
        let v = check(&board, &BoardRules::default());
        assert_eq!(v.iter().map(|v| v.error_type).collect::<Vec<_>>(), vec![ErrorType::HoleClearance.key()], "{v:#?}");
    }

    /// A zero resolved clearance (e.g. a future custom-rule override)
    /// disables the crossing special-case too, not just the normal
    /// clearance/shorting test -- `drc_engine.cpp`'s `EvalRules` gate.
    #[test]
    fn crossing_is_not_flagged_when_resolved_clearance_is_zero() {
        let rules = BoardRules { clearance: 0, ..BoardRules::default() };
        let board = empty_board(vec![seg("t1", Some("A"), (0, 0), (1000, 1000)), seg("t2", Some("B"), (0, 1000), (1000, 0))], vec![]);
        let v = check(&board, &rules);
        assert!(v.is_empty(), "{v:#?}");
    }

    /// Regression for GAPS.md #3's "`hole_clearance` positions disagree
    /// every time" symptom: a real `hole_clearance` violation's `items[0]`
    /// must be the copper-bearing side and `items[1]` the hole-bearing
    /// side, matching KiCad's own `SetItems` order for this exact check
    /// (see `hole_clearance`'s doc comment) -- getting it backwards made
    /// every match attempt compare the wrong item's position against
    /// kicad-cli's JSON.
    #[test]
    fn hole_clearance_item_order_is_copper_then_hole() {
        let p1 = DrcPad {
            id: "P1".into(),
            footprint_ref: "P1".into(),
            number: "1".into(),
            net: Some("GND".into()),
            center: Point { x: 0, y: 0 },
            side: Side::Top,
            kind: PadKind::Smd,
            layers: vec!["F.Cu".into()],
            copper: Shape::Rect { x0: -200, y0: -200, x1: 200, y1: 200 },
            hole: None,
            drill_round: None,
            drill_slot: None,
        };
        let p2 = DrcPad {
            id: "P2".into(),
            footprint_ref: "P2".into(),
            number: "1".into(),
            net: Some("VCC".into()),
            center: Point { x: 500, y: 0 },
            side: Side::Top,
            kind: PadKind::ThroughHole,
            layers: vec!["F.Cu".into()],
            copper: Shape::Circle { c: Point { x: 500, y: 0 }, r: 100 },
            hole: Some(Shape::Circle { c: Point { x: 500, y: 0 }, r: 90 }),
            drill_round: Some(180),
            drill_slot: None,
        };
        let board = empty_board(vec![], vec![p1, p2]);
        let v = check(&board, &BoardRules::default());
        let hit = v.iter().find(|v| v.error_type == ErrorType::HoleClearance.key()).unwrap_or_else(|| panic!("expected a hole_clearance violation: {v:#?}"));
        assert_eq!(hit.items[0].id, "P1", "items[0] must be the copper-bearing side; got {hit:#?}");
        assert_eq!(hit.items[1].id, "P2", "items[1] must be the hole-bearing side; got {hit:#?}");
    }

    fn smd_pad(id: &str, footprint_ref: &str, net: Option<&str>, center: (i64, i64), half: i64) -> DrcPad {
        let c = Point { x: center.0, y: center.1 };
        DrcPad {
            id: id.into(),
            footprint_ref: footprint_ref.into(),
            number: "1".into(),
            net: net.map(String::from),
            center: c,
            side: Side::Top,
            kind: PadKind::Smd,
            layers: vec!["F.Cu".into()],
            copper: Shape::Rect { x0: c.x - half, y0: c.y - half, x1: c.x + half, y1: c.y + half },
            hole: None,
            drill_round: None,
            drill_slot: None,
        }
    }

    /// End-to-end regression for GAPS.md #3's `issue11814` tail (the
    /// "TightWSON" rule): a pad-to-track pair 0.17mm apart, both inside a
    /// tight footprint's courtyard, must clear a `(min 0.15mm)`
    /// `A.insideCourtyard('U4')` rule even though the board's own default
    /// (unconditional) `(min 0.2mm)` `PadToTrack`-style rule would otherwise
    /// flag it -- the courtyard-scoped rule is declared *after* the default
    /// one, so KiCad's (and this port's) last-match-wins precedence must
    /// pick it. Before `CourtyardMembership`/`insideCourtyard` existed, this
    /// condition could never match (an always-`Unknown` function), so the
    /// stricter default rule always won instead.
    #[test]
    fn inside_courtyard_rule_loosens_clearance_for_items_inside_it() {
        let pad = smd_pad("U4.1", "U4", Some("GND"), (0, 0), 600); // 1.2mm square pad, centred at the origin, right edge at x=600
        let track = seg("t1", Some("+5V"), (870, -2000), (870, 2000)); // width 200 (seg()'s default) -> left edge at x=770, a 170um gap from the pad
        let mut board = empty_board(vec![track], vec![pad]);
        board.footprints.push(crate::board::DrcFootprint { id: "U4".into(), side: Side::Top, courtyard: (-2000, -2000, 2000, 2000) });

        let default_rule = BoardRules { clearance: 200, ..BoardRules::default() };
        let v_default = check(&board, &default_rule);
        assert!(v_default.iter().any(|v| v.error_type == "clearance"), "sanity: the 0.2mm default rule alone must flag a 0.17mm gap: {v_default:#?}");

        let mut tight = default_rule;
        tight.custom_rules = vec![eda_model::CustomRule {
            name: "TightWSON".into(),
            constraint_type: "clearance".into(),
            min: Some(150),
            max: None,
            opt: None,
            layer: None,
            severity: None,
            condition: Some("A.insideCourtyard('U4')".into()),
        }];
        let v_tight = check(&board, &tight);
        assert!(!v_tight.iter().any(|v| v.error_type == "clearance"), "the courtyard-scoped 0.15mm rule must win over the 0.2mm default for a pair inside U4's courtyard: {v_tight:#?}");
    }
}
