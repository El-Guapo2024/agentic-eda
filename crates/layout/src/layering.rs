//! Phase 2: longest-path layering + layer-shift compaction.

use crate::graph::LayoutGraph;

/// Returns layer index per node (0-based, layer grows in the edge direction
/// after cycle-breaking is applied via `reversed`).
pub fn assign_layers(g: &LayoutGraph, reversed: &[bool]) -> Vec<usize> {
    let n = g.node_count();
    if n == 0 {
        return Vec::new();
    }

    // DAG adjacency respecting `reversed` (self-loops dropped).
    let mut out_adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut in_adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, e) in g.edges.iter().enumerate() {
        if e.from.node == e.to.node {
            continue;
        }
        let (from, to) = if reversed[i] { (e.to.node, e.from.node) } else { (e.from.node, e.to.node) };
        out_adj[from].push(to);
        in_adj[to].push(from);
    }
    for v in out_adj.iter_mut() {
        v.sort_unstable();
        v.dedup();
    }
    for v in in_adj.iter_mut() {
        v.sort_unstable();
        v.dedup();
    }

    // Kahn topological sort, deterministic (BTreeSet-free, but we always pick
    // the smallest-id ready node).
    let mut indeg: Vec<usize> = in_adj.iter().map(|v| v.len()).collect();
    let mut ready: std::collections::BTreeSet<usize> =
        (0..n).filter(|&v| indeg[v] == 0).collect();
    let mut topo: Vec<usize> = Vec::with_capacity(n);
    let mut indeg_work = indeg.clone();
    while let Some(&v) = ready.iter().next() {
        ready.remove(&v);
        topo.push(v);
        for &w in &out_adj[v] {
            indeg_work[w] -= 1;
            if indeg_work[w] == 0 {
                ready.insert(w);
            }
        }
    }
    // If topo didn't cover all nodes (shouldn't happen post cycle-breaking on
    // a correctly reversed edge set, but guard anyway), append remaining in
    // id order to stay total and deterministic.
    if topo.len() < n {
        let seen: std::collections::BTreeSet<usize> = topo.iter().copied().collect();
        for v in 0..n {
            if !seen.contains(&v) {
                topo.push(v);
            }
        }
    }
    let _ = &mut indeg; // degrees no longer needed beyond above

    // Longest path from sources.
    let mut layer = vec![0usize; n];
    for &v in &topo {
        for &u in &in_adj[v] {
            layer[v] = layer[v].max(layer[u] + 1);
        }
    }

    // Layer-shift compaction: process in reverse topo order, pull each node
    // up as close as possible to its successors without violating the
    // predecessor-derived lower bound.
    for &v in topo.iter().rev() {
        if out_adj[v].is_empty() {
            continue;
        }
        let min_succ = out_adj[v].iter().map(|&w| layer[w]).min().unwrap();
        if min_succ == 0 {
            continue;
        }
        let lower_bound = in_adj[v].iter().map(|&u| layer[u] + 1).max().unwrap_or(0);
        let target = min_succ.saturating_sub(1).max(lower_bound);
        layer[v] = target;
    }

    layer
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{EdgeEndpoint, Node};

    #[test]
    fn diamond_layers_monotone() {
        let mut g = LayoutGraph::new();
        for i in 0..4 {
            g.add_node(Node::with_default_ports(i, 1000, 1000));
        }
        g.add_edge(EdgeEndpoint { node: 0, port: 1 }, EdgeEndpoint { node: 1, port: 0 });
        g.add_edge(EdgeEndpoint { node: 0, port: 1 }, EdgeEndpoint { node: 2, port: 0 });
        g.add_edge(EdgeEndpoint { node: 1, port: 1 }, EdgeEndpoint { node: 3, port: 0 });
        g.add_edge(EdgeEndpoint { node: 2, port: 1 }, EdgeEndpoint { node: 3, port: 0 });
        let reversed = vec![false; 4];
        let layers = assign_layers(&g, &reversed);
        assert_eq!(layers[0], 0);
        assert_eq!(layers[3], 2);
        assert!(layers[1] < layers[3]);
        assert!(layers[2] < layers[3]);
    }
}
