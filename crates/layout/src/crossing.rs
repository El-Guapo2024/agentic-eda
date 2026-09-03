//! Phase 3: barycenter crossing minimization with up/down sweeps, keeping
//! the best ordering seen (by total crossing count over all adjacent-layer
//! pairs).

use crate::dummy::{Chain, ExtId, ExtGraph};

/// Returns the chosen ordering: for each layer, a Vec<ExtId> left-to-right.
pub fn minimize_crossings(ext: &ExtGraph, sweeps: usize) -> Vec<Vec<ExtId>> {
    let mut order: Vec<Vec<ExtId>> = ext.layers.clone();
    // Deterministic initial order: ext ids are already assigned in a
    // deterministic (insertion) order; sort each layer by ext id so the
    // starting point never depends on hash iteration.
    for layer in order.iter_mut() {
        layer.sort_unstable();
    }

    // adjacency: for each ext node, its neighbor ext ids (from chain
    // consecutive pairs only — chains only ever link adjacent layers).
    let n = ext.ext_nodes.len();
    let mut neighbors: Vec<Vec<ExtId>> = vec![Vec::new(); n];
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

    let mut pos: Vec<usize> = vec![0; n]; // position within its layer
    let sync_pos = |order: &Vec<Vec<ExtId>>, pos: &mut Vec<usize>| {
        for layer in order {
            for (i, &id) in layer.iter().enumerate() {
                pos[id] = i;
            }
        }
    };
    sync_pos(&order, &mut pos);

    let mut best_order = order.clone();
    let mut best_crossings = count_crossings(&order, &pos, &ext.chains);

    let num_layers = order.len();
    if num_layers < 2 {
        return best_order;
    }

    for sweep in 0..sweeps {
        let downward = sweep % 2 == 0;
        let layer_indices: Vec<usize> = if downward { (0..num_layers).collect() } else { (0..num_layers).rev().collect() };

        for &l in &layer_indices {
            // Skip the first layer processed in this direction: it has no
            // "already updated" neighbor layer to derive a barycenter from.
            let is_first = (downward && l == 0) || (!downward && l == num_layers - 1);
            if is_first {
                continue;
            }
            let mut with_bary: Vec<(f64, ExtId)> = order[l]
                .iter()
                .map(|&id| {
                    let nb = &neighbors[id];
                    let bary = if nb.is_empty() {
                        pos[id] as f64
                    } else {
                        nb.iter().map(|&w| pos[w] as f64).sum::<f64>() / nb.len() as f64
                    };
                    (bary, id)
                })
                .collect();
            // Stable sort by barycenter; ties keep prior relative order
            // (stable sort preserves that automatically).
            with_bary.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            order[l] = with_bary.into_iter().map(|(_, id)| id).collect();
            sync_pos(&order, &mut pos);
        }

        let crossings = count_crossings(&order, &pos, &ext.chains);
        if crossings < best_crossings {
            best_crossings = crossings;
            best_order = order.clone();
        }
    }

    best_order
}

fn count_crossings(order: &[Vec<ExtId>], pos: &[usize], chains: &[Chain]) -> usize {
    // For each adjacent layer pair, gather the (upper_pos, lower_pos) edges
    // that cross that boundary and count inversions.
    let num_layers = order.len();
    if num_layers < 2 {
        return 0;
    }
    let mut total = 0usize;
    for l in 0..num_layers - 1 {
        let mut segs: Vec<(usize, usize)> = Vec::new();
        for chain in chains {
            for w in chain.nodes.windows(2) {
                let (a, b) = (w[0], w[1]);
                // both endpoints must sit in layers l and l+1 respectively
                // (chains only ever connect consecutive layers by
                // construction, so just check membership).
                let la = layer_of(order, a);
                let lb = layer_of(order, b);
                if la == Some(l) && lb == Some(l + 1) {
                    segs.push((pos[a], pos[b]));
                } else if lb == Some(l) && la == Some(l + 1) {
                    segs.push((pos[b], pos[a]));
                }
            }
        }
        segs.sort_unstable();
        // Count inversions among the lower-layer positions via a simple
        // O(k^2) pass (k = edges crossing this boundary, small for
        // schematic-scale graphs).
        for i in 0..segs.len() {
            for j in (i + 1)..segs.len() {
                let inverted = (segs[i].0 <= segs[j].0 && segs[i].1 > segs[j].1)
                    || (segs[i].0 < segs[j].0 && segs[i].1 >= segs[j].1);
                if inverted {
                    total += 1;
                }
            }
        }
    }
    total
}

fn layer_of(order: &[Vec<ExtId>], id: ExtId) -> Option<usize> {
    order.iter().position(|layer| layer.contains(&id))
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
