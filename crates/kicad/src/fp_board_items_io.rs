//! A board footprint's text boxes, reference images, points and barcodes (UI-LEFT P17) and its dimensions and tables (UI-LEFT PP14): `fp_text_box`, `(image ..)`,
//! `(point ..)`, `(barcode ..)`, `(dimension ..)` and `(table ..)` inside a `(footprint ..)` of a `.kicad_pcb`, read into [`eda_model::ir::FootprintExtra::items`] and
//! written back, as `PCB_IO_KICAD_SEXPR_PARSER::parseFOOTPRINT_unchecked` and `PCB_IO_KICAD_SEXPR::format( const FOOTPRINT* )` do at KiCad 8303b2ad. Before this they were
//! kept as the text of their forms (`FootprintExtra::tokens`), which put an image, a point, a barcode or a dimension back where it was read whatever became of the
//! footprint (their file positions are the board's) and a text box or a table where the footprint's frame had it, never flipped; a dimension and a table were not drawn.
//!
//! # Frames
//!
//! The file does not write the four kinds in one frame:
//! - a text box is in the footprint's own frame (`format( PCB_TEXTBOX )`: `formatInternalUnits( aTextBox->GetStart(), parentFP )`, `angle -= parentFP->GetOrientation()`;
//!   `parsePCB_TEXTBOX` ends with `Rotate( { 0, 0 }, parentFP->GetOrientation() ); Move( parentFP->GetPosition() )`) -- the frame of the footprint as `FOOTPRINT::Flip`
//!   left it, so a footprint on the back has it flipped top to bottom; so is a table, whose cells are text boxes the footprint is an ancestor of (`GetParentFootprint()`
//!   is `findParent( PCB_FOOTPRINT_T )`, and `parseTextBoxContent` ends the same for a `table_cell`);
//! - an image, a point, a barcode and a dimension are where they are on the board (`format( PCB_REFERENCE_IMAGE )`, `format( PCB_POINT )`, `format( PCB_BARCODE )` and
//!   `format( PCB_DIMENSION_BASE )` write `GetPosition()`, `GetStart()` / `GetEnd()` and the barcode's own `GetAngle()` with no `parentFP`, and their parsers set them as
//!   read).
//!
//! The model holds all four in the frame of the footprint's pads ([`eda_model::fp_items_place`]): a text box of a footprint on the back is flipped back, as its art is
//! (`crate::fp_art_io`, the library's frame, `FootprintExtra::library_frame`); the others are taken off the board by the footprint's pose. An image's pixels are kept as
//! the file has them -- as the board shows them, turned and mirrored with the footprint (`REFERENCE_IMAGE::Rotate`, `Flip`) -- and written back byte for byte.
//!
//! A locked item (`(locked yes)`) is read with its lock (`FootprintExtra::locked_items`; a point has none, `parsePCB_POINT` reads no lock); one the board's reader of its
//! kind refuses (a dimension of the pre-6.0 form, a table whose cells are off its grid ..) stays the text of its form (`FootprintExtra::tokens`), as before.

use eda_model::barcode::Barcode;
use eda_model::fp_items::FootprintItems;
use eda_model::fp_items_place::{transformed, unplacement_steps, ItemXform};
use eda_model::ir::{Dimension, FootprintInstance, Nm, Point, Side};
use eda_model::pcb_items::{PcbPoint, RefImage, TextBox};
use eda_model::table::Table;

use crate::sexpr::{self, Sexpr};

/// The tags of the six kinds inside a footprint.
pub(crate) const ITEM_TAGS: [&str; 6] = ["fp_text_box", "image", "point", "barcode", "dimension", "table"];

/// One item of the six kinds as the file has it, in the file's frame.
pub(crate) enum FileItem {
    TextBox(TextBox),
    Image(RefImage),
    Point(PcbPoint),
    Barcode(Barcode),
    Dimension(Dimension),
    Table(Table),
}

/// One `fp_text_box`, `(image ..)`, `(point ..)`, `(barcode ..)`, `(dimension ..)` or `(table ..)` of a footprint, read by the board's reader of its kind
/// (`parsePCB_TEXTBOX`, `parsePCB_REFERENCE_IMAGE`, `parsePCB_POINT`, `parsePCB_BARCODE`, `parseDIMENSION`, `parsePCB_TABLE`), with whether it is locked (`(locked yes)`;
/// `parsePCB_POINT` reads no lock); `None` for another tag and a form the reader refuses. `version` is the board file's (an image's scale depends on it: `scale_from_file`).
pub(crate) fn read_file_item(item: &[Sexpr], version: i64) -> Option<(FileItem, bool)> {
    let tag = sexpr::tag(item)?;
    if !ITEM_TAGS.contains(&tag) {
        return None;
    }
    let locked = tag != "point" && crate::import_items::is_locked(item);
    let read = match tag {
        "fp_text_box" => FileItem::TextBox(crate::pcb_textbox::read(item, version)?),
        "image" => {
            let mut img = crate::pcb_image::read(item)?;
            img.scale_ppm = crate::pcb_image::scale_from_file(img.scale_ppm, &img.data, version);
            FileItem::Image(img)
        }
        "point" => FileItem::Point(crate::pcb_point::read(item)),
        "barcode" => FileItem::Barcode(crate::barcode_io::read(item)?),
        "dimension" => FileItem::Dimension(crate::import_items::read_dimension(item)?),
        // a footprint's table keeps its uuid where its items keep theirs (the footprint's groups name it so); its cells keep theirs (`TableCell::uuid`)
        _ => FileItem::Table(Table { uuid: String::new(), ..crate::pcb_table::read(item)? }),
    };
    Some((read, locked))
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

/// The footprint's items of the six kinds as [`read_items`] gives them.
pub(crate) struct ReadItems {
    pub items: FootprintItems,
    /// The file uuid of each item, by its id (the footprint's groups name them so).
    pub uuids: Vec<(String, String)>,
    /// The ids of the items the file says are locked (`FootprintExtra::locked_items`).
    pub locked: Vec<String>,
}

/// The footprint's items of the six kinds (`fp`: the footprint's form), in the frame of the footprint at `pose` (the pose the model gives it; `turn`: a footprint on the
/// back held in the library's frame, `FootprintExtra::library_frame`), each with its id, its file uuid and its lock.
pub(crate) fn read_items(fp: &[Sexpr], version: i64, pose: &FootprintInstance, turn: bool) -> ReadItems {
    let mut in_file = FootprintItems::default();
    let mut read_as: Vec<(usize, Option<String>, bool)> = Vec::new();
    let mut board = FootprintItems::default();
    for item in fp.iter().filter_map(Sexpr::as_list) {
        let Some((read, locked)) = read_file_item(item, version) else { continue };
        let uuid = crate::import_items::item_uuid(item);
        let kind = match read {
            FileItem::TextBox(t) => {
                in_file.text_boxes.push(t);
                0
            }
            FileItem::Image(i) => {
                board.images.push(i);
                1
            }
            FileItem::Point(p) => {
                board.points.push(p);
                2
            }
            FileItem::Barcode(b) => {
                board.barcodes.push(b);
                3
            }
            FileItem::Dimension(d) => {
                board.dimensions.push(d);
                4
            }
            FileItem::Table(t) => {
                in_file.tables.push(t);
                5
            }
        };
        read_as.push((kind, uuid, locked));
    }
    let mut items = transformed(&in_file, &text_box_steps_in(pose.side, turn));
    let off_board = transformed(&board, &unplacement_steps(pose));
    items.images = off_board.images;
    items.points = off_board.points;
    items.barcodes = off_board.barcodes;
    items.dimensions = off_board.dimensions;
    items.assign_missing_ids(&mut Default::default());
    // the uuids and locks in step with the lists, kind by kind in file order
    let ids: [Vec<String>; 6] = [
        items.text_boxes.iter().map(|t| t.id.clone()).collect(),
        items.images.iter().map(|i| i.id.clone()).collect(),
        items.points.iter().map(|p| p.id.clone()).collect(),
        items.barcodes.iter().map(|b| b.id.clone()).collect(),
        items.dimensions.iter().map(|d| d.id.clone()).collect(),
        items.tables.iter().map(|t| t.id.clone()).collect(),
    ];
    let mut next = [0usize; 6];
    let (mut uuids, mut locked_ids) = (Vec::new(), Vec::new());
    for (kind, uuid, locked) in read_as {
        let id = ids[kind].get(next[kind]).cloned();
        next[kind] += 1;
        let Some(id) = id else { continue };
        if locked {
            locked_ids.push(id.clone());
        }
        if let Some(uuid) = uuid {
            uuids.push((id, uuid));
        }
    }
    ReadItems { items, uuids, locked: locked_ids }
}

/// The uuid an item of the footprint `reference` is written with: made from the footprint and the item's id, as the art's are made from their places
/// (`crate::fp_art_io::write_art`).
/// The record `own` may keep another for the item (`FootprintExtra::child_uuids`, `item:<id>`).
pub(crate) fn item_uuid_of(own: Option<&eda_model::ir::FootprintExtra>, reference: &str, id: &str) -> String {
    crate::child_uuid(own, reference, &format!("item:{id}"), &format!("footprint:{reference}:item:{id}"))
}

/// The footprint's items as `format( const FOOTPRINT* )` writes them, after its graphics and texts: the images, the text boxes (`fp_text_box`), the tables, the barcodes
/// and the dimensions in `FOOTPRINT::cmp_drawings`' order of types (`PCB_REFERENCE_IMAGE_T`, `PCB_TEXTBOX_T`, `PCB_TABLE_T`, `PCB_BARCODE_T`, the `PCB_DIM_*_T`), then the
/// points (`Points()`, before the pads). `pose` is the footprint's pose in the model, `turn` whether it is written half a turn from it (`write_footprint`'s `turn`), `locked`
/// whether an item is locked (`FootprintExtra::item_locked`). An image, a point, a barcode and a dimension are written where they are on the board, a text box and a table
/// in the file's frame.
/// `own` is the footprint's record, whose kept uuids the items take ([`item_uuid_of`]).
pub(crate) fn write_items_of(out: &mut String, items: &FootprintItems, pose: &FootprintInstance, turn: bool, locked: &dyn Fn(&str) -> bool, own: Option<&eda_model::ir::FootprintExtra>) {
    let item_uuid = |reference: &str, id: &str| item_uuid_of(own, reference, id);
    let on_board = eda_model::fp_items_place::placed(items, pose);
    let in_file = transformed(&FootprintItems { text_boxes: items.text_boxes.clone(), tables: items.tables.clone(), ..Default::default() }, &text_box_steps_out(pose.side, turn));
    for img in &on_board.images {
        crate::pcb_image::write_with_uuid(out, img, locked(&img.id), &item_uuid(&pose.id, &img.id));
    }
    for t in &in_file.text_boxes {
        crate::pcb_textbox::write_as(out, t, locked(&t.id), "fp_text_box", &item_uuid(&pose.id, &t.id));
    }
    for t in &in_file.tables {
        crate::pcb_table::write_with_uuid(out, t, locked(&t.id), &item_uuid(&pose.id, &t.id));
    }
    for b in &on_board.barcodes {
        crate::barcode_io::write_with_uuid(out, b, locked(&b.id), &item_uuid(&pose.id, &b.id));
    }
    let mut dims: Vec<&Dimension> = on_board.dimensions.iter().collect();
    dims.sort_by_key(|d| crate::footprint_lib::dimension_type_rank(d));
    for d in dims {
        // the text and its place, which KiCad works out again when it reads the file (`dim->Update()`), as the board's writer gives them; the text is the dimension
        // (`PCB_DIMENSION_BASE` is a `PCB_TEXT`), written with the dimension's uuid
        let geom = eda_connectivity::dimension::compute_dimension_geometry(d);
        let uuid = item_uuid(&pose.id, &d.id);
        crate::pcb_items::write_dimension(
            out,
            d,
            &crate::pcb_items::DimensionArgs { uuid: &uuid, text_uuid: &uuid, locked: locked(&d.id), text: &geom.text, text_at: (geom.text_at.x, geom.text_at.y), text_angle_millideg: geom.text_angle },
        );
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
    fn the_four_kinds_are_read_in_the_footprints_frame_and_a_locked_one_keeps_its_lock() {
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
        let ReadItems { items: read, uuids, locked } = read_items(&fp, 20241229, &pose(100.0, 50.0, 0, Side::Top), false);
        assert_eq!(read.text_boxes.len(), 2, "the locked box is read too");
        assert_eq!(locked.len(), 1, "with its lock: {locked:?}");
        assert!(read.text_boxes.iter().any(|t| t.text == "locked" && locked.contains(&t.id)));
        assert_eq!(read.points.len(), 2, "a point has no lock: `parsePCB_POINT` reads none");
        let tb = read.text_boxes.iter().find(|t| t.text == "box").unwrap();
        assert_eq!((tb.start, tb.end), (Point { x: mm(1.0), y: mm(1.0) }, Point { x: mm(4.0), y: mm(3.0) }), "a text box is in the footprint's frame in the file");
        assert_eq!(read.images[0].at, Point { x: mm(2.0), y: Nm::ZERO }, "an image is where it is on the board in the file");
        assert_eq!(read.points[0].at, Point { x: Nm::ZERO, y: mm(2.0) });
        assert_eq!((read.barcodes[0].at, read.barcodes[0].angle), (Point { x: mm(-2.0), y: Nm::ZERO }, 30_000));
        assert_eq!(uuids.len(), 6);
        assert!(uuids.iter().any(|(id, u)| *id == read.images[0].id && u == "a2"));
        assert!(is_read_item(&parse(r#"(fp_text_box "locked" (locked yes) (start 0 0) (end 1 1) (layer "F.SilkS"))"#), 20241229));
        assert!(!is_read_item(&parse(r#"(dimension (type aligned) (layer "F.Fab"))"#), 20241229), "a dimension with no points is left as text");
    }

    fn dims_and_tables() -> String {
        // a table whose cells are in the footprint's frame (two columns, one row) and an aligned dimension where it is on the board, from 101 to 104 at 50
        r#"(dimension (type aligned) (locked yes) (layer "F.Fab") (uuid "d1") (pts (xy 101 50) (xy 104 50)) (height -2) (format (prefix "") (suffix "") (units 2) (units_format 1) (precision 2)) (style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (extension_height 0.58642) (extension_offset 0.5) keep_text_aligned))
           (table (column_count 2) (uuid "t1") (layer "F.Fab") (border (external yes) (header yes) (stroke (width 0.1) (type solid))) (separators (rows yes) (cols yes) (stroke (width 0.1) (type solid))) (column_widths 3 4) (row_heights 2)
             (cells (table_cell "a" (start -2 1) (end 1 3) (margins 0.5 0.5 0.5 0.5) (span 1 1) (layer "F.Fab") (uuid "c1") (effects (font (size 1 1) (thickness 0.15)) (justify left top)))
                    (table_cell "b" (start 1 1) (end 5 3) (margins 0.5 0.5 0.5 0.5) (span 1 1) (layer "F.Fab") (uuid "c2") (effects (font (size 1 1) (thickness 0.15)) (justify left top)))))"#
            .to_string()
    }

    #[test]
    fn a_footprints_dimension_is_read_off_the_board_and_its_table_in_its_frame() {
        let fp = parse(&footprint("100 50", "F.Cu", &dims_and_tables()));
        let ReadItems { items, uuids, locked } = read_items(&fp, 20241229, &pose(100.0, 50.0, 0, Side::Top), false);
        assert_eq!((items.dimensions.len(), items.tables.len()), (1, 1));
        let d = &items.dimensions[0];
        assert_eq!((d.start, d.end), (Point { x: mm(1.0), y: Nm::ZERO }, Point { x: mm(4.0), y: Nm::ZERO }), "a dimension is where it is on the board in the file");
        assert_eq!(locked, vec![d.id.clone()], "the dimension is locked");
        let t = &items.tables[0];
        assert_eq!((t.at, t.col_widths.clone()), (Point { x: mm(-2.0), y: mm(1.0) }, vec![mm(3.0), mm(4.0)]), "a table is in the footprint's frame in the file");
        assert_eq!(t.cells.iter().map(|c| c.uuid.as_str()).collect::<Vec<_>>(), ["c1", "c2"]);
        assert!(uuids.iter().any(|(id, u)| *id == t.id && u == "t1") && uuids.iter().any(|(id, u)| *id == d.id && u == "d1"));
        // written back for a pose on either side at any quarter: read again the same, the lock kept
        for (rot, side, turn) in [(0, Side::Top, false), (90_000, Side::Top, false), (0, Side::Bottom, true), (270_000, Side::Bottom, true)] {
            let at = pose(30.0, 20.0, rot, side);
            let mut out = String::new();
            write_items_of(&mut out, &items, &at, turn, &|id| locked.iter().any(|l| l == id), None);
            let layer = if side == Side::Bottom { "B.Cu" } else { "F.Cu" };
            let back = read_items(&parse(&footprint("30 20", layer, &out)), 20241229, &at, turn);
            let dim = |d: &Dimension| (d.start, d.end, d.kind.clone(), d.layer.clone());
            assert_eq!(back.items.dimensions.iter().map(dim).collect::<Vec<_>>(), items.dimensions.iter().map(dim).collect::<Vec<_>>(), "{rot} {side:?}\n{out}");
            let tab = |t: &Table| (t.at, t.angle, t.layer.clone(), t.col_widths.clone(), t.cells.iter().map(|c| (c.text.clone(), c.style.mirror)).collect::<Vec<_>>());
            assert_eq!(back.items.tables.iter().map(tab).collect::<Vec<_>>(), items.tables.iter().map(tab).collect::<Vec<_>>(), "{rot} {side:?}\n{out}");
            assert_eq!(back.locked.len(), 1);
            if side == Side::Bottom {
                assert!(out.contains("B.Fab"), "on the back's layer: {out}");
            }
        }
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
        let model = read_items(&lib, 20241229, &pose(0.0, 0.0, 0, Side::Top), false).items;
        for (rot, side, turn) in [(0, Side::Top, false), (90_000, Side::Top, false), (330_000, Side::Top, false), (0, Side::Bottom, true), (90_000, Side::Bottom, true)] {
            let at = pose(30.0, 20.0, rot, side);
            let mut out = String::new();
            write_items_of(&mut out, &model, &at, turn, &|_| false, None);
            let layer = if side == Side::Bottom { "B.Cu" } else { "F.Cu" };
            let text = footprint("30 20", layer, &out);
            let ReadItems { items: back, uuids, .. } = read_items(&parse(&text), 20241229, &at, turn);
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
