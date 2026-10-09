//! Footprints on the board as editable objects: the verbs behind the Footprint Properties dialog, the Pad Properties dialog and
//! the Properties panel's footprint, field and pad rows.
//!
//! Ports what `DIALOG_FOOTPRINT_PROPERTIES::TransferDataFromWindow` (`pcbnew/dialogs/dialog_footprint_properties.cpp` at KiCad
//! 8303b2ad) and `DIALOG_PAD_PROPERTIES::TransferDataFromWindow` (`dialog_pad_properties.cpp`) do to a `FOOTPRINT` and its
//! `PAD`s, over the model [`eda_model::fp_edit`] describes:
//!
//! * [`Cmd::EditBoardFootprint`](crate::Cmd::EditBoardFootprint) replaces what it is given of a footprint's Reference and Value layout, user
//!   fields and attributes -- the grid and the checkboxes of the dialog, one `BOARD_COMMIT::Push( "Edit Footprint Properties" )`.
//!   The dialog's fields validate text size and thickness (`Validate`); so does this.
//! * [`Cmd::EditBoardPad`](crate::Cmd::EditBoardPad) replaces the edit of one pad (shape, size, hole, offset, corner radius, margins).
//! * A field is an item of its own on the board (`PCB_FIELD`, selectable, movable): `MoveItems`, `RotateItems` and `FlipItems`
//!   accept its id (`REF:Reference`) and move, turn or flip the text on its own, as `EDIT_TOOL` does a selected field
//!   ([`Board::transform_fields`]).

use super::pcb_transform::{flip_layer, side_specific, FlipDirection, Xform};
use super::Board;
use eda_model::fp_edit::{default_reference_layout, default_value_layout, field_id, pad_index, parse_field_id, patched_footprint, FieldLayout, FootprintAttrs, FootprintEdit, PadEdit, UserField, REFERENCE, VALUE};
use eda_model::ir::{FootprintInstance, Um};
use eda_model::CheckResult;
use std::collections::BTreeSet;

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

/// `TEXT_MIN_SIZE_MM` and `TEXT_MAX_SIZE_MM` (`include/eda_text.h`), µm.
const TEXT_MIN_UM: Um = 1;
const TEXT_MAX_UM: Um = 250_000;

/// The layers a footprint's text can be on besides copper: `LSET::TechAndUserUIOrder`, plus the `User.1` .. `User.9` layers.
const TECH_LAYERS: [&str; 18] = ["F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", "Edge.Cuts", "Margin", "F.CrtYd", "B.CrtYd", "F.Fab", "B.Fab"];

/// `ClampTextPenSize`: the thickest pen a text of this size takes (a quarter of its smaller side).
fn max_pen(size: (Um, Um)) -> Um {
    (size.0.abs().min(size.1.abs()) as f64 * 0.25).round() as Um
}

impl Board<'_> {
    /// The checks `DIALOG_FOOTPRINT_PROPERTIES::Validate` makes of each row of the field grid: a size between 1 µm and 250 mm either way,
    /// a thickness no thicker than the size allows, a layer the board has.
    fn check_layout(&self, what: &str, l: &FieldLayout) -> Result<(), Vec<CheckResult>> {
        let (w, h) = l.size;
        for (axis, v) in [("width", w), ("height", h)] {
            if v < TEXT_MIN_UM {
                return Err(fail("ops_bad_field", what, format!("Text {axis} must be at least {} mm.", TEXT_MIN_UM as f64 / 1000.0)));
            }
            if v > TEXT_MAX_UM {
                return Err(fail("ops_bad_field", what, format!("Text {axis} must be at most {} mm.", TEXT_MAX_UM as f64 / 1000.0)));
            }
        }
        if l.thickness < 0 {
            return Err(fail("ops_bad_field", what, "Text thickness cannot be negative."));
        }
        if l.thickness > max_pen(l.size) {
            return Err(fail("ops_bad_field", what, format!("Text thickness is too large for the text size. It will be clamped at {} mm.", max_pen(l.size) as f64 / 1000.0)));
        }
        let layer = l.layer.as_str();
        let known = self.model.board.layers.iter().any(|c| c == layer) || TECH_LAYERS.contains(&layer) || layer.strip_prefix("User.").is_some_and(|n| n.parse::<u8>().is_ok_and(|n| (1..=9).contains(&n)));
        if !known {
            return Err(fail("ops_bad_field", what, format!("{layer:?} is not a layer of this board")));
        }
        if !(-1..=1).contains(&l.halign) || !(-1..=1).contains(&l.valign) {
            return Err(fail("ops_bad_field", what, "justification is -1, 0 or 1"));
        }
        Ok(())
    }

    /// The footprint's edit, made if it had none.
    fn edit_slot(&mut self, id: &str) -> &mut FootprintEdit {
        let dr = self.drawings_mut();
        let i = match dr.footprint_edits.iter().position(|e| e.id == id) {
            Some(i) => i,
            None => {
                dr.footprint_edits.push(FootprintEdit::new(id));
                dr.footprint_edits.sort_by(|a, b| a.id.cmp(&b.id));
                dr.footprint_edits.iter().position(|e| e.id == id).expect("just inserted")
            }
        };
        &mut dr.footprint_edits[i]
    }

    /// Drop the footprint's edit when nothing is left in it.
    fn tidy_edit(&mut self, id: &str) {
        if let Some(dr) = self.design.drawings.as_mut() {
            dr.footprint_edits.retain(|e| e.id != id || !e.is_empty());
        }
    }

    /// `Cmd::EditBoardFootprint`. What is `None` stays as it is.
    pub(crate) fn edit_board_footprint(&mut self, part: &str, reference: Option<&FieldLayout>, value: Option<&FieldLayout>, fields: Option<&[UserField]>, attrs: Option<&FootprintAttrs>) -> Result<(), Vec<CheckResult>> {
        self.require_placed(part)?;
        if reference.is_none() && value.is_none() && fields.is_none() && attrs.is_none() {
            return Err(fail("ops_bad_footprint_edit", part, "nothing to change: give a reference, a value, user fields or attributes"));
        }
        if let Some(l) = reference {
            self.check_layout(&format!("{part} Reference"), l)?;
        }
        if let Some(l) = value {
            self.check_layout(&format!("{part} Value"), l)?;
        }
        if let Some(list) = fields {
            let mut seen: BTreeSet<&str> = BTreeSet::new();
            for f in list {
                let name = f.name.trim();
                // `Validate`: "Fields must have a name."
                if name.is_empty() {
                    return Err(fail("ops_bad_field", part, "Fields must have a name."));
                }
                if name != f.name {
                    return Err(fail("ops_bad_field", part, format!("The field name {:?} starts or ends with a space.", f.name)));
                }
                if name == REFERENCE || name == VALUE || name == "Datasheet" || name == "Description" || name == "Footprint" {
                    return Err(fail("ops_bad_field", part, format!("{name:?} is the name of a mandatory field.")));
                }
                if name.contains(':') && name.starts_with(':') {
                    return Err(fail("ops_bad_field", part, "A field name cannot start with a colon."));
                }
                if !seen.insert(name) {
                    return Err(fail("ops_bad_field", part, format!("There are two fields called {name:?}.")));
                }
                self.check_layout(&format!("{part} {name}"), &f.layout)?;
            }
        }

        let slot = self.edit_slot(part);
        if let Some(l) = reference {
            slot.reference = Some(l.clone());
        }
        if let Some(l) = value {
            slot.value = Some(l.clone());
        }
        if let Some(list) = fields {
            slot.fields = list.to_vec();
        }
        if let Some(a) = attrs {
            slot.attrs = Some(*a);
            // The footprint's DNP and BOM flags are the symbol's too: see `Design::sync_symbol_bom_flags`.
            let edit = [slot.clone()];
            self.design.sync_symbol_bom_flags(&edit);
        }
        self.tidy_edit(part);
        Ok(())
    }

    /// `Cmd::EditBoardField`: one field's layout and/or text.
    pub(crate) fn edit_board_field(&mut self, part: &str, name: &str, layout: Option<&FieldLayout>, text: Option<&str>) -> Result<(), Vec<CheckResult>> {
        self.require_placed(part)?;
        if layout.is_none() && text.is_none() {
            return Err(fail("ops_bad_field", &field_id(part, name), "nothing to change: give a layout, a text, or both"));
        }
        let user = name != REFERENCE && name != VALUE;
        if user && !self.design.footprint_edit(part).is_some_and(|e| e.fields.iter().any(|f| f.name == name)) {
            return Err(fail("ops_unknown_item", &field_id(part, name), format!("{part} has no field called {name:?}")));
        }
        if text.is_some() && !user {
            return Err(fail("ops_bad_field", &field_id(part, name), format!("The text of {name} comes from the schematic: edit the symbol")));
        }
        if let Some(l) = layout {
            self.check_layout(&format!("{part} {name}"), l)?;
        }
        let slot = self.edit_slot(part);
        match name {
            REFERENCE => slot.reference = layout.cloned(),
            VALUE => slot.value = layout.cloned(),
            other => {
                if let Some(f) = slot.fields.iter_mut().find(|f| f.name == other) {
                    if let Some(l) = layout {
                        f.layout = l.clone();
                    }
                    if let Some(t) = text {
                        f.text = t.to_string();
                    }
                }
            }
        }
        self.tidy_edit(part);
        Ok(())
    }

    /// `Cmd::EditBoardPad`: the edit of the pad `edit` names (its number and which of the pads that share it) becomes `edit`; one that changes
    /// nothing takes the pad's edit away. Refused when the pad is not there, or the result is a pad that cannot be made (a through-hole pad
    /// with no hole, a hole as big as its pad, a rounded rectangle whose radius is out of range).
    pub(crate) fn edit_board_pad(&mut self, part: &str, edit: &PadEdit) -> Result<(), Vec<CheckResult>> {
        self.require_placed(part)?;
        let p = self.model.part(part).ok_or_else(|| fail("ops_unknown_part", part, "no part with this reference exists in the model"))?;
        let lib = self.model.library_footprint_of(p).ok_or_else(|| fail("ops_no_footprint", part, "this part has no footprint geometry to edit"))?;
        let nth = edit.nth.max(1);
        let pad_ref = format!("{part}.{}", edit.number);
        let Some(_index) = pad_index(&lib, &edit.number, nth) else {
            return Err(fail("ops_unknown_pad", &pad_ref, format!("{part} has no pad {}{}", edit.number, if nth > 1 { format!(" (#{nth})") } else { String::new() })));
        };
        if let Some((w, h)) = edit.size {
            if w <= 0 || h <= 0 {
                return Err(fail("ops_bad_pad", &pad_ref, "Pad size must be greater than zero."));
            }
        }
        if let Some(d) = edit.drill {
            if d <= 0 {
                return Err(fail("ops_bad_pad", &pad_ref, "Hole size must be greater than zero."));
            }
        }
        if let Some((w, h)) = edit.drill_slot {
            if w <= 0 || h <= 0 {
                return Err(fail("ops_bad_pad", &pad_ref, "Hole size must be greater than zero."));
            }
        }
        if edit.drill.is_some() && edit.drill_slot.is_some() {
            return Err(fail("ops_bad_pad", &pad_ref, "A pad has a round hole or an oblong one, not both."));
        }
        if let Some(r) = edit.roundrect_ratio {
            if !(0.0..=0.5).contains(&r) {
                return Err(fail("ops_bad_pad", &pad_ref, "The corner radius ratio must be between 0 and 50% of the shorter side."));
            }
        }
        if let Some(r) = edit.solder_paste_margin_ratio {
            // `padValuesOK`: the paste ratio is limited to -50% .. +100%.
            if !(-0.5..=1.0).contains(&r) {
                return Err(fail("ops_bad_pad", &pad_ref, "The solder paste margin ratio must be between -50% and 100%."));
            }
        }
        if let Some(c) = edit.clearance {
            if c < 0 {
                return Err(fail("ops_bad_pad", &pad_ref, "The clearance override cannot be negative."));
            }
        }

        // What the pad becomes must be a pad the library's own rules accept: edit the footprint's edits in a trial and judge it.
        let mut edit = edit.clone();
        edit.nth = nth;
        let mut all: Vec<PadEdit> = self.design.footprint_edit(part).map(|e| e.pads.clone()).unwrap_or_default();
        all.retain(|e| !(e.number == edit.number && e.nth == nth));
        if !edit.is_empty() {
            all.push(edit.clone());
        }
        let was: BTreeSet<String> = lib.validate().into_iter().filter_map(|c| c.hint).collect();
        let trial = patched_footprint(&lib, &all);
        if let Some(new) = trial.validate().into_iter().filter(|c| c.hint.as_ref().is_some_and(|h| !was.contains(h))).find(|c| c.location.as_deref().is_some_and(|l| l.ends_with(&format!("pad {}", edit.number)))) {
            return Err(fail("ops_bad_pad", &pad_ref, new.hint.unwrap_or_default()));
        }
        self.edit_slot(part).set_pad(edit);
        self.tidy_edit(part);
        Ok(())
    }

    /// A schematic symbol's `DNP` / `Exclude from BOM` changed: a footprint whose attributes were edited takes the same value, so the
    /// flag that was set last is the one both files carry (`Design::sync_symbol_bom_flags` is the other way).
    pub(crate) fn sync_footprint_bom_flags(&mut self, ids: &[String], dnp: Option<bool>, exclude_from_bom: Option<bool>) {
        let Some(dr) = self.design.drawings.as_mut() else { return };
        for edit in dr.footprint_edits.iter_mut().filter(|e| ids.contains(&e.id)) {
            if let Some(a) = edit.attrs.as_mut() {
                if let Some(v) = dnp {
                    a.dnp = v;
                }
                if let Some(v) = exclude_from_bom {
                    a.exclude_from_bom = v;
                }
            }
        }
    }

    /// The field an id names (`REF:Reference`, `REF:Value`, `REF:<user field>`): the footprint's reference and the field's name. `None`
    /// when the footprint is not on the board or has no such field.
    pub(crate) fn resolve_field(&self, id: &str) -> Option<(String, String)> {
        let (reference, name) = parse_field_id(id)?;
        self.pose_of(reference)?;
        let known = name == REFERENCE || name == VALUE || self.design.footprint_edit(reference).is_some_and(|e| e.fields.iter().any(|f| f.name == name));
        known.then(|| (reference.to_string(), name.to_string()))
    }

    /// The layout in effect for a field: the edit's, else the default placement.
    pub(crate) fn field_layout(&self, reference: &str, name: &str) -> Option<FieldLayout> {
        let fp = self.pose_of(reference)?;
        let edit = self.design.footprint_edit(reference);
        match name {
            REFERENCE => Some(edit.and_then(|e| e.reference.clone()).unwrap_or_else(|| {
                let footprint = self.model.part(reference).and_then(|p| self.model.footprint_of(p));
                match footprint {
                    Some(f) => default_reference_layout(fp, &f),
                    None => default_reference_layout(fp, &eda_model::Footprint { name: String::new(), pads: vec![], courtyard: None, courtyard_outlines: vec![], model: None }),
                }
            })),
            VALUE => Some(edit.and_then(|e| e.value.clone()).unwrap_or_else(|| default_value_layout(fp))),
            other => edit?.fields.iter().find(|f| f.name == other).map(|f| f.layout.clone()),
        }
    }

    fn store_layout(&mut self, reference: &str, name: &str, layout: FieldLayout) {
        let slot = self.edit_slot(reference);
        match name {
            REFERENCE => slot.reference = Some(layout),
            VALUE => slot.value = Some(layout),
            other => {
                if let Some(f) = slot.fields.iter_mut().find(|f| f.name == other) {
                    f.layout = layout;
                }
            }
        }
    }

    /// `PCB_TEXT::Move`/`::Rotate`/`::Flip` over the fields `ids` name, on their own (a field whose footprint moves with it is the
    /// footprint's business). The field's anchor and angle go through the transform on the board, and its layout is worked out from the
    /// result ([`FieldLayout::set_from_board`]). A flip also turns the text over to the other side: its layer, and whether it is mirrored.
    pub(crate) fn transform_fields(&mut self, ids: &BTreeSet<String>, x: Xform, copper: usize) -> Result<(), Vec<CheckResult>> {
        for id in ids {
            let (reference, name) = self.resolve_field(id).ok_or_else(|| fail("ops_unknown_item", id, "no field with this id"))?;
            let fp: FootprintInstance = self.require_placed(&reference)?;
            let mut layout = self.field_layout(&reference, &name).expect("the field was resolved");
            let pos = x.point(layout.board_position(&fp));
            let angle = layout.board_angle(&fp);
            let angle = match x {
                Xform::Move { .. } => angle,
                // A clockwise turn takes from a counter-clockwise angle.
                Xform::Rotate { angle: turn, .. } => (angle - turn).rem_euclid(360_000),
                Xform::Flip { dir: FlipDirection::LeftRight, .. } => (-angle).rem_euclid(360_000),
                Xform::Flip { dir: FlipDirection::TopBottom, .. } => (180_000 - angle).rem_euclid(360_000),
            };
            layout.set_from_board(&fp, pos, angle);
            if x.is_flip() {
                layout.layer = flip_layer(&layout.layer, copper);
                if side_specific(&layout.layer) {
                    layout.mirror = !layout.mirror;
                }
            }
            self.store_layout(&reference, &name, layout);
        }
        Ok(())
    }

    /// Turn the fields of footprint `id` over with it (`FOOTPRINT::Flip` flips each field): the layer goes to the other side and the
    /// text is mirrored. Positions and angles are in the footprint's own frame and need nothing. A footprint that was never edited has
    /// only default fields, which follow its side.
    pub(crate) fn flip_footprint_fields(&mut self, id: &str, copper: usize) {
        let Some(dr) = self.design.drawings.as_mut() else { return };
        let Some(edit) = dr.footprint_edits.iter_mut().find(|e| e.id == id) else { return };
        let turn = |l: &mut FieldLayout| {
            l.layer = flip_layer(&l.layer, copper);
            if side_specific(&l.layer) {
                l.mirror = !l.mirror;
            }
        };
        if let Some(l) = edit.reference.as_mut() {
            turn(l);
        }
        if let Some(l) = edit.value.as_mut() {
            turn(l);
        }
        for f in edit.fields.iter_mut() {
            turn(&mut f.layout);
        }
    }
}
