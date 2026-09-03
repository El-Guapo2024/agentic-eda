//! Grid A* with a via (layer-change) and bend cost.
//!
//! State/cost tables are flat `Vec`s indexed by cell rather than
//! `HashMap<State, _>` — this is the hot loop of the whole router, and
//! hashing a tuple key per visited state was the dominant cost before this
//! was array-indexed (see the 4-part fixture timing test).

use crate::grid::{Grid, Occ};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

const DIRS: [(i64, i64); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)]; // N, E, S, W

const STEP_COST: i64 = 1;
const BEND_COST: i64 = 2;
const VIA_COST: i64 = 10;

/// dir 0..=3 = last move direction (index into DIRS), 4 = none (start, or
/// just came off a via).
const NUM_DIRS: usize = 5;
const NO_DIR: u8 = 4;

/// (cx, cy, layer, dir)
type State = (i64, i64, u8, u8);

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
const MAX_EXPANSIONS: usize = 400_000;

struct StateTable {
    cells_x: i64,
    num_layers: usize,
    best: Vec<i64>,
    came_from: Vec<u32>, // packed state index, or u32::MAX for "none"
}

impl StateTable {
    fn new(grid: &Grid) -> Self {
        let n = (grid.cells_x * grid.cells_y) as usize * grid.num_layers * NUM_DIRS;
        StateTable {
            cells_x: grid.cells_x,
            num_layers: grid.num_layers,
            best: vec![i64::MAX; n],
            came_from: vec![u32::MAX; n],
        }
    }

    #[inline]
    fn index(&self, s: State) -> usize {
        let (cx, cy, layer, dir) = s;
        (((cy * self.cells_x + cx) as usize * self.num_layers + layer as usize) * NUM_DIRS) + dir as usize
    }

    #[inline]
    fn unindex(&self, i: usize) -> State {
        let dir = (i % NUM_DIRS) as u8;
        let i = i / NUM_DIRS;
        let layer = (i % self.num_layers) as u8;
        let i = i / self.num_layers;
        let cx = (i as i64) % self.cells_x;
        let cy = (i as i64) / self.cells_x;
        (cx, cy, layer, dir)
    }

    #[inline]
    fn get(&self, s: State) -> i64 {
        self.best[self.index(s)]
    }

    #[inline]
    fn set(&mut self, s: State, g: i64, from: State) {
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
    let start_state: State = (start.0, start.1, start.2, NO_DIR);
    let mut table = StateTable::new(grid);
    let mut heap = BinaryHeap::new();

    let h = |cx: i64, cy: i64| (cx - goal.0).abs() + (cy - goal.1).abs();

    let start_idx = table.index(start_state);
    table.best[start_idx] = 0;
    heap.push(QueueItem { f: h(start.0, start.1), g: 0, state: start_state });

    let mut expansions = 0usize;
    let mut goal_state: Option<State> = None;

    while let Some(QueueItem { g, state, .. }) = heap.pop() {
        if g > table.get(state) {
            continue; // stale queue entry
        }
        let (cx, cy, layer, dir) = state;
        if cx == goal.0 && cy == goal.1 && layer == goal.2 {
            goal_state = Some(state);
            break;
        }
        expansions += 1;
        if expansions > MAX_EXPANSIONS {
            return None;
        }

        // Same-layer moves.
        for (i, (dx, dy)) in DIRS.iter().enumerate() {
            let (nx, ny) = (cx + dx, cy + dy);
            if !grid.in_bounds(nx, ny) || !grid.passable(nx, ny, layer, net) {
                continue;
            }
            let turn_cost = if dir == NO_DIR || dir == i as u8 { 0 } else { BEND_COST };
            let ng = g + STEP_COST + turn_cost;
            let nstate: State = (nx, ny, layer, i as u8);
            if ng < table.get(nstate) {
                table.set(nstate, ng, state);
                heap.push(QueueItem { f: ng + h(nx, ny), g: ng, state: nstate });
            }
        }

        // Layer change (via) in place.
        for nl in 0..grid.num_layers as u8 {
            if nl == layer {
                continue;
            }
            if !grid.passable_as(cx, cy, nl, net, Occ::Via) || !grid.passable_as(cx, cy, layer, net, Occ::Via) {
                continue;
            }
            let ng = g + VIA_COST;
            let nstate: State = (cx, cy, nl, NO_DIR);
            if ng < table.get(nstate) {
                table.set(nstate, ng, state);
                heap.push(QueueItem { f: ng + h(cx, cy), g: ng, state: nstate });
            }
        }
    }

    let goal_state = goal_state?;
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
    Some(path.into_iter().map(|(x, y, l, _)| (x, y, l)).collect())
}
