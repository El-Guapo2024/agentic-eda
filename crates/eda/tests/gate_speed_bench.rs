//! Speed check for the `eda_gates` -> `eda_drc` compatibility shim (see the
//! task report's migration section): `check_placement`/`check_routing` now
//! call `eda_drc::run` internally instead of their own from-scratch
//! geometry, and the build placer's per-step gate loop needs this to stay
//! fast. `#[ignore]`d (prints timing, does not assert a hard budget --
//! machine-dependent); run with:
//! `cargo test -p eda --test gate_speed_bench -- --ignored --nocapture`

use eda_engine::{derive_schematic, EngineOptions};
use eda_gates::{check_placement, check_routing};
use eda_model::ConstraintModel;
use eda_place::{place, PlaceOptions};
use std::time::Instant;

#[test]
#[ignore]
fn check_placement_and_routing_stay_fast_on_l3() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().and_then(|p| p.parent()).expect("crates/eda -> repo root");
    let yaml_path = repo_root.join("examples/ladder/l3_motor_hub.yaml");
    let text = std::fs::read_to_string(&yaml_path).expect("read l3_motor_hub.yaml");
    let model: ConstraintModel = serde_yaml::from_str(&text).expect("parse l3_motor_hub.yaml");

    let seed = 0u64;
    let opts = EngineOptions { seed, intent_hash: "gate_speed_bench".into(), ..Default::default() };
    let design = derive_schematic(&model, &opts).expect("derive_schematic");
    let placed = place(&design, &model, &PlaceOptions { seed, ..Default::default() }).expect("place");
    let part_count = model.parts.len();

    // Placement-only timing (no routing yet -- this is what the build
    // placer's per-step gate loop actually calls, once per part placed).
    const N: u32 = 50;
    let t0 = Instant::now();
    for _ in 0..N {
        std::hint::black_box(check_placement(&placed, &model));
    }
    let placement_us = t0.elapsed().as_micros() as f64 / N as f64;

    let routed = eda_freeroute::design::route_design(&placed, &model, &model.board, 20, 10).expect("route_design");
    let mut full = placed.clone();
    full.routing = Some(routed.routing);

    let t1 = Instant::now();
    for _ in 0..N {
        std::hint::black_box(check_placement(&full, &model));
        std::hint::black_box(check_routing(&full, &model));
    }
    let full_us = t1.elapsed().as_micros() as f64 / N as f64;

    println!("L3 ({part_count} parts): check_placement alone = {placement_us:.0} us/call; check_placement+check_routing = {full_us:.0} us/call ({N} iterations each)");

    // No hard assertion (machine-dependent) -- this is a reported number
    // for the task report / future regressions to compare against, not a
    // pass/fail gate. A four-figure microsecond budget per call is what
    // "stays fast enough for a per-step gate loop" means in practice: the
    // build placer calls this once per part placed, not once per frame.
    assert!(full_us < 200_000.0, "check_placement+check_routing on L3 took {full_us:.0} us/call -- looks like a real regression, not noise");
}
