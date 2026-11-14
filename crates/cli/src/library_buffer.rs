//! What KiCad's library managers keep about a library beside the items themselves, for the libraries of the library tables (UI-LEFT-2 LL1, LL2, LL6).
//!
//! The library editors edit the design's copy of a library's symbol or footprint (`design.symbol_library`, `design.footprint_library`, keyed `Lib:Name`),
//! which is KiCad's library buffer (`LIB_SYMBOL_LIBRARY_MANAGER`'s `LIB_BUFFER` and `SYMBOL_BUFFER`, eeschema/symbol_library_manager.cpp; the Footprint
//! Editor's loaded footprint). Three things the copies do not say on their own live here:
//!
//!   * **Delete and Rename of an item the library holds** ([`with_library_items`]): the ops verbs mark the item for its library's next Save to take out of the
//!     file (`SymbolLibrarySection::deleted`, KiCad's `LIB_BUFFER::m_deleted` and a renamed buffer's original name), but they read no library. The server, which
//!     knows where the libraries are, rewrites the command before it is applied (`board::step_quiet`, as `crate::library_place` does for placing): a Delete takes
//!     the symbols the library derives from the symbol with it (`LIB_BUFFER::DeleteBuffer`: "Remove all derived symbols to prevent broken inheritance"), a
//!     Rename of an item the design has no copy of starts from the library's (`RenameSymbol` renames the buffer, which holds the whole library;
//!     `RenameFootprint` loads the footprint), the symbols derived from a renamed symbol follow it (KiCad links a derived symbol to its parent by pointer), a
//!     name the library already holds is refused unless the person said Overwrite, and New Footprint in a library takes the attributes of the library's last
//!     footprint (`PCB_BASE_FRAME::CreateNewFootprint`).
//!   * **Whether an item is modified** ([`note_step`], [`is_dirty`]): KiCad's flag per buffer (`SYMBOL_BUFFER::IsModified`, its screen's `IsContentModified()`;
//!     the Footprint Editor's screen), set by every edit and by Undo and Redo and cleared by Save and Revert. The studio's server keeps it per item from the
//!     steps it makes, beside "the copy's text differs from the file's" (`crate::library_save_api::modified`), so an item edited and edited back -- by hand or
//!     by Undo -- stays modified as in KiCad.
//!   * **The symbol as it was made** ([`original`]): a buffer KiCad makes for a new symbol (`LIB_BUFFER::CreateBuffer`: `m_original` is a copy of it) is
//!     what Revert puts back for a symbol never saved (`SYMBOL_LIBRARY_MANAGER::RevertSymbol`). Kept with the flag, for the same time.
//!   * **The name a renamed symbol had** ([`original_name`], UI-LEFT-3 L3-1, L3-2): KiCad's buffer keeps its original under the old name through a rename
//!     (`UpdateSymbolAfterRename` changes `GetSymbol()` only), Revert goes back to it, name and all (`RevertSymbol`: `UpdateSymbolAfterRename( &original,
//!     aSymbolName, .. )`), and a derived symbol stays derived (its buffer keeps the parent's pointer). The design keeps the names a symbol went by
//!     (`SymbolLibrarySection::renamed`, undone and redone with the rename); the first of them the library's file holds is the original.
//!
//! The flag and the made symbols are kept with the design's history (`.history/library-buffers.json`), so they last as the design's copies and its Undo do --
//! across a reload of the page and a restart of the server (UI-LEFT-3 L3-4: KiCad's last as long as its frame keeps the buffers).

use crate::library_index::Kind;
use crate::library_save_api as lsa;
use crate::library_tables;
use eda_model::ir::{Design, FootprintLibrarySection, LibrarySymbol, SymbolLibrarySection};
use eda_model::CheckResult;
use eda_ops::Cmd;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};

fn split(id: &str) -> (&str, &str) {
    id.split_once(':').unwrap_or(("", id))
}

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

// ------------------------------------------------------------------------------------------------ the rewrites

/// The library file `lib`'s symbol summaries (name and parent), or none when the tables have no such library or it does not read.
fn file_symbols(dir: &Path, lib: &str) -> Option<(PathBuf, std::sync::Arc<String>, std::sync::Arc<Vec<eda_kicad::SymbolSummary>>)> {
    let entry = library_tables::libs(dir, Kind::Symbol).find(lib).cloned()?;
    let (text, summaries) = lsa::saved_library(&entry.path)?;
    Some((entry.path, text, summaries))
}

/// `LIB_BUFFER::GetDerivedSymbolNames` over the library's file: the bare names of the symbols that extend `name`, at any depth, in the file's order.
pub(crate) fn file_children(summaries: &[eda_kicad::SymbolSummary], name: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut frontier: BTreeSet<String> = [name.to_string()].into();
    loop {
        let more: Vec<String> = summaries.iter().filter(|s| s.extends.as_ref().is_some_and(|p| frontier.contains(p)) && s.name != name && !out.contains(&s.name)).map(|s| s.name.clone()).collect();
        if more.is_empty() {
            return out;
        }
        frontier = more.iter().cloned().collect();
        out.extend(more);
    }
}

/// The file's symbol `name` of library `lib` as its own form says it: a derived one with its parent (`Lib:Parent`) and its own fields only (the reader's
/// `parse_library_symbols` of the one form), so that it can follow its parent's rename.
fn derived_from_file(lib: &str, text: &str, summary: &eda_kicad::SymbolSummary) -> Option<LibrarySymbol> {
    let form = eda_kicad::symbol_text(text, summary);
    let parsed = eda_kicad::parse_library_symbols(&format!("(kicad_symbol_lib (version 20250324)\n{form}\n)\n")).ok()?;
    let mut sym = parsed.symbols.into_iter().find(|s| s.lib_id == summary.name)?;
    sym.lib_id = format!("{lib}:{}", summary.name);
    sym.extends = sym.extends.map(|p| if p.contains(':') { p } else { format!("{lib}:{p}") });
    sym.assign_missing_ids();
    Some(sym)
}

/// `SymbolNameInUse( name, lib )` against the library's file (any case), the symbol itself aside.
fn file_has_symbol_in_any_case(summaries: &[eda_kicad::SymbolSummary], name: &str, except: &str) -> bool {
    summaries.iter().any(|s| s.name != except && s.name.to_lowercase() == name.to_lowercase())
}

/// The commands of the library editors that take an item out of a library of the tables or give it a new name, with what the library holds (see the module
/// doc), or `None` when there is nothing to add. A name neither the design nor the library has is refused, as KiCad finds no buffer for it.
pub(crate) fn with_library_items(dir: &Path, cmd: &Cmd, design: &Design) -> Result<Option<Cmd>, Vec<CheckResult>> {
    match cmd {
        Cmd::Batch { cmds } => {
            let mut changed = false;
            let mut out = Vec::with_capacity(cmds.len());
            for c in cmds {
                match with_library_items(dir, c, design)? {
                    Some(n) => {
                        changed = true;
                        out.push(n);
                    }
                    None => out.push(c.clone()),
                }
            }
            Ok(changed.then_some(Cmd::Batch { cmds: out }))
        }
        Cmd::DeleteLibrarySymbol { lib_id } if eda_ops::library_editors::is_table_library_item(lib_id) => delete_symbol(dir, lib_id, design),
        Cmd::RenameLibrarySymbol { lib_id, new_lib_id, overwrite } if eda_ops::library_editors::is_table_library_item(lib_id) => rename_symbol(dir, lib_id, new_lib_id, *overwrite, design),
        Cmd::DeleteLibraryFootprint { name } if eda_ops::library_editors::is_table_library_item(name) => {
            let held = design.footprint_library.as_ref().is_some_and(|l| l.by_name(name).is_some());
            if !held && lsa::saved_footprint_of(dir, name).is_none() {
                let (lib, item) = split(name);
                return Err(fail("ops_unknown_footprint", name, format!("Library '{lib}' has no footprint '{item}'.")));
            }
            Ok(None)
        }
        Cmd::RenameLibraryFootprint { name, new_name, overwrite } if eda_ops::library_editors::is_table_library_item(name) => rename_footprint(dir, name, new_name, *overwrite, design),
        Cmd::NewFootprint { name } if eda_ops::library_editors::is_table_library_item(name) => Ok(new_footprint_attributes(dir, name).map(|attributes| {
            Cmd::Batch { cmds: vec![cmd.clone(), Cmd::FpEditor(eda_ops::fp_editor::FpEditorCmd::SetAttributes { footprint: name.clone(), attributes })] }
        })),
        _ => Ok(None),
    }
}

/// `SYMBOL_LIBRARY_MANAGER::RemoveSymbol` -> `LIB_BUFFER::DeleteBuffer`: the symbol, and every symbol the library derives from it.
fn delete_symbol(dir: &Path, lib_id: &str, design: &Design) -> Result<Option<Cmd>, Vec<CheckResult>> {
    let (lib, name) = split(lib_id);
    let held = design.symbol_library.as_ref().is_some_and(|l| l.by_lib_id(lib_id).is_some());
    let file = file_symbols(dir, lib);
    let in_file = file.as_ref().is_some_and(|(_, _, s)| s.iter().any(|x| x.name == name));
    if !held && !in_file {
        return Err(fail("ops_unknown_symbol", lib_id, format!("Symbol library '{lib}' has no symbol '{name}'.")));
    }
    let children: Vec<String> = file.as_ref().map(|(_, _, s)| file_children(s, name)).unwrap_or_default();
    if children.is_empty() {
        return Ok(None);
    }
    let mut cmds = vec![Cmd::DeleteLibrarySymbol { lib_id: lib_id.to_string() }];
    cmds.extend(children.into_iter().map(|c| Cmd::DeleteLibrarySymbol { lib_id: format!("{lib}:{c}") }));
    Ok(Some(Cmd::Batch { cmds }))
}

/// `SYMBOL_EDITOR_CONTROL::RenameSymbol` of a symbol of a library of the tables: the library's symbol, when the design has no copy of it, and the symbols the
/// library derives from it (each as its own form says, derived), go into the design first, so the rename has them all to follow; a new name the library's file
/// already has, in any case, is refused unless `overwrite` ("Symbol '%s' already exists in library '%s'." asked first, KiCad's `KIDIALOG` with Overwrite).
fn rename_symbol(dir: &Path, lib_id: &str, new_lib_id: &str, overwrite: bool, design: &Design) -> Result<Option<Cmd>, Vec<CheckResult>> {
    let (lib, name) = split(lib_id);
    let section = design.symbol_library.as_ref();
    let held = |id: &str| section.is_some_and(|l| l.by_lib_id(id).is_some());
    let Some((path, text, summaries)) = file_symbols(dir, lib) else { return Ok(None) };
    let (_, new_name) = split(new_lib_id);
    if new_lib_id != lib_id && !overwrite && file_has_symbol_in_any_case(&summaries, new_name, name) && !new_name.eq_ignore_ascii_case(name) {
        return Err(fail("ops_symbol_exists", new_lib_id, format!("Symbol '{new_name}' already exists in library '{lib}'.")));
    }
    let mut cmds = Vec::new();
    if !held(lib_id) {
        let sym = lsa::saved_symbol(&path, lib_id).ok_or_else(|| fail("ops_unknown_symbol", lib_id, format!("Symbol library '{lib}' has no symbol '{name}'.")))?;
        cmds.push(Cmd::PutLibrarySymbol { symbol: sym, overwrite: true });
    }
    // the library's own children of it, parents first (`file_children` is breadth first), as derived symbols: the rename's `follow_symbol_rename` moves them
    for child in file_children(&summaries, name) {
        let child_id = format!("{lib}:{child}");
        if held(&child_id) {
            continue;
        }
        if let Some(sym) = summaries.iter().rev().find(|s| s.name == child).and_then(|s| derived_from_file(lib, &text, s)) {
            cmds.push(Cmd::PutLibrarySymbol { symbol: sym, overwrite: true });
        }
    }
    if cmds.is_empty() {
        return Ok(None);
    }
    cmds.push(Cmd::RenameLibrarySymbol { lib_id: lib_id.to_string(), new_lib_id: new_lib_id.to_string(), overwrite });
    Ok(Some(Cmd::Batch { cmds }))
}

/// `FOOTPRINT_EDITOR_CONTROL::RenameFootprint` of a footprint of a library of the tables: the library's footprint, when the design has no copy of it, goes into
/// the design first (`m_frame->LoadFootprint( fpID )`); a new name the library's folder already has is refused unless `overwrite` (`FootprintExists`).
fn rename_footprint(dir: &Path, name: &str, new_name: &str, overwrite: bool, design: &Design) -> Result<Option<Cmd>, Vec<CheckResult>> {
    if new_name != name && !overwrite && lsa::saved_footprint_of(dir, new_name).is_some() {
        let (lib, item) = split(new_name);
        return Err(fail("ops_footprint_exists", new_name, format!("Footprint '{item}' already exists in library '{lib}'.")));
    }
    if design.footprint_library.as_ref().is_some_and(|l| l.by_name(name).is_some()) {
        return Ok(None);
    }
    let fp = lsa::saved_footprint_of(dir, name).ok_or_else(|| {
        let (lib, item) = split(name);
        fail("ops_unknown_footprint", name, format!("Library '{lib}' has no footprint '{item}'."))
    })?;
    Ok(Some(Cmd::Batch { cmds: vec![Cmd::PutLibraryFootprint { footprint: fp, overwrite: true }, Cmd::RenameLibraryFootprint { name: name.to_string(), new_name: new_name.to_string(), overwrite }] }))
}

/// `PCB_BASE_FRAME::CreateNewFootprint( name, aLibName )`: "Try to infer the footprint attributes from an existing footprint in the library" -- the attributes
/// of the library's last footprint by name (`GetFootprintNames( aLibName, true )`, `fpnames.back()`), when there is one that reads.
fn new_footprint_attributes(dir: &Path, name: &str) -> Option<eda_model::ir::FootprintAttributes> {
    let (lib, _) = split(name);
    let entry = library_tables::libs(dir, Kind::Footprint).find(lib).cloned()?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(&entry.path).ok()?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().and_then(|x| x.to_str()) == Some("kicad_mod") && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))).collect();
    files.sort();
    let last = files.last()?;
    let fp = eda_kicad::parse_library_footprint(&std::fs::read_to_string(last).ok()?).ok()?.footprint;
    Some(fp.attributes)
}

// ------------------------------------------------------------------------------------------------ the modified flag and the originals

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Buffers {
    /// The items edited since they were opened, saved or reverted (`IsContentModified()`), by kind: `(symbol?, Lib:Name)`.
    #[serde(default)]
    dirty: BTreeSet<(bool, String)>,
    /// The symbols as they were made, for the ones a library's file does not have.
    #[serde(default)]
    originals: BTreeMap<String, LibrarySymbol>,
}

fn state() -> &'static Mutex<HashMap<PathBuf, Buffers>> {
    static STATE: OnceLock<Mutex<HashMap<PathBuf, Buffers>>> = OnceLock::new();
    STATE.get_or_init(Default::default)
}

/// Where the flags and the made symbols of a design are kept: beside its undo history (`board::history_root`), which they belong with (an Undo sets the flag).
fn buffers_file(dir: &Path) -> PathBuf {
    dir.join(".history").join("library-buffers.json")
}

/// The buffers of `dir`, read from [`buffers_file`] the first time this server asks (UI-LEFT-3 L3-4), changed by `f` and written back when `f` changed them.
fn with_buffers<T>(dir: &Path, f: impl FnOnce(&mut Buffers) -> T) -> T {
    let mut all = state().lock().unwrap_or_else(PoisonError::into_inner);
    let b = all.entry(dir.to_path_buf()).or_insert_with(|| std::fs::read_to_string(buffers_file(dir)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default());
    let before = serde_json::to_string(&*b).unwrap_or_default();
    let out = f(b);
    let after = serde_json::to_string(&*b).unwrap_or_default();
    if after != before {
        let path = buffers_file(dir);
        if b.dirty.is_empty() && b.originals.is_empty() {
            let _ = std::fs::remove_file(&path);
        } else if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, after).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
    }
    out
}

/// The two library sections of the design on disk, read without building the model (the design's own reader: its tagged items need it, `arbitrary_precision`).
#[derive(Default)]
pub(crate) struct LibrarySections {
    pub(crate) symbol_library: Option<SymbolLibrarySection>,
    pub(crate) footprint_library: Option<FootprintLibrarySection>,
}

pub(crate) fn read_sections(dir: &Path) -> Option<LibrarySections> {
    let design: Design = serde_json::from_str(&std::fs::read_to_string(dir.join("design.json")).ok()?).ok()?;
    Some(LibrarySections { symbol_library: design.symbol_library, footprint_library: design.footprint_library })
}

/// Before a step that may change the library editors' content (a command of their domains, an Undo, a Redo, `step_tag` names them): the sections as they were,
/// for [`note_step`]. `None` for a step of the board or the schematic alone.
pub(crate) fn before_step(dir: &Path, tag: Option<&str>) -> Option<LibrarySections> {
    match tag {
        Some("footprint_editor" | "symbol_editor") | None => read_sections(dir),
        _ => None,
    }
}

/// The step tag of an undo scope (`board::domain_tag`'s names).
pub(crate) fn scope_tag(d: eda_ops::Domain) -> &'static str {
    match d {
        eda_ops::Domain::Pcb => "pcb",
        eda_ops::Domain::Schematic => "schematic",
        eda_ops::Domain::FootprintEditor => "footprint_editor",
        eda_ops::Domain::SymbolEditor => "symbol_editor",
    }
}

fn by_id<T: serde::Serialize>(items: &[T], key: impl Fn(&T) -> &str) -> BTreeMap<String, String> {
    items.iter().map(|i| (key(i).to_string(), serde_json::to_string(i).unwrap_or_default())).collect()
}

/// After the step [`before_step`] was taken for: every table-library item whose copy changed is modified (KiCad's `OnModify` of an edit, an Undo, a Redo), and
/// one that appeared is modified unless it is what its library's file holds (opening an item copies the file's in: not a modification). A symbol that appeared
/// is kept as it was made (`m_original`).
pub(crate) fn note_step(dir: &Path, before: Option<LibrarySections>) {
    let Some(before) = before else { return };
    let Some(after) = read_sections(dir) else { return };
    let syms_before = by_id(before.symbol_library.as_ref().map_or(&[][..], |l| &l.symbols), |s| &s.lib_id);
    let fps_before = by_id(before.footprint_library.as_ref().map_or(&[][..], |l| &l.footprints), |f| &f.name);
    let mut touched: Vec<(bool, String)> = Vec::new();
    let mut made: Vec<LibrarySymbol> = Vec::new();
    for s in after.symbol_library.as_ref().map_or(&[][..], |l| &l.symbols).iter().filter(|s| eda_ops::library_editors::is_table_library_item(&s.lib_id)) {
        let now = serde_json::to_string(s).unwrap_or_default();
        match syms_before.get(&s.lib_id) {
            Some(was) if *was == now => {}
            Some(_) => touched.push((true, s.lib_id.clone())),
            None => {
                made.push(s.clone());
                if !lsa::symbol_matches_file(dir, s, after.symbol_library.as_ref()) {
                    touched.push((true, s.lib_id.clone()));
                }
            }
        }
    }
    for f in after.footprint_library.as_ref().map_or(&[][..], |l| &l.footprints).iter().filter(|f| eda_ops::library_editors::is_table_library_item(&f.name)) {
        let now = serde_json::to_string(f).unwrap_or_default();
        match fps_before.get(&f.name) {
            Some(was) if *was == now => {}
            Some(_) => touched.push((false, f.name.clone())),
            None => {
                if !lsa::footprint_matches_file(dir, f) {
                    touched.push((false, f.name.clone()));
                }
            }
        }
    }
    if touched.is_empty() && made.is_empty() {
        return;
    }
    with_buffers(dir, |b| {
        b.dirty.extend(touched);
        for s in made {
            b.originals.insert(s.lib_id.clone(), s);
        }
    });
}

/// `IsSymbolModified` / the Footprint Editor's `IsContentModified()` as the steps left it (see the module doc).
pub(crate) fn is_dirty(dir: &Path, kind: Kind, id: &str) -> bool {
    with_buffers(dir, |b| b.dirty.contains(&(kind == Kind::Symbol, id.to_string())))
}

/// Save, Revert or a reload cleared the flag of `ids` (`ClearModify()`, `SetContentModified( false )`); a saved symbol's file holds it now, so it is its own
/// original (`SYMBOL_BUFFER::SetOriginal` after `SaveBuffer`).
pub(crate) fn clear(dir: &Path, kind: Kind, ids: &[String], saved: bool) {
    with_buffers(dir, |b| {
        for id in ids {
            b.dirty.remove(&(kind == Kind::Symbol, id.clone()));
            if saved && kind == Kind::Symbol {
                b.originals.remove(id);
            }
        }
    });
}

/// The symbol `lib_id` as it was made, for one its library's file does not hold (`SYMBOL_BUFFER::GetOriginal` of a new buffer).
pub(crate) fn original(dir: &Path, lib_id: &str) -> Option<LibrarySymbol> {
    with_buffers(dir, |b| b.originals.get(lib_id).cloned())
}

/// The names the symbol `lib_id` went by, oldest first, then its own (`SymbolLibrarySection::renamed`).
pub(crate) fn names_of(section: Option<&SymbolLibrarySection>, lib_id: &str) -> Vec<String> {
    let mut names: Vec<String> = section.and_then(|l| l.renamed.get(lib_id)).cloned().unwrap_or_default();
    names.push(lib_id.to_string());
    names
}

/// The name of `SYMBOL_BUFFER::GetOriginal()` of the design's symbol `lib_id` (UI-LEFT-3 L3-1): the first name it went by that its library's file holds
/// (`Lib:Name`), or, when the file holds none of them (a symbol never saved), the first name it had (the symbol as it was made, [`original`]). A rename
/// keeps the original; a Save makes the saved name the file's, so the names before it are no longer held.
pub(crate) fn original_name(section: Option<&SymbolLibrarySection>, summaries: &[eda_kicad::SymbolSummary], lib_id: &str) -> String {
    let names = names_of(section, lib_id);
    names.iter().find(|n| summaries.iter().any(|s| s.name == split(n).1)).unwrap_or(&names[0]).clone()
}


#[cfg(test)]
#[path = "library_buffer_tests.rs"]
mod tests;
