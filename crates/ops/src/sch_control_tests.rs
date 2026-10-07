//! `Board::apply` for the schematic-control verbs of `sch_control.rs`.
use super::*;
use eda_model::ir::Provenance;

fn p(x: Um, y: Um) -> Point {
    Point { x, y }
}

fn empty_design() -> Design {
    Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: 1,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        routing: None,
        placement: None,
        drawings: None,
    }
}

fn place(b: &mut Board<'_>, id: &str, lib_id: &str, unit: u32, x: Um) {
    b.apply(&Cmd::AddSymbol { id: id.into(), lib_id: lib_id.into(), at: p(x, 10_000), rot_millideg: 0, value: String::new(), footprint: String::new(), unit }).unwrap();
}

fn symbols(b: &Board<'_>) -> Vec<SymbolInstance> {
    b.design().schematic.as_ref().unwrap().symbols.clone()
}

#[test]
fn the_attribute_verb_sets_every_unit_of_a_reference_and_leaves_the_unnamed_flags_alone() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, 100, 300);
    place(&mut b, "U1", "Device:R", 1, 0);
    place(&mut b, "U1", "Device:R", 2, 20_000);
    place(&mut b, "R2", "Device:R", 1, 40_000);
    b.apply(&Cmd::SetSymbolAttrs { ids: vec!["U1".into()], dnp: Some(true), exclude_from_bom: Some(true), exclude_from_board: None, exclude_from_sim: None }).unwrap();
    let s = symbols(&b);
    let u1: Vec<_> = s.iter().filter(|x| x.id == "U1").collect();
    assert_eq!(u1.len(), 2);
    assert!(u1.iter().all(|x| x.dnp && x.exclude_from_bom && !x.exclude_from_board && !x.exclude_from_sim), "both units follow");
    let r2 = s.iter().find(|x| x.id == "R2").unwrap();
    assert!(!r2.dnp && !r2.exclude_from_bom, "another reference is untouched");
    b.apply(&Cmd::SetSymbolAttrs { ids: vec!["U1".into()], dnp: Some(false), exclude_from_bom: None, exclude_from_board: Some(true), exclude_from_sim: Some(true) }).unwrap();
    let u1 = symbols(&b).into_iter().find(|x| x.id == "U1").unwrap();
    assert!(!u1.dnp && u1.exclude_from_bom && u1.exclude_from_board && u1.exclude_from_sim, "None keeps exclude_from_bom as it was");
}

#[test]
fn the_attribute_verb_refuses_nothing_selected_and_an_unknown_reference() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, 100, 300);
    place(&mut b, "R1", "Device:R", 1, 0);
    let none = Cmd::SetSymbolAttrs { ids: vec![], dnp: Some(true), exclude_from_bom: None, exclude_from_board: None, exclude_from_sim: None };
    assert_eq!(b.apply(&none).unwrap_err()[0].check, "ops_nothing_selected");
    let unknown = Cmd::SetSymbolAttrs { ids: vec!["R1".into(), "R9".into()], dnp: Some(true), exclude_from_bom: None, exclude_from_board: None, exclude_from_sim: None };
    assert_eq!(b.apply(&unknown).unwrap_err()[0].check, "ops_unknown_symbol");
    assert!(!symbols(&b)[0].dnp, "a refused verb changes nothing, not even for the reference that exists");
}

#[test]
fn a_sheet_page_is_set_on_the_placement_wherever_it_is_and_validated() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, 100, 300);
    b.apply(&Cmd::AddSheet { name: "Power".into(), file: "power".into(), at: p(10_000, 10_000), size: (30_000, 20_000) }).unwrap();
    let id = b.design().schematic.as_ref().unwrap().sheets[0].id.clone();
    b.apply(&Cmd::SetSheetPage { sheet: id.clone(), page: " A3 ".into() }).unwrap();
    assert_eq!(b.design().schematic.as_ref().unwrap().sheets[0].page, "A3", "trimmed");
    assert_eq!(b.apply(&Cmd::SetSheetPage { sheet: id.clone(), page: "2 b".into() }).unwrap_err()[0].check, "ops_bad_page");
    assert_eq!(b.apply(&Cmd::SetSheetPage { sheet: "nope".into(), page: "2".into() }).unwrap_err()[0].check, "ops_unknown_sheet");
    b.apply(&Cmd::SetSheetPage { sheet: id, page: String::new() }).unwrap();
    assert!(b.design().schematic.as_ref().unwrap().sheets[0].page.is_empty(), "empty puts the sheet back to its place in the hierarchy");
}

#[test]
fn a_sheet_page_reaches_a_placement_that_lives_on_a_child_screen() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, 100, 300);
    b.apply(&Cmd::AddSheet { name: "A".into(), file: "a".into(), at: p(0, 0), size: (10_000, 10_000) }).unwrap();
    // a grandchild placement written straight into the child screen (no verb draws on a child screen yet)
    let mut design = b.design().clone();
    let grandchild = eda_model::ir::SheetInstance { id: "g1".into(), name: "G".into(), file: "g.kicad_sch".into(), at: p(0, 0), size: (1000, 1000), pins: vec![], page: String::new() };
    design.sheet_contents.as_mut().unwrap().get_mut("a.kicad_sch").unwrap().sheets.push(grandchild);
    let mut b = Board::new(design, &m, 100, 300);
    b.apply(&Cmd::SetSheetPage { sheet: "g1".into(), page: "9".into() }).unwrap();
    assert_eq!(b.design().sheet_contents.as_ref().unwrap()["a.kicad_sch"].sheets[0].page, "9");
}

#[test]
fn a_label_or_text_changes_its_text_and_keeps_its_id_and_place() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, 100, 300);
    b.apply(&Cmd::AddLabel { net: "DATA0".into(), at: p(5_000, 5_000), kind: eda_model::ir::LabelKind::Local }).unwrap();
    b.apply(&Cmd::AddSchText { content: "Rev A".into(), at: p(9_000, 9_000), angle_millideg: 0, size_um: 1270 }).unwrap();
    // the studio loads the design fresh for every verb, which hands out the ids; do the same here
    let mut design = b.design().clone();
    design.assign_missing_ids();
    let mut b = Board::new(design, &m, 100, 300);
    let (lid, tid) = {
        let sch = b.design().schematic.as_ref().unwrap();
        (sch.labels[0].id.clone(), sch.texts[0].id.clone())
    };
    b.apply(&Cmd::SetSchItemText { id: lid.clone(), text: "DATA1".into() }).unwrap();
    b.apply(&Cmd::SetSchItemText { id: tid.clone(), text: "Rev B".into() }).unwrap();
    let sch = b.design().schematic.as_ref().unwrap();
    assert_eq!((sch.labels[0].id.as_str(), sch.labels[0].net.as_str(), sch.labels[0].at), (lid.as_str(), "DATA1", p(5_000, 5_000)));
    assert_eq!((sch.texts[0].id.as_str(), sch.texts[0].content.as_str()), (tid.as_str(), "Rev B"));
    assert_eq!(b.apply(&Cmd::SetSchItemText { id: lid, text: String::new() }).unwrap_err()[0].check, "ops_bad_label", "a label's text is its net name");
    assert_eq!(b.apply(&Cmd::SetSchItemText { id: "nope".into(), text: "x".into() }).unwrap_err()[0].check, "ops_unknown_item");
}

#[test]
fn the_library_link_of_a_group_moves_with_its_value_proxy_and_is_checked_first() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, 100, 300);
    b.apply(&Cmd::AddSymbol { id: "R1".into(), lib_id: "Device:R".into(), at: p(0, 0), rot_millideg: 0, value: "R".into(), footprint: String::new(), unit: 1 }).unwrap();
    b.apply(&Cmd::AddSymbol { id: "R2".into(), lib_id: "Device:R".into(), at: p(10_000, 0), rot_millideg: 0, value: "10k".into(), footprint: String::new(), unit: 1 }).unwrap();
    b.apply(&Cmd::AddSymbol { id: "C1".into(), lib_id: "Device:C".into(), at: p(20_000, 0), rot_millideg: 0, value: "C".into(), footprint: String::new(), unit: 1 }).unwrap();
    let change = |from: &str, to: &str| Cmd::SetSymbolLibIds { changes: vec![(from.into(), to.into())], update_fields: false };
    assert_eq!(b.apply(&change("Device:R", "R")).unwrap_err()[0].check, "ops_bad_lib_id");
    assert_eq!(b.apply(&change("Device:R", "Device:Nope")).unwrap_err()[0].check, "ops_unknown_symbol");
    assert_eq!(b.apply(&change("Device:X", "Device:L")).unwrap_err()[0].check, "ops_unknown_lib_id");
    assert_eq!(b.apply(&Cmd::SetSymbolLibIds { changes: vec![], update_fields: false }).unwrap_err()[0].check, "ops_nothing_to_change");
    b.apply(&change("Device:R", "Device:L")).unwrap();
    let s = symbols(&b);
    let get = |id: &str| s.iter().find(|x| x.id == id).unwrap().clone();
    assert_eq!(get("R1").lib_id, "Device:L");
    assert_eq!(get("R1").value, "L", "a value that was the old item's name follows the link");
    assert_eq!(get("R2").value, "10k", "a real value stays");
    assert_eq!(get("C1").lib_id, "Device:C", "another group is untouched");

    // "Update symbol fields from new library" resets Value and Footprint to the library symbol's
    b.apply(&Cmd::EditSymbolFields { id: "R2".into(), value: None, footprint: Some("Resistor_SMD:R_0603".into()), datasheet: None }).unwrap();
    b.apply(&Cmd::SetSymbolLibIds { changes: vec![("Device:L".into(), "Device:LED".into())], update_fields: true }).unwrap();
    let s = symbols(&b);
    let r2 = s.iter().find(|x| x.id == "R2").unwrap();
    assert_eq!((r2.lib_id.as_str(), r2.value.as_str(), r2.footprint.as_str()), ("Device:LED", "LED", ""));
}

#[test]
fn incrementing_annotations_moves_the_references_together_and_carries_the_pins() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, 100, 300);
    for (i, r) in ["R1", "R2", "R3", "C1"].iter().enumerate() {
        place(&mut b, r, "Device:R", 1, i as Um * 10_000);
    }
    b.apply(&Cmd::IncrementAnnotations { start: "R2".into(), increment: 1 }).unwrap();
    let mut ids: Vec<String> = symbols(&b).into_iter().map(|s| s.id).collect();
    ids.sort();
    assert_eq!(ids, vec!["C1", "R1", "R3", "R4"], "R2 -> R3 and R3 -> R4 in one go, R1 is below the start");
    b.apply(&Cmd::IncrementAnnotations { start: "R3".into(), increment: -1 }).unwrap();
    let mut ids: Vec<String> = symbols(&b).into_iter().map(|s| s.id).collect();
    ids.sort();
    assert_eq!(ids, vec!["C1", "R1", "R2", "R3"]);
}

#[test]
fn incrementing_annotations_refuses_a_collision_a_negative_number_and_leaves_a_bare_start_alone() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, 100, 300);
    for (i, r) in ["R1", "R2", "R3"].iter().enumerate() {
        place(&mut b, r, "Device:R", 1, i as Um * 10_000);
    }
    let ids = |b: &Board<'_>| {
        let mut ids: Vec<String> = symbols(b).into_iter().map(|s| s.id).collect();
        ids.sort();
        ids
    };
    // R3 -> R1 would land on a symbol that stays: refused as a whole, nothing renamed
    assert_eq!(b.apply(&Cmd::IncrementAnnotations { start: "R3".into(), increment: -2 }).unwrap_err()[0].check, "ops_duplicate_symbol");
    assert_eq!(ids(&b), vec!["R1", "R2", "R3"]);
    // R1 would become R-1
    assert_eq!(b.apply(&Cmd::IncrementAnnotations { start: "R1".into(), increment: -2 }).unwrap_err()[0].check, "ops_bad_increment");
    assert_eq!(ids(&b), vec!["R1", "R2", "R3"]);
    // a start with no number is not splittable: nothing to do, and no error (the C++ returns quietly)
    b.apply(&Cmd::IncrementAnnotations { start: "R?".into(), increment: 1 }).unwrap();
    assert_eq!(ids(&b), vec!["R1", "R2", "R3"]);
}
