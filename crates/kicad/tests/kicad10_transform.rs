//! KiCad 10's footprint `(transform ...)` (file version 20260616 on): a board kicad-cli has upgraded imports as the board it came from. The footprints
//! sit where they sat, turned as they were, and their own zones -- stored in the footprint's frame by the new format -- land where the old file put
//! them on the board (`crates/kicad/src/fp_transform.rs`).
//!
//! Slow: it needs kicad-cli 10 (`pcb upgrade` writes the new format) and KiCad's QA boards (`KICAD_SRC_DIR`). Run with
//! `EDA_SLOW_TESTS=1 KICAD_SRC_DIR=... cargo test -p eda-kicad --test kicad10_transform -- --nocapture`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use eda_kicad::import_kicad_pcb;
use serde_json::Value;

fn slow_tests_off() -> bool {
    let off = std::env::var_os("EDA_SLOW_TESTS").is_none();
    if off {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
    }
    off
}

fn kicad_cli() -> Option<PathBuf> {
    if let Ok(out) = Command::new("which").arg("kicad-cli").output() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    let mac = PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli");
    mac.exists().then_some(mac)
}

fn file_version(text: &str) -> i64 {
    text.split("(version ").nth(1).and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next()).and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn um(v: &Value) -> i64 {
    v.as_i64().unwrap_or_else(|| v.as_f64().unwrap_or(0.0).round() as i64)
}

/// Every footprint's place: reference -> (x, y, rotation, side).
fn poses(design: &Value) -> BTreeMap<String, (i64, i64, i64, String)> {
    let mut out = BTreeMap::new();
    for fp in design["placement"]["footprints"].as_array().into_iter().flatten() {
        let id = fp["id"].as_str().unwrap_or("").to_string();
        out.insert(id, (um(&fp["at"]["x"]), um(&fp["at"]["y"]), um(&fp["rot"]), fp["side"].to_string()));
    }
    out
}

/// The zones that belong to a footprint: (footprint, layer, outline) in file order.
fn footprint_zones(design: &Value) -> Vec<(String, String, Vec<(i64, i64)>)> {
    let mut out = Vec::new();
    for z in design["routing"]["zones"].as_array().into_iter().flatten() {
        let Some(parent) = z["parent_footprint"].as_str() else { continue };
        let pts = z["outline"].as_array().into_iter().flatten().map(|p| (um(&p["x"]), um(&p["y"]))).collect();
        out.push((parent.to_string(), z["layer"].as_str().unwrap_or("").to_string(), pts));
    }
    out
}

fn upgraded(cli: &Path, board: &Path, dir: &Path) -> String {
    let copy = dir.join(board.file_name().unwrap());
    std::fs::copy(board, &copy).unwrap();
    let ok = Command::new(cli).args(["pcb", "upgrade", "--force"]).arg(&copy).status().map(|s| s.success()).unwrap_or(false);
    assert!(ok, "kicad-cli pcb upgrade failed on {}", board.display());
    std::fs::read_to_string(&copy).unwrap()
}

#[test]
fn a_board_upgraded_by_kicad_10_imports_as_the_board_it_came_from() {
    if slow_tests_off() {
        return;
    }
    let (Some(src), Some(cli)) = (std::env::var_os("KICAD_SRC_DIR"), kicad_cli()) else {
        eprintln!("skipped: needs KICAD_SRC_DIR and kicad-cli");
        return;
    };
    let qa = PathBuf::from(src).join("qa/data/pcbnew");
    let dir = std::env::temp_dir().join(format!("eda-kicad10-transform-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Boards with zones inside turned footprints (keep-outs and pours), and many turned footprints.
    for name in ["issue5320", "issue14549", "severities", "issue14559"] {
        let board = qa.join(format!("{name}.kicad_pcb"));
        if !board.exists() {
            eprintln!("{name}: not in this KiCad source; skipped");
            continue;
        }
        let old_text = std::fs::read_to_string(&board).unwrap();
        let new_text = upgraded(&cli, &board, &dir);
        if file_version(&new_text) < 20260616 {
            eprintln!("skipped: kicad-cli writes file version {}, older than the footprint transform", file_version(&new_text));
            return;
        }
        assert!(new_text.contains("(transform"), "{name}: the upgraded file has no (transform ...)");
        let (old, _, _) = import_kicad_pcb(&old_text).unwrap_or_else(|e| panic!("{name} (as shipped): {e:?}"));
        let (new, _, notes) = import_kicad_pcb(&new_text).unwrap_or_else(|e| panic!("{name} (upgraded): {e:?}"));
        assert!(notes.footprints_scaled.is_empty(), "{name}: kicad-cli's upgrade scales no footprint");
        let (old, new) = (serde_json::to_value(&old).unwrap(), serde_json::to_value(&new).unwrap());
        let (po, pn) = (poses(&old), poses(&new));
        assert!(!po.is_empty(), "{name}: no footprints read");
        assert_eq!(po, pn, "{name}: the footprints moved or turned in the upgrade");
        // A zone on several layers reads as one zone per layer, in the order the file lists them; compare them as sets.
        let (mut zo, mut zn) = (footprint_zones(&old), footprint_zones(&new));
        zo.sort();
        zn.sort();
        assert_eq!(zo.len(), zn.len(), "{name}: footprint zone count");
        for ((fo, lo, a), (fne, ln, b)) in zo.iter().zip(&zn) {
            assert_eq!((fo, lo, a.len()), (fne, ln, b.len()), "{name}: footprint zone of {fo}");
            for (p, q) in a.iter().zip(b) {
                // The library frame and back: a nanometre of rounding each way is all that may differ.
                assert!((p.0 - q.0).abs() <= 1 && (p.1 - q.1).abs() <= 1, "{name}: a zone point of {fo} moved from {p:?} to {q:?}");
            }
        }
        eprintln!("{name}: {} footprints and {} footprint zones read the same from both files", po.len(), zo.len());
    }
    let _ = std::fs::remove_dir_all(&dir);
}
