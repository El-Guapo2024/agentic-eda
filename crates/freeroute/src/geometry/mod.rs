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

    /// Tiled with sections no wider than `max_width`, the last row and
    /// column absorbing the remainder. `IntBox.divide_into_sections`.
    pub fn divide_into_sections(&self, max_width: f64) -> Vec<IntBox> {
        let b = *self;
        if max_width <= 0.0 {
            return Vec::new();
        }
        let len = (b.ur.x - b.ll.x) as f64;
        let hgt = (b.ur.y - b.ll.y) as f64;
        let xc = (len / max_width).ceil() as i64;
        let yc = (hgt / max_width).ceil() as i64;
        if xc <= 0 || yc <= 0 {
            return vec![b];
        }
        let sx = (len / xc as f64).ceil() as i64;
        let sy = (hgt / yc as f64).ceil() as i64;
        let mut out = Vec::new();
        for j in 0..yc {
            let ly = b.ll.y + j * sy;
            let uy = if j == yc - 1 { b.ur.y } else { ly + sy };
            for i in 0..xc {
                let lx = b.ll.x + i * sx;
                let ux = if i == xc - 1 { b.ur.x } else { lx + sx };
                out.push(IntBox::new(lx, ly, ux, uy));
            }
        }
        out
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
