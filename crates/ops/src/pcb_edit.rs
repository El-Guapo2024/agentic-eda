//! PCB edit verbs ported from pcbnew's edit tools (`pcbnew/tools/` at KiCad
//! 8303b2ad) that need a polygon engine: the polygon boolean routines of
//! `EDIT_TOOL::BooleanPolygons` (`pcbnew/tools/item_modification_routine.cpp`'s
//! `POLYGON_MERGE_ROUTINE`, `POLYGON_SUBTRACT_ROUTINE`,
//! `POLYGON_INTERSECT_ROUTINE`).
//!
//! Everything that is plain line/arc geometry (fillet, chamfer, dogbone,
//! extend, heal, simplify, outsets, ...) is computed in the studio
//! (`web/studio/src/kicad-port/pcbModify.ts`) and sent as add/delete
//! commands; only what needs Clipper lives here, on `eda_shape_poly_set`
//! (a port of `SHAPE_POLY_SET`).

use eda_clipper2::Point64;
use eda_model::ir::{DrawingsSection, Point, Shape};
use eda_model::CheckResult;
use eda_shape_poly_set::{get_arc_to_segment_count, ShapePolySet};
use serde::{Deserialize, Serialize};

/// Which `POLYGON_BOOLEAN_ROUTINE` to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BooleanOp {
    /// `POLYGON_MERGE_ROUTINE`: the union of every shape.
    Merge,
    /// `POLYGON_SUBTRACT_ROUTINE`: the first shape minus every other.
    Subtract,
    /// `POLYGON_INTERSECT_ROUTINE`: the first shape clipped to every other.
    Intersect,
}

impl BooleanOp {
    fn is_commutative(self) -> bool {
        !matches!(self, BooleanOp::Subtract)
    }

    fn verb(self) -> &'static str {
        match self {
            BooleanOp::Merge => "merge",
            BooleanOp::Subtract => "subtract",
            BooleanOp::Intersect => "intersect",
        }
    }
}

/// `SHAPE_ARC::DefaultAccuracyForPCB() / 5` (`getArcPolygonizationMaxError`,
/// `ARC_HIGH_DEF` = 5 um): the sag a polygonised circle may have, in um.
const CIRCLE_POLY_MAX_ERROR_UM: i32 = 1;

fn pt64(p: Point) -> Point64 {
    Point64::new(p.x, p.y)
}

/// A circle as `SHAPE_LINE_CHAIN::Append( SHAPE_ARC( centre, centre + (R, 0), FULL_CIRCLE ) )`
/// does it (`SHAPE_ARC::ConvertToPolyline`): the start point exactly on the
/// circle, then one point per segment at the middle of its angular span on a
/// circle widened by half the effective error, so the chord stays inside the
/// error band.
fn circle_polyline(center: Point, radius: i64) -> Vec<Point64> {
    let n = get_arc_to_segment_count(radius.clamp(1, i32::MAX as i64) as i32, CIRCLE_POLY_MAX_ERROR_UM).max(2) as usize;
    let n_for_error = n.max(3) as f64;
    let alpha = std::f64::consts::PI / n_for_error;
    let effective_error = (radius as f64 * (1.0 / alpha.cos() - 1.0)).round();
    let r = radius as f64 + effective_error / 2.0;
    let mut out = Vec::with_capacity(n + 1);
    out.push(Point64::new(center.x + radius, center.y));
    for k in 0..n {
        let angle = (2 * k + 1) as f64 * std::f64::consts::PI / n as f64;
        out.push(Point64::new((center.x as f64 + r * angle.cos()).round() as i64, (center.y as f64 + r * angle.sin()).round() as i64));
    }
    out
}

/// `PCB_SHAPE` -> polygon, the way `POLYGON_BOOLEAN_ROUTINE::ProcessShape` reads it:
/// a polygon's outline, a rectangle's four corners, a circle polygonised.
fn poly_of(shape: &Shape) -> Option<ShapePolySet> {
    match shape {
        Shape::Polygon { pts, .. } => Some(ShapePolySet::from_outline(pts.iter().map(|p| pt64(*p)).collect())),
        Shape::Rect { start, end, .. } => Some(ShapePolySet::from_outline(vec![
            pt64(*start),
            Point64::new(end.x, start.y),
            pt64(*end),
            Point64::new(start.x, end.y),
        ])),
        Shape::Circle { center, end, .. } => {
            let r = (((end.x - center.x) as f64).hypot((end.y - center.y) as f64)).round() as i64;
            if r <= 0 {
                return None;
            }
            Some(ShapePolySet::from_outline(circle_polyline(*center, r)))
        }
        _ => None,
    }
}

/// `shape->GetBoundingBox().GetArea()`, the sort key of `BooleanPolygons`'s reverse-order retry.
fn bbox_area(shape: &Shape) -> f64 {
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    let mut take = |p: Point| {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    };
    match shape {
        Shape::Circle { center, end, .. } => {
            let r = ((end.x - center.x) as f64).hypot((end.y - center.y) as f64).round() as i64;
            take(Point { x: center.x - r, y: center.y - r });
            take(Point { x: center.x + r, y: center.y + r });
        }
        other => other.points().into_iter().for_each(&mut take),
    }
    if x1 < x0 {
        return 0.0;
    }
    (x1 - x0) as f64 * (y1 - y0) as f64
}

/// The result of one pass of the routine over the shapes in a given order.
struct Outcome {
    /// Indices of the shapes the routine consumed (the first shape, plus every later one it processed).
    removed: Vec<usize>,
    /// What is left: one polygon per remaining outline (holes carried fractured into the ring).
    rings: Vec<Vec<Point>>,
    successes: usize,
    /// `layer`, `stroke_width`, `filled` of the first shape: the property donor.
    donor: Option<(String, i64, bool)>,
}

/// `POLYGON_BOOLEAN_ROUTINE::ProcessShape` over every shape, then `Finalize`.
fn run_routine(op: BooleanOp, items: &[Shape]) -> Outcome {
    let mut working = ShapePolySet::new();
    let mut first = true;
    let mut removed = Vec::new();
    let mut successes = 0;
    let mut donor = None;
    for (i, shape) in items.iter().enumerate() {
        let Some(poly) = poly_of(shape) else { continue };
        if first {
            donor = Some((shape.layer().to_string(), shape.stroke_width(), shape.is_filled()));
            working = poly;
            first = false;
            removed.push(i);
            continue;
        }
        let consumed = match op {
            BooleanOp::Merge => {
                working.boolean_add(&poly);
                true
            }
            BooleanOp::Subtract => {
                let mut copy = working.clone();
                copy.boolean_subtract(&poly);
                working = copy;
                true
            }
            BooleanOp::Intersect => {
                let mut copy = working.clone();
                copy.boolean_intersection(&poly);
                // "There was no intersection. Rather than deleting the working polygon, we'll skip and report a failure."
                if copy.is_empty() {
                    false
                } else {
                    working = copy;
                    true
                }
            }
        };
        if consumed {
            removed.push(i);
            successes += 1;
        }
    }
    // `Finalize`: one new polygon per remaining outline. The IR polygon has no holes, so a hole is bridged into its ring (`Fracture`), as `zone_cutout` does.
    let mut rings = Vec::new();
    if !first && !working.is_empty() {
        working.fracture(false);
        for poly in &working.polys {
            let ring = &poly[0];
            if ring.len() >= 3 {
                rings.push(ring.iter().map(|p| Point { x: p.x, y: p.y }).collect());
            }
        }
    }
    Outcome { removed, rings, successes, donor }
}

/// `Cmd::BooleanShapes`: `EDIT_TOOL::BooleanPolygons` on `ids` (rectangles, circles and polygons; `ids` in the order the routine takes
/// them -- the caller puts the donor first). Consumed shapes are deleted and the result added as polygons on the donor's layer with its
/// width and fill. Subtract with nothing left retries from the largest bounding box down ("assume the user meant go in a different
/// opposite order").
pub fn boolean_shapes(dr: &mut DrawingsSection, op: BooleanOp, ids: &[String]) -> Result<(), Vec<CheckResult>> {
    let mut items: Vec<Shape> = Vec::new();
    for id in ids {
        if items.iter().any(|s| s.id() == id.as_str()) {
            continue;
        }
        if let Some(s) = dr.shapes.iter().find(|s| s.id() == id.as_str()) {
            if matches!(s, Shape::Polygon { .. } | Shape::Rect { .. } | Shape::Circle { .. }) {
                items.push(s.clone());
            }
        }
    }
    if items.len() < 2 {
        return Err(vec![CheckResult::fail("ops_bad_boolean", op.verb(), "a polygon boolean needs at least two polygons, rectangles or circles")]);
    }

    let mut outcome = run_routine(op, &items);
    if !op.is_commutative() && outcome.rings.is_empty() {
        items.sort_by(|a, b| bbox_area(b).partial_cmp(&bbox_area(a)).unwrap_or(std::cmp::Ordering::Equal));
        outcome = run_routine(op, &items);
    }
    if outcome.successes == 0 {
        return Err(vec![CheckResult::fail("ops_bad_boolean", op.verb(), format!("unable to {} the selected polygons", op.verb()))]);
    }

    let consumed: Vec<String> = outcome.removed.iter().map(|&i| items[i].id().to_string()).collect();
    dr.shapes.retain(|s| !consumed.iter().any(|id| id == s.id()));
    if let Some((layer, stroke_width, filled)) = outcome.donor {
        for ring in outcome.rings {
            dr.shapes.push(Shape::Polygon { id: String::new(), layer: layer.clone(), stroke_width, filled, pts: ring });
        }
    }
    dr.assign_missing_ids();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(id: &str, x0: i64, y0: i64, x1: i64, y1: i64) -> Shape {
        Shape::Rect { id: id.into(), layer: "F.Fab".into(), stroke_width: 100, filled: false, start: Point { x: x0, y: y0 }, end: Point { x: x1, y: y1 } }
    }

    fn circle(id: &str, cx: i64, cy: i64, r: i64) -> Shape {
        Shape::Circle { id: id.into(), layer: "F.SilkS".into(), stroke_width: 150, filled: true, center: Point { x: cx, y: cy }, end: Point { x: cx + r, y: cy } }
    }

    fn section(shapes: Vec<Shape>) -> DrawingsSection {
        let mut dr = DrawingsSection { shapes, ..Default::default() };
        dr.assign_missing_ids();
        dr
    }

    fn ids(dr: &DrawingsSection) -> Vec<String> {
        dr.shapes.iter().map(|s| s.id().to_string()).collect()
    }

    fn area(shape: &Shape) -> f64 {
        let Shape::Polygon { pts, .. } = shape else { panic!("not a polygon") };
        let mut a = 0.0;
        for i in 0..pts.len() {
            let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
            a += (p.x * q.y - q.x * p.y) as f64;
        }
        a.abs() / 2.0
    }

    #[test]
    fn merge_of_overlapping_rects_is_one_polygon_with_the_union_area() {
        let mut dr = section(vec![rect("a", 0, 0, 10_000, 10_000), rect("b", 5_000, 5_000, 15_000, 15_000)]);
        let all = ids(&dr);
        boolean_shapes(&mut dr, BooleanOp::Merge, &all).unwrap();
        assert_eq!(dr.shapes.len(), 1);
        // 100 + 100 - 25 mm^2
        assert!((area(&dr.shapes[0]) - 175_000_000.0).abs() < 1.0, "area {}", area(&dr.shapes[0]));
        // the donor (first shape) supplies layer, width and fill
        assert_eq!(dr.shapes[0].layer(), "F.Fab");
        assert_eq!(dr.shapes[0].stroke_width(), 100);
        assert!(!dr.shapes[0].is_filled());
    }

    #[test]
    fn merge_of_disjoint_rects_gives_two_polygons() {
        let mut dr = section(vec![rect("a", 0, 0, 1_000, 1_000), rect("b", 5_000, 5_000, 6_000, 6_000)]);
        let all = ids(&dr);
        boolean_shapes(&mut dr, BooleanOp::Merge, &all).unwrap();
        assert_eq!(dr.shapes.len(), 2);
        assert!(dr.shapes.iter().all(|s| matches!(s, Shape::Polygon { .. })));
    }

    #[test]
    fn subtract_removes_the_overlap_from_the_first_shape() {
        let mut dr = section(vec![rect("a", 0, 0, 10_000, 10_000), rect("b", 5_000, 0, 15_000, 10_000)]);
        let order = vec![dr.shapes[0].id().to_string(), dr.shapes[1].id().to_string()];
        boolean_shapes(&mut dr, BooleanOp::Subtract, &order).unwrap();
        assert_eq!(dr.shapes.len(), 1);
        assert!((area(&dr.shapes[0]) - 50_000_000.0).abs() < 1.0);
    }

    #[test]
    fn subtract_in_the_wrong_order_retries_largest_first() {
        // the small square first would leave nothing, so the larger one is used as the base
        let small = rect("small", 2_000, 2_000, 4_000, 4_000);
        let big = rect("big", 0, 0, 10_000, 10_000);
        let mut dr = section(vec![small, big]);
        let order = vec![dr.shapes[0].id().to_string(), dr.shapes[1].id().to_string()];
        boolean_shapes(&mut dr, BooleanOp::Subtract, &order).unwrap();
        // a square with a square hole, fractured into one ring: 100 - 4 mm^2 of area
        assert_eq!(dr.shapes.len(), 1);
        let Shape::Polygon { pts, .. } = &dr.shapes[0] else { panic!() };
        assert!(pts.len() >= 8);
    }

    #[test]
    fn intersect_keeps_only_the_overlap() {
        let mut dr = section(vec![rect("a", 0, 0, 10_000, 10_000), rect("b", 6_000, 6_000, 20_000, 20_000)]);
        let all = ids(&dr);
        boolean_shapes(&mut dr, BooleanOp::Intersect, &all).unwrap();
        assert_eq!(dr.shapes.len(), 1);
        assert!((area(&dr.shapes[0]) - 16_000_000.0).abs() < 1.0);
    }

    #[test]
    fn intersect_of_disjoint_shapes_is_refused_and_changes_nothing() {
        let mut dr = section(vec![rect("a", 0, 0, 1_000, 1_000), rect("b", 5_000, 5_000, 6_000, 6_000)]);
        let all = ids(&dr);
        let before = dr.shapes.clone();
        let err = boolean_shapes(&mut dr, BooleanOp::Intersect, &all).unwrap_err();
        assert_eq!(err[0].check, "ops_bad_boolean");
        assert_eq!(dr.shapes, before);
    }

    #[test]
    fn circles_are_polygonised_and_merge_with_rects() {
        let mut dr = section(vec![circle("c", 0, 0, 5_000), rect("r", 0, -2_000, 12_000, 2_000)]);
        let all = ids(&dr);
        boolean_shapes(&mut dr, BooleanOp::Merge, &all).unwrap();
        assert_eq!(dr.shapes.len(), 1);
        // the donor is the circle: silkscreen layer, 150 wide, filled
        assert_eq!(dr.shapes[0].layer(), "F.SilkS");
        assert_eq!(dr.shapes[0].stroke_width(), 150);
        assert!(dr.shapes[0].is_filled());
        let pi_r2 = std::f64::consts::PI * 25_000_000.0;
        // circle + the part of the rect outside it (a 4 mm wide bar from the circle edge to x=12 mm), within polygonisation error
        assert!(area(&dr.shapes[0]) > pi_r2);
    }

    #[test]
    fn needs_two_eligible_shapes() {
        let mut dr = section(vec![rect("a", 0, 0, 1_000, 1_000)]);
        let all = ids(&dr);
        assert!(boolean_shapes(&mut dr, BooleanOp::Merge, &all).is_err());
        // a segment is not eligible
        let mut dr2 = section(vec![
            rect("a", 0, 0, 1_000, 1_000),
            Shape::Segment { id: "s".into(), layer: "F.Fab".into(), stroke_width: 100, filled: false, start: Point { x: 0, y: 0 }, end: Point { x: 10, y: 0 } },
        ]);
        let all2 = ids(&dr2);
        assert!(boolean_shapes(&mut dr2, BooleanOp::Merge, &all2).is_err());
    }

    #[test]
    fn circle_polyline_stays_within_the_error_band() {
        let pts = circle_polyline(Point { x: 0, y: 0 }, 5_000);
        assert!(pts.len() > 20);
        for p in &pts {
            let d = (p.x as f64).hypot(p.y as f64);
            assert!((d - 5_000.0).abs() < 3.0, "{d}");
        }
        assert_eq!((pts[0].x, pts[0].y), (5_000, 0));
    }
}
