//! Ported from `pcbnew/drc/drc_test_provider_schematic_parity.cpp`'s
//! reference-designator checks (task item 5 / GAPS.md #19): does every
//! part the schematic calls for have exactly one footprint on the board?
//! KiCad's real provider diffs the board against a `NETLIST` built from the
//! actual `.kicad_sch`; this workspace's analogue of "the schematic's
//! intent" is `ConstraintModel::parts` -- the task's own instruction for
//! this check is explicit about that ("vs our IR's parts"), not a
//! from-scratch KiCad schematic re-import. The other two checks in that
//! same KiCad provider (`DRCE_SCHEMATIC_PARITY`'s field/value diff,
//! `DRCE_FOOTPRINT_FILTERS`) need a real library footprint to diff a
//! placed one against, which is a separate, larger gap (see
//! `drc_test_provider_library_parity.cpp`/GAPS.md #23) -- not ported here.
//!
//! Generated: `DRCE_DUPLICATE_FOOTPRINT`, `DRCE_MISSING_FOOTPRINT`,
//! `DRCE_EXTRA_FOOTPRINT`.

use std::collections::BTreeMap;

use crate::item::{DrcRefItem, DrcViolation, ErrorType};
use eda_model::ir::{Design, Point};
use eda_model::ConstraintModel;

pub fn check(design: &Design, model: &ConstraintModel) -> Vec<DrcViolation> {
    let expected: Vec<&str> = model.parts.iter().map(|p| p.reference.as_str()).collect();
    check_against(design, &expected)
}

/// The actual comparison, taking the expected reference-designator list
/// explicitly rather than always pulling it from `model.parts` -- kept
/// separate from [`check`] so a caller with a more authoritative reference
/// list on hand (e.g. a real re-imported `.kicad_sch`) can use it directly.
pub fn check_against(design: &Design, expected_references: &[&str]) -> Vec<DrcViolation> {
    let mut out = Vec::new();
    let Some(pl) = &design.placement else { return out };

    // ---- duplicate_footprint: same reference placed more than once ----
    // `DRC_TEST_PROVIDER_SCHEMATIC_PARITY::Run`'s own `compare` lambda
    // keys a `std::set<FOOTPRINT*>` by reference, so the *first* occurrence
    // of a reference is never itself a violation -- only the second (and
    // later) `insert` that fails is reported, matching KiCad's own
    // "duplicate of the first one" framing rather than flagging every
    // instance including the original.
    let mut seen: BTreeMap<&str, &eda_model::ir::FootprintInstance> = BTreeMap::new();
    for fp in &pl.footprints {
        if let Some(first) = seen.get(fp.id.as_str()) {
            out.push(DrcViolation::new(
                ErrorType::DuplicateFootprint,
                format!("(duplicate of {})", first.id),
                vec![footprint_ref(fp), footprint_ref(first)],
            ));
        } else {
            seen.insert(&fp.id, fp);
        }
    }

    // ---- missing_footprint: expected (schematic/intent) but not on the board ----
    for reference in expected_references {
        if !pl.footprints.iter().any(|fp| &fp.id == reference) {
            out.push(DrcViolation::new(ErrorType::MissingFootprint, format!("(missing footprint {reference})"), vec![DrcRefItem { description: format!("Missing footprint {reference}"), pos: (0, 0), id: reference.to_string() }]));
        }
    }

    // ---- extra_footprint: on the board but not expected ----
    for fp in &pl.footprints {
        if !expected_references.contains(&fp.id.as_str()) {
            out.push(DrcViolation::new(ErrorType::ExtraFootprint, format!("(extra footprint {})", fp.id), vec![footprint_ref(fp)]));
        }
    }

    out
}

fn footprint_ref(fp: &eda_model::ir::FootprintInstance) -> DrcRefItem {
    DrcRefItem { description: format!("Footprint {}", fp.id), pos: point_tuple(fp.at), id: fp.id.clone() }
}

fn point_tuple(p: Point) -> (eda_model::ir::Um, eda_model::ir::Um) {
    (p.x, p.y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{FootprintInstance, PlacementSection, Point, Provenance, Side};

    fn design_with_footprints(ids: &[&str]) -> Design {
        let footprints = ids.iter().enumerate().map(|(i, id)| FootprintInstance { id: id.to_string(), at: Point { x: i as i64 * 1000, y: 0 }, rot: 0, side: Side::Top, label: Default::default() }).collect();
        Design {
            footprint_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection { outline: vec![], footprints, modules: vec![] }),
            routing: None,
            drawings: None,
        }
    }

    #[test]
    fn exact_match_is_silent() {
        let design = design_with_footprints(&["R1", "C1"]);
        assert!(check_against(&design, &["R1", "C1"]).is_empty());
    }

    #[test]
    fn missing_from_board_is_reported() {
        let design = design_with_footprints(&["R1"]);
        let v = check_against(&design, &["R1", "C1"]);
        assert_eq!(v.len(), 1, "{v:#?}");
        assert_eq!(v[0].error_type, "missing_footprint");
    }

    #[test]
    fn extra_on_board_is_reported() {
        let design = design_with_footprints(&["R1", "C1"]);
        let v = check_against(&design, &["R1"]);
        assert_eq!(v.len(), 1, "{v:#?}");
        assert_eq!(v[0].error_type, "extra_footprint");
        assert_eq!(v[0].items[0].id, "C1");
    }

    #[test]
    fn duplicate_reference_is_reported_once_not_twice() {
        let design = design_with_footprints(&["R1", "R1", "R1"]);
        let v: Vec<_> = check_against(&design, &["R1"]).into_iter().filter(|v| v.error_type == "duplicate_footprints").collect();
        // Three placements of "R1" -> two *extra* duplicates of the first, not three.
        assert_eq!(v.len(), 2, "{v:#?}");
    }
}
