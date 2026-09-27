//! The board as the router sees it: each item it must keep clear of, as
//! the shapes FreeRouting stores in its search tree for the trace clearance
//! class being routed. Ported from the item side of `ShapeSearchTree` and
//! `ShapeSearchTree45Degree`, item kind by item kind: pins and vias so far.
//!
//! Every tree shape is an obstacle grown by the clearance it needs from
//! that trace class, so the search itself never measures a gap.

use crate::geometry::{Circle, IntBox, IntOctagon, Simplex};
use crate::rules::ClearanceMatrix;

/// A pad or via on one layer, in the shapes FreeRouting reads them as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PadShape {
    Circle(Circle),
    Box(IntBox),
    Octagon(IntOctagon),
    /// Any other convex outline, such as a rotated rectangle.
    Polygon(Simplex),
}

impl PadShape {
    /// `Shape.bounding_octagon`, per kind. `None` for a polygon beyond the
    /// critical bound.
    pub fn bounding_octagon(&self) -> Option<IntOctagon> {
        match self {
            PadShape::Circle(c) => Some(c.bounding_octagon()),
            PadShape::Box(b) => Some(b.to_octagon()),
            PadShape::Octagon(o) => Some(*o),
            PadShape::Polygon(s) => s.bounding_octagon(),
        }
    }

    /// `Shape.bounding_box`, per kind.
    pub fn bounding_box(&self) -> IntBox {
        match self {
            PadShape::Circle(c) => c.bounding_box(),
            PadShape::Box(b) => *b,
            PadShape::Octagon(o) => o.bounding_box(),
            PadShape::Polygon(s) => s.bounding_box(),
        }
    }
}

/// How far an item of clearance class `item_class` grows on `layer` in the
/// tree for trace class `trace_class`: its clearance to that class, less the
/// share a trace carries itself. Never negative; 0 for the null class.
/// `ShapeSearchTree.clearance_compensation_value`.
pub fn clearance_offset(rules: &ClearanceMatrix, item_class: i32, trace_class: i32, layer: i32) -> i64 {
    if item_class <= 0 {
        return 0;
    }
    (rules.get(item_class, trace_class, layer) - rules.compensation(trace_class, layer)).max(0)
}

/// One layer of a pin or via, as it enters the tree: its bounding octagon
/// grown by `offset`. Where that octagon is only a box, the box is grown
/// instead, which keeps its corners square rather than cutting them at 45
/// degrees. `None` where FreeRouting has no shape -- a polygon beyond the
/// critical bound, on which the Java in fact crashes.
/// `ShapeSearchTree45Degree.calculate_tree_shapes(DrillItem)`.
pub fn drill_tree_shape(shape: &PadShape, offset: i64) -> Option<IntOctagon> {
    let octagon = shape.bounding_octagon()?;
    if octagon.is_int_box() {
        return Some(shape.bounding_box().offset(offset).to_octagon());
    }
    Some(octagon.offset(offset as f64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::IntPoint;

    #[test]
    fn a_box_pad_stays_a_box() {
        let pad = PadShape::Box(IntBox::new(0, 0, 10, 6));
        assert_eq!(drill_tree_shape(&pad, 3), Some(IntBox::new(-3, -3, 13, 9).to_octagon()));
    }

    /// An octagon grows its diagonals by sqrt 2 times the offset, as they
    /// are measured along the x axis.
    #[test]
    fn a_round_pad_grows_its_diagonals_too() {
        let pad = PadShape::Circle(Circle::new(IntPoint::new(0, 0), 100));
        let before = pad.bounding_octagon().unwrap();
        let after = drill_tree_shape(&pad, 10).unwrap();
        assert_eq!(after.left_x, before.left_x - 10);
        assert_eq!(after.upper_right_diag_x, before.upper_right_diag_x + 14);
        assert!(!after.is_int_box());
    }

    #[test]
    fn no_offset_leaves_the_bounding_octagon() {
        let pad = PadShape::Circle(Circle::new(IntPoint::new(5, 5), 40));
        assert_eq!(drill_tree_shape(&pad, 0), pad.bounding_octagon());
    }

    #[test]
    fn the_offset_is_the_clearance_beyond_the_traces_own_share() {
        let mut m = ClearanceMatrix::new(3, 1);
        m.set(1, 1, 0, 200);
        m.set(2, 1, 0, 300);
        m.set(1, 2, 0, 300);
        assert_eq!(clearance_offset(&m, 2, 1, 0), 200, "300 needed, 100 carried by the trace");
        assert_eq!(clearance_offset(&m, 1, 1, 0), 100);
        assert_eq!(clearance_offset(&m, 0, 1, 0), 0, "the null class keeps no clearance");
        m.set(2, 1, 0, 50);
        assert_eq!(clearance_offset(&m, 2, 1, 0), 0, "never negative");
    }
}
