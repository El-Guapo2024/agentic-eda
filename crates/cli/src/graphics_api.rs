//! `POST /api/import_graphics/read`: what a DXF or SVG file holds, for the Import Graphics dialog.
//!
//! The dialog reads the file in the page (a browser hands the page a file's text, not its path) and asks here what is in it before the person
//! chooses where it goes: how many lines, arcs, circles and so on, what the plugin could not handle (`GRAPHICS_IMPORTER::GetMessages`, which KiCad
//! shows after the import), how big the picture is, and whether it fits (`GRAPHICS_IMPORTER_BUFFER::ImportTo`'s size check). Nothing on the
//! board changes; the import itself is `Cmd::ImportGraphics`.
//!
//! Request: the command's own fields (`format`, `text`, and optionally `dxf_units`, `scale`, `line_width_nm`, ...). Reply:
//! `{ "ok": true, "items": n, "counts": { "line": n, ... }, "messages": [{ "text", "count" }], "bbox_nm": [x0, y0, x1, y1] | null,
//! "width_mm", "height_mm", "unit": "mm" }` or `{ "ok": false, "message": "..." }`.

use eda_model::units::Nm;
use eda_kicad::graphics_import::ImportedShape;
use eda_ops::{graphics_board_items, ImportGraphics};
use serde_json::{json, Value};

pub fn read(body: &[u8]) -> Value {
    let mut req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return json!({ "ok": false, "message": format!("not an import request: {e}") }),
    };
    if let Some(o) = req.as_object_mut() {
        o.entry("layer").or_insert(json!("Dwgs.User"));
        o.remove("op");
    }
    // A plain struct (no tagged enum), so the derived reader is fine for the numbers.
    let r: ImportGraphics = match serde_json::from_value(req) {
        Ok(r) => r,
        Err(e) => return json!({ "ok": false, "message": format!("not an import request: {e}") }),
    };
    let imported = match r.read() {
        Ok(i) => i,
        Err(e) => return json!({ "ok": false, "message": e }),
    };
    let mut counts = serde_json::Map::new();
    for s in &imported.shapes {
        let k = match s {
            ImportedShape::Line { .. } => "line",
            ImportedShape::Circle { .. } => "circle",
            ImportedShape::Arc { .. } => "arc",
            ImportedShape::Polygon { .. } => "polygon",
            ImportedShape::Text { .. } => "text",
            ImportedShape::Spline { .. } => "curve",
        };
        let n = counts.get(k).and_then(Value::as_u64).unwrap_or(0) + 1;
        counts.insert(k.into(), json!(n));
    }
    // The items the board would get, at this scale and with no offset: the size check and the picture's box in the board's unit.
    let mut mapper = r.mapper();
    let items = graphics_board_items(&imported, &mut mapper, &r.layer);
    let mut bbox: Option<[Nm; 4]> = None;
    let mut grow = |x: Nm, y: Nm| match &mut bbox {
        None => bbox = Some([x, y, x, y]),
        Some(b) => {
            b[0] = b[0].min(x);
            b[1] = b[1].min(y);
            b[2] = b[2].max(x);
            b[3] = b[3].max(y);
        }
    };
    for s in &items.shapes {
        for p in s.points() {
            grow(p.x, p.y);
        }
        // A circle's box is its centre's, one radius out.
        if let eda_model::ir::Shape::Circle { center, end, .. } = s {
            let r = Nm::kiround((end.x - center.x).as_f64().hypot((end.y - center.y).as_f64()));
            grow(center.x - r, center.y - r);
            grow(center.x + r, center.y + r);
        }
    }
    for t in &items.texts {
        grow(t.at.x, t.at.y);
    }
    let mut messages: Vec<(String, usize)> = imported.message_counts();
    for m in &items.messages {
        messages.push((m.clone(), 1));
    }
    let count = items.shapes.len() + items.texts.len();
    json!({
        "ok": true,
        "items": count,
        "counts": counts,
        "messages": messages.iter().map(|(t, n)| json!({ "text": t, "count": n })).collect::<Vec<_>>(),
        "bbox_nm": bbox,
        "width_mm": imported.width_mm(),
        "height_mm": imported.height_mm(),
        "fits": !items.messages.iter().any(|m| m.starts_with("Imported graphic is too large")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_json(v: Value) -> Value {
        read(v.to_string().as_bytes())
    }

    #[test]
    fn a_dxf_is_counted_and_boxed_in_the_boards_unit() {
        let dxf = "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n0\n20\n0\n11\n10\n21\n0\n0\nCIRCLE\n10\n5\n20\n5\n40\n2\n0\nHATCH\n0\nENDSEC\n0\nEOF\n";
        let r = read_json(json!({ "format": "dxf", "text": dxf, "scale": 2.0 }));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["items"], 2);
        assert_eq!(r["counts"]["line"], 1);
        assert_eq!(r["counts"]["circle"], 1);
        assert_eq!(r["messages"][0]["text"], "DXF hatches not currently supported.");
        // Scaled by 2, the circle at (5, -5) with radius 2 reaches x 6..14 mm and y -14..-6 mm; the line is x 0..20.
        assert_eq!(r["bbox_nm"], json!([0, -14_000_000, 20_000_000, 0]));
        assert_eq!(r["fits"], true);
    }

    #[test]
    fn a_file_that_cannot_be_read_says_why() {
        let r = read_json(json!({ "format": "dxf", "text": "hello" }));
        assert_eq!(r["ok"], false);
        assert!(r["message"].as_str().unwrap().contains("not a DXF"), "{r}");
        assert_eq!(read(b"not json")["ok"], false);
        assert_eq!(read_json(json!({ "format": "pdf", "text": "x" }))["ok"], false);
    }

    #[test]
    fn a_picture_too_large_for_kicad_does_not_fit() {
        let dxf = "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n0\n20\n0\n11\n5000\n21\n0\n0\nENDSEC\n0\nEOF\n";
        let r = read_json(json!({ "format": "dxf", "text": dxf }));
        assert_eq!(r["fits"], false, "{r}");
        assert_eq!(r["items"], 0);
    }
}

/// `POST /api/import_graphics/preview`: the items an interactive Import Graphics would put on the board, before any is put there -- what `DRAWING_TOOL::PlaceImportedGraphics`
/// shows in its preview (`m_view->Add( &preview )`) while they follow the cursor. Request: `{ "cmd": <the import_graphics command> }` at the offset the page will move from
/// (0, 0). The command runs on a copy of the board, nothing is saved. Reply: `{ "ok": true, "shapes": [..], "texts": [..], "reference": { "x", "y" } }`, the shapes and the
/// texts as the board state has them, and the point they are held by -- the position of the top left item of the preview (`preview.GetTopLeftItem()->GetPosition()`: the
/// smallest x, then the smallest y, by each item's `GetPosition()`, the group's being the middle of its box) -- or `{ "ok": false, "message": .. }` in the command's words.
pub fn preview(dir: &std::path::Path, body: &[u8]) -> Value {
    let req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return json!({ "ok": false, "message": format!("not a preview request: {e}") }),
    };
    let cmd: eda_ops::Cmd = match serde_json::from_value(req.get("cmd").cloned().unwrap_or(Value::Null)) {
        Ok(c @ eda_ops::Cmd::ImportGraphics(_)) => c,
        Ok(_) => return json!({ "ok": false, "message": "the preview is of an import_graphics command" }),
        Err(e) => return json!({ "ok": false, "message": format!("not an import command: {e}") }),
    };
    let (meta, design, model) = match crate::board::load(dir) {
        Ok(l) => l,
        Err(e) => return json!({ "ok": false, "message": crate::board::reasons(&e) }),
    };
    let (n_shapes, n_texts, n_groups) = design.drawings.as_ref().map_or((0, 0, 0), |d| (d.shapes.len(), d.texts.len(), d.groups.len()));
    let mut b = eda_ops::Board::new(design, &model, meta.snap_nm, meta.spacing_nm);
    if let Err(e) = b.apply(&cmd) {
        return json!({ "ok": false, "message": crate::board::reasons(&e) });
    }
    let Some(d) = b.design().drawings.as_ref() else { return json!({ "ok": false, "message": "No graphic items found in file." }) };
    let shapes = &d.shapes[n_shapes.min(d.shapes.len())..];
    let texts = &d.texts[n_texts.min(d.texts.len())..];
    // `GetTopLeftItem`: every item of the preview, the group too
    let mut positions: Vec<eda_model::ir::Point> = shapes.iter().map(crate::append_api::shape_position).chain(texts.iter().map(|t| t.at)).collect();
    if d.groups.len() > n_groups && !positions.is_empty() {
        let (x0, x1) = (positions.iter().map(|p| p.x).min().unwrap_or(Nm::ZERO), positions.iter().map(|p| p.x).max().unwrap_or(Nm::ZERO));
        let (y0, y1) = (positions.iter().map(|p| p.y).min().unwrap_or(Nm::ZERO), positions.iter().map(|p| p.y).max().unwrap_or(Nm::ZERO));
        positions.push(eda_model::ir::Point { x: Nm::new((x0.get() + x1.get()) / 2), y: Nm::new((y0.get() + y1.get()) / 2) });
    }
    let reference = positions.iter().min_by(|a, b| a.x.cmp(&b.x).then(a.y.cmp(&b.y))).copied().unwrap_or(eda_model::ir::Point { x: Nm::ZERO, y: Nm::ZERO });
    json!({
        "ok": true,
        "shapes": shapes.iter().map(crate::studio::shape_json).collect::<Vec<_>>(),
        "texts": texts.iter().map(|t| json!({
            "id": t.id, "content": t.content, "x": t.at.x, "y": t.at.y, "angle": t.angle, "layer": t.layer, "size": t.size_nm, "stroke_width": t.stroke_width,
            "justify": match t.justify { eda_model::ir::TextJustify::Left => "left", eda_model::ir::TextJustify::Center => "center", eda_model::ir::TextJustify::Right => "right" },
            "mirror": t.mirror,
        })).collect::<Vec<_>>(),
        "reference": { "x": reference.x, "y": reference.y },
    })
}
