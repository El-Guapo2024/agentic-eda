//! `Line` and `Simplex`, ported from FreeRouting. A simplex is a convex
//! polygon kept as the half-planes of its border lines, which is how
//! FreeRouting stores pads that are neither round nor axis-aligned boxes.
//! Only what the search tree needs is ported so far: corners, and the
//! bounding box and octagon.

use super::{IntBox, IntOctagon, IntPoint, CRIT};

/// The directed line through two integer points. FreeRouting's `Line`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Line {
    pub a: IntPoint,
    pub b: IntPoint,
}

impl Line {
    pub const fn new(a: IntPoint, b: IntPoint) -> Self {
        Line { a, b }
    }

    /// Where this line meets `other`, in floating point; `(i32::MAX,
    /// i32::MAX)` for parallel lines. `Line.intersection_approx`, operation
    /// for operation: bounding shapes round these values, so the rounding
    /// has to come out the same.
    pub fn intersection_approx(&self, other: &Line) -> (f64, f64) {
        let (a, b, oa, ob) = (self.a, self.b, other.a, other.b);
        let d1x = (b.x - a.x) as f64;
        let d1y = (b.y - a.y) as f64;
        let d2x = (ob.x - oa.x) as f64;
        let d2y = (ob.y - oa.y) as f64;
        let det_1 = a.x as f64 * b.y as f64 - a.y as f64 * b.x as f64;
        let det_2 = oa.x as f64 * ob.y as f64 - oa.y as f64 * ob.x as f64;
        let det = d2x * d1y - d2y * d1x;
        if det == 0.0 {
            return (i32::MAX as f64, i32::MAX as f64);
        }
        ((d2x * det_1 - d1x * det_2) / det, (d2y * det_1 - d1y * det_2) / det)
    }
}

/// A convex polygon: what lies on the inner side of every border line,
/// the lines in counter-clockwise order. FreeRouting's `Simplex`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Simplex {
    pub lines: Vec<Line>,
}

impl Simplex {
    pub fn new(lines: Vec<Line>) -> Self {
        assert!(!lines.is_empty(), "Simplex::new: no border lines");
        Simplex { lines }
    }

    /// Corner `i`: where border line `i` meets the one before it, in
    /// floating point. `Simplex.corner_approx`.
    pub fn corner_approx(&self, i: usize) -> (f64, f64) {
        let prev = if i == 0 { self.lines.len() - 1 } else { i - 1 };
        self.lines[i].intersection_approx(&self.lines[prev])
    }

    /// The corners' extremes, rounded outwards. `Simplex.bounding_box`.
    pub fn bounding_box(&self) -> IntBox {
        let (mut llx, mut lly) = (i32::MAX as f64, i32::MAX as f64);
        let (mut urx, mut ury) = (i32::MIN as f64, i32::MIN as f64);
        for i in 0..self.lines.len() {
            let (x, y) = self.corner_approx(i);
            llx = llx.min(x);
            lly = lly.min(y);
            urx = urx.max(x);
            ury = ury.max(y);
        }
        IntBox::new(llx.floor() as i64, lly.floor() as i64, urx.ceil() as i64, ury.ceil() as i64)
    }

    /// The corners' extremes in all eight directions, rounded outwards;
    /// `None` if any lies beyond the critical bound, where the Java returns
    /// null. `Simplex.bounding_octagon`.
    pub fn bounding_octagon(&self) -> Option<IntOctagon> {
        let (mut lx, mut ly, mut rx, mut uy) = (i32::MAX as f64, i32::MAX as f64, i32::MIN as f64, i32::MIN as f64);
        let (mut ulx, mut lrx, mut llx, mut urx) = (i32::MAX as f64, i32::MIN as f64, i32::MAX as f64, i32::MIN as f64);
        for i in 0..self.lines.len() {
            let (x, y) = self.corner_approx(i);
            lx = lx.min(x);
            ly = ly.min(y);
            rx = rx.max(x);
            uy = uy.max(y);
            ulx = ulx.min(x - y);
            lrx = lrx.max(x - y);
            llx = llx.min(x + y);
            urx = urx.max(x + y);
        }
        let crit = CRIT as f64;
        if lx.min(ly) < -crit || rx.max(uy) > crit || ulx.min(llx) < -crit || lrx.max(urx) > crit {
            return None;
        }
        Some(IntOctagon::new(
            lx.floor() as i64,
            ly.floor() as i64,
            rx.ceil() as i64,
            uy.ceil() as i64,
            ulx.floor() as i64,
            lrx.ceil() as i64,
            llx.floor() as i64,
            urx.ceil() as i64,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn polygon(pts: &[(i64, i64)]) -> Simplex {
        let p: Vec<IntPoint> = pts.iter().map(|&(x, y)| IntPoint::new(x, y)).collect();
        Simplex::new((0..p.len()).map(|i| Line::new(p[i], p[(i + 1) % p.len()])).collect())
    }

    #[test]
    fn corners_of_a_square_given_by_its_sides() {
        let s = polygon(&[(0, 0), (10, 0), (10, 10), (0, 10)]);
        // Corner i is where side i meets side i - 1: the start of side i.
        assert_eq!(s.corner_approx(0), (0.0, 0.0));
        assert_eq!(s.corner_approx(1), (10.0, 0.0));
        assert_eq!(s.bounding_box(), IntBox::new(0, 0, 10, 10));
        assert_eq!(s.bounding_octagon(), Some(IntBox::new(0, 0, 10, 10).to_octagon()));
    }

    /// A square turned 45 degrees is its own octagon: its diagonals are the
    /// octagon's diagonal bounds.
    #[test]
    fn a_diamond_bounds_itself() {
        let s = polygon(&[(5, 0), (10, 5), (5, 10), (0, 5)]);
        let o = s.bounding_octagon().unwrap();
        assert_eq!((o.upper_left_diag_x, o.lower_right_diag_x, o.lower_left_diag_x, o.upper_right_diag_x), (-5, 5, 5, 15));
        assert_eq!(o.bounding_box(), IntBox::new(0, 0, 10, 10));
    }

    /// Corners at fractional coordinates round outwards, never in.
    #[test]
    fn fractional_corners_round_outwards() {
        // A triangle whose slanted sides meet at (5, 2.5).
        let p = IntPoint::new;
        let s = Simplex::new(vec![Line::new(p(0, 0), p(10, 0)), Line::new(p(10, 0), p(6, 2)), Line::new(p(4, 2), p(0, 0))]);
        assert_eq!(s.corner_approx(2), (5.0, 2.5));
        let b = s.bounding_box();
        assert_eq!((b.ll.y, b.ur.y), (0, 3), "apex y 2.5 rounds up to 3: {b:?}");
    }

    /// The octagon is the tightest with integer bounds around the corners:
    /// each corner is inside, and pulling any bound in by a unit would leave
    /// one out. On random triangles, whose corners are mostly fractional --
    /// unlike the pads on FreeRouting's example boards, where rounding the
    /// wrong way would go unseen.
    #[test]
    fn the_octagon_is_the_tightest_around_the_corners() {
        let mut seed: u64 = 0x5EED;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % 201) as i64 - 100
        };
        let mut checked = 0;
        for _ in 0..2_000 {
            let mut line = || Line::new(IntPoint::new(next(), next()), IntPoint::new(next(), next()));
            let s = Simplex::new(vec![line(), line(), line()]);
            let corners: Vec<(f64, f64)> = (0..3).map(|i| s.corner_approx(i)).collect();
            if corners.iter().any(|c| c.0.abs() > 1e6 || c.1.abs() > 1e6) {
                continue; // (near-)parallel sides
            }
            let o = s.bounding_octagon().unwrap();
            let lower = [o.left_x, o.bottom_y, o.upper_left_diag_x, o.lower_left_diag_x];
            let upper = [o.right_x, o.top_y, o.lower_right_diag_x, o.upper_right_diag_x];
            for k in 0..4 {
                let v: Vec<f64> = corners.iter().map(|&(x, y)| [x, y, x - y, x + y][k]).collect();
                let (min, max) = (v.iter().cloned().fold(f64::MAX, f64::min), v.iter().cloned().fold(f64::MIN, f64::max));
                assert!(lower[k] as f64 <= min && min < (lower[k] + 1) as f64, "lower bound {k}: {} for min {min}", lower[k]);
                assert!(upper[k] as f64 >= max && max > (upper[k] - 1) as f64, "upper bound {k}: {} for max {max}", upper[k]);
            }
            checked += 1;
        }
        assert!(checked > 1_000, "only {checked} triangles");
    }

    #[test]
    fn a_polygon_beyond_the_critical_bound_has_no_octagon() {
        let far = CRIT + 10;
        assert_eq!(polygon(&[(0, 0), (far, 0), (far, 10), (0, 10)]).bounding_octagon(), None);
    }
}
