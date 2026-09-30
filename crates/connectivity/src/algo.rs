//! `CN_CONNECTIVITY_ALGO` (`pcbnew/connectivity/connectivity_algo.{h,cpp}`):
//! find every pair of touching items, then flood-fill same-net clusters.
//!
//! `Build`/`searchConnections`/`CN_VISITOR` become [`build_graph`]:
//! spatially index every item, then for each pair whose boxes and layer
//! spans overlap, test the geometry (see [`crate::geom::touches`]) and
//! record a symmetric edge. `SearchClusters` becomes [`search_clusters`].
//!
//! One deliberate simplification, not a shortcut: KiCad's `SearchClusters`
//! takes a `CLUSTER_SEARCH_MODE`. `CSM_PROPAGATE` (net inference, used by
//! `PropagateNets`) floods across *any* touching items regardless of net
//! and then votes on which net should win -- `CN_CLUSTER`'s origin-net/
//! rank/"conflicting" bookkeeping exists entirely for that mode. Ratsnest
//! uses `CSM_RATSNEST`, whose flood-fill already refuses to cross a net
//! boundary (`if (withinAnyNet && n->Net() != root->Net()) continue;` in
//! `connectivity_algo.cpp`), so every item in one of its clusters already
//! shares one net by construction -- the rank/conflict machinery can
//! never fire and is dead code for that mode. Since this crate has no use
//! for `CSM_PROPAGATE` (every item's net is already given -- see the
//! crate root docs), [`search_clusters`] only implements the
//! `CSM_RATSNEST` shape: a [`Cluster`] is simply "a same-net,
//! physically-connected group of items."

use std::collections::VecDeque;

use eda_model::ir::Design;
use eda_model::ConstraintModel;

use crate::geom;
use crate::grid::Grid;
use crate::items::{self, layers_overlap, CnItem};

pub struct ConnGraph {
    pub items: Vec<CnItem>,
}

/// Build every [`CnItem`] and connect every pair that physically touches --
/// `CN_CONNECTIVITY_ALGO::Build` + `searchConnections`. Unlike upstream we
/// have no incremental/dirty-item story (there is no live editing session
/// here): every item is (re)built and every candidate pair is (re)tested
/// on each call.
pub fn build_graph(design: &Design, model: &ConstraintModel) -> ConnGraph {
    let mut items = items::build_items(design, model);
    if items.len() < 2 {
        return ConnGraph { items };
    }

    let boxes: Vec<_> = items.iter().map(|it| it.bbox).collect();
    let grid = Grid::build(&boxes);

    let mut edges: Vec<Vec<usize>> = vec![Vec::new(); items.len()];
    for i in 0..items.len() {
        for j in grid.query(items[i].bbox) {
            if j <= i {
                continue; // unordered pairs, each tested once
            }
            if !layers_overlap(&items[i], &items[j]) {
                continue;
            }
            // CN_VISITOR::operator(): two items that can't change net
            // (pads, zones) and disagree on net are never even
            // shape-tested -- KiCad's dead-short-avoidance early-out.
            // Items that *can* change net (tracks/vias) are still tested
            // against a different net's pad/zone: whether that overlap
            // should exist at all is a DRC (clearance/short) question,
            // out of scope here (see the crate root docs).
            if !items[i].can_change_net && !items[j].can_change_net && items[i].net != items[j].net {
                continue;
            }
            if geom::touches(&items[i].shape, &items[i].anchors, &items[j].shape, &items[j].anchors) {
                edges[i].push(j);
                edges[j].push(i);
            }
        }
    }

    for (it, mut e) in items.iter_mut().zip(edges) {
        e.sort_unstable();
        it.connected = e;
    }

    ConnGraph { items }
}

/// A same-net, physically-connected group of items -- see the module docs
/// for why this needs no origin-net/conflict bookkeeping the way
/// `CN_CLUSTER` does.
pub struct Cluster {
    pub net: String,
    /// Indices into the owning [`ConnGraph`]'s `items`.
    pub items: Vec<usize>,
}

/// `CN_CONNECTIVITY_ALGO::SearchClusters(CSM_RATSNEST)`: flood-fill
/// same-net connected components across every item with a net. Root
/// selection walks the item list in a fixed order (KiCad's own root
/// order is essentially pointer-address order -- arbitrary -- so a
/// deterministic index order is a strict improvement, not a deviation
/// that changes which clusters exist).
pub fn search_clusters(graph: &ConnGraph) -> Vec<Cluster> {
    let n = graph.items.len();
    let mut visited = vec![false; n];
    let mut clusters = Vec::new();

    for root in 0..n {
        if visited[root] || graph.items[root].net.is_empty() {
            continue;
        }
        let root_net = graph.items[root].net.clone();
        let mut members = Vec::new();
        let mut q = VecDeque::new();
        visited[root] = true;
        q.push_back(root);

        while let Some(cur) = q.pop_front() {
            members.push(cur);
            for &nb in &graph.items[cur].connected {
                if visited[nb] || graph.items[nb].net != root_net {
                    continue;
                }
                visited[nb] = true;
                q.push_back(nb);
            }
        }

        clusters.push(Cluster { net: root_net, items: members });
    }

    clusters.sort_by(|a, b| a.net.cmp(&b.net));
    clusters
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::ItemRef;
    use crate::tests_support::{pad_center, two_pad_model};
    use eda_model::ir::Track;

    #[test]
    fn two_pads_with_no_copper_are_two_separate_clusters() {
        let (design, model) = two_pad_model();
        let graph = build_graph(&design, &model);
        let clusters = search_clusters(&graph);
        assert_eq!(clusters.len(), 2);
        assert!(clusters.iter().all(|c| c.net == "N1"));
        assert!(clusters.iter().all(|c| c.items.len() == 1));
    }

    #[test]
    fn a_track_between_them_merges_into_one_cluster() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![a, b] });
        let graph = build_graph(&design, &model);
        let clusters = search_clusters(&graph);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].items.len(), 3, "2 pads + 1 track segment");
    }

    #[test]
    fn different_nets_never_merge_even_when_pads_touch() {
        let (mut design, mut model) = two_pad_model();
        // Move R2 on top of R1 so their pads coincide; they must still be
        // two clusters because their nets differ.
        model.nets[0].pins = vec!["R1.1".into()];
        model.nets.push(eda_model::Net { name: "N2".into(), pins: vec!["R2.1".into()] });
        if let Some(pl) = design.placement.as_mut() {
            pl.footprints[1].at = pl.footprints[0].at;
        }
        let graph = build_graph(&design, &model);
        let clusters = search_clusters(&graph);
        // Pads on different, fixed nets are never even shape-tested (see
        // `build_graph`), so no edge should have formed between them.
        let r1 = graph.items.iter().position(|i| matches!(&i.item, ItemRef::Pad{reference,number} if reference=="R1" && number=="1")).unwrap();
        let r2 = graph.items.iter().position(|i| matches!(&i.item, ItemRef::Pad{reference,number} if reference=="R2" && number=="1")).unwrap();
        assert!(!graph.items[r1].connected.contains(&r2));
        assert_eq!(clusters.len(), 2);
    }
}
