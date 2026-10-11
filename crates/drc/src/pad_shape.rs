//! A pad's copper on the board: `PAD::GetEffectiveShape` (`pcbnew/pad.cpp`) for a trapezoid, a chamfered rectangle, a custom pad and a
//! pad whose copper is off its position. The parts come from [`eda_model::pad_geom::pad_pieces`], the one place that knows what a
//! pad is made of, carried to the board by the pad's and footprint's pose.
//!
//! The DRC, the routing gate, the router and the zone filler all measure a pad by this shape, so a custom pad's primitives, a
//! trapezoid's slanted sides and a chamfer's cut corners are the clearance they were in KiCad.

use crate::kimath::{Seg, Shape};
use eda_model::footprint::{Pad, PadShape};
use eda_model::ir::{FootprintInstance, Point, Um};
use eda_model::pad_geom::{pad_pieces, PadFrame, PadPiece, P};

fn pt(p: P) -> Point {
    Point { x: p.0.round() as Um, y: p.1.round() as Um }
}

/// One piece, already in board space, as a DRC shape.
fn piece_shape(piece: &PadPiece) -> Option<Shape> {
    Some(match piece {
        PadPiece::Circle { c, r } => Shape::Circle { c: pt(*c), r: r.round() as Um },
        PadPiece::Stadium { a, b, r } => Shape::Stadium { a: pt(*a), b: pt(*b), r: r.round() as Um },
        PadPiece::Polygon(pts) if pts.len() >= 3 => Shape::Polygon { pts: pts.iter().map(|&p| pt(p)).collect() },
        PadPiece::Polygon(_) => return None,
        PadPiece::Strokes { pts, r } => {
            let segs: Vec<Seg> = pts.windows(2).map(|w| Seg::new(pt(w[0]), pt(w[1]))).collect();
            if segs.is_empty() {
                return None;
            }
            Shape::Strokes { segs, r: r.round() as Um }
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

/// `PAD::ShapePos` on the board: the pad's position moved by its offset, which is in the pad's own (turned, mirrored) frame.
pub fn shape_pos(fp: &FootprintInstance, pad: &Pad, center: Point) -> Point {
    match pad.ext.offset {
        Some((ox, oy)) if ox != 0 || oy != 0 => pt(PadFrame::of(fp, pad, center).to_board((ox as f64, oy as f64))),
        _ => center,
    }
}

/// The bounding box `(x0, y0, x1, y1)` of the pad's copper at orientation 0, position 0 and no offset: `dummy_pad.GetBoundingBox( layer )`
/// in `ZONE_FILLER::buildThermalSpokes`, the box a thermal relief's spokes are cut from. `None` for a pad whose box is its size.
pub fn spoke_box(pad: &Pad) -> Option<(Um, Um, Um, Um)> {
    if !pad.has_complex_outline() {
        return None;
    }
    let mut bare = pad.clone();
    bare.ext.offset = None;
    bare.rot = 0;
    let (x0, y0, x1, y1) = bare.local_bbox();
    Some((x0.floor() as Um, y0.floor() as Um, x1.ceil() as Um, y1.ceil() as Um))
}

/// Whether a zone keeps clear of the convex hull of the pad rather than its shape (`CUSTOM_SHAPE_ZONE_MODE::CONVEXHULL`).
pub fn custom_convex_hull(pad: &Pad) -> bool {
    pad.shape == PadShape::Custom && pad.ext.custom.as_ref().is_some_and(|c| c.zone == eda_model::footprint::CustomZoneMode::ConvexHull)
}

/// The pad's copper as polygons with holes in board space: the union of the outlines of its parts (`PAD::GetEffectivePolygon`, with the
/// error inside, the outline the studio draws and the 3D view extrudes). A custom pad with a ring has an outline and a hole. The studio's
/// painter fills the contours with the even-odd rule.
pub fn pad_polygons(fp: &FootprintInstance, pad: &Pad, center: Point, max_error: Um) -> eda_shape_poly_set::ShapePolySet {
    let shape = crate::fill::convert_shape(&pad_copper(fp, pad, center));
    eda_zone_filler::shape::union_polyset(&shape, max_error)
}

/// [`pad_polygons`] as the list of its contours, the outline of each polygon first and then its holes, in micrometres.
pub fn pad_outline(fp: &FootprintInstance, pad: &Pad, center: Point, max_error: Um) -> Vec<Vec<Point>> {
    pad_polygons(fp, pad, center, max_error).polys.iter().flat_map(|poly| poly.iter().map(|chain| chain.iter().map(|p| Point { x: p.x, y: p.y }).collect::<Vec<_>>())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::footprint::{PadAnchor, PadCustom, PadKind, PadPrimitive, PadShapeExt};
    use eda_model::ir::{ChamferCorners, LabelSide, Side};

    fn instance(at: (Um, Um), rot: i32, side: Side) -> FootprintInstance {
        FootprintInstance { id: "U1".into(), at: Point { x: at.0, y: at.1 }, rot: rot as u32, side, label: LabelSide::Above }
    }

    fn pad(shape: PadShape, size: (Um, Um), ext: PadShapeExt) -> Pad {
        Pad { number: "1".into(), at: (0, 0), size, shape, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None, opposite_side: false, ext }
    }

    fn line(a: (Um, Um), b: (Um, Um), w: Um) -> PadPrimitive {
        PadPrimitive { shape: eda_model::ir::Shape::Segment { id: String::new(), layer: String::new(), stroke_width: w, filled: false, start: Point { x: a.0, y: a.1 }, end: Point { x: b.0, y: b.1 } }, proxy: false }
    }

    #[test]
    fn a_trapezoids_polygon_has_the_area_of_its_two_sides_and_turns_with_the_footprint() {
        let t = pad(PadShape::Trapezoid, (2000, 1000), PadShapeExt { trapezoid_delta: Some((0, 800)), ..Default::default() });
        // Parallel sides 2.8 and 1.2 mm, 1 mm apart: (2.8 + 1.2) / 2 = 2 mm^2.
        for rot in [0, 90_000, 45_000] {
            let area = pad_polygons(&instance((10_000, 10_000), rot, Side::Top), &t, Point { x: 10_000, y: 10_000 }, 5).area();
            assert!((area - 2_000_000.0).abs() < 2_000.0, "rot {rot}: {area}");
        }
        // Copper never leaves the pad's own bounding box turned the same way.
        let shapes = pad_effective_shapes(&instance((0, 0), 0, Side::Top), &t, Point { x: 0, y: 0 });
        assert_eq!(shapes.len(), 1);
        assert_eq!(shapes[0].bbox(0), (-1400, -500, 1400, 500));
    }

    #[test]
    fn a_custom_pads_copper_is_a_compound_with_a_hole_where_a_ring_leaves_one() {
        let ring = PadPrimitive { shape: eda_model::ir::Shape::Circle { id: String::new(), layer: String::new(), stroke_width: 600, filled: false, center: Point { x: 0, y: 0 }, end: Point { x: 1500, y: 0 } }, proxy: false };
        let p = pad(PadShape::Custom, (500, 500), PadShapeExt { custom: Some(PadCustom { anchor: PadAnchor::Circle, zone: Default::default(), primitives: vec![ring] }), ..Default::default() });
        let fp = instance((0, 0), 0, Side::Top);
        let copper = pad_copper(&fp, &p, Point { x: 0, y: 0 });
        assert!(matches!(copper, Shape::Compound { ref parts } if parts.len() == 2), "{copper:?}");
        // A ring 1.2 to 1.8 mm out of the centre: the polygon has an outline and a hole, and its area is the ring's plus the anchor's disc inside it.
        let polys = pad_polygons(&fp, &p, Point { x: 0, y: 0 }, 5);
        let ring_area = std::f64::consts::PI * (1800.0f64.powi(2) - 1200.0f64.powi(2));
        let disc_area = std::f64::consts::PI * 250.0f64.powi(2);
        assert!((polys.area() - (ring_area + disc_area)).abs() / (ring_area + disc_area) < 0.01, "{}", polys.area());
        assert_eq!(polys.polys.len(), 2, "the ring (with its hole) and the anchor inside it");
        assert_eq!(polys.polys.iter().map(|p| p.len()).max(), Some(2), "one outline has a hole");
        // Clearance is measured to the ring's copper, not to the middle of it.
        let probe = Shape::Circle { c: Point { x: 0, y: 0 }, r: 100 };
        let (gap, _) = copper.clearance_to(&probe);
        assert_eq!(gap, 0, "the anchor is under the probe");
        let outside = Shape::Circle { c: Point { x: 2100, y: 0 }, r: 100 };
        let (gap, _) = copper.clearance_to(&outside);
        assert!((gap - 200).abs() <= 1, "the ring's outer edge is at 1800: {gap}");
        let inside = Shape::Circle { c: Point { x: 700, y: 0 }, r: 100 };
        let (gap, _) = copper.clearance_to(&inside);
        assert!((gap - 350).abs() <= 6, "the anchor's edge is at 250 and the ring's inner edge at 1200: 700 is 450 from the first and 500 from the second, less the probe's 100: {gap}");
    }

    #[test]
    fn the_copper_of_a_pad_with_an_offset_sits_where_the_offset_puts_it_on_either_side() {
        let p = pad(PadShape::Rect, (1000, 600), PadShapeExt { offset: Some((400, 0)), ..Default::default() });
        // On top: 0.4 mm to the right of the pad's position; on the bottom the frame is mirrored: 0.4 mm to the left.
        let top = pad_copper(&instance((5000, 5000), 0, Side::Top), &p, Point { x: 5000, y: 5000 });
        assert_eq!(top.bbox(0), (4900, 4700, 5900, 5300));
        let bottom = pad_copper(&instance((5000, 5000), 0, Side::Bottom), &p, Point { x: 5000, y: 5000 });
        assert_eq!(bottom.bbox(0), (4100, 4700, 5100, 5300));
        assert_eq!(shape_pos(&instance((5000, 5000), 0, Side::Top), &p, Point { x: 5000, y: 5000 }), Point { x: 5400, y: 5000 });
        assert_eq!(shape_pos(&instance((5000, 5000), 90_000, Side::Top), &p, Point { x: 5000, y: 5000 }), Point { x: 5000, y: 5400 }, "turned with the footprint");
    }

    #[test]
    fn the_spoke_box_of_a_custom_pad_is_the_box_of_all_its_copper_and_mirrors_with_the_side() {
        let p = pad(PadShape::Custom, (800, 800), PadShapeExt { custom: Some(PadCustom { anchor: PadAnchor::Rect, zone: Default::default(), primitives: vec![line((0, 0), (-2000, 0), 200)] }), ..Default::default() });
        assert_eq!(spoke_box(&p), Some((-2100, -400, 400, 400)));
        // A plain pad has none: the filler takes its size.
        assert_eq!(spoke_box(&pad(PadShape::Rect, (800, 800), PadShapeExt::default())), None);
        // The chamfered rectangle's box is its own size.
        let c = pad(PadShape::ChamferedRect, (1000, 600), PadShapeExt { chamfer_ratio: Some(0.2), chamfer_corners: ChamferCorners { top_left: true, ..Default::default() }, ..Default::default() });
        assert_eq!(spoke_box(&c), Some((-500, -300, 500, 300)));
    }
}
