//! `SHAPE_LINE_CHAIN` (`libs/kimath/src/geometry/shape_line_chain.cpp`), the part of it a meander is made and cut with: a polyline whose
//! runs of points can be arcs (`m_shapes`, one `(arc, arc)` pair per point, `SHAPE_IS_PT` for a plain point), `Append` of a point, of an
//! arc and of another chain, `Simplify`, `Split` / `Slice`, `Length` with its arcs measured as arcs, `Mirror`, `Reverse`.
//!
//! Nanometres, as everything in the tuning port (see [`crate::kigeom`]). Open chains only: a meander is never closed.
//!
//! Adapted: an arc is kept as the arc it was appended as, and as the chords of its polygonisation (1 um error,
//! `getArcPolygonizationMaxError`) for everything that wants segments, as KiCad does; `Collide( SEG )` tests the arc's chords where
//! KiCad tests the analytic arc (`SHAPE_ARC::Collide`), which differs by at most that error.

use eda_model::units::Nm;
use crate::kigeom::{isqrt, Seg, ShapeArc, ARC_POLYGONIZATION_MAX_ERROR_NM};
use eda_model::ir::Point;

/// `SHAPE_LINE_CHAIN::SHAPE_IS_PT`.
pub const SHAPE_IS_PT: isize = -1;
const SHAPES_ARE_PT: (isize, isize) = (SHAPE_IS_PT, SHAPE_IS_PT);

/// `SHAPE_LINE_CHAIN`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShapeLineChain {
    points: Vec<Point>,
    shapes: Vec<(isize, isize)>,
    arcs: Vec<ShapeArc>,
}

/// `TestSegmentHit( aRefPoint, aStart, aEnd, aDist )` (`libs/kimath/src/trigo.cpp`).
pub fn test_segment_hit(r: Point, start: Point, end: Point, dist: i64) -> bool {
    let (xmin, xmax) = (start.x.min(end.x), start.x.max(end.x));
    let (ymin, ymax) = (start.y.min(end.y), start.y.max(end.y));
    let delta = (start.x - r.x, start.y - r.y);
    if ymin - r.y > Nm::from_raw_shim(dist) || r.y - ymax > Nm::from_raw_shim(dist) {
        return false;
    }
    if xmin - r.x > Nm::from_raw_shim(dist) || r.x - xmax > Nm::from_raw_shim(dist) {
        return false;
    }
    if start.x == end.x && r.y > ymin && r.y < ymax {
        return delta.0.abs() <= Nm::from_raw_shim(dist);
    }
    if start.y == end.y && r.x > xmin && r.x < xmax {
        return delta.1.abs() <= Nm::from_raw_shim(dist);
    }
    Seg::new(start, end).squared_distance(r) < ((dist + 1) as i128) * ((dist + 1) as i128)
}

impl ShapeLineChain {
    pub fn new() -> Self {
        ShapeLineChain::default()
    }

    /// `SHAPE_LINE_CHAIN( const std::vector<VECTOR2I>& aV )`: plain points, repeats kept.
    pub fn from_points(pts: &[Point]) -> Self {
        ShapeLineChain { points: pts.to_vec(), shapes: vec![SHAPES_ARE_PT; pts.len()], arcs: Vec::new() }
    }

    pub fn clear(&mut self) {
        *self = ShapeLineChain::default();
    }

    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// `SegmentCount` of an open chain.
    pub fn segment_count(&self) -> usize {
        self.points.len().saturating_sub(1)
    }

    pub fn c_point(&self, i: usize) -> Point {
        self.points[i]
    }

    pub fn c_last_point(&self) -> Point {
        *self.points.last().expect("a chain with a point")
    }

    /// `SetPoint( aIndex, aPos )`: moves vertex `i` (an arc that has the vertex as an end keeps its own ends).
    pub fn set_point(&mut self, i: usize, p: Point) {
        self.points[i] = p;
    }

    pub fn points(&self) -> &[Point] {
        &self.points
    }

    pub fn arcs(&self) -> &[ShapeArc] {
        &self.arcs
    }

    /// `CShapes`: for each point the arc (or two arcs, at a shared point) it is part of, `SHAPE_IS_PT` for none.
    pub fn shapes(&self) -> &[(isize, isize)] {
        &self.shapes
    }

    pub fn arc(&self, i: usize) -> &ShapeArc {
        &self.arcs[i]
    }

    pub fn c_segment(&self, i: usize) -> Seg {
        Seg::new(self.points[i], self.points[i + 1])
    }

    // ------------------------------------------------------------------------------------------------- shapes

    pub fn is_shared_pt(&self, i: usize) -> bool {
        i < self.shapes.len() && self.shapes[i].0 != SHAPE_IS_PT && self.shapes[i].1 != SHAPE_IS_PT
    }

    pub fn is_pt_on_arc(&self, i: usize) -> bool {
        i < self.shapes.len() && self.shapes[i] != SHAPES_ARE_PT
    }

    /// `ArcIndex`: the arc segment `i` is part of, `SHAPE_IS_PT` if none.
    pub fn arc_index(&self, i: usize) -> isize {
        if self.is_shared_pt(i) {
            self.shapes[i].1
        } else {
            self.shapes[i].0
        }
    }

    /// `IsArcSegment`: segment `i` (from point `i` to point `i + 1`) is a chord of an arc.
    pub fn is_arc_segment(&self, i: usize) -> bool {
        let next = i + 1;
        if next + 1 > self.shapes.len() {
            return false;
        }
        self.is_pt_on_arc(i) && self.arc_index(i) == self.shapes[next].0
    }

    /// `IsArcStart`.
    pub fn is_arc_start(&self, i: usize) -> bool {
        if !self.is_arc_segment(i) {
            return false;
        }
        if self.is_shared_pt(i) {
            return true;
        }
        self.arcs[self.arc_index(i) as usize].start == self.points[i]
    }

    /// `IsArcEnd`.
    pub fn is_arc_end(&self, i: usize) -> bool {
        if i == 0 || i > self.points.len() - 1 || !self.is_arc_segment(i - 1) {
            return false;
        }
        if self.is_shared_pt(i) {
            return true;
        }
        self.arcs[self.arc_index(i) as usize].end == self.points[i]
    }

    /// `NextShape`: the index of the point where the shape after the one `i` is on starts, -1 at the end.
    pub fn next_shape(&self, i: isize) -> isize {
        let n = self.points.len() as isize;
        let mut i = if i < 0 { i + n } else { i };
        if i < 0 {
            return -1;
        }
        let last = n - 1;
        if i >= last {
            return -1;
        }
        let iu = i as usize;
        if self.shapes[iu] == SHAPES_ARE_PT {
            return if i == last - 1 { -1 } else { i + 1 };
        }
        let arc_start = i;
        if self.shapes[iu].0 == SHAPE_IS_PT {
            return -1; // malformed chain
        }
        let current = self.arc_index(iu);
        while i < last && self.arc_index(i as usize) == current {
            i += 1;
        }
        let still_on_arc = self.shapes[i as usize].0 == current || self.shapes[i as usize].1 == current;
        if i - arc_start > 1 && !still_on_arc {
            i -= 1;
        }
        if i == last {
            return -1;
        }
        i
    }

    // ------------------------------------------------------------------------------------------------- building

    /// `Append( const VECTOR2I& aP )`: a point equal to the last one is skipped.
    pub fn append_point(&mut self, p: Point) {
        if self.points.is_empty() || *self.points.last().unwrap() != p {
            self.points.push(p);
            self.shapes.push(SHAPES_ARE_PT);
        }
    }

    /// `Append( const SHAPE_LINE_CHAIN& )`.
    pub fn append_chain(&mut self, other: &ShapeLineChain) {
        if other.points.is_empty() {
            return;
        }
        let num_arcs = self.arcs.len() as isize;
        self.arcs.extend(other.arcs.iter().cloned());
        let fix = |s: (isize, isize)| -> (isize, isize) {
            let f = |v: isize| if v != SHAPE_IS_PT { v + num_arcs } else { v };
            (f(s.0), f(s.1))
        };
        if self.points.is_empty() || other.points[0] != *self.points.last().unwrap() {
            self.points.push(other.points[0]);
            self.shapes.push(fix(other.shapes[0]));
        } else if other.is_arc_segment(0) {
            // "Associate the new arc shape with the last point of this chain"
            let last = self.shapes.len() - 1;
            if self.shapes[last] == SHAPES_ARE_PT {
                self.shapes[last].0 = other.shapes[0].0 + num_arcs;
            } else {
                self.shapes[last].1 = other.shapes[0].0 + num_arcs;
            }
        }
        for i in 1..other.points.len() {
            self.points.push(other.points[i]);
            if other.arc_index(i) != SHAPE_IS_PT {
                self.shapes.push(fix(other.shapes[i]));
            } else {
                self.shapes.push(SHAPES_ARE_PT);
            }
        }
    }

    /// `Append( const SHAPE_ARC& aArc )`: the arc's polygonisation (1 um), marked as the arc it is when it has more than two points.
    pub fn append_arc(&mut self, arc: &ShapeArc) {
        self.append_arc_with(arc, ARC_POLYGONIZATION_MAX_ERROR_NM);
    }

    pub fn append_arc_with(&mut self, arc: &ShapeArc, max_error: i64) {
        let mut chain = ShapeLineChain::from_points(&[]);
        for p in arc.convert_to_polyline(max_error) {
            chain.append_point(p);
        }
        if chain.point_count() > 2 {
            let mut stored = arc.clone();
            stored.width = 0;
            chain.arcs.push(stored);
            for s in &mut chain.shapes {
                s.0 = 0;
            }
        }
        self.append_chain(&chain);
    }

    // ------------------------------------------------------------------------------------------------- measuring

    /// `Length`: the plain segments by `SEG::Length` (each rounded), the arcs by their true length.
    pub fn length(&self) -> i64 {
        let mut l: i64 = 0;
        for i in 0..self.segment_count() {
            if !self.is_arc_segment(i) {
                l += self.c_segment(i).length();
            }
        }
        for a in &self.arcs {
            l = (l as f64 + a.length()) as i64;
        }
        l
    }

    /// `Find( aP )`: the first vertex that is `aP` (no tolerance).
    pub fn find(&self, p: Point) -> Option<usize> {
        self.points.iter().position(|&q| q == p)
    }

    /// `NearestPoint( aP, aAllowInternalShapePoints )` for the chains this port cuts, which have no arcs (a nearest segment that is part of an arc
    /// snaps to the arc's end, as in KiCad).
    pub fn nearest_point(&self, p: Point, allow_internal: bool) -> Point {
        if self.points.is_empty() {
            return Point { x: Nm::ZERO, y: Nm::ZERO };
        }
        if self.points.len() == 1 {
            return self.points[0];
        }
        let mut min_d = i64::MAX;
        let mut nearest = 0usize;
        for i in 0..self.segment_count() {
            let d = self.c_segment(i).distance(p);
            if d < min_d {
                min_d = d;
                nearest = i;
            }
        }
        if !allow_internal && nearest > 0 && nearest < self.points.len() && self.is_arc_segment(nearest) {
            let seg = self.c_segment(nearest);
            let to_start = crate::kigeom::norm_i((seg.a.x - p.x).raw_shim(), (seg.a.y - p.y).raw_shim());
            let to_end = crate::kigeom::norm_i((seg.b.x - p.x).raw_shim(), (seg.b.y - p.y).raw_shim());
            if to_start > to_end {
                nearest += 1;
            }
            if self.is_arc_start(nearest) || self.is_arc_end(nearest) {
                return self.points[nearest];
            }
            let arc = &self.arcs[self.arc_index(nearest) as usize];
            let to_arc_start = crate::kigeom::norm_i((arc.start.x - p.x).raw_shim(), (arc.start.y - p.y).raw_shim());
            let to_arc_end = crate::kigeom::norm_i((arc.end.x - p.x).raw_shim(), (arc.end.y - p.y).raw_shim());
            return if to_arc_start > to_arc_end { arc.end } else { arc.start };
        }
        self.c_segment(nearest).nearest_point(p)
    }

    /// `SquaredDistance( aP )` of the chain's outline (the chords of the arcs included).
    pub fn squared_distance(&self, p: Point) -> i128 {
        let mut d = i128::MAX;
        for i in 0..self.segment_count() {
            d = d.min(self.c_segment(i).squared_distance(p));
        }
        d
    }

    /// `Collide( const SEG& aSeg, aClearance )` for the chain's plain segments and the chords of its arcs: is `aSeg` closer than the clearance (or
    /// touching) to the chain?
    pub fn collides_seg(&self, seg: &Seg, clearance: i64) -> bool {
        let clearance_sq = (clearance as i128) * (clearance as i128);
        let mut closest = i128::MAX;
        for i in 0..self.segment_count() {
            let d = self.c_segment(i).squared_distance_seg(seg);
            if d < closest {
                closest = d;
                if closest == 0 || closest < clearance_sq {
                    break;
                }
            }
        }
        closest == 0 || closest < clearance_sq
    }

    /// `PointOnEdge( aPt, aAccuracy )` (`EdgeContainingPoint`).
    pub fn point_on_edge(&self, p: Point, accuracy: i64) -> bool {
        let threshold = accuracy + 1;
        let threshold_sq = (threshold as i128) * (threshold as i128);
        match self.points.len() {
            0 => false,
            1 => {
                let d = ((self.points[0].x - p.x).raw_shim() as i128).pow(2) + ((self.points[0].y - p.y).raw_shim() as i128).pow(2);
                d <= threshold_sq
            }
            _ => (0..self.segment_count()).any(|i| {
                let s = self.c_segment(i);
                s.a == p || s.b == p || s.squared_distance(p) <= threshold_sq
            }),
        }
    }

    // ------------------------------------------------------------------------------------------------- editing

    /// `Mirror( const SEG& axis )`.
    pub fn mirror(&mut self, axis: &Seg) {
        for p in &mut self.points {
            *p = axis.reflect_point(*p);
        }
        for a in &mut self.arcs {
            a.mirror(axis);
        }
    }

    /// `Reverse`.
    pub fn reverse(&self) -> ShapeLineChain {
        let mut a = self.clone();
        a.points.reverse();
        a.shapes.reverse();
        a.arcs.reverse();
        let n = a.arcs.len() as isize;
        for sh in &mut a.shapes {
            if *sh != SHAPES_ARE_PT {
                for v in [&mut sh.0, &mut sh.1] {
                    if *v != SHAPE_IS_PT {
                        *v = n - *v - 1;
                    }
                }
                if sh.1 != SHAPE_IS_PT {
                    std::mem::swap(&mut sh.0, &mut sh.1);
                }
            }
        }
        for arc in &mut a.arcs {
            arc.reverse();
        }
        a
    }

    /// `convertArc`: the arc `idx` becomes plain points.
    fn convert_arc(&mut self, idx: isize) {
        for sh in &mut self.shapes {
            for v in [&mut sh.0, &mut sh.1] {
                if *v == idx {
                    *v = SHAPE_IS_PT;
                }
                if *v > idx {
                    *v -= 1;
                }
            }
            if sh.1 != SHAPE_IS_PT && sh.0 == SHAPE_IS_PT {
                std::mem::swap(&mut sh.0, &mut sh.1);
            }
        }
        self.arcs.remove(idx as usize);
    }

    /// `Simplify( 0 )`: drops the vertices that lie on the straight run between their neighbours (within a nanometre), never one on an arc.
    pub fn simplify(&mut self) {
        self.simplify_with(0);
    }

    pub fn simplify_with(&mut self, tolerance: i64) {
        let len = self.points.len();
        if len < 3 {
            return;
        }
        let mut new_points: Vec<Point> = Vec::with_capacity(len);
        let mut new_shapes: Vec<(isize, isize)> = Vec::with_capacity(len);
        let mut start_idx = 0usize;
        while start_idx < len {
            new_points.push(self.points[start_idx]);
            new_shapes.push(self.shapes[start_idx]);
            if start_idx == len - 2 {
                break;
            }
            let mut end_idx = (start_idx + 2) % len;
            let mut can_simplify = true;
            while can_simplify && end_idx != start_idx && end_idx > start_idx {
                let mut test_idx = (start_idx + 1) % len;
                while test_idx != end_idx {
                    if self.shapes[start_idx].0 != SHAPE_IS_PT || self.shapes[test_idx].0 != SHAPE_IS_PT || self.shapes[end_idx].0 != SHAPE_IS_PT {
                        can_simplify = false;
                        break;
                    }
                    if !test_segment_hit(self.points[test_idx], self.points[start_idx], self.points[end_idx], tolerance) {
                        can_simplify = false;
                        break;
                    }
                    test_idx = (test_idx + 1) % len;
                }
                if can_simplify {
                    end_idx = (end_idx + 1) % len;
                }
            }
            if end_idx == (start_idx + 2) % len {
                start_idx += 1;
            } else {
                let new_start_idx = (end_idx + len - 1) % len;
                if new_start_idx <= start_idx {
                    break;
                }
                start_idx = new_start_idx;
            }
        }
        if new_points.len() == 1 {
            new_points.push(*self.points.last().unwrap());
            new_shapes.push(*self.shapes.last().unwrap());
        }
        if self.points.last() != new_points.last() {
            new_points.push(*self.points.last().unwrap());
            new_shapes.push(*self.shapes.last().unwrap());
        }
        self.points = new_points;
        self.shapes = new_shapes;
    }

    /// `Insert( aVertex, aP )` for a plain point.
    fn insert_point(&mut self, at: usize, p: Point) {
        if at == self.points.len() {
            self.append_point(p);
            return;
        }
        // `splitArc( aVertex )`: "Nothing to do" for a point put in front of an arc's start
        let before_an_arc = !self.is_shared_pt(at) && self.is_arc_start(at);
        if at > 0 && self.is_pt_on_arc(at) && !before_an_arc {
            // an arc is cut here: it becomes plain points (KiCad splits it in two arcs; the points a tuner cuts a chain at are never inside one)
            let idx = self.arc_index(at);
            self.convert_arc(idx);
        }
        self.points.insert(at, p);
        self.shapes.insert(at, SHAPES_ARE_PT);
    }

    /// `Split( aP, aExact )`: makes `p` a vertex of the chain (if it is on it) and returns its index, -1 if it is not on the chain.
    pub fn split_point(&mut self, p: Point, exact: bool) -> isize {
        let found = self.find(p).map(|i| i as isize).unwrap_or(-1);
        if found >= 0 && exact {
            return found;
        }
        let mut ii: isize = -1;
        let mut min_dist = 2i64;
        for s in 0..self.segment_count() {
            let seg = self.c_segment(s);
            let dist = seg.distance(p);
            if dist < min_dist && seg.a != p && seg.b != p {
                min_dist = dist;
                if found < 0 || (s as isize) < found {
                    ii = s as isize;
                }
            }
        }
        if ii < 0 {
            ii = found;
        }
        if ii >= 0 {
            if self.points[ii as usize] == p {
                return ii;
            }
            let new_index = ii as usize + 1;
            self.insert_point(new_index, p);
            return new_index as isize;
        }
        -1
    }

    /// `Slice( aStart, aEnd )` (the vertices `aStart` to `aEnd`, both included; negative counts from the end), for a chain without arcs in the cut.
    pub fn slice(&self, start: isize, end: isize) -> ShapeLineChain {
        let n = self.points.len() as isize;
        let end = if end < 0 { end + n } else { end };
        let start = if start < 0 { start + n } else { start };
        if start < 0 || end < 0 || start >= n || end >= n || end < start {
            return ShapeLineChain::new();
        }
        let mut rv = ShapeLineChain::new();
        let mut i = start;
        while i <= end && i < n {
            if i == -1 {
                return rv;
            }
            let next = self.next_shape(i);
            let is_last = next < 0;
            if self.is_arc_start(i as usize) {
                if (is_last && end != n - 1) || next > end {
                    // cutting in the middle of an arc: keep its points and cut the arc itself
                    if i == end {
                        rv.points.push(self.points[i as usize]);
                        rv.shapes.push(SHAPES_ARE_PT);
                        return rv;
                    }
                    let arc_index = self.arc_index(i as usize);
                    let current = self.arcs[arc_index as usize].clone();
                    while i <= end && i < n {
                        if arc_index != self.arc_index(i as usize) {
                            break;
                        }
                        rv.points.push(self.points[i as usize]);
                        rv.shapes.push((rv.arcs.len() as isize, SHAPE_IS_PT));
                        i += 1;
                    }
                    let new_end = self.points[end as usize];
                    rv.arcs.push(ShapeArc::from_start_end_center(current.start, new_end, current.center(), current.is_clockwise()));
                    return rv;
                }
                rv.append_arc_with(&self.arcs[self.arc_index(i as usize) as usize], ARC_POLYGONIZATION_MAX_ERROR_NM);
                if is_last {
                    return rv;
                }
            } else {
                if i == start {
                    rv.append_point(self.points[i as usize]);
                }
                let next_is_arc = if is_last { false } else { self.is_arc_segment(next as usize) };
                if !next_is_arc && (i as usize) < self.segment_count() && i < end {
                    rv.append_point(self.points[i as usize + 1]);
                }
            }
            i = next;
            if i < 0 {
                break;
            }
        }
        rv
    }

    /// `Split( aStart, aEnd, aPre, aMid, aPost )`: the chain cut where it is nearest to `start` and to `end`: what comes before, the part between,
    /// what comes after. If the cut points come in the other order the chain is turned round first.
    pub fn split3(&self, start: Point, end: Point) -> (ShapeLineChain, ShapeLineChain, ShapeLineChain) {
        let n = self.nearest_point(end, false);
        let m = self.nearest_point(start, false);
        let mut l = self.clone();
        l.split_point(n, true);
        l.split_point(m, true);
        let (mut i_start, mut i_end) = (l.find(m), l.find(n));
        if let (Some(a), Some(b)) = (i_start, i_end) {
            if a > b {
                l = l.reverse();
                i_start = l.find(m);
                i_end = l.find(n);
            }
        }
        let (Some(i_start), Some(i_end)) = (i_start, i_end) else { return (ShapeLineChain::new(), ShapeLineChain::new(), ShapeLineChain::new()) };
        let pre = l.slice(0, i_start as isize);
        let post = l.slice(i_end as isize, -1);
        let mid = l.slice(i_start as isize, i_end as isize);
        (pre, mid, post)
    }
}

/// A convenience for the callers that hold plain polylines: the integer distance `SEG::Distance` gives, kept here so the `i128` squares do not
/// leak out of this module.
pub fn distance(a: Point, b: Point) -> i64 {
    isqrt(((a.x - b.x).raw_shim() as i128).pow(2) + ((a.y - b.y).raw_shim() as i128).pow(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i64, y: i64) -> Point {
        Point { x: Nm::from_raw_shim(x), y: Nm::from_raw_shim(y) }
    }

    fn quarter() -> ShapeArc {
        ShapeArc::from_start_end_angle(p(0, 0), p(1_000_000, 1_000_000), -90.0, 0)
    }

    #[test]
    fn appending_points_skips_a_repeat_and_keeps_the_order() {
        let mut c = ShapeLineChain::new();
        for q in [p(0, 0), p(0, 0), p(10, 0), p(10, 0), p(10, 10)] {
            c.append_point(q);
        }
        assert_eq!(c.points(), &[p(0, 0), p(10, 0), p(10, 10)]);
        assert_eq!((c.point_count(), c.segment_count(), c.length()), (3, 2, 20));
    }

    #[test]
    fn an_appended_arc_is_one_shape_of_many_chords_measured_as_an_arc() {
        let mut c = ShapeLineChain::new();
        c.append_point(p(-1_000_000, 0));
        c.append_point(p(0, 0));
        c.append_arc(&quarter());
        c.append_point(p(1_000_000, 3_000_000));
        assert_eq!(c.arcs().len(), 1);
        assert!(c.point_count() > 6);
        // the segments between the arc's own ends are its chords; the others are plain
        assert!(!c.is_arc_segment(0) && c.is_arc_segment(1) && c.is_arc_segment(c.point_count() - 3));
        assert!(!c.is_arc_segment(c.point_count() - 2));
        let expect = 1_000_000.0 + 1_000_000.0 * std::f64::consts::FRAC_PI_2 + 2_000_000.0;
        assert!((c.length() as f64 - expect).abs() < 2.0, "{} vs {expect}", c.length());
        // the arc starts where the line before it ended: its first point is shared, not repeated
        assert_eq!(c.points().iter().filter(|&&q| q == p(0, 0)).count(), 1);
    }

    #[test]
    fn appending_a_chain_that_starts_on_the_last_point_joins_without_a_repeat() {
        let mut a = ShapeLineChain::from_points(&[p(0, 0), p(10, 0)]);
        let b = ShapeLineChain::from_points(&[p(10, 0), p(10, 10)]);
        a.append_chain(&b);
        assert_eq!(a.points(), &[p(0, 0), p(10, 0), p(10, 10)]);
        let mut with_arc = ShapeLineChain::new();
        with_arc.append_point(p(0, 0));
        let mut arc_chain = ShapeLineChain::new();
        arc_chain.append_arc(&quarter());
        with_arc.append_chain(&arc_chain);
        assert_eq!(with_arc.arcs().len(), 1);
        assert!(with_arc.is_arc_segment(0), "the arc hangs off the shared first point");
    }

    #[test]
    fn simplify_drops_collinear_vertices_but_not_arc_vertices_or_the_ends() {
        let mut c = ShapeLineChain::from_points(&[p(0, 0), p(5, 0), p(10, 0), p(10, 10), p(10, 20)]);
        c.simplify();
        assert_eq!(c.points(), &[p(0, 0), p(10, 0), p(10, 20)]);
        let mut d = ShapeLineChain::from_points(&[p(0, 0), p(5, 5), p(10, 10), p(20, 20)]);
        d.simplify();
        assert_eq!(d.points(), &[p(0, 0), p(20, 20)], "a 45-degree run too");
        let mut e = ShapeLineChain::new();
        for x in [-3_000_000, -2_000_000, -1_000_000, 0] {
            e.append_point(p(x, 0));
        }
        e.append_arc(&quarter());
        let before = e.point_count();
        e.simplify();
        // the window that would take the arc's first vertex in is refused, so the vertex before the arc stays and one of the two before it goes
        assert_eq!(e.point_count(), before - 1);
        assert_eq!(&e.points()[..3], &[p(-3_000_000, 0), p(-1_000_000, 0), p(0, 0)]);
        assert_eq!(e.arcs().len(), 1);
    }

    #[test]
    fn mirroring_across_the_baseline_flips_the_arc_too() {
        let mut c = ShapeLineChain::new();
        c.append_point(p(0, 0));
        c.append_arc(&quarter());
        let axis = Seg::new(p(0, 0), p(1_000_000, 0));
        let mut m = c.clone();
        m.mirror(&axis);
        assert_eq!(m.points()[0], p(0, 0));
        assert_eq!(m.c_last_point(), p(1_000_000, -1_000_000));
        assert_eq!(m.arcs()[0].is_ccw(), !c.arcs()[0].is_ccw());
        assert!((m.length() - c.length()).abs() <= 1);
    }

    #[test]
    fn split_cuts_a_line_between_two_points_into_before_between_and_after() {
        let line = ShapeLineChain::from_points(&[p(0, 0), p(10_000, 0), p(10_000, 10_000)]);
        let (pre, mid, post) = line.split3(p(2_000, 0), p(10_000, 4_000));
        assert_eq!(pre.points(), &[p(0, 0), p(2_000, 0)]);
        assert_eq!(mid.points(), &[p(2_000, 0), p(10_000, 0), p(10_000, 4_000)]);
        assert_eq!(post.points(), &[p(10_000, 4_000), p(10_000, 10_000)]);
        // the cut points the other way round give the same cuts, the chain turned round: what comes before the start is now the far end
        let (pre2, mid2, post2) = line.split3(p(10_000, 4_000), p(2_000, 0));
        assert_eq!(pre2.points(), &[p(10_000, 10_000), p(10_000, 4_000)]);
        assert_eq!(mid2.points(), &[p(10_000, 4_000), p(10_000, 0), p(2_000, 0)]);
        assert_eq!(post2.points(), &[p(2_000, 0), p(0, 0)]);
        // a cut at the start of the line leaves nothing before it but the point
        let (pre3, mid3, _) = line.split3(p(0, 0), p(5_000, 0));
        assert_eq!(pre3.points(), &[p(0, 0)]);
        assert_eq!(mid3.points(), &[p(0, 0), p(5_000, 0)]);
    }

    #[test]
    fn a_chain_reversed_has_its_arcs_reversed_with_it() {
        let mut c = ShapeLineChain::new();
        c.append_point(p(-1_000_000, 0));
        c.append_point(p(0, 0));
        c.append_arc(&quarter());
        let r = c.reverse();
        assert_eq!(r.c_point(0), c.c_last_point());
        assert_eq!(r.c_last_point(), p(-1_000_000, 0));
        assert_eq!(r.arcs().len(), 1);
        assert!(r.is_arc_segment(0));
        assert_eq!(r.arcs()[0].start, quarter().end);
        assert!((r.length() - c.length()).abs() <= 1);
    }

    #[test]
    fn collide_with_a_segment_uses_the_clearance() {
        let c = ShapeLineChain::from_points(&[p(0, 0), p(10_000, 0)]);
        assert!(c.collides_seg(&Seg::new(p(5_000, 700), p(5_000, 2_000)), 800));
        assert!(!c.collides_seg(&Seg::new(p(5_000, 900), p(5_000, 2_000)), 800));
        assert!(c.collides_seg(&Seg::new(p(5_000, -500), p(5_000, 500)), 0), "crossing is a collision");
    }

    #[test]
    fn point_on_edge_allows_the_accuracy_asked() {
        let c = ShapeLineChain::from_points(&[p(0, 0), p(10, 0)]);
        assert!(c.point_on_edge(p(5, 1), 1) && !c.point_on_edge(p(5, 3), 1));
    }
}
