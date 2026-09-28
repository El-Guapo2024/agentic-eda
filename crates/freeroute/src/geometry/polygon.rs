//! Polygons that need not be convex, and areas with holes: how keepouts
//! and copper pours arrive, and how FreeRouting splits them into the
//! convex pieces its search tree stores. Ported from `Polygon`,
//! `PolygonShape` and `PolylineArea`.
//!
//! Each split cuts first at a concave corner found from a random start,
//! drawn from Java's own random number generator seeded with 99 afresh for
//! every polygon. The generator is ported too: any other would cut at other
//! corners, and the pieces would differ.

use super::line::{java_round, Line, Side};
use super::{IntPoint, TileShape};

/// `java.util.Random`: a 48-bit linear congruential generator, bit for bit.
struct JavaRandom {
    seed: i64,
}

impl JavaRandom {
    const MULTIPLIER: i64 = 0x5_DEEC_E66D;
    const ADDEND: i64 = 0xB;
    const MASK: i64 = (1 << 48) - 1;

    /// `new Random(seed)`, or `setSeed(seed)`.
    fn new(seed: i64) -> JavaRandom {
        JavaRandom { seed: (seed ^ Self::MULTIPLIER) & Self::MASK }
    }

    /// `Random.next(bits)`.
    fn next(&mut self, bits: u32) -> i32 {
        self.seed = self.seed.wrapping_mul(Self::MULTIPLIER).wrapping_add(Self::ADDEND) & Self::MASK;
        ((self.seed as u64) >> (48 - bits)) as i32
    }

    /// A value in `0..bound`, `bound` positive. `Random.nextInt(int)`,
    /// rejecting draws from the uneven top of the range as it does, its
    /// test relying on `int` overflow.
    fn next_int(&mut self, bound: i32) -> i32 {
        let r = self.next(31);
        let m = bound - 1;
        if bound & m == 0 {
            return ((bound as i64 * r as i64) >> 31) as i32;
        }
        let mut u = r;
        loop {
            let r = u % bound;
            if u.wrapping_sub(r).wrapping_add(m) >= 0 {
                return r;
            }
            u = self.next(31);
        }
    }
}

/// The seed `PolygonShape` resets its generator to for every split.
const SEED: i64 = 99;

/// A simple polygon, convex or not, by its corners counter-clockwise from
/// the lowest, then leftmost, one. FreeRouting's `PolygonShape`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolygonShape {
    pub corners: Vec<IntPoint>,
}

impl PolygonShape {
    /// The polygon through `points`: repeated and straight corners dropped,
    /// turned counter-clockwise, and started from its lowest corner.
    /// `PolygonShape(Point[])`.
    ///
    /// Only the last corner, then the first, is tested for lying straight
    /// between its neighbours across the closing side, once each, as in
    /// the Java; one that is still straight after moves into the list.
    pub fn new(points: &[IntPoint]) -> PolygonShape {
        let mut corners = polygon_corners(points);
        if winding_number_after_closing(&corners) < 0 {
            corners.reverse();
            corners = polygon_corners(&corners);
        }
        if corners.is_empty() {
            // The Java fails here, reading a start corner that is not there.
            return PolygonShape { corners };
        }
        let mut last = corners.len() - 1;
        if last > 0 && corners[0] == corners[last] {
            last -= 1;
        }
        if last >= 2 && corners[last].side_of(corners[last - 1], corners[0]) == Side::Collinear {
            last -= 1;
        }
        let mut first = 0;
        if last >= 2 && corners[0].side_of(corners[1], corners[last]) == Side::Collinear {
            first += 1;
        }
        let mut start = first;
        for i in first + 1..=last {
            let (c, s) = (corners[i], corners[start]);
            if c.y < s.y || c.y == s.y && c.x < s.x {
                start = i;
            }
        }
        let mut result = corners[start..=last].to_vec();
        result.extend_from_slice(&corners[first..start]);
        PolygonShape { corners: result }
    }

    /// -1 empty, 0 a point, 1 a segment, 2 an area: by the number of
    /// corners alone. `PolygonShape.dimension`.
    pub fn dimension(&self) -> i32 {
        match self.corners.len() {
            0 => -1,
            1 => 0,
            2 => 1,
            _ => 2,
        }
    }

    /// The polygon cut into convex pieces, in FreeRouting's order.
    /// `None` where a cut finds no side to end on, as the Java's null.
    /// `PolygonShape.split_to_convex`.
    pub fn split_to_convex(&self) -> Option<Vec<TileShape>> {
        let mut random = JavaRandom::new(SEED);
        let pieces = self.split_to_convex_recu(&mut random)?;
        Some(pieces.iter().map(|p| TileShape::from_corners(&p.corners)).collect())
    }

    /// From a random corner on, the first concave corner is cut across,
    /// horizontally or vertically, to the nearest side; the two halves are
    /// split in turn, the first half's pieces first. A polygon with no
    /// concave corner is one piece. `PolygonShape.split_to_convex_recu`.
    fn split_to_convex_recu(&self, random: &mut JavaRandom) -> Option<Vec<PolygonShape>> {
        let c = &self.corners;
        let n = c.len();
        if n == 0 {
            // `nextInt(0)` throws in the Java.
            return None;
        }
        let mut start = random.next_int(n as i32) as usize;
        let mut curr = c[start];
        let mut prev = c[if start != 0 { start - 1 } else { n - 1 }];
        let mut concave = None;
        for _ in 0..n {
            let next = c[if start < n - 1 { start + 1 } else { 0 }];
            if next.side_of(prev, curr) == Side::Right {
                concave = Some(start);
                break;
            }
            prev = curr;
            curr = next;
            start = (start + 1) % n;
        }
        let Some(concave) = concave else {
            return Some(vec![self.clone()]);
        };
        let ((px, py), after) = self.division_point(concave)?;
        let projection = IntPoint::new(java_round(px), java_round(py));
        // From the concave corner to the side cut, then back along the cut.
        let count = (after + n - concave) % n + 1;
        let mut first_arr: Vec<IntPoint> = (0..count - 1).map(|i| c[(concave + i) % n]).collect();
        first_arr.push(projection);
        // From the cut on round to the concave corner.
        let count = (concave + n - after) % n + 2;
        let mut last_arr = vec![projection];
        last_arr.extend((0..count - 1).map(|i| c[(after + i) % n]));
        let mut result = PolygonShape::new(&first_arr).split_to_convex_recu(random)?;
        result.extend(PolygonShape::new(&last_arr).split_to_convex_recu(random)?);
        Some(result)
    }

    /// Where a cut from concave corner `no` ends: the nearest crossing of a
    /// horizontal or vertical line through the corner with a side of the
    /// polygon, in a direction the corner's own sides open towards; with the
    /// number of the corner at that side's end. `None` if there is none.
    /// `PolygonShape.DivisionPoint`.
    fn division_point(&self, no: usize) -> Option<((f64, f64), usize)> {
        let c = &self.corners;
        let n = c.len();
        let float = |p: IntPoint| (p.x as f64, p.y as f64);
        let concave = float(c[no]);
        let before = float(c[if no != 0 { no - 1 } else { n - 1 }]);
        let after = float(if no == n - 1 { c[0] } else { c[no + 1] });
        let search_right = before.1 > concave.1 || concave.1 > after.1;
        let search_left = before.1 < concave.1 || concave.1 < after.1;
        let search_up = before.0 < concave.0 || concave.0 < after.0;
        let search_down = before.0 > concave.0 || concave.0 > after.0;
        let mut min_projection_dist = i32::MAX as f64;
        let mut min_projection = None;
        let mut corner_no_after_min_projection = 0;
        let mut corner_no_after_curr_projection = (no + 2) % n;
        let mut corner_before_curr_projection =
            c[if corner_no_after_curr_projection != 0 { corner_no_after_curr_projection - 1 } else { n - 1 }];
        let mut before_approx = float(corner_before_curr_projection);
        for _ in 0..n.saturating_sub(2) {
            let corner_after_curr_projection = c[corner_no_after_curr_projection];
            let after_approx = float(corner_after_curr_projection);
            let side = Line::new(corner_before_curr_projection, corner_after_curr_projection);
            if before_approx.1 != after_approx.1 {
                let (min_y, max_y) = if after_approx.1 > before_approx.1 {
                    (before_approx.1, after_approx.1)
                } else {
                    (after_approx.1, before_approx.1)
                };
                if concave.1 >= min_y && concave.1 <= max_y {
                    let x_intersect = side.function_in_y_value_approx(concave.1);
                    let curr_dist = (x_intersect - concave.0).abs();
                    let projection_ok = curr_dist < min_projection_dist
                        && (search_right && x_intersect > concave.0 && concave.1 <= after_approx.1
                            || search_left && x_intersect < concave.0 && concave.1 >= after_approx.1);
                    if projection_ok {
                        min_projection_dist = curr_dist;
                        corner_no_after_min_projection = corner_no_after_curr_projection;
                        min_projection = Some((x_intersect, concave.1));
                    }
                }
            }
            if before_approx.0 != after_approx.0 {
                let (min_x, max_x) = if after_approx.0 > before_approx.0 {
                    (before_approx.0, after_approx.0)
                } else {
                    (after_approx.0, before_approx.0)
                };
                if concave.0 >= min_x && concave.0 <= max_x {
                    let y_intersect = side.function_value_approx(concave.0);
                    let curr_dist = (y_intersect - concave.1).abs();
                    let projection_ok = curr_dist < min_projection_dist
                        && (search_up && y_intersect > concave.1 && concave.0 >= after_approx.0
                            || search_down && y_intersect < concave.1 && concave.0 <= after_approx.0);
                    if projection_ok {
                        min_projection_dist = curr_dist;
                        corner_no_after_min_projection = corner_no_after_curr_projection;
                        min_projection = Some((concave.0, y_intersect));
                    }
                }
            }
            corner_before_curr_projection = corner_after_curr_projection;
            before_approx = after_approx;
            corner_no_after_curr_projection = if corner_no_after_curr_projection == n - 1 { 0 } else { corner_no_after_curr_projection + 1 };
        }
        min_projection.map(|p| (p, corner_no_after_min_projection))
    }
}

/// The corners of `Polygon(Point[])`: repeats dropped, then the first
/// corner lying straight between its neighbours, over again until there is
/// neither. Neither test wraps round from the last corner to the first.
pub(crate) fn polygon_corners(points: &[IntPoint]) -> Vec<IntPoint> {
    let mut corners = points.to_vec();
    loop {
        let count = corners.len();
        corners.dedup();
        let mut removed = corners.len() != count;
        for k in 1..corners.len().saturating_sub(1) {
            if corners[k].side_of(corners[k - 1], corners[k + 1]) == Side::Collinear {
                corners.remove(k);
                removed = true;
                break;
            }
        }
        if !removed {
            return corners;
        }
    }
}

/// How many times the closed polygon winds round, counter-clockwise
/// positive: its turning angles summed, in turns, rounded.
/// `Polygon.winding_number_after_closing`.
fn winding_number_after_closing(corners: &[IntPoint]) -> i64 {
    if corners.len() < 2 {
        return 0;
    }
    let side = |from: IntPoint, to: IntPoint| (to.x - from.x, to.y - from.y);
    let first_side = side(corners[0], corners[1]);
    let mut prev_side = first_side;
    let mut corner_count = corners.len();
    if corners[0] == corners[corner_count - 1] {
        corner_count -= 1;
    }
    let mut angle_sum = 0.0;
    for i in 1..=corner_count {
        let next_side = if i == corner_count - 1 {
            side(corners[i], corners[0])
        } else if i == corner_count {
            first_side
        } else {
            side(corners[i], corners[i + 1])
        };
        angle_sum += angle_approx(prev_side, next_side);
        prev_side = next_side;
    }
    java_round(angle_sum / (2.0 * std::f64::consts::PI))
}

/// The signed angle from `v` to `w`, counter-clockwise positive.
/// `Vector.angle_approx(Vector)`. Java's `acos` may differ from Rust's in
/// the last place; the winding number rounds that away.
fn angle_approx(v: (i64, i64), w: (i64, i64)) -> f64 {
    let (vx, vy, wx, wy) = (v.0 as f64, v.1 as f64, w.0 as f64, w.1 as f64);
    let cos = (vx * wx + vy * wy) / ((vx * vx + vy * vy).sqrt() * (wx * wx + wy * wy).sqrt());
    let angle = cos.acos();
    // `v.side_of(w)`, FreeRouting's left being clockwise.
    if Side::of(wx * vy - wy * vx) == Side::Left {
        -angle
    } else {
        angle
    }
}

/// The border of an area, or one of its holes: a polygon, or a shape
/// already convex. FreeRouting's `PolylineShape`, as areas use it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolylineShape {
    Polygon(PolygonShape),
    Tile(TileShape),
}

impl PolylineShape {
    pub fn dimension(&self) -> i32 {
        match self {
            PolylineShape::Polygon(p) => p.dimension(),
            PolylineShape::Tile(t) => t.dimension(),
        }
    }

    /// `split_to_convex`: a convex shape is its own one piece.
    pub fn split_to_convex(&self) -> Option<Vec<TileShape>> {
        match self {
            PolylineShape::Polygon(p) => p.split_to_convex(),
            PolylineShape::Tile(t) => Some(vec![t.clone()]),
        }
    }
}

/// An area with holes. FreeRouting's `PolylineArea`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolylineArea {
    pub border: PolylineShape,
    pub holes: Vec<PolylineShape>,
}

impl PolylineArea {
    /// The area in convex pieces: the border's pieces, each convex piece of
    /// each hole cut out of all of them in turn, keeping what has area. A
    /// hole without area is passed over. `None` where a split fails, as
    /// the Java's null, or a hole's piece has no area to cut, where the
    /// Java crashes. `PolylineArea.split_to_convex`.
    pub fn split_to_convex(&self) -> Option<Vec<TileShape>> {
        let mut pieces = self.border.split_to_convex()?;
        for hole in &self.holes {
            if hole.dimension() < 2 {
                continue;
            }
            for hole_piece in hole.split_to_convex()? {
                let mut cut = Vec::new();
                for piece in &pieces {
                    cut.extend(piece.cutout(&hole_piece)?.into_iter().filter(|p| p.dimension() == 2));
                }
                pieces = cut;
            }
        }
        Some(pieces)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(xy: &[(i64, i64)]) -> Vec<IntPoint> {
        xy.iter().map(|&(x, y)| IntPoint::new(x, y)).collect()
    }

    /// `new java.util.Random(99)`'s draws, as Java 25 gives them: the
    /// first few, then a hash of 100,000 over bounds that include powers of
    /// two, which take the generator's other path.
    #[test]
    fn the_generator_draws_as_java_does() {
        let mut r = JavaRandom::new(99);
        let draws: Vec<i32> = [10, 7, 16, 1000, 3, 5].iter().map(|&b| r.next_int(b)).collect();
        assert_eq!(draws, [7, 6, 5, 11, 0, 2]);
        let mut r = JavaRandom::new(99);
        let mut h: i64 = 0;
        for i in 0..100_000 {
            let bound = 1 + (i * 7919) % 5000;
            h = h.wrapping_mul(31).wrapping_add(r.next_int(bound) as i64);
        }
        assert_eq!(h, -4400220974867812177);
    }

    /// A square given clockwise, repeated, closed and with a corner in the
    /// middle of a side comes out counter-clockwise from its lowest corner.
    #[test]
    fn a_polygon_is_cleaned_turned_and_started_low() {
        let p = PolygonShape::new(&points(&[(0, 10), (10, 10), (10, 10), (10, 0), (5, 0), (0, 0), (0, 10)]));
        assert_eq!(p.corners, points(&[(0, 0), (10, 0), (10, 10), (0, 10)]));
    }

    /// An L cut once at its concave corner: two boxes covering it.
    #[test]
    fn an_l_splits_into_two_boxes() {
        let l = PolygonShape::new(&points(&[(0, 0), (20, 0), (20, 10), (10, 10), (10, 20), (0, 20)]));
        let pieces = l.split_to_convex().unwrap();
        assert_eq!(pieces.len(), 2, "{pieces:?}");
        let area: i64 = pieces
            .iter()
            .map(|p| match p {
                TileShape::Box(b) => (b.ur.x - b.ll.x) * (b.ur.y - b.ll.y),
                other => panic!("expected boxes, got {other:?}"),
            })
            .sum();
        assert_eq!(area, 300);
    }

    #[test]
    fn a_convex_polygon_is_one_piece() {
        let tri = PolygonShape::new(&points(&[(0, 0), (30, 0), (0, 30)]));
        let pieces = tri.split_to_convex().unwrap();
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].dimension(), 2);
    }

    /// A square hole in a square area leaves a ring of pieces around it.
    #[test]
    fn a_hole_is_cut_out() {
        let square = |lo: i64, hi: i64| PolylineShape::Polygon(PolygonShape::new(&points(&[(lo, lo), (hi, lo), (hi, hi), (lo, hi)])));
        let area = PolylineArea { border: square(0, 30), holes: vec![square(10, 20)] };
        let pieces = area.split_to_convex().unwrap();
        let total: i64 = pieces
            .iter()
            .map(|p| {
                let b = p.bounding_box();
                assert!(matches!(p, TileShape::Box(_)), "{p:?}");
                (b.ur.x - b.ll.x) * (b.ur.y - b.ll.y)
            })
            .sum();
        assert_eq!(total, 900 - 100, "{pieces:?}");
    }
}
