//! `/api/sch/*` reads for the schematic-control actions (`eeschema.NavigateTool.*`, `eeschema.EditorControl.*`):
//! what the studio cannot work out from the one sheet it is showing.
//!
//! None of these runs kicad-cli (the ones that do live in `sch_export_api.rs`, behind the kicad-cli lane), and none
//! edits `design.json`: every edit is an `/api/cmd` verb.

use crate::fab_api::err;
use crate::kicad_engine;
use eda_model::ir::Design;
use serde_json::{json, Value};
use std::path::Path;

/// `GET /api/sch/hierarchy`: `{ "ok": true, "sheets": [{ "path": ["id", ...], "name", "file", "page" }] }`, the root first, in the order
/// `SCH_SHEET_LIST::BuildSheetList` walks them (`eda_model::sheet_list::walk`: depth first, a screen's sheets by place); `page` is the page each
/// sheet path shows (`SCH_SHEET_PATH::GetPageNumber`: the one stored for it, else its place in the walk, the root being 1), the one the derived
/// `.kicad_sch` gives it. The studio sorts it by page for Next / Previous Sheet and sorts each sheet's children by page in the Hierarchy Navigator.
///
/// `virtual_root` says the root is KiCad 10's virtual root (`SchExtras::virtual_root`: a project of several top-level sheets), whose sheets the navigator shows
/// as roots (`HIERARCHY_PANE::UpdateHierarchyTree`: one tree item per `GetTopLevelSheets()` entry under the hidden project row). With `net` (the
/// `?net=` of the request, the net Highlight Net lit), each sheet says whether its screen has that net (`HIERARCHY_PANE::UpdateNetHighlight`:
/// `ConnectionGraph()->GetAllSubgraphs( net )`'s sheets): a wire, a label or a power symbol of it. **Differs**: a screen shown on several paths keeps one name
/// per wire (its first path's), so the other paths' rows of a local net are not lit.
///
/// `screens` are the files of every screen the project keeps (`design.sheet_contents`' keys), placed or not: a file a deleted sheet showed stays in the
/// project, as a file on disk stays beside a KiCad project, and `SCH_EDIT_FRAME::ChangeSheetFile` asks before a new sheet is linked to it
/// (`loadFromFile = wxFileExists( newAbsoluteFilename )`, UI-LEFT-2 SS15).
pub fn hierarchy(dir: &Path, net: &str) -> Value {
    match kicad_engine::load_with_schematic(dir) {
        Ok((design, _)) => {
            let has_net = |file: &str| -> bool {
                let screen = if file.is_empty() { design.schematic.as_ref() } else { design.sheet_contents.as_ref().and_then(|c| c.get(file)) };
                screen.is_some_and(|sch| sch.wires.iter().any(|w| w.net == net) || sch.labels.iter().any(|l| l.net == net) || sch.power_symbols.iter().any(|p| p.net == net))
            };
            json!({
                "ok": true,
                "virtual_root": design.schematic.as_ref().is_some_and(|s| s.extras.virtual_root),
                "screens": design.sheet_contents.as_ref().map(|c| c.keys().cloned().collect::<Vec<_>>()).unwrap_or_default(),
                "sheets": eda_model::sheet_list::walk(&design).into_iter().map(|h| {
                    let page = h.effective_page();
                    let mut v = json!({ "path": h.path, "name": h.name, "file": h.file, "page": page });
                    if !net.is_empty() {
                        v["net"] = json!(has_net(&h.file));
                    }
                    v
                }).collect::<Vec<_>>(),
            })
        }
        Err(e) => err(crate::board::reasons(&e)),
    }
}

/// `GET /api/sch/symbols`: every placed symbol as each sheet path shows it (`SCH_SHEET_LIST::GetSymbols`: one `SCH_REFERENCE` per symbol and path), in walk
/// order: `{ "ok": true, "symbols": [{ "sheet": "<path>", "file", "id", "unit", "reference", "path_unit", "lib_id", "value" }] }`. `id` and `unit` tell the
/// symbol apart on its screen (`file`, the same on every path that shows it); `reference` and `path_unit` are the path's own (`GetRef( &path )`,
/// `GetUnitSelection( &path )`). Change Symbols and Update Symbols from Library match over these (`DIALOG_CHANGE_SYMBOLS::processMatchingSymbols` walks
/// `Hierarchy()` and `isMatch` reads `GetRef( aInstance )`; UI-LEFT S9).
pub fn symbol_rows(dir: &Path) -> Value {
    match kicad_engine::load_with_schematic(dir) {
        Ok((design, model)) => {
            let mut rows: Vec<Value> = Vec::new();
            for e in eda_model::sheet_list::walk(&design) {
                let Some(sch) = eda_model::sheet_list::screen_of(&design, &e.path) else { continue };
                for s in &sch.symbols {
                    let value = if s.value.is_empty() { model.part(&s.id).and_then(|p| p.value.clone()).unwrap_or_default() } else { s.value.clone() };
                    rows.push(json!({
                        "sheet": e.path.join("/"),
                        "file": e.file,
                        "id": s.id,
                        "unit": s.unit,
                        "reference": eda_model::sheet_list::reference_under(sch, s, &e.path),
                        "path_unit": eda_model::sheet_list::unit_under(sch, s, &e.path),
                        "lib_id": s.lib_id,
                        "value": value,
                    }));
                }
            }
            json!({ "ok": true, "symbols": rows })
        }
        Err(e) => err(crate::board::reasons(&e)),
    }
}

/// The library symbols the schematic uses, each `lib_id` once and in `lib_id` order (`libSymbols` of `ExportSymbolsToLibrary` is a `std::map<LIB_ID, ...>`):
/// the symbols of every sheet screen, and the power symbols when `include_power` is on (`SYMBOL_FILTER_ALL` against `SYMBOL_FILTER_NON_POWER`). A symbol with no
/// `lib_id` of its own is drawn as a plain box and has no library symbol to export: its reference comes back in the second list.
pub(crate) fn used_lib_ids(design: &Design, include_power: bool) -> (Vec<String>, Vec<String>) {
    let mut ids = std::collections::BTreeSet::new();
    let mut boxes = std::collections::BTreeSet::new();
    let screens = design.schematic.iter().chain(design.sheet_contents.iter().flat_map(|c| c.values()));
    for sch in screens {
        for s in &sch.symbols {
            if s.lib_id.is_empty() {
                boxes.insert(s.id.clone());
            } else {
                ids.insert(s.lib_id.clone());
            }
        }
        if include_power {
            ids.extend(sch.power_symbols.iter().map(|p| p.lib_id.clone()).filter(|l| !l.is_empty()));
        }
    }
    (ids.into_iter().collect(), boxes.into_iter().collect())
}

/// Two library symbols with one name cannot share a library file: the later one (in `lib_id` order) replaces the earlier, as `SaveSymbol` does when the
/// library already has the name. Returns what is kept and a line for each one replaced.
pub(crate) fn unique_by_name(found: Vec<(String, eda_model::ir::LibrarySymbol)>) -> (Vec<(String, eda_model::ir::LibrarySymbol)>, Vec<String>) {
    let mut kept: Vec<(String, eda_model::ir::LibrarySymbol)> = Vec::new();
    let mut clashes = Vec::new();
    for (id, sym) in found {
        let name = id.rsplit(':').next().unwrap_or(&id).to_string();
        match kept.iter().position(|(k, _)| k.rsplit(':').next().unwrap_or(k) == name) {
            Some(at) => {
                clashes.push(format!("{} and {id} are both named {name}: only {id} is exported", kept[at].0));
                kept[at] = (id, sym);
            }
            None => kept.push((id, sym)),
        }
    }
    (kept, clashes)
}

/// `POST /api/sch/export_symbols`: `{include_power?: bool}` -- what `SCH_EDITOR_CONTROL::ExportSymbolsToLibrary` writes into the library it asks for: every library symbol
/// the schematic uses, once, as one `.kicad_sym` (the studio has no library table to save into, so the browser saves the file). `{ok, text, ids, skipped, clashes}`.
pub fn export_symbols(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let include_power = req.get("include_power").and_then(Value::as_bool).unwrap_or(false);
    let (_, design, model) = match crate::board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(crate::board::reasons(&e)),
    };
    let (ids, boxes) = used_lib_ids(&design, include_power);
    let mut found = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    if !boxes.is_empty() {
        let shown: Vec<&str> = boxes.iter().take(8).map(String::as_str).collect();
        let more = if boxes.len() > shown.len() { format!(" and {} more", boxes.len() - shown.len()) } else { String::new() };
        skipped.push(format!("{} drawn as plain boxes, with no library symbol: {}{more}", if boxes.len() == 1 { "1 symbol is".to_string() } else { format!("{} symbols are", boxes.len()) }, shown.join(", ")));
    }
    let libs = crate::library_tables::libs(dir, crate::library_index::Kind::Symbol);
    for id in ids {
        match crate::library_api::symbol_in_libs(&*libs, &design, &model, &id) {
            Some((sym, _)) => {
                // the library the schematic's symbols are written into holds them flat, as KiCad's `ExportSymbolsToLibrary` writes the cached ones
                let flat = design.symbol_library.as_ref().filter(|l| l.is_derived(&id)).and_then(|l| l.flatten(&id).ok()).unwrap_or(sym);
                found.push((id, flat))
            }
            None => skipped.push(format!("{id}: no symbol with this lib_id in the project library or anything it loads")),
        }
    }
    let (kept, clashes) = unique_by_name(found);
    if kept.is_empty() {
        return err("This schematic uses no library symbols to export.");
    }
    let syms: Vec<&eda_model::ir::LibrarySymbol> = kept.iter().map(|(_, s)| s).collect();
    json!({ "ok": true, "text": eda_kicad::export_kicad_sym_library(&syms), "ids": kept.iter().map(|(id, _)| id).collect::<Vec<_>>(), "skipped": skipped, "clashes": clashes })
}

#[cfg(test)]
mod tests {
    use eda_model::units::{mm, Nm};
    use super::*;
    use eda_model::ir::{Point, Provenance, SchematicSection, SheetInstance};
    use std::collections::BTreeMap;

    fn sch(sheets: Vec<SheetInstance>) -> SchematicSection {
        SchematicSection {
            symbols: vec![],
            wires: vec![],
            labels: vec![],
            texts: vec![],
            power_symbols: vec![],
            no_connects: vec![],
            bus_entries: vec![],
            erc_exclusions: vec![],
            erc_pin_map: None,
            user_fields: Default::default(),
            field_layout: Default::default(),
            title_block: None,
            sheets,
            instance_overrides: vec![],
            junctions: vec![],
            lines: vec![],
            imported_from_kicad: false,
            extras: Default::default(),
        }
    }

    fn placement(id: &str, name: &str, file: &str, page: &str) -> SheetInstance {
        SheetInstance { id: id.into(), name: name.into(), file: file.into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, size: (mm(1.0), mm(1.0)), pins: vec![], page: page.into() }
    }

    fn design(root: SchematicSection, screens: BTreeMap<String, SchematicSection>) -> Design {
        Design {
            schema: eda_model::ir::SCHEMA_VERSION,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: Some(root),
            nets: None,
            placement: None,
            routing: None,
            drawings: None,
            footprint_library: None,
            sheet_contents: (!screens.is_empty()).then_some(screens),
            bus_aliases: vec![],
            symbol_library: None,
        }
    }

    #[test]
    fn a_flat_design_has_only_the_root() {
        let h = eda_model::sheet_list::walk(&design(sch(vec![]), BTreeMap::new()));
        assert_eq!(h.len(), 1);
        assert!(h[0].path.is_empty());
    }

    #[test]
    fn sheets_come_depth_first_with_their_paths_and_pages() {
        let mut screens = BTreeMap::new();
        screens.insert("a.kicad_sch".to_string(), sch(vec![placement("a1", "Deep", "c.kicad_sch", "")]));
        screens.insert("b.kicad_sch".to_string(), sch(vec![]));
        screens.insert("c.kicad_sch".to_string(), sch(vec![]));
        let d = design(sch(vec![placement("sa", "A", "a.kicad_sch", "7"), placement("sb", "B", "b.kicad_sch", "")]), screens);
        let h = eda_model::sheet_list::walk(&d);
        // the page each path shows, which `GET /api/sch/hierarchy` sends: the one set, else the place in the walk
        let seen: Vec<(Vec<&str>, &str, String)> = h.iter().map(|e| (e.path.iter().map(String::as_str).collect(), e.name.as_str(), e.effective_page())).collect();
        assert_eq!(seen, vec![(vec![], "", "1".into()), (vec!["sa"], "A", "7".into()), (vec!["sa", "a1"], "Deep", "3".into()), (vec!["sb"], "B", "4".into())]);
    }

    #[test]
    fn a_placement_without_a_screen_and_a_sheet_that_places_itself_do_not_break_the_walk() {
        let mut screens = BTreeMap::new();
        screens.insert("loop.kicad_sch".to_string(), sch(vec![placement("again", "Loop", "loop.kicad_sch", "")]));
        let d = design(sch(vec![placement("gone", "Gone", "missing.kicad_sch", ""), placement("lp", "Loop", "loop.kicad_sch", "")]), screens);
        let h = eda_model::sheet_list::walk(&d);
        // KiCad gives a sheet with no file an empty screen and lists it (r3 B3-M4); a file that places itself is listed once (r3 B3-M6)
        let paths: Vec<String> = h.iter().map(|e| e.path_text()).collect();
        assert_eq!(paths, vec!["", "gone", "lp"]);
    }

    fn placed(id: &str, lib_id: &str) -> eda_model::ir::SymbolInstance {
        eda_model::ir::SymbolInstance { id: id.into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, rot: 0, mirrored: false, mirror_y: false, lib_id: lib_id.into(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false }
    }

    fn power(id: &str, lib_id: &str) -> eda_model::ir::PowerSymbol {
        eda_model::ir::PowerSymbol { id: id.into(), lib_id: lib_id.into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, rot: 0, net: "GND".into(), pin: String::new() }
    }

    #[test]
    fn the_symbols_to_export_are_each_lib_id_once_from_every_sheet_with_power_only_when_asked() {
        let mut root = sch(vec![placement("sa", "A", "a.kicad_sch", "")]);
        root.symbols = vec![placed("R1", "Device:R"), placed("R2", "Device:R"), placed("U1", "MCU:STM32"), placed("X1", "")];
        root.power_symbols = vec![power("#PWR01", "power:GND")];
        let mut sub = sch(vec![]);
        sub.symbols = vec![placed("C1", "Device:C"), placed("R3", "Device:R")];
        let d = design(root, [("a.kicad_sch".to_string(), sub)].into_iter().collect());
        let (ids, boxes) = used_lib_ids(&d, false);
        assert_eq!(ids, vec!["Device:C", "Device:R", "MCU:STM32"], "in lib_id order, no power symbol, none twice");
        assert_eq!(boxes, vec!["X1"], "a symbol with no lib_id is reported, not exported");
        assert_eq!(used_lib_ids(&d, true).0, vec!["Device:C", "Device:R", "MCU:STM32", "power:GND"]);
    }

    #[test]
    fn two_symbols_of_one_name_keep_the_later_and_say_so() {
        let sym = |id: &str| eda_model::ir::LibrarySymbol {
            lib_id: id.into(),
            reference_prefix: String::new(),
            description: String::new(),
            keywords: String::new(),
            datasheet: String::new(),
            power: false,
            in_bom: true,
            on_board: true,
            pin_numbers_hidden: false,
            pin_names_hidden: false,
            pin_name_offset_mm: 0.508,
            unit_count: 1,
            has_alternate_body_style: false,
            body_style_names: Vec::new(),
            footprint_filters: vec![],
            graphics: vec![],
            pins: vec![],
            published: false,
            extends: None,
            fields: vec![],
            units_locked: false,
            power_local: false,
            exclude_from_sim: false,
            exclude_from_pos_files: false,
            kicad: Default::default(),
        };
        let (kept, clashes) = unique_by_name(vec![("A:R".into(), sym("A:R")), ("B:C".into(), sym("B:C")), ("C:R".into(), sym("C:R"))]);
        assert_eq!(kept.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), vec!["C:R", "B:C"]);
        assert_eq!(clashes, vec!["A:R and C:R are both named R: only C:R is exported"]);
    }
}
