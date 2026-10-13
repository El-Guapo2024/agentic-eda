//! The parts of KiCad's `SEG` (`libs/kimath/src/geometry/seg.cpp`) the dragger needs and
//! `eda_drc::kimath::Seg` has not: the infinite-line intersection (`IntersectLines`), the full
//! `SEG::intersects` with its collinear branch and the endpoint switch, `LineDistance` and
//! `SEG::Length`.
//!
//! Segments are `(a, b)` point pairs; all arithmetic is exact on `i128` and rounds where
//! KiCad's `rescale` / `KiROUND` do, so a result matches KiCad's at this IR's micrometre grid.

use eda_model::ir::Point;

/// A segment, `SEG( A, B )`.
pub type Seg2 = (Point, Point);

/// `rescale( num, val, den )` (`libs/kimath/src/math/util.cpp`): `num * val / den`, rounded to nearest, halves away from zero.
pub(crate) fn rescale(num: i128, val: i128, den: i128) -> i128 {
    let n = num * val;
    if (n < 0) ^ (den < 0) {
        (n - den / 2) / den
    } else {
        (n + den / 2) / den
    }
}

/// `VECTOR2::Cross`.
fn cross(a: (i128, i128), b: (i128, i128)) -> i128 {
    a.0 * b.1 - a.1 * b.0
}

fn vec_of(a: Point, b: Point) -> (i128, i128) {
    ((b.x - a.x) as i128, (b.y - a.y) as i128)
}

/// `SEG::checkCollinearOverlap`: where two collinear segments overlap, the midpoint of the overlap.
fn collinear_overlap(s: Seg2, o: Seg2, use_x_axis: bool, ignore_endpoints: bool) -> Option<Point> {
    let (a, b) = s;
    let (c, d) = o;
    let (s1_start, s1_end, s2_start, s2_end, c1_start, c1_end) = if use_x_axis { (a.x, b.x, c.x, d.x, a.y, b.y) } else { (a.y, b.y, c.y, d.y, a.x, b.x) };
    let (s1_min, s1_max) = (s1_start.min(s1_end), s1_start.max(s1_end));
    let (s2_min, s2_max) = (s2_start.min(s2_end), s2_start.max(s2_end));
    if !(s1_max >= s2_min && s2_max >= s1_min) {
        return None;
    }
    let overlap_start = s1_min.max(s2_min);
    let overlap_end = s1_max.min(s2_max);
    if ignore_endpoints && overlap_start == overlap_end {
        let at_s1_end = overlap_start == s1_min || overlap_start == s1_max;
        let at_s2_end = overlap_start == s2_min || overlap_start == s2_max;
        if at_s1_end && at_s2_end {
            return None;
        }
    }
    let proj = (overlap_start + overlap_end) / 2;
    let other = if s1_end != s1_start { c1_start + rescale((proj - s1_start) as i128, (c1_end - c1_start) as i128, (s1_end - s1_start) as i128) as i64 } else { c1_start };
    Some(if use_x_axis { Point { x: proj, y: other } } else { Point { x: other, y: proj } })
}

/// `SEG::intersects( aSeg, aIgnoreEndpoints, aLines, &pt )`: where the two segments (or, with `lines`, the two infinite lines
/// through them) cross. Parallel and not collinear is `None`; collinear lines give the midpoint of their start points, collinear
/// segments the midpoint of their overlap.
pub fn intersect(s: Seg2, o: Seg2, ignore_endpoints: bool, lines: bool) -> Option<Point> {
    let (a, b) = s;
    let (c, d) = o;
    if !lines && (a.x.max(b.x) < c.x.min(d.x) || c.x.max(d.x) < a.x.min(b.x) || a.y.max(b.y) < c.y.min(d.y) || c.y.max(d.y) < a.y.min(b.y)) {
        return None;
    }
    let dir1 = vec_of(a, b);
    let dir2 = vec_of(c, d);
    let offset = vec_of(a, c);
    let determinant = cross(dir2, dir1);
    if determinant == 0 {
        if cross(dir1, offset) != 0 {
            return None; // parallel, not collinear
        }
        if lines {
            return Some(if c == d {
                c
            } else if a == b {
                a
            } else {
                Point { x: (a.x + c.x) / 2, y: (a.y + c.y) / 2 }
            });
        }
        return collinear_overlap(s, o, dir1.0.abs() >= dir1.1.abs(), ignore_endpoints);
    }
    let param2 = cross(dir2, offset);
    let param1 = cross(dir1, offset);
    if !lines {
        if determinant > 0 {
            if param1 < 0 || param1 > determinant || param2 < 0 || param2 > determinant {
                return None;
            }
        } else if param1 > 0 || param1 < determinant || param2 > 0 || param2 < determinant {
            return None;
        }
        if ignore_endpoints && (param1 == 0 || param1 == determinant) && (param2 == 0 || param2 == determinant) {
            return None;
        }
    }
    Some(Point { x: c.x + rescale(param1, dir2.0, determinant) as i64, y: c.y + rescale(param1, dir2.1, determinant) as i64 })
}

/// `SEG::Intersect( aSeg )`: the segments' crossing point, endpoints included.
pub fn intersect_segs(s: Seg2, o: Seg2) -> Option<Point> {
    intersect(s, o, false, false)
}

/// `SEG::IntersectLines( aSeg )`: where the infinite lines through the two segments cross.
pub fn intersect_lines(s: Seg2, o: Seg2) -> Option<Point> {
    intersect(s, o, false, true)
}

/// `SEG::LineDistance( aP )`: the distance from `p` to the infinite line through the segment, rounded down.
pub fn line_distance(s: Seg2, p: Point) -> i64 {
    let (a, b) = s;
    let pp = a.y as i128 - b.y as i128;
    let q = b.x as i128 - a.x as i128;
    let r = -pp * a.x as i128 - q * a.y as i128;
    let l = pp * pp + q * q;
    let det = pp * p.x as i128 + q * p.y as i128 + r;
    let dist_sq = if l > 0 { rescale(det, det, l) } else { 0 };
    eda_drc::kimath::isqrt(dist_sq)
}

/// `SEG::Contains( aP )`: within `SquaredDistance <= 3` of the segment.
pub fn contains(s: Seg2, p: Point) -> bool {
    eda_drc::kimath::Seg::new(s.0, s.1).sq_distance_to_point(p) <= 3
}

/// `SEG::Collinear( aSeg )`: both ends of `o` are (to the cross product's `<= 1`) on the line through `s`.
pub fn collinear(s: Seg2, o: Seg2) -> bool {
    let (a, b) = s;
    let qa = a.y as i128 - b.y as i128;
    let qb = b.x as i128 - a.x as i128;
    let qc = -qa * a.x as i128 - qb * a.y as i128;
    let d1 = (o.0.x as i128 * qa + o.0.y as i128 * qb + qc).abs();
    let d2 = (o.1.x as i128 * qa + o.1.y as i128 * qb + qc).abs();
    d1 <= 1 && d2 <= 1
}

/// `SEG::Overlaps( aSeg )`: collinear, and one has an end of the other on it.
pub fn overlaps(s: Seg2, o: Seg2) -> bool {
    if o.0 == o.1 {
        // single point corner case
        if s.0 == o.0 || s.1 == o.0 {
            return false;
        }
        return contains(s, o.0);
    }
    if !collinear(s, o) {
        return false;
    }
    contains(s, o.0) || contains(s, o.1) || contains(o, s.0) || contains(o, s.1)
}

/// `SEG::Length()`: `VECTOR2I::EuclideanNorm`, rounded to the nearest unit.
pub fn length(a: Point, b: Point) -> i64 {
    let (dx, dy) = ((b.x - a.x).abs(), (b.y - a.y).abs());
    if dx == dy {
        (dx as f64 * std::f64::consts::SQRT_2).round() as i64
    } else if dx == 0 {
        dy
    } else if dy == 0 {
        dx
    } else {
        (dx as f64).hypot(dy as f64).round() as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i64, y: i64) -> Point {
        Point { x, y }
    }

    #[test]
    fn lines_cross_beyond_the_segments() {
        // y = 0 and x = 10, neither segment reaches the other
        assert_eq!(intersect_lines((p(0, 0), p(5, 0)), (p(10, 3), p(10, 8))), Some(p(10, 0)));
        assert_eq!(intersect_segs((p(0, 0), p(5, 0)), (p(10, 3), p(10, 8))), None);
    }

    #[test]
    fn parallel_lines_do_not_cross_and_collinear_ones_meet_between_their_starts() {
        assert_eq!(intersect_lines((p(0, 0), p(5, 0)), (p(0, 3), p(5, 3))), None);
        assert_eq!(intersect_lines((p(0, 0), p(5, 0)), (p(20, 0), p(30, 0))), Some(p(10, 0)));
        // a point as one of the segments is its own start
        assert_eq!(intersect_lines((p(7, 7), p(7, 7)), (p(0, 0), p(10, 0))), Some(p(7, 7)));
    }

    #[test]
    fn segments_that_touch_at_an_end_intersect_unless_endpoints_are_ignored() {
        let (s, o) = ((p(0, 0), p(10, 0)), (p(10, 0), p(10, 10)));
        assert_eq!(intersect(s, o, false, false), Some(p(10, 0)));
        assert_eq!(intersect(s, o, true, false), None);
    }

    #[test]
    fn collinear_overlap_is_its_midpoint() {
        assert_eq!(intersect_segs((p(0, 0), p(10, 0)), (p(6, 0), p(20, 0))), Some(p(8, 0)));
        assert_eq!(intersect_segs((p(0, 0), p(4, 0)), (p(6, 0), p(20, 0))), None);
    }

    #[test]
    fn line_distance_is_to_the_infinite_line() {
        assert_eq!(line_distance((p(0, 0), p(10, 0)), p(500, 7)), 7);
        assert_eq!(line_distance((p(0, 0), p(10, 10)), p(10, 0)), 7); // 10 / sqrt 2, rounded down
        assert_eq!(line_distance((p(3, 3), p(3, 3)), p(9, 9)), 0, "a point has no line");
    }

    #[test]
    fn collinear_segments_overlap_only_when_one_has_an_end_on_the_other() {
        let s = (p(0, 0), p(10, 0));
        assert!(collinear(s, (p(20, 0), p(30, 0))));
        assert!(!overlaps(s, (p(20, 0), p(30, 0))), "on the same line, apart");
        assert!(overlaps(s, (p(5, 0), p(30, 0))));
        assert!(overlaps(s, (p(2, 0), p(4, 0))), "inside");
        assert!(!overlaps(s, (p(0, 5), p(10, 5))), "parallel");
        assert!(!overlaps(s, (p(5, -5), p(5, 5))), "crossing");
        assert!(!overlaps(s, (p(10, 0), p(10, 0))), "a point at an end does not overlap");
        assert!(overlaps(s, (p(5, 0), p(5, 0))), "a point inside does");
    }

    #[test]
    fn length_is_rounded_like_euclidean_norm() {
        assert_eq!(length(p(0, 0), p(10, 10)), 14);
        assert_eq!(length(p(0, 0), p(3, 4)), 5);
        assert_eq!(length(p(0, 0), p(0, -9)), 9);
    }
}
