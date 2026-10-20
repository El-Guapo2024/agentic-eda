//! A footprint's own silkscreen, mask and copper graphics and its texts as the flattened board holds them (`DrcBoard::mask.silk`, `mask.graphics`): from the footprint's
//! art ([`eda_model::fp_art`]) put where the footprint is, and from its fields (the Reference, the Value and the user fields of [`eda_model::fp_edit`]), so a board made here and a board
//! imported from KiCad are read the same way. Before, only an imported board had them (`FootprintExtra::graphics` and `texts`, board space, filled by the importer) and a board made here
//! had an approximation of its reference label.
//!
//! The text shapes are the ones `eda_drc::stroke_font` lays out (`EDA_TEXT::GetEffectiveTextShape`); a graphic is `PCB_SHAPE::GetEffectiveShape` ([`crate::board::effective_shapes`]).

use crate::board::{effective_shapes, footprint_text_shape, DrcGraphic, DrcSilk, MaskData};
use eda_model::fp_art::TextVars;
use eda_model::fp_edit::{effective_reference, effective_value, FieldLayout, REFERENCE, VALUE};
use eda_model::ir::{Design, FootprintExtra, FootprintInstance, FootprintText, Point};
use eda_model::{Footprint, Part};

/// The text of a field laid out on the board as the importer's board-space text the DRC shapes are made from. `shown` is what it reads.
fn field_text(l: &FieldLayout, shown: &str, fp: &FootprintInstance) -> FootprintText {
    FootprintText {
        text: shown.to_string(),
        layer: l.layer.clone(),
        at: l.board_position(fp),
        angle_file_mdeg: l.board_angle(fp),
        size: l.size,
        thickness: l.thickness,
        halign: l.halign,
        valign: l.valign,
        mirror: l.mirror,
        keep_upright: l.keep_upright,
        bold: l.bold,
    }
}

/// Whether this footprint's silkscreen has to be read from what the importer once kept in board space (`FootprintExtra::graphics`, `texts`): a design imported before footprints had
/// art and fields of their own has no art and no field edit for it.
fn legacy(design: &Design, fp: &FootprintInstance, footprint: &Footprint, extra: Option<&FootprintExtra>) -> bool {
    extra.is_some_and(|e| !e.texts.is_empty() || e.graphics.iter().any(|g| g.pad_number.is_none())) && footprint.art.is_empty() && design.footprint_edit(&fp.id).is_none()
}

/// Put the silkscreen items and the mask and silk graphics of the placed footprint `fp` into `mask`. `index` is the footprint's place in `mask.footprints`.
pub(crate) fn add_footprint_art(design: &Design, part: &Part, fp: &FootprintInstance, footprint: &Footprint, extra: Option<&FootprintExtra>, index: usize, mask: &mut MaskData) {
    if legacy(design, fp, footprint, extra) {
        if let Some(e) = extra {
            add_extra(fp, e, index, mask);
        }
        return;
    }
    mask.has_footprint_silk = true;
    let silk = |layer: &str| layer == "F.SilkS" || layer == "B.SilkS";

    // The fields: Reference, Value, then the user fields; a hidden one has no ink.
    let value = part.value.as_deref().unwrap_or(&fp.id);
    let edit = design.footprint_edit(&fp.id);
    let reference = effective_reference(design, fp, footprint);
    let value_layout = effective_value(design, fp);
    let mut fields: Vec<(&str, FieldLayout, String)> = vec![(REFERENCE, reference.clone(), reference.shown(&fp.id).to_string()), (VALUE, value_layout.clone(), value_layout.shown(value).to_string())];
    for f in edit.map(|e| e.fields.as_slice()).unwrap_or_default() {
        fields.push((f.name.as_str(), f.layout.clone(), f.text.clone()));
    }
    for (name, layout, shown) in fields {
        if !layout.visible || shown.is_empty() || !silk(&layout.layer) {
            continue;
        }
        if let Some(shape) = footprint_text_shape(&field_text(&layout, &shown, fp)) {
            let pos = layout.board_position(fp);
            mask.silk.push(DrcSilk { fp: Some(index), id: format!("{}.f{}", fp.id, mask.silk.len()), desc: format!("{name} of {}", fp.id), pos, layer: layout.layer.clone(), shapes: vec![shape], is_shape: false });
        }
    }

    // The footprint's own texts.
    let vars = TextVars { reference: &fp.id, value, footprint: part.footprint.as_deref().or(part.package.as_deref()).unwrap_or("") };
    for t in &footprint.art.texts {
        let placed = t.placed(fp, &vars);
        if placed.text.is_empty() || !t.layout.visible {
            continue;
        }
        let text = FootprintText { text: placed.text.clone(), layer: placed.layer.clone(), at: placed.at, angle_file_mdeg: placed.angle, size: placed.size, thickness: placed.thickness, halign: placed.halign, valign: placed.valign, mirror: placed.mirror, keep_upright: placed.keep_upright, bold: placed.bold };
        if !silk(&placed.layer) {
            continue;
        }
        if let Some(shape) = footprint_text_shape(&text) {
            mask.silk.push(DrcSilk { fp: Some(index), id: format!("{}.t{}", fp.id, mask.silk.len()), desc: format!("Footprint text '{}' of {}", placed.text, fp.id), pos: placed.at, layer: placed.layer, shapes: vec![shape], is_shape: false });
        }
    }

    // The footprint's graphics: silk (clearance, over copper) and mask (openings).
    for s in &footprint.art.shapes {
        let placed = s.placed(fp);
        let pos = placed.shape.points().first().copied().unwrap_or(Point { x: 0, y: 0 });
        for layer in placed.all_layers() {
            if silk(layer) {
                mask.silk.push(DrcSilk { fp: Some(index), id: format!("{}.s{}", fp.id, mask.silk.len()), desc: format!("Graphic on {layer} of {}", fp.id), pos, layer: layer.to_string(), shapes: effective_shapes(&placed.shape), is_shape: true });
            } else if layer == "F.Mask" || layer == "B.Mask" {
                mask.graphics.push(DrcGraphic {
                    fp: Some(index),
                    id: format!("{}.g{}", fp.id, mask.graphics.len()),
                    desc: format!("Graphic on {layer} of {}", fp.id),
                    pos,
                    layer: layer.to_string(),
                    shapes: effective_shapes(&placed.shape),
                    margin: placed.mask_margin,
                    filled: placed.shape.is_filled(),
                    width: placed.shape.stroke_width(),
                    pad: None,
                });
            }
        }
    }

    // A pad with a mask layer and no copper is a pad of the footprint to the mask check; the importer keeps it with the graphics it is not.
    if let Some(e) = extra {
        add_mask_pads(fp, e, index, mask);
    }
}

/// What the importer kept in board space for a design that has no art: the footprint's texts, its silk and mask graphics and its mask-only pads.
fn add_extra(fp: &FootprintInstance, e: &FootprintExtra, index: usize, mask: &mut MaskData) {
    mask.has_footprint_silk = true;
    for t in &e.texts {
        if let Some(shape) = footprint_text_shape(t) {
            mask.silk.push(DrcSilk { fp: Some(index), id: format!("{}.t{}", fp.id, mask.silk.len()), desc: format!("Footprint text '{}' of {}", t.text, fp.id), pos: t.at, layer: t.layer.clone(), shapes: vec![shape], is_shape: false });
        }
    }
    for g in e.graphics.iter().filter(|g| g.pad_number.is_none()) {
        let (layer, pos) = (g.shape.layer().to_string(), g.shape.points().first().copied().unwrap_or(Point { x: 0, y: 0 }));
        if layer.ends_with(".SilkS") {
            mask.silk.push(DrcSilk { fp: Some(index), id: format!("{}.s{}", fp.id, mask.silk.len()), desc: format!("Graphic on {layer} of {}", fp.id), pos, layer, shapes: effective_shapes(&g.shape), is_shape: true });
            continue;
        }
        mask.graphics.push(DrcGraphic { fp: Some(index), id: format!("{}.g{}", fp.id, mask.graphics.len()), desc: format!("Graphic on {layer} of {}", fp.id), pos, layer, shapes: effective_shapes(&g.shape), margin: g.solder_mask_margin, filled: g.shape.is_filled(), width: g.shape.stroke_width(), pad: None });
    }
    add_mask_pads(fp, e, index, mask);
}

/// The mask-only pads (`FootprintGraphic::pad_number`) of the importer's extras, as the mask providers' `PCB_PAD_T` apertures.
fn add_mask_pads(fp: &FootprintInstance, e: &FootprintExtra, index: usize, mask: &mut MaskData) {
    for g in e.graphics.iter().filter(|g| g.pad_number.is_some()) {
        let (layer, pos) = (g.shape.layer().to_string(), g.shape.points().first().copied().unwrap_or(Point { x: 0, y: 0 }));
        let Some(n) = &g.pad_number else { continue };
        let pad = crate::board::DrcGraphicPad { number: n.clone(), net: if g.net.is_empty() { None } else { Some(g.net.clone()) }, pin_type: g.pin_type.clone() };
        mask.graphics.push(DrcGraphic {
            fp: Some(index),
            id: format!("{}.g{}", fp.id, mask.graphics.len()),
            desc: format!("Pad {n} of {}", fp.id),
            pos,
            layer,
            shapes: effective_shapes(&g.shape),
            margin: g.solder_mask_margin,
            filled: g.shape.is_filled(),
            width: g.shape.stroke_width(),
            pad: Some(pad),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::fp_art::{FpArt, FpShape, FpText};
    use eda_model::ir::{PlacementSection, Provenance, Shape as IrShape, Side};
    use eda_model::ConstraintModel;

    fn p(x: i64, y: i64) -> Point {
        Point { x, y }
    }

    fn seg(layer: &str, a: Point, b: Point, w: i64) -> FpShape {
        FpShape::new(IrShape::Segment { id: String::new(), layer: layer.into(), stroke_width: w, filled: false, start: a, end: b })
    }

    /// One footprint at (10, 10) mm with a silk line, a fab line, a mask rectangle, a copper line, and a text on the silk and one on the fab layer.
    fn board(side: Side, rot: u32) -> (Design, ConstraintModel) {
        let mut art = FpArt::default();
        art.shapes.push(seg("F.SilkS", p(-1_000, -2_000), p(1_000, -2_000), 120));
        art.shapes.push(seg("F.Fab", p(-1_000, -1_000), p(1_000, -1_000), 100));
        art.shapes.push(FpShape { mask_margin: Some(50), ..FpShape::new(IrShape::Rect { id: String::new(), layer: "F.Mask".into(), stroke_width: 0, filled: true, start: p(-500, -500), end: p(500, 500) }) });
        art.shapes.push(seg("F.Cu", p(-2_000, 3_000), p(2_000, 3_000), 300));
        art.texts.push(FpText::new("${REFERENCE}", p(0, 0), "F.SilkS"));
        art.texts.push(FpText::new("fab note", p(0, 500), "F.Fab"));
        let footprint = Footprint { name: "T".into(), art, ..Default::default() };
        let part = Part { reference: "U1".into(), mpn: None, lcsc: None, value: Some("MCU".into()), package: None, footprint: Some("T".into()), pins: vec![], body_um: None, symbol: None, datasheet: None, edge: None };
        let model = ConstraintModel { parts: vec![part], footprints: vec![footprint], ..Default::default() };
        let design = Design {
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection { outline: vec![p(0, 0), p(30_000, 0), p(30_000, 30_000), p(0, 30_000)], footprints: vec![FootprintInstance { id: "U1".into(), at: p(10_000, 10_000), rot, side, label: Default::default() }], modules: Vec::new() }),
            routing: None,
            drawings: None,
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
        };
        (design, model)
    }

    #[test]
    fn a_footprints_art_reaches_the_silk_and_mask_lists_where_it_is() {
        let (design, model) = board(Side::Top, 0);
        let b = crate::board::build(&design, &model);
        // two silk shapes/texts of the art, plus the Reference and Value fields of the footprint (the Value is on the fab layer: not silk)
        let silk: Vec<(&str, bool)> = b.mask.silk.iter().map(|s| (s.layer.as_str(), s.is_shape)).collect();
        assert_eq!(silk.iter().filter(|(_, shape)| *shape).count(), 1, "one silk graphic: {silk:?}");
        assert_eq!(silk.iter().filter(|(l, shape)| *l == "F.SilkS" && !*shape).count(), 2, "its ${{REFERENCE}} and the Reference field: {silk:?}");
        assert!(b.mask.silk.iter().all(|s| s.layer == "F.SilkS"), "the fab line and the fab text are not silk");
        let line = b.mask.silk.iter().find(|s| s.is_shape).expect("the silk line");
        assert_eq!(line.pos, p(9_000, 8_000), "its first point, board space");
        let text = b.mask.silk.iter().find(|s| s.desc.starts_with("Footprint text")).expect("the text");
        assert!(text.desc.contains("'U1'"), "the variable is resolved: {}", text.desc);
        assert!(b.mask.has_footprint_silk);
        // the mask rectangle
        assert_eq!(b.mask.graphics.len(), 1);
        assert_eq!((b.mask.graphics[0].layer.as_str(), b.mask.graphics[0].margin, b.mask.graphics[0].filled), ("F.Mask", Some(50), true));
    }

    #[test]
    fn a_bottom_side_footprint_has_its_art_on_the_other_sides_layers() {
        let (design, model) = board(Side::Bottom, 0);
        let b = crate::board::build(&design, &model);
        assert!(b.mask.silk.iter().all(|s| s.layer == "B.SilkS"), "{:?}", b.mask.silk.iter().map(|s| &s.layer).collect::<Vec<_>>());
        assert_eq!(b.mask.graphics[0].layer, "B.Mask");
        // x is mirrored: the line (-1 .. 1, -2) mm is at x = 11 .. 9 mm
        let line = b.mask.silk.iter().find(|s| s.is_shape).expect("the silk line");
        assert_eq!(line.pos, p(11_000, 8_000));
    }

    #[test]
    fn a_design_imported_before_art_keeps_its_board_space_silk() {
        let (mut design, mut model) = board(Side::Top, 0);
        model.footprints[0].art = FpArt::default();
        let mut extra = FootprintExtra { id: "U1".into(), ..Default::default() };
        extra.graphics.push(eda_model::ir::FootprintGraphic { shape: IrShape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: 120, filled: false, start: p(1, 1), end: p(2, 2) }, solder_mask_margin: None, pad_number: None, net: String::new(), pin_type: String::new() });
        extra.graphics.push(eda_model::ir::FootprintGraphic { shape: IrShape::Circle { id: String::new(), layer: "F.Mask".into(), stroke_width: 0, filled: true, center: p(5, 5), end: p(105, 5) }, solder_mask_margin: None, pad_number: Some("9".into()), net: "N".into(), pin_type: "free".into() });
        design.drawings = Some(eda_model::ir::DrawingsSection { footprint_extras: vec![extra], ..Default::default() });
        let b = crate::board::build(&design, &model);
        assert_eq!(b.mask.silk.len(), 1, "the one silk line the importer kept");
        assert_eq!(b.mask.graphics.len(), 1);
        assert_eq!(b.mask.graphics[0].pad.as_ref().map(|q| q.number.as_str()), Some("9"), "and the mask-only pad");
    }

    #[test]
    fn a_footprint_with_art_ignores_what_an_older_import_left_in_board_space_but_keeps_its_mask_pads() {
        let (mut design, model) = board(Side::Top, 0);
        let mut extra = FootprintExtra { id: "U1".into(), ..Default::default() };
        extra.graphics.push(eda_model::ir::FootprintGraphic { shape: IrShape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: 120, filled: false, start: p(1, 1), end: p(2, 2) }, solder_mask_margin: None, pad_number: None, net: String::new(), pin_type: String::new() });
        extra.graphics.push(eda_model::ir::FootprintGraphic { shape: IrShape::Circle { id: String::new(), layer: "F.Mask".into(), stroke_width: 0, filled: true, center: p(5, 5), end: p(105, 5) }, solder_mask_margin: None, pad_number: Some("9".into()), net: String::new(), pin_type: String::new() });
        design.drawings = Some(eda_model::ir::DrawingsSection { footprint_extras: vec![extra], ..Default::default() });
        let b = crate::board::build(&design, &model);
        assert_eq!(b.mask.silk.iter().filter(|s| s.is_shape).count(), 1, "the art's line, not it and the extra's");
        assert_eq!(b.mask.graphics.len(), 2, "the art's mask rectangle and the mask-only pad");
    }

    /// A hidden text of the footprint (a MountingHole's `${REFERENCE}` on its fab layer, or one on the silkscreen) has no ink: the silkscreen checks do not see it.
    #[test]
    fn a_hidden_text_has_no_silk() {
        let (design, mut model) = board(Side::Top, 0);
        let texts = &mut model.footprints[0].art.texts;
        texts[0].layout.visible = false;
        let b = crate::board::build(&design, &model);
        assert!(b.mask.silk.iter().all(|s| !s.desc.starts_with("Footprint text")), "{:?}", b.mask.silk.iter().map(|s| &s.desc).collect::<Vec<_>>());
    }
}
