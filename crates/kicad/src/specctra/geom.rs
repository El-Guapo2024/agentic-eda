//! The pieces of KiCad's geometry library that `SPECCTRA_DB::FromBOARD` calls, ported in KiCad's own integer arithmetic (nanometres, `int` rounded with `KiROUND`):
//! `RotatePoint` and `EDA_ANGLE` (`libs/kimath/src/trigo.cpp`, `geometry/eda_angle.h`), `VECTOR2I::Resize`, `SEG::ReflectPoint`, `GetArcToSegmentCount`,
//! `ConvertArcToPolyline` (about a centre, and through three points with `ARC_CHORD_PARAMS`), `TransformArcToPolygon`, `CornerListToPolygon` /
//! `TransformRoundChamferedRectToPolygon` (`convert_basic_shapes_to_polygon.cpp`) and `BuildConvexHull` (`geometry/convex_hull.cpp`); `TransformCircleToPolygon` and
//! `TransformOvalToPolygon` are `eda_shape_poly_set::basic_shapes`'.
//!
//! The coordinates are `i64` so a product of two never overflows; KiCad's are `int`.

use std::f64::consts::{FRAC_1_SQRT_2, PI};

/// `VECTOR2I`, nanometres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord)]
pub struct V {
    pub x: i64,
    pub y: i64,
}

impl V {
    pub const fn new(x: i64, y: i64) -> V {
        V { x, y }
    }

    /// `VECTOR2::Perpendicular`.
    pub fn perpendicular(self) -> V {
        V::new(-self.y, self.x)
    }

    /// `VECTOR2::EuclideanNorm` for an integer vector.
    pub fn euclidean_norm(self) -> i64 {
        if self.x.abs() == self.y.abs() {
            return kiround(self.x.abs() as f64 * std::f64::consts::SQRT_2);
        }
        if self.x == 0 {
            return self.y.abs();
        }
        if self.y == 0 {
            return self.x.abs();
        }
        kiround((self.x as f64).hypot(self.y as f64))
    }

    /// `VECTOR2::Resize`: the same direction, `new_len` long.
    pub fn resize(self, new_len: i64) -> V {
        if self.x == 0 && self.y == 0 {
            return V::new(0, 0);
        }
        let (nx, ny);
        if self.x.abs() == self.y.abs() {
            let v = new_len.abs() as f64 * FRAC_1_SQRT_2;
            nx = v;
            ny = v;
        } else {
            let x_sq = self.x as i128 * self.x as i128;
            let y_sq = self.y as i128 * self.y as i128;
            let l_sq = x_sq + y_sq;
            let new_sq = new_len as i128 * new_len as i128;
            nx = (rescale128(new_sq, x_sq, l_sq) as f64).sqrt();
            ny = (rescale128(new_sq, y_sq, l_sq) as f64).sqrt();
        }
        let sign = new_len.signum();
        V::new((if self.x < 0 { -kiround(nx) } else { kiround(nx) }) * sign, (if self.y < 0 { -kiround(ny) } else { kiround(ny) }) * sign)
    }
}

impl std::ops::Add for V {
    type Output = V;
    fn add(self, o: V) -> V {
        V::new(self.x + o.x, self.y + o.y)
    }
}

impl std::ops::Sub for V {
    type Output = V;
    fn sub(self, o: V) -> V {
        V::new(self.x - o.x, self.y - o.y)
    }
}

impl std::ops::Neg for V {
    type Output = V;
    fn neg(self) -> V {
        V::new(-self.x, -self.y)
    }
}

/// `KiROUND`: `llround`, half away from zero.
pub fn kiround(v: f64) -> i64 {
    v.round() as i64
}

/// `rescale( numerator, value, denominator )` for 64 bit integers: `numerator * value / denominator`, rounded to nearest.
fn rescale128(numerator: i128, value: i128, denominator: i128) -> i128 {
    if denominator == 0 {
        return 0;
    }
    let n = numerator * value;
    if (n < 0) ^ (denominator < 0) {
        (n - denominator / 2) / denominator
    } else {
        (n + denominator / 2) / denominator
    }
}

/// `SEG::ReflectPoint`: `p` mirrored in the line through `a` and `b`.
pub fn reflect_point(a: V, b: V, p: V) -> V {
    let d = b - a;
    let l_squared = d.x as i128 * d.x as i128 + d.y as i128 * d.y as i128;
    let t = d.x as i128 * (p.x - a.x) as i128 + d.y as i128 * (p.y - a.y) as i128;
    let (cx, cy) = if l_squared == 0 { (p.x as i128, p.y as i128) } else { (a.x as i128 + rescale128(t, d.x as i128, l_squared), a.y as i128 + rescale128(t, d.y as i128, l_squared)) };
    V::new((2 * cx - p.x as i128) as i64, (2 * cy - p.y as i128) as i64)
}

/// `EDA_ANGLE`: degrees as a double.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Angle(pub f64);

impl Angle {
    pub const ZERO: Angle = Angle(0.0);

    pub fn normalize(self) -> Angle {
        let mut v = self.0;
        while v < -0.0 {
            v += 360.0;
        }
        while v >= 360.0 {
            v -= 360.0;
        }
        Angle(v)
    }

    pub fn normalize180(self) -> Angle {
        let mut v = self.0;
        while v <= -180.0 {
            v += 360.0;
        }
        while v > 180.0 {
            v -= 360.0;
        }
        Angle(v)
    }

    pub fn radians(self) -> f64 {
        self.0 * (PI / 180.0)
    }

    pub fn sin(self) -> f64 {
        let t = self.normalize().0;
        if t == 0.0 || t == 180.0 {
            0.0
        } else if t == 45.0 || t == 135.0 {
            FRAC_1_SQRT_2
        } else if t == 225.0 || t == 315.0 {
            -FRAC_1_SQRT_2
        } else if t == 90.0 {
            1.0
        } else if t == 270.0 {
            -1.0
        } else {
            self.radians().sin()
        }
    }

    pub fn cos(self) -> f64 {
        let t = self.normalize().0;
        if t == 0.0 {
            1.0
        } else if t == 180.0 {
            -1.0
        } else if t == 90.0 || t == 270.0 {
            0.0
        } else if t == 45.0 || t == 315.0 {
            FRAC_1_SQRT_2
        } else if t == 135.0 || t == 225.0 {
            -FRAC_1_SQRT_2
        } else {
            self.radians().cos()
        }
    }
}

impl std::ops::Neg for Angle {
    type Output = Angle;
    fn neg(self) -> Angle {
        Angle(-self.0)
    }
}

impl std::ops::Sub for Angle {
    type Output = Angle;
    fn sub(self, o: Angle) -> Angle {
        Angle(self.0 - o.0)
    }
}

impl std::ops::Add for Angle {
    type Output = Angle;
    fn add(self, o: Angle) -> Angle {
        Angle(self.0 + o.0)
    }
}

/// `RotatePoint( VECTOR2I& pt, const EDA_ANGLE& angle )`: a point turned about the origin, counter-clockwise on a screen whose y points down.
pub fn rotate_point(pt: V, angle: Angle) -> V {
    let a = angle.normalize();
    if a.0 == 0.0 {
        pt
    } else if a.0 == 90.0 {
        V::new(pt.y, -pt.x)
    } else if a.0 == 180.0 {
        V::new(-pt.x, -pt.y)
    } else if a.0 == 270.0 {
        V::new(-pt.y, pt.x)
    } else {
        let (s, c) = (a.sin(), a.cos());
        V::new(kiround(pt.y as f64 * s + pt.x as f64 * c), kiround(pt.y as f64 * c - pt.x as f64 * s))
    }
}

/// `RotatePoint( pt, centre, angle )`.
pub fn rotate_point_about(pt: V, centre: V, angle: Angle) -> V {
    rotate_point(pt - centre, angle) + centre
}

/// `MIRROR( y, ref )`: `y` mirrored in the horizontal line through `ref`.
pub fn mirror_y(y: i64, reference: i64) -> i64 {
    -(y - reference) + reference
}

/// `GetArcToSegmentCount( aRadius, aErrorMax, aArcAngle )`: how many segments approximate an arc of `arc_degrees` within `error_max`.
pub fn arc_to_segment_count(radius: i64, error_max: i64, arc_degrees: f64) -> i64 {
    const MIN_SEGCOUNT_FOR_CIRCLE: f64 = 8.0;
    let radius = radius.max(1) as f64;
    let error_max = error_max.max(1) as f64;
    let rel_error = error_max / radius;
    let mut arc_increment = 180.0 / PI * (1.0 - rel_error).acos() * 2.0;
    arc_increment = arc_increment.min(360.0 / MIN_SEGCOUNT_FOR_CIRCLE);
    let seg_count = kiround(arc_degrees.abs() / arc_increment);
    seg_count.max(2)
}

/// `ConvertArcToPolyline( polyline, centre, radius, start, arc, accuracy, ERROR_INSIDE )`: the points of an arc, the first at `start` and the last at `start + arc`.
pub fn arc_to_polyline_inside(center: V, radius: i64, start: Angle, arc: Angle, accuracy: i64) -> Vec<V> {
    let n = if radius >= accuracy { arc_to_segment_count(radius, accuracy, arc.0) + 1 } else { 2 };
    let delta = Angle(arc.0 / n as f64);
    let mut out = Vec::with_capacity(n as usize + 1);
    let mut rot = start;
    for _ in 0..=n {
        let x = center.x as f64 + radius as f64 * rot.cos();
        let y = center.y as f64 + radius as f64 * rot.sin();
        out.push(V::new(kiround(x), kiround(y)));
        rot = Angle(rot.0 + delta.0);
    }
    out
}

pub use eda_shape_poly_set::basic_shapes::ErrorLoc;

fn opposite(loc: ErrorLoc) -> ErrorLoc {
    match loc {
        ErrorLoc::Inside => ErrorLoc::Outside,
        ErrorLoc::Outside => ErrorLoc::Inside,
    }
}

/// `SHAPE_LINE_CHAIN::Append( aP )`: a point equal to the last one is not added.
fn append(chain: &mut Vec<V>, p: V) {
    if chain.last() != Some(&p) {
        chain.push(p);
    }
}

/// `EDA_ANGLE( const VECTOR2D& aVector )`: the direction of a vector, the axes and the diagonals exact.
pub fn angle_of(x: f64, y: f64) -> Angle {
    if x == 0.0 && y == 0.0 {
        Angle(0.0)
    } else if y == 0.0 {
        Angle(if x >= 0.0 { 0.0 } else { -180.0 })
    } else if x == 0.0 {
        Angle(if y >= 0.0 { 90.0 } else { -90.0 })
    } else if x == y {
        Angle(if x >= 0.0 { 45.0 } else { -180.0 + 45.0 })
    } else if x == -y {
        Angle(if x >= 0.0 { -45.0 } else { 180.0 - 45.0 })
    } else {
        Angle(y.atan2(x) / (PI / 180.0))
    }
}

/// `CircleToEndSegmentDeltaRadius( aRadius, aSegCount )`: how far outside the circle the ends of the segments of a `seg_count`-gon tangent to it are.
pub fn circle_to_end_segment_delta_radius(radius: i64, seg_count: i64) -> i64 {
    let seg_count = if seg_count <= 2 { 3 } else { seg_count };
    let alpha = PI / seg_count as f64;
    kiround((radius as f64 * (1.0 - 1.0 / alpha.cos())).abs())
}

/// `ARC_CHORD_PARAMS` (`libs/kimath/src/geometry/arc_chord_params.cpp`): an arc through three points told by its chord, the chord's unit vectors, its sagitta and radius.
struct ArcChord {
    half_chord: f64,
    ux: f64,
    uy: f64,
    nx: f64,
    ny: f64,
    sagitta: f64,
    radius: f64,
    center_offset: f64,
    midx: f64,
    midy: f64,
}

impl ArcChord {
    /// `ARC_CHORD_PARAMS::Compute`: `None` for a degenerate arc (no chord, or the middle on it).
    fn compute(start: V, mid: V, end: V) -> Option<ArcChord> {
        let dx = end.x as f64 - start.x as f64;
        let dy = end.y as f64 - start.y as f64;
        let chord_len = (dx * dx + dy * dy).sqrt();
        if chord_len <= 0.0 {
            return None;
        }
        let mx = mid.x as f64 - start.x as f64;
        let my = mid.y as f64 - start.y as f64;
        let cross = mx * dy - my * dx;
        if cross == 0.0 {
            return None;
        }
        let sagitta = cross.abs() / chord_len;
        if sagitta <= 0.0 {
            return None;
        }
        let half_chord = chord_len / 2.0;
        let radius = (half_chord * half_chord + sagitta * sagitta) / (2.0 * sagitta);
        if radius <= 0.0 {
            return None;
        }
        let (ux, uy) = (dx / chord_len, dy / chord_len);
        let (mut nx, mut ny) = (-uy, ux);
        if cross < 0.0 {
            nx = -nx;
            ny = -ny;
        }
        Some(ArcChord {
            half_chord,
            ux,
            uy,
            nx,
            ny,
            sagitta,
            radius,
            center_offset: radius - sagitta,
            midx: (start.x as f64 + end.x as f64) * 0.5,
            midy: (start.y as f64 + end.y as f64) * 0.5,
        })
    }

    /// `GetStartAngle`: the direction from the centre to the start.
    fn start_angle(&self) -> Angle {
        let sin_half = self.half_chord / self.radius;
        let cos_half = self.center_offset / self.radius;
        angle_of(-sin_half * self.ux - cos_half * self.nx, -sin_half * self.uy - cos_half * self.ny)
    }

    /// `GetEndAngle`.
    fn end_angle(&self) -> Angle {
        let sin_half = self.half_chord / self.radius;
        let cos_half = self.center_offset / self.radius;
        angle_of(sin_half * self.ux - cos_half * self.nx, sin_half * self.uy - cos_half * self.ny)
    }

    /// `GetArcAngle`, radians.
    fn arc_angle(&self) -> f64 {
        let ratio = (self.half_chord / self.radius).clamp(0.0, 1.0);
        let base_angle = 2.0 * ratio.asin();
        if self.sagitta > self.radius {
            2.0 * PI - base_angle
        } else {
            base_angle
        }
    }
}

/// `ConvertArcToPolyline( aPolyline, aCenter, aRadius, aStartAngle, aArcAngle, aAccuracy, aErrorLoc )`: the points of an arc about a centre appended to `chain`.
pub fn arc_to_polyline_about(chain: &mut Vec<V>, center: V, radius: i64, start: Angle, arc: Angle, accuracy: f64, loc: ErrorLoc) -> i64 {
    let mut n = 2;
    if radius as f64 >= accuracy {
        n = arc_to_segment_count(radius, accuracy as i64, arc.0) + 1;
    }
    let delta = Angle(arc.0 / n as f64);
    let at = |r: f64, rot: Angle| V::new(kiround(center.x as f64 + r * rot.cos()), kiround(center.y as f64 + r * rot.sin()));
    match loc {
        ErrorLoc::Inside => {
            // With the error on the inside the end points of each segment are error-free.
            let mut rot = start;
            for _ in 0..=n {
                append(chain, at(radius as f64, rot));
                rot = rot + delta;
            }
        }
        ErrorLoc::Outside => {
            // With the error on the outside it is the middles of the segments that are error-free: a half segment at each end of the arc.
            let seg360 = kiround(n as f64 * 360.0 / arc.0).abs();
            let error_radius = radius + circle_to_end_segment_delta_radius(radius, seg360);
            append(chain, at(radius as f64, start));
            let mut rot = start + Angle(delta.0 / 2.0);
            for _ in 0..n {
                append(chain, at(error_radius as f64, rot));
                rot = rot + delta;
            }
            append(chain, at(radius as f64, start + arc));
        }
    }
    n
}

/// `ConvertArcToPolyline( aPolyline, aStart, aMid, aEnd, aAccuracy, aErrorLoc, aRadialOffset )`: the points of the arc through three points, `radial_offset` off it,
/// appended to `chain`.
pub fn arc_to_polyline_through(chain: &mut Vec<V>, start: V, mid: V, end: V, accuracy: f64, loc: ErrorLoc, radial_offset: f64) -> i64 {
    let Some(params) = ArcChord::compute(start, mid, end) else {
        append(chain, start);
        if end != start {
            append(chain, end);
        }
        return 0;
    };
    let arc_angle = params.arc_angle();
    if arc_angle <= 0.0 {
        append(chain, start);
        append(chain, end);
        return 0;
    }
    let max_coord = i32::MAX as f64;
    let point = |alpha: f64, effective_radius: f64| -> Option<V> {
        let u_offset = effective_radius * alpha.sin();
        let n_offset = params.center_offset - effective_radius * alpha.cos();
        let x = params.midx + params.ux * u_offset + params.nx * n_offset;
        let y = params.midy + params.uy * u_offset + params.ny * n_offset;
        (x.is_finite() && y.is_finite() && x.abs() <= max_coord && y.abs() <= max_coord).then(|| V::new(kiround(x), kiround(y)))
    };
    let effective_radius = params.radius + radial_offset;
    let arc_angle_eda = Angle(arc_angle / (PI / 180.0));
    let mut n = 2;
    let radius_for_seg = kiround(effective_radius.abs().min(max_coord));
    if radius_for_seg as f64 >= accuracy {
        n = arc_to_segment_count(radius_for_seg, accuracy as i64, arc_angle_eda.0) + 1;
    }
    let half_angle = arc_angle / 2.0;
    let delta = arc_angle / n as f64;
    let mut alphas: Vec<(f64, f64)> = Vec::new();
    let seg360 = kiround(n as f64 * 360.0 / arc_angle_eda.0).abs();
    if loc == ErrorLoc::Inside || seg360 <= 0 {
        alphas.extend((0..=n).map(|i| (-half_angle + delta * i as f64, effective_radius)));
    } else {
        let error_radius = effective_radius + circle_to_end_segment_delta_radius(radius_for_seg, seg360) as f64;
        alphas.push((-half_angle, effective_radius));
        alphas.extend((0..n).map(|i| (-half_angle + delta * (i as f64 + 0.5), error_radius)));
        alphas.push((half_angle, effective_radius));
    }
    for (alpha, r) in alphas {
        match point(alpha, r) {
            Some(p) => append(chain, p),
            None => {
                // An overflow falls back to a straight segment.
                chain.clear();
                append(chain, start);
                append(chain, end);
                return 0;
            }
        }
    }
    n
}

/// `SEG( a, b ).Distance( p )`: `isqrt( SquaredDistance( p ) )`.
fn seg_distance(a: V, b: V, p: V) -> i64 {
    let (abx, aby) = ((b.x - a.x) as i128, (b.y - a.y) as i128);
    let (apx, apy) = ((p.x - a.x) as i128, (p.y - a.y) as i128);
    let e = apx * abx + apy * aby;
    let sq: i128 = if e <= 0 {
        apx * apx + apy * apy
    } else {
        let f = abx * abx + aby * aby;
        if e >= f {
            let (bpx, bpy) = ((p.x - b.x) as i128, (p.y - b.y) as i128);
            bpx * bpx + bpy * bpy
        } else {
            let g = (apx * apx + apy * apy) as f64 - (e as f64 * e as f64) / f as f64;
            if g < 0.0 {
                0
            } else {
                kiround(g) as i128
            }
        }
    };
    let mut r = (sq as f64).sqrt() as i128;
    while r * r < sq {
        r += 1;
    }
    while r * r > sq {
        r -= 1;
    }
    r as i64
}

fn p64(v: V) -> eda_clipper2::Point64 {
    eda_clipper2::Point64::new(v.x, v.y)
}

fn from64(chain: &[eda_clipper2::Point64]) -> Vec<V> {
    chain.iter().map(|p| V::new(p.x, p.y)).collect()
}

/// `TransformOvalToPolygon( aBuffer, aStart, aEnd, aWidth, aError, aErrorLoc )`.
pub fn oval_polygon(start: V, end: V, width: i64, error: i64, loc: ErrorLoc) -> Vec<V> {
    from64(&eda_shape_poly_set::basic_shapes::oval_polygon(p64(start), p64(end), width, error, loc, 0))
}

/// `TransformCircleToPolygon( aBuffer, aCenter, aRadius, aError, aErrorLoc )`.
pub fn circle_polygon(center: V, radius: i64, error: i64, loc: ErrorLoc) -> Vec<V> {
    from64(&eda_shape_poly_set::basic_shapes::circle_polygon(p64(center), radius, error, loc, 0))
}

/// `TransformArcToPolygon( aBuffer, aStart, aMid, aEnd, aWidth, aError, aErrorLoc )` (`convert_basic_shapes_to_polygon.cpp`): the outline of an arc drawn `width`
/// wide -- the end cap at the start, the outside edge, the end cap at the end, the inside edge back -- always walked counter-clockwise.
pub fn transform_arc_to_polygon(start: V, mid: V, end: V, width: i64, error: i64, loc: ErrorLoc) -> Vec<V> {
    let distance_to_mid = seg_distance(start, end, mid);
    if distance_to_mid <= 1 {
        // Not an arc but essentially a straight line with a small error.
        return oval_polygon(start, end, width + distance_to_mid, error, loc);
    }
    // For a consistent winding the arc is always processed counter-clockwise, its ends swapped if need be.
    let (smx, smy) = ((mid.x - start.x) as i128, (mid.y - start.y) as i128);
    let (sex, sey) = ((end.x - start.x) as i128, (end.y - start.y) as i128);
    let cross = smx * sey - smy * sex;
    let (mut p0, mut p1) = (start, end);
    if cross < 0 {
        std::mem::swap(&mut p0, &mut p1);
    }
    let Some(params) = ArcChord::compute(p0, mid, p1) else {
        return oval_polygon(start, end, width, error, loc);
    };
    let start_angle = params.start_angle();
    let end_angle = params.end_angle();
    let radial_offset = width / 2;
    let arc_inner_radius = params.radius - radial_offset as f64;
    let accuracy = error as f64;
    let mut outline = Vec::new();
    // Starting end cap (a half circle at p0).
    arc_to_polyline_about(&mut outline, p0, radial_offset, start_angle - Angle(180.0), Angle(180.0), accuracy, loc);
    // Outside edge.
    arc_to_polyline_through(&mut outline, p0, mid, p1, accuracy, loc, radial_offset as f64);
    // Ending end cap (a half circle at p1).
    arc_to_polyline_about(&mut outline, p1, radial_offset, end_angle, Angle(180.0), accuracy, loc);
    // Inside edge, the other way.
    if arc_inner_radius > 0.0 {
        arc_to_polyline_through(&mut outline, p1, mid, p0, accuracy, opposite(loc), -(radial_offset as f64));
    }
    outline
}

/// `ROUNDED_CORNER`.
#[derive(Debug, Clone, Copy)]
pub struct RoundedCorner {
    pub pos: V,
    pub radius: i64,
}

/// `CornerListRemoveDuplicates`.
fn corner_list_remove_duplicates(corners: &mut Vec<RoundedCorner>) {
    let mut prev = corners[0].pos;
    let mut pos = corners.len() as i64 - 1;
    while pos >= 0 {
        if corners[pos as usize].pos == prev {
            corners.remove(pos as usize);
        } else {
            prev = corners[pos as usize].pos;
        }
        pos -= 1;
    }
}

/// `CornerListToPolygon( outline, corners, 0, error, ERROR_INSIDE )`: the outline of a convex shape given by its corners (clockwise, no duplicates), each with a radius.
pub fn corner_list_to_polygon_inside(corners: &[RoundedCorner], error: i64) -> Vec<V> {
    let mut outline: Vec<V> = Vec::new();
    let mut incoming = corners[0].pos - corners[corners.len() - 1].pos;
    let count = corners.len();
    for n in 0..count {
        let cur = corners[n];
        let next = corners[(n + 1) % count];
        let outgoing = next.pos - cur.pos;
        if cur.radius == 0 {
            outline.push(cur.pos);
        } else {
            let corner_position = cur.pos;
            let radius = cur.radius;
            let end_angle;
            let tan_angle2;
            if (incoming.x == 0 && outgoing.y == 0) || (incoming.y == 0 && outgoing.x == 0) {
                end_angle = Angle(90.0);
                tan_angle2 = 1.0;
            } else {
                let cos_num = incoming.x as f64 * outgoing.x as f64 + incoming.y as f64 * outgoing.y as f64;
                let cos_den = incoming.euclidean_norm() as f64 * outgoing.euclidean_norm() as f64;
                let angle = (cos_num / cos_den).acos();
                tan_angle2 = ((PI - angle) / 2.0).tan();
                end_angle = Angle(angle / (PI / 180.0));
            }

            // Ensure 16+ segments per 360deg and ensure first & last segment are the same size.
            let num_segs = 16.max(arc_to_segment_count(radius, error, 360.0));
            let ang_delta = Angle(360.0 / num_segs as f64);
            let mut last_seg = end_angle;
            if last_seg.0 > 0.0 {
                while last_seg.0 > ang_delta.0 {
                    last_seg = Angle(last_seg.0 - ang_delta.0);
                }
            } else {
                while last_seg.0 < -ang_delta.0 {
                    last_seg = Angle(last_seg.0 + ang_delta.0);
                }
            }
            let mut ang_pos = if last_seg.0 == 0.0 { ang_delta } else { Angle((ang_delta.0 + last_seg.0) / 2.0) };

            let arc_transition_distance = if tan_angle2 > 0.0 { radius as f64 / tan_angle2 } else { 0.0 };
            let arc_start = corner_position - incoming.resize(kiround(arc_transition_distance));
            let arc_center = arc_start + incoming.perpendicular().resize(radius);
            // ERROR_INSIDE
            let arc_end = reflect_point(corner_position, arc_center, arc_start);
            let arc_start_origin = arc_start - arc_center;
            outline.push(arc_start);
            while ang_pos.0 < end_angle.0 {
                let pt = rotate_point(arc_start_origin, -ang_pos);
                outline.push(pt + arc_center);
                ang_pos = Angle(ang_pos.0 + ang_delta.0);
            }
            outline.push(arc_end);
        }
        incoming = outgoing;
    }
    outline
}

pub const RECT_CHAMFER_TOP_LEFT: i32 = 1;
pub const RECT_CHAMFER_TOP_RIGHT: i32 = 2;
pub const RECT_CHAMFER_BOTTOM_LEFT: i32 = 4;
pub const RECT_CHAMFER_BOTTOM_RIGHT: i32 = 8;

/// `TransformRoundChamferedRectToPolygon( buffer, {0,0}, size, 0, corner_radius, chamfer_ratio, chamfer_corners, 0, error, ERROR_INSIDE )`: the outline of a
/// rounded and/or chamfered rectangle centred on the origin.
pub fn round_chamfered_rect_polygon(size: V, corner_radius: i64, chamfer_ratio: f64, chamfer_corners: i32, error: i64) -> Vec<V> {
    let half = V::new(size.x / 2, size.y / 2);
    let chamfer_cnt = (chamfer_corners & 0xff).count_ones();
    let mut corners: Vec<RoundedCorner> = vec![
        RoundedCorner { pos: V::new(-half.x, -half.y), radius: corner_radius },
        RoundedCorner { pos: V::new(half.x, -half.y), radius: corner_radius },
        RoundedCorner { pos: V::new(half.x, half.y), radius: corner_radius },
        RoundedCorner { pos: V::new(-half.x, half.y), radius: corner_radius },
    ];
    if chamfer_corners != 0 {
        let shorter_side = size.x.min(size.y);
        let chamfer = 0.max(kiround(chamfer_ratio * shorter_side as f64));
        let cham_id = [RECT_CHAMFER_TOP_LEFT, RECT_CHAMFER_TOP_RIGHT, RECT_CHAMFER_BOTTOM_RIGHT, RECT_CHAMFER_BOTTOM_LEFT];
        let sign: [i64; 8] = [0, 1, -1, 0, 0, -1, 1, 0];
        let mut pos = 0usize;
        for cc in 0..4usize {
            if chamfer_corners & cham_id[cc] == 0 {
                pos += 1;
                continue;
            }
            corners[pos].radius = 0;
            if chamfer == 0 {
                pos += 1;
                continue;
            }
            let dup = corners[pos];
            corners.insert(pos + 1, dup);
            corners[pos].pos.x += sign[(2 * cc) & 7] * chamfer;
            corners[pos].pos.y += sign[(2 * cc + 6) & 7] * chamfer;
            corners[pos + 1].pos.x += sign[(2 * cc + 1) & 7] * chamfer;
            corners[pos + 1].pos.y += sign[(2 * cc + 7) & 7] * chamfer;
            pos += 2;
        }
        if chamfer_cnt > 1 && 2 * chamfer >= shorter_side {
            corner_list_remove_duplicates(&mut corners);
        }
    }
    corner_list_to_polygon_inside(&corners, error)
}

/// `BuildConvexHull( result, points )`: Andrew's monotone chain, counter-clockwise, the last point not repeated.
pub fn build_convex_hull(points: &[V]) -> Vec<V> {
    let n = points.len();
    if n < 2 {
        return Vec::new();
    }
    let mut poly = points.to_vec();
    poly.sort_by(|a, b| a.x.cmp(&b.x).then(a.y.cmp(&b.y)));
    let cross = |o: V, a: V, b: V| -> i128 { (a.x - o.x) as i128 * (b.y - o.y) as i128 - (a.y - o.y) as i128 * (b.x - o.x) as i128 };
    let mut result = vec![V::default(); 2 * n];
    let mut k = 0usize;
    for p in &poly {
        while k >= 2 && cross(result[k - 2], result[k - 1], *p) <= 0 {
            k -= 1;
        }
        result[k] = *p;
        k += 1;
    }
    let mut t = k + 1;
    for ii in (0..n - 1).rev() {
        while k >= t && cross(result[k - 2], result[k - 1], poly[ii]) <= 0 {
            k -= 1;
        }
        result[k] = poly[ii];
        k += 1;
    }
    let _ = &mut t;
    if k > 1 && result[0] == result[k - 1] {
        k -= 1;
    }
    result.truncate(k);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_is_exact_on_quarter_turns_and_rounded_elsewhere() {
        assert_eq!(rotate_point(V::new(10, 3), Angle(90.0)), V::new(3, -10));
        assert_eq!(rotate_point(V::new(10, 3), Angle(180.0)), V::new(-10, -3));
        assert_eq!(rotate_point(V::new(10, 3), Angle(-90.0)), V::new(-3, 10));
        assert_eq!(rotate_point(V::new(1000, 0), Angle(45.0)), V::new(707, -707));
        assert_eq!(rotate_point(V::new(1000, 0), Angle(360.0)), V::new(1000, 0));
    }

    #[test]
    fn angles_normalize_like_eda_angle() {
        assert_eq!(Angle(-90.0).normalize().0, 270.0);
        assert_eq!(Angle(180.0).normalize180().0, 180.0);
        assert_eq!(Angle(-180.0).normalize180().0, 180.0);
        assert_eq!(Angle(540.0).normalize180().0, 180.0);
        assert_eq!(Angle(-135.0).normalize180().0, -135.0);
    }

    #[test]
    fn resize_keeps_the_direction() {
        assert_eq!(V::new(0, -1_401_902).resize(250_951), V::new(0, -250_951));
        assert_eq!(V::new(3, 4).resize(10), V::new(6, 8));
        assert_eq!(V::new(5, 5).resize(10), V::new(7, 7));
        assert_eq!(V::new(0, 0).resize(10), V::new(0, 0));
    }

    #[test]
    fn convex_hull_is_counter_clockwise_without_the_repeat() {
        let hull = build_convex_hull(&[V::new(0, 0), V::new(10, 0), V::new(10, 10), V::new(0, 10), V::new(5, 5)]);
        assert_eq!(hull, vec![V::new(0, 0), V::new(10, 0), V::new(10, 10), V::new(0, 10)]);
    }

    /// The polygon KiCad exports for the 1.15 x 1.4 mm pad of a 0805 footprint with a corner radius of 0.25 mm
    /// (`RoundRect[T]Pad_1150.000000x1400.000000_250.951000_um_0.000000_0` in the DSN KiCad wrote, `crates/freeroute/parity/fixtures`): the pad grown by
    /// the correction for a circle approximated by segments (`rradius * (1 - cos( pi / 36 ))`, 951 nm), and the corners rounded by sixteenths of a turn.
    #[test]
    fn a_rounded_rectangle_is_the_polygon_kicad_exports() {
        let extra = kiround(250_000.0 * (1.0 - (PI / 36.0).cos()));
        assert_eq!(extra, 951);
        let size = V::new(1_150_000 + 2 * extra, 1_400_000 + 2 * extra);
        let poly = round_chamfered_rect_polygon(size, 250_000 + extra, 0.0, 0, 5000);
        let dsn: Vec<(f64, f64)> = poly.iter().map(|p| (p.x as f64 / 1000.0, -(p.y as f64) / 1000.0)).collect();
        let want = [
            (-575.951, 450.0),
            (-556.848, 546.035),
            (-502.449, 627.449),
            (-421.035, 681.848),
            (-324.999, 700.95),
            (325.0, 700.951),
            (421.035, 681.848),
            (502.449, 627.449),
            (556.848, 546.035),
            (575.95, 449.999),
            (575.951, -450.0),
        ];
        for (i, w) in want.iter().enumerate() {
            let g = dsn[i];
            assert!((g.0 - w.0).abs() < 0.0015 && (g.1 - w.1).abs() < 0.0015, "vertex {i}: want {w:?} got {g:?}\nall: {dsn:?}");
        }
        assert_eq!(poly.len(), 20, "{dsn:?}");
    }
}
