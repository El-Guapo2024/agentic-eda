//! Edge.Cuts as KiCad sees it: the graphic items `BOARD::GetBoardPolygonOutlines` chains into the board outline, and the
//! arc and circle tessellation it chains them with.
//!
//! The geometry that needs no polygon set lives here, below `eda_drc`, so every crate can read an outline's items:
//!
//! - [`edge_cuts_shapes`]: the items. A board's Edge.Cuts are `placement.outline` (a closed polygon of straight edges,
//!   which the writer emits as `gr_line`s) plus every [`Shape`] on layer `Edge.Cuts` in `drawings.shapes` (an imported board's
//!   arcs, circles, rectangles and curves, or a cutout drawn in the studio). When [`DrawingsSection::outline_is_shapes`] is
//!   set the shapes are the whole outline and `placement.outline` is only their summary, so it is not an item.
//! - [`Arc`]: `SHAPE_ARC` through three points, and [`Arc::polyline`], `SHAPE_ARC::ConvertToPolyline`, the chord
//!   approximation `ConvertOutlineToPolygon` and `TransformShapeToPolygon` use at `BOARD_DESIGN_SETTINGS::m_MaxError`.
//! - [`shape_chains`]: an item's effective shape as polylines (`EDA_SHAPE::makeEffectiveShapes`), for the consumers that
//!   want the items themselves, not the polygon (the zone filler's edge knockout, the router's edge obstacle).
//!
//! The chaining itself, `ConvertOutlineToPolygon`, is `eda_drc::outline`.

use crate::bezier;
use crate::ir::{Design, DrawingsSection, Point, Shape, Um};

/// KiCad's `Edge_Cuts` layer.
pub const EDGE_CUTS: &str = "Edge.Cuts";

/// `DEFAULT_CHAINING_EPSILON_MM` (`pcbnew/board.h`): 0.01 mm, the largest gap between one item's end and the next one's start
/// that `ConvertOutlineToPolygon` still chains (`BOARD::GetOutlinesChainingEpsilon`).
pub const CHAINING_EPSILON_UM: Um = 10;

/// `BOARD_DESIGN_SETTINGS::m_MaxError`, which is `ARC_HIGH_DEF` (0.005 mm) for a board that does not set it: the largest
/// distance between a curve and the polygon that stands for it.
pub const MAX_ERROR_UM: Um = 5;

/// The id the segments of the polygon `placement.outline` carry as Edge.Cuts items: the one the writer maps their uuids to, so a
/// kicad-cli report that names an outline segment is pointed back at the board outline.
pub const POLYGON_OUTLINE_ID: &str = "outline";

/// A shape drawn on Edge.Cuts.
pub fn is_edge_cuts(shape: &Shape) -> bool {
    shape.layer() == EDGE_CUTS
}

/// `true` when the shapes on Edge.Cuts are the board's whole outline ([`DrawingsSection::outline_is_shapes`]).
pub fn outline_is_shapes(design: &Design) -> bool {
    design.drawings.as_ref().is_some_and(|d| d.outline_is_shapes)
}

/// `true` when the board has any shape on Edge.Cuts besides `placement.outline`'s polygon.
pub fn has_edge_cuts_shapes(design: &Design) -> bool {
    design.drawings.as_ref().is_some_and(|d| d.shapes.iter().any(is_edge_cuts))
}

/// The board's Edge.Cuts items, in the order the writer emits them: `placement.outline` as one `Segment` per side (unless the
/// shapes are the outline: [`DrawingsSection::outline_is_shapes`]), then every shape on Edge.Cuts. This is what kicad-cli reads
/// out of the exported `.kicad_pcb`, so a consumer built on it sees the outline kicad-cli sees.
pub fn edge_cuts_shapes(design: &Design) -> Vec<Shape> {
    let mut items = Vec::new();
    if !outline_is_shapes(design) {
        if let Some(pl) = &design.placement {
            let n = pl.outline.len();
            if n >= 2 {
                for i in 0..n {
                    items.push(Shape::Segment { id: POLYGON_OUTLINE_ID.into(), layer: EDGE_CUTS.into(), stroke_width: 0, filled: false, start: pl.outline[i], end: pl.outline[(i + 1) % n] });
                }
            }
        }
    }
    if let Some(d) = &design.drawings {
        items.extend(d.shapes.iter().filter(|s| is_edge_cuts(s)).cloned());
    }
    items
}

impl DrawingsSection {
    /// The shapes of this section drawn on Edge.Cuts.
    pub fn edge_cuts(&self) -> impl Iterator<Item = &Shape> {
        self.shapes.iter().filter(|s| is_edge_cuts(s))
    }
}

// ----------------------------------------------------------------------------------------------------------------------
// SHAPE_ARC
// ----------------------------------------------------------------------------------------------------------------------

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// Distance from `p` to the segment `a`-`b` (`SEG::Distance( VECTOR2I )`).
fn point_segment_distance(p: Point, a: Point, b: Point) -> f64 {
    let (px, py, ax, ay, bx, by) = (p.x as f64, p.y as f64, a.x as f64, a.y as f64, b.x as f64, b.y as f64);
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return (px - ax).hypot(py - ay);
    }
    let t = (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0);
    (px - (ax + t * dx)).hypot(py - (ay + t * dy))
}

/// `GetArcToSegmentCount` (`geometry_utils.cpp`): the number of segments that approximate an arc of `angle` radians to within
/// `error_max`, never fewer than two and never coarser than eight to a full circle.
pub fn arc_to_segment_count(radius: f64, error_max: f64, angle: f64) -> i64 {
    // KiCad clamps both to one internal unit (a nanometre) before dividing.
    let radius = radius.max(0.001);
    let error_max = error_max.max(0.001);
    let rel_error = error_max / radius;
    let mut arc_increment = (1.0 - rel_error).acos().to_degrees() * 2.0;
    // `MIN_SEGCOUNT_FOR_CIRCLE`: at least eight segments round a whole circle, however small.
    arc_increment = arc_increment.min(360.0 / 8.0);
    let seg_count = (angle.to_degrees().abs() / arc_increment).round() as i64;
    seg_count.max(2)
}

/// `CircleToEndSegmentDeltaRadius`: how far outside a circle of `radius` the vertices of a `seg_count`-gon go when its sides are
/// tangent to the circle -- the approximation error `ConvertToPolyline` splits either side of the arc.
pub fn circle_to_end_segment_delta_radius(radius: f64, seg_count: i64) -> f64 {
    // The minimal count is 3, otherwise the angle below is meaningless; in practice KiCad clamps to eight.
    let seg_count = if seg_count <= 2 { 3 } else { seg_count };
    let alpha = std::f64::consts::PI / seg_count as f64;
    (radius * (1.0 - 1.0 / alpha.cos())).abs()
}

/// `SHAPE_ARC`: the arc that goes from `start` through `mid` to `end`, a width of 0 (an outline's arc has none).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arc {
    pub start: Point,
    pub mid: Point,
    pub end: Point,
    /// `None` when the three points are collinear: a straight line, the circle through them has no centre.
    center: Option<(f64, f64)>,
    radius: f64,
}

impl Arc {
    /// `SHAPE_ARC( aArcStart, aArcMid, aArcEnd, 0 )`: `update_values` finds the centre from the three points (`CalcArcCenter`).
    pub fn through(start: Point, mid: Point, end: Point) -> Arc {
        let center = calc_arc_center(start, mid, end);
        let radius = center.map_or(f64::INFINITY, |c| dist((start.x as f64, start.y as f64), c));
        Arc { start, mid, end, center, radius }
    }

    /// `SHAPE_ARC( aCenter, aStart, ANGLE_360, 0 )` for the circle `ConvertOutlineToPolygon` builds from a `CIRCLE`: it starts at
    /// the centre's right (`start.x += radius`), goes round through the left and ends where it started.
    pub fn full_circle(center: Point, radius: Um) -> Arc {
        let start = Point { x: center.x + radius, y: center.y };
        let mid = Point { x: center.x - radius, y: center.y };
        Arc::through(start, mid, start)
    }

    pub fn center(&self) -> Option<(f64, f64)> {
        self.center
    }

    pub fn radius(&self) -> f64 {
        self.radius
    }

    /// `SHAPE_ARC::IsCCW`.
    pub fn is_ccw(&self) -> bool {
        let (v1x, v1y) = ((self.end.x - self.mid.x) as i128, (self.end.y - self.mid.y) as i128);
        let (v2x, v2y) = ((self.start.x - self.mid.x) as i128, (self.start.y - self.mid.y) as i128);
        v1x * v2y - v1y * v2x > 0
    }

    /// `SHAPE_ARC::GetStartAngle`: radians, `[0, 2 pi)`.
    pub fn start_angle(&self) -> f64 {
        let Some(c) = self.center else { return 0.0 };
        (self.start.y as f64 - c.1).atan2(self.start.x as f64 - c.0).rem_euclid(std::f64::consts::TAU)
    }

    /// `SHAPE_ARC::GetCentralAngle`: radians, positive for a counter-clockwise arc and negative for a clockwise one; a full turn
    /// when the arc starts where it ends (a circle).
    pub fn central_angle(&self) -> f64 {
        let Some(c) = self.center else { return 0.0 };
        if self.start == self.end {
            return std::f64::consts::TAU;
        }
        let a_end = (self.end.y as f64 - c.1).atan2(self.end.x as f64 - c.0);
        let a_start = (self.start.y as f64 - c.1).atan2(self.start.x as f64 - c.0);
        let mut angle = a_end - a_start;
        // The middle point says which of the two arcs between the ends is meant.
        if self.is_ccw() {
            if angle < 0.0 {
                angle += std::f64::consts::TAU;
            }
        } else if angle > 0.0 {
            angle -= std::f64::consts::TAU;
        }
        angle
    }

    /// `SHAPE_ARC::ConvertToPolyline( aMaxError )` with a width of 0: the first and last points are the arc's own ends, and the
    /// ones between sit at the middle of each of `n` equal slices, a little outside the arc, so the polyline stays within
    /// `max_error` of it on both sides. Collinear points, and an arc too small to bend, give the straight line.
    pub fn polyline(&self, max_error: f64) -> Vec<Point> {
        let Some(c) = self.center else { return vec![self.start, self.end] };
        let r = self.radius;
        let half_max_error = (max_error / 2.0).max(0.001);
        let ca = self.central_angle();
        let n;
        let effective_error;
        if r < half_max_error || point_segment_distance(self.mid, self.start, self.end) < half_max_error {
            // A very rare case: one segment, with an error between -max_error/2 and +max_error/2, as expected.
            n = 0;
            effective_error = r;
        } else {
            n = arc_to_segment_count(r, max_error, ca);
            // The error of the approximation actually reached, which can be less than `max_error`. `int seg360 = ...` truncates.
            let seg360 = (n as f64 * 360.0 / ca.to_degrees().abs()) as i64;
            effective_error = circle_to_end_segment_delta_radius(r, seg360);
        }
        // Split the error on either side of the arc: the start and end sit exactly on it, so the first and last segments are
        // shorter to stay inside the band.
        let r = r + effective_error / 2.0;
        let n2 = n * 2;
        let sa = self.start_angle();
        let mut pts = vec![self.start];
        let mut i = 1;
        while i < n2 {
            let a = sa + ca * i as f64 / n2 as f64;
            pts.push(Point { x: (c.0 + r * a.cos()).round() as Um, y: (c.1 + r * a.sin()).round() as Um });
            i += 2;
        }
        pts.push(self.end);
        pts
    }
}

/// `CalcArcCenter( aStart, aMid, aEnd )`: the centre of the circle through three points. `start == end` is a whole circle whose
/// middle point is the far side of it; collinear points have no centre.
fn calc_arc_center(start: Point, mid: Point, end: Point) -> Option<(f64, f64)> {
    let (sx, sy, mx, my, ex, ey) = (start.x as f64, start.y as f64, mid.x as f64, mid.y as f64, end.x as f64, end.y as f64);
    if start == end {
        return Some(((sx + mx) / 2.0, (sy + my) / 2.0));
    }
    let d = 2.0 * (sx * (my - ey) + mx * (ey - sy) + ex * (sy - my));
    if d == 0.0 {
        return None;
    }
    let ux = ((sx * sx + sy * sy) * (my - ey) + (mx * mx + my * my) * (ey - sy) + (ex * ex + ey * ey) * (sy - my)) / d;
    let uy = ((sx * sx + sy * sy) * (ex - mx) + (mx * mx + my * my) * (sx - ex) + (ex * ex + ey * ey) * (mx - sx)) / d;
    Some((ux, uy))
}

// ----------------------------------------------------------------------------------------------------------------------
// Items as polylines
// ----------------------------------------------------------------------------------------------------------------------

/// A polyline an item is made of; `closed` joins its last point back to its first.
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    pub pts: Vec<Point>,
    pub closed: bool,
}

/// The radius `PCB_SHAPE::GetRadius` gives a circle: the distance from its centre to the point on its circumference, rounded.
pub fn circle_radius(center: Point, rim: Point) -> Um {
    ((rim.x - center.x) as f64).hypot((rim.y - center.y) as f64).round() as Um
}

/// The closed contour `processClosedShape` makes of a circle: `SHAPE_ARC( center, start, ANGLE_360 )` polygonized at `max_error`,
/// without the repeated last point.
pub fn circle_contour(center: Point, rim: Point, max_error: f64) -> Vec<Point> {
    let mut pts = Arc::full_circle(center, circle_radius(center, rim)).polyline(max_error);
    // `SetClosed( true )` merges the last point with the first.
    if pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    pts
}

/// The four corners of a rectangle in the order `EDA_SHAPE::GetRectCorners` gives them.
pub fn rect_corners(start: Point, end: Point) -> [Point; 4] {
    [start, Point { x: end.x, y: start.y }, end, Point { x: start.x, y: end.y }]
}

/// An item's effective shape as polylines (`EDA_SHAPE::makeEffectiveShapes` before it is given a width): a segment is its two
/// ends, an arc or a Bezier curve their flattening at `max_error`, and a circle, a rectangle and a polygon a closed ring.
pub fn shape_chains(shape: &Shape, max_error: f64) -> Vec<Chain> {
    match shape {
        Shape::Segment { start, end, .. } => vec![Chain { pts: vec![*start, *end], closed: false }],
        Shape::Arc { start, mid, end, .. } => vec![Chain { pts: Arc::through(*start, *mid, *end).polyline(max_error), closed: false }],
        Shape::Rect { start, end, .. } => vec![Chain { pts: rect_corners(*start, *end).to_vec(), closed: true }],
        Shape::Circle { center, end, .. } => vec![Chain { pts: circle_contour(*center, *end, max_error), closed: true }],
        Shape::Polygon { pts, .. } => vec![Chain { pts: pts.clone(), closed: true }],
        Shape::Bezier { start, c1, c2, end, .. } => vec![Chain { pts: bezier::bezier_polyline(*start, *c1, *c2, *end, max_error.round() as Um), closed: false }],
    }
}

/// The box (min, max) around all of `shapes` -- `BOARD::GetBoardEdgesBoundingBox` -- each grown by half its line width
/// (`EDA_SHAPE::getBoundingBox`). `None` when there are none.
pub fn bounding_box(shapes: &[Shape], max_error: f64) -> Option<(Point, Point)> {
    let mut bb: Option<(Point, Point)> = None;
    for s in shapes {
        let half = s.stroke_width() / 2;
        for chain in shape_chains(s, max_error) {
            for p in chain.pts {
                let (lo, hi) = (Point { x: p.x - half, y: p.y - half }, Point { x: p.x + half, y: p.y + half });
                bb = Some(match bb {
                    None => (lo, hi),
                    Some((a, b)) => (Point { x: a.x.min(lo.x), y: a.y.min(lo.y) }, Point { x: b.x.max(hi.x), y: b.y.max(hi.y) }),
                });
            }
        }
    }
    bb
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i64, y: i64) -> Point {
        Point { x, y }
    }

    fn quarter() -> Arc {
        // A quarter circle of radius 10 mm round the origin, counter-clockwise from (10 mm, 0) to (0, 10 mm).
        Arc::through(p(10_000, 0), p(7_071, 7_071), p(0, 10_000))
    }

    #[test]
    fn the_centre_and_sweep_of_an_arc_come_from_its_three_points() {
        let a = quarter();
        let c = a.center().expect("a centre");
        assert!((c.0).abs() < 0.5 && (c.1).abs() < 0.5, "centre {c:?}");
        assert!((a.radius() - 10_000.0).abs() < 1.0);
        assert!(a.is_ccw());
        assert!((a.central_angle() - std::f64::consts::FRAC_PI_2).abs() < 1e-3);
        // The same ends with the middle on the other side go the long way round, clockwise.
        let long = Arc::through(p(10_000, 0), p(-7_071, -7_071), p(0, 10_000));
        assert!(!long.is_ccw());
        assert!((long.central_angle() + 1.5 * std::f64::consts::PI).abs() < 1e-3, "{}", long.central_angle());
    }

    #[test]
    fn a_quarter_arc_polygonizes_at_kicads_segment_count_and_error() {
        // GetArcToSegmentCount( 10 mm, 5 um, 90 deg ) is 25 (acos( 1 - 5e-4 ) * 2 = 3.62 deg), so 25 mid-slice points and both ends.
        let pts = quarter().polyline(5.0);
        assert_eq!(pts.len(), 27, "{pts:?}");
        assert_eq!((pts[0], *pts.last().expect("a last point")), (p(10_000, 0), p(0, 10_000)), "the ends are the arc's own");
        // Every vertex is within the error band of the true circle: the effective error of 25 slices is about 4.9 um, split either side.
        for q in &pts[1..pts.len() - 1] {
            let r = (q.x as f64).hypot(q.y as f64);
            assert!((r - 10_000.0 - 2.46).abs() < 0.8, "{q:?} is at {r}");
        }
        // And every chord stays within `max_error` of it.
        for w in pts.windows(2) {
            let mid = ((w[0].x + w[1].x) as f64 / 2.0, (w[0].y + w[1].y) as f64 / 2.0);
            let off = (mid.0.hypot(mid.1) - 10_000.0).abs();
            assert!(off <= 5.0 + 0.8, "chord {w:?} is {off} off");
        }
    }

    #[test]
    fn a_circle_is_a_ring_that_starts_at_its_right() {
        // 2 mm radius: 36 slices at 5 um; the contour is the start point plus one per slice, the repeated end dropped.
        let ring = circle_contour(p(1_000, 2_000), p(1_000, 4_000), 5.0);
        assert_eq!(ring[0], p(3_000, 2_000));
        assert_eq!(ring.len(), arc_to_segment_count(2_000.0, 5.0, std::f64::consts::TAU) as usize + 1);
        assert!(ring.windows(2).all(|w| w[0] != w[1]));
        let area: f64 = (0..ring.len()).map(|i| (ring[i].x as f64) * (ring[(i + 1) % ring.len()].y as f64) - (ring[(i + 1) % ring.len()].x as f64) * (ring[i].y as f64)).sum::<f64>() / 2.0;
        let circle = std::f64::consts::PI * 2_000.0 * 2_000.0;
        assert!((area.abs() - circle).abs() / circle < 0.005, "area {area} against {circle}");
    }

    #[test]
    fn collinear_points_and_tiny_arcs_are_one_segment() {
        assert_eq!(Arc::through(p(0, 0), p(500, 0), p(1_000, 0)).polyline(5.0), vec![p(0, 0), p(1_000, 0)]);
        // A 1 um sagitta is under half the 5 um error.
        assert_eq!(Arc::through(p(0, 0), p(5_000, 1), p(10_000, 0)).polyline(5.0), vec![p(0, 0), p(10_000, 0)]);
    }

    #[test]
    fn the_segment_count_never_goes_below_two_nor_above_eight_per_turn() {
        assert_eq!(arc_to_segment_count(1.0, 5.0, std::f64::consts::TAU), 8, "a tiny circle is still an octagon");
        assert_eq!(arc_to_segment_count(100_000.0, 5.0, 0.001), 2);
    }

    #[test]
    fn the_items_of_a_plain_polygon_outline_are_its_sides() {
        let d = Design {
            schema: 1,
            provenance: crate::ir::Provenance { engine_version: "test".into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(crate::ir::PlacementSection { outline: vec![p(0, 0), p(10_000, 0), p(10_000, 5_000)], footprints: vec![], modules: vec![] }),
            routing: None,
            drawings: None,
            footprint_library: None,
            symbol_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
        };
        let items = edge_cuts_shapes(&d);
        assert_eq!(items.len(), 3);
        assert_eq!(items[2].points(), vec![p(10_000, 5_000), p(0, 0)], "the last side closes the polygon");
        assert!(items.iter().all(|s| s.id() == POLYGON_OUTLINE_ID && s.layer() == EDGE_CUTS));
    }
}
