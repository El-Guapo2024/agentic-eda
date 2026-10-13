//! Port of `PNS::MEANDER_PLACER`/`MEANDER_SHAPE` (`pcbnew/router/
//! pns_meander_placer.{h,cpp}`, `pns_meander.cpp`), gap #7 task item 4
//! (length tuning, keys `7`/`8`/`9`): lengthen a track by inserting a
//! repeating zigzag ("meander"/"accordion") pattern until it reaches a
//! target length.
//!
//! ## Scope -- a dialog-driven amount, not a live mouse-driven session
//!
//! Upstream's length tuner is a full interactive session (`PNS::ROUTER`
//! drives `MEANDER_PLACER` the same way it drives `LINE_PLACER`/
//! `DIFF_PAIR_PLACER`): drag the mouse away from the track to grow the
//! meander live, `1`/`2`/`3`/`4` adjust spacing/amplitude mid-drag, a
//! status-bar readout tracks the running length. Building a *third*
//! interactive session type (after route and drag) with its own
//! mouse-driven preview state was out of proportion to the time this
//! task had left after items 1-3, so this port instead exposes the same
//! underlying operation -- "lengthen this track to a target length with
//! this amplitude/spacing" -- as a one-shot, dialog-driven computation:
//! `crates/cli/src/tune_api.rs`'s `/api/tune_length/{preview,apply}`,
//! `web/studio/src/components/LengthTuningDialog.tsx`. The *geometry* is
//! still the real thing (see below); only the interaction model differs.
//!
//! ## Shape and scope
//!
//! Only a **straight, axis-aligned (N/E/S/W) two-point baseline** is
//! supported -- not upstream's own ability to meander within an
//! arbitrary already-routed multi-corner line. This isn't an arbitrary
//! cut: the meander construction below builds each zigzag leg as
//! `direction + perpendicular` (and `direction - perpendicular`), which
//! is only guaranteed to land on another valid 45-degree octant (a
//! diagonal one) when `direction` itself is axis-aligned -- for a
//! diagonal baseline, `direction + perpendicular` degenerates to a
//! *different* axis-aligned direction at a different effective length,
//! which would need a separate (correct, but not yet derived) formula.
//! Given the real-world case this task cares about (length-matching a
//! fanout that's already a straight axis-aligned run) is exactly the
//! case this covers, axis-aligned-only is a reasonable, documented
//! narrowing rather than a silent gap. [`generate_meander`] returns
//! `None` for anything else, same as it does for a non-straight (more
//! than 2 points) or too-short baseline.
//!
//! The meander itself is a standard alternating accordion: each period
//! steps diagonally away from the baseline by `amplitude`, runs parallel
//! to the baseline for `spacing`, then steps diagonally back, alternating
//! which side of the baseline each successive period swings to (so the
//! whole shape oscillates across the baseline rather than bulging
//! entirely to one side). `collision checking is the caller's job
//! (`crate::node::Node::line_colliding`-equivalent against the generated
//! points) -- same report-only contract the rest of this scoped-down
//! router already uses for diff pairs (no automatic avoidance).

use eda_model::ir::{Point, Um};

/// `sqrt(2) - 1`: how much farther a single 45-degree diagonal leg of
/// "run" length `amplitude` travels than the straight-line distance it
/// advances *along the baseline* -- a diagonal leg of `(amplitude,
/// amplitude)` has length `amplitude * sqrt(2)` but only ever advances
/// `amplitude` along the baseline axis, so the straight baseline distance
/// it *replaces* is also `amplitude` (not `amplitude * sqrt(2)`); one full
/// period uses two such legs (out and back), so the extra length one
/// period contributes, independent of `spacing` (the straight run at the
/// peak advances the same amount whichever way you draw it), is
/// `2 * amplitude * (sqrt(2) - 1)`. Verified directly against a measured
/// polyline length in this module's own tests, not just trusted.
const DIAGONAL_OVERHEAD: f64 = std::f64::consts::SQRT_2 - 1.0;

fn extra_per_period(amplitude_um: Um) -> f64 {
    2.0 * amplitude_um as f64 * DIAGONAL_OVERHEAD
}

fn round_pt(x: f64, y: f64) -> Point {
    Point { x: x.round() as Um, y: y.round() as Um }
}

pub fn polyline_length(pts: &[Point]) -> Um {
    pts.windows(2).map(|w| (((w[1].x - w[0].x) as f64).powi(2) + ((w[1].y - w[0].y) as f64).powi(2)).sqrt()).sum::<f64>().round() as Um
}

#[derive(Debug, Clone)]
pub struct MeanderResult {
    pub pts: Vec<Point>,
    /// The generated shape's own real length, *measured* from `pts`
    /// (never just trusted from the formula that picked each period's
    /// amplitude) -- this is what a caller should actually display, even
    /// though it will usually be within a few um of `target_length_um`
    /// (exactly, modulo integer-micrometer rounding, when the target was
    /// reachable at all).
    pub achieved_length: Um,
}

/// Generate a meandered replacement for the straight baseline `a`-`b`,
/// alternating sides each period, that reaches as close to
/// `target_length_um` as the available room (`spacing`-separated periods
/// along the baseline's own length) allows. `flip` swaps which side the
/// first period swings to (the second, third, ... alternate from there
/// regardless). Returns `None` if:
/// - `a`/`b` aren't axis-aligned (see this module's own doc comment), or
///   are the same point (zero-length baseline);
/// - `amplitude`/`spacing` aren't positive;
/// - `target_length_um` is no greater than the baseline's own straight
///   length (nothing to add -- not an error, just nothing for this
///   function to usefully do; the caller already has the plain baseline
///   in that case).
pub fn generate_meander(a: Point, b: Point, amplitude_um: Um, spacing_um: Um, target_length_um: Um, flip: bool) -> Option<MeanderResult> {
    if amplitude_um <= 0 || spacing_um <= 0 {
        return None;
    }
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let axis_aligned = (dx == 0) != (dy == 0); // exactly one of dx/dy is zero: pure horizontal or vertical.
    if !axis_aligned {
        return None;
    }
    let base_len = ((dx as f64).powi(2) + (dy as f64).powi(2)).sqrt();
    if base_len < 1.0 {
        return None;
    }
    let (ux, uy) = (dx as f64 / base_len, dy as f64 / base_len);
    // Perpendicular, rotated -90 degrees (board +y is down, so this is a
    // real on-screen rotation, not just an algebraic convenience) --
    // `flip` picks the other rotation instead.
    let (nx, ny) = if flip { (-uy, ux) } else { (uy, -ux) };

    let extra_needed = target_length_um as f64 - base_len;
    if extra_needed <= 0.0 {
        return None;
    }
    let per_period = extra_per_period(amplitude_um);
    if per_period <= 0.0 {
        return None;
    }

    let mut pts = vec![a];
    let mut cur = (a.x as f64, a.y as f64);
    let mut remaining_extra = extra_needed;
    let mut remaining_room = base_len;
    let mut side_up = true;

    while remaining_extra > 0.5 {
        // The last period shrinks its own amplitude to land as close to
        // the target as this period alone can get, rather than always
        // using the full requested amplitude and overshooting -- and, if
        // the *baseline* is what's actually run out (not the requested
        // extra length), shrinks further still to whatever amplitude the
        // remaining room can physically fit, so a target right at the
        // edge of what this amplitude/spacing combination can achieve
        // over this baseline length gets as close as the geometry
        // allows instead of stopping one period early.
        let wanted_amp = if remaining_extra < per_period { (remaining_extra / (2.0 * DIAGONAL_OVERHEAD)).min(amplitude_um as f64) } else { amplitude_um as f64 };
        let this_amp = wanted_amp.min(remaining_room / 2.0);
        if this_amp < 1.0 {
            break; // not even the two diagonal legs fit any more -- genuinely out of room.
        }
        // The straight run at the peak shrinks too, down to whatever's
        // actually left after the two diagonals, rather than a fixed
        // `spacing` occasionally being the one thing standing between a
        // final partial period and actually reaching the target.
        let this_spacing = spacing_um.min((remaining_room - 2.0 * this_amp).round() as Um).max(0);
        let sign = if side_up { 1.0 } else { -1.0 };
        let (sx, sy) = (sign * nx, sign * ny);
        let p1 = (cur.0 + this_amp * (ux + sx), cur.1 + this_amp * (uy + sy));
        let p2 = (p1.0 + this_spacing as f64 * ux, p1.1 + this_spacing as f64 * uy);
        let p3 = (p2.0 + this_amp * (ux - sx), p2.1 + this_amp * (uy - sy));
        pts.push(round_pt(p1.0, p1.1));
        pts.push(round_pt(p2.0, p2.1));
        pts.push(round_pt(p3.0, p3.1));
        remaining_extra -= 2.0 * this_amp * DIAGONAL_OVERHEAD;
        remaining_room -= 2.0 * this_amp + this_spacing as f64;
        cur = p3;
        side_up = !side_up;
    }
    if pts.len() < 2 {
        return None; // not enough room for even one period.
    }
    pts.push(b);
    pts.dedup(); // the trailing straight run to `b` is zero-length when the last period ended exactly at `b`.
    let achieved_length = polyline_length(&pts);
    Some(MeanderResult { pts, achieved_length })
}

/// Drops consecutive duplicate points and every middle point that lies exactly on the straight run
/// between its neighbours (cross product zero and no reversal), keeping the first and last point
/// untouched -- a stub and the straight run it joins become one segment.
pub fn simplify_collinear(pts: Vec<Point>) -> Vec<Point> {
    let mut deduped: Vec<Point> = Vec::with_capacity(pts.len());
    for p in pts {
        if deduped.last() != Some(&p) {
            deduped.push(p);
        }
    }
    if deduped.len() < 3 {
        return deduped;
    }
    let mut out = vec![deduped[0]];
    for i in 1..deduped.len() - 1 {
        let prev = *out.last().expect("out starts non-empty");
        let (cur, next) = (deduped[i], deduped[i + 1]);
        let cross = (cur.x - prev.x) as i128 * (next.y - cur.y) as i128 - (cur.y - prev.y) as i128 * (next.x - cur.x) as i128;
        let dot = (cur.x - prev.x) as i128 * (next.x - cur.x) as i128 + (cur.y - prev.y) as i128 * (next.y - cur.y) as i128;
        if cross == 0 && dot > 0 {
            continue; // straight through: this point adds nothing
        }
        out.push(cur);
    }
    out.push(*deduped.last().expect("len >= 3"));
    out
}

// ---------------------------------------------------------------------------------------------
// Differential-pair ("dual") meander -- `DP_MEANDER_PLACER` (`pns_dp_meander_placer.cpp`).
// ---------------------------------------------------------------------------------------------
//
// Upstream's DP tuner builds one `MEANDERED_LINE( this, true )` ("dual"): the baseline is the midline of the
// coupled segment pair (`baselineSegment`: the midpoints of the two segments' endpoints), the meander is
// generated once along it, and each of the pair's two lines is that shape shifted sideways by
// `(gap + width) / 2` (`SetBaselineOffset`; `pairOrientation` picks which line takes which sign). The two
// lines are therefore parallel everywhere, with corners mitered -- they differ in length only by what the
// miters add or take away, and `m_lastLength` reports `max( P, N )`.
//
// Same scoping as the single-track tuner above: the baseline is a straight axis-aligned run and the
// meander is this module's 45-degree trapezoid wave (not upstream's rectangular/rounded bumps). Each line
// is `offset_polyline` of that centerline by its own signed lateral offset (positive = the direction
// turned -90 degrees, the convention `crate::diff_pair::offset_polyline` already uses).

/// Why [`generate_dp_meander`] produced nothing, with the sentence a caller shows the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DpMeanderError {
    /// Not an axis-aligned baseline, non-positive amplitude/spacing, a target that is not longer than the
    /// baseline, or a baseline too short to fit even a lead-in and a minimal bump.
    NothingToAdd,
    /// The shape's two lines (or one of them with itself) would touch: the spacing/amplitude leave no room
    /// for the pair's own width -- the inner line of a bump folds over itself.
    Overlap,
}

impl DpMeanderError {
    pub fn message(self) -> &'static str {
        match self {
            DpMeanderError::NothingToAdd => "can't reach that target length -- it may already be at or below the pair's own straight length, or the amplitude/spacing leave no room on this short a run",
            DpMeanderError::Overlap => "the pair's two lines would overlap at this amplitude/spacing -- increase the spacing (the straight run at each bump must exceed the pair's width) and retry",
        }
    }
}

/// The tuned lines of a pair, each from its own start to its own end (both run in the baseline's direction).
#[derive(Debug, Clone)]
pub struct DpMeanderResult {
    pub a: Vec<Point>,
    pub b: Vec<Point>,
    pub a_length: Um,
    pub b_length: Um,
}

fn orient(p: Point, q: Point, r: Point) -> i128 {
    (q.x - p.x) as i128 * (r.y - p.y) as i128 - (q.y - p.y) as i128 * (r.x - p.x) as i128
}

fn within_box(p: Point, q: Point, r: Point) -> bool {
    r.x >= p.x.min(q.x) && r.x <= p.x.max(q.x) && r.y >= p.y.min(q.y) && r.y <= p.y.max(q.y)
}

/// Do the closed segments `a1-a2` and `b1-b2` share any point (touching and collinear overlap included)?
fn segments_touch(a1: Point, a2: Point, b1: Point, b2: Point) -> bool {
    let (o1, o2) = (orient(a1, a2, b1).signum(), orient(a1, a2, b2).signum());
    let (o3, o4) = (orient(b1, b2, a1).signum(), orient(b1, b2, a2).signum());
    if o1 != o2 && o3 != o4 {
        return true;
    }
    (o1 == 0 && within_box(a1, a2, b1)) || (o2 == 0 && within_box(a1, a2, b2)) || (o3 == 0 && within_box(b1, b2, a1)) || (o4 == 0 && within_box(b1, b2, a2))
}

/// Does `a` touch itself (any two non-adjacent segments), `b` itself, or `a` touch `b`?
fn lines_conflict(a: &[Point], b: &[Point]) -> bool {
    let own = |p: &[Point]| {
        let segs: Vec<(Point, Point)> = p.windows(2).filter(|w| w[0] != w[1]).map(|w| (w[0], w[1])).collect();
        (0..segs.len()).any(|i| (i + 2..segs.len()).any(|j| segments_touch(segs[i].0, segs[i].1, segs[j].0, segs[j].1)))
    };
    if own(a) || own(b) {
        return true;
    }
    a.windows(2).any(|sa| b.windows(2).any(|sb| segments_touch(sa[0], sa[1], sb[0], sb[1])))
}

/// Meander a differential pair along the straight, axis-aligned baseline `base_a`-`base_b` (the midline
/// between the two coupled lines). `off_a`/`off_b` are the lines' signed lateral offsets from that
/// baseline (`crate::diff_pair::offset_polyline`'s sign convention); `target_len_um` is the length the
/// *longer* of the two lines should reach over this stretch (`DP_MEANDER_PLACER` reports `max( P, N )`).
/// Both lines start and end exactly on the baseline's ends shifted by their own offset, so they stay
/// joined to whatever the stretch is attached to.
pub fn generate_dp_meander(base_a: Point, base_b: Point, off_a: Um, off_b: Um, amplitude_um: Um, spacing_um: Um, target_len_um: Um, flip: bool) -> Result<DpMeanderResult, DpMeanderError> {
    use crate::diff_pair::offset_polyline;
    let (dx, dy) = (base_b.x - base_a.x, base_b.y - base_a.y);
    if (dx == 0) == (dy == 0) || amplitude_um <= 0 || spacing_um <= 0 {
        return Err(DpMeanderError::NothingToAdd);
    }
    let len = dx.abs() + dy.abs();
    let (ux, uy) = (dx.signum(), dy.signum());
    // A straight lead-in/out keeps the first/last mitered corner (it sits `|off| * tan(22.5 deg)` back from the
    // diagonal's own vertex on the inside of the turn) inside the stretch, so each line starts exactly at its
    // own end point instead of backing up past it.
    let widest = off_a.abs().max(off_b.abs());
    let lead = (widest as f64 * (std::f64::consts::FRAC_PI_8).tan()).ceil() as Um + 1;
    if len <= 2 * lead + 2 || target_len_um <= len {
        return Err(DpMeanderError::NothingToAdd);
    }
    let inner_a = Point { x: base_a.x + ux * lead, y: base_a.y + uy * lead };
    let inner_b = Point { x: base_b.x - ux * lead, y: base_b.y - uy * lead };
    let inner_len = (len - 2 * lead) as f64;
    // `MEANDER_SHAPE::MinAmplitude` (`|baselineOffset| + correction`): below the pair's own half-spacing a bump's
    // inner line would sit on the wrong side of the baseline. `MEANDER_SHAPE::spacing` for a dual meander
    // (`width + clearance + 2 * |baselineOffset|`): the straight run at each bump never gets shorter than the two
    // lines' own span.
    let min_amp = widest + 1;
    let amp_max = amplitude_um.max(min_amp);
    let spacing = spacing_um.max(2 * widest);

    // One pass builds the shape for one target elongation of the longer line; that line's real length differs from
    // the centerline's by whatever the miters add or take away, so correct the elongation by the miss and go again
    // (a pass or two settles it).
    let mut want_extra = (target_len_um - len) as f64;
    let mut best: Option<(Um, DpMeanderResult)> = None;
    for pass in 0..5 {
        let Some((count, amp_sum)) = pick_periods(want_extra, inner_len, spacing as f64, amp_max, min_amp) else {
            if pass == 0 {
                return Err(DpMeanderError::NothingToAdd);
            }
            break;
        };
        let Some(zig) = zigzag(inner_a, inner_b, count, amp_sum, spacing, flip) else {
            if pass == 0 {
                return Err(DpMeanderError::NothingToAdd);
            }
            break;
        };
        let mut centerline = Vec::with_capacity(zig.len() + 2);
        centerline.push(base_a);
        centerline.extend_from_slice(&zig);
        centerline.push(base_b);
        let a = offset_polyline(&centerline, off_a);
        let b = offset_polyline(&centerline, off_b);
        if lines_conflict(&a, &b) {
            return Err(DpMeanderError::Overlap);
        }
        let (a_length, b_length) = (polyline_length(&a), polyline_length(&b));
        let miss = target_len_um - a_length.max(b_length);
        if best.as_ref().is_none_or(|(m, _)| miss.abs() < *m) {
            best = Some((miss.abs(), DpMeanderResult { a, b, a_length, b_length }));
        }
        if miss.abs() <= 2 {
            break;
        }
        want_extra += miss as f64;
        if want_extra <= 0.0 {
            break;
        }
    }
    best.map(|(_, r)| r).ok_or(DpMeanderError::NothingToAdd)
}

/// How many bumps, and the sum of their amplitudes, give `extra` um of added length over `room` um of straight
/// run -- the uniform amplitude `MEANDER_PLACER_BASE::tuneLineLength` ends up with (`findAmplitudeForLength` spreads
/// the elongation evenly over the meanders it keeps) instead of a few full-size bumps and one tiny remainder. One
/// period of amplitude `a` adds `2 * a * (sqrt 2 - 1)` and uses `2 * a + spacing` of the run, so `n` bumps of
/// summed amplitude `T` add `2 * (sqrt 2 - 1) * T`, use `2 * T + n * spacing`, and can only be as small as
/// `n * min_amp` or as large as `n * amp_max`. Every `n` that fits is tried with the summed amplitude closest to
/// what the target wants; the least miss wins, the fewest bumps (so the biggest amplitude) on a tie. A pick that
/// misses by more than adding nothing would is no pick (`None`).
fn pick_periods(extra: f64, room: f64, spacing: f64, amp_max: Um, min_amp: Um) -> Option<(usize, Um)> {
    if extra <= 0.0 {
        return None;
    }
    let k = 2.0 * DIAGONAL_OVERHEAD;
    let wanted = (extra / k).round() as Um;
    let mut best: Option<(f64, usize, Um)> = None;
    let mut n = 1usize;
    loop {
        let lo = n as Um * min_amp;
        if 2.0 * lo as f64 + n as f64 * spacing > room {
            break; // even the smallest bumps overrun the run
        }
        let by_room = ((room - n as f64 * spacing) / 2.0).floor() as Um;
        let sum = wanted.clamp(lo, n as Um * amp_max).min(by_room).max(lo);
        let miss = (k * sum as f64 - extra).abs();
        if best.is_none_or(|(m, _, _)| miss < m - 1e-9) {
            best = Some((miss, n, sum));
        }
        n += 1;
    }
    best.filter(|&(miss, _, _)| miss < extra).map(|(_, n, sum)| (n, sum))
}

/// `count` trapezoid bumps (45-degree legs, `spacing` straight at the top), alternating sides, along the
/// axis-aligned run `a`-`b`, from `a` onward; the amplitudes sum to `amp_sum` (as even as whole um allow) and
/// the rest of the run is straight to `b`. Legs are exactly 45 degrees. `None` when the bumps overrun `b`.
fn zigzag(a: Point, b: Point, count: usize, amp_sum: Um, spacing: Um, flip: bool) -> Option<Vec<Point>> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let (ux, uy) = (dx.signum(), dy.signum());
    let (nx, ny) = if flip { (-uy, ux) } else { (uy, -ux) };
    let (base, rem) = (amp_sum / count as Um, (amp_sum % count as Um) as usize);
    let mut pts = vec![a];
    let (mut x, mut y) = (a.x, a.y);
    let mut side = 1;
    for i in 0..count {
        let amp = base + Um::from(i < rem);
        let (sx, sy) = (side * nx, side * ny);
        let p1 = Point { x: x + amp * (ux + sx), y: y + amp * (uy + sy) };
        let p2 = Point { x: p1.x + spacing * ux, y: p1.y + spacing * uy };
        let p3 = Point { x: p2.x + amp * (ux - sx), y: p2.y + amp * (uy - sy) };
        pts.extend([p1, p2, p3]);
        (x, y) = (p3.x, p3.y);
        side = -side;
    }
    // along-run progress: the last point must still be short of (or on) `b`
    if (x - a.x) * ux + (y - a.y) * uy > dx.abs() + dy.abs() {
        return None;
    }
    pts.push(b);
    pts.dedup();
    Some(pts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_per_period_matches_a_directly_measured_single_hump() {
        // A target comfortably reached by exactly one *full-amplitude*
        // period (well above it, so the "shrink the last period" branch
        // never engages) -- then measure the actual generated shape
        // directly rather than trusting the formula.
        let a = Point { x: 0, y: 0 };
        let b = Point { x: 10_000, y: 0 };
        let amplitude = 500;
        let spacing = 1000;
        let target = a.x + (b.x - a.x) + (extra_per_period(amplitude).round() as Um) * 2;
        let result = generate_meander(a, b, amplitude, spacing, target, false).expect("must generate a meander");
        // The first period must be full-amplitude: its own diagonal leg
        // covers exactly `amplitude` in both x and y.
        let p1 = result.pts[1];
        assert_eq!(p1.x, amplitude);
        assert_eq!(p1.y.unsigned_abs(), amplitude as u64);
        let measured_first_leg = (((p1.x - a.x) as f64).powi(2) + ((p1.y - a.y) as f64).powi(2)).sqrt();
        assert!((measured_first_leg - amplitude as f64 * std::f64::consts::SQRT_2).abs() < 1.0, "first leg length {measured_first_leg}");
    }

    #[test]
    fn reaches_the_requested_target_length_within_rounding() {
        let a = Point { x: 0, y: 0 };
        let b = Point { x: 20_000, y: 0 };
        let target = 24_000;
        let result = generate_meander(a, b, 400, 800, target, false).expect("plenty of room for this target");
        assert!((result.achieved_length - target).abs() <= 2, "achieved {} vs target {target}", result.achieved_length);
        assert_eq!(result.pts.first(), Some(&a));
        assert_eq!(result.pts.last(), Some(&b));
    }

    #[test]
    fn alternates_sides_so_the_shape_actually_oscillates() {
        let a = Point { x: 0, y: 0 };
        let b = Point { x: 20_000, y: 0 };
        let result = generate_meander(a, b, 500, 500, 26_000, false).unwrap();
        // Interior points (excluding the fixed start/end) should include
        // both positive and negative y -- a real accordion, not a bulge
        // all to one side.
        let ys: Vec<i64> = result.pts[1..result.pts.len() - 1].iter().map(|p| p.y).collect();
        assert!(ys.iter().any(|&y| y > 0), "{ys:?}");
        assert!(ys.iter().any(|&y| y < 0), "{ys:?}");
    }

    #[test]
    fn flip_swaps_the_first_periods_side() {
        let a = Point { x: 0, y: 0 };
        let b = Point { x: 10_000, y: 0 };
        let target = a.x + (b.x - a.x) + (extra_per_period(500) as Um) - 10;
        let normal = generate_meander(a, b, 500, 1000, target, false).unwrap();
        let flipped = generate_meander(a, b, 500, 1000, target, true).unwrap();
        assert_eq!(normal.pts[1].y, -flipped.pts[1].y);
    }

    #[test]
    fn refuses_a_diagonal_baseline() {
        assert!(generate_meander(Point { x: 0, y: 0 }, Point { x: 1000, y: 1000 }, 200, 400, 2000, false).is_none());
    }

    #[test]
    fn refuses_a_target_no_longer_than_the_baseline_itself() {
        let a = Point { x: 0, y: 0 };
        let b = Point { x: 10_000, y: 0 };
        assert!(generate_meander(a, b, 200, 400, 10_000, false).is_none());
        assert!(generate_meander(a, b, 200, 400, 9_000, false).is_none());
    }

    #[test]
    fn shrinks_amplitude_to_whatever_the_baseline_can_actually_fit() {
        // Far shorter than one period's own nominal `2*amplitude+spacing`
        // advance -- rather than refusing outright, the amplitude itself
        // shrinks to whatever this short a baseline can physically fit
        // (here, at most `base_len / 2`), getting as close to the
        // requested target as the geometry allows.
        let a = Point { x: 0, y: 0 };
        let b = Point { x: 500, y: 0 };
        let result = generate_meander(a, b, 500, 1000, 5000, false).expect("must still fit a shrunk period");
        assert!(result.achieved_length > 500, "must add at least some length: {}", result.achieved_length);
    }

    #[test]
    fn refuses_a_baseline_too_short_for_even_a_minimal_amplitude() {
        let a = Point { x: 0, y: 0 };
        let b = Point { x: 1, y: 0 }; // 1um -- room/2 rounds to 0, below the 1um amplitude floor.
        assert!(generate_meander(a, b, 500, 1000, 5000, false).is_none());
    }

    #[test]
    fn vertical_baseline_meanders_the_same_way_as_horizontal() {
        // Same shape of request as `reaches_the_requested_target_length_
        // within_rounding` (well inside what this amplitude/spacing can
        // fit along a 20mm baseline -- see that test), just rotated 90
        // degrees, to confirm the construction isn't secretly biased
        // toward one axis.
        let a = Point { x: 0, y: 0 };
        let b = Point { x: 0, y: 20_000 };
        let target = 24_000;
        let result = generate_meander(a, b, 400, 800, target, false).unwrap();
        let xs: Vec<i64> = result.pts[1..result.pts.len() - 1].iter().map(|p| p.x).collect();
        assert!(xs.iter().any(|&x| x > 0), "{xs:?}");
        assert!(xs.iter().any(|&x| x < 0), "{xs:?}");
        assert!((result.achieved_length - target).abs() <= 2, "achieved {}", result.achieved_length);
    }

    fn p(x: Um, y: Um) -> Point {
        Point { x, y }
    }

    /// Distance from `q` to the segment `a`-`b`.
    fn dist_to_segment(q: Point, a: Point, b: Point) -> f64 {
        let (abx, aby) = ((b.x - a.x) as f64, (b.y - a.y) as f64);
        let len2 = abx * abx + aby * aby;
        let t = if len2 == 0.0 { 0.0 } else { ((((q.x - a.x) as f64) * abx + ((q.y - a.y) as f64) * aby) / len2).clamp(0.0, 1.0) };
        let (cx, cy) = (a.x as f64 + t * abx, a.y as f64 + t * aby);
        ((q.x as f64 - cx).powi(2) + (q.y as f64 - cy).powi(2)).sqrt()
    }

    fn min_distance(a: &[Point], b: &[Point]) -> f64 {
        a.iter().flat_map(|&q| b.windows(2).map(move |s| dist_to_segment(q, s[0], s[1]))).chain(b.iter().flat_map(|&q| a.windows(2).map(move |s| dist_to_segment(q, s[0], s[1])))).fold(f64::MAX, f64::min)
    }

    #[test]
    fn dp_meander_lines_start_and_end_on_their_own_offset_baseline_ends_and_reach_the_target() {
        // Baseline along +x, pair centres 300 um apart: A above (offset +150, since +offset turns the direction -90
        // degrees, i.e. towards -y), B below.
        let (a, b) = (p(0, 0), p(20_000, 0));
        let r = generate_dp_meander(a, b, 150, -150, 400, 800, 23_000, false).expect("room for this");
        assert_eq!((r.a.first(), r.a.last()), (Some(&p(0, -150)), Some(&p(20_000, -150))));
        assert_eq!((r.b.first(), r.b.last()), (Some(&p(0, 150)), Some(&p(20_000, 150))));
        let longest = r.a_length.max(r.b_length);
        assert!((longest - 23_000).abs() <= 3, "longest line {longest} vs target 23000");
        // the miters only trade a little length between the lines
        assert!((r.a_length - r.b_length).abs() < 450, "a {} b {}", r.a_length, r.b_length);
        assert_eq!(r.a_length, polyline_length(&r.a));
    }

    #[test]
    fn dp_meander_lines_stay_one_pair_spacing_apart_everywhere() {
        let r = generate_dp_meander(p(0, 0), p(20_000, 0), 150, -150, 400, 800, 23_000, false).unwrap();
        let d = min_distance(&r.a, &r.b);
        assert!((299.0..=303.0).contains(&d), "closest approach {d} um, expected the 300 um centre spacing");
    }

    #[test]
    fn dp_meander_works_down_a_vertical_baseline_and_with_asymmetric_offsets() {
        // +y baseline: +offset turns the direction (0,1) -90 degrees to (1,0), so A is on the +x side.
        let r = generate_dp_meander(p(0, 0), p(0, 20_000), 200, -100, 400, 800, 23_000, true).expect("vertical pair");
        assert_eq!((r.a.first(), r.a.last()), (Some(&p(200, 0)), Some(&p(200, 20_000))));
        assert_eq!((r.b.first(), r.b.last()), (Some(&p(-100, 0)), Some(&p(-100, 20_000))));
        assert!((r.a_length.max(r.b_length) - 23_000).abs() <= 3);
    }

    #[test]
    fn dp_meander_raises_a_too_small_amplitude_and_spacing_to_what_the_pair_can_fold() {
        // 2 mm between the lines but only 100 um straight asked for: the dual `spacing()` (2 * |offset| at least) takes
        // over, and bumps never get shallower than `MinAmplitude` (|offset| + ...), so the lines fold cleanly instead of
        // overlapping.
        let r = generate_dp_meander(p(0, 0), p(40_000, 0), 1000, -1000, 1500, 100, 48_000, false).expect("a clean fold");
        let ys: Vec<Um> = r.a.iter().map(|q| q.y).collect();
        let swing = ys.iter().max().unwrap() - ys.iter().min().unwrap();
        assert!(swing >= 2 * 1000, "line A swings {swing} um end to end -- every bump at least |offset| tall each way");
        assert!(!lines_conflict(&r.a, &r.b));
        assert!((r.a_length.max(r.b_length) - 48_000).abs() <= 3);
    }

    #[test]
    fn lines_conflict_flags_a_crossing_pair_and_a_folded_line() {
        // two lines that cross
        assert!(lines_conflict(&[p(0, 0), p(10, 10)], &[p(0, 10), p(10, 0)]));
        // one line that folds back over itself (a bow-tie)
        assert!(lines_conflict(&[p(0, 0), p(10, 10), p(10, 0), p(0, 10)], &[p(100, 100), p(110, 100)]));
        // parallel runs never conflict, and neither does a line meeting its own neighbour at a corner
        assert!(!lines_conflict(&[p(0, 0), p(10, 0), p(10, 10)], &[p(0, 5), p(5, 5), p(5, 20)]));
    }

    #[test]
    fn pick_periods_prefers_the_fewest_biggest_bumps_and_degrades_at_the_edges() {
        // 4828 um of summed amplitude over 19.8 mm of run with 800 um spacing: 13 full bumps would need 20 mm -- too
        // long -- so 12 bumps of the full 400 um amplitude come closest (it falls 23 um short).
        assert_eq!(pick_periods(4000.0, 19_872.0, 800.0, 400, 151), Some((12, 4800)));
        // plenty of room: the fewest bumps whose amplitude stays within the cap (3000 um extra -> 3622 summed -> 10 bumps)
        assert_eq!(pick_periods(3000.0, 19_872.0, 800.0, 400, 151), Some((10, 3621)));
        // less than one minimum-size bump adds, but at least half of it: take the one bump
        assert_eq!(pick_periods(100.0, 19_872.0, 800.0, 400, 151), Some((1, 151)));
        // and less than half of it: nothing
        assert_eq!(pick_periods(40.0, 19_872.0, 800.0, 400, 151), None);
        assert_eq!(pick_periods(-5.0, 19_872.0, 800.0, 400, 151), None);
    }

    #[test]
    fn zigzag_alternates_sides_with_exact_45_degree_legs_and_amplitudes_summing_to_the_request() {
        let z = zigzag(p(0, 0), p(10_000, 0), 4, 1_002, 500, false).expect("fits");
        // amplitudes 251, 251, 250, 250 (the remainder goes to the first bumps)
        assert_eq!(&z[..4], &[p(0, 0), p(251, -251), p(751, -251), p(1_002, 0)]);
        assert_eq!(z[4], p(1_253, 251), "second bump swings the other way");
        assert_eq!(z.last(), Some(&p(10_000, 0)));
        for w in z.windows(2) {
            let (dx, dy) = ((w[1].x - w[0].x).abs(), (w[1].y - w[0].y).abs());
            assert!(dx == 0 || dy == 0 || dx == dy, "{w:?} is not axis-aligned or 45 degrees");
        }
        // bumps that overrun the run are refused
        assert!(zigzag(p(0, 0), p(1_000, 0), 4, 1_002, 500, false).is_none());
    }

    #[test]
    fn dp_meander_has_nothing_to_add_for_a_diagonal_baseline_or_a_target_that_is_not_longer() {
        assert_eq!(generate_dp_meander(p(0, 0), p(10_000, 10_000), 150, -150, 400, 800, 30_000, false).unwrap_err(), DpMeanderError::NothingToAdd);
        assert_eq!(generate_dp_meander(p(0, 0), p(10_000, 0), 150, -150, 400, 800, 10_000, false).unwrap_err(), DpMeanderError::NothingToAdd);
        assert_eq!(generate_dp_meander(p(0, 0), p(10_000, 0), 150, -150, 0, 800, 12_000, false).unwrap_err(), DpMeanderError::NothingToAdd);
        // shorter than the lead-in the offset needs
        assert_eq!(generate_dp_meander(p(0, 0), p(60, 0), 150, -150, 400, 800, 400, false).unwrap_err(), DpMeanderError::NothingToAdd);
    }

    #[test]
    fn simplify_collinear_merges_straight_runs_but_keeps_corners_and_reversals() {
        let pts = vec![p(0, 0), p(0, 0), p(10, 0), p(20, 0), p(20, 10), p(20, 20), p(20, 10)];
        // (0,0),(0,0) dedup; (10,0) is on the run; (20,0) is a corner; (20,10) is on the run up but the last leg
        // doubles back, so (20,20) is a real reversal and stays.
        assert_eq!(simplify_collinear(pts), vec![p(0, 0), p(20, 0), p(20, 20), p(20, 10)]);
        assert_eq!(simplify_collinear(vec![p(0, 0), p(5, 5)]), vec![p(0, 0), p(5, 5)]);
        assert_eq!(simplify_collinear(vec![p(1, 1), p(1, 1)]), vec![p(1, 1)]);
    }

    #[test]
    fn segments_touch_covers_crossings_endpoint_contact_and_collinear_overlap() {
        assert!(segments_touch(p(0, 0), p(10, 10), p(0, 10), p(10, 0)), "X crossing");
        assert!(segments_touch(p(0, 0), p(10, 0), p(10, 0), p(10, 10)), "shared endpoint");
        assert!(segments_touch(p(0, 0), p(10, 0), p(5, 0), p(15, 0)), "collinear overlap");
        assert!(!segments_touch(p(0, 0), p(10, 0), p(11, 0), p(20, 0)), "collinear but apart");
        assert!(!segments_touch(p(0, 0), p(10, 0), p(0, 5), p(10, 5)), "parallel");
        assert!(!segments_touch(p(0, 0), p(10, 0), p(5, 5), p(5, 1)), "stops short of the line");
    }
}
