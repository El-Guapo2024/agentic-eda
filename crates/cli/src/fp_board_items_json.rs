//! A board footprint's text boxes, reference images, points and barcodes (UI-LEFT P17) and its dimensions and tables (UI-LEFT PP14) for the studio's state: `parts[].items`, in board space like the footprint's art
//! (`crate::fp_art_json`), each in the JSON of the board's own item of its kind (`crate::pcb_items_json`) so the canvas draws it with the board's painters. The model holds them
//! in the footprint's frame ([`eda_model::ir::FootprintExtra::items`]); [`eda_model::fp_items_place::placed`] takes them to the board for the pose the footprint is in, so a
//! move, a turn or a flip needs nothing here. Each has the id [`eda_model::fp_items_place::board_id`] gives it (`U1~img_..`): an image's file
//! (`GET /api/pcb_image`) and a barcode's shape (`GET /api/barcode_shape`) are asked for by it, and the pixels a turn sends (`set_ref_image_data`) are filed under it.
//!
//! A text's variables are resolved as `PCB_TEXTBOX::GetShownText` resolves them: `${LAYER}` is the item's own layer, then the footprint's (`FOOTPRINT::ResolveTextVar`:
//! `${REFERENCE}`, `${VALUE}`, its fields, `${NET_NAME(n)}` ...), then the board's ([`PcbShown::footprint_text_shown`]).

use eda_model::barcode::Barcode;
use eda_model::fp_items_place::{board_id, items_of, parse_board_id, placed};
use eda_model::ir::{Design, FootprintInstance};
use eda_model::pcb_items::RefImage;
use eda_model::{ConstraintModel, Part};
use serde_json::{json, Value};

use crate::text_vars_json::PcbShown;

/// Put the footprint's items into the part's JSON `p` (`items: { text_boxes, images, points, barcodes, dimensions, tables }`), absent when it has none. A dimension is the
/// board's dimension JSON (`crate::studio::dimension_json`: its lines and its text worked out where it is), a table the board's (`crate::table_json::table_json`: its
/// borders and its cells' lines placed), each under the item's board id.
pub(crate) fn add_items(p: &mut Value, design: &Design, model: &ConstraintModel, part: &Part, fp: &FootprintInstance, tv: &PcbShown) {
    let Some(items) = items_of(design, model, part) else { return };
    let on_board = placed(items, fp);
    let mut out = serde_json::Map::new();
    if !on_board.text_boxes.is_empty() {
        let boxes = on_board.text_boxes.iter().map(|t| {
            let mut v = crate::pcb_items_json::text_box_json(t, tv);
            v["id"] = json!(board_id(&part.reference, &t.id));
            let shown = tv.footprint_text_shown(&part.reference, &t.text, &t.layer);
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
            let shown = tv.footprint_text_shown(&part.reference, &b.text, &b.layer);
            if shown != b.text {
                v["shown"] = json!(shown);
            } else if let Some(o) = v.as_object_mut() {
                o.remove("shown");
            }
            v
        });
        out.insert("barcodes".into(), Value::Array(barcodes.collect()));
    }
    if !on_board.dimensions.is_empty() {
        let dims = on_board.dimensions.iter().map(|d| {
            let mut v = crate::studio::dimension_json(d);
            v["id"] = json!(board_id(&part.reference, &d.id));
            v
        });
        out.insert("dimensions".into(), Value::Array(dims.collect()));
    }
    if !on_board.tables.is_empty() {
        let tables = on_board.tables.iter().map(|t| crate::table_json::table_json(&eda_model::table::Table { id: board_id(&part.reference, &t.id), ..t.clone() }, tv.board()));
        out.insert("tables".into(), Value::Array(tables.collect()));
    }
    if !out.is_empty() {
        p["items"] = Value::Object(out);
    }
}

/// The items on the board of the footprint the id of one of its items names (`U1~img_..`).
fn placed_of(design: &Design, model: &ConstraintModel, id: &str) -> Option<eda_model::fp_items::FootprintItems> {
    let (reference, _) = parse_board_id(id)?;
    let part = model.part(reference)?;
    let fp = design.placement.as_ref()?.footprints.iter().find(|f| f.id == reference)?;
    Some(placed(items_of(design, model, part)?, fp))
}

/// The image of a footprint that `id` names, as the board has it (`GET /api/pcb_image`).
pub(crate) fn placed_image(design: &Design, model: &ConstraintModel, id: &str) -> Option<RefImage> {
    let (_, item) = parse_board_id(id)?;
    placed_of(design, model, id)?.images.into_iter().find(|i| i.id == item)
}

/// The barcode of a footprint that `id` names, on the board, with the footprint's reference (its text is resolved against the footprint): what `GET /api/barcode_shape` plots.
pub(crate) fn placed_barcode(design: &Design, model: &ConstraintModel, id: &str) -> Option<(Barcode, String)> {
    let (reference, item) = parse_board_id(id)?;
    let b = placed_of(design, model, id)?.barcodes.into_iter().find(|b| b.id == item)?;
    Some((b, reference.to_string()))
}
