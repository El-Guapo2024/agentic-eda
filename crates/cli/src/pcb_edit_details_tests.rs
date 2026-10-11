//! The details of the PCB edit tools that kicad-cli judges (GAPS.md package P6): the file we write after Paste Special is one kicad-cli loads, and the
//! references it reads in it are the ones Paste Special gave.

use super::tests::{drc_counts, scratch, setup};
use super::{load, step, undo};
use eda_model::ir::Point;
use eda_ops::{Cmd, Domain, PasteMode};
use std::path::Path;

/// The position file kicad-cli writes for the board, as text.
fn positions(dir: &Path) -> String {
    let reply = crate::fab_api::pos(dir, serde_json::json!({ "format": "csv" }).to_string().as_bytes());
    assert_eq!(reply["ok"], true, "{reply}");
    let file = reply["files"][0].as_str().unwrap_or_else(|| panic!("no file in {reply}")).to_string();
    std::fs::read_to_string(dir.join(file)).unwrap()
}

fn copy_of(dir: &Path, ids: &[&str]) -> String {
    let reply = crate::clipboard_api::copy(dir, serde_json::json!({ "ids": ids, "reference": { "x": 0, "y": 0 } }).to_string().as_bytes());
    assert_eq!(reply["ok"], true, "{reply}");
    reply["text"].as_str().unwrap().to_string()
}

/// Slow tier (kicad-cli, `EDA_SLOW_TESTS=1`): after a plain Paste on the board the text came from, kicad-cli lists `U1` and `U2` twice, as it does after a Paste in
/// KiCad (the default keeps a taken reference); after Paste Special's "clear reference designators" it lists `REF**` for each pasted footprint; and the board with
/// the nets cleared still loads for kicad-cli's DRC. Undo takes each paste away again.
#[test]
fn kicad_cli_reads_the_references_a_paste_gave_and_the_board_after_paste_special_loads_for_its_drc() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    if eda_kicad_engine::find_cli().is_none() {
        eprintln!("kicad-cli not found; skipping");
        return;
    }
    let dir = scratch("paste_special_kicad_cli");
    setup(&dir);
    step(&dir, Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 2_000, y: 12_000 }, Point { x: 12_000, y: 12_000 }] }, false, "test").unwrap();
    // One row per footprint: the reference is the row's first column (the value, which the fixture sets to the reference, is the second).
    let count = |text: &str, reference: &str| text.lines().filter(|l| l.starts_with(&format!("\"{reference}\","))).count();

    let before = positions(&dir);
    assert_eq!((count(&before, "U1"), count(&before, "U2")), (1, 1), "{before}");
    let text = copy_of(&dir, &["U1", "U2"]);

    // A plain Paste: the references of the copy stay U1 and U2 (KiCad's `KEEP_ANNOTATIONS`).
    step(&dir, Cmd::PasteClipboard { text: text.clone(), at: Point { x: 10_000, y: 14_000 } }, false, "test").unwrap();
    let kept = positions(&dir);
    assert_eq!((count(&kept, "U1"), count(&kept, "U2")), (2, 2), "kicad-cli sees two U1 and two U2 after the paste:\n{kept}");
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(count(&positions(&dir), "U1"), 1, "Undo takes the paste away");

    // Paste Special, references cleared and nets cleared.
    step(&dir, Cmd::PasteSpecial { text: text.clone(), at: Point { x: 10_000, y: 14_000 }, mode: PasteMode::Remove, clear_nets: true }, false, "test").unwrap();
    let cleared = positions(&dir);
    assert_eq!(count(&cleared, "REF**"), 2, "both pasted footprints read REF** to kicad-cli:\n{cleared}");
    assert_eq!((count(&cleared, "U1"), count(&cleared, "U2")), (1, 1), "and the board's own are as they were");
    let (_, design, _) = load(&dir).unwrap();
    assert!(design.drawings.as_ref().unwrap().board_parts.iter().all(|p| p.pad_nets.is_empty()), "the pasted pads are on no net");
    // The file loads for the DRC (kicad-cli errors on a board it cannot parse).
    let counts = drc_counts(&dir);
    assert_eq!(counts.get("clearance").copied().unwrap_or(0), 0, "{counts:?}");

    // Unique references: kicad-cli lists U3 and U4.
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    step(&dir, Cmd::PasteSpecial { text, at: Point { x: 10_000, y: 14_000 }, mode: PasteMode::Unique, clear_nets: false }, false, "test").unwrap();
    let unique = positions(&dir);
    assert_eq!((count(&unique, "U3"), count(&unique, "U4")), (1, 1), "{unique}");
}
