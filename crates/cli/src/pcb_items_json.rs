//! The board items of `eda_model::pcb_items` as the studio's state sends them (`GET /api/state`, `drawings`): the display shape of
//! `web/studio/src/api/types.ts` (`BoardTextBox`, ..), points as `[x, y]` pairs and the text with its `${VARIABLES}` expanded as `shown` where that differs.

use eda_model::ir::{DrawingsSection, TextJustify, TextVAlign};
use eda_model::image_info::{content_hash, DEFAULT_PPI};
use eda_model::pcb_items::{PcbPoint, RefImage, TextBox};
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

/// One reference image as the canvas and the Properties panel read it: its centre, its size on the board (`w`, `h`), its scale, its size in pixels and its definition. The
/// pixels are not in the state: `hash` is the part of `GET /api/pcb_image` that changes when they do.
pub(crate) fn image_json(i: &RefImage) -> Value {
    let header = i.header();
    let (w, h) = i.size_um();
    json!({
        "id": i.id,
        "layer": i.layer,
        "x": i.at.x,
        "y": i.at.y,
        "w": w,
        "h": h,
        "scale_ppm": i.scale_ppm,
        "px": [header.map_or(0, |h| h.width), header.map_or(0, |h| h.height)],
        "ppi": header.map_or(DEFAULT_PPI, |h| h.ppi),
        "hash": content_hash(&i.data),
    })
}

/// One point as the canvas and the Properties panel read it: its centre and the size of its mark.
pub(crate) fn point_json(p: &PcbPoint) -> Value {
    json!({ "id": p.id, "layer": p.layer, "x": p.at.x, "y": p.at.y, "size": p.size_um })
}

/// Add the new item kinds to the `drawings` object of the state, and the hatch of the shapes `drawings["shapes"]` already lists.
pub(crate) fn extend(drawings: &mut Value, design: &eda_model::ir::Design, model: &eda_model::ConstraintModel, d: &DrawingsSection, tv: &PcbShown) {
    drawings["text_boxes"] = Value::Array(d.board_items.text_boxes.iter().map(|t| text_box_json(t, tv)).collect());
    drawings["images"] = Value::Array(d.board_items.images.iter().map(image_json).collect());
    drawings["points"] = Value::Array(d.board_items.points.iter().map(point_json).collect());
    drawings["barcodes"] = Value::Array(d.barcodes.iter().map(|b| barcode_json(b, tv)).collect());
    hatch_json(drawings, design, model, d);
}

/// One barcode as the canvas, the dialog and the Properties panel read it. Its shape is not in the state: the canvas asks for it (`GET /api/barcode_shape?id=..`).
pub(crate) fn barcode_json(b: &eda_model::barcode::Barcode, tv: &PcbShown) -> Value {
    let shown = tv.text_shown(&b.text, &b.layer);
    let mut v = json!({
        "id": b.id,
        "layer": b.layer,
        "x": b.at.x,
        "y": b.at.y,
        "angle": b.angle,
        "w": b.width_um,
        "h": b.height_um,
        "text": b.text,
        "text_height": b.text_height_um,
        "kind": b.kind.file_name(),
        "ecc": b.ecc.file_name(),
        "show_text": b.show_text,
        "knockout": b.knockout,
        "margin_x": b.margin_x_um,
        "margin_y": b.margin_y_um,
    });
    if shown != b.text {
        v["shown"] = json!(shown);
    }
    v
}

/// A hatched shape says how (`fill`: `hatch`, `reverse_hatch` or `cross_hatch`; `filled` stays false) and carries the lines `EDA_SHAPE::UpdateHatching` makes of it from the board as it
/// is now (`hatch_lines`, `[x1, y1, x2, y2]` micrometres) and the width to draw them with (`hatch_width`). KiCad does this again after every commit (`PCB_ACTIONS::rehatchShapes`), and
/// so does the state, whenever it is described.
fn hatch_json(drawings: &mut Value, design: &eda_model::ir::Design, model: &eda_model::ConstraintModel, d: &DrawingsSection) {
    if d.board_items.shape_styles.is_empty() {
        return;
    }
    let Some(Value::Array(shapes)) = drawings.get_mut("shapes") else { return };
    for (json, shape) in shapes.iter_mut().zip(&d.shapes) {
        let Some(kind) = d.board_items.hatch_of(shape.id()).filter(|_| !shape.is_filled()) else { continue };
        let hatching = eda_drc::hatch::hatching(design, Some(model), shape, kind);
        json["fill"] = json!(kind.token());
        json["hatch_width"] = json!(hatching.width);
        json["hatch_lines"] = Value::Array(hatching.lines.iter().map(|(a, b)| json!([a.x, a.y, b.x, b.y])).collect());
    }
}
