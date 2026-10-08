//! Port of `PNS::SHOVE` (`pcbnew/router/pns_shove.{h,cpp}`): instead of
//! stopping at (MarkObstacles) or routing around (Walkaround) an obstacle,
//! push it -- and transitively, anything *it* then collides with -- out of
//! the way.
//!
//! The control flow is KiCad's, function for function:
//!
//! - A **line stack** (`m_lineStack`) holds the lines whose collisions are
//!   still to be resolved; the head sits at the bottom with rank 100000 and
//!   every line it shoves gets rank - 1 ([`Shove::shove_iteration`]).
//! - Each iteration asks the node for the **nearest obstacle** of the line on
//!   top, searching pads first, then vias, then tracks
//!   (`NODE::NearestObstacle`, nearest by path length to the hull crossing,
//!   [`crate::node::Node::nearest_obstacle`]) -- D12.
//! - A track is pushed with `ShoveObstacleLine` ([`Shove::shove_obstacle_line`]):
//!   one hull per pusher segment, re-walk the obstacle along it, four
//!   traversal-order x winding attempts, three hull sizes.
//! - A **pad** cannot be pushed: `onCollidingSolid` walks the *current line*
//!   around the cluster of items touching the pad (`TOPOLOGY::AssembleCluster`,
//!   `WALKAROUND` with `RestrictToCluster` and `WP_SHORTEST`,
//!   [`crate::walkaround::Walker`]) and carries on -- D5. The same walk is the
//!   fallback for a locked track (`SH_TRY_WALK`) and for a via that may not
//!   move.
//! - A **via** is pushed by the minimum translation vector of its shape
//!   against the pusher, epsilon-free (`onCollidingVia`), and its attached
//!   tracks are re-shaped with a 45-degree `DragCorner` (`pushOrShoveVia`) --
//!   D6. A via that lands on an existing joint is pushed on until it does not.
//! - A line that runs into something already shoved *with a higher rank* does
//!   not shove it back (the ping-pong guard): it is the current line that gets
//!   pushed (`onCollidingLine` with the roles swapped, `onReverseCollidingVia`).
//! - After the loop every shoved line is **optimized** in the shoved world
//!   (`runOptimizer`: two `MERGE_SEGMENTS` passes, `SMART_PADS`, restricted to
//!   the area that changed), and the head is optimized by the caller against
//!   the world this returns -- D10/D11.
//!
//! What differs, on purpose:
//!
//! - **No springback stack, no time limit.** KiCad keeps the branches of
//!   earlier shoves so an interactive drag can un-shove cheaply; this port is
//!   driven one HTTP request per mouse sample and re-runs from the committed
//!   world every time, so there is nothing to pop. The 250-iteration cap
//!   (`ShoveIterationLimit`) is the only bound (no `ShoveTimeLimit`).
//! - **Widths survive.** `SHOVE::assembleLine` calls `AssembleLine` with the
//!   default `aAllowSegmentSizeMismatch = true`, so KiCad assembles through a
//!   change of track width and puts the whole chain back at the width of the
//!   segment that was hit. Here a line stops at a width change
//!   ([`crate::node::AssembleOpts`]): the shoved line keeps its width and the
//!   neighbour of another width is its own line, pinned at the shared joint.
//! - **Commit mapping.** KiCad commits the node diff; this port's IR tracks
//!   are whole polylines (or, for an imported board, one track per segment),
//!   so the outcome names every IR track a shoved line stood on (and re-adds
//!   the part of such a track the line did not cover). See [`ShoveOutcome`].
//! - **A via exactly on the pusher's centreline.** KiCad's MTV is zero there,
//!   the via is not moved, and the loop runs into the iteration limit and
//!   fails. This port pushes it along the pusher's normal instead (the
//!   stitching-via case the old tests cover).
//! - `SHP_REVERSED` (drag by the first vertex), `SHP_IGNORE`, joint locks
//!   (`LockJoint`: nothing here shoves a line of the head's own net), arcs and
//!   holes are not modelled.

use crate::item::{same_net, Item, ItemId, Kind, Net, Via};
use crate::line::Line;
use crate::node::{kind_bit, AssembleOpts, NearestHit, Node, QueryOpts};
use crate::settings::{OptEffort, RoutingSettings};
use crate::walkaround::{self, WalkPolicy, WalkStatus, Walker};
use eda_drc::kimath::{isqrt, Seg, Shape};
use eda_model::ir::{Point, Um};
use eda_model::BoardRules;
use std::collections::{BTreeMap, HashMap, HashSet};

/// An existing track this shove displaced. `source_track` is the original
/// `Track::id`; it is `None` only for a line that came from this same routing
/// session. A track can appear several times (a track a junction cuts into
/// several lines), and a line that stood on several tracks (an imported board
/// has one IR track per segment) is named once per track: the first carries the
/// new geometry, the others an empty `line` (fewer than two points), meaning
/// "remove this track, nothing replaces it".
#[derive(Debug, Clone)]
pub struct DisplacedLine {
    pub source_track: Option<String>,
    pub line: Line,
}

#[derive(Debug, Clone)]
pub struct DisplacedVia {
    pub source_via: String,
    pub pos: Point,
}

/// What a shove did.
#[derive(Debug, Clone)]
pub struct ShoveOutcome {
    /// The head as it ended up: the input when nothing was in the way, or the
    /// input walked around a pad (`onCollidingSolid` applied to the head).
    pub head: Vec<Point>,
    pub displaced_lines: Vec<DisplacedLine>,
    pub displaced_vias: Vec<DisplacedVia>,
    /// The world the shove left, with the head removed (`SHOVE::CurrentNode()`
    /// after `removeHeads`) -- what the head is optimized against.
    pub world: Node,
}

/// `SHOVE::SHOVE_STATUS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Ok,
    Null,
    Incomplete,
    TryWalk,
}

type Area = (Um, Um, Um, Um);

/// `SHOVE::Run`: `head.SetRank( 100000 )`.
const HEAD_RANK: i32 = 100_000;
/// `c_ENDPOINT_ON_HULL_THRESHOLD` (1000 nm) in this IR's µm.
const ENDPOINT_ON_HULL_THRESHOLD: f64 = 1.0;
/// `cHullFailureExpansionFactor` (1000 nm) in µm.
const HULL_FAILURE_EXPANSION: Um = 1;

// ----------------------------------------------------------------------------
// geometry helpers
// ----------------------------------------------------------------------------

/// `SHAPE_LINE_CHAIN::POINT_INSIDE_TRACKER` over the closed polygon
/// `obstacle + reverse(shoved)`: is `p` inside the area swept by the move?
fn point_inside_swept(p: Point, obstacle: &[Point], shoved: &[Point]) -> bool {
    let poly: Vec<Point> = obstacle.iter().copied().chain(shoved.iter().rev().copied()).collect();
    crate::walkaround::point_in_polygon(&poly, p)
}

/// `SHAPE_LINE_CHAIN::SelfIntersecting`: any two non-adjacent segments cross.
fn self_intersecting(pts: &[Point]) -> bool {
    let n = pts.len();
    for i in 0..n.saturating_sub(1) {
        for j in (i + 2)..n.saturating_sub(1) {
            let (a, b) = (Seg::new(pts[i], pts[i + 1]), Seg::new(pts[j], pts[j + 1]));
            if a.intersect(&b).is_some() {
                // Closed chains touch at the shared endpoint; this chain is open.
                return true;
            }
        }
    }
    false
}

/// `SHAPE_LINE_CHAIN::NearestPoint` on a closed hull.
fn hull_nearest(hull: &[Point], p: Point) -> Point {
    (0..hull.len())
        .map(|i| Seg::new(hull[i], hull[(i + 1) % hull.len()]).nearest_point(p))
        .min_by_key(|q| (q.x - p.x).pow(2) + (q.y - p.y).pow(2))
        .unwrap_or(p)
}

/// `VECTOR2I::EuclideanNorm()` of `b - a`: the rounded length.
fn norm(a: Point, b: Point) -> i64 {
    (((b.x - a.x) as f64).powi(2) + ((b.y - a.y) as f64).powi(2)).sqrt().round() as i64
}

fn merge_area(a: Option<Area>, b: Area) -> Option<Area> {
    Some(match a {
        None => b,
        Some(a) => (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)),
    })
}

/// `LINE::ChangedArea` (`pns_line.cpp:1260`): the box around the vertices
/// where two versions of a line differ, inflated by the wider width.
fn changed_area(a: &Line, b: &Line) -> Option<Area> {
    let (mut a, mut b) = (a.clone(), b.clone());
    a.simplify();
    b.simplify();
    let (np_self, np_other) = (a.pts.len(), b.pts.len());
    let n = np_self.min(np_other);
    let mut i_start: Option<usize> = None;
    for i in 0..n {
        let (p1, p2) = (a.pts[i], b.pts[i]);
        if p1 != p2 {
            if i != n - 1 {
                // `!s.Contains( p2 )`
                if Seg::new(a.pts[i], a.pts[i + 1]).sq_distance_to_point(p2) > 1 {
                    i_start = Some(i);
                    break;
                }
            } else {
                i_start = Some(i);
                break;
            }
        }
    }
    let (mut i_end_self, mut i_end_other) = (None, None);
    for i in 0..n {
        if a.pts[np_self - 1 - i] != b.pts[np_other - 1 - i] {
            i_end_self = Some(np_self - 1 - i);
            i_end_other = Some(np_other - 1 - i);
            break;
        }
    }
    let i_start = i_start.unwrap_or(n);
    let i_end_self = i_end_self.unwrap_or(np_self - 1);
    let i_end_other = i_end_other.unwrap_or(np_other - 1);
    let mut area: Option<Area> = None;
    for i in i_start..=i_end_self {
        if let Some(p) = a.pts.get(i) {
            area = merge_area(area, (p.x, p.y, p.x, p.y));
        }
    }
    for i in i_start..=i_end_other {
        if let Some(p) = b.pts.get(i) {
            area = merge_area(area, (p.x, p.y, p.x, p.y));
        }
    }
    let w = a.width.max(b.width);
    area.map(|(x0, y0, x1, y1)| (x0 - w, y0 - w, x1 + w, y1 + w))
}

/// `VIA::ChangedArea`: nothing if it did not move, else both shapes' boxes.
fn via_changed_area(a: &Via, b: &Via) -> Option<Area> {
    if a.pos == b.pos {
        return None;
    }
    let boxed = |v: &Via| (v.pos.x - v.diameter / 2, v.pos.y - v.diameter / 2, v.pos.x + v.diameter / 2, v.pos.y + v.diameter / 2);
    merge_area(Some(boxed(a)), boxed(b))
}

/// `pushoutForce( SHAPE_CIRCLE, SEG, clearance )` (`shape_collisions.cpp:158`):
/// the displacement that takes a circle out of `seg`'s clearance, found by
/// trying a growing correction until the circle really clears.
fn pushout_force(c: Point, r: Um, seg: Seg, clearance: Um) -> (i64, i64) {
    let nearest = seg.nearest_point(c);
    let dist = norm(nearest, c);
    let min_dist = clearance + r;
    if dist >= min_dist {
        return (0, 0);
    }
    let mut f = (0, 0);
    for corr in 0..5 {
        f = crate::hull::resize(c.x - nearest.x, c.y - nearest.y, (min_dist - dist + corr) as Um);
        let moved = Point { x: c.x + f.0, y: c.y + f.1 };
        if isqrt(seg.sq_distance_to_point(moved)) >= min_dist {
            break;
        }
    }
    f
}

/// `Collide( SHAPE_CIRCLE, SHAPE_LINE_CHAIN_BASE, clearance, .., &mtv )` for an
/// open chain (`shape_collisions.cpp:182`): `Some(mtv)` when the circle is
/// within `clearance` of the chain, `mtv` being the sum of the push-outs of
/// the circle out of each segment in turn.
fn circle_chain_mtv(c: Point, r: Um, chain: &[Point], clearance: Um) -> Option<(i64, i64)> {
    let reach = (r as i128 + clearance as i128).max(0);
    let hit = chain.windows(2).any(|w| {
        let d = Seg::new(w[0], w[1]).sq_distance_to_point(c);
        d == 0 || d < reach * reach
    });
    if !hit {
        return None;
    }
    let (mut moved, mut total) = (c, (0i64, 0i64));
    for w in chain.windows(2) {
        let f = pushout_force(moved, r, Seg::new(w[0], w[1]), clearance);
        moved = Point { x: moved.x + f.0 as Um, y: moved.y + f.1 as Um };
        total = (total.0 + f.0, total.1 + f.1);
    }
    Some(total)
}

/// `Collide( SHAPE_CIRCLE, SHAPE_CIRCLE, clearance, .., &mtv )` (`:43`): the
/// vector from `a` to `b` resized to what is missing, plus the "apparent
/// rounding error" 3.
fn circle_circle_mtv(a: Point, ra: Um, b: Point, rb: Um, clearance: Um) -> Option<(i64, i64)> {
    let min_dist = clearance + ra + rb;
    let (dx, dy) = ((b.x - a.x) as i128, (b.y - a.y) as i128);
    let dist_sq = dx * dx + dy * dy;
    if dist_sq == 0 || dist_sq < (min_dist as i128) * (min_dist as i128) {
        let len = (min_dist as f64 - (dist_sq as f64).sqrt() + 3.0) as Um;
        let m = crate::hull::resize(b.x - a.x, b.y - a.y, len);
        return Some((m.0 as i64, m.1 as i64));
    }
    None
}

/// `Collide( SHAPE_RECT, SHAPE_CIRCLE, clearance, .., &mtv )` (`:70`), applied
/// to whatever shape a pad has: away from the pad's nearest boundary point
/// (towards the inside's far side when the via is inside it).
fn solid_via_mtv(shape: &Shape, c: Point, r: Um, clearance: Um) -> Option<(i64, i64)> {
    shape.collides(&Shape::Circle { c, r }, clearance)?;
    let (mut best, mut nearest) = (i128::MAX, c);
    for s in shape.boundary_segs() {
        let d = s.sq_distance_to_point(c);
        if d < best {
            best = d;
            nearest = s.nearest_point(c);
        }
    }
    let inside = best > 0 && shape.collides(&Shape::Circle { c, r: 0 }, 0).is_some();
    let dist = (best as f64).sqrt() + shape.radius() as f64 * if inside { -1.0 } else { 0.0 };
    let min_dist = clearance as f64 + r as f64;
    let (delta, len) = if inside { ((nearest.x - c.x, nearest.y - c.y), ((min_dist + 1.0 + dist).abs() + 1.0) as Um) } else { ((c.x - nearest.x, c.y - nearest.y), ((min_dist + 1.0 - dist).abs() + 1.0) as Um) };
    let m = crate::hull::resize(delta.0, delta.1, len);
    Some((m.0 as i64, m.1 as i64))
}

// ----------------------------------------------------------------------------
// the shove's own state
// ----------------------------------------------------------------------------

/// A via a [`SLine`] ends in: `LINE::m_via`. `linked` is the node item when it
/// was attached with `LinkVia` (a "tadpole": the via is part of the line's links),
/// `None` for the detached copy `AppendVia` makes.
#[derive(Debug, Clone)]
struct LineVia {
    data: Via,
    linked: Option<ItemId>,
}

/// `PNS::LINE`: the line of a [`Line`] (net, layer, width, points, and the
/// segments of the node it is assembled from as `segment_ids`) plus the via it
/// ends in and its rank (`m_rank`, the value an unlinked line carries).
#[derive(Debug, Clone)]
struct SLine {
    line: Line,
    via: Option<LineVia>,
    rank: i32,
}

impl SLine {
    fn new(line: Line) -> Self {
        SLine { line, via: None, rank: -1 }
    }

    /// `LINK_HOLDER::IsLinked`.
    fn is_linked(&self) -> bool {
        !self.line.segment_ids.is_empty() || self.via.as_ref().is_some_and(|v| v.linked.is_some())
    }

    /// `ContainsLink`.
    fn contains_link(&self, id: ItemId) -> bool {
        self.line.segment_ids.contains(&id) || self.via.as_ref().is_some_and(|v| v.linked == Some(id))
    }

    fn seg_count(&self) -> usize {
        self.line.segment_count()
    }

    /// `ClearLinks`: the geometry and the via survive, the node bookkeeping does not.
    fn clear_links(&mut self) {
        self.line.segment_ids.clear();
        if let Some(v) = &mut self.via {
            v.linked = None;
        }
    }

    /// Every linked item: the segments, then the via if linked.
    fn links(&self) -> Vec<ItemId> {
        let mut l = self.line.segment_ids.clone();
        if let Some(id) = self.via.as_ref().and_then(|v| v.linked) {
            l.push(id);
        }
        l
    }

    /// `LINE::LinkVia`: the via goes at the end of the line.
    fn link_via(&mut self, id: ItemId, data: Via) {
        if self.line.pts.len() > 1 && data.pos == self.line.pts[0] {
            self.line.reverse();
        }
        self.via = Some(LineVia { data, linked: Some(id) });
    }

    /// `LINE::AppendVia`: a detached copy of the via at the end.
    fn append_via(&mut self, data: Via) {
        if self.line.pts.len() > 1 && data.pos == self.line.pts[0] {
            self.line.reverse();
        }
        self.via = Some(LineVia { data, linked: None });
    }
}

/// `SHOVE::ROOT_LINE_ENTRY`: the pre-shove version of a line (or via) and the
/// latest one.
#[derive(Debug, Clone, Default)]
struct RootEntry {
    root_line: Option<SLine>,
    new_line: Option<SLine>,
    is_head: bool,
    old_via: Option<(ItemId, Via)>,
    new_via: Option<Via>,
}

/// `OBSTACLE`, as the iteration passes it on: the item, and the width of the
/// widest track fanning out of a via (`m_maxFanoutWidth`).
#[derive(Debug, Clone)]
struct ObstacleInfo {
    id: ItemId,
    kind: Kind,
    max_fanout_width: Um,
}

/// The first argument of `onCollidingVia`: what pushes the via.
enum Pusher<'p> {
    Line(&'p SLine),
    Item(ItemId),
}

struct Shove<'a> {
    /// The pristine world, to look original segments up (their IR tracks).
    world: &'a Node,
    /// `m_currentNode`.
    node: Node,
    rules: &'a BoardRules,
    settings: &'a RoutingSettings,
    /// `ITEM::m_rank`: absent = -1.
    ranks: HashMap<ItemId, i32>,
    /// `MK_HEAD`: the head's own segments, which never join a cluster.
    head_marked: HashSet<ItemId>,
    /// `m_lineStack` / `m_optimizerQueue`.
    line_stack: Vec<SLine>,
    optimizer_queue: Vec<SLine>,
    /// `m_rootLineHistory`.
    roots: Vec<RootEntry>,
    root_of: HashMap<ItemId, usize>,
    /// `m_affectedArea`.
    affected: Option<Area>,
    /// Vias taken out of the node, so a line that still ends in one can be read.
    via_archive: HashMap<ItemId, Via>,
    /// `m_iter`.
    iter: i32,
}

impl<'a> Shove<'a> {
    fn new(world: &'a Node, rules: &'a BoardRules, settings: &'a RoutingSettings) -> Self {
        Shove { world, node: world.branch(), rules, settings, ranks: HashMap::new(), head_marked: HashSet::new(), line_stack: Vec::new(), optimizer_queue: Vec::new(), roots: Vec::new(), root_of: HashMap::new(), affected: None, via_archive: HashMap::new(), iter: 0 }
    }

    fn clearance(&self, a: &Net, b: &Net) -> Um {
        Node::clearance(self.rules, a, b)
    }

    fn rank_of_item(&self, id: ItemId) -> i32 {
        self.ranks.get(&id).copied().unwrap_or(-1)
    }

    /// `LINE::Rank`: the lowest rank among the links, `m_rank` when unlinked.
    fn line_rank(&self, l: &SLine) -> i32 {
        if l.is_linked() {
            l.links().into_iter().map(|id| self.rank_of_item(id)).min().unwrap_or(-1)
        } else {
            l.rank
        }
    }

    /// `LINE::SetRank`: the line's own and every link's.
    fn set_line_rank(&mut self, l: &mut SLine, rank: i32) {
        l.rank = rank;
        for id in l.links() {
            self.ranks.insert(id, rank);
        }
    }

    fn via_item(&self, id: ItemId) -> Option<Via> {
        match self.node.get(id) {
            Some(Item::Via(v)) => Some(v.clone()),
            _ => self.via_archive.get(&id).cloned(),
        }
    }

    /// `JOINT::Via()` of the joint at `pos` on `net`.
    fn joint_via(&self, pos: Point, net: &Net) -> Option<ItemId> {
        self.node.joint_at(pos, net)?.links.iter().copied().find(|&id| matches!(self.node.get(id), Some(Item::Via(_))))
    }

    fn has_locked_segments(&self, l: &SLine) -> bool {
        l.line.segment_ids.iter().any(|&id| matches!(self.node.get(id), Some(Item::Segment(s)) if s.locked))
    }

    // ------------------------------------------------------------ assembling

    /// `SHOVE::assembleLine`: a line of one width, stopping in front of a locked segment.
    fn assemble(&self, seg: ItemId) -> Option<SLine> {
        let mut line = self.node.assemble_line_with(seg, AssembleOpts { follow_locked_segments: false, allow_width_mismatch: false })?;
        // `NODE::AssembleLine` does not link vias: a line ends in a via only when
        // a push made it a tadpole.
        line.via_at_start = None;
        line.via_at_end = None;
        Some(SLine::new(line))
    }

    /// `assembleLine( .., aPreCleanup = true )` / `preShoveCleanup`: merge collinear
    /// points of the assembled line in the node first.
    fn assemble_pre_cleanup(&mut self, seg: ItemId) -> Option<SLine> {
        let cur = self.assemble(seg)?;
        let mut cleaned = cur.line.clone();
        cleaned.simplify();
        if cleaned.pts.len() != cur.line.pts.len() {
            let mut new = cur.clone();
            new.clear_links();
            new.line.pts = cleaned.pts;
            self.replace_line(&cur, &mut new, true);
            return Some(new);
        }
        Some(cur)
    }

    // ------------------------------------------------------------ node edits

    fn new_root(&mut self, entry: RootEntry) -> usize {
        self.roots.push(entry);
        self.roots.len() - 1
    }

    /// `SHOVE::replaceLine`: swap a line in the node for a new one, remembering the
    /// pre-shove version the first time (the "root line").
    fn replace_line(&mut self, old: &SLine, new: &mut SLine, include_in_changed_area: bool) -> usize {
        if include_in_changed_area {
            if let Some(a) = changed_area(&old.line, &new.line) {
                self.affected = merge_area(self.affected, a);
            }
        }
        // `aOld.Unlink( viaLink )`: replacing a line never removes its via.
        let old_segments = old.line.segment_ids.clone();
        let found = old_segments.iter().find_map(|id| self.root_of.get(id).copied());
        let idx = match found {
            Some(i) => i,
            None => {
                let mut root = old.clone();
                if let Some(v) = &mut root.via {
                    v.linked = None;
                }
                let i = self.new_root(RootEntry { root_line: Some(root), ..RootEntry::default() });
                for id in &old_segments {
                    self.root_of.insert(*id, i);
                }
                i
            }
        };
        self.node.remove_line_segments(&old.line);
        let ids = self.node.add_line(&new.line, None, false);
        new.line.segment_ids = ids;
        // the new segments carry the line's own rank (`SEGMENT( aLine, seg )`)
        for &id in &new.line.segment_ids {
            self.ranks.insert(id, new.rank);
            self.root_of.insert(id, idx);
        }
        self.roots[idx].new_line = Some(new.clone());
        idx
    }

    /// `SHOVE::replaceItems` for a via.
    fn replace_via(&mut self, old_id: ItemId, new_via: Via, rank: i32) -> ItemId {
        let old = self.via_item(old_id).expect("a via being replaced exists");
        if let Some(a) = via_changed_area(&old, &new_via) {
            self.affected = merge_area(self.affected, a);
        }
        let idx = match self.root_of.get(&old_id).copied() {
            Some(i) => i,
            None => {
                let i = self.new_root(RootEntry { old_via: Some((old_id, old.clone())), ..RootEntry::default() });
                self.root_of.insert(old_id, i);
                i
            }
        };
        self.roots[idx].new_via = Some(new_via.clone());
        self.via_archive.insert(old_id, old);
        self.node.remove(old_id);
        let new_id = self.node.add(Item::Via(new_via));
        self.ranks.insert(new_id, rank);
        self.root_of.insert(new_id, idx);
        new_id
    }

    // ------------------------------------------------------------ line stack

    /// `unwindLineStack( const LINKED_ITEM* )`: drop the lines that contain this link
    /// (a tadpole keeps its via when the link was a segment).
    fn unwind_link(&mut self, id: ItemId, is_via: bool) {
        let mut i = 0;
        while i < self.line_stack.len() {
            if self.line_stack[i].contains_link(id) {
                if self.line_stack[i].via.is_some() && !is_via {
                    // "if we have a tadpole in the stack, keep track of the via even if the
                    // parent line has been deleted"
                    let linked_via = self.line_stack[i].via.as_ref().and_then(|v| v.linked.map(|vid| (vid, v.data.clone())));
                    if let Some((vid, data)) = linked_via {
                        let l = &mut self.line_stack[i];
                        l.clear_links();
                        l.line.pts.clear();
                        l.link_via(vid, data);
                    }
                    i += 1;
                } else {
                    self.line_stack.remove(i);
                }
            } else {
                i += 1;
            }
        }
        if !is_via {
            self.optimizer_queue.retain(|l| !l.contains_link(id));
        }
    }

    /// `unwindLineStack( const ITEM* )` (what a plain `ITEM*` gets): only tracks count.
    fn unwind_item(&mut self, id: ItemId) {
        if matches!(self.node.get(id), Some(Item::Segment(_))) {
            self.unwind_link(id, false);
        }
    }

    /// `unwindLineStack( &line )`: every link in turn.
    fn unwind_line(&mut self, l: &SLine) {
        for &id in &l.line.segment_ids {
            self.unwind_link(id, false);
        }
        if let Some(vid) = l.via.as_ref().and_then(|v| v.linked) {
            self.unwind_link(vid, true);
        }
    }

    /// `pruneLineFromOptimizerQueue`.
    fn prune_optimizer_queue(&mut self, l: &SLine) {
        let segs = l.line.segment_ids.clone();
        self.optimizer_queue.retain(|q| !segs.iter().any(|&s| q.contains_link(s)));
    }

    /// `pushLineStack`.
    fn push_line_stack(&mut self, l: SLine, keep_current_on_top: bool) -> bool {
        if !l.is_linked() && l.seg_count() != 0 {
            return false;
        }
        if keep_current_on_top && !self.line_stack.is_empty() {
            let at = self.line_stack.len() - 1;
            self.line_stack.insert(at, l.clone());
        } else {
            self.line_stack.push(l.clone());
        }
        self.prune_optimizer_queue(&l);
        self.optimizer_queue.push(l);
        true
    }

    /// `popLineStack`.
    fn pop_line_stack(&mut self) {
        if let Some(l) = self.line_stack.pop() {
            self.prune_optimizer_queue(&l);
        }
    }

    // ------------------------------------------------------------ collisions between lines

    /// Does the via collide with the line (its tracks and its own via)?
    fn via_vs_line(&self, v: &Via, l: &SLine) -> bool {
        let circle = Shape::Circle { c: v.pos, r: v.diameter / 2 };
        if !same_net(&v.net, &l.line.net) && l.line.layer >= v.layers.start() && l.line.layer <= v.layers.end() {
            let clearance = self.clearance(&v.net, &l.line.net);
            if l.line.segs().any(|(a, b)| Shape::Stadium { a, b, r: l.line.width / 2 }.collides(&circle, clearance).is_some()) {
                return true;
            }
        }
        if let Some(lv) = &l.via {
            if !same_net(&v.net, &lv.data.net) {
                let clearance = self.clearance(&v.net, &lv.data.net);
                if (Shape::Circle { c: lv.data.pos, r: lv.data.diameter / 2 }).collides(&circle, clearance).is_some() {
                    return true;
                }
            }
        }
        false
    }

    /// `l.Collide( &cur, m_currentNode, layer )` for two lines: track against
    /// track, and either line's via against the other.
    fn lines_collide(&self, l: &SLine, cur: &SLine) -> bool {
        if !same_net(&l.line.net, &cur.line.net) {
            let clearance = self.clearance(&l.line.net, &cur.line.net);
            let hit = l.line.segs().any(|(a, b)| cur.line.segs().any(|(c, d)| Shape::Stadium { a, b, r: l.line.width / 2 }.collides(&Shape::Stadium { a: c, b: d, r: cur.line.width / 2 }, clearance).is_some()));
            if hit {
                return true;
            }
        }
        if let Some(v) = &cur.via {
            if self.via_vs_line(&v.data, l) {
                return true;
            }
        }
        if let Some(v) = &l.via {
            if self.via_vs_line(&v.data, cur) {
                return true;
            }
        }
        false
    }

    /// `ITEM::Collide` between two items of the node (different nets, shapes within clearance).
    fn items_collide(&self, a: ItemId, b: ItemId) -> bool {
        let (Some(ia), Some(ib)) = (self.node.get(a), self.node.get(b)) else { return false };
        if same_net(ia.net(), ib.net()) || !ia.layers().overlaps(&ib.layers()) {
            return false;
        }
        let clearance = self.clearance(ia.net(), ib.net());
        ia.shape(ia.layers().start()).collides(&ib.shape(ib.layers().start()), clearance).is_some()
    }

    // ------------------------------------------------------------ shoving a track

    /// `SHOVE::checkShoveDirection`: the obstacle must move *away* from the
    /// pusher, i.e. the pusher's start point is not inside the area between
    /// the obstacle's old and new shape.
    fn check_shove_direction(&self, cur: &SLine, obstacle: &Line, shoved: &Line) -> bool {
        let lone_via = cur.line.pts.is_empty() && cur.via.is_some();
        let cp = if lone_via { cur.via.as_ref().map(|v| v.data.pos) } else { cur.line.first() };
        let Some(cp) = cp else { return true };
        !point_inside_swept(cp, &obstacle.pts, &shoved.pts)
    }

    /// `SHOVE::shoveLineFromLoneVia`: push the obstacle off a via that has no
    /// track of its own on this layer -- walk it around the via's hull.
    fn shove_line_from_lone_via(&self, cur: &SLine, obstacle: &SLine, result: &mut SLine) -> bool {
        let Some(lv) = &cur.via else { return false };
        let via = &lv.data;
        let clearance = self.clearance(&via.net, &obstacle.line.net);
        let hull = Item::Via(via.clone()).hull(clearance, obstacle.line.width, cur.line.layer);
        let Some(path_cw) = walkaround::walk_around_hull(&obstacle.line.pts, &hull, true) else { return false };
        let Some(path_ccw) = walkaround::walk_around_hull(&obstacle.line.pts, &hull, false) else { return false };
        result.line.pts = path_cw;
        if !self.check_shove_direction(cur, &obstacle.line, &result.line) {
            result.line.pts = path_ccw;
        }
        if result.line.pts.len() < 2 {
            return false;
        }
        if obstacle.line.last() != result.line.last() || obstacle.line.first() != result.line.first() {
            return false;
        }
        !self.lines_collide(result, cur)
    }

    /// `SHOVE::shoveLineToHullSet`: re-walk `obstacle` around every hull in
    /// turn, trying the 4 combinations of hull traversal order x winding, and
    /// accept the first result that keeps both endpoints, moves away from the
    /// pusher, doesn't self-intersect and clears the pusher.
    fn shove_line_to_hull_set(&self, cur: &SLine, obstacle: &SLine, result: &mut SLine, hulls: &[Vec<Point>], adjust_start: bool, adjust_end: bool) -> bool {
        for attempt in 0..4 {
            let invert = attempt >= 2;
            let clockwise = attempt % 2 == 1;
            let order: Vec<usize> = if invert { (0..hulls.len()).rev().collect() } else { (0..hulls.len()).collect() };
            let mut obs = obstacle.line.pts.clone();

            if (adjust_start || adjust_end) && obs.len() >= 2 {
                let min_dist_p = |pref: Point| -> Option<(f64, Point)> {
                    let mut best: Option<(f64, Point)> = None;
                    for &i in &order {
                        let hull = &hulls[i];
                        let p = hull_nearest(hull, pref);
                        let d = if walkaround::point_in_polygon(hull, pref) { 0.0 } else { (((p.x - pref.x) as f64).powi(2) + ((p.y - pref.y) as f64).powi(2)).sqrt() };
                        if d < ENDPOINT_ON_HULL_THRESHOLD && best.is_none_or(|(bd, _)| d < bd) {
                            best = Some((d, p));
                        }
                    }
                    best
                };
                let p0 = min_dist_p(obs[0]);
                let p1 = min_dist_p(*obs.last().unwrap());
                if let (Some((_, p)), true) = (p1, adjust_end) {
                    obs.push(p);
                }
                if let (Some((_, p)), true) = (p0, adjust_start) {
                    obs.insert(0, p);
                }
            }

            let mut path = obs.clone();
            let mut fail = false;
            for &i in &order {
                match walkaround::walk_around_hull(&path, &hulls[i], clockwise) {
                    Some(p) => {
                        let mut l = Line::from_points(obstacle.line.net.clone(), obstacle.line.layer, obstacle.line.width, p);
                        l.simplify();
                        path = l.pts;
                    }
                    None => {
                        fail = true;
                        break;
                    }
                }
            }
            if fail || path.len() < 2 {
                continue;
            }
            if path.first() != obs.first() || path.last() != obs.last() {
                continue;
            }
            let shoved = Line::from_points(obstacle.line.net.clone(), obstacle.line.layer, obstacle.line.width, path.clone());
            if !self.check_shove_direction(cur, &obstacle.line, &shoved) {
                continue;
            }
            if self_intersecting(&path) {
                continue;
            }
            let candidate = SLine { line: shoved, via: None, rank: obstacle.rank };
            if self.lines_collide(&candidate, cur) {
                continue;
            }
            result.line.pts = path;
            return true;
        }
        false
    }

    /// `SHOVE::ShoveObstacleLine`: one hull per pusher segment at clearance +
    /// the obstacle's width, three tries with the hulls grown by 1 µm each
    /// time; the third try may also snap the obstacle's free (via-less) ends
    /// onto a hull. The pusher's end via gets a hull of its own.
    fn shove_obstacle_line(&self, cur: &SLine, obstacle: &SLine, result: &mut SLine) -> bool {
        let voe = |p: Option<Point>| p.is_some_and(|p| self.joint_via(p, &obstacle.line.net).is_some());
        let (voe_start, voe_end) = (voe(obstacle.line.first()), voe(obstacle.line.last()));
        result.clear_links();
        let via_on_end = cur.via.is_some();
        let mut obstacle_line = obstacle.clone();
        let obs_via = obstacle_line.via.take();
        result.via = None;
        if via_on_end && cur.seg_count() == 0 {
            let ok = self.shove_line_from_lone_via(cur, &obstacle_line, result);
            if ok {
                result.via = obs_via;
            }
            return ok;
        }
        let obstacle_width = obstacle_line.line.width;
        let clearance = self.clearance(&cur.line.net, &obstacle_line.line.net);
        for attempt in 0..3 {
            // the hulls grow by `cHullFailureExpansionFactor` after each failed attempt
            let extra = attempt * HULL_FAILURE_EXPANSION;
            let mut hulls: Vec<Vec<Point>> = cur.line.segs().map(|(a, b)| crate::hull::segment_hull(a, b, cur.line.width, clearance + extra + crate::hull::HULL_ROUNDING_GUARD, obstacle_width)).filter(|h| h.len() >= 3).collect();
            if let Some(lv) = &cur.via {
                let via_clearance = self.clearance(&lv.data.net, &obstacle_line.line.net);
                hulls.push(Item::Via(lv.data.clone()).hull(via_clearance, obstacle_width, obstacle_line.line.layer));
            }
            let (adj_start, adj_end) = (attempt >= 2 && !voe_start, attempt >= 2 && !voe_end);
            if self.shove_line_to_hull_set(cur, &obstacle_line, result, &hulls, adj_start, adj_end) {
                result.via = obs_via;
                return true;
            }
        }
        false
    }

    // ------------------------------------------------------------ the on_colliding_* family

    /// `SHOVE::onCollidingSegment`.
    fn on_colliding_segment(&mut self, cur: &SLine, seg: ItemId) -> Status {
        let Some(obstacle_line) = self.assemble_pre_cleanup(seg) else { return Status::Incomplete };
        let mut shoved = obstacle_line.clone();
        if self.has_locked_segments(&obstacle_line) {
            return Status::TryWalk;
        }
        if self.shove_obstacle_line(cur, &obstacle_line, &mut shoved) {
            let rank = self.line_rank(cur);
            shoved.rank = rank - 1;
            shoved.line.simplify();
            self.unwind_line(&obstacle_line);
            self.replace_line(&obstacle_line, &mut shoved, true);
            if !self.push_line_stack(shoved, false) {
                return Status::Incomplete;
            }
            return Status::Ok;
        }
        Status::Incomplete
    }

    /// `SHOVE::onCollidingLine`: shove `obstacle` away from `cur`.
    fn on_colliding_line(&mut self, cur: &SLine, obstacle: &SLine, next_rank: i32) -> Status {
        let mut shoved = obstacle.clone();
        if self.shove_obstacle_line(cur, obstacle, &mut shoved) {
            self.replace_line(obstacle, &mut shoved, true);
            self.set_line_rank(&mut shoved, next_rank);
            if !self.push_line_stack(shoved, false) {
                return Status::Incomplete;
            }
            return Status::Ok;
        }
        Status::Incomplete
    }

    /// `SHOVE::onCollidingSolid`: the pad cannot move, so the current line walks
    /// around the cluster of items touching it (`WALKAROUND`, `WP_SHORTEST`,
    /// restricted to that cluster) and replaces itself in the node.
    fn on_colliding_solid(&mut self, cur: &SLine, obstacle: ItemId, info: &ObstacleInfo) -> Status {
        if let Some(lv) = &cur.via {
            // the line ends in a via that collides with the obstacle: push the via instead
            let Some(via_id) = self.joint_via(lv.data.pos, &cur.line.net) else { return Status::Incomplete };
            if self.items_collide(via_id, obstacle) {
                let next = self.rank_of_item(obstacle) - 1;
                return self.on_colliding_via(Pusher::Item(obstacle), via_id, info, next);
            }
        }
        let current_rank = self.line_rank(cur);
        let mut skip: HashSet<ItemId> = self.head_marked.clone();
        skip.extend(cur.line.segment_ids.iter().copied());
        let walked: Option<(SLine, i32)> = {
            let cluster = self.node.assemble_cluster(obstacle, cur.line.layer, 10.0, None, &|id| skip.contains(&id));
            let mut walker = Walker::new(&self.node, self.rules);
            walker.restrict_to_cluster(&cluster);
            walker.skip_items(skip.iter().copied());
            walker.exclude_items(cur.links());
            walker.set_allowed_policies(&[WalkPolicy::Shortest]);
            walker.iteration_limit = self.settings.walkaround_iteration_limit.max(0) as u32;
            let mut found = None;
            for attempt in 0..2 {
                let next_rank = if attempt == 1 || self.settings.jump_over_obstacles { current_rank - 1 } else { current_rank + 10_000 };
                let outcome = walker.route(&cur.line.net, cur.line.layer, cur.line.width, &cur.line.pts);
                if outcome.status_of(WalkPolicy::Shortest) != WalkStatus::Done {
                    continue;
                }
                let mut walk_line = cur.clone();
                walk_line.clear_links();
                walk_line.line.pts = outcome.line_of(WalkPolicy::Shortest).to_vec();
                walk_line.line.simplify();
                if walk_line.line.has_loops() {
                    continue;
                }
                if let Some(last_line) = self.line_stack.first() {
                    if self.lines_collide(last_line, &walk_line) {
                        let mut dummy = last_line.clone();
                        if self.shove_obstacle_line(&walk_line, last_line, &mut dummy) {
                            found = Some((walk_line, next_rank));
                            break;
                        }
                    } else {
                        found = Some((walk_line, next_rank));
                        break;
                    }
                }
            }
            found
        };
        let Some((mut walk_line, next_rank)) = walked else { return Status::Incomplete };
        self.replace_line(cur, &mut walk_line, true);
        self.set_line_rank(&mut walk_line, next_rank);
        self.pop_line_stack();
        if !self.push_line_stack(walk_line, false) {
            return Status::Incomplete;
        }
        Status::Ok
    }

    /// `SHOVE::onCollidingVia`: the minimum translation vector that takes the via
    /// out of the pusher's clearance, epsilon-free, then `pushOrShoveVia`.
    fn on_colliding_via(&mut self, cur: Pusher, via_id: ItemId, info: &ObstacleInfo, next_rank: i32) -> Status {
        let Some(Item::Via(obstacle_via)) = self.node.get(via_id).cloned() else { return Status::Incomplete };
        let (mut mtv, mut collided) = ((0i64, 0i64), false);
        match &cur {
            Pusher::Line(line) => {
                let clearance = self.clearance(&line.line.net, &obstacle_via.net);
                let mut vtmp = obstacle_via.clone();
                if info.max_fanout_width > 0 && info.max_fanout_width > vtmp.diameter {
                    vtmp.diameter = info.max_fanout_width;
                }
                let line_mtv = circle_chain_mtv(vtmp.pos, vtmp.diameter / 2, &line.line.pts, clearance + line.line.width / 2);
                // "Check the via if present. Via takes priority."
                let via_mtv = line.via.as_ref().and_then(|lv| circle_circle_mtv(lv.data.pos, lv.data.diameter / 2, vtmp.pos, vtmp.diameter / 2, self.clearance(&lv.data.net, &vtmp.net)));
                collided = line_mtv.is_some() || via_mtv.is_some();
                mtv = via_mtv.or(line_mtv).unwrap_or((0, 0));
            }
            Pusher::Item(id) => {
                if let Some(Item::Solid(s)) = self.node.get(*id) {
                    let clearance = self.clearance(&s.net, &obstacle_via.net);
                    if let Some(m) = solid_via_mtv(&s.shape, obstacle_via.pos, obstacle_via.diameter / 2, clearance) {
                        mtv = m;
                        collided = true;
                    }
                }
            }
        }
        if collided && mtv == (0, 0) {
            mtv = self.degenerate_push(&cur, &obstacle_via);
        }
        self.push_or_shove_via(via_id, mtv, next_rank, false)
    }

    /// Where a via whose centre sits exactly on the pusher goes: KiCad's MTV is
    /// zero there (and the shove runs into its iteration limit); push it along
    /// the normal of the nearest pusher segment instead, far enough to clear.
    fn degenerate_push(&self, cur: &Pusher, via: &Via) -> (i64, i64) {
        let (dir, reach) = match cur {
            Pusher::Line(line) => {
                let clearance = self.clearance(&line.line.net, &via.net);
                let seg = line.line.segs().min_by_key(|&(a, b)| Seg::new(a, b).sq_distance_to_point(via.pos));
                let dir = seg.map(|(a, b)| (-(b.y - a.y), b.x - a.x)).filter(|d| *d != (0, 0)).unwrap_or((0, 1));
                (dir, clearance + line.line.width / 2 + via.diameter / 2 + 1)
            }
            Pusher::Item(_) => ((0, 1), via.diameter + 1),
        };
        let m = crate::hull::resize(dir.0, dir.1, reach);
        (m.0 as i64, m.1 as i64)
    }

    /// `SHOVE::pushOrShoveVia`: move the via by `force` (further, if that spot
    /// is taken by a joint), drag the ends of the tracks attached to it along
    /// with a 45-degree re-solve, and put the moved lines on the stack.
    fn push_or_shove_via(&mut self, via_id: ItemId, force: (i64, i64), new_rank: i32, dont_unwind: bool) -> Status {
        let Some(Item::Via(via)) = self.node.get(via_id).cloned() else { return Status::Incomplete };
        let p0 = via.pos;
        if force == (0, 0) {
            return Status::Ok; // nothing to do...
        }
        let mut p0_pushed = Point { x: p0.x + force.0 as Um, y: p0.y + force.1 as Um };
        let Some(joint) = self.node.joint_at(p0, &via.net) else { return Status::Incomplete };
        let fan_ids: Vec<ItemId> = joint.links.clone();
        if !self.settings.shove_vias || via.locked {
            return Status::TryWalk;
        }
        // "make sure pushed via does not overlap with any existing joint"
        let step = crate::hull::resize(force.0 as Um, force.1 as Um, 2);
        for _ in 0..10_000 {
            if self.node.joint_at(p0_pushed, &via.net).is_none() {
                break;
            }
            p0_pushed = Point { x: p0_pushed.x + step.0, y: p0_pushed.y + step.1 };
        }
        let pushed = Via { pos: p0_pushed, ..via.clone() };

        let mut dragged: Vec<(SLine, SLine)> = Vec::new();
        for id in fan_ids {
            if !matches!(self.node.get(id), Some(Item::Segment(_))) {
                continue;
            }
            let Some(mut first) = self.assemble(id) else { continue };
            if self.has_locked_segments(&first) {
                return Status::TryWalk;
            }
            // the via is at one end of the line; make it the last
            if first.line.segment_ids.first() == Some(&id) {
                first.line.reverse();
            }
            let mut second = first.clone();
            second.clear_links();
            if let Some(at) = second.line.find(p0) {
                second.line.drag_corner45(p0_pushed, at);
            }
            second.line.simplify();
            dragged.push((first, second));
        }

        if !dont_unwind {
            self.unwind_link(via_id, true);
        }
        let v2 = self.replace_via(via_id, pushed.clone(), new_rank);

        if dragged.is_empty() {
            // a stitching via: make sure the shove won't forget about it
            let mut tmp = SLine::new(Line::new(pushed.net.clone(), pushed.layers.start(), pushed.diameter));
            tmp.link_via(v2, pushed.clone());
            tmp.rank = new_rank;
            if !self.push_line_stack(tmp, false) {
                return Status::Incomplete;
            }
        }
        for (first, mut second) in dragged {
            if !dont_unwind {
                self.unwind_line(&first);
            }
            if second.seg_count() > 0 {
                second.clear_links();
                let root = self.replace_line(&first, &mut second, true);
                second.link_via(v2, pushed.clone());
                if !dont_unwind {
                    self.unwind_line(&second);
                }
                self.set_line_rank(&mut second, new_rank);
                self.roots[root].new_line = Some(second.clone());
                if !self.push_line_stack(second, false) {
                    return Status::Incomplete;
                }
            } else {
                self.node.remove_line_segments(&first.line);
            }
        }
        Status::Ok
    }

    /// `SHOVE::onReverseCollidingVia`: the current line ran into a via that was
    /// already shoved with a higher rank -- it is the line that has to yield:
    /// every track fanning out of the via, with the via, shoves the current line.
    fn on_reverse_colliding_via(&mut self, cur: &SLine, via_id: ItemId, info: &ObstacleInfo) -> Status {
        let Some(obstacle_via) = self.via_item(via_id) else { return Status::Incomplete };
        if let Some(lv) = &cur.via {
            let clearance = self.clearance(&lv.data.net, &obstacle_via.net);
            let hit = !same_net(&lv.data.net, &obstacle_via.net) && Shape::Circle { c: lv.data.pos, r: lv.data.diameter / 2 }.collides(&Shape::Circle { c: obstacle_via.pos, r: obstacle_via.diameter / 2 }, clearance).is_some();
            if hit {
                let next = self.line_rank(cur) - 1;
                return self.on_colliding_via(Pusher::Line(cur), via_id, info, next);
            }
        }
        // `cur`: the current line as the obstacle, without links or via
        let mut working = cur.clone();
        working.clear_links();
        working.via = None;
        let mut shoved = cur.clone();
        shoved.clear_links();
        self.unwind_line(cur);

        let fan_ids: Vec<ItemId> = self.node.joint_at(obstacle_via.pos, &obstacle_via.net).map(|j| j.links.clone()).unwrap_or_default();
        let mut n = 0;
        for id in fan_ids {
            if !matches!(self.node.get(id), Some(Item::Segment(s)) if s.layer == cur.line.layer) {
                continue;
            }
            let Some(mut head) = self.assemble(id) else { continue };
            head.append_via(obstacle_via.clone());
            if !self.shove_obstacle_line(&head, &working, &mut shoved) {
                return Status::Incomplete;
            }
            working.line.pts = shoved.line.pts.clone();
            n += 1;
        }
        if n == 0 {
            // a lone via: nothing of its own on this layer
            let mut head = cur.clone();
            head.line.pts.clear();
            head.clear_links();
            head.append_via(obstacle_via.clone());
            if !self.shove_obstacle_line(&head, cur, &mut shoved) {
                return Status::Incomplete;
            }
            working.line.pts = shoved.line.pts.clone();
        }
        if let Some(lv) = &cur.via {
            shoved.append_via(lv.data.clone());
        }
        let current_rank = self.line_rank(cur);
        self.unwind_line(cur);
        self.replace_line(cur, &mut shoved, true);
        if !self.push_line_stack(shoved.clone(), false) {
            return Status::Incomplete;
        }
        self.set_line_rank(&mut shoved, current_rank);
        Status::Ok
    }

    /// `SHOVE::patchTadpoleVia`: a line that ends on a colliding via takes the via along.
    fn patch_tadpole_via(&self, current: &mut SLine) {
        let Some(last) = current.line.last() else { return };
        let Some(via_id) = self.joint_via(last, &current.line.net) else { return };
        let Some(Item::Via(v)) = self.node.get(via_id) else { return };
        let colliding = self.via_colliding_world(via_id);
        if current.via.is_none() && colliding {
            current.link_via(via_id, v.clone());
        }
    }

    /// `m_currentNode->CheckColliding( viaEnd )`.
    fn via_colliding_world(&self, via_id: ItemId) -> bool {
        let Some(Item::Via(v)) = self.node.get(via_id) else { return false };
        let shape = Shape::Circle { c: v.pos, r: v.diameter / 2 };
        self.node.first_colliding(&shape, &v.net, v.layers, self.rules, &[via_id]).is_some()
    }

    /// `SHOVE::fixupViaCollisions`: a via whose fan-out tracks are as wide as it is
    /// cannot be force-propagated; and a track narrower than its via-end is
    /// treated as that via.
    fn fixup_via_collisions(&self, cur: &SLine, obs: &mut ObstacleInfo) -> bool {
        match self.node.get(obs.id) {
            Some(Item::Via(v)) => {
                let mut maxw = 0;
                if let Some(j) = self.node.joint_at(v.pos, &v.net) {
                    for &lid in &j.links {
                        if let Some(Item::Segment(s)) = self.node.get(lid) {
                            maxw = maxw.max(s.width);
                        }
                    }
                }
                obs.max_fanout_width = 0;
                if maxw > 0 && maxw >= v.diameter {
                    obs.max_fanout_width = maxw + 1;
                    return true;
                }
                false
            }
            Some(Item::Segment(s)) => {
                for end in [s.a, s.b] {
                    let Some(via_id) = self.joint_via(end, &s.net) else { continue };
                    let Some(Item::Via(v)) = self.node.get(via_id) else { continue };
                    if v.diameter > s.width {
                        continue;
                    }
                    let mut vtest = v.clone();
                    vtest.diameter = s.width;
                    if self.via_vs_line(&vtest, cur) {
                        obs.id = via_id;
                        obs.kind = Kind::Via;
                        obs.max_fanout_width = s.width + 1;
                        return true;
                    }
                }
                false
            }
            _ => false,
        }
    }

    // ------------------------------------------------------------ the loop

    fn nearest_for(&self, cur: &SLine, kind: Kind) -> Option<NearestHit> {
        let exclude = cur.links();
        let opts = QueryOpts { kind_mask: kind_bit(kind), filter: None, use_epsilon: true };
        self.node.nearest_obstacle(&cur.line, cur.via.as_ref().map(|v| &v.data), self.rules, &exclude, opts)
    }

    /// `SHOVE::shoveIteration`: resolve the next collision of the line on top.
    fn shove_iteration(&mut self) -> Status {
        let Some(mut current) = self.line_stack.last().cloned() else { return Status::Ok };
        let mut nearest = None;
        for kind in [Kind::Solid, Kind::Via, Kind::Segment] {
            nearest = self.nearest_for(&current, kind);
            if nearest.is_some() {
                break;
            }
        }
        let Some(hit) = nearest else {
            self.line_stack.pop();
            return Status::Ok;
        };
        let mut info = ObstacleInfo { id: hit.id, kind: hit.kind, max_fanout_width: 0 };
        self.fixup_via_collisions(&current, &mut info);
        let ni = info.id;
        self.unwind_item(ni);

        let ni_rank = self.rank_of_item(ni);
        let cur_rank = self.line_rank(&current);
        if info.kind != Kind::Solid && ni_rank >= 0 && ni_rank > cur_rank {
            // collision with a higher-ranking object (one that was already shoved)
            match info.kind {
                Kind::Via => {
                    self.patch_tadpole_via(&mut current);
                    // does the obstacle via itself collide with the via the current line ends in?
                    let mut vias_collide = false;
                    if let (Some(lv), Some(Item::Via(nv))) = (&current.via, self.node.get(ni)) {
                        let clearance = self.clearance(&nv.net, &lv.data.net);
                        vias_collide = !same_net(&nv.net, &lv.data.net) && Shape::Circle { c: nv.pos, r: nv.diameter / 2 }.collides(&Shape::Circle { c: lv.data.pos, r: lv.data.diameter / 2 }, clearance).is_some();
                    }
                    if vias_collide {
                        self.on_colliding_via(Pusher::Line(&current), ni, &info, ni_rank + 1)
                    } else {
                        self.on_reverse_colliding_via(&current, ni, &info)
                    }
                }
                Kind::Segment => {
                    let Some(rev_line) = self.assemble(ni) else { return Status::Incomplete };
                    self.pop_line_stack();
                    self.unwind_line(&rev_line);
                    self.patch_tadpole_via(&mut current);
                    let st = if current.via.as_ref().is_some_and(|lv| self.via_vs_line(&lv.data, &rev_line)) {
                        // `FindViaByHandle`: the via the current line ends in
                        let handle = current.via.as_ref().map(|lv| (lv.data.pos, lv.data.net.clone()));
                        let rvia = handle.and_then(|(pos, net)| self.joint_via(pos, &net));
                        match rvia {
                            None => Status::Incomplete,
                            Some(rv) => {
                                let next = self.line_rank(&rev_line) + 1;
                                self.on_colliding_via(Pusher::Line(&rev_line), rv, &info, next)
                            }
                        }
                    } else {
                        let next = self.line_rank(&rev_line) + 1;
                        self.on_colliding_line(&rev_line, &current, next)
                    };
                    if !self.push_line_stack(rev_line, false) {
                        return Status::Incomplete;
                    }
                    st
                }
                Kind::Solid => Status::Null,
            }
        } else {
            // collision with a lower-ranking object, or a solid
            match info.kind {
                Kind::Segment => {
                    let mut st = self.on_colliding_segment(&current, ni);
                    if st == Status::TryWalk {
                        st = self.on_colliding_solid(&current, ni, &info);
                    }
                    st
                }
                Kind::Via => {
                    let mut st = self.on_colliding_via(Pusher::Line(&current), ni, &info, cur_rank - 1);
                    if st == Status::TryWalk {
                        st = self.on_colliding_solid(&current, ni, &info);
                    }
                    st
                }
                Kind::Solid => self.on_colliding_solid(&current, ni, &info),
            }
        }
    }

    /// `SHOVE::shoveMainLoop`.
    fn shove_main_loop(&mut self) -> Status {
        let mut st = Status::Ok;
        let iter_limit = self.settings.shove_iteration_limit;
        self.iter = 0;
        while !self.line_stack.is_empty() {
            st = self.shove_iteration();
            self.iter += 1;
            if st == Status::Incomplete || self.iter >= iter_limit {
                st = Status::Incomplete;
                break;
            }
        }
        st
    }

    // ------------------------------------------------------------ after the loop

    /// `SHOVE::runOptimizer`: optimize every shoved line in the shoved world.
    fn run_optimizer(&mut self) {
        use crate::optimizer::effort;
        let (mut flags, passes) = match self.settings.optimizer_effort {
            OptEffort::Low => (effort::MERGE_OBTUSE, 1),
            OptEffort::Medium | OptEffort::Full => (effort::MERGE_SEGMENTS, 2),
        };
        // Smart pads need the 45-degree corner mode, the only one this router places in.
        if self.settings.smart_pads {
            flags |= effort::SMART_PADS;
        }
        let max_width = self.optimizer_queue.iter().map(|l| l.line.width).max().unwrap_or(0);
        let area = self.affected.map(|(x0, y0, x1, y1)| (x0 - max_width, y0 - max_width, x1 + max_width, y1 + max_width));
        for _ in 0..passes {
            self.optimizer_queue.reverse();
            for i in 0..self.optimizer_queue.len() {
                let line = self.optimizer_queue[i].clone();
                let root = line.line.segment_ids.iter().find_map(|id| self.root_of.get(id).copied());
                if root.is_some_and(|r| self.roots[r].is_head) || line.line.pts.len() < 2 {
                    continue;
                }
                let optimized = crate::optimizer::optimize_in_area(&line.line, &self.node, self.rules, &line.links(), flags, area);
                if optimized.pts != line.line.pts {
                    let mut new = SLine { line: optimized, via: line.via.clone(), rank: self.line_rank(&line) };
                    new.clear_links();
                    self.replace_line(&line, &mut new, false);
                    self.optimizer_queue[i] = new;
                }
            }
        }
    }

    /// `SHOVE::Run` for one head.
    fn run(mut self, head: Line) -> Option<ShoveOutcome> {
        let mut head = head;
        let raw = head.pts.clone();
        let ids = self.node.add_line(&head, None, false);
        if ids.is_empty() {
            return Some(ShoveOutcome { head: raw, displaced_lines: Vec::new(), displaced_vias: Vec::new(), world: self.node });
        }
        head.segment_ids = ids.clone();
        let mut head_line = SLine::new(head);
        head_line.rank = HEAD_RANK;
        let head_root = self.new_root(RootEntry { root_line: Some(head_line.clone()), is_head: true, ..RootEntry::default() });
        for &id in &ids {
            self.ranks.insert(id, HEAD_RANK);
            self.head_marked.insert(id);
            self.root_of.insert(id, head_root);
        }
        if !self.push_line_stack(head_line, false) {
            return None;
        }
        if self.shove_main_loop() != Status::Ok {
            return None;
        }
        self.run_optimizer();
        Some(self.outcome(head_root, raw))
    }

    /// `reconstructHeads` + `removeHeads` + the diff against the world.
    fn outcome(mut self, head_root: usize, raw: Vec<Point>) -> ShoveOutcome {
        let head_entry = self.roots[head_root].clone();
        let head = head_entry.new_line.as_ref().map(|l| l.line.pts.clone()).unwrap_or(raw);
        let mut head_ids: Vec<ItemId> = self.head_marked.iter().copied().collect();
        if let Some(l) = &head_entry.new_line {
            head_ids.extend(l.line.segment_ids.iter().copied());
        }
        for id in head_ids {
            self.node.remove(id);
        }

        // tracks: group by the IR track each shoved line stood on
        let mut by_track: BTreeMap<String, Vec<Line>> = BTreeMap::new();
        let mut template: BTreeMap<String, Line> = BTreeMap::new();
        let mut covered: HashSet<ItemId> = HashSet::new();
        for r in &self.roots {
            if r.is_head {
                continue;
            }
            let (Some(root_line), Some(new_line)) = (&r.root_line, &r.new_line) else { continue };
            if root_line.line.pts == new_line.line.pts {
                continue; // pushed and pushed back: nothing to commit
            }
            let tracks = self.source_tracks(&root_line.line.segment_ids);
            covered.extend(root_line.line.segment_ids.iter().copied());
            let mut it = tracks.iter();
            if let Some(first) = it.next() {
                by_track.entry(first.clone()).or_default().push(Line::from_points(new_line.line.net.clone(), new_line.line.layer, new_line.line.width, new_line.line.pts.clone()));
                template.entry(first.clone()).or_insert_with(|| Line::new(new_line.line.net.clone(), new_line.line.layer, new_line.line.width));
            }
            for other in it {
                by_track.entry(other.clone()).or_default();
                template.entry(other.clone()).or_insert_with(|| Line::new(new_line.line.net.clone(), new_line.line.layer, new_line.line.width));
            }
        }
        // what of an affected track no shoved line covered goes back as it was
        for (track, lines) in by_track.iter_mut() {
            lines.extend(self.uncovered_runs(track, &covered));
        }
        let mut displaced_lines = Vec::new();
        for (track, lines) in by_track {
            if lines.is_empty() {
                displaced_lines.push(DisplacedLine { source_track: Some(track.clone()), line: template[&track].clone() });
            }
            for line in lines {
                displaced_lines.push(DisplacedLine { source_track: Some(track.clone()), line });
            }
        }

        let mut displaced_vias = Vec::new();
        for r in &self.roots {
            if let (Some((_, old)), Some(new)) = (&r.old_via, &r.new_via) {
                if old.pos != new.pos {
                    if let Some(name) = &old.source_via {
                        displaced_vias.push(DisplacedVia { source_via: name.clone(), pos: new.pos });
                    }
                }
            }
        }
        ShoveOutcome { head, displaced_lines, displaced_vias, world: self.node }
    }

    /// The IR tracks a set of the original world's segments belong to.
    fn source_tracks(&self, ids: &[ItemId]) -> Vec<String> {
        let mut out: Vec<String> = ids
            .iter()
            .filter_map(|id| match self.world.get(*id) {
                Some(Item::Segment(s)) => s.source_track.as_ref().map(|(t, _)| t.clone()),
                _ => None,
            })
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The runs of an IR track's segments no shoved line covered, as they were.
    fn uncovered_runs(&self, track: &str, covered: &HashSet<ItemId>) -> Vec<Line> {
        let mut segs: Vec<(usize, ItemId, &crate::item::Segment)> = self
            .world
            .iter()
            .filter_map(|(id, it)| match it {
                Item::Segment(s) if s.source_track.as_ref().is_some_and(|(t, _)| t == track) && !covered.contains(&id) => Some((s.source_track.as_ref().unwrap().1, id, s)),
                _ => None,
            })
            .collect();
        segs.sort_by_key(|&(idx, id, _)| (idx, id));
        let mut runs: Vec<Line> = Vec::new();
        let mut prev_idx: Option<usize> = None;
        for (idx, _, s) in segs {
            let extend = prev_idx.is_some_and(|p| p + 1 == idx) && runs.last().is_some_and(|l: &Line| l.last() == Some(s.a) && l.width == s.width && l.layer == s.layer);
            if extend {
                runs.last_mut().unwrap().pts.push(s.b);
            } else {
                runs.push(Line::from_points(s.net.clone(), s.layer, s.width, vec![s.a, s.b]));
            }
            prev_idx = Some(idx);
        }
        runs
    }
}

/// `SHOVE::Run` + `ShoveLine`: push whatever `raw` (the candidate head path)
/// collides with out of the way, transitively, until nothing left on the
/// stack collides or the iteration limit
/// (`ROUTING_SETTINGS::ShoveIterationLimit`) is spent. `None` means the
/// shove failed outright (a pad it could not walk around, a track it could
/// not push, or the limit) -- the caller should fall back to walkaround,
/// same as upstream's `rhShoveOnly`.
pub fn shove_line(node: &Node, raw: &[Point], net: &Net, layer: i32, width: Um, rules: &BoardRules, settings: &RoutingSettings) -> Option<ShoveOutcome> {
    if raw.len() < 2 {
        return Some(ShoveOutcome { head: raw.to_vec(), displaced_lines: Vec::new(), displaced_vias: Vec::new(), world: node.clone() });
    }
    let head = Line::from_points(net.clone(), layer, width, raw.to_vec());
    Shove::new(node, rules, settings).run(head)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{net_of, Segment};
    use crate::layer::LayerRange;
    use eda_drc::kimath::Shape;
    use eda_model::ir::Point;

    fn rules() -> BoardRules {
        serde_yaml::from_str("track_width: 200\nclearance: 200\nvia_drill: 300\nvia_diameter: 600\n").unwrap()
    }

    #[test]
    fn pushes_a_crossing_track_out_of_the_way() {
        let mut node = Node::new();
        // An existing GND track running vertically through where the new
        // SIG route needs to go horizontally.
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: 2500, y: -2000 }, b: Point { x: 2500, y: 2000 }, width: 200, source_track: Some(("trkA".into(), 0)), locked: false }));
        let rules = rules();
        let settings = RoutingSettings::default();
        let raw = vec![Point { x: 0, y: 0 }, Point { x: 5000, y: 0 }];
        let outcome = shove_line(&node, &raw, &net_of("SIG"), 0, 200, &rules, &settings).expect("a single crossing track must be shovable");
        assert_eq!(outcome.head, raw, "the pusher itself goes straight through; only the obstacle moves");
        assert_eq!(outcome.displaced_lines.len(), 1);
        assert_eq!(outcome.displaced_lines[0].source_track.as_deref(), Some("trkA"));
        // The displaced track must actually clear the new route's own
        // geometry (the pusher's path at its own width), not just not be
        // exactly where it started.
        let moved = &outcome.displaced_lines[0].line;
        for pusher_leg in raw.windows(2) {
            let pusher_shape = Shape::Stadium { a: pusher_leg[0], b: pusher_leg[1], r: 100 };
            for moved_leg in moved.segs() {
                let moved_shape = Shape::Stadium { a: moved_leg.0, b: moved_leg.1, r: 100 };
                assert!(pusher_shape.collides(&moved_shape, rules.clearance_of("SIG")).is_none(), "shoved track still too close to the new route");
            }
        }
        // Endpoints of the shoved track must be preserved (shove never
        // disconnects anything it moves).
        assert_eq!(moved.first(), Some(Point { x: 2500, y: -2000 }));
        assert_eq!(moved.last(), Some(Point { x: 2500, y: 2000 }));
    }

    #[test]
    fn shove_direction_rejects_moving_toward_the_pusher() {
        // Obstacle along y=0; pusher starts above it at y=500. Shoving the
        // obstacle up past the pusher's start puts that start inside the
        // swept area -- the wrong way.
        let node = Node::new();
        let (rules, settings) = (rules(), RoutingSettings::default());
        let sh = Shove::new(&node, &rules, &settings);
        let pusher = SLine::new(Line::from_points(None, 0, 100, vec![Point { x: 500, y: 500 }, Point { x: 500, y: -500 }]));
        let obs = Line::from_points(None, 0, 100, vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }]);
        let up = Line::from_points(None, 0, 100, vec![Point { x: 0, y: 0 }, Point { x: 0, y: 800 }, Point { x: 1000, y: 800 }, Point { x: 1000, y: 0 }]);
        let down = Line::from_points(None, 0, 100, vec![Point { x: 0, y: 0 }, Point { x: 0, y: -800 }, Point { x: 1000, y: -800 }, Point { x: 1000, y: 0 }]);
        assert!(!sh.check_shove_direction(&pusher, &obs, &up));
        assert!(sh.check_shove_direction(&pusher, &obs, &down));
    }

    /// `SHOVE::onCollidingSegment`: a locked track cannot move (`SH_TRY_WALK`),
    /// so `onCollidingSolid` walks the pusher around it instead.
    #[test]
    fn walks_the_head_around_a_locked_track() {
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: 2500, y: -2000 }, b: Point { x: 2500, y: 2000 }, width: 200, source_track: Some(("trkA".into(), 0)), locked: true }));
        let rules = rules();
        let settings = RoutingSettings::default();
        let raw = vec![Point { x: 0, y: 0 }, Point { x: 5000, y: 0 }];
        let outcome = shove_line(&node, &raw, &net_of("SIG"), 0, 200, &rules, &settings).expect("the head walks around the locked track");
        assert!(outcome.displaced_lines.is_empty(), "a locked track never moves");
        assert_ne!(outcome.head, raw, "the head went around it");
        assert_eq!(outcome.head.first(), raw.first());
        assert_eq!(outcome.head.last(), raw.last());
    }

    /// `SHOVE::runOptimizer`: a shoved line with a needless bump is flattened again, in the shoved world.
    #[test]
    fn the_optimizer_flattens_a_shoved_line() {
        let mut node = Node::new();
        let id = node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: 0, y: 0 }, b: Point { x: 4000, y: 0 }, width: 200, source_track: Some(("t".into(), 0)), locked: false }));
        let (rules, settings) = (rules(), RoutingSettings::default());
        let mut sh = Shove::new(&node, &rules, &settings);
        let old = sh.assemble(id).unwrap();
        let bumpy = vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }, Point { x: 1500, y: 500 }, Point { x: 2500, y: 500 }, Point { x: 3000, y: 0 }, Point { x: 4000, y: 0 }];
        let mut shoved = old.clone();
        shoved.clear_links();
        shoved.line.pts = bumpy;
        sh.replace_line(&old, &mut shoved, true);
        assert!(sh.push_line_stack(shoved, false));
        sh.line_stack.clear();
        sh.run_optimizer();
        let seg = sh.node.iter().find_map(|(id, it)| matches!(it, Item::Segment(_)).then_some(id)).unwrap();
        let after = sh.assemble(seg).unwrap();
        assert_eq!(after.line.pts, vec![Point { x: 0, y: 0 }, Point { x: 4000, y: 0 }]);
        // and the root line remembers where the track was, so the diff is against the original
        assert_eq!(sh.roots.len(), 1);
        assert_eq!(sh.roots[0].root_line.as_ref().unwrap().line.pts, vec![Point { x: 0, y: 0 }, Point { x: 4000, y: 0 }]);
    }

    /// A junction cuts an IR track into two lines; only one is shoved, and the other part of the
    /// track is handed back as it was (the commit replaces whole IR tracks).
    #[test]
    fn the_part_of_a_track_a_shove_did_not_touch_is_handed_back() {
        let mut node = Node::new();
        for (i, (a, b)) in [(Point { x: 2500, y: -2000 }, Point { x: 2500, y: 0 }), (Point { x: 2500, y: 0 }, Point { x: 2500, y: 2000 }), (Point { x: 2500, y: 2000 }, Point { x: 2500, y: 4000 })].into_iter().enumerate() {
            node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a, b, width: 200, source_track: Some(("t".into(), i)), locked: false }));
        }
        // a second track of the same net leaves the middle joint: the first two segments and the last are different lines
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: 2500, y: 2000 }, b: Point { x: 5000, y: 2000 }, width: 200, source_track: Some(("branch".into(), 0)), locked: false }));
        let raw = vec![Point { x: 0, y: -1000 }, Point { x: 5000, y: -1000 }];
        let out = shove_line(&node, &raw, &net_of("SIG"), 0, 200, &rules(), &RoutingSettings::default()).expect("the first line is pushed");
        let of_t: Vec<_> = out.displaced_lines.iter().filter(|d| d.source_track.as_deref() == Some("t")).collect();
        assert_eq!(of_t.len(), 2, "the pushed line and the part of t that was not: {of_t:?}");
        assert!(of_t.iter().any(|d| d.line.pts == vec![Point { x: 2500, y: 2000 }, Point { x: 2500, y: 4000 }]), "the untouched part comes back as it was: {of_t:?}");
        assert!(out.displaced_lines.iter().all(|d| d.source_track.as_deref() != Some("branch")));
    }

    /// `VIA::IsLocked()` -> `SH_TRY_WALK`: a locked via is walked around, not moved.
    #[test]
    fn a_locked_via_is_walked_around() {
        use crate::item::Via;
        let mut node = Node::new();
        node.add(Item::Via(Via { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: 2500, y: 0 }, diameter: 600, drill: 300, source_via: Some("viaA".into()), locked: true }));
        let (rules, settings) = (rules(), RoutingSettings::default());
        let raw = vec![Point { x: 0, y: 0 }, Point { x: 5000, y: 0 }];
        let out = shove_line(&node, &raw, &net_of("SIG"), 0, 200, &rules, &settings).expect("walks around");
        assert!(out.displaced_vias.is_empty());
        assert_ne!(out.head, raw);
    }

    #[test]
    fn pushes_a_stitching_via_aside() {
        use crate::item::Via;
        let mut node = Node::new();
        node.add(Item::Via(Via { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: 2500, y: 0 }, diameter: 600, drill: 300, source_via: Some("viaA".into()), locked: false }));
        let rules = rules();
        let settings = RoutingSettings::default();
        let raw = vec![Point { x: 0, y: 0 }, Point { x: 5000, y: 0 }];
        let outcome = shove_line(&node, &raw, &net_of("SIG"), 0, 200, &rules, &settings).expect("a lone via must be shovable");
        assert_eq!(outcome.displaced_vias.len(), 1);
        assert_eq!(outcome.displaced_vias[0].source_via, "viaA");
        assert_ne!(outcome.displaced_vias[0].pos, Point { x: 2500, y: 0 }, "the via must actually move");
    }
}
