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
