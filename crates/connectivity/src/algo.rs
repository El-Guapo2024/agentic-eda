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
use eda_drc::zone_copper::ZoneCopper;

pub struct ConnGraph {
    pub items: Vec<CnItem>,
}

/// Build every [`CnItem`] and connect every pair that physically touches --
/// `CN_CONNECTIVITY_ALGO::Build` + `searchConnections`. Unlike upstream we
/// have no incremental/dirty-item story (there is no live editing session
/// here): every item is (re)built and every candidate pair is (re)tested
/// on each call.
pub fn build_graph(design: &Design, model: &ConstraintModel, copper: &ZoneCopper) -> ConnGraph {
    let mut items = items::build_items(design, model, copper);
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

    // The pieces of one padstack pad are one pad: every piece touches every other (`CN_ITEM` of a pad spans its layers).
    let mut pieces: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for (i, it) in items.iter().enumerate() {
        if let Some(group) = it.pad_piece_of {
            pieces.entry(group).or_default().push(i);
        }
    }
    for members in pieces.values() {
        for &a in members {
            for &b in members {
                if a != b && !edges[a].contains(&b) {
                    edges[a].push(b);
                }
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
        let graph = build_graph(&design, &model, &ZoneCopper::default());
        let clusters = search_clusters(&graph);
        assert_eq!(clusters.len(), 2);
        assert!(clusters.iter().all(|c| c.net == "N1"));
        assert!(clusters.iter().all(|c| c.items.len() == 1));
    }

    #[test]
    fn a_track_between_them_merges_into_one_cluster() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![a, b], arc_mid_offset: None });
        let graph = build_graph(&design, &model, &ZoneCopper::default());
        let clusters = search_clusters(&graph);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].items.len(), 3, "2 pads + 1 track segment");
    }

    /// A plated through-hole pad J1.1 on net N: a 1.6 mm circle at the front, a 3 mm square on the inner layers and a 2.4 x 1.0 mm oval at the back of a four-layer board.
    fn padstack_model(side: eda_model::ir::Side) -> (eda_model::ir::Design, eda_model::ConstraintModel) {
        use eda_model::padstack::{PadLayerShape, PadStack, PadStackMode};
        use eda_model::{BoardRules, Footprint, Net, Pad, PadKind, PadShape, Part, Pin, PinKind};
        let part = Part { reference: "J1".into(), mpn: None, lcsc: None, value: None, package: Some("TH1".into()), footprint: Some("TH1".into()), pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }], body_um: None, symbol: None, datasheet: None, edge: None };
        let mut pad = Pad { number: "1".into(), at: (0, 0), size: (1600, 1600), shape: PadShape::Circle, kind: PadKind::ThroughHole, drill: Some(800), drill_slot: None, rot: 0, roundrect_ratio: None, opposite_side: false, ext: Default::default() };
        pad.ext.stack = PadStack { mode: PadStackMode::FrontInnerBack, inner: Some(PadLayerShape::plain(PadShape::Rect, (3000, 3000))), back: Some(PadLayerShape::plain(PadShape::Oval, (2400, 1000))), ..Default::default() };
        let model = eda_model::ConstraintModel {
            parts: vec![part],
            nets: vec![Net { name: "N".into(), pins: vec!["J1.1".into()] }],
            footprints: vec![Footprint { name: "TH1".into(), pads: vec![pad], courtyard: Some((2000, 2000)), model: None, courtyard_outlines: vec![], models3d: vec![], art: Default::default() }],
            board: BoardRules { layers: vec!["F.Cu".into(), "In1.Cu".into(), "In2.Cu".into(), "B.Cu".into()], ..Default::default() },
            ..Default::default()
        };
        let (mut design, _) = two_pad_model();
        design.placement.as_mut().unwrap().footprints = vec![eda_model::ir::FootprintInstance { id: "J1".into(), at: eda_model::ir::Point { x: 10_000, y: 10_000 }, rot: 0, side, label: Default::default() }];
        (design, model)
    }

    /// The track on `layer` whose end is at (10000 + dx, 10000), and what the pad's pieces and the track have to say about one another.
    fn track_touches_the_pad(layer: &str, dx: i64, side: eda_model::ir::Side) -> (usize, bool) {
        let (mut design, model) = padstack_model(side);
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N".into(), pins: vec![], layer: layer.into(), width: 200, pts: vec![eda_model::ir::Point { x: 10_000 + dx, y: 10_000 }, eda_model::ir::Point { x: 10_000 + dx + 3_000, y: 10_000 }], arc_mid_offset: None });
        let graph = build_graph(&design, &model, &ZoneCopper::default());
        let pads: Vec<usize> = graph.items.iter().enumerate().filter(|(_, i)| matches!(i.item, ItemRef::Pad { .. })).map(|(i, _)| i).collect();
        let track = graph.items.iter().position(|i| matches!(i.item, ItemRef::TrackSeg { .. })).unwrap();
        (pads.len(), pads.iter().any(|p| graph.items[track].connected.contains(p)))
    }

    #[test]
    fn a_track_connects_to_a_padstack_pad_where_the_copper_of_its_layer_is() {
        use eda_model::ir::Side;
        // 1.2 mm from the centre: outside the 0.8 mm circle of the front, inside the 1.5 mm half side of the inner square, outside the oval's 1.2 mm half length by nothing.
        let (pieces, on_inner) = track_touches_the_pad("In1.Cu", 1_200, Side::Top);
        assert_eq!(pieces, 3, "an item for the front, one for the inner layers and one for the back");
        assert!(on_inner, "the inner square reaches the track's end");
        assert!(!track_touches_the_pad("F.Cu", 1_200, Side::Top).1, "the front circle does not");
        assert!(track_touches_the_pad("B.Cu", 1_150, Side::Top).1, "the back oval is 2.4 mm long: it reaches 1.15 mm");
        assert!(!track_touches_the_pad("B.Cu", 1_500, Side::Top).1, "a track end within its half width of the copper connects, farther does not");
        // A bottom-side footprint has the oval on the board's front.
        assert!(track_touches_the_pad("F.Cu", 1_150, Side::Bottom).1);
        assert!(!track_touches_the_pad("B.Cu", 1_150, Side::Bottom).1);
    }

    #[test]
    fn the_pieces_of_one_padstack_pad_are_one_pad_to_the_clusters() {
        let (mut design, model) = padstack_model(eda_model::ir::Side::Top);
        // A track on In1 ends in the inner square and a second one on B.Cu ends in the oval: the pad joins them.
        for (id, layer, x) in [("t1", "In1.Cu", 11_200), ("t2", "B.Cu", 11_150)] {
            design.routing.as_mut().unwrap().tracks.push(Track { id: id.into(), net: "N".into(), pins: vec![], layer: layer.into(), width: 200, pts: vec![eda_model::ir::Point { x, y: 10_000 }, eda_model::ir::Point { x: x + 3_000, y: 10_000 }], arc_mid_offset: None });
        }
        let graph = build_graph(&design, &model, &ZoneCopper::default());
        let clusters = search_clusters(&graph);
        assert_eq!(clusters.len(), 1, "one pad, two tracks: one cluster");
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
        let graph = build_graph(&design, &model, &ZoneCopper::default());
        let clusters = search_clusters(&graph);
        // Pads on different, fixed nets are never even shape-tested (see
        // `build_graph`), so no edge should have formed between them.
        let r1 = graph.items.iter().position(|i| matches!(&i.item, ItemRef::Pad{reference,number} if reference=="R1" && number=="1")).unwrap();
        let r2 = graph.items.iter().position(|i| matches!(&i.item, ItemRef::Pad{reference,number} if reference=="R2" && number=="1")).unwrap();
        assert!(!graph.items[r1].connected.contains(&r2));
        assert_eq!(clusters.len(), 2);
    }
}
