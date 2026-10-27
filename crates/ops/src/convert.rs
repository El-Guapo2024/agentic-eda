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
//! items).

use eda_model::units::Nm;
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

/// `bds.m_MaxError` (`ARC_HIGH_DEF`, 0.005 mm), the error every shape of the conversion is polygonised and inflated to.
const MAX_ERROR_NM: i32 = 5_000;

fn pt64(p: Point) -> Point64 {
    Point64::new(p.x.get(), p.y.get())
}

fn pt(p: &Point64) -> Point {
    Point { x: Nm::new(p.x), y: Nm::new(p.y) }
}

fn norm(a: f64) -> f64 {
    a.rem_euclid(std::f64::consts::TAU)
}

/// An arc's polyline within [`MAX_ERROR_NM`] of the curve (`SHAPE_ARC::ConvertToPolyline`): the sweep through `mid`, split by
/// `GetArcToSegmentCount( radius, error )` per full turn; endpoints exact. Collinear points give the chord.
pub fn arc_points(start: Point, mid: Point, end: Point) -> Vec<Point> {
    let (sx, sy, mx, my, ex, ey) = (start.x.as_f64(), start.y.as_f64(), mid.x.as_f64(), mid.y.as_f64(), end.x.as_f64(), end.y.as_f64());
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
    let per_turn = get_arc_to_segment_count(r.round().clamp(1.0, i32::MAX as f64) as i32, MAX_ERROR_NM).max(4) as f64;
    let n = ((sweep.abs() / std::f64::consts::TAU * per_turn).ceil() as usize).clamp(2, 4096);
    tessellate_arc(start, mid, end, n)
}

/// A circle as `SHAPE_ARC( centre - (R, 0), centre + (R, 0), centre - (R, 0) )` polygonises: `n` points on the circle.
fn circle_ring(center: Point, r: Nm) -> Vec<Point64> {
    let n = get_arc_to_segment_count(r.get().clamp(1, i32::MAX as i64) as i32, MAX_ERROR_NM).max(8) as usize;
    (0..n)
        .map(|k| {
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            Point64::new((center.x.as_f64() + r.as_f64() * a.cos()).round() as i64, (center.y.as_f64() + r.as_f64() * a.sin()).round() as i64)
        })
        .collect()
}

/// A stadium (`TransformOvalToPolygon`): the segment `a`-`b` with round caps of radius `r`.
fn stadium_ring(a: Point, b: Point, r: Nm) -> Vec<Point64> {
    let r = r.max(Nm::new(1));
    let (dx, dy) = ((b.x - a.x).as_f64(), (b.y - a.y).as_f64());
    if dx.hypot(dy) < 1.0 {
        return circle_ring(a, r);
    }
    let base = dy.atan2(dx);
    let half = (get_arc_to_segment_count(r.get().clamp(1, i32::MAX as i64) as i32, MAX_ERROR_NM).max(8) as usize / 2).max(2);
    let cap = |c: Point, from: f64| -> Vec<Point64> {
        (0..=half)
            .map(|i| {
                let t = from + std::f64::consts::PI * i as f64 / half as f64;
                Point64::new((c.x.as_f64() + r.as_f64() * t.cos()).round() as i64, (c.y.as_f64() + r.as_f64() * t.sin()).round() as i64)
            })
            .collect()
    };
    let mut out = cap(b, base - std::f64::consts::FRAC_PI_2);
    out.extend(cap(a, base + std::f64::consts::FRAC_PI_2));
    out
}

/// A polyline stroked `width` wide with round caps and joins (`TransformShapeToPolygon` of a track / segment / arc).
fn stroke(pts: &[Point], width: Nm) -> ShapePolySet {
    let mut set = ShapePolySet::new();
    let r = (width / 2).max(Nm::new(1));
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
fn stroke_closed(ring: &[Point64], width: Nm, filled: bool) -> ShapePolySet {
    let half = (width / 2).max(Nm::ZERO).get();
    let base = ShapePolySet::from_outline(ring.to_vec());
    if half == 0 {
        return base;
    }
    let mut outer = base.clone();
    outer.inflate(half, CornerStrategy::RoundAllCorners, MAX_ERROR_NM, false);
    if filled {
        return outer;
    }
    let mut inner = base;
    inner.deflate(half, CornerStrategy::RoundAllCorners, MAX_ERROR_NM);
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
/// A rectangle with a corner radius (`GetCornerRadius() > 0`, `DrawingsSection::board_items`) is its rounded outline (`TransformRoundChamferedRectToPolygon`,
/// here `ROUNDRECT::TransformToPolygon`'s arcs at the same error, `eda_model::outline::rounded_rect_ring`; round 5 B5-8).
fn closed_ring(item: &Item, strategy: ConvertStrategy, corner_radius: Nm) -> Option<(Vec<Point64>, Nm, bool)> {
    match item {
        Item::Shape(Shape::Rect { start, end, stroke_width, filled, .. }) if corner_radius > Nm::ZERO => {
            let ring = eda_model::outline::rounded_rect_ring(*start, *end, corner_radius, MAX_ERROR_NM as f64).into_iter().map(pt64).collect();
            Some((ring, *stroke_width, *filled))
        }
        Item::Shape(Shape::Rect { start, end, stroke_width, filled, .. }) => {
            let ring = vec![pt64(*start), pt64(Point { x: end.x, y: start.y }), pt64(*end), pt64(Point { x: start.x, y: end.y })];
            Some((ring, *stroke_width, *filled))
        }
        Item::Shape(Shape::Circle { center, end, stroke_width, filled, .. }) => {
            let r = Nm::kiround((end.x - center.x).as_f64().hypot((end.y - center.y).as_f64()));
            if r <= Nm::ZERO {
                return None;
            }
            // `CIRCLE && aStrategy != BOUNDING_HULL`: the circle as two arcs; the hull takes the shape's polygon like any other.
            let _ = strategy;
            Some((circle_ring(*center, r), *stroke_width, *filled))
        }
        Item::Shape(Shape::Polygon { pts, stroke_width, filled, .. }) if pts.len() >= 3 => Some((pts.iter().map(|p| pt64(*p)).collect(), *stroke_width, *filled)),
        Item::Zone(z) if z.outline.len() >= 3 => Some((z.outline.iter().map(|p| pt64(*p)).collect(), Nm::ZERO, true)),
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
    (p.x.get(), p.y.get())
}

/// `chainingEpsilon = 100`: "max dist from one endPt to next startPt in IU" (`closeEnough`: the squared distance at most its square).
const CHAINING_EPSILON_NM: i64 = 100;

impl Chainer {
    fn new(mut edges: Vec<Edge>) -> Self {
        // `findInsertionPoint`: an end within the chaining epsilon of a point already in `connections` is filed under that point (round 5 B5-13). The edge's end
        // is moved onto it, so the walk meets it exactly; KiCad's walk, which appends the far end of each item it enters, drops the near-miss point the same way.
        let mut keys: Vec<Point> = Vec::new();
        let canon = |p: Point, keys: &mut Vec<Point>| -> Point {
            if keys.contains(&p) {
                return p;
            }
            let close = |q: &&Point| {
                let (dx, dy) = ((p.x - q.x).get() as i128, (p.y - q.y).get() as i128);
                dx * dx + dy * dy <= (CHAINING_EPSILON_NM as i128).pow(2)
            };
            if let Some(q) = keys.iter().find(close) {
                return *q;
            }
            keys.push(p);
            p
        };
        for e in edges.iter_mut() {
            let n = e.pts.len();
            e.pts[0] = canon(e.pts[0], &mut keys);
            e.pts[n - 1] = canon(e.pts[n - 1], &mut keys);
        }
        edges.retain(|e| e.start() != e.end() || e.pts.len() > 2);
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
            Item::Via(v) => Some(ShapePolySet::from_outline(circle_ring(v.at, (v.diameter / 2).max(Nm::new(1))))),
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
pub fn polys_from_items(design: &Design, ids: &[String], strategy: ConvertStrategy, gap: Nm) -> ConvertedPolys {
    let items = find_items(design, ids);
    let mut set = ShapePolySet::new();
    let mut consumed: Vec<String> = Vec::new();

    // makePolysFromClosedGraphics
    for (id, item) in &items {
        let radius = design.drawings.as_ref().map(|d| d.board_items.corner_radius_of(id)).unwrap_or(Nm::ZERO);
        let Some((ring, width, filled)) = closed_ring(item, strategy, radius) else { continue };
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
        if gap != Nm::ZERO {
            set.inflate(gap.get(), CornerStrategy::RoundAllCorners, MAX_ERROR_NM, false);
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
    use eda_model::units::mm;
    use eda_model::units::Nm;
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
            schema: eda_model::ir::SCHEMA_VERSION,
            provenance: Provenance { engine_version: "test".into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection { outline: vec![], footprints: vec![], modules: vec![] }),
            routing: Some(RoutingSection { tracks: vec![], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], diff_pair_presets: vec![], teardrop_settings: Default::default() }),
            drawings: Some(dr),
        }
    }

    fn seg(a: (Nm, Nm), b: (Nm, Nm), w: Nm) -> Shape {
        Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: w, filled: false, start: Point { x: a.0, y: a.1 }, end: Point { x: b.0, y: b.1 } }
    }

    fn rect(x0: Nm, y0: Nm, x1: Nm, y1: Nm, w: Nm) -> Shape {
        Shape::Rect { id: String::new(), layer: "F.Fab".into(), stroke_width: w, filled: false, start: Point { x: x0, y: y0 }, end: Point { x: x1, y: y1 } }
    }

    fn ids(d: &Design) -> Vec<String> {
        d.drawings.as_ref().unwrap().shapes.iter().map(|s| s.id().to_string()).collect()
    }

    fn area(ring: &[Point]) -> f64 {
        let mut a = 0.0;
        for i in 0..ring.len() {
            let (p, q) = (ring[i], ring[(i + 1) % ring.len()]);
            a += (p.x.area(q.y) - q.x.area(p.y)) as f64;
        }
        a.abs() / 2.0
    }

    #[test]
    fn a_rectangle_is_its_own_outline() {
        let d = design(vec![rect(Nm::ZERO, Nm::ZERO, mm(10.0), mm(5.0), mm(0.2))]);
        let r = polys_from_items(&d, &ids(&d), ConvertStrategy::Centerline, Nm::ZERO);
        assert_eq!(r.rings.len(), 1);
        assert_eq!(r.rings[0].len(), 4);
        assert!((area(&r.rings[0]) - 50_000_000_000_000.0).abs() < 1e6);
        assert_eq!(r.consumed, ids(&d));
    }

    #[test]
    fn a_circle_becomes_a_polygon_on_the_circle() {
        let d = design(vec![Shape::Circle { id: String::new(), layer: "F.Fab".into(), stroke_width: mm(0.1), filled: false, center: Point { x: mm(1.0), y: mm(2.0) }, end: Point { x: mm(6.0), y: mm(2.0) } }]);
        let r = polys_from_items(&d, &ids(&d), ConvertStrategy::Centerline, Nm::ZERO);
        assert_eq!(r.rings.len(), 1);
        assert!(r.rings[0].len() >= 16);
        for p in &r.rings[0] {
            let dist = (p.x - mm(1.0)).as_f64().hypot((p.y - mm(2.0)).as_f64());
            assert!((dist - 5_000_000.0).abs() < 2_000.0, "{dist}");
        }
        let full = std::f64::consts::PI * 25_000_000_000_000.0;
        // the polygon of KiCad's `m_MaxError` (5 um, was the 1 um this port took): its area within 0.2 % of the circle's
        assert!((area(&r.rings[0]) - full).abs() / full < 0.002);
    }

    #[test]
    fn four_segments_chain_into_a_square() {
        let d = design(vec![seg((Nm::ZERO, Nm::ZERO), (mm(4.0), Nm::ZERO), mm(0.15)), seg((mm(4.0), Nm::ZERO), (mm(4.0), mm(3.0)), mm(0.15)), seg((mm(4.0), mm(3.0)), (Nm::ZERO, mm(3.0)), mm(0.15)), seg((Nm::ZERO, mm(3.0)), (Nm::ZERO, Nm::ZERO), mm(0.15))]);
        let r = polys_from_items(&d, &ids(&d), ConvertStrategy::Centerline, Nm::ZERO);
        assert_eq!(r.rings.len(), 1, "{:?}", r.rings);
        assert_eq!(r.rings[0].len(), 4);
        assert!((area(&r.rings[0]) - 12_000_000_000_000.0).abs() < 1e6);
        assert_eq!(r.consumed.len(), 4);
    }

    #[test]
    fn segments_given_in_any_order_still_chain() {
        let d = design(vec![seg((mm(4.0), mm(3.0)), (Nm::ZERO, mm(3.0)), mm(0.15)), seg((Nm::ZERO, Nm::ZERO), (mm(4.0), Nm::ZERO), mm(0.15)), seg((Nm::ZERO, mm(3.0)), (Nm::ZERO, Nm::ZERO), mm(0.15)), seg((mm(4.0), Nm::ZERO), (mm(4.0), mm(3.0)), mm(0.15))]);
        let r = polys_from_items(&d, &ids(&d), ConvertStrategy::Centerline, Nm::ZERO);
        assert_eq!(r.rings.len(), 1);
        assert!((area(&r.rings[0]) - 12_000_000_000_000.0).abs() < 1e6);
    }

    #[test]
    fn an_open_chain_makes_nothing_and_consumes_nothing() {
        let d = design(vec![seg((Nm::ZERO, Nm::ZERO), (mm(4.0), Nm::ZERO), mm(0.15)), seg((mm(4.0), Nm::ZERO), (mm(4.0), mm(3.0)), mm(0.15))]);
        let r = polys_from_items(&d, &ids(&d), ConvertStrategy::Centerline, Nm::ZERO);
        assert!(r.rings.is_empty());
        assert!(r.consumed.is_empty());
    }

    #[test]
    fn an_arc_and_its_chord_close_a_half_disc() {
        let arc = Shape::Arc { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.15), filled: false, start: Point { x: mm(-5.0), y: Nm::ZERO }, mid: Point { x: Nm::ZERO, y: mm(-5.0) }, end: Point { x: mm(5.0), y: Nm::ZERO } };
        let chord = seg((mm(5.0), Nm::ZERO), (mm(-5.0), Nm::ZERO), mm(0.15));
        let d = design(vec![arc, chord]);
        let r = polys_from_items(&d, &ids(&d), ConvertStrategy::Centerline, Nm::ZERO);
        assert_eq!(r.rings.len(), 1);
        assert!(r.rings[0].len() > 12, "{} points", r.rings[0].len());
        let half = std::f64::consts::PI * 25_000_000_000_000.0 / 2.0;
        assert!((area(&r.rings[0]) - half).abs() / half < 0.01, "{}", area(&r.rings[0]));
        assert_eq!(r.consumed.len(), 2);
    }

    #[test]
    fn the_hull_of_two_crossing_lines_is_one_polygon_with_their_width() {
        let d = design(vec![seg((Nm::ZERO, Nm::ZERO), (mm(10.0), Nm::ZERO), mm(1.0)), seg((mm(5.0), mm(-5.0)), (mm(5.0), mm(5.0)), mm(1.0))]);
        let r = polys_from_items(&d, &ids(&d), ConvertStrategy::BoundingHull, Nm::ZERO);
        assert_eq!(r.rings.len(), 1);
        // a plus sign: 2 bars of (10000+1000) x 1000 and (10000+1000) x 1000 overlapping in 1000 x 1000, round caps trim the ends
        let expected = 2.0 * (10_000_000.0 * 1_000_000.0 + std::f64::consts::PI * 500_000.0 * 500_000.0) - 1_000_000_000_000.0;
        assert!((area(&r.rings[0]) - expected).abs() / expected < 0.01, "{} vs {}", area(&r.rings[0]), expected);
        assert_eq!(r.consumed.len(), 2);
    }

    #[test]
    fn the_hull_gap_grows_the_polygon() {
        let d = design(vec![seg((Nm::ZERO, Nm::ZERO), (mm(10.0), Nm::ZERO), mm(1.0))]);
        let plain = polys_from_items(&d, &ids(&d), ConvertStrategy::BoundingHull, Nm::ZERO);
        let grown = polys_from_items(&d, &ids(&d), ConvertStrategy::BoundingHull, mm(0.5));
        assert!(area(&grown.rings[0]) > area(&plain.rings[0]) * 1.5);
    }

    #[test]
    fn the_hull_of_an_outline_rectangle_is_a_ring_bridged_into_one_polygon() {
        let d = design(vec![rect(Nm::ZERO, Nm::ZERO, mm(10.0), mm(10.0), mm(0.2))]);
        let r = polys_from_items(&d, &ids(&d), ConvertStrategy::BoundingHull, Nm::ZERO);
        assert_eq!(r.rings.len(), 1);
        // (10200^2 - 9800^2) minus the rounded outer corners
        let expected = 10_200_000.0_f64.powi(2) - 9_800_000.0_f64.powi(2);
        assert!((area(&r.rings[0]) - expected).abs() / expected < 0.02, "{}", area(&r.rings[0]));
        assert!(r.rings[0].len() > 8);
    }

    #[test]
    fn tracks_and_vias_join_the_hull() {
        let mut d = design(vec![]);
        let rt = d.routing.as_mut().unwrap();
        rt.tracks.push(Track { id: "t1".into(), net: "N".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.5), pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(4.0), y: Nm::ZERO }], arc_mid_offset: None });
        rt.vias.push(Via { id: "v1".into(), net: "N".into(), at: Point { x: mm(4.0), y: Nm::ZERO }, drill: mm(0.3), diameter: mm(0.8), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        let r = polys_from_items(&d, &["t1".to_string(), "v1".to_string()], ConvertStrategy::BoundingHull, Nm::ZERO);
        assert_eq!(r.rings.len(), 1);
        assert_eq!(r.consumed, vec!["t1".to_string(), "v1".to_string()]);
    }

    #[test]
    fn a_zone_outline_is_taken_as_it_is() {
        let mut d = design(vec![]);
        let rt = d.routing.as_mut().unwrap();
        rt.zones.push(Zone { id: "z1".into(), net: "GND".into(), layer: "F.Cu".into(), outline: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(3.0), y: Nm::ZERO }, Point { x: mm(3.0), y: mm(3.0) }], ..Default::default() });
        let r = polys_from_items(&d, &["z1".to_string()], ConvertStrategy::Centerline, Nm::ZERO);
        assert_eq!(r.rings.len(), 1);
        assert_eq!(r.rings[0].len(), 3);
        assert_eq!(r.consumed, vec!["z1".to_string()]);
    }

    /// Round 5's B5-8: a 10 x 5 mm rectangle with 2 mm corners is its rounded outline (`TransformShapeToPolygon` of the `ROUNDRECT`), not four corners.
    #[test]
    fn polys_from_items_keeps_a_rounded_rectangles_corners() {
        let mut d = design(vec![rect(Nm::ZERO, Nm::ZERO, mm(10.0), mm(5.0), mm(0.2))]);
        let id = ids(&d)[0].clone();
        d.drawings.as_mut().unwrap().board_items.set_corner_radius(&id, mm(2.0));
        let r = polys_from_items(&d, &ids(&d), ConvertStrategy::Centerline, Nm::ZERO);
        assert_eq!(r.rings.len(), 1);
        assert!(r.rings[0].len() > 8, "rounded corners: {} points", r.rings[0].len());
        // 50 mm^2 less four corners of (4 - pi) mm^2 each: 50 - 4 * 0.8584 = 46.566 mm^2.
        let expect = (50.0 - 4.0 * (4.0 - std::f64::consts::PI)) * 1e12;
        assert!((area(&r.rings[0]) - expect).abs() < 0.01e12, "area {} vs {expect}", area(&r.rings[0]));
    }

    /// Round 5's B5-13: `makePolysFromChainedSegs` joins ends within `chainingEpsilon = 100` nm -- a triangle with one end 50 nm off is a polygon.
    #[test]
    fn ends_within_100_nm_chain() {
        let p = |x: f64, y: f64| (mm(x), mm(y));
        let off = (mm(0.0) + Nm::new(50), mm(0.0));
        let d = design(vec![seg(p(0.0, 0.0), p(10.0, 0.0), mm(0.1)), seg(p(10.0, 0.0), p(5.0, 8.0), mm(0.1)), seg(p(5.0, 8.0), off, mm(0.1))]);
        let r = polys_from_items(&d, &ids(&d), ConvertStrategy::Centerline, Nm::ZERO);
        assert_eq!(r.rings.len(), 1, "{r:?}");
        assert_eq!(r.rings[0].len(), 3);
        // 150 nm is too far.
        let far = (mm(0.0) + Nm::new(150), mm(0.0));
        let d = design(vec![seg(p(0.0, 0.0), p(10.0, 0.0), mm(0.1)), seg(p(10.0, 0.0), p(5.0, 8.0), mm(0.1)), seg(p(5.0, 8.0), far, mm(0.1))]);
        assert!(polys_from_items(&d, &ids(&d), ConvertStrategy::Centerline, Nm::ZERO).rings.is_empty());
    }

    #[test]
    fn unknown_ids_are_ignored() {
        let d = design(vec![rect(Nm::ZERO, Nm::ZERO, mm(0.1), mm(0.1), mm(0.01))]);
        let r = polys_from_items(&d, &["nope".to_string()], ConvertStrategy::Centerline, Nm::ZERO);
        assert!(r.rings.is_empty() && r.consumed.is_empty());
    }
}
