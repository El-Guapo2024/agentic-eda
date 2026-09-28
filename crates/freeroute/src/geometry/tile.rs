//! `TileShape`: a convex shape in whichever of FreeRouting's three forms
//! it takes -- an axis-aligned box, an octagon with sides at multiples of
//! 45 degrees, or a general convex polygon -- and the intersections
//! between them.
//!
//! The Java picks each intersection by double dispatch, and the form it
//! lands in decides which shape's lines come first when two polygons are
//! combined. That order breaks ties when lines are sorted, so it is
//! followed case by case.

use super::line::{Direction, Line};
use super::{IntBox, IntOctagon, IntPoint, Simplex, CRIT};

/// A convex shape in one of FreeRouting's three forms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileShape {
    Box(IntBox),
    Octagon(IntOctagon),
    Simplex(Simplex),
}

impl TileShape {
    /// The polygon inside all of `lines`, in its simplest form.
    /// `TileShape.get_instance(Line[])`.
    pub fn from_lines(lines: &[Line]) -> TileShape {
        Simplex::from_lines(lines).simplify()
    }

    pub fn is_empty(&self) -> bool {
        match self {
            TileShape::Box(b) => b.ll.x > b.ur.x || b.ll.y > b.ur.y,
            TileShape::Octagon(o) => o.is_empty(),
            TileShape::Simplex(s) => s.is_empty(),
        }
    }

    /// `bounding_octagon`, per form; `None` for a polygon beyond the
    /// critical bound.
    pub fn bounding_octagon(&self) -> Option<IntOctagon> {
        match self {
            TileShape::Box(b) => Some(b.to_octagon()),
            TileShape::Octagon(o) => Some(*o),
            TileShape::Simplex(s) => s.bounding_octagon(),
        }
    }

    /// An octagon that is a box becomes the box; a polygon, whatever it
    /// simplifies to. `simplify`.
    pub fn simplify(self) -> TileShape {
        match self {
            TileShape::Octagon(o) if o.is_int_box() => TileShape::Box(o.bounding_box()),
            TileShape::Simplex(s) => s.simplify(),
            other => other,
        }
    }

    /// The shape inside both, in the form the Java's `intersection` gives.
    /// Every form's `intersection(TileShape)` hands itself to the other
    /// shape, so the case is chosen by `other`, then by `self`.
    pub fn intersection(&self, other: &TileShape) -> TileShape {
        match self {
            TileShape::Box(b) => other.meet_box(b),
            TileShape::Octagon(o) => other.meet_octagon(o),
            TileShape::Simplex(s) => other.meet_simplex(s),
        }
    }

    /// Whether this shape and octagon `o` share a point, touching included.
    /// A polygon asks whether its intersection with the octagon's sides is
    /// empty; a box tests as its octagon. `intersects`, through the Java's
    /// double dispatch.
    pub fn intersects_octagon(&self, o: &IntOctagon) -> bool {
        match self {
            TileShape::Box(b) => o.intersects(&b.to_octagon()),
            TileShape::Octagon(own) => o.intersects(own),
            TileShape::Simplex(s) => !s.intersection(&o.to_simplex()).is_empty(),
        }
    }

    /// `intersection` then `simplify`. `TileShape.intersection_with_simplify`.
    pub fn intersection_with_simplify(&self, other: &TileShape) -> TileShape {
        self.intersection(other).simplify()
    }

    /// `self.intersection(IntBox)`.
    fn meet_box(&self, b: &IntBox) -> TileShape {
        match self {
            TileShape::Box(own) => TileShape::Box(own.intersection(b)),
            TileShape::Octagon(own) => TileShape::Octagon(own.intersection(&b.to_octagon())),
            TileShape::Simplex(own) => TileShape::Simplex(own.intersection(&b.to_simplex())),
        }
    }

    /// `self.intersection(IntOctagon)`.
    fn meet_octagon(&self, o: &IntOctagon) -> TileShape {
        match self {
            TileShape::Box(own) => TileShape::Octagon(o.intersection(&own.to_octagon())),
            TileShape::Octagon(own) => TileShape::Octagon(own.intersection(o)),
            TileShape::Simplex(own) => TileShape::Simplex(own.intersection(&o.to_simplex())),
        }
    }

    /// `self.intersection(Simplex)`: the polygon's lines come first, except
    /// between two polygons, where `self`'s do.
    fn meet_simplex(&self, s: &Simplex) -> TileShape {
        match self {
            TileShape::Box(own) => TileShape::Simplex(s.intersection(&own.to_simplex())),
            TileShape::Octagon(own) => TileShape::Simplex(s.intersection(&own.to_simplex())),
            TileShape::Simplex(own) => TileShape::Simplex(own.intersection(s)),
        }
    }
}

impl IntBox {
    /// The box shared by both; the Java's empty box, the critical bound
    /// inverted, if they are apart. `IntBox.intersection(IntBox)`.
    pub fn intersection(&self, other: &IntBox) -> IntBox {
        if other.ll.x > self.ur.x || other.ll.y > self.ur.y || self.ll.x > other.ur.x || self.ll.y > other.ur.y {
            return IntBox::new(CRIT, CRIT, -CRIT, -CRIT);
        }
        IntBox::new(self.ll.x.max(other.ll.x), self.ll.y.max(other.ll.y), self.ur.x.min(other.ur.x), self.ur.y.min(other.ur.y))
    }

    /// Its four sides as a polygon, not reduced. `IntBox.to_Simplex`.
    pub fn to_simplex(&self) -> Simplex {
        if self.ll.x > self.ur.x || self.ll.y > self.ur.y {
            return Simplex::empty();
        }
        Simplex::new(vec![
            Line::through(self.ll, Direction::RIGHT),
            Line::through(self.ur, Direction::UP),
            Line::through(self.ur, Direction::LEFT),
            Line::through(self.ll, Direction::DOWN),
        ])
    }
}

impl IntOctagon {
    /// Border line `no`, through the points the Java picks.
    /// `IntOctagon.border_line`.
    pub fn border_line(&self, no: usize) -> Line {
        let (ly, rx, uy, lx) = (self.bottom_y, self.right_x, self.top_y, self.left_x);
        let (ulx, lrx, llx, urx) = (self.upper_left_diag_x, self.lower_right_diag_x, self.lower_left_diag_x, self.upper_right_diag_x);
        let (a, b) = match no {
            0 => ((0, ly), (1, ly)),
            1 => ((lrx, 0), (lrx + 1, 1)),
            2 => ((rx, 0), (rx, 1)),
            3 => ((urx, 0), (urx - 1, 1)),
            4 => ((0, uy), (-1, uy)),
            5 => ((ulx, 0), (ulx - 1, -1)),
            6 => ((lx, 0), (lx, -1)),
            7 => ((llx, 0), (llx + 1, -1)),
            _ => panic!("IntOctagon::border_line: {no} out of range 0..8"),
        };
        Line::new(IntPoint::new(a.0, a.1), IntPoint::new(b.0, b.1))
    }

    /// Its eight sides as a polygon, redundant ones dropped.
    /// `IntOctagon.to_Simplex`.
    pub fn to_simplex(&self) -> Simplex {
        if self.is_empty() {
            return Simplex::empty();
        }
        Simplex::from_sorted_lines((0..8).map(|i| self.border_line(i)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An octagon's sides, as a polygon and simplified, give it back.
    #[test]
    fn an_octagon_round_trips_through_its_sides() {
        let o = IntOctagon::new(0, 0, 100, 100, -70, 70, 30, 170);
        assert_eq!(o.to_simplex().simplify(), TileShape::Octagon(o));
        let b = IntBox::new(-5, -5, 20, 10);
        assert_eq!(b.to_octagon().to_simplex().simplify(), TileShape::Box(b));
    }

    /// The same intersection through every pair of forms: a box and an
    /// octagon overlapping, each also given as a polygon.
    #[test]
    fn intersections_agree_across_forms() {
        let b = IntBox::new(0, 0, 100, 60);
        let o = IntOctagon::new(40, 20, 160, 120, -60, 120, 80, 260);
        let want = o.intersection(&b.to_octagon());
        let forms_b = [TileShape::Box(b), TileShape::Simplex(b.to_simplex())];
        let forms_o = [TileShape::Octagon(o), TileShape::Simplex(o.to_simplex())];
        for x in &forms_b {
            for y in &forms_o {
                for (p, q) in [(x, y), (y, x)] {
                    let got = p.intersection_with_simplify(q).bounding_octagon().unwrap();
                    assert_eq!(got, want, "{p:?} with {q:?}");
                }
            }
        }
    }

    #[test]
    fn disjoint_boxes_meet_in_the_empty_box() {
        let e = IntBox::new(0, 0, 10, 10).intersection(&IntBox::new(20, 0, 30, 10));
        assert!(TileShape::Box(e).is_empty());
    }
}
