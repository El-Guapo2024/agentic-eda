//! The polygon construction of `CONVERT_TOOL::CreatePolys` (`pcbnew/tools/convert_tool.cpp` at KiCad
//! 8303b2ad), behind "Create Polygon / Zone / Rule Area from Selection": `getPolys`' three strategies
//! over a set of board items --
//!
//! * `CENTERLINE` and `COPY_LINEWIDTH`: every closed graphic (rectangle, circle, polygon, zone outline) as
//!   its own outline, plus every closed loop the selected segments, arcs, curves and tracks chain into
//!   (`makePolysFromClosedGraphics` + `makePolysFromChainedSegs`);
//! * `BOUNDING_HULL`: the union of everything *with* its line width (closed graphics, open graphics, tracks
//!   and vias: `makePolysFromOpenGraphics`), grown by `gap` with round corners.
//!
//! The IR polygon has no holes, so an outline with holes comes back fractured into one ring
//! (`ShapePolySet::fracture`, as `zone_cutout` and the boolean routines do). The result is read-only
//! geometry: the studio sends the polygons (and the deletion of what `consumed` lists) back as ordinary
//! `add_shape` / `add_zone` / `delete_*` commands.
//!
//! Not ported: text and pads as sources (the studio has no font outlines, and pads are not selectable
//! items), a rounded rectangle's corner radius (the IR rectangle has none).

use eda_clipper2::Point64;
use eda_model::ir::{tessellate_arc, Design, Point, Shape, Track, Via, Zone};
use eda_shape_poly_set::{get_arc_to_segment_count, CornerStrategy, ShapePolySet};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `CONVERT_STRATEGY`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConvertStrategy {
    /// "Copy line width of first object": the polygons are the centerlines, the stroke is copied by the caller.
    CopyLinewidth,
    /// "Use centerlines".
    Centerline,
    /// "Create bounding hull".
    BoundingHull,
}

/// What `getPolys` produced.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConvertedPolys {
    /// One ring per resulting outline (holes bridged into it).
    pub rings: Vec<Vec<Point>>,
    /// The ids that contributed (`SKIP_STRUCT`): what "Delete source objects after conversion" removes.
    pub consumed: Vec<String>,
}

/// `chainingEpsilon = 100` IU (nm) is a tenth of a micrometre: with integer micrometre coordinates only exact meetings chain.
const MAX_ERROR_UM: i32 = 5;

fn pt64(p: Point) -> Point64 {
    Point64::new(p.x, p.y)
}

fn pt(p: &Point64) -> Point {
    Point { x: p.x, y: p.y }
}

fn norm(a: f64) -> f64 {
    a.rem_euclid(std::f64::consts::TAU)
}

/// An arc's polyline within [`MAX_ERROR_UM`] of the curve (`SHAPE_ARC::ConvertToPolyline`): the sweep through `mid`, split by
/// `GetArcToSegmentCount( radius, error )` per full turn; endpoints exact. Collinear points give the chord.
pub fn arc_points(start: Point, mid: Point, end: Point) -> Vec<Point> {
    let (sx, sy, mx, my, ex, ey) = (start.x as f64, start.y as f64, mid.x as f64, mid.y as f64, end.x as f64, end.y as f64);
    let d = 2.0 * (sx * (my - ey) + mx * (ey - sy) + ex * (sy - my));
    if d.abs() < 1e-6 {
        return vec![start, end];
    }
    let ux = ((sx * sx + sy * sy) * (my - ey) + (mx * mx + my * my) * (ey - sy) + (ex * ex + ey * ey) * (sy - my)) / d;
    let uy = ((sx * sx + sy * sy) * (ex - mx) + (mx * mx + my * my) * (sx - ex) + (ex * ex + ey * ey) * (mx - sx)) / d;
    let r = (sx - ux).hypot(sy - uy);
    let ang = |x: f64, y: f64| (y - uy).atan2(x - ux);
    let (a0, a1, a2) = (ang(sx, sy), ang(mx, my), ang(ex, ey));
    let mut sweep = norm(a2 - a0);
    if norm(a1 - a0) > sweep {
        sweep -= std::f64::consts::TAU;
    }
    let per_turn = get_arc_to_segment_count(r.round().clamp(1.0, i32::MAX as f64) as i32, MAX_ERROR_UM).max(4) as f64;
    let n = ((sweep.abs() / std::f64::consts::TAU * per_turn).ceil() as usize).clamp(2, 4096);
    tessellate_arc(start, mid, end, n)
}

/// A circle as `SHAPE_ARC( centre - (R, 0), centre + (R, 0), centre - (R, 0) )` polygonises: `n` points on the circle.
fn circle_ring(center: Point, r: i64) -> Vec<Point64> {
    let n = get_arc_to_segment_count(r.clamp(1, i32::MAX as i64) as i32, 1).max(8) as usize;
    (0..n)
        .map(|k| {
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            Point64::new((center.x as f64 + r as f64 * a.cos()).round() as i64, (center.y as f64 + r as f64 * a.sin()).round() as i64)
        })
        .collect()
}

/// A stadium (`TransformOvalToPolygon`): the segment `a`-`b` with round caps of radius `r`.
fn stadium_ring(a: Point, b: Point, r: i64) -> Vec<Point64> {
    let r = r.max(1);
    let (dx, dy) = ((b.x - a.x) as f64, (b.y - a.y) as f64);
    if dx.hypot(dy) < 1.0 {
        return circle_ring(a, r);
    }
    let base = dy.atan2(dx);
    let half = (get_arc_to_segment_count(r.clamp(1, i32::MAX as i64) as i32, 1).max(8) as usize / 2).max(2);
    let cap = |c: Point, from: f64| -> Vec<Point64> {
        (0..=half)
            .map(|i| {
                let t = from + std::f64::consts::PI * i as f64 / half as f64;
                Point64::new((c.x as f64 + r as f64 * t.cos()).round() as i64, (c.y as f64 + r as f64 * t.sin()).round() as i64)
            })
            .collect()
    };
    let mut out = cap(b, base - std::f64::consts::FRAC_PI_2);
    out.extend(cap(a, base + std::f64::consts::FRAC_PI_2));
    out
}

/// A polyline stroked `width` wide with round caps and joins (`TransformShapeToPolygon` of a track / segment / arc).
fn stroke(pts: &[Point], width: i64) -> ShapePolySet {
    let mut set = ShapePolySet::new();
    let r = (width / 2).max(1);
    if pts.len() == 1 {
        set.add_outline(circle_ring(pts[0], r));
    }
    for w in pts.windows(2) {
        if w[0] != w[1] {
            set.add_outline(stadium_ring(w[0], w[1], r));
        }
    }
    set.simplify();
    set
}

/// A closed outline stroked `width` wide: the ring between the outline grown and shrunk by half the width (a filled shape is just grown).
fn stroke_closed(ring: &[Point64], width: i64, filled: bool) -> ShapePolySet {
    let half = (width / 2).max(0);
    let base = ShapePolySet::from_outline(ring.to_vec());
    if half == 0 {
        return base;
    }
    let mut outer = base.clone();
    outer.inflate(half, CornerStrategy::RoundAllCorners, 1, false);
    if filled {
        return outer;
    }
    let mut inner = base;
    inner.deflate(half, CornerStrategy::RoundAllCorners, 1);
    if !inner.is_empty() {
        outer.boolean_subtract(&inner);
    }
    outer
}

// ------------------------------------------------------------------------------------------ items

enum Item<'a> {
    Shape(&'a Shape),
    Zone(&'a Zone),
    Track(&'a Track),
    Via(&'a Via),
}

fn find_items<'a>(design: &'a Design, ids: &[String]) -> Vec<(String, Item<'a>)> {
    let mut out = Vec::new();
    for id in ids {
        if out.iter().any(|(i, _): &(String, Item)| i == id) {
            continue;
        }
        if let Some(s) = design.drawings.as_ref().and_then(|d| d.shapes.iter().find(|s| s.id() == id.as_str())) {
            out.push((id.clone(), Item::Shape(s)));
        } else if let Some(rt) = design.routing.as_ref() {
            if let Some(z) = rt.zones.iter().find(|z| &z.id == id) {
                out.push((id.clone(), Item::Zone(z)));
            } else if let Some(t) = rt.tracks.iter().find(|t| &t.id == id) {
                out.push((id.clone(), Item::Track(t)));
            } else if let Some(v) = rt.vias.iter().find(|v| &v.id == id) {
                out.push((id.clone(), Item::Via(v)));
            }
        }
    }
    out
}

/// The ring of a closed graphic as `makePolysFromClosedGraphics` reads it (the raw shape, line width ignored), its stroke width and whether it is filled.
fn closed_ring(item: &Item, strategy: ConvertStrategy) -> Option<(Vec<Point64>, i64, bool)> {
    match item {
        Item::Shape(Shape::Rect { start, end, stroke_width, filled, .. }) => {
            let ring = vec![pt64(*start), Point64::new(end.x, start.y), pt64(*end), Point64::new(start.x, end.y)];
            Some((ring, *stroke_width, *filled))
        }
        Item::Shape(Shape::Circle { center, end, stroke_width, filled, .. }) => {
            let r = ((end.x - center.x) as f64).hypot((end.y - center.y) as f64).round() as i64;
            if r <= 0 {
                return None;
            }
            // `CIRCLE && aStrategy != BOUNDING_HULL`: the circle as two arcs; the hull takes the shape's polygon like any other.
            let _ = strategy;
            Some((circle_ring(*center, r), *stroke_width, *filled))
        }
        Item::Shape(Shape::Polygon { pts, stroke_width, filled, .. }) if pts.len() >= 3 => Some((pts.iter().map(|p| pt64(*p)).collect(), *stroke_width, *filled)),
        Item::Zone(z) if z.outline.len() >= 3 => Some((z.outline.iter().map(|p| pt64(*p)).collect(), 0, true)),
        _ => None,
    }
}

// ------------------------------------------------------------------------------- chained segments

/// One piece `makePolysFromChainedSegs` can chain: a segment, an arc or a curve with a start and an end (`getStartEndPoints`).
struct Edge {
    owner: String,
    /// Its points start to end (two for a segment, the arc's or curve's polyline otherwise).
    pts: Vec<Point>,
}

impl Edge {
    fn start(&self) -> Point {
        self.pts[0]
    }
    fn end(&self) -> Point {
        self.pts[self.pts.len() - 1]
    }
}

fn edges_of(items: &[(String, Item)]) -> Vec<Edge> {
    let mut edges = Vec::new();
    for (id, item) in items {
        match item {
            Item::Shape(Shape::Segment { start, end, .. }) if start != end => edges.push(Edge { owner: id.clone(), pts: vec![*start, *end] }),
            Item::Shape(Shape::Arc { start, mid, end, .. }) if start != end => edges.push(Edge { owner: id.clone(), pts: arc_points(*start, *mid, *end) }),
            Item::Shape(s @ Shape::Bezier { start, end, .. }) if start != end => {
                if let Some(pts) = s.bezier_points() {
                    if pts.len() >= 2 {
                        edges.push(Edge { owner: id.clone(), pts });
                    }
                }
            }
            Item::Track(t) => {
                if let Some((a, m, b)) = t.arc() {
                    if a != b {
                        edges.push(Edge { owner: id.clone(), pts: arc_points(a, m, b) });
                    }
                } else {
                    for w in t.pts.windows(2) {
                        if w[0] != w[1] {
                            edges.push(Edge { owner: id.clone(), pts: vec![w[0], w[1]] });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    edges
}

struct Chainer {
    edges: Vec<Edge>,
    /// `connections`: for each meeting point the edges that start or end there, in insertion order.
    at: BTreeMap<(i64, i64), Vec<usize>>,
    /// `SKIP_STRUCT`.
    flagged: Vec<bool>,
}

fn key(p: Point) -> (i64, i64) {
    (p.x, p.y)
}

impl Chainer {
    fn new(edges: Vec<Edge>) -> Self {
        let mut at: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
        for (i, e) in edges.iter().enumerate() {
            at.entry(key(e.start())).or_default().push(i);
            at.entry(key(e.end())).or_default().push(i);
        }
        let flagged = vec![false; edges.len()];
        Chainer { edges, at, flagged }
    }

    /// The `insert` lambda: the edge, entered at `anchor`, appended (or, walking left, prepended) to the outline.
    fn insert(&self, edge: usize, anchor: Point, forward: bool, outline: &mut Vec<Point>, inserted: &mut Vec<usize>) {
        let e = &self.edges[edge];
        // Points from the anchor's end to the far end.
        let run: Vec<Point> = if anchor == e.start() { e.pts.clone() } else { e.pts.iter().rev().copied().collect() };
        if forward {
            for p in run {
                if outline.last() != Some(&p) {
                    outline.push(p);
                }
            }
        } else {
            // Walking left, what lies beyond the anchor goes in front of what is there (the anchor itself is already its first point).
            for p in run.into_iter().skip(1) {
                if outline.first() != Some(&p) {
                    outline.insert(0, p);
                }
            }
        }
        inserted.push(edge);
    }

    /// The `process` lambda: flag the edge, insert it, and carry on through every other edge meeting at its far end.
    fn process(&mut self, edge: usize, anchor: Point, forward: bool, outline: &mut Vec<Point>, inserted: &mut Vec<usize>) {
        if self.flagged[edge] {
            return;
        }
        self.flagged[edge] = true;
        self.insert(edge, anchor, forward, outline, inserted);
        let (a, b) = (self.edges[edge].start(), self.edges[edge].end());
        let next = if anchor == a { b } else { a };
        let neighbours = self.at.get(&key(next)).cloned().unwrap_or_default();
        for other in neighbours {
            if other == edge {
                continue;
            }
            self.process(other, next, forward, outline, inserted);
        }
    }

    /// `makePolysFromChainedSegs`: every closed loop of chained edges becomes an outline; open chains stay unflagged.
    fn closed_loops(&mut self) -> Vec<(Vec<Point>, Vec<usize>)> {
        let mut out = Vec::new();
        for candidate in 0..self.edges.len() {
            if self.flagged[candidate] {
                continue;
            }
            let mut outline: Vec<Point> = Vec::new();
            let mut inserted: Vec<usize> = Vec::new();
            let (a, b) = (self.edges[candidate].start(), self.edges[candidate].end());
            // "Start with the first object and walk right": the candidate is entered at its end `b`, so the walk leaves through `a`.
            self.process(candidate, b, true, &mut outline, &mut inserted);
            // "Check for any candidates on the left".
            let left = self.at.get(&key(a)).and_then(|v| v.iter().copied().find(|&e| e != candidate));
            if let Some(left) = left {
                self.process(left, a, false, &mut outline, &mut inserted);
            }
            if outline.len() < 3 || outline[0] != outline[outline.len() - 1] {
                for i in inserted {
                    self.flagged[i] = false;
                }
                continue;
            }
            // The walk ends where it began: drop the repeated closing vertex.
            outline.pop();
            if outline.len() < 3 {
                for i in inserted {
                    self.flagged[i] = false;
                }
                continue;
            }
            out.push((outline, inserted));
        }
        out
    }
}

// ------------------------------------------------------------------------------------ the strategies

fn open_graphic_polys(items: &[(String, Item)], flagged: &mut Vec<String>, set: &mut ShapePolySet) {
    for (id, item) in items {
        if flagged.contains(id) {
            continue;
        }
        let stroked = match item {
            Item::Shape(Shape::Segment { start, end, stroke_width, .. }) => Some(stroke(&[*start, *end], *stroke_width)),
            Item::Shape(Shape::Arc { start, mid, end, stroke_width, .. }) => Some(stroke(&arc_points(*start, *mid, *end), *stroke_width)),
            Item::Shape(s @ Shape::Bezier { stroke_width, .. }) => s.bezier_points().map(|p| stroke(&p, *stroke_width)),
            Item::Track(t) => Some(match t.arc() {
                Some((a, m, b)) => stroke(&arc_points(a, m, b), t.width),
                None => stroke(&t.pts, t.width),
            }),
            // `PCB_VIA::TransformShapeToPolygon( UNDEFINED_LAYER )`: the via's diameter.
            Item::Via(v) => Some(ShapePolySet::from_outline(circle_ring(v.at, (v.diameter / 2).max(1)))),
            _ => None,
        };
        if let Some(poly) = stroked {
            for p in poly.polys {
                set.add_polygon(p);
            }
            flagged.push(id.clone());
        }
    }
}

/// `CONVERT_TOOL::CreatePolys`' `getPolys` over `ids` (selection order). `gap` is the already-resolved hull gap (`m_Gap`, plus half the line
/// width when positive -- the caller adds it, as `resolvedSettings` does).
pub fn polys_from_items(design: &Design, ids: &[String], strategy: ConvertStrategy, gap: i64) -> ConvertedPolys {
    let items = find_items(design, ids);
    let mut set = ShapePolySet::new();
    let mut consumed: Vec<String> = Vec::new();

    // makePolysFromClosedGraphics
    for (id, item) in &items {
        let Some((ring, width, filled)) = closed_ring(item, strategy) else { continue };
        if strategy == ConvertStrategy::BoundingHull {
            // `TransformShapeToPolygon( ..., ignoreLineWidth = false )`: the shape with its line (a filled one grown by half the width, an outline one as a ring).
            let zone = matches!(item, Item::Zone(_));
            let hull = if zone { ShapePolySet::from_outline(ring) } else { stroke_closed(&ring, width, filled) };
            for p in hull.polys {
                set.add_polygon(p);
            }
        } else {
            set.add_outline(ring);
        }
        consumed.push(id.clone());
    }

    if strategy == ConvertStrategy::BoundingHull {
        // makePolysFromOpenGraphics( items, 0 ), then `Simplify()` and `Inflate( gap, ROUND_ALL_CORNERS )`.
        open_graphic_polys(&items, &mut consumed, &mut set);
        set.simplify();
        if gap != 0 {
            set.inflate(gap, CornerStrategy::RoundAllCorners, 1, false);
        }
    } else {
        let mut chainer = Chainer::new(edges_of(&items));
        for (outline, inserted) in chainer.closed_loops() {
            set.add_outline(outline.iter().map(|p| pt64(*p)).collect());
            for i in inserted {
                let owner = chainer.edges[i].owner.clone();
                if !consumed.contains(&owner) {
                    consumed.push(owner);
                }
            }
        }
    }

    let mut rings = Vec::new();
    if !set.is_empty() {
        // Hulls and the union may leave holes; the IR polygon bridges them into the ring.
        let mut fractured = set.clone();
        fractured.fracture(false);
        for poly in &fractured.polys {
            let ring: Vec<Point> = poly[0].iter().map(pt).collect();
            if ring.len() >= 3 {
                rings.push(ring);
            }
        }
    }
    ConvertedPolys { rings, consumed }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{DrawingsSection, PlacementSection, Provenance, RoutingSection};

    fn design(shapes: Vec<Shape>) -> Design {
        let mut dr = DrawingsSection { shapes, ..Default::default() };
        dr.assign_missing_ids();
        Design {
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "test".into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection { outline: vec![], footprints: vec![], modules: vec![] }),
            routing: Some(RoutingSection { tracks: vec![], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }),
            drawings: Some(dr),
        }
    }

    fn seg(a: (i64, i64), b: (i64, i64), w: i64) -> Shape {
        Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: w, filled: false, start: Point { x: a.0, y: a.1 }, end: Point { x: b.0, y: b.1 } }
    }

    fn rect(x0: i64, y0: i64, x1: i64, y1: i64, w: i64) -> Shape {
        Shape::Rect { id: String::new(), layer: "F.Fab".into(), stroke_width: w, filled: false, start: Point { x: x0, y: y0 }, end: Point { x: x1, y: y1 } }
    }

    fn ids(d: &Design) -> Vec<String> {
        d.drawings.as_ref().unwrap().shapes.iter().map(|s| s.id().to_string()).collect()
    }

    fn area(ring: &[Point]) -> f64 {
        let mut a = 0.0;
        for i in 0..ring.len() {
            let (p, q) = (ring[i], ring[(i + 1) % ring.len()]);
            a += (p.x * q.y - q.x * p.y) as f64;
        }
        a.abs() / 2.0
    }

    #[test]
    fn a_rectangle_is_its_own_outline() {
        let d = design(vec![rect(0, 0, 10_000, 5_000, 200)]);
        let r = polys_from_items(&d, &ids(&d), ConvertStrategy::Centerline, 0);
        assert_eq!(r.rings.len(), 1);
        assert_eq!(r.rings[0].len(), 4);
        assert!((area(&r.rings[0]) - 50_000_000.0).abs() < 1.0);
        assert_eq!(r.consumed, ids(&d));
    }

    #[test]
    fn a_circle_becomes_a_polygon_on_the_circle() {
        let d = design(vec![Shape::Circle { id: String::new(), layer: "F.Fab".into(), stroke_width: 100, filled: false, center: Point { x: 1_000, y: 2_000 }, end: Point { x: 6_000, y: 2_000 } }]);
        let r = polys_from_items(&d, &ids(&d), ConvertStrategy::Centerline, 0);
        assert_eq!(r.rings.len(), 1);
        assert!(r.rings[0].len() >= 16);
        for p in &r.rings[0] {
            let dist = ((p.x - 1_000) as f64).hypot((p.y - 2_000) as f64);
            assert!((dist - 5_000.0).abs() < 2.0, "{dist}");
        }
        let full = std::f64::consts::PI * 25_000_000.0;
        assert!((area(&r.rings[0]) - full).abs() / full < 0.001);
    }

    #[test]
    fn four_segments_chain_into_a_square() {
        let d = design(vec![seg((0, 0), (4_000, 0), 150), seg((4_000, 0), (4_000, 3_000), 150), seg((4_000, 3_000), (0, 3_000), 150), seg((0, 3_000), (0, 0), 150)]);
        let r = polys_from_items(&d, &ids(&d), ConvertStrategy::Centerline, 0);
        assert_eq!(r.rings.len(), 1, "{:?}", r.rings);
        assert_eq!(r.rings[0].len(), 4);
        assert!((area(&r.rings[0]) - 12_000_000.0).abs() < 1.0);
        assert_eq!(r.consumed.len(), 4);
    }

    #[test]
    fn segments_given_in_any_order_still_chain() {
        let d = design(vec![seg((4_000, 3_000), (0, 3_000), 150), seg((0, 0), (4_000, 0), 150), seg((0, 3_000), (0, 0), 150), seg((4_000, 0), (4_000, 3_000), 150)]);
        let r = polys_from_items(&d, &ids(&d), ConvertStrategy::Centerline, 0);
        assert_eq!(r.rings.len(), 1);
        assert!((area(&r.rings[0]) - 12_000_000.0).abs() < 1.0);
    }

    #[test]
    fn an_open_chain_makes_nothing_and_consumes_nothing() {
        let d = design(vec![seg((0, 0), (4_000, 0), 150), seg((4_000, 0), (4_000, 3_000), 150)]);
        let r = polys_from_items(&d, &ids(&d), ConvertStrategy::Centerline, 0);
        assert!(r.rings.is_empty());
        assert!(r.consumed.is_empty());
    }

    #[test]
    fn an_arc_and_its_chord_close_a_half_disc() {
        let arc = Shape::Arc { id: String::new(), layer: "F.SilkS".into(), stroke_width: 150, filled: false, start: Point { x: -5_000, y: 0 }, mid: Point { x: 0, y: -5_000 }, end: Point { x: 5_000, y: 0 } };
        let chord = seg((5_000, 0), (-5_000, 0), 150);
        let d = design(vec![arc, chord]);
        let r = polys_from_items(&d, &ids(&d), ConvertStrategy::Centerline, 0);
        assert_eq!(r.rings.len(), 1);
        assert!(r.rings[0].len() > 12, "{} points", r.rings[0].len());
        let half = std::f64::consts::PI * 25_000_000.0 / 2.0;
        assert!((area(&r.rings[0]) - half).abs() / half < 0.01, "{}", area(&r.rings[0]));
        assert_eq!(r.consumed.len(), 2);
    }

    #[test]
    fn the_hull_of_two_crossing_lines_is_one_polygon_with_their_width() {
        let d = design(vec![seg((0, 0), (10_000, 0), 1_000), seg((5_000, -5_000), (5_000, 5_000), 1_000)]);
        let r = polys_from_items(&d, &ids(&d), ConvertStrategy::BoundingHull, 0);
        assert_eq!(r.rings.len(), 1);
        // a plus sign: 2 bars of (10000+1000) x 1000 and (10000+1000) x 1000 overlapping in 1000 x 1000, round caps trim the ends
        let expected = 2.0 * (10_000.0 * 1_000.0 + std::f64::consts::PI * 500.0 * 500.0) - 1_000_000.0;
        assert!((area(&r.rings[0]) - expected).abs() / expected < 0.01, "{} vs {}", area(&r.rings[0]), expected);
        assert_eq!(r.consumed.len(), 2);
    }

    #[test]
    fn the_hull_gap_grows_the_polygon() {
        let d = design(vec![seg((0, 0), (10_000, 0), 1_000)]);
        let plain = polys_from_items(&d, &ids(&d), ConvertStrategy::BoundingHull, 0);
        let grown = polys_from_items(&d, &ids(&d), ConvertStrategy::BoundingHull, 500);
        assert!(area(&grown.rings[0]) > area(&plain.rings[0]) * 1.5);
    }

    #[test]
    fn the_hull_of_an_outline_rectangle_is_a_ring_bridged_into_one_polygon() {
        let d = design(vec![rect(0, 0, 10_000, 10_000, 200)]);
        let r = polys_from_items(&d, &ids(&d), ConvertStrategy::BoundingHull, 0);
        assert_eq!(r.rings.len(), 1);
        // (10200^2 - 9800^2) minus the rounded outer corners
        let expected = 10_200.0_f64.powi(2) - 9_800.0_f64.powi(2);
        assert!((area(&r.rings[0]) - expected).abs() / expected < 0.02, "{}", area(&r.rings[0]));
        assert!(r.rings[0].len() > 8);
    }

    #[test]
    fn tracks_and_vias_join_the_hull() {
        let mut d = design(vec![]);
        let rt = d.routing.as_mut().unwrap();
        rt.tracks.push(Track { id: "t1".into(), net: "N".into(), pins: vec![], layer: "F.Cu".into(), width: 500, pts: vec![Point { x: 0, y: 0 }, Point { x: 4_000, y: 0 }], arc_mid_offset: None });
        rt.vias.push(Via { id: "v1".into(), net: "N".into(), at: Point { x: 4_000, y: 0 }, drill: 300, diameter: 800, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        let r = polys_from_items(&d, &["t1".to_string(), "v1".to_string()], ConvertStrategy::BoundingHull, 0);
        assert_eq!(r.rings.len(), 1);
        assert_eq!(r.consumed, vec!["t1".to_string(), "v1".to_string()]);
    }

    #[test]
    fn a_zone_outline_is_taken_as_it_is() {
        let mut d = design(vec![]);
        let rt = d.routing.as_mut().unwrap();
        rt.zones.push(Zone { id: "z1".into(), net: "GND".into(), layer: "F.Cu".into(), outline: vec![Point { x: 0, y: 0 }, Point { x: 3_000, y: 0 }, Point { x: 3_000, y: 3_000 }], ..Default::default() });
        let r = polys_from_items(&d, &["z1".to_string()], ConvertStrategy::Centerline, 0);
        assert_eq!(r.rings.len(), 1);
        assert_eq!(r.rings[0].len(), 3);
        assert_eq!(r.consumed, vec!["z1".to_string()]);
    }

    #[test]
    fn unknown_ids_are_ignored() {
        let d = design(vec![rect(0, 0, 100, 100, 10)]);
        let r = polys_from_items(&d, &["nope".to_string()], ConvertStrategy::Centerline, 0);
        assert!(r.rings.is_empty() && r.consumed.is_empty());
    }
}
