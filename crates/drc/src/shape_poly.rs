//! A [`Shape`] as polygons: the outline of a pad's copper the studio draws and the 3D view extrudes (`crate::pad_shape::pad_polygons`), with the error of a curve on the inside
//! (`PAD::GetEffectivePolygon`, `TransformCircleToPolygon` with `ERROR_INSIDE`'s segment count, `GetArcToSegmentCount`).

use crate::kimath::Shape;
use eda_clipper2::{Point64, PI};
use eda_shape_poly_set::{LineChain, ShapePolySet};

fn p64(p: eda_model::ir::Point) -> Point64 {
    Point64::new(p.x.get(), p.y.get())
}

/// Every outline the shape is made of: the shape's own for a simple one, a stadium per segment of a stroke, the outlines of each part of a compound. Their union is the shape.
pub fn exact_polygons(shape: &Shape, max_error: i64) -> Vec<LineChain> {
    match shape {
        Shape::Strokes { segs, r } => segs.iter().map(|s| stadium_polygon(p64(s.a), p64(s.b), r.get(), max_error)).collect(),
        Shape::Compound { parts } => parts.iter().flat_map(|p| exact_polygons(p, max_error)).collect(),
        Shape::Circle { c, r } => vec![circle_polygon(p64(*c), r.get(), max_error)],
        Shape::Rect { x0, y0, x1, y1 } => vec![vec![Point64::new(x0.get(), y0.get()), Point64::new(x1.get(), y0.get()), Point64::new(x1.get(), y1.get()), Point64::new(x0.get(), y1.get())]],
        Shape::RoundRect { x0, y0, x1, y1, r } => vec![round_rect_polygon(x0.get(), y0.get(), x1.get(), y1.get(), r.get(), max_error)],
        Shape::Stadium { a, b, r } => vec![stadium_polygon(p64(*a), p64(*b), r.get(), max_error)],
        Shape::Polygon { pts } if pts.is_empty() => Vec::new(),
        Shape::Polygon { pts } => vec![pts.iter().map(|&p| p64(p)).collect()],
    }
}

fn circle_polygon(c: Point64, r: i64, max_error: i64) -> LineChain {
    let r = r.max(1);
    let seg_count = eda_shape_poly_set::get_arc_to_segment_count(r as i32, max_error.max(1) as i32).max(8) as usize;
    (0..seg_count)
        .map(|i| {
            let theta = 2.0 * PI * (i as f64) / (seg_count as f64);
            Point64::new(c.x + (r as f64 * theta.cos()).round() as i64, c.y + (r as f64 * theta.sin()).round() as i64)
        })
        .collect()
}

fn round_rect_polygon(x0: i64, y0: i64, x1: i64, y1: i64, r: i64, max_error: i64) -> LineChain {
    if r <= 0 {
        return vec![Point64::new(x0, y0), Point64::new(x1, y0), Point64::new(x1, y1), Point64::new(x0, y1)];
    }
    let seg_count = eda_shape_poly_set::get_arc_to_segment_count(r as i32, max_error.max(1) as i32).max(8) as usize;
    let per_corner = (seg_count / 4).max(2);
    let (ix0, iy0, ix1, iy1) = (x0 + r, y0 + r, x1 - r, y1 - r);
    let corners = [
        (ix1, iy1, 0.0),
        (ix0, iy1, std::f64::consts::FRAC_PI_2),
        (ix0, iy0, std::f64::consts::PI),
        (ix1, iy0, 3.0 * std::f64::consts::FRAC_PI_2),
    ];
    let mut out = Vec::with_capacity(per_corner * 4);
    for &(cx, cy, start_angle) in &corners {
        for i in 0..=per_corner {
            let theta = start_angle + std::f64::consts::FRAC_PI_2 * (i as f64) / (per_corner as f64);
            out.push(Point64::new(cx + (r as f64 * theta.cos()).round() as i64, cy + (r as f64 * theta.sin()).round() as i64));
        }
    }
    out
}

fn stadium_polygon(a: Point64, b: Point64, r: i64, max_error: i64) -> LineChain {
    let r = r.max(1);
    let (dx, dy) = ((b.x - a.x) as f64, (b.y - a.y) as f64);
    let len = dx.hypot(dy);
    if len < 1.0 {
        return circle_polygon(a, r, max_error);
    }
    let (ux, uy) = (dx / len, dy / len);
    let seg_count = eda_shape_poly_set::get_arc_to_segment_count(r as i32, max_error.max(1) as i32).max(8) as usize;
    let half = (seg_count / 2).max(2);
    let base_angle = uy.atan2(ux);
    let mut out = Vec::with_capacity(half * 2 + 2);
    // The cap at b, from -90 to +90 degrees of the direction a -> b, then the cap at a, continuing the turn.
    for i in 0..=half {
        let theta = base_angle - std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * (i as f64) / (half as f64);
        out.push(Point64::new(b.x + (r as f64 * theta.cos()).round() as i64, b.y + (r as f64 * theta.sin()).round() as i64));
    }
    for i in 0..=half {
        let theta = base_angle + std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * (i as f64) / (half as f64);
        out.push(Point64::new(a.x + (r as f64 * theta.cos()).round() as i64, a.y + (r as f64 * theta.sin()).round() as i64));
    }
    out
}

/// The shape as polygons with holes: the union of its outlines (`SHAPE_POLY_SET::Simplify`). A ring is an outline with a hole.
pub fn union_polyset(shape: &Shape, max_error: i64) -> ShapePolySet {
    let mut set = ShapePolySet::new();
    for poly in exact_polygons(shape, max_error) {
        if poly.len() >= 3 {
            set.add_outline(poly);
        }
    }
    set.simplify();
    set
}
