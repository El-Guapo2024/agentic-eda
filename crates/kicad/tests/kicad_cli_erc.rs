//! Optional integration test: only runs when `kicad-cli` is present on the
//! machine (checked via `which kicad-cli` or the macOS app bundle path).
//! `#[ignore]` by default — run with `cargo test -p eda-kicad --test
//! kicad_cli_erc -- --ignored`.
use std::path::PathBuf;
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
