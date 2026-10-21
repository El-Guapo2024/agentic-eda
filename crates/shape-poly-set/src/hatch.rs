//! `SHAPE_POLY_SET::GenerateHatchLines` (`libs/kimath/src/geometry/shape_poly_set.cpp`) with the pieces it stands on: `SEG::IntersectsLine`
//! (`libs/kimath/src/geometry/seg.cpp`), `SHAPE_POLY_SET::Contains( aP, -1, 1, true )` (`containsSingle`: the even-odd test of
//! `SHAPE_LINE_CHAIN::PointInside` against the outline, and against each hole) and the `rescale` that test divides with
//! (`libs/kimath/src/math/util.cpp`, round to nearest).
//!
//! This is what `EDA_SHAPE::UpdateHatching` calls to make the lines of a hatched rectangle, circle or polygon, and what `ZONE::HatchBorder` calls for the border of a
//! zone; a zone's `aLineLength` (the short ticks of the border hatch) is kept, a shape's is -1. The points are in the unit of the set (micrometres here, nanometres in
//! KiCad): nothing below depends on the unit but the 0.5 of the collinear test of `IntersectsLine`, which is half the last digit in either.

use crate::ShapePolySet;
use eda_clipper2::Point64;

/// `KiROUND( double )`: half away from zero.
fn round(v: f64) -> i64 {
    v.round() as i64
}

/// `rescale( a, b, c )` of `int64_t`: `a * b / c` rounded to the nearest, the product kept in 128 bits.
fn rescale(a: i64, b: i64, c: i64) -> i64 {
    let n = a as i128 * b as i128;
    let c = c as i128;
    (if (n < 0) ^ (c < 0) { (n - c / 2) / c } else { (n + c / 2) / c }) as i64
}

/// `SHAPE_LINE_CHAIN::PointInside( aPt, 1 )` for a closed chain: the number of edges a ray towards +x crosses is odd.
pub fn point_inside(chain: &[Point64], pt: Point64) -> bool {
    let n = chain.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    for i in 0..n {
        let (p1, p2) = (chain[i], chain[(i + 1) % n]);
        let (dx, dy) = (p2.x - p1.x, p2.y - p1.y);
        if dy == 0 {
            continue;
        }
        let d = rescale(dx, pt.y - p1.y, dy);
        if ((p1.y >= pt.y) != (p2.y >= pt.y)) && (pt.x - p1.x < d) {
            inside = !inside;
        }
    }
    inside
}

/// `SEG::IntersectsLine( aSlope, aOffset, aIntersection )`: where the segment `a`-`b` meets the line `y = slope * x + offset`, if it does. A vertical segment answers at the
/// height the line has above it, a segment lying along the line (to within half a unit) by its middle.
fn intersects_line(a: Point64, b: Point64, slope: f64, offset: f64) -> Option<Point64> {
    let (sx, sy) = (b.x - a.x, b.y - a.y);
    if sx == 0 {
        let y = round(slope * a.x as f64 + offset);
        return (y >= a.y.min(b.y) && y <= a.y.max(b.y)).then(|| Point64::new(a.x, y));
    }
    // `VECTOR2L lineDir( 1000, static_cast<ecoord>( aSlope * 1000 ) )`: the cast truncates.
    let (lx, ly) = (1000i64, (slope * 1000.0) as i64);
    let cross = sx * ly - sy * lx;
    if cross == 0 {
        let expected_y = slope * a.x as f64 + offset;
        if (a.y as f64 - expected_y).abs() < 0.5 {
            return Some(Point64::new((a.x + b.x) / 2, (a.y + b.y) / 2));
        }
        return None;
    }
    let numerator = slope * a.x as f64 + offset - a.y as f64;
    let denominator = sy as f64 - slope * sx as f64;
    let t = numerator / denominator;
    (0.0..=1.0).contains(&t).then(|| Point64::new(round(a.x as f64 + t * sx as f64), round(a.y as f64 + t * sy as f64)))
}

impl ShapePolySet {
    /// `Contains( aP, -1, 1, true )`: whether the point is inside one of the polygons and in none of its holes (no tolerance at the edge).
    pub fn contains_point(&self, pt: Point64) -> bool {
        self.polys.iter().any(|poly| point_inside(&poly[0], pt) && !poly[1..].iter().any(|hole| point_inside(hole, pt)))
    }

    /// `GenerateHatchLines( aSlopes, aSpacing, aLineLength )`: the lines `y = slope * x + a`, one for each multiple of `spacing` in the range the set spans, cut into the pieces
    /// that lie inside the set. The crossings of a line with the edges (the holes' too) are sorted by descending x and each two neighbours that differ and have their middle
    /// inside make a piece. With `line_length` -1 a piece is a line; with another, a piece longer than twice that is two ticks of that length at its ends.
    pub fn generate_hatch_lines(&self, slopes: &[f64], spacing: i64, line_length: i64) -> Vec<(Point64, Point64)> {
        let mut hatch_lines = Vec::new();
        let mut vertices = self.polys.iter().flat_map(|p| p.iter()).flat_map(|c| c.iter());
        let Some(first) = vertices.next() else { return hatch_lines };
        if spacing <= 0 {
            return hatch_lines;
        }
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (first.x, first.x, first.y, first.y);
        for v in self.polys.iter().flat_map(|p| p.iter()).flat_map(|c| c.iter()) {
            min_x = min_x.min(v.x);
            max_x = max_x.max(v.x);
            min_y = min_y.min(v.y);
            max_y = max_y.max(v.y);
        }
        // `CIterateSegmentsWithHoles`: every edge of every contour, the last one closing it.
        let segments: Vec<(Point64, Point64)> = self.polys.iter().flat_map(|p| p.iter()).filter(|c| c.len() >= 2).flat_map(|c| (0..c.len()).map(move |i| (c[i], c[(i + 1) % c.len()]))).collect();

        for &slope0 in slopes {
            let mut slope = slope0;
            let (max_a, mut min_a) = if slope > 0.0 {
                (round(max_y as f64 - slope * min_x as f64), round(min_y as f64 - slope * max_x as f64))
            } else {
                (round(max_y as f64 - slope * max_x as f64), round(min_y as f64 - slope * min_x as f64))
            };
            min_a = (min_a / spacing) * spacing;

            let mut a = min_a;
            while a < max_a {
                let mut points: Vec<Point64> = Vec::new();
                for &(sa, sb) in &segments {
                    if let Some(pt) = intersects_line(sa, sb, slope, a as f64) {
                        // An intersection outside the box of the polygon is skipped.
                        if pt.x < min_x || pt.x > max_x || pt.y < min_y || pt.y > max_y {
                            continue;
                        }
                        points.push(pt);
                    }
                }
                // Sorted by descending x only when there are more than two, so the two ends of a piece come one after the other.
                if points.len() > 2 {
                    points.sort_by(|p, q| q.x.cmp(&p.x));
                }
                for ip in 0..points.len().saturating_sub(1) {
                    let (p1, p2) = (points[ip], points[ip + 1]);
                    // Duplicated intersections make no piece.
                    if p1 == p2 {
                        continue;
                    }
                    // A piece is inside when its middle is.
                    let mid = Point64::new((p1.x + p2.x) / 2, (p1.y + p2.y) / 2);
                    if !self.contains_point(mid) {
                        continue;
                    }
                    let dx = p2.x - p1.x;
                    if line_length == -1 || dx.abs() < 2 * line_length {
                        hatch_lines.push((p1, p2));
                    } else {
                        let dy = (p2.y - p1.y) as f64;
                        slope = dy / dx as f64;
                        let dx = if dx > 0 { line_length } else { -line_length };
                        let (x1, x2) = (round((p1.x + dx) as f64), round((p2.x - dx) as f64));
                        let (y1, y2) = (round(p1.y as f64 + dx as f64 * slope), round(p2.y as f64 - dx as f64 * slope));
                        hatch_lines.push((p1, Point64::new(x1, y1)));
                        hatch_lines.push((p2, Point64::new(x2, y2)));
                    }
                }
                a += spacing;
            }
        }
        hatch_lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: i64, y0: i64, x1: i64, y1: i64) -> ShapePolySet {
        ShapePolySet::from_outline(vec![Point64::new(x0, y0), Point64::new(x1, y0), Point64::new(x1, y1), Point64::new(x0, y1)])
    }

    #[test]
    fn the_even_odd_test_is_the_one_of_shape_line_chain() {
        let sq = square(0, 0, 10, 10);
        assert!(sq.contains_point(Point64::new(5, 5)));
        assert!(!sq.contains_point(Point64::new(15, 5)));
        assert!(!sq.contains_point(Point64::new(5, -3)));
        let mut with_hole = square(0, 0, 100, 100);
        with_hole.add_hole(vec![Point64::new(40, 40), Point64::new(60, 40), Point64::new(60, 60), Point64::new(40, 60)], Some(0));
        assert!(with_hole.contains_point(Point64::new(10, 10)));
        assert!(!with_hole.contains_point(Point64::new(50, 50)));
    }

    #[test]
    fn a_square_hatched_at_minus_one_has_one_line_per_spacing_across_its_diagonal() {
        // 100 by 100 at a spacing of 10: the lines y = -x + a with a from 0 to 200 in steps of 10, those at the corners being points.
        let lines = square(0, 0, 100, 100).generate_hatch_lines(&[-1.0], 10, -1);
        // Every line is a chord at 45 degrees inside the square, the ends on its edges.
        assert!(!lines.is_empty());
        for (p, q) in &lines {
            assert_eq!(p.x + p.y, q.x + q.y, "a line of slope -1 keeps x + y");
            for e in [p, q] {
                assert!((0..=100).contains(&e.x) && (0..=100).contains(&e.y));
            }
        }
        // 19 chords have two different ends (a = 10 .. 190); the two corner lines (a = 0, 200) are single points and make none.
        assert_eq!(lines.len(), 19);
        // The longest one is the diagonal.
        let longest = lines.iter().map(|(p, q)| (p.x - q.x).abs()).max().unwrap();
        assert_eq!(longest, 100);
    }

    #[test]
    fn a_reverse_hatch_runs_the_other_way_and_a_cross_hatch_has_both() {
        let sq = square(0, 0, 100, 100);
        let up = sq.generate_hatch_lines(&[1.0], 10, -1);
        assert!(up.iter().all(|(p, q)| p.y - p.x == q.y - q.x));
        let both = sq.generate_hatch_lines(&[1.0, -1.0], 10, -1);
        assert_eq!(both.len(), up.len() + sq.generate_hatch_lines(&[-1.0], 10, -1).len());
    }

    #[test]
    fn a_hole_cuts_the_lines_that_cross_it() {
        let mut p = square(0, 0, 100, 100);
        p.add_hole(vec![Point64::new(40, 40), Point64::new(60, 40), Point64::new(60, 60), Point64::new(40, 60)], Some(0));
        let lines = p.generate_hatch_lines(&[-1.0], 10, -1);
        // The diagonal x + y = 100 passes through the hole, so it is two pieces; none of the pieces has its middle in the hole.
        let on_diagonal: Vec<_> = lines.iter().filter(|(a, b)| a.x + a.y == 100 && b.x + b.y == 100).collect();
        assert_eq!(on_diagonal.len(), 2, "{on_diagonal:?}");
        for (a, b) in &lines {
            assert!(p.contains_point(Point64::new((a.x + b.x) / 2, (a.y + b.y) / 2)));
        }
    }

    #[test]
    fn an_empty_set_and_a_zero_spacing_make_no_lines() {
        assert!(ShapePolySet::new().generate_hatch_lines(&[-1.0], 10, -1).is_empty());
        assert!(square(0, 0, 10, 10).generate_hatch_lines(&[-1.0], 0, -1).is_empty());
    }

    #[test]
    fn line_length_cuts_a_long_piece_into_two_ticks() {
        // A flat triangle-free case: slope 0.0 lines across a 100-wide square, ticks of 10 at each end.
        let lines = square(0, 0, 100, 100).generate_hatch_lines(&[0.0], 50, 10);
        assert!(!lines.is_empty());
        assert!(lines.iter().all(|(p, q)| (p.x - q.x).abs() == 10));
    }
}
