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

/// This item's resolved net-class name, for a `.kicad_dru` condition's
/// `A.NetClass`/`B.NetClass`/`hasNetclass()` (task item 4) -- `"Default"`
/// for an unclassed net, matching KiCad's own convention that every net
/// belongs to at least the implicit default class.
fn net_class_name<'a>(rules: &'a BoardRules, net: Option<&str>) -> &'a str {
    net.and_then(|n| rules.class_of(n)).map(|c| c.name.as_str()).unwrap_or("Default")
}

fn facts_of_pad<'a>(rules: &'a BoardRules, p: &'a DrcPad) -> Facts<'a> {
    Facts { item_type: "Pad", net_class: net_class_name(rules, p.net.as_deref()), net_name: p.net.as_deref().unwrap_or(""), reference: &p.footprint_ref }
}
fn facts_of_track<'a>(rules: &'a BoardRules, t: &'a DrcTrackSeg) -> Facts<'a> {
    Facts { item_type: "Track", net_class: net_class_name(rules, t.net.as_deref()), net_name: t.net.as_deref().unwrap_or(""), reference: "" }
}
fn facts_of_via<'a>(rules: &'a BoardRules, v: &'a DrcVia) -> Facts<'a> {
    Facts { item_type: "Via", net_class: net_class_name(rules, v.net.as_deref()), net_name: v.net.as_deref().unwrap_or(""), reference: "" }
}
fn facts_of_zone<'a>(rules: &'a BoardRules, z: &'a DrcZone) -> Facts<'a> {
    Facts { item_type: "Zone", net_class: net_class_name(rules, z.net.as_deref()), net_name: z.net.as_deref().unwrap_or(""), reference: "" }
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

/// One collision result between two copper items already known to be on
/// different nets (or one/both netless): clearance/shorting, exactly
/// `testSingleLayerItemAgainstItem`'s core (minus net-tie exclusions, which
/// this model has no data for). `layer`/`facts_a`/`facts_b` feed task item
/// 4's custom-rule resolution (`constraints::clearance_with_custom_rules`);
/// a board with no `.kicad_dru` rules is unaffected (same plain
/// `constraints::clearance` result).
#[allow(clippy::too_many_arguments)]
fn clearance_or_short(rules: &BoardRules, net_a: Option<&str>, net_b: Option<&str>, layer: &str, facts_a: &Facts, facts_b: &Facts, shape_a: &Shape, shape_b: &Shape, ref_a: DrcRefItem, ref_b: DrcRefItem, out: &mut Vec<DrcViolation>) {
    let clearance = constraints::clearance_with_custom_rules(rules, net_a, net_b, layer, facts_a, facts_b);
    if clearance <= 0 {
        return; // KiCad's own gate: a resolved 0 clearance is never checked.
    }
    if let Some((actual, _pos)) = shape_a.collides(shape_b, clearance) {
        if let (true, Some(na), Some(nb)) = (actual == 0, net_a, net_b) {
            out.push(DrcViolation::new(ErrorType::ShortingItems, format!("(nets {na} and {nb})"), vec![ref_a, ref_b]));
        } else {
            out.push(DrcViolation::new(ErrorType::Clearance, format!("(clearance {}; actual {})", format_um(clearance), format_um(actual)), vec![ref_a, ref_b]));
        }
    }
}

/// Hole-clearance both ways (each side's hole against the other's copper),
/// regardless of net -- `testSingleLayerItemAgainstItem`'s hole loop.
///
/// Item order in the pushed violation matters for more than cosmetics: the
/// parity harness's position match (`crates/drc/tests/parity_drc.rs`) reads
/// `items[0]`'s position only, same as kicad-cli's own JSON. KiCad's
/// `SetItems(a[ii], b[ii])` for this exact loop
/// (`drc_test_provider_copper_clearance.cpp`'s `testSingleLayerItemAgainstItem`,
/// where `a[ii]` carries the copper shape under test and `b[ii]` carries
/// the hole) always puts the **copper**-bearing side first and the
/// **hole**-bearing side second -- confirmed against three separate call
/// sites in that file (`testSingleLayerItemAgainstItem`, `testPadAgainstItem`'s
/// `doTestHole`), which is the opposite order from its *zone* counterpart,
/// `testItemAgainstZone` (`SetItems(aItem, aZone)` with `aItem` the hole
/// owner) -- see [`hole_clearance_zone`], which already had this right.
/// Getting this backwards here previously meant our `items[0]` was always
/// the hole owner's own anchor while kicad-cli's was always the copper
/// owner's -- two different real-world positions that essentially never
/// land within the match tolerance of each other, which is exactly
/// GAPS.md #3's "`hole_clearance` ... the *positions* disagree every
/// time" symptom (0 matched despite hundreds of raw hits on both sides).
#[allow(clippy::too_many_arguments)]
fn hole_clearance(rules: &BoardRules, hole_a: Option<&Shape>, copper_a: &Shape, ref_a: &DrcRefItem, hole_b: Option<&Shape>, copper_b: &Shape, ref_b: &DrcRefItem, out: &mut Vec<DrcViolation>) {
    let clearance = constraints::hole_clearance_min(rules);
    if let Some(ha) = hole_a {
        // a's hole vs b's copper: b is the copper side here, a is the hole side.
        if let Some((actual, _)) = ha.collides(copper_b, clearance.max(0)) {
            out.push(DrcViolation::new(ErrorType::HoleClearance, format!("(clearance {}; actual {})", format_um(clearance), format_um(actual)), vec![ref_b.clone(), ref_a.clone()]));
        }
    }
    if let Some(hb) = hole_b {
        // b's hole vs a's copper: a is the copper side here, b is the hole side.
        if let Some((actual, _)) = hb.collides(copper_a, clearance.max(0)) {
            out.push(DrcViolation::new(ErrorType::HoleClearance, format!("(clearance {}; actual {})", format_um(clearance), format_um(actual)), vec![ref_a.clone(), ref_b.clone()]));
        }
    }
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

/// `hole_clearance`'s zone-side special case: the zone side never has a
/// hole of its own, so only "does `hole`, if any, come too close to the
/// zone's copper" applies -- against every fragment, not just the outline.
fn hole_clearance_zone(rules: &BoardRules, hole: Option<&Shape>, hole_ref: &DrcRefItem, zone: &DrcZone, zone_ref: &DrcRefItem, fills: &FillResults, out: &mut Vec<DrcViolation>) {
    let clearance = constraints::hole_clearance_min(rules).max(0);
    if let Some(h) = hole {
        if let Some((actual, _)) = collides_zone(h, zone, fills, clearance) {
            out.push(DrcViolation::new(ErrorType::HoleClearance, format!("(clearance {}; actual {})", format_um(clearance), format_um(actual)), vec![hole_ref.clone(), zone_ref.clone()]));
        }
    }
}

fn same_logical_pad(a: &DrcPad, b: &DrcPad) -> bool {
    a.footprint_ref == b.footprint_ref && a.number == b.number
}

pub fn check(board: &DrcBoard, rules: &BoardRules) -> Vec<DrcViolation> {
    let mut out = Vec::new();
    // `DRC_RTREE` queries are bounded by one run-wide worst-case clearance
    // (`BOARD::m_DRCMaxClearance`, see `crate::constraints::
    // worst_case_clearance`'s doc comment) -- the exact-pair clearance is
    // still resolved per-pair below, same as it always was; this only
    // bounds which pairs get tested at all.
    let worst_clearance = constraints::worst_case_clearance(rules);
    let fills = crate::fill::fill_all_zones(board, rules);

    for layer in &board.layers {
        let pads: Vec<&DrcPad> = board.pads.iter().filter(|p| p.layers.iter().any(|l| l == layer)).collect();
        let tracks: Vec<&DrcTrackSeg> = board.tracks.iter().filter(|t| &t.layer == layer).collect();
        let vias: Vec<&DrcVia> = board.vias.iter().collect(); // vias flash every layer in our simplified model
        let zones: Vec<&DrcZone> = board.zones.iter().filter(|z| &z.layer == layer).collect();

        // ---- track vs track: crossing, then clearance/shorting/holes ----
        // KiCad structure, not an all-pairs scan: `testTrackClearances`
        // (`drc_test_provider_copper_clearance.cpp`) queries a `DRC_RTREE`
        // bounded by the run's worst-case clearance instead of testing
        // every other track; [`crate::rtree::DrcRTree`] is this crate's
        // port of that same structure. Tracks are the item that scales with
        // routing density (thousands of segments on a real board), so this
        // is where an all-pairs scan actually hurt -- pads/vias below scale
        // with part count instead and stay a plain nested loop.
        let mut track_idx = crate::rtree::DrcRTree::new(worst_clearance.max(1));
        for (i, t) in tracks.iter().enumerate() {
            track_idx.insert(i, t.shape().bbox(0));
        }
        for i in 0..tracks.len() {
            let a = tracks[i];
            for j in track_idx.query(a.shape().bbox(worst_clearance)) {
                if j <= i {
                    continue; // each unordered pair tested once (bbox overlap is symmetric -- see DrcRTree's tests), matching KiCad's pointer-order dedup
                }
                let b = tracks[j];
                if a.net == b.net {
                    continue; // netcode equality, not "both assigned": two netless items share net 0 in KiCad too
                }
                // KiCad's own gate (`drc_engine.cpp`'s `EvalRules` call site
                // in `testSingleLayerItemAgainstItem`): the crossing
                // special-case below -- and the normal clearance/shorting
                // test it falls through to -- only run when the resolved
                // clearance is actually positive; a rule that sets it to
                // exactly 0 between two nets disables both, not just one.
                let pair_clearance = constraints::clearance_with_custom_rules(rules, a.net.as_deref(), b.net.as_deref(), layer, &facts_of_track(rules, a), &facts_of_track(rules, b));
                if pair_clearance > 0 {
                    let (sa, sb) = (crate::kimath::Seg::new(a.a, a.b), crate::kimath::Seg::new(b.a, b.b));
                    if let Some(pt) = sa.intersect(&sb) {
                        out.push(DrcViolation::new(ErrorType::TracksCrossing, "", vec![track_ref(a), ref_item("crossing point".into(), pt, String::new())]));
                        continue;
                    }
                }
                clearance_or_short(rules, a.net.as_deref(), b.net.as_deref(), layer, &facts_of_track(rules, a), &facts_of_track(rules, b), &a.shape(), &b.shape(), track_ref(a), track_ref(b), &mut out);
            }
        }

        // ---- track vs pad / via (holes only apply to vias/pads which may have one) ----
        let mut pad_idx = crate::rtree::DrcRTree::new(worst_clearance.max(1));
        for (i, p) in pads.iter().enumerate() {
            pad_idx.insert(i, p.copper.bbox(0));
        }
        let mut via_idx = crate::rtree::DrcRTree::new(worst_clearance.max(1));
        for (i, v) in vias.iter().enumerate() {
            via_idx.insert(i, v.shape().bbox(0));
        }
        for t in &tracks {
            let query_box = t.shape().bbox(worst_clearance);
            for pi in pad_idx.query(query_box) {
                let p = pads[pi];
                if t.net.is_some() && t.net.as_deref() == p.net.as_deref() {
                    continue;
                }
                if flashed(p) {
                    clearance_or_short(rules, t.net.as_deref(), p.net.as_deref(), layer, &facts_of_track(rules, t), &facts_of_pad(rules, p), &t.shape(), &p.copper, track_ref(t), pad_ref(p), &mut out);
                }
                hole_clearance(rules, None, &t.shape(), &track_ref(t), p.hole.as_ref(), &p.copper, &pad_ref(p), &mut out);
            }
            for vi in via_idx.query(query_box) {
                let v = vias[vi];
                if t.net.is_some() && t.net.as_deref() == v.net.as_deref() {
                    continue;
                }
                clearance_or_short(rules, t.net.as_deref(), v.net.as_deref(), layer, &facts_of_track(rules, t), &facts_of_via(rules, v), &t.shape(), &v.shape(), track_ref(t), via_ref(v), &mut out);
                hole_clearance(rules, None, &t.shape(), &track_ref(t), Some(&v.hole()), &v.shape(), &via_ref(v), &mut out);
            }
        }

        // ---- pad vs pad, pad vs via, via vs via ----
        // Pad/via counts scale with part count, not routing density, so a
        // plain nested loop stays fast at real-board scale; the track loops
        // above are where the DRC_RTREE treatment actually earns its keep.
        for i in 0..pads.len() {
            for j in (i + 1)..pads.len() {
                let (a, b) = (pads[i], pads[j]);
                if same_logical_pad(a, b) {
                    continue;
                }
                let same_net = a.net == b.net; // netcode equality, not "both assigned" -- see the track loop above
                if flashed(a) && flashed(b) && !same_net {
                    clearance_or_short(rules, a.net.as_deref(), b.net.as_deref(), layer, &facts_of_pad(rules, a), &facts_of_pad(rules, b), &a.copper, &b.copper, pad_ref(a), pad_ref(b), &mut out);
                }
                // Hole clearance is a *foreign-copper* check (see this
                // module's doc comment on `flashed`): same-net pads (two
                // pins of a plane-tied connector, stitched vias sharing a
                // hole) are never checked against each other's hole --
                // `testPadAgainstItem`'s `if (otherNet && otherNet ==
                // padNet) testHoles = false` (`drc_test_provider_copper_
                // clearance.cpp`). The track/zone loops already get this
                // for free from their shared same-net `continue`; pad-pad
                // and pad-via need it spelled out since `clearance_or_short`
                // and `hole_clearance` are two separate calls here.
                if !same_net {
                    hole_clearance(rules, a.hole.as_ref(), &a.copper, &pad_ref(a), b.hole.as_ref(), &b.copper, &pad_ref(b), &mut out);
                }
            }
            for v in &vias {
                let a = pads[i];
                let same_net = a.net.as_deref() == v.net.as_deref();
                if flashed(a) && !same_net {
                    clearance_or_short(rules, a.net.as_deref(), v.net.as_deref(), layer, &facts_of_pad(rules, a), &facts_of_via(rules, v), &a.copper, &v.shape(), pad_ref(a), via_ref(v), &mut out);
                }
                if !same_net {
                    hole_clearance(rules, a.hole.as_ref(), &a.copper, &pad_ref(a), Some(&v.hole()), &v.shape(), &via_ref(v), &mut out);
                }
            }
        }
        for i in 0..vias.len() {
            for j in (i + 1)..vias.len() {
                let (a, b) = (vias[i], vias[j]);
                if a.net == b.net {
                    continue;
                }
                clearance_or_short(rules, a.net.as_deref(), b.net.as_deref(), layer, &facts_of_via(rules, a), &facts_of_via(rules, b), &a.shape(), &b.shape(), via_ref(a), via_ref(b), &mut out);
                hole_clearance(rules, Some(&a.hole()), &a.shape(), &via_ref(a), Some(&b.hole()), &b.shape(), &via_ref(b), &mut out);
            }
        }

        // ---- item vs zone (by real fill, one test per disjoint fragment), same layer, different net ----
        for z in &zones {
            for p in &pads {
                if p.net == z.net {
                    continue;
                }
                let c = constraints::clearance_with_custom_rules(rules, p.net.as_deref(), z.net.as_deref(), layer, &facts_of_pad(rules, p), &facts_of_zone(rules, z));
                if flashed(p) && c > 0 {
                    if let Some((actual, _)) = collides_zone(&p.copper, z, &fills, c) {
                        out.push(DrcViolation::new(ErrorType::Clearance, format!("(clearance {}; actual {})", format_um(c), format_um(actual)), vec![pad_ref(p), zone_ref(z)]));
                    }
                }
                hole_clearance_zone(rules, p.hole.as_ref(), &pad_ref(p), z, &zone_ref(z), &fills, &mut out);
            }
            for t in &tracks {
                if t.net == z.net {
                    continue;
                }
                let c = constraints::clearance_with_custom_rules(rules, t.net.as_deref(), z.net.as_deref(), layer, &facts_of_track(rules, t), &facts_of_zone(rules, z));
                if c > 0 {
                    if let Some((actual, _)) = collides_zone(&t.shape(), z, &fills, c) {
                        out.push(DrcViolation::new(ErrorType::Clearance, format!("(clearance {}; actual {})", format_um(c), format_um(actual)), vec![track_ref(t), zone_ref(z)]));
                    }
                }
            }
            for v in &vias {
                if v.net == z.net {
                    continue;
                }
                let c = constraints::clearance_with_custom_rules(rules, v.net.as_deref(), z.net.as_deref(), layer, &facts_of_via(rules, v), &facts_of_zone(rules, z));
                if c > 0 {
                    if let Some((actual, _)) = collides_zone(&v.shape(), z, &fills, c) {
                        out.push(DrcViolation::new(ErrorType::Clearance, format!("(clearance {}; actual {})", format_um(c), format_um(actual)), vec![via_ref(v), zone_ref(z)]));
                    }
                }
                hole_clearance_zone(rules, Some(&v.hole()), &via_ref(v), z, &zone_ref(z), &fills, &mut out);
            }
        }

        // ---- zone vs zone, same layer (by real fill) ----
        for i in 0..zones.len() {
            for j in (i + 1)..zones.len() {
                let (a, b) = (zones[i], zones[j]);
                let same_net = a.net == b.net;
                if same_net {
                    if collides_zone_zone(a, b, &fills, 0).is_some() {
                        out.push(DrcViolation::new(ErrorType::ZonesIntersect, "(intersecting zones must have distinct priorities)", vec![zone_ref(a), zone_ref(b)]));
                    }
                } else {
                    let c = constraints::clearance_with_custom_rules(rules, a.net.as_deref(), b.net.as_deref(), layer, &facts_of_zone(rules, a), &facts_of_zone(rules, b));
                    if c > 0 {
                        if let Some((actual, _)) = collides_zone_zone(a, b, &fills, c) {
                            out.push(DrcViolation::new(ErrorType::Clearance, format!("(clearance {}; actual {})", format_um(c), format_um(actual)), vec![zone_ref(a), zone_ref(b)]));
                        }
                    }
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
        DrcBoard { layers: vec!["F.Cu".into()], outline: vec![], pads, tracks, vias: vec![], zones: vec![], footprints: vec![], shapes: vec![], texts: vec![], silk_items: vec![] }
    }

    fn seg(id: &str, net: Option<&str>, a: (i64, i64), b: (i64, i64)) -> DrcTrackSeg {
        DrcTrackSeg { id: id.into(), net: net.map(String::from), layer: "F.Cu".into(), width: 200, a: Point { x: a.0, y: a.1 }, b: Point { x: b.0, y: b.1 } }
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

    #[test]
    fn hole_clearance_different_net_pads_is_flagged() {
        let board = empty_board(vec![], vec![pad_with_hole("P1", Some("GND"), (0, 0), 150), pad_with_hole("P2", Some("VCC"), (300, 0), 150)]);
        let v = check(&board, &BoardRules::default());
        assert!(v.iter().any(|v| v.error_type == ErrorType::HoleClearance.key()), "{v:#?}");
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
}
