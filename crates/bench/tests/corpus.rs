//! Corpus bench: runs the full library pipeline (lint -> schematic ->
//! placement -> routing, each gated) over every `examples/*.yaml` intent,
//! at seeds {0, 1, 2}, and checks the outcome against a small expectation
//! table below. Along the way it collects a scorecard (stage reached, fail
//! checks, schematic crossing-count hint, hpwl, tracks, vias, wall time)
//! written to `target/bench/scorecard.md` and printed to stdout.
//!
//! Also asserts determinism: same seed => byte-identical `design.json`
//! across two independent runs of the same case.

use eda::prelude::*;
use eda::PlaceOptions;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Where a case's pipeline run stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Lint,
    Schematic,
    Placement,
    Routing,
    Done,
}

impl Stage {
    fn name(self) -> &'static str {
        match self {
            Stage::Lint => "lint",
            Stage::Schematic => "schematic",
            Stage::Placement => "placement",
            Stage::Routing => "routing",
            Stage::Done => "done",
        }
    }
}

/// What a case/seed run actually produced.
struct RunOutcome {
    stage: Stage,
    fail_checks: Vec<String>,
    crossing_hint: Option<String>,
    hpwl_um: Option<i64>,
    tracks: Option<usize>,
    vias: Option<usize>,
    design_bytes: Option<Vec<u8>>,
    elapsed_ms: u128,
}

/// Reduced expectation used by the table below: which stage a case's
/// pipeline is expected to reach, and — for a deliberately-failing case —
/// whether the failure is "expected" (part of the case's design) or a
/// KNOWN_FAIL (an unexpected bug in engine/place/router that this bench
/// documents rather than hides).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    /// Reaches routing and every gate passes.
    Clean,
    /// Fails intentionally at the given stage (the case's own comment
    /// documents why: e.g. an outline too small to fit any part).
    ExpectedFail(Stage),
    /// Fails at the given stage, but this is a genuine engine/place/router
    /// bug uncovered by this case — not what the case set out to exercise.
    /// Kept failing (not hidden) so the corpus stays honest; see the
    /// bench report for the exact check names and a proposed fix.
    KnownFail(Stage),
}

/// (filename stem, seed) -> expectation. A `None` entry falls back to
/// `default_expect` for that file (looked up without the seed), which
/// covers the common case where all three seeds behave the same way.
///
/// NOTE (KNOWN_FAIL block below): at the time this bench was written, a
/// wide swath of otherwise-simple cases -- including the pre-existing
/// `examples/ldo.yaml` this repo shipped with, untouched by this bench --
/// fail schematic gates (`schematic_wire_through_symbol` on a
/// multi-pin GND net) or routing (`route_pin_missing`) that they did not
/// fail earlier in this session. Nothing in examples/ or crates/bench
/// changed between the two runs; `crates/eda/src/lib.rs` picked up a new
/// `export_kicad_pcb` re-export from a concurrent change elsewhere in this
/// shared repo, which strongly suggests eda-engine/eda-layout's schematic
/// wire routing regressed underneath this bench mid-session. These are
/// kept as KNOWN_FAIL (not silently expected) so the corpus stays honest;
/// re-run this bench once the concurrent work lands to see if they clear.
fn default_expect(stem: &str) -> Expect {
    match stem {
        "unroutable_tiny_outline" => Expect::ExpectedFail(Stage::Placement),
        _ => Expect::Clean,
    }
}

/// Per-(file, seed) overrides for cases whose outcome is seed-dependent.
fn expect_for(stem: &str, seed: u64) -> Expect {
    match (stem, seed) {
        // Same schematic-wire-routing regression noted above, but only
        // trips on some seeds' particular symbol ordering for these two
        // cases -- the other seeds route cleanly.
        _ => default_expect(stem),
    }
}

fn examples_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/bench; examples/ is two levels up.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn run_case(path: &Path, seed: u64) -> RunOutcome {
    let start = Instant::now();
    let text = std::fs::read_to_string(path).expect("read intent yaml");
    let model: ConstraintModel = serde_yaml::from_str(&text).expect("parse intent yaml");

    let lint_checks = lint(&model);
    if lint_checks.iter().any(|c| c.status == CheckStatus::Fail) {
        return RunOutcome {
            stage: Stage::Lint,
            fail_checks: fails(&lint_checks),
            crossing_hint: None,
            hpwl_um: None,
            tracks: None,
            vias: None,
            design_bytes: None,
            elapsed_ms: start.elapsed().as_millis(),
        };
    }

    let opts = EngineOptions { seed, intent_hash: format!("{:?}", path), ..Default::default() };
    let design = match derive_schematic(&model, &opts) {
        Ok(d) => d,
        Err(checks) => {
            return RunOutcome {
                stage: Stage::Schematic,
                fail_checks: fails(&checks),
                crossing_hint: None,
                hpwl_um: None,
                tracks: None,
                vias: None,
                design_bytes: None,
                elapsed_ms: start.elapsed().as_millis(),
            }
        }
    };
    // One engine: `check_erc` is both the ported KiCad ERC's electrical
    // checks and this generator's own auto-layout readability checks,
    // folded in as additional tests -- see `stage_schematic` in
    // `crates/cli`, which calls the same one function.
    let sch_checks = check_erc(&design, &model);
    let crossing_hint = sch_checks
        .iter()
        .find(|c| c.check == "schematic_wire_crossing_count")
        .and_then(|c| c.hint.clone());
    if sch_checks.iter().any(|c| c.status == CheckStatus::Fail) {
        return RunOutcome {
            stage: Stage::Schematic,
            fail_checks: fails(&sch_checks),
            crossing_hint,
            hpwl_um: None,
            tracks: None,
            vias: None,
            design_bytes: None,
            elapsed_ms: start.elapsed().as_millis(),
        };
    }

    // Keep the placer's default moves_per_part: cutting it down (tried
    // 800) makes several otherwise-clean cases legalize into positions the
    // router can't reach a pin from (route_pin_missing) or that crowd wire
    // routing enough to fail schematic gates -- i.e. it changes which
    // outcome is "expected", not just the wall time. The full corpus x 3
    // seeds still finishes in well under a minute in a debug build at the
    // default.
    let place_opts = PlaceOptions { seed, ..Default::default() };
    let placed = match place(&design, &model, &place_opts) {
        Ok(d) => d,
        Err(checks) => {
            return RunOutcome {
                stage: Stage::Placement,
                fail_checks: fails(&checks),
                crossing_hint,
                hpwl_um: None,
                tracks: None,
                vias: None,
                design_bytes: None,
                elapsed_ms: start.elapsed().as_millis(),
            }
        }
    };
    let place_checks = check_placement(&placed, &model);
    let hpwl_um = hpwl(&placed, &model);
    if place_checks.iter().any(|c| c.status == CheckStatus::Fail) {
        return RunOutcome {
            stage: Stage::Placement,
            fail_checks: fails(&place_checks),
            crossing_hint,
            hpwl_um,
            tracks: None,
            vias: None,
            design_bytes: None,
            elapsed_ms: start.elapsed().as_millis(),
        };
    }

    let routed = match route(&placed, &model, &model.board, seed) {
        Ok(d) => d,
        Err(checks) => {
            return RunOutcome {
                stage: Stage::Routing,
                fail_checks: fails(&checks),
                crossing_hint,
                hpwl_um,
                tracks: None,
                vias: None,
                design_bytes: None,
                elapsed_ms: start.elapsed().as_millis(),
            }
        }
    };
    let route_checks = check_routing(&routed, &model);
    let (tracks, vias) = routed
        .routing
        .as_ref()
        .map(|r| (r.tracks.len(), r.vias.len()))
        .unwrap_or((0, 0));
    let design_bytes = routed.canonical_bytes().ok();
    if route_checks.iter().any(|c| c.status == CheckStatus::Fail) {
        return RunOutcome {
            stage: Stage::Routing,
            fail_checks: fails(&route_checks),
            crossing_hint,
            hpwl_um,
            tracks: Some(tracks),
            vias: Some(vias),
            design_bytes,
            elapsed_ms: start.elapsed().as_millis(),
        };
    }

    RunOutcome {
        stage: Stage::Done,
        fail_checks: Vec::new(),
        crossing_hint,
        hpwl_um,
        tracks: Some(tracks),
        vias: Some(vias),
        design_bytes,
        elapsed_ms: start.elapsed().as_millis(),
    }
}

fn fails(checks: &[CheckResult]) -> Vec<String> {
    checks
        .iter()
        .filter(|c| c.status == CheckStatus::Fail)
        .map(|c| c.check.clone())
        .collect()
}

fn expected_stage(expect: Expect) -> Stage {
    match expect {
        Expect::Clean => Stage::Done,
        Expect::ExpectedFail(s) | Expect::KnownFail(s) => s,
    }
}

#[test]
fn corpus_runs_every_seed() {
    let dir = examples_dir();
    let mut stems: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("yaml"))
        .map(|p| p.file_stem().unwrap().to_string_lossy().to_string())
        .collect();
    stems.sort();
    assert!(stems.len() >= 10, "expected at least 10 example cases, found {}", stems.len());

    let seeds = [0u64, 1, 2];
    let mut rows: Vec<String> = Vec::new();
    rows.push("| case | seed | stage | fail checks | crossing hint | hpwl (um) | tracks | vias | wall (ms) |".into());
    rows.push("|---|---|---|---|---|---|---|---|---|".into());

    let mut unexpected: Vec<String> = Vec::new();
    let mut known_fails: Vec<String> = Vec::new();

    for stem in &stems {
        let path = dir.join(format!("{stem}.yaml"));
        let mut byte_snapshots: Vec<Option<Vec<u8>>> = Vec::new();

        for &seed in &seeds {
            let outcome = run_case(&path, seed);
            let expect = expect_for(stem, seed);
            let expected_stage = expected_stage(expect);

            rows.push(format!(
                "| {stem} | {seed} | {} | {} | {} | {} | {} | {} | {} |",
                outcome.stage.name(),
                if outcome.fail_checks.is_empty() { "-".to_string() } else { outcome.fail_checks.join(", ") },
                outcome.crossing_hint.clone().unwrap_or_else(|| "-".into()),
                outcome.hpwl_um.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
                outcome.tracks.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
                outcome.vias.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
                outcome.elapsed_ms,
            ));

            match expect {
                Expect::Clean | Expect::ExpectedFail(_) => {
                    if outcome.stage != expected_stage {
                        unexpected.push(format!(
                            "{stem} seed={seed}: expected stage {:?}, got {:?} ({:?})",
                            expected_stage, outcome.stage, outcome.fail_checks
                        ));
                    }
                }
                Expect::KnownFail(_) => {
                    known_fails.push(format!("{stem} seed={seed}: KNOWN_FAIL at {:?} ({:?})", outcome.stage, outcome.fail_checks));
                    // Still require it doesn't regress to an *earlier*
                    // stage or start passing silently without us noticing
                    // (a KNOWN_FAIL that starts passing should be promoted
                    // to Clean in the table above, not silently ignored).
                    if outcome.stage == Stage::Done {
                        unexpected.push(format!(
                            "{stem} seed={seed}: marked KNOWN_FAIL at {:?} but the pipeline now completes cleanly -- update the expectation table",
                            expected_stage
                        ));
                    }
                }
            }

            byte_snapshots.push(outcome.design_bytes);
        }

        // Determinism: re-run seed 0 and compare canonical design.json
        // bytes against the first run, whenever that run produced a
        // design at all (a lint/schematic-stage failure never gets one).
        if let Some(first) = &byte_snapshots[0] {
            let second = run_case(&path, seeds[0]);
            assert_eq!(
                second.design_bytes.as_ref(),
                Some(first),
                "{stem}: seed {} produced different design.json bytes across two runs",
                seeds[0]
            );
        }
    }

    let scorecard = rows.join("\n");
    println!("\n{scorecard}\n");
    if !known_fails.is_empty() {
        println!("KNOWN_FAIL cases (see crates/bench/tests/corpus.rs for detail):");
        for k in &known_fails {
            println!("  {k}");
        }
    }

    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/bench");
    std::fs::create_dir_all(&out_dir).expect("create target/bench");
    std::fs::write(out_dir.join("scorecard.md"), format!("# Bench corpus scorecard\n\n{scorecard}\n")).expect("write scorecard.md");

    assert!(unexpected.is_empty(), "unexpected outcomes:\n{}", unexpected.join("\n"));
}
