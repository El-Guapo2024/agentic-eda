//! Stage 2 integration test: route a real connection across a realistic
//! multi-footprint board (built-in SOIC-8/0603 pad geometry, the same
//! library `crates/model/src/footprint.rs` uses for every example board in
//! this workspace), through other components sitting in the way, and
//! verify the committed result clears every obstacle at the board's
//! configured clearance -- the "tests on real IR boards ... check that
//! routes avoid obstacles with clearance" requirement for this stage.
//!
//! This builds its own small IR board (rather than loading a stored
//! `examples/*.yaml` pipeline output) so the fixture is self-contained,
//! deterministic, and independent of any other in-progress work elsewhere
//! in this workspace -- but it exercises exactly the same path real
//! `design.json` data does: `eda_drc::board::build`'s real pad-shape
//! flattening via `eda_pns::from_ir::build_node`.

use eda_drc::kimath::Shape;
use eda_model::ir::{Design, FootprintInstance, LabelSide, Millideg, PlacementSection, Point, Provenance, RoutingSection, Side, Um};
use eda_model::{ConstraintModel, Net as IrNet, Part, Pin, PinKind};
use eda_pns::item::net_of;
use eda_pns::line_placer::LinePlacer;
use eda_pns::settings::{Mode, RoutingSettings};

fn soic8_part(reference: &str) -> Part {
    Part {
        reference: reference.into(),
        mpn: None,
        lcsc: None,
        value: None,
        package: Some("SOIC-8".into()),
        footprint: Some("SOIC-8".into()),
        pins: (1..=8).map(|n| Pin { number: n.to_string(), name: None, kind: PinKind::Passive }).collect(),
        body_um: None,
        symbol: None,
        datasheet: None,
        edge: None,
    }
}

fn r0603_part(reference: &str) -> Part {
    Part {
        reference: reference.into(),
        mpn: None,
        lcsc: None,
        value: None,
        package: Some("0603".into()),
        footprint: Some("0603".into()),
        pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }],
        body_um: None,
        symbol: None,
        datasheet: None,
        edge: None,
    }
}

fn fp(id: &str, x: Um, y: Um, rot: Millideg) -> FootprintInstance {
    FootprintInstance { id: id.into(), at: Point { x, y }, rot, side: Side::Top, label: LabelSide::Above }
}

/// Two SOIC-8 ICs 10mm apart, both with pin 1 on the net "SIG", with three
/// 0603 resistors on an unrelated net ("GND") standing in a line directly
/// between them -- a real obstacle field a straight track cannot cross.
fn obstacle_course() -> (Design, ConstraintModel) {
    let model = ConstraintModel {
        parts: vec![soic8_part("U1"), soic8_part("U2"), r0603_part("R1"), r0603_part("R2"), r0603_part("R3")],
        nets: vec![IrNet { name: "SIG".into(), pins: vec!["U1.1".into(), "U2.1".into()] }, IrNet { name: "GND".into(), pins: vec!["R1.1".into(), "R1.2".into(), "R2.1".into(), "R2.2".into(), "R3.1".into(), "R3.2".into()] }],
        ..Default::default()
    };
    let design = Design {
        footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
        schema: 1,
        provenance: Provenance { engine_version: "test".into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(PlacementSection {
            outline: vec![],
            footprints: vec![fp("U1", 0, 0, 0), fp("U2", 10_000, 0, 180_000), fp("R1", 3_000, 0, 90_000), fp("R2", 5_000, 0, 90_000), fp("R3", 7_000, 0, 90_000)],
            modules: vec![],
        }),
        routing: Some(RoutingSection { tracks: vec![], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }),
        drawings: None,
    };
    (design, model)
}

#[test]
fn walkaround_route_clears_every_obstacle_on_a_realistic_board() {
    let (design, model) = obstacle_course();
    let (node, layers) = eda_pns::from_ir::build_node(&design, &model);
    let rules = &model.board;
    let settings = RoutingSettings { mode: Mode::Walkaround, ..RoutingSettings::default() };
    let layer = layers.index_of("F.Cu").expect("board must have F.Cu");

    // U1 pin 1 and U2 pin 1 centres, as `from_ir` placed them (SOIC-8's
    // pin 1 is the top of the left column -- see `footprint::dual_row`).
    let u1 = node.iter().find(|(_, it)| matches!(it, eda_pns::item::Item::Solid(s) if s.source == "U1.1")).map(|(_, it)| it.anchors()[0]).expect("U1.1 pad");
    let u2 = node.iter().find(|(_, it)| matches!(it, eda_pns::item::Item::Solid(s) if s.source == "U2.1")).map(|(_, it)| it.anchors()[0]).expect("U2.1 pad");

    let mut placer = LinePlacer::start(&node, u1, None, net_of("SIG"), layer, rules.width_of("SIG"));
    let outcome = placer.fix(&node, rules, &settings, u2);
    assert_eq!(outcome, eda_pns::line_placer::FixOutcome::Fixed { real_end: true }, "route must reach U2.1 in one leg; got preview {:?}", placer.preview(&node, rules, &settings, u2));
    // `real_end: true` means `fix` already committed the whole connection
    // into `placer.runs` -- no separate `finish()` call needed for a route
    // that snapped onto its target anchor via a plain click (see
    // `LinePlacer::finish`'s own doc comment for the one case that does
    // still need it: ending early, not on an anchor).
    let runs = placer.runs.clone();

    assert!(!runs.is_empty(), "must have committed at least one run");
    assert!(runs.iter().any(|l| l.point_count() > 2), "a real detour around three resistors must add vertices, not go straight through them");

    // Independent re-check: flatten the same board with eda_drc::board::build
    // (the DRC engine's own flattening) and verify every committed segment
    // clears every pad at the resolved clearance -- the same acceptance
    // bar DRC itself would apply.
    let drc_board = eda_drc::board::build(&design, &model);
    let clearance = rules.clearance_of("SIG");
    for run in &runs {
        for (a, b) in run.segs() {
            let seg_shape = Shape::Stadium { a, b, r: run.width / 2 };
            for pad in &drc_board.pads {
                if pad.net.as_deref() == Some("SIG") {
                    continue; // same net: touching your own net's pads is fine (e.g. landing on U1.1/U2.1)
                }
                if let Some((actual, pos)) = seg_shape.collides(&pad.copper, clearance) {
                    panic!("segment {:?}-{:?} violates clearance against pad {} (actual {actual}um < required {clearance}um) at {:?}", a, b, pad.id, pos);
                }
            }
        }
    }
}
