//! The studio's routes for the footprint assignment tools (GAPS.md package P2): what Update PCB from Schematic lists, and, as the steps land, what the exchange,
//! update, compare and reannotate dialogs show before and after they change the board.
//!
//!  * `GET /api/update_pcb`: `common.Control.updatePcbFromSchematic` (F8, `DIALOG_UPDATE_PCB` over `BOARD_NETLIST_UPDATER`): the footprints whose symbol names another
//!    footprint than the board has, each with whether a library has the footprint. The change itself is `Cmd::ExchangeFootprints`, one per part, which the studio sends as
//!    one batch.
//!  * `POST /api/exchange_plan`: `DIALOG_EXCHANGE_FOOTPRINTS::processFootprint`'s look before the change: for each footprint the dialog's choice names, whether the footprint it
//!    is to be put on (Change) or its own library copy (Update) is found.

use crate::board;
use crate::library_index::Kind;
use serde_json::{json, Value};
use std::path::Path;

/// `{ ok, changes: [{ reference, from, to, locked, found }] }`: the dry run of Update PCB from Schematic's footprint changes (`replaceFootprint` with `m_isDryRun`:
/// "Change %s footprint from '%s' to '%s'."). `found` is false where no library has the footprint ("Cannot update %s (footprint '%s' not found)."), `locked` where the
/// footprint is locked ("Cannot change %s footprint from '%s' to '%s' (footprint is locked).").
pub fn update_pcb(dir: &Path) -> Value {
    let (_, design, model) = match board::load(dir) {
        Ok(loaded) => loaded,
        Err(e) => return json!({ "ok": false, "message": board::reasons(&e) }),
    };
    let footprints = crate::library_tables::libs(dir, Kind::Footprint);
    let changes: Vec<Value> = eda_ops::pending_footprint_changes(&design, &model)
        .into_iter()
        .map(|c| {
            let found = crate::library_place::footprint_found(&c.to, &c.reference, &design, &model, &*footprints);
            json!({ "reference": c.reference, "from": c.from, "to": c.to, "locked": c.locked, "found": found })
        })
        .collect();
    json!({ "ok": true, "changes": changes })
}

/// The entry of `/api/exchange_plan` for one footprint: where it stands before an exchange. `to` is the footprint it is to be put on; `None` is an update, which is
/// from the library copy of the footprint it is on. `found` is whether that footprint is found.
fn plan_entry(dir_libs: &dyn crate::library_index::LibSource, design: &eda_model::ir::Design, model: &eda_model::ConstraintModel, reference: &str, to: Option<&str>) -> Value {
    let Some(part) = model.part(reference) else {
        return json!({ "reference": reference, "found": false, "from": "", "to": to.unwrap_or(""), "error": format!("no footprint {reference} on this board") });
    };
    let from = eda_ops::footprint_name_of(part);
    match to {
        Some(to) => json!({ "reference": reference, "from": from, "to": to, "found": crate::library_place::footprint_found(to, reference, design, model, dir_libs), "differs": true }),
        None => json!({ "reference": reference, "from": from, "to": from, "found": crate::library_place::library_copy(&from, design, dir_libs).is_some() }),
    }
}

/// `{ ok, entries: [{ reference, from, to, found }] }` for the body `{ "references": [..], "footprint": "Lib:Name" | null }`.
pub fn exchange_plan(dir: &Path, body: &[u8]) -> Value {
    let Ok(req) = serde_json::from_slice::<Value>(body) else { return json!({ "ok": false, "message": "the request is not JSON" }) };
    let references: Vec<String> = req["references"].as_array().map(|a| a.iter().filter_map(|r| r.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let to = req["footprint"].as_str().map(str::trim).filter(|t| !t.is_empty());
    let (_, design, model) = match board::load(dir) {
        Ok(loaded) => loaded,
        Err(e) => return json!({ "ok": false, "message": board::reasons(&e) }),
    };
    let footprints = crate::library_tables::libs(dir, Kind::Footprint);
    let entries: Vec<Value> = references.iter().map(|r| plan_entry(&*footprints, &design, &model, r, to)).collect();
    json!({ "ok": true, "entries": entries })
}
