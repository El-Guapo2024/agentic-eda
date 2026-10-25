//! A label's own fields (`SchExtras::label_fields`: its net class and the others) and a global label's Sheet References in the `.kicad_sch` file.
//!
//! * Pure Rust: our writer, our reader -- the fields come back in order with their text, visibility and italics; a global label's Sheet References is written
//!   for every global label (`SCH_GLOBALLABEL`'s mandatory field) and is not read back as a field of its own.
//! * Slow (`EDA_SLOW_TESTS=1`): kicad-cli loads the file and writes it again itself (`sch upgrade --force`), and what it kept of each label's fields is what
//!   we wrote.

use eda_model::units::mm;
use eda_model::units::Nm;
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
    LabelField { name: name.into(), text: text.into(), visible, italic }
}

/// (label kind, spin, fields) of each label the test draws.
fn cases() -> Vec<(LabelKind, LabelSpin, Vec<LabelField>)> {
    vec![
        (LabelKind::Local, LabelSpin::Right, vec![field("Netclass", "Power", true, true)]),
        (LabelKind::Local, LabelSpin::Up, vec![field("Netclass", "HV", false, false), field("Note", "keep short", true, false)]),
        (LabelKind::Global { shape: LabelShape::Input }, LabelSpin::Left, vec![field("Netclass", "Power", true, true)]),
        (LabelKind::Global { shape: LabelShape::Output }, LabelSpin::Bottom, vec![]),
        (LabelKind::Hierarchical { shape: LabelShape::Bidirectional }, LabelSpin::Right, vec![field("Netclass", "Signal", true, true)]),
    ]
}

fn drawn() -> (eda_model::ir::Design, ConstraintModel) {
    let model = model();
    let mut design = derive_schematic(&model, &EngineOptions::new(1, "hash")).unwrap();
    let sch = design.schematic.as_mut().unwrap();
    for (i, (kind, spin, fields)) in cases().into_iter().enumerate() {
        let id = format!("lbl_fields_{i}");
        sch.labels.push(NetLabel { id: id.clone(), net: format!("FLD{i}"), at: Point { x: mm(200.0), y: Nm::from_raw_shim(20_000_000 + 20_320_000 * i as i64) }, kind });
        sch.extras.label_spins.insert(id.clone(), spin);
        if !fields.is_empty() {
            sch.extras.label_fields.insert(id, fields);
        }
    }
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
