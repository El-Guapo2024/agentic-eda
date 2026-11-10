//! A footprint's own texts as items of the board (UI-LEFT P15): `PCB_SELECTION_TOOL::Selectable` lets a footprint's `PCB_FIELD`, `PAD` and `PCB_TEXT` be picked
//! on the board ("Most footprint children can only be selected in the footprint editor": its other graphics are picked as the footprint), so a footprint's
//! `fp_text user` (`REF~t<k>`, the k-th of `Footprint::art`'s texts) is moved, turned, flipped, edited (`DIALOG_TEXT_PROPERTIES` on a `PCB_TEXT` whose parent is
//! a footprint) and deleted (`EDIT_TOOL::DeleteItems`: `commit.Remove( board_item )`, the parent footprint modified) on its own.
//!
//! The art is the library footprint's, which every instance shares: an edit gives the instance its own copy of its footprint (`FootprintAssignment::definition`,
//! the name unchanged), as the global edit does (`crate::pcb_global_edit`) and Change Footprint does, with the pad edits still laid over it. The text's layout is a
//! [`FieldLayout`] in the footprint's frame, its layer as for a top-side footprint ([`eda_model::fp_art::layer_on_side`]).

use super::pcb_transform::{flip_layer, side_specific, FlipDirection, Xform};
use super::Board;
use eda_model::fp_assign::FootprintAssignment;
use eda_model::fp_edit::FieldLayout;
use eda_model::ir::FootprintInstance;
use eda_model::CheckResult;
use std::collections::BTreeSet;

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

/// `REF~t<k>`: the footprint's reference and k.
pub fn parse_fp_text_id(id: &str) -> Option<(&str, usize)> {
    let (reference, rest) = id.rsplit_once('~')?;
    let k = rest.strip_prefix('t')?.parse().ok()?;
    (!reference.is_empty()).then_some((reference, k))
}

impl Board<'_> {
    /// The footprint and k a `REF~t<k>` names, when the footprint is on the board and has that text.
    pub(crate) fn resolve_fp_text(&self, id: &str) -> Option<(String, usize)> {
        let (reference, k) = parse_fp_text_id(id)?;
        self.pose_of(reference)?;
        let part = self.model.part(reference)?;
        let footprint = self.unedited_footprint(part)?;
        (k < footprint.art.texts.len()).then(|| (reference.to_string(), k))
    }

    /// `f` on the instance's own copy of footprint `reference` (the footprint it is on now, before its pad edits): one assignment with the copy as its definition.
    pub(crate) fn with_own_copy(&mut self, reference: &str, f: impl FnOnce(&mut eda_model::Footprint, &FootprintInstance) -> Result<(), Vec<CheckResult>>) -> Result<(), Vec<CheckResult>> {
        let pose = self.require_placed(reference)?;
        let part = self.model.part(reference).ok_or_else(|| fail("ops_unknown_part", reference, "no part with this reference exists in the model"))?;
        let before = self.unedited_footprint(part).ok_or_else(|| fail("ops_no_footprint", reference, "this part has no footprint geometry to edit"))?;
        let name = self.design.footprint_assignment(reference).map(|a| a.footprint.clone()).unwrap_or_else(|| crate::fp_exchange::footprint_name_of(part));
        let mut fp = before.clone();
        f(&mut fp, &pose)?;
        if fp != before {
            self.drawings_mut().set_footprint_assignment(FootprintAssignment { reference: reference.to_string(), footprint: name, definition: Some(fp) });
        }
        Ok(())
    }

    /// The move, turn and flip of a selection on the footprints' own texts among it (`PCB_TEXT::Move`, `Rotate`, `Flip` of a footprint's child): its position and
    /// angle on the board turned as a field's are ([`Board::transform_fields`]), the layer and the mirror turned over by a flip.
    pub(crate) fn transform_fp_texts(&mut self, ids: &BTreeSet<String>, x: Xform, copper: usize) -> Result<(), Vec<CheckResult>> {
        let mut by_footprint: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
        for id in ids {
            let (reference, k) = self.resolve_fp_text(id).ok_or_else(|| fail("ops_unknown_item", id, "no footprint text with this id"))?;
            by_footprint.entry(reference).or_default().push(k);
        }
        for (reference, ks) in by_footprint {
            self.with_own_copy(&reference, |fp, pose| {
                for k in ks {
                    let Some(t) = fp.art.texts.get_mut(k) else { continue };
                    let pos = x.point(t.layout.board_position(pose));
                    let angle = t.layout.board_angle(pose);
                    let angle = match x {
                        Xform::Move { .. } => angle,
                        Xform::Rotate { angle: turn, .. } => (angle - turn).rem_euclid(360_000),
                        Xform::Flip { dir: FlipDirection::LeftRight, .. } => (-angle).rem_euclid(360_000),
                        Xform::Flip { dir: FlipDirection::TopBottom, .. } => (180_000 - angle).rem_euclid(360_000),
                    };
                    t.layout.set_from_board(pose, pos, angle);
                    if x.is_flip() {
                        // the art's layers are as for a top-side footprint: the text goes to the other side of the board, so to the other side here
                        t.layout.layer = flip_layer(&t.layout.layer, copper);
                        if side_specific(&t.layout.layer) {
                            t.layout.mirror = !t.layout.mirror;
                        }
                    }
                }
                Ok(())
            })?;
        }
        Ok(())
    }

    /// `Cmd::EditBoardFootprintText`: Text Properties on a footprint's own text (`DIALOG_TEXT_PROPERTIES::TransferDataFromWindow` on a `PCB_TEXT` of a footprint): its
    /// text and its layout (given in the footprint's frame, the layer on the board's side as the dialog shows it), or its deletion (`EDIT_TOOL::DeleteItems`).
    /// One undo step; refused for an empty text (`DIALOG_TEXT_PROPERTIES`: "The text can't be empty.") and when nothing changes.
    pub(crate) fn edit_board_footprint_text(&mut self, id: &str, text: Option<&str>, layout: Option<&FieldLayout>, delete: bool) -> Result<(), Vec<CheckResult>> {
        let (reference, k) = self.resolve_fp_text(id).ok_or_else(|| fail("ops_unknown_item", id, "no footprint text with this id"))?;
        if text.is_some_and(|t| t.trim().is_empty()) {
            return Err(fail("ops_bad_text", id, "The text can't be empty."));
        }
        let mut changed = false;
        self.with_own_copy(&reference, |fp, pose| {
            if delete {
                fp.art.texts.remove(k);
                changed = true;
                return Ok(());
            }
            let t = &mut fp.art.texts[k];
            let before = t.clone();
            if let Some(s) = text {
                t.text = s.to_string();
            }
            if let Some(l) = layout {
                let mut l = l.clone();
                // the layer as the board shows it, kept as for a top-side footprint
                l.layer = eda_model::fp_art::layer_on_side(&l.layer, pose.side);
                t.layout = l;
            }
            changed = *t != before;
            Ok(())
        })?;
        if !changed {
            return Err(fail("ops_nothing_to_change", id, "the text already is this"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_footprint_text_id_is_its_footprint_and_its_index() {
        assert_eq!(parse_fp_text_id("U1~t2"), Some(("U1", 2)));
        assert_eq!(parse_fp_text_id("U1~g2"), None);
        assert_eq!(parse_fp_text_id("~t2"), None);
        assert_eq!(parse_fp_text_id("U1:Reference"), None);
    }
}
