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
//! was fixed to run in the background instead. The rest of kicad-cli's
//! work (DRC, ERC, board statistics, every plot and export) is the same
//! kind of slow: it runs on threads of its own too (`offload`, and
//! `crate::kicad_lane`, which keeps it to one kicad-cli at a time), so
//! an edit or the /api/version poll never waits for one.

use crate::board;
use crate::bom_plugins;
use crate::cleanup_api;
use crate::convert_api;
use crate::fab_api;
use crate::kicad_lane::Lane;
use crate::route_api;
use crate::sch_api;
use crate::sch_export_api;
use crate::sch_move_api;
use crate::sch_output_api;
use crate::tune_api;
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
    // The in-progress interactive-router session (gap #7), if any -- lives
    // only in memory for the span of one route/drag gesture; see
    // `route_api`'s own doc comment for why it's safe to hold across
    // requests without re-reading the board each time.
    let route_session: crate::route_api::RouteCell = Mutex::new(None);
    // Where DRC, ERC, statistics and the exports run -- see `offload`.
    let lane = Arc::new(Lane::default());
    // A browser opens connections ahead of the requests it will send on them (Chrome's preconnect) and this loop reads one request at a time: a connection that has
    // said nothing yet held every request behind it until the browser closed it -- 16 s, measured, with the 3D models of a board waiting. Each connection is
    // waited on by a thread of its own and joins the loop when the first bytes of its request are there.
    // The 3D model routes need nothing of the loop, so the connection's own thread answers them too: a model is not made to wait behind a slow `/api/state`.
    let (ready, requests) = std::sync::mpsc::channel::<TcpStream>();
    let (model_dir, model_lane) = (dir.to_path_buf(), lane.clone());
    std::thread::Builder::new()
        .name("accept".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let (ready, dir, lane) = (ready.clone(), model_dir.clone(), model_lane.clone());
                std::thread::spawn(move || {
                    let mut head = [0u8; 32];
                    let n = stream.peek(&mut head).unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    if head[..n].starts_with(b"GET /api/3dmodel") || head[..n].starts_with(b"POST /api/3dmodel/") {
                        let done = read_request(&stream).and_then(|(method, target, body)| model_routes(&mut stream, &method, &target, &body, &dir, &lane, true).unwrap_or(Err("not a model route".into())));
                        if let Err(e) = done {
                            let _ = respond(&mut stream, "500 Internal Server Error", "text/plain", e.as_bytes());
                        }
                        return;
                    }
                    let _ = ready.send(stream);
                });
            }
        })
        .map_err(|e| vec![CheckResult::fail("serve_thread", "accept", e.to_string())])?;
    for mut stream in requests {
        if let Err(e) = handle(&mut stream, dir, &job, &schematic, &glb_job, &route_session, ui_root.as_deref(), &lane) {
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

/// Where the real `kicad-cli` binary lives -- `EDA_KICAD_CLI` if set
/// (it is not on PATH in this dev environment), else KiCad's own
/// default macOS install location. Same "env var, then a sane default"
/// shape as `ui_dir`.
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
fn serve_board_glb(stream: &mut TcpStream, dir: &Path, job: &Job, glb: &GlbJob, retry: bool) -> Result<(), String> {
    let ver = version_string(dir, job);
    let mut guard = glb.lock().map_err(|e| e.to_string())?;
    // `?retry=1`, the 3D viewer's "Reload board": forget the finished answer for this version -- a failure is otherwise kept until the board changes -- so
    // this request builds again. A build already running is left to finish.
    if retry && matches!(guard.as_ref(), Some((v, GlbBuild::Done(_) | GlbBuild::Failed(_))) if *v == ver) {
        *guard = None;
    }
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

/// The 3D view shows the copper the board HAS, whatever its netlist says. A track, via or pour on a net the netlist does not name (any more) makes the
/// exporter refuse the whole board -- copper on KiCad's "net 0" is a different board for DRC, which is why it refuses -- and the 3D tab then fell back to its
/// placeholder boxes for a board whose only fault was a net that was renamed after it was routed. For this export alone such nets are added to the netlist,
/// without pins, so the copper is drawn where it is; every other export (DRC, fabrication) keeps refusing.
fn ensure_routed_nets(design: &eda_model::ir::Design, model: &mut eda_model::ConstraintModel) {
    let Some(routing) = &design.routing else { return };
    let known: std::collections::BTreeSet<&str> = model.nets.iter().map(|n| n.name.as_str()).collect();
    let used = routing.tracks.iter().map(|t| t.net.as_str()).chain(routing.vias.iter().map(|v| v.net.as_str())).chain(routing.zones.iter().map(|z| z.net.as_str()));
    let missing: std::collections::BTreeSet<String> = used.filter(|n| !n.is_empty() && !known.contains(n)).map(String::from).collect();
    for name in missing {
        model.nets.push(eda_model::Net { name, pins: vec![] });
    }
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
    let (meta, design, mut model) = board::load(dir).map_err(|e| board::reasons(&e))?;
    ensure_routed_nets(&design, &mut model);
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

/// Runs `work` -- anything that starts kicad-cli -- off the request loop, and
/// answers `stream` with its reply (a JSON object, stamped with the `revision`
/// it ran on) when it is done. Returns at once, so the loop goes on to the next
/// request while kicad-cli runs: an edit, the /api/version poll and the 3D view
/// never wait for it. `route` and `body` name the request: the same one on the
/// same revision joins the run already going instead of starting another
/// kicad-cli, and a different one waits its turn -- one kicad-cli at a time
/// (`crate::kicad_lane`). A new route that starts kicad-cli goes through here:
/// answered inline it holds up the whole loop again (a test in `kicad_lane`
/// fails when a kicad-cli route is not an `offload` arm).
fn offload(stream: &TcpStream, lane: &Arc<Lane>, dir: &Path, job: &Job, route: &str, body: &[u8], work: impl FnOnce(&Path, &[u8]) -> Value + Send + 'static) -> Result<(), String> {
    // The loop drops its own handle on `stream` when it moves on; this one keeps the connection open for the answer.
    let mut out = stream.try_clone().map_err(|e| e.to_string())?;
    let what = format!("{route} {}", String::from_utf8_lossy(body));
    let (lane, dir, job, body) = (lane.clone(), dir.to_path_buf(), job.clone(), body.to_vec());
    std::thread::Builder::new()
        .name("kicad-cli".into())
        .spawn(move || {
            let reply = lane.run(&what, || version_string(&dir, &job), || work(&dir, &body));
            let _ = respond(&mut out, "200 OK", "application/json", reply.to_string().as_bytes());
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// One request off a connection: its method, its target (path and query) and its body.
fn read_request(stream: &TcpStream) -> Result<(String, String, Vec<u8>), String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).map_err(|e| e.to_string())?;
    let mut parts = request_line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("/").to_string());
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
    Ok((method, target, body))
}

/// The 3D model routes (crates/cli/src/model3d_api.rs), or `None` for any other request. They need nothing of the request loop, so a connection's own thread answers
/// them (`on_own_thread`, see `serve`): a model never waits behind a slow `/api/state`.
///
///   - `GET /api/3dmodel?name=..`: one footprint 3D model, read-only, for the 3D view's VRMLLoader: resolved like KiCad does and held to an allow-list; a STEP model is
///     converted to VRML by kicad-cli in the background and cached. `&wait=1` holds the request until the model is in.
///   - `POST /api/3dmodel/prepare`: every model of the board at once, queued for one conversion run.
fn model_routes(stream: &mut TcpStream, method: &str, target: &str, body: &[u8], dir: &Path, lane: &Arc<Lane>, on_own_thread: bool) -> Option<Result<(), String>> {
    match (method, target.split('?').next().unwrap_or("/")) {
        ("GET", "/api/3dmodel") => Some((|| {
            let store = crate::model3d_api::store(lane);
            if !crate::model3d_api::wants_wait(target) {
                let r = crate::model3d_api::reply(dir, target, &store);
                return respond(stream, r.status, r.kind, &r.body);
            }
            if on_own_thread {
                let r = crate::model3d_api::reply_waiting(dir, target, &store);
                return respond(stream, r.status, r.kind, &r.body);
            }
            // Held until the model is converted, on a thread of its own so the loop goes on (the same hand-off `offload` makes).
            let mut out = stream.try_clone().map_err(|e| e.to_string())?;
            let (dir, target) = (dir.to_path_buf(), target.to_string());
            std::thread::Builder::new()
                .name("3dmodel-wait".into())
                .spawn(move || {
                    let r = crate::model3d_api::reply_waiting(&dir, &target, &store);
                    let _ = respond(&mut out, r.status, r.kind, &r.body);
                })
                .map(|_| ())
                .map_err(|e| e.to_string())
        })()),
        ("POST", "/api/3dmodel/prepare") => Some(respond(stream, "200 OK", "application/json", crate::model3d_api::prepare(dir, body, &crate::model3d_api::store(lane)).to_string().as_bytes())),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)] // mirrors the existing job/schematic/glb_job cells this one more (route_session) joins.
fn handle(
    stream: &mut TcpStream,
    dir: &Path,
    job: &Job,
    schematic: &Mutex<Option<(std::time::SystemTime, String)>>,
    glb_job: &GlbJob,
    route_session: &crate::route_api::RouteCell,
    ui_root: Option<&Path>,
    lane: &Arc<Lane>,
) -> Result<(), String> {
    let (method, target, body) = read_request(stream)?;
    let (method, target) = (method.as_str(), target.as_str());
    let path = target.split('?').next().unwrap_or("/");
    if let Some(done) = model_routes(stream, method, target, &body, dir, lane, false) {
        return done;
    }

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
        ("GET", "/api/schematic") => {
            // `?sheet=<id>/<id>/...`: a `/`-joined path of `SheetInstance::id`s
            // from the root down to whichever sheet the Hierarchy panel has
            // currently navigated into (GAPS.md #6) -- omitted or empty means
            // the root sheet, exactly as every board before hierarchy support
            // existed already behaved.
            let query = target.split('?').nth(1).unwrap_or("");
            let sheet_path = query.split('&').find_map(|kv| kv.strip_prefix("sheet=")).unwrap_or("");
            let v = schematic_json(dir, sheet_path).unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
            respond(stream, "200 OK", "application/json", v.to_string().as_bytes())
        }
        // File > Save As... (`common.Control.saveAs`): the design as the derived KiCad files, for the browser to save.
        ("GET", "/api/board.kicad_pcb") => match kicad_pcb_text(dir) {
            Ok(text) => respond(stream, "200 OK", "text/plain; charset=utf-8", text.as_bytes()),
            Err(e) => respond(stream, "404 Not Found", "text/plain", board::reasons(&e).as_bytes()),
        },
        ("GET", "/api/schematic.kicad_sch") => {
            let v = kicad_sch_files_json(dir).unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
            respond(stream, "200 OK", "application/json", v.to_string().as_bytes())
        }
        ("GET", "/api/symbol_library") => {
            let v = symbol_library_json(dir).unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
            respond(stream, "200 OK", "application/json", v.to_string().as_bytes())
        }
        ("GET", "/api/board.glb") => serve_board_glb(stream, dir, job, glb_job, query_value(target, "retry") == "1"),
        // DRC and ERC are kicad-cli's (docs/ARCHITECTURE.md, "Engines"): the
        // current design.json revision is exported, kicad-cli runs (seconds),
        // and its report comes back mapped to our item ids, stamped with the
        // `revision` it was run on. They run off this loop (`offload`): the
        // request waits for its report and nothing else does -- the dialogs
        // show a running state, edits stay instant, and a report whose
        // `revision` is no longer /api/version's is shown as out of date. Our
        // own checks, the ones KiCad does not have, are `/api/lint`: cheap and
        // in-process, refreshed on every change.
        ("GET", "/api/drc") => {
            // `?refill_zones=1`: the DRC dialog's "Refill all zones before performing DRC". `&schematic_parity=1`: "Test for parity between PCB and schematic".
            let asked = |name: &str| target.split('?').nth(1).unwrap_or("").split('&').any(|kv| kv.split_once('=').is_some_and(|(k, v)| k == name && v == "1"));
            let (refill, parity) = (asked("refill_zones"), asked("schematic_parity"));
            offload(stream, lane, dir, job, &format!("{path} refill={refill} parity={parity}"), &[], move |dir, _| crate::kicad_engine::drc_with(dir, refill, parity).unwrap_or_else(|e| json!({ "error": board::reasons(&e) })))
        }
        ("GET", "/api/erc") => offload(stream, lane, dir, job, path, &[], |dir, _| crate::kicad_engine::erc(dir).unwrap_or_else(|e| json!({ "error": board::reasons(&e) }))),
        ("GET", "/api/lint") => {
            let v = crate::kicad_engine::lint(dir).unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
            respond(stream, "200 OK", "application/json", v.to_string().as_bytes())
        }
        // Symbol Fields Table / Find / ERC pin map backends: `crate::sch_api`.
        ("POST", "/api/sch/fields_table") => respond(stream, "200 OK", "application/json", sch_api::fields_table(dir, &body).to_string().as_bytes()),
        // Move, Drag, Rotate and Mirror previews: the commands applied in memory, the moved geometry back (`crate::sch_move_api`).
        ("POST", "/api/sch/move_preview") => respond(stream, "200 OK", "application/json", sch_move_api::move_preview(dir, &body).to_string().as_bytes()),
        ("POST", "/api/sch/bom_export") => respond(stream, "200 OK", "application/json", sch_api::bom_export(dir, &body).to_string().as_bytes()),
        ("POST", "/api/sch/find") => respond(stream, "200 OK", "application/json", sch_api::find(dir, &body).to_string().as_bytes()),
        ("GET", "/api/sch/erc_pin_map") => respond(stream, "200 OK", "application/json", sch_api::erc_pin_map(dir).to_string().as_bytes()),
        ("GET", "/api/sch/erc_severities") => respond(stream, "200 OK", "application/json", sch_api::erc_severities(dir).to_string().as_bytes()),
        // The schematic clipboard (Cut / Copy / Paste / Paste Special / Duplicate): the selection as KiCad's clipboard text, and a clipboard text as a paste.
        ("POST", "/api/sch/clipboard/copy") => respond(stream, "200 OK", "application/json", crate::sch_clipboard_api::copy(dir, &body).to_string().as_bytes()),
        ("POST", "/api/sch/clipboard/parse") => respond(stream, "200 OK", "application/json", crate::sch_clipboard_api::parse(dir, &body).to_string().as_bytes()),
        // The sheet tree (Next / Previous Sheet, Edit Sheet Page Number): `crate::sch_control_api`.
        ("GET", "/api/sch/hierarchy") => respond(stream, "200 OK", "application/json", crate::sch_control_api::hierarchy(dir).to_string().as_bytes()),
        // Export Symbols...: the library symbols the schematic uses as one `.kicad_sym` (a read; the browser saves it).
        ("POST", "/api/sch/export_symbols") => respond(stream, "200 OK", "application/json", crate::sch_control_api::export_symbols(dir, &body).to_string().as_bytes()),
        ("GET", "/api/ratsnest") => {
            let v = ratsnest_json(dir).unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
            respond(stream, "200 OK", "application/json", v.to_string().as_bytes())
        }
        ("GET", "/api/footprint") => {
            // Percent-decoded, like the symbol routes: the client sends `encodeURIComponent( name )`, and a `Lib:Name` arrives as `Lib%3AName`.
            let name = query_value(target, "name");
            let v = footprint_json(dir, &name).unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
            respond(stream, "200 OK", "application/json", v.to_string().as_bytes())
        }
        ("GET", "/api/footprint_library") => {
            let v = footprint_library_json(dir).unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
            respond(stream, "200 OK", "application/json", v.to_string().as_bytes())
        }
        ("GET", "/api/footprint/export") => {
            let name = query_value(target, "name");
            match footprint_kicad_mod(dir, &name) {
                Ok(text) => respond(stream, "200 OK", "text/plain; charset=utf-8", text.as_bytes()),
                Err(e) => respond(stream, "404 Not Found", "text/plain", board::reasons(&e).as_bytes()),
            }
        }
        // The Symbol Editor tab: same three-route shape the Footprint
        // Editor section just above already has (one document, one name
        // list, one derived-file export) -- see each function's own doc.
        ("GET", "/api/symbol") => {
            // A lib_id is `Library:Name`; the client sends it `encodeURIComponent`-escaped (`Device%3AR`).
            let lib_id = query_value(target, "lib_id");
            let v = symbol_edit_json(dir, &lib_id).unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
            respond(stream, "200 OK", "application/json", v.to_string().as_bytes())
        }
        ("GET", "/api/symbol_editor/names") => {
            let v = symbol_editor_names_json(dir).unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
            respond(stream, "200 OK", "application/json", v.to_string().as_bytes())
        }
        ("GET", "/api/symbol/export") => {
            let lib_id = query_value(target, "lib_id");
            match symbol_kicad_sym(dir, &lib_id) {
                Ok(text) => respond(stream, "200 OK", "text/plain; charset=utf-8", text.as_bytes()),
                Err(e) => respond(stream, "404 Not Found", "text/plain", board::reasons(&e).as_bytes()),
            }
        }
        // The library editors' read-only lookups of ANY symbol / footprint (project entry or resolved), for Duplicate, Save Copy As, Copy.
        ("GET", "/api/library/symbol") => respond(stream, "200 OK", "application/json", crate::library_api::symbol(dir, &query_value(target, "lib_id")).to_string().as_bytes()),
        ("GET", "/api/library/footprint") => respond(stream, "200 OK", "application/json", crate::library_api::footprint(dir, &query_value(target, "name")).to_string().as_bytes()),
        // The installed KiCad libraries (155 footprint, 223 symbol libraries) for the library trees: names only, one library at a time, cached
        // (`crate::library_index`) -- the libraries are too big to send or parse whole.
        ("GET", "/api/library/index" | "/api/library/items" | "/api/library/all") => {
            let reply = match crate::library_index::Kind::parse(&query_value(target, "kind")) {
                None => json!({ "error": "kind is footprint or symbol" }),
                Some(kind) => match path {
                    "/api/library/index" => crate::library_index::libraries(kind),
                    "/api/library/items" => crate::library_index::items(kind, &query_value(target, "lib")),
                    _ => crate::library_index::all(kind),
                },
            };
            respond(stream, "200 OK", "application/json", reply.to_string().as_bytes())
        }
        // The Symbol and Footprint Choosers (`crate::library_search`): one library's items with their descriptions, the search over every library's
        // names, descriptions and keywords (best first, capped), and the drawing of the one item selected. The project's own symbols come apart.
        ("GET", "/api/library/entries" | "/api/library/search" | "/api/library/details") => {
            let reply = match crate::library_index::Kind::parse(&query_value(target, "kind")) {
                None => json!({ "error": "kind is footprint or symbol" }),
                Some(kind) => {
                    let filter = crate::library_search::Filter::parse(&query_value(target, "pins"), &query_value(target, "fp_filters"), &query_value(target, "power"));
                    match path {
                        "/api/library/entries" => crate::library_search::entries(kind, &query_value(target, "lib"), &filter),
                        "/api/library/search" => {
                            let limit = query_value(target, "limit").parse::<usize>().ok().filter(|&n| n > 0).map_or(crate::library_search::SEARCH_LIMIT, |n| n.min(2000));
                            crate::library_search::search(kind, &query_value(target, "q"), &filter, limit)
                        }
                        _ => crate::library_search::details(kind, &query_value(target, "id")),
                    }
                }
            };
            respond(stream, "200 OK", "application/json", reply.to_string().as_bytes())
        }
        ("GET", "/api/library/project") => respond(stream, "200 OK", "application/json", crate::library_api::project_symbols(dir).to_string().as_bytes()),
        // Import / Paste in the two library editors: the read-only half (`crate::library_api`); the store is a `put_library_*` verb.
        ("POST", "/api/symbol_library/parse") => respond(stream, "200 OK", "application/json", crate::library_api::parse_symbols(&body).to_string().as_bytes()),
        ("POST", "/api/footprint/parse") => respond(stream, "200 OK", "application/json", crate::library_api::parse_footprint(&body).to_string().as_bytes()),
        ("GET", "/api/symbol_library/export") => match symbol_library_kicad_sym(dir) {
            Ok(text) => respond(stream, "200 OK", "text/plain; charset=utf-8", text.as_bytes()),
            Err(e) => respond(stream, "404 Not Found", "text/plain", board::reasons(&e).as_bytes()),
        },
        ("GET", "/api/fill") => {
            let v = fill_json(dir, query_value(target, "polys") == "1").unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
            respond(stream, "200 OK", "application/json", v.to_string().as_bytes())
        }
        // PCB Copy: the selection as KiCad's clipboard text (a read; the browser puts it on the system clipboard). Paste is `Cmd::PasteClipboard`.
        ("POST", "/api/clipboard/copy") => respond(stream, "200 OK", "application/json", crate::clipboard_api::copy(dir, &body).to_string().as_bytes()),
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
        ("GET", "/api/view") => respond(stream, "200 OK", "application/json", crate::view_api::get(dir).to_string().as_bytes()),
        ("POST", "/api/view") => respond(stream, "200 OK", "application/json", crate::view_api::post(dir, &String::from_utf8_lossy(&body)).to_string().as_bytes()),
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
        // Interactive router (gap #7): one session lives in `route_session`
        // across this whole sequence of calls -- see `route_api`'s doc
        // comment. Preview state never touches `design.json`; only
        // `finish` does, through the same `board::step` seam as any other
        // edit.
        ("POST", "/api/route/start") => respond(stream, "200 OK", "application/json", route_api::start(dir, route_session, &body).to_string().as_bytes()),
        ("POST", "/api/route/move") => respond(stream, "200 OK", "application/json", route_api::mv(route_session, &body).to_string().as_bytes()),
        ("POST", "/api/route/fix") => respond(stream, "200 OK", "application/json", route_api::fix(route_session, &body).to_string().as_bytes()),
        ("POST", "/api/route/undo_segment") => respond(stream, "200 OK", "application/json", route_api::undo_segment(route_session).to_string().as_bytes()),
        ("POST", "/api/route/via") => respond(stream, "200 OK", "application/json", route_api::via(route_session, &body).to_string().as_bytes()),
        ("POST", "/api/route/finish") => respond(stream, "200 OK", "application/json", route_api::finish(dir, route_session, &body).to_string().as_bytes()),
        ("POST", "/api/route/cancel") => respond(stream, "200 OK", "application/json", route_api::cancel(route_session).to_string().as_bytes()),
        ("POST", "/api/route/mode") => respond(stream, "200 OK", "application/json", route_api::set_mode(route_session, &body).to_string().as_bytes()),
        ("POST", "/api/route/settings") => respond(stream, "200 OK", "application/json", route_api::set_settings(route_session, &body).to_string().as_bytes()),
        // D (stage 5): drag an existing track segment/corner or via,
        // keeping its connections -- shares `route_session` with the
        // route endpoints above (see route_api::drag_start's doc comment).
        ("POST", "/api/route/drag_start") => respond(stream, "200 OK", "application/json", route_api::drag_start(dir, route_session, &body).to_string().as_bytes()),
        ("POST", "/api/route/drag_move") => respond(stream, "200 OK", "application/json", route_api::drag_move(route_session, &body).to_string().as_bytes()),
        ("POST", "/api/route/drag_finish") => respond(stream, "200 OK", "application/json", route_api::drag_finish(dir, route_session, &body).to_string().as_bytes()),
        // 6 (stage 6): route a differential pair -- shares `route_session`
        // with the route/drag endpoints above (see route_api's own "diff
        // pairs" section doc comment); `/api/route/cancel` above already
        // ends a dp session too.
        ("POST", "/api/route/dp_start") => respond(stream, "200 OK", "application/json", route_api::dp_start(dir, route_session, &body).to_string().as_bytes()),
        ("POST", "/api/route/dp_dims") => respond(stream, "200 OK", "application/json", route_api::dp_dims(route_session, &body).to_string().as_bytes()),
        ("POST", "/api/route/dp_move") => respond(stream, "200 OK", "application/json", route_api::dp_move(route_session, &body).to_string().as_bytes()),
        ("POST", "/api/route/dp_fix") => respond(stream, "200 OK", "application/json", route_api::dp_fix(route_session, &body).to_string().as_bytes()),
        ("POST", "/api/route/dp_undo_segment") => respond(stream, "200 OK", "application/json", route_api::dp_undo_segment(route_session).to_string().as_bytes()),
        ("POST", "/api/route/dp_finish") => respond(stream, "200 OK", "application/json", route_api::dp_finish(dir, route_session, &body).to_string().as_bytes()),
        // 7 (gap #7 task item 4): length tuning. Stateless -- no session
        // cell, every call re-reads design.json fresh (see tune_api's own
        // doc comment on why this one differs from route/drag/dp above).
        ("POST", "/api/tune_length/preview") => respond(stream, "200 OK", "application/json", tune_api::preview(dir, &body).to_string().as_bytes()),
        ("POST", "/api/tune_length/apply") => respond(stream, "200 OK", "application/json", tune_api::apply(dir, &body).to_string().as_bytes()),
        ("POST", "/api/cleanup_tracks/preview") => respond(stream, "200 OK", "application/json", cleanup_api::preview(dir, &body).to_string().as_bytes()),
        ("POST", "/api/convert/polys") => respond(stream, "200 OK", "application/json", convert_api::polys(dir, &body).to_string().as_bytes()),
        ("POST", "/api/cleanup_tracks/apply") => respond(stream, "200 OK", "application/json", cleanup_api::apply(dir, &body).to_string().as_bytes()),
        // "Board Statistics..." (task item 8): read-only, same stateless
        // no-Cmd shape as fab_api::bom below (nothing to undo -- it never
        // touches design.json). kicad-cli's, so off the loop like DRC.
        ("POST", "/api/board_stats") => offload(stream, lane, dir, job, path, &body, crate::kicad_engine::board_stats),
        // Board Setup > Custom Rules, "Check syntax": kicad-cli loads the text on a probe board (it has no checker of its own, and
        // drops a rules file it cannot parse without saying so); kicad-cli's, so off the loop like DRC.
        ("POST", "/api/check_rules") => offload(stream, lane, dir, job, path, &body, crate::kicad_engine::check_rules),
        // Fabrication and schematic outputs (Plot / Generate Drill Files /
        // Footprint Position Files / Plot Schematic / Export Netlist): each
        // dialog's JSON becomes kicad-cli arguments (`crate::fab_api`,
        // `crate::sch_output_api`) and runs through `crate::kicad_engine`,
        // into `<dir>/export/kicad/`. `/api/fab/kicad` is the same engine
        // with raw `pcb export` arguments. Every one runs off the loop
        // (`offload`), like DRC.
        ("POST", "/api/fab/kicad") => offload(stream, lane, dir, job, path, &body, fab_api::kicad),
        ("POST", "/api/fab/gerbers") => offload(stream, lane, dir, job, path, &body, fab_api::gerbers),
        ("POST", "/api/fab/drill") => offload(stream, lane, dir, job, path, &body, fab_api::drill),
        ("POST", "/api/fab/pos") => offload(stream, lane, dir, job, path, &body, fab_api::pos),
        ("POST", "/api/fab/bom") => offload(stream, lane, dir, job, path, &body, |dir, _| fab_api::bom(dir)),
        ("POST", "/api/sch/plot") => offload(stream, lane, dir, job, path, &body, sch_output_api::plot),
        ("POST", "/api/sch/netlist") => offload(stream, lane, dir, job, path, &body, sch_output_api::netlist),
        // The schematic control actions' outputs (`crate::sch_export_api`): the Fields Table's BOM file, a legacy BOM generator's output and one symbol's SVG.
        // kicad-cli's, so off the loop like the plots above.
        ("POST", "/api/sch/bom") => offload(stream, lane, dir, job, path, &body, sch_export_api::bom),
        ("POST", "/api/sch/bom_legacy") => offload(stream, lane, dir, job, path, &body, sch_export_api::bom_legacy),
        ("POST", "/api/sym/svg") => offload(stream, lane, dir, job, path, &body, sch_export_api::symbol_svg),
        // The generator scripts KiCad ships, for the legacy BOM dialog's list: a folder read, no process.
        ("GET", "/api/sch/bom_plugins") => respond(stream, "200 OK", "application/json", bom_plugins::listing().to_string().as_bytes()),
        // The other Export / Fabrication Outputs dialogs kicad-cli has a command for (STEP and the 3D formats, VRML,
        // GenCAD, IPC-D-356, IPC-2581, ODB++, the board's BOM): `crate::board_output_api`, off the loop like the rest.
        ("POST", "/api/fab/3d") => offload(stream, lane, dir, job, path, &body, crate::board_output_api::three_d),
        ("POST", "/api/fab/vrml") => offload(stream, lane, dir, job, path, &body, crate::board_output_api::vrml),
        ("POST", "/api/fab/gencad") => offload(stream, lane, dir, job, path, &body, crate::board_output_api::gencad),
        ("POST", "/api/fab/ipcd356") => offload(stream, lane, dir, job, path, &body, |dir, _| crate::board_output_api::ipcd356(dir)),
        ("POST", "/api/fab/ipc2581") => offload(stream, lane, dir, job, path, &body, crate::board_output_api::ipc2581),
        ("POST", "/api/fab/odb") => offload(stream, lane, dir, job, path, &body, crate::board_output_api::odb),
        ("POST", "/api/fab/pcb_bom") => offload(stream, lane, dir, job, path, &body, |dir, _| crate::board_output_api::pcb_bom(dir)),
        // Board control that is not a kicad-cli run (`crate::board_control_api`): cheap, answered on the loop.
        ("POST", "/api/repair_board") => respond(stream, "200 OK", "application/json", crate::board_control_api::repair_board(dir).to_string().as_bytes()),
        ("GET", "/api/footprint_associations") => respond(stream, "200 OK", "application/json", crate::board_control_api::footprint_associations(dir, &query_value(target, "ref")).to_string().as_bytes()),
        ("POST", "/api/fab/cmp") => respond(stream, "200 OK", "application/json", crate::board_control_api::export_cmp(dir).to_string().as_bytes()),
        ("GET", p) if ui_root.is_some() && !p.starts_with("/api/") => serve_file(stream, ui_root.unwrap(), p.trim_start_matches('/')),
        _ => respond(stream, "404 Not Found", "text/plain", b"not found"),
    }
}

/// `POST /api/undo`/`/api/redo`'s own
/// `{"domain": "pcb" | "schematic" | "footprint_editor"}` body -- which
/// tab asked, so `board::undo`/`redo` can scope to it (GAPS.md #15: Ctrl+Z
/// on the Schematic tab must never revert the PCB tab's last edit, or vice
/// versa; GAPS.md #8 gives the Footprint Editor tab the same treatment).
/// Absent, unparseable, or a genuinely empty body (the CLI never sends
/// one; nothing else hits this route) falls back to `None` -- the
/// original, unscoped "most recent edit of any kind" behavior -- rather
/// than refusing the request.
fn request_domain(body: &[u8]) -> Option<eda_ops::Domain> {
    let req: Value = serde_json::from_slice(body).ok()?;
    match req.get("domain").and_then(Value::as_str)? {
        "schematic" => Some(eda_ops::Domain::Schematic),
        "footprint_editor" => Some(eda_ops::Domain::FootprintEditor),
        "symbol_editor" => Some(eda_ops::Domain::SymbolEditor),
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
    // An outside edit of design.json (an agent writing the file directly)
    // becomes its own `file` history step before the page reloads.
    board::sync_external_edits(dir);
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
        let resolved = model.footprint_of(part);
        let size = resolved.as_ref().map(|f| f.courtyard_half()).map(|(w, h)| [w * 2, h * 2]);
        let mut p = json!({
            "ref": part.reference,
            "value": part.value,
            "package": part.package,
            "mpn": part.mpn,
            "block": block_of.get(&part.reference),
            "placed": fp.is_some(),
            "size": size,
            // GAPS.md #8: the name `ConstraintModel::footprint_of` actually
            // resolved this part's pads from (`Part::footprint` first, then
            // `package` -- same precedence, same key) -- not just `package`,
            // which can disagree with it (an explicit `footprint: "Lib:Name"`
            // naming a real library footprint while `package` is still a
            // generic bare name like "0603"). `Ctrl+E` ("Edit Footprint")
            // opens *this* name in the Footprint Editor, so it is editing
            // the actual definition the board uses, not a same-shaped but
            // different builtin.
            "footprint": part.footprint.clone().or_else(|| part.package.clone()),
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
            // The part's body (the box of its footprint's `F.Fab` graphics, board space) for the 3D view's fallback boxes -- the courtyard is the body plus its
            // clearance and the pads' reach. Absent when no `F.Fab` is known for the footprint: the view then sizes the box from the courtyard.
            p["body"] = json!(crate::body_api::placed_body(&design, part, fp).map(|c| [c.0, c.1, c.2, c.3]));
            // The part's 3D models with the placement its footprint gives each, and whether KiCad counts it a through-hole, SMD or virtual model: what the 3D
            // view loads (`GET /api/3dmodel`) and places. Absent when the footprint names no model: the view then draws the body box.
            if let Some(models) = resolved.as_ref().and_then(|f| crate::model3d_api::part_json(&model, part, f)) {
                p["models"] = models["models"].clone();
                p["kind3d"] = models["kind3d"].clone();
            }
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
                // The arc's mid point when this track is a KiCad arc (`Track::arc`), so an edit that re-sends the track keeps it an arc.
                "arc_mid": t.arc().map(|(_, m, _)| [m.x, m.y]),
            })).collect::<Vec<_>>(),
            "vias": r.vias.iter().map(|v| json!({
                "id": v.id, "net": v.net, "x": v.at.x, "y": v.at.y, "d": v.diameter,
                "drill": v.drill, "from": v.from_layer, "to": v.to_layer,
            })).collect::<Vec<_>>(),
            "zones": r.zones.iter().map(|z| json!({
                "id": z.id, "net": z.net, "layer": z.layer,
                "outline": z.outline.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(),
                // Full `ZONE_SETTINGS` (crates/model/src/ir.rs's `Zone`),
                // for ZoneDialog's edit mode (dialog_copper_zones.cpp) --
                // was net/layer/outline only. Enums (`pad_connection`/
                // `island_removal_mode`/`fill_mode`) serialize through
                // their own `Serialize` impl, same PascalCase variant
                // names `eda_ops::Cmd::EditZone` deserializes back.
                "clearance": z.clearance, "min_thickness": z.min_thickness,
                "thermal_gap": z.thermal_gap, "thermal_spoke_width": z.thermal_spoke_width,
                "pad_connection": z.pad_connection, "priority": z.priority,
                "island_removal_mode": z.island_removal_mode, "min_island_area": z.min_island_area,
                "fill_mode": z.fill_mode, "hatch_thickness": z.hatch_thickness, "hatch_gap": z.hatch_gap,
                "hatch_orientation_mdeg": z.hatch_orientation_mdeg, "hatch_smoothing_level": z.hatch_smoothing_level,
                "hatch_smoothing_value": z.hatch_smoothing_value, "hatch_hole_min_area": z.hatch_hole_min_area,
                "hatch_border_algorithm": z.hatch_border_algorithm,
                // Rule area / keepout (task item 3) -- see `eda_model::ir::
                // Zone::is_rule_area`'s own doc.
                "is_rule_area": z.is_rule_area, "keepout_tracks": z.keepout_tracks,
                "keepout_vias": z.keepout_vias, "keepout_pads": z.keepout_pads,
                "keepout_copper_pour": z.keepout_copper_pour, "keepout_footprints": z.keepout_footprints,
                // Task item 4: true for a generated teardrop, never a
                // hand-drawn zone -- see `eda_model::ir::Zone::teardrop`.
                "teardrop": z.teardrop,
                // `ZONE::GetZoneName()`: the Properties panel's "Name" row (`Cmd::SetZoneName`).
                "name": z.name,
            })).collect::<Vec<_>>(),
            // `BOARD_DESIGN_SETTINGS::m_TrackWidthList`/`m_ViaSizeList` --
            // the Board Setup "Track Widths & Vias" panel's editable
            // preset lists (`eda_ops::Cmd::SetTrackWidthPresets`/
            // `SetViaPresets`), driving the W/Shift+W and via-size-cycle
            // hotkeys. The board's own default (board_rules.track_width/
            // via_diameter/via_drill below) is always the implicit first
            // entry, not repeated in these lists.
            "track_width_presets": r.track_width_presets,
            "via_presets": r.via_presets.iter().map(|p| json!({ "diameter": p.diameter, "drill": p.drill })).collect::<Vec<_>>(),
            // Board Setup > Teardrops (task item 4).
            "teardrop_settings": json!({
                "enabled": r.teardrop_settings.enabled,
                "target_vias": r.teardrop_settings.target_vias,
                "target_pth_pads": r.teardrop_settings.target_pth_pads,
                "target_smd_pads": r.teardrop_settings.target_smd_pads,
                "best_length_ratio": r.teardrop_settings.best_length_ratio,
                "best_width_ratio": r.teardrop_settings.best_width_ratio,
                "max_len_um": r.teardrop_settings.max_len_um,
                "max_width_um": r.teardrop_settings.max_width_um,
                "width_to_size_filter_ratio": r.teardrop_settings.width_to_size_filter_ratio,
            }),
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
            // Task item 5 -- see `eda_model::ir::Group`'s own doc.
            "groups": d.groups.iter().map(|g| json!({ "id": g.id, "name": g.name, "member_ids": g.member_ids })).collect::<Vec<_>>(),
            // Task item 7 -- see `eda_model::ir::Dimension`'s own doc.
            "dimensions": d.dimensions.iter().map(dimension_json).collect::<Vec<_>>(),
            "dimension_settings": dimension_settings_json(&d.dimension_settings),
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
            // Board Setup dialog (dialog_board_setup.cpp) material this
            // project's constraint model holds, as the board is judged by it
            // now: the intent's (or the imported project's) values with any
            // Board Setup edit laid over them (`board::load` applies
            // `design.drawings.rules`). The pages edit these through the
            // `set_*` verbs of `crates/ops/src/board_setup.rs`; the fields
            // below that Board Setup does not edit stay as the intent says.
            // See GAPS.md #3 and PARITY-pcb.md's Board Setup section.
            "net_classes": model.board.net_classes,
            "hole_to_hole_min_um": model.board.hole_to_hole_min_um,
            "hole_clearance_um": model.board.hole_clearance_um,
            "silk_clearance_um": model.board.silk_clearance_um,
            "annular_width_min_um": model.board.annular_width_min_um,
            "min_silk_text_height_um": model.board.min_silk_text_height_um,
            "min_silk_text_thickness_um": model.board.min_silk_text_thickness_um,
            "refdes_font_um": model.board.refdes_font_um,
            "stackup": model.stackup,
            // Board Setup's pages (`eda_model::rules`), as the board is judged by them right now: the intent's or the
            // imported project's values with any edit laid over them (`board::load`). `overlay` names the pages an edit has
            // replaced; `severities` holds only the checks whose severity differs from KiCad's default (or is forced).
            "default_class": eda_model::rules::net_class_settings_of(&model.board).default,
            "constraints": eda_model::rules::Constraints::of(&model.board),
            "constraints_explicit": model.board.constraints_explicit,
            "mask_paste": model.board.solder_mask,
            "text_graphics": model.board.text_graphics,
            "board_thickness_um": model.board.board_thickness_um,
            "copper_layers": model.board.layers.len(),
            "layer_names": model.board.layers,
            "severities": eda_kicad::effective_rule_severities(&model),
            "custom_rules_text": model.board.custom_rules_text,
            "overlay": design.drawings.as_ref().and_then(|d| d.rules.as_ref()).map(|r| {
                [("net_classes", r.net_classes.is_some()), ("constraints", r.constraints.is_some()), ("mask_paste", r.mask_paste.is_some()), ("text_graphics", r.text_graphics.is_some()), ("stackup", r.stackup.is_some()), ("severities", r.severities.is_some()), ("custom_rules", r.custom_rules.is_some())]
                    .iter()
                    .filter(|(_, set)| *set)
                    .map(|(name, _)| *name)
                    .collect::<Vec<_>>()
            }).unwrap_or_default(),
        },
        "routing": routing,
        "drawings": drawings,
        // `BOARD_ITEM::IsLocked()` for every kind at once (a part ref or a
        // track/via/zone/shape/text id) -- see `DrawingsSection::locked_ids`.
        "locked": design.drawings.as_ref().map(|d| d.locked_ids.clone()).unwrap_or_default(),
        // The drill/place file origin (`BOARD_DESIGN_SETTINGS::GetAuxOrigin`), `[x, y]` um, or null at (0, 0).
        "aux_origin": design.drawings.as_ref().and_then(|d| d.aux_origin).map(|p| json!([p.x, p.y])),
        // The point the editing grid is anchored at (`BOARD_DESIGN_SETTINGS::GetGridOrigin`), `[x, y]` um, or null at (0, 0).
        "grid_origin": design.drawings.as_ref().and_then(|d| d.grid_origin).map(|p| json!([p.x, p.y])),
        // The board's paper and title block (Page Settings); null is A4 landscape / an empty title block.
        "page": design.drawings.as_ref().and_then(|d| d.page.as_ref()).map(crate::page_json::page_json),
        "title_block": design.drawings.as_ref().and_then(|d| d.title_block.as_ref()).map(crate::page_json::title_block_json),
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
        // `c1`/`c2` are the curve's own control points (KiCad's `(pts start c1 c2 end)` order); the flattened
        // polyline the canvas actually draws is computed client-side with the same `BEZIER_POLY` port.
        Shape::Bezier { id, layer, stroke_width, filled, start, c1, c2, end } => {
            json!({ "id": id, "kind": "bezier", "layer": layer, "stroke_width": stroke_width, "filled": filled, "start": pt(*start), "c1": pt(*c1), "c2": pt(*c2), "end": pt(*end) })
        }
    }
}

/// Task item 7. `kind`/`units`/`units_format`/`text_position`/
/// `arrow_direction` as lowercase strings, matching every other enum this
/// API already exposes (`justify` above). `height`/`horizontal`/
/// `leader_length` are present only for the kind that actually has them
/// (`null` otherwise) -- `eda_model::ir::DimensionKind`'s own fields.
/// `lines`/`text_at`/`computed_text_angle`/`measured_value_um`/`text` are
/// the one and only place this geometry is computed
/// (`eda_connectivity::dimension::compute_dimension_geometry`) -- a
/// renderer never re-derives it.
fn dimension_json(d: &eda_model::ir::Dimension) -> Value {
    use eda_model::ir::{ArrowDirection, DimensionKind, DimensionTextPosition, DimensionUnits, DimensionUnitsFormat};

    let (kind, height, horizontal, leader_length) = match d.kind {
        DimensionKind::Aligned { height } => ("aligned", Some(height), None, None),
        DimensionKind::Orthogonal { height, horizontal } => ("orthogonal", Some(height), Some(horizontal), None),
        DimensionKind::Radial { leader_length } => ("radial", None, None, Some(leader_length)),
        DimensionKind::Leader => ("leader", None, None, None),
        DimensionKind::Center => ("center", None, None, None),
    };
    let units = match d.units {
        DimensionUnits::Mm => "mm",
        DimensionUnits::Mil => "mil",
        DimensionUnits::Inch => "inch",
        DimensionUnits::Automatic => "automatic",
    };
    let units_format = match d.units_format {
        DimensionUnitsFormat::NoSuffix => "no_suffix",
        DimensionUnitsFormat::BareSuffix => "bare_suffix",
        DimensionUnitsFormat::ParenSuffix => "paren_suffix",
    };
    let text_position = match d.text_position {
        DimensionTextPosition::Outside => "outside",
        DimensionTextPosition::Inline => "inline",
    };
    let arrow_direction = match d.arrow_direction {
        ArrowDirection::Inward => "inward",
        ArrowDirection::Outward => "outward",
    };

    let geom = eda_connectivity::dimension::compute_dimension_geometry(d);

    json!({
        "id": d.id,
        "layer": d.layer,
        "kind": kind,
        "height": height,
        "horizontal": horizontal,
        "leader_length": leader_length,
        "start": [d.start.x, d.start.y],
        "end": [d.end.x, d.end.y],
        "prefix": d.prefix,
        "suffix": d.suffix,
        "override_text": d.override_text,
        "units": units,
        "units_format": units_format,
        "precision": d.precision,
        "suppress_trailing_zeros": d.suppress_trailing_zeros,
        "text_position": text_position,
        "keep_text_aligned": d.keep_text_aligned,
        "text_angle": d.text_angle,
        "text_size_um": d.text_size_um,
        // `EDA_TEXT::GetTextThickness()` of the label; null = 15 % of the size (the Properties panel's "Thickness").
        "text_thickness_um": d.text_thickness_um,
        "stroke_width": d.stroke_width,
        "arrow_length": d.arrow_length,
        "extension_offset": d.extension_offset,
        "extension_height": d.extension_height,
        "arrow_direction": arrow_direction,
        "lines": geom.lines.iter().map(|(a, b)| json!([[a.x, a.y], [b.x, b.y]])).collect::<Vec<_>>(),
        "text_at": [geom.text_at.x, geom.text_at.y],
        "computed_text_angle": geom.text_angle,
        "measured_value_um": geom.measured_value_um,
        "text": geom.text,
    })
}

fn dimension_settings_json(s: &eda_model::ir::DimensionSettings) -> Value {
    use eda_model::ir::{DimensionTextPosition, DimensionUnits, DimensionUnitsFormat};

    let units = match s.units {
        DimensionUnits::Mm => "mm",
        DimensionUnits::Mil => "mil",
        DimensionUnits::Inch => "inch",
        DimensionUnits::Automatic => "automatic",
    };
    let units_format = match s.units_format {
        DimensionUnitsFormat::NoSuffix => "no_suffix",
        DimensionUnitsFormat::BareSuffix => "bare_suffix",
        DimensionUnitsFormat::ParenSuffix => "paren_suffix",
    };
    let text_position = match s.text_position {
        DimensionTextPosition::Outside => "outside",
        DimensionTextPosition::Inline => "inline",
    };

    json!({
        "units": units,
        "units_format": units_format,
        "precision": s.precision,
        "suppress_trailing_zeros": s.suppress_trailing_zeros,
        "text_position": text_position,
        "keep_text_aligned": s.keep_text_aligned,
        "text_size_um": s.text_size_um,
        "stroke_width": s.stroke_width,
        "arrow_length": s.arrow_length,
        "extension_offset": s.extension_offset,
        "extension_height": s.extension_height,
    })
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

/// Walks `sheet_path` (`"<id>/<id>/..."`, root when empty) down from
/// `design`'s own root sheet through `design.sheet_contents` (GAPS.md #6) --
/// a sheet-instance id not found at some level, or naming a file this
/// design never descended into, falls back to whatever level was last
/// successfully resolved (the Hierarchy panel's own stale-path safety net:
/// a board reloaded after an edit that removed a sheet should not just
/// error the whole schematic view out). Returns the resolved section
/// alongside the `(id, name)` breadcrumb actually reached, which may be
/// shorter than the requested path when it had to fall back.
pub(crate) fn resolve_sheet(design: &eda_model::ir::Design, sheet_path: &str) -> (eda_model::ir::SchematicSection, Vec<(String, String)>) {
    let mut current = design.schematic.clone().unwrap_or(eda_model::ir::SchematicSection {
        power_symbols: vec![],
        no_connects: vec![], bus_entries: vec![],
        erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), field_layout: Default::default(),
        imported_from_kicad: false,
        title_block: None,
        sheets: vec![],
        instance_overrides: vec![], junctions: vec![], lines: vec![], extras: Default::default(),
        symbols: Vec::new(),
        wires: Vec::new(),
        labels: Vec::new(),
        texts: Vec::new(),
    });
    let mut breadcrumb = Vec::new();
    for id in sheet_path.split('/').filter(|s| !s.is_empty()) {
        let Some(sheet) = current.sheets.iter().find(|s| s.id == id) else { break };
        let Some(child) = design.sheet_contents.as_ref().and_then(|screens| screens.get(&sheet.file)) else { break };
        breadcrumb.push((sheet.id.clone(), sheet.name.clone()));
        current = child.clone();
    }
    (current, breadcrumb)
}

/// The same schematic `schematic_svg` draws, but as structured JSON for
/// the browser UI's own KiCad-style renderer instead of one baked image:
/// each symbol instance plus its part's pins (a `SymbolInstance` alone
/// says nothing about what it draws), every wire, and every net label.
/// `sheet_path`: see `resolve_sheet`'s own doc -- empty/root for every
/// design before hierarchy support existed, same response shape as always
/// for that case (plus the two new, always-present `sheets`/`sheet_path`
/// fields, empty arrays for a plain single-sheet design). No second cache
/// next to `schematic_svg`'s -- these boards are small, so deriving again
/// on a cache miss costs nothing worth guarding.
fn schematic_json(dir: &Path, sheet_path: &str) -> Result<Value, Vec<CheckResult>> {
    let (_, mut design, model) = board::load(dir)?;
    // A board with no stored schematic shows the one derived from its intent: module sheets, whole, so the sheet path resolves.
    if design.schematic.is_none() {
        let derived = board::derived_schematic(&design, &model)?;
        design.schematic = derived.schematic;
        design.sheet_contents = derived.sheet_contents;
    }
    Ok(schematic_json_of(&design, &model, sheet_path))
}

/// [`schematic_json`] of a design that is already loaded (the schematic clipboard asks it of a design a paste has just been tried on).
pub(crate) fn schematic_json_of(design: &eda_model::ir::Design, model: &eda_model::ConstraintModel, sheet_path: &str) -> Value {
    let (sch, breadcrumb) = resolve_sheet(design, sheet_path);
    let symbols: Vec<Value> = sch
        .symbols
        .iter()
        .map(|s| {
            let part = model.part(&s.id);
            // Multi-unit: this instance's own pin list is only the pins
            // that belong to its unit (or a `unit == 0` common-to-every-
            // unit pin) -- a sibling instance of the same reference, placed
            // for a different unit, carries the rest. Falls open (every
            // part.pins entry) when no real library symbol resolves one of
            // them by number, same "can't tell, so don't filter" fallback
            // `eda_engine::geometry`'s own unit-aware functions use.
            let resolved = part.and_then(|p| model.real_symbol_of(&s.lib_id, p));
            let pins: Vec<Value> = part
                .map(|p| {
                    p.pins
                        .iter()
                        .filter(|pin| resolved.as_ref().and_then(|r| r.pin_by_number(&pin.number)).is_none_or(|rp| rp.unit == 0 || rp.unit == s.unit))
                        .map(|pin| json!({ "number": pin.number, "name": pin.name, "kind": pin.kind }))
                        .collect()
                })
                .unwrap_or_default();
            // Where each field is drawn on the sheet (`eda_engine::fields`): the placements the section keeps, else Autoplace Fields'. Not for a
            // schematic read from a KiCad file, whose symbols are placed by their own origin rather than the engine's box corner.
            let fields: Vec<Value> = match (part, sch.imported_from_kicad) {
                (Some(p), false) => {
                    let mut placed = s.clone();
                    if placed.lib_id.is_empty() {
                        placed.lib_id = format!("eda:{}", s.id);
                    }
                    let geom = eda_engine::symgeom::SymbolGeom::of(&placed, p, resolved.as_ref());
                    eda_engine::fields::symbol_fields(&sch, &placed, Some(p), resolved.as_ref(), &geom).iter().map(page_field_json).collect()
                }
                _ => Vec::new(),
            };
            json!({
                "id": s.id,
                "fields": fields,
                "at": [s.at.x, s.at.y],
                // Millideg -> plain degrees, same convention `state()` uses for a PCB part's `rot`.
                "rot": s.rot as f64 / 1000.0,
                // `mirror`: the two-axis form the frontend's own renderer
                // (schematic/transform.ts, ported from `sch_symbol.cpp::
                // SetOrientation` before this field existed on this side)
                // has always expected -- `mirrored`/`mirror_y` are never
                // both true at once (`mirror_symbol`/`mirror_symbol_
                // vertical` each clear the other), so this is a clean
                // three-way choice, not a lossy collapse. `mirrored` is
                // also still sent, for api/client.ts's own legacy-shim
                // fallback (`sym.mirror ?? (legacy.mirrored ? "y" : null)`) --
                // redundant once this field exists, but harmless to leave
                // both until that shim is itself cleaned up.
                "mirror": if s.mirrored { Some("y") } else if s.mirror_y { Some("x") } else { None },
                "mirrored": s.mirrored,
                // KiCad library id this instance draws from ("Device:R", or
                // the synthetic "eda:<id>" for a part with no resolved real
                // symbol) -- key into "lib_symbols" below for its graphics.
                "lib_id": s.lib_id,
                // Which unit (1-based) of a multi-unit symbol this placed
                // instance is -- `libSymbol.ts`'s own `resolveLibSymbol`
                // already filters `lib_symbols[lib_id]`'s graphics/pins by
                // this field (`visibleFor`), so sending it is what actually
                // turns that pre-existing renderer plumbing on.
                "unit": s.unit,
                "value": if s.value.is_empty() { part.and_then(|p| p.value.clone()) } else { Some(s.value.clone()) },
                "footprint": if s.footprint.is_empty() { None } else { Some(s.footprint.clone()) },
                "datasheet": if s.datasheet.is_empty() { None } else { Some(s.datasheet.clone()) },
                "mpn": part.and_then(|p| p.mpn.clone()),
                "package": part.and_then(|p| p.package.clone()),
                "pins": pins,
                // Do not Populate / Exclude from BOM / Board / Simulation (`setDNP` ...): the flags the painter marks and the attribute actions toggle.
                "dnp": s.dnp,
                "exclude_from_bom": s.exclude_from_bom,
                "exclude_from_board": s.exclude_from_board,
                "exclude_from_sim": s.exclude_from_sim,
            })
        })
        .collect();
    let wires: Vec<Value> = sch
        .wires
        .iter()
        .map(|w| {
            let mut v = json!({ "id": w.id, "net": w.net, "pins": w.pins, "pts": w.pts.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(), "bus": w.bus });
            // The stroke Wire/Bus Properties set (width, style, colour), when one was.
            if let Some(stroke) = sch.extras.strokes.get(&w.id) {
                v["stroke"] = json!(stroke);
            }
            // A bus's member nets (`BUS_UNFOLD_MENU` lists them): the vector/group/alias name expansion (`eda_kicad::expand_bus_members`).
            if w.bus {
                v["members"] = json!(eda_kicad::expand_bus_members(&w.net, &design.bus_aliases).unwrap_or_default());
            }
            v
        })
        .collect();
    // Explicit junctions (`J`) and graphic lines on the notes layer (`I`) -- see `eda_model::ir::Junction`/`SchLine`.
    let junctions: Vec<Value> = sch.junctions.iter().map(|j| json!({ "id": j.id, "at": [j.at.x, j.at.y], "look": sch.extras.junction_looks.get(&j.id) })).collect();
    let lines: Vec<Value> = sch.lines.iter().map(|l| json!({ "id": l.id, "pts": l.pts.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(), "width_um": l.width_um, "stroke": sch.extras.strokes.get(&l.id) })).collect();
    // Drawn shapes, text boxes, rule areas and directive labels (`SchGraphic`, serialized as stored) and the ids of locked items.
    let graphics: Value = serde_json::to_value(&sch.extras.graphics).unwrap_or(Value::Null);
    let locked: Vec<&String> = sch.extras.locked.iter().collect();
    // GAPS.md #20: bus entries, for the bus/entry tool and for drawing the
    // diagonal stub on canvas.
    let bus_entries: Vec<Value> = sch.bus_entries.iter().map(|be| json!({ "id": be.id, "at": [be.at.x, be.at.y], "size": [be.size.x, be.size.y], "stroke": sch.extras.strokes.get(&be.id) })).collect();
    let labels: Vec<Value> = sch
        .labels
        .iter()
        .map(|l| {
            let (scope, shape) = match &l.kind {
                eda_model::ir::LabelKind::Local => ("local", None),
                eda_model::ir::LabelKind::Global { shape } => ("global", Some(*shape)),
                eda_model::ir::LabelKind::Hierarchical { shape } => ("hierarchical", Some(*shape)),
            };
            json!({ "id": l.id, "net": l.net, "at": [l.at.x, l.at.y], "scope": scope, "shape": shape.map(label_shape_str), "spin": sch.extras.label_spins.get(&l.id) })
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
        .map(|p| json!({ "id": p.id, "lib_id": p.lib_id, "at": [p.at.x, p.at.y], "rot": p.rot as f64 / 1000.0, "net": p.net, "pin": p.pin, "fields": eda_engine::fields::power_fields(&sch, p).iter().map(page_field_json).collect::<Vec<_>>() }))
        .collect();
    let no_connects: Vec<Value> = sch.no_connects.iter().map(|nc| json!({ "id": nc.id, "at": [nc.at.x, nc.at.y], "pin": nc.pin })).collect();
    let title_block = sch.title_block.as_ref().map(crate::page_json::title_block_json);

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
            // A sheet this project drew places its symbols by the corner of their box: draw the symbol the writer bakes (its origin
            // at that corner, every pin where a wire ends) instead of the library's own, so symbol and wires meet.
            let instance = sch.symbols.iter().find(|s| (if s.lib_id.is_empty() { format!("eda:{}", s.id) } else { s.lib_id.clone() }) == *lib_id);
            if let (false, Some(s)) = (sch.imported_from_kicad, instance) {
                if let Some(part) = model.part(&s.id) {
                    let resolved = model.real_symbol_of(&s.lib_id, part);
                    return (lib_id.clone(), lib_symbol_json(&eda_engine::placed::corner_symbol(lib_id, part, resolved.as_ref(), s.unit)));
                }
            }
            let resolved = if eda_model::is_synthetic_lib_id(lib_id) { None } else { model.symbol_of(lib_id) };
            let value = resolved.unwrap_or_else(|| synthesize_generic_symbol(lib_id, model));
            (lib_id.clone(), lib_symbol_json(&value))
        })
        .collect::<serde_json::Map<_, _>>()
        .into();

    // GAPS.md #6: the currently-displayed sheet's own child placements
    // (for the Hierarchy panel's tree and for drawing a child sheet as a
    // labeled box on the canvas) and the breadcrumb that reached here (for
    // "Leave Sheet"/an up-the-tree display) -- both empty for the
    // overwhelmingly common single-sheet design, same as `sheets`/
    // `instance_overrides` already default to empty everywhere else.
    let sheets: Vec<Value> = sch
        .sheets
        .iter()
        .map(|s| {
            json!({
                "id": s.id, "name": s.name, "file": s.file, "page": s.page,
                "fields": eda_engine::fields::sheet_fields(&sch, s).iter().map(page_field_json).collect::<Vec<_>>(),
                "at": [s.at.x, s.at.y], "size": [s.size.0, s.size.1],
                "pins": s.pins.iter().map(|p| json!({ "id": p.id, "name": p.name, "shape": label_shape_str(p.shape), "at": [p.at.x, p.at.y] })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let sheet_path: Vec<Value> = breadcrumb.iter().map(|(id, name)| json!({ "id": id, "name": name })).collect();
    // The paper this sheet is drawn on: its full Page Settings when it has them (portrait, a user size), else the name its title block carries
    // (the layout engine writes it per sheet); A4 landscape when neither.
    let page = eda_model::page::PageSettings::of_sheet(sch.extras.page.as_ref(), sch.title_block.as_ref().map(|t| t.paper.as_str()).unwrap_or(""));
    let (paper_w, paper_h) = page.size_um().unwrap_or((297_000, 210_000));
    let file = viewed_file(design, &breadcrumb);

    json!({
        "paper": { "name": page.paper, "width_um": paper_w, "height_um": paper_h },
        "file": file,
        "symbols": symbols,
        "wires": wires,
        "labels": labels,
        "texts": texts,
        "power_symbols": power_symbols,
        "no_connects": no_connects,
        "bus_entries": bus_entries,
        "junctions": junctions,
        "lines": lines,
        "graphics": graphics,
        "locked": locked,
        "title_block": title_block,
        "page": crate::page_json::page_json(&page),
        "lib_symbols": lib_symbols,
        "sheets": sheets,
        "sheet_path": sheet_path,
    })
}

/// The file of the screen the breadcrumb ends on (`""` for the root): the title block of the sheet in view names it. The sheets
/// of the whole design are `GET /api/sch/hierarchy`.
fn viewed_file(design: &eda_model::ir::Design, breadcrumb: &[(String, String)]) -> String {
    let mut here = design.schematic.as_ref();
    let mut file = String::new();
    for (id, _) in breadcrumb {
        let Some(sheet) = here.and_then(|s| s.sheets.iter().find(|s| &s.id == id)) else { break };
        file = sheet.file.clone();
        here = design.sheet_contents.as_ref().and_then(|c| c.get(&sheet.file));
    }
    file
}

/// `A`'s symbol chooser's own catalog -- GET /api/symbol_library. "The
/// libraries we already load" (the task brief's own framing, not "browse
/// every installed library"): every library name this project's intent
/// already resolved a part against, or that's already placed on the
/// sheet, scanned for its *full* contents (`eda_kicad::
/// list_symbols_in_library`) -- not just the one symbol some part
/// happened to already use -- so placing a second, different part from an
/// already-referenced library (say, a diode when only resistors are used
/// so far) is possible without it being on the sheet first. Power symbols
/// are excluded (`P` is their own tool, section 3) and so is the
/// parametric `Connector_Generic:Conn_01x<N>` family (see
/// `builtin_catalog`'s own doc) -- both documented gaps, not oversights.
///
/// `eda_model::symbol::builtin_catalog` fills in anything a real library
/// file didn't already cover for a referenced name, same "real file
/// first, builtin fallback" precedence every other symbol resolution in
/// this codebase already uses -- the common case in this project's own
/// test/example boards, which have no real KiCad install to read from at
/// all (`default_symbol_library_root` points at a path that doesn't exist
/// here), so without it the chooser would be empty on every board this
/// session could actually try it against.
fn symbol_library_json(dir: &Path) -> Result<Value, Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    let lib_root = eda_kicad::default_symbol_library_root();

    let lib_name_of = |lib_id: &str| lib_id.split_once(':').map(|(l, _)| l.to_string());
    let mut lib_names: std::collections::BTreeSet<String> = model.symbols.iter().filter_map(|s| lib_name_of(&s.lib_id)).collect();
    // every sheet's symbols: with module sheets the root holds none of its own
    for sch in design.schematic.iter().chain(design.sheet_contents.iter().flat_map(|c| c.values())) {
        lib_names.extend(sch.symbols.iter().filter_map(|s| lib_name_of(&s.lib_id)));
    }

    let mut by_lib_id: std::collections::BTreeMap<String, eda_model::LibSymbol> = std::collections::BTreeMap::new();
    for name in &lib_names {
        for sym in eda_kicad::list_symbols_in_library(&lib_root, name) {
            by_lib_id.insert(sym.lib_id.clone(), sym);
        }
    }
    for sym in eda_model::symbol::builtin_catalog() {
        by_lib_id.entry(sym.lib_id.clone()).or_insert(sym);
    }

    let entries: Vec<Value> = by_lib_id
        .values()
        .map(|s| {
            json!({
                "lib_id": s.lib_id,
                "description": s.description,
                "reference_prefix": if s.reference_prefix.is_empty() { "U" } else { s.reference_prefix.as_str() },
                // Multi-unit: how many units (`SymbolChooserDialog`'s own
                // unit picker, once a part names one) this library symbol
                // declares -- 1 for every single-unit symbol, unchanged
                // from before this field existed.
                "unit_count": s.unit_count,
            })
        })
        .collect();
    let lib_symbols: Value = by_lib_id.iter().map(|(id, s)| (id.clone(), lib_symbol_json(s))).collect::<serde_json::Map<_, _>>().into();

    Ok(json!({ "entries": entries, "lib_symbols": lib_symbols }))
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
    let reference = lib_id.strip_prefix("eda:").unwrap_or(lib_id);
    // This id already names one specific part (`"eda:<id>"` is 1:1 with a
    // `Part`, unlike a real lib_id which many instances can share) -- its
    // own reference's leading letters are the only "what kind of part is
    // this" signal available here, same heuristic `annotate()` already
    // uses to recover a prefix from an id.
    let reference_prefix: String = reference.chars().take_while(|c| c.is_alphabetic()).collect();
    let empty = || eda_model::LibSymbol {
        lib_id: lib_id.to_string(),
        graphics: vec![],
        pins: vec![],
        power: false,
        in_bom: true,
        on_board: true,
        datasheet: String::new(),
        description: String::new(),
        reference_prefix: reference_prefix.clone(),
        unit_count: 1,
        pin_names_hidden: false,
        pin_numbers_hidden: false,
        pin_name_offset_mm: eda_model::symbol::DEFAULT_PIN_NAME_OFFSET_MM,
    };
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
    eda_model::LibSymbol {
        lib_id: lib_id.to_string(),
        graphics,
        pins: lib_pins,
        power: false,
        in_bom: true,
        on_board: true,
        datasheet: String::new(),
        description: String::new(),
        reference_prefix,
        unit_count: 1,
        pin_names_hidden: false,
        pin_numbers_hidden: false,
        pin_name_offset_mm: eda_model::symbol::DEFAULT_PIN_NAME_OFFSET_MM,
    }
}

/// A field on the sheet as the painter reads it: the text, its anchor in micrometres, whether it runs vertically, how it is justified
/// against the anchor (in the text's own axes) and whether it is drawn.
fn page_field_json(f: &eda_engine::fields::PageField) -> Value {
    use eda_model::kicad_font::{HJustify, VJustify};
    json!({
        "name": f.name,
        "text": f.text,
        "at": [f.at.0.round(), f.at.1.round()],
        "vertical": f.vertical,
        "h": match f.h { HJustify::Left => "left", HJustify::Center => "center", HJustify::Right => "right" },
        "v": match f.v { VJustify::Top => "top", VJustify::Center => "center", VJustify::Bottom => "bottom" },
        "visible": f.visible,
    })
}

/// One `LibSymbol` as JSON: graphics/pins in the symbol's own local frame,
/// millimetres, +y **up** (KiCad's own library convention, not this API's
/// usual +y-down sheet millimetres) -- drawing it in sheet space needs the
/// same negate-y-then-rotate-then-mirror composition
/// `eda_kicad::lib::baked_local`'s doc comment spells out.
///
/// Each graphic and pin carries two spellings: the engine symbol's own (`stroke_mm`, `filled`, `radius_mm`, `text`) and the names the studio's painter and its
/// `LibSymbol` type read (`stroke_width`, `fill`, `radius`, `content`, `body_style`, and a pin's `hidden`). Without the second a symbol from a library was
/// drawn with no body and no pins: the painter keeps an item only when its `body_style` is 0 or the placed one's. The engine symbol has no body styles and
/// no hidden pins, so every item is shared (`body_style` 0) and none is hidden, and it keeps only whether a shape is filled, not how: a filled rectangle is
/// the body colour (what library ICs use), any other filled shape the outline colour.
fn lib_symbol_json(s: &eda_model::LibSymbol) -> Value {
    let pt = |p: eda_model::symbol::SPoint| json!([p.x, p.y]);
    let fill = |filled: bool, how: &str| if filled { how.to_string() } else { "none".to_string() };
    let graphics: Vec<Value> = s
        .graphics
        .iter()
        .map(|g| {
            use eda_model::SymbolGraphic::*;
            match g {
                Rectangle { unit, start, end, stroke_mm, filled } => json!({ "kind": "rectangle", "unit": unit, "body_style": 0, "start": pt(*start), "end": pt(*end), "stroke_mm": stroke_mm, "stroke_width": stroke_mm, "filled": filled, "fill": fill(*filled, "background") }),
                Polyline { unit, pts, stroke_mm, filled } => json!({ "kind": "polyline", "unit": unit, "body_style": 0, "pts": pts.iter().map(|p| pt(*p)).collect::<Vec<_>>(), "stroke_mm": stroke_mm, "stroke_width": stroke_mm, "filled": filled, "fill": fill(*filled, "outline") }),
                Circle { unit, center, radius_mm, stroke_mm, filled } => json!({ "kind": "circle", "unit": unit, "body_style": 0, "center": pt(*center), "radius_mm": radius_mm, "radius": radius_mm, "stroke_mm": stroke_mm, "stroke_width": stroke_mm, "filled": filled, "fill": fill(*filled, "outline") }),
                Arc { unit, start, mid, end, stroke_mm, filled } => json!({ "kind": "arc", "unit": unit, "body_style": 0, "start": pt(*start), "mid": pt(*mid), "end": pt(*end), "stroke_mm": stroke_mm, "stroke_width": stroke_mm, "filled": filled, "fill": fill(*filled, "outline") }),
                Text { unit, text, at, angle_deg, size_mm } => json!({ "kind": "text", "unit": unit, "body_style": 0, "text": text, "content": text, "at": pt(*at), "angle_deg": angle_deg, "size_mm": size_mm }),
            }
        })
        .collect();
    let pins: Vec<Value> = s
        .pins
        .iter()
        .map(|p| json!({ "number": p.number, "name": eda_model::kicad_geom::shown_name(&p.name), "electrical_type": p.electrical_type, "shape": p.shape, "at": pt(p.at), "angle_deg": p.angle_deg, "length_mm": p.length_mm, "unit": p.unit, "body_style": 0, "hidden": false }))
        .collect();
    // how the symbol's pins show their texts (`(pin_names (hide yes) (offset x))`, `(pin_numbers (hide yes))`)
    json!({ "power": s.power, "graphics": graphics, "pins": pins, "datasheet": s.datasheet, "description": s.description, "pin_names_hidden": s.pin_names_hidden, "pin_numbers_hidden": s.pin_numbers_hidden, "pin_name_offset": s.pin_name_offset_mm })
}

/// `GET /api/footprint?name=<name>` -- the Footprint Editor's own document
/// (GAPS.md #8), serialized exactly as `design.footprint_library` stores
/// it. Unlike `/api/state`'s hand-built display JSON, there is no second
/// shape to keep in sync here: the same `LibraryPad`/`Shape`/`Text` fields
/// a `Cmd::AddPad`/`AddFootprintGraphic`/`AddFootprintText` sends are what
/// comes back on the next poll. `name` must already be open (`Cmd::
/// OpenFootprintForEdit`, issued once when the tab/footprint opens,
/// through the ordinary `/api/cmd` route) -- this route never
/// materializes one on its own, matching every other GET route here being
/// a pure read of `design.json`. `name` is percent-decoded
/// (`query_value`): the client sends `encodeURIComponent( name )`, which
/// makes a library footprint's `Lib:Name` arrive as `Lib%3AName` -- read
/// raw, as this route once did, no footprint with a library nickname
/// (every installed KiCad one) could be fetched back after opening it.
fn footprint_json(dir: &Path, name: &str) -> Result<Value, Vec<CheckResult>> {
    let (_, design, _) = board::load(dir)?;
    let fp = design
        .footprint_library
        .as_ref()
        .and_then(|l| l.by_name(name))
        .ok_or_else(|| vec![CheckResult::fail("ops_unknown_footprint", name, "this footprint has not been opened in the Footprint Editor yet")])?;
    serde_json::to_value(fp).map_err(|e| vec![CheckResult::fail("board_bad_design", name, e.to_string())])
}

/// `GET /api/footprint_library` -- every footprint name available to open:
/// already-opened library entries plus the intent/real-library-resolved
/// names `ConstraintModel::footprints` carries, deduplicated and sorted.
/// The Footprint Editor's own "Open from Library" picker; see
/// `footprint_json`'s doc on why opening one is a separate `Cmd`, not a
/// side effect of listing them.
fn footprint_library_json(dir: &Path) -> Result<Value, Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    let mut names: std::collections::BTreeSet<String> = model.footprints.iter().map(|f| f.name.clone()).collect();
    // `project`: the names that are entries of the project library itself (editable in place, what the library tree's
    // Delete / Rename / Cut act on); the rest come from the intent, a loaded `.kicad_mod` or the builtin table.
    let mut project: Vec<String> = Vec::new();
    if let Some(lib) = &design.footprint_library {
        names.extend(lib.footprints.iter().map(|f| f.name.clone()));
        project = lib.footprints.iter().map(|f| f.name.clone()).collect();
        project.sort();
    }
    Ok(json!({ "names": names.into_iter().collect::<Vec<_>>(), "project": project }))
}

/// `GET /api/footprint/export?name=<name>` -- a derived, standalone
/// `.kicad_mod` for `name` (GAPS.md #8 step 6), via `eda_kicad::
/// export_kicad_mod`. `name` must already be open in `design.
/// footprint_library`, same requirement `footprint_json` documents (this
/// route exports the library's own current copy, not a fresh re-resolve
/// of the intent/builtin table). The frontend turns the returned text
/// into a browser download client-side (a `Blob` + synthetic anchor
/// click) -- this route itself only ever returns `text/plain`, no
/// `Content-Disposition`, matching every other GET route here.
fn footprint_kicad_mod(dir: &Path, name: &str) -> Result<String, Vec<CheckResult>> {
    // The project entry, else what the model resolves the name to (a read-only export never needs the footprint opened first).
    crate::library_api::footprint_kicad_mod_any(dir, name)
}

/// `GET /api/board.kicad_pcb` -- the design as a derived `.kicad_pcb` (File > Save As... on the PCB tab). `design.json` stays the
/// only master; this is the same derived file every kicad-cli run is fed, just handed to the browser to save.
fn kicad_pcb_text(dir: &Path) -> Result<String, Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    eda_kicad::export_kicad_pcb(&design, &model, &eda_kicad::ExportMeta { date: &crate::kicad_engine::chrono_like_today(), title: "board" })
}

/// `GET /api/schematic.kicad_sch` -- the schematic as derived `.kicad_sch` files (File > Save As... on the Schematic tab):
/// `{"files": [{"name", "text"}]}`, the root sheet first (named `board.kicad_sch`; the client renames it after the board) and
/// then one file per sub-sheet screen, so a hierarchical design saves whole.
fn kicad_sch_files_json(dir: &Path) -> Result<Value, Vec<CheckResult>> {
    let (meta, mut design, model) = board::load(dir)?;
    // A board with no stored schematic shows (and saves) the one derived from its intent, like every other schematic output.
    if design.schematic.is_none() {
        design = board::derived_schematic(&design, &model)?;
    }
    let project = PathBuf::from(&meta.intent).file_stem().and_then(|s| s.to_str()).unwrap_or("board").to_string();
    let files = eda_kicad::export_kicad_sch_tree(&design, &model, &eda_kicad::ExportMeta { date: &crate::kicad_engine::chrono_like_today(), title: &project }, "board.kicad_sch")?;
    Ok(json!({ "files": files.into_iter().map(|(name, text)| json!({ "name": name, "text": text })).collect::<Vec<_>>() }))
}

/// The percent-decoded value of query parameter `key` in a request target (`/api/symbol?lib_id=Device%3AR` -> `Device:R`),
/// "" when the parameter is absent.
fn query_value(target: &str, key: &str) -> String {
    let query = target.split('?').nth(1).unwrap_or("");
    let raw = query.split('&').find_map(|kv| kv.strip_prefix(key).and_then(|rest| rest.strip_prefix('='))).unwrap_or("");
    percent_decode(raw)
}

/// `%XX` escapes to their bytes (a malformed escape is kept literally); `+` is left alone -- `encodeURIComponent` never emits it.
fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%' && i + 2 < bytes.len())
            .then(|| std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()))
            .flatten();
        match escaped {
            Some(b) => {
                out.push(b);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `GET /api/symbol?lib_id=<id>` -- the Symbol Editor's own open document,
/// serialized exactly as `design.symbol_library` stores it, same
/// "no second hand-built shape" contract `footprint_json`'s own doc
/// explains (the `LibrarySymbolPin`/`LibrarySymbolGraphic` fields a
/// `Cmd::AddSymbolPin`/`AddSymbolGraphic` sends are what comes back on the
/// next poll). `lib_id` must already be open (`Cmd::OpenSymbolForEdit`).
fn symbol_edit_json(dir: &Path, lib_id: &str) -> Result<Value, Vec<CheckResult>> {
    let (_, design, _) = board::load(dir)?;
    let sym = design
        .symbol_library
        .as_ref()
        .and_then(|l| l.by_lib_id(lib_id))
        .ok_or_else(|| vec![CheckResult::fail("ops_unknown_symbol", lib_id, "this symbol has not been opened in the Symbol Editor yet")])?;
    serde_json::to_value(sym).map_err(|e| vec![CheckResult::fail("board_bad_design", lib_id, e.to_string())])
}

/// `GET /api/symbol_editor/names` -- every `lib_id` available to open: the
/// Symbol Editor's own "Open from Library" picker, mirroring
/// `footprint_library_json`'s doc. Besides already-opened entries and
/// every `lib_id` this project's intent already resolved
/// (`ConstraintModel::symbols`), the small hand-ported `builtin_catalog`
/// is always offered too (unlike footprints, which have no equivalent
/// enumeration function) -- so "New Symbol" always has at least
/// `Device:R`/`C`/`L`/`D`/`LED` to start editing from, even with no real
/// KiCad install and nothing on the sheet yet.
fn symbol_editor_names_json(dir: &Path) -> Result<Value, Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    let mut names: std::collections::BTreeSet<String> = model.symbols.iter().map(|s| s.lib_id.clone()).collect();
    names.extend(eda_model::symbol::builtin_catalog().into_iter().map(|s| s.lib_id));
    // `project`: the entries of the project library itself -- see `footprint_library_json`.
    let mut project: Vec<String> = Vec::new();
    if let Some(lib) = &design.symbol_library {
        names.extend(lib.symbols.iter().map(|s| s.lib_id.clone()));
        project = lib.symbols.iter().map(|s| s.lib_id.clone()).collect();
        project.sort();
    }
    Ok(json!({ "names": names.into_iter().collect::<Vec<_>>(), "project": project }))
}

/// `GET /api/symbol/export?lib_id=<id>` -- the derived, standalone
/// `.kicad_sym` for one symbol-library entry (Symbol Editor's "Export
/// .kicad_sym"), same shape as `footprint_kicad_mod`.
fn symbol_kicad_sym(dir: &Path, lib_id: &str) -> Result<String, Vec<CheckResult>> {
    // The project entry, else the resolved symbol (real library file, builtin table): Export and Copy work on any symbol in the tree.
    crate::library_api::symbol_kicad_sym_any(dir, lib_id)
}

/// `GET /api/symbol_library/export` -- every symbol of the project library in one derived `.kicad_sym`
/// (Symbol Editor's "Save Library As...", Ctrl+Shift+S), the whole-library sibling of [`symbol_kicad_sym`].
fn symbol_library_kicad_sym(dir: &Path) -> Result<String, Vec<CheckResult>> {
    let (_, design, _) = board::load(dir)?;
    let lib = design.symbol_library.as_ref().filter(|l| !l.symbols.is_empty()).ok_or_else(|| vec![CheckResult::fail("ops_unknown_symbol", "symbol_library", "the project symbol library has no symbols yet -- open or create one in the Symbol Editor first")])?;
    let syms: Vec<&eda_model::ir::LibrarySymbol> = lib.symbols.iter().collect();
    Ok(eda_kicad::export_kicad_sym_library(&syms))
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
    // `from_id`/`to_id` (a pad is `REF.NUMBER`) and `from_layers`/`to_layers` (inclusive, indexes into `board.layers`) name what each
    // end joins: the Local Ratsnest tool and the "visible layers" ratsnest mode decide per line from them, as RATSNEST_VIEW_ITEM does.
    let edges: Vec<Value> = report
        .ratsnest
        .iter()
        .map(|e| {
            json!({
                "net": e.net, "from": [e.from.x, e.from.y], "to": [e.to.x, e.to.y],
                "from_id": eda_connectivity::RatsnestEdge::item_id(&e.from_item), "to_id": eda_connectivity::RatsnestEdge::item_id(&e.to_item),
                "from_layers": [e.from_layers.0, e.from_layers.1], "to_layers": [e.to_layers.0, e.to_layers.1],
            })
        })
        .collect();
    Ok(json!({ "edges": edges }))
}

/// `GET /api/fill`: every zone's real computed fill (`eda_zone_filler`, via
/// `eda_drc::fill::fill_all_zones`) as polygons, in the same raw-micrometre
/// coordinate space `/api/ratsnest` already uses, for the UI to draw -- see
/// the task's stage 4. Each zone's fill may be several disjoint fragments
/// (islands); `outline` is always a single closed ring (post-`Fracture`,
/// already slitted, never a separate holes list). `with_polys` (`?polys=1`) adds
/// `polys`: the same fill unfractured, one `{ outline, holes }` per island --
/// what the "Draw Zone Fill Triangulation" display triangulates
/// (`kicad-port/polyTriangulate.ts`), as KiCad's `POLYGON_TRIANGULATION` does
/// (it bridges the holes itself rather than reading the fractured ring).
fn fill_json(dir: &Path, with_polys: bool) -> Result<Value, Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    let zones: &[eda_model::ir::Zone] = design.routing.as_ref().map(|r| r.zones.as_slice()).unwrap_or(&[]);
    let drc_board = eda_drc::board::build(&design, &model);
    let results = eda_drc::fill::fill_all_zones(&drc_board, &model.board);

    let zones_json: Vec<Value> = zones
        .iter()
        .map(|z| {
            let fill = results.get(&z.id);
            let fragments: Vec<Value> = fill
                .map(|f| f.polys.iter().map(|poly| json!(poly[0].iter().map(|p| [p.x, p.y]).collect::<Vec<_>>())).collect())
                .unwrap_or_default();
            let mut zone = json!({
                "id": z.id, "net": z.net, "layer": z.layer,
                "area_um2": fill.map(|f| f.area()).unwrap_or(0.0),
                "fragments": fragments,
            });
            if with_polys {
                let ring = |chain: &Vec<eda_clipper2::Point64>| chain.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>();
                let polys: Vec<Value> = fill
                    .map(|f| {
                        let mut whole = f.clone();
                        whole.unfracture();
                        whole.polys.iter().map(|poly| json!({ "outline": ring(&poly[0]), "holes": poly[1..].iter().map(ring).collect::<Vec<_>>() })).collect()
                    })
                    .unwrap_or_default();
                zone["polys"] = Value::Array(polys);
            }
            zone
        })
        .collect();
    Ok(json!({ "zones": zones_json }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{Point, Provenance, SchematicSection, SheetInstance};

    fn sch(sheets: Vec<SheetInstance>) -> SchematicSection {
        SchematicSection { symbols: vec![], wires: vec![], labels: vec![], texts: vec![], power_symbols: vec![], no_connects: vec![], bus_entries: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), field_layout: Default::default(), title_block: None, sheets, instance_overrides: vec![], junctions: vec![], lines: vec![], extras: Default::default(), imported_from_kicad: false }
    }

    fn design(root: SchematicSection, screens: std::collections::BTreeMap<String, SchematicSection>) -> eda_model::ir::Design {
        eda_model::ir::Design {
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: Some(root),
            nets: None,
            placement: None,
            routing: None,
            drawings: None,
            footprint_library: None,
            sheet_contents: (!screens.is_empty()).then_some(screens),
            bus_aliases: vec![], symbol_library: None,
        }
    }

    /// The 3D export draws copper on a net the netlist no longer names (a net renamed after the board was routed) instead of refusing the board.
    #[test]
    fn the_3d_export_adds_the_nets_the_copper_uses() {
        let mut d = design(sch(vec![]), Default::default());
        d.routing = Some(
            serde_json::from_value(json!({
                "tracks": [{ "net": "GND", "layer": "F.Cu", "width": 200, "pts": [{ "x": 0, "y": 0 }, { "x": 1000, "y": 0 }] }, { "net": "", "layer": "F.Cu", "width": 200, "pts": [{ "x": 0, "y": 0 }, { "x": 0, "y": 500 }] }],
                "vias": [{ "net": "VDD", "at": { "x": 0, "y": 0 }, "diameter": 600, "drill": 300, "from_layer": "F.Cu", "to_layer": "B.Cu" }],
                "zones": []
            }))
            .expect("a routing section"),
        );
        let mut model: eda_model::ConstraintModel = serde_json::from_value(json!({ "parts": [], "nets": [{ "name": "VDD", "pins": [] }] })).expect("a model");
        ensure_routed_nets(&d, &mut model);
        let names: Vec<&str> = model.nets.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["VDD", "GND"], "GND is added once, VDD (already there) is not, and an unnamed net is not made up");
    }

    #[test]
    fn a_library_symbol_reaches_the_painter_with_the_names_it_reads() {
        // `Device:R`: a body (rectangle) and two pins. The painter drops an item whose `body_style` is neither 0 nor the placed symbol's -- a symbol
        // from a library had no body and no pins on the canvas while the JSON left it out -- and sizes strokes by `stroke_width`.
        let r = eda_model::symbol::builtin("Device:R").expect("the built-in resistor");
        let j = lib_symbol_json(&r);
        let rect = j["graphics"].as_array().unwrap().iter().find(|g| g["kind"] == "rectangle").expect("the body");
        assert_eq!(rect["body_style"].as_u64(), Some(0));
        assert!(rect["unit"].is_u64(), "the unit stays as the engine symbol has it");
        assert_eq!(rect["stroke_width"], rect["stroke_mm"], "the width under both names");
        assert_eq!(rect["fill"], "none", "a resistor's body is not filled");
        let pins = j["pins"].as_array().unwrap();
        assert_eq!(pins.len(), 2);
        for p in pins {
            assert_eq!((p["body_style"].as_u64(), p["hidden"].as_bool()), (Some(0), Some(false)));
        }
        // a filled body is the body colour, any other filled shape the outline colour, a text carries its `content`
        let gnd = lib_symbol_json(&eda_model::LibSymbol {
            lib_id: "T:X".into(),
            graphics: vec![
                eda_model::SymbolGraphic::Rectangle { unit: 1, start: eda_model::symbol::SPoint::new(0.0, 0.0), end: eda_model::symbol::SPoint::new(1.0, 1.0), stroke_mm: 0.2, filled: true },
                eda_model::SymbolGraphic::Circle { unit: 0, center: eda_model::symbol::SPoint::new(0.0, 0.0), radius_mm: 0.5, stroke_mm: 0.2, filled: true },
                eda_model::SymbolGraphic::Text { unit: 0, text: "hi".into(), at: eda_model::symbol::SPoint::new(0.0, 0.0), angle_deg: 0.0, size_mm: 1.0 },
            ],
            pins: vec![],
            power: false,
            in_bom: true,
            on_board: true,
            datasheet: String::new(),
            description: String::new(),
            reference_prefix: String::new(),
            unit_count: 1,
            pin_names_hidden: false,
            pin_numbers_hidden: false,
            pin_name_offset_mm: 0.508,
        });
        let g = gnd["graphics"].as_array().unwrap();
        assert_eq!((g[0]["fill"].as_str(), g[1]["fill"].as_str(), g[1]["radius"].as_f64()), (Some("background"), Some("outline"), Some(0.5)));
        assert_eq!(g[2]["content"], "hi");
    }

    #[test]
    fn a_lib_id_query_value_is_percent_decoded() {
        assert_eq!(query_value("/api/symbol?lib_id=Device%3AR", "lib_id"), "Device:R");
        // The footprint routes read `name` the same way: the client encodes the `:` of a library footprint's `Lib:Name`.
        assert_eq!(query_value("/api/footprint?name=Package_SO%3ASOIC-16_3.9x9.9mm_P1.27mm", "name"), "Package_SO:SOIC-16_3.9x9.9mm_P1.27mm");
        assert_eq!(query_value("/api/symbol?x=1&lib_id=eda%3AMy%20Part", "lib_id"), "eda:My Part");
        assert_eq!(query_value("/api/symbol?lib_id=Device:R", "lib_id"), "Device:R", "an unescaped colon still works");
        assert_eq!(query_value("/api/symbol?lib_idx=1", "lib_id"), "", "a longer key is not the key");
        assert_eq!(query_value("/api/symbol", "lib_id"), "");
        assert_eq!(percent_decode("100%"), "100%", "a trailing or malformed escape is kept literally");
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
        assert_eq!(percent_decode("%C3%A9"), "\u{e9}");
    }

    #[test]
    fn empty_sheet_path_resolves_to_the_root() {
        let d = design(sch(vec![]), std::collections::BTreeMap::new());
        let (resolved, breadcrumb) = resolve_sheet(&d, "");
        assert_eq!(resolved.sheets.len(), 0);
        assert!(breadcrumb.is_empty());
    }

    #[test]
    fn a_real_sheet_id_descends_and_builds_the_breadcrumb() {
        let placement = SheetInstance { id: "s1".into(), name: "child".into(), file: "child.kicad_sch".into(), at: Point { x: 0, y: 0 }, size: (1000, 1000), pins: vec![], page: String::new() };
        let child = sch(vec![]);
        let mut screens = std::collections::BTreeMap::new();
        screens.insert("child.kicad_sch".to_string(), child);
        let d = design(sch(vec![placement]), screens);

        let (resolved, breadcrumb) = resolve_sheet(&d, "s1");
        assert_eq!(resolved.sheets.len(), 0, "the child has no sheets of its own");
        assert_eq!(breadcrumb, vec![("s1".to_string(), "child".to_string())]);
    }

    #[test]
    fn an_unknown_sheet_id_falls_back_to_the_last_good_level_instead_of_erroring() {
        let d = design(sch(vec![]), std::collections::BTreeMap::new());
        let (resolved, breadcrumb) = resolve_sheet(&d, "nonexistent");
        assert_eq!(resolved.sheets.len(), 0, "falls back to the root, not a panic/error");
        assert!(breadcrumb.is_empty());
    }
}
