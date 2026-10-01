//! `eda-cypress` — NVlabs Cypress (ISPD'25, DREAMPlace-based analytical
//! placer) as a workspace generator behind the same `Placer` interface as
//! `eda-place`.
//!
//! Today this drives a native CPU build of Cypress as a subprocess through
//! the Bookshelf bridge (`eda_interchange::bookshelf`): our IR out, its
//! `.gp.pl` back in, our gates judge the result. The subprocess boundary is
//! deliberate — "wrap, don't swallow" — until the placer math (electrostatic
//! density via FFT, weighted-average wirelength, Nesterov) is ported into
//! Rust, at which point this crate keeps its API and drops the Python.
//!
//! Locate the build with `CYPRESS_INSTALL` (default `~/ws/Cypress/install`)
//! and `CYPRESS_PYTHON` (default `python`, i.e. whatever env is active).

use eda_interchange::bookshelf::{from_bookshelf_pl_fixed, to_bookshelf_fixed, Bookshelf, EDGE_PIN_GAP_UM};
use eda_model::footprint::{is_edge_connector, placed_courtyard, placed_keepout, placed_pads, usable_edges};
use eda_model::ir::{Design, LabelSide, Point};
use std::collections::BTreeSet;
use eda_model::board::{fit_outline, trim_empty_edges};
use eda_model::{CheckResult, ConstraintModel, PlacementRule};
use eda_place::Placer;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Bookshelf unit used for the bridge, µm.
pub const UNIT_UM: i64 = 100;

#[derive(Debug, Clone)]
pub struct CypressOptions {
    /// See `SolverSettings::fit_board_utilization`; 0 keeps the outline.
    pub fit_board_utilization: f64,
    pub install: PathBuf,
    pub python: String,
    /// Scratch directory for Bookshelf files, config and results.
    pub work_dir: PathBuf,
    pub gpu: bool,
    pub bins: u32,
    pub target_density: f64,
    pub iterations: u32,
    /// Cypress' own stopping criterion (density overflow). For macro-only
    /// PCBs the metric has a geometric floor, so this is a knob, not truth:
    /// legality is judged by our gates after Cypress' legaliser.
    /// `CYPRESS_STOP_OVERFLOW` overrides.
    pub stop_overflow: f64,
    /// Allow 90° rotations (the Cypress PCB feature).
    pub rotation: bool,
    /// `.wts` weight given to the synthetic 2-pin net synthesised per
    /// `placement_rules: Proximity` rule (see
    /// `eda_interchange::bookshelf::PROXIMITY_NET_PREFIX`), so the pair is
    /// pulled together during global placement instead of just being
    /// checked afterward. `CYPRESS_PROXIMITY_WEIGHT` overrides; swept
    /// empirically (10x/50x/200x) against `l4_control_hub`'s 17
    /// `placement_proximity` failures — 50 binds the great majority without
    /// visibly hurting HPWL, 200 does no better. See bench notes.
    pub proximity_weight: f64,
}

impl Default for CypressOptions {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        CypressOptions {
            fit_board_utilization: 0.25,
            install: std::env::var("CYPRESS_INSTALL").map(PathBuf::from).unwrap_or_else(|_| Path::new(&home).join("ws/Cypress/install")),
            python: std::env::var("CYPRESS_PYTHON").unwrap_or_else(|_| "python".into()),
            work_dir: std::env::temp_dir().join("eda-cypress"),
            gpu: false,
            bins: 64,
            target_density: 0.5,
            iterations: 3000,
            stop_overflow: std::env::var("CYPRESS_STOP_OVERFLOW").ok().and_then(|v| v.parse().ok()).unwrap_or(0.30),
            rotation: true,
            proximity_weight: std::env::var("CYPRESS_PROXIMITY_WEIGHT").ok().and_then(|v| v.parse().ok()).unwrap_or(50.0),
        }
    }
}

impl CypressOptions {
    pub fn available(&self) -> bool {
        self.install.join("dreamplace/Placer.py").exists()
    }
}

/// DREAMPlace/Cypress config JSON (keys mirror test/simple.json and the
/// PCB tuner's defaults).
pub fn config_json(aux: &Path, result_dir: &Path, seed: u64, o: &CypressOptions) -> serde_json::Value {
    serde_json::json!({
        "aux_input": aux,
        "gpu": if o.gpu { 1 } else { 0 },
        "num_bins_x": o.bins, "num_bins_y": o.bins,
        // Cadence/weights follow the Cypress PCB tuner (artifacts/.../run-*.json):
        // slow density-weight schedule, lr decay, macro overlap penalty.
        "global_place_stages": [{
            "num_bins_x": o.bins, "num_bins_y": o.bins, "iteration": o.iterations,
            "learning_rate": 0.0038, "learning_rate_decay": 0.993,
            "wirelength": "weighted_average", "optimizer": "nesterov",
            "Llambda_density_weight_iteration": 10, "Lsub_iteration": 2
        }],
        "target_density": o.target_density, "density_weight": 0.0237, "gamma": 0.44,
        "macro_overlap_flag": 1, "macro_overlap_weight": 8e-6, "macro_halo_x": 1, "macro_halo_y": 1,
        "enable_rotation": if o.rotation { 1 } else { 0 },
        "random_seed": seed, "scale_factor": 1.0, "ignore_net_degree": 100,
        "enable_fillers": 1, "gp_noise_ratio": 0.025, "global_place_flag": 1,
        "legalize_flag": 1, "detailed_place_flag": 0, "stop_overflow": o.stop_overflow,
        "dtype": "float32", "plot_flag": 0, "result_dir": result_dir,
        // Same seed, same answer: needed for reproducible gating.
        "deterministic_flag": 1, "num_threads": 1
    })
}

/// Inspect Cypress' log for a diverged or skipped run. `Final PPA` carries
/// the final HPWL; `inf`/`nan` there means the optimiser blew up, and
/// "skip legalization" means it never produced a legal placement.
pub fn check_cypress_log(log: &str) -> Result<(), String> {
    let final_line = log.lines().rev().find(|l| l.contains("Final PPA")).ok_or_else(|| "no `Final PPA` line in DREAMPlace.log (run did not finish)".to_string())?;
    let hpwl = final_line.split("'hpwl': ").nth(1).and_then(|r| r.split(',').next()).map(|v| v.trim().to_string()).unwrap_or_default();
    if hpwl.is_empty() || hpwl.contains("inf") || hpwl.contains("nan") {
        return Err(format!("global placement diverged (final hpwl = {hpwl:?})"));
    }
    if log.contains("skip legalization") {
        return Err("Cypress skipped legalization (overflow/hpwl invalid), placement not legal".to_string());
    }
    Ok(())
}

fn io_fail(what: &str, e: impl std::fmt::Display) -> Vec<CheckResult> {
    vec![CheckResult::fail("cypress_io", what, e.to_string())]
}

// ------------------------------------------------------- proximity legalizer

/// Point-in-polygon (even-odd rule), duplicated in miniature from
/// `eda-gates` rather than depending on it from the placement bridge (see
/// module docs — the crate graph has no cycle either way, but the gate is
/// the caller's contract to define, not this crate's).
fn point_in_polygon(p: Point, poly: &[Point]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (poly[i].x, poly[i].y);
        let (xj, yj) = (poly[j].x, poly[j].y);
        if (yi > p.y) != (yj > p.y) {
            let x_int = xi as f64 + ((p.y - yi) as f64) * ((xj - xi) as f64) / ((yj - yi) as f64);
            if (p.x as f64) < x_int {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Edge-to-edge gap between two axis-aligned courtyard rects (µm), 0 when
/// they overlap on an axis. Matches `eda-gates`' `Rect::gap` semantics for
/// the `placement_proximity` check.
fn rect_gap(a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)) -> f64 {
    let dx = (a.0 - b.2).max(b.0 - a.2).max(0);
    let dy = (a.1 - b.3).max(b.1 - a.3).max(0);
    ((dx * dx + dy * dy) as f64).sqrt()
}

fn rect_overlaps(a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
}

/// Built-in legality check used automatically by `place_with_cypress`:
/// every courtyard corner inside the outline, no same-side courtyard
/// overlap. Independent of `eda-gates` so this crate can run the pass
/// without adding that dependency; a caller wanting the exact gate
/// semantics (e.g. clearance, connectivity) can pass its own closure to
/// `legalize_proximity_pass` instead.
pub fn default_is_legal(design: &Design, model: &ConstraintModel) -> bool {
    let Some(placement) = design.placement.as_ref() else { return false };
    let mut courtyards: Vec<(&str, eda_model::ir::Side, (i64, i64, i64, i64))> = Vec::new();
    for f in &placement.footprints {
        let Some(part) = model.part(&f.id) else { return false };
        let Some(ct) = placed_courtyard(model, part, f) else { return false };
        let Some(ko) = placed_keepout(model, &placement.outline, part, f) else { return false };
        let corners = [
            Point { x: ct.0, y: ct.1 },
            Point { x: ct.2, y: ct.1 },
            Point { x: ct.2, y: ct.3 },
            Point { x: ct.0, y: ct.3 },
        ];
        if !corners.iter().all(|c| point_in_polygon(*c, &placement.outline)) {
            return false;
        }
        courtyards.push((f.id.as_str(), f.side, ko));
    }
    // Keep-outs (courtyard + refdes label) must not overlap on a side,
    // mirroring `placement_courtyard_overlap` + `placement_refdes_clear`.
    for i in 0..courtyards.len() {
        for j in i + 1..courtyards.len() {
            if courtyards[i].1 == courtyards[j].1 && rect_overlaps(courtyards[i].2, courtyards[j].2) {
                return false;
            }
        }
    }
    true
}

fn dbg_on() -> bool {
    std::env::var("CYPRESS_DEBUG_LEGALIZE").is_ok()
}

fn rect_expand(ct: (i64, i64, i64, i64), by: i64) -> (i64, i64, i64, i64) {
    (ct.0 - by, ct.1 - by, ct.2 + by, ct.3 + by)
}

fn clamp_box(b: (i64, i64, i64, i64), bounds: (i64, i64, i64, i64)) -> (i64, i64, i64, i64) {
    (b.0.max(bounds.0), b.1.max(bounds.1), b.2.min(bounds.2), b.3.min(bounds.3))
}

/// Gap-satisfying + legal grid search for moving a single part. Tries
/// every `step_um` position in `search_box` (clamped to the board
/// bounds), all four rotations, ordered by increasing displacement from
/// `mover_fp`'s current position (ties broken by rotation then x then y,
/// so results are deterministic). `gap_check` — `(other_ct, max_mm)` —
/// additionally requires the candidate stay within `max_mm` of a fixed
/// anchor courtyard (used for the proximity rule itself); `avoid` rects
/// are extra no-go zones (used to keep a ripple-displaced neighbour off
/// the spot just freed for the rule's mover). Returns the chosen
/// `(x, y, rot)` — the caller applies it and re-checks whatever it needs
/// to (this function only proves *a* legal spot exists, not that the
/// whole multi-part edit is acceptable).
#[allow(clippy::too_many_arguments)]
fn find_spot(
    model: &ConstraintModel,
    base: &Design,
    mover_id: &str,
    mover_part: &eda_model::Part,
    mover_fp: &eda_model::ir::FootprintInstance,
    search_box: (i64, i64, i64, i64),
    gap_check: Option<((i64, i64, i64, i64), f64)>,
    avoid: &[(i64, i64, i64, i64)],
    step_um: i64,
    is_legal: &dyn Fn(&Design) -> bool,
) -> Option<(i64, i64, i32)> {
    let (sx0, sy0, sx1, sy1) = search_box;
    if sx0 > sx1 || sy0 > sy1 {
        return None;
    }
    let outline = &base.placement.as_ref()?.outline;
    let mut candidates: Vec<(i64, i32, i64, i64)> = Vec::new(); // (dist2, rot, x, y)
    let mut x = sx0;
    while x <= sx1 {
        let mut y = sy0;
        while y <= sy1 {
            for rot in [0i32, 90_000, 180_000, 270_000] {
                let (dx, dy) = (x - mover_fp.at.x, y - mover_fp.at.y);
                candidates.push((dx * dx + dy * dy, rot, x, y));
            }
            y += step_um;
        }
        x += step_um;
    }
    candidates.sort_by_key(|c| (c.0, c.1, c.2, c.3));

    // Cheap rejection before the full legality check: the mover's keep-out
    // against every other same-side keep-out and the board bbox, computed
    // once. Without it an unsatisfiable rule on the 25 um last-resort grid
    // cloned the design and ran the O(n^2) check for ~2M candidates (l3:
    // the legaliser sat at 100% CPU for 36 minutes on one rule).
    let placement = base.placement.as_ref()?;
    let mover_side = placement.footprints.iter().find(|f| f.id == mover_id)?.side;
    let others: Vec<(i64, i64, i64, i64)> = placement
        .footprints
        .iter()
        .filter(|f| f.id != mover_id && f.side == mover_side)
        .filter_map(|f| placed_keepout(model, outline, model.part(&f.id)?, f))
        .collect();
    // An empty outline used to yield a board spanning the whole i64
    // range, on which every candidate is trivially in bounds and the
    // width arithmetic below overflows. Placement without a board shape
    // is not a thing to cope with: `check_placement_locality` fails it,
    // and getting here means that gate was bypassed.
    assert!(outline.len() >= 3, "legalise needs a board outline, got {} point(s)", outline.len());
    let (bx0, by0, bx1, by1) = (
        outline.iter().map(|p| p.x).min().expect("outline non-empty"),
        outline.iter().map(|p| p.y).min().expect("outline non-empty"),
        outline.iter().map(|p| p.x).max().expect("outline non-empty"),
        outline.iter().map(|p| p.y).max().expect("outline non-empty"),
    );

    for (_, rot, x, y) in candidates {
        let mut candidate_fp = mover_fp.clone();
        candidate_fp.at = Point { x, y };
        candidate_fp.rot = rot as u32;
        let Some(mover_ct) = placed_courtyard(model, mover_part, &candidate_fp) else { continue };
        let Some(mover_ko) = placed_keepout(model, outline, mover_part, &candidate_fp) else { continue };
        if let Some((other_ct, max_mm)) = gap_check {
            if rect_gap(mover_ct, other_ct) / 1000.0 > max_mm {
                continue;
            }
        }
        if avoid.iter().any(|r| rect_overlaps(mover_ko, *r)) {
            continue;
        }
        if mover_ct.0 < bx0 || mover_ct.1 < by0 || mover_ct.2 > bx1 || mover_ct.3 > by1 || others.iter().any(|o| rect_overlaps(mover_ko, *o)) {
            continue;
        }
        let mut trial = base.clone();
        {
            let tp = trial.placement.as_mut().unwrap();
            let f = tp.footprints.iter_mut().find(|f| f.id == mover_id).unwrap();
            f.at = Point { x, y };
            f.rot = rot as u32;
        }
        if is_legal(&trial) {
            return Some((x, y, rot));
        }
    }
    None
}

/// Number of `Proximity` rules touching `id` that `design` violates.
fn violations_for(model: &ConstraintModel, design: &Design, id: &str) -> usize {
    let pl = design.placement.as_ref().unwrap();
    let ct = |r: &str| pl.footprints.iter().find(|f| f.id == r).and_then(|f| placed_courtyard(model, model.part(r)?, f));
    model
        .placement_rules
        .iter()
        .filter(|r| match r {
            PlacementRule::Proximity { a, b, max_mm, .. } if a == id || b == id => match (ct(a), ct(b)) {
                (Some(ca), Some(cb)) => rect_gap(ca, cb) / 1000.0 > *max_mm,
                _ => false,
            },
            _ => false,
        })
        .count()
}

/// True when every `Proximity` rule touching any of `ids` is satisfied in
/// `design` — used after a ripple move to confirm a displaced neighbour
/// (or the mover/anchor) didn't break a rule of its own.
fn rules_ok_for(model: &ConstraintModel, design: &Design, ids: &[&str]) -> bool {
    let Some(placement) = design.placement.as_ref() else { return false };
    let fp_of = |id: &str| placement.footprints.iter().find(|f| f.id == id);
    for rule in &model.placement_rules {
        let PlacementRule::Proximity { a, b, max_mm, .. } = rule else { continue };
        if !ids.contains(&a.as_str()) && !ids.contains(&b.as_str()) {
            continue;
        }
        let (Some(fa), Some(fb)) = (fp_of(a), fp_of(b)) else { continue };
        let (Some(pa), Some(pb)) = (model.part(a), model.part(b)) else { continue };
        let (Some(ca), Some(cb)) = (placed_courtyard(model, pa, fa), placed_courtyard(model, pb, fb)) else { continue };
        if rect_gap(ca, cb) / 1000.0 > *max_mm {
            return false;
        }
    }
    true
}

fn apply_move(design: &mut Design, id: &str, x: i64, y: i64, rot: i32) {
    let f = design.placement.as_mut().unwrap().footprints.iter_mut().find(|f| f.id == id).unwrap();
    f.at = Point { x, y };
    f.rot = rot as u32;
}

/// Bounded ripple: for the rule's mover, find the nearest candidate spot
/// that satisfies the gap (regardless of legality), see which other
/// placed parts (courtyard-overlap, same side) block it, and — if there
/// are at most `MAX_BLOCKERS` of them — try to relocate each blocker to
/// its own nearest legal spot elsewhere (avoiding the mover's target),
/// then place the mover. Accepted only if the whole design is legal
/// afterward *and* every rule touching the mover, anchor, or any moved
/// blocker still holds; otherwise the attempt (and any partial move) is
/// discarded — nothing here mutates `out` until every part of the ripple
/// has checked out.
#[allow(clippy::too_many_arguments)]
fn try_ripple(
    model: &ConstraintModel,
    out: &Design,
    mover_id: &str,
    anchor_id: &str,
    mover_part: &eda_model::Part,
    mover_fp: &eda_model::ir::FootprintInstance,
    anchor_ct: (i64, i64, i64, i64),
    max_mm: f64,
    bounds: (i64, i64, i64, i64),
    max_um: i64,
    step_um: i64,
    is_legal: &dyn Fn(&Design) -> bool,
) -> Option<Design> {
    const MAX_BLOCKERS: usize = 2;
    const RIPPLE_RADIUS_UM: i64 = 6_000; // how far a blocker may look for its own new spot
    // Each attempted candidate costs up to MAX_BLOCKERS full find_spot
    // searches; an unsatisfiable rule otherwise walks every candidate in
    // the halo (l3 U7/C15: the legaliser sat in this loop for 15+ min).
    const MAX_RIPPLE_TRIES: usize = 40;
    let mut tries = 0usize;

    let search_box = clamp_box(rect_expand(anchor_ct, max_um), bounds);
    // Enumerate gap-ok candidates for the mover (legality ignored), same
    // deterministic order as `find_spot`, and take the nearest one whose
    // blocker set is small enough to move.
    let (sx0, sy0, sx1, sy1) = search_box;
    if sx0 > sx1 || sy0 > sy1 {
        return None;
    }
    let mut candidates: Vec<(i64, i32, i64, i64)> = Vec::new();
    let mut x = sx0;
    while x <= sx1 {
        let mut y = sy0;
        while y <= sy1 {
            for rot in [0i32, 90_000, 180_000, 270_000] {
                let (dx, dy) = (x - mover_fp.at.x, y - mover_fp.at.y);
                candidates.push((dx * dx + dy * dy, rot, x, y));
            }
            y += step_um;
        }
        x += step_um;
    }
    candidates.sort_by_key(|c| (c.0, c.1, c.2, c.3));

    let placement = out.placement.as_ref()?;
    let mover_side = placement.footprints.iter().find(|f| f.id == mover_id)?.side;

    for (_, rot, x, y) in candidates {
        let mut candidate_fp = mover_fp.clone();
        candidate_fp.at = Point { x, y };
        candidate_fp.rot = rot as u32;
        let Some(mover_ct) = placed_courtyard(model, mover_part, &candidate_fp) else { continue };
        let Some(mover_ko) = placed_keepout(model, &placement.outline, mover_part, &candidate_fp) else { continue };
        if rect_gap(mover_ct, anchor_ct) / 1000.0 > max_mm {
            continue;
        }
        // Blockers: other placed parts (same side) whose courtyard
        // overlaps this candidate spot.
        let mut blockers: Vec<String> = Vec::new();
        let mut ok = true;
        for f in &placement.footprints {
            if f.id == mover_id || f.id == anchor_id || f.side != mover_side {
                continue;
            }
            let Some(part) = model.part(&f.id) else { continue };
            let Some(ct) = placed_keepout(model, &placement.outline, part, f) else { continue };
            if rect_overlaps(mover_ko, ct) {
                blockers.push(f.id.clone());
                if blockers.len() > MAX_BLOCKERS {
                    ok = false;
                    break;
                }
            }
        }
        if !ok || blockers.is_empty() {
            continue;
        }
        tries += 1;
        if tries > MAX_RIPPLE_TRIES {
            return None;
        }

        // Relocate each blocker in turn, chaining through `work` so later
        // blockers see earlier ones' new positions.
        let mut work = out.clone();
        let mut moved_ok = true;
        for bid in &blockers {
            let bf = work.placement.as_ref().unwrap().footprints.iter().find(|f| &f.id == bid).unwrap().clone();
            let Some(bpart) = model.part(bid) else {
                moved_ok = false;
                break;
            };
            let Some(bct) = placed_courtyard(model, bpart, &bf) else {
                moved_ok = false;
                break;
            };
            let bsearch = clamp_box(rect_expand(bct, RIPPLE_RADIUS_UM), bounds);
            match find_spot(model, &work, bid, bpart, &bf, bsearch, None, &[mover_ko], step_um, is_legal) {
                Some((bx, by, brot)) => apply_move(&mut work, bid, bx, by, brot),
                None => {
                    moved_ok = false;
                    break;
                }
            }
        }
        if !moved_ok {
            continue; // discard `work`, try the next candidate
        }
        apply_move(&mut work, mover_id, x, y, rot as i32);
        if !is_legal(&work) {
            continue;
        }
        let mut touched: Vec<&str> = vec![mover_id, anchor_id];
        touched.extend(blockers.iter().map(|s| s.as_str()));
        if rules_ok_for(model, &work, &touched) {
            return Some(work);
        }
        // Otherwise discard `work` (rolled back by not assigning it) and
        // try the next candidate spot.
    }
    None
}

/// Deterministic post-pass: for every `placement_rules: Proximity` rule
/// still violated after Cypress' global placement, escalate through:
/// 1. move the *smaller* part (by courtyard area) to the nearest legal
///    spot within `max_mm` of the *larger* one (100 µm grid, rotations
///    0/90/180/270);
/// 2. the same search with the roles swapped — move the larger part
///    toward the smaller one instead, in case the smaller one's
///    neighbourhood is the tight one;
/// 3. a bounded ripple: displace up to two blocking neighbours of the
///    smaller part's best candidate spot to their own nearest legal
///    positions, then place the smaller part — accepted only if the
///    whole design stays legal and every rule touching the mover,
///    anchor, or a moved neighbour still holds (atomic: nothing is kept
///    unless the entire chain checks out);
/// 4. a last-resort finer/wider retry of (1) and (2): 25 µm grid, 1.5×
///    the halo.
///
/// Every step is fully deterministic (fixed enumeration order, first
/// success wins) so the same input always yields the same output. No
/// fallback: a pair nothing above can satisfy is left exactly as it was
/// (Cypress' raw placement, or wherever an earlier successful step left
/// it), so `placement_proximity` still fails on it — that failure is the
/// signal, not something to paper over.
///
/// `is_legal` takes the whole candidate `Design` so a caller can supply
/// its own definition (e.g. wired to `eda-gates`' `check_placement`)
/// instead of the crate-local `default_is_legal`.
pub fn legalize_proximity_pass(design: &Design, model: &ConstraintModel, is_legal: &dyn Fn(&Design) -> bool) -> Design {
    legalize_proximity_pass_frozen(design, model, is_legal, &BTreeSet::new())
}

/// [`legalize_proximity_pass`] where `frozen` parts (pinned edge
/// connectors) are never moved: a rule against a frozen part can only be
/// met by moving the other part, so the search never even enumerates
/// candidates for the frozen one (which used to burn half an hour on a
/// 25 µm grid proving the obvious).
pub fn legalize_proximity_pass_frozen(design: &Design, model: &ConstraintModel, is_legal: &dyn Fn(&Design) -> bool, frozen: &BTreeSet<String>) -> Design {
    const STEP_UM: i64 = 100;
    const FINE_STEP_UM: i64 = 25;
    let mut out = design.clone();
    let Some(placement) = out.placement.clone() else { return out };
    let outline = &placement.outline;
    let (Some(bmin_x), Some(bmin_y), Some(bmax_x), Some(bmax_y)) = (
        outline.iter().map(|p| p.x).min(),
        outline.iter().map(|p| p.y).min(),
        outline.iter().map(|p| p.x).max(),
        outline.iter().map(|p| p.y).max(),
    ) else {
        return out;
    };
    let bounds = (bmin_x, bmin_y, bmax_x, bmax_y);

    if dbg_on() {
        eprintln!("DEBUG_LEGALIZE baseline is_legal(out)={}", is_legal(&out));
    }
    for rule in &model.placement_rules {
        let PlacementRule::Proximity { a, b, max_mm, .. } = rule else { continue };
        let max_um = (*max_mm * 1000.0) as i64;
        let (Some(fa), Some(fb)) =
            (out.placement.as_ref().unwrap().footprints.iter().find(|f| &f.id == a).cloned(),
             out.placement.as_ref().unwrap().footprints.iter().find(|f| &f.id == b).cloned())
        else {
            continue;
        };
        let (Some(pa), Some(pb)) = (model.part(a), model.part(b)) else { continue };
        let (Some(ca), Some(cb)) = (placed_courtyard(model, pa, &fa), placed_courtyard(model, pb, &fb)) else { continue };
        if rect_gap(ca, cb) / 1000.0 <= *max_mm {
            continue; // already satisfied, don't perturb a fine placement
        }

        let area = |ct: (i64, i64, i64, i64)| (ct.2 - ct.0) as i64 * (ct.3 - ct.1) as i64;
        let (small_id, small_fp, small_part, small_ct, large_id, large_fp, large_part, large_ct) = if area(ca) <= area(cb) {
            (a.as_str(), fa.clone(), pa, ca, b.as_str(), fb.clone(), pb, cb)
        } else {
            (b.as_str(), fb.clone(), pb, cb, a.as_str(), fa.clone(), pa, ca)
        };

        let (small_frozen, large_frozen) = (frozen.contains(small_id), frozen.contains(large_id));
        if small_frozen && large_frozen {
            continue;
        }
        let mut resolved = false;
        let t0 = std::time::Instant::now();
        let step = |n: u32, t: &std::time::Instant| {
            if dbg_on() {
                eprintln!("DEBUG_LEGALIZE   {a}/{b} step {n} done at {:?}", t.elapsed());
            }
        };

        // A move of the smaller part must not leave more of its own rules
        // broken than before (a cap with two anchors would otherwise be
        // dragged back and forth, each rule undoing the last).
        let before = violations_for(model, &out, small_id);
        let accept_small = |trial: &Design| violations_for(model, trial, small_id) < before;

        // 1. Move the smaller part toward the larger (fixed) one.
        let box1 = clamp_box(rect_expand(large_ct, max_um), bounds);
        if small_frozen {
            // nothing: only the larger part may move
        } else if let Some((x, y, rot)) = find_spot(model, &out, small_id, small_part, &small_fp, box1, Some((large_ct, *max_mm)), &[], STEP_UM, is_legal) {
            let mut trial = out.clone();
            apply_move(&mut trial, small_id, x, y, rot);
            if accept_small(&trial) {
                out = trial;
                resolved = true;
            }
        }

        step(1, &t0);
        // 2. Move the larger part toward the smaller (fixed) one instead —
        // only accepted if it doesn't break one of the larger part's own
        // other rules (e.g. dragging it out of range of a different anchor).
        if !resolved && !large_frozen {
            let box2 = clamp_box(rect_expand(small_ct, max_um), bounds);
            if let Some((x, y, rot)) = find_spot(model, &out, large_id, large_part, &large_fp, box2, Some((small_ct, *max_mm)), &[], STEP_UM, is_legal) {
                let mut trial = out.clone();
                apply_move(&mut trial, large_id, x, y, rot);
                if rules_ok_for(model, &trial, &[small_id, large_id]) {
                    out = trial;
                    resolved = true;
                }
            }
        }

        step(2, &t0);
        // 3. Bounded ripple around the smaller part's best spot.
        if !resolved && !small_frozen {
            if let Some(trial) = try_ripple(model, &out, small_id, large_id, small_part, &small_fp, large_ct, *max_mm, bounds, max_um, STEP_UM, is_legal) {
                out = trial;
                resolved = true;
            }
        }

        step(3, &t0);
        // 4. Last resort: finer grid, wider halo, both directions.
        if !resolved {
            let wide_um = max_um + max_um / 2;
            let box1w = clamp_box(rect_expand(large_ct, wide_um), bounds);
            let mut moved_small = false;
            if small_frozen {
            } else if let Some((x, y, rot)) = find_spot(model, &out, small_id, small_part, &small_fp, box1w, Some((large_ct, *max_mm)), &[], FINE_STEP_UM, is_legal) {
                let mut trial = out.clone();
                apply_move(&mut trial, small_id, x, y, rot);
                if accept_small(&trial) {
                    out = trial;
                    resolved = true;
                    moved_small = true;
                }
            }
            if !moved_small && !large_frozen {
                let box2w = clamp_box(rect_expand(small_ct, wide_um), bounds);
                if let Some((x, y, rot)) = find_spot(model, &out, large_id, large_part, &large_fp, box2w, Some((small_ct, *max_mm)), &[], FINE_STEP_UM, is_legal) {
                    let mut trial = out.clone();
                    apply_move(&mut trial, large_id, x, y, rot);
                    if rules_ok_for(model, &trial, &[small_id, large_id]) {
                        out = trial;
                        resolved = true;
                    }
                }
            }
        }

        if dbg_on() {
            eprintln!("DEBUG_LEGALIZE rule {a}/{b} max_mm={max_mm} small={small_id} large={large_id} resolved={resolved}");
        }
        // If nothing above resolved it, `out` is untouched for this rule
        // beyond whatever an earlier step already committed — the rule
        // stays violated, deliberately.
    }
    out
}

/// Run Cypress on `design` (its placement/outline seeds the problem) and
/// return a design with Cypress's placement. Precondition failures and
/// subprocess errors come back as `CheckResult`s like every generator.
/// Run Cypress once on a Bookshelf problem; `tag` names the work dir.
fn run_cypress(bs: &Bookshelf, seed: u64, o: &CypressOptions, tag: &str) -> Result<(String, PathBuf), Vec<CheckResult>> {
    let name = "board";
    let dir = o.work_dir.join(format!("{}-{seed}-{tag}", std::process::id()));
    let bs_dir = dir.join("bookshelf");
    std::fs::create_dir_all(&bs_dir).map_err(|e| io_fail("work dir", e))?;
    for (fname, content) in bs.files(name) {
        std::fs::write(bs_dir.join(fname), content).map_err(|e| io_fail("bookshelf", e))?;
    }
    let results = dir.join("results");
    let cfg = config_json(&bs_dir.join(format!("{name}.aux")), &results, seed, o);
    let cfg_path = dir.join("config.json");
    std::fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap()).map_err(|e| io_fail("config", e))?;

    // Run inside the per-run work dir: Placer.py writes `DREAMPlace.log` to
    // its cwd, so concurrent runs must not share one.
    let out = Command::new(&o.python)
        .arg(o.install.join("dreamplace").join("Placer.py"))
        .arg(&cfg_path)
        .current_dir(&dir)
        .env("PYTHONPATH", &o.install)
        .output()
        .map_err(|e| io_fail("spawn python", e))?;
    let pl_path = results.join(name).join(format!("{name}.gp.pl"));
    if !out.status.success() || !pl_path.exists() {
        let log = String::from_utf8_lossy(&out.stderr);
        let tail: String = log.chars().rev().take(1500).collect::<Vec<_>>().into_iter().rev().collect();
        return Err(vec![CheckResult::fail("cypress_failed", cfg_path.display().to_string(), format!("exit {:?}; log tail: {tail}", out.status.code()))]);
    }
    // Cypress exits 0 even when the optimiser diverged (it then writes the
    // input placement back). No fallback: read its own log and refuse that.
    let log = std::fs::read_to_string(dir.join("DREAMPlace.log")).unwrap_or_default();
    check_cypress_log(&log).map_err(|hint| vec![CheckResult::fail("cypress_diverged", cfg_path.display().to_string(), hint)])?;
    let pl = std::fs::read_to_string(&pl_path).map_err(|e| io_fail("read .pl", e))?;
    Ok((pl, cfg_path))
}

/// Spacing, µm, kept between two connectors pinned along one edge.
const EDGE_SPACING_UM: i64 = 1000;
/// Extra distance charged to an edge a connector must turn 90° to lie on.
const ROTATE_PENALTY_UM: i64 = 10_000;

/// Pin every edge connector to the board edge nearest to where Cypress's
/// free run left it (long side along the edge, per
/// [`eda_model::footprint::usable_edges`]), sliding connectors that share an
/// edge apart so their keep-outs do not overlap. The label goes on the
/// board side of the part (below at the top edge, above elsewhere).
/// Returns the pinned design and the set of pinned ids; hard-fails when an
/// edge cannot hold its connectors.
pub fn pin_connectors_to_edges(design: &Design, model: &ConstraintModel) -> Result<(Design, BTreeSet<String>), Vec<CheckResult>> {
    let mut out = design.clone();
    let Some(pl) = out.placement.as_mut() else { return Ok((out, BTreeSet::new())) };
    let bb = (
        pl.outline.iter().map(|p| p.x).min().unwrap(),
        pl.outline.iter().map(|p| p.y).min().unwrap(),
        pl.outline.iter().map(|p| p.x).max().unwrap(),
        pl.outline.iter().map(|p| p.y).max().unwrap(),
    );
    // Edge choice: nearest usable edge first, then rebalance — while an
    // edge needs more length than it has, move the connector there whose
    // second-best usable edge has the most room to spare. Edge 0 left,
    // 1 right, 2 top, 3 bottom.
    let edge_len = |e: u8| if e >= 2 { bb.2 - bb.0 } else { bb.3 - bb.1 } - 2 * EDGE_PIN_GAP_UM;
    let mut cands: Vec<(usize, Vec<(u8, i64)>, i64)> = Vec::new(); // (index, usable (edge, gap) sorted, length along an edge)
    for (i, f) in pl.footprints.iter().enumerate() {
        let Some(part) = model.part(&f.id) else { continue };
        if !is_edge_connector(part) {
            continue;
        }
        let Some(r) = placed_courtyard(model, part, f) else { continue };
        let Some(k) = placed_keepout(model, &pl.outline, part, f) else { continue };
        // Every edge is a candidate: an elongated connector turns 90° to
        // lie along an edge its long side does not already face. Gaps are
        // measured to the part's centre so the turned cases compare fairly.
        let (ul, ur, ut, ub) = usable_edges(r);
        let (cx, cy) = ((r.0 + r.2) / 2, (r.1 + r.3) / 2);
        let mut gaps: Vec<(u8, i64)> = [(ul, cx - bb.0), (ur, bb.2 - cx), (ut, cy - bb.1), (ub, bb.3 - cy)]
            .iter()
            .enumerate()
            .map(|(e, (u, g))| (e as u8, if *u { *g } else { *g + ROTATE_PENALTY_UM }))
            .collect();
        gaps.sort_by_key(|(_, g)| *g);
        // Along-edge length: the keep-out's long side.
        cands.push((i, gaps, (k.2 - k.0).max(k.3 - k.1)));
    }
    let mut choice: Vec<u8> = cands.iter().map(|(_, g, _)| g[0].0).collect();
    let need = |choice: &Vec<u8>, e: u8| -> i64 {
        let n: Vec<i64> = cands.iter().zip(choice).filter(|(_, c)| **c == e).map(|((_, _, l), _)| *l).collect();
        if n.is_empty() { 0 } else { n.iter().sum::<i64>() + EDGE_SPACING_UM * (n.len() as i64 - 1) }
    };
    loop {
        let Some(over) = (0..4u8).find(|&e| need(&choice, e) > edge_len(e)) else { break };
        // Best move: a connector on `over` with another usable edge that has room.
        let mut best: Option<(i64, usize, u8)> = None; // (spare after move, cand idx, edge)
        for (ci, (_, gaps, l)) in cands.iter().enumerate() {
            if choice[ci] != over {
                continue;
            }
            for &(e, _) in gaps.iter().skip(1) {
                let spare = edge_len(e) - need(&choice, e) - l - EDGE_SPACING_UM;
                if spare >= 0 && best.map_or(true, |(s, _, _)| spare > s) {
                    best = Some((spare, ci, e));
                }
            }
        }
        match best {
            Some((_, ci, e)) => choice[ci] = e,
            None => break, // left for the row sweep below to report
        }
    }
    let rotations: Vec<Option<u32>> = cands
        .iter()
        .zip(&choice)
        .map(|((i, _, _), edge)| {
            let f = &pl.footprints[*i];
            let r = placed_courtyard(model, model.part(&f.id).unwrap(), f).unwrap();
            let (ul, _, ut, _) = usable_edges(r);
            let usable = if *edge >= 2 { ut } else { ul };
            if usable { None } else { Some((f.rot + 90_000) % 360_000) }
        })
        .collect();
    let mut on_edge: Vec<(u8, usize)> = Vec::new();
    let mut pinned = BTreeSet::new();
    for (ci, (i, _, _)) in cands.iter().enumerate() {
        let edge = choice[ci];
        let i = *i;
        if let Some(rot) = rotations[ci] {
            pl.footprints[i].rot = rot;
        }
        let f = &mut pl.footprints[i];
        let part = model.part(&f.id).unwrap();
        let r = placed_courtyard(model, part, f).unwrap();
        let (hw, hh) = ((r.2 - r.0) / 2, (r.3 - r.1) / 2);
        match edge {
            0 => f.at.x = bb.0 + EDGE_PIN_GAP_UM + hw,
            1 => f.at.x = bb.2 - EDGE_PIN_GAP_UM - hw,
            2 => f.at.y = bb.1 + EDGE_PIN_GAP_UM + hh,
            _ => f.at.y = bb.3 - EDGE_PIN_GAP_UM - hh,
        }
        f.label = if edge == 2 { LabelSide::Below } else { LabelSide::Above };
        on_edge.push((edge, i));
        pinned.insert(f.id.clone());
    }
    // Spread connectors sharing an edge: sweep along the edge, push each
    // one past the previous keep-out, then pull the whole row back if it
    // ran off the far end.
    let mut fails = Vec::new();
    // Vertical edges first; the horizontal rows then start past whatever
    // sits in the corners, so a connector on the right edge and one on the
    // bottom edge cannot meet at the corner.
    for edge in [0u8, 1, 2, 3] {
        let horizontal = edge >= 2; // along x
        let mut ids: Vec<usize> = on_edge.iter().filter(|(e, _)| *e == edge).map(|(_, i)| *i).collect();
        if ids.is_empty() {
            continue;
        }
        let ko = |pl: &eda_model::ir::PlacementSection, i: usize| {
            let f = &pl.footprints[i];
            placed_keepout(model, &pl.outline, model.part(&f.id).unwrap(), f).unwrap()
        };
        let along = |k: (i64, i64, i64, i64)| if horizontal { (k.0, k.2) } else { (k.1, k.3) };
        let (mut lo, mut hi) = if horizontal { (bb.0 + EDGE_PIN_GAP_UM, bb.2 - EDGE_PIN_GAP_UM) } else { (bb.1 + EDGE_PIN_GAP_UM, bb.3 - EDGE_PIN_GAP_UM) };
        if horizontal {
            for &(e, j) in &on_edge {
                let k = ko(pl, j);
                let vertical_overlap = if edge == 2 { k.1 < bb.1 + 2 * EDGE_PIN_GAP_UM + (k.3 - k.1) } else { k.3 > bb.3 - 2 * EDGE_PIN_GAP_UM - (k.3 - k.1) };
                if e == 0 && vertical_overlap { lo = lo.max(k.2 + EDGE_SPACING_UM) }
                if e == 1 && vertical_overlap { hi = hi.min(k.0 - EDGE_SPACING_UM) }
            }
        }
        ids.sort_by_key(|&i| along(ko(pl, i)).0);
        let mut cursor = lo;
        for &i in &ids {
            let k = along(ko(pl, i));
            let shift = (cursor - k.0).max(0);
            if horizontal { pl.footprints[i].at.x += shift } else { pl.footprints[i].at.y += shift }
            cursor = k.1 + shift + EDGE_SPACING_UM;
        }
        let last = along(ko(pl, *ids.last().unwrap())).1;
        let over = last - hi;
        if over > 0 {
            // Pull the row back toward the start, closing gaps from the end.
            let mut cursor = hi;
            for &i in ids.iter().rev() {
                let k = along(ko(pl, i));
                let shift = (k.1 - cursor).max(0);
                if horizontal { pl.footprints[i].at.x -= shift } else { pl.footprints[i].at.y -= shift }
                cursor = k.0 - shift - EDGE_SPACING_UM;
            }
            if along(ko(pl, ids[0])).0 < lo {
                let edge_name = ["left", "right", "top", "bottom"][edge as usize];
                let names: Vec<&str> = ids.iter().map(|&i| pl.footprints[i].id.as_str()).collect();
                fails.push(CheckResult::fail(
                    "edge_connectors_overflow",
                    edge_name,
                    format!("connectors {} do not fit along this edge ({} µm short); widen the board or move one to another edge", names.join(", "), over),
                ).with_detail(serde_json::json!({ "suggest": "board.outline", "edge": edge_name, "short_um": over })));
            }
        }
    }
    if !fails.is_empty() {
        return Err(fails);
    }
    Ok((out, pinned))
}

/// The model plus one `Proximity` rule per decoupling capacitor (at
/// [`eda_model::DECOUPLING_MAX_GAP_UM`]) toward *one* IC it decouples —
/// the gate wants each cap near some IC, and a rail cap shares its nets
/// with every IC on the rail. ICs are dealt caps evenly, nearest first
/// by `placed` (the free run), so each IC gets its own before any gets a
/// second. Caps the intent already constrains keep the intent's rule.
fn with_decoupling_rules(model: &ConstraintModel, placed: &Design) -> ConstraintModel {
    let mut m = model.clone();
    let covered = |c: &str| model.placement_rules.iter().any(|r| matches!(r, PlacementRule::Proximity { a, b, .. } if a == c || b == c));
    let pl = placed.placement.as_ref();
    let ct = |id: &str| pl.and_then(|p| p.footprints.iter().find(|f| f.id == id)).and_then(|f| placed_courtyard(model, model.part(id)?, f));
    let pairs = eda_model::decoupling_pairs(model);
    let mut caps: Vec<String> = pairs.iter().map(|(c, _)| c.clone()).collect();
    caps.dedup();
    let mut dealt: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    // Caps with the fewest candidate ICs choose first.
    caps.sort_by_key(|c| (pairs.iter().filter(|(cc, _)| cc == c).count(), c.clone()));
    for c in caps {
        if covered(&c) {
            continue;
        }
        let Some(rc) = ct(&c) else { continue };
        let pick = pairs
            .iter()
            .filter(|(cc, _)| *cc == c)
            .filter_map(|(_, u)| ct(u).map(|ru| (*dealt.get(u).unwrap_or(&0), rect_gap(rc, ru) as i64, u.clone())))
            .min();
        let Some((_, _, u)) = pick else { continue };
        *dealt.entry(u.clone()).or_default() += 1;
        m.placement_rules.push(PlacementRule::Proximity { a: c, b: u, max_mm: eda_model::DECOUPLING_MAX_GAP_UM as f64 / 1000.0, reason: Some("decoupling cap and the IC it decouples".into()) });
    }
    m
}

fn orient(a: Point, b: Point, c: Point) -> i64 {
    ((b.x - a.x) as i128 * (c.y - a.y) as i128 - (b.y - a.y) as i128 * (c.x - a.x) as i128).signum() as i64
}

/// Crossing pairs of 2-pin net stubs where both stubs end on a free 2-pin
/// part — the same count as the `placement_stub_crossings` gate. Each
/// entry names the free part on each stub to swap.
fn stub_crossings(design: &Design, model: &ConstraintModel) -> Vec<(String, String)> {
    let Some(pl) = design.placement.as_ref() else { return vec![] };
    let mut centers: std::collections::HashMap<String, Point> = std::collections::HashMap::new();
    for fp in &pl.footprints {
        let Some(part) = model.part(&fp.id) else { continue };
        if let Some(pads) = placed_pads(model, part, fp) {
            for pad in pads {
                centers.insert(format!("{}.{}", fp.id, pad.number), pad.center);
            }
        }
    }
    // (a, b, free part on this stub)
    let mut stubs: Vec<(Point, Point, Option<String>)> = Vec::new();
    for net in &model.nets {
        if net.pins.len() != 2 {
            continue;
        }
        let (Some(ra), Some(rb)) = (net.pins[0].split_once('.'), net.pins[1].split_once('.')) else { continue };
        if ra.0 == rb.0 {
            continue;
        }
        let (Some(&a), Some(&b)) = (centers.get(&net.pins[0]), centers.get(&net.pins[1])) else { continue };
        let free = [ra.0, rb.0].iter().find(|r| model.part(r).map_or(false, eda_model::is_free_two_pin)).map(|r| r.to_string());
        stubs.push((a, b, free));
    }
    let mut out = Vec::new();
    for i in 0..stubs.len() {
        for j in i + 1..stubs.len() {
            let (s, t) = (&stubs[i], &stubs[j]);
            let (Some(fs), Some(ft)) = (&s.2, &t.2) else { continue };
            if fs == ft {
                continue;
            }
            let o = [orient(s.0, s.1, t.0), orient(s.0, s.1, t.1), orient(t.0, t.1, s.0), orient(t.0, t.1, s.1)];
            if o.iter().all(|v| *v != 0) && o[0] != o[1] && o[2] != o[3] {
                out.push((fs.clone(), ft.clone()));
            }
        }
    }
    out
}

/// Number of `Proximity` rules `design` violates.
fn total_violations(model: &ConstraintModel, design: &Design) -> usize {
    let pl = design.placement.as_ref().unwrap();
    let ct = |r: &str| pl.footprints.iter().find(|f| f.id == r).and_then(|f| placed_courtyard(model, model.part(r)?, f));
    model
        .placement_rules
        .iter()
        .filter(|r| match r {
            PlacementRule::Proximity { a, b, max_mm, .. } => match (ct(a), ct(b)) {
                (Some(ca), Some(cb)) => rect_gap(ca, cb) / 1000.0 > *max_mm,
                _ => false,
            },
            _ => false,
        })
        .count()
}

/// Put `pinned` parts back exactly where `source` has them: the Bookshelf
/// round trip rounds to its 100 µm unit, and a terminal must not drift.
fn restore_pinned(design: &mut Design, source: &Design, pinned: &BTreeSet<String>) {
    let Some(src) = source.placement.as_ref() else { return };
    let Some(pl) = design.placement.as_mut() else { return };
    for f in pl.footprints.iter_mut().filter(|f| pinned.contains(&f.id)) {
        if let Some(s) = src.footprints.iter().find(|s| s.id == f.id) {
            f.at = s.at;
            f.rot = s.rot;
            f.label = s.label;
        }
    }
}

/// Swap pairs of free 2-pin parts whose net stubs cross (what a reviewer
/// sees as "swap those two"), accepting a swap when it is legal, removes
/// crossings and breaks no extra proximity rule. Deterministic, bounded.
pub fn uncross_stubs(design: &Design, model: &ConstraintModel, is_legal: &dyn Fn(&Design) -> bool) -> Design {
    let mut out = design.clone();
    for _round in 0..8 {
        let crossings = stub_crossings(&out, model);
        if crossings.is_empty() {
            break;
        }
        let before = (crossings.len(), total_violations(model, &out));
        let mut improved = false;
        'pairs: for (a, b) in crossings {
            // Moves, cheapest first: turn one part 180° (its two pads trade
            // places), turn the other, swap the two, swap and turn.
            let moves: [(bool, bool, bool); 6] = [(false, true, false), (false, false, true), (true, false, false), (true, true, false), (true, false, true), (true, true, true)];
            for (swap, turn_a, turn_b) in moves {
                let mut trial = out.clone();
                {
                    let pl = trial.placement.as_mut().unwrap();
                    let ia = pl.footprints.iter().position(|f| f.id == a);
                    let ib = pl.footprints.iter().position(|f| f.id == b);
                    let (Some(ia), Some(ib)) = (ia, ib) else { continue 'pairs };
                    if swap {
                        let (fa, fb) = (pl.footprints[ia].clone(), pl.footprints[ib].clone());
                        pl.footprints[ia].at = fb.at;
                        pl.footprints[ia].rot = fb.rot;
                        pl.footprints[ia].label = fb.label;
                        pl.footprints[ib].at = fa.at;
                        pl.footprints[ib].rot = fa.rot;
                        pl.footprints[ib].label = fa.label;
                    }
                    if turn_a {
                        pl.footprints[ia].rot = (pl.footprints[ia].rot + 180_000) % 360_000;
                    }
                    if turn_b {
                        pl.footprints[ib].rot = (pl.footprints[ib].rot + 180_000) % 360_000;
                    }
                }
                let after = (stub_crossings(&trial, model).len(), total_violations(model, &trial));
                if after.0 < before.0 && after.1 <= before.1 && is_legal(&trial) {
                    if dbg_on() {
                        eprintln!("DEBUG_UNCROSS {a}/{b} swap={swap} turn=({turn_a},{turn_b}): crossings {} -> {}", before.0, after.0);
                    }
                    out = trial;
                    improved = true;
                    break 'pairs;
                }
            }
        }
        if !improved {
            break;
        }
    }
    out
}





/// Run Cypress on `design` (its placement/outline seeds the problem) and
/// return a design with Cypress's placement: a free run to learn which edge
/// each connector wants, then a second run with the connectors pinned on
/// their edges as fixed terminals. Precondition failures and subprocess
/// errors come back as `CheckResult`s like every generator.
pub fn place_with_cypress(design: &Design, model: &ConstraintModel, seed: u64, o: &CypressOptions) -> Result<Design, Vec<CheckResult>> {
    if !o.available() {
        return Err(vec![CheckResult::fail("cypress_unavailable", o.install.display().to_string(), "no dreamplace/Placer.py there; build Cypress or set CYPRESS_INSTALL")]);
    }
    let name = "board";
    let none = BTreeSet::new();
    let base_model = model;
    // Seed design: the outline from the intent (or the existing placement)
    // with every part at the centre, the shape `to_bookshelf_fixed` reads.
    let mut seed_design = design.clone();
    if seed_design.placement.is_none() {
        // Same reasoning: a seed design centred on the origin of a board
        // that has no shape is not a starting point, it is a pile.
        let outline = base_model.board.outline.clone().unwrap_or_default();
        assert!(outline.len() >= 3, "cypress needs a board outline, got {} point(s)", outline.len());
        let c = Point {
            x: (outline.iter().map(|p| p.x).min().expect("outline non-empty") + outline.iter().map(|p| p.x).max().expect("outline non-empty")) / 2,
            y: (outline.iter().map(|p| p.y).min().expect("outline non-empty") + outline.iter().map(|p| p.y).max().expect("outline non-empty")) / 2,
        };
        seed_design.placement = Some(eda_model::ir::PlacementSection {
            outline,
            footprints: base_model.parts.iter().map(|p| eda_model::ir::FootprintInstance { id: p.reference.clone(), at: c, rot: 0, side: eda_model::ir::Side::Top, label: LabelSide::Above }).collect(),
            modules: Vec::new(),
        });
    }
    // Board fit is a search, not a fallback: start at the target
    // utilisation and step the board back toward the intent's outline only
    // when the edge connectors do not fit along its edges.
    let mut scale_floor = 0.0;
    let mut placed = loop {
        let fitted = fit_outline(&seed_design, base_model, o.fit_board_utilization, scale_floor);
        let bs = to_bookshelf_fixed(&fitted, base_model, name, UNIT_UM, o.proximity_weight, &none)?;
        let (pl, _) = run_cypress(&bs, seed, o, "free")?;
        let free = from_bookshelf_pl_fixed(&pl, &fitted, base_model, UNIT_UM, &none)?;
        let model = &with_decoupling_rules(base_model, &free);
        match pin_connectors_to_edges(&free, model) {
            Ok((pinned_design, pinned)) => {
                if pinned.is_empty() {
                    let legal = |d: &Design| default_is_legal(d, model);
                    let placed = legalize_proximity_pass(&free, model, &legal);
                    break uncross_stubs(&placed, model, &legal);
                }
                let bs = to_bookshelf_fixed(&pinned_design, model, name, UNIT_UM, o.proximity_weight, &pinned)?;
                let (pl, _) = run_cypress(&bs, seed, o, "pinned")?;
                let mut placed = from_bookshelf_pl_fixed(&pl, &pinned_design, model, UNIT_UM, &pinned)?;
                restore_pinned(&mut placed, &pinned_design, &pinned);
                // Trim empty bands off edges that hold no connector (the
                // free run's connectors pull everything to their edges and
                // leave the far side blank), then place once more.
                for round in 0..2 {
                    let Some(trimmed) = trim_empty_edges(&placed, model, &pinned) else { break };
                    let bs = to_bookshelf_fixed(&trimmed, model, name, UNIT_UM, o.proximity_weight, &pinned)?;
                    let (pl, _) = run_cypress(&bs, seed, o, &format!("trimmed{round}"))?;
                    placed = from_bookshelf_pl_fixed(&pl, &trimmed, model, UNIT_UM, &pinned)?;
                    restore_pinned(&mut placed, &trimmed, &pinned);
                }
                // Pinned connectors stay where the edge pass put them.
                let frozen: Vec<(String, Point, u32)> = placed.placement.as_ref().unwrap().footprints.iter().filter(|f| pinned.contains(&f.id)).map(|f| (f.id.clone(), f.at, f.rot)).collect();
                let legal = |d: &Design| {
                    default_is_legal(d, model)
                        && frozen.iter().all(|(id, at, rot)| d.placement.as_ref().unwrap().footprints.iter().any(|f| &f.id == id && f.at == *at && f.rot == *rot))
                };
                // The .wts pull rarely satisfies every rule exactly — pin it
                // down deterministically. No fallback: an unsatisfiable pair
                // is left violated, so `placement_proximity` still fails.
                placed = legalize_proximity_pass_frozen(&placed, model, &legal, &pinned);
                placed = uncross_stubs(&placed, model, &legal);
                break placed;
            }
            Err(fails) => {
                let fitted_scale = fitted.placement.as_ref().map(|p| (p.outline.iter().map(|q| q.x).max().unwrap() - p.outline.iter().map(|q| q.x).min().unwrap()) as f64).unwrap_or(1.0)
                    / seed_design.placement.as_ref().map(|p| (p.outline.iter().map(|q| q.x).max().unwrap() - p.outline.iter().map(|q| q.x).min().unwrap()) as f64).unwrap_or(1.0);
                if fitted_scale >= 0.999 {
                    return Err(fails);
                }
                eprintln!("cypress: board fit at {:.2} of the intent outline leaves no room for the edge connectors; trying larger", fitted_scale);
                scale_floor = (fitted_scale * 1.25).min(1.0);
            }
        }
    };
    placed.provenance.seed = seed;
    placed.provenance.engine_version = format!("cypress@{}", o.install.display());
    Ok(placed)
}

/// `Placer` adapter so the loop can pick Cypress like any other generator.
pub struct Cypress(pub CypressOptions);

impl Placer for Cypress {
    fn name(&self) -> &str {
        "cypress"
    }
    fn place(&self, design: &Design, model: &ConstraintModel, seed: u64) -> Result<Design, Vec<CheckResult>> {
        place_with_cypress(design, model, seed, &self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_has_the_pcb_keys() {
        let o = CypressOptions::default();
        let c = config_json(Path::new("/x/b.aux"), Path::new("/r"), 7, &o);
        assert_eq!(c["gpu"], 0);
        assert_eq!(c["random_seed"], 7);
        assert_eq!(c["global_place_stages"][0]["wirelength"], "weighted_average");
    }

    #[test]
    fn unavailable_install_is_a_clean_failure() {
        let o = CypressOptions { install: PathBuf::from("/definitely/not/here"), ..Default::default() };
        let d = Design { schema: 1, provenance: eda_model::ir::Provenance { engine_version: "t".into(), intent_hash: "h".into(), seed: 0, stage_hashes: vec![] }, schematic: None, nets: None, placement: None, routing: None, drawings: None, footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None };
        let err = place_with_cypress(&d, &ConstraintModel::default(), 0, &o).unwrap_err();
        assert_eq!(err[0].check, "cypress_unavailable");
    }

    #[test]
    fn log_check_rejects_divergence() {
        assert!(check_cypress_log("x\n[INFO] Final PPA: {'rsmt': inf, 'hpwl': inf, 'iteration': 246}\n").is_err());
        assert!(check_cypress_log("[WARNING] skip legalization and detail placement steps\n[INFO] Final PPA: {'hpwl': 357.0}\n").is_err());
        assert!(check_cypress_log("[INFO] Final PPA: {'rsmt': 395.0, 'hpwl': 357.0, 'iteration': 281}\n").is_ok());
        assert!(check_cypress_log("nothing").is_err());
    }
}
