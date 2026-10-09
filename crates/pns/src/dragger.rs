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
//! - **Dragging a corner in `Mode::Walkaround`** is `dragWalkaround`: the
//!   dragged line is walked around what it would land on
//!   (`DRAGGER::tryWalkaround`), and only refused if it cannot be. A
//!   corner drag in `Mode::Shove` shoves what is in the way
//!   (`dragShove`), in `Mode::MarkObstacles` only reports it.
//! - **Dragging a via** (D7) is the same three modes for the via and the
//!   tracks attached to it: `Mode::Shove` shoves the via itself to the
//!   cursor, attached tracks dragged with it and whatever they run into
//!   pushed aside (`SHOVE::AddHeads( VIA_HANDLE, .. )`, `pushOrShoveVia`),
//!   and when that cannot be done -- or `ShoveVias()` is off -- walks
//!   around instead; `Mode::Walkaround` is that walkaround
//!   (`dragViaWalkaround`: the via pushed off what it sits on by
//!   `VIA::PushoutForce`, `ViaForcePropIterationLimit` steps, the
//!   attached tracks walked around what they would hit).
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
use crate::walkaround::{path_collides, WalkPolicy, WalkStatus, Walker};
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
    /// `Via` only: the layers the via spans.
    pub via_layers: LayerRange,
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
            Item::Via(v) => Some(Dragger { kind: DragKind::Via, net: v.net.clone(), layer: v.layers.start(), width: 0, original: Line::new(v.net.clone(), v.layers.start(), 0), grabbed_index: 0, via_id: Some(item_id), via_pos: v.pos, via_diameter: v.diameter, via_drill: v.drill, via_layers: v.layers, source_via: v.source_via.clone(), free_angle }),
            Item::Segment(seg) => {
                let (seg_a, seg_b) = (seg.a, seg.b);
                let line = node.assemble_line(item_id)?;
                let layer_of_line = line.layer;
                // Drag the nearer endpoint of the clicked segment -- find
                // which of `line.pts`'s two neighbours around the clicked
                // segment is closer to the grab point `at`, by locating
                // the clicked segment's own endpoints within the
                // assembled line first.
                let ia = line.pts.iter().position(|&p| p == seg_a)?;
                let ib = line.pts.iter().position(|&p| p == seg_b)?;
                let grabbed_index = if dist2(at, line.pts[ia]) <= dist2(at, line.pts[ib]) { ia } else { ib };
                Some(Dragger { kind: DragKind::Corner, net: line.net.clone(), layer: line.layer, width: line.width, original: line, grabbed_index, via_id: None, via_pos: Point { x: 0, y: 0 }, via_diameter: 0, via_drill: 0, via_layers: LayerRange::single(layer_of_line), source_via: None, free_angle })
            }
            _ => None,
        }
    }

    /// The tracks attached to the dragged via, as the world has them (`findViaFanoutByHandle`): one assembled line each.
    fn attached_lines(&self, node: &Node) -> Vec<Line> {
        let mut out: Vec<Line> = Vec::new();
        let Some(joint) = node.joint_at(self.via_pos, &self.net) else { return out };
        for &id in &joint.links {
            if let Some(Item::Segment(_)) = node.get(id) {
                if let Some(l) = node.assemble_line(id) {
                    if !out.iter().any(|o| o.segment_ids == l.segment_ids) {
                        out.push(l);
                    }
                }
            }
        }
        out
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
                let mut fanout = self.attached_lines(node);
                for l in fanout.iter_mut() {
                    if l.first() == Some(self.via_pos) {
                        l.pts[0] = to;
                    } else if l.last() == Some(self.via_pos) {
                        let last = l.pts.len() - 1;
                        l.pts[last] = to;
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
        if settings.mode == Mode::MarkObstacles || self.free_angle {
            // MarkObstacles: just report collisions.
            let layers = LayerRange::single(self.layer);
            let mut colliding = main_pts.windows(2).any(|w| {
                let shape = Shape::Stadium { a: w[0], b: w[1], r: self.width.max(1) / 2 };
                node.first_colliding(&shape, &self.net, layers, rules, &exclude).is_some()
            });
            if self.kind == DragKind::Via {
                let shape = Shape::Circle { c: to, r: self.via_diameter / 2 };
                colliding |= node.first_colliding(&shape, &self.net, self.via_layers, rules, &exclude).is_some();
            }
            for l in &fanout {
                colliding |= l.segs().any(|(a, b)| {
                    let shape = Shape::Stadium { a, b, r: l.width.max(1) / 2 };
                    node.first_colliding(&shape, &l.net, LayerRange::single(l.layer), rules, &exclude).is_some()
                });
            }
            return DragPreview { pts: main_pts, colliding, displaced_lines: Vec::new(), displaced_vias: Vec::new(), fanout };
        }

        // `dragWalkaround`: the dragged line (or via and its tracks) is walked around what it would land on.
        if settings.mode == Mode::Walkaround {
            return match self.kind {
                DragKind::Via => self.drag_via_walkaround(node, rules, settings, to),
                DragKind::Corner => {
                    let colliding = path_collides(node, rules, &self.net, self.layer, self.width.max(1), &main_pts, &exclude);
                    let walked = if colliding { try_walkaround(node, rules, settings, &self.net, self.layer, self.width.max(1), &main_pts) } else { Some(main_pts.clone()) };
                    match walked.filter(|w| w.len() >= 2) {
                        Some(pts) => DragPreview { pts, colliding: false, displaced_lines: Vec::new(), displaced_vias: Vec::new(), fanout: Vec::new() },
                        None => DragPreview { pts: main_pts, colliding: true, displaced_lines: Vec::new(), displaced_vias: Vec::new(), fanout: Vec::new() },
                    }
                }
            };
        }

        // `dragShove` for a via: the via is the head.
        if self.kind == DragKind::Via {
            return self.drag_via_shove(node, rules, settings, to);
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
            main_pts = head;
        }
        let fanout = fanout;

        DragPreview {
            pts: main_pts,
            colliding,
            displaced_lines: displaced_lines.into_values().flatten().collect(),
            displaced_vias: displaced_vias.into_iter().map(|(source_via, pos)| DisplacedVia { source_via, pos }).collect(),
            fanout,
        }
    }

    /// The IR tracks `line` stands on.
    fn source_tracks(node: &Node, line: &Line) -> Vec<String> {
        let mut out: Vec<String> = line
            .segment_ids
            .iter()
            .filter_map(|id| match node.get(*id) {
                Some(Item::Segment(s)) => s.source_track.as_ref().map(|(t, _)| t.clone()),
                _ => None,
            })
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// `DRAGGER::dragShove`, `DM_VIA`: the via is the head of a shove (`SHOVE::AddHeads( VIA_HANDLE, aP, SHP_SHOVE )`): it is
    /// moved to the cursor, the tracks attached to it go with it, and what they and the via collide with is pushed aside. The
    /// via ends where the shove left it, which is not the cursor if a pad or the board edge pushed it on. When the shove
    /// cannot be done -- `ShoveVias()` is off, the via is locked, or something will not give -- the drag walks around instead.
    fn drag_via_shove(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, to: Point) -> DragPreview {
        let Some(outcome) = self.via_id.and_then(|id| shove::shove_via(node, id, to, rules, settings)) else {
            return self.drag_via_walkaround(node, rules, settings, to);
        };
        let end = outcome.head.first().copied().unwrap_or(to);
        let mut displaced_lines = outcome.displaced_lines;
        // The attached tracks, as the shove left them: the displaced line that now ends at the via, in place of the original.
        let mut fanout = Vec::new();
        for original in self.attached_lines(node) {
            let sources = Self::source_tracks(node, &original);
            let found = displaced_lines.iter().position(|d| d.line.point_count() >= 2 && d.source_track.as_ref().is_some_and(|t| sources.contains(t)) && (d.line.first() == Some(end) || d.line.last() == Some(end)));
            // A track the drag shortened to nothing has no line left; it is removed when the drag is committed.
            let pts = found.map(|i| displaced_lines.remove(i).line.pts).unwrap_or_else(|| vec![end]);
            fanout.push(Line { pts, ..original });
        }
        let displaced_vias = outcome.displaced_vias.into_iter().filter(|v| Some(&v.source_via) != self.source_via.as_ref()).collect();
        DragPreview { pts: vec![end], colliding: false, displaced_lines, displaced_vias, fanout }
    }

    /// `DRAGGER::dragViaWalkaround`: the via is pushed off whatever it would sit on (`propagateViaForces`: `VIA::PushoutForce`,
    /// at most `ViaForcePropIterationLimit` steps, with the way back as the lead), then every track attached to it is dragged to
    /// the via's new place -- at 45 degrees (`DragCorner`) -- and walked around what it would hit
    /// ([`try_walkaround`]). A via that cannot be pushed free, or a track that cannot be walked, is a collision.
    fn drag_via_walkaround(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, to: Point) -> DragPreview {
        let exclude = self.exclude();
        let via = crate::item::Via { net: self.net.clone(), layers: self.via_layers, pos: to, diameter: self.via_diameter, drill: self.via_drill, source_via: self.source_via.clone(), locked: false };
        let lead = ((self.via_pos.x - to.x) as i64, (self.via_pos.y - to.y) as i64);
        let Some(force) = shove::via_pushout_force(node, rules, &via, lead, crate::node::kind_mask::ANY, settings.via_force_prop_iteration_limit, &exclude) else {
            // can't force-propagate the via? bummer...
            let (pts, fanout) = self.candidate(node, to);
            return DragPreview { pts, colliding: true, displaced_lines: Vec::new(), displaced_vias: Vec::new(), fanout };
        };
        let target = Point { x: to.x + force.0 as Um, y: to.y + force.1 as Um };
        let mut colliding = false;
        let mut fanout = Vec::new();
        for original in self.attached_lines(node) {
            let mut dragged = original.clone();
            if let Some(at) = dragged.find(self.via_pos) {
                dragged.drag_corner45(target, at);
                dragged.simplify();
            }
            if dragged.point_count() >= 2 && path_collides(node, rules, &dragged.net, dragged.layer, dragged.width.max(1), &dragged.pts, &exclude) {
                match try_walkaround(node, rules, settings, &dragged.net, dragged.layer, dragged.width.max(1), &dragged.pts) {
                    Some(walked) => dragged.pts = walked,
                    None => colliding = true,
                }
            }
            fanout.push(dragged);
        }
        DragPreview { pts: vec![target], colliding, displaced_lines: Vec::new(), displaced_vias: Vec::new(), fanout }
    }

    /// `DRAGGER::FixRoute`: commit the drag at `to`, subject to the same
    /// collision guard `LinePlacer::fix` uses (refuse outside
    /// `MarkObstacles` if still colliding). `None` on refusal.
    pub fn finish(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, to: Point) -> Option<DragCommit> {
        let preview = self.preview(node, rules, settings, to);
        if preview.colliding && !settings.allow_drc_violations() {
            return None;
        }
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
                // where the drag left the via: the cursor, or further, if a shove or a pushout moved it on
                let end = preview.pts.first().copied().unwrap_or(to);
                commit.vias.push((end, self.via_diameter, self.via_drill, self.source_via.clone()));
                for l in &preview.fanout {
                    for src in Self::source_tracks(node, l) {
                        if !commit.remove_track_ids.contains(&src) {
                            commit.remove_track_ids.push(src);
                        }
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

/// `DRAGGER::tryWalkaround`: walk `pts` around what it runs into (`WP_SHORTEST`, the walkaround iteration limit, a detour of up
/// to 30 times the line's length), or `None` if it cannot be done.
fn try_walkaround(node: &Node, rules: &BoardRules, settings: &RoutingSettings, net: &Net, layer: i32, width: Um, pts: &[Point]) -> Option<Vec<Point>> {
    let mut walker = Walker::new(node, rules);
    walker.iteration_limit = settings.walkaround_iteration_limit.max(0) as u32;
    walker.length_limit_on = true;
    walker.length_expansion_factor = 30.0;
    walker.set_allowed_policies(&[WalkPolicy::Shortest]);
    let outcome = walker.route(net, layer, width, pts);
    (outcome.status_of(WalkPolicy::Shortest) == WalkStatus::Done).then(|| outcome.line_of(WalkPolicy::Shortest).to_vec())
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
        // `DragCorner` re-solves the track's last leg at 45 degrees (`dragViaWalkaround`)
        let elbow = vec![Point { x: 0, y: 0 }, Point { x: 0, y: 1000 }, to];
        assert_eq!(preview.fanout[0].pts, elbow, "the fanout track must already end at the via's live (not yet committed) position");
        let commit = dragger.finish(&node, &rules, &settings, to).expect("collision-free via drag must commit");
        assert_eq!(commit.remove_via_ids, vec!["viaA"]);
        assert_eq!(commit.vias[0].0, to);
        assert_eq!(commit.remove_track_ids, vec!["trkA"]);
        assert_eq!(commit.tracks[0].pts, elbow, "the attached track must stretch to follow the via");
        // Highlight collisions only moves the end: the stretch is the straight line.
        let marking = RoutingSettings { mode: Mode::MarkObstacles, ..settings };
        assert_eq!(dragger.preview(&node, &rules, &marking, to).fanout[0].pts, vec![Point { x: 0, y: 0 }, to]);
    }

    /// A via, SIG, with a track coming in from the left, in a world with a GND track straight across where the via is dragged to.
    fn via_across_a_gnd_track() -> (Node, Dragger) {
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: -3000, y: 0 }, b: Point { x: 0, y: 0 }, width: 200, source_track: Some(("trkA".into(), 0)), locked: false }));
        let via_id = node.add(Item::Via(Via { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: Point { x: 0, y: 0 }, diameter: 600, drill: 300, source_via: Some("viaA".into()), locked: false }));
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: 4000, y: -3000 }, b: Point { x: 4000, y: 3000 }, width: 200, source_track: Some(("trkB".into(), 0)), locked: false }));
        let dragger = Dragger::start(&node, Point { x: 0, y: 0 }, via_id).expect("a via drag");
        assert_eq!(dragger.kind, DragKind::Via);
        (node, dragger)
    }

    /// D7, `DRAGGER::dragShove` for `DM_VIA`: the via is the head of a shove, the track attached to it goes with it and the
    /// GND track in its way is pushed aside, in one commit.
    #[test]
    fn shove_mode_via_drag_pushes_what_is_in_the_way_of_the_via_and_its_track() {
        let (node, dragger) = via_across_a_gnd_track();
        let rules = rules();
        let settings = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let to = Point { x: 5000, y: 0 };
        let preview = dragger.preview(&node, &rules, &settings, to);
        assert!(!preview.colliding, "{preview:?}");
        assert_eq!(preview.pts, vec![to], "nothing sits where the cursor is: the via goes there");
        assert_eq!(preview.fanout.len(), 1);
        assert_eq!(preview.fanout[0].last(), Some(to), "the track follows the via: {:?}", preview.fanout[0].pts);
        assert_eq!(preview.displaced_lines.len(), 1, "the GND track is pushed out of the way: {:?}", preview.displaced_lines);
        assert_eq!(preview.displaced_lines[0].source_track.as_deref(), Some("trkB"));
        let commit = dragger.finish(&node, &rules, &settings, to).expect("the shove commits");
        assert_eq!(commit.vias[0].0, to);
        let mut removed = commit.remove_track_ids.clone();
        removed.sort();
        assert_eq!(removed, vec!["trkA", "trkB"]);
        // and the board it leaves is clean: nothing the router's own collision test calls a violation
        let mut after = Node::new();
        after.add(Item::Via(Via { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: to, diameter: 600, drill: 300, source_via: None, locked: false }));
        for l in &commit.tracks {
            after.add_line(l, None, false);
        }
        for (id, item) in after.iter() {
            assert!(after.all_colliding(&item.shape(item.layers().start()), item.net(), item.layers(), &rules, &[id]).is_empty(), "{item:?} collides after the commit");
        }
    }

    /// With "Shove vias" off the via may not be shoved (`SH_TRY_WALK`), and the drag walks around instead -- the track the via
    /// would have pushed is walked around by the attached track.
    #[test]
    fn with_shove_vias_off_a_shove_mode_via_drag_walks_around_instead() {
        let (node, dragger) = via_across_a_gnd_track();
        let rules = rules();
        let settings = RoutingSettings { mode: Mode::Shove, shove_vias: false, ..RoutingSettings::default() };
        let to = Point { x: 5000, y: 0 };
        let preview = dragger.preview(&node, &rules, &settings, to);
        assert!(preview.displaced_lines.is_empty(), "nothing is pushed: {:?}", preview.displaced_lines);
        // the via is dragged across the GND track too (a via cannot walk around), so the drag is in collision with it
        assert!(preview.colliding || preview.fanout[0].pts.len() > 2, "{preview:?}");
    }

    /// `dragViaWalkaround`: a via dropped on a pad is pushed off it by `VIA::PushoutForce`, in Walk around mode and in Shove mode
    /// (where it is pushed by the shove), and the track attached to it follows to where the via ends.
    #[test]
    fn a_via_dragged_onto_a_pad_ends_clear_of_it() {
        use crate::item::Solid;
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: -3000, y: 0 }, b: Point { x: 0, y: 0 }, width: 200, source_track: Some(("trkA".into(), 0)), locked: false }));
        let via_id = node.add(Item::Via(Via { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: Point { x: 0, y: 0 }, diameter: 600, drill: 300, source_via: Some("viaA".into()), locked: false }));
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: 4000, y: 0 }, shape: Shape::Circle { c: Point { x: 4000, y: 0 }, r: 500 }, source: "U1.1".into(), edge: false }));
        let dragger = Dragger::start(&node, Point { x: 0, y: 0 }, via_id).unwrap();
        let rules = rules();
        let onto_the_pad = Point { x: 4000, y: 300 };
        for mode in [Mode::Walkaround, Mode::Shove] {
            let settings = RoutingSettings { mode, ..RoutingSettings::default() };
            let preview = dragger.preview(&node, &rules, &settings, onto_the_pad);
            assert!(!preview.colliding, "{mode:?}: {preview:?}");
            let end = preview.pts[0];
            let centre_distance = (((end.x - 4000) as f64).powi(2) + (end.y as f64).powi(2)).sqrt();
            assert!(centre_distance >= 500.0 + 300.0 + 200.0 - 2.0, "{mode:?}: the via ended {centre_distance} um from the pad's centre, at {end:?}");
            assert_eq!(preview.fanout[0].last(), Some(end), "{mode:?}: the track ends at the via");
            let commit = dragger.finish(&node, &rules, &settings, onto_the_pad).expect("a via that clears commits");
            assert_eq!(commit.vias[0].0, end, "{mode:?}: the via is committed where it ended, not where the cursor was");
        }
        // Highlight collisions leaves it where the cursor put it, and says so.
        let marking = RoutingSettings { mode: Mode::MarkObstacles, ..RoutingSettings::default() };
        let preview = dragger.preview(&node, &rules, &marking, onto_the_pad);
        assert!(preview.colliding);
        assert_eq!(preview.pts, vec![onto_the_pad]);
    }

    /// `dragWalkaround` for a corner: a Walk around drag that lands on something is walked around it, and refused only when it cannot be.
    #[test]
    fn walkaround_mode_corner_drag_is_walked_around_a_pad() {
        use crate::item::Solid;
        let mut node = Node::new();
        let seg_id = node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: 0, y: 0 }, b: Point { x: 1000, y: 0 }, width: 200, source_track: Some(("trkA".into(), 0)), locked: false }));
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: 3000, y: 0 }, shape: Shape::Circle { c: Point { x: 3000, y: 0 }, r: 400 }, source: "U1.1".into(), edge: false }));
        let rules = rules();
        let dragger = Dragger::start(&node, Point { x: 1000, y: 0 }, seg_id).unwrap();
        let settings = RoutingSettings::default();
        assert_eq!(settings.mode, Mode::Walkaround);
        let preview = dragger.preview(&node, &rules, &settings, Point { x: 5000, y: 0 });
        assert!(!preview.colliding, "the drag is walked around the pad: {preview:?}");
        assert!(preview.pts.len() > 2 && preview.pts.last() == Some(&Point { x: 5000, y: 0 }) && preview.pts.first() == Some(&Point { x: 0, y: 0 }), "{:?}", preview.pts);
        let commit = dragger.finish(&node, &rules, &settings, Point { x: 5000, y: 0 }).expect("the walked drag commits");
        assert_eq!(commit.tracks[0].pts, preview.pts);
        // Dropped on the pad itself, there is nowhere for the end to go.
        assert!(dragger.preview(&node, &rules, &settings, Point { x: 3000, y: 0 }).colliding);
        assert!(dragger.finish(&node, &rules, &settings, Point { x: 3000, y: 0 }).is_none());
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
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: 3000, y: 0 }, shape: Shape::Circle { c: Point { x: 3000, y: 0 }, r: 400 }, source: "U1.1".into(), edge: false }));
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
        let pad_id = node.add(Item::Solid(Solid { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: Point { x: 0, y: 0 }, shape: Shape::Circle { c: Point { x: 0, y: 0 }, r: 400 }, source: "U1.1".into(), edge: false }));
        assert!(Dragger::start(&node, Point { x: 0, y: 0 }, pad_id).is_none());
    }
}
