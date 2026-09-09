//! Grid A* with a via (layer-change) and bend cost.
//!
//! State/cost tables are flat `Vec`s indexed by cell rather than
//! `HashMap<State, _>` — this is the hot loop of the whole router, and
//! hashing a tuple key per visited state was the dominant cost before this
//! was array-indexed (see the 4-part fixture timing test).

use crate::grid::{Grid, Occ};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

pub(crate) const DIRS: [(i64, i64); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)]; // N, E, S, W

pub(crate) const STEP_COST: i64 = 1;
pub(crate) const BEND_COST: i64 = 2;
/// 30 cells (~7.6 mm of track on the reference 254 µm grid) per layer
/// change. At 10 a via pair cost less than a 5 mm detour and the router
/// hopped layers to dodge a single track on a three-part board; at 60 the
/// 30-part board grew 20% in track length and started crossing refdes
/// labels rather than via. 30 is the corpus sweet spot at the reference
/// grid; [`via_cost`] rescales it to cells at the design's actual grid so
/// the physical via-vs-detour trade-off the corpus was tuned against
/// doesn't quietly shift when the grid gets finer or coarser.
const VIA_COST_REF_CELLS: i64 = 30;
const REF_GRID_UM: i64 = 254;

pub(crate) fn via_cost(grid_um: i64) -> i64 {
    ((VIA_COST_REF_CELLS * REF_GRID_UM) / grid_um.max(1)).max(1)
}


/// dir 0..=3 = last move direction (index into DIRS), 4 = none (start, or
/// just came off a via).
pub(crate) const NUM_DIRS: usize = 5;
pub(crate) const NO_DIR: u8 = 4;

/// (cx, cy, layer, dir)
pub(crate) type State = (i64, i64, u8, u8);

#[derive(Eq, PartialEq)]
struct QueueItem {
    f: i64,
    g: i64,
    state: State,
}
impl Ord for QueueItem {
    fn cmp(&self, other: &Self) -> Ordering {
        other.f.cmp(&self.f) // min-heap
    }
}
impl PartialOrd for QueueItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Bound on expanded states, so a hopeless search fails instead of hanging.
/// Tuned at the reference 254 µm grid; a finer grid has quadratically more
/// cells to cover the same board, so the same absolute cap would let the
/// search declare defeat well before it has covered as much *physical*
/// area as it used to — [`max_expansions`] rescales it by the grid's cell
/// density relative to the reference, capped so a very fine grid can't
/// blow the routing time budget.
const MAX_EXPANSIONS_REF: usize = 400_000;
const MAX_EXPANSIONS_CAP: usize = 4_000_000;

pub(crate) fn max_expansions(grid_um: i64) -> usize {
    let ratio = (REF_GRID_UM as f64 / grid_um.max(1) as f64).powi(2);
    ((MAX_EXPANSIONS_REF as f64 * ratio) as usize).clamp(MAX_EXPANSIONS_REF, MAX_EXPANSIONS_CAP)
}

pub(crate) struct StateTable {
    cells_x: i64,
    num_layers: usize,
    pub(crate) best: Vec<i64>,
    pub(crate) came_from: Vec<u32>, // packed state index, or u32::MAX for "none"
}

impl StateTable {
    pub(crate) fn new(grid: &Grid) -> Self {
        let n = (grid.cells_x * grid.cells_y) as usize * grid.num_layers * NUM_DIRS;
        StateTable {
            cells_x: grid.cells_x,
            num_layers: grid.num_layers,
            best: vec![i64::MAX; n],
            came_from: vec![u32::MAX; n],
        }
    }

    #[inline]
    pub(crate) fn index(&self, s: State) -> usize {
        let (cx, cy, layer, dir) = s;
        (((cy * self.cells_x + cx) as usize * self.num_layers + layer as usize) * NUM_DIRS) + dir as usize
    }

    #[inline]
    pub(crate) fn unindex(&self, i: usize) -> State {
        let dir = (i % NUM_DIRS) as u8;
        let i = i / NUM_DIRS;
        let layer = (i % self.num_layers) as u8;
        let i = i / self.num_layers;
        let cx = (i as i64) % self.cells_x;
        let cy = (i as i64) / self.cells_x;
        (cx, cy, layer, dir)
    }

    #[inline]
    pub(crate) fn get(&self, s: State) -> i64 {
        self.best[self.index(s)]
    }

    #[inline]
    pub(crate) fn set(&mut self, s: State, g: i64, from: State) {
        let i = self.index(s);
        self.best[i] = g;
        self.came_from[i] = self.index(from) as u32;
    }
}

/// Path as a sequence of (cx, cy, layer) grid cells, start to goal
/// inclusive, or None if unreachable within the expansion budget.
pub fn route(
    grid: &Grid,
    net: &str,
    start: (i64, i64, u8),
    goal: (i64, i64, u8),
) -> Option<Vec<(i64, i64, u8)>> {
    let goals: std::collections::HashSet<(i64, i64, u8)> = std::iter::once(goal).collect();
    route_to_any(grid, net, &[start], &goals, &[(goal.0, goal.1)])
}

/// Multi-goal A*, with multiple valid starting cells (e.g. a through-hole
/// pad, whose copper — and thus a track's legal starting layer — spans
/// every layer). Every start is seeded at cost 0, so the search finds
/// whichever (start, goal) pair is cheapest instead of forcing a via just
/// because the caller had to pick one layer to name as "the" start;
/// picking only `pa.layers[0]` here used to add a spurious via on every
/// two-pin, all-through-hole net whenever the direct path favoured the
/// other layer.
///
/// The path ends at the first cell contained in `goals`. `h_targets`
/// (cell x/y) drive the heuristic — the nearest one by Manhattan
/// distance; goals that are not targets (e.g. existing tracks) may be
/// found earlier than the heuristic predicts, which only makes the
/// result slightly less optimal, never wrong.
pub fn route_to_any(
    grid: &Grid,
    net: &str,
    starts: &[(i64, i64, u8)],
    goals: &std::collections::HashSet<(i64, i64, u8)>,
    h_targets: &[(i64, i64)],
) -> Option<Vec<(i64, i64, u8)>> {
    route_to_any_ex(grid, net, starts, goals, h_targets).0
}

/// [`route_to_any`], plus the set of cells the search expanded. On
/// failure that set is the pocket the start is enclosed in, and the
/// copper bordering it is what has to be ripped up to get out.
pub fn route_to_any_ex(
    grid: &Grid,
    net: &str,
    starts: &[(i64, i64, u8)],
    goals: &std::collections::HashSet<(i64, i64, u8)>,
    h_targets: &[(i64, i64)],
) -> (Option<Vec<(i64, i64, u8)>>, Vec<(i64, i64, u8)>) {
    let mut explored: Vec<(i64, i64, u8)> = Vec::new();
    let mut table = StateTable::new(grid);
    let mut heap = BinaryHeap::new();

    let h = |cx: i64, cy: i64| h_targets.iter().map(|&(gx, gy)| (cx - gx).abs() + (cy - gy).abs()).min().unwrap_or(0);

    for &start in starts {
        let start_state: State = (start.0, start.1, start.2, NO_DIR);
        let start_idx = table.index(start_state);
        table.best[start_idx] = 0;
        heap.push(QueueItem { f: h(start.0, start.1), g: 0, state: start_state });
    }

    let mut expansions = 0usize;
    let max_expansions = max_expansions(grid.grid_um);
    let mut goal_state: Option<State> = None;

    while let Some(QueueItem { g, state, .. }) = heap.pop() {
        if g > table.get(state) {
            continue; // stale queue entry
        }
        let (cx, cy, layer, dir) = state;
        if goals.contains(&(cx, cy, layer)) {
            goal_state = Some(state);
            break;
        }
        expansions += 1;
        explored.push((cx, cy, layer));
        if expansions > max_expansions {
            return (None, explored);
        }

        // Same-layer moves.
        for (i, (dx, dy)) in DIRS.iter().enumerate() {
            let (nx, ny) = (cx + dx, cy + dy);
            if !grid.in_bounds(nx, ny) || !grid.passable(nx, ny, layer, net) {
                continue;
            }
            let turn_cost = if dir == NO_DIR || dir == i as u8 { 0 } else { BEND_COST };
            let ng = g + STEP_COST + turn_cost + grid.penalty(nx, ny, layer);
            let nstate: State = (nx, ny, layer, i as u8);
            if ng < table.get(nstate) {
                table.set(nstate, ng, state);
                heap.push(QueueItem { f: ng + h(nx, ny), g: ng, state: nstate });
            }
        }

        // Layer change (via) in place. Never inside or overlapping pad
        // copper on either layer, own net included (via-in-pad).
        for nl in 0..grid.num_layers as u8 {
            if nl == layer {
                continue;
            }
            // Through-hole via: legal only if its barrel is clear on every
            // layer, not just the two it connects.
            if (0..grid.num_layers as u8).any(|ol| !grid.passable_as(cx, cy, ol, net, Occ::Via) || grid.via_near_pad(cx, cy, ol)) {
                continue;
            }
            let ng = g + via_cost(grid.grid_um) + grid.penalty(cx, cy, layer) + grid.penalty(cx, cy, nl);
            let nstate: State = (cx, cy, nl, NO_DIR);
            if ng < table.get(nstate) {
                table.set(nstate, ng, state);
                heap.push(QueueItem { f: ng + h(cx, cy), g: ng, state: nstate });
            }
        }
    }

    if goal_state.is_none() && std::env::var_os("EDA_ROUTE_DEBUG").is_some() {
        eprintln!("astar: no path for {net} after {expansions} expansions (soft_active={})", grid.soft_active);
    }
    if std::env::var_os("EDA_ROUTE_DEBUG").is_some() {
        eprintln!("astar: {net} found={} expansions={expansions} heap_left={} soft={}/{}", goal_state.is_some(), heap.len(), grid.soft_active, grid.soft_via_active);
    }
    let Some(goal_state) = goal_state else { return (None, explored) };
    let mut path = vec![goal_state];
    let mut cur_idx = table.index(goal_state);
    loop {
        let from = table.came_from[cur_idx];
        if from == u32::MAX {
            break;
        }
        let from_state = table.unindex(from as usize);
        path.push(from_state);
        cur_idx = from as usize;
    }
    path.reverse();
    (Some(path.into_iter().map(|(x, y, l, _)| (x, y, l)).collect()), explored)
}
