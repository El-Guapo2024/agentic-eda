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
    // the net by its pins: the board's loader gives a track the KiCad name of its net (`/FB` for the label FB on the root, `Net-(C1-Pad1)` for a
    // net the old binary's stored schematic draws with no label) and keeps the nets by those names in `design.nets`; the old file held the intent's
    // names, whose pins are the intent's
    let pins_of = pins_by_net(d);
    let net = |v: &Value| -> String {
        let name = v.as_str().unwrap_or("");
        pins_of.get(name).cloned().unwrap_or_else(|| panic!("the net {name} is a net of the design or of its intent"))
    };
    let s = |p: (i64, i64)| format!("{} {}", p.0 * k, p.1 * k);
    for f in d["placement"]["footprints"].as_array().into_iter().flatten() {
        out.push(format!("fp {} {} {} {}", f["id"], s(pt(&f["at"])), f["rot"], f["side"]));
    }
    for p in d["placement"]["outline"].as_array().into_iter().flatten() {
        out.push(format!("outline {}", s(pt(p))));
    }
    for t in d["routing"]["tracks"].as_array().into_iter().flatten() {
        let pts: Vec<String> = t["pts"].as_array().unwrap().iter().map(|p| s(pt(p))).collect();
        out.push(format!("track {} {} {} {}", net(&t["net"]), t["layer"], t["width"].as_i64().unwrap() * k, pts.join(",")));
    }
    for v in d["routing"]["vias"].as_array().into_iter().flatten() {
        out.push(format!("via {} {} {} {}", net(&v["net"]), s(pt(&v["at"])), v["drill"].as_i64().unwrap() * k, v["diameter"].as_i64().unwrap() * k));
    }
    for z in d["routing"]["zones"].as_array().into_iter().flatten() {
        let pts: Vec<String> = z["outline"].as_array().unwrap().iter().map(|p| s(pt(p))).collect();
        out.push(format!("zone {} {} {}", net(&z["net"]), z["layer"], pts.join(",")));
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
        // (a wire's `net` is the drawing's note, which the loader leaves as the stored drawing has it: the wires join by their ends)
        out.push(format!("wire {}", pts.join(",")));
    }
    out.sort();
    out
}

/// Each net's pins, `REF.PIN` sorted and joined, by the net's name: the design's own nets when it keeps them, else its intent's
/// (`examples/opamp_filter.yaml`).
fn pins_by_net(d: &Value) -> std::collections::BTreeMap<String, String> {
    let nets: Vec<(String, Vec<String>)> = match d["nets"].as_array() {
        Some(nets) => nets.iter().map(|n| (n["name"].as_str().unwrap().to_string(), n["pins"].as_array().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect())).collect(),
        None => {
            let intent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/opamp_filter.yaml");
            let model: serde_yaml::Value = serde_yaml::from_str(&std::fs::read_to_string(intent).unwrap()).unwrap();
            model["nets"].as_sequence().unwrap().iter().map(|n| (n["name"].as_str().unwrap().to_string(), n["pins"].as_sequence().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect())).collect()
        }
    };
    nets.into_iter()
        .map(|(name, mut pins)| {
            pins.sort();
            (name, pins.join(" "))
        })
        .collect()
}

/// `lengths` of two designs are the same, what differs shown alone.
fn assert_same(now: Vec<String>, was: Vec<String>, what: &str) {
    let only_now: Vec<&String> = now.iter().filter(|l| !was.contains(l)).collect();
    let only_was: Vec<&String> = was.iter().filter(|l| !now.contains(l)).collect();
    assert!(now == was, "{what}: only now {only_now:#?}\nonly before {only_was:#?}");
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
    assert_same(lengths(&undone, 1), lengths(&before_text, 1000), "the snapshot before the text, x1000");

    // Redo brings it back: the board as the old binary left it, x1000, the text's size included.
    eda(&["board", "redo", "-C", dir.to_str().unwrap()]);
    let redone = json(&dir.join("design.json"));
    assert_same(lengths(&redone, 1), lengths(&old, 1000), "the old board, every length x1000");
    let text = &redone["drawings"]["texts"][0];
    assert_eq!((pt(&text["at"]), text["size_nm"].as_i64(), text["stroke_width"].as_i64()), ((5_000_000, 3_000_000), Some(1_000_000), Some(100_000)));
    assert_eq!(redone["routing"]["zones"][0]["outline"][2], serde_json::json!({ "x": 40_000_000, "y": 30_000_000 }));

    // Two more undos go back past the zone to where the old binary started, and a redo comes forward again: no step is lost, none mixes
    // micrometres with nanometres.
    eda(&["board", "undo", "-C", dir.to_str().unwrap()]);
    eda(&["board", "undo", "-C", dir.to_str().unwrap()]);
    let before_zone = json(&dir.join("design.json"));
    assert!(before_zone["routing"]["zones"].as_array().is_none_or(|z| z.is_empty()), "the zone is undone");
    assert_same(lengths(&before_zone, 1), lengths(&snaps[1], 1000), "the snapshot before the zone, x1000");
    eda(&["board", "redo", "-C", dir.to_str().unwrap()]);
    assert_same(lengths(&json(&dir.join("design.json")), 1), lengths(&snaps[2], 1000), "the snapshot, x1000");

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
    let mut new: Value = serde_json::from_str(out[start..].trim()).unwrap_or_else(|e| panic!("{e}: {out}"));
    let old = json(&fixture().join("old_board_um.drc.json"));
    // U1, a `SOIC-8` package part, carries the installed `Package_SO:SOIC-8_3.9x4.9mm_P1.27mm` whole since R6 (PARITY-pcb.md section 53), its
    // reference field where the library puts it, over C2's silkscreen and its pad 2: two violations the old binary's built-in field did not have,
    // which say nothing of the units. They are left out, and every other violation is the old binary's.
    let library_field = |v: &Value| v["items"].as_array().into_iter().flatten().any(|i| i["description"] == "Reference field of U1");
    let dropped: Vec<Value> = new["violations"].as_array().into_iter().flatten().filter(|v| library_field(v)).cloned().collect();
    assert_eq!(dropped.iter().map(|v| v["type"].as_str().unwrap_or("").to_string()).collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>(), ["silk_over_copper", "silk_overlap"], "{dropped:#?}");
    assert_eq!(dropped.len(), 2, "{dropped:#?}");
    if let Some(v) = new["violations"].as_array_mut() {
        v.retain(|x| !library_field(x));
    }
    for d in &dropped {
        let k = d["type"].as_str().unwrap();
        new["counts"][k] = Value::from(new["counts"][k].as_i64().unwrap() - 1);
    }
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
