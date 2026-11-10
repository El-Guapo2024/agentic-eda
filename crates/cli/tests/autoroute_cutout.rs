//! The autoroute (the studio's Route, `eda board route`: the FreeRouting port, `eda_freeroute::design::route_design`) on a board with a
//! cutout (UI-LEFT R4, GAPS item 27). The board is `examples/through_hole_headers.yaml` placed so that two nets run straight across the
//! middle of the board, where a rectangle on Edge.Cuts cuts a window out of it.
//!
//! Slow tier (`EDA_SLOW_TESTS=1`, kicad-cli): kicad-cli's DRC is the judge. Routed with the cutout on the board, the board has no
//! `copper_edge_clearance` violation, no clearance, short, crossing or hole violation and no unconnected item; routed first and cut after
//! (what the autoroute made of the board when it routed inside the outline's summary polygon and did not see the cutout), the same board
//! has `copper_edge_clearance` violations at the cutout, so the check is not empty. kicad-cli's own Specctra export of the board carries
//! the cutout as KiCad's `fillBOUNDARY` writes it, `(keepout "" (polygon signal 0 ..))`, which is the form the port's DSN takes.

use std::path::{Path, PathBuf};
use std::process::Command;

fn slow_tests_off() -> bool {
    let off = std::env::var_os("EDA_SLOW_TESTS").is_none();
    if off {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
    }
    off
}

fn eda(dir: &Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_eda")).arg("board").args(args).arg("-C").arg(dir).output().expect("run eda");
    let log = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "eda board {args:?} failed:\n{log}");
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// `examples/through_hole_headers.yaml` placed: J3's eight pins along the top, J2 and J1 along the bottom, the resistors on the right, so
/// SDA and SCL (J3.1, J3.2 to J2.3, J2.4) run down the left half of the board.
fn placed_board(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("eda_autoroute_cutout_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let yaml = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/through_hole_headers.yaml");
    let out = Command::new(env!("CARGO_BIN_EXE_eda")).args(["board", "new"]).arg(&yaml).arg("-o").arg(&dir).args(["--origin", "0,0"]).output().expect("run eda");
    assert!(out.status.success(), "eda board new: {}", String::from_utf8_lossy(&out.stderr));
    for (part, at) in [("J3", "11.5,3"), ("J2", "7,20"), ("J1", "17,20"), ("R1", "20,8"), ("R2", "20,14")] {
        eda(&dir, &["place", part, "--at", at]);
    }
    dir
}

/// The window cut out of the board: 4..11 x 8..15 mm, across SDA's and SCL's straight runs.
fn cut(dir: &Path) {
    eda(dir, &["shape", "add", "--kind", "rect", "--layer", "Edge.Cuts", "--pts", "4,8 11,15", "--width", "0.05"]);
}

/// kicad-cli's DRC of the board: the violations' types, and the unconnected items.
fn drc(dir: &Path) -> (Vec<String>, usize) {
    let report: serde_json::Value = serde_json::from_str(&eda(dir, &["drc"])).expect("the DRC report is JSON");
    let types = report["violations"].as_array().into_iter().flatten().filter_map(|v| v["type"].as_str().map(str::to_string)).collect();
    (types, report["unconnected_items"].as_array().map_or(0, Vec::len))
}

/// What the router is answerable for: copper against the edge, other copper and holes, and the connections.
const ROUTER_TYPES: &[&str] = &["copper_edge_clearance", "clearance", "shorting_items", "tracks_crossing", "hole_clearance", "hole_to_hole", "track_width"];

#[test]
fn an_autorouted_board_with_a_cutout_is_clean_for_kicad_clis_drc() {
    if slow_tests_off() {
        return;
    }
    let with_cutout = placed_board("cut");
    cut(&with_cutout);
    // kicad-cli's own Specctra export writes the cutout as a keep-out on the signal layers.
    let dsn = with_cutout.join("board.dsn");
    eda(&with_cutout, &["export", "specctra", "-o", dsn.to_str().unwrap()]);
    let text = std::fs::read_to_string(&dsn).expect("the DSN is written");
    assert!(text.contains("(keepout \"\" (polygon signal 0"), "the export carries the cutout as fillBOUNDARY's keep-out:\n{text}");
    eda(&with_cutout, &["route"]);
    let (types, unconnected) = drc(&with_cutout);
    let routed_wrong: Vec<&String> = types.iter().filter(|t| ROUTER_TYPES.contains(&t.as_str())).collect();
    assert!(routed_wrong.is_empty() && unconnected == 0, "kicad-cli finds {routed_wrong:?} and {unconnected} unconnected item(s) on the board routed with its cutout (all: {types:?})");

    // The same board routed without seeing the cutout (the routing the autoroute made of it before the cutouts were keep-outs), then cut.
    let cut_after = placed_board("cut_after");
    eda(&cut_after, &["route"]);
    cut(&cut_after);
    let (types, _) = drc(&cut_after);
    assert!(types.iter().any(|t| t == "copper_edge_clearance"), "routed blind to the cutout, the board crosses it: {types:?}");
    let _ = std::fs::remove_dir_all(&with_cutout);
    let _ = std::fs::remove_dir_all(&cut_after);
}
