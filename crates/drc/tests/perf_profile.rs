//! Performance guard + a one-off profiling entry point for the DRC engine.
//!
//! `l4_drc_perf_budget` is the task's required regression test: KiCad-scale
//! boards must run full DRC in a couple of seconds, not the 60s+ measured
//! before the DRC_RTREE port (see `docs/parity/GAPS.md` #2). `profile_l2_once`
//! is not an assertion -- it exists so `sample <pid>` (macOS) can be pointed
//! at a single, long-enough `eda_drc::run` call while developing a fix; left
//! in place (`#[ignore]`d) as a reproducible way to re-profile later,
//! matching the task's own instruction to profile "a pipeline output of l2
//! at seed 0".
//!
//! Construction mirrors `crates/drc/tests/parity_drc.rs::process_example`
//! (derive -> place -> best-effort route), minus the kicad-cli comparison --
//! this file needs no external tools and no QA corpus.

use std::path::PathBuf;
use std::time::Instant;

use eda_engine::{derive_schematic, EngineOptions};
use eda_model::ir::Design;
use eda_model::ConstraintModel;
use eda_place::{place, PlaceOptions};

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().and_then(|p| p.parent()).expect("crates/drc -> repo root").to_path_buf()
}

fn route_best_effort(design: &Design, model: &ConstraintModel) -> Design {
    let mut out = design.clone();
    if let Ok(routed) = eda_freeroute::design::route_design(design, model, &model.board, 20, 10) {
        if !routed.unrouted.is_empty() {
            eprintln!("  (best-effort route left unrouted: {:?})", routed.unrouted);
        }
        out.routing = Some(routed.routing);
    }
    out
}

fn build_routed(rung: &str) -> (Design, ConstraintModel) {
    let repo = repo_root();
    let yaml_path = repo.join("examples/ladder").join(format!("{rung}.yaml"));
    let text = std::fs::read_to_string(&yaml_path).unwrap_or_else(|e| panic!("read {}: {e}", yaml_path.display()));
    let model: ConstraintModel = serde_yaml::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", yaml_path.display()));
    let opts = EngineOptions { seed: 0, intent_hash: format!("perf_profile_{rung}"), ..Default::default() };

    let t0 = Instant::now();
    let design = derive_schematic(&model, &opts).unwrap_or_else(|e| panic!("derive_schematic: {e:?}"));
    let t_schem = t0.elapsed();

    let t1 = Instant::now();
    let placed = place(&design, &model, &PlaceOptions { seed: 0, ..Default::default() }).unwrap_or_else(|e| panic!("place: {e:?}"));
    let t_place = t1.elapsed();

    let t2 = Instant::now();
    let routed = route_best_effort(&placed, &model);
    let t_route = t2.elapsed();

    let n_tracks: usize = routed.routing.as_ref().map(|r| r.tracks.len()).unwrap_or(0);
    let n_segs: usize = routed.routing.as_ref().map(|r| r.tracks.iter().map(|t| t.pts.len().saturating_sub(1)).sum()).unwrap_or(0);
    let n_vias: usize = routed.routing.as_ref().map(|r| r.vias.len()).unwrap_or(0);
    eprintln!("{rung}: stage timings -- schematic={t_schem:?} place={t_place:?} route={t_route:?} ({n_tracks} tracks, {n_segs} segments, {n_vias} vias)");
    (routed, model)
}

/// Not a real assertion: a long-enough, reproducible `eda_drc::run` call to
/// point `sample <pid>` (macOS) at while profiling, e.g.:
///   cargo test --release -p eda-drc --test perf_profile -- --ignored --nocapture profile_l2_once &
///   sleep 1; sample $! 10 -f /tmp/l2_sample.txt
#[test]
#[ignore]
fn profile_l2_once() {
    let (design, model) = build_routed("l2_sensor_hub");
    let t = Instant::now();
    let violations = eda_drc::run(&design, &model);
    eprintln!("l2_sensor_hub: eda_drc::run took {:?}, {} violations", t.elapsed(), violations.len());
    let mut counts = std::collections::BTreeMap::new();
    for v in &violations {
        *counts.entry(v.error_type).or_insert(0) += 1;
    }
    eprintln!("by type: {counts:?}");
    eprintln!("sample tracks_crossing items:");
    for v in violations.iter().filter(|v| v.error_type == "tracks_crossing").take(8) {
        eprintln!("  {:?}", v.items.iter().map(|it| &it.description).collect::<Vec<_>>());
    }
    eprintln!("sample hole_clearance items:");
    for v in violations.iter().filter(|v| v.error_type == "hole_clearance").take(8) {
        eprintln!("  {:?}", v.items.iter().map(|it| &it.description).collect::<Vec<_>>());
    }
}

#[test]
#[ignore]
fn profile_l3_once() {
    let (design, model) = build_routed("l3_motor_hub");
    let t = Instant::now();
    let violations = eda_drc::run(&design, &model);
    eprintln!("l3_motor_hub: eda_drc::run took {:?}, {} violations", t.elapsed(), violations.len());
}

#[test]
#[ignore]
fn profile_l4_once() {
    let (design, model) = build_routed("l4_control_hub");
    let t = Instant::now();
    let violations = eda_drc::run(&design, &model);
    eprintln!("l4_control_hub: eda_drc::run took {:?}, {} violations", t.elapsed(), violations.len());
}

/// The task's required perf regression test. Only `eda_drc::run` itself is
/// timed -- schematic/place/route are setup, not the thing under test.
/// Budget is generous in a debug build (nobody runs perf-sensitive
/// `cargo test` without `--release`, but a plain debug `cargo test` must
/// not flake here); release is held to the task's "a couple of seconds"
/// target with headroom.
#[test]
fn l4_drc_perf_budget() {
    let (design, model) = build_routed("l4_control_hub");
    let t = Instant::now();
    let violations = eda_drc::run(&design, &model);
    let elapsed = t.elapsed();
    let budget = if cfg!(debug_assertions) { std::time::Duration::from_secs(120) } else { std::time::Duration::from_secs(10) };
    eprintln!("l4_control_hub: eda_drc::run took {elapsed:?} ({} violations, budget {budget:?})", violations.len());
    assert!(elapsed <= budget, "eda_drc::run on l4_control_hub took {elapsed:?}, over the {budget:?} budget (see docs/parity/GAPS.md #2)");
}
