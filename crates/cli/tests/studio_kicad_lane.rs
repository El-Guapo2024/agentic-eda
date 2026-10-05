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
esac
rmdir "$FAKE_KICAD_LOG.running" 2>/dev/null
echo "end $1-$2" >> "$FAKE_KICAD_LOG"
"#
    );
    let path = dir.join("fake-kicad-cli.sh");
    std::fs::write(&path, script).unwrap();
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
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
    for _ in 0..5 {
        let (status, body, took) = studio.request("GET", "/api/version", "");
        assert_eq!(status, 200);
        assert!(took < INSTANT, "/api/version took {took:?} while a DRC ran");
        assert_eq!(serde_json::from_str::<String>(&body).unwrap(), before, "nothing has changed the board yet");
    }
    // An edit, applied and undoable as ever.
    let (reply, took) = studio.cmd(rotate("R1"));
    assert_eq!(reply["ok"], true, "{reply}");
    assert!(took < INSTANT, "an /api/cmd edit took {took:?} while a DRC ran");
    assert_eq!(studio.rotation_of("R1"), 90.0);
    let (undone, took) = {
        let (status, body, took) = studio.request("POST", "/api/undo", &json!({ "domain": "pcb" }).to_string());
        assert_eq!(status, 200);
        (serde_json::from_str::<Value>(&body).unwrap(), took)
    };
    assert_eq!(undone["ok"], true, "{undone}");
    assert!(took < INSTANT, "/api/undo took {took:?} while a DRC ran");
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
        assert!(took < Duration::from_secs(1), "an edit waited {took:?} for the DRC");
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
