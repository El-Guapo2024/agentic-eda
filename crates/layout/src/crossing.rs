//! Phase 3: barycenter crossing minimization with up/down sweeps, keeping
//! the best ordering seen (by total crossing count over all adjacent-layer
//! pairs).

use crate::dummy::{Chain, ExtId, ExtGraph};

/// Returns the chosen ordering: for each layer, a Vec<ExtId> left-to-right.
///
/// ELK-style layer sweep: alternating down/up barycenter sweeps where each
/// layer is ordered by the positions of its neighbours in the *fixed*
/// adjacent layer only, followed by a transposition pass (adjacent swaps
/// that reduce crossings), repeated until no improvement; several
/// deterministic random restarts, best total crossing count wins.
/// `sweeps` bounds the sweeps per restart.
pub fn minimize_crossings(ext: &ExtGraph, sweeps: usize) -> Vec<Vec<ExtId>> {
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
        if cur < best_crossings {
            best_crossings = cur;
            best_order = Some(order);
        }
        if best_crossings == 0 {
            break;
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
        let order = minimize_crossings(&ext, 4);
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
