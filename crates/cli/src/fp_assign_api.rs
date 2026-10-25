//! The studio's routes for the footprint assignment tools (GAPS.md package P2): what Update PCB from Schematic lists, and, as the steps land, what the exchange,
//! update, compare and reannotate dialogs show before and after they change the board.
//!
//!  * `GET /api/update_pcb`: `common.Control.updatePcbFromSchematic` (F8, `DIALOG_UPDATE_PCB` over `BOARD_NETLIST_UPDATER`): the footprints whose symbol names another
//!    footprint than the board has, each with whether a library has the footprint. The change itself is `Cmd::ExchangeFootprints`, one per part, which the studio sends as
//!    one batch.
//!  * `POST /api/exchange_plan`: `DIALOG_EXCHANGE_FOOTPRINTS::processFootprint`'s look before the change: for each footprint the dialog's choice names, whether the footprint it
//!    is to be put on (Change) or its own library copy (Update) is found and, for an update, whether the board's differs from it (`FootprintNeedsUpdate`).
//!  * `GET /api/footprint_diff?ref=U1`: `BOARD_INSPECTION_TOOL::DiffFootprint`: the board's footprint against its library copy, as the lines of the report and both drawings.
//!  * `POST /api/reannotate_plan`: `DIALOG_BOARD_REANNOTATE::ReannotateBoard`'s plan, before anything is written: the front and back lists in their order, the prefixes, the change log and what is
//!    wrong with it (`eda_model::reannotate`). The change itself is `Cmd::ReannotateBoard`.

use eda_model::units::Nm;
use crate::board;
use crate::library_index::Kind;
use eda_model::Footprint;
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

/// `Library:Name` -> (`Library`, `Name`); a bare name has no library.
fn split_id(id: &str) -> (&str, &str) {
    id.split_once(':').unwrap_or(("", id))
}

/// The entry of `/api/exchange_plan` for one footprint: where it stands before an exchange. `to` is the footprint it is to be put on; `None` is an update, which is
/// from the library copy of the footprint it is on. `found` is whether that footprint is found; for an update `differs` says whether the board's is not the library's
/// (`FootprintNeedsUpdate`) and `lines` how.
fn plan_entry(dir_libs: &dyn crate::library_index::LibSource, design: &eda_model::ir::Design, model: &eda_model::ConstraintModel, reference: &str, to: Option<&str>) -> Value {
    let Some(part) = model.part(reference) else {
        return json!({ "reference": reference, "found": false, "from": "", "to": to.unwrap_or(""), "error": format!("no footprint {reference} on this board") });
    };
    // the footprint the board names (a built-in `0603` is `Resistor_SMD:R_0603_1608Metric` in the files), which an update takes the library copy of
    let from = eda_model::footprint::footprint_lib_id(part);
    match to {
        Some(to) => json!({ "reference": reference, "from": from, "to": to, "found": crate::library_place::footprint_found(to, reference, design, model, dir_libs), "differs": true }),
        None => {
            let copy: Option<Footprint> = crate::library_place::library_copy(&from, design, dir_libs);
            let lines: Vec<String> = match (&copy, eda_ops::board_footprint(design, model, reference)) {
                (Some(library), Some(board)) => eda_model::fp_compare::footprint_differences(&board.facts(), library),
                _ => Vec::new(),
            };
            json!({ "reference": reference, "from": from, "to": from, "found": copy.is_some(), "differs": !lines.is_empty(), "lines": lines })
        }
    }
}

/// `{ ok, entries: [{ reference, from, to, found, differs, lines? }] }` for the body `{ "references": [..], "footprint": "Lib:Name" | null }`.
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

/// A footprint for the Compare dialog's drawing: its pads (position, size, shape, rotation, hole), its graphics as board shapes, its courtyard outlines -- all in the
/// footprint's own frame, micrometres.
fn footprint_drawing(fp: &Footprint) -> Value {
    let pads: Vec<Value> = fp
        .pads
        .iter()
        .map(|p| json!({ "num": p.number, "x": p.at.0, "y": p.at.1, "w": p.size.0, "h": p.size.1, "shape": crate::fp_json::shape_name(p.shape), "rot": p.rot as f64 / 1000.0, "ratio": p.roundrect_ratio, "drill": p.drill, "slot": p.drill_slot.map(|(w, h)| json!([w, h])), "th": p.kind != eda_model::PadKind::Smd }))
        .collect();
    let courtyard: Vec<Vec<[Nm; 2]>> = if fp.courtyard_outlines.is_empty() {
        fp.courtyard.map(|(hw, hh)| vec![vec![[-hw, -hh], [hw, -hh], [hw, hh], [-hw, hh]]]).unwrap_or_default()
    } else {
        fp.courtyard_outlines.iter().map(|o| o.iter().map(|&(x, y)| [x, y]).collect()).collect()
    };
    json!({ "pads": pads, "shapes": fp.art.shapes.iter().map(|s| crate::studio::shape_json(&s.shape)).collect::<Vec<_>>(), "courtyard": courtyard })
}

/// `BOARD_INSPECTION_TOOL::DiffFootprint` for the footprint `reference`: `{ ok, reference, library, item, found, reason?, lines, board, library_copy }`. Where the library or
/// the item is missing `found` is false and `reason` says it in KiCad's words; otherwise `lines` are the differences (`FootprintNeedsUpdate`; empty is "No relevant
/// differences detected.") and `board` and `library_copy` are the two drawings.
pub fn footprint_diff(dir: &Path, reference: &str) -> Value {
    let (_, design, model) = match board::load(dir) {
        Ok(loaded) => loaded,
        Err(e) => return json!({ "ok": false, "message": board::reasons(&e) }),
    };
    let Some(part) = model.part(reference) else { return json!({ "ok": false, "message": format!("no footprint {reference} on this board") }) };
    let Some(board_fp) = eda_ops::board_footprint(&design, &model, reference) else { return json!({ "ok": false, "message": format!("{reference} has no footprint") }) };
    let name = eda_model::footprint::footprint_lib_id(part);
    let (library, item) = split_id(&name);
    let footprints = crate::library_tables::libs(dir, Kind::Footprint);
    let base = json!({ "ok": true, "reference": reference, "library": library, "item": item });
    let mut out = base.as_object().cloned().unwrap_or_default();
    let in_tables = !library.is_empty() && crate::library_index::LibSource::lib_path(&*footprints, Kind::Footprint, library).is_ok();
    let edited = design.footprint_library.as_ref().is_some_and(|l| l.by_name(&name).is_some());
    if !in_tables && !edited {
        out.insert("found".into(), json!(false));
        out.insert("reason".into(), json!("The library is not included in the current configuration."));
        return Value::Object(out);
    }
    match crate::library_place::library_copy(&name, &design, &*footprints) {
        None => {
            out.insert("found".into(), json!(false));
            out.insert("reason".into(), json!(format!("The library no longer contains the item {item}.")));
        }
        Some(copy) => {
            out.insert("found".into(), json!(true));
            out.insert("lines".into(), json!(eda_model::fp_compare::footprint_differences(&board_fp.facts(), &copy)));
            out.insert("board".into(), footprint_drawing(&board_fp.footprint));
            out.insert("library_copy".into(), footprint_drawing(&copy));
        }
    }
    Value::Object(out)
}

/// `{ ok, valid, plan }` for the body `{ "options": {..}, "selection": [..] }` (the fields of `Cmd::ReannotateBoard`): what the dialog's run works out for the board as it is -- the lists
/// it logs, the change log, the references that are empty or have no number and the duplicates. `valid` is false when two footprints would be left with one reference. Nothing is changed.
pub fn reannotate_plan(dir: &Path, body: &[u8]) -> Value {
    let Ok(req) = serde_json::from_slice::<Value>(body) else { return json!({ "ok": false, "message": "the request is not JSON" }) };
    let options: eda_model::reannotate::ReannotateOptions = match req.get("options") {
        Some(v) if !v.is_null() => match serde_json::from_value(v.clone()) {
            Ok(o) => o,
            Err(e) => return json!({ "ok": false, "message": format!("bad options: {e}") }),
        },
        _ => Default::default(),
    };
    let selection: Vec<String> = req.get("selection").and_then(Value::as_array).map(|a| a.iter().filter_map(|r| r.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let (meta, design, model) = match board::load(dir) {
        Ok(loaded) => loaded,
        Err(e) => return json!({ "ok": false, "message": board::reasons(&e) }),
    };
    let plan = eda_ops::Board::new(design, &model, Nm::from_raw_shim(meta.snap_nm), Nm::from_raw_shim(meta.spacing_nm)).reannotate_plan(&options, &selection);
    json!({ "ok": true, "valid": plan.ok(), "plan": plan })
}
