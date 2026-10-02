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
    }
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
    }
}
