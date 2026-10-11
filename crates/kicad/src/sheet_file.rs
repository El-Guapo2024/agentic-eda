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

    // the library symbols of every file in the tree
    let mut lib_symbols: Vec<LibrarySymbol> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    collect_lib_symbols(path, &mut seen, &mut lib_symbols, &mut notes);
    // a name two files give to different symbols: the first wins, as the design's own would
    let mut names = BTreeSet::new();
    lib_symbols.retain(|s| names.insert(s.lib_id.clone()));

    if imported.unresolved_symbols > 0 {
        notes.push(format!("{} symbol(s) had no library symbol in the file and were left out", imported.unresolved_symbols));
    }
    if imported.sheets_not_descended > 0 {
        notes.push(format!("{} sheet(s) name a file that could not be read; they come in empty", imported.sheets_not_descended));
    }
    Ok(SheetFile { root, screens, lib_symbols, notes })
}

fn collect_lib_symbols(path: &Path, seen: &mut BTreeSet<PathBuf>, out: &mut Vec<LibrarySymbol>, notes: &mut Vec<String>) {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !seen.insert(canonical) {
        return;
    }
    let Ok(text) = std::fs::read_to_string(path) else { return };
    let Ok(tree) = sexpr::parse(&text) else { return };
    let Some(root) = tree.as_list() else { return };
    for cache in sexpr::find_all(root, "lib_symbols") {
        let parsed = crate::symbol_import::symbols_of(cache);
        notes.extend(parsed.warnings);
        out.extend(parsed.symbols);
    }
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    for sheet in sexpr::find_all(root, "sheet") {
        let file = sexpr::find_all(sheet, "property").find(|p| sexpr::txt(p, 1) == Some("Sheetfile")).and_then(|p| sexpr::txt(p, 2));
        if let Some(file) = file {
            collect_lib_symbols(&dir.join(file), seen, out, notes);
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

    #[test]
    fn a_sheet_whose_file_is_missing_comes_in_empty_and_is_said() {
        let (dir, names) = written("missing_child");
        let child = names.iter().find(|n| *n != "whole.kicad_sch").unwrap();
        std::fs::remove_file(dir.join(child)).unwrap();
        let file = load_sheet_file(&dir.join("whole.kicad_sch")).unwrap();
        assert!(file.notes.iter().any(|n| n.contains("could not be read")), "{:?}", file.notes);
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
