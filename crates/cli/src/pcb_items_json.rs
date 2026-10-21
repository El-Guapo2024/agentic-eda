//! The board items of `eda_model::pcb_items` as the studio's state sends them (`GET /api/state`, `drawings`): the display shape of
//! `web/studio/src/api/types.ts` (`BoardTextBox`, ..), points as `[x, y]` pairs and the text with its `${VARIABLES}` expanded as `shown` where that differs.

use eda_model::ir::{DrawingsSection, TextJustify, TextVAlign};
use eda_model::pcb_items::TextBox;
use serde_json::{json, Value};

use crate::text_vars_json::PcbShown;

fn pt(p: eda_model::ir::Point) -> Value {
    json!([p.x, p.y])
}

fn halign(j: TextJustify) -> &'static str {
    match j {
        TextJustify::Left => "left",
        TextJustify::Center => "center",
        TextJustify::Right => "right",
    }
}

fn valign(v: TextVAlign) -> &'static str {
    match v {
        TextVAlign::Top => "top",
        TextVAlign::Center => "center",
        TextVAlign::Bottom => "bottom",
    }
}

/// One text box as the canvas, the dialog and the Properties panel read it.
pub(crate) fn text_box_json(t: &TextBox, tv: &PcbShown) -> Value {
    let shown = tv.text_shown(&t.text, &t.layer);
    let mut v = json!({
        "id": t.id,
        "layer": t.layer,
        "text": t.text,
        "start": pt(t.start),
        "end": pt(t.end),
        "pts": t.pts.iter().map(|p| pt(*p)).collect::<Vec<_>>(),
        "angle": t.angle,
        "margins": [t.margin_left_um, t.margin_top_um, t.margin_right_um, t.margin_bottom_um],
        "border": t.border,
        "stroke_width": t.stroke_width_um,
        "line_style": t.line_style.token(),
        "stroke_color": t.stroke_color,
        "knockout": t.knockout,
        "size": t.size_um,
        "width": t.text_width_um(),
        "thickness": t.effective_pen_um(),
        "thickness_auto": t.thickness_um == 0,
        "halign": halign(t.halign),
        "valign": valign(t.valign),
        "mirror": t.mirror,
        "bold": t.bold,
        "italic": t.italic,
        "line_spacing_permille": if t.line_spacing_permille == 0 { 1000 } else { t.line_spacing_permille },
        "face": t.face,
    });
    if shown != t.text {
        v["shown"] = json!(shown);
    }
    v
}

/// Add the new item kinds to the `drawings` object of the state.
pub(crate) fn extend(drawings: &mut Value, d: &DrawingsSection, tv: &PcbShown) {
    drawings["text_boxes"] = Value::Array(d.board_items.text_boxes.iter().map(|t| text_box_json(t, tv)).collect());
}
