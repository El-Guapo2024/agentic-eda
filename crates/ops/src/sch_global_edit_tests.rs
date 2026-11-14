//! Edit Text & Graphics Properties over the hierarchy (`SchCmd::GlobalEditTextGraphics`, `crate::sch_global_edit`) and Change / Update Symbols' reset of the
//! fields to the library's (`SchCmd::ResetSymbolFields`): UI-LEFT-2 SS14.
//!
//! The design: the root holds R1 (a resistor, mirrored), a wire of net N1 with a local label and a junction on it, a free text, a rectangle, a graphic line,
//! the power symbol #PWR01 and the sheet `sa`, which shows `a.kicad_sch`: R2, a bus `D[0..1]` with a label on it, a hierarchical label.
use super::*;
use crate::sch_edit::SchCmd;
use crate::sch_global_edit::{GlobalEditSpec, SymbolTypeFilter};
use eda_engine::fields_edit::{field_id, OwnerCtx};
use eda_model::ir::{field_key, FieldPlacement, Junction, LabelKind, LabelShape, NetLabel, PowerSymbol, Provenance, SchLine, SchematicSection, SchematicText, SheetInstance, SymbolInstance, TextJustify, TextVAlign, Wire};
use eda_model::sch_extras::{LabelSpin, SchColor, SchFill, SchGraphic, SchGraphicKind, SchHAlign, SchLineStyle, SchVAlign};
use eda_model::units::mm;
use eda_model::{Part, Pin, PinKind};

fn p(x: f64, y: f64) -> Point {
    Point { x: mm(x), y: mm(y) }
}

fn part(reference: &str) -> Part {
    Part { reference: reference.into(), mpn: None, lcsc: None, value: Some("10k".into()), package: None, footprint: None, symbol: None, datasheet: None, pins: (1..=2).map(|i| Pin { number: i.to_string(), name: None, kind: PinKind::Passive }).collect(), body_nm: None, edge: None }
}

fn model() -> ConstraintModel {
    ConstraintModel { parts: vec![part("R1"), part("R2")], ..Default::default() }
}

fn resistor(id: &str, at: Point, mirrored: bool) -> SymbolInstance {
    SymbolInstance { id: id.into(), at, rot: 0, mirrored, mirror_y: false, lib_id: "Device:R".into(), unit: 1, value: "10k".into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false, no_footprint: false }
}

fn wire(id: &str, a: Point, b: Point, net: &str, bus: bool) -> Wire {
    Wire { id: id.into(), net: net.into(), pins: vec![], pts: vec![a, b], bus }
}

fn label(id: &str, net: &str, at: Point, kind: LabelKind) -> NetLabel {
    NetLabel { id: id.into(), net: net.into(), at, kind }
}

fn design() -> Design {
    let mut root = SchematicSection {
        symbols: vec![resistor("R1", p(50.8, 50.8), true)],
        wires: vec![wire("w1", p(0.0, 20.32), p(40.64, 20.32), "N1", false)],
        labels: vec![label("l1", "N1", p(10.16, 20.32), LabelKind::Local), label("g1", "G", p(100.0, 100.0), LabelKind::Global { shape: LabelShape::Input })],
        junctions: vec![Junction { id: "j1".into(), at: p(20.32, 20.32) }],
        texts: vec![SchematicText { id: "t1".into(), content: "note".into(), at: p(80.0, 80.0), angle: 0, size_nm: mm(1.27) }],
        lines: vec![SchLine { id: "ln1".into(), pts: vec![p(0.0, 90.0), p(20.0, 90.0)], width_nm: Nm::ZERO }],
        power_symbols: vec![PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: p(70.0, 70.0), rot: 0, net: "GND".into(), pin: String::new() }],
        sheets: vec![SheetInstance { id: "sa".into(), name: "A".into(), file: "a.kicad_sch".into(), at: p(150.0, 20.0), size: (mm(50.0), mm(30.0)), pins: vec![], page: String::new() }],
        ..Default::default()
    };
    root.extras.graphics.push(SchGraphic { id: "rect1".into(), ..SchGraphic::new(SchGraphicKind::Rectangle { start: p(60.0, 0.0), end: p(70.0, 10.0), corner_radius_nm: Nm::ZERO }) });
    let a = SchematicSection {
        symbols: vec![resistor("R2", p(50.8, 50.8), false)],
        wires: vec![wire("b1", p(0.0, 30.48), p(40.64, 30.48), "D[0..1]", true)],
        labels: vec![label("lb", "D[0..1]", p(10.16, 30.48), LabelKind::Local), label("h1", "H", p(0.0, 0.0), LabelKind::Hierarchical { shape: LabelShape::Input })],
        ..Default::default()
    };
    Design {
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: Some(root),
        nets: None,
        placement: None,
        routing: None,
        drawings: None,
        footprint_library: None,
        sheet_contents: Some(BTreeMap::from([("a.kicad_sch".to_string(), a)])),
        bus_aliases: vec![],
        symbol_library: None,
    }
}

fn edit(spec: GlobalEditSpec) -> Cmd {
    Cmd::sch(SchCmd::GlobalEditTextGraphics { spec })
}

fn run(spec: GlobalEditSpec) -> Result<Design, Vec<CheckResult>> {
    let m = model();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    b.apply(&edit(spec))?;
    Ok(b.design().clone())
}

fn root(d: &Design) -> &SchematicSection {
    d.schematic.as_ref().unwrap()
}

fn sheet_a(d: &Design) -> &SchematicSection {
    &d.sheet_contents.as_ref().unwrap()["a.kicad_sch"]
}

/// The placement the field `name` of the item `key` has on `sch` (stored, else what the editor gives it).
fn place(sch: &SchematicSection, key: &str, name: &str) -> FieldPlacement {
    let m = model();
    let ctx = OwnerCtx::new(sch, &m, key).unwrap();
    let i = ctx.specs.iter().position(|s| s.name == name).unwrap();
    ctx.current(sch)[i].clone()
}

/// `TransferDataFromWindow` walks every sheet path: the Reference designators of both sheets take the size, the Values do not.
#[test]
fn the_edit_reaches_every_sheet_of_the_hierarchy() {
    let d = run(GlobalEditSpec { references: true, text_size_nm: Some(mm(2.0)), ..Default::default() }).unwrap();
    assert_eq!(place(root(&d), "R1", "Reference").size_nm, mm(2.0), "the root's symbol");
    assert_eq!(place(sheet_a(&d), "R2", "Reference").size_nm, mm(2.0), "the sheet's symbol (the sheet in view was the root)");
    assert_eq!(place(root(&d), "R1", "Value").size_nm, Nm::ZERO, "Values not ticked");
    assert_eq!(place(root(&d), "#PWR01", "Reference").size_nm, mm(2.0), "a power symbol is a symbol");
}

/// "By parent reference designator:", "By parent symbol library id:" and "By parent symbol type:".
#[test]
fn the_reference_symbol_and_type_filters_choose_the_symbols() {
    let d = run(GlobalEditSpec { references: true, values: true, reference_filter: Some("r2".into()), bold: Some(true), ..Default::default() }).unwrap();
    assert!(place(sheet_a(&d), "R2", "Reference").bold && place(sheet_a(&d), "R2", "Value").bold, "R2 matches, case folded");
    assert!(!place(root(&d), "R1", "Reference").bold && !place(root(&d), "#PWR01", "Value").bold, "R1 and #PWR01 do not");
    let d = run(GlobalEditSpec { values: true, symbol_filter: Some("power:*".into()), italic: Some(true), ..Default::default() }).unwrap();
    assert!(place(root(&d), "#PWR01", "Value").italic && !place(root(&d), "R1", "Value").italic);
    let d = run(GlobalEditSpec { values: true, type_filter: Some(SymbolTypeFilter::NonPower), italic: Some(true), ..Default::default() }).unwrap();
    assert!(!place(root(&d), "#PWR01", "Value").italic && place(root(&d), "R1", "Value").italic && place(sheet_a(&d), "R2", "Value").italic);
}

/// "By net:": the wire, label and junction of the net; a symbol has no connection, so its fields are left whatever the scope says.
#[test]
fn the_net_filter_takes_what_is_on_the_net_and_no_symbol() {
    let d = run(GlobalEditSpec { references: true, wires: true, net_filter: Some("N*".into()), text_size_nm: Some(mm(2.0)), line_width_nm: Some(mm(0.3)), junction_size_nm: Some(mm(1.5)), ..Default::default() }).unwrap();
    assert_eq!(root(&d).extras.strokes["w1"].width_nm, mm(0.3));
    assert_eq!(root(&d).extras.label_looks["l1"].size_nm, mm(2.0), "the label on the wire");
    assert_eq!(root(&d).extras.junction_looks["j1"].diameter_nm, mm(1.5));
    assert_eq!(place(root(&d), "R1", "Reference").size_nm, Nm::ZERO, "a symbol is not on a net");
    assert!(run(GlobalEditSpec { wires: true, net_filter: Some("X*".into()), line_width_nm: Some(mm(0.3)), ..Default::default() }).is_err(), "nothing on that net: nothing changes");
}

/// Wires & wire labels and Buses & bus labels: a label is a bus label when it sits on a bus; Global and Hierarchical labels by their kind.
#[test]
fn labels_are_reached_by_what_they_sit_on_and_by_their_kind() {
    let d = run(GlobalEditSpec { buses: true, italic: Some(true), ..Default::default() }).unwrap();
    assert!(sheet_a(&d).extras.label_looks["lb"].italic, "the label on the bus");
    assert!(!root(&d).extras.label_looks.contains_key("l1") && !sheet_a(&d).extras.label_looks.contains_key("h1"));
    let d = run(GlobalEditSpec { global_labels: true, hier_labels: true, bold: Some(true), set_text_color: true, text_color: Some(SchColor { r: 200, g: 0, b: 0, a: 255 }), ..Default::default() }).unwrap();
    assert!(root(&d).extras.label_looks["g1"].bold && sheet_a(&d).extras.label_looks["h1"].bold);
    assert_eq!(root(&d).extras.text_attrs["g1"].color, Some(SchColor { r: 200, g: 0, b: 0, a: 255 }), "the label's colour");
    assert!(!root(&d).extras.label_looks.contains_key("l1"), "a local label is no global one");
}

/// Orientation (labels only): the spin is set (the dialog sends the spin its choice casts to).
#[test]
fn orientation_sets_a_labels_spin() {
    let d = run(GlobalEditSpec { wires: true, orientation: Some(LabelSpin::Up), ..Default::default() }).unwrap();
    assert_eq!(root(&d).extras.label_spins.get("l1"), Some(&LabelSpin::Up));
}

/// H Align on a field of a mirrored symbol is flipped (`GetTransform().x1 < 0`), so it reads as chosen; the font and colour go to the field's looks.
#[test]
fn a_fields_alignment_flips_on_a_mirrored_symbol_and_its_font_and_colour_are_kept() {
    let red = SchColor { r: 255, g: 0, b: 0, a: 255 };
    let d = run(GlobalEditSpec { references: true, h_align: Some(SchHAlign::Left), v_align: Some(SchVAlign::Top), font: Some("KiCad Font".into()), set_text_color: true, text_color: Some(red), ..Default::default() }).unwrap();
    let r1 = place(root(&d), "R1", "Reference");
    assert_eq!((r1.h, r1.v), (TextJustify::Right, TextVAlign::Top), "R1 is mirrored: left reads as right in its own frame");
    let r2 = place(sheet_a(&d), "R2", "Reference");
    assert_eq!((r2.h, r2.v), (TextJustify::Left, TextVAlign::Top));
    let look = root(&d).extras.field_look("R1", "Reference").cloned().unwrap();
    assert_eq!((look.font.as_deref(), look.color), (Some("KiCad Font"), Some(red)));
}

/// Visible and Show field name (fields only), on the Other symbol fields the field name filter names.
#[test]
fn visibility_and_shown_name_reach_the_fields_the_name_filter_names() {
    let d = run(GlobalEditSpec { other_fields: true, field_name_filter: Some("foot*".into()), visible: Some(true), show_field_names: Some(true), ..Default::default() }).unwrap();
    let fp = place(root(&d), "R1", "Footprint");
    assert!(fp.visible && fp.name_shown);
    assert!(!place(root(&d), "R1", "Datasheet").visible, "the filter leaves the Datasheet");
}

/// Schematic text & graphics: a text's size, bold, colour and justification, a shape's stroke and fill colour, a graphic line's width and style.
#[test]
fn texts_shapes_and_graphic_lines() {
    let blue = SchColor { r: 0, g: 0, b: 255, a: 255 };
    let d = run(GlobalEditSpec { sch_text_graphics: true, text_size_nm: Some(mm(2.54)), bold: Some(true), h_align: Some(SchHAlign::Center), line_width_nm: Some(mm(0.5)), line_style: Some(SchLineStyle::Dash), set_line_color: true, line_color: Some(blue), set_fill_color: true, fill_color: Some(blue), ..Default::default() }).unwrap();
    let r = root(&d);
    assert_eq!(r.texts[0].size_nm, mm(2.54));
    assert!(r.extras.text_attrs["t1"].bold);
    assert_eq!(r.extras.text_justify_of("t1").h, TextJustify::Center);
    let g = &r.extras.graphics[0];
    assert_eq!((g.width_nm, g.line_style, g.color, g.fill, g.fill_color), (mm(0.5), SchLineStyle::Dash, Some(blue), SchFill::Color, Some(blue)));
    assert_eq!(r.lines[0].width_nm, mm(0.5));
    assert_eq!(r.extras.strokes["ln1"].style, SchLineStyle::Dash);
    // an unspecified fill colour is no fill (`SetFillMode( FILL_T::NO_FILL )`)
    let m = model();
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&edit(GlobalEditSpec { sch_text_graphics: true, set_fill_color: true, fill_color: None, ..Default::default() })).unwrap();
    let g = &b.design().schematic.as_ref().unwrap().extras.graphics[0];
    assert_eq!((g.fill, g.fill_color), (SchFill::None, None));
}

/// Sheet titles, other sheet fields, borders and pins.
#[test]
fn sheets_titles_borders() {
    let d = run(GlobalEditSpec { sheet_titles: true, sheet_borders: true, text_size_nm: Some(mm(2.0)), line_width_nm: Some(mm(0.4)), ..Default::default() }).unwrap();
    assert_eq!(place(root(&d), "sa", "Sheetname").size_nm, mm(2.0));
    assert_eq!(place(root(&d), "sa", "Sheetfile").size_nm, Nm::ZERO, "Other sheet fields not ticked");
    assert_eq!(root(&d).extras.sheet_props_of("sa").border_width_nm, mm(0.4));
}

/// Selected items only: the items selected on the sheet in view and the children of a selected item; the other sheets have nothing selected.
#[test]
fn selected_items_only_takes_the_selection_of_the_sheet_in_view() {
    let d = run(GlobalEditSpec { references: true, values: true, selected_only: true, selection: vec!["R1".into()], bold: Some(true), ..Default::default() }).unwrap();
    assert!(place(root(&d), "R1", "Reference").bold && place(root(&d), "R1", "Value").bold, "the fields of a selected symbol");
    assert!(!place(sheet_a(&d), "R2", "Reference").bold);
    // a field selected by itself
    let d = run(GlobalEditSpec { references: true, values: true, selected_only: true, selection: vec![field_id(&field_key("R1", 1), "Value")], bold: Some(true), ..Default::default() }).unwrap();
    assert!(!place(root(&d), "R1", "Reference").bold && place(root(&d), "R1", "Value").bold);
    // on the sheet `sa`, R2 is the selection's
    let m = model();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::OnSheet { sheet: "sa".into(), cmd: Box::new(edit(GlobalEditSpec { references: true, selected_only: true, selection: vec!["R2".into()], bold: Some(true), ..Default::default() })) }).unwrap();
    assert!(place(sheet_a(b.design()), "R2", "Reference").bold && !place(root(b.design()), "R1", "Reference").bold);
}

/// One undo step for the whole hierarchy, refused when nothing changes, the text size checked, the verb as the dialog sends it.
#[test]
fn one_step_refusals_and_the_wire_form() {
    assert_eq!(run(GlobalEditSpec { references: true, ..Default::default() }).unwrap_err()[0].check, "ops_nothing_changed");
    assert_eq!(run(GlobalEditSpec { references: true, text_size_nm: Some(Nm::new(1_000)), ..Default::default() }).unwrap_err()[0].hint.as_deref(), Some("Text size must be at least 0.0254 mm."));
    let cmd: Cmd = serde_json::from_value(serde_json::json!({"op": "sch_edit", "verb": "global_edit_text_graphics", "spec": {"references": true, "reference_filter": "R*", "text_size_nm": 2000000, "set_text_color": true, "text_color": {"r": 1, "g": 2, "b": 3, "a": 255}, "orientation": "up", "h_align": "left", "line_style": "dash", "type_filter": "power"}})).unwrap();
    let Cmd::SchEdit(SchCmd::GlobalEditTextGraphics { spec }) = cmd else { panic!("the verb") };
    assert_eq!((spec.text_size_nm, spec.orientation, spec.type_filter), (Some(mm(2.0)), Some(LabelSpin::Up), Some(SymbolTypeFilter::Power)));
}

// ------------------------------------------------------------------------------------------------------------- Change / Update Symbols' reset

/// "Update field positions": each field at the library's place in the symbol's own frame (`SetTextPos( symbol->GetPosition() + libField->GetTextPos() )`),
/// here the frame of the box corner the sheet places the symbol by; then autoplaced when the symbol's fields were.
#[test]
fn reset_positions_puts_the_fields_where_the_library_symbol_has_them() {
    use eda_model::sym_fields::LibrarySymbolField;
    let m = model();
    let mut d = design();
    let mut lib = eda_model::ir::LibrarySymbol { lib_id: "Device:R".into(), reference_prefix: "R".into(), published: true, ..Default::default() };
    lib.set_fields(vec![LibrarySymbolField { name: "Reference".into(), text: "R".into(), at: eda_model::symbol::SPoint { x: 2.032, y: 0.0 }, ..LibrarySymbolField::default() }]);
    d.symbol_library = Some(eda_model::ir::SymbolLibrarySection { symbols: vec![lib], ..Default::default() });
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let reset = Cmd::sch(SchCmd::ResetSymbolFields { ids: vec!["R1".into()], visibility: false, effects: false, positions: true, allow_unchanged: false });
    b.apply(&reset).unwrap();
    // `Device:R`'s box: x from -1.016 mm, top at 3.81 mm (its pins' ends)
    let (x0, _, _, y1) = eda_engine::geometry::real_symbol_bbox(&eda_model::symbol::builtin("Device:R").unwrap(), 1);
    let r = place(root(b.design()), "R1", "Reference");
    assert_eq!((r.dx, r.dy), (mm(2.032 - x0), mm(y1)), "the library's place, from the box corner");
    // again: nothing to change, refused; but not inside Change Symbols' own step
    assert_eq!(b.apply(&reset).unwrap_err()[0].check, "ops_field_unchanged");
    b.apply(&Cmd::sch(SchCmd::ResetSymbolFields { ids: vec!["R1".into()], visibility: false, effects: false, positions: true, allow_unchanged: true })).unwrap();
    // fields that were autoplaced are autoplaced again after
    let mut sch = b.design().schematic.clone().unwrap();
    sch.extras.fields_autoplaced.insert("R1".into(), eda_model::sch_extras::AutoplaceAlgo::Auto);
    let mut d2 = b.design().clone();
    d2.schematic = Some(sch);
    let mut b = Board::new(d2, &m, mm(0.1), mm(0.3));
    b.apply(&reset).unwrap();
    let auto = OwnerCtx::new(root(b.design()), &m, "R1").unwrap().autoplace(root(b.design()), &m, eda_model::sch_extras::AutoplaceAlgo::Auto);
    assert_eq!(root(b.design()).field_layout["R1"], auto);
}

/// Change Symbols sends the change and the reset as one step: one batch, one undo.
#[test]
fn a_change_and_its_reset_are_one_batch() {
    let m = model();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    let batch = Cmd::Batch { cmds: vec![Cmd::sch(SchCmd::ResetSymbolFields { ids: vec!["R1".into()], visibility: true, effects: true, positions: true, allow_unchanged: true })] };
    b.apply(&batch).unwrap();
    assert!(root(b.design()).field_layout.contains_key("R1"));
}

/// UI-LEFT-3 S3-3: "Automatically place symbol fields" off (`EESCHEMA_SETTINGS::m_AutoplaceFields.enable`): a symbol placed has its library's fields where the
/// library puts them (`SCH_SYMBOL::SCH_SYMBOL( const LIB_SYMBOL& .. )`, no `AutoplaceFields` in `PlaceSymbol`) and is not autoplaced; and Change Symbols' reset
/// of the positions leaves them at the library's places (`if( resetPositions && .. m_AutoplaceFields.enable )`).
#[test]
fn a_symbol_placed_with_automatic_placement_off_has_its_fields_where_the_library_puts_them() {
    use eda_engine::fields::{with_autoplace_prefs, AutoplacePrefs};
    use eda_model::sym_fields::LibrarySymbolField;
    let m = ConstraintModel { parts: vec![part("R1"), part("R7"), part("R7")], ..Default::default() };
    let mut d = design();
    // (not published: the sheet draws the model's Device:R, the copy keeps only the fields' places)
    let mut lib = eda_model::ir::LibrarySymbol { lib_id: "Device:R".into(), reference_prefix: "R".into(), published: false, ..Default::default() };
    lib.set_fields(vec![LibrarySymbolField { name: "Reference".into(), text: "R".into(), at: eda_model::symbol::SPoint { x: 2.032, y: 0.0 }, ..LibrarySymbolField::default() }]);
    d.symbol_library = Some(eda_model::ir::SymbolLibrarySection { symbols: vec![lib], ..Default::default() });
    let add = Cmd::AddSymbol { annotate: None, id: "R7".into(), lib_id: "Device:R".into(), at: p(101.6, 101.6), rot_millideg: 0, value: "10k".into(), footprint: String::new(), unit: 1 };
    let off = AutoplacePrefs { enable: false, ..AutoplacePrefs::default() };
    let mut b = Board::new(d.clone(), &m, mm(0.1), mm(0.3));
    with_autoplace_prefs(off, || b.apply(&add)).unwrap();
    let (x0, _, _, y1) = eda_engine::geometry::real_symbol_bbox(&eda_model::symbol::builtin("Device:R").unwrap(), 1);
    let r = root(b.design()).field_layout["R7"].iter().find(|f| f.name == "Reference").cloned().unwrap();
    assert_eq!((r.dx, r.dy), (mm(2.032 - x0), mm(y1)), "the library's place");
    assert!(!root(b.design()).extras.fields_autoplaced.contains_key("R7"), "not autoplaced (AUTOPLACE_NONE)");
    // on (KiCad's default): no places are stored, the derivation autoplaces the new symbol
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&add).unwrap();
    assert!(!root(b.design()).field_layout.contains_key("R7"));
    // off, with no library copy to take the places from: the places drawn now are stored as the symbol's own, not autoplaced
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    with_autoplace_prefs(off, || b.apply(&add)).unwrap();
    assert!(root(b.design()).field_layout.contains_key("R7") && !root(b.design()).extras.fields_autoplaced.contains_key("R7"));
}

/// UI-LEFT-3 S3-10: H Align on a label is its `SetHorizJustify`, which `GetSpinStyle` reads back as its spin (Right justified: LEFT; Left: RIGHT; a vertical
/// label BOTTOM / UP); the Orientation, set after it in `processItem`, wins.
#[test]
fn h_align_turns_a_labels_spin_and_the_orientation_wins() {
    let d = run(GlobalEditSpec { wires: true, h_align: Some(SchHAlign::Right), ..Default::default() }).unwrap();
    assert_eq!(root(&d).extras.label_spins.get("l1"), Some(&LabelSpin::Left), "right justified: the text ends at the anchor");
    let d = run(GlobalEditSpec { wires: true, h_align: Some(SchHAlign::Right), orientation: Some(LabelSpin::Up), ..Default::default() }).unwrap();
    assert_eq!(root(&d).extras.label_spins.get("l1"), Some(&LabelSpin::Up), "SetSpinStyle after SetHorizJustify");
    // a label already left justified (spin RIGHT): Left changes nothing, so nothing the dialog reaches changes
    assert!(run(GlobalEditSpec { wires: true, h_align: Some(SchHAlign::Left), ..Default::default() }).is_err());
    // a vertical label: Right justified is BOTTOM
    let m = model();
    let mut des = design();
    des.schematic.as_mut().unwrap().extras.label_spins.insert("l1".into(), LabelSpin::Up);
    let mut b = Board::new(des, &m, mm(0.1), mm(0.3));
    b.apply(&edit(GlobalEditSpec { wires: true, h_align: Some(SchHAlign::Right), ..Default::default() })).unwrap();
    assert_eq!(root(b.design()).extras.label_spins.get("l1"), Some(&LabelSpin::Bottom));
}

/// UI-LEFT-3 S3-10: a directive label is an `EDA_TEXT`: `processItem` sets its size, bold and italic as well as its font and colour.
#[test]
fn a_directive_label_takes_the_size_bold_and_italic() {
    let m = model();
    let mut des = design();
    let mut flag = SchGraphic::new(SchGraphicKind::Directive { at: p(30.48, 20.32), orientation: 0, shape: eda_model::sch_extras::DirectiveShape::Round, pin_length_nm: mm(2.54), netclass: "HV".into(), component_class: String::new(), text_size_nm: Nm::ZERO });
    flag.id = "dir1".into();
    des.schematic.as_mut().unwrap().extras.graphics.push(flag);
    let mut b = Board::new(des, &m, mm(0.1), mm(0.3));
    b.apply(&edit(GlobalEditSpec { wires: true, text_size_nm: Some(mm(2.0)), bold: Some(true), italic: Some(true), ..Default::default() })).unwrap();
    let sch = root(b.design());
    let g = sch.extras.graphics.iter().find(|g| g.id == "dir1").unwrap();
    assert!(matches!(g.shape, SchGraphicKind::Directive { text_size_nm, .. } if text_size_nm == mm(2.0)), "{:?}", g.shape);
    assert!(sch.extras.text_attrs["dir1"].bold && sch.extras.text_attrs["dir1"].italic);
    // 50 mil is kept as none, as the reader keeps it
    b.apply(&edit(GlobalEditSpec { wires: true, text_size_nm: Some(mm(1.27)), ..Default::default() })).unwrap();
    let g = root(b.design()).extras.graphics.iter().find(|g| g.id == "dir1").unwrap().clone();
    assert!(matches!(g.shape, SchGraphicKind::Directive { text_size_nm, .. } if text_size_nm == Nm::ZERO));
}

/// UI-LEFT-3 S3-10: "By net:" on a screen shown twice reads each sheet path's name of a wire's net (`aItem->Connection( &aSheetPath )->Name()`): a local net
/// under each placement's path ("/A/LOC", "/B/LOC"), a net named by a pin under the reference each placement gives it. The model keeps one name per wire.
#[test]
fn the_net_filter_names_a_shared_screens_wire_per_sheet_path() {
    let mut m = model();
    m.nets = vec![eda_model::Net { name: "NA".into(), pins: vec!["R2.1".into()] }, eda_model::Net { name: "NB".into(), pins: vec!["R3.1".into()] }];
    let shared = || {
        let mut des = design();
        let root = des.schematic.as_mut().unwrap();
        root.sheets.push(SheetInstance { id: "sb".into(), name: "B".into(), file: "a.kicad_sch".into(), at: p(150.0, 80.0), size: (mm(50.0), mm(30.0)), pins: vec![], page: String::new() });
        let a = des.sheet_contents.as_mut().unwrap().get_mut("a.kicad_sch").unwrap();
        a.wires.push(wire("loc", p(0.0, 60.96), p(20.32, 60.96), "/A/LOC", false));
        let mut by_pin = wire("pinw", p(0.0, 71.12), p(20.32, 71.12), "NA", false);
        by_pin.pins = vec!["R2.1".into()];
        a.wires.push(by_pin);
        a.instance_overrides = vec![eda_model::ir::SymbolPathOverride { at: p(50.8, 50.8), sheet_path: vec!["sb".into()], reference: "R3".into(), unit: 1 }];
        des
    };
    let width = |filter: &str| -> Option<Vec<String>> {
        let mut b = Board::new(shared(), &m, mm(0.1), mm(0.3));
        b.apply(&edit(GlobalEditSpec { wires: true, net_filter: Some(filter.into()), line_width_nm: Some(mm(0.3)), ..Default::default() })).ok()?;
        let mut ids: Vec<String> = sheet_a(b.design()).extras.strokes.keys().cloned().collect();
        ids.sort();
        Some(ids)
    };
    assert_eq!(width("/B/LOC"), Some(vec!["loc".to_string()]), "the second placement's name of the local net");
    assert_eq!(width("/A/LOC"), Some(vec!["loc".to_string()]), "the first's");
    assert_eq!(width("NB"), Some(vec!["pinw".to_string()]), "R2 is R3 on the second placement: its net there");
    assert_eq!(width("NA"), Some(vec!["pinw".to_string()]));
    assert_eq!(width("/C/LOC"), None, "no placement names it so");
}

/// UI-LEFT-3 S3-10: H Align Center centres a label's text on its anchor and V Align is its own (`SchExtras::label_justify`), both put back by `SetSpinStyle` (the
/// Orientation) and kept by a turn.
#[test]
fn a_labels_centred_text_and_vertical_justification_are_kept_until_its_spin_is_set() {
    let m = model();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    b.apply(&edit(GlobalEditSpec { wires: true, h_align: Some(SchHAlign::Center), v_align: Some(SchVAlign::Top), ..Default::default() })).unwrap();
    let j = root(b.design()).extras.label_justify["l1"];
    assert_eq!((j.center, j.v), (true, Some(SchVAlign::Top)));
    // a local label's own bottom is `SetSpinStyle`'s: no entry for it
    b.apply(&edit(GlobalEditSpec { wires: true, h_align: Some(SchHAlign::Left), v_align: Some(SchVAlign::Bottom), ..Default::default() })).unwrap();
    assert!(!root(b.design()).extras.label_justify.contains_key("l1"));
    b.apply(&edit(GlobalEditSpec { wires: true, h_align: Some(SchHAlign::Center), ..Default::default() })).unwrap();
    b.apply(&edit(GlobalEditSpec { wires: true, orientation: Some(LabelSpin::Up), ..Default::default() })).unwrap();
    assert!(!root(b.design()).extras.label_justify.contains_key("l1"), "SetSpinStyle puts the justification back");
}
