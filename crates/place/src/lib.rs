//! `eda-place` — native component placer (v1): seeded simulated annealing
//! over real courtyards and real pad centres, against our own IR.
//!
//! Objective (all integer-µm, summed):
//! * half-perimeter wirelength of every net over its pad centres, plus
//!   `W_COMPACT` × the excess of a small net's HPWL over its packed bound,
//! * courtyard overlap area (same side) × `W_OVERLAP`,
//! * courtyard area outside the outline × `W_OUTSIDE`,
//! * proximity-rule, cluster-membership and (automatic) decoupling-cap
//!   distance excess × `W_RULE`,
//! * pin-escape blocking, RUDY congestion overflow,
//! * edge-connector distance to a usable board edge × `W_EDGE`,
//! * crossing pairs of free 2-pin stubs × `W_CROSS`,
//! * on an explicit outline, cluster off-centring and span shortfall
//!   × `W_USE`.
//!
//! Each of those mirrors a gate in `eda_gates::pcb::check_placement_locality`.
//!
//! Moves: displace, jump (edge connectors: to a random edge), swap two
//! parts, rotate by 90°. Edge connectors are kept flush on a usable edge
//! by construction (`flush_to_edge`), from the initial perimeter walk
//! through every move and legalisation push. Positions snap to
//! `PlaceOptions::snap`. After annealing a greedy polish tries each part
//! at each rotation flush against its neighbours, around its nets' pads
//! and (connectors) along the edges, keeping exact-cost improvements. The
//! result is legalised (overlaps pushed apart) and returned only if it is
//! overlap-free and inside the outline — otherwise the caller gets
//! `CheckResult` failures, never a silently broken placement. An auto
//! outline (none given) is finally shrunk to the placed cluster.

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

/// Overlap penalty, `W_OVERLAP * (sqrt(area) + 500)` µm of HPWL-equivalent:
/// a full-body overlap of two headers must cost more than the wirelength
/// saved by leaving them stacked on one edge.
const W_OVERLAP: f64 = 16.0;
const W_OUTSIDE: f64 = 8.0;
/// Explicit proximity/cluster/decoupling rules are hard gates: their pull
/// must beat every other linear term (HPWL 1, compactness 3, use 4).
const W_RULE: f64 = 40.0;
const CLUSTER_MM: f64 = 4.0;
/// Congestion overflow penalty, µm of HPWL-equivalent per (track-over-capacity)².
const W_CONGEST: f64 = 6000.0;
/// Per-pad penalty, µm of HPWL-equivalent, for each escape side beyond the
/// 2nd that a neighbour's courtyard blocks.
const W_ESCAPE: f64 = 3000.0;
/// Edge-connector pull, µm of HPWL-equivalent per µm between a connector's
/// courtyard and the nearest board edge (gate: `placement_edge_connector`).
const W_EDGE: f64 = 30.0;
/// Board-use pull, per µm of centring imbalance / span shortfall beyond
/// the targets below (gate: `placement_board_use`, 0.15 / 0.5).
const W_USE: f64 = 4.0;
const USE_MAX_IMBALANCE: f64 = 0.08;
const USE_MIN_SPAN: f64 = 0.58;
/// Per crossing pair of 2-pin-net stubs (gate: `placement_stub_crossings`).
const W_CROSS: f64 = 15000.0;
/// Net-compactness pull (gate: `placement_net_compactness`, 1.6x): per
/// µm a small net's HPWL exceeds `COMPACT_RATIO` × its packed bound
/// `2·sqrt(Σ member courtyard areas)`. Two 2-pin nets through one
/// resistor sum to the same HPWL wherever the resistor sits between its
/// partners; this is what says "not 38 mm from the MCU".
const W_COMPACT: f64 = 3.0;
const COMPACT_RATIO: f64 = 1.3;
/// Auto proximity rule, µm courtyard gap, for a decoupling capacitor and
/// the IC it decouples (gate: `placement_decoupling`, 3000 µm).
const DECOUPLING_UM: i64 = 2200;

struct Item {
    id: String,
    /// Edge connector (header/jack/USB): wants a board edge.
    connector: bool,
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
    /// Per net: HPWL above which `W_COMPACT` applies, or `None` for nets
    /// the gate doesn't judge (wide fan-out, or touching an edge connector).
    compact_limit: Vec<Option<f64>>,
    /// (item a, item b, max distance µm)
    pair_rules: Vec<(usize, usize, i64)>,
    /// The same pairs with the rule's true maximum (µm), for the final
    /// repair pass that measures exactly as the gate does.
    pair_rules_true: Vec<(usize, usize, i64)>,
    /// (item, candidate items, max distance µm): the nearest candidate
    /// must be within max — decoupling caps that could serve several ICs.
    group_rules: Vec<(usize, Vec<usize>, i64)>,
    /// 2-pin nets between distinct parts as (item, pad, item, pad).
    stubs: Vec<(usize, usize, usize, usize)>,
    /// Per stub: one end is a free 2-pin part (R/C/D…). A crossing of two
    /// such stubs is the placer's to fix (swap the two). Matches the gate.
    stub_free: Vec<bool>,
    stubs_of: Vec<Vec<usize>>,
    bbox: (Um, Um, Um, Um),
    outline: Vec<Point>,
    /// No outline was given: we placed on a generous square and will
    /// shrink it to the cluster afterwards, so board-use terms don't apply.
    auto_outline: bool,
    spacing: Um,
    /// Refdes font size for this board; the label box above each courtyard
    /// is part of the part's keep-out (gate `placement_refdes_clear`).
    font: Um,
    model: &'a ConstraintModel,
    /// Congestion grid bin size, µm (~2 mm).
    bin_um: Um,
    /// Tracks that can cross one bin, all routing layers combined —
    /// `layers * bin_um / (track_width + clearance)`.
    congest_capacity: f64,
    /// Nets eligible for congestion accounting: 2-6 pins. Wide fan-out
    /// nets (GND/VCC) spread over the whole board aren't a local
    /// congestion signal and would dominate the bin sums if included.
    congest_nets: Vec<usize>,
}

impl Problem<'_> {
    /// Courtyard (spacing-inflated) plus the refdes label box above it: the
    /// rectangle two parts must not share. Proximity gaps and outline
    /// containment still use the bare courtyard, as the gates do.
    fn keepout(&self, i: usize, pose: Pose) -> (Um, Um, Um, Um) {
        let c = courtyard(&self.items[i], pose);
        // The label hangs off the *bare* courtyard (the gate's geometry);
        // it is silkscreen and needs no routing-channel spacing of its own,
        // so it is unioned with the spacing-inflated courtyard as is.
        let sp = self.spacing;
        let l = eda_model::footprint::refdes_box_for((c.0 + sp, c.1 + sp, c.2 - sp, c.3 - sp), &self.items[i].id, self.font, self.bbox.1);
        (c.0.min(l.0), c.1.min(l.1), c.2.max(l.2), c.3.max(l.3))
    }

    fn pad_center(&self, i: usize, k: usize, pose: Pose) -> Point {
        let fp = instance(&self.items[i], pose);
        eda_model::footprint::to_board(&fp, self.items[i].pads[k].1)
    }

    fn hpwl(&self, net: usize, poses: &[Pose]) -> i64 {
        let (x0, y0, x1, y1) = self.net_bbox(net, poses);
        if x0 == i64::MAX {
            0
        } else {
            (x1 - x0) + (y1 - y0)
        }
    }

    /// Net's pad bounding box; (`i64::MAX`, ...) when the net has no placed
    /// pads (shouldn't happen — `build_problem` drops sub-2-pin nets).
    fn net_bbox(&self, net: usize, poses: &[Pose]) -> (Um, Um, Um, Um) {
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for &(i, k) in &self.nets[net] {
            let p = self.pad_center(i, k, poses[i]);
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        (x0, y0, x1, y1)
    }

    /// RUDY-style demand this net adds to each grid bin it overlaps:
    /// `net_hpwl * (bin ∩ net_bbox area) / net_bbox area / bin_um`, a
    /// dimensionless "tracks needed" estimate spread uniformly over the
    /// net's bounding box. Empty for ineligible (wide fan-out) nets.
    fn congest_contrib(&self, net: usize, poses: &[Pose], out: &mut Vec<((i32, i32), f64)>) {
        let (x0, y0, x1, y1) = self.net_bbox(net, poses);
        if x0 == i64::MAX {
            return;
        }
        let (w, h) = ((x1 - x0).max(1), (y1 - y0).max(1));
        let area = w as f64 * h as f64;
        let hpwl = (w + h) as f64;
        let bin = self.bin_um;
        let (bx0, by0, bx1, by1) = ((x0.div_euclid(bin)) as i32, (y0.div_euclid(bin)) as i32, (x1.div_euclid(bin)) as i32, (y1.div_euclid(bin)) as i32);
        if (bx1 - bx0 + 1) as i64 * (by1 - by0 + 1) as i64 > 64 {
            // Too spread out right now to be a useful local congestion
            // signal (and too many bins to touch on every move); it'll
            // start contributing once the annealer pulls it in tighter.
            return;
        }
        for by in by0..=by1 {
            for bx in bx0..=bx1 {
                let (cx0, cy0, cx1, cy1) = (bx as i64 * bin, by as i64 * bin, (bx as i64 + 1) * bin, (by as i64 + 1) * bin);
                let ow = (x1.min(cx1) - x0.max(cx0)).max(0) as f64;
                let oh = (y1.min(cy1) - y0.max(cy0)).max(0) as f64;
                let ov = ow * oh;
                if ov > 0.0 {
                    out.push(((bx, by), hpwl * ov / area / bin as f64));
                }
            }
        }
    }

    /// Penalty for one bin's demand over `congest_capacity`: squared
    /// overflow, so a bin already saturated resists piling on further.
    fn bin_penalty(&self, demand: f64) -> f64 {
        let over = demand - self.congest_capacity;
        if over > 0.0 {
            W_CONGEST * over * over
        } else {
            0.0
        }
    }

    /// Pin-escape cost for item `i`: for each of its pads, count how many
    /// of the 4 cardinal escape directions are blocked by a neighbour's
    /// courtyard within one clearance of the pad — a pad boxed in on 3-4
    /// sides is hard or impossible to route out of even if there's no
    /// overlap. `esc` is a short probe, one clearance + a hair, along each
    /// axis from the pad centre.
    fn escape_cost(&self, i: usize, poses: &[Pose]) -> f64 {
        let esc = self.model.board.clearance + self.spacing + 200;
        let mut cost = 0.0;
        for k in 0..self.items[i].pads.len() {
            let p = self.pad_center(i, k, poses[i]);
            let probes = [(esc, 0), (-esc, 0), (0, esc), (0, -esc)];
            let mut blocked = 0;
            for (dx, dy) in probes {
                let probe = (p.x.min(p.x + dx), p.y.min(p.y + dy), p.x.max(p.x + dx), p.y.max(p.y + dy));
                for j in 0..self.items.len() {
                    if j == i {
                        continue;
                    }
                    if overlap(probe, self.keepout(j, poses[j])) > 0 {
                        blocked += 1;
                        break;
                    }
                }
            }
            if blocked >= 3 {
                // 3 or 4 sides blocked: a real escape problem, not just a
                // tight neighbour on one side (routing can usually still
                // exit past a single blocked side).
                cost += W_ESCAPE * (blocked - 2) as f64;
            }
        }
        cost
    }

    fn compact_cost(&self, net: usize, hpwl: f64) -> f64 {
        match self.compact_limit[net] {
            Some(limit) if hpwl > limit => W_COMPACT * (hpwl - limit),
            _ => 0.0,
        }
    }

    /// Cost contribution of item `i` (its nets, its overlaps, its rules).
    fn local_cost(&self, i: usize, poses: &[Pose]) -> f64 {
        let mut c = 0.0;
        for &n in &self.nets_of[i] {
            let h = self.hpwl(n, poses) as f64;
            c += h + self.compact_cost(n, h);
        }
        let ci = self.keepout(i, poses[i]);
        for j in 0..self.items.len() {
            if j != i {
                let ov = overlap(ci, self.keepout(j, poses[j]));
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
        for (a, bs, max) in &self.group_rules {
            if *a == i || bs.contains(&i) {
                c += self.group_rule_cost(*a, bs, *max, poses);
            }
        }
        c += self.escape_cost(i, poses);
        c += self.edge_cost(i, poses);
        c += self.crossing_cost(i, poses);
        c
    }

    /// For an edge connector, the same pose moved flush against the
    /// nearest usable edge (long side along it); anything else unchanged.
    /// Connectors live on edges by construction — every move that touches
    /// one ends here — so the edge cost only has to catch what
    /// legalisation later pushes inward.
    fn flush_to_edge(&self, i: usize, pose: Pose, snap_um: Um) -> Pose {
        if !self.items[i].connector {
            return pose;
        }
        let r = courtyard(&self.items[i], pose);
        let (w, h) = (r.2 - r.0, r.3 - r.1);
        let (left, right, top, bottom) = (r.0 - self.bbox.0, self.bbox.2 - r.2, r.1 - self.bbox.1, self.bbox.3 - r.3);
        let horizontal_ok = !(h as f64 > 1.3 * w as f64); // may lie along top/bottom
        let vertical_ok = !(w as f64 > 1.3 * h as f64); // may lie along left/right
        let mut best: Option<(Um, Pose)> = None;
        let mut consider = |gap: Um, p: Pose| {
            if best.map_or(true, |(g, _)| gap < g) {
                best = Some((gap, p));
            }
        };
        if vertical_ok {
            consider(left, Pose { x: snap_up(self.bbox.0 + w / 2, snap_um), ..pose });
            consider(right, Pose { x: snap_down(self.bbox.2 - w / 2, snap_um), ..pose });
        }
        if horizontal_ok {
            consider(top, Pose { y: snap_up(self.bbox.1 + h / 2, snap_um), ..pose });
            consider(bottom, Pose { y: snap_down(self.bbox.3 - h / 2, snap_um), ..pose });
        }
        best.map_or(pose, |(_, p)| p)
    }

    /// Distance from an edge connector's courtyard to the nearest usable
    /// board edge. Zero for non-connectors.
    fn edge_cost(&self, i: usize, poses: &[Pose]) -> f64 {
        if !self.items[i].connector {
            return 0.0;
        }
        let r = courtyard(&self.items[i], poses[i]);
        let (w, h) = (r.2 - r.0, r.3 - r.1);
        let (left, right, top, bottom) = (r.0 - self.bbox.0, self.bbox.2 - r.2, r.1 - self.bbox.1, self.bbox.3 - r.3);
        // Same rule as the gate: an elongated connector only counts the
        // edges its long side can lie along.
        let gap = if w as f64 > 1.3 * h as f64 {
            top.min(bottom)
        } else if h as f64 > 1.3 * w as f64 {
            left.min(right)
        } else {
            left.min(right).min(top).min(bottom)
        };
        W_EDGE * gap.max(0) as f64
    }

    /// Crossing pairs between item `i`'s 2-pin stubs and every other stub.
    fn crossing_cost(&self, i: usize, poses: &[Pose]) -> f64 {
        let mut n = 0;
        for &s in &self.stubs_of[i] {
            let (a, ka, b, kb) = self.stubs[s];
            let (p, q) = (self.pad_center(a, ka, poses[a]), self.pad_center(b, kb, poses[b]));
            for (t, &(c, kc, d, kd)) in self.stubs.iter().enumerate() {
                if t == s || !(self.stub_free[s] && self.stub_free[t]) {
                    continue;
                }
                let (u, v) = (self.pad_center(c, kc, poses[c]), self.pad_center(d, kd, poses[d]));
                if segments_cross(p, q, u, v) {
                    n += 1;
                }
            }
        }
        W_CROSS * n as f64
    }

    /// Board-use penalty over the union of all courtyards: centring
    /// imbalance beyond `USE_MAX_IMBALANCE` and span shortfall below
    /// `USE_MIN_SPAN`, each in µm. Global — recomputed per move, O(n).
    fn use_cost(&self, poses: &[Pose]) -> f64 {
        if self.items.len() < 2 || self.auto_outline {
            return 0.0;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for (i, it) in self.items.iter().enumerate() {
            let r = courtyard(it, poses[i]);
            x0 = x0.min(r.0);
            y0 = y0.min(r.1);
            x1 = x1.max(r.2);
            y1 = y1.max(r.3);
        }
        // The gate measures true courtyards: strip the spacing inflation
        // from the union so a span that only just clears the target here
        // clears it there too.
        let (x0, y0, x1, y1) = (x0 + self.spacing, y0 + self.spacing, x1 - self.spacing, y1 - self.spacing);
        let (w, h) = ((self.bbox.2 - self.bbox.0) as f64, (self.bbox.3 - self.bbox.1) as f64);
        let imb_x = ((x0 - self.bbox.0) - (self.bbox.2 - x1)).abs() as f64;
        let imb_y = ((y0 - self.bbox.1) - (self.bbox.3 - y1)).abs() as f64;
        let short_x = USE_MIN_SPAN * w - (x1 - x0) as f64;
        let short_y = USE_MIN_SPAN * h - (y1 - y0) as f64;
        W_USE * ((imb_x - USE_MAX_IMBALANCE * w).max(0.0) + (imb_y - USE_MAX_IMBALANCE * h).max(0.0) + short_x.max(0.0) + short_y.max(0.0))
    }

    /// Full congestion penalty, built from scratch — only for `total_cost`
    /// bookkeeping (called a handful of times per run, not per move; the
    /// per-move delta is tracked incrementally by `anneal`'s own grid).
    fn full_congestion_penalty(&self, poses: &[Pose]) -> f64 {
        let mut demand: HashMap<(i32, i32), f64> = HashMap::new();
        let mut buf = Vec::new();
        for &n in &self.congest_nets {
            buf.clear();
            self.congest_contrib(n, poses, &mut buf);
            for &(bin, d) in &buf {
                *demand.entry(bin).or_insert(0.0) += d;
            }
        }
        demand.values().map(|&d| self.bin_penalty(d)).sum()
    }

    fn group_rule_cost(&self, a: usize, bs: &[usize], max: i64, poses: &[Pose]) -> f64 {
        let d = bs.iter().map(|&b| self.rule_gap(a, b, poses)).fold(f64::MAX, f64::min);
        if d > max as f64 {
            W_RULE * (d - max as f64)
        } else {
            0.0
        }
    }

    /// True-courtyard edge gap (spacing inflation removed), matching the
    /// placement gate's definition of proximity.
    fn rule_gap(&self, a: usize, b: usize, poses: &[Pose]) -> f64 {
        let shrink = |r: (Um, Um, Um, Um)| (r.0 + self.spacing, r.1 + self.spacing, r.2 - self.spacing, r.3 - self.spacing);
        gap(shrink(courtyard(&self.items[a], poses[a])), shrink(courtyard(&self.items[b], poses[b])))
    }

    /// Cost breakdown for `EDA_PLACE_DEBUG`: (hpwl, overlap+outside, rules+compactness, escape, edge, cross, use, congest).
    fn breakdown(&self, poses: &[Pose]) -> [f64; 8] {
        let mut b = [0.0; 8];
        for n in 0..self.nets.len() {
            let h = self.hpwl(n, poses) as f64;
            b[0] += h;
            b[2] += self.compact_cost(n, h);
        }
        for i in 0..self.items.len() {
            let ci = self.keepout(i, poses[i]);
            for j in i + 1..self.items.len() {
                let ov = overlap(ci, self.keepout(j, poses[j]));
                if ov > 0 {
                    b[1] += W_OVERLAP * ((ov as f64).sqrt() + 500.0);
                }
            }
            let out = outside_area(ci, self.bbox);
            if out > 0 {
                b[1] += W_OUTSIDE * ((out as f64).sqrt() + 500.0);
            }
            b[3] += self.escape_cost(i, poses);
            b[4] += self.edge_cost(i, poses);
            b[5] += self.crossing_cost(i, poses) / 2.0;
        }
        for &(a, bb, max) in &self.pair_rules {
            let d = self.rule_gap(a, bb, poses);
            if d > max as f64 {
                b[2] += W_RULE * (d - max as f64);
            }
        }
        for (a, bs, max) in &self.group_rules {
            b[2] += self.group_rule_cost(*a, bs, *max, poses);
        }
        b[6] = self.use_cost(poses);
        b[7] = self.full_congestion_penalty(poses);
        b
    }

    fn total_cost(&self, poses: &[Pose]) -> f64 {
        let mut c = 0.0;
        for n in 0..self.nets.len() {
            let h = self.hpwl(n, poses) as f64;
            c += h + self.compact_cost(n, h);
        }
        for i in 0..self.items.len() {
            let ci = self.keepout(i, poses[i]);
            for j in i + 1..self.items.len() {
                let ov = overlap(ci, self.keepout(j, poses[j]));
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
        for (a, bs, max) in &self.group_rules {
            c += self.group_rule_cost(*a, bs, *max, poses);
        }
        for i in 0..self.items.len() {
            c += self.escape_cost(i, poses);
            c += self.edge_cost(i, poses);
            c += self.crossing_cost(i, poses) / 2.0;
        }
        c += self.use_cost(poses);
        c += self.full_congestion_penalty(poses);
        c
    }
}

fn orient(a: Point, b: Point, c: Point) -> i128 {
    (b.x - a.x) as i128 * (c.y - a.y) as i128 - (b.y - a.y) as i128 * (c.x - a.x) as i128
}

/// Proper crossing of segments ab and cd (shared endpoints don't count).
fn segments_cross(a: Point, b: Point, c: Point, d: Point) -> bool {
    let o1 = orient(a, b, c).signum();
    let o2 = orient(a, b, d).signum();
    let o3 = orient(c, d, a).signum();
    let o4 = orient(c, d, b).signum();
    o1 != o2 && o3 != o4 && o1 != 0 && o2 != 0 && o3 != 0 && o4 != 0
}

/// Reference `J…` or a value/mpn naming a header/jack/USB/terminal.
/// Must agree with `eda_gates::pcb::is_edge_connector`.
fn is_edge_connector(part: &eda_model::Part) -> bool {
    let r = part.reference.as_str();
    let by_ref = r.len() > 1 && r.starts_with('J') && r[1..].chars().all(|c| c.is_ascii_digit());
    let text = format!("{} {}", part.value.clone().unwrap_or_default(), part.mpn.clone().unwrap_or_default()).to_ascii_lowercase();
    by_ref || ["header", "hdr", "conn", "usb", "jack", "terminal", "receptacle"].iter().any(|k| text.contains(k))
}

/// (capacitor, IC) pairs where a 2-pin `C…` shares both its nets with a
/// `U…`. Must agree with `eda_gates::pcb::decoupling_pairs`.
fn decoupling_pairs(model: &ConstraintModel) -> Vec<(String, String)> {
    let net_of_pin: HashMap<&str, &str> = model.nets.iter().flat_map(|n| n.pins.iter().map(move |p| (p.as_str(), n.name.as_str()))).collect();
    let mut pairs = Vec::new();
    for c in &model.parts {
        if !c.reference.starts_with('C') || c.pins.len() != 2 {
            continue;
        }
        let nets: Vec<&str> = c.pins.iter().filter_map(|p| net_of_pin.get(format!("{}.{}", c.reference, p.number).as_str()).copied()).collect();
        if nets.len() != 2 || nets[0] == nets[1] {
            continue;
        }
        for u in &model.parts {
            if !u.reference.starts_with('U') {
                continue;
            }
            let has = |net: &str| u.pins.iter().any(|p| net_of_pin.get(format!("{}.{}", u.reference, p.number).as_str()) == Some(&net));
            if has(nets[0]) && has(nets[1]) {
                pairs.push((c.reference.clone(), u.reference.clone()));
            }
        }
    }
    pairs
}

fn snap_up(v: Um, s: Um) -> Um {
    if s <= 1 {
        v
    } else {
        v.div_euclid(s) * s + if v.rem_euclid(s) == 0 { 0 } else { s }
    }
}

fn snap_down(v: Um, s: Um) -> Um {
    if s <= 1 {
        v
    } else {
        v.div_euclid(s) * s
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
                items.push(Item { id: part.reference.clone(), connector: is_edge_connector(part), half: (hw + opts.spacing, hh + opts.spacing), pads });
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

    // Gate's packed bound per net, from true courtyard areas.
    let compact_limit: Vec<Option<f64>> = nets
        .iter()
        .map(|members| {
            let parts: std::collections::BTreeSet<usize> = members.iter().map(|&(i, _)| i).collect();
            if parts.len() < 2 || parts.len() > 6 || parts.iter().any(|&i| items[i].connector) {
                return None;
            }
            let area: f64 = parts.iter().map(|&i| ((items[i].half.0 - opts.spacing) * 2) as f64 * ((items[i].half.1 - opts.spacing) * 2) as f64).sum();
            Some((COMPACT_RATIO * 2.0 * area.sqrt()).max(6000.0))
        })
        .collect();

    let mut stubs = Vec::new();
    let mut stub_free = Vec::new();
    let mut stubs_of = vec![Vec::new(); items.len()];
    let free_two_pin = |id: &str| model.part(id).map_or(false, |p| p.pins.len() == 2 && !id.starts_with('J') && !id.starts_with('U'));
    for members in &nets {
        if members.len() == 2 && members[0].0 != members[1].0 {
            let (a, ka) = members[0];
            let (b, kb) = members[1];
            stubs_of[a].push(stubs.len());
            stubs_of[b].push(stubs.len());
            stubs.push((a, ka, b, kb));
            stub_free.push(free_two_pin(&items[a].id) || free_two_pin(&items[b].id));
        }
    }

    let mut pair_rules = Vec::new();
    let mut pair_rules_true = Vec::new();
    // A capacitor on a shared power net "decouples" every IC on it; like
    // the gate, only the nearest candidate IC has to be close.
    let mut group_rules: Vec<(usize, Vec<usize>, i64)> = Vec::new();
    for (c, u) in decoupling_pairs(model) {
        if let (Some(&ic), Some(&iu)) = (index.get(&c), index.get(&u)) {
            match group_rules.iter_mut().find(|(a, _, _)| *a == ic) {
                Some((_, bs, _)) => bs.push(iu),
                None => group_rules.push((ic, vec![iu], DECOUPLING_UM)),
            }
        }
    }
    for rule in &model.placement_rules {
        if let PlacementRule::Proximity { a, b, max_mm } = rule {
            if let (Some(&ia), Some(&ib)) = (index.get(a), index.get(b)) {
                // 300 µm under the rule: the gate measures true courtyards
                // after snapping, and a linear penalty barely resists a
                // few-µm excess.
                pair_rules.push((ia, ib, ((max_mm * 1000.0) as i64 - 300).max(0)));
                pair_rules_true.push((ia, ib, (max_mm * 1000.0) as i64));
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

    let auto_outline = opts.outline.is_none() && model.board.outline.is_none();
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
    let bin_um: Um = 2000;
    let pitch = (model.board.track_width + model.board.clearance).max(1);
    let congest_capacity = model.board.layers.len() as f64 * bin_um as f64 / pitch as f64;
    let congest_nets: Vec<usize> = (0..nets.len()).filter(|&n| nets[n].len() >= 2 && nets[n].len() <= 6).collect();

    let font = model.board.refdes_font(&outline);
    Ok(Problem { items, index, nets, nets_of, compact_limit, pair_rules, pair_rules_true, group_rules, stubs, stub_free, stubs_of, bbox, outline, auto_outline, spacing: opts.spacing, font, model, bin_um, congest_capacity, congest_nets })
}

/// Connectivity-aware placement order: the highest-degree part seeds a
/// breadth-first walk over the shared-net graph (each item's neighbours —
/// other items on any of its nets — visited in degree order), so directly
/// connected parts land next to each other on the shelf pack below instead
/// of in whatever order they happened to sort alphabetically. Anything
/// left unreached (disconnected islands) is appended by degree.
fn connectivity_order(pb: &Problem) -> Vec<usize> {
    let n = pb.items.len();
    let degree = |i: usize| pb.nets_of[i].len();
    let mut order = Vec::with_capacity(n);
    let mut visited = vec![false; n];
    let mut remaining: Vec<usize> = (0..n).collect();
    remaining.sort_by_key(|&i| (std::cmp::Reverse(degree(i)), pb.items[i].id.clone()));

    let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    for &seed in &remaining.clone() {
        if visited[seed] {
            continue;
        }
        visited[seed] = true;
        queue.push_back(seed);
        while let Some(i) = queue.pop_front() {
            order.push(i);
            let mut neighbours: Vec<usize> = Vec::new();
            for &net in &pb.nets_of[i] {
                for &(oi, _) in &pb.nets[net] {
                    if oi != i && !visited[oi] && !neighbours.contains(&oi) {
                        neighbours.push(oi);
                    }
                }
            }
            neighbours.sort_by_key(|&k| (std::cmp::Reverse(degree(k)), pb.items[k].id.clone()));
            for k in neighbours {
                if !visited[k] {
                    visited[k] = true;
                    queue.push_back(k);
                }
            }
        }
    }
    order
}

/// Initial placement: shelf packing. Parts in connectivity order are laid
/// left-to-right in rows across the board width; every row is as tall as
/// its tallest member. Overlap-free and inside the board by construction
/// whenever the board is big enough, which is what makes legalisation a
/// no-op for the common case.
fn initial(pb: &Problem, snap_um: Um) -> Vec<Pose> {
    let n = pb.items.len();
    let order = connectivity_order(pb);
    let mut poses = vec![Pose { x: 0, y: 0, rot: 0 }; n];
    let (bx0, by0, bx1, by1) = pb.bbox;

    // Edge connectors first, walking the perimeter: along the top edge
    // left to right, then down the right edge, along the bottom, up the
    // left — long side along the edge, flush against it. The inner
    // rectangle left over is inset by the thickest connector on each edge.
    let mut inset = [0i64; 4]; // top, right, bottom, left
    let mut edge = 0usize;
    let mut along = 0i64;
    for &i in order.iter().filter(|&&i| pb.items[i].connector) {
        let (hw, hh) = pb.items[i].half;
        let (long, thick) = ((hw.max(hh)) * 2, (hw.min(hh)) * 2);
        let mut placed = false;
        for _ in 0..4 {
            let edge_len = if edge % 2 == 0 { bx1 - bx0 } else { by1 - by0 };
            if along + long <= edge_len || along == 0 {
                let rot: u8 = if (edge % 2 == 0) == (hw >= hh) { 0 } else { 1 };
                let c = along + long / 2;
                let (x, y) = match edge {
                    0 => (bx0 + c, by0 + thick / 2),
                    1 => (bx1 - thick / 2, by0 + c),
                    2 => (bx1 - c, by1 - thick / 2),
                    _ => (bx0 + thick / 2, by1 - c),
                };
                poses[i] = pb.flush_to_edge(i, Pose { x: snap(x, snap_um), y: snap(y, snap_um), rot }, snap_um);
                inset[edge] = inset[edge].max(thick);
                along += long;
                placed = true;
                break;
            }
            edge = (edge + 1) % 4;
            along = 0;
        }
        if !placed {
            poses[i] = pb.flush_to_edge(i, Pose { x: bx0 + long / 2, y: by0 + thick / 2, rot: 0 }, snap_um);
        }
    }

    // Everyone else: shelf packing across the inner rectangle, in
    // connectivity order so directly connected parts land adjacent.
    let (ix0, iy0, ix1) = (bx0 + inset[3], by0 + inset[0], bx1 - inset[1]);
    let mut x = ix0;
    let mut y = iy0;
    let mut row_h = 0;
    for &i in order.iter().filter(|&&i| !pb.items[i].connector) {
        let (w, h) = (pb.items[i].half.0 * 2, pb.items[i].half.1 * 2);
        if x + w > ix1 && x > ix0 {
            x = ix0;
            y += row_h;
            row_h = 0;
        }
        poses[i] = Pose { x: snap(x + w / 2, snap_um), y: snap(y + h / 2, snap_um), rot: 0 };
        x += w;
        row_h = row_h.max(h);
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
    // Floor: a six-part board gets the schedule of a 25-part one — the
    // moves are cheap and the extra length is what lets a star net fold
    // into a ring instead of freezing in the first tidy local minimum.
    let total_moves = opts.moves_per_part * n.max(25);
    let mut cur = pb.total_cost(poses);
    let t0 = (cur / n as f64).max(1000.0);
    let t_end = 10.0;
    let mut best = cur;
    let mut best_poses: Vec<Pose> = poses.to_vec();

    // Incremental RUDY-style congestion grid: bin -> demand. Rebuilt from
    // scratch here once, then kept in sync per move (only the touched
    // nets' bins change), so per-move cost stays cheap.
    let mut demand: HashMap<(i32, i32), f64> = HashMap::new();
    {
        let mut buf = Vec::new();
        for &net in &pb.congest_nets {
            buf.clear();
            pb.congest_contrib(net, poses, &mut buf);
            for &(bin, d) in &buf {
                *demand.entry(bin).or_insert(0.0) += d;
            }
        }
    }
    use std::collections::HashSet;
    let mut touched_nets: HashSet<usize> = HashSet::new();
    let mut touched_bins: HashSet<(i32, i32)> = HashSet::new();
    let mut old_contribs: Vec<((i32, i32), f64)> = Vec::new();
    let mut new_contribs: Vec<((i32, i32), f64)> = Vec::new();

    for step in 0..total_moves {
        let frac = step as f64 / total_moves.max(1) as f64;
        let temp = t0 * (t_end / t0).powf(frac);
        let reach = (span * (1.0 - frac) * 0.5).max(opts.snap as f64 * 2.0);

        let kind = rng.below(10);
        let i = rng.below(n);
        let old_i = poses[i];
        let mut j = usize::MAX;
        let mut old_j = old_i;
        if kind >= 6 && kind < 8 && n > 1 {
            j = rng.below(n);
            if j == i {
                j = (i + 1) % n;
            }
            old_j = poses[j];
        }

        // Congestion nets touched by this move, and their demand
        // contribution before it (bin_penalty uses current `demand`).
        touched_nets.clear();
        touched_bins.clear();
        old_contribs.clear();
        for &net in &pb.nets_of[i] {
            if pb.congest_nets.contains(&net) {
                touched_nets.insert(net);
            }
        }
        if j != usize::MAX {
            for &net in &pb.nets_of[j] {
                if pb.congest_nets.contains(&net) {
                    touched_nets.insert(net);
                }
            }
        }
        for &net in &touched_nets {
            let start = old_contribs.len();
            pb.congest_contrib(net, poses, &mut old_contribs);
            for &(bin, _) in &old_contribs[start..] {
                touched_bins.insert(bin);
            }
        }
        let penalty_before_congest: f64 = touched_bins.iter().map(|b| pb.bin_penalty(*demand.get(b).unwrap_or(&0.0))).sum();
        let use_before = pb.use_cost(poses);

        let before = if kind < 5 {
            let c = pb.local_cost(i, poses);
            let dx = rng.range(-(reach as i64), reach as i64 + 1);
            let dy = rng.range(-(reach as i64), reach as i64 + 1);
            poses[i] = Pose {
                x: snap((old_i.x + dx).clamp(pb.bbox.0, pb.bbox.2), opts.snap),
                y: snap((old_i.y + dy).clamp(pb.bbox.1, pb.bbox.3), opts.snap),
                rot: old_i.rot,
            };
            poses[i] = pb.flush_to_edge(i, poses[i], opts.snap);
            c
        } else if kind < 6 {
            // Jump: anywhere on the board, or — for an edge connector —
            // flush against a random edge with its long side along it.
            // Reach-limited displacement can't carry a long header across
            // a board already holding other parts; this can.
            let c = pb.local_cost(i, poses);
            let (hw, hh) = pb.items[i].half;
            if pb.items[i].connector {
                let side = rng.below(4);
                let rot: u8 = if (side < 2) == (hw >= hh) { 1 } else { 0 };
                let (w, h) = if rot == 1 { (hh * 2, hw * 2) } else { (hw * 2, hh * 2) };
                let x = match side {
                    0 => pb.bbox.0 + w / 2,
                    1 => pb.bbox.2 - w / 2,
                    _ => rng.range(pb.bbox.0 + w / 2, (pb.bbox.2 - w / 2).max(pb.bbox.0 + w / 2) + 1),
                };
                let y = match side {
                    2 => pb.bbox.1 + h / 2,
                    3 => pb.bbox.3 - h / 2,
                    _ => rng.range(pb.bbox.1 + h / 2, (pb.bbox.3 - h / 2).max(pb.bbox.1 + h / 2) + 1),
                };
                poses[i] = Pose { x: snap(x, opts.snap), y: snap(y, opts.snap), rot };
            } else {
                poses[i] = Pose {
                    x: snap(rng.range(pb.bbox.0, pb.bbox.2 + 1), opts.snap),
                    y: snap(rng.range(pb.bbox.1, pb.bbox.3 + 1), opts.snap),
                    rot: old_i.rot,
                };
            }
            c
        } else if kind < 8 && n > 1 {
            let c = pb.local_cost(i, poses) + pb.local_cost(j, poses);
            poses[i] = pb.flush_to_edge(i, Pose { x: old_j.x, y: old_j.y, rot: old_i.rot }, opts.snap);
            poses[j] = pb.flush_to_edge(j, Pose { x: old_i.x, y: old_i.y, rot: old_j.rot }, opts.snap);
            c
        } else {
            let c = pb.local_cost(i, poses);
            poses[i] = pb.flush_to_edge(i, Pose { rot: (old_i.rot + 1 + rng.below(3) as u8) % 4, ..old_i }, opts.snap);
            c
        };
        let after = if j != usize::MAX { pb.local_cost(i, poses) + pb.local_cost(j, poses) } else { pb.local_cost(i, poses) };

        new_contribs.clear();
        for &net in &touched_nets {
            let start = new_contribs.len();
            pb.congest_contrib(net, poses, &mut new_contribs);
            for &(bin, _) in &new_contribs[start..] {
                touched_bins.insert(bin);
            }
        }
        for &(bin, d) in &old_contribs {
            *demand.entry(bin).or_insert(0.0) -= d;
        }
        for &(bin, d) in &new_contribs {
            *demand.entry(bin).or_insert(0.0) += d;
        }
        let penalty_after_congest: f64 = touched_bins.iter().map(|b| pb.bin_penalty(*demand.get(b).unwrap_or(&0.0))).sum();

        // Local cost double-counts the (i,j) overlap term symmetrically on
        // both sides of the delta, so the delta stays exact.
        let delta = (after - before) + (penalty_after_congest - penalty_before_congest) + (pb.use_cost(poses) - use_before);
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
            // Undo the grid update: remove what we added, restore what we removed.
            for &(bin, d) in &new_contribs {
                *demand.entry(bin).or_insert(0.0) -= d;
            }
            for &(bin, d) in &old_contribs {
                *demand.entry(bin).or_insert(0.0) += d;
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

/// Greedy polish: for every item try each rotation at its current spot,
/// flush against every other item's courtyard on all four sides (centre-
/// and corner-aligned) and, for connectors, flush on each board edge;
/// keep the best exact-cost improvement that is overlap-free and inside
/// the board. Sweeps until nothing improves (bounded). Annealing gets
/// the global shape right; this folds nets tight in a way random
/// displacement rarely finishes — a six-way star net into a ring, a
/// stranded header onto its edge.
fn polish(pb: &Problem, poses: &mut [Pose], snap_um: Um) {
    let n = pb.items.len();
    let clean_at = |pb: &Problem, poses: &[Pose], i: usize| -> bool {
        let ci = pb.keepout(i, poses[i]);
        if outside_area(ci, pb.bbox) > 0 {
            return false;
        }
        (0..n).all(|j| j == i || overlap(ci, pb.keepout(j, poses[j])) == 0)
    };
    let cost_of = |pb: &Problem, poses: &[Pose], i: usize| pb.local_cost(i, poses) + pb.use_cost(poses) + pb.full_congestion_penalty(poses);
    for _sweep in 0..4 {
        let mut improved = false;
        for i in 0..n {
            let start = poses[i];
            let base = cost_of(pb, poses, i);
            let mut best = (base, start);
            for rot in 0..4u8 {
                let probe = Pose { rot, ..start };
                let ci = pb.keepout(i, probe);
                let (w, h) = (ci.2 - ci.0, ci.3 - ci.1);
                let mut cands: Vec<(Um, Um)> = vec![(start.x, start.y)];
                for j in 0..n {
                    if j == i {
                        continue;
                    }
                    let cj = pb.keepout(j, poses[j]);
                    let (jx, jy) = (poses[j].x, poses[j].y);
                    // Right / left / below / above of j, centre-aligned and
                    // aligned to j's near corners.
                    // Snapped outward so the snapped courtyards still
                    // touch rather than overlap.
                    for ay in [jy, cj.1 + h / 2, cj.3 - h / 2] {
                        cands.push((snap_up(cj.2 + w / 2, snap_um), ay));
                        cands.push((snap_down(cj.0 - w / 2, snap_um), ay));
                    }
                    for ax in [jx, cj.0 + w / 2, cj.2 - w / 2] {
                        cands.push((ax, snap_up(cj.3 + h / 2, snap_um)));
                        cands.push((ax, snap_down(cj.1 - h / 2, snap_um)));
                    }
                }
                // Around every pad of the other parts on this item's
                // nets: the spots a designer tries first when a stub
                // crosses or a net is long, and ones no neighbour's
                // courtyard edge happens to line up with.
                for &net in &pb.nets_of[i] {
                    for &(j, k) in &pb.nets[net] {
                        if j == i {
                            continue;
                        }
                        let q = pb.pad_center(j, k, poses[j]);
                        // Three rings out from the pad: the nearest spot
                        // is often inside a decoupling ring or another
                        // part's courtyard; the next ones are not.
                        for (dx, dy) in [(w, 0), (-w, 0), (0, h), (0, -h), (w, h), (-w, h), (w, -h), (-w, -h)] {
                            for m in [3, 6, 10] {
                                cands.push((q.x + dx * m / 4, q.y + dy * m / 4));
                            }
                        }
                    }
                }
                if pb.items[i].connector {
                    for t in [0.25, 0.5, 0.75] {
                        let x = pb.bbox.0 + ((pb.bbox.2 - pb.bbox.0) as f64 * t) as Um;
                        let y = pb.bbox.1 + ((pb.bbox.3 - pb.bbox.1) as f64 * t) as Um;
                        cands.push((snap_up(pb.bbox.0 + w / 2, snap_um), y));
                        cands.push((snap_down(pb.bbox.2 - w / 2, snap_um), y));
                        cands.push((x, snap_up(pb.bbox.1 + h / 2, snap_um)));
                        cands.push((x, snap_down(pb.bbox.3 - h / 2, snap_um)));
                    }
                }
                for (x, y) in cands {
                    poses[i] = Pose { x: snap(x, snap_um), y: snap(y, snap_um), rot };
                    if clean_at(pb, poses, i) {
                        let c = cost_of(pb, poses, i);
                        if c < best.0 - 1.0 {
                            best = (c, poses[i]);
                        }
                    }
                }
            }
            poses[i] = best.1;
            if best.0 < base - 1.0 {
                improved = true;
                if std::env::var_os("EDA_PLACE_DEBUG").is_some() {
                    eprintln!("polish: {} {:.0} -> {:.0} at ({},{}) rot {}", pb.items[i].id, base, best.0, best.1.x, best.1.y, best.1.rot);
                }
            }
        }
        if !improved {
            break;
        }
    }
}

/// Margin, µm, from the placed cluster's true courtyards to an auto-sized
/// board edge (routing channel + mechanical clearance).
const AUTO_OUTLINE_MARGIN: Um = 2000;

/// Shrink an auto-sized outline to the placed cluster: the board becomes
/// the union of true courtyards plus `AUTO_OUTLINE_MARGIN`, except on a
/// side where an edge connector is the outermost part — there the margin
/// is just the routing spacing plus edge clearance so the header stays on
/// the edge. Rounded out to a 0.5 mm grid and re-based at (0, 0). A generous square is
/// right for annealing (room to move) and wrong as a deliverable (a
/// breakout with a quarter of bare copper reads as unfinished).
fn shrink_outline(pb: &Problem, poses: &mut [Pose]) -> (Vec<Point>, (Um, Um, Um, Um)) {
    let sp = pb.spacing;
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    let mut flush = [false; 4]; // left, top, right, bottom held by a connector
    let rects: Vec<(Um, Um, Um, Um)> = pb.items.iter().zip(poses.iter()).map(|(it, p)| {
        let r = courtyard(it, *p);
        (r.0 + sp, r.1 + sp, r.2 - sp, r.3 - sp)
    }).collect();
    for r in &rects {
        x0 = x0.min(r.0);
        y0 = y0.min(r.1);
        x1 = x1.max(r.2);
        y1 = y1.max(r.3);
    }
    for (it, r) in pb.items.iter().zip(rects.iter()) {
        if it.connector {
            flush[0] |= r.0 == x0;
            flush[1] |= r.1 == y0;
            flush[2] |= r.2 == x1;
            flush[3] |= r.3 == y1;
        }
    }
    // On a tiny board the margin is capped at a quarter of the cluster so
    // the parts still span most of it (`placement_board_use`: ≥ 0.5). A
    // held side keeps the routing spacing plus KiCad's 0.5 mm copper-to-
    // edge clearance, so a track skirting the header's outer pads still
    // clears the edge, and is not rounded further out (gate
    // `placement_edge_connector`: ≤ 1.5 mm); the opposite side absorbs
    // the rounding so the board dimension still comes out on the grid.
    const HELD: Um = 1200;
    const GRID: Um = 500;
    let span_of = |lo: Um, hi: Um, held_lo: bool, held_hi: bool| -> (Um, Um) {
        let margin = AUTO_OUTLINE_MARGIN.min((hi - lo) / 4);
        match (held_lo, held_hi) {
            (true, true) => (snap_down(lo - HELD, 100), snap_up(hi + HELD, 100)),
            (true, false) => {
                let b0 = snap_down(lo - HELD, 100);
                (b0, b0 + snap_up(hi + margin - b0, GRID))
            }
            (false, true) => {
                let b1 = snap_up(hi + HELD, 100);
                (b1 - snap_up(b1 - (lo - margin), GRID), b1)
            }
            (false, false) => (snap_down(lo - margin, GRID), snap_up(hi + margin, GRID)),
        }
    };
    let (bx0, bx1) = span_of(x0, x1, flush[0], flush[2]);
    let (by0, by1) = span_of(y0, y1, flush[1], flush[3]);
    for p in poses.iter_mut() {
        p.x -= bx0;
        p.y -= by0;
    }
    let (w, h) = (bx1 - bx0, by1 - by0);
    (vec![Point { x: 0, y: 0 }, Point { x: w, y: 0 }, Point { x: w, y: h }, Point { x: 0, y: h }], (0, 0, w, h))
}

/// Push overlapping courtyards apart (each half the penetration along the
/// axis of least overlap) and pull stragglers inside the board.
/// Deterministic, bounded; returns whether the result is clean.
/// Final pass for proximity rules, measured exactly as the gate does
/// (true courtyard gap against the rule's true maximum). The anneal's
/// linear penalty leaves near misses of a few hundred µm (sweep: 5.17 mm
/// against 5, 1.62 against 1.5); here the part that is not an edge
/// connector walks toward its partner in snap steps until the rule holds
/// or the next step would overlap something or leave the board.
fn repair_rules(pb: &Problem, poses: &mut [Pose], snap_um: Um, debug: bool) {
    let n = pb.items.len();
    let clean_at = |poses: &[Pose], i: usize| -> bool {
        let ci = pb.keepout(i, poses[i]);
        outside_area(ci, pb.bbox) == 0 && (0..n).all(|j| j == i || overlap(ci, pb.keepout(j, poses[j])) == 0)
    };
    for _ in 0..3 {
        let mut any = false;
        for &(a, b, max) in &pb.pair_rules_true {
            if pb.rule_gap(a, b, poses) <= max as f64 {
                continue;
            }
            // Try the non-connector (or smaller) part first, then the
            // other one; a connector only slides along its own edge (one
            // axis), which legalize re-flushes afterwards.
            let area = |i: usize| { let c = courtyard(&pb.items[i], poses[i]); (c.2 - c.0) * (c.3 - c.1) };
            let first = match (pb.items[a].connector, pb.items[b].connector) {
                (true, false) => b,
                (false, true) => a,
                _ => if area(a) <= area(b) { a } else { b },
            };
            for mover in [first, if first == a { b } else { a }] {
            let axis_only = pb.items[mover].connector;
            // Windowed search: the best clean pose for the mover within
            // ±8 snaps, then repeat from there. A single axis step is
            // often blocked by a neighbour's keepout while a diagonal or
            // a two-snap hop is not.
            for _round in 0..6 {
                let d0 = pb.rule_gap(a, b, poses);
                if d0 <= max as f64 {
                    break;
                }
                let start = poses[mover];
                let mut best: Option<(f64, Pose)> = None;
                for rot in 0..4u8 {
                if axis_only && rot != start.rot { continue }
                for sx in -40..=40i64 {
                    for sy in -40..=40i64 {
                        if sx == 0 && sy == 0 && rot == start.rot { continue }
                        if axis_only && sx != 0 && sy != 0 { continue }
                        let probe = Pose { x: start.x + sx * snap_um, y: start.y + sy * snap_um, rot };
                        poses[mover] = probe;
                        if clean_at(poses, mover) {
                            let d = pb.rule_gap(a, b, poses);
                            if d < d0 && best.map_or(true, |(bd, _)| d < bd) {
                                best = Some((d, probe));
                            }
                        }
                    }
                }
                }
                match best {
                    Some((_, p)) => { poses[mover] = p; any = true; }
                    None => { poses[mover] = start; break; }
                }
            }
            if pb.rule_gap(a, b, poses) <= max as f64 { break; }
            }
            if debug {
                eprintln!("place debug: rule {}-{} gap {:.0} (max {max}) after repair", pb.items[a].id, pb.items[b].id, pb.rule_gap(a, b, poses));
            }
        }
        if !any {
            break;
        }
    }
}

fn legalize(pb: &Problem, poses: &mut [Pose], snap_um: Um) -> bool {
    let n = pb.items.len();
    let clamp_inside = |pb: &Problem, poses: &mut [Pose], i: usize| {
        let ci = pb.keepout(i, poses[i]);
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
                let ci = pb.keepout(i, poses[i]);
                let cj = pb.keepout(j, poses[j]);
                if overlap(ci, cj) == 0 {
                    continue;
                }
                let px = (ci.2 - cj.0).min(cj.2 - ci.0);
                let py = (ci.3 - cj.1).min(cj.3 - ci.1);
                let dir_x: i64 = if poses[j].x >= poses[i].x { 1 } else { -1 };
                let dir_y: i64 = if poses[j].y >= poses[i].y { 1 } else { -1 };
                // An edge connector is an anchor: the other part takes
                // the whole push, and a connector that does move is
                // re-flushed so it never drifts off its edge.
                let (ci_conn, cj_conn) = (pb.items[i].connector, pb.items[j].connector);
                let (wi, wj) = match (ci_conn, cj_conn) {
                    (true, false) => (0, 2),
                    (false, true) => (2, 0),
                    _ => (1, 1),
                };
                // Two connectors on one edge separate along that edge
                // (their centres differ most along it); pushing them
                // apart across it would only bounce off the board edge.
                let along_x = if ci_conn && cj_conn { (poses[i].x - poses[j].x).abs() >= (poses[i].y - poses[j].y).abs() } else { px <= py };
                if along_x {
                    let step = px / 2 + snap_um;
                    poses[j].x = snap(poses[j].x + dir_x * step * wj, snap_um);
                    poses[i].x = snap(poses[i].x - dir_x * step * wi, snap_um);
                } else {
                    let step = py / 2 + snap_um;
                    poses[j].y = snap(poses[j].y + dir_y * step * wj, snap_um);
                    poses[i].y = snap(poses[i].y - dir_y * step * wi, snap_um);
                }
                poses[i] = pb.flush_to_edge(i, poses[i], snap_um);
                poses[j] = pb.flush_to_edge(j, poses[j], snap_um);
                moved = true;
            }
        }
        if !moved {
            return true;
        }
    }
    // Final verdict.
    for i in 0..n {
        let ci = pb.keepout(i, poses[i]);
        if outside_area(ci, pb.bbox) > 0 {
            return false;
        }
        for j in i + 1..n {
            if overlap(ci, pb.keepout(j, poses[j])) > 0 {
                return false;
            }
        }
    }
    true
}

/// A placement generator. Every placer — ours, Cypress, anything wrapped —
/// speaks this so the loop can run N candidates from any mix and let the
/// gates and critic choose. Options live on the implementor.
pub trait Placer {
    fn name(&self) -> &str;
    fn place(&self, design: &Design, model: &ConstraintModel, seed: u64) -> Result<Design, Vec<CheckResult>>;
}

/// `eda-place`'s own annealer as a `Placer`.
pub struct Anneal(pub PlaceOptions);

impl Placer for Anneal {
    fn name(&self) -> &str {
        "eda-place"
    }
    fn place(&self, design: &Design, model: &ConstraintModel, seed: u64) -> Result<Design, Vec<CheckResult>> {
        place(design, model, &PlaceOptions { seed, ..self.0.clone() })
    }
}

/// Place every part of `model` on the board. `design` supplies provenance
/// and (if present) the schematic section, which is carried through
/// untouched; the returned design has a fresh `placement` section.
pub fn place(design: &Design, model: &ConstraintModel, opts: &PlaceOptions) -> Result<Design, Vec<CheckResult>> {
    let pb = build_problem(model, opts)?;
    let debug = std::env::var_os("EDA_PLACE_DEBUG").is_some();
    let t0 = std::time::Instant::now();
    let mut poses = initial(&pb, opts.snap);
    anneal(&pb, &mut poses, opts);
    if debug {
        eprintln!("place debug: anneal {:.1}s", t0.elapsed().as_secs_f64());
    }
    let mut clean = legalize(&pb, &mut poses, opts.snap);
    // A run that legalisation can't clean up is a bad local minimum, not a
    // verdict on the board: re-anneal from the shelf pack under other
    // seeds before giving up.
    for retry in 1..=3u64 {
        if clean {
            break;
        }
        poses = initial(&pb, opts.snap);
        anneal(&pb, &mut poses, &PlaceOptions { seed: opts.seed.wrapping_add(retry.wrapping_mul(0x9E37_79B9)), ..opts.clone() });
        clean = legalize(&pb, &mut poses, opts.snap);
    }
    if clean {
        let t1 = std::time::Instant::now();
        polish(&pb, &mut poses, opts.snap);
        clean = legalize(&pb, &mut poses, opts.snap);
        if clean {
            repair_rules(&pb, &mut poses, opts.snap, debug);
            clean = legalize(&pb, &mut poses, opts.snap);
        }
        if debug {
            eprintln!("place debug: polish {:.1}s", t1.elapsed().as_secs_f64());
        }
    }

    if std::env::var_os("EDA_PLACE_DEBUG").is_some() {
        for si in 0..pb.stubs.len() {
            for ti in si + 1..pb.stubs.len() {
                if !(pb.stub_free[si] && pb.stub_free[ti]) {
                    continue;
                }
                let ((a, ka, b, kb), (c, kc, d, kd)) = (pb.stubs[si], pb.stubs[ti]);
                let (p, q) = (pb.pad_center(a, ka, poses[a]), pb.pad_center(b, kb, poses[b]));
                let (u, v) = (pb.pad_center(c, kc, poses[c]), pb.pad_center(d, kd, poses[d]));
                if segments_cross(p, q, u, v) {
                    eprintln!("place debug: crossing {}.{}-{}.{} x {}.{}-{}.{}", pb.items[a].id, pb.items[a].pads[ka].0, pb.items[b].id, pb.items[b].pads[kb].0, pb.items[c].id, pb.items[c].pads[kc].0, pb.items[d].id, pb.items[d].pads[kd].0);
                }
            }
        }
        let b = pb.breakdown(&poses);
        eprintln!("place debug: cost hpwl {:.0} overlap/outside {:.0} rules {:.0} escape {:.0} edge {:.0} cross {:.0} use {:.0} congest {:.0}", b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]);
    }
    let mut outline = pb.outline.clone();
    if clean && pb.auto_outline {
        let (o, _) = shrink_outline(&pb, &mut poses);
        outline = o;
    }
    let mut footprints: Vec<FootprintInstance> = pb.items.iter().zip(poses.iter()).map(|(it, p)| instance(it, *p)).collect();
    footprints.sort_by(|a, b| a.id.cmp(&b.id));

    let mut out = design.clone();
    out.provenance.seed = opts.seed;
    out.placement = Some(PlacementSection { outline, footprints });
    out.routing = None;

    if !clean {
        let mut fails = vec![CheckResult::fail("place_legalize", "design", "could not resolve all courtyard overlaps within the outline; enlarge the board or relax rules")];
        if std::env::var_os("EDA_PLACE_DEBUG").is_some() {
            for (it, p) in pb.items.iter().zip(poses.iter()) {
                eprintln!("place debug: {} at ({},{}) rot {} courtyard {:?}", it.id, p.x, p.y, p.rot, courtyard(it, *p));
            }
        }
        // Report the offenders so the caller can act.
        for i in 0..pb.items.len() {
            let ci = pb.keepout(i, poses[i]);
            let outside = outside_area(ci, pb.bbox);
            if outside > 0 {
                fails.push(CheckResult::fail("place_outside", &pb.items[i].id, format!("{outside} µm² outside the board (courtyard {ci:?}, board {:?})", pb.bbox)));
            }
            for j in i + 1..pb.items.len() {
                let ov = overlap(pb.keepout(i, poses[i]), pb.keepout(j, poses[j]));
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
