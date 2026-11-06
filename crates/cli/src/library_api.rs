//! `/api/symbol_library/parse` and `/api/footprint/parse`: the read-only half of the library editors'
//! Import and Paste actions (`web/studio/src/state/symbolEditorStore.tsx`, `footprintEditorStore.tsx`).
//!
//! The browser reads the file (or the clipboard text), posts it here, and gets back the editable
//! library entry (`LibrarySymbol` / `LibraryFootprint`, exactly as `design.json` stores them) plus a
//! list of anything the entry cannot hold. What happens next -- picking a free name, storing it -- is a
//! `Cmd::PutLibrarySymbol` / `Cmd::PutLibraryFootprint` through `/api/cmd`, with undo, like every other
//! edit; these two routes never touch `design.json`.

use crate::board;
use crate::library_index::{Kind, LibSource};
use eda_model::ir::{LibraryFootprint, LibrarySymbol};
use eda_model::{CheckResult, ConstraintModel};
use serde_json::{json, Value};
use std::path::Path;

/// A library nickname or item name is part of a file path under the library root: nothing in it may lead out of that root.
fn path_safe(part: &str) -> bool {
    !part.is_empty() && !part.starts_with('.') && !part.contains(['/', '\\']) && !part.contains("..")
}

/// A footprint of the libraries of the library tables (`Lib:Name` -> the folder the row nicknamed `Lib` names, `/Name.kicad_mod`) in the editable type, with
/// every graphic, text, attribute and pad layer the file holds (`eda_kicad::parse_library_footprint`, the reader behind Import -- the engine's own loader
/// keeps only pads and a courtyard) and named `Lib:Name`, the key the library tree shows it under. `Ok(None)` when the libraries have no footprint by this
/// name; `Err` (why) when its file is there and cannot be read or parsed (`library_search::read_installed_footprint`).
fn installed_footprint_in<S: LibSource + ?Sized>(src: &S, name: &str) -> Result<Option<LibraryFootprint>, String> {
    crate::library_search::read_installed_footprint(src, name)
}

/// A symbol of the libraries of the library tables (`Lib:Name` in the file the row nicknamed `Lib` names) in the editable type: units, body styles, keywords
/// and footprint filters as the file has them. Only that symbol (and the ones it extends) is cut out of the file and parsed -- a library of hundreds of
/// symbols is scanned once for the chooser and kept (`library_search::symbol_lib`), never parsed whole to open one of them.
fn installed_symbol_in<S: LibSource + ?Sized>(src: &S, lib_id: &str) -> Option<LibrarySymbol> {
    read_installed_symbol_in(src, lib_id).ok().flatten()
}

/// [`installed_symbol_in`] that tells a symbol the libraries do not have (`Ok(None)`) from one whose library or text cannot be read (`Err`, why:
/// `library_search::read_installed_symbol`).
fn read_installed_symbol_in<S: LibSource + ?Sized>(src: &S, lib_id: &str) -> Result<Option<LibrarySymbol>, String> {
    match lib_id.split_once(':') {
        Some((_, name)) if path_safe(name) => Ok(crate::library_search::read_installed_symbol(src, lib_id)?.map(|(sym, _)| sym)),
        _ => Ok(None),
    }
}

/// A footprint by name from the project library, else KiCad's installed libraries, else whatever the model resolves it to (intent, a loaded
/// `.kicad_mod`, the builtin table) -- for a *read-only* answer; opening one in the editor copies it into the project library first. A library's file
/// that is there and cannot be read is an error that says why, not "no footprint with this name", for which the editor would start a blank footprint.
pub(crate) fn footprint_any(dir: &Path, name: &str) -> Result<(LibraryFootprint, bool), Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    // a footprint loaded from the board (UI-LEFT F3) is written under its own name (`GetFPID()`), not the key the editor holds it by
    if let Some(copy) = design.footprint_library.as_ref().and_then(|l| l.board_copy(name)) {
        return Ok((LibraryFootprint { name: copy.fpid.clone(), ..copy.footprint.clone() }, true));
    }
    if let Some(fp) = design.footprint_library.as_ref().and_then(|l| l.by_name(name)) {
        return Ok((fp.clone(), true));
    }
    // The libraries of the library tables come before what the board's own model resolved the name to: that is the pad-only copy of the same file.
    match installed_footprint_in(&*crate::library_tables::libs(dir, Kind::Footprint), name) {
        Ok(Some(fp)) => return Ok((fp, false)),
        Ok(None) => {}
        Err(why) => return Err(vec![CheckResult::fail("ops_bad_footprint", name, why)]),
    }
    resolve_footprint(&model, name)
        .map(|fp| {
            let mut lib_fp = LibraryFootprint::from_engine_footprint(&fp);
            lib_fp.assign_missing_ids();
            (lib_fp, false)
        })
        .ok_or_else(|| vec![CheckResult::fail("ops_unknown_footprint", name, "no footprint with this name in the project library or anything it loads")])
}

fn resolve_footprint(model: &ConstraintModel, name: &str) -> Option<eda_model::Footprint> {
    if let Some(fp) = model.footprints.iter().find(|f| f.name == name) {
        return Some(fp.clone());
    }
    let norm = eda_model::footprint::normalize_name(name);
    if let Some(fp) = model.footprints.iter().find(|f| eda_model::footprint::normalize_name(&f.name) == norm) {
        return Some(fp.clone());
    }
    eda_model::footprint::builtin(name)
}

/// A symbol by `lib_id` from the project library, else KiCad's installed libraries, else the resolved one (the board's model, the builtin table). A library
/// symbol whose library or text is there and cannot be read is an error that says why, not "no symbol with this lib_id", for which the editor would start a
/// blank symbol (`SYMBOL_EDIT_FRAME::LoadSymbol`: "Error loading symbol %s from library '%s'.").
pub(crate) fn symbol_any(dir: &Path, lib_id: &str) -> Result<(LibrarySymbol, bool), Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    if design.symbol_library.as_ref().and_then(|l| l.by_lib_id(lib_id)).is_none() {
        if let Err(why) = read_installed_symbol_in(&*crate::library_tables::libs(dir, Kind::Symbol), lib_id) {
            return Err(vec![CheckResult::fail("ops_bad_symbol", lib_id, why)]);
        }
    }
    symbol_in(dir, &design, &model, lib_id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_symbol", lib_id, "no symbol with this lib_id in the project library or anything it loads")])
}

/// [`symbol_any`]'s lookup over a design and model that are already loaded (a caller that resolves many symbols loads the board once).
pub(crate) fn symbol_in(dir: &Path, design: &eda_model::ir::Design, model: &ConstraintModel, lib_id: &str) -> Option<(LibrarySymbol, bool)> {
    symbol_in_libs(&*crate::library_tables::libs(dir, Kind::Symbol), design, model, lib_id)
}

/// [`symbol_in`] over symbol libraries already read (a caller that resolves many symbols reads the tables once).
pub(crate) fn symbol_in_libs<S: LibSource + ?Sized>(libs: &S, design: &eda_model::ir::Design, model: &ConstraintModel, lib_id: &str) -> Option<(LibrarySymbol, bool)> {
    if let Some(sym) = design.symbol_library.as_ref().and_then(|l| l.by_lib_id(lib_id)) {
        let mut sym = sym.clone();
        sym.absorb_kept_text_styles();
        return Some((sym, true));
    }
    // The library file of the tables first: the builtin table's hand-ported `Device:R` is a stand-in for it.
    if let Some(sym) = installed_symbol_in(libs, lib_id) {
        return Some((sym, false));
    }
    model.symbol_of(lib_id).map(|s| {
        let mut sym = LibrarySymbol::from_engine_symbol(&s);
        sym.assign_missing_ids();
        (sym, false)
    })
}

/// `GET /api/library/symbol?lib_id=` -- one symbol for the library actions that read without editing (Duplicate, Save Copy As,
/// Copy): `{"symbol": LibrarySymbol, "in_project": bool}`. Unlike `GET /api/symbol` it also answers for a symbol that only a library
/// file or the builtin table defines, and it never materializes anything.
pub fn symbol(dir: &Path, lib_id: &str) -> Value {
    match symbol_any(dir, lib_id) {
        Ok((symbol, in_project)) => json!({ "symbol": symbol, "in_project": in_project }),
        Err(e) => json!({ "error": board::reasons(&e) }),
    }
}

/// The power symbols the built-in table offers Place Power where KiCad's libraries are not installed.
const BUILTIN_POWER: [&str; 9] = ["power:GND", "power:+5V", "power:+3V3", "power:+12V", "power:-12V", "power:VCC", "power:VDD", "power:VSS", "power:PWR_FLAG"];

/// `GET /api/library/project`: the symbols the chooser lists beside the installed libraries -- the project's own and the ones the design uses --
/// as `{"entries": [{"id": "Lib:Name", "source", "placed", "description", "reference", "units", "pins", "power", "keywords"}]}`, sorted by id.
/// `source` is `project` (an entry of the project symbol library: authored here, imported, or the copy a placed symbol keeps -- editable), `design`
/// (the board's model resolves it: the intent's own or an installed one it already uses) or `builtin` (the small table that stands in for
/// `Device:R`, `C`, `L`, `D`, `LED`, and the common power rails, where KiCad's libraries are not installed). `placed` is set for a symbol (or power
/// symbol) with an instance on the schematic, KiCad's "Already Placed" group.
pub fn project_symbols(dir: &Path) -> Value {
    let (_, design, model) = match board::load(dir) {
        Ok(loaded) => loaded,
        Err(e) => return json!({ "error": board::reasons(&e) }),
    };
    let mut placed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    match design.schematic.as_ref() {
        Some(root) => {
            for sch in std::iter::once(root).chain(design.sheet_contents.iter().flat_map(|c| c.values())) {
                placed.extend(sch.symbols.iter().map(|s| s.lib_id.clone()).filter(|id| !id.is_empty()));
                // a power symbol is an `SCH_SYMBOL` too: Place Power's chooser lists the placed ones (`PlaceSymbol`'s `alreadyPlaced` walks every one)
                placed.extend(sch.power_symbols.iter().map(|p| p.lib_id.clone()).filter(|id| !id.is_empty()));
            }
        }
        // No stored schematic: the studio shows the one derived from the intent, whose symbols are the parts' own.
        None => placed.extend(model.parts.iter().map(|p| model.lib_id_of(p)).filter(|id| !id.is_empty())),
    }
    placed.retain(|id| !eda_model::is_synthetic_lib_id(id));

    let mut entries: std::collections::BTreeMap<String, Value> = std::collections::BTreeMap::new();
    let mut put = |id: &str, source: &str, description: &str, reference: &str, units: u32, pins: usize, power: bool, keywords: &str| {
        entries.entry(id.to_string()).or_insert_with(|| {
            json!({ "id": id, "source": source, "placed": placed.contains(id), "description": description, "reference": reference, "units": units.max(1), "pins": pins, "power": power, "keywords": keywords })
        });
    };
    let distinct_pins = |numbers: &mut dyn Iterator<Item = &str>| numbers.collect::<std::collections::BTreeSet<&str>>().len();
    if let Some(lib) = &design.symbol_library {
        for s in &lib.symbols {
            put(&s.lib_id, "project", &s.description, &s.reference_prefix, s.unit_count, distinct_pins(&mut s.pins.iter().filter(|p| p.body_style <= 1).map(|p| p.number.as_str())), s.power, &s.keywords);
        }
    }
    for s in &model.symbols {
        if !eda_model::is_synthetic_lib_id(&s.lib_id) {
            put(&s.lib_id, "design", &s.description, &s.reference_prefix, s.unit_count, distinct_pins(&mut s.pins.iter().map(|p| p.number.as_str())), s.power, "");
        }
    }
    for id in &placed {
        if let Some(s) = model.symbol_of(id) {
            put(id, "design", &s.description, &s.reference_prefix, s.unit_count, distinct_pins(&mut s.pins.iter().map(|p| p.number.as_str())), s.power, "");
        }
    }
    for s in eda_model::symbol::builtin_catalog() {
        put(&s.lib_id, "builtin", &s.description, &s.reference_prefix, s.unit_count, distinct_pins(&mut s.pins.iter().map(|p| p.number.as_str())), s.power, "");
    }
    // Place Power's chooser where KiCad's libraries are not installed: the common rails the built-in symbols draw (`symbol::builtin` has one for any
    // `power:<net>`; these are the names the old Power Symbol dialog offered).
    for id in BUILTIN_POWER {
        if let Some(s) = eda_model::symbol::builtin(id) {
            put(&s.lib_id, "builtin", &s.description, &s.reference_prefix, s.unit_count, distinct_pins(&mut s.pins.iter().map(|p| p.number.as_str())), s.power, "");
        }
    }
    json!({ "entries": entries.into_values().collect::<Vec<_>>() })
}

/// `GET /api/library/footprint?name=` -- the footprint sibling of [`symbol`].
pub fn footprint(dir: &Path, name: &str) -> Value {
    match footprint_any(dir, name) {
        Ok((footprint, in_project)) => json!({ "footprint": footprint, "in_project": in_project }),
        Err(e) => json!({ "error": board::reasons(&e) }),
    }
}

/// The derived `.kicad_sym` of any symbol (`Export...`, `Copy`): the project entry, else the resolved one -- a derived symbol as the flat symbol it stands for, as KiCad's
/// `ExportSymbol` and `CopySymbolToClipboard` write it (`LIB_SYMBOL::Flatten`), so the file or the clipboard text stands on its own.
pub fn symbol_kicad_sym_any(dir: &Path, lib_id: &str) -> Result<String, Vec<CheckResult>> {
    let (_, design, _) = board::load(dir)?;
    let (sym, _) = symbol_any(dir, lib_id)?;
    let flat = match design.symbol_library.as_ref().filter(|l| l.is_derived(lib_id)) {
        Some(lib) => lib.flatten(lib_id).map_err(|e| vec![CheckResult::fail("ops_bad_symbol", lib_id, e)])?,
        None => sym,
    };
    let embedded: std::collections::BTreeMap<String, eda_model::embedded_files::EmbeddedFiles> = design.symbol_library.as_ref().and_then(|l| l.embedded.get(lib_id)).map(|e| [(flat.lib_id.clone(), e.clone())].into()).unwrap_or_default();
    Ok(eda_kicad::export_kicad_sym_library_with(&[&flat], &embedded))
}

/// `GET /api/symbol?lib_id=&flatten=1`: the project-library symbol with everything it inherits -- what the Symbol Editor draws of a derived symbol -- or the symbol itself.
pub fn symbol_flattened(dir: &Path, lib_id: &str) -> Result<LibrarySymbol, Vec<CheckResult>> {
    let (_, design, _) = board::load(dir)?;
    let lib = design.symbol_library.as_ref().ok_or_else(|| vec![CheckResult::fail("ops_unknown_symbol", lib_id, "the project symbol library is empty")])?;
    let mut flat = lib.flatten(lib_id).map_err(|e| vec![CheckResult::fail("ops_bad_symbol", lib_id, e)])?;
    flat.absorb_kept_text_styles();
    Ok(flat)
}

/// `POST /api/symbol/clipboard/copy` -- `{"lib_id", "ids": [..]}`: the clipboard text of `SYMBOL_EDITOR_EDIT_TOOL::Copy` for the Symbol Editor's selection. KiCad flags every
/// drawn item that is not selected `STRUCT_DELETED`, copies the symbol (the copy leaves those out, the fields stay) and writes the copy with
/// `SCH_IO_KICAD_SEXPR::FormatLibSymbol`: the symbol -- its properties and fields -- with only the selected pins and graphics. Here the symbol is the one the editor draws (a
/// derived symbol's flat one), written by the library writer; `{"text"}` is that `.kicad_sym` (the client takes the `(symbol ..)` form out of it, as Copy of a whole symbol
/// does), or `{"error"}`. Nothing selected among the pins and graphics is an empty `ids` and still copies the symbol's fields, as KiCad's does not run (`!selection.GetSize()`).
pub fn symbol_items_copy(dir: &Path, body: &[u8]) -> Value {
    #[derive(serde::Deserialize)]
    struct Req {
        lib_id: String,
        ids: Vec<String>,
    }
    let req: Req = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(e) => return json!({ "error": format!("bad request: {e}") }),
    };
    if req.ids.is_empty() {
        return json!({ "error": "nothing is selected" });
    }
    let mut sym = match symbol_flattened(dir, &req.lib_id) {
        Ok(s) => s,
        Err(e) => return json!({ "error": board::reasons(&e) }),
    };
    let keep: std::collections::BTreeSet<&str> = req.ids.iter().map(String::as_str).collect();
    sym.graphics.retain(|g| keep.contains(g.id()));
    sym.pins.retain(|p| keep.contains(p.id.as_str()));
    // what the file kept beside an item that is not copied goes with it (a bezier kept as text by an older reader is not an item of the selection)
    sym.kicad.items.clear();
    let ids: std::collections::BTreeSet<String> = sym.graphics.iter().map(|g| g.id().to_string()).collect();
    sym.kicad.stroke_types.retain(|id, _| ids.contains(id));
    sym.kicad.text_styles.retain(|id, _| ids.contains(id));
    // a derived symbol is copied as the flat one (`partCopy` of the symbol the editor holds): no parent to name
    sym.extends = None;
    json!({ "text": eda_kicad::export_kicad_sym_library(&[&sym]) })
}

/// `POST /api/footprint/clipboard/copy` -- the Footprint Editor's Copy / Cut of items (`EDIT_TOOL::copyToClipboard` -> `CLIPBOARD_IO::SaveSelection( .., isFootprintEditor )`,
/// UI-LEFT-2 FF1): `{"footprint": name, "ids": [...], "reference": {"x": nm, "y": nm}}` gives `{"text": ".."}` -- a `(footprint "clipboard:<uuid>" ..)` of the selected items
/// measured from the reference point (`eda_ops::fp_copy::clipboard_footprint`), the text KiCad's Footprint Editor puts on the clipboard -- or `{"error": ..}`. A read: the
/// copy changes nothing (a Cut is the copy and then a delete verb).
pub fn footprint_items_copy(dir: &Path, body: &[u8]) -> Value {
    let req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return json!({ "error": format!("bad request: {e}") }),
    };
    let name = req.get("footprint").and_then(Value::as_str).unwrap_or_default().to_string();
    let ids: Vec<String> = req.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    if ids.is_empty() {
        return json!({ "error": "nothing is selected" });
    }
    // field by field: this build's `serde_json` keeps numbers arbitrary-precision (see crate::clipboard_api)
    let at = |k: &str| req.get("reference").and_then(|r| r.get(k)).and_then(Value::as_i64).unwrap_or(0);
    let reference = eda_model::ir::Point { x: eda_model::units::Nm::new(at("x")), y: eda_model::units::Nm::new(at("y")) };
    let (fp, _) = match footprint_any(dir, &name) {
        Ok(f) => f,
        Err(e) => return json!({ "error": board::reasons(&e) }),
    };
    // `KIID dummy; LIB_ID( "clipboard", dummy.AsString() )`: a fresh id each copy
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let uuid = format!("{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}", (n >> 32) as u32, (n >> 16) as u16, (n >> 4) as u16 & 0xfff, n as u16 & 0xfff, (n as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) & 0xffff_ffff_ffff);
    match eda_ops::fp_copy::clipboard_footprint(&fp, &ids, reference, &uuid) {
        Ok(clip) => json!({ "text": eda_kicad::export_kicad_mod(&clip) }),
        Err(e) => json!({ "error": board::reasons(&e) }),
    }
}

/// The derived `.kicad_mod` of any footprint (`Export Current Footprint...`).
pub fn footprint_kicad_mod_any(dir: &Path, name: &str) -> Result<String, Vec<CheckResult>> {
    let (fp, _) = footprint_any(dir, name)?;
    // the files a project footprint embeds are written with it
    let embedded = board::load(dir).ok().and_then(|(_, design, _)| design.footprint_library.and_then(|l| l.embedded.get(name).cloned())).unwrap_or_default();
    Ok(eda_kicad::export_kicad_mod_with(&fp, &embedded))
}

fn text_of(body: &[u8]) -> Result<&str, Value> {
    std::str::from_utf8(body).map_err(|_| json!({ "error": "the text is not valid UTF-8" }))
}

/// `POST /api/symbol_library/parse` -- the body is a `.kicad_sym` file, or the clipboard text of one or
/// more `(symbol ...)` forms. `{"symbols": [LibrarySymbol...], "warnings": [...]}` or `{"error": ...}`.
pub fn parse_symbols(body: &[u8]) -> Value {
    let text = match text_of(body) {
        Ok(t) => t,
        Err(e) => return e,
    };
    match eda_kicad::parse_library_symbols(text) {
        Ok(parsed) => json!({ "symbols": parsed.symbols, "warnings": parsed.warnings, "embedded": parsed.embedded }),
        Err(e) => json!({ "error": e }),
    }
}

/// `POST /api/footprint/parse` -- the body is a `.kicad_mod` file. `{"footprint": LibraryFootprint,
/// "warnings": [...]}` or `{"error": ...}`.
pub fn parse_footprint(body: &[u8]) -> Value {
    let text = match text_of(body) {
        Ok(t) => t,
        Err(e) => return e,
    };
    match eda_kicad::parse_library_footprint(text) {
        Ok(parsed) => json!({ "footprint": parsed.footprint, "warnings": parsed.warnings, "embedded": parsed.embedded }),
        Err(e) => json!({ "error": e }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_symbols_answers_with_the_symbols_or_an_error() {
        let ok = parse_symbols(br#"(kicad_symbol_lib (version 20231120) (symbol "R" (symbol "R_1_1" (pin passive line (at 0 3.81 270) (length 1.27) (name "~") (number "1")))))"#);
        assert_eq!(ok["symbols"][0]["lib_id"], "R");
        assert_eq!(ok["symbols"][0]["pins"][0]["number"], "1");
        assert!(ok["warnings"].as_array().unwrap().is_empty());
        assert!(parse_symbols(b"nope")["error"].is_string());
        assert!(parse_symbols(&[0xff, 0xfe])["error"].is_string());
    }

    #[test]
    fn parse_footprint_answers_with_the_footprint_or_an_error() {
        let ok = parse_footprint(br#"(footprint "X" (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu")))"#);
        assert_eq!(ok["footprint"]["name"], "X");
        assert_eq!(ok["footprint"]["pads"][0]["number"], "1");
        assert!(parse_footprint(b"(kicad_pcb)")["error"].is_string());
    }

    /// A `.kicad_mod` dropped on the Footprint Editor (`pcbnew.Control.ddImportFootprint`) is read by this route: its silkscreen, its fab drawing and its text come with it.
    #[test]
    fn a_dropped_kicad_mod_brings_its_silkscreen_and_fab_art() {
        let text = br#"(footprint "Dropped" (layer "F.Cu")
            (fp_line (start -2 -1) (end 2 -1) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))
            (fp_rect (start -1 -0.5) (end 1 0.5) (stroke (width 0.1) (type solid)) (fill no) (layer "F.Fab"))
            (fp_text user "${REFERENCE}" (at 0 0 0) (layer "F.Fab") (effects (font (size 0.5 0.5) (thickness 0.08))))
            (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Mask" "F.Paste")))"#;
        let r = parse_footprint(text);
        let layers: Vec<String> = r["footprint"]["graphics"].as_array().unwrap_or(&vec![]).iter().map(|g| g["layer"].as_str().unwrap_or("").to_string()).collect();
        assert!(layers.iter().any(|l| l == "F.SilkS") && layers.iter().any(|l| l == "F.Fab"), "{r}");
        assert_eq!(r["footprint"]["texts"].as_array().map(Vec::len), Some(1), "{r}");
    }

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("eda-library-api-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_installed_footprint_opens_with_its_graphics_under_its_full_name() {
        let root = temp_root("fp");
        std::fs::create_dir_all(root.join("Package_X.pretty")).unwrap();
        std::fs::write(
            root.join("Package_X.pretty").join("Chip-4.kicad_mod"),
            r#"(footprint "Chip-4" (descr "a chip") (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu")) (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu"))
                (fp_line (start -2 -1) (end 2 -1) (stroke (width 0.12) (type solid)) (layer "F.SilkS")) (fp_text user "${REFERENCE}" (at 0 2) (layer "F.Fab")))"#,
        )
        .unwrap();
        let fp = installed_footprint_in(&root, "Package_X:Chip-4").unwrap().expect("found");
        assert_eq!(fp.name, "Package_X:Chip-4", "the project key is the tree's: Lib:Name");
        assert_eq!(fp.pads.len(), 2);
        assert_eq!(fp.graphics.len(), 1, "the silkscreen line the engine's pad-only loader would drop");
        assert!(fp.pads.iter().all(|p| !p.id.is_empty()), "ids are assigned, so every edit verb can address a pad");
        assert!(installed_footprint_in(&root, "Package_X:Missing").unwrap().is_none());
        assert!(installed_footprint_in(&root, "Chip-4").unwrap().is_none(), "a bare name is not an installed-library id");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_installed_symbol_opens_from_its_library_file() {
        let root = temp_root("sym");
        std::fs::write(
            root.join("Tiny.kicad_sym"),
            r#"(kicad_symbol_lib (version 20231120) (symbol "Res" (property "Reference" "R" (at 0 0 0)) (symbol "Res_1_1" (pin passive line (at 0 3.81 270) (length 1.27) (name "~") (number "1")) (pin passive line (at 0 -3.81 90) (length 1.27) (name "~") (number "2")))))"#,
        )
        .unwrap();
        let sym = installed_symbol_in(&root, "Tiny:Res").expect("found");
        assert_eq!(sym.lib_id, "Tiny:Res");
        assert_eq!(sym.pins.len(), 2);
        assert!(installed_symbol_in(&root, "Tiny:Nope").is_none());
        assert!(installed_symbol_in(&root, "Missing:Res").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_installed_id_cannot_name_a_file_outside_the_library_root() {
        let root = temp_root("escape");
        std::fs::create_dir_all(root.join("Lib.pretty")).unwrap();
        std::fs::write(root.parent().unwrap().join("outside.kicad_mod"), r#"(footprint "outside" (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu")))"#).unwrap();
        for bad in ["Lib:../../outside", "Lib:../outside", "..:outside", "Lib:a/b", "Lib:..\\outside"] {
            assert!(installed_footprint_in(&root, bad).is_ok_and(|f| f.is_none()), "{bad}");
            assert!(installed_symbol_in(&root, bad).is_none(), "{bad}");
        }
        let _ = std::fs::remove_file(root.parent().unwrap().join("outside.kicad_mod"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// KiCad.app's own libraries, when installed: the footprint of the 3D-model brief (`SOIC-16_3.9x9.9mm_P1.27mm`) and a symbol both open.
    #[test]
    fn kicads_installed_libraries_open_when_present() {
        let (fp_root, sym_root) = (eda_kicad::default_footprint_library_root(), eda_kicad::default_symbol_library_root());
        if !fp_root.is_dir() || !sym_root.is_dir() {
            return;
        }
        let fp = installed_footprint_in(&fp_root, "Package_SO:SOIC-16_3.9x9.9mm_P1.27mm").unwrap().expect("SOIC-16 is installed");
        assert_eq!(fp.pads.len(), 16);
        assert!(!fp.graphics.is_empty(), "silkscreen, fab and courtyard graphics");
        assert!(fp.model.as_deref().is_some_and(|m| m.contains("SOIC-16_3.9x9.9mm_P1.27mm")), "and its 3D model path: {:?}", fp.model);
        let r = installed_symbol_in(&sym_root, "Device:R").expect("Device:R is installed");
        assert_eq!(r.pins.len(), 2);
    }

    /// A board directory of its own (`board.json`, an empty intent, a design with the sections the chooser's project list reads).
    fn scratch_board(tag: &str, design_json: &str) -> std::path::PathBuf {
        let dir = temp_root(tag);
        let intent = dir.join("intent.yaml");
        std::fs::write(&intent, serde_yaml::to_string(&ConstraintModel::default()).unwrap()).unwrap();
        std::fs::write(dir.join("board.json"), serde_json::to_string(&json!({ "intent": intent.display().to_string(), "snap_nm": 100_000, "spacing_nm": 300_000 })).unwrap()).unwrap();
        std::fs::write(dir.join("design.json"), design_json).unwrap();
        dir
    }

    #[test]
    fn the_choosers_project_list_has_the_project_library_what_is_placed_and_the_stand_ins() {
        let dir = scratch_board(
            "project",
            r##"{"schema":1,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]},
                "placement":{"outline":[],"footprints":[],"modules":[]},
                "schematic":{"symbols":[
                    {"id":"R1","at":{"x":0,"y":0},"rot":0,"lib_id":"Device:R"},
                    {"id":"U1","at":{"x":9000,"y":0},"rot":0,"lib_id":"Mine:Part"},
                    {"id":"U2","at":{"x":19000,"y":0},"rot":0,"lib_id":"Amp:LM358"}],"wires":[],
                    "power_symbols":[{"id":"#PWR01","lib_id":"power:GND","at":{"x":0,"y":9000},"rot":0,"net":"GND","pin":""}]},
                "symbol_library":{"symbols":[
                    {"lib_id":"Mine:Part","reference_prefix":"U","description":"My part","keywords":"mine custom","unit_count":2,"pins":[{"number":"1","name":"A","electrical_type":"input","shape":"line","at":{"x":0,"y":0},"angle_deg":0,"length_mm":2.54,"unit":1},{"number":"1","name":"A","electrical_type":"input","shape":"line","at":{"x":0,"y":0},"angle_deg":0,"length_mm":2.54,"unit":2},{"number":"2","name":"B","electrical_type":"output","shape":"line","at":{"x":0,"y":2.54},"angle_deg":0,"length_mm":2.54,"unit":1}]},
                    {"lib_id":"eda:Unused","description":"Never placed"},
                    {"lib_id":"Amp:LM358","reference_prefix":"U","description":"Dual op-amp","published":true,"unit_count":3}]}}"##,
        );
        let v = project_symbols(&dir);
        let entries = v["entries"].as_array().unwrap_or_else(|| panic!("{v}"));
        let by = |id: &str| entries.iter().find(|e| e["id"] == id).unwrap_or_else(|| panic!("no {id} in {v}"));
        let mine = by("Mine:Part");
        assert_eq!((mine["source"].clone(), mine["placed"].clone(), mine["units"].clone(), mine["pins"].clone(), mine["keywords"].clone()), (json!("project"), json!(true), json!(2), json!(2), json!("mine custom")), "distinct pin numbers, the first body style");
        assert_eq!((by("eda:Unused")["placed"].clone(), by("eda:Unused")["source"].clone()), (json!(false), json!("project")));
        assert_eq!((by("Amp:LM358")["placed"].clone(), by("Amp:LM358")["units"].clone()), (json!(true), json!(3)), "a placed installed symbol keeps its definition in the project library");
        // what the design resolves, and the stand-ins for where KiCad's libraries are not installed
        assert_eq!(by("Device:R")["placed"], true);
        assert_eq!(by("Device:R")["source"], "design", "the model resolves it: the built-in table or the library file");
        assert_eq!(by("Device:LED")["source"], "builtin");
        // the power symbols are flagged (the Symbol Chooser leaves them out, Place Power's takes only them): the built-in rails, and a placed one is "Already Placed"
        assert!(entries.iter().filter(|e| e["id"].as_str().is_some_and(|id| id.starts_with("power:"))).all(|e| e["power"] == true));
        assert_eq!((by("power:+3V3")["source"].clone(), by("power:+3V3")["placed"].clone()), (json!("builtin"), json!(false)));
        assert_eq!(by("power:GND")["placed"], true, "{v}");
        assert!(entries.windows(2).all(|w| w[0]["id"].as_str() <= w[1]["id"].as_str()), "sorted by id");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// UI-LEFT Y5: the Symbol Editor's Copy puts `.kicad_sym` text on the clipboard (`SYMBOL_EDITOR_EDIT_TOOL::Copy`): the symbol with its fields and only the selected pins and
    /// graphics, which Paste reads back -- here or in KiCad (`ParseLibSymbols`).
    #[test]
    fn copy_of_items_is_the_symbol_with_only_the_selected_pins_and_graphics_as_kicad_sym_text() {
        let dir = scratch_board(
            "items-copy",
            r##"{"schema":2,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]},
                "placement":{"outline":[],"footprints":[],"modules":[]},
                "symbol_library":{"symbols":[
                    {"lib_id":"Mine:Part","reference_prefix":"U","description":"My part",
                     "graphics":[{"kind":"rectangle","id":"r","unit":0,"start":{"x":-2.54,"y":-2.54},"end":{"x":2.54,"y":2.54},"stroke_mm":0.254},
                                 {"kind":"text","id":"t","unit":0,"text":"Hi","at":{"x":0,"y":1.27},"size_mm":1.27,"h_align":"left"},
                                 {"kind":"bezier","id":"b","unit":0,"start":{"x":0,"y":0},"c1":{"x":1,"y":2},"c2":{"x":3,"y":2},"end":{"x":4,"y":0},"stroke_mm":0}],
                     "pins":[{"id":"p1","number":"1","name":"A","electrical_type":"input","shape":"line","at":{"x":-5.08,"y":0},"angle_deg":0,"length_mm":2.54,"unit":1},
                             {"id":"p2","number":"2","name":"B","electrical_type":"output","shape":"line","at":{"x":5.08,"y":0},"angle_deg":180,"length_mm":2.54,"unit":1}]}]}}"##,
        );
        let v = symbol_items_copy(&dir, br#"{"lib_id":"Mine:Part","ids":["p2","t","b"]}"#);
        let text = v["text"].as_str().unwrap_or_else(|| panic!("{v}"));
        let parsed = eda_kicad::parse_library_symbols(text).unwrap();
        let sym = &parsed.symbols[0];
        assert_eq!(sym.pins.iter().map(|p| p.number.as_str()).collect::<Vec<_>>(), ["2"]);
        assert_eq!(sym.graphics.iter().map(|g| match g {
            eda_model::ir::LibrarySymbolGraphic::Text { h_align, .. } => format!("text {h_align:?}"),
            eda_model::ir::LibrarySymbolGraphic::Bezier { .. } => "bezier".to_string(),
            other => format!("{other:?}"),
        }).collect::<Vec<_>>(), ["text Left", "bezier"]);
        assert_eq!(sym.description, "My part", "the fields come with it");
        assert!(symbol_items_copy(&dir, br#"{"lib_id":"Mine:Part","ids":[]}"#)["error"].is_string());
        assert!(symbol_items_copy(&dir, br#"{"lib_id":"Mine:Nope","ids":["p1"]}"#)["error"].is_string());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_the_parsers_return_is_what_a_put_verb_accepts() {
        // The studio posts the parsed entry straight back inside `put_library_*`: it must deserialize.
        let sym = parse_symbols(br#"(kicad_symbol_lib (symbol "R" (symbol "R_1_1" (pin passive line (at 0 3.81 270) (length 1.27) (name "~") (number "1")))))"#);
        let mut symbol = sym["symbols"][0].clone();
        symbol["lib_id"] = json!("eda:R");
        let cmd = json!({ "op": "put_library_symbol", "symbol": symbol });
        assert!(serde_json::from_value::<eda_ops::Cmd>(cmd).is_ok());
        let fp = parse_footprint(br#"(footprint "X" (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu")))"#);
        let cmd = json!({ "op": "put_library_footprint", "footprint": fp["footprint"].clone() });
        assert!(serde_json::from_value::<eda_ops::Cmd>(cmd).is_ok());
    }
}
