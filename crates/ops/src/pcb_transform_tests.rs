//! `MoveItems`, `RotateItems` and `FlipItems`: every kind of item moves, turns and flips as KiCad's own item
//! classes do (`BOARD_ITEM::Move`/`Rotate`/`Flip`), and a footprint ends where the mirror image of its pads says.

use super::*;
use eda_model::footprint::{placed_pads, PlacedPad};
use eda_model::ir::{ArrowDirection, DimensionKind, DimensionTextPosition, DimensionUnits, DimensionUnitsFormat, PlacementSection, Provenance};
use eda_model::{Footprint, Net, Pad, PadKind, PadShape, Part, Pin, PinKind};

pub(super) fn p(x: Um, y: Um) -> Point {
    Point { x, y }
}

pub(super) fn id(s: &str) -> String {
    s.to_string()
}

/// Three pads, none on an axis of symmetry, so a mirror image cannot be mistaken for the original.
pub(super) fn lopsided() -> Footprint {
    let pad = |n: &str, at: (Um, Um)| Pad { opposite_side: false, number: n.into(), at, size: (600, 600), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None };
    Footprint { name: "LOPSIDED".into(), pads: vec![pad("1", (-1500, -600)), pad("2", (1500, 0)), pad("3", (-1500, 700))], courtyard: Some((4000, 2400)), model: None, courtyard_outlines: vec![] }
}

pub(super) fn part(r: &str) -> Part {
    Part {
        reference: r.into(),
        mpn: None,
        lcsc: None,
        value: None,
        package: Some("LOPSIDED".into()),
        footprint: Some("LOPSIDED".into()),
        pins: ["1", "2", "3"].iter().map(|n| Pin { number: (*n).into(), name: None, kind: PinKind::Passive }).collect(),
        body_um: None,
        symbol: None,
        datasheet: None,
        edge: None,
    }
}

pub(super) fn model(layers: &[&str]) -> ConstraintModel {
    let mut m = ConstraintModel {
        parts: vec![part("U1"), part("U2")],
        nets: vec![Net { name: "GND".into(), pins: vec!["U1.1".into()] }, Net { name: "VCC".into(), pins: vec!["U1.2".into()] }],
        footprints: vec![lopsided()],
        ..Default::default()
    };
    m.board.layers = layers.iter().map(|s| s.to_string()).collect();
    m
}

pub(super) fn track(id: &str, layer: &str, pts: &[(Um, Um)]) -> Track {
    Track { id: id.into(), net: "GND".into(), pins: vec![], layer: layer.into(), width: 200, pts: pts.iter().map(|&(x, y)| p(x, y)).collect(), arc_mid_offset: None }
}

pub(super) fn via(id: &str, at: (Um, Um), from: &str, to: &str) -> Via {
    Via { id: id.into(), net: "GND".into(), at: p(at.0, at.1), drill: 300, diameter: 600, from_layer: from.into(), to_layer: to.into() }
}

pub(super) fn zone(id: &str, layer: &str, outline: &[(Um, Um)]) -> Zone {
    Zone { id: id.into(), net: "GND".into(), layer: layer.into(), outline: outline.iter().map(|&(x, y)| p(x, y)).collect(), ..Default::default() }
}

pub(super) fn dimension(id: &str, kind: DimensionKind, start: Point, end: Point) -> Dimension {
    Dimension {
        id: id.into(),
        layer: "Dwgs.User".into(),
        kind,
        start,
        end,
        prefix: String::new(),
        suffix: String::new(),
        override_text: None,
        units: DimensionUnits::Mm,
        units_format: DimensionUnitsFormat::NoSuffix,
        precision: 2,
        suppress_trailing_zeros: true,
        text_position: DimensionTextPosition::Outside,
        keep_text_aligned: true,
        text_angle: 0,
        text_size_um: 1000,
        stroke_width: 150,
        arrow_length: 1000,
        extension_offset: 300,
        extension_height: 500,
        arrow_direction: ArrowDirection::Inward,
        text_thickness_um: None,
    }
}

pub(super) fn text(id: &str, layer: &str, at: (Um, Um), angle: Millideg) -> Text {
    Text { id: id.into(), content: "T".into(), at: p(at.0, at.1), angle, layer: layer.into(), size_um: 1000, stroke_width: 150, justify: TextJustify::Center, mirror: false }
}

/// A board with one of everything, on the given copper layers.
pub(super) fn design() -> Design {
    Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: 1,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(PlacementSection {
            outline: vec![p(0, 0), p(100_000, 0), p(100_000, 100_000), p(0, 100_000)],
            footprints: vec![
                FootprintInstance { id: "U1".into(), at: p(30_000, 20_000), rot: 0, side: Side::Top, label: LabelSide::Above },
                FootprintInstance { id: "U2".into(), at: p(60_000, 20_000), rot: 90_000, side: Side::Top, label: LabelSide::Left },
            ],
            modules: vec![],
        }),
        routing: Some(RoutingSection {
            tracks: vec![track("trk_a", "F.Cu", &[(10_000, 40_000), (20_000, 40_000), (20_000, 50_000)])],
            vias: vec![via("via_a", (20_000, 50_000), "F.Cu", "B.Cu")],
            zones: vec![zone("zone_a", "F.Cu", &[(0, 60_000), (10_000, 60_000), (10_000, 70_000), (0, 70_000)])],
            track_width_presets: vec![],
            via_presets: vec![],
            teardrop_settings: Default::default(),
        }),
        drawings: Some(DrawingsSection {
            shapes: vec![
                Shape::Segment { id: "shp_seg".into(), layer: "F.SilkS".into(), stroke_width: 150, filled: false, start: p(0, 80_000), end: p(10_000, 80_000) },
                Shape::Rect { id: "shp_rect".into(), layer: "F.Fab".into(), stroke_width: 100, filled: false, start: p(0, 90_000), end: p(10_000, 95_000) },
                Shape::Arc { id: "shp_arc".into(), layer: "F.SilkS".into(), stroke_width: 150, filled: false, start: p(40_000, 80_000), mid: p(45_000, 75_000), end: p(50_000, 80_000) },
            ],
            texts: vec![text("txt_a", "F.SilkS", (70_000, 80_000), 0)],
            dimensions: vec![dimension("dim_a", DimensionKind::Aligned { height: 2000 }, p(70_000, 50_000), p(80_000, 50_000))],
            ..Default::default()
        }),
    }
}

pub(super) fn board(m: &ConstraintModel) -> Board<'_> {
    Board::new(design(), m, 100, 300)
}

pub(super) fn routing<'a>(b: &'a Board<'_>) -> &'a RoutingSection {
    b.design().routing.as_ref().unwrap()
}
pub(super) fn drawings<'a>(b: &'a Board<'_>) -> &'a DrawingsSection {
    b.design().drawings.as_ref().unwrap()
}
pub(super) fn pose(b: &Board<'_>, r: &str) -> FootprintInstance {
    b.design().placement.as_ref().unwrap().footprints.iter().find(|f| f.id == r).unwrap().clone()
}
pub(super) fn pads_of(b: &Board<'_>, m: &ConstraintModel, r: &str) -> Vec<PlacedPad> {
    placed_pads(m, m.part(r).unwrap(), &pose(b, r)).unwrap()
}

pub(super) fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

pub(super) fn everything() -> Vec<String> {
    ids(&["U1", "U2", "trk_a", "via_a", "zone_a", "shp_seg", "shp_rect", "shp_arc", "txt_a", "dim_a"])
}

// ------------------------------------------------------------------- move

#[test]
fn move_items_moves_every_kind_by_the_same_delta_and_keeps_their_ids() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::MoveItems { ids: everything(), dx: 2_000, dy: -1_000 }).unwrap();

    assert_eq!(routing(&b).tracks[0].id, "trk_a");
    assert_eq!(routing(&b).tracks[0].pts, vec![p(12_000, 39_000), p(22_000, 39_000), p(22_000, 49_000)]);
    assert_eq!(routing(&b).vias[0].at, p(22_000, 49_000));
    assert_eq!(routing(&b).zones[0].outline[0], p(2_000, 59_000));
    let d = drawings(&b);
    assert_eq!(d.shapes.iter().find(|s| s.id() == "shp_seg").unwrap().points(), vec![p(2_000, 79_000), p(12_000, 79_000)]);
    assert_eq!(d.shapes.iter().find(|s| s.id() == "shp_arc").unwrap().points(), vec![p(42_000, 79_000), p(47_000, 74_000), p(52_000, 79_000)]);
    assert_eq!(d.texts[0].at, p(72_000, 79_000));
    assert_eq!((d.dimensions[0].start, d.dimensions[0].end), (p(72_000, 49_000), p(82_000, 49_000)));
    assert_eq!(pose(&b, "U1").at, p(32_000, 19_000));
    assert_eq!(pose(&b, "U2").at, p(62_000, 19_000));
    // Only a position changed.
    assert_eq!((pose(&b, "U2").rot, pose(&b, "U2").side), (90_000, Side::Top));
}

#[test]
fn move_items_moves_a_groups_members_and_a_member_named_twice_only_once() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().groups.push(Group { id: "grp_a".into(), name: String::new(), member_ids: ids(&["trk_a", "via_a", "U1"]) });
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::MoveItems { ids: ids(&["grp_a", "via_a"]), dx: 500, dy: 0 }).unwrap();
    assert_eq!(routing(&b).vias[0].at, p(20_500, 50_000), "named by the group and by itself: moved once");
    assert_eq!(routing(&b).tracks[0].pts[0], p(10_500, 40_000));
    assert_eq!(pose(&b, "U1").at, p(30_500, 20_000));
    assert_eq!(pose(&b, "U2").at, p(60_000, 20_000), "a footprint outside the group stays");
}

#[test]
fn a_moved_arc_track_is_still_an_arc() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    let arc = Track::new_arc("GND".into(), "F.Cu".into(), 200, p(10_000, 10_000), p(15_000, 5_000), p(20_000, 10_000));
    d.routing.as_mut().unwrap().tracks.push(Track { id: "trk_arc".into(), ..arc });
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::MoveItems { ids: ids(&["trk_arc"]), dx: 3_333, dy: 777 }).unwrap();
    let t = routing(&b).tracks.iter().find(|t| t.id == "trk_arc").unwrap();
    assert_eq!(t.arc(), Some((p(13_333, 10_777), p(18_333, 5_777), p(23_333, 10_777))));
}

#[test]
fn an_unknown_id_refuses_the_whole_command_and_nothing_moves() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let e = b.apply(&Cmd::MoveItems { ids: ids(&["trk_a", "no_such_item"]), dx: 1_000, dy: 0 }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_item");
    assert_eq!(routing(&b).tracks[0].pts[0], p(10_000, 40_000));
    let e = b.apply(&Cmd::MoveItems { ids: vec![], dx: 1_000, dy: 0 }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_transform");
}

// ------------------------------------------------------------------- rotate

#[test]
fn rotate_items_turns_copper_and_graphics_about_one_shared_pivot() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    // A quarter turn clockwise on the screen: (dx, dy) -> (-dy, dx).
    b.apply(&Cmd::RotateItems { ids: ids(&["trk_a", "via_a", "zone_a", "shp_seg", "txt_a", "dim_a"]), pivot: p(0, 0), angle_millideg: 90_000 }).unwrap();
    assert_eq!(routing(&b).tracks[0].pts, vec![p(-40_000, 10_000), p(-40_000, 20_000), p(-50_000, 20_000)]);
    assert_eq!(routing(&b).vias[0].at, p(-50_000, 20_000));
    assert_eq!(routing(&b).zones[0].outline[0], p(-60_000, 0));
    assert_eq!(drawings(&b).shapes.iter().find(|s| s.id() == "shp_seg").unwrap().points(), vec![p(-80_000, 0), p(-80_000, 10_000)]);
    assert_eq!(drawings(&b).dimensions[0].start, p(-50_000, 70_000));
}

#[test]
fn a_text_turned_clockwise_loses_the_same_angle_because_its_angle_is_kicads() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::RotateItems { ids: ids(&["txt_a"]), pivot: p(70_000, 80_000), angle_millideg: 90_000 }).unwrap();
    assert_eq!(drawings(&b).texts[0].at, p(70_000, 80_000), "it turns about its own anchor");
    assert_eq!(drawings(&b).texts[0].angle, 270_000, "KiCad angles run counter-clockwise: a clockwise quarter turn is 270");
    b.apply(&Cmd::RotateItems { ids: ids(&["txt_a"]), pivot: p(70_000, 80_000), angle_millideg: -90_000 }).unwrap();
    assert_eq!(drawings(&b).texts[0].angle, 0);
}

#[test]
fn a_footprint_turned_about_a_pivot_has_its_pads_where_the_same_turn_puts_them() {
    let m = model(&["F.Cu", "B.Cu"]);
    for (rot, side) in [(0, Side::Top), (90_000, Side::Top), (30_000, Side::Bottom), (270_000, Side::Bottom)] {
        let mut d = design();
        let fp = d.placement.as_mut().unwrap().footprints.iter_mut().find(|f| f.id == "U1").unwrap();
        fp.rot = rot;
        fp.side = side;
        let mut b = Board::new(d, &m, 100, 300);
        let before = pads_of(&b, &m, "U1");
        let pivot = p(35_000, 25_000);
        b.apply(&Cmd::RotateItems { ids: ids(&["U1"]), pivot, angle_millideg: 90_000 }).unwrap();
        let after = pads_of(&b, &m, "U1");
        for (was, now) in before.iter().zip(&after) {
            let want = rotate_point_about(was.center, pivot, 90_000);
            assert!((now.center.x - want.x).abs() <= 1 && (now.center.y - want.y).abs() <= 1, "pad {} of a part at rot {rot} {side:?}: {:?} -> {:?}, wanted {want:?}", was.number, was.center, now.center);
        }
        assert_eq!(pose(&b, "U1").rot, ((rot as i64 + 90_000) % 360_000) as u32);
    }
}

#[test]
fn a_rectangle_turned_a_quarter_stays_a_normalised_rectangle_and_any_other_turn_makes_it_a_polygon() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::RotateItems { ids: ids(&["shp_rect"]), pivot: p(5_000, 92_500), angle_millideg: 90_000 }).unwrap();
    let r = drawings(&b).shapes.iter().find(|s| s.id() == "shp_rect").unwrap();
    // 10 x 5 mm about its own centre: 5 x 10 mm, start at the top left.
    assert_eq!(r.points(), vec![p(2_500, 87_500), p(7_500, 97_500)]);

    let mut b = board(&m);
    b.apply(&Cmd::RotateItems { ids: ids(&["shp_rect"]), pivot: p(5_000, 92_500), angle_millideg: 45_000 }).unwrap();
    let r = drawings(&b).shapes.iter().find(|s| s.id() == "shp_rect").unwrap();
    assert!(matches!(r, Shape::Polygon { pts, .. } if pts.len() == 4), "{r:?}");
    assert_eq!(r.id(), "shp_rect", "it is still the same item");
}

#[test]
fn a_rotated_arc_track_is_still_an_arc_with_its_mid_point_turned() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    let arc = Track::new_arc("GND".into(), "F.Cu".into(), 200, p(10_000, 10_000), p(15_000, 5_000), p(20_000, 10_000));
    d.routing.as_mut().unwrap().tracks.push(Track { id: "trk_arc".into(), ..arc });
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::RotateItems { ids: ids(&["trk_arc"]), pivot: p(10_000, 10_000), angle_millideg: 90_000 }).unwrap();
    let t = routing(&b).tracks.iter().find(|t| t.id == "trk_arc").unwrap();
    assert_eq!(t.arc(), Some((p(10_000, 10_000), p(15_000, 15_000), p(10_000, 20_000))));
}

#[test]
fn an_orthogonal_dimension_swaps_its_axis_and_its_side_with_a_quarter_turn() {
    let m = model(&["F.Cu", "B.Cu"]);
    // KiCad: counter-clockwise 90 on a horizontal crossbar makes it vertical, height unchanged; the same turn on a vertical one makes it
    // horizontal with the height negated. Ours is clockwise-positive, so the signs below are KiCad's, negated.
    let mut d = design();
    d.drawings.as_mut().unwrap().dimensions = vec![dimension("o_h", DimensionKind::Orthogonal { height: 3_000, horizontal: true }, p(0, 0), p(10_000, 5_000)), dimension("o_v", DimensionKind::Orthogonal { height: 3_000, horizontal: false }, p(0, 0), p(10_000, 5_000))];
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::RotateItems { ids: ids(&["o_h", "o_v"]), pivot: p(0, 0), angle_millideg: -90_000 }).unwrap();
    let kinds: Vec<DimensionKind> = drawings(&b).dimensions.iter().map(|d| d.kind).collect();
    assert_eq!(kinds, vec![DimensionKind::Orthogonal { height: 3_000, horizontal: false }, DimensionKind::Orthogonal { height: -3_000, horizontal: true }]);
    // Half a turn only changes the side.
    b.apply(&Cmd::RotateItems { ids: ids(&["o_h"]), pivot: p(0, 0), angle_millideg: 180_000 }).unwrap();
    assert_eq!(drawings(&b).dimensions[0].kind, DimensionKind::Orthogonal { height: -3_000, horizontal: false });
}

// --------------------------------------------------------------------- flip

#[test]
fn flip_items_mirrors_copper_across_the_axis_and_puts_it_on_the_other_layer() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::FlipItems { ids: ids(&["trk_a", "via_a", "zone_a"]), pivot: p(50_000, 0), direction: FlipDirection::LeftRight }).unwrap();
    let t = &routing(&b).tracks[0];
    assert_eq!((t.layer.as_str(), t.pts.clone()), ("B.Cu", vec![p(90_000, 40_000), p(80_000, 40_000), p(80_000, 50_000)]));
    assert_eq!(routing(&b).vias[0].at, p(80_000, 50_000));
    assert_eq!((routing(&b).vias[0].from_layer.as_str(), routing(&b).vias[0].to_layer.as_str()), ("F.Cu", "B.Cu"), "a through via has no layer pair to change");
    let z = &routing(&b).zones[0];
    assert_eq!((z.layer.as_str(), z.outline[0]), ("B.Cu", p(100_000, 60_000)));

    // Top-bottom mirrors y instead.
    let mut b = board(&m);
    b.apply(&Cmd::FlipItems { ids: ids(&["trk_a"]), pivot: p(0, 40_000), direction: FlipDirection::TopBottom }).unwrap();
    assert_eq!(routing(&b).tracks[0].pts, vec![p(10_000, 40_000), p(20_000, 40_000), p(20_000, 30_000)]);
}

#[test]
fn a_flipped_blind_via_changes_its_layer_pair_and_inner_layers_mirror_through_the_stack() {
    let m = model(&["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]);
    let mut d = design();
    d.routing.as_mut().unwrap().vias.push(via("via_blind", (30_000, 30_000), "F.Cu", "In1.Cu"));
    d.routing.as_mut().unwrap().tracks.push(track("trk_in", "In1.Cu", &[(0, 0), (1_000, 0)]));
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::FlipItems { ids: ids(&["via_blind", "trk_in"]), pivot: p(0, 0), direction: FlipDirection::LeftRight }).unwrap();
    let v = routing(&b).vias.iter().find(|v| v.id == "via_blind").unwrap();
    assert_eq!((v.from_layer.as_str(), v.to_layer.as_str()), ("B.Cu", "In2.Cu"));
    assert_eq!(routing(&b).tracks.iter().find(|t| t.id == "trk_in").unwrap().layer, "In2.Cu");
}

#[test]
fn layers_flip_as_kicads_flip_layer_does() {
    assert_eq!(flip_layer("F.Cu", 2), "B.Cu");
    assert_eq!(flip_layer("B.SilkS", 2), "F.SilkS");
    assert_eq!(flip_layer("F.CrtYd", 2), "B.CrtYd");
    assert_eq!(flip_layer("Edge.Cuts", 2), "Edge.Cuts");
    assert_eq!(flip_layer("Dwgs.User", 4), "Dwgs.User");
    assert_eq!((flip_layer("In1.Cu", 4).as_str(), flip_layer("In2.Cu", 4).as_str()), ("In2.Cu", "In1.Cu"));
    assert_eq!(
        ["In1.Cu", "In2.Cu", "In3.Cu", "In4.Cu"].map(|l| flip_layer(l, 6)),
        ["In4.Cu".to_string(), "In3.Cu".to_string(), "In2.Cu".to_string(), "In1.Cu".to_string()]
    );
    assert_eq!(flip_layer("In1.Cu", 2), "In1.Cu", "a two layer board has no inner layers to swap");
}

#[test]
fn flipped_graphics_mirror_swap_an_arcs_ends_and_change_layer() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::FlipItems { ids: ids(&["shp_seg", "shp_rect", "shp_arc"]), pivot: p(0, 0), direction: FlipDirection::LeftRight }).unwrap();
    let d = drawings(&b);
    let seg = d.shapes.iter().find(|s| s.id() == "shp_seg").unwrap();
    assert_eq!((seg.layer(), seg.points()), ("B.SilkS", vec![p(0, 80_000), p(-10_000, 80_000)]));
    let rect = d.shapes.iter().find(|s| s.id() == "shp_rect").unwrap();
    assert_eq!((rect.layer(), rect.points()), ("B.Fab", vec![p(-10_000, 90_000), p(0, 95_000)]), "normalised: start at the top left");
    let arc = d.shapes.iter().find(|s| s.id() == "shp_arc").unwrap();
    assert_eq!(arc.points(), vec![p(-50_000, 80_000), p(-45_000, 75_000), p(-40_000, 80_000)], "start and end swapped, still the same arc");
}

#[test]
fn a_flipped_text_changes_layer_negates_its_angle_and_reads_mirrored() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().texts.push(text("txt_b", "F.SilkS", (0, 0), 30_000));
    d.drawings.as_mut().unwrap().texts.push(text("txt_user", "Dwgs.User", (0, 0), 30_000));
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::FlipItems { ids: ids(&["txt_b", "txt_user"]), pivot: p(0, 0), direction: FlipDirection::LeftRight }).unwrap();
    let t = drawings(&b).texts.iter().find(|t| t.id == "txt_b").unwrap();
    assert_eq!((t.layer.as_str(), t.angle, t.mirror), ("B.SilkS", 330_000, true));
    let u = drawings(&b).texts.iter().find(|t| t.id == "txt_user").unwrap();
    assert_eq!((u.layer.as_str(), u.mirror), ("Dwgs.User", false), "a layer that belongs to no side does not mirror");
    b.apply(&Cmd::FlipItems { ids: ids(&["txt_b"]), pivot: p(0, 0), direction: FlipDirection::TopBottom }).unwrap();
    let t = drawings(&b).texts.iter().find(|t| t.id == "txt_b").unwrap();
    assert_eq!((t.layer.as_str(), t.angle, t.mirror), ("F.SilkS", 210_000, false), "180 - 330 = -150");
}

#[test]
fn a_flipped_dimension_moves_its_crossbar_side_with_the_mirror() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().dimensions = vec![
        dimension("aligned", DimensionKind::Aligned { height: 2_000 }, p(10_000, 0), p(20_000, 0)),
        dimension("o_h", DimensionKind::Orthogonal { height: 3_000, horizontal: true }, p(10_000, 0), p(20_000, 5_000)),
        dimension("o_v", DimensionKind::Orthogonal { height: 3_000, horizontal: false }, p(10_000, 0), p(20_000, 5_000)),
    ];
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::FlipItems { ids: ids(&["aligned", "o_h", "o_v"]), pivot: p(0, 0), direction: FlipDirection::LeftRight }).unwrap();
    let by_id = |name: &str| drawings(&b).dimensions.iter().find(|d| d.id == name).unwrap().clone();
    assert_eq!(by_id("aligned").kind, DimensionKind::Aligned { height: -2_000 });
    assert_eq!((by_id("aligned").start, by_id("aligned").end), (p(-10_000, 0), p(-20_000, 0)));
    assert_eq!(by_id("o_h").kind, DimensionKind::Orthogonal { height: 3_000, horizontal: true }, "a left-right flip keeps a horizontal crossbar's side");
    assert_eq!(by_id("o_v").kind, DimensionKind::Orthogonal { height: -3_000, horizontal: false }, "and swaps a vertical one's");
    assert_eq!(by_id("aligned").layer, "Dwgs.User");
}

#[test]
fn a_flipped_footprint_has_every_pad_where_the_mirror_image_puts_it() {
    let m = model(&["F.Cu", "B.Cu"]);
    for dir in [FlipDirection::LeftRight, FlipDirection::TopBottom] {
        for (rot, side) in [(0, Side::Top), (90_000, Side::Top), (180_000, Side::Top), (270_000, Side::Top), (30_000, Side::Top), (0, Side::Bottom), (90_000, Side::Bottom), (210_000, Side::Bottom)] {
            let mut d = design();
            let fp = d.placement.as_mut().unwrap().footprints.iter_mut().find(|f| f.id == "U1").unwrap();
            fp.rot = rot;
            fp.side = side;
            let mut b = Board::new(d, &m, 100, 300);
            let before = pads_of(&b, &m, "U1");
            let pivot = p(35_000, 26_000);
            b.apply(&Cmd::FlipItems { ids: ids(&["U1"]), pivot, direction: dir }).unwrap();
            let after = pads_of(&b, &m, "U1");
            let new_pose = pose(&b, "U1");
            assert_eq!(new_pose.side, if side == Side::Top { Side::Bottom } else { Side::Top });
            for (was, now) in before.iter().zip(&after) {
                let want = match dir {
                    FlipDirection::LeftRight => p(2 * pivot.x - was.center.x, was.center.y),
                    FlipDirection::TopBottom => p(was.center.x, 2 * pivot.y - was.center.y),
                };
                assert!((now.center.x - want.x).abs() <= 1 && (now.center.y - want.y).abs() <= 1, "{dir:?}, part at rot {rot} {side:?}, pad {}: {:?} -> {:?}, the mirror image is {want:?}", was.number, was.center, now.center);
            }
        }
    }
}

#[test]
fn flipping_a_footprint_twice_gives_it_back_and_swaps_its_label_side_both_ways() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let was = pose(&b, "U2");
    b.apply(&Cmd::FlipItems { ids: ids(&["U2"]), pivot: p(50_000, 0), direction: FlipDirection::LeftRight }).unwrap();
    let once = pose(&b, "U2");
    assert_eq!((once.at, once.rot, once.side, once.label), (p(40_000, 20_000), 270_000, Side::Bottom, LabelSide::Right));
    b.apply(&Cmd::FlipItems { ids: ids(&["U2"]), pivot: p(50_000, 0), direction: FlipDirection::LeftRight }).unwrap();
    let twice = pose(&b, "U2");
    assert_eq!((twice.at, twice.rot, twice.side, twice.label), (was.at, was.rot, was.side, was.label));
}

// ------------------------------------------------------------ the routing

#[test]
fn only_a_footprint_in_the_selection_clears_the_routing() {
    let d = design();
    let copper = Cmd::MoveItems { ids: ids(&["trk_a", "via_a", "zone_a", "shp_seg", "txt_a", "dim_a"]), dx: 1, dy: 1 };
    assert!(!copper.clears_routing_in(&d), "copper and graphics alone cannot be moved out from under a track");
    assert!(!Cmd::RotateItems { ids: ids(&["trk_a"]), pivot: p(0, 0), angle_millideg: 90_000 }.clears_routing_in(&d));
    assert!(Cmd::MoveItems { ids: ids(&["trk_a", "U1"]), dx: 1, dy: 1 }.clears_routing_in(&d));
    assert!(Cmd::FlipItems { ids: ids(&["U2"]), pivot: p(0, 0), direction: FlipDirection::LeftRight }.clears_routing_in(&d));

    let mut with_group = design();
    with_group.drawings.as_mut().unwrap().groups.push(Group { id: "grp_a".into(), name: String::new(), member_ids: ids(&["trk_a", "U1"]) });
    assert!(Cmd::MoveItems { ids: ids(&["grp_a"]), dx: 1, dy: 1 }.clears_routing_in(&with_group), "a group's footprint members count");
    assert!(Cmd::Batch { cmds: vec![copper.clone(), Cmd::MoveItems { ids: ids(&["U1"]), dx: 1, dy: 1 }] }.clears_routing_in(&d));
    assert!(!Cmd::Batch { cmds: vec![copper] }.clears_routing_in(&d));
    // The old verbs keep their old answer.
    assert!(Cmd::MoveTo { part: id("U1"), x: 0, y: 0 }.clears_routing_in(&d));
}

#[test]
fn the_new_verbs_round_trip_through_json_as_the_studio_sends_them() {
    let cmds = [
        Cmd::MoveItems { ids: ids(&["trk_a"]), dx: -1_500, dy: 20 },
        Cmd::RotateItems { ids: ids(&["trk_a", "U1"]), pivot: p(1, 2), angle_millideg: -90_000 },
        Cmd::FlipItems { ids: ids(&["zone_a"]), pivot: p(3, 4), direction: FlipDirection::TopBottom },
    ];
    for c in cmds {
        let json = serde_json::to_string(&c).unwrap();
        let back: Cmd = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c, "{json}");
    }
    let from_studio: Cmd = serde_json::from_str(r#"{"op":"flip_items","ids":["a"],"pivot":{"x":5,"y":6},"direction":"left_right"}"#).unwrap();
    assert_eq!(from_studio, Cmd::FlipItems { ids: ids(&["a"]), pivot: p(5, 6), direction: FlipDirection::LeftRight });
}
