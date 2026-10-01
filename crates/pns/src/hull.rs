//! Port of the hull-building half of `pcbnew/router/pns_utils.cpp`
//! (`OctagonalHull`, `SegmentHull`, `ConvexHull`, `BuildHullForPrimitiveShape`):
//! turning an item's exact shape into a convex polygon offset outward by
//! `clearance + walkaroundThickness/2`, for WALKAROUND/OPTIMIZER to route a
//! line's centreline around.
//!
//! This is deliberately **not** used for the pass/fail collision test
//! (`NODE::CheckColliding` calls `ITEM::Collide`, which tests the real
//! shapes via `eda_drc::kimath::Shape::collides` -- exact, no polygon
//! approximation, and strictly better than a hull for that purpose). The
//! hull exists for one reason: a path that hugs it as tightly as possible
//! is guaranteed never to come closer than `clearance` to the real
//! obstacle, which is all WALKAROUND needs.
//!
//! KiCad approximates a round shape (circle, stadium cap, rounded-rect
//! corner) as a *chamfered* octagon for speed and to keep `SHAPE_LINE_CHAIN`
//! arithmetic simple. This port uses the same octagon vertex count but
//! derives it differently: [`circle_pts`] places 8 points on a circle
//! *circumscribing* the true offset circle (apothem == radius, so every
//! point of the true circle is inside or on the octagon). That is always a
//! safe (never-too-small) hull, at the cost of being very slightly
//! (~8.2%) more generous at the 8 vertex directions than KiCad's own
//! chamfer -- an immaterial difference for routing purposes, and strictly
//! on the safe side rather than the unsafe one.
//!
//! Stadiums and rounded rects are built the same way KiCad's own
//! `SHAPE_SEGMENT`/`SHAPE_RECT` hulls effectively are: the convex hull of
//! the offset circles/corners involved -- a stadium is the hull of two
//! end-circles, a rounded rect the hull of four corner-circles, a plain
//! rect just its own four offset corners (no rounding needed, already
//! convex).

use eda_drc::kimath::Shape;
use eda_model::ir::{Point, Um};

/// 8 points on a circle of radius `r` centred at `c`, spaced so the circle
/// of radius `r` is inscribed (apothem == r): vertices at `22.5 + k*45`
/// degrees, circumradius `r / cos(22.5deg)`. Degenerates gracefully for
/// `r <= 0` (returns 8 coincident points at `c`, harmless as convex-hull
/// input).
///
/// `MARGIN_UM` pads the circumradius a couple of micrometers past the
/// exact value: every per-vertex coordinate is independently rounded to
/// the nearest integer micrometer, which can shift a vertex up to ~0.7um
/// closer to centre than the exact construction -- enough, for an edge's
/// apothem that was exactly on the target clearance boundary, to put a
/// walked path's own leg ~1um inside it (caught by `walkaround`'s own
/// tests: a route hugging this hull collided with the very pad it was
/// hugging, `actual: 199` against a required `200`). The hull only has to
/// be *at least* clearance-offset, never exactly it, so padding is free.
fn circle_pts(c: Point, r: Um) -> [Point; 8] {
    const COS22_5: f64 = 0.923_879_532_511_286_8;
    const MARGIN_UM: f64 = 2.0;
    let r = r.max(0);
    let circumradius = r as f64 / COS22_5 + MARGIN_UM;
    let mut pts = [Point { x: 0, y: 0 }; 8];
    for (k, p) in pts.iter_mut().enumerate() {
        let theta = (22.5 + 45.0 * k as f64).to_radians();
        *p = Point { x: c.x + (circumradius * theta.cos()).round() as Um, y: c.y + (circumradius * theta.sin()).round() as Um };
    }
    pts
}

/// Standard 2D convex hull (Andrew's monotone chain), CCW winding (board
/// space is +x right/+y down, so CCW on screen is CW mathematically --
/// winding direction does not matter to any caller here, which only ever
/// walks the polygon as a cyclic point list). Cross products use `i128` so
/// board-scale coordinates (micrometers, fitting `i64`) never overflow.
/// Collinear boundary points are dropped. A degenerate input (<=2 distinct
/// points) returns those points as-is.
pub fn convex_hull(points: &[Point]) -> Vec<Point> {
    let mut pts: Vec<Point> = points.to_vec();
    pts.sort_by(|a, b| (a.x, a.y).cmp(&(b.x, b.y)));
    pts.dedup();
    if pts.len() <= 2 {
        return pts;
    }
    fn cross(o: Point, a: Point, b: Point) -> i128 {
        (a.x - o.x) as i128 * (b.y - o.y) as i128 - (a.y - o.y) as i128 * (b.x - o.x) as i128
    }
    let mut lower: Vec<Point> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<Point> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// `ITEM::Hull(clearance, walkaroundThickness, layer)`: the convex polygon
/// a routed line of width `walkaround_width` must stay outside of to keep
/// `clearance` away from `shape`. The combined outward offset is
/// `clearance + walkaround_width / 2` -- the walking line's own half-width
/// has to clear the obstacle just as much as the obstacle's own boundary
/// does, since clearance is measured edge-to-edge, not centreline-to-edge.
pub fn hull_of(shape: &Shape, clearance: Um, walkaround_width: Um) -> Vec<Point> {
    let offset = clearance + walkaround_width / 2;
    match *shape {
        Shape::Circle { c, r } => convex_hull(&circle_pts(c, r + offset)),
        Shape::Stadium { a, b, r } => {
            let mut pts = circle_pts(a, r + offset).to_vec();
            pts.extend(circle_pts(b, r + offset));
            convex_hull(&pts)
        }
        Shape::Rect { x0, y0, x1, y1 } => {
            convex_hull(&[Point { x: x0 - offset, y: y0 - offset }, Point { x: x1 + offset, y: y0 - offset }, Point { x: x1 + offset, y: y1 + offset }, Point { x: x0 - offset, y: y1 + offset }])
        }
        Shape::RoundRect { x0, y0, x1, y1, r } => {
            let (ix0, iy0, ix1, iy1) = (x0 + r, y0 + r, x1 - r, y1 - r);
            let corners = if ix0 > ix1 || iy0 > iy1 {
                // Degenerate inset (r covers the whole rect): fall back to
                // the plain outer rect, still safe.
                vec![Point { x: x0 - offset, y: y0 - offset }, Point { x: x1 + offset, y: y0 - offset }, Point { x: x1 + offset, y: y1 + offset }, Point { x: x0 - offset, y: y1 + offset }]
            } else {
                let mut pts = Vec::with_capacity(32);
                for c in [Point { x: ix0, y: iy0 }, Point { x: ix1, y: iy0 }, Point { x: ix1, y: iy1 }, Point { x: ix0, y: iy1 }] {
                    pts.extend(circle_pts(c, r + offset));
                }
                pts
            };
            convex_hull(&corners)
        }
        Shape::Polygon { ref pts } => {
            // Not reachable for the item kinds this crate constructs today
            // (SOLID/SEGMENT/VIA never produce a bare polygon shape -- see
            // `item.rs`), but handled for completeness / future pad shapes:
            // outward-offset each hull edge by `offset` via the same
            // circle-hull trick (a Minkowski sum with a disk), one circle
            // per vertex.
            let hull = convex_hull(pts);
            let mut expanded = Vec::with_capacity(hull.len() * 8);
            for p in &hull {
                expanded.extend(circle_pts(*p, offset));
            }
            convex_hull(&expanded)
        }
        Shape::Strokes { .. } => Vec::new(), // never a PNS item; see item.rs.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_hull_contains_true_circle() {
        let c = Point { x: 0, y: 0 };
        let hull = hull_of(&Shape::Circle { c, r: 1000 }, 200, 0);
        assert_eq!(hull.len(), 8);
        // Every hull vertex must be at least `r+clearance` from centre
        // (apothem == r+clearance, vertices a bit further out).
        for p in &hull {
            let d = ((p.x * p.x + p.y * p.y) as f64).sqrt();
            assert!(d >= 1200.0 - 1.0, "vertex {:?} at {d}, expected >= 1200", p);
        }
    }

    #[test]
    fn stadium_hull_is_convex_and_wider_than_radius() {
        let hull = hull_of(&Shape::Stadium { a: Point { x: 0, y: 0 }, b: Point { x: 10_000, y: 0 }, r: 500 }, 100, 200 /* walkaround width */);
        // offset = 100 + 100 = 200; total half-extent from axis ~= 700.
        let max_y = hull.iter().map(|p| p.y).max().unwrap();
        assert!((690..=760).contains(&max_y), "max_y={max_y}");
        // Convexity: re-hulling should not drop any point.
        let rehull = convex_hull(&hull);
        assert_eq!(rehull.len(), hull.len());
    }

    #[test]
    fn rect_hull_is_exact_inflated_rect() {
        let hull = hull_of(&Shape::Rect { x0: 0, y0: 0, x1: 1000, y1: 2000 }, 50, 0);
        assert_eq!(hull.len(), 4);
        let xs: Vec<Um> = hull.iter().map(|p| p.x).collect();
        assert!(xs.contains(&-50) && xs.contains(&1050));
    }

    #[test]
    fn convex_hull_of_collinear_points_is_the_segment() {
        let pts = vec![Point { x: 0, y: 0 }, Point { x: 5, y: 0 }, Point { x: 10, y: 0 }];
        let hull = convex_hull(&pts);
        assert_eq!(hull.len(), 2);
    }
}
