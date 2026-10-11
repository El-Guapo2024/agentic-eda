//! What the studio's board state says about a footprint beyond its pose: its fields as laid out on the board (Reference, Value, user
//! fields), the attributes in effect, and every pad's own shape, hole and overrides. The web draws fields and pads from this and the
//! Footprint Properties and Pad Properties dialogs start from it; the model side is [`eda_model::fp_edit`].
//!
//! Positions and angles are on the board (what the canvas draws and a move works on); `lx`, `ly` and `langle` are the same in the
//! footprint's own frame, which is what an edit sends back.

use eda_model::footprint::{to_board, PlacedPad};
use eda_model::fp_edit::{default_reference_layout, default_value_layout, field_id, pad_id, FieldLayout, FootprintAttrs, FootprintKind, PadEdit, REFERENCE, VALUE};
use eda_model::ir::{Design, FootprintInstance, Point, Um};
use eda_model::{ConstraintModel, Footprint, Pad, PadKind, PadShape, Part};
use serde_json::{json, Value};

fn field_json(id: String, name: &str, text: &str, l: &FieldLayout, fp: &FootprintInstance, custom: bool) -> Value {
    let at = l.board_position(fp);
    json!({
        "id": id,
        "name": name,
        "text": text,
        "x": at.x,
        "y": at.y,
        // KiCad's angle: counter-clockwise on the screen, millidegrees, absolute.
        "angle": l.board_angle(fp),
        "w": l.size.0,
        "h": l.size.1,
        "thickness": l.thickness,
        "layer": l.layer,
        "visible": l.visible,
        "halign": l.halign,
        "valign": l.valign,
        "mirror": l.mirror,
        "bold": l.bold,
        "italic": l.italic,
        "upright": l.keep_upright,
        "knockout": l.knockout,
        "lx": l.at.x,
        "ly": l.at.y,
        "langle": l.angle,
        // True once the layout is stored in the design; false for the default placement.
        "custom": custom,
    })
}

/// The Reference, the Value and the user fields of the footprint `fp`, in the order the dialog's grid lists them.
pub(crate) fn fields_json(design: &Design, part: &Part, fp: &FootprintInstance, footprint: &Footprint) -> Vec<Value> {
    let edit = design.footprint_edit(&fp.id);
    let mut out = Vec::new();
    let reference = edit.and_then(|e| e.reference.clone());
    let l = reference.clone().unwrap_or_else(|| default_reference_layout(fp, footprint));
    out.push(field_json(field_id(&fp.id, REFERENCE), REFERENCE, l.shown(&fp.id), &l, fp, reference.is_some()));
    let value = edit.and_then(|e| e.value.clone());
    let l = value.clone().unwrap_or_else(|| default_value_layout(fp));
    out.push(field_json(field_id(&fp.id, VALUE), VALUE, l.shown(part.value.as_deref().unwrap_or(&fp.id)), &l, fp, value.is_some()));
    for f in edit.map(|e| e.fields.as_slice()).unwrap_or_default() {
        out.push(field_json(field_id(&fp.id, &f.name), &f.name, &f.text, &f.layout, fp, true));
    }
    out
}

/// The attributes in effect: the edit's, else derived from the pads and the schematic symbol.
pub(crate) fn attrs_json(design: &Design, fp: &FootprintInstance, footprint: &Footprint) -> Value {
    let a: FootprintAttrs = design.effective_attrs(&fp.id, footprint);
    json!({
        "kind": match a.kind { FootprintKind::Unspecified => "unspecified", FootprintKind::Smd => "smd", FootprintKind::ThroughHole => "through_hole" },
        "board_only": a.board_only,
        "exclude_from_pos_files": a.exclude_from_pos_files,
        "exclude_from_bom": a.exclude_from_bom,
        "dnp": a.dnp,
        "allow_missing_courtyard": a.allow_missing_courtyard,
        "custom": design.footprint_edit(&fp.id).is_some_and(|e| e.attrs.is_some()),
    })
}

fn shape_name(s: PadShape) -> &'static str {
    match s {
        PadShape::Rect => "rect",
        PadShape::RoundRect => "round_rect",
        PadShape::Circle => "circle",
        PadShape::Oval => "oval",
    }
}

fn kind_name(k: PadKind) -> &'static str {
    match k {
        PadKind::Smd => "smd",
        PadKind::ThroughHole => "through_hole",
        PadKind::NonPlatedHole => "non_plated_hole",
    }
}

/// The copper's centre in the footprint's frame: the pad's position moved by its shape offset, which is in the pad's own (turned) frame.
fn copper_centre(pad: &Pad, offset: Option<Point>) -> (Um, Um) {
    let Some(o) = offset else { return pad.at };
    let rad = (pad.rot as f64) / 1000.0 * std::f64::consts::PI / 180.0;
    let (sin, cos) = rad.sin_cos();
    (pad.at.0 + (o.x as f64 * cos - o.y as f64 * sin).round() as Um, pad.at.1 + (o.x as f64 * sin + o.y as f64 * cos).round() as Um)
}

/// One pad of `fp` for the studio: where it is and how big (the keys the canvas has always drawn from), plus its own shape, type,
/// hole, rotation, corner ratio and the edit that was made to it. `x` and `y` are the copper's centre; `px` and `py` the pad's position
/// (where the hole is), which differ only for a pad with an offset. `placed` is the pad as the engine places it (board space); `pad` is
/// the engine's pad in the footprint's frame.
pub(crate) fn pad_json(design: &Design, model: &ConstraintModel, fp: &FootprintInstance, placed: &PlacedPad, pad: &Pad, nth: u32, net: Option<&str>) -> Value {
    let _ = model;
    let edit: Option<&PadEdit> = design.footprint_edit(&fp.id).and_then(|e| e.pad(&pad.number, nth));
    let offset = edit.and_then(|e| e.offset).filter(|o| o.x != 0 || o.y != 0);
    let centre = if offset.is_some() { to_board(fp, copper_centre(pad, offset)) } else { placed.center };
    json!({
        "num": placed.number,
        "net": net,
        "x": centre.x,
        "y": centre.y,
        "w": placed.size.0,
        "h": placed.size.1,
        "round": placed.is_round(),
        "th": placed.through_hole,
        "id": pad_id(&fp.id, &pad.number, nth),
        "shape": shape_name(pad.shape),
        "kind": kind_name(pad.kind),
        // Un-rotated size, the pad's own rotation (clockwise degrees, relative to the footprint), the corner ratio and the hole.
        "size": [pad.size.0, pad.size.1],
        "rot": pad.rot as f64 / 1000.0,
        "ratio": pad.roundrect_ratio,
        "drill": pad.drill,
        "slot": pad.drill_slot.map(|(w, h)| json!([w, h])),
        "px": placed.center.x,
        "py": placed.center.y,
        "offset": offset.map(|o| json!([o.x, o.y])),
        "mask_margin": edit.and_then(|e| e.solder_mask_margin),
        "paste_margin": edit.and_then(|e| e.solder_paste_margin),
        "paste_ratio": edit.and_then(|e| e.solder_paste_margin_ratio),
        "edited": edit.is_some(),
        // The edit as stored, so the dialog sends back the overrides that are already there along with the one it changes.
        "edit": edit.and_then(|e| serde_json::to_value(e).ok()),
    })
}

/// The pads of `footprint` in the order the studio lists them (sorted by number, ties in the footprint's own order), each with the
/// `nth` that tells it from another pad of the same number.
pub(crate) fn pads_in_studio_order(footprint: &Footprint) -> Vec<(&Pad, u32)> {
    let mut pads: Vec<&Pad> = footprint.pads.iter().collect();
    pads.sort_by(|a, b| a.number.cmp(&b.number));
    let mut seen: std::collections::BTreeMap<&str, u32> = std::collections::BTreeMap::new();
    pads.into_iter()
        .map(|p| {
            let n = seen.entry(p.number.as_str()).or_insert(0);
            *n += 1;
            (p, *n)
        })
        .collect()
}
