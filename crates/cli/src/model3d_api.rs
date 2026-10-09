//! The 3D models of the board's parts, for the browser's 3D view (`web/studio/src/components/viewer3d`).
//!
//! The view used to wait for `kicad-cli pcb export glb` -- the whole board, every model through OpenCascade, seconds to minutes -- before it showed one real
//! model. It now loads each footprint's KiCad 3D model by itself, with three.js's own `VRMLLoader`, and places it the way KiCad does. This module is the
//! server's half of that:
//!
//!   - **`GET /api/3dmodel?name=<model path>`** serves one model, read-only. `name` is the path a footprint's `(model "...")` line writes
//!     (`${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.step`, a legacy `${KISYS3DMOD}/...wrl`, a project-relative one), percent-encoded. It is
//!     resolved the way KiCad's `FILENAME_RESOLVER` does (see [`resolve`]) and then held to an allow-list: the file must lie, once symlinks are resolved,
//!     under KiCad's 3D model directory or the board's own directory, and be a `.wrl`, `.step` or `.stp`. Nothing else is readable through this route.
//!   - **STEP models** are what KiCad's installed library holds (7238 `.step`, no `.wrl`), and the browser has no STEP reader. The first request for one
//!     converts it with `kicad-cli pcb export vrml --models-dir` (`eda_kicad_engine::convert_models_to_vrml`: KiCad's own 3D cache reads the STEP and
//!     writes the VRML KiCad's libraries are written in) and keeps the result in a cache directory, keyed by the file's path, size and modification time,
//!     so a model is converted once, not once per board or per session. The route answers `202 {"status":"pending"}` while it runs and the browser asks
//!     again; the request loop never waits for kicad-cli. Requests that arrive together (one per distinct model of the board) are converted in one
//!     kicad-cli run: a run costs a second or two before it loads anything.
//!   - **The models of each part** go out in `/api/state` (`parts[].models`, `parts[].kind3d`), from [`part_json`].

use crate::kicad_lane::Lane;
use eda_kicad::part_models;
use eda_model::{ConstraintModel, Footprint, Part};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

// ----------------------------------------------------------------------- where the models are

/// Where KiCad's installed 3D model library lives, in precedence order: `EDA_KICAD_3DMODELS_DIR`, then the macOS bundle -- the directory
/// `${KICAD10_3DMODEL_DIR}` stands for.
pub fn models_dir() -> PathBuf {
    if let Ok(p) = std::env::var("EDA_KICAD_3DMODELS_DIR") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from("/Applications/KiCad/KiCad.app/Contents/SharedSupport/3dmodels")
}

/// The two directories a model may be read from: KiCad's 3D model library and the board's own directory (`${KIPRJMOD}`, and what a relative path is relative to).
#[derive(Debug, Clone)]
pub struct Roots {
    pub models: PathBuf,
    pub project: PathBuf,
}

/// Why a model name does not name a readable model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reject {
    /// No name, a NUL in it, or a name too long to be a path.
    BadName,
    /// No such file (after KiCad's own search: the name as given, relative to the project, relative to the model library, a STEP in place of a missing VRML).
    NotFound,
    /// A file, but not one this route may serve: outside the two directories, or not a model file.
    Forbidden(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Vrml,
    Step,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Canonical: symlinks resolved, inside one of the [`Roots`].
    pub path: PathBuf,
    pub format: Format,
}

/// KiCad's variable expansion for a model path (`KIwxExpandEnvVars`, `common/common.cpp`): `${NAME}` and `$(NAME)`. A name resolves, in order, to the board's
/// directory (`KIPRJMOD`), to the process environment, and -- for `KISYS3DMOD` and any `KICAD<n>_3DMODEL_DIR`, however old -- to the model library of the KiCad
/// that is installed ("replace unmatched older variables with current locations"). Anything else is left as written, and then names no file.
fn expand(name: &str, roots: &Roots, env: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(name.len());
    let mut rest = name;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let (open, close) = match after.chars().next() {
            Some('{') => ('{', '}'),
            Some('(') => ('(', ')'),
            _ => {
                out.push('$');
                rest = after;
                continue;
            }
        };
        let Some(end) = after.find(close) else {
            out.push_str(&rest[at..]);
            return out;
        };
        let var = &after[open.len_utf8()..end];
        let value = if var == "KIPRJMOD" {
            Some(roots.project.to_string_lossy().into_owned())
        } else if let Some(v) = env(var).filter(|v| !v.is_empty()) {
            Some(v)
        } else if var == "KISYS3DMOD" || (var.starts_with("KICAD") && var.ends_with("_3DMODEL_DIR")) {
            Some(roots.models.to_string_lossy().into_owned())
        } else {
            None
        };
        match value {
            Some(v) => out.push_str(&v),
            None => out.push_str(&rest[at..at + 1 + end + 1]),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Lower case, `-` and ` ` turned into `_`, the extension (any 3D one) taken off: how KiCad matches a missing VRML to a STEP of the same part
/// (`MODEL_SUBSTITUTION::normalizeStem`).
fn normalized_stem(file_name: &str) -> String {
    let lower = file_name.to_lowercase();
    let stem = [".step.gz", ".stp.gz", ".wrl", ".wrz", ".step", ".stp", ".stpz", ".iges", ".igs"].iter().find_map(|ext| lower.strip_suffix(ext)).unwrap_or(&lower);
    stem.replace(['-', ' '], "_")
}

/// The STEP that stands in for a VRML model that is not there (`kicad-cli ... --subst-models`, `MODEL_SUBSTITUTION`): the same name with a STEP extension
/// in the same directory, else the file there whose normalized name is the same.
fn step_for_missing_wrl(wrl: &Path) -> Option<PathBuf> {
    let ext = wrl.extension()?.to_str()?.to_ascii_lowercase();
    if ext != "wrl" && ext != "wrz" {
        return None;
    }
    for step_ext in ["step", "stp"] {
        let sibling = wrl.with_extension(step_ext);
        if sibling.is_file() {
            return Some(sibling);
        }
    }
    let want = normalized_stem(wrl.file_name()?.to_str()?);
    std::fs::read_dir(wrl.parent()?).ok()?.flatten().map(|e| e.path()).find(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "step" | "stp")) && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| normalized_stem(n) == want))
}

fn format_of(path: &Path) -> Option<Format> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "wrl" => Some(Format::Vrml),
        "step" | "stp" => Some(Format::Step),
        _ => None,
    }
}

/// The file a footprint's `(model "name")` names, found the way KiCad's `FILENAME_RESOLVER::ResolvePath` finds it and then held to the allow-list.
///
/// KiCad's search: the name with its variables expanded as it stands (an absolute path), relative to the project's directory, relative to the model library
/// ("the legacy partial path": `Resistor_SMD.3dshapes/R_0603_1608Metric.wrl`); a `.wrl` that is not there is a `.step` in the same place.
///
/// The allow-list is the part KiCad does not have, because KiCad reads files for the person who owns them and this route answers a browser: after the
/// search, the file is made canonical (every symlink and `..` resolved) and must lie under the model library or the board's directory, and be a model
/// file (`.wrl`, `.step`, `.stp`). A name that finds a file anywhere else is [`Reject::Forbidden`], whatever it holds.
pub fn resolve(name: &str, roots: &Roots, env: &dyn Fn(&str) -> Option<String>) -> Result<Resolved, Reject> {
    if name.is_empty() || name.len() > 4096 || name.contains('\0') {
        return Err(Reject::BadName);
    }
    let name = name.replace('\\', "/");
    let expanded = expand(&name, roots, env);
    if expanded.contains("${") || expanded.contains("$(") {
        return Err(Reject::NotFound);
    }
    let given = PathBuf::from(&expanded);
    let candidates: Vec<PathBuf> = if given.is_absolute() { vec![given] } else { vec![roots.project.join(&given), roots.models.join(&given)] };
    let found = candidates.into_iter().find_map(|c| if c.is_file() { Some(c) } else { step_for_missing_wrl(&c) }).ok_or(Reject::NotFound)?;

    let canonical = found.canonicalize().map_err(|_| Reject::NotFound)?;
    let inside = [&roots.models, &roots.project].iter().filter_map(|r| r.canonicalize().ok()).any(|r| canonical.starts_with(&r));
    if !inside {
        return Err(Reject::Forbidden("the file is outside KiCad's 3D model directory and the board's directory".into()));
    }
    match format_of(&canonical) {
        Some(format) => Ok(Resolved { path: canonical, format }),
        None => Err(Reject::Forbidden("only .wrl, .step and .stp files are served".into())),
    }
}

/// The text of a query value with its `%XX` escapes decoded (`encodeURIComponent`'s: `+` is a plus, not a space); `None` for a `%` not followed by two hex
/// digits or for bytes that are not UTF-8.
pub fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

// ------------------------------------------------------------------- converting STEP to VRML

/// What asking for a STEP model's VRML found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// The converted file, in the cache.
    Ready(PathBuf),
    /// Queued or being converted: ask again shortly.
    Pending,
    /// kicad-cli could not convert it (or could not run); kept, so a model that reliably fails does not start kicad-cli on every poll, until a retry is asked for.
    Failed(String),
}

#[derive(Debug, Clone)]
enum Entry {
    Queued,
    Running,
    Done(PathBuf),
    Failed(String),
}

#[derive(Default)]
struct Inner {
    state: HashMap<PathBuf, Entry>,
    queue: VecDeque<PathBuf>,
    worker: bool,
}

/// How a batch of STEP files becomes VRML files: one result per model, the path of its `.wrl` (written where the converter likes: the store moves it into
/// its cache) or why not.
pub type Converter = Arc<dyn Fn(&[PathBuf]) -> Vec<Result<PathBuf, String>> + Send + Sync>;

/// The converted models, and the one background thread that converts more.
pub struct Store {
    cache_dir: PathBuf,
    convert: Converter,
    /// How long the worker waits for more requests before it converts what it has: the page asks for every model of the board at once.
    gather: Duration,
    inner: Mutex<Inner>,
    /// Signalled when a batch has been converted (or has failed): what a request that waits for its model sleeps on.
    settled: Condvar,
}

/// The most models one kicad-cli run converts: it loads them all at once.
const MAX_BATCH: usize = 16;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Store {
    pub fn new(cache_dir: PathBuf, convert: Converter, gather: Duration) -> Arc<Store> {
        Arc::new(Store { cache_dir, convert, gather, inner: Mutex::new(Inner::default()), settled: Condvar::new() })
    }

    /// Where the converted file of `step` is kept: named after the model, and after the file's path, size and modification time, so a model that changes on
    /// disk is converted again and one that does not is converted once.
    pub fn cache_file(&self, step: &Path) -> PathBuf {
        let meta = std::fs::metadata(step).ok();
        let stamp = meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
        let size = meta.map_or(0, |m| m.len());
        let digest = blake3::hash(format!("{}\0{size}\0{stamp}", step.display()).as_bytes()).to_hex();
        let stem: String = step.file_stem().map(|s| s.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect()).unwrap_or_else(|| "model".into());
        self.cache_dir.join(format!("{stem}-{}.wrl", &digest.as_str()[..16]))
    }

    /// The VRML of the STEP file `step` (a canonical path): what is in the cache, or a conversion started (or joined) in the background.
    pub fn ask(self: &Arc<Self>, step: &Path, retry: bool) -> Ask {
        let cached = self.cache_file(step);
        if cached.is_file() {
            return Ask::Ready(cached);
        }
        let mut inner = lock(&self.inner);
        match inner.state.get(step).cloned() {
            Some(Entry::Done(p)) if p.is_file() => return Ask::Ready(p),
            Some(Entry::Queued | Entry::Running) => return Ask::Pending,
            Some(Entry::Failed(e)) if !retry => return Ask::Failed(e),
            _ => {}
        }
        inner.state.insert(step.to_path_buf(), Entry::Queued);
        inner.queue.push_back(step.to_path_buf());
        if !inner.worker {
            inner.worker = true;
            let store = self.clone();
            if std::thread::Builder::new().name("3dmodels".into()).spawn(move || store.work()).is_err() {
                inner.worker = false;
                inner.state.insert(step.to_path_buf(), Entry::Failed("could not start the conversion thread".into()));
                return Ask::Failed("could not start the conversion thread".into());
            }
        }
        Ask::Pending
    }

    /// Sleeps until the conversion of `step` is over -- it is in the cache or has failed -- or `timeout` has passed. A model that nobody asked for is not waited for.
    /// What a request for a model that is not there yet does on its own thread, so the page needs no timer to learn that the model is in.
    pub fn wait(&self, step: &Path, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        let mut inner = lock(&self.inner);
        loop {
            if !matches!(inner.state.get(step), Some(Entry::Queued | Entry::Running)) {
                return;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            inner = self.settled.wait_timeout(inner, left).map(|(g, _)| g).unwrap_or_else(|e| e.into_inner().0);
        }
    }

    /// Converts what is queued, a batch at a time, until nothing is.
    fn work(self: Arc<Self>) {
        std::thread::sleep(self.gather);
        loop {
            let batch: Vec<PathBuf> = {
                let mut inner = lock(&self.inner);
                let mut batch: Vec<PathBuf> = Vec::new();
                let mut later = VecDeque::new();
                while let Some(p) = inner.queue.pop_front() {
                    // Two models of one file name would be one `.wrl` in one run: the second waits for the next.
                    let clash = batch.iter().any(|b| b.file_stem() == p.file_stem());
                    if batch.len() < MAX_BATCH && !clash {
                        batch.push(p);
                    } else {
                        later.push_back(p);
                    }
                }
                inner.queue = later;
                if batch.is_empty() {
                    inner.worker = false;
                    return;
                }
                for p in &batch {
                    inner.state.insert(p.clone(), Entry::Running);
                }
                batch
            };
            let results = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.convert)(&batch))).unwrap_or_else(|_| batch.iter().map(|_| Err("the model conversion crashed".to_string())).collect());
            let mut inner = lock(&self.inner);
            for (i, step) in batch.iter().enumerate() {
                let outcome = match results.get(i) {
                    Some(Ok(produced)) => self.keep(step, produced),
                    Some(Err(e)) => Err(e.clone()),
                    None => Err("the conversion returned nothing for this model".to_string()),
                };
                inner.state.insert(step.clone(), match outcome {
                    Ok(path) => Entry::Done(path),
                    Err(e) => Entry::Failed(e),
                });
            }
            drop(inner);
            self.settled.notify_all();
        }
    }

    /// Moves a converted file into the cache under its final name.
    fn keep(&self, step: &Path, produced: &Path) -> Result<PathBuf, String> {
        let target = self.cache_file(step);
        std::fs::create_dir_all(&self.cache_dir).map_err(|e| format!("cannot create the 3D model cache {}: {e}", self.cache_dir.display()))?;
        if std::fs::rename(produced, &target).is_err() {
            std::fs::copy(produced, &target).map_err(|e| format!("cannot keep the converted model: {e}"))?;
            let _ = std::fs::remove_file(produced);
        }
        Ok(target)
    }
}

/// Where converted models are kept: `EDA_3DMODEL_CACHE`, else the user's cache directory (`~/Library/Caches/eda-studio/3dmodels` on macOS,
/// `$XDG_CACHE_HOME` or `~/.cache` elsewhere), else the temp directory.
pub fn cache_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("EDA_3DMODEL_CACHE").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library").join("Caches"))
    } else {
        std::env::var_os("XDG_CACHE_HOME").filter(|p| !p.is_empty()).map(PathBuf::from).or_else(|| home.map(|h| h.join(".cache")))
    };
    base.unwrap_or_else(std::env::temp_dir).join("eda-studio").join("3dmodels")
}

/// The converter the studio runs: kicad-cli, in the studio's one kicad-cli lane (a DRC, an ERC and an export never run beside it). Each run works in a
/// directory of its own under the cache; what it produced is moved out of it into the cache's top level before the directory goes.
fn kicad_cli_converter(lane: Arc<Lane>, cache: PathBuf) -> Converter {
    static RUN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    Arc::new(move |batch: &[PathBuf]| {
        let run = RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let work = cache.join(format!("run-{}-{run}", std::process::id()));
        let reply = lane.run("3d models", String::new, || match eda_kicad_engine::convert_models_to_vrml(&work, batch) {
            Ok(files) => json!({ "files": files.iter().map(|f| f.as_ref().map(|p| p.display().to_string())).collect::<Vec<_>>() }),
            Err(e) => json!({ "error": crate::board::reasons(&e) }),
        });
        let produced: Vec<Result<PathBuf, String>> = match (reply["files"].as_array(), reply["error"].as_str()) {
            (Some(files), _) => files.iter().map(|f| f.as_str().map(PathBuf::from).ok_or_else(|| "kicad-cli could not read this model".to_string())).collect(),
            (None, error) => batch.iter().map(|_| Err(error.unwrap_or("the conversion failed").to_string())).collect(),
        };
        let kept = produced
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                r.and_then(|p| {
                    let out = cache.join(format!("converted-{}-{run}-{i}.wrl", std::process::id()));
                    std::fs::rename(&p, &out).map(|_| out).map_err(|e| format!("cannot keep the converted model: {e}"))
                })
            })
            .collect();
        let _ = std::fs::remove_dir_all(&work);
        kept
    })
}

/// The studio's store: one per process, started the first time a model is asked for.
pub fn store(lane: &Arc<Lane>) -> Arc<Store> {
    static STORE: OnceLock<Arc<Store>> = OnceLock::new();
    STORE.get_or_init(|| {
        let cache = cache_dir();
        Store::new(cache.clone(), kicad_cli_converter(lane.clone(), cache), Duration::from_millis(60))
    })
    .clone()
}

// ------------------------------------------------------------------------------- the route

/// An answer for `studio::respond`.
#[derive(Debug)]
pub struct Reply {
    pub status: &'static str,
    pub kind: &'static str,
    pub body: Vec<u8>,
}

fn json_reply(status: &'static str, v: Value) -> Reply {
    Reply { status, kind: "application/json", body: v.to_string().into_bytes() }
}

/// The largest model file this route sends: a real model is a few hundred KB, the biggest of KiCad's libraries about 10 MB.
const MAX_MODEL_BYTES: u64 = 64 * 1024 * 1024;

fn file_reply(path: &Path) -> Reply {
    match std::fs::metadata(path) {
        Ok(m) if m.len() <= MAX_MODEL_BYTES => match std::fs::read(path) {
            Ok(body) => Reply { status: "200 OK", kind: "model/vrml", body },
            Err(e) => json_reply("500 Internal Server Error", json!({ "status": "failed", "error": e.to_string() })),
        },
        Ok(_) => json_reply("413 Payload Too Large", json!({ "status": "failed", "error": "the model file is too large" })),
        Err(e) => json_reply("404 Not Found", json!({ "status": "missing", "error": e.to_string() })),
    }
}

/// `GET /api/3dmodel?name=<percent-encoded model path>[&retry=1]` (`target` is the request target, query included) for the board in `dir`.
///
///   - `200 model/vrml`: the model, ready to parse;
///   - `202 {"status":"pending"}`: a STEP model being converted, ask again;
///   - `200 {"status":"failed","error":..}`: its conversion failed (kept until `retry=1`);
///   - `404 {"status":"missing"}`: the name finds no model; `403 {"status":"forbidden"}`: it finds a file this route may not serve (nothing is read);
///     `400`: no usable name.
pub fn reply(dir: &Path, target: &str, store: &Arc<Store>) -> Reply {
    reply_with(dir, target, store, &models_dir(), &|v| std::env::var(v).ok(), None)
}

/// How long a request with `wait=1` is held for its model: the page asks again after this (a conversion that long is a conversion that is stuck, or a queue of
/// big models). Long enough that the page almost never asks twice, short enough that a closed tab does not leave a thread sleeping for a minute.
pub const WAIT_FOR_MODEL: Duration = Duration::from_secs(25);

/// Whether `target` asks to be held until its model is ready (`&wait=1`): the studio then answers on a thread of its own ([`reply_waiting`]), not on the request loop.
pub fn wants_wait(target: &str) -> bool {
    target.split_once('?').is_some_and(|(_, q)| q.split('&').any(|kv| kv == "wait=1"))
}

/// [`reply`] for a request that waits: a STEP model still being converted is waited for (up to [`WAIT_FOR_MODEL`]) and then answered, so one request per model is
/// all the page sends and the answer comes the moment the model is in. Blocks its thread: not for the request loop.
pub fn reply_waiting(dir: &Path, target: &str, store: &Arc<Store>) -> Reply {
    reply_with(dir, target, store, &models_dir(), &|v| std::env::var(v).ok(), Some(WAIT_FOR_MODEL))
}

/// [`reply`] with the model library and the environment given (the tests'), and how long to wait for a model that is being converted (`None`: not at all).
pub fn reply_with(dir: &Path, target: &str, store: &Arc<Store>, models: &Path, env: &dyn Fn(&str) -> Option<String>, wait: Option<Duration>) -> Reply {
    let query = target.split_once('?').map_or("", |(_, q)| q);
    let value = |key: &str| query.split('&').find_map(|kv| kv.strip_prefix(key).and_then(|v| v.strip_prefix('=')));
    let Some(name) = value("name").and_then(percent_decode) else {
        return Reply { status: "400 Bad Request", kind: "text/plain", body: b"invalid model name".to_vec() };
    };
    let retry = value("retry") == Some("1");
    let roots = Roots { models: models.to_path_buf(), project: dir.to_path_buf() };
    match resolve(&name, &roots, env) {
        Err(Reject::BadName) => Reply { status: "400 Bad Request", kind: "text/plain", body: b"invalid model name".to_vec() },
        Err(Reject::NotFound) => json_reply("404 Not Found", json!({ "status": "missing", "error": format!("no 3D model file for {name}") })),
        Err(Reject::Forbidden(why)) => json_reply("403 Forbidden", json!({ "status": "forbidden", "error": why })),
        Ok(Resolved { path, format: Format::Vrml }) => file_reply(&path),
        Ok(Resolved { path, format: Format::Step }) => {
            let mut asked = store.ask(&path, retry);
            if let (Ask::Pending, Some(limit)) = (&asked, wait) {
                store.wait(&path, limit);
                asked = store.ask(&path, false);
            }
            match asked {
                Ask::Ready(vrml) => file_reply(&vrml),
                Ask::Pending => json_reply("202 Accepted", json!({ "status": "pending" })),
                Ask::Failed(error) => json_reply("200 OK", json!({ "status": "failed", "error": error })),
            }
        }
    }
}

/// The most model names one `prepare` request may carry: a board has a few dozen distinct packages, a big one a few hundred.
const MAX_PREPARE: usize = 1024;

/// `POST /api/3dmodel/prepare {"names": [<model path>, ..]}`: queues the conversion of every STEP model among them that is not in the cache yet, all at once, and answers
/// at once with how many of the names are in each state. The page sends it with every model of the board before it asks for the first, so they are converted in one
/// kicad-cli run and not in the runs the browser's own pace of asking would make. Names that resolve to nothing, or to a file this route may not serve, are counted
/// as missing and not read.
pub fn prepare(dir: &Path, body: &[u8], store: &Arc<Store>) -> Value {
    prepare_with(dir, body, store, &models_dir(), &|v| std::env::var(v).ok())
}

pub fn prepare_with(dir: &Path, body: &[u8], store: &Arc<Store>, models: &Path, env: &dyn Fn(&str) -> Option<String>) -> Value {
    let names: Vec<String> = serde_json::from_slice::<Value>(body).ok().and_then(|v| v["names"].as_array().map(|a| a.iter().filter_map(|n| n.as_str().map(String::from)).collect())).unwrap_or_default();
    let roots = Roots { models: models.to_path_buf(), project: dir.to_path_buf() };
    let (mut ready, mut pending, mut failed, mut missing) = (0, 0, 0, 0);
    for name in names.iter().take(MAX_PREPARE) {
        match resolve(name, &roots, env) {
            Ok(Resolved { format: Format::Vrml, .. }) => ready += 1,
            Ok(Resolved { path, format: Format::Step }) => match store.ask(&path, false) {
                Ask::Ready(_) => ready += 1,
                Ask::Pending => pending += 1,
                Ask::Failed(_) => failed += 1,
            },
            Err(_) => missing += 1,
        }
    }
    json!({ "ready": ready, "pending": pending, "failed": failed, "missing": missing })
}

// ------------------------------------------------------------------------ the parts' models

fn model_json(m: &eda_model::footprint::Model3d) -> Value {
    json!({ "name": m.path, "offset": m.offset, "scale": m.scale, "rotate": m.rotate, "opacity": m.opacity, "show": m.show })
}

/// The 3D models of `part` for `/api/state`: `{ "models": [{ name, offset, scale, rotate, opacity, show }], "kind3d": "smd" | "tht" | "virtual" }`, `None` when
/// it has no model. `footprint` is what the model resolved the part's footprint to. The placement is the footprint file's, in the studio's frame for a top-side
/// footprint (`Model3d::flipped_frame`); the 3D view turns it with a bottom-side part.
pub fn part_json(model: &ConstraintModel, part: &Part, footprint: &Footprint) -> Option<Value> {
    let found = part_models(model, part, footprint)?;
    Some(json!({ "models": found.models.iter().map(model_json).collect::<Vec<_>>(), "kind3d": found.kind.as_str() }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

    /// A scratch directory tree: `<tmp>/models` (KiCad's 3D library), `<tmp>/project`, `<tmp>/outside`.
    struct Tree {
        root: PathBuf,
    }

    impl Tree {
        fn new(name: &str) -> Tree {
            static N: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!("eda-3dmodel-{name}-{}-{}", std::process::id(), N.fetch_add(1, SeqCst)));
            let _ = std::fs::remove_dir_all(&root);
            for d in ["models/Lib.3dshapes", "project/prj.3dshapes", "outside"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            std::fs::write(root.join("models/Lib.3dshapes/a.wrl"), "#VRML V2.0 utf8\n# a").unwrap();
            std::fs::write(root.join("models/Lib.3dshapes/b.step"), "ISO-10303-21;").unwrap();
            std::fs::write(root.join("models/Lib.3dshapes/C-d e.step"), "ISO-10303-21;").unwrap();
            std::fs::write(root.join("models/Lib.3dshapes/notes.txt"), "not a model").unwrap();
            std::fs::write(root.join("project/prj.3dshapes/p.wrl"), "#VRML V2.0 utf8\n# p").unwrap();
            std::fs::write(root.join("project/design.json"), "{\"secret\":\"design\"}").unwrap();
            std::fs::write(root.join("outside/secret.wrl"), "#VRML V2.0 utf8\n# SECRET").unwrap();
            std::fs::write(root.join("outside/secret.step"), "SECRET").unwrap();
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(root.join("outside/secret.wrl"), root.join("models/Lib.3dshapes/link.wrl")).unwrap();
                std::os::unix::fs::symlink(root.join("outside"), root.join("models/escape")).unwrap();
            }
            Tree { root }
        }
        fn roots(&self) -> Roots {
            Roots { models: self.root.join("models"), project: self.root.join("project") }
        }
        fn path(&self, rel: &str) -> PathBuf {
            self.root.join(rel).canonicalize().unwrap()
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn names_resolve_the_way_kicad_resolves_them() {
        let t = Tree::new("resolve");
        let r = t.roots();
        let ok = |name: &str| resolve(name, &r, &no_env);
        let a = Resolved { path: t.path("models/Lib.3dshapes/a.wrl"), format: Format::Vrml };
        assert_eq!(ok("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/a.wrl"), Ok(a.clone()));
        assert_eq!(ok("${KICAD6_3DMODEL_DIR}/Lib.3dshapes/a.wrl"), Ok(a.clone()), "an older version's variable is the current library");
        assert_eq!(ok("$(KISYS3DMOD)/Lib.3dshapes/a.wrl"), Ok(a.clone()), "so is the deprecated one, in either bracket");
        assert_eq!(ok("Lib.3dshapes/a.wrl"), Ok(a.clone()), "a partial path is relative to the library");
        assert_eq!(ok("${KIPRJMOD}/prj.3dshapes/p.wrl"), Ok(Resolved { path: t.path("project/prj.3dshapes/p.wrl"), format: Format::Vrml }));
        assert_eq!(ok("prj.3dshapes/p.wrl"), Ok(Resolved { path: t.path("project/prj.3dshapes/p.wrl"), format: Format::Vrml }), "and relative to the project first");
        assert_eq!(ok("Lib.3dshapes\\a.wrl"), Ok(a), "a Windows separator is a separator");
        assert_eq!(ok("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/b.step"), Ok(Resolved { path: t.path("models/Lib.3dshapes/b.step"), format: Format::Step }));
        // A VRML that is not there is the STEP of the same part (kicad-cli's --subst-models).
        assert_eq!(ok("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/b.wrl"), Ok(Resolved { path: t.path("models/Lib.3dshapes/b.step"), format: Format::Step }));
        assert_eq!(ok("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/c_d_e.wrl"), Ok(Resolved { path: t.path("models/Lib.3dshapes/C-d e.step"), format: Format::Step }), "a name that differs in case, '-' and ' ' too");
        assert_eq!(ok("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/nothing.step"), Err(Reject::NotFound));
        assert_eq!(ok("${NOT_A_VARIABLE}/Lib.3dshapes/a.wrl"), Err(Reject::NotFound), "a variable nobody defines is left as written");
        // The process environment is read like KiCad's own.
        let env = |v: &str| (v == "MY_3D").then(|| t.root.join("models").to_string_lossy().into_owned());
        assert!(resolve("${MY_3D}/Lib.3dshapes/a.wrl", &r, &env).is_ok());
    }

    #[test]
    fn nothing_outside_the_model_library_and_the_project_is_readable() {
        let t = Tree::new("escapes");
        let r = t.roots();
        let forbidden = |name: &str| matches!(resolve(name, &r, &no_env), Err(Reject::Forbidden(_)));
        let nope = |name: &str| matches!(resolve(name, &r, &no_env), Err(Reject::NotFound | Reject::Forbidden(_) | Reject::BadName));
        assert!(forbidden("${KICAD10_3DMODEL_DIR}/../outside/secret.wrl"), "a .. out of the library");
        assert!(forbidden("../outside/secret.wrl"), "a .. out of the project");
        assert!(forbidden("${KIPRJMOD}/../outside/secret.step"));
        assert!(forbidden(&t.root.join("outside/secret.wrl").to_string_lossy()), "an absolute path elsewhere");
        assert!(forbidden("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/link.wrl"), "a symlink in the library that leads out of it");
        assert!(forbidden("${KICAD10_3DMODEL_DIR}/escape/secret.wrl"), "a directory symlink that leads out of it");
        assert!(forbidden("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/notes.txt"), "a file in the library that is no model");
        assert!(forbidden("${KIPRJMOD}/design.json"), "the board's own file: only model files are served");
        assert!(nope("/etc/passwd"), "{:?}", resolve("/etc/passwd", &r, &no_env));
        assert!(nope(&format!("{}/../../../../../etc/passwd", r.models.display())));
        assert!(nope("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/a.wrl\0.txt"), "a NUL ends a C string");
        assert!(nope(""));
        // The environment cannot lead anywhere the roots do not.
        let env = |v: &str| (v == "ELSEWHERE").then(|| t.root.join("outside").to_string_lossy().into_owned());
        assert!(matches!(resolve("${ELSEWHERE}/secret.wrl", &r, &env), Err(Reject::Forbidden(_))));
    }

    #[test]
    fn the_route_answers_each_case_and_never_reads_what_it_refuses() {
        let t = Tree::new("route");
        let store = Store::new(t.root.join("cache"), Arc::new(|b: &[PathBuf]| b.iter().map(|_| Err("no converter".to_string())).collect()), Duration::from_millis(1));
        let (models, project) = (t.root.join("models"), t.root.join("project"));
        let get = |target: &str| reply_with(&project, target, &store, &models, &no_env, None);
        let enc = |s: &str| s.replace('%', "%25").replace('{', "%7B").replace('}', "%7D").replace('/', "%2F").replace(' ', "%20");

        let ok = get(&format!("/api/3dmodel?name={}", enc("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/a.wrl")));
        assert_eq!((ok.status, ok.kind), ("200 OK", "model/vrml"));
        assert_eq!(ok.body, b"#VRML V2.0 utf8\n# a");
        for (target, status) in [
            (format!("/api/3dmodel?name={}", enc("${KICAD10_3DMODEL_DIR}/../outside/secret.wrl")), "403 Forbidden"),
            ("/api/3dmodel?name=%2e%2e%2foutside%2fsecret.wrl".to_string(), "403 Forbidden"),
            ("/api/3dmodel?name=..%2F..%2F..%2Fetc%2Fpasswd".to_string(), "404 Not Found"),
            (format!("/api/3dmodel?name={}", enc(&t.root.join("outside/secret.wrl").to_string_lossy())), "403 Forbidden"),
            (format!("/api/3dmodel?name={}", enc("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/link.wrl")), "403 Forbidden"),
            (format!("/api/3dmodel?name={}", enc("${KIPRJMOD}/design.json")), "403 Forbidden"),
            ("/api/3dmodel?name=%zz".to_string(), "400 Bad Request"),
            ("/api/3dmodel?name=".to_string(), "400 Bad Request"),
            ("/api/3dmodel".to_string(), "400 Bad Request"),
            (format!("/api/3dmodel?name={}", enc("Lib.3dshapes/missing.wrl")), "404 Not Found"),
        ] {
            let r = get(&target);
            assert_eq!(r.status, status, "{target}: {}", String::from_utf8_lossy(&r.body));
            let body = String::from_utf8_lossy(&r.body);
            assert!(!body.contains("SECRET") && !body.contains("secret\":\"design") && !body.contains("root:"), "{target} leaked a file: {body}");
        }
    }

    #[test]
    fn percent_decoding_is_encode_uri_components() {
        assert_eq!(percent_decode("${KICAD10_3DMODEL_DIR}%2FLib.3dshapes%2Fa%20b%2Bc%2C1.step").as_deref(), Some("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/a b+c,1.step"));
        assert_eq!(percent_decode("a+b").as_deref(), Some("a+b"), "a plus is a plus");
        assert_eq!(percent_decode("%e2%82%ac").as_deref(), Some("\u{20ac}"), "UTF-8");
        assert_eq!(percent_decode("%"), None);
        assert_eq!(percent_decode("%4"), None);
        assert_eq!(percent_decode("%gg"), None);
        assert_eq!(percent_decode("%ff"), None, "not UTF-8");
    }

    /// A converter that writes a VRML file per model and counts its runs and the size of each.
    fn counting_converter(dir: PathBuf, runs: Arc<Mutex<Vec<usize>>>) -> Converter {
        Arc::new(move |batch: &[PathBuf]| {
            runs.lock().unwrap().push(batch.len());
            std::fs::create_dir_all(&dir).unwrap();
            batch
                .iter()
                .enumerate()
                .map(|(i, step)| {
                    if step.to_string_lossy().contains("bad") {
                        return Err("kicad-cli could not read this model".to_string());
                    }
                    let out = dir.join(format!("run{}-{i}.wrl", runs.lock().unwrap().len()));
                    std::fs::write(&out, format!("#VRML V2.0 utf8\n# {}", step.file_name().unwrap().to_string_lossy())).unwrap();
                    Ok(out)
                })
                .collect()
        })
    }

    fn wait_for(store: &Arc<Store>, step: &Path) -> Ask {
        for _ in 0..400 {
            match store.ask(step, false) {
                Ask::Pending => std::thread::sleep(Duration::from_millis(10)),
                other => return other,
            }
        }
        Ask::Pending
    }

    #[test]
    fn models_asked_for_together_are_converted_in_one_run_and_kept() {
        let t = Tree::new("store");
        let runs = Arc::new(Mutex::new(Vec::new()));
        let store = Store::new(t.root.join("cache"), counting_converter(t.root.join("work"), runs.clone()), Duration::from_millis(120));
        let steps: Vec<PathBuf> = ["b.step", "C-d e.step"].iter().map(|n| t.path(&format!("models/Lib.3dshapes/{n}"))).collect();
        assert_eq!(store.ask(&steps[0], false), Ask::Pending, "the first ask starts the work and answers at once");
        assert_eq!(store.ask(&steps[1], false), Ask::Pending);
        let ready: Vec<Ask> = steps.iter().map(|s| wait_for(&store, s)).collect();
        for (step, got) in steps.iter().zip(&ready) {
            let Ask::Ready(path) = got else { panic!("{step:?} -> {got:?}") };
            assert!(path.starts_with(t.root.join("cache")), "kept in the cache: {path:?}");
            assert!(std::fs::read_to_string(path).unwrap().contains(step.file_name().unwrap().to_str().unwrap()), "each model has its own file");
        }
        assert_eq!(*runs.lock().unwrap(), vec![2], "two models, one run");
        // Asked again, a model is in the cache: no run. A new store over the same directory finds it too (a model is converted once, not once per session).
        assert!(matches!(store.ask(&steps[0], false), Ask::Ready(_)));
        let second = Store::new(t.root.join("cache"), counting_converter(t.root.join("work"), runs.clone()), Duration::from_millis(1));
        assert!(matches!(second.ask(&steps[1], false), Ask::Ready(_)), "found on disk");
        assert_eq!(runs.lock().unwrap().len(), 1, "and neither asked kicad-cli again");
        // The cached file is named after the source file's size and time: a model that changes is another file.
        let before = store.cache_file(&steps[0]);
        std::fs::write(&steps[0], "ISO-10303-21; changed").unwrap();
        assert_ne!(store.cache_file(&steps[0]), before);
    }

    #[test]
    fn a_failed_conversion_is_kept_until_a_retry_and_models_of_one_name_run_apart() {
        let t = Tree::new("failed");
        let runs = Arc::new(Mutex::new(Vec::new()));
        let store = Store::new(t.root.join("cache"), counting_converter(t.root.join("work"), runs.clone()), Duration::from_millis(30));
        std::fs::write(t.root.join("models/Lib.3dshapes/bad.step"), "x").unwrap();
        std::fs::create_dir_all(t.root.join("models/Other.3dshapes")).unwrap();
        std::fs::write(t.root.join("models/Other.3dshapes/b.step"), "other b").unwrap();
        let bad = t.path("models/Lib.3dshapes/bad.step");
        assert_eq!(store.ask(&bad, false), Ask::Pending);
        let Ask::Failed(why) = wait_for(&store, &bad) else { panic!("a model kicad-cli cannot read fails") };
        assert!(why.contains("could not read"), "{why}");
        assert!(matches!(store.ask(&bad, false), Ask::Failed(_)), "kept: not run again on every poll");
        assert_eq!(runs.lock().unwrap().len(), 1);
        assert_eq!(store.ask(&bad, true), Ask::Pending, "a retry runs it again");
        assert!(matches!(wait_for(&store, &bad), Ask::Failed(_)));
        assert_eq!(runs.lock().unwrap().len(), 2);

        // Two models with one file name (one library's b.step and another's) would write one b.wrl: they are converted in runs of their own.
        let (b1, b2) = (t.path("models/Lib.3dshapes/b.step"), t.path("models/Other.3dshapes/b.step"));
        runs.lock().unwrap().clear();
        assert_eq!(store.ask(&b1, false), Ask::Pending);
        assert_eq!(store.ask(&b2, false), Ask::Pending);
        assert!(matches!(wait_for(&store, &b1), Ask::Ready(_)) && matches!(wait_for(&store, &b2), Ask::Ready(_)));
        assert_eq!(*runs.lock().unwrap(), vec![1, 1]);
    }

    fn delayed(inner: Converter, delay: Duration) -> Converter {
        Arc::new(move |batch: &[PathBuf]| {
            std::thread::sleep(delay);
            inner(batch)
        })
    }

    /// The page sends every model of the board in one `prepare` (one kicad-cli run converts them), then one request per model that waits for it on a thread of its
    /// own: the answer comes when the model is in, with no polling and so no timer in the page.
    #[test]
    fn prepare_queues_a_boards_models_together_and_a_waiting_request_is_answered_when_its_model_is_in() {
        let t = Tree::new("wait");
        let runs = Arc::new(Mutex::new(Vec::new()));
        let store = Store::new(t.root.join("cache"), delayed(counting_converter(t.root.join("work"), runs.clone()), Duration::from_millis(250)), Duration::from_millis(40));
        let (models, project) = (t.root.join("models"), t.root.join("project"));
        let body = json!({ "names": [
            "${KICAD10_3DMODEL_DIR}/Lib.3dshapes/b.step",
            "${KICAD10_3DMODEL_DIR}/Lib.3dshapes/C-d e.step",
            "${KICAD10_3DMODEL_DIR}/Lib.3dshapes/a.wrl",
            "${KICAD10_3DMODEL_DIR}/../outside/secret.wrl",
            "nothing.step"
        ] })
        .to_string();
        let got = prepare_with(&project, body.as_bytes(), &store, &models, &no_env);
        assert_eq!(got, json!({ "ready": 1, "pending": 2, "failed": 0, "missing": 2 }), "a VRML is ready; two STEPs are queued; an escape and an unknown name are missing and not read");
        assert_eq!(prepare_with(&project, b"not json", &store, &models, &no_env), json!({ "ready": 0, "pending": 0, "failed": 0, "missing": 0 }));

        let enc = |s: &str| s.replace('{', "%7B").replace('}', "%7D").replace('/', "%2F").replace(' ', "%20");
        let started = Instant::now();
        let target = format!("/api/3dmodel?name={}&wait=1", enc("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/b.step"));
        let r = reply_with(&project, &target, &store, &models, &no_env, Some(Duration::from_secs(20)));
        assert_eq!((r.status, r.kind), ("200 OK", "model/vrml"), "{}", String::from_utf8_lossy(&r.body));
        assert!(r.body.starts_with(b"#VRML V2.0 utf8") && started.elapsed() >= Duration::from_millis(200), "held until the conversion was over: {:?}", started.elapsed());
        let r2 = reply_with(&project, &format!("/api/3dmodel?name={}&wait=1", enc("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/C-d e.step")), &store, &models, &no_env, Some(Duration::from_secs(20)));
        assert_eq!(r2.status, "200 OK");
        assert_eq!(*runs.lock().unwrap(), vec![2], "the two prepared models were one run");

        // A wait that runs out answers pending: the page asks again.
        std::fs::write(t.root.join("models/Lib.3dshapes/slow.step"), "x").unwrap();
        let slow = format!("/api/3dmodel?name={}&wait=1", enc("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/slow.step"));
        let short = reply_with(&project, &slow, &store, &models, &no_env, Some(Duration::from_millis(20)));
        assert_eq!(short.status, "202 Accepted", "{}", String::from_utf8_lossy(&short.body));
        let long = reply_with(&project, &slow, &store, &models, &no_env, Some(Duration::from_secs(20)));
        assert_eq!(long.status, "200 OK");
    }

    #[test]
    fn only_wait_equals_one_holds_a_request() {
        assert!(wants_wait("/api/3dmodel?name=a&wait=1"));
        assert!(wants_wait("/api/3dmodel?wait=1&name=a"));
        assert!(!wants_wait("/api/3dmodel?name=a"));
        assert!(!wants_wait("/api/3dmodel?name=a&wait=0"));
        assert!(!wants_wait("/api/3dmodel?name=wait%3D1"));
        assert!(!wants_wait("/api/3dmodel"));
    }

    #[test]
    fn the_parts_models_go_out_with_their_placement_and_kind() {
        let part = Part { reference: "R1".into(), mpn: None, lcsc: None, value: None, package: Some("0603".into()), footprint: Some("0603".into()), pins: vec![], body_um: None, symbol: None, datasheet: None, edge: None };
        let model = ConstraintModel { parts: vec![part.clone()], ..Default::default() };
        let fp = model.footprint_of(&part).unwrap();
        let v = part_json(&model, &part, &fp).expect("a 0603 has a model");
        assert_eq!(v["kind3d"], "smd");
        let m = &v["models"][0];
        assert!(m["name"].as_str().unwrap().ends_with("Resistor_SMD.3dshapes/R_0603_1608Metric.step"), "{m}");
        assert_eq!((m["scale"].clone(), m["opacity"].clone(), m["show"].clone()), (json!([1.0, 1.0, 1.0]), json!(1.0), json!(true)));
        let none = Part { package: Some("NO-SUCH-PACKAGE".into()), footprint: Some("NO-SUCH-PACKAGE".into()), ..part.clone() };
        assert!(model.footprint_of(&none).is_none(), "no footprint, no models");
    }

    /// The real thing, with KiCad.app installed (a kicad-cli round trip: the slow tier): a STEP of the installed library becomes VRML in KiCad's units.
    #[test]
    fn a_real_step_model_is_converted_by_kicad_cli() {
        if std::env::var_os("EDA_SLOW_TESTS").is_none() || eda_kicad_engine::find_cli().is_none() {
            return;
        }
        let step = models_dir().join("Resistor_SMD.3dshapes/R_0603_1608Metric.step");
        if !step.is_file() {
            return;
        }
        let t = Tree::new("real");
        let store = Store::new(t.root.join("cache"), kicad_cli_converter(Arc::new(Lane::default()), t.root.join("cache")), Duration::from_millis(1));
        let Ask::Ready(vrml) = wait_for_long(&store, &step.canonicalize().unwrap()) else { panic!("converted") };
        let text = std::fs::read_to_string(vrml).unwrap();
        assert!(text.starts_with("#VRML V2.0 utf8"), "{}", &text[..text.len().min(80)]);
        assert!(text.contains("IndexedFaceSet"), "geometry");
    }

    fn wait_for_long(store: &Arc<Store>, step: &Path) -> Ask {
        for _ in 0..3000 {
            match store.ask(step, false) {
                Ask::Pending => std::thread::sleep(Duration::from_millis(20)),
                other => return other,
            }
        }
        Ask::Pending
    }
}
