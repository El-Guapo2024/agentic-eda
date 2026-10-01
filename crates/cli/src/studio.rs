//! `eda board serve`: the board in a browser, editable.
//!
//! A person and a model work the same board through the same verbs. The
//! page shows what is on the board -- parts, pads by net, the airwires
//! still to route, tracks once routed, the gates failing -- and turns a
//! drag, a key press or a click into a board command, which runs through
//! [`crate::board::step`] exactly as the CLI's does. Commands typed at
//! the CLI show up on the page within a second, and the page's show up
//! in `activity.jsonl` for the CLI side to read: one board, one set of
//! verbs, two hands.
//!
//! A local tool, so a small one: the standard library's TCP listener,
//! one request at a time, bound to 127.0.0.1. Because requests are
//! handled one at a time, anything slow has to get off that thread
//! itself rather than making every other route wait: routing runs on
//! its own thread (`Job`, below), and so does a GLB export (`GlbJob`,
//! serve_board_glb) -- the export alone can take minutes on a many-part
//! board (real STEP models through kicad-cli/OpenCascade), and it once
//! blocked /api/version and everything else for that long before this
//! was fixed to run in the background instead.

use crate::board;
use eda_model::footprint::{placed_courtyard, placed_pads};
use eda_model::ir::{LabelSide, Shape, Side};
use eda_model::{CheckResult, CheckStatus};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const PAGE: &str = include_str!("studio.html");

/// What the routing thread is doing: "idle", "running", or how the last
/// run ended.
type Job = Arc<Mutex<String>>;

/// Where the built React UI lives, in precedence order: `--ui <dir>`,
/// then `EDA_STUDIO_UI`, then the workspace-relative `web/studio/dist`
/// this binary was compiled from. `None` from [`built_ui`] means none of
/// those has an `index.html` yet, so `serve` falls back to the embedded
/// single-file page.
fn ui_dir(cli_ui: Option<&Path>) -> PathBuf {
    if let Some(p) = cli_ui {
        return p.to_path_buf();
    }
    if let Ok(p) = std::env::var("EDA_STUDIO_UI") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/studio/dist")
}

fn built_ui(cli_ui: Option<&Path>) -> Option<PathBuf> {
    let dir = ui_dir(cli_ui);
    dir.join("index.html").is_file().then_some(dir)
}

pub fn serve(dir: &Path, port: u16, ui: Option<PathBuf>) -> Result<(), Vec<CheckResult>> {
    // Fail now, not on the first request, if this is no board.
    board::load(dir)?;
    let ui_root = built_ui(ui.as_deref());
    // A taken port is usually another studio already showing a board;
    // take the next free one rather than refuse.
    let (listener, got) = (port..port.saturating_add(20))
        .find_map(|p| TcpListener::bind(("127.0.0.1", p)).ok().map(|l| (l, p)))
        .ok_or_else(|| vec![CheckResult::fail("serve_bind", format!("127.0.0.1:{port}"), format!("ports {port}-{} are all taken", port.saturating_add(19)))])?;
    if got != port {
        eprintln!("board: port {port} is taken (another studio open there?), using {got}");
    }
    eprintln!(
        "board: serving {} at http://127.0.0.1:{got}/ ({})",
        dir.display(),
        match &ui_root {
            Some(d) => format!("ui: {}", d.display()),
            None => "ui: embedded studio.html".to_string(),
        }
    );
    let job: Job = Arc::new(Mutex::new("idle".into()));
    let schematic: Mutex<Option<(std::time::SystemTime, String)>> = Mutex::new(None);
    // The kicad-cli export a GLB needs can take minutes on a many-part
    // board (see GlbBuild's doc comment) -- this server handles one
    // request at a time, so that export runs on its own background
    // thread rather than inside handle(), and this slot is how the next
    // /api/board.glb poll finds out how it's going. Cached by the same
    // version string /api/version returns, same reasoning as `schematic`
    // above: an unchanged board should never re-run kicad-cli.
    let glb_job: GlbJob = Arc::new(Mutex::new(None));
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        if let Err(e) = handle(&mut stream, dir, &job, &schematic, &glb_job, ui_root.as_deref()) {
            let _ = respond(&mut stream, "500 Internal Server Error", "text/plain", e.as_bytes());
        }
    }
    Ok(())
}

/// A file under the built UI's directory, or 404 if it does not resolve
/// to one (missing, or outside `root` -- no serving `../../etc/passwd`
/// through a crafted path).
fn serve_file(stream: &mut TcpStream, root: &Path, rel: &str) -> Result<(), String> {
    let rel = if rel.is_empty() { "index.html" } else { rel };
    let candidate = root.join(rel);
    let resolved = candidate.canonicalize().ok().zip(root.canonicalize().ok()).filter(|(p, r)| p.starts_with(r)).map(|(p, _)| p);
    match resolved.and_then(|p| std::fs::read(&p).ok().map(|b| (p, b))) {
        Some((p, bytes)) => respond(stream, "200 OK", mime_of(&p), &bytes),
        // A client-side route (no file extension) falls back to index.html,
        // like any single-page app; a genuinely missing asset still 404s.
        None if !rel.contains('.') => {
            let index = root.join("index.html");
            match std::fs::read(&index) {
                Ok(bytes) => respond(stream, "200 OK", mime_of(&index), &bytes),
                Err(_) => respond(stream, "404 Not Found", "text/plain", b"not found"),
            }
        }
        None => respond(stream, "404 Not Found", "text/plain", b"not found"),
    }
}

/// Where KiCad's own installed 3D model library lives, in precedence
/// order: `EDA_KICAD_3DMODELS_DIR`, then the well-known macOS install
/// path -- same "env var, then a sane default" shape as [`ui_dir`].
fn kicad_3dmodels_dir() -> PathBuf {
    if let Ok(p) = std::env::var("EDA_KICAD_3DMODELS_DIR") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from("/Applications/KiCad/KiCad.app/Contents/SharedSupport/3dmodels")
}

/// GET /api/3dmodel?name=<Lib.3dshapes/File.ext> -- serves one file out
/// of KiCad's own installed 3D model library, read-only, format-
/// agnostic (whatever bytes are on disk at that path). `name` is
/// rejected outright if it contains `..` or is itself an absolute path;
/// what's left is resolved against the library root and canonicalized,
/// the same traversal protection [`serve_file`] already uses for the UI
/// directory, so nothing outside that one directory tree is ever
/// readable through this route. Not percent-decoded: real KiCad
/// library/file names are plain ASCII (letters, digits, `_.-` and the
/// one literal `/` between library and file) with nothing that needs
/// escaping in a query string, so the caller sends `name` unencoded
/// rather than this needing a general percent-decoder for one path.
///
/// The task this route was built for asked for `.wrl` (VRML) files
/// specifically, loaded client-side with three's VRMLLoader -- checked
/// directly against the KiCad 10.99.0 install on this machine
/// (`/Applications/KiCad/KiCad.app`, `EDA_KICAD_3DMODELS_DIR` unset) and
/// it ships zero `.wrl` files: 7238 `.step` files and no VRML anywhere
/// under 3dmodels/. Modern KiCad (this one included) bundles STEP, not
/// VRML, for its footprint 3D models. Three.js has no STEP loader (it's
/// a full CAD B-rep format, not a mesh format a mesh loader can read),
/// so this route is real and correct but nothing in this app's frontend
/// calls it yet -- wiring VRMLLoader up against a library that has no
/// `.wrl` files would 404 on every single model. Left in place as
/// working, generically useful (format-agnostic) infrastructure for
/// whatever actually converts/serves real geometry later, rather than
/// building a client-side loader that can only ever fail here.
fn serve_3dmodel(stream: &mut TcpStream, name: &str) -> Result<(), String> {
    if name.is_empty() || name.contains("..") || Path::new(name).is_absolute() {
        return respond(stream, "400 Bad Request", "text/plain", b"invalid model name");
    }
    let root = kicad_3dmodels_dir();
    let candidate = root.join(name);
    let resolved = candidate.canonicalize().ok().zip(root.canonicalize().ok()).filter(|(p, r)| p.starts_with(r)).map(|(p, _)| p);
    match resolved.and_then(|p| std::fs::read(&p).map(|b| (p, b)).ok()) {
        // Content-Type by actual extension, not assumed VRML -- this
        // library is all .step right now (see the doc comment above).
        Some((p, bytes)) => {
            let kind = match p.extension().and_then(|e| e.to_str()).unwrap_or("") {
                "wrl" => "model/vrml",
                "step" | "stp" => "model/step",
                _ => "application/octet-stream",
            };
            respond(stream, "200 OK", kind, &bytes)
        }
        None => respond(stream, "404 Not Found", "text/plain", b"model not found"),
    }
}

/// Where the real `kicad-cli` binary lives -- `EDA_KICAD_CLI` if set
/// (it is not on PATH in this dev environment), else KiCad's own
/// default macOS install location. Same "env var, then a sane default"
/// shape as `kicad_3dmodels_dir`/`ui_dir`.
fn kicad_cli_path() -> PathBuf {
    if let Ok(p) = std::env::var("EDA_KICAD_CLI") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli")
}

/// What the background GLB-export thread (spawned by [`serve_board_glb`])
/// is doing for the one version string it was started for. A single
/// `Option<(String, GlbBuild)>` slot rather than a map keyed by version:
/// there is never more than one export in flight (see the guard in
/// `serve_board_glb`), and a new one always replaces whatever the slot
/// held before, exactly like `glb_job` in the doc comment on `serve`.
enum GlbBuild {
    Running,
    /// `Arc` so a request that finds a `Done` result can clone the
    /// handle and drop the mutex lock *before* writing the (possibly
    /// several-MB) body to the socket -- holding the lock across that
    /// blocking I/O would defeat the whole point of this background-
    /// thread design by serializing unrelated requests behind a slow
    /// client again.
    Done(Arc<Vec<u8>>),
    Failed(String),
}
type GlbJob = Arc<Mutex<Option<(String, GlbBuild)>>>;

/// kicad-cli killed and the export marked failed if it runs longer than
/// this. Chosen well above what a legitimately large board should take
/// (a small board with only built-in-package-sized models exports in
/// single-digit seconds; a board using a handful of large real-library
/// models -- some run several MB, e.g. Connector_Molex's SlimStack
/// parts -- measured several times slower still) but well short of "the
/// server looks hung" -- the whole point of this route is to never
/// again be the thing that makes the studio look frozen (see GlbBuild's
/// doc comment on why this races on a thread at all).
const GLB_EXPORT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

/// GET /api/board.glb -- the whole board (body, copper, silk, mask, and
/// every placed footprint's real 3D model) as one binary GLTF file, for
/// the 3D tab's GLTFLoader to render directly instead of this app's own
/// procedural boxes-and-planes scene. Built by exporting the current
/// design to a temp .kicad_pcb (the same writer `eda export` uses,
/// crates/kicad/src/pcb.rs's write_footprint -- now emitting each
/// footprint's `(model ...)` when `Footprint::model` is set) and
/// shelling out to `kicad-cli pcb export glb`, the one thing that can
/// actually read a STEP model and bake a whole board's geometry into
/// one file -- there is no Rust STEP/GLTF pipeline in this codebase to
/// do it directly.
///
/// That export can be slow -- one report measured over two minutes on a
/// 30-part board -- and it's almost entirely kicad-cli loading STEP
/// models through OpenCascade: on this dev machine the same 30-part
/// board with tracks/vias/small built-in-package models re-exports in
/// 2.5-4.5s, but skipping components entirely (`--no-components`) drops
/// that to well under a second, and swapping just one small (~150KB)
/// connector model for a large (~9MB) real one added several more
/// seconds by itself -- so both "which parts a board uses" and ordinary
/// machine load can easily stretch this from single-digit seconds to
/// minutes. This server answers one request at a time (`serve`'s doc
/// comment), so running the export inline here once blocked every other
/// route, including /api/version, until it finished: the
/// whole studio looked frozen for as long as the export ran. Fixed by
/// never blocking in the request handler at all:
///  - nothing cached/running yet for the current version -> mark this
///    version `Running`, spawn it on its own thread, answer 202 at once;
///  - already `Running` (this version or a stale one -- there is only
///    ever one export in flight) -> 202 again, still pending;
///  - `Done` for the *current* version -> the actual GLB, 200;
///  - `Failed` for the *current* version -> that error, 200 (a
///    well-formed answer, not a server error) -- and left cached rather
///    than retried, so a board that reliably fails/times out doesn't
///    get kicad-cli re-run on every poll; a genuinely new attempt has to
///    wait for the version to actually change.
fn serve_board_glb(stream: &mut TcpStream, dir: &Path, job: &Job, glb: &GlbJob) -> Result<(), String> {
    let ver = version_string(dir, job);
    let mut guard = glb.lock().map_err(|e| e.to_string())?;
    match guard.as_ref() {
        Some((v, GlbBuild::Done(bytes))) if *v == ver => {
            let bytes = bytes.clone();
            drop(guard);
            respond(stream, "200 OK", "model/gltf-binary", &bytes)
        }
        Some((v, GlbBuild::Failed(err))) if *v == ver => {
            let body = json!({ "status": "failed", "error": err }).to_string();
            drop(guard);
            respond(stream, "200 OK", "application/json", body.as_bytes())
        }
        // Either this exact version is already being built, or a stale
        // one is and hasn't been superseded yet -- either way, kicking
        // off a second kicad-cli process would violate "only one export
        // at a time", so this request just joins the same wait as
        // whoever's already polling.
        Some((_, GlbBuild::Running)) => {
            drop(guard);
            respond(stream, "202 Accepted", "application/json", br#"{"status":"pending"}"#)
        }
        // Stale Done/Failed for an old version, or nothing at all yet.
        _ => {
            *guard = Some((ver.clone(), GlbBuild::Running));
            drop(guard);
            let dir = dir.to_path_buf();
            let glb = glb.clone();
            std::thread::spawn(move || {
                let outcome = match build_glb(&dir) {
                    Ok(bytes) => GlbBuild::Done(Arc::new(bytes)),
                    Err(e) => GlbBuild::Failed(e),
                };
                if let Ok(mut g) = glb.lock() {
                    *g = Some((ver, outcome));
                }
            });
            respond(stream, "202 Accepted", "application/json", br#"{"status":"pending"}"#)
        }
    }
}

/// Kills kicad-cli's whole process group, not just the one pid
/// `std::process::Child` knows about. `Child::kill` only signals that
/// exact process; if it had spawned children of its own, a plain kill
/// leaves them orphaned and running forever -- confirmed directly while
/// testing this timeout path against a slow stand-in process, not a
/// theoretical worry. `build_glb` spawns kicad-cli with
/// `.process_group(0)`, putting it in a fresh group led by its own pid,
/// so signalling the *negative* pid here reaches that whole group in
/// one shot. Shells out to `kill` rather than adding a libc dependency
/// for one syscall -- consistent with this whole module's existing
/// approach to anything std doesn't cover (it already shells out to
/// kicad-cli itself). Unix-only (`process_group`/negative-pid-as-group
/// is POSIX), matching this file's existing macOS-only assumptions
/// elsewhere (e.g. kicad_cli_path's default).
fn kill_process_group(child: &std::process::Child) {
    let _ = std::process::Command::new("kill").arg("-9").arg("--").arg(format!("-{}", child.id())).status();
}

/// The actual export: current design -> temp .kicad_pcb -> `kicad-cli
/// pcb export glb` -> the produced file's bytes. Runs on the background
/// thread [`serve_board_glb`] spawns, never on the request-handling
/// thread -- see that function's doc comment for why. `kicad-cli` is
/// given [`GLB_EXPORT_TIMEOUT`] to finish and killed (not just abandoned
/// -- [`kill_process_group`] + `wait` so the whole thing is actually
/// reaped, not left as a zombie/orphan still holding the board's temp
/// files open) if it runs longer, so a stuck or pathological export can
/// never pile up processes or run forever even though nothing is
/// polling its stdout.
fn build_glb(dir: &Path) -> Result<Vec<u8>, String> {
    let (meta, design, model) = board::load(dir).map_err(|e| board::reasons(&e))?;
    let title = Path::new(&meta.intent).file_stem().and_then(|s| s.to_str()).unwrap_or("board").to_string();
    let date = eda::now_rfc3339();
    let pcb_text = eda::export_kicad_pcb(&design, &model, &eda::ExportMeta { date: &date[..10], title: &title }).map_err(|e| board::reasons(&e))?;

    // Unique per call (not just per process, as the old synchronous
    // version had it): the single-export-at-a-time guard means two of
    // these never race, but a timed-out export's thread is still
    // cleaning up its own tmp_dir in the background for a moment after
    // `build_glb` returns Err to it, and reusing the exact same path for
    // the *next* export (if the version changed again quickly) would
    // race that cleanup. Nanosecond timestamp is enough entropy for
    // "never collides with the previous call from this same process".
    let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let tmp_dir = std::env::temp_dir().join(format!("eda-board-glb-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("could not create temp dir: {e}"))?;
    let cleanup = || {
        let _ = std::fs::remove_dir_all(&tmp_dir);
    };
    let pcb_path = tmp_dir.join(format!("{title}.kicad_pcb"));
    let glb_path = tmp_dir.join(format!("{title}.glb"));
    let stderr_path = tmp_dir.join("stderr.log");
    if let Err(e) = std::fs::write(&pcb_path, &pcb_text) {
        cleanup();
        return Err(format!("could not write temp .kicad_pcb: {e}"));
    }
    let stderr_file = match std::fs::File::create(&stderr_path) {
        Ok(f) => f,
        Err(e) => {
            cleanup();
            return Err(format!("could not create temp stderr file: {e}"));
        }
    };

    // Its own process group (pgid = its own pid), not this server's --
    // see kill_process_group's doc comment for why that matters on
    // timeout.
    use std::os::unix::process::CommandExt as _;
    let start = std::time::Instant::now();
    let mut child = match std::process::Command::new(kicad_cli_path())
        .args([
            "pcb",
            "export",
            "glb",
            "--subst-models",
            "--include-tracks",
            "--include-pads",
            "--include-zones",
            "--include-silkscreen",
            "--include-soldermask",
            "-o",
        ])
        .arg(&glb_path)
        .arg(&pcb_path)
        .stdout(std::process::Stdio::null())
        // A file, not `Stdio::piped()`: piped output has to be actively
        // drained or a chatty child can fill the pipe buffer and block
        // on its own write() -- exactly the kind of stall this whole
        // change exists to get away from. A file needs no draining.
        .stderr(std::process::Stdio::from(stderr_file))
        .process_group(0)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            cleanup();
            return Err(format!("could not run kicad-cli ({}): {e}", kicad_cli_path().display()));
        }
    };

    let deadline = start + GLB_EXPORT_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if std::time::Instant::now() >= deadline => {
                kill_process_group(&child);
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(200)),
            Err(e) => {
                cleanup();
                return Err(format!("waiting on kicad-cli failed: {e}"));
            }
        }
    };
    let elapsed = start.elapsed();

    let Some(status) = status else {
        cleanup();
        return Err(format!("kicad-cli pcb export glb timed out after {}s and was killed", GLB_EXPORT_TIMEOUT.as_secs()));
    };
    // Not the report itself (this fn has no request to answer with one),
    // but the one place that actually knows the number: the server's own
    // stderr, so `eda board serve`'s own operator can see it too.
    eprintln!("board: kicad-cli pcb export glb ({title}) took {:.1}s", elapsed.as_secs_f64());
    if !status.success() {
        let stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
        cleanup();
        return Err(format!("kicad-cli pcb export glb failed: {stderr}"));
    }
    let bytes = match std::fs::read(&glb_path) {
        Ok(b) => b,
        Err(e) => {
            cleanup();
            return Err(format!("kicad-cli did not produce a readable .glb: {e}"));
        }
    };
    cleanup();
    Ok(bytes)
}

fn mime_of(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        _ => "application/octet-stream",
    }
}

fn handle(
    stream: &mut TcpStream,
    dir: &Path,
    job: &Job,
    schematic: &Mutex<Option<(std::time::SystemTime, String)>>,
    glb_job: &GlbJob,
    ui_root: Option<&Path>,
) -> Result<(), String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).map_err(|e| e.to_string())?;
    let mut parts = request_line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let mut length = 0usize;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).map_err(|e| e.to_string())? == 0 || header == "\r\n" || header == "\n" {
            break;
        }
        if let Some((k, v)) = header.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                length = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0u8; length.min(1 << 20)];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    let path = target.split('?').next().unwrap_or("/");

    match (method, path) {
        ("GET", "/") => match ui_root {
            Some(root) => serve_file(stream, root, "index.html"),
            None => respond(stream, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes()),
        },
        ("GET", "/api/version") => respond(stream, "200 OK", "application/json", version(dir, job).to_string().as_bytes()),
        ("GET", "/api/state") => {
            let v = state(dir, job).unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
            respond(stream, "200 OK", "application/json", v.to_string().as_bytes())
        }
        ("GET", "/api/schematic.svg") => match schematic_svg(dir, schematic) {
            Ok(svg) => respond(stream, "200 OK", "image/svg+xml", svg.as_bytes()),
            Err(e) => respond(stream, "404 Not Found", "text/plain", board::reasons(&e).as_bytes()),
        },
        ("GET", "/api/3dmodel") => {
            let query = target.split('?').nth(1).unwrap_or("");
            let name = query.split('&').find_map(|kv| kv.strip_prefix("name=")).unwrap_or("");
            serve_3dmodel(stream, name)
        }
        ("GET", "/api/schematic") => {
            let v = schematic_json(dir).unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
            respond(stream, "200 OK", "application/json", v.to_string().as_bytes())
        }
        ("GET", "/api/board.glb") => serve_board_glb(stream, dir, job, glb_job),
        ("GET", "/api/drc") => {
            let v = drc_json(dir).unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
            respond(stream, "200 OK", "application/json", v.to_string().as_bytes())
        }
        ("GET", "/api/erc") => {
            let v = erc_json(dir).unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
            respond(stream, "200 OK", "application/json", v.to_string().as_bytes())
        }
        ("GET", "/api/ratsnest") => {
            let v = ratsnest_json(dir).unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
            respond(stream, "200 OK", "application/json", v.to_string().as_bytes())
        }
        ("POST", "/api/cmd") => {
            let req: Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
            let strict = req.get("strict").and_then(Value::as_bool).unwrap_or(true);
            let reply = match serde_json::from_value::<eda_ops::Cmd>(req.get("cmd").cloned().unwrap_or(Value::Null)) {
                Err(e) => json!({ "ok": false, "message": format!("not a board command: {e}") }),
                Ok(cmd) => match board::step(dir, cmd, strict, "ui") {
                    Ok(summary) => json!({ "ok": true, "message": summary }),
                    Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
                },
            };
            respond(stream, "200 OK", "application/json", reply.to_string().as_bytes())
        }
        ("POST", "/api/undo") => {
            let scope = request_domain(&body);
            let reply = match board::undo(dir, "ui", scope) {
                Ok(summary) => json!({ "ok": true, "message": summary }),
                Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
            };
            respond(stream, "200 OK", "application/json", reply.to_string().as_bytes())
        }
        ("POST", "/api/redo") => {
            let scope = request_domain(&body);
            let reply = match board::redo(dir, "ui", scope) {
                Ok(summary) => json!({ "ok": true, "message": summary }),
                Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
            };
            respond(stream, "200 OK", "application/json", reply.to_string().as_bytes())
        }
        ("POST", "/api/route") => {
            let mut j = job.lock().map_err(|e| e.to_string())?;
            if *j == "running" {
                return respond(stream, "409 Conflict", "application/json", json!({ "ok": false, "message": "already routing" }).to_string().as_bytes());
            }
            *j = "running".into();
            drop(j);
            let (dir, job) = (dir.to_path_buf(), job.clone());
            std::thread::spawn(move || {
                let end = match board::route_board(&dir, "ui") {
                    Ok(s) => s,
                    Err(e) => format!("route failed: {}", board::reasons(&e)),
                };
                if let Ok(mut j) = job.lock() {
                    *j = end;
                }
            });
            respond(stream, "200 OK", "application/json", json!({ "ok": true, "message": "routing" }).to_string().as_bytes())
        }
        ("GET", p) if ui_root.is_some() && !p.starts_with("/api/") => serve_file(stream, ui_root.unwrap(), p.trim_start_matches('/')),
        _ => respond(stream, "404 Not Found", "text/plain", b"not found"),
    }
}

/// `POST /api/undo`/`/api/redo`'s own `{"domain": "pcb" | "schematic"}`
/// body -- which tab asked, so `board::undo`/`redo` can scope to it
/// (GAPS.md #15: Ctrl+Z on the Schematic tab must never revert the PCB
/// tab's last edit, or vice versa). Absent, unparseable, or a genuinely
/// empty body (the CLI never sends one; nothing else hits this route)
/// falls back to `None` -- the original, unscoped "most recent edit of
/// any kind" behavior -- rather than refusing the request.
fn request_domain(body: &[u8]) -> Option<eda_ops::Domain> {
    let req: Value = serde_json::from_slice(body).ok()?;
    match req.get("domain").and_then(Value::as_str)? {
        "schematic" => Some(eda_ops::Domain::Schematic),
        _ => Some(eda_ops::Domain::Pcb),
    }
}

fn respond(stream: &mut TcpStream, status: &str, kind: &str, body: &[u8]) -> Result<(), String> {
    let head = format!("HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len());
    stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(body)).map_err(|e| e.to_string())
}

fn stamp(p: PathBuf) -> u128 {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos())
}

/// Changes whenever the board, its activity or the routing job does, so
/// the page knows to fetch the state again.
fn version_string(dir: &Path, job: &Job) -> String {
    let j = job.lock().map(|j| j.clone()).unwrap_or_default();
    format!("{}-{}-{}", stamp(dir.join("design.json")), stamp(dir.join("activity.jsonl")), j.len() + j.bytes().map(|b| b as usize).sum::<usize>())
}

fn version(dir: &Path, job: &Job) -> Value {
    json!(version_string(dir, job))
}

fn state(dir: &Path, job: &Job) -> Result<Value, Vec<CheckResult>> {
    let (meta, design, model) = board::load(dir)?;
    let b = eda_ops::Board::new(design.clone(), &model, meta.snap_um, meta.spacing_um);
    let view = eda_ops::view::view(&b, &model)?;
    let block_of: std::collections::HashMap<String, String> = view["parts"]
        .as_array()
        .map(|ps| ps.iter().filter_map(|p| Some((p["ref"].as_str()?.to_string(), p["block"].as_str().unwrap_or("").to_string()))).collect())
        .unwrap_or_default();
    let net_of: std::collections::HashMap<&str, &str> = model.nets.iter().flat_map(|n| n.pins.iter().map(move |p| (p.as_str(), n.name.as_str()))).collect();
    let pl = design.placement.as_ref();
    let mut parts = Vec::new();
    for part in &model.parts {
        let fp = pl.and_then(|pl| pl.footprints.iter().find(|f| f.id == part.reference));
        let size = model.footprint_of(part).map(|f| f.courtyard_half()).map(|(w, h)| [w * 2, h * 2]);
        let mut p = json!({
            "ref": part.reference,
            "value": part.value,
            "package": part.package,
            "mpn": part.mpn,
            "block": block_of.get(&part.reference),
            "placed": fp.is_some(),
            "size": size,
        });
        if let Some(fp) = fp {
            let pads: Vec<Value> = placed_pads(&model, part, fp)
                .unwrap_or_default()
                .iter()
                .map(|q| {
                    let net = net_of.get(format!("{}.{}", part.reference, q.number).as_str()).copied();
                    json!({ "num": q.number, "net": net, "x": q.center.x, "y": q.center.y, "w": q.size.0, "h": q.size.1, "round": q.is_round(), "th": q.through_hole })
                })
                .collect();
            p["at"] = json!([fp.at.x, fp.at.y]);
            p["rot"] = json!(fp.rot as f64 / 1000.0);
            p["side"] = json!(if fp.side == Side::Bottom { "bottom" } else { "top" });
            p["label"] = json!(match fp.label {
                LabelSide::Above => "above",
                LabelSide::Below => "below",
                LabelSide::Left => "left",
                LabelSide::Right => "right",
            });
            p["courtyard"] = json!(placed_courtyard(&model, part, fp).map(|c| [c.0, c.1, c.2, c.3]));
            p["pads"] = json!(pads);
        }
        parts.push(p);
    }
    let mut checks: Vec<&CheckResult> = Vec::new();
    let placement_checks = b.checks();
    let routing_checks = if design.routing.is_some() { eda_gates::check_routing(&design, &model) } else { Vec::new() };
    checks.extend(placement_checks.iter().chain(routing_checks.iter()).filter(|c| !matches!(c.status, CheckStatus::Pass)));
    let checks: Vec<Value> = checks
        .iter()
        .map(|c| json!({ "check": c.check, "fail": matches!(c.status, CheckStatus::Fail), "at": c.location, "hint": c.hint }))
        .collect();
    let activity: Vec<Value> = std::fs::read_to_string(dir.join("activity.jsonl"))
        .unwrap_or_default()
        .lines()
        .rev()
        .take(60)
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let routing = design.routing.as_ref().map(|r| {
        json!({
            "tracks": r.tracks.iter().map(|t| json!({
                "id": t.id, "net": t.net, "layer": t.layer, "width": t.width,
                "pts": t.pts.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "vias": r.vias.iter().map(|v| json!({
                "id": v.id, "net": v.net, "x": v.at.x, "y": v.at.y, "d": v.diameter,
                "drill": v.drill, "from": v.from_layer, "to": v.to_layer,
            })).collect::<Vec<_>>(),
            "zones": r.zones.iter().map(|z| json!({
                "id": z.id, "net": z.net, "layer": z.layer,
                "outline": z.outline.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    });
    let drawings = design.drawings.as_ref().map(|d| {
        json!({
            "shapes": d.shapes.iter().map(shape_json).collect::<Vec<_>>(),
            "texts": d.texts.iter().map(|t| json!({
                "id": t.id, "content": t.content, "x": t.at.x, "y": t.at.y, "angle": t.angle,
                "layer": t.layer, "size": t.size_um, "stroke_width": t.stroke_width,
                "justify": match t.justify {
                    eda_model::ir::TextJustify::Left => "left",
                    eda_model::ir::TextJustify::Center => "center",
                    eda_model::ir::TextJustify::Right => "right",
                },
                "mirror": t.mirror,
            })).collect::<Vec<_>>(),
        })
    });
    Ok(json!({
        "name": Path::new(&meta.intent).file_stem().and_then(|s| s.to_str()).unwrap_or("board"),
        "dir": dir.display().to_string(),
        "outline": pl.map(|pl| pl.outline.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>()),
        "layers": model.board.layers,
        "snap": meta.snap_um,
        "parts": parts,
        "rules": model.placement_rules,
        // Read-only board-wide defaults for the auxiliary toolbar's
        // track-width/via-size indicators. `/api/cmd` takes any `Cmd` as
        // JSON, including `add_track`/`add_via`, which can draw copper at
        // other widths/sizes than these -- these are only the defaults a
        // new track/via would start from.
        "board_rules": {
            "track_width": model.board.track_width,
            "via_drill": model.board.via_drill,
            "via_diameter": model.board.via_diameter,
            "clearance": model.board.clearance,
        },
        "routing": routing,
        "drawings": drawings,
        "checks": checks,
        "activity": activity,
        "job": job.lock().map(|j| j.clone()).unwrap_or_default(),
    }))
}

/// One `Shape` as JSON: every variant carries `id`/`kind`/`layer`/
/// `stroke_width`/`filled` plus its own geometry, points as `[x, y]` pairs
/// -- the same convention `outline`/`pts` already use elsewhere in this
/// API, so a `(add|move|delete)_shape` command and this read use the same
/// shape.
fn shape_json(s: &Shape) -> Value {
    let pt = |p: eda_model::ir::Point| json!([p.x, p.y]);
    match s {
        Shape::Segment { id, layer, stroke_width, filled, start, end } => {
            json!({ "id": id, "kind": "segment", "layer": layer, "stroke_width": stroke_width, "filled": filled, "start": pt(*start), "end": pt(*end) })
        }
        Shape::Arc { id, layer, stroke_width, filled, start, mid, end } => {
            json!({ "id": id, "kind": "arc", "layer": layer, "stroke_width": stroke_width, "filled": filled, "start": pt(*start), "mid": pt(*mid), "end": pt(*end) })
        }
        Shape::Rect { id, layer, stroke_width, filled, start, end } => {
            json!({ "id": id, "kind": "rect", "layer": layer, "stroke_width": stroke_width, "filled": filled, "start": pt(*start), "end": pt(*end) })
        }
        Shape::Circle { id, layer, stroke_width, filled, center, end } => {
            json!({ "id": id, "kind": "circle", "layer": layer, "stroke_width": stroke_width, "filled": filled, "center": pt(*center), "end": pt(*end) })
        }
        Shape::Polygon { id, layer, stroke_width, filled, pts } => {
            json!({ "id": id, "kind": "polygon", "layer": layer, "stroke_width": stroke_width, "filled": filled, "pts": pts.iter().map(|p| pt(*p)).collect::<Vec<_>>() })
        }
    }
}

/// The design's schematic drawn, or, for a board started from an intent
/// alone, one derived from the intent (cached until the intent changes).
fn schematic_svg(dir: &Path, cache: &Mutex<Option<(std::time::SystemTime, String)>>) -> Result<String, Vec<CheckResult>> {
    let (meta, design, model) = board::load(dir)?;
    if design.schematic.is_some() {
        return eda::render_schematic(&design, &model);
    }
    let when = std::fs::metadata(&meta.intent).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
    if let Ok(c) = cache.lock() {
        if let Some((t, svg)) = c.as_ref() {
            if *t == when {
                return Ok(svg.clone());
            }
        }
    }
    let derived = eda::prelude::derive_schematic(&model, &eda::prelude::EngineOptions::default())?;
    let svg = eda::render_schematic(&derived, &model)?;
    if let Ok(mut c) = cache.lock() {
        *c = Some((when, svg.clone()));
    }
    Ok(svg)
}

/// The same schematic `schematic_svg` draws, but as structured JSON for
/// the browser UI's own KiCad-style renderer instead of one baked image:
/// each symbol instance plus its part's pins (a `SymbolInstance` alone
/// says nothing about what it draws), every wire, and every net label.
/// No second cache next to `schematic_svg`'s -- these boards are small,
/// so deriving again on a cache miss costs nothing worth guarding.
fn schematic_json(dir: &Path) -> Result<Value, Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    let sch = match design.schematic {
        Some(s) => s,
        None => {
            eda::prelude::derive_schematic(&model, &eda::prelude::EngineOptions::default())?.schematic.unwrap_or(eda_model::ir::SchematicSection {
                power_symbols: vec![],
                no_connects: vec![],
                title_block: None,
                sheets: vec![],
                symbols: Vec::new(),
                wires: Vec::new(),
                labels: Vec::new(),
                texts: Vec::new(),
            })
        }
    };
    let symbols: Vec<Value> = sch
        .symbols
        .iter()
        .map(|s| {
            let part = model.part(&s.id);
            json!({
                "id": s.id,
                "at": [s.at.x, s.at.y],
                // Millideg -> plain degrees, same convention `state()` uses for a PCB part's `rot`.
                "rot": s.rot as f64 / 1000.0,
                "mirrored": s.mirrored,
                // KiCad library id this instance draws from ("Device:R", or
                // the synthetic "eda:<id>" for a part with no resolved real
                // symbol) -- key into "lib_symbols" below for its graphics.
                "lib_id": s.lib_id,
                "value": if s.value.is_empty() { part.and_then(|p| p.value.clone()) } else { Some(s.value.clone()) },
                "footprint": if s.footprint.is_empty() { None } else { Some(s.footprint.clone()) },
                "datasheet": if s.datasheet.is_empty() { None } else { Some(s.datasheet.clone()) },
                "mpn": part.and_then(|p| p.mpn.clone()),
                "package": part.and_then(|p| p.package.clone()),
                "pins": part.map(|p| p.pins.iter().map(|pin| json!({ "number": pin.number, "name": pin.name, "kind": pin.kind })).collect::<Vec<_>>()).unwrap_or_default(),
            })
        })
        .collect();
    let wires: Vec<Value> = sch.wires.iter().map(|w| json!({ "id": w.id, "net": w.net, "pins": w.pins, "pts": w.pts.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>() })).collect();
    let labels: Vec<Value> = sch
        .labels
        .iter()
        .map(|l| {
            let (scope, shape) = match &l.kind {
                eda_model::ir::LabelKind::Local => ("local", None),
                eda_model::ir::LabelKind::Global { shape } => ("global", Some(*shape)),
                eda_model::ir::LabelKind::Hierarchical { shape } => ("hierarchical", Some(*shape)),
            };
            json!({ "id": l.id, "net": l.net, "at": [l.at.x, l.at.y], "scope": scope, "shape": shape.map(label_shape_str) })
        })
        .collect();
    let texts: Vec<Value> = sch
        .texts
        .iter()
        .map(|t| json!({ "id": t.id, "content": t.content, "at": [t.at.x, t.at.y], "angle": t.angle as f64 / 1000.0, "size_um": t.size_um }))
        .collect();
    let power_symbols: Vec<Value> = sch
        .power_symbols
        .iter()
        .map(|p| json!({ "id": p.id, "lib_id": p.lib_id, "at": [p.at.x, p.at.y], "rot": p.rot as f64 / 1000.0, "net": p.net, "pin": p.pin }))
        .collect();
    let no_connects: Vec<Value> = sch.no_connects.iter().map(|nc| json!({ "id": nc.id, "at": [nc.at.x, nc.at.y], "pin": nc.pin })).collect();
    let title_block = sch.title_block.as_ref().map(|t| json!({ "title": t.title, "date": t.date, "rev": t.rev, "company": t.company, "comments": t.comments }));

    // Resolved library-symbol graphics for every distinct lib_id this sheet
    // uses, so the frontend can draw KiCad's actual symbols instead of a
    // generic box -- the same resolution `export_kicad_sch` does, exposed
    // here as its own small step so studio.rs's other owners are
    // unaffected by it. Real installed libraries first (already resolved
    // into `model.symbols` at board-load time), then
    // `eda_model::symbol::builtin`, then a per-part synthesized generic box
    // for anything neither covers.
    let mut lib_ids: Vec<String> = sch.symbols.iter().map(|s| if s.lib_id.is_empty() { format!("eda:{}", s.id) } else { s.lib_id.clone() }).collect();
    lib_ids.extend(sch.power_symbols.iter().map(|p| p.lib_id.clone()));
    lib_ids.sort();
    lib_ids.dedup();
    let lib_symbols: Value = lib_ids
        .iter()
        .map(|lib_id| {
            let resolved = if eda_model::is_synthetic_lib_id(lib_id) { None } else { model.symbol_of(lib_id) };
            let value = resolved.unwrap_or_else(|| synthesize_generic_symbol(lib_id, &model));
            (lib_id.clone(), lib_symbol_json(&value))
        })
        .collect::<serde_json::Map<_, _>>()
        .into();

    Ok(json!({
        "symbols": symbols,
        "wires": wires,
        "labels": labels,
        "texts": texts,
        "power_symbols": power_symbols,
        "no_connects": no_connects,
        "title_block": title_block,
        "lib_symbols": lib_symbols,
    }))
}

fn label_shape_str(s: eda_model::ir::LabelShape) -> &'static str {
    use eda_model::ir::LabelShape;
    match s {
        LabelShape::Input => "input",
        LabelShape::Output => "output",
        LabelShape::Bidirectional => "bidirectional",
        LabelShape::TriState => "tri_state",
        LabelShape::Passive => "passive",
    }
}

/// A synthesized generic box for a part with no real/built-in library
/// symbol (most multi-pin ICs), described in `eda_model::LibSymbol`'s own
/// shape so the frontend has one uniform schema for every symbol
/// regardless of where it came from. Deliberately *not* the exact
/// box/port layout `derive_schematic`/`export_kicad_sch` compute (that
/// algorithm lives in `eda-engine`/`eda-layout`, crates this binary does
/// not otherwise depend on, and this JSON view is a visual reference for
/// the frontend, not the authoritative pin geometry -- that authority is
/// the exported `.kicad_sch` itself): pins are simply spread evenly,
/// N/S/E/W, in ascending pin-number order. `"eda:<id>"` names exactly one
/// part (unlike a real lib_id, which can be shared), so this always has a
/// `Part` to build from.
pub(crate) fn synthesize_generic_symbol(lib_id: &str, model: &eda_model::ConstraintModel) -> eda_model::LibSymbol {
    let empty = || eda_model::LibSymbol { lib_id: lib_id.to_string(), graphics: vec![], pins: vec![], power: false, in_bom: true, on_board: true, datasheet: String::new(), description: String::new(), unit_count: 1 };
    let reference = lib_id.strip_prefix("eda:").unwrap_or(lib_id);
    let Some(part) = model.part(reference) else { return empty() };
    let wireable: Vec<&eda_model::Pin> = part.pins.iter().filter(|p| p.kind != eda_model::PinKind::Nc).collect();
    if wireable.is_empty() {
        return empty();
    }
    // A grid box, sized so a pin-per-side pitch of 2.54mm never crowds:
    // split the wireable pins into 4 sides round-robin (N,E,S,W,N,E,...).
    let mut sides: [Vec<&eda_model::Pin>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    for (i, p) in wireable.iter().enumerate() {
        sides[i % 4].push(p);
    }
    let pitch = 2.54;
    let side_len = |n: usize| (n.max(1) + 1) as f64 * pitch;
    let width = side_len(sides[0].len().max(sides[2].len())).max(5.08);
    let height = side_len(sides[1].len().max(sides[3].len())).max(5.08);
    let graphics = vec![eda_model::SymbolGraphic::Rectangle { unit: 1, start: eda_model::symbol::SPoint::new(0.0, 0.0), end: eda_model::symbol::SPoint::new(width, -height), stroke_mm: 0.254, filled: false }];

    let etype = |k: eda_model::PinKind| match k {
        eda_model::PinKind::Power | eda_model::PinKind::Ground => "power_in",
        eda_model::PinKind::Signal => "bidirectional",
        eda_model::PinKind::Passive => "passive",
        eda_model::PinKind::Nc => "no_connect",
    };
    let mut pins = Vec::new();
    // (side pins, fixed axis value, varying-axis span, angle, point builder)
    let place = |i: usize, n: usize, span: f64| (i as f64 + 1.0) / (n as f64 + 1.0) * span;
    for (i, p) in sides[0].iter().enumerate() {
        pins.push((p, eda_model::symbol::SPoint::new(place(i, sides[0].len(), width), 0.0), 90.0)); // North
    }
    for (i, p) in sides[1].iter().enumerate() {
        pins.push((p, eda_model::symbol::SPoint::new(width, -place(i, sides[1].len(), height)), 180.0)); // East
    }
    for (i, p) in sides[2].iter().enumerate() {
        pins.push((p, eda_model::symbol::SPoint::new(place(i, sides[2].len(), width), -height), 270.0)); // South
    }
    for (i, p) in sides[3].iter().enumerate() {
        pins.push((p, eda_model::symbol::SPoint::new(0.0, -place(i, sides[3].len(), height)), 0.0)); // West
    }
    let lib_pins: Vec<eda_model::LibPin> = pins
        .into_iter()
        .map(|(p, at, angle_deg)| eda_model::LibPin { number: p.number.clone(), name: p.name.clone().unwrap_or_default(), electrical_type: etype(p.kind).to_string(), shape: "line".to_string(), at, angle_deg, length_mm: 1.27, unit: 1 })
        .collect();
    eda_model::LibSymbol { lib_id: lib_id.to_string(), graphics, pins: lib_pins, power: false, in_bom: true, on_board: true, datasheet: String::new(), description: String::new(), unit_count: 1 }
}

/// One `LibSymbol` as JSON: graphics/pins in the symbol's own local frame,
/// millimetres, +y **up** (KiCad's own library convention, not this API's
/// usual +y-down sheet millimetres) -- drawing it in sheet space needs the
/// same negate-y-then-rotate-then-mirror composition
/// `eda_kicad::lib::baked_local`'s doc comment spells out.
fn lib_symbol_json(s: &eda_model::LibSymbol) -> Value {
    let pt = |p: eda_model::symbol::SPoint| json!([p.x, p.y]);
    let graphics: Vec<Value> = s
        .graphics
        .iter()
        .map(|g| {
            use eda_model::SymbolGraphic::*;
            match g {
                Rectangle { unit, start, end, stroke_mm, filled } => json!({ "kind": "rectangle", "unit": unit, "start": pt(*start), "end": pt(*end), "stroke_mm": stroke_mm, "filled": filled }),
                Polyline { unit, pts, stroke_mm, filled } => json!({ "kind": "polyline", "unit": unit, "pts": pts.iter().map(|p| pt(*p)).collect::<Vec<_>>(), "stroke_mm": stroke_mm, "filled": filled }),
                Circle { unit, center, radius_mm, stroke_mm, filled } => json!({ "kind": "circle", "unit": unit, "center": pt(*center), "radius_mm": radius_mm, "stroke_mm": stroke_mm, "filled": filled }),
                Arc { unit, start, mid, end, stroke_mm, filled } => json!({ "kind": "arc", "unit": unit, "start": pt(*start), "mid": pt(*mid), "end": pt(*end), "stroke_mm": stroke_mm, "filled": filled }),
                Text { unit, text, at, angle_deg, size_mm } => json!({ "kind": "text", "unit": unit, "text": text, "at": pt(*at), "angle_deg": angle_deg, "size_mm": size_mm }),
            }
        })
        .collect();
    let pins: Vec<Value> = s
        .pins
        .iter()
        .map(|p| json!({ "number": p.number, "name": p.name, "electrical_type": p.electrical_type, "shape": p.shape, "at": pt(p.at), "angle_deg": p.angle_deg, "length_mm": p.length_mm, "unit": p.unit }))
        .collect();
    json!({ "power": s.power, "graphics": graphics, "pins": pins, "datasheet": s.datasheet, "description": s.description })
}

/// `GET /api/drc`: the ported KiCad design-rule checker (`eda_drc`), run
/// fresh on the board's current design/model -- no caching, since DRC is
/// cheap enough on these board sizes to just run on every request the same
/// way `/api/state`'s own gate checks do.
///
/// Shaped like `kicad-cli pcb drc --format json`'s own report (`type`/
/// `description`/`severity`/`items`), for the React DRC dialog this feeds
/// and for anyone cross-checking against the real oracle by eye. Two
/// deliberate differences: positions are this API's own µm integers (every
/// other endpoint here -- `pads`, `routing.tracks`, `outline` -- already
/// uses board-space µm, not kicad-cli's millimetres), each as `[x, y]`
/// rather than kicad-cli's own `{x, y}` mm object; and an extra `fix` key
/// (`null` when absent) carrying the placement-quality providers' agent-fix
/// metadata (see `eda_drc::FixHint`), which kicad-cli's own JSON has no
/// concept of.
/// GET /api/erc: `eda_kicad::check_erc` (the ERC engine, gap #4 in
/// GAPS.md -- "built, zero UI exposure") run fresh on every call, same
/// no-caching reasoning `drc_json`'s own doc gives. Flatter than
/// `drc_json`'s `DrcViolation`/items shape: `check_erc` reports plain
/// `CheckResult`s (check name, Fail/Warn, an optional "REF" or "REF.PIN"
/// `location` string, a human `hint`) rather than DRC's richer
/// positioned-item list, so there is no canvas-marker position to extract
/// here the way DRC's `it.pos` gives one -- `ErcDialog.tsx`'s row click
/// resolves `location` back to a symbol/pin to select instead. `Excluded`
/// results are dropped (nothing in this app writes to `Exclusions` yet, so
/// there is never one to show); `Pass` is never present in `check_erc`'s
/// own output in the first place.
fn erc_json(dir: &Path) -> Result<Value, Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    let found = eda_kicad::check_erc(&design, &model);
    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    let violations: Vec<Value> = found
        .iter()
        .filter(|c| !matches!(c.status, CheckStatus::Pass | CheckStatus::Excluded))
        .map(|c| {
            *counts.entry(c.check.as_str()).or_default() += 1;
            json!({
                "check": c.check,
                "severity": match c.status { CheckStatus::Fail => "error", _ => "warning" },
                "location": c.location,
                "hint": c.hint,
            })
        })
        .collect();
    Ok(json!({ "violations": violations, "counts": counts }))
}

fn drc_json(dir: &Path) -> Result<Value, Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    let found = eda_drc::run(&design, &model);
    let counts = eda_drc::counts_by_type(&found);
    let violations: Vec<Value> = found
        .iter()
        .map(|v| {
            json!({
                "type": v.error_type,
                "description": v.description,
                "severity": match v.severity { eda_drc::Severity::Error => "error", eda_drc::Severity::Warning => "warning" },
                "items": v.items.iter().map(|it| json!({
                    "description": it.description,
                    "pos": [it.pos.0, it.pos.1],
                    "id": it.id,
                })).collect::<Vec<_>>(),
                "fix": v.fix,
            })
        })
        .collect();
    Ok(json!({ "violations": violations, "counts": counts }))
}

/// `GET /api/ratsnest`: the board's airwires for the React view's ratsnest
/// display -- every still-missing copper connection, one line per pair,
/// `from`/`to` in board-space micrometers like every other endpoint here.
/// Computed by `eda_connectivity` (KiCad's `CN_CONNECTIVITY_ALGO`/`RN_NET`
/// ported; see `crates/connectivity`), which does its own from-scratch
/// pass over `design`/`model` each call -- no caching, same as every other
/// read here.
fn ratsnest_json(dir: &Path) -> Result<Value, Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    let report = eda_connectivity::analyze(&design, &model);
    let edges: Vec<Value> = report.ratsnest.iter().map(|e| json!({ "net": e.net, "from": [e.from.x, e.from.y], "to": [e.to.x, e.to.y] })).collect();
    Ok(json!({ "edges": edges }))
}
