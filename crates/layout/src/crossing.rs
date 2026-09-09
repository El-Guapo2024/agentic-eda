//! Phase 3: barycenter crossing minimization with up/down sweeps, keeping
//! the best ordering seen (by total crossing count over all adjacent-layer
//! pairs).

use crate::dummy::{self, Chain, ExtId, ExtGraph};
use crate::coords;
use crate::graph::LayoutGraph;
use crate::LayoutOptions;

/// Returns the chosen ordering: for each layer, a Vec<ExtId> left-to-right.
///
/// ELK-style layer sweep: alternating down/up barycenter sweeps where each
/// layer is ordered by the positions of its neighbours in the *fixed*
/// adjacent layer only, followed by a transposition pass (adjacent swaps
/// that reduce crossings), repeated until no improvement; several
/// deterministic random restarts, best total crossing count wins, with a
/// compactness tie-break — see the comment at the restart loop below.
/// `sweeps` bounds the sweeps per restart. `g` and `opts` are the same
/// (already axis-transposed) graph and options `lib.rs` passes to
/// `coords::assign_coords` right after this call; they are used here only
/// to trial-run coordinate assignment per restart for the tie-break, never
/// to change what gets returned besides the chosen ordering.
pub fn minimize_crossings(ext: &ExtGraph, sweeps: usize, g: &LayoutGraph, opts: &LayoutOptions) -> Vec<Vec<ExtId>> {
    let n = ext.ext_nodes.len();
    let num_layers = ext.layers.len();
    let mut base: Vec<Vec<ExtId>> = ext.layers.clone();
    for layer in base.iter_mut() {
        layer.sort_unstable();
    }
    if num_layers < 2 {
        return base;
    }

    let mut layer_of: Vec<usize> = vec![0; n];
    for (l, layer) in base.iter().enumerate() {
        for &id in layer {
            layer_of[id] = l;
        }
    }
    // up[id]: neighbours in layer-1, down[id]: neighbours in layer+1.
    let mut up: Vec<Vec<ExtId>> = vec![Vec::new(); n];
    let mut down: Vec<Vec<ExtId>> = vec![Vec::new(); n];
    for Chain { nodes, .. } in &ext.chains {
        for w in nodes.windows(2) {
            if layer_of[w[0]] == layer_of[w[1]] {
                // Same-node / same-layer edge: no boundary to cross.
                continue;
            }
            let (a, b) = if layer_of[w[0]] < layer_of[w[1]] { (w[0], w[1]) } else { (w[1], w[0]) };
            down[a].push(b);
            up[b].push(a);
        }
    }
    for v in up.iter_mut().chain(down.iter_mut()) {
        v.sort_unstable();
        v.dedup();
    }
    // Per-boundary edge list (upper ext id, lower ext id).
    let mut boundary_edges: Vec<Vec<(ExtId, ExtId)>> = vec![Vec::new(); num_layers - 1];
    for (a, ds) in down.iter().enumerate() {
        for &b in ds {
            boundary_edges[layer_of[a]].push((a, b));
        }
    }

    let sweeps = sweeps.max(1);
    let restarts = 4usize;
    let mut best_order: Option<Vec<Vec<ExtId>>> = None;
    let mut best_crossings = usize::MAX;
    // Restarts used to be picked by crossing count alone, which can land on
    // an ordering that is crossing-optimal yet spreads a tightly-connected
    // cluster's members far apart within their layers (long inter-layer
    // hops for edges that, in a more compact ordering, would stay just as
    // short with the same crossing count). `coords::assign_coords`'s median
    // alignment inherits whatever spread the ordering hands it, so a wide
    // ordering here becomes a wide drawn block downstream — which is
    // exactly the defect `schematic_cluster_split` catches.
    //
    // Fix: break ties among restarts with a compactness term. Two proxies
    // measured purely from ordering slots were tried and both misfired on
    // `ldo_proximity_heavy` seed 3 (which has zero crossings under several
    // restarts): a raw sum of every boundary edge's slot distance let a
    // restart "win" by herding dummy nodes together at one side of their
    // layer while shoving the real nodes they route between into a cramped
    // tail (0 recorded ordering crossings, but 6 actual wire crossings and
    // a `schematic_cluster_split` failure once coordinates were assigned);
    // a real-endpoints-only normalized-slot variant still picked the wrong
    // restart because slot position doesn't account for how the downstream
    // median-alignment coordinate pass actually spaces things (zero-width
    // dummy runs don't cost slots the way real boxes do). So the tie-break
    // trial-runs `coords::assign_coords` for each restart's ordering and
    // measures the actual resulting real-node x-extent (max center_x - min
    // center_x) — the exact quantity `schematic_cluster_split` complains
    // about — and picks the smallest. This is a *lexicographic* tie-break,
    // never a traded-off scalar sum: crossing count is what the router and
    // the `schematic_wire_crossing_count` gate actually pay for, so a
    // restart may never win by adding crossings in exchange for a smaller
    // extent. Only among restarts that tie on the minimum crossing count
    // seen so far does extent decide the winner.
    let mut best_extent = i64::MAX;

    for restart in 0..restarts {
        let mut order = base.clone();
        if restart > 0 {
            let mut state = 0x9E37_79B9_7F4A_7C15u64.wrapping_mul(restart as u64 + 1);
            for layer in order.iter_mut() {
                for k in (1..layer.len()).rev() {
                    state = splitmix(state);
                    layer.swap(k, (state as usize) % (k + 1));
                }
            }
        }
        let mut pos: Vec<usize> = vec![0; n];
        sync_pos(&order, &mut pos);
        let mut cur = total_crossings(&boundary_edges, &pos);
        let mut stale = 0;
        for sweep in 0..sweeps.max(8) * 2 {
            let downward = sweep % 2 == 0;
            if downward {
                for l in 1..num_layers {
                    barycenter_layer(&mut order[l], &up, &pos);
                    sync_layer(&order[l], &mut pos);
                }
            } else {
                for l in (0..num_layers - 1).rev() {
                    barycenter_layer(&mut order[l], &down, &pos);
                    sync_layer(&order[l], &mut pos);
                }
            }
            transpose(&mut order, &mut pos, &up, &down);
            let c = total_crossings(&boundary_edges, &pos);
            if c < cur {
                cur = c;
                stale = 0;
            } else {
                stale += 1;
                if stale >= 2 {
                    break;
                }
            }
        }
        let extent = real_node_extent(ext, &order, g, opts);
        if std::env::var_os("EDA_LAYOUT_DEBUG").is_some() {
            eprintln!("restart {restart}: crossings={cur} extent={extent}");
        }
        if cur < best_crossings || (cur == best_crossings && extent < best_extent) {
            best_crossings = cur;
            best_extent = extent;
            best_order = Some(order);
        }
    }
    best_order.unwrap_or(base)
}

fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

fn sync_pos(order: &[Vec<ExtId>], pos: &mut [usize]) {
    for layer in order {
        sync_layer(layer, pos);
    }
}

fn sync_layer(layer: &[ExtId], pos: &mut [usize]) {
    for (i, &id) in layer.iter().enumerate() {
        pos[id] = i;
    }
}

/// Reorder `layer` by the mean position of each node's neighbours in the
/// fixed reference layer; nodes without such neighbours keep their slot
/// (stable sort on current position as the key).
fn barycenter_layer(layer: &mut Vec<ExtId>, refs: &[Vec<ExtId>], pos: &[usize]) {
    let mut keyed: Vec<(f64, usize, ExtId)> = layer
        .iter()
        .map(|&id| {
            let nb = &refs[id];
            let key = if nb.is_empty() { pos[id] as f64 } else { nb.iter().map(|&w| pos[w] as f64).sum::<f64>() / nb.len() as f64 };
            (key, pos[id], id)
        })
        .collect();
    keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
    *layer = keyed.into_iter().map(|(_, _, id)| id).collect();
}

/// Crossings across one boundary given positions.
fn boundary_crossings(edges: &[(ExtId, ExtId)], pos: &[usize]) -> usize {
    let mut segs: Vec<(usize, usize)> = edges.iter().map(|&(a, b)| (pos[a], pos[b])).collect();
    segs.sort_unstable();
    // Inversions in the lower sequence: O(k log k) via merge count is
    // overkill for schematic scale; k^2 with early exit is fine.
    let mut total = 0;
    for i in 0..segs.len() {
        let (ua, la) = segs[i];
        for &(ub, lb) in &segs[i + 1..] {
            if (ua < ub && la > lb) || (ua == ub && la > lb) {
                total += 1;
            }
        }
    }
    total
}

fn total_crossings(boundary_edges: &[Vec<(ExtId, ExtId)>], pos: &[usize]) -> usize {
    boundary_edges.iter().map(|e| boundary_crossings(e, pos)).sum()
}

/// Compactness measure for one candidate ordering: actually runs
/// `coords::assign_coords` for it (the same function `lib.rs` will call for
/// real, right after `minimize_crossings` returns) and reports the drawn
/// horizontal extent of the real (non-dummy) nodes — max center_x minus min
/// center_x. This is the exact axis `schematic_cluster_split` measures a
/// cluster's spread on (post axis-transpose, `lib.rs`'s screen y), so
/// picking the restart that minimizes it directly targets the gate instead
/// of an ordering-level proxy.
fn real_node_extent(ext: &ExtGraph, order: &[Vec<ExtId>], g: &LayoutGraph, opts: &LayoutOptions) -> i64 {
    let channel_demand = dummy::channel_demand(ext, g);
    let coords = coords::assign_coords(g, ext, order, opts, &channel_demand);
    let mut lo = i64::MAX;
    let mut hi = i64::MIN;
    for (id, en) in ext.ext_nodes.iter().enumerate() {
        if en.real_node.is_some() {
            lo = lo.min(coords.center_x[id]);
            hi = hi.max(coords.center_x[id]);
        }
    }
    if hi < lo {
        0
    } else {
        hi - lo
    }
}

/// Adjacent-swap local search: for every layer, swap neighbouring nodes
/// while doing so lowers the crossings on the boundaries above and below.
/// Only edges incident to the swapped pair can change, so the delta is
/// computed from those alone (O(deg_a * deg_b) per candidate swap).
fn transpose(order: &mut [Vec<ExtId>], pos: &mut [usize], up: &[Vec<ExtId>], down: &[Vec<ExtId>]) {
    let num_layers = order.len();
    let mut improved = true;
    let mut rounds = 0;
    while improved && rounds < 20 {
        improved = false;
        rounds += 1;
        for l in 0..num_layers {
            for i in 0..order[l].len().saturating_sub(1) {
                let (a, b) = (order[l][i], order[l][i + 1]);
                // a is left of b now. Crossings between a's and b's edges to
                // a fixed layer: pair (u of a, v of b) crosses iff pos[u] >
                // pos[v]; after swapping, iff pos[u] < pos[v].
                let mut delta: i64 = 0;
                for refs in [up, down] {
                    for &u in &refs[a] {
                        for &v in &refs[b] {
                            if pos[u] > pos[v] {
                                delta -= 1;
                            } else if pos[u] < pos[v] {
                                delta += 1;
                            }
                        }
                    }
                }
                if delta < 0 {
                    order[l].swap(i, i + 1);
                    pos[a] = i + 1;
                    pos[b] = i;
                    improved = true;
                }
            }
        }
    }
}

/// Total crossings of an ordering (used by tests and callers).
pub fn count_crossings(order: &[Vec<ExtId>], pos: &[usize], chains: &[Chain]) -> usize {
    let num_layers = order.len();
    if num_layers < 2 {
        return 0;
    }
    let mut layer_of = std::collections::HashMap::new();
    for (l, layer) in order.iter().enumerate() {
        for &id in layer {
            layer_of.insert(id, l);
        }
    }
    let mut boundary_edges: Vec<Vec<(ExtId, ExtId)>> = vec![Vec::new(); num_layers - 1];
    for chain in chains {
        for w in chain.nodes.windows(2) {
            if layer_of[&w[0]] == layer_of[&w[1]] {
                continue;
            }
            let (a, b) = if layer_of[&w[0]] < layer_of[&w[1]] { (w[0], w[1]) } else { (w[1], w[0]) };
            boundary_edges[layer_of[&a]].push((a, b));
        }
    }
    total_crossings(&boundary_edges, pos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dummy::build;
    use crate::graph::{EdgeEndpoint, LayoutGraph, Node};
    use crate::layering::assign_layers;
    use crate::cycle::break_cycles;

    #[test]
    fn fanout_has_zero_crossings() {
        // one driver, 8 fanned-out loads: no edges cross by construction.
        let mut g = LayoutGraph::new();
        g.add_node(Node::with_default_ports(0, 1000, 1000));
        for i in 1..9 {
            g.add_node(Node::with_default_ports(i, 1000, 1000));
            g.add_edge(EdgeEndpoint { node: 0, port: 1 }, EdgeEndpoint { node: i, port: 0 });
        }
        let reversed = break_cycles(&g);
        let layers = assign_layers(&g, &reversed);
        let ext = build(&g, &layers);
        let order = minimize_crossings(&ext, 4, &g, &crate::LayoutOptions::default());
        let pos = {
            let mut pos = vec![0usize; ext.ext_nodes.len()];
            for layer in &order {
                for (i, &id) in layer.iter().enumerate() {
                    pos[id] = i;
                }
            }
            pos
        };
        assert_eq!(count_crossings(&order, &pos, &ext.chains), 0);
    }
}
