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

// ---------------------------------------------------------------------------------------------------------------- Cleanup Tracks & Vias: Filter Items

/// Two dangling stubs on the two-part board of `setup`, one per net: GND from U1.1 and VCC from U1.2 run up to nothing. Returns their ids (GND's, VCC's).
fn two_stubs(dir: &Path) -> (String, String) {
    step(dir, Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 4_000, y: 5_000 }, Point { x: 4_000, y: 9_000 }] }, false, "test").unwrap();
    step(dir, Cmd::AddTrack { net: "VCC".into(), layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 6_000, y: 5_000 }, Point { x: 6_000, y: 9_000 }] }, false, "test").unwrap();
    let (_, design, _) = load(dir).unwrap();
    let tracks = &design.routing.as_ref().unwrap().tracks;
    let id_of = |net: &str| tracks.iter().find(|t| t.net == net).unwrap_or_else(|| panic!("no {net} track")).id.clone();
    (id_of("GND"), id_of("VCC"))
}

fn tracks_left(dir: &Path) -> Vec<String> {
    let (_, design, _) = load(dir).unwrap();
    design.routing.as_ref().map(|r| r.tracks.iter().map(|t| t.net.clone()).collect()).unwrap_or_default()
}

fn body(v: serde_json::Value) -> Vec<u8> {
    v.to_string().into_bytes()
}

/// The dialog's "Filter Items" box over HTTP: the preview and the apply both leave alone what the filter skips, Update PCB is one undo step, and a locked track is
/// never touched.
#[test]
fn cleanup_tracks_and_vias_honours_the_filter_items_box() {
    let dir = scratch("cleanup_filters");
    setup(&dir);
    let (gnd, vcc) = two_stubs(&dir);
    let removed = |req: serde_json::Value| -> u64 {
        let reply = crate::cleanup_api::preview(&dir, &body(req));
        assert_eq!(reply["ok"], true, "{reply}");
        reply["tracks_removed"].as_u64().unwrap()
    };
    let on = serde_json::json!({ "delete_dangling_tracks": true });
    let with = |extra: serde_json::Value| {
        let mut v = on.clone();
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        v
    };
    assert_eq!(removed(on.clone()), 2, "no filter: both stubs dangle");
    assert_eq!(removed(with(serde_json::json!({ "filter_net": "GND" }))), 1);
    assert_eq!(removed(with(serde_json::json!({ "filter_net": "NOPE" }))), 0);
    assert_eq!(removed(with(serde_json::json!({ "filter_layer": "B.Cu" }))), 0, "both stubs are on F.Cu");
    assert_eq!(removed(with(serde_json::json!({ "filter_layer": "F.Cu" }))), 2);
    assert_eq!(removed(with(serde_json::json!({ "filter_netclass": "Default" }))), 2, "no class claims either net: both are in Default");
    assert_eq!(removed(with(serde_json::json!({ "filter_netclass": "Power" }))), 0);
    assert_eq!(removed(with(serde_json::json!({ "selected_ids": [vcc.clone()] }))), 1);
    assert_eq!(removed(with(serde_json::json!({ "selected_ids": [] }))), 0, "'Selected items only' with nothing selected cleans nothing");
    assert_eq!(removed(with(serde_json::json!({ "filter_net": "", "filter_layer": "" }))), 2, "an empty name is a filter that is off");

    // Update PCB with the net filter: the GND stub goes, the VCC one stays, one undo brings it back.
    let reply = crate::cleanup_api::apply(&dir, &body(with(serde_json::json!({ "filter_net": "GND" }))));
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(tracks_left(&dir), vec!["VCC".to_string()]);
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(tracks_left(&dir).len(), 2, "Update PCB is one undo step");

    // A locked stub is never removed, with or without a filter.
    step(&dir, Cmd::SetLocked { ids: vec![gnd.clone()], locked: true }, false, "test").unwrap();
    assert_eq!(removed(on.clone()), 1);
    let reply = crate::cleanup_api::apply(&dir, &body(on));
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(tracks_left(&dir), vec!["GND".to_string()], "only the locked GND stub is left");
}

/// Slow tier (kicad-cli, `EDA_SLOW_TESTS=1`): kicad-cli's DRC counts the dangling stubs (`track_dangling`); after Update PCB with a net filter it counts the
/// ones the filter skipped, and after one without a filter none.
#[test]
fn kicad_cli_counts_the_stubs_the_filtered_cleanup_left() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    if eda_kicad_engine::find_cli().is_none() {
        eprintln!("kicad-cli not found; skipping");
        return;
    }
    let dir = scratch("cleanup_filters_kicad_cli");
    setup(&dir);
    two_stubs(&dir);
    let dangling = |dir: &Path| drc_counts(dir).get("track_dangling").copied().unwrap_or(0);
    assert_eq!(dangling(&dir), 2, "kicad-cli sees both stubs dangle: {:?}", drc_counts(&dir));
    let reply = crate::cleanup_api::apply(&dir, &body(serde_json::json!({ "delete_dangling_tracks": true, "filter_net": "GND" })));
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(dangling(&dir), 1, "the VCC stub is the one the filter skipped");
    let reply = crate::cleanup_api::apply(&dir, &body(serde_json::json!({ "delete_dangling_tracks": true })));
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(dangling(&dir), 0);
}
