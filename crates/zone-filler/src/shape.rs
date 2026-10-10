//! Geometric primitives this crate's callers describe pads/tracks/vias
//! with, and their conversion into `eda_shape_poly_set` polygons (the
//! "`TransformShapeToPolygon(..., aClearance, aError, ERROR_OUTSIDE)`" half
//! of KiCad's `zone_filler.cpp`).
//!
//! This is a deliberately small, dependency-free duplicate of
//! `eda_drc::kimath::Shape`'s shape set (same four board-primitive
//! variants, matching this workspace's established pad/track/via
//! simplifications -- axis-aligned boxes, exact for 0/90/180/270 degree
//! rotation) rather than a dependency on `eda-drc` itself: `eda-drc` is
//! meant to grow a dependency *on* this crate (to DRC against real fills,
//! per the task's stage 4), so depending the other way would be circular.
//! A small adapter in the caller (CLI/API wiring) maps `eda_drc::board`'s
//! richer types into this one.

use eda_clipper2::{Point64, PI};
use eda_shape_poly_set::{CornerStrategy, LineChain, ShapePolySet};

#[derive(Debug, Clone)]
pub enum Shape {
    Circle { c: Point64, r: i64 },
    /// A "stadium" (KiCad's `SHAPE_SEGMENT`): a track segment, an oval pad,
    /// or a slotted hole.
    Stadium { a: Point64, b: Point64, r: i64 },
    Rect { x0: i64, y0: i64, x1: i64, y1: i64 },
    RoundRect { x0: i64, y0: i64, x1: i64, y1: i64, r: i64 },
    Polygon { pts: Vec<Point64> },
    /// A run of stroked segments of half width `r` (`SHAPE_ARC` with a width): a custom pad's arc, or the ring of a circle drawn
    /// without fill. Each segment is a stadium.
    Strokes { segs: Vec<(Point64, Point64)>, r: i64 },
    /// `SHAPE_COMPOUND`: the parts that make one item's copper -- a custom pad's anchor and primitives. The shape of the union.
    Compound { parts: Vec<Shape> },
}

/// The exact (zero-clearance) boundary polygon for a shape, same
/// `max_error`-driven segment counts `GetArcToSegmentCount` uses for the
/// real circular/rounded shapes.
pub fn exact_polygon(shape: &Shape, max_error: i64) -> LineChain {
    match shape {
        Shape::Circle { c, r } => circle_polygon(*c, *r, max_error),
        Shape::Rect { x0, y0, x1, y1 } => vec![Point64::new(*x0, *y0), Point64::new(*x1, *y0), Point64::new(*x1, *y1), Point64::new(*x0, *y1)],
        Shape::RoundRect { x0, y0, x1, y1, r } => round_rect_polygon(*x0, *y0, *x1, *y1, *r, max_error),
        Shape::Stadium { a, b, r } => stadium_polygon(*a, *b, *r, max_error),
        Shape::Polygon { pts } => pts.clone(),
        // A shape of several outlines has no single one: the first stands in (use [`exact_polygons`] for all of them).
        Shape::Strokes { .. } | Shape::Compound { .. } => exact_polygons(shape, max_error).into_iter().next().unwrap_or_default(),
    }
}

/// Every outline the shape is made of: the shape's own for a simple one, a stadium per segment of a stroke, the outlines of each part
/// of a compound. Their union is the shape.
pub fn exact_polygons(shape: &Shape, max_error: i64) -> Vec<LineChain> {
    match shape {
        Shape::Strokes { segs, r } => segs.iter().map(|&(a, b)| stadium_polygon(a, b, *r, max_error)).collect(),
        Shape::Compound { parts } => parts.iter().flat_map(|p| exact_polygons(p, max_error)).collect(),
        _ => {
            let poly = exact_polygon(shape, max_error);
            if poly.is_empty() {
                Vec::new()
            } else {
                vec![poly]
            }
        }
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
    let r = r.max(1);
    let seg_count = eda_shape_poly_set::get_arc_to_segment_count(r as i32, max_error.max(1) as i32).max(8) as usize;
    let per_corner = (seg_count / 4).max(2);
    let (ix0, iy0, ix1, iy1) = (x0 + r, y0 + r, x1 - r, y1 - r);
    let corners = [
        (ix1, iy1, 0.0),                // bottom-right, 0..90
        (ix0, iy1, std::f64::consts::FRAC_PI_2), // bottom-left, 90..180
        (ix0, iy0, std::f64::consts::PI),        // top-left, 180..270
        (ix1, iy0, 3.0 * std::f64::consts::FRAC_PI_2), // top-right, 270..360
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
    let (ux, uy) = (dx / len, dy / len); // along a->b
    let (nx, ny) = (-uy, ux); // left normal
    let seg_count = eda_shape_poly_set::get_arc_to_segment_count(r as i32, max_error.max(1) as i32).max(8) as usize;
    let half = (seg_count / 2).max(2);
    let base_angle = uy.atan2(ux); // direction of a->b

    let mut out = Vec::with_capacity(half * 2 + 2);
    // right-hand cap at b, from -90deg to +90deg relative to a->b direction
    for i in 0..=half {
        let theta = base_angle - std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * (i as f64) / (half as f64);
        out.push(Point64::new(b.x + (r as f64 * theta.cos()).round() as i64, b.y + (r as f64 * theta.sin()).round() as i64));
    }
    // left-hand cap at a, continuing the turn
    for i in 0..=half {
        let theta = base_angle + std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * (i as f64) / (half as f64);
        out.push(Point64::new(a.x + (r as f64 * theta.cos()).round() as i64, a.y + (r as f64 * theta.sin()).round() as i64));
    }
    let _ = (nx, ny);
    out
}

/// `TransformShapeToPolygon(..., aClearance, aError, ERROR_OUTSIDE)`: the
/// exact shape, Minkowski-summed outward with a disk of radius `gap` (via
/// `eda_shape_poly_set`'s round-join `Inflate`, matching KiCad's actual
/// corner handling for a clearance "rolled" around a shape).
pub fn shape_to_polygon(shape: &Shape, gap: i64, max_error: i64) -> LineChain {
    let base = exact_polygon(shape, max_error);
    if gap <= 0 {
        return base;
    }
    let mut sps = ShapePolySet::from_outline(base);
    sps.inflate(gap, CornerStrategy::RoundAllCorners, max_error.max(1) as i32, false);
    if sps.outline_count() > 0 {
        sps.outline(0).clone()
    } else {
        Vec::new()
    }
}

/// `TransformShapeToPolygon( ..., aClearance, aError, ERROR_OUTSIDE )` for
/// the shapes that grow analytically (a circle, an oval/track, a (rounded)
/// rectangle): the shape is grown by `clearance` first and polygonised
/// once, with every arc's radius pushed out by `max_error`
/// (`GetCircleToPolyCorrection`) so no chord ever cuts inside the true
/// clearance boundary. A general polygon falls back to [`shape_to_polygon`]
/// with the correction added to the inflate amount.
pub fn shape_to_polygon_outside(shape: &Shape, clearance: i64, max_error: i64) -> LineChain {
    let c = clearance.max(0);
    let grow = c + max_error;
    match shape {
        Shape::Circle { c: center, r } => exact_polygon(&Shape::Circle { c: *center, r: r + grow }, max_error),
        Shape::Stadium { a, b, r } => exact_polygon(&Shape::Stadium { a: *a, b: *b, r: r + grow }, max_error),
        // `RoundRect`'s corner centres sit `r` inside its bounds, so the
        // bounds grow with the radius to keep them on the original corners.
        Shape::Rect { x0, y0, x1, y1 } if c > 0 => exact_polygon(&Shape::RoundRect { x0: x0 - grow, y0: y0 - grow, x1: x1 + grow, y1: y1 + grow, r: grow }, max_error),
        Shape::Rect { .. } => exact_polygon(shape, max_error),
        Shape::RoundRect { x0, y0, x1, y1, r } => exact_polygon(&Shape::RoundRect { x0: x0 - grow, y0: y0 - grow, x1: x1 + grow, y1: y1 + grow, r: r + grow }, max_error),
        Shape::Polygon { .. } => shape_to_polygon(shape, c + max_error, max_error),
        Shape::Strokes { .. } | Shape::Compound { .. } => polygons_outside(shape, clearance, max_error).into_iter().next().unwrap_or_default(),
    }
}

/// [`shape_to_polygon_outside`] for a shape of several outlines: one per segment of a stroke and per part of a compound. Growing a
/// union by a clearance is the union of the parts grown by it, so the knockout of a custom pad is these outlines together.
pub fn polygons_outside(shape: &Shape, clearance: i64, max_error: i64) -> Vec<LineChain> {
    match shape {
        Shape::Strokes { segs, r } => segs.iter().map(|&(a, b)| shape_to_polygon_outside(&Shape::Stadium { a, b, r: *r }, clearance, max_error)).collect(),
        Shape::Compound { parts } => parts.iter().flat_map(|p| polygons_outside(p, clearance, max_error)).collect(),
        _ => {
            let poly = shape_to_polygon_outside(shape, clearance, max_error);
            if poly.len() >= 3 {
                vec![poly]
            } else {
                Vec::new()
            }
        }
    }
}

/// `BuildConvexHull` (`libs/kimath`, Andrew's monotone chain): the convex hull of the points of `outlines`, counter-clockwise.
pub fn convex_hull(outlines: &[LineChain]) -> LineChain {
    let mut pts: Vec<(i64, i64)> = outlines.iter().flatten().map(|p| (p.x, p.y)).collect();
    pts.sort_unstable();
    pts.dedup();
    if pts.len() < 3 {
        return pts.into_iter().map(|(x, y)| Point64::new(x, y)).collect();
    }
    let cross = |o: (i64, i64), a: (i64, i64), b: (i64, i64)| (a.0 - o.0) as i128 * (b.1 - o.1) as i128 - (a.1 - o.1) as i128 * (b.0 - o.0) as i128;
    let mut hull: Vec<(i64, i64)> = Vec::with_capacity(pts.len() * 2);
    for &p in &pts {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0 {
            hull.pop();
        }
        hull.push(p);
    }
    let lower = hull.len() + 1;
    for &p in pts.iter().rev().skip(1) {
        while hull.len() >= lower && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0 {
            hull.pop();
        }
        hull.push(p);
    }
    hull.pop();
    hull.into_iter().map(|(x, y)| Point64::new(x, y)).collect()
}

/// The shape's axis-aligned bounding box, `(x0, y0, x1, y1)`.
pub fn bounds(shape: &Shape) -> (i64, i64, i64, i64) {
    match shape {
        Shape::Circle { c, r } => (c.x - r, c.y - r, c.x + r, c.y + r),
        Shape::Rect { x0, y0, x1, y1 } | Shape::RoundRect { x0, y0, x1, y1, .. } => (*x0, *y0, *x1, *y1),
        Shape::Stadium { a, b, r } => (a.x.min(b.x) - r, a.y.min(b.y) - r, a.x.max(b.x) + r, a.y.max(b.y) + r),
        Shape::Polygon { pts } => {
            let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
            for p in pts {
                x0 = x0.min(p.x);
                y0 = y0.min(p.y);
                x1 = x1.max(p.x);
                y1 = y1.max(p.y);
            }
            (x0, y0, x1, y1)
        }
        Shape::Strokes { segs, r } => {
            let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
            for &(a, b) in segs {
                x0 = x0.min(a.x).min(b.x);
                y0 = y0.min(a.y).min(b.y);
                x1 = x1.max(a.x).max(b.x);
                y1 = y1.max(a.y).max(b.y);
            }
            (x0 - r, y0 - r, x1 + r, y1 + r)
        }
        Shape::Compound { parts } => parts.iter().map(bounds).fold((i64::MAX, i64::MAX, i64::MIN, i64::MIN), |a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))),
    }
}
