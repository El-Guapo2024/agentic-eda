//! Port of `PNS::WALKAROUND` (`pcbnew/router/pns_walkaround.{h,cpp}`) and
//! the geometric core it calls into, `LINE::Walkaround`
//! (`pns_line.cpp`): make a path hug an obstacle's hull instead of
//! stopping at it, trying both winding directions and chaining onto
//! whatever new obstacle the hugged path runs into next.
//!
//! Scoped down from the full algorithm (see the task research spec this
//! was ported from, and `crates/pns/PARITY.md`):
//! - KiCad hugs a whole *cluster* of mutually-touching foreign-net items
//!   (`TOPOLOGY::AssembleCluster`) per step; this port hugs one obstacle
//!   item at a time (still re-detects and chains onto the next obstacle
//!   every iteration, so a multi-item blob is still walked around, just
//!   one hull-hug per loop iteration instead of one per whole blob).
//! - The `WP_SHORTEST` internal CW/CCW arbitration, the live-cursor
//!   proximity fallback, and the length-expansion/time-limit telemetry are
//!   replaced by the simpler contract every caller in this crate actually
//!   needs: try both windings, return both results, let the caller (which
//!   already knows the target point) pick.
//! - No `RestrictToCluster` scoping (no diff-pair/shove-internal caller
//!   needs it yet).
//!
//! The per-obstacle [`route`] below is what plain Walkaround mode still uses.
//! [`Walker`] is the faithful `WALKAROUND` -- policies, cluster hugging,
//! item masks, `RestrictToCluster`, the `WP_SHORTEST` check-back -- which
//! the shove needs (`onCollidingSolid` walks a line around the cluster of a
//! pad) and which the solids-only pre-pass of `rhShoveOnly` runs.

use crate::item::{ItemId, Net};
use crate::layer::LayerRange;
use crate::line::Line;
use crate::node::{kind_mask, Node, QueryOpts};
use eda_drc::kimath::{Seg, Shape};
use eda_model::ir::{Point, Um};
use eda_model::BoardRules;
use std::collections::HashSet;

fn dist_f(a: Point, b: Point) -> f64 {
    (((b.x - a.x) as f64).powi(2) + ((b.y - a.y) as f64).powi(2)).sqrt()
}

pub(crate) fn point_in_polygon(poly: &[Point], p: Point) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let n = poly.len();
    let mut inside = false;
    for i in 0..n {
        let (p1, p2) = (poly[i], poly[(i + 1) % n]);
        if (p1.y > p.y) != (p2.y > p.y) {
            let x_cross = p1.x as f64 + (p.y - p1.y) as f64 * (p2.x - p1.x) as f64 / (p2.y - p1.y) as f64;
            if (p.x as f64) < x_cross {
                inside = !inside;
            }
        }
    }
    inside
}

/// The hull vertices strictly between `from_edge` and `to_edge` (exclusive
/// of whatever point sits exactly on those edges -- the caller appends the
/// exact entry/exit points itself), walking forward (`from_edge+1 ..=
/// to_edge`) or backward (`from_edge ..= to_edge+1`, reversed to come out
/// in travel order). `forward`/`backward` is an internal, self-consistent
/// convention (hull vertex order as built by [`crate::hull::hull_of`]) --
/// it stands in for KiCad's CW/CCW, which is meaningless to fix to a
/// screen direction once board +y is "down" (see `crate::direction45`'s
/// doc comment on the same point).
fn hull_arc(hull: &[Point], from_edge: usize, to_edge: usize, forward: bool) -> Vec<Point> {
    let n = hull.len();
    let mut out = Vec::new();
    if forward {
        let mut i = (from_edge + 1) % n;
        for _ in 0..=n {
            out.push(hull[i]);
            if i == to_edge {
                break;
            }
            i = (i + 1) % n;
        }
    } else {
        let mut i = from_edge;
        let stop = (to_edge + 1) % n;
        for _ in 0..=n {
            out.push(hull[i]);
            if i == stop {
                break;
            }
            i = (i + n - 1) % n;
        }
    }
    out
}

/// `LINE::Walkaround(const SHAPE_LINE_CHAIN&, SHAPE_LINE_CHAIN&, bool aCw)`:
/// reroute `path` so it hugs `hull`'s boundary across whatever span of
/// `path` the hull blocks, instead of passing through it. `None` if `path`
/// starts inside the hull (KiCad: "you cannot walk around an obstacle you
/// already start inside of") or the walk cannot close.
pub fn walk_around_hull(path: &[Point], hull: &[Point], forward: bool) -> Option<Vec<Point>> {
    // `LINE::Walkaround` proper (crate::line_walk), on a clockwise hull.
    let cw_hull = crate::hull::make_clockwise(hull.to_vec());
    crate::line_walk::walkaround(path, &cw_hull, forward)
}

/// The previous, simplified hull walk (entry/exit crossing + hull arc),
/// kept for reference and its own tests.
#[allow(dead_code)]
fn walk_around_hull_simple(path: &[Point], hull: &[Point], forward: bool) -> Option<Vec<Point>> {
    if path.len() < 2 || hull.len() < 3 {
        return None;
    }
    if point_in_polygon(hull, path[0]) {
        return None;
    }

    // Every path-segment x hull-edge crossing, ordered by distance along
    // the path (so "entry" = first touch, "exit" = last touch).
    let mut hits: Vec<(f64, usize, usize, Point)> = Vec::new();
    let mut cum = 0.0;
    for i in 0..path.len() - 1 {
        let (a, b) = (path[i], path[i + 1]);
        for e in 0..hull.len() {
            let (c, d) = (hull[e], hull[(e + 1) % hull.len()]);
            if let Some(p) = Seg::new(a, b).intersect(&Seg::new(c, d)) {
                hits.push((cum + dist_f(a, p), i, e, p));
            }
        }
        cum += dist_f(a, b);
    }
    if hits.is_empty() {
        return Some(path.to_vec()); // the hull never actually blocks this path
    }
    hits.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
    hits.dedup_by(|a, b| (a.0 - b.0).abs() < 1.0);

    let (_, entry_seg, entry_edge, entry_pt) = hits[0];
    // Keep every untouched vertex up to and including the one that starts
    // the entry segment, then append the exact crossing point partway
    // along it (the entry segment itself gets truncated there, not
    // discarded -- overwriting `path[entry_seg]` here would drop the
    // path's own leading vertex/vertices entirely).
    let mut out = path[..=entry_seg].to_vec();
    out.push(entry_pt);

    let last_inside = point_in_polygon(hull, *path.last().unwrap());
    if hits.len() == 1 && last_inside {
        // The target itself sits inside the obstacle (cursor hovering over
        // it): hug up to the hull point nearest the target and stop there,
        // same as KiCad's early-projection exit -- the walk legitimately
        // ends short of reaching the original target.
        let target = *path.last().unwrap();
        let nearest_edge = (0..hull.len()).min_by(|&a, &b| {
            let da = Seg::new(hull[a], hull[(a + 1) % hull.len()]).sq_distance_to_point(target);
            let db = Seg::new(hull[b], hull[(b + 1) % hull.len()]).sq_distance_to_point(target);
            da.cmp(&db)
        })?;
        out.extend(hull_arc(hull, entry_edge, nearest_edge, forward));
        let (c, d) = (hull[nearest_edge], hull[(nearest_edge + 1) % hull.len()]);
        out.push(Seg::new(c, d).nearest_point(target));
        out.dedup();
        return Some(out);
    }

    let (_, exit_seg, exit_edge, exit_pt) = *hits.last().unwrap();
    out.extend(hull_arc(hull, entry_edge, exit_edge, forward));
    out.push(exit_pt);
    out.extend(path[(exit_seg + 1)..].to_vec());
    out.dedup();
    Some(out)
}

/// The nearest obstacle touching any leg of `path`, entry point included
/// (for ordering ties) -- `NODE::NearestObstacle`, narrowed to "first leg
/// that collides, nearest hit on it" rather than KiCad's whole-path
/// parallel search (see this module's doc comment).
fn first_obstacle(path: &[Point], node: &Node, net: &Net, layer: i32, width: Um, rules: &BoardRules, exclude: &[ItemId]) -> Option<ItemId> {
    for w in path.windows(2) {
        let shape = Shape::Stadium { a: w[0], b: w[1], r: width / 2 };
        let obstacles = node.all_colliding(&shape, net, LayerRange::single(layer), rules, exclude);
        if let Some(nearest) = obstacles.into_iter().min_by(|a, b| dist_f(w[0], a.pos).partial_cmp(&dist_f(w[0], b.pos)).unwrap()) {
            return Some(nearest.id);
        }
    }
    None
}

#[allow(clippy::too_many_arguments)] // mirrors the collision query's own (node, net, layer, width, rules, exclude) parameter set, plus the direction/iteration-limit this is specific to.
fn route_one(mut path: Vec<Point>, forward: bool, node: &Node, net: &Net, layer: i32, width: Um, rules: &BoardRules, exclude: &[ItemId], iteration_limit: u32) -> Option<Vec<Point>> {
    for _ in 0..iteration_limit {
        let Some(obstacle_id) = first_obstacle(&path, node, net, layer, width, rules, exclude) else {
            return Some(path); // ST_DONE: nothing left in the way
        };
        let obstacle = node.get(obstacle_id)?;
        let clearance = Node::clearance(rules, net, obstacle.net());
        let hull = obstacle.hull(clearance, width, layer);
        path = walk_around_hull(&path, &hull, forward)?;
        if path.len() < 2 {
            return None;
        }
    }
    None // iteration limit: ST_ALMOST_DONE/ST_STUCK, treated as failure here
}

pub struct WalkResult {
    pub forward: Option<Vec<Point>>,
    pub backward: Option<Vec<Point>>,
}

impl WalkResult {
    /// The shorter of the two successful candidates, if any -- the common
    /// case callers want (KiCad's `WP_SHORTEST`).
    pub fn best(&self) -> Option<&Vec<Point>> {
        fn len(p: &[Point]) -> f64 {
            p.windows(2).map(|w| dist_f(w[0], w[1])).sum()
        }
        match (&self.forward, &self.backward) {
            (Some(f), Some(b)) => Some(if len(f) <= len(b) { f } else { b }),
            (Some(f), None) => Some(f),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }
}

/// `WALKAROUND::Route`: try both windings, re-detecting and hugging
/// whatever obstacle the in-progress path runs into, up to
/// `iteration_limit` rounds per winding (`ROUTING_SETTINGS::
/// WalkaroundIterationLimit`, default 40).
#[allow(clippy::too_many_arguments)] // see `route_one`'s own note just above.
pub fn route(path: &[Point], node: &Node, net: &Net, layer: i32, width: Um, rules: &BoardRules, exclude: &[ItemId], iteration_limit: u32) -> WalkResult {
    WalkResult {
        forward: route_one(path.to_vec(), true, node, net, layer, width, rules, exclude, iteration_limit),
        backward: route_one(path.to_vec(), false, node, net, layer, width, rules, exclude, iteration_limit),
    }
}

/// `WALKAROUND::STATUS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkStatus {
    InProgress,
    AlmostDone,
    Done,
    Stuck,
    None,
}

/// `WALKAROUND::WALK_POLICY`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkPolicy {
    Cw = 0,
    Ccw = 1,
    Shortest = 2,
}

/// `WALKAROUND::RESULT`: one status and one path per policy.
#[derive(Debug, Clone)]
pub struct WalkOutcome {
    pub status: [WalkStatus; 3],
    pub lines: [Vec<Point>; 3],
}

impl WalkOutcome {
    pub fn status_of(&self, p: WalkPolicy) -> WalkStatus {
        self.status[p as usize]
    }
    pub fn line_of(&self, p: WalkPolicy) -> &[Point] {
        &self.lines[p as usize]
    }
}

/// `SHAPE_LINE_CHAIN::Length()`: the sum of the segments' rounded lengths.
pub(crate) fn chain_length(pts: &[Point]) -> i64 {
    pts.windows(2).map(|w| dist_f(w[0], w[1]).round() as i64).sum()
}

/// The faithful `PNS::WALKAROUND` (`pns_walkaround.cpp`): hug the nearest
/// obstacle's whole *cluster* per step, one path per allowed policy
/// (clockwise, counter-clockwise, and `WP_SHORTEST`, which tries both and
/// keeps the better one after a check-back against the clusters already
/// processed), until nothing is in the way, the path is stuck, grows past
/// `lengthExpansionFactor` times the initial length, or the iteration limit
/// is spent. The path's net, layer and width come from the line given to
/// [`Walker::route`].
pub struct Walker<'a> {
    node: &'a Node,
    rules: &'a BoardRules,
    /// `SetItemMask` / `SetSolidsOnly`: `kind_mask::*` bits an obstacle may have.
    pub item_mask: u8,
    /// `SetIterationLimit` (`ROUTING_SETTINGS::WalkaroundIterationLimit`).
    pub iteration_limit: u32,
    /// `SetLengthLimit( on, factor )`.
    pub length_limit_on: bool,
    pub length_expansion_factor: f64,
    policies: [bool; 3],
    /// `RestrictToCluster`: when non-empty, only these items are obstacles.
    restricted: HashSet<ItemId>,
    /// Items that never join a cluster (`MK_HEAD`), see [`Node::assemble_cluster`].
    skip: HashSet<ItemId>,
    /// Items that are never obstacles (the walking line's own segments in the node).
    exclude: Vec<ItemId>,
    // per-route state
    iteration: u32,
    initial_length: f64,
    processed: HashSet<ItemId>,
}

impl<'a> Walker<'a> {
    pub fn new(node: &'a Node, rules: &'a BoardRules) -> Self {
        Walker { node, rules, item_mask: kind_mask::ANY, iteration_limit: 40, length_limit_on: true, length_expansion_factor: 10.0, policies: [false; 3], restricted: HashSet::new(), skip: HashSet::new(), exclude: Vec::new(), iteration: 0, initial_length: 0.0, processed: HashSet::new() }
    }

    /// `SetAllowedPolicies`.
    pub fn set_allowed_policies(&mut self, policies: &[WalkPolicy]) {
        self.policies = [false; 3];
        for p in policies {
            self.policies[*p as usize] = true;
        }
    }

    /// `RestrictToCluster( true, cluster )`.
    pub fn restrict_to_cluster(&mut self, cluster: &[ItemId]) {
        self.restricted = cluster.iter().copied().collect();
    }

    /// Items that must not join the clusters the walk hugs.
    pub fn skip_items(&mut self, items: impl IntoIterator<Item = ItemId>) {
        self.skip = items.into_iter().collect();
    }

    /// Items that are never obstacles at all (a line already in the node must
    /// not be walked around by itself, which only matters for an unconnected net).
    pub fn exclude_items(&mut self, items: impl IntoIterator<Item = ItemId>) {
        self.exclude = items.into_iter().collect();
    }

    fn nearest_obstacle(&self, pts: &[Point], net: &Net, layer: i32, width: Um) -> Option<crate::node::NearestHit> {
        let line = Line::from_points(net.clone(), layer, width, pts.to_vec());
        let restricted = &self.restricted;
        let filter = |id: ItemId| restricted.contains(&id);
        let opts = QueryOpts { kind_mask: self.item_mask, filter: if restricted.is_empty() { None } else { Some(&filter) }, use_epsilon: true };
        self.node.nearest_obstacle(&line, None, self.rules, &self.exclude, opts)
    }

    /// `CheckColliding( &line )`: any leg touches anything in the world.
    fn world_collides(&self, pts: &[Point], net: &Net, layer: i32, width: Um) -> bool {
        pts.windows(2).any(|w| {
            let shape = Shape::Stadium { a: w[0], b: w[1], r: width / 2 };
            self.node.first_colliding(&shape, net, LayerRange::single(layer), self.rules, &self.exclude).is_some()
        })
    }

    /// `shortest->Collide( item, .. )`: does the path collide with this one item.
    fn collides_item(&self, pts: &[Point], net: &Net, layer: i32, width: Um, id: ItemId) -> bool {
        let Some(item) = self.node.get(id) else { return false };
        if !item.layers().overlaps_layer(layer) || crate::item::same_net(item.net(), net) {
            return false;
        }
        let clearance = Node::clearance(self.rules, net, item.net());
        let clearance = if clearance > 0 { (clearance - crate::node::CLEARANCE_EPSILON).max(0) } else { clearance };
        let item_shape = item.shape(item.layers().start());
        pts.windows(2).any(|w| Shape::Stadium { a: w[0], b: w[1], r: width / 2 }.collides(&item_shape, clearance).is_some())
    }

    /// `processCluster`: walk `pts` around every item of the cluster in turn.
    fn process_cluster(&self, cluster: &[ItemId], pts: &mut Vec<Point>, net: &Net, layer: i32, width: Um, cw: bool) -> bool {
        for &id in cluster {
            let Some(item) = self.node.get(id) else { continue };
            let clearance = Node::clearance(self.rules, net, item.net());
            let hull = item.hull(clearance, width, layer);
            let mut simplified = Line::from_points(net.clone(), layer, width, std::mem::take(pts));
            simplified.simplify();
            match walk_around_hull(&simplified.pts, &hull, cw) {
                Some(p) => *pts = p,
                None => {
                    *pts = simplified.pts;
                    return false;
                }
            }
        }
        true
    }

    fn single_step(&mut self, out: &mut WalkOutcome, net: &Net, layer: i32, width: Um) {
        let mut pending: [Vec<ItemId>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        for (i, cluster) in pending.iter_mut().enumerate() {
            if !self.policies[i] || out.status[i] != WalkStatus::InProgress {
                continue;
            }
            match self.nearest_obstacle(&out.lines[i], net, layer, width) {
                None => out.status[i] = WalkStatus::Done,
                Some(hit) => {
                    let skip = &self.skip;
                    *cluster = self.node.assemble_cluster(hit.id, layer, 0.0, Some(net), &|id| skip.contains(&id));
                }
            }
        }
        if self.policies[WalkPolicy::Cw as usize] {
            let mut pts = std::mem::take(&mut out.lines[0]);
            if !self.process_cluster(&pending[0], &mut pts, net, layer, width, true) {
                out.status[0] = WalkStatus::Stuck;
            }
            out.lines[0] = pts;
        }
        if self.policies[WalkPolicy::Ccw as usize] {
            let mut pts = std::mem::take(&mut out.lines[1]);
            if !self.process_cluster(&pending[1], &mut pts, net, layer, width, false) {
                out.status[1] = WalkStatus::Stuck;
            }
            out.lines[1] = pts;
        }
        if self.policies[WalkPolicy::Shortest as usize] {
            let line = out.lines[2].clone();
            let (mut path_cw, mut path_ccw) = (line.clone(), line);
            let st_cw = self.process_cluster(&pending[2], &mut path_cw, net, layer, width, true);
            let st_ccw = self.process_cluster(&pending[2], &mut path_ccw, net, layer, width, false);
            let cw_coll = st_cw && self.world_collides(&path_cw, net, layer, width);
            let ccw_coll = st_ccw && self.world_collides(&path_ccw, net, layer, width);
            let mut shortest: Option<Vec<Point>> = None;
            let mut shortest_alt: Option<Vec<Point>> = None;
            if st_cw && st_ccw {
                if (!cw_coll && !ccw_coll) || (cw_coll && ccw_coll) {
                    if chain_length(&path_cw) > chain_length(&path_ccw) {
                        shortest = Some(path_ccw);
                        shortest_alt = Some(path_cw);
                    } else {
                        shortest = Some(path_cw);
                        shortest_alt = Some(path_ccw);
                    }
                } else if !cw_coll {
                    shortest = Some(path_cw);
                } else if !ccw_coll {
                    shortest = Some(path_ccw);
                }
            } else if st_ccw {
                shortest = Some(path_ccw);
            } else if st_cw {
                shortest = Some(path_cw);
            }
            // check-back: the pick must not run into a cluster already hugged
            if let Some(sh) = &shortest {
                if self.processed.iter().any(|&id| self.collides_item(sh, net, layer, width, id)) {
                    shortest = shortest_alt;
                }
            }
            match shortest {
                None => out.status[2] = WalkStatus::Stuck,
                Some(sh) => out.lines[2] = sh,
            }
            self.processed.extend(pending[2].iter().copied());
        }
    }

    /// `WALKAROUND::Route( aInitialPath )`.
    pub fn route(&mut self, net: &Net, layer: i32, width: Um, initial: &[Point]) -> WalkOutcome {
        self.initial_length = chain_length(initial) as f64;
        self.iteration = 0;
        self.processed.clear();
        let mut out = WalkOutcome { status: [WalkStatus::InProgress; 3], lines: [initial.to_vec(), initial.to_vec(), initial.to_vec()] };
        while self.iteration < self.iteration_limit {
            self.single_step(&mut out, net, layer, width);
            let mut still_in_progress = false;
            for pol in 0..3 {
                if !self.policies[pol] {
                    continue;
                }
                let length_factor = if self.initial_length > 0.0 { chain_length(&out.lines[pol]) as f64 / self.initial_length } else { 0.0 };
                if self.length_limit_on && out.status[pol] != WalkStatus::Done && length_factor > self.length_expansion_factor {
                    out.status[pol] = WalkStatus::AlmostDone;
                }
                if out.status[pol] == WalkStatus::InProgress {
                    still_in_progress = true;
                }
            }
            if !still_in_progress {
                break;
            }
            self.iteration += 1;
        }
        for pol in 0..3 {
            if out.status[pol] == WalkStatus::InProgress {
                out.status[pol] = WalkStatus::AlmostDone;
            }
            let ln = &out.lines[pol];
            if ln.len() < 2 || ln[0] != initial[0] {
                out.status[pol] = WalkStatus::Stuck;
            }
            if !ln.is_empty() && ln.last() != initial.last() {
                out.status[pol] = WalkStatus::AlmostDone;
            }
        }
        out
    }
}

/// `rhWalkBase`'s walk of a head for a given item mask, for a head that has
/// no tail in this port: both windings (`{ WP_CCW, WP_CW }`), each finished
/// walk merged by the optimizer's `MERGE_SEGMENTS` against the *whole* world
/// (`OPTIMIZER::Optimize( &line, MERGE_SEGMENTS, node )`, whose collision
/// test ignores the kind mask), and the shorter of the two returned.
/// `None` when neither winding got through (`rhWalkBase` returns false).
#[allow(clippy::too_many_arguments)] // the node/rules/net/layer/width query context, plus the head, the kind mask and the iteration limit this is specific to.
pub fn walk_masked(node: &Node, rules: &BoardRules, net: &Net, layer: i32, width: Um, head: &[Point], mask: u8, iteration_limit: u32) -> Option<Vec<Point>> {
    let mut w = Walker::new(node, rules);
    w.item_mask = mask;
    w.iteration_limit = iteration_limit;
    w.set_allowed_policies(&[WalkPolicy::Ccw, WalkPolicy::Cw]);
    let wr = w.route(net, layer, width, head);
    let mut best: Option<(i64, Vec<Point>)> = None;
    for pol in [WalkPolicy::Cw, WalkPolicy::Ccw] {
        if wr.status_of(pol) != WalkStatus::Done {
            continue;
        }
        let line = Line::from_points(net.clone(), layer, width, wr.line_of(pol).to_vec());
        let merged = crate::optimizer::optimize_with(&line, node, rules, &[], crate::optimizer::effort::MERGE_SEGMENTS);
        let len = chain_length(&merged.pts);
        // `if( len_ccw < len_cw ) bestLine = ccw` -- CW wins ties
        if best.as_ref().is_none_or(|(bl, _)| len < *bl || (pol == WalkPolicy::Cw && len == *bl)) {
            best = Some((len, merged.pts));
        }
    }
    best.map(|(_, pts)| pts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{net_of, Item, Solid};

    fn rules() -> BoardRules {
        serde_yaml::from_str("track_width: 200\nclearance: 200\nvia_drill: 300\nvia_diameter: 600\n").unwrap()
    }

    #[test]
    fn straight_path_with_no_obstacle_is_unchanged() {
        let node = Node::new();
        let path = vec![Point { x: 0, y: 0 }, Point { x: 5000, y: 0 }];
        let wr = route(&path, &node, &net_of("SIG"), 0, 200, &rules(), &[], 10);
        assert_eq!(wr.best().unwrap(), &path);
    }

    /// A long leg whose straight-45-degree elbow passes exactly through a
    /// second, narrower obstacle on the way to a much farther target --
    /// the shape that originally caught the `Seg::intersect` sign bug this
    /// module depends on (see `eda_drc::kimath`'s own regression test,
    /// `asymmetric_crossing_point_is_on_both_segments`): a short
    /// roundrect pad straddling one long horizontal leg, asymmetric enough
    /// that a mislabeled crossing point lands nowhere near either segment.
    #[test]
    fn walks_around_a_narrow_pad_on_a_long_asymmetric_leg() {
        use crate::item::Solid;
        let mut node = Node::new();
        node.add(Item::Solid(Solid {
            net: None,
            layers: LayerRange::new(0, 1),
            pos: Point { x: 2475, y: -1905 },
            shape: Shape::RoundRect { x0: 2475 - 975, y0: -1905 - 300, x1: 2475 + 975, y1: -1905 + 300, r: 150 },
            source: "U1.8".into(),
        }));
        let rules: BoardRules = serde_yaml::from_str("track_width: 200\nclearance: 200\nvia_drill: 300\nvia_diameter: 600\n").unwrap();
        let path = vec![Point { x: -2475, y: -1905 }, Point { x: 8665, y: -1905 }, Point { x: 12475, y: 1905 }];
        let net = net_of("SIG");
        let wr = route(&path, &node, &net, 0, 200, &rules, &[], 40);
        let best = wr.best().expect("at least one winding must clear a single narrow pad");
        assert_eq!(best.first(), Some(&path[0]));
        assert_eq!(best.last(), Some(&path[2]));
        for w in best.windows(2) {
            let shape = Shape::Stadium { a: w[0], b: w[1], r: 100 };
            assert!(node.first_colliding(&shape, &net, LayerRange::single(0), &rules, &[]).is_none(), "leg {:?}-{:?} still collides", w[0], w[1]);
        }
    }

    #[test]
    fn walks_around_a_pad_blocking_the_direct_path() {
        let mut node = Node::new();
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: 2500, y: 0 }, shape: Shape::Circle { c: Point { x: 2500, y: 0 }, r: 500 }, source: "U1.1".into() }));
        let rules = rules();
        let path = vec![Point { x: 0, y: 0 }, Point { x: 5000, y: 0 }];
        let wr = route(&path, &node, &net_of("SIG"), 0, 200, &rules, &[], 10);
        assert!(wr.forward.is_some() || wr.backward.is_some(), "at least one winding must succeed");
        let best = wr.best().unwrap();
        assert_eq!(best.first(), Some(&Point { x: 0, y: 0 }));
        assert_eq!(best.last(), Some(&Point { x: 5000, y: 0 }));
        assert!(best.len() > 2, "a real detour must add at least one vertex");
        // The walked path must actually clear the pad at the required
        // clearance -- check every leg against the node directly, the same
        // query the router itself would use.
        for w in best.windows(2) {
            let shape = Shape::Stadium { a: w[0], b: w[1], r: 100 };
            assert!(node.first_colliding(&shape, &net_of("SIG"), LayerRange::single(0), &rules, &[]).is_none(), "leg {:?}-{:?} still collides with the pad", w[0], w[1]);
        }
    }

    fn walker_pad(node: &mut Node, net: &str, c: Point, r: Um) -> ItemId {
        node.add(Item::Solid(Solid { net: net_of(net), layers: LayerRange::new(0, 1), pos: c, shape: Shape::Circle { c, r }, source: "P".into() }))
    }

    fn walker_track(node: &mut Node, net: &str, a: Point, b: Point, width: Um) -> ItemId {
        node.add(Item::Segment(crate::item::Segment { net: net_of(net), layer: 0, a, b, width, source_track: None, locked: false }))
    }

    fn all_clear(node: &Node, pts: &[Point], width: Um, rules: &BoardRules) -> bool {
        pts.windows(2).all(|w| node.first_colliding(&Shape::Stadium { a: w[0], b: w[1], r: width / 2 }, &net_of("SIG"), LayerRange::single(0), rules, &[]).is_none())
    }

    /// `WALKAROUND` with `{ WP_CCW, WP_CW }`: each winding gets its own path, on opposite sides.
    #[test]
    fn both_windings_get_their_own_path_around_a_pad() {
        let mut node = Node::new();
        walker_pad(&mut node, "GND", Point { x: 2500, y: 0 }, 500);
        let rules = rules();
        let mut w = Walker::new(&node, &rules);
        w.set_allowed_policies(&[WalkPolicy::Ccw, WalkPolicy::Cw]);
        let out = w.route(&net_of("SIG"), 0, 200, &[Point { x: 0, y: 0 }, Point { x: 5000, y: 0 }]);
        assert_eq!(out.status_of(WalkPolicy::Cw), WalkStatus::Done);
        assert_eq!(out.status_of(WalkPolicy::Ccw), WalkStatus::Done);
        let (cw, ccw) = (out.line_of(WalkPolicy::Cw), out.line_of(WalkPolicy::Ccw));
        assert!(cw.iter().any(|p| p.y < -500) && ccw.iter().any(|p| p.y > 500) || cw.iter().any(|p| p.y > 500) && ccw.iter().any(|p| p.y < -500), "one each way: {cw:?} / {ccw:?}");
        assert!(all_clear(&node, cw, 200, &rules) && all_clear(&node, ccw, 200, &rules));
    }

    /// `SetItemMask( SOLID_T )`: tracks are not obstacles for the solids-only walk.
    #[test]
    fn the_solids_only_walk_goes_through_a_track() {
        let mut node = Node::new();
        walker_track(&mut node, "GND", Point { x: 2500, y: -2000 }, Point { x: 2500, y: 2000 }, 200);
        let rules = rules();
        let path = [Point { x: 0, y: 0 }, Point { x: 5000, y: 0 }];
        let mut solids = Walker::new(&node, &rules);
        solids.item_mask = kind_mask::SOLID;
        solids.set_allowed_policies(&[WalkPolicy::Cw]);
        assert_eq!(solids.route(&net_of("SIG"), 0, 200, &path).line_of(WalkPolicy::Cw), &path, "nothing to walk around");
        let mut any = Walker::new(&node, &rules);
        any.set_allowed_policies(&[WalkPolicy::Cw]);
        assert_ne!(any.route(&net_of("SIG"), 0, 200, &path).line_of(WalkPolicy::Cw), &path);
    }

    /// `AssembleCluster`: a pad and the track attached to it are one obstacle -- the path goes around the
    /// track's far end, not between the two.
    #[test]
    fn a_pad_and_its_attached_track_are_walked_around_together() {
        let mut node = Node::new();
        let pad = walker_pad(&mut node, "GND", Point { x: 2500, y: 0 }, 400);
        walker_track(&mut node, "GND", Point { x: 2500, y: 0 }, Point { x: 2500, y: 2500 }, 200);
        let rules = rules();
        let mut w = Walker::new(&node, &rules);
        w.item_mask = kind_mask::SOLID; // only the pad is "in the way" ...
        w.set_allowed_policies(&[WalkPolicy::Shortest]);
        let out = w.route(&net_of("SIG"), 0, 200, &[Point { x: 0, y: 0 }, Point { x: 5000, y: 0 }]);
        assert_eq!(out.status_of(WalkPolicy::Shortest), WalkStatus::Done);
        let path = out.line_of(WalkPolicy::Shortest);
        // ... but the walk hugs the whole blob: it takes the short way, under the pad, away from the track
        assert!(path.iter().all(|p| p.y <= 0 || p.y < 300), "{path:?}");
        assert!(path.iter().any(|p| p.y < -400), "{path:?}");
        assert!(all_clear(&node, path, 200, &rules));
        let _ = pad;
    }

    /// `RestrictToCluster`: only the cluster's items are obstacles.
    #[test]
    fn a_restricted_walk_ignores_everything_outside_the_cluster() {
        let mut node = Node::new();
        let near = walker_pad(&mut node, "GND", Point { x: 1500, y: 0 }, 300);
        walker_pad(&mut node, "VCC", Point { x: 3500, y: 0 }, 300);
        let rules = rules();
        let mut w = Walker::new(&node, &rules);
        w.restrict_to_cluster(&[near]);
        w.set_allowed_policies(&[WalkPolicy::Cw]);
        let out = w.route(&net_of("SIG"), 0, 200, &[Point { x: 0, y: 0 }, Point { x: 5000, y: 0 }]);
        let path = out.line_of(WalkPolicy::Cw);
        assert!(path.iter().any(|p| p.y.abs() > 300), "went around the first pad: {path:?}");
        assert!(!all_clear(&node, path, 200, &rules), "the second pad was not looked at, so the path may touch it");
    }

    /// `WP_SHORTEST` picks the shorter way round an off-centre pad.
    #[test]
    fn shortest_takes_the_shorter_side() {
        let mut node = Node::new();
        walker_pad(&mut node, "GND", Point { x: 2500, y: 150 }, 400); // the pad sits a little below the path (y grows downward)
        let rules = rules();
        let mut w = Walker::new(&node, &rules);
        w.set_allowed_policies(&[WalkPolicy::Shortest, WalkPolicy::Cw, WalkPolicy::Ccw]);
        let out = w.route(&net_of("SIG"), 0, 200, &[Point { x: 0, y: 0 }, Point { x: 5000, y: 0 }]);
        assert_eq!(out.status_of(WalkPolicy::Shortest), WalkStatus::Done);
        let shortest = chain_length(out.line_of(WalkPolicy::Shortest));
        assert_eq!(shortest, chain_length(out.line_of(WalkPolicy::Cw)).min(chain_length(out.line_of(WalkPolicy::Ccw))));
        assert!(out.line_of(WalkPolicy::Shortest).iter().any(|p| p.y < -400), "passes on the side away from the pad's centre");
    }

    /// `m_lengthExpansionFactor`: a walk that grows past ten times the direct length is abandoned.
    #[test]
    fn a_walk_that_runs_away_is_cut_off() {
        let mut node = Node::new();
        walker_pad(&mut node, "GND", Point { x: 2000, y: 0 }, 300);
        let rules = rules();
        let mut w = Walker::new(&node, &rules);
        w.length_expansion_factor = 1.01;
        w.set_allowed_policies(&[WalkPolicy::Cw]);
        let out = w.route(&net_of("SIG"), 0, 200, &[Point { x: 0, y: 0 }, Point { x: 4000, y: 0 }]);
        assert_eq!(out.status_of(WalkPolicy::Cw), WalkStatus::AlmostDone);
    }

    /// `rhWalkBase` with the solids mask: a head with a pad on it gets walked, one without is returned as it is.
    #[test]
    fn walk_masked_returns_the_head_untouched_when_no_pad_is_in_the_way() {
        let mut node = Node::new();
        walker_pad(&mut node, "GND", Point { x: 2500, y: 5000 }, 400);
        let rules = rules();
        let head = [Point { x: 0, y: 0 }, Point { x: 5000, y: 0 }];
        assert_eq!(walk_masked(&node, &rules, &net_of("SIG"), 0, 200, &head, kind_mask::SOLID, 40).unwrap(), head);
        let mut blocked = Node::new();
        walker_pad(&mut blocked, "GND", Point { x: 2500, y: 0 }, 400);
        let walked = walk_masked(&blocked, &rules, &net_of("SIG"), 0, 200, &head, kind_mask::SOLID, 40).unwrap();
        assert!(walked.len() > 2 && all_clear(&blocked, &walked, 200, &rules));
    }

    #[test]
    fn starting_inside_the_hull_fails() {
        let hull = crate::hull::hull_of(&Shape::Circle { c: Point { x: 0, y: 0 }, r: 1000 }, 0, 0);
        let path = vec![Point { x: 0, y: 0 }, Point { x: 5000, y: 0 }];
        assert!(walk_around_hull(&path, &hull, true).is_none());
    }
}
