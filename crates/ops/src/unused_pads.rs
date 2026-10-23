//! "Remove Unused Pads..." (`pcbnew.GlobalEdit.removeUnusedPads`): what `DIALOG_UNUSED_PAD_LAYERS::updatePadsAndVias`
//! (`pcbnew/dialogs/dialog_unused_pad_layers.cpp`) does, over the pads' padstacks ([`eda_model::padstack`]) and the vias' tokens
//! ([`eda_model::via_extra`]).
//!
//! The dialog has two buttons that matter. "Remove Unused Layers" turns on `remove_unused_layers` for the plated through-hole pads and the vias it picked:
//! `SetRemoveUnconnected( true )` and, with "Keep outside layers" checked, `SetKeepTopBottom( true )` (`REMOVE_EXCEPT_START_AND_END`), else the pad removes every
//! layer nothing connects it on (`REMOVE_ALL`). "Restore All Layers" is `SetRemoveUnconnected( false )` (`KEEP_ALL`) for the same items. What the settings
//! do to the copper -- a layer where nothing of the pad's net is connected has no ring (`PAD::FlashLayer`) -- is the connectivity's
//! (`eda_connectivity::flashed_layers`), and kicad-cli's when it plots, fills and checks the board.
//!
//! * "Pads" and "Vias" pick the kinds of item; "Selected only" limits them to the selection (a footprint stands for its pads, a pad for itself, a via for
//!   itself), otherwise every footprint and via of the board is taken.
//! * A pad qualifies when it is plated through-hole (`PAD_ATTRIB::PTH`); a via when it is a through via on a board of more than two copper layers, or any
//!   other via that spans more than one `BOARD::LayerDepth` (`viaHasPotentiallyUnusedLayers`, ported with its layer numbering).
//! * One undo step (`m_commit.Push( "Remove Unused Pads" )`). KiCad closes the dialog silently when nothing qualified; here it is a refusal
//!   (`ops_nothing_to_change`) so the studio can say so and leave no empty undo step.

use super::Board;
use eda_model::footprint::PadKind;
use eda_model::fp_edit::{normalized_stack, parse_pad_id, patched_footprint, PadEdit};
use eda_model::padstack::UnconnectedLayers;
use eda_model::via_extra::{ViaExtra, ViaKind};
use eda_model::CheckResult;
use std::collections::BTreeSet;

/// What the dialog asks for.
pub(crate) struct UnusedPadLayers<'a> {
    /// "Selected only": the ids the dialog was opened with (a footprint's reference, a pad `REF.NUM`, a via's id); `None`: the whole board.
    pub selected: Option<&'a [String]>,
    /// The "Vias" check box.
    pub vias: bool,
    /// The "Pads" check box.
    pub pads: bool,
    /// "Keep outside layers".
    pub keep_outside: bool,
    /// "Remove Unused Layers" (`true`) or "Restore All Layers" (`false`).
    pub remove: bool,
}

fn refuse(check: &str, what: &str, message: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, message.into())]
}

/// The `PCB_LAYER_ID` KiCad numbers a copper layer by: `F_Cu` 0, `B_Cu` 2, `In1_Cu` 4, `In2_Cu` 6 ...
fn kicad_layer_id(layer: &str) -> Option<i64> {
    match layer {
        "F.Cu" => Some(0),
        "B.Cu" => Some(2),
        _ => layer.strip_prefix("In").and_then(|r| r.strip_suffix(".Cu")).and_then(|n| n.parse::<i64>().ok()).map(|n| 2 + 2 * n),
    }
}

/// `BOARD::LayerDepth( start, end )`: the two layers' ids put in order, the difference of them, and the back layer, when it comes out as the end, taken for the last of
/// the stack (`F_Cu + count - 1`). The ids are two apart (`F_Cu` 0, `B_Cu` 2, `In1_Cu` 4), so the back layer is the end only for a via that starts on the front, and the
/// depth is 1 only for the front to the back of a board of two copper layers: any other pair, one between two inner layers or from an inner layer to the back, is 2 or more.
pub(super) fn layer_depth(start: &str, end: &str, copper_layers: usize) -> Option<i64> {
    let (mut a, mut b) = (kicad_layer_id(start)?, kicad_layer_id(end)?);
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }
    if b == 2 {
        b = copper_layers as i64 - 1;
    }
    Some(b - a)
}

impl Board<'_> {
    /// `Cmd::RemoveUnusedPads`. See the module doc.
    pub(crate) fn remove_unused_pads(&mut self, req: &UnusedPadLayers) -> Result<(), Vec<CheckResult>> {
        let target = if req.remove { UnconnectedLayers::remove(req.keep_outside) } else { UnconnectedLayers::KeepAll };
        let copper_layers = self.model.board.layers.len();

        // The selection, sorted into what each id names.
        let mut parts: BTreeSet<String> = BTreeSet::new();
        let mut pad_ids: BTreeSet<(String, String, u32)> = BTreeSet::new();
        let mut via_ids: BTreeSet<String> = BTreeSet::new();
        if let Some(selected) = req.selected {
            for id in selected {
                if self.pose_of(id).is_some() {
                    parts.insert(id.clone());
                } else if let Some((reference, number, nth)) = parse_pad_id(id).filter(|(r, _, _)| self.pose_of(r).is_some()) {
                    pad_ids.insert((reference, number, nth));
                } else if self.design.routing.as_ref().is_some_and(|rt| rt.vias.iter().any(|v| &v.id == id)) {
                    via_ids.insert(id.clone());
                }
                // Anything else a selection holds (a track, a zone, a text) is no business of this dialog.
            }
        }

        let mut changed = 0usize;

        if req.pads {
            let references: Vec<String> = match req.selected {
                None => self.placement().footprints.iter().map(|f| f.id.clone()).collect(),
                Some(_) => parts.iter().cloned().chain(pad_ids.iter().map(|(r, _, _)| r.clone())).collect::<BTreeSet<_>>().into_iter().collect(),
            };
            for reference in references {
                let Some(part) = self.model.part(&reference).cloned() else { continue };
                let Some(library) = self.unedited_footprint(&part) else { continue };
                let edits: Vec<PadEdit> = self.design.footprint_edit(&reference).map(|e| e.pads.clone()).unwrap_or_default();
                let patched = patched_footprint(&library, &edits);
                let whole = req.selected.is_none() || parts.contains(&reference);
                let mut seen: std::collections::BTreeMap<&str, u32> = std::collections::BTreeMap::new();
                for (index, pad) in patched.pads.iter().enumerate() {
                    let nth = {
                        let n = seen.entry(pad.number.as_str()).or_insert(0);
                        *n += 1;
                        *n
                    };
                    // `padHasPotentiallyUnusedLayers`: a plated through-hole pad.
                    if pad.kind != PadKind::ThroughHole || !(whole || pad_ids.contains(&(reference.clone(), pad.number.clone(), nth))) {
                        continue;
                    }
                    if pad.ext.stack.unconnected == target {
                        continue;
                    }
                    // The pad's padstack with the new setting, as the edit keeps it: nothing when it is the library's own.
                    let mut stack = normalized_stack(&pad.ext.stack);
                    stack.unconnected = target;
                    let mut edit = self.design.footprint_edit(&reference).and_then(|e| e.pad(&pad.number, nth)).cloned().unwrap_or_else(|| PadEdit::none(&pad.number, nth));
                    edit.stack = (stack != normalized_stack(&library.pads[index].ext.stack)).then_some(stack);
                    self.edit_board_pad(&reference, &edit)?;
                    changed += 1;
                }
            }
        }

        if req.vias {
            let vias: Vec<(String, String, String)> = self
                .design
                .routing
                .as_ref()
                .map(|rt| rt.vias.iter().filter(|v| req.selected.is_none() || via_ids.contains(&v.id)).map(|v| (v.id.clone(), v.from_layer.clone(), v.to_layer.clone())).collect())
                .unwrap_or_default();
            for (id, from, to) in vias {
                let kind = self.design.drawings.as_ref().and_then(|d| d.via_extra(&id)).map(|e| e.kind).unwrap_or_default();
                // `viaHasPotentiallyUnusedLayers`.
                let potentially_unused = if kind == ViaKind::Through {
                    copper_layers > 2
                } else {
                    match layer_depth(&from, &to, copper_layers) {
                        Some(depth) => depth > 1,
                        None => copper_layers > 2,
                    }
                };
                if !potentially_unused {
                    continue;
                }
                let current = self.design.drawings.as_ref().and_then(|d| d.via_extra(&id)).map(ViaExtra::unconnected_layers).unwrap_or_default();
                if current == target {
                    continue;
                }
                let dr = self.drawings_mut();
                let i = match dr.via_extras.iter().position(|e| e.id == id) {
                    Some(i) => i,
                    None => {
                        dr.via_extras.push(ViaExtra { id: id.clone(), ..Default::default() });
                        dr.via_extras.len() - 1
                    }
                };
                dr.via_extras[i].set_unconnected_layers(target);
                if dr.via_extras[i].is_default() {
                    dr.via_extras.remove(i);
                }
                changed += 1;
            }
        }

        if changed == 0 {
            return Err(refuse(
                "ops_nothing_to_change",
                "remove_unused_pads",
                if req.remove { "No through-hole pad or via of the choice has a layer left to remove." } else { "Every through-hole pad and via of the choice keeps all its layers." },
            ));
        }
        Ok(())
    }
}
