//! The studio stays responsive while kicad-cli runs (`crates/cli/src/studio.rs`,
//! `kicad_lane.rs`).
//!
//! A DRC takes seconds, and the serve loop used to run it inline, so every
//! other request -- an edit, the `/api/version` poll that drives live
//! co-editing, the 3D view -- waited for it. These start the real server on
//! a free port with `EDA_KICAD_CLI` pointing at a fake kicad-cli (a shell
//! script that logs its start and end, sleeps, and writes a valid empty
//! report) and check what the server does meanwhile: answer, stamp the
//! report with the revision it ran on, and never run two kicad-cli at once.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// What "answers while a DRC runs" means: far under the seconds the run takes.
const INSTANT: Duration = Duration::from_millis(300);

/// A busy machine can stall one request for a moment; a server that makes
/// requests wait out the run stalls every one of them for as long as it takes.
/// So the fastest of a few tries has to be instant.
fn quick(what: &str, tries: &[Duration]) {
    let fastest = tries.iter().min().unwrap();
    assert!(*fastest < INSTANT, "{what}: even the fastest of {} tries took {fastest:?} while a DRC ran: {tries:?}", tries.len());
}

/// A kicad-cli that takes `secs` and writes an empty report. `FAKE_KICAD_LOG`
/// (set by the server's environment, which kicad-cli inherits) gets one line
/// per start and end, and one `overlap` line when another run was going.
fn fake_kicad_cli(dir: &Path, secs: u32) -> PathBuf {
    let script = format!(
        r#"#!/bin/sh
if [ "$1" = version ]; then echo 10.99.0-fake; exit 0; fi
echo "start $1-$2" >> "$FAKE_KICAD_LOG"
mkdir "$FAKE_KICAD_LOG.running" 2>/dev/null || echo "overlap $1-$2" >> "$FAKE_KICAD_LOG"
out=""; prev=""
for a in "$@"; do if [ "$prev" = "-o" ]; then out="$a"; fi; prev="$a"; done
sleep {secs}
case "$1-$2" in
  pcb-drc) echo '{{"coordinate_units":"mm","kicad_version":"10.99.0-fake","violations":[],"unconnected_items":[]}}' > "$out" ;;
  sch-erc) echo '{{"coordinate_units":"mm","kicad_version":"10.99.0-fake","sheets":[]}}' > "$out" ;;
  pcb-export)
    # `pcb export vrml --models-dir m`: one VRML file per `(model ...)` of the board, in `m` beside the output.
    if [ "$3" = vrml ]; then
      for a in "$@"; do board="$a"; done
      outdir=$(dirname "$out"); mkdir -p "$outdir/m"
      grep -o '(model "[^"]*"' "$board" | sed 's/(model "//; s/"$//' | while read -r p; do
        stem=$(basename "$p" | sed 's/\.[^.]*$//')
        printf '#VRML V2.0 utf8\n# fake %s\n' "$stem" > "$outdir/m/$stem.wrl"
      done
      printf '#VRML V2.0 utf8\n' > "$out"
    fi ;;
esac
rmdir "$FAKE_KICAD_LOG.running" 2>/dev/null
echo "end $1-$2" >> "$FAKE_KICAD_LOG"
"#
    );
    let path = dir.join("fake-kicad-cli.sh");
    std::fs::write(&path, script).unwrap();
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    // The first run of a new executable can take a second on macOS (the system scans it first):
    // run it once here, so a test with a short time limit is not timing that.
    let _ = Command::new(&path).arg("version").output();
    path
}

/// A board served by the real `eda board serve`, its kicad-cli the fake one.
/// Killed, and its directory removed, when dropped.
struct Studio {
    dir: PathBuf,
    port: u16,
    server: Child,
}

impl Studio {
    fn start(name: &str, kicad_cli_secs: u32) -> Studio {
        Studio::start_with(name, kicad_cli_secs, &[])
    }

    /// [`Studio::start`] with more environment for the server (and so for its kicad-cli).
    fn start_with(name: &str, kicad_cli_secs: u32, env: &[(&str, &str)]) -> Studio {
        let dir = std::env::temp_dir().join(format!("eda_cli_lane_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let eda = |args: &[&str]| {
            let out = Command::new(env!("CARGO_BIN_EXE_eda")).args(args).output().expect("run eda");
            assert!(out.status.success(), "eda {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
        };
        let board = dir.join("board");
        let intent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/two_pin_nets.yaml");
        eda(&["board", "new", intent.to_str().unwrap(), "-o", board.to_str().unwrap()]);
        for (part, at) in [("R1", "3,3"), ("R2", "6,3"), ("C1", "4.5,6")] {
            eda(&["board", "place", part, "--at", at, "-C", board.to_str().unwrap()]);
        }
        let free = TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
        let mut server = Command::new(env!("CARGO_BIN_EXE_eda"))
            .args(["board", "serve", "-C", board.to_str().unwrap(), "--port", &free.to_string(), "--ui", dir.join("no-ui").to_str().unwrap()])
            .env("EDA_KICAD_CLI", fake_kicad_cli(&dir, kicad_cli_secs))
            .env("FAKE_KICAD_LOG", dir.join("kicad-cli.log"))
            .envs(env.iter().copied())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start eda board serve");
        // The server says which port it took; keep reading its stderr so the pipe never fills.
        let stderr = server.stderr.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Some(port) = line.split("http://127.0.0.1:").nth(1).and_then(|r| r.split('/').next()).and_then(|p| p.parse::<u16>().ok()) {
                    let _ = tx.send(port);
                }
            }
        });
        let port = rx.recv_timeout(Duration::from_secs(60)).expect("the server says where it listens");
        Studio { dir: board, port, server }
    }

    /// One request, one connection (the server answers `Connection: close`):
    /// status, body, and how long it took.
    fn request(&self, method: &str, path: &str, body: &str) -> (u16, String, Duration) {
        request(self.port, method, path, body)
    }

    fn get_json(&self, path: &str) -> Value {
        let (status, body, _) = self.request("GET", path, "");
        assert_eq!(status, 200, "GET {path}: {body}");
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("GET {path}: {e}: {body}"))
    }

    /// `POST /api/cmd` with Strict off, the way the studio sends an edit.
    fn cmd(&self, cmd: Value) -> (Value, Duration) {
        let (status, body, took) = self.request("POST", "/api/cmd", &json!({ "cmd": cmd, "strict": false }).to_string());
        assert_eq!(status, 200, "{body}");
        (serde_json::from_str(&body).unwrap(), took)
    }

    fn kicad_cli_log(&self) -> Vec<String> {
        let log = self.dir.parent().unwrap().join("kicad-cli.log");
        std::fs::read_to_string(log).unwrap_or_default().lines().map(str::to_string).collect()
    }

    fn kicad_cli_runs(&self, kind: &str) -> usize {
        self.kicad_cli_log().iter().filter(|l| **l == format!("start {kind}")).count()
    }

    /// Block until kicad-cli has started a run of `kind`, so what comes next happens while it runs.
    fn wait_for_kicad_cli(&self, kind: &str) {
        let until = Instant::now() + Duration::from_secs(30);
        while self.kicad_cli_runs(kind) == 0 {
            assert!(Instant::now() < until, "kicad-cli never started a {kind} run");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn rotation_of(&self, part: &str) -> f64 {
        let state = self.get_json("/api/state");
        state["parts"].as_array().unwrap().iter().find(|p| p["ref"] == part).unwrap()["rot"].as_f64().unwrap()
    }
}

impl Drop for Studio {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
        let _ = std::fs::remove_dir_all(self.dir.parent().unwrap());
    }
}

fn request(port: u16, method: &str, path: &str, body: &str) -> (u16, String, Duration) {
    let start = Instant::now();
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).unwrap();
    let took = start.elapsed();
    let raw = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    let status = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, body.to_string(), took)
}

/// The edit the tests make: turn `part` by a quarter.
fn rotate(part: &str) -> Value {
    json!({ "op": "rotate", "part": part, "quarter_turns": 1 })
}

#[test]
fn version_and_edits_answer_while_a_drc_runs_and_the_report_is_stamped_with_its_revision() {
    let studio = Studio::start("responsive", 3);
    let before: String = serde_json::from_value(studio.get_json("/api/version")).unwrap();

    let port = studio.port;
    let drc = std::thread::spawn(move || request(port, "GET", "/api/drc", ""));
    studio.wait_for_kicad_cli("pcb-drc");

    // The poll that drives live co-editing, several times: it must keep answering.
    let mut polls = Vec::new();
    for _ in 0..5 {
        let (status, body, took) = studio.request("GET", "/api/version", "");
        assert_eq!(status, 200);
        assert_eq!(serde_json::from_str::<String>(&body).unwrap(), before, "nothing has changed the board yet");
        polls.push(took);
    }
    quick("/api/version", &polls);
    // Edits, applied and undoable as ever.
    let edits: Vec<Duration> = (0..3)
        .map(|_| {
            let (reply, took) = studio.cmd(rotate("R1"));
            assert_eq!(reply["ok"], true, "{reply}");
            took
        })
        .collect();
    quick("an /api/cmd edit", &edits);
    assert_eq!(studio.rotation_of("R1"), 270.0, "three quarter turns, every one applied");
    let undos: Vec<Duration> = (0..3)
        .map(|_| {
            let (status, body, took) = studio.request("POST", "/api/undo", &json!({ "domain": "pcb" }).to_string());
            assert_eq!(status, 200);
            assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["ok"], true, "{body}");
            took
        })
        .collect();
    quick("/api/undo", &undos);
    assert_eq!(studio.rotation_of("R1"), 0.0);
    // Edit again so the board has moved on from the revision the run started on.
    let (reply, _) = studio.cmd(rotate("R2"));
    assert_eq!(reply["ok"], true, "{reply}");
    assert!(!drc.is_finished(), "the DRC was over before the edits were: this test saw nothing");

    let (status, body, took) = drc.join().unwrap();
    assert_eq!(status, 200, "{body}");
    assert!(took > Duration::from_millis(2500), "the run itself still takes as long as kicad-cli does: {took:?}");
    let report: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(report["engine"], "kicad-cli 10.99.0-fake", "{report}");
    assert_eq!(report["violations"], json!([]));
    // The report names the revision it was run on -- the one before the edits --
    // so the studio can tell it is out of date now.
    assert_eq!(report["revision"], before.as_str(), "{report}");
    let now: String = serde_json::from_value(studio.get_json("/api/version")).unwrap();
    assert_ne!(report["revision"], now.as_str(), "the board has changed since the report's revision");
    assert_eq!(studio.kicad_cli_runs("pcb-drc"), 1);
}

#[test]
fn concurrent_drc_requests_for_the_same_revision_start_one_kicad_cli() {
    let studio = Studio::start("single_flight", 2);
    let port = studio.port;
    let drcs: Vec<_> = (0..3).map(|_| std::thread::spawn(move || request(port, "GET", "/api/drc", ""))).collect();
    let replies: Vec<(u16, String, Duration)> = drcs.into_iter().map(|t| t.join().unwrap()).collect();
    assert!(replies.iter().all(|(status, _, _)| *status == 200), "{replies:?}");
    assert!(replies.iter().all(|(_, body, _)| *body == replies[0].1), "every request gets the one run's report: {replies:?}");
    let report: Value = serde_json::from_str(&replies[0].1).unwrap();
    assert!(report["revision"].is_string(), "{report}");
    assert_eq!(studio.kicad_cli_runs("pcb-drc"), 1, "three requests, one kicad-cli process: {:?}", studio.kicad_cli_log());

    // A request after the run is over is a new run, not a cached answer.
    let (status, _, _) = studio.request("GET", "/api/drc", "");
    assert_eq!(status, 200);
    assert_eq!(studio.kicad_cli_runs("pcb-drc"), 2);
}

#[test]
fn a_drc_and_an_erc_never_run_kicad_cli_side_by_side() {
    let studio = Studio::start("one_at_a_time", 1);
    let port = studio.port;
    let drc = std::thread::spawn(move || request(port, "GET", "/api/drc", ""));
    let erc = std::thread::spawn(move || request(port, "GET", "/api/erc", ""));
    let (drc_status, drc_body, _) = drc.join().unwrap();
    let (erc_status, erc_body, _) = erc.join().unwrap();
    assert_eq!((drc_status, erc_status), (200, 200), "{drc_body} / {erc_body}");
    assert_eq!(serde_json::from_str::<Value>(&drc_body).unwrap()["engine"], "kicad-cli 10.99.0-fake", "{drc_body}");
    let erc: Value = serde_json::from_str(&erc_body).unwrap();
    assert_eq!(erc["engine"], "kicad-cli 10.99.0-fake", "{erc_body}");
    assert!(erc["revision"].is_string());
    let log = studio.kicad_cli_log();
    assert_eq!((studio.kicad_cli_runs("pcb-drc"), studio.kicad_cli_runs("sch-erc")), (1, 1), "{log:?}");
    assert!(!log.iter().any(|l| l.starts_with("overlap")), "two kicad-cli ran at once: {log:?}");
}

#[test]
fn edits_made_at_the_same_time_during_a_drc_still_take_turns() {
    let studio = Studio::start("edits_serialize", 2);
    let port = studio.port;
    let drc = std::thread::spawn(move || request(port, "GET", "/api/drc", ""));
    studio.wait_for_kicad_cli("pcb-drc");
    // Six turns of R1 at once. If two of them read the design before either
    // wrote it back, a turn would be lost and R1 would not end up at 180.
    let edits: Vec<_> = (0..6)
        .map(|_| {
            std::thread::spawn(move || {
                let (status, body, took) = request(port, "POST", "/api/cmd", &json!({ "cmd": rotate("R1"), "strict": false }).to_string());
                (status, serde_json::from_str::<Value>(&body).unwrap(), took)
            })
        })
        .collect();
    for edit in edits {
        let (status, reply, took) = edit.join().unwrap();
        assert_eq!(status, 200);
        assert_eq!(reply["ok"], true, "{reply}");
        assert!(took < Duration::from_millis(1500), "an edit waited {took:?} for the DRC");
    }
    assert_eq!(studio.rotation_of("R1"), 180.0, "six quarter turns");
    let (status, _, _) = drc.join().unwrap();
    assert_eq!(status, 200);
    // One shared history: six undo steps, then nothing left to undo.
    for _ in 0..6 {
        let (_, body, _) = studio.request("POST", "/api/undo", &json!({ "domain": "pcb" }).to_string());
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["ok"], true, "{body}");
    }
    assert_eq!(studio.rotation_of("R1"), 0.0);
}

#[test]
fn a_hung_kicad_cli_is_killed_with_a_clear_error_and_the_lane_goes_on() {
    // The fake kicad-cli sleeps 30 s; the server gives a run two seconds (`EDA_KICAD_TIMEOUT_SECS`,
    // standing in for the two minutes a real DRC gets, `eda_kicad_engine::REPORT_TIMEOUT`).
    let studio = Studio::start_with("hung", 30, &[("EDA_KICAD_TIMEOUT_SECS", "2")]);
    let (status, body, took) = studio.request("GET", "/api/drc", "");
    assert_eq!(status, 200, "{body}");
    let reply: Value = serde_json::from_str(&body).unwrap();
    let said = reply["error"].as_str().unwrap_or_else(|| panic!("a DRC that hangs answers with an error: {reply}"));
    assert!(said.contains("kicad-cli pcb drc did not finish within 2 s and was killed"), "{said}");
    assert!(took >= Duration::from_secs(2) && took < Duration::from_secs(20), "the DRC answered after its limit, not after kicad-cli's 30 s: {took:?}");
    assert!(reply["revision"].is_string(), "the error is stamped like any other reply: {reply}");

    // The lane is free again: a different request starts at once and is held to the same limit,
    // instead of waiting behind a DRC that never ends.
    let (status, body, took) = studio.request("GET", "/api/erc", "");
    assert_eq!(status, 200, "{body}");
    let said = serde_json::from_str::<Value>(&body).unwrap()["error"].as_str().unwrap_or_default().to_string();
    assert!(said.contains("kicad-cli sch erc did not finish within 2 s and was killed"), "{said}");
    assert!(took < Duration::from_secs(20), "{took:?}");
    // An export is held to a limit too, and says so the way the Plot dialog reads it.
    let (status, body, _) = studio.request("POST", "/api/fab/drill", "{}");
    assert_eq!(status, 200, "{body}");
    let reply: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(reply["ok"], false, "{reply}");
    assert!(reply["message"].as_str().unwrap_or_default().contains("kicad-cli pcb export drill did not finish within 2 s and was killed"), "{reply}");

    let log = studio.kicad_cli_log();
    assert_eq!((studio.kicad_cli_runs("pcb-drc"), studio.kicad_cli_runs("sch-erc")), (1, 1), "{log:?}");
    assert!(!log.iter().any(|l| l.starts_with("end")), "no run finished: they were killed: {log:?}");
    // And the studio never stopped answering.
    let (status, _, took) = studio.request("GET", "/api/version", "");
    assert_eq!(status, 200);
    assert!(took < Duration::from_secs(2), "{took:?}");
}

/// `GET /api/3dmodel?name=<model path>` over real HTTP, for a model KiCad's library holds as STEP only (all of them in the installed 10.99 library): the page
/// sends every model of the board in one `prepare` and then one request per model that waits for it; the request loop is not held up by kicad-cli's conversion
/// nor by the held requests; the models of one board are one kicad-cli run; the answer is cached; and nothing outside the library and the board is readable.
#[test]
fn the_3d_model_route_converts_in_the_lane_without_holding_the_loop_and_reads_nothing_outside() {
    let name = "models";
    let base = std::env::temp_dir().join(format!("eda_cli_lane_{}_{name}", std::process::id()));
    let (library, cache) = (base.join("3dmodels"), base.join("cache"));
    let studio = Studio::start_with(name, 2, &[("EDA_KICAD_3DMODELS_DIR", library.to_str().unwrap()), ("EDA_3DMODEL_CACHE", cache.to_str().unwrap())]);
    std::fs::create_dir_all(library.join("Lib.3dshapes")).unwrap();
    for part in ["a", "b"] {
        std::fs::write(library.join(format!("Lib.3dshapes/{part}.step")), "ISO-10303-21;").unwrap();
    }
    std::fs::write(base.join("secret.wrl"), "#VRML V2.0 utf8\n# SECRET").unwrap();
    let url = |model: &str, wait: bool| format!("/api/3dmodel?name={}{}", model.replace('{', "%7B").replace('}', "%7D").replace('/', "%2F"), if wait { "&wait=1" } else { "" });
    let (a, b) = ("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/a.step", "${KICAD10_3DMODEL_DIR}/Lib.3dshapes/b.wrl");

    // The board's models, queued together; and the loop is free while kicad-cli runs.
    let names = json!({ "names": [a, b, "${KICAD10_3DMODEL_DIR}/../secret.wrl"] }).to_string();
    let (status, body, took) = studio.request("POST", "/api/3dmodel/prepare", &names);
    assert_eq!(status, 200, "{body}");
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), json!({ "ready": 0, "pending": 2, "failed": 0, "missing": 1 }), "{body}");
    quick("/api/3dmodel/prepare", &[took]);
    // One request per model, each held until its model is in -- a b.wrl the library lacks is its b.step (kicad-cli's --subst-models).
    let port = studio.port;
    let held: Vec<_> = [a, b].iter().map(|m| { let target = url(m, true); std::thread::spawn(move || request(port, "GET", &target, "")) }).collect();
    studio.wait_for_kicad_cli("pcb-export");
    let polls: Vec<Duration> = (0..5).map(|_| studio.request("GET", "/api/version", "").2).collect();
    quick("/api/version while a model converts and two requests are held", &polls);
    let answers: Vec<(u16, String, Duration)> = held.into_iter().map(|t| t.join().unwrap()).collect();
    assert!(answers.iter().all(|(status, _, _)| *status == 200), "{answers:?}");
    assert!(answers[0].1.starts_with("#VRML V2.0 utf8") && answers[0].1.contains("fake a"), "{}", answers[0].1);
    assert!(answers[1].1.contains("fake b"), "{}", answers[1].1);
    assert!(answers.iter().all(|(_, _, took)| *took > Duration::from_millis(1500)), "held for the run, which takes the fake kicad-cli two seconds: {answers:?}");
    assert_eq!(studio.kicad_cli_runs("pcb-export"), 1, "two models of one board, one kicad-cli run: {:?}", studio.kicad_cli_log());
    // Asked again, with or without waiting: from the cache, no new run, and as quick as any other answer.
    for wait in [false, true] {
        let (status, _, took) = studio.request("GET", &url(a, wait), "");
        assert_eq!(status, 200);
        assert!(took < INSTANT, "{took:?}");
    }
    assert_eq!(studio.kicad_cli_runs("pcb-export"), 1);

    // A request that does not wait is answered pending at once, for a model that is not converted yet.
    std::fs::write(library.join("Lib.3dshapes/c.step"), "ISO-10303-21;").unwrap();
    let (status, body, took) = studio.request("GET", &url("${KICAD10_3DMODEL_DIR}/Lib.3dshapes/c.step", false), "");
    assert_eq!((status, serde_json::from_str::<Value>(&body).unwrap()["status"].clone()), (202, json!("pending")), "{body}");
    quick("/api/3dmodel pending", &[took]);

    // Nothing outside the library and the board: not by `..`, not by an absolute path, not by an encoded one, not a file that is no model.
    for target in [
        url("${KICAD10_3DMODEL_DIR}/../secret.wrl", false),
        url(&base.join("secret.wrl").to_string_lossy(), false),
        "/api/3dmodel?name=%2e%2e%2fsecret.wrl".to_string(),
        url("/etc/passwd", false),
        url("${KIPRJMOD}/design.json", false),
        url("${KICAD10_3DMODEL_DIR}/../board/design.json", true),
    ] {
        let (status, body, _) = studio.request("GET", &target, "");
        assert!(status == 403 || status == 404, "{target} -> {status} {body}");
        assert!(!body.contains("SECRET") && !body.contains("root:") && !body.contains("\"schema\""), "{target} leaked a file: {body}");
    }
}
