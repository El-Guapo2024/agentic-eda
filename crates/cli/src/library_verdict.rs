//! What KiCad's own parser refuses in a library of the tables (UI-LEFT-3 L3-6). KiCad reads a footprint library with `FP_CACHE::Load` (pcbnew/pcb_io/kicad_sexpr/
//! pcb_io_kicad_sexpr.cpp: "Queue I/O errors so only files that fail to parse don't get loaded": a file `PCB_IO_KICAD_SEXPR_PARSER` refuses is not in the
//! library, which reports "Unable to read file '%s'" and the reason) and a symbol library with `SCH_IO_KICAD_SEXPR_LIB_CACHE::Load` (a file library that does
//! not parse does not load; a `.kicad_symdir` folder loses the files that do not, KiCad 25ec2335). The studio's readers refuse what they cannot read
//! (`eda_kicad::library_save::footprint_library_refusals`, `eda_kicad::symbol_library_load_errors`); KiCad's parser is stricter (an unknown token, a value out
//! of its grammar), and its verdict is kicad-cli's here, the one KiCad engine the studio runs: `kicad-cli <sym|fp> upgrade --force` loads a library whole
//! through that very plugin and answers "Unable to load library" when a file does not parse ([`eda_kicad_engine::foreign::library_loads`]).
//!
//! kicad-cli tells only whether the whole library loads, so the files it refuses are found by halving: a library that does not load is split in two and each
//! half loaded alone (a folder of links to its files), down to the single files (a symbol file loaded with the files holding its symbols' parents, which a
//! derived symbol needs). A library that loads costs one run (about half a second for KiCad's largest); the verdict is kept against the library's state, in
//! memory and in the studio's library cache (`crate::library_convert::cache_dir`, `verdicts/`), so each state of a library is loaded once.
//!
//! Where it is read: the trees (`GET /api/library/items`, `library_index`, asked now for the library opened) and the choosers' items (`library_search::entries`,
//! the same); the choosers' search and footprint selector and the trees' search over every library leave out what is known when they answer, and the
//! choosers' index thread asks kicad-cli about each library in turn once it has read them all (as KiCad's lazy load reads every library; the index itself
//! never waits for kicad-cli). **Differs**: kicad-cli does not print the parser's reason, so a file refused by KiCad's parser alone says "Unable to read file '%s'" and
//! [`KICAD_REFUSES`] in place of the lexer's words.

use crate::library_index::Kind;
use eda_kicad_engine::foreign::LibraryKind;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::SystemTime;

/// The reason given for a file only KiCad's parser refuses (kicad-cli prints none).
pub(crate) const KICAD_REFUSES: &str = "KiCad's parser does not read it (kicad-cli: \"Unable to load library\").";

/// Whether KiCad's parser loads the library whose items are `files` (a folder library made of them, or the one file): `None` when it cannot be asked.
pub(crate) type Loads<'a> = &'a dyn Fn(Kind, &[PathBuf]) -> Option<bool>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The files of a library: a `.pretty` folder's `*.kicad_mod`, a `.kicad_symdir` folder's `*.kicad_sym`, or the one `.kicad_sym` file; hidden files left out
/// (KiCad's `wxDir` listings skip them), sorted.
fn library_files(kind: Kind, path: &Path) -> Vec<PathBuf> {
    if !path.is_dir() {
        return if path.is_file() { vec![path.to_path_buf()] } else { Vec::new() };
    }
    let ext = match kind {
        Kind::Footprint => "kicad_mod",
        Kind::Symbol => "kicad_sym",
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(path)
        .map(|r| r.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().and_then(|x| x.to_str()) == Some(ext) && p.is_file() && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))).collect())
        .unwrap_or_default();
    files.sort();
    files
}

/// The state a verdict is kept against: each file's size and time, and the folder's.
fn stamp(kind: Kind, path: &Path) -> Option<String> {
    let time = |p: &Path| std::fs::metadata(p).ok().map(|m| (m.len(), m.modified().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos())));
    let (len, at) = time(path)?;
    let mut text = format!("{kind:?}|{}|{len}@{at}", path.display());
    for f in library_files(kind, path) {
        if let Some((l, t)) = time(&f) {
            text.push_str(&format!("|{}={l}@{t}", f.file_name().unwrap_or_default().to_string_lossy()));
        }
    }
    Some(text)
}

/// The files of `files` whose symbols derive from a symbol of another file of the library: the parents' files, which a derived symbol needs to load.
fn with_parents(kind: Kind, all: &[PathBuf], files: &[PathBuf]) -> Vec<PathBuf> {
    if kind != Kind::Symbol || all.len() < 2 {
        return files.to_vec();
    }
    let summaries: Vec<(PathBuf, Vec<eda_kicad::SymbolSummary>)> = all.iter().map(|f| (f.clone(), std::fs::read_to_string(f).map(|t| eda_kicad::scan_symbols(&t)).unwrap_or_default())).collect();
    let file_of: HashMap<&str, &PathBuf> = summaries.iter().flat_map(|(f, ss)| ss.iter().map(move |s| (s.name.as_str(), f))).collect();
    let mut out: BTreeSet<PathBuf> = files.iter().cloned().collect();
    loop {
        let more: Vec<PathBuf> = summaries
            .iter()
            .filter(|(f, _)| out.contains(f))
            .flat_map(|(_, ss)| ss.iter().filter_map(|s| s.extends.as_deref()).filter_map(|p| file_of.get(p).map(|f| (*f).clone())))
            .filter(|f| !out.contains(f))
            .collect();
        if more.is_empty() {
            return out.into_iter().collect();
        }
        out.extend(more);
    }
}

/// The files of the library at `path` that `loads` refuses, by halving (see the module doc).
pub(crate) fn refused_files_with(kind: Kind, path: &Path, loads: Loads<'_>) -> Option<Vec<PathBuf>> {
    let all = library_files(kind, path);
    if all.is_empty() {
        return Some(Vec::new());
    }
    let whole: Vec<PathBuf> = if path.is_dir() { all.clone() } else { vec![path.to_path_buf()] };
    if loads(kind, &whole)? {
        return Some(Vec::new());
    }
    if !path.is_dir() {
        return Some(whole);
    }
    fn halve(kind: Kind, all: &[PathBuf], part: &[PathBuf], loads: Loads<'_>, out: &mut Vec<PathBuf>) -> Option<()> {
        if part.len() == 1 {
            out.push(part[0].clone());
            return Some(());
        }
        let (a, b) = part.split_at(part.len() / 2);
        for half in [a, b] {
            if !loads(kind, &with_parents(kind, all, half))? {
                halve(kind, all, half, loads, out)?;
            }
        }
        Some(())
    }
    let mut out = Vec::new();
    halve(kind, &all, &all, loads, &mut out)?;
    Some(out)
}

/// kicad-cli's answer for `files` of a library of `kind` (a folder of links to them, or the one file).
fn kicad_loads(kind: Kind, files: &[PathBuf]) -> Option<bool> {
    let lk = match kind {
        Kind::Symbol => LibraryKind::Symbol,
        Kind::Footprint => LibraryKind::Footprint,
    };
    let scratch = crate::library_convert::cache_dir().join("verdicts").join(format!("run-{}-{}", std::process::id(), RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let single_file = files.len() == 1 && kind == Kind::Symbol && files[0].extension().and_then(|x| x.to_str()) == Some("kicad_sym") && !files[0].parent().is_some_and(|p| p.extension().and_then(|x| x.to_str()) == Some("kicad_symdir"));
    if single_file {
        return eda_kicad_engine::foreign::library_loads(lk, &files[0], &scratch);
    }
    let folder = scratch.with_extension(match kind {
        Kind::Symbol => "kicad_symdir",
        Kind::Footprint => "pretty",
    });
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).ok()?;
    for f in files {
        let link = folder.join(f.file_name()?);
        #[cfg(unix)]
        std::os::unix::fs::symlink(f, &link).ok()?;
        #[cfg(not(unix))]
        std::fs::copy(f, &link).ok()?;
    }
    let answer = eda_kicad_engine::foreign::library_loads(lk, &folder, &scratch);
    let _ = std::fs::remove_dir_all(&folder);
    answer
}

static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

type Verdicts = Mutex<HashMap<(Kind, PathBuf), (String, Vec<PathBuf>)>>;

fn verdicts() -> &'static Verdicts {
    static V: OnceLock<Verdicts> = OnceLock::new();
    V.get_or_init(Default::default)
}

/// One kicad-cli check at a time (a library opened twice at once is checked once).
fn running() -> &'static Mutex<()> {
    static R: OnceLock<Mutex<()>> = OnceLock::new();
    R.get_or_init(Default::default)
}

fn disk_file(kind: Kind, path: &Path) -> PathBuf {
    let key = blake3::hash(format!("{kind:?}|{}", path.display()).as_bytes()).to_hex()[..24].to_string();
    crate::library_convert::cache_dir().join("verdicts").join(format!("{key}.json"))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct OnDisk {
    stamp: String,
    refused: Vec<PathBuf>,
}

/// The verdict kept for the library's present state, in memory or on disk.
fn known(kind: Kind, path: &Path, stamp: &str) -> Option<Vec<PathBuf>> {
    if let Some((s, files)) = lock(verdicts()).get(&(kind, path.to_path_buf())) {
        if s == stamp {
            return Some(files.clone());
        }
    }
    let disk: OnDisk = serde_json::from_str(&std::fs::read_to_string(disk_file(kind, path)).ok()?).ok()?;
    (disk.stamp == stamp).then(|| {
        lock(verdicts()).insert((kind, path.to_path_buf()), (disk.stamp, disk.refused.clone()));
        disk.refused
    })
}

fn keep(kind: Kind, path: &Path, stamp: &str, refused: &[PathBuf]) {
    lock(verdicts()).insert((kind, path.to_path_buf()), (stamp.to_string(), refused.to_vec()));
    let file = disk_file(kind, path);
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&file, serde_json::to_string(&OnDisk { stamp: stamp.to_string(), refused: refused.to_vec() }).unwrap_or_default());
}

/// Whether the verdict is asked of kicad-cli for this library: always, when there is a kicad-cli (`EDA_LIBRARY_VERDICT=off` turns it off); a test turns it on
/// for its own libraries only ([`tests::check_with_kicad_cli`]), so the other tests' scratch libraries do not each start kicad-cli.
fn enabled(path: &Path) -> bool {
    if std::env::var("EDA_LIBRARY_VERDICT").is_ok_and(|v| v == "off") {
        return false;
    }
    #[cfg(test)]
    {
        return tests::checked(path);
    }
    #[allow(unreachable_code)]
    {
        let _ = path;
        cli_found()
    }
}

/// Whether there is a kicad-cli to ask, looked up once (`find_cli` runs `which`, and a search asks about every library).
fn cli_found() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| eda_kicad_engine::find_cli().is_some())
}

/// What a look that asks nothing answered lately, by library: a search asks about every library on every key, and the state of a library is a look at
/// each of its files.
type Recent = Mutex<HashMap<(Kind, PathBuf), (std::time::Instant, Vec<PathBuf>)>>;

fn recent() -> &'static Recent {
    static R: OnceLock<Recent> = OnceLock::new();
    R.get_or_init(Default::default)
}

/// How long a look that asks nothing is taken again without looking at the library's files.
const RECENT: std::time::Duration = std::time::Duration::from_secs(2);

/// The files of the library at `path` KiCad's parser refuses: the verdict kept for its state, else kicad-cli asked now when `ask` (else none known yet).
pub(crate) fn kicad_refused(kind: Kind, path: &Path, ask: bool) -> Vec<PathBuf> {
    if !enabled(path) {
        return Vec::new();
    }
    let key = (kind, path.to_path_buf());
    if !ask {
        if let Some((at, hit)) = lock(recent()).get(&key) {
            if at.elapsed() < RECENT {
                return hit.clone();
            }
        }
    }
    let Some(stamp) = stamp(kind, path) else { return Vec::new() };
    if let Some(hit) = known(kind, path, &stamp) {
        lock(recent()).insert(key, (std::time::Instant::now(), hit.clone()));
        return hit;
    }
    if !ask {
        lock(recent()).insert(key, (std::time::Instant::now(), Vec::new()));
        return Vec::new();
    }
    let _one = lock(running());
    if let Some(hit) = known(kind, path, &stamp) {
        return hit;
    }
    match refused_files_with(kind, path, &kicad_loads) {
        Some(refused) => {
            keep(kind, path, &stamp, &refused);
            lock(recent()).insert(key, (std::time::Instant::now(), refused.clone()));
            refused
        }
        None => Vec::new(),
    }
}

/// `"Unable to read file '%s'\n%s"`, the entry `FP_CACHE::Load` and the folder `Load` queue for a file.
fn unable(file: &Path, why: &str) -> String {
    format!("Unable to read file '{}'\n{why}", file.display())
}

/// The footprints of the `.pretty` folder `dir` that are not in the library, and the library's message (`FP_CACHE::Load`'s queued errors): the files this
/// crate's reader refuses (with its reason) and those only KiCad's parser refuses.
pub(crate) fn footprint_refusals(dir: &Path, ask: bool) -> (Vec<String>, Vec<String>) {
    let (mut names, mut messages) = eda_kicad::library_save::footprint_library_refusals(dir);
    for f in kicad_refused(Kind::Footprint, dir, ask) {
        let name = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if !names.contains(&name) {
            names.push(name);
            messages.push(unable(&f, KICAD_REFUSES));
        }
    }
    (names, messages)
}

/// The symbols of the symbol library at `path` that are not in it, and its load error (`GetLibraryLoadErrors`): this crate's reader's
/// (`eda_kicad::symbol_library_load_errors`, whose refused files are not read at all) and the files only KiCad's parser refuses -- a file library's every
/// symbol (it does not load), a folder library's files' symbols ("Unable to read file '%s'" for each, then "The library cannot be saved until these errors
/// are fixed manually.").
pub(crate) fn symbol_refusals(path: &Path, ask: bool) -> (Vec<String>, Option<String>) {
    let ours = eda_kicad::symbol_library_load_errors(path);
    // a file this crate's reader refuses already says so, in its own words
    let refused: Vec<PathBuf> = kicad_refused(Kind::Symbol, path, ask)
        .into_iter()
        .filter(|f| !ours.as_deref().is_some_and(|m| m.contains(&format!("Unable to read file '{}'", f.display()))))
        .collect();
    if refused.is_empty() {
        return (Vec::new(), ours);
    }
    let names: Vec<String> = refused.iter().flat_map(|f| std::fs::read_to_string(f).map(|t| eda_kicad::scan_symbols(&t).into_iter().map(|s| s.name).collect::<Vec<_>>()).unwrap_or_default()).collect();
    const CANNOT_SAVE: &str = "\n\nThe library cannot be saved until these errors are fixed manually.";
    let entries: Vec<String> = refused.iter().map(|f| unable(f, KICAD_REFUSES)).collect();
    let message = if !path.is_dir() {
        entries.join("\n\n")
    } else {
        let before = ours.as_deref().map(|m| m.strip_suffix(CANNOT_SAVE).unwrap_or(m).to_string());
        let mut all: Vec<String> = before.into_iter().collect();
        all.extend(entries);
        format!("{}{CANNOT_SAVE}", all.join("\n\n"))
    };
    (names, Some(message))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn allowed() -> &'static Mutex<Vec<PathBuf>> {
        static A: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();
        A.get_or_init(Default::default)
    }

    /// Ask kicad-cli about the libraries under `root` (a test's own folder).
    pub(crate) fn check_with_kicad_cli(root: &Path) {
        lock(allowed()).push(root.to_path_buf());
    }

    pub(super) fn checked(path: &Path) -> bool {
        lock(allowed()).iter().any(|r| path.starts_with(r)) && super::cli_found()
    }

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eda-verdict-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A stand-in for kicad-cli: a library loads unless one of its files says `frobnicate`, or holds a derived symbol whose parent is not among them.
    fn fake(runs: &std::cell::Cell<u32>) -> impl Fn(Kind, &[PathBuf]) -> Option<bool> + '_ {
        move |_, files| {
            runs.set(runs.get() + 1);
            let texts: Vec<String> = files.iter().map(|f| std::fs::read_to_string(f).unwrap()).collect();
            let defined: Vec<String> = texts.iter().flat_map(|t| eda_kicad::scan_symbols(t).into_iter().map(|s| s.name)).collect();
            let orphan = texts.iter().flat_map(|t| eda_kicad::scan_symbols(t)).any(|s| s.extends.is_some_and(|p| !defined.contains(&p)));
            Some(!orphan && !texts.iter().any(|t| t.contains("frobnicate")))
        }
    }

    const GOOD: &str = "(footprint \"G\" (version 20240108) (generator \"eda\") (layer \"F.Cu\")\n\t(pad \"1\" thru_hole circle (at 0 0) (size 1.5 1.5) (drill 0.8) (layers \"*.Cu\" \"*.Mask\"))\n)\n";

    #[test]
    fn halving_finds_each_refused_file_and_a_derived_symbol_is_loaded_with_its_parent() {
        let dir = temp("halve");
        let pretty = dir.join("A.pretty");
        std::fs::create_dir_all(&pretty).unwrap();
        for i in 0..9 {
            let text = if i == 2 || i == 7 { GOOD.replace("(layer", "(frobnicate 1) (layer") } else { GOOD.to_string() };
            std::fs::write(pretty.join(format!("F{i}.kicad_mod")), text).unwrap();
        }
        let runs = std::cell::Cell::new(0);
        let refused = refused_files_with(Kind::Footprint, &pretty, &fake(&runs)).unwrap();
        assert_eq!(refused, [pretty.join("F2.kicad_mod"), pretty.join("F7.kicad_mod")]);
        assert!(runs.get() < 14, "{} runs", runs.get());
        // a library that loads costs one run
        let runs = std::cell::Cell::new(0);
        std::fs::remove_file(pretty.join("F2.kicad_mod")).unwrap();
        std::fs::remove_file(pretty.join("F7.kicad_mod")).unwrap();
        assert!(refused_files_with(Kind::Footprint, &pretty, &fake(&runs)).unwrap().is_empty());
        assert_eq!(runs.get(), 1);
        // a folder symbol library: the child in its own file is loaded with its parent's file, so only the bad file is refused
        let symdir = dir.join("S.kicad_symdir");
        std::fs::create_dir_all(&symdir).unwrap();
        std::fs::write(symdir.join("A.kicad_sym"), "(kicad_symbol_lib (version 20251024)\n(symbol \"A\" (property \"Reference\" \"U\" (at 0 0 0)))\n)\n").unwrap();
        std::fs::write(symdir.join("B.kicad_sym"), "(kicad_symbol_lib (version 20251024)\n(symbol \"B\" (frobnicate) (property \"Reference\" \"U\" (at 0 0 0)))\n)\n").unwrap();
        std::fs::write(symdir.join("Z.kicad_sym"), "(kicad_symbol_lib (version 20251024)\n(symbol \"Z\" (extends \"A\") (property \"Reference\" \"U\" (at 0 0 0)))\n)\n").unwrap();
        let runs = std::cell::Cell::new(0);
        assert_eq!(refused_files_with(Kind::Symbol, &symdir, &fake(&runs)).unwrap(), [symdir.join("B.kicad_sym")]);
        // a file library is refused whole
        let file = dir.join("F.kicad_sym");
        std::fs::write(&file, "(kicad_symbol_lib (version 20251024)\n(symbol \"B\" (frobnicate) (property \"Reference\" \"U\" (at 0 0 0)))\n)\n").unwrap();
        assert_eq!(refused_files_with(Kind::Symbol, &file, &fake(&runs)).unwrap(), [file.clone()]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
