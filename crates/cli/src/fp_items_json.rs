//! The items of a library footprint that `eda_model::fp_items` adds, as the Footprint Editor's canvas draws them (`GET /api/footprint`, its `display`): the board's own
//! display shapes (`web/studio/src/api/types.ts` `Dimension`, `BoardTextBox`, `BoardTable` ...), so the board's painters draw them unchanged. The geometry KiCad works
//! out from an item (a dimension's lines and text, a table's placed text lines) is worked out here, once, as it is for the board (`crate::studio::dimension_json`,
//! `crate::table_json`).
//!
//! The texts are the footprint editor's: its board is a footprint holder (`BOARD_USE::FPHOLDER`), whose footprint resolves no variable
//! (`FOOTPRINT::ResolveTextVar` returns false), so a `${REFERENCE}` shows as it is written; the board's own variables (`${LAYER}`, the project's) still resolve.

use eda_model::ir::LibraryFootprint;
use serde_json::{json, Value};

use crate::text_vars_json::PcbShown;

/// One zone of the footprint: what the canvas draws and Rule Area Properties / Zone Properties edit (the IR's zone with `[x, y]` outlines, as the board's state has it).
fn zone_json(z: &eda_model::ir::Zone) -> Value {
    let mut v = serde_json::to_value(z).unwrap_or(Value::Null);
    v["layers"] = json!(z.layer_set());
    v["outline_xy"] = json!(z.outline.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>());
    v["holes_xy"] = json!(z.holes.iter().map(|h| h.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>()).collect::<Vec<_>>());
    v
}

/// The fills the footprint's zones brought (`LibraryFootprintKeep::zone_fills`), as they hold for each zone now (`eda_kicad::zone_fills::kept_fill_current`): one entry per
/// zone and layer in the shape of the board's `GET /api/fill` (`FillZone`: its rings as `fragments`), which the board's zone painter draws (UI-LEFT-2 FF6). A rule area
/// has none.
fn zone_fills_json(fp: &LibraryFootprint) -> Value {
    let mut out = Vec::new();
    for z in fp.items.zones.iter().filter(|z| !z.is_rule_area) {
        let Some(entry) = fp.kicad.zone_fills.get(&z.id).and_then(|k| eda_kicad::zone_fills::kept_fill_current(k, z)) else { continue };
        for (layer, rings) in &entry.layers {
            let fragments: Vec<Vec<[i64; 2]>> = rings.iter().map(|r| r.pts.clone()).collect();
            out.push(json!({ "id": z.id, "net": "", "layer": layer, "filled": !entry.unfilled, "area_nm2": 0, "fragments": fragments }));
        }
    }
    json!(out)
}

/// `display` of `GET /api/footprint`: the dimensions, zones, text boxes, images, points, barcodes and tables of `fp` in the board's display shapes, and the zones' fills.
pub(crate) fn display_json(fp: &LibraryFootprint, tv: &PcbShown) -> Value {
    let it = &fp.items;
    json!({
        "zone_fills": zone_fills_json(fp),
        "dimensions": it.dimensions.iter().map(crate::studio::dimension_json).collect::<Vec<_>>(),
        "zones": it.zones.iter().map(zone_json).collect::<Vec<_>>(),
        "text_boxes": it.text_boxes.iter().map(|t| crate::pcb_items_json::text_box_json(t, tv)).collect::<Vec<_>>(),
        "images": it.images.iter().map(crate::pcb_items_json::image_json).collect::<Vec<_>>(),
        "points": it.points.iter().map(crate::pcb_items_json::point_json).collect::<Vec<_>>(),
        "barcodes": it.barcodes.iter().map(|b| crate::pcb_items_json::barcode_json(b, tv)).collect::<Vec<_>>(),
        "tables": it.tables.iter().map(|t| crate::table_json::table_json(t, tv.board())).collect::<Vec<_>>(),
    })
}
