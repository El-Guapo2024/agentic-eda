//! A text's, a text box's and a label's font, colour and link, and a label's fields placed by hand, in the `.kicad_sch` file (UI-LEFT S6).
//!
//! * Pure Rust: our writer, our reader -- `(face ..)` and `(color ..)` in `(font ..)`, `(href ..)`, `(exclude_from_sim ..)` come back as `SchExtras::text_attrs`
//!   (`EDA_TEXT::Format`, `parseEDA_TEXT`); a label whose fields were placed by hand has no `(fields_autoplaced yes)` and its fields come back at their places
//!   (`parseSchText`: `AUTOPLACE_NONE` until the token is read), as `field_layout` under the label's id.
//! * Slow (`EDA_SLOW_TESTS=1`): kicad-cli loads the file and writes it again itself (`sch upgrade --force`), and what it kept is what we wrote.

use eda_engine::{derive_schematic, EngineOptions};
use eda_kicad::{export_kicad_sch, import_kicad_sch, ExportMeta};
use eda_model::ir::{FieldPlacement, LabelKind, LabelShape, NetLabel, Point, SchematicText};
use eda_model::sch_extras::{LabelField, LabelSpin, SchColor, SchGraphic, SchGraphicKind, TextAttrs};
use eda_model::units::{mm, Nm};
use eda_model::{ConstraintModel, Net, Part, Pin, PinKind};
use std::path::PathBuf;
use std::process::Command;

fn model() -> ConstraintModel {
    let pin = |number: &str| Pin { number: number.into(), name: None, kind: PinKind::Passive };
    let r1 = Part { reference: "R1".into(), mpn: None, lcsc: None, value: Some("10k".into()), package: None, footprint: Some("Foo:Bar".into()), pins: vec![pin("1"), pin("2")], body_nm: None, symbol: Some("Device:R".into()), datasheet: None, edge: None };
    ConstraintModel { parts: vec![r1], nets: vec![Net { name: "SIG".into(), pins: vec!["R1.1".into(), "R1.2".into()] }], ..Default::default() }
}

const META: ExportMeta<'static> = ExportMeta { date: "2026-11-06", title: "text looks" };

fn blue() -> SchColor {
    SchColor { r: 0, g: 0, b: 255, a: 255 }
}

/// The hand-placed offsets of the net class field and the Sheet References of the global label `LOOK_G`.
fn hand_places() -> Vec<FieldPlacement> {
    vec![
        FieldPlacement { dx: mm(12.7), dy: mm(-2.54), ..FieldPlacement::at_origin("Intersheetrefs") },
        FieldPlacement { dx: mm(2.54), dy: mm(5.08), angle: 90_000, h: eda_model::ir::TextJustify::Left, ..FieldPlacement::at_origin("Netclass") },
    ]
}

fn drawn() -> (eda_model::ir::Design, ConstraintModel) {
    let model = model();
    let mut design = derive_schematic(&model, &EngineOptions::new(1, "hash")).unwrap();
    let sch = design.schematic.as_mut().unwrap();
    sch.texts.push(SchematicText { id: "txt_look".into(), content: "LOOK_TEXT".into(), at: Point { x: mm(200.0), y: mm(20.0) }, angle: 0, size_nm: mm(1.27) });
    sch.extras.text_attrs.insert("txt_look".into(), TextAttrs { bold: true, italic: true, color: Some(blue()), href: Some("#1".into()), font: Some("KiCad Font".into()), exclude_from_sim: true });
    sch.extras.graphics.push(SchGraphic {
        id: "shp_look".into(),
        shape: SchGraphicKind::TextBox { start: Point { x: mm(200.0), y: mm(40.0) }, end: Point { x: mm(230.0), y: mm(50.0) }, text: "LOOK_BOX".into(), angle: 0, size_nm: mm(1.27), bold: false, italic: false, h_align: Default::default(), v_align: Default::default(), margin_nm: Nm::ZERO, margins_nm: None },
        width_nm: Nm::ZERO,
        line_style: Default::default(),
        color: None,
        fill: Default::default(),
        fill_color: None,
    });
    sch.extras.text_attrs.insert("shp_look".into(), TextAttrs { color: Some(blue()), href: Some("https://kicad.org".into()), font: Some("KiCad Font".into()), ..Default::default() });
    sch.labels.push(NetLabel { id: "lbl_look_l".into(), net: "LOOK_L".into(), at: Point { x: mm(200.0), y: mm(70.0) }, kind: LabelKind::Local });
    sch.extras.label_spins.insert("lbl_look_l".into(), LabelSpin::Right);
    sch.extras.text_attrs.insert("lbl_look_l".into(), TextAttrs { color: Some(blue()), font: Some("KiCad Font".into()), ..Default::default() });
    sch.labels.push(NetLabel { id: "lbl_look_g".into(), net: "LOOK_G".into(), at: Point { x: mm(200.0), y: mm(90.0) }, kind: LabelKind::Global { shape: LabelShape::Input } });
    sch.extras.label_spins.insert("lbl_look_g".into(), LabelSpin::Right);
    sch.extras.label_fields.insert("lbl_look_g".into(), vec![LabelField { name: "Netclass".into(), text: "HV".into(), visible: true, italic: false }]);
    sch.field_layout.insert("lbl_look_g".into(), hand_places());
    (design, model)
}

/// What the file gives back: the text's, the text box's and the local label's attributes, and the global label's field places (dx, dy, angle, h) by name.
fn read_back(text: &str) -> (TextAttrs, TextAttrs, TextAttrs, Option<Vec<(String, Nm, Nm, i64, eda_model::ir::TextJustify)>>) {
    let (design, _, _) = import_kicad_sch(text).expect("the file reads");
    let sch = design.schematic.unwrap();
    let t = sch.texts.iter().find(|t| t.content == "LOOK_TEXT").expect("the text");
    let b = sch.extras.graphics.iter().find(|g| matches!(&g.shape, SchGraphicKind::TextBox { text, .. } if text == "LOOK_BOX")).expect("the text box");
    let l = sch.labels.iter().find(|l| l.net == "LOOK_L").expect("the local label");
    let g = sch.labels.iter().find(|l| l.net == "LOOK_G").expect("the global label");
    let attrs = |id: &str| sch.extras.text_attrs.get(id).cloned().unwrap_or_default();
    let places = sch.field_layout.get(&g.id).map(|ps| {
        let mut v: Vec<_> = ps.iter().map(|p| (p.name.clone(), p.dx, p.dy, p.angle as i64, p.h)).collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    });
    (attrs(&t.id), attrs(&b.id), attrs(&l.id), places)
}

fn want_places() -> Option<Vec<(String, Nm, Nm, i64, eda_model::ir::TextJustify)>> {
    let mut v: Vec<_> = hand_places().into_iter().map(|p| (p.name, p.dx, p.dy, p.angle as i64, p.h)).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    Some(v)
}

#[test]
fn a_texts_font_colour_and_link_and_a_labels_hand_placed_fields_come_back_from_our_file() {
    let (design, model) = drawn();
    let text = export_kicad_sch(&design, &model, &META).expect("the sheet exports");
    let line = text.lines().skip_while(|l| !l.contains("LOOK_TEXT")).find(|l| l.contains("(effects")).unwrap();
    assert!(line.contains("(font (face \"KiCad Font\") (size 1.27 1.27) (bold yes) (italic yes) (color 0 0 255 1))") && line.contains("(href \"#1\")"), "{line}");
    let (t, b, l, places) = read_back(&text);
    assert_eq!(t, design.schematic.as_ref().unwrap().extras.text_attrs["txt_look"]);
    assert_eq!(b, design.schematic.as_ref().unwrap().extras.text_attrs["shp_look"]);
    assert_eq!(l, design.schematic.as_ref().unwrap().extras.text_attrs["lbl_look_l"]);
    assert_eq!(places, want_places(), "the hand places of the global label's fields");
    // the hand-placed label has no `(fields_autoplaced yes)`; the other global labels keep it
    let label = text.split("(global_label \"LOOK_G\"").nth(1).unwrap();
    let label = &label[..label.find("\n\t)").unwrap()];
    assert!(!label.contains("fields_autoplaced"), "{label}");
    assert!(label.contains("(property \"Netclass\" \"HV\" (at 202.54 95.08 90)"), "{label}");
}

#[test]
fn kicad_keeps_the_text_looks_and_the_hand_placed_fields() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let cli = std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("kicad-cli"));
    if Command::new(&cli).arg("version").output().is_err() {
        eprintln!("skipped: no kicad-cli");
        return;
    }
    let (design, model) = drawn();
    let text = export_kicad_sch(&design, &model, &META).expect("the sheet exports");
    let dir = std::env::temp_dir().join(format!("eda_text_looks_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("looks.kicad_sch");
    std::fs::write(&path, &text).unwrap();
    let up = Command::new(&cli).args(["sch", "upgrade", "--force"]).arg(&path).output().expect("run kicad-cli");
    assert!(up.status.success(), "kicad-cli sch upgrade: {}{}", String::from_utf8_lossy(&up.stdout), String::from_utf8_lossy(&up.stderr));
    let kicads = std::fs::read_to_string(&path).unwrap();
    assert_ne!(kicads, text, "KiCad wrote the file itself");
    assert_eq!(read_back(&kicads), read_back(&text), "what KiCad kept of the looks and the places");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A sheet's style and attributes (`SchExtras::sheet_props`, UI-LEFT S8): `saveSheet`'s flags, `(stroke ..)` and `(fill ..)`, read back by `parseSheet`.
fn with_sheet() -> (eda_model::ir::Design, ConstraintModel) {
    let (mut design, model) = drawn();
    let sch = design.schematic.as_mut().unwrap();
    sch.sheets.push(eda_model::ir::SheetInstance { id: "a1b2c3d4-0000-4000-8000-000000000001".into(), name: "Power".into(), file: "power.kicad_sch".into(), at: Point { x: mm(250.0), y: mm(20.0) }, size: (mm(30.0), mm(20.0)), pins: vec![], page: "2".into() });
    sch.extras.sheet_props.insert(
        "a1b2c3d4-0000-4000-8000-000000000001".into(),
        eda_model::sch_extras::SheetProps { border_width_nm: mm(0.3), border_color: Some(blue()), fill_color: Some(SchColor { r: 255, g: 0, b: 0, a: 128 }), exclude_from_sim: true, exclude_from_bom: true, exclude_from_board: false, dnp: true },
    );
    sch.extras.sheet_fields.insert("a1b2c3d4-0000-4000-8000-000000000001".into(), vec![LabelField { name: "Rev".into(), text: "B".into(), visible: false, italic: false }]);
    design.sheet_contents = Some([("power.kicad_sch".to_string(), eda_model::ir::SchematicSection::default())].into_iter().collect());
    (design, model)
}

fn sheet_back(text: &str) -> (eda_model::sch_extras::SheetProps, Vec<LabelField>) {
    let (design, _, _) = import_kicad_sch(text).expect("the file reads");
    let sch = design.schematic.unwrap();
    let s = sch.sheets.iter().find(|s| s.name == "Power").expect("the sheet");
    (sch.extras.sheet_props_of(&s.id), sch.extras.sheet_fields.get(&s.id).cloned().unwrap_or_default())
}

#[test]
fn a_sheets_border_fill_and_attributes_come_back_from_our_file() {
    let (design, model) = with_sheet();
    let files = eda_kicad::export_kicad_sch_tree(&design, &model, &META, "looks.kicad_sch").expect("the tree exports");
    let root = &files.iter().find(|(n, _)| n == "looks.kicad_sch").unwrap().1;
    assert!(root.contains("(exclude_from_sim yes) (in_bom no) (on_board yes) (dnp yes)") && root.contains("(stroke (width 0.3) (type solid) (color 0 0 255 1))") && root.contains("(fill (color 255 0 0 0.502))"), "{root}");
    let want = design.schematic.as_ref().unwrap().extras.sheet_props.values().next().cloned().unwrap();
    let (props, fields) = sheet_back(root);
    assert_eq!(props, want);
    assert_eq!(fields, vec![LabelField { name: "Rev".into(), text: "B".into(), visible: false, italic: false }]);
}

#[test]
fn kicad_keeps_a_sheets_border_fill_and_attributes() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let cli = std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("kicad-cli"));
    if Command::new(&cli).arg("version").output().is_err() {
        eprintln!("skipped: no kicad-cli");
        return;
    }
    let (design, model) = with_sheet();
    let files = eda_kicad::export_kicad_sch_tree(&design, &model, &META, "looks.kicad_sch").expect("the tree exports");
    let dir = std::env::temp_dir().join(format!("eda_sheet_looks_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, text) in &files {
        std::fs::write(dir.join(name), text).unwrap();
    }
    let path = dir.join("looks.kicad_sch");
    let ours = std::fs::read_to_string(&path).unwrap();
    let up = Command::new(&cli).args(["sch", "upgrade", "--force"]).arg(&path).output().expect("run kicad-cli");
    assert!(up.status.success(), "kicad-cli sch upgrade: {}{}", String::from_utf8_lossy(&up.stdout), String::from_utf8_lossy(&up.stderr));
    let kicads = std::fs::read_to_string(&path).unwrap();
    assert_ne!(kicads, ours, "KiCad wrote the file itself");
    assert_eq!(sheet_back(&kicads), sheet_back(&ours), "what KiCad kept of the sheet's style, attributes and fields");
    let _ = std::fs::remove_dir_all(&dir);
}
