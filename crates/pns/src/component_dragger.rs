//! Port of `PNS::COMPONENT_DRAGGER` (`pcbnew/router/pns_component_dragger.{h,cpp}` at 8303b2ad): `D` on a footprint
//! (`ROUTER_TOOL::InlineDrag` with a selection of footprints, `ROUTER::StartDragging( aP, ITEM_SET )` of nothing but `SOLID`s,
//! `m_state = DRAG_COMPONENT`). The footprint's pads move with the cursor, and every track that ends on one of them follows: the
//! line from the pad to the track's far end is re-solved at 45 degrees with its end at the pad's new place (`LINE::DragCorner`), and a
//! track that runs from one dragged pad to another moves with them as it is.
//!
//! ## `Start` ([`ComponentDragger::start`])
//!
//! For each solid of the set (`m_solids`) that is routable, the tracks linked at its joint (`FindJoint( solid->Pos(), solid )`, then
//! `SEGMENT_T | ARC_T` links) are taken, each once (`seenItems`):
//! - a track whose other end is a joint with a pad of the set is moved as it is (`m_fixedItems`: "Segments that go directly between two
//!   linked pads are special-cased");
//! - else the line through it is assembled (`m_world->AssembleLine( aItem )`), and if its far end is a joint with a pad of the set, every
//!   item of the line is moved as it is ("Lines that go directly between two linked pads are also special-cased");
//! - else the line is a connection (`DRAGGED_CONNECTION`): its end at the pad follows the pad.
//!
//! Then the joints inside the solid's box (`QueryJoints( solid->Hull().BBox(), .., SEGMENT_T | ARC_T )`) of the pad's net with one link,
//! where that track collides with the pad (`li->Collide( solid, m_world, solid->Layer() )`), are connections too, with the offset of the
//! track's end from the pad's centre. As in KiCad at 8303b2ad this finds nothing for a pad of the track's own net: `ITEM::collideSimple`
//! gives two items of one net no clearance ("same nets? no clearance!"), and its hole is of the net too. A track that ends inside a pad
//! off its centre therefore stays where it is when the pad moves.
//!
//! ## `Drag` ([`ComponentDragger::drag`])
//!
//! The world is branched afresh each position (`m_world->KillChildren(); m_currentNode = m_world->Branch()`): the solids are moved by
//! `aP - m_p0` (`snew->SetPos( p_next )`), the fixed items by the same (`SetEnds`, `SHAPE_ARC::Move`), and each connection's line is the
//! original with the vertex at the pad dragged to the pad's new place (`l_new.DragCorner( cn.p_next, cn.origLine.CLine().Find( cn.p_orig ) )`:
//! 45 degrees, no snap, no preferred direction). Every position "works" (`Drag` returns true).
//!
//! ## `FixRoute` ([`ComponentDragger::fix_route`])
//!
//! `Settings().AllowDRCViolations() || aForceCommit || !node->CheckColliding( m_draggedItems )`: the moved pads, the moved tracks and the new
//! lines may collide with nothing of the world, unless violations are allowed (Highlight collisions with "Allow DRC violations") or the click
//! is a Ctrl+click. `GetForceMarkObstaclesMode` is false and `m_dragStatus` is never set: there is no "Track violates DRC." hint, and a
//! refused click ends the drag with nothing committed (`InlineDrag` breaks out of its loop whatever `FixRoute` says).
//!
//! ## The commit
//!
//! `ROUTER::CommitRouting` and `PNS_KICAD_IFACE::Commit`: a moved pad is not committed as a pad -- its footprint is moved by the pad's offset
//! (`m_fpOffsets`, `footprint->SetPosition( p_orig + offset )`); a moved track keeps its board item (`UpdateItem`: the clone has the original's
//! parent); a connection's new line replaces the tracks it stood on. Here: [`ComponentCommit`], which the router turns into a footprint move
//! and a `Cmd::CommitRoute` in one undo step. An IR track whose segments all moved as they are is moved whole (it keeps its id); one that
//! moved only in part is cut into what moved and what did not.
//!
//! **Differs:**
//! - (no longer a difference) a pad made of several solids (a padstack whose shape changes from layer to layer, `syncPad`'s solids) hands the
//!   dragger its first solid only, as KiCad's `FindItemByParent( pad )` does (`Router::footprint_solids`, UI-LEFT-2 RR7);
//! - (no longer a difference) a footprint's own Edge.Cuts and Margin graphics -- its art since the importer keeps them there -- are among the
//!   solids dragged (`InlineDrag`: `GraphicalItems()` on Edge_Cuts or Margin, `itemsToDrag.Add( solid )`; `from_ir::edge_source`), so a pad
//!   keeps its distance to the footprint's own slot or edge as the footprint moves;
//! - a pad's own hole is not asked as the probe of `CheckColliding` (see [`crate::node::Node::item_colliding`]); the world's holes are, and a
//!   moved pad's hole moves with it ([`crate::rule_resolver::RuleResolver::pad_hole_of`]);
//! - the rules that look at an item's geometry (`A.intersectsArea( .. )`) see a moved pad where the board has it.

use crate::item::{Item, ItemId, Net};
use crate::line::Line;
use crate::node::{kind_mask, AssembleOpts, Node};
use crate::settings::RoutingSettings;
use eda_drc::kimath::Shape;
use eda_model::ir::{Nm, Point};
use eda_model::BoardRules;
use std::collections::{BTreeSet, HashSet};

/// `COMPONENT_DRAGGER::DRAGGED_CONNECTION`: a line that ends on a dragged pad and follows it.
#[derive(Debug, Clone)]
pub struct DraggedConnection {
    /// `origLine`: the line as the world has it (`AssembleLine`).
    pub orig_line: Line,
    /// `attachedPad`.
    pub attached_pad: ItemId,
    /// `offset`: where the line's end is from the pad's centre (`extraJoint->Pos() - solid->Pos()`; zero for a track linked at the centre).
    pub offset: (Nm, Nm),
    /// The track item linked at the pad (`aItem`): the board track its line descends from (`pl.SetSourceItem( aSeg->GetSourceItem() )`).
    pub seed: ItemId,
}

/// `PNS::COMPONENT_DRAGGER`: see the module doc.
#[derive(Debug, Clone)]
pub struct ComponentDragger {
    /// The footprints dragged (`ROUTER_TOOL::InlineDrag`'s `footprints`), by reference: what the commit moves.
    pub footprints: Vec<String>,
    /// `m_initialDraggedItems`: the solids of the footprints.
    initial: Vec<ItemId>,
    /// `m_solids`.
    solids: Vec<ItemId>,
    /// `m_fixedItems`: the tracks moved as they are.
    fixed: BTreeSet<ItemId>,
    /// `m_conns`.
    conns: Vec<DraggedConnection>,
    /// `m_p0`: where the drag started.
    p0: Point,
    /// The last position `Drag` was given (`m_currentNode != nullptr`).
    last: Option<Point>,
}

/// What `COMPONENT_DRAGGER::Drag` makes of one position: `m_currentNode` and `m_draggedItems`.
#[derive(Debug, Clone)]
pub struct ComponentDrag {
    /// `aP - m_p0`: how far every pad, every fixed track and the footprints move.
    pub offset: (Nm, Nm),
    /// `m_currentNode`: the world with the pads, the fixed tracks and the lines moved.
    pub node: Node,
    /// The moved solids, by their id in [`Self::node`].
    pub solids: Vec<ItemId>,
    /// The fixed items as moved: `(id in the world, id in [`Self::node`])`.
    pub fixed: Vec<(ItemId, ItemId)>,
    /// `l_new` of each connection, in the order of [`ComponentDragger::connections`], linked to its items in [`Self::node`].
    pub lines: Vec<Line>,
}

/// What committing a component drag does (`ROUTER::CommitRouting` + `PNS_KICAD_IFACE::Commit`): see the module doc.
#[derive(Debug, Clone, Default)]
pub struct ComponentCommit {
    /// The footprints moved (`m_fpOffsets`), and by how much.
    pub footprints: Vec<String>,
    pub offset: (Nm, Nm),
    /// The IR tracks moved whole by [`Self::offset`]: every item of theirs was a fixed item (`UpdateItem` of each).
    pub moved_tracks: Vec<String>,
    /// The IR tracks removed, and what takes their place: the connections' new lines, the fixed items of a track moved only in part, and
    /// what a removed track keeps of itself.
    pub remove_track_ids: Vec<String>,
    pub tracks: Vec<Line>,
    /// The board track each of `tracks` replaces, by the same index.
    pub sources: Vec<Option<String>>,
}

/// `NODE::AssembleLine( aSeg )` with its defaults (`aStopAtLockedJoints = false, aFollowLockedSegments = false, aAllowSegmentSizeMismatch = true`).
fn assemble(node: &Node, id: ItemId) -> Option<Line> {
    node.assemble_line_with(id, AssembleOpts { follow_locked_segments: false, allow_width_mismatch: true, stop_at_locked_joints: false })
}

/// The item `it` moved by `(dx, dy)`: a segment's ends (`s_new->SetEnds( aP - m_p0 + orig.A, aP - m_p0 + orig.B )`), an arc's three points
/// (`arc.Move( aP - m_p0 )`).
fn moved_item(it: &Item, dx: Nm, dy: Nm) -> Option<Item> {
    let t = |p: Point| Point { x: p.x + dx, y: p.y + dy };
    match it {
        Item::Segment(s) => Some(Item::Segment(crate::item::Segment { a: t(s.a), b: t(s.b), ..s.clone() })),
        Item::Arc(a) => {
            let arc = crate::arcs::ArcPts::new(t(a.arc.start), t(a.arc.mid), t(a.arc.end));
            Some(Item::Arc(crate::item::Arc::new(a.net.clone(), a.layer, a.width, arc, a.source_track.clone(), a.locked)))
        }
        // "Unexpected item type in COMPONENT_DRAGGER::m_fixedItems"
        _ => None,
    }
}

/// Does the joint at `pos` of `net` link a solid of `set`? (`otherJoint->LinkCount( ITEM::SOLID_T )` and `aPrimitives.Contains( otherItem )`.)
fn joint_links_one_of(node: &Node, pos: Point, net: &Net, set: &HashSet<ItemId>) -> bool {
    node.joint_at(pos, net).is_some_and(|j| j.links.iter().any(|id| set.contains(id) && matches!(node.get(*id), Some(Item::Solid(_)))))
}

impl ComponentDragger {
    /// `COMPONENT_DRAGGER::Start( aP, aPrimitives )`: `items` are the solids of the dragged footprints (anything else in the set is left
    /// alone, as the `SOLID_T` filter does), `at` the point the drag starts from (`m_p0`). Always starts (`return true`). `rules` are what
    /// the test of a track ending inside a pad is asked with.
    pub fn start(node: &Node, rules: &BoardRules, at: Point, footprints: Vec<String>, items: &[ItemId]) -> ComponentDragger {
        let primitives: HashSet<ItemId> = items.iter().copied().collect();
        let mut d = ComponentDragger { footprints, initial: items.to_vec(), solids: Vec::new(), fixed: BTreeSet::new(), conns: Vec::new(), p0: at, last: None };
        // `seenItems`
        let mut seen: HashSet<ItemId> = HashSet::new();
        for &id in items {
            let Some(Item::Solid(solid)) = node.get(id) else { continue };
            d.solids.push(id);
            if !solid.routable() {
                continue;
            }
            // `FindJoint( solid->Pos(), solid )`: the joint of the pad's net at its centre, on its layers
            let links: Vec<ItemId> = node.joint_at(solid.pos, &solid.net).map(|j| j.links.clone()).unwrap_or_default();
            for link in links {
                if node.get(link).is_some_and(|it| it.is_track() && it.layers().overlaps(&solid.layers)) {
                    d.add_linked(node, &primitives, &mut seen, id, solid.pos, link, (Nm::ZERO, Nm::ZERO));
                }
            }
            // `QueryJoints( solid->Hull().BBox(), extraJoints, solid->Layers(), SEGMENT_T | ARC_T )`: a track ending inside the pad, off its centre
            let bbox = solid.shape.bbox(Nm::ZERO);
            for (pos, net, links) in node.joints_in(bbox, solid.layers, kind_mask::TRACK) {
                // `extraJoint->Net() == jt->Net() && extraJoint->LinkCount() == 1`
                if !crate::item::same_net(&net, &solid.net) || links.len() != 1 {
                    continue;
                }
                let li = links[0];
                let Some(li_item) = node.get(li) else { continue };
                // `li->Collide( solid, m_world, solid->Layer() )`: the track against the pad as the head (see the module doc: never for a pad of its net)
                let solid_item = Item::Solid(solid.clone());
                let collides = node.item_collision(rules, li, li_item, &solid.shape, crate::node::Side::Item(&solid_item), solid.layers, crate::node::QueryOpts::default()).is_some();
                if li_item.is_track() && collides {
                    d.add_linked(node, &primitives, &mut seen, id, pos, li, (pos.x - solid.pos.x, pos.y - solid.pos.y));
                }
            }
        }
        d
    }

    /// `addLinked( aSolid, aJoint, aItem, aOffset )`: see the module doc. `joint` is where the track is linked to the pad.
    #[allow(clippy::too_many_arguments)] // the lambda's captures (the world, the set, `seenItems`) and its four arguments
    fn add_linked(&mut self, node: &Node, primitives: &HashSet<ItemId>, seen: &mut HashSet<ItemId>, solid: ItemId, joint: Point, item: ItemId, offset: (Nm, Nm)) {
        if !seen.insert(item) {
            return;
        }
        let Some(it) = node.get(item) else { return };
        let Some((a, b)) = it.track_ends() else { return };
        // "Segments that go directly between two linked pads are special-cased"
        let other_end = if joint == a { b } else { a };
        if joint_links_one_of(node, other_end, it.net(), primitives) {
            self.fixed.insert(item);
            return;
        }
        let Some(orig_line) = assemble(node, item) else { return };
        // "Lines that go directly between two linked pads are also special-cased": the end of the line that is not at `aJoint`
        let (first, last) = (orig_line.first().unwrap_or(joint), orig_line.last().unwrap_or(joint));
        let far = if first == joint { last } else { first };
        if joint_links_one_of(node, far, it.net(), primitives) {
            self.fixed.extend(orig_line.segment_ids.iter().copied());
            return;
        }
        self.conns.push(DraggedConnection { orig_line, attached_pad: solid, offset, seed: item });
    }

    /// The lines that follow the pads (`m_conns`).
    pub fn connections(&self) -> &[DraggedConnection] {
        &self.conns
    }

    /// The tracks moved as they are (`m_fixedItems`).
    pub fn fixed_items(&self) -> impl Iterator<Item = ItemId> + '_ {
        self.fixed.iter().copied()
    }

    /// `m_p0`.
    pub fn start_point(&self) -> Point {
        self.p0
    }

    /// The last position the drag was given, if any.
    pub fn last_point(&self) -> Option<Point> {
        self.last
    }

    /// `COMPONENT_DRAGGER::Drag( aP )`: see the module doc. Remembers `to` as the position a release commits.
    pub fn drag(&mut self, node: &Node, to: Point) -> ComponentDrag {
        self.last = Some(to);
        self.solve(node, to)
    }

    /// [`Self::drag`] without remembering the position.
    pub fn solve(&self, world: &Node, to: Point) -> ComponentDrag {
        let (dx, dy) = (to.x - self.p0.x, to.y - self.p0.y);
        // `m_currentNode = m_world->Branch()`
        let mut node = world.branch();
        for &id in &self.initial {
            node.remove(id);
        }
        let mut solids = Vec::new();
        // `p_orig` / `p_next` of each connection, by its pad
        let mut ends: Vec<(Point, Point)> = vec![(Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::ZERO, y: Nm::ZERO }); self.conns.len()];
        for &id in &self.solids {
            let Some(Item::Solid(s)) = world.get(id) else { continue };
            // `VECTOR2I p_next = aP - m_p0 + s->Pos(); snew->SetPos( p_next )`
            let p_next = Point { x: s.pos.x + dx, y: s.pos.y + dy };
            solids.push(node.add(Item::Solid(s.moved_to(p_next))));
            if !s.routable() {
                continue;
            }
            for (k, c) in self.conns.iter().enumerate() {
                if c.attached_pad == id {
                    ends[k] = (Point { x: s.pos.x + c.offset.0, y: s.pos.y + c.offset.1 }, Point { x: p_next.x + c.offset.0, y: p_next.y + c.offset.1 });
                }
            }
        }
        let mut fixed = Vec::new();
        for &id in &self.fixed {
            let Some(it) = world.get(id) else { continue };
            node.remove(id);
            if let Some(moved) = moved_item(it, dx, dy) {
                fixed.push((id, node.add(moved)));
            }
        }
        let mut lines = Vec::new();
        for (k, c) in self.conns.iter().enumerate() {
            // `LINE l_new( cn.origLine ); l_new.Unmark(); l_new.ClearLinks(); l_new.DragCorner( cn.p_next, cn.origLine.CLine().Find( cn.p_orig ) )`
            let (p_orig, p_next) = ends[k];
            let mut l_new = c.orig_line.clone();
            l_new.clear_links();
            // `wxCHECK_RET( aIndex >= 0 )`: a point the line does not have leaves it as it was
            if let Some(index) = c.orig_line.find(p_orig) {
                l_new.drag_corner(p_next, index, false, Nm::ZERO);
            }
            // `m_currentNode->Remove( l_orig ); m_currentNode->Add( l_new )`
            node.remove_line_segments(&c.orig_line);
            let source = world.get(c.seed).and_then(|it| it.source_track_id()).map(|t| (t.to_string(), 0));
            l_new.segment_ids = node.add_line_unless_redundant(&l_new, source, false);
            lines.push(l_new);
        }
        ComponentDrag { offset: (dx, dy), node, solids, fixed, lines }
    }

    /// `NODE::CheckColliding( m_draggedItems )`: does a moved pad, a moved track or a new line collide with anything of the drag's world?
    pub fn colliding(drag: &ComponentDrag, rules: &BoardRules) -> bool {
        let node = &drag.node;
        if drag.solids.iter().chain(drag.fixed.iter().map(|(_, id)| id)).any(|&id| node.item_colliding(rules, id).is_some()) {
            return true;
        }
        // a LINE: `const SEGMENT s( *line, l.CSegment( i ) ); QueryColliding( &s, .. )` for each of its segments (its own are of its net)
        drag.lines.iter().any(|l| {
            l.segs().any(|(a, b)| {
                let shape = Shape::Stadium { a, b, r: l.width / 2 };
                node.first_colliding(&shape, &l.net, crate::layer::LayerRange::single(l.layer), rules, &[]).is_some()
            })
        })
    }

    /// `COMPONENT_DRAGGER::FixRoute( aForceCommit )`: the drag at the last position it was given (or where it started), committed when
    /// `Settings().AllowDRCViolations() || aForceCommit || !node->CheckColliding( m_draggedItems )`; `None` otherwise.
    pub fn fix_route(&self, world: &Node, rules: &BoardRules, settings: &RoutingSettings, force_commit: bool) -> Option<ComponentCommit> {
        let drag = self.solve(world, self.last.unwrap_or(self.p0));
        if !(settings.allow_drc_violations() || force_commit || !Self::colliding(&drag, rules)) {
            return None;
        }
        Some(self.commit_of(world, &drag))
    }

    /// What committing `drag` replaces and moves: see the module doc.
    pub fn commit_of(&self, world: &Node, drag: &ComponentDrag) -> ComponentCommit {
        let mut commit = ComponentCommit { footprints: self.footprints.clone(), offset: drag.offset, ..ComponentCommit::default() };
        // every item of the world the drag took (the connections' lines and the fixed items), and the IR tracks they stood on
        let mut covered: HashSet<ItemId> = self.fixed.iter().copied().collect();
        for c in &self.conns {
            covered.extend(c.orig_line.segment_ids.iter().copied());
        }
        let mut touched: BTreeSet<String> = BTreeSet::new();
        for id in &covered {
            if let Some(t) = world.get(*id).and_then(|it| it.source_track_id()) {
                touched.insert(t.to_string());
            }
        }
        // an IR track all of whose items are fixed moves whole: it keeps its id (`UpdateItem`)
        let whole: BTreeSet<String> = touched
            .iter()
            .filter(|t| {
                let mut items = world.iter().filter(|(_, it)| it.source_track_id() == Some(t.as_str())).map(|(id, _)| id).peekable();
                items.peek().is_some() && items.all(|id| self.fixed.contains(&id))
            })
            .cloned()
            .collect();
        commit.moved_tracks = whole.iter().cloned().collect();
        for t in touched.iter().filter(|t| !whole.contains(*t)) {
            commit.remove_track_ids.push(t.clone());
            for run in world.uncovered_track_runs(t, &covered) {
                commit.tracks.push(run);
                commit.sources.push(Some(t.clone()));
            }
        }
        // the connections' new lines (`l_new`), from the track linked at the pad
        for (c, line) in self.conns.iter().zip(&drag.lines) {
            commit.tracks.push(line.clone());
            commit.sources.push(world.get(c.seed).and_then(|it| it.source_track_id()).map(str::to_string));
        }
        // the fixed items of a track that moved only in part: each as it was moved
        for (orig, moved) in &drag.fixed {
            let Some(track) = world.get(*orig).and_then(|it| it.source_track_id()) else { continue };
            if whole.contains(track) {
                continue;
            }
            let Some(it) = drag.node.get(*moved) else { continue };
            let mut line = Line::new(it.net().clone(), it.layers().start(), it.width());
            match it {
                Item::Segment(s) => line.pts = vec![s.a, s.b],
                Item::Arc(a) => line.append_arc(a.arc),
                _ => continue,
            }
            commit.tracks.push(line);
            commit.sources.push(Some(track.to_string()));
        }
        commit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{net_of, Segment, Solid};
    use crate::layer::LayerRange;
    use eda_model::units::mm;

    fn rules() -> BoardRules {
        serde_yaml::from_str("track_width: 200000\nclearance: 200000\nvia_drill: 300000\nvia_diameter: 600000\n").unwrap()
    }

    fn p(x: f64, y: f64) -> Point {
        Point { x: mm(x), y: mm(y) }
    }

    /// A 1 mm square SMD pad `source` of `net` on F.Cu at `at`.
    fn pad(node: &mut Node, source: &str, net: &str, at: Point) -> ItemId {
        let h = mm(0.5);
        node.add(Item::Solid(Solid { net: net_of(net), layers: LayerRange::single(0), pos: at, shape: Shape::Rect { x0: at.x - h, y0: at.y - h, x1: at.x + h, y1: at.y + h }, source: source.into(), edge: false, rule: 0, anchor_points: Vec::new() }))
    }

    /// A 0.2 mm track of `net` on F.Cu through `pts`, one segment per leg, all of IR track `track`.
    fn track(node: &mut Node, net: &str, track: &str, pts: &[Point]) -> Vec<ItemId> {
        pts.windows(2).enumerate().map(|(i, w)| node.add(Item::Segment(Segment { net: net_of(net), layer: 0, a: w[0], b: w[1], width: mm(0.2), source_track: Some((track.into(), i)), locked: false }))).collect()
    }

    /// U1: pad 1 (net A) at the origin with a track straight up 5 mm, pad 2 (net B) 2 mm east of it with a track 5 mm down and 4 mm east.
    fn two_routed_pads() -> (Node, Vec<ItemId>) {
        let mut node = Node::new();
        let pads = vec![pad(&mut node, "U1.1", "A", p(0.0, 0.0)), pad(&mut node, "U1.2", "B", p(2.0, 0.0))];
        track(&mut node, "A", "ta", &[p(0.0, 0.0), p(0.0, -5.0)]);
        track(&mut node, "B", "tb", &[p(2.0, 0.0), p(2.0, 5.0), p(6.0, 5.0)]);
        (node, pads)
    }

    /// `COMPONENT_DRAGGER::Start` + `Drag`: each track that ends on a pad is a connection, and dragging the footprint by (1, 0.5) mm drags each
    /// line's end at its pad with it (`DragCorner( p_next, Find( p_orig ) )`, 45 degrees), the far ends where they were. The corners are KiCad's:
    /// line A (one segment) is `BuildInitialTrace` from its far end, straight first (`DIRECTION_45( P0 - P1 )` is not diagonal): up the x = 0
    /// line to 1 mm short of the pad's new row, then the diagonal onto the pad; line B keeps its far leg (`dragCornerInternal` finds the
    /// vertical leg's own direction from (2, 5)) and turns onto the pad at (2, 1.5).
    #[test]
    fn a_footprint_with_two_routed_pads_drags_its_tracks_along() {
        let (node, pads) = two_routed_pads();
        let mut d = ComponentDragger::start(&node, &rules(), p(0.0, 0.0), vec!["U1".into()], &pads);
        assert_eq!(d.connections().len(), 2, "both tracks follow their pads");
        assert_eq!(d.fixed_items().count(), 0);
        let drag = d.drag(&node, p(1.0, 0.5));
        assert_eq!(drag.offset, (mm(1.0), mm(0.5)));
        let line = |net: &str| drag.lines.iter().find(|l| l.net == net_of(net)).expect("the line").pts.clone();
        assert_eq!(line("A"), vec![p(1.0, 0.5), p(0.0, -0.5), p(0.0, -5.0)]);
        assert_eq!(line("B"), vec![p(3.0, 0.5), p(2.0, 1.5), p(2.0, 5.0), p(6.0, 5.0)]);
        // the pads are where the drag put them, and each line's end is on its pad's centre: the tracks are still connected
        for (k, at) in [(0, p(1.0, 0.5)), (1, p(3.0, 0.5))] {
            let Some(Item::Solid(s)) = drag.node.get(drag.solids[k]) else { panic!("a pad") };
            assert_eq!(s.pos, at);
            let joint = drag.node.joint_at(at, &s.net).expect("a joint at the pad");
            assert!(joint.links.iter().any(|id| drag.node.get(*id).is_some_and(Item::is_track)), "a track ends on the moved pad");
        }
        assert!(!ComponentDragger::colliding(&drag, &rules()));
        let commit = d.fix_route(&node, &rules(), &RoutingSettings::default(), false).expect("commits");
        assert_eq!((commit.footprints.clone(), commit.offset), (vec!["U1".to_string()], (mm(1.0), mm(0.5))));
        let mut removed = commit.remove_track_ids.clone();
        removed.sort();
        assert_eq!(removed, vec!["ta", "tb"]);
        assert_eq!(commit.tracks.len(), 2);
        assert_eq!(commit.sources, vec![Some("ta".to_string()), Some("tb".to_string())]);
        assert!(commit.moved_tracks.is_empty());
    }

    /// "Segments that go directly between two linked pads are special-cased", and so are lines: a track from one pad of the footprint to another
    /// moves as it is with the footprint, and its IR track moves whole (it keeps its id); a line of two segments between two of its pads too.
    #[test]
    fn a_track_between_two_pads_of_the_footprint_moves_as_it_is() {
        let mut node = Node::new();
        let pads = vec![pad(&mut node, "U1.1", "C", p(0.0, 0.0)), pad(&mut node, "U1.2", "C", p(0.0, 2.0)), pad(&mut node, "U1.3", "D", p(4.0, 0.0)), pad(&mut node, "U1.4", "D", p(6.0, 2.0))];
        let straight = track(&mut node, "C", "tc", &[p(0.0, 0.0), p(0.0, 2.0)]);
        let bent = track(&mut node, "D", "td", &[p(4.0, 0.0), p(4.0, -1.0), p(6.0, -1.0), p(6.0, 2.0)]);
        let mut d = ComponentDragger::start(&node, &rules(), p(0.0, 0.0), vec!["U1".into()], &pads);
        assert!(d.connections().is_empty());
        let fixed: Vec<ItemId> = d.fixed_items().collect();
        assert_eq!(fixed.len(), straight.len() + bent.len(), "every segment of both tracks moves as it is");
        let drag = d.drag(&node, p(-1.0, 3.0));
        for (_, moved) in &drag.fixed {
            let Some(Item::Segment(s)) = drag.node.get(*moved) else { panic!() };
            let orig = node.iter().find_map(|(_, it)| match it {
                Item::Segment(o) if o.source_track == s.source_track => Some(o.clone()),
                _ => None,
            });
            let o = orig.unwrap();
            assert_eq!((s.a, s.b), (Point { x: o.a.x - mm(1.0), y: o.a.y + mm(3.0) }, Point { x: o.b.x - mm(1.0), y: o.b.y + mm(3.0) }));
        }
        let commit = d.fix_route(&node, &rules(), &RoutingSettings::default(), false).expect("commits");
        assert_eq!(commit.moved_tracks, vec!["tc".to_string(), "td".to_string()], "both IR tracks move whole");
        assert!(commit.remove_track_ids.is_empty() && commit.tracks.is_empty());
    }

    /// `FixRoute`: a drag whose pads, moved tracks or new lines collide with the world is refused -- unless the click is a Ctrl+click
    /// (`aForceCommit`) or Highlight collisions allows violations (`AllowDRCViolations()`) -- and every position "works" for the preview.
    #[test]
    fn a_drag_that_collides_is_refused_unless_forced() {
        let (mut node, pads) = two_routed_pads();
        // a GND track along the row the pads are dragged onto, short of track B
        track(&mut node, "GND", "tg", &[p(-3.0, 3.0), p(1.5, 3.0)]);
        let rules = rules();
        let mut d = ComponentDragger::start(&node, &rules, p(0.0, 0.0), vec!["U1".into()], &pads);
        let drag = d.drag(&node, p(0.0, 3.0));
        assert!(ComponentDragger::colliding(&drag, &rules), "the pads land on the GND track");
        for mode in [crate::settings::Mode::Shove, crate::settings::Mode::Walkaround, crate::settings::Mode::MarkObstacles] {
            let settings = RoutingSettings { mode, ..RoutingSettings::default() };
            assert!(d.fix_route(&node, &rules, &settings, false).is_none(), "{mode:?}: refused");
            assert!(d.fix_route(&node, &rules, &settings, true).is_some(), "{mode:?}: Ctrl+click commits it");
        }
        let allowed = RoutingSettings { mode: crate::settings::Mode::MarkObstacles, can_violate_drc: true, ..RoutingSettings::default() };
        assert!(d.fix_route(&node, &rules, &allowed, false).is_some(), "Highlight collisions with Allow DRC violations commits it");
        // away from it, it commits
        d.drag(&node, p(0.0, 1.0));
        assert!(d.fix_route(&node, &rules, &RoutingSettings::default(), false).is_some());
    }

    /// The extra joints (`QueryJoints( solid->Hull().BBox(), .. )`): a track of the pad's net that ends inside the pad, off its centre, is asked
    /// `li->Collide( solid, .. )`, which two items of one net never do ("same nets? no clearance!") -- so, as in KiCad at 8303b2ad, it is not a
    /// connection and stays where it is. A track of another net ending there is not of the joint's net and is not taken either.
    #[test]
    fn a_track_ending_off_the_pad_centre_stays_where_it_is() {
        let mut node = Node::new();
        let pads = vec![pad(&mut node, "U1.1", "A", p(0.0, 0.0))];
        track(&mut node, "A", "off", &[p(0.3, 0.0), p(5.0, 0.0)]);
        let mut d = ComponentDragger::start(&node, &rules(), p(0.0, 0.0), vec!["U1".into()], &pads);
        assert!(d.connections().is_empty() && d.fixed_items().count() == 0);
        let drag = d.drag(&node, p(0.0, 1.0));
        assert!(drag.lines.is_empty());
        let commit = d.fix_route(&node, &rules(), &RoutingSettings::default(), false).expect("commits");
        assert!(commit.remove_track_ids.is_empty() && commit.tracks.is_empty());
    }

    /// A track on another layer that ends below an SMD pad's centre is not linked to it (`FindJoint( pos, solid )` asks the pad's layers).
    #[test]
    fn a_track_on_another_layer_under_the_pad_is_not_dragged() {
        let mut node = Node::new();
        let pads = vec![pad(&mut node, "U1.1", "A", p(0.0, 0.0))];
        node.add(Item::Segment(Segment { net: net_of("A"), layer: 1, a: p(0.0, 0.0), b: p(5.0, 0.0), width: mm(0.2), source_track: Some(("back".into(), 0)), locked: false }));
        let d = ComponentDragger::start(&node, &rules(), p(0.0, 0.0), vec!["U1".into()], &pads);
        assert!(d.connections().is_empty());
    }
}
