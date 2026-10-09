//! Tests for the body style a placed symbol is drawn in (`SchCmd::SetBodyStyle`, `SchExtras::body_styles`): Cycle Body Style and the Symbol Properties choice
//! (`SCH_EDIT_TOOL::CycleBodyStyle`, `SCH_EDIT_FRAME::SelectBodyStyle`), and what a symbol in its alternate ("De Morgan") body style is: its pins where that body
//! puts them, its key following a rename.

use super::*;
use crate::sch_edit::SchCmd;
use eda_model::ir::{field_key, Provenance, SchematicSection, SymbolInstance};
use eda_model::symbol::{AlternateBody, LibPin, LibSymbol, SPoint, SymbolGraphic};
use eda_model::{Part, Pin, PinKind};

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

fn pin(number: &str, name: &str, x: f64, y: f64, angle: f64) -> LibPin {
    LibPin { number: number.into(), name: name.into(), electrical_type: "passive".into(), shape: "line".into(), at: SPoint::new(x, y), angle_deg: angle, length_mm: 2.54, unit: 1 }
}

fn rect(x0: f64, x1: f64) -> SymbolGraphic {
    SymbolGraphic::Rectangle { unit: 1, start: SPoint::new(x0, 2.54), end: SPoint::new(x1, -2.54), stroke_mm: 0.254, filled: false }
}

/// A gate whose alternate body has pin 2 one grid cell further down: the two bodies do not put every pin in the same place.
fn gate() -> LibSymbol {
    LibSymbol {
        lib_id: "test:GATE".into(),
        graphics: vec![rect(-2.54, 2.54)],
        pins: vec![pin("1", "A", -7.62, 1.27, 0.0), pin("2", "B", -7.62, -1.27, 0.0), pin("3", "Y", 7.62, 0.0, 180.0)],
        power: false,
        in_bom: true,
        on_board: true,
        datasheet: String::new(),
        description: String::new(),
        reference_prefix: "U".into(),
        unit_count: 1,
        pin_names_hidden: false,
        pin_numbers_hidden: false,
        pin_name_offset_mm: 0.508,
        alternate: Some(Box::new(AlternateBody::new(vec![rect(-3.81, 3.81)], vec![pin("1", "A", -7.62, 1.27, 0.0), pin("2", "B", -7.62, -2.54, 0.0), pin("3", "Y", 7.62, 0.0, 180.0)]))),
    }
}

fn part(reference: &str, pins: &[&str]) -> Part {
    Part {
        reference: reference.into(),
        mpn: None,
        lcsc: None,
        value: Some("v".into()),
        package: None,
        footprint: None,
        symbol: None,
        datasheet: None,
        pins: pins.iter().map(|n| Pin { number: n.to_string(), name: None, kind: PinKind::Passive }).collect(),
        body_um: None,
        edge: None,
    }
}

fn model() -> ConstraintModel {
    let mut resistor = eda_model::symbol::builtin("Device:R").expect("the built-in resistor");
    resistor.lib_id = "Device:R".into();
    ConstraintModel { parts: vec![part("U1", &["1", "2", "3"]), part("U2", &["1", "2", "3"]), part("R1", &["1", "2"])], symbols: vec![gate(), resistor], ..Default::default() }
}

fn instance(id: &str, lib_id: &str, at: Point) -> SymbolInstance {
    SymbolInstance { id: id.into(), at, rot: 0, mirrored: false, mirror_y: false, lib_id: lib_id.into(), unit: 1, value: "v".into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false }
}

fn sheet() -> SchematicSection {
    SchematicSection {
        symbols: vec![instance("U1", "test:GATE", Point { x: 50_800, y: 50_800 }), instance("U2", "test:GATE", Point { x: 101_600, y: 50_800 }), instance("R1", "Device:R", Point { x: 152_400, y: 50_800 })],
        ..Default::default()
    }
}

fn board<'a>(m: &'a ConstraintModel) -> Board<'a> {
    let mut design = empty_design();
    design.schematic = Some(sheet());
    Board::new(design, m, 100, 300)
}

fn set(ids: &[&str], style: Option<u32>) -> Cmd {
    Cmd::SchEdit(SchCmd::SetBodyStyle { ids: ids.iter().map(|s| s.to_string()).collect(), style })
}

fn style_of(b: &Board<'_>, id: &str) -> u32 {
    let sch = b.design().schematic.as_ref().unwrap();
    sch.body_style_of(sch.symbols.iter().find(|s| s.id == id).unwrap())
}

/// Where the pins of `id` are on the sheet, by pin number.
fn pins_of(b: &Board<'_>, m: &ConstraintModel, id: &str) -> Vec<(String, Point)> {
    let sch = b.design().schematic.as_ref().unwrap();
    let sym = sch.symbols.iter().find(|s| s.id == id).unwrap();
    let part = m.part(id).unwrap();
    eda_engine::placed::pin_points(sym, part, m.real_symbol_of_instance(sch, sym, part).as_ref())
}

#[test]
fn cycle_body_style_goes_to_the_next_style_and_back_to_the_first() {
    let m = model();
    let mut b = board(&m);
    assert_eq!(style_of(&b, "U1"), 1, "a symbol is placed in its normal body style");
    b.apply(&set(&["U1"], None)).unwrap();
    assert_eq!(style_of(&b, "U1"), 2);
    assert_eq!(style_of(&b, "U2"), 1, "only the symbol asked for");
    assert_eq!(b.design().schematic.as_ref().unwrap().extras.body_styles.get("U1"), Some(&2));
    // `nextBodyStyle > GetBodyStyleCount()` -> 1; the first style keeps no entry
    b.apply(&set(&["U1"], None)).unwrap();
    assert_eq!(style_of(&b, "U1"), 1);
    assert!(b.design().schematic.as_ref().unwrap().extras.body_styles.is_empty());
}

#[test]
fn select_body_style_is_clamped_and_a_no_op_changes_nothing() {
    let m = model();
    let mut b = board(&m);
    // `if( aBodyStyle > bodyStyleCount ) aBodyStyle = bodyStyleCount`
    b.apply(&set(&["U1", "U2"], Some(9))).unwrap();
    assert_eq!((style_of(&b, "U1"), style_of(&b, "U2")), (2, 2));
    // `currentBodyStyle == aBodyStyle`: nothing to do, so nothing is refused to be undone
    let before = serde_json::to_value(b.design().schematic.as_ref().unwrap()).unwrap();
    let again = b.apply(&set(&["U1", "U2"], Some(2)));
    assert!(again.is_err(), "every symbol is in that style already");
    assert_eq!(serde_json::to_value(b.design().schematic.as_ref().unwrap()).unwrap(), before);
    // a mix: the ones that change change
    b.apply(&set(&["U1"], Some(1))).unwrap();
    b.apply(&set(&["U1", "U2"], Some(2))).unwrap();
    assert_eq!((style_of(&b, "U1"), style_of(&b, "U2")), (2, 2));
}

#[test]
fn a_symbol_with_one_body_style_is_refused() {
    let m = model();
    let mut b = board(&m);
    let err = b.apply(&set(&["R1"], None)).unwrap_err();
    assert_eq!(err[0].check, "ops_no_body_style", "{err:?}");
    assert!(b.design().schematic.as_ref().unwrap().extras.body_styles.is_empty());
    let err = b.apply(&set(&["nope"], None)).unwrap_err();
    assert_eq!(err[0].check, "ops_unknown_symbol", "{err:?}");
    // R1 alongside a gate: the gate changes, R1 stays
    b.apply(&set(&["R1", "U1"], None)).unwrap();
    assert_eq!((style_of(&b, "R1"), style_of(&b, "U1")), (1, 2));
}

#[test]
fn a_symbol_in_the_alternate_body_style_has_its_pins_where_that_body_puts_them() {
    let m = model();
    let mut b = board(&m);
    let pin2 = |pins: &[(String, Point)]| pins.iter().find(|(n, _)| n == "2").unwrap().1;
    let pin1 = |pins: &[(String, Point)]| pins.iter().find(|(n, _)| n == "1").unwrap().1;
    let normal = pins_of(&b, &m, "U1");
    b.apply(&set(&["U1"], None)).unwrap();
    let alternate = pins_of(&b, &m, "U1");
    assert_eq!(pin1(&normal), pin1(&alternate), "a pin both bodies share is where it was");
    assert_eq!(pin2(&alternate).y - pin2(&normal).y, 1_270, "pin 2 is one grid cell further down in the alternate body");
    // the other gate, still in the normal style, is where it was
    assert_eq!(pins_of(&b, &m, "U2").iter().find(|(n, _)| n == "2").unwrap().1.y, pin2(&normal).y);
}

#[test]
fn moving_a_symbol_keeps_its_body_style_and_renaming_it_takes_the_style_along() {
    let m = model();
    let mut b = board(&m);
    b.apply(&set(&["U1"], None)).unwrap();
    b.apply(&Cmd::SchMove(crate::sch_move::SchMoveCmd::Move { ids: vec!["U1".into()], dx: 2_540, dy: 0, turns: Vec::new(), about: None })).unwrap();
    assert_eq!(style_of(&b, "U1"), 2, "a move does not change the body style");
    // the style is kept by the symbol's key: the key follows a rename
    b.apply(&Cmd::RenameSymbol { id: "U1".into(), new_id: "U7".into() }).unwrap();
    let styles = &b.design().schematic.as_ref().unwrap().extras.body_styles;
    assert_eq!(styles.get(&field_key("U7", 1)), Some(&2), "{styles:?}");
    assert!(!styles.contains_key("U1"));
    assert_eq!(style_of(&b, "U7"), 2);
    // and a deleted symbol leaves none behind
    b.apply(&Cmd::DeleteSymbol { id: "U7".into(), unit: None }).unwrap();
    assert!(b.design().schematic.as_ref().unwrap().extras.body_styles.is_empty());
}
