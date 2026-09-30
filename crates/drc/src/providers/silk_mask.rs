//! Ported from `pcbnew/drc/drc_test_provider_silk_clearance.cpp` (silk vs.
//! silk, and -- at KiCad's own factory-default zero solder-mask-min-width,
//! which is what makes `drc_test_provider_silk_clearance.cpp` itself
//! respons­ible for "silk clipped by an exposed pad" rather than
//! `drc_test_provider_solder_mask.cpp`'s whole-board mask polygon -- silk
//! vs. exposed copper) and `drc_test_provider_solder_mask.cpp` (mask
//! bridging between different-net copper).
//!
//! Fidelity notes:
//! - No per-item/footprint solder-mask-expansion field exists in this
//!   model, and KiCad's own `SolderMaskMinWidth`/`SolderMaskToCopper
//!   Clearance` both factory-default to 0 -- so this port takes them as
//!   literally 0 rather than adding board-setting fields nothing else
//!   would ever set. At those (real, shipped) defaults, KiCad's own
//!   bridging test reduces to exactly what's below: different-net exposed
//!   copper touching with no expansion margin. See the report for the case
//!   this misses (a board that has actually raised those settings above 0).
//!
//! Generated: `DRCE_SILK_CLEARANCE`, `DRCE_SILK_MASK_CLEARANCE`,
//! `DRCE_SOLDERMASK_BRIDGE`.

use crate::board::{DrcBoard, SilkItem};
use crate::constraints;
use crate::item::{format_um, DrcRefItem, DrcViolation, ErrorType};
use eda_model::BoardRules;

fn silk_ref(s: &SilkItem) -> DrcRefItem {
    let p = s.shape.boundary_segs()[0].a;
    DrcRefItem { description: s.desc.clone(), pos: (p.x, p.y), id: s.id.clone() }
}

pub fn check(board: &DrcBoard, rules: &BoardRules) -> Vec<DrcViolation> {
    let mut out = Vec::new();
    let silk_clearance = constraints::silk_clearance_min(rules);

    // ---- silk vs silk, same layer: DRCE_SILK_CLEARANCE ("silk_overlap") ----
    for i in 0..board.silk_items.len() {
        for j in (i + 1)..board.silk_items.len() {
            let (a, b) = (&board.silk_items[i], &board.silk_items[j]);
            if a.layer != b.layer {
                continue;
            }
            if let Some((actual, _)) = a.shape.collides(&b.shape, silk_clearance) {
                let detail = if silk_clearance > 0 { format!("(clearance {}; actual {})", format_um(silk_clearance), format_um(actual)) } else { String::new() };
                out.push(DrcViolation::new(ErrorType::SilkOverlap, detail, vec![silk_ref(a), silk_ref(b)]));
            }
        }
    }

    // ---- silk vs exposed copper on the matching side: DRCE_SILK_MASK_CLEARANCE ("silk_over_copper") ----
    for s in &board.silk_items {
        let copper_layer = if s.layer == "F.SilkS" { "F.Cu" } else { "B.Cu" };
        for p in &board.pads {
            if p.kind == eda_model::PadKind::NonPlatedHole || !p.layers.iter().any(|l| l == copper_layer) {
                continue; // an NPTH is a mechanical hole, not exposed copper
            }
            if let Some((actual, _)) = s.shape.collides(&p.copper, silk_clearance.max(0)) {
                let detail = if silk_clearance > 0 { format!("(clearance {}; actual {})", format_um(silk_clearance), format_um(actual)) } else { String::new() };
                let pad_item = DrcRefItem { description: format!("Pad {} [{}] of {}", p.number, p.net.as_deref().unwrap_or("<no net>"), p.footprint_ref), pos: (p.center.x, p.center.y), id: p.id.clone() };
                out.push(DrcViolation::new(ErrorType::SilkOverCopper, detail, vec![silk_ref(s), pad_item]));
            }
        }
        for v in &board.vias {
            if let Some((actual, _)) = s.shape.collides(&v.shape(), silk_clearance.max(0)) {
                let detail = if silk_clearance > 0 { format!("(clearance {}; actual {})", format_um(silk_clearance), format_um(actual)) } else { String::new() };
                let via_item = DrcRefItem { description: format!("Via [{}] on {}-{}", v.net.as_deref().unwrap_or("<no net>"), v.from_layer, v.to_layer), pos: (v.at.x, v.at.y), id: v.id.clone() };
                out.push(DrcViolation::new(ErrorType::SilkOverCopper, detail, vec![silk_ref(s), via_item]));
            }
        }
        // Routed copper on the matching layer -- easy to miss since it postdates
        // placement (which is what keeps a label clear of *pads*): a track can
        // freely route underneath a silk label with nothing stopping it.
        for t in &board.tracks {
            if t.layer != copper_layer {
                continue;
            }
            if let Some((actual, _)) = s.shape.collides(&t.shape(), silk_clearance.max(0)) {
                let detail = if silk_clearance > 0 { format!("(clearance {}; actual {})", format_um(silk_clearance), format_um(actual)) } else { String::new() };
                let track_item = DrcRefItem { description: format!("Track [{}] on {}", t.net.as_deref().unwrap_or("<no net>"), t.layer), pos: (t.a.x, t.a.y), id: t.id.clone() };
                out.push(DrcViolation::new(ErrorType::SilkOverCopper, detail, vec![silk_ref(s), track_item]));
            }
        }
    }

    // ---- solder mask bridging: different-net exposed copper touching, at
    // KiCad's own zero-web-width/zero-mask-clearance defaults (see module doc) ----
    for layer in ["F.Cu", "B.Cu"] {
        // An NPTH has no copper to bridge through (mechanical hole only).
        let pads: Vec<_> = board.pads.iter().filter(|p| p.kind != eda_model::PadKind::NonPlatedHole && p.layers.iter().any(|l| l == layer)).collect();
        let tracks: Vec<_> = board.tracks.iter().filter(|t| t.layer == layer).collect();
        let vias: Vec<_> = board.vias.iter().collect();

        let bridge = |net_a: Option<&str>, net_b: Option<&str>, shape_a: &crate::kimath::Shape, shape_b: &crate::kimath::Shape, ref_a: DrcRefItem, ref_b: DrcRefItem, out: &mut Vec<DrcViolation>| {
            let (Some(na), Some(nb)) = (net_a, net_b) else { return };
            if na == nb {
                return;
            }
            if shape_a.collides(shape_b, 0).is_some() {
                out.push(DrcViolation::new(ErrorType::SolderMaskBridge, "", vec![ref_a, ref_b]));
            }
        };

        for i in 0..pads.len() {
            for j in (i + 1)..pads.len() {
                let (a, b) = (pads[i], pads[j]);
                if a.footprint_ref == b.footprint_ref && a.number == b.number {
                    continue;
                }
                let (ra, rb) = (DrcRefItem { description: format!("Pad {} of {}", a.number, a.footprint_ref), pos: (a.center.x, a.center.y), id: a.id.clone() }, DrcRefItem { description: format!("Pad {} of {}", b.number, b.footprint_ref), pos: (b.center.x, b.center.y), id: b.id.clone() });
                bridge(a.net.as_deref(), b.net.as_deref(), &a.copper, &b.copper, ra, rb, &mut out);
            }
            for v in &vias {
                let a = pads[i];
                let (ra, rb) = (DrcRefItem { description: format!("Pad {} of {}", a.number, a.footprint_ref), pos: (a.center.x, a.center.y), id: a.id.clone() }, DrcRefItem { description: format!("Via on {}-{}", v.from_layer, v.to_layer), pos: (v.at.x, v.at.y), id: v.id.clone() });
                bridge(a.net.as_deref(), v.net.as_deref(), &a.copper, &v.shape(), ra, rb, &mut out);
            }
            for t in &tracks {
                let a = pads[i];
                let (ra, rb) = (DrcRefItem { description: format!("Pad {} of {}", a.number, a.footprint_ref), pos: (a.center.x, a.center.y), id: a.id.clone() }, DrcRefItem { description: format!("Track on {}", t.layer), pos: (t.a.x, t.a.y), id: t.id.clone() });
                bridge(a.net.as_deref(), t.net.as_deref(), &a.copper, &t.shape(), ra, rb, &mut out);
            }
        }
        for i in 0..tracks.len() {
            for j in (i + 1)..tracks.len() {
                let (a, b) = (tracks[i], tracks[j]);
                let (ra, rb) = (DrcRefItem { description: format!("Track on {}", a.layer), pos: (a.a.x, a.a.y), id: a.id.clone() }, DrcRefItem { description: format!("Track on {}", b.layer), pos: (b.a.x, b.a.y), id: b.id.clone() });
                bridge(a.net.as_deref(), b.net.as_deref(), &a.shape(), &b.shape(), ra, rb, &mut out);
            }
            for v in &vias {
                let a = tracks[i];
                let (ra, rb) = (DrcRefItem { description: format!("Track on {}", a.layer), pos: (a.a.x, a.a.y), id: a.id.clone() }, DrcRefItem { description: format!("Via on {}-{}", v.from_layer, v.to_layer), pos: (v.at.x, v.at.y), id: v.id.clone() });
                bridge(a.net.as_deref(), v.net.as_deref(), &a.shape(), &v.shape(), ra, rb, &mut out);
            }
        }
        for i in 0..vias.len() {
            for j in (i + 1)..vias.len() {
                let (a, b) = (vias[i], vias[j]);
                let (ra, rb) = (DrcRefItem { description: format!("Via on {}-{}", a.from_layer, a.to_layer), pos: (a.at.x, a.at.y), id: a.id.clone() }, DrcRefItem { description: format!("Via on {}-{}", b.from_layer, b.to_layer), pos: (b.at.x, b.at.y), id: b.id.clone() });
                bridge(a.net.as_deref(), b.net.as_deref(), &a.shape(), &b.shape(), ra, rb, &mut out);
            }
        }
    }

    out
}
