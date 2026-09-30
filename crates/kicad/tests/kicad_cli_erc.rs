//! Optional integration test: only runs when `kicad-cli` is present on the
//! machine (checked via `which kicad-cli` or the macOS app bundle path).
//! `#[ignore]` by default — run with `cargo test -p eda-kicad --test
//! kicad_cli_erc -- --ignored`.
use std::path::{Path, PathBuf};
use std::process::Command;

use eda_engine::{derive_schematic, EngineOptions};
use eda_kicad::{export_kicad_sch, ExportMeta};
use eda_model::{ConstraintModel, Net, Part, Pin, PinKind};

fn find_kicad_cli() -> Option<PathBuf> {
    if let Ok(out) = Command::new("which").arg("kicad-cli").output() {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() {
                return Some(PathBuf::from(p));
            }
        }
    }
    let mac = PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli");
    if mac.exists() {
        return Some(mac);
    }
    None
}

fn ldo_model() -> ConstraintModel {
    let pin = |number: &str, name: &str, kind: PinKind| Pin { number: number.into(), name: Some(name.into()), kind };
    let part = |reference: &str, pins: Vec<Pin>| Part {
        reference: reference.into(),
        mpn: None,
        value: Some(format!("{reference}_val")),
        package: None,
        footprint: Some("Foo:Bar".into()),
        pins,
        body_um: None, symbol: None, datasheet: None,
        edge: None,
    };
    let net = |name: &str, pins: &[&str]| Net { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() };
    let u1 = part(
        "U1",
        vec![
            pin("1", "VIN", PinKind::Power),
            pin("2", "GND", PinKind::Ground),
            pin("3", "EN", PinKind::Signal),
            pin("4", "VOUT", PinKind::Power),
            pin("5", "NC", PinKind::Nc),
        ],
    );
    let cin = part("CIN", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]);
    let cout = part("COUT", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]);
    ConstraintModel {
        parts: vec![u1, cin, cout],
        nets: vec![
            net("VIN", &["U1.1", "CIN.1", "U1.3"]),
            net("VOUT", &["U1.4", "COUT.1"]),
            net("GND", &["U1.2", "CIN.2", "COUT.2"]),
        ],
        ..Default::default()
    }
}

#[test]
#[ignore]
fn kicad_cli_erc_parses_export() {
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let model = ldo_model();
    let design = derive_schematic(&model, &EngineOptions::new(1, "hash")).unwrap();
    let meta = ExportMeta { date: "2026-01-01", title: "LDO ERC test" };
    let text = export_kicad_sch(&design, &model, &meta).unwrap();

    let dir = std::env::temp_dir().join("eda_kicad_erc_test");
    std::fs::create_dir_all(&dir).unwrap();
    let sch_path = dir.join("ldo.kicad_sch");
    std::fs::write(&sch_path, &text).unwrap();
    let report_path = dir.join("erc.json");

    let out = Command::new(&cli)
        .args(["sch", "erc", "--format", "json", "--output"])
        .arg(&report_path)
        .arg(&sch_path)
        .output()
        .expect("failed to run kicad-cli");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    eprintln!("kicad-cli stdout:\n{stdout}\nstderr:\n{stderr}");

    // A parse failure prints to stderr/stdout and kicad-cli exits nonzero
    // *without* ever writing a report; ERC violations (warnings/errors in
    // content) still produce a report and are acceptable here — only a
    // failure to parse/load the file is not.
    assert!(report_path.exists(), "kicad-cli did not produce an ERC report — file likely failed to parse");
}

/// Runs `kicad-cli sch erc` on `sch_path` and returns every violation whose
/// `severity` is `"error"` (never `"warning"`) — the bar the task set: no
/// *errors* caused by the export itself. A library/footprint-not-registered
/// warning for a synthesized generic-IC symbol or a placeholder footprint
/// name is expected and not asserted against; only genuine connectivity
/// errors (dangling pins/wires, undriven power pins, conflicting pin
/// types, ...) are.
fn erc_errors(cli: &Path, sch_path: &Path) -> Vec<serde_json::Value> {
    let report_path = sch_path.with_file_name("erc.json");
    let out = Command::new(cli).args(["sch", "erc", "--format", "json", "--output"]).arg(&report_path).arg(sch_path).output().expect("failed to run kicad-cli");
    eprintln!("kicad-cli sch erc stdout:\n{}\nstderr:\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(report_path.exists(), "kicad-cli did not produce an ERC report for {} — file likely failed to parse", sch_path.display());
    let report: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&report_path).unwrap()).expect("parse ERC json report");
    let mut errors = Vec::new();
    if let Some(sheets) = report.get("sheets").and_then(|s| s.as_array()) {
        for sheet in sheets {
            if let Some(violations) = sheet.get("violations").and_then(|v| v.as_array()) {
                for v in violations {
                    if v.get("severity").and_then(|s| s.as_str()) == Some("error") {
                        errors.push(v.clone());
                    }
                }
            }
        }
    }
    if !errors.is_empty() {
        eprintln!("ERC errors on {}:", sch_path.display());
        for e in &errors {
            eprintln!("  {}", serde_json::to_string(e).unwrap_or_default());
        }
    }
    errors
}

/// Loads a real example intent, resolves its library symbols the same way
/// `eda board`/`eda schematic` do (real installed KiCad libraries first,
/// falling back to `eda_model::symbol::builtin`/a synthesized generic box),
/// derives the schematic, and exports it -- the full pipeline this task's
/// readable generator + writer + loader are ported for, run against a real
/// example file rather than a small hand-built fixture.
fn export_example(yaml_path: &Path, seed: u64, title: &str) -> (eda_model::ConstraintModel, PathBuf) {
    let text = std::fs::read_to_string(yaml_path).unwrap_or_else(|e| panic!("read {}: {e}", yaml_path.display()));
    let mut model: ConstraintModel = serde_yaml::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", yaml_path.display()));
    let root = eda_kicad::default_symbol_library_root();
    for w in eda_kicad::resolve_library_symbols(&mut model, &root) {
        eprintln!("symbol library: {w}");
    }
    let design = derive_schematic(&model, &EngineOptions::new(seed, title)).unwrap_or_else(|e| panic!("derive_schematic({title}): {e:?}"));
    let meta = ExportMeta { date: "2026-01-01", title };
    let text = export_kicad_sch(&design, &model, &meta).unwrap_or_else(|e| panic!("export_kicad_sch({title}): {e:?}"));

    let dir = std::env::temp_dir().join(format!("eda_kicad_erc_{title}"));
    std::fs::create_dir_all(&dir).unwrap();
    let sch_path = dir.join(format!("{title}.kicad_sch"));
    std::fs::write(&sch_path, &text).unwrap();
    (model, sch_path)
}

/// `examples/ldo.yaml`, the small LDO breakout the task names explicitly:
/// exported through the real generator/loader/writer and checked with real
/// `kicad-cli sch erc` for zero export-caused errors.
#[test]
#[ignore]
fn kicad_cli_erc_ldo_yaml_is_error_free() {
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().and_then(|p| p.parent()).expect("crates/kicad -> repo root");
    let yaml_path = repo_root.join("examples/ldo.yaml");
    let (_, sch_path) = export_example(&yaml_path, 3, "ldo");
    let errors = erc_errors(&cli, &sch_path);
    assert!(errors.is_empty(), "{} export-caused ERC error(s) on examples/ldo.yaml", errors.len());
}

/// `examples/ladder/l1_usb_mcu.yaml`, the ~17-part USB-powered MCU board
/// the task names explicitly as the other required check -- a real
/// resolvable regulator (`AMS1117-3.3`), a real generic connector-shaped
/// USB header, several passives, an unresolvable MCU (synthesized generic
/// box), and NC-marked unused GPIOs, all in one board.
#[test]
#[ignore]
fn kicad_cli_erc_l1_usb_mcu_is_error_free() {
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().and_then(|p| p.parent()).expect("crates/kicad -> repo root");
    let yaml_path = repo_root.join("examples/ladder/l1_usb_mcu.yaml");
    let (_, sch_path) = export_example(&yaml_path, 1, "l1_usb_mcu");
    let errors = erc_errors(&cli, &sch_path);
    assert!(errors.is_empty(), "{} export-caused ERC error(s) on examples/ladder/l1_usb_mcu.yaml", errors.len());
}

/// The `.kicad_sch` this project writes for a real example, read back by
/// `import_kicad_sch`, must itself re-export error-free -- proof the
/// reader's reconstruction (nets rebuilt from drawn connectivity, since a
/// `.kicad_sch` carries none explicitly) is faithful enough that a second
/// generation of the same file is not worse than the first.
#[test]
#[ignore]
fn kicad_cli_erc_survives_a_read_write_round_trip() {
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().and_then(|p| p.parent()).expect("crates/kicad -> repo root");
    let yaml_path = repo_root.join("examples/ldo.yaml");
    let (_, sch_path) = export_example(&yaml_path, 3, "ldo_roundtrip");
    let original_errors = erc_errors(&cli, &sch_path);
    assert!(original_errors.is_empty(), "the first export must already be error-free");

    let text = std::fs::read_to_string(&sch_path).unwrap();
    let (design, model, notes) = eda_kicad::import_kicad_sch(&text).expect("import_kicad_sch parses our own export");
    assert_eq!(notes.unresolved_symbols, 0);
    let meta = ExportMeta { date: "2026-01-01", title: "ldo_roundtrip2" };
    let re_exported = export_kicad_sch(&design, &model, &meta).expect("re-export the reconstructed design");
    let dir = std::env::temp_dir().join("eda_kicad_erc_ldo_roundtrip2");
    std::fs::create_dir_all(&dir).unwrap();
    let sch_path2 = dir.join("ldo_roundtrip2.kicad_sch");
    std::fs::write(&sch_path2, &re_exported).unwrap();
    let errors = erc_errors(&cli, &sch_path2);
    assert!(errors.is_empty(), "{} export-caused ERC error(s) after a read/write round trip", errors.len());
}
