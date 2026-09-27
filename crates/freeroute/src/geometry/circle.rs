//! `Circle`, ported from FreeRouting's `Circle.java`. Most pads and all
//! vias are round, and the router only ever sees a circle through its
//! bounding box or octagon, so that is all that is ported.

use super::{IntBox, IntOctagon, IntPoint};

/// A circle with an integer centre and radius.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Circle {
    pub center: IntPoint,
    pub radius: i64,
}

impl Circle {
    pub const fn new(center: IntPoint, radius: i64) -> Self {
        Circle { center, radius }
    }

    pub fn bounding_box(&self) -> IntBox {
        let (c, r) = (self.center, self.radius);
        IntBox::new(c.x - r, c.y - r, c.x + r, c.y + r)
    }

    /// `Circle.bounding_octagon`, rounding included. Each diagonal cut sits
    /// (sqrt 2 - 1) r from a box corner, floored for the upper-left and
    /// lower-left diagonals and ceiled for the other two, so those two can
    /// clip the circle by under a unit. Kept, as these are the shapes the
    /// router sees.
    pub fn bounding_octagon(&self) -> IntOctagon {
        let (cx, cy, r) = (self.center.x, self.center.y, self.radius);
        let (lx, rx, ly, uy) = (cx - r, cx + r, cy - r, cy + r);
        let corner = (std::f64::consts::SQRT_2 - 1.0) * r as f64;
        let (ceil, floor) = (corner.ceil() as i64, corner.floor() as i64);
        IntOctagon::new(lx, ly, rx, uy, lx - (cy + floor), rx - (cy - ceil), lx + (cy - floor), rx + (cy + ceil))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every integer point of the circle is inside the octagon, give or take
    /// the unit two of its diagonals are rounded in by; and the octagon's
    /// straight sides are the circle's box.
    #[test]
    fn the_octagon_holds_the_circle() {
        for r in 0..80 {
            let c = Circle::new(IntPoint::new(7, -3), r);
            let o = c.bounding_octagon();
            assert_eq!(o.bounding_box(), c.bounding_box());
            let loose = o.offset(1.0);
            for x in -r..=r {
                for y in -r..=r {
                    if x * x + y * y <= r * r {
                        let (px, py) = (7 + x, -3 + y);
                        assert!(loose.contains_point(px as f64, py as f64), "r {r}: ({px}, {py}) outside {o:?}");
                    }
                }
            }
        }
    }
}
