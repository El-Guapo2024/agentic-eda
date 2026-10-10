//! The installed KiCad libraries, as the library trees of the Footprint and Symbol editors list them
//! (`/api/library/index`, `/api/library/items`, `/api/library/all`).
//!
//! KiCad's footprint libraries (`<Library>.pretty/<Footprint>.kicad_mod`, 155 of them in KiCad.app) and symbol libraries
//! (`<Library>.kicad_sym`, 223 files, 220 MB, the biggest 15 MB) are far too large to ship to the browser or to parse up
//! front, so the tree is filled in lazily and every answer is cached:
//!
//!   - **index**: the library names only -- a `read_dir` of the root (and, for footprints, a `read_dir` of each library for
//!     its count). The tree draws these collapsed.
//!   - **items**: the item names of ONE library, asked when the person expands it. A footprint library is a `read_dir`; a
//!     symbol library is a single pass over its text that picks out the top-level `(symbol "Name" ...)` forms
//!     ([`scan_symbol_names`]) -- no s-expression tree is built just to list names. The result is cached against the
//!     library's modification time, so a library that changes on disk is listed again and one that does not is read once.
//!   - **all**: every library's names in one answer, for the tree's search box (KiCad's tree searches every library).
//!     It is built on a thread of its own (the symbol pass reads all 220 MB) and the route answers `pending` until it
//!     is done, like `/api/board.glb`: the request loop never waits for it.
//!
//! Where the libraries come from is a [`LibSource`]: KiCad's library tables as the studio reads them (`library_tables::LibSet` -- the global table, the
//! libraries it chains, the project's own table), or a plain folder of library files (the tests', and what KiCad.app's install is when no table names
//! it: `eda_kicad::default_footprint_library_root` / `default_symbol_library_root`, overridable with `EDA_KICAD_FOOTPRINTS` / `EDA_KICAD_SYMBOLS`).
//!
//! Opening an item is not here: `library_api` resolves a `Lib:Name` against the same libraries.

use eda_kicad::{default_footprint_library_root, default_symbol_library_root};
use eda_kicad::library_table::TableKind;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::SystemTime;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    Footprint,
    Symbol,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "footprint" => Some(Kind::Footprint),
            "symbol" => Some(Kind::Symbol),
            _ => None,
        }
    }

    pub(crate) fn root(self) -> PathBuf {
        match self {
            Kind::Footprint => default_footprint_library_root(),
            Kind::Symbol => default_symbol_library_root(),
        }
    }

    /// The library table of this kind (`sym-lib-table`, `fp-lib-table`).
    pub(crate) fn table_kind(self) -> TableKind {
        match self {
            Kind::Footprint => TableKind::Footprint,
            Kind::Symbol => TableKind::Symbol,
        }
    }

    /// The path of library `lib` under `root`: a `.pretty` directory or a `.kicad_sym` file.
    pub(crate) fn library_path(self, root: &Path, lib: &str) -> PathBuf {
        match self {
            Kind::Footprint => root.join(format!("{lib}.pretty")),
            Kind::Symbol => root.join(format!("{lib}.kicad_sym")),
        }
    }
}

/// A library nickname is a file name: nothing in it may lead out of the library root.
pub(crate) fn legal_library_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\']) && !name.contains("..")
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LibraryInfo {
    pub(crate) name: String,
    /// How many items it holds, when that is cheap to know (a footprint library: its directory listing; a symbol library: unknown until it is opened).
    pub(crate) count: Option<usize>,
}

pub(crate) fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Where the libraries of one kind come from. A library is a nickname that names a `.kicad_sym` file (symbols) or a `.pretty` folder (footprints), and what
/// the trees, the choosers and the lookups need of a source is only that: the nickname's path, the list of nicknames to show, and a few words about each.
pub(crate) trait LibSource: Send + Sync {
    /// The file (symbols) or folder (footprints) of library `lib`, or why this source has none by that name.
    fn lib_path(&self, kind: Kind, lib: &str) -> Result<PathBuf, String>;
    /// The libraries the trees and the choosers list, sorted by name (case-insensitively, like the tree).
    fn listed(&self, kind: Kind) -> Arc<Vec<LibraryInfo>>;
    /// The description of a library (the Description column of a table row).
    fn describe(&self, kind: Kind, lib: &str) -> Option<String>;
    /// Which table the library is in: `global` (KiCad's own) or `project`; none for a plain folder.
    fn scope_of(&self, _kind: Kind, _lib: &str) -> Option<&'static str> {
        None
    }
    /// What names this source in the caches of the background builds: another set of libraries is another build.
    fn fingerprint(&self, kind: Kind) -> String;
    /// An owned copy a thread may keep.
    fn boxed(&self) -> Arc<dyn LibSource>;
}

impl LibSource for PathBuf {
    fn lib_path(&self, kind: Kind, lib: &str) -> Result<PathBuf, String> {
        if !legal_library_name(lib) {
            return Err(format!("{lib:?} is not a library name"));
        }
        Ok(kind.library_path(self, lib))
    }

    fn listed(&self, kind: Kind) -> Arc<Vec<LibraryInfo>> {
        scan_root(kind, self)
    }

    fn describe(&self, kind: Kind, lib: &str) -> Option<String> {
        crate::library_search::library_descriptions(kind, self).get(lib).cloned()
    }

    fn fingerprint(&self, kind: Kind) -> String {
        format!("root:{kind:?}:{}", self.display())
    }

    fn boxed(&self) -> Arc<dyn LibSource> {
        Arc::new(self.clone())
    }
}

impl LibSource for Path {
    fn lib_path(&self, kind: Kind, lib: &str) -> Result<PathBuf, String> {
        self.to_path_buf().lib_path(kind, lib)
    }

    fn listed(&self, kind: Kind) -> Arc<Vec<LibraryInfo>> {
        scan_root(kind, self)
    }

    fn describe(&self, kind: Kind, lib: &str) -> Option<String> {
        crate::library_search::library_descriptions(kind, self).get(lib).cloned()
    }

    fn fingerprint(&self, kind: Kind) -> String {
        format!("root:{kind:?}:{}", self.display())
    }

    fn boxed(&self) -> Arc<dyn LibSource> {
        Arc::new(self.to_path_buf())
    }
}

/// Everything already read, keyed by what it was read from and stamped with the modification time it was read at.
#[derive(Default)]
struct Cache {
    libraries: HashMap<(Kind, PathBuf), (Option<SystemTime>, Arc<Vec<LibraryInfo>>)>,
    items: HashMap<(Kind, PathBuf), (Option<SystemTime>, Arc<Vec<String>>)>,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The libraries of a source, sorted by name (case-insensitively, like the tree).
pub(crate) fn libraries_in<S: LibSource + ?Sized>(kind: Kind, src: &S) -> Arc<Vec<LibraryInfo>> {
    src.listed(kind)
}

/// The libraries under `root`, sorted by name (case-insensitively, like the tree).
fn scan_root(kind: Kind, root: &Path) -> Arc<Vec<LibraryInfo>> {
    let stamp = modified(root);
    let key = (kind, root.to_path_buf());
    if let Some((s, hit)) = lock(cache()).libraries.get(&key) {
        if *s == stamp {
            return hit.clone();
        }
    }
    let mut libs: Vec<LibraryInfo> = std::fs::read_dir(root)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let path = e.path();
                    let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("");
                    let name = path.file_stem().and_then(|s| s.to_str())?.to_string();
                    match kind {
                        Kind::Footprint if ext == "pretty" && path.is_dir() => Some(LibraryInfo { count: Some(count_footprints(&path)), name }),
                        Kind::Symbol if ext == "kicad_sym" && path.is_file() => Some(LibraryInfo { name, count: None }),
                        _ => None,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    libs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.name.cmp(&b.name)));
    let libs = Arc::new(libs);
    lock(cache()).libraries.insert(key, (stamp, libs.clone()));
    libs
}

fn footprint_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let path = e.path();
                    (path.extension().and_then(|x| x.to_str()) == Some("kicad_mod")).then(|| path.file_stem().and_then(|s| s.to_str()).map(String::from)).flatten()
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b)));
    names
}

pub(crate) fn count_footprints(dir: &Path) -> usize {
    std::fs::read_dir(dir).map(|entries| entries.filter_map(|e| e.ok()).filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("kicad_mod")).count()).unwrap_or(0)
}

/// The names of the top-level `(symbol "Name" ...)` forms of a `.kicad_sym` file's text, in file order. A single pass over the bytes, keeping only the
/// paren depth: a symbol's own units and body styles are `(symbol "Name_1_1" ...)` forms NESTED inside it (depth 3), and a quoted string -- a pin name,
/// a property value, a description -- may hold parentheses, so strings are skipped whole. Building the s-expression tree of a 15 MB library just to
/// list its names would cost far more than this and keep the whole tree alive.
pub(crate) fn scan_symbol_names(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let mut names = Vec::new();
    let (mut i, mut depth) = (0usize, 0usize);
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'(' => {
                // The library is `(kicad_symbol_lib ...)`: its direct children are at depth 1 before their own `(`.
                if depth == 1 && b[i + 1..].starts_with(b"symbol") && b.get(i + 7).is_some_and(|c| c.is_ascii_whitespace()) {
                    let mut j = i + 7;
                    while j < b.len() && b[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if b.get(j) == Some(&b'"') {
                        let start = j + 1;
                        let mut k = start;
                        while k < b.len() && b[k] != b'"' {
                            if b[k] == b'\\' {
                                k += 1;
                            }
                            k += 1;
                        }
                        names.push(String::from_utf8_lossy(&b[start..k.min(b.len())]).into_owned());
                    }
                }
                depth += 1;
                i += 1;
            }
            b')' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            _ => i += 1,
        }
    }
    names
}

/// The item names of library `lib` of a source (sorted), cached against the library's modification time.
fn items_in<S: LibSource + ?Sized>(kind: Kind, src: &S, lib: &str) -> Result<Arc<Vec<String>>, String> {
    let path = src.lib_path(kind, lib)?;
    let stamp = modified(&path);
    if stamp.is_none() {
        return Err(format!("no library {lib:?} is installed"));
    }
    let key = (kind, path.clone());
    if let Some((s, hit)) = lock(cache()).items.get(&key) {
        if *s == stamp {
            return Ok(hit.clone());
        }
    }
    let mut names = match kind {
        Kind::Footprint => footprint_names(&path),
        Kind::Symbol => {
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let mut names = scan_symbol_names(&text);
            names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b)));
            names.dedup();
            names
        }
    };
    names.shrink_to_fit();
    let names = Arc::new(names);
    lock(cache()).items.insert(key, (stamp, names.clone()));
    Ok(names)
}

/// `GET /api/library/index?kind=footprint|symbol`: `{"available": bool, "libraries": [{"name", "count"?, "description"?, "scope"?}]}` -- the libraries of
/// the board's library tables (KiCad's own through the global table, the project's through its own), names only, with the description each row gives
/// (the choosers' Description column) and the table it is in.
pub fn libraries(dir: &Path, kind: Kind) -> Value {
    libraries_json(kind, &*crate::library_tables::libs(dir, kind))
}

fn libraries_json<S: LibSource + ?Sized>(kind: Kind, src: &S) -> Value {
    let libs = libraries_in(kind, src);
    json!({
        "available": !libs.is_empty(),
        "libraries": libs.iter().map(|l| {
            let mut v = match l.count {
                Some(count) => json!({ "name": l.name, "count": count }),
                None => json!({ "name": l.name }),
            };
            if let Some(d) = src.describe(kind, &l.name) {
                v["description"] = json!(d);
            }
            if let Some(scope) = src.scope_of(kind, &l.name) {
                v["scope"] = json!(scope);
            }
            v
        }).collect::<Vec<_>>(),
    })
}

/// `GET /api/library/items?kind=...&lib=Name`: `{"lib", "names": [..]}` -- the bare item names of one library, or `{"error"}`.
pub fn items(dir: &Path, kind: Kind, lib: &str) -> Value {
    match items_in(kind, &*crate::library_tables::libs(dir, kind), lib) {
        Ok(names) => json!({ "lib": lib, "names": names.as_slice() }),
        Err(e) => json!({ "error": e }),
    }
}

/// What the background build of the all-libraries answer is doing, per kind and set of libraries.
enum AllBuild {
    Running,
    Done(Arc<BTreeMap<String, Arc<Vec<String>>>>),
}

fn all_builds() -> &'static Mutex<HashMap<(Kind, String), AllBuild>> {
    static BUILDS: OnceLock<Mutex<HashMap<(Kind, String), AllBuild>>> = OnceLock::new();
    BUILDS.get_or_init(Default::default)
}

fn build_all<S: LibSource + ?Sized>(kind: Kind, src: &S) -> BTreeMap<String, Arc<Vec<String>>> {
    let mut all = BTreeMap::new();
    for lib in libraries_in(kind, src).iter() {
        if let Ok(names) = items_in(kind, src, &lib.name) {
            all.insert(lib.name.clone(), names);
        }
    }
    all
}

/// `GET /api/library/all?kind=...`: every library's item names, for the tree's search box. `{"status": "pending"}` while the thread that reads them is
/// running (the first call starts it), then `{"status": "ready", "libraries": {"Lib": [names]}}`. A different set of libraries (a row added to a table,
/// a library file changed) is a different build.
pub fn all(dir: &Path, kind: Kind) -> Value {
    all_of(kind, &*crate::library_tables::libs(dir, kind))
}

fn all_of(kind: Kind, src: &dyn LibSource) -> Value {
    let key = (kind, src.fingerprint(kind));
    let mut builds = lock(all_builds());
    match builds.get(&key) {
        Some(AllBuild::Done(all)) => {
            let all = all.clone();
            drop(builds);
            json!({ "status": "ready", "libraries": all.iter().map(|(lib, names)| (lib.clone(), json!(names.as_slice()))).collect::<serde_json::Map<_, _>>() })
        }
        Some(AllBuild::Running) => json!({ "status": "pending" }),
        None => {
            builds.insert(key.clone(), AllBuild::Running);
            drop(builds);
            let owned = src.boxed();
            let spawned = std::thread::Builder::new().name("library-index".into()).spawn({
                let key = key.clone();
                move || {
                    let all = Arc::new(build_all(kind, &*owned));
                    lock(all_builds()).insert(key, AllBuild::Done(all));
                }
            });
            if spawned.is_err() {
                lock(all_builds()).remove(&key);
                return json!({ "error": "could not start the library index thread" });
            }
            json!({ "status": "pending" })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eda-library-index-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scan_symbol_names_picks_top_level_symbols_only() {
        let text = r#"(kicad_symbol_lib (version 20231120) (generator "kicad_symbol_editor")
            (symbol "R" (pin_numbers hide) (property "Reference" "R" (at 0 0 0))
                (symbol "R_0_1" (rectangle (start -1 -2) (end 1 2)))
                (symbol "R_1_1" (pin passive line (at 0 3.81 270) (length 1.27) (name "~") (number "1"))))
            (symbol "C (polarised)" (extends "R") (property "Value" "C)" (at 0 0 0)))
            (symbol "Odd\"Name" (symbol "Odd\"Name_1_1")))"#;
        assert_eq!(scan_symbol_names(text), vec!["R", "C (polarised)", "Odd\\\"Name"]);
    }

    #[test]
    fn scan_symbol_names_survives_junk() {
        assert!(scan_symbol_names("").is_empty());
        assert!(scan_symbol_names("(kicad_symbol_lib").is_empty());
        assert!(scan_symbol_names("(kicad_symbol_lib (symbol").is_empty(), "a cut-off file must not read past its end");
        assert!(scan_symbol_names("(kicad_symbol_lib (symbol \"unterminated").len() <= 1);
        assert_eq!(scan_symbol_names("(kicad_symbol_lib (symbol \"A\") ))) (symbol \"B\")"), vec!["A"], "stray closing parens do not underflow into depth 1 of nothing");
    }

    #[test]
    fn library_names_that_could_leave_the_root_are_refused() {
        for bad in ["", ".", "..", "../x", "a/b", "a\\b", ".hidden", "x..y"] {
            assert!(!legal_library_name(bad), "{bad:?}");
        }
        assert!(legal_library_name("Package_SO"));
        assert!(legal_library_name("Connector_PinHeader_2.54mm"));
    }

    #[test]
    fn footprint_libraries_are_listed_with_counts_and_items_by_name() {
        let root = temp_root("fp");
        for (lib, items) in [("Zeta", vec!["Z1"]), ("alpha", vec!["B", "a", "C"])] {
            let dir = root.join(format!("{lib}.pretty"));
            std::fs::create_dir_all(&dir).unwrap();
            for item in items {
                std::fs::write(dir.join(format!("{item}.kicad_mod")), "(footprint)").unwrap();
            }
            std::fs::write(dir.join("notes.txt"), "not a footprint").unwrap();
        }
        std::fs::create_dir_all(root.join("NotALibrary")).unwrap();
        std::fs::write(root.join("stray.kicad_sym"), "").unwrap();

        let libs = libraries_in(Kind::Footprint, &root);
        assert_eq!(libs.as_slice(), [LibraryInfo { name: "alpha".into(), count: Some(3) }, LibraryInfo { name: "Zeta".into(), count: Some(1) }], "sorted case-insensitively; only .pretty directories");
        assert_eq!(items_in(Kind::Footprint, &root, "alpha").unwrap().as_slice(), ["a", "B", "C"]);
        assert!(items_in(Kind::Footprint, &root, "Missing").is_err());
        assert!(items_in(Kind::Footprint, &root, "../alpha").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn symbol_libraries_are_listed_by_file_and_read_once_until_they_change() {
        let root = temp_root("sym");
        let lib = root.join("Device.kicad_sym");
        std::fs::write(&lib, r#"(kicad_symbol_lib (symbol "R" (symbol "R_1_1")) (symbol "C"))"#).unwrap();
        std::fs::write(root.join("README.md"), "x").unwrap();

        let libs = libraries_in(Kind::Symbol, &root);
        assert_eq!(libs.as_slice(), [LibraryInfo { name: "Device".into(), count: None }]);
        let first = items_in(Kind::Symbol, &root, "Device").unwrap();
        assert_eq!(first.as_slice(), ["C", "R"]);
        let again = items_in(Kind::Symbol, &root, "Device").unwrap();
        assert!(Arc::ptr_eq(&first, &again), "the second ask is served from the cache");

        // A changed library is listed again (its modification time moved).
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(&lib, r#"(kicad_symbol_lib (symbol "R") (symbol "C") (symbol "L"))"#).unwrap();
        let fresh = items_in(Kind::Symbol, &root, "Device").unwrap();
        assert_eq!(fresh.as_slice(), ["C", "L", "R"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_root_is_an_empty_list_not_an_error() {
        let nowhere = std::env::temp_dir().join("eda-library-index-does-not-exist");
        assert!(libraries_in(Kind::Footprint, &nowhere).is_empty());
        assert!(libraries_in(Kind::Symbol, &nowhere).is_empty());
    }

    #[test]
    fn the_whole_index_is_every_library() {
        let root = temp_root("all");
        std::fs::create_dir_all(root.join("L1.pretty")).unwrap();
        std::fs::write(root.join("L1.pretty").join("A.kicad_mod"), "").unwrap();
        std::fs::create_dir_all(root.join("L2.pretty")).unwrap();
        let all = build_all(Kind::Footprint, &root);
        assert_eq!(all.len(), 2);
        assert_eq!(all["L1"].as_slice(), ["A"]);
        assert!(all["L2"].is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The real libraries, when KiCad.app is installed: the listing is what the brief says is there, and a symbol library of any size lists quickly.
    #[test]
    fn the_installed_kicad_libraries_are_listed() {
        let fp_root = Kind::Footprint.root();
        let sym_root = Kind::Symbol.root();
        if !fp_root.is_dir() || !sym_root.is_dir() {
            return; // no KiCad install on this machine
        }
        let fps = libraries_in(Kind::Footprint, &fp_root);
        assert!(fps.iter().any(|l| l.name == "Package_SO" && l.count.unwrap_or(0) > 50), "Package_SO is a footprint library of KiCad's");
        let so = items_in(Kind::Footprint, &fp_root, "Package_SO").unwrap();
        assert!(so.iter().any(|n| n == "SOIC-16_3.9x9.9mm_P1.27mm"));
        let syms = libraries_in(Kind::Symbol, &sym_root);
        assert!(syms.iter().any(|l| l.name == "Device"));
        let device = items_in(Kind::Symbol, &sym_root, "Device").unwrap();
        for name in ["R", "C", "LED"] {
            assert!(device.iter().any(|n| n == name), "Device:{name}");
        }
        assert!(!device.iter().any(|n| n.starts_with("R_") && n.ends_with("_1")), "units are not symbols of their own");
    }
}
