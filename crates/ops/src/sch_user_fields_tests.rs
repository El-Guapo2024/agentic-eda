//! Tests for a user-defined field's place on the sheet (P10b step 2): a field of a symbol beyond Reference, Value, Footprint and Datasheet
//! (`SchematicSection::user_fields`) is hidden until it is shown, and then is placed, moved, turned, mirrored, edited and hidden like any other field
//! (`SchCmd::EditField`, `Cmd::SchMove`, `SchCmd::AutoplaceFields`); `SchCmd::AddUserField` and `RemoveUserField` make and take away one, and what is kept
//! under the symbol's reference follows a rename and goes with a delete.
//!
//! The sheet is one this project drew (a symbol sits at the corner of its box), with a resistor `R1` the model has a part for.

use super::*;
use crate::sch_edit::SchCmd;
use crate::sch_move::SchMoveCmd;
use eda_engine::fields_edit::{field_id, OwnerCtx};
use eda_model::ir::{field_key, FieldPlacement, Provenance, SchematicSection, SymbolInstance, TextJustify, TextVAlign};
use eda_model::sch_extras::AutoplaceAlgo;
use eda_model::{Part, Pin, PinKind};
use std::collections::BTreeMap;

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

fn p(x: Nm, y: Nm) -> Point {
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
        body_nm: None,
        edge: None,
    }
}

fn model() -> ConstraintModel {
    ConstraintModel { parts: vec![part("R1"), part("R2")], ..Default::default() }
}

fn resistor(id: &str, at: Point, rot: u32, mirrored: bool) -> SymbolInstance {
    SymbolInstance {
        id: id.into(),
        at,
        rot,
        mirrored,
        mirror_y: false,
        lib_id: "Device:R".into(),
        unit: 1,
        value: "330".into(),
        footprint: String::new(),
        datasheet: String::new(),
        dnp: false,
        exclude_from_bom: false,
        exclude_from_board: false,
        exclude_from_sim: false,
    }
}

/// R1 with one user-defined field, `MPN`.
fn sheet(rot: u32, mirrored: bool) -> SchematicSection {
    let mut sch = SchematicSection { symbols: vec![resistor("R1", p(50_800, 50_800), rot, mirrored)], ..Default::default() };
    sch.user_fields.entry("R1".into()).or_default().insert("MPN".into(), "ERJ-3EKF".into());
    sch
}

fn run(sch: SchematicSection, cmd: Cmd) -> Result<SchematicSection, Vec<CheckResult>> {
    let m = model();
    let mut design = empty_design();
    design.schematic = Some(sch);
    let mut b = Board::new(design, &m, 100, 300);
    b.apply(&cmd)?;
    Ok(b.design().schematic.clone().unwrap())
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn edit(id: &str, f: impl FnOnce(&mut Edit)) -> Cmd {
    let mut e = Edit::default();
    f(&mut e);
    Cmd::SchEdit(SchCmd::EditField {
        id: id.into(),
        text: e.text,
        at: e.at,
        vertical: e.vertical,
        h: e.h,
        v: e.v,
        size_nm: e.size_nm,
        bold: e.bold,
        italic: e.italic,
        visible: e.visible,
        name_shown: e.name_shown,
        allow_autoplace: e.allow_autoplace,
    })
}

#[derive(Default)]
struct Edit {
    text: Option<String>,
    at: Option<Point>,
    vertical: Option<bool>,
    h: Option<TextJustify>,
    v: Option<TextVAlign>,
    size_nm: Option<Nm>,
    bold: Option<bool>,
    italic: Option<bool>,
    visible: Option<bool>,
    name_shown: Option<bool>,
    allow_autoplace: Option<bool>,
}

fn show(sch: SchematicSection, name: &str) -> SchematicSection {
    run(sch, edit(&field_id("R1", name), |e| e.visible = Some(true))).unwrap_or_else(|e| panic!("{e:?}"))
}

fn mv(v: &[&str], dx: Nm, dy: Nm) -> Cmd {
    Cmd::SchMove(SchMoveCmd::Move { ids: ids(v), dx, dy, turns: Vec::new(), about: None })
}

fn rotate(v: &[&str], ccw: bool) -> Cmd {
    Cmd::SchMove(SchMoveCmd::Rotate { ids: ids(v), vertices: BTreeMap::new(), ccw, about: None, grid: 0 })
}

fn mirror(v: &[&str], vertical: bool) -> Cmd {
    Cmd::SchMove(SchMoveCmd::Mirror { ids: ids(v), vertices: BTreeMap::new(), vertical, about: None, grid: 0 })
}

/// The fields of `R1` as they read on the sheet.
fn page(sch: &SchematicSection) -> Vec<eda_engine::fields::PageField> {
    let m = model();
    OwnerCtx::new(sch, &m, "R1").expect("R1 has fields").page_fields(sch)
}

fn field_named(sch: &SchematicSection, name: &str) -> eda_engine::fields::PageField {
    page(sch).into_iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no field {name}"))
}

fn near(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1.0 && (a.1 - b.1).abs() < 1.0
}

fn boxes_overlap(a: &eda_engine::fields::PageField, b: &eda_engine::fields::PageField) -> bool {
    let (x, y) = (eda_engine::fields::field_rect(a), eda_engine::fields::field_rect(b));
    x.x0 < y.x1 && y.x0 < x.x1 && x.y0 < y.y1 && y.y0 < x.y1
}

// --------------------------------------------------------------------------------------------------------------------------- the field

#[test]
fn a_user_defined_field_is_a_field_of_its_symbol_hidden_until_it_is_shown() {
    let start = sheet(0, false);
    let names: Vec<String> = page(&start).into_iter().map(|f| f.name).collect();
    assert_eq!(names, ["Reference", "Value", "Footprint", "Datasheet", "MPN"], "after the four standard fields");
    let mpn = field_named(&start, "MPN");
    assert_eq!(mpn.text, "ERJ-3EKF");
    assert!(!mpn.visible, "a field KiCad adds is hidden (`SetVisible( false )`)");
    // a field named like a standard one, whatever the case, is no field of its own (the file's `value` is its Value)
    let mut odd = sheet(0, false);
    odd.user_fields.get_mut("R1").unwrap().insert("value".into(), "x".into());
    odd.user_fields.get_mut("R1").unwrap().insert("Value".into(), "y".into());
    assert_eq!(page(&odd).iter().filter(|f| f.name.eq_ignore_ascii_case("value")).count(), 1);
    assert_eq!(page(&odd).len(), 5);
}

#[test]
fn a_symbol_with_no_user_fields_has_the_four_it_always_had() {
    let mut plain = sheet(0, false);
    plain.user_fields.clear();
    assert_eq!(page(&plain).len(), 4);
}

#[test]
fn showing_a_user_field_places_it_beside_the_others_and_clear_of_them_while_the_fields_are_autoplaced() {
    let m = model();
    let mut start = sheet(0, false);
    start.extras.fields_autoplaced.insert("R1".into(), AutoplaceAlgo::Auto);
    start.field_layout.insert("R1".into(), OwnerCtx::new(&start, &m, "R1").unwrap().current(&start));
    let after = show(start, "MPN");
    let (r, v, f) = (field_named(&after, "Reference"), field_named(&after, "Value"), field_named(&after, "MPN"));
    assert!(f.visible);
    assert!(!boxes_overlap(&f, &r) && !boxes_overlap(&f, &v), "the three texts are stacked, not on one another: {r:?} {v:?} {f:?}");
    assert_eq!(after.extras.fields_autoplaced.get("R1"), Some(&AutoplaceAlgo::Auto), "still autoplaced");
    // its place is stored with the others
    let stored = &after.field_layout["R1"];
    assert!(stored.iter().any(|p| p.name == "MPN" && p.visible));
    assert_eq!(stored.len(), 5);
}

#[test]
fn hiding_a_shown_user_field_is_delete_on_it() {
    let shown = show(sheet(0, false), "MPN");
    assert!(field_named(&shown, "MPN").visible);
    let hidden = run(shown, edit(&field_id("R1", "MPN"), |e| e.visible = Some(false))).unwrap();
    let f = field_named(&hidden, "MPN");
    assert!(!f.visible, "`DoDelete` on a field hides it");
    assert_eq!(f.text, "ERJ-3EKF", "and keeps its text");
}

// ------------------------------------------------------------------------------------------------------------------ move, turn, mirror

#[test]
fn a_user_field_moves_by_the_offset_whatever_the_symbol_is_turned_to_and_the_other_fields_stay() {
    for rot in [0, 90_000, 180_000, 270_000] {
        for mirrored in [false, true] {
            let before = show(sheet(rot, mirrored), "MPN");
            let (at, reference, value) = (field_named(&before, "MPN").at, field_named(&before, "Reference").at, field_named(&before, "Value").at);
            let after = run(before, mv(&[&field_id("R1", "MPN")], 2_540, -1_270)).unwrap_or_else(|e| panic!("rot {rot} mirrored {mirrored}: {e:?}"));
            let moved = field_named(&after, "MPN").at;
            assert!(near(moved, (at.0 + 2_540.0, at.1 - 1_270.0)), "rot {rot} mirrored {mirrored}: {at:?} -> {moved:?}");
            assert!(near(field_named(&after, "Reference").at, reference) && near(field_named(&after, "Value").at, value), "the others stay");
            assert_eq!(after.symbols[0].at, p(50_800, 50_800), "the symbol did not move");
            assert!(!after.extras.fields_autoplaced.contains_key("R1"), "a field moved on its own is the user's: `SetFieldsAutoplaced( AUTOPLACE_NONE )`");
        }
    }
}

#[test]
fn a_user_field_turns_its_text_a_quarter_and_mirrors_by_its_justification() {
    let start = show(sheet(0, false), "MPN");
    let before = field_named(&start, "MPN");
    assert!(!before.vertical);
    let once = run(start.clone(), rotate(&[&field_id("R1", "MPN")], true)).unwrap();
    let f = field_named(&once, "MPN");
    assert!(f.vertical && near(f.at, before.at), "{f:?}");
    let back = run(once, rotate(&[&field_id("R1", "MPN")], false)).unwrap();
    assert!(!field_named(&back, "MPN").vertical);
    let flipped = run(start, mirror(&[&field_id("R1", "MPN")], false)).unwrap();
    let g = field_named(&flipped, "MPN");
    assert_ne!(g.h, before.h, "left becomes right");
    assert!(near(g.at, before.at));
}

#[test]
fn a_user_field_goes_round_with_its_symbol() {
    let m = model();
    let mut start = show(sheet(0, false), "MPN");
    // placed by hand: its place is in the symbol's own frame and turns with it
    start.extras.fields_autoplaced.clear();
    start.field_layout.insert("R1".into(), OwnerCtx::new(&start, &m, "R1").unwrap().current(&start));
    let (sym, before) = (start.symbols[0].at, field_named(&start, "MPN"));
    let turned = run(start, rotate(&["R1"], true)).unwrap();
    let after = field_named(&turned, "MPN");
    assert!(after.vertical && !before.vertical, "the text turned with the symbol");
    // the offset from the symbol's origin was turned a quarter, as the symbol's own points are (KiCad's `TRANSFORM` for 90: (x, y) goes to (y, -x))
    let (dx, dy) = (before.at.0 - sym.x as f64, before.at.1 - sym.y as f64);
    let at = turned.symbols[0].at;
    assert_eq!(turned.symbols[0].rot, 90_000);
    assert!(near(after.at, (at.x as f64 + dy, at.y as f64 - dx)), "{:?} from {:?}", after.at, (dx, dy));
}

// ----------------------------------------------------------------------------------------------------------------------------- edit

#[test]
fn field_properties_edit_the_text_of_a_user_field_and_it_is_the_symbols_own() {
    let start = sheet(0, false);
    let after = run(start.clone(), edit(&field_id("R1", "MPN"), |e| e.text = Some("RC0603FR-07330RL".into()))).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(after.user_fields["R1"]["MPN"], "RC0603FR-07330RL");
    assert_eq!(field_named(&after, "MPN").text, "RC0603FR-07330RL");
    // a field may be empty (`SCH_FIELD` keeps one with no text)
    let empty = run(start.clone(), edit(&field_id("R1", "MPN"), |e| e.text = Some(String::new()))).unwrap();
    assert_eq!(empty.user_fields["R1"]["MPN"], "");
    // the same text changes nothing
    let e = run(start, edit(&field_id("R1", "MPN"), |e| e.text = Some("ERJ-3EKF".into()))).unwrap_err();
    assert_eq!(e[0].check, "ops_field_unchanged");
}

#[test]
fn field_properties_set_the_look_and_the_place_of_a_user_field_in_one_command() {
    let start = show(sheet(0, false), "MPN");
    let before = field_named(&start, "MPN");
    let after = run(
        start,
        edit(&field_id("R1", "MPN"), |e| {
            e.size_nm = Some(2_000);
            e.bold = Some(true);
            e.italic = Some(true);
            e.name_shown = Some(true);
            e.allow_autoplace = Some(false);
            e.at = Some(p(before.at.0 as i64 + 2_540, before.at.1 as i64 + 2_540));
            e.vertical = Some(true);
            e.h = Some(TextJustify::Right);
        }),
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let f = field_named(&after, "MPN");
    assert!(f.size_nm == 2_000.0 && f.bold && f.italic && f.name_shown && f.no_autoplace && f.vertical, "{f:?}");
    assert_eq!(f.h, eda_model::kicad_font::HJustify::Right);
    assert!(near(f.at, (before.at.0 + 2_540.0, before.at.1 + 2_540.0)), "{:?}", f.at);
    assert_eq!(f.shown_text(), "MPN: ERJ-3EKF");
}

#[test]
fn the_fields_of_a_locked_symbol_include_its_user_fields() {
    let mut start = sheet(0, false);
    start.extras.set_locked("R1", true);
    let e = run(start.clone(), mv(&[&field_id("R1", "MPN")], 1_270, 0)).unwrap_err();
    assert_eq!(e[0].check, "ops_locked");
    let e = run(start, edit(&field_id("R1", "MPN"), |e| e.text = Some("x".into()))).unwrap_err();
    assert_eq!(e[0].check, "ops_locked");
}

#[test]
fn autoplace_fields_places_a_shown_user_field_with_the_others() {
    let m = model();
    let mut start = sheet(0, false);
    start.field_layout.insert("R1".into(), OwnerCtx::new(&start, &m, "R1").unwrap().current(&start));
    let start = show(start, "MPN");
    let placed = run(start, Cmd::SchEdit(SchCmd::AutoplaceFields { ids: ids(&["R1"]) })).unwrap_or_else(|e| panic!("{e:?}"));
    let (r, v, f) = (field_named(&placed, "Reference"), field_named(&placed, "Value"), field_named(&placed, "MPN"));
    assert!(f.visible && !boxes_overlap(&f, &r) && !boxes_overlap(&f, &v), "{r:?} {v:?} {f:?}");
    assert_eq!(placed.extras.fields_autoplaced.get("R1"), Some(&AutoplaceAlgo::Manual));
}

/// `AUTOPLACER` places the fields `GetFields( aVisibleOnly = true )` gives it, and that skips a field with no text: a user field that is shown but empty
/// takes no row of the box, so the Reference and the Value go where they go without it.
#[test]
fn autoplace_fields_leaves_a_shown_field_with_no_text_out_of_the_box() {
    let m = model();
    let mut start = sheet(0, false);
    start.user_fields.get_mut("R1").unwrap().insert("MPN".into(), String::new());
    start.field_layout.insert("R1".into(), OwnerCtx::new(&start, &m, "R1").unwrap().current(&start));
    let without = run(start.clone(), Cmd::SchEdit(SchCmd::AutoplaceFields { ids: ids(&["R1"]) })).unwrap_or_else(|e| panic!("{e:?}"));
    let shown = show(start, "MPN");
    let with = run(shown, Cmd::SchEdit(SchCmd::AutoplaceFields { ids: ids(&["R1"]) })).unwrap_or_else(|e| panic!("{e:?}"));
    for name in ["Reference", "Value"] {
        let (a, b) = (field_named(&without, name), field_named(&with, name));
        assert!(near(a.at, b.at), "{name}: {:?} against {:?}", a.at, b.at);
    }
}

// ----------------------------------------------------------------------------------------------------------------- add, remove, rename

#[test]
fn add_user_field_makes_a_hidden_field_and_refuses_a_name_that_is_taken() {
    let start = sheet(0, false);
    let add = |sch: SchematicSection, name: &str, text: &str| run(sch, Cmd::SchEdit(SchCmd::AddUserField { id: "R1".into(), name: name.into(), text: text.into() }));
    let after = add(start.clone(), " Manufacturer ", "Panasonic").unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(after.user_fields["R1"]["Manufacturer"], "Panasonic", "the name is trimmed");
    let f = field_named(&after, "Manufacturer");
    assert!(!f.visible && f.text == "Panasonic");
    for (name, why) in [("MPN", "the symbol has it"), ("mpn", "whatever the case"), ("Value", "a standard field"), ("reference", "a standard field, whatever the case"), ("  ", "no name"), ("${X}", "a generated name")] {
        let e = add(start.clone(), name, "x").unwrap_err();
        assert!(matches!(e[0].check.as_str(), "ops_field_exists" | "ops_bad_field"), "{why}: {e:?}");
    }
    // KiCad's words: `Validate`'s "Fields must have a name." and `OnGridCellChanging`'s "Field name '%s' already in use."
    assert_eq!(add(start.clone(), "  ", "x").unwrap_err()[0].hint.as_deref(), Some("Fields must have a name."));
    assert_eq!(add(start.clone(), "mpn", "x").unwrap_err()[0].hint.as_deref(), Some("Field name 'mpn' already in use."));
    let e = run(start.clone(), Cmd::SchEdit(SchCmd::AddUserField { id: "R9".into(), name: "A".into(), text: String::new() })).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_item");
    let mut locked = start;
    locked.extras.set_locked("R1", true);
    assert_eq!(add(locked, "Other", "").unwrap_err()[0].check, "ops_locked");
}

#[test]
fn remove_user_field_takes_the_field_and_its_places_and_not_a_standard_one() {
    let start = show(sheet(0, false), "MPN");
    assert!(start.field_layout["R1"].iter().any(|p| p.name == "MPN"));
    let rm = |sch: SchematicSection, name: &str| run(sch, Cmd::SchEdit(SchCmd::RemoveUserField { id: "R1".into(), name: name.into() }));
    let after = rm(start.clone(), "MPN").unwrap_or_else(|e| panic!("{e:?}"));
    assert!(after.user_fields.is_empty(), "the last field of the symbol takes its entry with it");
    assert!(after.field_layout["R1"].iter().all(|p| p.name != "MPN"));
    assert_eq!(page(&after).len(), 4);
    assert_eq!(rm(start.clone(), "Value").unwrap_err()[0].check, "ops_field_mandatory", "\"The first %d fields are mandatory\"");
    assert_eq!(rm(start, "Nothing").unwrap_err()[0].check, "ops_unknown_item");
}

#[test]
fn a_renamed_symbol_keeps_its_user_fields_and_where_they_are() {
    let m = model();
    let mut start = show(sheet(0, false), "MPN");
    start.extras.fields_autoplaced.clear();
    start.field_layout.insert("R1".into(), OwnerCtx::new(&start, &m, "R1").unwrap().current(&start));
    let after = run(start, Cmd::RenameSymbol { id: "R1".into(), new_id: "R9".into() }).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(after.user_fields["R9"]["MPN"], "ERJ-3EKF");
    assert!(after.field_layout["R9"].iter().any(|p| p.name == "MPN" && p.visible));
    assert!(!after.user_fields.contains_key("R1") && !after.field_layout.contains_key("R1"));
}

#[test]
fn deleting_the_symbol_takes_its_user_fields_and_their_places_with_it() {
    let start = show(sheet(0, false), "MPN");
    let after = run(start, Cmd::DeleteSymbol { id: "R1".into(), unit: None }).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(after.user_fields.is_empty() && !after.field_layout.contains_key("R1"));
}

#[test]
fn the_fields_table_removing_a_column_takes_its_places_and_renaming_one_keeps_them() {
    use crate::fields_table::{apply_field_changes, FieldChanges, FieldRename};
    let shown = show(sheet(0, false), "MPN");
    assert!(shown.field_layout["R1"].iter().any(|p| p.name == "MPN" && p.visible));
    let mut renamed = shown.clone();
    apply_field_changes(&mut renamed, &FieldChanges { rename_fields: vec![FieldRename { from: "MPN".into(), to: "Part".into() }], ..Default::default() }).unwrap();
    assert_eq!(renamed.user_fields["R1"]["Part"], "ERJ-3EKF");
    let f = field_named(&renamed, "Part");
    assert!(f.visible, "it is shown where it was: {f:?}");
    assert!(renamed.field_layout["R1"].iter().all(|p| p.name != "MPN"));
    let mut removed = shown;
    apply_field_changes(&mut removed, &FieldChanges { remove_fields: vec!["MPN".into()], ..Default::default() }).unwrap();
    assert!(removed.user_fields.is_empty());
    assert!(removed.field_layout["R1"].iter().all(|p| p.name != "MPN"));
}

#[test]
fn a_user_field_added_by_the_table_after_the_fields_were_placed_is_hidden_and_the_others_stay() {
    use crate::fields_table::{apply_field_changes, FieldChanges};
    let m = model();
    let mut start = SchematicSection { symbols: vec![resistor("R1", p(50_800, 50_800), 0, false)], ..Default::default() };
    start.field_layout.insert("R1".into(), OwnerCtx::new(&start, &m, "R1").unwrap().current(&start));
    let (r, v) = (field_named(&start, "Reference"), field_named(&start, "Value"));
    apply_field_changes(&mut start, &FieldChanges { add_fields: vec!["MPN".into()], ..Default::default() }).unwrap();
    let f = field_named(&start, "MPN");
    assert!(!f.visible && f.text.is_empty());
    assert!(near(field_named(&start, "Reference").at, r.at) && near(field_named(&start, "Value").at, v.at), "the fields placed before are where they were");
}

// ------------------------------------------------------------------------------------------------------------------ a sheet read from a file

#[test]
fn a_sheet_read_from_a_kicad_file_keeps_its_user_fields_but_has_no_places_to_edit() {
    let mut start = sheet(0, false);
    start.imported_from_kicad = true;
    let e = run(start.clone(), edit(&field_id("R1", "MPN"), |e| e.visible = Some(true))).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_item");
    // adding and removing a field is the symbol's own, whatever drew it
    let after = run(start, Cmd::SchEdit(SchCmd::AddUserField { id: "R1".into(), name: "Other".into(), text: "x".into() })).unwrap();
    assert_eq!(after.user_fields["R1"].len(), 2);
}

#[test]
fn a_stored_placement_of_a_user_field_is_where_the_sheet_draws_it() {
    let mut start = sheet(0, false);
    start.field_layout.insert("R1".into(), vec![FieldPlacement { dx: 3_000, dy: -7_000, visible: true, h: TextJustify::Left, ..FieldPlacement::at_origin("MPN") }]);
    let f = field_named(&start, "MPN");
    assert!(f.visible);
    assert!(near(f.at, (50_800.0 + 3_000.0, 50_800.0 - 7_000.0)), "{:?}", f.at);
    // the standard fields have no stored place: Autoplace Fields' (live)
    assert_eq!(field_key("R1", 1), "R1");
    assert!(field_named(&start, "Reference").visible);
}
