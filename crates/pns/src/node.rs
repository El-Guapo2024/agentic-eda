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

/// See `all_colliding`: `m_clearanceEpsilon`, the board's `GetDRCEpsilon()` (0.0005 mm) (`pns_kicad_iface.cpp`).
pub const CLEARANCE_EPSILON: Nm = 500;

/// How far from a point [`Node::segment_at`] looks for a segment: half the widest track it can find (5 mm of copper).
const SEGMENT_AT_REACH: Nm = 2_500_000;

/// `NODE::AssembleLine`'s `MaxVerts` (1024 * 16): hard cap on a followed line.
const ASSEMBLE_MAX_VERTS: usize = 1024 * 16;

use crate::arcs::ArcPts;
use crate::item::{same_net, Item, ItemId, Kind, Net, Via};
use crate::joint::{Joint, JointKey};
use crate::layer::LayerRange;
use crate::line::Line;
use eda_drc::kimath::Shape;
use eda_model::ir::{Point, Nm};
use eda_model::BoardRules;
use std::collections::HashMap;

use crate::index::Index;

/// `ITEM::PnsKind` masks (`SOLID_T | SEGMENT_T | VIA_T`): which kinds of item a
/// search takes into account (`COLLISION_SEARCH_OPTIONS::m_kindMask`).
pub mod kind_mask {
    pub const SOLID: u8 = 1;
    pub const SEGMENT: u8 = 2;
    pub const VIA: u8 = 4;
    pub const ARC: u8 = 8;
    /// `ITEM::SEGMENT_T | ITEM::ARC_T`: the two things a track is made of.
    pub const TRACK: u8 = SEGMENT | ARC;
    pub const ANY: u8 = SOLID | SEGMENT | VIA | ARC;
}

/// The `kind_mask` bit of one [`Kind`].
pub fn kind_bit(k: Kind) -> u8 {
    match k {
        Kind::Solid => kind_mask::SOLID,
        Kind::Segment => kind_mask::SEGMENT,
        Kind::Arc => kind_mask::ARC,
        Kind::Via => kind_mask::VIA,
    }
}

/// `COLLISION_SEARCH_OPTIONS`, narrowed to what this port's callers set:
/// the kind mask, a per-item filter (the walkaround's `RestrictToCluster`,
/// the shove's `SHP_IGNORE`) and the clearance epsilon switch.
#[derive(Clone, Copy)]
pub struct QueryOpts<'a> {
    pub kind_mask: u8,
    pub filter: Option<&'a dyn Fn(ItemId) -> bool>,
    pub use_epsilon: bool,
    /// [`Node::nearest_obstacle`] only: measure where the line enters each obstacle's hull on the hull's bounding box instead (`NearestObstacle`'s
    /// `simplifyHull`, which KiCad turns on in the 90-degree corner modes: `cornerMode == MITERED_90 || cornerMode == ROUNDED_90`).
    pub box_hull: bool,
}

impl Default for QueryOpts<'_> {
    fn default() -> Self {
        QueryOpts { kind_mask: kind_mask::ANY, filter: None, use_epsilon: true, box_hull: false }
    }
}

/// `NODE::NearestObstacle`'s result: the obstacle and where along the
/// searched line its hull is first entered.
#[derive(Debug, Clone)]
pub struct NearestHit {
    pub id: ItemId,
    pub kind: Kind,
    /// `OBSTACLE::m_distFirst`: path length from the line's start to the first
    /// hull crossing (`INT_MAX`, here `i64::MAX`, when no crossing was found).
    pub dist_first: i64,
    /// `OBSTACLE::m_ipFirst`.
    pub ip: Point,
}

/// What [`Node::assemble_line_with`] may walk across (`NODE::AssembleLine`'s
/// `aFollowLockedSegments` / `aAllowSegmentSizeMismatch`).
#[derive(Debug, Clone, Copy)]
pub struct AssembleOpts {
    /// Continue through locked segments (`JOINT::NextSegment( .., aAllowLockedSegs )`).
    pub follow_locked_segments: bool,
    /// Continue through a change of track width. When false the line stops at the
    /// joint where the width changes, so a line never has two widths.
    pub allow_width_mismatch: bool,
    /// `aStopAtLockedJoints`: end the line at a locked joint ([`crate::joint::Joint::locked`], `NODE::LockJoint`).
    pub stop_at_locked_joints: bool,
}

impl Default for AssembleOpts {
    /// What the plain [`Node::assemble_line`] has always done: through everything.
    fn default() -> Self {
        AssembleOpts { follow_locked_segments: true, allow_width_mismatch: true, stop_at_locked_joints: false }
    }
}

/// `PNS::OBSTACLE`, trimmed to the fields this port actually uses.
#[derive(Debug, Clone)]
pub struct Obstacle {
    pub id: ItemId,
    pub kind: Kind,
    /// Edge-to-edge gap actually measured, vs. `clearance_needed` required.
    pub actual: Nm,
    pub clearance_needed: Nm,
    pub pos: Point,
}

#[derive(Default)]
pub struct Node {
    items: HashMap<ItemId, Item>,
    next_id: ItemId,
    joints: HashMap<JointKey, Joint>,
    index: Index,
    /// The sources of the pads that are non-plated holes (`PAD_ATTRIB::NPTH`): a route does not start on one (`isStartingPointRoutable`).
    npth: std::rc::Rc<std::collections::HashSet<String>>,
}

impl Clone for Node {
    fn clone(&self) -> Self {
        Node { items: self.items.clone(), next_id: self.next_id, joints: self.joints.clone(), index: self.index.clone(), npth: self.npth.clone() }
    }
}

/// Cell size for the broad-phase grid: a few mm, coarse enough that a
/// typical query touches a handful of cells regardless of board size (the
/// same reasoning as `eda_drc::rtree::DrcRTree::new`'s doc comment).
const INDEX_CELL_NM: Nm = 5_000_000; // 5 mm

impl Node {
    pub fn new() -> Self {
        Node { items: HashMap::new(), next_id: 1, joints: HashMap::new(), index: Index::new(INDEX_CELL_NM), npth: Default::default() }
    }

    /// Is the pad `source` a non-plated hole?
    pub fn is_npth(&self, source: &str) -> bool {
        self.npth.contains(source)
    }

    /// The pads that are non-plated holes ([`Self::is_npth`]).
    pub fn set_npth(&mut self, sources: std::collections::HashSet<String>) {
        self.npth = std::rc::Rc::new(sources);
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

    /// `ITEM::Mark( MK_LOCKED )` / `Unmark( MK_LOCKED )` of a track, arc or via: what a drag the person has agreed to (`DRAGGER::Start`:
    /// `startItem->Unmark( MK_LOCKED )`) does to the item it takes hold of.
    pub fn set_locked(&mut self, id: ItemId, locked: bool) {
        match self.items.get_mut(&id) {
            Some(Item::Segment(s)) => s.locked = locked,
            Some(Item::Arc(a)) => a.locked = locked,
            Some(Item::Via(v)) => v.locked = locked,
            _ => {}
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Every item but the virtual ones ([`crate::item::Via::is_virtual`]: they are not drawn, not committed, not picked, and collide with nothing;
    /// the joints have them, and [`Self::get`] finds them).
    pub fn iter(&self) -> impl Iterator<Item = (ItemId, &Item)> {
        self.items.iter().filter(|(_, it)| !it.is_virtual()).map(|(&id, it)| (id, it))
    }

    /// `NODE::FixupVirtualVias` (`pns_node.cpp:1270`, run by `ROUTER::SyncWorld`): a virtual via (`VVIA`) at every joint on one layer with no pad and no
    /// via where the width changes or where a locked segment ends -- and one at the other end of that locked segment too, pad or via there or not.
    /// Its diameter is the widest track's plus twice `PNS_HULL_MARGIN` (10 nm). As KiCad's, only
    /// segments are counted (an arc is no `SEGMENT`), "the locked one" is the last segment of the joint, and the solder mask, which KiCad compares
    /// too, is not in the model. **Where three segments meet there is none**: the condition reads `n_seg >= 3`, but KiCad's loop never counts a
    /// segment into `n_seg` (it stays 0), so a T of tracks of one width is no virtual via in KiCad's world -- and so not here (the review of
    /// 2026-10-23 found the first port counting them).
    pub fn fixup_virtual_vias(&mut self) {
        let mut vvias: Vec<Via> = Vec::new();
        let mut keys: Vec<&JointKey> = self.joints.keys().collect();
        keys.sort_by_key(|k| format!("{k:?}")); // one order, whatever the hash map's
        for key in keys {
            let joint = &self.joints[key];
            if joint.layers.start() != joint.layers.end() {
                continue; // `IsMultilayer()`
            }
            // `int n_seg = 0;` -- never incremented in KiCad's loop
            let n_seg = 0;
            let (mut n_solid, mut n_vias) = (0, 0);
            let (mut prev_w, mut max_w, mut is_width_change) = (-1, -1, false);
            let mut locked_seg: Option<&crate::item::Segment> = None;
            let mut is_locked = false;
            let mut pos: Option<Point> = None;
            let mut net = None;
            for id in &joint.links {
                match self.items.get(id) {
                    Some(Item::Via(_)) => n_vias += 1,
                    Some(Item::Solid(_)) => n_solid += 1,
                    Some(Item::Segment(s)) => {
                        let w = s.width;
                        // `if( prev_w < 0 ) prev_w = w; else if( w != prev_w ) is_width_change = true;`
                        if prev_w >= 0 && w != prev_w {
                            is_width_change = true;
                        }
                        max_w = max_w.max(w);
                        prev_w = w;
                        is_locked = s.locked;
                        locked_seg = Some(s);
                        net = Some(s.net.clone());
                        pos = Some(key.pos());
                    }
                    _ => {}
                }
            }
            let (Some(pos), Some(net)) = (pos, net) else { continue };
            let layer = joint.layers.start();
            let vvia = |at: Point| Via { net: net.clone(), layers: LayerRange::single(layer), pos: at, diameter: max_w + 2 * crate::hull::PNS_HULL_MARGIN, drill: (max_w + 2 * crate::hull::PNS_HULL_MARGIN) / 2, source_via: None, locked: false, is_virtual: true };
            if (is_width_change || n_seg >= 3 || is_locked) && n_solid == 0 && n_vias == 0 {
                vvias.push(vvia(pos));
            }
            if is_locked {
                if let Some(s) = locked_seg {
                    vvias.push(vvia(if s.a == pos { s.b } else { s.a }));
                }
            }
        }
        for v in vvias {
            self.add(Item::Via(v));
        }
    }

    fn bbox_of(item: &Item) -> (Nm, Nm, Nm, Nm) {
        let mut bbox: Option<(Nm, Nm, Nm, Nm)> = None;
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
        // a virtual item is in the joints but not in the index: "By default, virtual items cannot collide" (`QueryColliding`), so no spatial
        // query is to find it
        if !item.is_virtual() {
            self.index.insert(id, Self::bbox_of(&item));
        }
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

    /// `NODE::LockJoint( aPos, aItem, true )`: lock the joint of `net` at `pos`, if there is one (KiCad's `touchJoint` makes an empty one where
    /// there is none, which nothing can stop at and which goes when an item links to it and leaves again).
    pub fn lock_joint(&mut self, pos: Point, net: &Net) {
        if let Some(j) = self.joints.get_mut(&JointKey::new(pos, net)) {
            j.locked = true;
        }
    }

    /// `NODE::QueryJoints( bbox, joints, layers, ITEM::SOLID_T )` for what the optimizer's `KEEP_TOPOLOGY_CONSTRAINT` asks: the position and net of the joint of
    /// every pad (`Solid`) that is inside `bbox` (`x0, y0, x1, y1`) and on one of `layers`.
    pub fn solid_joints_in(&self, bbox: (Nm, Nm, Nm, Nm), layers: LayerRange) -> Vec<(Point, Net)> {
        let mut out: Vec<(Point, Net)> = Vec::new();
        for id in self.index.query(bbox) {
            let Some(Item::Solid(s)) = self.items.get(&id) else { continue };
            let p = s.pos;
            if p.x < bbox.0 || p.x > bbox.2 || p.y < bbox.1 || p.y > bbox.3 || !s.layers.overlaps(&layers) {
                continue;
            }
            if !out.iter().any(|(q, n)| *q == p && *n == s.net) {
                out.push((p, s.net.clone()));
            }
        }
        out.sort_by_key(|(p, _)| (p.x, p.y));
        out
    }

    /// `JOINT::NextSegment`: the item linked at `joint` other than
    /// `current`, if the joint is "trivial" for walk-through purposes --
    /// exactly one same-net, overlapping-layer `Segment` candidate, and no
    /// `Solid`/`Via` present at all (KiCad's rule: a pad or via at a joint
    /// is *always* a hard stop, even if a valid segment candidate also
    /// exists there -- see `pns_joint.h`'s `NextSegment`).
    fn next_segment(&self, joint: &Joint, current: ItemId, current_net: &Net, current_layers: LayerRange, follow_locked: bool) -> Option<ItemId> {
        let mut candidate = None;
        for &id in &joint.links {
            if id == current {
                continue;
            }
            match self.items.get(&id) {
                Some(item @ (Item::Segment(_) | Item::Arc(_))) => {
                    if same_net(item.net(), current_net) && item.layers().overlaps(&current_layers) {
                        if candidate.is_some() {
                            return None; // branch point: more than one same-net candidate
                        }
                        // `JOINT::NextSegment`: a locked neighbour is skipped as the
                        // continuation (but still counted above as a branch).
                        if !item.is_locked() || follow_locked {
                            candidate = Some(id);
                        }
                    }
                }
                // "Virtual via will be added at the joint between an unlocked and locked seg": followed through when locked segments are
                Some(Item::Via(v)) if v.is_virtual && follow_locked => {}
                Some(Item::Solid(_)) | Some(Item::Via(_)) => return None, // pad/via: always a hard stop
                _ => {}
            }
        }
        candidate
    }

    /// The points a walk along a track adds when it goes through item `id` entering it at `entry`: the item's polyline without the entry point
    /// (a segment's other end; an arc's interior points and then its other end, in the order the walk meets them), and where it leaves.
    fn walk_points(&self, id: ItemId, entry: Point) -> Option<(Vec<Point>, Point)> {
        match self.items.get(&id)? {
            Item::Segment(s) => {
                let other = if s.a == entry { s.b } else { s.a };
                Some((vec![other], other))
            }
            Item::Arc(a) => {
                let poly = a.polyline();
                if a.start() == entry {
                    Some((poly[1..].to_vec(), a.end()))
                } else {
                    Some((poly.iter().rev().skip(1).copied().collect(), a.start()))
                }
            }
            _ => None,
        }
    }

    /// `NODE::AssembleLine`: walk the joint graph outward from `start` (a
    /// `Segment` or an `Arc`) in both directions until a non-trivial joint,
    /// returning the flattened polyline plus the ordered item ids it came
    /// from -- one id per segment of the polyline, so an arc's id repeats
    /// for each chord of it -- and the arcs it passed through
    /// ([`Line::arcs`]). `None` if `start` isn't a track item (a lone `Via`
    /// has no line to assemble -- callers that want via continuity read
    /// `Item::Via` directly).
    pub fn assemble_line(&self, start: ItemId) -> Option<Line> {
        self.assemble_line_with(start, AssembleOpts::default())
    }

    /// [`Self::assemble_line`] with the two `NODE::AssembleLine` switches a
    /// shove needs: stop in front of a locked segment, and stop where the
    /// track width changes (so the line, and every segment the shove puts
    /// back, keeps one width -- see `shove.rs`'s note on `D10`).
    pub fn assemble_line_with(&self, start: ItemId, opts: AssembleOpts) -> Option<Line> {
        let start_item = self.items.get(&start)?;
        let (net, layer, width) = (start_item.net().clone(), start_item.layers().start(), start_item.width());
        let (a0, b0) = start_item.track_ends()?;
        let layers = LayerRange::single(layer);
        let mut arcs: Vec<ArcPts> = Vec::new();
        if let Item::Arc(a) = start_item {
            arcs.push(a.arc);
        }

        // Forward walk from b, backward walk from a; each produces an
        // ordered (point, item-just-traversed) list we splice together.
        let (first_pts, _) = self.walk_points(start, a0)?;
        let mut fwd_pts: Vec<Point> = first_pts;
        let mut fwd_ids: Vec<ItemId> = vec![start; fwd_pts.len()];
        let (mut cur, mut cur_end) = (start, b0);
        let mut via_end = None;
        // `NODE::followLine`'s `guard`: the walk returned to its start
        // anchor (a closed same-net loop) -- stop, and skip the backward
        // walk (`AssembleLine`: `if( !guardHit )`). `MaxVerts` caps the walk.
        let guard = b0;
        let mut guard_hit = false;
        while let Some(joint) = self.joint_at(cur_end, &net) {
            // `followLine`: "bool locked = aStopAtLockedJoints ? jt->IsLocked() : false; if( locked || .. ) break;"
            if opts.stop_at_locked_joints && joint.locked {
                break;
            }
            if let Some(next_id) = self.next_segment(joint, cur, &net, layers, opts.follow_locked_segments) {
                if !opts.allow_width_mismatch && self.items[&next_id].width() != width {
                    break; // the line ends where the track width changes
                }
                let Some((added, other_end)) = self.walk_points(next_id, cur_end) else { break };
                if next_id == start || other_end == guard || fwd_pts.len() >= ASSEMBLE_MAX_VERTS {
                    if next_id == start || other_end == guard {
                        // Close the loop at the guard point (KiCad
                        // records the re-traversed start segment's corner here).
                        if fwd_pts.last() != Some(&guard) {
                            fwd_pts.push(guard);
                        }
                        guard_hit = true;
                    }
                    break;
                }
                if let Item::Arc(a) = &self.items[&next_id] {
                    arcs.push(a.arc);
                }
                fwd_ids.extend(std::iter::repeat_n(next_id, added.len()));
                fwd_pts.extend(added);
                cur = next_id;
                cur_end = other_end;
            } else {
                via_end = joint.links.iter().find(|&&id| matches!(self.items.get(&id), Some(Item::Via(_)))).copied();
                break;
            }
        }

        let mut back_pts = vec![a0];
        let mut back_ids: Vec<ItemId> = Vec::new();
        let (mut cur, mut cur_end) = (start, a0);
        let mut via_start = None;
        // `AssembleLine`: only walk backward `if( !guardHit )` -- a closed loop was already
        // walked all the way round going forward.
        if !guard_hit {
            while let Some(joint) = self.joint_at(cur_end, &net) {
                if opts.stop_at_locked_joints && joint.locked {
                    break;
                }
                if let Some(next_id) = self.next_segment(joint, cur, &net, layers, opts.follow_locked_segments) {
                    if !opts.allow_width_mismatch && self.items[&next_id].width() != width {
                        break;
                    }
                    let Some((added, other_end)) = self.walk_points(next_id, cur_end) else { break };
                    if next_id == start || back_pts.len() >= ASSEMBLE_MAX_VERTS {
                        break;
                    }
                    if let Item::Arc(a) = &self.items[&next_id] {
                        arcs.push(a.arc);
                    }
                    back_ids.extend(std::iter::repeat_n(next_id, added.len()));
                    back_pts.extend(added);
                    cur = next_id;
                    cur_end = other_end;
                } else {
                    via_start = joint.links.iter().find(|&&id| matches!(self.items.get(&id), Some(Item::Via(_)))).copied();
                    break;
                }
            }
        }

        if guard_hit {
            back_pts.clear();
            back_ids.clear();
        }
        back_pts.reverse();
        back_ids.reverse();
        let mut pts = back_pts;
        pts.extend(fwd_pts);
        let mut segment_ids = back_ids;
        segment_ids.extend(fwd_ids);

        let mut line = Line::from_points(net, layer, width, pts);
        line.segment_ids = segment_ids;
        line.via_at_start = via_start;
        line.via_at_end = via_end;
        line.arcs = arcs;
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
            if !item.is_track() || !same_net(item.net(), net) {
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

    /// Insert `line` as a fresh chain of `Segment`s and `Arc`s (`NODE::Add(LINE&)`: an `ARC` for each arc of the line, a `SEGMENT` for each
    /// segment that is not one), without the redundant-segment check -- callers only ever add freshly-computed geometry, and
    /// [`Self::add_line_unless_redundant`] is the one with it. Returns the new item ids, one per segment of the line (an arc's id repeats for
    /// each chord of it, as in [`Line::segment_ids`]).
    pub fn add_line(&mut self, line: &Line, source_track: Option<(String, usize)>, locked: bool) -> Vec<ItemId> {
        let arc_source = source_track.as_ref().map(|(t, _)| t.clone());
        self.add_line_items(line, locked, |i, _| source_track.as_ref().map(|(t, _)| (t.clone(), i)), arc_source)
    }

    /// [`Self::add_line`] for a line that descends from IR track `track` -- a shove re-made it. The segments carry the track, numbered by
    /// their own ids (consecutive along the line, and far above the 0.. of a plain track), so a later shove in a world that has the
    /// line in it still knows which IR track it replaces, and [`Self::uncovered_track_runs`] still sees one run per line.
    pub fn add_line_tagged(&mut self, line: &Line, track: &str) -> Vec<ItemId> {
        const LINEAGE_INDEX_BASE: usize = 1 << 32;
        self.add_line_items(line, false, |_, id| Some((track.to_string(), LINEAGE_INDEX_BASE + id)), Some(track.to_string()))
    }

    /// The common part of the two: `seg_source( i, id )` is the `source_track` of the segment for the line's `i`th segment that is going to
    /// be item `id`; an arc of the line is one `Arc` item with `arc_source`.
    fn add_line_items(&mut self, line: &Line, locked: bool, seg_source: impl Fn(usize, usize) -> Option<(String, usize)>, arc_source: Option<String>) -> Vec<ItemId> {
        let runs = line.arc_runs();
        let mut ids = Vec::with_capacity(line.segment_count());
        let mut i = 0usize;
        while i < line.segment_count() {
            if let Some(r) = runs.iter().find(|r| r.first == i) {
                // an arc of the line: one item, its id for each chord
                let id = self.add(Item::Arc(crate::item::Arc::new(line.net.clone(), line.layer, line.width, line.arcs[r.arc], arc_source.clone(), locked)));
                ids.extend(std::iter::repeat_n(id, r.last - r.first));
                i = r.last;
                continue;
            }
            let (a, b) = (line.pts[i], line.pts[i + 1]);
            if a != b {
                // degenerate leg, same as KiCad's A==B rejection
                let source_track = seg_source(i, self.next_id as usize);
                ids.push(self.add(Item::Segment(crate::item::Segment { net: line.net.clone(), layer: line.layer, a, b, width: line.width, source_track, locked })));
            }
            i += 1;
        }
        ids
    }

    /// `NODE::Add( LINE&, aAllowRedundant = false )`: [`Self::add_line`], but a segment that is already in the node -- the same two ends on the same
    /// layer and net (`findRedundantSegment`) -- is not added again: the line links the existing one instead, as does an arc already there
    /// (`findRedundantArc`). A shoved line that retraces a track would otherwise stack a copy of it on the original.
    pub fn add_line_unless_redundant(&mut self, line: &Line, source_track: Option<(String, usize)>, locked: bool) -> Vec<ItemId> {
        let runs = line.arc_runs();
        let mut ids = Vec::with_capacity(line.segment_count());
        let mut i = 0usize;
        while i < line.segment_count() {
            if let Some(r) = runs.iter().find(|r| r.first == i) {
                let arc = line.arcs[r.arc];
                let id = match self.find_redundant_arc(arc.start, arc.end, line.layer, &line.net) {
                    Some(existing) => existing,
                    None => self.add(Item::Arc(crate::item::Arc::new(line.net.clone(), line.layer, line.width, arc, source_track.clone().map(|(t, _)| t), locked))),
                };
                ids.extend(std::iter::repeat_n(id, r.last - r.first));
                i = r.last;
                continue;
            }
            let (a, b) = (line.pts[i], line.pts[i + 1]);
            if a != b {
                let id = match self.find_redundant_segment(a, b, line.layer, &line.net) {
                    Some(existing) => existing,
                    None => self.add(Item::Segment(crate::item::Segment { net: line.net.clone(), layer: line.layer, a, b, width: line.width, source_track: source_track.clone().map(|(t, _)| (t, i)), locked })),
                };
                // "another line could be referencing this segment too": a segment is linked once
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            i += 1;
        }
        ids
    }

    /// `NODE::findRedundantSegment( A, B, lr, aNet )` (`pns_node.cpp:1707`): a segment of the node on layer `layer` and net `net` that joins `a` and
    /// `b`, in either direction -- found at the joint at `a`.
    pub fn find_redundant_segment(&self, a: Point, b: Point, layer: i32, net: &Net) -> Option<ItemId> {
        let joint = self.joint_at(a, net)?;
        joint.links.iter().copied().find(|id| match self.items.get(id) {
            Some(Item::Segment(s)) => s.layer == layer && ((s.a == a && s.b == b) || (s.a == b && s.b == a)),
            _ => false,
        })
    }

    /// `NODE::findRedundantArc` (`:1742`): an arc of the node on `layer` and `net` with the ends `a` and `b`, in either direction.
    pub fn find_redundant_arc(&self, a: Point, b: Point, layer: i32, net: &Net) -> Option<ItemId> {
        let joint = self.joint_at(a, net)?;
        joint.links.iter().copied().find(|id| match self.items.get(id) {
            Some(Item::Arc(x)) => x.layer == layer && ((x.start() == a && x.end() == b) || (x.start() == b && x.end() == a)),
            _ => false,
        })
    }

    // ---------------------------------------------------------------- a track in pieces

    /// The runs of IR track `track`'s segments that are not in `covered`, as this node has them: consecutive segments (by their index
    /// in the track) that join end to end, with one width and layer, are one run. What an edit left of a track it replaced only in
    /// part has to be handed back, or the commit, which replaces whole IR tracks, would delete it.
    pub fn uncovered_track_runs(&self, track: &str, covered: &std::collections::HashSet<ItemId>) -> Vec<Line> {
        let mut segs: Vec<(usize, ItemId, &crate::item::Segment)> = self
            .items
            .iter()
            .filter_map(|(&id, it)| match it {
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

    /// IR track `track` as the one polyline it is, its segments in order -- `None` when this node has none of it, or its segments do
    /// not join end to end (a track a shove has since cut).
    pub fn track_polyline(&self, track: &str) -> Option<Line> {
        let mut segs: Vec<(usize, ItemId, &crate::item::Segment)> = self
            .items
            .iter()
            .filter_map(|(&id, it)| match it {
                Item::Segment(s) if s.source_track.as_ref().is_some_and(|(t, _)| t == track) => Some((s.source_track.as_ref().unwrap().1, id, s)),
                _ => None,
            })
            .collect();
        segs.sort_by_key(|&(idx, id, _)| (idx, id));
        let first = segs.first()?.2;
        let mut line = Line::from_points(first.net.clone(), first.layer, first.width, vec![first.a]);
        for (_, id, s) in &segs {
            if line.last() != Some(s.a) {
                return None;
            }
            line.pts.push(s.b);
            line.segment_ids.push(*id);
        }
        Some(line)
    }

    /// `LINE_PLACER::SplitAdjacentSegments( aNode, aSeg, aP )`: segment `id` is taken out and two put in its place, one from its start to
    /// `p` and one from `p` to its end, both with the old one's net, layer, width and track -- so a route can start or end in the middle
    /// of a track, where a joint is then. Not done (`None`) when something already joins at `p` (`jt && jt->LinkCount() >= 1`: `p` is
    /// one of the segment's own ends, or another track ends on it), or `p` is not on the segment. The IR track the segment is part of
    /// gets a vertex at `p`: the second half is the next segment of the track, and the ones after it move up by one.
    pub fn split_segment(&mut self, id: ItemId, p: Point) -> Option<(ItemId, ItemId)> {
        let old = self.segment_to_split(id, p)?;
        if let Some((track, k)) = &old.source_track {
            for it in self.items.values_mut() {
                if let Item::Segment(s) = it {
                    if let Some((t, i)) = &mut s.source_track {
                        if t == track && *i > *k {
                            *i += 1;
                        }
                    }
                }
            }
        }
        self.remove(id);
        let first = crate::item::Segment { b: p, ..old.clone() };
        let second = crate::item::Segment { a: p, source_track: old.source_track.clone().map(|(t, k)| (t, k + 1)), ..old };
        let a = self.add(Item::Segment(first));
        let b = self.add(Item::Segment(second));
        Some((a, b))
    }

    /// The segment [`Self::split_segment`] would split at `p`, if it would (the checks of `SplitAdjacentSegments`).
    pub fn segment_to_split(&self, id: ItemId, p: Point) -> Option<crate::item::Segment> {
        let Some(Item::Segment(old)) = self.items.get(&id) else { return None };
        if p == old.a || p == old.b || old.seg().sq_distance_to_point(p) > 4 {
            return None;
        }
        if let Some(j) = self.joint_at(p, &old.net) {
            if j.links.iter().any(|l| self.items.get(l).is_some_and(|it| it.layers().overlaps_layer(old.layer))) {
                return None;
            }
        }
        Some(old.clone())
    }

    /// The segment or arc whose copper `pos` is on, on `layer` (of `net`, when given): the one whose centre line passes nearest. `within` is how
    /// far off the centre line `pos` may be -- the half width when `None`. The items in `exclude` are not looked at.
    pub fn segment_at(&self, pos: Point, layer: i32, net: Option<&Net>, within: Option<Nm>, exclude: &[ItemId]) -> Option<ItemId> {
        let bbox = Shape::Circle { c: pos, r: 0 }.bbox(SEGMENT_AT_REACH);
        let mut best: Option<(ItemId, i128)> = None;
        for id in self.index.query(bbox) {
            let Some(item) = self.items.get(&id) else { continue };
            // the squared distance from the centre line, and the width it is measured against
            let (d2, width, item_layer) = match item {
                Item::Segment(s) => (s.seg().sq_distance_to_point(pos), s.width, s.layer),
                Item::Arc(a) => {
                    let d = a.arc.distance_to(pos);
                    ((d * d).round() as i128, a.width, a.layer)
                }
                _ => continue,
            };
            if item_layer != layer || net.is_some_and(|n| !same_net(item.net(), n)) || exclude.contains(&id) {
                continue;
            }
            let reach = within.unwrap_or(width / 2) as i128;
            if d2 <= reach * reach && best.is_none_or(|(bid, bd)| (d2, id) < (bd, bid)) {
                best = Some((id, d2));
            }
        }
        best.map(|(id, _)| id)
    }

    /// `LINE_PLACER::SplitAdjacentArcs( aNode, aArc, aP )`: arc `id` is taken out and two put in its place, one from its start to `p` and one from `p`
    /// to its end, both on its circle and with its net, layer, width and track -- so a route can start or end in the middle of an arc track. Not
    /// done (`None`) when something already joins at `p` (`jt && jt->LinkCount() >= 1`), or `p` is not on the arc.
    pub fn split_arc(&mut self, id: ItemId, p: Point) -> Option<(ItemId, ItemId)> {
        let old = self.arc_to_split(id, p)?;
        let (first, second) = old.arc.split(p)?;
        self.remove(id);
        let make = |arc: ArcPts| Item::Arc(crate::item::Arc::new(old.net.clone(), old.layer, old.width, arc, old.source_track.clone(), old.locked));
        let a = self.add(make(first));
        let b = self.add(make(second));
        Some((a, b))
    }

    /// The arc [`Self::split_arc`] would split at `p`, if it would (the checks of `SplitAdjacentArcs`).
    pub fn arc_to_split(&self, id: ItemId, p: Point) -> Option<crate::item::Arc> {
        let Some(Item::Arc(old)) = self.items.get(&id) else { return None };
        if p == old.start() || p == old.end() || old.arc.is_degenerate() || old.arc.distance_to(p) > 2.0 {
            return None;
        }
        if let Some(j) = self.joint_at(p, &old.net) {
            if j.links.iter().any(|l| self.items.get(l).is_some_and(|it| it.layers().overlaps_layer(old.layer))) {
                return None;
            }
        }
        Some(old.clone())
    }

    // ---------------------------------------------------------------- topology

    /// `TOPOLOGY::ConnectedJoints( aStart )`: the joints reachable from the joint at `start` by walking along segments -- track to
    /// track, through junctions, pads and vias alike -- as the set of positions.
    fn connected_joints(&self, start: Point, net: &Net) -> std::collections::HashSet<(Nm, Nm)> {
        let mut seen: std::collections::HashSet<(Nm, Nm)> = std::collections::HashSet::new();
        let mut queue: std::collections::VecDeque<Point> = std::collections::VecDeque::new();
        seen.insert((start.x, start.y));
        queue.push_back(start);
        while let Some(pos) = queue.pop_front() {
            let Some(joint) = self.joint_at(pos, net) else { continue };
            for &id in &joint.links {
                if let Some((a, b)) = self.items.get(&id).and_then(Item::track_ends) {
                    let next = if a == pos { b } else { a };
                    if seen.insert((next.x, next.y)) {
                        queue.push_back(next);
                    }
                }
            }
        }
        seen
    }

    /// `TOPOLOGY::NearestUnconnectedItem( aStart )`: of the items on `net` that the joint at `start` does not reach along tracks
    /// ([`Self::connected_joints`]), the one with an anchor nearest to `start` -- `(its id, that anchor)`. The first of equals, by id.
    pub fn nearest_unconnected_item(&self, start: Point, net: &Net) -> Option<(ItemId, Point)> {
        let reached = self.connected_joints(start, net);
        let mut connected: std::collections::HashSet<ItemId> = std::collections::HashSet::new();
        for &(x, y) in &reached {
            if let Some(j) = self.joint_at(Point { x, y }, net) {
                connected.extend(j.links.iter().copied());
            }
        }
        let mut best: Option<(i64, ItemId, Point)> = None;
        let mut ids: Vec<ItemId> = self.items.iter().filter(|(id, it)| same_net_exact(it.net(), net) && !connected.contains(id) && !it.is_virtual() && !matches!(it, Item::Solid(s) if !s.routable())).map(|(&id, _)| id).collect();
        ids.sort_unstable();
        for id in ids {
            for a in self.items[&id].anchors() {
                let d = crate::seg_ops::length(a, start);
                if best.is_none_or(|(bd, _, _)| d < bd) {
                    best = Some((d, id, a));
                }
            }
        }
        best.map(|(_, id, a)| (id, a))
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
    pub fn clearance(rules: &BoardRules, a: &Net, b: &Net) -> Nm {
        eda_drc::constraints::clearance(rules, Self::net_str(a), Self::net_str(b))
    }

    /// `PNS_PCBNEW_RULE_RESOLVER::Clearance( aA, aB )` for a copper item on `net` against `item`: the clearance
    /// [`Node::clearance`] gives for two nets, except against the board outline. An Edge.Cuts graphic is not copper
    /// (`isCopper` is false for it), so `CT_CLEARANCE` is never asked; `isEdge( aB )` asks `CT_EDGE_CLEARANCE`, which is
    /// the board's copper-to-edge clearance (`bds.m_CopperEdgeClearance`, [`eda_drc::constraints::edge_clearance_min`]) --
    /// whatever net class the copper is in.
    pub fn clearance_to(rules: &BoardRules, net: &Net, item: &Item) -> Nm {
        match item {
            Item::Solid(s) if s.edge => eda_drc::constraints::edge_clearance_min(rules),
            _ => Self::clearance(rules, net, item.net()),
        }
    }

    /// How far from an item the search for what it could collide with has to look: the largest clearance anything can ask for
    /// (the net classes', the holes', and the board edge's).
    fn search_margin(rules: &BoardRules) -> Nm {
        eda_drc::constraints::worst_case_clearance(rules).max(eda_drc::constraints::edge_clearance_min(rules))
    }

    /// `NODE::QueryColliding`/`ITEM::Collide`, narrowed to "every obstacle
    /// a candidate shape on `layers`, net `net`, would hit" -- the broad
    /// phase (spatial index, inflated by the board's worst-case clearance)
    /// narrowed by the exact test (`Shape::collides`), skipping same-net
    /// items, non-overlapping layers, and anything in `exclude` (the
    /// query's own in-flight items, so a line never collides with its own
    /// soon-to-be-replaced segments).
    pub fn all_colliding(&self, shape: &Shape, net: &Net, layers: LayerRange, rules: &BoardRules, exclude: &[ItemId]) -> Vec<Obstacle> {
        self.colliding_with(shape, net, layers, rules, exclude, QueryOpts::default())
    }

    /// [`Self::all_colliding`] with `COLLISION_SEARCH_OPTIONS`: the kind mask
    /// and filter are tested first (`DEFAULT_OBSTACLE_VISITOR::operator()`),
    /// and `use_epsilon` switches `m_useClearanceEpsilon`.
    pub fn colliding_with(&self, shape: &Shape, net: &Net, layers: LayerRange, rules: &BoardRules, exclude: &[ItemId], opts: QueryOpts) -> Vec<Obstacle> {
        let margin = Self::search_margin(rules);
        let bbox = shape.bbox(margin);
        let mut out = Vec::new();
        for id in self.index.query(bbox) {
            if exclude.contains(&id) {
                continue;
            }
            let Some(item) = self.items.get(&id) else { continue };
            if kind_bit(item.kind()) & opts.kind_mask == 0 {
                continue;
            }
            if let Some(filter) = opts.filter {
                if !filter(id) {
                    continue;
                }
            }
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
            let clearance = Self::clearance_to(rules, net, item);
            let clearance = if opts.use_epsilon && clearance > 0 { (clearance - CLEARANCE_EPSILON).max(0) } else { clearance };
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

    /// `NODE::NearestObstacle( aLine, aOpts )`: query every leg of `line` (and
    /// its `via`, if the line ends in one), then pick the obstacle whose
    /// clearance hull the line enters first -- by path length along the line,
    /// not by distance. `exclude` are the line's own segments in this node.
    pub fn nearest_obstacle(&self, line: &Line, via: Option<&crate::item::Via>, rules: &BoardRules, exclude: &[ItemId], opts: QueryOpts) -> Option<NearestHit> {
        let layers = LayerRange::single(line.layer);
        // `OBSTACLES` is a `std::set`: one entry per item, a stable order here by id.
        let mut found: std::collections::BTreeMap<ItemId, Obstacle> = std::collections::BTreeMap::new();
        for (a, b) in line.segs() {
            let shape = Shape::Stadium { a, b, r: line.width / 2 };
            for o in self.colliding_with(&shape, &line.net, layers, rules, exclude, opts) {
                found.entry(o.id).or_insert(o);
            }
        }
        if let Some(v) = via.filter(|v| !v.is_virtual) {
            let shape = Shape::Circle { c: v.pos, r: v.diameter / 2 };
            for o in self.colliding_with(&shape, &v.net, v.layers, rules, exclude, opts) {
                found.entry(o.id).or_insert(o);
            }
        }
        if found.is_empty() {
            return None;
        }
        let eps = |c: Nm| if opts.use_epsilon && c > 0 { (c - CLEARANCE_EPSILON).max(0) } else { c };
        let first_entry = |hull: &[Point]| -> Option<(i64, Point)> {
            crate::line_walk::hull_intersection(hull, &line.pts).into_iter().map(|(p, idx)| (path_length(&line.pts, p, idx), p)).min_by_key(|&(d, _)| d)
        };
        let mut best: Option<NearestHit> = None;
        for (id, o) in &found {
            let item = &self.items[id];
            // `GetClearance( item, aLine ) + aLine->Width() / 2`, hull thickness 0.
            // `makeHull`: the bounding box of the hull in a 90-degree corner mode (`simplifyHull`)
            let make_hull = |h: Vec<Point>| if opts.box_hull { crate::hull::bbox_hull(&h) } else { h };
            let hull = make_hull(item.hull(eps(Self::clearance_to(rules, &line.net, item)) + line.width / 2, 0, line.layer));
            let mut dist: Option<(i64, Point)> = first_entry(&hull);
            if let Some(v) = via {
                let via_hull = make_hull(item.hull(eps(Self::clearance_to(rules, &v.net, item)) + v.diameter / 2, 0, line.layer));
                if let Some(d) = first_entry(&via_hull) {
                    if dist.is_none_or(|(bd, _)| d.0 < bd) {
                        dist = Some(d);
                    }
                }
            }
            if let Some((d, ip)) = dist {
                if best.as_ref().is_none_or(|b| d < b.dist_first) {
                    best = Some(NearestHit { id: o.id, kind: o.kind, dist_first: d, ip });
                    if d == 0 {
                        break;
                    }
                }
            }
        }
        // `if( nearest.m_distFirst == INT_MAX ) nearest = obstacles[0];`
        best.or_else(|| found.values().next().map(|o| NearestHit { id: o.id, kind: o.kind, dist_first: i64::MAX, ip: o.pos }))
    }

    /// `QueryColliding` with `m_differentNetsOnly = false` and
    /// `m_overrideClearance = 0` (what `TOPOLOGY::AssembleCluster` asks): every
    /// item, whatever its net, whose shape touches `shape` on `layers`.
    pub fn overlapping(&self, shape: &Shape, layers: LayerRange, exclude: &[ItemId]) -> Vec<ItemId> {
        let bbox = shape.bbox(0);
        let mut out = Vec::new();
        for id in self.index.query(bbox) {
            if exclude.contains(&id) {
                continue;
            }
            let Some(item) = self.items.get(&id) else { continue };
            if !item.layers().overlaps(&layers) {
                continue;
            }
            if shape.collides(&item.shape(item.layers().start()), 0).is_some() {
                out.push(id);
            }
        }
        out.sort_unstable();
        out
    }

    /// `TOPOLOGY::AssembleCluster( aStart, aLayer, aAreaExpansionLimit,
    /// aExcludedNet )` (`pns_topology.cpp:1142`): the blob of items that touch
    /// `start`, transitively -- pads, vias, and the tracks attached to them --
    /// that a line must go around as a whole. `skip` are items that never
    /// join (the head's own segments, `MK_HEAD`). The cluster is returned in
    /// the order the breadth-first walk found it, `start` first.
    pub fn assemble_cluster(&self, start: ItemId, layer: i32, area_expansion_limit: f64, excluded_net: Option<&Net>, skip: &dyn Fn(ItemId) -> bool) -> Vec<ItemId> {
        let Some(start_item) = self.items.get(&start) else { return Vec::new() };
        let mut cluster: Vec<ItemId> = Vec::new();
        let mut pending: std::collections::VecDeque<ItemId> = std::collections::VecDeque::new();
        pending.push_back(start);
        let bbox_of_shape = |it: &Item| it.shape(layer).bbox(0);
        let merge = |a: (Nm, Nm, Nm, Nm), b: (Nm, Nm, Nm, Nm)| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3));
        let area = |b: (Nm, Nm, Nm, Nm)| (b.2 - b.0) as i128 * (b.3 - b.1) as i128;
        let mut cluster_bbox = bbox_of_shape(start_item);
        let initial_area = area(cluster_bbox);
        let mut processed: std::collections::HashSet<ItemId> = std::collections::HashSet::new();
        while let Some(top) = pending.pop_front() {
            if !processed.contains(&top) {
                cluster.push(top);
            }
            processed.insert(top);
            let Some(top_item) = self.items.get(&top) else { continue };
            let top_shape = top_item.shape(layer);
            let touching = self.overlapping(&top_shape, top_item.layers(), &[top]);
            for obs_id in touching {
                let Some(obs) = self.items.get(&obs_id) else { continue };
                // different-net track-on-track contact is not a cluster
                let track_on_track = !same_net_exact(obs.net(), top_item.net()) && obs.is_track() && top_item.is_track();
                if track_on_track {
                    continue;
                }
                if let Some(ex) = excluded_net {
                    if ex.is_some() && obs.net() == ex {
                        continue;
                    }
                }
                if obs.is_track() && obs.layers().overlaps_layer(layer) {
                    if let Some(l) = self.assemble_line(obs_id) {
                        if let Some(b) = line_bbox(&l) {
                            cluster_bbox = merge(cluster_bbox, b);
                        }
                    }
                } else {
                    cluster_bbox = merge(cluster_bbox, bbox_of_shape(obs));
                }
                let ratio = area(cluster_bbox) as f64 / (initial_area + 1) as f64;
                if area_expansion_limit > 0.0 && ratio > area_expansion_limit {
                    break;
                }
                if !processed.contains(&obs_id) && obs.layers().overlaps_layer(layer) && !skip(obs_id) {
                    processed.insert(obs_id);
                    cluster.push(obs_id);
                    pending.push_back(obs_id);
                }
            }
        }
        cluster
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
    pub fn nearest_anchor(&self, pos: Point, layers: LayerRange, max_dist: Nm, net_filter: Option<&Net>) -> Option<(ItemId, Point)> {
        let mut best: Option<(ItemId, Point, i128)> = None;
        for (&id, item) in &self.items {
            if !item.layers().overlaps(&layers) || item.is_virtual() {
                continue;
            }
            if let Some(f) = net_filter {
                if !same_net(item.net(), f) {
                    continue;
                }
            }
            for a in item.anchors() {
                let d2 = (a.x - pos.x) as i128 * (a.x - pos.x) as i128 + (a.y - pos.y) as i128 * (a.y - pos.y) as i128;
                // equal distances (a track ending exactly on a pad's centre) are resolved by
                // item id, not by the order the item table happens to iterate in -- the start
                // item decides the starting posture, so a random pick made routes unrepeatable
                if d2 <= (max_dist as i128) * (max_dist as i128) && best.as_ref().map(|(bid, _, bd)| (d2, id) < (*bd, *bid)).unwrap_or(true) {
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
    pub fn item_at(&self, pos: Point, layers: LayerRange, max_dist: Nm) -> Option<ItemId> {
        let probe = Shape::Circle { c: pos, r: 0 };
        let bbox = probe.bbox(max_dist);
        let mut best: Option<(ItemId, Nm)> = None;
        for id in self.index.query(bbox) {
            let Some(item) = self.items.get(&id) else { continue };
            if !item.layers().overlaps(&layers) || matches!(item, Item::Solid(s) if !s.routable()) {
                continue; // the board outline and a copper text are obstacles, never something to pick up
            }
            let (actual, _) = probe.clearance_to(&item.shape(item.layers().start()));
            if actual <= max_dist && best.as_ref().map(|(_, d)| actual < *d).unwrap_or(true) {
                best = Some((id, actual));
            }
        }
        best.map(|(id, _)| id)
    }
}

/// `Net` equality as `ITEM::Net() == ITEM::Net()` (a pointer compare): two
/// unconnected (`None`) items are on the same "net" here.
fn same_net_exact(a: &Net, b: &Net) -> bool {
    a == b
}

/// Bounding box of a line's points, inflated by half its width.
fn line_bbox(l: &Line) -> Option<(Nm, Nm, Nm, Nm)> {
    let first = l.pts.first()?;
    let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x, first.y);
    for p in &l.pts {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    Some((x0, y0, x1, y1))
}

/// `SHAPE_LINE_CHAIN::PathLength( aP, aIndex )`: the length along the chain
/// from its start to `p`, where `p` lies on segment `index` (or at vertex
/// `index`, which is the start of segment `index`; `index == segments` means
/// the last segment). Segment lengths are rounded like `SEG::Length()`.
pub(crate) fn path_length(pts: &[Point], p: Point, index: usize) -> i64 {
    let n_segs = pts.len().saturating_sub(1);
    let dist = |a: Point, b: Point| -> i64 { (((b.x - a.x) as f64).powi(2) + ((b.y - a.y) as f64).powi(2)).sqrt().round() as i64 };
    let mut sum = 0;
    for i in 0..n_segs {
        let hit = if index == n_segs { i == n_segs - 1 } else { i == index };
        if hit {
            return sum + dist(p, pts[i]);
        }
        sum += dist(pts[i], pts[i + 1]);
    }
    -1
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Node({} items, {} joints)", self.items.len(), self.joints.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{net_of, Segment, Solid, Via};
    use eda_model::ir::Point;

    fn p(x: Nm, y: Nm) -> Point {
        Point { x, y }
    }

    fn rules() -> BoardRules {
        serde_yaml::from_str("track_width: 200000\nclearance: 200000\nvia_drill: 300000\nvia_diameter: 600000\n").unwrap()
    }

    #[test]
    fn add_remove_round_trip() {
        let mut n = Node::new();
        let id = n.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: p(0, 0), b: p(1_000_000, 0), width: 200_000, source_track: None, locked: false }));
        assert_eq!(n.len(), 1);
        assert!(n.joint_at(p(0, 0), &net_of("GND")).is_some());
        let removed = n.remove(id).unwrap();
        assert!(matches!(removed, Item::Segment(_)));
        assert_eq!(n.len(), 0);
        assert!(n.joint_at(p(0, 0), &net_of("GND")).is_none(), "joint must be pruned once empty");
    }

    /// D2: a closed same-net loop must terminate (followLine's guard).
    #[test]
    fn assemble_line_terminates_on_closed_loop() {
        let mut n = Node::new();
        let net = net_of("GND");
        let corners = [p(0, 0), p(1_000_000, 0), p(1_000_000, 1_000_000), p(0, 1_000_000)];
        let mut ids = vec![];
        for i in 0..4 {
            ids.push(n.add(Item::Segment(Segment { net: net.clone(), layer: 0, a: corners[i], b: corners[(i + 1) % 4], width: 200_000, source_track: None, locked: false })));
        }
        let line = n.assemble_line(ids[0]).unwrap();
        assert_eq!(line.segment_ids.len(), 4);
        assert_eq!(line.first(), line.last(), "closed loop");
        assert_eq!(line.pts.len(), 5);
        // Every start segment terminates too.
        for id in ids {
            assert_eq!(n.assemble_line(id).unwrap().segment_ids.len(), 4);
        }
    }

    #[test]
    fn assemble_line_walks_through_trivial_joints_and_stops_at_via() {
        let mut n = Node::new();
        let net = net_of("SIG");
        let s1 = n.add(Item::Segment(Segment { net: net.clone(), layer: 0, a: p(0, 0), b: p(1_000_000, 0), width: 200_000, source_track: None, locked: false }));
        let _s2 = n.add(Item::Segment(Segment { net: net.clone(), layer: 0, a: p(1_000_000, 0), b: p(2_000_000, 0), width: 200_000, source_track: None, locked: false }));
        let _via = n.add(Item::Via(Via { net: net.clone(), layers: LayerRange::new(0, 1), pos: p(2_000_000, 0), diameter: 600_000, drill: 300_000, source_via: None, locked: false, is_virtual: false }));
        let line = n.assemble_line(s1).expect("line");
        assert_eq!(line.pts, vec![p(0, 0), p(1_000_000, 0), p(2_000_000, 0)]);
        assert_eq!(line.segment_ids.len(), 2);
        assert!(line.via_at_end.is_some());
    }

    #[test]
    fn assemble_line_stops_at_branch_point() {
        let mut n = Node::new();
        let net = net_of("SIG");
        let s1 = n.add(Item::Segment(Segment { net: net.clone(), layer: 0, a: p(0, 0), b: p(1_000_000, 0), width: 200_000, source_track: None, locked: false }));
        // Two different segments continuing from (1000,0): a branch, not a
        // trivial pass-through, so the walk must stop at s1 alone.
        n.add(Item::Segment(Segment { net: net.clone(), layer: 0, a: p(1_000_000, 0), b: p(2_000_000, 0), width: 200_000, source_track: None, locked: false }));
        n.add(Item::Segment(Segment { net: net.clone(), layer: 0, a: p(1_000_000, 0), b: p(1_000_000, 1_000_000), width: 200_000, source_track: None, locked: false }));
        let line = n.assemble_line(s1).unwrap();
        assert_eq!(line.pts, vec![p(0, 0), p(1_000_000, 0)]);
    }

    #[test]
    fn collision_ignores_same_net_and_excluded() {
        let mut n = Node::new();
        let rules = rules();
        let pad = n.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: p(0, 0), shape: Shape::Circle { c: p(0, 0), r: 500_000 }, source: "U1.1".into(), edge: false }));
        let shape = Shape::Stadium { a: p(0, 0), b: p(1_000_000, 0), r: 100_000 };
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
        n.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: p(1_000_000, 1_000_000), shape: Shape::Circle { c: p(1_000_000, 1_000_000), r: 400_000 }, source: "U1.1".into(), edge: false }));
        let hit = n.nearest_anchor(p(1_050_000, 1_000_000), LayerRange::single(0), 200_000, None);
        assert_eq!(hit.map(|(_, p)| p), Some(p(1_000_000, 1_000_000)));
        assert!(n.nearest_anchor(p(5_000_000, 5_000_000), LayerRange::single(0), 200_000, None).is_none());
    }

    /// A track ending exactly on a pad's centre: which of the two a route starts from decides its starting
    /// posture, so it must not depend on the item table's iteration order (which differs per clone).
    #[test]
    fn nearest_anchor_breaks_ties_by_item_id() {
        // a fresh `Node` per round: every `HashMap` gets its own random hasher, so an answer that
        // followed the table's iteration order would differ from round to round
        for _ in 0..40 {
            let mut n = Node::new();
            let pad = n.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: p(1_000_000, 1_000_000), shape: Shape::Circle { c: p(1_000_000, 1_000_000), r: 400_000 }, source: "U1.1".into(), edge: false }));
            n.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: p(1_000_000, 1_000_000), b: p(3_000_000, 1_000_000), width: 200_000, source_track: None, locked: false }));
            assert_eq!(n.nearest_anchor(p(1_100_000, 1_000_000), LayerRange::single(0), 500_000, None).map(|(id, _)| id), Some(pad));
        }
    }

    #[test]
    fn branch_is_independent_clone() {
        let mut n = Node::new();
        let id = n.add(Item::Via(Via { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: p(0, 0), diameter: 600_000, drill: 300_000, source_via: None, locked: false, is_virtual: false }));
        let mut child = n.branch();
        child.remove(id);
        assert!(n.contains(id), "removing from a branch must not affect the parent");
        assert!(!child.contains(id));
    }

    fn seg_item(net: &str, a: Point, b: Point, width: Nm) -> Item {
        Item::Segment(Segment { net: net_of(net), layer: 0, a, b, width, source_track: None, locked: false })
    }

    fn pad_item(net: &str, c: Point, r: Nm) -> Item {
        Item::Solid(Solid { net: net_of(net), layers: LayerRange::new(0, 1), pos: c, shape: Shape::Circle { c, r }, source: "P".into(), edge: false })
    }

    /// D12: `NearestObstacle` is the first hull the line enters, not the item it overlaps most.
    #[test]
    fn nearest_obstacle_is_the_first_hull_along_the_line_not_the_deepest_overlap() {
        let mut n = Node::new();
        let rules = rules();
        // a pad dead on the line far along it (gap 0) ...
        n.add(pad_item("A", p(6_000_000, 0), 400_000));
        // ... and a track whose end only grazes the line much earlier (gap 50 < 200)
        let early = n.add(seg_item("B", p(2_000_000, 250_000), p(2_000_000, 3_000_000), 200_000));
        let line = Line::from_points(net_of("SIG"), 0, 200_000, vec![p(0, 0), p(10_000_000, 0)]);
        let hit = n.nearest_obstacle(&line, None, &rules, &[], QueryOpts::default()).expect("obstacles");
        assert_eq!(hit.id, early);
        assert_eq!(hit.kind, Kind::Segment);
        // the line enters the track's hull (200 + 100 + 100 from its centre line) at x = 2000 - 400 + a bit
        assert!(hit.dist_first > 1_500_000 && hit.dist_first < 2_000_000, "{}", hit.dist_first);
        // searching only pads skips the track, as `shoveIteration`'s per-kind search does
        let only_pads = n.nearest_obstacle(&line, None, &rules, &[], QueryOpts { kind_mask: kind_mask::SOLID, ..QueryOpts::default() }).unwrap();
        assert_eq!(only_pads.kind, Kind::Solid);
    }

    #[test]
    fn nearest_obstacle_filter_and_exclude() {
        let mut n = Node::new();
        let rules = rules();
        let a = n.add(pad_item("A", p(2_000_000, 0), 300_000));
        let b = n.add(pad_item("B", p(5_000_000, 0), 300_000));
        let line = Line::from_points(net_of("SIG"), 0, 200_000, vec![p(0, 0), p(8_000_000, 0)]);
        assert_eq!(n.nearest_obstacle(&line, None, &rules, &[], QueryOpts::default()).unwrap().id, a);
        assert_eq!(n.nearest_obstacle(&line, None, &rules, &[a], QueryOpts::default()).unwrap().id, b);
        let only_b = |id: ItemId| id == b;
        assert_eq!(n.nearest_obstacle(&line, None, &rules, &[], QueryOpts { filter: Some(&only_b), ..QueryOpts::default() }).unwrap().id, b);
        assert!(n.nearest_obstacle(&Line::from_points(net_of("SIG"), 0, 200_000, vec![p(0, 3_000_000), p(8_000_000, 3_000_000)]), None, &rules, &[], QueryOpts::default()).is_none());
    }

    /// A line ending in a via is searched for that via as well.
    #[test]
    fn nearest_obstacle_includes_the_vias_own_collisions() {
        let mut n = Node::new();
        let rules = rules();
        let pad = n.add(pad_item("A", p(1_000_000, 3_000_000), 300_000));
        let line = Line::from_points(net_of("SIG"), 0, 200_000, vec![p(0, 0), p(1_000_000, 0)]);
        assert!(n.nearest_obstacle(&line, None, &rules, &[], QueryOpts::default()).is_none(), "the track alone is clear of the pad");
        let via = crate::item::Via { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: p(1_000_000, 2_500_000), diameter: 600_000, drill: 300_000, source_via: None, locked: false, is_virtual: false };
        assert_eq!(n.nearest_obstacle(&line, Some(&via), &rules, &[], QueryOpts::default()).unwrap().id, pad);
    }

    #[test]
    fn path_length_counts_whole_legs_before_the_hit_leg() {
        let pts = [p(0, 0), p(1_000_000, 0), p(1_000_000, 500_000)];
        assert_eq!(path_length(&pts, p(400_000, 0), 0), 400_000);
        assert_eq!(path_length(&pts, p(1_000_000, 200_000), 1), 1_200_000);
        assert_eq!(path_length(&pts, p(1_000_000, 500_000), 2), 1_500_000, "an index one past the last leg means the last leg");
    }

    /// D10: a shove assembles a line of one width, and never through a locked segment.
    #[test]
    fn strict_assembly_stops_at_a_width_change_and_in_front_of_a_locked_segment() {
        let mut n = Node::new();
        let a = n.add(seg_item("N", p(0, 0), p(1_000_000, 0), 200_000));
        let b = n.add(seg_item("N", p(1_000_000, 0), p(2_000_000, 0), 200_000));
        let wide = n.add(seg_item("N", p(2_000_000, 0), p(3_000_000, 0), 500_000));
        let locked = n.add(Item::Segment(Segment { net: net_of("N"), layer: 0, a: p(0, 0), b: p(-1_000_000, 0), width: 200_000, source_track: None, locked: true }));
        let loose = n.assemble_line(a).unwrap();
        assert_eq!(loose.pts, vec![p(-1_000_000, 0), p(0, 0), p(1_000_000, 0), p(2_000_000, 0), p(3_000_000, 0)], "the plain assembly goes through everything");
        let strict = n.assemble_line_with(a, AssembleOpts { follow_locked_segments: false, allow_width_mismatch: false, stop_at_locked_joints: false }).unwrap();
        assert_eq!(strict.pts, vec![p(0, 0), p(1_000_000, 0), p(2_000_000, 0)]);
        assert_eq!(strict.segment_ids, vec![a, b]);
        assert_eq!(strict.width, 200_000);
        let from_wide = n.assemble_line_with(wide, AssembleOpts { follow_locked_segments: false, allow_width_mismatch: false, stop_at_locked_joints: false }).unwrap();
        assert_eq!(from_wide.pts, vec![p(2_000_000, 0), p(3_000_000, 0)], "the wide part is a line of its own");
        // a line seeded on the locked segment itself still contains it (so `HasLockedSegments` can say so)
        let from_locked = n.assemble_line_with(locked, AssembleOpts { follow_locked_segments: false, allow_width_mismatch: false, stop_at_locked_joints: false }).unwrap();
        assert!(from_locked.segment_ids.contains(&locked));
    }

    /// `TOPOLOGY::AssembleCluster`: what touches the pad, and what touches that.
    #[test]
    fn a_cluster_is_the_blob_of_touching_items() {
        let mut n = Node::new();
        let pad_a = n.add(pad_item("N1", p(0, 0), 500_000));
        let attached = n.add(seg_item("N1", p(0, 0), p(3_000_000, 0), 200_000));
        let pad_b = n.add(pad_item("N2", p(3_000_000, 350_000), 300_000)); // touches the track's end, other net
        let crossing = n.add(seg_item("N3", p(1_500_000, -1_000_000), p(1_500_000, 1_000_000), 200_000)); // different-net track on the track: never joins
        let far = n.add(pad_item("N4", p(0, 5_000_000), 500_000));
        let c = n.assemble_cluster(pad_a, 0, 0.0, None, &|_| false);
        assert_eq!(c[0], pad_a);
        let set: std::collections::HashSet<_> = c.iter().copied().collect();
        assert!(set.contains(&attached) && set.contains(&pad_b), "{c:?}");
        assert!(!set.contains(&far) && !set.contains(&crossing), "{c:?}");
        // the walking line's own net never joins; `skip` items (the head) never do either
        let ex = n.assemble_cluster(pad_a, 0, 0.0, Some(&net_of("N2")), &|_| false);
        assert!(!ex.contains(&pad_b));
        let sk = n.assemble_cluster(pad_a, 0, 0.0, None, &|id| id == attached);
        assert!(!sk.contains(&attached));
        // `aAreaExpansionLimit`: a blob that grows past ten times the first item's box stops growing
        let capped = n.assemble_cluster(pad_a, 0, 1.5, None, &|_| false);
        assert!(capped.len() < c.len(), "{capped:?} vs {c:?}");
    }

    #[test]
    fn item_at_finds_a_track_by_its_middle_not_just_its_ends() {
        let mut n = Node::new();
        let id = n.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: p(0, 0), b: p(10_000_000, 0), width: 200_000, source_track: None, locked: false }));
        // Dead centre of a long track: `nearest_anchor` (endpoints only)
        // would miss this entirely.
        assert_eq!(n.item_at(p(5_000_000, 50_000), LayerRange::single(0), 200_000), Some(id));
        assert!(n.nearest_anchor(p(5_000_000, 50_000), LayerRange::single(0), 200_000, None).is_none(), "sanity: nearest_anchor really doesn't see the middle");
        assert!(n.item_at(p(5_000_000, 5_000_000), LayerRange::single(0), 200_000).is_none());
    }
}
