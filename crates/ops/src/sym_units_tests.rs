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
