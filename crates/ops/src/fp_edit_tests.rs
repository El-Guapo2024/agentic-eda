//! `EditBoardFootprint`, `EditBoardPad` and the fields of a footprint moving, turning and flipping on their own.

use super::pcb_transform_tests::{board, design, drawings, ids, model, p, pose};
use super::*;
use eda_model::fp_edit::{FieldLayout, FootprintAttrs, FootprintEdit, FootprintKind, PadEdit, UserField};
use eda_model::{PadKind, PadShape};

fn layout(x: Nm, y: Nm, layer: &str) -> FieldLayout {
    FieldLayout::new(p(x, y), layer)
}

fn edit_of<'a>(b: &'a Board<'_>, id: &str) -> Option<&'a FootprintEdit> {
    b.design().footprint_edit(id)
}

fn set_fields(part: &str, reference: Option<FieldLayout>, value: Option<FieldLayout>, fields: Option<Vec<UserField>>, attrs: Option<FootprintAttrs>) -> Cmd {
    Cmd::EditBoardFootprint { part: part.into(), reference, value, fields, attrs }
}

fn refusal(b: &mut Board<'_>, cmd: Cmd) -> String {
    let e = b.apply(&cmd).unwrap_err();
    format!("{}: {}", e[0].check, e[0].hint.clone().unwrap_or_default())
}

#[test]
fn a_footprints_fields_and_attributes_are_stored_replaced_and_dropped_when_they_say_nothing() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    assert!(edit_of(&b, "U1").is_none(), "a board nothing was edited on has no edits");

    let mut reference = layout(1_000, -3_000, "F.Fab");
    reference.size = (1_200, 800);
    reference.thickness = 150;
    reference.halign = -1;
    let vendor = UserField { name: "Vendor".into(), text: "ACME".into(), layout: layout(0, 2_000, "F.Fab") };
    let attrs = FootprintAttrs { kind: FootprintKind::Smd, exclude_from_pos_files: true, dnp: true, ..Default::default() };
    b.apply(&set_fields("U1", Some(reference.clone()), None, Some(vec![vendor.clone()]), Some(attrs))).unwrap();
    let e = edit_of(&b, "U1").unwrap();
    assert_eq!(e.reference, Some(reference.clone()));
    assert_eq!((e.value.is_none(), e.fields.clone(), e.attrs), (true, vec![vendor.clone()], Some(attrs)));
    assert!(edit_of(&b, "U2").is_none(), "another footprint is untouched");

    // A second edit changes what it names and leaves the rest.
    let mut moved = reference.clone();
    moved.at = p(-500, -3_000);
    b.apply(&set_fields("U1", Some(moved.clone()), None, None, None)).unwrap();
    let e = edit_of(&b, "U1").unwrap();
    assert_eq!((e.reference.clone(), e.fields.len(), e.attrs), (Some(moved), 1, Some(attrs)));

    // The user fields are replaced as a list: an empty one removes them.
    b.apply(&set_fields("U1", None, None, Some(vec![]), None)).unwrap();
    assert!(edit_of(&b, "U1").unwrap().fields.is_empty());
}

#[test]
fn a_field_the_dialog_would_refuse_is_refused_with_its_message() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let mut tiny = layout(0, 0, "F.SilkS");
    tiny.size = (0, 1_000);
    assert!(refusal(&mut b, set_fields("U1", Some(tiny), None, None, None)).contains("Text width must be at least"));
    let mut huge = layout(0, 0, "F.SilkS");
    huge.size = (1_000, 300_000);
    assert!(refusal(&mut b, set_fields("U1", Some(huge), None, None, None)).contains("Text height must be at most"));
    let mut thick = layout(0, 0, "F.SilkS");
    thick.thickness = 400;
    assert!(refusal(&mut b, set_fields("U1", Some(thick), None, None, None)).contains("thickness is too large"), "a quarter of the 1 mm size is 0.25 mm");
    let mut nowhere = layout(0, 0, "No.Such");
    nowhere.thickness = 0;
    assert!(refusal(&mut b, set_fields("U1", None, Some(nowhere), None, None)).contains("not a layer"));
    let user = |name: &str| UserField { name: name.into(), text: String::new(), layout: layout(0, 0, "F.Fab") };
    assert!(refusal(&mut b, set_fields("U1", None, None, Some(vec![user("")]), None)).contains("must have a name"));
    assert!(refusal(&mut b, set_fields("U1", None, None, Some(vec![user("Value")]), None)).contains("mandatory"));
    assert!(refusal(&mut b, set_fields("U1", None, None, Some(vec![user("A"), user("A")]), None)).contains("two fields"));
    assert!(refusal(&mut b, set_fields("U1", None, None, None, None)).contains("nothing to change"));
    assert!(refusal(&mut b, set_fields("U9", Some(layout(0, 0, "F.SilkS")), None, None, None)).starts_with("ops_not_placed"));
    assert!(edit_of(&b, "U1").is_none(), "a refused edit leaves the board as it was");
}

#[test]
fn dnp_and_exclude_from_bom_reach_the_symbol_and_a_symbol_edit_reaches_the_footprint() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::AddSymbol { id: "U1".into(), lib_id: "Device:R".into(), at: p(0, 0), rot_millideg: 0, value: String::new(), footprint: String::new(), unit: 1 }).unwrap();
    let flags = |b: &Board<'_>| {
        let s = b.design().schematic.as_ref().unwrap().symbols.iter().find(|s| s.id == "U1").unwrap().clone();
        (s.dnp, s.exclude_from_bom)
    };
    assert_eq!(flags(&b), (false, false));

    let attrs = FootprintAttrs { dnp: true, exclude_from_bom: true, ..Default::default() };
    b.apply(&set_fields("U1", None, None, None, Some(attrs))).unwrap();
    assert_eq!(flags(&b), (true, true), "the BOM kicad-cli writes from the schematic honours the footprint's flags");

    // The schematic editor clears DNP: the footprint's own copy follows, the other flag stays.
    b.apply(&Cmd::SetSymbolAttrs { ids: vec!["U1".into()], dnp: Some(false), exclude_from_bom: None, exclude_from_board: None, exclude_from_sim: None }).unwrap();
    let a = edit_of(&b, "U1").unwrap().attrs.unwrap();
    assert_eq!((a.dnp, a.exclude_from_bom), (false, true));
}

#[test]
fn a_pad_edit_is_stored_by_number_and_cleared_by_an_empty_one() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let mut e = PadEdit::none("2", 1);
    e.shape = Some(PadShape::Oval);
    e.size = Some((900, 500));
    e.offset = Some(p(100, -50));
    e.solder_mask_margin = Some(30);
    b.apply(&Cmd::EditBoardPad { part: "U1".into(), edit: e.clone() }).unwrap();
    assert_eq!(edit_of(&b, "U1").unwrap().pad("2", 1), Some(&e));

    // An edit of the same pad replaces it; one that changes nothing takes it away, and the footprint's edit with it.
    let mut again = PadEdit::none("2", 1);
    again.size = Some((800, 800));
    b.apply(&Cmd::EditBoardPad { part: "U1".into(), edit: again.clone() }).unwrap();
    assert_eq!(edit_of(&b, "U1").unwrap().pad("2", 1), Some(&again));
    b.apply(&Cmd::EditBoardPad { part: "U1".into(), edit: PadEdit::none("2", 1) }).unwrap();
    assert!(edit_of(&b, "U1").is_none());
}

#[test]
fn a_pad_edit_that_cannot_be_a_pad_is_refused() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let pad = |f: &dyn Fn(&mut PadEdit)| {
        let mut e = PadEdit::none("1", 1);
        f(&mut e);
        Cmd::EditBoardPad { part: "U1".into(), edit: e }
    };
    assert!(refusal(&mut b, pad(&|e| e.size = Some((0, 500)))).contains("greater than zero"));
    assert!(refusal(&mut b, pad(&|e| e.roundrect_ratio = Some(0.8))).contains("corner radius"));
    assert!(refusal(&mut b, pad(&|e| e.solder_paste_margin_ratio = Some(-0.9))).contains("paste"));
    assert!(refusal(&mut b, pad(&|e| {
        e.drill = Some(300);
        e.drill_slot = Some((300, 600));
    }))
    .contains("not both"));
    assert!(refusal(&mut b, pad(&|e| e.kind = Some(PadKind::ThroughHole))).contains("no drill"), "a through-hole pad needs a hole");
    assert!(refusal(&mut b, pad(&|e| {
        e.kind = Some(PadKind::ThroughHole);
        e.drill = Some(900);
    }))
    .contains("annular ring"), "a hole as big as the 600 um pad leaves no copper");
    let mut other = PadEdit::none("7", 1);
    other.size = Some((500, 500));
    assert_eq!(b.apply(&Cmd::EditBoardPad { part: "U1".into(), edit: other }).unwrap_err()[0].check, "ops_unknown_pad");
    assert!(edit_of(&b, "U1").is_none());

    // And the same pad made right is accepted: a plated hole smaller than the pad, once the pad is big enough.
    b.apply(&pad(&|e| {
        e.kind = Some(PadKind::ThroughHole);
        e.drill = Some(500);
        e.size = Some((1_200, 1_200));
        e.shape = Some(PadShape::Circle);
    }))
    .unwrap();
}

/// The pad a board has after the edits of footprint `part` are laid on its library footprint (what `ConstraintModel::footprint_of` answers once the
/// board's loader has folded them in).
fn pad_after_edits(b: &Board<'_>, m: &ConstraintModel, part: &str, number: &str) -> eda_model::Pad {
    let lib = m.library_footprint_of(m.part(part).unwrap()).unwrap();
    let edits = b.design().footprint_edit(part).map(|e| e.pads.clone()).unwrap_or_default();
    eda_model::fp_edit::patched_footprint(&lib, &edits).pads.into_iter().find(|p| p.number == number).unwrap()
}

#[test]
fn a_pad_becomes_a_trapezoid_a_chamfered_rectangle_or_a_custom_pad_and_an_empty_edit_brings_the_rectangle_back() {
    use eda_model::footprint::{CustomZoneMode, PadAnchor, PadCustom, PadPrimitive};
    use eda_model::ir::{ChamferCorners, Shape};
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let before = pad_after_edits(&b, &m, "U1", "1");
    assert_eq!(before.shape, PadShape::Rect);

    // A trapezoid: the shape and its delta.
    let mut e = PadEdit::none("1", 1);
    e.shape = Some(PadShape::Trapezoid);
    e.size = Some((1_000, 600));
    e.trapezoid_delta = Some((0, 300));
    b.apply(&Cmd::EditBoardPad { part: "U1".into(), edit: e }).unwrap();
    let p = pad_after_edits(&b, &m, "U1", "1");
    assert_eq!((p.shape, p.size, p.ext.trapezoid_delta), (PadShape::Trapezoid, (1_000, 600), Some((0, 300))));

    // A chamfered rectangle (the default ratios of a shape that has none: 0.25 for the rounded corners, 0.2 for the chamfer).
    let mut e = PadEdit::none("1", 1);
    e.shape = Some(PadShape::ChamferedRect);
    e.chamfer_corners = Some(ChamferCorners { top_left: true, ..Default::default() });
    b.apply(&Cmd::EditBoardPad { part: "U1".into(), edit: e }).unwrap();
    let p = pad_after_edits(&b, &m, "U1", "1");
    assert_eq!((p.shape, p.roundrect_ratio, p.ext.chamfer_ratio, p.ext.trapezoid_delta), (PadShape::ChamferedRect, Some(0.25), Some(0.2), None), "the delta went with the trapezoid");
    assert!(p.ext.chamfer_corners.top_left && !p.ext.chamfer_corners.top_right);

    // A custom pad: an anchor and a polygon, a ring and a line.
    let polygon = PadPrimitive { shape: Shape::Polygon { id: String::new(), layer: String::new(), stroke_width: 0, filled: true, pts: vec![p0(0, 0), p0(1_500, 0), p0(0, 1_500)] }, proxy: false };
    let custom = PadCustom { anchor: PadAnchor::Rect, zone: CustomZoneMode::ConvexHull, primitives: vec![polygon] };
    let mut e = PadEdit::none("1", 1);
    e.shape = Some(PadShape::Custom);
    e.custom = Some(custom.clone());
    b.apply(&Cmd::EditBoardPad { part: "U1".into(), edit: e }).unwrap();
    let p = pad_after_edits(&b, &m, "U1", "1");
    assert_eq!((p.shape, p.ext.custom.clone(), p.ext.chamfer_ratio, p.roundrect_ratio), (PadShape::Custom, Some(custom), None, None), "the chamfer went with the chamfered rectangle");
    // The copper the DRC measures is the anchor and the polygon.
    let fp = pose(&b, "U1");
    let copper = eda_drc::board::placed_pad_copper(&fp, &p);
    assert!(matches!(copper, eda_drc::kimath::Shape::Compound { .. }), "{copper:?}");

    // An edit that says nothing takes the whole edit away, down to the rectangle the library has (the studio's undo restores the design
    // as it was before each step; this is the verb's side of it).
    b.apply(&Cmd::EditBoardPad { part: "U1".into(), edit: PadEdit::none("1", 1) }).unwrap();
    assert_eq!(pad_after_edits(&b, &m, "U1", "1"), before);
    assert!(edit_of(&b, "U1").is_none());
}

fn p0(x: Nm, y: Nm) -> Point {
    p(x, y)
}

#[test]
fn a_pad_whose_new_shape_is_not_a_pad_is_refused_in_kicads_words() {
    use eda_model::footprint::{PadAnchor, PadCustom, PadPrimitive};
    use eda_model::ir::{ChamferCorners, Shape};
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let pad = |f: &dyn Fn(&mut PadEdit)| {
        let mut e = PadEdit::none("1", 1);
        f(&mut e);
        Cmd::EditBoardPad { part: "U1".into(), edit: e }
    };
    // `PAD::doCheckPad`: "(trapezoid delta is too large)" -- the 600 um pad cannot lose more than its own width.
    assert!(refusal(&mut b, pad(&|e| {
        e.shape = Some(PadShape::Trapezoid);
        e.trapezoid_delta = Some((0, 700));
    }))
    .contains("trapezoid delta is too large"));
    assert!(refusal(&mut b, pad(&|e| {
        e.shape = Some(PadShape::ChamferedRect);
        e.chamfer_ratio = Some(1.5);
    }))
    .contains("chamfer"));
    // Two neighbouring corners cannot each lose more than half the side.
    assert!(refusal(&mut b, pad(&|e| {
        e.shape = Some(PadShape::ChamferedRect);
        e.chamfer_ratio = Some(0.7);
        e.chamfer_corners = Some(ChamferCorners { top_left: true, top_right: true, ..Default::default() });
    }))
    .contains("corner chamfer is too large"));
    let line = |a: Point, z: Point| PadPrimitive { shape: Shape::Segment { id: String::new(), layer: String::new(), stroke_width: 100, filled: false, start: a, end: z }, proxy: false };
    // A zero-length line is no shape.
    assert!(refusal(&mut b, pad(&|e| {
        e.shape = Some(PadShape::Custom);
        e.custom = Some(PadCustom { anchor: PadAnchor::Circle, zone: Default::default(), primitives: vec![line(p(0, 0), p(0, 0))] });
    }))
    .contains("Basic shape 1"));
    // Shapes far from the anchor make two pieces of copper: "(custom pad shape must resolve to a single polygon)".
    assert!(refusal(&mut b, pad(&|e| {
        e.shape = Some(PadShape::Custom);
        e.custom = Some(PadCustom { anchor: PadAnchor::Circle, zone: Default::default(), primitives: vec![line(p(5_000, 5_000), p(6_000, 5_000))] });
    }))
    .contains("single polygon"));
    assert!(edit_of(&b, "U1").is_none(), "nothing was stored");
    // The same line touching the anchor is fine.
    b.apply(&pad(&|e| {
        e.shape = Some(PadShape::Custom);
        e.custom = Some(PadCustom { anchor: PadAnchor::Circle, zone: Default::default(), primitives: vec![line(p(0, 0), p(2_000, 0))] });
    }))
    .unwrap();
}

#[test]
fn a_field_moves_on_its_own_by_the_board_delta_whatever_the_footprints_frame() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    // U2 is turned 90 degrees clockwise and its default Reference sits to the left of the courtyard.
    let before = b.field_layout("U2", "Reference").unwrap();
    let fp = pose(&b, "U2");
    let start = before.board_position(&fp);
    b.apply(&Cmd::MoveItems { ids: ids(&["U2:Reference"]), dx: 1_500, dy: -700 }).unwrap();
    let after = edit_of(&b, "U2").unwrap().reference.clone().unwrap();
    assert_eq!(after.board_position(&fp), p(start.x + 1_500, start.y - 700), "on the board it moved by the delta");
    assert_ne!(after.at, before.at, "in the footprint's own frame the delta is turned back");
    assert_eq!((pose(&b, "U2").at, pose(&b, "U2").rot), (fp.at, fp.rot), "the footprint did not move");
    assert_eq!(after.layer, before.layer);
    assert_eq!(after.board_angle(&fp), before.board_angle(&fp), "a move leaves the angle");
}

#[test]
fn a_field_turns_about_the_point_asked_and_its_text_with_it() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let fp = pose(&b, "U1");
    let layout = b.field_layout("U1", "Value").unwrap();
    let at = layout.board_position(&fp);
    let angle = layout.board_angle(&fp);
    // 90 degrees clockwise about a point 1 mm to its left: the anchor swings down (clockwise on the screen), and the text turns with it.
    b.apply(&Cmd::RotateItems { ids: ids(&["U1:Value"]), pivot: p(at.x - 1_000, at.y), angle_millideg: 90_000 }).unwrap();
    let turned = edit_of(&b, "U1").unwrap().value.clone().unwrap();
    assert_eq!(turned.board_position(&fp), p(at.x - 1_000, at.y + 1_000));
    assert_eq!(turned.board_angle(&fp), (angle - 90_000).rem_euclid(360_000));
}

#[test]
fn a_field_flips_to_the_other_side_with_its_layer_and_mirror() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let fp = pose(&b, "U1");
    let l = b.field_layout("U1", "Reference").unwrap();
    assert_eq!((l.layer.as_str(), l.mirror), ("F.SilkS", false));
    let at = l.board_position(&fp);
    b.apply(&Cmd::FlipItems { ids: ids(&["U1:Reference"]), pivot: p(at.x + 2_000, at.y), direction: FlipDirection::LeftRight }).unwrap();
    let flipped = edit_of(&b, "U1").unwrap().reference.clone().unwrap();
    assert_eq!((flipped.layer.as_str(), flipped.mirror), ("B.SilkS", true));
    assert_eq!(flipped.board_position(&fp), p(at.x + 4_000, at.y), "mirrored across the pivot");
    // Flipping it back is where it started.
    b.apply(&Cmd::FlipItems { ids: ids(&["U1:Reference"]), pivot: p(at.x + 2_000, at.y), direction: FlipDirection::LeftRight }).unwrap();
    let back = edit_of(&b, "U1").unwrap().reference.clone().unwrap();
    assert_eq!((back.layer.as_str(), back.mirror, back.board_position(&fp), back.board_angle(&fp)), ("F.SilkS", false, at, l.board_angle(&fp)));
}

#[test]
fn the_fields_of_a_footprint_turn_over_with_it_and_a_field_named_with_its_footprint_is_not_moved_twice() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let mut reference = layout(0, -3_100, "F.SilkS");
    reference.angle = 0;
    let vendor = UserField { name: "Vendor".into(), text: "ACME".into(), layout: layout(0, 2_000, "F.Fab") };
    b.apply(&set_fields("U1", Some(reference), None, Some(vec![vendor]), None)).unwrap();

    // Moving the footprint and its field together moves the field once (the footprint carries it).
    let fp = pose(&b, "U1");
    let at = edit_of(&b, "U1").unwrap().reference.clone().unwrap().board_position(&fp);
    b.apply(&Cmd::MoveItems { ids: ids(&["U1", "U1:Reference"]), dx: 1_000, dy: 0 }).unwrap();
    let fp2 = pose(&b, "U1");
    assert_eq!(fp2.at, p(fp.at.x + 1_000, fp.at.y));
    assert_eq!(edit_of(&b, "U1").unwrap().reference.clone().unwrap().board_position(&fp2), p(at.x + 1_000, at.y));

    // Flipping the footprint flips every field: layer and mirror, nothing else.
    b.apply(&Cmd::FlipItems { ids: ids(&["U1"]), pivot: fp2.at, direction: FlipDirection::LeftRight }).unwrap();
    let e = edit_of(&b, "U1").unwrap();
    assert_eq!(pose(&b, "U1").side, Side::Bottom);
    let r = e.reference.clone().unwrap();
    assert_eq!((r.layer.as_str(), r.mirror, r.at), ("B.SilkS", true, p(0, -3_100)));
    assert_eq!((e.fields[0].layout.layer.as_str(), e.fields[0].layout.mirror), ("B.Fab", true));
    // The legacy single-part flip does the same.
    b.apply(&Cmd::Flip { part: "U1".into() }).unwrap();
    let r = edit_of(&b, "U1").unwrap().reference.clone().unwrap();
    assert_eq!((pose(&b, "U1").side, r.layer.as_str(), r.mirror), (Side::Top, "F.SilkS", false));
}

#[test]
fn a_field_of_a_footprint_that_is_not_on_the_board_is_unknown_and_a_deleted_footprint_takes_its_edit() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&set_fields("U2", Some(layout(0, -3_000, "F.SilkS")), None, None, None)).unwrap();
    b.apply(&Cmd::SetFootprintZoneConnection { part: "U2".into(), zone_connection: Some(eda_model::ir::PadConnection::Full), clearance: Some(300) }).unwrap();
    assert_eq!(b.apply(&Cmd::MoveItems { ids: ids(&["U9:Reference"]), dx: 1, dy: 0 }).unwrap_err()[0].check, "ops_unknown_item");
    assert_eq!(b.apply(&Cmd::MoveItems { ids: ids(&["U2:Nothing"]), dx: 1, dy: 0 }).unwrap_err()[0].check, "ops_unknown_item", "U2 has no such field");
    b.apply(&Cmd::Rip { part: "U2".into() }).unwrap();
    assert!(edit_of(&b, "U2").is_none(), "the edit goes with the footprint");
    assert!(drawings(&b).zone_overrides.iter().all(|z| z.id != "U2"), "and so do its zone overrides: a footprint put there again, or a copy that takes the reference, starts from the library's own");
    let _ = (design, drawings);
}

#[test]
fn one_field_is_edited_on_its_own_and_edits_of_two_fields_of_one_footprint_compose() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let user = |name: &str, y: Nm| UserField { name: name.into(), text: format!("{name} text"), layout: layout(0, y, "F.Fab") };
    b.apply(&set_fields("U1", None, None, Some(vec![user("Vendor", 2_000), user("Rev", 3_000)]), None)).unwrap();

    let mut vendor = layout(500, 2_500, "F.Fab");
    vendor.visible = true;
    let mut rev = layout(-500, 3_500, "F.SilkS");
    rev.size = (800, 800);
    b.apply(&Cmd::Batch {
        cmds: vec![
            Cmd::EditBoardField { part: "U1".into(), name: "Vendor".into(), layout: Some(vendor.clone()), text: Some("ACME".into()) },
            Cmd::EditBoardField { part: "U1".into(), name: "Rev".into(), layout: Some(rev.clone()), text: None },
        ],
    })
    .unwrap();
    let e = edit_of(&b, "U1").unwrap();
    assert_eq!((e.fields[0].layout.clone(), e.fields[0].text.as_str()), (vendor, "ACME"));
    assert_eq!((e.fields[1].layout.clone(), e.fields[1].text.as_str()), (rev, "Rev text"), "the second edit did not undo the first");

    // The Reference's layout is set; its text is not the board's to change.
    let mut r = layout(0, -3_000, "F.SilkS");
    r.bold = true;
    b.apply(&Cmd::EditBoardField { part: "U1".into(), name: "Reference".into(), layout: Some(r.clone()), text: None }).unwrap();
    assert_eq!(edit_of(&b, "U1").unwrap().reference, Some(r.clone()));
    assert!(refusal(&mut b, Cmd::EditBoardField { part: "U1".into(), name: "Reference".into(), layout: None, text: Some("R9".into()) }).contains("comes from the schematic"));
    assert!(refusal(&mut b, Cmd::EditBoardField { part: "U1".into(), name: "Nope".into(), layout: Some(r), text: None }).contains("no field called"));
    assert!(refusal(&mut b, Cmd::EditBoardField { part: "U1".into(), name: "Value".into(), layout: None, text: None }).contains("nothing to change"));
}

#[test]
fn a_pad_gets_a_padstack_the_edit_replaces_whole_and_each_layer_is_checked_as_a_pad_is() {
    use eda_model::footprint::{PadAnchor, PadCustom, PadPrimitive};
    use eda_model::ir::{ChamferCorners, Shape};
    use eda_model::padstack::{PadLayerShape, PadStack, PadStackMode, UnconnectedLayers};
    let m = model(&["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]);
    let mut b = board(&m);
    let through = |stack: PadStack| {
        let mut e = PadEdit::none("1", 1);
        e.kind = Some(PadKind::ThroughHole);
        e.shape = Some(PadShape::Circle);
        e.size = Some((1_600, 1_600));
        e.drill = Some(800);
        e.stack = Some(stack);
        Cmd::EditBoardPad { part: "U1".into(), edit: e }
    };

    // Front/inner/back: the layers' shapes, normalized as a pad's own are (a circle is square), and the unconnected-layer mode with them.
    let mut inner = PadLayerShape::plain(PadShape::Circle, (1_400, 900));
    inner.roundrect_ratio = Some(0.3);
    let stack = PadStack { mode: PadStackMode::FrontInnerBack, inner: Some(inner), back: Some(PadLayerShape::plain(PadShape::Oval, (2_400, 1_000))), unconnected: UnconnectedLayers::RemoveExceptStartAndEnd, ..Default::default() };
    b.apply(&through(stack)).unwrap();
    let pad1 = pad_after_edits(&b, &m, "U1", "1");
    assert_eq!((pad1.shape, pad1.size, pad1.drill), (PadShape::Circle, (1_600, 1_600), Some(800)));
    let s = &pad1.ext.stack;
    assert_eq!(s.mode, PadStackMode::FrontInnerBack);
    assert_eq!(s.inner.as_ref().map(|i| (i.shape, i.size, i.roundrect_ratio)), Some((PadShape::Circle, (1_400, 1_400), None)), "a circle is square and has no corner ratio");
    assert_eq!(s.back.as_ref().map(|i| i.size), Some((2_400, 1_000)));
    assert_eq!(s.unconnected, UnconnectedLayers::RemoveExceptStartAndEnd);
    // The stack gives the pad's extents the copper of every layer: the box the placer keeps clear holds the 2.4 mm oval.
    let fp = pose(&b, "U1");
    assert_eq!(eda_model::footprint::placed_pad_of(&fp, &pad1).size.0, 2_400);

    // A normal stack has no layers (and takes the old ones away): only the unconnected-layer mode stays.
    let normal = PadStack { mode: PadStackMode::Normal, inner: Some(PadLayerShape::plain(PadShape::Rect, (900, 900))), unconnected: UnconnectedLayers::RemoveAll, ..Default::default() };
    b.apply(&through(normal)).unwrap();
    let s = pad_after_edits(&b, &m, "U1", "1").ext.stack;
    assert_eq!((s.mode, s.inner.is_none(), s.unconnected), (PadStackMode::Normal, true, UnconnectedLayers::RemoveAll));

    // A layer of a custom stack is judged as the pad is: a basic shape that cannot be drawn, copper in two pieces, a chamfer too large.
    let custom_layer = |shape: PadLayerShape| through(PadStack { mode: PadStackMode::Custom, layers: [("In1.Cu".to_string(), shape)].into(), ..Default::default() });
    let line = |a: Point, z: Point| PadPrimitive { shape: Shape::Segment { id: String::new(), layer: String::new(), stroke_width: 100, filled: false, start: a, end: z }, proxy: false };
    let mut bad_shape = PadLayerShape::plain(PadShape::Custom, (1_000, 1_000));
    bad_shape.custom = Some(PadCustom { anchor: PadAnchor::Circle, zone: Default::default(), primitives: vec![line(p(0, 0), p(0, 0))] });
    assert!(refusal(&mut b, custom_layer(bad_shape)).contains("Basic shape 1 on In1.Cu"));
    let mut two_pieces = PadLayerShape::plain(PadShape::Custom, (1_000, 1_000));
    two_pieces.custom = Some(PadCustom { anchor: PadAnchor::Circle, zone: Default::default(), primitives: vec![line(p(5_000, 5_000), p(6_000, 5_000))] });
    assert!(refusal(&mut b, custom_layer(two_pieces)).contains("single polygon"));
    let mut chamfer = PadLayerShape::plain(PadShape::ChamferedRect, (1_000, 1_000));
    chamfer.chamfer_ratio = Some(0.7);
    chamfer.chamfer_corners = ChamferCorners { top_left: true, top_right: true, ..Default::default() };
    assert!(refusal(&mut b, custom_layer(chamfer)).contains("corner chamfer is too large"));
    let negative = PadLayerShape::plain(PadShape::Rect, (-5, 1_000));
    assert!(refusal(&mut b, custom_layer(negative)).contains("must not be negative"));
    // The edit that was refused left the one before it.
    assert_eq!(pad_after_edits(&b, &m, "U1", "1").ext.stack.unconnected, UnconnectedLayers::RemoveAll);
}

#[test]
fn a_footprints_pad_edit_that_differs_from_the_libraries_padstack_is_found_by_pad_edits_between() {
    use eda_model::padstack::{PadLayerShape, PadStack, PadStackMode};
    let m = model(&["F.Cu", "B.Cu"]);
    let lib = m.library_footprint_of(m.part("U1").unwrap()).unwrap();
    let mut edited = lib.clone();
    edited.pads[0].ext.stack = PadStack { mode: PadStackMode::FrontInnerBack, back: Some(PadLayerShape::plain(PadShape::Oval, (2_000, 900))), ..Default::default() };
    let edits = eda_model::fp_edit::pad_edits_between(&lib, &edited);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].stack.as_ref().map(|s| s.mode), Some(PadStackMode::FrontInnerBack));
    // The patched library footprint is the edited one again.
    assert_eq!(eda_model::fp_edit::patched_footprint(&lib, &edits).pads[0].ext.stack.back, edited.pads[0].ext.stack.back);
}
