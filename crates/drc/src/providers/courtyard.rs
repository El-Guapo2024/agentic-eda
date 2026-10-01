//! Ported from `pcbnew/drc/drc_test_provider_courtyard_clearance.cpp`.
//!
//! `DRCE_MISSING_COURTYARD`/`DRCE_MALFORMED_COURTYARD` are not ported:
//! `eda_model::Footprint::courtyard_half` always derives a courtyard (the
//! explicit one, or the pad bounding box plus margin), so every footprint
//! in this model has one by construction and neither condition can occur.
//!
//! Generated: `DRCE_OVERLAPPING_FOOTPRINTS`, `DRCE_PTH_IN_COURTYARD`,
//! `DRCE_NPTH_IN_COURTYARD`.

use crate::board::{DrcBoard, DrcFootprint};
use crate::item::{DrcRefItem, DrcViolation, ErrorType};
use eda_model::PadKind;

fn rects_overlap(a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
}

fn rect_contains_point(r: (i64, i64, i64, i64), p: eda_model::ir::Point) -> bool {
    p.x > r.0 && p.x < r.2 && p.y > r.1 && p.y < r.3
}

fn fp_ref(f: &DrcFootprint) -> DrcRefItem {
    DrcRefItem { description: format!("Footprint {}", f.id), pos: ((f.courtyard.0 + f.courtyard.2) / 2, (f.courtyard.1 + f.courtyard.3) / 2), id: f.id.clone() }
}

pub fn check(board: &DrcBoard) -> Vec<DrcViolation> {
    let mut out = Vec::new();

    // Courtyards are rectangles in this model (`placed_courtyard`), so
    // overlap is exact axis-aligned rect/rect intersection -- KiCad's own
    // `COURTYARD_CLEARANCE_CONSTRAINT` resolves to 0 in the absence of a
    // custom rule (see `constraints.rs`'s module doc), so any overlap at
    // all, not just a clearance shortfall, is the violation.
    for i in 0..board.footprints.len() {
        for j in (i + 1)..board.footprints.len() {
            let (a, b) = (&board.footprints[i], &board.footprints[j]);
            if a.side != b.side {
                continue; // front/back courtyards can never physically collide
            }
            if rects_overlap(a.courtyard, b.courtyard) {
                out.push(DrcViolation::new(ErrorType::CourtyardsOverlap, "", vec![fp_ref(a), fp_ref(b)]));
            }
        }
    }

    // A pad's plated/non-plated hole landing inside *another* footprint's
    // courtyard (heatsink pads are excluded -- we have no pad "property"
    // field for that, so it never applies here).
    for p in &board.pads {
        if !matches!(p.kind, PadKind::ThroughHole | PadKind::NonPlatedHole) {
            continue;
        }
        let error_type = if p.kind == PadKind::ThroughHole { ErrorType::PthInsideCourtyard } else { ErrorType::NpthInsideCourtyard };
        for f in &board.footprints {
            if f.id == p.footprint_ref {
                continue;
            }
            // A top-side pad's hole still bores through the whole board, so
            // it is tested against a courtyard on either side -- unlike
            // courtyard/courtyard overlap, KiCad checks both F_CrtYd and
            // B_CrtYd for a pad hole regardless of which side the pad's
            // footprint is on.
            let _ = f.side;
            if rect_contains_point(f.courtyard, p.center) {
                out.push(DrcViolation::new(error_type, "", vec![DrcRefItem { description: format!("Pad {} of {}", p.number, p.footprint_ref), pos: (p.center.x, p.center.y), id: p.id.clone() }, fp_ref(f)]));
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{Point, Side};

    fn fp(id: &str, side: Side, c: (i64, i64, i64, i64)) -> DrcFootprint {
        DrcFootprint { id: id.into(), side, courtyard: c }
    }

    fn empty_board() -> DrcBoard {
        DrcBoard { layers: vec![], outline: vec![], pads: vec![], tracks: vec![], vias: vec![], zones: vec![], keepouts: vec![], footprints: vec![], shapes: vec![], texts: vec![], silk_items: vec![] }
    }

    #[test]
    fn overlapping_same_side_courtyards_fail() {
        let mut b = empty_board();
        b.footprints = vec![fp("U1", Side::Top, (0, 0, 1000, 1000)), fp("U2", Side::Top, (500, 500, 1500, 1500))];
        let v = check(&b);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].error_type, "courtyards_overlap");
    }

    #[test]
    fn opposite_side_courtyards_never_collide() {
        let mut b = empty_board();
        b.footprints = vec![fp("U1", Side::Top, (0, 0, 1000, 1000)), fp("U2", Side::Bottom, (0, 0, 1000, 1000))];
        assert!(check(&b).is_empty());
    }

    #[test]
    fn pth_inside_a_foreign_courtyard_fails() {
        let mut b = empty_board();
        b.footprints = vec![fp("U1", Side::Top, (0, 0, 1000, 1000))];
        b.pads = vec![crate::board::DrcPad {
            id: "U2.1".into(),
            footprint_ref: "U2".into(),
            number: "1".into(),
            net: None,
            center: Point { x: 500, y: 500 },
            side: Side::Top,
            kind: PadKind::ThroughHole,
            layers: vec![],
            copper: crate::kimath::Shape::Circle { c: Point { x: 500, y: 500 }, r: 100 },
            hole: None,
            drill_round: Some(80),
            drill_slot: None,
        }];
        let v = check(&b);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].error_type, "pth_inside_courtyard");
    }
}
