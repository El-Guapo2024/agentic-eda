//! Intents that declare no `board.outline`, run with no flags.
//!
//! The default placer (`build`) needs a board before it places anything,
//! so it sizes one from the parts. From 2026-09-21, when it became the
//! default, until this test it could not: `seed_outline` asked
//! `fit_outline`, which only shrinks an outline that already exists, so
//! every example without one failed with `build_precondition`. The
//! annealer sizes its own board, which is why `--placer anneal` worked
//! and nothing noticed.

use std::path::PathBuf;
use std::process::Command;

/// `examples/<name>.yaml`, checked to declare no outline -- with one, these
/// tests would test nothing.
fn no_outline_example(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(format!("{name}.yaml"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(!text.contains("outline"), "{name}.yaml declares an outline; pick an example without one");
    path
}

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("eda_cli_no_outline_{}_{name}", std::process::id()))
}

/// The four-corner outline in `<dir>/design.json`.
fn outline(dir: &std::path::Path) -> Vec<serde_json::Value> {
    let text = std::fs::read_to_string(dir.join("design.json")).expect("design.json written");
    let design: serde_json::Value = serde_json::from_str(&text).expect("design.json parses");
    design["placement"]["outline"].as_array().cloned().unwrap_or_default()
}

#[test]
fn pipeline_with_no_flags_fits_a_board_for_an_intent_without_one() {
    for name in ["nc_pins", "star_net"] {
        let out = scratch(name);
        let run = Command::new(env!("CARGO_BIN_EXE_eda"))
            .arg("pipeline")
            .arg(no_outline_example(name))
            .arg("-o")
            .arg(&out)
            .args(["--seed", "0"])
            .output()
            .expect("run eda");
        let log = format!("{}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
        assert!(run.status.success(), "eda pipeline {name}.yaml failed:\n{log}");
        // The path under test ran: a default switched back to the annealer
        // would pass here without ever sizing a board for `build`.
        assert!(log.contains("place greedy:"), "{name}: the default placer was not build:\n{log}");
        assert_eq!(outline(&out).len(), 4, "{name}: no fitted outline in design.json");
    }
}

#[test]
fn board_new_fits_a_board_for_an_intent_without_one() {
    // `eda board new` seeds the shared board through the same function.
    let out = scratch("board_new_ldo");
    let run = Command::new(env!("CARGO_BIN_EXE_eda"))
        .args(["board", "new"])
        .arg(no_outline_example("ldo"))
        .arg("-o")
        .arg(&out)
        .output()
        .expect("run eda");
    assert!(run.status.success(), "eda board new failed:\n{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(outline(&out).len(), 4, "no fitted outline in the new board");
}
