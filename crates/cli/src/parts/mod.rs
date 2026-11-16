//! The parts cache: one SQLite file (`~/.cache/eda-studio/parts.sqlite`, `EDA_PARTS_DB` to move it) holding what every installed KiCad symbol and
//! footprint is -- fields, pins, pads, pitch, courtyard size, 3D model, KiCad's parser's verdict -- so the studio, the CLI and an agent ask one index
//! instead of each reading 222 MB of symbols and 15,000 footprint files (`docs/PARTS-CACHE.md` for its use).
//!
//! It is not a KiCad feature: KiCad keeps its libraries in memory per process (`LIBRARY_MANAGER`, `FOOTPRINT_LIST_IMPL`) and reads them again on every
//! start. The cache stands in for those reads where it can say they would give the same answer (phase 1b: `library_search::symbol_lib` /
//! `footprint_lib` are filled from it when the library's stamp matches), and is otherwise a store of facts about the libraries, never a source of truth: the
//! library files are.
//!
//!   * [`schema`]: the tables (`libraries`, `files`, `table_rows`, `symbols`, `symbol_pins`, `footprints`, `footprint_pads`, the FTS5 trigram indexes, and the
//!     empty supplier tables of phase 2).
//!   * [`store`]: opening it (WAL, a 5 s busy timeout, the `kicad_wildcard` SQL function), the lease that lets one process build at a time, the reads.
//!   * [`refresh`]: indexing -- a library is re-read when its stamp moved (shallow: one stat per library; deep: one per file, only the changed footprint files
//!     re-read), several libraries at once (`std::thread::scope`) and one writer, one transaction per library; KiCad's parser's verdict
//!     (`crate::library_verdict`) is imported where it is known and asked of kicad-cli only by `refresh --verify`, `eda parts verify` and the studio's indexer
//!     thread -- never on a query.
//!   * [`query`]: search (KiCad's chooser scoring, `library_search::score_item`, over an FTS prefilter), show, fits, stats, check-board, status, SQL.
//!   * [`cmd`]: `eda parts ...`; [`api`]: `GET /api/parts/*` and the studio's indexer thread.
//!
//! A library is known by its kind and its path (the path a table row resolves to); nicknames are the tables' business and are resolved per board through
//! `library_tables::libs` on every query.

pub(crate) mod api;
pub(crate) mod cmd;
pub(crate) mod query;
pub(crate) mod refresh;
pub(crate) mod schema;
pub(crate) mod store;

use crate::library_index::Kind;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock, PoisonError};
use std::time::SystemTime;

/// Where the cache is: `EDA_PARTS_DB`, else `parts.sqlite` in the studio's cache folder (beside `3dmodels/` and `libraries/`). A test that does not name one gets a
/// file of its own process, never the user's.
pub(crate) fn db_path() -> PathBuf {
    if let Some(p) = std::env::var_os("EDA_PARTS_DB").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    #[cfg(test)]
    {
        return std::env::temp_dir().join(format!("eda-parts-test-{}.sqlite", std::process::id()));
    }
    #[allow(unreachable_code)]
    crate::model3d_api::cache_dir().parent().map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir).join("parts.sqlite")
}

pub(crate) fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Symbol => "symbol",
        Kind::Footprint => "footprint",
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the studio's indexer thread is asked to do next ([`invalidate`], [`queue_verdict`]): libraries to read again, libraries whose verdict a query wanted.
#[derive(Default)]
pub(crate) struct Queue {
    pub(crate) stale: Vec<(Kind, PathBuf)>,
    pub(crate) verdicts: Vec<(Kind, PathBuf)>,
}

pub(crate) fn queue() -> &'static (Mutex<Queue>, Condvar) {
    static Q: OnceLock<(Mutex<Queue>, Condvar)> = OnceLock::new();
    Q.get_or_init(Default::default)
}

/// The library at `path` changed (the studio saved, reverted or deleted in it, a table row was added or changed): the indexer reads it again soon. Nothing
/// waits for it -- every query compares the library's stamp anyway; this only makes the cache catch up before the next one asks.
pub(crate) fn invalidate(kind: Kind, path: &Path) {
    let (q, cv) = queue();
    let mut q = lock(q);
    if !q.stale.iter().any(|(k, p)| *k == kind && p == path) {
        q.stale.push((kind, path.to_path_buf()));
    }
    cv.notify_all();
}

/// Every library of the board's tables may have changed (a table was written): the indexer looks at all of them again.
pub(crate) fn invalidate_tables(dir: &Path) {
    for kind in [Kind::Symbol, Kind::Footprint] {
        // the paths only (`LibSet::from_libs` would convert a foreign library: the indexer does that on its own thread)
        for lib in crate::library_tables::load(dir, kind.table_kind()).libs() {
            invalidate(kind, &lib.path);
        }
    }
}

/// A query met a library whose KiCad verdict is not known: the indexer asks kicad-cli about it (one at a time, never on the query's thread).
pub(crate) fn queue_verdict(kind: Kind, path: &Path) {
    let (q, cv) = queue();
    let mut q = lock(q);
    if !q.verdicts.iter().any(|(k, p)| *k == kind && p == path) {
        q.verdicts.push((kind, path.to_path_buf()));
    }
    cv.notify_all();
}

// ------------------------------------------------------------------------------------------------ phase 1b: the choosers' indexes from the cache

/// The row of the library at `path` when it is of the state `stamp` (the library's modification time, as the choosers' caches stamp it).
fn fresh_row(store: &store::Store, kind: Kind, path: &Path, stamp: SystemTime) -> Option<store::LibRow> {
    let row = store.library(kind, path)?;
    (row.stamp.as_deref() == Some(refresh::nanos(stamp).to_string().as_str())).then_some(row)
}

/// The symbols of the library at `path` as `library_search::symbol_lib` would read them -- scanned, derived ones given their bases' fields, sorted by name
/// the tree's way, a name once -- and the reader's load message, from the cache's rows when they are of the state `stamp`; `None` when they are not (or
/// there is no cache), and the library is read.
pub(crate) fn cached_symbols(path: &Path, stamp: SystemTime) -> Option<(Vec<eda_kicad::SymbolSummary>, Option<String>)> {
    store::with_reader(|store| {
        let row = fresh_row(store, Kind::Symbol, path, stamp)?;
        let mut st = store.conn.prepare_cached(&format!("SELECT {} FROM symbols s WHERE s.library_id = ?1", query::SYMBOL_COLUMNS)).ok()?;
        let mut list: Vec<eda_kicad::SymbolSummary> = st.query_map([row.id], |r| query::symbol_summary_of(r, 0)).ok()?.collect::<Result<_, _>>().ok()?;
        list.sort_by(|a, b| crate::library_search::natural_cmp(&a.name, &b.name).then_with(|| a.name.cmp(&b.name)));
        Some((list, row.load_error))
    })
    .flatten()
}

/// The reader's load message of the symbol library at `path` (`eda_kicad::symbol_library_load_errors`), from the cache's row when it is of the state `stamp`.
pub(crate) fn cached_load_error(path: &Path, stamp: SystemTime) -> Option<Option<String>> {
    store::with_reader(|store| fresh_row(store, Kind::Symbol, path, stamp).map(|r| r.load_error)).flatten()
}

/// The footprints of the `.pretty` folder `dir` as `library_search::footprint_lib` would read them -- every `*.kicad_mod` by name, sorted the tree's way, with
/// the chooser's summary (a file that does not read kept by its name) -- from the cache's rows when they are of the folder's state `stamp` and name as many
/// files as the folder holds now; `None` otherwise, and the folder is read.
pub(crate) fn cached_footprints(dir: &Path, stamp: SystemTime) -> Option<(Vec<String>, Vec<eda_kicad::FootprintSummary>)> {
    store::with_reader(|store| {
        let row = fresh_row(store, Kind::Footprint, dir, stamp)?;
        let mut st = store.conn.prepare_cached("SELECT name, description, tags, numbered_pads, pads FROM footprints WHERE library_id = ?1").ok()?;
        let mut list: Vec<(String, eda_kicad::FootprintSummary)> = st
            .query_map([row.id], |r| Ok((r.get::<_, String>(0)?, eda_kicad::FootprintSummary { description: r.get(1)?, tags: r.get(2)?, numbered_pads: r.get(3)?, pads: r.get(4)? })))
            .ok()?
            .collect::<Result<_, _>>()
            .ok()?;
        // the folder's files as `footprint_lib` lists them (a hidden one too): the same count, or the folder is read
        let files = std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("kicad_mod")).count();
        if files != list.len() {
            return None;
        }
        list.sort_by(|a, b| crate::library_search::natural_cmp(&a.0, &b.0).then_with(|| a.0.cmp(&b.0)));
        Some(list.into_iter().unzip())
    })
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::refresh::tests::fixture;
    use super::refresh::{refresh_targets, ModelCtx, Target};
    use super::*;
    use crate::library_search::natural_cmp;

    /// What `library_search::symbol_lib` reads of a library when it reads the files.
    fn scanned_symbols(path: &Path) -> (Vec<eda_kicad::SymbolSummary>, Option<String>) {
        let (text, load_error) = eda_kicad::read_symbol_library(path).unwrap();
        let mut list = eda_kicad::scan_symbols(&text);
        eda_kicad::inherit_symbol_summaries(&mut list);
        list.sort_by(|a, b| natural_cmp(&a.name, &b.name).then_with(|| a.name.cmp(&b.name)));
        list.dedup_by(|b, a| a.name == b.name);
        (list, load_error)
    }

    /// What `library_search::footprint_lib` reads of a folder when it reads the files.
    fn scanned_footprints(dir: &Path) -> (Vec<String>, Vec<eda_kicad::FootprintSummary>) {
        let mut files: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("kicad_mod"))
            .filter_map(|p| Some((p.file_stem()?.to_str()?.to_string(), p)))
            .collect();
        files.sort_by(|a, b| natural_cmp(&a.0, &b.0).then_with(|| a.0.cmp(&b.0)));
        files.into_iter().map(|(n, p)| (n, std::fs::read_to_string(&p).ok().and_then(|t| eda_kicad::scan_footprint(&t)).unwrap_or_default())).unzip()
    }

    /// Phase 1b: the choosers' indexes filled from the cache are the indexes the files give -- on the fixture, and on some of the installed libraries when
    /// KiCad's are there -- and a library changed since it was indexed is read from its files again.
    #[test]
    fn the_choosers_indexes_from_the_cache_are_the_ones_the_files_give() {
        let (dir, mut targets) = fixture("1b");
        for (kind, names) in [(Kind::Symbol, &["Device", "Amplifier_Operational", "power", "Regulator_Linear", "MCU_ST_STM32F1"][..]), (Kind::Footprint, &["Resistor_SMD", "Package_SO", "Package_QFP", "Connector_PinHeader_2.54mm"][..])] {
            for n in names {
                if let Ok(path) = crate::library_index::LibSource::lib_path(&kind.root(), kind, n) {
                    if kind.modified(&path).is_some() {
                        targets.push(Target { kind, path, nickname: n.to_string(), format: "kicad" });
                    }
                }
            }
        }
        let store = store::Store::open(&db_path()).unwrap();
        refresh_targets(&store, &targets, false, &ModelCtx::for_dir(&dir)).unwrap();
        let (mut symbols, mut footprints) = (0, 0);
        for t in &targets {
            let stamp = t.kind.modified(&t.path).unwrap();
            match t.kind {
                Kind::Symbol => {
                    let loaded = cached_symbols(&t.path, stamp).unwrap_or_else(|| panic!("{} is in the cache", t.path.display()));
                    assert_eq!(loaded, scanned_symbols(&t.path), "{}", t.path.display());
                    assert_eq!(cached_load_error(&t.path, stamp), Some(loaded.1.clone()));
                    symbols += loaded.0.len();
                }
                Kind::Footprint => {
                    let loaded = cached_footprints(&t.path, stamp).unwrap_or_else(|| panic!("{} is in the cache", t.path.display()));
                    assert_eq!(loaded, scanned_footprints(&t.path), "{}", t.path.display());
                    footprints += loaded.0.len();
                }
            }
        }
        assert!(symbols >= 5 && footprints >= 3, "{symbols} symbols, {footprints} footprints compared");
        // the chooser's index of the fixture's library comes from the cache and answers as the files do
        let idx = crate::library_search::symbol_lib(&dir, "Lib").unwrap();
        assert_eq!(idx.summaries, scanned_symbols(&dir.join("Lib.kicad_sym")).0);
        // another state of the library: not the cache's
        std::thread::sleep(std::time::Duration::from_millis(20));
        let lib = dir.join("Lib.kicad_sym");
        std::fs::write(&lib, std::fs::read_to_string(&lib).unwrap().replace("Dual operational amplifier", "Edited")).unwrap();
        assert!(cached_symbols(&lib, Kind::Symbol.modified(&lib).unwrap()).is_none());
        let pretty = dir.join("Pkg.pretty");
        std::fs::write(pretty.join(".hidden.kicad_mod"), "(footprint \"h\")").unwrap();
        assert!(cached_footprints(&pretty, Kind::Footprint.modified(&pretty).unwrap()).is_none(), "a folder of another state");
        let idx = crate::library_search::symbol_lib(&dir, "Lib").unwrap();
        assert!(idx.summaries.iter().any(|s| s.description == "Edited"), "read from the file");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
