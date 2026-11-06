//! Round 4's C-1: our teardrop generator (`eda_connectivity::teardrop::generate_teardrops`, the port of `TEARDROP_MANAGER::computeTeardropPolygon`) gives the
//! outlines KiCad's own gives. A board of vias, pads and tracks is written with each via's and pad's `(teardrops ..)` parameters and without
//! `(legacy_teardrops yes)`, so `kicad-cli pcb drc --refill-zones --save-board` runs `TEARDROP_MANAGER::UpdateTeardrops` (`zone_filler_tool.cpp`) and saves its
//! teardrops (round 5's A5-1 adds arc tracks, both ways round; round 7 adds tracks that end off a via's centre or cross it, R7-1, and SMD pads of every shape
//! but a custom one, UI-LEFT P1). That board is read back, its teardrop zones taken out, and ours generated on the same vias, pads and tracks: every outline is
//! the same, point for point.
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
    // Round 6 D6-1: a track that leaves the via by 0.05 mm, less than its width. 25ec2335's `computeChordThroughShape` measures the chord on the extended
    // centreline (the via's diameter), so it gets a teardrop; 8303b2ad's `computeEmergingTrackLength` refused it.
    (100.0, 125.0, 0.6, 100.35, 125.0, 0.2),
];

/// Round 5's A5-1: arc tracks. The via (x, y, diameter), and the arc (mid x, mid y, other end x, other end y, width) from the via's centre; `true` in the last
/// field writes the arc the other way round (it ends at the via, `findAnchorPointsOnTrack`'s `arc.Reverse()`).
const ARC_CASES: &[(f64, f64, f64, f64, f64, f64, f64, f64, bool)] = &[
    (100.0, 110.0, 0.6, 101.0, 110.4, 102.0, 110.0, 0.2, false),
    (110.0, 110.0, 0.8, 111.3, 109.1, 112.1, 110.7, 0.25, false),
    (120.0, 110.0, 1.0, 121.7, 111.3, 123.9, 110.2, 0.2, true),
    (130.0, 110.0, 0.7, 129.2, 112.0, 130.3, 113.5, 0.15, true),
    // Round 6 D6-1: a short arc (0.4 mm, emerging by less than its width measured in a straight line): 25ec2335 exempts arcs from the chord filter.
    (140.0, 110.0, 0.6, 140.25, 110.1, 140.4, 110.05, 0.2, false),
];

/// A segment: start x, start y, end x, end y, width (mm).
type Seg = (f64, f64, f64, f64, f64);

/// Round 7 R7-1: vias (x, y, diameter) and tracks that do not start on their centre, with the number of teardrops kicad-cli makes. `UpdateTeardrops` hit-tests
/// a track's ends against the via (`via->HitTest( track->GetStart() )`), so an end inside the via off its centre gets a teardrop; a track crossing the via's
/// centre gets one on each side (`buildCrossingStub`); a track whose centreline crosses the via without covering its centre goes through
/// `findAnchorPointsOnTrack` as it is; two segments meeting on the via get one each.
const VIA_TRACK_CASES: &[(f64, f64, f64, &[Seg], usize)] = &[
    (100.0, 135.0, 0.8, &[(100.1, 135.0, 105.0, 135.0, 0.2)], 1),
    (110.0, 135.0, 0.6, &[(110.1, 135.15, 113.0, 137.0, 0.15)], 1),
    (120.0, 135.0, 0.8, &[(115.0, 135.0, 125.0, 135.0, 0.2)], 2),
    (130.0, 135.0, 0.8, &[(125.0, 135.05, 135.0, 135.05, 0.2)], 2),
    (140.0, 135.0, 0.8, &[(135.0, 135.25, 145.0, 135.25, 0.2)], 1),
    (150.0, 135.0, 0.8, &[(150.0, 130.0, 150.0, 135.0, 0.2), (150.0, 135.0, 155.0, 135.0, 0.2)], 2),
    (160.0, 135.0, 0.8, &[(156.0, 133.0, 164.0, 137.0, 0.2)], 2),
];

/// UI-LEFT P1: SMD pads (`m_UseRoundShapesOnly` is off by default, so every shape but a custom one gets teardrops). The footprint (x, y, rotation in degrees,
/// on the back), the pad's shape and size as the file writes them, its tracks, and the number of teardrops kicad-cli makes.
const PAD_CASES: &[(f64, f64, f64, bool, &str, &[Seg], usize)] = &[
    // A rectangle entered along its length and across it, and off its centre.
    (100.0, 145.0, 0.0, false, "rect (size 1.5 1)", &[(100.0, 145.0, 105.0, 145.0, 0.25)], 1),
    (110.0, 145.0, 0.0, false, "rect (size 1.5 1)", &[(110.0, 145.0, 110.0, 150.0, 0.25)], 1),
    (120.0, 145.0, 0.0, false, "rect (size 1 1.5)", &[(120.2, 145.3, 124.0, 146.0, 0.2)], 1),
    // Entered near a corner: D is kept inside the copper the track enters.
    (130.0, 145.0, 0.0, false, "rect (size 1.5 1)", &[(130.5, 145.3, 134.0, 149.0, 0.2)], 1),
    // Rounded rectangles: on a diagonal, and crossed (one teardrop each side).
    (140.0, 145.0, 0.0, false, "roundrect (size 1.2 0.9) (roundrect_rratio 0.25)", &[(140.0, 145.0, 143.0, 147.0, 0.2)], 1),
    (150.0, 145.0, 0.0, false, "roundrect (size 1.5 1) (roundrect_rratio 0.25)", &[(147.0, 145.0, 153.0, 145.0, 0.2)], 2),
    // A footprint turned a quarter, one turned by 30 degrees, and one on the back.
    (160.0, 145.0, 90.0, false, "rect (size 1.5 1)", &[(160.0, 145.0, 165.0, 145.0, 0.25)], 1),
    (170.0, 145.0, 30.0, false, "roundrect (size 1.6 0.8) (roundrect_rratio 0.25)", &[(170.0, 145.0, 174.0, 146.0, 0.2)], 1),
    (180.0, 145.0, 0.0, true, "rect (size 1.5 1)", &[(180.0, 145.0, 185.0, 145.0, 0.25)], 1),
    // A long oval, a chamfered rectangle and a trapezoid.
    (100.0, 160.0, 0.0, false, "oval (size 2 1)", &[(100.0, 160.0, 100.0, 165.0, 0.2)], 1),
    (110.0, 160.0, 0.0, false, "roundrect (size 1.5 1) (roundrect_rratio 0) (chamfer_ratio 0.2) (chamfer top_right)", &[(110.0, 160.0, 114.0, 162.0, 0.2)], 1),
    (120.0, 160.0, 0.0, false, "trapezoid (size 1.5 1) (rect_delta 0 0.3)", &[(120.0, 160.0, 125.0, 160.0, 0.2)], 1),
    // A round pad entered off its centre (the symmetric re-fit, as for a via).
    (130.0, 160.0, 0.0, false, "circle (size 1 1)", &[(130.1, 160.0, 134.0, 160.0, 0.2)], 1),
];

const PARAMS: &str = "(teardrops (best_length_ratio 0.5) (max_length 1) (best_width_ratio 1) (max_width 2) (curved_edges no) (filter_ratio 0.9) (enabled yes) (allow_two_segments no) (prefer_zone_connections yes))";

fn board() -> String {
    let mut s = String::from(
        "(kicad_pcb (version 20241229) (generator \"pcbnew\") (generator_version \"9.0\")\n  (general (thickness 1.6))\n  (paper \"A4\")\n  (layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal) (1 \"F.Mask\" user) (3 \"B.Mask\" user) (13 \"F.Paste\" user) (15 \"B.Paste\" user) (25 \"Edge.Cuts\" user))\n  (setup (pad_to_mask_clearance 0))\n  (net 0 \"\")\n  (net 1 \"N1\")\n  (gr_rect (start 85 85) (end 200 175) (stroke (width 0.1) (type solid)) (fill no) (layer \"Edge.Cuts\") (uuid \"00000000-0000-0000-0000-000000000001\"))\n",
    );
    let mut uuid = 0x1000;
    let mut next = || {
        uuid += 1;
        format!("00000000-0000-0000-0000-{uuid:012x}")
    };
    let segment = |(sx, sy, ex, ey, w): Seg, layer: &str, id: String| format!("  (segment (start {sx} {sy}) (end {ex} {ey}) (width {w}) (layer \"{layer}\") (net 1) (uuid \"{id}\"))\n");
    for (i, (x, y, d, ex, ey, w)) in CASES.iter().enumerate() {
        s += &format!("  (via (at {x} {y}) (size {d}) (drill 0.3) (layers \"F.Cu\" \"B.Cu\") (net 1) {PARAMS} (uuid \"00000000-0000-0000-0000-0000000001{i:02}\"))\n");
        s += &format!("  (segment (start {x} {y}) (end {ex} {ey}) (width {w}) (layer \"F.Cu\") (net 1) (uuid \"00000000-0000-0000-0000-0000000002{i:02}\"))\n");
    }
    for (i, (x, y, d, mx, my, ex, ey, w, rev)) in ARC_CASES.iter().enumerate() {
        s += &format!("  (via (at {x} {y}) (size {d}) (drill 0.3) (layers \"F.Cu\" \"B.Cu\") (net 1) {PARAMS} (uuid \"00000000-0000-0000-0000-0000000003{i:02}\"))\n");
        let ((sx, sy), (tx, ty)) = if *rev { ((*ex, *ey), (*x, *y)) } else { ((*x, *y), (*ex, *ey)) };
        s += &format!("  (arc (start {sx} {sy}) (mid {mx} {my}) (end {tx} {ty}) (width {w}) (layer \"F.Cu\") (net 1) (uuid \"00000000-0000-0000-0000-0000000004{i:02}\"))\n");
    }
    for (x, y, d, segs, _) in VIA_TRACK_CASES {
        s += &format!("  (via (at {x} {y}) (size {d}) (drill 0.3) (layers \"F.Cu\" \"B.Cu\") (net 1) {PARAMS} (uuid \"{}\"))\n", next());
        for seg in segs.iter() {
            s += &segment(*seg, "F.Cu", next());
        }
    }
    for (i, (x, y, rot, back, shape, segs, _)) in PAD_CASES.iter().enumerate() {
        let side = if *back { "B" } else { "F" };
        let (at, pad_at) = if *rot == 0.0 { (format!("(at {x} {y})"), "(at 0 0)".to_string()) } else { (format!("(at {x} {y} {rot})"), format!("(at 0 0 {rot})")) };
        let (shape_word, rest) = shape.split_once(' ').unwrap();
        s += &format!("  (footprint \"T:P{i}\" (layer \"{side}.Cu\") (uuid \"{}\") {at}\n", next());
        s += &format!(
            "    (property \"Reference\" \"U{i}\" (at 0 -3 {rot}) (layer \"{side}.SilkS\") (hide yes) (uuid \"{}\") (effects (font (size 1 1) (thickness 0.15)){}))\n",
            next(),
            if *back { " (justify mirror)" } else { "" }
        );
        s += &format!("    (pad \"1\" smd {shape_word} {pad_at} {rest} (layers \"{side}.Cu\" \"{side}.Mask\" \"{side}.Paste\") (net 1 \"N1\") {PARAMS} (uuid \"{}\"))\n  )\n", next());
        for seg in segs.iter() {
            s += &segment(*seg, if *back { "B.Cu" } else { "F.Cu" }, next());
        }
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
    let mut kicad: Vec<_> = rt.zones.iter().filter(|z| z.teardrop).map(|z| (z.layer.clone(), z.outline.clone())).collect();
    rt.zones.retain(|z| !z.teardrop);
    let expected = CASES.len() + ARC_CASES.len() + VIA_TRACK_CASES.iter().map(|c| c.4).sum::<usize>() + PAD_CASES.iter().map(|c| c.6).sum::<usize>();
    assert_eq!(kicad.len(), expected, "kicad-cli made the teardrops each case expects");

    let settings = TeardropSettings { enabled: true, ..Default::default() };
    let mut ours: Vec<_> = generate_teardrops(&design, &model, &settings).into_iter().map(|z| (z.layer, z.outline)).collect();
    let key = |o: &(String, Vec<eda_model::ir::Point>)| (o.1[0].x.get(), o.1[0].y.get());
    kicad.sort_by_key(key);
    ours.sort_by_key(key);
    let show = |o: &(String, Vec<eda_model::ir::Point>)| format!("{} {:?}", o.0, o.1.iter().map(|p| (p.x.get(), p.y.get())).collect::<Vec<_>>());
    if kicad.len() == ours.len() {
        for (k, o) in kicad.iter().zip(ours.iter()).filter(|(k, o)| k != o) {
            eprintln!("kicad {}\nours  {}", show(k), show(o));
        }
    } else {
        eprintln!("kicad {} teardrops, ours {}:\nkicad {:#?}\nours  {:#?}", kicad.len(), ours.len(), kicad.iter().map(show).collect::<Vec<_>>(), ours.iter().map(show).collect::<Vec<_>>());
    }
    assert_eq!(ours, kicad);
    let _ = std::fs::remove_dir_all(&dir);
}

/// UI-LEFT P3: each via's and pad's own teardrop parameters. KiCad 25ec2335's `UpdateTeardrops` builds an anchor's teardrops from its own `TEARDROP_PARAMETERS`
/// (`via->GetTeardropParams()`, `pad->GetTeardropParams()`), and so does ours (`eda_model::teardrop_params`, read from the file's `(teardrops ..)`), with Board
/// Setup's block off: an anchor whose own are off gets none, one whose own are on gets teardrops as long and wide as its own ratios and limits say, and a
/// track at or past its own filter ratio gets none.
#[test]
fn each_anchors_own_teardrop_parameters_give_kicad_clis_teardrops() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let params = |enabled: bool, len_ratio: f64, max_len: f64, width_ratio: f64, max_width: f64, filter: f64| {
        format!("(teardrops (best_length_ratio {len_ratio}) (max_length {max_len}) (best_width_ratio {width_ratio}) (max_width {max_width}) (curved_edges no) (filter_ratio {filter}) (enabled {}) (allow_two_segments no) (prefer_zone_connections yes))", if enabled { "yes" } else { "no" })
    };
    // (x, y, diameter, track width, the via's parameters, teardrops expected)
    let vias: Vec<(f64, f64, f64, f64, String, usize)> = vec![
        (100.0, 100.0, 0.8, 0.2, params(true, 0.5, 1.0, 1.0, 2.0, 0.9), 1),
        (110.0, 100.0, 0.8, 0.2, params(false, 0.5, 1.0, 1.0, 2.0, 0.9), 0),
        (120.0, 100.0, 0.8, 0.2, params(true, 1.0, 3.0, 1.0, 2.0, 0.9), 1),
        (130.0, 100.0, 1.0, 0.25, params(true, 0.6, 0.4, 0.8, 0.6, 0.9), 1),
        (140.0, 100.0, 0.8, 0.3, params(true, 0.5, 1.0, 1.0, 2.0, 0.3), 0),
        (150.0, 100.0, 0.8, 0.2, params(true, 0.5, 1.0, 1.0, 0.0, 0.9), 1),
    ];
    // (x, y, the pad's shape and size, track width, its parameters, teardrops expected)
    let pads: Vec<(f64, f64, &str, f64, String, usize)> = vec![
        (100.0, 120.0, "rect (size 1.5 1)", 0.25, params(true, 0.5, 1.0, 1.0, 2.0, 0.9), 1),
        (110.0, 120.0, "rect (size 1.5 1)", 0.25, params(false, 0.5, 1.0, 1.0, 2.0, 0.9), 0),
        (120.0, 120.0, "roundrect (size 1.2 0.9) (roundrect_rratio 0.25)", 0.2, params(true, 0.9, 2.0, 0.9, 1.2, 0.9), 1),
        (130.0, 120.0, "circle (size 1 1)", 0.2, params(true, 0.4, 0.5, 0.7, 0.8, 0.9), 1),
    ];
    let mut s = String::from(
        "(kicad_pcb (version 20241229) (generator \"pcbnew\") (generator_version \"9.0\")\n  (general (thickness 1.6))\n  (paper \"A4\")\n  (layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal) (1 \"F.Mask\" user) (3 \"B.Mask\" user) (13 \"F.Paste\" user) (15 \"B.Paste\" user) (25 \"Edge.Cuts\" user))\n  (setup (pad_to_mask_clearance 0))\n  (net 0 \"\")\n  (net 1 \"N1\")\n  (gr_rect (start 85 85) (end 200 175) (stroke (width 0.1) (type solid)) (fill no) (layer \"Edge.Cuts\") (uuid \"00000000-0000-0000-0000-000000000001\"))\n",
    );
    let mut uuid = 0x2000;
    let mut next = || {
        uuid += 1;
        format!("00000000-0000-0000-0000-{uuid:012x}")
    };
    for (x, y, d, w, p, _) in &vias {
        s += &format!("  (via (at {x} {y}) (size {d}) (drill 0.3) (layers \"F.Cu\" \"B.Cu\") (net 1) {p} (uuid \"{}\"))\n", next());
        s += &format!("  (segment (start {x} {y}) (end {} {y}) (width {w}) (layer \"F.Cu\") (net 1) (uuid \"{}\"))\n", x + 5.0, next());
    }
    for (i, (x, y, shape, w, p, _)) in pads.iter().enumerate() {
        let (shape_word, rest) = shape.split_once(' ').unwrap();
        s += &format!("  (footprint \"T:Q{i}\" (layer \"F.Cu\") (uuid \"{}\") (at {x} {y})\n", next());
        s += &format!("    (property \"Reference\" \"Q{i}\" (at 0 -3 0) (layer \"F.SilkS\") (hide yes) (uuid \"{}\") (effects (font (size 1 1) (thickness 0.15))))\n", next());
        s += &format!("    (pad \"1\" smd {shape_word} (at 0 0) {rest} (layers \"F.Cu\" \"F.Mask\" \"F.Paste\") (net 1 \"N1\") {p} (uuid \"{}\"))\n  )\n", next());
        s += &format!("  (segment (start {x} {y}) (end {} {}) (width {w}) (layer \"F.Cu\") (net 1) (uuid \"{}\"))\n", x + 4.0, y + 1.5, next());
    }
    s += ")\n";

    let dir = std::env::temp_dir().join(format!("eda_teardrops_own_kicad_cli_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pcb = dir.join("td.kicad_pcb");
    std::fs::write(&pcb, &s).unwrap();
    let out = Command::new(&cli).args(["pcb", "drc", "--refill-zones", "--save-board", "-o"]).arg(dir.join("drc.rpt")).arg(&pcb).output().unwrap();
    assert!(out.status.success() || dir.join("drc.rpt").exists(), "kicad-cli: {}", String::from_utf8_lossy(&out.stderr));

    let (mut design, model, _notes) = import_kicad_pcb(&std::fs::read_to_string(&pcb).unwrap()).unwrap();
    let rt = design.routing.as_mut().unwrap();
    let mut kicad: Vec<_> = rt.zones.iter().filter(|z| z.teardrop).map(|z| (z.layer.clone(), z.outline.clone())).collect();
    rt.zones.retain(|z| !z.teardrop);
    let expected = vias.iter().map(|v| v.5).sum::<usize>() + pads.iter().map(|p| p.5).sum::<usize>();
    assert_eq!(kicad.len(), expected, "kicad-cli made the teardrops each anchor's own parameters ask for");

    // Board Setup's block off: every anchor here is governed by its own parameters.
    let mut ours: Vec<_> = generate_teardrops(&design, &model, &TeardropSettings::default()).into_iter().map(|z| (z.layer, z.outline)).collect();
    let key = |o: &(String, Vec<eda_model::ir::Point>)| (o.1[0].x.get(), o.1[0].y.get());
    kicad.sort_by_key(key);
    ours.sort_by_key(key);
    let show = |o: &(String, Vec<eda_model::ir::Point>)| format!("{} {:?}", o.0, o.1.iter().map(|p| (p.x.get(), p.y.get())).collect::<Vec<_>>());
    if kicad != ours {
        eprintln!("kicad {:#?}\nours  {:#?}", kicad.iter().map(show).collect::<Vec<_>>(), ours.iter().map(show).collect::<Vec<_>>());
    }
    assert_eq!(ours, kicad);
    // And our writer gives kicad-cli each anchor's own parameters back: the board written from the import, read again, has the same teardrops of ours.
    let written = eda_kicad::export_kicad_pcb(&design, &model, &eda_kicad::ExportMeta { date: "2026-11-06", title: "td" }).unwrap();
    for (_, _, _, _, p, _) in &vias {
        assert!(written.contains(p.as_str()), "a via's own parameters are written back: {p}");
    }
    let (again, again_model, _) = import_kicad_pcb(&written).unwrap();
    let mut ours_again: Vec<_> = generate_teardrops(&again, &again_model, &TeardropSettings::default()).into_iter().map(|z| (z.layer, z.outline)).collect();
    ours_again.sort_by_key(key);
    assert_eq!(ours_again, ours, "the parameters survive our writer and reader");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The teardrops kicad-cli made on `pcb` (refilled and saved) and ours made on the same board with `settings`, each as (layer, outline) sorted by its first
/// point; the board's own teardrops taken out before ours are made.
fn kicad_and_ours(cli: &std::path::Path, dir: &std::path::Path, pcb_text: &str, project: Option<&str>, settings: &TeardropSettings) -> (Vec<(String, Vec<eda_model::ir::Point>)>, Vec<(String, Vec<eda_model::ir::Point>)>) {
    std::fs::create_dir_all(dir).unwrap();
    let pcb = dir.join("td.kicad_pcb");
    std::fs::write(&pcb, pcb_text).unwrap();
    if let Some(p) = project {
        std::fs::write(dir.join("td.kicad_pro"), p).unwrap();
    }
    let out = Command::new(cli).args(["pcb", "drc", "--refill-zones", "--save-board", "-o"]).arg(dir.join("drc.rpt")).arg(&pcb).output().unwrap();
    assert!(out.status.success() || dir.join("drc.rpt").exists(), "kicad-cli: {}", String::from_utf8_lossy(&out.stderr));
    let (mut design, model, _notes) = import_kicad_pcb(&std::fs::read_to_string(&pcb).unwrap()).unwrap();
    let rt = design.routing.as_mut().unwrap();
    let mut kicad: Vec<_> = rt.zones.iter().filter(|z| z.teardrop).map(|z| (z.layer.clone(), z.outline.clone())).collect();
    rt.zones.retain(|z| !z.teardrop);
    let mut ours: Vec<_> = generate_teardrops(&design, &model, settings).into_iter().map(|z| (z.layer, z.outline)).collect();
    let key = |o: &(String, Vec<eda_model::ir::Point>)| (o.0.clone(), o.1[0].x.get(), o.1[0].y.get());
    kicad.sort_by_key(key);
    ours.sort_by_key(key);
    (kicad, ours)
}

fn show_all(v: &[(String, Vec<eda_model::ir::Point>)]) -> Vec<String> {
    v.iter().map(|o| format!("{} {:?}", o.0, o.1.iter().map(|p| (p.x.get(), p.y.get())).collect::<Vec<_>>())).collect()
}

/// UI-LEFT-2 PP6 and PP7: the teardrops 25ec2335's `TEARDROP_MANAGER` makes that round 7 did not port, each as kicad-cli 10.99 (4fe1ca2060) makes it, ours point
/// for point:
/// - fitted to other nets' clearance (`computeFittedTeardropPolygon`: a teardrop that would come within clearance of another net's track, via or pad is
///   narrowed by bisection, or dropped when not even the track's width fits);
/// - curved edges (`m_CurvedEdges`: `computeCurvedForRoundShape` on vias and round pads, `computeCurvedForRectShape` on rectangles, a rounded rectangle's corner
///   and an oval's end);
/// - two segments (`m_AllowUseTwoTracks`: a first segment too short lends the teardrop the length of the one it joins, straight on or bent, straight or curved;
///   a bend past 60 degrees is not taken);
/// - track to track (`AddTeardropsOnTracks`: a narrow track ending on a wider one, from the project's `td_track_end` block);
/// - a padstack's inner layers (a through-hole pad and a via whose inner shape differs, entered on In1.Cu);
/// - the solder-mask teardrop (`createTeardropMask`: a track with a mask opening, on F.Mask, grown by the board's expansion or the track's own margin).
/// A custom pad (`MergePrimitivesAsPolygon`) is compared within 10 um: its polygon is cut into segments by `eda_drc::pad_shape`, not KiCad's routines.
#[test]
fn the_teardrops_round_7_left_out_are_kicad_clis_own() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let params = |curved: bool, two: bool, max_len: f64| {
        format!("(teardrops (best_length_ratio 0.5) (max_length {max_len}) (best_width_ratio 1) (max_width 2) (curved_edges {}) (filter_ratio 0.9) (enabled yes) (allow_two_segments {}) (prefer_zone_connections yes))", if curved { "yes" } else { "no" }, if two { "yes" } else { "no" })
    };
    let (plain, curved) = (params(false, false, 1.0), params(true, false, 1.0));
    let mut s = String::from(
        "(kicad_pcb (version 20241229) (generator \"pcbnew\") (generator_version \"9.0\")\n  (general (thickness 1.6))\n  (paper \"A3\")\n  (layers (0 \"F.Cu\" signal) (4 \"In1.Cu\" signal) (6 \"In2.Cu\" signal) (2 \"B.Cu\" signal) (1 \"F.Mask\" user) (3 \"B.Mask\" user) (13 \"F.Paste\" user) (15 \"B.Paste\" user) (25 \"Edge.Cuts\" user))\n  (setup (pad_to_mask_clearance 0.05))\n  (net 0 \"\")\n  (net 1 \"N1\")\n  (net 2 \"N2\")\n  (gr_rect (start 85 85) (end 260 200) (stroke (width 0.1) (type solid)) (fill no) (layer \"Edge.Cuts\") (uuid \"00000000-0000-0000-0000-000000000001\"))\n",
    );
    let uuid = std::cell::Cell::new(0x3000);
    let next = || {
        uuid.set(uuid.get() + 1);
        format!("00000000-0000-0000-0000-{:012x}", uuid.get())
    };
    let via = |s: &mut String, x: f64, y: f64, d: f64, p: &str, net: u32, extra: &str, id: String| {
        *s += &format!("  (via (at {x} {y}) (size {d}) (drill 0.3) (layers \"F.Cu\" \"B.Cu\") {extra}(net {net}) {p} (uuid \"{id}\"))\n");
    };
    let seg = |s: &mut String, (sx, sy, ex, ey, w): Seg, layer: &str, net: u32, id: String| {
        *s += &format!("  (segment (start {sx} {sy}) (end {ex} {ey}) (width {w}) (layer \"{layer}\") (net {net}) (uuid \"{id}\"))\n");
    };
    let mut expected = 0usize;
    // PP6, fitted: a foreign track close to the teardrop's flank, one to its far side, a via of another net beside it, and one too close for any teardrop.
    via(&mut s, 100.0, 100.0, 0.8, &plain, 1, "", next());
    seg(&mut s, (100.0, 100.0, 105.0, 100.0, 0.2), "F.Cu", 1, next());
    seg(&mut s, (100.6, 100.48, 100.6, 103.0, 0.2), "F.Cu", 2, next());
    expected += 1;
    via(&mut s, 110.0, 100.0, 1.0, &plain, 1, "", next());
    seg(&mut s, (110.0, 100.0, 113.0, 104.0, 0.25), "F.Cu", 1, next());
    via(&mut s, 111.25, 100.45, 0.5, "", 2, "", next());
    expected += 1;
    via(&mut s, 120.0, 100.0, 0.8, &plain, 1, "", next());
    seg(&mut s, (120.0, 100.0, 125.0, 100.0, 0.2), "F.Cu", 1, next());
    seg(&mut s, (120.55, 99.62, 123.0, 99.62, 0.2), "F.Cu", 2, next());
    seg(&mut s, (120.55, 100.38, 123.0, 100.38, 0.2), "F.Cu", 2, next());
    // (no teardrop: the N2 tracks leave no room for one as wide as the track)
    // Curved: a via along and off the axes, a via whose teardrop the max width clips (C and E put back on the circle).
    via(&mut s, 100.0, 115.0, 0.8, &curved, 1, "", next());
    seg(&mut s, (100.0, 115.0, 105.0, 115.0, 0.2), "F.Cu", 1, next());
    via(&mut s, 110.0, 115.0, 0.8, &curved, 1, "", next());
    seg(&mut s, (110.0, 115.0, 113.0, 119.0, 0.25), "F.Cu", 1, next());
    via(&mut s, 120.0, 115.0, 2.0, &params(true, false, 2.0).replace("(max_width 2)", "(max_width 1)"), 1, "", next());
    seg(&mut s, (120.0, 115.0, 125.0, 115.0, 0.25), "F.Cu", 1, next());
    expected += 3;
    // Two segments: a first segment 0.1 mm out of a 1.6 mm via, then bent by 18 degrees, then straight on, then curved, then bent by 70 degrees (not taken).
    let two = params(false, true, 2.0);
    for (i, (bend_x, bend_y, p)) in [(124.0, 101.0, two.clone()), (124.0, 100.0, two.clone()), (124.0, 101.0, params(true, true, 2.0)), (121.9, 103.5, two.clone())].into_iter().enumerate() {
        let x0 = 100.0 + 10.0 * i as f64;
        via(&mut s, x0, 130.0, 1.6, &p, 1, "", next());
        seg(&mut s, (x0, 130.0, x0 + 0.9, 130.0, 0.2), "F.Cu", 1, next());
        seg(&mut s, (x0 + 0.9, 130.0, x0 + bend_x - 120.0, 130.0 + bend_y - 100.0, 0.2), "F.Cu", 1, next());
        expected += 1;
    }
    // Mask teardrops: a track with an opening on a via (the board's 0.05 mm expansion), and one with its own 0.1 mm margin.
    via(&mut s, 150.0, 100.0, 0.8, &plain, 1, "", next());
    s += &format!("  (segment (start 150 100) (end 155 100) (width 0.2) (layers \"F.Cu\" \"F.Mask\") (net 1) (uuid \"{}\"))\n", next());
    via(&mut s, 160.0, 100.0, 0.8, &plain, 1, "", next());
    s += &format!("  (segment (start 160 100) (end 163 104) (width 0.2) (layers \"F.Cu\" \"F.Mask\") (solder_mask_margin 0.1) (net 1) (uuid \"{}\"))\n", next());
    expected += 4;
    // A via whose inner layers are larger (`(padstack (mode front_inner_back) ..)`), entered on In1.Cu.
    via(&mut s, 170.0, 100.0, 0.8, &plain, 1, "(padstack (mode front_inner_back) (layer \"Inner\" (size 1.2)) (layer \"B.Cu\" (size 0.8))) ", next());
    seg(&mut s, (170.0, 100.0, 175.0, 100.0, 0.2), "In1.Cu", 1, next());
    expected += 1;
    // Track to track: a 0.2 mm track on the end of a 0.6 mm one, straight on and at an angle, and on the start of one.
    seg(&mut s, (100.0, 150.0, 105.0, 150.0, 0.6), "F.Cu", 1, next());
    seg(&mut s, (105.0, 150.0, 110.0, 150.0, 0.2), "F.Cu", 1, next());
    seg(&mut s, (115.0, 150.0, 120.0, 150.0, 0.6), "F.Cu", 1, next());
    seg(&mut s, (120.0, 150.0, 123.0, 154.0, 0.2), "F.Cu", 1, next());
    seg(&mut s, (130.0, 150.0, 135.0, 150.0, 0.6), "B.Cu", 1, next());
    seg(&mut s, (130.0, 150.0, 126.0, 147.0, 0.2), "B.Cu", 1, next());
    expected += 3;
    // Pads: curved on a rectangle, on a rounded rectangle's corner and on an oval's end; two segments on a rectangle; a fitted rectangle; a through-hole pad's
    // inner shape; a custom pad.
    let pad = |s: &mut String, i: usize, x: f64, y: f64, kind: &str, body: &str, layers: &str, p: &str| {
        *s += &format!("  (footprint \"T:P{i}\" (layer \"F.Cu\") (uuid \"{}\") (at {x} {y})\n", next());
        *s += &format!("    (property \"Reference\" \"U{i}\" (at 0 -3 0) (layer \"F.SilkS\") (hide yes) (uuid \"{}\") (effects (font (size 1 1) (thickness 0.15))))\n", next());
        *s += &format!("    (pad \"1\" {kind} {body} (layers {layers}) (net 1 \"N1\") {p} (uuid \"{}\"))\n  )\n", next());
    };
    let smd = "\"F.Cu\" \"F.Mask\" \"F.Paste\"";
    pad(&mut s, 0, 100.0, 170.0, "smd", "rect (at 0 0) (size 1.5 1)", smd, &curved);
    seg(&mut s, (100.0, 170.0, 105.0, 170.0, 0.25), "F.Cu", 1, next());
    pad(&mut s, 1, 110.0, 170.0, "smd", "roundrect (at 0 0) (size 1.2 0.9) (roundrect_rratio 0.25)", smd, &curved);
    seg(&mut s, (110.0, 170.0, 113.0, 172.0, 0.2), "F.Cu", 1, next());
    pad(&mut s, 2, 120.0, 170.0, "smd", "oval (at 0 0) (size 2 1)", smd, &curved);
    seg(&mut s, (120.0, 170.0, 125.0, 170.0, 0.2), "F.Cu", 1, next());
    pad(&mut s, 3, 130.0, 170.0, "smd", "rect (at 0 0) (size 1.5 1)", smd, &two);
    seg(&mut s, (130.0, 170.0, 130.95, 170.0, 0.25), "F.Cu", 1, next());
    seg(&mut s, (130.95, 170.0, 135.0, 171.0, 0.25), "F.Cu", 1, next());
    pad(&mut s, 4, 140.0, 170.0, "smd", "rect (at 0 0) (size 1.5 1)", smd, &plain);
    seg(&mut s, (140.0, 170.0, 145.0, 170.0, 0.25), "F.Cu", 1, next());
    seg(&mut s, (141.3, 170.55, 141.3, 173.0, 0.2), "F.Cu", 2, next());
    pad(&mut s, 5, 150.0, 170.0, "thru_hole", "circle (at 0 0) (size 1 1) (drill 0.5) (remove_unused_layers no) (padstack (mode front_inner_back) (layer \"Inner\" (shape circle) (size 1.6 1.6)) (layer \"B.Cu\" (shape circle) (size 1 1)))", "\"*.Cu\" \"*.Mask\"", &plain);
    seg(&mut s, (150.0, 170.0, 155.0, 170.0, 0.2), "In1.Cu", 1, next());
    expected += 6;
    let custom_at = (160.0, 170.0);
    pad(&mut s, 6, custom_at.0, custom_at.1, "smd", "custom (at 0 0) (size 0.6 0.6) (options (clearance outline) (anchor rect)) (primitives (gr_poly (pts (xy -0.3 -0.3) (xy 1.2 -0.3) (xy 1.2 0.3) (xy -0.3 0.3)) (width 0) (fill yes)))", smd, &plain);
    seg(&mut s, (160.0, 170.0, 160.0, 175.0, 0.2), "F.Cu", 1, next());
    expected += 1;
    s += ")\n";
    let project = r#"{"board":{"design_settings":{"teardrop_options":[{"td_onvia":true,"td_onpthpad":true,"td_onsmdpad":true,"td_ontrackend":true,"td_onroundshapesonly":false}],"teardrop_parameters":[{"td_target_name":"td_round_shape","td_enabled":false},{"td_target_name":"td_rect_shape","td_enabled":false},{"td_target_name":"td_track_end","td_enabled":true,"td_maxlen":1.0,"td_maxheight":2.0,"td_length_ratio":0.5,"td_height_ratio":1.0,"td_curve_segcount":0,"td_width_to_size_filter_ratio":0.9,"td_allow_use_two_tracks":true,"td_on_pad_in_zone":false}]}},"meta":{"filename":"td.kicad_pro","version":3}}"#;
    let settings = TeardropSettings { track_end: eda_model::teardrop_params::TeardropParams { enabled: true, ..Default::default() }, ..Default::default() };
    let dir = std::env::temp_dir().join(format!("eda_teardrops_new_kinds_{}", std::process::id()));
    let (kicad, ours) = kicad_and_ours(&cli, &dir, &s, Some(project), &settings);
    assert_eq!(kicad.len(), expected, "kicad-cli made the teardrops each case expects:\n{:#?}", show_all(&kicad));
    // The custom pad's teardrop within 10 um of kicad-cli's, the others point for point.
    let near_custom = |o: &(String, Vec<eda_model::ir::Point>)| o.1.iter().all(|p| (p.x.get() - eda_model::units::mm(custom_at.0).get()).abs() < 2_000_000 && (p.y.get() - eda_model::units::mm(custom_at.1).get()).abs() < 2_000_000);
    let (kc, ko): (Vec<_>, Vec<_>) = (kicad.iter().filter(|o| near_custom(o)).collect(), ours.iter().filter(|o| near_custom(o)).collect());
    let (kr, or): (Vec<_>, Vec<_>) = (kicad.iter().filter(|o| !near_custom(o)).cloned().collect(), ours.iter().filter(|o| !near_custom(o)).cloned().collect());
    if kr != or {
        let ks = show_all(&kr);
        let os = show_all(&or);
        for k in &ks {
            if !os.contains(k) {
                eprintln!("kicad only: {k}");
            }
        }
        for o in &os {
            if !ks.contains(o) {
                eprintln!("ours only:  {o}");
            }
        }
    }
    assert_eq!(or, kr);
    assert_eq!(kc.len(), 1, "kicad-cli's custom pad teardrop");
    assert_eq!(ko.len(), 1, "our custom pad teardrop: {:?}", show_all(&ours));
    let dist = |a: eda_model::ir::Point, b: &[eda_model::ir::Point]| b.iter().map(|q| ((a.x.get() - q.x.get()) as f64).hypot((a.y.get() - q.y.get()) as f64)).fold(f64::MAX, f64::min);
    for p in &ko[0].1 {
        assert!(dist(*p, &kc[0].1) < 10_000.0, "our custom pad teardrop's {p:?} is within 10 um of kicad-cli's {:?}", kc[0].1);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
