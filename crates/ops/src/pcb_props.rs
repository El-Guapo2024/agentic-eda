//! The verbs the Properties panel needs where no existing verb fits (`pcbnew/widgets/pcb_properties_panel.cpp` at KiCad 8303b2ad).
//!
//! The panel is a property grid (`PROPERTIES_PANEL`): one edit of one row sets one property on every selected item, and
//! `PCB_PROPERTIES_PANEL::valueChanged` pushes the whole thing as ONE `BOARD_COMMIT` ("Edit Properties"). The studio builds that
//! as a single `Cmd::Batch`, out of the verbs that already existed (`move_items`, `rotate_items`, `flip_items`, `set_locked`,
//! `set_track_width`, `edit_tracks_and_vias`, `edit_via`, `edit_zone`, `edit_shape`, `edit_text`, `edit_dimension`, `edit_group`).
//! Four properties had none, and each gets the smallest verb that does it:
//!
//! * `PCB_TRACK::SetStartX/Y`, `SetEndX/Y` ("Start X", "End Y" ...): [`Cmd::EditTrack`](crate::Cmd::EditTrack);
//! * `BOARD_CONNECTED_ITEM::SetNetCode` ("Net"): [`Cmd::SetItemNet`](crate::Cmd::SetItemNet), for tracks, vias and zones;
//! * `ZONE::SetZoneName` ("Name"): [`Cmd::SetZoneName`](crate::Cmd::SetZoneName);
//! * `EDA_SHAPE::SetStartX/SetEndX/SetCenterX/SetRadius/SetRectangleWidth` ("Start X", "Radius", "Width" ...):
//!   [`Cmd::ReplaceShape`](crate::Cmd::ReplaceShape) -- the shape with its new geometry, in place, keeping its id and its lock (the
//!   studio plans the geometry the way those setters do, `kicad-port/pcbProperties.ts`).
//!
//! Five more came with the rows the panel lacked (UI-LEFT P2, `task-u1-props`), each over an overlay the board reader and writer already keep:
//!
//! * `EDA_TEXT::SetItalic`, `SetBold`, `SetVertJustify`, `SetTextWidth`, `SetAutoThickness` and `BOARD_ITEM::SetIsKnockout` of a board text:
//!   [`Cmd::EditTextStyle`](crate::Cmd::EditTextStyle), over the text's `TextExtra`;
//! * `PCB_TRACK::SetHasSolderMask` / `SetLocalSolderMaskMargin` and the same of a `PCB_SHAPE` ("Soldermask", "Soldermask Margin Override"):
//!   [`Cmd::SetSolderMask`](crate::Cmd::SetSolderMask), over `track_masks` and the shapes' styles;
//! * `EDA_SHAPE::SetCornerRadius` ("Corner Radius"): [`Cmd::SetCornerRadius`](crate::Cmd::SetCornerRadius);
//! * a via's tenting, covering, plugging, capping, filling, backdrill and post-machining (`TRACK_VIA_DESC`): [`Cmd::SetViaProps`](crate::Cmd::SetViaProps),
//!   over the via's tokens ([`eda_model::via_extra::ViaProps`]);
//! * `FOOTPRINT::SetLocalSolderPasteMargin` / `SetLocalSolderPasteMarginRatio`: [`Cmd::SetFootprintPasteMargin`](crate::Cmd::SetFootprintPasteMargin), over
//!   the footprint's tokens of a board read from a file.

use eda_model::units::Nm;
use super::Board;
use eda_model::board_text::TextExtra;
use eda_model::ir::{tessellate_arc, Point, Shape, TextVAlign, Track, TRACK_ARC_SEGMENTS};
use eda_model::via_extra::{ViaExtra, ViaProps};
use eda_model::CheckResult;
use std::collections::BTreeSet;

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

impl Board<'_> {
    /// `Cmd::EditTrack`: move the first and/or the last point of a track. A field left out stays where it is. A KiCad arc
    /// (`PCB_ARC`: start, mid, end) keeps its mid point and is drawn through the new end points, as `PCB_ARC::SetStart`/`SetEnd`
    /// do; any other track is a polyline whose first or last vertex moves.
    pub(crate) fn edit_track(&mut self, id: &str, start: Option<Point>, end: Option<Point>) -> Result<(), Vec<CheckResult>> {
        if start.is_none() && end.is_none() {
            return Err(fail("ops_bad_track", id, "nothing to change: give a new start, a new end, or both"));
        }
        let rt = self.design.routing.as_mut().ok_or_else(|| fail("ops_unknown_track", id, "the board has no routing yet"))?;
        let t = rt.tracks.iter_mut().find(|t| t.id == id).ok_or_else(|| fail("ops_unknown_track", id, "no track with this id"))?;
        let moved = moved_ends(t, start, end);
        if moved.pts.len() == 2 && moved.pts[0] == moved.pts[1] {
            return Err(fail("ops_bad_track", id, "a track needs two different end points"));
        }
        t.pts = moved.pts;
        t.arc_mid_offset = moved.arc_mid_offset;
        self.sort_routing();
        Ok(())
    }

    /// `Cmd::SetItemNet`: `BOARD_CONNECTED_ITEM::SetNetCode` on tracks, vias and zones. The net must exist in the model; only a zone may be given
    /// no net at all ("" -- a rule area has none), as `add_zone` allows.
    pub(crate) fn set_item_net(&mut self, ids: &[String], net: &str) -> Result<(), Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(fail("ops_bad_net", "set_item_net", "no items given"));
        }
        if !net.is_empty() {
            self.known_net(net)?;
        }
        let (tracks, vias, zones): (BTreeSet<&str>, BTreeSet<&str>, BTreeSet<&str>) = match self.design.routing.as_ref() {
            Some(rt) => (
                rt.tracks.iter().map(|t| t.id.as_str()).collect(),
                rt.vias.iter().map(|v| v.id.as_str()).collect(),
                rt.zones.iter().map(|z| z.id.as_str()).collect(),
            ),
            None => Default::default(),
        };
        for id in ids {
            if !tracks.contains(id.as_str()) && !vias.contains(id.as_str()) && !zones.contains(id.as_str()) {
                return Err(fail("ops_unknown_item", id, "no track, via or zone with this id"));
            }
            if net.is_empty() && !zones.contains(id.as_str()) {
                return Err(fail("ops_bad_net", id, "a track or a via needs a net"));
            }
        }
        let wanted: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
        let rt = self.design.routing.as_mut().expect("the ids were found in the routing");
        for t in rt.tracks.iter_mut().filter(|t| wanted.contains(t.id.as_str())) {
            t.net = net.to_string();
        }
        for v in rt.vias.iter_mut().filter(|v| wanted.contains(v.id.as_str())) {
            v.net = net.to_string();
        }
        for z in rt.zones.iter_mut().filter(|z| wanted.contains(z.id.as_str())) {
            z.net = net.to_string();
        }
        self.sort_routing();
        Ok(())
    }

    /// `Cmd::SetZoneName`: `ZONE::SetZoneName` (the label the Zone Manager shows). Empty is KiCad's own "unnamed".
    pub(crate) fn set_zone_name(&mut self, id: &str, name: &str) -> Result<(), Vec<CheckResult>> {
        let rt = self.design.routing.as_mut().ok_or_else(|| fail("ops_unknown_zone", id, "the board has no routing yet"))?;
        let z = rt.zones.iter_mut().find(|z| z.id == id).ok_or_else(|| fail("ops_unknown_zone", id, "no zone with this id"))?;
        z.name = name.trim().to_string();
        Ok(())
    }

    /// `Cmd::ReplaceShape`: the drawn shape `id` becomes `shape`, in place -- its id, its place in the drawing order and its lock stay.
    /// Refused for what `add_shape` refuses (no layer, a polygon under three points) and for a width below zero or a circle without a radius.
    pub(crate) fn replace_shape(&mut self, id: &str, mut shape: Shape) -> Result<(), Vec<CheckResult>> {
        if shape.layer().is_empty() {
            return Err(fail("ops_bad_shape", id, "a shape needs a layer"));
        }
        if shape.stroke_width() < Nm::ZERO {
            return Err(fail("ops_bad_shape", id, "line width cannot be negative"));
        }
        match &shape {
            Shape::Polygon { pts, .. } if pts.len() < 3 => return Err(fail("ops_bad_shape", id, "a polygon needs at least three points")),
            Shape::Circle { center, end, .. } if center == end => return Err(fail("ops_bad_shape", id, "a circle needs a radius")),
            _ => {}
        }
        let dr = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_shape", id, "the board has no drawings yet"))?;
        let slot = dr.shapes.iter_mut().find(|s| s.id() == id).ok_or_else(|| fail("ops_unknown_shape", id, "no shape with this id"))?;
        shape.set_id(id.to_string());
        let solid = shape.is_filled();
        *slot = shape;
        // A solid fill takes the place of a hatch.
        if solid {
            dr.board_items.set_hatch(id, None);
        }
        Ok(())
    }
}

/// What [`Cmd::EditTextStyle`](crate::Cmd::EditTextStyle) sets of a board text; `None` leaves it as it is.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TextStyleEdit {
    pub width_nm: Option<Nm>,
    pub valign: Option<TextVAlign>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub knockout: Option<bool>,
    pub auto_thickness: Option<bool>,
}

/// `IsExternalCopperLayer`: the front or the back copper.
fn is_outer_copper(layer: &str) -> bool {
    layer == "F.Cu" || layer == "B.Cu"
}

impl Board<'_> {
    /// `Cmd::EditTextStyle`: the board text `id`'s glyph width (`SetTextWidth`; the height is `Text::size_nm`, a width equal to it keeps no entry), vertical
    /// justification, bold, italic, knockout and automatic stroke width (`SetAutoThickness`: the file then has no `(thickness ..)`, and the text is drawn with
    /// `GetPenSizeForNormal`, or `GetPenSizeForBold` for a bold one, of its smaller size -- which `Text::stroke_width` takes). Refused when nothing changes.
    pub(crate) fn edit_text_style(&mut self, id: &str, edit: &TextStyleEdit) -> Result<(), Vec<CheckResult>> {
        let d = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_text", id, "the board has no drawings yet"))?;
        let ti = d.texts.iter().position(|t| t.id == id).ok_or_else(|| fail("ops_unknown_text", id, "no text with this id"))?;
        if edit.width_nm.is_some_and(|w| w <= Nm::ZERO) {
            return Err(fail("ops_bad_text", id, "a text's width must be greater than zero"));
        }
        let before = d.text_extra(id).cloned().unwrap_or(TextExtra { id: id.to_string(), ..Default::default() });
        let mut extra = before.clone();
        let height = d.texts[ti].size_nm;
        if let Some(w) = edit.width_nm {
            extra.width_nm = (w != height).then_some(w);
        }
        if let Some(v) = edit.valign {
            extra.valign = v;
        }
        if let Some(b) = edit.bold {
            extra.bold = b;
        }
        if let Some(i) = edit.italic {
            extra.italic = i;
        }
        if let Some(k) = edit.knockout {
            extra.knockout = k;
        }
        if let Some(a) = edit.auto_thickness {
            extra.auto_thickness = a;
        }
        let stroke_before = d.texts[ti].stroke_width;
        if extra.auto_thickness {
            // `EDA_TEXT::GetEffectiveTextPenWidth` with no thickness: the pen of the smaller of the glyph's two sizes.
            let size = extra.width_nm.unwrap_or(height).min(height);
            d.texts[ti].stroke_width = eda_model::pcb_items::auto_pen_nm(size, extra.bold);
        }
        if extra == before && d.texts[ti].stroke_width == stroke_before {
            return Err(fail("ops_nothing_to_change", id, "the text already has this style"));
        }
        match (d.text_extras.iter().position(|e| e.id == id), extra.is_default()) {
            (Some(i), true) => {
                d.text_extras.remove(i);
            }
            (Some(i), false) => d.text_extras[i] = extra,
            (None, false) => d.text_extras.push(extra),
            (None, true) => {}
        }
        Ok(())
    }

    /// `Cmd::SetSolderMask`: a solder-mask opening on every named track and shape (`SetHasSolderMask`), with its own margin (`SetLocalSolderMaskMargin`; `None`
    /// is the board's). Only an item on an outer copper layer has one (the rows' `isExternalLayerTrack` / `isExternalCuLayer`); a track or shape without an
    /// opening keeps no margin (the file writes `(solder_mask_margin ..)` only after a mask layer).
    pub(crate) fn set_solder_mask(&mut self, ids: &[String], solder_mask: bool, margin: Option<Nm>) -> Result<(), Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(fail("ops_bad_mask", "set_solder_mask", "no items given"));
        }
        let mut tracks = Vec::new();
        let mut shapes = Vec::new();
        for id in ids {
            if let Some(t) = self.design.routing.as_ref().and_then(|r| r.tracks.iter().find(|t| &t.id == id)) {
                if !is_outer_copper(&t.layer) {
                    return Err(fail("ops_bad_mask", id, "only a track on the front or back copper has a solder mask opening"));
                }
                tracks.push(id.clone());
            } else if let Some(s) = self.design.drawings.as_ref().and_then(|d| d.shapes.iter().find(|s| s.id() == id)) {
                if !is_outer_copper(s.layer()) {
                    return Err(fail("ops_bad_mask", id, "only a shape on the front or back copper has a solder mask opening"));
                }
                shapes.push(id.clone());
            } else {
                return Err(fail("ops_unknown_item", id, "no track or shape with this id"));
            }
        }
        let d = self.drawings_mut();
        let before = (d.track_masks.clone(), d.board_items.shape_styles.clone());
        for id in &tracks {
            d.track_masks.retain(|m| &m.id != id);
            if solder_mask {
                d.track_masks.push(eda_model::track_mask::TrackMask { id: id.clone(), margin });
            }
        }
        d.track_masks.sort_by(|a, b| a.id.cmp(&b.id));
        for id in &shapes {
            d.board_items.set_solder_mask(id, solder_mask, margin);
        }
        if (d.track_masks.clone(), d.board_items.shape_styles.clone()) == before {
            return Err(fail("ops_nothing_to_change", "set_solder_mask", "the items already have this solder mask opening"));
        }
        Ok(())
    }

    /// `Cmd::SetCornerRadius`: `EDA_SHAPE::SetCornerRadius` of every named rectangle. The row's validator: not below zero, not more than half the shorter side.
    pub(crate) fn set_corner_radius(&mut self, ids: &[String], radius: Nm) -> Result<(), Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(fail("ops_bad_shape", "set_corner_radius", "no shapes given"));
        }
        if radius < Nm::ZERO {
            return Err(fail("ops_bad_shape", "set_corner_radius", "Value must be greater than or equal to 0"));
        }
        let d = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_shape", "set_corner_radius", "the board has no drawings yet"))?;
        for id in ids {
            match d.shapes.iter().find(|s| s.id() == id) {
                Some(Shape::Rect { start, end, .. }) => {
                    let half = (end.x - start.x).abs().min((end.y - start.y).abs()) / 2;
                    if radius > half {
                        return Err(fail("ops_bad_shape", id, "the corner radius cannot be more than half the rectangle's shorter side"));
                    }
                }
                Some(_) => return Err(fail("ops_bad_shape", id, "only a rectangle has a corner radius")),
                None => return Err(fail("ops_unknown_shape", id, "no shape with this id")),
            }
        }
        if ids.iter().all(|id| d.board_items.corner_radius_of(id) == radius) {
            return Err(fail("ops_nothing_to_change", "set_corner_radius", "the rectangles already have this corner radius"));
        }
        for id in ids {
            d.board_items.set_corner_radius(id, radius);
        }
        Ok(())
    }

    /// `Cmd::SetViaProps`: the via `id`'s tenting, covering, plugging, capping, filling, backdrill, post-machining and padstack become `props` (the studio works out a
    /// row's new value the way the `PCB_VIA` / `PADSTACK` setters do, `kicad-port/viaProps.ts`; Track & Via Properties its own, `kicad-port/trackViaDialog.ts`). The drills' layers must be copper layers of the board and their sizes
    /// above zero. The Rust DRC's tenting table (`DrawingsSection::via_tenting`, by position and net) follows the tenting. Refused when nothing changes.
    pub(crate) fn set_via_props(&mut self, id: &str, props: &ViaProps) -> Result<(), Vec<CheckResult>> {
        let board_copper = self.model.board.layers.clone();
        for d in [&props.backdrill, &props.tertiary_drill].into_iter().flatten() {
            if d.size <= Nm::ZERO {
                return Err(fail("ops_bad_via", id, "a backdrill's size must be greater than zero"));
            }
            for layer in [&d.start, &d.end] {
                if !board_copper.iter().any(|l| l == layer) {
                    return Err(fail("ops_bad_via", layer, "Via layer is outside the board stack."));
                }
            }
        }
        for p in [&props.front_post_machining, &props.back_post_machining].into_iter().flatten() {
            if p.size < Nm::ZERO || p.depth < Nm::ZERO || !(0..=1800).contains(&p.angle_ddeg) {
                return Err(fail("ops_bad_via", id, "a post-machining size or depth cannot be negative, nor its angle outside 0 to 180 degrees"));
            }
        }
        if let Some(stack) = &props.padstack {
            for (layer, size) in &stack.sizes {
                if *size <= Nm::ZERO {
                    return Err(fail("ops_bad_via", layer, "Via diameter must be greater than zero."));
                }
                if layer != "Inner" && !board_copper.iter().any(|l| l == layer) {
                    return Err(fail("ops_bad_via", layer, "Via layer is outside the board stack."));
                }
            }
        }
        let (at, net, diameter) = {
            let v = self.design.routing.as_ref().and_then(|rt| rt.vias.iter().find(|v| v.id == id)).ok_or_else(|| fail("ops_unknown_via", id, "no via with this id"))?;
            (v.at, v.net.clone(), v.diameter)
        };
        let before = self.design.drawings.as_ref().and_then(|d| d.via_extra(id)).cloned().unwrap_or(ViaExtra { id: id.to_string(), ..Default::default() });
        let mut extra = before.clone();
        extra.set_props(props);
        // the padstack (Track & Via Properties' "Padstack mode:"), written as `format( const PCB_TRACK* )` writes it for this board's copper
        extra.set_padstack(props.padstack.as_ref(), diameter, &board_copper);
        if extra == before {
            return Err(fail("ops_nothing_to_change", id, "the via already has these properties"));
        }
        let dr = self.drawings_mut();
        match (dr.via_extras.iter().position(|e| e.id == id), extra.is_default()) {
            (Some(i), true) => {
                dr.via_extras.remove(i);
            }
            (Some(i), false) => dr.via_extras[i] = extra,
            (None, false) => dr.via_extras.push(extra),
            (None, true) => {}
        }
        dr.via_tenting.retain(|t| !(t.at == at && t.net == net));
        if props.front_tenting.is_some() || props.back_tenting.is_some() {
            dr.via_tenting.push(eda_model::ir::ViaTenting { at, net, front: props.front_tenting, back: props.back_tenting });
        }
        Ok(())
    }

    /// `Cmd::SetTeardropParams`: `GetTeardropParams() = params` on every via and pad named (`REF.NUM`, `REF.NUM#k`) -- Track & Via Properties' and Pad Properties'
    /// Teardrops parts (`DIALOG_TRACK_VIA_PROPERTIES::TransferDataFromWindow`, `DIALOG_PAD_PROPERTIES::transferDataToPad`) and the Properties panel's Teardrops
    /// rows. A via's land on its `(teardrops ..)` token, a pad's on its edit ([`eda_model::fp_edit::PadEdit::teardrops`]); `None` takes an item's own parameters
    /// away, a board file's token too, and Board Setup's block serves it again ([`eda_model::teardrop_params`]). Refused for an id that is neither a via nor a
    /// pad on the board, for a ratio or size below zero, and when nothing changes.
    pub(crate) fn set_teardrop_params(&mut self, ids: &[String], params: Option<&eda_model::teardrop_params::TeardropParams>) -> Result<(), Vec<CheckResult>> {
        use eda_model::fp_edit::{pad_index, parse_pad_id, PadEdit};
        if let Some(p) = params {
            if p.best_length_ratio < 0.0 || p.best_width_ratio < 0.0 || p.filter_ratio < 0.0 || p.max_length < Nm::ZERO || p.max_width < Nm::ZERO {
                return Err(fail("ops_bad_teardrops", "set_teardrop_params", "teardrop ratios and sizes cannot be negative"));
            }
        }
        let snapshot = |b: &Self| b.design.drawings.as_ref().map(|d| (d.via_extras.clone(), d.footprint_edits.clone(), d.footprint_extras.clone()));
        let before = snapshot(self);
        for id in ids {
            let is_via = self.design.routing.as_ref().is_some_and(|rt| rt.vias.iter().any(|v| v.id == *id));
            if is_via {
                let dr = self.drawings_mut();
                let mut extra = dr.via_extra(id).cloned().unwrap_or(ViaExtra { id: id.clone(), ..Default::default() });
                extra.set_teardrops(params);
                match (dr.via_extras.iter().position(|e| e.id == *id), extra.is_default()) {
                    (Some(i), true) => {
                        dr.via_extras.remove(i);
                    }
                    (Some(i), false) => dr.via_extras[i] = extra,
                    (None, false) => dr.via_extras.push(extra),
                    (None, true) => {}
                }
                continue;
            }
            let Some((reference, number, nth)) = parse_pad_id(id) else { return Err(fail("ops_unknown_item", id, "no via or pad with this id")) };
            self.require_placed(&reference)?;
            let footprint = self.model.part(&reference).and_then(|p| self.unedited_footprint(p)).ok_or_else(|| fail("ops_no_footprint", &reference, "this part has no footprint geometry to edit"))?;
            let Some(index) = pad_index(&footprint, &number, nth) else { return Err(fail("ops_unknown_pad", id, format!("{reference} has no pad {number}"))) };
            let mut edit = self.design.footprint_edit(&reference).and_then(|e| e.pad(&number, nth)).cloned().unwrap_or_else(|| PadEdit::none(&number, nth));
            edit.teardrops = params.copied();
            self.edit_slot(&reference).set_pad(edit);
            self.tidy_edit(&reference);
            if params.is_none() {
                // the token a board file gave the pad goes too
                let count = footprint.pads.len();
                if let Some(e) = self.drawings_mut().footprint_extras.iter_mut().find(|e| e.id == reference).filter(|e| e.pads.len() == count) {
                    eda_model::teardrop_params::set_in_tokens(&mut e.pads[index].tokens, None);
                }
            }
        }
        if snapshot(self).unwrap_or_default() == before.unwrap_or_default() {
            return Err(fail("ops_nothing_to_change", "set_teardrop_params", "the items already have these teardrop parameters"));
        }
        Ok(())
    }

    /// `Cmd::SetFootprintPasteMargin`: `FOOTPRINT::SetLocalSolderPasteMargin` / `SetLocalSolderPasteMarginRatio` (`None` takes the board's). A footprint of a board
    /// read from a file keeps them among its tokens (`FootprintExtra::tokens`, `(solder_paste_margin ..)` and `(solder_paste_margin_ratio ..)`); one placed from the
    /// intent in its edit (`FootprintEdit::solder_paste_margin`, `..ratio`; UI-LEFT P14), which the writer writes.
    pub(crate) fn set_footprint_paste_margin(&mut self, part: &str, margin: Option<Nm>, ratio: Option<f64>) -> Result<(), Vec<CheckResult>> {
        if let Some(r) = ratio {
            // `MARGIN_OFFSET_BINDER`: the ratio is a percentage the dialog limits like a pad's (`padValuesOK`: -50% .. +100%)
            if !(-0.5..=1.0).contains(&r) {
                return Err(fail("ops_bad_footprint", part, "The solder paste margin ratio must be between -50% and 100%."));
            }
        }
        // (a record of facts the board gave a footprint made here holds no file form: its margins are its edit's, as for one with no record)
        let from_file = self.design.drawings.as_ref().is_some_and(|d| d.footprint_extras.iter().any(|e| e.id == part && e.holds_file_form()));
        if !from_file {
            self.require_placed(part)?;
            let before = self.design.footprint_edit(part).map(|e| (e.solder_paste_margin, e.solder_paste_ratio)).unwrap_or_default();
            if before == (margin, ratio) {
                return Err(fail("ops_nothing_to_change", part, "the footprint already has these paste margins"));
            }
            let slot = self.edit_slot(part);
            slot.solder_paste_margin = margin;
            slot.solder_paste_ratio = ratio;
            self.tidy_edit(part);
            return Ok(());
        }
        let d = self.design.drawings.as_mut().ok_or_else(|| fail("ops_no_board_file_footprint", part, "this footprint was not read from a board file"))?;
        let e = d.footprint_extras.iter_mut().find(|e| e.id == part).ok_or_else(|| fail("ops_no_board_file_footprint", part, "this footprint was not read from a board file"))?;
        let before = e.tokens.clone();
        set_paste_margin_tokens(&mut e.tokens, margin, ratio);
        if e.tokens == before {
            return Err(fail("ops_nothing_to_change", part, "the footprint already has these paste margins"));
        }
        Ok(())
    }
}

/// A footprint's paste margins in the kept forms of its board file (`FootprintExtra::tokens`): `(solder_paste_margin ..)` and `(solder_paste_margin_ratio ..)` (the older
/// `(solder_paste_ratio ..)` replaced) set to `margin` and `ratio`, each where the file had it, or taken out for `None`.
pub(crate) fn set_paste_margin_tokens(tokens: &mut Vec<String>, margin: Option<Nm>, ratio: Option<f64>) {
    let fmt = |v: f64| {
        let s = format!("{v:.6}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    let tag = |t: &str| t.strip_prefix('(').and_then(|r| r.split_whitespace().next()).map(str::to_string);
    let mut set = |name: &str, aliases: &[&str], value: Option<String>| {
        let at = tokens.iter().position(|t| tag(t).is_some_and(|n| n == name || aliases.contains(&n.as_str())));
        tokens.retain(|t| !tag(t).is_some_and(|n| n == name || aliases.contains(&n.as_str())));
        if let Some(v) = value {
            tokens.insert(at.unwrap_or(tokens.len()).min(tokens.len()), format!("({name} {v})"));
        }
    };
    set("solder_paste_margin", &[], margin.map(|m| fmt(m.as_f64() / 1_000_000.0)));
    set("solder_paste_margin_ratio", &["solder_paste_ratio"], ratio.map(fmt));
}

/// A track's points and arc mid offset once its first and/or last point is where `start` / `end` say.
struct MovedEnds {
    pts: Vec<Point>,
    arc_mid_offset: Option<Point>,
}

fn moved_ends(t: &Track, start: Option<Point>, end: Option<Point>) -> MovedEnds {
    if let Some((s, mid, e)) = t.arc() {
        let (s, e) = (start.unwrap_or(s), end.unwrap_or(e));
        return MovedEnds { pts: tessellate_arc(s, mid, e, TRACK_ARC_SEGMENTS), arc_mid_offset: Some(Point { x: mid.x - s.x, y: mid.y - s.y }) };
    }
    let mut pts = t.pts.clone();
    if let (Some(s), Some(first)) = (start, pts.first_mut()) {
        *first = s;
    }
    if let (Some(e), Some(last)) = (end, pts.last_mut()) {
        *last = e;
    }
    // A plain polyline that carried a stale arc mid offset (the points were reshaped since) keeps it: `Track::arc` ignores it unless the points
    // still are the arc's tessellation, exactly as before this edit.
    MovedEnds { pts, arc_mid_offset: t.arc_mid_offset }
}
