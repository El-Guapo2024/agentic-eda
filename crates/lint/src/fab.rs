//! Fab readiness: the defects a board house finds after you have paid, none
//! of them geometry, so neither DRC nor ERC can see them.
//!
//! A placement with no part number is a line item the assembler cannot
//! source, and a part number on nothing is a reel that arrives for a board
//! with nowhere to put it. Fails, it does not warn -- an unsourceable
//! placement is not a note on an otherwise good order.

use eda_model::ir::Design;
use eda_model::{CheckResult, ConstraintModel};

/// Gate the package before it is offered as one.
pub fn check(design: &Design, model: &ConstraintModel) -> Vec<CheckResult> {
    let mut out = Vec::new();
    let Some(pl) = &design.placement else {
        return vec![CheckResult::fail("fab_placement", "placement", "no placement: nothing to fabricate")];
    };
    for f in &pl.footprints {
        match model.part(&f.id) {
            None => out.push(CheckResult::fail(
                "fab_unknown_part",
                f.id.clone(),
                format!("{} is placed on the board but is not in the netlist; the assembler has a position and no part", f.id),
            )),
            Some(p) => {
                let has_mpn = p.mpn.as_deref().is_some_and(|m| !m.trim().is_empty());
                if !has_mpn {
                    out.push(CheckResult::fail(
                        "fab_no_mpn",
                        f.id.clone(),
                        format!(
                            "{} has no manufacturer part number, so nothing says what to solder there. \
                             A value alone ({}) names a quantity, not a part you can order.",
                            f.id,
                            p.value.as_deref().unwrap_or("unset")
                        ),
                    ));
                }
                // Warn, not fail: JLCPCB can still be asked to source a
                // part from its MPN by hand, so a missing LCSC number does
                // not make the board unbuildable the way a missing MPN
                // does. It does leave a blank "LCSC Part #" cell in the
                // BOM, which is worth a flag before ordering.
                let has_lcsc = p.lcsc.as_deref().is_some_and(|c| !c.trim().is_empty());
                if !has_lcsc {
                    out.push(CheckResult {
                        check: "fab_no_lcsc".into(),
                        status: eda_model::CheckStatus::Warn,
                        location: Some(f.id.clone()),
                        hint: Some(format!(
                            "{} has no LCSC part number, so its \"LCSC Part #\" cell in the JLCPCB BOM will be blank; \
                             look one up on jlcpcb.com before ordering{}.",
                            f.id,
                            p.mpn.as_deref().map(|m| format!(" (mpn {m})")).unwrap_or_default()
                        )),
                        detail: None,
                    });
                }
            }
        }
    }
    let placed: std::collections::HashSet<&str> = pl.footprints.iter().map(|f| f.id.as_str()).collect();
    for p in &model.parts {
        if !placed.contains(p.reference.as_str()) {
            out.push(CheckResult::fail(
                "fab_unplaced_part",
                p.reference.clone(),
                format!("{} is in the netlist but was never placed; it would be ordered and have nowhere to go", p.reference),
            ));
        }
    }
    out.sort_by(|a, b| (&a.check, &a.location).cmp(&(&b.check, &b.location)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{FootprintInstance, PlacementSection, Point, Provenance, Side};
    use eda_model::{CheckStatus, Part};

    fn part(reference: &str, mpn: Option<&str>) -> Part {
        Part {
            reference: reference.into(),
            mpn: mpn.map(|s| s.to_string()),
            lcsc: mpn.map(|_| "C00000".to_string()),
            value: Some("1uF, 16V".into()),
            package: Some("0402".into()),
            footprint: None,
            pins: vec![],
            body_um: None, symbol: None, datasheet: None,
            edge: None,
        }
    }

    fn fixture() -> (Design, ConstraintModel) {
        let mut model = ConstraintModel::default();
        model.parts = vec![part("R1", Some("RC0402")), part("R10", Some("RC0402")), part("R2", Some("RC0402"))];
        let placement = PlacementSection {
            outline: vec![Point { x: 0, y: 0 }, Point { x: 10_000, y: 0 }, Point { x: 10_000, y: 10_000 }],
            footprints: vec![
                FootprintInstance { id: "R1".into(), at: Point { x: 1_000, y: 2_000 }, rot: 90_000, side: Side::Top, label: Default::default() },
                FootprintInstance { id: "R10".into(), at: Point { x: 3_000, y: 4_000 }, rot: 0, side: Side::Bottom, label: Default::default() },
                FootprintInstance { id: "R2".into(), at: Point { x: 5_000, y: 6_000 }, rot: 0, side: Side::Top, label: Default::default() },
            ],
            modules: Vec::new(),
        };
        let design = Design {
            footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None, nets: None,
            placement: Some(placement),
            routing: None,
            drawings: None,
        };
        (design, model)
    }

    fn fails(c: &[CheckResult]) -> Vec<String> {
        c.iter().filter(|c| c.status == CheckStatus::Fail).map(|c| c.check.clone()).collect()
    }

    #[test]
    fn a_complete_package_passes() {
        let (design, model) = fixture();
        assert!(check(&design, &model).is_empty());
    }

    #[test]
    fn a_placement_with_no_part_number_fails() {
        // The defect a board house finds after you have paid.
        let (design, mut model) = fixture();
        model.parts[0].mpn = None;
        assert_eq!(fails(&check(&design, &model)), vec!["fab_no_mpn"]);
    }

    #[test]
    fn a_blank_part_number_fails_like_a_missing_one() {
        let (design, mut model) = fixture();
        model.parts[0].mpn = Some("   ".into());
        assert_eq!(fails(&check(&design, &model)), vec!["fab_no_mpn"]);
    }

    #[test]
    fn a_missing_lcsc_number_warns_but_does_not_fail() {
        // Unlike a missing mpn, a missing LCSC number does not make the
        // board unbuildable -- JLCPCB can still be asked to source it by
        // hand -- so it is a warning, not a blocker for the rest of the
        // package.
        let (design, mut model) = fixture();
        model.parts[0].lcsc = None;
        let checks = check(&design, &model);
        assert!(fails(&checks).is_empty(), "{checks:?}");
        let warns: Vec<_> = checks.iter().filter(|c| c.status == CheckStatus::Warn).map(|c| c.check.as_str()).collect();
        assert_eq!(warns, vec!["fab_no_lcsc"]);
    }

    #[test]
    fn a_part_that_was_never_placed_fails() {
        // It would be ordered and have nowhere to go.
        let (design, mut model) = fixture();
        model.parts.push(part("R3", Some("RC0402")));
        assert_eq!(fails(&check(&design, &model)), vec!["fab_unplaced_part"]);
    }

    #[test]
    fn a_placement_with_no_part_fails() {
        let (mut design, model) = fixture();
        design.placement.as_mut().unwrap().footprints.push(FootprintInstance {
            id: "R99".into(),
            at: Point { x: 0, y: 0 },
            rot: 0,
            side: Side::Top,
            label: Default::default(),
        });
        assert_eq!(fails(&check(&design, &model)), vec!["fab_unknown_part"]);
    }
}
