//! Modules and their board regions: the step a human team does before
//! anyone opens the layout editor.
//!
//! A hundred-part board is not placed by one person moving a hundred
//! parts. It is cut into functional blocks -- the buck, the MCU and its
//! decoupling, each motor driver -- every block is given a patch of board,
//! and the blocks are only then filled in. That is what this module does:
//! [`partition`] recovers the blocks the intent already implies, and
//! [`arrange`] gives each one a rectangle.
//!
//! The partition is not guessed. The intent's proximity rules are an
//! author saying "these two belong together", so the connected components
//! of those rules *are* the modules. A part touched by no proximity rule
//! joins no module and is left free: an honest "the intent never said
//! where this belongs" beats inventing a block for it.

use crate::footprint::{is_edge_connector, placed_courtyard};
use crate::ir::{FootprintInstance, Point, Side, Um};
use crate::{ConstraintModel, PlacementRule};
use std::collections::{BTreeMap, BTreeSet};

/// Fraction of a module's rectangle its parts' *keepouts* may fill (see
/// `part_area`). This is a packing-efficiency allowance, not a density
/// target: the keepouts already carry the routing channel and the label
/// band, and no shelf pack of mixed rectangles reaches 100%.
pub const MODULE_FILL: f64 = 0.65;

/// A functional block: the parts one person would lay out in one sitting.
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    /// Named for the lowest refdes in it, so the name is stable across
    /// runs and means something to a reader ("mod_U4").
    pub name: String,
    pub refs: Vec<String>,
    /// Summed courtyard area of the members, µm².
    pub area: f64,
    /// The module holds an edge connector, so its block has to touch a
    /// board edge. Without this the plan parks a cable header's block in
    /// the middle of the board and the placer -- which pulls connectors to
    /// the edge harder than it holds them in their block -- drags the part
    /// straight out of its own module.
    pub edge: bool,
    /// Largest single-part courtyard extent in the module, µm: (widest,
    /// tallest) at rotation 0. A block sized only from summed area can be
    /// too small to hold its own biggest part -- an edge connector 41 mm
    /// long has the area of a 6 mm square -- and the placer then drags
    /// that part straight out of the region it was given.
    pub largest: (Um, Um),
}

/// A module with the patch of board it owns.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedModule {
    pub name: String,
    pub refs: Vec<String>,
    /// (x0, y0, x1, y1) µm.
    pub rect: (Um, Um, Um, Um),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Floorplan {
    pub modules: Vec<PlacedModule>,
    /// Parts in no module: the intent never tied them to anything.
    pub free: Vec<String>,
}

impl Floorplan {
    /// The rectangle part `id` must stay inside, if it has one.
    pub fn region_of(&self, id: &str) -> Option<(Um, Um, Um, Um)> {
        self.modules.iter().find(|m| m.refs.iter().any(|r| r == id)).map(|m| m.rect)
    }
}

/// Courtyard area of one part at rotation 0, µm². `None` when the part has
/// no resolvable footprint -- the caller decides whether that is fatal;
/// here it simply cannot be sized, and a module is never sized from a
/// guess.
fn part_area(model: &ConstraintModel, id: &str) -> Option<f64> {
    let (w, h) = part_extent(model, id)?;
    // What the placer packs is not the courtyard but the *keepout*: the
    // courtyard, the spacing it keeps on every side, and the refdes label
    // band above it. For an 0402 resistor those margins are an order of
    // magnitude more area than the part itself, so a block sized from bare
    // courtyards is nowhere near big enough to hold what goes in it.
    let s = model.solver.place_spacing_um;
    Some(((w + 2 * s) as f64) * ((h + 2 * s + MODULE_LABEL_BAND_UM) as f64))
}

/// Courtyard extent of one part at rotation 0, µm.
fn part_extent(model: &ConstraintModel, id: &str) -> Option<(Um, Um)> {
    let part = model.part(id)?;
    let fp = FootprintInstance { id: id.to_string(), at: Point { x: 0, y: 0 }, rot: 0, side: Side::Top, label: Default::default() };
    let c = placed_courtyard(model, part, &fp)?;
    Some((c.2 - c.0, c.3 - c.1))
}

/// Cut the netlist into modules along the intent's own proximity rules.
///
/// Union-find over every `Proximity` pair. `Separation` deliberately does
/// not join: it says the opposite. Parts in no rule come back in
/// [`Floorplan::free`] rather than being swept into a catch-all module.
pub fn partition(model: &ConstraintModel) -> (Vec<Module>, Vec<String>) {
    let ids: Vec<String> = model.parts.iter().map(|p| p.reference.clone()).collect();
    let index: BTreeMap<&str, usize> = ids.iter().enumerate().map(|(i, s)| (s.as_str(), i)).collect();
    let mut up: Vec<usize> = (0..ids.len()).collect();
    fn find(up: &mut Vec<usize>, mut i: usize) -> usize {
        while up[i] != i {
            up[i] = up[up[i]];
            i = up[i];
        }
        i
    }
    let mut touched = vec![false; ids.len()];
    for r in &model.placement_rules {
        let (a, b) = match r {
            PlacementRule::Proximity { a, b, .. } => (a, b),
            _ => continue,
        };
        let (Some(&ia), Some(&ib)) = (index.get(a.as_str()), index.get(b.as_str())) else { continue };
        touched[ia] = true;
        touched[ib] = true;
        let (ra, rb) = (find(&mut up, ia), find(&mut up, ib));
        if ra != rb {
            up[ra] = rb;
        }
    }
    let mut groups: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    let mut free = Vec::new();
    for i in 0..ids.len() {
        if !touched[i] {
            free.push(ids[i].clone());
            continue;
        }
        let root = find(&mut up, i);
        groups.entry(root).or_default().push(ids[i].clone());
    }
    // A proximity rule is the strongest signal, but not the only one. A
    // free part whose narrow nets all land in one module (an LED and its
    // series resistor hanging off one driver, a pull-up on one bus) has
    // been told where it belongs by the netlist even though no rule says
    // so. Absorb those, to a fixpoint, so a growing module can pull in the
    // part behind the part it just took. A free part pulled by two
    // different modules stays free: the netlist is genuinely ambiguous
    // there and guessing would be worse than admitting it.
    loop {
        let owner: BTreeMap<&str, usize> = groups.iter().flat_map(|(&root, refs)| refs.iter().map(move |r| (r.as_str(), root))).collect();
        let mut moved = Vec::new();
        for id in &free {
            let mut seen: BTreeSet<usize> = BTreeSet::new();
            for net in &model.nets {
                if net.pins.len() > 8 || !net.pins.iter().any(|p| p.split('.').next() == Some(id.as_str())) {
                    continue;
                }
                for pin in &net.pins {
                    if let Some(&m) = owner.get(pin.split('.').next().unwrap_or("")) {
                        seen.insert(m);
                    }
                }
            }
            if seen.len() == 1 {
                moved.push((id.clone(), *seen.iter().next().unwrap()));
            }
        }
        if moved.is_empty() {
            break;
        }
        for (id, root) in moved {
            free.retain(|f| f != &id);
            groups.entry(root).or_default().push(id);
        }
    }
    for refs in groups.values_mut() {
        refs.sort();
    }

    let mut modules: Vec<Module> = groups
        .into_values()
        .map(|refs| {
            let area = refs.iter().filter_map(|r| part_area(model, r)).sum();
            // Name the module after the part with the most pads: a block
            // is understood by its IC, not by whichever passive happens to
            // sort first. Ties break on refdes so the name is stable.
            let lead = refs
                .iter()
                .max_by_key(|r| (model.part(r).map(|p| p.pins.len()).unwrap_or(0), std::cmp::Reverse((*r).clone())))
                .cloned()
                .unwrap_or_default();
            let edge = refs.iter().filter_map(|r| model.part(r)).any(is_edge_connector);
            let largest = refs.iter().filter_map(|r| part_extent(model, r)).fold((0, 0), |a, b| (a.0.max(b.0), a.1.max(b.1)));
            Module { name: format!("mod_{lead}"), refs, area, edge, largest }
        })
        .collect();
    modules.sort_by(|a, b| b.area.partial_cmp(&a.area).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.name.cmp(&b.name)));
    (modules, free)
}

/// Nets crossing each pair of modules, as a weight per (i, j) with i < j.
/// A net wholly inside one module contributes nothing: it is that module's
/// own business and the placer will handle it. Wide nets (over 8 pins --
/// ground, the supply rails) are excluded for the same reason the
/// congestion model excludes them: they touch everything and would flatten
/// every pair weight to the same number.
fn cut_weights(model: &ConstraintModel, modules: &[Module]) -> BTreeMap<(usize, usize), f64> {
    let owner: BTreeMap<&str, usize> = modules.iter().enumerate().flat_map(|(i, m)| m.refs.iter().map(move |r| (r.as_str(), i))).collect();
    let mut w: BTreeMap<(usize, usize), f64> = BTreeMap::new();
    for net in &model.nets {
        if net.pins.len() > 8 {
            continue;
        }
        let mut hit: BTreeSet<usize> = BTreeSet::new();
        for pin in &net.pins {
            let refdes = pin.split('.').next().unwrap_or("");
            if let Some(&m) = owner.get(refdes) {
                hit.insert(m);
            }
        }
        let hit: Vec<usize> = hit.into_iter().collect();
        if hit.len() < 2 {
            continue;
        }
        // Spread one net's weight over the pairs it induces, so a 3-module
        // net does not count as three full 2-module nets.
        let share = 1.0 / (hit.len() - 1) as f64;
        for (k, &a) in hit.iter().enumerate() {
            for &b in &hit[k + 1..] {
                *w.entry((a, b)).or_default() += share;
            }
        }
    }
    w
}

/// Aspect-ratio-capped rectangle for a module: enough area for its parts
/// at [`MODULE_FILL`], shaped toward the board's own proportions so the
/// blocks tile rather than fight.
fn size_for(area: f64, largest: (Um, Um), board_aspect: f64, snap: Um, margin: Um) -> (Um, Um) {
    let need = area / MODULE_FILL;
    let h = (need / board_aspect).sqrt().max(1000.0);
    let w = (need / h).max(1000.0);
    let up = |v: f64| -> Um {
        let s = snap.max(1);
        ((v as Um + s - 1) / s) * s
    };
    // The block must hold its biggest part, plus a margin for the
    // courtyard spacing and its refdes label. Per axis, not squared off: a
    // 41 mm connector needs a 43 mm-wide block, not a 43 mm square, and
    // rounding it up to a square is most of a small board.
    (up(w).max(largest.0 + margin), up(h).max(largest.1 + margin))
}

/// Refdes label band, µm, above a courtyard. The placer keeps parts apart
/// by courtyard *plus* this band, so a block sized to bare courtyards is
/// always a little too small for what the placer actually packs.
pub const MODULE_LABEL_BAND_UM: Um = 1200;

/// A tiny xorshift, kept local so the floorplan reproduces from its seed
/// without pulling the placer's RNG into the model crate.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

fn rect_overlap(a: (Um, Um, Um, Um), b: (Um, Um, Um, Um)) -> f64 {
    let dx = (a.2.min(b.2) - a.0.max(b.0)).max(0) as f64;
    let dy = (a.3.min(b.3) - a.1.max(b.1)).max(0) as f64;
    dx * dy
}

fn centre(r: (Um, Um, Um, Um)) -> (f64, f64) {
    (((r.0 + r.2) as f64) / 2.0, ((r.1 + r.3) as f64) / 2.0)
}

/// Cost of a module arrangement: cut nets pulled short, blocks kept from
/// overlapping, everything kept on the board. The units are µm of
/// centre-to-centre distance; the two penalties are scaled to dominate it,
/// because an overlapping or off-board floorplan is not a worse answer,
/// it is not an answer.
fn arrangement_cost(rects: &[(Um, Um, Um, Um)], w: &BTreeMap<(usize, usize), f64>, bb: (Um, Um, Um, Um), edge: &[bool]) -> f64 {
    let mut c = 0.0;
    for (&(i, j), &weight) in w {
        let (ax, ay) = centre(rects[i]);
        let (bx, by) = centre(rects[j]);
        c += weight * ((ax - bx).abs() + (ay - by).abs());
    }
    for i in 0..rects.len() {
        for j in i + 1..rects.len() {
            c += 40.0 * rect_overlap(rects[i], rects[j]).sqrt();
        }
        let r = rects[i];
        let out = (bb.0 - r.0).max(0) + (r.2 - bb.2).max(0) + (bb.1 - r.1).max(0) + (r.3 - bb.3).max(0);
        c += 200.0 * out as f64;
        if edge[i] {
            // Distance from the block to the nearest board edge. Weighted
            // above the cut-net pull that would otherwise hold it inboard.
            let gap = (r.0 - bb.0).min(bb.2 - r.2).min(r.1 - bb.1).min(bb.3 - r.3).max(0);
            c += 30.0 * gap as f64;
        }
    }
    c
}

/// The board edge an edge-anchored block is pinned to: 0 left, 1 right,
/// 2 top, 3 bottom. Which pair is available is decided by the shape of the
/// block, because that is what decides which way its connector lies: a
/// 41 mm header lying flat can only reach a top or bottom edge.
fn edge_side(r: (Um, Um, Um, Um), bb: (Um, Um, Um, Um)) -> u8 {
    let (w, h) = (r.2 - r.0, r.3 - r.1);
    let gaps = if w >= h { [(r.1 - bb.1, 2u8), (bb.3 - r.3, 3)] } else { [(r.0 - bb.0, 0u8), (bb.2 - r.2, 1)] };
    gaps.iter().copied().min().map(|(_, s)| s).unwrap_or(2)
}

/// Slide a block flush against the edge it is pinned to, keeping its size.
/// Applied after every candidate move rather than once at the end: the
/// placer pins a connector's courtyard a fixed gap from the *board* edge,
/// so a block resting even half a millimetre inboard puts its own
/// connector outside itself. Enforcing it during the search lets the
/// blocks resolve overlaps by sliding along their edge; enforcing it
/// afterwards just pushes them into each other.
fn snap_to_edge(r: (Um, Um, Um, Um), bb: (Um, Um, Um, Um), side: u8) -> (Um, Um, Um, Um) {
    let (w, h) = (r.2 - r.0, r.3 - r.1);
    match side {
        0 => (bb.0, r.1, bb.0 + w, r.3),
        1 => (bb.2 - w, r.1, bb.2, r.3),
        2 => (r.0, bb.1, r.2, bb.1 + h),
        _ => (r.0, bb.3 - h, r.2, bb.3),
    }
}

/// Lay the modules out on the board.
///
/// Annealed rectangle placement: minimise the cut-net pull between blocks
/// while keeping them disjoint and on the board. This is the same shape of
/// problem as part placement but two orders of magnitude smaller -- a
/// hundred parts become a dozen blocks -- which is exactly why it is worth
/// doing first.
///
/// Fails rather than returning an overlapping plan: a floorplan whose
/// blocks share board area is not a floorplan.
pub fn arrange(model: &ConstraintModel, modules: &[Module], bb: (Um, Um, Um, Um), snap: Um, seed: u64) -> Result<Floorplan, String> {
    if modules.is_empty() {
        return Ok(Floorplan::default());
    }
    let bw = (bb.2 - bb.0) as f64;
    let bh = (bb.3 - bb.1) as f64;
    let aspect = if bh > 0.0 { bw / bh } else { 1.0 };
    let sizes: Vec<(Um, Um)> =     {
        // The placer separates parts by courtyard plus its own spacing and
        // the refdes band, so that is what a block has to hold.
        let margin = 2 * model.solver.place_spacing_um + MODULE_LABEL_BAND_UM;
        modules.iter().map(|m| size_for(m.area, m.largest, aspect, snap, margin)).collect()
    };
    let need: f64 = sizes.iter().map(|s| (s.0 as f64) * (s.1 as f64)).sum();
    if need > bw * bh {
        return Err(format!(
            "modules need {:.0} mm\u{b2} of board at {:.0}% fill but the board is {:.0} mm\u{b2}: give the parts more board, or fewer modules",
            need / 1e6,
            MODULE_FILL * 100.0,
            bw * bh / 1e6
        ));
    }
    // Seed with a shelf pack: rows of blocks in the order `partition`
    // returned them (largest first). Annealing from a legal arrangement
    // beats annealing from a random one.
    let mut rects: Vec<(Um, Um, Um, Um)> = Vec::with_capacity(modules.len());
    let (mut cx, mut cy, mut row_h) = (bb.0, bb.1, 0);
    for &(w, h) in &sizes {
        if cx + w > bb.2 && cx > bb.0 {
            cx = bb.0;
            cy += row_h;
            row_h = 0;
        }
        rects.push((cx, cy, cx + w, cy + h));
        cx += w;
        row_h = row_h.max(h);
    }
    let weights = cut_weights(model, modules);
    let edge: Vec<bool> = modules.iter().map(|m| m.edge).collect();
    // Pin each edge block to a side once, from the shelf pack, and hold it
    // there for the whole search. Re-choosing the side every move would
    // let a block teleport across the board and makes the cost landscape
    // discontinuous; a block that wants the other edge gets there through
    // the rotate move, which changes its shape and so its available pair.
    let mut side: Vec<u8> = rects.iter().map(|&r| edge_side(r, bb)).collect();
    for i in 0..modules.len() {
        if edge[i] {
            rects[i] = snap_to_edge(rects[i], bb, side[i]);
        }
    }
    let mut rng = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
    let mut cost = arrangement_cost(&rects, &weights, bb, &edge);
    let n = modules.len();
    let steps = 6000 * n;
    let span = ((bw.max(bh)) / 3.0) as Um;
    for step in 0..steps {
        let t = 1.0 - (step as f64) / (steps as f64);
        let temp = 0.25 * t * t * cost.max(1.0) / (n as f64);
        let i = rng.below(n);
        let before = rects.clone();
        if rng.below(8) == 0 {
            // Turn the block a quarter turn about its own centre. A block
            // holding a 41 mm connector is long and thin, and which way it
            // lies decides whether it can sit against a top edge or a side
            // one. Without this move the shelf pack's orientation is final
            // and half the board's edges are unreachable.
            let r = rects[i];
            let (cx, cy) = ((r.0 + r.2) / 2, (r.1 + r.3) / 2);
            let (w, h) = (r.2 - r.0, r.3 - r.1);
            rects[i] = (cx - h / 2, cy - w / 2, cx - h / 2 + h, cy - w / 2 + w);
        } else if rng.below(4) == 0 && n > 1 {
            // Swap two blocks' positions -- the move that escapes a bad
            // shelf order, which nudging alone never can.
            let j = rng.below(n);
            if i == j {
                continue;
            }
            let (ai, aj) = (rects[i], rects[j]);
            rects[i] = (aj.0, aj.1, aj.0 + (ai.2 - ai.0), aj.1 + (ai.3 - ai.1));
            rects[j] = (ai.0, ai.1, ai.0 + (aj.2 - aj.0), ai.1 + (aj.3 - aj.1));
        } else {
            let reach = (span as f64 * (0.15 + 0.85 * t)) as Um;
            let dx = (rng.below((2 * reach.max(snap)) as usize + 1) as Um - reach.max(snap)) / snap.max(1) * snap.max(1);
            let dy = (rng.below((2 * reach.max(snap)) as usize + 1) as Um - reach.max(snap)) / snap.max(1) * snap.max(1);
            let r = rects[i];
            rects[i] = (r.0 + dx, r.1 + dy, r.2 + dx, r.3 + dy);
        }
        // A rotated block has a new long axis, so it may now belong to
        // the other pair of edges; every edge block is re-seated flush
        // before the move is priced.
        let side_before = side.clone();
        if edge[i] {
            side[i] = edge_side(rects[i], bb);
        }
        for k in 0..n {
            if edge[k] {
                rects[k] = snap_to_edge(rects[k], bb, side[k]);
            }
        }
        let c = arrangement_cost(&rects, &weights, bb, &edge);
        let accept = c < cost || (temp > 0.0 && ((cost - c) / temp).exp() * (u32::MAX as f64) > (rng.next() >> 32) as f64);
        if accept {
            cost = c;
        } else {
            rects = before;
            side = side_before;
        }
    }
    // Hard gate on the result, not a warning: overlapping blocks or a
    // block off the board mean the plan cannot be handed to a placer.
    for i in 0..n {
        let r = rects[i];
        if r.0 < bb.0 || r.1 < bb.1 || r.2 > bb.2 || r.3 > bb.3 {
            return Err(format!("module {} landed off the board at ({}, {})-({}, {})", modules[i].name, r.0, r.1, r.2, r.3));
        }
        for j in i + 1..n {
            let ov = rect_overlap(r, rects[j]);
            if ov > 0.0 {
                return Err(format!("modules {} and {} overlap by {:.1} mm\u{b2}", modules[i].name, modules[j].name, ov / 1e6));
            }
        }
    }
    Ok(Floorplan {
        modules: modules.iter().zip(rects).map(|(m, r)| PlacedModule { name: m.name.clone(), refs: m.refs.clone(), rect: r }).collect(),
        free: Vec::new(),
    })
}

/// Partition and arrange in one call: the whole floorplan stage.
pub fn plan(model: &ConstraintModel, bb: (Um, Um, Um, Um), snap: Um, seed: u64) -> Result<Floorplan, String> {
    let (modules, free) = partition(model);
    let mut fp = arrange(model, &modules, bb, snap, seed)?;
    fp.free = free;
    Ok(fp)
}
