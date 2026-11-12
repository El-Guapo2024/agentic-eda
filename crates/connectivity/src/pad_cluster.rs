//! `CONNECTIVITY_DATA::GetConnectedItems( pad, EXCLUDE_ZONES )` (KiCad 25ec2335 `pcbnew/connectivity/connectivity_data.cpp`) for the tools that move a pad's copper
//! with it (Swap Pad Nets and Swap Gate Nets, `EDIT_TOOL::SwapPadNets`): the items of the cluster of same-net copper the pad is in -- its tracks, vias and the other pads
//! those reach, however far. Zones are not in it, and so a zone fill carries nothing from one item to the next: `CN_CONNECTIVITY_ALGO::SearchClusters( .., aExcludeZones )`
//! leaves the zones out of the items it searches (`addToSearchList`), so a pad that only a fill joins to a swapped one is not reached and keeps its net. That is
//! KiCad 25ec2335's (UI-LEFT-2 PP17); 8303b2ad asked `GetConnectedItems( pad, 0 )`, which walks through the fills and dropped only the zones themselves from what it
//! changed, so such a pad took the new net there.

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
                        ItemRef::ZoneOutline { .. } | ItemRef::Shape { .. } => {}
                    }
                }
            }
            out
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    
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
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![a, mid], arc_mid_offset: None });
        rt.tracks.push(Track { id: "t2".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![mid, b], arc_mid_offset: None });
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: mid, drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        rt.tracks.push(Track { id: "far".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: a.x, y: a.y + mm(5.0) }, Point { x: a.x + mm(1.0), y: a.y + mm(5.0) }], arc_mid_offset: None });
        let clusters = pad_clusters(&design, &model, &[("R1".into(), "1".into(), 1)]);
        let c = &clusters[0];
        assert_eq!(c.tracks, ["t1".to_string(), "t2".to_string()].into(), "the tracks that reach the pad, and the one beyond the via, not the one that touches nothing");
        assert_eq!(c.vias, ["v1".to_string()].into());
        assert_eq!(c.pads, [("R1".to_string(), "1".to_string(), 1), ("R2".to_string(), "1".to_string(), 1)].into(), "R2.1 is reached through the copper");
    }

    /// UI-LEFT-2 PP17: two pads that only a zone fill joins are one cluster of the connectivity (`SearchClusters( .., false )`, what 8303b2ad's Swap Pad Nets walked)
    /// but not of what Swap Pad Nets moves (KiCad 25ec2335's `EXCLUDE_ZONES`): the other pad keeps its net.
    #[test]
    fn a_pad_only_a_zone_fill_joins_is_not_in_the_swaps_cluster() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        let (x0, x1, y) = (a.x.min(b.x) - mm(1.0), a.x.max(b.x) + mm(1.0), a.y);
        let outline = vec![Point { x: x0, y: y - mm(1.0) }, Point { x: x1, y: y - mm(1.0) }, Point { x: x1, y: y + mm(1.0) }, Point { x: x0, y: y + mm(1.0) }];
        let mut copper = ZoneCopper::default();
        let chain = outline.iter().map(|p| eda_clipper2::Point64 { x: p.x.get(), y: p.y.get(), z: 0 }).collect();
        copper.zones.insert(("z".into(), "F.Cu".into()), eda_shape_poly_set::ShapePolySet::from_outline(chain));
        design.routing.as_mut().unwrap().zones.push(eda_model::ir::Zone { id: "z".into(), net: "N1".into(), layer: "F.Cu".into(), outline, ..Default::default() });
        // with its fill, the zone joins the two pads
        let graph = build_graph(&design, &model, &copper);
        let pad = |r: &str| graph.items.iter().position(|i| matches!(&i.item, ItemRef::Pad { reference, number } if reference == r && number == "1")).unwrap();
        let joined = search_clusters(&graph).iter().any(|c| c.items.contains(&pad("R1")) && c.items.contains(&pad("R2")));
        assert!(joined, "the fill joins R1.1 and R2.1");
        // the swap's cluster leaves the zones out
        let clusters = pad_clusters(&design, &model, &[("R1".into(), "1".into(), 1)]);
        assert_eq!(clusters[0].pads, [("R1".to_string(), "1".to_string(), 1)].into(), "R2.1 is not reached: it keeps its net");
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
