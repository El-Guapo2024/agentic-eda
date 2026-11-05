//! The library editors' Save, Save Library As and Revert into the libraries of the library tables (UI-LEFT L1, GAPS.md item 5): the routes behind
//! `web/studio/src/actions/librarySaveOps.ts`.
//!
//! The editors edit a copy of a library's symbol or footprint in the design (`design.symbol_library`, `design.footprint_library`, keyed `Lib:Name`, every
//! edit an undoable `/api/cmd` verb), which is what KiCad's library manager keeps as the library's buffer (`LIB_BUFFER`, `SYMBOL_BUFFER`): Save writes the
//! buffer into the library's file (`SYMBOL_EDIT_FRAME::saveLibrary` -> `SYMBOL_LIBRARY_MANAGER::SaveLibrary`; `FOOTPRINT_EDIT_FRAME::SaveFootprint` ->
//! `SaveFootprintInLibrary`), Revert puts the file's version back into the buffer (`RevertSymbol`, `RevertLibrary`, `RevertFootprint`; the web sends that
//! as a `put_library_*` verb, so it is one undo step), and a symbol is modified (`IsSymbolModified`, the tree's ` *`) when its buffer is not what the file
//! holds. The project library (`eda`, the design's own) has no file: its symbols and footprints are saved as they are made.
//!
//! Every write goes to the path a row of the board's library tables names (`crate::library_tables`), or -- Save Library As, New Library -- to a new path
//! that the same request puts in the board's table. Nothing is written into KiCad's own configuration folder. The files are written by
//! `eda_kicad::library_save` with the editors' writers.
//!
//! Where this differs from KiCad:
//!   * a symbol is modified when this writer's text of the buffer differs from its text of the file's symbol, where KiCad keeps a flag per buffer (its
//!     screen's `IsContentModified`): a symbol edited and edited back is not modified here;
//!   * Save writes only the modified symbols of the library (the others keep the file's text, `eda_kicad::library_save`), where KiCad formats every symbol
//!     of the library again; a library with nothing modified is not written;
//!   * the editor opens a derived symbol of a library as the flat symbol it stands for (`eda_kicad::parse_symbol`); saved back, it is written derived from
//!     its parent again when that loses nothing (its drawing, units and flags are still the parent's: what KiCad lets one edit of a derived symbol, its
//!     fields), else flat;
//!   * Delete and Rename of a library's symbol stay in the design: the file keeps the old symbol (KiCad's buffer remembers the deletion, `GetDeletedBuffers`);
//!   * the file is not watched: a library changed on disk while its symbol is open is not offered for reloading (`SYMBOL_EDIT_FRAME`'s file watcher).

use crate::board;
use crate::library_index::Kind;
use crate::library_tables::{self, Env, LibEntry};
use eda_kicad::library_save::{self as ls, SymbolSave};
use eda_kicad::SymbolSummary;
use eda_model::ir::{Design, LibraryFootprint, LibrarySymbol, SymbolLibrarySection};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::SystemTime;

/// The nickname of the studio's own project library (`eda:Name`): part of the design, no file of its own.
pub(crate) const PROJECT_LIBRARY: &str = "eda";

fn bad(msg: impl Into<String>) -> Value {
    json!({ "error": msg.into() })
}

fn split(id: &str) -> (&str, &str) {
    id.split_once(':').unwrap_or(("", id))
}

/// The library nicknamed `lib` of the board's tables (one that can be read and is switched on), or none.
fn lib_entry(dir: &Path, kind: Kind, lib: &str) -> Option<LibEntry> {
    if lib.is_empty() || lib == PROJECT_LIBRARY {
        return None;
    }
    library_tables::libs(dir, kind).find(lib).cloned()
}

// ------------------------------------------------------------------------------------------------ the saved forms

/// A symbol library's text and its symbols, read again only when the library changes (a folder library is hundreds of files; the tree asks which symbols
/// are modified every few seconds).
type SavedText = (Option<SystemTime>, Arc<String>, Arc<Vec<SymbolSummary>>);

fn saved_library(path: &Path) -> Option<(Arc<String>, Arc<Vec<SymbolSummary>>)> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, SavedText>>> = OnceLock::new();
    let stamp = eda_kicad::symbol_library_modified(path);
    stamp?;
    let cache = CACHE.get_or_init(Default::default);
    if let Some((s, text, summaries)) = cache.lock().unwrap_or_else(PoisonError::into_inner).get(path) {
        if *s == stamp {
            return Some((text.clone(), summaries.clone()));
        }
    }
    let text = Arc::new(eda_kicad::read_symbol_library_text(path).ok()?);
    let summaries = Arc::new(eda_kicad::scan_symbols(&text));
    cache.lock().unwrap_or_else(PoisonError::into_inner).insert(path.to_path_buf(), (stamp, text.clone(), summaries.clone()));
    Some((text, summaries))
}

/// The library file's symbol `lib_id` (`Lib:Name`) as the editor opens it (`parse_symbol`: a derived one flat), or none when the file has no such symbol.
fn saved_symbol(path: &Path, lib_id: &str) -> Option<LibrarySymbol> {
    let (text, summaries) = saved_library(path)?;
    let mut sym = eda_kicad::parse_symbol(&text, &summaries, split(lib_id).1)?;
    sym.lib_id = lib_id.to_string();
    sym.assign_missing_ids();
    Some(sym)
}

/// The symbol the library file's symbol `name` extends, when it is derived.
fn saved_parent(path: &Path, name: &str) -> Option<String> {
    let (_, summaries) = saved_library(path)?;
    summaries.iter().rev().find(|s| s.name == name).and_then(|s| s.extends.clone())
}

/// The library file's footprint `Lib:Name` as the editor opens it, or none.
fn saved_footprint(path: &Path, name: &str) -> Option<LibraryFootprint> {
    let text = std::fs::read_to_string(path.join(ls::footprint_file_name(split(name).1))).ok()?;
    let mut fp = eda_kicad::parse_library_footprint(&text).ok()?.footprint;
    fp.name = name.to_string();
    fp.assign_missing_ids();
    Some(fp)
}

/// What a symbol is compared by: this writer's text of it, under its bare name (ids and the `published` flag are not in a file).
fn symbol_text(sym: &LibrarySymbol) -> String {
    let mut s = sym.clone();
    s.lib_id = split(&sym.lib_id).1.to_string();
    s.published = false;
    eda_kicad::export_kicad_sym(&s)
}

fn footprint_text(fp: &LibraryFootprint) -> String {
    let mut f = fp.clone();
    f.name = split(&fp.name).1.to_string();
    f.published = false;
    eda_kicad::export_kicad_mod(&f)
}

/// The symbol with everything it inherits (what the file's form is compared with); the entry itself when it is not derived or its parent is missing.
fn flat_entry(section: &SymbolLibrarySection, lib_id: &str) -> Option<LibrarySymbol> {
    section.flatten(lib_id).ok().or_else(|| section.by_lib_id(lib_id).cloned())
}

/// The design's symbols of library `lib`, by `lib_id`.
fn buffered_symbols<'a>(design: &'a Design, lib: &str) -> Vec<&'a LibrarySymbol> {
    design.symbol_library.as_ref().map(|l| l.symbols.iter().filter(|s| split(&s.lib_id).0 == lib).collect()).unwrap_or_default()
}

/// `SYMBOL_LIBRARY_MANAGER::IsSymbolModified` for the design's symbols of library `lib` at `path`: the ones whose text differs from the file's symbol, or
/// that the file lacks (see the module doc).
fn modified_symbols(design: &Design, path: &Path, lib: &str) -> Vec<String> {
    let Some(section) = design.symbol_library.as_ref() else { return Vec::new() };
    buffered_symbols(design, lib)
        .into_iter()
        .filter(|s| match (flat_entry(section, &s.lib_id), saved_symbol(path, &s.lib_id)) {
            (Some(ours), Some(saved)) => symbol_text(&ours) != symbol_text(&saved),
            _ => true,
        })
        .map(|s| s.lib_id.clone())
        .collect()
}

/// The modified symbol `lib_id` as Save writes it: as it is when it is derived in the design or is new to the file; when the file had it derived from a
/// parent and the design has it flat (opened flat), derived again if that loses nothing (see the module doc).
fn as_saved(section: &SymbolLibrarySection, path: &Path, lib_id: &str) -> Option<LibrarySymbol> {
    let sym = section.by_lib_id(lib_id)?.clone();
    if sym.extends.is_some() {
        return Some(sym);
    }
    let (lib, name) = split(lib_id);
    let Some(parent) = saved_parent(path, name) else { return Some(sym) };
    let parent_id = format!("{lib}:{parent}");
    // the parent as it will be after the save: the design's when the design has it, else the file's
    let parent_flat = flat_entry(section, &parent_id).or_else(|| saved_symbol(path, &parent_id))?;
    let mut candidate = sym.clone();
    candidate.extends = Some(parent_id.clone());
    let mut root = parent_flat;
    root.lib_id = parent_id;
    root.extends = None;
    let trial = SymbolLibrarySection { symbols: vec![root, candidate.clone()], ..Default::default() };
    match trial.flatten(lib_id) {
        Ok(flat) if symbol_text(&flat) == symbol_text(&sym) => Some(candidate),
        _ => Some(sym),
    }
}

/// What Save writes of library `lib` at `path`: each modified symbol (see [`as_saved`]).
fn symbols_to_save(design: &Design, path: &Path, lib: &str) -> Vec<LibrarySymbol> {
    let Some(section) = design.symbol_library.as_ref() else { return Vec::new() };
    modified_symbols(design, path, lib).iter().filter_map(|id| as_saved(section, path, id)).collect()
}

fn saves<'a>(design: &'a Design, symbols: &'a [LibrarySymbol]) -> Vec<SymbolSave<'a>> {
    let embedded = design.symbol_library.as_ref().map(|l| &l.embedded);
    symbols.iter().map(|s| SymbolSave { symbol: s, embedded: embedded.and_then(|e| e.get(&s.lib_id)) }).collect()
}

// ------------------------------------------------------------------------------------------------ the routes

fn kind_of(v: &str) -> Result<Kind, Value> {
    Kind::parse(v).ok_or_else(|| bad("kind is footprint or symbol"))
}

/// `GET /api/library/writable?kind=`: every library of the board's tables with whether it can be written (`IsSymbolLibWritable`, `IsFootprintLibWritable`),
/// for the Save As dialogs (KiCad lists every library and refuses a read-only one when it is chosen) and Save's read-only question.
/// `{"libraries": [{name, path, scope, description, writable, hidden}]}`.
pub(crate) fn writable(dir: &Path, kind: &str) -> Value {
    let kind = match kind_of(kind) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let libs = library_tables::libs(dir, kind);
    let rows: Vec<Value> = libs
        .entries()
        .iter()
        .map(|e| {
            let writable = match kind {
                Kind::Symbol => ls::is_symbol_library_writable(&e.path),
                Kind::Footprint => ls::is_footprint_library_writable(&e.path),
            };
            json!({ "name": e.name, "path": e.path.display().to_string(), "scope": e.scope.name(), "description": e.description, "writable": writable, "hidden": e.hidden })
        })
        .collect();
    json!({ "libraries": rows })
}

/// `GET /api/library/modified?kind=`: the design's symbols (or footprints) of the table libraries that differ from what the library's file holds, and their
/// libraries -- the library tree's ` *` (`SYMBOL_TREE_SYNCHRONIZING_ADAPTER::GetValue`: "mark modified items with an asterisk"), Save All's list.
/// `{"items": ["Lib:Name"...], "libraries": ["Lib"...]}`.
pub(crate) fn modified(dir: &Path, kind: &str) -> Value {
    let kind = match kind_of(kind) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let (_, design, _) = match board::load(dir) {
        Ok(l) => l,
        Err(e) => return bad(board::reasons(&e)),
    };
    let libs = library_tables::libs(dir, kind);
    let mut items: Vec<String> = Vec::new();
    match kind {
        Kind::Symbol => {
            let names: BTreeSet<&str> = design.symbol_library.iter().flat_map(|l| l.symbols.iter().map(|s| split(&s.lib_id).0)).collect();
            for lib in names {
                if let Some(e) = (lib != PROJECT_LIBRARY).then(|| libs.find(lib)).flatten() {
                    items.extend(modified_symbols(&design, &e.path, lib));
                }
            }
        }
        Kind::Footprint => {
            for fp in design.footprint_library.iter().flat_map(|l| l.footprints.iter()) {
                let (lib, _) = split(&fp.name);
                if let Some(e) = (!lib.is_empty() && lib != PROJECT_LIBRARY).then(|| libs.find(lib)).flatten() {
                    if saved_footprint(&e.path, &fp.name).is_none_or(|saved| footprint_text(&saved) != footprint_text(fp)) {
                        items.push(fp.name.clone());
                    }
                }
            }
        }
    }
    items.sort();
    let libraries: BTreeSet<&str> = items.iter().map(|i| split(i).0).collect();
    json!({ "items": items, "libraries": libraries })
}

/// `GET /api/library/saved?kind=&id=` (or `&lib=`): what the library's file holds of an item, for Revert (`RevertSymbol`, `RevertFootprint`) -- `{"symbol" |
/// "footprint": definition | null}` (null: the file has no such item, it was never saved). With `lib` (`RevertLibrary`, symbols): every design symbol of the
/// library, `{"symbols": {lib_id: definition | null}}`.
pub(crate) fn saved(dir: &Path, kind: &str, id: &str, lib: &str) -> Value {
    let kind = match kind_of(kind) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let nick = if lib.is_empty() { split(id).0 } else { lib };
    let Some(entry) = lib_entry(dir, kind, nick) else { return bad(format!("Library '{nick}' not found.")) };
    match kind {
        Kind::Symbol if !lib.is_empty() => {
            let (_, design, _) = match board::load(dir) {
                Ok(l) => l,
                Err(e) => return bad(board::reasons(&e)),
            };
            let out: BTreeMap<String, Value> = buffered_symbols(&design, lib).into_iter().map(|s| (s.lib_id.clone(), json!(saved_symbol(&entry.path, &s.lib_id)))).collect();
            json!({ "symbols": out })
        }
        Kind::Symbol => json!({ "symbol": saved_symbol(&entry.path, id) }),
        Kind::Footprint => json!({ "footprint": saved_footprint(&entry.path, id) }),
    }
}

fn parse_body(body: &[u8]) -> Result<Value, Value> {
    serde_json::from_slice(body).map_err(|e| bad(format!("bad request: {e}")))
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// `POST /api/library/save` -- Save in the two library editors.
///  * `{kind: "symbol", lib}`: `SYMBOL_EDIT_FRAME::saveLibrary( lib, false )`: the modified symbols of the library into its file. A library that cannot be
///    written answers `{error: "Symbol library '%s' is not writable.", detail: "You must save to a different location.", read_only: true}` (the question
///    `Save` asks before it offers Save Library As); a library the tables do not have, "No library specified.".
///  * `{kind: "footprint", id: "Lib:Name"}`: `FOOTPRINT_EDIT_FRAME::SaveFootprint` -> `SaveFootprintInLibrary`: the footprint into its library's folder.
///
/// `{ok, path, written: [files], message}` or `{error, detail?}`.
pub(crate) fn save(dir: &Path, body: &[u8]) -> Value {
    let req = match parse_body(body) {
        Ok(r) => r,
        Err(e) => return e,
    };
    if let Err(e) = library_tables::refuse_kicads_configuration(&Env::discover(), dir) {
        return bad(e);
    }
    let (_, design, _) = match board::load(dir) {
        Ok(l) => l,
        Err(e) => return bad(board::reasons(&e)),
    };
    match text(&req, "kind") {
        "symbol" => {
            let lib = text(&req, "lib");
            let Some(entry) = lib_entry(dir, Kind::Symbol, lib) else { return bad("No library specified.") };
            if !ls::is_symbol_library_writable(&entry.path) {
                return json!({ "error": format!("Symbol library '{lib}' is not writable."), "detail": "You must save to a different location.", "read_only": true });
            }
            let symbols = symbols_to_save(&design, &entry.path, lib);
            let path = entry.path.display().to_string();
            let written = if symbols.is_empty() {
                Vec::new()
            } else {
                match ls::save_symbols(&entry.path, &saves(&design, &symbols)) {
                    Ok(w) => w.written,
                    Err(e) => return json!({ "error": format!("Failed to save changes to symbol library file '{path}'."), "detail": e }),
                }
            };
            json!({ "ok": true, "path": path, "written": written, "saved": symbols.iter().map(|s| &s.lib_id).collect::<Vec<_>>(), "message": format!("Symbol library file '{path}' saved.") })
        }
        "footprint" => {
            let id = text(&req, "id");
            let (lib, item) = split(id);
            let Some(entry) = lib_entry(dir, Kind::Footprint, lib) else { return bad(format!("Library '{lib}' not found.")) };
            let Some(fp) = design.footprint_library.as_ref().and_then(|l| l.by_name(id)) else { return bad(format!("Footprint '{id}' is not open in the Footprint Editor.")) };
            let embedded = design.footprint_library.as_ref().and_then(|l| l.embedded.get(id)).cloned().unwrap_or_default();
            match ls::save_footprint(&entry.path, fp, &embedded) {
                Ok(file) => json!({ "ok": true, "path": file.display().to_string(), "written": [file], "message": format!("Footprint '{item}' saved in '{lib}'.") }),
                Err(e) => bad(e),
            }
        }
        other => bad(format!("kind is footprint or symbol, not {other:?}")),
    }
}

/// `POST /api/library/save_as` -- Save Library As.
///  * Symbols, `{kind: "symbol", lib, path, table: "project" | "replace", overwrite?}`: `SYMBOL_EDIT_FRAME::saveLibrary( lib, true )` -- every symbol of the
///    library, its modified ones as the design has them, into a new `.kicad_sym` file (a folder library is packed into one file, as `SaveLibrary` writes the
///    cache to a file path), then the library table: "Add new project library table entry" (`addLibTableEntry`) or "Update existing library table entry"
///    (`replaceLibTableEntry`). An existing file is replaced only when `overwrite` (the file dialog's `wxFD_OVERWRITE_PROMPT`: `{error, confirm: "Replace"}`
///    asks first). The project library (`eda`) is saved as a library file of its own.
///  * Footprints, `{kind: "footprint", lib, path, nickname?, descr?}`: `FOOTPRINT_EDIT_FRAME::SaveLibraryAs` -- `CreateNewLibrary` (the new `.pretty` and its
///    row in the board's table, `crate::library_tables::add_library`) and every footprint of the library copied into it; "Footprint library '%s' saved as '%s'.".
///
/// The new path is written into the board's table in the same request, so the studio writes no library file outside the tables' paths.
pub(crate) fn save_library_as(dir: &Path, body: &[u8]) -> Value {
    let req = match parse_body(body) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let env = Env::discover();
    if let Err(e) = library_tables::refuse_kicads_configuration(&env, dir) {
        return bad(e);
    }
    let (_, design, _) = match board::load(dir) {
        Ok(l) => l,
        Err(e) => return bad(board::reasons(&e)),
    };
    let lib = text(&req, "lib");
    let typed = text(&req, "path").trim();
    if typed.is_empty() {
        return bad("Give the library a file name.");
    }
    let vars = env.vars(dir, &crate::text_variables::project_variables(dir));
    match text(&req, "kind") {
        "symbol" => {
            let mut dst = vars.expand_uri(typed, dir);
            // `if( fn.GetExt().IsEmpty() ) fn.SetExt( FILEEXT::KiCadSymbolLibFileExtension )`
            if dst.extension().is_none() {
                dst.set_extension(eda_kicad::SYMBOL_LIB_FILE_EXT);
            }
            if dst.extension().and_then(|e| e.to_str()) != Some(eda_kicad::SYMBOL_LIB_FILE_EXT) {
                return bad(format!("'{}' is not a symbol library file name: it ends in .kicad_sym.", dst.display()));
            }
            if let Err(e) = library_tables::refuse_inside_kicads_configuration(&env, &dst) {
                return bad(e);
            }
            let replace = match text(&req, "table") {
                "replace" => Some(lib),
                "project" | "" => None,
                other => return bad(format!("table is project or replace, not {other:?}")),
            };
            let src = if lib == PROJECT_LIBRARY {
                None
            } else {
                match lib_entry(dir, Kind::Symbol, lib) {
                    Some(e) => Some(e.path),
                    None => return bad("No library specified."),
                }
            };
            if dst.is_dir() {
                return bad(format!("'{}' is a folder.", dst.display()));
            }
            if dst.exists() && src.as_deref() != Some(dst.as_path()) {
                let name = dst.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if req.get("overwrite").and_then(Value::as_bool) != Some(true) {
                    return json!({ "error": format!("A file named \"{name}\" already exists. Do you want to replace it?"), "confirm": "Replace" });
                }
                // a file that is not a symbol library is not the person's library to replace
                if !std::fs::read_to_string(&dst).is_ok_and(|t| t.trim_start().starts_with("(kicad_symbol_lib")) {
                    return bad(format!("'{}' is not a symbol library; choose another name.", dst.display()));
                }
            }
            let symbols: Vec<LibrarySymbol> = match &src {
                Some(path) => symbols_to_save(&design, path, lib),
                None => buffered_symbols(&design, PROJECT_LIBRARY).into_iter().cloned().collect(),
            };
            let shown = dst.display().to_string();
            if let Err(e) = ls::save_symbol_library_as(src.as_deref(), &dst, &saves(&design, &symbols)) {
                return json!({ "error": format!("Failed to save changes to symbol library file '{shown}'."), "detail": e });
            }
            if replace == Some(PROJECT_LIBRARY) {
                return bad("The project library has no library table entry to update: add the new library to the project's table instead.");
            }
            match library_tables::put_library_row(dir, eda_kicad::library_table::TableKind::Symbol, &dst, replace) {
                Ok((nickname, uri)) => json!({ "ok": true, "path": shown, "nickname": nickname, "uri": uri, "message": format!("Symbol library file '{shown}' saved.") }),
                Err(e) => json!({ "error": format!("Error saving library table.\n{e}"), "path": shown }),
            }
        }
        "footprint" => {
            let src = if lib == PROJECT_LIBRARY {
                None
            } else {
                match lib_entry(dir, Kind::Footprint, lib) {
                    Some(e) => Some(e.path),
                    None => return bad(format!("Library '{lib}' not found.")),
                }
            };
            let mut add = json!({ "kind": "footprint", "path": typed, "create": true });
            for key in ["nickname", "descr"] {
                if !text(&req, key).is_empty() {
                    add[key] = json!(text(&req, key));
                }
            }
            let added = library_tables::add_library_with(&env, dir, add.to_string().as_bytes());
            if added.get("ok").and_then(Value::as_bool) != Some(true) {
                return added;
            }
            let dst = PathBuf::from(added["added"]["path"].as_str().unwrap_or_default());
            let nickname = added["added"]["nickname"].as_str().unwrap_or_default().to_string();
            let copied = match &src {
                Some(path) => ls::copy_footprint_library(path, &dst),
                None => {
                    let mut names = Vec::new();
                    let lib = design.footprint_library.as_ref();
                    let mut out = Ok(());
                    for fp in lib.iter().flat_map(|l| l.footprints.iter()).filter(|f| !f.name.contains(':')) {
                        let embedded = lib.and_then(|l| l.embedded.get(&fp.name)).cloned().unwrap_or_default();
                        if let Err(e) = ls::save_footprint(&dst, fp, &embedded) {
                            out = Err(e);
                            break;
                        }
                        names.push(fp.name.clone());
                    }
                    out.map(|()| names)
                }
            };
            let from = src.as_ref().map_or_else(|| PROJECT_LIBRARY.to_string(), |p| p.display().to_string());
            match copied {
                Ok(names) => json!({ "ok": true, "path": dst.display().to_string(), "nickname": nickname, "copied": names, "message": format!("Footprint library '{from}' saved as '{}'.", dst.display()) }),
                Err(e) => json!({ "error": "Error saving footprint library", "detail": e, "path": dst.display().to_string(), "nickname": nickname }),
            }
        }
        other => bad(format!("kind is footprint or symbol, not {other:?}")),
    }
}

#[cfg(test)]
#[path = "library_save_tests.rs"]
mod tests;
