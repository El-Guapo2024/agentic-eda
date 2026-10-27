//! `Board::apply` for the schematic-control verbs of `sch_control.rs`.
use super::*;
use eda_model::ir::Provenance;
use eda_model::units::mm;

fn p(x: Nm, y: Nm) -> Point {
    Point { x, y }
}

fn empty_design() -> Design {
    Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        routing: None,
        placement: None,
        drawings: None,
    }
}

fn place(b: &mut Board<'_>, id: &str, lib_id: &str, unit: u32, x: Nm) {
    b.apply(&Cmd::AddSymbol { annotate: None, id: id.into(), lib_id: lib_id.into(), at: p(x, mm(10.0)), rot_millideg: 0, value: String::new(), footprint: String::new(), unit }).unwrap();
}

fn symbols(b: &Board<'_>) -> Vec<SymbolInstance> {
    b.design().schematic.as_ref().unwrap().symbols.clone()
}

#[test]
fn the_attribute_verb_sets_every_unit_of_a_reference_and_leaves_the_unnamed_flags_alone() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    place(&mut b, "U1", "Device:R", 1, Nm::ZERO);
    place(&mut b, "U1", "Device:R", 2000, mm(20.0));
    place(&mut b, "R2", "Device:R", 1, mm(40.0));
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
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    place(&mut b, "R1", "Device:R", 1, Nm::ZERO);
    let none = Cmd::SetSymbolAttrs { ids: vec![], dnp: Some(true), exclude_from_bom: None, exclude_from_board: None, exclude_from_sim: None };
    assert_eq!(b.apply(&none).unwrap_err()[0].check, "ops_nothing_selected");
    let unknown = Cmd::SetSymbolAttrs { ids: vec!["R1".into(), "R9".into()], dnp: Some(true), exclude_from_bom: None, exclude_from_board: None, exclude_from_sim: None };
    assert_eq!(b.apply(&unknown).unwrap_err()[0].check, "ops_unknown_symbol");
    assert!(!symbols(&b)[0].dnp, "a refused verb changes nothing, not even for the reference that exists");
}

#[test]
fn a_sheet_page_is_set_on_the_placement_wherever_it_is_and_validated() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::AddSheet { name: "Power".into(), file: "power".into(), at: p(mm(10.0), mm(10.0)), size: (mm(30.0), mm(20.0)) }).unwrap();
    let id = b.design().schematic.as_ref().unwrap().sheets[0].id.clone();
    b.apply(&Cmd::SetSheetPage { sheet: id.clone(), page: " A3 ".into(), path: None }).unwrap();
    assert_eq!(b.design().schematic.as_ref().unwrap().sheets[0].page, "A3", "trimmed");
    assert_eq!(b.apply(&Cmd::SetSheetPage { sheet: id.clone(), page: "2 b".into(), path: None }).unwrap_err()[0].check, "ops_bad_page");
    assert_eq!(b.apply(&Cmd::SetSheetPage { sheet: "nope".into(), page: "2".into(), path: None }).unwrap_err()[0].check, "ops_unknown_sheet");
    b.apply(&Cmd::SetSheetPage { sheet: id, page: String::new(), path: None }).unwrap();
    assert!(b.design().schematic.as_ref().unwrap().sheets[0].page.is_empty(), "empty puts the sheet back to its place in the hierarchy");
}

#[test]
fn a_drawn_sheet_gets_the_next_free_page_and_one_drawn_to_its_left_does_not_take_it() {
    // `SCH_DRAWING_TOOLS::DrawSheet`: `GetNextPageNumber`; the sheets are walked by place (`SCH_SCREEN::GetSheets`), so a sheet drawn to the
    // left of another comes before it in the walk, and the one there keeps the page it had
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    let shown = |b: &Board<'_>| eda_model::sheet_list::walk(b.design()).into_iter().map(|e| (e.name.clone(), e.effective_page())).collect::<Vec<_>>();
    let s = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
    b.apply(&Cmd::AddSheet { name: "First".into(), file: "first".into(), at: p(mm(60.0), mm(10.0)), size: (mm(10.0), mm(10.0)) }).unwrap();
    assert_eq!(shown(&b), s(&[("", "1"), ("First", "2")]));
    b.apply(&Cmd::AddSheet { name: "Second".into(), file: "second".into(), at: p(mm(30.0), mm(10.0)), size: (mm(10.0), mm(10.0)) }).unwrap();
    assert_eq!(shown(&b), s(&[("", "1"), ("Second", "3"), ("First", "2")]), "Second is walked first and is page 3");
    b.apply(&Cmd::AddSheet { name: "Third".into(), file: "third".into(), at: p(mm(0.0), mm(10.0)), size: (mm(10.0), mm(10.0)) }).unwrap();
    assert_eq!(shown(&b), s(&[("", "1"), ("Third", "4"), ("Second", "3"), ("First", "2")]));
    // a page set by hand frees its number: the next sheet drawn takes the first free one
    let first = b.design().schematic.as_ref().unwrap().sheets.iter().find(|x| x.name == "First").unwrap().id.clone();
    b.apply(&Cmd::SetSheetPage { sheet: first.clone(), page: "9".into(), path: Some(vec![first]) }).unwrap();
    b.apply(&Cmd::AddSheet { name: "Fourth".into(), file: "fourth".into(), at: p(mm(90.0), mm(10.0)), size: (mm(10.0), mm(10.0)) }).unwrap();
    assert_eq!(shown(&b), s(&[("", "1"), ("Third", "4"), ("Second", "3"), ("First", "9"), ("Fourth", "2")]));
    assert_eq!(b.apply(&Cmd::SetSheetPage { sheet: "nope".into(), page: "2".into(), path: Some(vec!["nope".into()]) }).unwrap_err()[0].check, "ops_unknown_sheet");
}

#[test]
fn a_sheet_page_reaches_a_placement_that_lives_on_a_child_screen() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::AddSheet { name: "A".into(), file: "a".into(), at: p(Nm::ZERO, Nm::ZERO), size: (mm(10.0), mm(10.0)) }).unwrap();
    // a grandchild placement written straight into the child screen (no verb draws on a child screen yet)
    let mut design = b.design().clone();
    let grandchild = eda_model::ir::SheetInstance { id: "g1".into(), name: "G".into(), file: "g.kicad_sch".into(), at: p(Nm::ZERO, Nm::ZERO), size: (mm(1.0), mm(1.0)), pins: vec![], page: String::new() };
    design.sheet_contents.as_mut().unwrap().get_mut("a.kicad_sch").unwrap().sheets.push(grandchild);
    let mut b = Board::new(design, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::SetSheetPage { sheet: "g1".into(), page: "9".into(), path: None }).unwrap();
    assert_eq!(b.design().sheet_contents.as_ref().unwrap()["a.kicad_sch"].sheets[0].page, "9");
}

#[test]
fn a_label_or_text_changes_its_text_and_keeps_its_id_and_place() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::AddLabel { net: "DATA0".into(), at: p(mm(5.0), mm(5.0)), kind: eda_model::ir::LabelKind::Local }).unwrap();
    b.apply(&Cmd::AddSchText { content: "Rev A".into(), at: p(mm(9.0), mm(9.0)), angle_millideg: 0, size_nm: mm(1.27) }).unwrap();
    // the studio loads the design fresh for every verb, which hands out the ids; do the same here
    let mut design = b.design().clone();
    design.assign_missing_ids();
    let mut b = Board::new(design, &m, mm(0.1), mm(0.3));
    let (lid, tid) = {
        let sch = b.design().schematic.as_ref().unwrap();
        (sch.labels[0].id.clone(), sch.texts[0].id.clone())
    };
    b.apply(&Cmd::SetSchItemText { id: lid.clone(), text: "DATA1".into() }).unwrap();
    b.apply(&Cmd::SetSchItemText { id: tid.clone(), text: "Rev B".into() }).unwrap();
    let sch = b.design().schematic.as_ref().unwrap();
    assert_eq!((sch.labels[0].id.as_str(), sch.labels[0].net.as_str(), sch.labels[0].at), (lid.as_str(), "DATA1", p(mm(5.0), mm(5.0))));
    assert_eq!((sch.texts[0].id.as_str(), sch.texts[0].content.as_str()), (tid.as_str(), "Rev B"));
    assert_eq!(b.apply(&Cmd::SetSchItemText { id: lid, text: String::new() }).unwrap_err()[0].check, "ops_bad_label", "a label's text is its net name");
    assert_eq!(b.apply(&Cmd::SetSchItemText { id: "nope".into(), text: "x".into() }).unwrap_err()[0].check, "ops_unknown_item");
}

#[test]
fn the_library_link_of_a_group_moves_with_its_value_proxy_and_is_checked_first() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::AddSymbol { annotate: None, id: "R1".into(), lib_id: "Device:R".into(), at: p(Nm::ZERO, Nm::ZERO), rot_millideg: 0, value: "R".into(), footprint: String::new(), unit: 1 }).unwrap();
    b.apply(&Cmd::AddSymbol { annotate: None, id: "R2".into(), lib_id: "Device:R".into(), at: p(mm(10.0), Nm::ZERO), rot_millideg: 0, value: "10k".into(), footprint: String::new(), unit: 1 }).unwrap();
    b.apply(&Cmd::AddSymbol { annotate: None, id: "C1".into(), lib_id: "Device:C".into(), at: p(mm(20.0), Nm::ZERO), rot_millideg: 0, value: "C".into(), footprint: String::new(), unit: 1 }).unwrap();
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
fn a_symbol_of_the_project_library_is_a_link_target_even_when_it_was_put_there_a_moment_ago() {
    // Export Symbols... with "Update schematic symbols to link to exported symbols": the symbols go into the project library under the
    // new nickname and the placed ones are linked to them, in one batch -- the model knows nothing of the new entries.
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    place(&mut b, "R1", "Device:R", 1, Nm::ZERO);
    let sym = eda_model::ir::LibrarySymbol {
        lib_id: "mylib:R".into(),
        reference_prefix: "R".into(),
        description: String::new(),
        keywords: String::new(),
        datasheet: "http://example.com/r.pdf".into(),
        power: false,
        in_bom: true,
        on_board: true,
        pin_numbers_hidden: false,
        pin_names_hidden: false,
        pin_name_offset_mm: 0.508,
        unit_count: 1,
        has_alternate_body_style: false,
        footprint_filters: vec![],
        graphics: vec![],
        pins: vec![],
        published: false,
        extends: None,
        fields: vec![],
        units_locked: false,
        power_local: false,
        exclude_from_sim: false,
        exclude_from_pos_files: false,
        kicad: Default::default(),
    };
    b.apply(&Cmd::SetSymbolLibIds { changes: vec![("Device:R".into(), "mylib:R".into())], update_fields: false }).unwrap_err();
    b.apply(&Cmd::PutLibrarySymbol { symbol: sym, overwrite: false }).unwrap();
    b.apply(&Cmd::SetSymbolLibIds { changes: vec![("Device:R".into(), "mylib:R".into())], update_fields: true }).unwrap();
    let r1 = symbols(&b).into_iter().find(|s| s.id == "R1").unwrap();
    assert_eq!((r1.lib_id.as_str(), r1.datasheet.as_str()), ("mylib:R", "http://example.com/r.pdf"), "linked, and the datasheet follows the new library symbol");
}

#[test]
fn incrementing_annotations_moves_the_references_together_and_carries_the_pins() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    for (i, r) in ["R1", "R2", "R3", "C1"].iter().enumerate() {
        place(&mut b, r, "Device:R", 1, i as i64 * mm(10.0));
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
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    for (i, r) in ["R1", "R2", "R3"].iter().enumerate() {
        place(&mut b, r, "Device:R", 1, i as i64 * mm(10.0));
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

#[test]
fn replace_all_in_references_renames_through_a_chain() {
    // r5 C5-M2: `SCH_FIELD::Replace` sets each reference with no uniqueness test; only the end state has to be unique.
    // R1 is left of R10, so the hits come R1 first: R1 -> R10 while R10 still stands.
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    place(&mut b, "R1", "Device:R", 1, mm(10.0));
    place(&mut b, "R10", "Device:R", 1, mm(50.0));
    let search = crate::search::SearchData { find: "R1".into(), replace: "R10".into(), replace_references: true, ..Default::default() };
    b.apply(&Cmd::ReplaceText { search, items: None, all_sheets: false }).unwrap();
    let mut refs: Vec<(String, Nm)> = symbols(&b).into_iter().map(|s| (s.id, s.at.x)).collect();
    refs.sort();
    assert_eq!(refs, vec![("R10".to_string(), mm(10.0)), ("R100".to_string(), mm(50.0))], "R1 became R10 and R10 became R100");
}

#[test]
fn replace_can_empty_a_label() {
    // r5 C5-L6: `EDA_TEXT::Replace` sets the empty text; ERC reports `empty_label_name`.
    let m = ConstraintModel::default();
    let mut d = empty_design();
    d.schematic = Some(eda_model::ir::SchematicSection { labels: vec![eda_model::ir::NetLabel { id: "lbl_x".into(), net: "X".into(), at: p(mm(30.0), mm(10.0)), kind: Default::default() }], ..Default::default() });
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let search = crate::search::SearchData { find: "X".into(), replace: String::new(), ..Default::default() };
    b.apply(&Cmd::ReplaceText { search, items: None, all_sheets: false }).unwrap();
    assert_eq!(b.design().schematic.as_ref().unwrap().labels[0].net, "");
}
