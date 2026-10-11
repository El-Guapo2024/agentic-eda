//! Port of `PNS::ROUTER` (`pcbnew/router/pns_router.{h,cpp}`): the
//! top-level session object the HTTP API layer
//! (`crates/cli/src/studio.rs`) drives one call at a time -- start a
//! route, preview/fix/undo as the user works, finish or cancel.
//!
//! Scoped down from upstream the same way the rest of this crate is (see
//! `PARITY.md`): single-track routing and single-item dragging only, no
//! diff pairs/meandering/tuning (`PNS_MODE_ROUTE_SINGLE` is the only
//! `ROUTER_MODE` this port implements), and the commit granularity is
//! "whole finished lines" rather than upstream's per-segment `AddItem`/
//! `RemoveItem`/`UpdateItem` diff (this project's own `Track` IR is
//! already a whole polyline -- see [`RouteCommit`]'s own doc comment).
//!
//! `Router` owns the committed [`Node`] (`m_world`) and, while a route is
//! in progress, a [`crate::line_placer::LinePlacer`] session -- there is no
//! persistent working/preview `Node` the way upstream keeps one, because
//! neither `LinePlacer` nor `shove` need one between calls (both re-derive
//! everything fresh from `m_world` plus the session's own state on every
//! call; see those modules' doc comments for why that's enough here).

use crate::item::{net_of, Item, ItemId, Net};
use crate::layer::{LayerMap, LayerRange};
use crate::line::Line;
use crate::line_placer::{FixOutcome, LinePlacer, Preview, EXACT_UM};
use crate::node::Node;
use crate::settings::RoutingSettings;
use eda_model::ir::{Design, Point, Um};
use eda_model::ConstraintModel;
use std::collections::HashSet;

/// `ANCHOR_SNAP_UM`-equivalent used for picking a *start* item -- see
/// `line_placer::SNAP_UM` for the same radius used at the route's end.
const START_SNAP_UM: Um = 500;

/// A track cut in two where a route starts or ends in the middle of one (`SplitAdjacentSegments`): the commit gives the IR track
/// `track` a vertex at `at`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackSplit {
    pub track: String,
    pub at: Point,
}

struct Session {
    placer: LinePlacer,
    /// Set by `toggle_via`: the layer the *next* fix should switch to
    /// (dropping a via there first) -- Router-level orchestration, not
    /// `LinePlacer`'s own concern (it only knows "append a via to the
    /// head," not which layer comes next).
    pending_via_layer: Option<i32>,
    /// The world the session works on when it started in the middle of a track (`initPlacement`: `SplitAdjacentSegments( rootNode,
    /// m_startItem, m_currentStart )`): the router's world with that track cut in two. `None` is the router's world itself.
    world: Option<Node>,
    /// The tracks the session cut at its start, for the commit.
    splits: Vec<TrackSplit>,
}

/// What a route starts from (`TOOL_BASE::updateStartItem`, `snapToItem`).
struct StartPick {
    /// The pad, via or track the route starts on, and the point it starts at.
    item: ItemId,
    pos: Point,
    /// The track whose middle `pos` is: it is split there.
    split: Option<ItemId>,
}

/// What a finished (or shove-touched) route turns into for the IR --
/// `crate::ops::Cmd::CommitRoute`'s payload (the frontend/`studio.rs`
/// seam, not part of this crate). One `Track`/`Via` per fixed run/via this
/// session placed, plus whatever existing tracks/vias a shove displaced
/// along the way (`remove_track_ids`/`remove_via_ids` name the ones that
/// must be deleted before the replacement `tracks`/`vias` are added, all
/// as one undo step).
#[derive(Debug, Clone, Default)]
pub struct RouteCommit {
    pub tracks: Vec<eda_model::ir::Track>,
    pub vias: Vec<eda_model::ir::Via>,
    pub remove_track_ids: Vec<String>,
    pub remove_via_ids: Vec<String>,
}

/// Where "Route From Other End" restarts and what it commits on the way (`ROUTER::ContinueFromEnd`): see
/// [`Router::continue_from_end`].
#[derive(Debug, Clone)]
pub struct Continuation {
    /// What the route has fixed so far, to commit before the new one starts. Empty if nothing was fixed.
    pub commit: RouteCommit,
    pub net: String,
    /// The layer the new route starts on: the current one if the far end is on it, else the far end's own.
    pub layer: String,
    pub width: Um,
    /// The far end of the connection: where the new route starts.
    pub from: Point,
    /// Where the head of the old route ended: the new route heads there.
    pub to: Point,
}

pub struct Router {
    world: Node,
    layers: LayerMap,
    rules: eda_model::BoardRules,
    pub settings: RoutingSettings,
    session: Option<Session>,
    /// Stage 5: `crate::dragger::Dragger`'s own session, mutually
    /// exclusive with `session` the same way upstream's `RouterState`
    /// (`IDLE`/`ROUTE_TRACK`/`DRAG_SEGMENT`) is a single enum, not two
    /// independent flags -- this port just uses two `Option`s instead of
    /// a tri-state, since nothing here ever needs to ask "which one" in
    /// the abstract, only "is a drag in progress" or "is a route."
    drag: Option<crate::dragger::Dragger>,
    /// Gap #7 task item 6: `crate::diff_pair::DiffPairPlacer`'s own
    /// session, mutually exclusive with `session`/`drag` the same way
    /// those two already are with each other.
    diff: Option<crate::diff_pair::DiffPairPlacer>,
    /// The ids of the IR tracks that are arcs (`arc_mid_offset`): the world has them as chords, so a route cannot start or end in the
    /// middle of one (`SplitAdjacentArcs` would split the arc itself).
    arc_tracks: HashSet<String>,
}

impl Router {
    pub fn new(design: &Design, model: &ConstraintModel) -> Self {
        let (world, layers) = crate::from_ir::build_node(design, model);
        let arc_tracks = design.routing.as_ref().map(|r| r.tracks.iter().filter(|t| t.arc_mid_offset.is_some()).map(|t| t.id.clone()).collect()).unwrap_or_default();
        Router { world, layers, rules: model.board.clone(), settings: RoutingSettings::default(), session: None, drag: None, diff: None, arc_tracks }
    }

    pub fn is_routing(&self) -> bool {
        self.session.is_some()
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn is_routing_diff_pair(&self) -> bool {
        self.diff.is_some()
    }

    pub fn layer_index(&self, name: &str) -> Option<i32> {
        self.layers.index_of(name)
    }

    pub fn layer_name(&self, idx: i32) -> &str {
        self.layers.name_of(idx)
    }

    /// The net of the route currently in progress, if any -- the UI's own
    /// "which net am I routing" readout (not consulted by `Router` itself
    /// anywhere; `LinePlacer` already knows its own net for every actual
    /// decision).
    pub fn current_net(&self) -> Option<String> {
        self.session().and_then(|s| s.placer.net.as_deref().map(str::to_string))
    }

    /// `ROUTER::StartRouting`: pick a starting net from whatever's at `at`:
    /// a pad, a via or a track end, or -- in the middle of a track --
    /// the track itself, which is split there (`LINE_PLACER::initPlacement`:
    /// `SplitAdjacentSegments( rootNode, m_startItem, m_currentStart )`; nothing
    /// is committed unless the route places something). This port requires
    /// starting on a connectable item (a real net), unlike upstream, which
    /// also allows starting from bare space onto an orphaned net; starting
    /// mid-air has no obvious net to resolve to in this project's model and
    /// isn't a mode the task asked for. Replaces any route already in
    /// progress (matching `StopRouting()` + fresh `StartRouting()`, not an
    /// error).
    pub fn start(&mut self, at: Point, layer_name: &str, width: Um) -> Result<(), String> {
        let layer = self.layer_index(layer_name).ok_or_else(|| format!("unknown layer {layer_name:?}"))?;
        let Some(pick) = self.pick_start(at, layer) else {
            return Err("no pad, via, or track end there to route from".into());
        };
        let Some(net) = self.world.get(pick.item).map(|i| i.net().clone()).filter(|n| n.is_some()) else {
            return Err("that item has no net to route".into());
        };
        let mut placer = LinePlacer::start(&self.world, pick.pos, Some(pick.item), net, layer, width);
        let (mut world, mut splits) = (None, Vec::new());
        if let Some(seg) = pick.split {
            // the track is cut in two in the session's own copy of the world; it stays whole in the router's
            let mut w = self.world.branch();
            let track = match self.world.get(seg) {
                Some(Item::Segment(s)) => s.source_track.as_ref().map(|(t, _)| t.clone()),
                _ => None,
            };
            if let (Some((a, b)), Some(track)) = (w.split_segment(seg, pick.pos), track) {
                splits.push(TrackSplit { track, at: pick.pos });
                placer.exclude_end.extend([a, b]);
                world = Some(w);
            }
        }
        // `pickSingleItem( .., { m_startItem } )`: the track the route started on is never where it ends
        if let Some(Item::Segment(_)) = self.world.get(pick.item) {
            placer.exclude_end.push(pick.item);
        }
        self.drag = None;
        self.diff = None;
        self.session = Some(Session { placer, pending_via_layer: None, world, splits });
        Ok(())
    }

    /// `TOOL_BASE::updateStartItem` for a point the studio's snapping has already put on what it picked (`PcbSnap.routePoint`): the
    /// pad, via or track end the point is exactly on; else the track whose copper it is in -- at the end nearer to it if that is within
    /// half the track's width (`snapToItem`), else at the point on its centre line, in the middle of the track; else (a point that was
    /// not snapped: a script's, the API's) the nearest pad, via or track end within [`START_SNAP_UM`].
    fn pick_start(&self, at: Point, layer: i32) -> Option<StartPick> {
        let layers = LayerRange::single(layer);
        if let Some((item, pos)) = self.world.nearest_anchor(at, layers, EXACT_UM, None) {
            return Some(StartPick { item, pos, split: None });
        }
        if let Some(id) = self.world.segment_at(at, layer, None, None, &[]) {
            if let Some(Item::Segment(s)) = self.world.get(id) {
                let seg = eda_drc::kimath::Seg::new(s.a, s.b);
                let on = if seg.sq_distance_to_point(at) <= (EXACT_UM * EXACT_UM) as i128 { at } else { seg.nearest_point(at) };
                let half = s.width / 2;
                let (da, db) = (crate::seg_ops::length(at, s.a), crate::seg_ops::length(at, s.b));
                if da < half || db < half {
                    return Some(StartPick { item: id, pos: if da < db { s.a } else { s.b }, split: None });
                }
                let is_arc = s.source_track.as_ref().is_some_and(|(t, _)| self.arc_tracks.contains(t));
                if !is_arc && s.source_track.is_some() {
                    return Some(StartPick { item: id, pos: on, split: Some(id) });
                }
            }
        }
        let (item, pos) = self.world.nearest_anchor(at, layers, START_SNAP_UM, None)?;
        Some(StartPick { item, pos, split: None })
    }

    fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    /// `ROUTER::Move`/`movePlacing`: the live preview. `None` if no route
    /// is in progress.
    pub fn preview(&self, at: Point) -> Option<Preview> {
        self.session().map(|s| s.placer.preview(s.world.as_ref().unwrap_or(&self.world), &self.rules, &self.settings, at))
    }

    /// The `/` posture toggle and `W` width-cycle hook both fold into this
    /// call (applied before the preview is computed) rather than being
    /// separate endpoints, keeping the per-request surface small per the
    /// task's own instruction -- the frontend sends them alongside the
    /// next mouse-move it already has to send.
    pub fn flip_posture(&mut self) {
        if let Some(s) = &mut self.session {
            s.placer.flip_posture();
        }
    }

    pub fn set_width(&mut self, width: Um) {
        if let Some(s) = &mut self.session {
            s.placer.width = width;
        }
    }

    /// `ROUTER::ToggleViaPlacement`: arm (or disarm) "drop a via here" for
    /// the *next* fix. `to_layer`, when arming, is which layer the route
    /// continues on after that via -- the layer-switch half of the task's
    /// "live via placement (V with layer switch)" requirement.
    pub fn toggle_via(&mut self, enabled: bool, diameter: Um, drill: Um, to_layer: Option<i32>) {
        if let Some(s) = &mut self.session {
            s.placer.toggle_via(enabled, diameter, drill);
            s.pending_via_layer = if enabled { to_layer } else { None };
        }
    }

    /// `ROUTER::FixRoute`: commit the current head. If a via was armed
    /// (`toggle_via`), this fixes the head ending in that via and switches
    /// to the pending layer instead of a plain fix -- mirroring the
    /// existing single-segment router's own `dropViaAndSwitchLayer` being
    /// "two immediate commits," just via `LinePlacer::switch_layer`
    /// instead of two separate `Cmd`s.
    pub fn fix(&mut self, at: Point) -> Option<FixOutcome> {
        let Some(s) = &mut self.session else { return None };
        let world = s.world.as_ref().unwrap_or(&self.world);
        if let Some(to_layer) = s.pending_via_layer.take() {
            let (diameter, drill) = (s.placer.via_diameter, s.placer.via_drill);
            s.placer.switch_layer(world, &self.rules, &self.settings, at, to_layer, diameter, drill);
            return Some(FixOutcome::Fixed { real_end: false });
        }
        Some(s.placer.fix(world, &self.rules, &self.settings, at))
    }

    /// `ROUTER::ContinueFromEnd` (Ctrl+E, "Route From Other End"), the half that needs the session: where the route restarts, and
    /// what to commit first. Nothing is changed -- the caller commits [`Continuation::commit`] (`CommitRouting()`: the segments fixed
    /// so far, which is nothing if none were) and starts a fresh session at [`Continuation::from`], layer [`Continuation::layer`],
    /// which then heads for [`Continuation::to`] (`Move( currentEnd )`), the end of the head this session was drawing.
    ///
    /// The other end is `GetNearestRatnestAnchor`'s: with the head drawn, `TOPOLOGY::NearestUnconnectedAnchorPoint( trace )` -- the
    /// head's own end if something is already connected there, else the unconnected pad, via or track of the net nearest to it; with
    /// nothing drawn, `NearestUnconnectedItem` from where the route starts. What the session has fixed counts as connected
    /// (`CurrentNode( true )`), so the far end is the other side of the connection, not the pad the route began at.
    pub fn continue_from_end(&self, at: Point) -> Result<Continuation, String> {
        let s = self.session().ok_or("not routing")?;
        let world = s.world.as_ref().unwrap_or(&self.world);
        let placer = &s.placer;
        let net = placer.net.clone();
        let preview = placer.preview(world, &self.rules, &self.settings, at);
        let head = &preview.head;
        // what the session has fixed, in the world: segments, and a via where two runs on different layers meet
        let mut node = world.branch();
        for run in &placer.runs {
            node.add_line(run, None, false);
        }
        for w in placer.runs.windows(2) {
            if w[0].layer != w[1].layer && w[0].last() == w[1].first() {
                let (l0, l1) = (w[0].layer, w[1].layer);
                node.add(Item::Via(crate::item::Via { net: net.clone(), layers: LayerRange::new(l0, l1), pos: w[0].last().unwrap(), diameter: placer.via_diameter.max(1), drill: placer.via_drill.max(1), source_via: None, locked: false }));
            }
        }
        let current_end = head.last().unwrap_or_else(|| placer.fixed_start());
        let (from, layers) = if head.segment_count() > 0 {
            // `NearestUnconnectedAnchorPoint( aTrack )`: the trace goes into a node of its own, the joint at its end decides
            node.add_line(head, None, false);
            let ends_with_via = preview.via.is_some();
            let links = node.joint_at(current_end, &net).map_or(0, |j| j.links.len());
            if (!ends_with_via && links >= 2) || (ends_with_via && links >= 3) {
                // "we got something connected"
                (current_end, node.joint_at(current_end, &net).map_or(LayerRange::single(placer.current_layer), |j| j.layers))
            } else {
                let (id, anchor) = node.nearest_unconnected_item(current_end, &net).ok_or("Route From Other End: nothing is left to connect to.")?;
                (anchor, node.get(id).map_or(LayerRange::single(placer.current_layer), |i| i.layers()))
            }
        } else {
            let start = placer.fixed_start();
            if node.joint_at(start, &net).is_none() {
                return Err("Route From Other End: this route does not start on a connection.".into());
            }
            let (id, anchor) = node.nearest_unconnected_item(start, &net).ok_or("Route From Other End: nothing is left to connect to.")?;
            (anchor, node.get(id).map_or(LayerRange::single(placer.current_layer), |i| i.layers()))
        };
        let layer = if layers.overlaps_layer(placer.current_layer) { placer.current_layer } else { layers.start() };
        let mut commit = self.build_commit(&net, placer.runs.clone(), placer.current_layer, placer.width);
        if !commit.tracks.is_empty() || !commit.vias.is_empty() {
            let splits = self.session_splits(false);
            self.apply_splits(&splits, &mut commit);
        }
        Ok(Continuation { commit, net: net.as_deref().unwrap_or("").to_string(), layer: self.layer_name(layer).to_string(), width: placer.width, from, to: current_end })
    }

    /// `ROUTER::UndoLastSegment` (Backspace).
    pub fn undo_last_segment(&mut self) -> bool {
        self.session.as_mut().map(|s| s.placer.undo_last_segment()).unwrap_or(false)
    }

    /// `ROUTER::StopRouting()` without committing.
    pub fn cancel(&mut self) {
        self.session = None;
    }

    /// `ROUTER::CommitRouting`: finish the route at `at` and return
    /// everything that needs to land in the IR, or `None` if the final
    /// head still collides and the mode refuses to commit it (the session
    /// stays open so the caller can keep adjusting). Ends the session on
    /// success, same as upstream's `StopRouting()` following a successful
    /// `CommitPlacement()`.
    pub fn finish(&mut self, at: Point) -> Option<RouteCommit> {
        let net_name = self.session()?.placer.net.clone();
        let (width, layer) = {
            let s = self.session()?;
            (s.placer.width, s.placer.current_layer)
        };
        let runs = {
            let s = self.session.as_mut()?;
            let world = s.world.as_ref().unwrap_or(&self.world);
            s.placer.finish(world, &self.rules, &self.settings, at)?
        };
        let mut commit = self.build_commit(&net_name, runs, layer, width);
        // nothing placed, nothing committed (`CommitRouting`: "if( m_state == ROUTE_TRACK && !m_placer->HasPlacedAnything() ) return"),
        // and so no track is cut either
        if !commit.tracks.is_empty() || !commit.vias.is_empty() {
            let splits = self.session_splits(true);
            self.apply_splits(&splits, &mut commit);
        }
        self.session = None;
        Some(commit)
    }

    /// The tracks the session cuts: the one it started in the middle of, and -- once it has ended there -- the one it ended in the
    /// middle of (`Move`: `SplitAdjacentSegments( m_lastNode, aEndItem, splitPoint )`, which does nothing where a joint already is).
    fn session_splits(&self, with_end: bool) -> Vec<TrackSplit> {
        let Some(s) = self.session() else { return Vec::new() };
        let mut splits = s.splits.clone();
        if let (true, Some((seg, at))) = (with_end, s.placer.end_split) {
            let world = s.world.as_ref().unwrap_or(&self.world);
            if let Some(old) = world.segment_to_split(seg, at) {
                if let Some((track, _)) = old.source_track {
                    splits.push(TrackSplit { track, at });
                }
            }
        }
        splits
    }

    /// A track a route starts or ends in the middle of gets a vertex there: it is replaced by itself with the point inserted
    /// (unless a shove moved it, in which case what the shove left stands).
    fn apply_splits(&self, splits: &[TrackSplit], commit: &mut RouteCommit) {
        let mut by_track: std::collections::BTreeMap<&str, Vec<Point>> = std::collections::BTreeMap::new();
        for sp in splits {
            by_track.entry(sp.track.as_str()).or_default().push(sp.at);
        }
        for (track, points) in by_track {
            if commit.remove_track_ids.iter().any(|t| t == track) {
                continue;
            }
            let Some(line) = self.world.track_polyline(track) else { continue };
            let mut pts = line.pts.clone();
            for p in points {
                if pts.contains(&p) {
                    continue;
                }
                if let Some(k) = (0..pts.len() - 1).find(|&k| eda_drc::kimath::Seg::new(pts[k], pts[k + 1]).sq_distance_to_point(p) <= (EXACT_UM * EXACT_UM) as i128) {
                    pts.insert(k + 1, p);
                }
            }
            if pts.len() == line.pts.len() {
                continue;
            }
            commit.remove_track_ids.push(track.to_string());
            commit.tracks.push(eda_model::ir::Track { id: String::new(), net: self.net_name_of(&line), pins: Vec::new(), layer: self.layer_name(line.layer).to_string(), width: line.width, pts, arc_mid_offset: None });
        }
    }

    fn build_commit(&self, net: &Net, runs: Vec<Line>, _layer: i32, _width: Um) -> RouteCommit {
        let mut commit = RouteCommit::default();
        let net_name = net.as_deref().unwrap_or("").to_string();
        for run in &runs {
            if run.point_count() < 2 {
                continue;
            }
            commit.tracks.push(eda_model::ir::Track { id: String::new(), net: net_name.clone(), pins: Vec::new(), layer: self.layer_name(run.layer).to_string(), width: run.width, pts: run.pts.clone(), arc_mid_offset: None });
        }
        // Vias this session placed sit at the shared endpoint between two
        // consecutive runs (see `LinePlacer::switch_layer`'s doc comment);
        // reconstruct them from the run boundaries rather than threading a
        // separate via list through `LinePlacer`, since a run's own first/
        // last point already says exactly where one must be.
        if let Some(s) = self.session() {
            for w in runs.windows(2) {
                // A real via changes layer; a same-layer shared endpoint is
                // just an intermediate fix click (D1).
                if w[0].layer != w[1].layer && w[0].last() == w[1].first() {
                    commit.vias.push(eda_model::ir::Via {
                        id: String::new(),
                        net: net_name.clone(),
                        at: w[0].last().unwrap(),
                        drill: s.placer.via_drill.max(1),
                        diameter: s.placer.via_diameter.max(1),
                        from_layer: self.layer_name(w[0].layer).to_string(),
                        to_layer: self.layer_name(w[1].layer).to_string(),
                    });
                }
            }
            for (track_id, lines) in s.placer.displaced_tracks() {
                commit.remove_track_ids.push(track_id.to_string());
                for line in lines.iter().filter(|l| l.point_count() >= 2) {
                    commit.tracks.push(eda_model::ir::Track { id: String::new(), net: self.net_name_of(line), pins: Vec::new(), layer: self.layer_name(line.layer).to_string(), width: line.width, pts: line.pts.clone(), arc_mid_offset: None });
                }
            }
            for (via_id, pos) in s.placer.displaced_vias() {
                commit.remove_via_ids.push(via_id.to_string());
                if let Some(v) = self.find_via_by_source(via_id) {
                    commit.vias.push(eda_model::ir::Via { id: String::new(), net: v.net.as_deref().unwrap_or("").to_string(), at: pos, drill: v.drill, diameter: v.diameter, from_layer: self.layer_name(v.layers.start()).to_string(), to_layer: self.layer_name(v.layers.end()).to_string() });
                }
            }
        }
        commit
    }

    fn net_name_of(&self, line: &Line) -> String {
        line.net.as_deref().unwrap_or("").to_string()
    }

    fn find_via_by_source(&self, source_id: &str) -> Option<crate::item::Via> {
        self.world.iter().find_map(|(_, it)| match it {
            Item::Via(v) if v.source_via.as_deref() == Some(source_id) => Some(v.clone()),
            _ => None,
        })
    }

    /// Looks up the item at `at` (a hit-test for the UI's own hover/click
    /// resolution, e.g. deciding what `start()` would attach to before the
    /// user actually presses X). Not a literal upstream method name -- see
    /// `node.rs`'s own doc comment on `nearest_anchor`.
    pub fn item_near(&self, at: Point, layer_name: &str, max_dist: Um) -> Option<ItemId> {
        let layer = self.layer_index(layer_name)?;
        self.world.nearest_anchor(at, LayerRange::single(layer), max_dist, None).map(|(id, _)| id)
    }

    // ------------------------------------------------------------- dragging (stage 5)

    /// `ROUTER::StartDragging`: grab whatever track segment/corner or via
    /// is under `at` on `layer_name` (hit-tested by shape, not just anchor
    /// points -- `Node::item_at`, so grabbing the middle of a long track
    /// works same as grabbing its end). Replaces any route *or* drag
    /// already in progress, matching `StopRouting()` + a fresh start.
    pub fn drag_start(&mut self, at: Point, layer_name: &str) -> Result<(), String> {
        self.drag_start_with(at, layer_name, false)
    }

    /// [`Self::drag_start`] with `PNS::DM_FREE_ANGLE` (`G`,
    /// `pcbnew.InteractiveRouter.DragFreeAngle`): see `crate::dragger`'s
    /// "Free-angle mode" doc.
    pub fn drag_start_with(&mut self, at: Point, layer_name: &str, free_angle: bool) -> Result<(), String> {
        let layer = self.layer_index(layer_name).ok_or_else(|| format!("unknown layer {layer_name:?}"))?;
        let item_id = self.world.item_at(at, LayerRange::single(layer), START_SNAP_UM).ok_or("nothing to drag there")?;
        let dragger = crate::dragger::Dragger::start_with(&self.world, at, item_id, free_angle).ok_or("that item can't be dragged (only track segments/corners and vias can)")?;
        self.session = None;
        self.diff = None;
        self.drag = Some(dragger);
        Ok(())
    }

    /// `ROUTER::Move`/`moveDragging`: the live drag preview, `DRAGGER::Drag` at `at`. The drag remembers the last position that
    /// worked: one that does not shows that one again (`DragPreview::restored`).
    pub fn drag_preview(&mut self, at: Point) -> Option<crate::dragger::DragPreview> {
        let (world, rules, settings) = (&self.world, &self.rules, &self.settings);
        self.drag.as_mut().map(|d| d.drag(world, rules, settings, at))
    }

    /// What the drag in progress is (`DM_SEGMENT`, `DM_CORNER` or `DM_VIA`).
    pub fn drag_kind(&self) -> Option<crate::dragger::DragKind> {
        self.drag.as_ref().map(|d| d.kind)
    }

    /// `GetForceMarkObstaclesMode`: the drag in progress only marks obstacles because its first position failed.
    pub fn drag_forces_mark_obstacles(&self) -> bool {
        self.drag.as_ref().is_some_and(|d| d.force_mark_obstacles())
    }

    pub fn drag_cancel(&mut self) {
        self.drag = None;
    }

    /// `DRAGGER::FixRoute( false )`: [`Self::drag_finish_with`] without the Ctrl.
    pub fn drag_finish(&mut self, at: Point) -> Option<RouteCommit> {
        self.drag_finish_with(at, false)
    }

    /// `DRAGGER::FixRoute( aForceCommit )`: commit the drag at `at`, converting its
    /// `DragCommit` into the same [`RouteCommit`] shape a finished route
    /// produces (one `Cmd::CommitRoute` seam for both -- see that type's
    /// own doc comment). A position the drag cannot resolve commits the last one that
    /// worked; `None` if there is nothing to commit -- the drag is only marking obstacles
    /// and `force_commit` (Ctrl+click) is not set -- and the drag session stays
    /// open, so the caller can keep adjusting or explicitly cancel.
    pub fn drag_finish_with(&mut self, at: Point, force_commit: bool) -> Option<RouteCommit> {
        let drag_commit = self.drag.as_mut()?.finish(&self.world, &self.rules, &self.settings, at, force_commit)?;
        let mut commit = RouteCommit { remove_track_ids: drag_commit.remove_track_ids, remove_via_ids: drag_commit.remove_via_ids, ..RouteCommit::default() };
        for line in &drag_commit.tracks {
            if line.point_count() < 2 {
                continue;
            }
            commit.tracks.push(eda_model::ir::Track { id: String::new(), net: line.net.as_deref().unwrap_or("").to_string(), pins: Vec::new(), layer: self.layer_name(line.layer).to_string(), width: line.width, pts: line.pts.clone(), arc_mid_offset: None });
        }
        for (pos, diameter, drill, source_via) in &drag_commit.vias {
            // A dragged via keeps its own net/layer/size; a via displaced
            // by this drag's own shove keeps all of its, found the same
            // way a finished route's displaced vias are
            // (`find_via_by_source`) -- `Dragger` only knows the real
            // diameter/drill of the one via it's directly dragging (a
            // displaced one arrives here with `0, 0` placeholders, see
            // `Dragger::finish`'s own comment), so this prefers the
            // board's own values whenever a lookup succeeds and only
            // falls back to the tuple's own fields for the directly-
            // dragged via on the vanishingly unlikely chance it has no
            // `source_via` at all (can't happen for a real committed via,
            // but keeps this total rather than panicking).
            let via_item = source_via.as_deref().and_then(|s| self.find_via_by_source(s));
            let (net, from_layer, to_layer, resolved_diameter, resolved_drill) = via_item
                .as_ref()
                .map(|v| (v.net.as_deref().unwrap_or("").to_string(), self.layer_name(v.layers.start()).to_string(), self.layer_name(v.layers.end()).to_string(), v.diameter, v.drill))
                .unwrap_or_else(|| ("".to_string(), self.layer_name(0).to_string(), self.layer_name(self.layers.count() as i32 - 1).to_string(), *diameter, *drill));
            commit.vias.push(eda_model::ir::Via { id: String::new(), net, at: *pos, drill: resolved_drill, diameter: resolved_diameter, from_layer, to_layer });
        }
        self.drag = None;
        Some(commit)
    }

    // ------------------------------------------------------------- diff pairs (stage 6)
    //
    // `6` on a net with a recognized differential-pair suffix: route two
    // parallel, gap-matched lines at once (`crate::diff_pair::
    // DiffPairPlacer`). See that module's own doc comment for this port's
    // scope (no shove/walkaround/via-switch for a pair yet -- a direct
    // 45-trace with collision *reporting* only, same contract as
    // `Mode::MarkObstacles`).

    /// `DIFF_PAIR_PLACER::Start` plus upstream's own `FindDpPrimitivePair`:
    /// resolve whatever's at `at` the same way [`Self::start`] does (a
    /// real pad/via/track-end within [`START_SNAP_UM`]), then find its
    /// differential-pair partner net and a real anchor on it already on
    /// the board. Replaces any route/drag/diff-pair session already in
    /// progress.
    pub fn start_diff_pair(&mut self, at: Point, layer_name: &str) -> Result<(), String> {
        let layer = self.layer_index(layer_name).ok_or_else(|| format!("unknown layer {layer_name:?}"))?;
        let hit = self.world.nearest_anchor(at, LayerRange::single(layer), START_SNAP_UM, None);
        let Some((start_item, start_pos)) = hit else {
            return Err("no pad, via, or track end there to route from".into());
        };
        let Some(net) = self.world.get(start_item).map(|i| i.net().clone()).filter(|n| n.is_some()) else {
            return Err("that item has no net to route".into());
        };
        let Some(placer) = crate::diff_pair::DiffPairPlacer::start(&self.world, start_pos, &net, layer, &self.rules) else {
            return Err("not a recognized differential-pair net (needs a +/-/P/N suffix, with a matching pad for the other half already on the board)".into());
        };
        self.session = None;
        self.drag = None;
        self.diff = Some(placer);
        Ok(())
    }

    /// `ROUTER::UpdateSizes` for a pair (the "Differential Pair Dimensions..." dialog,
    /// `ROUTER_TOOL::DpDimensionsDialog`): the running pair's track width and gap. `None` leaves a value as it is. The pair's
    /// own preview and its commit read them, so the next `move` already uses the new values. Returns whether a pair is running.
    pub fn set_diff_pair_dimensions(&mut self, width: Option<Um>, gap: Option<Um>) -> bool {
        let Some(d) = self.diff.as_mut() else { return false };
        if let Some(w) = width.filter(|w| *w > 0) {
            d.width = w;
        }
        if let Some(g) = gap.filter(|g| *g > 0) {
            d.gap = g;
        }
        true
    }

    /// The net names of the diff-pair session currently in progress, if
    /// any -- `(net_a, net_b)`, the UI's own readout (not consulted by
    /// `Router` itself for any decision, same as [`Self::current_net`]).
    pub fn diff_pair_nets(&self) -> Option<(Option<String>, Option<String>)> {
        self.diff.as_ref().map(|d| (d.net_a.as_deref().map(str::to_string), d.net_b.as_deref().map(str::to_string)))
    }

    pub fn diff_pair_preview(&self, at: Point) -> Option<crate::diff_pair::DiffPairPreview> {
        self.diff.as_ref().map(|d| d.preview(&self.world, &self.rules, at))
    }

    pub fn flip_diff_pair_posture(&mut self) {
        if let Some(d) = &mut self.diff {
            d.flip_posture();
        }
    }

    /// `DIFF_PAIR_PLACER::FixRoute` for an intermediate click.
    pub fn fix_diff_pair(&mut self, at: Point) -> Option<FixOutcome> {
        let d = self.diff.as_mut()?;
        Some(match d.fix(&self.world, &self.rules, at) {
            Some(real_end) => FixOutcome::Fixed { real_end },
            None => FixOutcome::Blocked,
        })
    }

    pub fn undo_diff_pair_segment(&mut self) -> bool {
        self.diff.as_mut().map(|d| d.undo_last_segment()).unwrap_or(false)
    }

    pub fn diff_pair_cancel(&mut self) {
        self.diff = None;
    }

    /// `DIFF_PAIR_PLACER::FixRoute` with `aForceFinish`: both lines'
    /// finished runs become ordinary `Track` IR entries on their own
    /// nets, in the *same* [`RouteCommit`] (and so the same
    /// `Cmd::CommitRoute` undo step) a single-track finish produces --
    /// this port's diff pair needed no new commit shape at all, just more
    /// tracks in the existing one.
    pub fn finish_diff_pair(&mut self, at: Point) -> Option<RouteCommit> {
        let (net_a, net_b, layer, width) = {
            let d = self.diff.as_ref()?;
            (d.net_a.clone(), d.net_b.clone(), d.layer, d.width)
        };
        let layer_name = self.layer_name(layer).to_string();
        let (runs_a, runs_b) = self.diff.as_mut()?.finish(&self.world, &self.rules, at)?;
        let mut commit = RouteCommit::default();
        for (net, runs) in [(&net_a, &runs_a), (&net_b, &runs_b)] {
            let net_name = net.as_deref().unwrap_or("").to_string();
            for run in runs {
                if run.point_count() < 2 {
                    continue;
                }
                commit.tracks.push(eda_model::ir::Track { id: String::new(), net: net_name.clone(), pins: Vec::new(), layer: layer_name.clone(), width, pts: run.pts.clone(), arc_mid_offset: None });
            }
        }
        self.diff = None;
        Some(commit)
    }

    pub fn net_of(&self, id: ItemId) -> Option<String> {
        self.world.get(id)?.net().as_deref().map(str::to_string)
    }
}

/// Net lookup helper for callers that only have a name, not an `ItemId` --
/// e.g. resolving a `Cmd`'s own net field the same way the rest of this
/// crate does.
pub fn net(name: &str) -> Net {
    net_of(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{FootprintInstance, LabelSide, PlacementSection, Provenance, RoutingSection, Side};
    use eda_model::{Net as IrNet, Part, Pin, PinKind};

    fn two_pad_board() -> (Design, ConstraintModel) {
        let part = |r: &str| Part { reference: r.into(), mpn: None, lcsc: None, value: None, package: Some("0603".into()), footprint: Some("0603".into()), pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }], body_um: None, symbol: None, datasheet: None, edge: None };
        let model = ConstraintModel { parts: vec![part("R1"), part("R2")], nets: vec![IrNet { name: "SIG".into(), pins: vec!["R1.1".into(), "R2.1".into()] }], ..Default::default() };
        let design = Design {
            footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "test".into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection { outline: vec![], footprints: vec![FootprintInstance { id: "R1".into(), at: Point { x: 0, y: 0 }, rot: 0, side: Side::Top, label: LabelSide::Above }, FootprintInstance { id: "R2".into(), at: Point { x: 6000, y: 0 }, rot: 0, side: Side::Top, label: LabelSide::Above }], modules: vec![] }),
            routing: Some(RoutingSection { tracks: vec![], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }),
            drawings: None,
        };
        (design, model)
    }

    /// One track (SIG, (0,0)-(1000,0)) and one via (GND) placed so that
    /// dragging the track's free end across the board puts it right on
    /// top of the via.
    fn track_and_via_board() -> (Design, ConstraintModel) {
        let model = ConstraintModel::default();
        let design = Design {
            footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "test".into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection { outline: vec![], footprints: vec![], modules: vec![] }),
            routing: Some(RoutingSection {
                tracks: vec![eda_model::ir::Track { id: "trkA".into(), net: "SIG".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }], arc_mid_offset: None }],
                vias: vec![eda_model::ir::Via { id: "viaA".into(), net: "GND".into(), at: Point { x: 5000, y: 0 }, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }],
                zones: vec![],
                track_width_presets: vec![],
                via_presets: vec![],
                teardrop_settings: Default::default(),
            }),
            drawings: None,
        };
        (design, model)
    }

    #[test]
    fn drag_moves_a_track_end_and_commits_it_in_place_of_the_original() {
        let (design, model) = track_and_via_board();
        let mut router = Router::new(&design, &model);
        router.drag_start(Point { x: 1000, y: 0 }, "F.Cu").expect("must grab the track's free end");
        assert!(router.is_dragging());
        let preview = router.drag_preview(Point { x: 1000, y: 2000 }).expect("preview while dragging");
        assert!(!preview.colliding);
        let commit = router.drag_finish(Point { x: 1000, y: 2000 }).expect("collision-free drag must commit");
        assert!(!router.is_dragging());
        assert_eq!(commit.remove_track_ids, vec!["trkA"]);
        assert_eq!(commit.tracks.len(), 1);
        // `dragCorner45`: the dragged end is at the cursor and the track reaches it by 45-degree legs, not one leg at 63 degrees
        assert_eq!(commit.tracks[0].pts, vec![Point { x: 0, y: 0 }, Point { x: 0, y: 1000 }, Point { x: 1000, y: 2000 }]);
    }

    #[test]
    fn shove_mode_drag_displaces_a_via_and_the_commit_carries_its_real_size() {
        let (design, model) = track_and_via_board();
        let mut router = Router::new(&design, &model);
        router.settings.mode = crate::settings::Mode::Shove;
        router.drag_start(Point { x: 1000, y: 0 }, "F.Cu").unwrap();
        // Drag the track's free end straight across the board, through
        // the via's position -- shove must push the via aside.
        let to = Point { x: 10_000, y: 0 };
        let preview = router.drag_preview(to).expect("preview while dragging");
        assert!(!preview.colliding, "shove must clear the via, not just report a collision");
        let commit = router.drag_finish(to).expect("collision-free shoved drag must commit");
        assert!(commit.remove_via_ids.contains(&"viaA".to_string()));
        let moved_via = commit.vias.iter().find(|v| v.at != Point { x: 5000, y: 0 }).expect("the via must have moved");
        assert_eq!(moved_via.diameter, 600, "a shoved via's real diameter must survive, not the 0 placeholder");
        assert_eq!(moved_via.drill, 300);
        assert_eq!(moved_via.net, "GND");
    }

    /// An imported board has one IR track per segment, so a pushed line stands on several. The commit
    /// removes every one of them and puts one polyline in their place -- not the first one replaced by the
    /// whole line while the rest stay behind as copies.
    #[test]
    fn a_pushed_line_on_two_ir_tracks_is_replaced_by_one_track_in_the_commit() {
        let (mut design, model) = two_pad_board();
        let track = |id: &str, a: Point, b: Point| eda_model::ir::Track { id: id.into(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![a, b], arc_mid_offset: None };
        design.routing.as_mut().unwrap().tracks = vec![track("ta", Point { x: 2500, y: -2000 }, Point { x: 2500, y: -500 }), track("tb", Point { x: 2500, y: -500 }, Point { x: 2500, y: 2000 })];
        let mut router = Router::new(&design, &model);
        router.settings.mode = crate::settings::Mode::Shove;
        router.start(Point { x: -825, y: 0 }, "F.Cu", 200).expect("starts on R1.1");
        let preview = router.preview(Point { x: 4500, y: 0 }).expect("a route is in progress");
        assert!(!preview.colliding);
        assert_eq!(preview.displaced_lines.iter().filter(|d| d.line.point_count() >= 2).count(), 1, "one new shape");
        let commit = router.finish(Point { x: 4500, y: 0 }).expect("the pushed route commits");
        let mut removed = commit.remove_track_ids.clone();
        removed.sort();
        assert_eq!(removed, vec!["ta", "tb"], "both tracks the line stood on are removed");
        let gnd: Vec<_> = commit.tracks.iter().filter(|t| t.net == "GND").collect();
        assert_eq!(gnd.len(), 1, "and one polyline replaces them");
        assert_eq!(gnd[0].width, 200);
        assert_eq!(gnd[0].pts.first(), Some(&Point { x: 2500, y: -2000 }));
        assert_eq!(gnd[0].pts.last(), Some(&Point { x: 2500, y: 2000 }));
    }

    /// A SIG track `trkA` (0,0)-(4000,0) on F.Cu, 200 wide, and a SIG via `viaB` at (8000, 3000) to route to.
    fn track_and_a_via_of_the_same_net() -> (Design, ConstraintModel) {
        let (mut design, model) = track_and_via_board();
        let rt = design.routing.as_mut().unwrap();
        rt.tracks = vec![eda_model::ir::Track { id: "trkA".into(), net: "SIG".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 0, y: 0 }, Point { x: 4000, y: 0 }], arc_mid_offset: None }];
        rt.vias = vec![eda_model::ir::Via { id: "viaB".into(), net: "SIG".into(), at: Point { x: 8000, y: 3000 }, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }];
        (design, model)
    }

    /// The design `Cmd::CommitRoute` leaves.
    fn committed(design: &Design, commit: &RouteCommit) -> Design {
        let mut d = design.clone();
        let rt = d.routing.as_mut().unwrap();
        rt.tracks.retain(|t| !commit.remove_track_ids.contains(&t.id));
        rt.vias.retain(|v| !commit.remove_via_ids.contains(&v.id));
        rt.tracks.extend(commit.tracks.iter().cloned());
        rt.vias.extend(commit.vias.iter().cloned());
        d.assign_missing_ids();
        d
    }

    /// `SplitAdjacentSegments` at `Start`: a route that starts in the middle of a track starts there, and the track gets a vertex where it
    /// started once the route is committed -- one track in, two out of the world at that point: the joint is real.
    #[test]
    fn a_route_can_start_in_the_middle_of_a_track() {
        let (design, model) = track_and_a_via_of_the_same_net();
        let mut router = Router::new(&design, &model);
        router.start(Point { x: 2000, y: 0 }, "F.Cu", 200).expect("the middle of trkA is something to route from");
        assert_eq!(router.current_net().as_deref(), Some("SIG"));
        let via = Point { x: 8000, y: 3000 };
        let pv = router.preview(via).expect("a route");
        assert!(!pv.colliding);
        assert_eq!(pv.head.first(), Some(Point { x: 2000, y: 0 }), "the head leaves the middle of the track");
        assert_eq!(pv.head.last(), Some(via));
        assert_eq!(pv.snapped_end, Some(via));
        assert!(matches!(router.fix(via), Some(FixOutcome::Fixed { real_end: true })));
        let commit = router.finish(via).expect("the route commits");
        // the track is replaced by itself with a vertex at the start
        assert_eq!(commit.remove_track_ids, vec!["trkA"]);
        let split = commit.tracks.iter().find(|t| t.pts.first() == Some(&Point { x: 0, y: 0 })).expect("the cut track");
        assert_eq!(split.pts, vec![Point { x: 0, y: 0 }, Point { x: 2000, y: 0 }, Point { x: 4000, y: 0 }]);
        assert_eq!((split.net.as_str(), split.layer.as_str(), split.width), ("SIG", "F.Cu", 200));
        let route = commit.tracks.iter().find(|t| t.pts.first() == Some(&Point { x: 2000, y: 0 })).expect("the route");
        assert_eq!(route.pts.last(), Some(&via));
        // and in the board that results, the new route and both halves of the track meet at one joint
        let (after, _) = crate::from_ir::build_node(&committed(&design, &commit), &model);
        let joint = after.joint_at(Point { x: 2000, y: 0 }, &net_of("SIG")).expect("a joint where the route starts");
        assert_eq!(joint.links.len(), 3, "the two halves of the track and the route");
    }

    /// A route that starts and places nothing leaves the track whole (`CommitRouting` commits nothing).
    #[test]
    fn a_route_that_places_nothing_does_not_cut_the_track() {
        let (design, model) = track_and_a_via_of_the_same_net();
        let mut router = Router::new(&design, &model);
        router.start(Point { x: 2000, y: 0 }, "F.Cu", 200).unwrap();
        let commit = router.finish(Point { x: 2000, y: 0 }).expect("nothing to refuse");
        assert!(commit.tracks.is_empty() && commit.remove_track_ids.is_empty(), "{commit:?}");
        // cancelled: the router's own world was never cut
        let mut again = Router::new(&design, &model);
        again.start(Point { x: 2000, y: 0 }, "F.Cu", 200).unwrap();
        again.cancel();
        assert!(again.world.iter().filter(|(_, it)| matches!(it, Item::Segment(_))).count() == 1, "the world still has the one segment");
    }

    /// `snapToItem`: within half the track's width of an end it is the end, not the middle.
    #[test]
    fn near_a_tracks_end_the_route_starts_at_the_end_and_cuts_nothing() {
        let (design, model) = track_and_a_via_of_the_same_net();
        let mut router = Router::new(&design, &model);
        router.start(Point { x: 3950, y: 20 }, "F.Cu", 200).expect("the end");
        let pv = router.preview(Point { x: 8000, y: 3000 }).unwrap();
        assert_eq!(pv.head.first(), Some(Point { x: 4000, y: 0 }));
        let commit = router.finish(Point { x: 8000, y: 3000 }).unwrap();
        assert!(!commit.remove_track_ids.contains(&"trkA".to_string()), "the track is not touched: {commit:?}");
    }

    /// A point on the track that the studio's snapping put there exactly wins over a pad within the generous 500 um; one that was not
    /// snapped near a pad still goes to the pad.
    #[test]
    fn a_start_on_the_tracks_centre_line_beats_a_pad_beside_it_and_an_unsnapped_one_still_snaps() {
        let (mut design, model) = track_and_a_via_of_the_same_net();
        design.routing.as_mut().unwrap().vias.push(eda_model::ir::Via { id: "near".into(), net: "SIG".into(), at: Point { x: 2200, y: 300 }, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        let mut router = Router::new(&design, &model);
        router.start(Point { x: 2000, y: 0 }, "F.Cu", 200).unwrap();
        assert_eq!(router.preview(Point { x: 8000, y: 3000 }).unwrap().head.first(), Some(Point { x: 2000, y: 0 }), "on the centre line: the track");
        // 300 um above the track and 100 um from the via: no track under it, the via is within the snap
        router.start(Point { x: 2100, y: 300 }, "F.Cu", 200).unwrap();
        assert_eq!(router.preview(Point { x: 8000, y: 3000 }).unwrap().head.first(), Some(Point { x: 2200, y: 300 }), "the via's centre");
    }

    /// A route ends in the middle of a same-net track the cursor is on (`snap_to_tracks`), and the track is cut there.
    #[test]
    fn a_route_can_end_in_the_middle_of_a_track() {
        let (design, model) = track_and_a_via_of_the_same_net();
        let mut router = Router::new(&design, &model);
        router.start(Point { x: 8000, y: 3000 }, "B.Cu", 200).expect("starts on the via");
        let middle = Point { x: 3000, y: 0 };
        let pv = router.preview(middle).unwrap();
        // the via spans both layers, the track is on F.Cu: the route is on B.Cu and cannot end on a F.Cu track
        assert_eq!(pv.snapped_end, None, "a track on another layer is not an end");
        router.start(Point { x: 8000, y: 3000 }, "F.Cu", 200).unwrap();
        let pv = router.preview(middle).unwrap();
        assert!(!pv.colliding);
        assert_eq!(pv.snapped_end, Some(middle));
        assert!(pv.snapped_segment.is_some());
        assert_eq!(pv.head.last(), Some(middle));
        assert!(matches!(router.fix(middle), Some(FixOutcome::Fixed { real_end: true })), "the click ends the connection");
        let commit = router.finish(middle).expect("commits");
        assert_eq!(commit.remove_track_ids, vec!["trkA"]);
        let cut = commit.tracks.iter().find(|t| t.pts.first() == Some(&Point { x: 0, y: 0 })).expect("the cut track");
        assert_eq!(cut.pts, vec![Point { x: 0, y: 0 }, middle, Point { x: 4000, y: 0 }]);
        let (after, _) = crate::from_ir::build_node(&committed(&design, &commit), &model);
        assert_eq!(after.joint_at(middle, &net_of("SIG")).map(|j| j.links.len()), Some(3));
    }

    /// With "snap to tracks" off the cursor over a track is no end, and nothing is cut.
    #[test]
    fn without_snap_to_tracks_a_route_does_not_end_on_a_track() {
        let (design, model) = track_and_a_via_of_the_same_net();
        let mut router = Router::new(&design, &model);
        router.settings.snap_to_tracks = false;
        router.start(Point { x: 8000, y: 3000 }, "F.Cu", 200).unwrap();
        let pv = router.preview(Point { x: 3000, y: 0 }).unwrap();
        assert_eq!((pv.snapped_end, pv.snapped_segment), (None, None));
    }

    /// A route that comes in along the track it ends on ends where it first touches it (`Move`: `lastSeg.Collinear( targetSeg ) &&
    /// targetSeg.Overlaps( lastSeg )`), instead of running along it to the cursor.
    #[test]
    fn a_route_that_runs_along_the_track_it_ends_on_ends_where_it_meets_it() {
        let (mut design, model) = track_and_a_via_of_the_same_net();
        design.routing.as_mut().unwrap().vias[0].at = Point { x: 8000, y: 0 };
        let mut router = Router::new(&design, &model);
        router.start(Point { x: 8000, y: 0 }, "F.Cu", 200).unwrap();
        // the cursor is on the track, 1 mm in from its end, with the route coming from the right along the same line
        let pv = router.preview(Point { x: 3000, y: 0 }).unwrap();
        assert_eq!(pv.head.last(), Some(Point { x: 4000, y: 0 }), "it ends at the track's end, not 1 mm along it: {:?}", pv.head.pts);
        assert_eq!(pv.snapped_end, Some(Point { x: 4000, y: 0 }));
    }

    /// `ROUTER::ContinueFromEnd`: from the other pad of the connection -- before anything is fixed, and after the first fix, when
    /// what was fixed is committed and the far end is the other side of it.
    #[test]
    fn route_from_other_end_restarts_at_the_far_pad_and_commits_what_is_fixed() {
        let (design, model) = two_pad_board();
        let (r1, r2) = (Point { x: -825, y: 0 }, Point { x: 5175, y: 0 });
        let mut router = Router::new(&design, &model);
        router.start(r1, "F.Cu", 200).unwrap();
        // nothing fixed yet: the connection's other end is R2.1, and the route heads for where the old head ended
        let cursor = Point { x: 3000, y: 800 };
        let head_end = |router: &Router, at: Point| router.preview(at).unwrap().head.last().unwrap();
        let first = router.continue_from_end(cursor).expect("R2.1 is unconnected");
        assert_eq!((first.from, first.to, first.layer.as_str(), first.net.as_str(), first.width), (r2, head_end(&router, cursor), "F.Cu", "SIG", 200));
        assert!(first.commit.tracks.is_empty() && first.commit.remove_track_ids.is_empty(), "nothing was placed, so nothing is committed");
        assert!(router.is_routing(), "the session is untouched: the caller replaces it");
        // fix the first leg: R1.1 is connected to the fixed end now, and the far end is still R2.1
        let fixed_at = Point { x: 2000, y: 800 };
        assert!(matches!(router.fix(fixed_at), Some(FixOutcome::Fixed { real_end: false })));
        let cursor = Point { x: 3500, y: 1500 };
        let second = router.continue_from_end(cursor).expect("R2.1 is still unconnected");
        assert_eq!(second.from, r2);
        assert_eq!(second.to, head_end(&router, cursor), "the head ended at the cursor");
        assert_eq!(second.commit.tracks.len(), 1, "the fixed leg is committed");
        assert_eq!((second.commit.tracks[0].pts.first().copied(), second.commit.tracks[0].pts.last().copied()), (Some(r1), Some(fixed_at)));
        // with the cursor on the pad the head is already connected: the route restarts there
        let on_the_pad = router.continue_from_end(r2).expect("connected end");
        assert_eq!((on_the_pad.from, on_the_pad.to), (r2, r2));
        // once the committed leg is on the board, a new session from the far end heads for the old head's end
        let board_after = committed(&design, &second.commit);
        let mut restart = Router::new(&board_after, &model);
        restart.start(second.from, &second.layer, second.width).expect("starts on R2.1");
        let pv = restart.preview(second.to).unwrap();
        assert!(!pv.colliding && pv.head.first() == Some(r2) && pv.head.last() == Some(second.to), "{pv:?}");
    }

    /// With the whole net connected there is nothing to continue to.
    #[test]
    fn route_from_other_end_with_nothing_left_unconnected_says_so() {
        let (mut design, model) = two_pad_board();
        design.routing.as_mut().unwrap().tracks.push(eda_model::ir::Track { id: "t".into(), net: "SIG".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![Point { x: -825, y: 0 }, Point { x: 5175, y: 0 }], arc_mid_offset: None });
        let mut router = Router::new(&design, &model);
        router.start(Point { x: -825, y: 0 }, "F.Cu", 200).unwrap();
        let err = router.continue_from_end(Point { x: 1000, y: 0 }).expect_err("everything is connected");
        assert!(err.contains("nothing is left"), "{err}");
    }

    #[test]
    fn start_fails_off_any_item() {
        let (design, model) = two_pad_board();
        let mut router = Router::new(&design, &model);
        assert!(router.start(Point { x: 50_000, y: 50_000 }, "F.Cu", 200).is_err());
        assert!(!router.is_routing());
    }

    #[test]
    fn full_session_start_move_fix_finish_produces_a_track() {
        let (design, model) = two_pad_board();
        let mut router = Router::new(&design, &model);
        // R1 pin 1 local pos for a two_pad 0603 footprint is (-825, 0); R2 is at board (6000,0) so its pin 1 is at (6000-825,0)=(5175,0).
        let start_pos = Point { x: -825, y: 0 };
        router.start(start_pos, "F.Cu", 200).expect("must start on R1.1");
        assert!(router.is_routing());
        let pv = router.preview(Point { x: 5175, y: 0 }).expect("preview while routing");
        assert!(!pv.colliding);
        let commit = router.finish(Point { x: 5175, y: 0 }).expect("must finish onto R2.1");
        assert!(!router.is_routing());
        assert_eq!(commit.tracks.len(), 1);
        assert_eq!(commit.tracks[0].net, "SIG");
        assert_eq!(commit.tracks[0].layer, "F.Cu");
        assert_eq!(commit.tracks[0].pts.first(), Some(&start_pos));
        assert_eq!(commit.tracks[0].pts.last(), Some(&Point { x: 5175, y: 0 }));
    }

    /// D1: intermediate fix clicks must not emit vias.
    #[test]
    fn multi_click_route_emits_no_fake_vias() {
        let (design, model) = two_pad_board();
        let mut router = Router::new(&design, &model);
        router.start(Point { x: -825, y: 0 }, "F.Cu", 200).unwrap();
        router.fix(Point { x: 2000, y: 0 });
        router.fix(Point { x: 2000, y: 1000 });
        let commit = router.finish(Point { x: 5175, y: 0 }).expect("must finish");
        assert!(commit.tracks.len() >= 3);
        assert!(commit.vias.is_empty(), "got vias: {:?}", commit.vias);
    }

    #[test]
    fn cancel_drops_the_session_without_a_commit() {
        let (design, model) = two_pad_board();
        let mut router = Router::new(&design, &model);
        router.start(Point { x: -825, y: 0 }, "F.Cu", 200).unwrap();
        router.cancel();
        assert!(!router.is_routing());
        assert!(router.finish(Point { x: 5175, y: 0 }).is_none());
    }

    #[test]
    fn diff_pair_dimensions_change_the_running_pair() {
        let (design, mut model) = two_pad_board();
        model.nets = vec![IrNet { name: "USB_DP".into(), pins: vec!["R1.1".into(), "R2.1".into()] }, IrNet { name: "USB_DN".into(), pins: vec!["R1.2".into(), "R2.2".into()] }];
        let mut router = Router::new(&design, &model);
        assert!(!router.set_diff_pair_dimensions(Some(200), Some(300)), "no pair is running yet");
        router.start_diff_pair(Point { x: -825, y: 0 }, "F.Cu").expect("R1.1 and R1.2 are a pair");
        let at = Point { x: 3000, y: 0 };
        let before = router.diff_pair_preview(at).expect("preview");
        assert_eq!(before.width, 125, "the net class's own width until the dialog says otherwise");
        let spacing = |p: &crate::diff_pair::DiffPairPreview| (p.head_a.pts.last().unwrap().y - p.head_b.pts.last().unwrap().y).abs();
        // The two lines sit `(width + gap) / 2` either side of the spine (`half_total`, integer division).
        let apart = |w: i64, g: i64| 2 * ((w + g) / 2);
        assert_eq!(spacing(&before), apart(125, 180));
        assert!(router.set_diff_pair_dimensions(Some(200), Some(300)));
        let after = router.diff_pair_preview(at).expect("preview");
        assert_eq!(after.width, 200);
        assert_eq!(spacing(&after), apart(200, 300), "the lines sit width + gap apart");
        // Zero or negative values are ignored (the dialog refuses a gap of 0).
        assert!(router.set_diff_pair_dimensions(Some(0), Some(-5)));
        assert_eq!(router.diff_pair_preview(at).unwrap().width, 200);
        // `None` leaves a value as it is.
        router.set_diff_pair_dimensions(None, Some(400));
        assert_eq!(spacing(&router.diff_pair_preview(at).unwrap()), apart(200, 400));
    }

    #[test]
    fn undo_last_segment_on_a_fresh_session_is_a_safe_no_op() {
        let (design, model) = two_pad_board();
        let mut router = Router::new(&design, &model);
        router.start(Point { x: -825, y: 0 }, "F.Cu", 200).unwrap();
        assert!(!router.undo_last_segment());
    }
}
