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

use eda_interchange::bookshelf::to_bookshelf_weighted;
use eda_interchange::from_bookshelf_pl;
use eda_model::footprint::{placed_courtyard, placed_keepout};
use eda_model::ir::{Design, Point};
use eda_model::{CheckResult, ConstraintModel, PlacementRule};
use eda_place::Placer;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Bookshelf unit used for the bridge, µm.
pub const UNIT_UM: i64 = 100;

#[derive(Debug, Clone)]
pub struct CypressOptions {
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

/// True when every `Proximity` rule touching any of `ids` is satisfied in
/// `design` — used after a ripple move to confirm a displaced neighbour
/// (or the mover/anchor) didn't break a rule of its own.
fn rules_ok_for(model: &ConstraintModel, design: &Design, ids: &[&str]) -> bool {
    let Some(placement) = design.placement.as_ref() else { return false };
    let fp_of = |id: &str| placement.footprints.iter().find(|f| f.id == id);
    for rule in &model.placement_rules {
        let PlacementRule::Proximity { a, b, max_mm } = rule else { continue };
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
        let PlacementRule::Proximity { a, b, max_mm } = rule else { continue };
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

        let mut resolved = false;

        // 1. Move the smaller part toward the larger (fixed) one.
        let box1 = clamp_box(rect_expand(large_ct, max_um), bounds);
        if let Some((x, y, rot)) = find_spot(model, &out, small_id, small_part, &small_fp, box1, Some((large_ct, *max_mm)), &[], STEP_UM, is_legal) {
            apply_move(&mut out, small_id, x, y, rot);
            resolved = true;
        }

        // 2. Move the larger part toward the smaller (fixed) one instead —
        // only accepted if it doesn't break one of the larger part's own
        // other rules (e.g. dragging it out of range of a different anchor).
        if !resolved {
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

        // 3. Bounded ripple around the smaller part's best spot.
        if !resolved {
            if let Some(trial) = try_ripple(model, &out, small_id, large_id, small_part, &small_fp, large_ct, *max_mm, bounds, max_um, STEP_UM, is_legal) {
                out = trial;
                resolved = true;
            }
        }

        // 4. Last resort: finer grid, wider halo, both directions.
        if !resolved {
            let wide_um = max_um + max_um / 2;
            let box1w = clamp_box(rect_expand(large_ct, wide_um), bounds);
            if let Some((x, y, rot)) = find_spot(model, &out, small_id, small_part, &small_fp, box1w, Some((large_ct, *max_mm)), &[], FINE_STEP_UM, is_legal) {
                apply_move(&mut out, small_id, x, y, rot);
                resolved = true;
            } else {
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
pub fn place_with_cypress(design: &Design, model: &ConstraintModel, seed: u64, o: &CypressOptions) -> Result<Design, Vec<CheckResult>> {
    if !o.available() {
        return Err(vec![CheckResult::fail("cypress_unavailable", o.install.display().to_string(), "no dreamplace/Placer.py there; build Cypress or set CYPRESS_INSTALL")]);
    }
    let name = "board";
    let bs = to_bookshelf_weighted(design, model, name, UNIT_UM, o.proximity_weight)?;
    let dir = o.work_dir.join(format!("{}-{seed}", std::process::id()));
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
    let mut placed = from_bookshelf_pl(&pl, design, model, UNIT_UM)?;
    placed.provenance.seed = seed;
    placed.provenance.engine_version = format!("cypress@{}", o.install.display());
    // The .wts pull (above) rarely satisfies every rule exactly — pin it
    // down deterministically. No fallback: an unsatisfiable pair is left
    // violated, so `placement_proximity` still fails on it.
    placed = legalize_proximity_pass(&placed, model, &|d| default_is_legal(d, model));
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
        let d = Design { schema: 1, provenance: eda_model::ir::Provenance { engine_version: "t".into(), intent_hash: "h".into(), seed: 0, stage_hashes: vec![] }, schematic: None, placement: None, routing: None };
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
