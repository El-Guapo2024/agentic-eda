//! Writing the library editors' symbols and footprints into the libraries of the library tables: a `.kicad_sym` file, a KiCad 10 folder library
//! (`Device.kicad_symdir/`, a `.kicad_sym` per symbol) and a `.pretty` folder of `.kicad_mod` files.
//!
//! KiCad's symbol library is a cache of every symbol of the library (`SCH_IO_KICAD_SEXPR_LIB_CACHE`): `SCH_IO_KICAD_SEXPR::SaveSymbol` puts the symbol into
//! the cache (`SCH_IO_LIB_CACHE::AddSymbol`: a symbol of the same name is replaced) and `SCH_IO_KICAD_SEXPR_LIB_CACHE::Save` writes the cache back -- a file
//! library as one file, its symbols ordered by inheritance depth and then by name; a folder library file by file, each symbol in the file it was read from
//! (`m_symbolSourceFiles`, filled by `Load`) and a new one in a file of its own named `EscapeString( name, CTX_FILENAME ).kicad_sym`, each file's symbols in
//! the same order. A footprint library is a folder: `PCB_IO_KICAD_SEXPR::FootprintSave` writes the one footprint's file,
//! `ReplaceIllegalFileNameChars( name, '_' ).kicad_mod`, and nothing else (`FP_CACHE::Save( aFootprint )`).
//!
//! The symbols and footprints written are the editors' own (`LibrarySymbol`, `LibraryFootprint`), through this crate's writers
//! ([`crate::symbol_lib`]'s `write_symbol_named_with`, [`crate::export_kicad_mod_with`]): no second formatter.
//!
//! Where this differs from KiCad:
//!   * the symbols of a library that are not being saved are kept as the file has them, character for character, where KiCad parses every symbol and
//!     formats it again: this port's reader does not hold every token of every symbol, and a library saved here must not lose what it does not read. The
//!     library's header is this writer's (`(version 20250324)`, generator `eda`), so a symbol kept from a newer file is read under that version: KiCad's
//!     reader gates only behaviours on it (`~` as an empty text before 20250318, the DeMorgan body style told from the drawing before 20250827, which
//!     comes to the same for a symbol whose `_N_2` units exist), not tokens;
//!   * a folder library writes only the files whose symbols changed, where `Save` writes every file of the folder again;
//!   * a library that does not read as a whole (a symbol cut off, a parenthesis missing) is refused with KiCad's message for a library loaded with
//!     errors ("Cannot save library '%s' because it had a parse error during loading.") -- this reader finds the errors with an s-expression parse of the
//!     file, not with KiCad's symbol parser;
//!   * every file is written to a sibling temporary file and renamed over the old one (`PRETTIFIED_FILE_OUTPUTFORMATTER` writes in place for a symbol
//!     library; a footprint "writes atomically via sibling temp + rename"); a symbolic link is written through, as `GetRealFile` and
//!     `ResolvePossibleSymlinks` do;
//!   * `IsLibraryWritable` of a folder library asks about the folder itself (`wxFileName::IsDirWritable` of a path without a trailing separator asks about
//!     its parent folder).

use crate::symbol_scan::library_forms;
use eda_model::embedded_files::EmbeddedFiles;
use eda_model::ir::{LibraryFootprint, LibrarySymbol};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One symbol to write: the editor's definition (its `lib_id` may carry the library's nickname; the name in the file is the bare name) and the files it embeds.
#[derive(Clone, Copy, Debug)]
pub struct SymbolSave<'a> {
    pub symbol: &'a LibrarySymbol,
    pub embedded: Option<&'a EmbeddedFiles>,
}

/// What a save wrote: the files, in the order written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LibraryWrite {
    pub written: Vec<PathBuf>,
}

// ------------------------------------------------------------------------------------------------ names and checks

/// `EscapeString( aName, CTX_FILENAME )` (common/string_utils.cpp): the file name a folder library gives a new symbol, without the extension.
pub fn escape_file_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '/' => out.push_str("{slash}"),
            '\\' => out.push_str("{backslash}"),
            '"' => out.push_str("{dblquote}"),
            '<' => out.push_str("{lt}"),
            '>' => out.push_str("{gt}"),
            '|' => out.push_str("{bar}"),
            ':' => out.push_str("{colon}"),
            '\t' => out.push_str("{tab}"),
            '\n' | '\r' => out.push_str("{return}"),
            c => out.push(c),
        }
    }
    out
}

/// The file of a new symbol in a folder library: `EscapeString( name, CTX_FILENAME ).kicad_sym` (`SCH_IO_KICAD_SEXPR_LIB_CACHE::Save`).
pub fn symbol_file_name(name: &str) -> String {
    format!("{}.{}", escape_file_name(name), crate::SYMBOL_LIB_FILE_EXT)
}

/// `ReplaceIllegalFileNameChars( fpName, '_' )` + `.kicad_mod` (`PCB_IO_KICAD_SEXPR::FootprintSave`): the characters `\/:"<>|*?` become `_`.
pub fn footprint_file_name(name: &str) -> String {
    let stem: String = name.chars().map(|c| if "\\/:\"<>|*?".contains(c) { '_' } else { c }).collect();
    format!("{stem}.kicad_mod")
}

/// `SCH_IO_KICAD_SEXPR::IsLibraryWritable`: a file that exists is writable when the file is; a folder library when the folder is (see the module doc).
pub fn is_symbol_library_writable(path: &Path) -> bool {
    crate::design_block::may_write(path)
}

/// `FP_CACHE::IsWritable`: `m_lib_path.IsOk() && m_lib_path.IsDirWritable()`, the `.pretty` folder.
pub fn is_footprint_library_writable(path: &Path) -> bool {
    path.is_dir() && crate::design_block::may_write(path)
}

/// `SCH_IO_KICAD_SEXPR_LIB_CACHE::Save`'s refusal of a library whose load had errors (issue #22241), word for word.
fn parse_error_message(path: &Path) -> String {
    format!(
        "Cannot save library '{}' because it had a parse error during loading.\n\nSaving would permanently lose symbols that could not be loaded.\nPlease fix the library file manually before saving.",
        path.display()
    )
}

// ------------------------------------------------------------------------------------------------ writing a file

/// Where a write to `path` lands: through a symbolic link to the file it names (`GetRealFile`, `ResolvePossibleSymlinks`).
fn real_file(path: &Path) -> PathBuf {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()),
        _ => path.to_path_buf(),
    }
}

/// `text` into `path` by a sibling temporary file renamed over it: a reader never sees half a library. The temporary file is hidden (`.<name>.eda-save`), so
/// the folder library's reader (which skips hidden files) never takes it for a library file.
pub(crate) fn write_atomically(path: &Path, text: &str) -> Result<PathBuf, String> {
    let path = real_file(path);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or_else(|| format!("'{}' is not a file name.", path.display()))?;
    let tmp = path.with_file_name(format!(".{name}.eda-save"));
    std::fs::write(&tmp, text).map_err(|e| format!("Cannot write '{}': {e}", path.display()))?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("Cannot write '{}': {e}", path.display()));
    }
    Ok(path)
}

// ------------------------------------------------------------------------------------------------ symbols

/// One top-level `(symbol ...)` form of a library: its name, its text (from `(symbol` to its closing parenthesis) and the symbol it extends.
#[derive(Clone, Debug)]
struct Block {
    name: String,
    text: String,
    parent: Option<String>,
}

/// The `(extends "Name")` of a symbol form, found as the chooser's scanner finds it.
fn extends_of(form: &str) -> Option<String> {
    crate::scan_symbols(&format!("(kicad_symbol_lib {form})")).into_iter().next().and_then(|s| s.extends)
}

/// The symbol forms of one `.kicad_sym` text, in file order, or `None` when the text does not read as a library (see the module doc).
fn read_blocks(text: &str) -> Option<Vec<Block>> {
    crate::sexpr::parse(text).ok()?;
    let forms = library_forms(text)?;
    Some(
        forms
            .symbols
            .into_iter()
            .map(|(name, span)| {
                let form = text[span].to_string();
                Block { parent: extends_of(&form), name, text: form }
            })
            .collect(),
    )
}

/// A symbol as `SCH_IO_KICAD_SEXPR_LIB_CACHE::SaveSymbol` writes it into a library, under its bare name: a derived symbol as `(symbol "Name" (extends
/// "Parent") ..)` with its fields, a root symbol with its units.
fn written_block(save: &SymbolSave) -> Block {
    let bare = eda_model::sym_fields::bare_name(&save.symbol.lib_id).to_string();
    let mut out = String::new();
    crate::symbol_lib::write_symbol_named_with(&mut out, save.symbol, &bare, save.embedded);
    let text = out.strip_prefix('\t').unwrap_or(&out).trim_end_matches('\n').to_string();
    Block { name: bare, text, parent: save.symbol.extends.as_deref().map(|p| eda_model::sym_fields::bare_name(p).to_string()) }
}

/// `LIB_SYMBOL::GetInheritanceDepth` of every symbol of a library: 0 for a root, one more than its parent for a derived symbol (a parent the library lacks,
/// or a loop, ends the count).
fn inheritance_depths(blocks: &[&Block]) -> HashMap<String, usize> {
    let parent: HashMap<&str, Option<&str>> = blocks.iter().map(|b| (b.name.as_str(), b.parent.as_deref())).collect();
    let mut out = HashMap::new();
    for b in blocks {
        let (mut depth, mut at) = (0usize, b.parent.as_deref());
        while let Some(p) = at {
            if p == b.name || depth > blocks.len() {
                break;
            }
            match parent.get(p) {
                Some(next) => {
                    depth += 1;
                    at = *next;
                }
                None => break,
            }
        }
        out.insert(b.name.clone(), depth);
    }
    out
}

/// `formatLibraryHeader`, then the symbols "ordered by inheritance depth" and by name (`GetInheritanceDepth`, then `GetName() <`), then the closing parenthesis.
fn library_text(blocks: &[&Block], depths: &HashMap<String, usize>) -> String {
    let mut ordered: Vec<&Block> = blocks.to_vec();
    ordered.sort_by(|a, b| depths.get(&a.name).cmp(&depths.get(&b.name)).then_with(|| a.name.cmp(&b.name)));
    let mut out = crate::symbol_lib::library_header();
    for b in ordered {
        out.push('\t');
        out.push_str(&b.text);
        out.push('\n');
    }
    out.push_str(")\n");
    out
}

/// A derived symbol written into a library must have its parent there (`LIB_BUFFER::SaveBuffer` saves the buffered parent first; `wxCHECK( oldParent )`
/// refuses one with none: "Derived symbol '%s' found with undefined parent.").
fn check_parents(all: &HashMap<&str, &Block>, written: &[Block]) -> Result<(), String> {
    for b in written {
        if let Some(p) = &b.parent {
            if !all.contains_key(p.as_str()) {
                return Err(format!("Derived symbol '{}' found with undefined parent.", b.name));
            }
        }
    }
    Ok(())
}

/// `SCH_IO_KICAD_SEXPR::SaveSymbol` of each symbol (`AddSymbol`: a symbol of the same name is replaced) and then `SCH_IO_KICAD_SEXPR_LIB_CACHE::Save`: the
/// library at `path` -- a `.kicad_sym` file, which need not exist yet (`CreateLibrary` makes an empty one), or a folder library -- with `symbols` in it.
/// A derived symbol's parent must be in the library or among `symbols`.
pub fn save_symbols(path: &Path, symbols: &[SymbolSave]) -> Result<LibraryWrite, String> {
    if path.is_dir() {
        return save_symbols_in_folder(path, symbols);
    }
    let mut blocks: Vec<Block> = match std::fs::read_to_string(path) {
        Ok(text) => read_blocks(&text).ok_or_else(|| parse_error_message(path))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let written: Vec<Block> = symbols.iter().map(written_block).collect();
    for w in &written {
        // `m_symbols[ name ] = symbol`: the one of that name (a file holding the name twice keeps the last, as `ParseLib` does) gives way
        blocks.retain(|b| b.name != w.name);
    }
    blocks.extend(written.iter().cloned());
    let by_name: HashMap<&str, &Block> = blocks.iter().map(|b| (b.name.as_str(), b)).collect();
    check_parents(&by_name, &written)?;
    // a name read twice is the later form
    let unique: Vec<&Block> = blocks.iter().enumerate().filter(|(i, b)| blocks.iter().rposition(|o| o.name == b.name) == Some(*i)).map(|(_, b)| b).collect();
    let depths = inheritance_depths(&unique);
    let file = write_atomically(path, &library_text(&unique, &depths))?;
    Ok(LibraryWrite { written: vec![file] })
}

/// The folder branch of `SCH_IO_KICAD_SEXPR_LIB_CACHE::Save` (see the module doc): each symbol into the file it was read from, a new one into a file of its
/// own, each file written with the symbols that are its own (`m_symbolSourceFiles`: a name two files define is the later file's). Only the files a symbol
/// went into are written.
fn save_symbols_in_folder(dir: &Path, symbols: &[SymbolSave]) -> Result<LibraryWrite, String> {
    // (file, its forms) in the order `Load` reads them here (by name: `crate::symbol_lib_dir`)
    let mut files: Vec<(PathBuf, Vec<Block>)> = Vec::new();
    for file in crate::symbol_library_files(dir) {
        let text = std::fs::read_to_string(&file).map_err(|_| parse_error_message(dir))?;
        files.push((file, read_blocks(&text).ok_or_else(|| parse_error_message(dir))?));
    }
    // `m_symbolSourceFiles`: the file each name was last read from
    let mut source: HashMap<String, usize> = HashMap::new();
    for (i, (_, blocks)) in files.iter().enumerate() {
        for b in blocks {
            source.insert(b.name.clone(), i);
        }
    }
    let written: Vec<Block> = symbols.iter().map(written_block).collect();
    let mut dirty: Vec<usize> = Vec::new();
    for w in &written {
        let at = match source.get(&w.name) {
            Some(&i) => i,
            None => {
                // "New symbol without source file - create individual file", or the file of that name when there is one
                let new_path = dir.join(symbol_file_name(&w.name));
                match files.iter().position(|(p, _)| *p == new_path) {
                    Some(i) => i,
                    None => {
                        files.push((new_path, Vec::new()));
                        files.len() - 1
                    }
                }
            }
        };
        let blocks = &mut files[at].1;
        blocks.retain(|b| b.name != w.name);
        blocks.push(w.clone());
        source.insert(w.name.clone(), at);
        if !dirty.contains(&at) {
            dirty.push(at);
        }
    }
    // every symbol of the library as `Load` leaves it (one form per name), for the parents and the inheritance depths
    let source_ref = &source;
    let library: HashMap<&str, &Block> = files.iter().enumerate().flat_map(|(i, (_, blocks))| blocks.iter().filter(move |b| source_ref.get(&b.name) == Some(&i))).map(|b| (b.name.as_str(), b)).collect();
    check_parents(&library, &written)?;
    let all: Vec<&Block> = library.values().copied().collect();
    let depths = inheritance_depths(&all);
    if !dir.is_dir() {
        std::fs::create_dir_all(dir).map_err(|_| format!("Cannot create symbol library path '{}'.", dir.display()))?;
    }
    let mut out = LibraryWrite::default();
    dirty.sort_by(|a, b| files[*a].0.cmp(&files[*b].0));
    for i in dirty {
        let (file, blocks) = &files[i];
        let own: Vec<&Block> = blocks.iter().enumerate().filter(|(k, b)| source.get(&b.name) == Some(&i) && blocks.iter().rposition(|o| o.name == b.name) == Some(*k)).map(|(_, b)| b).collect();
        out.written.push(write_atomically(file, &library_text(&own, &depths))?);
    }
    Ok(out)
}

/// `SYMBOL_EDIT_FRAME::saveLibrary( aLibrary, aNewFile = true )` -> `SYMBOL_LIBRARY_MANAGER::SaveLibrary` to a new file: every symbol of the library at `src`
/// (a file or a folder; the library's buffer holds them all, `getLibraryBuffer`), the ones of `symbols` in place of their saved forms, as one `.kicad_sym` at
/// `dst` -- `SCH_IO_KICAD_SEXPR::SaveLibrary` writes the cache to the new path as a file. `src` may not exist yet (a library made and never saved).
pub fn save_symbol_library_as(src: &Path, dst: &Path, symbols: &[SymbolSave]) -> Result<LibraryWrite, String> {
    let mut blocks: Vec<Block> = if src.is_dir() {
        let text = crate::read_symbol_library_text(src)?;
        let all = read_blocks(&text).ok_or_else(|| parse_error_message(src))?;
        // a folder whose file does not read is refused as `Load` would leave it ("loaded with errors")
        for file in crate::symbol_library_files(src) {
            if std::fs::read_to_string(&file).ok().and_then(|t| read_blocks(&t)).is_none() {
                return Err(parse_error_message(src));
            }
        }
        all
    } else {
        match std::fs::read_to_string(src) {
            Ok(text) => read_blocks(&text).ok_or_else(|| parse_error_message(src))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(format!("{}: {e}", src.display())),
        }
    };
    let written: Vec<Block> = symbols.iter().map(written_block).collect();
    for w in &written {
        blocks.retain(|b| b.name != w.name);
    }
    blocks.extend(written.iter().cloned());
    let unique: Vec<&Block> = blocks.iter().enumerate().filter(|(i, b)| blocks.iter().rposition(|o| o.name == b.name) == Some(*i)).map(|(_, b)| b).collect();
    let by_name: HashMap<&str, &Block> = unique.iter().map(|b| (b.name.as_str(), *b)).collect();
    check_parents(&by_name, &written)?;
    let depths = inheritance_depths(&unique);
    if let Some(parent) = dst.parent().filter(|p| !p.as_os_str().is_empty()) {
        if !parent.is_dir() {
            return Err(format!("The folder '{}' does not exist.", parent.display()));
        }
    }
    let file = write_atomically(dst, &library_text(&unique, &depths))?;
    Ok(LibraryWrite { written: vec![file] })
}

// ------------------------------------------------------------------------------------------------ footprints

/// `PCB_IO_KICAD_SEXPR::FootprintSave( aLibraryPath, aFootprint )` into the `.pretty` folder `pretty`: the footprint, named by the bare name of its
/// `name` (`SaveFootprintInLibrary` clears the library nickname of the FPID before the save, so the file says `(footprint "Name"` and the Value that
/// follows the name is the bare name), as `ReplaceIllegalFileNameChars( name, '_' ).kicad_mod`, quietly replacing a footprint of that name. A folder that
/// cannot be written is refused with KiCad's words ("Library '%s' is read only.", the folder's path); so is a file of that name that cannot be
/// ("Insufficient permissions to delete '%s'."). The answer is the file written.
pub fn save_footprint(pretty: &Path, fp: &LibraryFootprint, embedded: &EmbeddedFiles) -> Result<PathBuf, String> {
    if !is_footprint_library_writable(pretty) {
        return Err(format!("Library '{}' is read only.", pretty.display()));
    }
    let bare = fp.name.rsplit_once(':').map_or(fp.name.as_str(), |(_, item)| item);
    if bare.is_empty() {
        return Err(format!("Footprint file name '{}' is not valid.", pretty.join(footprint_file_name(bare)).display()));
    }
    let file = real_file(&pretty.join(footprint_file_name(bare)));
    if file.is_file() && !crate::design_block::may_write(&file) {
        return Err(format!("Insufficient permissions to delete '{}'.", file.display()));
    }
    let mut copy = fp.clone();
    copy.name = bare.to_string();
    write_atomically(&file, &crate::export_kicad_mod_with(&copy, embedded))
}

/// The footprint files of a `.pretty` folder (`FP_CACHE::Load` reads `*.kicad_mod`), sorted by name; hidden files left out.
fn footprint_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("kicad_mod") && p.is_file() && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
        .collect();
    files.sort();
    files
}

/// `FOOTPRINT_EDIT_FRAME::SaveLibraryAs`'s copy, after `CreateNewLibrary` made the `.pretty` folder `dst`: every footprint of the library `src`
/// (`FootprintEnumerate`, each one named by its file) saved into `dst` (`FootprintSave`). The footprints are copied as their files have them (KiCad formats
/// each one again; the module doc says why this port does not). The answer is the names copied, in order.
pub fn copy_footprint_library(src: &Path, dst: &Path) -> Result<Vec<String>, String> {
    if !src.is_dir() {
        return Err(format!("Library '{}' not found.", src.display()));
    }
    if !is_footprint_library_writable(dst) {
        return Err(format!("Library '{}' is read only.", dst.display()));
    }
    let mut names = Vec::new();
    for file in footprint_files(src) {
        let name = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        write_atomically(&dst.join(footprint_file_name(&name)), &text)?;
        names.push(name);
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eda-library-save-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const LIB: &str = "(kicad_symbol_lib\n\t(version 20251024)\n\t(generator \"kicad_symbol_editor\")\n\t(symbol \"Zeta\"\n\t\t(property \"Reference\" \"U\" (at 0 0 0))\n\t\t(property \"Value\" \"Zeta\" (at 0 0 0))\n\t\t(symbol \"Zeta_1_1\" (pin input line (at 0 0 0) (length 2.54) (name \"A\") (number \"1\")))\n\t)\n\t(symbol \"Alpha\" (extends \"Zeta\")\n\t\t(property \"Value\" \"Alpha\" (at 0 0 0))\n\t)\n)\n";

    fn symbol(lib_id: &str) -> LibrarySymbol {
        let mut s = LibrarySymbol::new_empty(lib_id);
        s.reference_prefix = "R".into();
        s.pins.push(eda_model::ir::LibrarySymbolPin::from_engine_pin(&eda_model::symbol::LibPin {
            number: "1".into(),
            name: "~".into(),
            electrical_type: "passive".into(),
            shape: "line".into(),
            at: eda_model::symbol::SPoint::new(0.0.into(), 3.81.into()),
            angle_deg: 270.0,
            length_mm: 1.27,
            unit: 1,
            hidden: false,
        }));
        s.assign_missing_ids();
        s
    }

    #[test]
    fn file_names_are_kicads() {
        assert_eq!(symbol_file_name("R"), "R.kicad_sym");
        assert_eq!(symbol_file_name("A/B:C\"D"), "A{slash}B{colon}C{dblquote}D.kicad_sym");
        assert_eq!(footprint_file_name("R_0603:1608*?"), "R_0603_1608__.kicad_mod");
    }

    #[test]
    fn a_new_symbol_goes_into_the_file_and_the_others_are_kept_as_they_were() {
        let dir = temp_dir("file");
        let path = dir.join("Mine.kicad_sym");
        std::fs::write(&path, LIB).unwrap();
        let new = symbol("Mine:Beta");
        save_symbols(&path, &[SymbolSave { symbol: &new, embedded: None }]).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        // the kept symbols are character for character the file's, and the order is by inheritance depth, then name
        assert!(text.contains("\t(symbol \"Zeta\"\n\t\t(property \"Reference\" \"U\" (at 0 0 0))"), "{text}");
        assert!(text.contains("\t(symbol \"Alpha\" (extends \"Zeta\")\n\t\t(property \"Value\" \"Alpha\" (at 0 0 0))\n\t)"), "{text}");
        let order: Vec<String> = crate::scan_symbols(&text).into_iter().map(|s| s.name).collect();
        assert_eq!(order, ["Beta", "Zeta", "Alpha"]);
        // it reads back as the symbol saved
        let back = crate::parse_library_symbols(&text).unwrap();
        let beta = back.symbols.iter().find(|s| s.lib_id == "Beta").expect("Beta read back");
        assert_eq!(beta.pins.len(), 1);
        assert_eq!(beta.reference_prefix, "R");
        assert!(!dir.join(".Mine.kicad_sym.eda-save").exists(), "no temporary file is left");
    }

    #[test]
    fn a_symbol_of_the_same_name_is_replaced_and_a_missing_file_is_made() {
        let dir = temp_dir("replace");
        let path = dir.join("New.kicad_sym");
        let mut a = symbol("New:Zeta");
        save_symbols(&path, &[SymbolSave { symbol: &a, embedded: None }]).unwrap();
        a.reference_prefix = "Q".into();
        save_symbols(&path, &[SymbolSave { symbol: &a, embedded: None }]).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let names: Vec<String> = crate::scan_symbols(&text).into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["Zeta"]);
        assert!(text.contains("\"Reference\" \"Q\""), "{text}");
    }

    #[test]
    fn a_derived_symbol_needs_its_parent_and_a_broken_library_is_not_saved() {
        let dir = temp_dir("parents");
        let path = dir.join("Lib.kicad_sym");
        let mut child = symbol("Lib:Child");
        child.extends = Some("Lib:Nobody".into());
        let err = save_symbols(&path, &[SymbolSave { symbol: &child, embedded: None }]).unwrap_err();
        assert_eq!(err, "Derived symbol 'Child' found with undefined parent.");
        assert!(!path.exists());
        // the parent saved with it is enough, and the parent comes first
        let parent = symbol("Lib:Nobody");
        save_symbols(&path, &[SymbolSave { symbol: &child, embedded: None }, SymbolSave { symbol: &parent, embedded: None }]).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(crate::scan_symbols(&text).into_iter().map(|s| (s.name, s.extends)).collect::<Vec<_>>(), [("Nobody".to_string(), None), ("Child".to_string(), Some("Nobody".to_string()))]);
        // a library cut off in the middle keeps its text: nothing is written
        let broken = dir.join("Broken.kicad_sym");
        std::fs::write(&broken, &LIB[..LIB.len() - 20]).unwrap();
        let err = save_symbols(&broken, &[SymbolSave { symbol: &parent, embedded: None }]).unwrap_err();
        assert!(err.starts_with("Cannot save library '") && err.contains("because it had a parse error during loading."), "{err}");
        assert_eq!(std::fs::read_to_string(&broken).unwrap(), &LIB[..LIB.len() - 20]);
    }

    #[test]
    fn a_folder_library_writes_the_symbols_file_and_a_new_file_for_a_new_symbol() {
        let dir = temp_dir("folder");
        let lib = dir.join("Mine.kicad_symdir");
        std::fs::create_dir(&lib).unwrap();
        // one file holding two symbols (kept together), and a file of its own for a third
        std::fs::write(lib.join("Zeta.kicad_sym"), LIB).unwrap();
        let other = "(kicad_symbol_lib (version 20251024)\n\t(symbol \"Other\" (property \"Reference\" \"U\") (symbol \"Other_1_1\"))\n)\n";
        std::fs::write(lib.join("Other.kicad_sym"), other).unwrap();
        let mut alpha = symbol("Mine:Alpha");
        alpha.extends = Some("Mine:Zeta".into());
        let slash = symbol("Mine:A/B");
        let w = save_symbols(&lib, &[SymbolSave { symbol: &alpha, embedded: None }, SymbolSave { symbol: &slash, embedded: None }]).unwrap();
        assert_eq!(w.written, [lib.join("A{slash}B.kicad_sym"), lib.join("Zeta.kicad_sym")]);
        assert_eq!(std::fs::read_to_string(lib.join("Other.kicad_sym")).unwrap(), other, "a file none of the symbols is in is not written");
        let zeta = std::fs::read_to_string(lib.join("Zeta.kicad_sym")).unwrap();
        assert_eq!(crate::scan_symbols(&zeta).into_iter().map(|s| s.name).collect::<Vec<_>>(), ["Zeta", "Alpha"], "the derived symbol stays in its parent's file, after it");
        assert!(zeta.contains("\t(symbol \"Zeta\"\n\t\t(property \"Reference\" \"U\" (at 0 0 0))"));
        // the folder reads back as one library with the three of them
        let text = crate::read_symbol_library_text(&lib).unwrap();
        let mut names: Vec<String> = crate::scan_symbols(&text).into_iter().map(|s| s.name).collect();
        names.sort();
        assert_eq!(names, ["A/B", "Alpha", "Other", "Zeta"]);
    }

    #[test]
    fn a_new_derived_symbol_in_a_folder_library_finds_its_parent_in_another_file() {
        let dir = temp_dir("folder-parent");
        let lib = dir.join("Mine.kicad_symdir");
        std::fs::create_dir(&lib).unwrap();
        std::fs::write(lib.join("Zeta.kicad_sym"), LIB).unwrap();
        let mut kid = symbol("Mine:Kid");
        kid.extends = Some("Mine:Zeta".into());
        let w = save_symbols(&lib, &[SymbolSave { symbol: &kid, embedded: None }]).unwrap();
        assert_eq!(w.written, [lib.join("Kid.kicad_sym")]);
        let kid_text = std::fs::read_to_string(lib.join("Kid.kicad_sym")).unwrap();
        assert!(kid_text.contains("(symbol \"Kid\" (extends \"Zeta\")"), "{kid_text}");
        assert_eq!(std::fs::read_to_string(lib.join("Zeta.kicad_sym")).unwrap(), LIB);
    }

    #[test]
    fn save_library_as_writes_every_symbol_with_the_edits_into_one_file() {
        let dir = temp_dir("save-as");
        let lib = dir.join("Mine.kicad_symdir");
        std::fs::create_dir(&lib).unwrap();
        std::fs::write(lib.join("Zeta.kicad_sym"), LIB).unwrap();
        let mut edited = symbol("Mine:Zeta");
        edited.reference_prefix = "Q".into();
        let dst = dir.join("Copy.kicad_sym");
        save_symbol_library_as(&lib, &dst, &[SymbolSave { symbol: &edited, embedded: None }]).unwrap();
        let text = std::fs::read_to_string(&dst).unwrap();
        assert_eq!(crate::scan_symbols(&text).into_iter().map(|s| s.name).collect::<Vec<_>>(), ["Zeta", "Alpha"]);
        assert!(text.contains("\"Reference\" \"Q\""));
        assert_eq!(std::fs::read_to_string(lib.join("Zeta.kicad_sym")).unwrap(), LIB, "the library saved from is untouched");
    }

    #[test]
    fn a_footprint_is_written_under_its_bare_name_and_a_read_only_library_is_refused() {
        let dir = temp_dir("pretty");
        let pretty = dir.join("Mine.pretty");
        std::fs::create_dir(&pretty).unwrap();
        let mut fp = LibraryFootprint::new_empty("Mine:R_0603");
        fp.assign_missing_ids();
        let file = save_footprint(&pretty, &fp, &EmbeddedFiles::default()).unwrap();
        assert_eq!(file, pretty.join("R_0603.kicad_mod"));
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.starts_with("(footprint \"R_0603\""), "{text}");
        assert!(text.contains("(property \"Value\" \"R_0603\""), "the Value that follows the name is the bare name: {text}");
        let back = crate::parse_library_footprint(&text).unwrap().footprint;
        assert_eq!(back.name, "R_0603");
        // Save Library As copies every footprint file
        let copy = dir.join("Copy.pretty");
        std::fs::create_dir(&copy).unwrap();
        assert_eq!(copy_footprint_library(&pretty, &copy).unwrap(), ["R_0603"]);
        assert_eq!(std::fs::read_to_string(copy.join("R_0603.kicad_mod")).unwrap(), text);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let is_root = std::fs::read_to_string("/proc/self/status").ok().and_then(|s| s.lines().find_map(|l| l.strip_prefix("Uid:")).map(|r| r.split_whitespace().nth(1) == Some("0"))).unwrap_or(false);
            if !is_root {
                std::fs::set_permissions(&pretty, std::fs::Permissions::from_mode(0o555)).unwrap();
                let err = save_footprint(&pretty, &fp, &EmbeddedFiles::default()).unwrap_err();
                std::fs::set_permissions(&pretty, std::fs::Permissions::from_mode(0o755)).unwrap();
                assert_eq!(err, format!("Library '{}' is read only.", pretty.display()));
                assert!(!is_symbol_library_writable(&dir.join("nothing.kicad_sym")));
            }
        }
    }
}
