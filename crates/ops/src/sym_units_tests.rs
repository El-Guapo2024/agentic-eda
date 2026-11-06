//! r7 E7-S5, S6, S7, S10: Symbol Properties' unit count and body styles change the symbol's items as `LIB_SYMBOL::SetUnitCount` and
//! `SetBodyStyleCount` do, and a pin's unit and body style are ones the symbol has (the Pin Table's combo boxes).

use super::*;
use eda_model::ir::{Design, LibraryFill};
use eda_model::symbol::SPoint;
use eda_model::ConstraintModel;
use eda_model::units::mm;

fn design() -> Design {
    serde_json::from_str(r#"{"schema":1,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]}}"#).unwrap()
}

fn board(m: &ConstraintModel) -> Board<'_> {
    Board::new(design(), m, mm(0.1), mm(0.3))
}

fn props(lib_id: &str, unit_count: u32, demorgan: bool) -> Cmd {
    Cmd::EditSymbolProperties {
        lib_id: lib_id.into(),
        reference_prefix: "U".into(),
        description: String::new(),
        keywords: String::new(),
        datasheet: String::new(),
        power: false,
        in_bom: true,
        on_board: true,
        pin_numbers_hidden: false,
        pin_names_hidden: false,
        pin_name_offset_mm: 0.508,
        unit_count,
        has_alternate_body_style: demorgan,
        footprint_filters: vec![],
        units_locked: None,
        power_local: None,
        exclude_from_sim: None,
        exclude_from_pos_files: None,
    }
}

fn pin(number: &str, unit: u32, body_style: u32) -> LibrarySymbolPin {
    LibrarySymbolPin {
        id: String::new(),
        number: number.into(),
        name: String::new(),
        electrical_type: "passive".into(),
        shape: "line".into(),
        at: SPoint { x: -5.08, y: 0.0 },
        angle_deg: 0.0,
        length_mm: 2.54,
        unit,
        body_style,
        hidden: false,
        name_size_mm: None,
        number_size_mm: None,
        alternates: vec![],
    }
}

fn rect(unit: u32, body_style: u32) -> LibrarySymbolGraphic {
    LibrarySymbolGraphic::Rectangle { id: String::new(), unit, body_style, start: SPoint { x: -2.54, y: 2.54 }, end: SPoint { x: 2.54, y: -2.54 }, stroke_mm: 0.254, fill: LibraryFill::default() }
}

fn sym<'a>(b: &'a Board<'_>, lib_id: &str) -> &'a LibrarySymbol {
    b.design().symbol_library.as_ref().unwrap().by_lib_id(lib_id).unwrap()
}

fn units_of_pins(b: &Board<'_>, lib_id: &str) -> Vec<(String, u32, u32)> {
    let mut v: Vec<_> = sym(b, lib_id).pins.iter().map(|p| (p.number.clone(), p.unit, p.body_style)).collect();
    v.sort();
    v
}

fn add_pin(b: &mut Board<'_>, lib_id: &str, p: LibrarySymbolPin) {
    b.apply(&Cmd::AddSymbolPin { lib_id: lib_id.into(), pin: p }).unwrap();
}

#[test]
fn lowering_the_unit_count_removes_the_items_of_the_dropped_units() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    b.apply(&Cmd::NewSymbol { lib_id: "Lib:Q".into() }).unwrap();
    b.apply(&props("Lib:Q", 3, false)).unwrap();
    for (n, u) in [("1", 1), ("2", 2), ("3", 3), ("8", 0)] {
        add_pin(&mut b, "Lib:Q", pin(n, u, 1));
    }
    for u in [0, 1, 2, 3] {
        b.apply(&Cmd::AddSymbolGraphic { lib_id: "Lib:Q".into(), graphic: rect(u, 1) }).unwrap();
    }
    b.apply(&props("Lib:Q", 2, false)).unwrap();
    let s = sym(&b, "Lib:Q");
    assert_eq!(s.unit_count, 2);
    assert_eq!(units_of_pins(&b, "Lib:Q"), [("1".to_string(), 1, 1), ("2".to_string(), 2, 1), ("8".to_string(), 0, 1)], "unit 3's pin is erased, a pin common to all units stays");
    let mut g: Vec<u32> = sym(&b, "Lib:Q").graphics.iter().map(|g| g.unit()).collect();
    g.sort();
    assert_eq!(g, [0, 1, 2]);
}

#[test]
fn raising_the_unit_count_copies_unit_1_into_each_new_unit() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    b.apply(&Cmd::NewSymbol { lib_id: "Lib:Q".into() }).unwrap();
    for (n, u) in [("1", 1), ("2", 1), ("8", 0)] {
        add_pin(&mut b, "Lib:Q", pin(n, u, 1));
    }
    b.apply(&Cmd::AddSymbolGraphic { lib_id: "Lib:Q".into(), graphic: rect(1, 1) }).unwrap();
    b.apply(&props("Lib:Q", 3, false)).unwrap();
    assert_eq!(
        units_of_pins(&b, "Lib:Q"),
        [("1".to_string(), 1, 1), ("1".to_string(), 2, 1), ("1".to_string(), 3, 1), ("2".to_string(), 1, 1), ("2".to_string(), 2, 1), ("2".to_string(), 3, 1), ("8".to_string(), 0, 1)],
        "`SetUnitCount( n, true )` duplicates the items of unit 1 only"
    );
    let s = sym(&b, "Lib:Q");
    let mut g: Vec<u32> = s.graphics.iter().map(|g| g.unit()).collect();
    g.sort();
    assert_eq!(g, [1, 2, 3]);
    let mut ids: Vec<&str> = s.pins.iter().map(|p| p.id.as_str()).chain(s.graphics.iter().map(|g| g.id())).collect();
    let n = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), n, "every copy has an id of its own");
    assert!(ids.iter().all(|i| !i.is_empty()));
}

#[test]
fn de_morgan_on_copies_the_style_1_pins_and_off_removes_style_2() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    b.apply(&Cmd::NewSymbol { lib_id: "Lib:G".into() }).unwrap();
    add_pin(&mut b, "Lib:G", pin("1", 1, 1));
    add_pin(&mut b, "Lib:G", pin("2", 1, 0));
    b.apply(&Cmd::AddSymbolGraphic { lib_id: "Lib:G".into(), graphic: rect(1, 1) }).unwrap();
    b.apply(&props("Lib:G", 1, true)).unwrap();
    assert_eq!(units_of_pins(&b, "Lib:G"), [("1".to_string(), 1, 1), ("1".to_string(), 1, 2), ("2".to_string(), 1, 0)], "25ec2335 `SetBodyStyleCount( 2, false, true )`: the pins");
    assert_eq!(sym(&b, "Lib:G").graphics.len(), 1, "and not the graphics");
    b.apply(&Cmd::AddSymbolGraphic { lib_id: "Lib:G".into(), graphic: rect(1, 2) }).unwrap();
    b.apply(&props("Lib:G", 1, false)).unwrap();
    assert_eq!(units_of_pins(&b, "Lib:G"), [("1".to_string(), 1, 1), ("2".to_string(), 1, 0)], "`SetBodyStyleCount( 1, .. )` erases body style 2");
    assert!(sym(&b, "Lib:G").graphics.iter().all(|g| g.body_style() <= 1));
    assert!(!sym(&b, "Lib:G").has_alternate_body_style);
}

#[test]
fn a_pins_unit_beyond_the_unit_count_or_a_body_style_the_symbol_has_not_is_refused() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    b.apply(&Cmd::NewSymbol { lib_id: "Lib:Q".into() }).unwrap();
    b.apply(&props("Lib:Q", 2, false)).unwrap();
    let e = b.apply(&Cmd::AddSymbolPin { lib_id: "Lib:Q".into(), pin: pin("5", 5, 1) }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_pin");
    let e = b.apply(&Cmd::AddSymbolPin { lib_id: "Lib:Q".into(), pin: pin("5", 1, 2) }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_pin", "body style 2 without De Morgan would be dropped by the writer");
    add_pin(&mut b, "Lib:Q", pin("1", 2, 0));
    let id = sym(&b, "Lib:Q").pins[0].id.clone();
    let e = b.apply(&Cmd::EditSymbolPin { lib_id: "Lib:Q".into(), id: id.clone(), pin: pin("1", 3, 1) }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_pin");
    b.apply(&props("Lib:Q", 2, true)).unwrap();
    b.apply(&Cmd::EditSymbolPin { lib_id: "Lib:Q".into(), id, pin: pin("1", 0, 2) }).unwrap();
    assert_eq!(units_of_pins(&b, "Lib:Q"), [("1".to_string(), 0, 2)]);
}

#[test]
fn the_unit_names_of_dropped_units_go_with_them() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    b.apply(&Cmd::NewSymbol { lib_id: "Lib:Q".into() }).unwrap();
    b.apply(&props("Lib:Q", 3, false)).unwrap();
    {
        let s = b.design.symbol_library.as_mut().unwrap().by_lib_id_mut("Lib:Q").unwrap();
        s.kicad.unit_names.insert(1, "Gate".into());
        s.kicad.unit_names.insert(3, "Power".into());
    }
    b.apply(&props("Lib:Q", 2, false)).unwrap();
    assert_eq!(sym(&b, "Lib:Q").kicad.unit_names.keys().copied().collect::<Vec<_>>(), [1]);
}

/// UI-LEFT Y4: Symbol Properties' Units & Body Styles and Pin Connections tabs (KiCad 25ec2335 `DIALOG_LIB_SYMBOL_PROPERTIES::TransferDataFromWindow`).
fn units_and_connections(lib_id: &str, names: &[(u32, &str)], demorgan: bool, styles: &[&str], jumpers_dup: bool, rows: &[&str]) -> Cmd {
    Cmd::EditSymbolUnitsAndConnections {
        lib_id: lib_id.into(),
        unit_names: names.iter().map(|(u, n)| (*u, n.to_string())).collect(),
        demorgan,
        body_style_names: styles.iter().map(|s| s.to_string()).collect(),
        duplicate_pin_numbers_are_jumpers: jumpers_dup,
        jumper_rows: rows.iter().map(|r| r.to_string()).collect(),
    }
}

fn styles_of(b: &Board<'_>, lib_id: &str) -> (Vec<u32>, Vec<u32>) {
    let s = sym(b, lib_id);
    let mut p: Vec<u32> = s.pins.iter().map(|p| p.body_style).collect();
    let mut g: Vec<u32> = s.graphics.iter().map(|g| g.body_style()).collect();
    p.sort();
    g.sort();
    (p, g)
}

#[test]
fn custom_body_styles_are_populated_from_the_standard_one_and_pruned_as_25ec2335_does() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    b.apply(&Cmd::NewSymbol { lib_id: "Lib:B".into() }).unwrap();
    add_pin(&mut b, "Lib:B", pin("1", 1, 1));
    add_pin(&mut b, "Lib:B", pin("2", 1, 0));
    b.apply(&Cmd::AddSymbolGraphic { lib_id: "Lib:B".into(), graphic: rect(1, 1) }).unwrap();
    // single -> three custom styles: `SetBodyStyleCount( 3, true, true )` copies every style-1 item into styles 2 and 3
    b.apply(&units_and_connections("Lib:B", &[], false, &["Front", "Side", "Top"], false, &[])).unwrap();
    assert_eq!(styles_of(&b, "Lib:B"), (vec![0, 1, 2, 3], vec![1, 2, 3]));
    let s = sym(&b, "Lib:B");
    assert_eq!(s.body_style_names, ["Front", "Side", "Top"]);
    assert!(!s.has_alternate_body_style);
    assert_eq!((s.body_style_count(), s.body_style_description(2).as_str(), s.body_style_description(4).as_str()), (3, "Side", "?"));
    // three -> two: `PruneBodyStyleDrawItems( 2 )`, nothing copied a second time into style 2 (8303b2ad's did)
    b.apply(&units_and_connections("Lib:B", &[], false, &["Front", "", "Side"], false, &[])).unwrap();
    assert_eq!(styles_of(&b, "Lib:B"), (vec![0, 1, 2], vec![1, 2]), "the empty row is no style");
    assert_eq!(sym(&b, "Lib:B").body_style_names, ["Front", "Side"]);
    // two custom -> De Morgan: the count stays 2, nothing copied or erased
    b.apply(&units_and_connections("Lib:B", &[], true, &[], false, &[])).unwrap();
    assert_eq!(styles_of(&b, "Lib:B"), (vec![0, 1, 2], vec![1, 2]));
    assert!(sym(&b, "Lib:B").has_alternate_body_style && sym(&b, "Lib:B").body_style_names.is_empty());
    assert_eq!(sym(&b, "Lib:B").body_style_description(1), "Standard");
    // a "not De Morgan" from Symbol Properties' own verb leaves custom styles alone, and single erases them
    b.apply(&units_and_connections("Lib:B", &[], false, &["A", "B"], false, &[])).unwrap();
    b.apply(&props("Lib:B", 1, false)).unwrap();
    assert_eq!(sym(&b, "Lib:B").body_style_names, ["A", "B"]);
    b.apply(&units_and_connections("Lib:B", &[], false, &[], false, &[])).unwrap();
    assert_eq!(styles_of(&b, "Lib:B"), (vec![0, 1], vec![1]));
    assert!(!sym(&b, "Lib:B").is_multi_body_style());
    // "Custom body styles must have at least 2 entries"
    let e = b.apply(&units_and_connections("Lib:B", &[], false, &["Only"], false, &[])).unwrap_err();
    assert_eq!(e[0].hint.as_deref(), Some("Custom body styles must have at least 2 entries"));
    // a pin of body style 3 is one the symbol has once it has three
    b.apply(&units_and_connections("Lib:B", &[], false, &["A", "B", "C"], false, &[])).unwrap();
    add_pin(&mut b, "Lib:B", pin("9", 1, 3));
    assert!(b.apply(&Cmd::AddSymbolPin { lib_id: "Lib:B".into(), pin: pin("9", 1, 4) }).is_err());
}

#[test]
fn unit_names_and_jumper_groups_are_the_grids_rows() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    b.apply(&Cmd::NewSymbol { lib_id: "Lib:J".into() }).unwrap();
    b.apply(&props("Lib:J", 2, false)).unwrap();
    for n in ["1", "2", "3", "4"] {
        add_pin(&mut b, "Lib:J", pin(n, 1, 1));
    }
    b.apply(&units_and_connections("Lib:J", &[(1, "Gate"), (2, ""), (3, "Gone")], false, &[], true, &["3, 1", "", "4\t2 2"])).unwrap();
    let s = sym(&b, "Lib:J");
    assert_eq!(s.kicad.unit_names.iter().map(|(u, n)| (*u, n.as_str())).collect::<Vec<_>>(), [(1, "Gate")], "an empty row and a unit past the count have no name");
    assert!(s.kicad.duplicate_pin_numbers_are_jumpers);
    assert_eq!(s.kicad.jumper_pin_groups, vec![vec!["1".to_string(), "3".to_string()], vec!["2".to_string(), "4".to_string()]], "sorted sets; an empty row is no group (`JUMPER_GROUP::Make`)");
    let e = b.apply(&units_and_connections("Lib:J", &[], false, &[], false, &["1 2", "1, 9"])).unwrap_err();
    assert_eq!(e[0].hint.as_deref(), Some("Pin '9' in jumper pin group 2 does not exist in this symbol."));
    assert_eq!(sym(&b, "Lib:J").kicad.jumper_pin_groups.len(), 2, "a refused edit changes nothing");
    // the written library says it as KiCad does
    let text = eda_kicad::export_kicad_sym(sym(&b, "Lib:J"));
    assert!(text.contains("(duplicate_pin_numbers_are_jumpers yes)") && text.contains(r#"(jumper_pin_groups ("1" "3") ("2" "4"))"#) && text.contains(r#"(unit_name "Gate")"#), "{text}");
}

#[test]
fn a_derived_symbols_units_and_pins_are_its_parents() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    let mut child = LibrarySymbol::new_empty("Lib:Child");
    child.extends = Some("Lib:Parent".into());
    b.apply(&Cmd::NewSymbol { lib_id: "Lib:Parent".into() }).unwrap();
    b.apply(&Cmd::PutLibrarySymbol { symbol: child, overwrite: false }).unwrap();
    assert!(b.apply(&units_and_connections("Lib:Child", &[(1, "A")], false, &[], false, &[])).is_err());
}
