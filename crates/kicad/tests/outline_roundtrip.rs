//! The board outline through the importer and the writer: Edge.Cuts arcs, circles, rectangles, curves and cutouts stay the shapes they
//! are (`eda_model::ir::DrawingsSection::outline_is_shapes`), come back out of `export_kicad_pcb` as the same `gr_` items, and a
//! malformed outline stays malformed. The first group needs no external tool; the last one (`EDA_SLOW_TESTS=1`) asks kicad-cli: the
//! Edge.Cuts it plots are the same before and after, and it still reports the outline it should.

use std::path::{Path, PathBuf};
use std::process::Command;

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::ir::{Design, Point, Shape};
use eda_model::ConstraintModel;

fn p(x: i64, y: i64) -> Point {
    Point { x, y }
}

const HEADER: &str = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
	(general (thickness 1.6) (legacy_teardrops no))
	(paper "A4")
	(layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
	(setup (pad_to_mask_clearance 0))
	(net 0 "")
"#;

fn board(items: &str) -> String {
    format!("{HEADER}{items})\n")
}

/// 40 x 30 mm, 5 mm rounded corners, a round hole, a square hole, and a D-shaped slot made of a curve and a line.
fn rounded_board_items() -> &'static str {
    r#"	(gr_line (start 5 0) (end 35 0) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-000000000001"))
	(gr_arc (start 35 0) (mid 38.535534 1.464466) (end 40 5) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-000000000002"))
	(gr_line (start 40 5) (end 40 25) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-000000000003"))
	(gr_arc (start 40 25) (mid 38.535534 28.535534) (end 35 30) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-000000000004"))
	(gr_line (start 35 30) (end 5 30) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-000000000005"))
	(gr_arc (start 5 30) (mid 1.464466 28.535534) (end 0 25) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-000000000006"))
	(gr_line (start 0 25) (end 0 5) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-000000000007"))
	(gr_arc (start 0 5) (mid 1.464466 1.464466) (end 5 0) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-000000000008"))
	(gr_circle (center 20 15) (end 23.2 15) (stroke (width 0.05) (type default)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-000000000009"))
	(gr_rect (start 8 8) (end 12 12) (stroke (width 0.05) (type default)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-00000000000a"))
	(gr_curve (pts (xy 28 20) (xy 28 26) (xy 34 26) (xy 34 20)) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-00000000000b"))
	(gr_line (start 34 20) (end 28 20) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-00000000000c"))
"#
}

fn edge_shapes(d: &Design) -> Vec<&Shape> {
    d.drawings.as_ref().map(|dr| dr.shapes.iter().filter(|s| s.layer() == "Edge.Cuts").collect()).unwrap_or_default()
}

fn kind(s: &Shape) -> &'static str {
    match s {
        Shape::Segment { .. } => "line",
        Shape::Arc { .. } => "arc",
        Shape::Rect { .. } => "rect",
        Shape::Circle { .. } => "circle",
        Shape::Polygon { .. } => "poly",
        Shape::Bezier { .. } => "curve",
    }
}

fn counts(d: &Design) -> std::collections::BTreeMap<&'static str, usize> {
    let mut m = std::collections::BTreeMap::new();
    for s in edge_shapes(d) {
        *m.entry(kind(s)).or_insert(0) += 1;
    }
    m
}

fn export(d: &Design, model: &ConstraintModel) -> String {
    export_kicad_pcb(d, model, &ExportMeta { date: "2026-01-01", title: "outline_rt" }).expect("export_kicad_pcb")
}

fn count_in(text: &str, token: &str) -> usize {
    text.matches(token).count()
}

#[test]
fn arcs_circles_rectangles_and_curves_are_kept_as_the_shapes_they_are() {
    let (design, model, notes) = import_kicad_pcb(&board(rounded_board_items())).expect("imports");
    assert_eq!(notes.outline_source, "shapes");
    assert!(!notes.outline_open && notes.outline_errors.is_empty(), "{:?}", notes.outline_errors);
    assert_eq!(notes.outline_shapes, 12);
    assert!(design.drawings.as_ref().expect("drawings").outline_is_shapes);
    assert_eq!(counts(&design), [("arc", 4), ("circle", 1), ("curve", 1), ("line", 5), ("rect", 1)].into_iter().collect());
    assert_eq!(model.board.outline_closed, Some(true));

    // The arc is the file's arc, three points and all: its middle point survives to the micrometre.
    let arc = edge_shapes(&design).into_iter().find(|s| matches!(s, Shape::Arc { start, .. } if *start == p(35_000, 0))).expect("the top-right arc");
    assert_eq!(arc.points(), vec![p(35_000, 0), p(38_536, 1_464), p(40_000, 5_000)]);

    // `placement.outline` is the summary of what they make: the round corners are many points, not the 8 of the chord outline.
    let outline = &design.placement.as_ref().expect("placement").outline;
    assert!(outline.len() > 60, "{} points", outline.len());
    assert!(outline.iter().all(|q| (0..=40_000).contains(&q.x) && (0..=30_000).contains(&q.y)));
    assert_eq!(model.board.outline.as_ref(), Some(outline));
}

#[test]
fn the_writer_emits_the_same_items_and_not_the_summary_polygon_as_well() {
    let (design, model, _) = import_kicad_pcb(&board(rounded_board_items())).expect("imports");
    let text = export(&design, &model);
    assert_eq!(count_in(&text, "(gr_arc "), 4, "{text}");
    assert_eq!(count_in(&text, "(gr_circle "), 1);
    assert_eq!(count_in(&text, "(gr_rect "), 1);
    assert_eq!(count_in(&text, "(gr_curve "), 1);
    assert_eq!(count_in(&text, "(gr_line "), 5, "the five lines of the file, not also the summary polygon's sides");
    assert_eq!(count_in(&text, "(layer \"Edge.Cuts\")"), 12);

    // And back in again they are the same shapes, with the same ids and the same geometry.
    let (again, _, notes) = import_kicad_pcb(&text).expect("re-imports");
    assert_eq!(notes.outline_source, "shapes");
    let (a, b) = (edge_shapes(&design), edge_shapes(&again));
    assert_eq!(a.len(), b.len());
    for want in a {
        let got = b.iter().find(|s| s.id() == want.id()).unwrap_or_else(|| panic!("{} did not round-trip", want.id()));
        assert_eq!((kind(want), want.points(), want.stroke_width()), (kind(got), got.points(), got.stroke_width()));
    }
    assert_eq!(design.placement.as_ref().expect("p").outline, again.placement.as_ref().expect("p").outline);
}

#[test]
fn a_closed_loop_of_lines_is_still_the_plain_polygon_and_is_written_as_lines() {
    let items = r#"	(gr_line (start 0 0) (end 20 0) (layer "Edge.Cuts"))
	(gr_line (start 20 0) (end 20 10) (layer "Edge.Cuts"))
	(gr_line (start 20 10) (end 0 10) (layer "Edge.Cuts"))
	(gr_line (start 0 10) (end 0 0) (layer "Edge.Cuts"))
"#;
    let (design, model, notes) = import_kicad_pcb(&board(items)).expect("imports");
    assert_eq!(notes.outline_source, "lines");
    assert!(!design.drawings.as_ref().is_some_and(|d| d.outline_is_shapes), "a plain loop needs no shapes");
    assert!(edge_shapes(&design).is_empty());
    assert_eq!(design.placement.as_ref().expect("placement").outline.len(), 4);
    let text = export(&design, &model);
    assert_eq!(count_in(&text, "(gr_line "), 4);
}

#[test]
fn a_gr_rect_outline_comes_back_as_a_gr_rect() {
    let items = r#"	(gr_rect (start 0 0) (end 30 20) (stroke (width 0.1) (type default)) (fill no) (layer "Edge.Cuts") (uuid "11111111-0000-0000-0000-000000000001"))
"#;
    let (design, model, notes) = import_kicad_pcb(&board(items)).expect("imports");
    assert_eq!(notes.outline_source, "shapes");
    assert_eq!(counts(&design), [("rect", 1)].into_iter().collect());
    assert_eq!(design.placement.as_ref().expect("placement").outline.len(), 4);
    let text = export(&design, &model);
    assert_eq!(count_in(&text, "(gr_rect "), 1);
    assert_eq!(count_in(&text, "(gr_line "), 0);
    assert!(text.contains("(stroke (width 0.1)"), "the line width is the file's");
}

#[test]
fn an_outline_that_does_not_close_stays_open_and_is_reported() {
    // A square with 2 mm missing from its bottom side.
    let items = r#"	(gr_line (start 0 0) (end 4 0) (layer "Edge.Cuts") (uuid "22222222-0000-0000-0000-000000000001"))
	(gr_line (start 6 0) (end 10 0) (layer "Edge.Cuts") (uuid "22222222-0000-0000-0000-000000000002"))
	(gr_line (start 10 0) (end 10 10) (layer "Edge.Cuts") (uuid "22222222-0000-0000-0000-000000000003"))
	(gr_line (start 10 10) (end 0 10) (layer "Edge.Cuts") (uuid "22222222-0000-0000-0000-000000000004"))
	(gr_line (start 0 10) (end 0 0) (layer "Edge.Cuts") (uuid "22222222-0000-0000-0000-000000000005"))
"#;
    let (design, model, notes) = import_kicad_pcb(&board(items)).expect("imports");
    assert_eq!(notes.outline_source, "shapes");
    assert!(notes.outline_open);
    assert_eq!(model.board.outline_closed, Some(false));
    assert_eq!(notes.outline_errors.len(), 1, "{:?}", notes.outline_errors);
    assert!(notes.outline_errors[0].contains("(not a closed shape)") && notes.outline_errors[0].contains("(5.000, 0.000) mm"), "{}", notes.outline_errors[0]);
    // The five lines are the five lines; nothing was added to close the gap.
    assert_eq!(counts(&design), [("line", 5)].into_iter().collect());
    let text = export(&design, &model);
    assert_eq!(count_in(&text, "(gr_line "), 5, "the exported file has the same gap");
    // The summary is KiCad's inferred rectangle round the edges.
    let outline = &design.placement.as_ref().expect("placement").outline;
    assert_eq!(outline.len(), 4);
    assert_eq!((outline.iter().map(|q| q.x).min(), outline.iter().map(|q| q.x).max()), (Some(0), Some(10_000)));
}

#[test]
fn a_footprints_edge_cuts_are_board_shapes_in_board_space() {
    // A connector with a 4 mm slot cut out of the board: a line pair and two arcs in the footprint's own frame, turned 90 degrees.
    let text = board(
        r#"	(gr_rect (start 0 0) (end 40 30) (stroke (width 0.1) (type default)) (fill no) (layer "Edge.Cuts") (uuid "33333333-0000-0000-0000-000000000001"))
	(footprint "J:slot" (layer "F.Cu") (uuid "33333333-0000-0000-0000-0000000000f1") (at 20 15 90)
		(property "Reference" "J1" (at 0 0 0) (layer "F.SilkS"))
		(fp_line (start -3 -1) (end 3 -1) (stroke (width 0.1) (type default)) (layer "Edge.Cuts") (uuid "33333333-0000-0000-0000-000000000002"))
		(fp_arc (start 3 -1) (mid 4 0) (end 3 1) (stroke (width 0.1) (type default)) (layer "Edge.Cuts") (uuid "33333333-0000-0000-0000-000000000003"))
		(fp_line (start 3 1) (end -3 1) (stroke (width 0.1) (type default)) (layer "Edge.Cuts") (uuid "33333333-0000-0000-0000-000000000004"))
		(fp_arc (start -3 1) (mid -4 0) (end -3 -1) (stroke (width 0.1) (type default)) (layer "Edge.Cuts") (uuid "33333333-0000-0000-0000-000000000005"))
		(pad "1" smd rect (at 0 3 0) (size 1 1) (layers "F.Cu")))
"#,
    );
    let (design, _model, notes) = import_kicad_pcb(&text).expect("imports");
    assert_eq!(notes.outline_source, "shapes");
    assert_eq!(counts(&design), [("arc", 2), ("line", 2), ("rect", 1)].into_iter().collect());
    assert!(notes.outline_errors.is_empty(), "{:?}", notes.outline_errors);
    // Turned 90 degrees about (20, 15), the slot lies along y: the line (-3,-1)-(3,-1) is at x = 20 + 1 (KiCad turns clockwise on screen).
    let line = edge_shapes(&design).into_iter().find(|s| matches!(s, Shape::Segment { .. })).expect("a line");
    let pts = line.points();
    assert!(pts.iter().all(|q| q.x == 19_000 || q.x == 21_000), "{pts:?}");
    assert!(pts.iter().all(|q| (11_999..=12_001).contains(&q.y) || (17_999..=18_001).contains(&q.y)), "{pts:?}");
}

#[test]
fn a_board_from_before_kicad_7_closes_its_outline_through_its_centre_start_angle_arcs() {
    // `(gr_arc (start CENTER) (end ARC_START) (angle A))` and `(width W)` on the item: the grammar of files up to KiCad 6 (issue8909 is one).
    // The corner arc goes from (20, 5) a quarter turn about (15, 5) to (15, 10).
    let items = r#"	(gr_line (start 0 0) (end 20 0) (layer "Edge.Cuts") (width 0.1) (tstamp 55555555-0000-0000-0000-000000000001))
	(gr_line (start 20 0) (end 20 5) (layer "Edge.Cuts") (width 0.1) (tstamp 55555555-0000-0000-0000-000000000002))
	(gr_arc (start 15 5) (end 20 5) (angle 90) (layer "Edge.Cuts") (width 0.1) (tstamp 55555555-0000-0000-0000-000000000003))
	(gr_line (start 15 10) (end 0 10) (layer "Edge.Cuts") (width 0.1) (tstamp 55555555-0000-0000-0000-000000000004))
	(gr_line (start 0 10) (end 0 0) (layer "Edge.Cuts") (width 0.1) (tstamp 55555555-0000-0000-0000-000000000005))
"#;
    let (design, model, notes) = import_kicad_pcb(&board(items)).expect("imports");
    assert_eq!(notes.outline_source, "shapes", "{notes:?}");
    assert!(!notes.outline_open && notes.outline_errors.is_empty(), "{:?}", notes.outline_errors);
    assert_eq!(model.board.outline_closed, Some(true));
    let arc = edge_shapes(&design).into_iter().find(|s| matches!(s, Shape::Arc { .. })).expect("the arc");
    assert_eq!(arc.points(), vec![p(20_000, 5_000), p(18_536, 8_536), p(15_000, 10_000)]);
    assert_eq!(arc.stroke_width(), 100, "the legacy (width ..) is the stroke width");
    // The exported file writes it in today's grammar and it reads back as the same arc.
    let text = export(&design, &model);
    assert!(text.contains("(gr_arc (start 20 5) (mid 18.536 8.536) (end 15 10)"), "{text}");
}

#[test]
fn a_tilted_footprint_rectangle_becomes_a_polygon() {
    let text = board(
        r#"	(gr_rect (start 0 0) (end 40 30) (stroke (width 0.1) (type default)) (fill no) (layer "Edge.Cuts") (uuid "44444444-0000-0000-0000-000000000001"))
	(footprint "J:square" (layer "F.Cu") (uuid "44444444-0000-0000-0000-0000000000f1") (at 20 15 45)
		(property "Reference" "J1" (at 0 0 0) (layer "F.SilkS"))
		(fp_rect (start -2 -2) (end 2 2) (stroke (width 0.1) (type default)) (fill no) (layer "Edge.Cuts") (uuid "44444444-0000-0000-0000-000000000002")))
"#,
    );
    let (design, _model, notes) = import_kicad_pcb(&text).expect("imports");
    assert_eq!(counts(&design), [("poly", 1), ("rect", 1)].into_iter().collect());
    assert!(notes.outline_errors.is_empty(), "{:?}", notes.outline_errors);
}

// ----------------------------------------------------------------------------------------------------------------------
// kicad-cli
// ----------------------------------------------------------------------------------------------------------------------

fn slow_tests_off() -> bool {
    let off = std::env::var_os("EDA_SLOW_TESTS").is_none();
    if off {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
    }
    off
}

fn find_kicad_cli() -> Option<PathBuf> {
    if let Ok(out) = Command::new("which").arg("kicad-cli").output() {
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    let mac = PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli");
    mac.exists().then_some(mac)
}

/// kicad-cli's plot of a board's Edge.Cuts, as the sorted list of what it drew: lines undirected, arcs by their ends and radius, circles,
/// the rectangle and curves, each rounded to the 0.002 mm that the micrometre grid of this workspace costs a curve.
fn plotted_edge_cuts(cli: &Path, board: &Path, dir: &Path, name: &str) -> Vec<String> {
    let svg = dir.join(format!("{name}.svg"));
    let out = Command::new(cli).args(["pcb", "export", "svg", "--layers", "Edge.Cuts", "--mode-single", "--exclude-drawing-sheet", "-o"]).arg(&svg).arg(board).output().expect("runs kicad-cli");
    assert!(out.status.success(), "kicad-cli: {}", String::from_utf8_lossy(&out.stderr));
    let text = std::fs::read_to_string(&svg).expect("an svg");
    let r = |v: f64| format!("{:.2}", (v * 500.0).round() / 500.0);
    let nums = |s: &str| -> Vec<f64> { s.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter(|t| !t.is_empty()).filter_map(|t| t.parse().ok()).collect() };
    let mut items = Vec::new();
    // `<path d="..."/>` elements, and `<circle .../>`.
    let mut rest = text.as_str();
    while let Some(i) = rest.find("<path") {
        rest = &rest[i..];
        let end = rest.find("/>").expect("a closed element");
        let el = &rest[..end];
        let d = el.split("d=\"").last().and_then(|t| t.split('"').next()).unwrap_or("");
        let n = nums(d);
        let item = if d.contains(" A") {
            // M x0 y0 A r r 0 0 sweep x1 y1: ends sorted so direction does not matter.
            let (a, b) = ((n[0], n[1]), (n[7], n[8]));
            let (lo, hi) = if (r(a.0), r(a.1)) <= (r(b.0), r(b.1)) { (a, b) } else { (b, a) };
            format!("arc {} {} {} {} r{}", r(lo.0), r(lo.1), r(hi.0), r(hi.1), r(n[2]))
        } else if d.contains('C') {
            format!("curve {}", n.iter().map(|v| r(*v)).collect::<Vec<_>>().join(" "))
        } else if d.contains('Z') {
            format!("poly {}", n.iter().map(|v| r(*v)).collect::<Vec<_>>().join(" "))
        } else {
            let (a, b) = ((n[0], n[1]), (n[2], n[3]));
            let (lo, hi) = if (r(a.0), r(a.1)) <= (r(b.0), r(b.1)) { (a, b) } else { (b, a) };
            format!("line {} {} {} {}", r(lo.0), r(lo.1), r(hi.0), r(hi.1))
        };
        items.push(item);
        rest = &rest[end..];
    }
    let mut rest = text.as_str();
    while let Some(i) = rest.find("<circle") {
        rest = &rest[i..];
        let end = rest.find("/>").expect("a closed element");
        let n = nums(&rest[..end].replace("cx=", " ").replace("cy=", " ").replace("r=", " "));
        items.push(format!("circle {} {} r{}", r(n[0]), r(n[1]), r(n[2])));
        rest = &rest[end..];
    }
    items.sort();
    items
}

fn drc_types(cli: &Path, board: &Path, dir: &Path, name: &str) -> Vec<String> {
    let json = dir.join(format!("{name}.drc.json"));
    let out = Command::new(cli).args(["pcb", "drc", "--format", "json", "--severity-all", "-o"]).arg(&json).arg(board).output().expect("runs kicad-cli");
    assert!(out.status.success(), "kicad-cli drc: {}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&json).expect("a report")).expect("json");
    v["violations"].as_array().map(|a| a.iter().filter_map(|x| x["type"].as_str().map(str::to_string)).collect()).unwrap_or_default()
}

#[test]
fn kicad_cli_plots_the_same_edge_cuts_before_and_after_the_round_trip() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let dir = std::env::temp_dir().join(format!("eda_outline_rt_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch dir");

    // The rounded board, and the same with a footprint slot and a gr_rect outline on a second board.
    let original = board(rounded_board_items());
    let original_path = dir.join("original.kicad_pcb");
    std::fs::write(&original_path, &original).expect("write");
    let (design, model, notes) = import_kicad_pcb(&original).expect("imports");
    assert!(notes.outline_errors.is_empty());
    let exported_path = dir.join("exported.kicad_pcb");
    std::fs::write(&exported_path, export(&design, &model)).expect("write");

    let before = plotted_edge_cuts(&cli, &original_path, &dir, "original");
    let after = plotted_edge_cuts(&cli, &exported_path, &dir, "exported");
    assert_eq!(before.len(), 12, "{before:?}");
    assert_eq!(before, after, "kicad-cli draws other Edge.Cuts after the round trip");

    // kicad-cli finds a well-formed outline in both.
    assert!(!drc_types(&cli, &original_path, &dir, "original").contains(&"invalid_outline".to_string()));
    assert!(!drc_types(&cli, &exported_path, &dir, "exported").contains(&"invalid_outline".to_string()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn kicad_cli_still_reports_the_open_outline_after_the_round_trip() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let dir = std::env::temp_dir().join(format!("eda_outline_open_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch dir");
    let original = board(
        r#"	(gr_line (start 0 0) (end 4 0) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "22222222-0000-0000-0000-000000000001"))
	(gr_line (start 6 0) (end 10 0) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "22222222-0000-0000-0000-000000000002"))
	(gr_line (start 10 0) (end 10 10) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "22222222-0000-0000-0000-000000000003"))
	(gr_line (start 10 10) (end 0 10) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "22222222-0000-0000-0000-000000000004"))
	(gr_line (start 0 10) (end 0 0) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "22222222-0000-0000-0000-000000000005"))
"#,
    );
    let original_path = dir.join("open.kicad_pcb");
    std::fs::write(&original_path, &original).expect("write");
    let (design, model, _) = import_kicad_pcb(&original).expect("imports");
    let exported_path = dir.join("open_exported.kicad_pcb");
    std::fs::write(&exported_path, export(&design, &model)).expect("write");
    let before = drc_types(&cli, &original_path, &dir, "open");
    let after = drc_types(&cli, &exported_path, &dir, "open_exported");
    assert!(before.contains(&"invalid_outline".to_string()), "kicad-cli reports the gap in the original: {before:?}");
    assert!(after.contains(&"invalid_outline".to_string()), "and after the round trip, not a loop closed behind the user's back: {after:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
