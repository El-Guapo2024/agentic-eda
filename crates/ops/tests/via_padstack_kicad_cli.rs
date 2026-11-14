//! UI-LEFT-2 PP12 as kicad-cli sees it: a via whose padstack gives its inner layers a wider ring (`(padstack (mode front_inner_back) (layer "Inner" (size ..))
//! ..)`, `PCB_VIA::GetWidth( aLayer )`) is judged by Draw Via's check (`crates/ops/src/via_place.rs`, `VIA_PLACER::checkDRCViolation`) at each layer's own
//! diameter, as kicad-cli's DRC judges it: a new via that clears the other's front ring but not its inner one is refused, and the board written with it
//! placed anyway (Allow DRC violations) has kicad-cli's clearance violation; one that clears the inner ring too is placed and kicad-cli finds none; the same
//! board with a normal padstack takes the first via without a word from either.
//!
//! Slow tier: kicad-cli runs, so it needs `EDA_SLOW_TESTS=1`; skipped, loudly, without it.

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::units::mm;
use eda_ops::{Board, Cmd};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Four copper layers; a via of N1 at (15, 15) mm, 0.6 mm on the front and the back, `inner` mm on the inner layers when given.
fn board(inner: Option<f64>) -> String {
    let stack = inner.map(|d| format!("(padstack (mode front_inner_back) (layer \"Inner\" (size {d})) (layer \"B.Cu\" (size 0.6)))")).unwrap_or_default();
    format!(
        r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers (0 "F.Cu" signal) (4 "In1.Cu" signal) (6 "In2.Cu" signal) (2 "B.Cu" signal) (1 "F.Mask" user) (3 "B.Mask" user) (13 "F.Paste" user) (15 "B.Paste" user) (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (net 1 "N1")
  (net 2 "N2")
  (gr_rect (start 5 5) (end 45 30) (stroke (width 0.1) (type solid)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-000000000001"))
  (via (at 15 15) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1) {stack} (uuid "00000000-0000-0000-0000-000000000011"))
)
"#
    )
}

fn kicad_cli() -> PathBuf {
    std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).unwrap_or_else(|| "kicad-cli".into())
}

/// `kicad-cli pcb drc` of `text`: the report.
fn drc(dir: &Path, name: &str, text: &str) -> String {
    let pcb = dir.join(format!("{name}.kicad_pcb"));
    let rpt = dir.join(format!("{name}.rpt"));
    std::fs::write(&pcb, text).unwrap();
    let out = Command::new(kicad_cli()).args(["pcb", "drc", "-o"]).arg(&rpt).arg(&pcb).output().expect("kicad-cli runs");
    assert!(out.status.success() || rpt.exists(), "kicad-cli: {}", String::from_utf8_lossy(&out.stderr));
    std::fs::read_to_string(&rpt).unwrap()
}

/// Draw Via of an N2 through via at (`x`, 15) mm, 0.6 mm, on the board `text`: whether it was refused, and kicad-cli's report of the board with it placed.
fn place(dir: &Path, name: &str, text: &str, x: f64) -> (bool, String) {
    let (design, model, _) = import_kicad_pcb(text).expect("read the board");
    let cmd = |allow: bool| Cmd::PlaceVia { net: "N2".into(), x: mm(x), y: mm(15.0), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into(), split_track: None, allow_drc_violations: allow, hidden_layers: Vec::new(), barcode_shapes: Default::default() };
    let mut b = Board::new(design.clone(), &model, mm(0.1), mm(0.3));
    let refused = match b.apply(&cmd(false)) {
        Ok(()) => false,
        Err(e) => {
            assert_eq!(e[0].hint.as_deref(), Some(eda_ops::via_place::VIA_VIOLATES_DRC));
            true
        }
    };
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    b.apply(&cmd(true)).expect("Allow DRC violations places it");
    let written = export_kicad_pcb(b.design(), &model, &ExportMeta { date: "2026-11-07", title: "via padstack" }).expect("export");
    assert!(inner_size(&written).is_some() == text.contains("(padstack"), "the writer keeps the padstack");
    (refused, drc(dir, name, &written))
}

fn inner_size(text: &str) -> Option<&str> {
    text.find("(layer \"Inner\" (size ").map(|i| &text[i..i + 30])
}

#[test]
fn a_vias_inner_ring_refuses_what_kicad_clis_drc_flags() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let dir = std::env::temp_dir().join(format!("via_padstack_kicad_cli_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // 1.05 mm from the N1 via: 0.45 mm clear of its 0.6 mm front ring, 0.05 mm of its 1.4 mm inner ring (the clearance is 0.2 mm)
    let (refused, report) = place(&dir, "inner_hit", &board(Some(1.4)), 16.05);
    assert!(refused, "Draw Via refuses it");
    // kicad-cli measures the inner ring: 0.05 mm, on each of the two inner layers (the front's would be 0.45 mm)
    assert_eq!(report.matches("[clearance]: Clearance violation (netclass 'Default' clearance 0.2000 mm; actual 0.0500 mm)").count(), 2, "kicad-cli flags the inner layers: {report}");
    // 1.25 mm away: 0.25 mm clear of the inner ring too
    let (refused, report) = place(&dir, "inner_clear", &board(Some(1.4)), 16.25);
    assert!(!refused, "Draw Via places it");
    assert!(!report.contains("[clearance]"), "kicad-cli finds no clearance violation: {report}");
    // a normal padstack: the first spot is clear for both
    let (refused, report) = place(&dir, "normal", &board(None), 16.0);
    assert!(!refused, "Draw Via places it beside a normal via");
    assert!(!report.contains("[clearance]"), "{report}");
    let _ = std::fs::remove_dir_all(&dir);
}
