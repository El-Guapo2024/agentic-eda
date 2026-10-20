//! Port of `PNS::DRAGGER` (`pcbnew/router/pns_dragger.{h,cpp}`): drag an
//! existing track segment/corner or via without disconnecting it --
//! whatever was attached to it stretches to follow, and (in shove mode)
//! anything the new shape runs into gets pushed aside, reusing
//! `crate::shove` exactly the way upstream's own `DRAGGER` reuses `SHOVE`.
//!
//! ## What is grabbed (`DRAGGER::Start`, `startDragSegment`)
//!
//! - Grab a track within half its width of one of its ends and it is a **corner** drag (`DM_CORNER`): that vertex follows the
//!   cursor, the legs next to it re-solved at 45 degrees (`LINE::DragCorner`, `dragCorner45`).
//! - Grab it anywhere else and it is a **segment** drag (`DM_SEGMENT`): the whole segment slides sideways, at whatever angle it
//!   had (the 45-degree heading it quantizes to), its neighbours stretching or turning to follow (`LINE::DragSegment`,
//!   `dragSegment45`: [`crate::line::Line::drag_segment45`]).
//! - Grab a via and the via moves with every track attached to it dragged along.
//! - Grab an **arc** track (`DM_ARC`) and the arc's curvature follows the cursor, tangent to the tracks next to it ([`crate::arc_drag`]).
//!   Elsewhere a drag of a segment or a corner of a line that has an arc in it leaves the arc as it is (`dragSegment45` and `dragCorner45` keep an
//!   arc's points apart from the ones they move).
//! - `G` (free-angle, `DM_FREE_ANGLE`): a grab in the middle of a segment is a corner drag too (the nearer vertex), the vertex goes
//!   to the cursor at any angle (`dragCornerFree`), and the drag only marks obstacles.
//!
//! ## What a position gives (`Drag`)
//!
//! A drag that is refused at one cursor position is not lost: [`Dragger::drag`] is `DRAGGER::Drag` with its memory. Each position
//! the mode can resolve (Highlight collisions: any, with the collisions marked; Walk around: the dragged line walked round what it
//! lands on; Shove: whatever is in the way pushed aside) becomes the last valid solution and point. A position it cannot resolve
//! shows the last valid solution again (`m_lastDragSolution`), and a release there commits the last valid point
//! (`DRAGGER::FixRoute` re-runs `Drag( m_lastValidPoint )`). If the very first position fails, the drag goes on in Highlight
//! collisions mode (`m_forceMarkObstaclesMode`): the violations are shown, and only Ctrl+click commits them.
//!
//! Scoped down from upstream (see `PARITY.md`):
//! - No springback/incremental-branch reuse, for the same reason
//!   `crate::shove` has none: every `preview`/`finish` call re-derives
//!   everything fresh from the committed world.
//! - **Dragging a corner in `Mode::Walkaround`** is `dragWalkaround`: the
//!   dragged line is walked around what it would land on
//!   (`DRAGGER::tryWalkaround`), and only refused if it cannot be. A
//!   corner or segment drag in `Mode::Shove` shoves what is in the way
//!   (`dragShove`, with `SHP_REVERSED` when the first vertex is dragged),
//!   in `Mode::MarkObstacles` only reports it.
//! - **Dragging a via** (D7) is the same three modes for the via and the
//!   tracks attached to it: `Mode::Shove` shoves the via itself to the
//!   cursor, attached tracks dragged with it and whatever they run into
//!   pushed aside (`SHOVE::AddHeads( VIA_HANDLE, .. )`, `pushOrShoveVia`),
//!   and when that cannot be done -- or `ShoveVias()` is off -- walks
//!   around instead; `Mode::Walkaround` is that walkaround
//!   (`dragViaWalkaround`: the via pushed off what it sits on by
//!   `VIA::PushoutForce`, `ViaForcePropIterationLimit` steps, the
//!   attached tracks walked around what they would hit).

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
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragKind {
    /// `DM_CORNER`: one vertex of an assembled track follows the cursor; `grabbed_index` is which point of `original.pts`.
    Corner,
    /// `DM_SEGMENT`: a whole segment of the assembled track slides sideways; `grabbed_index` is which segment of `original`.
    Segment,
    Via,
    /// `DM_ARC`: the arc of an assembled track changes its curvature; `grabbed_index` is the first chord of it in `original`.
    Arc,
}

pub struct Dragger {
    pub kind: DragKind,
    pub net: Net,
    pub layer: i32,
    pub width: Um,
    /// The assembled line at drag start (`Corner`, `Segment`) -- its own segment ids
    /// are exactly what must be removed from the committed board on
    /// finish. Unused (empty) for `Via`.
    pub original: Line,
    /// `m_draggedSegmentIndex`: the vertex (`Corner`) or the segment (`Segment`) of `original` being dragged.
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
    /// `DragKind::Arc` only: what `DragArcTrack` works out before its loop.
    arc: Option<crate::arc_drag::ArcDrag>,
    /// Where the drag started (`m_lastValidPoint = aP` in `Start`).
    start_point: Point,
    /// What `Drag` remembers from one cursor position to the next.
    state: DragState,
}

/// `DRAGGER`'s `m_lastNode` / `m_lastValidPoint` / `m_lastDragSolution` / `m_dragStatus` / `m_forceMarkObstaclesMode`.
#[derive(Debug, Clone, Default)]
struct DragState {
    /// `m_lastNode != nullptr`: `Drag` has been called.
    started: bool,
    /// `m_forceMarkObstaclesMode`: the first position failed, so the drag only marks obstacles.
    force_mark_obstacles: bool,
    /// `m_dragStatus`: the position `Drag` was last given can be committed.
    drag_status: bool,
    /// `m_lastValidPoint`.
    last_valid: Option<Point>,
    /// `m_lastDragSolution`: what the last position that worked gave.
    last_solution: Option<DragPreview>,
}

#[derive(Debug, Clone, Default)]
pub struct DragPreview {
    pub pts: Vec<Point>,
    /// The arcs `pts` still has as arcs (the dragged line's [`Line::arcs`]).
    pub arcs: Vec<crate::arcs::ArcPts>,
    pub colliding: bool,
    pub displaced_lines: Vec<DisplacedLine>,
    pub displaced_vias: Vec<DisplacedVia>,
    /// `DragKind::Via` only: the via's own directly-attached tracks, each
    /// stretched to follow the via's live position. KiCad's real
    /// `InlineDrag` visibly stretches a dragged item's own connections in
    /// real time -- that's the whole point of a router-driven drag over a
    /// plain move -- so the frontend needs a shape to draw for them while
    /// the drag is still in progress, not just once it commits. Always
    /// empty for `DragKind::Corner`/`Segment` (the whole stretched line is `pts`). Identical in every [`Mode`]: shove only
    /// ever displaces *other* items out of a pusher's way, it never
    /// reroutes the pusher's own path (the same reason
    /// `shove::ShoveOutcome::head` always echoes back its own input), so
    /// each fanout leg's shape here is simply its own candidate position,
    /// not something shove could have altered.
    pub fanout: Vec<Line>,
    /// This is not what the cursor's position gave but the last position that worked: the cursor is somewhere the drag cannot
    /// be resolved (`Drag`: "Restore last solution").
    pub restored: bool,
    /// The drag is in Highlight collisions mode because its first position failed (`GetForceMarkObstaclesMode`): `colliding` then
    /// says whether releasing here is a violation, which only Ctrl+click commits.
    pub forced_mark_obstacles: bool,
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

/// What a drag may not take hold of (`DRAGGER::Start`, `ROUTER_TOOL::performDragging`).
pub const LOCKED_MESSAGE: &str = "The selected item is locked.";

/// `m_world->AssembleLine( aSeg, &m_draggedSegmentIndex )`: the defaults of `NODE::AssembleLine` -- the line stops in front of a locked segment (`aFollowLockedSegments`
/// false), so a locked track next to the dragged line stays where it is -- and goes on through a change of track width.
fn assemble(node: &Node, id: ItemId) -> Option<Line> {
    node.assemble_line_with(id, crate::node::AssembleOpts { follow_locked_segments: false, allow_width_mismatch: true })
}

fn dist(a: Point, b: Point) -> f64 {
    let (dx, dy) = ((a.x - b.x) as f64, (a.y - b.y) as f64);
    dx.hypot(dy)
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
        Self::start_checked(node, at, item_id, free_angle).ok()
    }

    /// [`Self::start_with`] with the reason when there is nothing to drag (`EDIT_TOOL::DragArcTrack`'s message for an arc that is too wide).
    pub fn start_checked(node: &Node, at: Point, item_id: ItemId, free_angle: bool) -> Result<Dragger, String> {
        const CANT: &str = "that item can't be dragged (only track segments/corners, arcs and vias can)";
        let item = node.get(item_id).ok_or(CANT)?;
        // `ROUTER_TOOL::performDragging`: "The selected item is locked." -- "Drag Anyway" is [`crate::router::Router::drag_start_unlocking`]
        if item.is_locked() {
            return Err(LOCKED_MESSAGE.to_string());
        }
        match item {
            Item::Arc(_) => {
                // `startDragArc`: the line the arc is part of, `DM_ARC`
                let line = assemble(node, item_id).ok_or(CANT)?;
                let index = line.segment_ids.iter().position(|&id| id == item_id).ok_or(CANT)?.min(line.segment_count().saturating_sub(1));
                let run = line.arc_runs().into_iter().find(|r| index >= r.first && index < r.last).ok_or(CANT)?;
                let drag = crate::arc_drag::ArcDrag::new(&line, run)?.ok_or("The arc has no corner to round: its tangents do not meet.")?;
                Ok(Dragger { kind: DragKind::Arc, net: line.net.clone(), layer: line.layer, width: line.width, original: line.clone(), grabbed_index: run.first, via_id: None, via_pos: Point { x: 0, y: 0 }, via_diameter: 0, via_drill: 0, source_via: None, via_layers: LayerRange::single(line.layer), free_angle, arc: Some(drag), start_point: at, state: DragState::default() })
            }
            _ => Self::start_plain(node, at, item_id, free_angle).ok_or_else(|| CANT.to_string()),
        }
    }

    fn start_plain(node: &Node, at: Point, item_id: ItemId, free_angle: bool) -> Option<Dragger> {
        let item = node.get(item_id)?;
        match item {
            Item::Via(v) => Some(Dragger { kind: DragKind::Via, net: v.net.clone(), layer: v.layers.start(), width: 0, original: Line::new(v.net.clone(), v.layers.start(), 0), grabbed_index: 0, via_id: Some(item_id), via_pos: v.pos, via_diameter: v.diameter, via_drill: v.drill, via_layers: v.layers, source_via: v.source_via.clone(), free_angle, arc: None, start_point: at, state: DragState::default() }),
            Item::Segment(seg) => {
                // `startDragSegment`
                let (seg_a, seg_b, w2) = (seg.a, seg.b, seg.width / 2);
                let line = assemble(node, item_id)?;
                let layer_of_line = line.layer;
                // `m_world->AssembleLine( aSeg, &m_draggedSegmentIndex )`: which segment of the line the grabbed one is
                let mut index = line.segment_ids.iter().position(|&id| id == item_id)?.min(line.segment_count().saturating_sub(1));
                let (dist_a, dist_b) = (dist(at, seg_a), dist(at, seg_b));
                let kind;
                if dist_a < w2 as f64 || dist_b < w2 as f64 {
                    // within half its width of an end: that end is the corner to drag
                    kind = DragKind::Corner;
                    if dist_b <= dist_a {
                        index += 1;
                    }
                } else if free_angle {
                    // "( m_draggedSegmentIndex < PointCount() - 2 )": the line's own last vertex is not taken by a grab in the middle
                    if dist_b < dist_a && (index as isize) < line.point_count() as isize - 2 {
                        index += 1;
                    }
                    kind = DragKind::Corner;
                } else {
                    kind = DragKind::Segment;
                }
                Some(Dragger { kind, net: line.net.clone(), layer: line.layer, width: line.width, original: line, grabbed_index: index, via_id: None, via_pos: Point { x: 0, y: 0 }, via_diameter: 0, via_drill: 0, via_layers: LayerRange::single(layer_of_line), source_via: None, free_angle, arc: None, start_point: at, state: DragState::default() })
            }
            _ => None,
        }
    }

    /// The tracks attached to the dragged via, as the world has them (`findViaFanoutByHandle`): one assembled line each.
    fn attached_lines(&self, node: &Node) -> Vec<Line> {
        let mut out: Vec<Line> = Vec::new();
        let Some(joint) = node.joint_at(self.via_pos, &self.net) else { return out };
        for &id in &joint.links {
            if node.get(id).is_some_and(Item::is_track) {
                if let Some(l) = assemble(node, id) {
                    if !out.iter().any(|o| o.segment_ids == l.segment_ids) {
                        out.push(l);
                    }
                }
            }
        }
        out
    }

    /// The candidate geometry for a via at `to`, before any shove/collision resolution: the via-fanout lines, each with its end at the
    /// via dragged to `to`.
    fn via_candidate(&self, node: &Node, to: Point) -> Vec<Line> {
        let mut fanout = self.attached_lines(node);
        for l in fanout.iter_mut() {
            if l.first() == Some(self.via_pos) {
                l.pts[0] = to;
            } else if l.last() == Some(self.via_pos) {
                let last = l.pts.len() - 1;
                l.pts[last] = to;
            }
        }
        fanout
    }

    fn exclude(&self) -> Vec<ItemId> {
        let mut ex = self.original.segment_ids.clone();
        if let Some(id) = self.via_id {
            ex.push(id);
        }
        ex
    }

    /// `m_snapThreshhold` of the dragged line: `SmoothDraggedSegments() ? Width() / 4 : 0` for Highlight collisions and Walk around,
    /// `Width() / 2` for Shove (`dragMarkObstacles`, `dragWalkaround`, `dragShove`).
    fn snap_threshold(&self, settings: &RoutingSettings, shove: bool) -> Um {
        if !settings.smooth_dragged_segments {
            0
        } else if shove {
            self.width / 2
        } else {
            self.width / 4
        }
    }

    /// The dragged line's points with the cursor at `to`: `LINE::DragSegment` for a segment drag, `LINE::DragCorner` for a corner drag
    /// (in `free_angle` mode `dragCornerFree`).
    fn dragged_line(&self, to: Point, snap: Um, free_angle: bool) -> Line {
        let mut line = self.original.clone();
        match self.kind {
            DragKind::Segment => line.drag_segment45(to, self.grabbed_index, snap),
            // `DragArcTrack`: the arc through the cursor, tangent to the tracks next to it
            DragKind::Arc => {
                if let Some((_, dragged)) = self.arc.as_ref().and_then(|a| a.at(&self.original, to)) {
                    line = dragged;
                }
            }
            _ => line.drag_corner(to, self.grabbed_index, free_angle, snap),
        }
        line
    }

    /// `DRAGGER::dragMarkObstacles`: put the dragged shape where the cursor is and say whether it collides. Always "works".
    fn solve_mark_obstacles(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, to: Point) -> DragPreview {
        let exclude = self.exclude();
        if self.kind == DragKind::Via {
            let fanout = self.via_candidate(node, to);
            let shape = Shape::Circle { c: to, r: self.via_diameter / 2 };
            let mut colliding = node.first_colliding(&shape, &self.net, self.via_layers, rules, &exclude).is_some();
            for l in &fanout {
                colliding |= l.segs().any(|(a, b)| {
                    let shape = Shape::Stadium { a, b, r: l.width.max(1) / 2 };
                    node.first_colliding(&shape, &l.net, LayerRange::single(l.layer), rules, &exclude).is_some()
                });
            }
            return DragPreview { pts: vec![to], colliding, fanout, ..DragPreview::default() };
        }
        let line = self.dragged_line(to, self.snap_threshold(settings, false), self.free_angle);
        let layers = LayerRange::single(self.layer);
        let colliding = line.pts.windows(2).any(|w| {
            let shape = Shape::Stadium { a: w[0], b: w[1], r: self.width.max(1) / 2 };
            node.first_colliding(&shape, &self.net, layers, rules, &exclude).is_some()
        });
        DragPreview { pts: line.pts, arcs: line.arcs, colliding, ..DragPreview::default() }
    }

    /// `DRAGGER::dragWalkaround`: the dragged line (or via and its tracks) is walked around what it would land on. `None` when it
    /// cannot be (`ok` false).
    fn solve_walkaround(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, to: Point) -> Option<DragPreview> {
        if self.kind == DragKind::Via {
            let pv = self.drag_via_walkaround(node, rules, settings, to);
            return (!pv.colliding).then_some(pv);
        }
        let exclude = self.exclude();
        let dragged = self.dragged_line(to, self.snap_threshold(settings, false), false);
        let pts = dragged.pts.clone();
        let colliding = path_collides(node, rules, &self.net, self.layer, self.width.max(1), &pts, &exclude);
        // an arc is not walked round anything: the arc drag either fits where the cursor puts it or is refused
        let walked = if !colliding {
            Some(pts)
        } else if self.kind == DragKind::Arc {
            None
        } else {
            try_walkaround(node, rules, settings, &self.net, self.layer, self.width.max(1), &pts)
        };
        let walked = walked.filter(|w| w.len() >= 2)?;
        // `optimizeAndUpdateDraggedLine`: with "Optimize entire track being dragged" off -- KiCad's default, and all this port has --
        // the walked line is kept as it is
        Some(DragPreview { pts: walked, arcs: dragged.arcs, ..DragPreview::default() })
    }

    /// `DRAGGER::dragShove`: whatever the dragged shape runs into is pushed aside, a pad walked around. `None` when it cannot be
    /// (`ok` false); a via that will not be shoved is walked around instead, as upstream does.
    fn solve_shove(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, to: Point) -> Option<DragPreview> {
        if self.kind == DragKind::Via {
            let pv = self.drag_via_shove(node, rules, settings, to);
            return (!pv.colliding).then_some(pv);
        }
        let dragged = self.dragged_line(to, self.snap_threshold(settings, true), false);
        // `m_mode == DM_CORNER && m_draggedSegmentIndex == 0`: the head is dragged by its first vertex, so its last point is the pusher
        let reversed = self.kind == DragKind::Corner && self.grabbed_index == 0;
        let outcome = shove::shove_dragged_line(node, &self.original.segment_ids, &dragged.pts, &dragged.arcs, &self.net, self.layer, self.width.max(1), rules, settings, reversed)?;
        Some(DragPreview { pts: outcome.head, arcs: dragged.arcs, displaced_lines: outcome.displaced_lines, displaced_vias: outcome.displaced_vias, ..DragPreview::default() })
    }

    /// One cursor position: what `dragMarkObstacles` / `dragWalkaround` / `dragShove` make of it, and whether it `ok`s -- their return
    /// value, true for every position when marking obstacles.
    fn solve(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, to: Point, mark_only: bool) -> (DragPreview, bool) {
        // `DRAGGER::Drag`: `if( m_freeAngleMode || m_forceMarkObstaclesMode ) dragMarkObstacles`, else by `m_currentMode`
        if mark_only || self.free_angle || settings.mode == Mode::MarkObstacles {
            return (self.solve_mark_obstacles(node, rules, settings, to), true);
        }
        let resolved = match settings.mode {
            Mode::Walkaround => self.solve_walkaround(node, rules, settings, to),
            _ => self.solve_shove(node, rules, settings, to),
        };
        match resolved {
            Some(pv) => (pv, true),
            None => {
                // what the position would be, flagged: it collides
                let mut pv = self.solve_mark_obstacles(node, rules, settings, to);
                pv.colliding = true;
                (pv, false)
            }
        }
    }

    /// `DRAGGER::Drag` without its memory: the shape the cursor at `to` gives, resolved per the active mode, for a drag that has not
    /// failed yet. Pure -- never mutates `node` or `self`; [`Self::drag`] is the one a drag session uses.
    pub fn preview(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, to: Point) -> DragPreview {
        let mut pv = self.solve(node, rules, settings, to, self.state.force_mark_obstacles).0;
        pv.forced_mark_obstacles = self.state.force_mark_obstacles;
        pv
    }

    /// Can a drag that ended in `pv` be committed? `DRAGGER::m_dragStatus`: `dragMarkObstacles` says
    /// `AllowDRCViolations() || !CheckColliding( dragged )`, the others that they resolved the position.
    fn status_of(&self, pv: &DragPreview, ok: bool, mark: bool, settings: &RoutingSettings) -> bool {
        if mark {
            settings.allow_drc_violations() || !pv.colliding
        } else {
            ok
        }
    }

    /// `DRAGGER::Drag( aP )`: the live preview of the drag at the cursor position `to`, remembering what worked.
    ///
    /// A position the mode can resolve becomes the last valid solution and point. One it cannot shows the last valid solution again
    /// (`restored`), and leaves the drag uncommittable until a position works (`FixRoute` then goes back to the last valid
    /// point). If the *first* position fails, the drag carries on in Highlight collisions mode (`m_forceMarkObstaclesMode`).
    pub fn drag(&mut self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, to: Point) -> DragPreview {
        let first = !self.state.started;
        self.state.started = true;
        let mark = self.free_angle || self.state.force_mark_obstacles || settings.mode == Mode::MarkObstacles;
        let (mut pv, ok) = self.solve(node, rules, settings, to, mark);
        if ok {
            self.state.last_valid = Some(to);
            self.state.drag_status = self.status_of(&pv, ok, mark, settings);
            pv.forced_mark_obstacles = self.state.force_mark_obstacles;
            self.state.last_solution = Some(pv.clone());
            return pv;
        }
        if first {
            // "First collision resolution failed, switch to highlight mode"
            self.state.force_mark_obstacles = true;
            let mut pv = self.solve_mark_obstacles(node, rules, settings, to);
            self.state.last_valid = Some(to);
            self.state.drag_status = self.status_of(&pv, true, true, settings);
            pv.forced_mark_obstacles = true;
            return pv;
        }
        // "Restore last solution": m_dragStatus stays false, the line goes back to where the last good position put it
        self.state.drag_status = false;
        let mut restored = self.state.last_solution.clone().unwrap_or_else(|| DragPreview { pts: self.original.pts.clone(), arcs: self.original.arcs.clone(), ..DragPreview::default() });
        restored.restored = true;
        restored.forced_mark_obstacles = self.state.force_mark_obstacles;
        restored
    }

    /// Whether the position `Drag` was last given can be committed (`m_dragStatus`).
    pub fn drag_status(&self) -> bool {
        self.state.drag_status
    }

    /// `GetForceMarkObstaclesMode`: the drag only marks obstacles, because its first position failed.
    pub fn force_mark_obstacles(&self) -> bool {
        self.state.force_mark_obstacles
    }

    /// The IR tracks the dragged line stands on (nothing for a via): the snapping must not put the cursor on them
    /// (`TOOL_BASE::checkSnap`: "if we're dragging a track segment, don't try to snap to items that are part of the original line").
    pub fn line_tracks(&self, node: &Node) -> Vec<String> {
        Self::source_tracks(node, &self.original)
    }

    /// The IR tracks `line` stands on.
    fn source_tracks(node: &Node, line: &Line) -> Vec<String> {
        let mut out: Vec<String> = line.segment_ids.iter().filter_map(|id| node.get(*id).and_then(|it| it.source_track_id()).map(str::to_string)).collect();
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
        DragPreview { pts: vec![end], colliding: false, displaced_lines, displaced_vias, fanout, ..DragPreview::default() }
    }

    /// `DRAGGER::dragViaWalkaround`: the via is pushed off whatever it would sit on (`propagateViaForces`: `VIA::PushoutForce`,
    /// at most `ViaForcePropIterationLimit` steps, with the way back as the lead), then every track attached to it is dragged to
    /// the via's new place -- at 45 degrees (`DragCorner`) -- and walked around what it would hit
    /// ([`try_walkaround`]). A via that cannot be pushed free, or a track that cannot be walked, is a collision.
    fn drag_via_walkaround(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, to: Point) -> DragPreview {
        let exclude = self.exclude();
        let via = crate::item::Via { net: self.net.clone(), layers: self.via_layers, pos: to, diameter: self.via_diameter, drill: self.via_drill, source_via: self.source_via.clone(), locked: false };
        let lead = (self.via_pos.x - to.x, self.via_pos.y - to.y);
        let Some(force) = shove::via_pushout_force(node, rules, &via, lead, crate::node::kind_mask::ANY, settings.via_force_prop_iteration_limit, &exclude) else {
            // can't force-propagate the via? bummer...
            let fanout = self.via_candidate(node, to);
            return DragPreview { pts: vec![to], colliding: true, fanout, ..DragPreview::default() };
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
        DragPreview { pts: vec![target], colliding, fanout, ..DragPreview::default() }
    }

    /// `DRAGGER::FixRoute( aForceCommit )`: commit the drag at `to`. The cursor goes there first (`Drag`); if that works the result is
    /// committed. If not -- the position is illegal -- and the drag is in Highlight collisions mode, only `force_commit` (Ctrl+click)
    /// commits it, violations and all; otherwise the drag is run again at the last valid point and *that* is committed ("all tracks
    /// to be committed will be in valid positions, even if the current routing solution to the mouse cursor is invalid"). `None`
    /// when there is nothing to commit.
    pub fn finish(&mut self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, to: Point, force_commit: bool) -> Option<DragCommit> {
        let pv = self.drag(node, rules, settings, to);
        if self.state.drag_status {
            return Some(self.commit_of(node, &pv));
        }
        if self.state.force_mark_obstacles {
            return force_commit.then(|| self.commit_of(node, &pv));
        }
        let last = self.state.last_valid.unwrap_or(self.start_point);
        let pv = self.drag(node, rules, settings, last);
        self.state.drag_status.then(|| self.commit_of(node, &pv))
    }

    /// What committing the drag as `preview` replaces and adds.
    fn commit_of(&self, node: &Node, preview: &DragPreview) -> DragCommit {
        let mut commit = DragCommit::default();
        match self.kind {
            DragKind::Corner | DragKind::Segment | DragKind::Arc => {
                let tracks = Self::source_tracks(node, &self.original);
                commit.remove_track_ids.extend(tracks.iter().cloned());
                commit.tracks.push(Line { pts: preview.pts.clone(), arcs: preview.arcs.clone(), ..self.original.clone() });
                // an IR track the dragged line stood on only in part keeps the rest (a junction cuts a track into lines); a track the
                // shove moved too has been handed back by the shove
                let covered: HashSet<ItemId> = self.original.segment_ids.iter().copied().collect();
                let shoved: HashSet<&String> = preview.displaced_lines.iter().filter_map(|d| d.source_track.as_ref()).collect();
                for t in tracks.iter().filter(|t| !shoved.contains(t)) {
                    commit.tracks.extend(node.uncovered_track_runs(t, &covered));
                }
            }
            DragKind::Via => {
                if let Some(src) = &self.source_via {
                    commit.remove_via_ids.push(src.clone());
                }
                // where the drag left the via: the cursor, or further, if a shove or a pushout moved it on
                let end = preview.pts.first().copied().unwrap_or(self.via_pos);
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
        commit
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

    fn p(x: i64, y: i64) -> Point {
        Point { x, y }
    }

    fn on_45(pts: &[Point]) -> bool {
        pts.windows(2).all(|w| {
            let (dx, dy) = ((w[1].x - w[0].x).abs(), (w[1].y - w[0].y).abs());
            dx == 0 || dy == 0 || dx == dy
        })
    }

    /// A SIG track of `pts` on layer 0 (width 200) in the node, one `Segment` per leg, all of IR track `track`.
    fn add_track(node: &mut Node, net: &str, track: &str, pts: &[Point]) -> Vec<ItemId> {
        pts.windows(2).enumerate().map(|(i, w)| node.add(Item::Segment(Segment { net: net_of(net), layer: 0, a: w[0], b: w[1], width: 200, source_track: Some((track.into(), i)), locked: false }))).collect()
    }

    #[test]
    fn drags_a_track_end_and_keeps_the_other_end_attached() {
        let mut node = Node::new();
        let seg_id = node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: 0, y: 0 }, b: Point { x: 1000, y: 0 }, width: 200, source_track: Some(("trkA".into(), 0)), locked: false }));
        let rules = rules();
        let settings = RoutingSettings::default();
        let mut dragger = Dragger::start(&node, Point { x: 1000, y: 0 }, seg_id).expect("must start a corner drag");
        assert_eq!(dragger.kind, DragKind::Corner);
        let preview = dragger.preview(&node, &rules, &settings, Point { x: 1000, y: 1000 });
        assert!(!preview.colliding);
        assert_eq!(preview.pts.first(), Some(&Point { x: 0, y: 0 }), "the ungrabbed end must stay put");
        assert_eq!(preview.pts.last(), Some(&Point { x: 1000, y: 1000 }), "the grabbed end follows the cursor");
        let commit = dragger.finish(&node, &rules, &settings, Point { x: 1000, y: 1000 }, false).expect("collision-free drag must commit");
        assert_eq!(commit.remove_track_ids, vec!["trkA"]);
        assert_eq!(commit.tracks.len(), 1);
        assert_eq!(commit.tracks[0].pts, vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 1000 }]);
    }

    /// `startDragSegment`: within half the width of an end it is that corner, anywhere else the segment; `G` makes a middle grab a corner.
    #[test]
    fn grabbing_near_an_end_drags_the_corner_and_anywhere_else_the_segment() {
        let mut node = Node::new();
        let ids = add_track(&mut node, "SIG", "t", &[p(0, 0), p(10_000, 0), p(10_000, 5000)]);
        let at = |x, y, free| Dragger::start_with(&node, p(x, y), ids[0], free).unwrap();
        let near_a = at(50, 30, false);
        assert_eq!((near_a.kind, near_a.grabbed_index), (DragKind::Corner, 0), "50 um from the first end: its vertex");
        let near_b = at(9_950, 0, false);
        assert_eq!((near_b.kind, near_b.grabbed_index), (DragKind::Corner, 1), "within half the width of the second end: the vertex after it");
        let middle = at(5000, 0, false);
        assert_eq!((middle.kind, middle.grabbed_index), (DragKind::Segment, 0), "the middle of the segment slides it");
        let past_the_pad_of_the_end = at(9_850, 0, false);
        assert_eq!(past_the_pad_of_the_end.kind, DragKind::Segment, "150 um from an end of a 200 um track is the track, not the corner");
        // free angle: the nearer vertex, but the line's own last vertex is not taken by a grab in the middle of its last segment
        let free_b = at(7000, 0, true);
        assert_eq!((free_b.kind, free_b.grabbed_index), (DragKind::Corner, 1));
        let last = Dragger::start_with(&node, p(10_000, 3000), ids[1], true).unwrap();
        assert_eq!((last.kind, last.grabbed_index), (DragKind::Corner, 1), "nearer the end, but vertex 2 is the line's last: it stays with the segment's start");
    }

    /// `DM_SEGMENT`: the diagonal between two straight legs slides, the legs stretching to follow; one IR track in, one out.
    #[test]
    fn a_segment_drag_slides_the_segment_and_its_neighbours_follow_at_45_degrees() {
        let mut node = Node::new();
        let ids = add_track(&mut node, "SIG", "t", &[p(0, 0), p(2000, 0), p(3000, 1000), p(5000, 1000)]);
        let (rules, settings) = (rules(), RoutingSettings::default());
        let mut dragger = Dragger::start(&node, p(2500, 500), ids[1]).unwrap();
        assert_eq!((dragger.kind, dragger.grabbed_index), (DragKind::Segment, 1));
        let pv = dragger.drag(&node, &rules, &settings, p(2500, 1500));
        assert!(!pv.colliding && !pv.restored);
        // the diagonal now goes through (2500, 1500): x - y = 1000
        assert_eq!(pv.pts, vec![p(0, 0), p(1000, 0), p(2000, 1000), p(5000, 1000)]);
        let commit = dragger.finish(&node, &rules, &settings, p(2500, 1500), false).expect("a clear slide commits");
        assert_eq!(commit.remove_track_ids, vec!["t"]);
        assert_eq!(commit.tracks.len(), 1);
        assert_eq!(commit.tracks[0].pts, pv.pts);
        assert_eq!((commit.tracks[0].net.clone(), commit.tracks[0].width), (net_of("SIG"), 200));
    }

    /// `SmoothDraggedSegments`: within a quarter of the track width (Highlight collisions, Walk around) or half of it (Shove) of the line of
    /// the parallel segment two along, the dragged segment lines up with it and the two merge. Off: nothing snaps. (Expected values:
    /// KiCad's own `dragSegment45`, see `line.rs`.)
    #[test]
    fn smooth_dragged_segments_lines_the_slid_segment_up_with_the_next_parallel_one() {
        let mut node = Node::new();
        let pts = [p(0, 0), p(1000, 0), p(2000, 1000), p(3000, 1000), p(4000, 2000)];
        let ids: Vec<ItemId> = pts.windows(2).enumerate().map(|(i, w)| node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: w[0], b: w[1], width: 250, source_track: Some(("t".into(), i)), locked: false }))).collect();
        let rules = rules();
        // the cursor is 70 um from the line of the last diagonal: inside the 125 of Shove (width / 2), outside the 62 of Walk around (width / 4)
        let slid = |settings: &RoutingSettings| {
            let mut d = Dragger::start(&node, p(1500, 500), ids[1]).unwrap();
            assert_eq!((d.kind, d.grabbed_index), (DragKind::Segment, 1));
            d.drag(&node, &rules, settings, p(2000, 100)).pts
        };
        let shove = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        assert_eq!(slid(&shove), vec![p(0, 0), p(2000, 0), p(4000, 2000)], "merged with the next diagonal");
        let walk = RoutingSettings { mode: Mode::Walkaround, ..RoutingSettings::default() };
        assert_eq!(slid(&walk), vec![p(0, 0), p(1900, 0), p(2900, 1000), p(3000, 1000), p(4000, 2000)], "62 um is not enough");
        let rough = RoutingSettings { smooth_dragged_segments: false, ..shove };
        assert_eq!(slid(&rough), vec![p(0, 0), p(1900, 0), p(2900, 1000), p(3000, 1000), p(4000, 2000)], "with smoothing off nothing snaps");
    }

    /// A 30-degree segment (imported geometry, not on KiCad's 45-degree grid) between a horizontal and a vertical leg is dragged at the
    /// 45-degree heading it quantizes to, and the legs follow: every leg of the result is on a 45-degree heading. (Expected values:
    /// KiCad's own `dragSegment45`, see `line.rs`.)
    #[test]
    fn a_30_degree_segment_is_dragged_at_45_degrees_with_its_neighbours() {
        let mut node = Node::new();
        let ids = add_track(&mut node, "SIG", "t", &[p(0, 0), p(2000, 0), p(3732, 1000), p(3732, 3000)]);
        let (rules, settings) = (rules(), RoutingSettings::default());
        let mut dragger = Dragger::start(&node, p(2866, 500), ids[1]).unwrap();
        assert_eq!((dragger.kind, dragger.grabbed_index), (DragKind::Segment, 1), "the middle of the 30-degree segment");
        let pv = dragger.drag(&node, &rules, &settings, p(3000, 1500));
        assert!(!pv.colliding);
        assert_eq!(pv.pts, vec![p(0, 0), p(1500, 0), p(3732, 2232), p(3732, 3000)]);
        assert!(on_45(&pv.pts), "{:?}", pv.pts);
        // the far ends did not move
        assert_eq!((pv.pts.first(), pv.pts.last()), (Some(&p(0, 0)), Some(&p(3732, 3000))));
    }

    /// A SIG track (0,0)-(1000,0) whose end is dragged, and a GND pad at (3000, 0) that the end cannot be dropped on.
    fn track_end_and_a_pad() -> (Node, Dragger) {
        use crate::item::Solid;
        let mut node = Node::new();
        let seg = node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: p(0, 0), b: p(1000, 0), width: 200, source_track: Some(("t".into(), 0)), locked: false }));
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: p(3000, 0), shape: Shape::Circle { c: p(3000, 0), r: 400 }, source: "U1.1".into(), edge: false }));
        let dragger = Dragger::start(&node, p(1000, 0), seg).unwrap();
        assert_eq!((dragger.kind, dragger.grabbed_index), (DragKind::Corner, 1));
        (node, dragger)
    }

    /// `Drag`: a position that cannot be resolved shows the last one that could, and a release over it commits that one
    /// (`FixRoute`: `Drag( m_lastValidPoint )`), not nothing -- in Walk around and in Shove.
    #[test]
    fn an_illegal_position_falls_back_to_the_last_valid_one() {
        for mode in [Mode::Walkaround, Mode::Shove] {
            let (node, mut dragger) = track_end_and_a_pad();
            let (rules, settings) = (rules(), RoutingSettings { mode, ..RoutingSettings::default() });
            let first = dragger.drag(&node, &rules, &settings, p(1000, 0));
            assert!(!first.colliding && !first.restored && !first.forced_mark_obstacles, "{mode:?}: the priming move at the grab point");
            let valid = dragger.drag(&node, &rules, &settings, p(2000, 1500));
            assert!(!valid.colliding && !valid.restored, "{mode:?}: {valid:?}");
            assert_eq!(valid.pts.last(), Some(&p(2000, 1500)));
            assert!(dragger.drag_status());
            // on the pad: the end has nowhere to go
            let on_the_pad = dragger.drag(&node, &rules, &settings, p(3000, 0));
            assert!(on_the_pad.restored, "{mode:?}: the preview is the last valid one, not the illegal position: {on_the_pad:?}");
            assert_eq!(on_the_pad.pts, valid.pts);
            assert!(!on_the_pad.colliding);
            assert!(!dragger.drag_status(), "{mode:?}: the cursor is not at a position that can be committed");
            assert!(!dragger.force_mark_obstacles());
            // letting go there commits the last valid point
            let commit = dragger.finish(&node, &rules, &settings, p(3000, 0), false).expect("a release over an illegal position commits the last valid one");
            assert_eq!(commit.tracks[0].pts, valid.pts, "{mode:?}");
            assert_eq!(commit.remove_track_ids, vec!["t"]);
            // a legal position again is not restored
            let again = dragger.drag(&node, &rules, &settings, p(2000, 1000));
            assert!(!again.restored && dragger.drag_status(), "{mode:?}");
        }
    }

    /// `m_forceMarkObstaclesMode`: when the very first position fails, the drag goes on marking obstacles, and only Ctrl+click commits
    /// what is in violation.
    #[test]
    fn when_the_first_position_fails_the_drag_only_marks_obstacles_and_ctrl_click_commits_it() {
        let (node, mut dragger) = track_end_and_a_pad();
        let (rules, settings) = (rules(), RoutingSettings { mode: Mode::Walkaround, ..RoutingSettings::default() });
        let pv = dragger.drag(&node, &rules, &settings, p(3000, 0));
        assert!(pv.forced_mark_obstacles && pv.colliding && !pv.restored, "{pv:?}");
        assert_eq!(pv.pts.last(), Some(&p(3000, 0)), "the end is where the cursor is, in violation");
        assert!(dragger.force_mark_obstacles() && !dragger.drag_status());
        // still marking: another position that collides is shown where the cursor is
        let again = dragger.drag(&node, &rules, &settings, p(3100, 100));
        assert!(again.colliding && again.forced_mark_obstacles && !again.restored);
        assert!(dragger.finish(&node, &rules, &settings, p(3100, 100), false).is_none(), "a plain click does not commit a violation");
        let forced = dragger.finish(&node, &rules, &settings, p(3100, 100), true).expect("Ctrl+click commits it anyway");
        assert_eq!(forced.tracks[0].pts, again.pts);
        // and a position that does not collide can be committed plainly
        let clear = dragger.drag(&node, &rules, &settings, p(2000, 1500));
        assert!(!clear.colliding && dragger.drag_status());
        assert!(dragger.finish(&node, &rules, &settings, p(2000, 1500), false).is_some());
    }

    /// In Highlight collisions mode every position "works": the collision is marked, a release over it is refused (no restore: the
    /// cursor is the last valid point there), and Ctrl+click is not needed once it clears.
    #[test]
    fn highlight_collisions_marks_and_refuses_a_colliding_release() {
        let (node, mut dragger) = track_end_and_a_pad();
        let (rules, settings) = (rules(), RoutingSettings { mode: Mode::MarkObstacles, ..RoutingSettings::default() });
        let pv = dragger.drag(&node, &rules, &settings, p(3000, 0));
        assert!(pv.colliding && !pv.restored && !pv.forced_mark_obstacles);
        assert_eq!(pv.pts.last(), Some(&p(3000, 0)));
        assert!(dragger.finish(&node, &rules, &settings, p(3000, 0), false).is_none());
        let allow = RoutingSettings { can_violate_drc: true, ..settings };
        assert!(dragger.finish(&node, &rules, &allow, p(3000, 0), false).is_some(), "Allow DRC violations commits it");
    }

    /// An IR track the dragged line stands on only in part keeps the rest: a branch cuts the track into two lines, the first is dragged.
    #[test]
    fn the_part_of_a_track_the_dragged_line_did_not_cover_goes_back() {
        let mut node = Node::new();
        let ids = add_track(&mut node, "SIG", "t", &[p(0, 0), p(2000, 0), p(2000, 2000), p(4000, 2000)]);
        // a second track of the same net leaves the first corner: the first segment is a line of its own
        add_track(&mut node, "SIG", "branch", &[p(2000, 0), p(2000, -2000)]);
        let (rules, settings) = (rules(), RoutingSettings::default());
        let mut dragger = Dragger::start(&node, p(1000, 0), ids[0]).unwrap();
        assert_eq!((dragger.kind, dragger.grabbed_index), (DragKind::Segment, 0));
        assert_eq!(dragger.original.pts, vec![p(0, 0), p(2000, 0)]);
        let commit = dragger.finish(&node, &rules, &settings, p(1000, 600), false).expect("clear");
        assert_eq!(commit.remove_track_ids, vec!["t"]);
        let mut lines: Vec<&Vec<Point>> = commit.tracks.iter().map(|l| &l.pts).collect();
        lines.sort();
        assert_eq!(lines.len(), 2, "the dragged line and the rest of t: {lines:?}");
        assert!(lines.contains(&&vec![p(2000, 0), p(2000, 2000), p(4000, 2000)]), "the rest of t comes back as it was: {lines:?}");
        let dragged = commit.tracks.iter().find(|l| l.pts.first() == Some(&p(0, 0))).expect("the dragged line");
        assert_eq!(dragged.pts.last(), Some(&p(2000, 0)), "its far end stays at the junction");
        assert!(on_45(&dragged.pts));
    }

    #[test]
    fn dragging_a_via_drags_its_connected_track_with_it() {
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: 0, y: 0 }, b: Point { x: 1000, y: 0 }, width: 200, source_track: Some(("trkA".into(), 0)), locked: false }));
        let via_id = node.add(Item::Via(Via { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: Point { x: 1000, y: 0 }, diameter: 600, drill: 300, source_via: Some("viaA".into()), locked: false }));
        let rules = rules();
        let settings = RoutingSettings::default();
        let mut dragger = Dragger::start(&node, Point { x: 1000, y: 0 }, via_id).expect("must start a via drag");
        assert_eq!(dragger.kind, DragKind::Via);
        let to = Point { x: 1000, y: 2000 };
        let preview = dragger.preview(&node, &rules, &settings, to);
        assert!(!preview.colliding);
        assert_eq!(preview.fanout.len(), 1, "the live preview must carry the attached track's stretched shape too, not just the via's own new point");
        // `DragCorner` re-solves the track's last leg at 45 degrees (`dragViaWalkaround`)
        let elbow = vec![Point { x: 0, y: 0 }, Point { x: 0, y: 1000 }, to];
        assert_eq!(preview.fanout[0].pts, elbow, "the fanout track must already end at the via's live (not yet committed) position");
        let commit = dragger.finish(&node, &rules, &settings, to, false).expect("collision-free via drag must commit");
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
        let (node, mut dragger) = via_across_a_gnd_track();
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
        let commit = dragger.finish(&node, &rules, &settings, to, false).expect("the shove commits");
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
        let rules = rules();
        let onto_the_pad = Point { x: 4000, y: 300 };
        for mode in [Mode::Walkaround, Mode::Shove] {
            let mut dragger = Dragger::start(&node, Point { x: 0, y: 0 }, via_id).unwrap();
            let settings = RoutingSettings { mode, ..RoutingSettings::default() };
            let preview = dragger.preview(&node, &rules, &settings, onto_the_pad);
            assert!(!preview.colliding, "{mode:?}: {preview:?}");
            let end = preview.pts[0];
            let centre_distance = (((end.x - 4000) as f64).powi(2) + (end.y as f64).powi(2)).sqrt();
            assert!(centre_distance >= 500.0 + 300.0 + 200.0 - 2.0, "{mode:?}: the via ended {centre_distance} um from the pad's centre, at {end:?}");
            assert_eq!(preview.fanout[0].last(), Some(end), "{mode:?}: the track ends at the via");
            let commit = dragger.finish(&node, &rules, &settings, onto_the_pad, false).expect("a via that clears commits");
            assert_eq!(commit.vias[0].0, end, "{mode:?}: the via is committed where it ended, not where the cursor was");
        }
        // Highlight collisions leaves it where the cursor put it, and says so.
        let dragger = Dragger::start(&node, Point { x: 0, y: 0 }, via_id).unwrap();
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
        let mut dragger = Dragger::start(&node, Point { x: 1000, y: 0 }, seg_id).unwrap();
        let settings = RoutingSettings::default();
        assert_eq!(settings.mode, Mode::Walkaround);
        let preview = dragger.preview(&node, &rules, &settings, Point { x: 5000, y: 0 });
        assert!(!preview.colliding, "the drag is walked around the pad: {preview:?}");
        assert!(preview.pts.len() > 2 && preview.pts.last() == Some(&Point { x: 5000, y: 0 }) && preview.pts.first() == Some(&Point { x: 0, y: 0 }), "{:?}", preview.pts);
        let commit = dragger.finish(&node, &rules, &settings, Point { x: 5000, y: 0 }, false).expect("the walked drag commits");
        assert_eq!(commit.tracks[0].pts, preview.pts);
        // Dropped on the pad itself, there is nowhere for the end to go: the pad's place is refused, and the last valid point -- the
        // walked drag above -- is what a release there commits
        let on_the_pad = dragger.preview(&node, &rules, &settings, Point { x: 3000, y: 0 });
        assert!(on_the_pad.colliding);
        let commit = dragger.finish(&node, &rules, &settings, Point { x: 3000, y: 0 }, false).expect("the last valid point");
        assert_eq!(commit.tracks[0].pts, preview.pts);
        // a drag whose first position is the pad has no valid point to fall back to
        let mut fresh = Dragger::start(&node, Point { x: 1000, y: 0 }, seg_id).unwrap();
        assert!(fresh.finish(&node, &rules, &settings, Point { x: 3000, y: 0 }, false).is_none());
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

    /// `dragShove` for a segment: the slid segment pushes the track in its way, and the board it leaves is clean.
    #[test]
    fn shove_mode_segment_drag_pushes_a_track_out_of_the_way() {
        let mut node = Node::new();
        let ids = add_track(&mut node, "SIG", "u", &[p(0, -3000), p(0, 0), p(6000, 0), p(6000, -3000)]);
        // a GND track below, across where the middle leg is dragged to
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: p(3000, 1000), b: p(3000, 6000), width: 200, source_track: Some(("gnd".into(), 0)), locked: false }));
        let rules = rules();
        let settings = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let mut dragger = Dragger::start(&node, p(4500, 0), ids[1]).unwrap();
        assert_eq!(dragger.kind, DragKind::Segment);
        let pv = dragger.drag(&node, &rules, &settings, p(4500, 2500));
        assert!(!pv.colliding && !pv.restored, "{pv:?}");
        assert_eq!(pv.displaced_lines.len(), 1, "the GND track is pushed: {:?}", pv.displaced_lines);
        assert_eq!(pv.displaced_lines[0].source_track.as_deref(), Some("gnd"));
        let commit = dragger.finish(&node, &rules, &settings, p(4500, 2500), false).expect("commits");
        assert_eq!(commit.remove_track_ids.len(), 2);
        // the board after: no collision between the dragged SIG line and the pushed GND, and the old place of the SIG leg is free
        let mut after = Node::new();
        for l in &commit.tracks {
            after.add_line(l, None, false);
        }
        for (id, item) in after.iter() {
            assert!(after.all_colliding(&item.shape(item.layers().start()), item.net(), item.layers(), &rules, &[id]).is_empty(), "{item:?} collides after the commit");
        }
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
        let mut dragger = Dragger::start(&node, Point { x: 1000, y: 0 }, seg_id).unwrap();
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
        let commit = dragger.finish(&node, &rules, &settings, Point { x: 6000, y: 0 }, false).expect("the walked drag is accepted");
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
        let mut free = Dragger::start_with(&node, Point { x: 1000, y: 0 }, seg_id, true).unwrap();
        assert!(free.free_angle);
        let preview = free.preview(&node, &rules, &settings, Point { x: 5000, y: 0 });
        assert!(preview.colliding, "a free-angle drag must report, not resolve, the collision");
        assert!(preview.displaced_lines.is_empty());
        // `FixRoute` refuses the colliding drop (outside MarkObstacles + can_violate_drc) ...
        assert!(free.finish(&node, &rules, &settings, Point { x: 5000, y: 0 }, false).is_none());
        // ... and commits a clear one, free angle included (the grabbed end simply follows the cursor).
        let commit = free.finish(&node, &rules, &settings, Point { x: 1000, y: 1000 }, false).expect("a clear free-angle drop commits");
        assert_eq!(commit.tracks[0].pts, vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 1000 }]);
        // a free-angle grab in the middle of a segment is a corner (the nearer vertex), not a slide
        let mut node = Node::new();
        let long = node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: 0, y: 0 }, b: Point { x: 4000, y: 0 }, width: 200, source_track: Some(("t".into(), 0)), locked: false }));
        let mid = Dragger::start_with(&node, Point { x: 1500, y: 0 }, long, true).unwrap();
        assert_eq!((mid.kind, mid.grabbed_index), (DragKind::Corner, 0));
    }

    #[test]
    fn starting_on_a_pad_refuses() {
        use crate::item::Solid;
        let mut node = Node::new();
        let pad_id = node.add(Item::Solid(Solid { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: Point { x: 0, y: 0 }, shape: Shape::Circle { c: Point { x: 0, y: 0 }, r: 400 }, source: "U1.1".into(), edge: false }));
        assert!(Dragger::start(&node, Point { x: 0, y: 0 }, pad_id).is_none());
    }
}
