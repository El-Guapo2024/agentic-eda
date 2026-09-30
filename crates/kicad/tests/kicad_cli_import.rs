//! `import_kicad_pcb` verification: round-tripping our own writer's output
//! (always runs -- pure Rust, no external tools) and cross-checking real
//! KiCad boards against `kicad-cli pcb export pos` (`#[ignore]`d like
//! `kicad_cli_drc.rs`; run with
//! `cargo test -p eda-kicad --test kicad_cli_import -- --ignored --nocapture`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use eda_engine::{derive_schematic, EngineOptions};
use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::ir::{Side, Track, Via};
use eda_model::ConstraintModel;
use eda_place::{place, PlaceOptions};

fn find_kicad_cli() -> Option<PathBuf> {
    if let Ok(out) = Command::new("which").arg("kicad-cli").output() {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() {
                return Some(PathBuf::from(p));
            }
        }
    }
    let mac = PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli");
    mac.exists().then_some(mac)
}

/// The FreeRouting port, failing loudly on anything left unrouted -- same
/// helper `kicad_cli_drc.rs` uses.
fn route(design: &eda_model::ir::Design, model: &ConstraintModel, rules: &eda_model::BoardRules) -> eda_model::ir::Design {
    let routed = eda_freeroute::design::route_design(design, model, rules, 20, 10).expect("route_design");
    assert!(routed.unrouted.is_empty(), "unrouted: {:?}", routed.unrouted);
    let mut out = design.clone();
    out.routing = Some(routed.routing);
    out
}

/// (net, layer, width, (endpoint a, endpoint b)); endpoints sorted so a
/// segment compares equal regardless of which end was "start" in the file.
type Segment = (String, String, i64, ((i64, i64), (i64, i64)));

/// A track's copper as a set of undirected 2-point segments. `.kicad_pcb`
/// has no polyline -- `export_kicad_pcb` writes one `(segment ...)` per
/// consecutive point pair, so a multi-point `Track` round-trips as N
/// independent 2-point `Track`s (true of any KiCad-written board, not
/// particular to this writer). That per-segment set, not the original
/// `Track` grouping, is what a round trip can actually preserve.
fn segment_set(tracks: &[Track]) -> BTreeSet<Segment> {
    let mut out = BTreeSet::new();
    for t in tracks {
        for w in t.pts.windows(2) {
            let (mut a, mut b) = ((w[0].x, w[0].y), (w[1].x, w[1].y));
            if b < a {
                std::mem::swap(&mut a, &mut b);
            }
            out.insert((t.net.clone(), t.layer.clone(), t.width, (a, b)));
        }
    }
    out
}

fn via_set(vias: &[Via]) -> BTreeSet<(String, i64, i64, i64, i64, String, String)> {
    vias.iter().map(|v| (v.net.clone(), v.at.x, v.at.y, v.drill, v.diameter, v.from_layer.clone(), v.to_layer.clone())).collect()
}

/// schematic -> place -> route -> `export_kicad_pcb` -> `import_kicad_pcb`:
/// every footprint's position/rotation/side and every routed copper
/// segment/via must come back exact to the micrometre. No kicad-cli
/// needed (both ends are our own code), so this runs unconditionally.
#[test]
fn round_trips_own_pipeline_output() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().and_then(|p| p.parent()).expect("crates/kicad -> repo root");

    for name in ["mcu_board_30plus", "two_pin_nets"] {
        let yaml_path = repo_root.join("examples").join(format!("{name}.yaml"));
        let text = std::fs::read_to_string(&yaml_path).unwrap_or_else(|e| panic!("read {}: {e}", yaml_path.display()));
        let model: ConstraintModel = serde_yaml::from_str(&text).unwrap_or_else(|e| panic!("parse {name}: {e}"));

        let seed = 0u64;
        let opts = EngineOptions { seed, intent_hash: "kicad_import_rt".into(), ..Default::default() };
        let design = derive_schematic(&model, &opts).expect("derive_schematic");
        let placed = place(&design, &model, &PlaceOptions { seed, ..Default::default() }).expect("place");
        let routed = route(&placed, &model, &model.board);

        let meta = ExportMeta { date: "2026-01-01", title: name };
        let pcb_text = export_kicad_pcb(&routed, &model, &meta).unwrap_or_else(|e| panic!("{name}: export_kicad_pcb failed: {e:?}"));

        let (back, _model2, notes) = import_kicad_pcb(&pcb_text).unwrap_or_else(|e| panic!("{name}: import_kicad_pcb failed: {e:?}"));

        let orig_pl = routed.placement.as_ref().expect("placed");
        let back_pl = back.placement.as_ref().expect("import always produces a placement");
        assert_eq!(orig_pl.footprints.len(), back_pl.footprints.len(), "{name}: footprint count");
        let by_id: std::collections::HashMap<&str, _> = back_pl.footprints.iter().map(|f| (f.id.as_str(), f)).collect();
        for fp in &orig_pl.footprints {
            let got = by_id.get(fp.id.as_str()).unwrap_or_else(|| panic!("{name}: {} missing after import", fp.id));
            assert_eq!(fp.at, got.at, "{name}: {} position", fp.id);
            assert_eq!(fp.rot, got.rot, "{name}: {} rotation", fp.id);
            assert_eq!(fp.side, got.side, "{name}: {} side", fp.id);
        }

        let orig_tracks = &routed.routing.as_ref().expect("routed").tracks;
        let orig_vias = &routed.routing.as_ref().expect("routed").vias;
        let back_tracks = back.routing.as_ref().map(|r| r.tracks.clone()).unwrap_or_default();
        let back_vias = back.routing.as_ref().map(|r| r.vias.clone()).unwrap_or_default();
        assert_eq!(segment_set(orig_tracks), segment_set(&back_tracks), "{name}: routed copper segments differ after round trip");
        assert_eq!(via_set(orig_vias), via_set(&back_vias), "{name}: vias differ after round trip");

        println!(
            "{name}: round trip OK -- {} footprint(s), {} segment(s), {} via(s); notes: {notes:?}",
            orig_pl.footprints.len(),
            segment_set(orig_tracks).len(),
            orig_vias.len()
        );
    }
}

fn collect_kicad_pcb(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_kicad_pcb(&p, out);
        } else if p.extension().is_some_and(|e| e == "kicad_pcb") {
            out.push(p);
        }
    }
}

struct PosRow {
    reference: String,
    x: f64,
    y: f64,
    rot: f64,
    side: String,
}

/// Minimal CSV line splitter (quoted fields, `""`-escaped quotes) -- not a
/// general parser, just enough for `kicad-cli pcb export pos --format csv`.
fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => fields.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    fields.push(cur);
    fields
}

fn read_pos_csv(path: &Path) -> Vec<PosRow> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines = text.lines();
    let Some(header) = lines.next() else { return Vec::new() };
    let cols = parse_csv_line(header);
    let idx = |name: &str| cols.iter().position(|c| c == name);
    let (Some(i_ref), Some(i_x), Some(i_y), Some(i_rot), Some(i_side)) = (idx("Ref"), idx("PosX"), idx("PosY"), idx("Rot"), idx("Side")) else {
        return Vec::new();
    };
    lines
        .filter_map(|line| {
            let f = parse_csv_line(line);
            let (x, y, rot) = (f.get(i_x)?.parse().ok()?, f.get(i_y)?.parse().ok()?, f.get(i_rot)?.parse().ok()?);
            Some(PosRow { reference: f.get(i_ref)?.clone(), x, y, rot, side: f.get(i_side)?.clone() })
        })
        .collect()
}

fn norm360(deg: f64) -> f64 {
    deg.rem_euclid(360.0)
}

/// `kicad-cli pcb export pos`'s convention differs from the raw file (and
/// from our model, which mirrors the raw file): PosY is negated and Rot is
/// the raw file angle -- the opposite sign from `FootprintInstance.rot`
/// (see `pcb.rs::write_footprint`'s negation comment, and `import.rs`'s
/// `import_rot_millideg`, which undoes it on the way in).
fn expected_pos(fp: &eda_model::ir::FootprintInstance) -> (f64, f64, f64, &'static str) {
    (fp.at.x as f64 / 1000.0, -(fp.at.y as f64) / 1000.0, norm360(-(fp.rot as f64) / 1000.0), if fp.side == Side::Bottom { "bottom" } else { "top" })
}

/// Cross-check against KiCad's own template boards (installed alongside
/// kicad-cli on this machine): every footprint's reference/position/
/// rotation/side must match `kicad-cli pcb export pos`. `#[ignore]`d like
/// `kicad_cli_drc.rs`; skips quietly when kicad-cli or the templates
/// aren't installed at the usual macOS path.
#[test]
#[ignore]
fn kicad_cli_template_boards_match_pos_export() {
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let template_dir = PathBuf::from("/Applications/KiCad/KiCad.app/Contents/SharedSupport/template");
    if !template_dir.exists() {
        eprintln!("KiCad templates not found at {}; skipping", template_dir.display());
        return;
    }

    let mut boards = Vec::new();
    collect_kicad_pcb(&template_dir, &mut boards);
    assert!(!boards.is_empty(), "found no .kicad_pcb under {}", template_dir.display());
    boards.sort();

    let tmp = std::env::temp_dir().join("eda_kicad_import_pos_test");
    std::fs::create_dir_all(&tmp).unwrap();

    let (mut boards_whole, mut boards_total, mut fps_total, mut fps_matched) = (0usize, 0usize, 0usize, 0usize);
    let mut failures: Vec<String> = Vec::new();

    for board in &boards {
        let text = match std::fs::read_to_string(board) {
            Ok(t) => t,
            Err(e) => {
                failures.push(format!("{}: read failed: {e}", board.display()));
                continue;
            }
        };
        let (design, _model, notes) = match import_kicad_pcb(&text) {
            Ok(r) => r,
            Err(e) => {
                failures.push(format!("{}: import failed: {e:?}", board.display()));
                continue;
            }
        };
        let pos_csv = tmp.join(format!("{}.csv", board.file_stem().unwrap().to_string_lossy()));
        let out = Command::new(&cli)
            .args(["pcb", "export", "pos", "--format", "csv", "--units", "mm", "--side", "both", "-o"])
            .arg(&pos_csv)
            .arg(board)
            .output()
            .expect("run kicad-cli pcb export pos");
        if !out.status.success() {
            failures.push(format!("{}: kicad-cli pos export failed: {}", board.display(), String::from_utf8_lossy(&out.stderr)));
            continue;
        }

        boards_total += 1;
        let rows = read_pos_csv(&pos_csv);
        let fps = design.placement.as_ref().map(|p| p.footprints.clone()).unwrap_or_default();
        let by_id: std::collections::HashMap<&str, _> = fps.iter().map(|f| (f.id.as_str(), f)).collect();
        // Several templates (enclosures, blank Eurocards) are pure
        // mechanical boards with no real components -- kicad-cli reports
        // zero pos rows for them, which is a correct, vacuous match, not a
        // failure to import anything.
        let mut board_ok = true;
        for row in &rows {
            fps_total += 1;
            let Some(fp) = by_id.get(row.reference.as_str()) else {
                board_ok = false;
                failures.push(format!("{}: {} not found after import ({} outline pts, source {})", board.display(), row.reference, fps.len(), notes.outline_source));
                continue;
            };
            let (ex, ey, erot, eside) = expected_pos(fp);
            let ok = (row.x - ex).abs() < 0.0011 && (row.y - ey).abs() < 0.0011 && (norm360(row.rot - erot)).min(360.0 - norm360(row.rot - erot)) < 0.01 && row.side == eside;
            if ok {
                fps_matched += 1;
            } else {
                board_ok = false;
                failures.push(format!("{}: {} got=({:.4},{:.4},{:.3},{}) expected=({ex:.4},{ey:.4},{erot:.3},{eside})", board.display(), row.reference, row.x, row.y, row.rot, row.side));
            }
        }
        if board_ok {
            boards_whole += 1;
        }
    }

    println!("KiCad template boards: {boards_whole}/{boards_total} whole, {fps_matched}/{fps_total} footprints matched kicad-cli's pos export");
    if !failures.is_empty() {
        println!("failures ({}):", failures.len());
        for f in failures.iter().take(40) {
            println!("  {f}");
        }
    }
    assert_eq!(fps_matched, fps_total, "{fps_matched}/{fps_total} footprints matched kicad-cli's pos export exactly");
    assert_eq!(boards_whole, boards_total, "{boards_whole}/{boards_total} boards had every footprint match");
}
