//! The Footprint Editor's verbs on any item of the open footprint ([`crate::fp_items`]): a field moved, turned, mirrored, edited and hidden by a delete; Mirror as
//! `EDIT_TOOL::Mirror` does it item by item; Rotate about one point; the items `eda_model::fp_items` adds placed, edited and deleted; Group and Ungroup.

use super::fp_items::{FpItem, FpItemCmd};
use super::pcb_transform::FlipDirection;
use super::pcb_transform_tests::{dimension, model, p, zone};
use super::*;
use eda_model::footprint::PadKind;
use eda_model::ir::{Design, DimensionKind, LibraryPadShape, TextJustify};
use eda_model::pcb_items::{PcbPoint, TextBox};
use eda_model::units::mm;

fn design() -> Design {
    serde_json::from_str(r#"{"schema":1,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]}}"#).unwrap()
}

fn pad(number: &str, x: Nm, y: Nm) -> LibraryPad {
    LibraryPad {
        id: String::new(),
        number: number.into(),
        at: Point { x, y },
        size: (mm(1.0), mm(0.6)),
        offset: Point::default(),
        shape: LibraryPadShape::Rect,
        kind: PadKind::Smd,
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

fn text(content: &str, x: Nm, y: Nm, angle: Millideg) -> Text {
    Text { id: String::new(), content: content.into(), at: p(x, y), angle, layer: "F.SilkS".into(), size_nm: mm(1.0), stroke_width: mm(0.15), justify: TextJustify::Left, mirror: false }
}

const NAME: &str = "Lib:Part";

/// A board that holds the footprint `Lib:Part`: two pads, a line, an arc and a text (ids assigned).
fn with_footprint(m: &ConstraintModel) -> Board<'_> {
    let mut b = Board::new(design(), m, mm(0.1), mm(0.3));
    let mut fp = LibraryFootprint::new_empty(NAME);
    let mut p2 = pad("2", mm(2.0), mm(1.0));
    p2.offset = p(mm(0.2), mm(0.1));
    p2.trapezoid_delta = Some((mm(0.1), Nm::ZERO));
    p2.rot = 30_000;
    fp.pads = vec![pad("1", mm(-2.0), Nm::ZERO), p2];
    fp.graphics = vec![
        Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.12), filled: false, start: p(mm(-3.0), mm(-1.0)), end: p(mm(3.0), mm(-1.0)) },
        Shape::Arc { id: String::new(), layer: "F.Fab".into(), stroke_width: mm(0.1), filled: false, start: p(mm(1.0), Nm::ZERO), mid: p(Nm::ZERO, mm(1.0)), end: p(mm(-1.0), Nm::ZERO) },
    ];
    fp.texts = vec![text("${REFERENCE}", mm(1.0), mm(2.0), 0)];
    fp.assign_missing_ids();
    b.apply(&Cmd::PutLibraryFootprint { footprint: fp, overwrite: false }).unwrap();
    b
}

fn fp<'a>(b: &'a Board<'_>) -> &'a LibraryFootprint {
    b.design().footprint_library.as_ref().unwrap().by_name(NAME).unwrap()
}

fn item(c: FpItemCmd) -> Cmd {
    Cmd::FpItem(c)
}

fn refusal(b: &mut Board<'_>, c: FpItemCmd) -> String {
    let e = b.apply(&item(c)).unwrap_err();
    format!("{}: {}", e[0].check, e[0].hint.clone().unwrap_or_default())
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_field_with_no_layout_of_its_own_moves_from_kicads_default_place() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = with_footprint(&m);
    assert!(fp(&b).reference.is_none());
    b.apply(&item(FpItemCmd::Move { footprint: NAME.into(), ids: ids(&["field:Reference"]), dx: mm(1.0), dy: mm(0.5) })).unwrap();
    let def = eda_model::fp_edit::default_library_reference_layout();
    let l = fp(&b).reference.clone().expect("the default layout is made the field's own, then moved");
    assert_eq!(l.at, p(def.at.x + mm(1.0), def.at.y + mm(0.5)));
    assert_eq!((l.layer.as_str(), l.size), (def.layer.as_str(), def.size));
    // a field the footprint does not have is refused, and nothing moved
    let before = fp(&b).clone();
    assert!(refusal(&mut b, FpItemCmd::Move { footprint: NAME.into(), ids: ids(&["field:Nope"]), dx: mm(1.0), dy: Nm::ZERO }).starts_with("ops_unknown_item"));
    assert_eq!(fp(&b), &before);
}

#[test]
fn rotate_turns_every_item_about_one_point_and_its_own_angle_follows() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = with_footprint(&m);
    let f = fp(&b).clone();
    let (p1, line, txt) = (f.pads[0].id.clone(), f.graphics.iter().find(|g| matches!(g, Shape::Segment { .. })).unwrap().id().to_string(), f.texts[0].id.clone());
    // R in the footprint editor is `rotateCcw`: -90 000 here (clockwise-positive), about the origin
    b.apply(&item(FpItemCmd::Rotate { footprint: NAME.into(), ids: vec![p1.clone(), line.clone(), txt.clone(), "field:Value".into()], pivot: p(Nm::ZERO, Nm::ZERO), angle_millideg: -90_000 })).unwrap();
    let f = fp(&b);
    let pad = f.pads.iter().find(|x| x.id == p1).unwrap();
    // `RotatePoint` counter-clockwise on a y-down screen: (-2, 0) -> (0, 2); `m_orient += 90`
    assert_eq!((pad.at, pad.rot), (p(Nm::ZERO, mm(2.0)), 90_000));
    let t = f.texts.iter().find(|x| x.id == txt).unwrap();
    assert_eq!((t.at, t.angle), (p(mm(2.0), mm(-1.0)), 90_000));
    let v = f.value.as_ref().unwrap();
    let def = eda_model::fp_edit::default_library_value_layout();
    assert_eq!((v.at, v.angle), (p(def.at.y, -def.at.x), 90_000));
    match f.graphics.iter().find(|g| g.id() == line).unwrap() {
        Shape::Segment { start, end, .. } => assert_eq!((*start, *end), (p(mm(-1.0), mm(3.0)), p(mm(-1.0), mm(-3.0)))),
        g => panic!("{g:?}"),
    }
}

#[test]
fn mirror_is_kicads_item_by_item_on_the_layer_each_is_on() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = with_footprint(&m);
    let f = fp(&b).clone();
    let all: Vec<String> = f.pads.iter().map(|x| x.id.clone()).chain(f.graphics.iter().map(|g| g.id().to_string())).chain(f.texts.iter().map(|t| t.id.clone())).chain(["field:Reference".to_string()]).collect();
    b.apply(&item(FpItemCmd::Mirror { footprint: NAME.into(), ids: all, pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::LeftRight })).unwrap();
    let f = fp(&b);
    // `mirrorPad`: the position, the offset and the delta mirror, the orientation negates; the layers stay (no flip to the back)
    let p2 = f.pads.iter().find(|x| x.number == "2").unwrap();
    assert_eq!((p2.at, p2.offset, p2.trapezoid_delta, p2.rot), (p(mm(-2.0), mm(1.0)), p(mm(-0.2), mm(0.1)), Some((mm(-0.1), Nm::ZERO)), 330_000));
    // `EDA_SHAPE::flip`: an arc's ends swap so it runs the same way; the layer stays
    match f.graphics.iter().find(|g| matches!(g, Shape::Arc { .. })).unwrap() {
        Shape::Arc { start, mid, end, layer, .. } => {
            assert_eq!((*start, *mid, *end), (p(mm(1.0), Nm::ZERO), p(Nm::ZERO, mm(1.0)), p(mm(-1.0), Nm::ZERO)));
            assert_eq!(layer, "F.Fab");
        }
        g => panic!("{g:?}"),
    }
    // `PCB_TEXT::Mirror`: a horizontal text's justification turns over; the glyphs are not mirrored
    let t = &f.texts[0];
    assert_eq!((t.at, t.justify, t.mirror, t.layer.as_str()), (p(mm(-1.0), mm(2.0)), TextJustify::Right, false, "F.SilkS"));
    let r = f.reference.as_ref().unwrap();
    let def = eda_model::fp_edit::default_library_reference_layout();
    assert_eq!((r.at.x, r.halign, r.mirror), (-def.at.x, -def.halign, def.mirror));
}

#[test]
fn mirror_top_bottom_flips_a_vertical_texts_justification_and_a_text_boxs_angle() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = with_footprint(&m);
    let mut vertical = text("V", mm(1.0), mm(1.0), 90_000);
    vertical.justify = TextJustify::Left;
    b.apply(&Cmd::AddFootprintText { footprint: NAME.into(), text: vertical }).unwrap();
    let mut tb = TextBox::new("F.Fab", p(mm(1.0), mm(1.0)), p(mm(4.0), mm(2.0)), mm(1.0), mm(0.1));
    tb.angle = 30_000;
    b.apply(&item(FpItemCmd::Add { footprint: NAME.into(), item: FpItem::TextBox(tb) })).unwrap();
    let f = fp(&b).clone();
    let v = f.texts.iter().find(|t| t.content == "V").unwrap().id.clone();
    let tbx = f.items.text_boxes[0].id.clone();
    b.apply(&item(FpItemCmd::Mirror { footprint: NAME.into(), ids: vec![v.clone(), tbx.clone()], pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::TopBottom })).unwrap();
    let f = fp(&b);
    let t = f.texts.iter().find(|t| t.id == v).unwrap();
    assert_eq!((t.at, t.justify), (p(mm(1.0), mm(-1.0)), TextJustify::Right));
    let tb = &f.items.text_boxes[0];
    assert_eq!((tb.start, tb.end, tb.angle), (p(mm(1.0), mm(-1.0)), p(mm(4.0), mm(-2.0)), 330_000));
}

#[test]
fn delete_hides_a_field_and_takes_the_other_items_and_a_group_with_its_members() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = with_footprint(&m);
    let f = fp(&b).clone();
    let (p1, p2, txt) = (f.pads[0].id.clone(), f.pads[1].id.clone(), f.texts[0].id.clone());
    b.apply(&item(FpItemCmd::Group { footprint: NAME.into(), ids: vec![p1.clone(), txt.clone()] })).unwrap();
    let g = fp(&b).items.groups[0].id.clone();
    b.apply(&item(FpItemCmd::Delete { footprint: NAME.into(), ids: vec![g, "field:Value".into()] })).unwrap();
    let f = fp(&b);
    assert_eq!(f.pads.iter().map(|x| x.id.clone()).collect::<Vec<_>>(), vec![p2]);
    assert!(f.texts.is_empty() && f.items.groups.is_empty());
    // `case PCB_FIELD_T: field->SetVisible( false )`
    assert!(!f.value_visible);
    assert!(!f.value.as_ref().unwrap().visible);
}

#[test]
fn edit_field_is_the_text_dialog_on_a_field() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = with_footprint(&m);
    let mut l = eda_model::fp_edit::default_library_reference_layout();
    l.at = p(mm(1.0), mm(-3.0));
    l.angle = 90_000;
    l.layer = "F.Fab".into();
    l.visible = false;
    b.apply(&item(FpItemCmd::EditField { footprint: NAME.into(), name: "Reference".into(), text: "U**".into(), layout: l.clone() })).unwrap();
    let f = fp(&b);
    let r = f.reference.as_ref().unwrap();
    assert_eq!((r.at, r.angle, r.layer.as_str(), r.text.as_deref(), f.reference_visible), (l.at, 90_000, "F.Fab", Some("U**"), false));
    // the text KiCad shows by default is no override
    b.apply(&item(FpItemCmd::EditField { footprint: NAME.into(), name: "Value".into(), text: NAME.into(), layout: eda_model::fp_edit::default_library_value_layout() })).unwrap();
    assert_eq!(fp(&b).value.as_ref().unwrap().text, None);
    // an empty box leaves the text as it was
    b.apply(&item(FpItemCmd::EditField { footprint: NAME.into(), name: "Reference".into(), text: String::new(), layout: l.clone() })).unwrap();
    assert_eq!(fp(&b).reference.as_ref().unwrap().text.as_deref(), Some("U**"));
    let mut tiny = l.clone();
    tiny.size = (Nm::new(10), Nm::new(10));
    assert!(refusal(&mut b, FpItemCmd::EditField { footprint: NAME.into(), name: "Reference".into(), text: "U**".into(), layout: tiny }).starts_with("ops_bad_text"));
    assert!(refusal(&mut b, FpItemCmd::EditField { footprint: NAME.into(), name: "Datasheet".into(), text: "x".into(), layout: l }).starts_with("ops_unknown_field"));
}

#[test]
fn the_new_kinds_are_added_edited_moved_and_deleted() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = with_footprint(&m);
    let d = dimension("", DimensionKind::Aligned { height: mm(1.0) }, p(Nm::ZERO, Nm::ZERO), p(mm(5.0), Nm::ZERO));
    b.apply(&item(FpItemCmd::Add { footprint: NAME.into(), item: FpItem::Dimension(d.clone()) })).unwrap();
    let mut keepout = zone("", "F.Cu", &[(Nm::ZERO, Nm::ZERO), (mm(2.0), Nm::ZERO), (mm(2.0), mm(2.0))]);
    keepout.net = "GND".into();
    b.apply(&item(FpItemCmd::Add { footprint: NAME.into(), item: FpItem::Zone(keepout) })).unwrap();
    b.apply(&item(FpItemCmd::Add { footprint: NAME.into(), item: FpItem::Point(PcbPoint::new("F.Fab", p(mm(1.0), mm(1.0)))) })).unwrap();
    let f = fp(&b).clone();
    let (dim, zn, pt) = (f.items.dimensions[0].id.clone(), f.items.zones[0].id.clone(), f.items.points[0].id.clone());
    assert!(dim.starts_with("dim_") && zn.starts_with("zone_") && pt.starts_with("pnt_"));
    assert_eq!(f.items.zones[0].net, "", "a zone of a library footprint has no net");
    // Set keeps the id; another kind under that id is refused
    let mut d2 = d.clone();
    d2.layer = "Cmts.User".into();
    b.apply(&item(FpItemCmd::Set { footprint: NAME.into(), id: dim.clone(), item: FpItem::Dimension(d2) })).unwrap();
    assert_eq!((fp(&b).items.dimensions[0].id.as_str(), fp(&b).items.dimensions[0].layer.as_str()), (dim.as_str(), "Cmts.User"));
    assert!(refusal(&mut b, FpItemCmd::Set { footprint: NAME.into(), id: dim.clone(), item: FpItem::Point(PcbPoint::new("F.Fab", p(Nm::ZERO, Nm::ZERO))) }).starts_with("ops_unknown_item"));
    // the generic Move takes them
    b.apply(&item(FpItemCmd::Move { footprint: NAME.into(), ids: vec![dim.clone(), zn.clone(), pt.clone()], dx: mm(1.0), dy: Nm::ZERO })).unwrap();
    let f = fp(&b);
    assert_eq!((f.items.dimensions[0].start, f.items.zones[0].outline[0], f.items.points[0].at), (p(mm(1.0), Nm::ZERO), p(mm(1.0), Nm::ZERO), p(mm(2.0), mm(1.0))));
    // a zone of two corners is refused
    assert!(refusal(&mut b, FpItemCmd::Add { footprint: NAME.into(), item: FpItem::Zone(zone("", "F.Cu", &[(Nm::ZERO, Nm::ZERO), (mm(1.0), Nm::ZERO)])) }).starts_with("ops_bad_zone"));
    b.apply(&item(FpItemCmd::Delete { footprint: NAME.into(), ids: vec![dim, zn, pt] })).unwrap();
    assert!(fp(&b).items.is_empty());
}

#[test]
fn group_and_ungroup_leave_a_point_out_and_a_group_moves_as_its_members() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = with_footprint(&m);
    b.apply(&item(FpItemCmd::Add { footprint: NAME.into(), item: FpItem::Point(PcbPoint::new("F.Fab", p(mm(1.0), mm(1.0)))) })).unwrap();
    let f = fp(&b).clone();
    let (p1, line, pt) = (f.pads[0].id.clone(), f.graphics[0].id().to_string(), f.items.points[0].id.clone());
    b.apply(&item(FpItemCmd::Group { footprint: NAME.into(), ids: vec![p1.clone(), line.clone(), "field:Reference".into(), pt.clone()] })).unwrap();
    let g = fp(&b).items.groups[0].clone();
    assert_eq!(g.member_ids, vec![p1.clone(), line.clone(), "field:Reference".to_string()], "`IsGroupableType`: a point is not groupable, a field is");
    let pad_before = fp(&b).pads[0].at;
    b.apply(&item(FpItemCmd::Move { footprint: NAME.into(), ids: vec![g.id.clone()], dx: mm(1.0), dy: mm(1.0) })).unwrap();
    assert_eq!(fp(&b).pads[0].at, p(pad_before.x + mm(1.0), pad_before.y + mm(1.0)));
    b.apply(&item(FpItemCmd::SetGroupName { footprint: NAME.into(), id: g.id.clone(), name: "body".into() })).unwrap();
    assert_eq!(fp(&b).items.groups[0].name, "body");
    b.apply(&item(FpItemCmd::Ungroup { footprint: NAME.into(), ids: vec![g.id.clone()] })).unwrap();
    assert!(fp(&b).items.groups.is_empty());
    assert!(refusal(&mut b, FpItemCmd::Group { footprint: NAME.into(), ids: vec![p1] }).starts_with("ops_bad_group"));
    assert!(refusal(&mut b, FpItemCmd::Ungroup { footprint: NAME.into(), ids: vec![g.id] }).starts_with("ops_unknown_group"));
}

#[test]
fn the_verbs_are_footprint_editor_steps_read_from_the_studios_json() {
    let c: Cmd = serde_json::from_value(serde_json::json!({
        "op": "fp_item", "verb": "mirror", "footprint": NAME, "ids": ["field:Reference"], "pivot": {"x": 0, "y": 0}, "direction": "left_right"
    }))
    .unwrap();
    assert_eq!(c.domain(), Domain::FootprintEditor);
    let c: Cmd = serde_json::from_value(serde_json::json!({
        "op": "fp_item", "verb": "add", "footprint": NAME, "item": {"point": {"layer": "F.Fab", "at": {"x": 1000000, "y": 0}}}
    }))
    .unwrap();
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = with_footprint(&m);
    b.apply(&c).unwrap();
    assert_eq!(fp(&b).items.points[0].size_nm, eda_model::pcb_items::DEFAULT_POINT_SIZE_NM);
}

#[test]
fn set_anchor_moves_the_graphical_items_and_the_zones_not_the_points() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = with_footprint(&m);
    b.apply(&item(FpItemCmd::Add { footprint: NAME.into(), item: FpItem::Dimension(dimension("", DimensionKind::Aligned { height: mm(1.0) }, p(Nm::ZERO, Nm::ZERO), p(mm(5.0), Nm::ZERO))) })).unwrap();
    b.apply(&item(FpItemCmd::Add { footprint: NAME.into(), item: FpItem::Zone(zone("", "F.Cu", &[(Nm::ZERO, Nm::ZERO), (mm(2.0), Nm::ZERO), (mm(2.0), mm(2.0))])) })).unwrap();
    b.apply(&item(FpItemCmd::Add { footprint: NAME.into(), item: FpItem::Point(PcbPoint::new("F.Fab", p(mm(1.0), mm(1.0)))) })).unwrap();
    // `FOOTPRINT::MoveAnchorPosition( -cursorPos )`: the click point becomes the origin
    b.apply(&Cmd::SetFootprintAnchor { name: NAME.into(), at: p(mm(1.0), mm(2.0)) }).unwrap();
    let f = fp(&b);
    assert_eq!(f.items.dimensions[0].start, p(mm(-1.0), mm(-2.0)));
    assert_eq!(f.items.zones[0].outline[0], p(mm(-1.0), mm(-2.0)));
    assert_eq!(f.items.points[0].at, p(mm(1.0), mm(1.0)), "`m_points` is not in MoveAnchorPosition's lists");
}
