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
