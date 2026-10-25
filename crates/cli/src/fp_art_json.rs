//! A placed footprint's graphics and texts for the studio's board state: `parts[].graphics` and `parts[].texts`, in board space (what the canvas draws, what a
//! click hits and what a move carries), the same way a pad is. The model side is [`eda_model::fp_art`]: the art lives in the footprint's own frame, and
//! [`FpShape::placed`] and [`FpText::placed`] take it to the board for the pose the footprint is in, so a move, a turn or a flip needs nothing here.
//!
//! A graphic is the JSON of a board-level `Shape` (`kind`, `layer`, `stroke_width`, `filled`, the points: `studio::shape_json`), so the canvas draws it with
//! the code that draws the board's own shapes, without an `id` (art is not addressed) and with what a `Shape` has no field for: `dash` for a stroke that is not
//! solid, `hatch` for a hatched fill, `layers` for a shape on several layers, `start_ending` and `end_ending` for a line's endings. A text has the keys of a field (`FieldInfo`), at the angle it has on the board; its
//! variables (`${REFERENCE}` ...) are resolved.

use eda_model::footprint::to_board;
use eda_model::fp_art::{FpShape, LineStyle, TextVars};
use eda_model::ir::{FootprintInstance, Point, Nm};
use eda_model::{Footprint, Part};
use serde_json::{json, Value};

fn graphic_json(s: &FpShape) -> Value {
    let mut v = crate::studio::shape_json(&s.shape);
    if let Some(o) = v.as_object_mut() {
        // Not addressed, and mostly not filled: leave the keys out when they say nothing.
        o.remove("id");
        if !s.shape.is_filled() {
            o.remove("filled");
        }
        if !matches!(s.style, LineStyle::Solid | LineStyle::Unset) {
            o.insert("dash".into(), json!(s.style.token()));
        }
        if let Some(h) = s.hatch {
            o.insert("hatch".into(), json!(h.token()));
        }
        if !s.layers.is_empty() {
            o.insert("layers".into(), json!(s.layers));
        }
        // KiCad 10's line endings of a line, an arc or a curve, as a board shape has them (`pcb_items_json::line_ending_json`).
        if !matches!(s.shape, eda_model::ir::Shape::Rect { .. } | eda_model::ir::Shape::Circle { .. } | eda_model::ir::Shape::Polygon { .. }) {
            if !s.endings.start.is_none() {
                o.insert("start_ending".into(), crate::pcb_items_json::line_ending_json(&s.endings.start));
            }
            if !s.endings.end.is_none() {
                o.insert("end_ending".into(), crate::pcb_items_json::line_ending_json(&s.endings.end));
            }
        }
    }
    v
}

/// Put the footprint's `graphics` and `texts` into the part's JSON `p`: absent when it has none.
pub(crate) fn add_art(p: &mut Value, part: &Part, fp: &FootprintInstance, footprint: &Footprint) {
    let art = &footprint.art;
    if art.is_empty() {
        return;
    }
    if !art.shapes.is_empty() {
        p["graphics"] = Value::Array(art.shapes.iter().map(|s| graphic_json(&s.placed(fp))).collect());
    }
    if !art.texts.is_empty() {
        // `${FOOTPRINT_LIBRARY}:${FOOTPRINT_NAME}` of the link the board file gives the footprint (`footprint_lib_id`)
        let fpid = eda_model::footprint::footprint_lib_id(part);
        let vars = TextVars { reference: &part.reference, value: part.value.as_deref().unwrap_or(""), footprint: &fpid };
        p["texts"] = Value::Array(
            art.texts
                .iter()
                .enumerate()
                .filter_map(|(i, t)| {
                    // A hidden text (a MountingHole's `${REFERENCE}` on its fab layer) is in the file and not on the canvas.
                    if !art.texts[i].layout.visible {
                        return None;
                    }
                    let t = t.placed(fp, &vars);
                    (!t.text.is_empty()).then(|| {
                        let raw = art.texts[i].text.as_str();
                        let mut v = json!({
                            "id": format!("{}~t{i}", part.reference),
                            "text": t.text,
                            "x": t.at.x,
                            "y": t.at.y,
                            // KiCad's angle: counter-clockwise on the screen, absolute, millidegrees (`PCB_TEXT::GetTextAngle`).
                            "angle": t.angle,
                            "w": t.size.0,
                            "h": t.size.1,
                            "thickness": t.thickness,
                            "layer": t.layer,
                            "halign": t.halign,
                            "valign": t.valign,
                            "mirror": t.mirror,
                            "bold": t.bold,
                            "italic": t.italic,
                            "upright": t.keep_upright,
                            "knockout": t.knockout,
                        });
                        // The References and Values rows of the Appearance panel switch these two (`PCB_TEXT::ViewGetLOD`).
                        if raw == "${REFERENCE}" || raw == "${VALUE}" {
                            v["role"] = json!(if raw == "${REFERENCE}" { "reference" } else { "value" });
                        }
                        v
                    })
                })
                .collect(),
        );
    }
}

/// The part's body in board space, `(x0, y0, x1, y1)`: the box of its footprint's `F.Fab` drawings, the four corners of the local box through the pose and boxed
/// again. `None` when the footprint has no fab drawing or only a line of one (the caller then looks the footprint up in the libraries, or falls back to the
/// courtyard).
pub(crate) fn placed_body(fp: &FootprintInstance, footprint: &Footprint) -> Option<(Nm, Nm, Nm, Nm)> {
    let (x0, y0, x1, y1) = footprint.art.geometry_bbox_on("F.Fab")?;
    // A degenerate box (one fab line) is no body.
    if x1 - x0 <= 0 || y1 - y0 <= 0 {
        return None;
    }
    let corners: [Point; 4] = [to_board(fp, (x0, y0)), to_board(fp, (x1, y0)), to_board(fp, (x1, y1)), to_board(fp, (x0, y1))];
    Some((corners.iter().map(|c| c.x).min()?, corners.iter().map(|c| c.y).min()?, corners.iter().map(|c| c.x).max()?, corners.iter().map(|c| c.y).max()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::fp_art::{FpArt, FpText, Hatch};
    use eda_model::ir::{Shape, Side};

    fn p(x: Nm, y: Nm) -> Point {
        Point { x, y }
    }

    fn part() -> Part {
        Part { reference: "U1".into(), mpn: None, lcsc: None, value: Some("LM358".into()), package: None, footprint: Some("Package_SO:SOIC-8".into()), pins: vec![], body_nm: None, symbol: None, datasheet: None, edge: None }
    }

    fn footprint() -> Footprint {
        let mut art = FpArt::default();
        art.shapes.push(FpShape::new(Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: 120_000, filled: false, start: p(-1_000_000, -2_000_000), end: p(1_000_000, -2_000_000) }));
        art.shapes.push(FpShape { style: LineStyle::Dash, hatch: Some(Hatch::CrossHatch), ..FpShape::new(Shape::Rect { id: String::new(), layer: "F.Fab".into(), stroke_width: 100_000, filled: false, start: p(-1_500_000, -1_000_000), end: p(1_500_000, 1_000_000) }) });
        art.texts.push(FpText::new("${REFERENCE} ${VALUE}", p(0, 0), "F.Fab"));
        Footprint { name: "SOIC-8".into(), art, ..Default::default() }
    }

    fn pose(side: Side, rot: u32) -> FootprintInstance {
        FootprintInstance { id: "U1".into(), at: p(10_000_000, 20_000_000), rot, side, label: Default::default() }
    }

    #[test]
    fn a_part_carries_its_graphics_and_texts_in_board_space() {
        let mut v = json!({});
        add_art(&mut v, &part(), &pose(Side::Top, 90_000), &footprint());
        let g = v["graphics"].as_array().unwrap();
        assert_eq!(g.len(), 2);
        // turned a quarter clockwise on the screen about (10, 20) mm: (x, y) -> (10 - y, 20 + x)
        assert_eq!((g[0]["kind"].as_str(), g[0]["layer"].as_str()), (Some("segment"), Some("F.SilkS")));
        assert_eq!((g[0]["start"].clone(), g[0]["end"].clone()), (json!([12_000_000, 19_000_000]), json!([12_000_000, 21_000_000])));
        assert!(g[0].get("id").is_none() && g[0].get("filled").is_none() && g[0].get("dash").is_none(), "{}", g[0]);
        assert_eq!((g[1]["dash"].as_str(), g[1]["hatch"].as_str()), (Some("dash"), Some("cross_hatch")));
        let t = v["texts"].as_array().unwrap();
        assert_eq!(t[0]["text"], "U1 LM358");
        assert!(t[0].get("role").is_none(), "a text that says more than the variable is neither the reference nor the value");
        assert_eq!((t[0]["x"].clone(), t[0]["y"].clone(), t[0]["angle"].clone()), (json!(10_000_000), json!(20_000_000), json!(270_000)));
    }

    #[test]
    fn a_bottom_side_footprint_draws_the_other_sides_layers_and_mirrored() {
        let mut v = json!({});
        add_art(&mut v, &part(), &pose(Side::Bottom, 0), &footprint());
        let g = v["graphics"].as_array().unwrap();
        assert_eq!(g[0]["layer"], "B.SilkS");
        assert_eq!(g[0]["start"], json!([11_000_000, 18_000_000]));
        assert_eq!(v["texts"][0]["layer"], "B.Fab");
        assert_eq!(v["texts"][0]["mirror"], true);
    }

    #[test]
    fn a_footprint_without_art_adds_nothing_and_the_body_is_the_fab_box_through_the_pose() {
        let mut v = json!({"ref": "R1"});
        add_art(&mut v, &part(), &pose(Side::Top, 0), &Footprint::default());
        assert_eq!(v, json!({"ref": "R1"}));
        // the fab rectangle is 3 x 2 mm and 0.1 wide: the box does not include the stroke, as the library's body never did
        assert_eq!(placed_body(&pose(Side::Top, 0), &footprint()), Some((8_500_000, 19_000_000, 11_500_000, 21_000_000)));
        assert_eq!(placed_body(&pose(Side::Top, 90_000), &footprint()), Some((9_000_000, 18_500_000, 11_000_000, 21_500_000)));
        assert_eq!(placed_body(&pose(Side::Top, 0), &Footprint::default()), None);
    }

    /// A hidden text is in the file and not on the canvas.
    #[test]
    fn a_hidden_text_is_not_sent_to_the_canvas() {
        let mut fp = footprint();
        fp.art.texts.push({
            let mut t = FpText::new("hidden", p(0, 100_000), "F.Fab");
            t.layout.visible = false;
            t
        });
        let mut v = json!({});
        add_art(&mut v, &part(), &pose(Side::Top, 0), &fp);
        let texts = v["texts"].as_array().unwrap();
        assert_eq!(texts.len(), 1, "{texts:?}");
        assert_ne!(texts[0]["text"], "hidden");
    }
}
