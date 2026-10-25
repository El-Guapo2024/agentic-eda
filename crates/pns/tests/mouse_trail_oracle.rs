//! The mouse trail port against KiCad's own code.
//!
//! `crates/pns/oracle/` compiles `pcbnew/router/pns_mouse_trail_tracer.cpp` verbatim, with `DIRECTION_45` and the real `libs/kimath` geometry, into a
//! program that answers one line per case (`oracle_trail_main.cpp`). Each case here is a line of text; [`evaluate`] runs it through this crate's port
//! ([`eda_pns::mouse_trail::MouseTrailTracer`]) and renders the answer in the oracle's format: after every cursor move the posture the tracer gave, the
//! lead vector of its trail and whether the posture was forced by hand.
//!
//! * `kicad_answers_baked_in`: cases with the answers the oracle gave (`tests/data/mouse_trail_kicad_cases.txt`), always run.
//! * `random_walks_agree_with_the_tracer`: random walks of the cursor, port against oracle, line by line. It needs the oracle binary:
//!   `EDA_PNS_ORACLE_TRAIL=/path/to/oracle_trail cargo test -p eda-pns --test mouse_trail_oracle` (skipped without it). `EDA_PNS_ORACLE_CASES` scales the number
//!   of cases (default 400). `KICAD_SRC_DIR=<checkout of KiCad 8303b2ad> crates/pns/oracle/build.sh` builds it (a minute or two, clang++, no downloads).

use eda_model::units::Nm;
use eda_model::ir::Point;
use eda_pns::direction45::Direction45;
use eda_pns::mouse_trail::MouseTrailTracer;
use std::io::Write;
use std::process::{Command, Stdio};

const NM: i64 = 1000;

fn p(x: i64, y: i64) -> Point {
    Point { x: Nm::new(x), y: Nm::new(y) }
}

/// KiCad's number of a direction: `DIRECTION_45::Directions`, `UNDEFINED` -1.
fn number(d: Direction45) -> i64 {
    match d {
        Direction45::N => 0,
        Direction45::NE => 1,
        Direction45::E => 2,
        Direction45::SE => 3,
        Direction45::S => 4,
        Direction45::SW => 5,
        Direction45::W => 6,
        Direction45::NW => 7,
        Direction45::Undefined => -1,
    }
}

fn direction(n: i64) -> Direction45 {
    [Direction45::N, Direction45::NE, Direction45::E, Direction45::SE, Direction45::S, Direction45::SW, Direction45::W, Direction45::NW].get(n as usize).copied().unwrap_or(Direction45::Undefined)
}

/// One case, in the oracle's text format (nanometres; `crates/pns/oracle/oracle_trail_main.cpp`), run through the port.
fn evaluate(case: &str) -> String {
    let mut it = case.split_whitespace();
    assert_eq!(it.next(), Some("TRAIL"));
    let mut next = || it.next().expect("a token").parse::<i64>().unwrap();
    let tolerance_nm = next();
    let (init, last, disabled, nops) = (next(), next(), next(), next());
    let um = |nm: i64| {
        assert_eq!(nm % NM, 0, "the port's grid is the micrometre");
        nm / NM
    };
    let mut t = MouseTrailTracer::new();
    t.clear();
    t.add_trail_point(p(0, 0));
    t.set_tolerance(Nm::new(um(tolerance_nm)));
    t.set_default_directions(direction(init), direction(last));
    t.set_mouse_disabled(disabled != 0);
    let mut out = String::from("ok");
    let rest: Vec<&str> = case.split_whitespace().skip(6).collect();
    let mut k = 0;
    for _ in 0..nops {
        match rest[k] {
            "M" => {
                let at = p(um(rest[k + 1].parse().unwrap()), um(rest[k + 2].parse().unwrap()));
                k += 3;
                let d = t.get_posture(at);
                t.add_trail_point(at);
                let (lx, ly) = t.trail_lead_vector();
                out += &format!(" d{} m{},{} f{}", number(d), lx * NM, ly * NM, i32::from(t.is_manually_forced()));
            }
            "F" => {
                k += 1;
                t.flip_posture();
                let (lx, ly) = t.trail_lead_vector();
                out += &format!(" F m{},{} f{}", lx * NM, ly * NM, i32::from(t.is_manually_forced()));
            }
            "R" => {
                let at = p(um(rest[k + 1].parse().unwrap()), um(rest[k + 2].parse().unwrap()));
                let (ri, rl) = (rest[k + 3].parse().unwrap(), rest[k + 4].parse().unwrap());
                k += 5;
                t.clear();
                t.set_tolerance(Nm::new(um(tolerance_nm)));
                t.add_trail_point(at);
                t.set_default_directions(direction(ri), direction(rl));
                out += " R";
            }
            "D" => {
                let flag: i64 = rest[k + 1].parse().unwrap();
                k += 2;
                t.set_mouse_disabled(flag != 0);
                out += " D";
            }
            other => panic!("op {other}"),
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------------------------------------ generators

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0.wrapping_mul(0x2545F4914F6CDD1D)
    }
    /// `lo..=hi`.
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo + 1) as u64) as i64
    }
    fn chance(&mut self, percent: i64) -> bool {
        self.range(0, 99) < percent
    }
}

/// A walk of the cursor from the start of a trail: runs along a heading (a multiple of 45 degrees or any), turns, doubles back across itself, returns
/// near the start, with flips, restarts and the mouse switched off and on among the moves.
fn trail_case(r: &mut Rng) -> String {
    let tolerance = r.range(60, 400);
    let init = if r.chance(20) { -1 } else { r.range(0, 7) };
    let last = if r.chance(70) { -1 } else { r.range(0, 7) };
    let disabled = i64::from(r.chance(12));
    let nops = r.range(6, 70);
    let mut ops: Vec<String> = Vec::new();
    let (mut x, mut y) = (0i64, 0i64);
    let mut heading = r.range(0, 7);
    let mut count = 0;
    while count < nops {
        let roll = r.range(0, 99);
        if roll < 4 {
            ops.push("F".into());
        } else if roll < 6 {
            // a restart (`FixRoute`, `UnfixRoute`) at the cursor
            ops.push(format!("R {} {} {} {}", x * NM, y * NM, r.range(-1, 7), if r.chance(70) { -1 } else { r.range(0, 7) }));
        } else if roll < 8 {
            ops.push(format!("D {}", r.range(0, 1)));
        } else {
            if r.chance(25) {
                heading = r.range(0, 7);
            }
            let step = r.range(20, 900);
            if r.chance(6) {
                // back to near the start
                x = r.range(-3, 3) * tolerance;
                y = r.range(-3, 3) * tolerance;
            } else if r.chance(6) {
                // a jump across
                x += r.range(-2500, 2500);
                y += r.range(-2500, 2500);
            } else if r.chance(15) {
                // any angle
                x += r.range(-step, step);
                y += r.range(-step, step);
            } else {
                let (ux, uy) = [(0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)][heading as usize];
                x += ux * step;
                y += uy * step;
            }
            ops.push(format!("M {} {}", x * NM, y * NM));
        }
        count += 1;
    }
    format!("TRAIL {} {init} {last} {disabled} {} {}", tolerance * NM, ops.len(), ops.join(" "))
}

// ------------------------------------------------------------------------------------------------------------------------------ the oracle

fn ask_oracle(oracle: &str, inputs: &[String]) -> Vec<String> {
    let mut child = Command::new(oracle).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("the oracle starts");
    let mut text = String::new();
    for i in inputs {
        text += i;
        text.push('\n');
    }
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        stdin.write_all(text.as_bytes()).unwrap();
    });
    let out = child.wait_with_output().expect("the oracle answers");
    writer.join().unwrap();
    assert!(out.status.success(), "the oracle failed: {:?}", out.status);
    String::from_utf8_lossy(&out.stdout).lines().map(String::from).collect()
}

fn oracle() -> Option<String> {
    std::env::var("EDA_PNS_ORACLE_TRAIL").ok().filter(|p| std::path::Path::new(p).exists())
}

fn n_cases() -> usize {
    std::env::var("EDA_PNS_ORACLE_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(400)
}

#[test]
fn random_walks_agree_with_the_tracer() {
    let Some(oracle) = oracle() else {
        eprintln!("EDA_PNS_ORACLE_TRAIL is not set: skipped");
        return;
    };
    let mut r = Rng(0x9E3779B97F4A7C15);
    let inputs: Vec<String> = (0..n_cases()).map(|_| trail_case(&mut r)).collect();
    let answers = ask_oracle(&oracle, &inputs);
    assert_eq!(answers.len(), inputs.len(), "one answer per case");
    let mut bad = 0;
    let mut first: Option<String> = None;
    for (input, got) in inputs.iter().zip(&answers) {
        let port = evaluate(input);
        if port != *got {
            bad += 1;
            if first.is_none() {
                first = Some(format!("input: {input}\nport:  {port}\nKiCad: {got}"));
            }
        }
    }
    eprintln!("{} walks, {} differ", inputs.len(), bad);
    if let Some(f) = first {
        eprintln!("{f}");
    }
    assert_eq!(bad, 0, "the port and KiCad's own MOUSE_TRAIL_TRACER differ");
}

/// The cases KiCad's code was asked, with its answers (`tests/data/mouse_trail_kicad_cases.txt`: a case line, its answer line, a blank line).
#[test]
fn kicad_answers_baked_in() {
    let data = include_str!("data/mouse_trail_kicad_cases.txt");
    let mut n = 0;
    let mut lines = data.lines();
    while let Some(input) = lines.next() {
        if input.trim().is_empty() || input.starts_with('#') {
            continue;
        }
        let expect = lines.next().expect("an answer after every case");
        assert_eq!(evaluate(input), expect, "{input}");
        n += 1;
    }
    assert!(n >= 40, "{n} cases");
}

/// Writes the baked cases: `EDA_PNS_ORACLE_TRAIL=... EDA_PNS_BAKE=path cargo test -p eda-pns --test mouse_trail_oracle bake -- --ignored` puts a spread
/// of random walks with the oracle's answers into `path`.
#[test]
#[ignore]
fn bake() {
    let (Some(oracle), Ok(path)) = (oracle(), std::env::var("EDA_PNS_BAKE")) else { return };
    let mut r = Rng(0x1234_5678_9ABC_DEF1);
    let inputs: Vec<String> = (0..120).map(|_| trail_case(&mut r)).collect();
    let answers = ask_oracle(&oracle, &inputs);
    let mut kept = String::from("# Cases and the answers pcbnew/router/pns_mouse_trail_tracer.cpp (KiCad 8303b2ad) gave them, nanometres; see tests/mouse_trail_oracle.rs.\n\n");
    let mut n = 0;
    for (i, a) in inputs.iter().zip(&answers) {
        // a spread: walks that lock the posture, flip it or unlock it are kept; so are plain short ones
        if n < 60 && a.starts_with("ok") && i.len() < 2600 {
            kept += &format!("{i}\n{a}\n\n");
            n += 1;
        }
    }
    std::fs::write(path, kept).unwrap();
}
