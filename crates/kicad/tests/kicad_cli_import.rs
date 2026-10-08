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

/// Our exporter writes the KiCad 9 file format; an older kicad-cli (e.g.
/// Ubuntu's 7.0) refuses to open it, so it can't serve as the oracle.
fn kicad_cli_reads_our_format(cli: &Path) -> bool {
    let v = Command::new(cli).arg("version").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let major: u32 = v.split('.').next().and_then(|m| m.parse().ok()).unwrap_or(0);
    if major < 9 {
        eprintln!("kicad-cli {v} is older than the KiCad 9 format we export; skipping");
    }
    major >= 9
}

fn find_kicad_cli() -> Option<PathBuf> {
    find_kicad_cli_any().filter(|p| kicad_cli_reads_our_format(p))
}

fn find_kicad_cli_any() -> Option<PathBuf> {
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
    if slow_tests_off() {
        return;
    }
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

/// `export_kicad_pcb` -> `import_kicad_pcb` for every graphic shape kind
/// and free text: no kicad-cli needed, both ends are our own code. Ids
/// round-trip too -- the importer assigns them from the same content the
/// exporter wrote, so they land back on the same values.
#[test]
fn round_trips_shapes_and_text() {
    if slow_tests_off() {
        return;
    }
    use eda_model::ir::{Design, DrawingsSection, PlacementSection, Point, Provenance, Shape, Text, TextJustify};

    let mut drawings = DrawingsSection {
        shapes: vec![
            Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: 150, filled: false, start: Point { x: 0, y: 0 }, end: Point { x: 5_000, y: 0 } },
            Shape::Arc { id: String::new(), layer: "Cmts.User".into(), stroke_width: 100, filled: false, start: Point { x: 10_000, y: 0 }, mid: Point { x: 7_071, y: 7_071 }, end: Point { x: 0, y: 10_000 } },
            Shape::Rect { id: String::new(), layer: "F.Fab".into(), stroke_width: 100, filled: true, start: Point { x: 0, y: 0 }, end: Point { x: 8_000, y: 4_000 } },
            Shape::Circle { id: String::new(), layer: "B.SilkS".into(), stroke_width: 120, filled: false, center: Point { x: 20_000, y: 20_000 }, end: Point { x: 23_000, y: 20_000 } },
            Shape::Polygon { id: String::new(), layer: "F.CrtYd".into(), stroke_width: 50, filled: true, pts: vec![Point { x: 0, y: 0 }, Point { x: 3_000, y: 0 }, Point { x: 3_000, y: 3_000 }, Point { x: 0, y: 3_000 }] },
        ],
        texts: vec![
            Text { id: String::new(), content: "REV A".into(), at: Point { x: 1_000, y: 2_000 }, angle: 90_000, layer: "F.SilkS".into(), size_um: 1_000, stroke_width: 150, justify: TextJustify::Left, mirror: true },
            Text { id: String::new(), content: "made in eda".into(), at: Point { x: 0, y: 0 }, angle: 0, layer: "F.Fab".into(), size_um: 800, stroke_width: 120, justify: TextJustify::Center, mirror: false },
        ],
        ..Default::default()
    };
    drawings.assign_missing_ids();

    let design = Design {
        footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
        schema: 1,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "drawings_rt".into(), seed: 0, stage_hashes: vec![] },
        schematic: None, nets: None,
        placement: Some(PlacementSection {
            outline: vec![Point { x: 0, y: 0 }, Point { x: 30_000, y: 0 }, Point { x: 30_000, y: 30_000 }, Point { x: 0, y: 30_000 }],
            footprints: vec![],
            modules: vec![],
        }),
        routing: None,
        drawings: Some(drawings.clone()),
    };
    let model = ConstraintModel::default();
    let meta = ExportMeta { date: "2026-01-01", title: "drawings_rt" };
    let pcb_text = export_kicad_pcb(&design, &model, &meta).expect("export_kicad_pcb");

    let (back, _model2, _notes) = import_kicad_pcb(&pcb_text).expect("import_kicad_pcb");
    let back_dr = back.drawings.expect("shapes/text must come back");
    assert_eq!(back_dr.shapes.len(), drawings.shapes.len());
    assert_eq!(back_dr.texts.len(), drawings.texts.len());

    let by_id_before: std::collections::BTreeMap<&str, &Shape> = drawings.shapes.iter().map(|s| (s.id(), s)).collect();
    for got in &back_dr.shapes {
        let want = by_id_before.get(got.id()).unwrap_or_else(|| panic!("id {} did not round-trip (ids are content hashes, so a mismatch means the geometry changed)", got.id()));
        assert_eq!(want.points(), got.points(), "{}: points", got.id());
        assert_eq!(want.layer(), got.layer(), "{}: layer", got.id());
    }
    for (want, got) in drawings.texts.iter().zip(&back_dr.texts) {
        assert_eq!(want.id, got.id);
        assert_eq!(want.content, got.content);
        assert_eq!(want.at, got.at);
        assert_eq!(want.angle, got.angle);
        assert_eq!(want.justify, got.justify);
        assert_eq!(want.mirror, got.mirror);
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

/// What the writer keeps for kicad-cli to judge (WP5 step 1) survives KiCad's own parser and writer: `kicad-cli pcb upgrade --force`
/// loads the board with KiCad's reader and saves it with KiCad's writer, so an item KiCad did not understand is missing from what
/// comes back. Our importer then reads KiCad's file, and an arc track, a dimension, a group, the locks, a teardrop, a keepout and a
/// pour with its own settings must all still be there. (The unit tests in `pcb.rs` hold the same items to our own importer; this is
/// the same round trip with KiCad in the middle.)
#[test]
fn kicad_loads_and_resaves_arcs_dimensions_groups_locks_teardrops_and_keepouts() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    use eda_model::ir::{ArrowDirection, Dimension, DimensionKind, DimensionTextPosition, DimensionUnits, DimensionUnitsFormat, Group, Point, Zone};

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().and_then(|p| p.parent()).expect("crates/kicad -> repo root");
    let text = std::fs::read_to_string(repo_root.join("examples/two_pin_nets.yaml")).expect("read two_pin_nets");
    let model: ConstraintModel = serde_yaml::from_str(&text).expect("parse two_pin_nets");
    let opts = EngineOptions { seed: 0, intent_hash: "kicad_resave".into(), ..Default::default() };
    let design = derive_schematic(&model, &opts).expect("derive_schematic");
    let placed = place(&design, &model, &PlaceOptions { seed: 0, ..Default::default() }).expect("place");
    let mut board = route(&placed, &model, &model.board);

    let net = model.nets[0].name.clone();
    let footprint = board.placement.as_ref().unwrap().footprints[0].id.clone();
    let corner = board.placement.as_ref().unwrap().outline[0];
    let at = |dx: i64, dy: i64| Point { x: corner.x + dx, y: corner.y + dy };

    // The routing section: an arc track, a teardrop, a keepout, a pour with settings of its own.
    let routing = board.routing.as_mut().unwrap();
    routing.tracks.push(Track::new_arc(net.clone(), "F.Cu".into(), 250, at(1_000, 1_000), at(2_000, 500), at(3_000, 1_000)));
    routing.zones.push(Zone { net: net.clone(), layer: "F.Cu".into(), outline: vec![at(1_000, 3_000), at(2_000, 3_300), at(2_000, 4_300)], teardrop: true, ..Default::default() });
    routing.zones.push(Zone { layer: "F.Cu".into(), outline: vec![at(4_000, 1_000), at(7_000, 1_000), at(7_000, 3_000), at(4_000, 3_000)], is_rule_area: true, keepout_tracks: true, keepout_vias: true, ..Default::default() });
    routing.zones.push(Zone {
        net: net.clone(),
        layer: "B.Cu".into(),
        outline: vec![at(1_000, 5_000), at(6_000, 5_000), at(6_000, 8_000), at(1_000, 8_000)],
        name: "pour".into(),
        priority: 3,
        clearance: 300,
        min_thickness: 180,
        ..Default::default()
    });

    // The drawings section: a dimension and a group; locks on a footprint and on the arc track.
    let dimension = Dimension {
        id: String::new(),
        layer: "Dwgs.User".into(),
        kind: DimensionKind::Aligned { height: 2_000 },
        start: at(0, 9_000),
        end: at(8_000, 9_000),
        prefix: "L=".into(),
        suffix: "".into(),
        override_text: None,
        units: DimensionUnits::Mm,
        units_format: DimensionUnitsFormat::BareSuffix,
        precision: 2,
        suppress_trailing_zeros: true,
        text_position: DimensionTextPosition::Outside,
        keep_text_aligned: true,
        text_angle: 0,
        text_size_um: 1_200,
        stroke_width: 150,
        arrow_length: 1_270,
        extension_offset: 500,
        extension_height: 580,
        arrow_direction: ArrowDirection::Inward,
        text_thickness_um: Some(180),
    };
    board.drawings = Some(eda_model::ir::DrawingsSection { dimensions: vec![dimension.clone()], ..Default::default() });
    board.assign_missing_ids();
    let arc_id = board.routing.as_ref().unwrap().tracks.iter().find(|t| t.arc().is_some()).expect("the arc").id.clone();
    let via_id = board.routing.as_ref().unwrap().vias.first().map(|v| v.id.clone());
    let mut members = vec![footprint.clone(), arc_id.clone()];
    members.extend(via_id);
    let drawings = board.drawings.as_mut().unwrap();
    drawings.groups = vec![Group { id: String::new(), name: "kept together".into(), member_ids: members.clone() }];
    drawings.locked_ids = vec![footprint.clone(), arc_id.clone()];
    board.assign_missing_ids();

    let meta = ExportMeta { date: "2026-01-01", title: "kicad_resave" };
    let ours = export_kicad_pcb(&board, &model, &meta).unwrap_or_else(|e| panic!("export_kicad_pcb failed: {e:?}"));
    let dir = std::env::temp_dir().join(format!("eda_kicad_resave_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("board.kicad_pcb");
    std::fs::write(&path, &ours).unwrap();
    let out = Command::new(&cli).args(["pcb", "upgrade", "--force"]).arg(&path).output().expect("run kicad-cli pcb upgrade");
    assert!(out.status.success(), "kicad-cli pcb upgrade: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let theirs = std::fs::read_to_string(&path).unwrap();
    assert_ne!(ours, theirs, "KiCad wrote the file back in its own words");
    // What KiCad's own file says, before our importer reads any of it.
    // (KiCad's own writer puts a line break after the name of a block: `(arc` and a newline, where ours writes it on one line.)
    for (what, want) in [("\t(arc\n", 1), ("\t(dimension\n", 1), ("\t(group \"kept together\"", 1), ("(locked yes)", 2), ("(teardrop\n", 1), ("(keepout\n", 1)] {
        assert_eq!(theirs.matches(what).count(), want, "KiCad's file has {want} of {what:?}");
    }

    let (back, _model, _notes) = import_kicad_pcb(&theirs).unwrap_or_else(|e| panic!("import of KiCad's own file failed: {e:?}"));
    let rt = back.routing.as_ref().expect("routing comes back");
    let dr = back.drawings.as_ref().expect("drawings come back");

    // The arc is still an arc, with the same three points.
    let arcs: Vec<_> = rt.tracks.iter().filter_map(|t| t.arc()).collect();
    assert_eq!(arcs, vec![(at(1_000, 1_000), at(2_000, 500), at(3_000, 1_000))], "the arc track");
    // The dimension keeps its kind, its feature points, its height and its format.
    assert_eq!(dr.dimensions.len(), 1, "the dimension");
    let d = &dr.dimensions[0];
    assert_eq!((d.kind.clone(), d.start, d.end, d.layer.as_str()), (dimension.kind.clone(), dimension.start, dimension.end, "Dwgs.User"));
    assert_eq!((d.prefix.as_str(), d.units, d.units_format, d.precision), ("L=", DimensionUnits::Mm, DimensionUnitsFormat::BareSuffix, 2));
    // The group keeps its name and every member (the footprint, the arc track, the via if there is one).
    assert_eq!(dr.groups.len(), 1, "the group");
    assert_eq!(dr.groups[0].name, "kept together");
    assert_eq!(dr.groups[0].member_ids.len(), members.len(), "members {:?}", dr.groups[0].member_ids);
    assert!(dr.groups[0].member_ids.contains(&footprint), "the footprint is a member");
    // The locks: the footprint and the arc track, nothing else.
    assert_eq!(dr.locked_ids.len(), 2, "locked {:?}", dr.locked_ids);
    assert!(dr.locked_ids.contains(&footprint), "the footprint stays locked");
    // The zones: a teardrop, a keepout and the pour with its own settings.
    assert!(rt.zones.iter().any(|z| z.teardrop), "the teardrop flag");
    let keepout = rt.zones.iter().find(|z| z.is_rule_area).expect("the keepout");
    assert!(keepout.keepout_tracks && keepout.keepout_vias && !keepout.keepout_pads, "the keepout's own flags: {keepout:?}");
    let pour = rt.zones.iter().find(|z| z.name == "pour").expect("the pour keeps its name");
    assert_eq!((pour.priority, pour.clearance, pour.min_thickness, pour.layer.as_str()), (3, 300, 180, "B.Cu"), "the pour's own settings");
    println!("KiCad loaded and re-saved {} bytes of our board; arc, dimension, group, locks, teardrop, keepout and pour all came back", theirs.len());
}

/// Runs our pipeline, then kicad-cli, on each board: most of a minute in release and many minutes in debug, so it
/// runs only when `EDA_SLOW_TESTS` is set. `tools/check.sh full`, the check
/// before a merge lands on main, sets it.
fn slow_tests_off() -> bool {
    let off = std::env::var_os("EDA_SLOW_TESTS").is_none();
    if off {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
    }
    off
}
