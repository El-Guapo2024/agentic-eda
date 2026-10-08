//! The module sheets we write are a KiCad project kicad-cli can read whole: the root, one file per sheet, instance paths that name
//! each sheet, and no mismatch between a sheet pin and the hierarchical label it stands for. Skipped when kicad-cli is not installed.

use eda_engine::{derive_schematic_modules, EngineOptions};
use eda_model::ConstraintModel;

fn model_of(file: &str) -> ConstraintModel {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(file);
    serde_yaml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// kicad-cli's checks that speak about the hierarchy or about a symbol that never got an annotation on its sheet.
const HIERARCHY_CHECKS: &[&str] = &["hier_label_mismatch", "unannotated", "duplicate_reference", "duplicate_sheet_names", "different_unit_footprint", "undefined_netclass", "label_dangling", "pin_not_connected", "power_pin_not_driven", "unresolved_variable"];

/// Every example design, as module sheets, through kicad-cli: what it finds, listed. Run by hand (it starts kicad-cli once per
/// example): `cargo test -p eda-kicad-engine --test hier_erc -- --ignored --nocapture`.
#[test]
#[ignore]
fn every_example_as_module_sheets_in_kicad_cli() {
    let Some(_) = eda_kicad_engine::find_cli() else { return };
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "yaml")).collect();
    files.extend(std::fs::read_dir(dir.join("ladder")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "yaml")));
    files.sort();
    for f in files {
        let Ok(model) = serde_yaml::from_str::<ConstraintModel>(&std::fs::read_to_string(&f).unwrap()) else { continue };
        let Ok(design) = derive_schematic_modules(&model, &EngineOptions::new(1, "t")) else { continue };
        let sheets = design.sheet_contents.as_ref().map(|c| c.len()).unwrap_or(1);
        match eda_kicad_engine::with_scratch(|d| eda_kicad_engine::erc(&design, &model, d)) {
            Ok(r) => {
                let mut counts = std::collections::BTreeMap::<String, usize>::new();
                for v in &r.violations {
                    *counts.entry(format!("{} ({})", v.kind, v.severity)).or_default() += 1;
                }
                println!("{}: {sheets} sheet(s): {counts:?}", f.file_name().unwrap().to_string_lossy());
            }
            Err(e) => println!("{}: {e:?}", f.file_name().unwrap().to_string_lossy()),
        }
    }
}

#[test]
fn the_module_sheets_load_in_kicad_cli_and_its_erc_finds_no_hierarchy_mismatch() {
    let Some(_) = eda_kicad_engine::find_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let model = model_of("mcu_board_30plus.yaml");
    let design = derive_schematic_modules(&model, &EngineOptions::new(1, "t")).unwrap();
    assert_eq!(design.sheet_contents.as_ref().map(|c| c.len()), Some(3));
    let report = eda_kicad_engine::with_scratch(|dir| eda_kicad_engine::erc(&design, &model, dir)).expect("kicad-cli loads the hierarchy and runs ERC");
    let found: Vec<(String, String, String)> = report.violations.iter().map(|v| (v.kind.clone(), v.severity.clone(), v.description.clone())).collect();
    for v in &report.violations {
        println!("ERC {} {} at {:?}", v.kind, v.severity, v.items.iter().map(|i| (i.description.clone(), i.id.clone(), i.pos)).collect::<Vec<_>>());
    }
    let bad: Vec<&(String, String, String)> = found.iter().filter(|(k, sev, _)| HIERARCHY_CHECKS.contains(&k.as_str()) && sev != "exclusion").collect();
    assert!(bad.is_empty(), "kicad-cli reports {bad:?}");
}
