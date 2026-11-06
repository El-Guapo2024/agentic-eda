//! UI-LEFT-2 PP6 as kicad-cli sees it: teardrops kept current as the board is edited (`teardrop_upkeep.rs`, KiCad 25ec2335 `BOARD_COMMIT::Push`) and fitted to
//! other nets' clearance (`computeFittedTeardropPolygon`).
//!
//! A board of two vias with their tracks (N1, each via with its own teardrop parameters) and a track of N2 well away gets its teardrops (`Cmd::AddAllTeardrops`);
//! then one command moves the N2 track next to the first via's teardrop. In that same command the teardrop is fitted again, narrower, and the second via's
//! teardrop is left as it was. Then:
//! - kicad-cli's own update of the board as it now is (our writer's file without our teardrops, `pcb drc --refill-zones --save-board`, which runs
//!   `UpdateTeardrops( .., true )`) makes the same teardrops, point for point;
//! - kicad-cli's DRC of the board as the studio writes it (our teardrops, refilled) finds no clearance violation, and the fitted teardrop's fill is its whole
//!   outline (the filler had nothing to cut away); the teardrop as it was before the move, kept, would have had its fill cut by the N2 track's clearance.
//!
//! Slow tier: kicad-cli runs, so it needs `EDA_SLOW_TESTS=1`; skipped, loudly, without it.

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::ir::{Design, Point, Zone};
use eda_model::units::mm;
use eda_model::ConstraintModel;
use eda_ops::{Board, Cmd};
use std::path::{Path, PathBuf};
use std::process::Command;

const PARAMS: &str = "(teardrops (best_length_ratio 0.5) (max_length 1) (best_width_ratio 1) (max_width 2) (curved_edges no) (filter_ratio 0.9) (enabled yes) (allow_two_segments no) (prefer_zone_connections yes))";

fn board() -> String {
    format!(
        r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (1 "F.Mask" user) (3 "B.Mask" user) (13 "F.Paste" user) (15 "B.Paste" user) (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (net 1 "N1")
  (net 2 "N2")
  (gr_rect (start 5 5) (end 45 30) (stroke (width 0.1) (type solid)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-000000000001"))
  (via (at 15 15) (size 0.8) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1) {PARAMS} (uuid "00000000-0000-0000-0000-000000000011"))
  (segment (start 15 15) (end 20 15) (width 0.2) (layer "F.Cu") (net 1) (uuid "00000000-0000-0000-0000-000000000012"))
  (via (at 30 15) (size 0.8) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1) {PARAMS} (uuid "00000000-0000-0000-0000-000000000021"))
  (segment (start 30 15) (end 35 15) (width 0.2) (layer "F.Cu") (net 1) (uuid "00000000-0000-0000-0000-000000000022"))
  (segment (start 15.6 17.48) (end 15.6 20) (width 0.2) (layer "F.Cu") (net 2) (uuid "00000000-0000-0000-0000-000000000031"))
)
"#
    )
}

fn kicad_cli() -> PathBuf {
    std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).unwrap_or_else(|| "kicad-cli".into())
}

fn write(design: &Design, model: &ConstraintModel) -> String {
    export_kicad_pcb(design, model, &ExportMeta { date: "2026-11-07", title: "teardrop update" }).expect("export")
}

/// `kicad-cli pcb drc --refill-zones --save-board` of `text`: the saved board and the report.
fn drc(dir: &Path, name: &str, text: &str) -> (String, String) {
    let pcb = dir.join(format!("{name}.kicad_pcb"));
    let rpt = dir.join(format!("{name}.rpt"));
    std::fs::write(&pcb, text).unwrap();
    let out = Command::new(kicad_cli()).args(["pcb", "drc", "--refill-zones", "--save-board", "-o"]).arg(&rpt).arg(&pcb).output().expect("kicad-cli runs");
    assert!(out.status.success() || rpt.exists(), "kicad-cli: {}", String::from_utf8_lossy(&out.stderr));
    (std::fs::read_to_string(&pcb).unwrap(), std::fs::read_to_string(&rpt).unwrap())
}

fn teardrops(design: &Design) -> Vec<Zone> {
    design.routing.as_ref().map(|r| r.zones.iter().filter(|z| z.teardrop).cloned().collect()).unwrap_or_default()
}

fn outlines(design: &Design) -> Vec<Vec<Point>> {
    let mut v: Vec<Vec<Point>> = teardrops(design).into_iter().map(|z| z.outline).collect();
    v.sort_by_key(|o| (o[0].x, o[0].y));
    v
}

/// The area of a ring, mm².
fn area(pts: &[[f64; 2]]) -> f64 {
    let n = pts.len();
    (0..n).map(|i| pts[i][0] * pts[(i + 1) % n][1] - pts[(i + 1) % n][0] * pts[i][1]).sum::<f64>().abs() / 2.0
}

/// Every `(filled_polygon ..)` ring of the teardrop zones in a saved board, mm, and the zone's outline area beside it.
fn teardrop_fills(text: &str) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for chunk in text.split("(zone").skip(1) {
        if !chunk.contains("(teardrop") {
            continue;
        }
        let rings: Vec<Vec<[f64; 2]>> = chunk
            .split("(pts")
            .skip(1)
            .map(|r| {
                r.split("(xy ").skip(1).filter_map(|xy| {
                    let mut it = xy.split(|c: char| c == ' ' || c == ')').filter(|s| !s.is_empty());
                    Some([it.next()?.parse().ok()?, it.next()?.parse().ok()?])
                })
                .collect()
            })
            .collect();
        if rings.len() >= 2 {
            out.push((area(&rings[0]), rings[1..].iter().map(|r| area(r)).sum()));
        }
    }
    out
}

#[test]
fn a_teardrop_fitted_again_after_a_move_is_kicad_clis_and_passes_its_drc() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let (design, model, _) = import_kicad_pcb(&board()).expect("import");
    let n2 = design.routing.as_ref().unwrap().tracks.iter().find(|t| t.net == "N2").unwrap().id.clone();
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    b.apply(&Cmd::AddAllTeardrops).expect("add_all_teardrops");
    let first = teardrops(b.design());
    assert_eq!(first.len(), 2, "one teardrop on each via");
    let second_id = first.iter().find(|z| z.outline[0].x > mm(25.0)).unwrap().id.clone();
    let stale = first.iter().find(|z| z.outline[0].x < mm(25.0)).unwrap().clone();

    // One command: the N2 track moved next to the first via's teardrop.
    b.apply(&Cmd::MoveItems { ids: vec![n2], dx: mm(0.0), dy: mm(-2.0) }).expect("move_items");
    let after = teardrops(b.design());
    assert_eq!(after.len(), 2);
    assert!(after.iter().any(|z| z.id == second_id), "the second via's teardrop is not touched");
    let fitted = after.iter().find(|z| z.outline[0].x < mm(25.0)).unwrap();
    assert_ne!(fitted.outline, stale.outline, "the first via's teardrop was fitted again");
    let design = b.into_design();

    let dir = std::env::temp_dir().join(format!("teardrop_update_kicad_cli_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    // kicad-cli's own update of the moved board: the same teardrops.
    let mut bare = design.clone();
    bare.routing.as_mut().unwrap().zones.retain(|z| !z.teardrop);
    let bare_text = write(&bare, &model);
    assert!(bare_text.contains("(legacy_teardrops no)") && bare_text.contains("(enabled yes)"), "kicad-cli builds its own from the vias' parameters");
    let (theirs, _) = drc(&dir, "theirs", &bare_text);
    let (their_design, _, _) = import_kicad_pcb(&theirs).expect("read kicad-cli's board");
    assert_eq!(outlines(&their_design), outlines(&design), "kicad-cli's teardrops after the move are ours");

    // kicad-cli's DRC of the board the studio writes: no clearance violation, and the fitted teardrop's fill is its outline.
    let (filled, report) = drc(&dir, "ours", &write(&design, &model));
    assert!(!report.contains("[clearance]"), "{report}");
    for (outline, fill) in teardrop_fills(&filled) {
        assert!((outline - fill).abs() < 0.002 * outline + 1e-4, "a teardrop's fill is its whole outline: {fill} of {outline} mm2");
    }
    // The teardrop as it was before the move, kept: the filler cuts the N2 track's clearance out of it.
    let mut kept = design.clone();
    let rt = kept.routing.as_mut().unwrap();
    rt.zones.retain(|z| !(z.teardrop && z.outline[0].x < mm(25.0)));
    rt.zones.push(Zone { id: String::new(), ..stale });
    rt.assign_missing_ids();
    let (kept_filled, _) = drc(&dir, "stale", &write(&kept, &model));
    assert!(teardrop_fills(&kept_filled).iter().any(|(outline, fill)| *fill < 0.995 * outline), "the stale teardrop loses copper to the N2 track's clearance: {:?}", teardrop_fills(&kept_filled));
    let _ = std::fs::remove_dir_all(&dir);
}
