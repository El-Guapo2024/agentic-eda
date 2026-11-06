//! A board shape's line style (UI-LEFT P5; GAPS.md item 2): `(stroke (width ..) (type dash|dot|dash_dot|dash_dot_dot|solid|default))` of a `gr_line`, `gr_arc`, `gr_rect`,
//! `gr_circle`, `gr_poly` and `gr_curve` is read into the board's `shape_styles` (`STROKE_PARAMS_PARSER::ParseStroke`; `parsePCB_SHAPE` starts from
//! `STROKE_PARAMS( 0, LINE_STYLE::SOLID )`, so a shape that names none is solid and has no entry) and written back where `STROKE_PARAMS::Format` puts it. The QA boards
//! with dashes (`issue12831`, `api_kitchen_sink`, KiCad 25ec2335's `issue22267` and `connect`) come back with every one of their line types. The slow test
//! (`EDA_SLOW_TESTS=1`) has kicad-cli plot a dashed F.SilkS line to a Gerber: `BRDITEMS_PLOTTER::PlotShape` strokes it with `STROKE_PARAMS::Stroke`, so the
//! Gerber has a draw for each dash where the solid line has one.

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::fp_art::LineStyle;
use eda_model::ir::{Design, Point, Shape};
use eda_model::units::{mm, Nm};
use std::path::PathBuf;

const HEADER: &str = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
	(general (thickness 1.6) (legacy_teardrops no))
	(paper "A4")
	(layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user) (5 "F.SilkS" user "F.Silkscreen") (35 "F.Fab" user) (39 "User.2" user))
	(setup (pad_to_mask_clearance 0))
	(net 0 "")
"#;

fn board(items: &str) -> String {
    format!("{HEADER}{items})\n")
}

/// One shape of each kind, each with a line type of its own, and one with the old `(width ..)` and no stroke at all.
const ITEMS: &str = r#"	(gr_line (start 0 0) (end 20 0) (stroke (width 0.15) (type dash)) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-000000000001"))
	(gr_arc (start 0 10) (mid 5 15) (end 10 10) (stroke (width 0.1) (type dot)) (layer "F.Fab") (uuid "00000000-0000-0000-0000-000000000002"))
	(gr_rect (start 30 0) (end 40 10) (stroke (width 0.1) (type dash_dot)) (fill no) (layer "F.Fab") (uuid "00000000-0000-0000-0000-000000000003"))
	(gr_circle (center 55 5) (end 60 5) (stroke (width 0.1) (type dash_dot_dot)) (fill no) (layer "User.2") (uuid "00000000-0000-0000-0000-000000000004"))
	(gr_poly (pts (xy 70 0) (xy 80 0) (xy 75 10)) (stroke (width 0.1) (type default)) (fill no) (layer "F.Fab") (uuid "00000000-0000-0000-0000-000000000005"))
	(gr_curve (pts (xy 0 30) (xy 5 25) (xy 10 35) (xy 15 30)) (stroke (width 0.1) (type solid)) (layer "F.Fab") (uuid "00000000-0000-0000-0000-000000000006"))
	(gr_line (start 0 40) (end 10 40) (width 0.1) (layer "F.Fab") (uuid "00000000-0000-0000-0000-000000000007"))
"#;

fn export(d: &Design, model: &eda_model::ConstraintModel) -> String {
    export_kicad_pcb(d, model, &ExportMeta { date: "2026-01-01", title: "line_styles" }).expect("export_kicad_pcb")
}

fn style_at(d: &Design, x: Nm, y: Nm) -> LineStyle {
    let dr = d.drawings.as_ref().unwrap();
    let shape = dr.shapes.iter().find(|s| s.points().first() == Some(&Point { x, y })).unwrap_or_else(|| panic!("no shape at {x:?},{y:?}"));
    dr.board_items.line_style_of(shape.id())
}

#[test]
fn every_line_type_of_a_board_shape_is_read_and_a_solid_one_says_nothing() {
    let (design, _, _) = import_kicad_pcb(&board(ITEMS)).expect("imports");
    assert_eq!(design.drawings.as_ref().unwrap().shapes.len(), 7);
    assert_eq!(style_at(&design, Nm::ZERO, Nm::ZERO), LineStyle::Dash);
    assert_eq!(style_at(&design, Nm::ZERO, mm(10.0)), LineStyle::Dot);
    assert_eq!(style_at(&design, mm(30.0), Nm::ZERO), LineStyle::DashDot);
    assert_eq!(style_at(&design, mm(55.0), mm(5.0)), LineStyle::DashDotDot);
    // The file's `default` is kept as it is (`LINE_STYLE::DEFAULT`, drawn solid), so the file KiCad reads back says the same.
    assert_eq!(style_at(&design, mm(70.0), Nm::ZERO), LineStyle::Unset);
    assert_eq!(style_at(&design, Nm::ZERO, mm(30.0)), LineStyle::Solid);
    assert_eq!(style_at(&design, Nm::ZERO, mm(40.0)), LineStyle::Solid, "a legacy (width ..) with no stroke is solid");
    // A solid shape keeps no style entry: five of the seven have one.
    assert_eq!(design.drawings.as_ref().unwrap().board_items.shape_styles.len(), 5);
}

#[test]
fn the_writer_puts_each_line_type_in_the_stroke_and_it_reads_back_the_same() {
    let (design, model, _) = import_kicad_pcb(&board(ITEMS)).expect("imports");
    let text = export(&design, &model);
    for (token, n) in [("dash", 1), ("dot", 1), ("dash_dot", 1), ("dash_dot_dot", 1), ("default", 1), ("solid", 2)] {
        let tag = format!("(type {token}))");
        let shapes = text.lines().filter(|l| l.trim_start().starts_with("(gr_") && l.contains(&tag)).count();
        assert_eq!(shapes, n, "{tag}: {text}");
    }
    assert!(text.contains("(gr_line (start 0 0) (end 20 0) (stroke (width 0.15) (type dash)) (layer \"F.SilkS\")"), "{text}");
    let (again, model2, _) = import_kicad_pcb(&text).expect("re-imports");
    let styles = |d: &Design| {
        let dr = d.drawings.as_ref().unwrap();
        let mut v: Vec<(Vec<Point>, LineStyle)> = dr.shapes.iter().map(|s| (s.points(), dr.board_items.line_style_of(s.id()))).collect();
        v.sort_by(|a, b| format!("{:?}", a.0).cmp(&format!("{:?}", b.0)));
        v
    };
    assert_eq!(styles(&again), styles(&design));
    assert_eq!(export(&again, &model2), text, "a second round is the same text");
}

#[test]
fn a_line_style_set_on_the_board_is_written_and_follows_the_shapes_id() {
    let (mut design, model, _) = import_kicad_pcb(&board(ITEMS)).expect("imports");
    let dr = design.drawings.as_mut().unwrap();
    let curve = dr.shapes.iter().find(|s| matches!(s, Shape::Bezier { .. })).unwrap().id().to_string();
    dr.board_items.set_line_style(&curve, LineStyle::DashDot);
    let dashed = dr.shapes.iter().find(|s| s.points().first() == Some(&Point { x: Nm::ZERO, y: Nm::ZERO })).unwrap().id().to_string();
    dr.board_items.set_line_style(&dashed, LineStyle::Solid);
    assert_eq!(dr.board_items.line_style_of(&dashed), LineStyle::Solid);
    assert!(dr.board_items.style_of(&dashed).is_none(), "a solid line keeps no entry");
    let text = export(&design, &model);
    assert!(text.contains("(gr_curve (pts (xy 0 30) (xy 5 25) (xy 10 35) (xy 15 30)) (stroke (width 0.1) (type dash_dot))"), "{text}");
    assert!(text.contains("(gr_line (start 0 0) (end 20 0) (stroke (width 0.15) (type solid))"), "{text}");
}

/// The QA boards of KiCad's corpus that have dashed board shapes: `$KICAD_QA_DATA` (or `$KICAD_SRC_DIR/qa/data`) for 8303b2ad's, `$KICAD10_SRC_DIR` (or
/// `~/src/kicad-src-25ec2335`) for the KiCad 10 ones.
fn qa_board(rel: &str, kicad10: bool) -> Option<String> {
    let root = if kicad10 {
        std::env::var_os("KICAD10_SRC_DIR").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("src/kicad-src-25ec2335"))).map(|r| r.join("qa/data"))
    } else {
        std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).or_else(|| std::env::var_os("KICAD_SRC_DIR").map(|d| PathBuf::from(d).join("qa/data")))
    };
    let text = root.and_then(|d| std::fs::read_to_string(d.join("pcbnew").join(rel)).ok());
    if text.is_none() {
        eprintln!("skipped: {rel} needs the KiCad QA boards (KICAD_QA_DATA, KICAD_SRC_DIR or KICAD10_SRC_DIR)");
    }
    text
}

/// The line type of every top-level board shape of a file, in the file's order of each tag: `(gr_line .. (stroke (width ..) (type T)) ..)`, the stroke on one line or
/// several. Footprint graphics, text boxes and tables are not counted (their own readers keep theirs).
fn board_shape_types(text: &str) -> std::collections::BTreeMap<String, usize> {
    let tree = eda_kicad_test_sexpr(text);
    let mut out = std::collections::BTreeMap::new();
    for item in tree {
        if let Some((tag, ty)) = item {
            *out.entry(format!("{tag} {ty}")).or_insert(0) += 1;
        }
    }
    out
}

/// A crude reader of the top-level `gr_*` shapes of a board file and the `(type ..)` inside their `(stroke ..)`: enough for counting, without the crate's private
/// s-expression parser.
fn eda_kicad_test_sexpr(text: &str) -> Vec<Option<(String, String)>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut i = 0;
    let mut in_str = false;
    let mut item: Option<(String, usize)> = None;
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            if c == b'\\' {
                i += 1;
            } else if c == b'"' {
                in_str = false;
            }
        } else if c == b'"' {
            in_str = true;
        } else if c == b'(' {
            depth += 1;
            if depth == 2 {
                let rest = &text[i + 1..];
                let tag: String = rest.chars().take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_').collect();
                if ["gr_line", "gr_arc", "gr_rect", "gr_circle", "gr_poly", "gr_curve"].contains(&tag.as_str()) {
                    item = Some((tag, i));
                }
            }
        } else if c == b')' {
            if depth == 2 {
                if let Some((tag, start)) = item.take() {
                    let body = &text[start..=i];
                    let ty = body.find("(stroke").and_then(|s| body[s..].find("(type").map(|t| s + t + 5)).map(|t| body[t..].trim_start().chars().take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_').collect::<String>());
                    out.push(Some((tag, ty.unwrap_or_else(|| "none".into()))));
                }
            }
            depth -= 1;
        }
        i += 1;
    }
    out
}

/// Each QA board's shapes come back with the line types the file gave them: every dash, dot and dash-dot-dot, and every `default` and `solid` (a shape with no stroke
/// at all comes back `solid`, which is what `parsePCB_SHAPE` makes of it). An outline loop KiCad stores as a `gr_poly` or lines may come back as other kinds, so the
/// types are compared over all shapes, not kind by kind.
#[test]
fn the_qa_boards_with_dashed_shapes_keep_every_line_type() {
    let mut seen = 0;
    for (rel, k10) in [("issue12831.kicad_pcb", false), ("api_kitchen_sink.kicad_pcb", false), ("issue22267.kicad_pcb", true), ("connect/connect.kicad_pcb", true)] {
        let Some(text) = qa_board(rel, k10) else { continue };
        seen += 1;
        let (design, model, _) = import_kicad_pcb(&text).unwrap_or_else(|e| panic!("{rel}: {e:?}"));
        let ours = export(&design, &model);
        let by_type = |m: std::collections::BTreeMap<String, usize>| {
            let mut t: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
            for (k, n) in m {
                let ty = k.split(' ').nth(1).unwrap().to_string();
                *t.entry(if ty == "none" { "solid".into() } else { ty }).or_insert(0) += n;
            }
            t
        };
        let (theirs, mine) = (by_type(board_shape_types(&text)), by_type(board_shape_types(&ours)));
        assert!(theirs.keys().any(|k| k != "solid" && k != "default"), "{rel} has a dashed shape: {theirs:?}");
        assert_eq!(mine, theirs, "{rel}: the line types of the board's shapes");
    }
    if seen == 0 {
        eprintln!("skipped: no QA board found");
    }
}

fn slow_tests_off() -> bool {
    let off = std::env::var_os("EDA_SLOW_TESTS").is_none();
    if off {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
    }
    off
}

fn kicad_cli() -> PathBuf {
    std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("kicad-cli"))
}

/// The draws (`D01`) of a Gerber file: a plotted segment is a move (`D02`) and a draw.
fn gerber_draws(text: &str) -> usize {
    text.lines().filter(|l| l.ends_with("D01*")).count()
}

/// kicad-cli plots our dashed F.SilkS line as dashes. `BRDITEMS_PLOTTER::PlotShape`: a line style that is not solid goes through `STROKE_PARAMS::Stroke` with the
/// plotter's render settings (dash `(12 - 1) w`, gap `(3 + 1) w`, `RENDER_SETTINGS::GetDashLength` / `GetGapLength`), each dash a `ThickSegment`. A 20 mm line 0.15 mm
/// wide is a 1.65 mm dash every 2.25 mm: nine dashes (the ninth from 18 to 19.65 mm), where the solid line beside it is one draw. A dotted rectangle 0.1 mm wide (a dot
/// `max( 1 - 1, 0.2 ) w` = 0.02 mm long every 0.42 mm, `MakeEffectiveShapes` stroking each side by itself from its own start) is 24 dots a side.
#[test]
fn kicad_cli_plots_our_dashed_silkscreen_line_as_dashes() {
    if slow_tests_off() {
        return;
    }
    let items = r#"	(gr_rect (start -5 -5) (end 45 25) (stroke (width 0.1) (type default)) (fill no) (layer "Edge.Cuts") (uuid "10000000-0000-0000-0000-000000000001"))
	(gr_line (start 0 0) (end 20 0) (stroke (width 0.15) (type solid)) (layer "F.SilkS") (uuid "10000000-0000-0000-0000-000000000002"))
	(gr_line (start 0 10) (end 20 10) (stroke (width 0.15) (type solid)) (layer "F.SilkS") (uuid "10000000-0000-0000-0000-000000000003"))
	(gr_rect (start 30 0) (end 40 10) (stroke (width 0.1) (type solid)) (fill no) (layer "F.SilkS") (uuid "10000000-0000-0000-0000-000000000004"))
"#;
    let (mut design, model, _) = import_kicad_pcb(&board(items)).expect("imports");
    // The second line and the rectangle are styled as the shape dialog styles them (`set_line_style`, what `SetShapeStyle` does): the board file carries it.
    let dr = design.drawings.as_mut().unwrap();
    let id_at = |dr: &eda_model::ir::DrawingsSection, x: f64, y: f64| dr.shapes.iter().find(|s| s.layer() == "F.SilkS" && s.points().first() == Some(&Point { x: mm(x), y: mm(y) })).unwrap().id().to_string();
    let (line, rect) = (id_at(dr, 0.0, 10.0), id_at(dr, 30.0, 0.0));
    dr.board_items.set_line_style(&line, LineStyle::Dash);
    dr.board_items.set_line_style(&rect, LineStyle::Dot);
    let text = export(&design, &model);
    assert!(text.contains("(gr_line (start 0 10) (end 20 10) (stroke (width 0.15) (type dash))"), "{text}");

    let root = std::env::temp_dir().join(format!("eda_line_styles_gerber_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let pcb = root.join("b.kicad_pcb");
    std::fs::write(&pcb, &text).unwrap();
    let plot = |board: &std::path::Path, out: &std::path::Path| -> String {
        std::fs::create_dir_all(out).unwrap();
        let o = std::process::Command::new(kicad_cli()).args(["pcb", "export", "gerbers", "--layers", "F.SilkS", "--no-protel-ext", "-o"]).arg(out).arg(board).output().expect("kicad-cli runs");
        assert!(o.status.success(), "kicad-cli pcb export gerbers: {}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        let gbr = std::fs::read_dir(out).unwrap().flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "gbr")).expect("a Gerber file");
        std::fs::read_to_string(gbr).unwrap()
    };
    let gerber = plot(&pcb, &root.join("ours"));
    // The same board with every shape solid: the two lines and the rectangle's four sides.
    let solid_text = export(&import_kicad_pcb(&board(items)).unwrap().0, &model);
    let solid_pcb = root.join("solid.kicad_pcb");
    std::fs::write(&solid_pcb, &solid_text).unwrap();
    let solid = plot(&solid_pcb, &root.join("solid"));
    assert_eq!(gerber_draws(&solid), 1 + 1 + 4, "{solid}");
    // One solid line, nine dashes, 96 dots.
    assert_eq!(gerber_draws(&gerber), 1 + 9 + 4 * 24, "{gerber}");
    let _ = std::fs::remove_dir_all(&root);
}
