//! Fabrication outputs: the files a board house actually reads.
//!
//! A routed `.kicad_pcb` is a design. A fab package is an order. The two
//! differ in ways that only bite at the factory: the assembler needs a
//! part number for every placement, a rotation in the convention their
//! machine uses, and a side; none of that is checked by DRC, and all of
//! it is silently absent from a board that opens perfectly in a viewer.
//!
//! **Division of labour, updated.** Gerbers (`gerber`), the job file
//! (`job`), the Excellon drill file (`drill`) and KiCad's own `.pos`
//! format (`position`) are all written natively here now -- ports of
//! KiCad's own writers (`GERBER_PLOTTER`, `GERBER_JOBFILE_WRITER`,
//! `EXCELLON_WRITER`, `PLACE_FILE_EXPORTER`; see each module's own doc
//! comment), not `kicad-cli` invocations. `kicad-cli` remains the
//! *oracle* these writers are checked against (`tests/
//! kicad_cli_parity.rs`, `PARITY.md`): the copper this crate plots is the
//! same outline-plus-fill `eda_drc`/`eda_zone_filler` already compute and
//! `kicad-cli pcb drc` already checks, so there is no second flood-fill
//! implementation to drift from the first -- only a second *writer* of
//! the one fill this workspace already trusts.
//!
//! The BOM and the JLCPCB-template placement file ([`cpl_csv`]/
//! [`bom_csv`], below) are written here too, from our own model; see
//! [`position::write_pos`]'s own doc comment for how that differs from
//! KiCad's *own* position-file format, which `position` also writes.

use eda_model::ir::{Design, Side};
use eda_model::{CheckResult, ConstraintModel};

pub mod drill;
pub mod gerber;
pub mod job;
pub mod position;

/// CSV escaping: quote when the field contains a comma, quote or newline,
/// and double any embedded quote. A part described as `1uF, 16V` is not
/// exotic, and an unescaped comma silently shifts every later column.
fn csv(field: &str) -> String {
    if field.contains([',', '"', '\n']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// µm to mm, three decimals: 1 µm resolution, which is finer than any
/// assembly machine places.
fn mm(um: i64) -> String {
    format!("{:.3}", um as f64 / 1000.0)
}

/// One line per distinct part, references grouped: JLCPCB's own SMT BOM
/// template (Comment, Designator, Footprint, "LCSC Part #"). No Quantity
/// column -- JLCPCB counts designators itself -- and no bare MPN column:
/// their assembler places from their own catalog by LCSC number, not a
/// manufacturer's, so a part with no `lcsc` prints a blank cell rather
/// than the MPN standing in for it (see `Part::lcsc`).
///
/// Grouping is by (value, package, lcsc) rather than by mpn alone: two
/// parts sharing an mpn but not a package are not interchangeable, and a
/// BOM that merges them orders the wrong thing.
pub fn bom_csv(model: &ConstraintModel) -> String {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<(String, String, String), Vec<&str>> = BTreeMap::new();
    for p in &model.parts {
        let key = (
            p.value.clone().unwrap_or_default(),
            p.package.clone().or_else(|| p.footprint.clone()).unwrap_or_default(),
            p.lcsc.clone().unwrap_or_default(),
        );
        groups.entry(key).or_default().push(&p.reference);
    }
    let mut out = String::from("Comment,Designator,Footprint,LCSC Part #\n");
    for ((value, package, lcsc), mut refs) in groups {
        refs.sort_by(|a, b| natural_ref(a).cmp(&natural_ref(b)));
        out.push_str(&format!("{},{},{},{}\n", csv(&value), csv(&refs.join(",")), csv(&package), csv(&lcsc)));
    }
    out
}

/// Sort key that puts R9 before R10. A BOM ordered `R1, R10, R2` is the
/// mark of a generated file nobody read, and it makes hand-checking a
/// reel list against the board needlessly hard.
fn natural_ref(r: &str) -> (String, u64) {
    let split = r.find(|c: char| c.is_ascii_digit()).unwrap_or(r.len());
    let (alpha, num) = r.split_at(split);
    (alpha.to_string(), num.parse().unwrap_or(0))
}

/// Pick-and-place, one line per placed part.
///
/// Rotation is emitted counter-clockwise in degrees, which is what both
/// KiCad and the common JLC/PCBWay importers expect; our `rot` is
/// millidegrees in the same direction, so this is a scale, not a flip.
/// Bottom-side parts keep the same angle convention -- mirroring is the
/// assembler's job and doing it here would double-apply.
pub fn cpl_csv(design: &Design) -> Result<String, Vec<CheckResult>> {
    let Some(pl) = &design.placement else {
        return Err(vec![CheckResult::fail("fab.cpl", "placement", "no placement to emit: a pick-and-place file without positions is an empty order")]);
    };
    let mut out = String::from("Designator,Mid X,Mid Y,Layer,Rotation\n");
    let mut fps: Vec<_> = pl.footprints.iter().collect();
    fps.sort_by(|a, b| natural_ref(&a.id).cmp(&natural_ref(&b.id)));
    for f in fps {
        let layer = match f.side {
            Side::Top => "top",
            Side::Bottom => "bottom",
        };
        out.push_str(&format!(
            "{},{},{},{},{:.1}\n",
            csv(&f.id),
            mm(f.at.x),
            mm(f.at.y),
            layer,
            f.rot as f64 / 1000.0
        ));
    }
    Ok(out)
}

/// Gate the package before it is offered as one.
///
/// These are the defects a board house finds after you have paid: a
/// placement with no part number is a line item the assembler cannot
/// source, and a part number on nothing is a reel that arrives for a
/// board with nowhere to put it. DRC cannot see either, because neither
/// is geometry. Fails, it does not warn -- an unsourceable placement is
/// not a note on an otherwise good order.
pub fn check_fab(design: &Design, model: &ConstraintModel) -> Vec<CheckResult> {
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
                // generated BOM, which is worth a flag before ordering.
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
    use eda_model::ir::{FootprintInstance, PlacementSection, Point, Provenance};
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
            footprint_library: None, sheet_contents: None, bus_aliases: vec![],
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
        assert!(check_fab(&design, &model).is_empty());
    }

    #[test]
    fn a_placement_with_no_part_number_fails() {
        // The defect a board house finds after you have paid.
        let (design, mut model) = fixture();
        model.parts[0].mpn = None;
        assert_eq!(fails(&check_fab(&design, &model)), vec!["fab_no_mpn"]);
    }

    #[test]
    fn a_blank_part_number_fails_like_a_missing_one() {
        let (design, mut model) = fixture();
        model.parts[0].mpn = Some("   ".into());
        assert_eq!(fails(&check_fab(&design, &model)), vec!["fab_no_mpn"]);
    }

    #[test]
    fn a_missing_lcsc_number_warns_but_does_not_fail() {
        // Unlike a missing mpn, a missing LCSC number does not make the
        // board unbuildable -- JLCPCB can still be asked to source it by
        // hand -- so it is a warning, not a blocker for the rest of the
        // package.
        let (design, mut model) = fixture();
        model.parts[0].lcsc = None;
        let checks = check_fab(&design, &model);
        assert!(fails(&checks).is_empty(), "{checks:?}");
        let warns: Vec<_> = checks.iter().filter(|c| c.status == CheckStatus::Warn).map(|c| c.check.as_str()).collect();
        assert_eq!(warns, vec!["fab_no_lcsc"]);
    }

    #[test]
    fn a_part_that_was_never_placed_fails() {
        // It would be ordered and have nowhere to go.
        let (design, mut model) = fixture();
        model.parts.push(part("R3", Some("RC0402")));
        assert_eq!(fails(&check_fab(&design, &model)), vec!["fab_unplaced_part"]);
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
        assert_eq!(fails(&check_fab(&design, &model)), vec!["fab_unknown_part"]);
    }

    #[test]
    fn a_value_containing_a_comma_is_quoted() {
        // "1uF, 16V" unescaped shifts every later column, which is how a
        // BOM silently orders the wrong footprint.
        let (_, model) = fixture();
        let bom = bom_csv(&model);
        assert!(bom.contains("\"1uF, 16V\""), "{bom}");
    }

    #[test]
    fn references_sort_naturally_not_lexically() {
        // R1, R2, R10 -- not R1, R10, R2.
        let (_, model) = fixture();
        let bom = bom_csv(&model);
        assert!(bom.contains("\"R1,R2,R10\""), "{bom}");
    }

    #[test]
    fn identical_parts_are_one_line_with_their_lcsc_number() {
        // JLCPCB's BOM has no Quantity column of its own -- the assembler
        // counts designators -- so grouping still has to collapse R1/R2/R10
        // onto one line, just without a count column to show for it.
        let (_, model) = fixture();
        let bom = bom_csv(&model);
        assert_eq!(bom.lines().count(), 2, "header + one grouped line: {bom}");
        assert!(bom.contains(",C00000"), "{bom}");
        assert!(!bom.contains("RC0402"), "the MPN must not stand in for the LCSC column: {bom}");
    }

    #[test]
    fn a_part_with_no_lcsc_number_prints_a_blank_cell_not_the_mpn() {
        let (_, mut model) = fixture();
        model.parts[0].lcsc = None;
        model.parts[1].lcsc = None;
        model.parts[2].lcsc = None;
        let bom = bom_csv(&model);
        assert_eq!(bom, "Comment,Designator,Footprint,LCSC Part #\n\"1uF, 16V\",\"R1,R2,R10\",0402,\n");
    }

    #[test]
    fn the_bom_header_is_jlcpcbs_own() {
        let (_, model) = fixture();
        assert!(bom_csv(&model).starts_with("Comment,Designator,Footprint,LCSC Part #\n"));
    }

    #[test]
    fn the_placement_file_carries_side_and_rotation() {
        let (design, _) = fixture();
        let cpl = cpl_csv(&design).unwrap();
        assert!(cpl.contains("R1,1.000,2.000,top,90.0"), "{cpl}");
        assert!(cpl.contains("R10,3.000,4.000,bottom,0.0"), "{cpl}");
    }

    #[test]
    fn a_placement_file_without_a_placement_is_an_error_not_an_empty_file() {
        let (mut design, _) = fixture();
        design.placement = None;
        assert!(cpl_csv(&design).is_err());
    }
}
