//! `TransformCircleToPolygon` and `TransformOvalToPolygon` (`libs/kimath/src/convert_basic_shapes_to_polygon.cpp`): a circle and a stadium (a segment with round ends) as
//! polygons, the error of the approximation inside the shape (`ERROR_INSIDE`, every vertex on the true outline) or outside it (`ERROR_OUTSIDE`, the radius grown by the
//! error the vertices leave, `GetCircleToPolyCorrection( CircleToEndSegmentDeltaRadius( .. ) )`). The number of segments is that of `GetArcToSegmentCount` for a whole
//! circle, rounded up to a multiple of eight so the approximation lines up at 45 degrees.
//!
//! Hatching makes its lines into these (`EDA_SHAPE::UpdateHatching`) and a hatched shape's knockouts are made of them (`PCB_SHAPE::getHatchingKnockouts`).

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
