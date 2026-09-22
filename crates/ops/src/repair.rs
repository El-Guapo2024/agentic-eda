//! Move parts that are already down, to fix what the gates report.
//!
//! Until now the build loop could only ever *decline* to place a part.
//! Nothing could move one that had already landed, so a violation
//! between two placed parts was permanent however small it was: on L4,
//! U12 and J23 sat 82.7 mm apart under an 8 mm rule and no verb in the
//! loop could close the gap.
//!
//! `Cmd::Nudge`, `Cmd::Swap` and `Cmd::Rotate` have existed since the
//! command set was written and the build loop had never emitted one.
//! This issues them.
//!
//! # Failures as vectors, not verdicts
//!
//! A gate says "U12/J23: 82.70 mm apart, rule allows 8 mm". That is a
//! verdict: true, exact, and impossible to act on directly. What a
//! repair needs from it is a direction and a distance -- which part is
//! free to move, which way, and how far -- so the same fact becomes
//! something to do rather than something that went wrong.
//!
//! [`Fix`] is that reading. It is also the shape a decision layer would
//! be given: the encoding exists so a model can propose the move a
//! person would, over the same verbs, judged by the same gates.
//!
//! # Every move is earned
//!
//! A repair is applied to a fork and kept only if the failure count
//! drops. Nothing here trusts its own reasoning about geometry -- the
//! gates decide, exactly as they do for placement. A proposed move that
//! makes things worse costs a fork and is discarded.

use crate::{Board, Cmd, Dir};
use eda_model::{CheckResult, ConstraintModel};
use std::collections::BTreeSet;

/// A failure read as something to do about it.
#[derive(Debug, Clone)]
pub struct Fix {
    /// The part to move: the one that is free.
    pub mover: String,
    /// What it should end up near.
    pub toward: String,
    /// How far apart they are now, µm.
    pub apart_um: i64,
    /// How far apart they are allowed to be, µm.
    pub allowed_um: i64,
    /// The gate that objected.
    pub check: String,
}

impl Fix {
    /// How far the mover has to travel, µm.
    pub fn close_um(&self) -> i64 {
        (self.apart_um - self.allowed_um).max(0)
    }
}

/// Parts that must not move: a connector's position is fixed by the
/// outside world, so a repair that drags one inward trades a proximity
/// failure for an edge failure.
fn pinned(model: &ConstraintModel) -> BTreeSet<String> {
    model
        .parts
        .iter()
        .filter(|p| eda_model::footprint::is_edge_connector(p))
        .map(|p| p.reference.clone())
        .collect()
}

/// Read the board's proximity failures as fixes.
///
/// Only proximity is read here. It is the failure whose repair is
/// unambiguous -- two named parts and a distance between them -- and
/// starting with the unambiguous case keeps the loop honest about what
/// it can actually do. A gate whose repair is not obvious is left
/// reported rather than guessed at.
pub fn fixes_for(board: &Board, model: &ConstraintModel) -> Vec<Fix> {
    let pin = pinned(model);
    let mut out = Vec::new();
    for c in board.checks() {
        if !matches!(c.status, eda_model::CheckStatus::Fail) || c.check != "placement_proximity" {
            continue;
        }
        let Some(loc) = c.location.as_deref() else { continue };
        let Some((a, b)) = loc.split_once('/') else { continue };
        let placed = board.placed();
        if !placed.contains(a) || !placed.contains(b) {
            continue;
        }
        // Prefer to move the part that is not pinned. If both are free,
        // the first is moved; if both are pinned there is nothing to do.
        let (mover, toward) = match (pin.contains(a), pin.contains(b)) {
            (false, _) => (a, b),
            (true, false) => (b, a),
            (true, true) => continue,
        };
        let Some((apart, allowed)) = distances(board, model, a, b) else { continue };
        out.push(Fix {
            mover: mover.to_string(),
            toward: toward.to_string(),
            apart_um: apart,
            allowed_um: allowed,
            check: c.check.clone(),
        });
    }
    out
}

/// Centre distance between two placed parts, and what the rule allows.
fn distances(board: &Board, model: &ConstraintModel, a: &str, b: &str) -> Option<(i64, i64)> {
    let d = board.design();
    let pl = d.placement.as_ref()?;
    let fa = pl.footprints.iter().find(|f| f.id == a)?;
    let fb = pl.footprints.iter().find(|f| f.id == b)?;
    let dx = (fa.at.x - fb.at.x) as f64;
    let dy = (fa.at.y - fb.at.y) as f64;
    let apart = (dx * dx + dy * dy).sqrt() as i64;
    // The tightest proximity rule naming this pair, in µm.
    let allowed = model
        .placement_rules
        .iter()
        .filter_map(|r| match r {
            eda_model::PlacementRule::Proximity { a: ra, b: rb, max_mm, .. }
                if (ra == a && rb == b) || (ra == b && rb == a) =>
            {
                Some((*max_mm * 1000.0) as i64)
            }
            _ => None,
        })
        .min()
        .unwrap_or(apart);
    Some((apart, allowed))
}

/// Walk `mover` toward `toward`, keeping only what the gates approve.
///
/// Greedy and deliberately simple: step along whichever axis is furthest
/// out, one nudge at a time, and stop the moment a step stops helping.
/// The point of this pass is to prove the verbs work and that a repair
/// loop can close a real violation -- a cleverer search belongs to the
/// decision layer that will propose moves over this same interface.
pub fn apply_fix(board: &mut Board, fix: &Fix, max_steps: u32) -> bool {
    let mut improved = false;
    for _ in 0..max_steps {
        let before = board.failures();
        let Some((dx, dy)) = offset(board, &fix.mover, &fix.toward) else { break };
        if dx == 0 && dy == 0 {
            break;
        }
        // Whichever axis is furthest out moves first.
        let dir = if dx.abs() >= dy.abs() {
            if dx > 0 { Dir::West } else { Dir::East }
        } else if dy > 0 {
            Dir::North
        } else {
            Dir::South
        };
        let mut trial = board.fork();
        if trial.apply(&Cmd::Nudge { part: fix.mover.clone(), dir, steps: 1 }).is_err() {
            break;
        }
        if trial.failures() > before {
            break; // this step made things worse; stop here
        }
        let helped = trial.failures() < before;
        *board = trial;
        improved |= helped;
    }
    improved
}

/// How far `mover` sits from `toward`, as a vector in µm.
fn offset(board: &Board, mover: &str, toward: &str) -> Option<(i64, i64)> {
    let d = board.design();
    let pl = d.placement.as_ref()?;
    let m = pl.footprints.iter().find(|f| f.id == mover)?;
    let t = pl.footprints.iter().find(|f| f.id == toward)?;
    Some((m.at.x - t.at.x, m.at.y - t.at.y))
}

/// Repair what can be repaired, returning how many failures were closed.
///
/// Runs until nothing improves: a fix can unblock another, and stopping
/// after one pass would leave those on the table.
pub fn repair(board: &mut Board, model: &ConstraintModel, max_steps: u32) -> usize {
    let start = board.failures();
    for _ in 0..8 {
        let before = board.failures();
        for fix in fixes_for(board, model) {
            if std::env::var("EDA_BUILD_TRACE").is_ok() {
                eprintln!(
                    "TRACE fix {} toward {} ({}): {:.2}mm apart, allows {:.2}mm, close {:.2}mm",
                    fix.mover,
                    fix.toward,
                    fix.check,
                    fix.apart_um as f64 / 1000.0,
                    fix.allowed_um as f64 / 1000.0,
                    fix.close_um() as f64 / 1000.0
                );
            }
            apply_fix(board, &fix, max_steps);
        }
        if board.failures() >= before {
            break;
        }
    }
    start.saturating_sub(board.failures())
}

/// Errors that could not be read as a fix, for a caller to report.
pub fn unactionable(board: &Board) -> Vec<CheckResult> {
    board
        .checks()
        .into_iter()
        .filter(|c| matches!(c.status, eda_model::CheckStatus::Fail) && c.check != "placement_proximity")
        .collect()
}
