//! `/api/symbol_library/parse` and `/api/footprint/parse`: the read-only half of the library editors'
//! Import and Paste actions (`web/studio/src/state/symbolEditorStore.tsx`, `footprintEditorStore.tsx`).
//!
//! The browser reads the file (or the clipboard text), posts it here, and gets back the editable
//! library entry (`LibrarySymbol` / `LibraryFootprint`, exactly as `design.json` stores them) plus a
//! list of anything the entry cannot hold. What happens next -- picking a free name, storing it -- is a
//! `Cmd::PutLibrarySymbol` / `Cmd::PutLibraryFootprint` through `/api/cmd`, with undo, like every other
//! edit; these two routes never touch `design.json`.

use crate::board;
use eda_model::ir::{LibraryFootprint, LibrarySymbol};
use eda_model::{CheckResult, ConstraintModel};
use serde_json::{json, Value};
use std::path::Path;

/// A library nickname or item name is part of a file path under the library root: nothing in it may lead out of that root.
fn path_safe(part: &str) -> bool {
    !part.is_empty() && !part.starts_with('.') && !part.contains(['/', '\\']) && !part.contains("..")
}

/// A footprint of KiCad's installed libraries (`Lib:Name` -> `<root>/Lib.pretty/Name.kicad_mod`) in the editable type, with every graphic, text, attribute
/// and pad layer the file holds (`eda_kicad::parse_library_footprint`, the reader behind Import -- the engine's own loader keeps only pads and a courtyard)
/// and named `Lib:Name`, the key the library tree shows it under.
fn installed_footprint_in(root: &Path, name: &str) -> Option<LibraryFootprint> {
    let (lib, item) = name.split_once(':')?;
    if !path_safe(lib) || !path_safe(item) {
        return None;
    }
    let text = std::fs::read_to_string(eda_kicad::find_footprint_file(root, name)?).ok()?;
    let mut fp = eda_kicad::parse_library_footprint(&text).ok()?.footprint;
    fp.name = name.to_string();
    fp.assign_missing_ids();
    Some(fp)
}

/// A symbol of KiCad's installed libraries (`Lib:Name` in `<root>/Lib.kicad_sym`) in the editable type. The library file is parsed once and kept
/// (`eda_kicad::resolve_symbol`'s own cache), so a library is read on the first symbol anyone opens from it, not before.
fn installed_symbol_in(root: &Path, lib_id: &str) -> Option<LibrarySymbol> {
    let (lib, item) = lib_id.split_once(':')?;
    if !path_safe(lib) || !path_safe(item) {
        return None;
    }
    let sym = eda_kicad::resolve_symbol(root, lib_id)?;
    let mut sym = LibrarySymbol::from_engine_symbol(&sym);
    sym.assign_missing_ids();
    Some(sym)
}

/// A footprint by name from the project library, else KiCad's installed libraries, else whatever the model resolves it to (intent, a loaded
/// `.kicad_mod`, the builtin table) -- for a *read-only* answer; opening one in the editor copies it into the project library first.
fn footprint_any(dir: &Path, name: &str) -> Result<(LibraryFootprint, bool), Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    if let Some(fp) = design.footprint_library.as_ref().and_then(|l| l.by_name(name)) {
        return Ok((fp.clone(), true));
    }
    // KiCad's installed libraries come before what the board's own model resolved the name to: that is the pad-only copy of the same file.
    if let Some(fp) = installed_footprint_in(&eda_kicad::default_footprint_library_root(), name) {
        return Ok((fp, false));
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

/// A symbol by `lib_id` from the project library, else KiCad's installed libraries, else the resolved one (the board's model, the builtin table).
fn symbol_any(dir: &Path, lib_id: &str) -> Result<(LibrarySymbol, bool), Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    symbol_in(&design, &model, lib_id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_symbol", lib_id, "no symbol with this lib_id in the project library or anything it loads")])
}

/// [`symbol_any`]'s lookup over a design and model that are already loaded (a caller that resolves many symbols loads the board once).
pub(crate) fn symbol_in(design: &eda_model::ir::Design, model: &ConstraintModel, lib_id: &str) -> Option<(LibrarySymbol, bool)> {
    if let Some(sym) = design.symbol_library.as_ref().and_then(|l| l.by_lib_id(lib_id)) {
        return Some((sym.clone(), true));
    }
    // The installed library file first: the builtin table's hand-ported `Device:R` is a stand-in for it.
    if let Some(sym) = installed_symbol_in(&eda_kicad::default_symbol_library_root(), lib_id) {
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

/// `GET /api/library/footprint?name=` -- the footprint sibling of [`symbol`].
pub fn footprint(dir: &Path, name: &str) -> Value {
    match footprint_any(dir, name) {
        Ok((footprint, in_project)) => json!({ "footprint": footprint, "in_project": in_project }),
        Err(e) => json!({ "error": board::reasons(&e) }),
    }
}

/// The derived `.kicad_sym` of any symbol (`Export...`, `Copy`): the project entry, else the resolved one.
pub fn symbol_kicad_sym_any(dir: &Path, lib_id: &str) -> Result<String, Vec<CheckResult>> {
    symbol_any(dir, lib_id).map(|(sym, _)| eda_kicad::export_kicad_sym(&sym))
}

/// The derived `.kicad_mod` of any footprint (`Export Current Footprint...`).
pub fn footprint_kicad_mod_any(dir: &Path, name: &str) -> Result<String, Vec<CheckResult>> {
    footprint_any(dir, name).map(|(fp, _)| eda_kicad::export_kicad_mod(&fp))
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
        Ok(parsed) => json!({ "symbols": parsed.symbols, "warnings": parsed.warnings }),
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
        Ok(parsed) => json!({ "footprint": parsed.footprint, "warnings": parsed.warnings }),
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
        let fp = installed_footprint_in(&root, "Package_X:Chip-4").expect("found");
        assert_eq!(fp.name, "Package_X:Chip-4", "the project key is the tree's: Lib:Name");
        assert_eq!(fp.pads.len(), 2);
        assert_eq!(fp.graphics.len(), 1, "the silkscreen line the engine's pad-only loader would drop");
        assert!(fp.pads.iter().all(|p| !p.id.is_empty()), "ids are assigned, so every edit verb can address a pad");
        assert!(installed_footprint_in(&root, "Package_X:Missing").is_none());
        assert!(installed_footprint_in(&root, "Chip-4").is_none(), "a bare name is not an installed-library id");
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
            assert!(installed_footprint_in(&root, bad).is_none(), "{bad}");
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
        let fp = installed_footprint_in(&fp_root, "Package_SO:SOIC-16_3.9x9.9mm_P1.27mm").expect("SOIC-16 is installed");
        assert_eq!(fp.pads.len(), 16);
        assert!(!fp.graphics.is_empty(), "silkscreen, fab and courtyard graphics");
        assert!(fp.model.as_deref().is_some_and(|m| m.contains("SOIC-16_3.9x9.9mm_P1.27mm")), "and its 3D model path: {:?}", fp.model);
        let r = installed_symbol_in(&sym_root, "Device:R").expect("Device:R is installed");
        assert_eq!(r.pins.len(), 2);
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
