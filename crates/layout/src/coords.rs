//! Phase 4: priority/median-based x coordinate assignment within each
//! layer, uniform layer spacing, everything snapped to `opts.grid`.

use crate::dummy::{Chain, ExtGraph};
use crate::graph::{LayoutGraph, STUB_LEN};
use crate::LayoutOptions;
use std::collections::BTreeMap;

/// Center-x per ext node, and top-y per layer (both grid-snapped).
pub struct Coords {
    pub center_x: Vec<i64>,
    pub layer_y: Vec<i64>,
}

fn snap(v: i64, grid: i64) -> i64 {
    if grid <= 0 {
        return v;
    }
    let half = grid / 2;
    let q = if v >= 0 { (v + half).div_euclid(grid) } else { -((-v + half).div_euclid(grid)) };
    q * grid
}

fn snap_up(v: i64, grid: i64) -> i64 {
    if grid <= 0 || v <= 0 {
        return v.max(0);
    }
    ((v + grid - 1) / grid) * grid
}

pub fn assign_coords(g: &LayoutGraph, ext: &ExtGraph, order: &[Vec<usize>], opts: &LayoutOptions, channel_demand: &[Vec<usize>]) -> Coords {
    let n = ext.ext_nodes.len();
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); n];
    for Chain { nodes, .. } in &ext.chains {
        for w in nodes.windows(2) {
            neighbors[w[0]].push(w[1]);
            neighbors[w[1]].push(w[0]);
        }
    }
    for nb in neighbors.iter_mut() {
        nb.sort_unstable();
        nb.dedup();
    }

    let min_gap = opts.node_spacing.max(1);

    // Initial placement: left-to-right packing within each layer.
    let mut center_x: Vec<i64> = vec![0; n];
    for layer in order {
        let mut cursor: i64 = 0;
        for (i, &id) in layer.iter().enumerate() {
            let w = ext.ext_nodes[id].width;
            if i == 0 {
                cursor = w / 2;
            } else {
                cursor += min_gap + w / 2 + ext.ext_nodes[layer[i - 1]].width / 2;
            }
            center_x[id] = cursor;
        }
    }

    // Median alignment sweeps, alternating direction, each followed by an
    // order-preserving overlap resolution pass.
    let sweeps = 4;
    for sweep in 0..sweeps {
        let downward = sweep % 2 == 0;
        let indices: Vec<usize> = if downward { (0..order.len()).collect() } else { (0..order.len()).rev().collect() };
        for &l in &indices {
            let is_first = (downward && l == 0) || (!downward && l == order.len().saturating_sub(1));
            if is_first || order.is_empty() {
                continue;
            }
            let layer = &order[l];
            let mut desired: Vec<i64> = layer
                .iter()
                .map(|&id| {
                    let nb = &neighbors[id];
                    if nb.is_empty() {
                        center_x[id]
                    } else {
                        median(nb.iter().map(|&w| center_x[w]).collect())
                    }
                })
                .collect();
            resolve_overlaps(layer, &mut desired, ext, min_gap);
            for (i, &id) in layer.iter().enumerate() {
                center_x[id] = desired[i];
            }
        }
    }

    // Snap to grid, then re-resolve overlaps in grid units so rounding
    // cannot reintroduce an overlap.
    for layer in order {
        let mut snapped: Vec<i64> = layer.iter().map(|&id| snap(center_x[id], opts.grid)).collect();
        resolve_overlaps_grid(layer, &mut snapped, ext, min_gap, opts.grid);
        for (i, &id) in layer.iter().enumerate() {
            center_x[id] = snapped[i];
        }
    }

    // Per-gap layer spacing: at least as large as the tallest node overall
    // plus node_spacing (so vertically adjacent layers never overlap
    // regardless of caller-chosen node heights), and grown further, gap by
    // gap, when that gap's channel needs more distinct horizontal track
    // rows than the base spacing has room for (one row per genuinely
    // distinct edge hop routed through that channel — see
    // `dummy::channel_demand`).
    //
    // `routing::route_edges` carves the actual channel out of the gap as
    // `[layer_y[l] + tallest + STUB_LEN, layer_y[l+1] - STUB_LEN]` (the
    // `tallest` term clears every box in the upper layer, not just this
    // gap's own nodes, since layer spacing is otherwise uniform), so the
    // *available* track height is `gap - tallest - 2*STUB_LEN`; sizing the
    // gap to fit `rows` grid-spaced rows there means requiring
    // `gap >= tallest + 2*STUB_LEN + rows*grid`.
    let tallest = g.nodes.iter().map(|n| n.height).max().unwrap_or(0);
    let base_spacing = snap_up((opts.layer_spacing).max(tallest + min_gap), opts.grid).max(opts.grid);
    let mut layer_y: Vec<i64> = vec![0; order.len()];
    for l in 1..order.len() {
        let rows = channel_demand.get(l - 1).map(|d| d.len()).unwrap_or(0);
        let demand_spacing = snap_up(tallest + 2 * STUB_LEN + (rows as i64) * opts.grid, opts.grid);
        let gap = base_spacing.max(demand_spacing).max(opts.grid);
        layer_y[l] = snap(layer_y[l - 1] + gap, opts.grid);
    }

    Coords { center_x, layer_y }
}

fn median(mut v: Vec<i64>) -> i64 {
    v.sort_unstable();
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2
    }
}

fn resolve_overlaps(layer: &[usize], xs: &mut [i64], ext: &ExtGraph, min_gap: i64) {
    // Left-to-right sweep enforcing minimum center-to-center distance given
    // each node's half-width, without reordering.
    for i in 1..layer.len() {
        let prev_id = layer[i - 1];
        let id = layer[i];
        let required = ext.ext_nodes[prev_id].width / 2 + min_gap + ext.ext_nodes[id].width / 2;
        if xs[i] < xs[i - 1] + required {
            xs[i] = xs[i - 1] + required;
        }
    }
}

fn resolve_overlaps_grid(layer: &[usize], xs: &mut [i64], ext: &ExtGraph, min_gap: i64, grid: i64) {
    for i in 1..layer.len() {
        let prev_id = layer[i - 1];
        let id = layer[i];
        let required = snap_up(ext.ext_nodes[prev_id].width / 2 + min_gap + ext.ext_nodes[id].width / 2, grid);
        if xs[i] < xs[i - 1] + required {
            xs[i] = xs[i - 1] + required;
        }
    }
}

/// Convenience used by lib.rs: map real node id -> its ext id, for reading
/// final coordinates back out.
pub fn real_ext_lookup(ext: &ExtGraph) -> BTreeMap<usize, usize> {
    let mut m = BTreeMap::new();
    for (ext_id, en) in ext.ext_nodes.iter().enumerate() {
        if let Some(real) = en.real_node {
            m.insert(real, ext_id);
        }
    }
    m
}
