//! Port vs binding: the INACTIVE Rust port (`eda_clipper2::port`) and the
//! active C++ binding (`eda_clipper2::cpp`, KiCad's vendored Clipper2 2.0.1)
//! are compared on every operation the workspace calls:
//! `Clipper64::Execute` (paths and tree, every clip type and fill rule, with
//! open subjects and with a Z callback), `ClipperOffset::Execute` (every
//! join and end type), `RectClip64`, `RectClip` and `SimplifyPath`.
//!
//! The two are not the same Clipper2: the port follows an older release
//! (1.x: `ClipperOffset` keeps a hole list and skips holes smaller than the
//! offset, `DoBevel` leaves `z` at 0, a zero-area closed path offsets to
//! nothing), and KiCad's vendored copy is 2.0.1 (`GetLowestClosedPathInfo`
//! orients each group by its lowest path, a zero-area closed path offsets
//! like an open one, `DoBevel` carries `z`). So the test asserts what must
//! agree (the boolean operations on real board polygons, `SimplifyPath`,
//! the success flag) and prints a table of the rest: exact, the same region
//! in another form, or a different region.
//!
//! Inputs: random polygons (small integer grids, so collinear edges,
//! touching vertices and self-intersections are common), and, when
//! `KICAD_QA_DATA` is set, every `(pts ...)` polygon of KiCad's pcbnew QA
//! boards (zone outlines, filled zone polygons, board graphics), in nm.

use eda_clipper2::{cpp, port, ClipType, EndType, FillRule, JoinType, Path64, Paths64, Point64, PolyTree64, Rect64};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

type Key = Vec<Vec<(i64, i64, i64)>>;

fn key(paths: &Paths64) -> Key {
    paths.iter().map(|p| p.iter().map(|q| (q.x, q.y, q.z)).collect()).collect()
}

fn tree_key(t: &PolyTree64) -> Vec<(usize, bool, Vec<(i64, i64, i64)>)> {
    fn walk(t: &PolyTree64, i: usize, depth: usize, out: &mut Vec<(usize, bool, Vec<(i64, i64, i64)>)>) {
        let n = &t.nodes[i];
        out.push((depth, n.is_hole(), n.polygon.iter().map(|q| (q.x, q.y, q.z)).collect()));
        for &c in &n.children {
            walk(t, c, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    for &r in &t.roots {
        walk(t, r, 0, &mut out);
    }
    out
}

/// Every `(pts (xy x y) ...)` polygon of KiCad's pcbnew QA boards, in nm.
fn qa_polygons() -> Vec<Path64> {
    let Ok(dir) = std::env::var("KICAD_QA_DATA") else { return Vec::new() };
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(std::path::Path::new(&dir).join("pcbnew")) else { return out };
    let mut files: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "kicad_pcb")).collect();
    files.sort();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let mut rest = text.as_str();
        while let Some(i) = rest.find("(pts") {
            rest = &rest[i + 4..];
            let mut depth = 1;
            let mut end = 0;
            for (j, ch) in rest.char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = j;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let body = &rest[..end];
            if body.contains("(arc") {
                continue;
            }
            let mut path = Vec::new();
            for xy in body.split("(xy").skip(1) {
                let nums: Vec<f64> = xy.split(|c: char| c.is_whitespace() || c == ')').filter_map(|s| s.parse().ok()).take(2).collect();
                if nums.len() == 2 {
                    path.push(Point64::new((nums[0] * 1e6).round() as i64, (nums[1] * 1e6).round() as i64));
                }
            }
            if path.len() >= 3 {
                out.push(path);
            }
        }
    }
    out
}

fn random_polygon(rng: &mut Rng, grid: i64, scale: i64) -> Path64 {
    let n = rng.range(3, 12);
    (0..n).map(|_| Point64::new(rng.range(0, grid) * scale, rng.range(0, grid) * scale)).collect()
}

fn with_z(paths: &Paths64, base: i64) -> Paths64 {
    paths.iter().enumerate().map(|(i, p)| p.iter().enumerate().map(|(j, q)| Point64::with_z(q.x, q.y, base + (i as i64) * 1000 + j as i64)).collect()).collect()
}

fn translate(p: &Path64, dx: i64, dy: i64) -> Path64 {
    p.iter().map(|q| Point64::with_z(q.x + dx, q.y + dy, q.z)).collect()
}

fn bounds(paths: &[Path64]) -> Rect64 {
    eda_clipper2::get_bounds_paths(paths)
}

const CLIP_TYPES: [ClipType; 4] = [ClipType::Intersection, ClipType::Union, ClipType::Difference, ClipType::Xor];
const FILL_RULES: [FillRule; 4] = [FillRule::EvenOdd, FillRule::NonZero, FillRule::Positive, FillRule::Negative];
const JOINS: [JoinType; 4] = [JoinType::Square, JoinType::Bevel, JoinType::Round, JoinType::Miter];
const ENDS: [EndType; 5] = [EndType::Polygon, EndType::Joined, EndType::Butt, EndType::Square, EndType::Round];

/// The region a set of closed paths covers under `NonZero` (Clipper's own
/// outputs: outer rings one way, holes the other) is the same for `a` and
/// `b` when their XOR (computed by the C++ engine) has no area.
fn same_region(a: &Paths64, b: &Paths64) -> bool {
    let x = eda_clipper2::boolean_op(ClipType::Xor, FillRule::NonZero, a, b);
    eda_clipper2::area_paths(&x).abs() < 1e-9
}

/// What each operation's comparisons came to: `exact` (the same points in
/// the same order, z included), `region` (not exact, but the closed paths
/// cover the same region), `open` (the closed region is the same, the open
/// paths of a clip with open subjects differ), and `differ` (the region
/// differs: listed with the inputs).
#[derive(Default)]
struct Tally {
    by_op: std::collections::BTreeMap<String, [usize; 4]>,
    prefix: String,
    diffs: Vec<String>,
}

impl Tally {
    fn add(&mut self, op: &str, class: usize, detail: impl FnOnce() -> String) {
        self.by_op.entry(format!("{}{op}", self.prefix)).or_default()[class] += 1;
        if class == 3 && self.diffs.len() < 10 {
            self.diffs.push(detail());
        }
    }
    /// Closed results (and, for a clip, the open ones beside them).
    fn closed(&mut self, op: &str, what: &str, port: (&Paths64, &Paths64), cpp: (&Paths64, &Paths64)) {
        let class = if key(port.0) == key(cpp.0) && key(port.1) == key(cpp.1) {
            0
        } else if !same_region(port.0, cpp.0) {
            3
        } else if key(port.1) != key(cpp.1) {
            2
        } else {
            1
        };
        self.add(op, class, || format!("{what}\n  port {:?}\n  cpp  {:?}", key(port.0), key(cpp.0)));
    }
    fn exact<T: PartialEq + std::fmt::Debug>(&mut self, op: &str, what: &str, port: T, cpp: T) {
        let class = if port == cpp { 0 } else { 3 };
        self.add(op, class, || format!("{what}\n  port {port:?}\n  cpp  {cpp:?}"));
    }
    /// Prints the table, and fails when an operation named in `exact` is not exact every time.
    fn report(&self, what: &str, exact: &[&str]) {
        eprintln!("{what}: op: exact / same region / same region, open paths differ / region differs");
        for (op, n) in &self.by_op {
            eprintln!("  {op}: {} / {} / {} / {}", n[0], n[1], n[2], n[3]);
        }
        for (op, n) in &self.by_op {
            let name = op.split_once(": ").map_or(op.as_str(), |(_, o)| o);
            if exact.contains(&name) {
                assert_eq!(n[1] + n[2] + n[3], 0, "{what}: {op} is not exact every time: {n:?}");
            }
        }
        if !self.diffs.is_empty() {
            eprintln!("first regions that differ:\n{}", self.diffs.join("\n"));
        }
    }}

fn trace(s: &str) {
    if std::env::var_os("DIFF_TRACE").is_some() {
        eprintln!("  {s}");
    }
}

fn tree_paths(t: &PolyTree64) -> Paths64 {
    eda_clipper2::poly_tree_to_paths64(t)
}

fn boolean_case(t: &mut Tally, label: &str, subj: &Paths64, clip: &Paths64, open: &Paths64, ct: ClipType, fr: FillRule, z: bool) {
    let z_cb = |e1b: Point64, e1t: Point64, e2b: Point64, e2t: Point64, pt: &mut Point64| {
        pt.z = e1b.z.wrapping_mul(31).wrapping_add(e1t.z).wrapping_mul(31).wrapping_add(e2b.z).wrapping_mul(31).wrapping_add(e2t.z);
    };
    let mut p = port::Clipper64::new();
    let mut c = cpp::Clipper64::new();
    if z {
        p.set_z_callback(Box::new(z_cb));
        c.set_z_callback(Box::new(z_cb));
        p.default_z = -7;
        c.default_z = -7;
    }
    p.add_subject(subj);
    c.add_subject(subj);
    if !open.is_empty() {
        p.add_open_subject(open);
        c.add_open_subject(open);
    }
    p.add_clip(clip);
    c.add_clip(clip);
    let what = || format!("{label} {ct:?} {fr:?} z={z}\n  subject {:?}\n  clip {:?}\n  open {:?}", key(subj), key(clip), key(open));
    let (mut pc, mut po, mut cc, mut co) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    trace(&format!("port execute {ct:?} {fr:?}"));
    let pok = p.execute(ct, fr, &mut pc, &mut po);
    trace("cpp execute");
    let cok = c.execute(ct, fr, &mut cc, &mut co);
    t.exact("clip: success", &what(), pok, cok);
    t.closed(if open.is_empty() { "clip" } else { "clip with open subjects" }, &what(), (&pc, &po), (&cc, &co));
    // Execute again on the same objects (the binding replays into a fresh
    // C++ object, the port reuses its state), into a PolyTree64.
    let (mut pt, mut ct_) = (PolyTree64::new(), PolyTree64::new());
    trace("port execute_tree");
    let pok = p.execute_tree(ct, fr, &mut pt, &mut po);
    trace("cpp execute_tree");
    let cok = c.execute_tree(ct, fr, &mut ct_, &mut co);
    t.exact("clip: success", &what(), pok, cok);
    let op = if open.is_empty() { "clip to tree" } else { "clip to tree with open subjects" };
    if tree_key(&pt) == tree_key(&ct_) && key(&po) == key(&co) {
        t.add(op, 0, String::new);
    } else {
        t.closed(op, &what(), (&tree_paths(&pt), &po), (&tree_paths(&ct_), &co));
    }
}

#[allow(clippy::too_many_arguments)]
fn offset_case(t: &mut Tally, label: &str, paths: &Paths64, delta: f64, jt: JoinType, et: EndType, miter: f64, arc_tol: f64, tree: bool) {
    let mut p = port::ClipperOffset::new(miter, arc_tol, false, false);
    let mut c = cpp::ClipperOffset::new(miter, arc_tol, false, false);
    p.add_paths(paths, jt, et);
    c.add_paths(paths, jt, et);
    trace(&format!("offset {delta} {jt:?} {et:?} tree={tree} sizes {:?}", paths.iter().map(|p| p.len()).collect::<Vec<_>>()));
    let what = format!("{label} offset {delta} {jt:?} {et:?} miter {miter} arc {arc_tol}\n  paths {:?}", key(paths));
    let none = Vec::new();
    if tree {
        let (mut a, mut b) = (PolyTree64::new(), PolyTree64::new());
        p.execute_tree(delta, &mut a);
        c.execute_tree(delta, &mut b);
        if tree_key(&a) == tree_key(&b) {
            t.add("offset to tree", 0, String::new);
        } else {
            t.closed("offset to tree", &what, (&tree_paths(&a), &none), (&tree_paths(&b), &none));
        }
    } else {
        let (mut a, mut b) = (Vec::new(), Vec::new());
        p.execute(delta, &mut a);
        c.execute(delta, &mut b);
        t.closed("offset", &what, (&a, &none), (&b, &none));
    }
}

fn run_cases(t: &mut Tally, rng: &mut Rng, label: &str, pick: &mut dyn FnMut(&mut Rng) -> Path64, iters: usize, max_delta: Option<f64>) {
    let start = std::time::Instant::now();
    t.prefix = format!("{label}: ");
    for it in 0..iters {
        if std::env::var_os("DIFF_TRACE").is_some() {
            eprintln!("{label}#{it} {:?}", start.elapsed());
        }
        let ns = rng.range(1, 3) as usize;
        let subj: Paths64 = (0..ns).map(|_| pick(rng)).collect();
        let sb = bounds(&subj);
        let nc = rng.range(1, 3) as usize;
        let clip: Paths64 = (0..nc)
            .map(|_| {
                let p = pick(rng);
                let b = bounds(std::slice::from_ref(&p));
                // Move the clip's centre to a random point of the subject's bounds.
                let tx = sb.left + (rng.unit() * sb.width() as f64) as i64 - (b.left + b.width() / 2);
                let ty = sb.top + (rng.unit() * sb.height() as f64) as i64 - (b.top + b.height() / 2);
                translate(&p, tx, ty)
            })
            .collect();
        let open: Paths64 = if rng.below(3) == 0 { vec![pick(rng)] } else { Vec::new() };
        let z = rng.below(2) == 0;
        let (subj, clip, open) = if z { (with_z(&subj, 1), with_z(&clip, 1_000_000), with_z(&open, 2_000_000)) } else { (subj, clip, open) };
        let ct = CLIP_TYPES[rng.below(4) as usize];
        let fr = FILL_RULES[rng.below(4) as usize];
        boolean_case(t, &format!("{label}#{it}"), &subj, &clip, &open, ct, fr, z);

        let span = sb.width().max(sb.height()).max(1) as f64;
        // Up to a fifth of the size of the subject, or (on the QA boards) up to `max_delta` nm, the size of a clearance.
        let delta = (rng.unit() - 0.4) * max_delta.unwrap_or(span * 0.2);
        let jt = JOINS[rng.below(4) as usize];
        let et = ENDS[rng.below(5) as usize];
        let miter = [2.0, 3.0, 1.5][rng.below(3) as usize];
        // KiCad's inflate2: arc tolerance = |delta| * (1 - cos(pi / segments)).
        let arc_tol = delta.abs() * (1.0 - (std::f64::consts::PI / [8.0, 16.0, 32.0][rng.below(3) as usize]).cos());
        offset_case(t, &format!("{label}#{it}"), &subj, delta, jt, et, miter, arc_tol, rng.below(2) == 0);

        let rect = Rect64::new(
            sb.left + (rng.unit() * sb.width() as f64) as i64 / 2,
            sb.top + (rng.unit() * sb.height() as f64) as i64 / 2,
            sb.right - (rng.unit() * sb.width() as f64) as i64 / 2,
            sb.bottom - (rng.unit() * sb.height() as f64) as i64 / 2,
        );
        let all: Paths64 = subj.iter().chain(&clip).cloned().collect();
        let what = format!("{label}#{it} rect {rect:?}\n  paths {:?}", key(&all));
        t.exact("rect_clip", &what, key(&port::rect_clip(rect, &all)), key(&cpp::rect_clip(rect, &all)));
        t.exact("RectClip64", &what, key(&port::RectClip64::new(rect).execute(&all)), key(&cpp::RectClip64::new(rect).execute(&all)));
        let eps = rng.unit() * span * 0.05;
        let closed = rng.below(2) == 0;
        for p in &subj {
            t.exact("simplify_path", &format!("{label}#{it} simplify {eps} {closed} {:?}", key(&vec![p.clone()])), key(&vec![port::simplify_path(p, eps, closed)]), key(&vec![cpp::simplify_path(p, eps, closed)]));
        }
    }
}

#[test]
fn port_matches_binding_on_random_grid_polygons() {
    let mut t = Tally::default();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    run_cases(&mut t, &mut rng, "grid8", &mut |r| random_polygon(r, 8, 1), 3000, None);
    run_cases(&mut t, &mut rng, "grid100", &mut |r| random_polygon(r, 100, 1000), 3000, None);
    run_cases(&mut t, &mut rng, "wide", &mut |r| random_polygon(r, 1 << 20, 1 << 10), 1000, None);
    // Every case runs; what must agree exactly on any input is the success flag and SimplifyPath. The port follows an older Clipper2 than
    // KiCad's 2.0.1 (see the module doc), so the rest is counted and printed.
    t.report("random polygons", &["clip: success", "simplify_path"]);
}

#[test]
fn port_matches_binding_on_kicad_qa_board_polygons() {
    // Polygons of up to 3000 points (a zone outline, a board edge, a filled area of a small zone): the 28k-point fills make each case take
    // seconds without adding anything the smaller ones do not.
    let polys: Vec<Path64> = qa_polygons().into_iter().filter(|p| p.len() <= 3000).collect();
    if polys.is_empty() {
        eprintln!("skipped: KICAD_QA_DATA is not set");
        return;
    }
    let mut t = Tally::default();
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let n = polys.len() as u64;
    run_cases(&mut t, &mut rng, "qa", &mut |r| polys[r.below(n) as usize].clone(), 2000, Some(2_000_000.0));
    // On real board polygons the boolean operations (what zones, outlines and hulls run) must be exact; offsets and RectClip are counted.
    t.report(&format!("{} QA board polygons", polys.len()), &["clip: success", "clip", "clip to tree", "clip with open subjects", "clip to tree with open subjects", "simplify_path"]);
}
