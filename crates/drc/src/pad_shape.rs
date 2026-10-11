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
