//! The meander port against KiCad's own code.
//!
//! `crates/pns/oracle/` compiles `pcbnew/router/pns_meander.cpp` and the tuning functions of `pns_meander_placer_base.cpp` verbatim over the real
//! `libs/kimath` (`SHAPE_ARC`, `SHAPE_LINE_CHAIN`, `SEG`, `VECTOR2`, `EDA_ANGLE`) into a program that answers one line per case. Each case here is
//! a line of text (the generators below); [`evaluate`] runs it through this crate's port and renders the answer in the oracle's format.
//!
//! * `kicad_answers_baked_in`: a few dozen cases with the answers the oracle gave (`tests/data/meander_kicad_cases.txt`), always run.
//! * the four `*_agree_with_*` tests: random cases, port against oracle, line by line. They need the oracle binary:
//!   `EDA_PNS_ORACLE=/path/to/oracle cargo test -p eda-pns --test meander_oracle` (skipped without it). `EDA_PNS_ORACLE_CASES` scales the
//!   number of cases (default 400, times 4 for arcs and chains, 10 for shapes, 2 for lines; 100 000 cases ran when this was written).
//!   `KICAD_SRC_DIR=<checkout of KiCad 8303b2ad> crates/pns/oracle/build.sh` builds it (a minute, clang++, no downloads).

use eda_model::units::Nm;
use eda_model::ir::Point;
use eda_pns::chain::ShapeLineChain;
use eda_pns::dp_meander_placer::coupled_segment_pairs;
use eda_pns::kigeom::{Seg, ShapeArc, ARC_POLYGONIZATION_MAX_ERROR_NM};
use eda_pns::meander::{tune_line_length, MeanderEnv, MeanderSettings, MeanderShape, MeanderSide, MeanderStyle, MeanderType, MeanderedLine};
use std::io::Write;
use std::process::{Command, Stdio};

// ---------------------------------------------------------------------------------------------------------------------------- rendering

fn render_chain(tag: &str, c: &ShapeLineChain) -> String {
    let mut s = format!(" {tag} {}", c.point_count());
    for (i, q) in c.points().iter().enumerate() {
        let (a, b) = c.shapes()[i];
        s += &format!(" {},{},{},{}", q.x, q.y, a, b);
    }
    s += &format!(" arcs {}", c.arcs().len());
    for a in c.arcs() {
        s += &format!(" {},{},{},{},{},{}", a.start.x, a.start.y, a.mid.x, a.mid.y, a.end.x, a.end.y);
    }
    s + &format!(" len {}", c.length())
}

fn p(x: i64, y: i64) -> Point {
    Point { x: Nm::new(x), y: Nm::new(y) }
}

/// `a` moved by `(dx, dy)` nanometres (the generators draw counts).
fn off(a: Point, dx: i64, dy: i64) -> Point {
    Point { x: a.x + Nm::new(dx), y: a.y + Nm::new(dy) }
}

#[derive(Clone)]
struct Circle {
    cx: Nm,
    cy: Nm,
    r: Nm,
}

fn hits(c: &ShapeLineChain, obstacles: &[Circle]) -> bool {
    c.points().iter().any(|q| obstacles.iter().any(|o| (q.x - o.cx).area(q.x - o.cx) + (q.y - o.cy).area(q.y - o.cy) < o.r.area(o.r)))
}

/// The stand-in for `CheckFit` the oracle repeats: no vertex of a line inside an obstacle circle, then `CheckSelfIntersections` for a line.
struct Env {
    settings: MeanderSettings,
    clearance: Nm,
    obstacles: Vec<Circle>,
    line: bool,
    /// `DP_MEANDER_PLACER::CheckFit` keeps four widths from the earlier shapes, not `width + spacing`.
    dual_fit: bool,
}

impl MeanderEnv for Env {
    fn settings(&self) -> &MeanderSettings {
        &self.settings
    }
    fn update_settings(&mut self, s: MeanderSettings) {
        self.settings = s;
    }
    fn clearance(&self) -> Nm {
        self.clearance
    }
    fn check_fit(&mut self, shape: &MeanderShape, existing: &[MeanderShape]) -> bool {
        if hits(shape.c_line(0), &self.obstacles) || (shape.is_dual() && hits(shape.c_line(1), &self.obstacles)) {
            return false;
        }
        if !self.line {
            return true;
        }
        let w = shape.width();
        MeanderedLine::check_self_intersections(existing, shape, if self.dual_fit { w + w * 3 } else { w + self.settings.spacing })
    }
}

fn code(t: MeanderType) -> i32 {
    match t {
        MeanderType::Single => 0,
        MeanderType::Start => 1,
        MeanderType::Finish => 2,
        MeanderType::Turn => 3,
        MeanderType::CheckStart => 4,
        MeanderType::CheckFinish => 5,
        MeanderType::Corner => 6,
        MeanderType::Arc => 7,
        MeanderType::Empty => 8,
    }
}

fn type_of(t: i64) -> MeanderType {
    match t {
        0 => MeanderType::Single,
        1 => MeanderType::Start,
        2 => MeanderType::Finish,
        3 => MeanderType::Turn,
        4 => MeanderType::CheckStart,
        5 => MeanderType::CheckFinish,
        _ => MeanderType::Empty,
    }
}

// ---------------------------------------------------------------------------------------------------------------------------- the port's answer

/// The numbers of a case line after its first word.
struct Nums(std::vec::IntoIter<i64>);

impl Nums {
    fn next(&mut self) -> i64 {
        self.0.next().expect("a number")
    }
    /// The next number, a length.
    fn len(&mut self) -> Nm {
        Nm::new(self.next())
    }
    fn point(&mut self) -> Point {
        let x = self.next();
        p(x, self.next())
    }
    fn obstacles(&mut self) -> Vec<Circle> {
        let n = self.next();
        (0..n)
            .map(|_| {
                let cx = self.len();
                let cy = self.len();
                Circle { cx, cy, r: self.len() }
            })
            .collect()
    }
}

#[allow(clippy::too_many_arguments)]
fn settings_from(min: Nm, max: Nm, spacing: Nm, step: Nm, style: i64, radius: i64, single: i64, initial: i64) -> MeanderSettings {
    let mut s = MeanderSettings::default();
    s.min_amplitude = min;
    s.max_amplitude = max;
    s.spacing = spacing;
    s.step = step;
    s.corner_style = if style == 0 { MeanderStyle::Chamfer } else { MeanderStyle::Round };
    s.corner_radius_percentage = radius;
    s.single_sided = single != 0;
    s.initial_side = match initial {
        -1 => MeanderSide::Left,
        1 => MeanderSide::Right,
        _ => MeanderSide::Default,
    };
    s
}

/// The answer this crate's port gives to a case line, in the oracle's format.
fn evaluate(input: &str) -> String {
    let mut words = input.split_whitespace();
    let cmd = words.next().expect("a command");
    let mut n = Nums(words.map(|w| w.parse::<i64>().expect("an integer")).collect::<Vec<_>>().into_iter());
    match cmd {
        "ARC" => {
            let (s, e) = (n.point(), n.point());
            let millideg = n.next();
            let arc = ShapeArc::from_start_end_angle(s, e, millideg as f64 / 1000.0, Nm::ZERO);
            let mut pl = ShapeLineChain::new();
            for q in arc.convert_to_polyline(ARC_POLYGONIZATION_MAX_ERROR_NM) {
                pl.append_point(q);
            }
            format!("ARC mid {},{} center {},{} r {:.6} len {:.6} ccw {}{}", arc.mid.x, arc.mid.y, arc.center().x, arc.center().y, arc.radius(), arc.length(), arc.is_ccw() as i32, render_chain("poly", &pl))
        }
        "CHAIN" => {
            let count = n.next() as usize;
            let pts: Vec<Point> = (0..count).map(|_| n.point()).collect();
            let (a, b) = (n.point(), n.point());
            let c = ShapeLineChain::from_points(&pts);
            let mut simp = c.clone();
            simp.simplify();
            let (pre, mid, post) = c.split3(a, b);
            let np = c.nearest_point(a, false);
            let mut out = format!("CHAIN len {}", c.length());
            out += &render_chain("simp", &simp);
            out += &render_chain("pre", &pre);
            out += &render_chain("mid", &mid);
            out += &render_chain("post", &post);
            out + &format!(" nearest {},{}", np.x, np.y)
        }
        "SHAPE" => {
            let ty = n.next();
            let base = Seg::new(n.point(), n.point());
            let pp = n.point();
            let side = n.next() != 0;
            let (width, clearance, dual, offset) = (n.len(), n.len(), n.next() != 0, n.len());
            let (min, max, spacing, step, style, radius, single) = (n.len(), n.len(), n.len(), n.len(), n.next(), n.next(), n.next());
            let (tbl, amp) = (n.len(), n.len());
            let obstacles = n.obstacles();
            let mut env = Env { settings: settings_from(min, max, spacing, step, style, radius, single, 0), clearance, obstacles, line: false, dual_fit: false };
            let mut m = MeanderShape::new(&env, width, dual);
            m.set_baseline_offset(offset);
            m.set_target_baseline_length(tbl);
            let ok = m.fit(&mut env, &[], type_of(ty), base, pp, side);
            if ok && amp > Nm::ZERO {
                m.resize(amp);
            }
            let mut out = format!("SHAPE ok {} amp {} end {},{} base {} mincr {} minamp {} tunable {}", ok as i32, m.amplitude(), m.end().x, m.end().y, m.baseline_length(), 0, m.min_amplitude(), if ok { m.min_tunable_length() } else { Nm::ZERO });
            if ok {
                out += &render_chain("l0", m.c_line(0));
                if dual {
                    out += &render_chain("l1", m.c_line(1));
                }
            }
            out
        }
        "LINE" => {
            let base = Seg::new(n.point(), n.point());
            let side = n.next() != 0;
            let (width, clearance) = (n.len(), n.len());
            let (min, max, spacing, step, style, radius, single, initial) = (n.len(), n.len(), n.len(), n.len(), n.next(), n.next(), n.next(), n.next());
            let elong = n.len();
            let obstacles = n.obstacles();
            let mut env = Env { settings: settings_from(min, max, spacing, step, style, radius, single, initial), clearance, obstacles, line: true, dual_fit: false };
            let mut result = MeanderedLine::new(false);
            result.set_width(width);
            result.set_baseline_offset(Nm::ZERO);
            result.add_corner(&env, base.a, p(0, 0));
            result.meander_segment(&mut env, base, side, 0);
            result.add_corner(&env, base.b, p(0, 0));
            tune_line_length(&mut result, elong);
            let (mut kinds, mut amps) = (String::new(), String::new());
            let mut tuned = ShapeLineChain::new();
            for m in result.meanders() {
                kinds += &format!("{},", code(m.kind()));
                amps += &format!("{},", m.amplitude());
                if m.kind() != MeanderType::Empty {
                    tuned.append_chain(m.c_line(0));
                }
            }
            let mut simple = tuned.clone();
            simple.simplify();
            let mut out = format!("LINE n {} kinds {kinds} amps {amps} side {}", result.meanders().len(), env.settings.initial_side.value());
            out += &render_chain("tuned", &tuned);
            out + &render_chain("simp", &simple)
        }
        "COUPLED" => {
            let np = n.next() as usize;
            let pp: Vec<Point> = (0..np).map(|_| n.point()).collect();
            let nn = n.next() as usize;
            let nt: Vec<Point> = (0..nn).map(|_| n.point()).collect();
            let (width, gap) = (n.len(), n.len());
            let pairs = coupled_segment_pairs(&ShapeLineChain::from_points(&pp), &ShapeLineChain::from_points(&nt), width, gap);
            let mut out = format!("COUPLED n {}", pairs.len());
            for c in &pairs {
                out += &format!(" p {},{},{},{} n {},{},{},{} i {},{}", c.coupled_p.a.x, c.coupled_p.a.y, c.coupled_p.b.x, c.coupled_p.b.y, c.coupled_n.a.x, c.coupled_n.a.y, c.coupled_n.b.x, c.coupled_n.b.y, c.index_p, c.index_n);
            }
            out
        }
        "DLINE" => {
            let base = Seg::new(n.point(), n.point());
            let side = n.next() != 0;
            let (width, clearance) = (n.len(), n.len());
            let (min, max, spacing, step, style, radius, single, initial) = (n.len(), n.len(), n.len(), n.len(), n.next(), n.next(), n.next(), n.next());
            let elong = n.len();
            let offset = n.len();
            let obstacles = n.obstacles();
            let mut env = Env { settings: settings_from(min, max, spacing, step, style, radius, single, initial), clearance, obstacles, line: true, dual_fit: true };
            let mut result = MeanderedLine::new(true);
            result.set_width(width);
            result.set_baseline_offset(offset);
            result.add_corner(&env, base.a, base.a);
            result.meander_segment(&mut env, base, side, 0);
            result.add_corner(&env, base.b, base.b);
            tune_line_length(&mut result, elong);
            let (mut kinds, mut amps) = (String::new(), String::new());
            let (mut tuned_p, mut tuned_n) = (ShapeLineChain::new(), ShapeLineChain::new());
            for m in result.meanders() {
                kinds += &format!("{},", code(m.kind()));
                amps += &format!("{},", m.amplitude());
                if m.kind() != MeanderType::Empty {
                    tuned_p.append_chain(m.c_line(0));
                    tuned_n.append_chain(m.c_line(1));
                }
            }
            let mut out = format!("DLINE n {} kinds {kinds} amps {amps} side {}", result.meanders().len(), env.settings.initial_side.value());
            out += &render_chain("p", &tuned_p);
            out + &render_chain("n", &tuned_n)
        }
        other => panic!("unknown case {other}"),
    }
}

// ---------------------------------------------------------------------------------------------------------------------------- random cases

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
    fn pick<T: Copy>(&mut self, v: &[T]) -> T {
        v[self.range(0, v.len() as i64 - 1) as usize]
    }
}

fn base_segment(r: &mut Rng) -> Seg {
    let a = p(r.range(-20_000_000, 20_000_000), r.range(-20_000_000, 20_000_000));
    let len = r.range(3_000_000, 40_000_000);
    let (dx, dy) = match r.range(0, 5) {
        0 => (len, 0),
        1 => (0, -len),
        2 => (-len, 0),
        3 => (0, len),
        4 => {
            let d = (len as f64 / std::f64::consts::SQRT_2) as i64;
            (r.pick(&[-1, 1]) * d, r.pick(&[-1, 1]) * d)
        }
        _ => {
            let ang = r.range(0, 359_999) as f64 / 1000.0 * std::f64::consts::PI / 180.0;
            ((len as f64 * ang.cos()) as i64, (len as f64 * ang.sin()) as i64)
        }
    };
    Seg::new(a, off(a, dx, dy))
}

/// "min max spacing step style radius single" and the initial side.
fn settings_text(r: &mut Rng) -> (String, i64) {
    let min = r.pick(&[100_000, 200_000, 300_000]);
    let max = r.range(150_000, 1_500_000);
    let spacing = r.range(300_000, 1_500_000);
    let step = r.pick(&[25_000, 50_000, 100_000]);
    let style = if r.chance(70) { 1 } else { 0 };
    let radius = r.pick(&[0, 40, 80, 100]);
    let single = r.chance(30) as i32;
    let initial = r.pick(&[-1, 0, 1]);
    (format!("{min} {max} {spacing} {step} {style} {radius} {single}"), initial)
}

fn obstacle_text(r: &mut Rng, around: &Seg, n: i64) -> String {
    let mut s = format!(" {n}");
    for _ in 0..n {
        let t = r.range(0, 100);
        let (x, y) = (around.a.x + (around.b.x - around.a.x) * t / 100, around.a.y + (around.b.y - around.a.y) * t / 100);
        let (dx, dy, rad) = (r.range(-1_500_000, 1_500_000), r.range(-1_500_000, 1_500_000), r.range(300_000, 3_000_000));
        s += &format!(" {} {} {}", x + Nm::new(dx), y + Nm::new(dy), rad);
    }
    s
}

fn arc_case(r: &mut Rng) -> String {
    let s = p(r.range(-50_000_000, 50_000_000), r.range(-50_000_000, 50_000_000));
    let chord = r.range(100_000, 5_000_000);
    let ang = r.range(0, 359_999) as f64 * std::f64::consts::PI / 180_000.0;
    let e = off(s, (chord as f64 * ang.cos()) as i64, (chord as f64 * ang.sin()) as i64);
    let (rnd1, rnd2) = (r.range(-179_000, 179_000), r.range(-179_000, 179_000));
    let millideg = r.pick(&[-90_000i64, 90_000, -90_000, 90_000, rnd1, rnd2]);
    let millideg = if millideg == 0 { 45_000 } else { millideg };
    format!("ARC {} {} {} {} {}", s.x, s.y, e.x, e.y, millideg)
}

fn chain_case(r: &mut Rng) -> String {
    let n = r.range(2, 7) as usize;
    let mut pts = vec![p(r.range(-5_000_000, 5_000_000), r.range(-5_000_000, 5_000_000))];
    while pts.len() < n {
        let last = *pts.last().unwrap();
        let len = r.range(200_000, 6_000_000);
        let (dx, dy) = match r.range(0, 3) {
            0 => (r.pick(&[-1, 1]) * len, 0),
            1 => (0, r.pick(&[-1, 1]) * len),
            2 => (r.pick(&[-1, 1]) * len, r.pick(&[-1, 1]) * len),
            _ => (r.range(-6_000_000, 6_000_000), r.range(-6_000_000, 6_000_000)),
        };
        let q = off(last, dx, dy);
        if q != last {
            pts.push(q);
        }
        // sometimes a point in the middle of the run it just made (collinear)
        if r.chance(25) && pts.len() < n {
            let q2 = Point { x: (last.x + q.x) / 2, y: (last.y + q.y) / 2 };
            if q2 != last && q2 != q {
                let at = pts.len() - 1;
                pts.insert(at, q2);
            }
        }
    }
    let near = |r: &mut Rng| {
        let i = r.range(0, pts.len() as i64 - 2) as usize;
        let seg = Seg::new(pts[i], pts[i + 1]);
        let t = r.range(0, 100);
        let (jx, jy) = (r.range(-3, 3), r.range(-3, 3));
        off(Point { x: seg.a.x + (seg.b.x - seg.a.x) * t / 100, y: seg.a.y + (seg.b.y - seg.a.y) * t / 100 }, jx, jy)
    };
    let (a, b) = (near(r), near(r));
    let mut input = format!("CHAIN {}", pts.len());
    for q in &pts {
        input += &format!(" {} {}", q.x, q.y);
    }
    input + &format!(" {} {} {} {}", a.x, a.y, b.x, b.y)
}

fn shape_case(r: &mut Rng) -> String {
    let base = base_segment(r);
    let (settings, _) = settings_text(r);
    let (width, clearance) = (r.range(100_000, 500_000), r.range(100_000, 400_000));
    let ty = r.range(0, 6);
    let t = if r.chance(50) { 0 } else { r.range(0, 60) };
    let pp = Point { x: base.a.x + (base.b.x - base.a.x) * t / 100, y: base.a.y + (base.b.y - base.a.y) * t / 100 };
    let side = r.chance(50);
    let dual = r.chance(35);
    let offset = if dual { r.range(100_000, 700_000) * r.pick(&[-1, 1]) } else { 0 };
    let tbl = if r.chance(40) { r.range(500_000, 4_000_000) } else { 0 };
    let amp = if r.chance(40) { r.range(100_000, 1_500_000) } else { 0 };
    let nobs = r.range(0, 2);
    let obstacles = obstacle_text(r, &base, nobs);
    format!("SHAPE {ty} {} {} {} {} {} {} {} {width} {clearance} {} {offset} {settings} {tbl} {amp}{obstacles}", base.a.x, base.a.y, base.b.x, base.b.y, pp.x, pp.y, side as i32, dual as i32)
}

fn line_case(r: &mut Rng) -> String {
    let base = base_segment(r);
    let (settings, initial) = settings_text(r);
    let (width, clearance) = (r.range(100_000, 500_000), r.range(100_000, 400_000));
    let side = r.chance(50);
    let elong = r.range(0, 30_000_000) * r.pick(&[1, 1, 1, -1]) / 3;
    let nobs = r.range(0, 3);
    let obstacles = obstacle_text(r, &base, nobs);
    format!("LINE {} {} {} {} {} {width} {clearance} {settings} {initial} {elong}{obstacles}", base.a.x, base.a.y, base.b.x, base.b.y, side as i32)
}

/// Two lines that run beside each other here and there: the second is the first moved by a vector (and cut or bent now and then), with a gap
/// asked for that is one the lines really have, give or take the tolerance.
fn coupled_case(r: &mut Rng) -> String {
    let n = r.range(2, 6) as usize;
    let mut pts = vec![p(r.range(-5_000_000, 5_000_000), r.range(-5_000_000, 5_000_000))];
    while pts.len() < n {
        let last = *pts.last().unwrap();
        let len = r.range(500_000, 8_000_000);
        let (dx, dy) = match r.range(0, 3) {
            0 => (r.pick(&[-1, 1]) * len, 0),
            1 => (0, r.pick(&[-1, 1]) * len),
            2 => (r.pick(&[-1, 1]) * len, r.pick(&[-1, 1]) * len),
            _ => (r.range(-8_000_000, 8_000_000), r.range(-8_000_000, 8_000_000)),
        };
        let q = off(last, dx, dy);
        if q != last {
            pts.push(q);
        }
    }
    let (tx, ty) = (r.range(-700_000, 700_000), r.range(-700_000, 700_000));
    let mut other: Vec<Point> = pts.iter().map(|&q| off(q, tx, ty)).collect();
    if r.chance(40) {
        let k = r.range(0, other.len() as i64 - 1) as usize;
        other[k] = off(other[k], r.range(-300_000, 300_000), r.range(-300_000, 300_000));
    }
    if r.chance(30) && other.len() > 2 {
        other.pop();
    }
    let width = Nm::new(r.pick(&[100_000, 200_000, 250_000]));
    let i = r.range(0, pts.len() as i64 - 2) as usize;
    let j = r.range(0, other.len() as i64 - 2) as usize;
    let real = ((Seg::new(pts[i], pts[i + 1]).distance_seg(&Seg::new(other[j], other[j + 1]))) - width).abs();
    let gap = real + Nm::new(r.pick(&[0, 0, 0, 5_000, 9_999, 10_001, -9_000, -11_000, 40_000]));
    let mut input = format!("COUPLED {}", pts.len());
    for q in &pts {
        input += &format!(" {} {}", q.x, q.y);
    }
    input += &format!(" {}", other.len());
    for q in &other {
        input += &format!(" {} {}", q.x, q.y);
    }
    input + &format!(" {width} {gap}")
}

fn dline_case(r: &mut Rng) -> String {
    let base = base_segment(r);
    let (settings, initial) = settings_text(r);
    let (width, clearance) = (r.range(100_000, 400_000), r.range(100_000, 400_000));
    let side = r.chance(50);
    let elong = r.range(0, 30_000_000) * r.pick(&[1, 1, 1, -1]) / 3;
    let offset = r.range(100_000, 700_000) * r.pick(&[-1, 1]);
    let nobs = r.range(0, 3);
    let obstacles = obstacle_text(r, &base, nobs);
    format!("DLINE {} {} {} {} {} {width} {clearance} {settings} {initial} {elong} {offset}{obstacles}", base.a.x, base.a.y, base.b.x, base.b.y, side as i32)
}

// ---------------------------------------------------------------------------------------------------------------------------- the tests

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

fn run_kind(name: &str, oracle: &str, inputs: &[String]) -> usize {
    let answers = ask_oracle(oracle, inputs);
    assert_eq!(answers.len(), inputs.len(), "{name}: one answer per case");
    let mut bad = 0;
    let mut first: Option<String> = None;
    let mut ported = Vec::with_capacity(inputs.len());
    for (input, got) in inputs.iter().zip(&answers) {
        let port = evaluate(input);
        if port != *got {
            bad += 1;
            if first.is_none() {
                first = Some(format!("input:    {input}\nport:     {port}\nKiCad:    {got}"));
            }
        }
        ported.push(port);
    }
    // how much of the code the cases reached
    let reached = |needle: &str| ported.iter().filter(|c| c.contains(needle)).count();
    let coverage = match name {
        "DLINE" => format!(" ({} with a start, {} with a turn, {} left straight)", reached("kinds 6,1,"), reached(",3,"), reached("kinds 6,6,")),
        "COUPLED" => format!(" ({} with a coupled pair, {} with two or more)", ported.iter().filter(|c| !c.starts_with("COUPLED n 0")).count(), ported.iter().filter(|c| !c.starts_with("COUPLED n 0") && !c.starts_with("COUPLED n 1")).count()),
        "LINE" => format!(" ({} with a start, {} with a turn, {} left straight, {} with the first side flipped)", reached("kinds 6,6,1,"), reached(",3,"), reached("kinds 6,6,8,") + reached("kinds 6,6,6,6"), reached("side -1") + reached("side 1")),
        "SHAPE" => format!(" ({} fitted, {} dual, {} with arcs)", reached("ok 1"), reached(" l1 "), reached("arcs 4") + reached("arcs 2") + reached("arcs 1")),
        _ => String::new(),
    };
    if let Some(f) = first {
        eprintln!("{name}: {bad} of {} cases differ; the first:\n{f}", inputs.len());
    } else {
        eprintln!("{name}: {} cases agree with KiCad{coverage}", inputs.len());
    }
    bad
}

fn n_cases() -> usize {
    std::env::var("EDA_PNS_ORACLE_CASES").ok().and_then(|s| s.parse().ok()).unwrap_or(400)
}

fn oracle() -> Option<String> {
    std::env::var("EDA_PNS_ORACLE").ok().filter(|p| std::path::Path::new(p).exists())
}

macro_rules! oracle_test {
    ($name:ident, $kind:literal, $gen:ident, $seed:expr, $scale:expr) => {
        #[test]
        fn $name() {
            let Some(oracle) = oracle() else {
                eprintln!("EDA_PNS_ORACLE is not set: skipped");
                return;
            };
            let mut r = Rng($seed);
            let inputs: Vec<String> = (0..n_cases() * $scale).map(|_| $gen(&mut r)).collect();
            assert_eq!(run_kind($kind, &oracle, &inputs), 0, "the port and KiCad's own code differ");
        }
    };
}

oracle_test!(arcs_agree_with_shape_arc, "ARC", arc_case, 0x9E3779B97F4A7C15, 4);
oracle_test!(chains_agree_with_shape_line_chain, "CHAIN", chain_case, 0xD1B54A32D192ED03, 4);
oracle_test!(shapes_agree_with_meander_shape, "SHAPE", shape_case, 0x8CB92BA72F3D8DD7, 10);
oracle_test!(lines_agree_with_meandered_line_and_tune_line_length, "LINE", line_case, 0xA0761D6478BD642F, 2);
oracle_test!(pairs_agree_with_a_dual_meandered_line, "DLINE", dline_case, 0xE7037ED1A0B428DB, 2);
oracle_test!(coupled_segments_agree_with_coupled_segment_pairs, "COUPLED", coupled_case, 0xC2B2AE3D27D4EB4F, 4);

/// The cases KiCad's code was asked, with its answers (`tests/data/meander_kicad_cases.txt`: a case line, its answer line, a blank line).
#[test]
fn kicad_answers_baked_in() {
    let data = include_str!("data/meander_kicad_cases.txt");
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
    assert!(n >= 24, "{n} cases");
}

/// Writes the baked cases: `EDA_PNS_ORACLE=... EDA_PNS_BAKE=path cargo test -p eda-pns --test meander_oracle bake -- --ignored` puts a spread
/// of random cases with the oracle's answers into `path`.
#[test]
#[ignore]
fn bake() {
    let (Some(oracle), Ok(path)) = (oracle(), std::env::var("EDA_PNS_BAKE")) else { return };
    let mut r = Rng(0x1234_5678_9ABC_DEF1);
    let mut inputs: Vec<String> = Vec::new();
    inputs.extend((0..6).map(|_| arc_case(&mut r)));
    inputs.extend((0..8).map(|_| chain_case(&mut r)));
    inputs.extend((0..80).map(|_| shape_case(&mut r)));
    inputs.extend((0..80).map(|_| line_case(&mut r)));
    inputs.extend((0..40).map(|_| dline_case(&mut r)));
    inputs.extend((0..20).map(|_| coupled_case(&mut r)));
    let answers = ask_oracle(&oracle, &inputs);
    let mut kept = String::from("# Cases and the answers pcbnew/router/pns_meander.cpp (KiCad 8303b2ad) gave them, nanometres; see tests/meander_oracle.rs.\n\n");
    let (mut shapes, mut lines, mut arcs, mut chains, mut dlines) = (0, 0, 0, 0, 0);
    let mut coupleds = 0;
    for (i, a) in inputs.iter().zip(&answers) {
        let (count, cap) = match i.split_whitespace().next().unwrap() {
            "ARC" => (&mut arcs, 3),
            "CHAIN" => (&mut chains, 5),
            "SHAPE" => (&mut shapes, 14),
            "DLINE" => (&mut dlines, 5),
            "COUPLED" => (&mut coupleds, 6),
            _ => (&mut lines, 6),
        };
        let limit = if i.starts_with("LINE") || i.starts_with("DLINE") { 6000 } else { 2600 };
        if *count < cap && a.len() < limit && !a.contains("ok 0") {
            kept += &format!("{i}\n{a}\n\n");
            *count += 1;
        }
    }
    std::fs::write(path, kept).unwrap();
}
