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
//! ## Springback (Shove mode)
//!
//! `DRAGGER` makes one `SHOVE` for the whole gesture (`m_shove`, in `Start`), so the frames each successful shove pushed
//! (`SHOVE::m_nodeStack`, [`crate::springback`]) stay for the next position: a track or via the drag pushed stays pushed while the
//! head still collides with the frame that has it -- and is pushed on from where that frame left it, not from where it started -- a
//! frame the head no longer runs into anything of is popped, and the oldest frame of the gesture is never popped
//! (`while( m_nodeStack.size() > 1 )`; [`DragShove`]). A via drag records the via's place in each frame
//! (`SPRINGBACK_TAG::m_draggedVias`), and a frame popped puts the head via back to the one left on top. [`Self::preview`] runs the
//! same reduction on a copy of the stack, so it shows what [`Self::drag`] would without moving the gesture on. The other modes
//! re-derive their one world every position (`dragMarkObstacles`, `dragWalkaround`: `m_lastNode = m_world->Branch()`).
//!
//! Scoped down from upstream (see `PARITY.md`):
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
use crate::springback::{HeadSet, Springback, World};
use crate::walkaround::{path_collides, WalkPolicy, WalkStatus, Walker};
use eda_drc::kimath::Shape;
use eda_model::ir::{Point, Nm};
use eda_model::BoardRules;
use std::cell::RefCell;
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

/// `DRAGGER::checkVirtualVia( aP, aSeg )`: a segment taken hold of within half its width of one of its ends (`EuclideanNorm() <= w2`, the start
/// first), where the joint has a virtual via -- that via.
fn check_virtual_via(node: &Node, at: Point, seg: &crate::item::Segment) -> Option<ItemId> {
    // `( aP - aSeg->Seg().A ).EuclideanNorm() <= w2`: a double against the integer half width, not rounded
    let w2 = (seg.width / 2).as_f64();
    let norm = |p: Point| ((at.x - p.x).as_f64()).hypot((at.y - p.y).as_f64());
    let psnap = if norm(seg.a) <= w2 {
        seg.a
    } else if norm(seg.b) <= w2 {
        seg.b
    } else {
        return None;
    };
    let joint = node.joint_at(psnap, &seg.net)?;
    joint.links.iter().copied().find(|&id| node.get(id).is_some_and(Item::is_virtual))
}

pub struct Dragger {
    pub kind: DragKind,
    pub net: Net,
    pub layer: i32,
    pub width: Nm,
    /// The assembled line at drag start (`Corner`, `Segment`) -- its own segment ids
    /// are exactly what must be removed from the committed board on
    /// finish. Unused (empty) for `Via`.
    pub original: Line,
    /// `m_draggedSegmentIndex`: the vertex (`Corner`) or the segment (`Segment`) of `original` being dragged.
    pub grabbed_index: usize,
    /// `Via` only: the via's own identity/position/size at drag start.
    pub via_id: Option<ItemId>,
    pub via_pos: Point,
    pub via_diameter: Nm,
    pub via_drill: Nm,
    /// `Via` only: the layers the via spans.
    pub via_layers: LayerRange,
    pub source_via: Option<String>,
    /// `Via` only: the via is a virtual one (`VVIA`, [`crate::item::Via::is_virtual`]): the joint where tracks meet with no pad or via, taken hold
    /// of at the end of a segment (`DRAGGER::checkVirtualVia`). It collides with nothing and is not committed -- only the tracks it drags are.
    pub via_virtual: bool,
    /// `m_freeAngleMode` (`DM_FREE_ANGLE`, the `G` hotkey): see the module
    /// doc -- never shoves, only marks obstacles.
    pub free_angle: bool,
    /// `DragKind::Arc` only: what `DragArcTrack` works out before its loop.
    arc: Option<crate::arc_drag::ArcDrag>,
    /// Where the drag started (`m_lastValidPoint = aP` in `Start`).
    start_point: Point,
    /// What `Drag` remembers from one cursor position to the next.
    state: DragState,
    /// `m_shove` (`Start`: a `SHOVE` over `m_preDragNode` in Shove mode): the frames the gesture's successful shoves pushed. Other
    /// modes never push to it (`dragMarkObstacles`/`dragWalkaround` branch their world afresh every position).
    shove: RefCell<DragShove>,
}

/// `DRAGGER`'s one `SHOVE` for the gesture, as far as the drag is concerned: its springback stack (`m_nodeStack`) and, per frame, the
/// place the dragged via is left in it (`SPRINGBACK_TAG::m_draggedVias`: `reduceSpringback` puts the head via back to the one of the
/// frame the shove starts from). A line or arc drag has no via places.
#[derive(Debug, Clone, Default)]
struct DragShove {
    stack: Springback,
    via_frames: Vec<Point>,
}

impl DragShove {
    /// `SHOVE::reduceSpringback( aHeadSet )`: pop the frames, newest first, the head no longer collides with -- never the last one,
    /// never a locked one (there are none: a drag fixes nothing until it commits) -- and put the head via back to the place the frame
    /// left on top has for it.
    fn reduce(&mut self, base: &Node, head: &HeadSet, rules: &BoardRules) -> (World, Option<shove::Area>) {
        let top = self.stack.reduce(base, head, rules);
        self.via_frames.truncate(self.stack.len());
        top
    }

    /// `SHOVE::pushSpringback`: a frame for the world a successful line shove left.
    fn push(&mut self, base: &Node, world: World, area: Option<shove::Area>) {
        debug_assert!(self.via_frames.is_empty(), "a via drag's frames carry the via's place");
        self.stack.push(base, world, area);
    }

    /// `SHOVE::pushSpringback` of a via drag: the frame, with the via's place in it (`m_draggedVias[i]`, the via as the shove left it).
    fn push_via(&mut self, base: &Node, world: World, area: Option<shove::Area>, via: Point) {
        debug_assert_eq!(self.via_frames.len(), self.stack.len(), "one via place per frame");
        self.stack.push(base, world, area);
        self.via_frames.push(via);
    }

    /// `m_headLines[0].theVia` after `reduceSpringback`: the dragged via's place in the frame the next position shoves from, or
    /// `original` (where the drag started) before the first frame.
    fn via_pos(&self, original: Point) -> Point {
        self.via_frames.last().copied().unwrap_or(original)
    }
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
    /// `m_lastDragSolution`: the line a failed position puts back. `startDragSegment` sets it to the line as it was assembled
    /// (`m_lastDragSolution = m_draggedLine`, here `None`: the original), and only `dragShove` moves it on (`m_lastDragSolution =
    /// std::move( draggedPostShove )`, the shoved line before the optimizer: `optimizeAndUpdateDraggedLine` splits it at the anchor and
    /// optimizes a copy, `draggedPostOpt`). Walk around and Highlight collisions never set it.
    last_solution: Option<(Vec<Point>, Vec<crate::arcs::ArcPts>)>,
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
    /// The IR tracks each [`Self::fanout`] leg stands on, one entry per leg, where the leg was taken from a frame's node
    /// (`dragViaWalkaround` in the frame a shove left, or the frame a failed position restores): a line the frame re-made has item
    /// ids the session's world does not know, so the commit cannot look its tracks up there. Empty when the legs are the session's
    /// own lines (their `segment_ids` name the session's items).
    pub fanout_tracks: Vec<Vec<String>>,
    /// This is not what the cursor's position gave but the last position that worked: the cursor is somewhere the drag cannot
    /// be resolved (`Drag`: "Restore last solution").
    pub restored: bool,
    /// The drag is in Highlight collisions mode because its first position failed (`GetForceMarkObstaclesMode`): `colliding` then
    /// says whether releasing here is a violation, which only Ctrl+click commits.
    pub forced_mark_obstacles: bool,
    /// `dragShove` only: the line `m_lastDragSolution` takes from this position (the shoved line, split at the anchor when the optimizer
    /// runs, not optimized), what a later failed position shows again.
    pub(crate) solution: Option<(Vec<Point>, Vec<crate::arcs::ArcPts>)>,
}

/// What a finished drag replaces/adds -- `crate::router::Router` turns
/// this into the same `RouteCommit` shape a finished route produces, so
/// both go through the identical `Cmd::CommitRoute` seam.
#[derive(Debug, Clone, Default)]
pub struct DragCommit {
    pub remove_track_ids: Vec<String>,
    pub remove_via_ids: Vec<String>,
    pub tracks: Vec<Line>,
    pub vias: Vec<(Point, Nm, Nm, Option<String>)>,
}

/// What a drag may not take hold of (`DRAGGER::Start`, `ROUTER_TOOL::performDragging`).
pub const LOCKED_MESSAGE: &str = "The selected item is locked.";

/// `m_world->AssembleLine( aSeg, &m_draggedSegmentIndex )`: the defaults of `NODE::AssembleLine` -- the line stops in front of a locked segment (`aFollowLockedSegments`
/// false), so a locked track next to the dragged line stays where it is -- and goes on through a change of track width.
fn assemble(node: &Node, id: ItemId) -> Option<Line> {
    node.assemble_line_with(id, crate::node::AssembleOpts { follow_locked_segments: false, allow_width_mismatch: true, stop_at_locked_joints: false })
}

fn dist(a: Point, b: Point) -> f64 {
    let (dx, dy) = ((a.x - b.x).as_f64(), (a.y - b.y).as_f64());
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
                Ok(Dragger { kind: DragKind::Arc, net: line.net.clone(), layer: line.layer, width: line.width, original: line.clone(), grabbed_index: run.first, via_id: None, via_pos: Point { x: Nm::ZERO, y: Nm::ZERO }, via_diameter: Nm::ZERO, via_drill: Nm::ZERO, source_via: None, via_virtual: false, via_layers: LayerRange::single(line.layer), free_angle, arc: Some(drag), start_point: at, state: DragState::default(), shove: RefCell::new(DragShove::default()) })
            }
            _ => Self::start_plain(node, at, item_id, free_angle).ok_or_else(|| CANT.to_string()),
        }
    }

    fn start_plain(node: &Node, at: Point, item_id: ItemId, free_angle: bool) -> Option<Dragger> {
        let item = node.get(item_id)?;
        match item {
            Item::Via(v) => Some(Dragger { kind: DragKind::Via, net: v.net.clone(), layer: v.layers.start(), width: Nm::ZERO, original: Line::new(v.net.clone(), v.layers.start(), Nm::ZERO), grabbed_index: 0, via_id: Some(item_id), via_pos: v.pos, via_diameter: v.diameter, via_drill: v.drill, via_layers: v.layers, source_via: v.source_via.clone(), via_virtual: v.is_virtual, free_angle, arc: None, start_point: at, state: DragState::default(), shove: RefCell::new(DragShove::default()) }),
            Item::Segment(seg) => {
                // `DRAGGER::Start`: "VVIA* vvia = checkVirtualVia( aP, seg ); if( vvia ) return startDragVia( vvia );"
                if let Some(vvia) = check_virtual_via(node, at, seg) {
                    return Self::start_plain(node, at, vvia, free_angle);
                }
                // `startDragSegment`
                let (seg_a, seg_b, w2) = (seg.a, seg.b, seg.width / 2);
                let line = assemble(node, item_id)?;
                let layer_of_line = line.layer;
                // `m_world->AssembleLine( aSeg, &m_draggedSegmentIndex )`: which segment of the line the grabbed one is
                let mut index = line.segment_ids.iter().position(|&id| id == item_id)?.min(line.segment_count().saturating_sub(1));
                let (dist_a, dist_b) = (dist(at, seg_a), dist(at, seg_b));
                let kind;
                if dist_a < w2.as_f64() || dist_b < w2.as_f64() {
                    // within half its width of an end: that end is the corner to drag
                    kind = DragKind::Corner;
                    if dist_b <= dist_a {
                        index += 1;
                    }
                } else if free_angle {
                    // "( m_draggedSegmentIndex < PointCount() - 2 ) && ( !IsPtOnArc( m_draggedSegmentIndex + 1 ) )": the line's own last vertex,
                    // and the vertex an arc starts at, are not taken by a grab in the middle (findings p5 F3-11)
                    if dist_b < dist_a && (index as isize) < line.point_count() as isize - 2 && !line.is_pt_on_arc(index + 1) {
                        index += 1;
                    }
                    kind = DragKind::Corner;
                } else {
                    kind = DragKind::Segment;
                }
                Some(Dragger { kind, net: line.net.clone(), layer: line.layer, width: line.width, original: line, grabbed_index: index, via_id: None, via_pos: Point { x: Nm::ZERO, y: Nm::ZERO }, via_diameter: Nm::ZERO, via_drill: Nm::ZERO, via_layers: LayerRange::single(layer_of_line), source_via: None, via_virtual: false, free_angle, arc: None, start_point: at, state: DragState::default(), shove: RefCell::new(DragShove::default()) })
            }
            _ => None,
        }
    }

    /// The tracks attached to the dragged via at `via_pos` -- where the via is in `node`, which is not where the drag started once a
    /// frame has moved it -- as the node has them (`findViaFanoutByHandle`): one assembled line each.
    fn attached_lines(&self, node: &Node, via_pos: Point) -> Vec<Line> {
        let mut out: Vec<Line> = Vec::new();
        let Some(joint) = node.joint_at(via_pos, &self.net) else { return out };
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

    /// The candidate geometry for a via at `to`, before any shove/collision resolution (`DRAGGER::dragViaMarkObstacles`): every line of the
    /// via's fan-out with its end at the via dragged to `to`, re-solved in the drag's angle mode -- `draggedLine.DragCorner( aP,
    /// origLine.CLine().Find( aHandle.pos ), m_freeAngleMode )`: at 45 degrees for `D`, a straight leg only for `G` (the fan-out lines carry
    /// no snap threshold of their own).
    fn via_candidate(&self, node: &Node, via_pos: Point, to: Point) -> Vec<Line> {
        let mut fanout = self.attached_lines(node, via_pos);
        for l in fanout.iter_mut() {
            if let Some(at) = l.find(via_pos) {
                l.drag_corner(to, at, self.free_angle, Nm::ZERO);
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
    fn snap_threshold(&self, settings: &RoutingSettings, shove: bool) -> Nm {
        if !settings.smooth_dragged_segments {
            Nm::ZERO
        } else if shove {
            self.width / 2
        } else {
            self.width / 4
        }
    }

    /// The dragged line's points with the cursor at `to`: `LINE::DragSegment` for a segment drag, `LINE::DragCorner` for a corner drag
    /// (in `free_angle` mode `dragCornerFree`).
    fn dragged_line(&self, to: Point, snap: Nm, free_angle: bool) -> Line {
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
            let fanout = self.via_candidate(node, self.via_pos, to);
            let shape = Shape::Circle { c: to, r: self.via_diameter / 2 };
            // a virtual via collides with nothing: only its tracks can
            let mut colliding = !self.via_virtual && node.first_colliding(&shape, &self.net, self.via_layers, rules, &exclude).is_some();
            for l in &fanout {
                colliding |= l.segs().any(|(a, b)| {
                    let shape = Shape::Stadium { a, b, r: l.width.max(Nm::new(1)) / 2 };
                    node.first_colliding(&shape, &l.net, LayerRange::single(l.layer), rules, &exclude).is_some()
                });
            }
            return DragPreview { pts: vec![to], colliding, fanout, ..DragPreview::default() };
        }
        let line = self.dragged_line(to, self.snap_threshold(settings, false), self.free_angle);
        let layers = LayerRange::single(self.layer);
        let colliding = line.pts.windows(2).any(|w| {
            let shape = Shape::Stadium { a: w[0], b: w[1], r: self.width.max(Nm::new(1)) / 2 };
            node.first_colliding(&shape, &self.net, layers, rules, &exclude).is_some()
        });
        DragPreview { pts: line.pts, arcs: line.arcs, colliding, ..DragPreview::default() }
    }

    /// `DRAGGER::dragWalkaround`: the dragged line (or via and its tracks) is walked around what it would land on. `None` when it
    /// cannot be (`ok` false).
    fn solve_walkaround(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, to: Point) -> Option<DragPreview> {
        if self.kind == DragKind::Via {
            let pv = self.drag_via_walkaround(node, node, None, self.via_pos, rules, settings, to);
            return (!pv.colliding).then_some(pv);
        }
        let exclude = self.exclude();
        let dragged = self.dragged_line(to, self.snap_threshold(settings, false), false);
        let pts = dragged.pts.clone();
        let colliding = path_collides(node, rules, &self.net, self.layer, self.width.max(Nm::new(1)), &pts, &exclude);
        // an arc is not walked round anything: the arc drag either fits where the cursor puts it or is refused
        let walked = if !colliding {
            Some(pts)
        } else if self.kind == DragKind::Arc {
            None
        } else {
            try_walkaround(node, rules, settings, &self.net, self.layer, self.width.max(Nm::new(1)), &pts, &dragged.arcs)
        };
        let walked = walked.filter(|w| w.len() >= 2)?;
        // `optimizeAndUpdateDraggedLine( draggedWalk, origLine, aP )` (the lines of an arc drag are left as they are: DM_ARC is not a PNS drag)
        let walked_line = Line { pts: walked, arcs: dragged.arcs.clone(), ..dragged };
        let walked_line = if self.kind == DragKind::Arc { walked_line } else { self.optimize_and_update_dragged_line(node, rules, settings, &walked_line, &self.original, to) };
        Some(DragPreview { pts: walked_line.pts, arcs: walked_line.arcs, ..DragPreview::default() })
    }

    /// `DRAGGER::optimizeAndUpdateDraggedLine( aDragged, aOrig, aP )`: with "Optimize entire track being dragged" on, the dragged line is run through the
    /// optimizer -- `MERGE_SEGMENTS | KEEP_TOPOLOGY | RESTRICT_AREA`, and `MERGE_COLINEAR` with "Smooth dragged segments" -- restricted to the area the drag
    /// changed (`ChangedArea( aOrig )`, or the cursor's own point when nothing changed, which disables the optimization), with the vertex under the cursor (or
    /// the nearest point of the line to it, which is made a vertex: `Split( anchor )`) preserved. With it off, KiCad's default, the line is kept as it is.
    /// `world` is the world the optimizer checks against, without the line's old place.
    fn optimize_and_update_dragged_line(&self, world: &Node, rules: &BoardRules, settings: &RoutingSettings, dragged: &Line, orig: &Line, cursor: Point) -> Line {
        self.optimize_and_update_dragged_line_split(world, rules, settings, dragged, orig, cursor).0
    }

    /// [`Self::optimize_and_update_dragged_line`], and `aDragged` as the function leaves it: split at the anchor when the optimizer runs
    /// (`aDragged.Line().Split( anchor )`; `optimizer.Optimize( &aDragged, &draggedPostOpt, .. )` optimizes a copy), as it was otherwise.
    fn optimize_and_update_dragged_line_split(&self, world: &Node, rules: &BoardRules, settings: &RoutingSettings, dragged: &Line, orig: &Line, cursor: Point) -> (Line, Line) {
        use crate::optimizer::{effort, optimize_constrained, Constraints};
        if !settings.optimize_entire_dragged_track {
            return (dragged.clone(), dragged.clone());
        }
        let mut flags = effort::MERGE_SEGMENTS | effort::KEEP_TOPOLOGY | effort::RESTRICT_AREA;
        if settings.smooth_dragged_segments {
            flags |= effort::MERGE_COLINEAR;
        }
        let affected = shove::changed_area(dragged, orig).unwrap_or((cursor.x, cursor.y, cursor.x, cursor.y));
        let mut anchor = cursor;
        if dragged.find(cursor).is_none() {
            anchor = nearest_point_of(&dragged.pts, cursor);
        }
        let mut line = dragged.clone();
        line.clear_links();
        split_chain_at(&mut line.pts, anchor);
        let constraints = Constraints { area: Some(affected), preserve_vertex: Some(anchor), keep_topology: true, corner_mode: settings.corner_mode };
        (optimize_constrained(&line, world, rules, &self.exclude(), flags | effort::PRESERVE_VERTEX, &constraints), line)
    }

    /// `DRAGGER::dragShove`: whatever the dragged shape runs into is pushed aside, a pad walked around. `None` when it cannot be
    /// (`ok` false); a via that will not be shoved is walked around instead, as upstream does. `shove` is the gesture's `SHOVE`: the
    /// stack is reduced against this position's head first (`reduceSpringback`; the pops stay, even when the shove then fails -- `Run`
    /// leaves its `m_currentNode` at the reduced parent), and the frame a successful shove leaves is pushed (`pushSpringback`).
    fn solve_shove(&self, shove: &mut DragShove, node: &Node, rules: &BoardRules, settings: &RoutingSettings, to: Point) -> Option<DragPreview> {
        if self.kind == DragKind::Via {
            let pv = self.drag_via_shove(shove, node, rules, settings, to);
            return (!pv.colliding).then_some(pv);
        }
        let dragged = self.dragged_line(to, self.snap_threshold(settings, true), false);
        let width = self.width.max(Nm::new(1));
        // `m_headLines[0].origHead`: the head where the cursor puts it, which the stack is popped against
        let head = HeadSet { pts: &dragged.pts, net: &self.net, layer: self.layer, width, via: None };
        let (parent, area) = shove.reduce(node, &head, rules);
        // `m_currentNode = parent->Branch()`: the shove starts from the frame left on top
        let frame = parent.apply(node);
        // `m_mode == DM_CORNER && m_draggedSegmentIndex == 0`: the head is dragged by its first vertex, so its last point is the pusher
        let reversed = self.kind == DragKind::Corner && self.grabbed_index == 0;
        let outcome = shove::shove_dragged_line(&frame, &self.original.segment_ids, &dragged.pts, &dragged.arcs, &self.net, self.layer, width, rules, settings, reversed, area)?;
        // `m_lastNode = m_shove->CurrentNode()->Branch()`: what the next position starts from is this frame, its moves folded into
        // the one below it (`pushSpringback`)
        let mut world = parent;
        world.merge_shove(&outcome.displaced_lines, &outcome.displaced_vias);
        shove.push(node, world.clone(), outcome.area);
        // `optimizeAndUpdateDraggedLine( draggedPostShove, m_draggedLine, aP )`, against the world the shove left
        let shoved = Line { pts: outcome.head.clone(), arcs: dragged.arcs.clone(), ..dragged };
        let (shoved, solution) = if self.kind == DragKind::Arc { (shoved.clone(), shoved) } else { self.optimize_and_update_dragged_line_split(&outcome.world, rules, settings, &shoved, &self.original, to) };
        // the preview shows the frame's whole world: every track and via the gesture has moved so far, not just this position's
        Some(DragPreview { pts: shoved.pts, arcs: shoved.arcs, displaced_lines: world.displaced_lines(node), displaced_vias: world.displaced_vias(), solution: Some((solution.pts, solution.arcs)), ..DragPreview::default() })
    }

    /// One cursor position: what `dragMarkObstacles` / `dragWalkaround` / `dragShove` make of it, and whether it `ok`s -- their return
    /// value, true for every position when marking obstacles.
    fn solve(&self, shove: &mut DragShove, node: &Node, rules: &BoardRules, settings: &RoutingSettings, to: Point, mark_only: bool) -> (DragPreview, bool) {
        // `DRAGGER::Drag`: `if( m_freeAngleMode || m_forceMarkObstaclesMode ) dragMarkObstacles`, else by `m_currentMode`
        if mark_only || self.free_angle || settings.mode == Mode::MarkObstacles {
            return (self.solve_mark_obstacles(node, rules, settings, to), true);
        }
        let resolved = match settings.mode {
            Mode::Walkaround => self.solve_walkaround(node, rules, settings, to),
            _ => self.solve_shove(shove, node, rules, settings, to),
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
    /// failed yet. Pure -- never mutates `node` or `self`; [`Self::drag`] is the one a drag session uses. The gesture's springback
    /// stack is reduced on a copy (`m_nodeStack` in KiCad lives as long as the dragger and a preview does not move it on), so the
    /// preview shows the frames the drag's earlier positions pushed.
    pub fn preview(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, to: Point) -> DragPreview {
        let mut shove = self.shove.borrow().clone();
        let mut pv = self.solve(&mut shove, node, rules, settings, to, self.state.force_mark_obstacles).0;
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
        // the gesture's `SHOVE` moves on with this position (`dragShove`'s `m_shove->Run()`): frames popped and pushed here are the
        // ones the next position starts from
        let (mut pv, ok) = {
            let mut shove = self.shove.borrow_mut();
            self.solve(&mut shove, node, rules, settings, to, mark)
        };
        if ok {
            self.state.last_valid = Some(to);
            self.state.drag_status = self.status_of(&pv, ok, mark, settings);
            pv.forced_mark_obstacles = self.state.force_mark_obstacles;
            if let Some(solution) = pv.solution.take() {
                self.state.last_solution = Some(solution);
            }
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
        // "Restore last solution": m_dragStatus stays false; the view is the parent of the last node (the session's world in Walk around
        // mode, the shove's reduced frame in Shove mode) with `m_lastDragSolution` added, and nothing dragged (`m_draggedItems.Clear()`)
        self.state.drag_status = false;
        let (pts, arcs) = self.state.last_solution.clone().unwrap_or_else(|| (self.original.pts.clone(), self.original.arcs.clone()));
        let mut restored = DragPreview { pts, arcs, ..DragPreview::default() };
        if !mark && settings.mode == Mode::Shove {
            self.restore_onto_frame(node, &mut restored);
        } else if self.kind == DragKind::Via {
            // a via drag has no `m_lastDragSolution` of its own: the via and its tracks are where the world has them
            restored.pts = vec![self.via_pos];
            restored.fanout = self.attached_lines(node, self.via_pos);
        }
        restored.restored = true;
        restored.forced_mark_obstacles = self.state.force_mark_obstacles;
        restored
    }

    /// `DRAGGER::Drag`'s "Restore last solution" in Shove mode: `NODE* parent = m_lastNode->GetParent()->Branch(); ..
    /// m_lastNode->Add( m_lastDragSolution )` -- what is shown is the frame this failed position reduced the stack to (`SHOVE::Run`
    /// leaves `m_currentNode` at the reduced parent when it fails), not the frame the last good position left: a frame this position
    /// popped has sprung back. A line drag shows the last good line in it. A via drag has no `m_lastDragSolution` of its own (`dragShove`
    /// sets it only for `DM_SEGMENT` / `DM_CORNER`), so the via and its tracks are where that frame has them. (In Walk around mode the
    /// parent is the session's world, and `m_lastDragSolution` is still the line `startDragSegment` assembled, since `dragWalkaround`
    /// never sets it: the line is shown where it started, [`Self::drag`].)
    fn restore_onto_frame(&self, node: &Node, pv: &mut DragPreview) {
        let shove = self.shove.borrow();
        let (top, _) = shove.stack.top();
        if self.kind == DragKind::Via {
            let at = shove.via_pos(self.via_pos);
            let frame = top.apply(node);
            let legs = self.attached_lines(&frame, at);
            let tracks: Vec<Vec<String>> = legs.iter().map(|l| Self::source_tracks(&frame, l)).collect();
            let (lines, vias) = self.frame_displacement(node, &top, &legs, &tracks);
            pv.pts = vec![at];
            pv.fanout = legs;
            pv.fanout_tracks = tracks;
            pv.displaced_lines = lines;
            pv.displaced_vias = vias;
        } else {
            pv.displaced_lines = top.displaced_lines(node);
            pv.displaced_vias = top.displaced_vias();
        }
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
    /// moved to the cursor from where the frame left on top has it, the tracks attached to it go with it, and what they and the via
    /// collide with is pushed aside. The via ends where the shove left it, which is not the cursor if a pad or the board edge pushed
    /// it on. When the shove cannot be done -- `ShoveVias()` is off, the via is locked, or something will not give -- the drag walks
    /// around instead, in the frame the stack was reduced to (`m_lastNode = m_shove->CurrentNode()->Branch()`).
    fn drag_via_shove(&self, shove: &mut DragShove, node: &Node, rules: &BoardRules, settings: &RoutingSettings, to: Point) -> DragPreview {
        // `reduceSpringback`'s head set is the via as the current node has it (`FindViaByHandle( theVia )`), not where the cursor is
        let from = shove.via_pos(self.via_pos);
        let head_via = crate::item::Via { net: self.net.clone(), layers: self.via_layers, pos: from, diameter: self.via_diameter, drill: self.via_drill, source_via: self.source_via.clone(), locked: false, is_virtual: self.via_virtual };
        let head = HeadSet { pts: &[], net: &self.net, layer: self.layer, width: self.via_diameter, via: Some(&head_via) };
        let (parent, area) = shove.reduce(node, &head, rules);
        // the via may have sprung back to a frame below's place (`reduceSpringback` restores the handle)
        let from = shove.via_pos(self.via_pos);
        // `m_currentNode = parent->Branch()`: the frame the shove starts from
        let mut frame = parent.apply(node);
        let Some(id) = self.place_via(&mut frame, from) else {
            return self.drag_via_walkaround(node, &frame, Some(&parent), from, rules, settings, to);
        };
        let Some(outcome) = shove::shove_via(&frame, id, to, rules, settings, area) else {
            return self.drag_via_walkaround(node, &frame, Some(&parent), from, rules, settings, to);
        };
        let end = outcome.head.first().copied().unwrap_or(to);
        // `m_lastNode = m_shove->CurrentNode()->Branch()`: the frame this shove leaves, with the via's place in it
        let mut world = parent;
        world.merge_shove(&outcome.displaced_lines, &outcome.displaced_vias);
        shove.push_via(node, world.clone(), outcome.area, end);
        let mut displaced_lines = world.displaced_lines(node);
        // The attached tracks, as the drag left them: the displaced line that now ends at the via, in place of the original.
        let mut fanout = Vec::new();
        for original in self.attached_lines(node, self.via_pos) {
            let sources = Self::source_tracks(node, &original);
            let attached = |d: &DisplacedLine| d.line.point_count() >= 2 && d.source_track.as_ref().is_some_and(|t| sources.contains(t)) && (d.line.first() == Some(end) || d.line.last() == Some(end));
            let found = displaced_lines.iter().position(attached);
            let pts = match found {
                Some(i) => displaced_lines.remove(i).line.pts,
                // the frame has the leg where it started (no frame has moved the via yet, or the shove left it there)
                None if original.first() == Some(end) || original.last() == Some(end) => original.pts.clone(),
                // A track the drag shortened to nothing has no line left; it is removed when the drag is committed.
                None => vec![end],
            };
            fanout.push(Line { pts, ..original });
        }
        let displaced_vias = world.displaced_vias().into_iter().filter(|v| Some(&v.source_via) != self.source_via.as_ref()).collect();
        DragPreview { pts: vec![end], colliding: false, displaced_lines, displaced_vias, fanout, ..DragPreview::default() }
    }

    /// The dragged via in a frame's node, at `at`: a via the frame's world moved is already there (the world carries it by its
    /// `source_via` name); a via the world cannot carry (`SHOVE` does not record a virtual one) is the session's via moved here.
    fn place_via(&self, frame: &mut Node, at: Point) -> Option<ItemId> {
        if let Some(id) = self.via_id {
            if let Some(Item::Via(v)) = frame.get(id) {
                if v.pos == at {
                    return Some(id);
                }
            }
        }
        if let Some(name) = &self.source_via {
            if let Some(id) = frame.iter().find_map(|(id, it)| match it {
                Item::Via(v) if v.source_via.as_ref() == Some(name) && v.pos == at => Some(id),
                _ => None,
            }) {
                return Some(id);
            }
        }
        let id = self.via_id?;
        let Item::Via(v) = frame.get(id)?.clone() else { return None };
        frame.remove(id);
        Some(frame.add(Item::Via(crate::item::Via { pos: at, ..v })))
    }

    /// `DRAGGER::dragViaWalkaround( aHandle, aNode, aP )`: the via is put at the cursor and pushed off whatever it would sit on
    /// (`propagateViaForces`: `VIA::PushoutForce`, at most `ViaForcePropIterationLimit` steps, with the way back as the lead), then
    /// every track attached to it -- the frame's, at `via_pos` -- is dragged to the via's new place -- at 45 degrees (`DragCorner`)
    /// -- and walked around what it would hit ([`try_walkaround`]). A via that cannot be pushed free, or a track that cannot be
    /// walked, is a collision. `world` is the session's world, which says whether a track would collide at all
    /// (`m_world->CheckColliding( &draggedLine )`); `frame` is the node the pushout and the walkaround work in (`m_lastNode`), the
    /// session's world unless a shove left a frame.
    ///
    /// `moved` is that frame's world (`dragShove`'s fallback: `dragViaWalkaround( m_draggedVia, m_lastNode, aP )`, `m_lastNode` a branch
    /// of the frame the stack was reduced to): KiCad edits the branch -- `m_lastNode->Remove( origLine ); m_lastNode->Add( draggedLine )`
    /// -- so what the earlier positions pushed stays, in the preview and in the commit, beside the walked fan-out. The preview carries
    /// the frame's displacement without the via's own tracks (the fan-out replaces them) and without the dragged via, and the IR
    /// tracks each leg stands on as the frame's items name them ([`DragPreview::fanout_tracks`]).
    #[allow(clippy::too_many_arguments)]
    fn drag_via_walkaround(&self, world: &Node, frame: &Node, moved: Option<&World>, via_pos: Point, rules: &BoardRules, settings: &RoutingSettings, to: Point) -> DragPreview {
        let exclude = self.exclude();
        let via = crate::item::Via { net: self.net.clone(), layers: self.via_layers, pos: to, diameter: self.via_diameter, drill: self.via_drill, source_via: self.source_via.clone(), locked: false, is_virtual: false };
        let lead = (via_pos.x - to.x, via_pos.y - to.y);
        let Some(force) = shove::via_pushout_force(frame, rules, &via, lead, crate::node::kind_mask::ANY, settings.via_force_prop_iteration_limit, &exclude) else {
            // can't force-propagate the via? bummer...
            let fanout = self.via_candidate(frame, via_pos, to);
            return DragPreview { pts: vec![to], colliding: true, fanout, ..DragPreview::default() };
        };
        let target = Point { x: to.x + force.0, y: to.y + force.1 };
        let mut colliding = false;
        let mut fanout = Vec::new();
        let legs = self.attached_lines(frame, via_pos);
        let fanout_tracks: Vec<Vec<String>> = legs.iter().map(|l| Self::source_tracks(frame, l)).collect();
        let (displaced_lines, displaced_vias) = match moved {
            Some(w) => self.frame_displacement(world, w, &legs, &fanout_tracks),
            None => (Vec::new(), Vec::new()),
        };
        for original in legs {
            let mut dragged = original.clone();
            if let Some(at) = dragged.find(via_pos) {
                dragged.drag_corner45(target, at);
                dragged.simplify();
            }
            if dragged.point_count() >= 2 && path_collides(world, rules, &dragged.net, dragged.layer, dragged.width.max(Nm::new(1)), &dragged.pts, &exclude) {
                match try_walkaround(frame, rules, settings, &dragged.net, dragged.layer, dragged.width.max(Nm::new(1)), &dragged.pts, &dragged.arcs) {
                    Some(walked) => {
                        dragged.pts = walked;
                        // `optimizeAndUpdateDraggedLine( walkLine, origLine, aP )`
                        dragged = self.optimize_and_update_dragged_line(frame, rules, settings, &dragged, &original, to);
                    }
                    None => colliding = true,
                }
            }
            fanout.push(dragged);
        }
        DragPreview { pts: vec![target], colliding, displaced_lines, displaced_vias, fanout, fanout_tracks, ..DragPreview::default() }
    }

    /// What a frame's world `moved` has displaced, as a via drag's preview shows it beside the via's own tracks: the lines of
    /// `legs` (the via's tracks as the frame has them, standing on `leg_tracks`) are left out, the fan-out takes their place, and so
    /// is the dragged via. A line of a leg's IR track that is not part of the leg (the track runs on past a junction) stays.
    fn frame_displacement(&self, base: &Node, moved: &World, legs: &[Line], leg_tracks: &[Vec<String>]) -> (Vec<DisplacedLine>, Vec<DisplacedVia>) {
        let is_leg = |d: &DisplacedLine| {
            legs.iter().zip(leg_tracks).any(|(l, ts)| d.source_track.as_ref().is_some_and(|t| ts.contains(t)) && d.line.point_count() >= 2 && d.line.pts.iter().all(|p| l.pts.contains(p)))
        };
        let lines = moved.displaced_lines(base).into_iter().filter(|d| !is_leg(d)).collect();
        let vias = moved.displaced_vias().into_iter().filter(|v| Some(&v.source_via) != self.source_via.as_ref()).collect();
        (lines, vias)
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
                // where the drag left the via: the cursor, or further, if a shove or a pushout moved it on -- a virtual via is not committed
                // (`ROUTER::CommitRouting`: `if( !item->IsVirtual() ) m_iface->AddItem( item )`)
                let end = preview.pts.first().copied().unwrap_or(self.via_pos);
                if !self.via_virtual {
                    commit.vias.push((end, self.via_diameter, self.via_drill, self.source_via.clone()));
                }
                // a leg taken from a frame names its tracks itself (its items are the frame's, not the session's)
                for (i, l) in preview.fanout.iter().enumerate() {
                    let sources = if preview.fanout_tracks.len() == preview.fanout.len() { preview.fanout_tracks[i].clone() } else { Self::source_tracks(node, l) };
                    for src in sources {
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
            commit.vias.push((d.pos, Nm::ZERO, Nm::ZERO, Some(d.source_via.clone())));
        }
        commit
    }
}

/// `DRAGGER::tryWalkaround`: walk `pts` around what it runs into (`WP_SHORTEST`, the walkaround iteration limit, a detour of up
/// to 30 times the line's length), or `None` if it cannot be done.
/// `SHAPE_LINE_CHAIN::NearestPoint( aP )`: the point of the polyline nearest `p` (a vertex or the foot of the perpendicular on a segment).
fn nearest_point_of(pts: &[Point], p: Point) -> Point {
    let mut best: Option<(i128, Point)> = None;
    for w in pts.windows(2) {
        let seg = eda_drc::kimath::Seg::new(w[0], w[1]);
        let q = seg.nearest_point(p);
        let d = ((q.x - p.x).get() as i128).pow(2) + ((q.y - p.y).get() as i128).pow(2);
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, q));
        }
    }
    best.map(|(_, q)| q).or_else(|| pts.first().copied()).unwrap_or(p)
}

/// `SHAPE_LINE_CHAIN::Split( aP )`: `p`, which lies on the chain, becomes a vertex of it (when it is not one already).
fn split_chain_at(pts: &mut Vec<Point>, p: Point) {
    if pts.contains(&p) {
        return;
    }
    if let Some(i) = (0..pts.len().saturating_sub(1)).find(|&i| eda_drc::kimath::Seg::new(pts[i], pts[i + 1]).sq_distance_to_point(p) <= 1) {
        pts.insert(i + 1, p);
    }
}

#[allow(clippy::too_many_arguments)] // the query context, the line's net, layer and width, its points and its arcs
fn try_walkaround(node: &Node, rules: &BoardRules, settings: &RoutingSettings, net: &Net, layer: i32, width: Nm, pts: &[Point], arcs: &[crate::arcs::ArcPts]) -> Option<Vec<Point>> {
    let mut walker = Walker::new(node, rules);
    walker.corner_mode = settings.corner_mode;
    walker.arcs = arcs.to_vec();
    walker.iteration_limit = settings.walkaround_iteration_limit.max(0) as u32;
    walker.length_limit_on = true;
    walker.length_expansion_factor = 30.0;
    walker.set_allowed_policies(&[WalkPolicy::Shortest]);
    let outcome = walker.route(net, layer, width, pts);
    (outcome.status_of(WalkPolicy::Shortest) == WalkStatus::Done).then(|| outcome.line_of(WalkPolicy::Shortest).to_vec())
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;
    use crate::item::{net_of, Segment, Via};

    fn rules() -> BoardRules {
        serde_yaml::from_str("track_width: 200000\nclearance: 200000\nvia_drill: 300000\nvia_diameter: 600000\n").unwrap()
    }

    fn p(x: i64, y: i64) -> Point {
        Point { x: Nm::new(x), y: Nm::new(y) }
    }

    fn on_45(pts: &[Point]) -> bool {
        pts.windows(2).all(|w| {
            let (dx, dy) = ((w[1].x - w[0].x).abs(), (w[1].y - w[0].y).abs());
            dx == Nm::ZERO || dy == Nm::ZERO || dx == dy
        })
    }

    /// A SIG track of `pts` on layer 0 (width 200) in the node, one `Segment` per leg, all of IR track `track`.
    fn add_track(node: &mut Node, net: &str, track: &str, pts: &[Point]) -> Vec<ItemId> {
        pts.windows(2).enumerate().map(|(i, w)| node.add(Item::Segment(Segment { net: net_of(net), layer: 0, a: w[0], b: w[1], width: mm(0.2), source_track: Some((track.into(), i)), locked: false }))).collect()
    }

    /// `NODE::FixupVirtualVias` + `DRAGGER::checkVirtualVia`: where tracks of different widths meet with no pad or via, the world has a virtual via,
    /// and a segment taken hold of within half its width of that end drags the junction as a via -- all three tracks of this T follow, in every
    /// mode, and the commit moves the three and adds no via. Taken hold of further along, it is a corner or segment drag of that track as before.
    /// A T of one width has no virtual via (KiCad's `n_seg` is never counted), and its end is a corner drag of the one line.
    #[test]
    fn a_t_junction_is_dragged_as_a_virtual_via() {
        let mut node = Node::new();
        let a = add_track(&mut node, "SIG", "trkA", &[p(-3_000_000, 0), p(0, 0)]);
        add_track(&mut node, "SIG", "trkB", &[p(0, 0), p(3_000_000, 0)]);
        let one_width = {
            let mut n = node.branch();
            add_track(&mut n, "SIG", "trkC", &[p(0, 0), p(0, 3_000_000)]);
            n.fixup_virtual_vias();
            n
        };
        assert!(one_width.joint_at(p(0, 0), &net_of("SIG")).unwrap().links.iter().all(|&id| !one_width.get(id).is_some_and(Item::is_virtual)), "a T of one width: none");
        assert_eq!(Dragger::start(&one_width, p(-50_000, 0), a[0]).expect("a drag").kind, DragKind::Corner, "and its end is a corner drag");
        node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: p(0, 0), b: p(0, 3_000_000), width: mm(0.3), source_track: Some(("trkC".into(), 0)), locked: false }));
        node.fixup_virtual_vias();
        let joint = node.joint_at(p(0, 0), &net_of("SIG")).unwrap();
        let vvia = joint.links.iter().copied().find(|&id| node.get(id).is_some_and(Item::is_virtual)).expect("a virtual via at the T");
        assert!(node.iter().all(|(id, _)| id != vvia), "a virtual via is not one of the items the world lists");
        assert!(node.item_at(p(0, 0), LayerRange::single(0), mm(0.1)).is_some_and(|id| id != vvia), "and not one it picks");
        let rules = rules();
        for mode in [Mode::MarkObstacles, Mode::Walkaround, Mode::Shove] {
            let settings = RoutingSettings { mode, ..RoutingSettings::default() };
            let mut d = Dragger::start(&node, p(-50_000, 0), a[0]).expect("a drag");
            assert_eq!((d.kind, d.via_virtual, d.via_id), (DragKind::Via, true, Some(vvia)), "{mode:?}: the end of the segment at the T is the virtual via");
            let pv = d.drag(&node, &rules, &settings, p(500_000, 500_000));
            assert!(!pv.colliding, "{mode:?}");
            assert_eq!(pv.fanout.len(), 3, "{mode:?}: all three tracks follow: {:?}", pv.fanout);
            assert!(pv.fanout.iter().all(|l| l.first() == Some(p(500_000, 500_000)) || l.last() == Some(p(500_000, 500_000))), "{mode:?}: {:?}", pv.fanout);
            let commit = d.finish(&node, &rules, &settings, p(500_000, 500_000), false).expect("commits");
            assert!(commit.vias.is_empty() && commit.remove_via_ids.is_empty(), "{mode:?}: no via is committed: {:?}", commit.vias);
            let mut removed = commit.remove_track_ids.clone();
            removed.sort();
            assert_eq!(removed, vec!["trkA", "trkB", "trkC"], "{mode:?}");
        }
        // further along the segment it is the segment's own drag
        let d = Dragger::start(&node, p(-1_500_000, 0), a[0]).expect("a drag");
        assert_eq!(d.kind, DragKind::Segment);
        // a plain corner of one track (two segments) has no virtual via
        let mut plain = Node::new();
        let b = add_track(&mut plain, "SIG", "trkD", &[p(0, 0), p(2_000_000, 0), p(2_000_000, 2_000_000)]);
        plain.fixup_virtual_vias();
        assert_eq!(Dragger::start(&plain, p(2_000_000, 50_000), b[0]).unwrap().kind, DragKind::Corner);
    }

    /// `FixupVirtualVias`: a change of width and the two ends of a locked segment are joints with a virtual via too, a pad or a via already there is not.
    #[test]
    fn virtual_vias_go_where_the_width_changes_and_at_the_ends_of_a_locked_segment() {
        let count = |n: &Node, at: Point| n.joint_at(at, &net_of("SIG")).map_or(0, |j| j.links.iter().filter(|&&id| n.get(id).is_some_and(Item::is_virtual)).count());
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: p(0, 0), b: p(1_000_000, 0), width: mm(0.2), source_track: None, locked: false }));
        node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: p(1_000_000, 0), b: p(2_000_000, 0), width: mm(0.4), source_track: None, locked: false }));
        node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: p(2_000_000, 0), b: p(3_000_000, 0), width: mm(0.4), source_track: None, locked: true }));
        node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: p(3_000_000, 0), b: p(4_000_000, 0), width: mm(0.4), source_track: None, locked: false }));
        node.add(Item::Via(Via { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: p(4_000_000, 0), diameter: mm(0.6), drill: mm(0.3), source_via: None, locked: false, is_virtual: false }));
        node.fixup_virtual_vias();
        assert_eq!(count(&node, p(1_000_000, 0)), 1, "where the width changes");
        assert_eq!(count(&node, p(0, 0)), 0, "a free end");
        // the joint at (3000, 0): the locked segment is the last one there, so it is "the locked one" and both its ends get one
        assert!(count(&node, p(3_000_000, 0)) >= 1 && count(&node, p(2_000_000, 0)) >= 1, "the ends of the locked segment");
        assert_eq!(count(&node, p(4_000_000, 0)), 0, "a real via is there already");
        let d = node.joint_at(p(1_000_000, 0), &net_of("SIG")).unwrap().links.iter().find_map(|&id| match node.get(id) { Some(Item::Via(v)) if v.is_virtual => Some((v.diameter, v.layers)), _ => None });
        assert_eq!(d, Some((mm(0.4) + 2 * crate::hull::PNS_HULL_MARGIN, LayerRange::single(0))), "the widest track plus twice `PNS_HULL_MARGIN`, on its one layer");
    }

    #[test]
    fn drags_a_track_end_and_keeps_the_other_end_attached() {
        let mut node = Node::new();
        let seg_id = node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: Nm::ZERO, y: Nm::ZERO }, b: Point { x: mm(1.0), y: Nm::ZERO }, width: mm(0.2), source_track: Some(("trkA".into(), 0)), locked: false }));
        let rules = rules();
        let settings = RoutingSettings::default();
        let mut dragger = Dragger::start(&node, Point { x: mm(1.0), y: Nm::ZERO }, seg_id).expect("must start a corner drag");
        assert_eq!(dragger.kind, DragKind::Corner);
        let preview = dragger.preview(&node, &rules, &settings, Point { x: mm(1.0), y: mm(1.0) });
        assert!(!preview.colliding);
        assert_eq!(preview.pts.first(), Some(&Point { x: Nm::ZERO, y: Nm::ZERO }), "the ungrabbed end must stay put");
        assert_eq!(preview.pts.last(), Some(&Point { x: mm(1.0), y: mm(1.0) }), "the grabbed end follows the cursor");
        let commit = dragger.finish(&node, &rules, &settings, Point { x: mm(1.0), y: mm(1.0) }, false).expect("collision-free drag must commit");
        assert_eq!(commit.remove_track_ids, vec!["trkA"]);
        assert_eq!(commit.tracks.len(), 1);
        assert_eq!(commit.tracks[0].pts, vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: mm(1.0) }]);
    }

    /// `startDragSegment`: within half the width of an end it is that corner, anywhere else the segment; `G` makes a middle grab a corner.
    #[test]
    fn grabbing_near_an_end_drags_the_corner_and_anywhere_else_the_segment() {
        let mut node = Node::new();
        let ids = add_track(&mut node, "SIG", "t", &[p(0, 0), p(10_000_000, 0), p(10_000_000, 5_000_000)]);
        let at = |x, y, free| Dragger::start_with(&node, p(x, y), ids[0], free).unwrap();
        let near_a = at(50_000, 30_000, false);
        assert_eq!((near_a.kind, near_a.grabbed_index), (DragKind::Corner, 0), "50 um from the first end: its vertex");
        let near_b = at(9_950_000, 0, false);
        assert_eq!((near_b.kind, near_b.grabbed_index), (DragKind::Corner, 1), "within half the width of the second end: the vertex after it");
        let middle = at(5_000_000, 0, false);
        assert_eq!((middle.kind, middle.grabbed_index), (DragKind::Segment, 0), "the middle of the segment slides it");
        let past_the_pad_of_the_end = at(9_850_000, 0, false);
        assert_eq!(past_the_pad_of_the_end.kind, DragKind::Segment, "150 um from an end of a 200 um track is the track, not the corner");
        // free angle: the nearer vertex, but the line's own last vertex is not taken by a grab in the middle of its last segment
        let free_b = at(7_000_000, 0, true);
        assert_eq!((free_b.kind, free_b.grabbed_index), (DragKind::Corner, 1));
        let last = Dragger::start_with(&node, p(10_000_000, 3_000_000), ids[1], true).unwrap();
        assert_eq!((last.kind, last.grabbed_index), (DragKind::Corner, 1), "nearer the end, but vertex 2 is the line's last: it stays with the segment's start");
    }

    /// `DM_SEGMENT`: the diagonal between two straight legs slides, the legs stretching to follow; one IR track in, one out.
    #[test]
    fn a_segment_drag_slides_the_segment_and_its_neighbours_follow_at_45_degrees() {
        let mut node = Node::new();
        let ids = add_track(&mut node, "SIG", "t", &[p(0, 0), p(2_000_000, 0), p(3_000_000, 1_000_000), p(5_000_000, 1_000_000)]);
        let (rules, settings) = (rules(), RoutingSettings::default());
        let mut dragger = Dragger::start(&node, p(2_500_000, 500_000), ids[1]).unwrap();
        assert_eq!((dragger.kind, dragger.grabbed_index), (DragKind::Segment, 1));
        let pv = dragger.drag(&node, &rules, &settings, p(2_500_000, 1_500_000));
        assert!(!pv.colliding && !pv.restored);
        // the diagonal now goes through (2500, 1500): x - y = 1000
        assert_eq!(pv.pts, vec![p(0, 0), p(1_000_000, 0), p(2_000_000, 1_000_000), p(5_000_000, 1_000_000)]);
        let commit = dragger.finish(&node, &rules, &settings, p(2_500_000, 1_500_000), false).expect("a clear slide commits");
        assert_eq!(commit.remove_track_ids, vec!["t"]);
        assert_eq!(commit.tracks.len(), 1);
        assert_eq!(commit.tracks[0].pts, pv.pts);
        assert_eq!((commit.tracks[0].net.clone(), commit.tracks[0].width), (net_of("SIG"), mm(0.2)));
    }

    /// `SmoothDraggedSegments`: within a quarter of the track width (Highlight collisions, Walk around) or half of it (Shove) of the line of
    /// the parallel segment two along, the dragged segment lines up with it and the two merge. Off: nothing snaps. (Expected values:
    /// KiCad's own `dragSegment45`, see `line.rs`.)
    #[test]
    fn smooth_dragged_segments_lines_the_slid_segment_up_with_the_next_parallel_one() {
        let mut node = Node::new();
        let pts = [p(0, 0), p(1_000_000, 0), p(2_000_000, 1_000_000), p(3_000_000, 1_000_000), p(4_000_000, 2_000_000)];
        let ids: Vec<ItemId> = pts.windows(2).enumerate().map(|(i, w)| node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: w[0], b: w[1], width: mm(0.25), source_track: Some(("t".into(), i)), locked: false }))).collect();
        let rules = rules();
        // the cursor is 70 um from the line of the last diagonal: inside the 125 of Shove (width / 2), outside the 62 of Walk around (width / 4)
        let slid = |settings: &RoutingSettings| {
            let mut d = Dragger::start(&node, p(1_500_000, 500_000), ids[1]).unwrap();
            assert_eq!((d.kind, d.grabbed_index), (DragKind::Segment, 1));
            d.drag(&node, &rules, settings, p(2_000_000, 100_000)).pts
        };
        let shove = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        assert_eq!(slid(&shove), vec![p(0, 0), p(2_000_000, 0), p(4_000_000, 2_000_000)], "merged with the next diagonal");
        let walk = RoutingSettings { mode: Mode::Walkaround, ..RoutingSettings::default() };
        assert_eq!(slid(&walk), vec![p(0, 0), p(1_900_000, 0), p(2_900_000, 1_000_000), p(3_000_000, 1_000_000), p(4_000_000, 2_000_000)], "62 um is not enough");
        let rough = RoutingSettings { smooth_dragged_segments: false, ..shove };
        assert_eq!(slid(&rough), vec![p(0, 0), p(1_900_000, 0), p(2_900_000, 1_000_000), p(3_000_000, 1_000_000), p(4_000_000, 2_000_000)], "with smoothing off nothing snaps");
    }

    /// `optimizeAndUpdateDraggedLine`: with "Optimize entire track being dragged" on, the dragged line is optimized -- shortcuts where the drag changed
    /// the line, the point of the line nearest the cursor kept. Off (KiCad's default), the line is the drag's own. Here the middle segment of two jogs is
    /// dragged to a place where it makes a bump; the bump and the jogs next to it are merged into the diagonal the line had.
    #[test]
    fn optimize_entire_track_being_dragged_merges_what_the_drag_left_next_to_the_dragged_segment() {
        let mut node = Node::new();
        let pts = [p(0, 0), p(1_000_000, 0), p(1_500_000, 500_000), p(2_500_000, 500_000), p(3_000_000, 1_000_000), p(4_000_000, 1_000_000)];
        let ids: Vec<ItemId> = pts.windows(2).enumerate().map(|(i, w)| node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: w[0], b: w[1], width: mm(0.25), source_track: Some(("t".into(), i)), locked: false }))).collect();
        let rules = rules();
        let dragged = |settings: &RoutingSettings| {
            let mut d = Dragger::start(&node, p(2_000_000, 500_000), ids[2]).unwrap();
            assert_eq!((d.kind, d.grabbed_index), (DragKind::Segment, 2));
            let pv = d.drag(&node, &rules, settings, p(1_400_000, 800_000));
            assert!(!pv.colliding && !pv.restored);
            (pv.pts.clone(), d.finish(&node, &rules, settings, p(1_400_000, 800_000), false).expect("a clear drag commits").tracks[0].pts.clone())
        };
        let off = RoutingSettings { mode: Mode::Walkaround, smooth_dragged_segments: false, ..RoutingSettings::default() };
        let on = RoutingSettings { optimize_entire_dragged_track: true, ..off.clone() };
        let (plain, plain_commit) = dragged(&off);
        assert_eq!(plain, vec![p(0, 0), p(1_000_000, 0), p(1_800_000, 800_000), p(2_800_000, 800_000), p(3_000_000, 1_000_000), p(4_000_000, 1_000_000)]);
        assert_eq!(plain_commit, plain);
        let (optimized, optimized_commit) = dragged(&on);
        assert_eq!(optimized, vec![p(0, 0), p(1_000_000, 0), p(2_000_000, 1_000_000), p(4_000_000, 1_000_000)], "the bump is gone: it is the shortcut with the fewest corners that still passes the cursor's point of the line");
        assert_eq!(optimized_commit, optimized, "what is committed is what is shown");
        assert!(on_45(&optimized));
        // Shove mode runs the same hook against the world the shove left
        let on_shove = RoutingSettings { mode: Mode::Shove, ..on };
        let (shoved, _) = dragged(&on_shove);
        assert!(shoved.len() < plain.len() && on_45(&shoved), "{shoved:?}");
    }

    /// A 30-degree segment (imported geometry, not on KiCad's 45-degree grid) between a horizontal and a vertical leg is dragged at the
    /// 45-degree heading it quantizes to, and the legs follow: every leg of the result is on a 45-degree heading. (Expected values:
    /// KiCad's own `dragSegment45`, see `line.rs`.)
    #[test]
    fn a_30_degree_segment_is_dragged_at_45_degrees_with_its_neighbours() {
        let mut node = Node::new();
        let ids = add_track(&mut node, "SIG", "t", &[p(0, 0), p(2_000_000, 0), p(3_732_000, 1_000_000), p(3_732_000, 3_000_000)]);
        let (rules, settings) = (rules(), RoutingSettings::default());
        let mut dragger = Dragger::start(&node, p(2_866_000, 500_000), ids[1]).unwrap();
        assert_eq!((dragger.kind, dragger.grabbed_index), (DragKind::Segment, 1), "the middle of the 30-degree segment");
        let pv = dragger.drag(&node, &rules, &settings, p(3_000_000, 1_500_000));
        assert!(!pv.colliding);
        assert_eq!(pv.pts, vec![p(0, 0), p(1_500_000, 0), p(3_732_000, 2_232_000), p(3_732_000, 3_000_000)]);
        assert!(on_45(&pv.pts), "{:?}", pv.pts);
        // the far ends did not move
        assert_eq!((pv.pts.first(), pv.pts.last()), (Some(&p(0, 0)), Some(&p(3_732_000, 3_000_000))));
    }

    /// A SIG track (0,0)-(1000,0) whose end is dragged, and a GND pad at (3000, 0) that the end cannot be dropped on.
    fn track_end_and_a_pad() -> (Node, Dragger) {
        use crate::item::Solid;
        let mut node = Node::new();
        let seg = node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: p(0, 0), b: p(1_000_000, 0), width: mm(0.2), source_track: Some(("t".into(), 0)), locked: false }));
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: p(3_000_000, 0), shape: Shape::Circle { c: p(3_000_000, 0), r: mm(0.4) }, source: "U1.1".into(), edge: false, rule: 0 }));
        let dragger = Dragger::start(&node, p(1_000_000, 0), seg).unwrap();
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
            let first = dragger.drag(&node, &rules, &settings, p(1_000_000, 0));
            assert!(!first.colliding && !first.restored && !first.forced_mark_obstacles, "{mode:?}: the priming move at the grab point");
            let valid = dragger.drag(&node, &rules, &settings, p(2_000_000, 1_500_000));
            assert!(!valid.colliding && !valid.restored, "{mode:?}: {valid:?}");
            assert_eq!(valid.pts.last(), Some(&p(2_000_000, 1_500_000)));
            assert!(dragger.drag_status());
            // on the pad: the end has nowhere to go
            let on_the_pad = dragger.drag(&node, &rules, &settings, p(3_000_000, 0));
            assert!(on_the_pad.restored, "{mode:?}: the preview is not the illegal position: {on_the_pad:?}");
            // `m_lastDragSolution`: the last shoved line in Shove mode; in Walk around mode the line as the drag started (round-2 R-7)
            let shown = if mode == Mode::Shove { valid.pts.clone() } else { vec![p(0, 0), p(1_000_000, 0)] };
            assert_eq!(on_the_pad.pts, shown, "{mode:?}");
            assert!(!on_the_pad.colliding);
            assert!(!dragger.drag_status(), "{mode:?}: the cursor is not at a position that can be committed");
            assert!(!dragger.force_mark_obstacles());
            // letting go there commits the last valid point
            let commit = dragger.finish(&node, &rules, &settings, p(3_000_000, 0), false).expect("a release over an illegal position commits the last valid one");
            assert_eq!(commit.tracks[0].pts, valid.pts, "{mode:?}");
            assert_eq!(commit.remove_track_ids, vec!["t"]);
            // a legal position again is not restored
            let again = dragger.drag(&node, &rules, &settings, p(2_000_000, 1_000_000));
            assert!(!again.restored && dragger.drag_status(), "{mode:?}");
        }
    }

    /// Round-2 R-7: `DRAGGER::Drag`'s "Restore last solution" in Walk around mode adds `m_lastDragSolution` to a branch of the world, and
    /// `dragWalkaround` never sets it: it is the line `startDragSegment` assembled. A position that cannot be walked shows the track where it
    /// started (not the last good position), with nothing dragged; releasing there still commits the last valid point.
    #[test]
    fn walkaround_restore_shows_the_original_line() {
        let (node, mut dragger) = track_end_and_a_pad();
        let (rules, settings) = (rules(), RoutingSettings { mode: Mode::Walkaround, ..RoutingSettings::default() });
        dragger.drag(&node, &rules, &settings, p(1_000_000, 0));
        let valid = dragger.drag(&node, &rules, &settings, p(2_000_000, 1_500_000));
        assert!(!valid.restored && valid.pts.last() == Some(&p(2_000_000, 1_500_000)), "{valid:?}");
        let boxed_in = dragger.drag(&node, &rules, &settings, p(3_000_000, 0));
        assert!(boxed_in.restored);
        assert_eq!(boxed_in.pts, dragger.original.pts, "the original line, not the first position's");
        assert!(boxed_in.displaced_lines.is_empty() && boxed_in.displaced_vias.is_empty());
        let commit = dragger.finish(&node, &rules, &settings, p(3_000_000, 0), false).expect("the last valid point is committed");
        assert_eq!(commit.tracks[0].pts, valid.pts);
    }

    /// Round-2 R-7: in Shove mode `m_lastDragSolution` is `draggedPostShove` before the optimizer: `optimizeAndUpdateDraggedLine` splits it at the
    /// anchor (`aDragged.Line().Split( anchor )`) and optimizes a copy (`optimizer.Optimize( &aDragged, &draggedPostOpt, .. )`). With "Optimize entire
    /// track being dragged" on, the line a later failed position shows again is that split line, not the optimized one the preview draws.
    #[test]
    fn shove_restore_shows_the_line_before_the_optimizer() {
        let mut node = Node::new();
        let pts = [p(0, 0), p(1_000_000, 0), p(1_500_000, 500_000), p(2_500_000, 500_000), p(3_000_000, 1_000_000), p(4_000_000, 1_000_000)];
        let ids: Vec<ItemId> = pts.windows(2).enumerate().map(|(i, w)| node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: w[0], b: w[1], width: mm(0.25), source_track: Some(("t".into(), i)), locked: false }))).collect();
        let rules = rules();
        let cursor = p(1_400_000, 800_000);
        let run = |settings: &RoutingSettings| {
            let d = Dragger::start(&node, p(2_000_000, 500_000), ids[2]).unwrap();
            let pv = d.preview(&node, &rules, settings, cursor);
            assert!(!pv.colliding && !pv.restored, "{pv:?}");
            pv
        };
        let off = RoutingSettings { mode: Mode::Shove, smooth_dragged_segments: false, ..RoutingSettings::default() };
        let on = RoutingSettings { optimize_entire_dragged_track: true, ..off.clone() };
        let (plain, optimized) = (run(&off), run(&on));
        assert_ne!(optimized.pts, plain.pts, "the optimizer changed the line");
        assert_eq!(plain.solution.as_ref().map(|s| &s.0), Some(&plain.pts), "with the optimizer off the solution is the shoved line");
        let mut split = plain.pts.clone();
        split_chain_at(&mut split, if plain.pts.contains(&cursor) { cursor } else { nearest_point_of(&plain.pts, cursor) });
        assert_eq!(optimized.solution.as_ref().map(|s| &s.0), Some(&split), "with it on, the shoved line split at the anchor, not optimized");
    }

    /// `m_forceMarkObstaclesMode`: when the very first position fails, the drag goes on marking obstacles, and only Ctrl+click commits
    /// what is in violation.
    #[test]
    fn when_the_first_position_fails_the_drag_only_marks_obstacles_and_ctrl_click_commits_it() {
        let (node, mut dragger) = track_end_and_a_pad();
        let (rules, settings) = (rules(), RoutingSettings { mode: Mode::Walkaround, ..RoutingSettings::default() });
        let pv = dragger.drag(&node, &rules, &settings, p(3_000_000, 0));
        assert!(pv.forced_mark_obstacles && pv.colliding && !pv.restored, "{pv:?}");
        assert_eq!(pv.pts.last(), Some(&p(3_000_000, 0)), "the end is where the cursor is, in violation");
        assert!(dragger.force_mark_obstacles() && !dragger.drag_status());
        // still marking: another position that collides is shown where the cursor is
        let again = dragger.drag(&node, &rules, &settings, p(3_100_000, 100_000));
        assert!(again.colliding && again.forced_mark_obstacles && !again.restored);
        assert!(dragger.finish(&node, &rules, &settings, p(3_100_000, 100_000), false).is_none(), "a plain click does not commit a violation");
        let forced = dragger.finish(&node, &rules, &settings, p(3_100_000, 100_000), true).expect("Ctrl+click commits it anyway");
        assert_eq!(forced.tracks[0].pts, again.pts);
        // and a position that does not collide can be committed plainly
        let clear = dragger.drag(&node, &rules, &settings, p(2_000_000, 1_500_000));
        assert!(!clear.colliding && dragger.drag_status());
        assert!(dragger.finish(&node, &rules, &settings, p(2_000_000, 1_500_000), false).is_some());
    }

    /// In Highlight collisions mode every position "works": the collision is marked, a release over it is refused (no restore: the
    /// cursor is the last valid point there), and Ctrl+click is not needed once it clears.
    #[test]
    fn highlight_collisions_marks_and_refuses_a_colliding_release() {
        let (node, mut dragger) = track_end_and_a_pad();
        let (rules, settings) = (rules(), RoutingSettings { mode: Mode::MarkObstacles, ..RoutingSettings::default() });
        let pv = dragger.drag(&node, &rules, &settings, p(3_000_000, 0));
        assert!(pv.colliding && !pv.restored && !pv.forced_mark_obstacles);
        assert_eq!(pv.pts.last(), Some(&p(3_000_000, 0)));
        assert!(dragger.finish(&node, &rules, &settings, p(3_000_000, 0), false).is_none());
        let allow = RoutingSettings { can_violate_drc: true, ..settings };
        assert!(dragger.finish(&node, &rules, &allow, p(3_000_000, 0), false).is_some(), "Allow DRC violations commits it");
    }

    /// An IR track the dragged line stands on only in part keeps the rest: a branch cuts the track into two lines, the first is dragged.
    #[test]
    fn the_part_of_a_track_the_dragged_line_did_not_cover_goes_back() {
        let mut node = Node::new();
        let ids = add_track(&mut node, "SIG", "t", &[p(0, 0), p(2_000_000, 0), p(2_000_000, 2_000_000), p(4_000_000, 2_000_000)]);
        // a second track of the same net leaves the first corner: the first segment is a line of its own
        add_track(&mut node, "SIG", "branch", &[p(2_000_000, 0), p(2_000_000, -2_000_000)]);
        let (rules, settings) = (rules(), RoutingSettings::default());
        let mut dragger = Dragger::start(&node, p(1_000_000, 0), ids[0]).unwrap();
        assert_eq!((dragger.kind, dragger.grabbed_index), (DragKind::Segment, 0));
        assert_eq!(dragger.original.pts, vec![p(0, 0), p(2_000_000, 0)]);
        let commit = dragger.finish(&node, &rules, &settings, p(1_000_000, 600_000), false).expect("clear");
        assert_eq!(commit.remove_track_ids, vec!["t"]);
        let mut lines: Vec<&Vec<Point>> = commit.tracks.iter().map(|l| &l.pts).collect();
        lines.sort();
        assert_eq!(lines.len(), 2, "the dragged line and the rest of t: {lines:?}");
        assert!(lines.contains(&&vec![p(2_000_000, 0), p(2_000_000, 2_000_000), p(4_000_000, 2_000_000)]), "the rest of t comes back as it was: {lines:?}");
        let dragged = commit.tracks.iter().find(|l| l.pts.first() == Some(&p(0, 0))).expect("the dragged line");
        assert_eq!(dragged.pts.last(), Some(&p(2_000_000, 0)), "its far end stays at the junction");
        assert!(on_45(&dragged.pts));
    }

    #[test]
    fn dragging_a_via_drags_its_connected_track_with_it() {
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: Nm::ZERO, y: Nm::ZERO }, b: Point { x: mm(1.0), y: Nm::ZERO }, width: mm(0.2), source_track: Some(("trkA".into(), 0)), locked: false }));
        let via_id = node.add(Item::Via(Via { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: Point { x: mm(1.0), y: Nm::ZERO }, diameter: mm(0.6), drill: mm(0.3), source_via: Some("viaA".into()), locked: false, is_virtual: false }));
        let rules = rules();
        let settings = RoutingSettings::default();
        let mut dragger = Dragger::start(&node, Point { x: mm(1.0), y: Nm::ZERO }, via_id).expect("must start a via drag");
        assert_eq!(dragger.kind, DragKind::Via);
        let to = Point { x: mm(1.0), y: mm(2.0) };
        let preview = dragger.preview(&node, &rules, &settings, to);
        assert!(!preview.colliding);
        assert_eq!(preview.fanout.len(), 1, "the live preview must carry the attached track's stretched shape too, not just the via's own new point");
        // `DragCorner` re-solves the track's last leg at 45 degrees (`dragViaWalkaround`)
        let elbow = vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::ZERO, y: mm(1.0) }, to];
        assert_eq!(preview.fanout[0].pts, elbow, "the fanout track must already end at the via's live (not yet committed) position");
        let commit = dragger.finish(&node, &rules, &settings, to, false).expect("collision-free via drag must commit");
        assert_eq!(commit.remove_via_ids, vec!["viaA"]);
        assert_eq!(commit.vias[0].0, to);
        assert_eq!(commit.remove_track_ids, vec!["trkA"]);
        assert_eq!(commit.tracks[0].pts, elbow, "the attached track must stretch to follow the via");
        // Highlight collisions re-solves it at 45 degrees too (`dragViaMarkObstacles`: `DragCorner( .., m_freeAngleMode )`, findings p5 F3-1)
        let marking = RoutingSettings { mode: Mode::MarkObstacles, ..settings };
        assert_eq!(dragger.preview(&node, &rules, &marking, to).fanout[0].pts, elbow);
    }

    /// A via, SIG, with a track coming in from the left, in a world with a GND track straight across where the via is dragged to.
    fn via_across_a_gnd_track() -> (Node, Dragger) {
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: mm(-3.0), y: Nm::ZERO }, b: Point { x: Nm::ZERO, y: Nm::ZERO }, width: mm(0.2), source_track: Some(("trkA".into(), 0)), locked: false }));
        let via_id = node.add(Item::Via(Via { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: Point { x: Nm::ZERO, y: Nm::ZERO }, diameter: mm(0.6), drill: mm(0.3), source_via: Some("viaA".into()), locked: false, is_virtual: false }));
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: mm(4.0), y: mm(-3.0) }, b: Point { x: mm(4.0), y: mm(3.0) }, width: mm(0.2), source_track: Some(("trkB".into(), 0)), locked: false }));
        let dragger = Dragger::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, via_id).expect("a via drag");
        assert_eq!(dragger.kind, DragKind::Via);
        (node, dragger)
    }

    /// findings p5 F3-1: in Highlight collisions a via's tracks follow it at 45 degrees (`dragViaMarkObstacles`: `DragCorner( aP,
    /// Find( aHandle.pos ), m_freeAngleMode )`, so `dragCorner45` for `D`); only a free-angle drag (`G`) leaves a straight leg.
    #[test]
    fn mark_obstacles_via_drag_keeps_its_tracks_at_45_degrees() {
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: Nm::ZERO, y: Nm::ZERO }, b: Point { x: mm(5.0), y: Nm::ZERO }, width: mm(0.2), source_track: Some(("trkA".into(), 0)), locked: false }));
        let via_id = node.add(Item::Via(Via { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: Point { x: Nm::ZERO, y: Nm::ZERO }, diameter: mm(0.6), drill: mm(0.3), source_via: Some("viaA".into()), locked: false, is_virtual: false }));
        let rules = rules();
        let settings = RoutingSettings { mode: Mode::MarkObstacles, ..RoutingSettings::default() };
        let to = Point { x: mm(2.0), y: mm(1.3) };
        let d = Dragger::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, via_id).expect("a via drag");
        let pv = d.preview(&node, &rules, &settings, to);
        assert_eq!(pv.fanout.len(), 1);
        let pts = &pv.fanout[0].pts;
        assert_eq!(pts.len(), 3, "an elbow: {pts:?}");
        assert!(on_45(pts), "{pts:?}");
        assert!(pts.contains(&to) && pts.contains(&Point { x: mm(5.0), y: Nm::ZERO }), "{pts:?}");
        let free = Dragger::start_with(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, via_id, true).expect("a free-angle via drag");
        let pv = free.preview(&node, &rules, &settings, to);
        assert_eq!(pv.fanout[0].pts.len(), 2, "one straight leg: {:?}", pv.fanout[0].pts);
    }

    /// D7, `DRAGGER::dragShove` for `DM_VIA`: the via is the head of a shove, the track attached to it goes with it and the
    /// GND track in its way is pushed aside, in one commit.
    #[test]
    fn shove_mode_via_drag_pushes_what_is_in_the_way_of_the_via_and_its_track() {
        let (node, mut dragger) = via_across_a_gnd_track();
        let rules = rules();
        let settings = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let to = Point { x: mm(5.0), y: Nm::ZERO };
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
        after.add(Item::Via(Via { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: to, diameter: mm(0.6), drill: mm(0.3), source_via: None, locked: false, is_virtual: false }));
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
        let to = Point { x: mm(5.0), y: Nm::ZERO };
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
        node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: mm(-3.0), y: Nm::ZERO }, b: Point { x: Nm::ZERO, y: Nm::ZERO }, width: mm(0.2), source_track: Some(("trkA".into(), 0)), locked: false }));
        let via_id = node.add(Item::Via(Via { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: Point { x: Nm::ZERO, y: Nm::ZERO }, diameter: mm(0.6), drill: mm(0.3), source_via: Some("viaA".into()), locked: false, is_virtual: false }));
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: mm(4.0), y: Nm::ZERO }, shape: Shape::Circle { c: Point { x: mm(4.0), y: Nm::ZERO }, r: mm(0.5) }, source: "U1.1".into(), edge: false, rule: 0 }));
        let rules = rules();
        let onto_the_pad = Point { x: mm(4.0), y: mm(0.3) };
        for mode in [Mode::Walkaround, Mode::Shove] {
            let mut dragger = Dragger::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, via_id).unwrap();
            let settings = RoutingSettings { mode, ..RoutingSettings::default() };
            let preview = dragger.preview(&node, &rules, &settings, onto_the_pad);
            assert!(!preview.colliding, "{mode:?}: {preview:?}");
            let end = preview.pts[0];
            let centre_distance = (((end.x - mm(4.0)).as_f64()).powi(2) + (end.y.as_f64()).powi(2)).sqrt();
            assert!(centre_distance >= 500_000.0 + 300_000.0 + 200_000.0 - 500.0, "{mode:?}: the via ended {centre_distance} um from the pad's centre, at {end:?}");
            assert_eq!(preview.fanout[0].last(), Some(end), "{mode:?}: the track ends at the via");
            let commit = dragger.finish(&node, &rules, &settings, onto_the_pad, false).expect("a via that clears commits");
            assert_eq!(commit.vias[0].0, end, "{mode:?}: the via is committed where it ended, not where the cursor was");
        }
        // Highlight collisions leaves it where the cursor put it, and says so.
        let dragger = Dragger::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, via_id).unwrap();
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
        let seg_id = node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: Nm::ZERO, y: Nm::ZERO }, b: Point { x: mm(1.0), y: Nm::ZERO }, width: mm(0.2), source_track: Some(("trkA".into(), 0)), locked: false }));
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: mm(3.0), y: Nm::ZERO }, shape: Shape::Circle { c: Point { x: mm(3.0), y: Nm::ZERO }, r: mm(0.4) }, source: "U1.1".into(), edge: false, rule: 0 }));
        let rules = rules();
        let mut dragger = Dragger::start(&node, Point { x: mm(1.0), y: Nm::ZERO }, seg_id).unwrap();
        let settings = RoutingSettings::default();
        assert_eq!(settings.mode, Mode::Walkaround);
        let preview = dragger.preview(&node, &rules, &settings, Point { x: mm(5.0), y: Nm::ZERO });
        assert!(!preview.colliding, "the drag is walked around the pad: {preview:?}");
        assert!(preview.pts.len() > 2 && preview.pts.last() == Some(&Point { x: mm(5.0), y: Nm::ZERO }) && preview.pts.first() == Some(&Point { x: Nm::ZERO, y: Nm::ZERO }), "{:?}", preview.pts);
        let commit = dragger.finish(&node, &rules, &settings, Point { x: mm(5.0), y: Nm::ZERO }, false).expect("the walked drag commits");
        assert_eq!(commit.tracks[0].pts, preview.pts);
        // Dropped on the pad itself, there is nowhere for the end to go: the pad's place is refused, and the last valid point -- the
        // walked drag above -- is what a release there commits
        let on_the_pad = dragger.preview(&node, &rules, &settings, Point { x: mm(3.0), y: Nm::ZERO });
        assert!(on_the_pad.colliding);
        let commit = dragger.finish(&node, &rules, &settings, Point { x: mm(3.0), y: Nm::ZERO }, false).expect("the last valid point");
        assert_eq!(commit.tracks[0].pts, preview.pts);
        // a drag whose first position is the pad has no valid point to fall back to
        let mut fresh = Dragger::start(&node, Point { x: mm(1.0), y: Nm::ZERO }, seg_id).unwrap();
        assert!(fresh.finish(&node, &rules, &settings, Point { x: mm(3.0), y: Nm::ZERO }, false).is_none());
    }

    #[test]
    fn shove_mode_pushes_an_obstacle_out_of_a_dragged_tracks_new_path() {
        let mut node = Node::new();
        let seg_id = node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: Nm::ZERO, y: Nm::ZERO }, b: Point { x: mm(1.0), y: Nm::ZERO }, width: mm(0.2), source_track: Some(("trkA".into(), 0)), locked: false }));
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: mm(2.5), y: mm(-2.0) }, b: Point { x: mm(2.5), y: mm(2.0) }, width: mm(0.2), source_track: Some(("trkB".into(), 0)), locked: false }));
        let rules = rules();
        let settings = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let dragger = Dragger::start(&node, Point { x: mm(1.0), y: Nm::ZERO }, seg_id).unwrap();
        let preview = dragger.preview(&node, &rules, &settings, Point { x: mm(5.0), y: Nm::ZERO });
        assert!(!preview.colliding, "shove must clear the crossing track rather than reporting a collision");
        assert_eq!(preview.displaced_lines.len(), 1);
        assert_eq!(preview.displaced_lines[0].source_track.as_deref(), Some("trkB"));
    }

    /// `dragShove` for a segment: the slid segment pushes the track in its way, and the board it leaves is clean.
    #[test]
    fn shove_mode_segment_drag_pushes_a_track_out_of_the_way() {
        let mut node = Node::new();
        let ids = add_track(&mut node, "SIG", "u", &[p(0, -3_000_000), p(0, 0), p(6_000_000, 0), p(6_000_000, -3_000_000)]);
        // a GND track below, across where the middle leg is dragged to
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: p(3_000_000, 1_000_000), b: p(3_000_000, 6_000_000), width: mm(0.2), source_track: Some(("gnd".into(), 0)), locked: false }));
        let rules = rules();
        let settings = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let mut dragger = Dragger::start(&node, p(4_500_000, 0), ids[1]).unwrap();
        assert_eq!(dragger.kind, DragKind::Segment);
        let pv = dragger.drag(&node, &rules, &settings, p(4_500_000, 2_500_000));
        assert!(!pv.colliding && !pv.restored, "{pv:?}");
        assert_eq!(pv.displaced_lines.len(), 1, "the GND track is pushed: {:?}", pv.displaced_lines);
        assert_eq!(pv.displaced_lines[0].source_track.as_deref(), Some("gnd"));
        let commit = dragger.finish(&node, &rules, &settings, p(4_500_000, 2_500_000), false).expect("commits");
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
        let seg_id = node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: Nm::ZERO, y: Nm::ZERO }, b: Point { x: mm(1.0), y: Nm::ZERO }, width: mm(0.2), source_track: Some(("trkA".into(), 0)), locked: false }));
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: mm(3.0), y: Nm::ZERO }, shape: Shape::Circle { c: Point { x: mm(3.0), y: Nm::ZERO }, r: mm(0.4) }, source: "U1.1".into(), edge: false, rule: 0 }));
        let rules = rules();
        let settings = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let mut dragger = Dragger::start(&node, Point { x: mm(1.0), y: Nm::ZERO }, seg_id).unwrap();
        let preview = dragger.preview(&node, &rules, &settings, Point { x: mm(6.0), y: Nm::ZERO });
        assert!(!preview.colliding);
        assert!(preview.pts.len() > 2, "walked around the pad: {:?}", preview.pts);
        assert_eq!(preview.pts.first(), Some(&Point { x: Nm::ZERO, y: Nm::ZERO }));
        assert_eq!(preview.pts.last(), Some(&Point { x: mm(6.0), y: Nm::ZERO }));
        for w in preview.pts.windows(2) {
            let leg = Shape::Stadium { a: w[0], b: w[1], r: mm(0.1) };
            assert!(leg.collides(&Shape::Circle { c: Point { x: mm(3.0), y: Nm::ZERO }, r: mm(0.4) }, mm(0.199)).is_none(), "{:?} touches the pad", w);
        }
        // and what finish() commits is that shape
        let commit = dragger.finish(&node, &rules, &settings, Point { x: mm(6.0), y: Nm::ZERO }, false).expect("the walked drag is accepted");
        assert_eq!(commit.tracks[0].pts, preview.pts);
    }

    #[test]
    fn free_angle_drag_never_shoves_it_only_reports_the_collision() {
        let mut node = Node::new();
        let seg_id = node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: Nm::ZERO, y: Nm::ZERO }, b: Point { x: mm(1.0), y: Nm::ZERO }, width: mm(0.2), source_track: Some(("trkA".into(), 0)), locked: false }));
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: mm(2.5), y: mm(-2.0) }, b: Point { x: mm(2.5), y: mm(2.0) }, width: mm(0.2), source_track: Some(("trkB".into(), 0)), locked: false }));
        let rules = rules();
        // Shove mode: a plain drag pushes the crossing track aside ...
        let settings = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let plain = Dragger::start(&node, Point { x: mm(1.0), y: Nm::ZERO }, seg_id).unwrap();
        assert!(!plain.free_angle);
        assert!(!plain.preview(&node, &rules, &settings, Point { x: mm(5.0), y: Nm::ZERO }).colliding);
        // ... but DM_FREE_ANGLE (`G`) always takes `dragMarkObstacles`: nothing is displaced, the collision is reported.
        let mut free = Dragger::start_with(&node, Point { x: mm(1.0), y: Nm::ZERO }, seg_id, true).unwrap();
        assert!(free.free_angle);
        let preview = free.preview(&node, &rules, &settings, Point { x: mm(5.0), y: Nm::ZERO });
        assert!(preview.colliding, "a free-angle drag must report, not resolve, the collision");
        assert!(preview.displaced_lines.is_empty());
        // `FixRoute` refuses the colliding drop (outside MarkObstacles + can_violate_drc) ...
        assert!(free.finish(&node, &rules, &settings, Point { x: mm(5.0), y: Nm::ZERO }, false).is_none());
        // ... and commits a clear one, free angle included (the grabbed end simply follows the cursor).
        let commit = free.finish(&node, &rules, &settings, Point { x: mm(1.0), y: mm(1.0) }, false).expect("a clear free-angle drop commits");
        assert_eq!(commit.tracks[0].pts, vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: mm(1.0) }]);
        // a free-angle grab in the middle of a segment is a corner (the nearer vertex), not a slide
        let mut node = Node::new();
        let long = node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: Nm::ZERO, y: Nm::ZERO }, b: Point { x: mm(4.0), y: Nm::ZERO }, width: mm(0.2), source_track: Some(("t".into(), 0)), locked: false }));
        let mid = Dragger::start_with(&node, Point { x: mm(1.5), y: Nm::ZERO }, long, true).unwrap();
        assert_eq!((mid.kind, mid.grabbed_index), (DragKind::Corner, 0));
    }

    /// `DRAGGER`'s single `SHOVE` (`m_shove`): a frame for each position that shoved, the stack reduced against the head before a
    /// move (`SHOVE::reduceSpringback`) and the world a shove leaves pushed after it. A via the drag pushed stays pushed -- in the
    /// preview and in the commit -- while the dragged line still collides with something of that frame's world (here a pad above
    /// its path, which the shove walks it around), even at a position that no longer touches the via. Without the stack every
    /// position re-derives from the committed world, so the via is back where it started the moment the head has moved off it.
    #[test]
    fn a_pushed_via_stays_pushed_while_the_drag_still_presses_on_the_frame() {
        use crate::item::Solid;
        let mut node = Node::new();
        let ids = add_track(&mut node, "SIG", "t", &[p(0, 0), p(4_000_000, 0), p(4_000_000, 5_000_000)]);
        node.add(Item::Via(Via { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: p(2_000_000, 800_000), diameter: mm(0.6), drill: mm(0.3), source_via: Some("viaM".into()), locked: false, is_virtual: false }));
        // a pad above the line's path: the head is walked around it there, so it still collides with the frame's world
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: p(2_000_000, 3_600_000), shape: Shape::Circle { c: p(2_000_000, 3_600_000), r: mm(0.4) }, source: "U1.1".into(), edge: false, rule: 0 }));
        let rules = rules();
        let settings = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let mut dragger = Dragger::start(&node, p(2_000_000, 0), ids[0]).unwrap();
        assert_eq!(dragger.kind, DragKind::Segment);
        // the first position pushes the via off the dragged segment, to clearance below it
        let first = dragger.drag(&node, &rules, &settings, p(2_000_000, 1_000_000));
        assert!(!first.colliding && !first.restored, "{first:?}");
        let pushed = |pv: &DragPreview| pv.displaced_vias.iter().map(|v| (v.source_via.clone(), v.pos)).collect::<Vec<_>>();
        assert_eq!(pushed(&first), vec![("viaM".to_string(), p(2_000_000, 400_000))], "the via is pushed to clearance below the line: {first:?}");
        // the head is now far from the via and only meets the pad: the frame that pushed the via is still the one the shove starts from
        let second = dragger.drag(&node, &rules, &settings, p(2_000_000, 3_000_000));
        assert!(!second.colliding && !second.restored, "{second:?}");
        assert_eq!(pushed(&second), vec![("viaM".to_string(), p(2_000_000, 400_000))], "the via stays pushed: {second:?}");
        let commit = dragger.finish(&node, &rules, &settings, p(2_000_000, 3_000_000), false).expect("the drag commits");
        assert!(commit.remove_via_ids.contains(&"viaM".to_string()), "the via the drag pushed is replaced: {commit:?}");
        assert!(commit.vias.iter().any(|v| v.0 == p(2_000_000, 400_000) && v.3.as_deref() == Some("viaM")), "the pushed via is committed where the drag left it: {commit:?}");
        // the first frame of a gesture is never popped (`while( m_nodeStack.size() > 1 )`): drawn back away from everything the
        // via still stays where that frame put it, as in KiCad
        let away = dragger.drag(&node, &rules, &settings, p(2_000_000, -2_000_000));
        assert!(!away.colliding && !away.restored, "{away:?}");
        assert_eq!(pushed(&away), vec![("viaM".to_string(), p(2_000_000, 400_000))], "the bottom frame is never popped: {away:?}");
    }

    /// `DRAGGER::dragShove`, `DM_VIA`, when the shove fails: `dragViaWalkaround( m_draggedVia, m_lastNode, aP )` works in a branch of
    /// the frame the stack was reduced to, so what the earlier positions pushed stays (preview and commit), and the fan-out it walks
    /// replaces the via's own tracks -- the frame's lines of them, which the frame re-made, map back to their IR tracks.
    #[test]
    fn a_via_shove_that_fails_walks_around_in_the_frame_and_keeps_its_pushes() {
        let (mut node, _) = via_across_a_gnd_track();
        // a locked GND track the via is then dragged onto: the shove cannot move it, so the via is pushed off it and its track walked
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: p(-2_000_000, 3_000_000), b: p(2_000_000, 3_000_000), width: mm(0.2), source_track: Some(("trkL".into(), 0)), locked: true }));
        let via_id = node.iter().find_map(|(id, it)| matches!(it, Item::Via(_)).then_some(id)).unwrap();
        let mut dragger = Dragger::start(&node, p(0, 0), via_id).expect("a via drag");
        let rules = rules();
        let settings = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        // the via pushes the GND track aside: the gesture's one frame, with trkA re-made to end at the via's new place
        let first = dragger.drag(&node, &rules, &settings, p(4_000_000, 0));
        assert!(!first.colliding && !first.restored, "{first:?}");
        let pushed_b = first.displaced_lines.iter().find(|d| d.source_track.as_deref() == Some("trkB")).expect("trkB is pushed").line.pts.clone();
        // onto the locked track: the shove fails, `dragViaWalkaround` works in the frame
        let to = p(500_000, 3_200_000);
        let second = dragger.drag(&node, &rules, &settings, to);
        assert!(!second.colliding && !second.restored, "{second:?}");
        assert_ne!(second.pts, vec![to], "the via is pushed off the locked track: {second:?}");
        let b_of = |lines: &[DisplacedLine]| lines.iter().filter(|d| d.source_track.as_deref() == Some("trkB")).map(|d| d.line.pts.clone()).collect::<Vec<_>>();
        assert_eq!(b_of(&second.displaced_lines), vec![pushed_b.clone()], "what the frame pushed stays in the preview: {second:?}");
        assert!(second.displaced_lines.iter().all(|d| d.source_track.as_deref() != Some("trkA")), "the via's own track is the fan-out, not a displaced line: {second:?}");
        assert!(second.displaced_vias.is_empty(), "the dragged via is not a displaced via: {second:?}");
        assert_eq!(second.fanout.len(), 1, "{second:?}");
        assert_eq!(second.fanout[0].pts.last(), second.pts.first(), "the walked track ends at the via: {second:?}");
        let commit = dragger.finish(&node, &rules, &settings, to, false).expect("the walked position commits");
        let mut removed = commit.remove_track_ids.clone();
        removed.sort();
        assert_eq!(removed, vec!["trkA".to_string(), "trkB".to_string()], "the via's original track and the pushed track are replaced, once each: {commit:?}");
        let sig: Vec<_> = commit.tracks.iter().filter(|l| l.net == net_of("SIG")).collect();
        assert_eq!(sig.len(), 1, "one SIG track, the walked one -- not the original beside it: {commit:?}");
        assert_eq!(sig[0].pts, second.fanout[0].pts);
        let gnd: Vec<_> = commit.tracks.iter().filter(|l| l.net == net_of("GND")).map(|l| l.pts.clone()).collect();
        assert_eq!(gnd, vec![pushed_b], "the earlier push is committed: {commit:?}");
        assert_eq!(commit.remove_via_ids, vec!["viaA"]);
        assert_eq!(commit.vias.len(), 1, "{commit:?}");
        assert_eq!(Some(commit.vias[0].0), second.pts.first().copied());
    }

    /// `DRAGGER::Drag`'s "Restore last solution" in Shove mode shows the frame the failed position reduced the stack to
    /// (`m_lastNode->GetParent()`), not the one the last good position left: the via's place in the newest frame no longer touches
    /// that frame's world, so the frame is popped, and when the via then cannot get to the cursor it is shown where the frame below
    /// has it, with that frame's push.
    #[test]
    fn a_failed_via_position_shows_the_frame_it_reduced_the_stack_to() {
        let (mut node, _) = via_across_a_gnd_track();
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: p(-2_000_000, 3_000_000), b: p(2_000_000, 3_000_000), width: mm(0.2), source_track: Some(("trkL".into(), 0)), locked: true }));
        let via_id = node.iter().find_map(|(id, it)| matches!(it, Item::Via(_)).then_some(id)).unwrap();
        let mut dragger = Dragger::start(&node, p(0, 0), via_id).expect("a via drag");
        let rules = rules();
        let settings = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let first = dragger.drag(&node, &rules, &settings, p(4_000_000, 0));
        assert!(!first.colliding && !first.restored, "{first:?}");
        let second = dragger.drag(&node, &rules, &settings, p(4_000_000, 1_000_000));
        assert!(!second.colliding && !second.restored, "{second:?}");
        assert_eq!(dragger.shove.borrow().stack.len(), 2, "each move that shoved left a frame");
        let b_of = |pv: &DragPreview| pv.displaced_lines.iter().filter(|d| d.source_track.as_deref() == Some("trkB")).map(|d| d.line.pts.clone()).collect::<Vec<_>>();
        assert_ne!(b_of(&first), b_of(&second), "the second move pushed trkB on");
        // centred on the locked track: neither the shove nor the pushout can place the via
        let failed = dragger.drag(&node, &rules, &settings, p(0, 3_000_000));
        assert!(failed.restored && !dragger.drag_status(), "{failed:?}");
        assert_eq!(dragger.shove.borrow().stack.len(), 1, "the via's place in the newest frame touches nothing of it: popped");
        assert_eq!(failed.pts, first.pts, "the via is where the frame left on top has it: {failed:?}");
        assert_eq!(b_of(&failed), b_of(&first), "with that frame's push: {failed:?}");
        assert_eq!(failed.fanout.len(), 1);
        assert_eq!(failed.fanout[0].pts.last(), first.pts.first(), "its track ends at it: {failed:?}");
        assert!(failed.displaced_lines.iter().all(|d| d.source_track.as_deref() != Some("trkA")), "{failed:?}");
    }

    #[test]
    fn starting_on_a_pad_refuses() {
        use crate::item::Solid;
        let mut node = Node::new();
        let pad_id = node.add(Item::Solid(Solid { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: Point { x: Nm::ZERO, y: Nm::ZERO }, shape: Shape::Circle { c: Point { x: Nm::ZERO, y: Nm::ZERO }, r: mm(0.4) }, source: "U1.1".into(), edge: false, rule: 0 }));
        assert!(Dragger::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, pad_id).is_none());
    }
}
