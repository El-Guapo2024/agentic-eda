//! The board's tables as kicad-cli judges them (GAPS.md item 24, package P16t): the stackup table and the board characteristics table that Place > Add Stackup Table
//! and Add Board Characteristics build are plotted by kicad-cli (`pcb export svg`) the way we laid them out -- the text of every cell on a line of its own, no cell
//! broken by KiCad's line breaking, and the lines of the border and the separators where `PCB_TABLE::DrawBorders` puts them.

use super::tests::{scratch, setup};
use super::{load, step};
use eda_model::ir::Point;
use eda_model::table::Table;
use eda_ops::{Cmd, TableCmd};
use std::path::Path;

/// The SVG plot of `layer` kicad-cli makes of the board in `dir`.
fn plot(dir: &Path, layer: &str) -> String {
    let (_, design, model) = load(dir).expect("the board loads");
    let work = dir.join("table_work");
    let reply = eda_kicad_engine::export_pcb(&design, &model, &work, dir, "board", "svg", &["--layers".to_string(), layer.to_string(), "--exclude-drawing-sheet".to_string()]).unwrap_or_else(|e| panic!("kicad-cli plots the board: {e:?}"));
    let file = reply["files"][0].as_str().unwrap_or_else(|| panic!("no file in {reply}")).to_string();
    std::fs::read_to_string(dir.join(file)).expect("the plot is there")
}

/// The text of each `<text>` of the plot, which is one per line of text drawn (the glyphs are drawn as strokes beside it).
fn plotted_texts(svg: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = svg;
    while let Some(i) = rest.find("<text ") {
        let after = &rest[i..];
        let (Some(open), Some(close)) = (after.find('>'), after.find("</text>")) else { break };
        out.push(after[open + 1..close].trim().to_string());
        rest = &after[close + 7..];
    }
    out
}

/// The straight lines the plot draws that are not glyph strokes (`<path d="M x y\nL x y">` outside the `stroked-text` groups): the table's border and separators, in
/// micrometres, each with its ends in reading order.
fn plotted_lines(svg: &str) -> Vec<[i64; 4]> {
    let mut bare = String::new();
    let mut rest = svg;
    while let Some(i) = rest.find("<g class=\"stroked-text\">") {
        bare.push_str(&rest[..i]);
        let after = &rest[i..];
        let end = after.find("</g>").map(|e| e + 4000).unwrap_or(after.len());
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
        // Only a plain segment: `M x y L x y`, not a closed outline.
        if d.contains(',') || d.contains('Z') {
            continue;
        }
        let nums: Vec<f64> = d.split(|c: char| c == 'L' || c.is_whitespace()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
        if let [x0, y0, x1, y1] = nums[..] {
            let um = |v: f64| (v * 1000.0).round() as i64;
            let (a, b) = ((um(x0), um(y0)), (um(x1), um(y1)));
            let (a, b) = if a <= b { (a, b) } else { (b, a) };
            out.push([a.0, a.1, b.0, b.1]);
        }
    }
    out
}

/// The text of every cell that has any and is not covered, trimmed (a trailing space is dropped at the end of a line).
fn cell_texts(t: &Table) -> Vec<String> {
    let mut out: Vec<String> = t.cells.iter().filter(|c| !c.is_covered() && !c.text.trim().is_empty()).map(|c| c.text.trim().to_string()).collect();
    out.sort();
    out
}

fn the_tables(dir: &Path) -> Vec<Table> {
    let (_, design, _) = load(dir).expect("the board loads");
    design.drawings.map(|d| d.tables).unwrap_or_default()
}

/// Slow tier (kicad-cli, `EDA_SLOW_TESTS=1`): the two tables the Place menu builds -- placed through the verbs, written into the board file, plotted by kicad-cli.
#[test]
fn kicad_cli_plots_the_stackup_and_characteristics_tables_the_way_they_were_laid_out() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    if eda_kicad_engine::find_cli().is_none() {
        eprintln!("kicad-cli not found; skipping");
        return;
    }
    let dir = scratch("tables_plot");
    setup(&dir);
    step(&dir, Cmd::Table(TableCmd::PlaceStackup { layer: "Dwgs.User".into(), at: Point { x: 2_000_000, y: 2_000_000 }, units: None }), false, "test").unwrap();
    let characteristics: Cmd = serde_json::from_value(serde_json::json!({
        "op": "table", "verb": "place_characteristics", "layer": "Cmts.User", "at": { "x": 2_000_000, "y": 100_000_000 }, "units": "mm",
        "numbers": { "thickness_nm": 1_600_000, "width_nm": 20_000_000, "height_nm": 20_000_000, "min_track_nm": 200_000, "min_clearance_nm": 200_000, "min_drill_nm": 300_000, "castellated_pads": 0, "press_fit_pads": 2 }
    }))
    .unwrap();
    step(&dir, characteristics, false, "test").unwrap();
    let tables = the_tables(&dir);
    assert_eq!(tables.len(), 2, "both tables are on the board");
    let (stackup, chars) = (&tables[0], &tables[1]);
    assert_eq!((stackup.col_count(), chars.col_count()), (7000, 4000));

    // The stackup table, on Dwgs.User (no footprint draws its text there): every cell's text drawn whole on one line, and the lines of its borders.
    let svg = plot(&dir, "Dwgs.User");
    let mut drawn = plotted_texts(&svg);
    drawn.sort();
    assert_eq!(drawn, cell_texts(stackup), "kicad-cli draws the text of every cell of the stackup table on a line of its own (none broken to the column)");
    let lines = plotted_lines(&svg);
    let expected: Vec<[i64; 4]> = stackup
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
        assert!(lines.iter().any(|l| l.iter().zip(line).all(|(a, b)| (a - b).abs() <= 2000)), "kicad-cli draws no line {line:?} of the table's border; it drew {lines:?}");
    }
    assert_eq!(lines.len(), expected.len(), "and no other line: {lines:?} against {expected:?}");

    // The characteristics table, on Cmts.User: no border at all, the text on lines of its own.
    let svg = plot(&dir, "Cmts.User");
    let mut drawn = plotted_texts(&svg);
    drawn.sort();
    assert_eq!(drawn, cell_texts(chars), "kicad-cli draws the text of every cell of the characteristics table on a line of its own");
    assert!(chars.borders().is_empty(), "the characteristics table has no lines");
    assert!(plotted_lines(&svg).is_empty(), "kicad-cli draws none: {:?}", plotted_lines(&svg));
}

/// The move preview of a held sheet table (`POST /api/sch/move_preview`) carries the table where the drop would put it, every cell's corners and text lines worked out
/// by the server, and a selected cell holds its table (`aPromoteCellSelections`).
#[test]
fn the_move_preview_carries_a_held_sheet_table_where_it_would_be() {
    let dir = scratch("sch_table_preview");
    setup(&dir);
    let mut t = Table::new_sch_grid(Point { x: 20_000_000, y: 30_000_000 }, 2, 2, 20_000_000, 5_000_000, 1_270_000);
    t.cells[0].text = "TITLE ${ROW}".into();
    step(&dir, Cmd::SchTable(TableCmd::Add { table: t }), false, "test").unwrap();
    let (_, design, _) = load(&dir).expect("the board loads");
    let id = design.schematic.as_ref().unwrap().extras.tables[0].id.clone();
    let cell = eda_model::table::cell_item_id(&id, 1, 1);
    let preview = |cmd: serde_json::Value| crate::sch_move_api::move_preview(&dir, serde_json::json!({ "sheet": "", "cmds": [cmd] }).to_string().as_bytes());
    let before = preview(serde_json::json!({ "op": "sch_move", "verb": "move", "ids": [cell], "dx": 0, "dy": 0 }));
    let v = preview(serde_json::json!({ "op": "sch_move", "verb": "move", "ids": [cell], "dx": 2_540_000, "dy": 1_270_000 }));
    assert_eq!(v["ok"], serde_json::json!(true), "{v}");
    let corner = |v: &serde_json::Value| v["tables"][0]["cells"][0]["corners"][0].clone();
    assert_eq!(v["tables"][0]["id"], serde_json::json!(id), "{v}");
    assert_eq!((corner(&before), corner(&v)), (serde_json::json!([20_000_000, 30_000_000]), serde_json::json!([22_540_000, 31_270_000])), "the cell held the table: it moved by the offset");
    // the text lines go with it, and what a variable stands for in the cell is shown, not the variable
    let line = |v: &serde_json::Value| (v["tables"][0]["cells"][0]["lines"][0]["x"].as_f64().unwrap(), v["tables"][0]["cells"][0]["lines"][0]["text"].clone());
    assert!((line(&v).0 - line(&before).0 - 2_540_000.0).abs() < 1.0, "{}", v["tables"][0]["cells"][0]);
    assert_eq!(line(&v).1, serde_json::json!("TITLE 0"), "${{ROW}} of the first row of a sheet table is 0");
    let _ = std::fs::remove_dir_all(&dir);
}
