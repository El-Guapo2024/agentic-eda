//! `POST /api/microwave/shape`: what the Complex Shape dialog's "Read Shape Description File..." learns from a file (`MWAVE_POLYGONAL_SHAPE_DLG::ReadDataShapeDescr`,
//! `pcbnew/microwave/microwave_polygon.cpp` at KiCad 8303b2ad).
//!
//! A browser gives the page a file's text and not its path, so the file is read here by the same reader the Draw Microwave Polygonal Shapes verb uses
//! (`eda_ops::microwave::read_shape_file`): the dialog's Size boxes are filled with the scales it found (`m_sizeX->SetValue( (int) g_ShapeScaleX )`), in nanometres, and the
//! number of points says whether there is a shape at all. Nothing on the board changes; the shape itself is `Cmd::Microwave` with the file's text.
//!
//! Request: `{ "text": "<the shape description file>" }`. Reply: `{ "ok": true, "size_x_nm", "size_y_nm", "points" }`.
//!
//! `POST /api/microwave/inductor`: whether the inductor between two points can be as long as the "Length of track:" box says. KiCad answers it before it asks the value
//! (`createMicrowaveInductor`: the length dialog, the minimum check, `BuildCornersList_S_Shape`'s verdict, then "Component value:"), so the page asks it between the two prompts.
//! Request: `{ "start": {"x","y"}, "end": {"x","y"}, "length_nm", "track_width_nm" }` (micrometres). Reply: `{ "ok": true }` or `{ "ok": false, "message": "Requested length too large" }`.

use eda_model::ir::Point;
use serde_json::{json, Value};

pub fn read_shape(body: &[u8]) -> Value {
    let req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return json!({ "ok": false, "message": format!("not a shape request: {e}") }),
    };
    let Some(text) = req.get("text").and_then(Value::as_str) else { return json!({ "ok": false, "message": "no shape description file" }) };
    let shape = eda_ops::microwave::read_shape_file(text);
    // `(int)` truncates: 271.501 mm held as 271501000.4 nm is 271501000.
    json!({ "ok": true, "size_x_nm": shape.scale_x.trunc() as i64, "size_y_nm": shape.scale_y.trunc() as i64, "points": shape.edges.len() })
}

/// A point `{ "x", "y" }` of the request, read field by field.
fn point_of(v: &Value) -> Option<Point> {
    Some(Point { x: v.get("x")?.as_i64()?, y: v.get("y")?.as_i64()? })
}

pub fn check_inductor(body: &[u8]) -> Value {
    let req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return json!({ "ok": false, "message": format!("not an inductor request: {e}") }),
    };
    let (Some(start), Some(end)) = (req.get("start").and_then(point_of), req.get("end").and_then(point_of)) else { return json!({ "ok": false, "message": "an inductor needs two points" }) };
    let (Some(length), Some(width)) = (req.get("length_nm").and_then(Value::as_i64), req.get("track_width_nm").and_then(Value::as_i64)) else { return json!({ "ok": false, "message": "an inductor needs a length and a track width" }) };
    match eda_ops::microwave::inductor_corners(start, end, length, width) {
        Ok(_) => json!({ "ok": true }),
        Err(message) => json!({ "ok": false, "message": message }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scales_and_the_point_count_of_a_file_fill_the_dialog() {
        let body = json!({ "text": "Unit=MM\nXScale=271.501\nYScale=1.00133\n\n$COORD\n0 0.61126\n0.00185 0.61048\n$ENDCOORD\n" }).to_string();
        let r = read_shape(body.as_bytes());
        assert_eq!((r["ok"].as_bool(), r["size_x_nm"].as_i64(), r["size_y_nm"].as_i64(), r["points"].as_u64()), (Some(true), Some(271_501_000), Some(1_001_330), Some(2)));
        // inches
        let r = read_shape(json!({ "text": "Unit=inch\nXScale=1\nYScale=0.5\n$COORD\n0 0\n$ENDCOORD\n" }).to_string().as_bytes());
        assert_eq!((r["size_x_nm"].as_i64(), r["size_y_nm"].as_i64()), (Some(25_400_000), Some(12_700_000)));
    }

    #[test]
    fn the_inductors_length_is_checked_before_its_value_is_asked() {
        let ask = |length: i64| check_inductor(json!({ "start": {"x": 0, "y": 0}, "end": {"x": 0, "y": 10_000_000}, "length_nm": length, "track_width_nm": 250_000 }).to_string().as_bytes());
        assert_eq!(ask(20_000_000), json!({ "ok": true }));
        for (length, message) in [(9_000_000, "Requested length < minimum length"), (11_000_000, "Requested length too small"), (15_000_000, "Requested length can't be represented"), (100_000_000, "Requested length too large")] {
            assert_eq!(ask(length), json!({ "ok": false, "message": message }), "{length}");
        }
        assert_eq!(check_inductor(b"{}")["ok"], json!(false));
    }

    #[test]
    fn a_request_without_a_file_is_refused() {
        assert_eq!(read_shape(b"{}")["ok"], json!(false));
        assert_eq!(read_shape(b"nonsense")["ok"], json!(false));
    }
}
