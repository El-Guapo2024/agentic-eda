//! Clearance rules, ported from FreeRouting's `ClearanceMatrix`.
//!
//! Every item has a clearance class, and the matrix gives the gap required
//! between items of any two classes, layer by layer. The router does not
//! check gaps while it searches: it grows every obstacle by the gap up
//! front, once per trace clearance class, and then treats a trace as its
//! centre line plus half its own class's clearance to itself (see
//! [`crate::board::clearance_offset`]).

/// `ClearanceMatrix.clearance_safety_margin`: added to every clearance the
/// board's own checks look up.
pub const CLEARANCE_SAFETY_MARGIN: i64 = 16;

/// Clearances between classes, per layer. Class 0 is FreeRouting's null
/// class, which keeps no clearance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClearanceMatrix {
    classes: usize,
    layers: usize,
    values: Vec<i64>,
}

impl ClearanceMatrix {
    /// All clearances zero.
    pub fn new(classes: usize, layers: usize) -> Self {
        ClearanceMatrix { classes, layers, values: vec![0; classes * classes * layers] }
    }

    pub fn classes(&self) -> usize {
        self.classes
    }

    pub fn layers(&self) -> usize {
        self.layers
    }

    fn index(&self, i: usize, j: usize, layer: usize) -> usize {
        (j * self.classes + i) * self.layers + layer
    }

    /// Set the entry for classes `i` and `j` on `layer`. One direction only,
    /// as the Java stores row `j`, column `i`; set both for a symmetric rule.
    pub fn set(&mut self, i: usize, j: usize, layer: usize, value: i64) {
        let k = self.index(i, j, layer);
        self.values[k] = value;
    }

    /// The clearance between classes `i` and `j` on `layer`; 0 for a class
    /// or layer outside the matrix. `ClearanceMatrix.get_value`, without its
    /// safety margin.
    pub fn get(&self, i: i32, j: i32, layer: i32) -> i64 {
        let inside = |v: i32, n: usize| v >= 0 && (v as usize) < n;
        if !(inside(i, self.classes) && inside(j, self.classes) && inside(layer, self.layers)) {
            return 0;
        }
        self.values[self.index(i as usize, j as usize, layer as usize)]
    }

    /// `get_value(i, j, layer, true)`: with the safety margin, but 0 out
    /// of range.
    pub fn get_with_margin(&self, i: i32, j: i32, layer: i32) -> i64 {
        let inside = |v: i32, n: usize| v >= 0 && (v as usize) < n;
        if !(inside(i, self.classes) && inside(j, self.classes) && inside(layer, self.layers)) {
            return 0;
        }
        self.values[self.index(i as usize, j as usize, layer as usize)] + CLEARANCE_SAFETY_MARGIN
    }

    /// Half of `class`'s clearance to itself, rounded up: the share a trace
    /// of that class carries itself, so obstacles need grow only by the
    /// rest. `ClearanceMatrix.clearance_compensation_value`.
    pub fn compensation(&self, class: i32, layer: i32) -> i64 {
        (self.get(class, class, layer) + 1) / 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outside_the_matrix_is_no_clearance() {
        let mut m = ClearanceMatrix::new(3, 2);
        m.set(1, 2, 1, 250);
        assert_eq!(m.get(1, 2, 1), 250);
        assert_eq!(m.get(2, 1, 1), 0, "entries are one-directional");
        assert_eq!(m.get(-1, 2, 1), 0);
        assert_eq!(m.get(1, 3, 1), 0);
        assert_eq!(m.get(1, 2, 2), 0);
    }

    #[test]
    fn a_trace_carries_half_its_own_clearance_rounded_up() {
        let mut m = ClearanceMatrix::new(2, 1);
        m.set(1, 1, 0, 201);
        assert_eq!(m.compensation(1, 0), 101);
        m.set(1, 1, 0, 200);
        assert_eq!(m.compensation(1, 0), 100);
    }
}
