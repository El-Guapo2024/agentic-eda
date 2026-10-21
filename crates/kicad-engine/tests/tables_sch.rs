//! A sheet's tables as kicad-cli judges them (GAPS.md item 24, package P16t): a table we write into a `.kicad_sch` is plotted by kicad-cli (`sch export svg`) the way we laid
//! it out -- every line of every cell's text where `SCH_TEXTBOX::GetDrawPos` and `EDA_TEXT::GetLinePositions` put it, the border and the separators where
//! `SCH_TABLE::DrawBorders` draws them -- and the table of KiCad's own QA sheet comes back from our reader and writer plotted as it was. Slow tier (`EDA_SLOW_TESTS=1`);
//! skipped, loudly, without kicad-cli, and the QA sheet is skipped without `KICAD_QA_DATA` (default `~/ws/kicad-src-8303b2ad/qa/data`).

use eda_drc::stroke_font::{kicad_text_segments, TextLayout};
use eda_kicad::{export_kicad_sch, export_kicad_sch_tree, import_kicad_sch_tree, ExportMeta};
use eda_model::ir::{Design, Point, Provenance, SchematicSection, TextJustify, TextVAlign};
use eda_model::table::Table;
use eda_model::table_text::{sch_cell_draw, SCH_DEFAULT_PEN_NM};
use eda_model::ConstraintModel;
use std::path::{Path, PathBuf};

const META: ExportMeta<'static> = ExportMeta { date: "2026-10-21", title: "tables" };

fn slow() -> Option<PathBuf> {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return None;
    }
    let cli = eda_kicad_engine::find_cli();
    if cli.is_none() {
        eprintln!("kicad-cli not found; skipping");
    }
    cli
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("eda_sch_tables_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The SVG kicad-cli plots of the root sheet of `file`.
fn plot(cli: &Path, file: &Path, out: &Path) -> String {
    let mut cmd = eda_kicad_engine::cli_command(cli, None);
    cmd.args(["sch", "export", "svg", "--exclude-drawing-sheet", "--output"]).arg(out).arg(file);
    let result = cmd.output().expect("kicad-cli runs");
    assert!(result.status.success(), "{}{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
    let stem = file.file_stem().unwrap().to_string_lossy().to_string();
    std::fs::read_to_string(out.join(format!("{stem}.svg"))).unwrap_or_else(|e| panic!("no plot of {stem}: {e}"))
}

/// The numbers of an SVG path's `d` (`M x y` and `L x y`).
fn numbers(d: &str) -> Vec<f64> {
    d.split(|c: char| c == 'M' || c == 'L' || c.is_whitespace()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect()
}

/// Every text the plot draws as stroked glyphs, as (what it says, the box round its strokes in micrometres).
fn plotted_text(svg: &str) -> Vec<(String, [f64; 4])> {
    let mut out = Vec::new();
    let mut rest = svg;
    const OPEN: &str = "<g class=\"stroked-text\"><desc>";
    while let Some(i) = rest.find(OPEN) {
        let after = &rest[i + OPEN.len()..];
        let (Some(desc_end), Some(group_end)) = (after.find("</desc>"), after.find("</g>")) else { break };
        let text = after[..desc_end].to_string();
        let body = &after[desc_end..group_end];
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        let mut p = body;
        while let Some(j) = p.find("<path d=\"") {
            let q = &p[j + 9..];
            let end = q.find('"').unwrap_or(q.len());
            let n = numbers(&q[..end]);
            for pair in n.chunks(2).filter(|c| c.len() == 2) {
                x0 = x0.min(pair[0]);
                x1 = x1.max(pair[0]);
                y0 = y0.min(pair[1]);
                y1 = y1.max(pair[1]);
            }
            p = &q[end..];
        }
        out.push((text, [x0 * 1000.0, y0 * 1000.0, x1 * 1000.0, y1 * 1000.0]));
        rest = &after[group_end..];
    }
    out
}

/// The straight lines of the plot that are not glyph strokes, in micrometres, ends in reading order.
fn plotted_lines(svg: &str) -> Vec<[i64; 4]> {
    let mut bare = String::new();
    let mut rest = svg;
    while let Some(i) = rest.find("<g class=\"stroked-text\">") {
        bare.push_str(&rest[..i]);
        let after = &rest[i..];
        let end = after.find("</g>").map(|e| e + 4).unwrap_or(after.len());
        rest = &after[end..];
    }
    bare.push_str(rest);
    let mut out = Vec::new();
    let mut rest = bare.as_str();
    while let Some(i) = rest.find("<path d=\"M") {
        let after = &rest[i + 10..];
        let end = after.find('"').unwrap_or(after.len());
        let d = &after[..end];
        rest = &after[end..];
        if d.contains(',') || d.contains('Z') {
            continue;
        }
        if let [x0, y0, x1, y1] = numbers(&format!("M{d}"))[..] {
            let um = |v: f64| (v * 1000.0).round() as i64;
            let (a, b) = ((um(x0), um(y0)), (um(x1), um(y1)));
            let (a, b) = if a <= b { (a, b) } else { (b, a) };
            out.push([a.0, a.1, b.0, b.1]);
        }
    }
    out
}

/// The box round the ink of one line of a cell's text, as the stroke font draws it with its first glyph's origin at `origin` (µm).
fn expected_ink(text: &str, origin: (f64, f64), size: i64, pen_um: f64, angle_deg: f64) -> Option<[f64; 4]> {
    let (offx, offy) = (pen_um / 1.52, size as f64 - pen_um * 0.052);
    let (s, c) = angle_deg.to_radians().sin_cos();
    let (rx, ry) = (offx * c + offy * s, offy * c - offx * s);
    let pos = Point { x: (origin.0 - rx).round() as i64, y: (origin.1 - ry).round() as i64 };
    let segs = kicad_text_segments(&TextLayout { text, pos, size: (size, size), thickness: pen_um.round() as i64, angle_deg, mirror: false, halign: -1, valign: -1 });
    if segs.is_empty() {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for s in &segs {
        for p in [s.a, s.b] {
            x0 = x0.min(p.x as f64);
            x1 = x1.max(p.x as f64);
            y0 = y0.min(p.y as f64);
            y1 = y1.max(p.y as f64);
        }
    }
    Some([x0, y0, x1, y1])
}

/// Every line of every cell's text of `t` that has ink: what it says and the box of its ink.
fn expected_text(t: &Table) -> Vec<(String, [f64; 4])> {
    let mut out = Vec::new();
    for row in 0..t.row_count() {
        for col in 0..t.col_count() {
            let draw = sch_cell_draw(t, row, col, &|_: &str| None);
            let cell = t.cell(row, col).unwrap();
            for line in &draw.lines {
                if let Some(b) = expected_ink(&line.text, (line.x_um, line.y_um), cell.style.height_um, draw.pen_um, draw.angle as f64 / 1000.0) {
                    out.push((line.text.clone(), b));
                }
            }
        }
    }
    out
}

/// The plotted text that is the one `want` names: the same words, the nearest box.
fn nearest<'a>(plotted: &'a [(String, [f64; 4])], want: &(String, [f64; 4])) -> Option<&'a (String, [f64; 4])> {
    plotted.iter().filter(|(t, _)| t == &want.0).min_by(|a, b| {
        let d = |p: &(String, [f64; 4])| (p.1[0] - want.1[0]).abs() + (p.1[1] - want.1[1]).abs();
        d(a).partial_cmp(&d(b)).unwrap()
    })
}

fn assert_text_placed_as_kicad_cli_plots_it(t: &Table, svg: &str, what: &str) {
    let plotted = plotted_text(svg);
    let wanted = expected_text(t);
    assert!(!wanted.is_empty());
    let mut worst = 0.0f64;
    for w in &wanted {
        let Some(got) = nearest(&plotted, w) else { panic!("{what}: kicad-cli plots no {:?}; it plotted {:?}", w.0, plotted.iter().map(|p| &p.0).collect::<Vec<_>>()) };
        for k in 0..4 {
            assert!((got.1[k] - w.1[k]).abs() <= 8.0, "{what}: {:?} is inked at {:?} in kicad-cli's plot and at {:?} here ({k})", w.0, got.1, w.1);
        }
        worst = worst.max((0..4).map(|k| (got.1[k] - w.1[k]).abs()).fold(0.0, f64::max));
    }
    eprintln!("{what}: {} lines of text inked within {worst:.1} um of kicad-cli's plot", wanted.len());
}

fn assert_borders_drawn_as_kicad_cli_plots_them(t: &Table, svg: &str, what: &str) {
    let lines = plotted_lines(svg);
    let expected: Vec<[i64; 4]> = t
        .borders()
        .iter()
        .map(|l| {
            let (a, b) = ((l.a.x, l.a.y), (l.b.x, l.b.y));
            let (a, b) = if a <= b { (a, b) } else { (b, a) };
            [a.0, a.1, b.0, b.1]
        })
        .collect();
    assert!(!expected.is_empty());
    for line in &expected {
        assert!(lines.iter().any(|l| l.iter().zip(line).all(|(a, b)| (a - b).abs() <= 3)), "{what}: kicad-cli draws no line {line:?}; it drew {lines:?}");
    }
}

fn design_with(sch: SchematicSection) -> Design {
    Design { schema: 1, provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] }, schematic: Some(sch), nets: None, placement: None, routing: None, drawings: None, footprint_library: None, sheet_contents: None, bus_aliases: Default::default(), symbol_library: None }
}

fn sheet_of(tables: Vec<Table>) -> Design {
    let mut sch = SchematicSection::default();
    sch.extras.tables = tables;
    sch.assign_missing_ids();
    design_with(sch)
}

/// A 3 x 2 table: left and top, centred, right and bottom text; one cell of several lines; a bold one; a variable.
fn lying_table() -> Table {
    let mut t = Table::new_sch_grid(Point { x: 30_000, y: 40_000 }, 2, 3, 30_000, 8_000, 1_270);
    let texts = ["Header", "Value", "centred", "r ight\nbottom", "a long text that must be broken to the width of its column", "${ROW}:${COL} ${ADDR}"];
    for (cell, text) in t.cells.iter_mut().zip(texts) {
        cell.text = text.to_string();
    }
    t.cells[2].style.h_align = TextJustify::Center;
    t.cells[2].style.v_align = TextVAlign::Center;
    t.cells[3].style.h_align = TextJustify::Right;
    t.cells[3].style.v_align = TextVAlign::Bottom;
    t.cells[1].style.bold = true;
    t.cells[1].style.height_um = 1_800;
    t.cells[1].style.width_um = 1_800;
    t.cells[4].style.thickness_um = 300;
    t
}

#[test]
fn kicad_cli_plots_a_table_we_wrote_on_a_sheet_the_way_it_was_laid_out() {
    let Some(cli) = slow() else { return };
    let dir = scratch("lying");
    let standing = {
        // The same grid turned a quarter: the text stands, anchored along the height of the cell.
        let mut t = lying_table();
        t.at = Point { x: 150_000, y: 120_000 };
        t.angle = 90_000;
        t
    };
    let tables = vec![lying_table(), standing];
    let design = sheet_of(tables);
    let file = export_kicad_sch(&design, &ConstraintModel::default(), &META).unwrap();
    std::fs::write(dir.join("sheet.kicad_sch"), &file).unwrap();
    let svg = plot(&cli, &dir.join("sheet.kicad_sch"), &dir.join("out"));
    for (i, t) in design.schematic.as_ref().unwrap().extras.tables.iter().enumerate() {
        let what = if i == 0 { "the table lying" } else { "the table standing" };
        assert_text_placed_as_kicad_cli_plots_it(t, &svg, what);
        assert_borders_drawn_as_kicad_cli_plots_them(t, &svg, what);
    }
}

fn qa_sheet() -> Option<PathBuf> {
    let root = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data"));
    let file = root.join("eeschema/issue23840/BusAndVectors.kicad_sch");
    if file.exists() {
        Some(file)
    } else {
        eprintln!("KiCad's QA sheet BusAndVectors.kicad_sch not found under {}; skipping", root.display());
        None
    }
}

/// The table of KiCad's QA sheet goes through our reader and writer and comes back plotted as it was: the same lines of text and the same border.
#[test]
fn the_table_of_kicads_own_qa_sheet_is_plotted_the_same_after_our_reader_and_writer() {
    let Some(cli) = slow() else { return };
    let Some(qa) = qa_sheet() else { return };
    let dir = scratch("qa");
    // The original, with its two sub-sheets, in a folder of its own.
    let original_dir = dir.join("original");
    std::fs::create_dir_all(&original_dir).unwrap();
    for name in ["BusAndVectors.kicad_sch", "LED_A.kicad_sch", "LED_B.kicad_sch"] {
        if let Some(src) = qa.parent().map(|p| p.join(name)).filter(|p| p.exists()) {
            std::fs::copy(src, original_dir.join(name)).unwrap();
        }
    }
    let (design, model, _) = import_kicad_sch_tree(&original_dir.join("BusAndVectors.kicad_sch")).expect("our reader reads the QA sheet");
    let tables = design.schematic.as_ref().unwrap().extras.tables.clone();
    assert!(!tables.is_empty(), "the QA sheet has a table and our reader keeps it");
    let first = &tables[0];
    assert_eq!((first.col_count(), first.row_count()), (2, 9));
    assert_eq!(first.texts()[0], vec!["LED pin (by VectorIndex)", "Expected net"]);

    // Ours: the tree we write from what we read.
    let rewritten_dir = dir.join("rewritten");
    std::fs::create_dir_all(&rewritten_dir).unwrap();
    for (name, text) in export_kicad_sch_tree(&design, &model, &META, "BusAndVectors.kicad_sch").expect("the tree exports") {
        std::fs::write(rewritten_dir.join(&name), text).unwrap();
    }
    let a = plot(&cli, &original_dir.join("BusAndVectors.kicad_sch"), &dir.join("out_original"));
    let b = plot(&cli, &rewritten_dir.join("BusAndVectors.kicad_sch"), &dir.join("out_rewritten"));

    // Every line of text of every table is plotted by kicad-cli at the same place in both (to the 8 micrometres the stroke font is held to here).
    let cells: Vec<String> = tables.iter().flat_map(|t| t.cells.iter().filter(|c| !c.text.is_empty()).map(|c| c.text.clone())).collect();
    let (pa, pb) = (plotted_text(&a), plotted_text(&b));
    let mut compared = 0;
    for (text, boxa) in pa.iter().filter(|(t, _)| cells.iter().any(|c| c == t)) {
        let want = (text.clone(), *boxa);
        let got = nearest(&pb, &want).unwrap_or_else(|| panic!("the rewritten sheet plots no {text:?}"));
        for k in 0..4 {
            assert!((got.1[k] - boxa[k]).abs() <= 8.0, "{text:?}: {:?} against {:?}", got.1, boxa);
        }
        compared += 1;
    }
    assert!(compared >= 18, "the cells' texts were compared ({compared})");
    // And the lines of the border of each table: where our model puts them, kicad-cli draws them in both plots.
    for t in &tables {
        assert_borders_drawn_as_kicad_cli_plots_them(t, &a, "the QA sheet");
        assert_borders_drawn_as_kicad_cli_plots_them(t, &b, "the rewritten sheet");
    }
    // The texts of the table, placed as our model places them, are where kicad-cli drew them on KiCad's own file.
    for t in &tables {
        assert_text_placed_as_kicad_cli_plots_it(t, &a, "the QA sheet");
    }
    let _ = SCH_DEFAULT_PEN_NM;
}
