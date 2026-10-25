//! `DIRECTION_45::BuildInitialTrace` in every corner mode against KiCad's own (`libs/kimath/src/geometry/direction_45.cpp`, compiled verbatim over
//! the real kimath by `crates/pns/oracle/build_direction.sh`; the driver is `oracle_direction_main.cpp`).
//!
//! A case is `dir p0x p0y p1x p1y start_diagonal mode` in nanometres (the points on the micrometre grid, as a studio cursor's often are); the
//! answer is the chain's pieces in order: `P x y` for a point that is not on an arc, `A sx sy mx my ex ey` once per arc. The port and KiCad both
//! work in nanometres, and the answers are compared exactly.
//!
//! * `kicad_answers_baked_in`: cases with the answers the oracle gave (`tests/data/direction45_kicad_cases.txt`), always run.
//! * `random_traces_agree_with_kicad`: random cases, port against oracle. Needs `EDA_PNS_ORACLE_DIR=<path to oracle_direction>` (skipped without it);
//!   `EDA_PNS_ORACLE_CASES` scales the number of cases (default 20000).

use eda_model::units::Nm;
use eda_model::ir::Point;
use eda_pns::direction45::{CornerMode, Direction45};
use std::io::Write;
use std::process::{Command, Stdio};

const DIRS: [Direction45; 8] = [Direction45::N, Direction45::NE, Direction45::E, Direction45::SE, Direction45::S, Direction45::SW, Direction45::W, Direction45::NW];
const MODES: [CornerMode; 4] = [CornerMode::Mitered45, CornerMode::Rounded45, CornerMode::Mitered90, CornerMode::Rounded90];

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: i64) -> i64 {
        (self.next() % n as u64) as i64
    }
}

/// A piece of a chain: a point, or an arc's start, middle and end.
#[derive(Debug, Clone, PartialEq)]
enum Piece {
    P(Point),
    A(Point, Point, Point),
}

/// An arc of KiCad's whose middle is in line with its ends: the IR cannot hold its curve, and the port makes it its chord. Compared as the chord.
fn flat_on_the_grid(s: Point, m: Point, e: Point) -> bool {
    (m.x - s.x).area(e.y - s.y) - (m.y - s.y).area(e.x - s.x) == 0
}

fn parse(answer: &str) -> Vec<Piece> {
    let t: Vec<&str> = answer.split_whitespace().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let n = |s: &str| s.parse::<Nm>().unwrap();
    while i < t.len() {
        match t[i] {
            "P" => {
                out.push(Piece::P(Point { x: n(t[i + 1]), y: n(t[i + 2]) }));
                i += 3;
            }
            "A" => {
                let p = |k: usize| Point { x: n(t[i + k]), y: n(t[i + k + 1]) };
                if flat_on_the_grid(p(1), p(3), p(5)) {
                    out.push(Piece::P(p(1)));
                    out.push(Piece::P(p(5)));
                } else {
                    out.push(Piece::A(p(1), p(3), p(5)));
                }
                i += 7;
            }
            other => panic!("unexpected token {other}"),
        }
    }
    // KiCad's nanometres rounded to micrometres can make two pieces meet in one point (a straight leg of a few nanometres before an arc): the
    // point that repeats the piece before it, or the start of the arc after it, is one point, as the port's line has it
    let mut merged: Vec<Piece> = Vec::new();
    for (k, piece) in out.iter().enumerate() {
        if let Piece::P(p) = piece {
            let prev_end = match merged.last() {
                Some(Piece::P(q)) => Some(*q),
                Some(Piece::A(_, _, e)) => Some(*e),
                None => None,
            };
            let next_start = match out.get(k + 1) {
                Some(Piece::A(s, _, _)) => Some(*s),
                _ => None,
            };
            if prev_end == Some(*p) || next_start == Some(*p) {
                continue;
            }
        }
        merged.push(piece.clone());
    }
    merged
}

fn port(case: &str) -> Vec<Piece> {
    let v: Vec<i64> = case.split_whitespace().map(|s| s.parse().unwrap()).collect();
    let dir = if v[0] < 0 { Direction45::Undefined } else { DIRS[v[0] as usize] };
    let p = |x: i64, y: i64| Point { x: Nm::new(x), y: Nm::new(y) };
    let trace = dir.build_initial_line(p(v[1], v[2]), p(v[3], v[4]), v[5] != 0, MODES[v[6] as usize]);
    let line = eda_pns::line::Line { arcs: trace.arcs.clone(), ..eda_pns::line::Line::from_points(None, 0, Nm::ZERO, trace.pts.clone()) };
    let runs = line.arc_runs();
    let mut out: Vec<Piece> = Vec::new();
    let mut i = 0;
    while i < line.pts.len() {
        if let Some(r) = runs.iter().find(|r| r.first == i) {
            let a = if r.reversed { line.arcs[r.arc].reversed() } else { line.arcs[r.arc] };
            out.push(Piece::A(a.start, a.mid, a.end));
            i = r.last + 1;
            continue;
        }
        out.push(Piece::P(line.pts[i]));
        i += 1;
    }
    out
}

/// The two answers agree: the same pieces, exactly.
fn agree(port: &[Piece], kicad: &[Piece]) -> bool {
    port.len() == kicad.len()
        && port.iter().zip(kicad).all(|(a, b)| match (a, b) {
            (Piece::P(p), Piece::P(q)) => p == q,
            (Piece::A(s, m, e), Piece::A(s2, m2, e2)) => s == s2 && e == e2 && m == m2,
            _ => false,
        })
}

fn case(r: &mut Rng) -> String {
    let dir = r.below(9) - 1;
    // micrometre points, mostly a few mm apart, sometimes on a diagonal or an axis
    let span = [500, 5_000, 40_000][r.below(3) as usize];
    let (x0, y0) = (r.below(2 * span) - span, r.below(2 * span) - span);
    let (mut dx, mut dy) = (r.below(2 * span) - span, r.below(2 * span) - span);
    match r.below(8) {
        0 => dy = dx * if r.below(2) == 0 { 1 } else { -1 },
        1 => dx = 0,
        2 => dy = dx / 2 + r.below(3) - 1,
        _ => {}
    }
    if dx == 0 && dy == 0 {
        dx = 1;
    }
    format!("{dir} {} {} {} {} {} {}", x0 * 1000, y0 * 1000, (x0 + dx) * 1000, (y0 + dy) * 1000, r.below(2), r.below(4))
}

fn ask_oracle(oracle: &str, inputs: &[String]) -> Vec<String> {
    let mut child = Command::new(oracle).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("the oracle starts");
    let text: String = inputs.iter().map(|i| format!("{i}\n")).collect();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(text.as_bytes()).unwrap());
    let out = child.wait_with_output().expect("the oracle answers");
    writer.join().unwrap();
    assert!(out.status.success(), "the oracle failed: {:?}", out.status);
    String::from_utf8_lossy(&out.stdout).lines().map(String::from).collect()
}

fn oracle() -> Option<String> {
    std::env::var("EDA_PNS_ORACLE_DIR").ok().filter(|p| std::path::Path::new(p).exists())
}

#[test]
fn random_traces_agree_with_kicad() {
    let Some(oracle) = oracle() else {
        eprintln!("EDA_PNS_ORACLE_DIR is not set: skipped");
        return;
    };
    let n = std::env::var("EDA_PNS_ORACLE_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(20_000);
    let mut r = Rng(0x2545F4914F6CDD1D);
    let inputs: Vec<String> = (0..n).map(|_| case(&mut r)).collect();
    let answers = ask_oracle(&oracle, &inputs);
    assert_eq!(answers.len(), inputs.len());
    let mut bad = Vec::new();
    let mut arcs = 0;
    for (input, got) in inputs.iter().zip(&answers) {
        let (p, k) = (port(input), parse(got));
        arcs += k.iter().filter(|x| matches!(x, Piece::A(..))).count();
        if !agree(&p, &k) {
            bad.push(format!("{input}\n  port  {p:?}\n  kicad {k:?}"));
        }
    }
    eprintln!("{} cases, {arcs} arcs in KiCad's answers, {} disagree", inputs.len(), bad.len());
    assert!(bad.is_empty(), "{} of {} disagree; first:\n{}", bad.len(), inputs.len(), bad[0]);
}

#[test]
fn kicad_answers_baked_in() {
    let text = include_str!("data/direction45_kicad_cases.txt");
    let mut n = 0;
    for pair in text.lines().filter(|l| !l.starts_with('#')).collect::<Vec<_>>().chunks(2) {
        let (input, answer) = (pair[0], pair[1]);
        let (p, k) = (port(input), parse(answer));
        assert!(agree(&p, &k), "{input}\n  port  {p:?}\n  kicad {k:?}");
        n += 1;
    }
    assert!(n >= 100, "{n} baked cases");
}

/// Writes the baked cases: `EDA_PNS_ORACLE_DIR=... EDA_PNS_BAKE=path cargo test -p eda-pns --test direction45_oracle bake -- --ignored`.
#[test]
#[ignore]
fn bake() {
    let (Some(oracle), Ok(path)) = (oracle(), std::env::var("EDA_PNS_BAKE")) else { return };
    let mut r = Rng(0x9E3779B97F4A7C15);
    let inputs: Vec<String> = (0..160).map(|_| case(&mut r)).collect();
    let answers = ask_oracle(&oracle, &inputs);
    let mut out = String::from("# DIRECTION_45::BuildInitialTrace: a case (dir p0 p1 start_diagonal mode, nm), then KiCad's answer (crates/pns/oracle/build_direction.sh)\n");
    for (i, a) in inputs.iter().zip(&answers) {
        out += &format!("{i}\n{a}\n");
    }
    std::fs::write(path, out).unwrap();
}
