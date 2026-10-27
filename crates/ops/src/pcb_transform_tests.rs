//! `MoveItems`, `RotateItems` and `FlipItems`: every kind of item moves, turns and flips as KiCad's own item
//! classes do (`BOARD_ITEM::Move`/`Rotate`/`Flip`), and a footprint ends where the mirror image of its pads says.

use super::*;
use eda_model::footprint::{placed_pads, PlacedPad};
use eda_model::ir::{ArrowDirection, DimensionKind, DimensionTextPosition, DimensionUnits, DimensionUnitsFormat, PlacementSection, Provenance};
use eda_model::{Footprint, Net, Pad, PadKind, PadShape, Part, Pin, PinKind};
use eda_model::units::{mm, um};

pub(super) fn p(x: Nm, y: Nm) -> Point {
    Point { x, y }
}

pub(super) fn id(s: &str) -> String {
    s.to_string()
}

/// Three pads, none on an axis of symmetry, so a mirror image cannot be mistaken for the original.
pub(super) fn lopsided() -> Footprint {
    let pad = |n: &str, at: (Nm, Nm)| Pad { opposite_side: false, ext: Default::default(), number: n.into(), at, size: (mm(0.6), mm(0.6)), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None };
    Footprint { name: "LOPSIDED".into(), pads: vec![pad("1", (mm(-1.5), mm(-0.6))), pad("2", (mm(1.5), Nm::ZERO)), pad("3", (mm(-1.5), mm(0.7)))], courtyard: Some((mm(4.0), mm(2.4))), model: None, courtyard_outlines: vec![], models3d: vec![], art: Default::default() }
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
        body_nm: None,
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

pub(super) fn track(id: &str, layer: &str, pts: &[(Nm, Nm)]) -> Track {
    Track { id: id.into(), net: "GND".into(), pins: vec![], layer: layer.into(), width: mm(0.2), pts: pts.iter().map(|&(x, y)| p(x, y)).collect(), arc_mid_offset: None }
}

pub(super) fn via(id: &str, at: (Nm, Nm), from: &str, to: &str) -> Via {
    Via { id: id.into(), net: "GND".into(), at: p(at.0, at.1), drill: mm(0.3), diameter: mm(0.6), from_layer: from.into(), to_layer: to.into() }
}

pub(super) fn zone(id: &str, layer: &str, outline: &[(Nm, Nm)]) -> Zone {
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
        text_size_nm: mm(1.0),
        stroke_width: mm(0.15),
        arrow_length: mm(1.0),
        extension_offset: mm(0.3),
        extension_height: mm(0.5),
        arrow_direction: ArrowDirection::Inward,
        text_pos: None,
        text_frame: Default::default(),
        text_thickness_nm: None,
    }
}

pub(super) fn text(id: &str, layer: &str, at: (Nm, Nm), angle: Millideg) -> Text {
    Text { id: id.into(), content: "T".into(), at: p(at.0, at.1), angle, layer: layer.into(), size_nm: mm(1.0), stroke_width: mm(0.15), justify: TextJustify::Center, mirror: false }
}

/// A board with one of everything, on the given copper layers.
pub(super) fn design() -> Design {
    Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(PlacementSection {
            outline: vec![p(Nm::ZERO, Nm::ZERO), p(mm(100.0), Nm::ZERO), p(mm(100.0), mm(100.0)), p(Nm::ZERO, mm(100.0))],
            footprints: vec![
                FootprintInstance { id: "U1".into(), at: p(mm(30.0), mm(20.0)), rot: 0, side: Side::Top, label: LabelSide::Above },
                FootprintInstance { id: "U2".into(), at: p(mm(60.0), mm(20.0)), rot: 90_000, side: Side::Top, label: LabelSide::Left },
            ],
            modules: vec![],
        }),
        routing: Some(RoutingSection {
            tracks: vec![track("trk_a", "F.Cu", &[(mm(10.0), mm(40.0)), (mm(20.0), mm(40.0)), (mm(20.0), mm(50.0))])],
            vias: vec![via("via_a", (mm(20.0), mm(50.0)), "F.Cu", "B.Cu")],
            zones: vec![zone("zone_a", "F.Cu", &[(Nm::ZERO, mm(60.0)), (mm(10.0), mm(60.0)), (mm(10.0), mm(70.0)), (Nm::ZERO, mm(70.0))])],
            track_width_presets: vec![],
            via_presets: vec![],
            diff_pair_presets: vec![],
            teardrop_settings: Default::default(),
        }),
        drawings: Some(DrawingsSection {
            shapes: vec![
                Shape::Segment { id: "shp_seg".into(), layer: "F.SilkS".into(), stroke_width: mm(0.15), filled: false, start: p(Nm::ZERO, mm(80.0)), end: p(mm(10.0), mm(80.0)) },
                Shape::Rect { id: "shp_rect".into(), layer: "F.Fab".into(), stroke_width: mm(0.1), filled: false, start: p(Nm::ZERO, mm(90.0)), end: p(mm(10.0), mm(95.0)) },
                Shape::Arc { id: "shp_arc".into(), layer: "F.SilkS".into(), stroke_width: mm(0.15), filled: false, start: p(mm(40.0), mm(80.0)), mid: p(mm(45.0), mm(75.0)), end: p(mm(50.0), mm(80.0)) },
            ],
            texts: vec![text("txt_a", "F.SilkS", (mm(70.0), mm(80.0)), 0)],
            dimensions: vec![dimension("dim_a", DimensionKind::Aligned { height: mm(2.0) }, p(mm(70.0), mm(50.0)), p(mm(80.0), mm(50.0)))],
            ..Default::default()
        }),
    }
}

pub(super) fn board(m: &ConstraintModel) -> Board<'_> {
    Board::new(design(), m, mm(0.1), mm(0.3))
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
    b.apply(&Cmd::MoveItems { ids: everything(), dx: mm(2.0), dy: mm(-1.0) }).unwrap();

    assert_eq!(routing(&b).tracks[0].id, "trk_a");
    assert_eq!(routing(&b).tracks[0].pts, vec![p(mm(12.0), mm(39.0)), p(mm(22.0), mm(39.0)), p(mm(22.0), mm(49.0))]);
    assert_eq!(routing(&b).vias[0].at, p(mm(22.0), mm(49.0)));
    assert_eq!(routing(&b).zones[0].outline[0], p(mm(2.0), mm(59.0)));
    let d = drawings(&b);
    assert_eq!(d.shapes.iter().find(|s| s.id() == "shp_seg").unwrap().points(), vec![p(mm(2.0), mm(79.0)), p(mm(12.0), mm(79.0))]);
    assert_eq!(d.shapes.iter().find(|s| s.id() == "shp_arc").unwrap().points(), vec![p(mm(42.0), mm(79.0)), p(mm(47.0), mm(74.0)), p(mm(52.0), mm(79.0))]);
    assert_eq!(d.texts[0].at, p(mm(72.0), mm(79.0)));
    assert_eq!((d.dimensions[0].start, d.dimensions[0].end), (p(mm(72.0), mm(49.0)), p(mm(82.0), mm(49.0))));
    assert_eq!(pose(&b, "U1").at, p(mm(32.0), mm(19.0)));
    assert_eq!(pose(&b, "U2").at, p(mm(62.0), mm(19.0)));
    // Only a position changed.
    assert_eq!((pose(&b, "U2").rot, pose(&b, "U2").side), (90_000, Side::Top));
}

/// `FOOTPRINT::Move` (`m_pos + aMoveVector`) and `FOOTPRINT::Rotate` put the footprint exactly where the edit says: Move Exactly by 1.25 mm, or the
/// Properties panel's Position X typed as 12.95, moves it by 1.25 mm, not to the placer's 0.1 mm grid (the tool snapped the pointer already).
#[test]
fn move_and_rotate_items_put_a_footprint_where_the_edit_says_off_the_placement_grid() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::MoveItems { ids: ids(&["U1"]), dx: mm(1.25), dy: mm(-0.03) }).unwrap();
    assert_eq!(pose(&b, "U1").at, p(mm(31.25), mm(19.97)), "moved by the delta, to the um");
    // Turned about a pivot the footprint is not on: the position is the rotated one, unrounded.
    b.apply(&Cmd::RotateItems { ids: ids(&["U1"]), pivot: p(mm(30.0), mm(20.0)), angle_millideg: 90_000 }).unwrap();
    let at = pose(&b, "U1").at;
    assert_eq!((at.x - mm(30.0)).abs() + (at.y - mm(20.0)).abs(), mm(1.25) + mm(0.03), "about the pivot, the offset kept: {at:?}");
    assert!(at.x % mm(0.1) != Nm::ZERO || at.y % mm(0.1) != Nm::ZERO, "not snapped to the 0.1 mm placement grid: {at:?}");
}

#[test]
fn move_items_moves_a_groups_members_and_a_member_named_twice_only_once() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().groups.push(Group { id: "grp_a".into(), name: String::new(), member_ids: ids(&["trk_a", "via_a", "U1"]), design_block: String::new() });
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::MoveItems { ids: ids(&["grp_a", "via_a"]), dx: mm(0.5), dy: Nm::ZERO }).unwrap();
    assert_eq!(routing(&b).vias[0].at, p(mm(20.5), mm(50.0)), "named by the group and by itself: moved once");
    assert_eq!(routing(&b).tracks[0].pts[0], p(mm(10.5), mm(40.0)));
    assert_eq!(pose(&b, "U1").at, p(mm(30.5), mm(20.0)));
    assert_eq!(pose(&b, "U2").at, p(mm(60.0), mm(20.0)), "a footprint outside the group stays");
}

/// p6 F2: the point editor and Mirror change an item in place (`commit.Modify( item )`), so it keeps its id and its group: the studio's batches for a
/// handle drag of a grouped shape (`replace_shape`) and for Mirror of a grouped track (`mirror_tracks`) leave the group of two whole.
#[test]
fn point_edit_of_a_grouped_shape_and_mirror_of_a_grouped_track_keep_them_in_their_group() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().groups.push(Group { id: "grp_a".into(), name: String::new(), member_ids: ids(&["shp_seg", "trk_a"]), design_block: String::new() });
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let dragged = Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.15), filled: false, start: p(Nm::ZERO, mm(80.0)), end: p(mm(12.0), mm(81.0)) };
    b.apply(&Cmd::Batch { cmds: vec![Cmd::ReplaceShape { id: "shp_seg".into(), shape: dragged }] }).unwrap();
    b.apply(&Cmd::MirrorTracks { ids: ids(&["trk_a"]), pivot: p(mm(15.0), Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    let g = drawings(&b).groups.iter().find(|g| g.id == "grp_a").expect("the group is still there");
    assert_eq!(g.member_ids, ids(&["shp_seg", "trk_a"]));
    assert_eq!(drawings(&b).shapes.iter().find(|s| s.id() == "shp_seg").unwrap().points(), vec![p(Nm::ZERO, mm(80.0)), p(mm(12.0), mm(81.0))]);
    // `PCB_TRACK::Mirror`: the points mirror across x = 15 mm, the layer stays (Mirror is not Flip)
    let t = &routing(&b).tracks[0];
    assert_eq!((t.id.as_str(), t.layer.as_str(), t.pts.clone()), ("trk_a", "F.Cu", vec![p(mm(20.0), mm(40.0)), p(mm(10.0), mm(40.0)), p(mm(10.0), mm(50.0))]));
    // an arc stays an arc, mirrored
    let mut d = design();
    let arc = Track::new_arc("GND".into(), "F.Cu".into(), mm(0.2), p(mm(10.0), mm(10.0)), p(mm(15.0), mm(5.0)), p(mm(20.0), mm(10.0)));
    d.routing.as_mut().unwrap().tracks.push(Track { id: "trk_arc".into(), ..arc });
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::MirrorTracks { ids: ids(&["trk_arc"]), pivot: p(Nm::ZERO, mm(20.0)), direction: FlipDirection::TopBottom }).unwrap();
    let t = routing(&b).tracks.iter().find(|t| t.id == "trk_arc").unwrap();
    assert_eq!(t.arc(), Some((p(mm(10.0), mm(30.0)), p(mm(15.0), mm(35.0)), p(mm(20.0), mm(30.0)))));
    assert_eq!(b.apply(&Cmd::MirrorTracks { ids: ids(&["trk_nope"]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::TopBottom }).unwrap_err()[0].check, "ops_unknown_track");
}

#[test]
fn a_moved_arc_track_is_still_an_arc() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    let arc = Track::new_arc("GND".into(), "F.Cu".into(), mm(0.2), p(mm(10.0), mm(10.0)), p(mm(15.0), mm(5.0)), p(mm(20.0), mm(10.0)));
    d.routing.as_mut().unwrap().tracks.push(Track { id: "trk_arc".into(), ..arc });
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::MoveItems { ids: ids(&["trk_arc"]), dx: mm(3.333), dy: mm(0.777) }).unwrap();
    let t = routing(&b).tracks.iter().find(|t| t.id == "trk_arc").unwrap();
    assert_eq!(t.arc(), Some((p(mm(13.333), mm(10.777)), p(mm(18.333), mm(5.777)), p(mm(23.333), mm(10.777)))));
}

#[test]
fn an_unknown_id_refuses_the_whole_command_and_nothing_moves() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let e = b.apply(&Cmd::MoveItems { ids: ids(&["trk_a", "no_such_item"]), dx: mm(1.0), dy: Nm::ZERO }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_item");
    assert_eq!(routing(&b).tracks[0].pts[0], p(mm(10.0), mm(40.0)));
    let e = b.apply(&Cmd::MoveItems { ids: vec![], dx: mm(1.0), dy: Nm::ZERO }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_transform");
}

// ------------------------------------------------------------------- rotate

#[test]
fn rotate_items_turns_copper_and_graphics_about_one_shared_pivot() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    // A quarter turn clockwise on the screen: (dx, dy) -> (-dy, dx).
    b.apply(&Cmd::RotateItems { ids: ids(&["trk_a", "via_a", "zone_a", "shp_seg", "txt_a", "dim_a"]), pivot: p(Nm::ZERO, Nm::ZERO), angle_millideg: 90_000 }).unwrap();
    assert_eq!(routing(&b).tracks[0].pts, vec![p(mm(-40.0), mm(10.0)), p(mm(-40.0), mm(20.0)), p(mm(-50.0), mm(20.0))]);
    assert_eq!(routing(&b).vias[0].at, p(mm(-50.0), mm(20.0)));
    assert_eq!(routing(&b).zones[0].outline[0], p(mm(-60.0), Nm::ZERO));
    assert_eq!(drawings(&b).shapes.iter().find(|s| s.id() == "shp_seg").unwrap().points(), vec![p(mm(-80.0), Nm::ZERO), p(mm(-80.0), mm(10.0))]);
    assert_eq!(drawings(&b).dimensions[0].start, p(mm(-50.0), mm(70.0)));
}

#[test]
fn a_text_turned_clockwise_loses_the_same_angle_because_its_angle_is_kicads() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::RotateItems { ids: ids(&["txt_a"]), pivot: p(mm(70.0), mm(80.0)), angle_millideg: 90_000 }).unwrap();
    assert_eq!(drawings(&b).texts[0].at, p(mm(70.0), mm(80.0)), "it turns about its own anchor");
    assert_eq!(drawings(&b).texts[0].angle, 270_000, "KiCad angles run counter-clockwise: a clockwise quarter turn is 270");
    b.apply(&Cmd::RotateItems { ids: ids(&["txt_a"]), pivot: p(mm(70.0), mm(80.0)), angle_millideg: -90_000 }).unwrap();
    assert_eq!(drawings(&b).texts[0].angle, 0);
}

#[test]
fn rotating_a_dimension_turns_its_text_the_way_a_text_turns() {
    // `PCB_DIMENSION_BASE::Rotate`: `newAngle = GetTextAngle() + aAngle` with KiCad's counter-clockwise angle, the same
    // way `PCB_TEXT::Rotate` turns a text. Our clockwise-positive -90 is KiCad's +90.
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    let dims = &mut d.drawings.as_mut().unwrap().dimensions;
    dims[0].keep_text_aligned = false;
    dims[0].text_angle = 0;
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::RotateItems { ids: ids(&["dim_a", "txt_a"]), pivot: p(mm(70.0), mm(80.0)), angle_millideg: -90_000 }).unwrap();
    assert_eq!(drawings(&b).texts[0].angle, 90_000);
    assert_eq!(drawings(&b).dimensions[0].text_angle, 90_000, "the dimension's text turns with the text");
}

#[test]
fn a_footprint_turned_about_a_pivot_has_its_pads_where_the_same_turn_puts_them() {
    let m = model(&["F.Cu", "B.Cu"]);
    for (rot, side) in [(0, Side::Top), (90_000, Side::Top), (30_000_000, Side::Bottom), (270_000, Side::Bottom)] {
        let mut d = design();
        let fp = d.placement.as_mut().unwrap().footprints.iter_mut().find(|f| f.id == "U1").unwrap();
        fp.rot = rot;
        fp.side = side;
        let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
        let before = pads_of(&b, &m, "U1");
        let pivot = p(mm(35.0), mm(25.0));
        b.apply(&Cmd::RotateItems { ids: ids(&["U1"]), pivot, angle_millideg: 90_000 }).unwrap();
        let after = pads_of(&b, &m, "U1");
        for (was, now) in before.iter().zip(&after) {
            let want = rotate_point_about(was.center, pivot, 90_000);
            assert!((now.center.x - want.x).abs() <= Nm::new(1) && (now.center.y - want.y).abs() <= Nm::new(1), "pad {} of a part at rot {rot} {side:?}: {:?} -> {:?}, wanted {want:?}", was.number, was.center, now.center);
        }
        assert_eq!(pose(&b, "U1").rot, ((rot as i64 + 90_000) % 360_000) as u32);
    }
}

#[test]
fn a_rectangle_turned_a_quarter_stays_a_normalised_rectangle_and_any_other_turn_makes_it_a_polygon() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::RotateItems { ids: ids(&["shp_rect"]), pivot: p(mm(5.0), mm(92.5)), angle_millideg: 90_000 }).unwrap();
    let r = drawings(&b).shapes.iter().find(|s| s.id() == "shp_rect").unwrap();
    // 10 x 5 mm about its own centre: 5 x 10 mm, start at the top left.
    assert_eq!(r.points(), vec![p(mm(2.5), mm(87.5)), p(mm(7.5), mm(97.5))]);

    let mut b = board(&m);
    b.apply(&Cmd::RotateItems { ids: ids(&["shp_rect"]), pivot: p(mm(5.0), mm(92.5)), angle_millideg: 45_000 }).unwrap();
    let r = drawings(&b).shapes.iter().find(|s| s.id() == "shp_rect").unwrap();
    assert!(matches!(r, Shape::Polygon { pts, .. } if pts.len() == 4), "{r:?}");
    assert_eq!(r.id(), "shp_rect", "it is still the same item");
}

#[test]
fn a_rounded_rectangle_turned_45_degrees_is_the_polygon_of_its_rounded_rectangle() {
    // `EDA_SHAPE::rotate`: `ROUNDRECT rr( SHAPE_RECT( .. ), m_cornerRadius ); m_shape = SHAPE_T::POLY; rr.TransformToPolygon( .. ); .Rotate( .. )`.
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().board_items.set_corner_radius("shp_rect", mm(2.0));
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let pivot = p(mm(5.0), mm(92.5));
    b.apply(&Cmd::RotateItems { ids: ids(&["shp_rect"]), pivot, angle_millideg: 45_000 }).unwrap();
    let dr = drawings(&b);
    let r = dr.shapes.iter().find(|s| s.id() == "shp_rect").unwrap();
    let Shape::Polygon { pts, .. } = r else { panic!("{r:?}") };
    assert!(pts.len() > 4, "the corners are rounded: {pts:?}");
    // Every point turned back lies on the rounded rectangle 0..10 x 90..95 mm with 2 mm corners.
    let ring = eda_model::outline::rounded_rect_ring(p(Nm::ZERO, mm(90.0)), p(mm(10.0), mm(95.0)), mm(2.0), eda_model::outline::MAX_ERROR_NM.as_f64());
    let want: Vec<Point> = ring.iter().map(|q| rotate_point_about(*q, pivot, 45_000)).collect();
    assert_eq!(pts.len(), want.len());
    for (a, w) in pts.iter().zip(&want) {
        assert!((a.x - w.x).abs() <= Nm::new(2) && (a.y - w.y).abs() <= Nm::new(2), "{a:?} vs {w:?}");
    }
    assert_eq!(dr.board_items.corner_radius_of("shp_rect"), Nm::ZERO, "a polygon has no corner radius");
}

#[test]
fn a_rounded_rectangle_turned_off_quarter_is_flattened_at_the_boards_max_error() {
    // `rr.TransformToPolygon( GetPolyShape(), getMaxError() )`: the board's maximum error, not a constant (round 3 A-3)
    let mut m = model(&["F.Cu", "B.Cu"]);
    m.board.max_error_nm = Nm::new(1000);
    let mut d = design();
    d.drawings.as_mut().unwrap().board_items.set_corner_radius("shp_rect", mm(2.0));
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::RotateItems { ids: ids(&["shp_rect"]), pivot: p(mm(5.0), mm(92.5)), angle_millideg: 45_000 }).unwrap();
    let Shape::Polygon { pts, .. } = drawings(&b).shapes.iter().find(|s| s.id() == "shp_rect").unwrap() else { panic!() };
    let ring = eda_model::outline::rounded_rect_ring(p(Nm::ZERO, mm(90.0)), p(mm(10.0), mm(95.0)), mm(2.0), 1000.0);
    assert_eq!(pts.len(), ring.len());
    assert!(ring.len() > eda_model::outline::rounded_rect_ring(p(Nm::ZERO, mm(90.0)), p(mm(10.0), mm(95.0)), mm(2.0), 5000.0).len());
}

#[test]
fn a_rotated_arc_track_is_still_an_arc_with_its_mid_point_turned() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    let arc = Track::new_arc("GND".into(), "F.Cu".into(), mm(0.2), p(mm(10.0), mm(10.0)), p(mm(15.0), mm(5.0)), p(mm(20.0), mm(10.0)));
    d.routing.as_mut().unwrap().tracks.push(Track { id: "trk_arc".into(), ..arc });
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::RotateItems { ids: ids(&["trk_arc"]), pivot: p(mm(10.0), mm(10.0)), angle_millideg: 90_000 }).unwrap();
    let t = routing(&b).tracks.iter().find(|t| t.id == "trk_arc").unwrap();
    assert_eq!(t.arc(), Some((p(mm(10.0), mm(10.0)), p(mm(15.0), mm(15.0)), p(mm(10.0), mm(20.0)))));
}

#[test]
fn an_orthogonal_dimension_swaps_its_axis_and_its_side_with_a_quarter_turn() {
    let m = model(&["F.Cu", "B.Cu"]);
    // KiCad: counter-clockwise 90 on a horizontal crossbar makes it vertical, height unchanged; the same turn on a vertical one makes it
    // horizontal with the height negated. Ours is clockwise-positive, so the signs below are KiCad's, negated.
    let mut d = design();
    d.drawings.as_mut().unwrap().dimensions = vec![dimension("o_h", DimensionKind::Orthogonal { height: mm(3.0), horizontal: true }, p(Nm::ZERO, Nm::ZERO), p(mm(10.0), mm(5.0))), dimension("o_v", DimensionKind::Orthogonal { height: mm(3.0), horizontal: false }, p(Nm::ZERO, Nm::ZERO), p(mm(10.0), mm(5.0)))];
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::RotateItems { ids: ids(&["o_h", "o_v"]), pivot: p(Nm::ZERO, Nm::ZERO), angle_millideg: -90_000 }).unwrap();
    let kinds: Vec<DimensionKind> = drawings(&b).dimensions.iter().map(|d| d.kind).collect();
    assert_eq!(kinds, vec![DimensionKind::Orthogonal { height: mm(3.0), horizontal: false }, DimensionKind::Orthogonal { height: mm(-3.0), horizontal: true }]);
    // Half a turn only changes the side.
    b.apply(&Cmd::RotateItems { ids: ids(&["o_h"]), pivot: p(Nm::ZERO, Nm::ZERO), angle_millideg: 180_000 }).unwrap();
    assert_eq!(drawings(&b).dimensions[0].kind, DimensionKind::Orthogonal { height: mm(-3.0), horizontal: false });
}

// --------------------------------------------------------------------- flip

#[test]
fn flip_items_mirrors_copper_across_the_axis_and_puts_it_on_the_other_layer() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::FlipItems { ids: ids(&["trk_a", "via_a", "zone_a"]), pivot: p(mm(50.0), Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    let t = &routing(&b).tracks[0];
    assert_eq!((t.layer.as_str(), t.pts.clone()), ("B.Cu", vec![p(mm(90.0), mm(40.0)), p(mm(80.0), mm(40.0)), p(mm(80.0), mm(50.0))]));
    assert_eq!(routing(&b).vias[0].at, p(mm(80.0), mm(50.0)));
    assert_eq!((routing(&b).vias[0].from_layer.as_str(), routing(&b).vias[0].to_layer.as_str()), ("F.Cu", "B.Cu"), "a through via has no layer pair to change");
    let z = &routing(&b).zones[0];
    assert_eq!((z.layer.as_str(), z.outline[0]), ("B.Cu", p(mm(100.0), mm(60.0))));

    // Top-bottom mirrors y instead.
    let mut b = board(&m);
    b.apply(&Cmd::FlipItems { ids: ids(&["trk_a"]), pivot: p(Nm::ZERO, mm(40.0)), direction: FlipDirection::TopBottom }).unwrap();
    assert_eq!(routing(&b).tracks[0].pts, vec![p(mm(10.0), mm(40.0)), p(mm(20.0), mm(40.0)), p(mm(20.0), mm(30.0))]);
}

/// `ZONE::Flip`: every layer of a zone's set goes to the other side, inner layers mirrored through the stack; a technical zone swaps its front and back layer.
#[test]
fn a_flipped_zone_flips_each_layer_of_its_set() {
    let m = model(&["F.Cu", "In1.Cu", "In2.Cu", "In3.Cu", "In4.Cu", "B.Cu"]);
    let mut d = design();
    let mut inner = zone("zone_in", "F.Cu", &[(Nm::ZERO, Nm::ZERO), (mm(1.0), Nm::ZERO), (mm(1.0), mm(1.0))]);
    inner.set_layer_set(&["F.Cu", "In1.Cu", "In2.Cu"]);
    let mut silk = zone("zone_silk", "F.SilkS", &[(Nm::ZERO, Nm::ZERO), (mm(1.0), Nm::ZERO), (mm(1.0), mm(1.0))]);
    silk.net = String::new();
    silk.set_layer_set(&["F.SilkS", "F.Mask"]);
    d.routing.as_mut().unwrap().zones = vec![inner, silk];
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::FlipItems { ids: ids(&["zone_in", "zone_silk"]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    let zones = &routing(&b).zones;
    assert_eq!(zones[0].layer_set(), vec!["In3.Cu", "In4.Cu", "B.Cu"], "F.Cu to B.Cu, In1.Cu to In4.Cu and In2.Cu to In3.Cu on six layers, in the order of the stack");
    assert_eq!((zones[0].layer.as_str(), zones[1].layer_set()), ("In3.Cu", vec!["B.SilkS", "B.Mask"]));
}

#[test]
fn a_flipped_blind_via_changes_its_layer_pair_and_inner_layers_mirror_through_the_stack() {
    let m = model(&["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]);
    let mut d = design();
    d.routing.as_mut().unwrap().vias.push(via("via_blind", (mm(30.0), mm(30.0)), "F.Cu", "In1.Cu"));
    d.routing.as_mut().unwrap().tracks.push(track("trk_in", "In1.Cu", &[(Nm::ZERO, Nm::ZERO), (mm(1.0), Nm::ZERO)]));
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::FlipItems { ids: ids(&["via_blind", "trk_in"]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
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
    b.apply(&Cmd::FlipItems { ids: ids(&["shp_seg", "shp_rect", "shp_arc"]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    let d = drawings(&b);
    let seg = d.shapes.iter().find(|s| s.id() == "shp_seg").unwrap();
    assert_eq!((seg.layer(), seg.points()), ("B.SilkS", vec![p(Nm::ZERO, mm(80.0)), p(mm(-10.0), mm(80.0))]));
    let rect = d.shapes.iter().find(|s| s.id() == "shp_rect").unwrap();
    assert_eq!((rect.layer(), rect.points()), ("B.Fab", vec![p(mm(-10.0), mm(90.0)), p(Nm::ZERO, mm(95.0))]), "normalised: start at the top left");
    let arc = d.shapes.iter().find(|s| s.id() == "shp_arc").unwrap();
    assert_eq!(arc.points(), vec![p(mm(-50.0), mm(80.0)), p(mm(-45.0), mm(75.0)), p(mm(-40.0), mm(80.0))], "start and end swapped, still the same arc");
}

#[test]
fn a_flipped_text_changes_layer_negates_its_angle_and_reads_mirrored() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().texts.push(text("txt_b", "F.SilkS", (Nm::ZERO, Nm::ZERO), 30_000));
    d.drawings.as_mut().unwrap().texts.push(text("txt_user", "Dwgs.User", (Nm::ZERO, Nm::ZERO), 30_000));
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::FlipItems { ids: ids(&["txt_b", "txt_user"]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    let t = drawings(&b).texts.iter().find(|t| t.id == "txt_b").unwrap();
    assert_eq!((t.layer.as_str(), t.angle, t.mirror), ("B.SilkS", 330_000, true));
    let u = drawings(&b).texts.iter().find(|t| t.id == "txt_user").unwrap();
    assert_eq!((u.layer.as_str(), u.mirror), ("Dwgs.User", false), "a layer that belongs to no side does not mirror");
    b.apply(&Cmd::FlipItems { ids: ids(&["txt_b"]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::TopBottom }).unwrap();
    let t = drawings(&b).texts.iter().find(|t| t.id == "txt_b").unwrap();
    assert_eq!((t.layer.as_str(), t.angle, t.mirror), ("F.SilkS", 210_000, false), "180 - 330 = -150");
}

#[test]
fn a_flipped_dimension_moves_its_crossbar_side_with_the_mirror() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().dimensions = vec![
        dimension("aligned", DimensionKind::Aligned { height: mm(2.0) }, p(mm(10.0), Nm::ZERO), p(mm(20.0), Nm::ZERO)),
        dimension("o_h", DimensionKind::Orthogonal { height: mm(3.0), horizontal: true }, p(mm(10.0), Nm::ZERO), p(mm(20.0), mm(5.0))),
        dimension("o_v", DimensionKind::Orthogonal { height: mm(3.0), horizontal: false }, p(mm(10.0), Nm::ZERO), p(mm(20.0), mm(5.0))),
    ];
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::FlipItems { ids: ids(&["aligned", "o_h", "o_v"]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    let by_id = |name: &str| drawings(&b).dimensions.iter().find(|d| d.id == name).unwrap().clone();
    assert_eq!(by_id("aligned").kind, DimensionKind::Aligned { height: mm(-2.0) });
    assert_eq!((by_id("aligned").start, by_id("aligned").end), (p(mm(-10.0), Nm::ZERO), p(mm(-20.0), Nm::ZERO)));
    assert_eq!(by_id("o_h").kind, DimensionKind::Orthogonal { height: mm(3.0), horizontal: true }, "a left-right flip keeps a horizontal crossbar's side");
    assert_eq!(by_id("o_v").kind, DimensionKind::Orthogonal { height: mm(-3.0), horizontal: false }, "and swaps a vertical one's");
    assert_eq!(by_id("aligned").layer, "Dwgs.User");
}

/// `PCB_DIMENSION_BASE::Mirror` (which `Flip` calls) mirrors the text position first: a leader's or a radial's kept text goes with its lines (r5 F3).
#[test]
fn a_flipped_leader_mirrors_its_text_position() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    let mut leader = dimension("leader", DimensionKind::Leader, p(mm(2.0), mm(1.0)), p(mm(6.0), mm(4.0)));
    leader.text_pos = Some(p(mm(10.0), mm(5.0)));
    let mut radial = dimension("radial", DimensionKind::Radial { leader_length: mm(3.0) }, p(mm(20.0), mm(20.0)), p(mm(25.0), mm(20.0)));
    radial.text_pos = Some(p(mm(30.0), mm(22.0)));
    d.drawings.as_mut().unwrap().dimensions = vec![leader, radial];
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::FlipItems { ids: ids(&["leader", "radial"]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    let by_id = |b: &Board, name: &str| drawings(b).dimensions.iter().find(|d| d.id == name).unwrap().clone();
    let l = by_id(&b, "leader");
    assert_eq!((l.start, l.end), (p(mm(-2.0), mm(1.0)), p(mm(-6.0), mm(4.0))));
    assert_eq!(l.text_pos, Some(p(mm(-10.0), mm(5.0))));
    let r = by_id(&b, "radial");
    assert_eq!((r.start, r.end), (p(mm(-20.0), mm(20.0)), p(mm(-25.0), mm(20.0))));
    assert_eq!(r.text_pos, Some(p(mm(-30.0), mm(22.0))));
    b.apply(&Cmd::FlipItems { ids: ids(&["leader"]), pivot: p(Nm::ZERO, mm(10.0)), direction: FlipDirection::TopBottom }).unwrap();
    assert_eq!(by_id(&b, "leader").text_pos, Some(p(mm(-10.0), mm(15.0))));
}

#[test]
fn a_flipped_footprint_has_every_pad_where_the_mirror_image_puts_it() {
    let m = model(&["F.Cu", "B.Cu"]);
    for dir in [FlipDirection::LeftRight, FlipDirection::TopBottom] {
        for (rot, side) in [(0, Side::Top), (90_000, Side::Top), (180_000, Side::Top), (270_000, Side::Top), (30_000_000, Side::Top), (0, Side::Bottom), (90_000, Side::Bottom), (210_000_000, Side::Bottom)] {
            let mut d = design();
            let fp = d.placement.as_mut().unwrap().footprints.iter_mut().find(|f| f.id == "U1").unwrap();
            fp.rot = rot;
            fp.side = side;
            let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
            let before = pads_of(&b, &m, "U1");
            let pivot = p(mm(35.0), mm(26.0));
            b.apply(&Cmd::FlipItems { ids: ids(&["U1"]), pivot, direction: dir }).unwrap();
            let after = pads_of(&b, &m, "U1");
            let new_pose = pose(&b, "U1");
            assert_eq!(new_pose.side, if side == Side::Top { Side::Bottom } else { Side::Top });
            for (was, now) in before.iter().zip(&after) {
                let want = match dir {
                    FlipDirection::LeftRight => p(2 * pivot.x - was.center.x, was.center.y),
                    FlipDirection::TopBottom => p(was.center.x, 2 * pivot.y - was.center.y),
                };
                assert!((now.center.x - want.x).abs() <= Nm::new(1) && (now.center.y - want.y).abs() <= Nm::new(1), "{dir:?}, part at rot {rot} {side:?}, pad {}: {:?} -> {:?}, the mirror image is {want:?}", was.number, was.center, now.center);
            }
        }
    }
}

#[test]
fn flipping_a_footprint_twice_gives_it_back_and_swaps_its_label_side_both_ways() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let was = pose(&b, "U2");
    b.apply(&Cmd::FlipItems { ids: ids(&["U2"]), pivot: p(mm(50.0), Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    let once = pose(&b, "U2");
    assert_eq!((once.at, once.rot, once.side, once.label), (p(mm(40.0), mm(20.0)), 270_000, Side::Bottom, LabelSide::Right));
    b.apply(&Cmd::FlipItems { ids: ids(&["U2"]), pivot: p(mm(50.0), Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    let twice = pose(&b, "U2");
    assert_eq!((twice.at, twice.rot, twice.side, twice.label), (was.at, was.rot, was.side, was.label));
}

// ------------------------------------------------------------ the routing

/// `EDIT_TOOL::Move` / `Rotate` / `Flip` / `DeleteItems` act on the items they were given: the copper routed to a footprint's pads stays where
/// it is (GAPS.md item 8, "a footprint with copper routed to its pads still clears the routing when edited" -- no longer). The older verbs of the
/// CLI and the agents keep their answer.
#[test]
fn the_edit_tools_never_clear_the_routing_even_of_a_footprint_with_copper_on_its_pads() {
    let m = model(&["F.Cu", "B.Cu"]);
    // U1's pad 2 sits at (31 500, 20 000): a track runs into it, and a via sits on it.
    let mut routed = design();
    routed.routing.as_mut().unwrap().tracks.push(track("trk_u1", "F.Cu", &[(mm(31.5), mm(20.0)), (mm(40.0), mm(20.0))]));
    routed.routing.as_mut().unwrap().vias.push(via("via_u1", (mm(31.5), mm(20.0)), "F.Cu", "B.Cu"));
    let b = Board::new(routed.clone(), &m, mm(0.1), mm(0.3));

    let move_u1 = Cmd::MoveItems { ids: ids(&["trk_a", "U1"]), dx: Nm::new(1), dy: Nm::new(1) };
    assert!(!move_u1.clears_routing_in(&b), "a footprint with a track on its pad moves without the routing");
    assert!(!Cmd::RotateItems { ids: ids(&["U1"]), pivot: p(Nm::ZERO, Nm::ZERO), angle_millideg: 15_000 }.clears_routing_in(&b));
    assert!(!Cmd::FlipItems { ids: ids(&["U1"]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::TopBottom }.clears_routing_in(&b));
    assert!(!Cmd::Rip { part: id("U1") }.clears_routing_in(&b), "deleting a routed footprint leaves its copper");
    assert!(!Cmd::Batch { cmds: vec![move_u1.clone()] }.clears_routing_in(&b));
    assert!(!Cmd::InGroup { group: id("grp_a"), cmd: Box::new(move_u1) }.clears_routing_in(&b));
    // A group moves its footprint members the same way.
    let mut with_group = routed;
    with_group.drawings.as_mut().unwrap().groups.push(Group { id: "grp_a".into(), name: String::new(), member_ids: ids(&["trk_a", "U1"]), design_block: String::new() });
    assert!(!Cmd::MoveItems { ids: ids(&["grp_a"]), dx: Nm::new(1), dy: Nm::new(1) }.clears_routing_in(&Board::new(with_group, &m, mm(0.1), mm(0.3))));
    // The old verbs keep their old answer: a placement made there is followed by a new routing.
    assert!(Cmd::MoveTo { part: id("U2"), x: Nm::ZERO, y: Nm::ZERO }.clears_routing_in(&b));
    assert!(Cmd::Rotate { part: id("U2"), quarter_turns: 1 }.clears_routing_in(&b));
    assert!(Cmd::Flip { part: id("U2") }.clears_routing_in(&b));
}

#[test]
fn moving_turning_flipping_and_deleting_a_routed_footprint_leave_every_track_and_via_as_they_were() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.routing.as_mut().unwrap().tracks.push(track("trk_u1", "F.Cu", &[(mm(31.5), mm(20.0)), (mm(40.0), mm(20.0))]));
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let copper = |b: &Board<'_>| (routing(b).tracks.clone(), routing(b).vias.clone(), routing(b).zones.clone());
    let before = copper(&b);
    b.apply(&Cmd::MoveItems { ids: ids(&["U1"]), dx: mm(5.0), dy: mm(3.0) }).unwrap();
    b.apply(&Cmd::RotateItems { ids: ids(&["U1"]), pivot: p(mm(35.0), mm(23.0)), angle_millideg: -15_000 }).unwrap();
    b.apply(&Cmd::FlipItems { ids: ids(&["U1"]), pivot: p(mm(35.0), mm(23.0)), direction: FlipDirection::TopBottom }).unwrap();
    assert_eq!(copper(&b), before, "the footprint went; the copper did not follow it");
    assert_ne!(pose(&b, "U1").at, p(mm(30.0), mm(20.0)));
}

#[test]
fn the_new_verbs_round_trip_through_json_as_the_studio_sends_them() {
    let cmds = [
        Cmd::MoveItems { ids: ids(&["trk_a"]), dx: mm(-1.5), dy: mm(0.02) },
        Cmd::RotateItems { ids: ids(&["trk_a", "U1"]), pivot: p(um(1.0), um(2.0)), angle_millideg: -90_000 },
        Cmd::FlipItems { ids: ids(&["zone_a"]), pivot: p(um(3.0), um(4.0)), direction: FlipDirection::TopBottom },
    ];
    for c in cmds {
        let json = serde_json::to_string(&c).unwrap();
        let back: Cmd = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c, "{json}");
    }
    let from_studio: Cmd = serde_json::from_str(r#"{"op":"flip_items","ids":["a"],"pivot":{"x":5,"y":6},"direction":"left_right"}"#).unwrap();
    assert_eq!(from_studio, Cmd::FlipItems { ids: ids(&["a"]), pivot: p(Nm::new(5), Nm::new(6)), direction: FlipDirection::LeftRight });
}

/// r2-pcb-edit C4: `BOARD::FlipLayer` reads `m_opposite`, which `recalcOpposites` pairs for user layers typed front and back: by the name after the first dot,
/// else two consecutive renamed ones.
#[test]
fn flip_takes_a_front_user_layer_to_its_back_pair() {
    use eda_model::layers::LayerType;
    let mut m = model(&["F.Cu", "B.Cu"]);
    let ls = &mut m.board.layer_setup;
    ls.types.insert("User.1".into(), LayerType::Front);
    ls.types.insert("User.2".into(), LayerType::Back);
    ls.names.insert("User.1".into(), "F.Notes".into());
    ls.names.insert("User.2".into(), "B.Notes".into());
    ls.types.insert("User.5".into(), LayerType::Front);
    let mut d = design();
    let dr = d.drawings.as_mut().unwrap();
    dr.shapes.iter_mut().find(|s| s.id() == "shp_seg").unwrap().set_layer("User.1".into());
    dr.texts[0].layer = "User.5".into();
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::FlipItems { ids: ids(&["shp_seg", "txt_a"]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    assert_eq!(drawings(&b).shapes.iter().find(|s| s.id() == "shp_seg").unwrap().layer(), "User.2");
    assert_eq!(drawings(&b).texts[0].layer, "User.5", "a front user layer with no pair stays");
    // `PCB_TEXT::Flip`: `if( IsSideSpecific() ) SetMirrored( !IsMirrored() )`, and a user layer typed front or back is side specific (round 3 A-1)
    assert!(drawings(&b).texts[0].mirror, "a text on a front-typed user layer is turned over");
    assert_eq!(m.board.layer_setup.user_layer_opposites().get("User.2").map(String::as_str), Some("User.1"));
    // a text on User.1 lands on User.2, mirrored
    let mut d = design();
    d.drawings.as_mut().unwrap().texts[0].layer = "User.1".into();
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::FlipItems { ids: ids(&["txt_a"]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    assert_eq!((drawings(&b).texts[0].layer.as_str(), drawings(&b).texts[0].mirror), ("User.2", true));
    // a user layer of no side (typed auxiliary, the default) is not
    let mut d = design();
    d.drawings.as_mut().unwrap().texts[0].layer = "User.9".into();
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::FlipItems { ids: ids(&["txt_a"]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    assert!(!drawings(&b).texts[0].mirror);
}
