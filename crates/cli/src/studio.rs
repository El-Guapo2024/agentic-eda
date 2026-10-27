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

use eda_model::units::{mm, Nm};
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
use crate::inspect_api;
use crate::tune_api;
use eda_model::footprint::{placed_courtyard_box, placed_pads};
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
pub(crate) fn build_glb(dir: &Path) -> Result<Vec<u8>, String> {
    let (meta, design, mut model) = board::load(dir).map_err(|e| board::reasons(&e))?;
    ensure_routed_nets(&design, &mut model);
    let title = Path::new(&meta.intent).file_stem().and_then(|s| s.to_str()).unwrap_or("board").to_string();
    let date = eda::now_rfc3339();
    let fills = crate::fill_api::cached(dir);
    let pcb_text = eda_kicad::export_kicad_pcb_with(&design, &model, &eda::ExportMeta { date: &date[..10], title: &title }, fills.as_ref()).map_err(|e| board::reasons(&e))?;

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
    // The board's path variables, so a model addressed through one of them (`${TEAM_LIBS}/part.step`, `${KIPRJMOD}/3d/part.step`: a footprint of one of the board's own
    // libraries) is found here as it is in the derived project kicad-cli runs on elsewhere (`eda_kicad::library_table::derive_project`).
    // The project starts as the design's own (`export_kicad_pro_for`, as `eda_kicad_engine` writes it): the variables of Board Setup's Text Variables page, where
    // the board's path variables live too, are in it, and `derive_project` adds the board's tables to it.
    let mut project: serde_json::Value = serde_json::from_str(&eda_kicad::export_kicad_pro_for(&design, &model)).unwrap_or_else(|_| json!({}));
    let _ = eda_kicad::library_table::derive_project(dir, &tmp_dir, &mut project);
    let _ = std::fs::write(tmp_dir.join(format!("{title}.kicad_pro")), serde_json::to_string_pretty(&project).unwrap_or_default());
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
    let mut child = match eda_kicad_engine::cli_command(&kicad_cli_path(), None)
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

/// `POST /api/sim/run` (`crate::sim_api`): the netlist step -- kicad-cli -- on the lane like any kicad-cli job, then ngspice on the same thread
/// outside it, so a long simulation never holds up a DRC or an export. Every run is its own lane job (the counter): two runs never join.
fn offload_sim_run(stream: &TcpStream, lane: &Arc<Lane>, dir: &Path, job: &Job, body: &[u8]) -> Result<(), String> {
    static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut out = stream.try_clone().map_err(|e| e.to_string())?;
    let what = format!("/api/sim/run #{}", RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let (lane, dir, job, body) = (lane.clone(), dir.to_path_buf(), job.clone(), body.to_vec());
    std::thread::Builder::new()
        .name("ngspice".into())
        .spawn(move || {
            let req: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let prepared = std::sync::Mutex::new(None);
            let stamp = lane.run(&what, || version_string(&dir, &job), || {
                if crate::sim_api::slot().is_running() {
                    *prepared.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Err(eda_sim::ngspice::ALREADY_RUNNING.to_string()));
                } else {
                    *prepared.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(crate::sim_api::prepare(&dir, &req));
                }
                json!({})
            });
            let prepared = prepared.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner).unwrap_or_else(|| Err("the simulation was not prepared".to_string()));
            let mut reply = crate::sim_api::simulate(&dir, prepared);
            if let Some(rev) = stamp.get("revision") {
                reply["revision"] = rev.clone();
            }
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
    // A file dropped on the page (a board to append, a DXF or SVG to import) travels in the body, so it is not held to a megabyte.
    let mut body = vec![0u8; length.min(64 << 20)];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    Ok((method, target, body))
}

/// The 3D model routes (crates/cli/src/model3d_api.rs), or `None` for any other request. They need nothing of the request loop, so a connection's own thread answers
/// them (`on_own_thread`, see `serve`): a model never waits behind a slow `/api/state`.
///
///   - `GET /api/3dmodel?name=..`: one footprint 3D model, read-only, for the 3D view's VRMLLoader: resolved like KiCad does and held to an allow-list; a STEP model is
///     converted to VRML by kicad-cli in the background and cached. `&wait=1` holds the request until the model is in.
///   - `POST /api/3dmodel/prepare`: every model of the board at once, queued for one conversion run.
///   - `POST /api/3dmodel/check`, `GET /api/3dmodel/browse`, `/migrate` and `/preview`: what the 3D Models tab of Footprint Properties asks besides the models (crates/cli/src/model3d_tab_api.rs).
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
        ("POST", "/api/3dmodel/check") => {
            let r = crate::model3d_tab_api::check(dir, body);
            Some(respond(stream, r.status, r.kind, &r.body))
        }
        ("GET", "/api/3dmodel/browse") => {
            let r = crate::model3d_tab_api::browse(dir, target);
            Some(respond(stream, r.status, r.kind, &r.body))
        }
        ("GET", "/api/3dmodel/migrate") => {
            let r = crate::model3d_tab_api::migrate(dir, target);
            Some(respond(stream, r.status, r.kind, &r.body))
        }
        ("GET", "/api/3dmodel/preview") => {
            let r = crate::model3d_tab_api::preview(dir, target);
            Some(respond(stream, r.status, r.kind, &r.body))
        }
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
        // The pixels of a board's reference image and of a schematic's image (the state describes them; crates/cli/src/image_api.rs).
        ("GET", "/api/pcb_image") => {
            let r = crate::image_api::pcb_image(dir, target);
            respond(stream, r.status, r.kind, &r.body)
        }
        ("GET", "/api/sch_image") => {
            let r = crate::image_api::sch_image(dir, target);
            respond(stream, r.status, r.kind, &r.body)
        }
        // The shape of a barcode, as kicad-cli plots it (crates/cli/src/barcode_api.rs): `202 {"status":"pending"}` while the plot runs, the browser asks again.
        ("GET", "/api/barcode_shape") => {
            let r = crate::barcode_api::shape_route(dir, target);
            respond(stream, r.status, "application/json", r.body.as_bytes())
        }
        ("POST", "/api/barcode_preview") => {
            let r = crate::barcode_api::preview_route(dir, &body);
            respond(stream, r.status, "application/json", r.body.as_bytes())
        }
        ("GET", "/api/state") => {
            let v = state(dir, job, crate::text_vars_json::clock_of(target)).unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
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
            let v = schematic_json(dir, sheet_path, crate::text_vars_json::clock_of(target)).unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
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
        // The dialogs' "Save...": kicad-cli's own report file (`ERC_REPORT`, `DRC_REPORT`), for the severities and units the dialog shows.
        ("POST", "/api/erc/report") => offload(stream, lane, dir, job, path, &body, crate::kicad_engine::erc_report),
        ("POST", "/api/drc/report") => offload(stream, lane, dir, job, path, &body, crate::kicad_engine::drc_report),
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
        // Annotate Schematic's message window and the other dialogs that work across sheets (`crate::sch_hierarchy_api`).
        ("POST", "/api/sch/annotate") => respond(stream, "200 OK", "application/json", crate::sch_hierarchy_api::annotate(dir, &body).to_string().as_bytes()),
        // Import Sheet / Draw Sheet from File: the project's schematics, and the checks on one (`LoadSheetFromFile`) before it is taken in.
        ("GET", "/api/sch/sheet_files") => respond(stream, "200 OK", "application/json", crate::sch_hierarchy_api::sheet_files(dir).to_string().as_bytes()),
        ("POST", "/api/sch/sheet_file") => respond(stream, "200 OK", "application/json", crate::sch_hierarchy_api::sheet_file(dir, &body).to_string().as_bytes()),
        // A schematic file dropped on the sheet (`ddAppendFile`): the files of the drop put where Import Sheet reads them (a browser gives no path), the sheets they place found among them.
        ("POST", "/api/sch/dropped_sheets") => respond(stream, "200 OK", "application/json", crate::sheet_drop_api::put(&body).to_string().as_bytes()),
        ("GET", "/api/sch/erc_pin_map") => respond(stream, "200 OK", "application/json", sch_api::erc_pin_map(dir).to_string().as_bytes()),
        ("GET", "/api/sch/erc_severities") => respond(stream, "200 OK", "application/json", sch_api::erc_severities(dir).to_string().as_bytes()),
        ("GET", "/api/sch/formatting") => respond(stream, "200 OK", "application/json", sch_api::formatting(dir).to_string().as_bytes()),
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
            // `flatten=1`: the symbol with everything it inherits from its parents -- what the editor draws of a derived symbol
            let v = if query_value(target, "flatten") == "1" {
                crate::library_api::symbol_flattened(dir, &lib_id).map_err(|e| e).and_then(|s| serde_json::to_value(s).map_err(|e| vec![CheckResult::fail("board_bad_design", &lib_id, e.to_string())]))
            } else {
                symbol_edit_json(dir, &lib_id)
            }
            .unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
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
        // The libraries of the board's library tables -- KiCad's installed ones (155 footprint, 223 symbol libraries) through the global table, the
        // board's own through its project table -- for the library trees: names only, one library at a time, cached (`crate::library_index`); the
        // libraries are too big to send or parse whole.
        ("GET", "/api/library/index" | "/api/library/items" | "/api/library/all") => {
            let reply = match crate::library_index::Kind::parse(&query_value(target, "kind")) {
                None => json!({ "error": "kind is footprint or symbol" }),
                Some(kind) => match path {
                    "/api/library/index" => crate::library_index::libraries(dir, kind),
                    "/api/library/items" => crate::library_index::items(dir, kind, &query_value(target, "lib")),
                    _ => crate::library_index::all(dir, kind),
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
                        "/api/library/entries" => crate::library_search::entries(dir, kind, &query_value(target, "lib"), &filter),
                        "/api/library/search" => {
                            let limit = query_value(target, "limit").parse::<usize>().ok().filter(|&n| n > 0).map_or(crate::library_search::SEARCH_LIMIT, |n| n.min(2000));
                            crate::library_search::search(dir, kind, &query_value(target, "q"), &filter, limit)
                        }
                        _ => crate::library_search::details(dir, kind, &query_value(target, "id")),
                    }
                }
            };
            respond(stream, "200 OK", "application/json", reply.to_string().as_bytes())
        }
        ("GET", "/api/library/project") => respond(stream, "200 OK", "application/json", crate::library_api::project_symbols(dir).to_string().as_bytes()),
        // The library tables (`crate::library_tables`): KiCad's global ones, read only, and the board's own `sym-lib-table` / `fp-lib-table`, which the Manage Symbol Libraries
        // and Manage Footprint Libraries dialogs edit; New Library and Add Existing Library; the board's path variables (Configure Paths); and the folder listing the browse
        // buttons use in place of a native file dialog.
        ("GET", "/api/library_tables") => respond(stream, "200 OK", "application/json", crate::library_tables::view(dir).to_string().as_bytes()),
        ("POST", "/api/library_tables") => respond(stream, "200 OK", "application/json", crate::library_tables::set_table(dir, &body).to_string().as_bytes()),
        ("POST", "/api/library_tables/check") => respond(stream, "200 OK", "application/json", crate::library_tables::check_rows(dir, &body).to_string().as_bytes()),
        ("GET", "/api/library_tables/normalize") => respond(stream, "200 OK", "application/json", crate::library_tables::normalize(dir, &query_value(target, "path")).to_string().as_bytes()),
        ("POST", "/api/library_tables/add") => respond(stream, "200 OK", "application/json", crate::library_tables::add_library(dir, &body).to_string().as_bytes()),
        ("POST", "/api/path_variables") => respond(stream, "200 OK", "application/json", crate::library_tables::set_variables(dir, &body).to_string().as_bytes()),
        ("GET", "/api/fs/list") => respond(stream, "200 OK", "application/json", crate::library_tables::browse(dir, &query_value(target, "path"), &query_value(target, "what")).to_string().as_bytes()),
        // Import / Paste in the two library editors: the read-only half (`crate::library_api`); the store is a `put_library_*` verb.
        ("POST", "/api/symbol_library/parse") => respond(stream, "200 OK", "application/json", crate::library_api::parse_symbols(&body).to_string().as_bytes()),
        ("POST", "/api/footprint/parse") => respond(stream, "200 OK", "application/json", crate::library_api::parse_footprint(&body).to_string().as_bytes()),
        // The Embedded Files panel (`crate::embedded_api`): a list without the data, one file extracted, and what Collect and Embed 3D Models would embed.
        ("GET", "/api/embedded") => {
            let r = crate::embedded_api::list(dir, target);
            respond(stream, r.status, r.kind, &r.body)
        }
        ("GET", "/api/embedded/file") => {
            let r = crate::embedded_api::file(dir, target);
            respond(stream, r.status, r.kind, &r.body)
        }
        ("GET", "/api/embedded/collect") => {
            let r = crate::embedded_api::collect_plan(dir);
            respond(stream, r.status, r.kind, &r.body)
        }
        ("GET", "/api/symbol_library/export") => match symbol_library_kicad_sym(dir) {
            Ok(text) => respond(stream, "200 OK", "text/plain; charset=utf-8", text.as_bytes()),
            Err(e) => respond(stream, "404 Not Found", "text/plain", board::reasons(&e).as_bytes()),
        },
        ("GET", "/api/fill") => {
            let v = crate::fill_api::report(dir, query_value(target, "polys") == "1").unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
            respond(stream, "200 OK", "application/json", v.to_string().as_bytes())
        }
        // Fill All Zones (B) by KiCad's own filler -- the only one there is: `kicad-cli pcb drc --refill-zones --save-board` on a copy of the derived board, the fills kept
        // beside design.json (`crate::fill_api`). `{"zones": [ids]}` fills the selected zones only (the others keep the fill they had). A kicad-cli route, so it runs off the
        // loop like DRC. `/api/fill/clear` (Unfill All Zones / Unfill Selected Zones) only forgets fills, so it answers inline.
        ("POST", "/api/fill/kicad") => offload(stream, lane, dir, job, path, &body, |dir, body| crate::fill_api::refill(dir, crate::fill_api::zones_asked(body))),
        // The Zone Manager's Auto-Assign Priorities (`eda_ops::zone_priority`): the priorities KiCad would give the copper pours, worked out and not applied.
        ("GET", "/api/zone_priorities/auto") => respond(stream, "200 OK", "application/json", crate::fill_api::auto_priorities(dir).to_string().as_bytes()),
        ("POST", "/api/fill/clear") => respond(stream, "200 OK", "application/json", crate::fill_api::clear(dir, crate::fill_api::zones_asked(&body)).to_string().as_bytes()),
        // PCB Copy: the selection as KiCad's clipboard text (a read; the browser puts it on the system clipboard). Paste is `Cmd::PasteClipboard`.
        ("POST", "/api/clipboard/copy") => respond(stream, "200 OK", "application/json", crate::clipboard_api::copy(dir, &body).to_string().as_bytes()),
        // Import Graphics (DXF, SVG): what a file holds, for the dialog (a read; the import is `Cmd::ImportGraphics`).
        ("POST", "/api/import_graphics/read") => respond(stream, "200 OK", "application/json", crate::graphics_api::read(&body).to_string().as_bytes()),
        // File > Import > Non-KiCad Board / Schematic: the file the page posts converted by kicad-cli (so through `offload`) and read; the import itself is
        // `Cmd::ReplaceBoard` / `Cmd::ReplaceSchematic` with what this answers.
        ("POST", "/api/import_foreign") => offload(stream, lane, dir, job, path, &body, crate::foreign_api::convert),
        // Append Board: what a `.kicad_pcb` brings and the point it is held by (a read; the append is `Cmd::PasteClipboard` with the file's text).
        ("POST", "/api/append_board/read") => respond(stream, "200 OK", "application/json", crate::append_api::read(dir, &body).to_string().as_bytes()),
        // The microwave tools' Complex Shape dialog: the scales and the point count of a shape description file (the shape itself is `Cmd::Microwave` with the file's text).
        ("POST", "/api/microwave/shape") => respond(stream, "200 OK", "application/json", crate::microwave_api::read_shape(&body).to_string().as_bytes()),
        ("POST", "/api/microwave/inductor") => respond(stream, "200 OK", "application/json", crate::microwave_api::check_inductor(&body).to_string().as_bytes()),
        // The Design Blocks panels (`crate::design_block_api`): the libraries and their blocks, a block's board file, its preview (kicad-cli, off the loop), and the dialogs'
        // reads and writes. Placing a block is `Cmd::ImportSheet` / `Cmd::PlaceDesignBlock`; the save also applies the link of a selection to the block (`LinkDesignBlock`).
        ("GET", "/api/design_blocks") => respond(stream, "200 OK", "application/json", crate::design_block_api::list(dir).to_string().as_bytes()),
        ("GET", "/api/design_blocks/file") => respond(stream, "200 OK", "application/json", crate::design_block_api::file(dir, &query_value(target, "lib"), &query_value(target, "name"), &query_value(target, "what")).to_string().as_bytes()),
        ("GET", "/api/design_blocks/preview") => {
            let (lib, name, what) = (query_value(target, "lib"), query_value(target, "name"), query_value(target, "what"));
            offload(stream, lane, dir, job, &format!("{path} {lib} {name} {what}"), &[], move |dir, _| crate::design_block_api::preview(dir, &lib, &name, &what))
        }
        ("POST", "/api/design_blocks/prepare") => respond(stream, "200 OK", "application/json", crate::design_block_api::prepare(dir, &body).to_string().as_bytes()),
        ("POST", "/api/design_blocks/save") => respond(stream, "200 OK", "application/json", crate::design_block_api::save(dir, &body).to_string().as_bytes()),
        ("POST", "/api/design_blocks/properties") => respond(stream, "200 OK", "application/json", crate::design_block_api::properties(dir, &body).to_string().as_bytes()),
        // The multichannel tool's dialogs (`crate::multichannel_api`): the rows of Generate Placement Rule Areas, the sources the Placement tab offers, and the table of Repeat Layout.
        ("POST", "/api/multichannel/areas") => respond(stream, "200 OK", "application/json", crate::multichannel_api::areas(dir).to_string().as_bytes()),
        ("POST", "/api/multichannel/sources") => respond(stream, "200 OK", "application/json", crate::multichannel_api::sources(dir).to_string().as_bytes()),
        ("POST", "/api/multichannel/compat") => respond(stream, "200 OK", "application/json", crate::multichannel_api::compat(dir, &body).to_string().as_bytes()),
        ("POST", "/api/design_blocks/delete") => respond(stream, "200 OK", "application/json", crate::design_block_api::delete(dir, &body).to_string().as_bytes()),
        // What Swap Pad Nets / Swap Gate Nets would ask first: the unselected pads the copper connected to the swapped pads reaches (`PromptConnectedPadDecision`).
        ("GET", "/api/rule_editor") => respond(stream, "200 OK", "application/json", crate::rule_editor_api::view(dir).to_string().as_bytes()),
        ("POST", "/api/rule_editor/check") => respond(stream, "200 OK", "application/json", crate::rule_editor_api::check(dir, &body).to_string().as_bytes()),
        ("POST", "/api/swap_nets/preview") => respond(stream, "200 OK", "application/json", crate::pad_nets_api::preview(dir, &body).to_string().as_bytes()),
        // What Track & Via Properties' OK would ask first: a short, then the pads a net change takes along (`confirmShortingNets`, `confirmPadChange`).
        ("POST", "/api/track_via_nets/preview") => respond(stream, "200 OK", "application/json", crate::pad_nets_api::track_via_preview(dir, &body).to_string().as_bytes()),
        ("POST", "/api/cmd") => {
            let req: Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
            let strict = req.get("strict").and_then(Value::as_bool).unwrap_or(true);
            // `"carry": "begin"` / `"commit"`: the command starts or ends a carried placement (Duplicate and Paste and the drop are one undo step; `carry.rs`).
            let carry = req.get("carry").and_then(Value::as_str).and_then(board::carry::Carry::parse);
            let reply = match serde_json::from_value::<eda_ops::Cmd>(req.get("cmd").cloned().unwrap_or(Value::Null)) {
                Err(e) => json!({ "ok": false, "message": format!("not a board command: {e}") }),
                Ok(cmd) => {
                    // A Specctra session KiCad imported with items skipped (`crate::specctra_api::take_session_warning`): its warning travels in the reply.
                    let session = cmd.imports_session();
                    let _ = session.then(crate::specctra_api::take_session_warning);
                    match board::step_carrying(dir, cmd, if strict { board::Strictness::Gates } else { board::Strictness::Off }, "ui", carry) {
                        Ok(summary) => match session.then(crate::specctra_api::take_session_warning).flatten() {
                            Some(w) => json!({ "ok": true, "message": summary, "warning": w }),
                            None => json!({ "ok": true, "message": summary }),
                        },
                        Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
                    }
                }
            };
            respond(stream, "200 OK", "application/json", reply.to_string().as_bytes())
        }
        // The end of a carried placement without a command of its own: `{"action": "commit"}` (a drop where the items already were) or `{"action": "cancel"}` (Escape).
        ("POST", "/api/carry") => {
            let req: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let done = match req.get("action").and_then(Value::as_str) {
                Some("commit") => board::carry::commit(dir),
                Some("cancel") => board::carry::cancel(dir, "ui"),
                _ => Err(vec![eda_model::CheckResult::fail("board_bad_carry", "carry", "the action is commit or cancel")]),
            };
            let reply = match done {
                Ok(summary) => json!({ "ok": true, "message": summary }),
                Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
            };
            respond(stream, "200 OK", "application/json", reply.to_string().as_bytes())
        }
        ("GET", "/api/view") => respond(stream, "200 OK", "application/json", crate::view_api::get(dir).to_string().as_bytes()),
        ("POST", "/api/view") => respond(stream, "200 OK", "application/json", crate::view_api::post(dir, &String::from_utf8_lossy(&body)).to_string().as_bytes()),
        // The Appearance panel's per-project settings (`appearance.json`, never `design.json`).
        // (the nets it names under the board's names: a colour saved under the intent's name of a net is the net's under KiCad's)
        ("GET", "/api/appearance") => {
            let a = crate::appearance_api::get(dir);
            let a = match crate::board::load(dir) {
                Ok((_, _, model)) => eda_kicad::appearance::with_board_net_names(&a, &model),
                Err(_) => a,
            };
            respond(stream, "200 OK", "application/json", a.to_string().as_bytes())
        }
        ("POST", "/api/appearance") => respond(stream, "200 OK", "application/json", crate::appearance_api::post(dir, &String::from_utf8_lossy(&body)).to_string().as_bytes()),
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
        ("POST", "/api/route/attempt_finish") => respond(stream, "200 OK", "application/json", route_api::attempt_finish(dir, route_session, &body).to_string().as_bytes()),
        ("POST", "/api/route/continue_from_end") => respond(stream, "200 OK", "application/json", route_api::continue_from_end(dir, route_session, &body).to_string().as_bytes()),
        ("POST", "/api/route/cancel") => respond(stream, "200 OK", "application/json", route_api::cancel(dir, route_session).to_string().as_bytes()),
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
        ("POST", "/api/route/dp_via") => respond(stream, "200 OK", "application/json", route_api::dp_via(route_session, &body).to_string().as_bytes()),
        ("POST", "/api/route/dp_undo_segment") => respond(stream, "200 OK", "application/json", route_api::dp_undo_segment(route_session).to_string().as_bytes()),
        ("POST", "/api/route/dp_continue_from_end") => respond(stream, "200 OK", "application/json", route_api::dp_continue_from_end(dir, route_session, &body).to_string().as_bytes()),
        ("POST", "/api/route/dp_finish") => respond(stream, "200 OK", "application/json", route_api::dp_finish(dir, route_session, &body).to_string().as_bytes()),
        // 7 / 8 / 9 (P5c): length tuning -- KiCad's tuning pattern tool and its generator items; the session lives in tune_api's own thread-local, not in a cell passed here.
        ("POST", "/api/tune/hover") => respond(stream, "200 OK", "application/json", tune_api::hover(dir, &body).to_string().as_bytes()),
        ("POST", "/api/tune/start") => respond(stream, "200 OK", "application/json", tune_api::start(dir, &body).to_string().as_bytes()),
        ("POST", "/api/tune/move") => respond(stream, "200 OK", "application/json", tune_api::mv(dir, &body).to_string().as_bytes()),
        ("POST", "/api/tune/step") => respond(stream, "200 OK", "application/json", tune_api::step(dir, &body).to_string().as_bytes()),
        ("POST", "/api/tune/settings") => respond(stream, "200 OK", "application/json", tune_api::settings(dir, &body).to_string().as_bytes()),
        ("POST", "/api/tune/fix") => respond(stream, "200 OK", "application/json", tune_api::fix(dir, &body).to_string().as_bytes()),
        ("POST", "/api/tune/edit_start") => respond(stream, "200 OK", "application/json", tune_api::edit_start(dir, &body).to_string().as_bytes()),
        ("POST", "/api/tune/edit_move") => respond(stream, "200 OK", "application/json", tune_api::edit_move(dir, &body).to_string().as_bytes()),
        ("POST", "/api/tune/cancel") => respond(stream, "200 OK", "application/json", tune_api::cancel(dir).to_string().as_bytes()),
        ("POST", "/api/tune/patterns") => respond(stream, "200 OK", "application/json", tune_api::patterns(dir).to_string().as_bytes()),
        ("POST", "/api/tune/pattern") => respond(stream, "200 OK", "application/json", tune_api::pattern(dir, &body).to_string().as_bytes()),
        ("POST", "/api/tune/update") => respond(stream, "200 OK", "application/json", tune_api::update(dir, &body).to_string().as_bytes()),
        ("POST", "/api/tune/remove") => respond(stream, "200 OK", "application/json", tune_api::remove(dir, &body).to_string().as_bytes()),
        ("POST", "/api/tune/regenerate") => respond(stream, "200 OK", "application/json", tune_api::regenerate(dir, &body).to_string().as_bytes()),
        // Inspect > Clearance Resolution / Constraints Resolution: read-only reports on which rule wins (`inspect_api`), no kicad-cli.
        ("GET", "/api/net_inspector") => respond(stream, "200 OK", "application/json", crate::net_inspector_api::net_inspector(dir).to_string().as_bytes()),
        ("POST", "/api/inspect/clearance") => respond(stream, "200 OK", "application/json", inspect_api::clearance(dir, &body).to_string().as_bytes()),
        ("POST", "/api/inspect/constraints") => respond(stream, "200 OK", "application/json", inspect_api::constraints(dir, &body).to_string().as_bytes()),
        // Find by Properties' Query tab: the expression language runs here, on the items the studio sends (`eda_drc::query`).
        ("POST", "/api/pcb/query") => respond(stream, "200 OK", "application/json", eda_drc::query::handle(&body).to_string().as_bytes()),
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
        // The Footprint Editor's Footprint Checker: kicad-cli's DRC on a scratch board that holds the footprint being edited; kicad-cli's, so off the loop like DRC.
        ("POST", "/api/footprint/check") => offload(stream, lane, dir, job, path, &body, crate::kicad_engine::footprint_check),
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
        // The SPICE simulator (`crate::sim_api`): the circuit is kicad-cli's, so off the loop; a run is kicad-cli then ngspice; Stop and the
        // workbook's text are answered here (Stop must not wait behind the run it stops).
        ("POST", "/api/sim/circuit") => offload(stream, lane, dir, job, path, &body, crate::sim_api::circuit_route),
        ("POST", "/api/sim/run") => offload_sim_run(stream, lane, dir, job, &body),
        ("POST", "/api/sim/stop") => respond(stream, "200 OK", "application/json", crate::sim_api::stop_route().to_string().as_bytes()),
        ("POST", "/api/sim/workbook/parse") => respond(stream, "200 OK", "application/json", crate::sim_api::workbook_parse_route(&body).to_string().as_bytes()),
        ("POST", "/api/sim/workbook/text") => respond(stream, "200 OK", "application/json", crate::sim_api::workbook_text_route(&body).to_string().as_bytes()),
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
        ("POST", "/api/fab/idf") => offload(stream, lane, dir, job, path, &body, crate::board_output_api::idf),
        // Which of the exporters a kicad-cli may lack (IDF, GenCAD) this one has: one `kicad-cli pcb export --help` per process, so off the loop.
        ("GET", "/api/fab/exporters") => offload(stream, lane, dir, job, path, &body, |_, _| crate::board_output_api::exporters()),
        ("POST", "/api/fab/ipcd356") => offload(stream, lane, dir, job, path, &body, |dir, _| crate::board_output_api::ipcd356(dir)),
        ("POST", "/api/fab/ipc2581") => offload(stream, lane, dir, job, path, &body, crate::board_output_api::ipc2581),
        ("POST", "/api/fab/odb") => offload(stream, lane, dir, job, path, &body, crate::board_output_api::odb),
        ("POST", "/api/fab/pcb_bom") => offload(stream, lane, dir, job, path, &body, |dir, _| crate::board_output_api::pcb_bom(dir)),
        // Board control that is not a kicad-cli run (`crate::board_control_api`): cheap, answered on the loop.
        ("POST", "/api/repair_board") => respond(stream, "200 OK", "application/json", crate::board_control_api::repair_board(dir).to_string().as_bytes()),
        // The footprint assignment tools (`crate::fp_assign_api`)
        ("GET", "/api/update_pcb") => respond(stream, "200 OK", "application/json", crate::fp_assign_api::update_pcb(dir).to_string().as_bytes()),
        ("POST", "/api/exchange_plan") => respond(stream, "200 OK", "application/json", crate::fp_assign_api::exchange_plan(dir, &body).to_string().as_bytes()),
        ("POST", "/api/reannotate_plan") => respond(stream, "200 OK", "application/json", crate::fp_assign_api::reannotate_plan(dir, &body).to_string().as_bytes()),
        ("GET", "/api/footprint_diff") => respond(stream, "200 OK", "application/json", crate::fp_assign_api::footprint_diff(dir, &query_value(target, "ref")).to_string().as_bytes()),
        ("GET", "/api/footprint_associations") => respond(stream, "200 OK", "application/json", crate::board_control_api::footprint_associations(dir, &query_value(target, "ref")).to_string().as_bytes()),
        ("POST", "/api/fab/cmp") => respond(stream, "200 OK", "application/json", crate::board_control_api::export_cmp(dir).to_string().as_bytes()),
        // File > Export > Specctra DSN...: kicad-cli's `pcb export specctra` where it has it, so off the loop (`offload`) like the other exports.
        ("POST", "/api/specctra/export") => offload(stream, lane, dir, job, path, &body, |dir, _| crate::specctra_api::export(dir)),
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
    // The fills kicad-cli made (Fill All Zones) are part of what the board shows: the 3D view is rebuilt from them and a DRC run before them is out of date.
    format!("{}-{}-{}-{}", stamp(dir.join("design.json")), stamp(dir.join("activity.jsonl")), j.len() + j.bytes().map(|b| b as usize).sum::<usize>(), stamp(eda_kicad::zone_fills::cache_path(dir)))
}

fn version(dir: &Path, job: &Job) -> Value {
    json!(version_string(dir, job))
}

pub(crate) fn state(dir: &Path, job: &Job, clock: eda_model::text_vars::Clock) -> Result<Value, Vec<CheckResult>> {
    let (meta, design, model) = board::load(dir)?;
    // What `${NAME}` stands for in the board's texts and fields (`crate::text_vars_json`); each of them carries `shown` where it differs from the stored text.
    let tv = crate::text_vars_json::PcbShown::new(dir, &design, &model, crate::text_vars_json::project(dir, &meta.intent, clock));
    let b = eda_ops::Board::new(design.clone(), &model, meta.snap_nm, meta.spacing_nm);
    let view = eda_ops::view::view(&b, &model)?;
    // The copper layers the pads and vias that remove their unconnected layers (Remove Unused Pads) keep: empty, and not asked for, when none does.
    let flashed = eda_connectivity::flashed_layers(&design, &model);
    let block_of: std::collections::HashMap<String, String> = view["parts"]
        .as_array()
        .map(|ps| ps.iter().filter_map(|p| Some((p["ref"].as_str()?.to_string(), p["block"].as_str().unwrap_or("").to_string()))).collect())
        .unwrap_or_default();
    let net_of: std::collections::HashMap<&str, &str> = model.nets.iter().flat_map(|n| n.pins.iter().map(move |p| (p.as_str(), n.name.as_str()))).collect();
    let pl = design.placement.as_ref();
    // The footprint libraries the parts' bodies are looked up in (the board's library tables), read when the first part needs them.
    let body_libs = crate::body_api::Libs::of_board(dir);
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
            let footprint = model.footprint_of(part);
            let engine_pads = footprint.as_ref().map(crate::fp_json::pads_in_studio_order).unwrap_or_default();
            let pads: Vec<Value> = placed_pads(&model, part, fp)
                .unwrap_or_default()
                .iter()
                .enumerate()
                .map(|(i, q)| {
                    let net = net_of.get(format!("{}.{}", part.reference, q.number).as_str()).copied();
                    match engine_pads.get(i) {
                        // The pad with its own shape, type, hole and overrides (`crate::fp_json`).
                        Some((pad, nth)) => crate::fp_json::pad_json(&design, &model, fp, q, pad, *nth, net, &flashed),
                        None => json!({ "num": q.number, "net": net, "x": q.center.x, "y": q.center.y, "w": q.size.0, "h": q.size.1, "round": q.is_round(), "th": q.through_hole }),
                    }
                })
                .collect();
            if let Some(footprint) = &footprint {
                // The footprint's fields as laid out on the board, and its attributes.
                p["fields"] = json!(crate::fp_json::fields_json(&design, part, fp, footprint));
                tv.annotate_fields(&part.reference, &mut p["fields"]);
                p["attrs"] = crate::fp_json::attrs_json(&design, fp, footprint);
                // A multi-gate footprint's units (`FOOTPRINT::GetUnitInfo`), for Swap Gate Nets' menu: absent for fewer than two.
                let units = eda_model::fp_units::unit_info(&design, &model, &part.reference);
                if units.len() >= 2 {
                    p["units"] = json!(units);
                }
                // Its silkscreen, fab drawings and texts (`graphics`, `texts`), board space.
                crate::fp_art_json::add_art(&mut p, part, fp, footprint);
            }
            p["at"] = json!([fp.at.x, fp.at.y]);
            p["rot"] = json!(fp.rot as f64 / 1000.0);
            // KiCad's orientation (`FOOTPRINT::GetOrientationDegrees`, the file's angle): a back footprint made or flipped here is half a turn from `-rot`.
            p["orientation"] = json!(eda_kicad::kicad_footprint_orientation(&design, fp) as f64 / 1000.0);
            p["side"] = json!(if fp.side == Side::Bottom { "bottom" } else { "top" });
            p["label"] = json!(match fp.label {
                LabelSide::Above => "above",
                LabelSide::Below => "below",
                LabelSide::Left => "left",
                LabelSide::Right => "right",
            });
            // The box of the courtyard the board file has (`FOOTPRINT::GetBoundingBox` merges the `F.CrtYd` drawings as they are): an off-centre one stays off-centre,
            // so Align, Distribute, the Rotate and Flip pivot and a click's hit test measure the footprint as KiCad does (not the placer's symmetric box).
            p["courtyard"] = json!(placed_courtyard_box(&model, part, fp).map(|c| [c.0, c.1, c.2, c.3]));
            // The part's body (the box of its footprint's `F.Fab` graphics, board space) for the 3D view's fallback boxes -- the courtyard is the body plus its
            // clearance and the pads' reach. Absent when no `F.Fab` is known for the footprint: the view then sizes the box from the courtyard.
            // The footprint's own fab drawings first (they are what the board draws), else the library's.
            p["body"] = json!(resolved.as_ref().and_then(|f| crate::fp_art_json::placed_body(fp, f)).or_else(|| crate::body_api::placed_body(&body_libs, &design, part, fp)).map(|c| [c.0, c.1, c.2, c.3]));
            // The part's 3D models with the placement its footprint gives each, and whether KiCad counts it a through-hole, SMD or virtual model: what the 3D
            // view loads (`GET /api/3dmodel`) and places. Absent when the footprint names no model: the view then draws the body box.
            if let Some(models) = resolved.as_ref().and_then(|f| crate::model3d_api::part_json(&design, &model, part, f)) {
                p["models"] = models["models"].clone();
                p["kind3d"] = models["kind3d"].clone();
                p["dnp"] = models["dnp"].clone();
                p["not_in_pos"] = models["not_in_pos"].clone();
            }
            p["pads"] = json!(pads);
            // The zone-connection facts a pad or footprint carries (Pad Properties and Footprint Properties' zone fields), only when
            // there are any: `Design::pad_zone_facts` resolves an edit made on the board over an import's and a published library's.
            if let Some(footprint) = model.footprint_of(part) {
                let n = footprint.pads.len();
                let facts: Vec<_> = footprint.pads.iter().enumerate().map(|(i, pad)| (&pad.number, design.pad_zone_facts(&part.reference, &footprint.name, i, n, &pad.number))).collect();
                let own = |f: &eda_model::ir::PadZoneFacts| f.connection.is_some() || f.thermal_gap.is_some() || f.thermal_spoke_width.is_some() || f.thermal_spoke_angle_mdeg.is_some() || f.pad_clearance.is_some();
                let fp_level = facts.first().map(|(_, f)| (f.footprint_connection, f.footprint_clearance)).unwrap_or_default();
                if fp_level.0.is_some() || fp_level.1.is_some() || facts.iter().any(|(_, f)| own(f)) {
                    p["zone"] = json!({
                        "connection": fp_level.0,
                        "clearance": fp_level.1,
                        "pads": facts.iter().filter(|(_, f)| own(f)).map(|(num, f)| json!({
                            "num": num, "connection": f.connection, "gap": f.thermal_gap, "spoke_width": f.thermal_spoke_width,
                            "spoke_angle_mdeg": f.thermal_spoke_angle_mdeg, "clearance": f.pad_clearance,
                        })).collect::<Vec<_>>(),
                    });
                }
            }
            // `FOOTPRINT_DESC`'s Library Description, Keywords, Component Class and the solder paste overrides: what the board file said of the footprint
            // (`FootprintExtra`: its `(descr ..)`, `(tags ..)`, `(component_classes ..)`, `(solder_paste_margin ..)`, `(solder_paste_margin_ratio ..)`).
            if let Some(e) = design.drawings.as_ref().and_then(|d| d.footprint_extras.iter().find(|e| e.id == part.reference)) {
                let facts = eda_kicad::footprint_file_facts(&e.tokens);
                if let Some(d) = facts.description {
                    p["lib_description"] = json!(d);
                }
                if let Some(k) = facts.keywords {
                    p["keywords"] = json!(k);
                }
                if !e.component_classes.is_empty() {
                    p["component_classes"] = json!(e.component_classes);
                }
                if let Some(m) = facts.paste_margin {
                    p["paste_margin"] = json!(m);
                }
                if let Some(r) = facts.paste_margin_ratio {
                    p["paste_margin_ratio"] = json!(r);
                }
            }
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
            "vias": r.vias.iter().map(|v| {
                let extra = design.drawings.as_ref().and_then(|d| d.via_extra(&v.id));
                json!({
                    "id": v.id, "net": v.net, "x": v.at.x, "y": v.at.y, "d": v.diameter,
                    "drill": v.drill, "from": v.from_layer, "to": v.to_layer,
                    // What the via does with the layers nothing connects it on (`remove_unused_layers`, Remove Unused Pads): `remove_all`,
                    // `remove_except_start_and_end` or `start_end_only` -- absent when it keeps every layer -- and the layers it then has copper on.
                    "unconnected": extra.map(|e| e.unconnected_layers()).filter(|m| m.removes()),
                    // `VIATYPE` when it is not a through via (`blind`, `buried`, `micro`; `DrawingsSection::via_extras`).
                    "type": extra.map(|e| e.kind).filter(|k| *k != eda_model::via_extra::ViaKind::Through),
                    "flashed": flashed.vias.get(&v.id),
                })
            }).collect::<Vec<_>>(),
            "zones": r.zones.iter().map(|z| json!({
                "id": z.id, "net": z.net, "layer": z.layer,
                // `ZONE::GetLayerSet()`: the zone's layers, `layer` first -- one for a zone on one layer.
                "layers": z.layer_set(),
                "outline": z.outline.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(),
                // The outline's holes (`ZONE::Outline()`'s holes), drawn and hit-tested as outside the zone.
                "holes": z.holes.iter().map(|h| h.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>()).collect::<Vec<_>>(),
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
                // `ZONE_SETTINGS::m_cornerSmoothingType` / `m_cornerRadius`: ZoneDialog's "Corner smoothing" and "Radius".
                "smoothing": z.smoothing, "corner_radius": z.corner_radius,
                // `ZONE::GetZoneName()`: the Properties panel's "Name" row (`Cmd::SetZoneName`).
                "name": z.name,
                // The rule area's placement source (`ZONE::GetPlacementAreaEnabled` and the source type and name): the Placement tab of Zone Properties and what Repeat Layout reads.
                "placement": z.placement,
            })).collect::<Vec<_>>(),
            // `BOARD_DESIGN_SETTINGS::m_TrackWidthList`/`m_ViaSizeList`/
            // `m_DiffPairDimensionsList` -- the Board Setup "Track Widths &
            // Vias" panel's editable preset lists
            // (`eda_ops::Cmd::SetTrackWidthPresets`/`SetViaPresets`/
            // `SetDiffPairPresets`), driving the W/Shift+W, via-size-cycle
            // and differential-pair hotkeys. The board's own default
            // (board_rules.track_width/via_diameter/via_drill below) is
            // always the implicit first entry of the track and via lists,
            // and KiCad's diff pair list likewise keeps an unused index 0
            // (`GetDiffPairWidth()` only reads index >= 1); none of those
            // dummy entries is repeated in these lists.
            "track_width_presets": r.track_width_presets,
            "via_presets": r.via_presets.iter().map(|p| json!({ "diameter": p.diameter, "drill": p.drill })).collect::<Vec<_>>(),
            "diff_pair_presets": r.diff_pair_presets.iter().map(|p| json!({ "width": p.width, "gap": p.gap, "via_gap": p.via_gap })).collect::<Vec<_>>(),
            // Board Setup > Teardrops (task item 4).
            "teardrop_settings": json!({
                "enabled": r.teardrop_settings.enabled,
                "target_vias": r.teardrop_settings.target_vias,
                "target_pth_pads": r.teardrop_settings.target_pth_pads,
                "target_smd_pads": r.teardrop_settings.target_smd_pads,
                "best_length_ratio": r.teardrop_settings.best_length_ratio,
                "best_width_ratio": r.teardrop_settings.best_width_ratio,
                "max_len_nm": r.teardrop_settings.max_len_nm,
                "max_width_nm": r.teardrop_settings.max_width_nm,
                "width_to_size_filter_ratio": r.teardrop_settings.width_to_size_filter_ratio,
            }),
        })
    });
    let drawings = design.drawings.as_ref().map(|d| {
        let mut drawings = json!({
            "shapes": d.shapes.iter().map(shape_json).collect::<Vec<_>>(),
            "texts": d.texts.iter().map(|t| tv.text_json(json!({
                "id": t.id, "content": t.content, "x": t.at.x, "y": t.at.y, "angle": t.angle,
                "layer": t.layer, "size": t.size_nm, "stroke_width": t.stroke_width,
                "justify": match t.justify {
                    eda_model::ir::TextJustify::Left => "left",
                    eda_model::ir::TextJustify::Center => "center",
                    eda_model::ir::TextJustify::Right => "right",
                },
                "mirror": t.mirror,
            }))).collect::<Vec<_>>(),
            // Task item 5 -- see `eda_model::ir::Group`'s own doc. A tuning pattern (a `PCB_GENERATOR`, which is a `PCB_GROUP`) is listed with the groups so that it selects as one
            // (`generator` names what it is); `tuning_patterns` has the pattern itself.
            "groups": d.groups.iter().map(|g| json!({ "id": g.id, "name": g.name, "member_ids": g.member_ids, "design_block": g.design_block })).chain(d.tuning_patterns.iter().map(|p| json!({ "id": p.id, "name": "Tuning Pattern", "member_ids": p.member_ids, "generator": "tuning_pattern" }))).collect::<Vec<_>>(),
            "tuning_patterns": d.tuning_patterns.iter().map(|p| {
                let mut v = serde_json::to_value(p).unwrap_or(Value::Null);
                v["handles"] = tune_api::handles_json(p);
                v["info"] = json!(p.tuning_info());
                v
            }).collect::<Vec<_>>(),
            // Task item 7 -- see `eda_model::ir::Dimension`'s own doc.
            "dimensions": d.dimensions.iter().map(dimension_json).collect::<Vec<_>>(),
            "dimension_settings": dimension_settings_json(&d.dimension_settings),
            // `PCB_TABLE`: the grid, the corners of every cell, the border lines and the placed lines of the text -- see `crate::table_json`.
            "tables": d.tables.iter().map(|t| crate::table_json::table_json(t, tv.board())).collect::<Vec<_>>(),
        });
        // The text boxes, reference images and points (`eda_model::pcb_items`).
        crate::pcb_items_json::extend(&mut drawings, &design, &model, d, &tv);
        drawings
    });
    Ok(json!({
        "name": Path::new(&meta.intent).file_stem().and_then(|s| s.to_str()).unwrap_or("board"),
        "dir": dir.display().to_string(),
        "outline": pl.map(|pl| pl.outline.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>()),
        // The outline the way KiCad builds it from Edge.Cuts (`eda_drc::outline::board_outline`): every outline with its cutouts, where `outline`
        // is one polygon. `outline_is_shapes`: the canvas draws the Edge.Cuts shapes themselves and not the polygon. `outline_errors`: what
        // kicad-cli reports as `invalid_outline`.
        "outline_polys": outline_polys_json(&design, Some(&model)),
        "outline_is_shapes": eda_model::outline::outline_is_shapes(&design),
        "outline_errors": outline_errors_json(&design, Some(&model)),
        "layers": model.board.layers,
        "snap": meta.snap_nm,
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
            "hole_to_hole_min_nm": model.board.hole_to_hole_min_nm,
            "hole_clearance_nm": model.board.hole_clearance_nm,
            "silk_clearance_nm": model.board.silk_clearance_nm,
            "annular_width_min_nm": model.board.annular_width_min_nm,
            "min_silk_text_height_nm": model.board.min_silk_text_height_nm,
            "min_silk_text_thickness_nm": model.board.min_silk_text_thickness_nm,
            "refdes_font_nm": model.board.refdes_font_nm,
            "stackup": model.stackup,
            // Board Setup's pages (`eda_model::rules`), as the board is judged by them right now: the intent's or the
            // imported project's values with any edit laid over them (`board::load`). `overlay` names the pages an edit has
            // replaced; `severities` holds only the checks whose severity differs from KiCad's default (or is forced).
            "default_class": eda_model::rules::net_class_settings_of(&model.board).default,
            "constraints": eda_model::rules::Constraints::of(&model.board),
            "constraints_explicit": model.board.constraints_explicit,
            "mask_paste": model.board.solder_mask,
            "text_graphics": model.board.text_graphics,
            "board_thickness_nm": model.board.board_thickness_nm,
            "copper_layers": model.board.layers.len(),
            "layer_names": model.board.layers,
            // Board Editor Layers (`eda_model::layers`): the non-copper layers the board has in KiCad's order, the names the user gave its layers and the types that are not the default's.
            "layer_setup": model.board.layer_setup.canonical(),
            // The smaller pages (`eda_model::setup_pages`): Formatting, Zone Hatch Offsets (copper layer -> offset), Length-tuning Patterns and Tuning Profiles (with the profile each net class names).
            "formatting": model.board.formatting,
            "zone_hatch_offsets": model.board.hatch_offsets,
            "tuning_patterns": model.board.tuning_patterns,
            "tuning_profiles": model.board.tuning_profiles,
            "severities": eda_kicad::effective_rule_severities(&model),
            "custom_rules_text": model.board.custom_rules_text,
            // Text Variables (`PROJECT::GetTextVars`): the project's variables as `${NAME}` reads them, name -> value -- the page's rows once it has been used, else the
            // board folder's `path-variables.json` (`crate::text_variables`).
            "text_variables": crate::text_variables::project_variables(dir),
            "overlay": design.drawings.as_ref().and_then(|d| d.rules.as_ref()).map(|r| {
                [("net_classes", r.net_classes.is_some()), ("constraints", r.constraints.is_some()), ("mask_paste", r.mask_paste.is_some()), ("text_graphics", r.text_graphics.is_some()), ("stackup", r.stackup.is_some()), ("severities", r.severities.is_some()), ("custom_rules", r.custom_rules.is_some()), ("text_variables", r.text_variables.is_some()), ("board_layers", r.board_layers.is_some()), ("formatting", r.formatting.is_some()), ("zone_hatch_offsets", r.zone_hatch_offsets.is_some()), ("tuning_patterns", r.tuning_patterns.is_some()), ("tuning_profiles", r.tuning_profiles.is_some())]
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
        // The board's plot settings as the Plot and Generate Drill Files dialogs start from them (`DIALOG_PLOT::init_Dialog`,
        // `DIALOG_GENDRILL::TransferDataToWindow`): the layer selection, of the layers the board has, and "Use drill/place file origin".
        "plot_params": plot_params_json(&design, &model),
        // The point the editing grid is anchored at (`BOARD_DESIGN_SETTINGS::GetGridOrigin`), `[x, y]` um, or null at (0, 0).
        "grid_origin": design.drawings.as_ref().and_then(|d| d.grid_origin).map(|p| json!([p.x, p.y])),
        // The board's paper and title block (Page Settings); null is A4 landscape / an empty title block.
        "page": design.drawings.as_ref().and_then(|d| d.page.as_ref()).map(crate::page_json::page_json),
        "title_block": design.drawings.as_ref().and_then(|d| d.title_block.as_ref()).map(|tb| tv.title_block_json(tb)),
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
pub(crate) fn shape_json(s: &Shape) -> Value {
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

/// `BOARD::GetBoardPolygonOutlines`: `[{ "outer": [[x, y], ..], "holes": [[[x, y], ..], ..] }, ..]`, micrometres. A board with no outline is `[]`; one
/// whose Edge.Cuts do not close is KiCad's guess, the rectangle round the edges (`outline_errors` says so).
fn outline_polys_json(design: &eda_model::ir::Design, model: Option<&eda_model::ConstraintModel>) -> Value {
    let outline = eda_drc::outline::board_outline_with(design, model, true);
    let ring = |c: &eda_shape_poly_set::LineChain| c.iter().map(|p| json!([p.x, p.y])).collect::<Vec<_>>();
    Value::Array(
        (0..outline.polys.outline_count())
            .map(|i| json!({ "outer": ring(outline.polys.outline(i)), "holes": (0..outline.polys.hole_count(i)).map(|h| ring(outline.polys.hole(i, h))).collect::<Vec<_>>() }))
            .collect(),
    )
}

/// The board's plot settings as the Plot and Generate Drill Files dialogs start from them (`DIALOG_PLOT::init_Dialog`, `DIALOG_GENDRILL::TransferDataToWindow`):
/// the layer selection, of the layers the board has, and "Use drill/place file origin". With them, what Export IPC-2581 starts from
/// (`DIALOG_EXPORT_2581::TransferDataToWindow`): the BOM columns kept in the board and the root schematic's title block revision
/// (`m_IP2581Bom.schRevision`, the BOM revision when none was kept).
fn plot_params_json(design: &eda_model::ir::Design, model: &eda_model::ConstraintModel) -> Value {
    let plot = eda_model::plot_params::PlotParams::of_design(design);
    let bom = design.drawings.as_ref().and_then(|d| d.ipc2581_bom.clone()).unwrap_or_default();
    let sch_revision = design.schematic.as_ref().and_then(|s| s.title_block.as_ref()).map(|t| t.rev.clone()).unwrap_or_default();
    json!({ "layers": plot.board_layers(&model.board.layers), "use_aux_origin": plot.flag("useauxorigin").unwrap_or(false), "ipc2581_bom": bom, "sch_revision": sch_revision })
}

/// What `kicad-cli pcb drc` reports as `invalid_outline` ("Board has malformed outline") for this board, from the live port of the same check:
/// `[{ "message", "at": [x, y], "items": [id, ..] }, ..]`. Only a board with Edge.Cuts beyond its polygon is checked.
fn outline_errors_json(design: &eda_model::ir::Design, model: Option<&eda_model::ConstraintModel>) -> Value {
    let footprint_edges = model.is_some_and(|m| !eda_drc::outline::footprint_edge_cuts_shapes(design, m).is_empty());
    if !(eda_model::outline::has_edge_cuts_shapes(design) || eda_model::outline::outline_is_shapes(design) || footprint_edges) {
        return json!([]);
    }
    Value::Array(
        eda_drc::outline::check_board_outline_with(design, model)
            .iter()
            .map(|e| {
                let items: Vec<String> = e.item_a.iter().chain(e.item_b.iter()).cloned().collect();
                json!({ "message": e.describe(), "at": [e.at.x, e.at.y], "items": items })
            })
            .collect(),
    )
}

/// Task item 7. `kind`/`units`/`units_format`/`text_position`/
/// `arrow_direction` as lowercase strings, matching every other enum this
/// API already exposes (`justify` above). `height`/`horizontal`/
/// `leader_length` are present only for the kind that actually has them
/// (`null` otherwise) -- `eda_model::ir::DimensionKind`'s own fields.
/// `lines`/`text_at`/`computed_text_angle`/`measured_value_nm`/`text` are
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
        "text_size_nm": d.text_size_nm,
        // `EDA_TEXT::GetTextThickness()` of the label; null = 15 % of the size (the Properties panel's "Thickness").
        "text_thickness_nm": d.text_thickness_nm,
        "stroke_width": d.stroke_width,
        "arrow_length": d.arrow_length,
        "extension_offset": d.extension_offset,
        "extension_height": d.extension_height,
        "arrow_direction": arrow_direction,
        "lines": geom.lines.iter().map(|(a, b)| json!([[a.x, a.y], [b.x, b.y]])).collect::<Vec<_>>(),
        "text_at": [geom.text_at.x, geom.text_at.y],
        "computed_text_angle": geom.text_angle,
        "measured_value_nm": geom.measured_value_nm,
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
        "text_size_nm": s.text_size_nm,
        "stroke_width": s.stroke_width,
        "arrow_length": s.arrow_length,
        "extension_offset": s.extension_offset,
        "extension_height": s.extension_height,
    })
}

/// The design's schematic as KiCad plots it -- `kicad-cli sch export svg` on
/// the derived `.kicad_sch`, the root sheet's page -- or, for a board started
/// from an intent alone, the one derived from the intent
/// (`kicad_engine::load_with_schematic`, as ERC and File > Plot read it).
/// Without kicad-cli the route answers its reason. `EDA_RUST_PICTURES=1`
/// draws the INACTIVE `eda-render` sheet instead ([`schematic_svg_rust`]).
fn schematic_svg(dir: &Path, cache: &Mutex<Option<(std::time::SystemTime, String)>>) -> Result<String, Vec<CheckResult>> {
    if eda::rust_pictures() {
        return schematic_svg_rust(dir, cache);
    }
    let (design, model) = crate::kicad_engine::load_with_schematic(dir)?;
    let work = dir.join(".kicad");
    let pages = eda_kicad_engine::schematic_plot(&design, &model, &work, &work.join("schematic-svg"), &crate::kicad_engine::project_name(dir), eda_kicad_engine::PictureFormat::Svg)?;
    let root = pages.first().ok_or_else(|| vec![CheckResult::fail("kicad_cli_export", "svg", "kicad-cli plotted no page")])?;
    std::fs::read_to_string(root).map_err(|e| vec![CheckResult::fail("kicad_cli_export", "svg", format!("could not read {}: {e}", root.display()))])
}

/// INACTIVE (2026-10-28): replaced by kicad-cli `sch export svg` ([`schematic_svg`]) per the user's port-vs-reuse rule; kept for a future
/// all-Rust build, used only with `EDA_RUST_PICTURES=1`.
///
/// The design's schematic drawn, or, for a board started from an intent
/// alone, one derived from the intent (cached until the intent changes).
fn schematic_svg_rust(dir: &Path, cache: &Mutex<Option<(std::time::SystemTime, String)>>) -> Result<String, Vec<CheckResult>> {
    let (meta, design, model) = board::load(dir)?;
    if design.schematic.is_some() {
        return eda::render_schematic(&design, &model);
    }
    let when = std::fs::metadata(board::intent_path(dir, &meta, &design)).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
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
pub(crate) fn schematic_json(dir: &Path, sheet_path: &str, clock: eda_model::text_vars::Clock) -> Result<Value, Vec<CheckResult>> {
    let (meta, mut design, model) = board::load(dir)?;
    // A board with no stored schematic shows the one derived from its intent: module sheets, whole, so the sheet path resolves.
    if design.schematic.is_none() {
        let derived = board::derived_schematic(&design, &model)?;
        design.schematic = derived.schematic;
        design.sheet_contents = derived.sheet_contents;
    }
    let project = crate::text_vars_json::project(dir, &meta.intent, clock);
    Ok(schematic_json_with(&design, &model, sheet_path, Some((dir, project))))
}

/// [`schematic_json`] of a design that is already loaded (the schematic clipboard asks it of a design a paste has just been tried on).
pub(crate) fn schematic_json_of(design: &eda_model::ir::Design, model: &eda_model::ConstraintModel, sheet_path: &str) -> Value {
    schematic_json_with(design, model, sheet_path, None)
}

/// [`schematic_json_of`] with the board folder and project of the board it is the schematic of, which the `shown` form of each text and field needs
/// (`${NAME}`, `crate::text_vars_json`); without them the texts are sent as stored.
fn schematic_json_with(design: &eda_model::ir::Design, model: &eda_model::ConstraintModel, sheet_path: &str, shown: Option<(&Path, eda_model::text_vars::Project)>) -> Value {
    let (sch, breadcrumb) = resolve_sheet(design, sheet_path);
    // the sheet's path from the root (`SCH_SHEET_PATH`), which a screen shown twice files each placement's references under
    let sheet_ids: Vec<String> = breadcrumb.iter().map(|(id, _)| id.clone()).collect();
    // The net each pin is on ("REF.PIN" -> net), for the pin highlight and the pin's description.
    let pin_net: std::collections::HashMap<&str, &str> = model.nets.iter().flat_map(|n| n.pins.iter().map(move |p| (p.as_str(), n.name.as_str()))).collect();
    let shown = shown.map(|(dir, project)| crate::text_vars_json::SchShown::new(dir, design, model, &breadcrumb, project));
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
            let resolved = part.and_then(|p| model.real_symbol_of_instance(&sch, s, p));
            // A screen two sheets show: the reference this placement gives the symbol (`SCH_SYMBOL::GetRef( &GetCurrentSheet() )`), which its Reference
            // field shows and its pins' nets are filed under; the symbol's `id` stays what the verbs name it by.
            let reference = eda_model::sheet_list::reference_under(&sch, s, &sheet_ids);
            let pins: Vec<Value> = part
                .map(|p| {
                    p.pins
                        .iter()
                        .filter(|pin| resolved.as_ref().and_then(|r| r.pin_by_number(&pin.number)).is_none_or(|rp| rp.unit == 0 || rp.unit == s.unit))
                        .map(|pin| json!({ "number": pin.number, "name": pin.name, "kind": pin.kind, "net": pin_net.get(format!("{reference}.{}", pin.number).as_str()) }))
                        .collect()
                })
                .unwrap_or_default();
            // Where each field is drawn on the sheet (`eda_engine::fields`): the placements the section keeps, else Autoplace Fields'. Not for a
            // schematic read from a KiCad file, whose symbols are placed by their own origin rather than the engine's box corner.
            let owner = eda_model::ir::field_key(&s.id, s.unit);
            let mut fields: Vec<Value> = owner_fields_json(&sch, model, &owner);
            if let Some(sh) = &shown {
                sh.annotate_symbol_fields(&owner, &mut fields);
            }
            if reference != s.id {
                for f in fields.iter_mut().filter(|f| f["name"] == "Reference") {
                    f["text"] = json!(reference);
                    if f.get("shown").is_some() {
                        f["shown"] = json!(reference);
                    }
                }
            }
            let mut symbol = json!({
                "id": s.id,
                "reference": reference,
                "fields": fields,
                "fields_autoplaced": fields_autoplaced_json(&sch, &owner),
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
                // Which body style the symbol is drawn in (`SCH_SYMBOL::GetBodyStyle`: 1 the normal one, 2 the alternate "De Morgan" one) -- the
                // same filter `resolveLibSymbol` applies to the `body_style` of each item of `lib_symbols[lib_id]`.
                "body_style": sch.body_style_of(s),
                "value": if s.value.is_empty() { part.and_then(|p| p.value.clone()) } else { Some(s.value.clone()) },
                "footprint": if s.footprint.is_empty() { None } else { Some(s.footprint.clone()) },
                "datasheet": if s.datasheet.is_empty() { None } else { Some(s.datasheet.clone()) },
                "mpn": part.and_then(|p| p.mpn.clone()),
                "package": part.and_then(|p| p.package.clone()),
                "pins": pins,
                // The places Swap Pins traded (`SchExtras::pin_swaps`): pin number -> the number of the pin whose place it has. The canvas draws the pin there.
                "pin_swaps": sch.extras.pin_swaps_of(&owner),
                // Do not Populate / Exclude from BOM / Board / Simulation (`setDNP` ...): the flags the painter marks and the attribute actions toggle.
                "dnp": s.dnp,
                "exclude_from_bom": s.exclude_from_bom,
                "exclude_from_board": s.exclude_from_board,
                "exclude_from_sim": s.exclude_from_sim,
            });
            // The Value and Footprint the painter writes for a symbol that has no `fields` (a sheet read from a KiCad file), as `${NAME}` makes them read.
            if let Some(shown) = shown.as_ref().and_then(|sh| sh.symbol_shown_json(&owner)) {
                symbol["shown"] = shown;
            }
            symbol
        })
        .collect();
    // the texts of every global label of the schematic, for a wire's label driver
    let global_texts: std::collections::BTreeSet<&str> = design
        .schematic
        .iter()
        .chain(design.sheet_contents.iter().flat_map(|c| c.values()))
        .flat_map(|s| s.labels.iter().filter(|l| matches!(l.kind, eda_model::ir::LabelKind::Global { .. })).map(|l| l.net.as_str()))
        .collect();
    let wires: Vec<Value> = sch
        .wires
        .iter()
        .map(|w| {
            let mut v = json!({ "id": w.id, "net": w.net, "pins": w.pins, "pts": w.pts.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(), "bus": w.bus });
            if let Some(name) = wire_label_driver(&sch, w, &global_texts) {
                v["label_driver"] = json!(name);
            }
            // The stroke Wire/Bus Properties set (width, style, colour), when one was.
            if let Some(stroke) = sch.extras.strokes.get(&w.id) {
                v["stroke"] = json!(stroke);
            }
            // A bus's member nets (`BUS_UNFOLD_MENU` lists them): the vector/group/alias name expansion (`eda_kicad::expand_bus_members`).
            // A bus named with its sheet's path (KiCad's `/LED channels/PB[0..1]`) is expanded from the label's text, and its members are their local names, as
            // the unfold menu lists them (`member->FullLocalName()`, `SCH_LINE_WIRE_BUS_TOOL`'s `BUS_UNFOLD_MENU`).
            if w.bus {
                let text = if w.net.starts_with('/') { w.net.rsplit('/').next().unwrap_or(&w.net) } else { w.net.as_str() };
                v["members"] = json!(eda_kicad::expand_bus_members(text, &design.bus_aliases).unwrap_or_default());
            }
            v
        })
        .collect();
    // Explicit junctions (`J`) and graphic lines on the notes layer (`I`) -- see `eda_model::ir::Junction`/`SchLine`.
    let junctions: Vec<Value> = sch.junctions.iter().map(|j| json!({ "id": j.id, "at": [j.at.x, j.at.y], "look": sch.extras.junction_looks.get(&j.id) })).collect();
    let lines: Vec<Value> = sch.lines.iter().map(|l| json!({ "id": l.id, "pts": l.pts.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(), "width_nm": l.width_nm, "stroke": sch.extras.strokes.get(&l.id) })).collect();
    // Drawn shapes, text boxes, rule areas, directive labels and images (`SchGraphic`, serialized as stored; an image is described, its pixels are `GET /api/sch_image`) and the ids of locked items.
    let graphics: Value = crate::image_api::sch_graphics_json(&sch.extras.graphics);
    let locked: Vec<&String> = sch.extras.locked.iter().collect();
    // The groups of the sheet (`SCH_GROUP`): `{id, name, member_ids}`, a member being an item's id or another group's.
    let groups: Value = serde_json::to_value(&sch.extras.groups).unwrap_or(Value::Null);
    // The sheet's tables (`SCH_TABLE`): every cell's corners and placed text lines, worked out once here (`crate::table_json`); a cell's own variables (`${ROW}`, `${CELL(..)}`) and then the sheet's.
    let tables: Vec<Value> = sch
        .extras
        .tables
        .iter()
        .map(|t| match &shown {
            Some(sh) => crate::table_json::sch_table_json(t, &|tok: &str| sh.resolve(tok)),
            None => crate::table_json::sch_table_json(t, &|_: &str| None),
        })
        .collect();
    // GAPS.md #20: bus entries, for the bus/entry tool and for drawing the
    // diagonal stub on canvas.
    let bus_entries: Vec<Value> = sch.bus_entries.iter().map(|be| json!({ "id": be.id, "at": [be.at.x, be.at.y], "size": [be.size.x, be.size.y], "stroke": sch.extras.strokes.get(&be.id) })).collect();
    // A global label's Sheet References (`SCH_GLOBALLABEL::ResolveTextVar( "INTERSHEET_REFS" )`): the pages its name is on, as Schematic Setup says
    let iref = eda_kicad::design_intersheet_refs(design);
    let entries = eda_model::sheet_list::walk(design);
    let label_pages = eda_model::sheet_list::global_label_pages(design);
    let here: Vec<String> = breadcrumb.iter().map(|(id, _)| id.clone()).collect();
    let current = entries.iter().find(|e| e.path == here).map_or(1, |e| e.virtual_page);
    let page_text = |p: usize| entries.iter().find(|e| e.virtual_page == p).map(|e| e.effective_page()).unwrap_or_else(|| p.to_string());
    let labels: Vec<Value> = sch
        .labels
        .iter()
        .map(|l| {
            let (scope, shape) = match &l.kind {
                eda_model::ir::LabelKind::Local => ("local", None),
                eda_model::ir::LabelKind::Global { shape } => ("global", Some(*shape)),
                eda_model::ir::LabelKind::Hierarchical { shape } => ("hierarchical", Some(*shape)),
            };
            // The look Label Properties set (size, bold, italic); the defaults when it never did.
            let look = sch.extras.label_looks.get(&l.id).copied().unwrap_or_default();
            // Its own fields and a global label's Sheet References, where `AutoplaceFields` puts them (`eda_kicad::label_fields`)
            let fields: Vec<Value> = eda_kicad::label_fields::label_fields_placed(&sch, l, iref.show)
                .into_iter()
                .map(|f| {
                    let shown = if f.intersheet_refs { iref.shown(label_pages.get(&l.net), current, &page_text) } else { f.text.clone() };
                    let h = match f.h {
                        eda_model::kicad_font::HJustify::Left => "left",
                        eda_model::kicad_font::HJustify::Right => "right",
                        eda_model::kicad_font::HJustify::Center => "center",
                    };
                    json!({ "name": f.name, "text": f.text, "shown": shown, "visible": f.visible, "italic": f.italic, "intersheet_refs": f.intersheet_refs, "at": [f.at.x, f.at.y], "vertical": f.vertical, "h": h, "size_nm": 1_270_000 })
                })
                .collect();
            // the net the label is on, as the tracer named it (the wire under it): Highlight Net takes a label's connection, `conn->Name()`, not its text (r3 A3-M4)
            let traced = sch.wires.iter().find(|w| !w.net.is_empty() && w.pts.windows(2).any(|s| point_on_segment(s[0], s[1], l.at))).map(|w| w.net.clone()).or_else(|| {
                // a label put on a pin with no wire: the net of the pin it is on, a subgraph member like any other (r4 C4-M2)
                sch.symbols.iter().find_map(|sym| {
                    let part = model.part(&sym.id)?;
                    let resolved = model.real_symbol_of_instance(&sch, sym, part);
                    let (number, _) = eda_engine::placed::pin_points_in(&sch, sym, part, resolved.as_ref()).into_iter().find(|(_, at)| *at == l.at)?;
                    let reference = eda_model::sheet_list::reference_under(&sch, sym, &sheet_ids);
                    pin_net.get(format!("{reference}.{number}").as_str()).map(|n| n.to_string())
                })
            });
            json!({ "id": l.id, "net": l.net, "net_name": traced, "at": [l.at.x, l.at.y], "scope": scope, "shape": shape.map(label_shape_str), "spin": sch.extras.label_spins.get(&l.id), "size_nm": look.text_size_nm(), "bold": look.bold, "italic": look.italic, "auto_rotate": look.auto_rotate, "fields": fields })
        })
        .collect();
    let texts: Vec<Value> = sch
        .texts
        .iter()
        .map(|t| {
            let mut v = text_json(&sch, t);
            if let Some(sh) = &shown {
                let text = sh.text_shown(&t.content);
                if text != t.content {
                    v["shown"] = json!(text);
                }
            }
            v
        })
        .collect();
    let power_symbols: Vec<Value> = sch
        .power_symbols
        .iter()
        .map(|p| json!({ "id": p.id, "lib_id": p.lib_id, "at": [p.at.x, p.at.y], "rot": p.rot as f64 / 1000.0, "net": p.net, "pin": p.pin, "fields": eda_engine::fields::power_fields(&sch, p).iter().map(|f| page_field_json(&p.id, f)).collect::<Vec<_>>(), "fields_autoplaced": fields_autoplaced_json(&sch, &p.id) }))
        .collect();
    let no_connects: Vec<Value> = sch.no_connects.iter().map(|nc| json!({ "id": nc.id, "at": [nc.at.x, nc.at.y], "pin": nc.pin })).collect();
    let title_block = sch.title_block.as_ref().map(|tb| match &shown {
        Some(sh) => sh.title_block_json(tb),
        None => crate::page_json::title_block_json(tb),
    });

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
    // a sheet's page is the one it shows on this sheet path (`SCH_SHEET::getPageNumber( currentSheetPath )`, Sheet Properties' Page number):
    // stored for the path, else its place in the walk, as the derived file has it
    let here: Vec<String> = breadcrumb.iter().map(|(id, _)| id.clone()).collect();
    let walk = if sch.sheets.is_empty() { Vec::new() } else { eda_model::sheet_list::walk(design) };
    let shown_page = |s: &eda_model::ir::SheetInstance| -> String {
        walk.iter()
            .find(|e| e.path.len() == here.len() + 1 && e.path.starts_with(&here) && e.path.last() == Some(&s.id))
            .map_or_else(|| eda_model::sheet_list::page_on(&sch, s, &here).to_string(), eda_model::sheet_list::SheetEntry::effective_page)
    };
    // the screen in view: a sheet's Sheetfile is its file as this screen names it (r3 B3-M1)
    let placing_key = viewed_file(design, &breadcrumb);
    let sheets: Vec<Value> = sch
        .sheets
        .iter()
        .map(|s| {
            let mut fields = eda_engine::fields::sheet_fields_on(&sch, s, &placing_key).iter().map(|f| page_field_json(&s.id, f)).collect::<Vec<_>>();
            if let Some(sh) = &shown {
                sh.annotate_sheet_fields(&mut fields);
            }
            json!({
                "id": s.id, "name": s.name, "file": s.file, "page": shown_page(s),
                "fields": fields,
                "fields_autoplaced": fields_autoplaced_json(&sch, &s.id),
                "at": [s.at.x, s.at.y], "size": [s.size.0, s.size.1],
                // a pin's text is set as Sheet Pin Properties set it (`SchExtras::sheet_pin_looks`): the size, bold and italic are sent when they are not the default's
                "pins": s.pins.iter().map(|p| {
                    let mut pin = json!({ "id": p.id, "name": p.name, "shape": label_shape_str(p.shape), "at": [p.at.x, p.at.y] });
                    if let Some(look) = sch.extras.sheet_pin_looks.get(&p.id) {
                        if look.size_nm > Nm::ZERO {
                            pin["size_nm"] = json!(look.size_nm);
                        }
                        if look.bold {
                            pin["bold"] = json!(true);
                        }
                        if look.italic {
                            pin["italic"] = json!(true);
                        }
                    }
                    pin
                }).collect::<Vec<_>>(),
            })
        })
        .collect();
    let sheet_path: Vec<Value> = breadcrumb.iter().map(|(id, name)| json!({ "id": id, "name": name })).collect();
    // The paper this sheet is drawn on: its full Page Settings when it has them (portrait, a user size), else the name its title block carries
    // (the layout engine writes it per sheet); A4 landscape when neither.
    let page = eda_model::page::PageSettings::of_sheet(sch.extras.page.as_ref(), sch.title_block.as_ref().map(|t| t.paper.as_str()).unwrap_or(""));
    let (paper_w, paper_h) = page.size_nm().unwrap_or((mm(297.0), mm(210.0)));
    let file = viewed_file(design, &breadcrumb);
    // the page of the sheet in view as a number (`GetPageNumberAsInt`), which a placed symbol's "sheet number x 100" numbering counts from (r3 D3-M3)
    let page_number = eda_model::sheet_list::walk(design).iter().find(|e| e.path == sheet_ids).map_or(1, eda_model::sheet_list::SheetEntry::page_number_as_int);

    json!({
        "paper": { "name": page.paper, "width_nm": paper_w, "height_nm": paper_h },
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
        "tables": tables,
        "locked": locked,
        "groups": groups,
        "title_block": title_block,
        "page": crate::page_json::page_json(&page),
        "lib_symbols": lib_symbols,
        "sheets": sheets,
        "sheet_path": sheet_path,
        "page_number": page_number,
    })
}

/// The file of the screen the breadcrumb ends on (`""` for the root): the title block of the sheet in view names it. The sheets
/// of the whole design are `GET /api/sch/hierarchy`.
pub(crate) fn viewed_file(design: &eda_model::ir::Design, breadcrumb: &[(String, String)]) -> String {
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
    let libs = crate::library_tables::libs(dir, crate::library_index::Kind::Symbol);

    let lib_name_of = |lib_id: &str| lib_id.split_once(':').map(|(l, _)| l.to_string());
    let mut lib_names: std::collections::BTreeSet<String> = model.symbols.iter().filter_map(|s| lib_name_of(&s.lib_id)).collect();
    // every sheet's symbols: with module sheets the root holds none of its own
    for sch in design.schematic.iter().chain(design.sheet_contents.iter().flat_map(|c| c.values())) {
        lib_names.extend(sch.symbols.iter().filter_map(|s| lib_name_of(&s.lib_id)));
    }

    let mut by_lib_id: std::collections::BTreeMap<String, eda_model::LibSymbol> = std::collections::BTreeMap::new();
    for name in &lib_names {
        // the library of that nickname in the board's symbol library tables (KiCad's, or the board's own)
        let Ok(path) = crate::library_index::LibSource::lib_path(&*libs, crate::library_index::Kind::Symbol, name) else { continue };
        for sym in eda_kicad::list_symbols_in_file(&path, name) {
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
    let empty = || eda_model::LibSymbol { connectivity: Default::default(),
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
        alternate: None,
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
        .map(|(p, at, angle_deg)| eda_model::LibPin { number: p.number.clone(), name: p.name.clone().unwrap_or_default(), electrical_type: etype(p.kind).to_string(), shape: "line".to_string(), at, angle_deg, length_mm: 1.27, unit: 1, hidden: false })
        .collect();
    eda_model::LibSymbol { connectivity: Default::default(),
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
        alternate: None,
    }
}

/// A free-standing schematic text as the studio draws it: its angle (0 or 90 once it has been through KiCad's rules, whatever an older design says), whether it runs up the
/// sheet, and how it is justified about its anchor (`SchExtras::text_justify`: left and bottom unless something set it).
pub(crate) fn text_json(sch: &eda_model::ir::SchematicSection, t: &eda_model::ir::SchematicText) -> Value {
    use eda_model::ir::{TextJustify, TextVAlign};
    let j = sch.extras.text_justify_of(&t.id);
    let vertical = eda_model::kicad_geom::keep_upright_is_vertical(t.angle as f64 / 1000.0);
    json!({
        "id": t.id, "content": t.content, "at": [t.at.x, t.at.y], "angle": t.angle as f64 / 1000.0, "size_nm": t.size_nm, "vertical": vertical,
        "h": match j.h { TextJustify::Left => "left", TextJustify::Center => "center", TextJustify::Right => "right" },
        "v": match j.v { TextVAlign::Top => "top", TextVAlign::Center => "center", TextVAlign::Bottom => "bottom" },
    })
}

/// A field on the sheet as the painter reads it: the text, its anchor in micrometres, whether it runs vertically, how it is justified
/// against the anchor (in the text's own axes) and whether it is drawn.
/// One field as the studio draws and selects it: the text, its anchor on the sheet and which way it runs, how it is justified there, its size and look, and the
/// id (`fld:<owner key>:<name>`) every move and edit verb takes it by. `owner` is the item's key in `field_layout` (a symbol's reference, `#<unit>` after it for a
/// unit but the first, a power symbol's or a sheet's id).
pub(crate) fn page_field_json(owner: &str, f: &eda_engine::fields::PageField) -> Value {
    use eda_model::kicad_font::{HJustify, VJustify};
    json!({
        "id": eda_engine::fields_edit::field_id(owner, &f.name),
        "owner": owner,
        "name": f.name,
        "text": f.text,
        "at": [f.at.0.round(), f.at.1.round()],
        "vertical": f.vertical,
        "h": match f.h { HJustify::Left => "left", HJustify::Center => "center", HJustify::Right => "right" },
        "v": match f.v { VJustify::Top => "top", VJustify::Center => "center", VJustify::Bottom => "bottom" },
        "visible": f.visible,
        "size_nm": f.size_nm,
        "bold": f.bold,
        "italic": f.italic,
        "name_shown": f.name_shown,
        "allow_autoplace": !f.no_autoplace,
    })
}

/// The fields of the symbol, power symbol or sheet whose key in `field_layout` is `owner`, where they are on the sheet; none for an item the editor cannot place
/// fields of (a part the model does not know, a schematic read from a KiCad file).
pub(crate) fn owner_fields_json(sch: &eda_model::ir::SchematicSection, model: &eda_model::ConstraintModel, owner: &str) -> Vec<Value> {
    match eda_engine::fields_edit::OwnerCtx::new(sch, model, owner) {
        Some(ctx) => ctx.page_fields(sch).iter().map(|f| page_field_json(owner, f)).collect(),
        None => Vec::new(),
    }
}

/// `"auto"`, `"manual"` or null: whether Autoplace Fields put the fields of the item `key` where they are (`SCH_ITEM::GetFieldsAutoplaced`).
pub(crate) fn fields_autoplaced_json(sch: &eda_model::ir::SchematicSection, key: &str) -> Value {
    json!(sch.extras.fields_autoplaced.get(key))
}

/// One `LibSymbol` as JSON: graphics/pins in the symbol's own local frame,
/// millimetres, +y **up** (KiCad's own library convention, not this API's
/// usual +y-down sheet millimetres) -- drawing it in sheet space needs the
/// same negate-y-then-rotate-then-mirror composition
/// `eda_kicad::lib::baked_local`'s doc comment spells out.
///
/// Each graphic and pin carries two spellings: the engine symbol's own (`stroke_mm`, `filled`, `radius_mm`, `text`) and the names the studio's painter and its
/// `LibSymbol` type read (`stroke_width`, `fill`, `radius`, `content`, `body_style`, and a pin's `hidden`). Without the second a symbol from a library was
/// drawn with no body and no pins: the painter keeps an item only when its `body_style` is 0 or the placed one's. A symbol with one body style has every item
/// shared (`body_style` 0); one with an alternate ("De Morgan") body style has the items both bodies draw alike shared and the others of body style 1 (the
/// normal body) or 2 (the alternate one), which is what `resolveLibSymbol` picks from by the style a placed symbol is in. No pin is hidden, and the engine symbol
/// keeps only whether a shape is filled, not how: a filled rectangle is the body colour (what library ICs use), any other filled shape the outline colour.
fn lib_symbol_json(s: &eda_model::LibSymbol) -> Value {
    let pt = |p: eda_model::symbol::SPoint| json!([p.x, p.y]);
    let fill = |filled: bool, how: &str| if filled { how.to_string() } else { "none".to_string() };
    let graphic = |g: &eda_model::SymbolGraphic, style: u32| -> Value {
        use eda_model::SymbolGraphic::*;
        match g {
            Rectangle { unit, start, end, stroke_mm, filled } => json!({ "kind": "rectangle", "unit": unit, "body_style": style, "start": pt(*start), "end": pt(*end), "stroke_mm": stroke_mm, "stroke_width": stroke_mm, "filled": filled, "fill": fill(*filled, "background") }),
            Polyline { unit, pts, stroke_mm, filled } => json!({ "kind": "polyline", "unit": unit, "body_style": style, "pts": pts.iter().map(|p| pt(*p)).collect::<Vec<_>>(), "stroke_mm": stroke_mm, "stroke_width": stroke_mm, "filled": filled, "fill": fill(*filled, "outline") }),
            Circle { unit, center, radius_mm, stroke_mm, filled } => json!({ "kind": "circle", "unit": unit, "body_style": style, "center": pt(*center), "radius_mm": radius_mm, "radius": radius_mm, "stroke_mm": stroke_mm, "stroke_width": stroke_mm, "filled": filled, "fill": fill(*filled, "outline") }),
            Arc { unit, start, mid, end, stroke_mm, filled } => json!({ "kind": "arc", "unit": unit, "body_style": style, "start": pt(*start), "mid": pt(*mid), "end": pt(*end), "stroke_mm": stroke_mm, "stroke_width": stroke_mm, "filled": filled, "fill": fill(*filled, "outline") }),
            Text { unit, text, at, angle_deg, size_mm } => json!({ "kind": "text", "unit": unit, "body_style": style, "text": text, "content": text, "at": pt(*at), "angle_deg": angle_deg, "size_mm": size_mm }),
        }
    };
    let pin = |p: &eda_model::LibPin, style: u32| json!({ "number": p.number, "name": eda_model::kicad_geom::shown_name(&p.name), "electrical_type": p.electrical_type, "shape": p.shape, "at": pt(p.at), "angle_deg": p.angle_deg, "length_mm": p.length_mm, "unit": p.unit, "body_style": style, "hidden": false });
    let (normal, alt) = s.bodies();
    let mut graphics: Vec<Value> = normal.0.iter().map(|g| graphic(g, if alt.is_none_or(|a| a.0.contains(g)) { 0 } else { 1 })).collect();
    let mut pins: Vec<Value> = normal.1.iter().map(|p| pin(p, if alt.is_none_or(|a| a.1.contains(p)) { 0 } else { 1 })).collect();
    if let Some(a) = alt {
        graphics.extend(a.0.iter().filter(|g| !normal.0.contains(g)).map(|g| graphic(g, 2)));
        pins.extend(a.1.iter().filter(|p| !normal.1.contains(p)).map(|p| pin(p, 2)));
    }
    // how the symbol's pins show their texts (`(pin_names (hide yes) (offset x))`, `(pin_numbers (hide yes))`)
    json!({ "power": s.power, "graphics": graphics, "pins": pins, "datasheet": s.datasheet, "description": s.description, "pin_names_hidden": s.pin_names_hidden, "pin_numbers_hidden": s.pin_numbers_hidden, "pin_name_offset": s.pin_name_offset_mm, "body_style_count": s.body_style_count() })
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
    let fills = crate::fill_api::cached(dir);
    eda_kicad::export_kicad_pcb_with(&design, &model, &eda_kicad::ExportMeta { date: &crate::kicad_engine::chrono_like_today(), title: "board" }, fills.as_ref())
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
pub(crate) fn symbol_editor_names_json(dir: &Path) -> Result<Value, Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    let mut names: std::collections::BTreeSet<String> = model.symbols.iter().map(|s| s.lib_id.clone()).collect();
    names.extend(eda_model::symbol::builtin_catalog().into_iter().map(|s| s.lib_id));
    // `project`: the entries of the project library itself -- see `footprint_library_json`.
    let mut project: Vec<String> = Vec::new();
    // `derived`: each derived symbol of the project library with its parent -- the library tree offers Flatten and the related fields table for them
    let mut derived = serde_json::Map::new();
    if let Some(lib) = &design.symbol_library {
        names.extend(lib.symbols.iter().map(|s| s.lib_id.clone()));
        project = lib.symbols.iter().map(|s| s.lib_id.clone()).collect();
        project.sort();
        for s in lib.symbols.iter().filter(|s| s.extends.is_some()) {
            derived.insert(s.lib_id.clone(), json!(s.extends));
        }
    }
    Ok(json!({ "names": names.into_iter().collect::<Vec<_>>(), "project": project, "derived": derived }))
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
    Ok(eda_kicad::export_kicad_sym_library_with(&syms, &lib.embedded))
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
    // Live connectivity through the pours reads the fills KiCad made (a zone with none connects nothing), `crate::fill_api`.
    let copper = crate::fill_api::cached(dir).map(|k| k.copper(&design)).unwrap_or_default();
    let report = eda_connectivity::analyze(&design, &model, &copper);
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

/// `p` on the segment `a`-`b`, its ends included (`SCH_LINE::HitTest( p, 0 )`).
fn point_on_segment(a: eda_model::ir::Point, b: eda_model::ir::Point, p: eda_model::ir::Point) -> bool {
    let (ax, ay, bx, by, px, py) = (a.x.get() as i128, a.y.get() as i128, b.x.get() as i128, b.y.get() as i128, p.x.get() as i128, p.y.get() as i128);
    (bx - ax) * (py - ay) == (by - ay) * (px - ax) && ax.min(bx) <= px && px <= ax.max(bx) && ay.min(by) <= py && py <= ay.max(by)
}

/// The name a local or global label put down on `w` takes (`SCH_DRAWING_TOOLS::createNewLabel`: `if( SCH_LINE* wire = findWire( aPosition ) ) netName =
/// findWireLabelDriverName( wire );`, r2 C-L5): the net's name without its sheet's path when the net is named by a label -- a local label of that text on
/// this sheet (the net is `/<path>/<text>`), or a global label of the net's name anywhere -- else none (a power symbol, a hierarchical label, a pin, nothing).
fn wire_label_driver(sch: &eda_model::ir::SchematicSection, w: &eda_model::ir::Wire, global_texts: &std::collections::BTreeSet<&str>) -> Option<String> {
    if w.bus || w.net.is_empty() {
        return None;
    }
    if w.net.starts_with('/') {
        let local = w.net.rsplit('/').next().unwrap_or(&w.net);
        let unescaped = local.replace("{slash}", "/");
        return sch.labels.iter().any(|l| l.kind.is_local() && (l.net == local || l.net == unescaped)).then(|| unescaped);
    }
    global_texts.contains(w.net.as_str()).then(|| w.net.clone())
}

#[cfg(test)]
mod tests {
    use eda_model::units::{mm, Nm};
    use super::*;
    use eda_model::ir::{Point, Provenance, SchematicSection, SheetInstance};

    fn sch(sheets: Vec<SheetInstance>) -> SchematicSection {
        SchematicSection { symbols: vec![], wires: vec![], labels: vec![], texts: vec![], power_symbols: vec![], no_connects: vec![], bus_entries: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), field_layout: Default::default(), title_block: None, sheets, instance_overrides: vec![], junctions: vec![], lines: vec![], extras: Default::default(), imported_from_kicad: false }
    }

    fn design(root: SchematicSection, screens: std::collections::BTreeMap<String, SchematicSection>) -> eda_model::ir::Design {
        eda_model::ir::Design {
            schema: eda_model::ir::SCHEMA_VERSION,
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

    /// The fills kicad-cli made (Fill All Zones) are part of what the board shows: the version changes when they are kept or forgotten, so the 3D view is rebuilt from them, a
    /// DRC run before them is out of date and the page reads them again.
    #[test]
    fn the_version_changes_when_the_fills_are_kept_or_forgotten() {
        let dir = std::env::temp_dir().join(format!("eda_studio_version_fills_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("design.json"), "{}").unwrap();
        let job: Job = Arc::new(Mutex::new(String::new()));
        let before = version_string(&dir, &job);
        assert_eq!(version_string(&dir, &job), before, "nothing changed");
        let fills = eda_kicad::zone_fills::KicadFills::new("kicad-cli".into(), String::new(), 1, Default::default());
        eda_kicad::zone_fills::store(&dir, &fills).unwrap();
        let kept = version_string(&dir, &job);
        assert_ne!(kept, before, "the fills were kept");
        eda_kicad::zone_fills::forget(&dir);
        assert_ne!(version_string(&dir, &job), kept, "and forgotten");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The 3D export draws copper on a net the netlist no longer names (a net renamed after the board was routed) instead of refusing the board.
    #[test]
    fn the_3d_export_adds_the_nets_the_copper_uses() {
        let mut d = design(sch(vec![]), Default::default());
        d.routing = Some(
            serde_json::from_value(json!({
                "tracks": [{ "net": "GND", "layer": "F.Cu", "width": 200_000, "pts": [{ "x": 0, "y": 0 }, { "x": 1_000_000, "y": 0 }] }, { "net": "", "layer": "F.Cu", "width": 200_000, "pts": [{ "x": 0, "y": 0 }, { "x": 0, "y": 500_000 }] }],
                "vias": [{ "net": "VDD", "at": { "x": 0, "y": 0 }, "diameter": 600_000, "drill": 300_000, "from_layer": "F.Cu", "to_layer": "B.Cu" }],
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
        let gnd = lib_symbol_json(&eda_model::LibSymbol { connectivity: Default::default(),
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
            alternate: None,
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
        let placement = SheetInstance { id: "s1".into(), name: "child".into(), file: "child.kicad_sch".into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, size: (mm(1.0), mm(1.0)), pins: vec![], page: String::new() };
        let child = sch(vec![]);
        let mut screens = std::collections::BTreeMap::new();
        screens.insert("child.kicad_sch".to_string(), child);
        let d = design(sch(vec![placement]), screens);

        let (resolved, breadcrumb) = resolve_sheet(&d, "s1");
        assert_eq!(resolved.sheets.len(), 0, "the child has no sheets of its own");
        assert_eq!(breadcrumb, vec![("s1".to_string(), "child".to_string())]);
    }

    /// The outline as the canvas and the 3D view read it from `/api/state`: every outline with its cutouts, and what is wrong with it.
    #[test]
    fn the_state_carries_the_outline_with_its_cutouts_and_its_errors() {
        use eda_model::ir::{DrawingsSection, PlacementSection, Shape};
        let p = |x, y| Point { x, y };
        let line = |a: Point, b: Point| Shape::Segment { id: String::new(), layer: "Edge.Cuts".into(), stroke_width: mm(0.05), filled: false, start: a, end: b };
        let mut d = design(sch(vec![]), Default::default());
        d.placement = Some(PlacementSection { outline: vec![p(Nm::ZERO, Nm::ZERO), p(mm(40.0), Nm::ZERO), p(mm(40.0), mm(30.0)), p(Nm::ZERO, mm(30.0))], footprints: vec![], modules: vec![] });

        // A plain polygon: one outline, no hole, nothing to report.
        assert_eq!(outline_polys_json(&d, None), json!([{ "outer": [[0, 0], [40_000_000, 0], [40_000_000, 30_000_000], [0, 30_000_000]], "holes": [] }]));
        assert_eq!(outline_errors_json(&d, None), json!([]));

        // A round hole drawn on Edge.Cuts is a hole of it.
        let mut shapes = vec![Shape::Circle { id: "hole".into(), layer: "Edge.Cuts".into(), stroke_width: mm(0.05), filled: false, center: p(mm(20.0), mm(15.0)), end: p(mm(23.0), mm(15.0)) }];
        d.drawings = Some(DrawingsSection { shapes: shapes.clone(), ..Default::default() });
        let polys = outline_polys_json(&d, None);
        assert_eq!(polys.as_array().map(Vec::len), Some(1));
        assert_eq!(polys[0]["holes"].as_array().map(Vec::len), Some(1), "{polys}");
        assert_eq!(outline_errors_json(&d, None), json!([]));

        // The same board with the outline made of shapes, one side of it missing: malformed, and the outline is the rectangle round the edges.
        shapes = vec![line(p(Nm::ZERO, Nm::ZERO), p(mm(18.0), Nm::ZERO)), line(p(mm(22.0), Nm::ZERO), p(mm(40.0), Nm::ZERO)), line(p(mm(40.0), Nm::ZERO), p(mm(40.0), mm(30.0))), line(p(mm(40.0), mm(30.0)), p(Nm::ZERO, mm(30.0))), line(p(Nm::ZERO, mm(30.0)), p(Nm::ZERO, Nm::ZERO))];
        for (i, sh) in shapes.iter_mut().enumerate() {
            sh.set_id(format!("edge{i}"));
        }
        d.drawings = Some(DrawingsSection { shapes, outline_is_shapes: true, ..Default::default() });
        let errors = outline_errors_json(&d, None);
        assert_eq!(errors.as_array().map(Vec::len), Some(1), "{errors}");
        assert_eq!(errors[0]["message"], "Board has malformed outline (not a closed shape)");
        assert_eq!(errors[0]["at"], json!([20_000_000, 0]));
        let mut items: Vec<String> = errors[0]["items"].as_array().expect("items").iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
        items.sort();
        assert_eq!(items, vec!["edge0".to_string(), "edge1".to_string()], "the two pieces either side of the gap");
        let polys = outline_polys_json(&d, None);
        assert_eq!((polys.as_array().map(Vec::len), polys[0]["holes"].as_array().map(Vec::len)), (Some(1), Some(0)), "{polys}");
    }

    #[test]
    fn an_unknown_sheet_id_falls_back_to_the_last_good_level_instead_of_erroring() {
        let d = design(sch(vec![]), std::collections::BTreeMap::new());
        let (resolved, breadcrumb) = resolve_sheet(&d, "nonexistent");
        assert_eq!(resolved.sheets.len(), 0, "falls back to the root, not a panic/error");
        assert!(breadcrumb.is_empty());
    }

    /// r2 C-L5 (`createNewLabel`'s `findWireLabelDriverName`): a wire a local label names gives that label's text (without the sheet's path), one a global
    /// label names gives the global name, one only a power symbol or a hierarchical label names gives none.
    #[test]
    fn a_wire_tells_the_label_tool_the_label_that_names_it() {
        use eda_model::ir::{LabelKind, LabelShape, NetLabel, Point, SchematicSection, Wire};
        let wire = |net: &str| Wire { id: String::new(), net: net.into(), pins: vec![], pts: vec![Point::default(), Point::default()], bus: false };
        let label = |net: &str, kind: LabelKind| NetLabel { id: String::new(), net: net.into(), at: Point::default(), kind };
        let sch = SchematicSection { labels: vec![label("SDA", LabelKind::Local), label("PORT", LabelKind::Hierarchical { shape: LabelShape::Input })], ..Default::default() };
        let globals: std::collections::BTreeSet<&str> = ["CLK"].into_iter().collect();
        assert_eq!(wire_label_driver(&sch, &wire("/Sub/SDA"), &globals).as_deref(), Some("SDA"));
        assert_eq!(wire_label_driver(&sch, &wire("CLK"), &globals).as_deref(), Some("CLK"));
        assert_eq!(wire_label_driver(&sch, &wire("+3V3"), &globals), None, "a power symbol's net");
        assert_eq!(wire_label_driver(&sch, &wire("/Sub/PORT"), &globals), None, "a hierarchical label's");
        assert_eq!(wire_label_driver(&sch, &wire(""), &globals), None);
    }

}
