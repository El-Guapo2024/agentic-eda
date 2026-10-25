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
