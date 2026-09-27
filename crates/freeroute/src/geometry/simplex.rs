//! `Simplex`, ported from FreeRouting: a convex polygon kept as the
//! half-planes of its border lines, sorted counter-clockwise. How
//! FreeRouting stores pads that are neither round nor boxes, and every
//! widened trace segment.
//!
//! A line bounds the polygon on its FreeRouting-right side (see
//! [`Side`]): the inside of the line from `a` to `b` is where `(b - a) x
//! (p - a) > 0`, to the left looking along it with y up.

use super::line::{Line, Point, Side};
use super::{IntBox, IntOctagon, CRIT};

/// A convex polygon: what lies inside every border line, the lines in
/// counter-clockwise order. No lines means empty. FreeRouting's `Simplex`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Simplex {
    pub lines: Vec<Line>,
}

impl Simplex {
    /// Lines taken as they are, already sorted and without redundancy.
    pub fn new(lines: Vec<Line>) -> Self {
        Simplex { lines }
    }

    pub fn empty() -> Self {
        Simplex { lines: Vec::new() }
    }

    /// The polygon inside all of `lines`: sorted by direction, redundant
    /// lines dropped. `Simplex.get_instance`.
    pub fn from_lines(lines: &[Line]) -> Simplex {
        if lines.is_empty() {
            return Simplex::empty();
        }
        let mut sorted = lines.to_vec();
        sorted.sort_by(|p, q| p.compare(q));
        Simplex { lines: sorted }.remove_redundant_lines()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    fn prev(&self, i: usize) -> usize {
        if i == 0 {
            self.lines.len() - 1
        } else {
            i - 1
        }
    }

    /// Corner `i`, exactly: where border line `i` meets the one before it.
    /// `Simplex.corner`.
    pub fn corner(&self, i: usize) -> Point {
        self.lines[i].intersection(&self.lines[self.prev(i)])
    }

    /// Corner `i` in floating point. `Simplex.corner_approx`.
    pub fn corner_approx(&self, i: usize) -> (f64, f64) {
        self.lines[i].intersection_approx(&self.lines[self.prev(i)])
    }

    /// Whether line `i` turns left from the line before it, so that the two
    /// meet in a corner of the polygon. `Simplex.corner_is_bounded`.
    pub fn corner_is_bounded(&self, i: usize) -> bool {
        if self.lines.len() == 1 {
            return false;
        }
        let (p, c) = (self.lines[self.prev(i)].direction(), self.lines[i].direction());
        p.x as i128 * c.y as i128 - p.y as i128 * c.x as i128 > 0
    }

    /// Bounded, with only horizontal and vertical sides. `Simplex.is_IntBox`.
    pub fn is_int_box(&self) -> bool {
        (0..self.lines.len()).all(|i| self.lines[i].is_orthogonal() && self.corner_is_bounded(i))
    }

    /// Bounded, with sides only at multiples of 45 degrees.
    /// `Simplex.is_IntOctagon`.
    pub fn is_int_octagon(&self) -> bool {
        (0..self.lines.len()).all(|i| self.lines[i].is_multiple_of_45_degree() && self.corner_is_bounded(i))
    }

    /// The octagon with these sides, for a polygon that is one; normalized.
    /// `Simplex.to_IntOctagon`.
    pub fn to_int_octagon(&self) -> Option<IntOctagon> {
        if !self.is_int_octagon() {
            return None;
        }
        if self.is_empty() {
            return Some(IntOctagon::EMPTY);
        }
        let (mut rx, mut uy, mut lrx, mut urx) = (CRIT, CRIT, CRIT, CRIT);
        let (mut lx, mut ly, mut llx, mut ulx) = (-CRIT, -CRIT, -CRIT, -CRIT);
        for l in &self.lines {
            let (a, b) = (l.a, l.b);
            if a.y == b.y {
                if b.x >= a.x {
                    ly = a.y;
                }
                if b.x <= a.x {
                    uy = a.y;
                }
            }
            if a.x == b.x {
                if b.y >= a.y {
                    rx = a.x;
                }
                if b.y <= a.y {
                    lx = a.x;
                }
            }
            if a.y < b.y {
                if a.x < b.x {
                    lrx = a.x - a.y;
                } else if a.x > b.x {
                    urx = a.x + a.y;
                }
            } else if a.y > b.y {
                if a.x < b.x {
                    llx = a.x + a.y;
                } else if a.x > b.x {
                    ulx = a.x - a.y;
                }
            }
        }
        Some(IntOctagon::new(lx, ly, rx, uy, ulx, lrx, llx, urx).normalize())
    }

    /// An octagon, box or empty shape where the polygon is one, else the
    /// polygon itself. `Simplex.simplify`.
    pub fn simplify(self) -> super::TileShape {
        use super::TileShape;
        if self.is_empty() {
            TileShape::Simplex(Simplex::empty())
        } else if self.is_int_box() {
            TileShape::Box(self.bounding_box())
        } else if self.is_int_octagon() {
            TileShape::Octagon(self.to_int_octagon().expect("checked"))
        } else {
            TileShape::Simplex(self)
        }
    }

    /// The polygon inside both: this one's lines then `other`'s, sorted and
    /// reduced. The order matters where lines tie in direction.
    /// `Simplex.intersection(Simplex)`.
    pub fn intersection(&self, other: &Simplex) -> Simplex {
        if self.is_empty() || other.is_empty() {
            return Simplex::empty();
        }
        let mut lines = self.lines.clone();
        lines.extend_from_slice(&other.lines);
        lines.sort_by(|p, q| p.compare(q));
        Simplex { lines }.remove_redundant_lines()
    }

    /// -1 empty, 0 a point, 1 a segment, 2 an area. `Simplex.dimension`.
    pub fn dimension(&self) -> i32 {
        let l = &self.lines;
        match l.len() {
            0 => -1,
            1 => 2,
            2 => {
                if l[0].is_equal_or_opposite(&l[1]) {
                    1
                } else {
                    2
                }
            }
            3 => {
                if l[0].is_equal_or_opposite(&l[1]) || l[0].is_equal_or_opposite(&l[2]) || l[1].is_equal_or_opposite(&l[2]) {
                    return 1;
                }
                match l[0].side_of(&l[1].intersection(&l[2])) {
                    Side::Right => 2,
                    Side::Left => -1,
                    Side::Collinear => 0,
                }
            }
            4 => {
                let (c02, c13) = (l[0].is_equal_or_opposite(&l[2]), l[1].is_equal_or_opposite(&l[3]));
                if c02 && c13 {
                    0
                } else if c02 || c13 {
                    1
                } else {
                    2
                }
            }
            _ => 2,
        }
    }

    /// The corners' extremes, rounded outwards; the Java's empty box, the
    /// critical bound inverted, for an empty polygon. `Simplex.bounding_box`.
    pub fn bounding_box(&self) -> IntBox {
        if self.is_empty() {
            return IntBox::new(CRIT, CRIT, -CRIT, -CRIT);
        }
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
    /// null. Empty for an empty polygon. `Simplex.bounding_octagon`.
    pub fn bounding_octagon(&self) -> Option<IntOctagon> {
        if self.is_empty() {
            return Some(IntOctagon::EMPTY);
        }
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

    /// Lines already in counter-clockwise order, reduced but not sorted, as
    /// `IntOctagon.to_Simplex` builds its eight sides.
    pub(crate) fn from_sorted_lines(lines: Vec<Line>) -> Simplex {
        Simplex { lines }.remove_redundant_lines()
    }

    /// Drop repeated lines and lines that bound no corner; empty if the
    /// half-planes leave nothing. `Simplex.remove_redundant_lines`, loop
    /// for loop: which line goes first changes which remain.
    fn remove_redundant_lines(self) -> Simplex {
        let arr = &self.lines;
        if arr.is_empty() {
            return self;
        }
        let mut line_arr = Vec::with_capacity(arr.len());
        line_arr.push(arr[0]);
        let mut prev = arr[0];
        for l in &arr[1..] {
            if !l.fast_equals(&prev) {
                line_arr.push(*l);
                prev = *l;
            }
        }
        let mut new_length = line_arr.len();
        let mut sides: Vec<Option<Side>> = vec![None; new_length];
        let mut try_again = new_length > 2;
        let mut index_of_last_removed_line = new_length as i64;
        while try_again {
            try_again = false;
            let mut prev_ind = new_length - 1;
            let mut prev_line = line_arr[prev_ind];
            let mut curr_line = line_arr[0];
            let mut ind: i64 = 0;
            while ind < new_length as i64 {
                let i = ind as usize;
                let mut next_ind = if i == new_length - 1 { 0 } else { i + 1 };
                let next_line = line_arr[next_ind];
                let mut remove_line = false;
                let (prev_dir, next_dir) = (prev_line.direction(), next_line.direction());
                let det = prev_dir.determinant(&next_dir);
                if det != 0.0 {
                    if sides[i].is_none() {
                        sides[i] = Some(curr_line.side_of_intersection(&prev_line, &next_line));
                    }
                    if det > 0.0 {
                        remove_line = sides[i] != Some(Side::Left);
                    } else if sides[i] == Some(Side::Left) && prev_dir.determinant(&curr_line.direction()) > 0.0 {
                        new_length = 0;
                        break;
                    }
                } else if prev_line.side_of(&Point::Int(next_line.a)) == Side::Left {
                    new_length = 0;
                    break;
                }
                if remove_line {
                    try_again = true;
                    new_length -= 1;
                    for k in i..new_length {
                        line_arr[k] = line_arr[k + 1];
                        sides[k] = sides[k + 1];
                    }
                    if new_length < 3 {
                        try_again = false;
                        break;
                    }
                    if i == 0 {
                        prev_ind = new_length - 1;
                    }
                    sides[prev_ind] = None;
                    next_ind = if i >= new_length { 0 } else { i };
                    sides[next_ind] = None;
                    ind -= 1;
                    index_of_last_removed_line = ind;
                } else {
                    prev_line = curr_line;
                    prev_ind = i;
                }
                curr_line = next_line;
                if !try_again && ind >= index_of_last_removed_line {
                    break;
                }
                ind += 1;
            }
            if new_length == 0 {
                try_again = false;
            }
        }
        if new_length == 2 && line_arr[0].is_parallel(&line_arr[1]) {
            let first_a = Point::Int(line_arr[0].a);
            if line_arr[0].direction().same_as(&line_arr[1].direction()) {
                if line_arr[1].side_of(&first_a) == Side::Left {
                    line_arr[0] = line_arr[1];
                }
                new_length -= 1;
            } else if line_arr[1].side_of(&first_a) == Side::Left {
                new_length = 0;
            }
        }
        if new_length == self.lines.len() {
            return self;
        }
        if new_length == 0 {
            return Simplex::empty();
        }
        line_arr.truncate(new_length);
        Simplex { lines: line_arr }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{IntPoint, TileShape};
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
        assert_eq!(s.corner(1), Point::Int(IntPoint::new(10, 0)));
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

    /// Sorting and reduction: given in any order, with a duplicate and a
    /// line that bounds nothing, a square comes out as its four sides.
    #[test]
    fn redundant_lines_are_dropped() {
        let p = IntPoint::new;
        let sides = [
            Line::new(p(10, 10), p(0, 10)),
            Line::new(p(0, 0), p(10, 0)),
            Line::new(p(20, 0), p(30, 0)), // the bottom side again
            Line::new(p(10, 0), p(10, 10)),
            Line::new(p(-50, 50), p(-60, 40)), // x - y >= -100: far outside
            Line::new(p(0, 10), p(0, 0)),
        ];
        let s = Simplex::from_lines(&sides);
        assert_eq!(s.lines.len(), 4, "{s:?}");
        assert_eq!(s.simplify(), TileShape::Box(IntBox::new(0, 0, 10, 10)));
    }

    /// Half-planes that leave nothing make the empty polygon.
    #[test]
    fn disjoint_half_planes_are_empty() {
        let p = IntPoint::new;
        // y >= 10 and y <= 0.
        let s = Simplex::from_lines(&[Line::new(p(0, 10), p(1, 10)), Line::new(p(1, 0), p(0, 0))]);
        assert!(s.is_empty(), "{s:?}");
    }

    /// Cutting an octagon's corner with a diagonal keeps it an octagon.
    #[test]
    fn a_box_cut_by_a_diagonal_is_an_octagon() {
        let p = IntPoint::new;
        let box_sides = polygon(&[(0, 0), (10, 0), (10, 10), (0, 10)]);
        // Keep x + y <= 15: the line from (15, 0) up-left to (0, 15).
        let cut = Simplex::from_lines(&[Line::new(p(15, 0), p(0, 15))]);
        let s = box_sides.intersection(&cut);
        let o = match s.simplify() {
            TileShape::Octagon(o) => o,
            other => panic!("expected an octagon, got {other:?}"),
        };
        assert_eq!(o.upper_right_diag_x, 15);
        assert_eq!(o.bounding_box(), IntBox::new(0, 0, 10, 10));
    }
}
