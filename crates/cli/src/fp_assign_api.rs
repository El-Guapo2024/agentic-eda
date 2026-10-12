//! The studio's routes for the footprint assignment tools (GAPS.md package P2): what Update PCB from Schematic lists, and, as the steps land, what the exchange,
//! update, compare and reannotate dialogs show before and after they change the board.
//!
//!  * `GET /api/update_pcb`: `common.Control.updatePcbFromSchematic` (F8, `DIALOG_UPDATE_PCB` over `BOARD_NETLIST_UPDATER`): the footprints whose symbol names another
//!    footprint than the board has, each with whether a library has the footprint. The change itself is `Cmd::ExchangeFootprints`, one per part, which the studio sends as
//!    one batch.

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
