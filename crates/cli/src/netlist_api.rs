//! File > Import > Netlist... on the server (`DIALOG_IMPORT_NETLIST`; the updater is `eda_ops::netlist_update`):
//!
//!  * `POST /api/import_netlist`: Load and Test Netlist (`loadNetlist( true )`) -- the body `{ text, name, options }`, the report of what Update PCB would do,
//!    with nothing changed: `{ ok, lines: [{ severity, text }], added, warnings, errors, error? }`;
//!  * Update PCB is `/api/cmd` with `{ "op": "import_netlist", text, name, options, at }`, one undo step; its reply carries the same report as `report`.
//!
//! Either way the command is given what the libraries have of the footprints the netlist names ([`filled`]), as the exchange and Place Footprint are: a
//! command replays and undoes the same on a machine without the libraries.

use crate::board;
use crate::library_index::{Kind, LibSource};
use eda_ops::netlist_update::ImportNetlist;
use eda_model::ConstraintModel;
use serde_json::{json, Value};
use std::path::Path;

/// The root sheet's file of the schematic written beside the board (`kicad_engine`'s `board.kicad_sch`): a footprint on the root sheet has it as its
/// `GetSheetfile`.
const ROOT_SHEETFILE: &str = "board.kicad_sch";

/// `req` with the library footprints the netlist names: each one's library copy (`m_frame->LoadFootprint( aFootprintId )`), the `Lib:Name` a bare (legacy)
/// name is found under (`LoadFootprintWithOptionalNickname`: the libraries in alphabetical order), whether there is a footprint library at all, and the root
/// sheet's file. A name the libraries do not have is left out: the updater says so in KiCad's words. A file that cannot be read is left to the updater too.
pub(crate) fn filled<F: LibSource + ?Sized>(req: &ImportNetlist, model: &ConstraintModel, footprints: &F) -> ImportNetlist {
    let mut out = req.clone();
    out.root_sheetfile = ROOT_SHEETFILE.to_string();
    let listed = footprints.listed(Kind::Footprint);
    out.libraries = !listed.is_empty();
    let Ok(netlist) = eda_kicad::netlist_reader::read_netlist(&req.text, &req.name) else { return out };
    let mut names: Vec<String> = netlist.components.iter().map(|c| c.fpid.clone()).filter(|f| !f.is_empty()).collect();
    names.sort();
    names.dedup();
    let mut libraries: Vec<String> = listed.iter().map(|l| l.name.clone()).collect();
    libraries.sort();
    for name in names {
        let full = if name.contains(':') {
            name.clone()
        } else {
            let Some(lib) = libraries.iter().find(|lib| footprints.lib_path(Kind::Footprint, lib).is_ok_and(|p| p.join(format!("{name}.kicad_mod")).is_file())) else { continue };
            let full = format!("{lib}:{name}");
            out.found_as.insert(name.clone(), full.clone());
            full
        };
        if out.definitions.contains_key(&full) || out.library.contains_key(&full) {
            continue;
        }
        let Some(engine) = crate::library_search::installed_engine_footprint(footprints, &full) else { continue };
        if let Some(def) = crate::library_search::installed_footprint(footprints, &full) {
            // the library footprint as the Footprint Editor reads it: an exchange gives the board's footprint its record (UI-LEFT PP5)
            out.library_items.insert(full.clone(), def.clone());
            if !model.footprints.iter().any(|f| f.name == full) {
                out.definitions.insert(full.clone(), def);
            }
        }
        out.library.insert(full, engine);
    }
    out
}

/// `POST /api/import_netlist`: Load and Test Netlist.
pub(crate) fn test(dir: &Path, body: &[u8]) -> Value {
    let Ok(req) = serde_json::from_slice::<ImportNetlist>(body) else { return json!({ "ok": false, "message": "the request is not a netlist import (`{ text, name, options }`)" }) };
    let (meta, design, model) = match board::load(dir) {
        Ok(loaded) => loaded,
        Err(e) => return json!({ "ok": false, "message": board::reasons(&e) }),
    };
    let req = filled(&req, &model, &*crate::library_tables::libs(dir, Kind::Footprint));
    let report = eda_ops::netlist_update::test_netlist(design, &model, meta.snap_nm, meta.spacing_nm, &req);
    report_json(&report)
}

/// The report as the studio reads it.
pub(crate) fn report_json(report: &eda_ops::netlist_update::NetlistReport) -> Value {
    let mut v = serde_json::to_value(report).unwrap_or(Value::Null);
    v["ok"] = json!(report.error.is_none());
    v
}

