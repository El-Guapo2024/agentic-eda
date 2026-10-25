//! `eda place` (the greedy build and its repair loop, the default placer) on every example, at the seeds the examples are placed with: no placement
//! gate fails.
//!
//! The corpus tests (`crates/place/tests/locality_gate.rs`, `crates/bench/tests/corpus.rs`) place with the annealer, so nothing watched the default
//! placer until a part given by `package:` began to carry the installed KiCad footprint whole: `R_0603_1608Metric`'s courtyard is 5 µm wider than the
//! built-in one's and the SOT-223's 0.3 mm taller, and `mcu_board_30plus` came out with its PA stubs crossed on every seed and `ldo_proximity_heavy`
//! with C1 9 mm from C3 under a 3 mm rule. The parts come from the libraries installed on the machine, as `eda place` finds them.

use std::path::PathBuf;
use std::process::Command;

fn examples() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "yaml")).collect();
    // the ladder's first two boards (l3 and l4 take minutes a seed in a debug build)
    out.extend(std::fs::read_dir(dir.join("ladder")).unwrap().flatten().map(|e| e.path()).filter(|p| {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        name.ends_with(".yaml") && (name.starts_with("l1") || name.starts_with("l2"))
    }));
    out.sort();
    out
}

/// The failing placement gates of `eda place <example> --seed <seed>`, by the lines it prints (`FAIL <check> @ ..`), or what went wrong.
fn failures(example: &PathBuf, seed: u64) -> Vec<String> {
    let name = example.file_stem().unwrap().to_string_lossy().to_string();
    let out = std::env::temp_dir().join(format!("eda_place_examples_{}_{name}_{seed}", std::process::id()));
    let run = Command::new(env!("CARGO_BIN_EXE_eda")).arg("place").arg(example).arg("-o").arg(&out).args(["--seed", &seed.to_string()]).output().expect("eda runs");
    let _ = std::fs::remove_dir_all(&out);
    let text = format!("{}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    if !text.contains("placement gates:") {
        return vec![format!("no placement gates ran: {}", text.lines().last().unwrap_or(""))];
    }
    text.lines().filter(|l| l.starts_with("FAIL ")).map(|l| l.chars().take(200).collect()).collect()
}

#[test]
fn every_example_places_with_no_placement_gate_failing_at_every_seed() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let runs: Vec<(PathBuf, u64)> = examples().into_iter().flat_map(|e| (0..3).map(move |s| (e.clone(), s))).collect();
    assert!(runs.len() >= 40, "only {} runs", runs.len());
    // three at a time: the box is shared
    let mut bad: Vec<String> = Vec::new();
    for batch in runs.chunks(3) {
        let found: Vec<(String, Vec<String>)> = std::thread::scope(|s| {
            let handles: Vec<_> = batch.iter().map(|(e, seed)| s.spawn(move || (format!("{} seed {seed}", e.file_stem().unwrap().to_string_lossy()), failures(e, *seed)))).collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for (what, f) in found {
            if !f.is_empty() {
                bad.push(format!("{what}:\n  {}", f.join("\n  ")));
            }
        }
    }
    assert!(bad.is_empty(), "{} run(s) fail placement gates:\n{}", bad.len(), bad.join("\n"));
}
