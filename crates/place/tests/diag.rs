//! Diagnostic: `DIAG_RUNS=<dir> cargo test -p eda-place --test diag -- --ignored --nocapture`
//! prints every placement-quality check for each `<dir>/<name>/design.json`
//! against `examples/<name>.yaml` (or `examples/ladder/<name>.yaml`).
use eda_model::{ir::Design, ConstraintModel};
use std::path::PathBuf;

#[test]
#[ignore]
fn print_quality_checks() {
    let Ok(runs) = std::env::var("DIAG_RUNS") else { return };
    let ex = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut dirs: Vec<_> = std::fs::read_dir(&runs).unwrap().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for d in dirs {
        let name = d.file_name().unwrap().to_str().unwrap().to_string();
        let Ok(dj) = std::fs::read_to_string(d.join("design.json")) else { continue };
        let Ok(design) = serde_json::from_str::<Design>(&dj) else { continue };
        let yaml = [ex.join(format!("{name}.yaml")), ex.join("ladder").join(format!("{name}.yaml"))].into_iter().find(|p| p.exists());
        let Some(yaml) = yaml else { continue };
        let model: ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(yaml).unwrap()).unwrap();
        println!("== {name}");
        for c in eda_gates::check_placement_locality(&design, &model) {
            println!("  {:?} {} {} {}", c.status, c.check, c.location.unwrap_or_default(), c.hint.unwrap_or_default());
        }
    }
}

/// `DIAG_PLACE=<intent.yaml> [DIAG_SEED=n] cargo test -p eda-place --test diag place_one -- --ignored --nocapture`
/// prints every footprint pose (and the failure list) for one intent.
#[test]
#[ignore]
fn place_one() {
    let Ok(path) = std::env::var("DIAG_PLACE") else { return };
    let seed: u64 = std::env::var("DIAG_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let model: ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let design = Design {
        schema: 1,
        provenance: eda_model::ir::Provenance { engine_version: "diag".into(), intent_hash: "x".into(), seed, stage_hashes: vec![] },
        schematic: None,
        placement: None,
        routing: None,
        drawings: None,
    };
    match eda_place::place(&design, &model, &eda_place::PlaceOptions { seed, ..Default::default() }) {
        Ok(d) => {
            let pl = d.placement.unwrap();
            println!("outline {:?}", pl.outline);
            for fp in pl.footprints {
                let part = model.part(&fp.id).unwrap();
                let cy = eda_model::footprint::placed_courtyard(&model, part, &fp).unwrap();
                println!("  {:4} at ({:6},{:6}) rot {:3} courtyard {:?}", fp.id, fp.at.x, fp.at.y, fp.rot / 1000, cy);
            }
        }
        Err(f) => println!("FAILED {f:#?}"),
    }
}
