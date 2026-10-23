//! The multichannel tool through a board folder (GAPS.md items 24 and 32, package P24): Generate Placement Rule Areas and Repeat Layout are one undo step each of the board's
//! history and reach the derived `.kicad_pcb`; and (the slow tier, `EDA_SLOW_TESTS=1`) kicad-cli reads the rule areas this studio writes as it reads its own rewrite of them, and finds
//! the channels a Repeat Layout made as clean as the one it copied -- not one violation the first channel does not have, every channel with the same violations at the same places.
//!
//! The board is the one of `web/studio/e2e/multichannel-board.yaml` (three amplifier channels): channel 1 laid out compactly and routed by the router, the other two placed in
//! other arrangements and not routed.

use super::tests::scratch;
use super::{load, meta_path, save, step, undo, Meta};
use eda_model::ir::{Design, FootprintInstance, PlacementSection, Point, Provenance, Side};
use eda_model::multichannel::point_inside;
use eda_model::ConstraintModel;
use eda_ops::{AreaKind, AreaPick, Board, Cmd, Domain, MultichannelCmd, RepeatOptions};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn slow() -> bool {
    let off = std::env::var_os("EDA_SLOW_TESTS").is_none();
    if off {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
    }
    !off
}

/// Where the footprints go (um): channel 1 compact, the others in their own ways.
const PLACE: [(&str, i64, i64); 12] = [
    ("U1", 5_500_000, 9_000_000),
    ("R1", 3_500_000, 4_200_000),
    ("R2", 7_700_000, 4_200_000),
    ("C1", 5_500_000, 13_200_000),
    ("U2", 18_000_000, 9_000_000),
    ("R3", 20_200_000, 4_200_000),
    ("R4", 15_800_000, 4_200_000),
    ("C2", 18_000_000, 14_000_000),
    ("U3", 30_500_000, 9_000_000),
    ("R5", 30_500_000, 4_200_000),
    ("R6", 32_500_000, 13_500_000),
    ("C3", 28_000_000, 13_500_000),
];

const CH1: [&str; 4] = ["U1", "R1", "R2", "C1"];

fn intent_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/studio/e2e/multichannel-board.yaml")
}

/// The intent of the three channels, with the built-in footprints of its packages.
fn full_model() -> ConstraintModel {
    serde_yaml::from_str(&std::fs::read_to_string(intent_path()).expect("the e2e intent")).expect("the e2e intent parses")
}

/// Channel 1 alone: its four parts and the nets between them (the router routes what is placed).
fn channel_one(full: &ConstraintModel) -> ConstraintModel {
    let mut m = full.clone();
    m.parts.retain(|p| CH1.contains(&p.reference.as_str()));
    for n in &mut m.nets {
        n.pins.retain(|p| CH1.contains(&p.split('.').next().unwrap_or("")));
    }
    m.nets.retain(|n| n.pins.len() >= 2);
    m
}

fn design(placed: &[(&str, i64, i64)]) -> Design {
    Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "t".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(PlacementSection {
            outline: vec![Point { x: 0, y: 0 }, Point { x: 36_000_000, y: 0 }, Point { x: 36_000_000, y: 25_000_000 }, Point { x: 0, y: 25_000_000 }],
            footprints: placed.iter().map(|(id, x, y)| FootprintInstance { id: id.to_string(), at: Point { x: *x, y: *y }, rot: 0, side: Side::Top, label: Default::default() }).collect(),
            modules: vec![],
        }),
        routing: None,
        drawings: None,
    }
}

/// A board folder of the three channels: channel 1 routed, the other two not.
fn channels(dir: &Path) {
    let full = full_model();
    let one = channel_one(&full);
    let (routed, fails) = eda::route_partial(&design(&PLACE[..4]), &one, &one.board);
    let routed = routed.expect("channel 1 routes");
    assert!(fails.iter().all(|f| f.check != "route_net_unrouted"), "channel 1 is routed whole: {fails:?}");
    let mut all = design(&PLACE);
    all.routing = routed.routing;
    assert!(all.routing.as_ref().is_some_and(|r| r.tracks.len() >= 3), "the router drew channel 1's copper");
    write_board(dir, &all);
}

/// `design` as the board folder `dir`'s design, with the intent of the three channels.
fn write_board(dir: &Path, design: &Design) {
    save(dir, design).unwrap();
    let meta = Meta { intent: intent_path().canonicalize().unwrap().display().to_string(), snap_nm: 100_000, spacing_nm: 600_000 };
    std::fs::write(meta_path(dir), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
}

fn design_json(dir: &Path) -> Value {
    serde_json::to_value(load(dir).unwrap().1).unwrap()
}

/// Generate Placement Rule Areas with every sheet ticked, as the dialog's OK sends it.
fn generate(dir: &Path) {
    let (meta, design, model) = load(dir).unwrap();
    let rows = Board::new(design, &model, meta.snap_nm, meta.spacing_nm).mc_potential_areas();
    let areas: Vec<AreaPick> = rows.iter().filter(|r| r.kind == AreaKind::Sheet).map(|r| AreaPick { kind: r.kind, key: r.key.clone() }).collect();
    assert_eq!(areas.len(), 3, "one sheet for each amplifier: {rows:?}");
    step(dir, Cmd::Multichannel(MultichannelCmd::GenerateRuleAreas { areas, replace_existing: false, group_items: false }), false, "test").unwrap();
}

/// The rule areas as `(id, source, outline)`, in the order of their sources.
fn rule_areas(dir: &Path) -> Vec<(String, String, Vec<Point>)> {
    let mut areas: Vec<(String, String, Vec<Point>)> = load(dir)
        .unwrap()
        .1
        .routing
        .map(|r| r.zones.into_iter().filter(|z| z.is_rule_area && z.placement.enabled).map(|z| (z.id, z.placement.source, z.outline)).collect())
        .unwrap_or_default();
    areas.sort_by(|a, b| a.1.cmp(&b.1));
    areas
}

/// The router drew channel 1's copper a little outside the hull of its footprints (a track 0.2 mm beyond the area's edge), and a track that is not inside the area is not copied: as a
/// person does in KiCad when the area is too small, the reference area is made to enclose its routing (the edit of the outline's corners, `Cmd::SetZoneOutline`).
fn enclose_the_routing(dir: &Path) {
    let (_, design, _) = load(dir).unwrap();
    let areas = rule_areas(dir);
    let (id, _, outline) = &areas[0];
    let mut all: Vec<Point> = outline.clone();
    for t in &design.routing.as_ref().unwrap().tracks {
        all.extend(t.pts.iter().copied());
    }
    let (x0, x1) = (all.iter().map(|p| p.x).min().unwrap() - 500_000, all.iter().map(|p| p.x).max().unwrap() + 500_000);
    let (y0, y1) = (all.iter().map(|p| p.y).min().unwrap() - 500_000, all.iter().map(|p| p.y).max().unwrap() + 500_000);
    let rect = vec![Point { x: x0, y: y0 }, Point { x: x1, y: y0 }, Point { x: x1, y: y1 }, Point { x: x0, y: y1 }];
    step(dir, Cmd::SetZoneOutline { id: id.clone(), outline: rect }, false, "test").unwrap();
}

/// Repeat Layout from the first area onto the others, with KiCad's default options, as the dialog's OK sends it.
fn repeat(dir: &Path) {
    let areas = rule_areas(dir);
    let targets = areas[1..].iter().map(|a| a.0.clone()).collect();
    step(dir, Cmd::Multichannel(MultichannelCmd::RepeatLayout { reference: areas[0].0.clone(), targets, options: RepeatOptions::default() }), false, "test").unwrap();
}

fn pose(dir: &Path, id: &str) -> (Point, u32) {
    let f = load(dir).unwrap().1.placement.unwrap().footprints.into_iter().find(|f| f.id == id).unwrap();
    (f.at, f.rot)
}

#[test]
fn generate_and_repeat_are_one_undo_step_each_and_the_areas_are_in_the_board_file_kicad_reads() {
    let dir = scratch("p24_folder");
    channels(&dir);
    let start = design_json(&dir);
    generate(&dir);
    let areas = rule_areas(&dir);
    assert_eq!(areas.len(), 3);
    let generated = design_json(&dir);
    assert_ne!(generated, start);

    // the derived board file, as `kicad-cli` is given it, names each area's sheet
    let (_, design, model) = load(&dir).unwrap();
    let text = eda_kicad::export_kicad_pcb(&design, &model, &eda_kicad::ExportMeta { date: "2026-10-21", title: "p24" }).unwrap();
    for (_, source, _) in &areas {
        assert!(text.contains(&format!("(placement (enabled yes) (sheetname \"{source}\"))")), "{source}: {}", text.lines().filter(|l| l.contains("placement")).collect::<Vec<_>>().join("\n"));
    }

    // Repeat Layout: channels 2 and 3 are channel 1's layout moved, in one step
    let u1 = pose(&dir, "U1");
    enclose_the_routing(&dir);
    let enclosed = design_json(&dir);
    repeat(&dir);
    let (u2, u3) = (pose(&dir, "U2"), pose(&dir, "U3"));
    for (rin, rf, c, u) in [("R3", "R4", "C2", u2), ("R5", "R6", "C3", u3)] {
        let by = (u.0.x - u1.0.x, u.0.y - u1.0.y);
        for (mine, first) in [(rin, "R1"), (rf, "R2"), (c, "C1")] {
            let (a, b) = (pose(&dir, mine), pose(&dir, first));
            assert_eq!((a.0.x - b.0.x, a.0.y - b.0.y, a.1), (by.0, by.1, b.1), "{mine} is {first} moved by the vector of the amplifiers");
        }
    }
    let repeated = design_json(&dir);
    assert_eq!(repeated["routing"]["tracks"].as_array().unwrap().len() % 3, 0, "channel 1's tracks, and the same on each of the others: {}", repeated["routing"]["tracks"]);
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(design_json(&dir), enclosed, "one undo takes the whole repeat back");
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(design_json(&dir), generated, "one the edit of the area's corners");
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(design_json(&dir), start, "and one the three areas");
}

#[test]
fn strict_refuses_neither_the_areas_nor_the_repeat_on_a_board_that_is_not_routed_yet() {
    // The first zone of a board with no routing starts the routing section, and with it the connectivity gate's judgement of every net: a board judged to have no failure is then
    // judged to have eight (the nets of the three channels are open). A rule area moves nothing and routes nothing, so Strict -- the studio's Strict switch, which refuses a move
    // that makes the board worse -- does not refuse Generate; and the Repeat Layout that follows moves footprints into places that are free, which does not make it worse.
    let dir = scratch("p24_strict");
    write_board(&dir, &design(&PLACE));
    assert!(design_json(&dir)["routing"].is_null(), "no routing yet");
    let (meta, d, model) = load(&dir).unwrap();
    let rows = Board::new(d, &model, meta.snap_nm, meta.spacing_nm).mc_potential_areas();
    let areas: Vec<AreaPick> = rows.iter().filter(|r| r.kind == AreaKind::Sheet).map(|r| AreaPick { kind: r.kind, key: r.key.clone() }).collect();
    step(&dir, Cmd::Multichannel(MultichannelCmd::GenerateRuleAreas { areas, replace_existing: false, group_items: false }), true, "test").expect("Strict lets the rule areas be made");
    let found = rule_areas(&dir);
    assert_eq!(found.len(), 3);
    step(&dir, Cmd::Multichannel(MultichannelCmd::RepeatLayout { reference: found[0].0.clone(), targets: found[1..].iter().map(|a| a.0.clone()).collect(), options: RepeatOptions::default() }), true, "test").expect("and the layout be repeated");
    let (u1, u2) = (pose(&dir, "U1"), pose(&dir, "U2"));
    assert_ne!(u1.0, u2.0);
    // the same board, a copper pour on it instead: the gate does judge that (it is what the exemption is not)
    let other = scratch("p24_strict_pour");
    write_board(&other, &design(&PLACE));
    let pour = Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: vec![Point { x: 1_000_000, y: 16_000_000 }, Point { x: 8_000_000, y: 16_000_000 }, Point { x: 8_000_000, y: 20_000_000 }, Point { x: 1_000_000, y: 20_000_000 }] };
    assert_eq!(step(&other, pour, true, "test").unwrap_err()[0].check, "board_worse", "the first pour of a board with no routing is judged, a rule area is not");
}

/// `(zone ...)` blocks of a board file that are placement rule areas, each as its text.
fn placement_zones(board: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = board[at..].find("(zone") {
        let start = at + i;
        let mut depth = 0;
        let mut end = start;
        for (j, ch) in board[start..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + j + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        let block = &board[start..end];
        // KiCad's writer breaks the lines where this studio's does not: look at the block with its white space as one space
        if block.split_whitespace().collect::<Vec<_>>().join(" ").contains("(placement (enabled yes)") {
            out.push(block.to_string());
        }
        at = end;
    }
    out
}

/// The numbers of the `(xy x y)` points of a block, in order.
fn corners(block: &str) -> Vec<(f64, f64)> {
    block
        .split("(xy ")
        .skip(1)
        .filter_map(|p| {
            let mut n = p.split(|c: char| c == ' ' || c == ')').filter(|s| !s.is_empty());
            Some((n.next()?.parse().ok()?, n.next()?.parse().ok()?))
        })
        .collect()
}

/// The `(placement ..)` expression of a zone block, its white space as one space (KiCad's writer breaks the lines where this studio's does not).
fn placement_of(block: &str) -> String {
    let Some(start) = block.find("(placement") else { return String::new() };
    let mut depth = 0;
    let mut end = start;
    for (j, ch) in block[start..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = start + j + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    block[start..end].split_whitespace().collect::<Vec<_>>().join(" ").replace("( ", "(").replace(" )", ")")
}

#[test]
fn kicad_cli_reads_the_rule_areas_as_its_own_rewrite_and_finds_the_repeated_channels_as_clean_as_the_first() {
    if !slow() {
        return;
    }
    let Some(cli) = eda_kicad_engine::find_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let dir = scratch("p24_cli");
    channels(&dir);
    let first = crate::kicad_engine::drc(&dir, false).expect("kicad-cli's DRC of the board with channel 1 routed");
    let kinds = |r: &Value| -> BTreeMap<String, u64> { r["counts"].as_object().map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_u64().unwrap_or(0))).collect()).unwrap_or_default() };
    let first_counts = kinds(&first);
    eprintln!("before: {first_counts:?}");
    assert!(first_counts.get("unconnected_items").copied().unwrap_or(0) > 0, "channels 2 and 3 are not routed: their nets are open: {first_counts:?}");

    // Generate Placement Rule Areas: kicad-cli loads the board with the areas, and its rewrite of the file (`pcb upgrade --force`, the board read with KiCad's parser and written with
    // KiCad's writer) keeps what the studio wrote: each area's placement source and its corners
    generate(&dir);
    let areas = rule_areas(&dir);
    crate::kicad_engine::drc(&dir, false).expect("kicad-cli loads the board with the rule areas");
    let derived = dir.join(".kicad").join("board.kicad_pcb");
    let ours = std::fs::read_to_string(&derived).expect("the derived board file");
    let ours_zones = placement_zones(&ours);
    assert_eq!(ours_zones.len(), 3, "{ours}");
    let copy = dir.join("upgraded.kicad_pcb");
    std::fs::copy(&derived, &copy).unwrap();
    let done = eda_kicad_engine::cli_command(&cli, None).args(["pcb", "upgrade", "--force"]).arg(&copy).output().expect("kicad-cli runs");
    assert!(done.status.success(), "pcb upgrade: {}{}", String::from_utf8_lossy(&done.stdout), String::from_utf8_lossy(&done.stderr));
    let theirs_zones = placement_zones(&std::fs::read_to_string(&copy).unwrap());
    assert_eq!(theirs_zones.len(), 3, "kicad-cli's rewrite keeps the three areas with their placement");
    for (_, source, outline) in &areas {
        // each file lists the zones in an order of its own: find the area by its name
        let name = format!("(name \"auto-placement-area-{source}\")");
        let find = |zones: &[String]| zones.iter().find(|z| z.split_whitespace().collect::<Vec<_>>().join(" ").contains(&name)).cloned().unwrap_or_else(|| panic!("no zone {name} in {} zones", zones.len()));
        let (mine, kept) = (find(&ours_zones), find(&theirs_zones));
        assert_eq!(placement_of(&mine), placement_of(&kept), "{source}: the placement line is KiCad's");
        assert!(placement_of(&kept).contains(&format!("(sheetname \"{source}\")")), "{}", placement_of(&kept));
        let (a, b) = (corners(&mine), corners(&kept));
        assert_eq!(a.len(), outline.len(), "{source}: every corner is written");
        assert_eq!(a.len(), b.len(), "{source}: kicad-cli keeps every corner");
        for ((x1, y1), (x2, y2)) in a.iter().zip(b.iter()) {
            assert!((x1 - x2).abs() < 1e-6 && (y1 - y2).abs() < 1e-6, "{source}: ({x1}, {y1}) against ({x2}, {y2})");
        }
        for (p, (x, y)) in outline.iter().zip(a.iter()) {
            assert!(((p.x as f64 / 1e6) - x).abs() < 1e-6 && ((p.y as f64 / 1e6) - y).abs() < 1e-6, "{source}: the corner of the area is the corner written");
        }
    }

    // Repeat Layout, then kicad-cli's DRC again
    enclose_the_routing(&dir);
    repeat(&dir);
    let after = crate::kicad_engine::drc(&dir, false).expect("kicad-cli's DRC of the repeated board");
    let after_counts = kinds(&after);
    eprintln!("after: {after_counts:?}");
    // the channels' nets are closed now: fewer open ends than before
    assert!(after_counts.get("unconnected_items").copied().unwrap_or(0) < first_counts.get("unconnected_items").copied().unwrap_or(0), "{first_counts:?} then {after_counts:?}");
    // the violations of each channel, by the area its items are in
    let areas = rule_areas(&dir);
    let mut per: BTreeMap<(usize, String), u64> = BTreeMap::new();
    for v in after["violations"].as_array().unwrap().iter().chain(after["unconnected_items"].as_array().unwrap().iter()) {
        let at: Vec<Point> = v["items"].as_array().unwrap().iter().map(|i| Point { x: i["pos"][0].as_i64().unwrap_or(0), y: i["pos"][1].as_i64().unwrap_or(0) }).collect();
        if at.is_empty() {
            continue;
        }
        if let Some(k) = (0..areas.len()).find(|&k| at.iter().all(|p| point_inside(&areas[k].2, *p))) {
            *per.entry((k, v["type"].as_str().unwrap_or("?").to_string())).or_default() += 1;
        }
    }
    let mut all_kinds: Vec<String> = per.keys().map(|(_, kind)| kind.clone()).collect();
    all_kinds.sort();
    all_kinds.dedup();
    for kind in &all_kinds {
        let n: Vec<u64> = (0..3).map(|k| per.get(&(k, kind.clone())).copied().unwrap_or(0)).collect();
        assert!(n[0] == n[1] && n[1] == n[2], "{kind}: the three channels have the same violations of it (channel 1, 2, 3: {n:?}) -- all: {per:?}");
    }
    // and the kinds channel 1 has none of -- every kind of copper violation among them -- are no more than before the repeat
    for (kind, now) in &after_counts {
        let in_first_channel = per.get(&(0, kind.clone())).copied().unwrap_or(0);
        if in_first_channel == 0 && kind != "unconnected_items" {
            assert!(*now <= first_counts.get(kind).copied().unwrap_or(0), "{kind}: {now} after the repeat, {:?} before", first_counts.get(kind));
        }
    }
    for kind in ["clearance", "shorting_items", "tracks_crossing", "hole_clearance", "copper_edge_clearance", "courtyards_overlap", "track_dangling", "via_dangling"] {
        assert_eq!(after_counts.get(kind).copied().unwrap_or(0), first_counts.get(kind).copied().unwrap_or(0), "{kind}: the repeat adds none: {after_counts:?}");
    }
}
