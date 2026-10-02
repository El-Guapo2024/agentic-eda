//! Shape-touch tests standing in for KiCad's `SHAPE::Collide` /
//! `BOARD_CONNECTED_ITEM::GetEffectiveShape()->Collide(...)` calls in
//! `CN_VISITOR::operator()` (`pcbnew/connectivity/connectivity_algo.cpp`).
//!
//! KiCad tests exact rotated-polygon collision through its general
//! `SHAPE` hierarchy. Every pad in our model is already an axis-aligned
//! shape (`eda_model::footprint::PlacedPad` bakes a pad's own rotation and
//! its footprint's into an axis-aligned `size`, exact for multiples of 90
//! degrees -- see that module), so what is needed here is much narrower:
//! rect/roundrect/circle/oval pad vs. segment/point, segment vs. segment,
//! circle (via) vs. the same, and point-in-polygon for a zone's raw
//! outline (see [`crate::items`] for why a zone is its outline, not a
//! fill). These are exact for the circle/via cases and for a segment
//! approaching a pad roughly head-on; for a track grazing a
//! roundrect/oval pad's corner from a shallow angle they are a close
//! approximation, not an exact Minkowski-sum test -- acceptable here
//! because this crate answers "does copper touch," not "how much
//! clearance," which is `crates/drc`'s job.

use eda_model::footprint::PlacedPad;
use eda_model::ir::{Point, Um};

use crate::items::ItemShape;

/// Euclidean distance -- KiCad's `VECTOR2I::Distance`/`CN_ANCHOR::Dist`.
pub fn dist(a: Point, b: Point) -> f64 {
    (((a.x - b.x) as f64).powi(2) + ((a.y - b.y) as f64).powi(2)).sqrt()
}

/// The point on segment `a..b` closest to `p`.
pub fn closest_point_on_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let len2 = abx * abx + aby * aby;
    if len2 <= f64::EPSILON {
        return a;
    }
    let t = ((p.0 - a.0) * abx + (p.1 - a.1) * aby) / len2;
    let t = t.clamp(0.0, 1.0);
    (a.0 + t * abx, a.1 + t * aby)
}

/// Distance from `p` to the closest point of segment `a..b`.
pub fn point_seg_distance(p: Point, a: Point, b: Point) -> f64 {
    let pf = (p.x as f64, p.y as f64);
    let af = (a.x as f64, a.y as f64);
    let bf = (b.x as f64, b.y as f64);
    let c = closest_point_on_segment(pf, af, bf);
    ((pf.0 - c.0).powi(2) + (pf.1 - c.1).powi(2)).sqrt()
}

fn orient(o: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
}

/// Is `q` on segment `p..r`, given `p, q, r` already known collinear?
fn on_segment(p: (f64, f64), q: (f64, f64), r: (f64, f64)) -> bool {
    q.0 <= p.0.max(r.0) && q.0 >= p.0.min(r.0) && q.1 <= p.1.max(r.1) && q.1 >= p.1.min(r.1)
}

/// Do closed segments `p1..p2` and `p3..p4` intersect or touch (including
/// collinear overlap)? Standard orientation-based test.
pub fn segments_intersect(p1: (f64, f64), p2: (f64, f64), p3: (f64, f64), p4: (f64, f64)) -> bool {
    let d1 = orient(p3, p4, p1);
    let d2 = orient(p3, p4, p2);
    let d3 = orient(p1, p2, p3);
    let d4 = orient(p1, p2, p4);

    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return true;
    }
    if d1 == 0.0 && on_segment(p3, p1, p4) {
        return true;
    }
    if d2 == 0.0 && on_segment(p3, p2, p4) {
        return true;
    }
    if d3 == 0.0 && on_segment(p1, p3, p2) {
        return true;
    }
    if d4 == 0.0 && on_segment(p1, p4, p2) {
        return true;
    }
    false
}

/// Minimum distance between two finite segments (0 if they touch or cross).
pub fn seg_seg_distance(a1: Point, a2: Point, b1: Point, b2: Point) -> f64 {
    let (a1f, a2f, b1f, b2f) = ((a1.x as f64, a1.y as f64), (a2.x as f64, a2.y as f64), (b1.x as f64, b1.y as f64), (b2.x as f64, b2.y as f64));
    if segments_intersect(a1f, a2f, b1f, b2f) {
        return 0.0;
    }
    point_seg_distance(a1, b1, b2).min(point_seg_distance(a2, b1, b2)).min(point_seg_distance(b1, a1, a2)).min(point_seg_distance(b2, a1, a2))
}

/// Ray-casting point-in-polygon test. Treats an exactly-on-boundary point
/// as an implementation detail (may go either way) -- acceptable since
/// zone connectivity is already an outline-only approximation.
pub fn point_in_polygon(p: Point, poly: &[Point]) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let (px, py) = (p.x as f64, p.y as f64);
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (poly[i].x as f64, poly[i].y as f64);
        let (xj, yj) = (poly[j].x as f64, poly[j].y as f64);
        if (yi > py) != (yj > py) && px < (xj - xi) * (py - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Do two (possibly non-convex) polygon outlines overlap? Any vertex of
/// one inside the other, or any pair of edges crossing.
pub fn polygons_overlap(a: &[Point], b: &[Point]) -> bool {
    if a.len() < 3 || b.len() < 3 {
        return false;
    }
    if a.iter().any(|&p| point_in_polygon(p, b)) {
        return true;
    }
    if b.iter().any(|&p| point_in_polygon(p, a)) {
        return true;
    }
    for i in 0..a.len() {
        let (a1, a2) = (a[i], a[(i + 1) % a.len()]);
        for j in 0..b.len() {
            let (b1, b2) = (b[j], b[(j + 1) % b.len()]);
            if segments_intersect((a1.x as f64, a1.y as f64), (a2.x as f64, a2.y as f64), (b1.x as f64, b1.y as f64), (b2.x as f64, b2.y as f64)) {
                return true;
            }
        }
    }
    false
}

/// A pad's effective "corner radius" for the Minkowski-style distance
/// below: `PlacedPad::corner_radius` (0 unless `RoundRect`), or half the
/// shorter side for a round pad (circle/oval), so a circular pad reduces
/// to the exact circle-circle/circle-point distance.
fn effective_corner_radius(p: &PlacedPad) -> f64 {
    if p.is_round() {
        p.size.0.min(p.size.1) as f64 / 2.0
    } else {
        p.corner_radius()
    }
}

/// Distance between two pads: inset each to a plain rectangle by its own
/// corner radius, measure the separation between those rectangles
/// (0 if overlapping), then remove both radii again -- the same
/// inset-then-Minkowski-expand construction `PlacedPad::signed_distance`
/// uses for a single shape against a point, generalised to two shapes.
/// Exact for circle/circle and circle/point; a good approximation for two
/// roundrects meeting near a corner.
pub fn pad_pad_distance(a: &PlacedPad, b: &PlacedPad) -> f64 {
    let (ra, rb) = (effective_corner_radius(a), effective_corner_radius(b));
    let (ahw, ahh) = ((a.size.0 as f64 / 2.0 - ra).max(0.0), (a.size.1 as f64 / 2.0 - ra).max(0.0));
    let (bhw, bhh) = ((b.size.0 as f64 / 2.0 - rb).max(0.0), (b.size.1 as f64 / 2.0 - rb).max(0.0));
    let dx = ((b.center.x - a.center.x) as f64).abs();
    let dy = ((b.center.y - a.center.y) as f64).abs();
    let (ox, oy) = ((dx - ahw - bhw).max(0.0), (dy - ahh - bhh).max(0.0));
    let outside = (ox * ox + oy * oy).sqrt();
    let inset_sep = if outside > 0.0 { outside } else { (dx - ahw - bhw).max(dy - ahh - bhh) };
    inset_sep - ra - rb
}

/// Axis-aligned bounding box, `(x0, y0, x1, y1)`, of a point set.
pub fn bbox_of(points: &[Point]) -> (Um, Um, Um, Um) {
    let mut b = (Um::MAX, Um::MAX, Um::MIN, Um::MIN);
    for p in points {
        b.0 = b.0.min(p.x);
        b.1 = b.1.min(p.y);
        b.2 = b.2.max(p.x);
        b.3 = b.3.max(p.y);
    }
    if points.is_empty() {
        (0, 0, 0, 0)
    } else {
        b
    }
}

/// Whether two [`ItemShape`]s touch -- the geometric half of
/// `CN_VISITOR::operator()`. Zones are handled by outline containment of
/// the other shape's anchor points (see [`crate::items`]), matching the
/// task's "zones connect items within their outline" approximation;
/// everything else is a direct shape/shape distance test against 0.
pub fn touches(a: &ItemShape, a_anchors: &[Point], b: &ItemShape, b_anchors: &[Point]) -> bool {
    match (a, b) {
        (ItemShape::Zone { outline: oa }, ItemShape::Zone { outline: ob }) => polygons_overlap(oa, ob),
        (ItemShape::Zone { outline }, _) => b_anchors.iter().any(|&p| point_in_polygon(p, outline)),
        (_, ItemShape::Zone { outline }) => a_anchors.iter().any(|&p| point_in_polygon(p, outline)),

        (ItemShape::Pad(pa), ItemShape::Pad(pb)) => pad_pad_distance(pa, pb) <= 0.0,

        (ItemShape::Pad(pad), ItemShape::Segment { a, b, half_width }) | (ItemShape::Segment { a, b, half_width }, ItemShape::Pad(pad)) => {
            let c = closest_point_on_segment((pad.center.x as f64, pad.center.y as f64), (a.x as f64, a.y as f64), (b.x as f64, b.y as f64));
            let cp = Point { x: c.0.round() as Um, y: c.1.round() as Um };
            pad.signed_distance(cp) <= *half_width || pad.signed_distance(*a) <= *half_width || pad.signed_distance(*b) <= *half_width
        }

        (ItemShape::Pad(pad), ItemShape::Via { center, radius }) | (ItemShape::Via { center, radius }, ItemShape::Pad(pad)) => pad.signed_distance(*center) <= *radius,

        (ItemShape::Segment { a: a1, b: a2, half_width: hwa }, ItemShape::Segment { a: b1, b: b2, half_width: hwb }) => seg_seg_distance(*a1, *a2, *b1, *b2) <= hwa + hwb,

        (ItemShape::Segment { a, b, half_width }, ItemShape::Via { center, radius }) | (ItemShape::Via { center, radius }, ItemShape::Segment { a, b, half_width }) => {
            point_seg_distance(*center, *a, *b) <= radius + half_width
        }

        (ItemShape::Via { center: c1, radius: r1 }, ItemShape::Via { center: c2, radius: r2 }) => dist(*c1, *c2) <= r1 + r2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_in_square() {
        let square = [Point { x: 0, y: 0 }, Point { x: 100, y: 0 }, Point { x: 100, y: 100 }, Point { x: 0, y: 100 }];
        assert!(point_in_polygon(Point { x: 50, y: 50 }, &square));
        assert!(!point_in_polygon(Point { x: 150, y: 50 }, &square));
    }

    #[test]
    fn crossing_segments_have_zero_distance() {
        let d = seg_seg_distance(Point { x: 0, y: 0 }, Point { x: 100, y: 100 }, Point { x: 0, y: 100 }, Point { x: 100, y: 0 });
        assert_eq!(d, 0.0);
    }

    #[test]
    fn parallel_segments_measure_perpendicular_gap() {
        let d = seg_seg_distance(Point { x: 0, y: 0 }, Point { x: 100, y: 0 }, Point { x: 0, y: 50 }, Point { x: 100, y: 50 });
        assert!((d - 50.0).abs() < 1e-6);
    }

    #[test]
    fn circle_pads_measure_center_distance_minus_radii() {
        let pad = |x: Um, y: Um, d: Um| PlacedPad { number: "1".into(), center: Point { x, y }, size: (d, d), through_hole: false, shape: eda_model::footprint::PadShape::Circle, roundrect_ratio: None };
        let a = pad(0, 0, 1000);
        let b = pad(3000, 0, 1000);
        // centers 3000 apart, radii 500 each -> gap 2000.
        assert!((pad_pad_distance(&a, &b) - 2000.0).abs() < 1e-6);
    }
}

/// Shoelace area (absolute), µm² -- `SHAPE_LINE_CHAIN::Area( true )`.
pub fn polygon_area(pts: &[Point]) -> f64 {
    if pts.len() < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        sum += (a.x as f64) * (b.y as f64) - (b.x as f64) * (a.y as f64);
    }
    (sum / 2.0).abs()
}
