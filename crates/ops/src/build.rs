//! Build a whole board out of commands, one part at a time.
//!
//! This is the replacement for `initial()`. Where that lays a hundred
//! parts into shelf rows in reading order and hands the pile to an
//! annealer, this places each part against a part already down, and
//! checks the board after every step.
//!
//! The order is the way a person works: connectors to the edges they
//! have to reach, the busiest IC into the middle, then outward along the
//! netlist -- a part is only ever placed beside something it is actually
//! connected to.
//!
//! # Who decides what
//!
//! Two decisions per step, and they are deliberately separated:
//!
//! * **which part next, beside which neighbour** -- judgement about the
//!   circuit. A [`Chooser`] answers it.
//! * **exactly where** -- arithmetic. The command layer answers it, the
//!   same way every run.
//!
//! The split is measured, not assumed; see the crate docs. It also means
//! the chooser can be swapped without touching geometry, which is the
//! only way to find out whether a smarter chooser is what helped.

use crate::{Board, Cmd, Dir, Region};
use eda_model::ir::Design;
use eda_model::{CheckResult, CheckStatus, ConstraintModel};
use std::collections::BTreeSet;

/// One candidate step: put `part` beside `anchor`, on `side`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Option_ {
    pub part: String,
    pub anchor: String,
    pub side: Dir,
}

/// Decides which part goes down next.
///
/// That is the whole job. *Where* the part then lands -- which placed
/// neighbour it hangs off and on which side -- is settled by
/// [`best_pose`] from the geometry, identically every run.
///
/// The division is deliberate and was measured. On questions about what
/// a circuit needs the evaluation model is reliable and knows when it is
/// unsure; on questions about distance it is barely better than chance
/// and is *more* confident when wrong. So it is asked what, never where.
pub trait Chooser {
    /// Pick a part from `frontier`, or `None` to stop.
    fn choose_part(&mut self, board: &Board, frontier: &[String]) -> Result<Option<String>, Vec<CheckResult>>;

    /// Name for logs and result.json.
    fn name(&self) -> &'static str;
}

/// The best legal pose for one part: fewest failing gates, shortest
/// wirelength to break the ties.
///
/// Ties are the common case early on, when almost nothing can fail yet,
/// so the wirelength term is what actually does the work.
pub fn best_pose(board: &Board, part: &str) -> std::option::Option<(Option_, usize, i64)> {
    let mut best: std::option::Option<(Option_, usize, i64)> = None;
    for o in options_for(board, part) {
        let mut trial = board.fork();
        if trial.apply(&Cmd::Place { part: o.part.clone(), anchor: o.anchor.clone(), side: o.side }).is_err() {
            continue; // no room that side; not a failure, just not this one
        }
        let fails = trial.failures();
        let wl = hpwl_of(trial.design(), board.model());
        if best.as_ref().map_or(true, |(_, bf, bw)| (fails, wl) < (*bf, *bw)) {
            best = Some((o, fails, wl));
        }
    }
    best
}

/// Picks by gate outcome, with wirelength as the tie-break.
///
/// The baseline. Any claim that a model places better has to beat this,
/// and this costs nothing and is identical every run.
pub struct Greedy;

impl Chooser for Greedy {
    fn name(&self) -> &'static str {
        "greedy"
    }

    /// Take whichever frontier part places best right now.
    fn choose_part(&mut self, board: &Board, frontier: &[String]) -> Result<Option<String>, Vec<CheckResult>> {
        let mut best: std::option::Option<(String, usize, i64)> = None;
        for part in frontier {
            let Some((_, fails, wl)) = best_pose(board, part) else { continue };
            if best.as_ref().map_or(true, |(_, bf, bw)| (fails, wl) < (*bf, *bw)) {
                best = Some((part.clone(), fails, wl));
            }
        }
        Ok(best.map(|(p, _, _)| p))
    }
}

/// Total half-perimeter wirelength over the nets whose parts are placed.
fn hpwl_of(design: &Design, model: &ConstraintModel) -> i64 {
    let Some(pl) = design.placement.as_ref() else { return 0 };
    let at: std::collections::BTreeMap<&str, (i64, i64)> =
        pl.footprints.iter().map(|f| (f.id.as_str(), (f.at.x, f.at.y))).collect();
    let mut total = 0;
    for n in &model.nets {
        let pts: Vec<(i64, i64)> = n
            .pins
            .iter()
            .filter_map(|p| at.get(p.split('.').next().unwrap_or(p)).copied())
            .collect();
        if pts.len() < 2 {
            continue;
        }
        let (xs, ys): (Vec<i64>, Vec<i64>) = pts.into_iter().unzip();
        total += (xs.iter().max().unwrap() - xs.iter().min().unwrap())
            + (ys.iter().max().unwrap() - ys.iter().min().unwrap());
    }
    total
}

/// How the build went, for reporting against the annealer.
#[derive(Debug, Clone)]
pub struct Report {
    pub chooser: &'static str,
    pub placed: usize,
    pub total: usize,
    /// Parts that had to be ripped because nothing legal was left.
    pub ripped: usize,
    /// Gate failures at the end.
    pub failures: usize,
    pub steps: usize,
}

/// Build a complete placement.
///
/// Returns the design and a report. A part that cannot be placed at all
/// is a hard failure -- there is no partial board worth shipping.
pub fn build(
    design: Design,
    model: &ConstraintModel,
    snap: eda_model::ir::Um,
    spacing: eda_model::ir::Um,
    chooser: &mut dyn Chooser,
) -> Result<(Design, Report), Vec<CheckResult>> {
    let mut b = Board::new(design, model, snap, spacing);
    let total = model.parts.len();
    let mut steps = 0usize;
    let mut ripped = 0usize;

    seed(&mut b, model)?;

    // Blocks first, parts second -- the way a board is actually laid
    // out. `partition` derives the functional units from the proximity
    // rules and the netlist: on L4 that is 18 blocks holding 93 of 100
    // parts, and they read like the circuit (a regulator with its
    // inductor and capacitors, an MCU with all its support, an LED
    // driver with its whole array).
    //
    // Each block is finished before the next begins. Without that the
    // loop only ever asks "what is adjacent to something already
    // placed", which says nothing about belonging to the same circuit,
    // and a regulator's inductor drifts away from its own capacitors.
    // Every step places or rips exactly one part, so a build needing
    // more than this is cycling rather than converging.
    let step_budget = total * 20 + 100;
    let (modules, free) = eda_model::floorplan::partition(model);
    let mut order: Vec<(String, BTreeSet<String>)> =
        modules.iter().map(|m| (m.name.clone(), m.refs.iter().cloned().collect())).collect();
    // Biggest block first: it is usually the hub everything else hangs
    // off, and a board built outward from the largest unit wastes less
    // room than one grown from a corner. Name breaks ties so a build is
    // the same every run.
    order.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));
    // Parts in no block go last, once their neighbours exist.
    if !free.is_empty() {
        order.push(("unassigned".to_string(), free.into_iter().collect()));
    }

    for (name, members) in &order {
        if let Err(e) = place_block(&mut b, model, members, chooser, &mut steps, step_budget) {
            // A block that will not go down is worth naming: it is a
            // statement about that part of the circuit, not about the
            // board as a whole.
            eprintln!("build: block {name} did not complete ({})", e.first().and_then(|c| c.hint.clone()).unwrap_or_default());
        }
    }

    // Each pass takes the whole current frontier. A part that no
    // candidate could place is left for the next pass, when more of its
    // neighbours are down and it has more anchors to hang off.
    // Anything the block pass could not reach -- a part whose block
    // stalled, or one the netlist leaves disconnected -- is finished
    // here by the original part-at-a-time loop.
    //
    // The budget also guards against a cycle the placed-count cannot
    // see: ripping a part and placing it again returns the count to
    // where it started, and the first build of L1 span for twelve
    // minutes on a board the annealer does in seven seconds.
    let mut stalled_passes = 0;
    while b.placed().len() < total {
        if steps > step_budget {
            let left = unplaced(&b, model);
            return Err(vec![CheckResult::fail(
                "build_no_progress",
                left.first().cloned().unwrap_or_default(),
                format!(
                    "gave up after {steps} commands with {} part(s) still unplaced ({}); the build is cycling rather than converging",
                    left.len(),
                    left.iter().take(6).cloned().collect::<Vec<_>>().join(", ")
                ),
            )]);
        }
        let frontier = b.frontier();
        if frontier.is_empty() {
            // Nothing is reachable from what is placed. The netlist has
            // a disconnected island, so seed it and carry on rather than
            // silently leaving parts off the board.
            match unplaced(&b, model).first() {
                Some(p) => {
                    b.apply(&Cmd::PlaceRegion { part: p.clone(), region: Region::Centre })?;
                    steps += 1;
                    continue;
                }
                None => break,
            }
        }

        // One part per step, and the chooser picks *which* one.
        //
        // This used to take the whole frontier and place every part in
        // it before looking at the board again -- nineteen parts on one
        // pass of L4 -- with the order decided alphabetically and the
        // chooser consulted only about the side. That is not building a
        // board a step at a time; it is batch placement with a stale
        // frontier, and it left the most consequential decision, what to
        // place next, to the sort order of the refdes.
        let before = b.placed().len();
        match chooser.choose_part(&b, &frontier)? {
            Some(part) => match best_pose(&b, &part) {
                Some((o, _, _)) => {
                    b.apply(&Cmd::Place { part: o.part, anchor: o.anchor, side: o.side })?;
                    steps += 1;
                }
                // The chooser named a part that will not go down
                // anywhere. Not fatal -- more neighbours next pass may
                // open a side -- but it counts as a stall.
                None => stalled_passes += 1,
            },
            None => stalled_passes += 1,
        }

        if b.placed().len() == before {
            stalled_passes += 1;
            // A pass that placed nothing will not do better next time
            // unless something moves. Rip the most recently placed part
            // to free room, and if that does not help either, give up
            // rather than spin.
            if stalled_passes > 2 {
                let left = unplaced(&b, model);
                return Err(vec![CheckResult::fail(
                    "build_stalled",
                    left.first().cloned().unwrap_or_default(),
                    format!(
                        "{} part(s) could not be placed beside anything already on the board, and ripping did not free room: {}",
                        left.len(),
                        left.iter().take(6).cloned().collect::<Vec<_>>().join(", ")
                    ),
                )]);
            }
            if let Some(victim) = b.placed().into_iter().last() {
                b.apply(&Cmd::Rip { part: victim })?;
                ripped += 1;
                steps += 1;
            }
        } else {
            stalled_passes = 0;
        }
    }

    let placed = b.placed().len();
    let mut design = b.into_design();
    // Trimming is an optimisation, not a requirement, so it is allowed
    // to be refused. Cutting the outline changes where refdes labels
    // sit (they flip to stay on the board), which changes keepouts,
    // which can push a part that was comfortably inside back over the
    // new edge -- nine `placement_within_outline` failures on L4 came
    // from trusting the trim. Keep it only if the gates agree.
    let before_trim = failures_of(&design, model).len();
    let mut trimmed = design.clone();
    shrink_to_parts(&mut trimmed, model);
    if failures_of(&trimmed, model).len() <= before_trim {
        design = trimmed;
    }

    let failures = failures_of(&design, model).len();
    let report = Report { chooser: chooser.name(), placed, total, ripped, failures, steps };
    Ok((design, report))
}

/// Take back the board nobody used.
///
/// The outline is fitted before placement from part areas, assuming the
/// spread an annealer produces. Building outward from a seed packs
/// tighter than that, so the parts end up in a corner of a board sized
/// for someone else's habits -- `placement_board_use` then reports 12%
/// coverage against a 25% target and an off-centre layout, which is a
/// complaint about the board, not the placement. The annealer trims its
/// own outline for exactly this reason; this is the same move.
///
/// An edge carrying a connector is left where it is: the connector has
/// to reach the outside, and moving its edge inward would either drag it
/// along or strand it.
fn shrink_to_parts(design: &mut Design, model: &ConstraintModel) {
    let Some(pl) = design.placement.as_ref() else { return };
    if pl.footprints.is_empty() || pl.outline.len() < 3 {
        return;
    }
    let bb = (
        pl.outline.iter().map(|p| p.x).min().unwrap_or(0),
        pl.outline.iter().map(|p| p.y).min().unwrap_or(0),
        pl.outline.iter().map(|p| p.x).max().unwrap_or(0),
        pl.outline.iter().map(|p| p.y).max().unwrap_or(0),
    );

    // Union of every keepout, so refdes labels stay on the board too.
    let mut used: std::option::Option<(i64, i64, i64, i64)> = None;
    let mut edge_held = [false; 4]; // west, north, east, south
    for f in &pl.footprints {
        let Some(part) = model.part(&f.id) else { continue };
        let Some(k) = eda_model::footprint::placed_keepout(model, &pl.outline, part, f) else { continue };
        used = Some(match used {
            None => k,
            Some(u) => (u.0.min(k.0), u.1.min(k.1), u.2.max(k.2), u.3.max(k.3)),
        });
        if eda_model::footprint::is_edge_connector(part) {
            const FLUSH: i64 = 1_500;
            if k.0 - bb.0 <= FLUSH { edge_held[0] = true }
            if k.1 - bb.1 <= FLUSH { edge_held[1] = true }
            if bb.2 - k.2 <= FLUSH { edge_held[2] = true }
            if bb.3 - k.3 <= FLUSH { edge_held[3] = true }
        }
    }
    let Some(u) = used else { return };

    // Leave a rim so nothing sits hard against the cut edge.
    const MARGIN: i64 = 1_000;
    let x0 = if edge_held[0] { bb.0 } else { (u.0 - MARGIN).max(bb.0) };
    let y0 = if edge_held[1] { bb.1 } else { (u.1 - MARGIN).max(bb.1) };
    let x1 = if edge_held[2] { bb.2 } else { (u.2 + MARGIN).min(bb.2) };
    let y1 = if edge_held[3] { bb.3 } else { (u.3 + MARGIN).min(bb.3) };
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    design.placement.as_mut().unwrap().outline = vec![
        eda_model::ir::Point { x: x0, y: y0 },
        eda_model::ir::Point { x: x1, y: y0 },
        eda_model::ir::Point { x: x1, y: y1 },
        eda_model::ir::Point { x: x0, y: y1 },
    ];
}


/// Place one functional block, one part at a time.
///
/// The block's parts are taken in the chooser's order, each anchored to
/// something already on the board -- preferring, by construction, the
/// block's own parts, since those are placed first and are what the
/// frontier offers.
///
/// If nothing in the block is reachable yet, one of its parts is seeded:
/// a connector to an edge it fits, otherwise the block's best-connected
/// part into open board. That first part is what ties the block to the
/// rest of the layout.
fn place_block(
    b: &mut Board,
    model: &ConstraintModel,
    members: &BTreeSet<String>,
    chooser: &mut dyn Chooser,
    steps: &mut usize,
    budget: usize,
) -> Result<(), Vec<CheckResult>> {
    loop {
        let placed = b.placed();
        let remaining: Vec<&String> = members.iter().filter(|m| !placed.contains(*m)).collect();
        if remaining.is_empty() {
            return Ok(());
        }
        if *steps > budget {
            return Err(vec![CheckResult::fail(
                "build_no_progress",
                remaining[0].clone(),
                format!("step budget spent with {} part(s) of this block unplaced", remaining.len()),
            )]);
        }

        let frontier = b.frontier_within(members);
        let chosen = if frontier.is_empty() {
            None
        } else {
            chooser.choose_part(b, &frontier)?
        };

        match chosen.and_then(|p| best_pose(b, &p).map(|(o, _, _)| o)) {
            Some(o) => {
                b.apply(&Cmd::Place { part: o.part, anchor: o.anchor, side: o.side })?;
                *steps += 1;
            }
            None => {
                // Nothing in this block can hang off what is placed, so
                // start the block somewhere of its own.
                let seed_part = seed_of(model, &remaining);
                if let Some(part) = seed_part {
                    seed_one(b, model, &part)?;
                    *steps += 1;
                } else {
                    return Ok(());
                }
            }
        }
    }
}

/// The part to start a block from: its best-connected member.
fn seed_of(model: &ConstraintModel, remaining: &[&String]) -> std::option::Option<String> {
    remaining
        .iter()
        .max_by_key(|r| {
            let n = model.nets.iter().filter(|net| net.pins.iter().any(|p| p.split('.').next() == Some(r.as_str()))).count();
            (n, std::cmp::Reverse((**r).clone()))
        })
        .map(|r| (*r).clone())
}

/// Put one part down with no anchor: an edge if it is a connector and
/// fits one, otherwise open board.
fn seed_one(b: &mut Board, model: &ConstraintModel, part: &str) -> Result<(), Vec<CheckResult>> {
    let is_conn = model.part(part).is_some_and(eda_model::footprint::is_edge_connector);
    if is_conn {
        for edge in [Dir::West, Dir::North, Dir::East, Dir::South] {
            for f in [0.5, 0.25, 0.75, 0.1, 0.9] {
                if b.apply(&Cmd::PlaceEdge { part: part.to_string(), edge, fraction: f }).is_ok() {
                    return Ok(());
                }
            }
        }
    }
    // Try the regions in a fixed order so the result is reproducible.
    let mut last = None;
    for region in Region::ALL {
        match b.apply(&Cmd::PlaceRegion { part: part.to_string(), region }) {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| vec![CheckResult::fail("build_no_room", part, "nowhere on the board to start this block")]))
}

fn unplaced(b: &Board, model: &ConstraintModel) -> Vec<String> {
    let placed = b.placed();
    model.parts.iter().map(|p| p.reference.clone()).filter(|r| !placed.contains(r)).collect()
}

/// Put down the parts that have nowhere else to go.
///
/// Edge connectors must reach the outside, so they take the edges first
/// and in a fixed order. Then the part with the most connections goes in
/// the middle, because everything else is going to hang off it and a
/// board built outward from a corner wastes the middle.
fn seed(b: &mut Board, model: &ConstraintModel) -> Result<(), Vec<CheckResult>> {
    let edges = [Dir::West, Dir::North, Dir::East, Dir::South];
    let mut connectors: Vec<&str> = model
        .parts
        .iter()
        .filter(|p| eda_model::footprint::is_edge_connector(p))
        .map(|p| p.reference.as_str())
        .collect();
    connectors.sort();

    for (i, c) in connectors.iter().enumerate() {
        let edge = edges[i % edges.len()];
        // Spread them along their edge rather than stacking at the
        // middle: three connectors on one edge all asking for 0.5 is a
        // guaranteed collision.
        let n_on_edge = connectors.len().div_ceil(edges.len()).max(1);
        let slot = i / edges.len();
        let fraction = if n_on_edge == 1 { 0.5 } else { slot as f64 / (n_on_edge - 1) as f64 };
        // Preferred edge first, then the others: a connector longer
        // than the edge it drew simply does not go there, and a board
        // is rarely square. Round-robin alone put a 40.3mm header on a
        // 40mm edge and left it hanging over the outline.
        let mut order = vec![edge];
        order.extend(edges.iter().copied().filter(|e| *e != edge));
        let mut last: std::option::Option<Vec<CheckResult>> = None;
        let mut landed = false;
        for e in order {
            match b.apply(&Cmd::PlaceEdge { part: (*c).to_string(), edge: e, fraction }) {
                Ok(()) => {
                    landed = true;
                    break;
                }
                Err(err) => last = Some(err),
            }
        }
        // Not fatal -- the frontier loop will place it against a
        // neighbour -- but never silent: a connector that quietly ends
        // up mid-board fails `placement_edge_connector` later with no
        // clue why.
        if !landed {
            eprintln!(
                "build: {c} fits no board edge ({}); it will be placed against a neighbour and will fail placement_edge_connector",
                last.and_then(|e| e.first().and_then(|x| x.hint.clone())).unwrap_or_default()
            );
        }
    }

    if b.placed().is_empty() || !connectors.is_empty() {
        // Seed the interior with the busiest part, whether or not
        // connectors landed: the edges alone do not anchor the middle.
        if let Some(hub) = busiest(model, &b.placed()) {
            b.apply(&Cmd::PlaceRegion { part: hub, region: Region::Centre })?;
        }
    }
    Ok(())
}

/// The unplaced part on the most nets.
fn busiest(model: &ConstraintModel, placed: &BTreeSet<String>) -> Option<String> {
    model
        .parts
        .iter()
        .filter(|p| !placed.contains(&p.reference))
        .max_by_key(|p| {
            let n = model.nets.iter().filter(|net| net.pins.iter().any(|pin| pin.split('.').next() == Some(p.reference.as_str()))).count();
            // Refdes breaks ties, so the seed is the same every run.
            (n, std::cmp::Reverse(p.reference.clone()))
        })
        .map(|p| p.reference.clone())
}

/// Every legal (anchor, side) for this part.
pub(crate) fn options_for(b: &Board, part: &str) -> Vec<Option_> {
    let placed = b.placed();
    let mut out = Vec::new();
    for anchor in b.neighbours_of(part) {
        if !placed.contains(&anchor) {
            continue;
        }
        for side in Dir::ALL {
            out.push(Option_ { part: part.to_string(), anchor: anchor.clone(), side });
        }
    }
    out
}

/// Gate failures on a finished build, for the caller to report.
pub fn failures_of(design: &Design, model: &ConstraintModel) -> Vec<CheckResult> {
    eda_gates::check_placement(design, model)
        .into_iter()
        .filter(|c| matches!(c.status, CheckStatus::Fail))
        .collect()
}
