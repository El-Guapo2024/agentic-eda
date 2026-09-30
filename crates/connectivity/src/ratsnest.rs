//! `RN_NET` (`pcbnew/ratsnest/ratsnest_data.{h,cpp}`): for each net,
//! Delaunay-triangulate every cluster's anchors, then run Kruskal's MST
//! over (the triangulation's edges + one free "already connected" edge
//! per cluster) so the resulting positive-weight edges are exactly the
//! copper this net is still missing -- the ratsnest.
//!
//! `RN_NET::AddCluster` -> the node/`board_edges` set-up in
//! [`net_ratsnest`]. `RN_NET::TRIANGULATOR_STATE::Triangulate` ->
//! [`triangulate_nodes`], including its three special cases (empty, every
//! node coincident, all nodes collinear) and the "also add each
//! triangle-adjacency diagonal" step after the plain triangle edges,
//! which is a real KiCad optimisation (a denser candidate-edge set gives
//! Kruskal more (and often shorter) options to complete the net with) we
//! keep for fidelity. `RN_NET::kruskalMST` -> the union-find pass at the
//! end of [`net_ratsnest`]. `RN_NET::compute`'s `<= 2` node special case
//! is also ported directly.
//!
//! Not ported: `RN_NET::OptimizeRNEdges`, which nudges a ratsnest line's
//! *endpoint* to the nearest point on a zone's computed fill outline
//! (purely cosmetic -- it never changes which items a line connects, only
//! where it visually lands on a zone). Since zones here are an outline
//! approximation already (see [`crate::items`]), this would have snapped
//! to the same outline anyway; skipped for now rather than optimizing the
//! look of an already-approximate shape.

use std::collections::BTreeMap;

use eda_model::ir::Point;

use crate::algo::{Cluster, ConnGraph};
use crate::delaunay::{Triangulation, INVALID_INDEX};
use crate::items::ItemRef;

pub struct RatsnestEdge {
    pub net: String,
    pub from: Point,
    pub to: Point,
}

fn dist(a: Point, b: Point) -> f64 {
    (((a.x - b.x) as f64).powi(2) + ((a.y - b.y) as f64).powi(2)).sqrt()
}

/// The ratsnest for every net: KiCad's `CONNECTIVITY_DATA::updateRatsnest`
/// run over every net at once, minus the isolated-zone-island skip's
/// "only when zone fill exists" nuance (already handled, see
/// [`crate::items`]).
pub fn compute_ratsnest(graph: &ConnGraph, clusters: &[Cluster]) -> Vec<RatsnestEdge> {
    let mut by_net: BTreeMap<&str, Vec<&Cluster>> = BTreeMap::new();
    for c in clusters {
        // "Don't add intentionally-kept zone islands to the ratsnest":
        // `internalRecalculateRatsnest` skips a cluster that is exactly
        // one lone zone outline touching nothing else at all.
        if c.items.len() == 1 && matches!(graph.items[c.items[0]].item, ItemRef::ZoneOutline { .. }) {
            continue;
        }
        by_net.entry(c.net.as_str()).or_default().push(c);
    }

    let mut edges = Vec::new();
    for (net, nets_clusters) in by_net {
        edges.extend(net_ratsnest(graph, net, &nets_clusters));
    }
    edges
}

struct NodeInfo {
    pos: Point,
    cluster: usize,
    layer_lo: i32,
    layer_hi: i32,
}

/// `RN_NET` for one net: `AddCluster` for every cluster on it, then
/// `UpdateNet`/`compute`.
fn net_ratsnest(graph: &ConnGraph, net: &str, clusters: &[&Cluster]) -> Vec<RatsnestEdge> {
    let mut nodes: Vec<NodeInfo> = Vec::new();
    let mut board_edges: Vec<(usize, usize)> = Vec::new();

    for (ci, cluster) in clusters.iter().enumerate() {
        let mut first: Option<usize> = None;
        for &item_idx in &cluster.items {
            let it = &graph.items[item_idx];
            // A zone contributes only its first outline point as a node
            // (`nAnchors = zoneLayer ? 1 : anchors.size()` in
            // `RN_NET::AddCluster`); everything else contributes every
            // anchor it has (1 for a pad/via, 2 for a track/arc segment).
            let n_anchors = if matches!(it.item, ItemRef::ZoneOutline { .. }) { it.anchors.len().min(1) } else { it.anchors.len() };
            for &pos in &it.anchors[..n_anchors] {
                let node_idx = nodes.len();
                nodes.push(NodeInfo { pos, cluster: ci, layer_lo: it.layer_lo, layer_hi: it.layer_hi });
                match first {
                    None => first = Some(node_idx),
                    Some(f) => board_edges.push((f, node_idx)),
                }
            }
        }
    }

    if nodes.len() <= 2 {
        // `RN_NET::compute`'s early-out: with 0 or 1 node there is nothing
        // to connect; with exactly 2, a ratsnest edge is needed only if
        // they are *not* already in the same cluster (no board edge
        // between them).
        return if board_edges.is_empty() && nodes.len() == 2 { vec![RatsnestEdge { net: net.to_string(), from: nodes[0].pos, to: nodes[1].pos }] } else { Vec::new() };
    }

    let mut weighted: Vec<(usize, usize, f64)> = board_edges.into_iter().map(|(a, b)| (a, b, 0.0)).collect();
    weighted.extend(triangulate_nodes(&nodes));

    weighted.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));

    let mut dsu = DisjointSet::new(nodes.len());
    let mut out = Vec::new();
    for (a, b, w) in weighted {
        if dsu.unite(a, b) && w > 0.0 {
            out.push(RatsnestEdge { net: net.to_string(), from: nodes[a].pos, to: nodes[b].pos });
        }
    }
    out
}

/// `RN_NET::TRIANGULATOR_STATE::Triangulate`: de-duplicate coincident
/// node positions, triangulate the unique positions (or fall back to a
/// colinear chain / same-point chain), then re-attach every duplicate at
/// each unique position back into the edge set.
fn triangulate_nodes(nodes: &[NodeInfo]) -> Vec<(usize, usize, f64)> {
    let mut out = Vec::new();
    if nodes.is_empty() {
        return out;
    }

    // Sort by (x, y), matching the C++ `std::multiset<CN_ANCHOR, CN_PTR_CMP>`
    // node order (`Point` derives `Ord` in exactly that field order).
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by_key(|&i| nodes[i].pos);

    // `reps[k]`: one representative original node index for the k-th
    // unique position. `chains[k]`: every original index at that position
    // (including `reps[k]`), in sorted order.
    let mut reps: Vec<usize> = Vec::new();
    let mut chains: Vec<Vec<usize>> = Vec::new();
    let mut last_pos: Option<Point> = None;
    for &oi in &order {
        if last_pos != Some(nodes[oi].pos) {
            reps.push(oi);
            chains.push(Vec::new());
            last_pos = Some(nodes[oi].pos);
        }
        chains.last_mut().expect("just pushed").push(oi);
    }

    let add_edge = |out: &mut Vec<(usize, usize, f64)>, a: usize, b: usize| out.push((a, b, dist(nodes[a].pos, nodes[b].pos)));

    if reps.len() == 1 {
        // Every node coincides. Chain consecutive nodes (stable multiset
        // order) that share no copper layer -- e.g. a top-only and a
        // bottom-only SMD pad stacked at the same (x, y) are still
        // distinct electrically until *something* joins them.
        for w in order.windows(2) {
            let (a, b) = (w[0], w[1]);
            let overlap = nodes[a].layer_lo.max(nodes[b].layer_lo) <= nodes[a].layer_hi.min(nodes[b].layer_hi);
            if !overlap {
                out.push((a, b, 1.0));
            }
        }
        return out;
    }

    let colinear = reps.len() <= 2 || {
        let p0 = nodes[reps[0]].pos;
        let v0 = (nodes[reps[1]].pos.x - p0.x, nodes[reps[1]].pos.y - p0.y);
        reps[2..].iter().all(|&r| {
            let v1 = (nodes[r].pos.x - p0.x, nodes[r].pos.y - p0.y);
            v0.0 * v1.1 - v0.1 * v1.0 == 0
        })
    };

    if colinear {
        // No triangulation exists for collinear points: chain them along
        // the sorted order instead, same as upstream.
        for w in reps.windows(2) {
            add_edge(&mut out, w[0], w[1]);
        }
    } else {
        let coords: Vec<f64> = reps.iter().flat_map(|&r| [nodes[r].pos.x as f64, nodes[r].pos.y as f64]).collect();
        let tri = Triangulation::new(&coords);
        let t = &tri.triangles;
        for k in (0..t.len()).step_by(3) {
            add_edge(&mut out, reps[t[k]], reps[t[k + 1]]);
            add_edge(&mut out, reps[t[k + 1]], reps[t[k + 2]]);
            add_edge(&mut out, reps[t[k + 2]], reps[t[k]]);
        }
        // The triangle-adjacency diagonal: for every interior half-edge,
        // also connect its triangle's start vertex to the vertex across
        // the shared edge -- see the module docs.
        for (e, &he) in tri.halfedges.iter().enumerate() {
            if he != INVALID_INDEX {
                add_edge(&mut out, reps[t[e]], reps[t[he]]);
            }
        }
    }

    // Re-link same-position duplicates: consecutive, ordered by a
    // deterministic per-node cluster id. (KiCad sorts these by
    // `CN_CLUSTER*` pointer identity -- an arbitrary, process-local order
    // -- so a stable cluster index is a strict improvement, not a
    // deviation that changes the edge set's weights or endpoints.)
    for chain in &chains {
        if chain.len() < 2 {
            continue;
        }
        let mut sorted = chain.clone();
        sorted.sort_by_key(|&i| nodes[i].cluster);
        for w in sorted.windows(2) {
            let weight = if nodes[w[0]].cluster != nodes[w[1]].cluster { 1.0 } else { 0.0 };
            out.push((w[0], w[1], weight));
        }
    }

    out
}

/// Union-find with path compression and union-by-depth, matching
/// `ratsnest_data.cpp`'s private `disjoint_set` used by `kruskalMST`.
struct DisjointSet {
    parent: Vec<usize>,
    depth: Vec<usize>,
}

impl DisjointSet {
    fn new(n: usize) -> Self {
        DisjointSet { parent: (0..n).collect(), depth: vec![0; n] }
    }

    fn find(&mut self, v: usize) -> usize {
        let mut root = v;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        let mut cur = v;
        while self.parent[cur] != cur {
            let next = self.parent[cur];
            self.parent[cur] = root;
            cur = next;
        }
        root
    }

    /// Union the two sets; `true` if they were distinct (an actual union
    /// happened), matching `disjoint_set::unite`'s return value, which
    /// `kruskalMST` uses to decide whether an edge belongs in the MST.
    fn unite(&mut self, a: usize, b: usize) -> bool {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return false;
        }
        if self.depth[ra] < self.depth[rb] {
            self.parent[ra] = rb;
        } else {
            self.parent[rb] = ra;
            if self.depth[ra] == self.depth[rb] {
                self.depth[ra] += 1;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::{build_graph, search_clusters};
    use crate::tests_support::{pad_center, two_pad_model};
    use eda_model::ir::Track;

    #[test]
    fn union_find_reports_whether_it_actually_merged() {
        let mut dsu = DisjointSet::new(4);
        assert!(dsu.unite(0, 1));
        assert!(!dsu.unite(0, 1), "already in the same set");
        assert!(dsu.unite(2, 3));
        assert!(dsu.unite(1, 2));
        assert!(!dsu.unite(0, 3));
    }

    #[test]
    fn three_disconnected_pads_on_one_net_need_exactly_two_ratsnest_edges() {
        let (mut design, mut model) = two_pad_model();
        // Add a third unconnected pad on the same net -- N clusters always
        // need exactly N-1 ratsnest edges to finish, regardless of which
        // valid triangulation produced the candidate edges.
        model.parts.push(eda_model::Part { reference: "R3".into(), mpn: None, value: None, package: Some("0603".into()), footprint: Some("0603".into()), pins: vec![eda_model::Pin { number: "1".into(), name: None, kind: eda_model::PinKind::Passive }], body_um: None, edge: None });
        model.nets[0].pins.push("R3.1".into());
        design.placement.as_mut().unwrap().footprints.push(eda_model::ir::FootprintInstance { id: "R3".into(), at: eda_model::ir::Point { x: 10_000, y: 15_000 }, rot: 0, side: eda_model::ir::Side::Top, label: Default::default() });

        let graph = build_graph(&design, &model);
        let clusters = search_clusters(&graph);
        assert_eq!(clusters.len(), 3);
        let edges = compute_ratsnest(&graph, &clusters);
        assert_eq!(edges.len(), 2, "{} clusters need {} ratsnest edges", clusters.len(), clusters.len() - 1);
    }

    #[test]
    fn a_track_on_the_net_removes_its_ratsnest_edge() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![a, b] });
        let graph = build_graph(&design, &model);
        let clusters = search_clusters(&graph);
        assert_eq!(clusters.len(), 1);
        assert!(compute_ratsnest(&graph, &clusters).is_empty());
    }
}
