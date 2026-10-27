//! The ear cutting of `POLYGON_TRIANGULATION::TesselatePolygon( const SHAPE_LINE_CHAIN& )` (`libs/kimath/include/geometry/polygon_triangulation.h`,
//! with `VERTEX_SET` / `VERTEX` of `vertex_set.{h,cpp}`), for the router's rule areas: `PNS_KICAD_IFACE_BASE::syncZone` adds a keepout area to
//! the world as one `SOLID` per triangle of `aZone->Outline()->CacheTriangulation()`, so that each obstacle is convex and its hull is exact.
//!
//! Ported: `createList` (the winding, and points nearer than `TRIANGULATESIMPLIFICATIONLEVEL` (50 nm) to the last one dropped),
//! `simplifyList`, `updateList`'s removal of repeated points, and `earcutList`'s first pass -- the two-vertex look-ahead that takes the ear with
//! the best `earScore`, `isTooSmall` (`TRIANGULATEMINIMUMAREA`, 1000), and the merge of a local self-intersection. `isEar`'s z-order walk
//! visits the vertices inside the triangle's box in Morton order; a scan of every vertex of the ring finds the same ones, so the answer is
//! the same. **Not ported:** `subdividePolygon`, `removeNullTriangles` and `splitPolygon`, which `earcutList` turns to when no ear is left
//! (a self-intersecting or degenerate outline): [`tesselate`] says it could not finish instead, and the caller keeps the ring as it is.
//! Holes (`TesselatePolygon( const POLYGON& )`'s `eliminateHoles`) are not ported either.

use eda_model::ir::Point;

/// `TRIANGULATESIMPLIFICATIONLEVEL` (`ADVANCED_CFG::m_TriangulateSimplificationLevel`, 50).
const SIMPLIFICATION_LEVEL: f64 = 50.0;
/// `TRIANGULATEMINIMUMAREA` (`ADVANCED_CFG::m_TriangulateMinimumArea`, 1000).
const MINIMUM_AREA: f64 = 1000.0;

#[derive(Clone, Copy)]
struct Vertex {
    /// Index of the point in the chain (`VERTEX::i`).
    i: usize,
    x: f64,
    y: f64,
    prev: usize,
    next: usize,
}

struct Ring {
    v: Vec<Vertex>,
}

impl Ring {
    /// `VERTEX_SET::insertVertex`: a new vertex after `last` (or a ring of its own).
    fn insert(&mut self, i: usize, p: Point, last: Option<usize>) -> usize {
        let id = self.v.len();
        let (x, y) = (p.x.as_f64(), p.y.as_f64());
        match last {
            None => self.v.push(Vertex { i, x, y, prev: id, next: id }),
            Some(l) => {
                let n = self.v[l].next;
                self.v.push(Vertex { i, x, y, prev: l, next: n });
                self.v[n].prev = id;
                self.v[l].next = id;
            }
        }
        id
    }

    /// `VERTEX::remove`.
    fn remove(&mut self, k: usize) {
        let (p, n) = (self.v[k].prev, self.v[k].next);
        self.v[n].prev = p;
        self.v[p].next = n;
    }

    fn same(&self, a: usize, b: usize) -> bool {
        self.v[a].x == self.v[b].x && self.v[a].y == self.v[b].y
    }

    /// `VERTEX_SET::area`: twice the signed area of the triangle `p`, `q`, `r`.
    fn area(&self, p: usize, q: usize, r: usize) -> f64 {
        let (p, q, r) = (self.v[p], self.v[q], self.v[r]);
        (q.y - p.y) * (r.x - q.x) - (q.x - p.x) * (r.y - q.y)
    }

    /// `VERTEX::area()`: the signed area of the ring through `k`.
    fn ring_area(&self, k: usize) -> f64 {
        let mut a = 0.0;
        let mut p = k;
        loop {
            let n = self.v[p].next;
            a += (self.v[p].x + self.v[n].x) * (self.v[n].y - self.v[p].y);
            p = n;
            if p == k {
                break;
            }
        }
        a / 2.0
    }

    /// `VERTEX::inTriangle`: is `k` inside (or on) the triangle `a`, `b`, `c`?
    fn in_triangle(&self, k: usize, a: usize, b: usize, c: usize) -> bool {
        let (p, a, b, c) = (self.v[k], self.v[a], self.v[b], self.v[c]);
        (c.x - p.x) * (a.y - p.y) - (a.x - p.x) * (c.y - p.y) >= 0.0 && (a.x - p.x) * (b.y - p.y) - (b.x - p.x) * (a.y - p.y) >= 0.0 && (b.x - p.x) * (c.y - p.y) - (c.x - p.x) * (b.y - p.y) >= 0.0
    }

    /// `VERTEX::isEar`: `b` is convex and no reflex-or-flat vertex of the ring is inside its triangle.
    fn is_ear(&self, b: usize) -> bool {
        let (a, c) = (self.v[b].prev, self.v[b].next);
        if self.area(a, b, c) >= 0.0 {
            return false;
        }
        let mut p = self.v[c].next;
        while p != a {
            if self.in_triangle(p, a, b, c) && self.area(self.v[p].prev, p, self.v[p].next) >= 0.0 {
                return false;
            }
            p = self.v[p].next;
        }
        true
    }

    /// `POLYGON_TRIANGULATION::isTooSmall`.
    fn is_too_small(&self, k: usize) -> bool {
        let (p, a, n) = (self.v[self.v[k].prev], self.v[k], self.v[self.v[k].next]);
        let sq = |u: Vertex, w: Vertex| (u.x - w.x) * (u.x - w.x) + (u.y - w.y) * (u.y - w.y);
        sq(p, a) < MINIMUM_AREA || sq(n, a) < MINIMUM_AREA || sq(n, p) < MINIMUM_AREA
    }

    /// `POLYGON_TRIANGULATION::earScore`.
    fn ear_score(&self, a: usize, b: usize, c: usize) -> f64 {
        let (va, vb, vc) = (self.v[a], self.v[b], self.v[c]);
        let sq = |u: Vertex, w: Vertex| (u.x - w.x) * (u.x - w.x) + (u.y - w.y) * (u.y - w.y);
        let norm = sq(va, vb) + sq(vb, vc) + sq(vc, va);
        if norm <= 0.0 {
            return 0.0;
        }
        self.area(a, b, c).abs() / norm
    }

    /// `POLYGON_TRIANGULATION::intersects`, ends included.
    fn intersects(&self, p1: usize, q1: usize, p2: usize, q2: usize) -> bool {
        let sign = |v: f64| (v > 0.0) as i32 - (v < 0.0) as i32;
        let overlapping = |p: usize, q: usize, r: usize| {
            let (p, q, r) = (self.v[p], self.v[q], self.v[r]);
            q.x <= p.x.max(r.x) && q.x >= p.x.min(r.x) && q.y <= p.y.max(r.y) && q.y >= p.y.min(r.y)
        };
        let (s1, s2, s3, s4) = (sign(self.area(p1, q1, p2)), sign(self.area(p1, q1, q2)), sign(self.area(p2, q2, p1)), sign(self.area(p2, q2, q1)));
        (s1 != s2 && s3 != s4) || (s1 == 0 && overlapping(p1, p2, q1)) || (s2 == 0 && overlapping(p1, q2, q1)) || (s3 == 0 && overlapping(p2, p1, q2)) || (s4 == 0 && overlapping(p2, q1, q2))
    }

    /// `VERTEX_SET::locallyInside` (for a ring with no fracture, whose next and previous outline vertices are its neighbours).
    fn locally_inside(&self, a: usize, b: usize) -> bool {
        let (an, ap) = (self.v[a].next, self.v[a].prev);
        if self.area(ap, a, an) < 0.0 {
            self.area(a, b, an) >= 0.0 && self.area(a, ap, b) >= 0.0
        } else {
            self.area(a, b, ap) < 0.0 || self.area(a, an, b) < 0.0
        }
    }
}

/// `TesselatePolygon( aPoly )` of the closed chain `pts`: its triangles, each as three points of `pts`. `None` when the ear cutting does not
/// finish (see the module's doc); an empty list for a chain with no area (`!m_bbox.GetWidth() || !m_bbox.GetHeight()`, or under three points).
pub fn tesselate(pts: &[Point]) -> Option<Vec<[Point; 3]>> {
    let n = pts.len();
    if n == 0 {
        return Some(Vec::new());
    }
    let (x0, x1) = (pts.iter().map(|p| p.x).min().unwrap(), pts.iter().map(|p| p.x).max().unwrap());
    let (y0, y1) = (pts.iter().map(|p| p.y).min().unwrap(), pts.iter().map(|p| p.y).max().unwrap());
    if x0 == x1 || y0 == y1 {
        return Some(Vec::new());
    }
    let mut ring = Ring { v: Vec::with_capacity(n * 2) };
    // `createList`: the winding order (`sum > 0` is clockwise in KiCad's frame, so the points go in backwards)
    let mut sum = 0.0;
    for i in 0..n {
        let (p1, p2) = (pts[i], pts[(i + 1) % n]);
        sum += (p2.x.as_f64() - p1.x.as_f64()) * (p2.y.as_f64() + p1.y.as_f64());
    }
    let order: Vec<usize> = if sum > 0.0 { (0..n).rev().collect() } else { (0..n).collect() };
    let mut tail: Option<usize> = None;
    let mut last_pt: Option<Point> = None;
    let level = SIMPLIFICATION_LEVEL * SIMPLIFICATION_LEVEL;
    for i in order {
        let pt = pts[i];
        let far = last_pt.is_none_or(|l| (pt.x - l.x).as_f64().powi(2) + (pt.y - l.y).as_f64().powi(2) > level);
        if far {
            tail = Some(ring.insert(i, pt, tail));
            last_pt = Some(pt);
        }
    }
    let mut first = tail?;
    let nx = ring.v[first].next;
    if ring.same(first, nx) && nx != first {
        ring.remove(nx);
    }
    if ring.v[first].prev == ring.v[first].next {
        return Some(Vec::new());
    }
    // `simplifyList`: drop a vertex nearer than the simplification level to the one before it
    {
        let start = first;
        let mut p = start;
        let mut next = ring.v[p].next;
        let mut retval = start;
        let mut count = 0;
        loop {
            let (dx, dy) = (ring.v[next].x - ring.v[p].x, ring.v[next].y - ring.v[p].y);
            if dx * dx + dy * dy < level {
                if next == start {
                    retval = p;
                    ring.remove(start);
                    count += 1;
                    break;
                }
                next = ring.v[next].next;
                let pn = ring.v[p].next;
                ring.remove(pn);
                count += 1;
                retval = p;
            } else {
                p = next;
                next = ring.v[next].next;
            }
            if p == start {
                break;
            }
        }
        if count > 0 {
            first = retval;
        }
    }
    // `updateList`: no two vertices in a row at one point
    {
        let mut p = ring.v[first].next;
        while p != first {
            if ring.same(p, ring.v[p].next) {
                p = ring.v[p].prev;
                let pn = ring.v[p].next;
                if pn == first {
                    first = p;
                }
                ring.remove(pn);
                if p == ring.v[p].next {
                    break;
                }
            }
            p = ring.v[p].next;
        }
    }
    let mut tris: Vec<[usize; 3]> = Vec::new();
    // `earcutList( firstVertex )`, its first pass
    let mut point = first;
    let mut stop = first;
    while ring.v[point].prev != ring.v[point].next {
        let prev = ring.v[point].prev;
        let next = ring.v[point].next;
        let mut best: Option<(usize, f64)> = None;
        let mut candidate = point;
        for _ in 0..2 {
            if ring.is_ear(candidate) && !ring.is_too_small(candidate) {
                let score = ring.ear_score(ring.v[candidate].prev, candidate, ring.v[candidate].next);
                if best.is_none_or(|(_, s)| score > s) {
                    best = Some((candidate, score));
                }
            }
            candidate = ring.v[candidate].next;
        }
        if let Some((ear, _)) = best {
            let (p, nx) = (ring.v[ear].prev, ring.v[ear].next);
            tris.push([ring.v[p].i, ring.v[ear].i, ring.v[nx].i]);
            ring.remove(ear);
            point = ring.v[nx].next;
            stop = point;
            continue;
        }
        let next_next = ring.v[next].next;
        if !ring.same(prev, next_next) && ring.intersects(prev, point, next, next_next) && ring.locally_inside(prev, next_next) && ring.locally_inside(next_next, prev) {
            tris.push([ring.v[prev].i, ring.v[point].i, ring.v[next_next].i]);
            ring.remove(next);
            ring.remove(point);
            point = next_next;
            stop = next_next;
            continue;
        }
        point = next;
        if point == stop && ring.v[point].prev != ring.v[point].next {
            // no ear left: KiCad subdivides or splits the ring here
            return None;
        }
    }
    // "Check to see if we are left with only three points in the polygon": a reflex leftover is dropped; anything else is a finished ring
    let left = ring.v[point];
    if left.prev != left.next && ring.v[left.next].next != left.prev && ring.ring_area(point).abs() > MINIMUM_AREA {
        return None;
    }
    Some(tris.into_iter().map(|[a, b, c]| [pts[a], pts[b], pts[c]]).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::units::{mm, Nm};

    fn p(x: f64, y: f64) -> Point {
        Point { x: mm(x), y: mm(y) }
    }

    fn area2(t: &[Point; 3]) -> f64 {
        ((t[1].x - t[0].x).as_f64() * (t[2].y - t[0].y).as_f64() - (t[2].x - t[0].x).as_f64() * (t[1].y - t[0].y).as_f64()).abs() / 2.0
    }

    #[test]
    fn a_rectangle_is_two_triangles_of_its_area() {
        let tris = tesselate(&[p(0.0, 0.0), p(4.0, 0.0), p(4.0, 2.0), p(0.0, 2.0)]).unwrap();
        assert_eq!(tris.len(), 2);
        let total: f64 = tris.iter().map(area2).sum();
        assert!((total - 8e12).abs() < 1.0, "{total}");
    }

    #[test]
    fn a_concave_outline_is_cut_into_triangles_inside_it() {
        // an L
        let l = [p(0.0, 0.0), p(4.0, 0.0), p(4.0, 1.0), p(1.0, 1.0), p(1.0, 3.0), p(0.0, 3.0)];
        let tris = tesselate(&l).unwrap();
        assert_eq!(tris.len(), 4);
        let total: f64 = tris.iter().map(area2).sum();
        assert!((total - 6e12).abs() < 1.0, "{total}");
        // no triangle covers the notch
        let notch = Point { x: mm(3.0), y: mm(2.0) };
        let shape = |t: &[Point; 3]| eda_drc::kimath::Shape::Polygon { pts: t.to_vec() };
        assert!(tris.iter().all(|t| shape(t).collides(&eda_drc::kimath::Shape::Circle { c: notch, r: Nm::ZERO }, Nm::ZERO).is_none()));
    }

    #[test]
    fn a_ring_with_no_area_has_no_triangles() {
        assert!(tesselate(&[p(0.0, 0.0), p(4.0, 0.0), p(8.0, 0.0)]).unwrap().is_empty());
    }
}
