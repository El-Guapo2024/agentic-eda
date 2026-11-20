//! Live libraries (PARITY-common.md section 34, a deliberate difference from KiCad): a library table or a library file of the board's tables changed on disk
//! by anyone -- an agent writing `sym-lib-table` and a `.kicad_sym` beside the board, the CLI, another tab -- and every open page drops what it kept of the
//! libraries within a second or two, the choosers and trees on screen asking again.
//!
//! KiCad reads the tables when the project is opened (`PROJECT::SymbolLibs`, `PROJECT_PCB::PcbFootprintLibs`, KiCad 25ec2335 `LIBRARY_MANAGER::LoadProjectTables`)
//! and again only through Manage Libraries (`DIALOG_EDIT_LIBRARY_TABLES` -> `LIBRARY_MANAGER::ReloadTables`); a table written by something else is not seen
//! until the project is opened again. Its library editors watch the one library of the item they show (`SCH_BASE_FRAME::setSymWatcher`,
//! `PCB_BASE_FRAME::setFPWatcher`: `crate::library_watch`), which stays as it is: an item being edited is never replaced without asking.
//!
//! The studio's server watches by polling, as `crate::library_watch` does (no file-notification crate is a dependency). A thread of its own (started by the
//! first [`stamp`] of a project, ended when no page has asked for half a minute) takes a [`Snapshot`] of the board's libraries every [`PERIOD`]: each
//! table row as the tables resolve it (`library_tables::load(..).libs()`: nickname, path, type, options, description, scope, flags, found or not), the
//! library's shallow stamp (one stat: a symbol library's time, a `.kicad_symdir` folder's latest of its files, a `.pretty` folder's own time -- what the
//! choosers' caches compare, `crate::parts::refresh::shallow_stamp`) and, for a footprint library, the deep stamp of its files (`parts::refresh::list_files`,
//! one stat per `.kicad_mod`: a footprint written in place leaves the folder's time as it was). The project's own footprint libraries are looked at file by
//! file on every round; KiCad's installed ones (155 folders, 15,000 files) [`DEEP_GLOBAL_PER_ROUND`] at a time, in turn. A round that differs from the last
//! moves the stamp on, and the libraries that differ are given to the server's own caches: the parts cache's indexer reads them again
//! (`crate::parts::invalidate`) and the choosers' indexes of them are dropped (`library_search::forget`).
//!
//! `GET /api/version` carries the stamp in its `X-Library-Stamp` header (`studio::handle`): the page's poll reads it with the design's version, and a stamp
//! other than the last one it saw is "the libraries changed" (`web/studio/src/api/libraryTablesClient.ts` `librariesChanged`). The request pays a lock and a
//! string copy; the stat work is the thread's.

use crate::library_index::Kind;
use crate::parts::refresh::{files_stamp, list_files, shallow_stamp};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How often the thread looks: the pages poll every 700 ms, so a change is on every page within two seconds.
pub(crate) const PERIOD: Duration = Duration::from_millis(1000);
/// The thread ends when no page has asked for this long (the next ask starts it again, from the snapshot it kept).
const IDLE: Duration = Duration::from_secs(30);
/// How many of KiCad's installed footprint libraries are looked at file by file each round.
pub(crate) const DEEP_GLOBAL_PER_ROUND: usize = 16;

/// What one library is, as one round saw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LibState {
    kind: Kind,
    pub(crate) path: PathBuf,
    /// Its table row as resolved (`lt::Lib`'s fields but the table file).
    row: String,
    shallow: Option<String>,
    /// The footprint files' stamp (`files_stamp`), when it was looked at; a symbol library has none (its shallow stamp covers its files).
    deep: Option<String>,
    project: bool,
}

/// The board's libraries of both kinds, by kind (`parts::kind_name`) and nickname.
pub(crate) type Snapshot = BTreeMap<(&'static str, String), LibState>;

/// One round: the tables read again, every library stat'ed, the project's footprint libraries and `deep_global` of KiCad's (by their place in the
/// snapshot's order, from `cursor`) listed file by file; a global library not listed this round keeps the deep stamp `prev` had for it.
pub(crate) fn snapshot(dir: &Path, prev: Option<&Snapshot>, cursor: &mut usize, deep_global: usize) -> Snapshot {
    let mut out = Snapshot::new();
    let mut globals: Vec<(&'static str, String)> = Vec::new();
    for kind in [Kind::Symbol, Kind::Footprint] {
        for lib in crate::library_tables::load(dir, kind.table_kind()).libs() {
            let project = lib.scope == eda_kicad::library_table::Scope::Project;
            let row = format!("{}|{}|{}|{}|{}|{}|{}|{}|{:?}", lib.uri, lib.path.display(), lib.lib_type, lib.options, lib.description, lib.scope.name(), lib.disabled, lib.hidden, lib.error);
            let shallow = shallow_stamp(kind, &lib.path);
            let key = (crate::parts::kind_name(kind), lib.nickname.clone());
            let deep = match (kind, project) {
                (Kind::Footprint, true) => Some(files_stamp(&list_files(kind, &lib.path))),
                (Kind::Footprint, false) => {
                    globals.push(key.clone());
                    // the last look at it, while it is the same library
                    prev.and_then(|p| p.get(&key)).filter(|s| s.path == lib.path).and_then(|s| s.deep.clone())
                }
                (Kind::Symbol, _) => None,
            };
            out.insert(key, LibState { kind, path: lib.path, row, shallow, deep, project });
        }
    }
    if !globals.is_empty() {
        for i in 0..deep_global.min(globals.len()) {
            let key = &globals[(*cursor + i) % globals.len()];
            if let Some(s) = out.get_mut(key) {
                s.deep = Some(files_stamp(&list_files(s.kind, &s.path)));
            }
        }
        *cursor = (*cursor + deep_global) % globals.len();
    }
    out
}

/// The libraries that are not what they were: added, removed, another row, another stamp. A deep stamp only one of the two rounds looked at is no change.
pub(crate) fn changed(old: &Snapshot, new: &Snapshot) -> Vec<(Kind, PathBuf)> {
    let mut out = Vec::new();
    for (key, n) in new {
        match old.get(key) {
            None => out.push((n.kind, n.path.clone())),
            Some(o) => {
                let deep_moved = matches!((&o.deep, &n.deep), (Some(a), Some(b)) if a != b);
                if o.row != n.row || o.shallow != n.shallow || deep_moved {
                    out.push((n.kind, n.path.clone()));
                    if o.path != n.path {
                        out.push((o.kind, o.path.clone()));
                    }
                }
            }
        }
    }
    for (key, o) in old {
        if !new.contains_key(key) {
            out.push((o.kind, o.path.clone()));
        }
    }
    out
}

#[derive(Default)]
struct Watch {
    dir: Option<PathBuf>,
    /// Which project this is of the process's life (a new process, or another project, is another stamp whatever the count).
    epoch: u128,
    /// How many rounds found a change since the project was first asked about.
    generation: u64,
    snapshot: Option<Snapshot>,
    cursor: usize,
    last_asked: Option<Instant>,
    running: bool,
}

fn watch() -> &'static Mutex<Watch> {
    static W: OnceLock<Mutex<Watch>> = OnceLock::new();
    W.get_or_init(Default::default)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The stamp of the libraries the board in `dir` sees: another one whenever a table or a library of them changed since the last. Answered at once from
/// what the watch thread found (started here, see the module doc); the first ask of a project starts its watch.
pub(crate) fn stamp(dir: &Path) -> String {
    let mut w = lock(watch());
    if w.dir.as_deref() != Some(dir) {
        let epoch = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        // (the thread, if one runs, goes on with the new project)
        *w = Watch { dir: Some(dir.to_path_buf()), epoch, running: w.running, ..Default::default() };
    }
    w.last_asked = Some(Instant::now());
    if !w.running && !cfg!(test) {
        w.running = std::thread::Builder::new().name("library-live".into()).spawn(run).is_ok();
    }
    format!("{:x}.{}", w.epoch, w.generation)
}

/// One round of the watch on the project asked about last: a change moves the stamp on and tells the server's caches. What [`run`] does each [`PERIOD`].
pub(crate) fn round() {
    let (dir, prev, mut cursor) = {
        let mut w = lock(watch());
        let Some(dir) = w.dir.clone() else { return };
        (dir, w.snapshot.take(), w.cursor)
    };
    let next = snapshot(&dir, prev.as_ref(), &mut cursor, DEEP_GLOBAL_PER_ROUND);
    let moved = prev.as_ref().map(|p| changed(p, &next)).unwrap_or_default();
    for (kind, path) in &moved {
        // the parts cache reads it again, and the choosers' index of it is read again at its next ask (a footprint edited in place leaves the folder's time)
        crate::parts::invalidate(*kind, path);
        crate::library_search::forget(*kind, path);
    }
    let mut w = lock(watch());
    if w.dir.as_deref() != Some(dir.as_path()) {
        return; // another project was opened meanwhile: its own first round makes its baseline
    }
    if !moved.is_empty() {
        w.generation += 1;
    }
    w.snapshot = Some(next);
    w.cursor = cursor;
}

fn run() {
    loop {
        std::thread::sleep(PERIOD);
        {
            let mut w = lock(watch());
            if w.last_asked.is_none_or(|t| t.elapsed() > IDLE) {
                w.running = false;
                return;
            }
        }
        round();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::tests::{scratch, setup};
    use crate::library_tables;

    fn take(dir: &Path, prev: Option<&Snapshot>) -> Snapshot {
        let mut cursor = 0;
        snapshot(dir, prev, &mut cursor, DEEP_GLOBAL_PER_ROUND)
    }

    #[test]
    fn the_snapshot_moves_with_a_table_a_library_and_a_footprint_written_in_place_and_not_otherwise() {
        let dir = scratch("library_live");
        setup(&dir);
        let t0 = Instant::now();
        let first = take(&dir, None);
        let cold = t0.elapsed();
        // nothing changed: the same
        let t1 = Instant::now();
        let again = take(&dir, Some(&first));
        let warm = t1.elapsed();
        assert!(changed(&first, &again).is_empty(), "{:?}", changed(&first, &again));
        // cheap: the tables read and every library stat'ed, a few of KiCad's footprint folders listed (the installed ones, when this machine has KiCad's)
        eprintln!("library_live: {} libraries, first round {cold:?}, next {warm:?}", first.len());
        assert!(warm < Duration::from_millis(500), "a round took {warm:?}");

        // a project table with a symbol library added (an agent writing the files beside the board)
        std::fs::create_dir_all(dir.join("libs")).unwrap();
        std::fs::write(dir.join("libs").join("Live.kicad_sym"), "(kicad_symbol_lib (version 20251024)\n\t(symbol \"A\" (property \"Reference\" \"U\") (symbol \"A_1_1\"))\n)\n").unwrap();
        std::fs::write(dir.join("sym-lib-table"), "(sym_lib_table\n  (version 7)\n  (lib (name \"Live\")(type \"KiCad\")(uri \"${KIPRJMOD}/libs/Live.kicad_sym\")(options \"\")(descr \"\"))\n)\n").unwrap();
        let added = take(&dir, Some(&again));
        let moved = changed(&again, &added);
        assert_eq!(moved, vec![(Kind::Symbol, dir.join("libs").join("Live.kicad_sym"))], "the new library alone");
        // the library file edited
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(dir.join("libs").join("Live.kicad_sym"), "(kicad_symbol_lib (version 20251024)\n\t(symbol \"A\" (property \"Reference\" \"Q\") (symbol \"A_1_1\"))\n)\n").unwrap();
        let edited = take(&dir, Some(&added));
        assert_eq!(changed(&added, &edited).len(), 1, "the edited library");

        // a footprint library through the tables' own route, then a footprint added and one written in place
        let pretty = library_tables::add_library(&dir, br#"{"kind":"footprint","path":"${KIPRJMOD}/libs/LiveFp.pretty","create":true}"#);
        assert_eq!(pretty["ok"], true, "{pretty}");
        let with_fp = take(&dir, Some(&edited));
        assert!(changed(&edited, &with_fp).iter().any(|(k, p)| *k == Kind::Footprint && p.ends_with("LiveFp.pretty")));
        let folder = dir.join("libs").join("LiveFp.pretty");
        std::fs::write(folder.join("X.kicad_mod"), "(footprint \"X\" (layer \"F.Cu\") (descr \"one\"))\n").unwrap();
        let one = take(&dir, Some(&with_fp));
        assert_eq!(changed(&with_fp, &one), vec![(Kind::Footprint, folder.clone())], "a footprint added");
        // in place: the folder's time stays, its file's moves
        let folder_time = std::fs::metadata(&folder).unwrap().modified().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(folder.join("X.kicad_mod"), "(footprint \"X\" (layer \"F.Cu\") (descr \"two, longer\"))\n").unwrap();
        assert_eq!(std::fs::metadata(&folder).unwrap().modified().unwrap(), folder_time, "the folder's own time did not move");
        let two = take(&dir, Some(&one));
        assert_eq!(changed(&one, &two), vec![(Kind::Footprint, folder.clone())], "a footprint written in place");
        assert!(changed(&two, &take(&dir, Some(&two))).is_empty(), "and then nothing");

        // the row out of the table: the library is gone
        std::fs::write(dir.join("sym-lib-table"), "(sym_lib_table\n  (version 7)\n)\n").unwrap();
        let gone = take(&dir, Some(&two));
        assert_eq!(changed(&two, &gone), vec![(Kind::Symbol, dir.join("libs").join("Live.kicad_sym"))]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// KiCad's installed footprint libraries are looked at a few per round, in turn: a round that did not list a library keeps what the last one found,
    /// and that is no change.
    #[test]
    fn a_global_footprint_library_not_listed_this_round_is_no_change() {
        let (a, b) = (PathBuf::from("/x/A.pretty"), PathBuf::from("/x/B.pretty"));
        let st = |path: &Path, deep: Option<&str>| LibState { kind: Kind::Footprint, path: path.to_path_buf(), row: "r".into(), shallow: Some("1".into()), deep: deep.map(String::from), project: false };
        let old: Snapshot = [(("footprint", "A".to_string()), st(&a, Some("d1"))), (("footprint", "B".to_string()), st(&b, None))].into();
        let new: Snapshot = [(("footprint", "A".to_string()), st(&a, None)), (("footprint", "B".to_string()), st(&b, Some("d2")))].into();
        assert!(changed(&old, &new).is_empty());
        let moved: Snapshot = [(("footprint", "A".to_string()), st(&a, Some("d9"))), (("footprint", "B".to_string()), st(&b, Some("d2")))].into();
        assert_eq!(changed(&new, &moved), Vec::<(Kind, PathBuf)>::new(), "A was not looked at in `new`");
        assert_eq!(changed(&old, &moved), vec![(Kind::Footprint, a.clone())]);
    }

    #[test]
    fn the_stamp_moves_on_when_a_round_finds_a_change() {
        let dir = scratch("library_live_stamp");
        setup(&dir);
        let s0 = stamp(&dir);
        round();
        assert_eq!(stamp(&dir), s0, "the first round is the baseline");
        round();
        assert_eq!(stamp(&dir), s0, "nothing changed");
        let made = library_tables::add_library(&dir, br#"{"kind":"footprint","path":"${KIPRJMOD}/libs/S.pretty","create":true}"#);
        assert_eq!(made["ok"], true, "{made}");
        round();
        let s1 = stamp(&dir);
        assert_ne!(s1, s0);
        // per poll: a lock and a copy
        let t = Instant::now();
        for _ in 0..1000 {
            let _ = stamp(&dir);
        }
        let per = t.elapsed() / 1000;
        eprintln!("library_live: stamp() {per:?} per poll");
        assert!(per < Duration::from_millis(1), "{per:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
