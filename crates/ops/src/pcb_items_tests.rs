//! The verbs of the board items of GAPS.md item 24 (package P16): a text box added, edited and deleted; what the verbs that take any item (Move, Rotate, Flip, Lock, Group, Swap
//! Layers) do with one; and the wire form of the command.

use super::pcb_transform_tests::{design, drawings, ids, model, p};
use super::*;
use eda_model::pcb_items::TextBox;

fn text_box(layer: &str) -> TextBox {
    let mut t = TextBox::new(layer, p(10_000, 10_000), p(30_000, 20_000), 1_000, 120);
    t.text = "REV A\nsecond line".into();
    t
}

fn boxes<'a>(b: &'a Board<'_>) -> &'a [TextBox] {
    &drawings(b).board_items.text_boxes
}

fn add(b: &mut Board<'_>, t: TextBox) -> String {
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddTextBox { text_box: t })).unwrap();
    boxes(b).last().unwrap().id.clone()
}

#[test]
fn a_text_box_is_added_with_an_id_of_ours_edited_in_place_and_deleted() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, 100, 300);
    let mut t = text_box("F.SilkS");
    t.id = "caller_id".into();
    let id = add(&mut b, t);
    assert!(id.starts_with("tbx_"), "an id the caller sent is ignored: {id}");
    assert_eq!(boxes(&b).len(), 1);

    let mut edited = boxes(&b)[0].clone();
    edited.text = "REV B".into();
    edited.layer = "F.Fab".into();
    edited.knockout = true;
    edited.margin_top_um = 2_000;
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::EditTextBox { id: id.clone(), text_box: edited })).unwrap();
    let t = &boxes(&b)[0];
    assert_eq!((t.id.as_str(), t.text.as_str(), t.layer.as_str(), t.knockout, t.margin_top_um), (id.as_str(), "REV B", "F.Fab", true, 2_000), "an edit keeps the id");

    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::DeleteTextBox { id: id.clone() })).unwrap();
    assert!(boxes(&b).is_empty());
    assert_eq!(b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::DeleteTextBox { id: id.clone() })).unwrap_err()[0].check, "ops_unknown_text_box");
    assert_eq!(b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::EditTextBox { id, text_box: text_box("F.SilkS") })).unwrap_err()[0].check, "ops_unknown_text_box");
}

#[test]
fn a_text_box_that_cannot_be_drawn_is_refused_and_leaves_the_board_as_it_was() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, 100, 300);
    let refused = |b: &mut Board<'_>, mutate: &dyn Fn(&mut TextBox)| {
        let mut t = text_box("F.SilkS");
        mutate(&mut t);
        let before = boxes(b).len();
        let e = b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddTextBox { text_box: t })).unwrap_err();
        assert_eq!(boxes(b).len(), before, "nothing was added");
        e[0].check.clone()
    };
    assert_eq!(refused(&mut b, &|t| t.layer = String::new()), "ops_bad_text_box");
    assert_eq!(refused(&mut b, &|t| t.size_um = 0), "ops_bad_text_box");
    assert_eq!(refused(&mut b, &|t| t.size_um = 300_000), "ops_bad_text_box", "250 mm is the largest text");
    assert_eq!(refused(&mut b, &|t| t.width_um = Some(0)), "ops_bad_text_box");
    assert_eq!(refused(&mut b, &|t| t.margin_left_um = -1), "ops_bad_text_box");
    assert_eq!(refused(&mut b, &|t| t.stroke_width_um = -1), "ops_bad_text_box");
    assert_eq!(refused(&mut b, &|t| t.pts = vec![p(0, 0), p(1, 1)]), "ops_bad_text_box", "a polygon box has four corners");
    assert_eq!(refused(&mut b, &|t| t.angle = 360_000), "ops_bad_text_box");
    // a layer the board has turned off takes nothing (Board Editor Layers)
    let mut off = model(&["F.Cu", "B.Cu"]);
    off.board.layer_setup.enabled.retain(|l| l != "F.Fab");
    let mut c = Board::new(design(), &off, 100, 300);
    assert_eq!(c.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddTextBox { text_box: text_box("F.Fab") })).unwrap_err()[0].check, "ops_layer_off");
}

#[test]
fn a_text_box_moves_turns_and_flips_with_the_verbs_that_take_any_item() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, 100, 300);
    let id = add(&mut b, text_box("F.SilkS"));
    b.apply(&Cmd::MoveItems { ids: ids(&[&id]), dx: 1_000, dy: -2_000 }).unwrap();
    assert_eq!((boxes(&b)[0].start, boxes(&b)[0].end), (p(11_000, 8_000), p(31_000, 18_000)));

    b.apply(&Cmd::RotateItems { ids: ids(&[&id]), pivot: p(0, 0), angle_millideg: 90_000 }).unwrap();
    let t = &boxes(&b)[0];
    assert_eq!((t.start, t.end, t.angle), (p(-8_000, 11_000), p(-18_000, 31_000), 270_000));
    assert!(t.is_rect(), "a quarter turn keeps a rectangle");

    b.apply(&Cmd::FlipItems { ids: ids(&[&id]), pivot: p(0, 0), direction: FlipDirection::LeftRight }).unwrap();
    let t = &boxes(&b)[0];
    assert_eq!((t.layer.as_str(), t.mirror, t.start), ("B.SilkS", true, p(8_000, 11_000)));
    assert_eq!(t.angle, 90_000, "negated by a left-right flip");
}

#[test]
fn a_group_carries_a_text_box_and_a_lock_keeps_it_in_the_set() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, 100, 300);
    let id = add(&mut b, text_box("F.SilkS"));
    b.apply(&Cmd::Group { ids: ids(&[&id, "shp_seg"]) }).unwrap();
    let group = drawings(&b).groups.last().unwrap().id.clone();
    b.apply(&Cmd::MoveItems { ids: ids(&[&group]), dx: 500, dy: 0 }).unwrap();
    assert_eq!(boxes(&b)[0].start, p(10_500, 10_000), "the group moves its text box");
    b.apply(&Cmd::SetLocked { ids: ids(&[&id]), locked: true }).unwrap();
    assert!(drawings(&b).locked_ids.contains(&id));
    b.apply(&Cmd::SetLocked { ids: ids(&[&id]), locked: false }).unwrap();
    assert!(!drawings(&b).locked_ids.contains(&id));
}

#[test]
fn swap_layers_moves_a_text_box_to_the_other_layer() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, 100, 300);
    // Swap Layers is about the copper layers: a text box on one goes to the other
    add(&mut b, text_box("F.Cu"));
    b.apply(&Cmd::SwapLayers { mapping: vec![("F.Cu".into(), "B.Cu".into())] }).unwrap();
    assert_eq!(boxes(&b)[0].layer, "B.Cu");
}

#[test]
fn the_command_has_one_wire_form_and_describes_itself() {
    let json = serde_json::json!({
        "op": "pcb_item", "verb": "add_text_box",
        "text_box": {
            "layer": "F.SilkS", "text": "hi", "start": {"x": 0, "y": 0}, "end": {"x": 5000, "y": 3000},
            "margin_left_um": 810, "margin_top_um": 810, "margin_right_um": 810, "margin_bottom_um": 810,
            "stroke_width_um": 120, "size_um": 1000, "line_style": "dash", "halign": "left", "valign": "top"
        }
    });
    let cmd: Cmd = serde_json::from_value(json).unwrap();
    let Cmd::PcbItem(pcb_items::PcbItemCmd::AddTextBox { text_box }) = &cmd else { panic!("{cmd:?}") };
    assert_eq!((text_box.text.as_str(), text_box.end, text_box.border, text_box.line_style), ("hi", p(5_000, 3_000), true, eda_model::pcb_items::LineStyle::Dash));
    assert_eq!(cmd.domain(), Domain::Pcb);
    assert!(!cmd.clears_routing(), "a drawing never clears the routing");
    let back: Cmd = serde_json::from_value(serde_json::to_value(&cmd).unwrap()).unwrap();
    assert_eq!(back, cmd);
    let delete: Cmd = serde_json::from_value(serde_json::json!({"op": "pcb_item", "verb": "delete_text_box", "id": "tbx_x"})).unwrap();
    assert_eq!(delete.subjects(), vec!["tbx_x"]);
}
