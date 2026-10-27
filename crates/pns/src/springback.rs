//! Port of `PNS::SHOVE`'s spring-back stack (`pcbnew/router/pns_shove.{h,cpp}`: `SPRINGBACK_TAG`, `m_nodeStack`,
//! `reduceSpringback`, `pushSpringback`, `AddLockedSpringbackNode`, `RewindSpringbackTo`, `RewindToLastLockedNode`) as the placers use
//! it: the line placer's `rhShoveOnly`/`FixRoute`/`UnfixRoute` (a locked frame per click), the dragger's `dragShove`/`dragViaShove`
//! (one `SHOVE` per gesture), and the pair placer's `rhShoveOnly`/`FixRoute` (a fresh `SHOVE` over the committed world at every fix, so
//! its frames are never locked).
//!
//! KiCad keeps one `NODE` per successful shove of a route's head (`SHOVE::Run` ends in `pushSpringback( m_currentNode, .. )`), and the
//! next `Run` starts from the newest of them that the new head still collides with (`reduceSpringback`: "Pops NODE stackframes which no
//! longer collide with aHeadSet"). So a track the head pushed stays pushed while the head keeps pressing on it, is pushed further from
//! where it was left as the head moves on, and springs back to where it was -- frame by frame -- as soon as the head has drawn back far
//! enough that it no longer touches the world of the newest frame. A click (`FixRoute`) pushes a *locked* frame
//! (`AddLockedSpringbackNode`) holding the fixed segments, which is never popped; Backspace (`UnfixRoute`) erases the stack from the frame
//! the undone fix was made in (`RewindSpringbackTo`). The bottom frame is never popped (`while( m_nodeStack.size() > 1 )`).
//!
//! What differs, and why:
//!
//! - **A frame is a diff, not a `NODE`.** KiCad's `NODE::Branch()` is a cheap overlay; this crate's [`Node::branch`] is a full clone
//!   (`PARITY.md`, decision 1), and a route pushes one frame per mouse move. A frame here is [`World`]: what its world has changed
//!   against the session's world -- the IR tracks and vias the shoves moved, by their IR ids (exactly what the commit will write,
//!   [`crate::router::RouteCommit`]), and the segments and vias the clicks fixed. [`World::apply`] makes the `NODE` from it when a shove
//!   starts from the frame, and [`Springback::reduce`] asks the frame's collision question without making it (the session's world minus
//!   what the frame moved, plus a small node of what it moved to). The preview and the commit are therefore the same world, always.
//! - **The fixed runs are tracks of the frame, as in KiCad** (`FixRoute`: `m_lastNode->Add( segment )`): a later leg's shove may push the
//!   route's own tail (a track the head pushes into it pushes it on), and `CommitPlacement` commits the node, tail and all. Here a run
//!   carries a [`run_tag`] in the frame's world, so what a shove made of it is in the frame's [`World::tracks`] under that tag, and the
//!   preview and the commit draw the runs from there ([`World::placed_runs`]). Its ends are locked joints (`LockJoint`, which `SHOVE::Run`
//!   does to each head's start and which the frames inherit), so a pushed run keeps its ends and a shoved line never runs on into the
//!   head. The vias the clicks placed are locked (KiCad's are not: a shove could push one that is not at the head's start; here the commit
//!   puts a via where two runs on different layers meet). (Until the review of 2026-10-23 the runs were locked here, and what a later leg
//!   pushed into the tail was walked round it or stopped the head.)
//! - `SetSpringbackDoNotTouchNode` (the node that owns the placer's end item) has no counterpart: the end item is always picked from the
//!   session's world, which is not a frame. The place of a dragged via in each frame (`m_draggedVias`) is not part of a [`World`],
//!   because only the dragger has one: it keeps those places itself, one per frame (`crate::dragger`'s `DragShove`), and truncates
//!   them with the frames a reduction pops.

use crate::item::{Item, ItemId, Net, Via};
use crate::layer::LayerRange;
use crate::line::Line;
use crate::node::Node;
use crate::shove::{Area, DisplacedLine, DisplacedVia};
use eda_drc::kimath::Shape;
use eda_model::ir::Point;
use eda_model::BoardRules;
use std::collections::BTreeMap;

/// The track id the `i`th fixed run of the route carries in a frame's world ([`World::fixed`]): not an IR track's (it starts with a control
/// character no IR id has), so that a shove that pushes the route's own tail names it the way it names a track it pushes ([`World::tracks`]),
/// and what the run has become is found again ([`World::placed_runs`]).
pub fn run_tag(i: usize) -> String {
    format!("\u{1}run{i}")
}

/// [`run_tag`]'s ids.
pub fn is_run_tag(id: &str) -> bool {
    id.starts_with('\u{1}')
}

/// What a frame's world is, against the session's world: every IR track a shove replaced (by `Track::id`, with every line that replaces
/// it -- none: the track is gone), every IR via a shove moved (by `Via::id`, to where), and what the clicks fixed. A fixed run is an ordinary
/// track of the world, as `FixRoute`'s `m_lastNode->Add( segment )` makes it: a later shove may push it (it is then in `tracks`, under its
/// [`run_tag`]), and its ends are locked joints (`SHOVE::Run`'s `LockJoint` of each head's start, which the frames keep: every leg's start
/// stays a locked joint of the worlds after it), so a pushed run keeps its ends and the shove never runs on into the head. The vias the
/// clicks placed are locked.
#[derive(Debug, Clone, Default)]
pub struct World {
    pub tracks: BTreeMap<String, Vec<Line>>,
    pub vias: BTreeMap<String, Point>,
    pub fixed: Vec<Line>,
    pub fixed_vias: Vec<Via>,
}

impl World {
    /// A shove run from this frame's world, folded in: the lines of a track the shove moved take the place of what the frame had for it
    /// (the shove re-made each line it moved from the frame's own, tagged with the IR track it descends from, and handed back what of the
    /// track it did not reach -- [`crate::shove::ShoveOutcome`] -- so its lines for a track are the whole of the track), and so does the
    /// place of a via it moved.
    pub fn merge_shove(&mut self, lines: &[DisplacedLine], vias: &[DisplacedVia]) {
        let mut fresh: BTreeMap<&str, Vec<Line>> = BTreeMap::new();
        for d in lines {
            if let Some(id) = &d.source_track {
                let e = fresh.entry(id.as_str()).or_default();
                if d.line.point_count() >= 2 {
                    e.push(d.line.clone());
                }
            }
        }
        for (id, ls) in fresh {
            self.tracks.insert(id.to_string(), ls);
        }
        for d in vias {
            self.vias.insert(d.source_via.clone(), d.pos);
        }
    }

    /// The tracks and vias this world has moved, as a preview reports them (`Preview::displaced_lines` / `displaced_vias`): a track gone
    /// with nothing in its place is one empty line.
    /// `base` is the session's world, where the net, layer and width of a track that is gone are found.
    /// The route's own runs, pushed or not, are not among them ([`Self::placed_runs`]).
    pub fn displaced_lines(&self, base: &Node) -> Vec<DisplacedLine> {
        let mut out = Vec::new();
        for (track, lines) in self.tracks.iter().filter(|(t, _)| !is_run_tag(t)) {
            if lines.is_empty() {
                let template = base.iter().find_map(|(_, it)| match it {
                    Item::Segment(s) if s.source_track.as_ref().is_some_and(|(t, _)| t == track) => Some(Line::new(s.net.clone(), s.layer, s.width)),
                    Item::Arc(a) if a.source_track.as_deref() == Some(track.as_str()) => Some(Line::new(a.net.clone(), a.layer, a.width)),
                    _ => None,
                });
                if let Some(t) = template {
                    out.push(DisplacedLine { source_track: Some(track.clone()), line: t });
                }
            }
            for l in lines {
                out.push(DisplacedLine { source_track: Some(track.clone()), line: l.clone() });
            }
        }
        out
    }

    pub fn displaced_vias(&self) -> Vec<DisplacedVia> {
        self.vias.iter().map(|(id, &pos)| DisplacedVia { source_via: id.clone(), pos }).collect()
    }

    /// The IR tracks this world has replaced, without the route's own runs.
    pub fn moved_tracks(&self) -> BTreeMap<String, Vec<Line>> {
        self.tracks.iter().filter(|(t, _)| !is_run_tag(t)).map(|(t, l)| (t.clone(), l.clone())).collect()
    }

    /// `runs` (the route's fixed runs, in order) as this world has them: a run a shove pushed is the lines that replace it (none, where the
    /// line that took it in stood on an earlier run too), any other as it was fixed. What the preview draws and the commit writes.
    pub fn placed_runs(&self, runs: &[Line]) -> Vec<Line> {
        let mut out = Vec::new();
        for (i, r) in runs.iter().enumerate() {
            match self.tracks.get(&run_tag(i)) {
                Some(lines) => out.extend(lines.iter().filter(|l| l.point_count() >= 2).cloned()),
                None => out.push(r.clone()),
            }
        }
        out
    }

    /// The items of `base` this world has taken away: the segments and arcs of every IR track it replaced, and the vias it moved.
    fn removed(&self, base: &Node) -> Vec<ItemId> {
        if self.tracks.is_empty() && self.vias.is_empty() {
            return Vec::new();
        }
        base.iter()
            .filter(|(_, it)| match it {
                Item::Segment(s) => s.source_track.as_ref().is_some_and(|(t, _)| self.tracks.contains_key(t)),
                Item::Arc(a) => a.source_track.as_ref().is_some_and(|t| self.tracks.contains_key(t)),
                Item::Via(v) => v.source_via.as_ref().is_some_and(|n| self.vias.contains_key(n)),
                Item::Solid(_) => false,
            })
            .map(|(id, _)| id)
            .collect()
    }

    /// What this world has put in: the lines that replace tracks (tagged with the track, as the shove tags what it re-makes), the moved
    /// vias, the fixed segments and vias (locked). Added to `node`.
    fn add_to(&self, base: &Node, node: &mut Node) {
        for (track, lines) in &self.tracks {
            for l in lines.iter().filter(|l| l.point_count() >= 2) {
                node.add_line_tagged(l, track);
            }
        }
        for (name, &pos) in &self.vias {
            let original = base.iter().find_map(|(_, it)| match it {
                Item::Via(v) if v.source_via.as_deref() == Some(name.as_str()) => Some(v.clone()),
                _ => None,
            });
            if let Some(v) = original {
                node.add(Item::Via(Via { pos, ..v }));
            }
        }
        for (i, l) in self.fixed.iter().enumerate().filter(|(i, l)| l.point_count() >= 2 && !self.tracks.contains_key(&run_tag(*i))) {
            node.add_line_tagged(l, &run_tag(i));
        }
        for v in &self.fixed_vias {
            node.add(Item::Via(Via { locked: true, ..v.clone() }));
        }
        // every leg's start (and so every run's end) is a locked joint, as `LockJoint` left it in the frame each leg was routed in
        for l in &self.fixed {
            for p in [l.first(), l.last()].into_iter().flatten() {
                node.lock_joint(p, &l.net);
            }
        }
    }

    /// The frame's `NODE`: `base` with what this world took away removed and what it put in added.
    pub fn apply(&self, base: &Node) -> Node {
        let mut node = base.branch();
        for id in self.removed(base) {
            node.remove(id);
        }
        self.add_to(base, &mut node);
        node
    }

    /// This world with the head of a click fixed in it (`FixRoute`: `m_lastNode->Add( segment )`, `m_lastNode->Add( newVia )`).
    pub fn with_fixed(&self, run: Option<&Line>, via: Option<Via>) -> World {
        let mut w = self.clone();
        if let Some(r) = run.filter(|r| r.point_count() >= 2) {
            w.fixed.push(r.clone());
        }
        if let Some(v) = via {
            w.fixed_vias.push(v);
        }
        w
    }
}

/// `SPRINGBACK_TAG`: a frame's world, its area (`m_affectedArea`, merged with the frame below's), its sequence number and lock, and --
/// for the collision question [`Springback::reduce`] asks -- the items of the session's world it took away and a node of what it put in.
#[derive(Debug, Clone)]
struct Tag {
    world: World,
    area: Option<Area>,
    #[allow(dead_code)] // `m_seq`: KiCad's debug output only
    seq: i32,
    locked: bool,
    removed: Vec<ItemId>,
    added: Node,
}

/// `SHOVE::m_nodeStack`. The session's world (`m_root`) is not on it: an empty stack is that world.
#[derive(Debug, Clone, Default)]
pub struct Springback {
    stack: Vec<Tag>,
}

/// The head a shove is asked about (`ITEM_SET aHeadSet` of `reduceSpringback`): the line before the shove, and the via it ends in.
pub struct HeadSet<'a> {
    pub pts: &'a [Point],
    pub net: &'a Net,
    pub layer: i32,
    pub width: eda_model::ir::Nm,
    pub via: Option<&'a Via>,
}

impl Springback {
    pub fn new() -> Self {
        Springback { stack: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.stack.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    fn tag(base: &Node, world: World, area: Option<Area>, seq: i32, locked: bool) -> Tag {
        let removed = world.removed(base);
        let mut added = base.empty_like();
        world.add_to(base, &mut added);
        Tag { world, area, seq, locked, removed, added }
    }

    /// `spTag.m_node->CheckColliding( aHeadSet )`: does the head collide with anything of the frame's world -- the session's world without
    /// what the frame took away, or what it put in.
    fn collides(base: &Node, tag: &Tag, head: &HeadSet, rules: &BoardRules) -> bool {
        let hit = |shape: &Shape, layers: LayerRange| base.first_colliding(shape, head.net, layers, rules, &tag.removed).is_some() || tag.added.first_colliding(shape, head.net, layers, rules, &[]).is_some();
        let layers = LayerRange::single(head.layer);
        if head.pts.windows(2).any(|w| w[0] != w[1] && hit(&Shape::Stadium { a: w[0], b: w[1], r: head.width / 2 }, layers)) {
            return true;
        }
        head.via.is_some_and(|v| hit(&Shape::Circle { c: v.pos, r: v.diameter / 2 }, v.layers))
    }

    /// `SHOVE::reduceSpringback( aHeadSet )`: pop the frames, newest first, whose world none of the heads collides with -- never a locked
    /// one, never the last one -- and return the world the shove starts from (the newest frame left, or the session's when there is none)
    /// with its area. The pair placer has two heads (`m_shove->AddHeads` of each line), which `CheckColliding( aHeadSet )` asks about
    /// together: a frame one head still presses on stays for both.
    pub fn reduce_all(&mut self, base: &Node, heads: &[HeadSet], rules: &BoardRules) -> (World, Option<Area>) {
        while self.stack.len() > 1 {
            let top = self.stack.last().unwrap();
            if !top.locked && !heads.iter().any(|head| Self::collides(base, top, head, rules)) {
                self.stack.pop();
            } else {
                break;
            }
        }
        self.top()
    }

    /// [`Self::reduce_all`] for one head.
    pub fn reduce(&mut self, base: &Node, head: &HeadSet, rules: &BoardRules) -> (World, Option<Area>) {
        self.reduce_all(base, std::slice::from_ref(head), rules)
    }

    /// The newest frame's world and area (`CurrentNode()` after a run), or the session's world when the stack is empty.
    pub fn top(&self) -> (World, Option<Area>) {
        self.stack.last().map(|t| (t.world.clone(), t.area)).unwrap_or_default()
    }

    /// The world the clicks left (the newest locked frame: `m_currentNode` after `FixRoute` in a mode that does not shove), or the session's.
    pub fn locked_top(&self) -> World {
        self.stack.iter().rev().find(|t| t.locked).map(|t| t.world.clone()).unwrap_or_default()
    }

    /// `SHOVE::pushSpringback( aNode, aAffectedArea )`: a frame for the world a successful shove left, its area merged with the frame
    /// below's.
    pub fn push(&mut self, base: &Node, world: World, area: Option<Area>) {
        let prev = self.stack.last().and_then(|t| t.area);
        let merged = match (prev, area) {
            (Some(p), Some(a)) => Some((p.0.min(a.0), p.1.min(a.1), p.2.max(a.2), p.3.max(a.3))),
            (p, a) => a.or(p),
        };
        let seq = self.stack.last().map_or(1, |t| t.seq + 1);
        self.stack.push(Self::tag(base, world, merged, seq, false));
    }

    /// `SHOVE::AddLockedSpringbackNode( aNode )`: a frame that is never popped, for the world a click left.
    pub fn add_locked(&mut self, base: &Node, world: World) {
        self.stack.push(Self::tag(base, world, None, 0, true));
    }

    /// `SHOVE::RewindSpringbackTo( aNode )` for the frame at `len - 1`: erase it and every frame after it (`m_nodeStack.erase( start, end )`).
    /// **Differs:** when that frame is a locked one -- the undone leg's stage was fixed in the world the click before left, because no shove
    /// of the leg succeeded (`Run` fell back to its parent) -- KiCad erases it too, and with it the leg before, which is still fixed (and which
    /// `CommitPlacement`, `RewindToLastLockedNode`, then does not commit). Here only the frames after it go.
    pub fn rewind_to(&mut self, len: usize) {
        let keep = match len.checked_sub(1).and_then(|i| self.stack.get(i)) {
            Some(t) if t.locked => len,
            _ => len.saturating_sub(1),
        };
        self.stack.truncate(keep);
    }

    /// `SHOVE::RewindToLastLockedNode()`: pop to the newest locked frame (or the last one).
    pub fn rewind_to_last_locked(&mut self) {
        while self.stack.len() > 1 && !self.stack.last().unwrap().locked {
            self.stack.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;
    use crate::item::{net_of, Segment};
    use eda_model::ir::Nm;

    fn rules() -> BoardRules {
        serde_yaml::from_str("track_width: 200000\nclearance: 200000\nvia_drill: 300000\nvia_diameter: 600000\n").unwrap()
    }

    fn base() -> Node {
        let mut n = Node::new();
        n.add(Item::Segment(Segment { net: net_of("B"), layer: 0, a: Point { x: Nm::ZERO, y: mm(1.0) }, b: Point { x: mm(10.0), y: mm(1.0) }, width: mm(0.25), source_track: Some(("t1".into(), 0)), locked: false }));
        n
    }

    fn line(y: Nm) -> Line {
        Line::from_points(net_of("B"), 0, mm(0.25), vec![Point { x: Nm::ZERO, y }, Point { x: mm(10.0), y }])
    }

    fn head<'a>(pts: &'a [Point], net: &'a Net) -> HeadSet<'a> {
        HeadSet { pts, net, layer: 0, width: mm(0.25), via: None }
    }

    fn moved(y: Nm) -> World {
        let mut w = World::default();
        w.merge_shove(&[DisplacedLine { source_track: Some("t1".into()), line: line(y) }], &[]);
        w
    }

    #[test]
    fn a_frame_the_head_still_presses_on_is_kept_and_one_it_has_left_is_popped() {
        let (b, r, a) = (base(), rules(), net_of("A"));
        let mut sb = Springback::new();
        sb.push(&b, World::default(), None);
        sb.push(&b, moved(mm(2.0)), None);
        // the head at y = 1800 touches the track where the newest frame left it (y = 2000): the frame stays
        let near = [Point { x: mm(2.0), y: mm(1.8) }, Point { x: mm(8.0), y: mm(1.8) }];
        let (w, _) = sb.reduce(&b, &head(&near, &a), &r);
        assert_eq!(sb.len(), 2);
        assert_eq!(w.tracks["t1"][0].pts[0].y, mm(2.0));
        // drawn back to y = -1000 it touches nothing of that world: the track springs back to the frame below
        let far = [Point { x: mm(2.0), y: mm(-1.0) }, Point { x: mm(8.0), y: mm(-1.0) }];
        let (w, _) = sb.reduce(&b, &head(&far, &a), &r);
        assert_eq!(sb.len(), 1);
        assert!(w.tracks.is_empty());
    }

    #[test]
    fn a_frame_stays_while_any_of_the_heads_presses_on_it() {
        let (b, r) = (base(), rules());
        let (a, c) = (net_of("A"), net_of("C"));
        let mut sb = Springback::new();
        sb.push(&b, World::default(), None);
        sb.push(&b, moved(mm(2.0)), None);
        // one head drawn back, the other still on the track where the newest frame left it: the frame stays for both
        let back = [Point { x: mm(2.0), y: mm(-1.0) }, Point { x: mm(8.0), y: mm(-1.0) }];
        let near = [Point { x: mm(2.0), y: mm(1.8) }, Point { x: mm(8.0), y: mm(1.8) }];
        let (w, _) = sb.reduce_all(&b, &[head(&back, &a), head(&near, &c)], &r);
        assert_eq!(sb.len(), 2);
        assert_eq!(w.tracks["t1"][0].pts[0].y, mm(2.0));
        // both drawn back: it springs back to the frame below
        let (w, _) = sb.reduce_all(&b, &[head(&back, &a), head(&back, &c)], &r);
        assert_eq!(sb.len(), 1);
        assert!(w.tracks.is_empty());
    }

    #[test]
    fn a_locked_frame_and_the_last_frame_are_never_popped() {
        let (b, r, a) = (base(), rules(), net_of("A"));
        let far = [Point { x: mm(2.0), y: mm(-5.0) }, Point { x: mm(8.0), y: mm(-5.0) }];
        let mut sb = Springback::new();
        sb.push(&b, moved(mm(2.0)), None);
        sb.reduce(&b, &head(&far, &a), &r);
        assert_eq!(sb.len(), 1, "the bottom frame stays");
        sb.add_locked(&b, moved(mm(2.0)));
        sb.push(&b, moved(mm(3.0)), None);
        sb.reduce(&b, &head(&far, &a), &r);
        assert_eq!(sb.len(), 2, "popped down to the locked frame");
        // the stage of the fix was the frame the shove pushed before it (the stack's length then: 1)
        let mut undo = sb.clone();
        undo.rewind_to(1);
        assert_eq!(undo.len(), 0, "Backspace erases the frame the fix was made in and what came after");
        // a stage fixed in a locked frame (the click before's) keeps that frame
        sb.add_locked(&b, moved(mm(3.0)));
        sb.rewind_to(2);
        assert_eq!(sb.len(), 2, "the locked frame of the click before stays");
    }

    #[test]
    fn the_world_of_a_frame_is_the_session_world_with_what_it_moved() {
        let b = base();
        let n = moved(mm(2.0)).with_fixed(Some(&Line::from_points(net_of("A"), 0, mm(0.25), vec![Point { x: Nm::ZERO, y: mm(-3.0) }, Point { x: mm(5.0), y: mm(-3.0) }])), None).apply(&b);
        let segs: Vec<_> = n.iter().filter_map(|(_, it)| match it { Item::Segment(s) => Some((s.a.y, s.locked, s.source_track.as_ref().map(|t| t.0.clone()))), _ => None }).collect();
        assert_eq!(segs.len(), 2);
        assert!(segs.contains(&(mm(2.0), false, Some("t1".into()))), "{segs:?}");
        assert!(segs.contains(&(mm(-3.0), false, Some(run_tag(0)))), "the fixed run is in it, a track a shove may push: {segs:?}");
        let ends = [Point { x: Nm::ZERO, y: mm(-3.0) }, Point { x: mm(5.0), y: mm(-3.0) }];
        assert!(ends.iter().all(|&e| n.joint_at(e, &net_of("A")).is_some_and(|j| j.locked)), "its ends are locked joints (`LockJoint`)");
    }

    #[test]
    fn areas_merge_up_the_stack() {
        let b = base();
        let mut sb = Springback::new();
        sb.push(&b, World::default(), Some((Nm::ZERO, Nm::ZERO, mm(0.01), mm(0.01))));
        sb.push(&b, World::default(), Some((Nm::new(5000), Nm::new(-5000), mm(0.02), Nm::new(5000))));
        assert_eq!(sb.top().1, Some((Nm::ZERO, Nm::new(-5000), mm(0.02), mm(0.01))));
        sb.push(&b, World::default(), None);
        assert_eq!(sb.top().1, Some((Nm::ZERO, Nm::new(-5000), mm(0.02), mm(0.01))));
    }
}
