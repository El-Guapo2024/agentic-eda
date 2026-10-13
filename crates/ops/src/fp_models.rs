//! The 3D models of a footprint on the board: the verbs behind the 3D Models tab of the Footprint Properties dialog (`PANEL_FP_PROPERTIES_3D_MODEL`,
//! `pcbnew/dialogs/panel_fp_properties_3d_model.cpp`, and the preview panel's fields, `3d-viewer/dialogs/panel_preview_3d_model.cpp`).
//!
//! * [`Cmd::SetBoardModels`](crate::Cmd::SetBoardModels) replaces the whole list -- `DIALOG_FOOTPRINT_PROPERTIES::TransferDataFromWindow`'s
//!   `m_footprint->Models() = ...` -- and is what the dialog's OK sends (Migrate 3D Models sends the same);
//! * [`Cmd::AddBoardModel`](crate::Cmd::AddBoardModel), [`Cmd::RemoveBoardModel`](crate::Cmd::RemoveBoardModel), [`Cmd::MoveBoardModel`](crate::Cmd::MoveBoardModel) and
//!   [`Cmd::EditBoardModel`](crate::Cmd::EditBoardModel) are the tab's Add, Remove and cell edits, and the order of the rows, one at a time.
//!
//! The list lives in [`FootprintEdit::models3d`], the whole of it (the file's `(model ...)` entries, in the frame of the footprint on the top side), and the loader lays it over
//! the footprint ([`eda_model::fp_edit::edited_footprint`]) so that the 3D view, the `.kicad_pcb` writer and the exports all see it. A list that comes to what the library or the
//! board gives the footprint is not kept as an edit, so an update from the library still brings its models. Every verb is one undo step, and one that changes
//! nothing is refused so that no empty step is pushed.

use super::Board;
use eda_model::footprint::Model3d;
use std::collections::BTreeMap;
use eda_model::fp_models::{self, ModelPatch, ModelProblem};
use eda_model::CheckResult;

fn refuse(part: &str, p: ModelProblem) -> Vec<CheckResult> {
    vec![CheckResult::fail(p.check, part, p.reason)]
}

impl Board<'_> {
    /// The models the footprint of `part` has by itself: its library's, or its board's, or the package's generic ones -- what it has with nothing set on the board.
    fn default_models(&self, part: &str) -> Vec<Model3d> {
        let Some(p) = self.model.part(part) else { return Vec::new() };
        let Some(footprint) = self.unedited_footprint(p) else { return Vec::new() };
        eda_kicad::default_part_models(self.model, p, &footprint).map(|m| m.models).unwrap_or_default()
    }

    /// The list the footprint has now: the one the board set, else the one it has by itself. The footprint must be on the board.
    fn models_now(&self, part: &str) -> Result<Vec<Model3d>, Vec<CheckResult>> {
        self.require_placed(part)?;
        match self.design.footprint_edit(part).and_then(|e| e.models3d.clone()) {
            Some(list) => Ok(list),
            None => Ok(self.default_models(part)),
        }
    }

    /// Keep `list` as the footprint's models: as the board's edit, or -- when it is what the footprint has by itself -- no edit at all.
    fn store_models(&mut self, part: &str, list: Vec<Model3d>) {
        let by_itself = self.default_models(part) == list;
        self.edit_slot(part).models3d = (!by_itself).then_some(list);
        self.tidy_edit(part);
    }

    /// The list after `change`, refused when it comes to the one the footprint has.
    fn change_models(&mut self, part: &str, change: impl FnOnce(&mut Vec<Model3d>) -> Result<(), ModelProblem>) -> Result<(), Vec<CheckResult>> {
        let was = self.models_now(part)?;
        let mut list = was.clone();
        change(&mut list).map_err(|p| refuse(part, p))?;
        if list == was {
            return Err(refuse(part, ModelProblem { check: "ops_nothing_to_change", reason: "the 3D models are as they were".into() }));
        }
        self.store_models(part, list);
        Ok(())
    }

    /// `Cmd::SetBoardModels`.
    pub(crate) fn set_board_models(&mut self, part: &str, models: &[Model3d]) -> Result<(), Vec<CheckResult>> {
        self.require_placed(part)?;
        let checked = fp_models::checked_models(models).map_err(|p| refuse(part, p))?;
        self.change_models(part, |list| {
            *list = checked;
            Ok(())
        })
    }

    /// `Cmd::AddBoardModel`.
    pub(crate) fn add_board_model(&mut self, part: &str, model: &Model3d, at: Option<usize>) -> Result<(), Vec<CheckResult>> {
        self.change_models(part, |list| fp_models::add_model(list, model, at).map(|_| ()))
    }

    /// `Cmd::RemoveBoardModel`.
    pub(crate) fn remove_board_model(&mut self, part: &str, index: usize) -> Result<(), Vec<CheckResult>> {
        self.change_models(part, |list| fp_models::remove_model(list, index).map(|_| ()))
    }

    /// `Cmd::MoveBoardModel`.
    pub(crate) fn move_board_model(&mut self, part: &str, from: usize, to: usize) -> Result<(), Vec<CheckResult>> {
        self.change_models(part, |list| fp_models::move_model(list, from, to))
    }

    /// `Cmd::MigrateBoardModels` (`DIALOG_MIGRATE_3D_MODELS::OnReplaceClick`): the footprints on the board whose list has a model under one of the old names get it under the new, the
    /// rest of the placement as it was. A footprint's list is then its own, as the 3D Models tab's: one that comes to the library's takes the edit away.
    pub(crate) fn migrate_board_models(&mut self, replacements: &BTreeMap<String, String>) -> Result<(), Vec<CheckResult>> {
        if replacements.is_empty() {
            return Err(refuse("models", ModelProblem { check: "ops_nothing_to_change", reason: "no model is to be replaced".into() }));
        }
        let mut names: BTreeMap<&str, String> = BTreeMap::new();
        for (old, new) in replacements {
            let m = fp_models::checked_model(&Model3d::identity(new.as_str())).map_err(|p| refuse(old, ModelProblem { reason: format!("the new name for {old}: {}", p.reason), ..p }))?;
            names.insert(old.as_str(), m.path);
        }
        let parts: Vec<String> = self.design.placement.as_ref().map(|p| p.footprints.iter().map(|f| f.id.clone()).collect()).unwrap_or_default();
        let mut changed = 0;
        for part in parts {
            let now = self.models_now(&part)?;
            let next: Vec<Model3d> = now.iter().map(|m| names.get(m.path.as_str()).map_or_else(|| m.clone(), |new| Model3d { path: new.clone(), ..m.clone() })).collect();
            if next != now {
                self.store_models(&part, next);
                changed += 1;
            }
        }
        if changed == 0 {
            return Err(refuse("models", ModelProblem { check: "ops_nothing_to_change", reason: "no footprint on the board has a model under those names".into() }));
        }
        Ok(())
    }

    /// `Cmd::EditBoardModel`.
    pub(crate) fn edit_board_model(&mut self, part: &str, index: usize, patch: &ModelPatch) -> Result<(), Vec<CheckResult>> {
        self.change_models(part, |list| fp_models::edit_model(list, index, patch))
    }
}
