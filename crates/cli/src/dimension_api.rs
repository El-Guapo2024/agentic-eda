//! `POST /api/dimension/preview`: a dimension the studio is drawing or reshaping, worked out before it is committed (UI-LEFT P13).
//!
//! `DRAWING_TOOL::DrawDimension` keeps the dimension it is drawing in a preview (`preview.Add( dimension )`, `m_view->Update( &preview )` on every motion):
//! the crossbar, the extension lines, the arrows and the text, as the commit will draw them. The point editor (`PCB_POINT_EDITOR`) likewise redraws the
//! dimension at every move of a handle. The geometry is `eda_connectivity::dimension::compute_dimension_geometry`'s, the one place it is worked out, so the
//! studio sends the dimension as `add_dimension` / `edit_dimension` take it (`eda_model::ir::Dimension`) and gets back the shape `GET /api/state` gives a
//! board's dimensions (`crate::studio::dimension_json`): `{ "ok": true, "dimension": {...} }`. Nothing is stored and no history is kept.

use serde_json::{json, Value};

pub(crate) fn preview(body: &[u8]) -> Value {
    // As `/api/cmd` reads its command: a `Value` first, then the type (the tagged `kind` holds numbers).
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    match serde_json::from_value::<eda_model::ir::Dimension>(req.get("dimension").cloned().unwrap_or(Value::Null)) {
        Ok(d) => json!({ "ok": true, "dimension": crate::studio::dimension_json(&d) }),
        Err(e) => json!({ "ok": false, "message": format!("not a dimension: {e}") }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dimension_being_drawn_comes_back_with_its_geometry_and_text() {
        let dim = json!({
            "layer": "Cmts.User", "kind": { "kind": "aligned", "height": -2_000_000 },
            "start": { "x": 0, "y": 0 }, "end": { "x": 20_000_000, "y": 0 },
            "prefix": "", "suffix": "", "units": "mm", "units_format": "no_suffix", "precision": 2, "suppress_trailing_zeros": true,
            "text_position": "manual", "keep_text_aligned": true, "text_angle": 0, "text_size_nm": 1_000_000, "stroke_width": 100_000,
            "arrow_length": 1_270_000, "extension_offset": 500_000, "extension_height": 586_420, "arrow_direction": "outward",
            "text_pos": { "x": 5_000_000, "y": -2_000_000 }
        });
        let v = preview(json!({ "dimension": dim }).to_string().as_bytes());
        assert_eq!(v["ok"], true, "{v}");
        let d = &v["dimension"];
        assert_eq!(d["text"], "20");
        assert_eq!(d["text_position"], "manual");
        assert_eq!(d["text_at"], json!([5_000_000, -2_000_000]), "a text placed by hand stays: {d}");
        assert_eq!(d["crossbar"], json!([[0, -2_000_000], [20_000_000, -2_000_000]]));
        assert!(d["lines"].as_array().unwrap().len() >= 7);
        let bad = preview(b"{\"dimension\": {\"layer\": \"F.Fab\"}}");
        assert_eq!(bad["ok"], false);
    }

    #[test]
    fn a_leaders_frame_comes_back_as_lines_or_a_circle() {
        let leader = |frame: &str| {
            json!({ "dimension": {
                "layer": "Cmts.User", "kind": { "kind": "leader" }, "start": { "x": 0, "y": 0 }, "end": { "x": 5_000_000, "y": -5_000_000 },
                "prefix": "", "suffix": "", "override_text": "NOTE", "units": "inch", "units_format": "no_suffix", "precision": 4, "suppress_trailing_zeros": false,
                "text_position": "outside", "keep_text_aligned": false, "text_angle": 0, "text_size_nm": 1_000_000, "stroke_width": 100_000,
                "arrow_length": 1_270_000, "extension_offset": 500_000, "extension_height": 586_420, "arrow_direction": "outward",
                "text_pos": { "x": 15_000_000, "y": -5_000_000 }, "text_frame": frame
            }})
            .to_string()
        };
        let none = preview(leader("none").as_bytes());
        let rect = preview(leader("rectangle").as_bytes());
        let circle = preview(leader("circle").as_bytes());
        let lines = |v: &Value| v["dimension"]["lines"].as_array().unwrap().len();
        assert_eq!(lines(&rect), lines(&none) + 4, "{rect}");
        assert_eq!(circle["dimension"]["circles"].as_array().unwrap().len(), 1, "{circle}");
        assert_eq!(circle["dimension"]["text_frame"], "circle");
    }

    /// A dimension's label looks (UI-LEFT-2 PP11) as the studio sends them (`text_effects`) come back in the preview, and through `/api/cmd` an `edit_dimension`
    /// that sets them is one undo step of the board.
    #[test]
    fn a_labels_looks_come_back_and_an_edit_of_them_is_one_undo_step() {
        let dim = json!({
            "layer": "B.Fab", "kind": { "kind": "aligned", "height": 2_000_000 },
            "start": { "x": 0, "y": 0 }, "end": { "x": 20_000_000, "y": 0 },
            "prefix": "", "suffix": "", "units": "mm", "units_format": "no_suffix", "precision": 2, "suppress_trailing_zeros": true,
            "text_position": "outside", "keep_text_aligned": true, "text_angle": 0, "text_size_nm": 1_000_000, "stroke_width": 100_000,
            "arrow_length": 1_270_000, "extension_offset": 500_000, "extension_height": 586_420, "arrow_direction": "outward"
        });
        let plain = preview(json!({ "dimension": dim }).to_string().as_bytes());
        let d = &plain["dimension"];
        assert_eq!((d["text_width_nm"].as_i64(), d["mirrored"].as_bool(), d["h_justify"].as_str(), d["face"].is_null()), (Some(1_000_000), Some(true), Some("center"), true), "a back label is mirrored by its layer: {d}");
        let mut styled = dim.clone();
        styled["text_effects"] = json!({ "width_nm": 1_500_000, "face": "DejaVu Sans", "bold": true, "italic": true, "mirrored": false, "h_justify": "left", "keep_upright": true });
        let v = preview(json!({ "dimension": styled }).to_string().as_bytes());
        let d = &v["dimension"];
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(
            (d["text_width_nm"].as_i64(), d["face"].as_str(), d["bold"].as_bool(), d["italic"].as_bool(), d["mirrored"].as_bool(), d["h_justify"].as_str(), d["keep_upright"].as_bool()),
            (Some(1_500_000), Some("DejaVu Sans"), Some(true), Some(true), Some(false), Some("left"), Some(true)),
            "{d}"
        );
        assert_eq!(d["text_effects"]["width_nm"], 1_500_000);

        use crate::board::tests::{scratch, setup};
        use crate::board::{load, step, undo};
        let dir = scratch("dimension_label_looks");
        setup(&dir);
        let cmd = |v: Value| serde_json::from_value::<eda_ops::Cmd>(v).expect("a command, as /api/cmd reads it");
        step(&dir, cmd(json!({ "op": "add_dimension", "dimension": dim })), false, "test").unwrap();
        let dims = |dir: &std::path::Path| load(dir).unwrap().1.drawings.map(|d| d.dimensions).unwrap_or_default();
        let id = dims(&dir)[0].id.clone();
        step(&dir, cmd(json!({ "op": "edit_dimension", "id": id, "dimension": styled })), false, "test").unwrap();
        let fx = dims(&dir)[0].text_effects.clone();
        assert_eq!((fx.width_nm, fx.face.as_deref(), fx.bold, fx.italic, fx.mirrored, fx.h_justify), (Some(eda_model::units::mm(1.5)), Some("DejaVu Sans"), true, true, Some(false), eda_model::ir::TextJustify::Left));
        undo(&dir, "test", Some(eda_ops::Domain::Pcb)).unwrap();
        assert!(dims(&dir)[0].text_effects.is_default(), "one undo takes the looks back");
    }
}
