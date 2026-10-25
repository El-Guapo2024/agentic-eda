//! What the studio's board state says about a footprint beyond its pose: its fields as laid out on the board (Reference, Value, user
//! fields), the attributes in effect, and every pad's own shape, hole and overrides. The web draws fields and pads from this and the
//! Footprint Properties and Pad Properties dialogs start from it; the model side is [`eda_model::fp_edit`].
//!
//! Positions and angles are on the board (what the canvas draws and a move works on); `lx`, `ly` and `langle` are the same in the
//! footprint's own frame, which is what an edit sends back.

use eda_model::footprint::{to_board, PlacedPad};
use eda_model::fp_edit::{default_reference_layout, default_value_layout, field_id, pad_id, FieldLayout, FootprintAttrs, FootprintKind, PadEdit, REFERENCE, VALUE};
use eda_model::ir::{Design, FootprintInstance, Point, Nm};
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

pub(crate) fn shape_name(s: PadShape) -> &'static str {
    match s {
        PadShape::Rect => "rect",
        PadShape::RoundRect => "round_rect",
        PadShape::Circle => "circle",
        PadShape::Oval => "oval",
        PadShape::Trapezoid => "trapezoid",
        PadShape::ChamferedRect => "chamfered_rect",
        PadShape::Custom => "custom",
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
fn copper_centre(pad: &Pad, offset: Option<Point>) -> (Nm, Nm) {
    let Some(o) = offset else { return pad.at };
    let rad = (pad.rot as f64) / 1000.0 * std::f64::consts::PI / 180.0;
    let (sin, cos) = rad.sin_cos();
    (pad.at.0 + Nm::from_raw_shim((o.x.raw_shim() as f64 * cos - o.y.raw_shim() as f64 * sin).round() as i64), pad.at.1 + Nm::from_raw_shim((o.x.raw_shim() as f64 * sin + o.y.raw_shim() as f64 * cos).round() as i64))
}

/// One pad of `fp` for the studio: where it is and how big (the keys the canvas has always drawn from), plus its own shape, type,
/// hole, rotation, corner ratio and the edit that was made to it. `x` and `y` are the copper's centre; `px` and `py` the pad's position
/// (where the hole is), which differ only for a pad with an offset. `placed` is the pad as the engine places it (board space); `pad` is
/// the engine's pad in the footprint's frame.
/// How far the outline the studio is sent for a pad of no simple shape may stray from its curves: the board's default `m_MaxError`
/// (`ARC_HIGH_DEF`, 0.005 mm, [`eda_model::outline::MAX_ERROR_NM`]). It was written `5` when the IR was micrometres, and read 5 nm after the move
/// to nanometres: a thousandth of KiCad's error, some thirty times the vertices on every round edge of a custom pad.
pub(crate) const PAD_OUTLINE_MAX_ERROR: Nm = eda_model::outline::MAX_ERROR_NM;

pub(crate) fn pad_json(design: &Design, model: &ConstraintModel, fp: &FootprintInstance, placed: &PlacedPad, pad: &Pad, nth: u32, net: Option<&str>, flashed: &eda_connectivity::Flashed) -> Value {
    let edit: Option<&PadEdit> = design.footprint_edit(&fp.id).and_then(|e| e.pad(&pad.number, nth));
    let flashed_here: Option<&Vec<String>> = flashed.pads.get(&eda_model::fp_edit::pad_id(&fp.id, &pad.number, nth));
    // The offset the pad has (the library's or the one an edit made: the engine's pad already holds the edit).
    let offset = pad.ext.offset.map(|(x, y)| Point { x, y }).filter(|o| o.x != Nm::ZERO || o.y != Nm::ZERO);
    let centre = if offset.is_some() { to_board(fp, copper_centre(pad, offset)) } else { placed.center };
    // A trapezoid, a chamfered rectangle or a custom pad is not given by its size: the studio draws and hit-tests the outline the DRC measures
    // (`eda_drc::pad_shape::pad_outline`, the contours of the copper in nanometres on the board, holes included; fill them even-odd).
    let outline: Option<Vec<Vec<[Nm; 2]>>> = pad.has_complex_outline().then(|| eda_drc::pad_shape::pad_outline(fp, pad, placed.center, PAD_OUTLINE_MAX_ERROR).into_iter().map(|c| c.into_iter().map(|p| [p.x, p.y]).collect()).collect());
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
        // A trapezoid's two deltas, a chamfered rectangle's ratio and corners, a custom pad's anchor, clearance option and basic shapes
        // (the shapes of `eda_model::footprint::PadPrimitive`, in the pad's own frame).
        "delta": pad.ext.trapezoid_delta.map(|(x, y)| json!([x, y])),
        "chamfer_ratio": (pad.shape == PadShape::ChamferedRect).then(|| pad.ext.chamfer_ratio.unwrap_or(0.2)),
        "chamfer_corners": (pad.shape == PadShape::ChamferedRect).then(|| serde_json::to_value(pad.ext.chamfer_corners).unwrap_or(Value::Null)),
        "custom": pad.ext.custom.as_ref().and_then(|c| serde_json::to_value(c).ok()),
        "outline": outline,
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
        // The padstack (`eda_model::padstack::PadStack`) as the model has it, for the dialog; and, for a stack whose copper differs from layer to layer, the copper of
        // each group of layers on the board (`stack_layers`: what the canvas and the 3D scene draw, as the router and the gates measure it).
        "stack": (!pad.ext.stack.is_default()).then(|| serde_json::to_value(&pad.ext.stack).ok()).flatten(),
        "stack_layers": (pad.has_stack() || flashed_here.is_some()).then(|| stack_layers_json(model, fp, pad, flashed_here)),
        // A plated through-hole pad that removes the layers nothing connects it on (`remove_unused_layers`): the copper layers it has copper on, what the
        // connectivity says (`eda_connectivity::flashed_layers`).
        "flashed": flashed_here,
    })
}

/// The copper shapes of a padstack pad on `fp`, each with the board's layers it is on: `{ layers, x, y, w, h, round, shape, ratio, outline }`, the keys of a pad's own
/// (`x` and `y` the copper's centre on the board).
fn stack_layers_json(model: &ConstraintModel, fp: &FootprintInstance, pad: &Pad, flashed: Option<&Vec<String>>) -> Vec<Value> {
    pad.shape_groups(fp.side, &model.board.layers)
        .into_iter()
        .filter_map(|mut g| {
            // The layers where nothing connects the pad and it removes unconnected layers have no copper (`PAD::FlashLayer`): a group left with none is not drawn.
            if let Some(kept) = flashed {
                g.layers.retain(|l| kept.contains(l));
                if g.layers.is_empty() {
                    return None;
                }
            }
            Some(g)
        })
        .map(|g| {
            let view = g.shape.lay_on(pad);
            let placed = eda_model::footprint::placed_pad_of(fp, &view);
            let offset = view.ext.offset.map(|(x, y)| Point { x, y }).filter(|o| o.x != Nm::ZERO || o.y != Nm::ZERO);
            let centre = if offset.is_some() { to_board(fp, copper_centre(&view, offset)) } else { placed.center };
            let outline: Option<Vec<Vec<[Nm; 2]>>> = view.has_complex_outline().then(|| eda_drc::pad_shape::pad_outline(fp, &view, placed.center, PAD_OUTLINE_MAX_ERROR).into_iter().map(|c| c.into_iter().map(|p| [p.x, p.y]).collect()).collect());
            json!({
                "layers": g.layers,
                "role": g.role,
                "x": centre.x,
                "y": centre.y,
                "w": placed.size.0,
                "h": placed.size.1,
                "round": placed.is_round(),
                "shape": shape_name(view.shape),
                "ratio": view.roundrect_ratio,
                "outline": outline,
            })
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::PAD_OUTLINE_MAX_ERROR;
    use eda_model::footprint::{Pad, PadAnchor, PadCustom, PadKind, PadPrimitive, PadShape, PadShapeExt};
    use eda_model::ir::{FootprintInstance, LabelSide, Point, Side};
    use eda_model::units::{mm, Nm};

    /// A custom pad's ring 1.8 mm out is drawn within KiCad's 5 µm, not within 5 nm (a thousand-odd vertices).
    #[test]
    fn a_custom_pads_outline_keeps_kicads_max_error() {
        assert_eq!(PAD_OUTLINE_MAX_ERROR, mm(0.005));
        let ring = PadPrimitive { shape: eda_model::ir::Shape::Circle { id: String::new(), layer: String::new(), stroke_width: mm(0.6), filled: false, center: Point::default(), end: Point { x: mm(1.5), y: Nm::ZERO } }, proxy: false };
        let ext = PadShapeExt { custom: Some(PadCustom { anchor: PadAnchor::Circle, zone: Default::default(), primitives: vec![ring] }), ..Default::default() };
        let pad = Pad { number: "1".into(), at: (Nm::ZERO, Nm::ZERO), size: (mm(0.5), mm(0.5)), shape: PadShape::Custom, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None, opposite_side: false, ext };
        let fp = FootprintInstance { id: "U1".into(), at: Point::default(), rot: 0, side: Side::Top, label: LabelSide::Above };
        let outline = eda_drc::pad_shape::pad_outline(&fp, &pad, Point::default(), PAD_OUTLINE_MAX_ERROR);
        let vertices: usize = outline.iter().map(Vec::len).sum();
        assert!((60..400).contains(&vertices), "{vertices} vertices");
    }
}
