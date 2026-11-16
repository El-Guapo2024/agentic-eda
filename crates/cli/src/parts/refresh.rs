//! Indexing: bringing the cache's rows of a set of libraries up to date with the files.
//!
//! A library's **shallow stamp** is one stat: a symbol library's modification time (a `.kicad_symdir` folder's latest of itself and its files,
//! `eda_kicad::symbol_library_modified`, what the choosers' caches compare) or a `.pretty` folder's own time (a footprint added, removed or saved by KiCad's
//! write-then-rename). It is compared on every query and every refresh. A footprint edited in place moves only its file's time, so a **deep** check
//! (`eda parts refresh --deep`, the studio's start and every five minutes) lists every file's size and time against the `files` table and reads again only the
//! footprints that changed. A symbol library is read whole when it changed (a derived symbol takes its base's pins and fields, maybe from another file).
//!
//! Several libraries are read at once (`std::thread::scope`, [`workers`] threads) and one writer puts each into the database in a transaction of its own, so a
//! reader sees a library before or after, never half. One process builds at a time: the writer takes the `lease` row first; a second refresher waits for it,
//! then finds the stamps current and reads nothing (`two_refreshers_build_once`).

use super::store::{now_secs, Store};
use super::{kind_name, schema};
use crate::library_index::Kind;
use eda_kicad::{FootprintDetail, PinDetail, SymbolSummary};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// One library to keep current: its kind, its path and the nickname it was found under.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Target {
    pub(crate) kind: Kind,
    pub(crate) path: PathBuf,
    pub(crate) nickname: String,
    /// `kicad`, or `converted` for the KiCad copy kicad-cli made of another format's library (`crate::library_convert`), or `http` for an HTTP library's
    /// readable file (`crate::http_lib`).
    pub(crate) format: &'static str,
}

/// The libraries of the board in `dir` (both kinds unless `kinds` says), every row of its tables, hidden ones too, as `library_tables::libs` resolves them.
pub(crate) fn board_targets(dir: &Path, kinds: &[Kind]) -> Vec<Target> {
    let converted = crate::library_convert::cache_dir();
    let mut out = Vec::new();
    for &kind in kinds {
        for e in crate::library_tables::libs(dir, kind).entries() {
            let format = if e.path.starts_with(&converted) { "converted" } else if e.path.extension().is_some_and(|x| x == "http") { "http" } else { "kicad" };
            out.push(Target { kind, path: e.path.clone(), nickname: e.name.clone(), format });
        }
    }
    out
}

pub(crate) fn nanos(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64)
}

/// The shallow stamp of the library at `path` (see the module doc); `None` when nothing is there.
pub(crate) fn shallow_stamp(kind: Kind, path: &Path) -> Option<String> {
    let t = match kind {
        Kind::Symbol => eda_kicad::symbol_library_modified(path)?,
        Kind::Footprint => std::fs::metadata(path).ok().filter(|m| m.is_dir())?.modified().ok()?,
    };
    Some(nanos(t).to_string())
}

/// One file of a library, as the `files` table keeps it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FileStamp {
    pub(crate) rel: String,
    pub(crate) mtime: i64,
    pub(crate) size: i64,
}

/// The files of a library: a `.pretty` folder's `*.kicad_mod`, a `.kicad_symdir` folder's `*.kicad_sym` (hidden ones left out, as KiCad's listings leave
/// them), or the one `.kicad_sym` file; sorted by name.
pub(crate) fn list_files(kind: Kind, path: &Path) -> Vec<FileStamp> {
    let stamp = |p: &Path, rel: String| std::fs::metadata(p).ok().filter(|m| m.is_file()).map(|m| FileStamp { rel, mtime: m.modified().map(nanos).unwrap_or(0), size: m.len() as i64 });
    if !path.is_dir() {
        return path.file_name().and_then(|n| stamp(path, n.to_string_lossy().into_owned())).into_iter().collect();
    }
    let ext = match kind {
        Kind::Footprint => "kicad_mod",
        Kind::Symbol => "kicad_sym",
    };
    let mut out: Vec<FileStamp> = std::fs::read_dir(path)
        .map(|r| {
            r.filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let p = e.path();
                    (!name.starts_with('.') && p.extension().is_some_and(|x| x == ext)).then(|| stamp(&p, name)).flatten()
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

pub(crate) fn files_stamp(files: &[FileStamp]) -> String {
    let mut h = blake3::Hasher::new();
    for f in files {
        h.update(format!("{}={}@{}|", f.rel, f.size, f.mtime).as_bytes());
    }
    h.finalize().to_hex()[..24].to_string()
}

/// Where a footprint's 3D models are looked for: the board's roots and path variables (`model3d_api::resolve`, KiCad's `FILENAME_RESOLVER` and the 3D view's
/// allow-list), so `model_found` says what the 3D view of that board would find.
pub(crate) struct ModelCtx {
    roots: crate::model3d_api::Roots,
    names: crate::library_tables::ModelNames,
}

impl ModelCtx {
    pub(crate) fn for_dir(dir: &Path) -> ModelCtx {
        let names = crate::library_tables::model_names(dir);
        ModelCtx { roots: crate::model3d_api::board_roots(dir, &names), names }
    }

    /// It names a model, and every model it names is found.
    pub(crate) fn found(&self, models: &[String]) -> bool {
        !models.is_empty() && models.iter().all(|m| crate::model3d_api::resolve(m, &self.roots, &|v| self.names.get(v)).is_ok())
    }

    /// What `model_found` was worked out against: the model library's folder and time, and the other roots.
    pub(crate) fn stamp(&self) -> String {
        let t = std::fs::metadata(&self.roots.models).ok().and_then(|m| m.modified().ok()).map_or(0, nanos);
        format!("{}@{t}|{:?}|{:?}", self.roots.models.display(), self.roots.extra, self.roots.aliases)
    }
}

/// What a refresh did.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Report {
    /// Libraries looked at.
    pub(crate) libraries: usize,
    /// Libraries read (whole or in part).
    pub(crate) read: usize,
    /// Footprint files read in a library read in part, plus every file of a library read whole.
    pub(crate) files_read: usize,
    /// Libraries whose path is gone, taken out.
    pub(crate) removed: usize,
    /// The libraries read, `kind:path`.
    pub(crate) read_paths: Vec<String>,
    /// It waited for another process's build first.
    pub(crate) waited: bool,
    pub(crate) ms: u128,
}

impl Report {
    pub(crate) fn json(&self) -> Value {
        json!({ "libraries": self.libraries, "read": self.read, "files_read": self.files_read, "removed": self.removed, "waited_for_another_build": self.waited, "ms": self.ms as u64 })
    }
}

/// How many libraries are read at once: the cores there are, at most 8 (the writer is one more).
pub(crate) fn workers() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8)
}

/// What one library needs.
enum Need {
    Nothing,
    /// Only the stamps moved (a folder's time, no file changed): the row's stamps are written.
    Stamps { stamp: String, files: Vec<FileStamp> },
    Symbols { stamp: String, files: Vec<FileStamp> },
    Footprints { stamp: String, files: Vec<FileStamp>, full: bool, changed: Vec<FileStamp>, removed: Vec<String> },
    Gone,
}

struct ScannedSymbol {
    summary: SymbolSummary,
    pins: Vec<PinDetail>,
    file: String,
}

struct ScannedFootprint {
    name: String,
    file: FileStamp,
    detail: Option<FootprintDetail>,
    reader_error: Option<String>,
    model_found: bool,
}

/// KiCad's parser's verdict, where it is known: the items it refuses and the library's message with them.
struct Verdict {
    refused: Vec<String>,
    message: Option<String>,
    state: &'static str,
}

enum Scanned {
    Symbols { load_error: Option<String>, symbols: Result<Vec<ScannedSymbol>, String>, verdict: Option<Verdict> },
    Footprints { items: Vec<ScannedFootprint>, verdict: Option<Verdict> },
}

/// The symbols of the library at `path` as `library_search::symbol_lib` keeps them -- scanned, derived ones given their bases' fields, sorted by name the
/// tree's way, a name once -- with each one's pins, and the reader's load message.
fn scan_symbol_library(path: &Path) -> Result<(Vec<ScannedSymbol>, Option<String>), String> {
    let (text, load_error) = eda_kicad::read_symbol_library(path)?;
    let mut summaries = eda_kicad::scan_symbols(&text);
    eda_kicad::inherit_symbol_summaries(&mut summaries);
    summaries.sort_by(|a, b| crate::library_search::natural_cmp(&a.name, &b.name).then_with(|| a.name.cmp(&b.name)));
    summaries.dedup_by(|b, a| a.name == b.name);
    let mut pins: HashMap<String, Vec<PinDetail>> = HashMap::new();
    for (name, p) in eda_kicad::scan_symbol_pins(&text) {
        pins.entry(name).or_insert(p);
    }
    let folder = path.is_dir();
    let own = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok((
        summaries
            .into_iter()
            .map(|s| {
                let file = if folder {
                    let f = format!("{}.kicad_sym", s.name);
                    if path.join(&f).is_file() { f } else { String::new() }
                } else {
                    own.clone()
                };
                ScannedSymbol { pins: pins.remove(&s.name).unwrap_or_default(), summary: s, file }
            })
            .collect(),
        load_error,
    ))
}

fn scan_footprint_file(dir: &Path, file: &FileStamp, models: &ModelCtx) -> ScannedFootprint {
    let name = file.rel.strip_suffix(".kicad_mod").unwrap_or(&file.rel).to_string();
    let (detail, reader_error) = match std::fs::read_to_string(dir.join(&file.rel)) {
        Ok(text) => match eda_kicad::scan_footprint_detail(&text) {
            Some(d) => (Some(d), None),
            None => (None, Some("the top-level form is not (footprint ...); not a KiCad footprint file".to_string())),
        },
        Err(e) => (None, Some(e.to_string())),
    };
    let model_found = detail.as_ref().is_some_and(|d| models.found(&d.models));
    ScannedFootprint { name, file: file.clone(), detail, reader_error, model_found }
}

/// The verdict known for the library now (never asked here), with the items it names.
fn known_verdict(kind: Kind, path: &Path, load_error: Option<String>) -> Option<Verdict> {
    let refused = crate::library_verdict::verdict(kind, path, false)?;
    Some(verdict_of(kind, path, refused, load_error))
}

fn verdict_of(kind: Kind, path: &Path, refused: Vec<PathBuf>, load_error: Option<String>) -> Verdict {
    match kind {
        Kind::Footprint => {
            let names: Vec<String> = refused.iter().filter_map(|f| f.file_stem().map(|s| s.to_string_lossy().into_owned())).collect();
            Verdict { state: if names.is_empty() { "ok" } else { "some_refused" }, refused: names, message: None }
        }
        Kind::Symbol => {
            let any = !refused.is_empty();
            let (names, message) = crate::library_verdict::symbol_refusals_from(path, refused, load_error);
            Verdict { state: if any { "some_refused" } else { "ok" }, refused: names, message: if any { message } else { None } }
        }
    }
}

fn read_files_table(conn: &Connection, library_id: i64) -> HashMap<String, (i64, i64)> {
    let Ok(mut st) = conn.prepare_cached("SELECT rel, mtime, size FROM files WHERE library_id = ?1") else { return HashMap::new() };
    st.query_map([library_id], |r| Ok((r.get::<_, String>(0)?, (r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))).map(|rows| rows.filter_map(|r| r.ok()).collect()).unwrap_or_default()
}

type Rows = HashMap<(&'static str, String), super::store::LibRow>;

/// Every library row by kind and path, read once for a refresh.
fn rows_of(store: &Store) -> Rows {
    store.libraries().into_iter().map(|r| ((if r.kind == "symbol" { "symbol" } else { "footprint" }, r.path.clone()), r)).collect()
}

/// What library `t` needs, its row as it is in the database.
fn need_of(store: &Store, rows: &Rows, t: &Target, deep: bool) -> Need {
    let Some(stamp) = shallow_stamp(t.kind, &t.path) else { return Need::Gone };
    let row = rows.get(&(kind_name(t.kind), t.path.to_string_lossy().into_owned())).cloned();
    let current = row.as_ref().and_then(|r| r.stamp.clone());
    if current.as_deref() == Some(stamp.as_str()) && !deep {
        return Need::Nothing;
    }
    let files = list_files(t.kind, &t.path);
    let fstamp = files_stamp(&files);
    let unread = current.is_none();
    match t.kind {
        Kind::Symbol => {
            if !unread && current.as_deref() == Some(stamp.as_str()) && row.as_ref().and_then(|r| r.files_stamp.as_deref()) == Some(fstamp.as_str()) {
                Need::Nothing
            } else {
                Need::Symbols { stamp, files }
            }
        }
        Kind::Footprint => {
            let known = row.as_ref().map(|r| read_files_table(&store.conn, r.id)).unwrap_or_default();
            if unread {
                return Need::Footprints { stamp, changed: files.clone(), files, full: true, removed: Vec::new() };
            }
            let changed: Vec<FileStamp> = files.iter().filter(|f| known.get(&f.rel) != Some(&(f.mtime, f.size))).cloned().collect();
            let present: HashSet<&str> = files.iter().map(|f| f.rel.as_str()).collect();
            let removed: Vec<String> = known.keys().filter(|k| !present.contains(k.as_str())).cloned().collect();
            if changed.is_empty() && removed.is_empty() {
                if current.as_deref() == Some(stamp.as_str()) && row.as_ref().and_then(|r| r.files_stamp.as_deref()) == Some(fstamp.as_str()) {
                    Need::Nothing
                } else {
                    Need::Stamps { stamp, files }
                }
            } else {
                Need::Footprints { stamp, files, full: false, changed, removed }
            }
        }
    }
}

/// Bring the rows of the board's libraries (in `dir`) up to date, and its table rows. See [`refresh_targets`].
pub(crate) fn refresh(store: &Store, dir: &Path, kinds: &[Kind], deep: bool) -> Result<Report, String> {
    let targets = board_targets(dir, kinds);
    let report = refresh_targets(store, &targets, deep, &ModelCtx::for_dir(dir))?;
    write_table_rows(store, dir, kinds);
    Ok(report)
}

/// Bring the rows of `targets` up to date: each library whose stamp moved (or, `deep`, any of whose files did) is read again in the background threads and
/// written by this one, under the build lease.
pub(crate) fn refresh_targets(store: &Store, targets: &[Target], deep: bool, models: &ModelCtx) -> Result<Report, String> {
    let started = Instant::now();
    let owner = format!("{}-{:?}-{}", std::process::id(), std::thread::current().id(), nanos(SystemTime::now()));
    let mut report = Report { libraries: targets.len(), ..Default::default() };
    // nothing to do is found without writing anything (no lease): a query's look before it answers
    let rows = rows_of(store);
    let idle = targets.iter().all(|t| match need_of(store, &rows, t, deep) {
        Need::Nothing => true,
        Need::Gone => !rows.contains_key(&(kind_name(t.kind), t.path.to_string_lossy().into_owned())),
        _ => false,
    });
    if idle && schema::meta(&store.conn, "models_stamp").as_deref() == Some(models.stamp().as_str()) {
        report.ms = started.elapsed().as_millis();
        return Ok(report);
    }
    // one build at a time: wait for another process's (it may make ours a no-op)
    let wait_from = Instant::now();
    while !store.take_lease(&owner)? {
        report.waited = true;
        if wait_from.elapsed() > std::time::Duration::from_secs(600) {
            return Err("another process has been building the parts cache for ten minutes".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let result = build(store, targets, deep, models, &owner, &mut report);
    store.release_lease(&owner);
    result?;
    report.ms = started.elapsed().as_millis();
    Ok(report)
}

fn build(store: &Store, targets: &[Target], deep: bool, models: &ModelCtx, owner: &str, report: &mut Report) -> Result<(), String> {
    let mut work: Vec<(&Target, Need)> = Vec::new();
    let rows = rows_of(store);
    for t in targets {
        match need_of(store, &rows, t, deep) {
            Need::Nothing => {}
            Need::Gone => {
                if store.library(t.kind, &t.path).is_some() {
                    store.conn.execute("DELETE FROM libraries WHERE kind = ?1 AND path = ?2", params![kind_name(t.kind), t.path.to_string_lossy()]).map_err(|e| e.to_string())?;
                    report.removed += 1;
                }
            }
            Need::Stamps { stamp, files } => write_stamps(&store.conn, t, &stamp, &files).map_err(|e| e.to_string())?,
            need => work.push((t, need)),
        }
    }
    let next = AtomicUsize::new(0);
    let (tx, rx) = std::sync::mpsc::sync_channel::<(usize, Scanned)>(8);
    let mut failure: Option<String> = None;
    std::thread::scope(|scope| {
        for _ in 0..workers().min(work.len().max(1)) {
            let (tx, next, work) = (tx.clone(), &next, &work);
            scope.spawn(move || loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some((t, need)) = work.get(i) else { break };
                let scanned = match need {
                    Need::Symbols { .. } => match scan_symbol_library(&t.path) {
                        Ok((symbols, load_error)) => Scanned::Symbols { verdict: known_verdict(t.kind, &t.path, load_error.clone()), load_error, symbols: Ok(symbols) },
                        Err(e) => Scanned::Symbols { load_error: None, symbols: Err(e), verdict: None },
                    },
                    Need::Footprints { changed, .. } => Scanned::Footprints {
                        items: changed.iter().map(|f| scan_footprint_file(&t.path, f, models)).collect(),
                        verdict: known_verdict(t.kind, &t.path, None),
                    },
                    _ => continue,
                };
                if tx.send((i, scanned)).is_err() {
                    break;
                }
            });
        }
        drop(tx);
        for (i, scanned) in rx {
            let (t, need) = &work[i];
            let written = match (need, scanned) {
                (Need::Symbols { stamp, files }, Scanned::Symbols { load_error, symbols, verdict }) => {
                    report.files_read += files.len();
                    match symbols {
                        Ok(symbols) => write_symbols(&store.conn, t, stamp, files, &symbols, load_error, verdict),
                        // a library that cannot be read at all is a row with its error and no symbols
                        Err(e) => write_symbols(&store.conn, t, stamp, files, &[], Some(e), None),
                    }
                }
                (Need::Footprints { stamp, files, full, removed, .. }, Scanned::Footprints { items, verdict }) => {
                    report.files_read += items.len();
                    write_footprints(&store.conn, t, stamp, files, *full, removed, &items, verdict)
                }
                _ => Ok(()),
            };
            match written {
                Ok(()) => {
                    report.read += 1;
                    report.read_paths.push(format!("{}:{}", kind_name(t.kind), t.path.display()));
                }
                Err(e) => {
                    failure.get_or_insert(format!("{}: {e}", t.path.display()));
                }
            }
            store.beat(owner);
        }
    });
    if let Some(e) = failure {
        return Err(e);
    }
    refresh_model_found(store, models).map_err(|e| e.to_string())?;
    Ok(())
}

fn library_id(conn: &Connection, t: &Target, stamp: &str, files: &[FileStamp]) -> rusqlite::Result<i64> {
    conn.query_row(
        "INSERT INTO libraries (kind, path, nickname, format, stamp, files_stamp, indexed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(kind, path) DO UPDATE SET nickname = excluded.nickname, format = excluded.format, stamp = excluded.stamp, files_stamp = excluded.files_stamp, indexed_at = excluded.indexed_at
         RETURNING id",
        params![kind_name(t.kind), t.path.to_string_lossy(), t.nickname, t.format, stamp, files_stamp(files), now_secs()],
        |r| r.get(0),
    )
}

fn write_files(conn: &Connection, id: i64, files: &[FileStamp]) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM files WHERE library_id = ?1", [id])?;
    let mut st = conn.prepare_cached("INSERT INTO files (library_id, rel, mtime, size) VALUES (?1, ?2, ?3, ?4)")?;
    for f in files {
        st.execute(params![id, f.rel, f.mtime, f.size])?;
    }
    Ok(())
}

fn bump_generation(conn: &Connection) -> rusqlite::Result<()> {
    let g = schema::meta(conn, "generation").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
    schema::set_meta(conn, "generation", &(g + 1).to_string())
}

fn write_stamps(conn: &Connection, t: &Target, stamp: &str, files: &[FileStamp]) -> rusqlite::Result<()> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    let id = library_id(&tx, t, stamp, files)?;
    write_files(&tx, id, files)?;
    tx.commit()
}

/// The verdict's columns: the one given (for the library's files now), or none known for a state that is not the one the last verdict was for.
fn write_verdict(conn: &Connection, id: i64, kind: Kind, files: &[FileStamp], verdict: Option<&Verdict>) -> rusqlite::Result<()> {
    let fstamp = files_stamp(files);
    let table = match kind {
        Kind::Symbol => "symbols",
        Kind::Footprint => "footprints",
    };
    match verdict {
        Some(v) => {
            conn.execute(
                "UPDATE libraries SET verdict_stamp = ?2, verdict_state = ?3, verdict_message = ?4, verdict_at = ?5 WHERE id = ?1",
                params![id, fstamp, v.state, v.message, now_secs()],
            )?;
            conn.execute(&format!("UPDATE {table} SET kicad_refused = 0 WHERE library_id = ?1"), [id])?;
            let mut st = conn.prepare_cached(&format!("UPDATE {table} SET kicad_refused = 1 WHERE library_id = ?1 AND name = ?2"))?;
            for name in &v.refused {
                st.execute(params![id, name])?;
            }
        }
        None => {
            conn.execute(
                "UPDATE libraries SET verdict_state = 'unknown', verdict_message = NULL WHERE id = ?1 AND (verdict_stamp IS NULL OR verdict_stamp != ?2)",
                params![id, fstamp],
            )?;
        }
    }
    Ok(())
}

fn write_symbols(conn: &Connection, t: &Target, stamp: &str, files: &[FileStamp], symbols: &[ScannedSymbol], load_error: Option<String>, verdict: Option<Verdict>) -> rusqlite::Result<()> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    let id = library_id(&tx, t, stamp, files)?;
    tx.execute("DELETE FROM symbols_fts WHERE rowid IN (SELECT id FROM symbols WHERE library_id = ?1)", [id])?;
    tx.execute("DELETE FROM symbols WHERE library_id = ?1", [id])?;
    write_files(&tx, id, files)?;
    {
        let mut sym = tx.prepare_cached(
            "INSERT INTO symbols (library_id, lib_nick_hint, name, extends, reference, value, footprint, datasheet, description, keywords, fp_filters, power, units, pin_count, pin_numbers,
             alternate_body_style, user_fields_json, span_start, span_end, file) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        )?;
        let mut pin = tx.prepare_cached("INSERT INTO symbol_pins (symbol_id, unit, body_style, number, name, electrical_type, shape, hidden) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)")?;
        let mut fts = tx.prepare_cached("INSERT INTO symbols_fts (rowid, name, keywords, description, footprint) VALUES (?1, ?2, ?3, ?4, ?5)")?;
        for s in symbols {
            let m = &s.summary;
            sym.execute(params![
                id,
                t.nickname,
                m.name,
                m.extends,
                m.reference,
                m.value,
                m.footprint,
                m.datasheet,
                m.description,
                m.keywords,
                m.fp_filters,
                m.power,
                m.units,
                m.pin_count() as i64,
                m.pin_numbers.join("\n"),
                m.alternate_body_style,
                serde_json::to_string(&m.user_fields).unwrap_or_else(|_| "[]".into()),
                m.span.start as i64,
                m.span.end as i64,
                s.file,
            ])?;
            let sid = tx.last_insert_rowid();
            for p in &s.pins {
                pin.execute(params![sid, p.unit, p.body_style, p.number, p.name, p.electrical_type, p.shape, p.hidden])?;
            }
            fts.execute(params![sid, m.name, m.keywords, m.description, m.footprint])?;
        }
    }
    tx.execute("UPDATE libraries SET item_count = ?2, load_error = ?3 WHERE id = ?1", params![id, symbols.len() as i64, load_error])?;
    write_verdict(&tx, id, Kind::Symbol, files, verdict.as_ref())?;
    bump_generation(&tx)?;
    tx.commit()
}

#[allow(clippy::too_many_arguments)]
fn write_footprints(conn: &Connection, t: &Target, stamp: &str, files: &[FileStamp], full: bool, removed: &[String], items: &[ScannedFootprint], verdict: Option<Verdict>) -> rusqlite::Result<()> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    let id = library_id(&tx, t, stamp, files)?;
    if full {
        tx.execute("DELETE FROM footprints_fts WHERE rowid IN (SELECT id FROM footprints WHERE library_id = ?1)", [id])?;
        tx.execute("DELETE FROM footprints WHERE library_id = ?1", [id])?;
    }
    write_files(&tx, id, files)?;
    {
        let mut gone_fts = tx.prepare_cached("DELETE FROM footprints_fts WHERE rowid IN (SELECT id FROM footprints WHERE library_id = ?1 AND name = ?2)")?;
        let mut gone = tx.prepare_cached("DELETE FROM footprints WHERE library_id = ?1 AND name = ?2")?;
        for rel in removed.iter().map(String::as_str).chain(items.iter().map(|f| f.file.rel.as_str())) {
            let name = rel.strip_suffix(".kicad_mod").unwrap_or(rel);
            gone_fts.execute(params![id, name])?;
            gone.execute(params![id, name])?;
        }
        let mut fp = tx.prepare_cached(
            "INSERT INTO footprints (library_id, lib_nick_hint, name, file_mtime, file_size, description, tags, attr, pads, numbered_pads, smd_pads, tht_pads, npth_pads, pitch_nm,
             courtyard_w_nm, courtyard_h_nm, has_courtyard, model_paths_json, model_found, exclude_from_bom, exclude_from_pos, reader_error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
        )?;
        let mut pad = tx.prepare_cached("INSERT INTO footprint_pads (footprint_id, number, type, shape, x, y, rot, w, h, drill) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)")?;
        let mut fts = tx.prepare_cached("INSERT INTO footprints_fts (rowid, name, tags, description) VALUES (?1, ?2, ?3, ?4)")?;
        for f in items {
            let d = f.detail.clone().unwrap_or_default();
            let s = &d.summary;
            let (cw, ch) = d.courtyard().map_or((0, 0), |(x0, y0, x1, y1)| (x1 - x0, y1 - y0));
            let attr = if f.detail.is_some() { d.attr.as_str() } else { "unspecified" };
            fp.execute(params![
                id,
                t.nickname,
                f.name,
                f.file.mtime,
                f.file.size,
                s.description,
                s.tags,
                attr,
                s.pads,
                s.numbered_pads,
                d.count_of("smd") + d.count_of("connect"),
                d.count_of("thru_hole"),
                d.count_of("np_thru_hole"),
                d.pitch_nm,
                cw,
                ch,
                d.courtyard().is_some(),
                serde_json::to_string(&d.models).unwrap_or_else(|_| "[]".into()),
                f.model_found,
                d.exclude_from_bom,
                d.exclude_from_pos,
                f.reader_error,
            ])?;
            let fid = tx.last_insert_rowid();
            for p in &d.pads {
                pad.execute(params![fid, p.number, p.pad_type, p.shape, p.x, p.y, p.rot, p.w, p.h, p.drill])?;
            }
            fts.execute(params![fid, f.name, s.tags, s.description])?;
        }
    }
    let count: i64 = tx.query_row("SELECT count(*) FROM footprints WHERE library_id = ?1", [id], |r| r.get(0))?;
    let refused: Vec<String> = {
        let mut st = tx.prepare_cached("SELECT name FROM footprints WHERE library_id = ?1 AND reader_error IS NOT NULL ORDER BY name")?;
        let rows = st.query_map([id], |r| r.get::<_, String>(0))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    tx.execute("UPDATE libraries SET item_count = ?2, reader_refused_json = ?3 WHERE id = ?1", params![id, count, serde_json::to_string(&refused).unwrap_or_default()])?;
    write_verdict(&tx, id, Kind::Footprint, files, verdict.as_ref())?;
    bump_generation(&tx)?;
    tx.commit()
}

/// `model_found` again for every footprint when what it was worked out against changed (the 3D model library installed or updated, other roots).
fn refresh_model_found(store: &Store, models: &ModelCtx) -> rusqlite::Result<()> {
    let stamp = models.stamp();
    if schema::meta(&store.conn, "models_stamp").as_deref() == Some(stamp.as_str()) {
        return Ok(());
    }
    let tx = rusqlite::Transaction::new_unchecked(&store.conn, rusqlite::TransactionBehavior::Immediate)?;
    let rows: Vec<(i64, String, bool)> = {
        let mut st = tx.prepare("SELECT id, model_paths_json, model_found FROM footprints")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    {
        let mut up = tx.prepare_cached("UPDATE footprints SET model_found = ?2 WHERE id = ?1")?;
        for (id, paths, was) in rows {
            let list: Vec<String> = serde_json::from_str(&paths).unwrap_or_default();
            let now = models.found(&list);
            if now != was {
                up.execute(params![id, now])?;
            }
        }
    }
    schema::set_meta(&tx, "models_stamp", &stamp)?;
    tx.commit()
}

/// The rows of the board's tables (in `dir`), as `table_rows`: KiCad's global ones under `global`, the board's own under `project:<dir>`.
pub(crate) fn write_table_rows(store: &Store, dir: &Path, kinds: &[Kind]) {
    let env = crate::library_tables::Env::discover();
    let project_scope = format!("project:{}", dir.display());
    for &kind in kinds {
        let set = crate::library_tables::libs(dir, kind);
        let global_table = crate::library_tables::global_table_path(&env, kind.table_kind()).filter(|p| p.is_file()).unwrap_or_else(|| crate::library_tables::stock_table(&env, kind.table_kind()));
        let project_table = crate::library_tables::project_table_path(dir, kind.table_kind());
        let Ok(tx) = rusqlite::Transaction::new_unchecked(&store.conn, rusqlite::TransactionBehavior::Immediate) else { return };
        let _ = tx.execute("DELETE FROM table_rows WHERE kind = ?1 AND (scope = 'global' OR scope = ?2)", params![kind_name(kind), project_scope]);
        for e in set.entries() {
            let (scope, table) = match e.scope {
                eda_kicad::library_table::Scope::Global => ("global".to_string(), &global_table),
                eda_kicad::library_table::Scope::Project => (project_scope.clone(), &project_table),
            };
            let lib_id: Option<i64> = tx.query_row("SELECT id FROM libraries WHERE kind = ?1 AND path = ?2", params![kind_name(kind), e.path.to_string_lossy()], |r| r.get(0)).ok();
            let _ = tx.execute(
                "INSERT OR REPLACE INTO table_rows (scope, kind, nickname, library_id, path, table_path, description, hidden) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![scope, kind_name(kind), e.name, lib_id, e.path.to_string_lossy(), table.to_string_lossy(), e.description, e.hidden],
            );
        }
        let _ = tx.commit();
    }
}

/// KiCad's parser's verdict on each of `targets`, asked of kicad-cli where it is not known (one library at a time; the library cache's `verdicts/` answer
/// the ones checked before at once), written into the cache. Libraries the cache has not read are left for after their refresh. How many were asked and
/// how many answered.
pub(crate) fn verify_targets(store: &Store, targets: &[Target], ask: bool, mut each: impl FnMut(&Target)) -> (usize, usize) {
    let (mut asked, mut answered) = (0, 0);
    for t in targets {
        let Some(row) = store.library(t.kind, &t.path) else { continue };
        let files = list_files(t.kind, &t.path);
        if row.verdict_stamp.as_deref() == Some(files_stamp(&files).as_str()) && row.verdict_state != "unknown" {
            continue;
        }
        asked += 1;
        each(t);
        let refused = crate::library_verdict::verdict(t.kind, &t.path, ask);
        let Some(refused) = refused else { continue };
        answered += 1;
        let v = verdict_of(t.kind, &t.path, refused, row.load_error.clone());
        let Ok(tx) = rusqlite::Transaction::new_unchecked(&store.conn, rusqlite::TransactionBehavior::Immediate) else { continue };
        if write_verdict(&tx, row.id, t.kind, &files, Some(&v)).and_then(|_| bump_generation(&tx)).is_ok() {
            let _ = tx.commit();
        }
    }
    (asked, answered)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eda-parts-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub(crate) const SOIC8: &str = r#"(footprint "SOIC-8_3.9x4.9mm_P1.27mm" (version 20240108) (generator "eda") (layer "F.Cu")
  (descr "SOIC, 8 Pin") (tags "SOIC SO") (attr smd)
  (fp_rect (start -3.7 -2.7) (end 3.7 2.7) (stroke (width 0.05) (type solid)) (layer "F.CrtYd"))
  (pad "1" smd roundrect (at -2.475 -1.905) (size 1.95 0.6) (layers "F.Cu" "F.Mask" "F.Paste"))
  (pad "2" smd roundrect (at -2.475 -0.635) (size 1.95 0.6) (layers "F.Cu" "F.Mask" "F.Paste"))
  (pad "3" smd roundrect (at -2.475 0.635) (size 1.95 0.6) (layers "F.Cu" "F.Mask" "F.Paste"))
  (pad "4" smd roundrect (at -2.475 1.905) (size 1.95 0.6) (layers "F.Cu" "F.Mask" "F.Paste"))
  (pad "5" smd roundrect (at 2.475 1.905) (size 1.95 0.6) (layers "F.Cu" "F.Mask" "F.Paste"))
  (pad "6" smd roundrect (at 2.475 0.635) (size 1.95 0.6) (layers "F.Cu" "F.Mask" "F.Paste"))
  (pad "7" smd roundrect (at 2.475 -0.635) (size 1.95 0.6) (layers "F.Cu" "F.Mask" "F.Paste"))
  (pad "8" smd roundrect (at 2.475 -1.905) (size 1.95 0.6) (layers "F.Cu" "F.Mask" "F.Paste"))
  (model "${KICAD10_3DMODEL_DIR}/Package_SO.3dshapes/SOIC-8_3.9x4.9mm_P1.27mm.step"))
"#;

    pub(crate) fn dip8() -> String {
        let mut s = String::from("(footprint \"DIP-8_W7.62mm\" (version 20240108) (generator \"eda\") (layer \"F.Cu\")\n  (descr \"8-lead dip package\") (tags \"THT DIP DIL PDIP 2.54mm 7.62mm 300mil\") (attr through_hole)\n");
        for i in 0..8 {
            let (x, y) = if i < 4 { (0.0, 2.54 * i as f64) } else { (7.62, 2.54 * (7 - i) as f64) };
            s.push_str(&format!("  (pad \"{}\" thru_hole oval (at {x} {y}) (size 1.6 1.6) (drill 0.8) (layers \"*.Cu\" \"*.Mask\"))\n", i + 1));
        }
        s.push_str("  (fp_line (start -1.1 -1.3) (end 8.7 -1.3) (layer \"F.CrtYd\"))\n  (fp_line (start 8.7 -1.3) (end 8.7 9.1) (layer \"F.CrtYd\"))\n  (fp_line (start -1.1 9.1) (end -1.1 -1.3) (layer \"F.CrtYd\")))\n");
        s
    }

    pub(crate) const R0603: &str = r#"(footprint "R_0603_1608Metric" (version 20240108) (generator "eda") (layer "F.Cu")
  (descr "Resistor SMD 0603 (1608 Metric)") (tags "resistor") (attr smd)
  (pad "1" smd roundrect (at -0.825 0) (size 0.8 0.95) (layers "F.Cu" "F.Mask" "F.Paste"))
  (pad "2" smd roundrect (at 0.825 0) (size 0.8 0.95) (layers "F.Cu" "F.Mask" "F.Paste")))
"#;

    pub(crate) const SYMS: &str = r#"(kicad_symbol_lib (version 20241209) (generator "eda")
  (symbol "LM358" (property "Reference" "U" (at 0 0 0)) (property "Value" "LM358" (at 0 0 0))
    (property "Footprint" "Pkg:SOIC-8_3.9x4.9mm_P1.27mm" (at 0 0 0)) (property "Description" "Dual operational amplifier" (at 0 0 0))
    (property "ki_keywords" "dual opamp" (at 0 0 0)) (property "ki_fp_filters" "SOIC*3.9x4.9mm*P1.27mm* DIP*W7.62mm*" (at 0 0 0)) (property "MPN" "LM358DR" (at 0 0 0))
    (symbol "LM358_1_1" (pin output line (at 7.62 0 180) (length 2.54) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
      (pin input line (at -7.62 -2.54 0) (length 2.54) (name "-") (number "2")) (pin input line (at -7.62 2.54 0) (length 2.54) (name "+") (number "3")))
    (symbol "LM358_2_1" (pin input line (at -7.62 2.54 0) (length 2.54) (name "+") (number "5")) (pin input line (at -7.62 -2.54 0) (length 2.54) (name "-") (number "6"))
      (pin output line (at 7.62 0 180) (length 2.54) (name "~") (number "7")))
    (symbol "LM358_3_1" (pin power_in line (at -2.54 -7.62 90) (length 3.81) (name "V-") (number "4")) (pin power_in line (at -2.54 7.62 270) (length 3.81) (name "V+") (number "8"))))
  (symbol "LM2904" (extends "LM358") (property "Value" "LM2904" (at 0 0 0)) (property "Description" "" (at 0 0 0)))
  (symbol "R" (property "Reference" "R" (at 0 0 0)) (property "Value" "R" (at 0 0 0)) (property "Description" "Resistor" (at 0 0 0))
    (property "ki_keywords" "R res resistor" (at 0 0 0)) (property "ki_fp_filters" "R_*" (at 0 0 0))
    (symbol "R_1_1" (pin passive line (at 0 3.81 270) (length 1.27) (name "~") (number "1")) (pin passive line (at 0 -3.81 90) (length 1.27) (name "~") (number "2")))))
"#;

    /// A board-less set of libraries: `Pkg.pretty` (SOIC-8, DIP-8, R_0603), `Lib.kicad_sym` and a folder library `Dir.kicad_symdir`.
    pub(crate) fn fixture(tag: &str) -> (PathBuf, Vec<Target>) {
        let dir = temp(tag);
        let pretty = dir.join("Pkg.pretty");
        std::fs::create_dir_all(&pretty).unwrap();
        std::fs::write(pretty.join("SOIC-8_3.9x4.9mm_P1.27mm.kicad_mod"), SOIC8).unwrap();
        std::fs::write(pretty.join("DIP-8_W7.62mm.kicad_mod"), dip8()).unwrap();
        std::fs::write(pretty.join("R_0603_1608Metric.kicad_mod"), R0603).unwrap();
        std::fs::write(dir.join("Lib.kicad_sym"), SYMS).unwrap();
        let symdir = dir.join("Dir.kicad_symdir");
        std::fs::create_dir_all(&symdir).unwrap();
        std::fs::write(symdir.join("Base.kicad_sym"), "(kicad_symbol_lib (version 20241209) (generator \"eda\")\n  (symbol \"Base\" (property \"Reference\" \"U\" (at 0 0 0)) (property \"Description\" \"the base\" (at 0 0 0))\n    (symbol \"Base_1_1\" (pin input line (at 0 0 0) (length 1) (name \"IN\") (number \"1\")) (pin output line (at 2 0 180) (length 1) (name \"OUT\") (number \"2\"))))\n)\n").unwrap();
        std::fs::write(symdir.join("Child.kicad_sym"), "(kicad_symbol_lib (version 20241209) (generator \"eda\")\n  (symbol \"Child\" (extends \"Base\") (property \"Value\" \"Child\" (at 0 0 0)))\n)\n").unwrap();
        let targets = vec![
            Target { kind: Kind::Footprint, path: pretty, nickname: "Pkg".into(), format: "kicad" },
            Target { kind: Kind::Symbol, path: dir.join("Lib.kicad_sym"), nickname: "Lib".into(), format: "kicad" },
            Target { kind: Kind::Symbol, path: symdir, nickname: "Dir".into(), format: "kicad" },
        ];
        (dir, targets)
    }

    fn models(dir: &Path) -> ModelCtx {
        ModelCtx::for_dir(dir)
    }

    #[test]
    fn a_new_database_has_the_tables_and_another_schema_is_rebuilt() {
        let dir = temp("schema");
        let db = dir.join("p.sqlite");
        let store = Store::open(&db).unwrap();
        for t in ["meta", "libraries", "files", "table_rows", "symbols", "symbol_pins", "footprints", "footprint_pads", "symbols_fts", "footprints_fts", "parts", "part_params", "part_links", "offers", "lease"] {
            let n: i64 = store.conn.query_row("SELECT count(*) FROM sqlite_master WHERE name = ?1", [t], |r| r.get(0)).unwrap();
            assert_eq!(n, 1, "{t}");
        }
        assert_eq!(schema::meta(&store.conn, "schema_version").as_deref(), Some("1"));
        let journal: String = store.conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(journal, "wal");
        store.conn.execute("INSERT INTO libraries (kind, path, stamp) VALUES ('symbol', '/x', '1')", []).unwrap();
        // another schema: rebuilt, empty
        schema::set_meta(&store.conn, "schema_version", "0").unwrap();
        drop(store);
        let store = Store::open(&db).unwrap();
        assert_eq!(store.libraries().len(), 0);
        assert_eq!(schema::meta(&store.conn, "schema_version").as_deref(), Some("1"));
        // another scanner: the rows stay, unread
        store.conn.execute("INSERT INTO libraries (kind, path, stamp, files_stamp) VALUES ('symbol', '/x', '1', 'f')", []).unwrap();
        schema::set_meta(&store.conn, "scanner_version", "0").unwrap();
        drop(store);
        let store = Store::open(&db).unwrap();
        let rows = store.libraries();
        assert_eq!((rows.len(), rows[0].stamp.clone(), rows[0].files_stamp.clone()), (1, None, None));
        // a read-only open of a database that is not there is an error, not a new file
        assert!(Store::open_read_only(&dir.join("none.sqlite")).is_err());
        assert!(!dir.join("none.sqlite").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_fixture_libraries_are_indexed_with_their_pins_pads_pitch_and_courtyards() {
        let (dir, targets) = fixture("index");
        let store = Store::open(&dir.join("p.sqlite")).unwrap();
        let r = refresh_targets(&store, &targets, false, &models(&dir)).unwrap();
        assert_eq!((r.libraries, r.read, r.removed), (3, 3, 0));
        let fp = |name: &str| -> (String, i64, i64, i64, i64, i64, i64) {
            store
                .conn
                .query_row(
                    "SELECT attr, numbered_pads, smd_pads, tht_pads, pitch_nm, courtyard_w_nm, courtyard_h_nm FROM footprints WHERE name = ?1",
                    [name],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
                )
                .unwrap()
        };
        assert_eq!(fp("SOIC-8_3.9x4.9mm_P1.27mm"), ("smd".into(), 8, 8, 0, 1_270_000, 7_400_000, 5_400_000));
        assert_eq!(fp("DIP-8_W7.62mm"), ("through_hole".into(), 8, 0, 8, 2_540_000, 9_800_000, 10_400_000));
        assert_eq!(fp("R_0603_1608Metric").1, 2);
        let pads: i64 = store.conn.query_row("SELECT count(*) FROM footprint_pads", [], |r| r.get(0)).unwrap();
        assert_eq!(pads, 18);
        let models: String = store.conn.query_row("SELECT model_paths_json FROM footprints WHERE name LIKE 'SOIC%'", [], |r| r.get(0)).unwrap();
        assert!(models.contains("Package_SO.3dshapes"), "{models}");
        // symbols: the derived one has its base's pins, in a file library and in a folder library
        let pins = |name: &str| -> i64 { store.conn.query_row("SELECT count(*) FROM symbol_pins p JOIN symbols s ON s.id = p.symbol_id WHERE s.name = ?1", [name], |r| r.get(0)).unwrap() };
        assert_eq!((pins("LM358"), pins("LM2904"), pins("R"), pins("Base"), pins("Child")), (8, 8, 2, 2, 2));
        let (desc, count, nums, fields): (String, i64, String, String) =
            store.conn.query_row("SELECT description, pin_count, pin_numbers, user_fields_json FROM symbols WHERE name = 'LM2904'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
        assert_eq!((desc.as_str(), count), ("Dual operational amplifier", 8), "a derived symbol's empty field is its base's");
        assert_eq!(nums, "1\n2\n3\n4\n5\n6\n7\n8");
        assert_eq!(fields, r#"[["MPN","LM358DR"]]"#);
        let child_file: String = store.conn.query_row("SELECT file FROM symbols WHERE name = 'Child'", [], |r| r.get(0)).unwrap();
        assert_eq!(child_file, "Child.kicad_sym");
        // the substring index and the wildcard function
        let hits: i64 = store.conn.query_row("SELECT count(*) FROM symbols_fts WHERE symbols_fts MATCH '\"opamp\"'", [], |r| r.get(0)).unwrap();
        assert_eq!(hits, 2, "LM358 and LM2904 (whose keywords are its base's)");
        let wild: i64 = store.conn.query_row("SELECT count(*) FROM footprints WHERE kicad_wildcard('soic*3.9x4.9mm*', name)", [], |r| r.get(0)).unwrap();
        assert_eq!(wild, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_touched_footprint_is_the_only_file_read_again_and_a_removed_one_goes() {
        let (dir, targets) = fixture("touch");
        let store = Store::open(&dir.join("p.sqlite")).unwrap();
        let m = models(&dir);
        refresh_targets(&store, &targets, false, &m).unwrap();
        let again = refresh_targets(&store, &targets, true, &m).unwrap();
        assert_eq!((again.read, again.files_read), (0, 0), "nothing changed: a deep look reads nothing");
        // an edit in place: the folder's time stays, the file's moves
        let pretty = dir.join("Pkg.pretty");
        let folder_time = std::fs::metadata(&pretty).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(pretty.join("R_0603_1608Metric.kicad_mod"), R0603.replace("(tags \"resistor\")", "(tags \"resistor edited\")")).unwrap();
        let _ = filetime_set(&pretty, folder_time);
        let shallow = refresh_targets(&store, &targets, false, &m).unwrap();
        assert_eq!(shallow.read, 0, "a shallow look sees the folder's time only");
        let deep = refresh_targets(&store, &targets, true, &m).unwrap();
        assert_eq!((deep.read, deep.files_read), (1, 1), "only the edited file is read");
        let tags: String = store.conn.query_row("SELECT tags FROM footprints WHERE name = 'R_0603_1608Metric'", [], |r| r.get(0)).unwrap();
        assert_eq!(tags, "resistor edited");
        let n: i64 = store.conn.query_row("SELECT count(*) FROM footprints", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 3, "the others are kept");
        // a file removed: its row goes
        std::fs::remove_file(pretty.join("DIP-8_W7.62mm.kicad_mod")).unwrap();
        let gone = refresh_targets(&store, &targets, false, &m).unwrap();
        assert_eq!((gone.read, gone.files_read), (1, 0));
        let names: Vec<String> = store.conn.prepare("SELECT name FROM footprints ORDER BY name").unwrap().query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
        assert_eq!(names, ["R_0603_1608Metric", "SOIC-8_3.9x4.9mm_P1.27mm"]);
        let pads: i64 = store.conn.query_row("SELECT count(*) FROM footprint_pads", [], |r| r.get(0)).unwrap();
        assert_eq!(pads, 10, "its pads went with it");
        let fts: i64 = store.conn.query_row("SELECT count(*) FROM footprints_fts", [], |r| r.get(0)).unwrap();
        assert_eq!(fts, 2);
        // a library whose folder is gone is taken out
        std::fs::remove_dir_all(&pretty).unwrap();
        let removed = refresh_targets(&store, &targets, false, &m).unwrap();
        assert_eq!(removed.removed, 1);
        assert!(store.library(Kind::Footprint, &pretty).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Set a folder's modification time back (`utimensat` through `touch -d`), so an edit in place looks like one.
    fn filetime_set(path: &Path, t: SystemTime) -> std::io::Result<()> {
        let secs = t.duration_since(UNIX_EPOCH).unwrap();
        let stamp = format!("@{}.{:09}", secs.as_secs(), secs.subsec_nanos());
        std::process::Command::new("touch").arg("-m").arg("-d").arg(stamp).arg(path).status().map(|_| ())
    }

    #[test]
    fn two_refreshers_build_once() {
        let (dir, targets) = fixture("two");
        let db = dir.join("p.sqlite");
        drop(Store::open(&db).unwrap());
        let reports: Vec<Report> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..2)
                .map(|_| {
                    let (db, targets, dir) = (db.clone(), targets.clone(), dir.clone());
                    s.spawn(move || {
                        let store = Store::open(&db).unwrap();
                        refresh_targets(&store, &targets, false, &ModelCtx::for_dir(&dir)).unwrap()
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert_eq!(reports.iter().map(|r| r.read).sum::<usize>(), 3, "the libraries are read once between them: {reports:?}");
        let store = Store::open(&db).unwrap();
        assert_eq!(store.libraries().len(), 3);
        assert!(store.lease_holder().is_none(), "the lease is given back");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_lease_left_by_a_dead_builder_is_taken_over() {
        let dir = temp("lease");
        let store = Store::open(&dir.join("p.sqlite")).unwrap();
        assert!(store.take_lease("a").unwrap());
        assert!(!store.take_lease("b").unwrap(), "held");
        assert!(store.take_lease("a").unwrap(), "its own");
        store.conn.execute("UPDATE lease SET heartbeat = heartbeat - 3600", []).unwrap();
        assert!(store.take_lease("b").unwrap(), "stale");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_known_verdict_is_imported_and_marks_the_refused_items() {
        let (dir, targets) = fixture("verdict");
        crate::library_verdict::tests::check_with_kicad_cli(&dir);
        let store = Store::open(&dir.join("p.sqlite")).unwrap();
        refresh_targets(&store, &targets, false, &models(&dir)).unwrap();
        // what kicad-cli would say, kept where the library verdicts are kept (none asked here: the test plants it)
        let pretty = &targets[0].path;
        let planted = crate::library_verdict::tests::plant(Kind::Footprint, pretty, &[pretty.join("DIP-8_W7.62mm.kicad_mod")]);
        if !planted {
            eprintln!("no kicad-cli: verdicts are off on this machine; skipping");
            return;
        }
        let (asked, answered) = verify_targets(&store, &targets[..1], false, |_| {});
        assert_eq!((asked, answered), (1, 1));
        let row = store.library(Kind::Footprint, pretty).unwrap();
        assert_eq!(row.verdict_state, "some_refused");
        let refused: Vec<String> = store.conn.prepare("SELECT name FROM footprints WHERE kicad_refused = 1").unwrap().query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
        assert_eq!(refused, ["DIP-8_W7.62mm"]);
        // a library read again keeps a verdict of its state, and a new state's verdict is unknown until asked
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(pretty.join("R_0603_1608Metric.kicad_mod"), R0603.replace("resistor", "res")).unwrap();
        refresh_targets(&store, &targets, true, &models(&dir)).unwrap();
        assert_eq!(store.library(Kind::Footprint, pretty).unwrap().verdict_state, "unknown");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
