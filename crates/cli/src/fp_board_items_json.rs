//! A board footprint's text boxes, reference images, points and barcodes for the studio's state (UI-LEFT P17): `parts[].items`, in board space like the footprint's art
//! (`crate::fp_art_json`), each in the JSON of the board's own item of its kind (`crate::pcb_items_json`) so the canvas draws it with the board's painters. The model holds them
//! in the footprint's frame ([`eda_model::ir::FootprintExtra::items`]); [`eda_model::fp_items_place::placed`] takes them to the board for the pose the footprint is in, so a
//! move, a turn or a flip needs nothing here. Each has the id [`eda_model::fp_items_place::board_id`] gives it (`U1~img_..`): an image's file
//! (`GET /api/pcb_image`) and a barcode's shape (`GET /api/barcode_shape`) are asked for by it, and the pixels a turn sends (`set_ref_image_data`) are filed under it.
//!
//! A text's variables are resolved as `PCB_TEXTBOX::GetShownText` resolves them: `${LAYER}` is the item's own layer, then the footprint's (`FOOTPRINT::ResolveTextVar`:
//! `${REFERENCE}`, `${VALUE}` ...), then the board's.

use eda_model::barcode::Barcode;
use eda_model::fp_art::{resolve_text_vars, TextVars};
use eda_model::fp_items_place::{board_id, items_of, parse_board_id, placed};
use eda_model::ir::{Design, FootprintInstance};
use eda_model::pcb_items::RefImage;
use eda_model::{ConstraintModel, Part};
use serde_json::{json, Value};

use crate::text_vars_json::PcbShown;

/// `text` with the footprint's variables resolved and `${LAYER}` as `layer`: what is left is the board's to resolve.
fn with_footprint_vars(text: &str, layer: &str, part: &Part, fp: &FootprintInstance) -> String {
    let fpid = eda_model::footprint::footprint_lib_id(part);
    let vars = TextVars { reference: &part.reference, value: part.value.as_deref().unwrap_or(""), footprint: &fpid };
    resolve_text_vars(&text.replace("${LAYER}", layer), &vars, fp.side)
}

/// Put the footprint's items into the part's JSON `p` (`items: { text_boxes, images, points, barcodes }`), absent when it has none.
pub(crate) fn add_items(p: &mut Value, design: &Design, model: &ConstraintModel, part: &Part, fp: &FootprintInstance, tv: &PcbShown) {
    let Some(items) = items_of(design, model, part) else { return };
    let on_board = placed(items, fp);
    let mut out = serde_json::Map::new();
    if !on_board.text_boxes.is_empty() {
        let boxes = on_board.text_boxes.iter().map(|t| {
            let mut v = crate::pcb_items_json::text_box_json(t, tv);
            v["id"] = json!(board_id(&part.reference, &t.id));
            let shown = tv.text_shown(&with_footprint_vars(&t.text, &t.layer, part, fp), &t.layer);
            if shown != t.text {
                v["shown"] = json!(shown);
            } else if let Some(o) = v.as_object_mut() {
                o.remove("shown");
            }
            v
        });
        out.insert("text_boxes".into(), Value::Array(boxes.collect()));
    }
    if !on_board.images.is_empty() {
        let images = on_board.images.iter().map(|i| {
            let mut v = crate::pcb_items_json::image_json(i);
            v["id"] = json!(board_id(&part.reference, &i.id));
            v
        });
        out.insert("images".into(), Value::Array(images.collect()));
    }
    if !on_board.points.is_empty() {
        let points = on_board.points.iter().map(|q| {
            let mut v = crate::pcb_items_json::point_json(q);
            v["id"] = json!(board_id(&part.reference, &q.id));
            v
        });
        out.insert("points".into(), Value::Array(points.collect()));
    }
    if !on_board.barcodes.is_empty() {
        let barcodes = on_board.barcodes.iter().map(|b| {
            let mut v = crate::pcb_items_json::barcode_json(b, tv);
            v["id"] = json!(board_id(&part.reference, &b.id));
            let shown = tv.text_shown(&with_footprint_vars(&b.text, &b.layer, part, fp), &b.layer);
            if shown != b.text {
                v["shown"] = json!(shown);
            } else if let Some(o) = v.as_object_mut() {
                o.remove("shown");
            }
            v
        });
        out.insert("barcodes".into(), Value::Array(barcodes.collect()));
    }
    if !out.is_empty() {
        p["items"] = Value::Object(out);
    }
}

/// The part and pose of the footprint the id of one of its items names (`U1~img_..`), with its items on the board.
fn placed_of(design: &Design, model: &ConstraintModel, id: &str) -> Option<(Part, FootprintInstance, eda_model::fp_items::FootprintItems)> {
    let (reference, _) = parse_board_id(id)?;
    let part = model.part(reference)?.clone();
    let fp = design.placement.as_ref()?.footprints.iter().find(|f| f.id == reference)?.clone();
    let items = placed(items_of(design, model, &part)?, &fp);
    Some((part, fp, items))
}

/// The image of a footprint that `id` names, as the board has it (`GET /api/pcb_image`).
pub(crate) fn placed_image(design: &Design, model: &ConstraintModel, id: &str) -> Option<RefImage> {
    let (_, item) = parse_board_id(id)?;
    let (_, _, items) = placed_of(design, model, id)?;
    items.images.into_iter().find(|i| i.id == item)
}

/// The barcode of a footprint that `id` names, on the board, its text with the footprint's variables resolved (the board's are left to [`crate::barcode_api`]): what
/// `GET /api/barcode_shape` plots.
pub(crate) fn placed_barcode(design: &Design, model: &ConstraintModel, id: &str) -> Option<Barcode> {
    let (_, item) = parse_board_id(id)?;
    let (part, fp, items) = placed_of(design, model, id)?;
    let mut b = items.barcodes.into_iter().find(|b| b.id == item)?;
    b.text = with_footprint_vars(&b.text, &b.layer, &part, &fp);
    Some(b)
}
