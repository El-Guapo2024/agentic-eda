//! A board outline imported from a DXF is a board outline to KiCad: kicad-cli reads the board `Cmd::ImportGraphics` made, its DRC finds the
//! Edge.Cuts closed (and an outline with a side missing not), and its own DXF export of Edge.Cuts is the outline that was imported, the corners
//! still arcs. Slow tier: kicad-cli runs, so it needs `EDA_SLOW_TESTS=1`; skipped, loudly, without kicad-cli.

use eda_kicad::dxf_import::{read_dxf, DxfOptions};
use eda_kicad::graphics_import::ImportedShape;
use eda_kicad::{export_kicad_pcb, ExportMeta};
use eda_model::ir::{Design, PlacementSection, Provenance};
use eda_model::ConstraintModel;
use eda_ops::{Board, Cmd, GraphicsFormat, ImportGraphics};
use std::path::{Path, PathBuf};
use std::process::Command;

fn find_kicad_cli() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).filter(|p| p.exists()) {
        return Some(p);
    }
    if let Ok(out) = Command::new("which").arg("kicad-cli").output() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    let mac = PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli");
    mac.exists().then_some(mac)
}

fn slow_tests_off() -> bool {
    let off = std::env::var_os("EDA_SLOW_TESTS").is_none();
    if off {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
    }
    off
}

fn fixture(name: &str) -> String {
    let path = format!("{}/../kicad/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("eda_import_gfx_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// An empty board with no outline of its own: whatever is on Edge.Cuts is its outline.
fn empty_design() -> Design {
    Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "t".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(PlacementSection { outline: vec![], footprints: vec![], modules: vec![] }),
        routing: None,
        drawings: None,
    }
}

fn import(text: String, fix: Option<i64>) -> (Design, ConstraintModel) {
    let model = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &model, 100_000, 300_000);
    let req = ImportGraphics { format: GraphicsFormat::Dxf, text, layer: "Edge.Cuts".into(), dxf_units: String::new(), scale: 1.0, offset_x_nm: 0, offset_y_nm: 0, line_width_nm: 100_000, group: false, fix_tolerance_nm: fix, replace_outline: false };
    b.apply(&Cmd::ImportGraphics(req)).expect("the import applies");
    (b.design().clone(), model)
}

fn write_board(dir: &Path, design: &Design, model: &ConstraintModel) -> PathBuf {
    let text = export_kicad_pcb(design, model, &ExportMeta { date: "2026-10-10", title: "import" }).expect("the board exports");
    let pcb = dir.join("board.kicad_pcb");
    std::fs::write(&pcb, text).unwrap();
    pcb
}

/// The DXF without its first record of this kind (the group 0 that names it and the groups up to the next group 0).
fn drop_first_record(dxf: &str, kind: &str) -> String {
    let lines: Vec<&str> = dxf.lines().collect();
    let pairs: Vec<(&str, &str)> = lines.chunks(2).map(|c| (c[0], c.get(1).copied().unwrap_or(""))).collect();
    let at = pairs.iter().position(|(c, v)| c.trim() == "0" && v.trim() == kind).expect("a record of that kind");
    let end = (at + 1..pairs.len()).find(|&i| pairs[i].0.trim() == "0").unwrap();
    pairs.iter().enumerate().filter(|(i, _)| *i < at || *i >= end).map(|(_, (c, v))| format!("{c}\n{v}\n")).collect()
}

/// kicad-cli's DRC on `pcb`, as JSON.
fn drc(cli: &Path, dir: &Path, pcb: &Path) -> serde_json::Value {
    let report = dir.join("report.json");
    let out = Command::new(cli).args(["pcb", "drc", "--format", "json", "--severity-all", "--output"]).arg(&report).arg(pcb).output().expect("kicad-cli runs");
    assert!(report.exists(), "kicad-cli wrote no report -- it could not read the board: {}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap()
}

fn has_violation(report: &serde_json::Value, kind: &str) -> bool {
    report["violations"].as_array().unwrap().iter().any(|v| v["type"] == kind)
}

#[test]
fn kicad_cli_finds_a_dxf_imported_edge_cuts_outline_closed_and_exports_it_back_as_the_same_outline() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };

    // The board of lines and arcs, and the same one as a closed polyline with bulges: both are closed outlines to KiCad.
    for (name, file) in [("lines_arcs", "board_outline_lines_arcs.dxf"), ("lwpolyline", "board_outline_lwpolyline.dxf")] {
        let (design, model) = import(fixture(file), None);
        let dir = scratch(name);
        let pcb = write_board(&dir, &design, &model);
        let report = drc(&cli, &dir, &pcb);
        assert!(!has_violation(&report, "invalid_outline"), "{name}: kicad-cli finds the imported outline closed: {}", report["violations"]);

        // KiCad's own DXF export of Edge.Cuts: read back with the reader that read the first file.
        let out = dir.join("edge.dxf");
        let run = Command::new(&cli).args(["pcb", "export", "dxf", "--layers", "Edge.Cuts", "--output-units", "mm", "--mode-single", "--output"]).arg(&out).arg(&pcb).output().expect("kicad-cli runs");
        assert!(out.exists(), "kicad-cli wrote no DXF: {}\n{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
        let back = read_dxf(&std::fs::read_to_string(&out).unwrap(), &DxfOptions::default()).expect("kicad-cli's DXF reads");
        let count = |f: &dyn Fn(&ImportedShape) -> bool| back.shapes.iter().filter(|s| f(s)).count();
        assert_eq!(count(&|s| matches!(s, ImportedShape::Line { .. })), 4, "{name}: {:?}", back.shapes);
        assert_eq!(count(&|s| matches!(s, ImportedShape::Arc { .. })), 4, "{name}: the corners are still arcs: {:?}", back.shapes);
        let b = back.bbox.expect("a box");
        // The DXF export keeps the board's Y axis down as a DXF's up (it mirrors), so the box is 40 x 30 either way.
        assert!((b.width() - 40.0).abs() < 0.3 && (b.height() - 30.0).abs() < 0.3, "{name}: {b:?}");
        for s in &back.shapes {
            if let ImportedShape::Arc { center, start, .. } = s {
                assert!((((*start - *center).norm()) - 3.0).abs() < 0.01, "{name}: corner radius 3 mm: {s:?}");
            }
        }
    }

    // Negative control: the same outline with one side left out is not closed.
    let open: String = drop_first_record(&fixture("board_outline_lines_arcs.dxf"), "LINE");
    let (design, model) = import(open, None);
    let dir = scratch("open");
    let pcb = write_board(&dir, &design, &model);
    assert!(has_violation(&drc(&cli, &dir, &pcb), "invalid_outline"), "an outline with a side missing is not closed to kicad-cli");
}

#[test]
fn fix_discontinuities_closes_an_outline_whose_corners_miss() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    // A 10 x 10 mm square whose corners miss by 0.3 mm: open to kicad-cli (its chaining tolerance is 0.01 mm), closed after the fix.
    let dxf = "0\nSECTION\n2\nENTITIES\n".to_string()
        + &[("0", "0", "9.7", "0"), ("10", "0.3", "10", "9.7"), ("9.7", "10", "0.3", "10"), ("0", "9.7", "0", "0.3")]
            .iter()
            .map(|(a, b, c, d)| format!("0\nLINE\n10\n{a}\n20\n{b}\n11\n{c}\n21\n{d}\n"))
            .collect::<String>()
        + "0\nENDSEC\n0\nEOF\n";
    let (design, model) = import(dxf.clone(), None);
    let dir = scratch("gap");
    let pcb = write_board(&dir, &design, &model);
    assert!(has_violation(&drc(&cli, &dir, &pcb), "invalid_outline"), "the gaps are open to kicad-cli");
    let (design, model) = import(dxf, Some(1_000_000));
    let dir = scratch("gap_fixed");
    let pcb = write_board(&dir, &design, &model);
    let report = drc(&cli, &dir, &pcb);
    assert!(!has_violation(&report, "invalid_outline"), "the fix closes it: {}", report["violations"]);
}

#[test]
fn an_svg_outline_is_a_closed_outline_to_kicad_cli_too() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let model = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &model, 100_000, 300_000);
    let svg = std::fs::read_to_string(format!("{}/../kicad/tests/fixtures/board_outline_rect.svg", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let req = ImportGraphics { format: GraphicsFormat::Svg, text: svg, layer: "Edge.Cuts".into(), dxf_units: String::new(), scale: 1.0, offset_x_nm: 0, offset_y_nm: 0, line_width_nm: 100_000, group: false, fix_tolerance_nm: None, replace_outline: false };
    b.apply(&Cmd::ImportGraphics(req)).expect("the import applies");
    let dir = scratch("svg");
    let pcb = write_board(&dir, b.design(), &model);
    let report = drc(&cli, &dir, &pcb);
    assert!(!has_violation(&report, "invalid_outline"), "kicad-cli finds the imported polygon outline closed: {}", report["violations"]);
}
