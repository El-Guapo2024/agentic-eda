//! Place > Auto-Place Footprints: "Place Selected Footprints" and "Place Off-Board Footprints" (UI-LEFT P19).
//!
//! KiCad's two actions (`pcbnew.Autoplacer.autoplaceSelected`, `pcbnew.Autoplacer.autoplaceOffboard`; `AUTOPLACE_TOOL::autoplaceSelected`,
//! `autoplaceOffboard` and `autoplace` in pcbnew/autorouter/autoplace_tool.cpp at 8303b2ad) gather the footprints, drop the locked ones unless
//! Override locks is on, and hand them to `AR_AUTOPLACER::AutoplaceFootprints`, which searches a routing matrix of the board's cells for each one's
//! cheapest spot (`ar_autoplacer.cpp`, `ar_matrix.cpp`); the result is one commit, "Autoplace Footprints".
//!
//! The studio runs its own placer instead (the user's choice, 2026-11-05: expose our placer through KiCad's two menu items rather than port
//! `AR_AUTOPLACER`): [`eda_place::place_partial`], the annealer, legaliser, polish and rule repair of `eda board place`, with every footprint not asked
//! for an obstacle that stays where it is. What differs from KiCad is the search, not the frame: KiCad's cost is the ratsnest length plus a penalty
//! for the cells a footprint's keep-out takes from the routing matrix, tried footprint by footprint in order of size and connections; ours anneals the
//! footprints asked for together against the placement gates' own model -- half-perimeter wirelength, courtyard and refdes keep-outs, the proximity,
//! separation and decoupling rules, edge connectors on an edge, the escape of boxed pads, routing congestion -- and the command is then judged by the
//! placement gates like every board edit ([`crate::Cmd::always_judged`]: a placement that adds a gate failure is refused, with the failures named).
//!
//! The footprints come as KiCad gathers them:
//! * "Place Selected Footprints": the footprints of the selection (`item->Type() == PCB_FOOTPRINT_T`; a selected pad or field is not its footprint);
//! * "Place Off-Board Footprints": every footprint whose position is not on the board (`!boardShape.Contains( footprint->GetPosition() )`, the board's
//!   outlines with their holes), and -- the studio's counterpart of the footprints KiCad's Update PCB from Schematic stacks beside the board -- every
//!   part of the design that is not on the board yet (but one whose symbol is excluded from the board);
//! * without Override locks a locked footprint is left out (`fp->IsLocked()`, a footprint in a locked group included).
//!
//! A board with no outline is refused in KiCad's words ("Board edges must be defined on the Edge.Cuts layer."); nothing to place is refused too
//! (KiCad's commit is then empty and nothing happens; a verb that changes nothing would still be an undo step here). The placer's own refusal (it could
//! not place the footprints clear of the others inside the board) comes back with the offenders. The positions are whole steps of `grid_nm` (the
//! editor's connected-items grid; the board's placement step when absent), a footprint keeps its side, and its rotation is the quarter turn the placer
//! chooses. Tracks routed to a moved footprint stay where they were, as after any studio edit.

use crate::Board;
use eda_model::ir::Nm;
use eda_model::CheckResult;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One run of the autoplacer. On the wire: `{"op": "autoplace", "footprints": ["R1", "R2"]}` (Place Selected Footprints) or `{"op": "autoplace",
/// "offboard": true}` (Place Off-Board Footprints), with `grid_nm` and `override_locks` when they apply.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AutoplaceCmd {
    /// "Place Off-Board Footprints": the footprints off the board and the parts not on it; `footprints` is not read.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub offboard: bool,
    /// "Place Selected Footprints": the footprints selected, by reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub footprints: Vec<String>,
    /// The grid the positions are whole steps of (the editor's grid for footprints, `PCB_GRID_HELPER::GetGridSize( GRID_CONNECTABLE )`); absent: the board's
    /// placement step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid_nm: Option<Nm>,
    /// `PCB_BASE_EDIT_FRAME::GetOverrideLocks()`: a locked footprint is placed too.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub override_locks: bool,
}

impl AutoplaceCmd {
    /// The activity log's line.
    pub fn describe(&self) -> String {
        let what = if self.offboard { "--offboard".to_string() } else { format!("--selected {}", self.footprints.join(",")) };
        let grid = self.grid_nm.map(|g| format!(" --grid {:.4}", eda_model::units::to_mm(g))).unwrap_or_default();
        format!("autoplace {what}{grid}{}", if self.override_locks { " --override-locks" } else { "" })
    }

    pub fn ids(&self) -> Vec<&str> {
        if self.offboard {
            vec!["offboard"]
        } else {
            self.footprints.iter().map(String::as_str).collect()
        }
    }
}

/// `wxString::Format( _( "Board edges must be defined on the %s layer." ), LayerName( Edge_Cuts ) )`.
pub const NO_BOARD_EDGES: &str = "Board edges must be defined on the Edge.Cuts layer.";

impl Board<'_> {
    /// [`crate::Cmd::Autoplace`].
    pub(crate) fn apply_autoplace(&mut self, cmd: &AutoplaceCmd) -> Result<(), Vec<CheckResult>> {
        // `BOX2I bbox = board()->GetBoardEdgesBoundingBox(); if( bbox.GetWidth() == 0 || bbox.GetHeight() == 0 )`
        let outline = self.design.placement.as_ref().map(|p| p.outline.clone()).unwrap_or_default();
        let width = outline.iter().map(|p| p.x).max().unwrap_or(Nm::ZERO) - outline.iter().map(|p| p.x).min().unwrap_or(Nm::ZERO);
        let height = outline.iter().map(|p| p.y).max().unwrap_or(Nm::ZERO) - outline.iter().map(|p| p.y).min().unwrap_or(Nm::ZERO);
        if outline.len() < 3 || width == Nm::ZERO || height == Nm::ZERO {
            return Err(vec![CheckResult::fail("ops_autoplace_no_board", "Edge.Cuts", NO_BOARD_EDGES)]);
        }
        let placed = self.placed();
        let mut footprints: Vec<String> = if cmd.offboard {
            // `board()->GetBoardPolygonOutlines( boardShape, true ); if( !boardShape.Contains( footprint->GetPosition() ) )`
            let region = self.board_region();
            let off: Vec<String> = self.placement().footprints.iter().filter(|f| !region.contains(f.at)).map(|f| f.id.clone()).collect();
            // and the parts not on the board yet: what KiCad's Update PCB from Schematic would have stacked beside it -- not one whose symbol is excluded
            // from the board, which Update PCB leaves off it
            let excluded: BTreeSet<String> = eda_model::sheet_list::walk(&self.design)
                .iter()
                .filter_map(|e| eda_model::sheet_list::screen_of(&self.design, &e.path).map(|sec| (sec, e.path.clone())))
                .flat_map(|(sec, path)| sec.symbols.iter().filter(|s| s.exclude_from_board).map(move |s| eda_model::sheet_list::reference_under(sec, s, &path)))
                .collect();
            let unplaced = self.model.parts.iter().map(|p| p.reference.clone()).filter(|r| !placed.contains(r) && !excluded.contains(r) && self.model.part(r).and_then(|p| self.model.footprint_of(p)).is_some());
            off.into_iter().chain(unplaced).collect()
        } else {
            // `for( EDA_ITEM* item : selection() ) if( item->Type() == PCB_FOOTPRINT_T )`
            let mut seen = BTreeSet::new();
            cmd.footprints.iter().filter(|r| placed.contains(*r) && seen.insert(r.as_str())).cloned().collect()
        };
        // `if( !frame()->GetOverrideLocks() ) aFootprints.erase( remove_if( .., fp->IsLocked() ) )`
        if !cmd.override_locks {
            footprints.retain(|r| !self.locked_with_group(r));
        }
        if footprints.is_empty() {
            return Err(vec![CheckResult::fail("ops_autoplace_nothing", "autoplace", if cmd.offboard { "There are no footprints off the board to place." } else { "There are no unlocked footprints selected to place." })]);
        }
        let movable: BTreeSet<String> = footprints.iter().cloned().collect();
        let solver = &self.model.solver;
        let opts = eda_place::PlaceOptions {
            seed: 0,
            snap: cmd.grid_nm.filter(|g| *g > Nm::ZERO).unwrap_or(self.snap).max(Nm::new(1)),
            moves_per_part: solver.place_moves_per_part,
            outline: Some(outline),
            spacing: solver.place_spacing_nm,
            fit_floor: 0.0,
            floorplan: None,
        };
        let poses = eda_place::place_partial(&self.design, self.model, &opts, &movable)?;
        let fps = &mut self.design.placement.as_mut().expect("the board has an outline, so a placement").footprints;
        for pose in poses {
            match fps.iter_mut().find(|f| f.id == pose.id) {
                // the position and the turn the placer chose; the side and the reference's side as the placer left them
                Some(f) => {
                    f.at = pose.at;
                    f.rot = pose.rot;
                    f.side = pose.side;
                    f.label = pose.label;
                }
                None => fps.push(pose),
            }
        }
        self.sort_footprints();
        Ok(())
    }

    /// `BOARD_ITEM::IsLocked()` of a footprint: its own lock, or the lock of a group it is in (at any depth).
    fn locked_with_group(&self, id: &str) -> bool {
        let Some(d) = self.design.drawings.as_ref() else { return false };
        let locked = |x: &str| d.locked_ids.iter().any(|l| l == x);
        if locked(id) {
            return true;
        }
        let mut at = id.to_string();
        // a group cannot hold itself; the depth bound guards a malformed file
        for _ in 0..64 {
            match d.parent_group(&at) {
                Some(g) if locked(&g.id) => return true,
                Some(g) => at = g.id.clone(),
                None => return false,
            }
        }
        false
    }
}
