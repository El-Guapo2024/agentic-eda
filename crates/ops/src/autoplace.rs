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
//!
//! The placer works in the outline's bounding box; what of that box is not board -- the cutouts, and the corners outside an outline that is not a
//! rectangle -- goes to it as obstacles ([`off_board_rects`], from the board's outlines with their holes, `BOARD::GetBoardPolygonOutlines`), so no part
//! is put in a hole (UI-LEFT-2 PP20). The run reports to [`AUTOPLACE_WATCH`], which the studio's progress window reads and its Abort cancels
//! (`WX_PROGRESS_REPORTER( frame(), _( "Autoplace Footprints" ), 1, PR_CAN_ABORT )`): an aborted run places nothing (`commit.Revert()`).

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

/// The watcher of the autoplacer's run in this process: the studio's `/api/autoplace/progress` reads it and `/api/autoplace/abort` cancels it.
pub static AUTOPLACE_WATCH: eda_place::PlaceWatch = eda_place::PlaceWatch::new();

/// Marks [`AUTOPLACE_WATCH`]'s run over however the command ends.
struct WatchRun;

impl Drop for WatchRun {
    fn drop(&mut self) {
        AUTOPLACE_WATCH.finish();
    }
}

/// The parts of the box `bbox` that are not inside `rings` (even-odd: outlines and their holes), as rectangles covering them: the obstacles a
/// placer that works in the box keeps the parts out of.
///
/// The box is cut into horizontal strips at every vertex's y; within a strip every edge is a straight line from top to bottom, so what is board all
/// across the strip is the intersection of the board's spans just inside its top and its bottom, and the rest of the strip's width is blocked
/// (rounded outward, so a slanted edge leaves no sliver uncovered). Strips that block the same spans are joined; past `MAX_STRIPS` the two adjacent
/// strips of the least height together are joined, keeping only what is board across both (more is blocked, never less).
pub fn off_board_rects(rings: &[Vec<(i64, i64)>], bbox: (Nm, Nm, Nm, Nm)) -> Vec<(Nm, Nm, Nm, Nm)> {
    const MAX_STRIPS: usize = 32;
    let (bx0, by0, bx1, by1) = (bbox.0.get() as f64, bbox.1.get() as f64, bbox.2.get() as f64, bbox.3.get() as f64);
    if bx1 <= bx0 || by1 <= by0 || rings.iter().all(|r| r.len() < 3) {
        return Vec::new();
    }
    // The board's spans along the line at `y`: the crossings of every edge, sorted, taken in pairs.
    let spans_at = |y: f64| -> Vec<(f64, f64)> {
        let mut xs: Vec<f64> = Vec::new();
        for ring in rings.iter().filter(|r| r.len() >= 3) {
            for k in 0..ring.len() {
                let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
                let (ay, by) = (a.1 as f64, b.1 as f64);
                if (ay <= y) != (by <= y) {
                    xs.push(a.0 as f64 + (y - ay) / (by - ay) * (b.0 - a.0) as f64);
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        xs.chunks_exact(2).map(|c| (c[0], c[1])).collect()
    };
    let intersect = |a: &[(f64, f64)], b: &[(f64, f64)]| -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        for &(a0, a1) in a {
            for &(b0, b1) in b {
                let (l, r) = (a0.max(b0), a1.min(b1));
                if r > l {
                    out.push((l, r));
                }
            }
        }
        out.sort_by(|p, q| p.0.total_cmp(&q.0));
        out
    };
    let mut ys: Vec<f64> = rings.iter().flatten().map(|p| p.1 as f64).filter(|y| *y > by0 && *y < by1).chain([by0, by1]).collect();
    ys.sort_by(f64::total_cmp);
    ys.dedup();
    // (top, bottom, the board's spans all across)
    let mut strips: Vec<(f64, f64, Vec<(f64, f64)>)> = Vec::new();
    for w in ys.windows(2) {
        let (ya, yb) = (w[0], w[1]);
        let e = ((yb - ya) / 4.0).min(1.0);
        let spans = intersect(&spans_at(ya + e), &spans_at(yb - e));
        let rounded = |v: &[(f64, f64)]| v.iter().map(|s| (s.0.round() as i64, s.1.round() as i64)).collect::<Vec<_>>();
        match strips.last_mut() {
            Some(last) if rounded(&last.2) == rounded(&spans) => last.1 = yb,
            _ => strips.push((ya, yb, spans)),
        }
    }
    while strips.len() > MAX_STRIPS {
        let k = (0..strips.len() - 1).min_by(|&a, &b| (strips[a + 1].1 - strips[a].0).total_cmp(&(strips[b + 1].1 - strips[b].0))).unwrap_or(0);
        let next = strips.remove(k + 1);
        strips[k] = (strips[k].0, next.1, intersect(&strips[k].2, &next.2));
    }
    let mut out = Vec::new();
    for (ya, yb, spans) in strips {
        let (top, bottom) = (Nm::new(ya.floor() as i64), Nm::new(yb.ceil() as i64));
        let mut x = bx0;
        for (l, r) in spans.iter().copied().chain([(bx1, bx1)]) {
            let (l, r) = (l.max(bx0).min(bx1), r.max(bx0).min(bx1));
            if l > x {
                out.push((Nm::new(x.floor() as i64), top, Nm::new(l.ceil() as i64), bottom));
            }
            x = x.max(r);
        }
    }
    out
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
            outline: Some(outline.clone()),
            spacing: solver.place_spacing_nm,
            fit_floor: 0.0,
            floorplan: None,
        };
        // What of the outline's box is not board -- its cutouts, the corners an outline that is not a rectangle leaves -- the placer keeps the parts off
        let region = self.board_region();
        let polys = &region.polys;
        let rings: Vec<Vec<(i64, i64)>> = (0..polys.outline_count())
            .flat_map(|i| std::iter::once(polys.outline(i)).chain((0..polys.hole_count(i)).map(move |h| polys.hole(i, h))))
            .map(|ring| ring.iter().map(|p| (p.x, p.y)).collect())
            .collect();
        let bbox = (outline.iter().map(|p| p.x).min().unwrap_or(Nm::ZERO), outline.iter().map(|p| p.y).min().unwrap_or(Nm::ZERO), outline.iter().map(|p| p.x).max().unwrap_or(Nm::ZERO), outline.iter().map(|p| p.y).max().unwrap_or(Nm::ZERO));
        let blocked = off_board_rects(&rings, bbox);
        AUTOPLACE_WATCH.start();
        let _run = WatchRun;
        let poses = eda_place::place_partial_watched(&self.design, self.model, &opts, &movable, &blocked, Some(&AUTOPLACE_WATCH))?;
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
