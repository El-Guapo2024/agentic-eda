//! Where `eda board new` puts a new board on its page (UI-LEFT-2 CC4).
//!
//! KiCad shows the drawing sheet on a new board (`m_ShowPageLimits( true )`, `GAL_SET::DefaultVisible`'s
//! `LAYER_DRAWINGSHEET`) and puts what first comes onto an empty board in the middle of the page
//! (`BOARD_NETLIST_UPDATER::estimateFootprintInsertionPosition`). The placer used to start every board at
//! the page's corner, where the frame crosses it; a new board's outline is now centred on the page, and
//! `--origin x,y` puts its corner where a script wants it.

use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("eda_cli_new_board_page_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(format!("{name}.yaml"))
}

fn board_new(intent: &Path, dir: &Path, extra: &[&str]) {
    let out = Command::new(env!("CARGO_BIN_EXE_eda")).args(["board", "new"]).arg(intent).arg("-o").arg(dir).args(extra).output().expect("run eda");
    assert!(out.status.success(), "eda board new failed: {}", String::from_utf8_lossy(&out.stderr));
}

/// The outline's box in `<dir>/design.json`, mm: (x0, y0, x1, y1).
fn outline_box(dir: &Path) -> (f64, f64, f64, f64) {
    let d: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("design.json")).unwrap()).unwrap();
    let pts: Vec<(f64, f64)> = d["placement"]["outline"]
        .as_array()
        .expect("an outline")
        .iter()
        .map(|p| (p["x"].as_f64().unwrap() / 1e6, p["y"].as_f64().unwrap() / 1e6))
        .collect();
    let f = |g: fn(&(f64, f64)) -> f64, min: bool| pts.iter().map(g).fold(if min { f64::MAX } else { f64::MIN }, |a, b| if min { a.min(b) } else { a.max(b) });
    (f(|p| p.0, true), f(|p| p.1, true), f(|p| p.0, false), f(|p| p.1, false))
}

#[test]
fn a_new_board_is_centred_on_its_a4_page_inside_the_frame() {
    // a sized square (no outline declared) and a declared outline at the page's corner
    for name in ["mcu_board_30plus", "dense_small_outline"] {
        let dir = scratch(name);
        board_new(&example(name), &dir, &[]);
        let (x0, y0, x1, y1) = outline_box(&dir);
        assert!(((x0 + x1) / 2.0 - 148.5).abs() <= 1.0 && ((y0 + y1) / 2.0 - 105.0).abs() <= 1.0, "{name}: centred on A4's (148.5, 105): {x0} {y0} {x1} {y1}");
        assert!(x0 >= 10.0 && y0 >= 10.0 && x1 <= 287.0 && y1 <= 200.0, "{name}: inside the default sheet's 10 mm margins");
        assert_eq!((x0.fract(), y0.fract()), (0.0, 0.0), "{name}: the corner on a whole millimetre");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn origin_puts_the_corner_where_asked_and_keeps_the_size() {
    let a = scratch("centred");
    let b = scratch("corner");
    let c = scratch("at");
    board_new(&example("mcu_board_30plus"), &a, &[]);
    board_new(&example("mcu_board_30plus"), &b, &["--origin", "0,0"]);
    board_new(&example("mcu_board_30plus"), &c, &["--origin", "20.5,30"]);
    let (ax0, ay0, ax1, ay1) = outline_box(&a);
    let (bx0, by0, bx1, by1) = outline_box(&b);
    let (cx0, cy0, cx1, cy1) = outline_box(&c);
    assert_eq!((bx0, by0), (0.0, 0.0), "--origin 0,0: the page's corner, as before");
    assert_eq!((cx0, cy0), (20.5, 30.0));
    assert_eq!((bx1 - bx0, by1 - by0), (ax1 - ax0, ay1 - ay0), "the same size wherever it is");
    assert_eq!((cx1 - cx0, cy1 - cy0), (ax1 - ax0, ay1 - ay0));
    let bad = Command::new(env!("CARGO_BIN_EXE_eda")).args(["board", "new"]).arg(example("mcu_board_30plus")).arg("-o").arg(scratch("bad")).args(["--origin", "x"]).output().unwrap();
    assert!(!bad.status.success() && String::from_utf8_lossy(&bad.stderr).contains("--origin takes x,y in millimetres"));
    for d in [a, b, c, scratch("bad")] {
        let _ = std::fs::remove_dir_all(&d);
    }
}
