//! `CONNECTIVITY_DATA::GetConnectedItems( pad, 0 )` (`pcbnew/connectivity/connectivity_data.cpp`) for the tools that move a pad's copper with it (Swap Pad Nets and Swap
//! Gate Nets, `EDIT_TOOL::SwapPadNets`): the items of the cluster of same-net copper the pad is in -- its tracks, vias and the other pads those reach, however far.
//! Zones are not in the answer (the tools leave them alone), and a zone fill does not carry a cluster from one item to the next here, as the fills are kicad-cli's.

use std::collections::BTreeSet;

use eda_drc::zone_copper::ZoneCopper;
use eda_model::ir::Design;
use eda_model::ConstraintModel;

use crate::algo::{build_graph, search_clusters};
use crate::items::ItemRef;

/// The copper connected to a pad.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PadCluster {
    /// `Track::id` of every track with a segment in the cluster.
    pub tracks: BTreeSet<String>,
    /// `Via::id` of every via in it.
    pub vias: BTreeSet<String>,
    /// The pads in it, the pad itself included: footprint reference, pad number and which of the pads with that number it is (1-based, the studio's `REF.NUM#k`).
    pub pads: BTreeSet<(String, String, u32)>,
}

/// [`PadCluster`] of each of the pads asked for (`wanted`: footprint reference, number, `nth`), in the same order. A pad with no net, or one the board does not have, is a
/// cluster of its own alone.
pub fn pad_clusters(design: &Design, model: &ConstraintModel, wanted: &[(String, String, u32)]) -> Vec<PadCluster> {
    let graph = build_graph(design, model, &ZoneCopper::default());
    let clusters = search_clusters(&graph);
    let pad_of = crate::flash::pad_of_items(&graph);
    let mut cluster_of_item: Vec<Option<usize>> = vec![None; graph.items.len()];
    for (c, cluster) in clusters.iter().enumerate() {
        for &i in &cluster.items {
            cluster_of_item[i] = Some(c);
        }
    }
    wanted
        .iter()
        .map(|w| {
            let mut out = PadCluster::default();
            out.pads.insert(w.clone());
            let mut reached: BTreeSet<usize> = BTreeSet::new();
            for (i, p) in pad_of.iter().enumerate() {
                if p.as_ref() == Some(w) {
                    if let Some(c) = cluster_of_item[i] {
                        reached.insert(c);
                    }
                }
            }
            for c in reached {
                for &i in &clusters[c].items {
                    match &graph.items[i].item {
                        ItemRef::TrackSeg { track_id, .. } => {
                            out.tracks.insert(track_id.clone());
                        }
                        ItemRef::Via { via_id } => {
                            out.vias.insert(via_id.clone());
                        }
                        ItemRef::Pad { .. } => {
                            if let Some(p) = &pad_of[i] {
                                out.pads.insert(p.clone());
                            }
                        }
                        ItemRef::ZoneOutline { .. } => {}
                    }
                }
            }
            out
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::{pad_center, two_pad_model};
    use eda_model::ir::{Point, Track, Via};

    #[test]
    fn a_pads_cluster_is_the_copper_that_reaches_it_and_the_pads_that_copper_reaches() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        let mid = Point { x: (a.x + b.x) / 2, y: a.y };
        let rt = design.routing.as_mut().unwrap();
        // R1.1 - mid - R2.1 on F.Cu, a via at the middle, and a track of another net that crosses nothing.
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![a, mid], arc_mid_offset: None });
        rt.tracks.push(Track { id: "t2".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![mid, b], arc_mid_offset: None });
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: mid, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        rt.tracks.push(Track { id: "far".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![Point { x: a.x, y: a.y + 5_000 }, Point { x: a.x + 1_000, y: a.y + 5_000 }], arc_mid_offset: None });
        let clusters = pad_clusters(&design, &model, &[("R1".into(), "1".into(), 1)]);
        let c = &clusters[0];
        assert_eq!(c.tracks, ["t1".to_string(), "t2".to_string()].into(), "the tracks that reach the pad, and the one beyond the via, not the one that touches nothing");
        assert_eq!(c.vias, ["v1".to_string()].into());
        assert_eq!(c.pads, [("R1".to_string(), "1".to_string(), 1), ("R2".to_string(), "1".to_string(), 1)].into(), "R2.1 is reached through the copper");
    }

    #[test]
    fn a_pad_with_no_copper_is_its_own_cluster_and_one_the_board_lacks_is_too() {
        let (design, model) = two_pad_model();
        let clusters = pad_clusters(&design, &model, &[("R1".into(), "2".into(), 1), ("R9".into(), "1".into(), 1)]);
        assert_eq!(clusters[0].pads.len(), 1);
        assert!(clusters[0].tracks.is_empty() && clusters[0].vias.is_empty());
        assert_eq!(clusters[1].pads, [("R9".to_string(), "1".to_string(), 1)].into());
    }
}
