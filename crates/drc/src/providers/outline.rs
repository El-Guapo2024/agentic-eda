//! Ported from the spirit of `pcbnew/drc/drc_test_provider_misc.cpp`'s
//! `DRCE_INVALID_OUTLINE` (task item 5, by measured count: 12 missing in
//! the parity sample, 0 ported). KiCad's real check runs
//! `TestBoardOutlinesGraphicItems` over the board's raw Edge.Cuts
//! primitives and reports both open-gap and self-intersection failures,
//! plus a separate "suspicious tiny item" sweep -- this model keeps no raw
//! per-primitive Edge.Cuts data once `eda_kicad::import_kicad_pcb` chains
//! it into one outline polyline, so a byte-for-byte port is not possible
//! from this IR. What *is* ported, faithfully rather than approximated: a
//! real open-gap failure the importer's own chaining already detected
//! (`BoardRules::outline_closed`, carried from `ImportNotes::outline_open`)
//! is surfaced here as a real `DrcViolation` instead of silently staying an
//! import note nobody using `eda_drc::run` directly would ever see.
//!
//! Self-intersection and "suspicious tiny item" are not ported: this
//! model's outline is already reduced to a plain point list by the time
//! `eda_drc::run` sees it, with no per-primitive provenance left to
//! classify a failure the way KiCad's own error messages do.

use crate::item::{DrcRefItem, DrcViolation, ErrorType};
use eda_model::ir::{Design, Point};
use eda_model::ConstraintModel;

pub fn check(design: &Design, model: &ConstraintModel) -> Vec<DrcViolation> {
    let mut out = Vec::new();
    if model.board.outline_closed != Some(false) {
        return out; // closed, or nothing on record either way -- see BoardRules::outline_closed's doc comment
    }
    let Some(pl) = &design.placement else { return out };
    let (Some(&first), Some(&last)) = (pl.outline.first(), pl.outline.last()) else { return out };

    // The gap's midpoint -- the same "nearest-gap" convention
    // `findClosestOutlineGap` reports a marker position at, simplified to
    // the one gap this model can even represent (the chain's own
    // start/end, not a search over every candidate pair of dangling ends).
    let mid = Point { x: (first.x + last.x) / 2, y: (first.y + last.y) / 2 };
    out.push(DrcViolation::new(
        ErrorType::InvalidOutline,
        "(board outline graphics do not form a closed polygon)",
        vec![DrcRefItem { description: "Board outline".into(), pos: (mid.x, mid.y), id: String::new() }],
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{PlacementSection, Provenance};

    fn design_with_outline(pts: Vec<Point>) -> Design {
        Design {
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![], symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection { outline: pts, footprints: vec![], modules: vec![] }),
            routing: None,
            drawings: None,
        }
    }

    #[test]
    fn an_open_chain_is_reported() {
        let design = design_with_outline(vec![Point { x: 0, y: 0 }, Point { x: 10_000, y: 0 }, Point { x: 10_000, y: 5_000 }]);
        let model = ConstraintModel { board: eda_model::BoardRules { outline_closed: Some(false), ..Default::default() }, ..Default::default() };
        let v = check(&design, &model);
        assert_eq!(v.len(), 1, "{v:#?}");
        assert_eq!(v[0].error_type, "invalid_outline");
    }

    #[test]
    fn a_board_with_no_tracked_closure_fact_is_not_flagged() {
        // This workspace's own pipeline never sets `outline_closed` at
        // all -- the common case, and it must never false-positive here.
        let design = design_with_outline(vec![Point { x: 0, y: 0 }, Point { x: 10_000, y: 0 }, Point { x: 10_000, y: 10_000 }, Point { x: 0, y: 10_000 }]);
        let model = ConstraintModel::default();
        assert!(check(&design, &model).is_empty());
    }

    #[test]
    fn explicitly_closed_is_not_flagged() {
        let design = design_with_outline(vec![Point { x: 0, y: 0 }, Point { x: 10_000, y: 0 }, Point { x: 10_000, y: 10_000 }, Point { x: 0, y: 10_000 }]);
        let model = ConstraintModel { board: eda_model::BoardRules { outline_closed: Some(true), ..Default::default() }, ..Default::default() };
        assert!(check(&design, &model).is_empty());
    }
}
