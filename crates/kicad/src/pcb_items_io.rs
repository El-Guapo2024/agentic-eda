//! The board items of [`eda_model::pcb_items`] (text boxes, reference images, points, shape styles) read and written, as one call each for the importer and the exporter, so
//! that `import.rs` and `pcb.rs` hold a line each for all of them.

use std::collections::BTreeMap;

use eda_model::pcb_items::BoardItems;

use crate::import_items::ItemRef;
use crate::sexpr::Sexpr;

/// The file uuid and lock of each item of [`BoardItems`], in the order the items were read.
#[derive(Debug, Default)]
pub(crate) struct ItemRefs {
    pub text_boxes: Vec<ItemRef>,
    pub images: Vec<ItemRef>,
    pub points: Vec<ItemRef>,
}

/// Every item of the kinds [`BoardItems`] holds that the board `root` has.
pub(crate) fn read_all(root: &[Sexpr], refs: &mut ItemRefs) -> BoardItems {
    BoardItems { text_boxes: crate::pcb_textbox::read_all(root, &mut refs.text_boxes), images: crate::pcb_image::read_all(root, &mut refs.images), points: crate::pcb_point::read_all(root, &mut refs.points), shape_styles: Vec::new() }
}

/// Write the items, one after another in id order, and note their uuids in `written` for the groups that name them.
pub(crate) fn write_all(out: &mut String, items: &BoardItems, is_locked: &dyn Fn(&str) -> bool, written: &mut BTreeMap<String, Vec<String>>) {
    let mut boxes: Vec<&eda_model::pcb_items::TextBox> = items.text_boxes.iter().collect();
    boxes.sort_by(|a, b| a.id.cmp(&b.id));
    for t in boxes {
        let uuid = crate::pcb_textbox::write(out, t, is_locked(&t.id));
        written.entry(t.id.clone()).or_default().push(uuid);
    }
    let mut images: Vec<&eda_model::pcb_items::RefImage> = items.images.iter().collect();
    images.sort_by(|a, b| a.id.cmp(&b.id));
    for img in images {
        let uuid = crate::pcb_image::write(out, img, is_locked(&img.id));
        written.entry(img.id.clone()).or_default().push(uuid);
    }
    let mut points: Vec<&eda_model::pcb_items::PcbPoint> = items.points.iter().collect();
    points.sort_by(|a, b| a.id.cmp(&b.id));
    for p in points {
        // A point is not a groupable type (`BOARD_ITEM::IsGroupableType`): no group names it as a member in the file.
        crate::pcb_point::write(out, p);
    }
}
