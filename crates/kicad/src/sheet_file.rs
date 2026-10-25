//! Reading a `.kicad_sch` to put it in a design as a sheet: `SCH_EDIT_FRAME::LoadSheetFromFile`'s reading half.
//!
//! The file and the sheets it places are read by the tree importer ([`crate::import_kicad_sch_tree_with`]) -- the symbols that cannot be told
//! apart by their reference get placeholders of their own (`R?1`) -- and the library symbols each file embeds come through the Symbol Editor's
//! reader, which keeps hidden pins and fills. What the file has that the design could not hold is said in `notes`.

use crate::sexpr;
use eda_model::ir::LibrarySymbol;
use eda_model::sheet_file::SheetFile;
use eda_model::CheckResult;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The largest file read as a sheet: a schematic is text, and a megabyte is a big one.
const MAX_BYTES: u64 = 32 * 1024 * 1024;

/// Read `path` and the sheets it places.
pub fn load_sheet_file(path: &Path) -> Result<SheetFile, Vec<CheckResult>> {
    let fail = |check: &str, msg: String| vec![CheckResult::fail(check, path.display().to_string(), msg)];
    if path.extension().and_then(|e| e.to_str()) != Some("kicad_sch") {
        return Err(fail("sheet_file_kind", "a sheet file is a .kicad_sch".into()));
    }
    let size = std::fs::metadata(path).map_err(|e| fail("sheet_file_read", format!("{e}")))?.len();
    if size > MAX_BYTES {
        return Err(fail("sheet_file_size", format!("the file is {size} bytes; the largest schematic read as a sheet is {MAX_BYTES}")));
    }
    let opts = crate::ImportOptions { unique_references: true };
    let (design, _model, imported) = crate::import_kicad_sch_tree_with(path, &opts)?;
    let root = design.schematic.ok_or_else(|| fail("sheet_file_empty", "the file holds no schematic".into()))?;
    let screens = design.sheet_contents.unwrap_or_default();

    // the library symbols of every file in the tree, and the sheet files that are not there
    let mut lib_symbols: Vec<LibrarySymbol> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    collect_lib_symbols(path, None, &mut seen, &mut lib_symbols, &mut notes, &mut missing);
    // a name two files give to different symbols: the first wins, as the design's own would
    let mut names = BTreeSet::new();
    lib_symbols.retain(|s| names.insert(s.lib_id.clone()));

    if imported.unresolved_symbols > 0 {
        notes.push(format!("{} symbol(s) had no library symbol in the file and were left out", imported.unresolved_symbols));
    }
    if imported.sheets_not_descended > 0 {
        notes.push(format!("{} sheet(s) name a file that could not be read; they come in empty", imported.sheets_not_descended));
    }
    // which: the files the sheets place that are not beside the one read (a file taken from a project alone, or a drop that left some out)
    for m in missing {
        notes.push(m);
    }
    Ok(SheetFile { root, screens, lib_symbols, bus_aliases: design.bus_aliases, notes })
}

/// The files the sheets of a schematic text place, as the text names them (the `Sheetfile` of each `(sheet ..)`: a path relative to the folder of the file that places them, in the
/// file's own spelling), in the order they are placed, repeats included. Nothing is read from disk: it is how a set of files that arrived as text -- a drop in a browser -- is put
/// together before [`load_sheet_file`] reads them.
pub fn referenced_sheet_files(text: &str) -> Vec<String> {
    let Ok(tree) = sexpr::parse(text) else { return Vec::new() };
    let Some(root) = tree.as_list() else { return Vec::new() };
    sexpr::find_all(root, "sheet")
        .filter_map(|sheet| sexpr::find_all(sheet, "property").find(|p| sexpr::txt(p, 1).is_some_and(|n| crate::sch_import::is_sheet_field(n, "Sheetfile"))).and_then(|p| sexpr::txt(p, 2)))
        .filter(|f| !f.is_empty())
        .map(String::from)
        .collect()
}

/// `placed_by` is the sheet file that names `path` and how it names it (`None` for the file that was asked for): a file that is not there is said by name, with the file that places it.
fn collect_lib_symbols(path: &Path, placed_by: Option<(&str, &str)>, seen: &mut BTreeSet<PathBuf>, out: &mut Vec<LibrarySymbol>, notes: &mut Vec<String>, missing: &mut Vec<String>) {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !seen.insert(canonical) {
        return;
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        if let Some((parent, named)) = placed_by {
            missing.push(format!("The sheet file '{named}' that '{parent}' places was not found: its sheet comes in empty."));
        }
        return;
    };
    let Ok(tree) = sexpr::parse(&text) else { return };
    let Some(root) = tree.as_list() else { return };
    for cache in sexpr::find_all(root, "lib_symbols") {
        let parsed = crate::symbol_import::symbols_of(cache);
        notes.extend(parsed.warnings);
        out.extend(parsed.symbols);
    }
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let own = path.file_name().and_then(|n| n.to_str()).unwrap_or("the sheet");
    for sheet in sexpr::find_all(root, "sheet") {
        let file = sexpr::find_all(sheet, "property").find(|p| sexpr::txt(p, 1).is_some_and(|n| crate::sch_import::is_sheet_field(n, "Sheetfile"))).and_then(|p| sexpr::txt(p, 2));
        if let Some(file) = file {
            collect_lib_symbols(&dir.join(file), Some((own, file)), seen, out, notes, missing);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// mcu30 as module sheets, written as a KiCad project tree in a scratch folder: the root `whole.kicad_sch` and a file per sheet.
    fn written(name: &str) -> (PathBuf, Vec<String>) {
        let dir = std::env::temp_dir().join(format!("eda_kicad_sheet_file_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/mcu_board_30plus.yaml");
        let model: eda_model::ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let design = eda_engine::derive_schematic_modules(&model, &eda_engine::EngineOptions::new(1, "t")).unwrap();
        let tree = crate::export_kicad_sch_tree(&design, &model, &crate::ExportMeta { date: "2026-10-10", title: "t" }, "whole.kicad_sch").unwrap();
        let mut names = Vec::new();
        for (file, text) in &tree {
            std::fs::write(dir.join(file), text).unwrap();
            names.push(file.clone());
        }
        (dir, names)
    }

    #[test]
    fn a_file_is_read_with_the_sheets_it_places_and_the_library_symbols_they_use() {
        let (dir, names) = written("tree");
        let file = load_sheet_file(&dir.join("whole.kicad_sch")).unwrap();
        // the root places the three module sheets and holds no symbols of its own; each sheet's screen is read under the file the placement names
        assert_eq!(file.root.sheets.len(), 3);
        assert!(file.root.symbols.is_empty());
        let mut screens: Vec<&String> = file.screens.keys().collect();
        screens.sort();
        let mut want: Vec<&String> = names.iter().filter(|n| *n != "whole.kicad_sch").collect();
        want.sort();
        assert_eq!(screens, want);
        // every symbol is there once, with the definitions of the libraries it names
        let refs = file.references();
        assert!(refs.contains(&"U1".to_string()) && refs.contains(&"R8".to_string()) && refs.contains(&"J1".to_string()), "{refs:?}");
        assert!(file.lib_symbols.iter().any(|s| s.lib_id == "Device:R"), "{:?}", file.lib_symbols.iter().map(|s| &s.lib_id).collect::<Vec<_>>());
        assert!(file.library_nicknames().contains(&"Device".to_string()));
        assert!(file.root.imported_from_kicad && file.screens.values().all(|s| s.imported_from_kicad), "the screens are in KiCad's frame until the design takes them in");
        assert!(file.notes.is_empty(), "{:?}", file.notes);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// p12 S4 (a): the bus aliases come with the file (`eda import-kicad`, Import Sheet and File > Import > Non-KiCad Schematic read through it): KiCad's
    /// QA `bus_connection` declares three in its root, and they are moved to its `.kicad_pro` as KiCad 10 keeps them, they still come.
    #[test]
    fn a_sheet_files_bus_aliases_come_with_it() {
        let data = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data"));
        let src = data.join("eeschema/netlists/bus_connection");
        if !src.exists() {
            eprintln!("QA corpus not found at {}; skipping", src.display());
            return;
        }
        let names = |f: &SheetFile| -> Vec<String> {
            let mut n: Vec<String> = f.bus_aliases.iter().map(|a| format!("{}={}", a.name, a.members.join(" "))).collect();
            n.sort();
            n
        };
        let want = vec!["a_xyz=x y z".to_string(), "b_x=x".into(), "b_yz=y z".into()];
        assert_eq!(names(&load_sheet_file(&src.join("bus_connection.kicad_sch")).unwrap()), want);
        // the same aliases in the project instead of the file
        let dir = std::env::temp_dir().join(format!("eda_kicad_sheet_file_{}_aliases", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for e in std::fs::read_dir(&src).unwrap().flatten() {
            let text = std::fs::read_to_string(e.path()).unwrap_or_default();
            let kept: Vec<&str> = text.lines().filter(|l| !l.trim_start().starts_with("(bus_alias")).collect();
            std::fs::write(dir.join(e.file_name()), kept.join("\n")).unwrap();
        }
        std::fs::write(dir.join("bus_connection.kicad_pro"), r#"{ "schematic": { "bus_aliases": { "a_xyz": ["x", "y", "z"], "b_x": ["x"], "b_yz": ["y", "z"] } } }"#).unwrap();
        assert_eq!(names(&load_sheet_file(&dir.join("bus_connection.kicad_sch")).unwrap()), want);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sheet_whose_file_is_missing_comes_in_empty_and_is_said() {
        let (dir, names) = written("missing_child");
        let child = names.iter().find(|n| *n != "whole.kicad_sch").unwrap();
        std::fs::remove_file(dir.join(child)).unwrap();
        let file = load_sheet_file(&dir.join("whole.kicad_sch")).unwrap();
        assert!(file.notes.iter().any(|n| n.contains("could not be read")), "{:?}", file.notes);
        // and the file that is missing is named, with the one that places it
        assert!(file.notes.iter().any(|n| n.contains(&format!("The sheet file '{child}' that 'whole.kicad_sch' places was not found"))), "{:?}", file.notes);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_sheet_files_a_text_places_are_listed_as_it_names_them() {
        let (dir, names) = written("references");
        let text = std::fs::read_to_string(dir.join("whole.kicad_sch")).unwrap();
        let mut listed = referenced_sheet_files(&text);
        listed.sort();
        let mut want: Vec<String> = names.iter().filter(|n| *n != "whole.kicad_sch").cloned().collect();
        want.sort();
        assert_eq!(listed, want);
        assert!(referenced_sheet_files("(kicad_sch (version 20250114))").is_empty());
        assert!(referenced_sheet_files("not a schematic (").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_is_not_a_schematic_is_refused_with_the_reason() {
        let dir = std::env::temp_dir().join(format!("eda_kicad_sheet_file_{}_refused", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let wrong = dir.join("notes.txt");
        std::fs::write(&wrong, "hello").unwrap();
        assert_eq!(load_sheet_file(&wrong).unwrap_err()[0].check, "sheet_file_kind");
        assert_eq!(load_sheet_file(&dir.join("absent.kicad_sch")).unwrap_err()[0].check, "sheet_file_read");
        let empty = dir.join("empty.kicad_sch");
        std::fs::write(&empty, "").unwrap();
        assert!(load_sheet_file(&empty).is_err(), "an empty file is not a schematic");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
