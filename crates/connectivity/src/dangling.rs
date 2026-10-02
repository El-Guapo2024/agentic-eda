//! `CONNECTIVITY_DATA::TestTrackEndpointDangling`
//! (`pcbnew/connectivity/connectivity_data.cpp`), the function
//! `DRC_TEST_PROVIDER_CONNECTIVITY::Run` calls to produce `DRCE_DANGLING_TRACK`
//! / `DRCE_DANGLING_VIA` (`pcbnew/drc/drc_test_provider_connectivity.cpp`,
//! which is what `kicad-cli pcb drc`'s `track_dangling`/`via_dangling`
//! come from) -- ported directly rather than via the more generic
//! `CN_ANCHOR::IsDangling`, since that DRC provider is the actual oracle
//! this crate is checked against, and it calls
//! `TestTrackEndpointDangling(track, /*aIgnoreTracksInPads=*/true, ...)`
//! specifically. That `true` is hard-coded below to match.

use eda_model::ir::Point;

use crate::algo::ConnGraph;
use crate::geom;
use crate::items::{ItemRef, ItemShape};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DanglingKind {
    Track,
    Via,
}

pub struct DanglingItem {
    pub kind: DanglingKind,
    pub net: String,
    /// The track's or via's own id (`Track::id`/`Via::id`); for a
    /// multi-segment track, `#N` names which segment (0-based).
    pub id: String,
    pub at: Point,
}

/// Every dangling track/arc segment and via on the board.
pub fn dangling_tracks_and_vias(graph: &ConnGraph) -> Vec<DanglingItem> {
    let mut out = Vec::new();
    for idx in 0..graph.items.len() {
        let it = &graph.items[idx];
        match &it.item {
            ItemRef::TrackSeg { track_id, seg_index } => {
                if let Some(at) = track_seg_dangling(graph, idx) {
                    let id = if *seg_index == 0 { track_id.clone() } else { format!("{track_id}#{seg_index}") };
                    out.push(DanglingItem { kind: DanglingKind::Track, net: it.net.clone(), id, at });
                }
            }
            ItemRef::Via { via_id } => {
                if let Some(at) = via_dangling(graph, idx) {
                    out.push(DanglingItem { kind: DanglingKind::Via, net: it.net.clone(), id: via_id.clone(), at });
                }
            }
            _ => {}
        }
    }
    out
}

/// Does `other`'s shape come within `accuracy` of point `p`? KiCad tests
/// `item->GetEffectiveShape(layer)->Collide(pos, accuracy)` here; the
/// per-shape equivalents already exist in [`crate::geom`].
fn item_hits_point(other: &crate::items::CnItem, p: Point, accuracy: f64) -> bool {
    match &other.shape {
        ItemShape::Pad(pad) => pad.signed_distance(p) <= accuracy,
        ItemShape::Via { center, radius } => geom::dist(p, *center) <= radius + accuracy,
        ItemShape::Segment { a, b, half_width } => geom::point_seg_distance(p, *a, *b) <= half_width + accuracy,
        // `rtree->QueryColliding( SHAPE_CIRCLE( p, accuracy ) )` on the fill.
        ItemShape::Zone { outline } => geom::zone_hits_point(outline, p, accuracy),
    }
}

/// `getMinDist` in `connectivity_data.cpp`, restricted to the one branch
/// this port ever reaches it for (see [`track_seg_dangling`]).
fn min_dist_to_track(other: &crate::items::CnItem, p: Point) -> f64 {
    match &other.shape {
        ItemShape::Segment { a, b, .. } => geom::dist(p, *a).min(geom::dist(p, *b)),
        _ => 0.0,
    }
}

/// `TestTrackEndpointDangling`'s `PCB_TRACE_T`/`PCB_ARC_T` branch, called
/// with `aIgnoreTracksInPads = true` (see module docs). `graph.items[idx]`
/// must be a [`ItemRef::TrackSeg`].
fn track_seg_dangling(graph: &ConnGraph, idx: usize) -> Option<Point> {
    let it = &graph.items[idx];
    let (a, b, half_width) = match &it.shape {
        ItemShape::Segment { a, b, half_width } => (*a, *b, *half_width),
        _ => return None,
    };

    if it.connected.is_empty() {
        // Nothing touches either end: KiCad's loop below never increments
        // either counter, so `start_count == 0` and the reported position
        // defaults to the start.
        return Some(a);
    }

    let (mut start_count, mut end_count) = (0i32, 0i32);
    for &cj in &it.connected {
        let other = &graph.items[cj];
        let (hit_a, hit_b) = (item_hits_point(other, a, half_width), item_hits_point(other, b, half_width));

        if hit_a && hit_b {
            match &other.item {
                // Both ends land on the same track/arc: fall through to
                // the ordinary tie-break below.
                ItemRef::TrackSeg { .. } => {}
                // Both ends in the same zone: "may be redundant, but not
                // dangling" -- not just this endpoint, the whole segment.
                ItemRef::ZoneOutline { .. } => return None,
                // Both ends under the same pad/via: with
                // `aIgnoreTracksInPads == true`, not dangling.
                ItemRef::Pad { .. } | ItemRef::Via { .. } => return None,
            }
            if min_dist_to_track(other, a) < min_dist_to_track(other, b) {
                start_count += 1;
            } else {
                end_count += 1;
            }
        } else if hit_a {
            start_count += 1;
        } else if hit_b {
            end_count += 1;
        }

        if start_count > 0 && end_count > 0 {
            return None;
        }
    }

    Some(if start_count == 0 { a } else { b })
}

/// `TestTrackEndpointDangling`'s `PCB_VIA_T` branch: dangling when it
/// connects to nothing, or when every connection sits on the very same
/// (`layer_lo`) copper layer -- a via that never actually bridges layers.
fn via_dangling(graph: &ConnGraph, idx: usize) -> Option<Point> {
    let it = &graph.items[idx];
    let center = match &it.shape {
        ItemShape::Via { center, .. } => *center,
        _ => return None,
    };

    if it.connected.is_empty() {
        // "No connections AND no net is not an error."
        return if it.net.is_empty() { None } else { Some(center) };
    }

    let mut first_layer: Option<i32> = None;
    for &cj in &it.connected {
        let layer = graph.items[cj].layer_lo;
        match first_layer {
            None => first_layer = Some(layer),
            Some(fl) if layer != fl => return None,
            _ => {}
        }
    }
    Some(center)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::build_graph;
    use crate::tests_support::{pad_center, two_pad_model};
    use eda_model::ir::{Point, Track, Via};

    #[test]
    fn a_stub_track_with_one_free_end_is_dangling() {
        let (mut design, model) = two_pad_model();
        // Track from R1's pad out into empty space: one end lands on the
        // pad, the other dangles.
        let a = pad_center(&design, &model, "R1", "1");
        let free_end = Point { x: a.x, y: a.y + 10_000 };
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![a, free_end], arc_mid_offset: None });
        let graph = build_graph(&design, &model);
        let out = dangling_tracks_and_vias(&graph);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, DanglingKind::Track);
        assert_eq!(out[0].at, free_end);
    }

    #[test]
    fn a_fully_routed_track_has_no_dangling_ends() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![a, b], arc_mid_offset: None });
        let graph = build_graph(&design, &model);
        assert!(dangling_tracks_and_vias(&graph).is_empty());
    }

    #[test]
    fn a_via_touching_only_one_layer_of_copper_is_dangling() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let via_at = Point { x: a.x, y: a.y + 3_000 };
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![a, via_at], arc_mid_offset: None });
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: via_at, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        let graph = build_graph(&design, &model);
        let out = dangling_tracks_and_vias(&graph);
        let via_dangle = out.iter().find(|d| d.kind == DanglingKind::Via);
        assert!(via_dangle.is_some(), "a via touching copper on only one layer must be reported dangling");
    }

    #[test]
    fn a_via_bridging_two_layers_is_not_dangling() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        let via_at = Point { x: a.x, y: a.y + 3_000 };
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![a, via_at], arc_mid_offset: None });
        rt.tracks.push(Track { id: "t2".into(), net: "N1".into(), pins: vec![], layer: "B.Cu".into(), width: 200, pts: vec![via_at, b], arc_mid_offset: None });
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: via_at, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        let graph = build_graph(&design, &model);
        let out = dangling_tracks_and_vias(&graph);
        assert!(out.iter().all(|d| d.kind != DanglingKind::Via), "{:?}", out.iter().map(|d| &d.id).collect::<Vec<_>>());
    }
}
