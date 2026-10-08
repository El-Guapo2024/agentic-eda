//! Port of `PNS::LINE` (`pcbnew/router/pns_line.h`): a track "as a whole" --
//! a polyline connecting two non-trivial joints -- assembled on demand from
//! the [`crate::node::Node`]'s individual [`crate::item::Segment`]s rather
//! than stored directly, exactly as KiCad's own doc comment on `LINE`
//! describes. A `Line` can be "loose" (built fresh by a placer/optimizer,
//! not yet linked into any `Node`) or the result of
//! [`crate::node::Node::assemble_line`] (tagged with the `segment_ids` it
//! came from, so a caller can remove exactly those before re-adding the
//! edited geometry).

use crate::item::{ItemId, Net};
use eda_model::ir::{Point, Um};

#[derive(Debug, Clone)]
pub struct Line {
    pub net: Net,
    pub layer: i32,
    pub width: Um,
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
}

impl Line {
    pub fn new(net: Net, layer: i32, width: Um) -> Self {
        Line { net, layer, width, pts: Vec::new(), segment_ids: Vec::new(), via_at_start: None, via_at_end: None }
    }

    pub fn from_points(net: Net, layer: i32, width: Um, pts: Vec<Point>) -> Self {
        Line { net, layer, width, pts, segment_ids: Vec::new(), via_at_start: None, via_at_end: None }
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
        self.pts.windows(2).map(|w| (((w[1].x - w[0].x) as f64).powi(2) + ((w[1].y - w[0].y) as f64).powi(2)).sqrt()).sum()
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
        let n_segs = self.pts.len().saturating_sub(1);
        let width = self.width;
        let new_pts = if index == 0 {
            let mut rev = self.pts.clone();
            rev.reverse();
            let mut out = drag_corner_internal(&rev, p);
            out.reverse();
            out
        } else if index >= n_segs {
            drag_corner_internal(&self.pts, p)
        } else {
            let mut path = drag_corner_internal(&self.pts[..=index], p);
            let mut tail: Vec<Point> = self.pts[index..].to_vec();
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

    /// Drop consecutive duplicate/collinear points -- `SHAPE_LINE_CHAIN::
    /// Simplify()`'s collinear-removal half (arc simplification doesn't
    /// apply; this port has no arc geometry). Exact collinearity only
    /// (integer cross product == 0); near-collinear merging is the
    /// optimizer's job (`crate::optimizer`), not this basic cleanup.
    pub fn simplify(&mut self) {
        if self.pts.len() < 3 {
            return;
        }
        let mut out = Vec::with_capacity(self.pts.len());
        out.push(self.pts[0]);
        for i in 1..self.pts.len() - 1 {
            let (a, b, c) = (*out.last().unwrap(), self.pts[i], self.pts[i + 1]);
            let cross = (b.x - a.x) as i128 * (c.y - a.y) as i128 - (b.y - a.y) as i128 * (c.x - a.x) as i128;
            if cross == 0 && on_segment(a, c, b) {
                continue; // b is collinear with and between a,c -- drop it.
            }
            out.push(b);
        }
        out.push(*self.pts.last().unwrap());
        out.dedup();
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

fn on_segment(a: Point, c: Point, b: Point) -> bool {
    b.x >= a.x.min(c.x) && b.x <= a.x.max(c.x) && b.y >= a.y.min(c.y) && b.y <= a.y.max(c.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplify_drops_collinear_midpoint() {
        let mut l = Line::from_points(None, 0, 200, vec![Point { x: 0, y: 0 }, Point { x: 500, y: 0 }, Point { x: 1000, y: 0 }]);
        l.simplify();
        assert_eq!(l.pts, vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }]);
    }

    #[test]
    fn simplify_keeps_real_corner() {
        let mut l = Line::from_points(None, 0, 200, vec![Point { x: 0, y: 0 }, Point { x: 500, y: 0 }, Point { x: 500, y: 500 }]);
        l.simplify();
        assert_eq!(l.pts.len(), 3);
    }

    #[test]
    fn reverse_swaps_via_ends() {
        let mut l = Line::from_points(None, 0, 200, vec![Point { x: 0, y: 0 }, Point { x: 100, y: 0 }]);
        l.via_at_start = Some(5);
        l.reverse();
        assert_eq!(l.via_at_end, Some(5));
        assert_eq!(l.first(), Some(Point { x: 100, y: 0 }));
    }
}
