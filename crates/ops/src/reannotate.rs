//! Geographical Reannotate on the board (`Cmd::ReannotateBoard`): `pcbnew.ReannotateTool.ShowReannotateDialog`, `DIALOG_BOARD_REANNOTATE::ReannotateBoard`.
//!
//! The plan is `eda_model::reannotate::plan` (the dialog's `BuildFootprintList`, `BuildUnavailableRefsList` and `BuildChangeArray`, over the footprints on the board). Applying it is
//! where the studio goes beyond KiCad: in KiCad the dialog sets the footprints' references and says "PCB annotation changes should be synchronized with schematic using
//! 'Update Schematic from PCB'", because the netlist ties a footprint to its symbol by path and uuid. Here a part is its reference -- the key of its symbol, its pose, its
//! footprint, its copper, its fields and edits --, so the symbols take the new references in the same step, as the back-annotation of "Update Schematic from PCB" would have
//! them take, and the board's parts move with [`Board::rename_part`]: every renumbered reference at once (the numbers of two parts can swap), one undo step on both editors,
//! no part, pose or route left under an old reference.

use super::fp_exchange::footprint_name_of;
use super::Board;
use eda_model::fp_edit::default_reference_layout;
use eda_model::reannotate::{plan, Location, Plan, ReannotateInput, ReannotateOptions};
use eda_model::CheckResult;
use std::collections::BTreeSet;

/// `LIB_ID::IsValid`: a library nickname, a colon and an item name.
fn is_library_id(id: &str) -> bool {
    id.split_once(':').is_some_and(|(library, item)| !library.is_empty() && !item.is_empty())
}

impl Board<'_> {
    /// The footprints on the board as the dialog reads them, and the references of the design's parts that have no footprint on it (a symbol not placed yet): their numbers are not
    /// given out, because a part is its reference and two parts cannot have one.
    fn reannotate_inputs(&self, opts: &ReannotateOptions, selection: &[String]) -> (Vec<ReannotateInput>, Vec<String>) {
        let locked: BTreeSet<&str> = self.design.drawings.iter().flat_map(|d| d.locked_ids.iter().map(String::as_str)).collect();
        let selected: BTreeSet<&str> = selection.iter().map(String::as_str).collect();
        let mut inputs = Vec::new();
        for f in &self.placement().footprints {
            let part = self.model.part(&f.id);
            let name = part.map(footprint_name_of).unwrap_or_default();
            // `footprint->Reference().GetPosition()`: where the Reference text is on the board, its own layout or the one a footprint is given
            let at = match (opts.location, part) {
                (Location::Reference, Some(part)) => {
                    let layout = self.design.footprint_edit(&f.id).and_then(|e| e.reference.clone()).or_else(|| self.model.footprint_of(part).map(|fp| default_reference_layout(f, &fp)));
                    layout.map_or(f.at, |l| l.board_position(f))
                }
                _ => f.at,
            };
            inputs.push(ReannotateInput {
                reference: f.id.clone(),
                fpid: if is_library_id(&name) { name } else { String::new() },
                x: at.x,
                y: at.y,
                front: f.side == eda_model::ir::Side::Top,
                locked: locked.contains(f.id.as_str()),
                selected: selected.contains(f.id.as_str()),
            });
        }
        let on_board: BTreeSet<&str> = inputs.iter().map(|i| i.reference.as_str()).collect();
        let mut elsewhere: BTreeSet<String> = self.model.parts.iter().map(|p| p.reference.clone()).filter(|r| !on_board.contains(r.as_str())).collect();
        elsewhere.extend(self.symbol_references().into_iter().filter(|r| !on_board.contains(r.as_str())));
        (inputs, elsewhere.into_iter().collect())
    }

    /// What the dialog's run works out for the board as it is: the lists it logs, the change log and any duplicate. Nothing is changed.
    pub fn reannotate_plan(&self, opts: &ReannotateOptions, selection: &[String]) -> Plan {
        let (inputs, elsewhere) = self.reannotate_inputs(opts, selection);
        plan(&inputs, &elsewhere, opts)
    }

    /// `Cmd::ReannotateBoard`: refused as a whole when two footprints would be left with one reference ("Selected options resulted in errors! Change them and try again.");
    /// otherwise the footprints get their new references and the symbols and the board's parts go with them, all at once.
    pub(crate) fn reannotate_board(&mut self, opts: &ReannotateOptions, selection: &[String]) -> Result<(), Vec<CheckResult>> {
        let plan = self.reannotate_plan(opts, selection);
        if !plan.ok() {
            return Err(plan.errors.iter().map(|e| CheckResult::fail("ops_reannotate_duplicate", "reannotate", e)).collect());
        }
        let renames = plan.renames();
        if renames.is_empty() {
            return Ok(());
        }
        // the symbols (on every sheet, with the pins of their wires, their fields and the rest that is keyed by the reference), then the board's parts, in one go
        let symbols = self.reference_renames(&renames);
        self.annotate_apply(&symbols, false);
        self.rename_part(&renames);
        Ok(())
    }
}
