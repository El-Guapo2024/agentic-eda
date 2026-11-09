//! A board polygon's holes (UI-LEFT-2 PP10): `PCB_SHAPE::GetPolyShape()` is a `SHAPE_POLY_SET`, so a zone with holes made a polygon (`CONVERT_TOOL::CreatePolys`:
//! `graphic->SetPolyShape( poly )`) keeps them. Here the outline is `Shape::Polygon::pts` and the holes [`eda_model::pcb_items::ShapeStyle::holes`]; a polygon handed
//! to `add_shape` / `replace_shape` as one fractured ring (what the studio sends, and what the conversion's `rings` are) is taken apart into them again
//! (`SHAPE_POLY_SET::unfractureSingle`), and the writer bridges them back into one ring (`eda_kicad`), since KiCad's file has the outline alone.

use eda_clipper2::Point64;
use eda_model::ir::{Nm, Point, Shape};

/// The holes of a polygon given as a fractured ring, its `pts` made the outline; none (and the shape as it was) for any other shape or a ring with no bridge.
pub(crate) fn split_holes(shape: &mut Shape) -> Vec<Vec<Point>> {
    let Shape::Polygon { pts, .. } = shape else { return Vec::new() };
    if pts.len() < 8 {
        return Vec::new(); // a bridge and a hole take at least eight corners
    }
    let mut poly: Vec<Vec<Point64>> = vec![pts.iter().map(|p| Point64::new(p.x.get(), p.y.get())).collect()];
    eda_shape_poly_set::fracture::unfracture_single(&mut poly);
    if poly.len() < 2 || poly[0].len() < 3 {
        return Vec::new();
    }
    // the corner a bridge left on a side (`SHAPE_POLY_SET::Unfracture`'s `Simplify()` merges it away)
    let back = |c: &Vec<Point64>| drop_collinear(c).iter().map(|p| Point { x: Nm::new(p.x), y: Nm::new(p.y) }).collect::<Vec<Point>>();
    *pts = back(&poly[0]);
    poly[1..].iter().map(back).filter(|h| h.len() >= 3).collect()
}

/// A closed contour without the corners that lie on the straight line between their neighbours.
fn drop_collinear(c: &[Point64]) -> Vec<Point64> {
    let mut out: Vec<Point64> = c.to_vec();
    let mut i = 0;
    while out.len() > 3 && i < out.len() {
        let n = out.len();
        let (a, p, b) = (out[(i + n - 1) % n], out[i], out[(i + 1) % n]);
        let cross = (p.x - a.x) as i128 * (b.y - p.y) as i128 - (p.y - a.y) as i128 * (b.x - p.x) as i128;
        let dot = (p.x - a.x) as i128 * (b.x - p.x) as i128 + (p.y - a.y) as i128 * (b.y - p.y) as i128;
        if (cross == 0 && dot >= 0) || p == a {
            out.remove(i);
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::units::mm;

    fn sq(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
        [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].iter().map(|&(x, y)| Point { x: mm(x), y: mm(y) }).collect()
    }

    #[test]
    fn a_fractured_ring_is_taken_apart_into_its_outline_and_holes_and_a_plain_polygon_is_left_alone() {
        let mut set = eda_shape_poly_set::ShapePolySet::new();
        let p64 = |v: Vec<Point>| v.iter().map(|p| Point64::new(p.x.get(), p.y.get())).collect::<Vec<_>>();
        let mut hole = p64(sq(3.0, 3.0, 6.0, 6.0));
        hole.reverse();
        set.add_polygon(vec![p64(sq(0.0, 0.0, 10.0, 10.0)), hole]);
        set.fracture(false);
        let ring: Vec<Point> = set.polys[0][0].iter().map(|p| Point { x: Nm::new(p.x), y: Nm::new(p.y) }).collect();
        let mut shape = Shape::Polygon { id: String::new(), layer: "F.Cu".into(), stroke_width: Nm::ZERO, filled: true, pts: ring };
        let holes = split_holes(&mut shape);
        assert_eq!(holes.len(), 1);
        let Shape::Polygon { pts, .. } = &shape else { unreachable!() };
        assert_eq!(pts.len(), 4, "the outline alone: {pts:?}");
        assert_eq!(holes[0].len(), 4);
        let mut plain = Shape::Polygon { id: String::new(), layer: "F.Cu".into(), stroke_width: Nm::ZERO, filled: true, pts: sq(0.0, 0.0, 10.0, 10.0) };
        assert!(split_holes(&mut plain).is_empty());
    }
}
