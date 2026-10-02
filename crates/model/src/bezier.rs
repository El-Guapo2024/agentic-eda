//! Port of `BEZIER_POLY::GetPoly` for cubic curves (`libs/kimath/src/
//! bezier_curves.cpp`; "Fast, Precise Flattening of Cubic Bezier segments"
//! by Hain et al., with a recursive-subdivision fallback) -- the polyline
//! KiCad draws, plots and design-rule-checks for a `PCB_SHAPE` of type
//! `BEZIER` (`EDA_SHAPE::RebuildBezierToSegmentsPointsList( maxError )`).
//!
//! The studio carries the same algorithm client-side for the canvas
//! (`web/studio/src/kicad-port/bezierPoly.ts`); keep the two in step.

use crate::ir::{Point, Um};

type V = (f64, f64);

fn sub(a: V, b: V) -> V {
    (a.0 - b.0, a.1 - b.1)
}
fn add(a: V, b: V) -> V {
    (a.0 + b.0, a.1 + b.1)
}
fn mul(a: V, s: f64) -> V {
    (a.0 * s, a.1 * s)
}
fn dot(a: V, b: V) -> f64 {
    a.0 * b.0 + a.1 * b.1
}
fn cross(a: V, b: V) -> f64 {
    a.0 * b.1 - a.1 * b.0
}
fn norm2(a: V) -> f64 {
    a.0 * a.0 + a.1 * a.1
}

/// The four control points (`BEZIER_POLY::m_ctrlPts`).
type Cubic = [V; 4];

/// `KiROUND`: round half away from zero.
fn ki_round(v: f64) -> i64 {
    v.round() as i64
}

fn is_nan(c: &Cubic) -> bool {
    c.iter().any(|p| p.0.is_nan() || p.1.is_nan())
}

/// `BEZIER_POLY::isFlat` (4 control points).
fn is_flat(c: &Cubic, max_error: f64) -> bool {
    let delta = sub(c[3], c[0]);
    let d21 = sub(c[1], c[0]);
    let d31 = sub(c[2], c[0]);
    let cross1 = cross(delta, d21);
    let cross2 = cross(delta, d31);
    let inv_delta_sq = 1.0 / norm2(delta);
    let d1 = cross1 * cross1 * inv_delta_sq;
    let d2 = cross2 * cross2 * inv_delta_sq;
    let factor = if cross1 * cross2 > 0.0 { 3.0 / 4.0 } else { 4.0 / 9.0 };
    let f2 = factor * factor;
    let tol = max_error * max_error;
    d1 * f2 <= tol && d2 * f2 <= tol
}

/// `BEZIER_POLY::subdivide( aT, left, right )`.
fn subdivide(c: &Cubic, t: f64) -> (Cubic, Cubic) {
    let left1 = add(c[0], mul(sub(c[1], c[0]), t));
    let tmp = add(c[1], mul(sub(c[2], c[1]), t));
    let left2 = add(left1, mul(sub(tmp, left1), t));
    let right2 = add(c[2], mul(sub(c[3], c[2]), t));
    let right1 = add(tmp, mul(sub(right2, tmp), t));
    let shared = add(left2, mul(sub(right1, left2), t));
    ([c[0], left1, left2, shared], [shared, right1, right2, c[3]])
}

/// `BEZIER_POLY::recursiveSegmentation`: depth-first halving until each piece is flat; zero-length pieces are dropped.
fn recursive_segmentation(c: Cubic, out: &mut Vec<V>, threshold: f64) {
    let mut stack: Vec<Cubic> = vec![c];
    while let Some(&bezier) = stack.last() {
        if bezier[3] == bezier[0] {
            stack.pop();
        } else if is_flat(&bezier, threshold) {
            out.push(bezier[3]);
            stack.pop();
        } else {
            let (left, right) = subdivide(&bezier, 0.5);
            let top = stack.len() - 1;
            stack[top] = right;
            stack.push(left);
        }
    }
}

/// `BEZIER_POLY::numberOfInflectionPoints`.
fn number_of_inflection_points(c: &Cubic) -> i32 {
    let d21 = sub(c[1], c[0]);
    let d32 = sub(c[2], c[1]);
    let d43 = sub(c[3], c[2]);
    let cross1 = cross(d21, d32) * cross(d32, d43);
    let cross2 = cross(d21, d32) * cross(d21, d43);
    if cross1 < 0.0 {
        return 1;
    } else if cross2 > 0.0 {
        return 0;
    }
    let b1 = dot(d21, d32) > 0.0;
    let b2 = dot(d32, d43) > 0.0;
    if b1 ^ b2 {
        return 0;
    }
    -1 // "rare cases where there are potentially 2 or 0 inflection points"
}

/// `BEZIER_POLY::findInflectionPoints`: how many (0-2) and where (`t1 <= t2`).
fn find_inflection_points(c: &Cubic) -> (i32, f64, f64) {
    let a: V = (-c[0].0 + 3.0 * c[1].0 - 3.0 * c[2].0 + c[3].0, -c[0].1 + 3.0 * c[1].1 - 3.0 * c[2].1 + c[3].1);
    let b: V = (3.0 * c[0].0 - 6.0 * c[1].0 + 3.0 * c[2].0, 3.0 * c[0].1 - 6.0 * c[1].1 + 3.0 * c[2].1);
    let cc: V = (-3.0 * c[0].0 + 3.0 * c[1].0, -3.0 * c[0].1 + 3.0 * c[1].1);
    let qa = 3.0 * cross(a, b);
    let qb = 3.0 * cross(a, cc);
    let qc = cross(b, cc);
    let r2 = qb * qb - 4.0 * qa * qc;
    if r2 >= 0.0 && qa != 0.0 {
        let r = r2.sqrt();
        let mut t1 = (-qb + r) / (2.0 * qa);
        let mut t2 = (-qb - r) / (2.0 * qa);
        let in1 = t1 > 0.0 && t1 < 1.0;
        let in2 = t2 > 0.0 && t2 < 1.0;
        if in1 && in2 {
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            return (if t2 - t1 > 0.00001 { 2 } else { 1 }, t1, t2);
        } else if in1 {
            return (1, t1, 0.0);
        } else if in2 {
            return (1, t2, 0.0);
        }
    }
    (0, 0.0, 0.0)
}

/// `BEZIER_POLY::thirdControlPointDeviation`.
fn third_control_point_deviation(c: &Cubic) -> f64 {
    let delta = sub(c[1], c[0]);
    let len_sq = norm2(delta);
    if len_sq < 1e-6 {
        return 0.0;
    }
    let len = len_sq.sqrt();
    let r = (c[1].1 - c[0].1) / len;
    let s = (c[0].0 - c[1].0) / len;
    let u = (c[1].0 * c[0].1 - c[0].0 * c[1].1) / len;
    (r * c[2].0 + s * c[2].1 + u).abs()
}

/// `BEZIER_POLY::cubicParabolicApprox` (Hain et al.'s formula 2 picks each step's `t`).
fn cubic_parabolic_approx(start: Cubic, out: &mut Vec<V>, max_error: f64) {
    let mut c = start;
    loop {
        if is_nan(&c) {
            break;
        }
        if is_flat(&c, max_error) {
            out.push(c[3]);
            break;
        }
        let d = third_control_point_deviation(&c);
        let t = 2.0 * (max_error / (3.0 * d)).sqrt();
        if t > 1.0 {
            // "Case where the t value calculated is invalid, so use recursive subdivision"
            recursive_segmentation(c, out, max_error);
            break;
        }
        let (b1, b2) = subdivide(&c, t);
        if is_flat(&b1, max_error) {
            out.push(b1[3]);
        } else {
            recursive_segmentation(b1, out, max_error); // "use segment to handle any mathematical errors"
        }
        c = b2;
    }
}

/// `BEZIER_POLY::getCubicPoly`: split at inflection points, parabolic approximation per piece.
fn get_cubic_poly(c: Cubic, out: &mut Vec<V>, max_error: f64) {
    out.push(c[0]);
    if number_of_inflection_points(&c) == 0 {
        cubic_parabolic_approx(c, out, max_error);
        return;
    }
    let (n, t1, _t2) = find_inflection_points(&c);
    if n == 2 {
        let (sub1, tmp1) = subdivide(&c, t1);
        cubic_parabolic_approx(sub1, out, max_error);
        let (n2, u1, _u2) = find_inflection_points(&tmp1);
        if n2 == 2 || n2 == 1 {
            let (sub2, sub3) = subdivide(&tmp1, u1);
            recursive_segmentation(sub2, out, max_error); // "Use Segment for the second (middle) subsegment"
            cubic_parabolic_approx(sub3, out, max_error);
        } else {
            out.push(tmp1[3]);
        }
    } else if n == 1 {
        let (sub1, sub2) = subdivide(&c, t1);
        cubic_parabolic_approx(sub1, out, max_error);
        cubic_parabolic_approx(sub2, out, max_error);
    } else {
        cubic_parabolic_approx(c, out, max_error);
    }
}

/// `BEZIER_POLY( start, c1, c2, end ).GetPoly( out, maxError )`: the integer polyline
/// (start first, end last) within `max_error` (board units -- micrometres here) of the
/// true curve. `max_error <= 0` falls back to 10, as the C++ does.
pub fn bezier_polyline(start: Point, c1: Point, c2: Point, end: Point, max_error: Um) -> Vec<Point> {
    let err = if max_error <= 0 { 10.0 } else { max_error as f64 };
    let v = |p: Point| (p.x as f64, p.y as f64);
    let mut out: Vec<V> = Vec::new();
    get_cubic_poly([v(start), v(c1), v(c2), v(end)], &mut out, err);
    out.into_iter().map(|p| Point { x: ki_round(p.0), y: ki_round(p.1) }).collect()
}

/// `EDA_SHAPE::getMaxError()` for the board: `BOARD_DESIGN_SETTINGS::m_MaxError`
/// defaults to 0.005 mm (`pcbIUScale.mmToIU( 0.005 )`), i.e. 5 um.
pub const BEZIER_MAX_ERROR_UM: Um = 5;

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i64, y: i64) -> Point {
        Point { x, y }
    }

    fn dist_to_polyline(q: (f64, f64), poly: &[Point]) -> f64 {
        let mut best = f64::INFINITY;
        for w in poly.windows(2) {
            let (ax, ay) = (w[0].x as f64, w[0].y as f64);
            let (bx, by) = (w[1].x as f64, w[1].y as f64);
            let (dx, dy) = (bx - ax, by - ay);
            let len2 = dx * dx + dy * dy;
            let t = if len2 == 0.0 { 0.0 } else { (((q.0 - ax) * dx + (q.1 - ay) * dy) / len2).clamp(0.0, 1.0) };
            best = best.min((q.0 - (ax + t * dx)).hypot(q.1 - (ay + t * dy)));
        }
        best
    }

    fn at(s: Point, c1: Point, c2: Point, e: Point, t: f64) -> (f64, f64) {
        let omt = 1.0 - t;
        let (a, b, c, d) = (omt * omt * omt, 3.0 * t * omt * omt, 3.0 * t * t * omt, t * t * t);
        (a * s.x as f64 + b * c1.x as f64 + c * c2.x as f64 + d * e.x as f64, a * s.y as f64 + b * c1.y as f64 + c * c2.y as f64 + d * e.y as f64)
    }

    #[test]
    fn a_straight_curve_flattens_to_its_end_points() {
        assert_eq!(bezier_polyline(p(0, 0), p(100, 0), p(200, 0), p(300, 0), 5), vec![p(0, 0), p(300, 0)]);
    }

    #[test]
    fn a_rounded_curve_stays_within_max_error_and_keeps_its_ends() {
        let (s, c1, c2, e) = (p(0, 0), p(0, 10_000), p(10_000, 10_000), p(10_000, 0));
        for max_error in [5, 50, 500] {
            let poly = bezier_polyline(s, c1, c2, e, max_error);
            assert_eq!(poly.first(), Some(&s));
            assert_eq!(poly.last(), Some(&e));
            for i in 0..=200 {
                let d = dist_to_polyline(at(s, c1, c2, e, i as f64 / 200.0), &poly);
                assert!(d <= max_error as f64 + 1.5, "max_error {max_error}: sample {i} is {d:.2} from the polyline");
            }
        }
        assert!(bezier_polyline(s, c1, c2, e, 5).len() > bezier_polyline(s, c1, c2, e, 500).len());
    }

    #[test]
    fn an_s_curve_with_inflection_points_is_within_tolerance() {
        let (s, c1, c2, e) = (p(0, 0), p(8_000, 12_000), p(2_000, -12_000), p(10_000, 0));
        let poly = bezier_polyline(s, c1, c2, e, 20);
        assert_eq!(poly.first(), Some(&s));
        assert_eq!(poly.last(), Some(&e));
        for i in 0..=400 {
            let d = dist_to_polyline(at(s, c1, c2, e, i as f64 / 400.0), &poly);
            assert!(d <= 21.5, "sample {i} is {d:.2} from the polyline");
        }
    }

    #[test]
    fn a_closed_loop_still_produces_a_real_polyline() {
        let poly = bezier_polyline(p(0, 0), p(5_000, 5_000), p(-5_000, 5_000), p(0, 0), 10);
        assert!(poly.len() > 4);
        assert_eq!(poly.first(), Some(&p(0, 0)));
        assert_eq!(poly.last(), Some(&p(0, 0)));
    }

    #[test]
    fn a_non_positive_max_error_falls_back_to_ten() {
        assert_eq!(bezier_polyline(p(0, 0), p(0, 4_000), p(4_000, 4_000), p(4_000, 0), 0), bezier_polyline(p(0, 0), p(0, 4_000), p(4_000, 4_000), p(4_000, 0), 10));
    }
}
