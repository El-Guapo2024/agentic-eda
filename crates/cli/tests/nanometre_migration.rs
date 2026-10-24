//! A board folder written by the micrometre binary (main at be562f2, before docs/NANOMETRES.md) is read by this one: every length x1000,
//! undo and redo go back and forth across the migration in nanometres, a read writes nothing, and (slow tier, the real kicad-cli) the
//! board's DRC is the one the old binary got.
//!
//! The fixture (`tests/fixtures/old_board_um/`) is `examples/opamp_filter.yaml` placed and routed by `eda pipeline`, put in a folder by
//! `eda board new`, then `eda board zone add --net GND --layer B.Cu --pts "0,0 40,0 40,30 0,30"` and `eda board text add --content "REV A"
//! --at 5,3 --layer F.SilkS`, all by the old binary: `design.json` is schema 1, `board.json` has `snap_um`/`spacing_um`, and
//! `.history/undo/` holds the old binary's two board snapshots (before the zone, before the text) and the outline it started from.
//! `old_board_um.drc.json` is that binary's `eda board drc` of the folder.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn eda(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_eda")).args(args).output().expect("run eda");
    let log = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "eda {}:\n{log}", args.join(" "));
    log
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let t = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &t);
        } else {
            std::fs::copy(e.path(), &t).unwrap();
        }
    }
}

/// A scratch copy of the old board, its intent pointed at this checkout's example (the old binary wrote its own absolute path).
fn old_board(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("eda_nm_migration_{}_{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    copy_dir(&fixture().join("old_board_um"), &d);
    let mut meta: Value = serde_json::from_str(&std::fs::read_to_string(d.join("board.json")).unwrap()).unwrap();
    let intent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/opamp_filter.yaml").canonicalize().unwrap();
    meta["intent"] = Value::String(intent.display().to_string());
    std::fs::write(d.join("board.json"), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
    d
}

fn json(p: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn pt(v: &Value) -> (i64, i64) {
    (v["x"].as_i64().unwrap(), v["y"].as_i64().unwrap())
}

/// The lengths of a design this test holds to the old file: every footprint's place (its turn and side as they were), every track's
/// points and width, every via's place, drill and diameter, the zones' outlines, the texts' place and size, the board outline, the
/// schematic's symbols and wires.
fn lengths(d: &Value, k: i64) -> Vec<String> {
    let mut out = Vec::new();
    let s = |p: (i64, i64)| format!("{} {}", p.0 * k, p.1 * k);
    for f in d["placement"]["footprints"].as_array().into_iter().flatten() {
        out.push(format!("fp {} {} {} {}", f["id"], s(pt(&f["at"])), f["rot"], f["side"]));
    }
    for p in d["placement"]["outline"].as_array().into_iter().flatten() {
        out.push(format!("outline {}", s(pt(p))));
    }
    for t in d["routing"]["tracks"].as_array().into_iter().flatten() {
        let pts: Vec<String> = t["pts"].as_array().unwrap().iter().map(|p| s(pt(p))).collect();
        out.push(format!("track {} {} {} {}", t["net"], t["layer"], t["width"].as_i64().unwrap() * k, pts.join(",")));
    }
    for v in d["routing"]["vias"].as_array().into_iter().flatten() {
        out.push(format!("via {} {} {} {}", v["net"], s(pt(&v["at"])), v["drill"].as_i64().unwrap() * k, v["diameter"].as_i64().unwrap() * k));
    }
    for z in d["routing"]["zones"].as_array().into_iter().flatten() {
        let pts: Vec<String> = z["outline"].as_array().unwrap().iter().map(|p| s(pt(p))).collect();
        out.push(format!("zone {} {} {}", z["net"], z["layer"], pts.join(",")));
    }
    for t in d["drawings"]["texts"].as_array().into_iter().flatten() {
        let size = t.get("size_nm").or_else(|| t.get("size_um")).and_then(Value::as_i64).unwrap();
        out.push(format!("text {} {} {} {}", t["content"], s(pt(&t["at"])), size * k, t["stroke_width"].as_i64().unwrap() * k));
    }
    for y in d["schematic"]["symbols"].as_array().into_iter().flatten() {
        out.push(format!("symbol {} {} {}", y["id"], s(pt(&y["at"])), y["rot"]));
    }
    for w in d["schematic"]["wires"].as_array().into_iter().flatten() {
        let pts: Vec<String> = w["pts"].as_array().unwrap().iter().map(|p| s(pt(p))).collect();
        out.push(format!("wire {} {}", w["net"], pts.join(",")));
    }
    out.sort();
    out
}

/// The old binary's snapshots in `.history/undo`, oldest first.
fn undo_snapshots(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir.join(".history/undo")).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    v.sort();
    v
}

#[test]
fn a_board_the_micrometre_binary_wrote_is_read_in_nanometres_and_undo_crosses_the_migration() {
    let dir = old_board("undo");
    let old = json(&dir.join("design.json"));
    assert_eq!(old["schema"], 1, "the fixture is the old binary's");
    let old_bytes = std::fs::read(dir.join("design.json")).unwrap();
    // read now: undo and redo move the snapshot files between the stacks
    let snaps: Vec<Value> = undo_snapshots(&dir).iter().map(|p| json(p)).collect();
    assert_eq!(snaps.len(), 3, "the outline it started from, before the zone, before the text");
    let before_text = snaps[2].clone();

    // A read writes nothing: the folder stays the old binary's until something is changed.
    eda(&["board", "status", "-C", dir.to_str().unwrap()]);
    assert_eq!(std::fs::read(dir.join("design.json")).unwrap(), old_bytes, "a read leaves design.json as it was");

    // Undo takes the text away: the old snapshot comes back in nanometres.
    eda(&["board", "undo", "-C", dir.to_str().unwrap()]);
    let undone = json(&dir.join("design.json"));
    assert_eq!(undone["schema"], 2, "what is written is nanometres");
    assert!(undone["drawings"]["texts"].as_array().is_none_or(|t| t.is_empty()), "the text is undone: {}", undone["drawings"]);
    assert_eq!(lengths(&undone, 1), lengths(&before_text, 1000), "the snapshot before the text, x1000");

    // Redo brings it back: the board as the old binary left it, x1000, the text's size included.
    eda(&["board", "redo", "-C", dir.to_str().unwrap()]);
    let redone = json(&dir.join("design.json"));
    assert_eq!(lengths(&redone, 1), lengths(&old, 1000), "the old board, every length x1000");
    let text = &redone["drawings"]["texts"][0];
    assert_eq!((pt(&text["at"]), text["size_nm"].as_i64(), text["stroke_width"].as_i64()), ((5_000_000, 3_000_000), Some(1_000_000), Some(100_000)));
    assert_eq!(redone["routing"]["zones"][0]["outline"][2], serde_json::json!({ "x": 40_000_000, "y": 30_000_000 }));

    // Two more undos go back past the zone to where the old binary started, and a redo comes forward again: no step is lost, none mixes
    // micrometres with nanometres.
    eda(&["board", "undo", "-C", dir.to_str().unwrap()]);
    eda(&["board", "undo", "-C", dir.to_str().unwrap()]);
    let before_zone = json(&dir.join("design.json"));
    assert!(before_zone["routing"]["zones"].as_array().is_none_or(|z| z.is_empty()), "the zone is undone");
    assert_eq!(lengths(&before_zone, 1), lengths(&snaps[1], 1000), "the snapshot before the zone, x1000");
    eda(&["board", "redo", "-C", dir.to_str().unwrap()]);
    assert_eq!(lengths(&json(&dir.join("design.json")), 1), lengths(&snaps[2], 1000));

    // board.json is written with its grid in nanometres the first time it is written; read, the old keys are x1000.
    let meta = json(&dir.join("board.json"));
    assert!(meta.get("snap_um").is_some() || meta["snap_nm"] == 100_000, "{meta}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Slow tier (kicad-cli, `EDA_SLOW_TESTS=1`): the DRC of the old board read by this binary is the old binary's, violation for violation,
/// at the same places (kicad-cli's own nanometres).
#[test]
fn the_old_boards_drc_is_the_one_the_old_binary_got() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let dir = old_board("drc");
    let out = eda(&["board", "drc", "-C", dir.to_str().unwrap()]);
    let start = out.find('{').expect("a JSON report");
    let new: Value = serde_json::from_str(out[start..].trim()).unwrap_or_else(|e| panic!("{e}: {out}"));
    let old = json(&fixture().join("old_board_um.drc.json"));
    assert_eq!(new["counts"], old["counts"], "the same violations, kind by kind");
    let places = |r: &Value| {
        let mut v: Vec<String> = r["violations"].as_array().into_iter().flatten().map(|x| format!("{} {}", x["type"], x["positions_nm"])).collect();
        v.sort();
        v
    };
    assert_eq!(places(&new), places(&old), "at the same places");
    assert_eq!(new["unconnected_items"], old["unconnected_items"]);
    let _ = std::fs::remove_dir_all(&dir);
}
