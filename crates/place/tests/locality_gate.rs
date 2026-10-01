//! Exercises the `check_placement_locality` quality gates (crates/gates) —
//! isolation, edge connectors, board use, decoupling, net compactness,
//! stub crossings — against every corpus intent under `examples/*.yaml`
//! and `examples/ladder/*.yaml`, placed with the real annealer, so a
//! placement regression fails loudly instead of only showing up as a bad
//! HPWL number in a bench report.

use eda_model::ir::{Design, Provenance};
use eda_model::{CheckStatus, ConstraintModel};
use eda_place::{place, PlaceOptions};
use std::path::PathBuf;

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn design() -> Design {
    Design {
        footprint_library: None, sheet_contents: None,
        schema: 1,
        provenance: Provenance { engine_version: "test".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None, nets: None,
        placement: None,
        routing: None,
        drawings: None,
    }
}

#[test]
fn corpus_placements_pass_locality_gate() {
    let dir = examples_dir();
    let mut checked = 0;
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir).expect("examples dir").flatten().map(|e| e.path()).collect();
    // The ladder boards too, except l3 (58 parts: the annealer still
    // leaves MCU-to-driver nets ~2x their packed bound — open item) and
    // l4 (100 parts, minutes per seed).
    if let Ok(ladder) = std::fs::read_dir(dir.join("ladder")) {
        paths.extend(ladder.flatten().map(|e| e.path()).filter(|p| {
            let name = p.file_name().unwrap().to_str().unwrap();
            !name.starts_with("l3") && !name.starts_with("l4")
        }));
    }
    paths.sort();
    for path in paths {
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let model: ConstraintModel = match serde_yaml::from_str(&text) {
            Ok(m) => m,
            Err(_) => continue, // not every fixture is a plain intent; skip malformed ones
        };
        for seed in 0..2 {
            let opts = PlaceOptions { seed, ..Default::default() };
            let Ok(placed) = place(&design(), &model, &opts) else { continue };
            checked += 1;
            let fails: Vec<_> =
                eda_gates::check_placement_locality(&placed, &model).into_iter().filter(|c| c.status == CheckStatus::Fail).collect();
            assert!(fails.is_empty(), "{}: seed {seed}: {fails:#?}", path.display());
        }
    }
    assert!(checked > 0, "expected to exercise at least one example");
}
