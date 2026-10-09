//! The board outline is an obstacle to the router (`PNS_KICAD_IFACE_BASE::syncGraphicalItem` for Edge.Cuts, and
//! `PNS_PCBNEW_RULE_RESOLVER::Clearance`'s `CT_EDGE_CLEARANCE` for it): copper keeps the board's copper-to-edge clearance
//! from it, whatever net class it is in, and a shove cannot push a track past it. Before this, a shove toward the edge ended
//! inside the clearance and kicad-cli reported `copper_edge_clearance` on a shoved track (seen on `mcu30`).

use eda_drc::kimath::Shape;
use eda_model::ir::{Design, PlacementSection, Point, Provenance, RoutingSection, Track, Um};
use eda_model::{ConstraintModel, NetClass};
use eda_pns::from_ir::build_node;
use eda_pns::item::{net_of, Item};
use eda_pns::layer::LayerRange;
use eda_pns::node::Node;
use eda_pns::router::Router;
use eda_pns::settings::{Mode, RoutingSettings};
use eda_pns::shove::shove_line;

fn p(x: Um, y: Um) -> Point {
    Point { x, y }
}

fn track(id: &str, net: &str, layer: &str, pts: &[Point]) -> Track {
    Track { id: id.into(), net: net.into(), pins: vec![], layer: layer.into(), width: 200, pts: pts.to_vec(), arc_mid_offset: None }
}

/// A 20 mm square board with no parts and the given tracks.
fn board(outline: bool, tracks: Vec<Track>) -> (Design, ConstraintModel) {
    let outline = if outline { vec![p(0, 0), p(20_000, 0), p(20_000, 20_000), p(0, 20_000)] } else { vec![] };
    let design = Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: 1,
        provenance: Provenance { engine_version: "t".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(PlacementSection { outline, footprints: vec![], modules: vec![] }),
        routing: Some(RoutingSection { tracks, vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }),
        drawings: None,
    };
    (design, ConstraintModel::default())
}

/// The outline's four sides as the router's world holds them.
fn edges(node: &Node) -> Vec<(Point, Point)> {
    let mut out: Vec<(Point, Point)> = node
        .iter()
        .filter_map(|(_, it)| match it {
            Item::Solid(s) if s.edge => match &s.shape {
                Shape::Stadium { a, b, r: 0 } => Some((*a, *b)),
                other => panic!("an edge is a zero-width segment, not {other:?}"),
            },
            _ => None,
        })
        .collect();
    out.sort_by_key(|(a, b)| (a.x, a.y, b.x, b.y));
    out
}

/// The least gap between a track of `width` along `pts` and the outline.
fn gap_to_outline(pts: &[Point], width: Um) -> f64 {
    let outline = [p(0, 0), p(20_000, 0), p(20_000, 20_000), p(0, 20_000)];
    let mut best = f64::MAX;
    for w in pts.windows(2) {
        let seg = Shape::Stadium { a: w[0], b: w[1], r: width / 2 };
        for i in 0..4 {
            best = best.min(seg.gap_to(&Shape::Stadium { a: outline[i], b: outline[(i + 1) % 4], r: 0 }));
        }
    }
    best
}

#[test]
fn the_outline_is_one_edge_solid_per_side_on_every_copper_layer() {
    let (design, model) = board(true, vec![]);
    let (node, layers) = build_node(&design, &model);
    assert_eq!(edges(&node), vec![(p(0, 0), p(20_000, 0)), (p(0, 20_000), p(0, 0)), (p(20_000, 0), p(20_000, 20_000)), (p(20_000, 20_000), p(0, 20_000))]);
    for (_, it) in node.iter() {
        assert_eq!(it.layers(), layers.all(), "an edge is on every copper layer");
        assert!(it.net().is_none(), "an edge has no net: it collides with everything");
        assert!(it.anchors().is_empty(), "nothing connects to an edge");
        assert!(!it.is_movable());
    }
    // A board with no outline has no edge to keep from.
    let (design, model) = board(false, vec![]);
    assert!(edges(&build_node(&design, &model).0).is_empty());
}

#[test]
fn copper_keeps_the_boards_edge_clearance_from_the_outline_not_its_net_classs() {
    // A 200 um track whose copper is `gap` from the bottom edge (y = 0): its centreline is at y = gap + 100.
    let near = |gap: Um, model: &ConstraintModel| {
        let (design, _) = board(true, vec![]);
        let (node, layers) = build_node(&design, model);
        let shape = Shape::Stadium { a: p(5_000, gap + 100), b: p(15_000, gap + 100), r: 100 };
        // the router's own layer index of F.Cu and B.Cu: an edge is in the way on both
        let hit = |layer: &str| !node.all_colliding(&shape, &net_of("SIG"), LayerRange::single(layers.index_of(layer).unwrap()), &model.board, &[]).is_empty();
        (hit("F.Cu"), hit("B.Cu"))
    };
    let default = ConstraintModel::default();
    // KiCad's default copper-to-edge clearance is 0.5 mm; the router's epsilon is 1 um.
    assert_eq!(near(300, &default), (true, true), "300 um from the edge is inside 500");
    assert_eq!(near(498, &default), (true, true), "two microns inside the clearance");
    assert_eq!(near(499, &default), (false, false), "the router's 1 um epsilon (`COLLISION_SEARCH_OPTIONS::m_useClearanceEpsilon`) lets a micron go");
    assert_eq!(near(500, &default), (false, false), "exactly the clearance is clear");
    assert_eq!(near(600, &default), (false, false));
    // The board's own setting (`min_copper_edge_clearance`) is what counts.
    let mut tight = ConstraintModel::default();
    tight.board.copper_edge_clearance_um = Some(250);
    assert_eq!(near(300, &tight), (false, false), "300 um is outside a 250 um setting");
    assert_eq!(near(200, &tight), (true, true));
    // A net class that asks for more copper-to-copper clearance does not move the edge.
    let mut classed = ConstraintModel::default();
    classed.board.net_classes.push(NetClass { name: "power".into(), nets: vec!["SIG".into()], track_width: None, clearance: Some(800), via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 });
    assert_eq!(near(600, &classed), (false, false), "an 800 um class clearance is between copper, not to the edge");
}

/// GND runs along y = 1000 (copper 900..1100, 900 um from the bottom edge). SIG comes down x = 10000 from y = 8000 and
/// pushes it toward the edge: the track moves to 400 um (200 clearance + both half widths) above SIG's tip. The edge's
/// 500 um stops it at SIG's tip y = 1000: its copper is then at 500.
fn pushed_toward_the_edge(outline: bool, tip: Um) -> Option<Vec<Point>> {
    let gnd = track("gnd", "GND", "F.Cu", &[p(2_000, 1_000), p(18_000, 1_000)]);
    let (design, model) = board(outline, vec![gnd]);
    let (node, layers) = build_node(&design, &model);
    let settings = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
    let layer = layers.index_of("F.Cu").unwrap();
    let outcome = shove_line(&node, &[p(10_000, 8_000), p(10_000, tip)], &net_of("SIG"), layer, 200, &model.board, &settings)?;
    assert_eq!(outcome.head, vec![p(10_000, 8_000), p(10_000, tip)], "SIG is the pusher: it goes where it was asked");
    let gnd: Vec<&eda_pns::shove::DisplacedLine> = outcome.displaced_lines.iter().filter(|d| d.source_track.as_deref() == Some("gnd")).collect();
    assert!(!gnd.is_empty(), "GND is in the way and is pushed");
    // its longest piece is the pushed run
    Some(gnd.iter().flat_map(|d| d.line.pts.clone()).collect())
}

#[test]
fn a_shove_toward_the_edge_stops_at_the_clearance() {
    // SIG's tip at 1400: GND is 200 um from it already, nothing moves.
    // SIG's tip at 1100: GND has to be 400 below the tip (y <= 700); its copper (600) is 100 um outside the edge's 500.
    let pushed = pushed_toward_the_edge(true, 1_100).expect("there is room");
    let gap = gap_to_outline(&pushed, 200);
    assert!(gap >= 499.0, "GND was pushed to {gap} um from the edge: {pushed:?}");
    assert!(pushed.iter().any(|q| q.y < 1_000), "and it did move: {pushed:?}");

    // SIG's tip at 1000: GND's copper reaches exactly 500 um from the edge: the limit.
    let pushed = pushed_toward_the_edge(true, 1_000).expect("exactly the clearance is room enough");
    assert!(gap_to_outline(&pushed, 200) >= 499.0, "{pushed:?}");

    // SIG's tip at 800 would need GND at 400, 300 um from the edge: the shove refuses instead of pushing it there.
    assert!(pushed_toward_the_edge(true, 800).is_none(), "a track is not pushed past the board's edge clearance");
    // Without the outline there is nothing to stop it, and GND ends 300 um from where the edge would be.
    let pushed = pushed_toward_the_edge(false, 800).expect("with no outline the shove goes through");
    assert!(gap_to_outline(&pushed, 200) < 400.0, "{pushed:?}");
}

#[test]
fn a_route_whose_shove_would_cross_the_edge_clearance_is_not_a_shove_in_the_studio_either() {
    // The same board through `Router`, as the studio drives it: SIG starts at the end of its own stub.
    let gnd = track("gnd", "GND", "F.Cu", &[p(2_000, 1_000), p(18_000, 1_000)]);
    let sig = track("sig", "SIG", "F.Cu", &[p(10_000, 9_000), p(10_000, 8_000)]);
    let (design, model) = board(true, vec![gnd, sig]);
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::Shove;
    router.start(p(10_000, 8_000), "F.Cu", 200).expect("SIG's stub end is a start");

    let ok = router.preview(p(10_000, 1_100)).expect("a session");
    assert!(!ok.colliding);
    let moved: Vec<Point> = ok.displaced_lines.iter().filter(|d| d.source_track.as_deref() == Some("gnd")).flat_map(|d| d.line.pts.clone()).collect();
    assert!(!moved.is_empty(), "GND is pushed toward the edge");
    assert!(gap_to_outline(&moved, 200) >= 499.0, "{moved:?}");

    // Further than the clearance allows, whatever the router calls fine keeps clear of the edge.
    for tip in [900, 800, 600, 400] {
        let pv = router.preview(p(10_000, tip)).expect("a session");
        let moved: Vec<Point> = pv.displaced_lines.iter().flat_map(|d| d.line.pts.clone()).collect();
        if !moved.is_empty() {
            assert!(gap_to_outline(&moved, 200) >= 499.0, "tip {tip}: a pushed track is {} um from the edge: {moved:?}", gap_to_outline(&moved, 200));
        }
        if !pv.colliding {
            assert!(gap_to_outline(&pv.head.pts, 200) >= 499.0, "tip {tip}: the head, not marked colliding, is {} um from the edge: {:?}", gap_to_outline(&pv.head.pts, 200), pv.head.pts);
        }
    }
}

#[test]
fn a_head_that_would_end_inside_the_clearance_is_marked_and_a_pad_free_one_is_not() {
    let sig = track("sig", "SIG", "F.Cu", &[p(10_000, 9_000), p(10_000, 8_000)]);
    let (design, model) = board(true, vec![sig]);
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::MarkObstacles;
    router.start(p(10_000, 8_000), "F.Cu", 200).unwrap();
    assert!(!router.preview(p(10_000, 700)).unwrap().colliding, "600 um of copper from the edge is clear");
    assert!(router.preview(p(10_000, 500)).unwrap().colliding, "400 um is inside the clearance");
}
