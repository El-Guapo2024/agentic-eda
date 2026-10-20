//! Arc tracks as router items (`ARC_T`, `pns_arc.h`): the world holds an arc as one item with its hull and its collisions, a line
//! assembled through it holds its points and says which run of them is the arc, and what a route, a shove and a drag make of a line with an
//! arc in it is written back to the IR as segments and `(arc ..)` tracks (`docs/parity/CODE-COMPARE-router.md` D18).

use eda_drc::kimath::Shape;
use eda_model::ir::{Design, FootprintInstance, LabelSide, PlacementSection, Point, Provenance, RoutingSection, Side, Track, Um};
use eda_model::{ConstraintModel, Net as IrNet, Part, Pin, PinKind};
use eda_pns::arcs::ArcPts;
use eda_pns::item::{net_of, Arc, Item, Segment};
use eda_pns::line::LinePiece;
use eda_pns::node::Node;
use eda_pns::router::{Router, RouteCommit};
use eda_pns::settings::Mode;

fn p(x: Um, y: Um) -> Point {
    Point { x, y }
}

/// Even-odd point in polygon.
fn point_in_polygon(poly: &[Point], q: Point) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > q.y) != (b.y > q.y) && (q.x as f64) < (b.x - a.x) as f64 * (q.y - a.y) as f64 / (b.y - a.y) as f64 + a.x as f64 {
            inside = !inside;
        }
    }
    inside
}

fn rules() -> eda_model::BoardRules {
    serde_yaml::from_str("track_width: 200\nclearance: 200\nvia_drill: 300\nvia_diameter: 600\n").unwrap()
}

/// A quarter circle of radius 4 mm about (0, 0), from (4000, 0) to (0, 4000).
fn quarter() -> ArcPts {
    let h = (4000.0 * std::f64::consts::FRAC_1_SQRT_2).round() as Um;
    ArcPts::new(p(4000, 0), p(h, h), p(0, 4000))
}

fn seg_item(net: &str, a: Point, b: Point, track: &str) -> Item {
    Item::Segment(Segment { net: net_of(net), layer: 0, a, b, width: 200, source_track: Some((track.into(), 0)), locked: false })
}

fn arc_item(net: &str, arc: ArcPts, track: &str) -> Item {
    Item::Arc(Arc::new(net_of(net), 0, 200, arc, Some(track.into()), false))
}

/// segment -- arc -- segment of one net: (4000, -3000) up to the arc's start, the quarter circle, then out to (-3000, 4000).
fn s_bend(node: &mut Node, net: &str) -> [u64; 3] {
    let a = quarter();
    let s1 = node.add(seg_item(net, p(4000, -3000), a.start, "t_in"));
    let ar = node.add(arc_item(net, a, "t_arc"));
    let s2 = node.add(seg_item(net, a.end, p(-3000, 4000), "t_out"));
    [s1, ar, s2]
}

#[test]
fn a_line_through_an_arc_holds_its_points_and_knows_which_run_is_the_arc() {
    let mut node = Node::new();
    let [s1, ar, s2] = s_bend(&mut node, "SIG");
    let a = quarter();
    let poly = a.polyline();
    for start in [s1, ar, s2] {
        let line = node.assemble_line(start).expect("a track item assembles");
        assert_eq!(line.pts.len(), poly.len() + 2, "the arc's {} points and the two outer ends", poly.len());
        assert_eq!((line.first(), line.last()), (Some(p(4000, -3000)), Some(p(-3000, 4000))));
        assert_eq!(line.arc_count(), 1);
        let run = line.arc_runs()[0];
        assert_eq!((run.first, run.last), (1, poly.len()));
        assert_eq!(&line.pts[run.first..=run.last], poly.as_slice());
        assert_eq!(line.segment_ids.len(), line.segment_count(), "one id per segment of the line");
        assert_eq!(line.segment_ids.first(), Some(&s1));
        assert_eq!(line.segment_ids.last(), Some(&s2));
        assert!(line.segment_ids[1..=poly.len() - 1].iter().all(|&id| id == ar), "the arc's id stands for each of its chords");
        assert!(line.is_arc_segment(1) && !line.is_arc_segment(0) && !line.is_arc_segment(poly.len()));
        assert!(line.is_pt_on_arc(1) && line.is_pt_on_arc(poly.len()) && !line.is_pt_on_arc(0));
    }
}

#[test]
fn walking_the_line_the_other_way_finds_the_same_arc() {
    let mut node = Node::new();
    let [_, _, s2] = s_bend(&mut node, "SIG");
    let mut line = node.assemble_line(s2).unwrap();
    line.reverse();
    assert_eq!(line.arc_count(), 1);
    let pieces = line.pieces();
    assert_eq!(pieces.len(), 3);
    assert!(matches!(&pieces[1], LinePiece::Arc(a) if a.start == quarter().end && a.end == quarter().start), "{pieces:?}");
}

#[test]
fn a_line_with_an_arc_goes_back_into_the_node_as_the_items_it_was_made_of() {
    let mut node = Node::new();
    let [s1, _, _] = s_bend(&mut node, "SIG");
    let line = node.assemble_line(s1).unwrap();
    let mut other = Node::new();
    let ids = other.add_line(&line, Some(("t".into(), 0)), false);
    assert_eq!(ids.len(), line.segment_count());
    assert_eq!(other.len(), 3, "two segments and one arc, not a segment per chord");
    assert_eq!(other.iter().filter(|(_, it)| matches!(it, Item::Arc(_))).count(), 1);
    let again = other.assemble_line(ids[0]).unwrap();
    assert_eq!(again.pts, line.pts);
    assert_eq!(again.arc_count(), 1);
}

#[test]
fn an_edit_that_cuts_into_an_arc_leaves_segments_and_one_that_does_not_leaves_the_arc() {
    let mut node = Node::new();
    let [s1, _, _] = s_bend(&mut node, "SIG");
    let line = node.assemble_line(s1).unwrap();

    // a vertex of the arc moved: the arc is gone, the points stay
    let mut cut = line.clone();
    cut.pts[5].x += 3;
    assert_eq!(cut.arc_count(), 0);
    // a vertex inserted in the middle of one of its chords
    let mut inserted = line.clone();
    let mid = p((inserted.pts[5].x + inserted.pts[6].x) / 2 + 1, (inserted.pts[5].y + inserted.pts[6].y) / 2);
    inserted.pts.insert(6, mid);
    assert_eq!(inserted.arc_count(), 0);
    // the straight end moved: the arc is still the arc
    let mut moved = line.clone();
    moved.pts[0] = p(4000, -5000);
    assert_eq!(moved.arc_count(), 1);
    // a vertex before the arc: the run moves with the points
    let mut longer = line.clone();
    longer.pts.insert(0, p(4000, -6000));
    assert_eq!(longer.arc_count(), 1);
    assert_eq!(longer.arc_runs()[0].first, 2);
    // simplify never drops a point of an arc, however flat the arc is there
    let mut simplified = line.clone();
    simplified.simplify();
    assert_eq!(simplified.pts, line.pts);
}

#[test]
fn an_arc_collides_with_a_track_across_it_and_not_with_one_beyond_the_clearance() {
    let mut node = Node::new();
    node.add(arc_item("GND", quarter(), "t_arc"));
    let rules = rules();
    // a SIG track that crosses the arc
    let across = Shape::Stadium { a: p(3000, 3000), b: p(1000, 1000), r: 100 };
    assert!(!node.all_colliding(&across, &net_of("SIG"), eda_pns::layer::LayerRange::single(0), &rules, &[]).is_empty());
    // a SIG track 4 mm - 100 - 100 - 200 = 3600 from the centre, at the clearance of the arc's near edge: the arc's stroke is 3900..4100
    let radius_at = |r: f64| {
        let h = (r * std::f64::consts::FRAC_1_SQRT_2).round() as Um;
        Shape::Circle { c: p(h, h), r: 100 }
    };
    // 3900 - 200 (clearance) - 100 (the circle) = 3600: touching the clearance is not a collision, 30 um inside it is
    assert!(node.all_colliding(&radius_at(3630.0), &net_of("SIG"), eda_pns::layer::LayerRange::single(0), &rules, &[]).len() == 1);
    assert!(node.all_colliding(&radius_at(3595.0), &net_of("SIG"), eda_pns::layer::LayerRange::single(0), &rules, &[]).is_empty());
    // the same net never collides
    assert!(node.all_colliding(&across, &net_of("GND"), eda_pns::layer::LayerRange::single(0), &rules, &[]).is_empty());
}

#[test]
fn the_hull_of_an_arc_item_is_the_ring_round_the_arc() {
    let a = arc_item("GND", quarter(), "t_arc");
    let hull = a.hull(200, 200, 0);
    assert!(hull.len() > 20, "a ring of the arc's points, not an octagon: {}", hull.len());
    let inside = |q: Point| point_in_polygon(&hull, q);
    assert!(inside(p(2828, 2828)), "on the arc");
    assert!(inside(p(2700, 2700)), "inside the clearance");
    assert!(!inside(p(0, 0)), "the centre of the circle");
}

#[test]
fn splitting_an_arc_joins_its_halves_at_the_point_and_leaves_the_track_whole_for_the_rest() {
    let mut node = Node::new();
    let [_, ar, _] = s_bend(&mut node, "SIG");
    let a = quarter();
    let at = a.nearest_point(p(3000, 3000));
    let (x, y) = node.split_arc(ar, at).expect("a point inside the arc splits it");
    assert!(node.get(ar).is_none());
    let (Some(Item::Arc(first)), Some(Item::Arc(second))) = (node.get(x), node.get(y)) else { panic!("two arcs") };
    assert_eq!((first.start(), first.end()), (a.start, at));
    assert_eq!((second.start(), second.end()), (at, a.end));
    assert_eq!(first.source_track, Some("t_arc".into()));
    // the joint is there now: nothing to split at the same point again
    assert!(node.arc_to_split(x, at).is_none());
    // the whole line still assembles through the two halves
    let line = node.assemble_line(x).unwrap();
    assert_eq!(line.arc_count(), 2);
    assert_eq!(line.first(), Some(p(4000, -3000)));
}

// ------------------------------------------------------------------------------------------------------------------------------
// through the router
// ------------------------------------------------------------------------------------------------------------------------------

fn part(r: &str) -> Part {
    Part { reference: r.into(), mpn: None, lcsc: None, value: None, package: Some("0603".into()), footprint: Some("0603".into()), pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }], body_um: None, symbol: None, datasheet: None, edge: None }
}

/// Two 0603 resistors, R1 at (-9000, 0) and R2 at (9000, 0), net SIG on R1.1 and R2.1, and a GND arc track across the middle of the board.
fn board_with(tracks: Vec<Track>) -> (Design, ConstraintModel) {
    board_with_pads_at(tracks, 0)
}

/// [`board_with`], the two resistors `y` below the origin.
fn board_with_pads_at(tracks: Vec<Track>, y: Um) -> (Design, ConstraintModel) {
    let model = ConstraintModel { parts: vec![part("R1"), part("R2")], nets: vec![IrNet { name: "SIG".into(), pins: vec!["R1.1".into(), "R2.1".into()] }], ..Default::default() };
    let design = Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: 1,
        provenance: Provenance { engine_version: "test".into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(PlacementSection {
            outline: vec![],
            footprints: vec![FootprintInstance { id: "R1".into(), at: p(-9000, y), rot: 0, side: Side::Top, label: LabelSide::Above }, FootprintInstance { id: "R2".into(), at: p(9000, y), rot: 0, side: Side::Top, label: LabelSide::Above }],
            modules: vec![],
        }),
        routing: Some(RoutingSection { tracks, vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }),
        drawings: None,
    };
    (design, model)
}

fn arc_track(id: &str, net: &str, a: ArcPts) -> Track {
    let mut t = Track::new_arc(net.into(), "F.Cu".into(), 200, a.start, a.mid, a.end);
    t.id = id.into();
    t
}

fn straight(id: &str, net: &str, pts: &[Point]) -> Track {
    Track { id: id.into(), net: net.into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: pts.to_vec(), arc_mid_offset: None }
}

/// A quarter arc of radius 3 mm bulging toward +x +y: centre (-1000, -1000), from (2000, -1000) to (-1000, 2000).
fn gnd_arc() -> ArcPts {
    let r = 3000.0;
    let h = (r * std::f64::consts::FRAC_1_SQRT_2).round() as Um;
    ArcPts::new(p(2000, -1000), p(-1000 + h, -1000 + h), p(-1000, 2000))
}

fn tracks_of(commit: &RouteCommit) -> (Vec<&Track>, Vec<&Track>) {
    (commit.tracks.iter().filter(|t| t.arc().is_some()).collect(), commit.tracks.iter().filter(|t| t.arc().is_none()).collect())
}

#[test]
fn the_world_holds_an_arc_track_as_one_arc_item() {
    let (design, model) = board_with(vec![arc_track("trk_arc", "GND", gnd_arc())]);
    let (node, _) = eda_pns::from_ir::build_node(&design, &model);
    let arcs: Vec<&Arc> = node.iter().filter_map(|(_, it)| if let Item::Arc(a) = it { Some(a) } else { None }).collect();
    assert_eq!(arcs.len(), 1, "one item for the arc, not 32 chords");
    assert_eq!((arcs[0].start(), arcs[0].end(), arcs[0].width), (gnd_arc().start, gnd_arc().end, 200));
    assert_eq!(arcs[0].source_track.as_deref(), Some("trk_arc"));
    assert!(!node.iter().any(|(_, it)| matches!(it, Item::Segment(s) if s.net.as_deref() == Some("GND"))));
}

#[test]
fn a_route_started_in_the_middle_of_an_arc_track_cuts_it_there() {
    let arc = ArcPts::new(p(2000, -1000), gnd_arc().mid, p(-1000, 2000));
    let (design, model) = board_with(vec![arc_track("trk_arc", "SIG", arc)]);
    let mut router = Router::new(&design, &model);
    let on = arc.nearest_point(p(1500, 1500));
    router.start(on, "F.Cu", 200).expect("an arc track of the net can start a route");
    // head for R2.1 (the pad at x = 9000 - 825)
    let target = p(9000 - 825, 0);
    let commit = router.finish(target).expect("the route commits");
    let (arcs, others) = tracks_of(&commit);
    assert_eq!(commit.remove_track_ids, vec!["trk_arc".to_string()], "the arc track is replaced");
    assert_eq!(arcs.len(), 2, "by two arc tracks that meet where the route starts");
    let ends: Vec<_> = arcs.iter().map(|t| t.arc().unwrap()).collect();
    assert!(ends.iter().any(|a| a.2 == on) && ends.iter().any(|a| a.0 == on));
    assert!(others.iter().any(|t| t.pts.first() == Some(&on) || t.pts.last() == Some(&on)), "the route starts where the arc was cut: {others:?}");
}

#[test]
fn a_route_that_ends_on_an_arc_track_of_its_net_cuts_it_there() {
    let arc = ArcPts::new(p(2000, 1000), p(-1000 + 2121, 1000 - 2121 + 2000), p(-1000, 2000));
    let _ = arc;
    let a = gnd_arc();
    let (design, model) = board_with(vec![arc_track("trk_arc", "SIG", a)]);
    let mut router = Router::new(&design, &model);
    router.start(p(-9000 - 825, 0), "F.Cu", 200).expect("starts on R1.1");
    let on = a.nearest_point(p(300, 1800));
    let preview = router.preview(on).expect("a route is in progress");
    assert_eq!(preview.snapped_end, Some(on), "the end snaps onto the arc");
    let commit = router.finish(on).expect("the route commits");
    let (arcs, _) = tracks_of(&commit);
    assert_eq!(arcs.len(), 2, "the arc is cut where the route ends");
    assert!(commit.remove_track_ids.contains(&"trk_arc".to_string()));
}

#[test]
fn a_route_in_walk_around_mode_goes_round_an_arc_it_would_cross() {
    let (design, model) = board_with(vec![arc_track("trk_arc", "GND", gnd_arc())]);
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::Walkaround;
    router.start(p(-9000 - 825, 0), "F.Cu", 200).expect("starts on R1.1");
    // straight at R2.1: the line y = 0 crosses the arc (x from 1000 up, near y = 0)
    let target = p(9000 - 825, 0);
    let preview = router.preview(target).expect("in progress");
    assert!(!preview.colliding, "walked round the arc");
    let rules = rules();
    let mut node = eda_pns::from_ir::build_node(&design, &model).0;
    let line = &preview.head;
    for (a, b) in line.segs() {
        let shape = Shape::Stadium { a, b, r: 100 };
        assert!(node.all_colliding(&shape, &net_of("SIG"), eda_pns::layer::LayerRange::single(0), &model.board, &[]).iter().all(|o| !matches!(node.get(o.id), Some(Item::Arc(_)))), "clear of the arc");
    }
    node.add_line(line, None, false);
    let _ = rules;
}

/// The commit applied to the board: every pair of its tracks and the board's own that the router's world calls a collision.
fn collisions_after(design: &Design, model: &ConstraintModel, commit: &RouteCommit) -> usize {
    let mut after = design.clone();
    let rt = after.routing.as_mut().unwrap();
    rt.tracks.retain(|t| !commit.remove_track_ids.contains(&t.id));
    rt.tracks.extend(commit.tracks.iter().cloned());
    let (world, _) = eda_pns::from_ir::build_node(&after, model);
    let mut bad = 0;
    for (id, item) in world.iter() {
        if matches!(item, Item::Solid(_)) {
            continue;
        }
        bad += world.all_colliding(&item.shape(item.layers().start()), item.net(), item.layers(), &model.board, &[id]).len();
    }
    bad
}

/// A GND arc that dips to y = -100 between two stubs, so the SIG head along y = 0 runs into its dip: centre (0, 5000), radius 5100, from (-3000, 876)
/// to (3000, 876).
fn dip() -> ArcPts {
    ArcPts::new(p(-3000, 876), p(0, -100), p(3000, 876))
}

#[test]
fn a_route_in_shove_mode_pushes_an_arc_track_out_of_its_way_and_the_commit_writes_the_rest_back() {
    let a = dip();
    let tracks = vec![straight("t_l", "GND", &[p(-3000, 3000), a.start]), arc_track("trk_arc", "GND", a), straight("t_r", "GND", &[a.end, p(3000, 3000)]), straight("t_sig", "SIG", &[p(-9000, 0), p(-8000, 0)])];
    let (design, model) = board_with_pads_at(tracks, 20_000);
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::Shove;
    router.start(p(-9000, 0), "F.Cu", 200).expect("starts on the end of the SIG stub");
    let target = p(9000, 0);
    let preview = router.preview(target).expect("in progress");
    assert!(!preview.colliding, "the shove resolves it");
    assert!(!preview.displaced_lines.is_empty(), "the arc's line is pushed, not walked round: {:?}", preview.head.pts.len());
    let commit = router.finish(target).expect("the shoved route commits");
    for t in ["t_l", "trk_arc", "t_r"] {
        assert!(commit.remove_track_ids.contains(&t.to_string()), "{t} is replaced, the line stood on all three: {:?}", commit.remove_track_ids);
    }
    assert_eq!(collisions_after(&design, &model, &commit), 0, "the pushed line clears the route: {:#?}", commit.tracks.iter().map(|t| (&t.net, t.pts.len(), t.arc().is_some())).collect::<Vec<_>>());
    // the pushed dip is segments now; the sides of the arc the shove did not reach are not asked to stay arcs, but nothing of GND is lost
    assert!(commit.tracks.iter().any(|t| t.net == "GND"));
}

#[test]
fn a_shove_that_walks_the_straight_part_of_a_line_round_a_pusher_leaves_its_arc_an_arc() {
    // GND: a long straight run along y = 0, a quarter arc turning away at its end, a run on. The SIG pusher is a short vertical stroke
    // that ends under the run, far from the arc: the run is walked round the pusher's end, the arc is not reached.
    let arc = ArcPts::new(p(2000, 0), p(2000 + 1414, 2000 - 1414), p(4000, 2000));
    let mut node = Node::new();
    node.add(seg_item("GND", p(-8000, 0), arc.start, "t_run"));
    node.add(arc_item("GND", arc, "t_arc"));
    node.add(seg_item("GND", arc.end, p(4000, 6000), "t_out"));
    let line = node.assemble_line(node.iter().find(|(_, it)| matches!(it, Item::Arc(_))).unwrap().0).unwrap();
    assert_eq!(line.arc_count(), 1);
    let pusher = eda_pns::line::Line::from_points(net_of("SIG"), 0, 200, vec![p(-5000, -2000), p(-5000, 300)]);
    let shoved = eda_pns::shove::shove_obstacle_line(&node, &rules(), &eda_pns::settings::RoutingSettings::default(), &pusher, None, &line, None, None).expect("the run can be walked round the pusher's end");
    assert_ne!(shoved.pts, line.pts, "something moved");
    assert_eq!(shoved.arc_count(), 1, "the arc did not: {:?}", shoved.pts.len());
    let run = shoved.arc_runs()[0];
    assert_eq!(&shoved.pts[run.first..=run.last], arc.polyline().as_slice(), "its points are the ones it had");
    // and the pieces the IR gets: segments, the very arc, segments
    let pieces = shoved.pieces();
    assert!(matches!(pieces.as_slice(), [LinePiece::Poly(_), LinePiece::Arc(a), LinePiece::Poly(_)] if *a == arc), "{pieces:?}");
}

#[test]
fn a_line_that_is_cut_across_an_arc_is_written_back_as_segments_and_one_that_is_not_has_its_arc_as_an_arc_track() {
    let mut node = Node::new();
    let [s1, _, _] = s_bend(&mut node, "SIG");
    let line = node.assemble_line(s1).unwrap();
    let tracks = eda_pns::router::line_to_tracks(&line, "F.Cu", None);
    assert_eq!(tracks.len(), 3);
    assert_eq!(tracks[0].pts, vec![p(4000, -3000), quarter().start]);
    assert_eq!(tracks[1].arc(), Some((quarter().start, quarter().mid, quarter().end)));
    assert_eq!(tracks[2].pts, vec![quarter().end, p(-3000, 4000)]);
    assert!(tracks.iter().all(|t| t.net == "SIG" && t.layer == "F.Cu" && t.width == 200));
    // the arc cut: one polyline
    let mut cut = line.clone();
    cut.pts[5].x += 3;
    let tracks = eda_pns::router::line_to_tracks(&cut, "F.Cu", None);
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].pts, cut.pts);
}
