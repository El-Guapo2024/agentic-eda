//! The verbs of the board items of GAPS.md item 24 (package P16): a text box added, edited and deleted; what the verbs that take any item (Move, Rotate, Flip, Lock, Group, Swap
//! Layers) do with one; and the wire form of the command.

use super::pcb_transform_tests::{design, drawings, ids, model, p};
use super::*;
use eda_model::pcb_items::TextBox;

fn text_box(layer: &str) -> TextBox {
    let mut t = TextBox::new(layer, p(Nm::from_raw_shim(10_000_000), Nm::from_raw_shim(10_000_000)), p(Nm::from_raw_shim(30_000_000), Nm::from_raw_shim(20_000_000)), Nm::from_raw_shim(1_000_000), Nm::from_raw_shim(120_000));
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
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let mut t = text_box("F.SilkS");
    t.id = "caller_id".into();
    let id = add(&mut b, t);
    assert!(id.starts_with("tbx_"), "an id the caller sent is ignored: {id}");
    assert_eq!(boxes(&b).len(), 1);

    let mut edited = boxes(&b)[0].clone();
    edited.text = "REV B".into();
    edited.layer = "F.Fab".into();
    edited.knockout = true;
    edited.margin_top_nm = Nm::from_raw_shim(2_000_000);
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::EditTextBox { id: id.clone(), text_box: edited })).unwrap();
    let t = &boxes(&b)[0];
    assert_eq!((t.id.as_str(), t.text.as_str(), t.layer.as_str(), t.knockout, t.margin_top_nm), (id.as_str(), "REV B", "F.Fab", true, Nm::from_raw_shim(2_000_000)), "an edit keeps the id");

    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::DeleteTextBox { id: id.clone() })).unwrap();
    assert!(boxes(&b).is_empty());
    assert_eq!(b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::DeleteTextBox { id: id.clone() })).unwrap_err()[0].check, "ops_unknown_text_box");
    assert_eq!(b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::EditTextBox { id, text_box: text_box("F.SilkS") })).unwrap_err()[0].check, "ops_unknown_text_box");
}

#[test]
fn a_text_box_that_cannot_be_drawn_is_refused_and_leaves_the_board_as_it_was() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let refused = |b: &mut Board<'_>, mutate: &dyn Fn(&mut TextBox)| {
        let mut t = text_box("F.SilkS");
        mutate(&mut t);
        let before = boxes(b).len();
        let e = b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddTextBox { text_box: t })).unwrap_err();
        assert_eq!(boxes(b).len(), before, "nothing was added");
        e[0].check.clone()
    };
    assert_eq!(refused(&mut b, &|t| t.layer = String::new()), "ops_bad_text_box");
    assert_eq!(refused(&mut b, &|t| t.size_nm = Nm::ZERO), "ops_bad_text_box");
    assert_eq!(refused(&mut b, &|t| t.size_nm = Nm::from_raw_shim(300_000_000)), "ops_bad_text_box", "250 mm is the largest text");
    assert_eq!(refused(&mut b, &|t| t.width_nm = Some(Nm::ZERO)), "ops_bad_text_box");
    assert_eq!(refused(&mut b, &|t| t.margin_left_nm = Nm::from_raw_shim(-1)), "ops_bad_text_box");
    assert_eq!(refused(&mut b, &|t| t.stroke_width_nm = Nm::from_raw_shim(-1)), "ops_bad_text_box");
    assert_eq!(refused(&mut b, &|t| t.pts = vec![p(Nm::ZERO, Nm::ZERO), p(Nm::from_raw_shim(1), Nm::from_raw_shim(1))]), "ops_bad_text_box", "a polygon box has four corners");
    assert_eq!(refused(&mut b, &|t| t.angle = 360_000), "ops_bad_text_box");
    // a layer the board has turned off takes nothing (Board Editor Layers)
    let mut off = model(&["F.Cu", "B.Cu"]);
    off.board.layer_setup.enabled.retain(|l| l != "F.Fab");
    let mut c = Board::new(design(), &off, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    assert_eq!(c.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddTextBox { text_box: text_box("F.Fab") })).unwrap_err()[0].check, "ops_layer_off");
}

#[test]
fn a_text_box_moves_turns_and_flips_with_the_verbs_that_take_any_item() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let id = add(&mut b, text_box("F.SilkS"));
    b.apply(&Cmd::MoveItems { ids: ids(&[&id]), dx: Nm::from_raw_shim(1_000_000), dy: Nm::from_raw_shim(-2_000_000) }).unwrap();
    assert_eq!((boxes(&b)[0].start, boxes(&b)[0].end), (p(Nm::from_raw_shim(11_000_000), Nm::from_raw_shim(8_000_000)), p(Nm::from_raw_shim(31_000_000), Nm::from_raw_shim(18_000_000))));

    b.apply(&Cmd::RotateItems { ids: ids(&[&id]), pivot: p(Nm::ZERO, Nm::ZERO), angle_millideg: 90_000 }).unwrap();
    let t = &boxes(&b)[0];
    assert_eq!((t.start, t.end, t.angle), (p(Nm::from_raw_shim(-8_000_000), Nm::from_raw_shim(11_000_000)), p(Nm::from_raw_shim(-18_000_000), Nm::from_raw_shim(31_000_000)), 270_000));
    assert!(t.is_rect(), "a quarter turn keeps a rectangle");

    b.apply(&Cmd::FlipItems { ids: ids(&[&id]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    let t = &boxes(&b)[0];
    assert_eq!((t.layer.as_str(), t.mirror, t.start), ("B.SilkS", true, p(Nm::from_raw_shim(8_000_000), Nm::from_raw_shim(11_000_000))));
    assert_eq!(t.angle, 90_000, "negated by a left-right flip");
}

#[test]
fn a_group_carries_a_text_box_and_a_lock_keeps_it_in_the_set() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let id = add(&mut b, text_box("F.SilkS"));
    b.apply(&Cmd::Group { ids: ids(&[&id, "shp_seg"]) }).unwrap();
    let group = drawings(&b).groups.last().unwrap().id.clone();
    b.apply(&Cmd::MoveItems { ids: ids(&[&group]), dx: Nm::from_raw_shim(500_000), dy: Nm::ZERO }).unwrap();
    assert_eq!(boxes(&b)[0].start, p(Nm::from_raw_shim(10_500_000), Nm::from_raw_shim(10_000_000)), "the group moves its text box");
    b.apply(&Cmd::SetLocked { ids: ids(&[&id]), locked: true }).unwrap();
    assert!(drawings(&b).locked_ids.contains(&id));
    b.apply(&Cmd::SetLocked { ids: ids(&[&id]), locked: false }).unwrap();
    assert!(!drawings(&b).locked_ids.contains(&id));
}

#[test]
fn swap_layers_moves_a_text_box_to_the_other_layer() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
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
            "layer": "F.SilkS", "text": "hi", "start": {"x": 0, "y": 0}, "end": {"x": 5_000_000, "y": 3_000_000},
            "margin_left_nm": 810_000, "margin_top_nm": 810_000, "margin_right_nm": 810_000, "margin_bottom_nm": 810_000,
            "stroke_width_nm": 120_000, "size_nm": 1_000_000, "line_style": "dash", "halign": "left", "valign": "top"
        }
    });
    let cmd: Cmd = serde_json::from_value(json).unwrap();
    let Cmd::PcbItem(pcb_items::PcbItemCmd::AddTextBox { text_box }) = &cmd else { panic!("{cmd:?}") };
    assert_eq!((text_box.text.as_str(), text_box.end, text_box.border, text_box.line_style), ("hi", p(Nm::from_raw_shim(5_000_000), Nm::from_raw_shim(3_000_000)), true, eda_model::pcb_items::LineStyle::Dash));
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
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let mut image = RefImage::new("Cmts.User", p(Nm::from_raw_shim(5_000_000), Nm::from_raw_shim(6_000_000)), &png_text(60, 40, [1, 2, 3]));
    image.id = "caller_id".into();
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddRefImage { image })).unwrap();
    let id = images(&b)[0].id.clone();
    assert!(id.starts_with("img_"), "{id}");
    assert_eq!(images(&b)[0].bounds(), [Nm::from_raw_shim(2_000_000), Nm::from_raw_shim(4_000_000), Nm::from_raw_shim(8_000_000), Nm::from_raw_shim(8_000_000)], "60 x 40 px at 254 ppi are 6 mm x 4 mm around the centre");

    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::EditRefImage { id: id.clone(), layer: "Dwgs.User".into(), at: p(Nm::ZERO, Nm::ZERO), scale_ppm: 2_000_000 })).unwrap();
    let i = &images(&b)[0];
    assert_eq!((i.id.as_str(), i.layer.as_str(), i.at, i.scale_ppm), (id.as_str(), "Dwgs.User", p(Nm::ZERO, Nm::ZERO), 2_000_000));
    assert_eq!(i.size_nm(), (Nm::from_raw_shim(12_000_000), Nm::from_raw_shim(8_000_000)), "the scale doubles the size");

    let greyer = png_text(60, 40, [9, 9, 9]);
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::SetRefImageData { id: id.clone(), data: greyer.clone() })).unwrap();
    assert_eq!((images(&b)[0].data.as_str(), images(&b)[0].scale_ppm), (greyer.as_str(), 2_000_000), "new pixels keep the place, the layer and the scale");

    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::DeleteRefImage { id: id.clone() })).unwrap();
    assert!(images(&b).is_empty());
    for cmd in [
        pcb_items::PcbItemCmd::DeleteRefImage { id: id.clone() },
        pcb_items::PcbItemCmd::EditRefImage { id: id.clone(), layer: "Dwgs.User".into(), at: p(Nm::ZERO, Nm::ZERO), scale_ppm: 1_000_000 },
        pcb_items::PcbItemCmd::SetRefImageData { id: id.clone(), data: greyer.clone() },
    ] {
        assert_eq!(b.apply(&Cmd::PcbItem(cmd)).unwrap_err()[0].check, "ops_unknown_ref_image");
    }
}

#[test]
fn a_reference_image_that_cannot_be_shown_is_refused() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let try_add = |b: &mut Board<'_>, mutate: &dyn Fn(&mut RefImage)| {
        let mut i = RefImage::new("Cmts.User", p(Nm::ZERO, Nm::ZERO), &png_text(20, 20, [0, 0, 0]));
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
    let mut c = Board::new(design(), &off, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    assert_eq!(c.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddRefImage { image: RefImage::new("Cmts.User", p(Nm::ZERO, Nm::ZERO), &png_text(20, 20, [0, 0, 0])) })).unwrap_err()[0].check, "ops_layer_off");
    // the white space of a pasted text is taken out
    let spaced = format!("{}\n{}", &png_text(20, 20, [0, 0, 0])[..8], &png_text(20, 20, [0, 0, 0])[8..]);
    let mut i = RefImage::new("Cmts.User", p(Nm::ZERO, Nm::ZERO), "");
    i.data = spaced;
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddRefImage { image: i })).unwrap();
    assert_eq!(images(&b)[0].data, png_text(20, 20, [0, 0, 0]));
}

#[test]
fn a_reference_image_moves_with_the_verbs_that_take_any_item_and_a_flip_changes_its_side() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let id = add_image(&mut b, "F.SilkS", p(Nm::from_raw_shim(10_000_000), Nm::from_raw_shim(20_000_000)));
    b.apply(&Cmd::MoveItems { ids: ids(&[&id]), dx: Nm::from_raw_shim(1_000_000), dy: Nm::from_raw_shim(-2_000_000) }).unwrap();
    assert_eq!(images(&b)[0].at, p(Nm::from_raw_shim(11_000_000), Nm::from_raw_shim(18_000_000)));
    b.apply(&Cmd::RotateItems { ids: ids(&[&id]), pivot: p(Nm::ZERO, Nm::ZERO), angle_millideg: 90_000 }).unwrap();
    assert_eq!(images(&b)[0].at, p(Nm::from_raw_shim(-18_000_000), Nm::from_raw_shim(11_000_000)), "clockwise on the screen about the pivot");
    b.apply(&Cmd::FlipItems { ids: ids(&[&id]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    assert_eq!((images(&b)[0].layer.as_str(), images(&b)[0].at), ("B.SilkS", p(Nm::from_raw_shim(18_000_000), Nm::from_raw_shim(11_000_000))));
    b.apply(&Cmd::SetLocked { ids: ids(&[&id]), locked: true }).unwrap();
    assert!(drawings(&b).locked_ids.contains(&id));
    b.apply(&Cmd::Group { ids: ids(&[&id, "shp_seg"]) }).unwrap();
    let group = drawings(&b).groups.last().unwrap().id.clone();
    b.apply(&Cmd::SetLocked { ids: ids(&[&id]), locked: false }).unwrap();
    b.apply(&Cmd::MoveItems { ids: ids(&[&group]), dx: Nm::from_raw_shim(500_000), dy: Nm::ZERO }).unwrap();
    assert_eq!(images(&b)[0].at, p(Nm::from_raw_shim(18_500_000), Nm::from_raw_shim(11_000_000)), "the group carries the image");
}

#[test]
fn the_image_commands_have_one_wire_form() {
    let data = png_text(20, 20, [0, 0, 0]);
    let cmd: Cmd = serde_json::from_value(serde_json::json!({
        "op": "pcb_item", "verb": "add_ref_image",
        "image": { "layer": "Cmts.User", "at": {"x": 1_000_000, "y": 2_000_000}, "scale_ppm": 500000, "data": data }
    }))
    .unwrap();
    let Cmd::PcbItem(pcb_items::PcbItemCmd::AddRefImage { image }) = &cmd else { panic!("{cmd:?}") };
    assert_eq!((image.layer.as_str(), image.at, image.scale_ppm), ("Cmts.User", p(Nm::from_raw_shim(1_000_000), Nm::from_raw_shim(2_000_000)), 500_000));
    assert_eq!(cmd.domain(), Domain::Pcb);
    // the scale of one is not sent
    let unit: Cmd = serde_json::from_value(serde_json::json!({"op": "pcb_item", "verb": "add_ref_image", "image": {"layer": "F.SilkS", "at": {"x": 0, "y": 0}, "data": data}})).unwrap();
    let Cmd::PcbItem(pcb_items::PcbItemCmd::AddRefImage { image }) = &unit else { panic!() };
    assert_eq!(image.scale_ppm, 1_000_000);
    let edit: Cmd = serde_json::from_value(serde_json::json!({"op": "pcb_item", "verb": "edit_ref_image", "id": "img_x", "layer": "F.SilkS", "at": {"x": 1_000, "y": 2000}, "scale_ppm": 750000})).unwrap();
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
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let mut p = PcbPoint::new("F.SilkS", p(Nm::from_raw_shim(5_000_000), Nm::from_raw_shim(6_000_000)));
    p.id = "caller_id".into();
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddPoint { point: p })).unwrap();
    let id = points(&b)[0].id.clone();
    assert!(id.starts_with("pnt_"), "{id}");
    assert_eq!((points(&b)[0].size_nm, points(&b)[0].bounds()), (Nm::from_raw_shim(1_000_000), [Nm::from_raw_shim(4_500_000), Nm::from_raw_shim(5_500_000), Nm::from_raw_shim(5_500_000), Nm::from_raw_shim(6_500_000)]), "a new point is 1 mm");

    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::EditPoint { id: id.clone(), layer: "F.Fab".into(), at: p_at(1_000_000, 2_000_000), size_nm: Nm::from_raw_shim(2_500_000) })).unwrap();
    let q = &points(&b)[0];
    assert_eq!((q.id.as_str(), q.layer.as_str(), q.at, q.size_nm), (id.as_str(), "F.Fab", p_at(1_000_000, 2_000_000), Nm::from_raw_shim(2_500_000)), "an edit keeps the id");

    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::DeletePoint { id: id.clone() })).unwrap();
    assert!(points(&b).is_empty());
    assert_eq!(b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::DeletePoint { id: id.clone() })).unwrap_err()[0].check, "ops_unknown_point");
    assert_eq!(b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::EditPoint { id, layer: "F.Fab".into(), at: p_at(0, 0), size_nm: Nm::from_raw_shim(1_000_000) })).unwrap_err()[0].check, "ops_unknown_point");
}

fn p_at(x: i64, y: i64) -> Point {
    p(Nm::from_raw_shim(x), Nm::from_raw_shim(y))
}

#[test]
fn a_point_that_cannot_be_drawn_is_refused() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let refused = |b: &mut Board<'_>, mutate: &dyn Fn(&mut PcbPoint)| {
        let mut q = PcbPoint::new("F.SilkS", p_at(0, 0));
        mutate(&mut q);
        let e = b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddPoint { point: q })).unwrap_err();
        assert!(points(b).is_empty(), "nothing was added");
        e[0].check.clone()
    };
    assert_eq!(refused(&mut b, &|q| q.size_nm = Nm::ZERO), "ops_bad_point");
    assert_eq!(refused(&mut b, &|q| q.size_nm = Nm::from_raw_shim(-5000)), "ops_bad_point");
    assert_eq!(refused(&mut b, &|q| q.layer = String::new()), "ops_bad_point");
    let mut off = model(&["F.Cu", "B.Cu"]);
    off.board.layer_setup.enabled.retain(|l| l != "F.Fab");
    let mut c = Board::new(design(), &off, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    assert_eq!(c.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddPoint { point: PcbPoint::new("F.Fab", p_at(0, 0)) })).unwrap_err()[0].check, "ops_layer_off");
}

#[test]
fn a_point_moves_turns_and_flips_with_the_verbs_that_take_any_item_but_cannot_be_locked() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let id = add_point(&mut b, "F.SilkS", p_at(10_000_000, 20_000_000));
    b.apply(&Cmd::MoveItems { ids: ids(&[&id]), dx: Nm::from_raw_shim(1_000_000), dy: Nm::from_raw_shim(-2_000_000) }).unwrap();
    assert_eq!(points(&b)[0].at, p_at(11_000_000, 18_000_000));
    b.apply(&Cmd::RotateItems { ids: ids(&[&id]), pivot: p_at(0, 0), angle_millideg: 90_000 }).unwrap();
    assert_eq!(points(&b)[0].at, p_at(-18_000_000, 11_000_000), "clockwise on the screen about the pivot");
    b.apply(&Cmd::FlipItems { ids: ids(&[&id]), pivot: p_at(0, 0), direction: FlipDirection::LeftRight }).unwrap();
    assert_eq!((points(&b)[0].layer.as_str(), points(&b)[0].at), ("B.SilkS", p_at(18_000_000, 11_000_000)));
    // the file has no lock for a point
    assert_eq!(b.apply(&Cmd::SetLocked { ids: ids(&[&id]), locked: true }).unwrap_err()[0].check, "ops_unknown_item");
}

#[test]
fn the_point_commands_have_one_wire_form() {
    let add: Cmd = serde_json::from_value(serde_json::json!({"op": "pcb_item", "verb": "add_point", "point": {"layer": "F.SilkS", "at": {"x": 1_000_000, "y": 2_000_000}}})).unwrap();
    let Cmd::PcbItem(pcb_items::PcbItemCmd::AddPoint { point }) = &add else { panic!("{add:?}") };
    assert_eq!((point.layer.as_str(), point.at, point.size_nm), ("F.SilkS", p_at(1_000_000, 2_000_000), Nm::from_raw_shim(1_000_000)), "the size left out is 1 mm");
    assert_eq!(add.domain(), Domain::Pcb);
    let edit: Cmd = serde_json::from_value(serde_json::json!({"op": "pcb_item", "verb": "edit_point", "id": "pnt_x", "layer": "F.Cu", "at": {"x": 1_000, "y": 2000}, "size_nm": 3_000_000})).unwrap();
    assert_eq!(edit.subjects(), vec!["pnt_x"]);
    let Cmd::PcbItem(c) = &edit else { panic!() };
    assert_eq!(c.describe(), "point edit pnt_x --layer F.Cu --at 0.001,0.002 --size 3.000");
    let back: Cmd = serde_json::from_value(serde_json::to_value(&add).unwrap()).unwrap();
    assert_eq!(back, add);
}

// ------------------------------------------------------------------------------------------------------------------ shape fills (hatch)

use eda_model::ir::Shape;
use eda_model::pcb_items::{HatchKind, ShapeFill};

fn style(b: &mut Board<'_>, shape_ids: &[&str], fill: ShapeFill) -> Result<(), Vec<CheckResult>> {
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::SetShapeStyle { ids: ids(shape_ids), fill: Some(fill), start_ending: None, end_ending: None }))
}

fn hatch_of(b: &Board<'_>, id: &str) -> Option<HatchKind> {
    drawings(b).board_items.hatch_of(id)
}

fn solid(b: &Board<'_>, id: &str) -> bool {
    drawings(b).shapes.iter().find(|s| s.id() == id).unwrap().is_filled()
}

#[test]
fn a_shape_is_hatched_unhatched_and_filled_by_one_verb_each_and_a_fill_replaces_the_one_before() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    style(&mut b, &["shp_rect"], ShapeFill::Hatch).unwrap();
    assert_eq!((hatch_of(&b, "shp_rect"), solid(&b, "shp_rect")), (Some(HatchKind::Hatch), false));
    style(&mut b, &["shp_rect"], ShapeFill::CrossHatch).unwrap();
    assert_eq!(hatch_of(&b, "shp_rect"), Some(HatchKind::CrossHatch));
    style(&mut b, &["shp_rect"], ShapeFill::Solid).unwrap();
    assert_eq!((hatch_of(&b, "shp_rect"), solid(&b, "shp_rect")), (None, true), "solid is not a hatch");
    style(&mut b, &["shp_rect"], ShapeFill::ReverseHatch).unwrap();
    assert_eq!((hatch_of(&b, "shp_rect"), solid(&b, "shp_rect")), (Some(HatchKind::ReverseHatch), false), "a hatch is not solid");
    style(&mut b, &["shp_rect"], ShapeFill::None).unwrap();
    assert_eq!((hatch_of(&b, "shp_rect"), solid(&b, "shp_rect")), (None, false));
    assert!(drawings(&b).board_items.shape_styles.is_empty(), "an empty style is not kept");
}

#[test]
fn several_shapes_are_given_a_fill_together_or_not_at_all() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    b.apply(&Cmd::AddShape { shape: Shape::Circle { id: String::new(), layer: "F.Fab".into(), stroke_width: Nm::from_raw_shim(100_000), filled: false, center: p(Nm::from_raw_shim(50_000_000), Nm::from_raw_shim(50_000_000)), end: p(Nm::from_raw_shim(55_000_000), Nm::from_raw_shim(50_000_000)) } }).unwrap();
    let circle = drawings(&b).shapes.iter().find(|s| matches!(s, Shape::Circle { .. })).unwrap().id().to_string();
    style(&mut b, &["shp_rect", &circle], ShapeFill::Hatch).unwrap();
    assert_eq!((hatch_of(&b, "shp_rect"), hatch_of(&b, &circle)), (Some(HatchKind::Hatch), Some(HatchKind::Hatch)));

    // A line, an arc and a curve have no fill; one of them among the ids refuses the lot, and nothing changes.
    let before = serde_json::to_value(b.design()).unwrap();
    assert_eq!(style(&mut b, &["shp_rect", "shp_seg"], ShapeFill::Solid).unwrap_err()[0].check, "ops_bad_shape_style");
    assert_eq!(style(&mut b, &["shp_rect", "shp_arc"], ShapeFill::None).unwrap_err()[0].check, "ops_bad_shape_style");
    assert_eq!(style(&mut b, &["shp_rect", "shp_nope"], ShapeFill::None).unwrap_err()[0].check, "ops_unknown_shape");
    assert_eq!(style(&mut b, &[], ShapeFill::None).unwrap_err()[0].check, "ops_bad_shape_style");
    assert_eq!(serde_json::to_value(b.design()).unwrap(), before, "a refused command changes nothing");
    // No fill given is no change.
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::SetShapeStyle { ids: ids(&["shp_rect"]), fill: None, start_ending: None, end_ending: None })).unwrap();
    assert_eq!(hatch_of(&b, "shp_rect"), Some(HatchKind::Hatch));
}

#[test]
fn a_hatched_shape_keeps_its_hatch_through_a_move_a_turn_and_a_flip_and_loses_it_when_it_is_deleted() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    style(&mut b, &["shp_rect"], ShapeFill::Hatch).unwrap();
    b.apply(&Cmd::MoveItems { ids: ids(&["shp_rect"]), dx: Nm::from_raw_shim(1_000_000), dy: Nm::from_raw_shim(2_000_000) }).unwrap();
    b.apply(&Cmd::RotateItems { ids: ids(&["shp_rect"]), pivot: p(Nm::ZERO, Nm::ZERO), angle_millideg: 90_000 }).unwrap();
    b.apply(&Cmd::FlipItems { ids: ids(&["shp_rect"]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    assert_eq!(hatch_of(&b, "shp_rect"), Some(HatchKind::Hatch), "the shape is the same shape, so it is as hatched as it was");
    b.apply(&Cmd::DeleteShape { id: "shp_rect".into() }).unwrap();
    assert!(drawings(&b).board_items.shape_styles.is_empty(), "the hatch goes with the shape");
}

#[test]
fn the_old_verbs_that_set_a_fill_put_a_solid_fill_in_place_of_a_hatch_and_leave_it_alone_otherwise() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    style(&mut b, &["shp_rect"], ShapeFill::Hatch).unwrap();
    // The layer and the line width change, the shape is not asked to be solid: it stays hatched.
    b.apply(&Cmd::EditShape { id: "shp_rect".into(), layer: "F.SilkS".into(), stroke_width: Nm::from_raw_shim(250_000), filled: false }).unwrap();
    assert_eq!(hatch_of(&b, "shp_rect"), Some(HatchKind::Hatch));
    b.apply(&Cmd::EditShape { id: "shp_rect".into(), layer: "F.SilkS".into(), stroke_width: Nm::from_raw_shim(250_000), filled: true }).unwrap();
    assert_eq!((hatch_of(&b, "shp_rect"), solid(&b, "shp_rect")), (None, true));

    style(&mut b, &["shp_rect"], ShapeFill::CrossHatch).unwrap();
    let mut replaced = drawings(&b).shapes.iter().find(|s| s.id() == "shp_rect").unwrap().clone();
    replaced.set_filled(false);
    b.apply(&Cmd::ReplaceShape { id: "shp_rect".into(), shape: replaced.clone() }).unwrap();
    assert_eq!(hatch_of(&b, "shp_rect"), Some(HatchKind::CrossHatch), "a replaced shape that is not solid is as hatched as it was");
    replaced.set_filled(true);
    b.apply(&Cmd::ReplaceShape { id: "shp_rect".into(), shape: replaced }).unwrap();
    assert_eq!(hatch_of(&b, "shp_rect"), None);
}

#[test]
fn a_shape_added_where_a_hatched_one_was_deleted_is_not_hatched() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    b.apply(&Cmd::AddShape { shape: Shape::Rect { id: String::new(), layer: "F.Fab".into(), stroke_width: Nm::from_raw_shim(100_000), filled: false, start: p(Nm::ZERO, Nm::ZERO), end: p(Nm::from_raw_shim(5_000_000), Nm::from_raw_shim(5_000_000)) } }).unwrap();
    let id = drawings(&b).shapes.iter().find(|s| s.points().first() == Some(&p(Nm::ZERO, Nm::ZERO))).unwrap().id().to_string();
    style(&mut b, &[&id], ShapeFill::Hatch).unwrap();
    b.apply(&Cmd::DeleteShape { id: id.clone() }).unwrap();
    b.apply(&Cmd::AddShape { shape: Shape::Rect { id: String::new(), layer: "F.Fab".into(), stroke_width: Nm::from_raw_shim(100_000), filled: false, start: p(Nm::ZERO, Nm::ZERO), end: p(Nm::from_raw_shim(5_000_000), Nm::from_raw_shim(5_000_000)) } }).unwrap();
    assert_eq!(hatch_of(&b, &id), None);
}

#[test]
fn the_shape_style_command_has_one_wire_form() {
    let c: Cmd = serde_json::from_value(serde_json::json!({"op": "pcb_item", "verb": "set_shape_style", "ids": ["shp_a", "shp_b"], "fill": "reverse_hatch"})).unwrap();
    let Cmd::PcbItem(pcb_items::PcbItemCmd::SetShapeStyle { ids: shape_ids, fill, .. }) = &c else { panic!("{c:?}") };
    assert_eq!((shape_ids.as_slice(), *fill), (&["shp_a".to_string(), "shp_b".to_string()][..], Some(ShapeFill::ReverseHatch)));
    assert_eq!(c.domain(), Domain::Pcb);
    assert_eq!(c.subjects(), vec!["shp_a", "shp_b"]);
    let Cmd::PcbItem(p) = &c else { panic!() };
    assert_eq!(p.describe(), "shape style shp_a,shp_b --fill reversehatch");
    let back: Cmd = serde_json::from_value(serde_json::to_value(&c).unwrap()).unwrap();
    assert_eq!(back, c);
    let none: Cmd = serde_json::from_value(serde_json::json!({"op": "pcb_item", "verb": "set_shape_style", "ids": ["shp_a"]})).unwrap();
    let Cmd::PcbItem(pcb_items::PcbItemCmd::SetShapeStyle { fill, .. }) = &none else { panic!() };
    assert_eq!(*fill, None, "the fill left out is no change");
}

#[test]
fn a_duplicate_a_paste_and_an_array_of_a_hatched_shape_are_hatched_the_same_way() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    style(&mut b, &["shp_rect"], ShapeFill::CrossHatch).unwrap();
    let hatched = |b: &Board<'_>| drawings(b).shapes.iter().filter(|s| hatch_of(b, s.id()).is_some()).count();

    // Duplicate: the copy has an id of its own and the hatch of the shape it was made from.
    b.apply(&Cmd::Duplicate { ids: ids(&["shp_rect"]) }).unwrap();
    assert_eq!(hatched(&b), 2);
    let copy = drawings(&b).shapes.iter().find(|s| s.id() != "shp_rect" && matches!(s, Shape::Rect { .. })).unwrap().id().to_string();
    assert_eq!(hatch_of(&b, &copy), Some(HatchKind::CrossHatch));

    // Copy and Paste: the KiCad text has the hatch (`(fill cross_hatch)`) and a paste reads it back.
    let text = eda_kicad::export_pcb_clipboard(b.design(), &m, &ids(&["shp_rect"]), p(Nm::ZERO, Nm::from_raw_shim(90_000_000))).unwrap();
    assert!(text.contains("(fill cross_hatch)"), "{text}");
    b.apply(&Cmd::PasteClipboard { text, at: p(Nm::from_raw_shim(40_000_000), Nm::from_raw_shim(40_000_000)) }).unwrap();
    assert_eq!(hatched(&b), 3);

    // Create Array: each copy of the set is hatched.
    b.apply(&Cmd::CreateArray {
        ids: ids(&["shp_rect"]),
        geometry: array::ArrayGeometry::Grid { nx: 3, ny: 1, dx: Nm::from_raw_shim(20_000_000), dy: Nm::ZERO, offset_x: Nm::ZERO, offset_y: Nm::ZERO, centred: false, stagger: 0, stagger_rows: true, horizontal_then_vertical: true, reverse_alternate: false },
        arrange: false,
        reannotate: true,
    })
    .unwrap();
    assert_eq!(hatched(&b), 5, "two more");
    assert!(drawings(&b).board_items.shape_styles.iter().all(|st| drawings(&b).shapes.iter().any(|s| s.id() == st.id)), "every style names a shape");
}

// ------------------------------------------------------------------------------------------------------------------------------- barcodes

use eda_model::barcode::{Barcode, BarcodeKind, EccLevel};

fn barcodes<'a>(b: &'a Board<'_>) -> &'a [Barcode] {
    &drawings(b).barcodes
}

fn qr(layer: &str, at: Point) -> Barcode {
    Barcode {
        id: String::new(),
        at,
        angle: 0,
        layer: layer.into(),
        width_nm: Nm::from_raw_shim(10_000_000),
        height_nm: Nm::from_raw_shim(10_000_000),
        text: "HELLO".into(),
        text_height_nm: Nm::from_raw_shim(1_500_000),
        kind: BarcodeKind::Qr,
        ecc: EccLevel::L,
        show_text: true,
        knockout: false,
        margin_x_nm: Nm::ZERO,
        margin_y_nm: Nm::ZERO,
    }
}

fn add_barcode(b: &mut Board<'_>, barcode: Barcode) -> String {
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddBarcode { barcode })).unwrap();
    barcodes(b).last().unwrap().id.clone()
}

#[test]
fn a_barcode_is_added_with_an_id_of_ours_edited_in_place_and_deleted() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let mut code = qr("F.SilkS", p(Nm::from_raw_shim(30_000_000), Nm::from_raw_shim(40_000_000)));
    code.id = "caller_id".into();
    let id = add_barcode(&mut b, code);
    assert!(id.starts_with("bar_"), "an id the caller sent is ignored: {id}");
    assert_eq!(barcodes(&b).len(), 1);

    let mut edited = barcodes(&b)[0].clone();
    edited.text = "WORLD".into();
    edited.kind = BarcodeKind::DataMatrix;
    edited.layer = "F.Fab".into();
    edited.knockout = true;
    edited.margin_x_nm = Nm::from_raw_shim(700_000);
    edited.angle = 90_000;
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::EditBarcode { id: id.clone(), barcode: edited })).unwrap();
    let c = &barcodes(&b)[0];
    assert_eq!((c.id.as_str(), c.text.as_str(), c.kind, c.layer.as_str(), c.knockout, c.margin_x_nm, c.angle), (id.as_str(), "WORLD", BarcodeKind::DataMatrix, "F.Fab", true, Nm::from_raw_shim(700_000), 90_000), "an edit keeps the id");

    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::DeleteBarcode { id: id.clone() })).unwrap();
    assert!(barcodes(&b).is_empty());
    assert_eq!(b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::DeleteBarcode { id: id.clone() })).unwrap_err()[0].check, "ops_unknown_barcode");
    assert_eq!(b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::EditBarcode { id, barcode: qr("F.SilkS", p(Nm::ZERO, Nm::ZERO)) })).unwrap_err()[0].check, "ops_unknown_barcode");
}

#[test]
fn a_barcode_that_cannot_be_drawn_is_refused_and_a_micro_qr_code_has_no_high_level() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let refused = |b: &mut Board<'_>, mutate: &dyn Fn(&mut Barcode)| {
        let mut c = qr("F.SilkS", p(Nm::ZERO, Nm::ZERO));
        mutate(&mut c);
        let e = b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddBarcode { barcode: c })).unwrap_err();
        assert!(barcodes(b).is_empty(), "nothing was added");
        e[0].check.clone()
    };
    assert_eq!(refused(&mut b, &|c| c.layer = String::new()), "ops_bad_barcode");
    assert_eq!(refused(&mut b, &|c| c.width_nm = Nm::ZERO), "ops_bad_barcode");
    assert_eq!(refused(&mut b, &|c| c.height_nm = Nm::from_raw_shim(-1)), "ops_bad_barcode");
    assert_eq!(refused(&mut b, &|c| c.text_height_nm = Nm::ZERO), "ops_bad_barcode");
    assert_eq!(refused(&mut b, &|c| c.angle = 360_000), "ops_bad_barcode");
    assert_eq!(refused(&mut b, &|c| c.margin_y_nm = Nm::from_raw_shim(-5000)), "ops_bad_barcode");
    let mut off = model(&["F.Cu", "B.Cu"]);
    off.board.layer_setup.enabled.retain(|l| l != "F.Fab");
    let mut c = Board::new(design(), &off, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    assert_eq!(c.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddBarcode { barcode: qr("F.Fab", p(Nm::ZERO, Nm::ZERO)) })).unwrap_err()[0].check, "ops_layer_off");

    // `SetErrorCorrection`: Micro QR takes Q for High; QR keeps it.
    let mut micro = qr("F.SilkS", p(Nm::ZERO, Nm::ZERO));
    micro.kind = BarcodeKind::MicroQr;
    micro.ecc = EccLevel::H;
    add_barcode(&mut b, micro);
    let mut full = qr("F.SilkS", p(Nm::from_raw_shim(5_000_000), Nm::ZERO));
    full.ecc = EccLevel::H;
    add_barcode(&mut b, full);
    let (m1, m2) = (&barcodes(&b)[0], &barcodes(&b)[1]);
    assert_eq!((m1.kind, m1.ecc, m2.kind, m2.ecc), (BarcodeKind::MicroQr, EccLevel::Q, BarcodeKind::Qr, EccLevel::H));
}

#[test]
fn a_barcode_moves_turns_flips_and_locks_with_the_verbs_that_take_any_item() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let id = add_barcode(&mut b, qr("F.SilkS", p(Nm::from_raw_shim(10_000_000), Nm::from_raw_shim(20_000_000))));
    b.apply(&Cmd::MoveItems { ids: ids(&[&id]), dx: Nm::from_raw_shim(1_000_000), dy: Nm::from_raw_shim(-2_000_000) }).unwrap();
    assert_eq!(barcodes(&b)[0].at, p(Nm::from_raw_shim(11_000_000), Nm::from_raw_shim(18_000_000)));
    // `PCB_BARCODE::Rotate`: the centre turns about the pivot, the angle (counter-clockwise in the file) grows by the turn; clockwise 90 here
    b.apply(&Cmd::RotateItems { ids: ids(&[&id]), pivot: p(Nm::ZERO, Nm::ZERO), angle_millideg: 90_000 }).unwrap();
    assert_eq!((barcodes(&b)[0].at, barcodes(&b)[0].angle), (p(Nm::from_raw_shim(-18_000_000), Nm::from_raw_shim(11_000_000)), 270_000), "clockwise on the screen about the pivot");
    // `Flip`: left-right mirrors the centre and puts it on the back layer; top-bottom turns it half a turn as well
    b.apply(&Cmd::FlipItems { ids: ids(&[&id]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    assert_eq!((barcodes(&b)[0].layer.as_str(), barcodes(&b)[0].at, barcodes(&b)[0].angle), ("B.SilkS", p(Nm::from_raw_shim(18_000_000), Nm::from_raw_shim(11_000_000)), 270_000));
    b.apply(&Cmd::FlipItems { ids: ids(&[&id]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::TopBottom }).unwrap();
    assert_eq!((barcodes(&b)[0].layer.as_str(), barcodes(&b)[0].at, barcodes(&b)[0].angle), ("F.SilkS", p(Nm::from_raw_shim(18_000_000), Nm::from_raw_shim(-11_000_000)), 90_000));
    // a barcode has a lock in the file (`(locked yes)`)
    b.apply(&Cmd::SetLocked { ids: ids(&[&id]), locked: true }).unwrap();
    assert!(drawings(&b).locked_ids.contains(&id));
}

#[test]
fn the_barcode_commands_have_one_wire_form_and_describe_themselves() {
    let add: Cmd = serde_json::from_value(serde_json::json!({
        "op": "pcb_item", "verb": "add_barcode",
        "barcode": {"layer": "F.SilkS", "at": {"x": 1_000_000, "y": 2_000_000}, "width_nm": 12_000_000, "height_nm": 12_000_000, "text": "A1", "text_height_nm": 1_500_000, "kind": "code128"}
    }))
    .unwrap();
    let Cmd::PcbItem(pcb_items::PcbItemCmd::AddBarcode { barcode }) = &add else { panic!("{add:?}") };
    assert_eq!((barcode.kind, barcode.ecc, barcode.show_text, barcode.knockout, barcode.angle), (BarcodeKind::Code128, EccLevel::L, true, false, 0), "what is left out is KiCad's default");
    assert_eq!(add.domain(), Domain::Pcb);
    let edit: Cmd = serde_json::from_value(serde_json::json!({
        "op": "pcb_item", "verb": "edit_barcode", "id": "bar_x",
        "barcode": {"layer": "F.Cu", "at": {"x": 1_000, "y": 2000}, "width_nm": 5_000_000, "height_nm": 6_000_000, "text": "T", "text_height_nm": 1_000_000, "kind": "qr", "ecc": "M"}
    }))
    .unwrap();
    assert_eq!(edit.subjects(), vec!["bar_x"]);
    let Cmd::PcbItem(c) = &edit else { panic!() };
    assert_eq!(c.describe(), "barcode edit bar_x --layer F.Cu --at 0.001,0.002 --size 5.000x6.000 --kind qr --text \"T\"");
    let back: Cmd = serde_json::from_value(serde_json::to_value(&add).unwrap()).unwrap();
    assert_eq!(back, add);
}

// ------------------------------------------------------------------------------------------------------------------ line endings (KiCad 10)

use eda_model::line_ending::{LineEnding, LineEndingStyle, LineEndings};

fn endings(b: &mut Board<'_>, shape_ids: &[&str], start: Option<LineEnding>, end: Option<LineEnding>) -> Result<(), Vec<CheckResult>> {
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::SetShapeStyle { ids: ids(shape_ids), fill: None, start_ending: start, end_ending: end }))
}

fn endings_of(b: &Board<'_>, id: &str) -> LineEndings {
    drawings(b).board_items.endings_of(id)
}

#[test]
fn a_line_an_arc_and_a_curve_get_line_endings_and_a_closed_shape_none() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let arrow = LineEnding { style: LineEndingStyle::Arrow, length_nm: Nm::from_raw_shim(3_000_000), width_nm: Nm::from_raw_shim(2_000_000), ..Default::default() };
    endings(&mut b, &["shp_seg", "shp_arc"], Some(arrow), None).unwrap();
    assert_eq!(endings_of(&b, "shp_seg"), LineEndings { start: arrow, end: LineEnding::default() });
    assert_eq!(endings_of(&b, "shp_arc").start, arrow);
    // The end alone, and the start as it was.
    endings(&mut b, &["shp_seg"], None, Some(LineEnding::new(LineEndingStyle::Circle))).unwrap();
    assert_eq!(endings_of(&b, "shp_seg"), LineEndings { start: arrow, end: LineEnding::new(LineEndingStyle::Circle) });
    // A size below 0 is 0 (`std::max( 0, .. )`).
    endings(&mut b, &["shp_seg"], Some(LineEnding { style: LineEndingStyle::Square, length_nm: Nm::from_raw_shim(-5000), width_nm: Nm::from_raw_shim(1_000_000), stroke_width_nm: Nm::from_raw_shim(-1), ..Default::default() }), None).unwrap();
    assert_eq!(endings_of(&b, "shp_seg").start, LineEnding { style: LineEndingStyle::Square, width_nm: Nm::from_raw_shim(1_000_000), ..Default::default() });

    let before = serde_json::to_value(b.design()).unwrap();
    assert_eq!(endings(&mut b, &["shp_seg", "shp_rect"], Some(arrow), None).unwrap_err()[0].check, "ops_bad_shape_style", "a rectangle is closed");
    assert_eq!(serde_json::to_value(b.design()).unwrap(), before, "a refused command changes nothing");

    // None takes them away, and a style that says nothing is not kept.
    endings(&mut b, &["shp_seg", "shp_arc"], Some(LineEnding::default()), Some(LineEnding::default())).unwrap();
    assert!(drawings(&b).board_items.shape_styles.is_empty());
}

#[test]
fn a_shape_keeps_its_endings_through_a_move_a_turn_a_flip_and_a_duplicate_and_loses_them_when_it_is_deleted() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let open = LineEnding::new(LineEndingStyle::ArrowOpen);
    endings(&mut b, &["shp_seg"], Some(open), Some(open)).unwrap();
    b.apply(&Cmd::MoveItems { ids: ids(&["shp_seg"]), dx: Nm::from_raw_shim(1_000_000), dy: Nm::from_raw_shim(2_000_000) }).unwrap();
    b.apply(&Cmd::RotateItems { ids: ids(&["shp_seg"]), pivot: p(Nm::ZERO, Nm::ZERO), angle_millideg: 90_000 }).unwrap();
    b.apply(&Cmd::FlipItems { ids: ids(&["shp_seg"]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    assert_eq!(endings_of(&b, "shp_seg").end, open);
    b.apply(&Cmd::Duplicate { ids: ids(&["shp_seg"]) }).unwrap();
    let with = drawings(&b).shapes.iter().filter(|s| !endings_of(&b, s.id()).is_none()).count();
    assert_eq!(with, 2, "the copy has the endings too");
    b.apply(&Cmd::DeleteShape { id: "shp_seg".into() }).unwrap();
    assert!(endings_of(&b, "shp_seg").is_none());
}

#[test]
fn the_endings_have_one_wire_form() {
    let c: Cmd = serde_json::from_value(serde_json::json!({"op": "pcb_item", "verb": "set_shape_style", "ids": ["shp_a"], "start_ending": {"style": "arrow_open", "length_nm": 3_000_000, "width_nm": 3_000_000, "stroke_width_nm": 600_000}})).unwrap();
    let Cmd::PcbItem(pcb_items::PcbItemCmd::SetShapeStyle { start_ending, end_ending, fill, .. }) = &c else { panic!("{c:?}") };
    assert_eq!((*start_ending, *end_ending, *fill), (Some(LineEnding { style: LineEndingStyle::ArrowOpen, length_nm: Nm::from_raw_shim(3_000_000), width_nm: Nm::from_raw_shim(3_000_000), stroke_width_nm: Nm::from_raw_shim(600_000), ..Default::default() }), None, None));
    let Cmd::PcbItem(p) = &c else { panic!() };
    assert_eq!(p.describe(), "shape style shp_a --fill unchanged --start-shape arrow_open --start-size 3.000x3.000 --start-stroke 0.600");
    let back: Cmd = serde_json::from_value(serde_json::to_value(&c).unwrap()).unwrap();
    assert_eq!(back, c);
}

// ------------------------------------------------------------------------------------------------------------------ grid items (KiCad 10)

use eda_model::grid_item::{GridItem, GridKind};

fn grids<'a>(b: &'a Board<'_>) -> &'a [GridItem] {
    &drawings(b).board_items.grid_items
}

#[test]
fn a_grid_item_is_placed_edited_turned_flipped_locked_and_deleted() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let g = GridItem { id: "caller".into(), extent: p(Nm::from_raw_shim(-3_000_000), Nm::from_raw_shim(2_000_000)), priority: 0, ..GridItem::new(p(Nm::from_raw_shim(10_000_000), Nm::ZERO)) };
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddGridItem { grid_item: g })).unwrap();
    let id = grids(&b)[0].id.clone();
    assert!(id.starts_with("grd_"), "{id}");
    assert_eq!((grids(&b)[0].extent, grids(&b)[0].priority), (p(Nm::from_raw_shim(3_000_000), Nm::from_raw_shim(2_000_000)), 1), "the setters' ranges");

    let edited = GridItem { kind: GridKind::Polar, phi_extent_deg: 90.0, angle: -30.0, ..grids(&b)[0].clone() };
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::EditGridItem { id: id.clone(), grid_item: edited })).unwrap();
    assert_eq!((grids(&b)[0].kind, grids(&b)[0].angle, grids(&b)[0].id.as_str()), (GridKind::Polar, 330.0, id.as_str()));

    // A clockwise quarter turn about the origin takes 90 from KiCad's counter-clockwise orientation.
    b.apply(&Cmd::RotateItems { ids: ids(&[&id]), pivot: p(Nm::ZERO, Nm::ZERO), angle_millideg: 90_000 }).unwrap();
    assert_eq!((grids(&b)[0].at, grids(&b)[0].angle), (p(Nm::ZERO, Nm::from_raw_shim(10_000_000)), 240.0));
    b.apply(&Cmd::FlipItems { ids: ids(&[&id]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    assert_eq!(grids(&b)[0].angle, 300.0, "180 - 240");
    b.apply(&Cmd::MoveItems { ids: ids(&[&id]), dx: Nm::from_raw_shim(1_000_000), dy: Nm::ZERO }).unwrap();
    assert_eq!(grids(&b)[0].at, p(Nm::from_raw_shim(1_000_000), Nm::from_raw_shim(10_000_000)));
    b.apply(&Cmd::SetLocked { ids: ids(&[&id]), locked: true }).unwrap();
    assert!(drawings(&b).locked_ids.contains(&id));

    assert_eq!(b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::DeleteGridItem { id: "grd_nope".into() })).unwrap_err()[0].check, "ops_unknown_grid_item");
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::DeleteGridItem { id })).unwrap();
    assert!(grids(&b).is_empty());
}

#[test]
fn the_grid_item_commands_have_one_wire_form() {
    let add: Cmd = serde_json::from_value(serde_json::json!({"op": "pcb_item", "verb": "add_grid_item", "grid_item": {
        "kind": "polar", "at": {"x": 1_000_000, "y": 2_000_000}, "extent": {"x": 5_000_000, "y": 0}, "spacing": {"x": 500_000, "y": 500_000}, "angle": 45.5,
        "phi_extent_deg": 180.0, "phi_spacing_deg": 15.0, "priority": 2, "tick_interval": 4000, "affects": {"cursor": true, "routing": false, "placement": true}}})).unwrap();
    let Cmd::PcbItem(pcb_items::PcbItemCmd::AddGridItem { grid_item }) = &add else { panic!("{add:?}") };
    assert_eq!((grid_item.kind, grid_item.angle, grid_item.phi_extent_deg, grid_item.affects.routing), (GridKind::Polar, 45.5, 180.0, false));
    let Cmd::PcbItem(c) = &add else { panic!() };
    assert_eq!(c.describe(), "grid-item add --polar --at 1.000,2.000 --extent 5.000x0.000 --spacing 0.500x0.500");
    let back: Cmd = serde_json::from_value(serde_json::to_value(&add).unwrap()).unwrap();
    assert_eq!(back, add);
}

// ------------------------------------------------------------------------------------------------------------------ via stitching (KiCad 10)

use eda_model::via_stitch::{StitchLayout, ViaStitch, ViaTemplate};
use pcb_items::PlannedVia;

fn stitch() -> ViaStitch {
    let mut v = ViaStitch::new(vec![p(Nm::ZERO, Nm::ZERO), p(Nm::from_raw_shim(10_000_000), Nm::ZERO), p(Nm::from_raw_shim(10_000_000), Nm::from_raw_shim(10_000_000)), p(Nm::ZERO, Nm::from_raw_shim(10_000_000))]);
    v.net = "GND".into();
    v.pitch_nm = Nm::from_raw_shim(2_000_000);
    v.via = ViaTemplate { size_nm: Nm::from_raw_shim(600_000), drill_nm: Nm::from_raw_shim(300_000), from_layer: "F.Cu".into(), to_layer: "B.Cu".into(), text: String::new() };
    v
}

fn planned(cells: &[(i64, i64)]) -> Vec<PlannedVia> {
    cells.iter().map(|&(c, r)| PlannedVia { cell: (c, r), at: p(Nm::from_raw_shim(c * 2_000_000), Nm::from_raw_shim(r * 2_000_000)) }).collect()
}

fn set_stitch(b: &mut Board<'_>, id: Option<&str>, v: ViaStitch, cells: &[(i64, i64)]) -> Result<(), Vec<CheckResult>> {
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::SetViaStitch { id: id.map(str::to_string), stitch: v, vias: Some(planned(cells)) }))
}

fn stitches<'a>(b: &'a Board<'_>) -> &'a [ViaStitch] {
    &drawings(b).board_items.via_stitches
}

fn member_vias(b: &Board<'_>) -> Vec<(String, eda_model::ir::Point)> {
    let ids = &stitches(b)[0].member_ids;
    let mut out: Vec<_> = b.design().routing.as_ref().unwrap().vias.iter().filter(|v| ids.contains(&v.id)).map(|v| (v.id.clone(), v.at)).collect();
    out.sort_by_key(|(_, at)| (at.x, at.y));
    out
}

#[test]
fn a_stitching_makes_its_vias_and_churns_as_few_as_update_does() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let before = b.design().routing.as_ref().unwrap().vias.len();
    set_stitch(&mut b, None, stitch(), &[(1, 1), (2, 1), (3, 1)]).unwrap();
    let id = stitches(&b)[0].id.clone();
    assert!(id.starts_with("vst_"), "{id}");
    assert_eq!(b.design().routing.as_ref().unwrap().vias.len(), before + 3);
    let first = member_vias(&b);
    assert_eq!(first.iter().map(|(_, at)| *at).collect::<Vec<_>>(), vec![p(Nm::from_raw_shim(2_000_000), Nm::from_raw_shim(2_000_000)), p(Nm::from_raw_shim(4_000_000), Nm::from_raw_shim(2_000_000)), p(Nm::from_raw_shim(6_000_000), Nm::from_raw_shim(2_000_000))]);
    // "Vias must be marked free".
    assert!(drawings(&b).via_extras.iter().any(|x| x.id == first[0].0 && x.tokens == ["(free yes)"]));

    // The same cells again: nothing changes, the ids stay (`UpdateDeltaStability`).
    let cur = stitches(&b)[0].clone();
    set_stitch(&mut b, Some(&id), cur, &[(1, 1), (2, 1), (3, 1)]).unwrap();
    assert_eq!(member_vias(&b), first);
    // One cell gone and one new: the two kept keep their ids.
    let cur = stitches(&b)[0].clone();
    set_stitch(&mut b, Some(&id), cur, &[(1, 1), (2, 1), (4, 1)]).unwrap();
    let after = member_vias(&b);
    assert_eq!((after[0].0.as_str(), after[1].0.as_str()), (first[0].0.as_str(), first[1].0.as_str()));
    assert!(!after.iter().any(|(vid, _)| *vid == first[2].0), "the via of the cell that went is gone");
    assert_eq!(after[2].1, p(Nm::from_raw_shim(8_000_000), Nm::from_raw_shim(2_000_000)));
    // A new stitch net keeps the vias (`SetNetCode` gives the members the net), ids and all.
    let mut vcc = stitches(&b)[0].clone();
    vcc.net = "VCC".into();
    set_stitch(&mut b, Some(&id), vcc, &[(1, 1), (2, 1), (4, 1)]).unwrap();
    assert_eq!(member_vias(&b), after, "the same vias");
    assert!(b.design().routing.as_ref().unwrap().vias.iter().filter(|v| stitches(&b)[0].member_ids.contains(&v.id)).all(|v| v.net == "VCC"));
    let mut gnd = stitches(&b)[0].clone();
    gnd.net = "GND".into();
    set_stitch(&mut b, Some(&id), gnd, &[(1, 1), (2, 1), (4, 1)]).unwrap();
    // A new via size makes them again.
    let mut bigger = stitches(&b)[0].clone();
    bigger.via.size_nm = Nm::from_raw_shim(800_000);
    set_stitch(&mut b, Some(&id), bigger, &[(1, 1), (2, 1), (4, 1)]).unwrap();
    assert!(member_vias(&b).iter().all(|(vid, _)| !after.iter().any(|(o, _)| o == vid)));
    assert!(b.design().routing.as_ref().unwrap().vias.iter().filter(|v| stitches(&b)[0].member_ids.contains(&v.id)).all(|v| v.diameter == Nm::from_raw_shim(800_000)));

    // `Remove`: the stitching and its vias.
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::DeleteViaStitch { id })).unwrap();
    assert!(stitches(&b).is_empty());
    assert_eq!(b.design().routing.as_ref().unwrap().vias.len(), before);
}

#[test]
fn a_nudged_via_is_put_back_on_its_cell_and_a_stitching_without_a_plan_is_refused() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    set_stitch(&mut b, None, stitch(), &[(1, 1)]).unwrap();
    let id = stitches(&b)[0].id.clone();
    let vid = member_vias(&b)[0].0.clone();
    b.apply(&Cmd::MoveVia { id: vid.clone(), x: Nm::from_raw_shim(2_300_000), y: Nm::from_raw_shim(2_000_000) }).unwrap();
    // The plan has the cell at its place: the via is "just nudged" back, not made again.
    let cur = stitches(&b)[0].clone();
    set_stitch(&mut b, Some(&id), cur, &[(1, 1)]).unwrap();
    assert_eq!(member_vias(&b), vec![(vid, p(Nm::from_raw_shim(2_000_000), Nm::from_raw_shim(2_000_000)))]);
    let err = b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::SetViaStitch { id: Some(id.clone()), stitch: stitch(), vias: None })).unwrap_err();
    assert_eq!(err[0].check, "ops_via_stitch_unplanned");
    let mut odd = stitch();
    odd.via.drill_nm = Nm::from_raw_shim(700_000);
    assert_eq!(set_stitch(&mut b, Some(&id), odd, &[]).unwrap_err()[0].check, "ops_bad_via_stitch");
    let mut poisson = stitch();
    poisson.layout = StitchLayout::Poisson;
    assert!(set_stitch(&mut b, Some(&id), poisson, &[]).is_ok());
}

/// A via stitching is a group: Rotate and Flip take its vias along (`PCB_GROUP::Rotate`, `Flip`) and a Flip takes it to the other side (`PCB_GENERATOR::Flip`'s
/// `SetLayer( GetFlippedLayer( GetLayer() ) )`); a Move moves them too, before the board directory regenerates the stitching.
#[test]
fn a_stitching_turns_and_flips_with_its_vias() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    set_stitch(&mut b, None, stitch(), &[(1, 1)]).unwrap();
    let id = stitches(&b)[0].id.clone();
    b.apply(&Cmd::RotateItems { ids: ids(&[&id]), pivot: p(Nm::ZERO, Nm::ZERO), angle_millideg: 90_000 }).unwrap();
    let at = member_vias(&b)[0].1;
    assert_eq!((at.x.abs(), at.y.abs()), (Nm::from_raw_shim(2_000_000), Nm::from_raw_shim(2_000_000)), "turned about the origin with the outline");
    assert_ne!(at, p(Nm::from_raw_shim(2_000_000), Nm::from_raw_shim(2_000_000)));
    b.apply(&Cmd::FlipItems { ids: ids(&[&id]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    assert_eq!(stitches(&b)[0].layer, "B.Cu");
    assert_eq!(member_vias(&b)[0].1.x, -at.x, "mirrored with it");
    b.apply(&Cmd::MoveItems { ids: ids(&[&id]), dx: Nm::from_raw_shim(500_000), dy: Nm::ZERO }).unwrap();
    assert_eq!(member_vias(&b)[0].1.x, -at.x + Nm::from_raw_shim(500_000));
}

#[test]
fn the_via_stitching_commands_have_one_wire_form() {
    let c: Cmd = serde_json::from_value(serde_json::json!({"op": "pcb_item", "verb": "set_via_stitch", "stitch": {
        "outline": [{"x": 0, "y": 0}, {"x": 1_000_000, "y": 0}, {"x": 1_000_000, "y": 1_000_000}], "net": "GND", "pitch_nm": 1_000_000, "layout": "staggered",
        "via": {"size_nm": 600_000, "drill_nm": 300_000, "from_layer": "F.Cu", "to_layer": "B.Cu"}}})).unwrap();
    let Cmd::PcbItem(pcb_items::PcbItemCmd::SetViaStitch { id, stitch, vias }) = &c else { panic!("{c:?}") };
    assert_eq!((id.as_deref(), stitch.layout, vias.is_none()), (None, StitchLayout::Staggered, true));
    let back: Cmd = serde_json::from_value(serde_json::to_value(&c).unwrap()).unwrap();
    assert_eq!(back, c);
}

// ------------------------------------------------------------------------------------------------------------------ drill maps (KiCad 10)

#[test]
fn a_drill_map_is_placed_once_a_layer_and_moves_its_marks_not_its_holes() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let place = |b: &mut Board<'_>, layer: &str| b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::PlaceDrillMap { layer: layer.into() }));
    place(&mut b, "F.Fab").unwrap();
    // "Two maps on one layer would overdraw each other's marks"; copper is no documentation layer.
    assert_eq!(place(&mut b, "F.Fab").unwrap_err()[0].hint.as_deref(), Some("Every documentation layer already has a drill map."));
    assert!(place(&mut b, "F.Cu").is_err());
    let map = drawings(&b).board_items.drill_maps[0].clone();
    assert!(map.id.starts_with("dmap_"), "{}", map.id);
    assert_eq!((map.size_nm, map.span.is_none(), map.outline_slots, map.guide_cross), (Nm::from_raw_shim(1_500_000), true, true, false), "PCB_DRILL_MAP's defaults");
    // Move slides the marks (`m_offset`); a turn has nothing to turn; a flip takes the map to the other side's layer.
    b.apply(&Cmd::MoveItems { ids: ids(&[&map.id]), dx: Nm::from_raw_shim(50_000_000), dy: Nm::ZERO }).unwrap();
    b.apply(&Cmd::RotateItems { ids: ids(&[&map.id]), pivot: p(Nm::ZERO, Nm::ZERO), angle_millideg: 90_000 }).unwrap();
    b.apply(&Cmd::FlipItems { ids: ids(&[&map.id]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    let moved = drawings(&b).board_items.drill_maps[0].clone();
    assert_eq!((moved.offset, moved.layer.as_str()), (p(Nm::from_raw_shim(50_000_000), Nm::ZERO), "B.Fab"));
    let mut edited = moved.clone();
    edited.guide_cross = true;
    edited.size_nm = Nm::ZERO;
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::EditDrillMap { id: map.id.clone(), map: edited })).unwrap();
    assert_eq!((drawings(&b).board_items.drill_maps[0].guide_cross, drawings(&b).board_items.drill_maps[0].size_nm), (true, Nm::from_raw_shim(1)), "`SetSymbolSize`: at least one");
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::DeleteDrillMap { id: map.id })).unwrap();
    assert!(drawings(&b).board_items.drill_maps.is_empty());
}

#[test]
fn a_drill_chart_is_refused_off_the_documentation_layers_and_lists_the_holes() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let err = b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::PlaceDrillChart { layer: "F.SilkS".into(), at: p(Nm::ZERO, Nm::ZERO) })).unwrap_err();
    assert_eq!(err[0].hint.as_deref(), Some("No documentation layer is enabled to hold a drill chart."));
    b.apply(&Cmd::AddVia { net: "GND".into(), x: Nm::from_raw_shim(10_000_000), y: Nm::from_raw_shim(10_000_000), drill: Nm::from_raw_shim(300_000), diameter: Nm::from_raw_shim(600_000), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::PlaceDrillChart { layer: "Dwgs.User".into(), at: p(Nm::from_raw_shim(5_000_000), Nm::from_raw_shim(6_000_000)) })).unwrap();
    let dr = drawings(&b);
    let chart = &dr.board_items.drill_charts[0];
    let t = dr.tables.iter().find(|t| t.id == chart.table_id).unwrap();
    assert_eq!((t.col_count(), t.at), (6, p(Nm::from_raw_shim(5_000_000), Nm::from_raw_shim(6_000_000))));
    let texts: Vec<&str> = t.cells.iter().map(|c| c.text.as_str()).collect();
    assert!(texts.contains(&"0.300") && texts.contains(&"F.Cu / B.Cu"), "{texts:?}");
    assert_eq!(chart.row_shapes.get(&1), Some(&0), "the first shape");
    assert!(dr.board_items.drill_profile.as_ref().is_some_and(|p| p.assignments.len() == 1), "the new mark is the board's");
    // Nothing changed: nothing is rebuilt. Another via of the size: the chart counts it.
    let ops_before: usize = t.cells[6 + 4].text.parse().unwrap();
    assert!(!b.refresh_drill_charts());
    b.apply(&Cmd::AddVia { net: "GND".into(), x: Nm::from_raw_shim(20_000_000), y: Nm::from_raw_shim(10_000_000), drill: Nm::from_raw_shim(300_000), diameter: Nm::from_raw_shim(600_000), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    assert!(b.refresh_drill_charts());
    let dr = drawings(&b);
    let t = dr.tables.iter().find(|t| t.id == dr.board_items.drill_charts[0].table_id).unwrap();
    assert_eq!(t.cells[6 + 4].text, (ops_before + 1).to_string(), "the OPS of the 0.3 mm row");
}

/// `PCB_DRILL_CHART::IsStale` is the drill generation, not the cells: a chart that was read (no generation) is rebuilt by the first command, a cell edited by hand keeps its
/// text until the holes change, and the Properties panel's edit rebuilds the chart whatever its generation (`PCB_PROPERTIES_PANEL::valueChanged`'s `RebuildCells`).
#[test]
fn a_drill_chart_is_stale_by_the_drill_generation_not_by_its_text() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    b.apply(&Cmd::AddVia { net: "GND".into(), x: Nm::from_raw_shim(10_000_000), y: Nm::from_raw_shim(10_000_000), drill: Nm::from_raw_shim(300_000), diameter: Nm::from_raw_shim(600_000), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::PlaceDrillChart { layer: "Dwgs.User".into(), at: p(Nm::from_raw_shim(5_000_000), Nm::from_raw_shim(6_000_000)) })).unwrap();
    let id = drawings(&b).board_items.drill_charts[0].table_id.clone();
    let cell = |b: &Board, i: usize| drawings(b).tables.iter().find(|t| t.id == id).unwrap().cells[i].text.clone();
    b.apply(&Cmd::Table(crate::TableCmd::EditCells { id: id.clone(), cells: vec![(1, 1)], edit: eda_model::table_edit::CellEdit { text: Some("by hand".into()), ..Default::default() } })).unwrap();
    assert!(!b.refresh_drill_charts(), "the holes are as they were: the chart is not stale");
    assert_eq!(cell(&b, 6 + 1), "by hand");
    // A chart as a file has it: no generation, stale.
    let mut read = b.design().clone();
    read.drawings.as_mut().unwrap().board_items.drill_charts[0].built_generation = None;
    let mut b = Board::new(read, &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    assert!(b.refresh_drill_charts(), "a chart that was read is rebuilt");
    assert_eq!(cell(&b, 6 + 1), "0.300");
    b.apply(&Cmd::Table(crate::TableCmd::EditCells { id: id.clone(), cells: vec![(1, 1)], edit: eda_model::table_edit::CellEdit { text: Some("by hand".into()), ..Default::default() } })).unwrap();
    // The via moved: a via changed, the generation with it.
    let via = b.design().routing.as_ref().unwrap().vias[0].id.clone();
    b.apply(&Cmd::MoveItems { ids: ids(&[&via]), dx: Nm::from_raw_shim(1_000_000), dy: Nm::ZERO }).unwrap();
    assert!(b.refresh_drill_charts(), "a via changed");
    assert_eq!(cell(&b, 6 + 1), "0.300");
    b.apply(&Cmd::Table(crate::TableCmd::EditCells { id: id.clone(), cells: vec![(1, 1)], edit: eda_model::table_edit::CellEdit { text: Some("by hand".into()), ..Default::default() } })).unwrap();
    let c = drawings(&b).board_items.drill_charts[0].clone();
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::SetDrillChart { id: id.clone(), units: c.units, precision: c.precision, show_totals: c.show_totals, filter: c.filter })).unwrap();
    assert_eq!(cell(&b, 6 + 1), "0.300", "the Properties panel rebuilds its chart");
}

/// p6 F17: `EDIT_TOOL::Duplicate` duplicates a barcode and a point like any item (`orig_item->Duplicate( true, &commit )`), and
/// `BOARD_ITEM::Duplicate` is a `Clone()`: a locked item's copy is locked.
#[test]
fn duplicate_copies_barcodes_and_points_and_keeps_a_lock() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let bar = add_barcode(&mut b, qr("F.SilkS", p(Nm::from_raw_shim(10_000_000), Nm::from_raw_shim(20_000_000))));
    b.apply(&Cmd::PcbItem(pcb_items::PcbItemCmd::AddPoint { point: PcbPoint::new("F.SilkS", p(Nm::from_raw_shim(5_000_000), Nm::from_raw_shim(5_000_000))) })).unwrap();
    let pnt = drawings(&b).board_items.points.last().unwrap().id.clone();
    b.apply(&Cmd::SetLocked { ids: ids(&[&bar]), locked: true }).unwrap();
    b.apply(&Cmd::Duplicate { ids: ids(&[&bar, &pnt]) }).unwrap();
    let dr = drawings(&b);
    assert_eq!(dr.barcodes.len(), 2);
    assert_eq!(dr.board_items.points.len(), 2);
    let copy = dr.barcodes.iter().find(|x| x.id != bar).unwrap();
    assert_eq!((copy.at, copy.text.clone()), (dr.barcodes[0].at, dr.barcodes[0].text.clone()), "an exact copy in place");
    assert!(dr.locked_ids.contains(&copy.id), "the copy of a locked barcode is locked");
    let pcopy = dr.board_items.points.iter().find(|x| x.id != pnt).unwrap();
    assert!(pcopy.id.starts_with("pnt_") && !dr.locked_ids.contains(&pcopy.id));
    // a lone barcode alone is duplicable
    b.apply(&Cmd::Duplicate { ids: ids(&[&bar]) }).unwrap();
    assert_eq!(drawings(&b).barcodes.len(), 3);
}
