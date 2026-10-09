//! The board outline the way KiCad builds it: `ConvertOutlineToPolygon` (`pcbnew/convert_shape_list_to_polygon.cpp`), `BuildBoardPolygonOutlines`
//! and `BOARD::GetBoardPolygonOutlines`, with the checks `DRC_TEST_PROVIDER_MISC::testOutline` makes of it.
//!
//! The items are the board's Edge.Cuts ([`eda_model::outline::edge_cuts_shapes`]): lines, arcs, circles, rectangles, polygons and
//! Bezier curves. They are chained end to end, within [`CHAINING_EPSILON_UM`] (0.01 mm) of each other, into closed contours whose
//! curves are flattened at [`MAX_ERROR_UM`] (0.005 mm); a contour inside an even number of others is an outline, inside an odd number a
//! hole of the one around it. Several outlines are allowed, as KiCad allows them; a closed shape on its own (a circle, a rectangle) is a
//! contour; an open chain, a self-intersecting one and a contour crossing another are reported, not silently closed.
//!
//! The zone filler clips to this outline and keeps its islands inside it, the router and the placement gate read it, and the studio draws the
//! 3D board body from it. `kicad-cli pcb drc` makes the same checks on the exported file (`invalid_outline`, "Board has malformed outline"),
//! and stays the engine that judges the board; this port is the live view the interactive consumers need, so a malformed outline is seen
//! while it is being drawn.
//!
//! Differences from the C++, all in how the data is held and none in the geometry:
//! - the KD-tree of end points is a grid of the same epsilon, with the same "two nearest end points" rule (`findNext`);
//! - a footprint's own Edge.Cuts graphics are board-level shapes here (the importer promotes them), so `BuildBoardPolygonOutlines`' pass
//!   over footprints (holes when copper lies outside a footprint's contour) is the ordinary hierarchy test: a closed contour inside the
//!   board is a hole;
//! - a closed shape's own start and end (a circle's centre and rim, a rectangle's corners) are not end points a chain can attach to;
//! - arcs are always flattened (`aAllowUseArcsInPolygons` is false), the setting every consumer except the STEP export uses.
//!
//! Units are micrometres, like the rest of this workspace; KiCad's are nanometres.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use eda_clipper2::Point64;
use eda_model::ir::{Design, Point, Shape, Um};
use eda_model::outline::{self as edge, Arc, CHAINING_EPSILON_UM, MAX_ERROR_UM};
use eda_shape_poly_set::{LineChain, ShapePolySet};

/// One call of KiCad's `OUTLINE_ERROR_HANDLER` (`void( msg, itemA, itemB, pt )`): what is wrong, the Edge.Cuts items it concerns (their ids;
/// `None` where KiCad passes `nullptr`) and where.
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineError {
    /// KiCad's own text: `(not a closed shape)`, `(self-intersecting)`, `(multiple board outlines not supported)`, or one of the
    /// `TestBoardOutlinesGraphicItems` ones (`(rectangle has null or very small size: 0 nm)`).
    pub message: String,
    pub item_a: Option<String>,
    pub item_b: Option<String>,
    pub at: Point,
}

impl OutlineError {
    /// `DRCE_INVALID_OUTLINE`'s title and detail, as a DRC report words it.
    pub fn describe(&self) -> String {
        format!("Board has malformed outline {}", self.message)
    }
}

/// What `BOARD::GetBoardPolygonOutlines` gives back.
#[derive(Debug, Clone, Default)]
pub struct BoardOutline {
    /// The outlines, each with its holes.
    pub polys: ShapePolySet,
    /// The function's return value: the outline is well-formed. When it is not, `polys` is KiCad's guess (see `inferred`) or whatever
    /// the chaining got to, and a consumer that clips to the outline (`ZONE_FILLER`'s `m_brdOutlinesValid`) does not.
    pub valid: bool,
    /// `aInferOutlineIfNecessary`: the edges did not make a closed outline, and `polys` is the rectangle round them.
    pub inferred: bool,
    /// Every call the chaining made of the error handler.
    pub errors: Vec<OutlineError>,
}

impl BoardOutline {
    /// The outline with the largest area, as a ring (the polygon `placement.outline` summarises it as). Empty when there is none.
    pub fn main_ring(&self) -> Vec<Point> {
        let best = (0..self.polys.outline_count()).max_by(|&a, &b| eda_clipper2::area(self.polys.outline(a)).abs().total_cmp(&eda_clipper2::area(self.polys.outline(b)).abs()));
        best.map(|i| self.polys.outline(i).iter().map(|p| Point { x: p.x, y: p.y }).collect()).unwrap_or_default()
    }

    /// Is `p` on the board: inside an outline and in none of its holes? (A point on an edge counts as on.)
    pub fn contains(&self, p: Point) -> bool {
        (0..self.polys.outline_count()).any(|i| {
            (point_inside(self.polys.outline(i), p) || point_on_ring(self.polys.outline(i), p)) && (0..self.polys.hole_count(i)).all(|h| !point_inside(self.polys.hole(i, h), p) || point_on_ring(self.polys.hole(i, h), p))
        })
    }

    /// Is `p` strictly on the board: inside an outline and in none of its holes, not on any edge? (What a courtyard corner must be: a corner
    /// on the edge of the board, or on the rim of a cutout, is not inside it.)
    pub fn contains_strictly(&self, p: Point) -> bool {
        (0..self.polys.outline_count()).any(|i| {
            let outer = self.polys.outline(i);
            point_inside(outer, p) && !point_on_ring(outer, p) && (0..self.polys.hole_count(i)).all(|h| !point_inside(self.polys.hole(i, h), p) && !point_on_ring(self.polys.hole(i, h), p))
        })
    }

    /// `true` when the outline has any hole or more than one outline: a board more than one polygon, which `placement.outline` cannot hold.
    pub fn is_complex(&self) -> bool {
        self.polys.outline_count() > 1 || self.polys.has_holes()
    }

    pub fn bounds(&self) -> Option<(Point, Point)> {
        let mut bb: Option<(Point, Point)> = None;
        for i in 0..self.polys.outline_count() {
            for p in self.polys.outline(i) {
                bb = Some(match bb {
                    None => (Point { x: p.x, y: p.y }, Point { x: p.x, y: p.y }),
                    Some((a, b)) => (Point { x: a.x.min(p.x), y: a.y.min(p.y) }, Point { x: b.x.max(p.x), y: b.y.max(p.y) }),
                });
            }
        }
        bb
    }
}

// ----------------------------------------------------------------------------------------------------------------------
// kimath: SEG, SHAPE_LINE_CHAIN::PointInside / Intersect
// ----------------------------------------------------------------------------------------------------------------------

fn sq_dist(a: Point, b: Point) -> i128 {
    let (dx, dy) = ((a.x - b.x) as i128, (a.y - b.y) as i128);
    dx * dx + dy * dy
}

fn norm(a: Point, b: Point) -> f64 {
    (sq_dist(a, b) as f64).sqrt()
}

/// `close_enough`: the squared distance is at most the squared limit.
fn close_enough(a: Point, b: Point, limit: Um) -> bool {
    sq_dist(a, b) <= (limit as i128) * (limit as i128)
}

/// `closer_to_first`.
fn closer_to_first(reference: Point, first: Point, second: Point) -> bool {
    sq_dist(reference, first) < sq_dist(reference, second)
}

/// `rescale( a, b, c )`: `a * b / c`, rounded to nearest (`util.cpp`).
fn rescale(a: i128, b: i128, c: i128) -> i128 {
    let numerator = a * b;
    if (numerator < 0) ^ (c < 0) {
        (numerator - c / 2) / c
    } else {
        (numerator + c / 2) / c
    }
}

#[derive(Clone, Copy)]
struct Seg {
    a: Point,
    b: Point,
}

impl Seg {
    fn bbox(&self) -> (Um, Um, Um, Um) {
        (self.a.x.min(self.b.x), self.a.y.min(self.b.y), self.a.x.max(self.b.x), self.a.y.max(self.b.y))
    }
}

/// `SEG::checkCollinearOverlap`.
fn collinear_overlap(s: Seg, o: Seg, use_x_axis: bool, ignore_endpoints: bool) -> Option<Point> {
    let (seg1_start, seg1_end, seg2_start, seg2_end, coord1_start, coord1_end) =
        if use_x_axis { (s.a.x, s.b.x, o.a.x, o.b.x, s.a.y, s.b.y) } else { (s.a.y, s.b.y, o.a.y, o.b.y, s.a.x, s.b.x) };
    let (seg1_min, seg1_max) = (seg1_start.min(seg1_end), seg1_start.max(seg1_end));
    let (seg2_min, seg2_max) = (seg2_start.min(seg2_end), seg2_start.max(seg2_end));
    if !(seg1_max >= seg2_min && seg2_max >= seg1_min) {
        return None;
    }
    let overlap_start = seg1_min.max(seg2_min);
    let overlap_end = seg1_max.min(seg2_max);
    if ignore_endpoints && overlap_start == overlap_end {
        // Only an endpoint of both segments touching is ignored.
        if (overlap_start == seg1_min || overlap_start == seg1_max) && (overlap_start == seg2_min || overlap_start == seg2_max) {
            return None;
        }
    }
    let proj = (overlap_start + overlap_end) / 2;
    let other = if seg1_end != seg1_start { coord1_start + rescale((proj - seg1_start) as i128, (coord1_end - coord1_start) as i128, (seg1_end - seg1_start) as i128) as Um } else { coord1_start };
    Some(if use_x_axis { Point { x: proj, y: other } } else { Point { x: other, y: proj } })
}

/// `SEG::Intersect( aSeg, aIgnoreEndpoints )` for two segments (not lines).
fn seg_intersect(s: Seg, o: Seg, ignore_endpoints: bool) -> Option<Point> {
    let (b1, b2) = (s.bbox(), o.bbox());
    if b1.2 < b2.0 || b2.2 < b1.0 || b1.3 < b2.1 || b2.3 < b1.1 {
        return None;
    }
    let (d1x, d1y) = ((s.b.x - s.a.x) as i128, (s.b.y - s.a.y) as i128);
    let (d2x, d2y) = ((o.b.x - o.a.x) as i128, (o.b.y - o.a.y) as i128);
    let (ox, oy) = ((o.a.x - s.a.x) as i128, (o.a.y - s.a.y) as i128);
    let cross = |ax: i128, ay: i128, bx: i128, by: i128| ax * by - ay * bx;
    let determinant = cross(d2x, d2y, d1x, d1y);
    if determinant == 0 {
        // Parallel; collinear only when the offset is parallel too.
        if cross(d1x, d1y, ox, oy) != 0 {
            return None;
        }
        let use_x_axis = d1x.abs() >= d1y.abs();
        return collinear_overlap(s, o, use_x_axis, ignore_endpoints);
    }
    let param2_num = cross(d2x, d2y, ox, oy);
    let param1_num = cross(d1x, d1y, ox, oy);
    if determinant > 0 {
        if param1_num < 0 || param1_num > determinant || param2_num < 0 || param2_num > determinant {
            return None;
        }
    } else if param1_num > 0 || param1_num < determinant || param2_num > 0 || param2_num < determinant {
        return None;
    }
    if ignore_endpoints && (param1_num == 0 || param1_num == determinant) && (param2_num == 0 || param2_num == determinant) {
        return None;
    }
    Some(Point { x: o.a.x + rescale(param1_num, d2x, determinant) as Um, y: o.a.y + rescale(param1_num, d2y, determinant) as Um })
}

fn ring_bbox(ring: &[Point]) -> (Um, Um, Um, Um) {
    let mut bb = (Um::MAX, Um::MAX, Um::MIN, Um::MIN);
    for p in ring {
        bb = (bb.0.min(p.x), bb.1.min(p.y), bb.2.max(p.x), bb.3.max(p.y));
    }
    bb
}

/// `SHAPE_LINE_CHAIN::PointInside( aPt, 0 )` of a closed chain: the even-odd rule, a point on the boundary is on either side.
fn point_inside_ring(ring: &[Point], p: Point) -> bool {
    let n = ring.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    for i in 0..n {
        let (p1, p2) = (ring[i], ring[(i + 1) % n]);
        let (dx, dy) = ((p2.x - p1.x) as i128, (p2.y - p1.y) as i128);
        if dy == 0 {
            continue;
        }
        let d = rescale(dx, (p.y - p1.y) as i128, dy);
        if ((p1.y >= p.y) != (p2.y >= p.y)) && ((p.x - p1.x) as i128) < d {
            inside = !inside;
        }
    }
    inside
}

fn to_ring(chain: &LineChain) -> Vec<Point> {
    chain.iter().map(|p| Point { x: p.x, y: p.y }).collect()
}

fn point_inside(chain: &LineChain, p: Point) -> bool {
    point_inside_ring(&to_ring(chain), p)
}

/// Is `p` on the boundary of the closed `chain` (`SHAPE_LINE_CHAIN::PointOnEdge`, to the micrometre)?
fn point_on_ring(chain: &LineChain, p: Point) -> bool {
    let n = chain.len();
    (0..n).any(|i| {
        let (a, b) = (Point { x: chain[i].x, y: chain[i].y }, Point { x: chain[(i + 1) % n].x, y: chain[(i + 1) % n].y });
        let (dx, dy) = ((b.x - a.x) as i128, (b.y - a.y) as i128);
        let (px, py) = ((p.x - a.x) as i128, (p.y - a.y) as i128);
        if dx * py - dy * px != 0 {
            return false;
        }
        let dot = dx * px + dy * py;
        dot >= 0 && dot <= dx * dx + dy * dy
    })
}

/// `SHAPE_LINE_CHAIN::Intersect( aChain, aIp, true )` on two closed rings: the first place their sides touch or cross. (KiCad's
/// "ExcludeColinearAndTouching" only skips the collinear branch; `SEG::Intersect` still reports a shared end point.)
fn rings_intersect(a: &[Point], b: &[Point]) -> Option<Point> {
    if a.len() < 2 || b.len() < 2 {
        return None;
    }
    let bb = ring_bbox(b);
    let sides = |r: &[Point]| -> Vec<Seg> { (0..r.len()).map(|i| Seg { a: r[i], b: r[(i + 1) % r.len()] }).collect() };
    let theirs = sides(b);
    for s in sides(a) {
        let sb = s.bbox();
        if sb.2 < bb.0 || sb.0 > bb.2 || sb.3 < bb.1 || sb.1 > bb.3 {
            continue;
        }
        for t in &theirs {
            if let Some(p) = seg_intersect(s, *t, false) {
                return Some(p);
            }
        }
    }
    None
}

// ----------------------------------------------------------------------------------------------------------------------
// The contours
// ----------------------------------------------------------------------------------------------------------------------

/// A `SHAPE_LINE_CHAIN` being built: points, whether it is closed (the last joined to the first, not repeated), and where its last arc
/// began, which is all `IsArcEnd` is asked about.
#[derive(Debug, Default, Clone)]
struct Contour {
    pts: Vec<Point>,
    closed: bool,
    /// The index of the point the last arc starts at (it stays), and that arc's middle, while the chain's last point is that arc's end.
    last_arc: Option<(usize, Point)>,
}

impl Contour {
    /// `Append( VECTOR2I )`: a repeat of the last point is dropped.
    fn append(&mut self, p: Point) {
        if self.pts.last() == Some(&p) {
            return;
        }
        self.pts.push(p);
        self.last_arc = None;
    }

    /// `Append( SHAPE_LINE_CHAIN )`: the other chain's first point is dropped when it is where this one ends.
    fn append_chain(&mut self, chain: &[Point], arc_mid: Option<Point>) {
        if chain.is_empty() {
            return;
        }
        if self.pts.last() != Some(&chain[0]) {
            self.pts.push(chain[0]);
        }
        let start_idx = self.pts.len() - 1;
        self.pts.extend_from_slice(&chain[1..]);
        // A chain of two points is a segment, not an arc (`Append( SHAPE_ARC )` marks an arc only above two).
        self.last_arc = match arc_mid {
            Some(mid) if chain.len() > 2 => Some((start_idx, mid)),
            _ => None,
        };
    }

    /// `SetClosed( true )`: the last point is merged into the first when they are the same.
    fn close(&mut self) {
        self.closed = true;
        if self.pts.len() > 1 && self.pts.first() == self.pts.last() {
            self.pts.pop();
        }
        self.last_arc = None;
    }
}

fn to_line_chain(pts: &[Point]) -> LineChain {
    pts.iter().map(|p| Point64::new(p.x, p.y)).collect()
}

// ----------------------------------------------------------------------------------------------------------------------
// ConvertOutlineToPolygon
// ----------------------------------------------------------------------------------------------------------------------

fn is_closed_shape(s: &Shape) -> bool {
    matches!(s, Shape::Polygon { .. } | Shape::Circle { .. } | Shape::Rect { .. })
}

/// The ends of an open shape (`GetStart()`, `GetEnd()`).
fn ends(s: &Shape) -> Option<(Point, Point)> {
    match s {
        Shape::Segment { start, end, .. } | Shape::Arc { start, end, .. } | Shape::Bezier { start, end, .. } => Some((*start, *end)),
        _ => None,
    }
}

/// `PCB_SHAPE_ENDPOINTS_ADAPTOR` and its KD-tree, for `findNext` and the gap report.
struct Endpoints {
    pts: Vec<(Point, usize)>,
    grid: HashMap<(Um, Um), Vec<usize>>,
    cell: Um,
}

impl Endpoints {
    fn new(shapes: &[Shape], epsilon: Um) -> Endpoints {
        let cell = epsilon.max(1);
        let mut pts = Vec::new();
        for (i, s) in shapes.iter().enumerate() {
            if let Some((a, b)) = ends(s) {
                pts.push((a, i));
                pts.push((b, i));
            }
        }
        let mut grid: HashMap<(Um, Um), Vec<usize>> = HashMap::new();
        for (k, (p, _)) in pts.iter().enumerate() {
            grid.entry((p.x.div_euclid(cell), p.y.div_euclid(cell))).or_default().push(k);
        }
        Endpoints { pts, grid, cell }
    }

    /// `knnSearch( 2 )` restricted to what `findNext` can accept: the end points within `epsilon` of `p`, nearest first, at most two.
    fn two_within(&self, p: Point, epsilon: Um) -> Vec<(i128, usize)> {
        let (cx, cy) = (p.x.div_euclid(self.cell), p.y.div_euclid(self.cell));
        let span = (epsilon + self.cell - 1) / self.cell;
        let mut found: Vec<(i128, usize, usize)> = Vec::new();
        for gx in cx - span..=cx + span {
            for gy in cy - span..=cy + span {
                if let Some(list) = self.grid.get(&(gx, gy)) {
                    for &k in list {
                        let d = sq_dist(self.pts[k].0, p);
                        if d < (epsilon as i128) * (epsilon as i128) {
                            found.push((d, k, self.pts[k].1));
                        }
                    }
                }
            }
        }
        found.sort();
        found.into_iter().take(2).map(|(d, _, shape)| (d, shape)).collect()
    }

    /// `knnSearch( 2 )`: the two end points nearest `p` whatever their distance.
    fn two_nearest(&self, p: Point) -> Vec<usize> {
        let mut all: Vec<(i128, usize, usize)> = self.pts.iter().enumerate().map(|(k, (q, shape))| (sq_dist(*q, p), k, *shape)).collect();
        all.sort();
        all.into_iter().take(2).map(|(_, _, shape)| shape).collect()
    }
}

/// The result of one `ConvertOutlineToPolygon`.
struct Converted {
    polys: ShapePolySet,
    success: bool,
    errors: Vec<OutlineError>,
}

struct Chainer<'a> {
    shapes: &'a [Shape],
    max_error: f64,
    epsilon: Um,
    index: Endpoints,
    /// `shapeOwners`: which shape a directed piece of a contour came from.
    owners: BTreeMap<(Point, Point), usize>,
    errors: Vec<OutlineError>,
}

impl<'a> Chainer<'a> {
    fn id(&self, i: usize) -> Option<String> {
        Some(self.shapes[i].id().to_string())
    }

    fn report(&mut self, message: &str, a: Option<usize>, b: Option<usize>, at: Point) {
        let (item_a, item_b) = (a.and_then(|i| self.id(i)), b.and_then(|i| self.id(i)));
        self.errors.push(OutlineError { message: message.to_string(), item_a, item_b, at });
    }

    /// `findNext`: the end point nearest `p` that is not `shape`'s own, within the chaining epsilon.
    fn find_next(&self, shape: usize, p: Point) -> Option<usize> {
        self.index.two_within(p, self.epsilon).into_iter().map(|(_, s)| s).find(|&s| s != shape)
    }

    fn own(&mut self, a: Point, b: Point, shape: usize) {
        self.owners.insert((a, b), shape);
    }

    /// `processClosedShape` (arcs flattened).
    fn process_closed(&mut self, i: usize, contour: &mut Contour) {
        match &self.shapes[i] {
            Shape::Polygon { pts, .. } => {
                let mut prev: Option<Point> = None;
                for &pt in pts {
                    contour.append(pt);
                    if let Some(p) = prev {
                        self.owners.insert((p, pt), i);
                    }
                    prev = Some(pt);
                }
                contour.close();
            }
            Shape::Circle { center, end, .. } => {
                // `SHAPE_ARC arc360( center, start, ANGLE_360, 0 )` appended at `aErrorMax`.
                let ring = edge::circle_contour(*center, *end, self.max_error);
                let mut with_end = ring.clone();
                if let Some(first) = ring.first() {
                    with_end.push(*first);
                }
                contour.append_chain(&with_end, None);
                contour.close();
                for w in contour.pts.windows(2).map(|w| (w[0], w[1])).collect::<Vec<_>>() {
                    self.owners.insert(w, i);
                }
            }
            Shape::Rect { start, end, .. } => {
                let corners = edge::rect_corners(*start, *end);
                let mut prev: Option<Point> = None;
                for pt in corners {
                    contour.append(pt);
                    if let Some(p) = prev {
                        self.owners.insert((p, pt), i);
                    }
                    prev = Some(pt);
                }
                contour.close();
            }
            _ => {}
        }
    }

    /// `processShapeSegment`: add the piece of open shape `i` that continues from `prev`, and move `prev` to its far end.
    fn process_segment(&mut self, i: usize, contour: &mut Contour, prev: &mut Point) {
        match self.shapes[i].clone() {
            Shape::Segment { start, end, .. } => {
                let next = if closer_to_first(*prev, start, end) { end } else { start };
                contour.append(next);
                self.own(*prev, next, i);
                *prev = next;
            }
            Shape::Arc { start, mid, end, .. } => {
                let (mut pstart, mut pend) = (start, end);
                if !close_enough(*prev, pstart, self.epsilon) {
                    if !close_enough(*prev, end, self.epsilon) {
                        return;
                    }
                    std::mem::swap(&mut pstart, &mut pend);
                }
                pstart = *prev;
                let chain = Arc::through(pstart, mid, pend).polyline(self.max_error);
                for w in chain.windows(2) {
                    self.owners.insert((w[0], w[1]), i);
                }
                contour.append_chain(&chain, Some(mid));
                *prev = pend;
            }
            Shape::Bezier { start, c1, c2, end, .. } => {
                let (next, reverse) = if closer_to_first(*prev, start, end) { (end, false) } else { (start, true) };
                let poly = eda_model::bezier::bezier_polyline(start, c1, c2, end, self.max_error.round() as Um);
                let walk: Vec<Point> = if reverse { poly.into_iter().rev().collect() } else { poly };
                for pt in walk {
                    if *prev == pt {
                        continue;
                    }
                    contour.append(pt);
                    self.own(*prev, pt, i);
                    *prev = pt;
                }
                *prev = next;
            }
            _ => {}
        }
    }

    /// The "(not a closed shape)" report for an end of an open contour: the two shapes whose ends are nearest, and the middle of the
    /// gap between them.
    fn report_gap(&mut self, pt: Point, reported: &mut HashSet<(usize, usize)>) {
        let near = self.index.two_nearest(pt);
        let Some(&a) = near.first() else { return };
        let b = near.get(1).copied().unwrap_or(a);
        if !reported.insert((a.min(b), a.max(b))) {
            return;
        }
        let at = if a == b { pt } else { nearest_midpoint(&self.shapes[a], &self.shapes[b], self.max_error).unwrap_or(pt) };
        self.report("(not a closed shape)", Some(a), Some(b), at);
    }
}

/// The middle of the shortest line between two shapes (`NearestPoints` of their effective shapes).
fn nearest_midpoint(a: &Shape, b: &Shape, max_error: f64) -> Option<Point> {
    let segs = |s: &Shape| -> Vec<Seg> {
        edge::shape_chains(s, max_error)
            .into_iter()
            .flat_map(|c| {
                let n = c.pts.len();
                let count = if c.closed { n } else { n.saturating_sub(1) };
                (0..count).map(|i| Seg { a: c.pts[i], b: c.pts[(i + 1) % n] }).collect::<Vec<_>>()
            })
            .collect()
    };
    let (sa, sb) = (segs(a), segs(b));
    let mut best: Option<(f64, Point, Point)> = None;
    for x in &sa {
        for y in &sb {
            let (p, q) = nearest_points(*x, *y);
            let d = norm(p, q);
            if best.is_none_or(|(bd, _, _)| d < bd) {
                best = Some((d, p, q));
            }
        }
    }
    best.map(|(_, p, q)| Point { x: (p.x + q.x) / 2, y: (p.y + q.y) / 2 })
}

fn nearest_on(s: Seg, p: Point) -> Point {
    let (dx, dy) = ((s.b.x - s.a.x) as f64, (s.b.y - s.a.y) as f64);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return s.a;
    }
    let t = ((((p.x - s.a.x) as f64) * dx + ((p.y - s.a.y) as f64) * dy) / len2).clamp(0.0, 1.0);
    Point { x: s.a.x + (t * dx).round() as Um, y: s.a.y + (t * dy).round() as Um }
}

/// `SEG::NearestPoints`.
fn nearest_points(a: Seg, b: Seg) -> (Point, Point) {
    if let Some(p) = seg_intersect(a, b, false) {
        return (p, p);
    }
    let cands = [(a.a, nearest_on(b, a.a)), (a.b, nearest_on(b, a.b)), (nearest_on(a, b.a), b.a), (nearest_on(a, b.b), b.b)];
    cands.into_iter().min_by_key(|(p, q)| sq_dist(*p, *q)).expect("four candidates")
}

/// `buildContourHierarchy`: for each contour, the contours that contain its first point.
fn contour_hierarchy(contours: &[Contour]) -> BTreeMap<usize, Vec<usize>> {
    let boxes: Vec<(Um, Um, Um, Um)> = contours.iter().map(|c| ring_bbox(&c.pts)).collect();
    let mut map = BTreeMap::new();
    for (ii, c) in contours.iter().enumerate() {
        if c.pts.is_empty() {
            continue;
        }
        let first = c.pts[0];
        let parents: Vec<usize> = (0..contours.len())
            .filter(|&jj| {
                jj != ii && {
                    let bb = boxes[jj];
                    first.x >= bb.0 && first.x <= bb.2 && first.y >= bb.1 && first.y <= bb.3 && point_inside_ring(&contours[jj].pts, first)
                }
            })
            .collect();
        map.insert(ii, parents);
    }
    map
}

/// `hasOverlappingClosedContours`.
fn has_overlapping_contours(contours: &[Contour]) -> bool {
    for ii in 0..contours.len() {
        for jj in ii + 1..contours.len() {
            if rings_intersect(&contours[ii].pts, &contours[jj].pts).is_some() {
                return true;
            }
        }
    }
    false
}

/// `doConvertOutlineToPolygon`.
fn convert(shapes: &[Shape], max_error: f64, epsilon: Um, allow_disjoint: bool) -> Converted {
    let mut c = Chainer { shapes, max_error, epsilon, index: Endpoints::new(shapes, epsilon), owners: BTreeMap::new(), errors: Vec::new() };
    let mut polys = ShapePolySet::new();
    if shapes.is_empty() {
        return Converted { polys, success: true, errors: c.errors };
    }

    // `SKIP_STRUCT`: a shape already in a contour.
    let mut used = vec![false; shapes.len()];
    let mut contours: Vec<Contour> = Vec::with_capacity(shapes.len());
    let mut reported_gaps: HashSet<(usize, usize)> = HashSet::new();

    for i in 0..shapes.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let mut contour = Contour::default();

        if is_closed_shape(&shapes[i]) {
            c.process_closed(i, &mut contour);
            contours.push(contour);
            continue;
        }

        // Build the chain of open shapes this one is part of, forwards from its end and then backwards from its start.
        let (first_start, first_end) = ends(&shapes[i]).expect("an open shape has ends");
        let mut chain: VecDeque<usize> = VecDeque::from([i]);
        let (mut front_pt, mut back_pt) = (first_start, first_end);
        let mut closed = false;
        for forward in [true, false] {
            if !forward && closed {
                break;
            }
            let mut curr = if forward { *chain.back().expect("non-empty") } else { *chain.front().expect("non-empty") };
            let mut prev = if forward { back_pt } else { front_pt };
            loop {
                let next = c.find_next(curr, prev);
                if let Some(nx) = next.filter(|&n| !used[n]) {
                    used[nx] = true;
                    if forward {
                        chain.push_back(nx);
                    } else {
                        chain.push_front(nx);
                    }
                    let (s, e) = ends(&shapes[nx]).expect("only open shapes are end points");
                    prev = if closer_to_first(prev, s, e) { e } else { s };
                    curr = nx;
                    continue;
                }
                if let Some(nx) = next {
                    let chain_end = if forward { *chain.front().expect("non-empty") } else { *chain.back().expect("non-empty") };
                    let chain_pt = if forward { front_pt } else { back_pt };
                    if nx == chain_end && close_enough(prev, chain_pt, epsilon) {
                        closed = true;
                    } else {
                        c.report("(self-intersecting)", Some(curr), Some(nx), prev);
                    }
                }
                if forward {
                    back_pt = prev;
                } else {
                    front_pt = prev;
                }
                break;
            }
        }

        // Which end of the first shape the contour starts at: the one that is not shared with the second.
        let first = *chain.front().expect("non-empty");
        let (fs, fe) = ends(&shapes[first]).expect("open");
        let start_pt = if chain.len() > 1 {
            let second = chain[1];
            let (ss, se) = ends(&shapes[second]).expect("open");
            if close_enough(fs, ss, epsilon) || close_enough(fs, se, epsilon) {
                fe
            } else {
                fs
            }
        } else {
            fs
        };
        contour.pts.push(start_pt);
        let mut prev = start_pt;
        for &s in &chain {
            c.process_segment(s, &mut contour, &mut prev);
        }

        // Closure.
        let (c0, cl) = (contour.pts[0], *contour.pts.last().expect("a start point"));
        if close_enough(c0, cl, epsilon) {
            if c0 != cl && contour.pts.len() > 2 {
                match contour.last_arc {
                    Some((start_idx, mid)) => {
                        // The last item is an arc that ends near the first point: polygonize it again to end exactly there.
                        let arc_start = contour.pts[start_idx];
                        let owner = c.owners.get(&(contour.pts[contour.pts.len() - 2], cl)).copied();
                        let re = Arc::through(arc_start, mid, c0).polyline(max_error);
                        contour.pts.truncate(start_idx + 1);
                        for w in re.windows(2) {
                            if let Some(o) = owner {
                                c.owners.insert((w[0], w[1]), o);
                            }
                        }
                        contour.append_chain(&re, None);
                    }
                    None => {
                        let n = contour.pts.len();
                        let owner = c.owners.get(&(contour.pts[n - 2], cl)).copied();
                        contour.pts[n - 1] = c0;
                        if let Some(o) = owner {
                            c.owners.insert((contour.pts[n - 2], c0), o);
                        }
                    }
                }
            }
            contour.close();
        } else {
            c.report_gap(c0, &mut reported_gaps);
            c.report_gap(cl, &mut reported_gaps);
        }
        contours.push(contour);
    }

    // Every contour must be closed.
    if contours.iter().any(|ct| !ct.closed) {
        return Converted { polys, success: false, errors: c.errors };
    }

    let hierarchy = contour_hierarchy(&contours);
    let has_malformed_overlap = has_overlapping_contours(&contours);

    // `addOutlinesToPolygon`
    let mut contour_to_outline: HashMap<usize, usize> = HashMap::new();
    for (&ci, parents) in &hierarchy {
        if parents.len() % 2 == 0 {
            if !allow_disjoint && !polys.is_empty() {
                let a = polys.outline(0).first().zip(polys.outline(0).get(1)).and_then(|(p, q)| c.owners.get(&(Point { x: p.x, y: p.y }, Point { x: q.x, y: q.y })).copied());
                let b = contours[ci].pts.first().zip(contours[ci].pts.get(1)).and_then(|(p, q)| c.owners.get(&(*p, *q)).copied());
                if let (Some(a), Some(b)) = (a, b) {
                    c.report("(multiple board outlines not supported)", Some(a), Some(b), contours[ci].pts[0]);
                    return Converted { polys, success: false, errors: c.errors };
                }
            }
            polys.add_outline(to_line_chain(&contours[ci].pts));
            contour_to_outline.insert(ci, polys.outline_count() - 1);
        }
    }

    // `addHolesToPolygon`
    if !has_malformed_overlap {
        for (&ci, parents) in &hierarchy {
            if parents.len() % 2 == 1 {
                // A hole of the parent that has one parent fewer than this contour.
                for &parent in parents {
                    if hierarchy[&parent].len() == parents.len() - 1 {
                        if let Some(&oi) = contour_to_outline.get(&parent) {
                            polys.add_hole(to_line_chain(&contours[ci].pts), Some(oi));
                        }
                        break;
                    }
                }
            }
        }
    } else {
        // Malformed overlapping contours: cutouts are subtracted and islands added back, as clipped polygons.
        let mut cutouts = ShapePolySet::new();
        let mut islands = ShapePolySet::new();
        for (&ci, parents) in &hierarchy {
            if parents.is_empty() {
                continue;
            }
            if parents.len() % 2 == 1 {
                cutouts.add_outline(to_line_chain(&contours[ci].pts));
            } else {
                islands.add_outline(to_line_chain(&contours[ci].pts));
            }
        }
        if cutouts.outline_count() > 0 {
            cutouts.simplify();
            polys.boolean_subtract(&cutouts);
        }
        if islands.outline_count() > 0 {
            islands.simplify();
            polys.boolean_add(&islands);
        }
    }

    // `checkSelfIntersections`
    let success = check_self_intersections(&polys, &mut c);
    Converted { polys, success, errors: c.errors }
}

/// `checkSelfIntersections`: a side that coincides with another, or crosses one it does not just share an end with. KiCad stops
/// comparing a side with the ones after it as soon as the next one's first point is farther from the origin than this side's last, which
/// is not a safe bound for a sort by x then y, and which this port keeps, so that the verdict is KiCad's.
fn check_self_intersections(polys: &ShapePolySet, c: &mut Chainer) -> bool {
    let mut segments: Vec<Seg> = Vec::new();
    for i in 0..polys.outline_count() {
        let mut rings: Vec<&LineChain> = vec![polys.outline(i)];
        rings.extend((0..polys.hole_count(i)).map(|h| polys.hole(i, h)));
        for ring in rings {
            let n = ring.len();
            for k in 0..n {
                let (a, b) = (Point { x: ring[k].x, y: ring[k].y }, Point { x: ring[(k + 1) % n].x, y: ring[(k + 1) % n].y });
                // `LexicographicalCompare( A, B ) > 0`: swap so that A is the smaller.
                let (a, b) = if (a.x, a.y) > (b.x, b.y) { (b, a) } else { (a, b) };
                segments.push(Seg { a, b });
            }
        }
    }
    segments.sort_by_key(|s| (s.a.x, s.a.y, s.b.x, s.b.y));
    let norm2 = |p: Point| (p.x as i128) * (p.x as i128) + (p.y as i128) * (p.y as i128);
    let mut self_intersecting = false;
    for i in 0..segments.len() {
        let s1 = segments[i];
        for s2 in segments.iter().skip(i + 1) {
            if norm2(s2.a) > norm2(s1.b) {
                break;
            }
            let same = (s1.a == s2.a && s1.b == s2.b) || (s1.a == s2.b && s1.b == s2.a);
            if same {
                let (a, b) = (c.owners.get(&(s1.a, s1.b)).copied(), c.owners.get(&(s2.a, s2.b)).copied());
                c.report("(self-intersecting)", a, b, s1.a);
                self_intersecting = true;
            } else if let Some(pt) = seg_intersect(s1, *s2, true) {
                let (a, b) = (c.owners.get(&(s1.a, s1.b)).copied(), c.owners.get(&(s2.a, s2.b)).copied());
                c.report("(self-intersecting)", a, b, pt);
                self_intersecting = true;
            }
        }
    }
    !self_intersecting
}

/// `ConvertOutlineToPolygon( aShapeList, aPolygons, aErrorMax, aChainingEpsilon, aAllowDisjoint, aErrorHandler )`: the contours the shapes
/// chain into as outlines with holes, `true` when every contour closed and none crosses itself, and every error the chaining found.
pub fn convert_outline_to_polygon(shapes: &[Shape], max_error: f64, chaining_epsilon: Um, allow_disjoint: bool) -> (ShapePolySet, bool, Vec<OutlineError>) {
    let r = convert(shapes, max_error, chaining_epsilon, allow_disjoint);
    (r.polys, r.success, r.errors)
}

/// `BuildBoardPolygonOutlines( aBoard, aOutlines, aErrorMax, aChainingEpsilon, aInferOutlineIfNecessary )` over the board's Edge.Cuts
/// items: [`convert_outline_to_polygon`] with disjoint outlines allowed, and, when `infer` and the edges do not make a closed outline,
/// the rectangle round them. (The footprint pass is the importer's: see the module doc.)
pub fn build_board_polygon_outlines(shapes: &[Shape], max_error: f64, chaining_epsilon: Um, infer: bool) -> BoardOutline {
    let mut out = BoardOutline::default();
    if !shapes.is_empty() {
        let r = convert(shapes, max_error, chaining_epsilon, true);
        out.polys = r.polys;
        out.valid = r.success;
        out.errors = r.errors;
    }
    if (!out.valid || out.polys.outline_count() == 0) && infer {
        if let Some((lo, hi)) = edge::bounding_box(shapes, max_error) {
            // A rectangle of no area is given a minimal size: one millimetre.
            let (lo, hi) = if hi.x == lo.x || hi.y == lo.y { (Point { x: lo.x - 1_000, y: lo.y - 1_000 }, Point { x: hi.x + 1_000, y: hi.y + 1_000 }) } else { (lo, hi) };
            out.polys.remove_all_contours();
            out.polys.add_outline(to_line_chain(&[lo, Point { x: lo.x, y: hi.y }, hi, Point { x: hi.x, y: lo.y }]));
            out.inferred = true;
        }
    }
    out
}

/// `BOARD::GetBoardPolygonOutlines( aOutlines, aInferOutlineIfNecessary )` for a design: the outline of its Edge.Cuts at the board's
/// default tolerances, made strictly simple.
pub fn board_outline(design: &Design, infer: bool) -> BoardOutline {
    // A board whose outline is a plain polygon of straight edges and nothing else is that polygon: the common case, and the one the
    // placer asks about thousands of times.
    if !edge::outline_is_shapes(design) && !edge::has_edge_cuts_shapes(design) {
        let ring: &[Point] = design.placement.as_ref().map_or(&[], |p| p.outline.as_slice());
        if ring.len() >= 3 {
            return BoardOutline { polys: ShapePolySet::from_outline(to_line_chain(ring)), valid: true, inferred: false, errors: Vec::new() };
        }
    }
    board_outline_of(&edge::edge_cuts_shapes(design), infer)
}

/// [`board_outline`] of a list of Edge.Cuts items.
pub fn board_outline_of(shapes: &[Shape], infer: bool) -> BoardOutline {
    let mut out = build_board_polygon_outlines(shapes, MAX_ERROR_UM as f64, CHAINING_EPSILON_UM, infer);
    // `aOutlines.Simplify()`: "make polygon strictly simple to avoid issues (especially in 3D viewer)".
    out.polys.simplify();
    out
}

/// Keep `placement.outline` a summary of the Edge.Cuts shapes when they are the outline ([`DrawingsSection::outline_is_shapes`]): the
/// largest outline as a ring, or nothing when there is none. A design whose outline is its polygon is left alone.
///
/// [`DrawingsSection::outline_is_shapes`]: eda_model::ir::DrawingsSection::outline_is_shapes
pub fn refresh_outline_summary(design: &mut Design) {
    if !edge::outline_is_shapes(design) {
        return;
    }
    let ring = board_outline(design, true).main_ring();
    if let Some(pl) = design.placement.as_mut() {
        if pl.outline != ring {
            pl.outline = ring;
        }
    }
}

// ----------------------------------------------------------------------------------------------------------------------
// DRC_TEST_PROVIDER_MISC::testOutline
// ----------------------------------------------------------------------------------------------------------------------

/// `TestBoardOutlinesGraphicItems`: shapes too small to chain (a few nanometres of line), and closed contours that touch or cross each
/// other. `min_dist` is the smallest size a shape may have, micrometres.
pub fn test_board_outlines_graphic_items(shapes: &[Shape], min_dist: f64, max_error: f64, chaining_epsilon: Um) -> Vec<OutlineError> {
    let mut errors = Vec::new();
    let min_dist = min_dist.max(0.0);
    let nm = |um: f64| (um * 1000.0) as i64;
    for s in shapes {
        let id = Some(s.id().to_string());
        let (message, at) = match s {
            Shape::Rect { start, end, .. } => {
                let dim = norm(*start, *end);
                if dim <= min_dist {
                    (format!("(rectangle has null or very small size: {} nm)", nm(dim)), *start)
                } else {
                    continue;
                }
            }
            Shape::Circle { center, end, .. } => {
                let r = edge::circle_radius(*center, *end) as f64;
                if r <= min_dist {
                    (format!("(circle has null or very small radius: {} nm)", nm(r)), *center)
                } else {
                    continue;
                }
            }
            Shape::Segment { start, end, .. } => {
                let dim = norm(*start, *end);
                if dim <= min_dist {
                    (format!("(segment has null or very small length: {} nm)", nm(dim)), *start)
                } else {
                    continue;
                }
            }
            Shape::Arc { start, mid, end, .. } => {
                // The size of an arc is the length of its two halves' chords: no need for more.
                let dim = norm(*start, *mid) + norm(*mid, *end);
                if dim <= min_dist {
                    (format!("(arc has null or very small size: {} nm)", nm(dim)), *start)
                } else {
                    continue;
                }
            }
            Shape::Polygon { .. } | Shape::Bezier { .. } => continue,
        };
        errors.push(OutlineError { message, item_a: id, item_b: None, at });
    }

    // Closed contours: every closed shape, and the loops the open ones chain into.
    let mut closed: Vec<(usize, Vec<Point>)> = Vec::new();
    for (i, s) in shapes.iter().enumerate() {
        if is_closed_shape(s) {
            let mut c = Chainer { shapes, max_error, epsilon: chaining_epsilon, index: Endpoints::new(shapes, chaining_epsilon), owners: BTreeMap::new(), errors: Vec::new() };
            let mut contour = Contour::default();
            c.process_closed(i, &mut contour);
            closed.push((i, contour.pts));
        }
    }
    let open: Vec<usize> = (0..shapes.len()).filter(|&i| !is_closed_shape(&shapes[i])).collect();
    if !open.is_empty() {
        for (owner, ring) in chained_loops(shapes, &open, max_error, chaining_epsilon) {
            closed.push((owner, ring));
        }
    }
    for ii in 0..closed.len() {
        for jj in ii + 1..closed.len() {
            let Some(first) = rings_intersect(&closed[ii].1, &closed[jj].1) else { continue };
            let (a, b) = (&shapes[closed[ii].0], &shapes[closed[jj].0]);
            // The middle of the boxes' overlap when they overlap with some area, else the first crossing.
            let at = box_overlap_centre(a, b, max_error).unwrap_or(first);
            errors.push(OutlineError { message: "(self-intersecting)".to_string(), item_a: Some(a.id().to_string()), item_b: Some(b.id().to_string()), at });
        }
    }
    errors
}

fn shape_box(s: &Shape, max_error: f64) -> Option<(Point, Point)> {
    edge::bounding_box(std::slice::from_ref(s), max_error)
}

fn box_overlap_centre(a: &Shape, b: &Shape, max_error: f64) -> Option<Point> {
    let (ba, bb) = (shape_box(a, max_error)?, shape_box(b, max_error)?);
    let (lo, hi) = (Point { x: ba.0.x.max(bb.0.x), y: ba.0.y.max(bb.0.y) }, Point { x: ba.1.x.min(bb.1.x), y: ba.1.y.min(bb.1.y) });
    (hi.x > lo.x && hi.y > lo.y).then_some(Point { x: (lo.x + hi.x) / 2, y: (lo.y + hi.y) / 2 })
}

/// `buildChainedClosedContour`, over all of `open`: the closed loops that segments, arcs and curves chain into, each with the first shape
/// of its chain (the owner an error is reported against). Chains that do not close are not loops and are left out.
fn chained_loops(shapes: &[Shape], open: &[usize], max_error: f64, epsilon: Um) -> Vec<(usize, Vec<Point>)> {
    let subset: Vec<Shape> = open.iter().map(|&i| shapes[i].clone()).collect();
    let mut c = Chainer { shapes: &subset, max_error, epsilon, index: Endpoints::new(&subset, epsilon), owners: BTreeMap::new(), errors: Vec::new() };
    let mut remaining: Vec<bool> = vec![true; subset.len()];
    let mut out = Vec::new();
    for start in 0..subset.len() {
        if !remaining[start] {
            continue;
        }
        let (fs, fe) = ends(&subset[start]).expect("open");
        let mut chain: VecDeque<usize> = VecDeque::from([start]);
        let (mut front_pt, mut back_pt) = (fs, fe);
        let mut visited: HashSet<usize> = HashSet::from([start]);
        let mut closed = false;
        for forward in [true, false] {
            if !forward && closed {
                break;
            }
            let mut curr = if forward { *chain.back().expect("non-empty") } else { *chain.front().expect("non-empty") };
            let mut prev = if forward { back_pt } else { front_pt };
            loop {
                // The index spans every open shape, including the ones an earlier loop used.
                let next = c.find_next(curr, prev).filter(|&n| remaining[n]);
                if let Some(nx) = next.filter(|n| !visited.contains(n)) {
                    visited.insert(nx);
                    if forward {
                        chain.push_back(nx);
                    } else {
                        chain.push_front(nx);
                    }
                    let (s, e) = ends(&subset[nx]).expect("open");
                    prev = if closer_to_first(prev, s, e) { e } else { s };
                    curr = nx;
                    continue;
                }
                if let Some(nx) = next {
                    let chain_end = if forward { *chain.front().expect("non-empty") } else { *chain.back().expect("non-empty") };
                    let chain_pt = if forward { front_pt } else { back_pt };
                    if nx == chain_end && close_enough(prev, chain_pt, epsilon) {
                        closed = true;
                    }
                }
                if forward {
                    back_pt = prev;
                } else {
                    front_pt = prev;
                }
                break;
            }
        }
        if !closed {
            remaining[start] = false;
            continue;
        }
        let first = *chain.front().expect("non-empty");
        let (fs, fe) = ends(&subset[first]).expect("open");
        let start_pt = if chain.len() > 1 {
            let (ss, se) = ends(&subset[chain[1]]).expect("open");
            if close_enough(fs, ss, epsilon) || close_enough(fs, se, epsilon) {
                fe
            } else {
                fs
            }
        } else {
            fs
        };
        let mut contour = Contour::default();
        contour.pts.push(start_pt);
        let mut prev = start_pt;
        for &s in &chain {
            c.process_segment(s, &mut contour, &mut prev);
        }
        if contour.pts.len() < 3 {
            for s in &chain {
                remaining[*s] = false;
            }
            continue;
        }
        if contour.pts[0] != *contour.pts.last().expect("points") {
            let n = contour.pts.len();
            contour.pts[n - 1] = contour.pts[0];
        }
        contour.close();
        for s in &chain {
            remaining[*s] = false;
        }
        out.push((open[first], contour.pts));
    }
    out
}

/// `DRC_TEST_PROVIDER_MISC::testOutline` for a design: the suspicious items and the outline that does not chain, every one a place where
/// `kicad-cli pcb drc` reports `invalid_outline` ("Board has malformed outline") on the exported file. Empty when the outline is
/// well-formed, and for a board that has none (`kicad-cli` has its own "(no edges found on Edge.Cuts layer)" for that).
pub fn check_board_outline(design: &Design) -> Vec<OutlineError> {
    check_edge_cuts(&edge::edge_cuts_shapes(design))
}

/// [`check_board_outline`] of a list of Edge.Cuts items.
pub fn check_edge_cuts(shapes: &[Shape]) -> Vec<OutlineError> {
    if shapes.is_empty() {
        return Vec::new();
    }
    // `minSizeForValideGraphics`: 0.001 mm.
    let mut errors = test_board_outlines_graphic_items(shapes, 1.0, MAX_ERROR_UM as f64, CHAINING_EPSILON_UM);
    // `maxError`: "not critical here: we do not use the outline shape" -- 0.05 mm.
    let built = build_board_polygon_outlines(shapes, 50.0, CHAINING_EPSILON_UM, true);
    errors.extend(built.errors);
    if !built.valid && errors.is_empty() {
        // Not a single call of the handler, yet the outline is not valid: `testOutline`'s own wording.
        errors.push(OutlineError { message: "(no edges found on Edge.Cuts layer)".to_string(), item_a: None, item_b: None, at: Point::default() });
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::outline::EDGE_CUTS;

    fn p(x: i64, y: i64) -> Point {
        Point { x, y }
    }

    fn line(id: &str, a: Point, b: Point) -> Shape {
        Shape::Segment { id: id.into(), layer: EDGE_CUTS.into(), stroke_width: 50, filled: false, start: a, end: b }
    }

    fn arc(id: &str, a: Point, m: Point, b: Point) -> Shape {
        Shape::Arc { id: id.into(), layer: EDGE_CUTS.into(), stroke_width: 50, filled: false, start: a, mid: m, end: b }
    }

    fn circle(id: &str, c: Point, rim: Point) -> Shape {
        Shape::Circle { id: id.into(), layer: EDGE_CUTS.into(), stroke_width: 50, filled: false, center: c, end: rim }
    }

    fn rect(id: &str, a: Point, b: Point) -> Shape {
        Shape::Rect { id: id.into(), layer: EDGE_CUTS.into(), stroke_width: 50, filled: false, start: a, end: b }
    }

    /// 40 x 30 mm with 5 mm rounded corners: four lines and four quarter arcs, in file order, not chained order.
    fn rounded_rectangle() -> Vec<Shape> {
        let r = 5_000;
        let (w, h) = (40_000, 30_000);
        let k = (r as f64 * (1.0 - std::f64::consts::FRAC_1_SQRT_2)).round() as i64;
        vec![
            line("top", p(r, 0), p(w - r, 0)),
            arc("tr", p(w - r, 0), p(w - k, k), p(w, r)),
            line("right", p(w, r), p(w, h - r)),
            arc("br", p(w, h - r), p(w - k, h - k), p(w - r, h)),
            line("bottom", p(w - r, h), p(r, h)),
            arc("bl", p(r, h), p(k, h - k), p(0, h - r)),
            line("left", p(0, h - r), p(0, r)),
            arc("tl", p(0, r), p(k, k), p(r, 0)),
        ]
    }

    #[test]
    fn an_outline_of_lines_and_arcs_is_one_closed_contour_with_the_arcs_kept_round() {
        let out = board_outline_of(&rounded_rectangle(), false);
        assert!(out.valid && out.errors.is_empty(), "{:?}", out.errors);
        assert_eq!(out.polys.outline_count(), 1);
        assert!(!out.polys.has_holes());
        // 40 x 30 minus the four corners' (1 - pi/4) r^2 is 1200 - 21.46 mm^2, to the polygon's 5 um.
        let want = 40_000.0 * 30_000.0 - 4.0 * (1.0 - std::f64::consts::FRAC_PI_4) * 5_000.0 * 5_000.0;
        let got = out.polys.area();
        assert!((got - want).abs() / want < 5e-4, "area {got} against {want}");
        // And the corners really are curved: far more points than the eight a chord outline has.
        assert!(out.polys.outline(0).len() > 60, "{} points", out.polys.outline(0).len());
    }

    #[test]
    fn the_order_and_direction_of_the_items_do_not_matter() {
        let mut shuffled = rounded_rectangle();
        shuffled.reverse();
        // Turn a few around, too.
        if let Shape::Segment { start, end, .. } = &mut shuffled[1] {
            std::mem::swap(start, end);
        }
        if let Shape::Arc { start, end, .. } = &mut shuffled[2] {
            std::mem::swap(start, end);
        }
        let a = board_outline_of(&rounded_rectangle(), false);
        let b = board_outline_of(&shuffled, false);
        assert!(b.valid && b.errors.is_empty(), "{:?}", b.errors);
        assert!((a.polys.area() - b.polys.area()).abs() < 1.0);
    }

    #[test]
    fn a_circle_inside_the_outline_is_a_cutout() {
        let mut items = vec![rect("board", p(0, 0), p(40_000, 30_000))];
        items.push(circle("hole", p(20_000, 15_000), p(23_200, 15_000)));
        let out = board_outline_of(&items, false);
        assert!(out.valid && out.errors.is_empty(), "{:?}", out.errors);
        assert_eq!((out.polys.outline_count(), out.polys.hole_count(0)), (1, 1));
        let want = 40_000.0 * 30_000.0 - std::f64::consts::PI * 3_200.0 * 3_200.0;
        assert!((out.polys.area() - want).abs() / want < 1e-3, "{}", out.polys.area());
        assert!(!out.contains(p(20_000, 15_000)), "the middle of the cutout is off the board");
        assert!(out.contains(p(10_000, 10_000)));
        assert!(out.contains(p(0, 0)), "an edge point is on the board");
    }

    #[test]
    fn a_slot_made_of_two_arcs_and_two_lines_is_a_cutout_too() {
        // A 10 x 3 mm stadium slot inside a 40 x 30 mm board.
        let (cx, cy, r, half) = (20_000, 15_000, 1_500, 3_500);
        let mut items = vec![rect("board", p(0, 0), p(40_000, 30_000))];
        items.extend([
            line("slot_t", p(cx - half, cy - r), p(cx + half, cy - r)),
            arc("slot_r", p(cx + half, cy - r), p(cx + half + r, cy), p(cx + half, cy + r)),
            line("slot_b", p(cx + half, cy + r), p(cx - half, cy + r)),
            arc("slot_l", p(cx - half, cy + r), p(cx - half - r, cy), p(cx - half, cy - r)),
        ]);
        let out = board_outline_of(&items, false);
        assert!(out.valid && out.errors.is_empty(), "{:?}", out.errors);
        assert_eq!((out.polys.outline_count(), out.polys.hole_count(0)), (1, 1));
        assert!(!out.contains(p(cx, cy)));
    }

    #[test]
    fn two_separate_outlines_are_allowed() {
        let items = vec![rect("left", p(0, 0), p(10_000, 10_000)), rect("right", p(20_000, 0), p(30_000, 10_000))];
        let out = board_outline_of(&items, false);
        assert!(out.valid && out.errors.is_empty(), "{:?}", out.errors);
        assert_eq!(out.polys.outline_count(), 2);
        assert!(out.contains(p(5_000, 5_000)) && out.contains(p(25_000, 5_000)) && !out.contains(p(15_000, 5_000)));
        // The main ring is the larger one; here they are equal, so either.
        assert_eq!(out.main_ring().len(), 4);
    }

    #[test]
    fn two_outlines_are_refused_where_disjoint_ones_are_not_allowed() {
        // The footprint pass of `BuildBoardPolygonOutlines` and any caller of `ConvertOutlineToPolygon` that wants one board.
        let items = vec![rect("left", p(0, 0), p(10_000, 10_000)), rect("right", p(20_000, 0), p(30_000, 10_000))];
        let (_, ok, errors) = convert_outline_to_polygon(&items, 5.0, 10, false);
        assert!(!ok);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "(multiple board outlines not supported)");
    }

    #[test]
    fn an_island_inside_a_cutout_is_an_outline_again() {
        // A board, a hole in it, and a post in the hole.
        let items = vec![rect("board", p(0, 0), p(40_000, 40_000)), rect("hole", p(10_000, 10_000), p(30_000, 30_000)), rect("post", p(15_000, 15_000), p(25_000, 25_000))];
        let out = board_outline_of(&items, false);
        assert!(out.valid, "{:?}", out.errors);
        assert_eq!(out.polys.outline_count(), 2);
        assert!(out.contains(p(20_000, 20_000)) && !out.contains(p(12_000, 12_000)) && out.contains(p(5_000, 5_000)));
    }

    #[test]
    fn an_open_outline_is_reported_where_it_is_open_and_not_closed_for_the_caller() {
        // A square whose bottom side has a 2 mm piece missing.
        let items = vec![
            line("bottom_left", p(0, 0), p(4_000, 0)),
            line("bottom_right", p(6_000, 0), p(10_000, 0)),
            line("right", p(10_000, 0), p(10_000, 10_000)),
            line("top", p(10_000, 10_000), p(0, 10_000)),
            line("left", p(0, 10_000), p(0, 0)),
        ];
        let out = board_outline_of(&items, false);
        assert!(!out.valid);
        assert_eq!(out.polys.outline_count(), 0, "nothing is made of an open chain");
        let gaps: Vec<_> = out.errors.iter().filter(|e| e.message == "(not a closed shape)").collect();
        assert_eq!(gaps.len(), 1, "both ends of the chain name the same two items: {:?}", out.errors);
        let g = gaps[0];
        let mut named = [g.item_a.clone().unwrap_or_default(), g.item_b.clone().unwrap_or_default()];
        named.sort();
        assert_eq!(named, ["bottom_left".to_string(), "bottom_right".to_string()], "the two pieces either side of the gap");
        assert_eq!(g.at, p(5_000, 0), "the middle of the gap");
        // `aInferOutlineIfNecessary`: KiCad then uses the rectangle round the edges.
        let inferred = board_outline_of(&items, true);
        assert!(!inferred.valid && inferred.inferred);
        assert_eq!(inferred.polys.outline_count(), 1);
        // 10 x 10 mm and half a line width (25 um) round it.
        assert_eq!(inferred.polys.area(), 10_050.0 * 10_050.0);
    }

    #[test]
    fn a_u_with_its_top_missing_is_open() {
        let items = vec![line("a", p(0, 0), p(0, 12_000)), line("b", p(0, 12_000), p(10_000, 12_000)), line("c", p(10_000, 12_000), p(10_000, 0))];
        let out = board_outline_of(&items, false);
        assert!(!out.valid && out.polys.outline_count() == 0);
        assert!(out.errors.iter().any(|e| e.message == "(not a closed shape)"), "{:?}", out.errors);
    }

    #[test]
    fn a_gap_wider_than_the_chaining_epsilon_is_open_and_a_narrower_one_is_not() {
        let board = |gap: i64| vec![line("a", p(0, 0), p(10_000, 0)), line("b", p(10_000, gap), p(10_000, 10_000)), line("c", p(10_000, 10_000), p(0, 10_000)), line("d", p(0, 10_000), p(0, 0))];
        assert!(board_outline_of(&board(9), false).valid, "9 um is under the 10 um epsilon");
        assert!(!board_outline_of(&board(11), false).valid, "11 um is not");
    }

    #[test]
    fn a_bow_tie_is_self_intersecting() {
        // Chains (0,0) -> (10,10) -> (10,0) -> (0,10) -> (0,0): the 1st and 3rd sides cross.
        let items = vec![
            line("a", p(0, 0), p(10_000, 10_000)),
            line("b", p(10_000, 10_000), p(10_000, 0)),
            line("c", p(10_000, 0), p(0, 10_000)),
            line("d", p(0, 10_000), p(0, 0)),
        ];
        let out = board_outline_of(&items, false);
        assert!(out.errors.iter().any(|e| e.message == "(self-intersecting)"), "{:?}", out.errors);
    }

    #[test]
    fn a_tee_of_three_lines_is_reported_where_the_chain_runs_into_a_used_item() {
        // A square with a stub off one corner: the chain reaches a used shape that is not the one it started from.
        let items = vec![
            line("a", p(0, 0), p(10_000, 0)),
            line("b", p(10_000, 0), p(10_000, 10_000)),
            line("c", p(10_000, 10_000), p(0, 10_000)),
            line("d", p(0, 10_000), p(0, 0)),
            line("stub", p(0, 0), p(-3_000, -3_000)),
        ];
        let out = board_outline_of(&items, false);
        assert!(!out.errors.is_empty(), "a stub off a corner is not a clean outline");
    }

    #[test]
    fn a_circle_that_touches_the_outline_is_a_malformed_overlap_that_still_cuts_a_hole() {
        // The hole's rim is a polygon vertex on the board edge: contours that touch take the boolean route.
        let items = vec![rect("board", p(0, 0), p(40_000, 30_000)), circle("notch", p(0, 15_000), p(3_000, 15_000))];
        let out = board_outline_of(&items, false);
        assert!(out.valid, "{:?}", out.errors);
        // Half the circle lies outside the board, so only the half inside is cut.
        let cut = 40_000.0 * 30_000.0 - out.polys.area();
        assert!(cut > 0.0, "the circle's inside is taken out");
        assert!(!out.contains(p(1_000, 15_000)) && out.contains(p(10_000, 15_000)));
    }

    #[test]
    fn the_checks_of_the_drc_provider_find_what_chaining_does_not() {
        // A circle crossing the board edge: both are closed, valid contours and they overlap.
        let items = vec![rect("board", p(0, 0), p(40_000, 30_000)), circle("hole", p(0, 15_000), p(5_000, 15_000))];
        let errors = check_edge_cuts(&items);
        assert!(errors.iter().any(|e| e.message == "(self-intersecting)" && e.item_a.as_deref() == Some("board") && e.item_b.as_deref() == Some("hole")), "{errors:?}");
        // A clean outline reports nothing, and a 1 nm line is a suspicious item.
        assert!(check_edge_cuts(&rounded_rectangle()).is_empty());
        let mut tiny = rounded_rectangle();
        tiny.push(line("dot", p(5, 5), p(5, 5)));
        let errors = check_edge_cuts(&tiny);
        assert!(errors.iter().any(|e| e.message.starts_with("(segment has null or very small length") && e.item_a.as_deref() == Some("dot")), "{errors:?}");
    }

    #[test]
    fn a_polygon_is_a_contour_by_itself_and_so_is_a_curve_chain() {
        let poly = Shape::Polygon { id: "poly".into(), layer: EDGE_CUTS.into(), stroke_width: 50, filled: false, pts: vec![p(0, 0), p(20_000, 0), p(20_000, 20_000), p(0, 20_000)] };
        let out = board_outline_of(&[poly], false);
        assert!(out.valid);
        assert_eq!(out.polys.area(), 400_000_000.0);
        // Two Bezier halves of a lens, end to end.
        let lens = vec![
            Shape::Bezier { id: "up".into(), layer: EDGE_CUTS.into(), stroke_width: 50, filled: false, start: p(0, 0), c1: p(0, -8_000), c2: p(20_000, -8_000), end: p(20_000, 0) },
            Shape::Bezier { id: "dn".into(), layer: EDGE_CUTS.into(), stroke_width: 50, filled: false, start: p(20_000, 0), c1: p(20_000, 8_000), c2: p(0, 8_000), end: p(0, 0) },
        ];
        let out = board_outline_of(&lens, false);
        assert!(out.valid && out.errors.is_empty(), "{:?}", out.errors);
        assert!(out.polys.area() > 100_000_000.0, "{}", out.polys.area());
    }

    #[test]
    fn a_design_with_a_plain_polygon_outline_is_that_polygon() {
        let ring = vec![p(0, 0), p(10_000, 0), p(10_000, 5_000), p(0, 5_000)];
        let d = design_with(ring.clone(), vec![], false);
        let out = board_outline(&d, true);
        assert!(out.valid);
        assert_eq!(out.main_ring(), ring);
    }

    fn design_with(outline: Vec<Point>, shapes: Vec<Shape>, is_shapes: bool) -> Design {
        Design {
            schema: 1,
            provenance: eda_model::ir::Provenance { engine_version: "test".into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(eda_model::ir::PlacementSection { outline, footprints: vec![], modules: vec![] }),
            routing: None,
            drawings: Some(eda_model::ir::DrawingsSection { shapes, outline_is_shapes: is_shapes, ..Default::default() }),
            footprint_library: None,
            symbol_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
        }
    }

    #[test]
    fn a_circle_drawn_on_a_polygon_board_is_a_cutout_and_a_shapes_outline_replaces_the_polygon() {
        let ring = vec![p(0, 0), p(40_000, 0), p(40_000, 30_000), p(0, 30_000)];
        let hole = circle("hole", p(20_000, 15_000), p(22_000, 15_000));
        // The polygon stays an item: the circle is a hole in it.
        let d = design_with(ring.clone(), vec![hole.clone()], false);
        let out = board_outline(&d, true);
        assert!(out.valid, "{:?}", out.errors);
        assert_eq!((out.polys.outline_count(), out.polys.hole_count(0)), (1, 1));
        // Shapes are the outline: the polygon is only their summary and is not an item.
        let mut d = design_with(ring, rounded_rectangle(), true);
        d.drawings.as_mut().expect("drawings").shapes.push(hole);
        let out = board_outline(&d, true);
        assert!(out.valid, "{:?}", out.errors);
        assert_eq!((out.polys.outline_count(), out.polys.hole_count(0)), (1, 1));
        refresh_outline_summary(&mut d);
        let summary = &d.placement.as_ref().expect("placement").outline;
        assert!(summary.len() > 60 && summary.iter().all(|q| (0..=40_000).contains(&q.x) && (0..=30_000).contains(&q.y)), "{} points", summary.len());
    }
}
