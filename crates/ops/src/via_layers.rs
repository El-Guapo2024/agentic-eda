//! Track & Via Properties' via rows that are not sizes (`pcbnew/dialogs/dialog_track_via_properties.cpp`, `TransferDataFromWindow`'s `case PCB_VIA_T`):
//! the via type (`m_ViaTypeChoice`: Through, Micro, Blind, Buried), the layer pair (`m_ViaStartLayer`, `m_ViaEndLayer`) and the annular rings
//! (`m_annularRingsCtrl`: "All copper layers", "Start, end, and connected layers", "Connected layers only", "Start and end layers only"), over every via the
//! dialog was opened on. A row left indeterminate (`None`) is left alone on each via.
//!
//! Per via, in KiCad's order: `SetViaType`, then the start and end layers (`SetTopLayer` / `SetBottomLayer`), then `SanitizeLayers` (a through via spans
//! `F_Cu`..`B_Cu`; a pair given back to front is swapped, `IsCopperLayerLowerThan`), then `Padstack().SetUnconnectedLayerMode`. The checks are
//! `PCB_VIA::ValidateViaParameters`' layer ones, in its words: "Via layer must be a copper layer.", "Via layer is outside the board stack.", "Via start
//! layer and end layer cannot be the same". One undo step for the whole dialog (`m_commit.Push`).
//!
//! The type is the via's [`ViaExtra::kind`], the rings its tokens ([`ViaExtra::set_unconnected_layers`]); a via left with KiCad's defaults keeps no entry.

use super::Board;
use eda_model::padstack::UnconnectedLayers;
use eda_model::via_extra::{ViaExtra, ViaKind};
use eda_model::CheckResult;
use serde::{Deserialize, Serialize};

/// The via rows of the dialog; `None` is an indeterminate row.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ViaLayerEdit {
    #[serde(default)]
    pub kind: Option<ViaKind>,
    #[serde(default)]
    pub from_layer: Option<String>,
    #[serde(default)]
    pub to_layer: Option<String>,
    #[serde(default)]
    pub unconnected: Option<UnconnectedLayers>,
}

fn refuse(what: &str, message: &str) -> Vec<CheckResult> {
    vec![CheckResult::fail("ops_bad_via", what, message)]
}

/// The copper layer's place in the stack, front first (`IsCopperLayerLowerThan`'s order): `F.Cu` 0, `In1.Cu` 1, .., `B.Cu` last.
fn stack_rank(layer: &str) -> Option<u32> {
    match layer {
        "F.Cu" => Some(0),
        "B.Cu" => Some(u32::MAX),
        _ => layer.strip_prefix("In").and_then(|r| r.strip_suffix(".Cu")).and_then(|n| n.parse::<u32>().ok()).filter(|n| (1..=30).contains(n)),
    }
}

impl Board<'_> {
    /// `Cmd::SetViaLayers`. See the module doc.
    pub(crate) fn set_via_layers(&mut self, ids: &[String], edit: &ViaLayerEdit) -> Result<(), Vec<CheckResult>> {
        let board_copper = self.model.board.layers.clone();
        // `ValidateViaParameters`' `validateLayer`: a copper layer, and one of the board's.
        for layer in [&edit.from_layer, &edit.to_layer].into_iter().flatten() {
            if stack_rank(layer).is_none() {
                return Err(refuse(layer, "Via layer must be a copper layer."));
            }
            if !board_copper.iter().any(|l| l == layer) {
                return Err(refuse(layer, "Via layer is outside the board stack."));
            }
        }
        if let (Some(a), Some(b)) = (&edit.from_layer, &edit.to_layer) {
            if a == b {
                return Err(refuse(a, "Via start layer and end layer cannot be the same"));
            }
        }
        if ids.is_empty() {
            return Err(refuse("vias", "no via to edit"));
        }
        let mut changed = false;
        for id in ids {
            let (from, to) = {
                let v = self.design.routing.as_ref().and_then(|rt| rt.vias.iter().find(|v| &v.id == id)).ok_or_else(|| refuse(id, "no via with this id"))?;
                (v.from_layer.clone(), v.to_layer.clone())
            };
            let extra_before = self.design.drawings.as_ref().and_then(|d| d.via_extra(id)).cloned().unwrap_or(ViaExtra { id: id.clone(), ..Default::default() });
            let mut extra = extra_before.clone();
            if let Some(kind) = edit.kind {
                extra.kind = kind;
            }
            let (mut top, mut bottom) = (edit.from_layer.clone().unwrap_or(from.clone()), edit.to_layer.clone().unwrap_or(to.clone()));
            // `SanitizeLayers`.
            if extra.kind == ViaKind::Through {
                (top, bottom) = ("F.Cu".to_string(), "B.Cu".to_string());
            }
            if stack_rank(&bottom) < stack_rank(&top) {
                std::mem::swap(&mut top, &mut bottom);
            }
            if top == bottom {
                return Err(refuse(id, "Via start layer and end layer cannot be the same"));
            }
            if let Some(mode) = edit.unconnected {
                extra.set_unconnected_layers(mode);
            }
            if extra != extra_before {
                changed = true;
                let dr = self.drawings_mut();
                match (dr.via_extras.iter().position(|e| &e.id == id), extra.is_default()) {
                    (Some(i), true) => {
                        dr.via_extras.remove(i);
                    }
                    (Some(i), false) => dr.via_extras[i] = extra,
                    (None, false) => dr.via_extras.push(extra),
                    (None, true) => {}
                }
            }
            if (top.as_str(), bottom.as_str()) != (from.as_str(), to.as_str()) {
                changed = true;
                let v = self.routing_mut().vias.iter_mut().find(|v| &v.id == id).expect("found above");
                v.from_layer = top;
                v.to_layer = bottom;
            }
        }
        if !changed {
            return Err(vec![CheckResult::fail("ops_nothing_to_change", "vias", "the vias already have this type, these layers and these annular rings")]);
        }
        Ok(())
    }
}
