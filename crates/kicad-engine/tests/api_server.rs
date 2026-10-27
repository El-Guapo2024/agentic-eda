//! The api-server lane (`eda_kicad_engine::api`) against the real `kicad-cli api-server`: it starts on demand, answers `Ping` and `GetVersion`, opens
//! a board the studio derives, comes back after its process is killed, and is gone after `shutdown`. Slow tier (it starts kicad-cli): `EDA_SLOW_TESTS=1`;
//! a kicad-cli without `api-server` skips it (the lane is closed there, which `unavailable` must say).
//!
//! One test, because the lane is one per process and these steps follow each other.

use eda_kicad_engine::api::proto::kiapi::board::commands::GetNets;
use eda_kicad_engine::api::proto::kiapi::board::commands::NetsResponse;
use eda_kicad_engine::api::{self, ApiError};
use std::time::{Duration, Instant};

fn group_alive(pgid: u32) -> bool {
    std::process::Command::new("kill").args(["-0", "--", &format!("-{pgid}")]).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success())
}

#[test]
fn the_lane_starts_answers_restarts_and_stops() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    if let Some(why) = api::unavailable() {
        eprintln!("skipped: the lane is closed here ({why})");
        return;
    }

    // Started on demand: the first request pays the start.
    let t = Instant::now();
    let version = api::with_server(|c| c.version()).expect("the server starts and answers GetVersion");
    eprintln!("api-server up and answering in {:.2} s: KiCad {version}", t.elapsed().as_secs_f64());
    assert!(version.starts_with("10.") || version.starts_with("11."), "{version}");
    let first = api::server::starts();
    assert!(first >= 1);

    // A second request reuses it.
    let t = Instant::now();
    api::with_server(|c| c.version()).unwrap();
    eprintln!("second request: {:.3} s", t.elapsed().as_secs_f64());
    assert_eq!(api::server::starts(), first, "the same server answers");

    // A board of KiCad's QA: opened, its nets read, closed.
    let qa = std::env::var_os("KICAD_QA_DATA").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from(format!("{}/src/kicad-src-8303b2ad/qa/data", std::env::var("HOME").unwrap())));
    let work = tempdir();
    let pcb = work.join("board.kicad_pcb");
    std::fs::copy(qa.join("pcbnew/issue5990.kicad_pcb"), &pcb).expect("KiCad's QA board issue5990 (set KICAD_QA_DATA)");
    let nets: NetsResponse = api::with_server(|c| {
        let doc = c.open_board(&pcb)?;
        let r = c.call(&GetNets { board: Some(doc), netclass_filter: vec![] }, Duration::from_secs(60));
        c.close_all()?;
        r
    })
    .expect("GetNets on the board");
    let text = std::fs::read_to_string(&pcb).unwrap();
    let names: Vec<&str> = nets.nets.iter().map(|n| n.name.as_str()).collect();
    assert!(names.len() > 1, "{names:?}");
    for n in names.iter().filter(|n| !n.is_empty()) {
        assert!(text.contains(&format!("\"{n}\"")), "net {n} is the file's");
    }

    // A command the server does not know is Unhandled, and that falls back.
    let e = ApiError::Status { code: eda_kicad_engine::api::proto::kiapi::common::ApiStatusCode::AsUnhandled, message: String::new() };
    assert!(e.falls_back());

    // Killed under the lane: the next request restarts it.
    let pgid = api::server::running_group_id().expect("a server runs");
    std::process::Command::new("kill").args(["-9", "--", &format!("-{pgid}")]).status().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let t = Instant::now();
    api::with_server(|c| c.version()).expect("a new server answers after the old one was killed");
    eprintln!("restart after kill -9: {:.2} s", t.elapsed().as_secs_f64());
    assert_eq!(api::server::starts(), first + 1, "one restart");

    // Shut down: the process group is gone.
    let pgid = api::server::running_group_id().unwrap();
    assert!(group_alive(pgid));
    api::shutdown();
    let deadline = Instant::now() + Duration::from_secs(10);
    while group_alive(pgid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!group_alive(pgid), "the server is gone after shutdown");
    let _ = std::fs::remove_dir_all(&work);
}

fn tempdir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("eda-api-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}
