//! `IntOctagon`, ported from FreeRouting's `IntOctagon.java`.
//!
//! The workhorse shape of the whole router: free space, obstacles and
//! clearance regions are all kept as octagons whose edges run at 0, 45 and
//! 90 degrees. An octagon is the set of integer points satisfying eight
//! inequalities:
//!
//! ```text
//!   left_x              <= x     <= right_x
//!   bottom_y            <= y     <= top_y
//!   upper_left_diag_x   <= x - y <= lower_right_diag_x
//!   lower_left_diag_x   <= x + y <= upper_right_diag_x
//! ```
//!
//! The diagonal bounds are named, as in the Java, by where each border line
//! crosses the x axis. That reading is what the tests use as an oracle: any
//! operation can be checked by counting the integer points that satisfy the
//! inequalities, with no Java runtime involved.
//!
//! One deliberate difference: Java marks the empty octagon by *identity*
//! (`this == EMPTY`). Rust compares values, so emptiness is a flag here --
//! otherwise an octagon that merely happened to carry the same numbers as
//! the sentinel would be mistaken for it.

use super::line::java_round;
use super::{IntBox, IntPoint};

/// FreeRouting's `Limits.CRIT_INT`, the bound its empty sentinel uses, and
/// the bound a room edge is pushed out to when it is dropped.
pub(crate) const CRIT: i64 = 33_554_432;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IntOctagon {
    pub left_x: i64,
    pub bottom_y: i64,
    pub right_x: i64,
    pub top_y: i64,
    /// Lower bound of `x - y`.
    pub upper_left_diag_x: i64,
    /// Upper bound of `x - y`.
    pub lower_right_diag_x: i64,
    /// Lower bound of `x + y`.
    pub lower_left_diag_x: i64,
    /// Upper bound of `x + y`.
    pub upper_right_diag_x: i64,
    empty: bool,
}

impl IntOctagon {
    /// The empty octagon. Its bounds are Java's sentinel values, kept so a
    /// ported caller that reads them sees what FreeRouting would.
    pub const EMPTY: IntOctagon = IntOctagon {
        left_x: CRIT,
        bottom_y: CRIT,
        right_x: -CRIT,
        top_y: -CRIT,
        upper_left_diag_x: CRIT,
        lower_right_diag_x: -CRIT,
        lower_left_diag_x: CRIT,
        upper_right_diag_x: -CRIT,
        empty: true,
    };

    /// Argument order follows the Java constructor exactly, so a ported
    /// call site can be copied without reordering.
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        left_x: i64,
        bottom_y: i64,
        right_x: i64,
        top_y: i64,
        upper_left_diag_x: i64,
        lower_right_diag_x: i64,
        lower_left_diag_x: i64,
        upper_right_diag_x: i64,
    ) -> Self {
        IntOctagon {
            left_x,
            bottom_y,
            right_x,
            top_y,
            upper_left_diag_x,
            lower_right_diag_x,
            lower_left_diag_x,
            upper_right_diag_x,
            empty: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.empty
    }

    /// -1 empty, 0 a point, 1 a segment, 2 an area.
    pub fn dimension(&self) -> i32 {
        if self.empty {
            return -1;
        }
        if self.right_x > self.left_x
            && self.top_y > self.bottom_y
            && self.lower_right_diag_x > self.upper_left_diag_x
            && self.upper_right_diag_x > self.lower_left_diag_x
        {
            2
        } else if self.right_x == self.left_x && self.top_y == self.bottom_y {
            0
        } else {
            1
        }
    }

    pub fn bounding_box(&self) -> IntBox {
        IntBox::new(self.left_x, self.bottom_y, self.right_x, self.top_y)
    }

    /// Corner `no` in 0..8, counter-clockwise from the lower-left end of
    /// the bottom edge. Only meaningful on a normalized octagon.
    pub fn corner(&self, no: usize) -> IntPoint {
        let (x, y) = match no {
            0 => (self.lower_left_diag_x - self.bottom_y, self.bottom_y),
            1 => (self.lower_right_diag_x + self.bottom_y, self.bottom_y),
            2 => (self.right_x, self.right_x - self.lower_right_diag_x),
            3 => (self.right_x, self.upper_right_diag_x - self.right_x),
            4 => (self.upper_right_diag_x - self.top_y, self.top_y),
            5 => (self.upper_left_diag_x + self.top_y, self.top_y),
            6 => (self.left_x, self.left_x - self.upper_left_diag_x),
            7 => (self.left_x, self.lower_left_diag_x - self.left_x),
            _ => panic!("IntOctagon::corner: {no} out of range 0..8"),
        };
        IntPoint::new(x, y)
    }

    /// Area by the shoelace formula over the eight corners, exactly as the
    /// Java expands it (to avoid allocating points).
    pub fn area(&self) -> f64 {
        let (lx, by, rx, ty) = (self.left_x as f64, self.bottom_y as f64, self.right_x as f64, self.top_y as f64);
        let (ulx, lrx, llx, urx) = (
            self.upper_left_diag_x as f64,
            self.lower_right_diag_x as f64,
            self.lower_left_diag_x as f64,
            self.upper_right_diag_x as f64,
        );
        let mut r = (llx - by) * (by - llx + lx);
        r += (lrx + by) * (rx - lrx - by);
        r += rx * (urx - 2.0 * rx - by + ty + lrx);
        r += (urx - ty) * (ty - urx + rx);
        r += (ulx + ty) * (lx - ulx - ty);
        r += lx * (llx - 2.0 * lx - ty + by + ulx);
        0.5 * r.abs()
    }

    pub fn translate_by(&self, dx: i64, dy: i64) -> IntOctagon {
        if self.empty || (dx == 0 && dy == 0) {
            return *self;
        }
        IntOctagon::new(
            self.left_x + dx,
            self.bottom_y + dy,
            self.right_x + dx,
            self.top_y + dy,
            self.upper_left_diag_x + dx - dy,
            self.lower_right_diag_x + dx - dy,
            self.lower_left_diag_x + dx + dy,
            self.upper_right_diag_x + dx + dy,
        )
    }

    /// Grow (or shrink, for negative `distance`) by a clearance. Diagonal
    /// bounds move by `sqrt(2) * distance` because they are measured along
    /// the x axis, not perpendicular to the edge.
    pub fn offset(&self, distance: f64) -> IntOctagon {
        let width = java_round(distance);
        if width == 0 || self.empty {
            return *self;
        }
        let dia = java_round(std::f64::consts::SQRT_2 * distance);
        IntOctagon::new(
            self.left_x - width,
            self.bottom_y - width,
            self.right_x + width,
            self.top_y + width,
            self.upper_left_diag_x - dia,
            self.lower_right_diag_x + dia,
            self.lower_left_diag_x - dia,
            self.upper_right_diag_x + dia,
        )
        .normalize()
    }

    /// Contains a real point. Like the Java, inexact right at the border
    /// because the point is floating.
    pub fn contains_point(&self, x: f64, y: f64) -> bool {
        if self.empty {
            return false;
        }
        if (self.left_x as f64) > x || (self.bottom_y as f64) > y || (self.right_x as f64) < x || (self.top_y as f64) < y {
            return false;
        }
        let d = x - y;
        let s = x + y;
        (self.upper_left_diag_x as f64) <= d
            && (self.lower_right_diag_x as f64) >= d
            && (self.lower_left_diag_x as f64) <= s
            && (self.upper_right_diag_x as f64) >= s
    }

    /// The smallest octagon containing both. Not normalized, as in Java.
    pub fn union(&self, other: &IntOctagon) -> IntOctagon {
        if self.empty {
            return *other;
        }
        if other.empty {
            return *self;
        }
        IntOctagon::new(
            self.left_x.min(other.left_x),
            self.bottom_y.min(other.bottom_y),
            self.right_x.max(other.right_x),
            self.top_y.max(other.top_y),
            self.upper_left_diag_x.min(other.upper_left_diag_x),
            self.lower_right_diag_x.max(other.lower_right_diag_x),
            self.lower_left_diag_x.min(other.lower_left_diag_x),
            self.upper_right_diag_x.max(other.upper_right_diag_x),
        )
    }

    pub fn intersection(&self, other: &IntOctagon) -> IntOctagon {
        if self.empty || other.empty {
            return IntOctagon::EMPTY;
        }
        IntOctagon::new(
            self.left_x.max(other.left_x),
            self.bottom_y.max(other.bottom_y),
            self.right_x.min(other.right_x),
            self.top_y.min(other.top_y),
            self.upper_left_diag_x.max(other.upper_left_diag_x),
            self.lower_right_diag_x.min(other.lower_right_diag_x),
            self.lower_left_diag_x.max(other.lower_left_diag_x),
            self.upper_right_diag_x.min(other.upper_right_diag_x),
        )
        .normalize()
    }

    /// Tighten every bound that another pair of bounds makes redundant, or
    /// return `EMPTY` if the inequalities admit no point.
    ///
    /// Ported line for line, including the order of the tightenings: each
    /// step reads bounds an earlier step may have moved, so reordering them
    /// would give a different (still valid, but not FreeRouting's) answer
    /// and break the differential comparison against the Java.
    pub fn normalize(&self) -> IntOctagon {
        if self.empty {
            return *self;
        }
        if self.left_x > self.right_x
            || self.bottom_y > self.top_y
            || self.lower_left_diag_x > self.upper_right_diag_x
            || self.upper_left_diag_x > self.lower_right_diag_x
        {
            return IntOctagon::EMPTY;
        }
        let mut lx = self.left_x;
        let mut rx = self.right_x;
        let mut ly = self.bottom_y;
        let mut uy = self.top_y;
        let mut llx = self.lower_left_diag_x;
        let mut ulx = self.upper_left_diag_x;
        let mut lrx = self.lower_right_diag_x;
        let mut urx = self.upper_right_diag_x;

        if lx < llx - uy {
            lx = llx - uy;
        }
        if lx < ulx + ly {
            lx = ulx + ly;
        }
        if rx > urx - ly {
            rx = urx - ly;
        }
        if rx > lrx + uy {
            rx = lrx + uy;
        }
        if ly < lx - lrx {
            ly = lx - lrx;
        }
        if ly < llx - rx {
            ly = llx - rx;
        }
        if uy > urx - lx {
            uy = urx - lx;
        }
        if uy > rx - ulx {
            uy = rx - ulx;
        }
        if llx - lx < ly {
            llx = lx + ly;
        }
        if rx - lrx < ly {
            lrx = rx - ly;
        }
        if urx - rx > uy {
            urx = uy + rx;
        }
        if lx - ulx > uy {
            ulx = lx - uy;
        }
        // Java computes these with `/ 2.0` and Math.ceil/floor on a double;
        // div_euclid/-div_euclid give the same integer for every i64 in
        // range, without a round trip through floating point.
        let diag_upper_y = ceil_half(urx - ulx);
        if uy > diag_upper_y {
            uy = diag_upper_y;
        }
        let diag_lower_y = floor_half(llx - lrx);
        if ly < diag_lower_y {
            ly = diag_lower_y;
        }
        let diag_right_x = ceil_half(urx + lrx);
        if rx > diag_right_x {
            rx = diag_right_x;
        }
        let diag_left_x = floor_half(llx + ulx);
        if lx < diag_left_x {
            lx = diag_left_x;
        }
        if lx > rx || ly > uy || llx > urx || ulx > lrx {
            return IntOctagon::EMPTY;
        }
        IntOctagon::new(lx, ly, rx, uy, ulx, lrx, llx, urx)
    }

    pub fn is_normalized(&self) -> bool {
        self.normalize() == *self
    }

    pub fn is_contained_in(&self, other: &IntOctagon) -> bool {
        if self.empty {
            return true;
        }
        if other.empty {
            return false;
        }
        self.left_x >= other.left_x
            && self.bottom_y >= other.bottom_y
            && self.right_x <= other.right_x
            && self.top_y <= other.top_y
            && self.lower_left_diag_x >= other.lower_left_diag_x
            && self.upper_left_diag_x >= other.upper_left_diag_x
            && self.lower_right_diag_x <= other.lower_right_diag_x
            && self.upper_right_diag_x <= other.upper_right_diag_x
    }

    /// Whether the diagonals only touch the corners of the bounding box,
    /// so that the octagon is that box. `IntOctagon.is_IntBox`.
    pub fn is_int_box(&self) -> bool {
        self.lower_left_diag_x == self.left_x + self.bottom_y
            && self.lower_right_diag_x == self.right_x - self.bottom_y
            && self.upper_right_diag_x == self.right_x + self.top_y
            && self.upper_left_diag_x == self.left_x - self.top_y
    }

    /// Whether each of `other`'s corners, by the corner formulas, lies in
    /// this octagon: Java's `TileShape.contains(TileShape)`, which the room
    /// code calls, rather than [`is_contained_in`](Self::is_contained_in)'s
    /// comparison of bounds. They differ where two diagonals meet at
    /// half-integer coordinates: integer bounds cannot sit tight there, the
    /// formula corner lands just outside, and an octagon need not contain
    /// its own copy.
    pub fn contains_corners(&self, other: &IntOctagon) -> bool {
        (0..8).all(|i| {
            let c = other.corner(i);
            (0..8).all(|line| self.side_of_border_line(c.x, c.y, line) <= 0)
        })
    }

    /// Whether two *normalized* octagons share at least one point.
    pub fn intersects(&self, other: &IntOctagon) -> bool {
        if self.empty || other.empty {
            return false;
        }
        self.left_x.max(other.left_x) <= self.right_x.min(other.right_x)
            && self.bottom_y.max(other.bottom_y) <= self.top_y.min(other.top_y)
            && self.lower_left_diag_x.max(other.lower_left_diag_x) <= self.upper_right_diag_x.min(other.upper_right_diag_x)
            && self.upper_left_diag_x.max(other.upper_left_diag_x) <= self.lower_right_diag_x.min(other.lower_right_diag_x)
    }

    /// Whether the intersection is two-dimensional -- touching along an
    /// edge or at a corner does not count.
    pub fn overlaps(&self, other: &IntOctagon) -> bool {
        if self.empty || other.empty {
            return false;
        }
        self.left_x.max(other.left_x) < self.right_x.min(other.right_x)
            && self.bottom_y.max(other.bottom_y) < self.top_y.min(other.top_y)
            && self.lower_left_diag_x.max(other.lower_left_diag_x) < self.upper_right_diag_x.min(other.upper_right_diag_x)
            && self.upper_left_diag_x.max(other.upper_left_diag_x) < self.lower_right_diag_x.min(other.lower_right_diag_x)
    }

    /// `d` minus this octagon, as 8 convex pieces: 4 boxes and 4 octagons
    /// with a corner cut off. FreeRouting's `IntOctagon.cutoutFrom(IntBox)`.
    ///
    /// This is how free space is carved around an obstacle. Ported by a
    /// mechanical rewrite of the Java (names and syntax only, logic
    /// untouched) because it is 200 lines of near-identical blocks where a
    /// swapped field would be invisible to a reader. Pieces may be empty.
    /// If the overlap is only at the border, `d` comes back whole.
    #[allow(unused_mut, unused_assignments)]
    pub fn cutout_from_box(&self, d: IntBox) -> Vec<IntOctagon> {
    let c = self.intersection(&d.to_octagon());

    if self.is_empty() || c.dimension() < self.dimension() {
      let mut result = vec![IntOctagon::EMPTY; 1];
      result[0] = d.to_octagon();
      return result;
    }

    let mut boxes = [IntBox::new(0, 0, 0, 0); 4];

    boxes[0] =
        IntBox::new(d.ll.x, c.lower_left_diag_x - c.left_x, c.left_x, c.left_x - c.upper_left_diag_x);

    boxes[1] =
        IntBox::new(
            c.right_x, c.right_x - c.lower_right_diag_x, d.ur.x, c.upper_right_diag_x - c.right_x);

    boxes[2] =
        IntBox::new(
            c.lower_left_diag_x - c.bottom_y, d.ll.y, c.lower_right_diag_x + c.bottom_y, c.bottom_y);

    boxes[3] =
        IntBox::new(c.upper_left_diag_x + c.top_y, c.top_y, c.upper_right_diag_x - c.top_y, d.ur.y);

    let mut octagons = [IntOctagon::EMPTY; 4];

    let mut current_oct =
        IntOctagon::new(
            d.ll.x,
            boxes[0].ur.y,
            boxes[3].ll.x,
            d.ur.y,
            -CRIT,
            c.upper_left_diag_x,
            -CRIT,
            CRIT);
    octagons[0] = current_oct.normalize();

    current_oct =
        IntOctagon::new(
            d.ll.x,
            d.ll.y,
            boxes[2].ll.x,
            boxes[0].ll.y,
            -CRIT,
            CRIT,
            -CRIT,
            c.lower_left_diag_x);
    octagons[1] = current_oct.normalize();

    current_oct =
        IntOctagon::new(
            boxes[2].ur.x,
            d.ll.y,
            d.ur.x,
            boxes[1].ll.y,
            c.lower_right_diag_x,
            CRIT,
            -CRIT,
            CRIT);
    octagons[2] = current_oct.normalize();

    current_oct =
        IntOctagon::new(
            boxes[3].ur.x,
            boxes[1].ur.y,
            d.ur.x,
            d.ur.y,
            -CRIT,
            CRIT,
            c.upper_right_diag_x,
            CRIT);
    octagons[3] = current_oct.normalize();

    let mut b = boxes[0];
    let mut o = octagons[0];
    if b.ur.x - b.ll.x > o.top_y - o.bottom_y {

      boxes[0] = IntBox::new(b.ll.x, b.ll.y, b.ur.x, o.top_y);
      current_oct =
          IntOctagon::new(
              b.ur.x,
              o.bottom_y,
              o.right_x,
              o.top_y,
              o.upper_left_diag_x,
              o.lower_right_diag_x,
              o.lower_left_diag_x,
              o.upper_right_diag_x);
      octagons[0] = current_oct.normalize();
    }

    b = boxes[3];
    o = octagons[0];
    if b.ur.y - b.ll.y > o.right_x - o.left_x {

      boxes[3] = IntBox::new(o.left_x, b.ll.y, b.ur.x, b.ur.y);
      current_oct =
          IntOctagon::new(
              o.left_x,
              o.bottom_y,
              o.right_x,
              b.ll.y,
              o.upper_left_diag_x,
              o.lower_right_diag_x,
              o.lower_left_diag_x,
              o.upper_right_diag_x);
      octagons[0] = current_oct.normalize();
    }
    b = boxes[3];
    o = octagons[3];
    if b.ur.y - b.ll.y > o.right_x - o.left_x {

      boxes[3] = IntBox::new(b.ll.x, b.ll.y, o.right_x, b.ur.y);
      current_oct =
          IntOctagon::new(
              o.left_x,
              o.bottom_y,
              o.right_x,
              o.top_y,
              o.upper_left_diag_x,
              o.lower_right_diag_x,
              o.lower_left_diag_x,
              o.upper_right_diag_x);
      octagons[3] = current_oct.normalize();
    }
    b = boxes[1];
    o = octagons[3];
    if b.ur.x - b.ll.x > o.top_y - o.bottom_y {

      boxes[1] = IntBox::new(b.ll.x, b.ll.y, b.ur.x, o.top_y);
      current_oct =
          IntOctagon::new(
              o.left_x,
              o.bottom_y,
              b.ll.x,
              o.top_y,
              o.upper_left_diag_x,
              o.lower_right_diag_x,
              o.lower_left_diag_x,
              o.upper_right_diag_x);
      octagons[3] = current_oct.normalize();
    }
    b = boxes[1];
    o = octagons[2];
    if b.ur.x - b.ll.x > o.top_y - o.bottom_y {

      boxes[1] = IntBox::new(b.ll.x, o.bottom_y, b.ur.x, b.ur.y);
      current_oct =
          IntOctagon::new(
              o.left_x,
              o.bottom_y,
              b.ll.x,
              o.top_y,
              o.upper_left_diag_x,
              o.lower_right_diag_x,
              o.lower_left_diag_x,
              o.upper_right_diag_x);
      octagons[2] = current_oct.normalize();
    }
    b = boxes[2];
    o = octagons[2];
    if b.ur.y - b.ll.y > o.right_x - o.left_x {

      boxes[2] = IntBox::new(b.ll.x, b.ll.y, o.right_x, b.ur.y);
      current_oct =
          IntOctagon::new(
              o.left_x,
              b.ur.y,
              o.right_x,
              o.top_y,
              o.upper_left_diag_x,
              o.lower_right_diag_x,
              o.lower_left_diag_x,
              o.upper_right_diag_x);
      octagons[2] = current_oct.normalize();
    }
    b = boxes[2];
    o = octagons[1];
    if b.ur.y - b.ll.y > o.right_x - o.left_x {

      boxes[2] = IntBox::new(o.left_x, b.ll.y, b.ur.x, b.ur.y);
      current_oct =
          IntOctagon::new(
              o.left_x,
              b.ur.y,
              o.right_x,
              o.top_y,
              o.upper_left_diag_x,
              o.lower_right_diag_x,
              o.lower_left_diag_x,
              o.upper_right_diag_x);
      octagons[1] = current_oct.normalize();
    }
    b = boxes[0];
    o = octagons[1];
    if b.ur.x - b.ll.x > o.top_y - o.bottom_y {
      boxes[0] = IntBox::new(b.ll.x, o.bottom_y, b.ur.x, b.ur.y);
      current_oct =
          IntOctagon::new(
              b.ur.x,
              o.bottom_y,
              o.right_x,
              o.top_y,
              o.upper_left_diag_x,
              o.lower_right_diag_x,
              o.lower_left_diag_x,
              o.upper_right_diag_x);
      octagons[1] = current_oct.normalize();
    }

    let mut result = vec![IntOctagon::EMPTY; 8];

    for i in 0..4 {
      result[i] = boxes[i].to_octagon();
    }

    result[4..8].copy_from_slice(&octagons);
    return result;
    }

    /// `d` minus this octagon, as 8 convex pieces without sharp angles.
    /// FreeRouting's `IntOctagon.cutoutFrom(IntOctagon)`, rewritten the same
    /// mechanical way. Pieces may be empty.
    #[allow(unused_mut, unused_assignments)]
    pub fn cutout_from(&self, d: IntOctagon) -> Vec<IntOctagon> {
    let c = self.intersection(&d);

    if self.is_empty() || c.dimension() < self.dimension() {
      let mut result = vec![IntOctagon::EMPTY; 1];
      result[0] = d;
      return result;
    }

    let mut result = vec![IntOctagon::EMPTY; 8];

    let mut tmp = c.lower_left_diag_x - c.left_x;

    result[0] =
        IntOctagon::new(
            d.left_x,
            tmp,
            c.left_x,
            c.left_x - c.upper_left_diag_x,
            d.upper_left_diag_x,
            d.lower_right_diag_x,
            d.lower_left_diag_x,
            d.upper_right_diag_x);

    let mut tmp2 = c.lower_left_diag_x - c.bottom_y;

    result[1] =
        IntOctagon::new(
            d.left_x,
            d.bottom_y,
            tmp2,
            tmp,
            d.upper_left_diag_x,
            d.lower_right_diag_x,
            d.lower_left_diag_x,
            c.lower_left_diag_x);

    tmp = c.lower_right_diag_x + c.bottom_y;

    result[2] =
        IntOctagon::new(
            tmp2,
            d.bottom_y,
            tmp,
            c.bottom_y,
            d.upper_left_diag_x,
            d.lower_right_diag_x,
            d.lower_left_diag_x,
            d.upper_right_diag_x);

    tmp2 = c.right_x - c.lower_right_diag_x;

    result[3] =
        IntOctagon::new(
            tmp,
            d.bottom_y,
            d.right_x,
            tmp2,
            c.lower_right_diag_x,
            d.lower_right_diag_x,
            d.lower_left_diag_x,
            d.upper_right_diag_x);

    tmp = c.upper_right_diag_x - c.right_x;

    result[4] =
        IntOctagon::new(
            c.right_x,
            tmp2,
            d.right_x,
            tmp,
            d.upper_left_diag_x,
            d.lower_right_diag_x,
            d.lower_left_diag_x,
            d.upper_right_diag_x);

    tmp2 = c.upper_right_diag_x - c.top_y;

    result[5] =
        IntOctagon::new(
            tmp2,
            tmp,
            d.right_x,
            d.top_y,
            d.upper_left_diag_x,
            d.lower_right_diag_x,
            c.upper_right_diag_x,
            d.upper_right_diag_x);

    tmp = c.upper_left_diag_x + c.top_y;

    result[6] =
        IntOctagon::new(
            tmp,
            c.top_y,
            tmp2,
            d.top_y,
            d.upper_left_diag_x,
            d.lower_right_diag_x,
            d.lower_left_diag_x,
            d.upper_right_diag_x);

    tmp2 = c.left_x - c.upper_left_diag_x;

    result[7] =
        IntOctagon::new(
            d.left_x,
            tmp2,
            tmp,
            d.top_y,
            d.upper_left_diag_x,
            c.upper_left_diag_x,
            d.lower_left_diag_x,
            d.upper_right_diag_x);

    for i in 0..8 {
      result[i] = result[i].normalize();
    }

    let mut curr1 = result[0];
    let mut curr2 = result[7];

    if !(curr1.is_empty() || curr2.is_empty())
        && curr1.right_x - curr1.left_x_at(curr1.top_y)
            > curr2.upper_y_at(curr1.right_x) - curr2.bottom_y {
      curr1 =
          IntOctagon::new(
              std::cmp::min(curr1.left_x, curr2.left_x),
              curr1.bottom_y,
              curr1.right_x,
              curr2.top_y,
              curr2.upper_left_diag_x,
              curr1.lower_right_diag_x,
              curr1.lower_left_diag_x,
              curr2.upper_right_diag_x);

      curr2 =
          IntOctagon::new(
              curr1.right_x,
              curr2.bottom_y,
              curr2.right_x,
              curr2.top_y,
              curr2.upper_left_diag_x,
              curr2.lower_right_diag_x,
              curr2.lower_left_diag_x,
              curr2.upper_right_diag_x);

      result[0] = curr1.normalize();
      result[7] = curr2.normalize();
    }
    curr1 = result[7];
    curr2 = result[6];
    if !(curr1.is_empty() || curr2.is_empty())
        && curr2.upper_y_at(curr1.right_x) - curr2.bottom_y
            > curr1.right_x - curr1.left_x_at(curr2.bottom_y) {
      curr2 =
          IntOctagon::new(
              curr1.left_x,
              curr2.bottom_y,
              curr2.right_x,
              std::cmp::max(curr2.top_y, curr1.top_y),
              curr1.upper_left_diag_x,
              curr2.lower_right_diag_x,
              curr1.lower_left_diag_x,
              curr2.upper_right_diag_x);

      curr1 =
          IntOctagon::new(
              curr1.left_x,
              curr1.bottom_y,
              curr1.right_x,
              curr2.bottom_y,
              curr1.upper_left_diag_x,
              curr1.lower_right_diag_x,
              curr1.lower_left_diag_x,
              curr1.upper_right_diag_x);

      result[7] = curr1.normalize();
      result[6] = curr2.normalize();
    }
    curr1 = result[6];
    curr2 = result[5];
    if !(curr1.is_empty() || curr2.is_empty())
        && curr2.upper_y_at(curr1.right_x) - curr1.bottom_y
            > curr2.right_x_at(curr1.bottom_y) - curr2.left_x {
      curr1 =
          IntOctagon::new(
              curr1.left_x,
              curr1.bottom_y,
              curr2.right_x,
              std::cmp::max(curr2.top_y, curr1.top_y),
              curr1.upper_left_diag_x,
              curr2.lower_right_diag_x,
              curr1.lower_left_diag_x,
              curr2.upper_right_diag_x);

      curr2 =
          IntOctagon::new(
              curr2.left_x,
              curr2.bottom_y,
              curr2.right_x,
              curr1.bottom_y,
              curr2.upper_left_diag_x,
              curr2.lower_right_diag_x,
              curr2.lower_left_diag_x,
              curr2.upper_right_diag_x);

      result[6] = curr1.normalize();
      result[5] = curr2.normalize();
    }
    curr1 = result[5];
    curr2 = result[4];
    if !(curr1.is_empty() || curr2.is_empty())
        && curr2.right_x_at(curr2.top_y) - curr2.left_x
            > curr1.upper_y_at(curr2.left_x) - curr2.top_y {
      curr2 =
          IntOctagon::new(
              curr2.left_x,
              curr2.bottom_y,
              std::cmp::max(curr2.right_x, curr1.right_x),
              curr1.top_y,
              curr1.upper_left_diag_x,
              curr2.lower_right_diag_x,
              curr2.lower_left_diag_x,
              curr1.upper_right_diag_x);

      curr1 =
          IntOctagon::new(
              curr1.left_x,
              curr1.bottom_y,
              curr2.left_x,
              curr1.top_y,
              curr1.upper_left_diag_x,
              curr1.lower_right_diag_x,
              curr1.lower_left_diag_x,
              curr1.upper_right_diag_x);

      result[5] = curr1.normalize();
      result[4] = curr2.normalize();
    }
    curr1 = result[4];
    curr2 = result[3];
    if !(curr1.is_empty() || curr2.is_empty())
        && curr1.right_x_at(curr1.bottom_y) - curr1.left_x
            > curr1.bottom_y - curr2.lower_y_at(curr1.left_x) {
      curr1 =
          IntOctagon::new(
              curr1.left_x,
              curr2.bottom_y,
              std::cmp::max(curr2.right_x, curr1.right_x),
              curr1.top_y,
              curr1.upper_left_diag_x,
              curr2.lower_right_diag_x,
              curr2.lower_left_diag_x,
              curr1.upper_right_diag_x);

      curr2 =
          IntOctagon::new(
              curr2.left_x,
              curr2.bottom_y,
              curr1.left_x,
              curr2.top_y,
              curr2.upper_left_diag_x,
              curr2.lower_right_diag_x,
              curr2.lower_left_diag_x,
              curr2.upper_right_diag_x);

      result[4] = curr1.normalize();
      result[3] = curr2.normalize();
    }

    curr1 = result[3];
    curr2 = result[2];

    if !(curr1.is_empty() || curr2.is_empty())
        && curr2.top_y - curr2.lower_y_at(curr2.right_x)
            > curr1.right_x_at(curr2.top_y) - curr2.right_x {
      curr2 =
          IntOctagon::new(
              curr2.left_x,
              std::cmp::min(curr1.bottom_y, curr2.bottom_y),
              curr1.right_x,
              curr2.top_y,
              curr2.upper_left_diag_x,
              curr1.lower_right_diag_x,
              curr2.lower_left_diag_x,
              curr1.upper_right_diag_x);

      curr1 =
          IntOctagon::new(
              curr1.left_x,
              curr2.top_y,
              curr1.right_x,
              curr1.top_y,
              curr1.upper_left_diag_x,
              curr1.lower_right_diag_x,
              curr1.lower_left_diag_x,
              curr1.upper_right_diag_x);

      result[3] = curr1.normalize();
      result[2] = curr2.normalize();
    }

    curr1 = result[2];
    curr2 = result[1];

    if !(curr1.is_empty() || curr2.is_empty())
        && curr1.top_y - curr1.lower_y_at(curr1.left_x)
            > curr1.left_x - curr2.left_x_at(curr1.top_y) {
      curr1 =
          IntOctagon::new(
              curr2.left_x,
              std::cmp::min(curr1.bottom_y, curr2.bottom_y),
              curr1.right_x,
              curr1.top_y,
              curr2.upper_left_diag_x,
              curr1.lower_right_diag_x,
              curr2.lower_left_diag_x,
              curr1.upper_right_diag_x);

      curr2 =
          IntOctagon::new(
              curr2.left_x,
              curr1.top_y,
              curr2.right_x,
              curr2.top_y,
              curr2.upper_left_diag_x,
              curr2.lower_right_diag_x,
              curr2.lower_left_diag_x,
              curr2.upper_right_diag_x);

      result[2] = curr1.normalize();
      result[1] = curr2.normalize();
    }

    curr1 = result[1];
    curr2 = result[0];

    if !(curr1.is_empty() || curr2.is_empty())
        && curr2.right_x - curr2.left_x_at(curr2.bottom_y)
            > curr2.bottom_y - curr1.lower_y_at(curr2.right_x) {
      curr2 =
          IntOctagon::new(
              std::cmp::min(curr2.left_x, curr1.left_x),
              curr1.bottom_y,
              curr2.right_x,
              curr2.top_y,
              curr2.upper_left_diag_x,
              curr1.lower_right_diag_x,
              curr1.lower_left_diag_x,
              curr2.upper_right_diag_x);

      curr1 =
          IntOctagon::new(
              curr2.right_x,
              curr1.bottom_y,
              curr1.right_x,
              curr1.top_y,
              curr1.upper_left_diag_x,
              curr1.lower_right_diag_x,
              curr1.lower_left_diag_x,
              curr1.upper_right_diag_x);

      result[1] = curr1.normalize();
      result[0] = curr2.normalize();
    }

    return result;
    }

    /// Which side of border line `no` the point (x, y) lies on:
    /// negative inside, positive outside, zero on the line.
    /// FreeRouting's `IntOctagon.sideOfBorderLine`, returning the raw sign.
    pub fn side_of_border_line(&self, x: i64, y: i64, no: usize) -> i64 {
        let v = match no {
            0 => self.bottom_y - y,
            1 => x - y - self.lower_right_diag_x,
            2 => x - self.right_x,
            3 => x + y - self.upper_right_diag_x,
            4 => y - self.top_y,
            5 => self.upper_left_diag_x + y - x,
            6 => self.left_x - x,
            7 => self.lower_left_diag_x - x - y,
            _ => panic!("IntOctagon::side_of_border_line: {no} out of range 0..8"),
        };
        v.signum()
    }

    pub fn left_x_at(&self, y: i64) -> i64 {
        self.left_x.max(self.upper_left_diag_x + y).max(self.lower_left_diag_x - y)
    }

    pub fn right_x_at(&self, y: i64) -> i64 {
        self.right_x.min(self.upper_right_diag_x - y).min(self.lower_right_diag_x + y)
    }

    pub fn lower_y_at(&self, x: i64) -> i64 {
        self.bottom_y.max(self.lower_left_diag_x - x).max(x - self.lower_right_diag_x)
    }

    pub fn upper_y_at(&self, x: i64) -> i64 {
        self.top_y.min(x - self.upper_left_diag_x).min(self.upper_right_diag_x - x)
    }
}

/// `Math.ceil(v / 2.0)` for an integer `v`.
fn ceil_half(v: i64) -> i64 {
    -((-v).div_euclid(2))
}

/// `Math.floor(v / 2.0)` for an integer `v`.
fn floor_half(v: i64) -> i64 {
    v.div_euclid(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// The oracle: every integer point in a window that satisfies the eight
    /// inequalities. No Java, no reuse of the code under test.
    fn points(o: &IntOctagon) -> BTreeSet<(i64, i64)> {
        let mut out = BTreeSet::new();
        if o.is_empty() {
            return out;
        }
        for x in -WIN..=WIN {
            for y in -WIN..=WIN {
                let (d, s) = (x - y, x + y);
                if o.left_x <= x
                    && x <= o.right_x
                    && o.bottom_y <= y
                    && y <= o.top_y
                    && o.upper_left_diag_x <= d
                    && d <= o.lower_right_diag_x
                    && o.lower_left_diag_x <= s
                    && s <= o.upper_right_diag_x
                {
                    out.insert((x, y));
                }
            }
        }
        out
    }

    const WIN: i64 = 14;

    /// A deterministic spread of small octagons, including loose ones whose
    /// bounds are redundant (what `normalize` exists to tighten) and ones
    /// that are empty in disguise.
    fn samples() -> Vec<IntOctagon> {
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move |lo: i64, hi: i64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            lo + (seed % ((hi - lo + 1) as u64)) as i64
        };
        (0..600)
            .map(|_| {
                let lx = next(-6, 3);
                let rx = lx + next(0, 8);
                let by = next(-6, 3);
                let ty = by + next(0, 8);
                let ulx = next(-14, 4);
                let lrx = ulx + next(0, 16);
                let llx = next(-14, 4);
                let urx = llx + next(0, 16);
                IntOctagon::new(lx, by, rx, ty, ulx, lrx, llx, urx)
            })
            .collect()
    }

    #[test]
    fn normalize_never_changes_the_point_set() {
        for o in samples() {
            let n = o.normalize();
            assert_eq!(points(&o), points(&n), "normalize changed the points of {o:?} -> {n:?}");
        }
    }

    #[test]
    fn normalize_is_empty_exactly_when_there_are_no_points() {
        for o in samples() {
            assert_eq!(o.normalize().is_empty(), points(&o).is_empty(), "{o:?}");
        }
    }

    #[test]
    fn normalize_is_idempotent() {
        for o in samples() {
            let n = o.normalize();
            assert_eq!(n.normalize(), n, "normalizing twice moved {n:?}");
        }
    }

    /// Corners of a normalized octagon lie on the shape to within the half
    /// unit that normalize's outward rounding allows.
    ///
    /// Not "on a lattice point of the shape": where two diagonals meet at a
    /// half-integer, FreeRouting rounds the axis bound *outward* with
    /// Math.ceil / Math.floor rather than tightening it. The octagon
    /// x in [2,5], y in [-2,0], x-y in [4,7], x+y in [0,3] keeps top_y = 0
    /// though its diagonals cross at y = -0.5, so its corner 4 is (3, 0),
    /// half a unit outside. The point set is unchanged -- that is what
    /// normalize_never_changes_the_point_set pins -- so this is
    /// FreeRouting's contract, and this test holds the port to it rather
    /// than to a stricter one the Java never promised.
    #[test]
    fn normalized_corners_lie_on_the_shape_within_rounding() {
        for o in samples() {
            let n = o.normalize();
            if n.is_empty() {
                continue;
            }
            for i in 0..8 {
                let c = n.corner(i);
                let (x, y) = (c.x as f64, c.y as f64);
                let grown = n.offset(1.0);
                assert!(grown.contains_point(x, y), "corner {i} {c:?} more than a unit outside {n:?}");
                assert!(
                    x >= n.left_x as f64 && x <= n.right_x as f64 && y >= n.bottom_y as f64 && y <= n.top_y as f64,
                    "corner {i} {c:?} outside the bounding box of {n:?}"
                );
            }
        }
    }

    #[test]
    fn intersection_is_the_common_points() {
        let s = samples();
        for pair in s.chunks(2) {
            let (a, b) = (pair[0], pair[1]);
            let want: BTreeSet<_> = points(&a).intersection(&points(&b)).copied().collect();
            assert_eq!(points(&a.intersection(&b)), want, "{a:?} ∩ {b:?}");
        }
    }

    #[test]
    fn intersects_agrees_with_the_points_on_normalized_shapes() {
        let s: Vec<_> = samples().into_iter().map(|o| o.normalize()).collect();
        for pair in s.chunks(2) {
            let (a, b) = (pair[0], pair[1]);
            let share = points(&a).intersection(&points(&b)).next().is_some();
            assert_eq!(a.intersects(&b), share, "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn union_contains_both() {
        let s = samples();
        for pair in s.chunks(2) {
            let (a, b) = (pair[0].normalize(), pair[1].normalize());
            let u = a.union(&b);
            assert!(a.is_contained_in(&u) && b.is_contained_in(&u), "{a:?} ∪ {b:?} = {u:?}");
        }
    }

    #[test]
    fn translation_moves_every_point() {
        for o in samples() {
            let moved: BTreeSet<_> = points(&o).iter().map(|&(x, y)| (x + 3, y - 2)).filter(|&(x, y)| x.abs() <= WIN && y.abs() <= WIN).collect();
            let t: BTreeSet<_> = points(&o.translate_by(3, -2)).into_iter().filter(|&(x, y)| (x - 3).abs() <= WIN && (y + 2).abs() <= WIN).collect();
            assert_eq!(t, moved, "{o:?}");
        }
    }

    #[test]
    fn a_box_as_an_octagon_is_the_same_box() {
        let b = IntBox::new(-3, -2, 4, 5);
        let want: BTreeSet<_> = (-3..=4).flat_map(|x| (-2..=5).map(move |y| (x, y))).collect();
        assert_eq!(points(&b.to_octagon()), want);
    }

    #[test]
    fn area_of_a_square_and_a_diamond() {
        assert_eq!(IntBox::new(0, 0, 4, 4).to_octagon().area(), 16.0);
        // |x| + |y| <= 2: a diamond of area 8.
        let diamond = IntOctagon::new(-2, -2, 2, 2, -2, 2, -2, 2).normalize();
        assert_eq!(diamond.area(), 8.0);
    }

    #[test]
    fn offset_contains_the_original_and_its_clearance_ring() {
        for o in samples() {
            let n = o.normalize();
            if n.is_empty() {
                continue;
            }
            let g = n.offset(2.0);
            assert!(n.is_contained_in(&g), "{n:?} not inside its offset {g:?}");
            // Every point within 2 (Chebyshev along axes) of a corner is inside.
            let c = n.corner(0);
            assert!(g.contains_point((c.x - 2) as f64, c.y as f64) || g.contains_point(c.x as f64, (c.y - 2) as f64));
        }
    }

    #[test]
    fn empty_behaves_as_nothing() {
        let e = IntOctagon::EMPTY;
        let o = IntBox::new(0, 0, 3, 3).to_octagon();
        assert_eq!(e.dimension(), -1);
        assert!(e.intersection(&o).is_empty());
        assert!(!e.intersects(&o));
        assert_eq!(e.union(&o), o);
        assert!(e.is_contained_in(&o));
    }

    /// Points strictly inside `o`: interior to every one of the eight
    /// half-planes, so off every border.
    fn interior(o: &IntOctagon) -> BTreeSet<(i64, i64)> {
        points(o)
            .into_iter()
            .filter(|&(x, y)| {
                let (d, s) = (x - y, x + y);
                o.left_x < x
                    && x < o.right_x
                    && o.bottom_y < y
                    && y < o.top_y
                    && o.upper_left_diag_x < d
                    && d < o.lower_right_diag_x
                    && o.lower_left_diag_x < s
                    && s < o.upper_right_diag_x
            })
            .collect()
    }

    /// Checks the three things a cutout must be: it covers what is left of
    /// `outer`, it never reaches into the hole, and it stays inside `outer`.
    fn check_cutout(outer: &IntOctagon, hole: &IntOctagon, pieces: &[IntOctagon]) {
        let outer_pts = points(outer);
        let cut = hole.intersection(outer);
        let hole_in = interior(&cut);
        let covered: BTreeSet<_> = pieces.iter().flat_map(|p| points(p)).collect();
        for pt in outer_pts.difference(&hole_in) {
            assert!(covered.contains(pt), "{pt:?} of {outer:?} minus {hole:?} not covered by {pieces:?}");
        }
        for pt in &hole_in {
            assert!(!covered.contains(pt), "piece reaches {pt:?} inside the hole {hole:?}");
        }
        for pt in &covered {
            assert!(outer_pts.contains(pt), "piece escapes {outer:?} at {pt:?}");
        }
    }

    #[test]
    fn cutout_from_an_octagon_covers_exactly_what_is_left() {
        let s: Vec<_> = samples().into_iter().map(|o| o.normalize()).filter(|o| !o.is_empty()).collect();
        let mut checked = 0;
        for pair in s.chunks(2) {
            if pair.len() < 2 {
                continue;
            }
            let (outer, hole) = (pair[0], pair[1]);
            check_cutout(&outer, &hole, &hole.cutout_from(outer));
            checked += 1;
        }
        assert!(checked > 100, "only {checked} pairs exercised");
    }

    #[test]
    fn cutout_from_a_box_covers_exactly_what_is_left() {
        let s: Vec<_> = samples().into_iter().map(|o| o.normalize()).filter(|o| !o.is_empty()).collect();
        for pair in s.chunks(2) {
            if pair.len() < 2 {
                continue;
            }
            let bb = pair[0].bounding_box();
            let hole = pair[1];
            check_cutout(&bb.to_octagon(), &hole, &hole.cutout_from_box(bb));
        }
    }

    /// The case the whole routine exists for: an obstacle strictly inside a
    /// free-space room leaves a ring, and none of the ring is lost.
    #[test]
    fn cutout_of_a_centred_obstacle_leaves_a_ring() {
        let room = IntBox::new(-10, -10, 10, 10);
        let obstacle = IntOctagon::new(-3, -3, 3, 3, -4, 4, -4, 4).normalize();
        let pieces = obstacle.cutout_from_box(room);
        assert_eq!(pieces.len(), 8);
        check_cutout(&room.to_octagon(), &obstacle, &pieces);
    }

    /// From a FreeRouting board: the two left diagonals meet at x =
    /// 1404937.5, so the left bound cannot be tight, and the corner formulas
    /// put a corner at (1404937, -1138267), just below the lower-left
    /// diagonal. By corners the octagon does not contain its own copy; by
    /// bounds it does. The room code needs the first answer, as FreeRouting
    /// keeps such a room where comparing bounds would drop it.
    #[test]
    fn corners_can_leave_an_octagon_whose_bounds_hold_them() {
        let o = IntOctagon::new(1404937, -1150211, 1418393, -1124811, 2543204, 2568604, 266671, 293582);
        assert!(o.is_contained_in(&o));
        assert!(!o.contains_corners(&o));
        // A tight one contains itself either way, and not a larger one.
        let b = IntBox::new(0, 0, 10, 6).to_octagon();
        assert!(b.contains_corners(&b));
        assert!(!b.contains_corners(&IntBox::new(0, 0, 11, 6).to_octagon()));
        assert!(IntBox::new(0, 0, 11, 6).to_octagon().contains_corners(&b));
    }

    #[test]
    fn half_rounding_matches_java() {
        for v in -9..=9i64 {
            assert_eq!(ceil_half(v), (v as f64 / 2.0).ceil() as i64, "ceil {v}");
            assert_eq!(floor_half(v), (v as f64 / 2.0).floor() as i64, "floor {v}");
        }
    }
}
