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

pub(crate) mod cmd;
pub(crate) mod query;
pub(crate) mod refresh;
pub(crate) mod schema;
pub(crate) mod store;

use crate::library_index::Kind;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock, PoisonError};

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
