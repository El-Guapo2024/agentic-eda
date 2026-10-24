//! A table in the studio's state: the grid, the borders, the corners of every cell and the placed lines of its text, all worked out here
//! (`eda_model::table`, `eda_model::table_text`) so that the canvas draws what kicad-cli plots without a second copy of the geometry.
//!
//! The state keeps the raw text of a cell beside what it shows (`shown`, only where the two differ: the variables resolved and the lines broken to the
//! column), because an edit starts from the raw one, as KiCad's dialogs do.

use eda_model::table::{cell_item_id, Table, TableLineStyle, TableStroke};
use eda_model::table_text::{pcb_cell_draw, sch_cell_draw, CellDraw};
use eda_model::text_vars::{PcbBoard, Resolver};
use serde_json::{json, Value};

fn color_json(c: Option<eda_model::table::TableColor>) -> Value {
    c.map_or(Value::Null, |c| json!([c.r, c.g, c.b, c.a]))
}

fn style_token(s: TableLineStyle) -> &'static str {
    s.token()
}

pub(crate) fn stroke_json(s: &TableStroke) -> Value {
    json!({ "width": s.width_nm, "style": style_token(s.style), "color": color_json(s.color) })
}

fn pt(p: eda_model::ir::Point) -> Value {
    json!([p.x, p.y])
}

fn halign(h: eda_model::ir::TextJustify) -> &'static str {
    match h {
        eda_model::ir::TextJustify::Left => "left",
        eda_model::ir::TextJustify::Center => "center",
        eda_model::ir::TextJustify::Right => "right",
    }
}

fn valign(v: eda_model::ir::TextVAlign) -> &'static str {
    match v {
        eda_model::ir::TextVAlign::Top => "top",
        eda_model::ir::TextVAlign::Center => "center",
        eda_model::ir::TextVAlign::Bottom => "bottom",
    }
}

/// One board table as the studio's state holds it.
pub(crate) fn table_json(t: &Table, board: &PcbBoard) -> Value {
    table_value(t, &|row, col| pcb_cell_draw(t, row, col, board), false)
}

/// One table of the sheet in view as the studio's state holds it: the same shape as a board's, with what only a sheet's cell has (`fill`, `fill_color`,
/// `exclude_from_sim`) and a stroke of width 0 sent as the sheet's default width. `sheet` resolves the sheet's variables.
pub(crate) fn sch_table_json(t: &Table, sheet: &Resolver) -> Value {
    table_value(t, &|row, col| sch_cell_draw(t, row, col, sheet), true)
}

/// The width a stroke of width 0 draws with on a sheet: 6 mils (`DEFAULT_LINE_WIDTH_MILS`), nanometres.
const SCH_DEFAULT_LINE_NM: f64 = 152_400.0;

fn table_value(t: &Table, draw_of: &dyn Fn(usize, usize) -> CellDraw, sheet: bool) -> Value {
    if !t.is_well_formed() {
        return json!({ "id": t.id, "layer": t.layer, "x": t.at.x, "y": t.at.y, "angle": t.angle, "rows": 0, "cols": 0, "cells": [], "borders": [] });
    }
    let bbox = t.bounding_box();
    let cells: Vec<Value> = (0..t.cells.len())
        .map(|i| {
            let (row, col) = t.row_col(i);
            let c = &t.cells[i];
            let draw = draw_of(row, col);
            let corners = t.cell_corners(row, col);
            let mut v = json!({
                "id": cell_item_id(&t.id, row, col),
                "row": row,
                "col": col,
                "text": c.text,
                "col_span": c.col_span,
                "row_span": c.row_span,
                "corners": corners.iter().map(|p| pt(*p)).collect::<Vec<_>>(),
                "margins": c.margins,
                "h_align": halign(c.style.h_align),
                "v_align": valign(c.style.v_align),
                "size_w": c.style.width_nm,
                "size_h": c.style.height_nm,
                "thickness": c.style.thickness_nm,
                "pen": draw.pen_nm,
                "bold": c.style.bold,
                "italic": c.style.italic,
                "mirror": c.style.mirror,
                "face": c.style.face,
                "line_spacing": c.style.line_spacing_permille,
                "color": color_json(c.style.color),
                "lines": draw.lines.iter().map(|l| json!({ "text": l.text, "x": l.x_nm, "y": l.y_nm })).collect::<Vec<_>>(),
            });
            if draw.shown != c.text {
                v["shown"] = json!(draw.shown);
            }
            if sheet {
                v["fill"] = json!(c.fill.token());
                v["fill_color"] = color_json(c.fill_color);
                v["exclude_from_sim"] = json!(c.exclude_from_sim);
            }
            v
        })
        .collect();
    let border_width = |w: i64| if sheet && w == 0 { json!(SCH_DEFAULT_LINE_NM) } else { json!(w) };
    json!({
        "id": t.id,
        "layer": t.layer,
        "x": t.at.x,
        "y": t.at.y,
        "angle": t.angle,
        "rows": t.row_count(),
        "cols": t.col_count(),
        "col_widths": t.col_widths,
        "row_heights": t.row_heights,
        "border": { "external": t.border.external, "header": t.border.header, "stroke": stroke_json(&t.border.stroke) },
        "separators": { "rows": t.separators.rows, "cols": t.separators.cols, "stroke": stroke_json(&t.separators.stroke) },
        "bbox": [bbox.x0, bbox.y0, bbox.x1, bbox.y1],
        "borders": t.borders().iter().map(|l| json!({ "a": pt(l.a), "b": pt(l.b), "width": border_width(l.stroke.width_nm), "style": style_token(l.stroke.drawn_style()), "color": color_json(l.stroke.color) })).collect::<Vec<_>>(),
        "cells": cells,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::Point;
    use eda_model::table::{TableBorder, TableCell, TableSeparators};

    #[test]
    fn a_tables_state_carries_the_corners_the_borders_and_the_placed_lines() {
        let mut cell = TableCell::pcb_new(false);
        cell.text = "R${ROW} ${ADDR}".into();
        let t = Table {
            id: "tbl_x".into(),
            uuid: String::new(),
            layer: "F.SilkS".into(),
            at: Point { x: 1_000_000, y: 2_000_000 },
            angle: 0,
            col_widths: vec![20_000_000],
            row_heights: vec![6_000_000],
            cells: vec![cell],
            border: TableBorder { external: true, header: true, stroke: TableStroke::solid(100_000) },
            separators: TableSeparators::default(),
        };
        let v = table_json(&t, &PcbBoard::default());
        assert_eq!(v["id"], "tbl_x");
        assert_eq!(v["bbox"], json!([1_000_000, 2_000_000, 21_000_000, 8_000_000]));
        assert_eq!(v["borders"].as_array().unwrap().len(), 4);
        let c = &v["cells"][0];
        assert_eq!(c["id"], "tbl_x:0:0");
        assert_eq!(c["corners"], json!([[1_000_000, 2_000_000], [21_000_000, 2_000_000], [21_000_000, 8_000_000], [1_000_000, 8_000_000]]));
        assert_eq!(c["shown"], "R1 A1");
        assert_eq!(c["lines"][0]["text"], "R1 A1");
    }

    #[test]
    fn a_sheets_table_counts_from_zero_resolves_the_sheets_variables_and_draws_a_stroke_of_width_zero_at_the_default_width() {
        let mut t = Table::new_sch_grid(Point { x: 10_000_000, y: 20_000_000 }, 2, 1, 30_000_000, 5_000_000, 1_270_000);
        t.cells[0].text = "${ADDR} ${ROW}/${COL}".into();
        t.cells[1].text = "${TITLE}".into();
        t.cells[1].fill = eda_model::sch_extras::SchFill::Color;
        t.cells[1].fill_color = Some(eda_model::sch_extras::SchColor { r: 1, g: 2, b: 3, a: 255 });
        t.id = "tbl_s".into();
        let v = sch_table_json(&t, &|tok: &str| (tok == "TITLE").then(|| "My sheet".to_string()));
        assert_eq!(v["layer"], "");
        assert_eq!(v["cells"][0]["shown"], "A0 0/0");
        assert_eq!(v["cells"][1]["shown"], "My sheet");
        assert_eq!((v["cells"][1]["fill"].as_str(), &v["cells"][1]["fill_color"]), (Some("color"), &json!([1, 2, 3, 255])));
        assert_eq!(v["cells"][0]["fill"], "none");
        assert_eq!(v["cells"][0]["exclude_from_sim"], false);
        assert!(v["borders"].as_array().unwrap().iter().all(|b| b["width"] == 152_400.0), "a stroke of width 0 is the sheet's default width");
        // the placed text uses the sheet's pen, not an eighth of the glyph
        assert_eq!(v["cells"][0]["pen"], 152_400.0);
    }
}
