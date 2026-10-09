//! Port of `PNS::DRAGGER` (`pcbnew/router/pns_dragger.{h,cpp}`): drag an
//! existing track segment/corner or via without disconnecting it --
//! whatever was attached to it stretches to follow, and (in shove mode)
//! anything the new shape runs into gets pushed aside, reusing
//! `crate::shove` exactly the way upstream's own `DRAGGER` reuses `SHOVE`.
//!
//! Scoped down from upstream (see `PARITY.md`):
//! - **Corner drag only, no segment-sideways-slide.** KiCad distinguishes
//!   grabbing near a segment's endpoint (`DM_CORNER`: relocate that one
//!   vertex) from grabbing its middle (`DM_SEGMENT`: slide the whole run
//!   sideways, inserting new connecting segments at 45 degrees). This port
//!   always drags the *nearer endpoint* of whatever segment was clicked --
//!   the common "reroute this track's end" gesture -- and does not
//!   implement the sideways-slide case.
//! - **Free-angle corner relocation**, not KiCad's default 45-degree-
//!   constrained `dragCorner45` (which re-solves the two adjacent
//!   segments' directions to keep a clean corner). The dragged vertex
//!   simply moves to the cursor; `crate::optimizer` can clean up the
//!   result afterward the same way it cleans up a shoved line.
//! - No springback/incremental-branch reuse, for the same reason
//!   `crate::shove` has none: every `preview`/`finish` call re-derives
//!   everything fresh from the committed world.
//! - `Mode::Walkaround` during a drag behaves like `Mode::MarkObstacles`
//!   (collisions are reported, not resolved) -- KiCad's own walkaround-
//!   while-dragging path is a secondary mode this port doesn't implement;
//!   `Mode::Shove` is the one that actually keeps the dragged item
//!   obstacle-free.
//!
//! ## Free-angle mode (`DM_FREE_ANGLE`, `G`)
//!
//! `pcbnew.InteractiveRouter.DragFreeAngle` starts the same drag with
//! `DM_ANY | DM_FREE_ANGLE`. In `DRAGGER` that flag (`m_freeAngleMode`)
//! does exactly two things, both ported here via [`Dragger::free_angle`]:
//!
//! 1. `startDragSegment`: a grab in the middle of a segment drags its
//!    nearer vertex (`DM_CORNER`) instead of sliding the whole segment
//!    sideways (`DM_SEGMENT`) -- which is what [`Dragger::start`] already
//!    does for every drag, so this port's corner drag *is* the free-angle
//!    path (the 45-degree-constrained `dragCorner45` of plain `D` is the
//!    part this port does not have).
//! 2. `DRAGGER::Start`/`Drag`: shove is never set up and `Drag()` always
//!    takes `dragMarkObstacles` -- whatever `Settings().Mode()` says --
//!    so a free-angle drag only reports collisions, and `FixRoute` refuses
//!    to commit a colliding result unless `AllowDRCViolations()`.

use crate::item::{Item, ItemId, Net};
use crate::layer::LayerRange;
use crate::line::Line;
use crate::node::Node;
use crate::settings::{Mode, RoutingSettings};
use crate::shove::{self, DisplacedLine, DisplacedVia};
use eda_drc::kimath::Shape;
use eda_model::ir::{Point, Um};
use eda_model::BoardRules;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragKind {
    /// Dragging one end of an assembled track line; `grabbed_index` is
    /// which point of `original.pts` follows the cursor.
    Corner,
    Via,
}

pub struct Dragger {
    pub kind: DragKind,
    pub net: Net,
    pub layer: i32,
    pub width: Um,
    /// The assembled line at drag start (`Corner`) -- its own segment ids
    /// are exactly what must be removed from the committed board on
    /// finish. Unused (empty) for `Via`.
    pub original: Line,
    pub grabbed_index: usize,
    /// `Via` only: the via's own identity/position/size at drag start.
    pub via_id: Option<ItemId>,
    pub via_pos: Point,
    pub via_diameter: Um,
    pub via_drill: Um,
    pub source_via: Option<String>,
    /// `m_freeAngleMode` (`DM_FREE_ANGLE`, the `G` hotkey): see the module
    /// doc -- never shoves, only marks obstacles.
    pub free_angle: bool,
}

#[derive(Debug, Clone)]
pub struct DragPreview {
    pub pts: Vec<Point>,
    pub colliding: bool,
    pub displaced_lines: Vec<DisplacedLine>,
    pub displaced_vias: Vec<DisplacedVia>,
    /// `DragKind::Via` only: the via's own directly-attached tracks, each
    /// stretched to follow the via's live position. KiCad's real
    /// `InlineDrag` visibly stretches a dragged item's own connections in
    /// real time -- that's the whole point of a router-driven drag over a
    /// plain move -- so the frontend needs a shape to draw for them while
    /// the drag is still in progress, not just once it commits. Always
    /// empty for `DragKind::Corner` (no fanout at all -- see
    /// [`Dragger::candidate`]). Identical in every [`Mode`]: shove only
    /// ever displaces *other* items out of a pusher's way, it never
    /// reroutes the pusher's own path (the same reason
    /// `shove::ShoveOutcome::head` always echoes back its own input), so
    /// each fanout leg's shape here is simply its own candidate position,
    /// not something shove could have altered.
    pub fanout: Vec<Line>,
}

/// What a finished drag replaces/adds -- `crate::router::Router` turns
/// this into the same `RouteCommit` shape a finished route produces, so
/// both go through the identical `Cmd::CommitRoute` seam.
#[derive(Debug, Clone, Default)]
pub struct DragCommit {
    pub remove_track_ids: Vec<String>,
    pub remove_via_ids: Vec<String>,
    pub tracks: Vec<Line>,
    pub vias: Vec<(Point, Um, Um, Option<String>)>,
}

fn dist2(a: Point, b: Point) -> i64 {
    let (dx, dy) = (a.x - b.x, a.y - b.y);
    dx * dx + dy * dy
}

impl Dragger {
    /// `DRAGGER::Start`: identify what's being dragged from whatever item
    /// is at `item_id` and where the grab point `at` falls on it. `None`
    /// if `item_id` names something that can't be dragged (a pad) or
    /// doesn't exist.
    pub fn start(node: &Node, at: Point, item_id: ItemId) -> Option<Dragger> {
        Self::start_with(node, at, item_id, false)
    }

    /// [`Self::start`] with `DM_FREE_ANGLE` (`free_angle`) -- `DRAGGER::Start`
    /// reads it from the drag mode: `m_freeAngleMode = (m_mode & DM_FREE_ANGLE)`.
    pub fn start_with(node: &Node, at: Point, item_id: ItemId, free_angle: bool) -> Option<Dragger> {
        let item = node.get(item_id)?;
        match item {
            Item::Via(v) => Some(Dragger { kind: DragKind::Via, net: v.net.clone(), layer: v.layers.start(), width: 0, original: Line::new(v.net.clone(), v.layers.start(), 0), grabbed_index: 0, via_id: Some(item_id), via_pos: v.pos, via_diameter: v.diameter, via_drill: v.drill, source_via: v.source_via.clone(), free_angle }),
            Item::Segment(seg) => {
                let (seg_a, seg_b) = (seg.a, seg.b);
                let line = node.assemble_line(item_id)?;
                // Drag the nearer endpoint of the clicked segment -- find
                // which of `line.pts`'s two neighbours around the clicked
                // segment is closer to the grab point `at`, by locating
                // the clicked segment's own endpoints within the
                // assembled line first.
                let ia = line.pts.iter().position(|&p| p == seg_a)?;
                let ib = line.pts.iter().position(|&p| p == seg_b)?;
                let grabbed_index = if dist2(at, line.pts[ia]) <= dist2(at, line.pts[ib]) { ia } else { ib };
                Some(Dragger { kind: DragKind::Corner, net: line.net.clone(), layer: line.layer, width: line.width, original: line, grabbed_index, via_id: None, via_pos: Point { x: 0, y: 0 }, via_diameter: 0, via_drill: 0, source_via: None, free_angle })
            }
            _ => None,
        }
    }

    /// The candidate geometry at `to`, before any shove/collision
    /// resolution: `(dragged line points, via-fanout lines each as (net,
    /// layer, width, points))`.
    fn candidate(&self, node: &Node, to: Point) -> (Vec<Point>, Vec<Line>) {
        match self.kind {
            DragKind::Corner => {
                let mut pts = self.original.pts.clone();
                pts[self.grabbed_index] = to;
                (pts, Vec::new())
            }
            DragKind::Via => {
                let mut fanout = Vec::new();
                if let Some(joint) = node.joint_at(self.via_pos, &self.net) {
                    for &id in &joint.links {
                        if let Some(Item::Segment(_)) = node.get(id) {
                            if let Some(mut l) = node.assemble_line(id) {
                                if l.first() == Some(self.via_pos) {
                                    l.pts[0] = to;
                                } else if l.last() == Some(self.via_pos) {
                                    let last = l.pts.len() - 1;
                                    l.pts[last] = to;
                                }
                                fanout.push(l);
                            }
                        }
                    }
                }
                (vec![to], fanout)
            }
        }
    }

    fn exclude(&self) -> Vec<ItemId> {
        let mut ex = self.original.segment_ids.clone();
        if let Some(id) = self.via_id {
            ex.push(id);
        }
        ex
    }

    /// `DRAGGER::Drag`: resolve the candidate shape at `to` per the active
    /// mode. Pure -- never mutates `node`.
    pub fn preview(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, to: Point) -> DragPreview {
        let (main_pts, fanout) = self.candidate(node, to);
        let exclude = self.exclude();

        // `DRAGGER::Drag`: `if( m_freeAngleMode || m_forceMarkObstaclesMode ) dragMarkObstacles`.
        if settings.mode != Mode::Shove || self.free_angle {
            // MarkObstacles (and Walkaround, simplified to the same thing
            // here -- see the module doc comment): just report collisions.
            let layers = LayerRange::single(self.layer);
            let mut colliding = main_pts.windows(2).any(|w| {
                let shape = Shape::Stadium { a: w[0], b: w[1], r: self.width.max(1) / 2 };
                node.first_colliding(&shape, &self.net, layers, rules, &exclude).is_some()
            });
            if self.kind == DragKind::Via {
                let shape = Shape::Circle { c: to, r: self.via_diameter / 2 };
                colliding |= node.first_colliding(&shape, &self.net, LayerRange::new(self.layer, self.layer), rules, &exclude).is_some();
            }
            for l in &fanout {
                colliding |= l.segs().any(|(a, b)| {
                    let shape = Shape::Stadium { a, b, r: l.width.max(1) / 2 };
                    node.first_colliding(&shape, &l.net, LayerRange::single(l.layer), rules, &exclude).is_some()
                });
            }
            return DragPreview { pts: main_pts, colliding, displaced_lines: Vec::new(), displaced_vias: Vec::new(), fanout };
        }

        // Shove mode: push whatever the dragged shape(s) now collide with.
        // The main dragged line and each via-fanout leg are independent
        // pushers against the same committed world; a real KiCad via drag
        // would resolve them as one coupled operation, but running them
        // sequentially against the same (unbranched, read-only) `node`
        // and merging their displaced sets is a reasonable approximation
        // at this project's board sizes -- each leg almost always touches
        // disjoint obstacles. A pusher the shove had to walk around a pad
        // (`onCollidingSolid`) comes back with its walked shape, which is the
        // shape the drag then has.
        let mut displaced_lines: std::collections::BTreeMap<String, Vec<DisplacedLine>> = std::collections::BTreeMap::new();
        let mut displaced_vias = std::collections::BTreeMap::new();
        let mut colliding = false;

        let mut push = |pts: &[Point], net: &Net, layer: i32, width: Um| -> Option<Vec<Point>> {
            match shove::shove_line(node, pts, net, layer, width, rules, settings) {
                Some(outcome) => {
                    // the latest shove's lines for a track replace an earlier one's
                    let mut fresh: std::collections::BTreeMap<String, Vec<DisplacedLine>> = std::collections::BTreeMap::new();
                    for d in outcome.displaced_lines {
                        if let Some(id) = d.source_track.clone() {
                            fresh.entry(id).or_default().push(d);
                        }
                    }
                    displaced_lines.extend(fresh);
                    for d in outcome.displaced_vias {
                        displaced_vias.insert(d.source_via.clone(), d.pos);
                    }
                    Some(outcome.head)
                }
                None => {
                    colliding = true;
                    None
                }
            }
        };
        let mut main_pts = main_pts;
        if let Some(head) = push(&main_pts, &self.net, self.layer, self.width.max(1)) {
            if self.kind == DragKind::Corner {
                main_pts = head;
            }
        }
        let mut fanout = fanout;
        for l in fanout.iter_mut() {
            if let Some(head) = push(&l.pts, &l.net, l.layer, l.width.max(1)) {
                l.pts = head;
            }
        }

        DragPreview {
            pts: main_pts,
            colliding,
            displaced_lines: displaced_lines.into_values().flatten().collect(),
            displaced_vias: displaced_vias.into_iter().map(|(source_via, pos)| DisplacedVia { source_via, pos }).collect(),
            fanout,
        }
    }

    /// `DRAGGER::FixRoute`: commit the drag at `to`, subject to the same
    /// collision guard `LinePlacer::fix` uses (refuse outside
    /// `MarkObstacles` if still colliding). `None` on refusal.
    pub fn finish(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, to: Point) -> Option<DragCommit> {
        let preview = self.preview(node, rules, settings, to);
        if preview.colliding && !settings.allow_drc_violations() {
            return None;
        }
        let (_, fanout) = self.candidate(node, to);
        let mut commit = DragCommit::default();

        match self.kind {
            DragKind::Corner => {
                commit.remove_track_ids.extend(self.original.segment_ids.iter().filter_map(|id| match node.get(*id) {
                    Some(Item::Segment(s)) => s.source_track.as_ref().map(|(t, _)| t.clone()),
                    _ => None,
                }));
                commit.tracks.push(Line { pts: preview.pts, ..self.original.clone() });
            }
            DragKind::Via => {
                if let Some(src) = &self.source_via {
                    commit.remove_via_ids.push(src.clone());
                }
                commit.vias.push((to, self.via_diameter, self.via_drill, self.source_via.clone()));
                for l in &fanout {
                    if let Some(src) = l.segment_ids.iter().find_map(|id| match node.get(*id) {
                        Some(Item::Segment(s)) => s.source_track.as_ref().map(|(t, _)| t.clone()),
                        _ => None,
                    }) {
                        commit.remove_track_ids.push(src);
                    }
                    commit.tracks.push(l.clone());
                }
            }
        }

        for d in &preview.displaced_lines {
            if let Some(id) = &d.source_track {
                // a track can be named once per line that stood on it
                if !commit.remove_track_ids.contains(id) {
                    commit.remove_track_ids.push(id.clone());
                }
                commit.tracks.push(d.line.clone());
            }
        }
        for d in &preview.displaced_vias {
            commit.remove_via_ids.push(d.source_via.clone());
            // Diameter/drill placeholders: `Dragger` only knows the size
            // of the one via it's directly dragging, not an incidentally-
            // shoved one -- `Router::drag_finish` re-resolves the real
            // values from the board itself before building the `Via` IR,
            // the same way a finished route's own displaced vias do.
            commit.vias.push((d.pos, 0, 0, Some(d.source_via.clone())));
        }
        Some(commit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{net_of, Segment, Via};

    fn rules() -> BoardRules {
        serde_yaml::from_str("track_width: 200\nclearance: 200\nvia_drill: 300\nvia_diameter: 600\n").unwrap()
    }

    #[test]
    fn drags_a_track_end_and_keeps_the_other_end_attached() {
        let mut node = Node::new();
        let seg_id = node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: 0, y: 0 }, b: Point { x: 1000, y: 0 }, width: 200, source_track: Some(("trkA".into(), 0)), locked: false }));
        let rules = rules();
        let settings = RoutingSettings::default();
        let dragger = Dragger::start(&node, Point { x: 1000, y: 0 }, seg_id).expect("must start a corner drag");
        assert_eq!(dragger.kind, DragKind::Corner);
        let preview = dragger.preview(&node, &rules, &settings, Point { x: 1000, y: 1000 });
        assert!(!preview.colliding);
        assert_eq!(preview.pts.first(), Some(&Point { x: 0, y: 0 }), "the ungrabbed end must stay put");
        assert_eq!(preview.pts.last(), Some(&Point { x: 1000, y: 1000 }), "the grabbed end follows the cursor");
        let commit = dragger.finish(&node, &rules, &settings, Point { x: 1000, y: 1000 }).expect("collision-free drag must commit");
        assert_eq!(commit.remove_track_ids, vec!["trkA"]);
        assert_eq!(commit.tracks.len(), 1);
        assert_eq!(commit.tracks[0].pts, vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 1000 }]);
    }

    #[test]
    fn dragging_a_via_drags_its_connected_track_with_it() {
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: 0, y: 0 }, b: Point { x: 1000, y: 0 }, width: 200, source_track: Some(("trkA".into(), 0)), locked: false }));
        let via_id = node.add(Item::Via(Via { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: Point { x: 1000, y: 0 }, diameter: 600, drill: 300, source_via: Some("viaA".into()), locked: false }));
        let rules = rules();
        let settings = RoutingSettings::default();
        let dragger = Dragger::start(&node, Point { x: 1000, y: 0 }, via_id).expect("must start a via drag");
        assert_eq!(dragger.kind, DragKind::Via);
        let to = Point { x: 1000, y: 2000 };
        let preview = dragger.preview(&node, &rules, &settings, to);
        assert!(!preview.colliding);
        assert_eq!(preview.fanout.len(), 1, "the live preview must carry the attached track's stretched shape too, not just the via's own new point");
        assert_eq!(preview.fanout[0].pts, vec![Point { x: 0, y: 0 }, to], "the fanout track must already end at the via's live (not yet committed) position");
        let commit = dragger.finish(&node, &rules, &settings, to).expect("collision-free via drag must commit");
        assert_eq!(commit.remove_via_ids, vec!["viaA"]);
        assert_eq!(commit.vias[0].0, to);
        assert_eq!(commit.remove_track_ids, vec!["trkA"]);
        assert_eq!(commit.tracks[0].pts, vec![Point { x: 0, y: 0 }, to], "the attached track must stretch to follow the via");
    }

    #[test]
    fn shove_mode_pushes_an_obstacle_out_of_a_dragged_tracks_new_path() {
        let mut node = Node::new();
        let seg_id = node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: 0, y: 0 }, b: Point { x: 1000, y: 0 }, width: 200, source_track: Some(("trkA".into(), 0)), locked: false }));
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: 2500, y: -2000 }, b: Point { x: 2500, y: 2000 }, width: 200, source_track: Some(("trkB".into(), 0)), locked: false }));
        let rules = rules();
        let settings = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let dragger = Dragger::start(&node, Point { x: 1000, y: 0 }, seg_id).unwrap();
        let preview = dragger.preview(&node, &rules, &settings, Point { x: 5000, y: 0 });
        assert!(!preview.colliding, "shove must clear the crossing track rather than reporting a collision");
        assert_eq!(preview.displaced_lines.len(), 1);
        assert_eq!(preview.displaced_lines[0].source_track.as_deref(), Some("trkB"));
    }

    /// A Shove-mode drag into a pad no longer gives up: the dragged line is walked around the pad
    /// (`onCollidingSolid`), and the walked shape is the one the drag has -- committing the straight
    /// line through the pad would be a violation.
    #[test]
    fn shove_mode_drag_into_a_pad_walks_the_dragged_line_around_it() {
        use crate::item::Solid;
        use crate::layer::LayerRange;
        let mut node = Node::new();
        let seg_id = node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: 0, y: 0 }, b: Point { x: 1000, y: 0 }, width: 200, source_track: Some(("trkA".into(), 0)), locked: false }));
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: 3000, y: 0 }, shape: Shape::Circle { c: Point { x: 3000, y: 0 }, r: 400 }, source: "U1.1".into() }));
        let rules = rules();
        let settings = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let dragger = Dragger::start(&node, Point { x: 1000, y: 0 }, seg_id).unwrap();
        let preview = dragger.preview(&node, &rules, &settings, Point { x: 6000, y: 0 });
        assert!(!preview.colliding);
        assert!(preview.pts.len() > 2, "walked around the pad: {:?}", preview.pts);
        assert_eq!(preview.pts.first(), Some(&Point { x: 0, y: 0 }));
        assert_eq!(preview.pts.last(), Some(&Point { x: 6000, y: 0 }));
        for w in preview.pts.windows(2) {
            let leg = Shape::Stadium { a: w[0], b: w[1], r: 100 };
            assert!(leg.collides(&Shape::Circle { c: Point { x: 3000, y: 0 }, r: 400 }, 199).is_none(), "{:?} touches the pad", w);
        }
        // and what finish() commits is that shape
        let commit = dragger.finish(&node, &rules, &settings, Point { x: 6000, y: 0 }).expect("the walked drag is accepted");
        assert_eq!(commit.tracks[0].pts, preview.pts);
    }

    #[test]
    fn free_angle_drag_never_shoves_it_only_reports_the_collision() {
        let mut node = Node::new();
        let seg_id = node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: 0, y: 0 }, b: Point { x: 1000, y: 0 }, width: 200, source_track: Some(("trkA".into(), 0)), locked: false }));
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: 2500, y: -2000 }, b: Point { x: 2500, y: 2000 }, width: 200, source_track: Some(("trkB".into(), 0)), locked: false }));
        let rules = rules();
        // Shove mode: a plain drag pushes the crossing track aside ...
        let settings = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let plain = Dragger::start(&node, Point { x: 1000, y: 0 }, seg_id).unwrap();
        assert!(!plain.free_angle);
        assert!(!plain.preview(&node, &rules, &settings, Point { x: 5000, y: 0 }).colliding);
        // ... but DM_FREE_ANGLE (`G`) always takes `dragMarkObstacles`: nothing is displaced, the collision is reported.
        let free = Dragger::start_with(&node, Point { x: 1000, y: 0 }, seg_id, true).unwrap();
        assert!(free.free_angle);
        let preview = free.preview(&node, &rules, &settings, Point { x: 5000, y: 0 });
        assert!(preview.colliding, "a free-angle drag must report, not resolve, the collision");
        assert!(preview.displaced_lines.is_empty());
        // `FixRoute` refuses the colliding drop (outside MarkObstacles + can_violate_drc) ...
        assert!(free.finish(&node, &rules, &settings, Point { x: 5000, y: 0 }).is_none());
        // ... and commits a clear one, free angle included (the grabbed end simply follows the cursor).
        let commit = free.finish(&node, &rules, &settings, Point { x: 1000, y: 1000 }).expect("a clear free-angle drop commits");
        assert_eq!(commit.tracks[0].pts, vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 1000 }]);
    }

    #[test]
    fn starting_on_a_pad_refuses() {
        use crate::item::Solid;
        let mut node = Node::new();
        let pad_id = node.add(Item::Solid(Solid { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: Point { x: 0, y: 0 }, shape: Shape::Circle { c: Point { x: 0, y: 0 }, r: 400 }, source: "U1.1".into() }));
        assert!(Dragger::start(&node, Point { x: 0, y: 0 }, pad_id).is_none());
    }
}
