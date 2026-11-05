//! `TransformCircleToPolygon` and `TransformOvalToPolygon` (`libs/kimath/src/convert_basic_shapes_to_polygon.cpp`): a circle and a stadium (a segment with round ends) as
//! polygons, the error of the approximation inside the shape (`ERROR_INSIDE`, every vertex on the true outline) or outside it (`ERROR_OUTSIDE`, the radius grown by the
//! error the vertices leave, `GetCircleToPolyCorrection( CircleToEndSegmentDeltaRadius( .. ) )`). The number of segments is that of `GetArcToSegmentCount` for a whole
//! circle, rounded up to a multiple of eight so the approximation lines up at 45 degrees.
//!
//! Hatching makes its lines into these (`EDA_SHAPE::UpdateHatching`) and a hatched shape's knockouts are made of them (`PCB_SHAPE::getHatchingKnockouts`).
//!
//! `CornerListToPolygon` and what is built on it (`TransformRoundChamferedRectToPolygon`, `TransformTrapezoidToPolygon`) make the rectangles, rounded and chamfered
//! rectangles and trapezoids of the pads a teardrop is built on (`eda_connectivity::teardrop`).

use crate::{get_arc_to_segment_count, LineChain, ShapePolySet};
use eda_clipper2::Point64;

/// Where the error of an approximation goes (`ERROR_LOC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorLoc {
    Inside,
    Outside,
}

/// `CircleToEndSegmentDeltaRadius`: how far outside the circle of `radius` the vertices of a `seg_count`-gon with its sides tangent to it go.
fn circle_to_end_segment_delta_radius(radius: i64, seg_count: i64) -> i64 {
    let seg_count = if seg_count <= 2 { 3 } else { seg_count };
    let alpha = std::f64::consts::PI / seg_count as f64;
    (radius as f64 * (1.0 - 1.0 / alpha.cos())).abs().round() as i64
}

/// The number of segments of a full circle and the radius the vertices are at, for the radius of the shape and the allowed error.
fn circle_parameters(radius: i64, error: i64, loc: ErrorLoc, min_seg_count: i64) -> (i64, i64) {
    let mut num_segs = get_arc_to_segment_count(radius.clamp(1, i32::MAX as i64) as i32, error.clamp(1, i32::MAX as i64) as i32) as i64;
    num_segs = num_segs.max(min_seg_count);
    num_segs = (num_segs + 7) / 8 * 8;
    let mut r = radius;
    if loc == ErrorLoc::Outside {
        r += circle_to_end_segment_delta_radius(radius, num_segs);
    }
    (num_segs, r)
}

/// `RotatePoint( x, y, angle )` in KiCad's y-down frame, rounded: the point `(x, y)` turned by `angle` (radians).
fn rotate(x: f64, y: f64, angle: f64) -> (f64, f64) {
    let (s, c) = angle.sin_cos();
    (y * s + x * c, y * c - x * s)
}

/// `RotatePoint( aPoint, aAngle )` for a point of the plane (KiCad's y-down frame, `angle` in radians): the same rounding as the vertices of `SHAPE_POLY_SET::Rotate`.
pub fn rotate_point(p: Point64, angle: f64) -> Point64 {
    let (x, y) = rotate(p.x as f64, p.y as f64, angle);
    Point64::new(x.round() as i64, y.round() as i64)
}

/// `TransformCircleToPolygon( aBuffer, aCenter, aRadius, aError, aErrorLoc, aMinSegCount )`: the vertices are half a step off the x axis, `radius` from `center`.
pub fn circle_polygon(center: Point64, radius: i64, error: i64, loc: ErrorLoc, min_seg_count: i64) -> LineChain {
    let (num_segs, r) = circle_parameters(radius, error, loc, min_seg_count);
    let delta = std::f64::consts::TAU / num_segs as f64;
    (0..num_segs)
        .map(|k| {
            let (x, y) = rotate(r as f64, 0.0, delta / 2.0 + k as f64 * delta);
            Point64::new(center.x + x.round() as i64, center.y + y.round() as i64)
        })
        .collect()
}

/// `TransformOvalToPolygon( aBuffer, aStart, aEnd, aWidth, aError, aErrorLoc, aMinSegCount )`: the stadium of `width` along `start`-`end`. It is built along the x axis, the
/// round ends of radius `width / 2` (grown by the error for `Outside`), clamped to a band of exactly `width`, then turned to the segment's direction and moved to `start`.
pub fn oval_polygon(start: Point64, end: Point64, width: i64, error: i64, loc: ErrorLoc, min_seg_count: i64) -> LineChain {
    let half = width / 2;
    let (num_segs, radius) = circle_parameters(half, error, loc, min_seg_count);
    let delta = std::f64::consts::TAU / num_segs as f64;

    // `endp.x >= 0`: the segment is walked from its left end.
    let (mut endp, mut startp) = (Point64::new(end.x - start.x, end.y - start.y), start);
    if endp.x < 0 {
        endp = Point64::new(start.x - end.x, start.y - end.y);
        startp = end;
    }
    let delta_angle = (endp.y as f64).atan2(endp.x as f64);
    let seg_len = (endp.x as f64).hypot(endp.y as f64).round() as i64;

    let mut shape: LineChain = Vec::new();
    // The right end, from the top to the bottom.
    shape.push(Point64::new(seg_len, radius));
    let mut angle = delta / 2.0;
    while angle < std::f64::consts::PI {
        let (x, y) = rotate(0.0, radius as f64, angle);
        shape.push(Point64::new(seg_len + x.round() as i64, y.round() as i64));
        angle += delta;
    }
    shape.push(Point64::new(seg_len, -radius));
    // The left end, from the bottom to the top.
    shape.push(Point64::new(0, -radius));
    let mut angle = delta / 2.0;
    while angle < std::f64::consts::PI {
        let (x, y) = rotate(0.0, -(radius as f64), angle);
        shape.push(Point64::new(x.round() as i64, y.round() as i64));
        angle += delta;
    }
    shape.push(Point64::new(0, radius));

    // The band: a rectangle of the exact width, a little longer than the shape.
    let band = vec![Point64::new(-radius - 2, half), Point64::new(-radius - 2, -half), Point64::new(radius + seg_len + 2, -half), Point64::new(radius + seg_len + 2, half)];
    let mut clamped = ShapePolySet::from_outline(shape);
    clamped.boolean_intersection(&ShapePolySet::from_outline(band));

    let Some(poly) = clamped.polys.first() else { return Vec::new() };
    let (s, c) = delta_angle.sin_cos();
    poly[0].iter().map(|p| Point64::new(startp.x + (p.x as f64 * c - p.y as f64 * s).round() as i64, startp.y + (p.x as f64 * s + p.y as f64 * c).round() as i64)).collect()
}

// ------------------------------------------------------------------------------------------------------------ rectangles and trapezoids
//
// `CornerListToPolygon`, `TransformTrapezoidToPolygon` and `TransformRoundChamferedRectToPolygon` (`convert_basic_shapes_to_polygon.cpp`, 25ec2335 :242-520) with
// no inflate and `ERROR_INSIDE`, at the origin and not rotated, in KiCad's integer arithmetic: `PAD::TransformShapeToPolygon`'s rectangle, rounded and chamfered
// rectangle and trapezoid, which the teardrops of a pad that is not round are built on (`eda_connectivity::teardrop`). (`eda_kicad::specctra::geom` keeps the
// same port for the Specctra exporter; this one is where the crates below `eda-kicad` reach it.)

/// `EDA_ANGLE::Normalize`: degrees into [0, 360).
fn deg_normalize(mut a: f64) -> f64 {
    while a < -0.0 {
        a += 360.0;
    }
    while a >= 360.0 {
        a -= 360.0;
    }
    a
}

/// `EDA_ANGLE::Sin` and `Cos` of a normalised angle: exact at the multiples of 45 degrees, else `sin` / `cos` of it in radians.
fn deg_sin_cos(a: f64) -> (f64, f64) {
    let r = std::f64::consts::FRAC_1_SQRT_2;
    match a {
        t if t == 0.0 => (0.0, 1.0),
        t if t == 45.0 => (r, r),
        t if t == 90.0 => (1.0, 0.0),
        t if t == 135.0 => (r, -r),
        t if t == 180.0 => (0.0, -1.0),
        t if t == 225.0 => (-r, -r),
        t if t == 270.0 => (-1.0, 0.0),
        t if t == 315.0 => (-r, r),
        _ => {
            let rad = a * (std::f64::consts::PI / 180.0);
            (rad.sin(), rad.cos())
        }
    }
}

/// `RotatePoint( VECTOR2I& aPoint, const EDA_ANGLE& aAngle )` (`trigo.cpp`), `degrees` as an `EDA_ANGLE` holds them: the angle normalised, the quarter turns exact,
/// else `KiROUND( y * sin + x * cos )`, `KiROUND( y * cos - x * sin )` of the normalised angle.
pub fn rotate_point_degrees(p: Point64, degrees: f64) -> Point64 {
    let a = deg_normalize(degrees);
    if a == 0.0 {
        p
    } else if a == 90.0 {
        Point64::new(p.y, -p.x)
    } else if a == 180.0 {
        Point64::new(-p.x, -p.y)
    } else if a == 270.0 {
        Point64::new(-p.y, p.x)
    } else {
        let (s, c) = deg_sin_cos(a);
        Point64::new((p.y as f64 * s + p.x as f64 * c).round() as i64, (p.y as f64 * c - p.x as f64 * s).round() as i64)
    }
}

/// `rescale( a, b, c )` (`util.cpp`) of 64 bit integers: `a * b / c` rounded half away from zero, the product in 128 bits.
fn rescale128(a: i128, b: i128, c: i128) -> i128 {
    if c == 0 {
        return 0;
    }
    let n = a * b;
    if (n < 0) ^ (c < 0) {
        (n - c / 2) / c
    } else {
        (n + c / 2) / c
    }
}

/// `VECTOR2I::Resize( aNewLength )`: the same direction, `len` long, exact on the diagonals and the axes.
fn resize(v: Point64, len: i64) -> Point64 {
    if v.x == 0 && v.y == 0 {
        return Point64::new(0, 0);
    }
    let (nx, ny) = if v.x.abs() == v.y.abs() {
        let n = len.abs() as f64 * std::f64::consts::FRAC_1_SQRT_2;
        (n, n)
    } else {
        let (xs, ys) = (v.x as i128 * v.x as i128, v.y as i128 * v.y as i128);
        let ls = len as i128 * len as i128;
        ((rescale128(ls, xs, xs + ys) as f64).sqrt(), (rescale128(ls, ys, xs + ys) as f64).sqrt())
    };
    let sign = len.signum();
    let (rx, ry) = (nx.round() as i64, ny.round() as i64);
    Point64::new(if v.x < 0 { -rx } else { rx } * sign, if v.y < 0 { -ry } else { ry } * sign)
}

/// `SEG( a, b ).ReflectPoint( p )`: `p` mirrored in the line through `a` and `b`.
fn reflect_point(a: Point64, b: Point64, p: Point64) -> Point64 {
    let (dx, dy) = ((b.x - a.x) as i128, (b.y - a.y) as i128);
    let l2 = dx * dx + dy * dy;
    let t = dx * (p.x - a.x) as i128 + dy * (p.y - a.y) as i128;
    let (cx, cy) = if l2 == 0 { (p.x as i128, p.y as i128) } else { (a.x as i128 + rescale128(t, dx, l2), a.y as i128 + rescale128(t, dy, l2)) };
    Point64::new((2 * cx - p.x as i128) as i64, (2 * cy - p.y as i128) as i64)
}

/// `VECTOR2I::EuclideanNorm`: `KiROUND( hypot )`, exact on the axes and the diagonals.
fn euclidean_norm(v: Point64) -> i64 {
    if v.x.abs() == v.y.abs() {
        (v.x.abs() as f64 * std::f64::consts::SQRT_2).round() as i64
    } else if v.x == 0 {
        v.y.abs()
    } else if v.y == 0 {
        v.x.abs()
    } else {
        (v.x as f64).hypot(v.y as f64).round() as i64
    }
}

/// `ROUNDED_CORNER`: a corner of a convex outline and the radius it is rounded by (0: a sharp corner).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoundedCorner {
    pub pos: Point64,
    pub radius: i64,
}

/// `CornerListRemoveDuplicates`.
fn corner_list_remove_duplicates(corners: &mut Vec<RoundedCorner>) {
    let mut prev = corners[0].pos;
    for pos in (0..corners.len()).rev() {
        if corners[pos].pos == prev {
            corners.remove(pos);
        } else {
            prev = corners[pos].pos;
        }
    }
}

/// `CornerListToPolygon( outline, corners, 0, aError, ERROR_INSIDE )`: the outline of a convex shape given by its corners (clockwise on the screen, no duplicates), each
/// corner with a radius replaced by its arc -- at least 16 segments a turn (`GetArcToSegmentCount`), the first and the last step of an arc the same size, every vertex
/// on the true arc. A point equal to the one before it is not added again (`SHAPE_LINE_CHAIN::Append`).
pub fn corner_list_to_polygon(corners: &[RoundedCorner], error: i64) -> LineChain {
    fn append(out: &mut LineChain, p: Point64) {
        if out.last() != Some(&p) {
            out.push(p);
        }
    }
    let mut out: LineChain = Vec::new();
    let count = corners.len();
    if count == 0 {
        return out;
    }
    let mut incoming = Point64::new(corners[0].pos.x - corners[count - 1].pos.x, corners[0].pos.y - corners[count - 1].pos.y);
    for n in 0..count {
        let cur = corners[n];
        let next = corners[(n + 1) % count];
        let outgoing = Point64::new(next.pos.x - cur.pos.x, next.pos.y - cur.pos.y);
        if cur.radius == 0 {
            append(&mut out, cur.pos);
        } else {
            let (corner, radius) = (cur.pos, cur.radius);
            let (end_angle, tan_angle2) = if (incoming.x == 0 && outgoing.y == 0) || (incoming.y == 0 && outgoing.x == 0) {
                (90.0, 1.0)
            } else {
                let cos_num = incoming.x as f64 * outgoing.x as f64 + incoming.y as f64 * outgoing.y as f64;
                let cos_den = euclidean_norm(incoming) as f64 * euclidean_norm(outgoing) as f64;
                let angle = (cos_num / cos_den).acos();
                (angle / (std::f64::consts::PI / 180.0), ((std::f64::consts::PI - angle) / 2.0).tan())
            };
            // "Ensure 16+ segments per 360deg and ensure first & last segment are the same size".
            let num_segs = 16.max(crate::get_arc_to_segment_count(radius.clamp(1, i32::MAX as i64) as i32, error.clamp(1, i32::MAX as i64) as i32));
            let ang_delta = 360.0 / num_segs as f64;
            let mut last_seg = end_angle;
            if last_seg > 0.0 {
                while last_seg > ang_delta {
                    last_seg -= ang_delta;
                }
            } else {
                while last_seg < -ang_delta {
                    last_seg += ang_delta;
                }
            }
            let mut ang_pos = if last_seg == 0.0 { ang_delta } else { (ang_delta + last_seg) / 2.0 };
            // `incoming.Resize( arcTransitionDistance )`: the double is truncated to the `int` `Resize` takes.
            let transition = if tan_angle2 > 0.0 { radius as f64 / tan_angle2 } else { 0.0 };
            let inc = resize(incoming, transition as i64);
            let arc_start = Point64::new(corner.x - inc.x, corner.y - inc.y);
            let perp = resize(Point64::new(-incoming.y, incoming.x), radius);
            let arc_center = Point64::new(arc_start.x + perp.x, arc_start.y + perp.y);
            let arc_end = reflect_point(corner, arc_center, arc_start);
            let origin = Point64::new(arc_start.x - arc_center.x, arc_start.y - arc_center.y);
            append(&mut out, arc_start);
            while ang_pos < end_angle {
                let p = rotate_point_degrees(origin, -ang_pos);
                append(&mut out, Point64::new(p.x + arc_center.x, p.y + arc_center.y));
                ang_pos += ang_delta;
            }
            append(&mut out, arc_end);
        }
        incoming = outgoing;
    }
    out
}

/// `TransformRoundChamferedRectToPolygon( buffer, { 0, 0 }, size, ANGLE_0, corner_radius, chamfer_ratio, chamfer_corners, 0, error, ERROR_INSIDE )`: a `size`
/// rectangle about the origin whose corners in `chamfer_corners` (`RECT_CHAMFER_POSITIONS`: top left 1, top right 2, bottom left 4, bottom right 8) are cut off by
/// `KiROUND( chamfer_ratio * shorter side )` and the others rounded by `corner_radius`. With no radius and no chamfer it is the plain rectangle.
pub fn round_chamfered_rect_polygon(size: (i64, i64), corner_radius: i64, chamfer_ratio: f64, chamfer_corners: u8, error: i64) -> LineChain {
    let (sx, sy) = (size.0 / 2, size.1 / 2);
    let mut corners = vec![
        RoundedCorner { pos: Point64::new(-sx, -sy), radius: corner_radius },
        RoundedCorner { pos: Point64::new(sx, -sy), radius: corner_radius },
        RoundedCorner { pos: Point64::new(sx, sy), radius: corner_radius },
        RoundedCorner { pos: Point64::new(-sx, sy), radius: corner_radius },
    ];
    if chamfer_corners != 0 {
        let shorter = size.0.min(size.1);
        let chamfer = ((chamfer_ratio * shorter as f64).round() as i64).max(0);
        // top left, top right, bottom right, bottom left
        let cham_id = [1u8, 2, 8, 4];
        let sign: [i64; 8] = [0, 1, -1, 0, 0, -1, 1, 0];
        let mut pos = 0usize;
        for (cc, id) in cham_id.iter().enumerate() {
            if chamfer_corners & id != 0 {
                corners[pos].radius = 0;
                if chamfer != 0 {
                    corners.insert(pos + 1, corners[pos]);
                    corners[pos].pos.x += sign[(2 * cc) & 7] * chamfer;
                    corners[pos].pos.y += sign[(2 * cc + 6) & 7] * chamfer; // ( 2 * cc - 2 ) & 7
                    corners[pos + 1].pos.x += sign[(2 * cc + 1) & 7] * chamfer;
                    corners[pos + 1].pos.y += sign[(2 * cc + 7) & 7] * chamfer; // ( 2 * cc - 1 ) & 7
                    pos += 1;
                }
            }
            pos += 1;
        }
        if chamfer_corners.count_ones() > 1 && 2 * chamfer >= shorter {
            corner_list_remove_duplicates(&mut corners);
        }
    }
    corner_list_to_polygon(&corners, error)
}

/// `TransformTrapezoidToPolygon( buffer, { 0, 0 }, size, ANGLE_0, delta_x, delta_y, 0, error, ERROR_INSIDE )`: the trapezoid about the origin, `delta_x` and `delta_y`
/// half the pad's `(rect_delta ..)` (`PAD::TransformShapeToPolygon`'s `ddx`, `ddy`); a corner a delta cancels is taken out (`CornerListRemoveDuplicates`).
pub fn trapezoid_polygon(size: (i64, i64), delta_x: i64, delta_y: i64, error: i64) -> LineChain {
    let (sx, sy) = (size.0 / 2, size.1 / 2);
    let mut corners = vec![
        RoundedCorner { pos: Point64::new(-sx + delta_y, -sy - delta_x), radius: 0 },
        RoundedCorner { pos: Point64::new(sx - delta_y, -sy + delta_x), radius: 0 },
        RoundedCorner { pos: Point64::new(sx + delta_y, sy - delta_x), radius: 0 },
        RoundedCorner { pos: Point64::new(-sx - delta_y, sy + delta_x), radius: 0 },
    ];
    if delta_y.abs() == sx.abs() || delta_x.abs() == sy.abs() {
        corner_list_remove_duplicates(&mut corners);
    }
    corner_list_to_polygon(&corners, error)
}

#[cfg(test)]
mod corner_list_tests {
    use super::*;

    #[test]
    fn a_rectangle_is_its_four_corners_clockwise_on_the_screen() {
        let r = round_chamfered_rect_polygon((1_500_000, 1_000_000), 0, 0.0, 0, 5_000);
        assert_eq!(r, vec![Point64::new(-750_000, -500_000), Point64::new(750_000, -500_000), Point64::new(750_000, 500_000), Point64::new(-750_000, 500_000)]);
    }

    #[test]
    fn a_rounded_rectangles_corners_are_arcs_with_every_vertex_on_the_arc() {
        // 1.2 x 0.9 mm, radius 0.225 mm (25 % of 0.9).
        let r = round_chamfered_rect_polygon((1_200_000, 900_000), 225_000, 0.0, 0, 5_000);
        // The top left corner's arc starts on the left side, `radius` below the corner, and ends on the top side `radius` right of it.
        assert_eq!(r[0], Point64::new(-600_000, -225_000));
        let top = r.iter().position(|p| *p == Point64::new(-375_000, -450_000)).expect("the arc's end on the top side");
        for p in &r[1..top] {
            let d = ((p.x + 375_000) as f64).hypot((p.y + 225_000) as f64);
            assert!((d - 225_000.0).abs() < 1.0, "every vertex on the arc: {d}");
        }
        // Four arcs of the same count, the straight sides between them.
        assert_eq!(r.len(), 4 * (top + 1), "{r:?}");
        // The first and the last step of the arc are the same size.
        let step = |a: Point64, b: Point64| ((a.x - b.x) as f64).hypot((a.y - b.y) as f64);
        assert!((step(r[0], r[1]) - step(r[top - 1], r[top])).abs() < 2.0);
    }

    #[test]
    fn a_chamfered_corner_is_cut_and_a_trapezoid_narrows() {
        // 1.5 x 1 mm, the top right corner chamfered by 20 % of 1 mm.
        let c = round_chamfered_rect_polygon((1_500_000, 1_000_000), 0, 0.2, 2, 5_000);
        assert_eq!(c, vec![Point64::new(-750_000, -500_000), Point64::new(550_000, -500_000), Point64::new(750_000, -300_000), Point64::new(750_000, 500_000), Point64::new(-750_000, 500_000)]);
        // `(rect_delta 0 0.3)`: the half delta 0.15 mm makes the top 0.3 mm narrower and the bottom 0.3 mm wider.
        let t = trapezoid_polygon((1_500_000, 1_000_000), 0, 150_000, 5_000);
        assert_eq!(t, vec![Point64::new(-600_000, -500_000), Point64::new(600_000, -500_000), Point64::new(900_000, 500_000), Point64::new(-900_000, 500_000)]);
    }

    #[test]
    fn rotate_point_degrees_is_kicads_rotate_point() {
        assert_eq!(rotate_point_degrees(Point64::new(10, 3), 90.0), Point64::new(3, -10));
        assert_eq!(rotate_point_degrees(Point64::new(10, 3), -90.0), Point64::new(-3, 10));
        assert_eq!(rotate_point_degrees(Point64::new(1000, 0), 45.0), Point64::new(707, -707));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn max_radius(poly: &[Point64], c: Point64) -> f64 {
        poly.iter().map(|p| ((p.x - c.x) as f64).hypot((p.y - c.y) as f64)).fold(0.0, f64::max)
    }

    #[test]
    fn a_circle_has_a_multiple_of_eight_vertices_on_it_or_outside_it() {
        let c = Point64::new(1000, -2000);
        let inside = circle_polygon(c, 500_000, 5_000, ErrorLoc::Inside, 0);
        assert_eq!(inside.len() % 8, 0);
        assert!(inside.len() >= 8);
        for p in &inside {
            let d = ((p.x - c.x) as f64).hypot((p.y - c.y) as f64);
            assert!((d - 500_000.0).abs() < 2.0, "{d}");
        }
        let outside = circle_polygon(c, 500_000, 5_000, ErrorLoc::Outside, 0);
        assert_eq!(outside.len(), inside.len());
        assert!(max_radius(&outside, c) > 500_000.0);
        assert!(max_radius(&outside, c) <= 500_000.0 + 5_000.0 + 2.0);
    }

    #[test]
    fn a_stadium_is_its_width_wide_and_its_length_long_with_round_ends() {
        // A horizontal one: from (0, 0) to (1000, 0), 200 wide.
        let poly = oval_polygon(Point64::new(0, 0), Point64::new(1000, 0), 200, 20, ErrorLoc::Inside, 0);
        assert!(poly.len() >= 8);
        let (min_x, max_x) = (poly.iter().map(|p| p.x).min().unwrap(), poly.iter().map(|p| p.x).max().unwrap());
        let (min_y, max_y) = (poly.iter().map(|p| p.y).min().unwrap(), poly.iter().map(|p| p.y).max().unwrap());
        // Eight segments round: the vertices of a cap are half a step (22.5 degrees) off the axis, so the polygon reaches cos(22.5 degrees) of the radius along it.
        assert_eq!((min_x, max_x), (-92, 1092));
        assert_eq!((min_y, max_y), (-100, 100));
    }

    #[test]
    fn a_stadium_turned_by_45_degrees_keeps_its_axis() {
        let poly = oval_polygon(Point64::new(0, 0), Point64::new(1000, 1000), 200, 20, ErrorLoc::Inside, 0);
        // Its centre is the middle of the segment.
        let n = poly.len() as f64;
        let (cx, cy) = (poly.iter().map(|p| p.x as f64).sum::<f64>() / n, poly.iter().map(|p| p.y as f64).sum::<f64>() / n);
        assert!((cx - 500.0).abs() < 2.0 && (cy - 500.0).abs() < 2.0, "{cx} {cy}");
        // Every vertex is within the width of the axis x = y.
        for p in &poly {
            let off = (p.x - p.y) as f64 / std::f64::consts::SQRT_2;
            assert!(off.abs() <= 100.5, "{off}");
        }
    }

    #[test]
    fn an_outside_stadium_is_not_wider_than_it_is_asked() {
        // The band clamps the grown round ends back to the width.
        let poly = oval_polygon(Point64::new(0, 0), Point64::new(1000, 0), 200, 20, ErrorLoc::Outside, 0);
        assert!(poly.iter().all(|p| p.y.abs() <= 100));
        assert!(poly.iter().map(|p| p.x).min().unwrap() <= -100);
    }

    #[test]
    fn the_end_of_the_segment_that_is_first_does_not_matter() {
        let a = oval_polygon(Point64::new(0, 0), Point64::new(700, 300), 100, 10, ErrorLoc::Inside, 0);
        let b = oval_polygon(Point64::new(700, 300), Point64::new(0, 0), 100, 10, ErrorLoc::Inside, 0);
        let (mut a, mut b) = (a, b);
        a.sort_by_key(|p| (p.x, p.y));
        b.sort_by_key(|p| (p.x, p.y));
        assert_eq!(a, b);
    }

    #[test]
    fn a_hatch_line_stadium_has_the_eight_segments_of_its_width_as_the_error() {
        // `maxError = lineWidth`: a line 100 wide is made with eight segments round, so a cap of three vertices on each end.
        let poly = oval_polygon(Point64::new(0, 0), Point64::new(1000, 0), 100, 100, ErrorLoc::Inside, 0);
        // Per end: the two corners of the straight sides and four vertices between them.
        assert_eq!(poly.len(), 12, "{poly:?}");
    }
}
