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
