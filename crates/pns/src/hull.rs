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
use eda_model::ir::{Point, Nm};

/// 8 points on a circle of radius `r` centred at `c`, spaced so the circle
/// of radius `r` is inscribed (apothem == r): vertices at `22.5 + k*45`
/// degrees, circumradius `r / cos(22.5deg)`. Degenerates gracefully for
/// `r <= 0` (returns 8 coincident points at `c`, harmless as convex-hull
/// input).
///
/// `MARGIN_NM` pads the circumradius a couple of micrometers past the
/// exact value: every per-vertex coordinate is independently rounded to
/// the nearest integer micrometer, which can shift a vertex up to ~0.7um
/// closer to centre than the exact construction -- enough, for an edge's
/// apothem that was exactly on the target clearance boundary, to put a
/// walked path's own leg ~1um inside it (caught by `walkaround`'s own
/// tests: a route hugging this hull collided with the very pad it was
/// hugging, `actual: 199` against a required `200`). The hull only has to
/// be *at least* clearance-offset, never exactly it, so padding is free.
fn circle_pts(c: Point, r: Nm) -> [Point; 8] {
    const COS22_5: f64 = 0.923_879_532_511_286_8;
    const MARGIN_NM: f64 = 2.0;
    let r = r.max(Nm::ZERO);
    let circumradius = r.as_f64() / COS22_5 + MARGIN_NM;
    let mut pts = [Point { x: Nm::ZERO, y: Nm::ZERO }; 8];
    for (k, p) in pts.iter_mut().enumerate() {
        let theta = (22.5 + 45.0 * k as f64).to_radians();
        *p = Point { x: c.x + Nm::kiround(circumradius * theta.cos()), y: c.y + Nm::kiround(circumradius * theta.sin()) };
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
        (a.x - o.x).area(b.y - o.y) - (a.y - o.y).area(b.x - o.x)
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
pub fn hull_of(shape: &Shape, clearance: Nm, walkaround_width: Nm) -> Vec<Point> {
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
        // A custom pad: the convex hull of the hulls of its parts. (The router's own hull, `primitive_hull`, keeps them apart.)
        Shape::Compound { ref parts } => convex_hull(&parts.iter().flat_map(|p| hull_of(p, clearance, walkaround_width)).collect::<Vec<_>>()),
    }
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;

    #[test]
    fn circle_hull_contains_true_circle() {
        let c = Point { x: Nm::ZERO, y: Nm::ZERO };
        let hull = hull_of(&Shape::Circle { c, r: Nm::new(1000) }, Nm::new(200), Nm::ZERO);
        assert_eq!(hull.len(), 8);
        // Every hull vertex must be at least `r+clearance` from centre
        // (apothem == r+clearance, vertices a bit further out).
        for p in &hull {
            let d = ((p.x.area(p.x) + p.y.area(p.y)) as f64).sqrt();
            assert!(d >= 1200.0 - 1.0, "vertex {:?} at {d}, expected >= 1200", p);
        }
    }

    #[test]
    fn stadium_hull_is_convex_and_wider_than_radius() {
        let hull = hull_of(&Shape::Stadium { a: Point { x: Nm::ZERO, y: Nm::ZERO }, b: Point { x: mm(0.01), y: Nm::ZERO }, r: Nm::new(500) }, Nm::new(100), Nm::new(200) /* walkaround width */);
        // offset = 100 + 100 = 200; total half-extent from axis ~= 700.
        let max_y = hull.iter().map(|p| p.y).max().unwrap();
        assert!((Nm::new(690)..=Nm::new(760)).contains(&max_y), "max_y={max_y}");
        // Convexity: re-hulling should not drop any point.
        let rehull = convex_hull(&hull);
        assert_eq!(rehull.len(), hull.len());
    }

    #[test]
    fn rect_hull_is_exact_inflated_rect() {
        let hull = hull_of(&Shape::Rect { x0: Nm::ZERO, y0: Nm::ZERO, x1: Nm::new(1000), y1: Nm::new(2000) }, Nm::new(50), Nm::ZERO);
        assert_eq!(hull.len(), 4);
        let xs: Vec<Nm> = hull.iter().map(|p| p.x).collect();
        assert!(xs.contains(&Nm::new(-50)) && xs.contains(&Nm::new(1050)));
    }

    #[test]
    fn convex_hull_of_collinear_points_is_the_segment() {
        let pts = vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::new(5), y: Nm::ZERO }, Point { x: Nm::new(10), y: Nm::ZERO }];
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

/// `PNS_HULL_MARGIN` (`pns_line.h`): 10 nm. (The micrometre-era `HULL_ROUNDING_GUARD`, a micrometre added to every router hull, is gone:
/// on KiCad's nanometre grid the hulls are KiCad's own, findings p5 F1-6.)
pub const PNS_HULL_MARGIN: Nm = Nm::new(10);

fn kiround(v: f64) -> Nm {
    Nm::kiround(v)
}

/// `VECTOR2I::Resize` (integral): same direction, length `len`.
pub(crate) fn resize(x: Nm, y: Nm, len: Nm) -> (Nm, Nm) {
    if x == Nm::ZERO && y == Nm::ZERO {
        return (Nm::ZERO, Nm::ZERO);
    }
    let (nx, ny) = if x.abs() == y.abs() {
        let v = (len.abs().as_f64()) * std::f64::consts::FRAC_1_SQRT_2;
        (v, v)
    } else {
        let (xs, ys) = (((x).as_f64()).powi(2), ((y).as_f64()).powi(2));
        let l = xs + ys;
        let n = ((len).as_f64()).powi(2);
        ((n * xs / l).sqrt(), (n * ys / l).sqrt())
    };
    let s = len.signum();
    ((if x < Nm::ZERO { -kiround(nx) } else { kiround(nx) }) * s, (if y < Nm::ZERO { -kiround(ny) } else { kiround(ny) }) * s)
}

fn shoelace(pts: &[Point]) -> i128 {
    let n = pts.len();
    (0..n).map(|i| pts[i].x.get() as i128 * pts[(i + 1) % n].y.get() as i128 - pts[(i + 1) % n].x.area(pts[i].y)).sum()
}

/// Reverse `pts` if needed so it winds clockwise in KiCad's sense.
/// The 90-degree corner modes' hull (`WALKAROUND::processCluster`, `NODE::NearestObstacle`'s `makeHull`): the bounding box of `hull`, as
/// `hull.Append( left, top ); Append( right, top ); Append( right, bottom ); Append( left, bottom )`.
pub fn bbox_hull(hull: &[Point]) -> Vec<Point> {
    let Some(first) = hull.first() else { return Vec::new() };
    let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x, first.y);
    for p in hull {
        (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
    }
    vec![Point { x: x0, y: y0 }, Point { x: x1, y: y0 }, Point { x: x1, y: y1 }, Point { x: x0, y: y1 }]
}

pub fn make_clockwise(mut pts: Vec<Point>) -> Vec<Point> {
    if shoelace(&pts) < 0 {
        pts.reverse();
    }
    pts
}

/// `OctagonalHull( aP0, aSize, aClearance, aChamfer )`.
pub fn octagonal_hull(p0: Point, size: (Nm, Nm), cl: Nm, chamfer: Nm) -> Vec<Point> {
    let p = |x, y| Point { x, y };
    let mut s = vec![p(p0.x - cl, p0.y - cl + chamfer)];
    if chamfer != Nm::ZERO {
        s.push(p(p0.x - cl + chamfer, p0.y - cl));
    }
    s.push(p(p0.x + size.0 + cl - chamfer, p0.y - cl));
    if chamfer != Nm::ZERO {
        s.push(p(p0.x + size.0 + cl, p0.y - cl + chamfer));
    }
    s.push(p(p0.x + size.0 + cl, p0.y + size.1 + cl - chamfer));
    if chamfer != Nm::ZERO {
        s.push(p(p0.x + size.0 + cl - chamfer, p0.y + size.1 + cl));
    }
    s.push(p(p0.x - cl + chamfer, p0.y + size.1 + cl));
    if chamfer != Nm::ZERO {
        s.push(p(p0.x - cl, p0.y + size.1 + cl - chamfer));
    }
    s
}

fn is_45(dx: Nm, dy: Nm) -> bool {
    dx == Nm::ZERO || dy == Nm::ZERO || dx.abs() == dy.abs()
}

/// `SegmentHull( aSeg, aClearance, aWalkaroundThickness )`.
pub fn segment_hull(a: Point, b0: Point, width: Nm, clearance: Nm, walk: Nm) -> Vec<Point> {
    let kink = clearance / 10;
    let mut cl = clearance + walk / 2;
    let mut b = b0;
    let len = Nm::new(((b.x - a.x).as_f64().powi(2) + (b.y - a.y).as_f64().powi(2)).sqrt() as i64);
    let (mut w, mut h) = (b.x - a.x, b.y - a.y);
    if a != b {
        if !is_45(w, h) {
            if len <= kink && len > Nm::ZERO {
                let ll = w.abs().max(h.abs());
                b = Point { x: a.x + w.signum() * ll, y: a.y + h.signum() * ll };
            }
        } else if len <= kink {
            let delta45 = (w.abs() - h.abs()).abs();
            if w.abs() <= Nm::new(1) {
                w = Nm::ZERO;
                cl += Nm::new(1);
            } else if h.abs() <= Nm::new(1) {
                h = Nm::ZERO;
                cl += Nm::new(1);
            } else if delta45 <= Nm::new(2) {
                let m = w.abs().max(h.abs());
                w = w.signum() * m;
                h = h.signum() * m;
                cl += Nm::new(2);
            }
            b = Point { x: a.x + w, y: a.y + h };
        }
    }
    let d = width.as_f64() / 2.0 + cl.as_f64();
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
    let add = |p: Point, v: (Nm, Nm)| Point { x: p.x + v.0, y: p.y + v.1 };
    let neg = |v: (Nm, Nm)| (-v.0, -v.1);
    let sum = |u: (Nm, Nm), v: (Nm, Nm)| (u.0 + v.0, u.1 + v.1);
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
    let side = (s1.x - s0.x).area(a.y - s0.y) - (s1.y - s0.y).area(a.x - s0.x);
    if side < 0 {
        s.into_iter().rev().collect()
    } else {
        s
    }
}

/// `VIA::Hull`: an equilateral octagon.
pub fn via_hull(pos: Point, diameter: Nm, clearance: Nm, walk: Nm) -> Vec<Point> {
    let cl = clearance + walk / 2;
    let chamfer = Nm::new(((2 * cl + diameter).as_f64() * (1.0 - std::f64::consts::FRAC_1_SQRT_2)) as i64);
    octagonal_hull(Point { x: pos.x - diameter / 2, y: pos.y - diameter / 2 }, (diameter, diameter), cl, chamfer)
}

/// Line-line intersection (`SEG::IntersectLines`).
pub(crate) fn intersect_lines(a: (Point, Point), b: (Point, Point)) -> Option<Point> {
    let (x1, y1, x2, y2) = (a.0.x.as_f64(), a.0.y.as_f64(), a.1.x.as_f64(), a.1.y.as_f64());
    let (x3, y3, x4, y4) = (b.0.x.as_f64(), b.0.y.as_f64(), b.1.x.as_f64(), b.1.y.as_f64());
    let den = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
    if den == 0.0 {
        return None;
    }
    let t = ((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / den;
    Some(Point { x: kiround(x1 + t * (x2 - x1)), y: kiround(y1 + t * (y2 - y1)) })
}

/// `SEG::LineDistance( p )` (seg.cpp:746): `isqrt( rescale( det, det, l ) )`,
/// i.e. the floor of the rounded squared distance to the infinite line.
fn line_distance(s: (Point, Point), q: Point) -> Nm {
    let (p, qq) = ((s.0.y - s.1.y).get() as i128, (s.1.x - s.0.x).get() as i128);
    let r = -p * s.0.x.get() as i128 - qq * s.0.y.get() as i128;
    let l = p * p + qq * qq;
    let det = p * q.x.get() as i128 + qq * q.y.get() as i128 + r;
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
    Nm::new(x as i64)
}

/// `ConvexHull( SHAPE_SIMPLE, aClearance )`: an octagon of the bbox
/// (inflated by the clearance) and four 45-degree diagonals moved in to
/// `aClearance` from the polygon (`MoveDiagonal`).
pub fn convex_hull_octagon(vertices: &[Point], clearance: Nm) -> Vec<Point> {
    let (mut x0, mut y0, mut x1, mut y1) = (Nm::MAX, Nm::MAX, Nm::MIN, Nm::MIN);
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
    let seg_dist = |d: (Point, Point)| -> Nm { vertices.iter().map(|&v| line_distance(d, v)).min().unwrap_or(Nm::MAX) };
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
pub fn primitive_hull(shape: &Shape, clearance: Nm, walk: Nm) -> Vec<Point> {
    let cl = clearance + (walk + Nm::new(1)) / 2;
    match *shape {
        Shape::Circle { c, r } => make_clockwise(octagonal_hull(Point { x: c.x - r, y: c.y - r }, (2 * r, 2 * r), cl, Nm::new((2.0 * (1.0 - std::f64::consts::FRAC_1_SQRT_2) * (r + cl).as_f64()) as i64))),
        Shape::Stadium { a, b, r } => segment_hull(a, b, 2 * r, clearance, walk),
        Shape::Rect { x0, y0, x1, y1 } => make_clockwise(octagonal_hull(Point { x: x0, y: y0 }, (x1 - x0, y1 - y0), cl, Nm::ZERO)),
        Shape::RoundRect { x0, y0, x1, y1, r } => {
            let (ix0, iy0, ix1, iy1) = (x0 + r, y0 + r, x1 - r, y1 - r);
            if ix1 - ix0 <= Nm::ZERO && iy1 - iy0 <= Nm::ZERO {
                let c = Point { x: (x0 + x1) / 2, y: (y0 + y1) / 2 };
                return primitive_hull(&Shape::Circle { c, r }, clearance, walk);
            }
            let corners = [Point { x: ix0, y: iy1 }, Point { x: ix1, y: iy1 }, Point { x: ix1, y: iy0 }, Point { x: ix0, y: iy0 }];
            let mut hulls = vec![make_clockwise(octagonal_hull(Point { x: ix0, y: iy0 }, (ix1 - ix0, iy1 - iy0), cl, Nm::ZERO))];
            for i in 0..4 {
                hulls.push(segment_hull(corners[i], corners[(i + 1) % 4], 2 * r, clearance, walk));
            }
            union_outline(&hulls)
        }
        Shape::Polygon { ref pts } => convex_hull_octagon(pts, cl),
        // `SOLID::Hull` of a compound: the hull of each primitive, unioned (`hullSet.Simplify(); Outline( 0 )`).
        Shape::Compound { ref parts } => match parts.as_slice() {
            [only] => primitive_hull(only, clearance, walk),
            _ => union_outline(&parts.iter().map(|p| primitive_hull(p, clearance, walk)).collect::<Vec<_>>()),
        },
        // A stroked arc (a custom pad's primitive): a segment per chord, unioned.
        Shape::Strokes { ref segs, r } => union_outline(&segs.iter().map(|s| segment_hull(s.a, s.b, 2 * r, clearance, walk)).collect::<Vec<_>>()),
    }
}

/// `SHAPE_POLY_SET::Simplify()` of several hulls, then `Outline( 0 )`.
fn union_outline(hulls: &[Vec<Point>]) -> Vec<Point> {
    use eda_shape_poly_set::ShapePolySet;
    let mut set = ShapePolySet::default();
    for h in hulls.iter().filter(|h| h.len() >= 3) {
        set.add_outline(h.iter().map(|p| eda_clipper2::Point64::new(p.x.get(), p.y.get())).collect());
    }
    set.simplify();
    let out: Vec<Point> = set.polys.first().and_then(|p| p.first()).map(|c| c.iter().map(|q| Point { x: Nm::new(q.x), y: Nm::new(q.y) }).collect()).unwrap_or_default();
    make_clockwise(out)
}

#[cfg(test)]
mod kicad_hull_tests {
    use super::*;

    #[test]
    fn a_custom_pads_hull_is_the_union_of_the_hulls_of_its_parts() {
        // A 1 mm round anchor and a 200 um arm 2 mm to the right (a custom pad): `SOLID::Hull` hulls each primitive and unions them.
        let pad = Shape::Compound { parts: vec![Shape::Circle { c: Point { x: Nm::ZERO, y: Nm::ZERO }, r: Nm::new(500) }, Shape::Stadium { a: Point { x: Nm::ZERO, y: Nm::ZERO }, b: Point { x: Nm::new(2000), y: Nm::ZERO }, r: Nm::new(100) }] };
        let hull = primitive_hull(&pad, Nm::new(200), Nm::ZERO);
        assert!(hull.len() >= 8, "{hull:?}");
        let (x0, x1) = (hull.iter().map(|p| p.x).min().unwrap(), hull.iter().map(|p| p.x).max().unwrap());
        // The arm's tip is at 2100: its hull reaches 200 past it, which the anchor's alone (to 700) does not.
        assert!(x1 >= Nm::new(2290) && x1 <= Nm::new(2310), "{x1}");
        assert!(x0 <= Nm::new(-690) && x0 >= Nm::new(-710), "{x0}");
        // Where the arm is thin the hull is too: nothing a 700-wide anchor hull would give at x = 1500.
        assert!(hull.iter().filter(|p| p.x > Nm::new(800) && p.x < Nm::new(1900)).all(|p| p.y.abs() <= Nm::new(310)), "{hull:?}");
        // A stroked arc's hull is a union of its chords'.
        let arc = Shape::Strokes { segs: vec![eda_drc::kimath::Seg::new(Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::new(1000), y: Nm::ZERO }), eda_drc::kimath::Seg::new(Point { x: Nm::new(1000), y: Nm::ZERO }, Point { x: Nm::new(1000), y: Nm::new(1000) })], r: Nm::new(100) };
        let hull = primitive_hull(&arc, Nm::new(100), Nm::ZERO);
        assert!(hull.iter().any(|p| p.x >= Nm::new(1190)) && hull.iter().any(|p| p.y >= Nm::new(1190)), "{hull:?}");
    }

    #[test]
    fn segment_hull_is_a_clockwise_octagon_at_the_right_offset() {
        // Horizontal 1 mm segment, width 200, clearance 100, walk 0:
        // d = 100 + 100 = 200 -> the long edges sit at y = +-200.
        let h = segment_hull(Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::new(1000), y: Nm::ZERO }, Nm::new(200), Nm::new(100), Nm::ZERO);
        assert_eq!(h.len(), 8);
        assert!(shoelace(&h) > 0, "clockwise in KiCad's sense");
        assert!(h.iter().any(|p| p.y == Nm::new(200)) && h.iter().any(|p| p.y == Nm::new(-200)));
        assert!(h.iter().any(|p| p.x == Nm::new(1200)) && h.iter().any(|p| p.x == Nm::new(-200)));
    }

    #[test]
    fn via_hull_is_an_equilateral_octagon() {
        let h = via_hull(Point { x: Nm::ZERO, y: Nm::ZERO }, Nm::new(600), Nm::new(200), Nm::ZERO);
        assert_eq!(h.len(), 8);
        // bbox = 300 + 200 on each side.
        assert_eq!(h.iter().map(|p| p.x).max(), Some(Nm::new(500)));
        assert_eq!(h.iter().map(|p| p.y).min(), Some(Nm::new(-500)));
    }

    /// D4: a 60-degree rotated 1.0 x 0.6 mm rectangle must keep the hull at
    /// least `cl` (minus rounding) from every pad edge; the old
    /// segment-distance version left a 64 um shortfall.
    #[test]
    fn rotated_polygon_pad_hull_keeps_clearance() {
        let (w, h, cl) = (500.0f64, 300.0f64, Nm::new(300));
        for deg in [10.0f64, 30.0, 55.0, 60.0, 70.0, 85.0] {
            let (sn, cs) = deg.to_radians().sin_cos();
            let poly: Vec<Point> = [(-w, -h), (w, -h), (w, h), (-w, h)].iter().map(|&(x, y)| Point { x: Nm::kiround(x * cs - y * sn), y: Nm::kiround(x * sn + y * cs) }).collect();
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
            assert!(min_d >= (cl - Nm::new(2)).as_f64(), "deg {deg}: hull only {min_d} from pad, need {cl}");
        }
    }

    #[test]
    fn rect_pad_hull_has_square_corners() {
        let h = primitive_hull(&Shape::Rect { x0: Nm::ZERO, y0: Nm::ZERO, x1: Nm::new(1000), y1: Nm::new(500) }, Nm::new(100), Nm::ZERO);
        assert_eq!(h.len(), 4);
    }

    #[test]
    fn roundrect_hull_is_one_outline_around_the_pad() {
        let h = primitive_hull(&Shape::RoundRect { x0: Nm::ZERO, y0: Nm::ZERO, x1: Nm::new(1000), y1: Nm::new(600), r: Nm::new(150) }, Nm::new(100), Nm::ZERO);
        assert!(h.len() >= 8);
        assert!(h.iter().map(|p| p.x).max().unwrap() >= Nm::new(1100));
    }
}
