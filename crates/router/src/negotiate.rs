//! Negotiated-congestion routing (PathFinder): every net routes every
//! iteration, overlaps are allowed and priced, and the price of a
//! contested cell rises with its present use and its history of contention
//! until the nets that have an alternative take it and the ones that do
//! not keep the cell. Converges to a legal routing or fails hard.
//!
//! Static obstacles (pads, between-pad strips, refdes label keep-outs,
//! board edge) stay impassable exactly as in the sequential router — only
//! net-vs-net copper is negotiated.

use crate::astar::{via_cost, StateTable, State, DIRS, NO_DIR, STEP_COST};
use crate::grid::{Grid, Occ};
use crate::{pad_cells, pad_interior_cells, path_to_geometry, Edge, PadInfo, RouteRules};
use eda_model::CheckResult;
use eda_model::ir::{Track, Via};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

// Iteration cap, present-cost start/growth/ceiling and the minimum history
// increment come from `RoutingTuning` (nc_*), settable in the intent.

#[derive(Eq, PartialEq)]
struct QueueItem {
    f: i64,
    g: i64,
    state: State,
}
impl Ord for QueueItem {
    fn cmp(&self, other: &Self) -> Ordering {
        other.f.cmp(&self.f)
    }
}
impl PartialOrd for QueueItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Per (cell, layer) contention state.
struct Claims {
    cells_x: i64,
    cells_y: i64,
    num_layers: usize,
    /// Number of distinct nets whose copper (dilated by separation) covers
    /// the cell.
    claims: Vec<u16>,
    /// Number of nets with actual copper (undilated path cell, via barrel)
    /// on the cell. A conflict is copper of one net inside another net's
    /// dilated claim — `copper > 0 && claims > 1` — never two dilated
    /// claims merely touching, which is what legal minimum spacing looks
    /// like and what made the first version count 1152 "overused" cells on
    /// a board that was actually clean.
    copper: Vec<u16>,
    /// Accumulated overuse.
    hist: Vec<u16>,
    /// Epoch stamp: cells the net being routed right now already covers
    /// (its own earlier edges) — free for it.
    mine: Vec<u32>,
    epoch: u32,
}

impl Claims {
    fn new(grid: &Grid) -> Self {
        let n = (grid.cells_x * grid.cells_y) as usize * grid.num_layers;
        Claims { cells_x: grid.cells_x, cells_y: grid.cells_y, num_layers: grid.num_layers, claims: vec![0; n], copper: vec![0; n], hist: vec![0; n], mine: vec![0; n], epoch: 1 }
    }
    #[inline]
    fn idx(&self, cx: i64, cy: i64, l: u8) -> Option<usize> {
        if cx < 0 || cy < 0 || cx >= self.cells_x || cy >= self.cells_y || (l as usize) >= self.num_layers {
            return None;
        }
        Some(((cy * self.cells_x + cx) as usize) * self.num_layers + l as usize)
    }
    /// Cells covered by a path: every path cell dilated by the track
    /// separation radius on its layer; via cells dilated by the via radius
    /// on every layer.
    /// Cells holding the path's actual copper (vias on every layer).
    fn copper_cells(&self, own_pads: &[&PadInfo], grid: &Grid, path: &[(i64, i64, u8)]) -> Vec<usize> {
        let mut out = Vec::new();
        for (i, &(cx, cy, l)) in path.iter().enumerate() {
            // Inside the net's own pad copper the track adds nothing — the
            // pad is a static obstacle every other net already keeps pad
            // clearance from, and no price can move it. Real copper only:
            // a rasterised pad *cell* can lie half a cell outside the pad,
            // and track copper there does need track clearance (l1: 3V3
            // leaving its pad, GND hugging at pad clearance, 181 um gap).
            if inside_own_pad(own_pads, grid, cx, cy, l) {
                continue;
            }
            let is_via = (i > 0 && path[i - 1].2 != l) || (i + 1 < path.len() && path[i + 1].2 != l);
            if is_via {
                for ol in 0..self.num_layers as u8 {
                    if let Some(j) = self.idx(cx, cy, ol) {
                        out.push(j);
                    }
                }
            } else if let Some(j) = self.idx(cx, cy, l) {
                out.push(j);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    fn footprint(&self, own_pads: &[&PadInfo], grid: &Grid, path: &[(i64, i64, u8)]) -> Vec<usize> {
        let r_t = grid.sep_cells(Occ::Track, grid.track_half_um) - 1;
        let r_v = grid.sep_cells(Occ::Via, grid.via_half_um) - 1;
        let mut out = Vec::new();
        for (i, &(cx, cy, l)) in path.iter().enumerate() {
            if inside_own_pad(own_pads, grid, cx, cy, l) {
                continue;
            }
            let is_via = (i > 0 && path[i - 1].2 != l) || (i + 1 < path.len() && path[i + 1].2 != l);
            if is_via {
                for ol in 0..self.num_layers as u8 {
                    for dx in -r_v..=r_v {
                        for dy in -r_v..=r_v {
                            if let Some(j) = self.idx(cx + dx, cy + dy, ol) {
                                out.push(j);
                            }
                        }
                    }
                }
            } else {
                for dx in -r_t..=r_t {
                    for dy in -r_t..=r_t {
                        if let Some(j) = self.idx(cx + dx, cy + dy, l) {
                            out.push(j);
                        }
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// True when the cell centre lies inside one of the net's own pads with
/// a track half-width to spare, on a layer the pad exists on.
fn inside_own_pad(own_pads: &[&PadInfo], grid: &Grid, cx: i64, cy: i64, l: u8) -> bool {
    let p = grid.to_point(cx, cy);
    own_pads.iter().any(|pd| pd.layers.contains(&l) && pd.geom.contains_with_margin(p, grid.track_half_um as f64))
}

/// A* for one edge under negotiated costs. Hard legality from `grid`
/// (static obstacles only); net-vs-net copper priced by `claims`.
#[allow(clippy::too_many_arguments)]
fn search(
    grid: &Grid,
    claims: &Claims,
    pres_fac: f64,
    net: &str,
    starts: &[(i64, i64, u8)],
    goals: &HashSet<(i64, i64, u8)>,
    h_targets: &[(i64, i64)],
) -> Option<Vec<(i64, i64, u8)>> {
    let mut table = StateTable::new(grid);
    let mut heap = BinaryHeap::new();
    let h = |cx: i64, cy: i64| h_targets.iter().map(|&(gx, gy)| (cx - gx).abs() + (cy - gy).abs()).min().unwrap_or(0);
    for &s in starts {
        let st: State = (s.0, s.1, s.2, NO_DIR);
        let i = table.index(st);
        table.best[i] = 0;
        heap.push(QueueItem { f: h(s.0, s.1), g: 0, state: st });
    }
    let cost_of = |cx: i64, cy: i64, l: u8| -> i64 {
        let Some(j) = claims.idx(cx, cy, l) else { return 0 };
        let foreign = if claims.mine[j] == claims.epoch { 0 } else { claims.claims[j] as i64 };
        // Present cost scales with the step so a contested cell costs a
        // multiple of a free one; history is additive.
        ((pres_fac * foreign as f64 * 4.0) as i64) + claims.hist[j] as i64 + grid.penalty(cx, cy, l)
    };
    // The grid is finite; under negotiated costs the search may legitimately
    // sweep most of it before the cheap cells run out, so no expansion cap.
    let mut goal_state: Option<State> = None;
    while let Some(QueueItem { g, state, .. }) = heap.pop() {
        if g > table.get(state) {
            continue;
        }
        let (cx, cy, layer, dir) = state;
        if goals.contains(&(cx, cy, layer)) {
            goal_state = Some(state);
            break;
        }
        for (i, (dx, dy)) in DIRS.iter().enumerate() {
            let (nx, ny) = (cx + dx, cy + dy);
            if !grid.passable_as(nx, ny, layer, net, Occ::Track) {
                continue;
            }
            let turn = if dir == NO_DIR || dir == i as u8 { 0 } else { grid.tuning.bend_cost };
            let ng = g + STEP_COST + turn + cost_of(nx, ny, layer);
            let ns: State = (nx, ny, layer, i as u8);
            if ng < table.get(ns) {
                table.set(ns, ng, state);
                heap.push(QueueItem { f: ng + h(nx, ny), g: ng, state: ns });
            }
        }
        // Through-hole via: barrel clear (statically) on every layer.
        if grid.num_layers > 1 && !(0..grid.num_layers as u8).any(|ol| !grid.passable_as(cx, cy, ol, net, Occ::Via) || grid.via_near_pad(cx, cy, ol)) {
            // A via's claim radius is wider than a track's, so the copper it
            // will conflict with sits in cells whose claims the via cell
            // itself does not see; price the whole ring it will cover, or
            // the via never pays and the conflict is price-immune (l1/l2:
            // four cells of NRST/SWCLK track inside a via claim, unmoved at
            // pres_fac 2000).
            let r_extra = (grid.sep_cells(Occ::Via, grid.via_half_um) - grid.sep_cells(Occ::Track, grid.track_half_um)).max(0);
            // Per layer, the *worst* cell in the ring, not the sum: a single
            // neighbouring track shows up in most ring cells, and summing
            // priced every via near any copper 25-50x too high — nets then
            // preferred a same-layer crossing at pres_fac 2000 to a legal
            // via (mcu_board_30plus seed 1, GND x PA7).
            // The ring scan is the expensive part of an expansion; skip it
            // unless the via could improve some layer's best even at zero
            // ring cost (barrel >= 0 is a valid lower bound).
            let lb = g + via_cost(grid);
            let worth = (0..grid.num_layers as u8).any(|nl| nl != layer && lb < table.get((cx, cy, nl, NO_DIR)));
            let mut barrel: i64 = 0;
            if worth {
                for ol in 0..grid.num_layers as u8 {
                    let mut worst = 0i64;
                    for dx in -r_extra..=r_extra {
                        for dy in -r_extra..=r_extra {
                            worst = worst.max(cost_of(cx + dx, cy + dy, ol));
                        }
                    }
                    barrel += worst;
                }
            }
            for nl in 0..grid.num_layers as u8 {
                if !worth || nl == layer {
                    continue;
                }
                let ng = g + via_cost(grid) + barrel;
                let ns: State = (cx, cy, nl, NO_DIR);
                if ng < table.get(ns) {
                    table.set(ns, ng, state);
                    heap.push(QueueItem { f: ng + h(cx, cy), g: ng, state: ns });
                }
            }
        }
    }
    let goal_state = goal_state?;
    let mut path = vec![goal_state];
    let mut cur = table.index(goal_state);
    loop {
        let from = table.came_from[cur];
        if from == u32::MAX {
            break;
        }
        path.push(table.unindex(from as usize));
        cur = from as usize;
    }
    path.reverse();
    Some(path.into_iter().map(|(x, y, l, _)| (x, y, l)).collect())
}

/// Route every edge of one net against the current claims; returns one
/// path per edge (in edge order) and the end pin each path landed on.
fn route_net(
    net: &str,
    edges: &[Edge],
    pads: &HashMap<String, PadInfo>,
    grid: &mut Grid,
    rules: &RouteRules,
    claims: &mut Claims,
    pres_fac: f64,
) -> Option<Vec<(Vec<(i64, i64, u8)>, String)>> {
    claims.epoch += 1;
    let mut out = Vec::new();
    let mut own: HashSet<(i64, i64, u8)> = HashSet::new();
    let mut connected: Vec<&String> = Vec::new();
    if let Some(first) = edges.first() {
        connected.push(&first.b_pin);
    }
    for e in edges {
        let pa = &pads[&e.a_pin];
        let pb = &pads[&e.b_pin];
        let (ax, ay) = grid.to_cell(pa.pt);
        let (bx, by) = grid.to_cell(pb.pt);
        let starts: Vec<(i64, i64, u8)> = pa.layers.iter().map(|&l| (ax, ay, l)).collect();
        let mut goals: HashSet<(i64, i64, u8)> = own.clone();
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
        let shx = (pa.size.0 / 2 + rules.grid / 2) / rules.grid;
        let shy = (pa.size.1 / 2 + rules.grid / 2) / rules.grid;
        for &l in &pa.layers {
            for dx in -shx..=shx {
                for dy in -shy..=shy {
                    goals.remove(&(ax + dx, ay + dy, l));
                }
            }
        }
        // Own-net pads not yet connected are not stepping stones.
        let stepping: Vec<&PadInfo> =
            edges.iter().flat_map(|x| [&x.a_pin, &x.b_pin]).filter(|p| *p != &e.a_pin && !connected.contains(p)).map(|p| &pads[p]).collect();
        let mut blocked = Vec::new();
        for p in &stepping {
            for (gx, gy) in pad_cells(grid, p) {
                for &l in &p.layers {
                    if !grid.is_blocked(gx, gy, l) {
                        grid.set_blocked(gx, gy, l, true);
                        blocked.push((gx, gy, l));
                    }
                }
            }
        }
        grid.soft_active = true;
        grid.soft_via_active = true;
        let found = search(grid, claims, pres_fac, net, &starts, &goals, &h_targets);
        grid.soft_active = false;
        grid.soft_via_active = false;
        for &(gx, gy, l) in &blocked {
            grid.set_blocked(gx, gy, l, false);
        }
        let mut path = found?;
        // A path that stops on a connected pad's edge cell (own copper from
        // an earlier edge ran along it) is finished into the pad centre, or
        // the track ends on the copper edge and the gate reads it as a
        // pass-through (mcu_board_30plus VDD/C8.1).
        let end0 = *path.last().unwrap();
        if let Some(pc) = connected.iter().copied().find(|c| {
            let pc = &pads[*c];
            pc.layers.contains(&end0.2) && pad_cells(grid, pc).contains(&(end0.0, end0.1))
        }) {
            let pc = &pads[pc];
            let (ccx, ccy) = grid.to_cell(pc.pt);
            if (ccx, ccy) != (end0.0, end0.1) {
                let mut x = end0.0;
                while x != ccx {
                    x += (ccx - x).signum();
                    path.push((x, end0.1, end0.2));
                }
                let mut y = end0.1;
                while y != ccy {
                    y += (ccy - y).signum();
                    path.push((ccx, y, end0.2));
                }
            }
        }
        let own_pads: Vec<&PadInfo> = pads.values().filter(|p| p.net == net).collect();
        for j in claims.footprint(&own_pads, grid, &path) {
            claims.mine[j] = claims.epoch;
        }
        let end = *path.last().unwrap();
        let end_pin = connected
            .iter()
            .copied()
            .find(|c| {
                let pc = &pads[*c];
                // Any cell the pad's copper touches, not just interior ones:
                // a path that stops on a pad's edge cell (own earlier copper
                // ran along it) must still be attached into the pad, or the
                // track ends on the copper edge and reads as a pass-through.
                pc.layers.contains(&end.2) && pad_cells(grid, pc).contains(&(end.0, end.1))
            })
            .cloned()
            .unwrap_or_default();
        for &c in &path {
            own.insert(c);
        }
        connected.push(&e.a_pin);
        out.push((path, end_pin));
    }
    Some(out)
}

/// Run the negotiation. Returns tracks/vias per net and the failures
/// (unrouted nets, or unresolved contention after `MAX_ITERS`).
pub(crate) fn run(
    grid: &mut Grid,
    pads: &HashMap<String, PadInfo>,
    rules: &RouteRules,
    edges_by_net: &HashMap<String, Vec<Edge>>,
    order: &[String],
    dbg: bool,
) -> (HashMap<String, Vec<Track>>, HashMap<String, Vec<Via>>, Vec<CheckResult>) {
    let mut claims = Claims::new(grid);
    let mut paths: HashMap<String, Vec<(Vec<(i64, i64, u8)>, String)>> = HashMap::new();
    let mut stamped: HashMap<String, (Vec<usize>, Vec<usize>)> = HashMap::new();
    let tn = grid.tuning.clone();
    let mut pres_fac = tn.nc_pres_fac_0;
    let mut unrouted: Vec<String> = Vec::new();
    let mut overused = usize::MAX;
    for iter in 0..tn.nc_max_iters {
        let t = std::time::Instant::now();
        unrouted.clear();
        for net in order {
            // Rip up: this net's claims come off before it re-routes.
            if let Some((cells, cu)) = stamped.remove(net) {
                for j in cells {
                    claims.claims[j] -= 1;
                }
                for j in cu {
                    claims.copper[j] -= 1;
                }
            }
            match route_net(net, &edges_by_net[net], pads, grid, rules, &mut claims, pres_fac) {
                Some(ps) => {
                    let own_pads: Vec<&PadInfo> = pads.values().filter(|p| &p.net == net).collect();
                    let mut cells: Vec<usize> = Vec::new();
                    let mut cu: Vec<usize> = Vec::new();
                    for (p, _) in &ps {
                        cells.extend(claims.footprint(&own_pads, grid, p));
                        cu.extend(claims.copper_cells(&own_pads, grid, p));
                    }
                    cells.sort_unstable();
                    cells.dedup();
                    cu.sort_unstable();
                    cu.dedup();
                    for &j in &cells {
                        claims.claims[j] += 1;
                    }
                    for &j in &cu {
                        claims.copper[j] += 1;
                    }
                    stamped.insert(net.clone(), (cells, cu));
                    paths.insert(net.clone(), ps);
                }
                None => {
                    paths.remove(net);
                    unrouted.push(net.clone());
                }
            }
        }
        overused = (0..claims.claims.len()).filter(|&j| claims.copper[j] > 0 && claims.claims[j] > 1).count();
        if dbg {
            eprintln!("negotiate: iter {iter} pres_fac {pres_fac:.2} overused {overused} unrouted {} in {:?}", unrouted.len(), t.elapsed());
        }
        if overused == 0 && unrouted.is_empty() {
            break;
        }
        // History must grow at the scale of the present cost, or two nets
        // with alternatives swap places forever (mcu_board_30plus seed 1:
        // GND/PA7 traded one cell for 40 iterations at a flat +2).
        let inc = (tn.nc_hist_inc as f64).max(pres_fac * 2.0).min(u16::MAX as f64 / 4.0) as u16;
        for j in 0..claims.claims.len() {
            if claims.copper[j] > 0 && claims.claims[j] > 1 {
                claims.hist[j] = claims.hist[j].saturating_add(inc.saturating_mul(claims.claims[j] - 1));
            }
        }
        pres_fac = (pres_fac * tn.nc_pres_fac_mult).min(tn.nc_pres_fac_max);
    }
    if dbg && overused > 0 {
        // Which of the contending nets has *any* legal alternative? Re-run
        // each with foreign claims priced prohibitively.
        let mut involved: Vec<String> = Vec::new();
        for j in 0..claims.claims.len() {
            if claims.copper[j] > 0 && claims.claims[j] > 1 {
                for (n, (cells, _)) in &stamped {
                    if cells.binary_search(&j).is_ok() && !involved.contains(n) {
                        involved.push(n.clone());
                    }
                }
            }
        }
        for n in &involved {
            if let Some((cells, cu)) = stamped.get(n).cloned() {
                for j in cells { claims.claims[j] -= 1; }
                for j in cu { claims.copper[j] -= 1; }
                let alt = route_net(n, &edges_by_net[n], pads, grid, rules, &mut claims, 1.0e7);
                eprintln!("negotiate: net {n} alternative avoiding all foreign claims: {}", if alt.is_some() { "EXISTS" } else { "none" });
                if let Some((cells, cu)) = stamped.get(n) {
                    for &j in cells { claims.claims[j] += 1; }
                    for &j in cu { claims.copper[j] += 1; }
                }
            }
        }
        for j in 0..claims.claims.len() {
            if claims.copper[j] > 0 && claims.claims[j] > 1 {
                let l = j % claims.num_layers;
                let c = j / claims.num_layers;
                let (cx, cy) = (c as i64 % claims.cells_x, c as i64 / claims.cells_x);
                let owners: Vec<&String> = stamped.iter().filter(|(_, (cells, _))| cells.binary_search(&j).is_ok()).map(|(n, _)| n).collect();
                let copper: Vec<&String> = stamped.iter().filter(|(_, (_, cu))| cu.binary_search(&j).is_ok()).map(|(n, _)| n).collect();
                eprintln!("negotiate: overused ({cx},{cy},{l}) {:?} copper={copper:?} claims={owners:?}", grid.to_point(cx, cy));
            }
        }
    }
    let mut fails = Vec::new();
    for n in &unrouted {
        fails.push(CheckResult::fail("route_net_unrouted", n, "negotiated router found no path for the net under the static obstacles"));
    }
    if overused > 0 {
        fails.push(CheckResult::fail("route_congestion_unresolved", "board", format!("{overused} cells still claimed by more than one net after {} iterations", tn.nc_max_iters)));
    }
    let mut tracks: HashMap<String, Vec<Track>> = HashMap::new();
    let mut vias: HashMap<String, Vec<Via>> = HashMap::new();
    for (net, ps) in &paths {
        let edges = &edges_by_net[net];
        for (i, (path, end_pin)) in ps.iter().enumerate() {
            let (t, v) = path_to_geometry(net, &edges[i].a_pin, end_pin, path, grid, rules);
            tracks.entry(net.clone()).or_default().extend(t);
            vias.entry(net.clone()).or_default().extend(v);
        }
    }
    (tracks, vias, fails)
}
