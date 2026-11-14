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
//!
//! Delete and Rename of a library's item mark it in the design (`SymbolLibrarySection::deleted`, `FootprintLibrarySection::deleted`; KiCad's
//! `LIB_BUFFER::m_deleted` and a renamed buffer's original name), and Save takes the marked items out of the library's file or folder (UI-LEFT-2 LL1,
//! `crate::library_buffer`). A modified item is one edited since it was opened, saved or reverted (`crate::library_buffer::is_dirty`, KiCad's flag), or whose
//! copy is not what the file holds.

use crate::board;
use crate::library_index::Kind;
use crate::library_tables::{self, Env, LibEntry};
use eda_kicad::library_save::{self as ls, SymbolSave};
use eda_kicad::SymbolSummary;
use eda_model::ir::{Design, LibraryFootprint, LibrarySymbol, SymbolLibrarySection};
use eda_ops::Cmd;
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

pub(crate) fn saved_library(path: &Path) -> Option<(Arc<String>, Arc<Vec<SymbolSummary>>)> {
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
pub(crate) fn saved_symbol(path: &Path, lib_id: &str) -> Option<LibrarySymbol> {
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

/// [`saved_footprint`] of the library the board's tables nickname `Lib` (`FootprintExists`, `LoadFootprint`).
pub(crate) fn saved_footprint_of(dir: &Path, name: &str) -> Option<LibraryFootprint> {
    saved_footprint(&lib_entry(dir, Kind::Footprint, split(name).0)?.path, name)
}

/// Whether the design's symbol `sym` is what its library's file holds (opening a symbol copies the file's in: not a modification).
pub(crate) fn symbol_matches_file(dir: &Path, sym: &LibrarySymbol, section: Option<&SymbolLibrarySection>) -> bool {
    let Some(entry) = lib_entry(dir, Kind::Symbol, split(&sym.lib_id).0) else { return false };
    let ours = section.and_then(|l| flat_entry(l, &sym.lib_id)).unwrap_or_else(|| sym.clone());
    saved_symbol(&entry.path, &sym.lib_id).is_some_and(|saved| symbol_text(&saved) == symbol_text(&ours))
}

/// Whether the design's copy `id` is modified (see the module doc), from the library sections alone (the watcher asks every second; the model is not built).
pub(crate) fn item_modified(dir: &Path, kind: Kind, id: &str, symbols: Option<&SymbolLibrarySection>, footprints: Option<&eda_model::ir::FootprintLibrarySection>) -> bool {
    if crate::library_buffer::is_dirty(dir, kind, id) {
        return true;
    }
    match kind {
        Kind::Symbol => {
            let (Some(entry), Some(ours)) = (lib_entry(dir, kind, split(id).0), symbols.and_then(|l| flat_entry(l, id))) else { return false };
            saved_symbol(&entry.path, id).is_none_or(|saved| symbol_text(&saved) != symbol_text(&ours))
        }
        Kind::Footprint => footprints.and_then(|l| l.by_name(id)).is_some_and(|fp| !footprint_matches_file(dir, fp)),
    }
}

/// Whether the design's footprint `fp` is what its library's folder holds.
pub(crate) fn footprint_matches_file(dir: &Path, fp: &LibraryFootprint) -> bool {
    saved_footprint_of(dir, &fp.name).is_some_and(|saved| footprint_text(&saved) == footprint_text(fp))
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

/// The design's symbols of library `lib`, by `lib_id` (a bare `lib_id` is the project library's).
fn buffered_symbols<'a>(design: &'a Design, lib: &str) -> Vec<&'a LibrarySymbol> {
    let of = |id: &str| match split(id).0 {
        "" => PROJECT_LIBRARY.to_string(),
        l => l.to_string(),
    };
    design.symbol_library.as_ref().map(|l| l.symbols.iter().filter(|s| of(&s.lib_id) == lib).collect()).unwrap_or_default()
}

/// The design's symbols of library `lib` at `path` whose text differs from the file's symbol, or that the file lacks: what Save writes.
fn differing_symbols(design: &Design, path: &Path, lib: &str) -> Vec<String> {
    let Some(section) = design.symbol_library.as_ref() else { return Vec::new() };
    buffered_symbols(design, lib)
        .into_iter()
        .filter(|s| match (flat_entry(section, &s.lib_id), saved_symbol(path, &s.lib_id)) {
            // a derived symbol whose parent is another than the file's (its parent was renamed: KiCad links it by pointer, and `SaveLibrary` writes every buffer)
            (Some(ours), Some(saved)) => symbol_text(&ours) != symbol_text(&saved) || s.extends.as_deref().is_some_and(|p| saved_parent(path, split(&s.lib_id).1).as_deref() != Some(split(p).1)),
            _ => true,
        })
        .map(|s| s.lib_id.clone())
        .collect()
}

/// `SYMBOL_LIBRARY_MANAGER::IsSymbolModified` for the design's symbols of library `lib` at `path` (see the module doc): edited since they were opened, saved or
/// reverted, or not what the file holds.
fn modified_symbols(dir: &Path, design: &Design, path: &Path, lib: &str) -> Vec<String> {
    let differing = differing_symbols(design, path, lib);
    buffered_symbols(design, lib)
        .into_iter()
        .map(|s| s.lib_id.clone())
        .filter(|id| differing.contains(id) || crate::library_buffer::is_dirty(dir, Kind::Symbol, id))
        .collect()
}

/// The marks of library `lib` (`deleted`) that its file still holds: what its next Save takes out (`GetDeletedBuffers`), as bare names. A mark the file no
/// longer holds (saved already, or gone from the file) is passed over.
fn live_symbol_marks(design: &Design, path: &Path, lib: &str) -> Vec<String> {
    let marks: Vec<&String> = design.symbol_library.iter().flat_map(|l| l.deleted.iter()).filter(|id| split(id).0 == lib).collect();
    if marks.is_empty() {
        return Vec::new();
    }
    let Some((_, summaries)) = saved_library(path) else { return Vec::new() };
    marks.into_iter().map(|id| split(id).1.to_string()).filter(|n| summaries.iter().any(|s| &s.name == n)).collect()
}

/// The footprint marks of library `lib` whose file is still in its folder (`Lib:Name`).
fn live_footprint_marks(design: &Design, path: &Path, lib: &str) -> Vec<String> {
    design.footprint_library.iter().flat_map(|l| l.deleted.iter()).filter(|id| split(id).0 == lib && path.join(ls::footprint_file_name(split(id).1)).is_file()).cloned().collect()
}

/// The modified symbol `lib_id` as Save writes it: as it is when it is derived in the design or is new to the file; when the file had it derived from a
/// parent and the design has it flat (opened flat), derived again if that loses nothing (see the module doc).
fn as_saved(section: &SymbolLibrarySection, path: &Path, lib_id: &str) -> Option<LibrarySymbol> {
    let sym = section.by_lib_id(lib_id)?.clone();
    if sym.extends.is_some() {
        return Some(sym);
    }
    let (lib, _) = split(lib_id);
    // the file's symbol it stands for: a renamed symbol's original (UI-LEFT-3 L3-2: KiCad's renamed buffer keeps its parent, `LIB_BUFFER::SaveBuffer`
    // writes it derived), and its parent by the name the design has it under now (a parent renamed too: KiCad links them by pointer)
    let summaries = saved_library(path).map(|(_, s)| s).unwrap_or_default();
    let from = crate::library_buffer::original_name(Some(section), &summaries, lib_id);
    let Some(parent) = saved_parent(path, split(&from).1) else { return Some(sym) };
    let parent_file_id = format!("{lib}:{parent}");
    let parent_id = section
        .symbols
        .iter()
        .find(|s| s.lib_id != lib_id && crate::library_buffer::original_name(Some(section), &summaries, &s.lib_id) == parent_file_id)
        .map_or(parent_file_id, |s| s.lib_id.clone());
    // a parent this Save takes out of the library (Delete, Rename) is not there to derive from: the symbol is written flat
    if section.deleted.contains(&parent_id) {
        return Some(sym);
    }
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

/// What Save writes of library `lib` at `path`: each symbol that differs from the file (see [`as_saved`]).
fn symbols_to_save(design: &Design, path: &Path, lib: &str) -> Vec<LibrarySymbol> {
    let Some(section) = design.symbol_library.as_ref() else { return Vec::new() };
    differing_symbols(design, path, lib).iter().filter_map(|id| as_saved(section, path, id)).collect()
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
    // the libraries whose next Save takes a deleted or renamed item out of their file (`LIB_BUFFER::IsModified` counts `m_deleted` too: `GetHash`)
    let mut marked: BTreeSet<String> = BTreeSet::new();
    match kind {
        Kind::Symbol => {
            let names: BTreeSet<&str> = design.symbol_library.iter().flat_map(|l| l.symbols.iter().map(|s| split(&s.lib_id).0).chain(l.deleted.iter().map(|d| split(d).0))).collect();
            for lib in names {
                if let Some(e) = (lib != PROJECT_LIBRARY).then(|| libs.find(lib)).flatten() {
                    items.extend(modified_symbols(dir, &design, &e.path, lib));
                    if !live_symbol_marks(&design, &e.path, lib).is_empty() {
                        marked.insert(lib.to_string());
                    }
                }
            }
        }
        Kind::Footprint => {
            for fp in design.footprint_library.iter().flat_map(|l| l.footprints.iter()) {
                let (lib, _) = split(&fp.name);
                if let Some(e) = (!lib.is_empty() && lib != PROJECT_LIBRARY).then(|| libs.find(lib)).flatten() {
                    if crate::library_buffer::is_dirty(dir, Kind::Footprint, &fp.name) || saved_footprint(&e.path, &fp.name).is_none_or(|saved| footprint_text(&saved) != footprint_text(fp)) {
                        items.push(fp.name.clone());
                    }
                }
            }
            for id in design.footprint_library.iter().flat_map(|l| l.deleted.iter()) {
                if let Some(e) = libs.find(split(id).0) {
                    if e.path.join(ls::footprint_file_name(split(id).1)).is_file() {
                        marked.insert(split(id).0.to_string());
                    }
                }
            }
        }
    }
    items.sort();
    let mut libraries: BTreeSet<String> = items.iter().map(|i| split(i).0.to_string()).collect();
    libraries.extend(marked);
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
///  * `{kind: "symbol", lib}`: `SYMBOL_EDIT_FRAME::saveLibrary( lib, false )`: the modified symbols of the library into its file, and the ones Delete and
///    Rename took out of it taken out (`SaveLibrary`'s deleted buffers, `SaveBuffer`'s original name; UI-LEFT-2 LL1). A library that cannot be written answers
///    `{error: "Symbol library '%s' is not writable.", detail: "You must save to a different location.", read_only: true}` (the question `Save` asks before
///    it offers Save Library As); a library the tables do not have, "No library specified.".
///  * `{kind: "footprint", id: "Lib:Name"}`: `FOOTPRINT_EDIT_FRAME::SaveFootprint` -> `SaveFootprintInLibrary`: the footprint into its library's folder, after
///    the footprints of that library that Delete or Rename took out of it are deleted (`if( nameChanged ) DeleteFootprintFromLibrary( oldFPID, false )`).
///    `{kind: "footprint", lib}` deletes those alone: `DeleteFootprintFromLibrary` and the rename of a footprint that is not loaded, which write at once.
///
/// `{ok, path, written: [files], removed: [files], saved: [ids], deleted: [ids], message}` or `{error, detail?}`. What was written is no longer modified
/// (`crate::library_buffer::clear`), and the watcher knows the files are the studio's own (`crate::library_watch::note_written`).
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
            let deleted = live_symbol_marks(&design, &entry.path, lib);
            let path = entry.path.display().to_string();
            let write = if symbols.is_empty() && deleted.is_empty() {
                ls::LibraryWrite::default()
            } else {
                // "Copy .kicad_symb file to .bak."
                if let Err(e) = ls::backup_file(&entry.path) {
                    return bad(e);
                }
                let gone: Vec<&str> = deleted.iter().map(String::as_str).collect();
                match ls::save_symbols_deleting(&entry.path, &saves(&design, &symbols), &gone) {
                    Ok(w) => w,
                    Err(e) => return json!({ "error": format!("Failed to save changes to symbol library file '{path}'."), "detail": e }),
                }
            };
            crate::library_watch::note_written(Kind::Symbol, &entry.path);
            // every symbol of the library is saved now: none is modified (`SaveLibrary` saves every buffer, `ClearDeletedBuffer`)
            let ids: Vec<String> = buffered_symbols(&design, lib).iter().map(|s| s.lib_id.clone()).collect();
            crate::library_buffer::clear(dir, Kind::Symbol, &ids, true);
            let deleted: Vec<String> = deleted.iter().map(|n| format!("{lib}:{n}")).collect();
            json!({ "ok": true, "path": path, "written": write.written, "removed": write.removed, "saved": symbols.iter().map(|s| &s.lib_id).collect::<Vec<_>>(), "deleted": deleted, "message": format!("Symbol library file '{path}' saved.") })
        }
        "footprint" => {
            let id = text(&req, "id");
            let lib = if id.is_empty() { text(&req, "lib") } else { split(id).0 };
            let item = split(id).1;
            let Some(entry) = lib_entry(dir, Kind::Footprint, lib) else { return bad(format!("Library '{lib}' not found.")) };
            let fp = if id.is_empty() {
                None
            } else {
                match design.footprint_library.as_ref().and_then(|l| l.by_name(id)) {
                    Some(fp) => Some(fp),
                    None => return bad(format!("Footprint '{id}' is not open in the Footprint Editor.")),
                }
            };
            // the old names first (`SaveFootprint`: `DeleteFootprintFromLibrary( oldFPID, false )` before `SaveFootprintInLibrary`)
            let marks = live_footprint_marks(&design, &entry.path, lib);
            let gone: Vec<&str> = marks.iter().map(String::as_str).collect();
            let removed = match ls::delete_footprints(&entry.path, &gone) {
                Ok(r) => r,
                Err(e) => return bad(e),
            };
            let mut out = json!({ "ok": true, "path": entry.path.display().to_string(), "written": [], "removed": removed, "deleted": marks, "message": "" });
            if let Some(fp) = fp {
                let embedded = design.footprint_library.as_ref().and_then(|l| l.embedded.get(id)).cloned().unwrap_or_default();
                match ls::save_footprint(&entry.path, fp, &embedded) {
                    Ok(file) => {
                        out["path"] = json!(file.display().to_string());
                        out["written"] = json!([file]);
                        out["message"] = json!(format!("Footprint '{item}' saved in '{lib}'."));
                        crate::library_buffer::clear(dir, Kind::Footprint, &[id.to_string()], true);
                    }
                    Err(e) => return bad(e),
                }
            } else if let [one] = marks.as_slice() {
                // `DeleteFootprintFromLibrary`'s status text
                out["message"] = json!(format!("Footprint '{}' deleted from library '{lib}'", split(one).1));
            }
            crate::library_watch::note_written(Kind::Footprint, &entry.path);
            out
        }
        other => bad(format!("kind is footprint or symbol, not {other:?}")),
    }
}

/// `POST /api/library/save_as` -- Save Library As.
///  * Symbols, `{kind: "symbol", lib, path, table: "none" | "replace" | "global" | "project", mode?: "packed" | "unpacked", overwrite?}`:
///    `SYMBOL_EDIT_FRAME::saveLibrary( lib, true )` -- every symbol of the library, its modified ones as the design has them, into a new `.kicad_sym` file (a
///    folder library is packed into one file, as `SaveLibrary` writes the cache to a file path), then the library table as `SYMBOL_LIBRARY_SAVE_AS_FILEDLG_HOOK`
///    says: "Do not update library tables" (`none`, the hook's default), "Update existing library table entry" (`replaceLibTableEntry`), "Add new global
///    library table entry" or "Add new project library table entry" (`addLibTableEntry( file, GLOBAL | PROJECT )`; UI-LEFT-2 LL5). An existing file is replaced
///    only when `overwrite` (the file dialog's `wxFD_OVERWRITE_PROMPT`: `{error, confirm: "Replace"}` asks first). The project library (`eda`) is saved as a
///    library file of its own. KiCad 25ec2335's Save Library As Packed (`mode: "packed"`) is the same file save of a folder library; Save Library As
///    Unpacked (`mode: "unpacked"`, a file library) writes every symbol into a file of its own in the folder `path` (`wxDirDialog`, no table choice: the tables
///    are not updated).
///  * Footprints, `{kind: "footprint", lib, path, nickname?, descr?, global?}`: `FOOTPRINT_EDIT_FRAME::SaveLibraryAs` -- `CreateNewLibrary` (the new `.pretty`
///    and its row in the board's table, or KiCad's global one with `global`: `FILEDLG_HOOK_NEW_LIBRARY`, `crate::library_tables::add_library`) and every
///    footprint of the library copied into it; "Footprint library '%s' saved as '%s'.".
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
        "symbol" if text(&req, "mode") == "unpacked" => {
            let dst = vars.expand_uri(typed, dir);
            if let Err(e) = library_tables::refuse_inside_kicads_configuration(&env, &dst) {
                return bad(e);
            }
            let src = match lib_entry(dir, Kind::Symbol, lib) {
                Some(e) => e.path,
                None => return bad("No library specified."),
            };
            if !src.is_file() {
                return bad(format!("Library '{lib}' is not a packed (single file) library."));
            }
            if dst.is_file() {
                return bad(format!("'{}' is a file: choose a folder.", dst.display()));
            }
            let symbols = symbols_to_save(&design, &src, lib);
            let deleted = live_symbol_marks(&design, &src, lib);
            let gone: Vec<&str> = deleted.iter().map(String::as_str).collect();
            let shown = dst.display().to_string();
            match ls::save_symbol_library_unpacked(&src, &dst, &saves(&design, &symbols), &gone) {
                Ok(w) => json!({ "ok": true, "path": shown, "nickname": Value::Null, "written": w.written, "message": format!("Symbol library file '{shown}' saved.") }),
                Err(e) => json!({ "error": format!("Failed to save changes to symbol library file '{shown}'."), "detail": e }),
            }
        }
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
            // `SYMBOL_SAVEAS_TYPE`: the hook's choice, "Do not update library tables" when none is given
            let (update, replace, global) = match text(&req, "table") {
                "none" | "" => (false, None, false),
                "replace" => (true, Some(lib), false),
                "global" => (true, None, true),
                "project" => (true, None, false),
                other => return bad(format!("table is none, replace, global or project, not {other:?}")),
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
            if let Err(e) = ls::backup_file(&dst) {
                return bad(e);
            }
            let deleted = src.as_deref().map(|p| live_symbol_marks(&design, p, lib)).unwrap_or_default();
            let gone: Vec<&str> = deleted.iter().map(String::as_str).collect();
            if let Err(e) = ls::save_symbol_library_as_deleting(src.as_deref(), &dst, &saves(&design, &symbols), &gone) {
                return json!({ "error": format!("Failed to save changes to symbol library file '{shown}'."), "detail": e });
            }
            if replace == Some(PROJECT_LIBRARY) {
                return bad("The project library has no library table entry to update: add the new library to the project's table instead.");
            }
            if !update {
                return json!({ "ok": true, "path": shown, "nickname": Value::Null, "message": format!("Symbol library file '{shown}' saved.") });
            }
            match library_tables::put_library_row(dir, eda_kicad::library_table::TableKind::Symbol, &dst, replace, global) {
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
            // an existing folder is asked about and replaced on Overwrite (`createNewLibrary`, `crate::library_tables::add_library_with`)
            let mut add = json!({ "kind": "footprint", "path": typed, "create": true, "overwrite": req.get("overwrite").and_then(Value::as_bool) == Some(true), "global": req.get("global").and_then(Value::as_bool) == Some(true) });
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

/// `POST /api/library/revert` -- Revert in the two library editors, and the reload of a library changed on disk (UI-LEFT-2 LL3), as one undoable step
/// (`Cmd::ForgetLibrarySymbols` / `ForgetLibraryFootprints` and the file's items put back, one batch):
///  * `{kind: "symbol", id}`: `SYMBOL_EDIT_FRAME::Revert` -> `SYMBOL_LIBRARY_MANAGER::RevertSymbol`: the symbol goes back to what the library's file holds
///    (`GetOriginal`), and one the file never had to the symbol as it was made (`crate::library_buffer::original`; KiCad's buffer of a new symbol keeps it);
///  * `{kind: "symbol", lib}`: `RevertLibrary`, and `UpdateLibraryBuffer` (the reload): every copy of the library goes back to the file's, the ones the file
///    never had go, and what Delete and Rename took out of it is back (`m_libs.erase( it )`: the buffer and its deleted list);
///  * `{kind: "footprint", id}`: `RevertFootprint`, and `OnFpChangeDebounceTimer`'s reload (`LoadFootprint` -> `ReloadFootprint`): the footprint back to its file.
///
/// What was reverted is no longer modified. `{ok, reverted: [ids], gone: [ids], message}` (`gone`: copies the file never had, which left the design), `{nothing:
/// true, message}` when there is no saved version to go back to, or `{error}`.
pub(crate) fn revert(dir: &Path, body: &[u8]) -> Value {
    let req = match parse_body(body) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let (_, design, _) = match board::load(dir) {
        Ok(l) => l,
        Err(e) => return bad(board::reasons(&e)),
    };
    let id = text(&req, "id");
    let (cmds, reverted, gone, kind): (Vec<Cmd>, Vec<String>, Vec<String>, Kind) = match text(&req, "kind") {
        "symbol" => {
            let lib = if id.is_empty() { text(&req, "lib") } else { split(id).0 };
            let Some(entry) = lib_entry(dir, Kind::Symbol, lib) else { return bad(format!("Symbol library '{lib}' not found.")) };
            if id.is_empty() {
                let mut ids: Vec<String> = buffered_symbols(&design, lib).iter().map(|s| s.lib_id.clone()).collect();
                ids.extend(design.symbol_library.iter().flat_map(|l| l.deleted.iter()).filter(|d| split(d).0 == lib).cloned());
                let mut puts = Vec::new();
                let mut gone = Vec::new();
                for i in &ids {
                    match saved_symbol(&entry.path, i) {
                        Some(sym) if buffered_symbols(&design, lib).iter().any(|s| &s.lib_id == i) => puts.push(Cmd::PutLibrarySymbol { symbol: sym, overwrite: true }),
                        Some(_) => {}
                        None => gone.push(i.clone()),
                    }
                }
                let mut cmds = vec![Cmd::ForgetLibrarySymbols { lib_ids: ids.clone() }];
                cmds.extend(puts);
                (cmds, ids, gone, Kind::Symbol)
            } else {
                // `RevertSymbol`: the buffer's original, under its own name -- a renamed symbol goes back to the name it had (`UpdateSymbolAfterRename(
                // &original, aSymbolName, aLibrary )`, UI-LEFT-3 L3-1), and the symbols derived from it follow it back (the rename's `follow_symbol_rename`)
                let summaries = saved_library(&entry.path).map(|(_, s)| s).unwrap_or_default();
                let to = crate::library_buffer::original_name(design.symbol_library.as_ref(), &summaries, id);
                let back = match saved_symbol(&entry.path, &to).or_else(|| crate::library_buffer::original(dir, &to)) {
                    Some(sym) => sym,
                    None => return json!({ "nothing": true, "message": format!("'{}' was never saved to {lib}: there is no saved version to revert to.", split(id).1) }),
                };
                if to == id {
                    (vec![Cmd::ForgetLibrarySymbols { lib_ids: vec![id.to_string()] }, Cmd::PutLibrarySymbol { symbol: back, overwrite: true }], vec![id.to_string()], Vec::new(), Kind::Symbol)
                } else {
                    // the new name leaves the tree (the open symbol leaves the canvas: `SymbolExists( curr_symbolName )` is false after the revert)
                    let cmds = vec![Cmd::RenameLibrarySymbol { lib_id: id.to_string(), new_lib_id: to.clone(), overwrite: true }, Cmd::PutLibrarySymbol { symbol: back, overwrite: true }];
                    (cmds, vec![id.to_string(), to], vec![id.to_string()], Kind::Symbol)
                }
            }
        }
        "footprint" => {
            let Some(back) = saved_footprint_of(dir, id) else {
                return json!({ "nothing": true, "message": format!("'{}' was never saved to {}: there is no saved version to revert to.", split(id).1, split(id).0) });
            };
            (vec![Cmd::ForgetLibraryFootprints { names: vec![id.to_string()] }, Cmd::PutLibraryFootprint { footprint: back, overwrite: true }], vec![id.to_string()], Vec::new(), Kind::Footprint)
        }
        other => return bad(format!("kind is footprint or symbol, not {other:?}")),
    };
    // the id the tree selects after the revert (`SelectLibId( libId )`): a renamed symbol's original name
    let lib_id = if kind == Kind::Symbol && !id.is_empty() { reverted.last().cloned() } else { None };
    match board::step(dir, Cmd::Batch { cmds }, false, "ui") {
        Ok(summary) => {
            crate::library_buffer::clear(dir, kind, &reverted, false);
            json!({ "ok": true, "reverted": reverted, "gone": gone, "lib_id": lib_id, "message": summary })
        }
        Err(e) => bad(board::reasons(&e)),
    }
}

/// `GET /api/library/derived?id=Lib:Name` -- `LIB_SYMBOL_LIBRARY_MANAGER::GetDerivedSymbolNames` over the library's buffer: the bare names of the symbols that
/// derive from the symbol, at any depth, in the design's copies and in the library's file (less what Delete took out), for Delete's question ("Deleting a
/// base symbol will delete all symbols derived from it."). `{"names": [..]}`.
pub(crate) fn derived(dir: &Path, id: &str) -> Value {
    let (_, design, _) = match board::load(dir) {
        Ok(l) => l,
        Err(e) => return bad(board::reasons(&e)),
    };
    let (lib, name) = split(id);
    let section = design.symbol_library.as_ref();
    let mut names: Vec<String> = section.map(|l| l.derived_names(id)).unwrap_or_default().iter().map(|d| split(d).1.to_string()).collect();
    if let Some(entry) = lib_entry(dir, Kind::Symbol, lib) {
        if let Some((_, summaries)) = saved_library(&entry.path) {
            for child in crate::library_buffer::file_children(&summaries, name) {
                let child_id = format!("{lib}:{child}");
                // (a copy of it the design holds flat goes too: the Delete's rewrite takes every child the file has, `crate::library_buffer`)
                let marked = section.is_some_and(|l| l.deleted.contains(&child_id));
                if !marked && !names.contains(&child) {
                    names.push(child);
                }
            }
        }
    }
    json!({ "names": names })
}

#[cfg(test)]
#[path = "library_save_tests.rs"]
mod tests;
