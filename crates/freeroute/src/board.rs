//! The board as the router sees it: each item it must keep clear of, as
//! the shapes FreeRouting stores in its search tree for the trace clearance
//! class being routed. Ported from the item side of `ShapeSearchTree` and
//! `ShapeSearchTree45Degree`, item kind by item kind: pins, vias, traces,
//! areas, and the board outline.
//!
//! Every tree shape is an obstacle grown by the clearance it needs from
//! that trace class, so the search itself never measures a gap.

use crate::geometry::{Circle, IntBox, IntOctagon, Line, PolygonShape, Polyline, PolylineArea, Simplex, TileShape};
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

/// A keepout, via keepout, component keepout or copper pour, in the shapes
/// FreeRouting reads them as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AreaShape {
    Circle(Circle),
    /// A polygon, convex or not.
    Polygon(PolygonShape),
    /// A shape convex already: a box, an octagon or a convex polygon.
    Tile(TileShape),
    /// A border with holes.
    WithHoles(PolylineArea),
}

impl AreaShape {
    /// The area in convex pieces, as FreeRouting splits it: a circle is
    /// its bounding octagon. `None` where the split fails.
    /// `Area.split_to_convex`.
    pub fn split_to_convex(&self) -> Option<Vec<TileShape>> {
        match self {
            AreaShape::Circle(c) => Some(vec![TileShape::Octagon(c.bounding_octagon())]),
            AreaShape::Polygon(p) => p.split_to_convex(),
            AreaShape::Tile(t) => Some(vec![t.clone()]),
            AreaShape::WithHoles(a) => a.split_to_convex(),
        }
    }
}

/// The tree shapes of an area on its layer: its convex pieces, each grown
/// by `offset`, cut into sections at most `max_section` wide keeping those
/// with area, and stored under their bounding octagons. None where the
/// split fails, as in the Java; `None` for a section beyond the critical
/// bound, where the Java has no shape.
/// `ShapeSearchTree45Degree.calculate_tree_shapes(ObstacleArea)`; see
/// [`area_section_width`] for `max_section`.
pub fn area_tree_shapes(area: &AreaShape, offset: i64, max_section: f64) -> Vec<Option<IntOctagon>> {
    match area.split_to_convex() {
        Some(pieces) => piece_tree_shapes(&pieces, offset, max_section),
        None => Vec::new(),
    }
}

/// [`area_tree_shapes`] from an area's convex pieces, already split: the
/// split is the costly part, and an area's pieces never change.
pub fn piece_tree_shapes(pieces: &[TileShape], offset: i64, max_section: f64) -> Vec<Option<IntOctagon>> {
    pieces
        .iter()
        .flat_map(|p| p.enlarge(offset as f64).divide_into_sections(max_section))
        .map(|s| s.bounding_octagon())
        .collect()
}

/// How wide an area's tree shapes may be: 50,000 units, or 500 mils if the
/// board came from a CAD system, whichever is less -- FreeRouting's guard
/// against a few huge shapes on coarse KiCad boards. `resolution_per_mil`
/// is the board's units per mil when it came from a CAD system.
pub fn area_section_width(resolution_per_mil: Option<f64>) -> f64 {
    let max = 50_000.0;
    match resolution_per_mil {
        Some(r) => (500.0 * r).min(max),
        None => max,
    }
}

/// The tree shapes of the board outline, when no keepout is generated
/// outside it -- FreeRouting's default. Each edge of each outline shape,
/// given by its border lines, is widened by `half_width + offset` as a
/// trace segment would be, between the lines of its two neighbours, and
/// bounded by an octagon; the lot is repeated on every layer. `offset` is
/// the outline's clearance on layer 0, which the Java uses for all layers.
/// `None` for an edge FreeRouting could not widen, where the Java crashes.
/// `ShapeSearchTree45Degree.calculate_tree_shapes(BoardOutline)`.
pub fn outline_tree_shapes(shapes: &[Vec<Line>], half_width: i64, offset: i64, layers: usize) -> Vec<Option<IntOctagon>> {
    let mut out = Vec::new();
    for _ in 0..layers {
        for border in shapes {
            let n = border.len();
            for i in 0..n {
                let edge = Polyline::from_lines(&[border[(i + n - 1) % n], border[i], border[(i + 1) % n]]);
                out.push(edge.offset_shape(half_width + offset, 0).and_then(|s| s.bounding_octagon()));
            }
        }
    }
    out
}

/// The tree shapes of a trace: each segment of its polyline widened by its
/// half width plus `offset`, its clearance on its layer. Unlike pads and
/// areas, a trace keeps its exact shape -- a box, an octagon, or a polygon
/// for a segment at another angle. `None` for a segment FreeRouting could
/// not widen. `ShapeSearchTree.calculate_tree_shapes(PolylineTrace)`.
pub fn trace_tree_shapes(polyline: &Polyline, half_width: i64, offset: i64) -> Vec<Option<TileShape>> {
    let segments = polyline.lines.len().saturating_sub(2);
    (0..segments).map(|i| polyline.offset_shape(half_width + offset, i)).collect()
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
    fn a_small_round_keepout_is_one_grown_octagon() {
        let area = AreaShape::Circle(Circle::new(IntPoint::new(0, 0), 500));
        let shapes = area_tree_shapes(&area, 100, area_section_width(None));
        assert_eq!(shapes, vec![Some(Circle::new(IntPoint::new(0, 0), 500).bounding_octagon().offset(100.0))]);
    }

    /// A keepout wider than the section width is cut into pieces that
    /// cover it and stay within it.
    #[test]
    fn a_large_keepout_is_cut_into_sections() {
        let big = Circle::new(IntPoint::new(0, 0), 60_000);
        let shapes: Vec<IntOctagon> = area_tree_shapes(&AreaShape::Circle(big), 0, 50_000.0).into_iter().flatten().collect();
        assert_eq!(shapes.len(), 9, "a 120,000-wide octagon in 50,000-wide sections: {shapes:?}");
        let whole: f64 = shapes.iter().map(|s| s.area()).sum();
        assert!((whole - big.bounding_octagon().area()).abs() < 1.0);
        for s in &shapes {
            assert!(s.is_contained_in(&big.bounding_octagon()));
        }
    }

    #[test]
    fn cad_boards_get_narrower_sections() {
        assert_eq!(area_section_width(None), 50_000.0);
        assert_eq!(area_section_width(Some(10.0)), 5_000.0);
        assert_eq!(area_section_width(Some(1_000.0)), 50_000.0);
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
