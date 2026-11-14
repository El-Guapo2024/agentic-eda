//! A label's own fields (`SchExtras::label_fields`: its net class and the others) and a global label's Sheet References in the `.kicad_sch` file.
//!
//! * Pure Rust: our writer, our reader -- the fields come back in order with their text, visibility and italics; a global label's Sheet References is written
//!   for every global label (`SCH_GLOBALLABEL`'s mandatory field) and is not read back as a field of its own.
//! * Slow (`EDA_SLOW_TESTS=1`): kicad-cli loads the file and writes it again itself (`sch upgrade --force`), and what it kept of each label's fields is what
//!   we wrote.

use eda_model::units::mm;
use eda_engine::{derive_schematic, EngineOptions};
use eda_kicad::{export_kicad_sch, import_kicad_sch, ExportMeta};
use eda_model::ir::{LabelKind, LabelShape, NetLabel, Point};
use eda_model::sch_extras::{LabelField, LabelSpin};
use eda_model::{ConstraintModel, Net, Part, Pin, PinKind};
use std::path::PathBuf;
use std::process::Command;

fn model() -> ConstraintModel {
    let pin = |number: &str| Pin { number: number.into(), name: None, kind: PinKind::Passive };
    let r1 = Part { reference: "R1".into(), mpn: None, lcsc: None, value: Some("10k".into()), package: None, footprint: Some("Foo:Bar".into()), pins: vec![pin("1"), pin("2")], body_nm: None, symbol: Some("Device:R".into()), datasheet: None, edge: None };
    ConstraintModel { parts: vec![r1], nets: vec![Net { name: "SIG".into(), pins: vec!["R1.1".into(), "R1.2".into()] }], ..Default::default() }
}

fn field(name: &str, text: &str, visible: bool, italic: bool) -> LabelField {
    LabelField { name: name.into(), text: text.into(), visible, italic, ..Default::default() }
}

/// (label kind, spin, fields) of each label the test draws.
fn cases() -> Vec<(LabelKind, LabelSpin, Vec<LabelField>)> {
    vec![
        (LabelKind::Local, LabelSpin::Right, vec![field("Netclass", "Power", true, true)]),
        (LabelKind::Local, LabelSpin::Up, vec![field("Netclass", "HV", false, false), field("Note", "keep short", true, false)]),
        (LabelKind::Global { shape: LabelShape::Input }, LabelSpin::Left, vec![field("Netclass", "Power", true, true)]),
        (LabelKind::Global { shape: LabelShape::Output }, LabelSpin::Bottom, vec![]),
        (LabelKind::Hierarchical { shape: LabelShape::Bidirectional }, LabelSpin::Right, vec![field("Netclass", "Signal", true, true)]),
        // a field's own size, bold and shown name (UI-LEFT-2 SS12): `(effects (font (size 2 2) (bold yes)))`, `(show_name yes)`
        (LabelKind::Local, LabelSpin::Right, vec![LabelField { size_nm: mm(2.0), bold: true, name_shown: true, ..field("Netclass", "Power", true, false) }, LabelField { bold: true, ..field("Note", "x", true, true) }]),
        // Allow Autoplacement unticked (UI-LEFT-3 S3-31): `(do_not_autoplace yes)`
        (LabelKind::Local, LabelSpin::Right, vec![LabelField { no_autoplace: true, ..field("Note", "free", true, false) }]),
    ]
}

fn drawn() -> (eda_model::ir::Design, ConstraintModel) {
    let model = model();
    let mut design = derive_schematic(&model, &EngineOptions::new(1, "hash")).unwrap();
    let sch = design.schematic.as_mut().unwrap();
    for (i, (kind, spin, fields)) in cases().into_iter().enumerate() {
        let id = format!("lbl_fields_{i}");
        sch.labels.push(NetLabel { id: id.clone(), net: format!("FLD{i}"), at: Point { x: mm(200.0), y: mm(20.0) + mm(20.32) * i as i64 }, kind });
        sch.extras.label_spins.insert(id.clone(), spin);
        if !fields.is_empty() {
            sch.extras.label_fields.insert(id, fields);
        }
    }
    // the first global label's Sheet References with looks of their own, and the first label's net class in a face and a colour (UI-LEFT-3 S3-31)
    sch.extras.intersheet_refs_looks.insert("lbl_fields_2".into(), LabelField { name: "Intersheetrefs".into(), size_nm: mm(2.0), bold: true, italic: true, name_shown: true, no_autoplace: true, ..Default::default() });
    sch.extras.field_looks.entry("lbl_fields_0".into()).or_default().insert("Netclass".into(), eda_model::sch_extras::FieldFontLook { font: Some("KiCad Font".into()), color: Some(eda_model::sch_extras::SchColor { r: 0, g: 128, b: 0, a: 255 }) });
    (design, model)
}

const META: ExportMeta<'static> = ExportMeta { date: "2026-10-23", title: "label fields" };

/// The fields the file gives each of the test's labels, in the test's order.
fn read_back(text: &str) -> Vec<Vec<LabelField>> {
    let (design, _, _) = import_kicad_sch(text).expect("the file reads");
    let sch = design.schematic.unwrap();
    (0..cases().len()).map(|i| {
        let l = sch.labels.iter().find(|l| l.net == format!("FLD{i}")).expect("the label");
        sch.extras.label_fields.get(&l.id).cloned().unwrap_or_default()
    }).collect()
}

#[test]
fn a_labels_fields_come_back_from_the_file_our_writer_wrote() {
    let (design, model) = drawn();
    let text = export_kicad_sch(&design, &model, &META).expect("the sheet exports");
    let want: Vec<Vec<LabelField>> = cases().into_iter().map(|(_, _, f)| f).collect();
    assert_eq!(read_back(&text), want);
    // every global label has its Sheet References, hidden while Schematic Setup does not show them
    assert_eq!(text.matches("(property \"Intersheetrefs\" \"${INTERSHEET_REFS}\"").count(), 2, "{text}");
    assert!(text.lines().filter(|l| l.contains("Intersheetrefs")).all(|l| l.contains("(hide yes)")));
    // the net class field of a label that runs right is under its text, left-justified (`AutoplaceFields`: half the label's height, then half the field's)
    let line = text.lines().find(|l| l.contains("(property \"Netclass\" \"Power\"")).unwrap();
    assert!(line.contains("(at 200 21.27 0)") && line.contains("(italic yes)") && line.contains("(justify left)"), "{line}");
    // the field with a size, bold and its name shown: `saveField`'s `(show_name yes)` after `(at ..)`, `EDA_TEXT::Format`'s size and bold; it is stacked by
    // its own height (`accumulated + field.GetTextHeight() / 2`: 0.635 + 1 under the label at y = 121.6)
    let line = text.lines().find(|l| l.contains("(property \"Netclass\" \"Power\"") && l.contains("show_name")).unwrap();
    assert!(line.contains("(at 200 123.235 0) (show_name yes)") && line.contains("(font (size 2 2) (bold yes))"), "{line}");
}

/// The looks a label keeps beside its fields' (UI-LEFT-3 S3-31): a global label's Sheet References' size, bold, italic, shown name and `do_not_autoplace`
/// (`SchExtras::intersheet_refs_looks`), a field's face and colour (`field_looks` under the label's id), each read back from the file.
fn looks_back(text: &str) -> (Option<LabelField>, Option<eda_model::sch_extras::FieldFontLook>) {
    let (design, _, _) = import_kicad_sch(text).expect("the file reads");
    let sch = design.schematic.unwrap();
    let id = |net: &str| sch.labels.iter().find(|l| l.net == net).unwrap().id.clone();
    (sch.extras.intersheet_refs_looks.get(&id("FLD2")).cloned(), sch.extras.field_look(&id("FLD0"), "Netclass").cloned())
}

#[test]
fn a_global_labels_sheet_references_and_a_fields_face_come_back() {
    let (design, model) = drawn();
    let text = export_kicad_sch(&design, &model, &META).expect("the sheet exports");
    let line = text.lines().find(|l| l.contains("(property \"Intersheetrefs\"") && l.contains("show_name")).expect("the looked Sheet References");
    assert!(line.contains("(show_name yes) (do_not_autoplace yes)") && line.contains("(size 2 2) (bold yes) (italic yes)"), "{line}");
    let (isr, face) = looks_back(&text);
    assert_eq!(isr, Some(LabelField { name: "Intersheetrefs".into(), size_nm: mm(2.0), bold: true, italic: true, name_shown: true, no_autoplace: true, ..Default::default() }));
    assert_eq!(face.and_then(|f| f.font), Some("KiCad Font".to_string()));
}

#[test]
fn kicad_loads_the_labels_fields_and_writes_them_back() {
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
    let dir = std::env::temp_dir().join(format!("eda_label_fields_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("labels.kicad_sch");
    std::fs::write(&path, &text).unwrap();
    let up = Command::new(&cli).args(["sch", "upgrade", "--force"]).arg(&path).output().expect("run kicad-cli");
    assert!(up.status.success(), "kicad-cli sch upgrade: {}{}", String::from_utf8_lossy(&up.stdout), String::from_utf8_lossy(&up.stderr));
    let kicads = std::fs::read_to_string(&path).unwrap();
    assert_ne!(kicads, text, "KiCad wrote the file itself");
    let want: Vec<Vec<LabelField>> = cases().into_iter().map(|(_, _, f)| f).collect();
    assert_eq!(read_back(&kicads), want, "what KiCad kept of the labels' fields");
    let (isr, face) = looks_back(&kicads);
    assert_eq!(isr, Some(LabelField { name: "Intersheetrefs".into(), size_nm: mm(2.0), bold: true, italic: true, name_shown: true, no_autoplace: true, ..Default::default() }), "the Sheet References' looks KiCad kept");
    assert_eq!(face.and_then(|f| f.color), Some(eda_model::sch_extras::SchColor { r: 0, g: 128, b: 0, a: 255 }), "the net class's colour KiCad kept");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `parseSchField` on a global label knows its Sheet References by the canonical name without regard to case and by KiCad 6's "Intersheet References"
/// (`qa/data/eeschema/NoConnectOnLineWithGlobalLabel.kicad_sch` writes it so): it is the label's mandatory field, not a field of its own -- read as one it
/// would be written back beside the Sheet References, twice. On a local label the same name is a field like any other.
#[test]
fn a_kicad_6_global_labels_sheet_references_is_not_a_field_of_its_own() {
    let (design, model) = drawn();
    let text = export_kicad_sch(&design, &model, &META).expect("the sheet exports");
    let legacy = text.replace("(property \"Intersheetrefs\"", "(property \"Intersheet References\"");
    assert_ne!(legacy, text);
    let want: Vec<Vec<LabelField>> = cases().into_iter().map(|(_, _, f)| f).collect();
    assert_eq!(read_back(&legacy), want, "KiCad 6's name is the Sheet References");
    assert_eq!(read_back(&text.replace("(property \"Intersheetrefs\"", "(property \"INTERSHEETREFS\"")), want, "the canonical name in any case");
    // on a local label the name is the user's own
    let local = text.replacen("(property \"Netclass\" \"Power\"", "(property \"Intersheet References\" \"Power\"", 1);
    assert_eq!(read_back(&local)[0], vec![field("Intersheet References", "Power", true, true)]);
}
