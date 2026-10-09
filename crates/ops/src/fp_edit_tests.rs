//! `EditBoardFootprint`, `EditBoardPad` and the fields of a footprint moving, turning and flipping on their own.

use super::pcb_transform_tests::{board, design, drawings, ids, model, p, pose};
use super::*;
use eda_model::fp_edit::{FieldLayout, FootprintAttrs, FootprintEdit, FootprintKind, PadEdit, UserField};
use eda_model::{PadKind, PadShape};

fn layout(x: Um, y: Um, layer: &str) -> FieldLayout {
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
    let user = |name: &str, y: Um| UserField { name: name.into(), text: format!("{name} text"), layout: layout(0, y, "F.Fab") };
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
