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

/// Pairs of placed courtyards that overlap on one side of the board.
///
/// Repair moves parts that are already down, and moving is the one thing
/// that can land a part on a neighbour: `Board::place_*` keep clear of every
/// keepout, but `Nudge`, `Rotate` and `Swap` do not look. Whether courtyards
/// may overlap at all is KiCad's call (`courtyards_overlap`, answered by
/// kicad-cli in `eda_gates::kicad`); the in-process gates do not measure it.
/// This only stops a repair move from leaving more overlaps than it found.
pub(crate) fn courtyard_overlaps(board: &Board) -> usize {
    let model = board.model();
    let Some(pl) = board.design().placement.as_ref() else { return 0 };
    let rects: Vec<(eda_model::ir::Side, (i64, i64, i64, i64))> = pl
        .footprints
        .iter()
        .filter_map(|fp| Some((fp.side, eda_model::footprint::placed_courtyard(model, model.part(&fp.id)?, fp)?)))
        .collect();
    let mut n = 0;
    for i in 0..rects.len() {
        for j in i + 1..rects.len() {
            let ((sa, a), (sb, b)) = (rects[i], rects[j]);
            if sa == sb && a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3 {
                n += 1;
            }
        }
    }
    n
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
        if trial.failures() > before || courtyard_overlaps(&trial) > courtyard_overlaps(board) {
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
        let uncrossed = uncross(board, model);
        if board.failures() >= before && !uncrossed {
            break;
        }
    }
    start.saturating_sub(board.failures())
}

/// Failing gates other than the crossing gate, and the crossing stub
/// pairs ("N1×N2") that gate names, from one run of the gates.
fn crossing_pairs(board: &Board) -> (usize, Vec<String>) {
    let checks = board.checks();
    let failing = |c: &&CheckResult| matches!(c.status, eda_model::CheckStatus::Fail);
    let (crossing, other): (Vec<&CheckResult>, Vec<&CheckResult>) =
        checks.iter().filter(failing).partition(|c| c.check == "placement_stub_crossings");
    let pairs = crossing
        .first()
        .and_then(|c| c.location.as_deref())
        .map_or_else(Vec::new, |loc| loc.split(',').map(String::from).collect());
    (other.len(), pairs)
}

/// Turn or swap the free two-pin parts on crossing stubs until nothing
/// uncrosses. Returns whether anything did.
///
/// A crossing is the one failure a constructive placer makes by
/// construction: parts go down at rotation 0, so which way round a
/// resistor's pins face is decided by its footprint, not its nets --
/// on two_pin_nets, R1's pin 1 faced away from R2 and N1 crossed N2.
/// Turning R1 half round is the whole fix.
///
/// The gate reports one failure however many pairs cross, so the
/// failure count cannot see one pair of five being fixed. Pairs are
/// counted instead: a move is kept when it uncrosses something and
/// fails no other gate that was passing -- re-hanging an LED where it
/// slides a centimetre along its resistor trades the crossing for a
/// compactness failure, which is no repair -- and the best such move is
/// taken each round.
fn uncross(board: &mut Board, model: &ConstraintModel) -> bool {
    let pin = pinned(model);
    let mut improved = false;
    for _ in 0..32 {
        let (fails, pairs) = crossing_pairs(board);
        if pairs.is_empty() {
            break;
        }
        // The parts on either stub of a crossing that are free to move:
        // each can turn, trade places with another, or go down again on
        // another side of the part at the far end of its own stub (an
        // LED re-hung on the other side of its resistor).
        let mut turn: BTreeSet<String> = BTreeSet::new();
        let mut trade: BTreeSet<(String, String)> = BTreeSet::new();
        let mut rehang: BTreeSet<(String, String)> = BTreeSet::new();
        for pair in &pairs {
            let stubs: Vec<Vec<String>> = pair
                .split('×')
                .filter_map(|net| model.nets.iter().find(|n| n.name == net))
                .map(|n| n.pins.iter().filter_map(|p| p.split_once('.').map(|(r, _)| r.to_string())).collect())
                .collect();
            let free = |r: &String| !pin.contains(r) && model.part(r).is_some_and(eda_model::is_free_two_pin);
            let parts: Vec<String> = stubs.iter().flatten().filter(|r| free(r)).cloned().collect::<BTreeSet<_>>().into_iter().collect();
            for (i, p) in parts.iter().enumerate() {
                turn.insert(p.clone());
                trade.extend(parts[i + 1..].iter().map(|q| (p.clone(), q.clone())));
            }
            for stub in &stubs {
                for p in stub.iter().filter(|r| free(r)) {
                    rehang.extend(stub.iter().filter(|q| *q != p).map(|q| (p.clone(), q.clone())));
                }
            }
        }
        let moves = turn
            .iter()
            .flat_map(|p| (1..=3).map(|quarter_turns| vec![Cmd::Rotate { part: p.clone(), quarter_turns }]))
            .chain(trade.into_iter().map(|(a, b)| vec![Cmd::Swap { a, b }]))
            .chain(rehang.into_iter().flat_map(|(p, q)| {
                // Every side, every way round: a part lands at rotation 0,
                // and turned it may fit where unturned it slid away.
                Dir::ALL.into_iter().flat_map(move |side| {
                    let (p, q) = (p.clone(), q.clone());
                    (0..4u8).map(move |quarter_turns| {
                        let mut cmds = vec![Cmd::Rip { part: p.clone() }, Cmd::Place { part: p.clone(), anchor: q.clone(), side }];
                        if quarter_turns > 0 {
                            cmds.push(Cmd::Rotate { part: p.clone(), quarter_turns });
                        }
                        cmds
                    })
                })
            }));
        let mut best: Option<(Board, (usize, usize), Vec<Cmd>)> = None;
        let overlaps_now = courtyard_overlaps(board);
        for cmds in moves {
            let mut trial = board.fork();
            if cmds.iter().any(|c| trial.apply(c).is_err()) {
                continue;
            }
            if courtyard_overlaps(&trial) > overlaps_now {
                continue;
            }
            let (f, p) = crossing_pairs(&trial);
            let after = (f, p.len());
            if f <= fails && after.1 < pairs.len() && best.as_ref().is_none_or(|(_, b, _)| after < *b) {
                best = Some((trial, after, cmds));
            }
        }
        let Some((trial, after, cmds)) = best else { break };
        if std::env::var("EDA_BUILD_TRACE").is_ok() {
            eprintln!("TRACE uncross {cmds:?}: {} -> {} crossing pair(s)", pairs.len(), after.1);
        }
        *board = trial;
        improved = true;
    }
    improved
}

/// Errors that could not be read as a fix, for a caller to report.
pub fn unactionable(board: &Board) -> Vec<CheckResult> {
    board
        .checks()
        .into_iter()
        .filter(|c| matches!(c.status, eda_model::CheckStatus::Fail) && c.check != "placement_proximity")
        .collect()
}
