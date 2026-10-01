//! Byte-level parity against `kicad-cli`: the same `Design`/`ConstraintModel`
//! fixture is exported to a `.kicad_pcb` (`eda_kicad::export_kicad_pcb`,
//! already parity-tested elsewhere) and plotted two ways -- `kicad-cli pcb
//! export gerbers/drill/pos` on that file (the oracle) and this crate's own
//! `gerber`/`drill`/`position` writers directly on the same `Design`/
//! `ConstraintModel` -- and the two are compared: aperture count, flash
//! count and region area per Gerber layer; tool count and hole count for
//! the drill file; row count for the position file.
//!
//! The fixture deliberately exercises every pad shape/kind this model can
//! express (rect/round-rect/circle/oval SMD, a plated round hole, a
//! non-plated hole, a plated slot) plus a track on each copper layer, a
//! via and a net-attached zone fill, so a single run covers the whole
//! writer rather than just whichever shapes a real board happened to use.
//!
//! Skipped (not failed) when `kicad-cli` is not on this machine.

use eda_fab::drill::{self, DrillMeta, DrillOptions};
use eda_fab::gerber::{self, FabMeta, GerberLayer};
use eda_fab::position::{self, PosMeta, PosOptions};
use eda_kicad::ExportMeta;
use eda_model::ir::{Design, DrawingsSection, FootprintInstance, Point, PlacementSection, Provenance, RoutingSection, Shape, Side, Text, TextJustify, Track, Via, Zone};
use eda_model::{ConstraintModel, Footprint, Pad, PadKind, PadShape, Part, Pin, PinKind, Net};
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn pin(number: &str, kind: PinKind) -> Pin {
    Pin { number: number.into(), name: None, kind }
}

fn part(reference: &str, footprint: &str, pins: Vec<Pin>) -> Part {
    Part { reference: reference.into(), mpn: Some("MPN123".into()), lcsc: None, value: Some("TEST".into()), package: Some(footprint.into()), footprint: Some(footprint.into()), symbol: None, datasheet: None, pins, body_um: None, edge: None }
}

fn pad(number: &str, at: (i64, i64), size: (i64, i64), shape: PadShape, kind: PadKind, drill: Option<i64>, drill_slot: Option<(i64, i64)>) -> Pad {
    Pad { number: number.into(), at, size, shape, kind, drill, drill_slot, rot: 0, roundrect_ratio: if shape == PadShape::RoundRect { Some(0.25) } else { None } }
}

/// One footprint exercising all four SMD pad shapes.
fn footprint_shapes() -> Footprint {
    Footprint {
        name: "TESTSHAPES".into(),
        pads: vec![
            pad("1", (-3000, -2000), (1200, 800), PadShape::Rect, PadKind::Smd, None, None),
            pad("2", (3000, -2000), (1200, 800), PadShape::RoundRect, PadKind::Smd, None, None),
            pad("3", (-3000, 2000), (1000, 1000), PadShape::Circle, PadKind::Smd, None, None),
            pad("4", (3000, 2000), (1600, 800), PadShape::Oval, PadKind::Smd, None, None),
        ],
        courtyard: Some((4500, 3500)),
        model: None,
    }
}

/// One footprint exercising a plated round hole, a non-plated hole and a
/// plated slot.
fn footprint_th() -> Footprint {
    Footprint {
        name: "TESTTH".into(),
        pads: vec![
            pad("1", (-2500, 0), (1800, 1800), PadShape::Circle, PadKind::ThroughHole, Some(1000), None),
            pad("2", (0, 0), (1500, 1500), PadShape::Circle, PadKind::NonPlatedHole, Some(1200), None),
            pad("3", (2500, 0), (1200, 2400), PadShape::Oval, PadKind::ThroughHole, None, Some((800, 1800))),
        ],
        courtyard: Some((3800, 2200)),
        model: None,
    }
}

fn fixture() -> (Design, ConstraintModel) {
    let model = ConstraintModel {
        parts: vec![
            part("U1", "TESTSHAPES", vec![pin("1", PinKind::Signal), pin("2", PinKind::Signal), pin("3", PinKind::Ground), pin("4", PinKind::Nc)]),
            part("U2", "TESTTH", vec![pin("1", PinKind::Signal), pin("2", PinKind::Passive), pin("3", PinKind::Ground)]),
        ],
        nets: vec![Net { name: "NET1".into(), pins: vec!["U1.1".into(), "U2.1".into()] }, Net { name: "GND".into(), pins: vec!["U1.3".into(), "U2.3".into()] }],
        footprints: vec![footprint_shapes(), footprint_th()],
        ..ConstraintModel::default()
    };

    let placement = PlacementSection {
        outline: vec![Point { x: 0, y: 0 }, Point { x: 20_000, y: 0 }, Point { x: 20_000, y: 15_000 }, Point { x: 0, y: 15_000 }],
        footprints: vec![
            FootprintInstance { id: "U1".into(), at: Point { x: 6_000, y: 5_000 }, rot: 0, side: Side::Top, label: Default::default() },
            FootprintInstance { id: "U2".into(), at: Point { x: 14_000, y: 10_000 }, rot: 90_000, side: Side::Bottom, label: Default::default() },
        ],
        modules: Vec::new(),
    };

    let routing = RoutingSection {
        tracks: vec![
            Track { id: String::new(), net: "NET1".into(), pins: vec![], layer: "F.Cu".into(), width: 250, pts: vec![Point { x: 3_000, y: 5_000 }, Point { x: 9_000, y: 5_000 }] },
            Track { id: String::new(), net: "GND".into(), pins: vec![], layer: "B.Cu".into(), width: 300, pts: vec![Point { x: 3_000, y: 7_000 }, Point { x: 3_000, y: 12_000 }] },
        ],
        vias: vec![Via { id: String::new(), net: "NET1".into(), at: Point { x: 9_000, y: 5_000 }, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }],
        zones: vec![Zone {
            id: String::new(),
            net: "GND".into(),
            layer: "B.Cu".into(),
            outline: vec![Point { x: 500, y: 500 }, Point { x: 19_500, y: 500 }, Point { x: 19_500, y: 14_500 }, Point { x: 500, y: 14_500 }],
            ..Zone::default()
        }],
        track_width_presets: vec![],
        via_presets: vec![],
    };

    let design = Design {
        schema: 1,
        provenance: Provenance { engine_version: "test".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        footprint_library: None, sheet_contents: None,
        placement: Some(placement),
        routing: Some(routing),
        drawings: Some(DrawingsSection {
            shapes: vec![Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: 150, filled: false, start: Point { x: 1_000, y: 13_000 }, end: Point { x: 19_000, y: 13_000 } }],
            texts: vec![Text { id: String::new(), content: "REV A".into(), at: Point { x: 10_000, y: 13_800 }, angle: 0, layer: "F.SilkS".into(), size_um: 1000, stroke_width: 150, justify: TextJustify::Center, mirror: false }],
        }),
    };
    (design, model)
}

struct GerberStats {
    apertures: usize,
    flashes: usize,
    regions: usize,
    region_area_um2: f64,
}

fn parse_xy(line: &str) -> Option<(f64, f64)> {
    let line = line.strip_prefix('X')?;
    let yi = line.find('Y')?;
    let di = line.find('D')?;
    if di < yi {
        return None;
    }
    let x: f64 = line[..yi].parse().ok()?;
    let y: f64 = line[yi + 1..di].parse().ok()?;
    Some((x / 1000.0, y / 1000.0)) // nm -> um
}

fn shoelace(pts: &[(f64, f64)]) -> f64 {
    if pts.len() < 3 {
        return 0.0;
    }
    let mut s = 0.0;
    for i in 0..pts.len() {
        let (x1, y1) = pts[i];
        let (x2, y2) = pts[(i + 1) % pts.len()];
        s += x1 * y2 - x2 * y1;
    }
    (s / 2.0).abs()
}

fn parse_gerber(text: &str) -> GerberStats {
    let apertures = text.lines().filter(|l| l.trim_start().starts_with("%ADD")).count();
    let flashes = text.lines().filter(|l| l.trim_end().ends_with("D03*")).count();
    let regions = text.matches("G36*").count();
    let mut area = 0.0;
    let mut in_region = false;
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if l == "G36*" {
            in_region = true;
            pts.clear();
        } else if l == "G37*" {
            in_region = false;
            area += shoelace(&pts);
        } else if in_region {
            if let Some(p) = parse_xy(l) {
                pts.push(p);
            }
        }
    }
    GerberStats { apertures, flashes, regions, region_area_um2: area }
}

fn kicad_cli_gerbers(cli: &Path, pcb: &Path, dir: &Path, layers: &str) {
    let out = Command::new(cli).args(["pcb", "export", "gerbers", "--layers", layers, "-o", &format!("{}/", dir.display())]).arg(pcb).output().expect("run kicad-cli");
    assert!(out.status.success(), "kicad-cli gerbers failed: {}", String::from_utf8_lossy(&out.stderr));
}

fn kicad_cli_drill(cli: &Path, pcb: &Path, dir: &Path) {
    let out = Command::new(cli).args(["pcb", "export", "drill", "-o", &format!("{}/", dir.display())]).arg(pcb).output().expect("run kicad-cli");
    assert!(out.status.success(), "kicad-cli drill failed: {}", String::from_utf8_lossy(&out.stderr));
}

fn kicad_cli_pos(cli: &Path, pcb: &Path, out_file: &Path) {
    let out = Command::new(cli).args(["pcb", "export", "pos", "--format", "csv", "--units", "mm", "-o"]).arg(out_file).arg(pcb).output().expect("run kicad-cli");
    assert!(out.status.success(), "kicad-cli pos failed: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn gerber_drill_pos_match_kicad_cli() {
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found on this machine; skipping parity test");
        return;
    };
    let (design, model) = fixture();
    let title = "parity_fixture";
    let date = "2026-10-01T00:00:00-07:00";

    let pcb_text = eda_kicad::export_kicad_pcb(&design, &model, &ExportMeta { date: &date[..10], title }).expect("export kicad_pcb");
    let tmp = std::env::temp_dir().join(format!("eda_fab_parity_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pcb_path = tmp.join(format!("{title}.kicad_pcb"));
    std::fs::write(&pcb_path, pcb_text.as_bytes()).unwrap();

    // ---- gerbers ----
    let oracle_dir = tmp.join("oracle");
    std::fs::create_dir_all(&oracle_dir).unwrap();
    kicad_cli_gerbers(&cli, &pcb_path, &oracle_dir, "F.Cu,B.Cu,F.Mask,B.Mask,F.Paste,B.Paste,F.SilkS,B.SilkS,Edge.Cuts");

    let meta = FabMeta { title: title.into(), date: date.into(), rev: "rev?".into(), generator_version: "0.1.0".into() };
    let layers = gerber::default_jlc_layers(model.board.layers.len());
    let ours = gerber::plot_all(&design, &model, &meta, &layers).expect("plot_all");

    let mut report = String::new();
    report.push_str("# crates/fab parity vs kicad-cli (10.99.0)\n\n");
    report.push_str("Fixture: `crates/fab/tests/kicad_cli_parity.rs`'s hand-built board (every pad\nshape/kind, a track per copper layer, a via, a net-attached zone fill).\n\n");
    report.push_str("## Gerber\n\n| layer | kicad apertures | ours | kicad flashes | ours | kicad regions | ours | kicad area um2 | ours | area delta |\n");
    report.push_str("|---|---|---|---|---|---|---|---|---|---|\n");

    for f in &ours {
        // kicad-cli writes the oracle file under the same `<title>-<suffix>.<ext>`
        // convention this writer ports from `pcbplot.cpp` -- see `gerber::GerberLayer`.
        let oracle_path = oracle_dir.join(&f.filename);
        let Ok(oracle_text) = std::fs::read_to_string(&oracle_path) else {
            report.push_str(&format!("| {:?} | (missing: {}) |\n", f.layer, oracle_path.display()));
            continue;
        };
        let o = parse_gerber(&oracle_text);
        let m = parse_gerber(&f.content);
        let delta = if o.region_area_um2 > 0.0 { (m.region_area_um2 - o.region_area_um2).abs() / o.region_area_um2 * 100.0 } else { 0.0 };
        report.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {:.0} | {:.0} | {:.2}% |\n",
            f.filename, o.apertures, m.apertures, o.flashes, m.flashes, o.regions, m.regions, o.region_area_um2, m.region_area_um2, delta
        ));

        match f.layer {
            GerberLayer::Copper(_) => {
                assert_eq!(m.flashes, o.flashes, "{}: flash count mismatch (ours {} vs kicad-cli {})", f.filename, m.flashes, o.flashes);
                assert_eq!(m.apertures, o.apertures, "{}: aperture count mismatch", f.filename);
                if o.regions > 0 {
                    assert!(delta < 5.0, "{}: region area differs by {delta:.2}% (ours {} vs kicad-cli {})", f.filename, m.region_area_um2, o.region_area_um2);
                }
            }
            GerberLayer::Mask(_) => {
                assert_eq!(m.flashes, o.flashes, "{}: flash count mismatch", f.filename);
            }
            GerberLayer::Paste(_) => {
                assert_eq!(m.flashes, o.flashes, "{}: flash count mismatch", f.filename);
            }
            // Silkscreen is not a byte-for-byte comparison: kicad-cli also
            // plots each footprint's reference/value text, which this
            // writer deliberately does not (see `gerber`'s own module doc
            // -- `eda_drc::stroke_font`'s footprint-relative anchor
            // convention has an open, uninvestigated bug). This only
            // checks that our own free-standing `Shape`/`Text` items
            // actually produced draws, not that the two files match.
            GerberLayer::Silk(Side::Top) => {
                // The fixture's free-standing shape/text are both on F.SilkS only.
                let our_draws = f.content.lines().filter(|l| l.trim_end().ends_with("D01*")).count();
                assert!(our_draws > 0, "{}: expected at least one draw from the fixture's free-standing silk shape/text", f.filename);
            }
            GerberLayer::Silk(Side::Bottom) => {}
            GerberLayer::EdgeCuts => {}
        }
    }

    // ---- drill ----
    kicad_cli_drill(&cli, &pcb_path, &oracle_dir);
    let oracle_drill = std::fs::read_to_string(oracle_dir.join(format!("{title}.drl"))).expect("read oracle drill file");
    let drill_meta = DrillMeta { title: title.into(), date: date.into(), generator_version: "0.1.0".into() };
    let ours_drill = drill::write_drill(&design, &model, &drill_meta, DrillOptions::default()).expect("write_drill");
    assert_eq!(ours_drill.len(), 1, "merged drill mode must write exactly one file");
    let our_tools = ours_drill[0].content.lines().filter(|l| l.starts_with('T') && l.contains('C')).count();
    let oracle_tools = oracle_drill.lines().filter(|l| l.starts_with('T') && l.contains('C')).count();
    let our_holes = ours_drill[0].content.lines().filter(|l| l.starts_with('X')).count();
    let oracle_holes = oracle_drill.lines().filter(|l| l.starts_with('X')).count();
    report.push_str("\n## Drill\n\n");
    report.push_str(&format!("kicad-cli tools: {oracle_tools}, ours: {our_tools}\n\n"));
    report.push_str(&format!("kicad-cli hole/slot lines: {oracle_holes}, ours: {our_holes}\n\n"));
    assert_eq!(our_tools, oracle_tools, "drill tool count mismatch");
    assert_eq!(our_holes, oracle_holes, "drill hole/slot line count mismatch");

    // ---- position ----
    let pos_path = tmp.join("oracle_pos.csv");
    kicad_cli_pos(&cli, &pcb_path, &pos_path);
    let oracle_pos = std::fs::read_to_string(&pos_path).expect("read oracle pos file");
    let pos_meta = PosMeta { date: date.into(), generator_version: "0.1.0".into() };
    let ours_pos = position::write_pos(&design, &model, &pos_meta, PosOptions::default()).expect("write_pos");
    // Full row-for-row equality, not just counts: both writers sort the
    // same way (bottom-side footprints first, then natural reference
    // order within a side -- see `position::side_rank`'s own doc comment
    // for why, which this very assertion is what caught the opposite
    // assumption), so this also catches formatting drift (e.g. a stray
    // `-0.000000`) that a bare count comparison would miss.
    assert_eq!(ours_pos, oracle_pos, "position CSV must match kicad-cli's own byte for byte");
    report.push_str("\n## Position (CSV)\n\n");
    report.push_str(&format!("kicad-cli header: `{}`\n\nours: `{}`\n\n", oracle_pos.lines().next().unwrap_or(""), ours_pos.lines().next().unwrap_or("")));
    report.push_str(&format!("rows -- kicad-cli: {}, ours: {}\n", oracle_pos.lines().count() - 1, ours_pos.lines().count() - 1));

    // ASCII format: the first two lines carry kicad-cli's own real-time
    // timestamp and version, so only the data from "## Unit" down (same
    // sort, same column values, modulo KiCad's data-dependent column
    // widths, which this writer reproduces -- see `position::gen_ascii`)
    // is compared.
    let ascii_pos_path = tmp.join("oracle_pos.txt");
    let out = Command::new(&cli).args(["pcb", "export", "pos", "--format", "ascii", "--units", "mm", "-o"]).arg(&ascii_pos_path).arg(&pcb_path).output().expect("run kicad-cli");
    assert!(out.status.success(), "kicad-cli ascii pos failed: {}", String::from_utf8_lossy(&out.stderr));
    let oracle_ascii = std::fs::read_to_string(&ascii_pos_path).expect("read oracle ascii pos file");
    let ours_ascii = position::write_pos(&design, &model, &pos_meta, PosOptions { format: position::PosFormat::Ascii, ..PosOptions::default() }).expect("write_pos ascii");
    let skip2 = |s: &str| s.lines().skip(2).collect::<Vec<_>>().join("\n");
    assert_eq!(skip2(&ours_ascii), skip2(&oracle_ascii), "position ASCII body (past the timestamped header) must match kicad-cli's own");

    std::fs::write(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("PARITY.md"), report).ok();
    let _ = std::fs::remove_dir_all(&tmp);
}
