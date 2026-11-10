//! Tests for Symbol Properties' grid columns and General box beyond the texts (UI-LEFT-2 SS7; `DIALOG_SYMBOL_PROPERTIES`, `FIELDS_GRID_TABLE` at 8303b2ad):
//! the X / Y Position and Orientation columns (`SchCmd::EditField`'s `local_at`, `local_vertical`), the dialog's edits leaving the fields autoplaced
//! (`keep_autoplaced`) until the step's `AutoplaceFieldsKept` places them again as `SCH_EDIT_TOOL::Properties` does after OK, Move Up / Move Down
//! (`ReorderFields`), the Angle and Mirror choices (`SetSymbolOrientation`), and a power symbol's user fields as items of the sheet.
//!
//! The sheet is one this project drew (a symbol sits at the corner of its box), with a resistor `R1` the model has a part for.

use super::*;
use crate::sch_edit::SchCmd;
use eda_engine::fields_edit::{field_id, OwnerCtx};
use eda_model::ir::{PowerSymbol, Provenance, SchematicSection, SymbolInstance};
use eda_model::sch_extras::{AutoplaceAlgo, LabelField, PowerProps};
use eda_model::units::mm;
use eda_model::{Part, Pin, PinKind};

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

fn model() -> ConstraintModel {
    let part = Part {
        reference: "R1".into(),
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
    };
    ConstraintModel { parts: vec![part], ..Default::default() }
}

fn resistor() -> SymbolInstance {
    SymbolInstance { id: "R1".into(), at: p(mm(50.8), mm(50.8)), rot: 0, mirrored: false, mirror_y: false, lib_id: "Device:R".into(), unit: 1, value: "330".into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false }
}

/// R1 with three user fields (`MPN`, `Note`, `Tol`) and a GND power symbol with two (`Rating`, shown, and `Spare`, hidden).
fn sheet() -> SchematicSection {
    let mut sch = SchematicSection { symbols: vec![resistor()], ..Default::default() };
    sch.user_fields.insert("R1".into(), [("MPN", "ERJ-3"), ("Note", "n"), ("Tol", "1%")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect());
    sch.power_symbols.push(PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: p(mm(101.6), mm(50.8)), rot: 0, net: "GND".into(), pin: String::new() });
    sch.extras.power_props.insert(
        "#PWR01".into(),
        PowerProps { fields: vec![LabelField { name: "Rating".into(), text: "1A".into(), visible: true, italic: false, ..Default::default() }, LabelField { name: "Spare".into(), text: "x".into(), visible: false, italic: false, ..Default::default() }], ..Default::default() },
    );
    sch
}

fn run_on(sch: SchematicSection, cmds: Vec<Cmd>) -> Result<SchematicSection, Vec<CheckResult>> {
    let m = model();
    let mut design = empty_design();
    design.schematic = Some(sch);
    let mut b = Board::new(design, &m, mm(0.1), mm(0.3));
    for c in &cmds {
        b.apply(c)?;
    }
    Ok(b.design().schematic.clone().unwrap())
}

fn edit_field(id: &str, local_at: Option<Point>, local_vertical: Option<bool>, keep_autoplaced: bool) -> Cmd {
    Cmd::SchEdit(SchCmd::EditField {
        id: id.into(),
        text: None,
        at: None,
        vertical: None,
        h: None,
        v: None,
        size_nm: None,
        bold: None,
        italic: None,
        visible: None,
        name_shown: None,
        allow_autoplace: None,
        local_at,
        local_vertical,
        keep_autoplaced,
    })
}

/// The stored placement of `name` of the item `key`, as the editor reads it.
fn placement(sch: &SchematicSection, key: &str, name: &str) -> eda_model::ir::FieldPlacement {
    let m = model();
    let ctx = OwnerCtx::new(sch, &m, key).expect("the item");
    let i = ctx.specs.iter().position(|s| s.name == name).expect("the field");
    ctx.current(sch)[i].clone()
}

fn spec_names(sch: &SchematicSection, key: &str) -> Vec<String> {
    let m = model();
    OwnerCtx::new(sch, &m, key).unwrap().specs.iter().map(|s| s.name.clone()).collect()
}

#[test]
fn the_x_y_position_and_orientation_columns_set_the_field_in_the_symbols_own_frame() {
    // `FDC_POSX` / `FDC_POSY`: `SetTextPos` of the offset from the symbol; `FDC_ORIENTATION`: `SetTextAngle( ANGLE_VERTICAL )`, the justification kept
    let start = sheet();
    let was = placement(&start, "R1", "Value");
    let after = run_on(start, vec![edit_field(&field_id("R1", "Value"), Some(p(mm(1.27), mm(-2.54))), Some(true), false)]).unwrap();
    let now = placement(&after, "R1", "Value");
    assert_eq!((now.dx, now.dy, now.angle), (mm(1.27), mm(-2.54), 90_000));
    assert_eq!((now.h, now.v), (was.h, was.v), "the field keeps its own justification");
    // Field Properties' rule for a move does not apply to the grid's columns given with `keep_autoplaced`: they are the dialog's
    let mut autoplaced = sheet();
    autoplaced.extras.fields_autoplaced.insert("R1".into(), AutoplaceAlgo::Auto);
    let kept = run_on(autoplaced, vec![edit_field(&field_id("R1", "Value"), Some(p(mm(1.27), mm(-2.54))), None, true)]).unwrap();
    assert_eq!(kept.extras.fields_autoplaced.get("R1"), Some(&AutoplaceAlgo::Auto), "Symbol Properties leaves the fields autoplaced");
    assert_eq!((placement(&kept, "R1", "Value").dx, placement(&kept, "R1", "Value").dy), (mm(1.27), mm(-2.54)));
}

#[test]
fn ok_places_the_fields_of_an_autoplaced_symbol_again_and_leaves_a_hand_placed_one() {
    // `SCH_EDIT_TOOL::Properties`: `if( fieldsAutoplaced == AUTOPLACE_AUTO || fieldsAutoplaced == AUTOPLACE_MANUAL ) symbol->AutoplaceFields( screen, fieldsAutoplaced )`
    let mut autoplaced = sheet();
    autoplaced.extras.fields_autoplaced.insert("R1".into(), AutoplaceAlgo::Auto);
    let home = placement(&autoplaced, "R1", "Value");
    let moved_then_placed = run_on(
        autoplaced,
        vec![Cmd::Batch { cmds: vec![edit_field(&field_id("R1", "Value"), Some(p(mm(20.0), mm(20.0))), None, true), Cmd::SchEdit(SchCmd::AutoplaceFieldsKept { keys: vec!["R1".into()] })] }],
    )
    .unwrap();
    let v = placement(&moved_then_placed, "R1", "Value");
    assert_eq!((v.dx, v.dy), (home.dx, home.dy), "the Value went back where Autoplace Fields puts it");
    // a symbol whose fields were placed by hand keeps the place typed in
    let by_hand = run_on(sheet(), vec![Cmd::Batch { cmds: vec![edit_field(&field_id("R1", "Value"), Some(p(mm(20.0), mm(20.0))), None, true), Cmd::SchEdit(SchCmd::AutoplaceFieldsKept { keys: vec!["R1".into()] })] }]).unwrap();
    let v = placement(&by_hand, "R1", "Value");
    assert_eq!((v.dx, v.dy), (mm(20.0), mm(20.0)));
    // nothing to place is no error (the dialog's batch is the step)
    assert!(run_on(sheet(), vec![Cmd::SchEdit(SchCmd::AutoplaceFieldsKept { keys: vec!["R1".into()] })]).is_ok());
}

#[test]
fn move_up_and_down_order_the_user_fields_after_the_mandatory_ones() {
    // `OnMoveUp` / `OnMoveDown` and `SetOrdinal( ordinal++ )`: the order is the fields', listed, autoplaced and written so
    let start = sheet();
    assert_eq!(spec_names(&start, "R1"), ["Reference", "Value", "Footprint", "Datasheet", "MPN", "Note", "Tol"]);
    let reorder = |names: &[&str]| Cmd::SchEdit(SchCmd::ReorderFields { key: "R1".into(), names: names.iter().map(|n| n.to_string()).collect() });
    let after = run_on(start.clone(), vec![reorder(&["Tol", "MPN", "Note"])]).unwrap();
    assert_eq!(spec_names(&after, "R1"), ["Reference", "Value", "Footprint", "Datasheet", "Tol", "MPN", "Note"]);
    // a mandatory field named in the list stays first; a field the list leaves out comes after the named ones
    let after = run_on(start.clone(), vec![reorder(&["Note", "Reference"])]).unwrap();
    assert_eq!(spec_names(&after, "R1"), ["Reference", "Value", "Footprint", "Datasheet", "Note", "MPN", "Tol"]);
    // the order goes into the file the writer writes, the same as the specs
    let e = run_on(start.clone(), vec![reorder(&["MPN", "Note", "Tol"])]).unwrap_err();
    assert_eq!(e[0].check, "ops_nothing_to_change");
    // a new field comes after the ordered ones
    let added = run_on(after, vec![Cmd::SchEdit(SchCmd::AddUserField { id: "R1".into(), name: "Aaa".into(), text: "a".into() })]).unwrap();
    assert_eq!(spec_names(&added, "R1").last().map(String::as_str), Some("Aaa"));
}

#[test]
fn angle_and_mirror_set_the_orientation_whatever_it_was() {
    // `SetOrientation( SYM_ORIENT_270 )` then `SetOrientation( SYM_MIRROR_X )`, read back as `GetOrientation` reads it
    let orient = |angle: u32, mirror: Option<&str>| Cmd::SchEdit(SchCmd::SetSymbolOrientation { id: "R1".into(), unit: None, angle, mirror: mirror.map(String::from) });
    let after = run_on(sheet(), vec![orient(270, Some("x"))]).unwrap();
    let s = &after.symbols[0];
    let want = {
        use crate::sch_scene::Matrix;
        Matrix::of(270_000, false, false).then(Matrix::MIRROR_X).orientation()
    };
    assert_eq!((s.rot, s.mirrored, s.mirror_y), want);
    assert_eq!(s.at, p(mm(50.8), mm(50.8)), "the symbol keeps its place");
    // again from there: absolute, not a turn more
    let again = run_on(after.clone(), vec![orient(90, None)]).unwrap();
    assert_eq!((again.symbols[0].rot, again.symbols[0].mirrored, again.symbols[0].mirror_y), (90_000, false, false));
    assert_eq!(run_on(again, vec![orient(90, None)]).unwrap_err()[0].check, "ops_nothing_to_change");
    assert_eq!(run_on(sheet(), vec![orient(45, None)]).unwrap_err()[0].check, "ops_bad_orientation");
    // a power symbol folds a mirror into a turn: "Around X axis" of a ground symbol is the symbol upside down
    let power = run_on(sheet(), vec![Cmd::SchEdit(SchCmd::SetSymbolOrientation { id: "#PWR01".into(), unit: None, angle: 0, mirror: Some("x".into()) })]).unwrap();
    assert_eq!(power.power_symbols[0].rot, 180_000);
    // a locked symbol keeps its orientation
    let mut locked = sheet();
    locked.extras.set_locked("R1", true);
    assert_eq!(run_on(locked, vec![orient(90, None)]).unwrap_err()[0].check, "ops_locked");
}

#[test]
fn a_power_symbols_user_fields_are_items_drawn_where_autoplace_puts_them_and_edited_like_a_symbols() {
    let start = sheet();
    let fields = eda_engine::fields::power_fields(&start, &start.power_symbols[0]);
    let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["Reference", "Value", "Rating", "Spare"]);
    let value = fields.iter().find(|f| f.name == "Value").unwrap();
    let rating = fields.iter().find(|f| f.name == "Rating").unwrap();
    assert!(rating.visible && !fields.iter().find(|f| f.name == "Spare").unwrap().visible);
    // stacked with the Value (`AUTOPLACER` places every shown field): not on the Value, not at the anchor
    assert!((rating.at.1 - value.at.1).abs() > 1.0, "{rating:?} {value:?}");
    assert!(rating.at != (mm(101.6).as_f64(), mm(50.8).as_f64()));
    // Field Properties on one: its text, place and Show are the power symbol's
    let id = field_id("#PWR01", "Rating");
    let edit = Cmd::SchEdit(SchCmd::EditField {
        id: id.clone(),
        text: Some("2A".into()),
        at: Some(p(mm(110.0), mm(40.0))),
        vertical: None,
        h: None,
        v: None,
        size_nm: None,
        bold: None,
        italic: Some(true),
        visible: Some(false),
        name_shown: None,
        allow_autoplace: None,
        local_at: None,
        local_vertical: None,
        keep_autoplaced: false,
    });
    let after = run_on(start, vec![edit]).unwrap();
    let f = &after.extras.power_props["#PWR01"].fields[0];
    assert_eq!((f.text.as_str(), f.visible, f.italic), ("2A", false, true), "the power symbol keeps the text, Show and Italic");
    let page = eda_engine::fields::power_fields(&after, &after.power_symbols[0]);
    let rating = page.iter().find(|f| f.name == "Rating").unwrap();
    assert_eq!((rating.at.0.round(), rating.at.1.round()), (mm(110.0).as_f64(), mm(40.0).as_f64()));
    // its Reference is still not edited
    let e = run_on(sheet(), vec![Cmd::SchEdit(SchCmd::EditField { id: field_id("#PWR01", "Reference"), text: Some("#PWR9".into()), at: None, vertical: None, h: None, v: None, size_nm: None, bold: None, italic: None, visible: None, name_shown: None, allow_autoplace: None, local_at: None, local_vertical: None, keep_autoplaced: false })]).unwrap_err();
    assert_eq!(e[0].check, "ops_field_text_fixed");
}
