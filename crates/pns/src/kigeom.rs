//! The pieces of KiCad's `libs/kimath` the length-tuning port stands on (`VECTOR2D::Resize`, `EDA_ANGLE`, `RotatePoint`,
//! `SEG`, `CalcArcCenter`, `SHAPE_ARC`), ported function by function so that `pns_meander.cpp` computed over them gives KiCad's
//! own numbers.
//!
//! **Every coordinate in this module (and in [`crate::chain`], [`crate::meander`], [`crate::meander_placer`]) is a nanometre**, the
//! unit KiCad computes in: the meander's thresholds are nanometre constants (`forward()` skips a run shorter than 5, the length
//! tolerance is 20, the arc polygonisation error is 1000), and its integer truncations and roundings only come out the same at
//! that grid. The rest of this crate (and the IR) is micrometres; [`to_nm`] and [`to_um`] cross the border, once in each direction.

use crate::seg_ops::{self, rescale};
use eda_model::ir::Point;

/// Micrometres to nanometres.
pub const NM_PER_UM: i64 = 1000;

/// A micrometre point as the nanometre point KiCad would hold.
pub fn to_nm(p: Point) -> Point {
    Point { x: p.x * NM_PER_UM, y: p.y * NM_PER_UM }
}

/// A nanometre point on the micrometre grid the IR lives on (halves away from zero).
pub fn to_um(p: Point) -> Point {
    let r = |v: i64| if v >= 0 { (v + NM_PER_UM / 2) / NM_PER_UM } else { -((-v + NM_PER_UM / 2) / NM_PER_UM) };
    Point { x: r(p.x), y: r(p.y) }
}

/// `KiROUND( v )`: halves away from zero.
pub fn ki_round(v: f64) -> i64 {
    (if v < 0.0 { v - 0.5 } else { v + 0.5 }) as i64
}

/// `isqrt` of `libs/kimath/src/geometry/seg.cpp`: the integer square root, rounded down.
pub fn isqrt(x: i128) -> i64 {
    if x < 0 {
        return i64::MAX;
    }
    let mut r = (x as f64).sqrt() as i128;
    while r * r < x {
        r += 1;
    }
    while r * r > x {
        r -= 1;
    }
    r as i64
}

// ---------------------------------------------------------------------------------------------------------------------
// VECTOR2D, EDA_ANGLE, RotatePoint
// ---------------------------------------------------------------------------------------------------------------------

/// `VECTOR2D`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct V2 {
    pub x: f64,
    pub y: f64,
}

impl V2 {
    pub fn new(x: f64, y: f64) -> V2 {
        V2 { x, y }
    }

    /// `VECTOR2D( const VECTOR2I& )`.
    pub fn of(p: Point) -> V2 {
        V2 { x: p.x as f64, y: p.y as f64 }
    }

    /// `VECTOR2I( const VECTOR2D& )`: the coordinates truncated toward zero ("beware of rounding issues").
    pub fn trunc(self) -> Point {
        Point { x: self.x as i64, y: self.y as i64 }
    }

    pub fn add(self, o: V2) -> V2 {
        V2 { x: self.x + o.x, y: self.y + o.y }
    }

    pub fn sub(self, o: V2) -> V2 {
        V2 { x: self.x - o.x, y: self.y - o.y }
    }

    pub fn scale(self, f: f64) -> V2 {
        V2 { x: self.x * f, y: self.y * f }
    }

    /// `VECTOR2::Perpendicular`.
    pub fn perpendicular(self) -> V2 {
        V2 { x: -self.y, y: self.x }
    }

    /// `VECTOR2D::EuclideanNorm`.
    pub fn norm(self) -> f64 {
        if self.x.abs() == self.y.abs() {
            return self.x.abs() * std::f64::consts::SQRT_2;
        }
        if self.x == 0.0 {
            return self.y.abs();
        }
        if self.y == 0.0 {
            return self.x.abs();
        }
        self.x.hypot(self.y)
    }

    /// `VECTOR2::Resize( aNewLength )` for a vector of doubles.
    pub fn resize(self, new_length: f64) -> V2 {
        if self.x == 0.0 && self.y == 0.0 {
            return V2 { x: 0.0, y: 0.0 };
        }
        let (new_x, new_y);
        if self.x.abs() == self.y.abs() {
            new_x = new_length.abs() * std::f64::consts::FRAC_1_SQRT_2;
            new_y = new_x;
        } else {
            let x_sq = self.x * self.x;
            let y_sq = self.y * self.y;
            let l_sq = x_sq + y_sq;
            let new_length_sq = new_length * new_length;
            // `rescale( T, T, T )` for a floating type is `aNumerator * aValue / aDenominator`.
            new_x = (new_length_sq * x_sq / l_sq).sqrt();
            new_y = (new_length_sq * y_sq / l_sq).sqrt();
        }
        let sign = if new_length > 0.0 { 1.0 } else if new_length < 0.0 { -1.0 } else { 0.0 };
        V2 { x: (if self.x < 0.0 { -new_x } else { new_x }) * sign, y: (if self.y < 0.0 { -new_y } else { new_y }) * sign }
    }
}

/// `EDA_ANGLE::Normalize`, degrees into `[0, 360)`.
pub fn normalize(mut deg: f64) -> f64 {
    while deg < -0.0 {
        deg += 360.0;
    }
    while deg >= 360.0 {
        deg -= 360.0;
    }
    deg
}

const DEGREES_TO_RADIANS: f64 = std::f64::consts::PI / 180.0;

/// `EDA_ANGLE::Sin`: exact for the multiples of 45 degrees.
pub fn sin_deg(deg: f64) -> f64 {
    let t = normalize(deg);
    if t == 0.0 || t == 180.0 {
        0.0
    } else if t == 45.0 || t == 135.0 {
        std::f64::consts::FRAC_1_SQRT_2
    } else if t == 225.0 || t == 315.0 {
        -std::f64::consts::FRAC_1_SQRT_2
    } else if t == 90.0 {
        1.0
    } else if t == 270.0 {
        -1.0
    } else {
        (deg * DEGREES_TO_RADIANS).sin()
    }
}

/// `EDA_ANGLE::Cos`.
pub fn cos_deg(deg: f64) -> f64 {
    let t = normalize(deg);
    if t == 0.0 {
        1.0
    } else if t == 180.0 {
        -1.0
    } else if t == 90.0 || t == 270.0 {
        0.0
    } else if t == 45.0 || t == 315.0 {
        std::f64::consts::FRAC_1_SQRT_2
    } else if t == 135.0 || t == 225.0 {
        -std::f64::consts::FRAC_1_SQRT_2
    } else {
        (deg * DEGREES_TO_RADIANS).cos()
    }
}

/// `EDA_ANGLE( const VECTOR2D& )`: the direction of a vector in degrees, `(-180, 180]`-ish, with KiCad's exact values on the axes and the diagonals.
pub fn angle_of(x: f64, y: f64) -> f64 {
    if x == 0.0 && y == 0.0 {
        0.0
    } else if y == 0.0 {
        if x >= 0.0 {
            0.0
        } else {
            -180.0
        }
    } else if x == 0.0 {
        if y >= 0.0 {
            90.0
        } else {
            -90.0
        }
    } else if x == y {
        if x >= 0.0 {
            45.0
        } else {
            -180.0 + 45.0
        }
    } else if x == -y {
        if x >= 0.0 {
            -45.0
        } else {
            180.0 - 45.0
        }
    } else {
        y.atan2(x) / DEGREES_TO_RADIANS
    }
}

/// `RotatePoint( double* pX, double* pY, aAngle )` (positive is counter-clockwise on a y-up screen, KiCad's convention).
pub fn rotate_d(v: V2, angle_deg: f64) -> V2 {
    let angle = normalize(angle_deg);
    if angle == 0.0 {
        v
    } else if angle == 90.0 {
        V2 { x: v.y, y: -v.x }
    } else if angle == 180.0 {
        V2 { x: -v.x, y: -v.y }
    } else if angle == 270.0 {
        V2 { x: -v.y, y: v.x }
    } else {
        let (sinus, cosinus) = (sin_deg(angle), cos_deg(angle));
        V2 { x: v.y * sinus + v.x * cosinus, y: v.y * cosinus - v.x * sinus }
    }
}

/// `RotatePoint( int* pX, int* pY, aAngle )`: the same for integer coordinates, rounded.
fn rotate_i(x: i64, y: i64, angle_deg: f64) -> (i64, i64) {
    let angle = normalize(angle_deg);
    if angle == 0.0 {
        (x, y)
    } else if angle == 90.0 {
        (y, -x)
    } else if angle == 180.0 {
        (-x, -y)
    } else if angle == 270.0 {
        (-y, x)
    } else {
        let (sinus, cosinus) = (sin_deg(angle), cos_deg(angle));
        (ki_round(y as f64 * sinus + x as f64 * cosinus), ki_round(y as f64 * cosinus - x as f64 * sinus))
    }
}

/// `RotatePoint( VECTOR2I& aPoint, const VECTOR2I& aCentre, aAngle )`.
pub fn rotate_about(p: Point, center: Point, angle_deg: f64) -> Point {
    let (ox, oy) = rotate_i(p.x - center.x, p.y - center.y, angle_deg);
    Point { x: ox + center.x, y: oy + center.y }
}

// ---------------------------------------------------------------------------------------------------------------------
// SEG
// ---------------------------------------------------------------------------------------------------------------------

/// `VECTOR2I::EuclideanNorm` of the vector `(dx, dy)`: rounded to an integer, exact on the diagonals.
pub fn norm_i(dx: i64, dy: i64) -> i64 {
    if dx.abs() == dy.abs() {
        return ki_round(dx.abs() as f64 * std::f64::consts::SQRT_2);
    }
    if dx == 0 {
        return dy.abs();
    }
    if dy == 0 {
        return dx.abs();
    }
    ki_round((dx as f64).hypot(dy as f64))
}

/// `SEG`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seg {
    pub a: Point,
    pub b: Point,
}

impl Seg {
    pub fn new(a: Point, b: Point) -> Seg {
        Seg { a, b }
    }

    /// `SEG::Length`.
    pub fn length(&self) -> i64 {
        norm_i(self.a.x - self.b.x, self.a.y - self.b.y)
    }

    /// `SEG::Side`: -1, 0 or 1 for the side of the line `aP` is on.
    pub fn side(&self, p: Point) -> i32 {
        let det = (self.b.x - self.a.x) as i128 * (p.y - self.a.y) as i128 - (self.b.y - self.a.y) as i128 * (p.x - self.a.x) as i128;
        if det < 0 {
            -1
        } else if det > 0 {
            1
        } else {
            0
        }
    }

    /// `SEG::LineProject`.
    pub fn line_project(&self, p: Point) -> Point {
        let (dx, dy) = ((self.b.x - self.a.x) as i128, (self.b.y - self.a.y) as i128);
        let l_squared = dx * dx + dy * dy;
        if l_squared == 0 {
            return self.a;
        }
        let t = dx * (p.x - self.a.x) as i128 + dy * (p.y - self.a.y) as i128;
        Point { x: self.a.x + rescale(t, dx, l_squared) as i64, y: self.a.y + rescale(t, dy, l_squared) as i64 }
    }

    /// `SEG::ReflectPoint`.
    pub fn reflect_point(&self, p: Point) -> Point {
        let (dx, dy) = ((self.b.x - self.a.x) as i128, (self.b.y - self.a.y) as i128);
        let l_squared = dx * dx + dy * dy;
        let t = dx * (p.x - self.a.x) as i128 + dy * (p.y - self.a.y) as i128;
        let (cx, cy) = if l_squared == 0 { (p.x as i128, p.y as i128) } else { (self.a.x as i128 + rescale(t, dx, l_squared), self.a.y as i128 + rescale(t, dy, l_squared)) };
        Point { x: (2 * cx - p.x as i128) as i64, y: (2 * cy - p.y as i128) as i64 }
    }

    /// `SEG::NearestPoint( VECTOR2I )`.
    pub fn nearest_point(&self, p: Point) -> Point {
        let (dx, dy) = ((self.b.x - self.a.x) as i128, (self.b.y - self.a.y) as i128);
        let l_squared = dx * dx + dy * dy;
        if l_squared == 0 {
            return self.a;
        }
        let t = dx * (p.x - self.a.x) as i128 + dy * (p.y - self.a.y) as i128;
        if t < 0 {
            return self.a;
        }
        if t > l_squared {
            return self.b;
        }
        Point { x: self.a.x + rescale(t, dx, l_squared) as i64, y: self.a.y + rescale(t, dy, l_squared) as i64 }
    }

    /// `SEG::TCoef`: the dot product of the segment's direction with the vector from its start to `p`.
    pub fn t_coef(&self, p: Point) -> i128 {
        (self.b.x - self.a.x) as i128 * (p.x - self.a.x) as i128 + (self.b.y - self.a.y) as i128 * (p.y - self.a.y) as i128
    }

    /// `SEG::Distance( SEG )`.
    pub fn distance_seg(&self, o: &Seg) -> i64 {
        isqrt(self.squared_distance_seg(o))
    }

    /// `SEG::SquaredDistance( VECTOR2I )`.
    pub fn squared_distance(&self, p: Point) -> i128 {
        let ab = ((self.b.x - self.a.x) as i128, (self.b.y - self.a.y) as i128);
        let ap = ((p.x - self.a.x) as i128, (p.y - self.a.y) as i128);
        let e = ap.0 * ab.0 + ap.1 * ab.1;
        if e <= 0 {
            return ap.0 * ap.0 + ap.1 * ap.1;
        }
        let f = ab.0 * ab.0 + ab.1 * ab.1;
        if e >= f {
            let bp = ((p.x - self.b.x) as i128, (p.y - self.b.y) as i128);
            return bp.0 * bp.0 + bp.1 * bp.1;
        }
        let g = (ap.0 * ap.0 + ap.1 * ap.1) as f64 - (e as f64 * e as f64) / f as f64;
        if g < 0.0 || g > i64::MAX as f64 {
            return 0;
        }
        ki_round(g) as i128
    }

    /// `SEG::Distance( VECTOR2I )`.
    pub fn distance(&self, p: Point) -> i64 {
        isqrt(self.squared_distance(p))
    }

    /// `SEG::Contains`.
    pub fn contains(&self, p: Point) -> bool {
        self.squared_distance(p) <= 3
    }

    /// `SEG::Intersects`.
    pub fn intersects(&self, o: &Seg) -> bool {
        seg_ops::intersect((self.a, self.b), (o.a, o.b), false, false).is_some()
    }

    /// `SEG::SquaredDistance( SEG )`.
    pub fn squared_distance_seg(&self, o: &Seg) -> i128 {
        if self.a == self.b {
            return o.squared_distance(self.a);
        }
        if o.a == o.b {
            return self.squared_distance(o.a);
        }
        if self.intersects(o) {
            return 0;
        }
        let d = |p: Point, q: Point| ((p.x - q.x) as i128).pow(2) + ((p.y - q.y) as i128).pow(2);
        [d(o.nearest_point(self.a), self.a), d(o.nearest_point(self.b), self.b), d(self.nearest_point(o.a), o.a), d(self.nearest_point(o.b), o.b)].into_iter().min().expect("four candidates")
    }

    fn squared_length(&self) -> i128 {
        ((self.a.x - self.b.x) as i128).pow(2) + ((self.a.y - self.b.y) as i128).pow(2)
    }

    /// `SEG::mutualDistanceSquared`.
    fn mutual_distance_squared(&self, o: &Seg) -> Option<(i128, i128)> {
        let (a, b) = if self.squared_length() < o.squared_length() { (*o, *self) } else { (*self, *o) };
        let p = (a.a.y - a.b.y) as i128;
        let q = (a.b.x - a.a.x) as i128;
        let r = -p * a.a.x as i128 - q * a.a.y as i128;
        let l = p * p + q * q;
        if l == 0 {
            return None;
        }
        let det1 = p * b.a.x as i128 + q * b.a.y as i128 + r;
        let det2 = p * b.b.x as i128 + q * b.b.y as i128 + r;
        let dsq1 = rescale(det1, det1, l);
        let dsq2 = rescale(det2, det2, l);
        Some((det1.signum() * dsq1, det2.signum() * dsq2))
    }

    /// `SEG::ApproxParallel( aSeg, aDistanceThreshold )`.
    pub fn approx_parallel(&self, o: &Seg, threshold: i64) -> bool {
        let Some((d1, d2)) = self.mutual_distance_squared(o) else { return false };
        (d1 - d2).abs() <= (threshold as i128) * (threshold as i128)
    }
}

/// `commonParallelProjection( p, n, pClip, nClip )` (`pns_diff_pair.cpp`): the stretch of `p` that `n` faces, and the stretch of `n` that
/// is that one's projection; `None` when they do not overlap.
pub fn common_parallel_projection(p: Seg, n: Seg) -> Option<(Seg, Seg)> {
    let n_proj_p = Seg::new(p.line_project(n.a), p.line_project(n.b));
    let (mut t_a, mut t_b) = (0i128, p.t_coef(p.b));
    let (mut tproj_a, mut tproj_b) = (p.t_coef(n_proj_p.a), p.t_coef(n_proj_p.b));
    if t_b < t_a {
        std::mem::swap(&mut t_b, &mut t_a);
    }
    if tproj_b < tproj_a {
        std::mem::swap(&mut tproj_b, &mut tproj_a);
    }
    if t_b <= tproj_a || t_a >= tproj_b {
        return None;
    }
    let mut tv = [0i128, p.t_coef(p.b), p.t_coef(n_proj_p.a), p.t_coef(n_proj_p.b)];
    tv.sort_unstable(); // "fixme: awful and disgusting way of finding 2 midpoints"
    let p_len_sq = (p.b.x - p.a.x) as i128 * (p.b.x - p.a.x) as i128 + (p.b.y - p.a.y) as i128 * (p.b.y - p.a.y) as i128;
    let dp = (p.b.x - p.a.x, p.b.y - p.a.y);
    let clip = |t: i128| Point { x: p.a.x + rescale(dp.0 as i128, t, p_len_sq) as i64, y: p.a.y + rescale(dp.1 as i128, t, p_len_sq) as i64 };
    let p_clip = Seg::new(clip(tv[1]), clip(tv[2]));
    let n_clip = Seg::new(n.line_project(p_clip.a), n.line_project(p_clip.b));
    Some((p_clip, n_clip))
}

// ---------------------------------------------------------------------------------------------------------------------
// CalcArcCenter
// ---------------------------------------------------------------------------------------------------------------------

/// `CalcArcCenter( aStart, aEnd, aAngle )`: the centre of the arc of `angle` degrees that goes from `start` to `end`.
pub fn calc_arc_center_by_angle(start: V2, end: V2, angle_deg: f64) -> V2 {
    let (mut angle, mut start, mut end) = (angle_deg, start, end);
    if angle < 0.0 {
        std::mem::swap(&mut start, &mut end);
        angle = -angle;
    }
    if angle > 180.0 {
        std::mem::swap(&mut start, &mut end);
        angle = 360.0 - angle;
    }
    let chord = end.sub(start).norm();
    let sin_half = sin_deg(angle / 2.0);
    let r = (chord / 2.0) / sin_half;
    let d_squared = r * r - chord * chord / 4.0;
    let mut d = 0.0;
    if d_squared > 0.0 {
        d = d_squared.sqrt();
    }
    let vec2 = end.sub(start).resize(d);
    let vc = end.sub(start).resize(chord / 2.0);
    let vec2 = rotate_d(vec2, -90.0);
    start.add(vc).add(vec2)
}

/// `CalcArcCenter( const VECTOR2D& aStart, aMid, aEnd )`: the centre of the circle through three points, with KiCad's error-propagation
/// snap to a round number.
pub fn calc_arc_center_d(start: V2, mid: V2, end: V2) -> V2 {
    // "If the three input points are clustered within a 10 IU bounding box, no meaningful circumcircle exists so return the centroid"
    const COINCIDENT_RADIUS: f64 = 5.0;
    let (min_x, max_x) = (start.x.min(mid.x).min(end.x), start.x.max(mid.x).max(end.x));
    let (min_y, max_y) = (start.y.min(mid.y).min(end.y), start.y.max(mid.y).max(end.y));
    if max_x - min_x < COINCIDENT_RADIUS && max_y - min_y < COINCIDENT_RADIUS {
        return V2 { x: (start.x + mid.x + end.x) / 3.0, y: (start.y + mid.y + end.y) / 3.0 };
    }

    let y_delta_21 = mid.y - start.y;
    let mut x_delta_21 = mid.x - start.x;
    let y_delta_32 = end.y - mid.y;
    let mut x_delta_32 = end.x - mid.x;

    // "a special case for aMid as the half-way point when aSlope = 0 and bSlope = inf or the other way around"
    if (x_delta_21 == 0.0 && y_delta_32 == 0.0) || (y_delta_21 == 0.0 && x_delta_32 == 0.0) {
        return V2 { x: (start.x + end.x) / 2.0, y: (start.y + end.y) / 2.0 };
    }

    // "Prevent div-by-0 errors"
    if x_delta_21 == 0.0 {
        x_delta_21 = f64::EPSILON;
    }
    if x_delta_32 == 0.0 {
        x_delta_32 = -f64::EPSILON;
    }

    let mut a_slope = y_delta_21 / x_delta_21;
    let mut b_slope = y_delta_32 / x_delta_32;

    let da_slope = a_slope * V2 { x: 0.5 / y_delta_21, y: 0.5 / x_delta_21 }.norm();
    let db_slope = b_slope * V2 { x: 0.5 / y_delta_32, y: 0.5 / x_delta_32 }.norm();

    if a_slope == b_slope {
        if start == end {
            // "a 360 degrees arc. In this case, the center is halfway between the midpoint and either end point."
            return V2 { x: (start.x + mid.x) / 2.0, y: (start.y + mid.y) / 2.0 };
        }
        // "If the points are colinear, the center is at infinity, so offset the slope by a minimal amount"
        a_slope += f64::EPSILON;
        b_slope -= f64::EPSILON;
    }

    // "a small value is used. std::numeric_limits<double>::epsilon() is too small and generate false results"
    if a_slope == 0.0 {
        a_slope = 1e-10;
    }
    if b_slope == 0.0 {
        b_slope = 1e-10;
    }

    // The expressions below are the C++ ones with their evaluation order (`a / b * c / d` is `((a / b) * c) / d`).
    let m = std::f64::consts::FRAC_1_SQRT_2;
    let ab_slope_start_end_y = a_slope * b_slope * (start.y - end.y);
    let dab_slope_start_end_y = ab_slope_start_end_y * (da_slope / a_slope * da_slope / a_slope + db_slope / b_slope * db_slope / b_slope + m / (start.y - end.y) * m / (start.y - end.y)).sqrt();

    let b_slope_start_mid_x = b_slope * (start.x + mid.x);
    let db_slope_start_mid_x = b_slope_start_mid_x * (db_slope / b_slope * db_slope / b_slope + m / (start.x + mid.x) * m / (start.x + mid.x)).sqrt();

    let a_slope_mid_end_x = a_slope * (mid.x + end.x);
    let da_slope_mid_end_x = a_slope_mid_end_x * (da_slope / a_slope * da_slope / a_slope + m / (mid.x + end.x) * m / (mid.x + end.x)).sqrt();

    let twice_ba_slope_diff = 2.0 * (b_slope - a_slope);
    let dtwice_ba_slope_diff = 2.0 * (db_slope * db_slope + da_slope * da_slope).sqrt();

    let center_numerator_x = ab_slope_start_end_y + b_slope_start_mid_x - a_slope_mid_end_x;
    let d_center_numerator_x = (dab_slope_start_end_y * dab_slope_start_end_y + db_slope_start_mid_x * db_slope_start_mid_x + da_slope_mid_end_x * da_slope_mid_end_x).sqrt();

    let center_x = (ab_slope_start_end_y + b_slope_start_mid_x - a_slope_mid_end_x) / twice_ba_slope_diff;
    let d_center_x = center_x * (d_center_numerator_x / center_numerator_x * d_center_numerator_x / center_numerator_x + dtwice_ba_slope_diff / twice_ba_slope_diff * dtwice_ba_slope_diff / twice_ba_slope_diff).sqrt();

    let center_numerator_y = (start.x + mid.x) / 2.0 - center_x;
    let d_center_numerator_y = (1.0 / 8.0 + d_center_x * d_center_x).sqrt();

    let center_first_term = center_numerator_y / a_slope;
    let dcenter_first_term_y = center_first_term * (d_center_numerator_y / center_numerator_y * d_center_numerator_y / center_numerator_y + da_slope / a_slope * da_slope / a_slope).sqrt();

    let center_y = center_first_term + (start.y + mid.y) / 2.0;
    let d_center_y = (dcenter_first_term_y * dcenter_first_term_y + 1.0 / 8.0).sqrt();

    let rounded100_x = ((center_x + 50.0) / 100.0).floor() * 100.0;
    let rounded100_y = ((center_y + 50.0) / 100.0).floor() * 100.0;
    let rounded10_x = ((center_x + 5.0) / 10.0).floor() * 10.0;
    let rounded10_y = ((center_y + 5.0) / 10.0).floor() * 10.0;

    // "see if they are within our uncertainty range. If they are, then we use this round value as the true value"
    if (rounded100_x - center_x).abs() < d_center_x && (rounded100_y - center_y).abs() < d_center_y {
        V2 { x: rounded100_x, y: rounded100_y }
    } else if (rounded10_x - center_x).abs() < d_center_x && (rounded10_y - center_y).abs() < d_center_y {
        V2 { x: rounded10_x, y: rounded10_y }
    } else {
        V2 { x: center_x, y: center_y }
    }
}

/// `CalcArcCenter( const VECTOR2I&, .. )`: the same, rounded to the integer grid.
pub fn calc_arc_center(start: Point, mid: Point, end: Point) -> Point {
    let c = calc_arc_center_d(V2::of(start), V2::of(mid), V2::of(end));
    let (lo, hi) = (i32::MIN as f64 + 100.0, i32::MAX as f64 - 100.0);
    Point { x: ki_round(c.x.clamp(lo, hi)), y: ki_round(c.y.clamp(lo, hi)) }
}

// ---------------------------------------------------------------------------------------------------------------------
// SHAPE_ARC
// ---------------------------------------------------------------------------------------------------------------------

/// `getArcPolygonizationMaxError()`: `SHAPE_ARC::DefaultAccuracyForPCB() / 5`, with `ARC_HIGH_DEF` 0.005 mm.
pub const ARC_POLYGONIZATION_MAX_ERROR_NM: i64 = 1_000;

/// `GetArcToSegmentCount( aRadius, aErrorMax, aArcAngle )`.
pub fn arc_to_segment_count(radius: i64, error_max: i64, arc_angle_deg: f64) -> i64 {
    const MIN_SEGCOUNT_FOR_CIRCLE: f64 = 8.0;
    let radius = radius.max(1);
    let error_max = error_max.max(1);
    let rel_error = error_max as f64 / radius as f64;
    let mut arc_increment = 180.0 / std::f64::consts::PI * (1.0 - rel_error).acos() * 2.0;
    arc_increment = arc_increment.min(360.0 / MIN_SEGCOUNT_FOR_CIRCLE);
    let seg_count = ki_round(arc_angle_deg.abs() / arc_increment);
    seg_count.max(2)
}

/// `CircleToEndSegmentDeltaRadius( aRadius, aSegCount )`.
pub fn circle_to_end_segment_delta_radius(radius: i64, seg_count: i64) -> i64 {
    let seg_count = if seg_count <= 2 { 3 } else { seg_count };
    let alpha = std::f64::consts::PI / seg_count as f64;
    ki_round((radius as f64 * (1.0 - 1.0 / alpha.cos())).abs())
}

/// `SHAPE_ARC`: the arc from `start` through `mid` to `end`.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeArc {
    pub start: Point,
    pub mid: Point,
    pub end: Point,
    pub width: i64,
    center: Point,
    radius: f64,
}

impl ShapeArc {
    /// `SHAPE_ARC( aArcStart, aArcMid, aArcEnd, aWidth )`.
    pub fn new(start: Point, mid: Point, end: Point, width: i64) -> ShapeArc {
        let mut a = ShapeArc { start, mid, end, width, center: Point::default(), radius: 0.0 };
        a.update_values();
        a
    }

    /// `SHAPE_ARC::ConstructFromStartEndAngle( aStart, aEnd, aAngle, aWidth )`.
    pub fn from_start_end_angle(start: Point, end: Point, angle_deg: f64, width: i64) -> ShapeArc {
        let center = calc_arc_center_by_angle(V2::of(start), V2::of(end), angle_deg).trunc();
        let mid = rotate_about(start, center, -angle_deg / 2.0);
        ShapeArc::new(start, mid, end, width)
    }

    /// `SHAPE_ARC::ConstructFromStartEndCenter`.
    pub fn from_start_end_center(start: Point, end: Point, center: Point, clockwise: bool) -> ShapeArc {
        let start_line = V2::of(Point { x: start.x - center.x, y: start.y - center.y });
        let end_line = V2::of(Point { x: end.x - center.x, y: end.y - center.y });
        let start_angle = normalize(angle_of(start_line.x, start_line.y));
        let end_angle = normalize(angle_of(end_line.x, end_line.y));
        let mut angle = end_angle - start_angle;
        angle = if clockwise { normalize(angle) - 360.0 } else { normalize(angle) };
        let mid = rotate_about(start, center, -angle / 2.0);
        ShapeArc::new(start, mid, end, 0)
    }

    fn update_values(&mut self) {
        self.center = calc_arc_center(self.start, self.mid, self.end);
        let (dx, dy) = ((self.start.x - self.center.x) as f64, (self.start.y - self.center.y) as f64);
        self.radius = (dx * dx + dy * dy).sqrt();
    }

    pub fn center(&self) -> Point {
        self.center
    }

    pub fn radius(&self) -> f64 {
        self.radius
    }

    /// `SHAPE_ARC::IsCCW`.
    pub fn is_ccw(&self) -> bool {
        let v1 = ((self.end.x - self.mid.x) as i128, (self.end.y - self.mid.y) as i128);
        let v2 = ((self.start.x - self.mid.x) as i128, (self.start.y - self.mid.y) as i128);
        v1.0 * v2.1 - v1.1 * v2.0 > 0
    }

    pub fn is_clockwise(&self) -> bool {
        !self.is_ccw()
    }

    /// `SHAPE_ARC::GetStartAngle`, degrees in `[0, 360)`.
    pub fn start_angle(&self) -> f64 {
        normalize(angle_of((self.start.x - self.center.x) as f64, (self.start.y - self.center.y) as f64))
    }

    /// `SHAPE_ARC::GetEndAngle`.
    pub fn end_angle(&self) -> f64 {
        normalize(angle_of((self.end.x - self.center.x) as f64, (self.end.y - self.center.y) as f64))
    }

    /// `SHAPE_ARC::GetCentralAngle`, degrees: positive counter-clockwise, 360 for a circle.
    pub fn central_angle(&self) -> f64 {
        if self.start == self.end {
            return 360.0;
        }
        let a_end = angle_of((self.end.x - self.center.x) as f64, (self.end.y - self.center.y) as f64);
        let a_start = angle_of((self.start.x - self.center.x) as f64, (self.start.y - self.center.y) as f64);
        let mut angle = a_end - a_start;
        if self.is_ccw() {
            if angle < 0.0 {
                angle += 360.0;
            }
        } else if angle > 0.0 {
            angle -= 360.0;
        }
        angle
    }

    /// `SHAPE_ARC::GetLength`.
    pub fn length(&self) -> f64 {
        (self.radius * (self.central_angle() * DEGREES_TO_RADIANS)).abs()
    }

    /// `SHAPE_ARC::ConvertToPolyline( aMaxError )`: the first and last points are the arc's own, the ones between sit at the middle of each slice,
    /// a little outside the arc.
    pub fn convert_to_polyline(&self, max_error: i64) -> Vec<Point> {
        let r = self.radius;
        let sa = self.start_angle();
        let c = self.center;
        let ca = self.central_angle();
        let start_to_end = Seg::new(self.start, self.end);
        let half_max_error = 1.0f64.max(max_error as f64 / 2.0);
        let external_radius = r + (self.width as f64 / 2.0);
        let n: i64;
        let effective_error: f64;
        if external_radius < half_max_error || (start_to_end.distance(self.mid) as f64) < half_max_error {
            // "a very rare case: the arc is approximated by one segment"
            n = 0;
            effective_error = external_radius;
        } else {
            n = arc_to_segment_count(external_radius as i64, max_error, ca);
            let seg360 = (n as f64 * 360.0 / ca.abs()) as i64;
            effective_error = circle_to_end_segment_delta_radius(external_radius as i64, seg360) as f64;
        }
        let r = r + effective_error / 2.0;
        let n = n * 2;
        let mut out = vec![self.start];
        let mut i = 1;
        while i < n {
            let mut a = sa;
            if n != 0 {
                a += (ca * i as f64) / n as f64;
            }
            let x = c.x as f64 + r * cos_deg(a);
            let y = c.y as f64 + r * sin_deg(a);
            let p = Point { x: ki_round(x), y: ki_round(y) };
            if out.last() != Some(&p) {
                out.push(p);
            }
            i += 2;
        }
        if out.last() != Some(&self.end) {
            out.push(self.end);
        }
        out
    }

    /// `SHAPE_ARC::Mirror( const SEG& axis )`.
    pub fn mirror(&mut self, axis: &Seg) {
        self.start = axis.reflect_point(self.start);
        self.end = axis.reflect_point(self.end);
        self.mid = axis.reflect_point(self.mid);
        self.update_values();
    }

    /// `SHAPE_ARC::Reverse`: swaps the ends and leaves the middle (and the centre) alone.
    pub fn reverse(&mut self) {
        std::mem::swap(&mut self.start, &mut self.end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i64, y: i64) -> Point {
        Point { x, y }
    }

    #[test]
    fn micrometres_cross_the_border_and_back() {
        assert_eq!(to_nm(p(3, -4)), p(3_000, -4_000));
        assert_eq!(to_um(p(3_499, -3_500)), p(3, -4), "halves go away from zero");
        assert_eq!(to_um(to_nm(p(-17, 25))), p(-17, 25));
    }

    #[test]
    fn resize_is_kicads_on_the_diagonals_and_off_them() {
        assert_eq!(V2::new(3.0, 4.0).resize(10.0), V2::new(6.0, 8.0));
        assert_eq!(V2::new(-3.0, 4.0).resize(10.0), V2::new(-6.0, 8.0));
        let d = V2::new(1.0, 1.0).resize(2.0f64.sqrt());
        assert!((d.x - 1.0).abs() < 1e-12 && (d.y - 1.0).abs() < 1e-12);
        assert_eq!(V2::new(5.0, 0.0).resize(-2.0), V2::new(-2.0, 0.0), "a negative length turns the vector round");
        assert_eq!(V2::new(0.0, 0.0).resize(7.0), V2::new(0.0, 0.0));
    }

    #[test]
    fn a_quarter_turn_is_exact_and_the_others_round() {
        assert_eq!(rotate_d(V2::new(10.0, 0.0), 90.0), V2::new(0.0, -10.0));
        assert_eq!(rotate_d(V2::new(10.0, 0.0), -90.0), V2::new(-0.0, 10.0));
        assert_eq!(rotate_about(p(100, 0), p(0, 0), 45.0), p(71, -71));
        assert_eq!(rotate_about(p(110, 10), p(10, 10), 90.0), p(10, -90));
    }

    #[test]
    fn segment_queries_round_as_kicad_does() {
        let s = Seg::new(p(0, 0), p(10, 0));
        assert_eq!(s.side(p(5, 3)), 1);
        assert_eq!(s.side(p(5, -3)), -1);
        assert_eq!(s.line_project(p(4, 7)), p(4, 0));
        assert_eq!(s.reflect_point(p(4, 7)), p(4, -7));
        assert_eq!(s.nearest_point(p(-5, 2)), p(0, 0));
        assert_eq!(s.distance(p(12, 0)), 2);
        assert!(s.contains(p(5, 1)) && !s.contains(p(5, 2)));
        assert_eq!(Seg::new(p(0, 0), p(3, 4)).length(), 5);
        assert_eq!(Seg::new(p(0, 0), p(10, 10)).length(), 14, "the diagonal is rounded once");
        assert!(Seg::new(p(0, 0), p(10, 0)).approx_parallel(&Seg::new(p(0, 5), p(10, 5)), 1));
        assert!(!Seg::new(p(0, 0), p(10, 0)).approx_parallel(&Seg::new(p(0, 0), p(10, 10)), 1));
        assert_eq!(Seg::new(p(0, 0), p(10, 0)).squared_distance_seg(&Seg::new(p(3, 4), p(3, 9))), 16);
    }

    #[test]
    fn a_ninety_degree_arc_is_built_the_way_the_meander_builds_it() {
        // `makeMiterShape`: a quarter arc of angle -90 from (0,0) to (1 mm, 1 mm) has its centre at (1 mm, 0), +90 at (0, 1 mm).
        let a = ShapeArc::from_start_end_angle(p(0, 0), p(1_000_000, 1_000_000), -90.0, 0);
        assert_eq!(a.center(), p(1_000_000, 0));
        assert!((a.radius() - 1_000_000.0).abs() < 1e-3);
        assert!(!a.is_ccw() && (a.central_angle() + 90.0).abs() < 1e-6, "{}", a.central_angle());
        let b = ShapeArc::from_start_end_angle(p(0, 0), p(1_000_000, 1_000_000), 90.0, 0);
        assert_eq!(b.center(), p(0, 1_000_000));
        assert!(b.is_ccw() && (b.central_angle() - 90.0).abs() < 1e-6, "{}", b.central_angle());
        assert!((a.length() - 1_000_000.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-3);
        // the middle point is the start turned half way
        assert_eq!(a.mid, p(292_893, 707_107), "{:?}", a.mid);
    }

    #[test]
    fn an_arc_polygonises_with_its_own_ends_and_the_segments_in_between() {
        let a = ShapeArc::from_start_end_angle(p(0, 0), p(1_000_000, 1_000_000), -90.0, 0);
        let pl = a.convert_to_polyline(ARC_POLYGONIZATION_MAX_ERROR_NM);
        assert_eq!((pl.first(), pl.last()), (Some(&p(0, 0)), Some(&p(1_000_000, 1_000_000))));
        assert_eq!(pl.len(), 20, "18 slices: the two ends and a point in the middle of each slice");
        // every chord vertex is within a micrometre of the circle
        let c = a.center();
        for q in &pl[1..pl.len() - 1] {
            let d = (((q.x - c.x) as f64).powi(2) + ((q.y - c.y) as f64).powi(2)).sqrt();
            assert!((d - a.radius()).abs() <= 1000.0, "{d}");
        }
    }

    #[test]
    fn the_centre_of_three_points_is_snapped_to_a_round_number_inside_its_error() {
        // three points of the circle of radius 5 mm round (2 mm, 3 mm): the centre is exactly that, in nanometres
        let c = calc_arc_center(p(7_000_000, 3_000_000), p(2_000_000, 8_000_000), p(-3_000_000, 3_000_000));
        assert_eq!(c, p(2_000_000, 3_000_000));
    }
}
