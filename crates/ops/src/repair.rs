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
    pub apart_nm: i64,
    /// How far apart they are allowed to be, µm.
    pub allowed_nm: i64,
    /// The gate that objected.
    pub check: String,
}

impl Fix {
    /// How far the mover has to travel, µm.
    pub fn close_nm(&self) -> i64 {
        (self.apart_nm - self.allowed_nm).max(0)
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
            apart_nm: apart,
            allowed_nm: allowed,
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
                Some(eda_model::units::from_mm(*max_mm))
            }
            _ => None,
        })
        .min()
        .unwrap_or(apart);
    Some((apart, allowed))
}

/// What a repair move must not make worse: courtyards overlapping on one
/// side of the board, and parts with pad copper inside the board-edge band.
///
/// Repair moves parts that are already down, and moving is the one thing
/// that can land a part on a neighbour or against the edge: `Board::place_*`
/// keep clear of both, but `Nudge`, `Rotate` and `Swap` do not look. Whether
/// either is a violation is KiCad's call (`courtyards_overlap`,
/// `copper_edge_clearance`, answered by kicad-cli in `eda_gates::kicad`); the
/// in-process gates do not measure them, so the repair loop guards itself.
fn faults(board: &Board) -> usize {
    courtyard_overlaps(board) + board.parts_in_edge_band()
}

/// Pairs of placed courtyards that overlap on one side of the board.
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
        if trial.failures() > before || faults(&trial) > faults(board) {
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
                    eda_model::units::to_mm(fix.apart_nm),
                    eda_model::units::to_mm(fix.allowed_nm),
                    eda_model::units::to_mm(fix.close_nm())
                );
            }
            let was = board.failures();
            apply_fix(board, &fix, max_steps);
            if board.failures() >= was && !replace(board, model, &fix) && !pinned(model).contains(&fix.toward) {
                // the other part of the pair, put down beside the first
                let back = Fix { mover: fix.toward.clone(), toward: fix.mover.clone(), ..fix.clone() };
                replace(board, model, &back);
            }
        }
        let uncrossed = uncross(board, model);
        if board.failures() >= before && !uncrossed {
            break;
        }
    }
    start.saturating_sub(board.failures())
}

/// Put the mover of a proximity failure down again where its rules want it, when walking it there does not work: ripped and placed beside the part
/// it has to be near (or beside any part one of its rules names), on every side and every way round; and if no such pose has room, with a part that
/// sits beside the target ripped first, so the mover takes its place, and that part put back wherever it now places best. The best move that leaves
/// fewer failures and no new overlap or edge fault is kept.
///
/// A walk is blocked by whatever lies between: on `ldo_proximity_heavy` with the installed SOT-223 (a courtyard 0.3 mm taller than the built-in's
/// pads give it), C4 took the last room beside U1 and C3, C1 went down 9 mm from C3 under a 3 mm rule, and a nudge towards C3 ran into U1 at once.
fn replace(board: &mut Board, model: &ConstraintModel, fix: &Fix) -> bool {
    let pin = pinned(model);
    let mover = fix.mover.as_str();
    // the parts the mover's rules name, the target first
    let mut anchors: Vec<String> = vec![fix.toward.clone()];
    for r in &model.placement_rules {
        if let eda_model::PlacementRule::Proximity { a, b, .. } = r {
            let other = if a == mover { b } else if b == mover { a } else { continue };
            if !anchors.contains(other) {
                anchors.push(other.clone());
            }
        }
    }
    let placed = board.placed();
    anchors.retain(|a| placed.contains(a));
    let beside = |cmds: &mut Vec<Vec<Cmd>>, prefix: &[Cmd]| {
        for anchor in &anchors {
            for side in Dir::ALL {
                for quarter_turns in 0..4u8 {
                    let mut c: Vec<Cmd> = prefix.to_vec();
                    c.push(Cmd::Rip { part: mover.to_string() });
                    c.push(Cmd::Place { part: mover.to_string(), anchor: anchor.clone(), side });
                    if quarter_turns > 0 {
                        c.push(Cmd::Rotate { part: mover.to_string(), quarter_turns });
                    }
                    cmds.push(c);
                }
            }
        }
    };
    let mut moves = Vec::new();
    beside(&mut moves, &[]);
    let mut best = best_move(board, moves, None);
    if best.is_none() {
        // the free parts that sit beside the target: within the rule's distance of it, courtyard to courtyard
        let Some(pl) = board.design().placement.as_ref() else { return false };
        let rect = |id: &str| pl.footprints.iter().find(|f| f.id == id).and_then(|f| eda_model::footprint::placed_courtyard(model, model.part(id)?, f));
        let Some(t) = rect(&fix.toward) else { return false };
        let reach = fix.allowed_nm;
        let near: Vec<String> = pl
            .footprints
            .iter()
            .map(|f| f.id.clone())
            .filter(|k| k != mover && *k != fix.toward && !pin.contains(k))
            .filter(|k| rect(k).is_some_and(|r| r.0 - reach < t.2 && t.0 < r.2 + reach && r.1 - reach < t.3 && t.1 < r.3 + reach))
            .collect();
        for k in near {
            let mut moves = Vec::new();
            beside(&mut moves, &[Cmd::Rip { part: k.clone() }]);
            if let Some((trial, f)) = best_move(board, moves, Some(&k)) {
                if best.as_ref().is_none_or(|(_, bf)| f < *bf) {
                    best = Some((trial, f));
                }
            }
        }
    }
    let Some((trial, f)) = best else { return false };
    if std::env::var("EDA_BUILD_TRACE").is_ok() {
        eprintln!("TRACE replace {mover} beside {}: {} -> {f} failure(s)", fix.toward, board.failures());
    }
    *board = trial;
    true
}

/// The best of `moves` on a fork of `board` (with `reseat`, a part each move ripped, put back at its best pose): fewer failures than now and no new
/// overlap or edge fault, the fewest failures first.
fn best_move<'a>(board: &Board<'a>, moves: Vec<Vec<Cmd>>, reseat: Option<&str>) -> Option<(Board<'a>, usize)> {
    let (fails_now, faults_now) = (board.failures(), faults(board));
    let mut best: Option<(Board, usize)> = None;
    for cmds in moves {
        let mut trial = board.fork();
        if cmds.iter().any(|c| trial.apply(c).is_err()) {
            continue;
        }
        // the ripped neighbour goes back where it now places best
        if let Some(k) = reseat {
            let Some((o, _)) = crate::build::best_pose(&trial, k) else { continue };
            if trial.apply(&Cmd::Place { part: o.part, anchor: o.anchor, side: o.side }).is_err() {
                continue;
            }
        }
        let f = trial.failures();
        if f < fails_now && faults(&trial) <= faults_now && best.as_ref().is_none_or(|(_, bf)| f < *bf) {
            best = Some((trial, f));
        }
    }
    best
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

/// A chain of free two-pin parts hung off a fixed part: `head` is the fixed part's pin, `parts` the free parts from it outwards (a resistor and the LED
/// it feeds), each with the number of its pin towards the head.
struct Chain {
    head: String,
    head_pin: String,
    parts: Vec<(String, String)>,
}

/// The chains of the board's two-pin stubs: a free two-pin part on a two-pin net to a part that is not one (U1.3 -- R1.1), and the free two-pin parts
/// that follow it on two-pin nets (R1.2 -- D1.1), up to three.
fn chains(model: &ConstraintModel, pin: &BTreeSet<String>) -> Vec<Chain> {
    let free = |r: &str| !pin.contains(r) && model.part(r).is_some_and(eda_model::is_free_two_pin);
    let two_pin: Vec<[(&str, &str); 2]> = model
        .nets
        .iter()
        .filter(|n| n.pins.len() == 2)
        .filter_map(|n| Some([n.pins[0].split_once('.')?, n.pins[1].split_once('.')?]))
        .filter(|[a, b]| a.0 != b.0)
        .collect();
    let mut out = Vec::new();
    for [a, b] in &two_pin {
        for (head, first) in [(a, b), (b, a)] {
            if free(head.0) || !free(first.0) {
                continue;
            }
            let mut parts = vec![(first.0.to_string(), first.1.to_string())];
            while parts.len() < 3 {
                let (last, in_pin) = parts.last().cloned().unwrap();
                // the free part on the last one's other pin
                let next = two_pin.iter().find_map(|[x, y]| {
                    let theirs = if x.0 == last && x.1 != in_pin { y } else if y.0 == last && y.1 != in_pin { x } else { return None };
                    (free(theirs.0) && !parts.iter().any(|(p, _)| p == theirs.0)).then(|| (theirs.0.to_string(), theirs.1.to_string()))
                });
                match next {
                    Some(n) => parts.push(n),
                    None => break,
                }
            }
            out.push(Chain { head: head.0.to_string(), head_pin: head.1.to_string(), parts });
        }
    }
    out
}

/// Give the chains hung off one part the places their pins' order asks for: the chains of one shape (the same footprints, link by link) on one head
/// trade places so that, going round the head, their places come in the order of their pins on it -- the order in which no two stubs cross -- each
/// part turned whichever way puts its pin nearer the one it joins. Kept when it leaves fewer crossing pairs, no other gate failing that was passing,
/// and no new overlap or edge fault.
///
/// A constructive placer hangs each chain off the head where there is room when its turn comes, and the order the chains come in is the order of the
/// parts' names, not of their pins: on `mcu_board_30plus`, R6 (U1's pin 8, low on its left side) went down above U1 and R1, R5 and R2 below it out of
/// order, six crossing pairs that no single turn or swap undoes (each one moves a crossing rather than removing it).
fn reorder_chains(board: &mut Board, model: &ConstraintModel) -> bool {
    let pin = pinned(model);
    let all = chains(model, &pin);
    let Some(pl) = board.design().placement.clone() else { return false };
    let fp_of = |id: &str| pl.footprints.iter().find(|f| f.id == id).cloned();
    let pin_at = |fp: &eda_model::ir::FootprintInstance, number: &str| -> Option<(f64, f64)> {
        let pads = eda_model::footprint::placed_pads(model, model.part(&fp.id)?, fp)?;
        let on: Vec<_> = pads.iter().filter(|p| p.number == number).collect();
        (!on.is_empty()).then(|| (on.iter().map(|p| p.center.x as f64).sum::<f64>() / on.len() as f64, on.iter().map(|p| p.center.y as f64).sum::<f64>() / on.len() as f64))
    };
    let shape = |c: &Chain| -> Vec<String> { c.parts.iter().map(|(p, _)| model.part(p).map(crate::footprint_name_of).unwrap_or_default()).collect() };
    // the groups: one head, one shape, every part placed
    let mut groups: std::collections::BTreeMap<(String, Vec<String>), Vec<&Chain>> = std::collections::BTreeMap::new();
    for c in &all {
        if fp_of(&c.head).is_some() && c.parts.iter().all(|(p, _)| fp_of(p).is_some()) {
            groups.entry((c.head.clone(), shape(c))).or_default().push(c);
        }
    }
    let (fails, pairs) = crossing_pairs(board);
    if pairs.is_empty() {
        return false;
    }
    let faults_now = faults(board);
    let mut best: Option<(Board, usize)> = None;
    for ((head, _), group) in &groups {
        if group.len() < 2 {
            continue;
        }
        let Some(hfp) = fp_of(head) else { continue };
        let centre = (hfp.at.x as f64, hfp.at.y as f64);
        let angle = |p: (f64, f64)| (p.1 - centre.1).atan2(p.0 - centre.0);
        // the chains in the order of their pins round the head, and their places in the order round it
        let mut by_pin: Vec<(f64, &Chain)> = group.iter().filter_map(|c| Some((angle(pin_at(&hfp, &c.head_pin)?), *c))).collect();
        if by_pin.len() != group.len() {
            continue;
        }
        by_pin.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.parts[0].0.cmp(&b.1.parts[0].0)));
        let mut slots: Vec<(f64, Vec<eda_model::ir::FootprintInstance>)> =
            group.iter().map(|c| { let fps: Vec<_> = c.parts.iter().filter_map(|(p, _)| fp_of(p)).collect(); (angle((fps[0].at.x as f64, fps[0].at.y as f64)), fps) }).collect();
        slots.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1[0].id.cmp(&b.1[0].id)));
        let n = by_pin.len();
        for shift in 0..n {
            let mut trial = board.fork();
            let mut ok = true;
            for (i, (_, chain)) in by_pin.iter().enumerate() {
                let slot = &slots[(i + shift) % n].1;
                // each part to its slot, turned whichever way of the two puts the pin towards the last one nearer it
                let mut towards = pin_at(&hfp, &chain.head_pin).unwrap_or(centre);
                for ((part, in_pin), place) in chain.parts.iter().zip(slot) {
                    let mut pick: Option<(f64, u32)> = None;
                    for rot in [place.rot, (place.rot + 180_000) % 360_000] {
                        let fp = eda_model::ir::FootprintInstance { id: part.clone(), at: place.at, rot, side: place.side, label: place.label };
                        let Some(p) = pin_at(&fp, in_pin) else { continue };
                        let d = (p.0 - towards.0).hypot(p.1 - towards.1);
                        if pick.is_none_or(|(bd, _)| d < bd) {
                            pick = Some((d, rot));
                        }
                    }
                    let Some((_, rot)) = pick else { ok = false; break };
                    let Some(cur) = fp_of(part) else { ok = false; break };
                    let turns = (((rot + 360_000 - cur.rot) % 360_000) / 90_000) as u8;
                    if trial.apply(&Cmd::MoveTo { part: part.clone(), x: place.at.x, y: place.at.y }).is_err()
                        || (turns > 0 && trial.apply(&Cmd::Rotate { part: part.clone(), quarter_turns: turns }).is_err())
                    {
                        ok = false;
                        break;
                    }
                    // the far pin of this part is what the next one joins
                    let fp = eda_model::ir::FootprintInstance { id: part.clone(), at: place.at, rot, side: place.side, label: place.label };
                    let other = model.part(part).and_then(|p| p.pins.iter().map(|q| q.number.clone()).find(|q| q != in_pin));
                    towards = other.and_then(|o| pin_at(&fp, &o)).unwrap_or(towards);
                }
                if !ok {
                    break;
                }
            }
            if !ok || faults(&trial) > faults_now {
                continue;
            }
            let (f, p) = crossing_pairs(&trial);
            if f <= fails && p.len() < pairs.len() && best.as_ref().is_none_or(|(_, bp)| p.len() < *bp) {
                best = Some((trial, p.len()));
            }
        }
    }
    let Some((trial, after)) = best else { return false };
    if std::env::var("EDA_BUILD_TRACE").is_ok() {
        eprintln!("TRACE reorder chains: {} -> {after} crossing pair(s)", pairs.len());
    }
    *board = trial;
    true
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
    let mut improved = reorder_chains(board, model);
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
        let faults_now = faults(board);
        for cmds in moves {
            let mut trial = board.fork();
            if cmds.iter().any(|c| trial.apply(c).is_err()) {
                continue;
            }
            if faults(&trial) > faults_now {
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

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{Design, PlacementSection, Point, Provenance};
    use eda_model::{Net, Part, Pin, PinKind};

    fn part(r: &str, pkg: &str, pins: usize) -> Part {
        Part {
            reference: r.into(),
            mpn: None,
            lcsc: None,
            value: None,
            package: Some(pkg.into()),
            footprint: None,
            pins: (1..=pins).map(|n| Pin { number: n.to_string(), name: None, kind: PinKind::Passive }).collect(),
            body_nm: None,
            symbol: None,
            datasheet: None,
            edge: None,
        }
    }

    /// U1 (SOIC-8) with three chains off its left pins 1, 2, 3: `PAi` U1.i -- Ri.1, `LEDi` Ri.2 -- Di.1, the LEDs' pins 2 on GND with U1.4.
    fn model() -> ConstraintModel {
        let mut parts = vec![part("U1", "SOIC-8", 8)];
        let mut nets = vec![Net { name: "GND".into(), pins: vec!["U1.4".into()] }];
        for i in 1..=3 {
            parts.push(part(&format!("R{i}"), "0603", 2));
            parts.push(part(&format!("D{i}"), "0603", 2));
            nets.push(Net { name: format!("PA{i}"), pins: vec![format!("U1.{i}"), format!("R{i}.1")] });
            nets.push(Net { name: format!("LED{i}"), pins: vec![format!("R{i}.2"), format!("D{i}.1")] });
            nets[0].pins.push(format!("D{i}.2"));
        }
        ConstraintModel { parts, nets, ..Default::default() }
    }

    fn design() -> Design {
        let o = 100_000_000;
        Design {
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
            schema: eda_model::ir::SCHEMA_VERSION,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            routing: None,
            placement: Some(PlacementSection { outline: vec![Point { x: 0, y: 0 }, Point { x: o, y: 0 }, Point { x: o, y: o }, Point { x: 0, y: o }], footprints: vec![], modules: vec![] }),
            drawings: None,
        }
    }

    fn y_of(b: &Board, r: &str) -> i64 {
        b.design().placement.as_ref().unwrap().footprints.iter().find(|f| f.id == r).unwrap().at.y
    }

    /// The chains down the left side in the reverse of their pins' order (R3 at the top, R1 at the bottom): every pair of PA stubs crosses, and one
    /// reorder puts R1 at the top and R3 at the bottom, with nothing crossing and no part on another.
    #[test]
    fn chains_hung_out_of_their_pins_order_are_put_in_it() {
        let m = model();
        let mut b = Board::new(design(), &m, 100_000, 300_000);
        b.apply(&Cmd::PlaceAt { part: "U1".into(), x: 50_000_000, y: 50_000_000 }).unwrap();
        for (i, y) in [(3, 47_000_000), (2, 49_500_000), (1, 52_000_000)] {
            b.apply(&Cmd::PlaceAt { part: format!("R{i}"), x: 44_500_000, y }).unwrap();
            b.apply(&Cmd::PlaceAt { part: format!("D{i}"), x: 41_500_000, y }).unwrap();
        }
        let (_, before) = crossing_pairs(&b);
        assert_eq!(before.len(), 3, "{before:?}");
        assert!(reorder_chains(&mut b, &m));
        let (fails, after) = crossing_pairs(&b);
        assert!(after.is_empty(), "{after:?}");
        assert_eq!(fails, 0);
        assert!(y_of(&b, "R1") < y_of(&b, "R2") && y_of(&b, "R2") < y_of(&b, "R3"), "the resistors in the order of U1's pins");
        assert!(y_of(&b, "D1") < y_of(&b, "D2") && y_of(&b, "D2") < y_of(&b, "D3"), "each LED goes with its resistor");
        assert_eq!(faults(&b), 0);
    }

    /// Chains already in their pins' order are left as they are.
    #[test]
    fn chains_in_their_pins_order_stay() {
        let m = model();
        let mut b = Board::new(design(), &m, 100_000, 300_000);
        b.apply(&Cmd::PlaceAt { part: "U1".into(), x: 50_000_000, y: 50_000_000 }).unwrap();
        for (i, y) in [(1, 47_000_000), (2, 49_500_000), (3, 52_000_000)] {
            b.apply(&Cmd::PlaceAt { part: format!("R{i}"), x: 44_500_000, y }).unwrap();
            b.apply(&Cmd::PlaceAt { part: format!("D{i}"), x: 41_500_000, y }).unwrap();
        }
        let was = b.design().placement.clone();
        assert!(!reorder_chains(&mut b, &m));
        assert_eq!(serde_json::to_string(&b.design().placement).unwrap(), serde_json::to_string(&was).unwrap());
    }
}
