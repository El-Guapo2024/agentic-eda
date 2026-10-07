//! The studio's board-control routes that are not a kicad-cli export (`crate::board_output_api` is those):
//!  * `POST /api/repair_board`: `pcbnew.Control.repairBoard` (`BOARD_EDITOR_CONTROL::RepairBoard`) -- the repair is
//!    `Cmd::RepairBoard` (one undo step); this route also hands back KiCad's report lines.
//!  * `GET /api/footprint_associations?ref=U1`: `pcbnew.InspectionTool.ShowFootprintAssociations`
//!    (`DIALOG_FOOTPRINT_ASSOCIATIONS`): the footprint's library link and the schematic symbol it belongs to.
//!  * `POST /api/fab/cmp`: `pcbnew.EditorControl.exportFootprintAssociations` (`RecreateCmpFile`): the `.cmp`
//!    footprint association file, a plain text listing kicad-cli has no command for.

use crate::{board, kicad_engine};
use eda_ops::Cmd;
use serde_json::{json, Value};
use std::path::Path;

/// `RepairBoard`: what it found and fixed, as `{ ok, repaired, message, details }` -- "%d potential problems
/// repaired." with the detail lines, or "No board problems found." (`DisplayInfoMessage`'s two texts).
pub fn repair_board(dir: &Path) -> Value {
    let (_, mut design, model) = match board::load(dir) {
        Ok(loaded) => loaded,
        Err(e) => return json!({ "ok": false, "message": board::reasons(&e) }),
    };
    // On a copy, to learn what would be repaired; `Cmd::RepairBoard` then does exactly that, once, as an undoable step.
    let report = eda_ops::board_control::repair_board(&mut design, &model.nets);
    if report.repaired == 0 {
        return json!({ "ok": true, "repaired": 0, "message": "No board problems found.", "details": [] });
    }
    match board::step(dir, Cmd::RepairBoard, false, "ui") {
        Ok(_) => json!({ "ok": true, "repaired": report.repaired, "message": format!("{} potential problems repaired.", report.repaired), "details": report.details }),
        Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
    }
}

/// `DIALOG_FOOTPRINT_ASSOCIATIONS::TransferDataToWindow`'s two grids as data: the library link (`Library:` and
/// `Footprint:` rows, with the descriptions the project library knows) and the symbol the footprint belongs to (its
/// sheet and the symbol: reference, library symbol and value).
pub fn footprint_associations(dir: &Path, reference: &str) -> Value {
    let (design, model) = match kicad_engine::load_with_schematic(dir) {
        Ok(loaded) => loaded,
        Err(e) => return json!({ "ok": false, "message": board::reasons(&e) }),
    };
    let Some(part) = model.part(reference) else {
        return json!({ "ok": false, "message": format!("no footprint {reference} on this board") });
    };
    // `LIB_ID`: "Library:Item", or an item with no library.
    let full = part.footprint.clone().or_else(|| model.footprint_of(part).map(|f| f.name)).or_else(|| part.package.clone()).unwrap_or_default();
    let (library, item) = full.split_once(':').map_or((String::new(), full.clone()), |(l, i)| (l.to_string(), i.to_string()));
    let fp_description = design.footprint_library.as_ref().and_then(|l| l.by_name(&full).or_else(|| l.by_name(&item))).map(|f| f.description.clone()).unwrap_or_default();

    // The sheet the symbol is on: the root, or the screen of a sub-sheet that has it.
    let symbol = design
        .schematic
        .iter()
        .map(|s| ("/".to_string(), s))
        .chain(design.sheet_contents.iter().flatten().map(|(name, s)| (format!("/{name}"), s)))
        .find_map(|(sheet, s)| s.symbols.iter().find(|y| y.id == reference).map(|y| (sheet, y)));
    // A symbol with no value of its own shows its part's (the schematic export does the same).
    let symbol = symbol.map(|(sheet, y)| json!({ "sheet": sheet, "reference": y.id, "lib_id": y.lib_id, "value": if y.value.is_empty() { part.value.clone().unwrap_or_default() } else { y.value.clone() } }));
    json!({
        "ok": true,
        "reference": reference,
        "library": library,
        "library_description": "",
        "footprint": item,
        "footprint_description": fp_description,
        "symbol": symbol,
    })
}

/// `RecreateCmpFile`'s text for these footprints: `(timestamp, path, reference, value, id)` each.
pub(crate) fn cmp_text(date: &str, footprints: &[(String, String, String, String, String)]) -> String {
    let mut out = format!("Cmp-Mod V01 Created by PcbNew   date = {date}\n");
    for (uuid, path, reference, value, id) in footprints {
        out.push_str("\nBeginCmp\n");
        out.push_str(&format!("TimeStamp = {uuid}\n"));
        out.push_str(&format!("Path = {path}\n"));
        out.push_str(&format!("Reference = {};\n", if reference.is_empty() { "[NoRef]" } else { reference }));
        out.push_str(&format!("ValeurCmp = {};\n", if value.is_empty() { "[NoVal]" } else { value }));
        out.push_str(&format!("IdModule  = {id};\n"));
        out.push_str("EndCmp\n");
    }
    out.push_str("\nEndListe\n");
    out
}

/// `POST /api/fab/cmp`: write `export/kicad/cmp/<project>.cmp`. The timestamps are the footprints' uuids in the
/// derived `.kicad_pcb` (the ones kicad-cli and KiCad see), the path the schematic symbol's uuid in the derived
/// `.kicad_sch`.
pub fn export_cmp(dir: &Path) -> Value {
    let (design, model) = match kicad_engine::load_with_schematic(dir) {
        Ok(loaded) => loaded,
        Err(e) => return crate::fab_api::err(board::reasons(&e)),
    };
    let Some(placement) = design.placement.as_ref() else {
        return crate::fab_api::err("the board has no placement");
    };
    let date = kicad_engine::chrono_like_today();
    let meta = eda_kicad::ExportMeta { date: &date, title: "board" };
    let (_, pcb_ids) = match eda_kicad::export_kicad_pcb_mapped(&design, &model, &meta) {
        Ok(r) => r,
        Err(e) => return crate::fab_api::err(board::reasons(&e)),
    };
    let sch_ids = eda_kicad::export_kicad_sch_mapped(&design, &model, &meta).map(|(_, m)| m).unwrap_or_default();
    let uuid_of = |map: &std::collections::HashMap<String, String>, id: &str| map.iter().find(|(_, ours)| ours.as_str() == id).map(|(u, _)| u.clone()).unwrap_or_default();

    let mut rows: Vec<(String, String, String, String, String)> = Vec::new();
    for fp in &placement.footprints {
        let Some(part) = model.part(&fp.id) else { continue };
        let full = part.footprint.clone().or_else(|| model.footprint_of(part).map(|f| f.name)).unwrap_or_default();
        // The exporter's own lib id for it: a footprint with no library is `eda:<name>`.
        let id = if full.contains(':') || full.is_empty() { full } else { format!("eda:{full}") };
        let symbol = uuid_of(&sch_ids, &fp.id);
        rows.push((uuid_of(&pcb_ids, &fp.id), if symbol.is_empty() { String::new() } else { format!("/{symbol}") }, fp.id.clone(), part.value.clone().unwrap_or_default(), id));
    }
    let text = cmp_text(&eda::now_rfc3339(), &rows);
    let name = std::fs::read_to_string(dir.join("board.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|m| m["intent"].as_str().and_then(|i| Path::new(i).file_stem().and_then(|s| s.to_str()).map(str::to_string)))
        .unwrap_or_else(|| "board".to_string());
    let out_dir = dir.join("export").join("kicad").join("cmp");
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        return crate::fab_api::err(format!("cannot create {}: {e}", out_dir.display()));
    }
    let file = out_dir.join(format!("{name}.cmp"));
    if let Err(e) = std::fs::write(&file, text) {
        return crate::fab_api::err(format!("cannot write {}: {e}", file.display()));
    }
    json!({ "ok": true, "message": format!("{} footprint(s) written.", rows.len()), "files": [format!("export/kicad/cmp/{name}.cmp")] })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cmp_file_is_recreate_cmp_files_text() {
        let rows = vec![
            ("11111111-2222-4333-8444-555555555555".to_string(), "/aaaa".to_string(), "R1".to_string(), "10k".to_string(), "Resistor_SMD:R_0402".to_string()),
            (String::new(), String::new(), String::new(), String::new(), "eda:X".to_string()),
        ];
        let text = cmp_text("2026-10-04T12:00:00-0700", &rows);
        assert_eq!(
            text,
            "Cmp-Mod V01 Created by PcbNew   date = 2026-10-04T12:00:00-0700\n\
             \n\
             BeginCmp\nTimeStamp = 11111111-2222-4333-8444-555555555555\nPath = /aaaa\nReference = R1;\nValeurCmp = 10k;\nIdModule  = Resistor_SMD:R_0402;\nEndCmp\n\
             \n\
             BeginCmp\nTimeStamp = \nPath = \nReference = [NoRef];\nValeurCmp = [NoVal];\nIdModule  = eda:X;\nEndCmp\n\
             \n\
             EndListe\n"
        );
    }
}
