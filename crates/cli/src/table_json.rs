//! A table in the studio's state: the grid, the borders, the corners of every cell and the placed lines of its text, all worked out here
//! (`eda_model::table`, `eda_model::table_text`) so that the canvas draws what kicad-cli plots without a second copy of the geometry.
//!
//! The state keeps the raw text of a cell beside what it shows (`shown`, only where the two differ: the variables resolved and the lines broken to the
//! column), because an edit starts from the raw one, as KiCad's dialogs do.

use eda_model::table::{cell_item_id, Table, TableLineStyle, TableStroke};
use eda_model::table_text::pcb_cell_draw;
use eda_model::text_vars::PcbBoard;
use serde_json::{json, Value};

fn color_json(c: Option<eda_model::table::TableColor>) -> Value {
    c.map_or(Value::Null, |c| json!([c.r, c.g, c.b, c.a]))
}

fn style_token(s: TableLineStyle) -> &'static str {
    s.token()
}

pub(crate) fn stroke_json(s: &TableStroke) -> Value {
    json!({ "width": s.width_um, "style": style_token(s.style), "color": color_json(s.color) })
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
    if !t.is_well_formed() {
        return json!({ "id": t.id, "layer": t.layer, "x": t.at.x, "y": t.at.y, "angle": t.angle, "rows": 0, "cols": 0, "cells": [], "borders": [] });
    }
    let bbox = t.bounding_box();
    let cells: Vec<Value> = (0..t.cells.len())
        .map(|i| {
            let (row, col) = t.row_col(i);
            let c = &t.cells[i];
            let draw = pcb_cell_draw(t, row, col, board);
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
                "size_w": c.style.width_um,
                "size_h": c.style.height_um,
                "thickness": c.style.thickness_um,
                "pen": draw.pen_um,
                "bold": c.style.bold,
                "italic": c.style.italic,
                "mirror": c.style.mirror,
                "face": c.style.face,
                "line_spacing": c.style.line_spacing_permille,
                "color": color_json(c.style.color),
                "lines": draw.lines.iter().map(|l| json!({ "text": l.text, "x": l.x_um, "y": l.y_um })).collect::<Vec<_>>(),
            });
            if draw.shown != c.text {
                v["shown"] = json!(draw.shown);
            }
            v
        })
        .collect();
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
        "borders": t.borders().iter().map(|l| json!({ "a": pt(l.a), "b": pt(l.b), "width": l.stroke.width_um, "style": style_token(l.stroke.drawn_style()), "color": color_json(l.stroke.color) })).collect::<Vec<_>>(),
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
            at: Point { x: 1_000, y: 2_000 },
            angle: 0,
            col_widths: vec![20_000],
            row_heights: vec![6_000],
            cells: vec![cell],
            border: TableBorder { external: true, header: true, stroke: TableStroke::solid(100) },
            separators: TableSeparators::default(),
        };
        let v = table_json(&t, &PcbBoard::default());
        assert_eq!(v["id"], "tbl_x");
        assert_eq!(v["bbox"], json!([1_000, 2_000, 21_000, 8_000]));
        assert_eq!(v["borders"].as_array().unwrap().len(), 4);
        let c = &v["cells"][0];
        assert_eq!(c["id"], "tbl_x:0:0");
        assert_eq!(c["corners"], json!([[1_000, 2_000], [21_000, 2_000], [21_000, 8_000], [1_000, 8_000]]));
        assert_eq!(c["shown"], "R1 A1");
        assert_eq!(c["lines"][0]["text"], "R1 A1");
    }
}
