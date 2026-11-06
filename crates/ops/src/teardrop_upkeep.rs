//! The teardrop update every board edit runs (UI-LEFT-2 PP6): KiCad 25ec2335's `BOARD_COMMIT::Push` collects, for the items its commit names, the tracks, vias
//! and pads whose teardrops it made stale (`staleTeardropPadsAndVias`, `staleTeardropTracks`) and the copper it moved (`dirtyCopper`), and before it lets go of
//! the commit runs `TEARDROP_MANAGER::RemoveTeardrops` and `UpdateTeardrops( *this, &staleTeardropPadsAndVias, &staleTeardropTracks )`, so a track moved on a
//! via takes its teardrop along, a teardrop near copper of another net that moved is fitted again, and all of it is the edit's own undo step.
//!
//! Here [`Board::apply`](crate::Board::apply) takes the board's copper before the command ([`eda_connectivity::teardrop::snapshot`]: nothing when the board has no
//! teardrop and nothing that could have one) and, once the command is applied, hands both states to [`eda_connectivity::teardrop::update_after_edit`], which
//! finds what changed and updates the teardrops it touches. A [`Cmd::Batch`](crate::Cmd::Batch) is one commit: the update runs once, after its last command.
//!
//! The commands KiCad pushes with `SKIP_TEARDROPS` skip it (Edit Teardrops, which rebuilds them all itself: `Cmd::AddAllTeardrops`, `Cmd::RemoveAllTeardrops`;
//! Lock and Unlock: `Cmd::SetLocked`), as do Board Setup's page (`Cmd::SetTeardropSettings`: in KiCad it changes no item's parameters, which Edit Teardrops
//! seeds), a board brought in whole (`Cmd::ReplaceBoard`) and the schematic's and the Footprint Editor's commands. **Differs:** the board's `(legacy_teardrops
//! yes)` is not kept (the reader drops it), so every board's teardrops are kept current, where KiCad leaves a legacy board's alone (`UpdateTeardrops` returns at
//! once) -- and the studio writes every board with teardrops so, only for kicad-cli to keep the ones it is given (`eda_kicad::pcb`).

use crate::{Board, Cmd, Domain};
use eda_connectivity::teardrop::TeardropWorld;
use eda_model::ConstraintModel;

impl Cmd {
    /// Whether the command is pushed without the teardrop update: see the module doc.
    pub(crate) fn skips_teardrop_update(&self) -> bool {
        matches!(self, Cmd::AddAllTeardrops | Cmd::RemoveAllTeardrops | Cmd::SetTeardropSettings { .. } | Cmd::SetLocked { .. } | Cmd::ReplaceBoard { .. })
            || matches!(self.domain(), Domain::Schematic | Domain::FootprintEditor)
    }

    /// Whether the command can give an item teardrop parameters of its own that are on (Pad Properties, Track & Via Properties, the Properties panel): the board
    /// before it is looked at even when nothing on it could have a teardrop yet.
    fn may_give_teardrops(&self) -> bool {
        match self {
            Cmd::SetTeardropParams { .. } | Cmd::EditBoardPad { .. } | Cmd::SetViaProps { .. } => true,
            Cmd::Batch { cmds } => cmds.iter().any(Cmd::may_give_teardrops),
            _ => false,
        }
    }
}

impl Board<'_> {
    /// The board's copper before `cmd`, for [`Board::tidy_teardrops`]; `None` when the command skips the update or nothing on the board can have a teardrop.
    pub(crate) fn teardrops_before(&self, cmd: &Cmd) -> Option<TeardropWorld> {
        if cmd.skips_teardrop_update() {
            return None;
        }
        if cmd.may_give_teardrops() {
            return Some(TeardropWorld::of(&self.design, self.model));
        }
        eda_connectivity::teardrop::snapshot(&self.design, self.model)
    }

    /// The teardrop update for a command whose model was made again after it (a pad's shape edited in Pad Properties: `eda` folds the board's pad edits into the
    /// model's footprints once the command is applied): the pads whose shape the model `was` had differently are stale, and their teardrops are made again on the
    /// pads as they are now, in the same step. Returns whether the teardrops changed.
    pub fn refresh_teardrops(&mut self, cmd: &Cmd, was: &ConstraintModel) -> bool {
        if cmd.skips_teardrop_update() || !eda_connectivity::teardrop::teardrops_possible(&self.design) {
            return false;
        }
        let before = TeardropWorld::of(&self.design, was);
        let changed = eda_connectivity::teardrop::update_after_edit(&mut self.design, self.model, &before);
        if changed {
            if let Some(rt) = self.design.routing.as_mut() {
                rt.assign_missing_ids();
            }
            self.sort_routing();
        }
        changed
    }

    /// `BOARD_COMMIT::Push`'s teardrop update after a command, from the board as it was (`before`): the teardrops it made stale made again, new ones given ids.
    pub(crate) fn tidy_teardrops(&mut self, before: Option<TeardropWorld>) {
        let Some(before) = before else { return };
        if eda_connectivity::teardrop::update_after_edit(&mut self.design, self.model, &before) {
            if let Some(rt) = self.design.routing.as_mut() {
                rt.assign_missing_ids();
            }
            self.sort_routing();
        }
    }
}
