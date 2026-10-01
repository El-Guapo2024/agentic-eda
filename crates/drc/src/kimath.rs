//! A small port of the parts of KiCad's `libs/kimath` that the DRC test
//! providers need: `SEG` distance/collision (from `geometry/seg.cpp`) and a
//! generic shape-clearance routine built the same way KiCad's own
//! `SHAPE::Collide` dispatch ultimately bottoms out (segment/point
//! primitives, `SquaredDistance`, and a point-in-polygon containment test
//! for the "one shape swallows the other" case).
//!
//! Units are integer micrometers throughout (`eda_model::ir::Um`), the same
//! as the rest of this workspace -- KiCad itself works in integer nanometers
//! internally for exactly the same reason (float coordinates in DRC are a
//! source of nondeterminism).
//!
//! Fidelity notes (see the task report for the full list):
//! - `PlacedPad`/`placed_courtyard` already reduce every pad/courtyard to an
//!   axis-aligned box in board space (exact for 0/90/180/270° rotation,
//!   conservative otherwise -- see `eda_model::footprint`'s own doc
//!   comments). This module consumes that convention as given rather than
//!   re-deriving true rotated geometry, so our `Rect`/`RoundRect`/`Oval`
//!   shapes are always axis-aligned.
//! - KiCad subtracts a small "DRC epsilon" from every clearance value before
//!   testing, to absorb the polygon-approximation error of its own arc
//!   rasterizer. We compute exact closed-form distances (no arc
//!   rasterization), so there is nothing to absorb and epsilon is 0.

use eda_model::ir::{Point, Um};

pub type EPoint = (i64, i64);

/// Integer square root, rounded to the nearest integer, ported the same way
/// `SEG.cpp`'s `isqrt` is: an `f64` estimate corrected by a couple of
/// increments/decrements so it is exact for the magnitudes DRC deals with
/// (board coordinates squared fit comfortably in an `i64`/`i128`).
pub fn isqrt(x: i128) -> i64 {
    if x <= 0 {
        return 0;
    }
    let mut r = (x as f64).sqrt() as i64;
    while (r as i128) * (r as i128) > x {
        r -= 1;
    }
    while ((r + 1) as i128) * ((r + 1) as i128) <= x {
        r += 1;
    }
    r
}

fn sq(x: i64) -> i128 {
    (x as i128) * (x as i128)
}

/// A line segment `A -> B`, exactly KiCad's `SEG` (a degenerate segment,
/// `A == B`, is a point -- every routine here handles that the way KiCad's
/// own `SEG` methods do).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seg {
    pub a: Point,
    pub b: Point,
}

impl Seg {
    pub fn new(a: Point, b: Point) -> Self {
        Seg { a, b }
    }

    pub fn point(p: Point) -> Self {
        Seg { a: p, b: p }
    }

    /// Squared distance to a point -- ported from `SEG::SquaredDistance(const VECTOR2I&)`.
    pub fn sq_distance_to_point(&self, p: Point) -> i128 {
        let ab = ((self.b.x - self.a.x) as i128, (self.b.y - self.a.y) as i128);
        let ap = ((p.x - self.a.x) as i128, (p.y - self.a.y) as i128);
        let e = ap.0 * ab.0 + ap.1 * ab.1;
        if e <= 0 {
            return ap.0 * ap.0 + ap.1 * ap.1;
        }
        let f = ab.0 * ab.0 + ab.1 * ab.1;
        if e >= f {
            let bp = ((p.x - self.b.x) as i128, (p.y - self.b.y) as i128);
            return bp.0 * bp.0 + bp.1 * bp.1;
        }
        let ap_sq = ap.0 * ap.0 + ap.1 * ap.1;
        // g = |ap|^2 - e^2/f, computed in floating point exactly the way
        // KiCad's own implementation does (it too falls back to `double`
        // here), then rounded back to an integer square-length.
        let g = ap_sq as f64 - (e as f64 * e as f64) / (f as f64);
        if g < 0.0 { 0 } else { g.round() as i128 }
    }

    /// Nearest point on the segment to `p` -- ported from `SEG::NearestPoint(const VECTOR2I&)`.
    pub fn nearest_point(&self, p: Point) -> Point {
        let d = ((self.b.x - self.a.x) as i128, (self.b.y - self.a.y) as i128);
        let l_sq = d.0 * d.0 + d.1 * d.1;
        if l_sq == 0 {
            return self.a;
        }
        let pa = ((p.x - self.a.x) as i128, (p.y - self.a.y) as i128);
        let t = d.0 * pa.0 + d.1 * pa.1;
        if t < 0 {
            return self.a;
        } else if t > l_sq {
            return self.b;
        }
        let xp = (t * d.0) / l_sq;
        let yp = (t * d.1) / l_sq;
        Point { x: self.a.x + xp as Um, y: self.a.y + yp as Um }
    }

    /// Squared distance to another segment -- ported from `SEG::SquaredDistance(const SEG&)`:
    /// exact intersection short-circuits to 0, otherwise the minimum of the
    /// four endpoint-to-opposite-segment distances (correct for two convex
    /// segments; KiCad relies on the same four-point reduction).
    pub fn sq_distance_to_seg(&self, other: &Seg) -> i128 {
        if self.a == self.b {
            return other.sq_distance_to_point(self.a);
        }
        if other.a == other.b {
            return self.sq_distance_to_point(other.a);
        }
        if self.intersect(other).is_some() {
            return 0;
        }
        [other.sq_distance_to_point(self.a), other.sq_distance_to_point(self.b), self.sq_distance_to_point(other.a), self.sq_distance_to_point(other.b)]
            .into_iter()
            .min()
            .unwrap()
    }

    /// Exact intersection point of the two (finite) segments, if any --
    /// ported from `SEG::intersects` (the non-collinear branch; an exact
    /// collinear overlap, a rare case for tracks, is reported as no
    /// intersection here -- see the module doc's fidelity notes).
    pub fn intersect(&self, other: &Seg) -> Option<Point> {
        let (ax, ay, bx, by) = (self.a.x as i128, self.a.y as i128, self.b.x as i128, self.b.y as i128);
        let (cx, cy, dx, dy) = (other.a.x as i128, other.a.y as i128, other.b.x as i128, other.b.y as i128);
        // Bounding-box rejection first (cheap, and matches SEG::intersects).
        if bx.max(ax) < cx.min(dx) || dx.max(cx) < ax.min(bx) || by.max(ay) < cy.min(dy) || dy.max(cy) < ay.min(by) {
            return None;
        }
        let (dir1x, dir1y) = (bx - ax, by - ay);
        let (dir2x, dir2y) = (dx - cx, dy - cy);
        let (offx, offy) = (cx - ax, cy - ay);
        let det = dir2x * dir1y - dir2y * dir1x; // dir2.Cross(dir1)
        if det == 0 {
            return None; // parallel/collinear: treated as "no crossing point" here.
        }
        let param2 = dir2x * offy - dir2y * offx; // dir2.Cross(offset)
        let param1 = dir1x * offy - dir1y * offx; // dir1.Cross(offset)
        let (t, s) = if det > 0 {
            if param1 < 0 || param1 > det || param2 < 0 || param2 > det {
                return None;
            }
            (param1, param2)
        } else {
            if param1 > 0 || param1 < det || param2 > 0 || param2 < det {
                return None;
            }
            (param1, param2)
        };
        let _ = s;
        let px = ax + (dir1x * t) / det;
        let py = ay + (dir1y * t) / det;
        Some(Point { x: px as Um, y: py as Um })
    }
}

/// A "core" shape reduced to a point-set primitive plus an inflation
/// radius, the way every DRC shape ultimately is in KiCad too (a track is a
/// segment with a half-width radius; a pad hole is a point or segment with
/// a drill radius; a polygon has radius 0). [`Shape::clearance_to`] computes
/// the boundary-to-boundary gap between two shapes generically from this
/// reduction, which is exact for every shape kind this crate produces.
#[derive(Debug, Clone)]
enum Core {
    Point(Point),
    Seg(Seg),
    /// Closed simple polygon (last point implicitly joins the first).
    Polygon(Vec<Point>),
    /// An open, disjoint set of pen-stroke segments (a glyph or a run of
    /// text): unlike `Polygon`, never has an interior, so
    /// `contains_point` always answers `false` for it (the same as
    /// `Point`/`Seg`) -- every collision against it falls through to the
    /// boundary-segment scan, which is the exact right test for a stroked
    /// (not filled) shape.
    Segs(Vec<Seg>),
}

impl Core {
    fn boundary_segs(&self) -> Vec<Seg> {
        match self {
            Core::Point(p) => vec![Seg::point(*p)],
            Core::Seg(s) => vec![*s],
            Core::Polygon(pts) => {
                let n = pts.len();
                if n < 2 {
                    return pts.iter().map(|p| Seg::point(*p)).collect();
                }
                (0..n).map(|i| Seg::new(pts[i], pts[(i + 1) % n])).collect()
            }
            Core::Segs(segs) => segs.clone(),
        }
    }

    /// A single point of this core, used as the containment probe (see
    /// [`Shape::clearance_to`]'s doc comment for why one point suffices).
    fn probe_point(&self) -> Point {
        match self {
            Core::Point(p) => *p,
            Core::Seg(s) => s.a,
            Core::Polygon(pts) => pts[0],
            Core::Segs(segs) => segs[0].a,
        }
    }

    /// Point-in-polygon, ported from `SHAPE_LINE_CHAIN::PointInside`'s
    /// even-odd ray cast (KiCad casts a ray in +x and counts crossings;
    /// same here). Non-polygons have no interior.
    fn contains_point(&self, p: Point) -> bool {
        let Core::Polygon(pts) = self else { return false };
        if pts.len() < 3 {
            return false;
        }
        let n = pts.len();
        let mut inside = false;
        for i in 0..n {
            let p1 = pts[i];
            let p2 = pts[(i + 1) % n];
            if p1.y == p2.y {
                continue;
            }
            // d = rescale(diff.x, (pt.y - p1.y), diff.y), matching KiCad's
            // integer rescale (proportional, rounds toward zero as `/` does).
            let diff = (p2.x - p1.x, p2.y - p1.y);
            let d = (diff.0 as i128 * (p.y - p1.y) as i128) / diff.1 as i128;
            if (p1.y >= p.y) != (p2.y >= p.y) && ((p.x - p1.x) as i128) < d {
                inside = !inside;
            }
        }
        inside
    }
}

/// Every DRC shape this crate needs, each reducible to a [`Core`] plus a
/// radius. Board space, integer micrometers.
#[derive(Debug, Clone)]
pub enum Shape {
    /// A circular pad, via, or round drilled hole.
    Circle { c: Point, r: Um },
    /// A "stadium": KiCad's `SHAPE_SEGMENT` -- a track segment, an oval pad
    /// (focus points along its long axis), or a slotted hole.
    Stadium { a: Point, b: Point, r: Um },
    /// An axis-aligned rectangular pad or courtyard.
    Rect { x0: Um, y0: Um, x1: Um, y1: Um },
    /// An axis-aligned rounded-rectangle pad: the rectangle inset by `r` on
    /// every side, Minkowski-summed with a disk of radius `r` -- exactly
    /// KiCad's own construction, and exactly what
    /// `eda_model::footprint::PlacedPad::signed_distance` already computes
    /// for a single point; here it is expressed as a boundary (inset-rect
    /// core + radius `r`) so it composes with every other shape generically.
    RoundRect { x0: Um, y0: Um, x1: Um, y1: Um, r: Um },
    /// A closed simple polygon: a zone outline, a courtyard, or a
    /// hand-drawn graphic polygon.
    Polygon { pts: Vec<Point> },
    /// A piece of stroked (vector-font) text: KiCad's Newstroke glyphs are
    /// pen strokes, not filled outlines, so this is the disjoint set of
    /// pen-down segments for a whole string, inflated by half the stroke
    /// thickness -- see `crate::stroke_font`. Never empty (an empty/
    /// all-space string produces no `SilkItem` at all; see `board.rs`).
    Strokes { segs: Vec<Seg>, r: Um },
}

impl Shape {
    fn core(&self) -> Core {
        match self {
            Shape::Circle { c, .. } => Core::Point(*c),
            Shape::Stadium { a, b, .. } => Core::Seg(Seg::new(*a, *b)),
            Shape::Rect { x0, y0, x1, y1 } => Core::Polygon(vec![Point { x: *x0, y: *y0 }, Point { x: *x1, y: *y0 }, Point { x: *x1, y: *y1 }, Point { x: *x0, y: *y1 }]),
            Shape::RoundRect { x0, y0, x1, y1, r } => {
                let (x0, y0, x1, y1) = (x0 + r, y0 + r, x1 - r, y1 - r);
                // A degenerate inset (r >= half the smaller side) collapses
                // to a segment or point core, still correct.
                if x0 > x1 && y0 > y1 {
                    Core::Point(Point { x: (x0 + x1) / 2, y: (y0 + y1) / 2 })
                } else if x0 > x1 {
                    Core::Seg(Seg::new(Point { x: (x0 + x1) / 2, y: y0 }, Point { x: (x0 + x1) / 2, y: y1 }))
                } else if y0 > y1 {
                    Core::Seg(Seg::new(Point { x: x0, y: (y0 + y1) / 2 }, Point { x: x1, y: (y0 + y1) / 2 }))
                } else {
                    Core::Polygon(vec![Point { x: x0, y: y0 }, Point { x: x1, y: y0 }, Point { x: x1, y: y1 }, Point { x: x0, y: y1 }])
                }
            }
            Shape::Polygon { pts } => Core::Polygon(pts.clone()),
            Shape::Strokes { segs, .. } => Core::Segs(segs.clone()),
        }
    }

    /// This shape's boundary, decomposed into segments -- e.g. for
    /// computing distance to a board edge (a bare line, not a filled
    /// region: `clearance_to`'s containment shortcut does not apply there,
    /// since being "inside the board" is not a collision).
    pub fn boundary_segs(&self) -> Vec<Seg> {
        self.core().boundary_segs()
    }

    pub fn radius(&self) -> Um {
        match self {
            Shape::Circle { r, .. } | Shape::Stadium { r, .. } | Shape::RoundRect { r, .. } | Shape::Strokes { r, .. } => *r,
            Shape::Rect { .. } | Shape::Polygon { .. } => 0,
        }
    }

    /// Axis-aligned bounding box `(x0, y0, x1, y1)`, inflated by `clearance`
    /// -- used to cheaply reject far-apart pairs before the exact test.
    pub fn bbox(&self, clearance: Um) -> (Um, Um, Um, Um) {
        let (mut x0, mut y0, mut x1, mut y1) = (Um::MAX, Um::MAX, Um::MIN, Um::MIN);
        let mut widen = |p: Point| {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        };
        match self {
            Shape::Circle { c, .. } => widen(*c),
            Shape::Stadium { a, b, .. } => {
                widen(*a);
                widen(*b);
            }
            Shape::Rect { x0: a, y0: b, x1: c, y1: d } | Shape::RoundRect { x0: a, y0: b, x1: c, y1: d, .. } => {
                widen(Point { x: *a, y: *b });
                widen(Point { x: *c, y: *d });
            }
            Shape::Polygon { pts } => pts.iter().for_each(|p| widen(*p)),
            Shape::Strokes { segs, .. } => segs.iter().for_each(|s| {
                widen(s.a);
                widen(s.b);
            }),
        }
        let r = self.radius() + clearance;
        (x0 - r, y0 - r, x1 + r, y1 + r)
    }

    /// Boundary-to-boundary clearance to `other`: `(actual, nearest_point)`,
    /// `actual` clamped to 0 the moment the two shapes touch or overlap --
    /// exactly KiCad's own convention (see e.g. `SHAPE_SEGMENT::Collide`'s
    /// `actual = max(0, sqrt(dist_sq) - (width+1)/2)`, and the hole-to-hole
    /// provider's explicit `actual = max(0, actual - r1 - r2)`).
    ///
    /// The core-to-core distance is exact for two *convex* cores (every
    /// shape here except general zone/graphic polygons) via the min over
    /// boundary-segment pairs. For two general simple polygons it is exact
    /// too: if neither contains the other, two overlapping simple regions
    /// must have crossing edges, which the segment-pair scan finds as a
    /// zero distance; if one fully contains the other, testing a single
    /// representative point of the contained core against the container
    /// suffices (every point of a fully-contained region is inside by
    /// definition, so the first point is as good as any other) -- so one
    /// probe point per side plus the boundary-segment scan is a complete
    /// test, the same one `SHAPE_POLY_SET::Collide` reduces to internally.
    pub fn clearance_to(&self, other: &Shape) -> (Um, Point) {
        let (c1, c2) = (self.core(), other.core());
        if c1.contains_point(c2.probe_point()) || c2.contains_point(c1.probe_point()) {
            return (0, c1.probe_point());
        }
        let (segs1, segs2) = (c1.boundary_segs(), c2.boundary_segs());
        let mut best_sq = i128::MAX;
        let mut best_pt = segs1[0].a;
        for s1 in &segs1 {
            for s2 in &segs2 {
                let d = s1.sq_distance_to_seg(s2);
                if d < best_sq {
                    best_sq = d;
                    best_pt = s1.nearest_point(s2.nearest_point(s1.a));
                }
            }
        }
        let core_dist = isqrt(best_sq);
        let actual = (core_dist - self.radius() - other.radius()).max(0);
        (actual, best_pt)
    }

    /// `true` when `self` and `other` are closer than `clearance` (KiCad's
    /// `SHAPE::Collide`): touching/overlapping is always a collision, and
    /// so is any positive gap smaller than `clearance`.
    pub fn collides(&self, other: &Shape, clearance: Um) -> Option<(Um, Point)> {
        let (bx0, by0, bx1, by1) = self.bbox(clearance);
        let (ox0, oy0, ox1, oy1) = other.bbox(0);
        if bx1 < ox0 || ox1 < bx0 || by1 < oy0 || oy1 < by0 {
            return None;
        }
        let (actual, pos) = self.clearance_to(other);
        if actual == 0 || actual < clearance {
            Some((actual, pos))
        } else {
            None
        }
    }
}

/// Distance from `shape` to the nearest of a set of bare edge segments
/// (Edge.Cuts, a board margin line -- geometry with no interior, unlike
/// [`Shape::Polygon`]'s area semantics in [`Shape::clearance_to`]). Used by
/// the edge/silk-to-edge clearance provider, where "inside the board
/// outline" must not be mistaken for "touching the edge".
pub fn distance_to_open_segments(shape: &Shape, edges: &[Seg]) -> (Um, Point) {
    let mut best_sq = i128::MAX;
    let mut best_pt = edges.first().map(|s| s.a).unwrap_or(Point { x: 0, y: 0 });
    for s1 in shape.boundary_segs() {
        for s2 in edges {
            let d = s1.sq_distance_to_seg(s2);
            if d < best_sq {
                best_sq = d;
                best_pt = s1.nearest_point(s2.nearest_point(s1.a));
            }
        }
    }
    let actual = (isqrt(best_sq) - shape.radius()).max(0);
    (actual, best_pt)
}

/// Euclidean distance between two points, rounded to the nearest micron --
/// used where KiCad reports `(a - b).EuclideanNorm()` directly (hole-to-hole
/// centre distance, annular width).
pub fn dist(a: Point, b: Point) -> Um {
    isqrt(sq(a.x - b.x) + sq(b.y - a.y).max(sq(a.y - b.y))) as Um
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: Um, y: Um) -> Point {
        Point { x, y }
    }

    #[test]
    fn circle_circle_gap() {
        let a = Shape::Circle { c: p(0, 0), r: 500 };
        let b = Shape::Circle { c: p(2000, 0), r: 500 };
        let (actual, _) = a.clearance_to(&b);
        assert_eq!(actual, 1000);
        assert!(a.collides(&b, 1001).is_some());
        assert!(a.collides(&b, 1000).is_none());
    }

    #[test]
    fn overlapping_circles_are_zero() {
        let a = Shape::Circle { c: p(0, 0), r: 500 };
        let b = Shape::Circle { c: p(100, 0), r: 500 };
        let (actual, _) = a.clearance_to(&b);
        assert_eq!(actual, 0);
        // Touching (clearance 0) must still be a collision.
        assert!(a.collides(&b, 0).is_some());
    }

    #[test]
    fn seg_seg_parallel_distance() {
        let a = Seg::new(p(0, 0), p(1000, 0));
        let b = Seg::new(p(0, 300), p(1000, 300));
        assert_eq!(isqrt(a.sq_distance_to_seg(&b)), 300);
    }

    #[test]
    fn seg_seg_crossing_is_zero() {
        let a = Seg::new(p(0, 0), p(1000, 1000));
        let b = Seg::new(p(0, 1000), p(1000, 0));
        assert_eq!(a.intersect(&b), Some(p(500, 500)));
        assert_eq!(a.sq_distance_to_seg(&b), 0);
    }

    #[test]
    fn point_in_polygon_square() {
        let core = Core::Polygon(vec![p(0, 0), p(1000, 0), p(1000, 1000), p(0, 1000)]);
        assert!(core.contains_point(p(500, 500)));
        assert!(!core.contains_point(p(1500, 500)));
    }

    #[test]
    fn pad_fully_inside_zone_is_zero_clearance() {
        let zone = Shape::Polygon { pts: vec![p(0, 0), p(10_000, 0), p(10_000, 10_000), p(0, 10_000)] };
        let pad = Shape::Rect { x0: 4000, y0: 4000, x1: 5000, y1: 5000 };
        let (actual, _) = zone.clearance_to(&pad);
        assert_eq!(actual, 0);
    }

    #[test]
    fn stadium_matches_kicad_shape_segment_collide() {
        // Two parallel tracks, width 200 each, centrelines 500 apart:
        // copper-to-copper gap = 500 - 100 - 100 = 300.
        let t1 = Shape::Stadium { a: p(0, 0), b: p(5000, 0), r: 100 };
        let t2 = Shape::Stadium { a: p(0, 500), b: p(5000, 500), r: 100 };
        let (actual, _) = t1.clearance_to(&t2);
        assert_eq!(actual, 300);
    }

    #[test]
    fn roundrect_corner_distance_matches_closed_form() {
        // A 2000x2000 roundrect, corner ratio 0.25 -> r=500, probed from a
        // point straight off one corner: distance must be the diagonal
        // distance to the inset-corner centre, minus r.
        let rr = Shape::RoundRect { x0: -1000, y0: -1000, x1: 1000, y1: 1000, r: 500 };
        let probe = Shape::Circle { c: p(2000, 2000), r: 0 };
        let (actual, _) = rr.clearance_to(&probe);
        // Inset corner centre at (500,500); distance from (2000,2000) is
        // 1500*sqrt(2) ~= 2121; minus r(500) ~= 1621.
        assert!((actual - 1621).abs() <= 1, "actual={actual}");
    }
}
