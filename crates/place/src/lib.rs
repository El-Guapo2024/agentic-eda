//! `eda-place` — native component placer (v1): seeded simulated annealing
//! over real courtyards and real pad centres, against our own IR.
//!
//! Objective (all integer-µm, summed):
//! * half-perimeter wirelength of every net over its pad centres,
//! * courtyard overlap area (same side) × `W_OVERLAP`,
//! * courtyard area outside the outline × `W_OUTSIDE`,
//! * proximity-rule and cluster-membership distance excess × `W_RULE`.
//!
//! Moves: displace, swap two parts, rotate by 90°. Positions snap to
//! `PlaceOptions::snap`. The result is legalised (overlaps pushed apart)
//! and returned only if it is overlap-free and inside the outline —
//! otherwise the caller gets `CheckResult` failures, never a silently
//! broken placement.

use eda_model::footprint::{placed_pads, rotated_extent};
use eda_model::ir::{Design, FootprintInstance, PlacementSection, Point, Side, Um};
use eda_model::{CheckResult, ConstraintModel, PlacementRule};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PlaceOptions {
    pub seed: u64,
    /// Position snap, µm.
    pub snap: Um,
    /// Annealing moves per part. Larger = better, slower.
    pub moves_per_part: usize,
    /// Explicit outline; else `model.board.outline`; else an auto square.
    pub outline: Option<Vec<Point>>,
    /// Extra keep-apart margin around every courtyard during placement,
    /// µm — routing channel space. Gates still judge the true courtyards.
    pub spacing: Um,
}

impl Default for PlaceOptions {
    fn default() -> Self {
        PlaceOptions { seed: 0, snap: 100, moves_per_part: 4000, outline: None, spacing: 600 }
    }
}

const W_OVERLAP: f64 = 4.0; // per µm² of overlap, expressed as µm of HPWL-equivalent per 1000 µm² -> see cost()
const W_OUTSIDE: f64 = 8.0;
const W_RULE: f64 = 12.0;
const CLUSTER_MM: f64 = 4.0;

struct Item {
    id: String,
    /// Courtyard half-extents (hw, hh) at rot 0.
    half: (Um, Um),
    /// Local pad centres (x, y) at rot 0, per pad number.
    pads: Vec<(String, (Um, Um))>,
}

#[derive(Clone, Copy)]
struct Pose {
    x: Um,
    y: Um,
    rot: u8, // quarter turns
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % ((hi - lo).max(1) as u64)) as i64
    }
}

fn instance(item: &Item, pose: Pose) -> FootprintInstance {
    FootprintInstance { id: item.id.clone(), at: Point { x: pose.x, y: pose.y }, rot: pose.rot as u32 * 90_000, side: Side::Top }
}

fn courtyard(item: &Item, pose: Pose) -> (Um, Um, Um, Um) {
    let fp = instance(item, pose);
    let (w, h) = rotated_extent(&fp, (item.half.0 * 2, item.half.1 * 2));
    (pose.x - w / 2, pose.y - h / 2, pose.x + w / 2, pose.y + h / 2)
}

fn overlap(a: (Um, Um, Um, Um), b: (Um, Um, Um, Um)) -> i64 {
    let w = (a.2.min(b.2) - a.0.max(b.0)).max(0);
    let h = (a.3.min(b.3) - a.1.max(b.1)).max(0);
    w * h
}

/// Edge-to-edge gap between two rectangles (0 when they touch/overlap).
fn gap(a: (Um, Um, Um, Um), b: (Um, Um, Um, Um)) -> f64 {
    let dx = (a.0 - b.2).max(b.0 - a.2).max(0) as f64;
    let dy = (a.1 - b.3).max(b.1 - a.3).max(0) as f64;
    (dx * dx + dy * dy).sqrt()
}

fn outside_area(r: (Um, Um, Um, Um), bb: (Um, Um, Um, Um)) -> i64 {
    let area = (r.2 - r.0) * (r.3 - r.1);
    area - overlap(r, bb)
}

struct Problem<'a> {
    items: Vec<Item>,
    index: HashMap<String, usize>,
    /// Nets as (item index, pad index) lists.
    nets: Vec<Vec<(usize, usize)>>,
    /// Which nets touch each item.
    nets_of: Vec<Vec<usize>>,
    /// (item a, item b, max distance µm)
    pair_rules: Vec<(usize, usize, i64)>,
    bbox: (Um, Um, Um, Um),
    outline: Vec<Point>,
    spacing: Um,
    model: &'a ConstraintModel,
}

impl Problem<'_> {
    fn pad_center(&self, i: usize, k: usize, pose: Pose) -> Point {
        let fp = instance(&self.items[i], pose);
        eda_model::footprint::to_board(&fp, self.items[i].pads[k].1)
    }

    fn hpwl(&self, net: usize, poses: &[Pose]) -> i64 {
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for &(i, k) in &self.nets[net] {
            let p = self.pad_center(i, k, poses[i]);
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        if x0 == i64::MAX {
            0
        } else {
            (x1 - x0) + (y1 - y0)
        }
    }

    /// Cost contribution of item `i` (its nets, its overlaps, its rules).
    fn local_cost(&self, i: usize, poses: &[Pose]) -> f64 {
        let mut c = 0.0;
        for &n in &self.nets_of[i] {
            c += self.hpwl(n, poses) as f64;
        }
        let ci = courtyard(&self.items[i], poses[i]);
        for j in 0..self.items.len() {
            if j != i {
                let ov = overlap(ci, courtyard(&self.items[j], poses[j]));
                if ov > 0 {
                    // sqrt keeps the penalty in µm units and dominant.
                    c += W_OVERLAP * ((ov as f64).sqrt() + 500.0);
                }
            }
        }
        let out = outside_area(ci, self.bbox);
        if out > 0 {
            c += W_OUTSIDE * ((out as f64).sqrt() + 500.0);
        }
        for &(a, b, max) in &self.pair_rules {
            if a == i || b == i {
                let d = self.rule_gap(a, b, poses);
                if d > max as f64 {
                    c += W_RULE * (d - max as f64);
                }
            }
        }
        c
    }

    /// True-courtyard edge gap (spacing inflation removed), matching the
    /// placement gate's definition of proximity.
    fn rule_gap(&self, a: usize, b: usize, poses: &[Pose]) -> f64 {
        let shrink = |r: (Um, Um, Um, Um)| (r.0 + self.spacing, r.1 + self.spacing, r.2 - self.spacing, r.3 - self.spacing);
        gap(shrink(courtyard(&self.items[a], poses[a])), shrink(courtyard(&self.items[b], poses[b])))
    }

    fn total_cost(&self, poses: &[Pose]) -> f64 {
        let mut c = 0.0;
        for n in 0..self.nets.len() {
            c += self.hpwl(n, poses) as f64;
        }
        for i in 0..self.items.len() {
            let ci = courtyard(&self.items[i], poses[i]);
            for j in i + 1..self.items.len() {
                let ov = overlap(ci, courtyard(&self.items[j], poses[j]));
                if ov > 0 {
                    c += W_OVERLAP * ((ov as f64).sqrt() + 500.0);
                }
            }
            let out = outside_area(ci, self.bbox);
            if out > 0 {
                c += W_OUTSIDE * ((out as f64).sqrt() + 500.0);
            }
        }
        for &(a, b, max) in &self.pair_rules {
            let d = self.rule_gap(a, b, poses);
            if d > max as f64 {
                c += W_RULE * (d - max as f64);
            }
        }
        c
    }
}

fn snap(v: Um, s: Um) -> Um {
    if s <= 1 {
        v
    } else {
        (v as f64 / s as f64).round() as Um * s
    }
}

fn build_problem<'a>(model: &'a ConstraintModel, opts: &PlaceOptions) -> Result<Problem<'a>, Vec<CheckResult>> {
    let mut fails = Vec::new();
    let mut items = Vec::new();
    let mut parts: Vec<&eda_model::Part> = model.parts.iter().collect();
    parts.sort_by(|a, b| a.reference.cmp(&b.reference));
    for part in parts {
        match model.footprint_of(part) {
            Some(fp) => {
                let mut pads: Vec<(String, (Um, Um))> = fp.pads.iter().map(|p| (p.number.clone(), p.at)).collect();
                pads.sort_by(|a, b| a.0.cmp(&b.0));
                let (hw, hh) = fp.courtyard_half();
                items.push(Item { id: part.reference.clone(), half: (hw + opts.spacing, hh + opts.spacing), pads });
            }
            None => fails.push(CheckResult::fail(
                "place_precondition",
                &part.reference,
                format!("no footprint geometry (footprint={:?}, package={:?}); define it in `footprints` or use a built-in package name", part.footprint, part.package),
            )),
        }
    }
    if !fails.is_empty() {
        return Err(fails);
    }
    if items.is_empty() {
        return Err(vec![CheckResult::fail("place_precondition", "model", "no parts to place")]);
    }
    let index: HashMap<String, usize> = items.iter().enumerate().map(|(i, it)| (it.id.clone(), i)).collect();

    let mut nets = Vec::new();
    let mut nets_of = vec![Vec::new(); items.len()];
    for net in &model.nets {
        let mut members = Vec::new();
        for pin in &net.pins {
            let Some((r, p)) = pin.split_once('.') else { continue };
            let Some(&i) = index.get(r) else { continue };
            if let Some(k) = items[i].pads.iter().position(|(n, _)| n == p) {
                members.push((i, k));
            }
        }
        if members.len() >= 2 {
            let n = nets.len();
            for &(i, _) in &members {
                if !nets_of[i].contains(&n) {
                    nets_of[i].push(n);
                }
            }
            nets.push(members);
        }
    }

    let mut pair_rules = Vec::new();
    for rule in &model.placement_rules {
        if let PlacementRule::Proximity { a, b, max_mm } = rule {
            if let (Some(&ia), Some(&ib)) = (index.get(a), index.get(b)) {
                pair_rules.push((ia, ib, (max_mm * 1000.0) as i64));
            }
        }
    }
    for cl in &model.clusters {
        if let Some(&ia) = index.get(&cl.anchor) {
            for m in &cl.members {
                if let Some(&im) = index.get(m) {
                    pair_rules.push((ia, im, (CLUSTER_MM * 1000.0) as i64));
                }
            }
        }
    }

    let outline = match opts.outline.clone().or_else(|| model.board.outline.clone()) {
        Some(o) if o.len() >= 3 => o,
        Some(_) => return Err(vec![CheckResult::fail("place_precondition", "outline", "outline needs at least 3 points")]),
        None => {
            // Auto square: 2.5x the summed courtyard area, rounded up to mm.
            let area: f64 = items.iter().map(|it| (it.half.0 * 2) as f64 * (it.half.1 * 2) as f64).sum();
            let side = ((area * 2.5).sqrt() / 1000.0).ceil() as Um * 1000;
            let side = side.max(items.iter().map(|it| it.half.0.max(it.half.1) * 2 + 1000).max().unwrap_or(1000));
            vec![Point { x: 0, y: 0 }, Point { x: side, y: 0 }, Point { x: side, y: side }, Point { x: 0, y: side }]
        }
    };
    let bbox = (
        outline.iter().map(|p| p.x).min().unwrap(),
        outline.iter().map(|p| p.y).min().unwrap(),
        outline.iter().map(|p| p.x).max().unwrap(),
        outline.iter().map(|p| p.y).max().unwrap(),
    );
    Ok(Problem { items, index, nets, nets_of, pair_rules, bbox, outline, spacing: opts.spacing, model })
}

/// Initial placement: shelf packing. Parts in connectivity order are laid
/// left-to-right in rows across the board width; every row is as tall as
/// its tallest member. Overlap-free and inside the board by construction
/// whenever the board is big enough, which is what makes legalisation a
/// no-op for the common case.
fn initial(pb: &Problem, snap_um: Um) -> Vec<Pose> {
    let n = pb.items.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(pb.nets_of[i].len()), pb.items[i].id.clone()));
    let width = pb.bbox.2 - pb.bbox.0;
    let mut poses = vec![Pose { x: 0, y: 0, rot: 0 }; n];
    let mut x = pb.bbox.0;
    let mut y = pb.bbox.1;
    let mut row_h = 0;
    for &i in &order {
        let (w, h) = (pb.items[i].half.0 * 2, pb.items[i].half.1 * 2);
        if x + w > pb.bbox.2 && x > pb.bbox.0 {
            x = pb.bbox.0;
            y += row_h;
            row_h = 0;
        }
        poses[i] = Pose { x: snap(x + w / 2, snap_um), y: snap(y + h / 2, snap_um), rot: 0 };
        x += w;
        row_h = row_h.max(h);
        let _ = width;
    }
    poses
}

fn anneal(pb: &Problem, poses: &mut [Pose], opts: &PlaceOptions) {
    let n = pb.items.len();
    if n == 0 {
        return;
    }
    let mut rng = Rng(opts.seed ^ 0xA5A5_5A5A_1234_5678);
    let span = ((pb.bbox.2 - pb.bbox.0).max(pb.bbox.3 - pb.bbox.1)) as f64;
    let total_moves = opts.moves_per_part * n;
    let mut cur = pb.total_cost(poses);
    let t0 = (cur / n as f64).max(1000.0);
    let t_end = 10.0;
    let mut best = cur;
    let mut best_poses: Vec<Pose> = poses.to_vec();

    for step in 0..total_moves {
        let frac = step as f64 / total_moves.max(1) as f64;
        let temp = t0 * (t_end / t0).powf(frac);
        let reach = (span * (1.0 - frac) * 0.5).max(opts.snap as f64 * 2.0);

        let kind = rng.below(10);
        let i = rng.below(n);
        let old_i = poses[i];
        let mut j = usize::MAX;
        let mut old_j = old_i;

        let before = if kind < 6 {
            let c = pb.local_cost(i, poses);
            let dx = rng.range(-(reach as i64), reach as i64 + 1);
            let dy = rng.range(-(reach as i64), reach as i64 + 1);
            poses[i] = Pose {
                x: snap((old_i.x + dx).clamp(pb.bbox.0, pb.bbox.2), opts.snap),
                y: snap((old_i.y + dy).clamp(pb.bbox.1, pb.bbox.3), opts.snap),
                rot: old_i.rot,
            };
            c
        } else if kind < 8 && n > 1 {
            j = rng.below(n);
            if j == i {
                j = (i + 1) % n;
            }
            old_j = poses[j];
            let c = pb.local_cost(i, poses) + pb.local_cost(j, poses);
            poses[i] = Pose { x: old_j.x, y: old_j.y, rot: old_i.rot };
            poses[j] = Pose { x: old_i.x, y: old_i.y, rot: old_j.rot };
            c
        } else {
            let c = pb.local_cost(i, poses);
            poses[i] = Pose { rot: (old_i.rot + 1 + rng.below(3) as u8) % 4, ..old_i };
            c
        };
        let after = if j != usize::MAX { pb.local_cost(i, poses) + pb.local_cost(j, poses) } else { pb.local_cost(i, poses) };
        // Local cost double-counts the (i,j) overlap term symmetrically on
        // both sides of the delta, so the delta stays exact.
        let delta = after - before;
        let accept = delta <= 0.0 || rng.unit() < (-delta / temp).exp();
        if accept {
            cur += delta;
            if cur < best {
                best = cur;
                best_poses.copy_from_slice(poses);
            }
        } else {
            poses[i] = old_i;
            if j != usize::MAX {
                poses[j] = old_j;
            }
        }
    }
    // Recompute exactly and keep whichever is truly better.
    let exact_cur = pb.total_cost(poses);
    let exact_best = pb.total_cost(&best_poses);
    if exact_best < exact_cur {
        poses.copy_from_slice(&best_poses);
    }
}

/// Push overlapping courtyards apart (each half the penetration along the
/// axis of least overlap) and pull stragglers inside the board.
/// Deterministic, bounded; returns whether the result is clean.
fn legalize(pb: &Problem, poses: &mut [Pose], snap_um: Um) -> bool {
    let n = pb.items.len();
    let clamp_inside = |pb: &Problem, poses: &mut [Pose], i: usize| {
        let ci = courtyard(&pb.items[i], poses[i]);
        let mut dx = 0;
        let mut dy = 0;
        if ci.0 < pb.bbox.0 {
            dx += pb.bbox.0 - ci.0;
        }
        if ci.2 > pb.bbox.2 {
            dx -= ci.2 - pb.bbox.2;
        }
        if ci.1 < pb.bbox.1 {
            dy += pb.bbox.1 - ci.1;
        }
        if ci.3 > pb.bbox.3 {
            dy -= ci.3 - pb.bbox.3;
        }
        if dx != 0 || dy != 0 {
            // Snap toward the interior so an odd half-extent can't
            // oscillate half a snap outside the edge forever.
            poses[i].x = snap(poses[i].x + dx, snap_um) + dx.signum() * snap_um;
            poses[i].y = snap(poses[i].y + dy, snap_um) + dy.signum() * snap_um;
            true
        } else {
            false
        }
    };
    for _ in 0..500 {
        let mut moved = false;
        for i in 0..n {
            moved |= clamp_inside(pb, poses, i);
        }
        for i in 0..n {
            for j in i + 1..n {
                let ci = courtyard(&pb.items[i], poses[i]);
                let cj = courtyard(&pb.items[j], poses[j]);
                if overlap(ci, cj) == 0 {
                    continue;
                }
                let px = (ci.2 - cj.0).min(cj.2 - ci.0);
                let py = (ci.3 - cj.1).min(cj.3 - ci.1);
                let dir_x: i64 = if poses[j].x >= poses[i].x { 1 } else { -1 };
                let dir_y: i64 = if poses[j].y >= poses[i].y { 1 } else { -1 };
                if px <= py {
                    let step = px / 2 + snap_um;
                    poses[j].x = snap(poses[j].x + dir_x * step, snap_um);
                    poses[i].x = snap(poses[i].x - dir_x * step, snap_um);
                } else {
                    let step = py / 2 + snap_um;
                    poses[j].y = snap(poses[j].y + dir_y * step, snap_um);
                    poses[i].y = snap(poses[i].y - dir_y * step, snap_um);
                }
                moved = true;
            }
        }
        if !moved {
            return true;
        }
    }
    // Final verdict.
    for i in 0..n {
        let ci = courtyard(&pb.items[i], poses[i]);
        if outside_area(ci, pb.bbox) > 0 {
            return false;
        }
        for j in i + 1..n {
            if overlap(ci, courtyard(&pb.items[j], poses[j])) > 0 {
                return false;
            }
        }
    }
    true
}

/// Place every part of `model` on the board. `design` supplies provenance
/// and (if present) the schematic section, which is carried through
/// untouched; the returned design has a fresh `placement` section.
pub fn place(design: &Design, model: &ConstraintModel, opts: &PlaceOptions) -> Result<Design, Vec<CheckResult>> {
    let pb = build_problem(model, opts)?;
    let mut poses = initial(&pb, opts.snap);
    anneal(&pb, &mut poses, opts);
    let clean = legalize(&pb, &mut poses, opts.snap);

    let mut footprints: Vec<FootprintInstance> = pb.items.iter().zip(poses.iter()).map(|(it, p)| instance(it, *p)).collect();
    footprints.sort_by(|a, b| a.id.cmp(&b.id));

    let mut out = design.clone();
    out.provenance.seed = opts.seed;
    out.placement = Some(PlacementSection { outline: pb.outline.clone(), footprints });
    out.routing = None;

    if !clean {
        let mut fails = vec![CheckResult::fail("place_legalize", "design", "could not resolve all courtyard overlaps within the outline; enlarge the board or relax rules")];
        // Report the offenders so the caller can act.
        for i in 0..pb.items.len() {
            let ci = courtyard(&pb.items[i], poses[i]);
            let outside = outside_area(ci, pb.bbox);
            if outside > 0 {
                fails.push(CheckResult::fail("place_outside", &pb.items[i].id, format!("{outside} µm² outside the board (courtyard {ci:?}, board {:?})", pb.bbox)));
            }
            for j in i + 1..pb.items.len() {
                let ov = overlap(courtyard(&pb.items[i], poses[i]), courtyard(&pb.items[j], poses[j]));
                if ov > 0 {
                    fails.push(CheckResult::fail("place_overlap", format!("{}/{}", pb.items[i].id, pb.items[j].id), format!("{ov} µm²")));
                }
            }
        }
        return Err(fails);
    }
    // Silence unused-field lints for fields kept for diagnostics.
    let _ = (&pb.index, &pb.model, placed_pads as fn(&ConstraintModel, &eda_model::Part, &FootprintInstance) -> Option<Vec<eda_model::footprint::PlacedPad>>);
    Ok(out)
}

/// Total half-perimeter wirelength of a placed design, µm. A cheap,
/// generator-agnostic quality number for scorecards.
pub fn hpwl(design: &Design, model: &ConstraintModel) -> Option<i64> {
    let pl = design.placement.as_ref()?;
    let mut centers: HashMap<String, Point> = HashMap::new();
    for fp in &pl.footprints {
        let part = model.part(&fp.id)?;
        for pad in placed_pads(model, part, fp)? {
            centers.insert(format!("{}.{}", fp.id, pad.number), pad.center);
        }
    }
    let mut total = 0;
    for net in &model.nets {
        let pts: Vec<&Point> = net.pins.iter().filter_map(|p| centers.get(p)).collect();
        if pts.len() < 2 {
            continue;
        }
        let x0 = pts.iter().map(|p| p.x).min().unwrap();
        let x1 = pts.iter().map(|p| p.x).max().unwrap();
        let y0 = pts.iter().map(|p| p.y).min().unwrap();
        let y1 = pts.iter().map(|p| p.y).max().unwrap();
        total += (x1 - x0) + (y1 - y0);
    }
    Some(total)
}
