//! Phase 1: cycle breaking via the greedy Eades-Lin-Smyth heuristic.
//!
//! Produces, for each edge index, whether it should be treated as reversed
//! for the purposes of layering (the original graph / edge list is never
//! mutated — routing still draws the edge in its original direction).

use crate::graph::LayoutGraph;
use std::collections::BTreeSet;

/// Returns a `Vec<bool>` parallel to `g.edges`: true if edge i should be
/// treated as pointing the other way for DAG purposes.
pub fn break_cycles(g: &LayoutGraph) -> Vec<bool> {
    let n = g.node_count();
    if n == 0 {
        return Vec::new();
    }

    // adjacency as sorted edge-index sets, for determinism.
    let mut out_edges: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
    let mut in_edges: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
    for (i, e) in g.edges.iter().enumerate() {
        if e.from.node == e.to.node {
            continue; // self loops: ignore for ordering, handled as forward
        }
        out_edges[e.from.node].insert(i);
        in_edges[e.to.node].insert(i);
    }

    let mut left: Vec<usize> = Vec::new();
    let mut right: Vec<usize> = Vec::new();

    let mut remaining: BTreeSet<usize> = (0..n).collect();

    // Recompute degrees restricted to `remaining` nodes each pass; n is small
    // (schematic-scale graphs), so O(n^2) here is fine and keeps determinism
    // trivial to reason about.
    let deg = |node: usize, remaining: &BTreeSet<usize>, out_edges: &[BTreeSet<usize>], in_edges: &[BTreeSet<usize>]| -> (usize, usize) {
        let od = out_edges[node]
            .iter()
            .filter(|&&ei| remaining.contains(&g.edges[ei].to.node))
            .count();
        let id = in_edges[node]
            .iter()
            .filter(|&&ei| remaining.contains(&g.edges[ei].from.node))
            .count();
        (od, id)
    };

    while !remaining.is_empty() {
        // Remove sinks (outdeg 0) repeatedly.
        loop {
            let sink = remaining.iter().copied().find(|&v| deg(v, &remaining, &out_edges, &in_edges).0 == 0);
            match sink {
                Some(v) => {
                    // Sinks accumulate onto the *front* of the right
                    // sequence: a sink uncovered later (by cascading
                    // removal) sits logically closer to the already-placed
                    // sinks than the first one found.
                    right.insert(0, v);
                    remaining.remove(&v);
                }
                None => break,
            }
        }
        // Remove sources (indeg 0) repeatedly.
        loop {
            let source = remaining.iter().copied().find(|&v| deg(v, &remaining, &out_edges, &in_edges).1 == 0);
            match source {
                Some(v) => {
                    left.push(v);
                    remaining.remove(&v);
                }
                None => break,
            }
        }
        if remaining.is_empty() {
            break;
        }
        // Pick node maximizing outdeg - indeg (ties broken by smallest id for determinism).
        let mut best: Option<(i64, usize)> = None;
        for &v in remaining.iter() {
            let (od, id) = deg(v, &remaining, &out_edges, &in_edges);
            let score = od as i64 - id as i64;
            match best {
                None => best = Some((score, v)),
                Some((bs, _)) if score > bs => best = Some((score, v)),
                _ => {}
            }
        }
        if let Some((_, v)) = best {
            left.push(v);
            remaining.remove(&v);
        }
    }

    left.extend(right);
    let order = left; // sequence s1..sn

    let mut pos = vec![0usize; n];
    for (i, &v) in order.iter().enumerate() {
        pos[v] = i;
    }

    g.edges
        .iter()
        .map(|e| {
            if e.from.node == e.to.node {
                false
            } else {
                pos[e.from.node] > pos[e.to.node]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{EdgeEndpoint, Node};

    #[test]
    fn breaks_simple_cycle() {
        let mut g = LayoutGraph::new();
        for i in 0..3 {
            g.add_node(Node::with_default_ports(i, 1000, 1000));
        }
        g.add_edge(EdgeEndpoint { node: 0, port: 1 }, EdgeEndpoint { node: 1, port: 0 });
        g.add_edge(EdgeEndpoint { node: 1, port: 1 }, EdgeEndpoint { node: 2, port: 0 });
        g.add_edge(EdgeEndpoint { node: 2, port: 1 }, EdgeEndpoint { node: 0, port: 0 });
        let reversed = break_cycles(&g);
        assert_eq!(reversed.iter().filter(|&&r| r).count(), 1);
    }
}
