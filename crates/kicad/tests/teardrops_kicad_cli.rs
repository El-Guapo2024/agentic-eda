//! Round 4's C-1: our teardrop generator (`eda_connectivity::teardrop::generate_teardrops`, the port of `TEARDROP_MANAGER::computeTeardropPolygon` for a round
//! anchor) gives the outlines KiCad's own gives. A board of vias and tracks is written with each via's `(teardrops ..)` parameters and without
//! `(legacy_teardrops yes)`, so `kicad-cli pcb drc --refill-zones --save-board` runs `TEARDROP_MANAGER::UpdateTeardrops` (`zone_filler_tool.cpp`) and saves its
//! teardrops. That board is read back, its teardrop zones taken out, and ours generated on the same vias and tracks: every outline is the same, point for point.
//!
//! Slow: it needs kicad-cli. Run with `EDA_SLOW_TESTS=1 cargo test -p eda-kicad --test teardrops_kicad_cli -- --nocapture`.

use eda_connectivity::teardrop::generate_teardrops;
use eda_kicad::import_kicad_pcb;
use eda_model::ir::TeardropSettings;
use std::path::PathBuf;
use std::process::Command;

fn kicad_cli() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("EDA_KICAD_CLI") {
        return Some(PathBuf::from(p));
    }
    let out = Command::new("which").arg("kicad-cli").output().ok()?;
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!p.is_empty()).then(|| PathBuf::from(p))
}

/// The vias (x, y, diameter in mm) and the track from each one's centre (end x, end y, width in mm).
const CASES: &[(f64, f64, f64, f64, f64, f64)] = &[
    (100.0, 100.0, 0.6, 105.0, 100.0, 0.2),
    (110.0, 100.0, 0.8, 113.0, 104.0, 0.25),
    (95.0, 95.0, 3.0, 95.0, 105.0, 0.25),
    (120.0, 120.0, 0.6, 117.0, 121.732, 0.15),
    (130.0, 120.0, 1.0, 132.0, 118.0, 0.2),
    (140.0, 120.0, 0.7, 140.6, 120.0, 0.1),
    (150.0, 120.0, 0.9, 149.0, 125.0, 0.3),
    (160.0, 120.0, 0.6, 160.0, 119.2, 0.12),
];

const PARAMS: &str = "(teardrops (best_length_ratio 0.5) (max_length 1) (best_width_ratio 1) (max_width 2) (curved_edges no) (filter_ratio 0.9) (enabled yes) (allow_two_segments no) (prefer_zone_connections yes))";

fn board() -> String {
    let mut s = String::from(
        "(kicad_pcb (version 20241229) (generator \"pcbnew\") (generator_version \"9.0\")\n  (general (thickness 1.6))\n  (paper \"A4\")\n  (layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal) (25 \"Edge.Cuts\" user))\n  (setup (pad_to_mask_clearance 0))\n  (net 0 \"\")\n  (net 1 \"N1\")\n  (gr_rect (start 85 85) (end 170 130) (stroke (width 0.1) (type solid)) (fill no) (layer \"Edge.Cuts\") (uuid \"00000000-0000-0000-0000-000000000001\"))\n",
    );
    for (i, (x, y, d, ex, ey, w)) in CASES.iter().enumerate() {
        s += &format!("  (via (at {x} {y}) (size {d}) (drill 0.3) (layers \"F.Cu\" \"B.Cu\") (net 1) {PARAMS} (uuid \"00000000-0000-0000-0000-0000000001{i:02}\"))\n");
        s += &format!("  (segment (start {x} {y}) (end {ex} {ey}) (width {w}) (layer \"F.Cu\") (net 1) (uuid \"00000000-0000-0000-0000-0000000002{i:02}\"))\n");
    }
    s + ")\n"
}

#[test]
fn our_teardrops_are_kicad_clis_own() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let dir = std::env::temp_dir().join(format!("eda_teardrops_kicad_cli_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pcb = dir.join("td.kicad_pcb");
    std::fs::write(&pcb, board()).unwrap();
    let out = Command::new(&cli).args(["pcb", "drc", "--refill-zones", "--save-board", "-o"]).arg(dir.join("drc.rpt")).arg(&pcb).output().unwrap();
    assert!(out.status.success() || dir.join("drc.rpt").exists(), "kicad-cli: {}", String::from_utf8_lossy(&out.stderr));

    let (mut design, model, _notes) = import_kicad_pcb(&std::fs::read_to_string(&pcb).unwrap()).unwrap();
    let rt = design.routing.as_mut().unwrap();
    let mut kicad: Vec<_> = rt.zones.iter().filter(|z| z.teardrop).map(|z| z.outline.clone()).collect();
    rt.zones.retain(|z| !z.teardrop);
    assert_eq!(kicad.len(), CASES.len(), "kicad-cli made one teardrop per via");

    let settings = TeardropSettings { enabled: true, ..Default::default() };
    let mut ours: Vec<_> = generate_teardrops(&design, &model, &settings).into_iter().map(|z| z.outline).collect();
    let key = |o: &Vec<eda_model::ir::Point>| (o[0].x.get(), o[0].y.get());
    kicad.sort_by_key(key);
    ours.sort_by_key(key);
    for (k, o) in kicad.iter().zip(ours.iter()) {
        eprintln!("kicad {:?}\nours  {:?}", k.iter().map(|p| (p.x.get(), p.y.get())).collect::<Vec<_>>(), o.iter().map(|p| (p.x.get(), p.y.get())).collect::<Vec<_>>());
    }
    assert_eq!(ours, kicad);
    let _ = std::fs::remove_dir_all(&dir);
}
