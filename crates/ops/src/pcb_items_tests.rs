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

// ----------------------------------------------------------------------------------------------------------------------------- reference images

use eda_model::image_info::{base64_encode, solid_png};
use eda_model::pcb_items::RefImage;

fn png_text(w: u32, h: u32, rgb: [u8; 3]) -> String {
    base64_encode(&solid_png(w, h, rgb, 254))
}

fn images<'a>(b: &'a Board<'_>) -> &'a [RefImage] {
    &drawings(b).board_items.images
}

fn add_image(b: &mut Board<'_>, layer: &str, at: Point) -> String {
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddRefImage { image: RefImage::new(layer, at, &png_text(60, 40, [1, 2, 3])) })).unwrap();
    images(b).last().unwrap().id.clone()
}

#[test]
fn a_reference_image_is_added_with_an_id_of_ours_edited_and_deleted() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, 100, 300);
    let mut image = RefImage::new("Cmts.User", p(5_000, 6_000), &png_text(60, 40, [1, 2, 3]));
    image.id = "caller_id".into();
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddRefImage { image })).unwrap();
    let id = images(&b)[0].id.clone();
    assert!(id.starts_with("img_"), "{id}");
    assert_eq!(images(&b)[0].bounds(), [2_000, 4_000, 8_000, 8_000], "60 x 40 px at 254 ppi are 6 mm x 4 mm around the centre");

    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::EditRefImage { id: id.clone(), layer: "Dwgs.User".into(), at: p(0, 0), scale_ppm: 2_000_000 })).unwrap();
    let i = &images(&b)[0];
    assert_eq!((i.id.as_str(), i.layer.as_str(), i.at, i.scale_ppm), (id.as_str(), "Dwgs.User", p(0, 0), 2_000_000));
    assert_eq!(i.size_um(), (12_000, 8_000), "the scale doubles the size");

    let greyer = png_text(60, 40, [9, 9, 9]);
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::SetRefImageData { id: id.clone(), data: greyer.clone() })).unwrap();
    assert_eq!((images(&b)[0].data.as_str(), images(&b)[0].scale_ppm), (greyer.as_str(), 2_000_000), "new pixels keep the place, the layer and the scale");

    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::DeleteRefImage { id: id.clone() })).unwrap();
    assert!(images(&b).is_empty());
    for cmd in [
        pcb_items::PcbItemCmd::DeleteRefImage { id: id.clone() },
        pcb_items::PcbItemCmd::EditRefImage { id: id.clone(), layer: "Dwgs.User".into(), at: p(0, 0), scale_ppm: 1_000_000 },
        pcb_items::PcbItemCmd::SetRefImageData { id: id.clone(), data: greyer.clone() },
    ] {
        assert_eq!(b.apply(&Cmd::PcbItem(cmd)).unwrap_err()[0].check, "ops_unknown_ref_image");
    }
}

#[test]
fn a_reference_image_that_cannot_be_shown_is_refused() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, 100, 300);
    let try_add = |b: &mut Board<'_>, mutate: &dyn Fn(&mut RefImage)| {
        let mut i = RefImage::new("Cmts.User", p(0, 0), &png_text(20, 20, [0, 0, 0]));
        mutate(&mut i);
        let e = b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddRefImage { image: i })).unwrap_err();
        assert!(images(b).is_empty(), "nothing was added");
        e[0].check.clone()
    };
    assert_eq!(try_add(&mut b, &|i| i.data = "AAAA".into()), "ops_bad_ref_image", "data that is no image");
    assert_eq!(try_add(&mut b, &|i| i.data = String::new()), "ops_bad_ref_image");
    assert_eq!(try_add(&mut b, &|i| i.scale_ppm = 0), "ops_bad_ref_image");
    assert_eq!(try_add(&mut b, &|i| i.scale_ppm = -5), "ops_bad_ref_image");
    assert_eq!(try_add(&mut b, &|i| i.layer = String::new()), "ops_bad_ref_image");
    let mut off = model(&["F.Cu", "B.Cu"]);
    off.board.layer_setup.enabled.retain(|l| l != "Cmts.User");
    let mut c = Board::new(design(), &off, 100, 300);
    assert_eq!(c.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddRefImage { image: RefImage::new("Cmts.User", p(0, 0), &png_text(20, 20, [0, 0, 0])) })).unwrap_err()[0].check, "ops_layer_off");
    // the white space of a pasted text is taken out
    let spaced = format!("{}\n{}", &png_text(20, 20, [0, 0, 0])[..8], &png_text(20, 20, [0, 0, 0])[8..]);
    let mut i = RefImage::new("Cmts.User", p(0, 0), "");
    i.data = spaced;
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddRefImage { image: i })).unwrap();
    assert_eq!(images(&b)[0].data, png_text(20, 20, [0, 0, 0]));
}

#[test]
fn a_reference_image_moves_with_the_verbs_that_take_any_item_and_a_flip_changes_its_side() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, 100, 300);
    let id = add_image(&mut b, "F.SilkS", p(10_000, 20_000));
    b.apply(&Cmd::MoveItems { ids: ids(&[&id]), dx: 1_000, dy: -2_000 }).unwrap();
    assert_eq!(images(&b)[0].at, p(11_000, 18_000));
    b.apply(&Cmd::RotateItems { ids: ids(&[&id]), pivot: p(0, 0), angle_millideg: 90_000 }).unwrap();
    assert_eq!(images(&b)[0].at, p(-18_000, 11_000), "clockwise on the screen about the pivot");
    b.apply(&Cmd::FlipItems { ids: ids(&[&id]), pivot: p(0, 0), direction: FlipDirection::LeftRight }).unwrap();
    assert_eq!((images(&b)[0].layer.as_str(), images(&b)[0].at), ("B.SilkS", p(18_000, 11_000)));
    b.apply(&Cmd::SetLocked { ids: ids(&[&id]), locked: true }).unwrap();
    assert!(drawings(&b).locked_ids.contains(&id));
    b.apply(&Cmd::Group { ids: ids(&[&id, "shp_seg"]) }).unwrap();
    let group = drawings(&b).groups.last().unwrap().id.clone();
    b.apply(&Cmd::SetLocked { ids: ids(&[&id]), locked: false }).unwrap();
    b.apply(&Cmd::MoveItems { ids: ids(&[&group]), dx: 500, dy: 0 }).unwrap();
    assert_eq!(images(&b)[0].at, p(18_500, 11_000), "the group carries the image");
}

#[test]
fn the_image_commands_have_one_wire_form() {
    let data = png_text(20, 20, [0, 0, 0]);
    let cmd: Cmd = serde_json::from_value(serde_json::json!({
        "op": "pcb_item", "verb": "add_ref_image",
        "image": { "layer": "Cmts.User", "at": {"x": 1000, "y": 2000}, "scale_ppm": 500000, "data": data }
    }))
    .unwrap();
    let Cmd::PcbItem(pcb_items::PcbItemCmd::AddRefImage { image }) = &cmd else { panic!("{cmd:?}") };
    assert_eq!((image.layer.as_str(), image.at, image.scale_ppm), ("Cmts.User", p(1_000, 2_000), 500_000));
    assert_eq!(cmd.domain(), Domain::Pcb);
    // the scale of one is not sent
    let unit: Cmd = serde_json::from_value(serde_json::json!({"op": "pcb_item", "verb": "add_ref_image", "image": {"layer": "F.SilkS", "at": {"x": 0, "y": 0}, "data": data}})).unwrap();
    let Cmd::PcbItem(pcb_items::PcbItemCmd::AddRefImage { image }) = &unit else { panic!() };
    assert_eq!(image.scale_ppm, 1_000_000);
    let edit: Cmd = serde_json::from_value(serde_json::json!({"op": "pcb_item", "verb": "edit_ref_image", "id": "img_x", "layer": "F.SilkS", "at": {"x": 1, "y": 2}, "scale_ppm": 750000})).unwrap();
    assert_eq!(edit.subjects(), vec!["img_x"]);
    let back: Cmd = serde_json::from_value(serde_json::to_value(&cmd).unwrap()).unwrap();
    assert_eq!(back, cmd);
    let Cmd::PcbItem(c) = &cmd else { panic!() };
    assert!(c.describe().starts_with("ref-image add --layer Cmts.User --at 1.000,2.000 --scale 0.5"), "{}", c.describe());
}

// ----------------------------------------------------------------------------------------------------------------------------- points

use eda_model::pcb_items::PcbPoint;

fn points<'a>(b: &'a Board<'_>) -> &'a [PcbPoint] {
    &drawings(b).board_items.points
}

fn add_point(b: &mut Board<'_>, layer: &str, at: Point) -> String {
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddPoint { point: PcbPoint::new(layer, at) })).unwrap();
    points(b).last().unwrap().id.clone()
}

#[test]
fn a_point_is_added_with_an_id_of_ours_edited_in_place_and_deleted() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, 100, 300);
    let mut p = PcbPoint::new("F.SilkS", p(5_000, 6_000));
    p.id = "caller_id".into();
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddPoint { point: p })).unwrap();
    let id = points(&b)[0].id.clone();
    assert!(id.starts_with("pnt_"), "{id}");
    assert_eq!((points(&b)[0].size_um, points(&b)[0].bounds()), (1_000, [4_500, 5_500, 5_500, 6_500]), "a new point is 1 mm");

    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::EditPoint { id: id.clone(), layer: "F.Fab".into(), at: p_at(1_000, 2_000), size_um: 2_500 })).unwrap();
    let q = &points(&b)[0];
    assert_eq!((q.id.as_str(), q.layer.as_str(), q.at, q.size_um), (id.as_str(), "F.Fab", p_at(1_000, 2_000), 2_500), "an edit keeps the id");

    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::DeletePoint { id: id.clone() })).unwrap();
    assert!(points(&b).is_empty());
    assert_eq!(b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::DeletePoint { id: id.clone() })).unwrap_err()[0].check, "ops_unknown_point");
    assert_eq!(b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::EditPoint { id, layer: "F.Fab".into(), at: p_at(0, 0), size_um: 1_000 })).unwrap_err()[0].check, "ops_unknown_point");
}

fn p_at(x: i64, y: i64) -> Point {
    p(x, y)
}

#[test]
fn a_point_that_cannot_be_drawn_is_refused() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, 100, 300);
    let refused = |b: &mut Board<'_>, mutate: &dyn Fn(&mut PcbPoint)| {
        let mut q = PcbPoint::new("F.SilkS", p_at(0, 0));
        mutate(&mut q);
        let e = b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddPoint { point: q })).unwrap_err();
        assert!(points(b).is_empty(), "nothing was added");
        e[0].check.clone()
    };
    assert_eq!(refused(&mut b, &|q| q.size_um = 0), "ops_bad_point");
    assert_eq!(refused(&mut b, &|q| q.size_um = -5), "ops_bad_point");
    assert_eq!(refused(&mut b, &|q| q.layer = String::new()), "ops_bad_point");
    let mut off = model(&["F.Cu", "B.Cu"]);
    off.board.layer_setup.enabled.retain(|l| l != "F.Fab");
    let mut c = Board::new(design(), &off, 100, 300);
    assert_eq!(c.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddPoint { point: PcbPoint::new("F.Fab", p_at(0, 0)) })).unwrap_err()[0].check, "ops_layer_off");
}

#[test]
fn a_point_moves_turns_and_flips_with_the_verbs_that_take_any_item_but_cannot_be_locked() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, 100, 300);
    let id = add_point(&mut b, "F.SilkS", p_at(10_000, 20_000));
    b.apply(&Cmd::MoveItems { ids: ids(&[&id]), dx: 1_000, dy: -2_000 }).unwrap();
    assert_eq!(points(&b)[0].at, p_at(11_000, 18_000));
    b.apply(&Cmd::RotateItems { ids: ids(&[&id]), pivot: p_at(0, 0), angle_millideg: 90_000 }).unwrap();
    assert_eq!(points(&b)[0].at, p_at(-18_000, 11_000), "clockwise on the screen about the pivot");
    b.apply(&Cmd::FlipItems { ids: ids(&[&id]), pivot: p_at(0, 0), direction: FlipDirection::LeftRight }).unwrap();
    assert_eq!((points(&b)[0].layer.as_str(), points(&b)[0].at), ("B.SilkS", p_at(18_000, 11_000)));
    // the file has no lock for a point
    assert_eq!(b.apply(&Cmd::SetLocked { ids: ids(&[&id]), locked: true }).unwrap_err()[0].check, "ops_unknown_item");
}

#[test]
fn the_point_commands_have_one_wire_form() {
    let add: Cmd = serde_json::from_value(serde_json::json!({"op": "pcb_item", "verb": "add_point", "point": {"layer": "F.SilkS", "at": {"x": 1000, "y": 2000}}})).unwrap();
    let Cmd::PcbItem(pcb_items::PcbItemCmd::AddPoint { point }) = &add else { panic!("{add:?}") };
    assert_eq!((point.layer.as_str(), point.at, point.size_um), ("F.SilkS", p_at(1_000, 2_000), 1_000), "the size left out is 1 mm");
    assert_eq!(add.domain(), Domain::Pcb);
    let edit: Cmd = serde_json::from_value(serde_json::json!({"op": "pcb_item", "verb": "edit_point", "id": "pnt_x", "layer": "F.Cu", "at": {"x": 1, "y": 2}, "size_um": 3000})).unwrap();
    assert_eq!(edit.subjects(), vec!["pnt_x"]);
    let Cmd::PcbItem(c) = &edit else { panic!() };
    assert_eq!(c.describe(), "point edit pnt_x --layer F.Cu --at 0.001,0.002 --size 3.000");
    let back: Cmd = serde_json::from_value(serde_json::to_value(&add).unwrap()).unwrap();
    assert_eq!(back, add);
}
