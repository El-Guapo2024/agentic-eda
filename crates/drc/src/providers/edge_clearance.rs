//! Ported from `pcbnew/drc/drc_test_provider_edge_clearance.cpp`: copper
//! and silkscreen clearance to the board edge (`Edge.Cuts`). The
//! "silkscreen crosses the edge outright" heuristic
//! (`resolveSilkDisposition`) is not ported -- see the crate report; the
//! direct clearance test below already catches a silk item that touches or
//! crosses an edge segment, since a crossing collides at `actual == 0`.
//!
//! Generated: `DRCE_EDGE_CLEARANCE`, `DRCE_SILK_EDGE_CLEARANCE`.

use crate::board::DrcBoard;
use crate::constraints;
use crate::item::{format_um, DrcRefItem, DrcViolation, ErrorType};
use crate::kimath::{distance_to_open_segments, Seg};
use eda_model::BoardRules;

fn edge_segments(board: &DrcBoard) -> Vec<Seg> {
    let n = board.outline.len();
    if n < 2 {
        return Vec::new();
    }
    (0..n).map(|i| Seg::new(board.outline[i], board.outline[(i + 1) % n])).collect()
}

/// KiCad's `SHAPE::Collide` convention (see `kimath::Shape::collides`):
/// touching/crossing is *always* a violation, even at a configured
/// clearance of exactly 0 (KiCad's own factory default for silk). A bare
/// `actual < clearance` would silently never fire at `clearance == 0`,
/// since `actual` is itself clamped to a 0 minimum.
fn violates(actual: eda_model::ir::Um, clearance: eda_model::ir::Um) -> bool {
    actual == 0 || actual < clearance
}

pub fn check(board: &DrcBoard, rules: &BoardRules) -> Vec<DrcViolation> {
    let mut out = Vec::new();
    let edges = edge_segments(board);
    if edges.is_empty() {
        return out;
    }

    let copper_clearance = constraints::edge_clearance_min(rules);
    for p in &board.pads {
        let (actual, _) = distance_to_open_segments(&p.copper, &edges);
        if violates(actual, copper_clearance) {
            let item = DrcRefItem { description: format!("Pad {} [{}] of {}", p.number, p.net.as_deref().unwrap_or("<no net>"), p.footprint_ref), pos: (p.center.x, p.center.y), id: p.id.clone() };
            out.push(DrcViolation::new(ErrorType::CopperEdgeClearance, format!("(clearance {}; actual {})", format_um(copper_clearance), format_um(actual)), vec![item]));
        }
    }
    for t in &board.tracks {
        let (actual, _) = distance_to_open_segments(&t.shape(), &edges);
        if violates(actual, copper_clearance) {
            let item = DrcRefItem { description: format!("Track [{}] on {}", t.net.as_deref().unwrap_or("<no net>"), t.layer), pos: (t.a.x, t.a.y), id: t.id.clone() };
            out.push(DrcViolation::new(ErrorType::CopperEdgeClearance, format!("(clearance {}; actual {})", format_um(copper_clearance), format_um(actual)), vec![item]));
        }
    }
    for v in &board.vias {
        let (actual, _) = distance_to_open_segments(&v.shape(), &edges);
        if violates(actual, copper_clearance) {
            let item = DrcRefItem { description: format!("Via [{}] on {}-{}", v.net.as_deref().unwrap_or("<no net>"), v.from_layer, v.to_layer), pos: (v.at.x, v.at.y), id: v.id.clone() };
            out.push(DrcViolation::new(ErrorType::CopperEdgeClearance, format!("(clearance {}; actual {})", format_um(copper_clearance), format_um(actual)), vec![item]));
        }
    }

    let silk_clearance = constraints::silk_clearance_min(rules);
    for s in &board.silk_items {
        let (actual, _) = distance_to_open_segments(&s.shape, &edges);
        if violates(actual, silk_clearance) {
            // Each item's own anchor, not the computed collision point --
            // matches kicad-cli's actual reported position for every other
            // item kind here (see `item.rs`'s doc comment on `DrcRefItem`).
            let anchor = s.shape.boundary_segs()[0].a;
            let item = DrcRefItem { description: s.desc.clone(), pos: (anchor.x, anchor.y), id: s.id.clone() };
            let detail = if silk_clearance > 0 { format!("(clearance {}; actual {})", format_um(silk_clearance), format_um(actual)) } else { String::new() };
            out.push(DrcViolation::new(ErrorType::SilkEdgeClearance, detail, vec![item]));
        }
    }

    out
}
