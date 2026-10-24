//! A pad's copper on the board: `PAD::GetEffectiveShape` (`pcbnew/pad.cpp`) for a trapezoid, a chamfered rectangle, a custom pad and a
//! pad whose copper is off its position. The parts come from [`eda_model::pad_geom::pad_pieces`], the one place that knows what a
//! pad is made of, carried to the board by the pad's and footprint's pose.
//!
//! The DRC, the routing gate and the router all measure a pad by this shape, so a custom pad's primitives, a
//! trapezoid's slanted sides and a chamfer's cut corners are the clearance they were in KiCad.

use crate::kimath::{Seg, Shape};
use eda_model::footprint::Pad;
use eda_model::ir::{FootprintInstance, Point, Nm};
use eda_model::pad_geom::{pad_pieces, PadFrame, PadPiece, P};

fn pt(p: P) -> Point {
    Point { x: p.0.round() as Nm, y: p.1.round() as Nm }
}

/// One piece, already in board space, as a DRC shape.
fn piece_shape(piece: &PadPiece) -> Option<Shape> {
    Some(match piece {
        PadPiece::Circle { c, r } => Shape::Circle { c: pt(*c), r: r.round() as Nm },
        PadPiece::Stadium { a, b, r } => Shape::Stadium { a: pt(*a), b: pt(*b), r: r.round() as Nm },
        PadPiece::Polygon(pts) if pts.len() >= 3 => Shape::Polygon { pts: pts.iter().map(|&p| pt(p)).collect() },
        PadPiece::Polygon(_) => return None,
        PadPiece::Strokes { pts, r } => {
            let segs: Vec<Seg> = pts.windows(2).map(|w| Seg::new(pt(w[0]), pt(w[1]))).collect();
            if segs.is_empty() {
                return None;
            }
            Shape::Strokes { segs, r: r.round() as Nm }
        }
    })
}

/// The parts of the pad's copper on the board (`PAD::GetEffectiveShape` is a compound of them), `center` being the pad's position.
pub fn pad_effective_shapes(fp: &FootprintInstance, pad: &Pad, center: Point) -> Vec<Shape> {
    let frame = PadFrame::of(fp, pad, center);
    pad_pieces(pad).iter().filter_map(|piece| piece_shape(&piece.map(&|v| frame.to_board(v)))).collect()
}

/// The pad's copper as one shape: its only part, or a compound of them.
pub fn pad_copper(fp: &FootprintInstance, pad: &Pad, center: Point) -> Shape {
    let mut parts = pad_effective_shapes(fp, pad, center);
    match parts.len() {
        0 => Shape::Circle { c: center, r: 0 },
        1 => parts.remove(0),
        _ => Shape::Compound { parts },
    }
}

/// The pad's copper as polygons with holes in board space: the union of the outlines of its parts (`PAD::GetEffectivePolygon`, with the
/// error inside, the outline the studio draws and the 3D view extrudes). A custom pad with a ring has an outline and a hole. The studio's
/// painter fills the contours with the even-odd rule.
pub fn pad_polygons(fp: &FootprintInstance, pad: &Pad, center: Point, max_error: Nm) -> eda_shape_poly_set::ShapePolySet {
    crate::shape_poly::union_polyset(&pad_copper(fp, pad, center), max_error)
}

/// A pad's copper in its footprint's own frame (the footprint at the origin, not turned and seen from the top) as polygons: where a
/// footprint's free graphics lie against it (`PAD::Recombine`'s `TransformShapeToPolygon( padPoly, layer, 0, maxError, ERROR_INSIDE )`).
pub fn pad_polygons_in_footprint(pad: &Pad, max_error: Nm) -> eda_shape_poly_set::ShapePolySet {
    let fp = FootprintInstance { id: String::new(), at: Point { x: 0, y: 0 }, rot: 0, side: eda_model::ir::Side::Top, label: eda_model::ir::LabelSide::Above };
    pad_polygons(&fp, pad, Point { x: pad.at.0, y: pad.at.1 }, max_error)
}

/// One graphic or basic shape as the polygons `PCB_SHAPE::TransformShapeToPolygon` makes of it at no clearance, the stroke counted
/// (`EDA_SHAPE::makeEffectiveShapes`): a line is a stadium, an unfilled shape its outline at its width, a filled one its area.
pub fn shape_polygons(shape: &eda_model::ir::Shape, max_error: Nm) -> eda_shape_poly_set::ShapePolySet {
    let pieces = eda_model::pad_geom::primitive_pieces(shape, (0.0, 0.0), max_error as f64);
    let parts: Vec<Shape> = pieces.iter().filter_map(piece_shape).collect();
    crate::shape_poly::union_polyset(&Shape::Compound { parts }, max_error)
}

/// [`pad_polygons`] as the list of its contours, the outline of each polygon first and then its holes, in nanometres.
pub fn pad_outline(fp: &FootprintInstance, pad: &Pad, center: Point, max_error: Nm) -> Vec<Vec<Point>> {
    pad_polygons(fp, pad, center, max_error).polys.iter().flat_map(|poly| poly.iter().map(|chain| chain.iter().map(|p| Point { x: p.x, y: p.y }).collect::<Vec<_>>())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::footprint::{PadAnchor, PadCustom, PadKind, PadPrimitive, PadShape, PadShapeExt};
    use eda_model::ir::{LabelSide, Side};

    fn instance(at: (Nm, Nm), rot: i32, side: Side) -> FootprintInstance {
        FootprintInstance { id: "U1".into(), at: Point { x: at.0, y: at.1 }, rot: rot as u32, side, label: LabelSide::Above }
    }

    fn pad(shape: PadShape, size: (Nm, Nm), ext: PadShapeExt) -> Pad {
        Pad { number: "1".into(), at: (0, 0), size, shape, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None, opposite_side: false, ext }
    }

    #[test]
    fn a_trapezoids_polygon_has_the_area_of_its_two_sides_and_turns_with_the_footprint() {
        let t = pad(PadShape::Trapezoid, (2_000_000, 1_000_000), PadShapeExt { trapezoid_delta: Some((0, 800_000)), ..Default::default() });
        // Parallel sides 2.8 and 1.2 mm, 1 mm apart: (2.8 + 1.2) / 2 = 2 mm^2.
        for rot in [0, 90_000, 45_000] {
            let area = pad_polygons(&instance((10_000_000, 10_000_000), rot, Side::Top), &t, Point { x: 10_000_000, y: 10_000_000 }, 5000).area();
            assert!((area - 2_000_000_000_000.0).abs() < 2_000_000_000.0, "rot {rot}: {area}");
        }
        // Copper never leaves the pad's own bounding box turned the same way.
        let shapes = pad_effective_shapes(&instance((0, 0), 0, Side::Top), &t, Point { x: 0, y: 0 });
        assert_eq!(shapes.len(), 1);
        assert_eq!(shapes[0].bbox(0), (-1_400_000, -500_000, 1_400_000, 500_000));
    }

    #[test]
    fn a_custom_pads_copper_is_a_compound_with_a_hole_where_a_ring_leaves_one() {
        let ring = PadPrimitive { shape: eda_model::ir::Shape::Circle { id: String::new(), layer: String::new(), stroke_width: 600_000, filled: false, center: Point { x: 0, y: 0 }, end: Point { x: 1_500_000, y: 0 } }, proxy: false };
        let p = pad(PadShape::Custom, (500_000, 500_000), PadShapeExt { custom: Some(PadCustom { anchor: PadAnchor::Circle, zone: Default::default(), primitives: vec![ring] }), ..Default::default() });
        let fp = instance((0, 0), 0, Side::Top);
        let copper = pad_copper(&fp, &p, Point { x: 0, y: 0 });
        assert!(matches!(copper, Shape::Compound { ref parts } if parts.len() == 2), "{copper:?}");
        // A ring 1.2 to 1.8 mm out of the centre: the polygon has an outline and a hole, and its area is the ring's plus the anchor's disc inside it.
        let polys = pad_polygons(&fp, &p, Point { x: 0, y: 0 }, 5000);
        let ring_area = std::f64::consts::PI * (1_800_000.0f64.powi(2) - 1_200_000.0f64.powi(2));
        let disc_area = std::f64::consts::PI * 250_000.0f64.powi(2);
        assert!((polys.area() - (ring_area + disc_area)).abs() / (ring_area + disc_area) < 0.01, "{}", polys.area());
        assert_eq!(polys.polys.len(), 2, "the ring (with its hole) and the anchor inside it");
        assert_eq!(polys.polys.iter().map(|p| p.len()).max(), Some(2), "one outline has a hole");
        // Clearance is measured to the ring's copper, not to the middle of it.
        let probe = Shape::Circle { c: Point { x: 0, y: 0 }, r: 100_000 };
        let (gap, _) = copper.clearance_to(&probe);
        assert_eq!(gap, 0, "the anchor is under the probe");
        let outside = Shape::Circle { c: Point { x: 2_100_000, y: 0 }, r: 100_000 };
        let (gap, _) = copper.clearance_to(&outside);
        assert!((gap - 200_000).abs() <= 1, "the ring's outer edge is at 1800: {gap}");
        let inside = Shape::Circle { c: Point { x: 700_000, y: 0 }, r: 100_000 };
        let (gap, _) = copper.clearance_to(&inside);
        assert!((gap - 350_000).abs() <= 6000, "the anchor's edge is at 250 and the ring's inner edge at 1200: 700 is 450 from the first and 500 from the second, less the probe's 100: {gap}");
    }

    #[test]
    fn the_copper_of_a_pad_with_an_offset_sits_where_the_offset_puts_it_on_either_side() {
        let p = pad(PadShape::Rect, (1_000_000, 600_000), PadShapeExt { offset: Some((400_000, 0)), ..Default::default() });
        // On top: 0.4 mm to the right of the pad's position; on the bottom the frame is mirrored: 0.4 mm to the left.
        let top = pad_copper(&instance((5_000_000, 5_000_000), 0, Side::Top), &p, Point { x: 5_000_000, y: 5_000_000 });
        assert_eq!(top.bbox(0), (4_900_000, 4_700_000, 5_900_000, 5_300_000));
        let bottom = pad_copper(&instance((5_000_000, 5_000_000), 0, Side::Bottom), &p, Point { x: 5_000_000, y: 5_000_000 });
        assert_eq!(bottom.bbox(0), (4_100_000, 4_700_000, 5_100_000, 5_300_000));
    }
}
