//! `eda-router` — a native grid-maze autorouter (v1), architecture-inspired
//! by FreeRouting but implemented from scratch against our own IR: no Java,
//! no DSN round-trip. Grid A* per net with rip-up & reroute on contention.
//!
//! See `route()` for the entry point and the module docs on `pads`, `grid`,
//! and `astar` for the pieces.

pub mod astar;
pub mod grid;
pub mod pads;

use eda_model::ir::{Design, Point, RoutingSection, Track, Via};
use eda_model::{CheckResult, ConstraintModel};
use grid::{Grid, Occ};
use std::collections::{HashMap, HashSet, VecDeque};

/// Routing rules are the model's board rules; re-exported under the name
/// the router API has always used.
pub use eda_model::BoardRules as RouteRules;

/// A "REF.PIN"-addressed pad and its net (or a synthetic private net for
/// unassigned pins, so they still act as physical obstacles).
struct PadInfo {
    pt: Point,
    size: (i64, i64),
    /// Full board-space pad geometry (shape-aware containment).
    geom: eda_model::footprint::PlacedPad,
    net: String,
    /// Copper layers this pad exists on.
    layers: Vec<u8>,
}

/// Grid cells whose centre lies strictly inside the pad's copper, so a
/// track ending on any of them is guaranteed to touch the pad. Computed
/// from the real rectangle: a pad centre is rarely grid-aligned.
fn pad_interior_cells(grid: &Grid, pad: &PadInfo, rules: &RouteRules) -> Vec<(i64, i64)> {
    let (cx, cy) = grid.to_cell(pad.pt);
    let hx = (pad.size.0 / 2 + rules.grid) / rules.grid;
    let hy = (pad.size.1 / 2 + rules.grid) / rules.grid;
    // A track ending on a goal cell must keep its whole copper width
    // inside the pad (KiCad connectivity wants the end point inside the
    // pad shape; rounded corners and circles have less copper than their
    // bounding box). The centre cell is always included: on tiny pads the
    // margin may exceed the pad, and the centre is the best we can do.
    let margin = (rules.track_width as f64) / 2.0;
    let mut out = vec![(cx, cy)];
    for dx in -hx..=hx {
        for dy in -hy..=hy {
            let p = grid.to_point(cx + dx, cy + dy);
            if (dx, dy) != (0, 0) && pad.geom.contains_with_margin(p, margin) {
                out.push((cx + dx, cy + dy));
            }
        }
    }
    out
}

/// Grid cells a pad's copper is rasterised over: every cell whose own
/// square intersects the pad rectangle (the pad edge never lies more
/// than half a cell past an occupied cell centre — the invariant `Grid`'s
/// clearance arithmetic relies on), plus the centre cell.
fn pad_cells(grid: &Grid, pad: &PadInfo) -> Vec<(i64, i64)> {
    let (x0, y0) = grid.to_cell(Point { x: pad.pt.x - pad.size.0 / 2, y: pad.pt.y - pad.size.1 / 2 });
    let (x1, y1) = grid.to_cell(Point { x: pad.pt.x + pad.size.0 / 2, y: pad.pt.y + pad.size.1 / 2 });
    let mut out = vec![grid.to_cell(pad.pt)];
    for gx in x0..=x1 {
        for gy in y0..=y1 {
            out.push((gx, gy));
        }
    }
    out
}

/// Hard-block the gap strip between two SMD pads of one footprint whose
/// copper is closer than `BETWEEN_PADS_MAX_GAP`: the strip lies under the
/// component body, and a track threading it (any net, the pads' own
/// included) is what a reviewer sends back. Through-hole pin rows are
/// exempt — routing between header pins is standard practice.
const BETWEEN_PADS_MAX_GAP: i64 = 2000;

/// Smallest facing-edge gap between two SMD pads of the same footprint
/// (adjacent pins on the same package), µm. `i64::MAX` when the board has
/// no multi-pad SMD footprint at all (e.g. every part is a through-hole
/// connector). Used to decide whether the routing grid needs refining to
/// escape a fine-pitch part — see the call site in `route_partial`.
fn smallest_same_footprint_gap(pads: &HashMap<String, PadInfo>) -> i64 {
    let mut by_fp: HashMap<&str, Vec<&PadInfo>> = HashMap::new();
    for (refpin, p) in pads {
        if p.geom.through_hole {
            continue;
        }
        let r = refpin.rsplit_once('.').map(|(r, _)| r).unwrap_or(refpin);
        by_fp.entry(r).or_default().push(p);
    }
    let mut min_gap = i64::MAX;
    for fpads in by_fp.values() {
        for i in 0..fpads.len() {
            for j in i + 1..fpads.len() {
                let (a, b) = (fpads[i], fpads[j]);
                let ra = (a.pt.x - a.size.0 / 2, a.pt.y - a.size.1 / 2, a.pt.x + a.size.0 / 2, a.pt.y + a.size.1 / 2);
                let rb = (b.pt.x - b.size.0 / 2, b.pt.y - b.size.1 / 2, b.pt.x + b.size.0 / 2, b.pt.y + b.size.1 / 2);
                let (oy0, oy1) = (ra.1.max(rb.1), ra.3.min(rb.3));
                let (ox0, ox1) = (ra.0.max(rb.0), ra.2.min(rb.2));
                let gap = if oy1 > oy0 && (ra.2 <= rb.0 || rb.2 <= ra.0) {
                    if ra.2 <= rb.0 { rb.0 - ra.2 } else { ra.0 - rb.2 }
                } else if ox1 > ox0 && (ra.3 <= rb.1 || rb.3 <= ra.1) {
                    if ra.3 <= rb.1 { rb.1 - ra.3 } else { ra.1 - rb.3 }
                } else {
                    continue;
                };
                min_gap = min_gap.min(gap);
            }
        }
    }
    min_gap
}

fn block_pad_gaps(grid: &mut Grid, pads: &HashMap<String, PadInfo>) {
    let mut by_fp: HashMap<&str, Vec<&PadInfo>> = HashMap::new();
    for (refpin, p) in pads {
        if p.geom.through_hole {
            continue;
        }
        let r = refpin.rsplit_once('.').map(|(r, _)| r).unwrap_or(refpin);
        by_fp.entry(r).or_default().push(p);
    }
    for fpads in by_fp.values() {
        for i in 0..fpads.len() {
            for j in i + 1..fpads.len() {
                let (a, b) = (fpads[i], fpads[j]);
                let ra = (a.pt.x - a.size.0 / 2, a.pt.y - a.size.1 / 2, a.pt.x + a.size.0 / 2, a.pt.y + a.size.1 / 2);
                let rb = (b.pt.x - b.size.0 / 2, b.pt.y - b.size.1 / 2, b.pt.x + b.size.0 / 2, b.pt.y + b.size.1 / 2);
                let (ox0, ox1) = (ra.0.max(rb.0), ra.2.min(rb.2));
                let (oy0, oy1) = (ra.1.max(rb.1), ra.3.min(rb.3));
                // Strip between the facing edges, limited to the pads'
                // shared extent along the other axis.
                let strip = if oy1 > oy0 && (ra.2 <= rb.0 || rb.2 <= ra.0) {
                    let (gx0, gx1) = if ra.2 <= rb.0 { (ra.2, rb.0) } else { (rb.2, ra.0) };
                    if gx1 - gx0 > BETWEEN_PADS_MAX_GAP { continue }
                    (gx0, oy0, gx1, oy1)
                } else if ox1 > ox0 && (ra.3 <= rb.1 || rb.3 <= ra.1) {
                    let (gy0, gy1) = if ra.3 <= rb.1 { (ra.3, rb.1) } else { (rb.3, ra.1) };
                    if gy1 - gy0 > BETWEEN_PADS_MAX_GAP { continue }
                    (ox0, gy0, ox1, gy1)
                } else {
                    continue;
                };
                // Only adjacent pairs: a strip that crosses a third pad of
                // the footprint (pins 1 and 3 of a row) is not a gap.
                let crosses_other = fpads.iter().enumerate().any(|(k, o)| {
                    k != i && k != j && {
                        let ro = (o.pt.x - o.size.0 / 2, o.pt.y - o.size.1 / 2, o.pt.x + o.size.0 / 2, o.pt.y + o.size.1 / 2);
                        ro.0 < strip.2 && ro.2 > strip.0 && ro.1 < strip.3 && ro.3 > strip.1
                    }
                });
                if crosses_other {
                    continue;
                }
                let (c0x, c0y) = grid.to_cell(Point { x: strip.0, y: strip.1 });
                let (c1x, c1y) = grid.to_cell(Point { x: strip.2, y: strip.3 });
                for cx in c0x..=c1x {
                    for cy in c0y..=c1y {
                        let p = grid.to_point(cx, cy);
                        if p.x > strip.0 && p.x < strip.2 && p.y > strip.1 && p.y < strip.3 {
                            for &l in &a.layers {
                                grid.set_blocked(cx, cy, l, true);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Per-cell surcharge for a track under a refdes label once the strict
/// pass has given up: 8 steps per cell at the 254 um reference grid, so
/// crossing a ~600 µm label costs about a 2 mm detour. 16 and 24 were
/// tried on the corpus and changed nothing the strict pass hadn't already
/// decided. Cell-denominated: the detour it buys is measured in cells,
/// so on a finer grid the per-cell price scales up to keep the same
/// physical detour-per-label ratio (see `refdes_penalty`). At the halved
/// 127 µm grid the old flat 8 bought only a 1 mm detour and A* clipped
/// label corners by 15 µm on the fallback pass.
const REFDES_PENALTY: u8 = 8;

/// `REFDES_PENALTY` rescaled to the effective grid so that clipping a
/// label costs the same physical detour regardless of resolution.
fn refdes_penalty(grid_um: i64) -> u8 {
    ((REFDES_PENALTY as i64 * 254 + grid_um / 2) / grid_um.max(1)).clamp(1, 255) as u8
}

/// Mark the cells under each refdes label as a soft keep-out (enforced on
/// the strict pass, penalised on the fallback). Silkscreen text over
/// copper is legal, but the label becomes unreadable; the judge renders
/// each refdes centred just above its courtyard on the part's own side
/// (see `eda_judge::render_board_svg` and `eda_gates::pcb::refdes_box`,
/// which this mirrors).
fn penalise_refdes_boxes(grid: &mut Grid, placement: &eda_model::ir::PlacementSection, model: &ConstraintModel, num_layers: usize) {
    let penalty = refdes_penalty(grid.grid_um);
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for p in &placement.outline {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    let m = 2000;
    let (vw, vh) = (x1 - x0 + 2 * m, y1 - y0 + 2 * m);
    let fs = (vw.min(vh) / 40).max(600);
    for fp in &placement.footprints {
        let Some(part) = model.part(&fp.id) else { continue };
        let Some((cx0, cy0, cx1, _)) = eda_model::footprint::placed_courtyard(model, part, fp) else { continue };
        let baseline = cy0 - 200;
        let half_w = (fs * 6 / 10) * fp.id.chars().count() as i64 / 2;
        let cx = (cx0 + cx1) / 2;
        let bx = (cx - half_w, baseline - fs * 3 / 4, cx + half_w, baseline + fs / 5);
        let layer = match fp.side {
            eda_model::ir::Side::Top => 0u8,
            eda_model::ir::Side::Bottom => (num_layers - 1) as u8,
        };
        // Two separate dilations, matching the two shapes the gate itself
        // checks against the label box: a track's centreline (dilated by
        // its own half-width) and a via's copper (dilated by the via
        // radius). Using the via radius for both — as a single keep-out
        // used to — over-blocks the track case by up to a via-vs-track
        // width difference on every side, which is enough to wall off a
        // pad on a dense board where a label's soft box sits close to a
        // neighbour's escape route with no other legal cell to spare.
        let dt = grid.track_half_um;
        let dv = grid.via_half_um;
        let (t0x, t0y) = grid.to_cell(Point { x: bx.0 - dt, y: bx.1 - dt });
        let (t1x, t1y) = grid.to_cell(Point { x: bx.2 + dt, y: bx.3 + dt });
        for gx in t0x..=t1x {
            for gy in t0y..=t1y {
                grid.add_penalty(gx, gy, layer, penalty);
                grid.set_soft(gx, gy, layer);
            }
        }
        let (v0x, v0y) = grid.to_cell(Point { x: bx.0 - dv, y: bx.1 - dv });
        let (v1x, v1y) = grid.to_cell(Point { x: bx.2 + dv, y: bx.3 + dv });
        for gx in v0x..=v1x {
            for gy in v0y..=v1y {
                grid.set_soft_via(gx, gy, layer);
            }
        }
    }
}

/// How hard the refdes-label keep-outs are enforced for one attempt.
/// Only `Strict` is used by the router: label crossings are a hard fail
/// (the gate rejects them, so routing through one never produced a
/// shippable candidate). The relaxed tiers are kept for diagnostics.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)]
enum SoftMode {
    /// Tracks and vias both kept out of label boxes.
    Strict,
    /// Tracks may cross a label (paying `REFDES_PENALTY`); vias may not.
    TracksRelaxed,
    /// Everything merely pays the penalty.
    Relaxed,
}

/// One routing step: connect `a_pin` to the net's already-connected
/// copper (pads of earlier steps plus every track/via routed so far);
/// `b_pin` is the nearest earlier pin, used for the heuristic and for
/// rip-up region selection. Steps come in Prim (nearest-first) order, so
/// the net grows as a spanning tree instead of a star out of one pad.
struct Edge {
    a_pin: String,
    b_pin: String,
}

/// Grid cell + physical layer of a pad, as an A* endpoint.
type CellLayer = (i64, i64, u8);
/// `route_net`'s failure payload: the (start, goal) of the edge that
/// couldn't be routed and the nets fencing in the pocket the search was
/// stuck in, for rip-up victim selection.
struct EdgeFailure {
    start: CellLayer,
    goal: CellLayer,
    fence: Vec<String>,
}

pub fn route(
    design: &Design,
    model: &ConstraintModel,
    rules: &RouteRules,
    seed: u64,
) -> Result<Design, Vec<CheckResult>> {
    match route_partial(design, model, rules, seed) {
        (Some(d), fails) if fails.is_empty() => Ok(d),
        (_, fails) => Err(fails),
    }
}

/// Like [`route`], but on failure also returns whatever was routed (for
/// review/rendering). `None` design means a precondition failed before
/// routing started. `route` is the gated contract; use this for diagnostics.
pub fn route_partial(
    design: &Design,
    model: &ConstraintModel,
    rules: &RouteRules,
    seed: u64,
) -> (Option<Design>, Vec<CheckResult>) {
    let placement = match &design.placement {
        Some(p) => p,
        None => {
            return (None, vec![CheckResult::fail(
                "route_precondition",
                "design",
                "placement section is required before routing",
            )])
        }
    };
    if placement.outline.len() < 3 {
        return (None, vec![CheckResult::fail(
            "route_precondition",
            "design.placement.outline",
            "board outline needs at least 3 points",
        )]);
    }

    let dbg = std::env::var_os("EDA_ROUTE_DEBUG").is_some();
    let t_start = std::time::Instant::now();
    // --- 1. pad geometry -----------------------------------------------
    let num_layers = rules.layers.len().max(1);
    let mut pads: HashMap<String, PadInfo> = HashMap::new();
    let mut precondition: Vec<CheckResult> = Vec::new();
    for fp in &placement.footprints {
        let Some(part) = model.part(&fp.id) else {
            precondition.push(CheckResult::fail("route_precondition", &fp.id, "placed footprint has no part in the model"));
            continue;
        };
        // SMD pads exist only on their side's outer copper layer; through
        // hole pads exist on every layer. The via cost in A* is what makes
        // hopping layers around a same-side obstacle a deliberate choice.
        let side_layer = match fp.side {
            eda_model::ir::Side::Top => 0u8,
            eda_model::ir::Side::Bottom => (num_layers - 1) as u8,
        };
        let Some(geoms) = pads::pad_positions(model, part, fp) else {
            precondition.push(CheckResult::fail(
                "route_precondition",
                &fp.id,
                format!(
                    "no footprint geometry for part (footprint={:?}, package={:?}); define it in `footprints` or use a built-in package name",
                    part.footprint, part.package
                ),
            ));
            continue;
        };
        for g in geoms {
            let refpin = format!("{}.{}", fp.id, g.number);
            let net = model
                .nets
                .iter()
                .find(|n| n.pins.iter().any(|p| p == &refpin))
                .map(|n| n.name.clone())
                .unwrap_or_else(|| format!("__unassigned__{refpin}"));
            let layers = if g.through_hole { (0..num_layers as u8).collect() } else { vec![side_layer] };
            pads.insert(refpin, PadInfo { pt: g.center, size: g.size, geom: g.clone(), net, layers });
        }
    }
    if !precondition.is_empty() {
        return (None, precondition);
    }

    // The configured grid is a design-rule pitch, not necessarily a
    // resolution the router can escape fine-pitch parts on: a TSSOP at a
    // 650 um pin pitch, say, has no grid cell landing on the centre line
    // between two adjacent pads at a 254 um grid, and the pad-to-pad gap
    // (pitch minus pad size) is regularly smaller than one such cell, so
    // the router sees a wall around the part instead of the real,
    // routable gaps beside it. Halving the grid (still coarser than most
    // clearances, so correctness is untouched) gives escape routing a
    // real shot at those gaps without the cost of an arbitrary GCD-of-
    // pitches grid, which for typical pitch/grid combinations (654 vs
    // 254, gcd 2) would be absurdly fine.
    //
    // This is only worth the (substantial — up to ~4x cells, ~4x search
    // cost) resolution increase when the board actually has a fine-pitch
    // part: scan same-footprint adjacent SMD pad pairs (the same notion
    // `block_pad_gaps` uses below) for the smallest facing-edge gap, and
    // only refine the grid when that gap wouldn't fit a single configured
    // cell. Boards without such a part keep the coarse grid and its
    // established performance.
    let min_pad_gap = smallest_same_footprint_gap(&pads);
    let mut eff_rules = rules.clone();
    if rules.grid > 130 && min_pad_gap < rules.grid {
        eff_rules.grid = rules.grid / 2;
    }
    if std::env::var_os("EDA_ROUTE_DEBUG").is_some() {
        eprintln!("route: min_pad_gap={min_pad_gap} eff_grid={}", eff_rules.grid);
    }
    let rules = &eff_rules;

    // --- 2. grid & obstacle map -----------------------------------------
    if dbg { eprintln!("route: t pads {:?}", t_start.elapsed()); }
    let mut grid = Grid::with_widths(placement.outline.clone(), rules.grid, rules.clearance, rules.track_width, rules.via_diameter, num_layers);
    for pad in pads.values() {
        // Rasterise the pad area from its real rectangle: every cell whose
        // own square intersects the copper (the cell nearest a corner is
        // the cell whose square contains it), so the pad edge never lies
        // more than half a cell past an occupied cell centre — the
        // invariant `Grid`'s clearance arithmetic relies on. Centring on
        // the rounded centre cell instead can miss a row when the pad
        // centre is off-grid.
        for &l in &pad.layers {
            for (gx, gy) in pad_cells(&grid, pad) {
                grid.set(gx, gy, l, &pad.net, Occ::Pad);
            }
        }
    }
    block_pad_gaps(&mut grid, &pads);
    penalise_refdes_boxes(&mut grid, placement, model, num_layers);

    if dbg { eprintln!("route: t grid {:?} cells={}x{}x{}", t_start.elapsed(), grid.cells_x, grid.cells_y, grid.num_layers); }
    // --- 3. net -> star edges --------------------------------------------
    let mut edges_by_net: HashMap<String, Vec<Edge>> = HashMap::new();
    let mut airline_by_net: HashMap<String, i64> = HashMap::new();
    let mut skipped: Vec<CheckResult> = Vec::new();

    for net in &model.nets {
        let mut resolved: Vec<&String> = Vec::new();
        for p in &net.pins {
            if pads.contains_key(p) {
                resolved.push(p);
            } else {
                skipped.push(CheckResult::fail(
                    "route_pin_missing",
                    format!("{}/{}", net.name, p),
                    "pin has no resolvable pad (unknown footprint/part)",
                ));
            }
        }
        if resolved.len() < 2 {
            continue;
        }
        // Prim's MST over pad centres (Manhattan), deterministic: ties go
        // to the lexically smaller pin ref.
        let mut es = Vec::new();
        let mut total = 0i64;
        let mut in_tree: Vec<&String> = vec![resolved[0]];
        let mut rest: Vec<&String> = resolved[1..].to_vec();
        while !rest.is_empty() {
            let mut best: Option<(i64, usize, &String)> = None;
            for (ri, r) in rest.iter().enumerate() {
                for t in &in_tree {
                    let (a, b) = (&pads[*r].pt, &pads[*t].pt);
                    let d = (a.x - b.x).abs() + (a.y - b.y).abs();
                    let better = match best {
                        None => true,
                        Some((bd, bri, bt)) => d < bd || (d == bd && (r.as_str(), t.as_str()) < (rest[bri].as_str(), bt.as_str())),
                    };
                    if better {
                        best = Some((d, ri, t));
                    }
                }
            }
            let (d, ri, t) = best.unwrap();
            let r = rest.remove(ri);
            total += d;
            es.push(Edge { a_pin: r.clone(), b_pin: t.clone() });
            in_tree.push(r);
        }
        airline_by_net.insert(net.name.clone(), total);
        edges_by_net.insert(net.name.clone(), es);
    }

    // --- 4. deterministic net ordering: shortest airline first, seed
    // shuffles only within ties -------------------------------------------
    let mut nets: Vec<String> = edges_by_net.keys().cloned().collect();
    nets.sort_by(|a, b| (airline_by_net[a], a).cmp(&(airline_by_net[b], b)));
    shuffle_ties(&mut nets, &airline_by_net, seed);

    if dbg { eprintln!("route: t edges {:?}", t_start.elapsed()); }
    // --- 5. route with rip-up & reroute -----------------------------------
    let mut queue: VecDeque<String> = nets.into();
    let mut ripup_rounds: HashMap<String, u32> = HashMap::new();
    let mut routed_order: Vec<String> = Vec::new();
    let mut tracks: HashMap<String, Vec<Track>> = HashMap::new();
    let mut vias: HashMap<String, Vec<Via>> = HashMap::new();
    let mut fails: Vec<CheckResult> = skipped;
    let mut permanently_failed: HashSet<String> = HashSet::new();

    let max_rounds: u32 = 6;
    let max_victims: usize = 6;
    let iter_budget = 10 * queue.len().max(1) + 50;
    let mut iters = 0usize;

    while let Some(net) = queue.pop_front() {
        iters += 1;
        if iters > iter_budget {
            fails.push(CheckResult::fail("route_net_unrouted", &net, "router iteration budget exceeded"));
            continue;
        }
        if permanently_failed.contains(&net) {
            continue;
        }

        // Refdes keep-outs are enforced first; a net that can only route
        // through a label rips up what fenced it in, and pays the label
        // penalty only once its rip-up budget is spent.
        // Refdes keep-outs are hard: a net that cannot route around the
        // labels after its rip-up budget fails, and the failure goes
        // upstream to placement rather than being papered over with a
        // label crossing.
        let rounds = *ripup_rounds.get(&net).unwrap_or(&0);
        let t_net = std::time::Instant::now();
        let result = route_net(&net, &edges_by_net[&net], &pads, &mut grid, rules, SoftMode::Strict);
        if dbg {
            eprintln!("route: net {net} round {rounds} ok={} in {:?}", result.is_ok(), t_net.elapsed());
        }
        match result {
            Ok((net_tracks, net_vias)) => {
                tracks.insert(net.clone(), net_tracks);
                vias.insert(net.clone(), net_vias);
                routed_order.retain(|n| n != &net);
                routed_order.push(net.clone());
            }
            Err(EdgeFailure { start: fa, goal: fb, fence }) => {
                // Expressed as a physical distance (four cells at the
                // reference 254 um grid) rather than a fixed cell count:
                // a finer grid must still search the same real-world
                // rip-up neighbourhood, or victim selection quietly loses
                // reach and the router falls back to the refdes-penalty
                // pass far more often than the coarse grid ever did.
                let margin = ((4 * 254 + rules.grid - 1) / rules.grid).max(1);
                // Prefer the nets fencing in the pocket the search was
                // stuck in (a pad enclosed by its neighbours' escape
                // tracks is the common case); fall back to everything in
                // the start-goal region when the search ran out of budget
                // in the open.
                let mut victims: Vec<String> = fence.into_iter().filter(|n| routed_order.contains(n)).collect();
                if victims.is_empty() {
                    victims = grid
                        .nets_in_region((fa.0, fa.1), (fb.0, fb.1), margin)
                        .into_iter()
                        .filter(|n| n != &net && routed_order.contains(n))
                        .collect();
                }
                // Shortest-airline-first, cap at 3: a short net is cheap to
                // reroute (little room for its replacement path to get
                // worse) and, since nets were routed shortest-first to
                // begin with, ripping one up reopens grid cells that were
                // claimed early — before the board got congested — so the
                // net doing the ripping (which by construction lost to
                // *something* already routed) has more freedom to find a
                // clean path through them.
                let mut victims: Vec<String> = victims;
                victims.sort_by_key(|n| (airline_by_net.get(n).copied().unwrap_or(i64::MAX), n.clone()));
                victims.truncate(max_victims);
                if std::env::var_os("EDA_ROUTE_DEBUG").is_some() {
                    eprintln!("route: net {net} failed (round {rounds}) region {:?}-{:?}; victims {:?}", fa, fb, victims);
                }

                if rounds < max_rounds && !victims.is_empty() {
                    ripup_rounds.insert(net.clone(), rounds + 1);
                    // The victims are cleared; this net's own partial
                    // copper from the failed attempt was never committed.
                    for v in &victims {
                        grid.clear_net(v);
                        tracks.remove(v);
                        vias.remove(v);
                        routed_order.retain(|n| n != v);
                        queue.push_back(v.clone());
                    }
                    // Retry this net after its blockers are cleared.
                    queue.push_front(net.clone());
                } else if rounds < max_rounds {
                    // Nothing to rip up: skip straight to the
                    // penalty-only pass rather than burn rounds.
                    ripup_rounds.insert(net.clone(), max_rounds);
                    queue.push_front(net.clone());
                } else {
                    permanently_failed.insert(net.clone());
                    fails.push(CheckResult::fail(
                        "route_net_unrouted",
                        &net,
                        "grid maze router could not find a path after rip-up budget was exhausted",
                    ));
                }
            }
        }
    }

    if dbg { eprintln!("route: t routed {:?}", t_start.elapsed()); }
    // --- 6. assemble output design ----------------------------------------
    let mut out = design.clone();
    let mut all_tracks: Vec<Track> = tracks.into_values().flatten().collect();
    let mut all_vias: Vec<Via> = vias.into_values().flatten().collect();
    all_tracks.sort_by(|a, b| (&a.net, &a.layer, a.pts.first()).cmp(&(&b.net, &b.layer, b.pts.first())));
    all_vias.sort_by(|a, b| (&a.net, a.at).cmp(&(&b.net, b.at)));
    out.routing = Some(RoutingSection { tracks: all_tracks, vias: all_vias, zones: vec![] });

    // Contract: `route` fails loudly on any unrouted net; the partial
    // design is only exposed through `route_partial` for diagnostics.
    (Some(out), fails)
}

/// Attempts to route every star edge of one net. On success returns the new
/// tracks/vias for the net. On failure returns the start/goal grid cells of
/// the edge that failed, for rip-up victim selection.
fn route_net(
    net: &str,
    edges: &[Edge],
    pads: &HashMap<String, PadInfo>,
    grid: &mut Grid,
    rules: &RouteRules,
    mode: SoftMode,
) -> Result<(Vec<Track>, Vec<Via>), EdgeFailure> {
    let mut tracks = Vec::new();
    let mut vias = Vec::new();
    // A previous failed attempt may have left this net's partial copper
    // in the grid (its tracks were never committed); start clean.
    grid.clear_net(net);

    // Pins connected so far (their pad cells are valid goals).
    let mut connected: Vec<&String> = Vec::new();
    if let Some(first) = edges.first() {
        connected.push(&first.b_pin);
    }
    for e in edges {
        let pa = &pads[&e.a_pin];
        let pb = &pads[&e.b_pin];
        let (ax, ay) = grid.to_cell(pa.pt);
        let (bx, by) = grid.to_cell(pb.pt);
        let start = (ax, ay, pa.layers[0]);
        let goal = (bx, by, pb.layers[0]);
        // Every layer the start pad's copper actually exists on is a
        // legal place to begin the track (through-hole pads exist on
        // every layer); seeding all of them keeps the router from paying
        // for a via just because it had to pick one layer to call "the"
        // start.
        let starts: Vec<(i64, i64, u8)> = pa.layers.iter().map(|&l| (ax, ay, l)).collect();

        // Goal set: routed copper of this net plus the pads of connected pins.
        let mut goals: HashSet<(i64, i64, u8)> = grid.routed_cells_of(net).into_iter().collect();
        let mut h_targets: Vec<(i64, i64)> = Vec::new();
        for c in &connected {
            let pc = &pads[*c];
            let (cx, cy) = grid.to_cell(pc.pt);
            for &l in &pc.layers {
                for (gx, gy) in pad_interior_cells(grid, pc, rules) {
                    goals.insert((gx, gy, l));
                }
            }
            h_targets.push((cx, cy));
        }
        if h_targets.is_empty() {
            h_targets.push((bx, by));
        }
        // Never terminate inside the start pad itself.
        let shx = (pa.size.0 / 2 + rules.grid / 2) / rules.grid;
        let shy = (pa.size.1 / 2 + rules.grid / 2) / rules.grid;
        for &l in &pa.layers {
            for dx in -shx..=shx {
                for dy in -shy..=shy {
                    goals.remove(&(ax + dx, ay + dy, l));
                }
            }
        }

        // Own-net pads that are neither the start nor already connected
        // are not stepping stones: a track running straight through a pad
        // it doesn't terminate on is a pass-through, so block them for
        // this edge (a later edge connects them properly).
        let stepping_stones: Vec<&PadInfo> = edges
            .iter()
            .flat_map(|x| [&x.a_pin, &x.b_pin])
            .filter(|p| *p != &e.a_pin && !connected.contains(p))
            .map(|p| &pads[p])
            .collect();
        let mut blocked_cells: Vec<(i64, i64, u8)> = Vec::new();
        for p in &stepping_stones {
            for (gx, gy) in pad_cells(grid, p) {
                for &l in &p.layers {
                    if !grid.is_blocked(gx, gy, l) {
                        grid.set_blocked(gx, gy, l, true);
                        blocked_cells.push((gx, gy, l));
                    }
                }
            }
        }
        // First with the soft keep-outs (refdes labels) enforced; only if
        // that leaves no path at all, fall back to paying the penalty.
        // Per edge, not per net: an edge that routes clean under the
        // strict keep-outs keeps that result even when a sibling edge of
        // the same net needs a relaxed tier, so a net that fell back for
        // one boxed-in pad doesn't start clipping labels everywhere else.
        let tiers: &[SoftMode] = match mode {
            SoftMode::Strict => &[SoftMode::Strict],
            SoftMode::TracksRelaxed => &[SoftMode::Strict, SoftMode::TracksRelaxed],
            SoftMode::Relaxed => &[SoftMode::Strict, SoftMode::TracksRelaxed, SoftMode::Relaxed],
        };
        let mut found = None;
        let mut explored = Vec::new();
        for &tier in tiers {
            grid.soft_active = tier == SoftMode::Strict;
            grid.soft_via_active = tier != SoftMode::Relaxed;
            let (f, e) = astar::route_to_any_ex(grid, net, &starts, &goals, &h_targets);
            grid.soft_active = false;
            grid.soft_via_active = false;
            found = f;
            explored = e;
            if found.is_some() {
                break;
            }
        }
        for &(gx, gy, l) in &blocked_cells {
            grid.set_blocked(gx, gy, l, false);
        }

        match found {
            Some(path) => {
                connected.push(&e.a_pin);
                // Mark cells so later edges of the same star net see this
                // one as routed (same net, so still passable for them).
                for (i, &(cx, cy, l)) in path.iter().enumerate() {
                    let is_via = (i > 0 && path[i - 1].2 != l) || (i + 1 < path.len() && path[i + 1].2 != l);
                    grid.set(cx, cy, l, net, if is_via { Occ::Via } else { Occ::Track });
                }
                // Which connected pin (if any) does the path end on?
                let end = path.last().copied().unwrap_or(start);
                let end_pin: Option<&String> = connected.iter().copied().find(|c| {
                    let pc = &pads[*c];
                    pc.layers.contains(&end.2) && pad_interior_cells(grid, pc, rules).contains(&(end.0, end.1))
                });
                let (edge_tracks, edge_vias) = path_to_geometry(net, &e.a_pin, end_pin.map(|s| s.as_str()).unwrap_or(""), &path, grid, rules);
                tracks.extend(edge_tracks);
                vias.extend(edge_vias);
            }
            None => {
                if std::env::var_os("EDA_ROUTE_DEBUG").is_some() {
                    eprintln!("route: net {net} pin {} -> nearest {} failed; start {:?} goals {} targets {:?}", e.a_pin, e.b_pin, start, goals.len(), h_targets);
                    for &(gx, gy, gl) in &goals {
                        eprintln!("goal ({gx},{gy},{gl}) passable={}", grid.passable(gx, gy, gl, net));
                    }
                    eprintln!("around goal {:?}:\n{}", goal, grid.dump_around(goal.0, goal.1, goal.2, net, 8));
                    for l in 0..grid.num_layers as u8 {
                        eprintln!("layer {l} around start (passable-as-track: P):\n{}", grid.dump_around(start.0, start.1, l, net, 8));
                        let mut row = String::new();
                        for dy in -8..=8i64 {
                            for dx in -8..=8i64 {
                                row.push(if grid.passable(start.0 + dx, start.1 + dy, l, net) { 'P' } else { '.' });
                            }
                            row.push('\n');
                        }
                        eprintln!("{row}");
                    }
                }
                // The copper bordering the explored pocket is what has to
                // move; the pocket itself is bounded by clearance, so look
                // one clearance radius past its cells.
                // A small explored set means the start is walled in; a
                // large one means the search roamed the board and it is
                // the goal that is walled in.
                let r = grid.clearance_cells + 1;
                let goal_cells: Vec<(i64, i64, u8)> = goals.iter().copied().collect();
                let pocket = if explored.len() < 2_000 { &explored } else { &goal_cells };
                let mut fence: Vec<String> = grid.nets_bordering(pocket, r).into_iter().filter(|n| n != net).collect();
                fence.sort();
                return Err(EdgeFailure { start, goal, fence });
            }
        }
    }
    Ok((tracks, vias))
}

fn path_to_geometry(
    net: &str,
    a_pin: &str,
    b_pin: &str,
    path: &[(i64, i64, u8)],
    grid: &Grid,
    rules: &RouteRules,
) -> (Vec<Track>, Vec<Via>) {
    let mut tracks = Vec::new();
    let mut vias = Vec::new();

    // Split into same-layer runs, inserting a via at each layer change.
    let mut run_start = 0usize;
    for i in 1..path.len() {
        if path[i].2 != path[i - 1].2 {
            let span = SegmentSpan { path, start: run_start, end: i - 1, a_pin, b_pin };
            tracks.push(segment_track(net, &span, grid, rules));
            let (vx, vy, from_l) = path[i - 1];
            let to_l = path[i].2;
            let at = grid.to_point(vx, vy);
            vias.push(Via {
                net: net.to_string(),
                at,
                drill: rules.via_drill,
                diameter: rules.via_diameter,
                from_layer: rules.layers[from_l as usize].clone(),
                to_layer: rules.layers[to_l as usize].clone(),
            });
            run_start = i;
        }
    }
    let span = SegmentSpan { path, start: run_start, end: path.len() - 1, a_pin, b_pin };
    tracks.push(segment_track(net, &span, grid, rules));
    (tracks, vias)
}

/// One same-layer run of a path, plus the pin refs to attach at its ends
/// (only set when that end is the very start/end of the whole path).
struct SegmentSpan<'a> {
    path: &'a [(i64, i64, u8)],
    start: usize,
    end: usize,
    a_pin: &'a str,
    b_pin: &'a str,
}

fn segment_track(net: &str, span: &SegmentSpan, grid: &Grid, rules: &RouteRules) -> Track {
    let SegmentSpan { path, start, end, a_pin, b_pin } = *span;
    let mut pts: Vec<Point> = Vec::new();
    for &(cx, cy, _) in &path[start..=end] {
        let p = grid.to_point(cx, cy);
        if let (Some(&prev2), Some(&prev1)) = (pts.get(pts.len().wrapping_sub(2)), pts.last()) {
            // Collinear merge: drop prev1 if prev2->prev1->p is a straight line.
            if is_collinear(prev2, prev1, p) {
                pts.pop();
            }
        }
        pts.push(p);
    }
    let layer = rules.layers[path[start].2 as usize].clone();
    let mut pins = Vec::new();
    if start == 0 {
        pins.push(a_pin.to_string());
    }
    if end == path.len() - 1 && !b_pin.is_empty() {
        pins.push(b_pin.to_string());
    }
    Track { net: net.to_string(), pins, layer, width: rules.track_width, pts }
}

fn is_collinear(a: Point, b: Point, c: Point) -> bool {
    (b.x - a.x) as i128 * (c.y - a.y) as i128 == (b.y - a.y) as i128 * (c.x - a.x) as i128
}

/// splitmix64, used only to permute equal-airline-length net groups.
fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

fn shuffle_ties(nets: &mut [String], airline: &HashMap<String, i64>, seed: u64) {
    let mut i = 0;
    while i < nets.len() {
        let mut j = i + 1;
        while j < nets.len() && airline[&nets[j]] == airline[&nets[i]] {
            j += 1;
        }
        if j - i > 1 {
            let group = &mut nets[i..j];
            // Deterministic Fisher-Yates keyed off seed + group content.
            let mut state = seed ^ splitmix64(i as u64);
            for k in (1..group.len()).rev() {
                state = splitmix64(state);
                let swap_with = (state as usize) % (k + 1);
                group.swap(k, swap_with);
            }
        }
        i = j;
    }
}
