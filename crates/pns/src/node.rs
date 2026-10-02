//! Port of `PNS::NODE` (`pcbnew/router/pns_node.{h,cpp}`): the router's
//! "world state" -- every item on the board plus the joint graph connecting
//! them, queryable for collisions and branchable for undoable what-if edits.
//!
//! ## Branching: a deliberate simplification
//!
//! KiCad's real `NODE` is a two-tier copy-on-write overlay: branching off
//! the *root* is O(1) (the child starts with empty joints/index and falls
//! through to the root on every miss), but branching off a non-root parent
//! deep-clones that parent's joints/index/override-set, because lookups
//! only ever consult "this node" or "the root" -- never a chain of
//! intermediate ancestors (confirmed by reading `pns_node.cpp`: `FindJoint`,
//! `QueryColliding`, `HitTest` etc. all hard-code `m_root`, never walk
//! `m_parent`). On top of that, `Commit()` doesn't clone geometry either --
//! it re-owns the branch's heap objects by flipping an owner pointer.
//!
//! That machinery exists to make KiCad's interactive router cheap across
//! thousands of mouse-move ticks on boards with tens of thousands of items.
//! This port targets this project's own board sizes (the task's own
//! instruction: "small test boards"), and the frontend drives the backend
//! over HTTP per mouse-move rather than natively, so request-granularity
//! (not microsecond-granularity) latency is what matters. Given that,
//! [`Node::branch`] is a **full clone** -- `items`/`joints`/`index` all
//! copied -- and "commit" is simply "replace the parent variable with the
//! finished branch." This is semantically identical for every caller in
//! this crate (try an edit on an isolated copy, keep it or drop it) at the
//! cost of O(board size) per branch instead of KiCad's amortized-O(delta);
//! see `crates/pns/PARITY.md` for the tradeoff written out explicitly.
//!
//! Everything else here -- the joint merge/split rules on add/remove, the
//! `AssembleLine` bidirectional joint walk, the obstacle search -- follows
//! `pns_node.cpp` directly.

/// See `all_colliding`.
pub const CLEARANCE_EPSILON: Um = 1;

use crate::item::{same_net, Item, ItemId, Kind, Net};
use crate::joint::{Joint, JointKey};
use crate::layer::LayerRange;
use crate::line::Line;
use eda_drc::kimath::Shape;
use eda_model::ir::{Point, Um};
use eda_model::BoardRules;
use std::collections::HashMap;

use crate::index::Index;

/// `PNS::OBSTACLE`, trimmed to the fields this port actually uses.
#[derive(Debug, Clone)]
pub struct Obstacle {
    pub id: ItemId,
    pub kind: Kind,
    /// Edge-to-edge gap actually measured, vs. `clearance_needed` required.
    pub actual: Um,
    pub clearance_needed: Um,
    pub pos: Point,
}

#[derive(Default)]
pub struct Node {
    items: HashMap<ItemId, Item>,
    next_id: ItemId,
    joints: HashMap<JointKey, Joint>,
    index: Index,
}

impl Clone for Node {
    fn clone(&self) -> Self {
        Node { items: self.items.clone(), next_id: self.next_id, joints: self.joints.clone(), index: self.index.clone() }
    }
}

/// Cell size for the broad-phase grid: a few mm, coarse enough that a
/// typical query touches a handful of cells regardless of board size (the
/// same reasoning as `eda_drc::rtree::DrcRTree::new`'s doc comment).
const INDEX_CELL_UM: Um = 5_000_000 / 1000; // 5mm in um (board coords are um)

impl Node {
    pub fn new() -> Self {
        Node { items: HashMap::new(), next_id: 1, joints: HashMap::new(), index: Index::new(INDEX_CELL_UM) }
    }

    /// `NODE::Branch()`, simplified to a full clone -- see this module's
    /// doc comment.
    pub fn branch(&self) -> Node {
        self.clone()
    }

    pub fn get(&self, id: ItemId) -> Option<&Item> {
        self.items.get(&id)
    }

    pub fn contains(&self, id: ItemId) -> bool {
        self.items.contains_key(&id)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (ItemId, &Item)> {
        self.items.iter().map(|(&id, it)| (id, it))
    }

    fn bbox_of(item: &Item) -> (Um, Um, Um, Um) {
        let mut bbox: Option<(Um, Um, Um, Um)> = None;
        for layer in item.layers().iter() {
            let b = item.shape(layer).bbox(0);
            bbox = Some(match bbox {
                None => b,
                Some(a) => (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)),
            });
        }
        bbox.unwrap_or((0, 0, 0, 0))
    }

    /// `NODE::addSegment`/`addVia`/`addSolid` + `linkJoint` -- inserts the
    /// item, indexes it, and links/merges joints at every anchor point.
    pub fn add(&mut self, item: Item) -> ItemId {
        let id = self.next_id;
        self.next_id += 1;
        self.index.insert(id, Self::bbox_of(&item));
        for anchor in item.anchors() {
            self.touch_joint(anchor, item.layers(), item.net()).link(id);
        }
        self.items.insert(id, item);
        id
    }

    /// `touchJoint`: find the joint at `(pos, net)` overlapping `layers`,
    /// merging its layer range in, or create a fresh one. Unlike KiCad's
    /// version this never needs a root-fallback copy-down step, since a
    /// branch already owns a full copy of `joints` (see the module doc).
    fn touch_joint(&mut self, pos: Point, layers: LayerRange, net: &Net) -> &mut Joint {
        let key = JointKey::new(pos, net);
        // KiCad merges every existing joint at this (pos,net) whose layer
        // range overlaps the new one into a single entry (a via's wide
        // range absorbing a track's single-layer range, for instance).
        // With a plain HashMap<JointKey,_> (one entry per key, same as
        // KiCad's map is one entry per *non-overlapping* layer sub-range in
        // the uncommon case of disjoint layer groups at one point) we don't
        // model multiple disjoint-layer joints at the same point -- not a
        // real configuration for the two-outer-layer boards this project
        // produces (no blind/buried vias), so one merged entry per
        // (pos,net) is exact here.
        let joint = self.joints.entry(key).or_default();
        joint.layers.merge(&layers);
        joint
    }

    /// `NODE::Remove(ITEM*)` for a single item, dispatched by kind.
    /// Returns the removed item, if it existed. Joints left empty by this
    /// removal are pruned immediately (a deliberate improvement over
    /// KiCad's acknowledged `// fixme: remove dangling joints` -- see
    /// `pns_node.cpp`; harmless either way since every lookup here is by
    /// exact `(pos,net)` key, never by iterating all joints for "real"
    /// ones).
    pub fn remove(&mut self, id: ItemId) -> Option<Item> {
        let item = self.items.remove(&id)?;
        self.index.remove(id);
        for anchor in item.anchors() {
            let key = JointKey::new(anchor, item.net());
            if let Some(joint) = self.joints.get_mut(&key) {
                if joint.unlink(id) {
                    self.joints.remove(&key);
                }
            }
        }
        Some(item)
    }

    pub fn joint_at(&self, pos: Point, net: &Net) -> Option<&Joint> {
        self.joints.get(&JointKey::new(pos, net))
    }

    /// `JOINT::NextSegment`: the item linked at `joint` other than
    /// `current`, if the joint is "trivial" for walk-through purposes --
    /// exactly one same-net, overlapping-layer `Segment` candidate, and no
    /// `Solid`/`Via` present at all (KiCad's rule: a pad or via at a joint
    /// is *always* a hard stop, even if a valid segment candidate also
    /// exists there -- see `pns_joint.h`'s `NextSegment`).
    fn next_segment(&self, joint: &Joint, current: ItemId, current_net: &Net, current_layers: LayerRange) -> Option<ItemId> {
        let mut candidate = None;
        for &id in &joint.links {
            if id == current {
                continue;
            }
            match self.items.get(&id) {
                Some(Item::Segment(s)) => {
                    if same_net(&s.net, current_net) && LayerRange::single(s.layer).overlaps(&current_layers) {
                        if candidate.is_some() {
                            return None; // branch point: more than one same-net candidate
                        }
                        candidate = Some(id);
                    }
                }
                Some(Item::Solid(_)) | Some(Item::Via(_)) => return None, // pad/via: always a hard stop
                _ => {}
            }
        }
        candidate
    }

    /// `NODE::AssembleLine`: walk the joint graph outward from `start` (a
    /// `Segment`) in both directions until a non-trivial joint, returning
    /// the flattened polyline plus the ordered segment ids it came from.
    /// `None` if `start` isn't a `Segment` (a lone `Via` has no line to
    /// assemble -- callers that want via continuity read `Item::Via`
    /// directly).
    pub fn assemble_line(&self, start: ItemId) -> Option<Line> {
        let Item::Segment(seg0) = self.items.get(&start)? else { return None };
        let (net, layer, width) = (seg0.net.clone(), seg0.layer, seg0.width);
        let layers = LayerRange::single(layer);

        // Forward walk from b, backward walk from a; each produces an
        // ordered (point, segment-just-traversed) list we splice together.
        let mut fwd_pts = vec![seg0.b];
        let mut fwd_segs = vec![start];
        let (mut cur, mut cur_end) = (start, seg0.b);
        let mut via_end = None;
        while let Some(joint) = self.joint_at(cur_end, &net) {
            if let Some(next_id) = self.next_segment(joint, cur, &net, layers) {
                let Item::Segment(ns) = &self.items[&next_id] else { break };
                let other_end = if ns.a == cur_end { ns.b } else { ns.a };
                fwd_pts.push(other_end);
                fwd_segs.push(next_id);
                cur = next_id;
                cur_end = other_end;
            } else {
                via_end = joint.links.iter().find(|&&id| matches!(self.items.get(&id), Some(Item::Via(_)))).copied();
                break;
            }
        }

        let mut back_pts = vec![seg0.a];
        let mut back_segs: Vec<ItemId> = Vec::new();
        let (mut cur, mut cur_end) = (start, seg0.a);
        let mut via_start = None;
        while let Some(joint) = self.joint_at(cur_end, &net) {
            if let Some(next_id) = self.next_segment(joint, cur, &net, layers) {
                let Item::Segment(ns) = &self.items[&next_id] else { break };
                let other_end = if ns.a == cur_end { ns.b } else { ns.a };
                back_pts.push(other_end);
                back_segs.push(next_id);
                cur = next_id;
                cur_end = other_end;
            } else {
                via_start = joint.links.iter().find(|&&id| matches!(self.items.get(&id), Some(Item::Via(_)))).copied();
                break;
            }
        }

        back_pts.reverse();
        back_segs.reverse();
        let mut pts = back_pts;
        pts.extend(fwd_pts);
        let mut segment_ids = back_segs;
        segment_ids.push(start);
        segment_ids.extend(fwd_segs.into_iter().skip(1));

        let mut line = Line::from_points(net, layer, width, pts);
        line.segment_ids = segment_ids;
        line.via_at_start = via_start;
        line.via_at_end = via_end;
        Some(line)
    }

    /// `NODE::FindLinesBetweenJoints`, narrowed to this port's one caller
    /// (`LinePlacer`'s loop removal, see that module's own doc comment):
    /// every distinct maximal same-net segment chain already in this node
    /// whose two endpoints are exactly `a` and `b` (in either order). A
    /// "line" here is the same unit [`Self::assemble_line`] already walks
    /// (a run between two non-trivial joints -- a branch, a pad/via, or a
    /// dead end); each one is returned at most once (assembling from any
    /// of its segments gives the same result, so the first one visited
    /// marks the rest as `seen` rather than re-discovering the same line
    /// from a different starting segment).
    pub fn find_lines_between_joints(&self, a: Point, b: Point, net: &Net) -> Vec<Line> {
        let mut seen: std::collections::HashSet<ItemId> = std::collections::HashSet::new();
        let mut out = Vec::new();
        for (&id, item) in &self.items {
            if seen.contains(&id) {
                continue;
            }
            let Item::Segment(seg) = item else { continue };
            if !same_net(&seg.net, net) {
                continue;
            }
            let Some(line) = self.assemble_line(id) else { continue };
            seen.extend(line.segment_ids.iter().copied());
            let (first, last) = (line.first(), line.last());
            if (first == Some(a) && last == Some(b)) || (first == Some(b) && last == Some(a)) {
                out.push(line);
            }
        }
        out
    }

    /// Remove every segment `line` was assembled from (its `via_at_*` are
    /// left alone -- a via is its own item, only re-touched if the caller
    /// explicitly wants to move/remove it) -- the write-side counterpart of
    /// `assemble_line`, used before re-adding an edited shape.
    pub fn remove_line_segments(&mut self, line: &Line) {
        for &id in &line.segment_ids {
            self.remove(id);
        }
    }

    /// Insert `line` as a fresh chain of `Segment`s (`NODE::Add(LINE&)`,
    /// minus the redundant-segment check this port doesn't need -- callers
    /// only ever add freshly-computed, non-redundant geometry). Returns the
    /// new segment ids in point order.
    pub fn add_line(&mut self, line: &Line, source_track: Option<(String, usize)>, locked: bool) -> Vec<ItemId> {
        let mut ids = Vec::with_capacity(line.segment_count());
        for (i, (a, b)) in line.segs().enumerate() {
            if a == b {
                continue; // degenerate leg, same as KiCad's A==B rejection
            }
            let seg = Item::Segment(crate::item::Segment { net: line.net.clone(), layer: line.layer, a, b, width: line.width, source_track: source_track.clone().map(|(t, _)| (t, i)), locked });
            ids.push(self.add(seg));
        }
        ids
    }

    // ---------------------------------------------------------------- collision

    fn net_str(n: &Net) -> Option<&str> {
        n.as_deref()
    }

    /// `NODE::GetClearance`-equivalent: resolve the clearance two nets must
    /// keep, via this project's own `eda_drc` constraint resolution --
    /// which is the seam the NODE research spec calls out as living in
    /// `ITEM::Collide` in real KiCad (board-specific, injected via
    /// `RULE_RESOLVER`), not in `NODE` itself. Same-net items never collide
    /// at all (checked by the caller before this is reached).
    pub fn clearance(rules: &BoardRules, a: &Net, b: &Net) -> Um {
        eda_drc::constraints::clearance(rules, Self::net_str(a), Self::net_str(b))
    }

    /// `NODE::QueryColliding`/`ITEM::Collide`, narrowed to "every obstacle
    /// a candidate shape on `layers`, net `net`, would hit" -- the broad
    /// phase (spatial index, inflated by the board's worst-case clearance)
    /// narrowed by the exact test (`Shape::collides`), skipping same-net
    /// items, non-overlapping layers, and anything in `exclude` (the
    /// query's own in-flight items, so a line never collides with its own
    /// soon-to-be-replaced segments).
    pub fn all_colliding(&self, shape: &Shape, net: &Net, layers: LayerRange, rules: &BoardRules, exclude: &[ItemId]) -> Vec<Obstacle> {
        let margin = eda_drc::constraints::worst_case_clearance(rules);
        let bbox = shape.bbox(margin);
        let mut out = Vec::new();
        for id in self.index.query(bbox) {
            if exclude.contains(&id) {
                continue;
            }
            let Some(item) = self.items.get(&id) else { continue };
            if !item.layers().overlaps(&layers) {
                continue;
            }
            if same_net(item.net(), net) {
                continue;
            }
            // `COLLISION_SEARCH_OPTIONS::m_useClearanceEpsilon` (default on):
            // `rv - m_clearanceEpsilon`, the board's DRC epsilon (0.5 µm).
            // Hull vertices here are rounded to whole µm (KiCad: nm), so the
            // epsilon is one µm -- the same "a hull-hugging path is not a
            // collision" tolerance at this IR's resolution.
            let clearance = Self::clearance(rules, net, item.net());
            let clearance = if clearance > 0 { (clearance - CLEARANCE_EPSILON).max(0) } else { clearance };
            // A multilayer item (a via) may present a different shape per
            // layer in a fuller port; this one shape per item is exact for
            // every kind we construct (see `Item::shape`'s own doc note).
            let other_layer = item.layers().start();
            if let Some((actual, pos)) = shape.collides(&item.shape(other_layer), clearance) {
                out.push(Obstacle { id, kind: item.kind(), actual, clearance_needed: clearance, pos });
            }
        }
        out
    }

    /// First (any) obstacle -- `NODE::CheckColliding`'s "does one exist"
    /// shortcut, used where the caller only needs a boolean.
    pub fn first_colliding(&self, shape: &Shape, net: &Net, layers: LayerRange, rules: &BoardRules, exclude: &[ItemId]) -> Option<Obstacle> {
        self.all_colliding(shape, net, layers, rules, exclude).into_iter().next()
    }

    /// Every obstacle a whole `Line`'s geometry (every leg, plus its via
    /// ends if any) would hit, deduplicated by item id and merged into one
    /// list -- used by shove/walkaround, which need the *nearest along the
    /// path*, not just the first leg's own nearest.
    pub fn line_colliding(&self, line: &Line, rules: &BoardRules, exclude: &[ItemId]) -> Vec<(usize, Obstacle)> {
        let layers = LayerRange::single(line.layer);
        let mut out = Vec::new();
        for (i, (a, b)) in line.segs().enumerate() {
            let shape = Shape::Stadium { a, b, r: line.width / 2 };
            for obs in self.all_colliding(&shape, &line.net, layers, rules, exclude) {
                out.push((i, obs));
            }
        }
        out
    }

    // ---------------------------------------------------------------- anchors / hit-testing

    /// Nearest item with an anchor within `max_dist` of `pos` on a layer
    /// overlapping `layers` -- used for route-start (click near a pad/via/
    /// track end) and route-finish (snap onto a same-net target). Not a
    /// literal KiCad `NODE` method (KiCad's equivalent snapping lives in
    /// the UI tool layer, `router_tool.cpp`), but the natural place for it
    /// here since it only needs `items`.
    pub fn nearest_anchor(&self, pos: Point, layers: LayerRange, max_dist: Um, net_filter: Option<&Net>) -> Option<(ItemId, Point)> {
        let mut best: Option<(ItemId, Point, i128)> = None;
        for (&id, item) in &self.items {
            if !item.layers().overlaps(&layers) {
                continue;
            }
            if let Some(f) = net_filter {
                if !same_net(item.net(), f) {
                    continue;
                }
            }
            for a in item.anchors() {
                let d2 = (a.x - pos.x) as i128 * (a.x - pos.x) as i128 + (a.y - pos.y) as i128 * (a.y - pos.y) as i128;
                if d2 <= (max_dist as i128) * (max_dist as i128) && best.as_ref().map(|(_, _, bd)| d2 < *bd).unwrap_or(true) {
                    best = Some((id, a, d2));
                }
            }
        }
        best.map(|(id, p, _)| (id, p))
    }

    /// Nearest item whose own *shape* (not just its anchor points) comes
    /// within `max_dist` of `pos` -- for hit-testing a drag-start click
    /// anywhere along a track, not just at its ends. [`Self::nearest_anchor`]
    /// is the right tool for "did this land on a connection point"
    /// (route start/end snapping); this one is "what's physically under
    /// the cursor," the same distinction `pcb_selection_tool.cpp` draws
    /// between a point hit-test and anchor snapping.
    pub fn item_at(&self, pos: Point, layers: LayerRange, max_dist: Um) -> Option<ItemId> {
        let probe = Shape::Circle { c: pos, r: 0 };
        let bbox = probe.bbox(max_dist);
        let mut best: Option<(ItemId, Um)> = None;
        for id in self.index.query(bbox) {
            let Some(item) = self.items.get(&id) else { continue };
            if !item.layers().overlaps(&layers) {
                continue;
            }
            let (actual, _) = probe.clearance_to(&item.shape(item.layers().start()));
            if actual <= max_dist && best.as_ref().map(|(_, d)| actual < *d).unwrap_or(true) {
                best = Some((id, actual));
            }
        }
        best.map(|(id, _)| id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{net_of, Segment, Solid, Via};
    use eda_model::ir::Point;

    fn p(x: Um, y: Um) -> Point {
        Point { x, y }
    }

    fn rules() -> BoardRules {
        serde_yaml::from_str("track_width: 200\nclearance: 200\nvia_drill: 300\nvia_diameter: 600\n").unwrap()
    }

    #[test]
    fn add_remove_round_trip() {
        let mut n = Node::new();
        let id = n.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: p(0, 0), b: p(1000, 0), width: 200, source_track: None, locked: false }));
        assert_eq!(n.len(), 1);
        assert!(n.joint_at(p(0, 0), &net_of("GND")).is_some());
        let removed = n.remove(id).unwrap();
        assert!(matches!(removed, Item::Segment(_)));
        assert_eq!(n.len(), 0);
        assert!(n.joint_at(p(0, 0), &net_of("GND")).is_none(), "joint must be pruned once empty");
    }

    #[test]
    fn assemble_line_walks_through_trivial_joints_and_stops_at_via() {
        let mut n = Node::new();
        let net = net_of("SIG");
        let s1 = n.add(Item::Segment(Segment { net: net.clone(), layer: 0, a: p(0, 0), b: p(1000, 0), width: 200, source_track: None, locked: false }));
        let _s2 = n.add(Item::Segment(Segment { net: net.clone(), layer: 0, a: p(1000, 0), b: p(2000, 0), width: 200, source_track: None, locked: false }));
        let _via = n.add(Item::Via(Via { net: net.clone(), layers: LayerRange::new(0, 1), pos: p(2000, 0), diameter: 600, drill: 300, source_via: None, locked: false }));
        let line = n.assemble_line(s1).expect("line");
        assert_eq!(line.pts, vec![p(0, 0), p(1000, 0), p(2000, 0)]);
        assert_eq!(line.segment_ids.len(), 2);
        assert!(line.via_at_end.is_some());
    }

    #[test]
    fn assemble_line_stops_at_branch_point() {
        let mut n = Node::new();
        let net = net_of("SIG");
        let s1 = n.add(Item::Segment(Segment { net: net.clone(), layer: 0, a: p(0, 0), b: p(1000, 0), width: 200, source_track: None, locked: false }));
        // Two different segments continuing from (1000,0): a branch, not a
        // trivial pass-through, so the walk must stop at s1 alone.
        n.add(Item::Segment(Segment { net: net.clone(), layer: 0, a: p(1000, 0), b: p(2000, 0), width: 200, source_track: None, locked: false }));
        n.add(Item::Segment(Segment { net: net.clone(), layer: 0, a: p(1000, 0), b: p(1000, 1000), width: 200, source_track: None, locked: false }));
        let line = n.assemble_line(s1).unwrap();
        assert_eq!(line.pts, vec![p(0, 0), p(1000, 0)]);
    }

    #[test]
    fn collision_ignores_same_net_and_excluded() {
        let mut n = Node::new();
        let rules = rules();
        let pad = n.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: p(0, 0), shape: Shape::Circle { c: p(0, 0), r: 500 }, source: "U1.1".into() }));
        let shape = Shape::Stadium { a: p(0, 0), b: p(1000, 0), r: 100 };
        // Same net as the pad: must not collide.
        assert!(n.first_colliding(&shape, &net_of("GND"), LayerRange::single(0), &rules, &[]).is_none());
        // Different net: must collide (pad radius 500 + clearance 200 > 0 distance).
        let hit = n.first_colliding(&shape, &net_of("SIG"), LayerRange::single(0), &rules, &[]);
        assert!(hit.is_some());
        // Excluded id: must not collide even on a different net.
        assert!(n.first_colliding(&shape, &net_of("SIG"), LayerRange::single(0), &rules, &[pad]).is_none());
    }

    #[test]
    fn nearest_anchor_finds_pad_within_threshold() {
        let mut n = Node::new();
        n.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: p(1000, 1000), shape: Shape::Circle { c: p(1000, 1000), r: 400 }, source: "U1.1".into() }));
        let hit = n.nearest_anchor(p(1050, 1000), LayerRange::single(0), 200, None);
        assert_eq!(hit.map(|(_, p)| p), Some(p(1000, 1000)));
        assert!(n.nearest_anchor(p(5000, 5000), LayerRange::single(0), 200, None).is_none());
    }

    #[test]
    fn branch_is_independent_clone() {
        let mut n = Node::new();
        let id = n.add(Item::Via(Via { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: p(0, 0), diameter: 600, drill: 300, source_via: None, locked: false }));
        let mut child = n.branch();
        child.remove(id);
        assert!(n.contains(id), "removing from a branch must not affect the parent");
        assert!(!child.contains(id));
    }

    #[test]
    fn item_at_finds_a_track_by_its_middle_not_just_its_ends() {
        let mut n = Node::new();
        let id = n.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: p(0, 0), b: p(10_000, 0), width: 200, source_track: None, locked: false }));
        // Dead centre of a long track: `nearest_anchor` (endpoints only)
        // would miss this entirely.
        assert_eq!(n.item_at(p(5000, 50), LayerRange::single(0), 200), Some(id));
        assert!(n.nearest_anchor(p(5000, 50), LayerRange::single(0), 200, None).is_none(), "sanity: nearest_anchor really doesn't see the middle");
        assert!(n.item_at(p(5000, 5000), LayerRange::single(0), 200).is_none());
    }
}
