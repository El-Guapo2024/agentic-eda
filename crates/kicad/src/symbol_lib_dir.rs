//! A symbol library that is a folder (KiCad 10): `Device.kicad_symdir/` holds one `.kicad_sym` file per symbol (`R.kicad_sym`, `C.kicad_sym`, and a
//! derived `Filter_EMI_C.kicad_sym` whose symbol `(extends "C_Feedthrough")` of another file) where `Device.kicad_sym` held them all. KiCad's own
//! symbol library (kicad-symbols, `${KICAD10_SYMBOL_DIR}`) comes in this form only.
//!
//! KiCad tells the two forms apart by the path alone: a path that is an existing folder is a folder library, whatever its name (`SCH_IO_LIB_CACHE`'s
//! constructor and `SCH_IO_KICAD_SEXPR_LIB_CACHE::Load`: `wxFileName::DirExists` -> `AssignDir`). `.kicad_symdir` is only the name KiCad's library
//! repository and its stock `sym-lib-table` give such a folder (`${KICAD10_SYMBOL_DIR}/Device.kicad_symdir`; "By default KiCad uses paths suffixed with
//! .kicad_symdir", `SYMBOL_EDIT_FRAME::saveLibrary`). `Load` reads every `*.kicad_sym` file of the folder in turn into the one symbol map
//! (`parser.ParseLib( m_symbols )`, where a name read again replaces the symbol read before it: `aSymbolLibMap[symbol->GetName()] = symbol`), and links
//! each derived symbol to its parent once every file is read (`updateParentSymbolLinks`), so a parent may be in any file of the folder.
//!
//! Every reader of a library in this repo takes the library's text (`scan_symbols` for the choosers, `scan_symbol_names` for the trees,
//! `parse_symbol_library` for the schematic's symbols, `sim_fields_of`), so a folder is read here as the one `.kicad_sym` text it stands for
//! ([`read_symbol_library_text`]): the `(symbol ...)` forms of its files, each name once (the last one read), in one `(kicad_symbol_lib ...)`.
//!
//! Where this differs from KiCad:
//!   * the files are read in the order of their names, so that which of two files defining one name wins is the same on every machine; `Load` reads
//!     them in the folder's own order (`wxDir::GetFirst`/`GetNext`, the file system's `readdir`);
//!   * a hidden file (`._R.kicad_sym`, the AppleDouble file a Mac copy leaves) is not read, as `SCH_IO_KICAD_SEXPR::CanReadLibrary` (`wxDIR_FILES`) and
//!     `KIPLATFORM::IO::TimestampDir` (`FNM_PERIOD`) skip it; `Load`'s `wxDir::GetFirst( .., wxDIR_DEFAULT )` includes it, and fails on it;
//!   * a file that cannot be read, or is not a `(kicad_symbol_lib ...)`, is left out and the rest is read, as `Load` goes on to the next file; and as
//!     KiCad 25ec2335's `Load` does, the library is then one "loaded with errors" ([`symbol_library_load_errors`]: "Unable to read file '%s'" and the
//!     reason, each file's, then "The library cannot be saved until these errors are fixed manually."), which the library tables' status, the choosers
//!     and the status bar show and `crate::library_save` refuses to save. The reason is this reader's: a file that is not `(kicad_symbol_lib ...)` or
//!     whose forms do not close (`Expecting '..' in '%s', line %d, offset %d.`, the words of KiCad's lexer, at the place this reader finds); a symbol
//!     KiCad's parser would refuse inside a well-formed file (`m_parseWarnings`) is not found, nor a folder whose name ends `.kicad_sym` (`wxDir`
//!     lists it and `FILE_LINE_READER` cannot open it);
//!   * the text has one `(version N)`, the last file's (`SetFileFormatVersionAtLoad` keeps the last file's too), where `Load` parses each file with its
//!     own: the readers that look at the version (a `~` that is an empty text before 20250318) read every file of the folder as the last one is;
//!   * the folder's modification time ([`symbol_library_modified`]) is the latest of the folder's and its files', where `TimestampDir` sums every
//!     file's time and size: a file put back with an older time is not seen as a change.
//!
//! Writing a folder library (`SCH_IO_KICAD_SEXPR_LIB_CACHE::Save`'s folder branch: a file per symbol, `EscapeString( name, CTX_FILENAME ).kicad_sym`
//! for a new one) is `crate::library_save`; the studio's New Library makes a `.kicad_sym` file, as `SYMBOL_EDIT_FRAME::AddLibraryFile` does.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A symbol library file's extension (`FILEEXT::KiCadSymbolLibFileExtension`).
pub const SYMBOL_LIB_FILE_EXT: &str = "kicad_sym";

/// The extension KiCad's library repository and its stock `sym-lib-table` give a folder library (`Device.kicad_symdir`).
pub const SYMBOL_LIB_DIR_EXT: &str = "kicad_symdir";

fn has_ext(path: &Path, ext: &str) -> bool {
    path.extension().and_then(|x| x.to_str()) == Some(ext)
}

fn hidden(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

/// The `*.kicad_sym` files of the folder library `dir`, sorted by name (see the module doc); hidden files left out. Empty when `dir` is not a folder.
pub fn symbol_library_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .filter(|e| !hidden(&e.file_name()))
        .map(|e| e.path())
        .filter(|p| has_ext(p, SYMBOL_LIB_FILE_EXT) && p.is_file())
        .collect();
    files.sort();
    files
}

/// `SCH_IO_KICAD_SEXPR::CanReadLibrary`: a folder holding at least one `*.kicad_sym` file, or an existing file with the `.kicad_sym` extension.
pub fn can_read_symbol_library(path: &Path) -> bool {
    if path.is_dir() {
        return !symbol_library_files(path).is_empty();
    }
    has_ext(path, SYMBOL_LIB_FILE_EXT) && path.is_file()
}

/// The name a folder listing gives the library at `path` (a row of the folder table, a library of the tree): the stem of a `.kicad_sym` file or of a
/// `.kicad_symdir` folder (`wxFileName::GetName`, as `PANEL_SYM_LIB_TABLE::browseLibrariesHandler` names a row), `None` for anything else.
pub fn symbol_library_name(path: &Path) -> Option<String> {
    let library = (has_ext(path, SYMBOL_LIB_FILE_EXT) && path.is_file()) || (has_ext(path, SYMBOL_LIB_DIR_EXT) && path.is_dir());
    library.then(|| path.file_stem().and_then(|s| s.to_str()).map(str::to_string)).flatten()
}

/// The text of the symbol library at `path`: a `.kicad_sym` file's own text, or for a folder library the one `(kicad_symbol_lib ...)` text of every
/// symbol its files define, a name defined twice kept as the file read last defines it (see the module doc). An error when `path` is neither.
pub fn read_symbol_library_text(path: &Path) -> Result<String, String> {
    read_symbol_library(path).map(|(text, _)| text)
}

/// [`read_symbol_library_text`] and what [`symbol_library_load_errors`] says of the library, from the same read (which is also kept for that lookup).
pub fn read_symbol_library(path: &Path) -> Result<(String, Option<String>), String> {
    let stamp = symbol_library_modified(path);
    if !path.is_dir() {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let problem = library_text_problem(&text, &path.display().to_string());
        remember_load_errors(path, stamp, problem.clone());
        return Ok((text, problem));
    }
    let mut texts: Vec<String> = Vec::new();
    // (which text, the symbol's name, its form's span in that text), in the order read
    let mut forms: Vec<(usize, String, std::ops::Range<usize>)> = Vec::new();
    let mut version: Option<String> = None;
    let mut errors = LoadErrors::default();
    for file in symbol_library_files(path) {
        let text = match std::fs::read(&file) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(t) => t,
                Err(_) => {
                    errors.add(&file, format!("'{}' is not UTF-8 text.", file.display()));
                    continue;
                }
            },
            Err(_) => {
                // `FILE_LINE_READER`'s constructor
                errors.add(&file, format!("Unable to open {} for reading.", file.display()));
                continue;
            }
        };
        if let Some(problem) = library_text_problem(&text, &file.display().to_string()) {
            errors.add(&file, problem);
        }
        // the symbols a file closed before its problem are read, as `ParseLib` puts each symbol in the map as it is parsed
        let Some(lib) = crate::symbol_scan::library_forms(&text) else { continue };
        if lib.version.is_some() {
            version = lib.version;
        }
        let at = texts.len();
        forms.extend(lib.symbols.into_iter().map(|(name, span)| (at, name, span)));
        texts.push(text);
    }
    let last: HashMap<&str, usize> = forms.iter().enumerate().map(|(i, (_, name, _))| (name.as_str(), i)).collect();
    let mut out = String::with_capacity(forms.iter().map(|(_, _, s)| s.len() + 1).sum::<usize>() + 64);
    out.push_str("(kicad_symbol_lib");
    if let Some(v) = &version {
        out.push_str(" (version ");
        out.push_str(v);
        out.push(')');
    }
    out.push('\n');
    for (i, (at, name, span)) in forms.iter().enumerate() {
        if last.get(name.as_str()) == Some(&i) {
            out.push_str(&texts[*at][span.clone()]);
            out.push('\n');
        }
    }
    out.push_str(")\n");
    let errors = errors.finish();
    remember_load_errors(path, stamp, errors.clone());
    Ok((out, errors))
}

/// The errors of one folder library's load, put together as KiCad 25ec2335's `SCH_IO_KICAD_SEXPR_LIB_CACHE::Load` puts them (`errorCache`).
#[derive(Default)]
struct LoadErrors(String);

impl LoadErrors {
    /// `errorCache += wxString::Format( _( "Unable to read file '%s'" ) + '\n', sourceFilePath ); errorCache += ioe.What();`, after a blank line.
    fn add(&mut self, file: &Path, reason: String) {
        if !self.0.is_empty() {
            self.0.push_str("\n\n");
        }
        self.0.push_str(&format!("Unable to read file '{}'\n{reason}", file.display()));
    }

    /// `errorCache += _( "\n\nThe library cannot be saved until these errors are fixed manually." )`, when there was any.
    fn finish(mut self) -> Option<String> {
        if self.0.is_empty() {
            return None;
        }
        self.0.push_str("\n\nThe library cannot be saved until these errors are fixed manually.");
        Some(self.0)
    }
}

/// `PARSE_ERROR`'s problem text (`"%s in '%s', line %d, offset %d."`) for the place `at` of `text` (KiCad's lines and offsets count from 1).
fn parse_problem(what: &str, source: &str, text: &str, at: usize) -> String {
    let at = at.min(text.len());
    let before = &text.as_bytes()[..at];
    let line = before.iter().filter(|&&c| c == b'\n').count() + 1;
    let offset = at - before.iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1) + 1;
    format!("{what} in '{source}', line {line}, offset {offset}.")
}

/// Why the text of one `.kicad_sym` file is no symbol library `ParseLib` reads, in the words of KiCad's lexer (`DSNLEXER::Expecting`), or `None`: the first
/// form must be `(kicad_symbol_lib`, and every form must close. A problem inside a well-formed symbol is not looked for (see the module doc).
pub fn library_text_problem(text: &str, source: &str) -> Option<String> {
    let b = text.as_bytes();
    let ws = |c: u8| matches!(c, b' ' | b'\t' | b'\r' | b'\n');
    let mut i = 0;
    while i < b.len() && ws(b[i]) {
        i += 1;
    }
    if b.get(i) != Some(&b'(') {
        return Some(parse_problem("Expecting '('", source, text, i));
    }
    let mut tag = i + 1;
    while tag < b.len() && ws(b[tag]) {
        tag += 1;
    }
    let mut end = tag;
    while end < b.len() && !ws(b[end]) && b[end] != b'(' && b[end] != b')' && b[end] != b'"' {
        end += 1;
    }
    if &b[tag..end] != b"kicad_symbol_lib" {
        return Some(parse_problem("Expecting 'kicad_symbol_lib'", source, text, tag));
    }
    let mut depth = 1usize;
    let mut k = end;
    while k < b.len() && depth > 0 {
        match b[k] {
            b'"' => {
                k += 1;
                while k < b.len() && b[k] != b'"' {
                    k += if b[k] == b'\\' { 2 } else { 1 };
                }
                if k >= b.len() {
                    return Some(parse_problem("Expecting '\"'", source, text, b.len()));
                }
            }
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        k += 1;
    }
    (depth > 0).then(|| parse_problem("Expecting ')'", source, text, b.len()))
}

type LoadErrorCache = std::sync::Mutex<HashMap<PathBuf, (Option<SystemTime>, Option<String>)>>;

fn load_error_cache() -> &'static LoadErrorCache {
    static C: std::sync::OnceLock<LoadErrorCache> = std::sync::OnceLock::new();
    C.get_or_init(Default::default)
}

fn remember_load_errors(path: &Path, stamp: Option<SystemTime>, errors: Option<String>) {
    load_error_cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(path.to_path_buf(), (stamp, errors));
}

/// Whether the symbol library at `path` loads with errors, and KiCad's message when it does: for a folder library KiCad 25ec2335's
/// `SCH_IO_KICAD_SEXPR_LIB_CACHE::Load` ("Unable to read file '%s'" and the reason, for every file that does not read, then "The library cannot be saved until
/// these errors are fixed manually."; the good files are read all the same); for a file library the reason the file does not read. What the library's row
/// in the tables (`LIB_STATUS::error`, `LOAD_STATUS::LOAD_ERROR`), the choosers and the status bar (`GetLibraryLoadErrors`) show. Kept against the
/// library's time ([`symbol_library_modified`]), so a library is read for this once until it changes; any read of its text answers it too.
/// `None` for a library that reads, or nothing at `path`.
pub fn symbol_library_load_errors(path: &Path) -> Option<String> {
    let stamp = symbol_library_modified(path)?;
    if let Some((s, hit)) = load_error_cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(path) {
        if *s == Some(stamp) {
            return hit.clone();
        }
    }
    read_symbol_library(path).ok().and_then(|(_, errors)| errors)
}

/// When the symbol library at `path` last changed, for the caches of what was read of it: a file's modification time; for a folder library the
/// latest of the folder's own (a file added, removed or renamed) and its `*.kicad_sym` files' (a file edited) -- `SCH_IO_LIB_CACHE::GetLibModificationTime`,
/// which takes `KIPLATFORM::IO::TimestampDir( path, "*.kicad_sym" )` of a folder. `None` when there is nothing at `path`.
pub fn symbol_library_modified(path: &Path) -> Option<SystemTime> {
    let meta = std::fs::metadata(path).ok()?;
    let own = meta.modified().ok()?;
    if !meta.is_dir() {
        return Some(own);
    }
    let Ok(entries) = std::fs::read_dir(path) else { return Some(own) };
    let latest = entries
        .filter_map(|e| e.ok())
        .filter(|e| !hidden(&e.file_name()))
        .map(|e| e.path())
        .filter(|p| has_ext(p, SYMBOL_LIB_FILE_EXT))
        .filter_map(|p| std::fs::metadata(p).ok().filter(|m| m.is_file()).and_then(|m| m.modified().ok()))
        .fold(own, SystemTime::max);
    Some(latest)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eda-symdir-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn lib(body: &str) -> String {
        format!("(kicad_symbol_lib\n\t(version 20251024)\n\t(generator \"kicad_symbol_editor\")\n\t(generator_version \"10.0\")\n{body}\n)\n")
    }

    /// A folder library the way kicad-symbols writes one: a file per symbol, a derived symbol in a file of its own, and two files that define one name.
    pub(crate) fn fixture(root: &Path) -> PathBuf {
        let dir = root.join("Mini.kicad_symdir");
        std::fs::create_dir_all(&dir).unwrap();
        let r = r#"	(symbol "R"
		(property "Reference" "R" (at 2.032 0 90))
		(property "Value" "R" (at 0 0 90))
		(property "Description" "Resistor")
		(property "ki_keywords" "R res resistor")
		(property "Sim.Device" "R")
		(symbol "R_0_1" (rectangle (start -1.016 -2.54) (end 1.016 2.54) (stroke (width 0.254) (type default)) (fill (type none))))
		(symbol "R_1_1"
			(pin passive line (at 0 3.81 270) (length 1.27) (name "~") (number "1"))
			(pin passive line (at 0 -3.81 90) (length 1.27) (name "~") (number "2"))))"#;
        let reg = r#"	(symbol "AP1117-15"
		(property "Reference" "U" (at 0 0 0))
		(property "Value" "AP1117-15" (at 0 0 0))
		(property "Description" "1A low dropout regulator")
		(symbol "AP1117-15_0_1" (rectangle (start -5.08 -5.08) (end 5.08 1.905) (stroke (width 0.254) (type default)) (fill (type background))))
		(symbol "AP1117-15_1_1"
			(pin power_in line (at -7.62 0 0) (length 2.54) (name "VI") (number "3"))
			(pin power_out line (at 7.62 0 180) (length 2.54) (name "VO") (number "2"))
			(pin power_in line (at 0 -7.62 90) (length 2.54) (name "GND") (number "1"))))"#;
        // the derived symbol, in a file read BEFORE its parent's (names sort "AMS..." < "AP...")
        let ams = r#"	(symbol "AMS1117-3.3"
		(extends "AP1117-15")
		(property "Reference" "U" (at 0 0 0))
		(property "Value" "AMS1117-3.3" (at 0 0 0))
		(property "Description" "1A LDO, 3.3V"))"#;
        std::fs::write(dir.join("R.kicad_sym"), lib(r)).unwrap();
        std::fs::write(dir.join("AP1117-15.kicad_sym"), lib(reg)).unwrap();
        std::fs::write(dir.join("AMS1117-3.3.kicad_sym"), lib(ams)).unwrap();
        // the same name in two files: the one read last (`Z_dup` sorts after `C`) is the library's
        std::fs::write(dir.join("C.kicad_sym"), lib(r#"	(symbol "C" (property "Reference" "C") (property "Description" "first C") (symbol "C_1_1" (pin passive line (at 0 3.81 270) (length 1.27) (name "~") (number "1"))))"#)).unwrap();
        std::fs::write(dir.join("Z_dup.kicad_sym"), lib(r#"	(symbol "C" (property "Reference" "C") (property "Description" "second C") (symbol "C_1_1" (pin passive line (at 0 3.81 270) (length 1.27) (name "~") (number "1")) (pin passive line (at 0 -3.81 90) (length 1.27) (name "~") (number "2"))))"#)).unwrap();
        // what is not read: another file, a hidden file, a file that is no library, a folder
        std::fs::write(dir.join("README.md"), "(kicad_symbol_lib (symbol \"NotMe\"))").unwrap();
        std::fs::write(dir.join("._R.kicad_sym"), lib("\t(symbol \"Hidden\")")).unwrap();
        std::fs::write(dir.join("broken.kicad_sym"), "(kicad_sch (symbol \"Broken\"))").unwrap();
        std::fs::create_dir_all(dir.join("sub.kicad_sym")).unwrap();
        dir
    }

    #[test]
    fn a_folder_reads_as_the_one_library_its_files_make() {
        let root = temp_dir("text");
        let dir = fixture(&root);
        assert_eq!(symbol_library_files(&dir).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>(), ["AMS1117-3.3.kicad_sym", "AP1117-15.kicad_sym", "C.kicad_sym", "R.kicad_sym", "Z_dup.kicad_sym", "broken.kicad_sym"]);
        assert!(can_read_symbol_library(&dir));
        let text = read_symbol_library_text(&dir).unwrap();
        assert!(text.starts_with("(kicad_symbol_lib (version 20251024)\n"), "{text}");
        let forms = crate::symbol_scan::library_forms(&text).unwrap();
        let names: Vec<&str> = forms.symbols.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["AMS1117-3.3", "AP1117-15", "R", "C"], "every symbol once, the duplicate where its last file is, nothing of the files that are not read");
        assert!(text.contains("second C") && !text.contains("first C"));
        assert!(crate::sexpr::parse(&text).is_ok(), "the text is one s-expression");
        // the readers of a library text read it
        let scanned = crate::scan_symbols(&text);
        assert_eq!(scanned.len(), 4);
        let table = crate::symbol_lib::parse_symbol_library(&text).unwrap();
        assert_eq!(table["C"].pins.len(), 2, "the C of the file read last");
        let ams = &table["AMS1117-3.3"];
        assert_eq!(ams.pins.len(), 3, "a derived symbol has its parent's pins, though the parent is in another file");
        assert!(!ams.graphics.is_empty());
        assert_eq!(ams.description, "1A LDO, 3.3V");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The lookups by library name a folder of libraries answers (the schematic's symbols, the Symbol Chooser of `A`, the simulator's fields): a folder
    /// library is found by its name, listed once with a file of the same name (the file read), and read through the cache, again when a file changes.
    #[test]
    fn a_folder_library_is_found_by_name_and_resolves_its_symbols() {
        let root = temp_dir("resolve");
        let dir = fixture(&root);
        std::fs::write(root.join("Other.kicad_sym"), lib("\t(symbol \"X\")")).unwrap();
        assert_eq!(crate::list_symbol_libraries(&root), ["Mini", "Other"]);
        assert_eq!(crate::find_symbol_library_file(&root, "Mini"), Some(dir.clone()));
        let ams = crate::resolve_symbol(&root, "Mini:AMS1117-3.3").expect("the derived symbol resolves");
        assert_eq!((ams.lib_id.as_str(), ams.pins.len()), ("Mini:AMS1117-3.3", 3));
        assert!(ams.pins.iter().any(|p| p.electrical_type == "power_out"), "the parent's VO pin, from the parent's file");
        assert!(crate::resolve_symbol(&root, "Mini:Hidden").is_none() && crate::resolve_symbol(&root, "Mini:NotMe").is_none());
        let all: Vec<String> = crate::list_symbols_in_library(&root, "Mini").into_iter().map(|s| s.lib_id).collect();
        assert_eq!(all, ["Mini:AMS1117-3.3", "Mini:AP1117-15", "Mini:C", "Mini:R"]);
        assert_eq!(crate::sim_fields_of(&dir, "R"), [("Sim.Device".to_string(), "R".to_string())]);
        // a file of the folder changes: the next lookup reads it
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(dir.join("R.kicad_sym"), lib("\t(symbol \"R\" (property \"Sim.Device\" \"SPICE\") (symbol \"R_1_1\" (pin passive line (at 0 3.81 270) (length 1.27) (name \"~\") (number \"1\"))))")).unwrap();
        assert_eq!(crate::resolve_symbol(&root, "Mini:R").unwrap().pins.len(), 1);
        assert_eq!(crate::sim_fields_of(&dir, "R"), [("Sim.Device".to_string(), "SPICE".to_string())]);
        // a library both as a file and as a folder is listed once and read from the file
        let both = root.join("Other.kicad_symdir");
        std::fs::create_dir_all(&both).unwrap();
        std::fs::write(both.join("Y.kicad_sym"), lib("\t(symbol \"Y\")")).unwrap();
        assert_eq!(crate::list_symbol_libraries(&root), ["Mini", "Other"]);
        assert!(crate::resolve_symbol(&root, "Other:X").is_some() && crate::resolve_symbol(&root, "Other:Y").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_reads_as_itself_and_nothing_is_an_error() {
        let root = temp_dir("file");
        let file = root.join("One.kicad_sym");
        std::fs::write(&file, "(kicad_symbol_lib (symbol \"A\") (symbol \"A\"))").unwrap();
        assert_eq!(read_symbol_library_text(&file).unwrap(), "(kicad_symbol_lib (symbol \"A\") (symbol \"A\"))", "a file is read as it is");
        assert!(read_symbol_library_text(&root.join("Missing.kicad_sym")).is_err());
        assert!(!can_read_symbol_library(&root.join("Missing.kicad_sym")));
        let empty = root.join("Empty.kicad_symdir");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(!can_read_symbol_library(&empty), "a folder with no .kicad_sym file is no library CanReadLibrary knows");
        assert_eq!(read_symbol_library_text(&empty).unwrap(), "(kicad_symbol_lib\n)\n", "but it reads, as an empty library (Load finds no file and loads nothing)");
        assert!(crate::symbol_lib::parse_symbol_library(&read_symbol_library_text(&empty).unwrap()).unwrap().is_empty());
        assert_eq!(symbol_library_name(&file).as_deref(), Some("One"));
        assert_eq!(symbol_library_name(&empty).as_deref(), Some("Empty"));
        assert_eq!(symbol_library_name(&root.join("Missing.kicad_sym")), None);
        std::fs::create_dir_all(root.join("Plain")).unwrap();
        assert_eq!(symbol_library_name(&root.join("Plain")), None, "a folder a listing names a library has the .kicad_symdir extension");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_folder_changes_when_a_file_of_it_does() {
        let root = temp_dir("stamp");
        let dir = fixture(&root);
        let before = symbol_library_modified(&dir).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(dir.join("R.kicad_sym"), lib("\t(symbol \"R\")")).unwrap();
        let after = symbol_library_modified(&dir).unwrap();
        assert!(after > before, "an edited file is a changed library");
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::remove_file(dir.join("C.kicad_sym")).unwrap();
        assert!(symbol_library_modified(&dir).unwrap() > after, "a removed file is a changed library");
        assert_eq!(symbol_library_modified(&root.join("Nothing.kicad_symdir")), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// KiCad 25ec2335's `SCH_IO_KICAD_SEXPR_LIB_CACHE::Load`: a folder library with a file that does not read is "loaded with errors" -- the message names
    /// each such file and why, the good files are read all the same, and it is kept until a file changes.
    #[test]
    fn a_folder_library_with_a_broken_file_loads_with_errors_and_says_which() {
        let root = temp_dir("errors");
        let dir = fixture(&root);
        let broken = dir.join("broken.kicad_sym");
        let message = symbol_library_load_errors(&dir).expect("the fixture's broken.kicad_sym is no library");
        assert_eq!(
            message,
            format!("Unable to read file '{}'\nExpecting 'kicad_symbol_lib' in '{}', line 1, offset 2.\n\nThe library cannot be saved until these errors are fixed manually.", broken.display(), broken.display())
        );
        let (text, errors) = read_symbol_library(&dir).unwrap();
        assert_eq!(errors.as_deref(), Some(message.as_str()), "the read says the same");
        assert_eq!(crate::scan_symbols(&text).len(), 4, "and the good files are read");
        // a second file cut off in a symbol: both are named, in the order read; the symbols it closed before the cut are read
        std::thread::sleep(std::time::Duration::from_millis(30));
        let cut = dir.join("D_cut.kicad_sym");
        std::fs::write(&cut, "(kicad_symbol_lib (version 20251024)\n\t(symbol \"D\" (property \"Reference\" \"D\"))\n\t(symbol \"E\" (pin").unwrap();
        let both = symbol_library_load_errors(&dir).unwrap();
        let (first, second) = (both.find("D_cut.kicad_sym").unwrap(), both.find("broken.kicad_sym").unwrap());
        assert!(first < second, "{both}");
        assert!(both.contains(&format!("Unable to read file '{}'\nExpecting ')' in '{}', line 3, offset 18.\n\nUnable to read file '{}'", cut.display(), cut.display(), broken.display())), "{both}");
        assert!(read_symbol_library_text(&dir).unwrap().contains("(symbol \"D\""), "D closed before the cut");
        // the files fixed: no error
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::remove_file(&cut).unwrap();
        std::fs::remove_file(&broken).unwrap();
        assert_eq!(symbol_library_load_errors(&dir), None);
        // a file library: its own reason; a good one, none
        let file = root.join("Bad.kicad_sym");
        std::fs::write(&file, "(kicad_symbol_lib (symbol \"A\"").unwrap();
        assert_eq!(symbol_library_load_errors(&file), Some(format!("Expecting ')' in '{}', line 1, offset 30.", file.display())));
        std::fs::write(root.join("Good.kicad_sym"), lib("\t(symbol \"G\")")).unwrap();
        assert_eq!(symbol_library_load_errors(&root.join("Good.kicad_sym")), None);
        assert_eq!(symbol_library_load_errors(&root.join("Nothing.kicad_sym")), None);
        assert_eq!(library_text_problem("  (kicad_sch)", "f"), Some("Expecting 'kicad_symbol_lib' in 'f', line 1, offset 4.".to_string()));
        assert_eq!(library_text_problem("x", "f"), Some("Expecting '(' in 'f', line 1, offset 1.".to_string()));
        assert_eq!(library_text_problem("(kicad_symbol_lib (symbol \"a)\"))", "f"), None, "a parenthesis in a string is text");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn library_forms_finds_the_version_and_the_symbols() {
        let text = "(kicad_symbol_lib (version 20251024) (generator \"a (b\") (symbol \"A\" (property \"x\" \"a)b\") (symbol \"A_1_1\")) (symbol B) (symbol \"Cut\" (pin";
        let forms = crate::symbol_scan::library_forms(text).unwrap();
        assert_eq!(forms.version.as_deref(), Some("20251024"));
        assert_eq!(forms.symbols.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["A", "B"], "a unit block is not a symbol; a bare name is a name; a cut-off symbol is left out");
        assert_eq!(&text[forms.symbols[0].1.clone()], "(symbol \"A\" (property \"x\" \"a)b\") (symbol \"A_1_1\"))");
        assert!(crate::symbol_scan::library_forms("(kicad_sch (symbol \"A\"))").is_none());
        assert!(crate::symbol_scan::library_forms("  ").is_none());
    }
}
