//! r5 R5-1: a route started on an item of another layer than the active one starts on the item's layer, with its net (`TOOL_BASE::pickSingleItem`
//! looks on every layer, the active one first; `ROUTER_TOOL::getStartLayer` then gives the item's layer when it is not on the active one, and
//! `prepareInteractive` makes that the active layer).

use eda_model::units::mm;
use eda_model::ir::{Design, FootprintInstance, LabelSide, PlacementSection, Point, Provenance, RoutingSection, Side, Track, Nm};
use eda_model::{ConstraintModel, Net as IrNet, Part, Pin, PinKind};
use eda_pns::from_ir::build_node;
use eda_pns::item::Item;
use eda_pns::router::Router;

fn p(x: Nm, y: Nm) -> Point {
    Point { x, y }
}

fn part(r: &str) -> Part {
    Part { reference: r.into(), mpn: None, lcsc: None, value: None, package: Some("0603".into()), footprint: Some("0603".into()), pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }], body_nm: None, symbol: None, datasheet: None, edge: None }
}

fn track(id: &str, net: &str, layer: &str, pts: &[Point]) -> Track {
    Track { id: id.into(), net: net.into(), pins: vec![], layer: layer.into(), width: mm(0.2), pts: pts.to_vec(), arc_mid_offset: None }
}

/// R1 on the bottom side at the origin (its SMD pads on B.Cu), R2 on the top far away, and the tracks given.
fn board(tracks: Vec<Track>) -> (Design, ConstraintModel) {
    let model = ConstraintModel { parts: vec![part("R1"), part("R2")], nets: vec![IrNet { name: "SIG".into(), pins: vec!["R1.1".into(), "R2.1".into()] }, IrNet { name: "OUT".into(), pins: vec!["R1.2".into(), "R2.2".into()] }], ..Default::default() };
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
            footprints: vec![FootprintInstance { id: "R1".into(), at: p(Nm::ZERO, Nm::ZERO), rot: 0, side: Side::Bottom, label: LabelSide::Above }, FootprintInstance { id: "R2".into(), at: p(mm(20.0), mm(20.0)), rot: 0, side: Side::Top, label: LabelSide::Above }],
            modules: vec![],
        }),
        routing: Some(RoutingSection { tracks, vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], diff_pair_presets: vec![], teardrop_settings: Default::default() }),
        drawings: None,
    };
    (design, model)
}

/// The centre of the pad `source` (`"R1.1"`).
fn pad_at(design: &Design, model: &ConstraintModel, source: &str) -> (Point, String) {
    let (node, _) = build_node(design, model);
    let found = node.iter().find_map(|(_, it)| match it {
        Item::Solid(s) if s.source == source => Some((s.pos, format!("{:?}", s.layers))),
        _ => None,
    });
    found.expect("the pad is in the world")
}

#[test]
fn a_route_started_on_a_pad_of_the_other_side_starts_on_its_layer() {
    let (design, model) = board(vec![]);
    let (at, _) = pad_at(&design, &model, "R1.1");
    let mut router = Router::new(&design, &model);
    router.start(at, "F.Cu", mm(0.25)).expect("starts on the bottom pad");
    assert_eq!(router.current_net().as_deref(), Some("SIG"), "the route has the pad's net");
    let pv = router.preview(p(at.x + mm(2.0), at.y)).expect("routing");
    assert_eq!(router.layer_name(pv.head.layer), "B.Cu", "getStartLayer: the pad's layer");
    assert_eq!(router.start_layer(at, "F.Cu").as_deref(), Some("B.Cu"));
}

#[test]
fn a_route_started_on_a_track_end_of_the_other_layer_starts_on_its_layer() {
    let (design, model) = board(vec![track("t_out", "OUT", "B.Cu", &[p(mm(5.0), mm(5.0)), p(mm(8.0), mm(5.0))])]);
    let mut router = Router::new(&design, &model);
    router.start(p(mm(8.0), mm(5.0)), "F.Cu", mm(0.25)).expect("starts on the bottom track");
    assert_eq!(router.current_net().as_deref(), Some("OUT"));
    let pv = router.preview(p(mm(10.0), mm(5.0))).expect("routing");
    assert_eq!(router.layer_name(pv.head.layer), "B.Cu");
    // the joined track's width is the B.Cu track's (`SetStartLayerFromPCBNew( pcbLayer )` before `ImportSizes`)
    assert_eq!(router.connected_track_width(p(mm(8.0), mm(5.0)), "F.Cu"), Some(mm(0.2)));
}

#[test]
fn an_item_on_the_active_layer_wins_over_one_on_another() {
    // a F.Cu track end and a B.Cu track end at the same point: `prioritized[1]` (on the top layer) before `prioritized[3]`
    let (design, model) = board(vec![track("t_b", "OUT", "B.Cu", &[p(mm(5.0), mm(5.0)), p(mm(8.0), mm(5.0))]), track("t_f", "SIG", "F.Cu", &[p(mm(8.0), mm(5.0)), p(mm(8.0), mm(9.0))])]);
    let mut router = Router::new(&design, &model);
    router.start(p(mm(8.0), mm(5.0)), "F.Cu", mm(0.25)).expect("starts");
    assert_eq!(router.current_net().as_deref(), Some("SIG"));
    router.start(p(mm(8.0), mm(5.0)), "B.Cu", mm(0.25)).expect("starts");
    assert_eq!(router.current_net().as_deref(), Some("OUT"));
}

/// UI-LEFT R5 (`ROUTER::GetNearestRatnestAnchor`, which `ROUTER_TOOL::handleLayerSwitch` takes as Place Through Via's implicit target): a route from R2.1 on
/// the top has the nearest unconnected anchor of its net at R1.1, a pad of the bottom-side R1, so on B.Cu -- with nothing drawn (`NearestUnconnectedItem`
/// from the start's joint) and with a head (`NearestUnconnectedAnchorPoint( trace )`).
#[test]
fn the_nearest_ratsnest_anchor_is_on_the_layer_of_the_pad_at_the_other_end() {
    let (design, model) = board(vec![]);
    let (r1, _) = pad_at(&design, &model, "R1.1");
    let (r2, _) = pad_at(&design, &model, "R2.1");
    let mut router = Router::new(&design, &model);
    router.start(r2, "F.Cu", mm(0.2)).expect("starts on R2.1");
    let (at, layers) = router.ratsnest_anchor(r2).expect("R1.1 is left to connect");
    assert_eq!(at, r1);
    assert_eq!((router.layer_name(layers.start()), router.layer_name(layers.end())), ("B.Cu", "B.Cu"));
    let (at, layers) = router.ratsnest_anchor(p(mm(10.0), mm(15.0))).expect("with a head too");
    assert_eq!((at, router.layer_name(layers.start())), (r1, "B.Cu"));
    // a route of no net has no ratsnest ("Can't finish something with no connections")
    let mut free = Router::new(&design, &model);
    free.start(p(mm(5.0), mm(30.0)), "F.Cu", mm(0.2)).expect("starts in empty space");
    assert!(free.ratsnest_anchor(p(mm(8.0), mm(30.0))).is_none());
}

/// UI-LEFT R5 (`ROUTER_TOOL::updateSizesAfterRouterEvent`): after a via to B.Cu the clearance is asked again of a dummy track on B.Cu, where a custom rule
/// gives 0.5 mm; on F.Cu it is the class's. The width is asked again too, as "Use Starting Track Width" is on (`!TrackWidthIsExplicit()`), and the head,
/// which has a fixed leg, keeps its width (`LINE_PLACER::UpdateSizes`).
#[test]
fn the_clearance_is_read_again_on_the_layer_a_via_goes_to() {
    let (design, mut model) = board(vec![]);
    model.board.custom_rules_text = Some("(version 1)\n(rule bclr\n  (layer \"B.Cu\")\n  (constraint clearance (min 0.5mm)))\n(rule bwidth\n  (layer \"B.Cu\")\n  (constraint track_width (min 0.3mm) (opt 0.35mm)))\n".into());
    let (r2, _) = pad_at(&design, &model, "R2.1");
    let mut router = Router::new(&design, &model);
    router.start(r2, "F.Cu", mm(0.2)).expect("starts on R2.1");
    let on_top = router.update_sizes_after_router_event(0, r2).expect("routing");
    assert_eq!(on_top.clearance, model.board.clearance.max(model.board.min_clearance_nm), "{on_top:?}");
    let back = router.layer_index("B.Cu").unwrap();
    router.toggle_via(true, mm(0.6), mm(0.3), Some(back));
    let at = p(mm(20.0), mm(15.0));
    router.preview(at);
    assert!(router.fix(at).is_some());
    let after = router.update_sizes_after_router_event(back, at).expect("routing");
    assert_eq!((after.clearance, after.clearance_source.as_str()), (mm(0.5), "bclr"), "{after:?}");
    assert_eq!(after.width, Some((mm(0.35), "bwidth".to_string())), "out of the rule's range: the rule's opt");
    assert!(!after.width_applied, "a leg is fixed and the width was inherited, not chosen");
    assert_eq!(router.current_width(), Some(mm(0.2)));
    // a width chosen while routing is explicit: the head takes the layer's width when it is out of range
    router.use_net_class_track = false;
    router.set_width(mm(0.25));
    let chosen = router.update_sizes_after_router_event(back, at).expect("routing");
    assert_eq!(chosen.width, None, "explicit and not the net class's: not asked again");
    router.use_net_class_track = true;
    let class = router.update_sizes_after_router_event(back, at).expect("routing");
    assert!(class.width_applied && router.current_width() == Some(mm(0.35)), "{class:?}");
}

/// UI-LEFT R7 (findings p5 F2-11, `TOOL_BASE::pickSingleItem`'s `maxSlopRadius = std::max( grid.x, grid.y )`): with nothing exactly under the cursor, a
/// route starts and ends on what is within the grid of it, measured to the copper -- not on what is within a fixed 0.5 mm of its anchor. A point 0.3 mm
/// beside R2.1's copper is empty space on a 0.25 mm grid and R2.1 on a 0.5 mm one, for the start and for the end.
#[test]
fn the_grab_radius_is_the_grid_measured_to_the_copper() {
    let (design, model) = board(vec![]);
    let (r2, _) = pad_at(&design, &model, "R2.1");
    let (node, _) = build_node(&design, &model);
    let top = node.iter().find_map(|(_, it)| match it {
        Item::Solid(s) if s.source == "R2.1" => Some(s.shape.bbox(Nm::ZERO).3),
        _ => None,
    });
    let beside = p(r2.x, top.expect("R2.1's copper") + mm(0.3));
    for (grid, grabbed) in [(mm(0.25), false), (mm(0.5), true)] {
        let mut router = Router::new(&design, &model);
        router.settings.grab_radius = grid;
        router.start(beside, "F.Cu", mm(0.2)).expect("starts");
        assert_eq!(router.current_net().as_deref() == Some("SIG"), grabbed, "start on a {grid} grid");
        // and the end, of a route of SIG (`pickSingleItem( mousePos, net, layer, .. )`: a route of no net has no end item, `updateEndItem`), from the free
        // end of a SIG stub on F.Cu
        let (stubbed, model) = board(vec![track("stub", "SIG", "F.Cu", &[p(mm(30.0), mm(25.0)), p(mm(28.0), mm(25.0))])]);
        let mut router = Router::new(&stubbed, &model);
        router.settings.grab_radius = grid;
        router.start(p(mm(28.0), mm(25.0)), "F.Cu", mm(0.2)).expect("the stub's end");
        assert_eq!(router.current_net().as_deref(), Some("SIG"));
        let pv = router.preview(beside).expect("routing");
        assert_eq!(pv.snapped_end.is_some(), grabbed, "end on a {grid} grid: {pv:?}");
    }
}
