//! Port of `PNS::WALKAROUND` (`pcbnew/router/pns_walkaround.{h,cpp}`) and
//! the geometric core it calls into, `LINE::Walkaround`
//! (`pns_line.cpp`): make a path hug an obstacle's hull instead of
//! stopping at it, trying both winding directions and chaining onto
//! whatever new obstacle the hugged path runs into next.
//!
//! [`walk_around_hull`] is `LINE::Walkaround` for one hull. [`Walker`] is
//! the faithful `WALKAROUND` -- policies, cluster hugging, item masks,
//! `RestrictToCluster`, the `WP_SHORTEST` check-back, the length-expansion
//! cut-off -- which the shove needs (`onCollidingSolid` walks a line around
//! the cluster of a pad) and the dragger (`tryWalkaround`) uses. [`walk_base`]
//! is `LINE_PLACER::rhWalkBase` on top of it: both windings, the shorter
//! taken, and the hug of the obstacle (`cursorDistMinimum`) when the detour
//! is too long -- Walk around mode's walk and the solids-only pre-pass of
//! `rhShoveOnly`.
//!
//! Still scoped down: no `m_PNSProcessClusterTimeout` wall-clock bound (the
//! iteration limit, `WalkaroundIterationLimit`, is the only one).

use crate::item::{ItemId, Net};
use crate::layer::LayerRange;
use crate::line::Line;
use crate::node::{kind_mask, Node, QueryOpts, Side};
use eda_drc::kimath::{Seg, Shape};
use eda_model::ir::{Point, Nm};
use eda_model::BoardRules;
use std::collections::HashSet;

fn dist_f(a: Point, b: Point) -> f64 {
    (((b.x - a.x).as_f64()).powi(2) + ((b.y - a.y).as_f64()).powi(2)).sqrt()
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
            let x_cross = p1.x.as_f64() + (p.y - p1.y).as_f64() * (p2.x - p1.x).as_f64() / (p2.y - p1.y).as_f64();
            if ((p.x).as_f64()) < x_cross {
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
pub(crate) fn chain_length(pts: &[Point]) -> Nm {
    pts.windows(2).map(|w| Nm::kiround(dist_f(w[0], w[1]))).sum()
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
    /// The via the walking line ends in, if it does (`LINE::EndsWithVia`): it is an obstacle for the nearest-obstacle search and the
    /// collision checks, as it is in KiCad -- the walk itself goes around the cluster it finds, with the line's chain.
    end_via: Option<crate::item::Via>,
    /// The arcs of the line being walked (`Line::arcs`): the walk works on bare points, and the clean-up of a path that has an arc in it must not drop the vertex an
    /// arc starts or ends at when it is collinear with its neighbours (a fillet's first chord is: it is tangent to the run before it).
    pub arcs: Vec<crate::arcs::ArcPts>,
    /// The router's corner mode (`Settings().GetCornerMode()`, which `WALKAROUND` reads itself): in a 90-degree mode every hull the walk goes
    /// round is its bounding box (`processCluster`: "if( cornerMode == MITERED_90 || cornerMode == ROUNDED_90 ) hull = cachedHull.BBox()"), and
    /// so is the hull the nearest obstacle is measured on (`NODE::NearestObstacle`'s `simplifyHull`) -- a walk in a 90-degree mode goes round a
    /// pad in axis legs.
    pub corner_mode: crate::direction45::CornerMode,
    // per-route state
    iteration: u32,
    initial_length: f64,
    processed: HashSet<ItemId>,
}

impl<'a> Walker<'a> {
    pub fn new(node: &'a Node, rules: &'a BoardRules) -> Self {
        Walker { node, rules, item_mask: kind_mask::ANY, iteration_limit: 40, length_limit_on: true, length_expansion_factor: 10.0, policies: [false; 3], restricted: HashSet::new(), skip: HashSet::new(), exclude: Vec::new(), end_via: None, arcs: Vec::new(), corner_mode: crate::direction45::CornerMode::Mitered45, iteration: 0, initial_length: 0.0, processed: HashSet::new() }
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

    /// The walking line ends in `via` (a pair's head that places vias).
    pub fn set_end_via(&mut self, via: Option<crate::item::Via>) {
        self.end_via = via;
    }

    fn nearest_obstacle(&self, pts: &[Point], net: &Net, layer: i32, width: Nm) -> Option<crate::node::NearestHit> {
        let line = Line::from_points(net.clone(), layer, width, pts.to_vec());
        let restricted = &self.restricted;
        let filter = |id: ItemId| restricted.contains(&id);
        let opts = QueryOpts { kind_mask: self.item_mask, filter: if restricted.is_empty() { None } else { Some(&filter) }, use_epsilon: true, box_hull: self.corner_mode.is_90(), head_hole: None };
        self.node.nearest_obstacle(&line, self.end_via.as_ref(), self.rules, &self.exclude, opts)
    }

    /// `CheckColliding( &line )`: any leg touches anything in the world.
    fn world_collides(&self, pts: &[Point], net: &Net, layer: i32, width: Nm) -> bool {
        pts.windows(2).any(|w| {
            let shape = Shape::Stadium { a: w[0], b: w[1], r: width / 2 };
            self.node.first_colliding(&shape, net, LayerRange::single(layer), self.rules, &self.exclude).is_some()
        }) || self.end_via.as_ref().is_some_and(|v| !self.node.via_colliding(v, self.rules, &self.exclude, QueryOpts::default()).is_empty())
    }

    /// `shortest->Collide( item, .. )`: does the path collide with this one item ([`Node::item_collision`], with the epsilon).
    fn collides_item(&self, pts: &[Point], net: &Net, layer: i32, width: Nm, id: ItemId) -> bool {
        let Some(item) = self.node.get(id) else { return false };
        let opts = QueryOpts::default();
        pts.windows(2).any(|w| {
            let shape = Shape::Stadium { a: w[0], b: w[1], r: width / 2 };
            self.node.item_collision(self.rules, id, item, &shape, Side::Track { net, layer, seg: Some((w[0], w[1])) }, LayerRange::single(layer), opts).is_some()
        }) || self.end_via.as_ref().is_some_and(|v| {
            let opts = QueryOpts { head_hole: (v.drill > Nm::ZERO).then_some(v.drill / 2), ..opts };
            self.node.item_collision(self.rules, id, item, &Shape::Circle { c: v.pos, r: v.diameter / 2 }, Side::via(v), v.layers, opts).is_some()
        })
    }

    /// `processCluster`: walk `pts` around every item of the cluster in turn.
    fn process_cluster(&self, cluster: &[ItemId], pts: &mut Vec<Point>, net: &Net, layer: i32, width: Nm, cw: bool) -> bool {
        for &id in cluster {
            let Some(item) = self.node.get(id) else { continue };
            // `m_world->GetClearance( clItem, &aLine, false )`, `clItem->Hull( clearance, aLine.Width(), layer )`
            let hull = self.node.obstacle_hull(self.rules, item, Side::Track { net, layer, seg: pts.first().zip(pts.get(1)).map(|(a, b)| (*a, *b)) }, Nm::ZERO, width, layer, false);
            let hull = if self.corner_mode.is_90() { crate::hull::bbox_hull(&hull) } else { hull };
            let mut simplified = Line { arcs: self.arcs.clone(), ..Line::from_points(net.clone(), layer, width, std::mem::take(pts)) };
            simplified.simplify2(); // `aLine.Line().Simplify2()`
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

    fn single_step(&mut self, out: &mut WalkOutcome, net: &Net, layer: i32, width: Nm) {
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
    pub fn route(&mut self, net: &Net, layer: i32, width: Nm, initial: &[Point]) -> WalkOutcome {
        self.initial_length = chain_length(initial).as_f64();
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
                let length_factor = if self.initial_length > 0.0 { chain_length(&out.lines[pol]).as_f64() / self.initial_length } else { 0.0 };
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

/// `LINE_PLACER::clipAndCheckCollisions`: split `chain` at `p` (a point on it, to a micron), keep the part up to there, and
/// accept it when it is no shorter than `*threshold` and collides with nothing (`CheckColliding( LINE( m_head, l2 ) )`);
/// an accepted slice raises `*threshold` to its own length.
fn clip_and_check(p: Point, chain: &[Point], threshold: &mut Nm, out: &mut Vec<Point>, collides: &dyn Fn(&[Point]) -> bool) -> bool {
    // `SHAPE_LINE_CHAIN::Split`: an existing vertex, else the segment nearest `p` (within a micron)
    let slice: Vec<Point> = if let Some(i) = chain.iter().position(|&q| q == p) {
        chain[..=i].to_vec()
    } else {
        let Some(i) = (0..chain.len().saturating_sub(1)).filter(|&i| Seg::new(chain[i], chain[i + 1]).sq_distance_to_point(p) <= 2).min_by_key(|&i| Seg::new(chain[i], chain[i + 1]).sq_distance_to_point(p)) else { return false };
        let mut v = chain[..=i].to_vec();
        v.push(p);
        v
    };
    let dist = chain_length(&slice);
    let mut ok = true;
    if dist < *threshold {
        ok = false;
    }
    if collides(&slice) {
        ok = false;
    }
    if ok {
        *out = slice;
        *threshold = dist;
    }
    ok
}

/// `LINE_PLACER::cursorDistMinimum`: where along the walked `chain` (within `length_threshold` of its start) the cursor is
/// nearest, as the part of the chain up to there. The candidates are every vertex and the nearest point of every segment;
/// the nearest one is tried first, then -- if it collides -- all of them, longest slice that clears first.
fn cursor_dist_minimum(chain: &[Point], cursor: Point, length_threshold: f64, collides: &dyn Fn(&[Point]) -> bool) -> Option<Vec<Point>> {
    if chain.is_empty() {
        return None;
    }
    let norm = |a: Point, b: Point| Nm::kiround(dist_f(a, b));
    let mut last_p = *chain.last()?;
    let mut accumulated = Nm::ZERO;
    let mut cands: Vec<(Nm, Point)> = Vec::new();
    for w in chain.windows(2) {
        let seg = Seg::new(w[0], w[1]);
        cands.push((norm(cursor, seg.a), seg.a));
        let pn = seg.nearest_point(cursor);
        if pn != seg.a && pn != seg.b {
            cands.push((norm(pn, cursor), pn));
        }
        accumulated += norm(seg.a, seg.b);
        if accumulated.as_f64() > length_threshold {
            last_p = seg.b;
            break;
        }
    }
    cands.push((norm(cursor, last_p), last_p));
    // `minPGlob`: the first of the nearest (the local minimum is computed in KiCad and then discarded)
    let preferred = cands.iter().enumerate().min_by_key(|&(i, &(d, _))| (d, i)).map(|(_, &(_, p))| p)?;
    let mut out = Vec::new();
    let mut threshold = Nm::ZERO;
    if clip_and_check(preferred, chain, &mut threshold, &mut out, collides) {
        return Some(out);
    }
    threshold = Nm::ZERO;
    let mut ok = false;
    for &(_, p) in &cands {
        ok |= clip_and_check(p, chain, &mut threshold, &mut out, collides);
    }
    ok.then_some(out)
}

/// Does `pts`, as a track on `net` of `width`, touch anything in the node (`NODE::CheckColliding( &line )`, whatever the kind)?
pub fn path_collides(node: &Node, rules: &BoardRules, net: &Net, layer: i32, width: Nm, pts: &[Point], exclude: &[ItemId]) -> bool {
    pts.windows(2).any(|w| {
        let shape = Shape::Stadium { a: w[0], b: w[1], r: width / 2 };
        node.first_colliding(&shape, net, LayerRange::single(layer), rules, exclude).is_some()
    })
}

/// `LINE_PLACER::rhWalkBase` for a head that has no tail in this port: walk it around whatever its item mask finds in
/// the way, both windings (`{ WP_CCW, WP_CW }`), each finished walk merged by the optimizer's `MERGE_SEGMENTS` against
/// the *whole* world (`OPTIMIZER::Optimize( &line, MERGE_SEGMENTS, node )`, whose collision test ignores the kind mask),
/// and the shorter of the two taken -- unless even that detour is `hug_threshold` x 2 times the direct length or more
/// (`ROUTING_SETTINGS::WalkaroundHugLengthThreshold`, 1.5): then the head does not chase the cursor round the obstacle but
/// hugs it, ending at the point of the walk nearest the cursor within `hug_threshold` times the direct length
/// ([`cursor_dist_minimum`]). `None` when no winding got through or no point to hug to was clear (`rhWalkBase` returns
/// false). The cursor is the head's last point.
#[allow(clippy::too_many_arguments)] // the node/rules/net/layer/width query context, plus the head, the kind mask, the iteration limit and the hug threshold this is specific to.
pub fn walk_base(node: &Node, rules: &BoardRules, net: &Net, layer: i32, width: Nm, head: &[Point], mask: u8, iteration_limit: u32, hug_threshold: f64, exclude: &[ItemId]) -> Option<Vec<Point>> {
    let line = Line::from_points(net.clone(), layer, width, head.to_vec());
    walk_base_line(node, rules, &line, mask, iteration_limit, hug_threshold, exclude, crate::direction45::CornerMode::Mitered45).map(|l| l.pts)
}

/// [`walk_base`] for a head that may hold arcs (a rounded corner, [`Line::arcs`]), in the router's corner mode: the optimizer's merge builds its
/// bypasses in `corner` (`OPTIMIZER::mergeStep`), and the walked line keeps the arcs the walk left alone and those the merge made.
#[allow(clippy::too_many_arguments)] // walk_base's, with the head as a line and the corner mode
pub fn walk_base_line(node: &Node, rules: &BoardRules, head_line: &Line, mask: u8, iteration_limit: u32, hug_threshold: f64, exclude: &[ItemId], corner: crate::direction45::CornerMode) -> Option<Line> {
    let (net, layer, width) = (&head_line.net, head_line.layer, head_line.width);
    let head: &[Point] = &head_line.pts;
    let finish = |pts: Vec<Point>, arcs: Vec<crate::arcs::ArcPts>| {
        let mut l = Line::from_points(net.clone(), layer, width, pts);
        l.arcs = arcs;
        let kept = l.arc_runs().into_iter().map(|r| l.arcs[r.arc]).collect();
        l.arcs = kept;
        l
    };
    let cursor = *head.last()?;
    let mut w = Walker::new(node, rules);
    w.corner_mode = corner;
    w.item_mask = mask;
    w.iteration_limit = iteration_limit;
    w.exclude_items(exclude.iter().copied());
    w.set_allowed_policies(&[WalkPolicy::Ccw, WalkPolicy::Cw]);
    let wr = w.route(net, layer, width, head);

    let initial_length = chain_length(head).as_f64();
    let hug_len = initial_length * hug_threshold;
    let hug_len_complete = 2.0 * hug_len;

    // index 0 = clockwise, 1 = counter-clockwise (`WP_CW`, `WP_CCW`)
    let pols = [WalkPolicy::Cw, WalkPolicy::Ccw];
    let mut lines: [Vec<Point>; 2] = [wr.line_of(WalkPolicy::Cw).to_vec(), wr.line_of(WalkPolicy::Ccw).to_vec()];
    let mut arcs: [Vec<crate::arcs::ArcPts>; 2] = [head_line.arcs.clone(), head_line.arcs.clone()];
    let mut len = [Nm::MAX; 2];
    let mut best: Option<usize> = None;
    for (i, pol) in pols.iter().enumerate() {
        let status = wr.status_of(*pol);
        if status != WalkStatus::Stuck {
            len[i] = chain_length(&lines[i]);
        }
        if status == WalkStatus::Done {
            let line = finish(lines[i].clone(), arcs[i].clone());
            let constraints = crate::optimizer::Constraints { corner_mode: corner, ..Default::default() };
            let optimized = crate::optimizer::optimize_constrained(&line, node, rules, exclude, crate::optimizer::effort::MERGE_SEGMENTS, &constraints);
            (lines[i], arcs[i]) = (optimized.pts, optimized.arcs);
            len[i] = chain_length(&lines[i]);
            // `if( len_ccw < len_cw ) bestLine = ccw` -- CW wins ties
            if i == 0 || len[1] < len[0] {
                best = Some(i);
            }
        }
    }
    let best_length = len[0].min(len[1]);
    if let Some(b) = best {
        if best_length.as_f64() < hug_len_complete {
            return Some(finish(lines[b].clone(), arcs[b].clone()));
        }
    }

    // too long a detour: hug the obstacle to the point nearest the cursor
    let collides = |pts: &[Point]| path_collides(node, rules, net, layer, width, pts, exclude);
    let mut hugged: [Option<Vec<Point>>; 2] = [None, None];
    let mut dists = [Nm::MAX; 2];
    for (i, pol) in pols.iter().enumerate() {
        if wr.status_of(*pol) == WalkStatus::Stuck {
            continue;
        }
        hugged[i] = cursor_dist_minimum(&lines[i], cursor, hug_len, &collides);
        if let Some(l) = &hugged[i] {
            dists[i] = l.last().map_or(Nm::MAX, |&p| Nm::kiround(dist_f(cursor, p)));
        }
    }
    let pick = if dists[0] < dists[1] && hugged[0].is_some() { 0 } else { 1 };
    hugged[pick].take().map(|pts| finish(pts, arcs[pick].clone()))
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;
    use crate::item::{net_of, Item, Solid};

    fn rules() -> BoardRules {
        serde_yaml::from_str("track_width: 200000\nclearance: 200000\nvia_drill: 300000\nvia_diameter: 600000\n").unwrap()
    }

    #[test]
    fn straight_path_with_no_obstacle_is_unchanged() {
        let node = Node::new();
        let path = vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(5.0), y: Nm::ZERO }];
        assert_eq!(walk_base(&node, &rules(), &net_of("SIG"), 0, mm(0.2), &path, kind_mask::ANY, 10, 1.5, &[]).unwrap(), path);
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
            pos: Point { x: mm(2.475), y: mm(-1.905) },
            shape: Shape::RoundRect { x0: mm(2.475 - 0.975), y0: mm(-1.905 - 0.3), x1: mm(2.475 + 0.975), y1: mm(-1.905 + 0.3), r: mm(0.15) },
            source: "U1.8".into(),
            edge: false,
            rule: 0,
        }));
        let rules: BoardRules = serde_yaml::from_str("track_width: 200000\nclearance: 200000\nvia_drill: 300000\nvia_diameter: 600000\n").unwrap();
        let path = vec![Point { x: mm(-2.475), y: mm(-1.905) }, Point { x: mm(8.665), y: mm(-1.905) }, Point { x: mm(12.475), y: mm(1.905) }];
        let net = net_of("SIG");
        let best = walk_base(&node, &rules, &net, 0, mm(0.2), &path, kind_mask::ANY, 40, 1.5, &[]).expect("at least one winding must clear a single narrow pad");
        let best = &best;
        assert_eq!(best.first(), Some(&path[0]));
        assert_eq!(best.last(), Some(&path[2]));
        for w in best.windows(2) {
            let shape = Shape::Stadium { a: w[0], b: w[1], r: mm(0.1) };
            assert!(node.first_colliding(&shape, &net, LayerRange::single(0), &rules, &[]).is_none(), "leg {:?}-{:?} still collides", w[0], w[1]);
        }
    }

    #[test]
    fn walks_around_a_pad_blocking_the_direct_path() {
        let mut node = Node::new();
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: mm(2.5), y: Nm::ZERO }, shape: Shape::Circle { c: Point { x: mm(2.5), y: Nm::ZERO }, r: mm(0.5) }, source: "U1.1".into(), edge: false, rule: 0 }));
        let rules = rules();
        let path = vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(5.0), y: Nm::ZERO }];
        let best = walk_base(&node, &rules, &net_of("SIG"), 0, mm(0.2), &path, kind_mask::ANY, 10, 1.5, &[]).expect("at least one winding must succeed");
        let best = &best;
        assert_eq!(best.first(), Some(&Point { x: Nm::ZERO, y: Nm::ZERO }));
        assert_eq!(best.last(), Some(&Point { x: mm(5.0), y: Nm::ZERO }));
        assert!(best.len() > 2, "a real detour must add at least one vertex");
        // The walked path must actually clear the pad at the required
        // clearance -- check every leg against the node directly, the same
        // query the router itself would use.
        for w in best.windows(2) {
            let shape = Shape::Stadium { a: w[0], b: w[1], r: mm(0.1) };
            assert!(node.first_colliding(&shape, &net_of("SIG"), LayerRange::single(0), &rules, &[]).is_none(), "leg {:?}-{:?} still collides with the pad", w[0], w[1]);
        }
    }

    /// `WALKAROUND::processCluster` in a 90-degree corner mode walks round the hull's bounding box ("if( cornerMode == MITERED_90 || cornerMode
    /// == ROUNDED_90 ) hull = BBox"): the way round a round pad is axis legs, where the 45-degree modes go round its octagon.
    #[test]
    fn a_walk_in_a_90_degree_mode_goes_round_the_box_of_the_hull() {
        use crate::direction45::CornerMode;
        let mut node = Node::new();
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: mm(2.5), y: Nm::ZERO }, shape: Shape::Circle { c: Point { x: mm(2.5), y: Nm::ZERO }, r: mm(0.5) }, source: "U1.1".into(), edge: false, rule: 0 }));
        let rules = rules();
        let head = Line::from_points(net_of("SIG"), 0, mm(0.2), vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(5.0), y: Nm::ZERO }]);
        let diagonal = |pts: &[Point]| pts.windows(2).any(|w| w[0].x != w[1].x && w[0].y != w[1].y);
        for (corner, diagonals) in [(CornerMode::Mitered45, true), (CornerMode::Mitered90, false), (CornerMode::Rounded90, false)] {
            let walked = walk_base_line(&node, &rules, &head, kind_mask::ANY, 10, 1.5, &[], corner).expect("a way round");
            assert_eq!((walked.first(), walked.last()), (Some(Point { x: Nm::ZERO, y: Nm::ZERO }), Some(Point { x: mm(5.0), y: Nm::ZERO })), "{corner:?}");
            assert_eq!(diagonal(&walked.pts), diagonals, "{corner:?}: {:?}", walked.pts);
            assert!(all_clear(&node, &walked.pts, mm(0.2), &rules), "{corner:?}: {:?}", walked.pts);
        }
    }

    fn walker_pad(node: &mut Node, net: &str, c: Point, r: Nm) -> ItemId {
        node.add(Item::Solid(Solid { net: net_of(net), layers: LayerRange::new(0, 1), pos: c, shape: Shape::Circle { c, r }, source: "P".into(), edge: false, rule: 0 }))
    }

    fn walker_track(node: &mut Node, net: &str, a: Point, b: Point, width: Nm) -> ItemId {
        node.add(Item::Segment(crate::item::Segment { net: net_of(net), layer: 0, a, b, width, source_track: None, locked: false }))
    }

    fn all_clear(node: &Node, pts: &[Point], width: Nm, rules: &BoardRules) -> bool {
        pts.windows(2).all(|w| node.first_colliding(&Shape::Stadium { a: w[0], b: w[1], r: width / 2 }, &net_of("SIG"), LayerRange::single(0), rules, &[]).is_none())
    }

    /// `WALKAROUND` with `{ WP_CCW, WP_CW }`: each winding gets its own path, on opposite sides.
    #[test]
    fn both_windings_get_their_own_path_around_a_pad() {
        let mut node = Node::new();
        walker_pad(&mut node, "GND", Point { x: mm(2.5), y: Nm::ZERO }, mm(0.5));
        let rules = rules();
        let mut w = Walker::new(&node, &rules);
        w.set_allowed_policies(&[WalkPolicy::Ccw, WalkPolicy::Cw]);
        let out = w.route(&net_of("SIG"), 0, mm(0.2), &[Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(5.0), y: Nm::ZERO }]);
        assert_eq!(out.status_of(WalkPolicy::Cw), WalkStatus::Done);
        assert_eq!(out.status_of(WalkPolicy::Ccw), WalkStatus::Done);
        let (cw, ccw) = (out.line_of(WalkPolicy::Cw), out.line_of(WalkPolicy::Ccw));
        assert!(cw.iter().any(|p| p.y < mm(-0.5)) && ccw.iter().any(|p| p.y > mm(0.5)) || cw.iter().any(|p| p.y > mm(0.5)) && ccw.iter().any(|p| p.y < mm(-0.5)), "one each way: {cw:?} / {ccw:?}");
        assert!(all_clear(&node, cw, mm(0.2), &rules) && all_clear(&node, ccw, mm(0.2), &rules));
    }

    /// `SetItemMask( SOLID_T )`: tracks are not obstacles for the solids-only walk.
    #[test]
    fn the_solids_only_walk_goes_through_a_track() {
        let mut node = Node::new();
        walker_track(&mut node, "GND", Point { x: mm(2.5), y: mm(-2.0) }, Point { x: mm(2.5), y: mm(2.0) }, mm(0.2));
        let rules = rules();
        let path = [Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(5.0), y: Nm::ZERO }];
        let mut solids = Walker::new(&node, &rules);
        solids.item_mask = kind_mask::SOLID;
        solids.set_allowed_policies(&[WalkPolicy::Cw]);
        assert_eq!(solids.route(&net_of("SIG"), 0, mm(0.2), &path).line_of(WalkPolicy::Cw), &path, "nothing to walk around");
        let mut any = Walker::new(&node, &rules);
        any.set_allowed_policies(&[WalkPolicy::Cw]);
        assert_ne!(any.route(&net_of("SIG"), 0, mm(0.2), &path).line_of(WalkPolicy::Cw), &path);
    }

    /// `AssembleCluster`: a pad and the track attached to it are one obstacle -- the path goes around the
    /// track's far end, not between the two.
    #[test]
    fn a_pad_and_its_attached_track_are_walked_around_together() {
        let mut node = Node::new();
        let pad = walker_pad(&mut node, "GND", Point { x: mm(2.5), y: Nm::ZERO }, mm(0.4));
        walker_track(&mut node, "GND", Point { x: mm(2.5), y: Nm::ZERO }, Point { x: mm(2.5), y: mm(2.5) }, mm(0.2));
        let rules = rules();
        let mut w = Walker::new(&node, &rules);
        w.item_mask = kind_mask::SOLID; // only the pad is "in the way" ...
        w.set_allowed_policies(&[WalkPolicy::Shortest]);
        let out = w.route(&net_of("SIG"), 0, mm(0.2), &[Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(5.0), y: Nm::ZERO }]);
        assert_eq!(out.status_of(WalkPolicy::Shortest), WalkStatus::Done);
        let path = out.line_of(WalkPolicy::Shortest);
        // ... but the walk hugs the whole blob: it takes the short way, under the pad, away from the track
        assert!(path.iter().all(|p| p.y <= Nm::ZERO || p.y < mm(0.3)), "{path:?}");
        assert!(path.iter().any(|p| p.y < mm(-0.4)), "{path:?}");
        assert!(all_clear(&node, path, mm(0.2), &rules));
        let _ = pad;
    }

    /// `RestrictToCluster`: only the cluster's items are obstacles.
    #[test]
    fn a_restricted_walk_ignores_everything_outside_the_cluster() {
        let mut node = Node::new();
        let near = walker_pad(&mut node, "GND", Point { x: mm(1.5), y: Nm::ZERO }, mm(0.3));
        walker_pad(&mut node, "VCC", Point { x: mm(3.5), y: Nm::ZERO }, mm(0.3));
        let rules = rules();
        let mut w = Walker::new(&node, &rules);
        w.restrict_to_cluster(&[near]);
        w.set_allowed_policies(&[WalkPolicy::Cw]);
        let out = w.route(&net_of("SIG"), 0, mm(0.2), &[Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(5.0), y: Nm::ZERO }]);
        let path = out.line_of(WalkPolicy::Cw);
        assert!(path.iter().any(|p| p.y.abs() > mm(0.3)), "went around the first pad: {path:?}");
        assert!(!all_clear(&node, path, mm(0.2), &rules), "the second pad was not looked at, so the path may touch it");
    }

    /// `WP_SHORTEST` picks the shorter way round an off-centre pad.
    #[test]
    fn shortest_takes_the_shorter_side() {
        let mut node = Node::new();
        walker_pad(&mut node, "GND", Point { x: mm(2.5), y: mm(0.15) }, mm(0.4)); // the pad sits a little below the path (y grows downward)
        let rules = rules();
        let mut w = Walker::new(&node, &rules);
        w.set_allowed_policies(&[WalkPolicy::Shortest, WalkPolicy::Cw, WalkPolicy::Ccw]);
        let out = w.route(&net_of("SIG"), 0, mm(0.2), &[Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(5.0), y: Nm::ZERO }]);
        assert_eq!(out.status_of(WalkPolicy::Shortest), WalkStatus::Done);
        let shortest = chain_length(out.line_of(WalkPolicy::Shortest));
        assert_eq!(shortest, chain_length(out.line_of(WalkPolicy::Cw)).min(chain_length(out.line_of(WalkPolicy::Ccw))));
        assert!(out.line_of(WalkPolicy::Shortest).iter().any(|p| p.y < mm(-0.4)), "passes on the side away from the pad's centre");
    }

    /// `m_lengthExpansionFactor`: a walk that grows past ten times the direct length is abandoned.
    #[test]
    fn a_walk_that_runs_away_is_cut_off() {
        let mut node = Node::new();
        walker_pad(&mut node, "GND", Point { x: mm(2.0), y: Nm::ZERO }, mm(0.3));
        let rules = rules();
        let mut w = Walker::new(&node, &rules);
        w.length_expansion_factor = 1.01;
        w.set_allowed_policies(&[WalkPolicy::Cw]);
        let out = w.route(&net_of("SIG"), 0, mm(0.2), &[Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(4.0), y: Nm::ZERO }]);
        assert_eq!(out.status_of(WalkPolicy::Cw), WalkStatus::AlmostDone);
    }

    /// `rhWalkBase` with the solids mask: a head with a pad on it gets walked, one without is returned as it is.
    #[test]
    fn walk_base_returns_the_head_untouched_when_no_pad_is_in_the_way() {
        let mut node = Node::new();
        walker_pad(&mut node, "GND", Point { x: mm(2.5), y: mm(5.0) }, mm(0.4));
        let rules = rules();
        let head = [Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(5.0), y: Nm::ZERO }];
        assert_eq!(walk_base(&node, &rules, &net_of("SIG"), 0, mm(0.2), &head, kind_mask::SOLID, 40, 1.5, &[]).unwrap(), head);
        let mut blocked = Node::new();
        walker_pad(&mut blocked, "GND", Point { x: mm(2.5), y: Nm::ZERO }, mm(0.4));
        let walked = walk_base(&blocked, &rules, &net_of("SIG"), 0, mm(0.2), &head, kind_mask::SOLID, 40, 1.5, &[]).unwrap();
        assert!(walked.len() > 2 && all_clear(&blocked, &walked, mm(0.2), &rules));
    }

    #[test]
    fn starting_inside_the_hull_fails() {
        let hull = crate::hull::hull_of(&Shape::Circle { c: Point { x: Nm::ZERO, y: Nm::ZERO }, r: mm(1.0) }, Nm::ZERO, Nm::ZERO);
        let path = vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(5.0), y: Nm::ZERO }];
        assert!(walk_around_hull(&path, &hull, true).is_none());
    }
}
