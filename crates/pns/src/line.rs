//! Port of `PNS::LINE` (`pcbnew/router/pns_line.h`): a track "as a whole" --
//! a polyline connecting two non-trivial joints -- assembled on demand from
//! the [`crate::node::Node`]'s individual [`crate::item::Segment`]s rather
//! than stored directly, exactly as KiCad's own doc comment on `LINE`
//! describes. A `Line` can be "loose" (built fresh by a placer/optimizer,
//! not yet linked into any `Node`) or the result of
//! [`crate::node::Node::assemble_line`] (tagged with the `segment_ids` it
//! came from, so a caller can remove exactly those before re-adding the
//! edited geometry).

use crate::arcs::ArcPts;
use crate::direction45::{AngleType, Direction45};
use crate::item::{ItemId, Net};
use crate::seg_ops::{self, Seg2};
use eda_model::ir::{Point, Nm};

#[derive(Debug, Clone)]
pub struct Line {
    pub net: Net,
    pub layer: i32,
    pub width: Nm,
    /// At least 1 point (a lone via has exactly 1); 0 or 1 points means
    /// there is no routable segment yet (`PointCount() < 2` in KiCad).
    pub pts: Vec<Point>,
    /// The `Node`'s own segment ids this line was assembled from, in
    /// point order -- empty for a line that was never linked into a node
    /// (e.g. the live preview head while routing). `Node::commit_line`
    /// uses this to know what to remove before re-adding the edited shape.
    pub segment_ids: Vec<ItemId>,
    /// A via sitting at `pts[0]`/`pts[last]`, if this line's end is
    /// terminated by one (`LINE::EndsWithVia`, `JOINT::Via()`) -- carried
    /// so a placer/dragger can keep a via "attached" the way the task
    /// spec's drag requirement means.
    pub via_at_start: Option<ItemId>,
    pub via_at_end: Option<ItemId>,
    /// The arcs the line was made with (`SHAPE_LINE_CHAIN::m_arcs`): by the points of the track arc, in the orientation it has in the node. A
    /// line holds an arc as the points of its polyline ([`ArcPts::polyline`], what `SHAPE_LINE_CHAIN::Append( SHAPE_ARC )` appends), so
    /// every piece of code that works on `pts` works on an arc as the chords it is; this list says which runs of `pts` are an arc. An arc
    /// is **found again by its points** ([`Self::arc_runs`]): whatever edits `pts` and leaves the points of an arc as they are leaves it an
    /// arc, and whatever cuts into them -- a vertex inserted, a point moved or dropped -- turns it into plain segments, as KiCad's
    /// `SHAPE_LINE_CHAIN::Replace`, `Insert` and `Split` do with an arc they cut. So a copy of a line (`..line.clone()`, `Line { pts, ..line }`)
    /// keeps the arcs that survive, and a line built from bare points ([`Self::from_points`]) has none.
    pub arcs: Vec<ArcPts>,
}

/// Where one of a line's arcs is: the points `first..=last` of the line are the polyline of `arcs[arc]`, walked forward or (`reversed`) backward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArcRun {
    pub arc: usize,
    pub first: usize,
    pub last: usize,
    pub reversed: bool,
}

/// [`Line::arc_runs`] over any list of points: the runs of `pts` that are the polyline of one of `arcs`.
pub fn arc_runs_in(arcs: &[ArcPts], pts: &[Point]) -> Vec<ArcRun> {
    let mut runs: Vec<ArcRun> = Vec::new();
    for (k, arc) in arcs.iter().enumerate() {
        let poly = arc.polyline();
        let n = poly.len();
        if n < 2 || n > pts.len() {
            continue;
        }
        for first in 0..=(pts.len() - n) {
            let window = &pts[first..first + n];
            let forward = window == poly.as_slice();
            let backward = !forward && window.iter().zip(poly.iter().rev()).all(|(a, b)| a == b);
            if forward || backward {
                runs.push(ArcRun { arc: k, first, last: first + n - 1, reversed: backward });
                break;
            }
        }
    }
    runs.sort_by_key(|r| r.first);
    // two arcs share an end at most: an overlap is a stale one
    let mut kept: Vec<ArcRun> = Vec::new();
    for r in runs {
        if kept.last().is_none_or(|l| r.first >= l.last) {
            kept.push(r);
        }
    }
    kept
}

/// A line cut into the pieces the IR holds: a polyline for a run of straight segments, an arc track for an arc.
#[derive(Debug, Clone, PartialEq)]
pub enum LinePiece {
    Poly(Vec<Point>),
    Arc(ArcPts),
}

impl Line {
    pub fn new(net: Net, layer: i32, width: Nm) -> Self {
        Line { net, layer, width, pts: Vec::new(), segment_ids: Vec::new(), via_at_start: None, via_at_end: None, arcs: Vec::new() }
    }

    pub fn from_points(net: Net, layer: i32, width: Nm, pts: Vec<Point>) -> Self {
        Line { net, layer, width, pts, segment_ids: Vec::new(), via_at_start: None, via_at_end: None, arcs: Vec::new() }
    }

    /// `LINE::ArcCount`: how many of the line's arcs are still arcs.
    pub fn arc_count(&self) -> usize {
        self.arc_runs().len()
    }

    /// Where the line's arcs are in its points: for each arc in [`Self::arcs`], the run of points that is its polyline, if the line still has
    /// them as they were made. The runs are in point order and never overlap.
    pub fn arc_runs(&self) -> Vec<ArcRun> {
        arc_runs_in(&self.arcs, &self.pts)
    }

    /// `SHAPE_LINE_CHAIN::IsArcSegment`: is segment `i` (from point `i` to point `i + 1`) a chord of one of the line's arcs?
    pub fn is_arc_segment(&self, i: usize) -> bool {
        self.arcs.is_empty() == false && self.arc_runs().iter().any(|r| i >= r.first && i < r.last)
    }

    /// `SHAPE_LINE_CHAIN::IsPtOnArc`: is point `i` one of an arc's points (its ends included)?
    pub fn is_pt_on_arc(&self, i: usize) -> bool {
        self.arcs.is_empty() == false && self.arc_runs().iter().any(|r| i >= r.first && i <= r.last)
    }

    /// `IsPtOnArc( i )` of the line the points `pts` would make with this line's arcs -- what the drag code asks of the path it has been editing.
    fn pt_on_arc_of(&self, pts: &[Point], i: usize) -> bool {
        !self.arcs.is_empty() && arc_runs_in(&self.arcs, pts).iter().any(|r| i >= r.first && i <= r.last)
    }

    /// Append `arc` to the line (`SHAPE_LINE_CHAIN::Append( SHAPE_ARC )`): its points, from the line's end if it starts there (else from
    /// its other end, if that is where the line ends), and the arc is listed. The points are what [`ArcPts::polyline`] gives.
    pub fn append_arc(&mut self, arc: ArcPts) {
        let poly = arc.polyline();
        let from_end = self.pts.last().is_some_and(|&l| l == arc.end && l != arc.start);
        let it: Box<dyn Iterator<Item = Point>> = if from_end { Box::new(poly.iter().rev().copied()) } else { Box::new(poly.iter().copied()) };
        for p in it {
            if self.pts.last() != Some(&p) {
                self.pts.push(p);
            }
        }
        self.arcs.push(arc);
    }

    /// The line as the pieces the IR keeps it in: the runs of straight segments between its arcs as polylines, and each arc as an arc
    /// (oriented as the line goes). A line with no arc is the one polyline. Degenerate pieces (a single point) are left out.
    pub fn pieces(&self) -> Vec<LinePiece> {
        let runs = self.arc_runs();
        if runs.is_empty() {
            return if self.pts.len() >= 2 { vec![LinePiece::Poly(self.pts.clone())] } else { Vec::new() };
        }
        let mut out = Vec::new();
        let mut from = 0usize;
        for r in &runs {
            if r.first > from {
                out.push(LinePiece::Poly(self.pts[from..=r.first].to_vec()));
            }
            let a = self.arcs[r.arc];
            out.push(LinePiece::Arc(if r.reversed { a.reversed() } else { a }));
            from = r.last;
        }
        if from + 1 < self.pts.len() {
            out.push(LinePiece::Poly(self.pts[from..].to_vec()));
        }
        out
    }

    pub fn point_count(&self) -> usize {
        self.pts.len()
    }

    pub fn segment_count(&self) -> usize {
        self.pts.len().saturating_sub(1)
    }

    pub fn is_linked(&self) -> bool {
        !self.segment_ids.is_empty()
    }

    pub fn first(&self) -> Option<Point> {
        self.pts.first().copied()
    }

    pub fn last(&self) -> Option<Point> {
        self.pts.last().copied()
    }

    /// `SHAPE_LINE_CHAIN::Length()` -- sum of segment lengths.
    pub fn length(&self) -> f64 {
        self.pts.windows(2).map(|w| (((w[1].x - w[0].x).raw_shim() as f64).powi(2) + ((w[1].y - w[0].y).raw_shim() as f64).powi(2)).sqrt()).sum()
    }

    /// `SEG`s of this line, in order -- what `Node::commit_line` turns into
    /// `Segment` items.
    pub fn segs(&self) -> impl Iterator<Item = (Point, Point)> + '_ {
        self.pts.windows(2).map(|w| (w[0], w[1]))
    }

    /// `LINE::ClearLinks()`: detach from whatever node this was assembled
    /// from -- the geometry survives, the node bookkeeping doesn't. Used
    /// the same way KiCad uses it: a result handed back across an
    /// algorithm boundary is a free-floating copy, not a live alias into
    /// the node it was read from.
    pub fn clear_links(&mut self) {
        self.segment_ids.clear();
    }

    /// `LINE::Reverse()`.
    pub fn reverse(&mut self) {
        self.pts.reverse();
        self.segment_ids.reverse();
        std::mem::swap(&mut self.via_at_start, &mut self.via_at_end);
    }

    /// `LINE::HasLoops`: some point comes back to an earlier non-adjacent one.
    pub fn has_loops(&self) -> bool {
        (0..self.pts.len()).any(|i| ((i + 2)..self.pts.len()).any(|j| self.pts[i] == self.pts[j]))
    }

    /// `SHAPE_LINE_CHAIN::Find( p )`: the index of the vertex that is exactly `p`.
    pub fn find(&self, p: Point) -> Option<usize> {
        self.pts.iter().position(|&q| q == p)
    }

    /// `LINE::DragCorner( aP, aIndex )` in its default 45-degree mode
    /// (`dragCorner45` with no preferred ending direction): move vertex
    /// `index` to `p` and re-solve the legs next to it so the line stays on
    /// 45-degree headings, instead of leaving an arbitrary-angle leg.
    pub fn drag_corner45(&mut self, p: Point, index: usize) {
        self.drag_corner45_snapped(p, index, Nm::ZERO);
    }

    /// `LINE::DragCorner( aP, aIndex, aFreeAngle )`: `dragCornerFree` (the vertex goes to the cursor, whatever angle that
    /// makes) or `dragCorner45`, which snaps the cursor first by `snap` (`m_snapThreshhold`).
    pub fn drag_corner(&mut self, p: Point, index: usize, free_angle: bool, snap: Nm) {
        if free_angle {
            self.drag_corner_free(p, index);
        } else {
            self.drag_corner45_snapped(p, index, snap);
        }
    }

    /// `LINE::dragCornerFree`: move the vertex, then drop what became collinear or double.
    pub fn drag_corner_free(&mut self, p: Point, index: usize) {
        let mut idx = index;
        // "If we're asked to drag the end of an arc, insert a new vertex to drag instead": the arc keeps its end, the new vertex next to it goes
        // to the cursor (KiCad's `Insert( idx, GetPoint( idx ) )` for the arc's end copies the point after `idx++`; the vertex meant is the arc's own)
        if self.is_pt_on_arc(idx) {
            let numpts = self.pts.len();
            if idx == 0 || (idx > 0 && !self.is_pt_on_arc(idx - 1)) {
                self.pts.insert(idx, self.pts[idx]);
            } else if idx == numpts - 1 || (idx < numpts - 1 && !self.is_arc_segment(idx)) {
                self.pts.insert(idx + 1, self.pts[idx]);
                idx += 1;
            }
        }
        if let Some(v) = self.pts.get_mut(idx) {
            *v = p;
        }
        self.simplify();
    }

    /// `LINE::dragCorner45` with `m_snapThreshhold = snap`: [`Self::drag_corner45`] after `snapDraggedCorner` has pulled the
    /// cursor onto the point where two nearby segments of the line, at an obtuse angle, would meet.
    pub fn drag_corner45_snapped(&mut self, p: Point, index: usize, snap: Nm) {
        let n_segs = self.pts.len().saturating_sub(1);
        let width = self.width;
        let p = self.snap_dragged_corner(p, index, snap);
        let new_pts = if index == 0 {
            let mut rev = self.pts.clone();
            rev.reverse();
            let mut out = drag_corner_internal(&rev, p);
            out.reverse();
            out
        } else if index >= n_segs {
            drag_corner_internal(&self.pts, p)
        } else {
            // "Are we next to an arc? Insert a new point so we slice correctly"
            let mut pts = self.pts.clone();
            if self.pt_on_arc_of(&pts, index + 1) {
                pts.insert(index + 1, pts[index + 1]);
            }
            let mut path = drag_corner_internal(&pts[..=index], p);
            let mut tail: Vec<Point> = pts[index..].to_vec();
            tail.reverse();
            let mut tail = drag_corner_internal(&tail, p);
            tail.reverse();
            path.extend(tail);
            path
        };
        self.pts = new_pts;
        self.width = width;
        self.simplify();
    }

    /// The `i`th segment of the line as `SEG( CPoint( i ), CPoint( i + 1 ) )`.
    fn seg_at(&self, i: usize) -> Seg2 {
        (self.pts[i], self.pts[i + 1])
    }

    /// `LINE::snapDraggedCorner( aPath, aP, aIndex )`: when the cursor is within `snap` (`m_snapThreshhold`) of the point where two
    /// segments among those two either side of the dragged vertex, at an obtuse angle to each other, would meet if extended,
    /// the vertex goes there instead -- so a corner dragged near where it would fall onto the neighbouring legs' own corner
    /// lands on it. `snap <= 0` leaves the cursor alone.
    fn snap_dragged_corner(&self, p: Point, index: usize, snap: Nm) -> Point {
        if snap <= Nm::ZERO {
            return p;
        }
        let seg_count = self.segment_count() as isize;
        let s_start = (index as isize - 2).max(0);
        let s_end = (index as isize + 2).min(seg_count - 1);
        let (mut best_dist, mut best_snap) = (i64::MAX, p);
        for i in s_start..=s_end {
            let a = self.seg_at(i as usize);
            for j in s_start..i {
                let b = self.seg_at(j as usize);
                if Direction45::from_seg(a.0, a.1).angle_to(&Direction45::from_seg(b.0, b.1)) != AngleType::Obtuse {
                    continue;
                }
                if let Some(ip) = seg_ops::intersect_lines(a, b) {
                    let dist = seg_ops::length(ip, p);
                    if dist < snap.raw_shim() && dist < best_dist {
                        best_dist = dist;
                        best_snap = ip;
                    }
                }
            }
        }
        best_snap
    }

    /// `LINE::snapToNeighbourSegments( aPath, aP, aIndex )`: when the segment being dragged runs in the same direction as the
    /// one two before it or two after it, and the cursor is within `snap` of that one's line, the cursor snaps to its start
    /// -- so the dragged segment lines up with it and the two can merge.
    fn snap_to_neighbour_segments(path: &[Point], p: Point, index: usize, snap: Nm) -> Point {
        if snap == Nm::ZERO {
            return p;
        }
        let seg = |i: usize| (path[i], path[i + 1]);
        let seg_count = path.len() as isize - 1;
        let drag_dir = Direction45::from_seg(seg(index).0, seg(index).1);
        let mut snap_p = [p, p];
        let mut snap_d: [i64; 2] = [-1, -1];
        if index >= 2 {
            let s = seg(index - 2);
            if Direction45::from_seg(s.0, s.1) == drag_dir {
                snap_d[0] = seg_ops::line_distance(s, p);
            }
            snap_p[0] = s.0;
        }
        if (index as isize) < seg_count - 2 {
            let s = seg(index + 2);
            if Direction45::from_seg(s.0, s.1) == drag_dir {
                snap_d[1] = seg_ops::line_distance(s, p);
            }
            snap_p[1] = s.0;
        }
        let (mut best, mut min_dist) = (p, i64::MAX);
        for i in 0..2 {
            if snap_d[i] >= 0 && snap_d[i] < min_dist && snap_d[i] <= snap.raw_shim() {
                min_dist = snap_d[i];
                best = snap_p[i];
            }
        }
        best
    }

    /// `LINE::DragSegment( aP, aIndex )` (`dragSegment45`): slide segment `index` -- at whatever angle it has -- so that it passes
    /// through `p`, its direction unchanged. The legs on either side give way: each is stretched or shortened along its own
    /// direction when it meets the dragged one at a right or acute angle, and where it continues the dragged direction, or the
    /// dragged segment is the first or the last of the line, a new 45-degree leg is made to connect (whichever of the two ways
    /// of turning gives the shortest line). A line of three or more segments keeps its far ends where they were.
    /// `snap` is `m_snapThreshhold`: `snapToNeighbourSegments`.
    pub fn drag_segment45(&mut self, p: Point, index: usize, snap: Nm) {
        let m_seg_count = self.segment_count();
        if self.pts.len() < 2 || index >= m_seg_count {
            return;
        }
        let mut path = self.pts.clone();
        let target = Self::snap_to_neighbour_segments(&path, p, index, snap);

        // "We require a valid s_prev and s_next. If we are at the start or end of the line, we insert a new point at the start
        // or end so there is a zero-length segment for prev or next."
        let mut idx = index;
        // ... "If we are next to an arc, we do this also, as we cannot drag away one of the arc's points."
        if idx == 0 || self.pt_on_arc_of(&path, idx) {
            path.insert(if idx > 0 { idx + 1 } else { 0 }, path[idx]);
            idx += 1;
        }
        if idx == path.len() - 2 {
            let last = *path.last().unwrap();
            let at = path.len() - 1;
            path.insert(at, last);
        } else if self.pt_on_arc_of(&path, idx + 1) {
            path.insert(idx + 1, path[idx + 1]);
        }

        let seg = |path: &[Point], i: usize| -> Seg2 { (path[i], path[i + 1]) };
        let dir_of = |s: Seg2| Direction45::from_seg(s.0, s.1);
        let dragged = seg(&path, idx);
        let drag_dir = dir_of(dragged);
        let mut dir_prev = dir_of(seg(&path, idx - 1));
        let mut dir_next = dir_of(seg(&path, idx + 1));

        if dir_prev == drag_dir {
            dir_prev = dir_prev.left();
            path.insert(idx, path[idx]);
            idx += 1;
        } else if dir_prev == Direction45::Undefined {
            dir_prev = drag_dir.left();
        }
        if dir_next == drag_dir {
            dir_next = dir_next.right();
            path.insert(idx + 1, path[idx + 1]);
        } else if dir_next == Direction45::Undefined {
            dir_next = drag_dir.right();
        }

        let s_prev = seg(&path, idx - 1);
        let s_next = seg(&path, idx + 1);
        let dragged = seg(&path, idx);
        let ray = |from: Point, d: Direction45| -> Seg2 {
            let (dx, dy) = d.unit();
            (from, Point { x: from.x + Nm::from_raw_shim(dx), y: from.y + Nm::from_raw_shim(dy) })
        };

        let (guide_a, guide_b): ([Seg2; 2], [Seg2; 2]);
        if index == 0 {
            guide_a = [ray(dragged.0, drag_dir.right()), ray(dragged.0, drag_dir.left())];
        } else if matches!(dir_prev.angle_to(&drag_dir), AngleType::Obtuse | AngleType::HalfFull) {
            guide_a = [ray(s_prev.0, drag_dir.left()), ray(s_prev.0, drag_dir.right())];
        } else {
            let g = ray(dragged.0, dir_prev);
            guide_a = [g, g];
        }
        if index == m_seg_count - 1 {
            guide_b = [ray(dragged.1, drag_dir.right()), ray(dragged.1, drag_dir.left())];
        } else if matches!(dir_next.angle_to(&drag_dir), AngleType::Obtuse | AngleType::HalfFull) {
            guide_b = [ray(s_next.1, drag_dir.left()), ray(s_next.1, drag_dir.right())];
        } else {
            let g = ray(dragged.1, dir_next);
            guide_b = [g, g];
        }

        let s_current = ray(target, drag_dir);
        let length_of = |pts: &[Point]| -> i64 { pts.windows(2).map(|w| seg_ops::length(w[0], w[1])).sum() };
        let append = |pts: &mut Vec<Point>, q: Point| {
            if pts.last() != Some(&q) {
                pts.push(q);
            }
        };
        let (mut best_len, mut best): (i64, Vec<Point>) = (i64::MAX, Vec::new());
        for ga in &guide_a {
            for gb in &guide_b {
                let (Some(ip1), Some(ip2)) = (seg_ops::intersect_lines(s_current, *ga), seg_ops::intersect_lines(s_current, *gb)) else { continue };
                let s1 = (s_prev.0, ip1);
                let s3 = (ip2, s_next.1);
                let mut np: Vec<Point> = Vec::new();
                if let Some(ip) = seg_ops::intersect_segs(s1, s_next) {
                    append(&mut np, s1.0);
                    append(&mut np, ip);
                    append(&mut np, s_next.1);
                } else if let Some(ip) = seg_ops::intersect_segs(s3, s_prev) {
                    append(&mut np, s_prev.0);
                    append(&mut np, ip);
                    append(&mut np, s3.1);
                } else if let Some(ip) = seg_ops::intersect_segs(s1, s3) {
                    append(&mut np, s_prev.0);
                    append(&mut np, ip);
                    append(&mut np, s_next.1);
                } else {
                    append(&mut np, s_prev.0);
                    append(&mut np, ip1);
                    append(&mut np, ip2);
                    append(&mut np, s_next.1);
                }
                let len = length_of(&np);
                if len < best_len {
                    best_len = len;
                    best = np;
                }
            }
        }

        if self.pts.len() == 1 {
            self.pts = best;
        } else if index == 0 {
            replace_points(&mut self.pts, 0, 1, best);
        } else if index == m_seg_count - 1 {
            let n = self.pts.len();
            replace_points(&mut self.pts, n - 2, n - 1, best);
        } else {
            replace_points(&mut self.pts, index, index + 1, best);
        }
        self.simplify();
    }

    /// Drop consecutive duplicate/collinear points -- `SHAPE_LINE_CHAIN::
    /// Simplify()`'s collinear-removal half. Exact collinearity only
    /// (integer cross product == 0); near-collinear merging is the
    /// optimizer's job (`crate::optimizer`), not this basic cleanup.
    /// A point of an arc is never dropped (`Simplify` "never one on an arc").
    pub fn simplify(&mut self) {
        if self.pts.len() < 3 {
            return;
        }
        let runs = self.arc_runs();
        let protected = |i: usize| runs.iter().any(|r| i >= r.first && i <= r.last);
        let mut out = Vec::with_capacity(self.pts.len());
        out.push(self.pts[0]);
        for i in 1..self.pts.len() - 1 {
            let (a, b, c) = (*out.last().unwrap(), self.pts[i], self.pts[i + 1]);
            let cross = (b.x - a.x).raw_shim() as i128 * (c.y - a.y).raw_shim() as i128 - (b.y - a.y).raw_shim() as i128 * (c.x - a.x).raw_shim() as i128;
            if cross == 0 && on_segment(a, c, b) && !protected(i) {
                continue; // b is collinear with and between a,c -- drop it.
            }
            out.push(b);
        }
        out.push(*self.pts.last().unwrap());
        out.dedup();
        // `SHAPE_LINE_CHAIN::Simplify`: "if we have only one point, we need to add a second point to make a line"
        if out.len() == 1 {
            out.push(out[0]);
        }
        self.pts = out;
    }

    /// `SHAPE_LINE_CHAIN::Simplify2( aRemoveColinear = true )` (`libs/kimath/src/geometry/shape_line_chain.cpp`), which the router's
    /// walkaround (`WALKAROUND::singleStep`'s `processCluster`), shove (`shoveLineToHullSet`, `ShoveObstacleLine`, `onCollidingSolid`,
    /// `pushOrShoveVia`, `preShoveCleanup`) and optimizer (`mergeFull`, `mergeStep`, the pair bypasses) call, where the rest of the
    /// router calls [`Self::simplify`] (`Simplify()`). A chain of fewer than 3 points is left alone, and one of 3 points only loses
    /// its second point when it repeats the first ("Always try to keep at least 2 points"). Otherwise duplicates go (stage 1), and
    /// from each kept point `p0` every next point that lies within `SEG::LineDistance <= 1` of the **infinite** line from `p0` to the
    /// point after it, or is `SEG::Collinear` with it, goes too (stage 2) -- a fold-back such as (0,0),(10,0),(5,0),(5,5) loses
    /// (10,0), and so do the 1 nm kinks a hull walk leaves. Only a run that starts on two plain points (not an arc's,
    /// `SHAPES_ARE_PT`) is merged, as KiCad has it.
    pub fn simplify2(&mut self) {
        let n = self.pts.len();
        if n < 3 {
            return;
        }
        if n == 3 {
            if self.pts[0] == self.pts[1] {
                self.pts.remove(1);
            }
            return;
        }
        let runs = self.arc_runs();
        let on_arc: Vec<bool> = (0..n).map(|i| runs.iter().any(|r| i >= r.first && i <= r.last)).collect();
        // stage 1: eliminate duplicate vertices (an arc's points are all distinct, so KiCad's shape test is the plain one here)
        let mut uniq: Vec<(Point, bool)> = Vec::with_capacity(n);
        for (i, &p) in self.pts.iter().enumerate() {
            match uniq.last_mut() {
                Some(last) if last.0 == p => last.1 |= on_arc[i],
                _ => uniq.push((p, on_arc[i])),
            }
        }
        let np = uniq.len();
        let mut out: Vec<Point> = Vec::with_capacity(np);
        // stage 2: eliminate colinear segments
        let mut i = 0usize;
        while i + 2 < np {
            let p0 = uniq[i].0;
            let mut k = i;
            if !uniq[i].1 && !uniq[i + 1].1 {
                while k + 2 < np
                    && (crate::seg_ops::line_distance((p0, uniq[k + 2].0), uniq[k + 1].0) <= 1 || crate::seg_ops::collinear((p0, uniq[k + 2].0), (p0, uniq[k + 1].0)))
                {
                    k += 1;
                }
            }
            out.push(p0);
            if k > i {
                i = k;
            }
            if k == np - 2 {
                out.push(uniq[np - 1].0);
                self.pts = out;
                return;
            }
            i += 1;
        }
        if np > 1 {
            out.push(uniq[np - 2].0);
        }
        out.push(uniq[np - 1].0);
        self.pts = out;
    }
}

/// `dragCornerInternal` (`pns_line.cpp:668`): re-route the chain `origin`
/// so its last vertex ends at `p`, keeping 45-degree headings. Walks back
/// from the last segment looking for the first vertex from which a 45-degree
/// trace to `p` leaves in that segment's own heading, or at least turns
/// obtusely from the previous one; falls back to a fresh trace from the
/// chain's first point.
fn drag_corner_internal(origin: &[Point], p: Point) -> Vec<Point> {
    use crate::direction45::{AngleType, CornerMode, Direction45};
    let trace = |from: Point, diag: bool| Direction45::Undefined.build_initial_trace(from, p, diag, CornerMode::Mitered45);
    if origin.len() == 1 {
        return trace(origin[0], false);
    }
    if origin.len() == 2 {
        // `DIRECTION_45 dir( P0 - P1 )`: note the reversed vector, as KiCad has it.
        let dir = Direction45::from_vector(origin[0].x - origin[1].x, origin[0].y - origin[1].y);
        return trace(origin[0], dir.is_diagonal());
    }
    let n_segs = origin.len() - 1;
    let mut picked: Option<Vec<Point>> = None;
    let mut i = n_segs as isize - 1;
    while i >= 0 {
        let iu = i as usize;
        let d_start = Direction45::from_seg(origin[iu], origin[iu + 1]);
        let p_start = origin[iu];
        let d_prev = if iu > 0 { Direction45::from_seg(origin[iu - 1], origin[iu]) } else { Direction45::Undefined };
        // `paths[j] = d_start.BuildInitialTrace( p_start, aP, j )`
        let mut paths: Vec<(Vec<Point>, Direction45)> = Vec::new();
        for j in 0..2 {
            let path = d_start.build_initial_trace(p_start, p, j == 1, CornerMode::Mitered45);
            if path.len() < 2 {
                continue;
            }
            let dir = Direction45::from_seg(path[0], path[1]);
            paths.push((path, dir));
        }
        picked = paths.iter().find(|(_, d)| *d == d_start).map(|(pp, _)| pp.clone());
        if picked.is_none() {
            picked = paths.iter().find(|(_, d)| d.angle_to(&d_prev) == AngleType::Obtuse).map(|(pp, _)| pp.clone());
        }
        if picked.is_some() {
            break;
        }
        i -= 1;
    }
    match picked {
        Some(path) => {
            let mut out = origin[..=(i as usize)].to_vec();
            out.extend(path.into_iter().skip(1)); // `Append` drops the duplicated joint
            out
        }
        None => {
            let n = origin.len();
            let dir = Direction45::from_vector(origin[n - 1].x - origin[n - 2].x, origin[n - 1].y - origin[n - 2].y);
            trace(origin[0], dir.is_diagonal())
        }
    }
}

/// `SHAPE_LINE_CHAIN::Replace( aStartIndex, aEndIndex, aLine )`: the points `start..=end` are replaced by `with`, except that a first
/// point equal to `pts[start]` and a last one equal to `pts[end]` (when `end > 0`) are not repeated.
pub(crate) fn replace_points(pts: &mut Vec<Point>, start: usize, end: usize, mut with: Vec<Point>) {
    let (mut start, mut end) = (start, end);
    if with.is_empty() {
        remove_points(pts, start, end);
        return;
    }
    if with[0] == pts[start] {
        start += 1;
        with.remove(0);
        if with.is_empty() {
            remove_points(pts, start, end);
            return;
        }
    }
    if *with.last().unwrap() == pts[end] && end > 0 {
        end -= 1;
        with.pop();
    }
    remove_points(pts, start, end);
    if with.is_empty() {
        return;
    }
    let tail = pts.split_off(start.min(pts.len()));
    pts.extend(with);
    pts.extend(tail);
}

/// `SHAPE_LINE_CHAIN::Remove( aStartIndex, aEndIndex )`: nothing when the range is empty or past the end.
fn remove_points(pts: &mut Vec<Point>, start: usize, end: usize) {
    if start >= pts.len() || end >= pts.len() || start > end {
        return;
    }
    pts.drain(start..=end);
}

fn on_segment(a: Point, c: Point, b: Point) -> bool {
    b.x >= a.x.min(c.x) && b.x <= a.x.max(c.x) && b.y >= a.y.min(c.y) && b.y <= a.y.max(c.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplify_drops_collinear_midpoint() {
        let mut l = Line::from_points(None, 0, Nm::new(200), vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::new(500), y: Nm::ZERO }, Point { x: Nm::new(1000), y: Nm::ZERO }]);
        l.simplify();
        assert_eq!(l.pts, vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::new(1000), y: Nm::ZERO }]);
    }

    /// findings p5 F1-4 / F2-15: `SHAPE_LINE_CHAIN::Simplify2` drops a point on the infinite line through its neighbours, so a fold-back goes.
    #[test]
    fn simplify2_drops_a_backtracking_point_like_kicad() {
        let mut l = Line::from_points(None, 0, Nm::new(200), vec![p(0, 0), p(10, 0), p(5, 0), p(5, 5)]);
        l.simplify2();
        assert_eq!(l.pts, vec![p(0, 0), p(5, 0), p(5, 5)]);
    }

    /// `Simplify2` on 3 points only drops a repeated first point ("Always try to keep at least 2 points").
    #[test]
    fn simplify2_keeps_a_collinear_middle_of_three_points() {
        let mut l = Line::from_points(None, 0, Nm::new(200), vec![p(0, 0), p(500, 0), p(1000, 0)]);
        l.simplify2();
        assert_eq!(l.pts.len(), 3);
        let mut l = Line::from_points(None, 0, Nm::new(200), vec![p(0, 0), p(0, 0), p(1000, 0)]);
        l.simplify2();
        assert_eq!(l.pts, vec![p(0, 0), p(1000, 0)]);
    }

    /// The 1 nm kinks a walk round a hull leaves (`LineDistance <= 1`) go; a real corner stays.
    #[test]
    fn simplify2_drops_one_nanometre_kinks() {
        let mut l = Line::from_points(None, 0, Nm::new(200), vec![p(0, 0), p(0, 5_000_000), p(1, 5_000_001), p(0, 9_000_000), p(4_000_000, 9_000_000)]);
        l.simplify2();
        assert_eq!(l.pts, vec![p(0, 0), p(0, 9_000_000), p(4_000_000, 9_000_000)]);
    }

    #[test]
    fn simplify_keeps_real_corner() {
        let mut l = Line::from_points(None, 0, Nm::new(200), vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::new(500), y: Nm::ZERO }, Point { x: Nm::new(500), y: Nm::new(500) }]);
        l.simplify();
        assert_eq!(l.pts.len(), 3);
    }

    fn p(x: i64, y: i64) -> Point {
        Point { x: Nm::from_raw_shim(x), y: Nm::from_raw_shim(y) }
    }

    fn on_45(pts: &[Point]) -> bool {
        pts.windows(2).all(|w| {
            let (dx, dy) = ((w[1].x - w[0].x).abs(), (w[1].y - w[0].y).abs());
            dx == Nm::ZERO || dy == Nm::ZERO || dx == dy
        })
    }

    /// `LINE::DragCorner` (45-degree): dragging the end of a straight track just moves it along.
    #[test]
    fn drag_corner_keeps_a_straight_track_straight() {
        let mut l = Line::from_points(None, 0, Nm::new(200), vec![p(2500, 3000), p(2500, 100)]);
        l.drag_corner45(p(2500, 600), 1);
        assert_eq!(l.pts, vec![p(2500, 3000), p(2500, 600)]);
    }

    /// The fan-out of a pushed via (`pushOrShoveVia`): a diagonal track whose via moved straight up gets a
    /// new 45-degree leg and a straight one, never a leg at an odd angle.
    #[test]
    fn drag_corner_re_solves_the_last_leg_at_45_degrees() {
        let mut l = Line::from_points(None, 0, Nm::new(200), vec![p(4500, 2100), p(2500, 100)]);
        l.drag_corner45(p(2500, 600), 1);
        assert_eq!(l.pts, vec![p(4500, 2100), p(3000, 600), p(2500, 600)]);
        assert!(on_45(&l.pts));
    }

    #[test]
    fn drag_corner_of_a_longer_line_only_changes_the_end() {
        let mut l = Line::from_points(None, 0, Nm::new(200), vec![p(0, 0), p(2000, 0), p(2000, 2000), p(2000, 3000)]);
        l.drag_corner45(p(2600, 3200), 3);
        assert!(on_45(&l.pts), "{:?}", l.pts);
        assert_eq!(l.pts.first(), Some(&p(0, 0)));
        assert_eq!(l.pts.last(), Some(&p(2600, 3200)));
        assert!(l.pts.contains(&p(2000, 0)), "the part far from the corner is left alone: {:?}", l.pts);
    }

    #[test]
    fn drag_corner_at_the_start_works_on_the_reversed_line() {
        let mut l = Line::from_points(None, 0, Nm::new(200), vec![p(0, 0), p(1000, 0), p(1000, 1000)]);
        l.drag_corner45(p(-300, 100), 0);
        assert!(on_45(&l.pts), "{:?}", l.pts);
        assert_eq!(l.pts.first(), Some(&p(-300, 100)));
        assert_eq!(l.pts.last(), Some(&p(1000, 1000)));
    }

    /// Expected values: KiCad's own `LINE::dragSegment45` / `dragCorner45` / `snapToNeighbourSegments` / `snapDraggedCorner`,
    /// the function texts of `pcbnew/router/pns_line.cpp` at 8303b2ad compiled verbatim over the real `SEG` and `DIRECTION_45`
    /// (80 000 random lines, 45-degree and arbitrary-angle ones, agreed with this port point for point); these are a few of them.
    /// `(kind, snap, index, cursor, line, result)`.
    #[test]
    fn drags_give_what_kicads_own_functions_give() {
        type Case = (char, i64, usize, (i64, i64), &'static [(i64, i64)], &'static [(i64, i64)]);
        let cases: &[Case] = &[
            // a 45-degree diagonal between two straight legs slides along them
            ('S', 0, 1, (2500, 1500), &[(0, 0), (2000, 0), (3000, 1000), (5000, 1000)], &[(0, 0), (1000, 0), (2000, 1000), (5000, 1000)]),
            // a 30-degree segment (the legs are at 0 and 90 degrees): it is dragged at the 45-degree heading it quantizes to, and the legs are stretched
            ('S', 0, 1, (3000, 1500), &[(0, 0), (2000, 0), (3732, 1000), (3732, 3000)], &[(0, 0), (1500, 0), (3732, 2232), (3732, 3000)]),
            ('S', 0, 1, (3000, -500), &[(0, 0), (2000, 0), (3732, 1000), (3732, 3000)], &[(0, 0), (3500, 0), (3732, 232), (3732, 3000)]),
            // the first and the last segment of a line: a new 45-degree leg joins them to the rest
            ('S', 0, 0, (1000, 500), &[(0, 0), (2000, 0), (3000, 1000)], &[(0, 0), (500, 500), (2500, 500), (3000, 1000)]),
            ('S', 0, 0, (1000, 800), &[(0, 0), (4000, 0)], &[(0, 0), (800, 800), (3200, 800), (4000, 0)]),
            // a segment dragged along its own line changes nothing
            ('S', 0, 1, (2000, -2000), &[(0, 0), (2000, 0), (2000, 2000)], &[(0, 0), (2000, 0), (2000, 2000)]),
            // within the snap threshold of a parallel segment two along, the dragged one lines up with it
            ('S', 125, 1, (2000, 100), &[(0, 0), (2000, 0), (3000, 1000), (5000, 1000), (6000, 0)], &[(0, 0), (1900, 0), (2900, 1000), (5000, 1000), (6000, 0)]),
            ('S', 0, 1, (3500, 1000), &[(0, 0), (1000, 0), (2000, 1000), (3000, 1000), (5000, 1000)], &[(0, 0), (2500, 0), (3000, 500), (3000, 1000), (5000, 1000)]),
            // corner drags (`dragCorner45`): the end follows the cursor with 45-degree legs, within the threshold the corner snaps
            ('C', 0, 1, (1500, 800), &[(0, 0), (1000, 0), (1000, 1000)], &[(0, 0), (700, 0), (1500, 800), (1300, 1000), (1000, 1000)]),
            ('C', 0, 0, (-300, 100), &[(0, 0), (1000, 0), (1000, 1000)], &[(-300, 100), (-200, 0), (1000, 0), (1000, 1000)]),
            ('C', 0, 2, (2600, 3200), &[(0, 0), (2000, 0), (2000, 2000)], &[(0, 0), (2000, 0), (2000, 2600), (2600, 3200)]),
            ('C', 125, 1, (1100, 130), &[(0, 0), (2000, 0), (4000, 2000)], &[(0, 0), (970, 0), (1100, 130), (2130, 130), (4000, 2000)]),
        ];
        for (kind, snap, index, cursor, line, want) in cases {
            let pts: Vec<Point> = line.iter().map(|&(x, y)| p(x, y)).collect();
            let mut l = Line::from_points(None, 0, Nm::new(200), pts);
            let to = p(cursor.0, cursor.1);
            if *kind == 'S' {
                l.drag_segment45(to, *index, Nm::from_raw_shim(*snap));
            } else {
                l.drag_corner45_snapped(to, *index, Nm::from_raw_shim(*snap));
            }
            let want: Vec<Point> = want.iter().map(|&(x, y)| p(x, y)).collect();
            assert_eq!(l.pts, want, "{kind} snap {snap} index {index} to {cursor:?} on {line:?}");
        }
    }

    /// `dragCornerFree`: the vertex goes to the cursor at any angle; the 45-degree drag keeps 45-degree legs.
    #[test]
    fn a_free_angle_corner_drag_moves_the_vertex_to_the_cursor() {
        let mut free = Line::from_points(None, 0, Nm::new(200), vec![p(0, 0), p(1000, 0)]);
        free.drag_corner(p(1000, 300), 1, true, Nm::ZERO);
        assert_eq!(free.pts, vec![p(0, 0), p(1000, 300)]);
        let mut to45 = Line::from_points(None, 0, Nm::new(200), vec![p(0, 0), p(1000, 0)]);
        to45.drag_corner(p(1000, 300), 1, false, Nm::ZERO);
        assert!(on_45(&to45.pts), "{:?}", to45.pts);
        assert_eq!(to45.pts.last(), Some(&p(1000, 300)));
    }

    #[test]
    fn has_loops_and_find() {
        let l = Line::from_points(None, 0, Nm::new(200), vec![p(0, 0), p(10, 0), p(10, 10), p(0, 10), p(0, 0)]);
        assert!(l.has_loops());
        assert!(!Line::from_points(None, 0, Nm::new(200), vec![p(0, 0), p(10, 0), p(10, 10)]).has_loops());
        assert_eq!(l.find(p(10, 10)), Some(2));
        assert_eq!(l.find(p(7, 7)), None);
    }

    #[test]
    fn reverse_swaps_via_ends() {
        let mut l = Line::from_points(None, 0, Nm::new(200), vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::new(100), y: Nm::ZERO }]);
        l.via_at_start = Some(5);
        l.reverse();
        assert_eq!(l.via_at_end, Some(5));
        assert_eq!(l.first(), Some(Point { x: Nm::new(100), y: Nm::ZERO }));
    }
}
