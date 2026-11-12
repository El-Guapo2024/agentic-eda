//! A KiCad 5 legacy schematic (`.sch`), opened as KiCad 8 / 10 opens it (UI-LEFT-2 II1): `SCH_EDIT_FRAME::OpenProjectFiles` with the legacy plugin.
//!
//! kicad-cli cannot do it: `sch upgrade` reloads the file with the s-expression reader (`EESCHEMA_JOBS_HANDLER::JobUpgrade`:
//! `SCH_IO_MGR::FindPlugin( SCH_KICAD )`, "Expecting '(' .. line 1"), `sch downgrade` the same, and `sch import` takes non-KiCad formats only ("No schematic
//! importer recognizes the file format"). Its netlist, ERC and plots do read a `.sch` (`EESCHEMA_HELPERS::LoadSchematic` picks the legacy plugin by the
//! extension) when a `.kicad_pro` is beside it, and they are what the slow tests prove the port against; nothing kicad-cli runs writes a `.kicad_sch` of it.
//!
//! So the schematic is read by the Rust port of KiCad's legacy reader (`eda_kicad::legacy_sch`), and the libraries by KiCad's own legacy library reader in
//! kicad-cli: `kicad-cli sym upgrade` of the project's cache library and of each legacy row of the tables (`crate::library_convert`, the copy every
//! library of another format is read as). Each symbol is linked as `SCH_SCREEN::UpdateSymbolLinks` links it -- the library of the symbol library tables
//! (the project's `sym-lib-table` beside the file, then KiCad's global one) that has it, else the cache library under `Lib_Name` -- and the screens are
//! written as the `.kicad_sch` files KiCad would save, into a scratch folder, which the s-expression reader then reads ([`eda_kicad::load_sheet_file`]).
//!
//! What the project keeps for the legacy rescue comes along to the board's folder ([`carry_project_files`]): the cache library (named after the board's
//! project, `LEGACY_SYMBOL_LIBS::CacheName`), the KiCad 5 project file with its legacy library list, and the legacy libraries it names, so Rescue Symbols
//! and Remap Legacy Library Symbols find what KiCad finds; and, for a project whose symbols name their library and that has a cache, the project table's
//! `<project>-cache` row of type Legacy that `OpenProjectFiles` adds.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use eda_kicad::legacy_sch::{self, LookedUp};
use eda_kicad::library_table::TableKind;
use eda_model::sheet_file::SheetFile;
use eda_model::CheckResult;

use crate::library_index::{Kind, LibSource};

/// A legacy schematic read.
#[derive(Debug)]
pub(crate) struct LegacySchematic {
    pub content: SheetFile,
    /// The project's folder and name (the root file's).
    pub project_dir: PathBuf,
    pub project: String,
    /// `SCH_SCREENS::HasNoFullyDefinedLibIds`: no symbol names a library -- KiCad opens Remap Symbols (`DIALOG_SYMBOL_REMAP`) as the project loads.
    pub remap: bool,
}

/// Whether the file at `path` is a KiCad legacy schematic (`SCH_IO_KICAD_LEGACY::CanReadSchematicFile`): its first line.
pub(crate) fn is_legacy_schematic_file(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 64];
    let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    legacy_sch::is_legacy_schematic(&String::from_utf8_lossy(&head[..n]))
}

/// `LEGACY_SYMBOL_LIBS::CacheName`: `<project>-cache.lib` in the project's folder, else the 2007 `<project>.cache.lib`.
pub(crate) fn cache_library(dir: &Path, project: &str) -> Option<PathBuf> {
    [format!("{project}-cache.lib"), format!("{project}.cache.lib")].into_iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

fn fail(what: &Path, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail("legacy_schematic", what.display().to_string(), msg)]
}

/// The legacy schematic `root` and its sheets read as KiCad opens them, its `.kicad_sch` files written into `work` (a scratch folder), and read back.
pub(crate) fn load(root: &Path, work: &Path) -> Result<LegacySchematic, Vec<CheckResult>> {
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let dir = root.parent().map(Path::to_path_buf).unwrap_or_default();
    let project = root.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "schematic".into());
    let tree = legacy_sch::load_tree(&root, &|p| std::fs::read(p).map(|b| String::from_utf8_lossy(&b).into_owned()).map_err(|e| e.to_string())).map_err(|e| fail(&root, e))?;

    // `PROJECT_SCH::LegacySchLibs`: the cache library, read by KiCad's legacy reader (kicad-cli)
    let mut messages: Vec<String> = Vec::new();
    let cache: Option<Arc<String>> = match cache_library(&dir, &project) {
        Some(path) => match crate::library_convert::readable(TableKind::Symbol, &path).and_then(|p| eda_kicad::read_symbol_library_text(&p)) {
            Ok(text) => Some(Arc::new(text)),
            Err(e) => {
                messages.push(format!("Error loading symbol library '{}'.\n{e}", path.display()));
                None
            }
        },
        None => {
            // "The project symbol library cache file '%s' was not found." (KiCad asks whether to load without it; the import goes on)
            messages.push(format!("The project symbol library cache file '{project}-cache.lib' was not found."));
            None
        }
    };
    let cache_names: std::collections::BTreeSet<String> = cache.as_ref().map(|t| eda_kicad::scan_symbols(t).into_iter().map(|s| s.name).collect()).unwrap_or_default();
    // `PROJECT_SCH::SymbolLibAdapter`: the project's table (the folder's `sym-lib-table`) over KiCad's global one; a legacy row read through kicad-cli
    let libs = crate::library_tables::libs(&dir, Kind::Symbol);
    let loaded: Mutex<BTreeMap<String, Option<(Arc<String>, std::collections::BTreeSet<String>)>>> = Mutex::new(BTreeMap::new());
    let lookup = |id: &str| -> Option<LookedUp> {
        let (nick, item) = id.split_once(':').unwrap_or(("", id));
        if !nick.is_empty() {
            let mut cache_of = loaded.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let lib = cache_of
                .entry(nick.to_string())
                .or_insert_with(|| {
                    let path = libs.lib_path(Kind::Symbol, nick).ok()?;
                    let text = eda_kicad::read_symbol_library_text(&path).ok()?;
                    let names = eda_kicad::scan_symbols(&text).into_iter().map(|s| s.name).collect();
                    Some((Arc::new(text), names))
                })
                .clone();
            if let Some((text, names)) = lib {
                if names.contains(item) {
                    return Some(LookedUp { library_text: text, name: item.to_string(), from_cache: false });
                }
            }
        }
        // "Falling back to cache": the cache library holds the symbol as `Lib_Name`
        let cached = id.replace(':', "_");
        let text = cache.as_ref()?;
        cache_names.contains(&cached).then(|| LookedUp { library_text: text.clone(), name: cached, from_cache: true })
    };
    let out = work.join("legacy");
    let written = legacy_sch::write_tree(&tree, &out, &project, &lookup).map_err(|e| fail(&root, e))?;
    // the project file beside the root, which the s-expression reader reads its bus aliases and unit notation from
    if let Ok(pro) = std::fs::read_to_string(root.with_extension("kicad_pro")) {
        let _ = std::fs::write(written.root.with_extension("kicad_pro"), pro);
    }
    let mut content = eda_kicad::load_sheet_file(&written.root)?;
    messages.extend(written.messages.iter().filter(|m| !m.starts_with("Falling back to cache")).cloned());
    let fallbacks = written.messages.iter().filter(|m| m.starts_with("Falling back to cache")).count();
    if fallbacks > 0 {
        messages.push(format!("{fallbacks} library symbol(s) taken from the project's cache library (no library of the symbol library tables has them)."));
    }
    // "This file was created by an older version of KiCad. It will be converted to the new format when saved."
    let mut notes = vec!["This file was created by an older version of KiCad. It will be converted to the new format when saved.".to_string()];
    notes.extend(messages);
    notes.append(&mut content.notes);
    content.notes = notes;
    Ok(LegacySchematic { content, project_dir: dir, project, remap: written.no_fully_defined_lib_ids })
}

/// `SCH_EDIT_FRAME::LoadSheetFromFile`'s reading of a file to put in as a sheet: a schematic of whichever of KiCad's own formats the file is
/// (`SCH_IO_MGR::GuessPluginTypeFromSchPath`): a legacy `.sch` read here (its `.kicad_sch` files written into a scratch folder that is removed after), any
/// other through [`eda_kicad::load_sheet_file`].
pub(crate) fn load_sheet_file(path: &Path) -> Result<SheetFile, Vec<CheckResult>> {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("sch")) && is_legacy_schematic_file(path) {
        return eda_kicad_engine::with_scratch(|work| load(path, work).map(|l| l.content));
    }
    eda_kicad::load_sheet_file(path)
}

/// What the legacy rescue reads, put in the board's folder `board_dir` whose project is `board_project` (the studio's project is the board: the intent's
/// name): the cache library as `<board_project>-cache.lib` (and its `.dcm`), the KiCad 5 project file as `<board_project>.pro` (its legacy library list,
/// `[eeschema] LibDir` / `[eeschema/libraries] LibName<n>`), and each legacy library that list names, at the same place relative to the folder. The cache
/// library and the project file are the schematic's and replace the ones a schematic imported before left; a library the board's folder has already is kept. For a schematic whose symbols name their library, `OpenProjectFiles`' cache row: the project table's
/// `<board_project>-cache` of type Legacy, when the table has none of that name. Returns what was done, to be said.
pub(crate) fn carry_project_files(legacy: &LegacySchematic, board_dir: &Path, board_project: &str) -> Vec<String> {
    let mut said = Vec::new();
    let (dir, project) = (&legacy.project_dir, legacy.project.as_str());
    let same = std::fs::canonicalize(dir).ok() == std::fs::canonicalize(board_dir).ok() && project == board_project;
    if same {
        return said;
    }
    let copy = |from: &Path, to: &Path, said: &mut Vec<String>, replace: bool| {
        if (to.exists() && !replace) || !from.is_file() {
            return false;
        }
        if let Some(p) = to.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        match std::fs::copy(from, to) {
            Ok(_) => {
                said.push(format!("{} copied to the board's folder as {}", from.file_name().unwrap_or_default().to_string_lossy(), to.strip_prefix(board_dir).unwrap_or(to).display()));
                true
            }
            Err(e) => {
                said.push(format!("{}: {e}", to.display()));
                false
            }
        }
    };
    let mut cache_copied = None;
    if let Some(cache) = cache_library(dir, project) {
        // always as `<project>-cache.lib`, the name `CacheName` looks for first: a 2007 `.cache.lib` would lose to a cache an earlier import left
        let to = board_dir.join(format!("{board_project}-cache.lib"));
        copy(&cache, &to, &mut said, true);
        copy(&cache.with_extension("dcm"), &to.with_extension("dcm"), &mut said, true);
        cache_copied = Some(to);
    }
    let pro = dir.join(format!("{project}.pro"));
    copy(&pro, &board_dir.join(format!("{board_project}.pro")), &mut said, true);
    // the legacy libraries the list names (`legacy_library_files`: beside the project, else in `LibDir`)
    let (lib_dir, names) = crate::rescue_api::legacy_library_list(dir, project);
    for (name, found) in crate::rescue_api::legacy_library_files(dir, &lib_dir, &names) {
        let Ok(path) = found else { continue };
        let rel = path.strip_prefix(dir).map(Path::to_path_buf).unwrap_or_else(|_| PathBuf::from(format!("{name}.lib")));
        if rel.is_absolute() {
            continue; // found by an absolute path, found the same from the board
        }
        copy(&path, &board_dir.join(&rel), &mut said, false);
        copy(&path.with_extension("dcm"), &board_dir.join(&rel).with_extension("dcm"), &mut said, false);
    }
    // `OpenProjectFiles`: "if( cacheExists )": the cache row, "Legacy project cache library"
    if let (false, Some(cache)) = (legacy.remap, cache_copied) {
        if cache.is_file() {
            let nickname = format!("{board_project}-cache");
            let table = crate::library_tables::load(board_dir, TableKind::Symbol);
            if !table.libs().iter().any(|l| l.nickname == nickname) {
                let uri = crate::library_tables::normalized_uri(board_dir, &cache);
                match crate::library_tables::set_project_row(board_dir, TableKind::Symbol, &nickname, &uri, "Legacy") {
                    Ok(()) => said.push(format!("the project symbol library table has the cache library as '{nickname}'")),
                    Err(e) => said.push(e),
                }
            }
        }
    }
    said
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eda-legacy-import-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const LIB: &str = "(kicad_symbol_lib (version 20241209) (generator \"t\")\n  (symbol \"R\" (property \"Reference\" \"R\" (at 0 0 0)) (property \"Value\" \"R\" (at 0 0 0))\n    (symbol \"R_1_1\" (pin passive line (at 0 3.81 270) (length 1.27) (name \"~\") (number \"1\")) (pin passive line (at 0 -3.81 90) (length 1.27) (name \"~\") (number \"2\")))))\n";

    fn sch(version: u32, lib_id: &str) -> String {
        format!(
            "EESchema Schematic File Version {version}\nEELAYER 30 0\nEELAYER END\n$Descr A4 11693 8268\n$EndDescr\n$Comp\nL {lib_id} R1\nU 1 1 5A000001\nP 2000 1500\nF 0 \"R1\" V 2080 1500 50  0000 C CNN\nF 1 \"10k\" V 2000 1500 50  0000 C CNN\n\t1    2000 1500\n\t1    0    0    -1  \n$EndComp\nWire Wire Line\n\t2000 1350 2000 1200\nText Label 2000 1200 0    50   ~ 0\nIN\n$EndSCHEMATC\n"
        )
    }

    #[test]
    fn a_legacy_schematic_is_linked_through_the_projects_table_and_its_project_file_comes_along() {
        let dir = scratch("table");
        std::fs::write(dir.join("demo.sch"), sch(4, "mylib:R")).unwrap();
        std::fs::write(dir.join("mylib.kicad_sym"), LIB).unwrap();
        std::fs::write(dir.join("sym-lib-table"), "(sym_lib_table\n  (lib (name \"mylib\")(type \"KiCad\")(uri \"${KIPRJMOD}/mylib.kicad_sym\")(options \"\")(descr \"\"))\n)\n").unwrap();
        std::fs::write(dir.join("demo.pro"), "[eeschema]\nversion=1\nLibDir=\n[eeschema/libraries]\n").unwrap();
        assert!(is_legacy_schematic_file(&dir.join("demo.sch")));
        let work = dir.join("work");
        let l = load(&dir.join("demo.sch"), &work).unwrap();
        assert!(!l.remap, "the symbol names its library");
        assert_eq!(l.content.root.symbols.len(), 1);
        assert_eq!(l.content.root.symbols[0].lib_id, "mylib:R");
        assert!(l.content.notes.iter().any(|n| n.contains("'demo-cache.lib' was not found")), "{:?}", l.content.notes);
        assert!(l.content.notes.iter().any(|n| n.starts_with("This file was created by an older version of KiCad.")));
        let board = dir.join("board");
        std::fs::create_dir_all(&board).unwrap();
        let said = carry_project_files(&l, &board, "intent");
        assert!(board.join("intent.pro").is_file(), "{said:?}");
        assert!(!board.join("sym-lib-table").exists(), "no cache library, no cache row");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_schematic_whose_symbols_name_no_library_wants_remapping_and_its_cache_is_carried_with_a_row() {
        let dir = scratch("remap");
        std::fs::write(dir.join("old.sch"), sch(2, "R")).unwrap();
        let l = load(&dir.join("old.sch"), &dir.join("work")).unwrap();
        assert!(l.remap, "a version 2 schematic names bare symbols: Remap Symbols");
        assert!(l.content.notes.iter().any(|n| n.contains("No library symbol found for schematic symbol 'R1 10k'")), "{:?}", l.content.notes);
        // a project of version 4 with its cache: the cache comes along as the board's, with `OpenProjectFiles`' row
        std::fs::write(dir.join("new.sch"), sch(4, "mylib:R")).unwrap();
        std::fs::write(dir.join("new-cache.lib"), "EESchema-LIBRARY Version 2.4\n#End Library\n").unwrap();
        let l = LegacySchematic { content: l.content, project_dir: dir.clone(), project: "new".into(), remap: false };
        let board = dir.join("board");
        std::fs::create_dir_all(&board).unwrap();
        carry_project_files(&l, &board, "intent");
        assert!(board.join("intent-cache.lib").is_file());
        let table = std::fs::read_to_string(board.join("sym-lib-table")).unwrap();
        assert!(table.contains("intent-cache") && table.contains("Legacy") && table.contains("${KIPRJMOD}/intent-cache.lib"), "{table}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
