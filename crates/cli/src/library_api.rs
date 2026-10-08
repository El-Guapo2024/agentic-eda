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

/// A footprint by name from the project library, else whatever the model resolves it to (intent, a loaded `.kicad_mod`, the
/// builtin table) -- the same two-tier lookup `eda_ops::Board::open_footprint_for_edit` uses, for a *read-only* answer.
fn footprint_any(dir: &Path, name: &str) -> Result<(LibraryFootprint, bool), Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    if let Some(fp) = design.footprint_library.as_ref().and_then(|l| l.by_name(name)) {
        return Ok((fp.clone(), true));
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

/// A symbol by `lib_id` from the project library, else the resolved one (real library file, builtin table).
fn symbol_any(dir: &Path, lib_id: &str) -> Result<(LibrarySymbol, bool), Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    symbol_in(&design, &model, lib_id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_symbol", lib_id, "no symbol with this lib_id in the project library or anything it loads")])
}

/// [`symbol_any`]'s lookup over a design and model that are already loaded (a caller that resolves many symbols loads the board once).
pub(crate) fn symbol_in(design: &eda_model::ir::Design, model: &ConstraintModel, lib_id: &str) -> Option<(LibrarySymbol, bool)> {
    if let Some(sym) = design.symbol_library.as_ref().and_then(|l| l.by_lib_id(lib_id)) {
        return Some((sym.clone(), true));
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
