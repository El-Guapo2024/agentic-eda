//! Exports through the kept `kicad-cli api-server` (`eda_kicad_engine::api::jobs`: `drill`, `pos`, `ipcd356`) against the per-run kicad-cli they replace
//! (`EDA_KICAD_API=0`), through the one entry point the studio and the CLI use (`export_pcb`): the same files, byte for byte but the lines that carry the
//! time of the run, the same words in the reply's `log`, and how long each takes. Boards: KiCad's QA `complex_hierarchy` (72 footprints, through holes)
//! and `vme-wren` (1,508 footprints), read into a design and derived again as the studio derives every board; `API_JOBS_BOARDS=a.kicad_pcb,..` names
//! others.
//!
//! The lane is asked for with `EDA_KICAD_API=1`, since X5-3: a one-shot process no longer starts the server unless told to (the studio says it is
//! long-lived instead).
//!
//! Slow tier (kicad-cli, many runs): `EDA_SLOW_TESTS=1`; a kicad-cli without `api-server` skips it. One test: the switch is an environment variable of
//! this process; the tests below share it, so they run one after the other (`ONE`).

use eda_kicad::import_kicad_pcb;
use eda_kicad_engine::{api, export_pcb};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

/// The tests here set one process-wide environment variable, so only one runs at a time.
static ONE: Mutex<()> = Mutex::new(());

fn qa() -> PathBuf {
    std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(format!("{}/src/kicad-src-8303b2ad/qa/data", std::env::var("HOME").unwrap())))
}

/// A file's text without the lines that carry the date and time of the run: Excellon's `; DRILL file {...} date ...`, the placement file's `## Created
/// on ...`, IPC-D-356's `C  ... DATE ...`, Gerber's `%TF.CreationDate,...*%`.
fn timeless(path: &Path) -> String {
    let text = String::from_utf8_lossy(&std::fs::read(path).unwrap()).to_string();
    text.lines().filter(|l| !(l.contains("date") || l.contains("Date") || l.contains("DATE") || l.to_lowercase().contains("created on"))).collect::<Vec<_>>().join("\n")
}

fn files_of(reply: &serde_json::Value, root: &Path) -> Vec<PathBuf> {
    reply["files"].as_array().unwrap().iter().map(|f| root.join(f.as_str().unwrap())).collect()
}

#[test]
fn the_server_writes_what_per_run_kicad_cli_writes_and_sooner() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let _one = ONE.lock().unwrap_or_else(|p| p.into_inner());
    std::env::set_var(api::server::ENV, "1");
    if let Some(why) = api::unavailable() {
        eprintln!("skipped: the lane is closed here ({why})");
        return;
    }
    let boards = std::env::var("API_JOBS_BOARDS").map(|v| v.split(',').map(String::from).collect::<Vec<_>>()).unwrap_or_else(|_| vec!["complex_hierarchy.kicad_pcb".into(), "vme-wren.kicad_pcb".into()]);
    let exports: Vec<(&str, Vec<String>)> = vec![
        ("drill", vec![]),
        ("drill", "--format gerber --generate-map --map-format svg --excellon-units in".split(' ').map(String::from).collect()),
        ("pos", vec![]),
        ("pos", "--format csv --units mm --side front".split(' ').map(String::from).collect()),
        ("ipcd356", vec![]),
    ];
    let root = std::env::temp_dir().join(format!("eda-api-jobs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut rows = vec!["times: the first export of the design / the same export again (the mean of two), each including the derivation of the board file".to_string()];
    // The server's start, paid once by the first export through it.
    let t = Instant::now();
    api::with_server(|c| c.version()).expect("the server starts");
    rows.push(format!("server start: {:.2} s", t.elapsed().as_secs_f64()));
    for board in &boards {
        let text = std::fs::read_to_string(qa().join("pcbnew").join(board)).unwrap_or_else(|e| panic!("{board}: {e} (set KICAD_QA_DATA)"));
        let (mut design, model, _) = import_kicad_pcb(&text).unwrap_or_else(|e| panic!("import {board}: {e:?}"));
        if let Some(r) = design.routing.as_mut() {
            r.assign_missing_ids();
        }
        for (kind, args) in &exports {
            let run = |tag: &str, per_run: bool| -> (Vec<PathBuf>, (f64, f64), String, String) {
                if per_run {
                    std::env::set_var(api::server::ENV, "0");
                } else {
                    std::env::set_var(api::server::ENV, "1");
                }
                let dir = root.join(format!("{board}-{kind}-{}-{tag}", args.join("_")));
                let mut times = Vec::new();
                let mut last = None;
                for _ in 0..3 {
                    let t = Instant::now();
                    let reply = export_pcb(&design, &model, &dir.join(".kicad"), &dir, "b", kind, args).unwrap_or_else(|e| panic!("{board} {kind} {args:?} ({tag}): {e:?}"));
                    times.push(t.elapsed().as_secs_f64());
                    last = Some(reply);
                }
                let reply = last.unwrap();
                // The first export of a revision, and the next ones (the same design again: the server keeps the board it loaded).
                let (first, again) = (times[0], (times[1] + times[2]) / 2.0);
                // The reply's words, the two runs' folders left out: they name the same files under a different directory.
                let log = reply["log"].as_str().unwrap_or("").replace(&dir.to_string_lossy().to_string(), "<dir>");
                (files_of(&reply, &dir), (first, again), reply["engine"].as_str().unwrap_or("").to_string(), log)
            };
            let (per_run, (c1, c2), e_cli, l_cli) = run("cli", true);
            let (served, (a1, a2), e_api, l_api) = run("api", false);
            std::env::remove_var(api::server::ENV);
            assert!(!e_cli.contains("api-server") && e_api.ends_with("(api-server)"), "{board} {kind}: engines {e_cli:?} / {e_api:?}");
            // X5-1: the served reply says what kicad-cli says, word for word (the job itself answers with no message on success).
            assert_eq!(l_cli, l_api, "{board} {kind} {args:?}: the same log");
            assert!(!l_cli.is_empty(), "{board} {kind} {args:?}: kicad-cli says something");
            let names = |v: &[PathBuf]| v.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect::<Vec<_>>();
            assert_eq!(names(&per_run), names(&served), "{board} {kind} {args:?}: the same files");
            for (a, b) in per_run.iter().zip(&served) {
                let (ta, tb) = (timeless(a), timeless(b));
                let first = ta.lines().zip(tb.lines()).enumerate().find(|(_, (x, y))| x != y).map(|(i, (x, y))| format!("line {}: {x:?} / {y:?}", i + 1));
                assert!(ta == tb, "{board} {kind} {args:?}: {} differs: {}", a.display(), first.unwrap_or_else(|| "length".into()));
            }
            rows.push(format!("{board:26} {kind:8} {:44} per-run {c1:5.2} / {c2:5.2} s  api-server {a1:5.2} / {a2:5.2} s  ({} file(s), same bytes)", args.join(" "), served.len()));
        }
    }
    eprintln!("\n{}", rows.join("\n"));
    api::shutdown();
    let _ = std::fs::remove_dir_all(&root);
}

/// X5-1: a successful export through the lane keeps kicad-cli's words. `ExecuteBoardJob` fills the reply's `message` only on failure, so the log is
/// rebuilt from the job's output paths (`eda_kicad_engine::api::jobs::success_log`); this test runs the per-run path into the same folder and holds the
/// served log to it, then to KiCad's own text for each kind.
#[test]
fn a_served_export_reports_what_kicad_says() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let _one = ONE.lock().unwrap_or_else(|p| p.into_inner());
    std::env::set_var(api::server::ENV, "1");
    if let Some(why) = api::unavailable() {
        eprintln!("skipped: the lane is closed here ({why})");
        return;
    }
    let board = std::env::var("API_JOBS_BOARD").unwrap_or_else(|_| "issue8407.kicad_pcb".into());
    let text = std::fs::read_to_string(qa().join("pcbnew").join(&board)).unwrap_or_else(|e| panic!("{board}: {e} (set KICAD_QA_DATA)"));
    let (mut design, model, _) = import_kicad_pcb(&text).unwrap_or_else(|e| panic!("import {board}: {e:?}"));
    if let Some(r) = design.routing.as_mut() {
        r.assign_missing_ids();
    }
    let root = std::env::temp_dir().join(format!("eda-api-log-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let args = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
    let cases: Vec<(&str, Vec<String>)> = vec![
        ("ipcd356", vec![]),
        ("drill", vec![]),
        ("drill", args("--generate-map --map-format svg")),
        ("pos", vec![]),
        ("pos", args("--format gerber --side front")),
    ];
    for (kind, args) in &cases {
        let dir = root.join(format!("{kind}-{}", args.join("_")));
        let run = |env: &str| {
            std::env::set_var(api::server::ENV, env);
            export_pcb(&design, &model, &dir.join(".kicad"), &dir, "b", kind, args).unwrap_or_else(|e| panic!("{board} {kind} {args:?} ({env}): {e:?}"))
        };
        let per_run = run("0");
        let served = run("1");
        std::env::remove_var(api::server::ENV);
        let l_cli = per_run["log"].as_str().unwrap_or("");
        let l_api = served["log"].as_str().unwrap_or("");
        assert_eq!(l_api, l_cli, "{board} {kind} {args:?}: the served reply says what kicad-cli says");
        assert!(!l_api.is_empty(), "{board} {kind} {args:?}: a successful job says something");
        assert!(served["engine"].as_str().unwrap_or("").ends_with("(api-server)"), "{}", served["engine"]);
        let files: Vec<String> = served["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap().to_string()).collect();
        let full = |f: &str| dir.join(f).display().to_string();
        match *kind {
            // `PCBNEW_JOBS_HANDLER::JobExportIpcD356`.
            "ipcd356" => assert_eq!(l_api, "Successfully created IPC-D-356 file"),
            // `EXCELLON_WRITER::CreateDrillandMapFilesSet`: the drill files with no full stop, the map with one, then `Done.`.
            "drill" => {
                let mut expected: Vec<String> = files.iter().filter(|f| f.ends_with(".drl")).map(|f| format!("Created file '{}'", full(f))).collect();
                expected.extend(files.iter().filter(|f| !f.ends_with(".drl")).map(|f| format!("Created file '{}'.", full(f))));
                expected.push("Done.".into());
                assert_eq!(l_api, expected.join("\n"));
            }
            // `JobExportPos`: one file of both sides is "Wrote position data"; the Gerber writer names the side it wrote.
            "pos" => {
                assert_eq!(files.len(), 1, "{files:?}");
                let expected = if args.iter().any(|a| a == "gerber") { format!("Wrote front position data to '{}'.", full(&files[0])) } else { format!("Wrote position data to '{}'.", full(&files[0])) };
                assert_eq!(l_api, expected);
            }
            _ => unreachable!("{kind}"),
        }
        assert!(files.iter().all(|f| dir.join(f).is_file()), "{files:?}");
    }
    api::shutdown();
    let _ = std::fs::remove_dir_all(&root);
}
