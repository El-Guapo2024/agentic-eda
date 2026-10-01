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
use eda_model::{BoardRules, PadKind};

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
/// this model has no data for).
#[allow(clippy::too_many_arguments)]
fn clearance_or_short(rules: &BoardRules, net_a: Option<&str>, net_b: Option<&str>, shape_a: &Shape, shape_b: &Shape, ref_a: DrcRefItem, ref_b: DrcRefItem, out: &mut Vec<DrcViolation>) {
    let clearance = constraints::clearance(rules, net_a, net_b);
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
#[allow(clippy::too_many_arguments)]
fn hole_clearance(rules: &BoardRules, hole_a: Option<&Shape>, copper_a: &Shape, ref_a: &DrcRefItem, hole_b: Option<&Shape>, copper_b: &Shape, ref_b: &DrcRefItem, out: &mut Vec<DrcViolation>) {
    let clearance = constraints::hole_clearance_min(rules);
    if let Some(ha) = hole_a {
        if let Some((actual, _)) = ha.collides(copper_b, clearance.max(0)) {
            out.push(DrcViolation::new(ErrorType::HoleClearance, format!("(clearance {}; actual {})", format_um(clearance), format_um(actual)), vec![ref_a.clone(), ref_b.clone()]));
        }
    }
    if let Some(hb) = hole_b {
        if let Some((actual, _)) = hb.collides(copper_a, clearance.max(0)) {
            out.push(DrcViolation::new(ErrorType::HoleClearance, format!("(clearance {}; actual {})", format_um(clearance), format_um(actual)), vec![ref_b.clone(), ref_a.clone()]));
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
    let fills = crate::fill::fill_all_zones(board, rules);

    for layer in &board.layers {
        let pads: Vec<&DrcPad> = board.pads.iter().filter(|p| p.layers.iter().any(|l| l == layer)).collect();
        let tracks: Vec<&DrcTrackSeg> = board.tracks.iter().filter(|t| &t.layer == layer).collect();
        let vias: Vec<&DrcVia> = board.vias.iter().collect(); // vias flash every layer in our simplified model
        let zones: Vec<&DrcZone> = board.zones.iter().filter(|z| &z.layer == layer).collect();

        // ---- track vs track: crossing, then clearance/shorting/holes ----
        for i in 0..tracks.len() {
            for j in (i + 1)..tracks.len() {
                let (a, b) = (tracks[i], tracks[j]);
                if a.net.is_some() && a.net == b.net {
                    continue;
                }
                let (sa, sb) = (crate::kimath::Seg::new(a.a, a.b), crate::kimath::Seg::new(b.a, b.b));
                if let Some(pt) = sa.intersect(&sb) {
                    out.push(DrcViolation::new(ErrorType::TracksCrossing, "", vec![track_ref(a), ref_item("crossing point".into(), pt, String::new())]));
                    continue;
                }
                clearance_or_short(rules, a.net.as_deref(), b.net.as_deref(), &a.shape(), &b.shape(), track_ref(a), track_ref(b), &mut out);
            }
        }

        // ---- track vs pad / via (holes only apply to vias/pads which may have one) ----
        for t in &tracks {
            for p in &pads {
                if t.net.is_some() && t.net.as_deref() == p.net.as_deref() {
                    continue;
                }
                if flashed(p) {
                    clearance_or_short(rules, t.net.as_deref(), p.net.as_deref(), &t.shape(), &p.copper, track_ref(t), pad_ref(p), &mut out);
                }
                hole_clearance(rules, None, &t.shape(), &track_ref(t), p.hole.as_ref(), &p.copper, &pad_ref(p), &mut out);
            }
            for v in &vias {
                if t.net.is_some() && t.net.as_deref() == v.net.as_deref() {
                    continue;
                }
                clearance_or_short(rules, t.net.as_deref(), v.net.as_deref(), &t.shape(), &v.shape(), track_ref(t), via_ref(v), &mut out);
                hole_clearance(rules, None, &t.shape(), &track_ref(t), Some(&v.hole()), &v.shape(), &via_ref(v), &mut out);
            }
        }

        // ---- pad vs pad, pad vs via, via vs via ----
        for i in 0..pads.len() {
            for j in (i + 1)..pads.len() {
                let (a, b) = (pads[i], pads[j]);
                if same_logical_pad(a, b) {
                    continue;
                }
                if flashed(a) && flashed(b) && !(a.net.is_some() && a.net == b.net) {
                    clearance_or_short(rules, a.net.as_deref(), b.net.as_deref(), &a.copper, &b.copper, pad_ref(a), pad_ref(b), &mut out);
                }
                hole_clearance(rules, a.hole.as_ref(), &a.copper, &pad_ref(a), b.hole.as_ref(), &b.copper, &pad_ref(b), &mut out);
            }
            for v in &vias {
                let a = pads[i];
                if flashed(a) && !(a.net.is_some() && a.net.as_deref() == v.net.as_deref()) {
                    clearance_or_short(rules, a.net.as_deref(), v.net.as_deref(), &a.copper, &v.shape(), pad_ref(a), via_ref(v), &mut out);
                }
                hole_clearance(rules, a.hole.as_ref(), &a.copper, &pad_ref(a), Some(&v.hole()), &v.shape(), &via_ref(v), &mut out);
            }
        }
        for i in 0..vias.len() {
            for j in (i + 1)..vias.len() {
                let (a, b) = (vias[i], vias[j]);
                if a.net.is_some() && a.net == b.net {
                    continue;
                }
                clearance_or_short(rules, a.net.as_deref(), b.net.as_deref(), &a.shape(), &b.shape(), via_ref(a), via_ref(b), &mut out);
                hole_clearance(rules, Some(&a.hole()), &a.shape(), &via_ref(a), Some(&b.hole()), &b.shape(), &via_ref(b), &mut out);
            }
        }

        // ---- item vs zone (by real fill, one test per disjoint fragment), same layer, different net ----
        for z in &zones {
            for p in &pads {
                if p.net.is_some() && p.net == z.net {
                    continue;
                }
                let c = constraints::clearance(rules, p.net.as_deref(), z.net.as_deref());
                if flashed(p) && c > 0 {
                    if let Some((actual, _)) = collides_zone(&p.copper, z, &fills, c) {
                        out.push(DrcViolation::new(ErrorType::Clearance, format!("(clearance {}; actual {})", format_um(c), format_um(actual)), vec![pad_ref(p), zone_ref(z)]));
                    }
                }
                hole_clearance_zone(rules, p.hole.as_ref(), &pad_ref(p), z, &zone_ref(z), &fills, &mut out);
            }
            for t in &tracks {
                if t.net.is_some() && t.net == z.net {
                    continue;
                }
                let c = constraints::clearance(rules, t.net.as_deref(), z.net.as_deref());
                if c > 0 {
                    if let Some((actual, _)) = collides_zone(&t.shape(), z, &fills, c) {
                        out.push(DrcViolation::new(ErrorType::Clearance, format!("(clearance {}; actual {})", format_um(c), format_um(actual)), vec![track_ref(t), zone_ref(z)]));
                    }
                }
            }
            for v in &vias {
                if v.net.is_some() && v.net == z.net {
                    continue;
                }
                let c = constraints::clearance(rules, v.net.as_deref(), z.net.as_deref());
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
                let same_net = a.net.is_some() && a.net == b.net;
                if same_net {
                    if collides_zone_zone(a, b, &fills, 0).is_some() {
                        out.push(DrcViolation::new(ErrorType::ZonesIntersect, "(intersecting zones must have distinct priorities)", vec![zone_ref(a), zone_ref(b)]));
                    }
                } else {
                    let c = constraints::clearance(rules, a.net.as_deref(), b.net.as_deref());
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
