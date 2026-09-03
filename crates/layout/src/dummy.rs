//! Builds the dummy-node-augmented representation used by ordering,
//! coordinate assignment and routing: every edge that spans more than one
//! layer gets a chain of zero-size "dummy" nodes, one per intermediate
//! layer, so crossing minimization and channel routing only ever have to
//! reason about adjacent-layer connections (standard Sugiyama technique).

use crate::graph::LayoutGraph;

pub type ExtId = usize;

#[derive(Debug, Clone)]
pub struct ExtNode {
    pub layer: usize,
    /// Some(node_id) for a real node's ext record; None for a dummy bend
    /// point inserted for an edge spanning more than one layer.
    pub real_node: Option<usize>,
    /// Width used for spacing/ordering; 0 for dummies.
    pub width: i64,
}

/// One edge's path through the extended graph, low-layer end first.
#[derive(Debug, Clone)]
pub struct Chain {
    pub edge_index: usize,
    pub nodes: Vec<ExtId>,
}

pub struct ExtGraph {
    pub ext_nodes: Vec<ExtNode>,
    /// Ext node ids grouped by layer (index = layer number).
    pub layers: Vec<Vec<ExtId>>,
    pub chains: Vec<Chain>,
}

/// For each inter-layer "channel" (the gap between layer `l` and layer
/// `l + 1`, indexed by `l`), the sorted (by `edge_index`) list of edges
/// whose chain has a hop crossing that channel. Two hops of the *same*
/// chain never land in the same channel (a chain only passes through each
/// layer gap once), so the length of `channel_demand()[l]` is exactly the
/// number of genuinely distinct rows that channel must provide.
///
/// Used both by `coords::assign_coords` (to size each channel's vertical
/// gap so it can fit one row per hop) and by `routing::route_edges` (to
/// assign each hop a unique row index within its channel, deterministically
/// via the edge's position in this sorted list).
pub fn channel_demand(ext: &ExtGraph) -> Vec<Vec<usize>> {
    let num_channels = ext.layers.len().saturating_sub(1);
    let mut demand: Vec<Vec<usize>> = vec![Vec::new(); num_channels];
    for chain in &ext.chains {
        // A same-real-node chain (both endpoints are pins of one box, e.g.
        // a star net's hub+leaf on the same part) has no inter-layer
        // channel hop at all — routing.rs handles it via `same_node_path`
        // instead, so skip it here too.
        if chain.nodes.len() < 2 {
            continue;
        }
        for w in chain.nodes.windows(2) {
            let a_layer = ext.ext_nodes[w[0]].layer;
            let b_layer = ext.ext_nodes[w[1]].layer;
            if b_layer != a_layer + 1 {
                // Same-node (self) chain: both ends resolve to the same
                // real node/layer. No real channel hop to size.
                continue;
            }
            demand[a_layer].push(chain.edge_index);
        }
    }
    for d in &mut demand {
        d.sort_unstable();
        d.dedup();
    }
    demand
}

pub fn build(g: &LayoutGraph, node_layer: &[usize]) -> ExtGraph {
    let num_layers = node_layer.iter().copied().max().map(|m| m + 1).unwrap_or(0);
    let mut ext_nodes: Vec<ExtNode> = Vec::new();
    let mut layers: Vec<Vec<ExtId>> = vec![Vec::new(); num_layers];

    // One ext node per real node, placed at its layer.
    let mut real_ext: Vec<ExtId> = vec![0; g.node_count()];
    for (id, node) in g.nodes.iter().enumerate() {
        let ext_id = ext_nodes.len();
        ext_nodes.push(ExtNode { layer: node_layer[id], real_node: Some(id), width: node.width });
        real_ext[id] = ext_id;
        layers[node_layer[id]].push(ext_id);
    }

    let mut chains: Vec<Chain> = Vec::new();

    for (edge_index, e) in g.edges.iter().enumerate() {
        let lu = node_layer[e.from.node];
        let lv = node_layer[e.to.node];
        let (lo_node, lo_port, lo_layer, hi_node, hi_port, hi_layer) = if lu <= lv {
            (e.from.node, e.from.port, lu, e.to.node, e.to.port, lv)
        } else {
            (e.to.node, e.to.port, lv, e.from.node, e.from.port, lu)
        };

        let lo_ext = real_ext[lo_node];
        let hi_ext = real_ext[hi_node];
        let mut path = vec![lo_ext];

        if hi_layer > lo_layer + 1 {
            #[allow(clippy::needless_range_loop)] // indexes two parallel structures (ext_nodes growth + layers) at once
            for l in (lo_layer + 1)..hi_layer {
                let ext_id = ext_nodes.len();
                ext_nodes.push(ExtNode { layer: l, real_node: None, width: 0 });
                layers[l].push(ext_id);
                path.push(ext_id);
            }
        }
        path.push(hi_ext);

        // Record real port ownership as metadata alongside the chain so
        // routing can find the exact port point rather than an ext-node
        // centroid for the two real endpoints.
        chains.push(Chain { edge_index, nodes: path });
        let _ = (lo_port, hi_port); // ports resolved from original edge in routing.rs
    }

    ExtGraph { ext_nodes, layers, chains }
}
