//! The legacy rescuer (`LEGACY_RESCUER`, eeschema/project_rescue.cpp at KiCad 8303b2ad): Rescue Symbols on a schematic none of whose symbols names a
//! library (`SCH_SCREENS::HasNoFullyDefinedLibIds`), and the rescue Remap Legacy Library Symbols runs first (`DIALOG_SYMBOL_REMAP::OnRemapSymbols`).
//!
//! * The libraries are the project's legacy ones (`PROJECT_SCH::LegacySchLibs`, `LEGACY_SYMBOL_LIBS::LoadAllLibraries`): each library of the project
//!   file's legacy list, then the cache library last (`crate::rescue_api::legacy_library_list`, `legacy_library_files`, `cache_library_path`), read
//!   through kicad-cli's conversion of the legacy file (`crate::library_convert`).
//! * The candidates (`LEGACY_RESCUER::FindCandidates`): `RESCUE_CASE_CANDIDATE::FindRescues` -- a name no library has, but one has in another case
//!   (`FindLibraryNearEntries`, `CmpNoCase`), "Rename %s to %s" -- then `RESCUE_CACHE_CANDIDATE::FindRescues` -- a name only the cache has, or whose
//!   cached symbol's pins conflict with the first library's (`PinsConflictWith( .., true, true, true, true, false )`) -- under its own name;
//!   `RemoveDuplicates` keeps the first of a name.
//! * The rescue (`OpenRescueLibrary`, `PerformAction`, `WriteRescueLibrary`): `<project>-rescue.lib` beside the board, what it held kept when the list
//!   names it, each chosen candidate's symbol (the cache's, else the library's) flattened and named by its new name, written by KiCad's legacy writer
//!   (`eda_kicad::legacy_lib`); the library put first in the project's legacy list (`SetLibNamesAndPaths`), which KiCad saves in the project file
//!   (`schematic.legacy_lib_list` of `<project>.kicad_pro`). A case candidate's symbols are renamed (`SetLibItemName( m_new_name )`), one command.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eda_kicad::library_table::TableKind;
use eda_model::ir::{Design, LibrarySymbol};
use serde_json::{json, Value};

/// One legacy library of the project, as the KiCad-format copy kicad-cli makes of it.
pub(crate) struct LegacyLib {
    pub name: String,
    pub cache: bool,
    pub text: Arc<String>,
    /// Its symbols by name, flattened.
    pub symbols: BTreeMap<String, LibrarySymbol>,
}

/// `LEGACY_SYMBOL_LIBS::LoadAllLibraries`: the listed libraries that are there, in the list's order, then the cache.
pub(crate) fn legacy_libs(dir: &Path, project: &str) -> Vec<LegacyLib> {
    let (lib_dir, names) = crate::rescue_api::legacy_library_list(dir, project);
    let mut out = Vec::new();
    let mut read = |name: String, path: &Path, cache: bool| {
        let Ok(converted) = crate::library_convert::readable(TableKind::Symbol, path) else { return };
        let Ok(text) = eda_kicad::read_symbol_library_text(&converted) else { return };
        let Ok(parsed) = eda_kicad::parse_library_symbols(&text) else { return };
        let section = eda_model::ir::SymbolLibrarySection { symbols: parsed.symbols, ..Default::default() };
        let symbols = section.symbols.iter().filter_map(|s| section.flatten(&s.lib_id).ok().map(|f| (s.lib_id.clone(), f))).collect();
        out.push(LegacyLib { name, cache, text: Arc::new(text), symbols });
    };
    for (name, file) in crate::rescue_api::legacy_library_files(dir, &lib_dir, &names) {
        if let Ok(path) = file {
            read(name, &path, false);
        }
    }
    if let Some(cache) = crate::rescue_api::cache_library_path(dir, project) {
        let name = cache.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        read(name, &cache, true);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Case,
    Cache,
}

/// `RESCUE_CASE_CANDIDATE` / `RESCUE_CACHE_CANDIDATE`.
#[derive(Debug, Clone)]
pub(crate) struct LegacyCandidate {
    pub kind: Kind,
    pub requested: String,
    pub new_name: String,
    pub cache: Option<LibrarySymbol>,
    pub lib: Option<LibrarySymbol>,
    /// The library (index into the list) and the name of the symbol the rescue copies: the cache's, else the library's.
    pub source: Option<(usize, String)>,
    pub unit: u32,
    pub body_style: u32,
}

impl LegacyCandidate {
    /// `GetActionDescription`.
    pub(crate) fn action(&self) -> String {
        match self.kind {
            Kind::Case => format!("Rename {} to {}", self.requested, self.new_name),
            Kind::Cache => match (&self.cache, &self.lib) {
                (None, None) => format!("Cannot rescue symbol {} which is not available in any library or the cache.", self.requested),
                (Some(_), None) => format!("Rescue symbol {} found only in cache library to {}.", self.requested, self.new_name),
                _ => format!("Rescue modified symbol {} to {}", self.requested, self.new_name),
            },
        }
    }
}

/// `LEGACY_RESCUER::FindCandidates` + `RemoveDuplicates` over the placed symbols (sorted by library identifier, as `getSymbols` sorts them).
pub(crate) fn find_candidates(placed: &[crate::rescue_api::Placed], libs: &[LegacyLib]) -> Vec<LegacyCandidate> {
    let mut placed: Vec<&crate::rescue_api::Placed> = placed.iter().collect();
    placed.sort_by(|a, b| a.lib_id.cmp(&b.lib_id));
    let item = |id: &str| id.split_once(':').map_or(id, |(_, i)| i).to_string();
    let mut out: Vec<LegacyCandidate> = Vec::new();
    // RESCUE_CASE_CANDIDATE::FindRescues
    let mut case: BTreeMap<String, LegacyCandidate> = BTreeMap::new();
    let mut last = None;
    for p in &placed {
        let name = item(&p.lib_id);
        if last.as_deref() == Some(name.as_str()) {
            continue;
        }
        last = Some(name.clone());
        if libs.iter().any(|l| l.symbols.contains_key(&name)) {
            continue;
        }
        let near = libs.iter().enumerate().find_map(|(i, l)| l.symbols.iter().find(|(n, _)| n.eq_ignore_ascii_case(&name)).map(|(n, s)| (i, n.clone(), s.clone())));
        let Some((i, found, sym)) = near else { continue };
        case.insert(name.clone(), LegacyCandidate { kind: Kind::Case, requested: name, new_name: found.clone(), cache: None, lib: Some(sym), source: Some((i, found)), unit: p.unit, body_style: p.body_style });
    }
    out.extend(case.into_values());
    // RESCUE_CACHE_CANDIDATE::FindRescues
    let mut cache: BTreeMap<String, LegacyCandidate> = BTreeMap::new();
    let mut last = None;
    for p in &placed {
        let name = item(&p.lib_id);
        if last.as_deref() == Some(name.as_str()) {
            continue;
        }
        last = Some(name.clone());
        let cache_match = libs.iter().enumerate().filter(|(_, l)| l.cache).find_map(|(i, l)| l.symbols.get(&name).map(|s| (i, s.clone())));
        let lib_match = libs.iter().enumerate().filter(|(_, l)| !l.cache).find_map(|(i, l)| l.symbols.get(&name).map(|s| (i, s.clone())));
        match (&cache_match, &lib_match) {
            (Some((_, c)), Some((_, l))) if !crate::rescue_api::pins_conflict_with(c, l, true, true, true, true, false) => continue,
            (None, Some(_)) => continue,
            _ => {}
        }
        let source = cache_match.as_ref().or(lib_match.as_ref()).map(|(i, _)| (*i, name.clone()));
        cache.insert(
            name.clone(),
            LegacyCandidate { kind: Kind::Cache, requested: name.clone(), new_name: name, cache: cache_match.map(|(_, s)| s), lib: lib_match.map(|(_, s)| s), source, unit: p.unit, body_style: p.body_style },
        );
    }
    out.extend(cache.into_values());
    let mut seen = BTreeSet::new();
    out.retain(|c| seen.insert(c.requested.clone()));
    out
}

/// `GetRescueLibraryFileName`: `<project>-rescue.lib` beside the board.
pub(crate) fn rescue_library_path(dir: &Path, project: &str) -> PathBuf {
    dir.join(format!("{project}-rescue.lib"))
}

/// The rescue of `chosen`: the rescue library written (`OpenRescueLibrary`, each `PerformAction`, `WriteRescueLibrary`) and put first in the legacy list.
/// Returns the library's path and the renames (`requested -> new name`) of the case candidates, with the symbols they are now drawn from.
pub(crate) fn rescue(dir: &Path, project: &str, libs: &[LegacyLib], chosen: &[&LegacyCandidate]) -> Result<(PathBuf, Vec<(String, String)>, Vec<LibrarySymbol>), String> {
    let path = rescue_library_path(dir, project);
    let rescue_name = format!("{project}-rescue");
    // "If a rescue library already exists copy the contents of that library so we do not lose any previous rescues."
    let mut symbols = BTreeMap::new();
    if let Some(old) = libs.iter().find(|l| !l.cache && l.name == rescue_name) {
        for name in old.symbols.keys() {
            if let Some(s) = eda_kicad::legacy_sch::flattened_symbol(&old.text, name, name) {
                symbols.insert(name.clone(), s);
            }
        }
    }
    let mut renames = Vec::new();
    let mut rescued = Vec::new();
    for c in chosen {
        // "A symbol that cannot be rescued is a valid condition so just bail out here."
        let Some((i, source)) = &c.source else { continue };
        let Some(lib) = libs.get(*i) else { continue };
        let Some(s) = eda_kicad::legacy_sch::flattened_symbol(&lib.text, source, &c.new_name) else { continue };
        symbols.insert(c.new_name.clone(), s);
        if c.requested != c.new_name {
            renames.push((c.requested.clone(), c.new_name.clone()));
            if let Some(mut m) = c.cache.clone().or_else(|| c.lib.clone()) {
                m.lib_id = c.new_name.clone();
                m.extends = None;
                rescued.push(m);
            }
        }
    }
    let text = eda_kicad::legacy_lib::write_legacy_library(&symbols.into_values().collect::<Vec<_>>());
    std::fs::write(&path, text).map_err(|_| format!("Failed to create symbol library file '{}'.", path.display()))?;
    // `SetLibNamesAndPaths`: the rescue library first in the list ("Make sure the library is not already in the list"), saved with the project
    let (lib_dir, mut names) = crate::rescue_api::legacy_library_list(dir, project);
    names.retain(|n| *n != rescue_name);
    names.insert(0, rescue_name);
    let pro_path = dir.join(format!("{project}.kicad_pro"));
    let mut pro: Value = std::fs::read_to_string(&pro_path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({}));
    if !pro.is_object() {
        pro = json!({});
    }
    if !pro["schematic"].is_object() {
        pro["schematic"] = json!({});
    }
    pro["schematic"]["legacy_lib_dir"] = json!(lib_dir);
    pro["schematic"]["legacy_lib_list"] = json!(names);
    std::fs::write(&pro_path, serde_json::to_string_pretty(&pro).unwrap_or_default()).map_err(|e| format!("{}: {e}", pro_path.display()))?;
    Ok((path, renames, rescued))
}

/// The placed symbols of a design alone (the remap has no model): `getSymbols` with each symbol's own value.
pub(crate) fn placed_of(design: &Design) -> Vec<crate::rescue_api::Placed> {
    crate::rescue_api::placed_symbols(design, &eda_model::ConstraintModel::default())
}

/// What the Project Rescue Helper is shown of a candidate (`candidate_json`'s shape).
pub(crate) fn candidate_json(c: &LegacyCandidate, placed: &[crate::rescue_api::Placed]) -> Value {
    let instances: Vec<Value> = placed.iter().filter(|p| p.lib_id == c.requested).map(|p| json!({ "reference": p.reference, "value": p.value })).collect();
    json!({
        "requested": c.requested,
        "new_id": c.new_name,
        "action": c.action(),
        "cache": c.cache,
        "lib": c.lib,
        "unit": c.unit,
        "body_style": c.body_style,
        "instances": instances,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rescue_api::Placed;

    const LIB: &str = "(kicad_symbol_lib (version 20261015) (generator \"t\")\n  (symbol \"R\" (property \"Reference\" \"R\" (at 0 0 0)) (property \"Value\" \"R\" (at 0 0 0))\n    (symbol \"R_1_1\" (pin passive line (at 0 3.81 270) (length 1.27) (name \"~\") (number \"1\")) (pin passive line (at 0 -3.81 90) (length 1.27) (name \"~\") (number \"2\")))))\n";

    fn lib(name: &str, cache: bool, text: &str) -> LegacyLib {
        let parsed = eda_kicad::parse_library_symbols(text).unwrap();
        let symbols = parsed.symbols.into_iter().map(|s| (s.lib_id.clone(), s)).collect();
        LegacyLib { name: name.into(), cache, text: Arc::new(text.into()), symbols }
    }

    fn placed(id: &str) -> Placed {
        Placed { lib_id: id.into(), unit: 1, body_style: 1, reference: "R1".into(), value: "10k".into() }
    }

    #[test]
    fn the_legacy_rescuer_finds_a_name_in_another_case_and_a_symbol_only_the_cache_has() {
        let moved = LIB.replace("(at 0 -3.81 90)", "(at 0 -5.08 90)");
        // `r` is `R` of the library in another case; `R` is in the cache only when the list's library lacks it
        let libs = vec![lib("mylib", false, &LIB.replace("\"R\"", "\"Q\"").replace("R_1_1", "Q_1_1")), lib("p-cache", true, LIB)];
        let found = find_candidates(&[placed("r"), placed("R")], &libs);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].action(), "Rename r to R");
        assert_eq!(found[1].action(), "Rescue symbol R found only in cache library to R.");
        // a cached symbol whose pins differ from the library's is a modified one; one that agrees is no candidate
        let libs = vec![lib("mylib", false, &moved), lib("p-cache", true, LIB)];
        assert_eq!(find_candidates(&[placed("R")], &libs)[0].action(), "Rescue modified symbol R to R");
        let libs = vec![lib("mylib", false, LIB), lib("p-cache", true, LIB)];
        assert!(find_candidates(&[placed("R")], &libs).is_empty());
        // neither: "Cannot rescue"
        assert_eq!(find_candidates(&[placed("X")], &libs)[0].action(), "Cannot rescue symbol X which is not available in any library or the cache.");
    }

    #[test]
    fn the_rescue_library_is_written_in_the_legacy_format_and_put_first_in_the_list() {
        let dir = std::env::temp_dir().join(format!("eda-legacy-rescue-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p.pro"), "[eeschema]\nLibDir=\n[eeschema/libraries]\nLibName1=mylib\n").unwrap();
        let libs = vec![lib("p-cache", true, LIB)];
        let found = find_candidates(&[placed("R")], &libs);
        let (path, renames, _) = rescue(&dir, "p", &libs, &found.iter().collect::<Vec<_>>()).unwrap();
        assert!(renames.is_empty(), "a cached symbol keeps its name");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("EESchema-LIBRARY Version 2.4\n") && text.contains("DEF R R 0 ") && text.contains("X ~ 1 0 150 50 D"), "{text}");
        let (_, names) = crate::rescue_api::legacy_library_list(&dir, "p");
        assert_eq!(names, vec!["p-rescue".to_string(), "mylib".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
