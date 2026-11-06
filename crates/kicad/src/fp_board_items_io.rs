//! A board footprint's text boxes, reference images, points and barcodes (UI-LEFT P17): `fp_text_box`, `(image ..)`, `(point ..)` and `(barcode ..)` inside a
//! `(footprint ..)` of a `.kicad_pcb`, read into [`eda_model::ir::FootprintExtra::items`] and written back, as `PCB_IO_KICAD_SEXPR_PARSER::parseFOOTPRINT_unchecked`
//! and `PCB_IO_KICAD_SEXPR::format( const FOOTPRINT* )` do at KiCad 8303b2ad. Before this they were kept as the text of their forms (`FootprintExtra::tokens`), which
//! put an image, a point or a barcode back where it was read whatever became of the footprint (their file positions are the board's) and a text box where the
//! footprint's frame had it, never flipped.
//!
//! # Frames
//!
//! The file does not write the four kinds in one frame:
//! - a text box is in the footprint's own frame (`format( PCB_TEXTBOX )`: `formatInternalUnits( aTextBox->GetStart(), parentFP )`, `angle -= parentFP->GetOrientation()`;
//!   `parsePCB_TEXTBOX` ends with `Rotate( { 0, 0 }, parentFP->GetOrientation() ); Move( parentFP->GetPosition() )`) -- the frame of the footprint as `FOOTPRINT::Flip`
//!   left it, so a footprint on the back has it flipped top to bottom;
//! - an image, a point and a barcode are where they are on the board (`format( PCB_REFERENCE_IMAGE )`, `format( PCB_POINT )` and `format( PCB_BARCODE )` write
//!   `GetPosition()` and the barcode's own `GetAngle()` with no `parentFP`, and their parsers set them as read).
//!
//! The model holds all four in the frame of the footprint's pads ([`eda_model::fp_items_place`]): a text box of a footprint on the back is flipped back, as its art is
//! (`crate::fp_art_io`, the library's frame, `FootprintExtra::library_frame`); the others are taken off the board by the footprint's pose. An image's pixels are kept as
//! the file has them -- as the board shows them, turned and mirrored with the footprint (`REFERENCE_IMAGE::Rotate`, `Flip`) -- and written back byte for byte.
//!
//! A locked item (`(locked yes)`), or one the board's reader of its kind refuses, stays the text of its form (`FootprintExtra::tokens`), as before.

use eda_model::barcode::Barcode;
use eda_model::fp_items::FootprintItems;
use eda_model::fp_items_place::{transformed, unplacement_steps, ItemXform};
use eda_model::ir::{FootprintInstance, Nm, Point, Side};
use eda_model::pcb_items::{PcbPoint, RefImage, TextBox};

use crate::sexpr::{self, Sexpr};

/// The tags of the four kinds inside a footprint.
pub(crate) const ITEM_TAGS: [&str; 4] = ["fp_text_box", "image", "point", "barcode"];

/// One item of the four kinds as the file has it, in the file's frame.
pub(crate) enum FileItem {
    TextBox(TextBox),
    Image(RefImage),
    Point(PcbPoint),
    Barcode(Barcode),
}

/// One `fp_text_box`, `(image ..)`, `(point ..)` or `(barcode ..)` of a footprint, read by the board's reader of its kind (`parsePCB_TEXTBOX`, `parsePCB_REFERENCE_IMAGE`,
/// `parsePCB_POINT`, `parsePCB_BARCODE`); `None` for another tag, a locked item (the footprint's items keep no lock: `parsePCB_POINT` reads none, the others are kept as text)
/// and a form the reader refuses. `version` is the board file's (an image's scale depends on it: `scale_from_file`).
pub(crate) fn read_file_item(item: &[Sexpr], version: i64) -> Option<FileItem> {
    let tag = sexpr::tag(item)?;
    if !ITEM_TAGS.contains(&tag) || (tag != "point" && crate::import_items::is_locked(item)) {
        return None;
    }
    Some(match tag {
        "fp_text_box" => FileItem::TextBox(crate::pcb_textbox::read(item, version)?),
        "image" => {
            let mut img = crate::pcb_image::read(item)?;
            img.scale_ppm = crate::pcb_image::scale_from_file(img.scale_ppm, &img.data, version);
            FileItem::Image(img)
        }
        "point" => FileItem::Point(crate::pcb_point::read(item)),
        _ => FileItem::Barcode(crate::barcode_io::read(item)?),
    })
}

/// Whether `item` is read into the footprint's items (and so not kept as the text of its form).
pub(crate) fn is_read_item(item: &[Sexpr], version: i64) -> bool {
    read_file_item(item, version).is_some()
}

/// The steps that take a text box of the file's frame into the model's (see the module doc): a footprint on the back is in the file flipped top to bottom
/// (`FOOTPRINT::Flip`), which is the model's left-right flip (`to_board`) and half a turn when the footprint is held in the library's frame (`turn`).
fn text_box_steps_in(side: Side, turn: bool) -> Vec<ItemXform> {
    let origin = Point { x: Nm::ZERO, y: Nm::ZERO };
    let mut steps = Vec::new();
    if side == Side::Bottom {
        steps.push(ItemXform::Flip { pivot: origin, top_bottom: false });
    }
    if turn {
        steps.push(ItemXform::Rotate { pivot: origin, angle: 180_000 });
    }
    steps
}

/// The reverse of [`text_box_steps_in`]: from the model's frame to the file's.
fn text_box_steps_out(side: Side, turn: bool) -> Vec<ItemXform> {
    let origin = Point { x: Nm::ZERO, y: Nm::ZERO };
    let mut steps = Vec::new();
    if turn {
        steps.push(ItemXform::Rotate { pivot: origin, angle: -180_000 });
    }
    if side == Side::Bottom {
        steps.push(ItemXform::Flip { pivot: origin, top_bottom: false });
    }
    steps
}

/// The footprint's items of the four kinds (`fp`: the footprint's form), in the frame of the footprint at `pose` (the pose the model gives it; `turn`: a footprint on the
/// back held in the library's frame, `FootprintExtra::library_frame`), each with its id; and the file uuid of each item by its id (the footprint's groups name them so).
pub(crate) fn read_items(fp: &[Sexpr], version: i64, pose: &FootprintInstance, turn: bool) -> (FootprintItems, Vec<(String, String)>) {
    let mut in_file = FootprintItems::default();
    let mut uuids: Vec<(usize, Option<String>)> = Vec::new();
    let mut board = FootprintItems::default();
    for item in fp.iter().filter_map(Sexpr::as_list) {
        let Some(read) = read_file_item(item, version) else { continue };
        let uuid = crate::import_items::item_uuid(item);
        match read {
            FileItem::TextBox(t) => {
                uuids.push((0, uuid));
                in_file.text_boxes.push(t);
            }
            FileItem::Image(i) => {
                uuids.push((1, uuid));
                board.images.push(i);
            }
            FileItem::Point(p) => {
                uuids.push((2, uuid));
                board.points.push(p);
            }
            FileItem::Barcode(b) => {
                uuids.push((3, uuid));
                board.barcodes.push(b);
            }
        }
    }
    let mut items = transformed(&in_file, &text_box_steps_in(pose.side, turn));
    let off_board = transformed(&board, &unplacement_steps(pose));
    items.images = off_board.images;
    items.points = off_board.points;
    items.barcodes = off_board.barcodes;
    items.assign_missing_ids(&mut Default::default());
    // the uuids in step with the lists, kind by kind in file order
    let ids: [Vec<String>; 4] = [
        items.text_boxes.iter().map(|t| t.id.clone()).collect(),
        items.images.iter().map(|i| i.id.clone()).collect(),
        items.points.iter().map(|p| p.id.clone()).collect(),
        items.barcodes.iter().map(|b| b.id.clone()).collect(),
    ];
    let mut next = [0usize; 4];
    let mut by_id = Vec::new();
    for (kind, uuid) in uuids {
        let id = ids[kind].get(next[kind]).cloned();
        next[kind] += 1;
        if let (Some(id), Some(uuid)) = (id, uuid) {
            by_id.push((id, uuid));
        }
    }
    (items, by_id)
}

/// The uuid an item of the footprint `reference` is written with: made from the footprint and the item's id, as the art's are made from their places
/// (`crate::fp_art_io::write_art`).
pub(crate) fn item_uuid(reference: &str, id: &str) -> String {
    crate::duid_for(&format!("footprint:{reference}:item:{id}"), reference)
}

/// The footprint's items as `format( const FOOTPRINT* )` writes them, after its graphics and texts: the images, the text boxes (`fp_text_box`) and the barcodes in
/// `FOOTPRINT::cmp_drawings`' order of types, then the points (`Points()`, before the pads). `pose` is the footprint's pose in the model, `turn` whether it is written
/// half a turn from it (`write_footprint`'s `turn`). An image, a point and a barcode are written where they are on the board, a text box in the file's frame.
pub(crate) fn write_items(out: &mut String, items: &FootprintItems, pose: &FootprintInstance, turn: bool) {
    let on_board = eda_model::fp_items_place::placed(items, pose);
    let boxes = transformed(&FootprintItems { text_boxes: items.text_boxes.clone(), ..Default::default() }, &text_box_steps_out(pose.side, turn));
    for img in &on_board.images {
        crate::pcb_image::write_with_uuid(out, img, false, &item_uuid(&pose.id, &img.id));
    }
    for t in &boxes.text_boxes {
        crate::pcb_textbox::write_as(out, t, false, "fp_text_box", &item_uuid(&pose.id, &t.id));
    }
    for b in &on_board.barcodes {
        crate::barcode_io::write_with_uuid(out, b, false, &item_uuid(&pose.id, &b.id));
    }
    for p in &on_board.points {
        crate::pcb_point::write_with_uuid(out, p, &item_uuid(&pose.id, &p.id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::units::mm;

    fn parse(text: &str) -> Vec<Sexpr> {
        sexpr::parse(text).unwrap().as_list().unwrap().to_vec()
    }

    fn pose(x: f64, y: f64, rot: u32, side: Side) -> FootprintInstance {
        FootprintInstance { id: "U1".into(), at: Point { x: mm(x), y: mm(y) }, rot, side, label: Default::default() }
    }

    fn png() -> String {
        let data = eda_model::image_info::base64_encode(&eda_model::image_info::solid_png(8, 4, [200, 10, 10], 300));
        data.as_bytes().chunks(76).map(|c| format!("\"{}\"", std::str::from_utf8(c).unwrap())).collect::<Vec<_>>().join(" ")
    }

    fn footprint(at: &str, layer: &str, items: &str) -> String {
        format!(r#"(footprint "L:F" (layer "{layer}") (at {at}) {items})"#)
    }

    #[test]
    fn the_four_kinds_are_read_in_the_footprints_frame_and_a_locked_one_is_left_as_text() {
        let items = format!(
            r#"(fp_text_box "box" (start 1 1) (end 4 3) (margins 0.5 0.5 0.5 0.5) (angle 0) (layer "F.SilkS") (uuid "a1") (effects (font (size 1 1) (thickness 0.15)) (justify left top)) (border yes) (stroke (width 0.1) (type solid)))
            (image (at 102 50) (layer "F.SilkS") (data {}) (uuid "a2"))
            (point (at 100 52) (size 1) (layer "F.Fab") (uuid "a3"))
            (barcode (at 98 50 30) (layer "F.SilkS") (size 3 3) (text "x") (text_height 1) (type qr) (ecc_level L) (hide no) (knockout no) (uuid "a4"))
            (point (locked yes) (at 100 50) (size 1) (layer "F.Fab") (uuid "a5"))
            (fp_text_box "locked" (locked yes) (start 0 0) (end 1 1) (margins 0 0 0 0) (layer "F.SilkS") (uuid "a6") (effects (font (size 1 1))) (border yes) (stroke (width 0.1) (type solid)))"#,
            png()
        );
        let fp = parse(&footprint("100 50", "F.Cu", &items));
        let (read, uuids) = read_items(&fp, 20241229, &pose(100.0, 50.0, 0, Side::Top), false);
        assert_eq!(read.text_boxes.len(), 1, "the locked box stays text");
        assert_eq!(read.points.len(), 2, "a point has no lock: `parsePCB_POINT` reads none");
        assert_eq!((read.text_boxes[0].start, read.text_boxes[0].end), (Point { x: mm(1.0), y: mm(1.0) }, Point { x: mm(4.0), y: mm(3.0) }), "a text box is in the footprint's frame in the file");
        assert_eq!(read.images[0].at, Point { x: mm(2.0), y: Nm::ZERO }, "an image is where it is on the board in the file");
        assert_eq!(read.points[0].at, Point { x: Nm::ZERO, y: mm(2.0) });
        assert_eq!((read.barcodes[0].at, read.barcodes[0].angle), (Point { x: mm(-2.0), y: Nm::ZERO }, 30_000));
        assert_eq!(uuids.len(), 5);
        assert!(uuids.iter().any(|(id, u)| *id == read.images[0].id && u == "a2"));
        assert!(!is_read_item(&parse(r#"(fp_text_box "locked" (locked yes) (start 0 0) (end 1 1) (layer "F.SilkS"))"#), 20241229));
    }

    #[test]
    fn what_is_written_reads_back_the_same_on_either_side_at_any_turn() {
        let items = format!(
            r#"(fp_text_box "box" (start 1 1) (end 4 3) (margins 0.5 0.5 0.5 0.5) (layer "F.SilkS") (uuid "a1") (effects (font (size 1 1) (thickness 0.15)) (justify left top)) (border yes) (stroke (width 0.1) (type solid)))
            (image (at 2 0) (layer "F.SilkS") (data {}) (uuid "a2"))
            (point (at 0 2) (size 1) (layer "F.Fab") (uuid "a3"))
            (barcode (at -2 0 30) (layer "F.SilkS") (size 3 3) (text "x") (text_height 1) (type qr) (ecc_level L) (hide no) (knockout no) (uuid "a4"))"#,
            png()
        );
        let lib = parse(&footprint("0 0", "F.Cu", &items));
        let (model, _) = read_items(&lib, 20241229, &pose(0.0, 0.0, 0, Side::Top), false);
        for (rot, side, turn) in [(0, Side::Top, false), (90_000, Side::Top, false), (330_000, Side::Top, false), (0, Side::Bottom, true), (90_000, Side::Bottom, true)] {
            let at = pose(30.0, 20.0, rot, side);
            let mut out = String::new();
            write_items(&mut out, &model, &at, turn);
            let layer = if side == Side::Bottom { "B.Cu" } else { "F.Cu" };
            let text = footprint("30 20", layer, &out);
            let (back, uuids) = read_items(&parse(&text), 20241229, &at, turn);
            assert_eq!(uuids.len(), 4);
            assert_eq!(back.points.iter().map(|p| (p.at, &p.layer)).collect::<Vec<_>>(), model.points.iter().map(|p| (p.at, &p.layer)).collect::<Vec<_>>(), "{rot} {side:?}\n{out}");
            assert_eq!(back.images.iter().map(|i| (i.at, &i.layer, &i.data)).collect::<Vec<_>>(), model.images.iter().map(|i| (i.at, &i.layer, &i.data)).collect::<Vec<_>>(), "{rot} {side:?}");
            assert_eq!(back.barcodes.iter().map(|b| (b.at, b.angle, &b.layer)).collect::<Vec<_>>(), model.barcodes.iter().map(|b| (b.at, b.angle, &b.layer)).collect::<Vec<_>>(), "{rot} {side:?}");
            let tb = |t: &TextBox| (t.start, t.end, t.angle, t.layer.clone(), t.mirror);
            assert_eq!(back.text_boxes.iter().map(tb).collect::<Vec<_>>(), model.text_boxes.iter().map(tb).collect::<Vec<_>>(), "{rot} {side:?}\n{out}");
            if side == Side::Bottom {
                // the file has KiCad's flipped footprint: the box mirrored top to bottom on the back silkscreen, its text mirrored
                assert!(out.contains("(start 1 -1) (end 4 -3)") && out.contains("B.SilkS") && out.contains("mirror"), "{out}");
            }
        }
    }
}
