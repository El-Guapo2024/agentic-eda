//! Swap (Alt+S) of every kind of item ([`crate::pcb_swap`]): `EDIT_TOOL::Swap`'s pairwise loop, each class's `GetPosition` / `SetPosition` and the layer swap of two
//! items of one type, on the board (`Cmd::SwapItems`) and in the Footprint Editor (`FpItemCmd::Swap`, two pads as `PAD::SwapShapePositions`).

use super::fp_items::FpItemCmd;
use super::pcb_transform_tests::{design, dimension, drawings, ids, p, pose, routing, text, track, via, zone};
use super::*;
use eda_model::ir::{tessellate_arc, DimensionKind, Group, LibraryPadShape, TextJustify, TRACK_ARC_SEGMENTS};
use eda_model::pcb_items::{PcbPoint, TextBox};
use eda_model::units::mm;

fn swap(b: &mut Board<'_>, list: &[&str]) -> Result<(), Vec<CheckResult>> {
    b.apply(&Cmd::SwapItems { ids: ids(list), centres: BTreeMap::new() })
}

fn four_layer_model() -> ConstraintModel {
    super::pcb_transform_tests::model(&["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"])
}

fn board_with(m: &ConstraintModel, edit: impl FnOnce(&mut Design)) -> Board<'_> {
    let mut d = design();
    edit(&mut d);
    Board::new(d, m, mm(0.1), mm(0.3))
}

fn text_at<'a>(b: &'a Board<'_>, i: &str) -> &'a Text {
    drawings(b).texts.iter().find(|t| t.id == i).unwrap()
}

#[test]
fn two_vias_trade_places_and_a_blind_and_a_buried_via_their_start_layers() {
    let m = four_layer_model();
    let mut b = board_with(&m, |d| {
        let rt = d.routing.as_mut().unwrap();
        rt.vias.push(via("via_b", (mm(40.0), mm(50.0)), "F.Cu", "In1.Cu"));
        rt.vias.push(via("via_c", (mm(45.0), mm(55.0)), "In1.Cu", "In2.Cu"));
    });
    // a through via and a blind one: places swap, the through via stays one, the blind via takes F.Cu (its own start already)
    swap(&mut b, &["via_a", "via_b"]).unwrap();
    let v = |b: &Board<'_>, i: &str| routing(b).vias.iter().find(|v| v.id == i).unwrap().clone();
    assert_eq!(v(&b, "via_a").at, p(mm(40.0), mm(50.0)));
    assert_eq!(v(&b, "via_b").at, p(mm(20.0), mm(50.0)));
    assert_eq!((v(&b, "via_a").from_layer.as_str(), v(&b, "via_a").to_layer.as_str()), ("F.Cu", "B.Cu"));
    // a blind via (F.Cu-In1) and a buried one (In1-In2): `PCB_VIA::SetLayer` sets each drill's start to the other's -- the blind via's start In1 would be its own end,
    // so it keeps its layers; the buried via starts on F.Cu
    swap(&mut b, &["via_b", "via_c"]).unwrap();
    assert_eq!((v(&b, "via_b").from_layer.as_str(), v(&b, "via_b").to_layer.as_str()), ("F.Cu", "In1.Cu"));
    assert_eq!((v(&b, "via_c").from_layer.as_str(), v(&b, "via_c").to_layer.as_str()), ("F.Cu", "In2.Cu"));
    assert_eq!(v(&b, "via_b").at, p(mm(45.0), mm(55.0)));
}

#[test]
fn an_arc_track_moves_only_its_start_and_gives_its_centre() {
    let m = four_layer_model();
    let arc_pts = tessellate_arc(p(mm(10.0), Nm::ZERO), p(Nm::ZERO, mm(10.0)), p(mm(-10.0), Nm::ZERO), TRACK_ARC_SEGMENTS);
    let mut b = board_with(&m, |d| {
        let mut t = track("trk_arc", "In1.Cu", &[]);
        t.pts = arc_pts.clone();
        t.arc_mid_offset = Some(p(mm(-10.0), mm(10.0)));
        d.routing.as_mut().unwrap().tracks.push(t);
    });
    swap(&mut b, &["trk_arc", "via_a"]).unwrap();
    let t = routing(&b).tracks.iter().find(|t| t.id == "trk_arc").unwrap().clone();
    // `PCB_ARC::SetPosition`: `m_Start = aPos`; the middle and the end stay
    assert_eq!(t.arc(), Some((p(mm(20.0), mm(50.0)), p(Nm::ZERO, mm(10.0)), p(mm(-10.0), Nm::ZERO))));
    // `PCB_ARC::GetPosition`: the centre of the circle through the three points
    assert_eq!(routing(&b).vias.iter().find(|v| v.id == "via_a").unwrap().at, p(Nm::ZERO, Nm::ZERO));
    // an arc and a via are not of one type: the arc stays on In1
    assert_eq!(t.layer, "In1.Cu");
    // a straight track is no item of Swap's
    assert_eq!(swap(&mut b, &["trk_a", "via_a"]).unwrap_err()[0].check, "ops_swap");
}

#[test]
fn a_dimension_moves_its_start_and_two_of_one_kind_swap_layers() {
    let m = four_layer_model();
    let mut b = board_with(&m, |d| {
        let dr = d.drawings.as_mut().unwrap();
        let mut d2 = dimension("dim_b", DimensionKind::Aligned { height: mm(1.0) }, p(mm(10.0), mm(10.0)), p(mm(15.0), mm(10.0)));
        d2.layer = "Cmts.User".into();
        dr.dimensions.push(d2);
        let mut d3 = dimension("dim_c", DimensionKind::Leader, p(mm(30.0), mm(30.0)), p(mm(35.0), mm(35.0)));
        d3.layer = "User.1".into();
        dr.dimensions.push(d3);
    });
    swap(&mut b, &["dim_a", "dim_b"]).unwrap();
    let d = |b: &Board<'_>, i: &str| drawings(b).dimensions.iter().find(|d| d.id == i).unwrap().clone();
    // `SetStart( aPos )`: the start alone moves
    assert_eq!((d(&b, "dim_a").start, d(&b, "dim_a").end), (p(mm(10.0), mm(10.0)), p(mm(80.0), mm(50.0))));
    assert_eq!((d(&b, "dim_b").start, d(&b, "dim_b").end), (p(mm(70.0), mm(50.0)), p(mm(15.0), mm(10.0))));
    assert_eq!((d(&b, "dim_a").layer.as_str(), d(&b, "dim_b").layer.as_str()), ("Cmts.User", "Dwgs.User"));
    // an aligned dimension and a leader are two types: their layers stay
    swap(&mut b, &["dim_a", "dim_c"]).unwrap();
    assert_eq!((d(&b, "dim_a").layer.as_str(), d(&b, "dim_c").layer.as_str()), ("Cmts.User", "User.1"));
    assert_eq!(d(&b, "dim_c").start, p(mm(10.0), mm(10.0)));
}

#[test]
fn three_texts_go_round_as_the_pairs_leave_them() {
    let m = four_layer_model();
    let mut b = board_with(&m, |d| {
        let dr = d.drawings.as_mut().unwrap();
        dr.texts.push(text("txt_b", "B.SilkS", (mm(1.0), mm(2.0)), 0));
        dr.texts.push(text("txt_c", "User.2", (mm(3.0), mm(4.0)), 0));
    });
    // swap(a, b), then swap(b, c): a takes b's place and layer, b c's, c a's
    swap(&mut b, &["txt_a", "txt_b", "txt_c"]).unwrap();
    assert_eq!((text_at(&b, "txt_a").at, text_at(&b, "txt_a").layer.as_str()), (p(mm(1.0), mm(2.0)), "B.SilkS"));
    assert_eq!((text_at(&b, "txt_b").at, text_at(&b, "txt_b").layer.as_str()), (p(mm(3.0), mm(4.0)), "User.2"));
    assert_eq!((text_at(&b, "txt_c").at, text_at(&b, "txt_c").layer.as_str()), (p(mm(70.0), mm(80.0)), "F.SilkS"));
}

#[test]
fn a_zone_stays_put_and_hands_its_first_corner_on() {
    let m = four_layer_model();
    let mut b = board_with(&m, |_| {});
    swap(&mut b, &["zone_a", "txt_a"]).unwrap();
    assert_eq!(routing(&b).zones[0].outline[0], p(Nm::ZERO, mm(60.0)));
    assert_eq!(text_at(&b, "txt_a").at, p(Nm::ZERO, mm(60.0)));
}

#[test]
fn a_group_moves_by_its_centre_and_needs_it() {
    let m = four_layer_model();
    let mut b = board_with(&m, |d| {
        let dr = d.drawings.as_mut().unwrap();
        dr.groups.push(Group { id: "grp_a".into(), name: String::new(), member_ids: vec!["shp_seg".into(), "shp_rect".into()], design_block: String::new() });
    });
    assert_eq!(swap(&mut b, &["grp_a", "txt_a"]).unwrap_err()[0].check, "ops_swap", "a group's centre is the caller's");
    let centres = BTreeMap::from([("grp_a".to_string(), p(mm(5.0), mm(87.5)))]);
    b.apply(&Cmd::SwapItems { ids: ids(&["grp_a", "txt_a"]), centres }).unwrap();
    // the group's members moved by (text - centre), the text to the centre; no layer swap (a group and a text)
    let seg = drawings(&b).shapes.iter().find(|s| s.id() == "shp_seg").unwrap().clone();
    assert_eq!(seg.points()[0], p(mm(65.0), mm(72.5)));
    assert_eq!(text_at(&b, "txt_a").at, p(mm(5.0), mm(87.5)));
    assert_eq!(text_at(&b, "txt_a").layer, "F.SilkS");
}

#[test]
fn text_boxes_and_points_swap_places_and_layers_and_a_shape_off_copper_loses_its_net() {
    let m = four_layer_model();
    let mut b = board_with(&m, |d| {
        let dr = d.drawings.as_mut().unwrap();
        let mut a = TextBox::new("F.SilkS", p(mm(1.0), mm(1.0)), p(mm(5.0), mm(3.0)), mm(1.0), mm(0.1));
        a.id = "tbx_a".into();
        let mut c = TextBox::new("B.Fab", p(mm(20.0), mm(20.0)), p(mm(25.0), mm(22.0)), mm(1.0), mm(0.1));
        c.id = "tbx_b".into();
        dr.board_items.text_boxes = vec![a, c];
        let mut q = PcbPoint::new("User.3", p(mm(50.0), mm(50.0)));
        q.id = "pnt_a".into();
        dr.board_items.points = vec![q];
        dr.shapes.push(Shape::Segment { id: "shp_cu".into(), layer: "F.Cu".into(), stroke_width: mm(0.2), filled: false, start: p(mm(60.0), mm(60.0)), end: p(mm(61.0), mm(60.0)) });
        dr.board_items.set_net("shp_cu", Some("GND".into()));
    });
    swap(&mut b, &["tbx_a", "tbx_b", "pnt_a"]).unwrap();
    let tb = |b: &Board<'_>, i: &str| drawings(b).board_items.text_boxes.iter().find(|t| t.id == i).unwrap().clone();
    assert_eq!((tb(&b, "tbx_a").start, tb(&b, "tbx_a").end, tb(&b, "tbx_a").layer.as_str()), (p(mm(20.0), mm(20.0)), p(mm(24.0), mm(22.0)), "B.Fab"));
    // then the second text box and the point: places only (two types)
    assert_eq!((tb(&b, "tbx_b").start, tb(&b, "tbx_b").layer.as_str()), (p(mm(50.0), mm(50.0)), "F.SilkS"));
    assert_eq!(drawings(&b).board_items.points[0].at, p(mm(1.0), mm(1.0)));
    // `PCB_SHAPE::SetLayer`: off copper, no net
    assert_eq!(drawings(&b).board_items.net_of("shp_cu"), Some("GND"));
    swap(&mut b, &["shp_cu", "shp_seg"]).unwrap();
    assert_eq!(drawings(&b).board_items.net_of("shp_cu"), None);
    assert_eq!(drawings(&b).shapes.iter().find(|s| s.id() == "shp_cu").unwrap().layer(), "F.SilkS");
    assert_eq!(drawings(&b).shapes.iter().find(|s| s.id() == "shp_seg").unwrap().layer(), "F.Cu");
}

#[test]
fn two_footprints_on_two_sides_trade_place_side_and_orientation() {
    let m = four_layer_model();
    let mut b = board_with(&m, |d| {
        let fps = &mut d.placement.as_mut().unwrap().footprints;
        fps[1].side = Side::Bottom;
    });
    swap(&mut b, &["U1", "U2"]).unwrap();
    let (u1, u2) = (pose(&b, "U1"), pose(&b, "U2"));
    assert_eq!((u1.at, u1.rot, u1.side), (p(mm(60.0), mm(20.0)), 90_000, Side::Bottom));
    assert_eq!((u2.at, u2.rot, u2.side), (p(mm(30.0), mm(20.0)), 0, Side::Top));
    // a footprint and a via: places only
    swap(&mut b, &["U1", "via_a"]).unwrap();
    assert_eq!((pose(&b, "U1").at, pose(&b, "U1").side), (p(mm(20.0), mm(50.0)), Side::Bottom));
}

#[test]
fn swap_refuses_fewer_than_two_items_and_one_named_twice() {
    let m = four_layer_model();
    let mut b = board_with(&m, |_| {});
    assert_eq!(swap(&mut b, &["txt_a"]).unwrap_err()[0].check, "ops_swap");
    assert_eq!(swap(&mut b, &["txt_a", "txt_a"]).unwrap_err()[0].check, "ops_swap_self");
    assert_eq!(swap(&mut b, &["txt_a", "nope"]).unwrap_err()[0].check, "ops_unknown_item");
}

#[test]
fn swap_items_reads_from_the_wire_with_its_group_centres() {
    let c: Cmd = serde_json::from_str(r#"{"op":"swap_items","ids":["a","b"],"centres":{"a":{"x":1,"y":2}}}"#).unwrap();
    assert_eq!(c, Cmd::SwapItems { ids: ids(&["a", "b"]), centres: BTreeMap::from([("a".to_string(), p(Nm::new(1), Nm::new(2)))]) });
    let c: Cmd = serde_json::from_str(r#"{"op":"fp_item","verb":"swap","footprint":"L:F","ids":["a","b"]}"#).unwrap();
    assert!(matches!(c, Cmd::FpItem(FpItemCmd::Swap { .. })));
}

// ------------------------------------------------------------------------------------------------- the Footprint Editor

const NAME: &str = "Lib:Swap";

fn lib_pad(number: &str, x: Nm, y: Nm) -> LibraryPad {
    LibraryPad {
        id: format!("pad{number}"),
        number: number.into(),
        at: Point { x, y },
        size: (mm(1.0), mm(0.6)),
        offset: Point::default(),
        shape: LibraryPadShape::Rect,
        kind: eda_model::footprint::PadKind::Smd,
        drill: None,
        drill_slot: None,
        rot: 0,
        roundrect_ratio: None,
        trapezoid_delta: None,
        chamfer_ratio: None,
        chamfer_corners: eda_model::ir::ChamferCorners::default(),
        custom: None,
        layers: vec![],
        clearance_override: None,
        thermal_gap_override: None,
        thermal_spoke_width_override: None,
        zone_connection: None,
        thermal_spoke_angle_mdeg: None,
        stack: Default::default(),
        kicad: Default::default(),
    }
}

fn fp_board(m: &ConstraintModel) -> Board<'_> {
    let d: Design = serde_json::from_str(r#"{"schema":1,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]}}"#).unwrap();
    let mut b = Board::new(d, m, mm(0.1), mm(0.3));
    let mut fp = LibraryFootprint::new_empty(NAME);
    let mut p2 = lib_pad("2", mm(2.0), mm(1.0));
    p2.offset = p(mm(0.5), Nm::ZERO);
    p2.rot = 90_000;
    fp.pads = vec![lib_pad("1", mm(-2.0), Nm::ZERO), p2];
    let t = |id: &str, at: Point, layer: &str| Text { id: id.into(), content: id.into(), at, angle: 0, layer: layer.into(), size_nm: mm(1.0), stroke_width: mm(0.15), justify: TextJustify::Left, mirror: false };
    fp.texts = vec![t("txt_f", p(mm(1.0), mm(3.0)), "F.SilkS"), t("txt_g", p(mm(-1.0), mm(-3.0)), "F.Fab")];
    let mut z = zone("zone_f", "F.Cu", &[(mm(5.0), mm(5.0)), (mm(6.0), mm(5.0)), (mm(6.0), mm(6.0))]);
    z.net = String::new();
    fp.items.zones = vec![z];
    b.apply(&Cmd::PutLibraryFootprint { footprint: fp, overwrite: false }).unwrap();
    b
}

fn lib<'a>(b: &'a Board<'_>) -> &'a LibraryFootprint {
    b.design().footprint_library.as_ref().unwrap().by_name(NAME).unwrap()
}

/// The id the library gave an item: a pad by its number (`pad1`), a text by its content (`txt_f`), the zone, else as it is.
fn lib_id(b: &Board<'_>, name: &str) -> String {
    let fp = lib(b);
    if let Some(n) = name.strip_prefix("pad") {
        return fp.pads.iter().find(|p| p.number == n).unwrap().id.clone();
    }
    fp.texts.iter().find(|t| t.content == name).map(|t| t.id.clone()).or_else(|| (name == "zone_f").then(|| fp.items.zones[0].id.clone())).unwrap_or_else(|| name.to_string())
}

fn fp_swap(b: &Board<'_>, list: &[&str]) -> Cmd {
    Cmd::FpItem(FpItemCmd::Swap { footprint: NAME.into(), ids: list.iter().map(|n| lib_id(b, n)).collect(), centres: BTreeMap::new() })
}

#[test]
fn two_pads_in_the_footprint_editor_trade_the_places_of_their_copper() {
    let m = four_layer_model();
    let mut b = fp_board(&m);
    b.apply(&fp_swap(&b, &["pad1", "pad2"])).unwrap();
    let pad = |b: &Board<'_>, i: &str| lib(b).pads.iter().find(|p| p.number == i[3..]).unwrap().clone();
    // pad 2's copper is at its hole plus its offset turned with it
    let off2 = crate::pad_edit::rotate(p(mm(0.5), Nm::ZERO), 90_000);
    assert_ne!(off2, p(mm(0.5), Nm::ZERO));
    let shape2 = p(mm(2.0) + off2.x, mm(1.0) + off2.y);
    // pad 1 (no offset) takes pad 2's copper place; pad 2's copper lands where pad 1's was
    assert_eq!(pad(&b, "pad1").at, shape2);
    assert_eq!(pad(&b, "pad2").at, p(mm(-2.0) - off2.x, -off2.y));
    // the numbers, sizes and turns stay
    assert_eq!((pad(&b, "pad1").number.as_str(), pad(&b, "pad2").rot), ("1", 90_000));
}

#[test]
fn footprint_editor_items_swap_places_and_layers_as_on_the_board() {
    let m = four_layer_model();
    let mut b = fp_board(&m);
    b.apply(&fp_swap(&b, &["txt_f", "txt_g"])).unwrap();
    let t = |b: &Board<'_>, i: &str| lib(b).texts.iter().find(|t| t.content == i).unwrap().clone();
    assert_eq!((t(&b, "txt_f").at, t(&b, "txt_f").layer.as_str()), (p(mm(-1.0), mm(-3.0)), "F.Fab"));
    assert_eq!((t(&b, "txt_g").at, t(&b, "txt_g").layer.as_str()), (p(mm(1.0), mm(3.0)), "F.SilkS"));
    // a pad and a text: places; a zone stays
    b.apply(&fp_swap(&b, &["pad1", "txt_f", "zone_f"])).unwrap();
    assert_eq!(lib(&b).pads.iter().find(|p| p.number == "1").unwrap().at, p(mm(-1.0), mm(-3.0)));
    assert_eq!(t(&b, "txt_f").at, p(mm(5.0), mm(5.0)));
    assert_eq!(lib(&b).items.zones[0].outline[0], p(mm(5.0), mm(5.0)));
    // a field and a text: places, not layers (`PCB_FIELD_T` and `PCB_TEXT_T`)
    b.apply(&fp_swap(&b, &["field:Value", "txt_g"])).unwrap();
    assert_eq!(t(&b, "txt_g").layer, "F.SilkS");
}

#[test]
fn two_zones_swap_layers_and_a_zone_on_several_layers_is_undefined_layer() {
    let m = four_layer_model();
    let mut b = board_with(&m, |d| {
        let rt = d.routing.as_mut().unwrap();
        rt.zones.push(zone("zone_b", "B.Cu", &[(mm(20.0), mm(60.0)), (mm(30.0), mm(60.0)), (mm(30.0), mm(70.0))]));
        let mut multi = zone("zone_m", "F.Cu", &[(mm(40.0), mm(60.0)), (mm(50.0), mm(60.0)), (mm(50.0), mm(70.0))]);
        multi.set_layer_set(&["F.Cu", "In1.Cu"]);
        rt.zones.push(multi);
    });
    let z = |b: &Board<'_>, i: &str| routing(b).zones.iter().find(|z| z.id == i).unwrap().clone();
    // two single-layer zones trade layers; neither moves (`ZONE::SetPosition` is a no-op)
    swap(&mut b, &["zone_a", "zone_b"]).unwrap();
    assert_eq!((z(&b, "zone_a").layer_set(), z(&b, "zone_b").layer_set()), (vec!["B.Cu"], vec!["F.Cu"]));
    assert_eq!(z(&b, "zone_b").outline[0], p(mm(20.0), mm(60.0)));
    // a zone on several layers is `UNDEFINED_LAYER`: it takes the other's one layer, and the other keeps its own (`SetLayer( UNDEFINED_LAYER )` changes nothing)
    swap(&mut b, &["zone_m", "zone_b"]).unwrap();
    assert_eq!((z(&b, "zone_m").layer_set(), z(&b, "zone_b").layer_set()), (vec!["F.Cu"], vec!["F.Cu"]));
}
