//! The studio's side of the parts cache: the indexer thread `eda board serve` starts, and `GET /api/parts/*`.
//!
//! The routes are answered on the connection's own thread (`studio::serve`'s accept closure, like `/api/3dmodel`), each on a read-only connection of its own:
//! they never wait behind the request loop, and the loop never waits for them. A library whose stamp moved is answered from what the cache has and named in
//! `stale`, and queued for the indexer. Nothing here runs kicad-cli: the indexer thread does, one library at a time, for the verdicts it does not know.
//!
//!   * `GET /api/parts/status`: the cache, the indexer's state, the board's stale libraries;
//!   * `GET /api/parts/search?kind=&q=&lib=&pins=&smd=1&tht=1&pitch=&has3d=1&fp_filter=&limit=`;
//!   * `GET /api/parts/show?kind=&id=Lib:Name&definition=1`;
//!   * `GET /api/parts/fits?symbol=Lib:Name&footprint=Lib:Name`;
//!   * `GET /api/parts/stats?by_lib=1`;
//!   * `GET /api/parts/check`: the board's parts (`eda parts check-board`);
//!   * `POST /api/parts/refresh`: every library of the board looked at again, file by file, by the indexer (answered at once: `{"queued": n}`).

use super::query::{self, SearchOpts};
use super::refresh::{self, ModelCtx};
use super::store::Store;
use crate::library_index::Kind;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How often the indexer looks at every file of the board's libraries when nothing asked it to (a footprint edited in place by another program).
pub(crate) const DEEP_EVERY: Duration = Duration::from_secs(300);

/// What the indexer thread is doing, for `status`.
#[derive(Default, Clone)]
struct IndexerState {
    phase: &'static str,
    last: Option<Value>,
    error: Option<String>,
    verdicts_asked: usize,
}

fn indexer_state() -> &'static Mutex<IndexerState> {
    static S: OnceLock<Mutex<IndexerState>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(IndexerState { phase: "off", ..Default::default() }))
}

fn set_phase(phase: &'static str) {
    super::lock(indexer_state()).phase = phase;
}

/// Start the indexer for the board in `dir` (`eda board serve`): a deep refresh of the board's libraries at once, then KiCad's verdicts it does not know
/// (kicad-cli, one library at a time), then it waits -- for [`super::invalidate`] (a save, a table written, a query that met a stale library) or five minutes
/// -- and refreshes again. `EDA_PARTS_INDEXER=off` leaves it out.
pub(crate) fn start_indexer(dir: &Path) {
    if std::env::var("EDA_PARTS_INDEXER").is_ok_and(|v| v == "off") {
        return;
    }
    // a workspace's project switch (`studio::serve`): the indexer of the project closed ends first (`stop_indexer`), this one waits for it
    let generation = GENERATION.fetch_add(1, AtomicOrdering::SeqCst) + 1;
    let dir = dir.to_path_buf();
    let _ = std::thread::Builder::new().name("parts-indexer".into()).spawn(move || {
        let _one = super::lock(running());
        if GENERATION.load(AtomicOrdering::SeqCst) != generation {
            return;
        }
        *super::lock(indexer_state()) = IndexerState { phase: "off", ..Default::default() };
        indexer(&dir, generation)
    });
}

/// Which indexer is the current one: [`start_indexer`] and [`stop_indexer`] move it on, and an indexer that is not current ends at its next look.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Held by the indexer thread that runs, so the next project's indexer starts once the last one has ended (one writer of the cache).
fn running() -> &'static Mutex<()> {
    static R: OnceLock<Mutex<()>> = OnceLock::new();
    R.get_or_init(Default::default)
}

/// The project is closed (a workspace opens another, `studio::serve`): its indexer ends when its present step is done, and what was queued for its
/// libraries is dropped. The status reads "off" until the next project's indexer starts.
pub(crate) fn stop_indexer() {
    GENERATION.fetch_add(1, AtomicOrdering::SeqCst);
    let (q, cv) = super::queue();
    let mut q = super::lock(q);
    q.stale.clear();
    q.verdicts.clear();
    cv.notify_all();
    drop(q);
    *super::lock(indexer_state()) = IndexerState { phase: "off", ..Default::default() };
}

fn indexer(dir: &Path, generation: u64) {
    let current = || GENERATION.load(AtomicOrdering::SeqCst) == generation;
    let db = super::db_path();
    let store = match Store::open(&db) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("parts: no parts cache: {e}");
            let mut st = super::lock(indexer_state());
            st.phase = "failed";
            st.error = Some(e);
            return;
        }
    };
    let kinds = [Kind::Symbol, Kind::Footprint];
    let mut deep = true;
    let mut last_deep = Instant::now();
    let mut extra_deep: Vec<PathBuf> = Vec::new();
    loop {
        if !current() {
            return;
        }
        set_phase("building");
        let targets = refresh::board_targets(dir, &kinds);
        let models = ModelCtx::for_dir(dir);
        let result = refresh::refresh_targets(&store, &targets, deep, &models).and_then(|r| {
            // a library a save named is looked at file by file (a footprint saved in place leaves its folder's time)
            let named: Vec<_> = targets.iter().filter(|t| extra_deep.contains(&t.path)).cloned().collect();
            if named.is_empty() || deep {
                Ok(r)
            } else {
                refresh::refresh_targets(&store, &named, true, &models).map(|more| refresh::Report { read: r.read + more.read, files_read: r.files_read + more.files_read, ..r })
            }
        });
        refresh::write_table_rows(&store, dir, &kinds);
        {
            let mut st = super::lock(indexer_state());
            match result {
                Ok(r) => {
                    st.last = Some(r.json());
                    st.error = None;
                }
                Err(e) => {
                    eprintln!("parts: refresh failed: {e}");
                    st.error = Some(e);
                }
            }
        }
        if deep {
            last_deep = Instant::now();
        }
        // KiCad's verdicts, where they are not known: the ones queries asked for first, then the rest
        set_phase("verifying");
        let wanted: Vec<(Kind, PathBuf)> = std::mem::take(&mut super::lock(&super::queue().0).verdicts);
        let mut order: Vec<_> = targets.iter().filter(|t| wanted.iter().any(|(k, p)| *k == t.kind && *p == t.path)).cloned().collect();
        order.extend(targets.iter().filter(|t| !wanted.iter().any(|(k, p)| *k == t.kind && *p == t.path)).cloned());
        if !current() {
            return;
        }
        let (asked, _) = refresh::verify_targets(&store, &order, true, |_| {});
        if !current() {
            return;
        }
        super::lock(indexer_state()).verdicts_asked += asked;
        set_phase("ready");
        // wait for something to do
        let (q, cv) = super::queue();
        let mut guard = super::lock(q);
        loop {
            if !current() {
                return;
            }
            if !guard.stale.is_empty() || !guard.verdicts.is_empty() {
                break;
            }
            let left = DEEP_EVERY.saturating_sub(last_deep.elapsed());
            if left.is_zero() {
                break;
            }
            guard = cv.wait_timeout(guard, left).map(|(g, _)| g).unwrap_or_else(|e| e.into_inner().0);
        }
        extra_deep = guard.stale.drain(..).map(|(_, p)| p).collect();
        drop(guard);
        deep = last_deep.elapsed() >= DEEP_EVERY;
    }
}

fn param(target: &str, key: &str) -> String {
    let query = target.split_once('?').map_or("", |(_, q)| q);
    query.split('&').find_map(|kv| kv.strip_prefix(key).and_then(|v| v.strip_prefix('='))).and_then(crate::model3d_api::percent_decode).unwrap_or_default()
}

fn flag(target: &str, key: &str) -> bool {
    matches!(param(target, key).as_str(), "1" | "true" | "yes")
}

/// Queue the board's libraries `answer["stale"]` names (by nickname) for the indexer.
fn queue_stale(dir: &Path, kind: Kind, answer: &Value) {
    let Some(names) = answer["stale"].as_array().filter(|a| !a.is_empty()) else { return };
    let set = crate::library_tables::libs(dir, kind);
    for n in names.iter().filter_map(Value::as_str) {
        // `status` names them `kind:nickname`
        let (k, nick) = match n.split_once(':') {
            Some((k, nick)) if Kind::parse(k).is_some() => (Kind::parse(k).expect("checked"), nick),
            _ => (kind, n),
        };
        if k != kind {
            continue;
        }
        if let Some(e) = set.find(nick) {
            super::invalidate(kind, &e.path);
        }
    }
}

/// The answer to `GET /api/parts/<what>` (or `POST /api/parts/refresh`) for the board in `dir`; `None` for a path that is not one of them.
pub(crate) fn answer(dir: &Path, method: &str, target: &str) -> Option<Value> {
    let path = target.split('?').next().unwrap_or("");
    let what = path.strip_prefix("/api/parts/")?;
    if method == "POST" {
        if what != "refresh" {
            return Some(json!({ "error": format!("no route POST {path}") }));
        }
        let targets = refresh::board_targets(dir, &[Kind::Symbol, Kind::Footprint]);
        for t in &targets {
            super::invalidate(t.kind, &t.path);
        }
        return Some(json!({ "queued": targets.len() }));
    }
    let kind = Kind::parse(&param(target, "kind")).unwrap_or(Kind::Symbol);
    let store = match Store::open_read_only(&super::db_path()) {
        Ok(s) => s,
        Err(e) => {
            let st = super::lock(indexer_state()).clone();
            return Some(json!({ "state": "building", "indexer": st.phase, "error": e }));
        }
    };
    let sets = || (crate::library_tables::libs(dir, Kind::Symbol), crate::library_tables::libs(dir, Kind::Footprint));
    let v = match what {
        "status" => {
            let (s, f) = sets();
            let mut v = query::status(&store, &*s, &*f);
            let st = super::lock(indexer_state()).clone();
            v["indexer"] = json!({ "phase": st.phase, "last": st.last, "error": st.error, "verdicts_asked": st.verdicts_asked });
            if v["state"] == "ready" && st.phase == "building" {
                v["state"] = json!("building");
            }
            queue_stale(dir, Kind::Symbol, &v);
            queue_stale(dir, Kind::Footprint, &v);
            v
        }
        "search" => {
            let set = crate::library_tables::libs(dir, kind);
            let opts = SearchOpts {
                query: param(target, "q"),
                lib: Some(param(target, "lib")).filter(|l| !l.is_empty()),
                pins: param(target, "pins").parse().ok(),
                smd: flag(target, "smd"),
                tht: flag(target, "tht"),
                pitch_mm: param(target, "pitch").parse().ok(),
                has_3d: flag(target, "has3d"),
                fp_filters: param(target, "fp_filter").split_whitespace().map(str::to_string).collect(),
                limit: param(target, "limit").parse::<usize>().ok().filter(|&n| n > 0).map_or(50, |n| n.min(2000)),
            };
            let v = query::search(&store, &*set, kind, &opts);
            queue_stale(dir, kind, &v);
            v
        }
        "show" => {
            let set = crate::library_tables::libs(dir, kind);
            query::show(&store, &*set, kind, &param(target, "id"), flag(target, "definition"))
        }
        "fits" => {
            let (s, f) = sets();
            let footprint = param(target, "footprint");
            let v = query::fits(&store, &*s, &*f, &param(target, "symbol"), Some(footprint.as_str()).filter(|f| !f.is_empty()));
            queue_stale(dir, Kind::Footprint, &v);
            v
        }
        "stats" => query::stats(&store, flag(target, "by_lib")),
        "check" => {
            let (s, f) = sets();
            query::check_board(&store, dir, &*s, &*f)
        }
        _ => json!({ "error": format!("no route {path}") }),
    };
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_query_values_are_decoded() {
        let t = "/api/parts/search?kind=footprint&q=soic%208&smd=1&pitch=1.27&fp_filter=SOIC*%20DIP*";
        assert_eq!((param(t, "kind"), param(t, "q"), param(t, "pitch"), param(t, "fp_filter")), ("footprint".into(), "soic 8".into(), "1.27".into(), "SOIC* DIP*".into()));
        assert!(flag(t, "smd") && !flag(t, "tht"));
        assert_eq!(param(t, "nope"), "");
        assert!(answer(Path::new("/nonexistent"), "GET", "/api/elsewhere").is_none());
    }
}
