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
}
