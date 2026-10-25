//! A user-defined field of a symbol in the `.kicad_sch` the studio writes and reads (package P10b, step 2): `(property "MPN" "text" (at x y angle) (effects ...))`,
//! written where the sheet keeps the field (`SchematicSection::field_layout`), hidden unless it is shown, and read back by `import_kicad_sch` with its text, its look
//! and its place in the frame of the symbol (`eda_engine::fields::page_field`'s inverse), so a second write puts it where the first did.
//!
//! The slow proof against kicad-cli (nested groups and a placed user field in one sheet: the ERC runs and the SVG draws the field where we put it) is in
//! `sch_groups.rs`. The last test is the text look of a hierarchical sheet pin (`SchExtras::sheet_pin_looks`: its size, bold and italic), written and read the same way.

use eda_model::units::mm;
use eda_kicad::{export_kicad_sch, import_kicad_sch, ExportMeta};
use eda_model::ir::{Design, FieldPlacement, Point, Provenance, SchematicSection, SymbolInstance, TextJustify, TextVAlign};
use eda_model::symbol::{LibPin, LibSymbol, SPoint, SymbolGraphic};
use eda_model::{ConstraintModel, Part, Pin, PinKind};

const META: ExportMeta<'static> = ExportMeta { date: "2026-10-13", title: "user fields" };

fn pin(number: &str, name: &str, etype: &str, x: f64, y: f64, angle: f64) -> LibPin {
    LibPin { number: number.into(), name: name.into(), electrical_type: etype.into(), shape: "line".into(), at: SPoint::new(x, y), angle_deg: angle, length_mm: 2.54, unit: 1, hidden: false }
}

/// A two-input gate: A and B on the left, Y on the right.
fn gate() -> LibSymbol {
    LibSymbol {
        lib_id: "test:GATE".into(),
        graphics: vec![SymbolGraphic::Rectangle { unit: 1, start: SPoint::new(-2.54, 5.08), end: SPoint::new(2.54, -5.08), stroke_mm: 0.254, filled: false }],
        pins: vec![pin("1", "A", "input", -7.62, 2.54, 0.0), pin("2", "B", "input", -7.62, 0.0, 0.0), pin("3", "Y", "output", 7.62, 0.0, 180.0)],
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
        alternate: None,
    }
}

fn model() -> ConstraintModel {
    let pins = ["1", "2", "3"].map(|n| Pin { number: n.into(), name: None, kind: PinKind::Signal }).to_vec();
    let part = Part {
        reference: "U1".into(),
        mpn: None,
        lcsc: None,
        value: Some("GATE".into()),
        package: None,
        footprint: Some("Foo:Bar".into()),
        symbol: Some("test:GATE".into()),
        datasheet: None,
        pins,
        body_nm: None,
        edge: None,
    };
    ConstraintModel { parts: vec![part], symbols: vec![gate()], ..Default::default() }
}

fn design_with(sch: SchematicSection) -> Design {
    Design {
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: Some(sch),
        nets: None,
        placement: None,
        routing: None,
        drawings: None,
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
    }
}

/// The gate `U1` turned `rot` (mirrored or not) with two user fields: `MPN`, shown 12.7 mm to the right and 15.24 above its origin in the frame of the symbol (left
/// justified, in 2 mm bold italic, its name shown), and `Manufacturer`, hidden.
fn sheet(rot: u32, mirrored: bool) -> SchematicSection {
    let sym = SymbolInstance {
        id: "U1".into(),
        at: Point { x: mm(76.2), y: mm(76.2) },
        rot,
        mirrored,
        mirror_y: false,
        lib_id: "test:GATE".into(),
        unit: 1,
        value: "GATE".into(),
        footprint: String::new(),
        datasheet: String::new(),
        dnp: false,
        exclude_from_bom: false,
        exclude_from_board: false,
        exclude_from_sim: false,
    };
    let mut sch = SchematicSection { symbols: vec![sym], ..Default::default() };
    let user = sch.user_fields.entry("U1".into()).or_default();
    user.insert("MPN".into(), "74LVC1G08".into());
    user.insert("Manufacturer".into(), "TI".into());
    sch.field_layout.insert(
        "U1".into(),
        vec![FieldPlacement { dx: mm(12.7), dy: mm(-15.24), h: TextJustify::Left, v: TextVAlign::Center, visible: true, size_nm: mm(2.0), bold: true, italic: true, name_shown: true, ..FieldPlacement::at_origin("MPN") }],
    );
    sch
}

/// The text of the `(property "name" ...)` form of the symbol, up to the next property or the end of the symbol.
fn property_of(text: &str, name: &str) -> String {
    let reference = text.find("(property \"Reference\" \"U1\"").expect("the placed symbol");
    let symbol = &text[text[..reference].rfind("(symbol").expect("its form")..];
    let from = symbol.find(&format!("(property \"{name}\"")).unwrap_or_else(|| panic!("no property {name} in\n{symbol}"));
    let rest = &symbol[from + 1..];
    let to = rest.find("(property").map(|i| i + 1).unwrap_or(rest.len());
    symbol[from..from + to].to_string()
}

fn export(sch: &SchematicSection, m: &ConstraintModel) -> String {
    export_kicad_sch(&design_with(sch.clone()), m, &META).expect("the sheet exports")
}

// ------------------------------------------------------------------------------------------------------------------------------ writing

#[test]
fn a_shown_user_field_is_written_where_the_sheet_keeps_it_and_a_hidden_one_is_hidden() {
    let m = model();
    let text = export(&sheet(0, false), &m);
    // the symbol is at 76.2 76.2 (the box corner, as this project draws): the field is 12.7 right and 15.24 above it
    let mpn = property_of(&text, "MPN");
    assert!(mpn.starts_with("(property \"MPN\" \"74LVC1G08\" (at 88.9 60.96 0)"), "{mpn}");
    assert!(mpn.contains("(show_name yes)") && mpn.contains("(size 2 2)") && mpn.contains("(bold yes)") && mpn.contains("(italic yes)") && mpn.contains("(justify left)"), "{mpn}");
    assert!(!mpn.contains("hide"), "{mpn}");
    let maker = property_of(&text, "Manufacturer");
    assert!(maker.contains("\"TI\"") && maker.contains("(hide yes)"), "a field KiCad adds is hidden: {maker}");
    // after the four standard fields, by name
    let reference = text.find("(property \"Reference\" \"U1\"").expect("the placed symbol");
    let symbol = &text[reference..];
    let order: Vec<usize> = ["Reference", "Value", "Footprint", "Datasheet", "MPN", "Manufacturer"].iter().map(|n| symbol.find(&format!("(property \"{n}\"")).unwrap_or_else(|| panic!("{n}"))).collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
}

#[test]
fn a_user_field_turns_with_its_symbol_in_the_file() {
    let m = model();
    let at = |rot: u32| -> (f64, f64) {
        let text = export(&sheet(rot, false), &m);
        let mpn = property_of(&text, "MPN");
        let nums: Vec<f64> = mpn.split("(at ").nth(1).unwrap().split(')').next().unwrap().split_whitespace().map(|n| n.parse().unwrap()).collect();
        (nums[0] - 76.2, nums[1] - 76.2)
    };
    let (dx, dy) = at(0);
    assert!((dx - 12.7).abs() < 1e-6 && (dy + 15.24).abs() < 1e-6);
    // KiCad's `TRANSFORM` for 90 degrees (counter-clockwise on the sheet): (x, y) goes to (y, -x)
    let (qx, qy) = at(90_000);
    assert!((qx + 15.24).abs() < 1e-6 && (qy + 12.7).abs() < 1e-6, "{qx} {qy}");
    let (hx, hy) = at(180_000);
    assert!((hx + 12.7).abs() < 1e-6 && (hy - 15.24).abs() < 1e-6, "{hx} {hy}");
}

// ------------------------------------------------------------------------------------------------------------------------------ reading

#[test]
fn a_file_with_user_fields_reads_back_with_their_texts_their_looks_and_their_places() {
    let m = model();
    let text = export(&sheet(0, false), &m);
    let (design, _model, _notes) = import_kicad_sch(&text).expect("the file reads");
    let sch = design.schematic.expect("a sheet");
    assert_eq!(sch.user_fields["U1"]["MPN"], "74LVC1G08");
    assert_eq!(sch.user_fields["U1"]["Manufacturer"], "TI");
    assert_eq!(sch.user_fields["U1"].len(), 2, "the four standard fields are not user fields");
    let placed = &sch.field_layout["U1"];
    let mpn = placed.iter().find(|p| p.name == "MPN").expect("the place of MPN");
    assert_eq!((mpn.dx, mpn.dy), (mm(12.7), mm(-15.24)));
    assert!(mpn.visible && mpn.bold && mpn.italic && mpn.name_shown && !mpn.no_autoplace);
    assert_eq!((mpn.h, mpn.v, mpn.size_nm, mpn.angle), (TextJustify::Left, TextVAlign::Center, mm(2.0), 0));
    let maker = placed.iter().find(|p| p.name == "Manufacturer").expect("the place of Manufacturer");
    assert!(!maker.visible);
    assert!(placed.iter().all(|p| p.name == "MPN" || p.name == "Manufacturer"), "only the user fields keep a place on a sheet read from a file");
}

#[test]
fn a_user_field_is_written_again_where_it_was_whatever_the_symbol_is_turned_to() {
    let m = model();
    for rot in [0, 90_000, 180_000, 270_000] {
        for mirrored in [false, true] {
            let first = export(&sheet(rot, mirrored), &m);
            let (design, model2, _) = import_kicad_sch(&first).unwrap_or_else(|e| panic!("rot {rot} mirrored {mirrored}: {e:?}"));
            let second = export_kicad_sch(&design, &model2, &META).expect("the sheet reads and writes again");
            for name in ["MPN", "Manufacturer"] {
                assert_eq!(property_of(&second, name), property_of(&first, name), "rot {rot} mirrored {mirrored}: {name}");
            }
        }
    }
}

const HAND_MADE: &str = r#"(kicad_sch (version 20260326) (generator "eeschema") (generator_version "10.0")
	(uuid "11111111-1111-1111-1111-111111111111")
	(paper "A4")
	(lib_symbols
		(symbol "test:GATE" (in_bom yes) (on_board yes)
			(property "Reference" "U" (at 0 0 0) (effects (font (size 1.27 1.27))))
			(property "Value" "GATE" (at 0 0 0) (effects (font (size 1.27 1.27))))
			(symbol "GATE_1_1"
				(rectangle (start -2.54 5.08) (end 2.54 -5.08) (stroke (width 0.254) (type default)) (fill (type none)))
				(pin input line (at -7.62 2.54 0) (length 2.54) (name "A" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
			)
		)
	)
	(symbol (lib_id "test:GATE") (at 100 100 0) (unit 1)
		(exclude_from_sim no) (in_bom yes) (on_board yes) (dnp no)
		(uuid "22222222-2222-2222-2222-222222222222")
		(property "Reference" "U1" (at 100 94.92 0) (effects (font (size 1.27 1.27))))
		(property "Value" "GATE" (at 100 105.08 0) (effects (font (size 1.27 1.27))))
		(property "Footprint" "" (at 100 100 0) (effects (font (size 1.27 1.27)) (hide yes)))
		(property "Datasheet" "" (at 100 100 0) (effects (font (size 1.27 1.27)) (hide yes)))
		(property "Description" "a gate" (at 100 100 0) (effects (font (size 1.27 1.27)) (hide yes)))
		(property "value" "dropped" (at 100 100 0) (effects (font (size 1.27 1.27)) (hide yes)))
		(property "Sim.Device" "SUBCKT" (at 100 100 0) (hide yes) (effects (font (size 1.27 1.27))))
		(property private "Internal note" "keep out" (at 104 96 90) (effects (font (size 1.016 1.016)) (justify right bottom)))
		(property "Empty" "" (at 100 100 0) (effects (font (size 1.27 1.27)) hide))
		(property "Sim.Device" "twice" (at 100 100 0) (effects (font (size 1.27 1.27)) (hide yes)))
		(pin "1" (uuid "33333333-3333-3333-3333-333333333333"))
		(instances (project "p" (path "/11111111-1111-1111-1111-111111111111" (reference "U1") (unit 1))))
	)
)
"#;

#[test]
fn the_properties_beyond_the_standard_ones_are_the_user_fields_of_a_file_made_by_hand() {
    let (design, _model, _notes) = import_kicad_sch(HAND_MADE).unwrap_or_else(|e| panic!("{e:?}"));
    let sch = design.schematic.expect("a sheet");
    let user = &sch.user_fields["U1"];
    let names: Vec<&str> = user.keys().map(String::as_str).collect();
    assert_eq!(names, ["Empty", "Internal note", "Sim.Device"], "not Reference, Value, Footprint, Datasheet and Description, whatever the case; the first of a name");
    assert_eq!(user["Sim.Device"], "SUBCKT");
    assert_eq!(user["Empty"], "", "a field may be empty");
    assert_eq!(user["Internal note"], "keep out", "`private` is a word before the name");
    let placed = &sch.field_layout["U1"];
    let get = |n: &str| placed.iter().find(|p| p.name == n).unwrap_or_else(|| panic!("no place for {n}"));
    assert!(!get("Sim.Device").visible, "(hide yes) in the property, as KiCad 9 writes it");
    assert!(!get("Empty").visible, "a bare `hide` in the effects");
    let note = get("Internal note");
    assert!(note.visible && note.size_nm == mm(1.016));
    assert_eq!((note.dx, note.dy), (mm(4.0), mm(-4.0)), "from the symbol's origin");
    assert_eq!(note.angle, 90_000, "a vertical text");
    assert_eq!((note.h, note.v), (TextJustify::Right, TextVAlign::Bottom));
}

#[test]
fn a_sheet_with_no_user_fields_reads_with_none() {
    let m = model();
    let mut plain = sheet(0, false);
    plain.user_fields.clear();
    plain.field_layout.clear();
    let (design, _model, _) = import_kicad_sch(&export(&plain, &m)).unwrap();
    let sch = design.schematic.unwrap();
    assert!(sch.user_fields.is_empty() && sch.field_layout.is_empty());
}

// ------------------------------------------------------------------------------------------------------------- a sheet pin's text look

#[test]
fn a_sheet_pins_text_look_is_written_and_read_back() {
    use eda_model::ir::{LabelShape, SheetInstance, SheetPin};
    use eda_model::sch_extras::LabelLook;
    let m = model();
    let mut sch = SchematicSection { symbols: vec![], ..Default::default() };
    sch.sheets.push(SheetInstance {
        id: "11111111-1111-4111-8111-111111111111".into(),
        name: "Sub".into(),
        file: "sub.kicad_sch".into(),
        at: Point { x: mm(100.0), y: mm(50.0) },
        size: (mm(40.0), mm(30.0)),
        pins: vec![
            SheetPin { id: "22222222-2222-4222-8222-222222222222".into(), name: "CLK".into(), shape: LabelShape::Input, at: Point { x: mm(100.0), y: mm(60.0) } },
            SheetPin { id: "33333333-3333-4333-8333-333333333333".into(), name: "DATA".into(), shape: LabelShape::Output, at: Point { x: mm(140.0), y: mm(70.0) } },
        ],
        page: String::new(),
    });
    sch.extras.sheet_pin_looks.insert("22222222-2222-4222-8222-222222222222".into(), LabelLook { size_nm: mm(2.0), bold: true, italic: true, auto_rotate: false });
    let text = export_kicad_sch(&design_with(sch), &m, &META).expect("the sheet exports");
    // the font of the pin says its size, bold and italic; the other pin has the default text
    let pin = |name: &str| {
        let from = text.find(&format!("(pin \"{name}\"")).unwrap_or_else(|| panic!("no pin {name}"));
        text[from..from + text[from..].find("(effects").unwrap() + 120].to_string()
    };
    assert!(pin("CLK").contains("(font (size 2 2) (bold yes) (italic yes))"), "{}", pin("CLK"));
    assert!(pin("DATA").contains("(font (size 1.27 1.27))") && !pin("DATA").contains("bold"), "{}", pin("DATA"));
    // and the reader gives the look back, by the pin's uuid
    let (design, _, _) = import_kicad_sch(&text).expect("the file reads");
    let back = design.schematic.expect("a sheet");
    let looks = &back.extras.sheet_pin_looks;
    assert_eq!(looks.len(), 1, "only a pin that is set differently has an entry: {looks:?}");
    let look = looks.get("22222222-2222-4222-8222-222222222222").copied().expect("the look of CLK");
    assert_eq!((look.size_nm, look.bold, look.italic), (mm(2.0), true, true));
}
