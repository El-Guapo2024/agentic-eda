//! A label's spin, size, bold and italic (`SchExtras::label_spins`, `label_looks`) and a field's look survive the `.kicad_sch` file.
//!
//! * The first test is pure Rust: our writer, our reader.
//! * The second hands the file to kicad-cli, which loads it and writes it again itself (`sch upgrade --force`, KiCad's own `SCH_IO_KICAD_SEXPR` writer),
//!   and reads KiCad's version back: what KiCad understood of each label is what we meant. It needs kicad-cli, so it runs with `EDA_SLOW_TESTS=1` (and
//!   skips when there is none).

use eda_engine::{derive_schematic, EngineOptions};
use eda_kicad::{export_kicad_sch, import_kicad_sch, ExportMeta};
use eda_model::ir::{LabelKind, LabelShape, NetLabel, Point};
use eda_model::sch_extras::{LabelLook, LabelSpin};
use eda_model::{ConstraintModel, Net, Part, Pin, PinKind};
use std::path::{Path, PathBuf};
use std::process::Command;

fn model() -> ConstraintModel {
    let pin = |number: &str, name: &str, kind: PinKind| Pin { number: number.into(), name: Some(name.into()), kind };
    let part = |reference: &str, symbol: Option<&str>, value: &str, pins: Vec<Pin>| Part {
        reference: reference.into(),
        mpn: None,
        lcsc: None,
        value: Some(value.into()),
        package: None,
        footprint: Some("Foo:Bar".into()),
        pins,
        body_um: None,
        symbol: symbol.map(String::from),
        datasheet: None,
        edge: None,
    };
    let r1 = part("R1", Some("Device:R"), "10k", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Passive)]);
    let c1 = part("C1", Some("Device:C"), "1u", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]);
    let net = |name: &str, pins: &[&str]| Net { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() };
    ConstraintModel { parts: vec![r1, c1], nets: vec![net("SIG", &["R1.1", "C1.1"]), net("GND", &["R1.2", "C1.2"])], ..Default::default() }
}

/// (kind, spin, look) of every label the test draws, on a derived sheet; the labels sit well clear of everything else.
fn cases() -> Vec<(LabelKind, LabelSpin, LabelLook)> {
    let kinds = [LabelKind::Local, LabelKind::Global { shape: LabelShape::Input }, LabelKind::Hierarchical { shape: LabelShape::Bidirectional }];
    let spins = [LabelSpin::Right, LabelSpin::Up, LabelSpin::Left, LabelSpin::Bottom];
    let mut out = Vec::new();
    for (k, kind) in kinds.iter().enumerate() {
        for (s, spin) in spins.iter().enumerate() {
            let look = match (k + s) % 4 {
                0 => LabelLook::default(),
                1 => LabelLook { size_um: 2_000, bold: false, italic: false },
                2 => LabelLook { size_um: 0, bold: true, italic: true },
                _ => LabelLook { size_um: 1_000, bold: true, italic: false },
            };
            out.push((kind.clone(), *spin, look));
        }
    }
    out
}

fn drawn() -> (eda_model::ir::Design, ConstraintModel) {
    let model = model();
    let mut design = derive_schematic(&model, &EngineOptions::new(1, "hash")).unwrap();
    let sch = design.schematic.as_mut().unwrap();
    for (i, (kind, spin, look)) in cases().into_iter().enumerate() {
        let id = format!("lbl_geometry_{i}");
        let at = Point { x: 250_000, y: 20_000 + 14_000 * i as i64 };
        sch.labels.push(NetLabel { id: id.clone(), net: format!("LABEL{i}"), at, kind });
        sch.extras.label_spins.insert(id.clone(), spin);
        if !look.is_default() {
            sch.extras.label_looks.insert(id, look);
        }
    }
    (design, model)
}

const META: ExportMeta<'static> = ExportMeta { date: "2026-10-08", title: "label geometry" };

/// What the file says of each of the test's labels, by net name: its spin and look.
fn read_back(text: &str) -> Vec<(String, LabelSpin, LabelLook)> {
    let (design, _, _) = import_kicad_sch(text).expect("the file reads");
    let sch = design.schematic.unwrap();
    let mut out: Vec<(String, LabelSpin, LabelLook)> = sch
        .labels
        .iter()
        .filter(|l| l.net.starts_with("LABEL"))
        .map(|l| (l.net.clone(), sch.extras.label_spins.get(&l.id).copied().expect("an imported label has the spin the file gives it"), sch.extras.label_looks.get(&l.id).copied().unwrap_or_default()))
        .collect();
    out.sort_by_key(|(net, _, _)| net.trim_start_matches("LABEL").parse::<usize>().unwrap_or(0));
    out
}

fn expected() -> Vec<(String, LabelSpin, LabelLook)> {
    cases().into_iter().enumerate().map(|(i, (_, spin, look))| (format!("LABEL{i}"), spin, look)).collect()
}

#[test]
fn a_labels_spin_size_bold_and_italic_come_back_from_the_file_our_writer_wrote() {
    let (design, model) = drawn();
    let text = export_kicad_sch(&design, &model, &META).expect("the sheet exports");
    assert_eq!(read_back(&text), expected());
    // the spin is written as KiCad writes it: half a turn added to the angle of a text that ends at its anchor
    assert!(text.contains("(label \"LABEL2\" (at 250 48 180)") || text.contains("(label \"LABEL2\"\n"), "{}", text.lines().filter(|l| l.contains("LABEL2")).collect::<Vec<_>>().join("\n"));
    assert!(text.contains("(bold yes)") && text.contains("(italic yes)"));
}

#[test]
fn a_global_label_keeps_its_shape_with_its_spin() {
    let (design, model) = drawn();
    let text = export_kicad_sch(&design, &model, &META).expect("the sheet exports");
    let (back, _, _) = import_kicad_sch(&text).unwrap();
    let sch = back.schematic.unwrap();
    for (i, (kind, ..)) in cases().into_iter().enumerate() {
        let label = sch.labels.iter().find(|l| l.net == format!("LABEL{i}")).expect("the label");
        assert_eq!(label.kind, kind, "label {i}");
    }
}

fn find_kicad_cli() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).filter(|p| p.exists()) {
        return Some(p);
    }
    if let Ok(out) = Command::new("which").arg("kicad-cli").output() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    Some(PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli")).filter(|p| p.exists())
}

fn run(cli: &Path, args: &[&str], file: &Path) -> std::process::Output {
    Command::new(cli).args(args).arg(file).output().expect("run kicad-cli")
}

#[test]
fn kicad_loads_the_labels_and_writes_them_back_with_the_geometry_we_gave_them() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let (design, model) = drawn();
    let text = export_kicad_sch(&design, &model, &META).expect("the sheet exports");
    let dir = std::env::temp_dir().join(format!("eda_label_geometry_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("labels.kicad_sch");
    std::fs::write(&path, &text).unwrap();

    // it loads (an ERC run reads every label)
    let report = dir.join("erc.json");
    let erc = Command::new(&cli).args(["sch", "erc", "--format", "json", "-o"]).arg(&report).arg(&path).output().expect("run kicad-cli");
    assert!(erc.status.code().is_some_and(|c| c == 0 || c == 5) && report.exists(), "kicad-cli could not load the sheet: {}{}", String::from_utf8_lossy(&erc.stdout), String::from_utf8_lossy(&erc.stderr));

    // and KiCad's own writer says the same of each label
    let up = run(&cli, &["sch", "upgrade", "--force"], &path);
    assert!(up.status.success(), "kicad-cli sch upgrade: {}{}", String::from_utf8_lossy(&up.stdout), String::from_utf8_lossy(&up.stderr));
    let kicads = std::fs::read_to_string(&path).unwrap();
    assert_ne!(kicads, text, "KiCad wrote the file itself");
    assert_eq!(read_back(&kicads), expected(), "what KiCad kept of the labels");
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------------------------------------------------------------------------- fields

/// The derived sheet with the Value of R1 set in 2 mm bold italic, its name shown, and left out of Autoplace Fields; C1's fields autoplaced.
fn with_styled_field() -> (eda_model::ir::Design, ConstraintModel) {
    let model = model();
    let mut design = derive_schematic(&model, &EngineOptions::new(1, "hash")).unwrap();
    let sch = design.schematic.as_mut().unwrap();
    let layout = sch.field_layout.get_mut("R1").expect("the derivation stores where the fields of R1 are");
    let value = layout.iter_mut().find(|p| p.name == "Value").unwrap();
    value.size_um = 2_000;
    value.bold = true;
    value.italic = true;
    value.name_shown = true;
    value.no_autoplace = true;
    sch.extras.fields_autoplaced.remove("R1");
    sch.extras.fields_autoplaced.insert("C1".into(), eda_model::sch_extras::AutoplaceAlgo::Auto);
    (design, model)
}

/// The text of the `(property "name" ...)` form of the symbol `reference`, up to the next property.
fn property_of(text: &str, reference: &str, name: &str) -> String {
    let start = text.find(&format!("(property \"Reference\" \"{reference}\"")).expect("the symbol");
    let symbol = &text[text[..start].rfind("(symbol").unwrap()..];
    let from = symbol.find(&format!("(property \"{name}\"")).expect("the property");
    let rest = &symbol[from + 1..];
    let to = rest.find("(property").map(|i| i + 1).unwrap_or(rest.len());
    symbol[from..from + to].to_string()
}

#[test]
fn a_fields_size_look_shown_name_and_autoplace_flags_are_in_the_file_we_write() {
    let (design, model) = with_styled_field();
    let text = export_kicad_sch(&design, &model, &META).expect("the sheet exports");
    let value = property_of(&text, "R1", "Value");
    assert!(value.contains("(size 2 2)") && value.contains("(bold yes)") && value.contains("(italic yes)"), "{value}");
    assert!(value.contains("(show_name yes)") && value.contains("(do_not_autoplace yes)"), "{value}");
    let reference = property_of(&text, "R1", "Reference");
    assert!(reference.contains("(size 1.27 1.27)") && !reference.contains("bold") && !reference.contains("show_name"), "{reference}");
    // C1's fields were placed by Autoplace Fields; R1's were not
    let c1 = text.find("(property \"Reference\" \"C1\"").unwrap();
    let c1_symbol = &text[text[..c1].rfind("(symbol").unwrap()..c1];
    assert!(c1_symbol.contains("(fields_autoplaced yes)"), "{c1_symbol}");
    let r1 = text.find("(property \"Reference\" \"R1\"").unwrap();
    assert!(!text[text[..r1].rfind("(symbol").unwrap()..r1].contains("fields_autoplaced"));
}

#[test]
fn kicad_keeps_the_size_look_and_flags_of_a_field_when_it_writes_the_file_again() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let (design, model) = with_styled_field();
    let text = export_kicad_sch(&design, &model, &META).expect("the sheet exports");
    let dir = std::env::temp_dir().join(format!("eda_field_look_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fields.kicad_sch");
    std::fs::write(&path, &text).unwrap();
    let up = run(&cli, &["sch", "upgrade", "--force"], &path);
    assert!(up.status.success(), "kicad-cli sch upgrade: {}{}", String::from_utf8_lossy(&up.stdout), String::from_utf8_lossy(&up.stderr));
    let kicads = std::fs::read_to_string(&path).unwrap();
    let value = property_of(&kicads, "R1", "Value");
    assert!(value.contains("(size 2 2)") && value.contains("(bold yes)") && value.contains("(italic yes)"), "{value}");
    assert!(value.contains("(show_name yes)") && value.contains("(do_not_autoplace yes)"), "{value}");
    let c1 = kicads.find("(property \"Reference\" \"C1\"").unwrap();
    assert!(kicads[kicads[..c1].rfind("(symbol").unwrap()..c1].contains("(fields_autoplaced yes)"), "KiCad kept the autoplaced flag");
    let _ = std::fs::remove_dir_all(&dir);
}
