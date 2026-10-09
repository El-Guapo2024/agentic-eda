//! Tests for the fields of a symbol as items: moved, turned, mirrored and edited one by one (`Cmd::SchMove`, `SchCmd::EditField`), and placed again by
//! Autoplace Fields (`SchCmd::AutoplaceFields`, and the automatic one that follows a turn of the symbol).
//!
//! The sheet is one this project drew (a symbol sits at the corner of its box), with a resistor `R1` the model has a part for.

use super::*;
use crate::sch_edit::SchCmd;
use crate::sch_move::SchMoveCmd;
use eda_engine::fields_edit::{field_id, OwnerCtx};
use eda_model::ir::{field_key, FieldPlacement, PowerSymbol, Provenance, SchematicSection, SheetInstance, SymbolInstance, TextJustify, TextVAlign, Wire};
use eda_model::sch_extras::AutoplaceAlgo;
use eda_model::{Part, Pin, PinKind};
use std::collections::BTreeMap;

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

fn p(x: Um, y: Um) -> Point {
    Point { x, y }
}

fn part(reference: &str) -> Part {
    Part {
        reference: reference.into(),
        mpn: None,
        lcsc: None,
        value: Some("330".into()),
        package: Some("0603".into()),
        footprint: None,
        symbol: None,
        datasheet: None,
        pins: (1..=2).map(|i| Pin { number: i.to_string(), name: None, kind: PinKind::Passive }).collect(),
        body_um: None,
        edge: None,
    }
}

fn model() -> ConstraintModel {
    ConstraintModel { parts: vec![part("R1"), part("R2")], ..Default::default() }
}

fn resistor(id: &str, at: Point, rot: u32, mirrored: bool) -> SymbolInstance {
    SymbolInstance { id: id.into(), at, rot, mirrored, mirror_y: false, lib_id: "Device:R".into(), unit: 1, value: "330".into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false }
}

fn sheet(rot: u32, mirrored: bool) -> SchematicSection {
    SchematicSection { symbols: vec![resistor("R1", p(50_800, 50_800), rot, mirrored)], ..Default::default() }
}

fn board(sch: SchematicSection, m: &ConstraintModel) -> Board<'_> {
    let mut design = empty_design();
    design.schematic = Some(sch);
    Board::new(design, m, 100, 300)
}

fn run(sch: SchematicSection, cmd: Cmd) -> Result<SchematicSection, Vec<CheckResult>> {
    let m = model();
    let mut b = board(sch, &m);
    b.apply(&cmd)?;
    Ok(b.design().schematic.clone().unwrap())
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn mv(v: &[&str], dx: Um, dy: Um) -> Cmd {
    Cmd::SchMove(SchMoveCmd::Move { ids: ids(v), dx, dy, turns: Vec::new(), about: None })
}

fn rotate(v: &[&str], ccw: bool) -> Cmd {
    Cmd::SchMove(SchMoveCmd::Rotate { ids: ids(v), vertices: BTreeMap::new(), ccw, about: None, grid: 0 })
}

fn mirror(v: &[&str], vertical: bool) -> Cmd {
    Cmd::SchMove(SchMoveCmd::Mirror { ids: ids(v), vertices: BTreeMap::new(), vertical, about: None, grid: 0 })
}

fn edit(id: &str, f: impl FnOnce(&mut Edit)) -> Cmd {
    let mut e = Edit::default();
    f(&mut e);
    Cmd::SchEdit(SchCmd::EditField { id: id.into(), text: e.text, at: e.at, vertical: e.vertical, h: e.h, v: e.v, size_um: e.size_um, bold: e.bold, italic: e.italic, visible: e.visible, name_shown: e.name_shown, allow_autoplace: e.allow_autoplace })
}

#[derive(Default)]
struct Edit {
    text: Option<String>,
    at: Option<Point>,
    vertical: Option<bool>,
    h: Option<TextJustify>,
    v: Option<TextVAlign>,
    size_um: Option<Um>,
    bold: Option<bool>,
    italic: Option<bool>,
    visible: Option<bool>,
    name_shown: Option<bool>,
    allow_autoplace: Option<bool>,
}

/// The fields of `R1` as they read on the sheet.
fn page(sch: &SchematicSection) -> Vec<eda_engine::fields::PageField> {
    let m = model();
    OwnerCtx::new(sch, &m, "R1").expect("R1 has fields").page_fields(sch)
}

fn field_named(sch: &SchematicSection, name: &str) -> eda_engine::fields::PageField {
    page(sch).into_iter().find(|f| f.name == name).expect("the field")
}

fn near(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1.0 && (a.1 - b.1).abs() < 1.0
}

// ------------------------------------------------------------------------------------------------------------------------------- move

#[test]
fn a_field_moves_by_the_offset_whatever_the_symbol_is_turned_to_and_the_other_fields_stay() {
    for rot in [0, 90_000, 180_000, 270_000] {
        for mirrored in [false, true] {
            let before = sheet(rot, mirrored);
            let reference = field_named(&before, "Reference").at;
            let value = field_named(&before, "Value").at;
            let after = run(before, mv(&[&field_id("R1", "Reference")], 2_540, -1_270)).unwrap_or_else(|e| panic!("{e:?}"));
            let moved = field_named(&after, "Reference").at;
            assert!(near(moved, (reference.0 + 2_540.0, reference.1 - 1_270.0)), "rot {rot} mirrored {mirrored}: {reference:?} -> {moved:?}");
            assert!(near(field_named(&after, "Value").at, value), "the Value stays");
            // the symbol did not move
            assert_eq!(after.symbols[0].at, p(50_800, 50_800));
        }
    }
}

#[test]
fn a_field_moved_on_its_own_is_no_longer_autoplaced_but_moving_its_symbol_keeps_the_fields_with_it() {
    let mut start = sheet(0, false);
    start.extras.fields_autoplaced.insert("R1".into(), AutoplaceAlgo::Auto);
    let moved_field = run(start.clone(), mv(&[&field_id("R1", "Value")], 1_270, 0)).unwrap();
    assert!(!moved_field.extras.fields_autoplaced.contains_key("R1"), "SetFieldsAutoplaced( AUTOPLACE_NONE )");
    assert!(moved_field.field_layout.contains_key("R1"), "its fields are now stored");

    let before = field_named(&start, "Reference").at;
    let moved_symbol = run(start, mv(&["R1"], 2_540, 2_540)).unwrap();
    assert!(near(field_named(&moved_symbol, "Reference").at, (before.0 + 2_540.0, before.1 + 2_540.0)), "the fields go with the symbol");
    assert_eq!(moved_symbol.extras.fields_autoplaced.get("R1"), Some(&AutoplaceAlgo::Auto), "and are still autoplaced");
}

#[test]
fn a_field_whose_symbol_is_moved_with_it_does_not_move_twice() {
    let start = sheet(0, false);
    let before = field_named(&start, "Reference").at;
    let after = run(start, mv(&["R1", &field_id("R1", "Reference")], 2_540, 0)).unwrap();
    assert!(near(field_named(&after, "Reference").at, (before.0 + 2_540.0, before.1)), "once, not twice: {:?}", field_named(&after, "Reference").at);
}

#[test]
fn the_fields_of_a_locked_symbol_stay_where_they_are() {
    let mut start = sheet(0, false);
    start.extras.set_locked("R1", true);
    let e = run(start, mv(&[&field_id("R1", "Value")], 1_270, 0)).unwrap_err();
    assert_eq!(e[0].check, "ops_locked");
}

#[test]
fn a_field_that_is_not_there_is_refused() {
    let e = run(sheet(0, false), mv(&[&field_id("R1", "Nothing")], 1_270, 0)).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_item");
    let e = run(sheet(0, false), mv(&[&field_id("R9", "Value")], 1_270, 0)).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_item");
}

// ----------------------------------------------------------------------------------------------------------------------- rotate, mirror

#[test]
fn a_lone_field_turns_its_text_a_quarter_where_it_stands_and_back() {
    let start = sheet(0, false);
    let before = field_named(&start, "Value");
    assert!(!before.vertical);
    let once = run(start, rotate(&[&field_id("R1", "Value")], true)).unwrap();
    let f = field_named(&once, "Value");
    assert!(f.vertical, "{f:?}");
    assert!(near(f.at, before.at), "the anchor does not move");
    let twice = run(once, rotate(&[&field_id("R1", "Value")], false)).unwrap();
    assert!(!field_named(&twice, "Value").vertical);
}

#[test]
fn two_fields_turn_about_the_middle_of_the_selection_and_swap_the_side_they_read_from() {
    let start = sheet(0, false);
    let r = field_named(&start, "Reference");
    let v = field_named(&start, "Value");
    let after = run(start, rotate(&[&field_id("R1", "Reference"), &field_id("R1", "Value")], true)).unwrap();
    let (r2, v2) = (field_named(&after, "Reference"), field_named(&after, "Value"));
    assert!(r2.vertical && v2.vertical, "each text turned a quarter");
    // the anchors went round one point: the distance between them is the same
    let d = |a: (f64, f64), b: (f64, f64)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
    assert!((d(r.at, v.at) - d(r2.at, v2.at)).abs() < 2.0);
    assert!(!near(r2.at, r.at));
}

#[test]
fn mirroring_a_field_flips_its_justification_and_nothing_else() {
    let start = sheet(0, false);
    let f = field_named(&start, "Value");
    assert_eq!(f.h, eda_model::kicad_font::HJustify::Left);
    let after = run(start.clone(), mirror(&[&field_id("R1", "Value")], false)).unwrap();
    let g = field_named(&after, "Value");
    assert_eq!(g.h, eda_model::kicad_font::HJustify::Right);
    assert!(near(g.at, f.at) && g.v == f.v);
    // top to bottom flips the vertical justification (a centred one stays)
    let down = run(start, mirror(&[&field_id("R1", "Value")], true)).unwrap();
    assert_eq!(field_named(&down, "Value").v, eda_model::kicad_font::VJustify::Center);
}

#[test]
fn turning_a_symbol_places_its_autoplaced_fields_again_but_carries_the_others_round_with_it() {
    let m = model();
    // autoplaced: the fields read horizontally beside the turned symbol
    let mut start = sheet(0, false);
    start.extras.fields_autoplaced.insert("R1".into(), AutoplaceAlgo::Auto);
    let turned = run(start, rotate(&["R1"], true)).unwrap();
    assert!(!field_named(&turned, "Reference").vertical, "Autoplace Fields keeps the text horizontal");
    let ctx = OwnerCtx::new(&turned, &m, "R1").unwrap();
    let again = ctx.autoplace(&turned, &m, AutoplaceAlgo::Auto);
    assert_eq!(turned.field_layout["R1"], again, "exactly what Autoplace Fields gives the turned symbol");
    // not autoplaced: the placements are in the symbol's own frame and turn with it
    let mut manual = sheet(0, false);
    manual.field_layout.insert("R1".into(), OwnerCtx::new(&manual, &m, "R1").unwrap().current(&manual));
    let before = field_named(&manual, "Reference");
    let turned = run(manual, rotate(&["R1"], true)).unwrap();
    let after = field_named(&turned, "Reference");
    assert!(after.vertical && !before.vertical, "the text turned with the symbol");
}

#[test]
fn mirroring_a_symbol_takes_it_out_of_the_autoplaced_ones() {
    let mut start = sheet(0, false);
    start.extras.fields_autoplaced.insert("R1".into(), AutoplaceAlgo::Auto);
    let after = run(start, mirror(&["R1"], false)).unwrap();
    assert!(!after.extras.fields_autoplaced.contains_key("R1"));
}

// ------------------------------------------------------------------------------------------------------------------------------- edit

#[test]
fn field_properties_set_the_text_size_look_and_place_in_one_command() {
    let start = sheet(0, false);
    let before = field_named(&start, "Value");
    let after = run(start, edit(&field_id("R1", "Value"), |e| {
        e.text = Some("470".into());
        e.size_um = Some(2_000);
        e.bold = Some(true);
        e.italic = Some(true);
        e.at = Some(p(before.at.0 as i64 + 2_540, before.at.1 as i64 + 2_540));
        e.vertical = Some(true);
        e.h = Some(TextJustify::Right);
    }))
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(after.symbols[0].value, "470");
    let f = field_named(&after, "Value");
    assert_eq!(f.text, "470");
    assert!(f.size_um == 2_000.0 && f.bold && f.italic && f.vertical);
    assert_eq!(f.h, eda_model::kicad_font::HJustify::Right);
    assert!(near(f.at, (before.at.0 + 2_540.0, before.at.1 + 2_540.0)), "{:?}", f.at);
}

#[test]
fn showing_a_hidden_field_puts_it_beside_the_others_while_the_fields_are_autoplaced() {
    let mut start = sheet(0, false);
    start.extras.fields_autoplaced.insert("R1".into(), AutoplaceAlgo::Auto);
    let m = model();
    start.field_layout.insert("R1".into(), OwnerCtx::new(&start, &m, "R1").unwrap().current(&start));
    let hidden = field_named(&start, "Footprint");
    assert!(!hidden.visible);
    let after = run(start, edit(&field_id("R1", "Footprint"), |e| e.visible = Some(true))).unwrap();
    let f = field_named(&after, "Footprint");
    assert!(f.visible);
    // three fields now stack; the Footprint is not where the Reference is
    assert!(!near(f.at, field_named(&after, "Reference").at));
    assert!(!near(f.at, field_named(&after, "Value").at));
    assert_eq!(after.extras.fields_autoplaced.get("R1"), Some(&AutoplaceAlgo::Auto), "still autoplaced");
}

#[test]
fn a_field_command_that_changes_nothing_is_refused_so_it_leaves_no_undo_step() {
    let start = sheet(0, false);
    let now = field_named(&start, "Value");
    let e = run(start.clone(), edit(&field_id("R1", "Value"), |e| e.text = Some(now.text.clone()))).unwrap_err();
    assert_eq!(e[0].check, "ops_field_unchanged");
    let e = run(start, edit(&field_id("R1", "Value"), |e| e.size_um = Some(0))).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_text_size");
}

#[test]
fn renaming_a_symbol_from_its_reference_field_keeps_where_its_fields_are() {
    let m = ConstraintModel { parts: vec![part("R1"), part("R7")], ..Default::default() };
    let mut start = sheet(0, false);
    start.field_layout.insert("R1".into(), OwnerCtx::new(&start, &m, "R1").unwrap().current(&start));
    start.extras.fields_autoplaced.insert("R1".into(), AutoplaceAlgo::Auto);
    let mut b = board(start, &m);
    b.apply(&edit(&field_id("R1", "Reference"), |e| e.text = Some("R7".into()))).unwrap_or_else(|e| panic!("{e:?}"));
    let sch = b.design().schematic.clone().unwrap();
    assert_eq!(sch.symbols[0].id, "R7");
    assert!(sch.field_layout.contains_key("R7") && !sch.field_layout.contains_key("R1"));
    assert!(sch.extras.fields_autoplaced.contains_key("R7") && !sch.extras.fields_autoplaced.contains_key("R1"));
}

#[test]
fn a_power_symbols_value_and_a_sheets_name_are_edited_like_any_field() {
    let m = model();
    let mut start = SchematicSection::default();
    start.power_symbols.push(PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: p(10_000, 10_000), rot: 0, net: "GND".into(), pin: "R1.2".into() });
    start.sheets.push(SheetInstance { id: "sheet_a".into(), name: "Sub".into(), file: "sub.kicad_sch".into(), at: p(100_000, 80_000), size: (40_000, 30_000), pins: vec![], page: String::new() });
    let mut b = board(start, &m);
    // the Value of a ground symbol sits below it; moved to the right it stays there
    b.apply(&mv(&[&field_id("#PWR01", "Value")], 5_080, 0)).unwrap_or_else(|e| panic!("{e:?}"));
    let ctx = OwnerCtx::new(b.design().schematic.as_ref().unwrap(), &m, "#PWR01").unwrap();
    let f = ctx.page_fields(b.design().schematic.as_ref().unwrap());
    assert!(near(f[1].at, (15_080.0, 13_810.0)), "{:?}", f[1].at);
    // the sheet's name is edited from its field
    b.apply(&edit(&field_id("sheet_a", "Sheetname"), |e| e.text = Some("Power".into()))).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(b.design().schematic.as_ref().unwrap().sheets[0].name, "Power");
    // and the file name can be shown beside its name
    b.apply(&edit(&field_id("sheet_a", "Sheetfile"), |e| e.name_shown = Some(true))).unwrap_or_else(|e| panic!("{e:?}"));
    let sch = b.design().schematic.clone().unwrap();
    let ctx = OwnerCtx::new(&sch, &m, "sheet_a").unwrap();
    assert!(ctx.page_fields(&sch)[1].name_shown);
}

// ------------------------------------------------------------------------------------------------------------------------- autoplace

#[test]
fn autoplace_fields_places_the_fields_again_and_keeps_them_autoplaced() {
    let m = model();
    let mut start = sheet(0, false);
    // the user dragged the Value far off
    start = run(start, mv(&[&field_id("R1", "Value")], 20_320, 20_320)).unwrap();
    assert!(start.extras.fields_autoplaced.is_empty());
    let off = field_named(&start, "Value").at;
    let after = run(start, Cmd::SchEdit(SchCmd::AutoplaceFields { ids: ids(&["R1"]) })).unwrap_or_else(|e| panic!("{e:?}"));
    let ctx = OwnerCtx::new(&after, &m, "R1").unwrap();
    assert_eq!(after.field_layout["R1"], ctx.autoplace(&after, &m, AutoplaceAlgo::Manual));
    assert_eq!(after.extras.fields_autoplaced.get("R1"), Some(&AutoplaceAlgo::Manual));
    assert!(!near(field_named(&after, "Value").at, off), "the Value came back beside the symbol");
    // placed already: nothing to do, so nothing to undo
    let e = run(after, Cmd::SchEdit(SchCmd::AutoplaceFields { ids: ids(&["R1"]) })).unwrap_err();
    assert_eq!(e[0].check, "ops_fields_unchanged");
}

#[test]
fn autoplace_fields_on_a_field_places_the_fields_of_its_symbol_and_skips_a_field_that_does_not_allow_it() {
    let start = sheet(0, false);
    let kept = field_named(&start, "Value").at;
    // the Value does not allow autoplacement
    let start = run(start, edit(&field_id("R1", "Value"), |e| e.allow_autoplace = Some(false))).unwrap();
    let start = run(start, mv(&[&field_id("R1", "Reference")], 25_400, 25_400)).unwrap();
    let after = run(start, Cmd::SchEdit(SchCmd::AutoplaceFields { ids: ids(&[&field_id("R1", "Reference")]) })).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(near(field_named(&after, "Value").at, kept), "the Value stayed");
    assert!(field_named(&after, "Reference").at.0 < 80_000.0, "the Reference came back");
}

#[test]
fn autoplace_fields_keeps_the_fields_off_a_wire_beside_the_symbol() {
    let m = model();
    let mut start = sheet(0, false);
    start.wires.push(Wire { id: "w1".into(), net: "N".into(), pins: vec![], pts: vec![p(56_000, 30_000), p(56_000, 80_000)], bus: false });
    let after = run(start, Cmd::SchEdit(SchCmd::AutoplaceFields { ids: ids(&["R1"]) })).unwrap_or_else(|e| panic!("{e:?}"));
    let _ = &m;
    let at = field_named(&after, "Reference").at;
    assert!(at.0 < 50_800.0, "the fields leave the side the wire crosses: {at:?}");
}

#[test]
fn deleting_a_symbol_takes_its_field_layout_with_it() {
    let m = model();
    let mut start = sheet(0, false);
    start.field_layout.insert("R1".into(), OwnerCtx::new(&start, &m, "R1").unwrap().current(&start));
    start.extras.fields_autoplaced.insert("R1".into(), AutoplaceAlgo::Auto);
    let mut b = board(start, &m);
    b.apply(&Cmd::DeleteSymbol { id: "R1".into(), unit: None }).unwrap_or_else(|e| panic!("{e:?}"));
    let sch = b.design().schematic.clone().unwrap();
    assert!(!sch.field_layout.contains_key(&field_key("R1", 1)) && sch.extras.fields_autoplaced.is_empty());
    let _ = FieldPlacement::at_origin("x");
}
