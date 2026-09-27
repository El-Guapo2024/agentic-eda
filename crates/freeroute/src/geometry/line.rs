//! Lines, directions and points, ported from FreeRouting's `Line`,
//! `IntDirection`, `IntVector`, `IntPoint` and `RationalPoint`: what
//! polylines and convex shapes are built from.
//!
//! FreeRouting names sides of a line by its own convention: a point is
//! `Left` of the line from `a` to `b` when `(b - a) x (p - a) < 0` -- to the
//! right, looking along the line with y up. Every side test here computes
//! exactly the Java's determinant and names the result as the Java does;
//! translating the names would only invite mistakes.
//!
//! Where two lines meet off the integer grid, the point is rational. The
//! Java keeps those in `BigInteger`; lines through points within the
//! critical bound give numerators under 2^80 and side tests under 2^110,
//! so 128-bit integers suffice.

use std::cmp::Ordering;

use super::{IntPoint, CRIT};

/// Which side of a line. FreeRouting's `Side`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
    Collinear,
}

impl Side {
    /// `Side.of`: left for a positive value, right for a negative one.
    pub fn of(value: f64) -> Side {
        if value > 0.0 {
            Side::Left
        } else if value < 0.0 {
            Side::Right
        } else {
            Side::Collinear
        }
    }

    fn of_exact(value: i128) -> Side {
        match value.cmp(&0) {
            Ordering::Greater => Side::Left,
            Ordering::Less => Side::Right,
            Ordering::Equal => Side::Collinear,
        }
    }

    pub fn negate(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
            Side::Collinear => Side::Collinear,
        }
    }
}

/// `Math.round` as Java defines it: halves round up, towards positive
/// infinity, where Rust's `round` goes away from zero.
pub(crate) fn java_round(v: f64) -> i64 {
    let f = v.floor();
    if v - f >= 0.5 {
        f as i64 + 1
    } else {
        f as i64
    }
}

/// A direction as integer steps with no common factor. FreeRouting's
/// `IntDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Direction {
    pub x: i64,
    pub y: i64,
}

impl Direction {
    pub const RIGHT: Direction = Direction { x: 1, y: 0 };
    pub const UP: Direction = Direction { x: 0, y: 1 };
    pub const LEFT: Direction = Direction { x: -1, y: 0 };
    pub const DOWN: Direction = Direction { x: 0, y: -1 };

    /// The direction of the step `(dx, dy)`, divided by the greatest common
    /// divisor of its components. `IntVector.to_normalized_direction`.
    pub fn of(dx: i64, dy: i64) -> Direction {
        let g = gcd(dx.abs(), dy.abs());
        if g > 1 {
            Direction { x: dx / g, y: dy / g }
        } else {
            Direction { x: dx, y: dy }
        }
    }

    /// `IntDirection.determinant`, in floating point as the Java computes it.
    pub fn determinant(&self, other: &Direction) -> f64 {
        self.x as f64 * other.y as f64 - self.y as f64 * other.x as f64
    }

    /// Which side of this direction `other` turns to. `Direction.side_of`,
    /// through `IntVector.side_of`, whose double dispatch negates the
    /// determinant twice over.
    pub fn side_of(&self, other: &Direction) -> Side {
        Side::of(other.x as f64 * self.y as f64 - other.y as f64 * self.x as f64)
    }

    /// Parallel and pointing the same way. `Direction.equals`.
    pub fn same_as(&self, other: &Direction) -> bool {
        self.side_of(other) == Side::Collinear && self.x as f64 * other.x as f64 + self.y as f64 * other.y as f64 > 0.0
    }

    pub fn is_orthogonal(&self) -> bool {
        self.x == 0 || self.y == 0
    }

    pub fn is_multiple_of_45_degree(&self) -> bool {
        self.is_orthogonal() || self.x.abs() == self.y.abs()
    }
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// A point with rational coordinates `x / z`, `y / z`, `z > 0`: where two
/// lines meet off the integer grid. FreeRouting's `RationalPoint`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RationalPoint {
    pub x: i128,
    pub y: i128,
    pub z: i128,
}

/// A point on the integer grid, or rational. FreeRouting's `Point`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Point {
    Int(IntPoint),
    Rational(RationalPoint),
}

impl Point {
    /// `Point.to_float`. A rational point at infinity (`z == 0`) comes out
    /// at `f32::MAX`, as in the Java.
    pub fn to_float(&self) -> (f64, f64) {
        match *self {
            Point::Int(p) => (p.x as f64, p.y as f64),
            Point::Rational(r) => {
                if r.z == 0 {
                    (f32::MAX as f64, f32::MAX as f64)
                } else {
                    (r.x as f64 / r.z as f64, r.y as f64 / r.z as f64)
                }
            }
        }
    }
}

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

    /// The line through `a` in direction `dir`. `Line.get_instance(Point,
    /// Direction)`.
    pub fn through(a: IntPoint, dir: Direction) -> Line {
        Line::new(a, IntPoint::new(a.x + dir.x, a.y + dir.y))
    }

    pub fn direction(&self) -> Direction {
        Direction::of(self.b.x - self.a.x, self.b.y - self.a.y)
    }

    pub fn opposite(&self) -> Line {
        Line::new(self.b, self.a)
    }

    /// Which side of the line a floating point is on, within `tolerance`.
    /// `Line.side_of(FloatPoint, double)`.
    pub fn side_of_float(&self, (x, y): (f64, f64), tolerance: f64) -> Side {
        let (a, b) = (self.a, self.b);
        let det = (b.y - a.y) as f64 * (x - a.x as f64) - (b.x - a.x) as f64 * (y - a.y as f64);
        if det - tolerance > 0.0 {
            Side::Left
        } else if det + tolerance < 0.0 {
            Side::Right
        } else {
            Side::Collinear
        }
    }

    /// Which side of the line a point is on, exactly: the same determinant
    /// as [`side_of_float`](Self::side_of_float). `Line.side_of(Point)`.
    pub fn side_of(&self, p: &Point) -> Side {
        let (a, b) = (self.a, self.b);
        let (dx, dy) = ((b.x - a.x) as i128, (b.y - a.y) as i128);
        match *p {
            Point::Int(p) => Side::of_exact(dy * (p.x - a.x) as i128 - dx * (p.y - a.y) as i128),
            Point::Rational(r) => Side::of_exact(dy * (r.x - a.x as i128 * r.z) - dx * (r.y - a.y as i128 * r.z)),
        }
    }

    /// Which side of this line `p_1` and `p_2` meet: by floating point
    /// where that is clear by more than a unit, exactly otherwise.
    /// `Line.side_of_intersection`.
    pub fn side_of_intersection(&self, p_1: &Line, p_2: &Line) -> Side {
        let side = self.side_of_float(p_1.intersection_approx(p_2), 1.0);
        if side == Side::Collinear {
            return self.side_of(&p_1.intersection(p_2));
        }
        side
    }

    /// Where this line meets `other`, exactly: an integer point where the
    /// quotient divides and stays within the critical bound, a rational one
    /// otherwise -- at infinity (`z == 0`) for parallel lines.
    /// `Line.intersection`, including its shortcuts for axis and diagonal
    /// lines, which never leave the integer grid.
    pub fn intersection(&self, other: &Line) -> Point {
        let (d1x, d1y) = (self.b.x - self.a.x, self.b.y - self.a.y);
        let (d2x, d2y) = (other.b.x - other.a.x, other.b.y - other.a.y);
        let (a, oa) = (self.a, other.a);
        let int = |x: i64, y: i64| Point::Int(IntPoint::new(x, y));
        if d1x == 0 {
            if d2y == 0 {
                return int(a.x, oa.y);
            }
            if d2x == d2y {
                return int(a.x, oa.y + a.x - oa.x);
            }
            if d2x == -d2y {
                return int(a.x, oa.y + oa.x - a.x);
            }
        } else if d1y == 0 {
            if d2x == 0 {
                return int(oa.x, a.y);
            }
            if d2x == d2y {
                return int(oa.x + a.y - oa.y, a.y);
            }
            if d2x == -d2y {
                return int(oa.x + oa.y - a.y, a.y);
            }
        } else if d1x == d1y {
            if d2x == 0 {
                return int(oa.x, a.y + oa.x - a.x);
            }
            if d2y == 0 {
                return int(a.x + oa.y - a.y, oa.y);
            }
        } else if d1x == -d1y {
            if d2x == 0 {
                return int(oa.x, a.y + a.x - oa.x);
            }
            if d2y == 0 {
                return int(a.x + a.y - oa.y, oa.y);
            }
        }
        let cross = |p: IntPoint, q: IntPoint| p.x as i128 * q.y as i128 - p.y as i128 * q.x as i128;
        let det_1 = cross(self.a, self.b);
        let det_2 = cross(other.a, other.b);
        let mut det = d2x as i128 * d1y as i128 - d2y as i128 * d1x as i128;
        let mut is_x = det_1 * d2x as i128 - det_2 * d1x as i128;
        let mut is_y = det_1 * d2y as i128 - det_2 * d1y as i128;
        if det != 0 {
            if det < 0 {
                (det, is_x, is_y) = (-det, -is_x, -is_y);
            }
            if is_x.rem_euclid(det) == 0 && is_y.rem_euclid(det) == 0 {
                (is_x, is_y) = (is_x / det, is_y / det);
                if (is_x as f64).abs() <= CRIT as f64 && (is_y as f64).abs() <= CRIT as f64 {
                    return int(is_x as i64, is_y as i64);
                }
                det = 1;
            }
        }
        Point::Rational(RationalPoint { x: is_x, y: is_y, z: det })
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

    /// The parallel line `dist` away: its start point moves along whichever
    /// axis the line is steeper against, rounded to the grid, and the line
    /// keeps its direction. `Line.translate`.
    pub fn translate(&self, dist: f64) -> Line {
        let dir = self.direction();
        let (vxvx, vyvy) = (dir.x as f64 * dir.x as f64, dir.y as f64 * dir.y as f64);
        let length = (vxvx + vyvy).sqrt();
        let a = if vxvx <= vyvy {
            let rel_x = java_round((dist * length) / dir.y as f64);
            IntPoint::new(self.a.x - rel_x, self.a.y)
        } else {
            let rel_y = java_round((dist * length) / dir.x as f64);
            IntPoint::new(self.a.x, self.a.y + rel_y)
        };
        Line::through(a, dir)
    }

    pub fn is_parallel(&self, other: &Line) -> bool {
        self.direction().side_of(&other.direction()) == Side::Collinear
    }

    /// Both of `other`'s points lie on this line. `Line.is_equal_or_opposite`,
    /// and `Line.overlaps`, which is the same test.
    pub fn is_equal_or_opposite(&self, other: &Line) -> bool {
        self.side_of(&Point::Int(other.a)) == Side::Collinear && self.side_of(&Point::Int(other.b)) == Side::Collinear
    }

    /// The same line, same way round: `other.a` lies on it and the
    /// directions agree. `Line.fast_equals`.
    pub fn fast_equals(&self, other: &Line) -> bool {
        let (a, b, oa) = (self.a, self.b, other.a);
        let det = (oa.x - a.x) as f64 * (b.y - a.y) as f64 - (b.x - a.x) as f64 * (oa.y - a.y) as f64;
        det == 0.0 && self.direction().same_as(&other.direction())
    }

    /// Order by direction, counter-clockwise from the positive x axis.
    /// `Line.compareTo`, which sorts the lines of a convex shape.
    pub fn compare(&self, other: &Line) -> Ordering {
        let (dx1, dy1) = (self.b.x - self.a.x, self.b.y - self.a.y);
        let (dx2, dy2) = (other.b.x - other.a.x, other.b.y - other.a.y);
        let less = Ordering::Less;
        let greater = Ordering::Greater;
        if dy1 > 0 {
            if dy2 < 0 {
                return less;
            }
            if dy2 == 0 {
                return if dx2 > 0 { greater } else { less };
            }
        } else if dy1 < 0 {
            if dy2 >= 0 {
                return greater;
            }
        } else {
            if dx1 > 0 {
                return if dy2 != 0 || dx2 < 0 { less } else { Ordering::Equal };
            }
            if dy2 > 0 || dy2 == 0 && dx2 > 0 {
                return greater;
            }
            if dy2 < 0 {
                return less;
            }
            return Ordering::Equal;
        }
        let determinant = dx2 as f64 * dy1 as f64 - dy2 as f64 * dx1 as f64;
        determinant.partial_cmp(&0.0).unwrap_or(Ordering::Equal)
    }

    pub fn is_orthogonal(&self) -> bool {
        self.direction().is_orthogonal()
    }

    pub fn is_multiple_of_45_degree(&self) -> bool {
        self.direction().is_multiple_of_45_degree()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(ax: i64, ay: i64, bx: i64, by: i64) -> Line {
        Line::new(IntPoint::new(ax, ay), IntPoint::new(bx, by))
    }

    #[test]
    fn java_rounds_halves_up() {
        assert_eq!(java_round(2.5), 3);
        assert_eq!(java_round(-2.5), -2);
        assert_eq!(java_round(-2.6), -3);
        assert_eq!(java_round(0.49999999999999994), 0);
    }

    #[test]
    fn directions_are_reduced() {
        assert_eq!(Direction::of(6, -4), Direction { x: 3, y: -2 });
        assert_eq!(Direction::of(0, -7), Direction::DOWN);
    }

    /// FreeRouting's left is what lies to the right of the line, y up.
    #[test]
    fn sides_follow_freerouting() {
        let l = line(0, 0, 10, 0);
        assert_eq!(l.side_of(&Point::Int(IntPoint::new(5, -3))), Side::Left);
        assert_eq!(l.side_of(&Point::Int(IntPoint::new(5, 3))), Side::Right);
        assert_eq!(l.side_of_float((5.0, -3.0), 0.0), Side::Left);
        assert_eq!(l.side_of(&Point::Int(IntPoint::new(-5, 0))), Side::Collinear);
    }

    /// Axis and diagonal lines meet on the grid; two diagonals can meet
    /// half-way between grid points, which is rational.
    #[test]
    fn intersections_on_and_off_the_grid() {
        assert_eq!(line(3, 0, 3, 1).intersection(&line(0, 5, 1, 6)), Point::Int(IntPoint::new(3, 8)));
        let p = line(0, 0, 1, 1).intersection(&line(0, 1, 1, 0));
        assert_eq!(p.to_float(), (0.5, 0.5));
        assert!(matches!(p, Point::Rational(RationalPoint { z: 2, .. }) | Point::Rational(RationalPoint { z: -2, .. })));
        // The general case, where the quotient divides.
        assert_eq!(line(0, 0, 2, 1).intersection(&line(0, 3, 3, 0)), Point::Int(IntPoint::new(2, 1)));
    }

    #[test]
    fn exact_and_float_intersections_agree() {
        let (l1, l2) = (line(0, 0, 3, 7), line(10, -2, -4, 5));
        let (x, y) = l1.intersection(&l2).to_float();
        let (fx, fy) = l1.intersection_approx(&l2);
        assert!((x - fx).abs() < 1e-9 && (y - fy).abs() < 1e-9);
    }

    /// Translating moves the line by the distance, whatever its slope.
    #[test]
    fn translate_moves_by_the_distance() {
        let l = line(0, 0, 10, 0).translate(-3.0);
        assert_eq!(l.a.y, -3);
        let d = line(0, 0, 1, 1).translate(10.0);
        // A diagonal moved 10 units: its offset along the x axis is 10 sqrt 2.
        assert_eq!(d.a, IntPoint::new(-14, 0));
    }

    #[test]
    fn lines_sort_counter_clockwise_from_the_x_axis() {
        let mut ls = [line(0, 0, 0, -1), line(0, 0, -1, 0), line(0, 0, 1, 1), line(0, 0, 1, 0), line(0, 0, 0, 1)];
        ls.sort_by(|p, q| p.compare(q));
        let dirs: Vec<(i64, i64)> = ls.iter().map(|l| (l.b.x, l.b.y)).collect();
        assert_eq!(dirs, [(1, 0), (1, 1), (0, 1), (-1, 0), (0, -1)]);
    }
}
