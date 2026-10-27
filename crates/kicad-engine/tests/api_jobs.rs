//! Exports through the kept `kicad-cli api-server` (`eda_kicad_engine::api::jobs`: `drill`, `pos`, `ipcd356`) against the per-run kicad-cli they replace
//! (`EDA_KICAD_API=0`), through the one entry point the studio and the CLI use (`export_pcb`): the same files, byte for byte but the lines that carry the
//! time of the run, and how long each takes. Boards: KiCad's QA `complex_hierarchy` (72 footprints, through holes) and `vme-wren` (1,508 footprints),
//! read into a design and derived again as the studio derives every board; `API_JOBS_BOARDS=a.kicad_pcb,..` names others.
//!
//! Slow tier (kicad-cli, many runs): `EDA_SLOW_TESTS=1`; a kicad-cli without `api-server` skips it. One test: the switch is an environment variable of
//! this process.

use eda_kicad::import_kicad_pcb;
use eda_kicad_engine::{api, export_pcb};
use std::path::{Path, PathBuf};
use std::time::Instant;

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
    std::env::remove_var(api::server::ENV);
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
            let run = |tag: &str, per_run: bool| -> (Vec<PathBuf>, (f64, f64), String) {
                if per_run {
                    std::env::set_var(api::server::ENV, "0");
                } else {
                    std::env::remove_var(api::server::ENV);
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
                (files_of(&reply, &dir), (first, again), reply["engine"].as_str().unwrap_or("").to_string())
            };
            let (per_run, (c1, c2), e_cli) = run("cli", true);
            let (served, (a1, a2), e_api) = run("api", false);
            std::env::remove_var(api::server::ENV);
            assert!(!e_cli.contains("api-server") && e_api.ends_with("(api-server)"), "{board} {kind}: engines {e_cli:?} / {e_api:?}");
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
