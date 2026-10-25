//! Arc tracks as router items (`ARC_T`, `pns_arc.h`): the world holds an arc as one item with its hull and its collisions, a line
//! assembled through it holds its points and says which run of them is the arc, and what a route, a shove and a drag make of a line with an
//! arc in it is written back to the IR as segments and `(arc ..)` tracks (`docs/parity/CODE-COMPARE-router.md` D18).

use eda_model::units::mm;
use eda_drc::kimath::Shape;
use eda_model::ir::{Design, FootprintInstance, LabelSide, PlacementSection, Point, Provenance, RoutingSection, Side, Track, Nm};
use eda_model::{ConstraintModel, Net as IrNet, Part, Pin, PinKind};
use eda_pns::arcs::ArcPts;
use eda_pns::item::{net_of, Arc, Item, Segment};
use eda_pns::line::LinePiece;
use eda_pns::node::Node;
use eda_pns::router::{Router, RouteCommit};
use eda_pns::settings::Mode;

fn p(x: Nm, y: Nm) -> Point {
    Point { x, y }
}

/// Even-odd point in polygon.
fn point_in_polygon(poly: &[Point], q: Point) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > q.y) != (b.y > q.y) && (q.x.raw_shim() as f64) < (b.x - a.x).raw_shim() as f64 * (q.y - a.y).raw_shim() as f64 / (b.y - a.y).raw_shim() as f64 + a.x.raw_shim() as f64 {
            inside = !inside;
        }
    }
    inside
}

fn rules() -> eda_model::BoardRules {
    serde_yaml::from_str("track_width: 200000\nclearance: 200000\nvia_drill: 300000\nvia_diameter: 600000\n").unwrap()
}

/// A quarter circle of radius 4 mm about (0, 0), from (4000, 0) to (0, 4000).
fn quarter() -> ArcPts {
    let h = Nm::from_raw_shim((4_000_000.0 * std::f64::consts::FRAC_1_SQRT_2).round() as i64);
    ArcPts::new(p(mm(4.0), Nm::ZERO), p(h, h), p(Nm::ZERO, mm(4.0)))
}

fn seg_item(net: &str, a: Point, b: Point, track: &str) -> Item {
    Item::Segment(Segment { net: net_of(net), layer: 0, a, b, width: mm(0.2), source_track: Some((track.into(), 0)), locked: false })
}

fn arc_item(net: &str, arc: ArcPts, track: &str) -> Item {
    Item::Arc(Arc::new(net_of(net), 0, mm(0.2), arc, Some(track.into()), false))
}

/// segment -- arc -- segment of one net: (4000, -3000) up to the arc's start, the quarter circle, then out to (-3000, 4000).
fn s_bend(node: &mut Node, net: &str) -> [u64; 3] {
    let a = quarter();
    let s1 = node.add(seg_item(net, p(mm(4.0), mm(-3.0)), a.start, "t_in"));
    let ar = node.add(arc_item(net, a, "t_arc"));
    let s2 = node.add(seg_item(net, a.end, p(mm(-3.0), mm(4.0)), "t_out"));
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
        assert_eq!((line.first(), line.last()), (Some(p(mm(4.0), mm(-3.0))), Some(p(mm(-3.0), mm(4.0)))));
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
    cut.pts[5].x += Nm::new(3000);
    assert_eq!(cut.arc_count(), 0);
    // a vertex inserted in the middle of one of its chords
    let mut inserted = line.clone();
    let mid = p((inserted.pts[5].x + inserted.pts[6].x) / 2 + Nm::new(1), (inserted.pts[5].y + inserted.pts[6].y) / 2);
    inserted.pts.insert(6, mid);
    assert_eq!(inserted.arc_count(), 0);
    // the straight end moved: the arc is still the arc
    let mut moved = line.clone();
    moved.pts[0] = p(mm(4.0), mm(-5.0));
    assert_eq!(moved.arc_count(), 1);
    // a vertex before the arc: the run moves with the points
    let mut longer = line.clone();
    longer.pts.insert(0, p(mm(4.0), mm(-6.0)));
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
    let across = Shape::Stadium { a: p(mm(3.0), mm(3.0)), b: p(mm(1.0), mm(1.0)), r: mm(0.1) };
    assert!(!node.all_colliding(&across, &net_of("SIG"), eda_pns::layer::LayerRange::single(0), &rules, &[]).is_empty());
    // a SIG track 4 mm - 100 - 100 - 200 = 3600 from the centre, at the clearance of the arc's near edge: the arc's stroke is 3900..4100
    let radius_at = |r: f64| {
        let h = Nm::from_raw_shim((r * std::f64::consts::FRAC_1_SQRT_2).round() as i64);
        Shape::Circle { c: p(h, h), r: mm(0.1) }
    };
    // 3900 - 200 (clearance) - 100 (the circle) = 3600: touching the clearance is not a collision, 30 um inside it is
    assert!(node.all_colliding(&radius_at(3_630_000.0), &net_of("SIG"), eda_pns::layer::LayerRange::single(0), &rules, &[]).len() == 1);
    assert!(node.all_colliding(&radius_at(3_595_000.0), &net_of("SIG"), eda_pns::layer::LayerRange::single(0), &rules, &[]).is_empty());
    // the same net never collides
    assert!(node.all_colliding(&across, &net_of("GND"), eda_pns::layer::LayerRange::single(0), &rules, &[]).is_empty());
}

#[test]
fn the_hull_of_an_arc_item_is_the_ring_round_the_arc() {
    let a = arc_item("GND", quarter(), "t_arc");
    let hull = a.hull(mm(0.2), mm(0.2), 0);
    assert!(hull.len() > 20, "a ring of the arc's points, not an octagon: {}", hull.len());
    let inside = |q: Point| point_in_polygon(&hull, q);
    assert!(inside(p(mm(2.828), mm(2.828))), "on the arc");
    assert!(inside(p(mm(2.7), mm(2.7))), "inside the clearance");
    assert!(!inside(p(Nm::ZERO, Nm::ZERO)), "the centre of the circle");
}

#[test]
fn splitting_an_arc_joins_its_halves_at_the_point_and_leaves_the_track_whole_for_the_rest() {
    let mut node = Node::new();
    let [_, ar, _] = s_bend(&mut node, "SIG");
    let a = quarter();
    let at = a.nearest_point(p(mm(3.0), mm(3.0)));
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
    assert_eq!(line.first(), Some(p(mm(4.0), mm(-3.0))));
}

// ------------------------------------------------------------------------------------------------------------------------------
// through the router
// ------------------------------------------------------------------------------------------------------------------------------

fn part(r: &str) -> Part {
    Part { reference: r.into(), mpn: None, lcsc: None, value: None, package: Some("0603".into()), footprint: Some("0603".into()), pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }], body_nm: None, symbol: None, datasheet: None, edge: None }
}

/// Two 0603 resistors, R1 at (-9000, 0) and R2 at (9000, 0), net SIG on R1.1 and R2.1, and a GND arc track across the middle of the board.
fn board_with(tracks: Vec<Track>) -> (Design, ConstraintModel) {
    board_with_pads_at(tracks, Nm::ZERO)
}

/// [`board_with`], the two resistors `y` below the origin.
fn board_with_pads_at(tracks: Vec<Track>, y: Nm) -> (Design, ConstraintModel) {
    let model = ConstraintModel { parts: vec![part("R1"), part("R2")], nets: vec![IrNet { name: "SIG".into(), pins: vec!["R1.1".into(), "R2.1".into()] }], ..Default::default() };
    let design = Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "test".into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(PlacementSection {
            outline: vec![],
            footprints: vec![FootprintInstance { id: "R1".into(), at: p(mm(-9.0), y), rot: 0, side: Side::Top, label: LabelSide::Above }, FootprintInstance { id: "R2".into(), at: p(mm(9.0), y), rot: 0, side: Side::Top, label: LabelSide::Above }],
            modules: vec![],
        }),
        routing: Some(RoutingSection { tracks, vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }),
        drawings: None,
    };
    (design, model)
}

fn arc_track(id: &str, net: &str, a: ArcPts) -> Track {
    let mut t = Track::new_arc(net.into(), "F.Cu".into(), mm(0.2), a.start, a.mid, a.end);
    t.id = id.into();
    t
}

fn straight(id: &str, net: &str, pts: &[Point]) -> Track {
    Track { id: id.into(), net: net.into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: pts.to_vec(), arc_mid_offset: None }
}

/// A quarter arc of radius 3 mm bulging toward +x +y: centre (-1000, -1000), from (2000, -1000) to (-1000, 2000).
fn gnd_arc() -> ArcPts {
    let r = 3_000_000.0;
    let h = Nm::from_raw_shim((r * std::f64::consts::FRAC_1_SQRT_2).round() as i64);
    ArcPts::new(p(mm(2.0), mm(-1.0)), p(mm(-1.0) + h, mm(-1.0) + h), p(mm(-1.0), mm(2.0)))
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
    assert_eq!((arcs[0].start(), arcs[0].end(), arcs[0].width), (gnd_arc().start, gnd_arc().end, mm(0.2)));
    assert_eq!(arcs[0].source_track.as_deref(), Some("trk_arc"));
    assert!(!node.iter().any(|(_, it)| matches!(it, Item::Segment(s) if s.net.as_deref() == Some("GND"))));
}

#[test]
fn a_route_started_in_the_middle_of_an_arc_track_cuts_it_there() {
    let arc = ArcPts::new(p(mm(2.0), mm(-1.0)), gnd_arc().mid, p(mm(-1.0), mm(2.0)));
    let (design, model) = board_with(vec![arc_track("trk_arc", "SIG", arc)]);
    let mut router = Router::new(&design, &model);
    let on = arc.nearest_point(p(mm(1.5), mm(1.5)));
    router.start(on, "F.Cu", mm(0.2)).expect("an arc track of the net can start a route");
    // head for R2.1 (the pad at x = 9000 - 825)
    let target = p(Nm::from_raw_shim(9_000_000 - 825_000), Nm::ZERO);
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
    let arc = ArcPts::new(p(mm(2.0), mm(1.0)), p(Nm::from_raw_shim(-1_000_000 + 2_121_000), Nm::from_raw_shim(1_000_000 - 2_121_000 + 2_000_000)), p(mm(-1.0), mm(2.0)));
    let _ = arc;
    let a = gnd_arc();
    let (design, model) = board_with(vec![arc_track("trk_arc", "SIG", a)]);
    let mut router = Router::new(&design, &model);
    router.start(p(Nm::from_raw_shim(-9_000_000 - 825_000), Nm::ZERO), "F.Cu", mm(0.2)).expect("starts on R1.1");
    let on = a.nearest_point(p(mm(0.3), mm(1.8)));
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
    router.start(p(Nm::from_raw_shim(-9_000_000 - 825_000), Nm::ZERO), "F.Cu", mm(0.2)).expect("starts on R1.1");
    // straight at R2.1: the line y = 0 crosses the arc (x from 1000 up, near y = 0)
    let target = p(Nm::from_raw_shim(9_000_000 - 825_000), Nm::ZERO);
    let preview = router.preview(target).expect("in progress");
    assert!(!preview.colliding, "walked round the arc");
    let rules = rules();
    let mut node = eda_pns::from_ir::build_node(&design, &model).0;
    let line = &preview.head;
    for (a, b) in line.segs() {
        let shape = Shape::Stadium { a, b, r: mm(0.1) };
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
    ArcPts::new(p(mm(-3.0), mm(0.876)), p(Nm::ZERO, mm(-0.1)), p(mm(3.0), mm(0.876)))
}

#[test]
fn a_route_in_shove_mode_pushes_an_arc_track_out_of_its_way_and_the_commit_writes_the_rest_back() {
    let a = dip();
    let tracks = vec![straight("t_l", "GND", &[p(mm(-3.0), mm(3.0)), a.start]), arc_track("trk_arc", "GND", a), straight("t_r", "GND", &[a.end, p(mm(3.0), mm(3.0))]), straight("t_sig", "SIG", &[p(mm(-9.0), Nm::ZERO), p(mm(-8.0), Nm::ZERO)])];
    let (design, model) = board_with_pads_at(tracks, mm(20.0));
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::Shove;
    router.start(p(mm(-9.0), Nm::ZERO), "F.Cu", mm(0.2)).expect("starts on the end of the SIG stub");
    let target = p(mm(9.0), Nm::ZERO);
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
    let arc = ArcPts::new(p(mm(2.0), Nm::ZERO), p(Nm::from_raw_shim(2_000_000 + 1_414_000), Nm::from_raw_shim(2_000_000 - 1_414_000)), p(mm(4.0), mm(2.0)));
    let mut node = Node::new();
    node.add(seg_item("GND", p(mm(-8.0), Nm::ZERO), arc.start, "t_run"));
    node.add(arc_item("GND", arc, "t_arc"));
    node.add(seg_item("GND", arc.end, p(mm(4.0), mm(6.0)), "t_out"));
    let line = node.assemble_line(node.iter().find(|(_, it)| matches!(it, Item::Arc(_))).unwrap().0).unwrap();
    assert_eq!(line.arc_count(), 1);
    let pusher = eda_pns::line::Line::from_points(net_of("SIG"), 0, mm(0.2), vec![p(mm(-5.0), mm(-2.0)), p(mm(-5.0), mm(0.3))]);
    let shoved = eda_pns::shove::shove_obstacle_line(&node, &rules(), &eda_pns::settings::RoutingSettings::default(), &pusher, None, &line, None, None).expect("the run can be walked round the pusher's end");
    assert_ne!(shoved.pts, line.pts, "something moved");
    assert_eq!(shoved.arc_count(), 1, "the arc did not: {:?}", shoved.pts.len());
    let run = shoved.arc_runs()[0];
    assert_eq!(&shoved.pts[run.first..=run.last], arc.polyline().as_slice(), "its points are the ones it had");
    // and the pieces the IR gets: segments, the very arc, segments
    let pieces = shoved.pieces();
    assert!(matches!(pieces.as_slice(), [LinePiece::Poly(_), LinePiece::Arc(a), LinePiece::Poly(_)] if *a == arc), "{pieces:?}");
}

/// The whole route, not the shove alone: the line the head shoves stands on three IR tracks (a run, an arc, a run on) and the commit replaces all three, but the arc
/// the shove did not reach is put back as an arc track -- the very arc, not a polyline of its chords.
#[test]
fn a_route_that_shoves_a_line_with_an_arc_puts_the_arc_it_did_not_reach_back_as_an_arc_track() {
    let arc = ArcPts::new(p(mm(2.0), Nm::ZERO), p(Nm::from_raw_shim(2_000_000 + 1_414_000), Nm::from_raw_shim(2_000_000 - 1_414_000)), p(mm(4.0), mm(2.0)));
    let tracks = vec![
        straight("t_run", "GND", &[p(mm(-8.0), Nm::ZERO), arc.start]),
        arc_track("t_arc", "GND", arc),
        straight("t_out", "GND", &[arc.end, p(mm(4.0), mm(6.0))]),
        straight("t_sig", "SIG", &[p(mm(-5.0), mm(-9.0)), p(mm(-5.0), mm(-8.0))]),
    ];
    let (design, model) = board_with_pads_at(tracks, mm(20.0));
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::Shove;
    router.start(p(mm(-5.0), mm(-8.0)), "F.Cu", mm(0.2)).expect("starts on the end of the SIG stub");
    // the head ends on the GND run, 7 mm from the arc: the run is pushed round the head's end
    let target = p(mm(-5.0), mm(0.3));
    let preview = router.preview(target).expect("in progress");
    assert!(!preview.colliding, "the shove resolves it");
    assert!(!preview.displaced_lines.is_empty(), "the run is pushed");
    assert!(preview.displaced_lines.iter().any(|d| d.line.arc_count() == 1), "and the line it is pushed as still has its arc: {:?}", preview.displaced_lines.iter().map(|d| (d.source_track.clone(), d.line.pts.len(), d.line.arc_count())).collect::<Vec<_>>());
    let commit = router.finish(target).expect("the shoved route commits");
    for t in ["t_run", "t_arc", "t_out"] {
        assert!(commit.remove_track_ids.contains(&t.to_string()), "{t} is replaced: {:?}", commit.remove_track_ids);
    }
    let (arcs, _) = tracks_of(&commit);
    assert_eq!(arcs.iter().map(|t| t.arc().unwrap()).collect::<Vec<_>>(), vec![(arc.start, arc.mid, arc.end)], "the arc comes back as the arc it was");
    assert!(arcs.iter().all(|t| t.net == "GND" && t.layer == "F.Cu" && t.width == mm(0.2)));
    assert_eq!(collisions_after(&design, &model, &commit), 0, "and the board is clean");
}

/// A fillet is tangent to the run before it: the vertex the run ends and the arc starts at is all but collinear with its neighbours (on a micrometre grid the first
/// chord of the polyline lay on the run's line exactly; in nanometres it leaves it by half a micrometre) -- and a clean-up of the shoved path that does not know the
/// arc could drop it, and the arc with it.
#[test]
fn a_route_that_pushes_the_run_before_a_tangent_arc_leaves_the_arc_an_arc() {
    let (s, m, e) = (p(mm(12.0), mm(10.0)), p(Nm::new(15_535_534), Nm::new(11_464_466)), p(mm(17.0), mm(15.0))); // a quarter arc about (12, 15) mm, radius 5 mm
    let arc = ArcPts::new(s, m, e);
    let poly = arc.polyline();
    assert_eq!(poly[0], s, "the arc's polyline starts where the run ends: {:?}", &poly[..3]);
    let tracks = vec![straight("t_a", "GND", &[p(mm(6.0), mm(10.0)), s]), arc_track("t_arc", "GND", arc), straight("t_b", "GND", &[e, p(mm(17.0), mm(21.0))]), straight("t_sig", "SIG", &[p(mm(8.0), mm(15.0)), p(mm(8.0), mm(14.0))])];
    let (design, model) = board_with_pads_at(tracks, mm(40.0));
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::Shove;
    router.start(p(mm(8.0), mm(14.0)), "F.Cu", mm(0.2)).expect("starts on the end of the SIG stub");
    // the head ends on the run, 4 mm before the arc: the run is pushed round the head's end
    let target = p(mm(8.0), mm(10.0));
    let preview = router.preview(target).expect("in progress");
    assert!(!preview.colliding && !preview.displaced_lines.is_empty(), "the run is pushed: {:?}", preview.colliding);
    assert!(preview.displaced_lines.iter().any(|d| d.line.arc_count() == 1), "and the line it is pushed as still has its arc: {:?}", preview.displaced_lines.iter().map(|d| (d.source_track.clone(), d.line.pts.len(), d.line.arc_count())).collect::<Vec<_>>());
    let commit = router.finish(target).expect("the shoved route commits");
    let (arcs, others) = tracks_of(&commit);
    assert_eq!(arcs.iter().map(|t| t.arc().unwrap()).collect::<Vec<_>>(), vec![(s, m, e)], "the arc comes back as the arc it was: {:?}", others.iter().map(|t| t.pts.clone()).collect::<Vec<_>>());
    assert_eq!(collisions_after(&design, &model, &commit), 0, "and the board is clean");
}

#[test]
fn a_line_that_is_cut_across_an_arc_is_written_back_as_segments_and_one_that_is_not_has_its_arc_as_an_arc_track() {
    let mut node = Node::new();
    let [s1, _, _] = s_bend(&mut node, "SIG");
    let line = node.assemble_line(s1).unwrap();
    let tracks = eda_pns::router::line_to_tracks(&line, "F.Cu", None);
    assert_eq!(tracks.len(), 3);
    assert_eq!(tracks[0].pts, vec![p(mm(4.0), mm(-3.0)), quarter().start]);
    assert_eq!(tracks[1].arc(), Some((quarter().start, quarter().mid, quarter().end)));
    assert_eq!(tracks[2].pts, vec![quarter().end, p(mm(-3.0), mm(4.0))]);
    assert!(tracks.iter().all(|t| t.net == "SIG" && t.layer == "F.Cu" && t.width == mm(0.2)));
    // the arc cut: one polyline
    let mut cut = line.clone();
    cut.pts[5].x += Nm::new(3000);
    let tracks = eda_pns::router::line_to_tracks(&cut, "F.Cu", None);
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].pts, cut.pts);
}

// ------------------------------------------------------------------------------------------------------------------------------
// dragging
// ------------------------------------------------------------------------------------------------------------------------------

/// A GND line that turns a corner with a fillet: along y = 0 to (6000, 0), the quarter arc about (6000, 4000) to (10000, 4000), down x = 10000.
fn fillet_tracks() -> (Vec<Track>, ArcPts) {
    let arc = ArcPts::from_start_end_center(p(mm(6.0), Nm::ZERO), p(mm(10.0), mm(4.0)), p(mm(6.0), mm(4.0)));
    (vec![straight("t_in", "GND", &[p(Nm::ZERO, Nm::ZERO), arc.start]), arc_track("t_arc", "GND", arc), straight("t_out", "GND", &[arc.end, p(mm(10.0), mm(12.0))])], arc)
}

#[test]
fn grabbing_an_arc_drags_its_curvature_with_the_tracks_next_to_it_following() {
    let (tracks, arc) = fillet_tracks();
    let (design, model) = board_with_pads_at(tracks, mm(30.0));
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::MarkObstacles;
    router.drag_start(arc.mid, "F.Cu").expect("an arc track can be grabbed");
    assert_eq!(router.drag_kind(), Some(eda_pns::dragger::DragKind::Arc));
    // toward the corner (10000, 0): a tighter arc
    let preview = router.drag_preview(p(mm(9.7), mm(0.3))).expect("a drag in progress");
    assert!(!preview.colliding);
    assert_eq!(preview.arcs.len(), 1, "the line still has its arc: {:?}", preview.arcs);
    let tight = preview.arcs[0];
    assert!(tight.radius() < 4_000_000.0, "{}", tight.radius());
    assert_eq!((preview.pts.first(), preview.pts.last()), (Some(&p(Nm::ZERO, Nm::ZERO)), Some(&p(mm(10.0), mm(12.0)))), "the far ends of the tracks stay");
    // tangent to the tracks: its ends are on them
    assert_eq!((tight.start.y, tight.end.x), (Nm::ZERO, mm(10.0)));
    // away from the corner: a larger one
    let wide = router.drag_preview(p(mm(7.5), mm(2.5))).unwrap().arcs[0];
    assert!(wide.radius() > 4_000_000.0, "{}", wide.radius());
    let commit = router.drag_finish(p(mm(7.5), mm(2.5))).expect("the drag commits");
    for t in ["t_in", "t_arc", "t_out"] {
        assert!(commit.remove_track_ids.contains(&t.to_string()), "{t}: {:?}", commit.remove_track_ids);
    }
    let arcs: Vec<_> = commit.tracks.iter().filter_map(|t| t.arc()).collect();
    assert_eq!(arcs.len(), 1, "one arc track, written back as an arc: {:?}", commit.tracks.iter().map(|t| (t.pts.len(), t.arc().is_some())).collect::<Vec<_>>());
    assert!(ArcPts::new(arcs[0].0, arcs[0].1, arcs[0].2).radius() > 4_000_000.0);
    assert_eq!(collisions_after(&design, &model, &commit), 0);
}

#[test]
fn an_arc_dragged_onto_a_track_of_another_net_is_refused_in_walk_around_mode_and_flagged_in_highlight_mode() {
    let (mut tracks, arc) = fillet_tracks();
    // a SIG track across the place a wider fillet would take
    tracks.push(straight("t_sig", "SIG", &[p(mm(7.0), mm(1.0)), p(mm(9.0), mm(3.0))]));
    // the arc as it is clears it: (8828, 1172) is the middle of the arc, 1657 from the corner; this track passes inside the arc's bend, 1 mm from it
    let (design, model) = board_with_pads_at(tracks, mm(30.0));
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::MarkObstacles;
    router.drag_start(arc.mid, "F.Cu").unwrap();
    let flagged = router.drag_preview(p(mm(8.0), mm(2.0))).unwrap();
    assert!(flagged.colliding, "the arc drawn through the cursor runs into the SIG track");
    // walk around mode: an arc is not walked round anything, the position is not one the drag can take, so the last good one is shown
    router.settings.mode = Mode::Walkaround;
    router.drag_start(arc.mid, "F.Cu").unwrap();
    let first = router.drag_preview(arc.mid).unwrap();
    assert!(!first.colliding);
    let refused = router.drag_preview(p(mm(8.0), mm(2.0))).unwrap();
    assert!(refused.restored, "the cursor is where the drag cannot be resolved");
    assert_eq!(refused.pts, first.pts, "the last position that worked is shown again");
}

#[test]
fn a_drag_of_a_segment_next_to_an_arc_leaves_the_arc_as_it_is() {
    let (mut tracks, arc) = fillet_tracks();
    // a longer way in: the first track is two segments, so that the one next to the arc can slide
    tracks[0] = straight("t_in", "GND", &[p(Nm::ZERO, Nm::ZERO), p(mm(3.0), Nm::ZERO), arc.start]);
    let (design, model) = board_with_pads_at(tracks, mm(30.0));
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::MarkObstacles;
    // the middle of the segment (3000, 0)..(6000, 0), which ends where the arc begins
    router.drag_start(p(mm(4.5), Nm::ZERO), "F.Cu").expect("the segment is grabbed");
    assert_eq!(router.drag_kind(), Some(eda_pns::dragger::DragKind::Segment));
    let preview = router.drag_preview(p(mm(4.5), mm(-1.5))).unwrap();
    assert!(!preview.colliding);
    assert_eq!(preview.arcs, vec![arc], "the arc is the arc it was");
    let commit = router.drag_finish(p(mm(4.5), mm(-1.5))).expect("commits");
    let arcs: Vec<_> = commit.tracks.iter().filter_map(|t| t.arc()).collect();
    assert_eq!(arcs, vec![(arc.start, arc.mid, arc.end)]);
    assert_eq!(collisions_after(&design, &model, &commit), 0);
}

#[test]
fn an_arc_of_179_degrees_or_more_cannot_be_dragged() {
    let half = ArcPts::new(p(Nm::ZERO, Nm::ZERO), p(mm(4.0), mm(-4.0)), p(mm(8.0), Nm::ZERO));
    let (design, model) = board_with_pads_at(vec![arc_track("t_half", "GND", half)], mm(30.0));
    let mut router = Router::new(&design, &model);
    let err = router.drag_start(p(mm(4.0), mm(-4.0)), "F.Cu").unwrap_err();
    assert_eq!(err, eda_pns::arc_drag::TOO_WIDE);
}

#[test]
fn a_dragged_arc_shoves_a_via_it_runs_into_aside() {
    let (tracks, arc) = fillet_tracks();
    let (mut design, model) = board_with_pads_at(tracks, mm(30.0));
    design.routing.as_mut().unwrap().vias.push(eda_model::ir::Via { id: "via_sig".into(), net: "SIG".into(), at: p(mm(8.0), mm(2.0)), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::Shove;
    router.drag_start(arc.mid, "F.Cu").expect("grabbed");
    // the arc is dragged through the via
    let preview = router.drag_preview(p(mm(8.0), mm(2.0))).unwrap();
    assert!(!preview.colliding && !preview.restored, "the via is shoved, not a collision");
    assert_eq!(preview.displaced_vias.len(), 1, "the via is pushed off the arc");
    assert_eq!(preview.arcs.len(), 1);
    let commit = router.drag_finish(p(mm(8.0), mm(2.0))).expect("commits");
    assert!(commit.remove_via_ids.contains(&"via_sig".to_string()));
    let moved = commit.vias.iter().find(|v| v.net == "SIG").expect("the via is put back");
    assert_ne!(moved.at, p(mm(8.0), mm(2.0)));
    assert_eq!(collisions_after_with_vias(&design, &model, &commit), 0);
}

/// [`collisions_after`] with the commit's vias applied as well.
fn collisions_after_with_vias(design: &Design, model: &ConstraintModel, commit: &RouteCommit) -> usize {
    let mut after = design.clone();
    let rt = after.routing.as_mut().unwrap();
    rt.tracks.retain(|t| !commit.remove_track_ids.contains(&t.id));
    rt.tracks.extend(commit.tracks.iter().cloned());
    rt.vias.retain(|v| !commit.remove_via_ids.contains(&v.id));
    rt.vias.extend(commit.vias.iter().cloned());
    let (world, _) = eda_pns::from_ir::build_node(&after, model);
    let mut bad = 0;
    for (id, item) in world.iter() {
        if matches!(item, Item::Solid(_)) {
            continue;
        }
        bad += world.all_colliding(&item.shape(item.layers().start()), item.net(), item.layers(), &model.board, &[id]).iter().filter(|o| !matches!(world.get(o.id), Some(Item::Solid(_)))).count();
    }
    bad
}

/// findings p5 F3-11: a free-angle grab (`G`) in the middle of a segment nearer the end where an arc starts takes the segment's start, not
/// the arc's (`startDragSegment`: `!m_draggedLine.CLine().IsPtOnArc( m_draggedSegmentIndex + 1 )`).
#[test]
fn a_free_angle_grab_next_to_an_arc_takes_the_segments_start() {
    let mut node = Node::new();
    let [s1, _, _] = s_bend(&mut node, "SIG");
    let at = p(mm(4.0), mm(-0.9));
    let d = eda_pns::dragger::Dragger::start_with(&node, at, s1, true).expect("a free-angle grab");
    assert_eq!(d.original.pts[d.grabbed_index], p(mm(4.0), mm(-3.0)), "the segment's own start");
}
