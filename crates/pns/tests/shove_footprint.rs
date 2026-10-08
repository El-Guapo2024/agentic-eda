//! Shove next to a footprint: the real SOIC-8 pad shapes (the footprint
//! library every board goes through) bound how far a track can be pushed.
//! Before the solids pre-pass and `onCollidingSolid` any pad within reach made
//! the shove give up and the placer fall back to walking around everything.

use eda_drc::kimath::Shape;
use eda_model::ir::{Design, FootprintInstance, LabelSide, PlacementSection, Point, Provenance, RoutingSection, Side, Track, Um};
use eda_model::{ConstraintModel, Net as IrNet, Part, Pin, PinKind};
use eda_pns::from_ir::build_node;
use eda_pns::item::net_of;
use eda_pns::line_placer::LinePlacer;
use eda_pns::settings::{Mode, RoutingSettings};

/// U1 (SOIC-8, left pad column at x -3450..-1500, rows 600 high at y = +-635 and +-1905) and a
/// GND track on its left, parallel to the pad column, 450 from the pads' edge.
fn board(track_x: Um) -> (Design, ConstraintModel) {
    let model = ConstraintModel {
        parts: vec![Part {
            reference: "U1".into(),
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
        }],
        nets: vec![IrNet { name: "VCC".into(), pins: vec!["U1.1".into(), "U1.2".into(), "U1.3".into(), "U1.4".into()] }],
        ..Default::default()
    };
    let mut design = Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: 1,
        provenance: Provenance { engine_version: "test".into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(PlacementSection { outline: vec![], footprints: vec![FootprintInstance { id: "U1".into(), at: Point { x: 0, y: 0 }, rot: 0, side: Side::Top, label: LabelSide::Above }], modules: vec![] }),
        routing: Some(RoutingSection {
            tracks: vec![Track { id: String::new(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![Point { x: track_x, y: -3_000 }, Point { x: track_x, y: 3_000 }], arc_mid_offset: None }],
            vias: vec![],
            zones: vec![],
            track_width_presets: vec![],
            via_presets: vec![],
            teardrop_settings: Default::default(),
        }),
        drawings: None,
    };
    design.assign_missing_ids();
    (design, model)
}

fn preview_head_at(track_x: Um, head_x: Um) -> (eda_pns::line_placer::Preview, ConstraintModel, Design) {
    let (design, model) = board(track_x);
    let (node, layers) = build_node(&design, &model);
    let settings = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
    let placer = LinePlacer::start(&node, Point { x: head_x, y: -4_000 }, None, net_of("SIG"), layers.index_of("F.Cu").unwrap(), model.board.width_of("SIG"));
    let pv = placer.preview(&node, &model.board, &settings, Point { x: head_x, y: 4_000 });
    (pv, model, design)
}

/// The head 300 from the track: the track is pushed 100 toward the pads, which still leave it room
/// (its window between the head and the pads is 50 wide).
#[test]
fn a_track_beside_the_pads_is_pushed_as_far_as_the_pads_allow() {
    let (pv, model, design) = preview_head_at(-3_900, -4_200);
    assert!(!pv.colliding);
    assert_eq!(pv.head.pts, vec![Point { x: -4_200, y: -4_000 }, Point { x: -4_200, y: 4_000 }], "the head goes straight, the track gives way");
    assert_eq!(pv.displaced_lines.len(), 1, "one track pushed");
    let moved = &pv.displaced_lines[0].line;
    assert_eq!(moved.width, 200);
    assert_eq!(moved.first().map(|p| p.y), Some(-3_000), "its ends stay where they were along the board");
    assert_eq!(moved.last().map(|p| p.y), Some(3_000));
    // clear of the head (200 + 100 + 100 from its centre line) and of every pad (200 + 100 from the edge)
    let drc = eda_drc::board::build(&design, &model);
    for p in &moved.pts {
        assert!(p.x >= -4_200 + 400 - 2, "still too close to the head: {p:?}");
    }
    for (a, b) in moved.segs() {
        for pad in &drc.pads {
            assert!(Shape::Stadium { a, b, r: 100 }.collides(&pad.copper, 200 - 1).is_none(), "the pushed track touches pad {}", pad.id);
        }
    }
}

/// The head 100 closer: the track can no longer keep both its distance from the head and from the
/// pads along the pad column. Where a pad stops it (`onCollidingSolid` walks the track along the pad's
/// clearance) the track outranks the head (a walked line is ranked above what it pushes), so the head
/// is pushed back in turn; between the pads they both come back to their line. Nothing ends up closer
/// than the clearance to anything.
#[test]
fn pads_that_stop_the_track_push_the_head_back_in_turn() {
    let (pv, model, design) = preview_head_at(-3_900, -4_000);
    assert!(!pv.colliding);
    assert_eq!(pv.displaced_lines.len(), 1, "the track was pushed");
    assert_eq!(pv.head.first(), Some(Point { x: -4_000, y: -4_000 }), "the head still starts and ends where it was asked to");
    assert_eq!(pv.head.last(), Some(Point { x: -4_000, y: 4_000 }));
    assert!(pv.head.pts.iter().any(|p| p.x < -4_000), "the head gave way along the pads: {:?}", pv.head.pts);
    let moved = &pv.displaced_lines[0].line;
    assert!(moved.pts.iter().any(|p| p.x > -3_700 && p.x < -3_500) && moved.pts.iter().any(|p| p.x <= -3_750), "the track follows the pads' edge: {:?}", moved.pts);

    let drc = eda_drc::board::build(&design, &model);
    let clear = |a: Point, b: Point, w: Um, other: &Shape| Shape::Stadium { a, b, r: w / 2 }.collides(other, 200 - 1).is_none();
    for (a, b) in pv.head.segs().map(|s| (s.0, s.1)).chain(moved.segs()) {
        for pad in &drc.pads {
            assert!(clear(a, b, 200, &pad.copper), "leg {a:?}-{b:?} touches pad {}", pad.id);
        }
    }
    for (a, b) in pv.head.segs() {
        for (c, d) in moved.segs() {
            assert!(clear(a, b, pv.head.width, &Shape::Stadium { a: c, b: d, r: moved.width / 2 }), "head leg {a:?}-{b:?} too close to the track leg {c:?}-{d:?}");
        }
    }
}
