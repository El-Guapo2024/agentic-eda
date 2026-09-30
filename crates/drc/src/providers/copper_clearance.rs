//! Ported from `pcbnew/drc/drc_test_provider_copper_clearance.cpp`: copper
//! clearance between pads, tracks, vias and zones (by outline -- see the
//! crate report for the zone-fill gap) on different nets, per layer.
//!
//! Generated: `DRCE_CLEARANCE`, `DRCE_HOLE_CLEARANCE`, `DRCE_TRACKS_CROSSING`,
//! `DRCE_SHORTING_ITEMS`, `DRCE_ZONES_INTERSECT`.

use crate::board::{DrcBoard, DrcPad, DrcTrackSeg, DrcVia, DrcZone};
use crate::constraints;
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

fn same_logical_pad(a: &DrcPad, b: &DrcPad) -> bool {
    a.footprint_ref == b.footprint_ref && a.number == b.number
}

pub fn check(board: &DrcBoard, rules: &BoardRules) -> Vec<DrcViolation> {
    let mut out = Vec::new();

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

        // ---- item vs zone (by outline), same layer, different net ----
        for z in &zones {
            for p in &pads {
                if p.net.is_some() && p.net == z.net {
                    continue;
                }
                let c = constraints::clearance(rules, p.net.as_deref(), z.net.as_deref());
                if flashed(p) && c > 0 {
                    if let Some((actual, _)) = p.copper.collides(&z.shape(), c) {
                        out.push(DrcViolation::new(ErrorType::Clearance, format!("(clearance {}; actual {})", format_um(c), format_um(actual)), vec![pad_ref(p), zone_ref(z)]));
                    }
                }
                hole_clearance(rules, p.hole.as_ref(), &p.copper, &pad_ref(p), None, &z.shape(), &zone_ref(z), &mut out);
            }
            for t in &tracks {
                if t.net.is_some() && t.net == z.net {
                    continue;
                }
                let c = constraints::clearance(rules, t.net.as_deref(), z.net.as_deref());
                if c > 0 {
                    if let Some((actual, _)) = t.shape().collides(&z.shape(), c) {
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
                    if let Some((actual, _)) = v.shape().collides(&z.shape(), c) {
                        out.push(DrcViolation::new(ErrorType::Clearance, format!("(clearance {}; actual {})", format_um(c), format_um(actual)), vec![via_ref(v), zone_ref(z)]));
                    }
                }
                hole_clearance(rules, Some(&v.hole()), &v.shape(), &via_ref(v), None, &z.shape(), &zone_ref(z), &mut out);
            }
        }

        // ---- zone vs zone, same layer ----
        for i in 0..zones.len() {
            for j in (i + 1)..zones.len() {
                let (a, b) = (zones[i], zones[j]);
                let same_net = a.net.is_some() && a.net == b.net;
                if same_net {
                    if let Some(_actual) = a.shape().collides(&b.shape(), 0) {
                        out.push(DrcViolation::new(ErrorType::ZonesIntersect, "(intersecting zones must have distinct priorities)", vec![zone_ref(a), zone_ref(b)]));
                    }
                } else {
                    let c = constraints::clearance(rules, a.net.as_deref(), b.net.as_deref());
                    if c > 0 {
                        if let Some((actual, _)) = a.shape().collides(&b.shape(), c) {
                            out.push(DrcViolation::new(ErrorType::Clearance, format!("(clearance {}; actual {})", format_um(c), format_um(actual)), vec![zone_ref(a), zone_ref(b)]));
                        }
                    }
                }
            }
        }
    }

    out
}
