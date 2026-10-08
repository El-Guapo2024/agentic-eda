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

// ---------------------------------------------------------------------
// Ports of KiCad's router hulls (`pns_utils.cpp`, `pns_via.cpp`,
// `pns_solid.cpp`). KiCad's router walks around *octagons*, not rounded
// outlines: these decide where a walked/shoved track ends up. Every hull
// is returned clockwise in KiCad's sense (a positive shoelace sum in
// raw, y-down coordinates), which `LINE::Walkaround` relies on.
// ---------------------------------------------------------------------

/// KiCad builds hulls in nm, where rounding a vertex loses under 1 nm; at
/// this IR's whole-µm resolution it can lose up to ~1 µm, enough for a
/// hull-hugging path to sit 1 µm inside the rule. Every router hull is
/// grown by this much so its output still meets clearance exactly.
pub const HULL_ROUNDING_GUARD: Um = 1;

fn kiround(v: f64) -> Um {
    v.round() as Um
}

/// `VECTOR2I::Resize` (integral): same direction, length `len`.
pub(crate) fn resize(x: Um, y: Um, len: Um) -> (Um, Um) {
    if x == 0 && y == 0 {
        return (0, 0);
    }
    let (nx, ny) = if x.abs() == y.abs() {
        let v = (len.abs() as f64) * std::f64::consts::FRAC_1_SQRT_2;
        (v, v)
    } else {
        let (xs, ys) = ((x as f64).powi(2), (y as f64).powi(2));
        let l = xs + ys;
        let n = (len as f64).powi(2);
        ((n * xs / l).sqrt(), (n * ys / l).sqrt())
    };
    let s = len.signum();
    ((if x < 0 { -kiround(nx) } else { kiround(nx) }) * s, (if y < 0 { -kiround(ny) } else { kiround(ny) }) * s)
}

fn shoelace(pts: &[Point]) -> i128 {
    let n = pts.len();
    (0..n).map(|i| pts[i].x as i128 * pts[(i + 1) % n].y as i128 - pts[(i + 1) % n].x as i128 * pts[i].y as i128).sum()
}

/// Reverse `pts` if needed so it winds clockwise in KiCad's sense.
pub fn make_clockwise(mut pts: Vec<Point>) -> Vec<Point> {
    if shoelace(&pts) < 0 {
        pts.reverse();
    }
    pts
}

/// `OctagonalHull( aP0, aSize, aClearance, aChamfer )`.
pub fn octagonal_hull(p0: Point, size: (Um, Um), cl: Um, chamfer: Um) -> Vec<Point> {
    let p = |x, y| Point { x, y };
    let mut s = vec![p(p0.x - cl, p0.y - cl + chamfer)];
    if chamfer != 0 {
        s.push(p(p0.x - cl + chamfer, p0.y - cl));
    }
    s.push(p(p0.x + size.0 + cl - chamfer, p0.y - cl));
    if chamfer != 0 {
        s.push(p(p0.x + size.0 + cl, p0.y - cl + chamfer));
    }
    s.push(p(p0.x + size.0 + cl, p0.y + size.1 + cl - chamfer));
    if chamfer != 0 {
        s.push(p(p0.x + size.0 + cl - chamfer, p0.y + size.1 + cl));
    }
    s.push(p(p0.x - cl + chamfer, p0.y + size.1 + cl));
    if chamfer != 0 {
        s.push(p(p0.x - cl, p0.y + size.1 + cl - chamfer));
    }
    s
}

fn is_45(dx: Um, dy: Um) -> bool {
    dx == 0 || dy == 0 || dx.abs() == dy.abs()
}

/// `SegmentHull( aSeg, aClearance, aWalkaroundThickness )`.
pub fn segment_hull(a: Point, b0: Point, width: Um, clearance: Um, walk: Um) -> Vec<Point> {
    let kink = clearance / 10;
    let mut cl = clearance + walk / 2;
    let mut b = b0;
    let len = (((b.x - a.x) as f64).powi(2) + ((b.y - a.y) as f64).powi(2)).sqrt() as Um;
    let (mut w, mut h) = (b.x - a.x, b.y - a.y);
    if a != b {
        if !is_45(w, h) {
            if len <= kink && len > 0 {
                let ll = w.abs().max(h.abs());
                b = Point { x: a.x + w.signum() * ll, y: a.y + h.signum() * ll };
            }
        } else if len <= kink {
            let delta45 = (w.abs() - h.abs()).abs();
            if w.abs() <= 1 {
                w = 0;
                cl += 1;
            } else if h.abs() <= 1 {
                h = 0;
                cl += 1;
            } else if delta45 <= 2 {
                let m = w.abs().max(h.abs());
                w = w.signum() * m;
                h = h.signum() * m;
                cl += 2;
            }
            b = Point { x: a.x + w, y: a.y + h };
        }
    }
    let d = width as f64 / 2.0 + cl as f64;
    if a == b {
        let xx2 = kiround(2.0 * (1.0 - std::f64::consts::FRAC_1_SQRT_2) * d);
        return make_clockwise(octagonal_hull(Point { x: a.x - width / 2, y: a.y - width / 2 }, (width, width), cl, xx2));
    }
    let x = 2.0 / (1.0 + std::f64::consts::SQRT_2) * d;
    let dr = kiround(d);
    let xr2 = kiround(x / 2.0);
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let (px, py) = (-dy, dx); // Perpendicular()
    let p0 = resize(px, py, dr);
    let ds = resize(px, py, xr2);
    let pd = resize(dx, dy, xr2);
    let dp = resize(dx, dy, dr);
    let add = |p: Point, v: (Um, Um)| Point { x: p.x + v.0, y: p.y + v.1 };
    let neg = |v: (Um, Um)| (-v.0, -v.1);
    let sum = |u: (Um, Um), v: (Um, Um)| (u.0 + v.0, u.1 + v.1);
    let s = vec![
        add(b, sum(p0, pd)),
        add(b, sum(dp, ds)),
        add(b, sum(dp, neg(ds))),
        add(b, sum(neg(p0), pd)),
        add(a, sum(neg(p0), neg(pd))),
        add(a, sum(neg(dp), neg(ds))),
        add(a, sum(neg(dp), ds)),
        add(a, sum(p0, neg(pd))),
    ];
    // "make sure the hull outline is always clockwise": s.CSegment(0).Side(a) < 0 -> reverse.
    let (s0, s1) = (s[0], s[1]);
    let side = (s1.x - s0.x) as i128 * (a.y - s0.y) as i128 - (s1.y - s0.y) as i128 * (a.x - s0.x) as i128;
    if side < 0 {
        s.into_iter().rev().collect()
    } else {
        s
    }
}

/// `VIA::Hull`: an equilateral octagon.
pub fn via_hull(pos: Point, diameter: Um, clearance: Um, walk: Um) -> Vec<Point> {
    let cl = clearance + walk / 2;
    let chamfer = ((2 * cl + diameter) as f64 * (1.0 - std::f64::consts::FRAC_1_SQRT_2)) as Um;
    octagonal_hull(Point { x: pos.x - diameter / 2, y: pos.y - diameter / 2 }, (diameter, diameter), cl, chamfer)
}

/// Line-line intersection (`SEG::IntersectLines`).
fn intersect_lines(a: (Point, Point), b: (Point, Point)) -> Option<Point> {
    let (x1, y1, x2, y2) = (a.0.x as f64, a.0.y as f64, a.1.x as f64, a.1.y as f64);
    let (x3, y3, x4, y4) = (b.0.x as f64, b.0.y as f64, b.1.x as f64, b.1.y as f64);
    let den = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
    if den == 0.0 {
        return None;
    }
    let t = ((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / den;
    Some(Point { x: kiround(x1 + t * (x2 - x1)), y: kiround(y1 + t * (y2 - y1)) })
}

/// `SEG::LineDistance( p )` (seg.cpp:746): `isqrt( rescale( det, det, l ) )`,
/// i.e. the floor of the rounded squared distance to the infinite line.
fn line_distance(s: (Point, Point), q: Point) -> Um {
    let (p, qq) = ((s.0.y - s.1.y) as i128, (s.1.x - s.0.x) as i128);
    let r = -p * s.0.x as i128 - qq * s.0.y as i128;
    let l = p * p + qq * qq;
    let det = p * q.x as i128 + qq * q.y as i128 + r;
    let dist_sq = if l > 0 {
        let n = det * det;
        (n + l / 2) / l
    } else {
        0
    };
    let mut x = (dist_sq as f64).sqrt() as i128;
    while x * x > dist_sq {
        x -= 1;
    }
    while (x + 1) * (x + 1) <= dist_sq {
        x += 1;
    }
    x as Um
}

/// `ConvexHull( SHAPE_SIMPLE, aClearance )`: an octagon of the bbox
/// (inflated by the clearance) and four 45-degree diagonals moved in to
/// `aClearance` from the polygon (`MoveDiagonal`).
pub fn convex_hull_octagon(vertices: &[Point], clearance: Um) -> Vec<Point> {
    let (mut x0, mut y0, mut x1, mut y1) = (Um::MAX, Um::MAX, Um::MIN, Um::MIN);
    for p in vertices {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    let (bx, by, bw, bh) = (x0 - clearance, y0 - clearance, x1 - x0 + 2 * clearance, y1 - y0 + 2 * clearance);
    let p = |x, y| Point { x, y };
    let topline = (p(bx, by + bh), p(bx + bw, by + bh));
    let rightline = (p(bx + bw, by + bh), p(bx + bw, by));
    let bottomline = (p(bx + bw, by), p(bx, by));
    let leftline = (p(bx, by), p(bx, by + bh));
    // `SHAPE_LINE_CHAIN::NearestPoint( SEG, dist )`: minimum over the polygon's
    // *vertices* of `SEG::LineDistance` (distance to the infinite line).
    let seg_dist = |d: (Point, Point)| -> Um { vertices.iter().map(|&v| line_distance(d, v)).min().unwrap_or(Um::MAX) };
    let mv = |d: (Point, Point)| -> (Point, Point) {
        let dist = seg_dist(d);
        let (vx, vy) = (d.0.x - d.1.x, d.0.y - d.1.y);
        let m = resize(-vy, vx, dist - clearance);
        (Point { x: d.0.x + m.0, y: d.0.y + m.1 }, Point { x: d.1.x + m.0, y: d.1.y + m.1 })
    };
    let c = p(bx + bw, by + bh);
    let tr = mv((c, p(c.x + bh, c.y - bh)));
    let c = p(bx + bw, by);
    let br = mv((p(c.x + bh, c.y + bh), c));
    let c = p(bx, by);
    let bl = mv((c, p(c.x - bh, c.y + bh)));
    let c = p(bx, by + bh);
    let tl = mv((p(c.x - bh, c.y - bh), c));
    let pts: Vec<Point> = [(leftline, bl), (bottomline, bl), (bottomline, br), (rightline, br), (rightline, tr), (topline, tr), (topline, tl), (leftline, tl)].iter().filter_map(|&(a, b)| intersect_lines(a, b)).collect();
    make_clockwise(pts)
}

/// `BuildHullForPrimitiveShape` + `SOLID::Hull`'s compound union, for the
/// shapes this crate's items carry. A rounded rectangle is KiCad's
/// compound (inner rect + one segment per edge, `PAD::buildEffectiveShape`),
/// hulled per primitive and unioned.
pub fn primitive_hull(shape: &Shape, clearance: Um, walk: Um) -> Vec<Point> {
    let cl = clearance + (walk + 1) / 2;
    match *shape {
        Shape::Circle { c, r } => make_clockwise(octagonal_hull(Point { x: c.x - r, y: c.y - r }, (2 * r, 2 * r), cl, (2.0 * (1.0 - std::f64::consts::FRAC_1_SQRT_2) * (r + cl) as f64) as Um)),
        Shape::Stadium { a, b, r } => segment_hull(a, b, 2 * r, clearance, walk),
        Shape::Rect { x0, y0, x1, y1 } => make_clockwise(octagonal_hull(Point { x: x0, y: y0 }, (x1 - x0, y1 - y0), cl, 0)),
        Shape::RoundRect { x0, y0, x1, y1, r } => {
            let (ix0, iy0, ix1, iy1) = (x0 + r, y0 + r, x1 - r, y1 - r);
            if ix1 - ix0 <= 0 && iy1 - iy0 <= 0 {
                let c = Point { x: (x0 + x1) / 2, y: (y0 + y1) / 2 };
                return primitive_hull(&Shape::Circle { c, r }, clearance, walk);
            }
            let corners = [Point { x: ix0, y: iy1 }, Point { x: ix1, y: iy1 }, Point { x: ix1, y: iy0 }, Point { x: ix0, y: iy0 }];
            let mut hulls = vec![make_clockwise(octagonal_hull(Point { x: ix0, y: iy0 }, (ix1 - ix0, iy1 - iy0), cl, 0))];
            for i in 0..4 {
                hulls.push(segment_hull(corners[i], corners[(i + 1) % 4], 2 * r, clearance, walk));
            }
            union_outline(&hulls)
        }
        Shape::Polygon { ref pts } => convex_hull_octagon(pts, cl),
        _ => hull_of(shape, clearance, walk),
    }
}

/// `SHAPE_POLY_SET::Simplify()` of several hulls, then `Outline( 0 )`.
fn union_outline(hulls: &[Vec<Point>]) -> Vec<Point> {
    use eda_shape_poly_set::ShapePolySet;
    let mut set = ShapePolySet::default();
    for h in hulls.iter().filter(|h| h.len() >= 3) {
        set.add_outline(h.iter().map(|p| eda_clipper2::Point64::new(p.x, p.y)).collect());
    }
    set.simplify();
    let out: Vec<Point> = set.polys.first().and_then(|p| p.first()).map(|c| c.iter().map(|q| Point { x: q.x, y: q.y }).collect()).unwrap_or_default();
    make_clockwise(out)
}

#[cfg(test)]
mod kicad_hull_tests {
    use super::*;

    #[test]
    fn segment_hull_is_a_clockwise_octagon_at_the_right_offset() {
        // Horizontal 1 mm segment, width 200, clearance 100, walk 0:
        // d = 100 + 100 = 200 -> the long edges sit at y = +-200.
        let h = segment_hull(Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }, 200, 100, 0);
        assert_eq!(h.len(), 8);
        assert!(shoelace(&h) > 0, "clockwise in KiCad's sense");
        assert!(h.iter().any(|p| p.y == 200) && h.iter().any(|p| p.y == -200));
        assert!(h.iter().any(|p| p.x == 1200) && h.iter().any(|p| p.x == -200));
    }

    #[test]
    fn via_hull_is_an_equilateral_octagon() {
        let h = via_hull(Point { x: 0, y: 0 }, 600, 200, 0);
        assert_eq!(h.len(), 8);
        // bbox = 300 + 200 on each side.
        assert_eq!(h.iter().map(|p| p.x).max(), Some(500));
        assert_eq!(h.iter().map(|p| p.y).min(), Some(-500));
    }

    /// D4: a 60-degree rotated 1.0 x 0.6 mm rectangle must keep the hull at
    /// least `cl` (minus rounding) from every pad edge; the old
    /// segment-distance version left a 64 um shortfall.
    #[test]
    fn rotated_polygon_pad_hull_keeps_clearance() {
        let (w, h, cl) = (500.0f64, 300.0f64, 300);
        for deg in [10.0f64, 30.0, 55.0, 60.0, 70.0, 85.0] {
            let (sn, cs) = deg.to_radians().sin_cos();
            let poly: Vec<Point> = [(-w, -h), (w, -h), (w, h), (-w, h)].iter().map(|&(x, y)| Point { x: (x * cs - y * sn).round() as Um, y: (x * sn + y * cs).round() as Um }).collect();
            let hull = convex_hull_octagon(&poly, cl);
            let mut min_d = f64::INFINITY;
            for i in 0..hull.len() {
                let (a, b) = (hull[i], hull[(i + 1) % hull.len()]);
                for j in 0..4 {
                    let (c, d) = (poly[j], poly[(j + 1) % 4]);
                    let sq = eda_drc::kimath::Seg::new(a, b).sq_distance_to_seg(&eda_drc::kimath::Seg::new(c, d));
                    min_d = min_d.min((sq as f64).sqrt());
                }
            }
            assert!(min_d >= (cl - 2) as f64, "deg {deg}: hull only {min_d} from pad, need {cl}");
        }
    }

    #[test]
    fn rect_pad_hull_has_square_corners() {
        let h = primitive_hull(&Shape::Rect { x0: 0, y0: 0, x1: 1000, y1: 500 }, 100, 0);
        assert_eq!(h.len(), 4);
    }

    #[test]
    fn roundrect_hull_is_one_outline_around_the_pad() {
        let h = primitive_hull(&Shape::RoundRect { x0: 0, y0: 0, x1: 1000, y1: 600, r: 150 }, 100, 0);
        assert!(h.len() >= 8);
        assert!(h.iter().map(|p| p.x).max().unwrap() >= 1100);
    }
}
