//! `CONNECTIVITY_DATA::TestTrackEndpointDangling`
//! (`pcbnew/connectivity/connectivity_data.cpp`), the function
//! `DRC_TEST_PROVIDER_CONNECTIVITY::Run` calls to produce `DRCE_DANGLING_TRACK`
//! / `DRCE_DANGLING_VIA` (`pcbnew/drc/drc_test_provider_connectivity.cpp`,
//! which is what `kicad-cli pcb drc`'s `track_dangling`/`via_dangling`
//! come from) -- ported directly rather than via the more generic
//! `CN_ANCHOR::IsDangling`, since that DRC provider is the actual oracle
//! this crate is checked against, and it calls
//! `TestTrackEndpointDangling(track, /*aIgnoreTracksInPads=*/true, ...)`
//! specifically. That `true` is the default here; Cleanup Tracks & Vias
//! (`TRACKS_CLEANER::deleteDanglingTracks`) passes `false`
//! ([`dangling_tracks_and_vias_with`]).

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

/// Every dangling track/arc segment and via on the board, as DRC sees them (`aIgnoreTracksInPads = true`).
pub fn dangling_tracks_and_vias(graph: &ConnGraph) -> Vec<DanglingItem> {
    dangling_tracks_and_vias_with(graph, true)
}

/// [`dangling_tracks_and_vias`] with `TestTrackEndpointDangling`'s `aIgnoreTracksInPads`: `false` is the cleaner's call
/// (`tracks_cleaner.cpp::deleteDanglingTracks`, `TestTrackEndpointDangling( track, false )`), where a segment with both ends under
/// one pad or via counts that item once, at its nearer end, and so dangles when nothing else holds its other end.
pub fn dangling_tracks_and_vias_with(graph: &ConnGraph, ignore_tracks_in_pads: bool) -> Vec<DanglingItem> {
    let mut out = Vec::new();
    for idx in 0..graph.items.len() {
        let it = &graph.items[idx];
        match &it.item {
            ItemRef::TrackSeg { track_id, seg_index } => {
                if let Some(at) = track_seg_dangling(graph, idx, ignore_tracks_in_pads) {
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
/// `item->GetEffectiveShape(layer)->Collide(pos, accuracy)` here, with the integer
/// `accuracy` of [`track_seg_dangling`]. A track is a `SHAPE_SEGMENT`
/// (`SHAPE_SEGMENT::Collide( aP, .. )`: `min_dist = ( m_width + 1 ) / 2 + aClearance`, a hit when
/// `dist_sq == 0 || dist_sq < SEG::Square( min_dist )`), a via a `SHAPE_CIRCLE` (`SHAPE::Collide( VECTOR2I )`
/// is `Collide( SEG( aP, aP ) )`: `minDist = aClearance + Radius`, the same strict test), an arc track the true
/// `SHAPE_ARC` ([`arc_hits_point`]), not its tessellation; a pad keeps its own distance test.
fn item_hits_point(other: &crate::items::CnItem, p: Point, accuracy: i64) -> bool {
    let strict = |dist_sq: f64, min_dist: i64| dist_sq == 0.0 || dist_sq < (min_dist as f64) * (min_dist as f64);
    match &other.shape {
        ItemShape::Pad(pad) => pad.signed_distance(p) <= accuracy as f64,
        // `GetEffectiveShape()` of a via: `SHAPE_CIRCLE( m_Start, GetWidth( layer ) / 2 )`, an integer radius.
        ItemShape::Via { center, radius } => strict(geom::dist(p, *center).powi(2), (radius.floor() as i64) + accuracy),
        ItemShape::Segment { a, b, half_width } => {
            let width = (half_width * 2.0).round() as i64;
            strict(geom::point_seg_distance(p, *a, *b).powi(2), (width + 1) / 2 + accuracy)
        }
        ItemShape::Arc { pts, half_width } => arc_hits_point(pts, (half_width * 2.0).round() as i64, p, accuracy),
        // `rtree->QueryColliding( SHAPE_CIRCLE( p, accuracy ) )` on the fill.
        ItemShape::Zone { outline } => geom::zone_hits_point(outline, p, accuracy as f64),
    }
}

/// `SHAPE_ARC::Collide( aP, aClearance )` for the arc an arc track's tessellation `pts` was made from (its first, middle and last points lie
/// on the arc): `minDist = aClearance + m_width / 2`; the distance to the full circle (`KiROUND( nearestPt.Distance( aP ) )`, or for a point on
/// the circle `KiROUND( radius - sqrt( .. ) )`), unless the point's angle is outside the arc's sweep, where it is the distance to the nearer end
/// (`EuclideanNorm`, truncated to `int`); a hit when `dist <= minDist`.
fn arc_hits_point(pts: &[Point], width: i64, p: Point, accuracy: i64) -> bool {
    let min_dist = accuracy + width / 2;
    let (s, m, e) = (pts[0], pts[pts.len() / 2], pts[pts.len() - 1]);
    let f = |q: Point| (q.x.as_f64(), q.y.as_f64());
    let ((sx, sy), (mx, my), (ex, ey), (px, py)) = (f(s), f(m), f(e), f(p));
    let d = 2.0 * (sx * (my - ey) + mx * (ey - sy) + ex * (sy - my));
    if d.abs() < 1e-9 || pts.len() < 3 {
        // collinear: a straight piece, as `SHAPE_ARC` does for an arc too big to hold (`seg1.Distance( aP )`)
        return pts.windows(2).any(|w| geom::point_seg_distance(p, w[0], w[1]) <= min_dist as f64);
    }
    let (s2, m2, e2) = (sx * sx + sy * sy, mx * mx + my * my, ex * ex + ey * ey);
    let cx = (s2 * (my - ey) + m2 * (ey - sy) + e2 * (sy - my)) / d;
    let cy = (s2 * (ex - mx) + m2 * (sx - ex) + e2 * (mx - sx)) / d;
    let radius = ((sx - cx).powi(2) + (sy - cy).powi(2)).sqrt();
    let mut dist = (((px - cx).powi(2) + (py - cy).powi(2)).sqrt() - radius).abs().round() as i64;
    if s != e {
        // the sweep from the start through the middle to the end, in the y-down frame the points are in
        let ang = |x: f64, y: f64| (y - cy).atan2(x - cx).rem_euclid(std::f64::consts::TAU);
        let (a_s, a_m, a_e, a_p) = (ang(sx, sy), ang(mx, my), ang(ex, ey), ang(px, py));
        let rel = |a: f64| (a - a_s).rem_euclid(std::f64::consts::TAU);
        let forward = rel(a_m) <= rel(a_e);
        let inside = if forward { rel(a_p) <= rel(a_e) } else { rel(a_p) >= rel(a_e) || rel(a_p) == 0.0 };
        if !inside {
            let (ds, de) = (geom::dist(p, s) as i64, geom::dist(p, e) as i64);
            dist = if ds < de { ds } else { de };
        }
    }
    dist <= min_dist
}

/// `getMinDist` in `connectivity_data.cpp`: for a track or an arc `std::min( track->GetStart().Distance( aPoint ),
/// track->GetEnd().Distance( aPoint ) )` returned as an `int`, so truncated; for anything else (a pad or a via, reached only with
/// `aIgnoreTracksInPads == false`) `aItem->GetPosition().Distance( aPoint )`.
fn min_dist_to_track(other: &crate::items::CnItem, p: Point) -> i64 {
    match &other.shape {
        ItemShape::Segment { a, b, .. } => geom::dist(p, *a).min(geom::dist(p, *b)) as i64,
        // `PCB_ARC_T`: the distance to its start or its end.
        ItemShape::Arc { pts, .. } => geom::dist(p, pts[0]).min(geom::dist(p, pts[pts.len() - 1])) as i64,
        ItemShape::Pad(pad) => geom::dist(p, pad.center) as i64,
        ItemShape::Via { center, .. } => geom::dist(p, *center) as i64,
        _ => 0,
    }
}

/// `TestTrackEndpointDangling`'s `PCB_TRACE_T`/`PCB_ARC_T` branch (see module docs for `aIgnoreTracksInPads`). `graph.items[idx]`
/// must be a [`ItemRef::TrackSeg`].
fn track_seg_dangling(graph: &ConnGraph, idx: usize, ignore_tracks_in_pads: bool) -> Option<Point> {
    let it = &graph.items[idx];
    let (a, b, half_width) = match &it.shape {
        ItemShape::Segment { a, b, half_width } => (*a, *b, *half_width),
        ItemShape::Arc { pts, half_width } => (pts[0], pts[pts.len() - 1], *half_width),
        _ => return None,
    };

    // `int accuracy = KiROUND( aTrack->GetWidth() / 2.0 );`
    let accuracy = ((half_width * 2.0).round() / 2.0).round() as i64; // `f64::round` is half away from zero, as `KiROUND`

    if it.connected.is_empty() {
        // Nothing touches either end: KiCad's loop below never increments
        // either counter, so `start_count == 0` and the reported position
        // defaults to the start.
        return Some(a);
    }

    let (mut start_count, mut end_count) = (0i32, 0i32);
    for &cj in &it.connected {
        let other = &graph.items[cj];
        let (hit_a, hit_b) = (item_hits_point(other, a, accuracy), item_hits_point(other, b, accuracy));

        if hit_a && hit_b {
            match &other.item {
                // Both ends land on the same track/arc: fall through to
                // the ordinary tie-break below.
                ItemRef::TrackSeg { .. } => {}
                // Both ends in the same zone: "may be redundant, but not
                // dangling" -- not just this endpoint, the whole segment.
                ItemRef::ZoneOutline { .. } => return None,
                // Both ends under the same pad/via: with
                // `aIgnoreTracksInPads == true`, not dangling; with `false`, counted once at the nearer end.
                ItemRef::Pad { .. } | ItemRef::Via { .. } if ignore_tracks_in_pads => return None,
                ItemRef::Pad { .. } | ItemRef::Via { .. } => {}
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
    use eda_model::units::mm;
    use eda_model::units::Nm;
    use super::*;
    use crate::algo::build_graph;
    use crate::tests_support::{pad_center, two_pad_model};
    use eda_drc::zone_copper::ZoneCopper;
    use eda_model::ir::{Point, Track, Via};

    #[test]
    fn a_stub_track_with_one_free_end_is_dangling() {
        let (mut design, model) = two_pad_model();
        // Track from R1's pad out into empty space: one end lands on the
        // pad, the other dangles.
        let a = pad_center(&design, &model, "R1", "1");
        let free_end = Point { x: a.x, y: a.y + mm(10.0) };
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![a, free_end], arc_mid_offset: None });
        let graph = build_graph(&design, &model, &ZoneCopper::default());
        let out = dangling_tracks_and_vias(&graph);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, DanglingKind::Track);
        assert_eq!(out[0].at, free_end);
    }

    /// An arc is one `PCB_ARC`, one `CN_ITEM` with an anchor at each end (`CN_LIST::Add( PCB_ARC* )`), not the pieces of its tessellation: a
    /// tessellation whose rounding to nanometres puts two points on one (as the importer's of `issue22475` does) has no piece of nothing to dangle.
    #[test]
    fn an_arc_is_one_item_and_a_point_its_tessellation_repeats_dangles_nothing() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        // A 20 nm arc half way: 32 pieces of under a nanometre each, so its tessellation has repeated points.
        let p = Point { x: (a.x + b.x) / 2 - Nm::new(10), y: a.y };
        let q = Point { x: p.x + Nm::new(20), y: p.y };
        let mut arc = Track::new_arc("N1".into(), "F.Cu".into(), mm(0.05), p, Point { x: p.x + Nm::new(10), y: p.y - Nm::new(10) }, q);
        arc.id = "a1".into();
        assert!(arc.pts.windows(2).any(|w| w[0] == w[1]), "the tessellation repeats a point");
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(arc);
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.05), pts: vec![a, p], arc_mid_offset: None });
        rt.tracks.push(Track { id: "t2".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.05), pts: vec![q, b], arc_mid_offset: None });
        let graph = build_graph(&design, &model, &ZoneCopper::default());
        let arc_items = graph.items.iter().filter(|i| matches!(&i.item, ItemRef::TrackSeg { track_id, .. } if track_id == "a1")).count();
        assert_eq!(arc_items, 1, "one item for the arc");
        let out = dangling_tracks_and_vias(&graph);
        assert!(out.is_empty(), "{:?}", out.iter().map(|d| (&d.id, d.at)).collect::<Vec<_>>());
    }

    #[test]
    fn a_fully_routed_track_has_no_dangling_ends() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![a, b], arc_mid_offset: None });
        let graph = build_graph(&design, &model, &ZoneCopper::default());
        assert!(dangling_tracks_and_vias(&graph).is_empty());
    }

    #[test]
    fn a_via_touching_only_one_layer_of_copper_is_dangling() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let via_at = Point { x: a.x, y: a.y + mm(3.0) };
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![a, via_at], arc_mid_offset: None });
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: via_at, drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        let graph = build_graph(&design, &model, &ZoneCopper::default());
        let out = dangling_tracks_and_vias(&graph);
        let via_dangle = out.iter().find(|d| d.kind == DanglingKind::Via);
        assert!(via_dangle.is_some(), "a via touching copper on only one layer must be reported dangling");
    }

    #[test]
    fn a_via_bridging_two_layers_is_not_dangling() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        let via_at = Point { x: a.x, y: a.y + mm(3.0) };
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![a, via_at], arc_mid_offset: None });
        rt.tracks.push(Track { id: "t2".into(), net: "N1".into(), pins: vec![], layer: "B.Cu".into(), width: mm(0.2), pts: vec![via_at, b], arc_mid_offset: None });
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: via_at, drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        let graph = build_graph(&design, &model, &ZoneCopper::default());
        let out = dangling_tracks_and_vias(&graph);
        assert!(out.iter().all(|d| d.kind != DanglingKind::Via), "{:?}", out.iter().map(|d| &d.id).collect::<Vec<_>>());
    }

    fn track(id: &str, width: Nm, a: Point, b: Point) -> Track {
        Track { id: id.into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width, pts: vec![a, b], arc_mid_offset: None }
    }

    /// R-8 (round 2): `int accuracy = KiROUND( aTrack->GetWidth() / 2.0 )` and `SHAPE_SEGMENT::Collide`'s `( m_width + 1 ) / 2`: a 201 nm track
    /// that crosses a 200 nm one and ends 200.8 nm beyond its centre line has its start on it (101 + 100 > 200.8), where half widths of
    /// 100.5 + 100 did not reach.
    #[test]
    fn a_track_end_within_the_integer_accuracy_is_connected() {
        let (mut design, model) = two_pad_model();
        let o = Point { x: mm(50.0), y: mm(50.0) };
        let at = |dx: i64, dy: i64| Point { x: o.x + Nm::new(dx), y: o.y + Nm::new(dy) };
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(track("b", Nm::new(200), at(-10_000, -10_000), at(10_000, 10_000)));
        rt.tracks.push(track("a", Nm::new(201), at(142, -142), at(-3_000, 3_000)));
        rt.tracks.push(track("c", Nm::new(201), at(-3_000, 3_000), at(-3_000, 8_000)));
        let graph = build_graph(&design, &model, &ZoneCopper::default());
        let out = dangling_tracks_and_vias(&graph);
        assert!(out.iter().all(|d| d.id != "a"), "{:?}", out.iter().map(|d| (&d.id, d.at)).collect::<Vec<_>>());
    }

    /// R-8 (round 2): `getMinDist` returns an `int`, so 1000.42 and 1000.72 are both 1000 and the tie counts the end
    /// (`getMinDist( item, start ) < getMinDist( item, end )` is false): the short track inside a wide one dangles at its start.
    #[test]
    fn a_tie_after_truncation_counts_the_end() {
        let (mut design, model) = two_pad_model();
        let o = Point { x: mm(50.0), y: mm(50.0) };
        let at = |dx: i64, dy: i64| Point { x: o.x + Nm::new(dx), y: o.y + Nm::new(dy) };
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(track("wide", Nm::new(5_000), o, at(100_000, 0)));
        rt.tracks.push(track("short", Nm::new(200), at(1_000, 29), at(1_000, -38)));
        let graph = build_graph(&design, &model, &ZoneCopper::default());
        let out = dangling_tracks_and_vias(&graph);
        let short = out.iter().find(|d| d.id == "short").expect("the short track dangles");
        assert_eq!(short.at, at(1_000, 29), "KiCad reports the start");
    }

    /// R-8 (round 2): an arc is tested against the true arc (`SHAPE_ARC::Collide`), not the chords of its tessellation, whose sag lies inside it.
    #[test]
    fn an_arc_is_hit_on_its_true_curve() {
        let (mut design, model) = two_pad_model();
        let o = Point { x: mm(50.0), y: mm(50.0) };
        // a half circle of 10 mm radius about o, from o - 10 mm in x to o + 10 mm, through the top
        let (s, m, e) = (Point { x: o.x - mm(10.0), y: o.y }, Point { x: o.x, y: o.y - mm(10.0) }, Point { x: o.x + mm(10.0), y: o.y });
        let mut arc = Track::new_arc("N1".into(), "F.Cu".into(), Nm::new(200), s, m, e);
        arc.id = "arc".into();
        // a point on the true circle between two tessellation points, outside the chord there by the sag: take the mid angle of the first chord
        let (p0, p1) = (arc.pts[0], arc.pts[1]);
        let mid_ang = (((p0.y + p1.y) / 2 - o.y).as_f64()).atan2(((p0.x + p1.x) / 2 - o.x).as_f64());
        // 199 nm out from the centreline: within a 200 nm track's accuracy 100 + the arc's half width 100, not within it of the chord
        let r = mm(10.0).as_f64() + 199.0;
        let q = Point { x: o.x + Nm::kiround(r * mid_ang.cos()), y: o.y + Nm::kiround(r * mid_ang.sin()) };
        let sag = mm(10.0).as_f64() - ((((p0.x + p1.x) / 2 - o.x).as_f64()).powi(2) + (((p0.y + p1.y) / 2 - o.y).as_f64()).powi(2)).sqrt();
        assert!(sag > 2.0, "the tessellation sags inside the arc by {sag} nm");
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(arc);
        // the track crosses the arc inwards from q, so the two touch; its far end is on another track
        let far = Point { x: o.x + Nm::kiround(mm(5.0).as_f64() * mid_ang.cos()), y: o.y + Nm::kiround(mm(5.0).as_f64() * mid_ang.sin()) };
        rt.tracks.push(track("t", Nm::new(200), q, far));
        rt.tracks.push(track("u", Nm::new(200), far, Point { x: far.x, y: far.y - mm(20.0) }));
        let graph = build_graph(&design, &model, &ZoneCopper::default());
        let out = dangling_tracks_and_vias(&graph);
        assert!(out.iter().all(|d| d.id != "t"), "{:?}", out.iter().map(|d| (&d.id, d.at)).collect::<Vec<_>>());
    }
}
