//! Port of `PNS::SHOVE`'s spring-back stack (`pcbnew/router/pns_shove.{h,cpp}`: `SPRINGBACK_TAG`, `m_nodeStack`,
//! `reduceSpringback`, `pushSpringback`, `AddLockedSpringbackNode`, `RewindSpringbackTo`, `RewindToLastLockedNode`) as the line
//! placer uses it (`pns_line_placer.cpp`: `rhShoveOnly`, `FixRoute`, `UnfixRoute`).
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
//! - **The fixed segments are locked in the frame.** KiCad's `FixRoute` adds them to `m_lastNode` as ordinary segments, so a later shove
//!   may push the route's own tail (and `CommitPlacement` commits the node, tail and all). The commit here is built from the placer's
//!   runs, which a shove cannot reach, so the tail is a solid to the shove (`SH_TRY_WALK`: what runs into it is walked round it).
//! - `SetSpringbackDoNotTouchNode` (the node that owns the placer's end item) has no counterpart: the end item is always picked from the
//!   session's world, which is not a frame. The dragged vias of a frame (`m_draggedVias`) are the dragger's, which does not use this.

use crate::item::{Item, ItemId, Net, Via};
use crate::layer::LayerRange;
use crate::line::Line;
use crate::node::Node;
use crate::shove::{Area, DisplacedLine, DisplacedVia};
use eda_drc::kimath::Shape;
use eda_model::ir::Point;
use eda_model::BoardRules;
use std::collections::BTreeMap;

/// What a frame's world is, against the session's world: every IR track a shove replaced (by `Track::id`, with every line that replaces
/// it -- none: the track is gone), every IR via a shove moved (by `Via::id`, to where), and what the clicks fixed (locked).
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
    pub fn displaced_lines(&self, base: &Node) -> Vec<DisplacedLine> {
        let mut out = Vec::new();
        for (track, lines) in &self.tracks {
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
        for l in self.fixed.iter().filter(|l| l.point_count() >= 2) {
            node.add_line(l, None, true);
        }
        for v in &self.fixed_vias {
            node.add(Item::Via(Via { locked: true, ..v.clone() }));
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
    pub width: eda_model::ir::Um,
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
        let mut added = Node::new();
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

    /// `SHOVE::reduceSpringback( aHeadSet )`: pop the frames, newest first, whose world the head no longer collides with -- never a locked
    /// one, never the last one -- and return the world the shove starts from (the newest frame left, or the session's when there is none)
    /// with its area.
    pub fn reduce(&mut self, base: &Node, head: &HeadSet, rules: &BoardRules) -> (World, Option<Area>) {
        while self.stack.len() > 1 {
            let top = self.stack.last().unwrap();
            if !top.locked && !Self::collides(base, top, head, rules) {
                self.stack.pop();
            } else {
                break;
            }
        }
        self.top()
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
    pub fn rewind_to(&mut self, len: usize) {
        self.stack.truncate(len.saturating_sub(1));
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
    use super::*;
    use crate::item::{net_of, Segment};
    use eda_model::ir::Um;

    fn rules() -> BoardRules {
        serde_yaml::from_str("track_width: 200\nclearance: 200\nvia_drill: 300\nvia_diameter: 600\n").unwrap()
    }

    fn base() -> Node {
        let mut n = Node::new();
        n.add(Item::Segment(Segment { net: net_of("B"), layer: 0, a: Point { x: 0, y: 1000 }, b: Point { x: 10_000, y: 1000 }, width: 250, source_track: Some(("t1".into(), 0)), locked: false }));
        n
    }

    fn line(y: Um) -> Line {
        Line::from_points(net_of("B"), 0, 250, vec![Point { x: 0, y }, Point { x: 10_000, y }])
    }

    fn head<'a>(pts: &'a [Point], net: &'a Net) -> HeadSet<'a> {
        HeadSet { pts, net, layer: 0, width: 250, via: None }
    }

    fn moved(y: Um) -> World {
        let mut w = World::default();
        w.merge_shove(&[DisplacedLine { source_track: Some("t1".into()), line: line(y) }], &[]);
        w
    }

    #[test]
    fn a_frame_the_head_still_presses_on_is_kept_and_one_it_has_left_is_popped() {
        let (b, r, a) = (base(), rules(), net_of("A"));
        let mut sb = Springback::new();
        sb.push(&b, World::default(), None);
        sb.push(&b, moved(2000), None);
        // the head at y = 1800 touches the track where the newest frame left it (y = 2000): the frame stays
        let near = [Point { x: 2000, y: 1800 }, Point { x: 8000, y: 1800 }];
        let (w, _) = sb.reduce(&b, &head(&near, &a), &r);
        assert_eq!(sb.len(), 2);
        assert_eq!(w.tracks["t1"][0].pts[0].y, 2000);
        // drawn back to y = -1000 it touches nothing of that world: the track springs back to the frame below
        let far = [Point { x: 2000, y: -1000 }, Point { x: 8000, y: -1000 }];
        let (w, _) = sb.reduce(&b, &head(&far, &a), &r);
        assert_eq!(sb.len(), 1);
        assert!(w.tracks.is_empty());
    }

    #[test]
    fn a_locked_frame_and_the_last_frame_are_never_popped() {
        let (b, r, a) = (base(), rules(), net_of("A"));
        let far = [Point { x: 2000, y: -5000 }, Point { x: 8000, y: -5000 }];
        let mut sb = Springback::new();
        sb.push(&b, moved(2000), None);
        sb.reduce(&b, &head(&far, &a), &r);
        assert_eq!(sb.len(), 1, "the bottom frame stays");
        sb.add_locked(&b, moved(2000));
        sb.push(&b, moved(3000), None);
        sb.reduce(&b, &head(&far, &a), &r);
        assert_eq!(sb.len(), 2, "popped down to the locked frame");
        sb.rewind_to(2);
        assert_eq!(sb.len(), 1, "Backspace erases the frame the fix was made in and what came after");
    }

    #[test]
    fn the_world_of_a_frame_is_the_session_world_with_what_it_moved() {
        let b = base();
        let n = moved(2000).with_fixed(Some(&Line::from_points(net_of("A"), 0, 250, vec![Point { x: 0, y: -3000 }, Point { x: 5000, y: -3000 }])), None).apply(&b);
        let segs: Vec<_> = n.iter().filter_map(|(_, it)| match it { Item::Segment(s) => Some((s.a.y, s.locked, s.source_track.as_ref().map(|t| t.0.clone()))), _ => None }).collect();
        assert_eq!(segs.len(), 2);
        assert!(segs.contains(&(2000, false, Some("t1".into()))), "{segs:?}");
        assert!(segs.contains(&(-3000, true, None)), "the fixed run is in it, locked: {segs:?}");
    }

    #[test]
    fn areas_merge_up_the_stack() {
        let b = base();
        let mut sb = Springback::new();
        sb.push(&b, World::default(), Some((0, 0, 10, 10)));
        sb.push(&b, World::default(), Some((5, -5, 20, 5)));
        assert_eq!(sb.top().1, Some((0, -5, 20, 10)));
        sb.push(&b, World::default(), None);
        assert_eq!(sb.top().1, Some((0, -5, 20, 10)));
    }
}
