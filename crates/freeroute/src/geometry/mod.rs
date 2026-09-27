//! Planar geometry kernel, ported from `app.freerouting.geometry.planar`.
//!
//! Coordinates are `i64`. FreeRouting uses Java `int` and guards overflow
//! with a critical bound of 2^25; widening to 64 bits keeps every
//! arithmetic identity the Java relies on while removing the overflow
//! cliff for boards measured in micrometres.

mod circle;
mod octagon;
mod simplex;

pub use circle::Circle;
pub(crate) use octagon::CRIT;
pub use octagon::IntOctagon;
pub use simplex::{Line, Simplex};

/// An integer point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IntPoint {
    pub x: i64,
    pub y: i64,
}

impl IntPoint {
    pub const fn new(x: i64, y: i64) -> Self {
        IntPoint { x, y }
    }
}

/// An axis-aligned integer box, lower-left and upper-right inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IntBox {
    pub ll: IntPoint,
    pub ur: IntPoint,
}

impl IntBox {
    pub const fn new(llx: i64, lly: i64, urx: i64, ury: i64) -> Self {
        IntBox { ll: IntPoint::new(llx, lly), ur: IntPoint::new(urx, ury) }
    }

    /// Grown by `dist` on every side, corners kept square; unchanged for a
    /// zero distance or an empty box. `IntBox.offset`, for the integer
    /// distances the search tree uses.
    pub fn offset(&self, dist: i64) -> IntBox {
        if dist == 0 || self.ll.x > self.ur.x || self.ll.y > self.ur.y {
            return *self;
        }
        IntBox::new(self.ll.x - dist, self.ll.y - dist, self.ur.x + dist, self.ur.y + dist)
    }

    /// The same region as an octagon whose diagonals do not cut it.
    pub fn to_octagon(&self) -> IntOctagon {
        IntOctagon::new(
            self.ll.x,
            self.ll.y,
            self.ur.x,
            self.ur.y,
            self.ll.x - self.ur.y,
            self.ur.x - self.ll.y,
            self.ll.x + self.ll.y,
            self.ur.x + self.ur.y,
        )
    }
}
