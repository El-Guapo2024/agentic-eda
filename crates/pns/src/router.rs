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
//! in progress, a [`crate::line_placer::LinePlacer`] session. The placer keeps
//! the shove's springback stack ([`crate::springback`]) as diffs against
//! `m_world`, not the `NODE`s upstream keeps; each call makes the world it
//! needs from `m_world` and the newest frame.

use crate::arcs::ArcPts;
use crate::item::{net_of, Item, ItemId, Net};
use crate::layer::{LayerMap, LayerRange};
use crate::line::{Line, LinePiece};
use crate::line_placer::{FixOutcome, LinePlacer, Preview, EXACT_NM};
use crate::node::Node;
use crate::settings::RoutingSettings;
use eda_model::ir::{Design, Point, Nm};
use eda_model::via_extra::ViaKind;
use eda_model::ConstraintModel;
use std::collections::HashMap;

/// `ANCHOR_SNAP_NM`-equivalent used for picking a *start* item -- see
/// `line_placer::SNAP_NM` for the same radius used at the route's end.
const START_SNAP_NM: Nm = Nm::new(500_000);

/// A track cut in two where a route starts or ends in the middle of one (`SplitAdjacentSegments`, `SplitAdjacentArcs`): the commit gives the IR
/// track `track` a vertex at `at`, or, for an arc track, cuts it into two arc tracks there.
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
    /// The type of the via armed with [`Router::toggle_via_typed`] (`SIZES_SETTINGS::ViaType()`), for the next fix.
    pending_via_kind: ViaKind,
    /// The type of each via the session placed (`VIA::ViaType()`, which `PNS_KICAD_IFACE::AddItem` gives the board via: "`via_board->SetViaType(
    /// via->ViaType() )`"), one per [`LinePlacer::fixed_vias`], in the same order (Backspace takes both away together).
    placed_vias: Vec<PlacedVia>,
}

/// A via a fix of the session placed: see [`Session::placed_vias`].
#[derive(Debug, Clone, Copy)]
struct PlacedVia {
    kind: ViaKind,
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
    /// The indices into `tracks` of the ones that are locked: the halves of a locked track a route started or ended in the middle of.
    pub lock_tracks: Vec<usize>,
    /// The vias of `vias` that are not through vias, by index, and their type: the blind/buried and micro vias the route placed
    /// (`ROUTER_TOOL::handleLayerSwitch`'s `sizes.SetViaType( viaType )`), and a displaced via keeps the type it had.
    pub via_kinds: Vec<(usize, ViaKind)>,
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
    pub width: Nm,
    /// The far end of the connection: where the new route starts.
    pub from: Point,
    /// Where the head of the old route ended: the new route heads there.
    pub to: Point,
    /// A pair's gap and via gap (`UpdateSizes`: the sizes the router keeps survive the restart); 0 for a single track.
    pub gap: Nm,
    pub via_gap: Nm,
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
    /// Gap #7 task item 6: `crate::dp_placer::DiffPairPlacer`'s own
    /// session, mutually exclusive with `session`/`drag` the same way
    /// those two already are with each other.
    diff: Option<crate::dp_placer::DiffPairPlacer>,
    /// This session restarted from the other end of a route whose fixed part was committed (`ROUTER::ContinueFromEnd`): what it commits
    /// is one undo step with that earlier commit (`APPEND_UNDO`). The caller that applies the commits keeps it.
    pub append_undo: bool,
    /// The type of every via of the board that is not a through via, by id (`DrawingsSection::via_extras`): a via a shove or a drag moves
    /// is removed and put back, and keeps its type (`PNS_KICAD_IFACE::syncVia` reads `aVia->GetViaType()` into the router's via).
    via_kinds: HashMap<String, ViaKind>,
    /// The `pins` of every board track that has some, by id: a track [`Self::simplify_new_lines`] takes into a new one hands them on.
    track_pins: HashMap<String, Vec<String>>,
    /// The mode the drag in progress started in (`DRAGGER::Start`: `m_currentMode = Settings().Mode()`, and a shove made only for Shove): a
    /// mode changed in the middle of a drag waits for the next one (findings p5 F3-12). The other settings are read live, as KiCad's are.
    drag_mode: Option<crate::settings::Mode>,
}

impl Router {
    pub fn new(design: &Design, model: &ConstraintModel) -> Self {
        let (world, layers) = crate::from_ir::build_node(design, model);
        let via_kinds = design.drawings.iter().flat_map(|d| d.via_extras.iter()).filter(|e| e.kind != ViaKind::Through).map(|e| (e.id.clone(), e.kind)).collect();
        let track_pins = design.routing.iter().flat_map(|r| r.tracks.iter()).filter(|t| !t.pins.is_empty()).map(|t| (t.id.clone(), t.pins.clone())).collect();
        Router { world, layers, rules: model.board.clone(), settings: RoutingSettings::default(), session: None, drag: None, diff: None, append_undo: false, via_kinds, track_pins, drag_mode: None }
    }

    /// The type of the board's via `id` (a through via unless the board says otherwise).
    fn kind_of_via(&self, id: Option<&str>) -> ViaKind {
        id.and_then(|id| self.via_kinds.get(id)).copied().unwrap_or_default()
    }

    /// Records the type of the via just pushed onto `commit.vias`, if it is not a through via.
    fn note_via_kind(commit: &mut RouteCommit, kind: ViaKind) {
        if kind != ViaKind::Through && !commit.vias.is_empty() {
            commit.via_kinds.push((commit.vias.len() - 1, kind));
        }
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
    /// is committed unless the route places something). With nothing there the
    /// route starts in empty space on no net (`LINE_PLACER::Start`: `m_currentNet =
    /// aStartItem ? aStartItem->Net() : GetOrphanedNetHandle()`), and commits
    /// tracks on no net, as KiCad's recorded `simple-shove-1` session does.
    /// An item of no net starts a route of no net (KiCad's no-net items share
    /// `OrphanedItem()`, [`crate::item::same_net`]); a non-plated hole is
    /// refused ("Cannot start routing from a non-plated hole.").
    /// `ROUTER::isStartingPointRoutable` first, unless
    /// violations are allowed: a start whose dot of track collides at the
    /// route's width and at the board's minimum width is refused ("The routing
    /// start point violates DRC.", findings p5 F2-8). Replaces any route
    /// already in progress (matching `StopRouting()` + fresh `StartRouting()`,
    /// not an error).
    pub fn start(&mut self, at: Point, layer_name: &str, width: Nm) -> Result<(), String> {
        let layer = self.layer_index(layer_name).ok_or_else(|| format!("unknown layer {layer_name:?}"))?;
        let pick = self.pick_start(at, layer);
        if let Some(Item::Solid(s)) = pick.as_ref().and_then(|p| self.world.get(p.item)) {
            if self.world.is_npth(&s.source) {
                return Err("Cannot start routing from a non-plated hole.".into());
            }
        }
        // the item's net, or none (`OrphanedItem()`: every item of no net is that one net, `same_net`), and none in empty space
        let net = pick.as_ref().and_then(|p| self.world.get(p.item)).and_then(|i| i.net().clone());
        let start_pos = pick.as_ref().map_or(at, |p| p.pos);
        if !self.settings.allow_drc_violations() {
            // `isStartingPointRoutable`: "If the only reason we collide is track width; it's better to allow the user to start anyway and just
            // highlight the resulting collisions, so they can change width later."
            let collides = |w: Nm| self.world.first_colliding(&eda_drc::kimath::Shape::Stadium { a: start_pos, b: start_pos, r: w / 2 }, &net, LayerRange::single(layer), &self.rules, &[]).is_some();
            if collides(width) && collides(self.rules.track_width_min_nm) {
                return Err("The routing start point violates DRC.".into());
            }
        }
        let mut placer = LinePlacer::start(&self.world, start_pos, pick.as_ref().map(|p| p.item), net, layer, width);
        let Some(pick) = pick else {
            self.drag = None;
            self.diff = None;
            self.session = Some(Session { placer, pending_via_layer: None, world: None, splits: Vec::new(), pending_via_kind: ViaKind::Through, placed_vias: Vec::new() });
            return Ok(());
        };
        let (mut world, mut splits) = (None, Vec::new());
        if let Some(seg) = pick.split {
            // the track is cut in two in the session's own copy of the world; it stays whole in the router's
            let mut w = self.world.branch();
            let track = self.world.get(seg).and_then(|it| it.source_track_id()).map(str::to_string);
            // `SplitAdjacentSegments`, or `SplitAdjacentArcs` for an arc track
            let halves = if matches!(self.world.get(seg), Some(Item::Arc(_))) { w.split_arc(seg, pick.pos) } else { w.split_segment(seg, pick.pos) };
            if let (Some((a, b)), Some(track)) = (halves, track) {
                splits.push(TrackSplit { track, at: pick.pos });
                placer.exclude_end.extend([a, b]);
                world = Some(w);
            }
        }
        // `pickSingleItem( .., { m_startItem } )`: the track the route started on is never where it ends
        if self.world.get(pick.item).is_some_and(Item::is_track) {
            placer.exclude_end.push(pick.item);
        }
        self.drag = None;
        self.diff = None;
        self.session = Some(Session { placer, pending_via_layer: None, world, splits, pending_via_kind: ViaKind::Through, placed_vias: Vec::new() });
        Ok(())
    }

    /// `TOOL_BASE::updateStartItem` for a point the studio's snapping has already put on what it picked (`PcbSnap.routePoint`): the
    /// pad, via or track end the point is exactly on; else the track whose copper it is in -- at the end nearer to it if that is within
    /// half the track's width (`snapToItem`), else at the point on its centre line, in the middle of the track; else (a point that was
    /// not snapped: a script's, the API's) the nearest pad, via or track end within [`START_SNAP_NM`].
    fn pick_start(&self, at: Point, layer: i32) -> Option<StartPick> {
        let layers = LayerRange::single(layer);
        if let Some((item, pos)) = self.world.nearest_anchor(at, layers, EXACT_NM, None) {
            return Some(StartPick { item, pos, split: None });
        }
        if let Some(id) = self.world.segment_at(at, layer, None, None, &[]) {
            match self.world.get(id) {
                Some(Item::Segment(s)) => {
                    let seg = eda_drc::kimath::Seg::new(s.a, s.b);
                    let on = if seg.sq_distance_to_point(at) <= EXACT_NM.area(EXACT_NM) { at } else { seg.nearest_point(at) };
                    let half = s.width / 2;
                    let (da, db) = (crate::seg_ops::length(at, s.a), crate::seg_ops::length(at, s.b));
                    if da < half || db < half {
                        return Some(StartPick { item: id, pos: if da < db { s.a } else { s.b }, split: None });
                    }
                    if s.source_track.is_some() {
                        return Some(StartPick { item: id, pos: on, split: Some(id) });
                    }
                }
                Some(Item::Arc(a)) => {
                    // `snapToItem` for `ARC_T`: within half the width of an end, that end; else the point of the arc nearest the cursor
                    // (`AlignToArc`), where `SplitAdjacentArcs` cuts it
                    let half = a.width / 2;
                    let (da, db) = (crate::seg_ops::length(at, a.start()), crate::seg_ops::length(at, a.end()));
                    if da < half || db < half {
                        return Some(StartPick { item: id, pos: if da < db { a.start() } else { a.end() }, split: None });
                    }
                    if a.source_track.is_some() {
                        let on = if a.arc.distance_to(at) <= EXACT_NM.as_f64() { at } else { a.arc.nearest_point(at) };
                        return Some(StartPick { item: id, pos: on, split: Some(id) });
                    }
                }
                _ => {}
            }
        }
        let (item, pos) = self.world.nearest_anchor(at, layers, START_SNAP_NM, None)?;
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

    pub fn set_width(&mut self, width: Nm) {
        if let Some(s) = &mut self.session {
            s.placer.width = width;
        }
    }

    /// `ROUTER::ToggleViaPlacement`: arm (or disarm) "drop a via here" for
    /// the *next* fix. `to_layer`, when arming, is which layer the route
    /// continues on after that via -- the layer-switch half of the task's
    /// "live via placement (V with layer switch)" requirement.
    pub fn toggle_via(&mut self, enabled: bool, diameter: Nm, drill: Nm, to_layer: Option<i32>) {
        self.toggle_via_typed(enabled, diameter, drill, to_layer, None, ViaKind::Through);
    }

    /// [`Self::toggle_via`] with the via's type and its own layers, as `ROUTER_TOOL::handleLayerSwitch` arms one: `sizes.SetViaType( viaType )`,
    /// `sizes.AddLayerPair( currentLayer, targetLayer )`, then `ToggleViaPlacement()`. `span` is the layers the via spans (`LINE_PLACER::makeVia`:
    /// `F_Cu` .. `B_Cu` for a through via, the pair for any other); `None` is from the layer being routed to `to_layer`. The via commits with
    /// `kind` (`RouteCommit::via_kinds`).
    pub fn toggle_via_typed(&mut self, enabled: bool, diameter: Nm, drill: Nm, to_layer: Option<i32>, span: Option<(i32, i32)>, kind: ViaKind) {
        if let Some(s) = &mut self.session {
            s.placer.toggle_via(enabled, diameter, drill);
            // the via spans from the layer being routed to the one the route goes on to (`makeVia`): the shove and the collision check see both
            s.placer.via_to_layer = if enabled { to_layer } else { None };
            s.placer.via_span = if enabled { span.map(|(a, b)| LayerRange::new(a, b)) } else { None };
            s.pending_via_layer = if enabled { to_layer } else { None };
            s.pending_via_kind = if enabled { kind } else { ViaKind::Through };
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
            let kind = std::mem::take(&mut s.pending_via_kind);
            s.placed_vias.push(PlacedVia { kind });
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
        let placer = &s.placer;
        let net = placer.net.clone();
        let (from, layers, current_end) = self.nearest_ratsnest_anchor(at)?;
        let runs = placer.placed_runs();
        let layer = if layers.overlaps_layer(placer.current_layer) { placer.current_layer } else { layers.start() };
        let mut commit = self.build_commit(&net, runs, placer.current_layer, placer.width);
        if !commit.tracks.is_empty() || !commit.vias.is_empty() {
            let splits = self.session_splits(false);
            self.apply_splits(&splits, &mut commit);
        }
        self.drop_redundant(&mut commit);
        self.simplify_new_lines(&mut commit);
        Ok(Continuation { commit, net: net.as_deref().unwrap_or("").to_string(), layer: self.layer_name(layer).to_string(), width: placer.width, from, to: current_end, gap: Nm::ZERO, via_gap: Nm::ZERO })
    }

    /// `ROUTER::GetNearestRatnestAnchor` for the head a `Move` to `at` gives: the far end of the connection and its layers, and where the
    /// head ends (`placer->CurrentEnd()`). See [`Self::continue_from_end`].
    fn nearest_ratsnest_anchor(&self, at: Point) -> Result<(Point, LayerRange, Point), String> {
        let s = self.session().ok_or("not routing")?;
        let world = s.world.as_ref().unwrap_or(&self.world);
        let placer = &s.placer;
        let net = placer.net.clone();
        let preview = placer.preview(world, &self.rules, &self.settings, at);
        let head = &preview.head;
        // what the session has fixed, in the world: segments, and a via where two runs on different layers meet
        let mut node = world.branch();
        let runs = placer.placed_runs();
        for run in &runs {
            node.add_line(run, None, false);
        }
        for (v, _) in &placer.fixed_vias {
            node.add(Item::Via(crate::item::Via { diameter: v.diameter.max(Nm::new(1)), drill: v.drill.max(Nm::new(1)), ..v.clone() }));
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
        Ok((from, layers, current_end))
    }

    /// `ROUTER::Finish` (AttemptFinish, `F`): route to the nearest unconnected anchor of the net (`GetNearestRatnestAnchor`), moving the head
    /// there until its end stops changing (at most five `Move`s), and when it gets there on a layer of the anchor, fix it there (`FixRoute(
    /// otherEnd, otherEndItem, false, false )`) and finish. `Ok` with the commit when it did; `Err` with where the head ended otherwise --
    /// the route goes on, and the tool warps the cursor there (`WarpMouseCursor( m_router->Placer()->CurrentEnd() )`). (findings p5 F2-9)
    pub fn attempt_finish(&mut self, at: Point) -> Result<RouteCommit, Point> {
        let fallback = self.preview(at).and_then(|pv| pv.head.last()).unwrap_or(at);
        let Ok((other_end, layers, mut current_end)) = self.nearest_ratsnest_anchor(at) else { return Err(fallback) };
        let mut tries = 5;
        let mut move_result;
        loop {
            move_result = current_end;
            let Some(pv) = self.preview(other_end) else { return Err(fallback) };
            current_end = pv.head.last().unwrap_or(move_result);
            tries -= 1;
            if current_end == move_result || tries == 0 {
                break;
            }
        }
        let layer = self.session().map_or(0, |s| s.placer.current_layer);
        if move_result == other_end && layers.overlaps_layer(layer) {
            if let Some(FixOutcome::Fixed { real_end: true }) = self.fix(other_end) {
                if let Some(commit) = self.finish(other_end) {
                    return Ok(commit);
                }
            }
        }
        Err(current_end)
    }

    /// `ROUTER::ContinueFromEnd` for a pair (Ctrl+E while routing one): KiCad restarts "the placer of the same mode" at the other end, so a pair continues from the
    /// other end as a pair. As [`Self::continue_from_end`], with the P line as the trace (`Traces()[0]`) and the P net as the net
    /// (`GetNearestRatnestAnchor`: `NearestUnconnectedAnchorPoint( trace )`, the fixed legs counting as connected); the caller commits
    /// [`Continuation::commit`] and starts a new pair with [`Self::start_diff_pair`] at [`Continuation::from`], which then heads for [`Continuation::to`].
    /// Nothing is changed, but for the pair's own last `Move` at `at`.
    pub fn continue_diff_pair_from_end(&mut self, at: Point) -> Result<Continuation, String> {
        let d = self.diff.as_mut().ok_or("not routing a diff pair")?;
        d.move_to(&self.rules, &self.settings, at);
        let preview = d.preview();
        let net = d.net_p.clone();
        let head = &preview.head_a;
        // what the pair has fixed is in the placer's world (`CurrentNode( true )`)
        let mut node = d.world().branch();
        let start = d.current_start_p();
        let current_end = head.last().unwrap_or(start);
        let (from, layers) = if head.segment_count() > 0 {
            node.add_line(head, None, false);
            let ends_with_via = !preview.vias.is_empty();
            let links = node.joint_at(current_end, &net).map_or(0, |j| j.links.len());
            if (!ends_with_via && links >= 2) || (ends_with_via && links >= 3) {
                // "we got something connected"
                (current_end, node.joint_at(current_end, &net).map_or(LayerRange::single(preview.layer), |j| j.layers))
            } else {
                let (id, anchor) = node.nearest_unconnected_item(current_end, &net).ok_or("Route From Other End: nothing is left to connect to.")?;
                (anchor, node.get(id).map_or(LayerRange::single(preview.layer), |i| i.layers()))
            }
        } else {
            if node.joint_at(start, &net).is_none() {
                return Err("Route From Other End: this route does not start on a connection.".into());
            }
            let (id, anchor) = node.nearest_unconnected_item(start, &net).ok_or("Route From Other End: nothing is left to connect to.")?;
            (anchor, node.get(id).map_or(LayerRange::single(preview.layer), |i| i.layers()))
        };
        let layer = if layers.overlaps_layer(preview.layer) { preview.layer } else { layers.start() };
        let (width, gap, via_gap) = (d.sizes.width, d.sizes.gap, d.sizes.via_gap);
        let dp_commit = d.commit();
        let mut commit = self.build_dp_commit(&dp_commit);
        self.drop_redundant(&mut commit);
        Ok(Continuation { commit, net: net.as_deref().unwrap_or("").to_string(), layer: self.layer_name(layer).to_string(), width, from, to: current_end, gap, via_gap })
    }

    /// `ROUTER::UndoLastSegment` (Backspace).
    pub fn undo_last_segment(&mut self) -> bool {
        self.session.as_mut().is_some_and(|s| {
            let undone = s.placer.undo_last_segment();
            let left = s.placer.fixed_vias.len();
            // `UnfixRoute`: the via the undone run ended in is armed again, of its type, to the layer it went on to
            let kind = s.placed_vias.get(left).map(|v| v.kind);
            s.placed_vias.truncate(left);
            if undone {
                (s.pending_via_layer, s.pending_via_kind) = if s.placer.placing_via { (s.placer.via_to_layer, kind.unwrap_or(ViaKind::Through)) } else { (None, ViaKind::Through) };
            }
            undone
        })
    }

    /// The layer the armed via goes on to, if one is armed (`IsPlacingVia()`).
    pub fn pending_via_layer(&self) -> Option<i32> {
        self.session().and_then(|s| s.pending_via_layer)
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
            let runs = s.placer.finish(world, &self.rules, &self.settings, at)?;
            // a via armed at the finish is committed (`FixRoute`: the head ends in it)
            if s.placer.fixed_vias.len() > s.placed_vias.len() {
                let kind = std::mem::take(&mut s.pending_via_kind);
                s.placed_vias.push(PlacedVia { kind });
                s.pending_via_layer = None;
            }
            runs
        };
        let mut commit = self.build_commit(&net_name, runs, layer, width);
        // nothing placed, nothing committed (`CommitRouting`: "if( m_state == ROUTE_TRACK && !m_placer->HasPlacedAnything() ) return"),
        // and so no track is cut either
        if !commit.tracks.is_empty() || !commit.vias.is_empty() {
            let splits = self.session_splits(true);
            self.apply_splits(&splits, &mut commit);
        }
        self.drop_redundant(&mut commit);
        self.simplify_new_lines(&mut commit);
        self.session = None;
        Some(commit)
    }

    /// `ROUTER::CommitRouting()` where the route ends without a last fix -- Esc, or a finish that was refused because the head still collides
    /// (`router_tool.cpp::performRouting`: the cancel branch only does `break;`, the finish branch `FixRoute( .. ); break;`, and after the loop
    /// `m_router->CommitRouting()`): what the clicks have fixed is committed, as `LINE_PLACER::CommitPlacement` / `DIFF_PAIR_PLACER::CommitPlacement`
    /// commit `m_lastNode`, and the session ends. `None` when nothing was fixed ("`if( m_state == ROUTE_TRACK && !m_placer->HasPlacedAnything() )
    /// return;`"), or no route or pair is in progress.
    pub fn commit_fixed(&mut self) -> Option<RouteCommit> {
        if let Some(dp) = self.diff.as_mut() {
            let c = dp.commit();
            self.diff = None;
            if c.legs.is_empty() {
                return None;
            }
            let mut commit = self.build_dp_commit(&c);
            self.drop_redundant(&mut commit);
            return Some(commit);
        }
        let s = self.session.as_mut()?;
        let runs = s.placer.commit_fixed();
        if runs.is_empty() && s.placer.fixed_vias.is_empty() {
            self.session = None;
            return None;
        }
        let (net, layer, width) = (s.placer.net.clone(), s.placer.current_layer, s.placer.width);
        let mut commit = self.build_commit(&net, runs, layer, width);
        let splits = self.session_splits(true);
        self.apply_splits(&splits, &mut commit);
        self.drop_redundant(&mut commit);
        self.simplify_new_lines(&mut commit);
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
            } else if let Some(old) = world.arc_to_split(seg, at) {
                if let Some(track) = old.source_track {
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
            // an arc track: cut where the route meets it, into arc tracks (`SplitAdjacentArcs`: the halves are on the arc's own circle)
            if let Some(arc) = self.world.iter().find_map(|(_, it)| match it {
                Item::Arc(a) if a.source_track.as_deref() == Some(track) => Some(a.clone()),
                _ => None,
            }) {
                let mut pieces = vec![arc.arc];
                for p in points {
                    if let Some(k) = pieces.iter().position(|a| a.start != p && a.end != p && a.distance_to(p) <= EXACT_NM.as_f64()) {
                        if let Some((l, r)) = pieces[k].split(p) {
                            pieces.splice(k..=k, [l, r]);
                        }
                    }
                }
                if pieces.len() > 1 {
                    commit.remove_track_ids.push(track.to_string());
                    let (net, layer) = (arc.net.as_deref().unwrap_or("").to_string(), self.layer_name(arc.layer).to_string());
                    for a in pieces {
                        // the halves of a locked arc are locked too
                        if arc.locked {
                            commit.lock_tracks.push(commit.tracks.len());
                        }
                        commit.tracks.push(eda_model::ir::Track::new_arc(net.clone(), layer.clone(), arc.width, a.start, a.mid, a.end));
                    }
                }
                continue;
            }
            let Some(line) = self.world.track_polyline(track) else { continue };
            let mut pts = line.pts.clone();
            for p in points {
                if pts.contains(&p) {
                    continue;
                }
                if let Some(k) = (0..pts.len() - 1).find(|&k| eda_drc::kimath::Seg::new(pts[k], pts[k + 1]).sq_distance_to_point(p) <= EXACT_NM.area(EXACT_NM)) {
                    pts.insert(k + 1, p);
                }
            }
            if pts.len() == line.pts.len() {
                continue;
            }
            commit.remove_track_ids.push(track.to_string());
            // a locked track cut in two is two locked tracks (`Clone( *s_old )` keeps `MK_LOCKED`; `AddItem` locks the board item)
            if self.world.iter().any(|(_, it)| it.source_track_id() == Some(track) && it.is_locked()) {
                commit.lock_tracks.push(commit.tracks.len());
            }
            commit.tracks.push(eda_model::ir::Track { id: String::new(), net: self.net_name_of(&line), pins: Vec::new(), layer: self.layer_name(line.layer).to_string(), width: line.width, pts, arc_mid_offset: None });
        }
    }

    /// `NODE::Add( .., aAllowRedundant = false )`, which `LINE_PLACER::FixRoute`, `SHOVE::replaceLine` and the draggers all use: a segment of a new track that a
    /// segment of the board already is -- the same two ends on the same layer and net (`findRedundantSegment`) -- is not added again, nor is an arc the board already
    /// has (`findRedundantArc`). The IR keeps tracks as polylines, so a track is cut where it runs over one and the stretches left are the tracks; one that
    /// is all redundant is no track. An item the commit removes does not count: it is gone.
    fn drop_redundant(&self, commit: &mut RouteCommit) {
        let removed: std::collections::HashSet<&str> = commit.remove_track_ids.iter().map(String::as_str).collect();
        let stays = |id: ItemId| self.world.get(id).and_then(|it| it.source_track_id()).is_none_or(|t| !removed.contains(t));
        let mut out: Vec<eda_model::ir::Track> = Vec::with_capacity(commit.tracks.len());
        let mut locks: Vec<usize> = Vec::new();
        let mut changed = false;
        for (index, t) in commit.tracks.iter().enumerate() {
            let (Some(layer), net) = (self.layer_index(&t.layer), net_of(&t.net)) else {
                out.push(t.clone());
                continue;
            };
            let locked = commit.lock_tracks.contains(&index);
            let mut keep = |track: eda_model::ir::Track, out: &mut Vec<eda_model::ir::Track>| {
                if locked {
                    locks.push(out.len());
                }
                out.push(track);
            };
            if let Some((s, _, e)) = t.arc() {
                if self.world.find_redundant_arc(s, e, layer, &net).is_some_and(stays) {
                    changed = true;
                } else {
                    keep(t.clone(), &mut out);
                }
                continue;
            }
            // the runs of its segments the board does not have
            let redundant = |a: Point, b: Point| a != b && self.world.find_redundant_segment(a, b, layer, &net).is_some_and(stays);
            if !t.pts.windows(2).any(|w| redundant(w[0], w[1])) {
                keep(t.clone(), &mut out);
                continue;
            }
            changed = true;
            let mut run: Vec<Point> = Vec::new();
            for w in t.pts.windows(2) {
                if redundant(w[0], w[1]) {
                    if run.len() >= 2 {
                        keep(eda_model::ir::Track { pts: std::mem::take(&mut run), ..t.clone() }, &mut out);
                    }
                    run.clear();
                } else {
                    if run.is_empty() {
                        run.push(w[0]);
                    }
                    run.push(w[1]);
                }
            }
            if run.len() >= 2 {
                keep(eda_model::ir::Track { pts: run, ..t.clone() }, &mut out);
            }
        }
        if changed {
            commit.tracks = out;
            commit.lock_tracks = locks;
        }
    }

    fn build_commit(&self, net: &Net, runs: Vec<Line>, _layer: i32, _width: Nm) -> RouteCommit {
        let mut commit = RouteCommit::default();
        let net_name = net.as_deref().unwrap_or("").to_string();
        for run in &runs {
            commit.tracks.extend(self.line_tracks(run, Some(&net_name)));
        }
        // Every via the session's fixes placed (`FixRoute` adds the via a head ends in to `m_lastNode`, whose diff `CommitRouting` writes): between
        // two runs, at the very start of the route (a via before any segment), or at its end (a route finished on a via). A via is where the
        // walk or the shove left it, which is where the run before it ends and the run after it starts.
        if let Some(s) = self.session() {
            for (k, (v, _)) in s.placer.fixed_vias.iter().enumerate() {
                let kind = s.placed_vias.get(k).map_or(ViaKind::Through, |p| p.kind);
                commit.vias.push(eda_model::ir::Via {
                    id: String::new(),
                    net: net_name.clone(),
                    at: v.pos,
                    drill: v.drill.max(Nm::new(1)),
                    diameter: v.diameter.max(Nm::new(1)),
                    from_layer: self.layer_name(v.layers.start()).to_string(),
                    to_layer: self.layer_name(v.layers.end()).to_string(),
                });
                Self::note_via_kind(&mut commit, kind);
            }
            for (track_id, lines) in s.placer.displaced_tracks() {
                commit.remove_track_ids.push(track_id.to_string());
                for line in lines.iter().filter(|l| l.point_count() >= 2) {
                    commit.tracks.extend(self.line_tracks(line, None));
                }
            }
            for (via_id, pos) in s.placer.displaced_vias() {
                commit.remove_via_ids.push(via_id.to_string());
                if let Some(v) = self.find_via_by_source(via_id) {
                    commit.vias.push(eda_model::ir::Via { id: String::new(), net: v.net.as_deref().unwrap_or("").to_string(), at: pos, drill: v.drill, diameter: v.diameter, from_layer: self.layer_name(v.layers.start()).to_string(), to_layer: self.layer_name(v.layers.end()).to_string() });
                    Self::note_via_kind(&mut commit, self.kind_of_via(Some(via_id)));
                }
            }
        }
        commit
    }

    /// `LINE_PLACER::simplifyNewLine`, which every `FixRoute` runs on what it added: the new segments are assembled into one line with the
    /// tracks they continue (`AssembleLine( aLatest, nullptr, false, false, false )`: no locked segment followed, no change of width; a joint
    /// that is not a line corner -- a pad, a via, a branch -- ends it), `Simplify()` and `MERGE_COLINEAR` drop the points where it goes straight
    /// on, and if that changed the line it replaces the old one (`aNode->Remove( l_orig ); aNode->Add( l )`). Here on the board as the commit
    /// leaves it: for each new track, the line it is part of; when simplifying that line drops a point, the line's tracks -- new ones, and old
    /// ones the commit then removes -- become the one simplified track. So a route that goes straight on from a track's end, or a click in the
    /// middle of a straight run, leaves one segment where KiCad does (findings p5 F2-6). **Differs:** an old track is taken in only when the
    /// whole of its polyline is in the line, and a commit with an arc track is left as it is (`Simplify` keeps an arc's points anyway).
    fn simplify_new_lines(&self, commit: &mut RouteCommit) {
        if commit.tracks.is_empty() || commit.tracks.iter().any(|t| t.arc().is_some()) {
            return;
        }
        const NEW: &str = "\u{1}new";
        let mut w = self.world.branch();
        let removed: std::collections::HashSet<&str> = commit.remove_track_ids.iter().map(String::as_str).collect();
        let doomed: Vec<ItemId> = w.iter().filter(|(_, it)| it.source_track_id().is_some_and(|t| removed.contains(t))).map(|(id, _)| id).collect();
        for id in doomed {
            w.remove(id);
        }
        let gone_vias: std::collections::HashSet<&str> = commit.remove_via_ids.iter().map(String::as_str).collect();
        let doomed: Vec<ItemId> = w.iter().filter(|(_, it)| matches!(it, Item::Via(v) if v.source_via.as_deref().is_some_and(|s| gone_vias.contains(s)))).map(|(id, _)| id).collect();
        for id in doomed {
            w.remove(id);
        }
        for v in &commit.vias {
            let layers = LayerRange::new(self.layer_index(&v.from_layer).unwrap_or(0), self.layer_index(&v.to_layer).unwrap_or(0));
            w.add(Item::Via(crate::item::Via { net: net_of(&v.net), layers, pos: v.at, diameter: v.diameter, drill: v.drill, source_via: None, locked: false, is_virtual: false }));
        }
        let mut new_ids: Vec<Vec<ItemId>> = Vec::with_capacity(commit.tracks.len());
        for (i, t) in commit.tracks.iter().enumerate() {
            let Some(layer) = self.layer_index(&t.layer) else {
                new_ids.push(Vec::new());
                continue;
            };
            let line = Line::from_points(net_of(&t.net), layer, t.width, t.pts.clone());
            new_ids.push(w.add_line(&line, Some((format!("{NEW}{i}"), 0)), commit.lock_tracks.contains(&i)));
        }
        let count_of = |w: &Node, track: &str| w.iter().filter(|(_, it)| it.source_track_id() == Some(track)).count();
        let opts = crate::node::AssembleOpts { follow_locked_segments: false, allow_width_mismatch: false, stop_at_locked_joints: false };
        let mut consumed: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
        let mut merged: Vec<eda_model::ir::Track> = Vec::new();
        for i in 0..commit.tracks.len() {
            if consumed.contains(&i) || commit.lock_tracks.contains(&i) {
                continue;
            }
            let Some(&seg) = new_ids[i].first() else { continue };
            let Some(line) = w.assemble_line_with(seg, opts) else { continue };
            let mut simplified = line.clone();
            simplified.simplify();
            if simplified.pts.len() >= line.pts.len() {
                continue;
            }
            // the tracks of the line, each wholly in it
            let mut members: Vec<&str> = line.segment_ids.iter().filter_map(|id| w.get(*id).and_then(|it| it.source_track_id())).collect();
            members.sort_unstable();
            members.dedup();
            let whole = members.iter().all(|t| line.segment_ids.iter().filter(|id| w.get(**id).and_then(|it| it.source_track_id()) == Some(*t)).count() == count_of(&w, t));
            let news: Vec<usize> = members.iter().filter_map(|t| t.strip_prefix(NEW).and_then(|k| k.parse().ok())).collect();
            if !whole || news.iter().any(|k| consumed.contains(k) || commit.lock_tracks.contains(k)) {
                continue;
            }
            consumed.extend(news.iter().copied());
            let olds: Vec<&str> = members.iter().copied().filter(|t| !t.starts_with(NEW)).collect();
            commit.remove_track_ids.extend(olds.iter().map(|t| t.to_string()));
            // the pads the tracks it takes in land on, it lands on
            let mut pins: Vec<String> = olds.iter().flat_map(|t| self.track_pins.get(*t).into_iter().flatten().cloned()).chain(news.iter().flat_map(|&k| commit.tracks[k].pins.iter().cloned())).collect();
            pins.sort();
            pins.dedup();
            let template = &commit.tracks[i];
            merged.push(eda_model::ir::Track { id: String::new(), net: template.net.clone(), pins, layer: template.layer.clone(), width: template.width, pts: simplified.pts, arc_mid_offset: None });
        }
        if consumed.is_empty() {
            return;
        }
        let mut tracks = Vec::with_capacity(commit.tracks.len());
        let mut locks = Vec::new();
        for (i, t) in std::mem::take(&mut commit.tracks).into_iter().enumerate() {
            if consumed.contains(&i) {
                continue;
            }
            if commit.lock_tracks.contains(&i) {
                locks.push(tracks.len());
            }
            tracks.push(t);
        }
        tracks.extend(merged);
        commit.tracks = tracks;
        commit.lock_tracks = locks;
    }

    fn net_name_of(&self, line: &Line) -> String {
        line.net.as_deref().unwrap_or("").to_string()
    }

    /// A line as the IR tracks it becomes: a polyline for each run of straight segments, and an arc track for each arc the line still has
    /// (`NODE::Add( LINE )` makes an `ARC` of an arc of the line and a `SEGMENT` of a segment that is not one; the IR keeps a polyline whole and an arc
    /// as a track of its own). On `net`, or the line's own.
    pub fn line_tracks(&self, line: &Line, net: Option<&str>) -> Vec<eda_model::ir::Track> {
        line_to_tracks(line, self.layer_name(line.layer), net)
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
    pub fn item_near(&self, at: Point, layer_name: &str, max_dist: Nm) -> Option<ItemId> {
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
        self.drag_start_locked(at, layer_name, free_angle, false)
    }

    /// [`Self::drag_start_with`] for a person who has said "Drag Anyway" to a locked item (`ROUTER_TOOL::performDragging`'s dialog; `DRAGGER::Start`:
    /// `startItem->Unmark( MK_LOCKED )`): the item taken hold of is unlocked in the router's world, the ones next to it still are not (the dragged line stops in front
    /// of them). Without `drag_locked` a locked item is refused with [`crate::dragger::LOCKED_MESSAGE`].
    pub fn drag_start_locked(&mut self, at: Point, layer_name: &str, free_angle: bool, drag_locked: bool) -> Result<(), String> {
        let layer = self.layer_index(layer_name).ok_or_else(|| format!("unknown layer {layer_name:?}"))?;
        let item_id = self.world.item_at(at, LayerRange::single(layer), START_SNAP_NM).ok_or("nothing to drag there")?;
        if drag_locked {
            self.world.set_locked(item_id, false);
        }
        let dragger = crate::dragger::Dragger::start_checked(&self.world, at, item_id, free_angle)?;
        self.session = None;
        self.diff = None;
        self.drag = Some(dragger);
        self.drag_mode = Some(self.settings.mode);
        Ok(())
    }

    /// `ROUTER::Move`/`moveDragging`: the live drag preview, `DRAGGER::Drag` at `at`. The drag remembers the last position that
    /// worked: one that does not shows that one again (`DragPreview::restored`).
    pub fn drag_preview(&mut self, at: Point) -> Option<crate::dragger::DragPreview> {
        let settings = self.drag_settings();
        let (world, rules) = (&self.world, &self.rules);
        self.drag.as_mut().map(|d| d.drag(world, rules, &settings, at))
    }

    /// The router's settings with the mode the drag started in.
    fn drag_settings(&self) -> RoutingSettings {
        RoutingSettings { mode: self.drag_mode.unwrap_or(self.settings.mode), ..self.settings.clone() }
    }

    /// The IR tracks the dragged line stands on: what the cursor must not snap to while dragging (`checkSnap`).
    pub fn drag_line_tracks(&self) -> Vec<String> {
        self.drag.as_ref().map(|d| d.line_tracks(&self.world)).unwrap_or_default()
    }

    /// What the drag in progress is (`DM_SEGMENT`, `DM_CORNER` or `DM_VIA`).
    pub fn drag_kind(&self) -> Option<crate::dragger::DragKind> {
        self.drag.as_ref().map(|d| d.kind)
    }

    /// `GetForceMarkObstaclesMode`: the drag in progress only marks obstacles because its first position failed.
    /// The drag is of a virtual via (`DRAGGER::checkVirtualVia`: the end of a segment where tracks meet with no pad or via), which is not drawn.
    pub fn drag_virtual_via(&self) -> bool {
        self.drag.as_ref().is_some_and(|d| d.via_virtual)
    }

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
        let settings = self.drag_settings();
        let drag_commit = self.drag.as_mut()?.finish(&self.world, &self.rules, &settings, at, force_commit)?;
        let mut commit = RouteCommit { remove_track_ids: drag_commit.remove_track_ids, remove_via_ids: drag_commit.remove_via_ids, ..RouteCommit::default() };
        for line in &drag_commit.tracks {
            commit.tracks.extend(self.line_tracks(line, None));
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
            Self::note_via_kind(&mut commit, self.kind_of_via(source_via.as_deref()));
        }
        self.drop_redundant(&mut commit);
        self.drag = None;
        Some(commit)
    }

    // ------------------------------------------------------------- diff pairs (stage 6)
    //
    // `6` on a net with a recognized differential-pair suffix: route two parallel, gap-matched lines at once
    // (`crate::dp_placer::DiffPairPlacer`, KiCad's `DIFF_PAIR_PLACER`): coupled in all three modes, walked or shoved together, with
    // vias. See that module's own doc comment for what differs from KiCad.

    /// `DIFF_PAIR_PLACER::Start` plus `ROUTER::isStartingPointRoutable`'s checks for a pair: resolve whatever is at `at` the same
    /// way [`Self::start`] does (a real pad/via/track-end within [`START_SNAP_NM`]), then find its differential-pair partner net's
    /// nearest item of the same kind ([`crate::dp_placer::find_dp_primitive_pair`]). Replaces any route/drag/diff-pair session already
    /// in progress. The error is KiCad's own message.
    pub fn start_diff_pair(&mut self, at: Point, layer_name: &str) -> Result<(), String> {
        let layer = self.layer_index(layer_name).ok_or_else(|| format!("unknown layer {layer_name:?}"))?;
        // the pad, via or track end there, else the track under the cursor (`isStartingPointRoutable` with `aStartItem` the item picked:
        // `FindDpPrimitivePair` then finds its free end, or says "Can't find a suitable starting point. ..."), else nothing (findings p5 F4-6)
        let hit = self.world.nearest_anchor(at, LayerRange::single(layer), START_SNAP_NM, None).map(|(id, _)| id).or_else(|| self.world.segment_at(at, layer, None, None, &[]));
        let Some(start_item) = hit else {
            return Err("Cannot start a differential pair in the middle of nowhere.".into());
        };
        let mut placer = crate::dp_placer::DiffPairPlacer::start(&self.world, &self.rules, &self.settings, start_item, layer, self.layers.all())?;
        // `ImportSizes`: "First try to pick up diff pair width from starting track" (`m_UseConnectedTrackWidth`, on by default) -- also off a
        // pad or via with a stub (findings p5 F4-5)
        if let Some(w) = self.inherit_track_width(start_item, at, layer) {
            placer.set_dimensions(Some(w), None, None);
        }
        self.session = None;
        self.drag = None;
        self.diff = Some(placer);
        Ok(())
    }

    /// `PNS_KICAD_IFACE_BASE::inheritTrackWidth( aItem, .., aStartPosition )`: a track's own width; for a pad or via, of the tracks joined to it
    /// on the start layer the one whose far end is nearest the cursor ("the far-end direction is a proxy for which exit stub the user is
    /// pointing at"), else the narrowest on the start layer, else the narrowest of all; none with no track joined.
    fn inherit_track_width(&self, item: ItemId, at: Point, layer: i32) -> Option<Nm> {
        let it = self.world.get(item)?;
        if it.is_track() {
            return Some(it.width());
        }
        let p = match it {
            Item::Via(v) => v.pos,
            Item::Solid(s) => s.pos,
            _ => return None,
        };
        let joint = self.world.joint_at(p, it.net())?;
        let linked: Vec<&Item> = joint.links.iter().filter(|&&id| id != item).filter_map(|&id| self.world.get(id)).filter(|i| i.is_track()).collect();
        let sq = |a: Point, b: Point| ((a.x - b.x).get() as i128).pow(2) + ((a.y - b.y).get() as i128).pow(2);
        let closest = linked.iter().filter(|i| i.layers().start() == layer).filter_map(|i| i.track_ends().map(|(a, b)| (i, if sq(a, p) > sq(b, p) { a } else { b }))).min_by_key(|(_, far)| sq(*far, at));
        if let Some((i, _)) = closest {
            return Some(i.width());
        }
        linked.iter().filter(|i| i.layers().start() == layer).map(|i| i.width()).min().or_else(|| linked.iter().map(|i| i.width()).min())
    }

    /// `ROUTER::UpdateSizes` for a pair (the "Differential Pair Dimensions..." dialog,
    /// `ROUTER_TOOL::DpDimensionsDialog`): the running pair's track width, gap and via gap. `None` leaves a value as it is. The pair's
    /// own preview and its commit read them, so the next `move` already uses the new values. Returns whether a pair is running.
    pub fn set_diff_pair_dimensions(&mut self, width: Option<Nm>, gap: Option<Nm>) -> bool {
        self.set_diff_pair_sizes(width, gap, None)
    }

    /// [`Self::set_diff_pair_dimensions`] with the dialog's via gap too.
    pub fn set_diff_pair_sizes(&mut self, width: Option<Nm>, gap: Option<Nm>, via_gap: Option<Nm>) -> bool {
        let Some(d) = self.diff.as_mut() else { return false };
        d.set_dimensions(width, gap, via_gap);
        true
    }

    /// The net names of the diff-pair session currently in progress, if any -- `(P, N)`, the UI's own readout (not consulted by
    /// `Router` itself for any decision, same as [`Self::current_net`]).
    pub fn diff_pair_nets(&self) -> Option<(Option<String>, Option<String>)> {
        self.diff.as_ref().map(|d| (d.net_p.as_deref().map(str::to_string), d.net_n.as_deref().map(str::to_string)))
    }

    /// `ROUTER::Move` for a pair: the live preview at `at` (`DIFF_PAIR_PLACER::Move`), in the router's mode.
    pub fn diff_pair_preview(&mut self, at: Point) -> Option<crate::dp_placer::DiffPairPreview> {
        let d = self.diff.as_mut()?;
        d.move_to(&self.rules, &self.settings, at);
        Some(d.preview())
    }

    pub fn flip_diff_pair_posture(&mut self) {
        if let Some(d) = &mut self.diff {
            d.flip_posture();
        }
    }

    /// `ROUTER::ToggleViaPlacement` for a pair (`V`): the head ends in a pair of vias, and the route goes on to `to_layer` from them.
    pub fn toggle_diff_pair_via(&mut self, enabled: bool, diameter: Option<Nm>, drill: Option<Nm>, to_layer: Option<i32>) {
        if let Some(d) = &mut self.diff {
            d.toggle_via(enabled, diameter, drill, to_layer);
        }
    }

    /// `DIFF_PAIR_PLACER::FixRoute` for an intermediate click.
    pub fn fix_diff_pair(&mut self, at: Point) -> Option<FixOutcome> {
        let d = self.diff.as_mut()?;
        Some(d.fix_route(&self.rules, &self.settings, at, false).unwrap_or(FixOutcome::Blocked))
    }

    pub fn undo_diff_pair_segment(&mut self) -> bool {
        self.diff.as_mut().map(|d| d.undo_last_segment()).unwrap_or(false)
    }

    pub fn diff_pair_cancel(&mut self) {
        self.diff = None;
    }

    /// `DIFF_PAIR_PLACER::FixRoute` with `aForceFinish`: both lines' finished legs become ordinary `Track` IR entries on their own
    /// nets, the vias `Via` entries, and whatever a shove moved is replaced -- in the *same* [`RouteCommit`] (and so the same
    /// `Cmd::CommitRoute` undo step) a single-track finish produces. `None` if the head does not fit; the session stays open then.
    pub fn finish_diff_pair(&mut self, at: Point) -> Option<RouteCommit> {
        let commit = self.diff.as_mut()?.finish(&self.rules, &self.settings, at)?;
        let mut route_commit = self.build_dp_commit(&commit);
        self.drop_redundant(&mut route_commit);
        self.diff = None;
        Some(route_commit)
    }

    /// What a finished pair puts on the board, as the IR has it.
    fn build_dp_commit(&self, c: &crate::dp_placer::DpCommit) -> RouteCommit {
        let mut commit = RouteCommit::default();
        let track = |net: &Net, layer: i32, width: Nm, pts: &[Point]| eda_model::ir::Track { id: String::new(), net: net.as_deref().unwrap_or("").to_string(), pins: Vec::new(), layer: self.layer_name(layer).to_string(), width, pts: pts.to_vec(), arc_mid_offset: None };
        for leg in &c.legs {
            for (net, pts) in [(&c.net_p, &leg.p), (&c.net_n, &leg.n)] {
                // a line that goes nowhere is no track
                if pts.windows(2).any(|w| w[0] != w[1]) {
                    commit.tracks.push(track(net, leg.layer, leg.width, pts));
                }
            }
            if let Some((a, b)) = &leg.vias {
                for v in [a, b] {
                    commit.vias.push(eda_model::ir::Via { id: String::new(), net: v.net.as_deref().unwrap_or("").to_string(), at: v.pos, drill: v.drill.max(Nm::new(1)), diameter: v.diameter.max(Nm::new(1)), from_layer: self.layer_name(v.layers.start()).to_string(), to_layer: self.layer_name(v.layers.end()).to_string() });
                }
            }
        }
        for (track_id, lines) in &c.displaced_tracks {
            commit.remove_track_ids.push(track_id.clone());
            for line in lines.iter().filter(|l| l.point_count() >= 2) {
                commit.tracks.extend(self.line_tracks(line, None));
            }
        }
        for (via_id, pos) in &c.displaced_vias {
            commit.remove_via_ids.push(via_id.clone());
            if let Some(v) = self.find_via_by_source(via_id) {
                commit.vias.push(eda_model::ir::Via { id: String::new(), net: v.net.as_deref().unwrap_or("").to_string(), at: *pos, drill: v.drill, diameter: v.diameter, from_layer: self.layer_name(v.layers.start()).to_string(), to_layer: self.layer_name(v.layers.end()).to_string() });
                Self::note_via_kind(&mut commit, self.kind_of_via(Some(via_id)));
            }
        }
        commit
    }

    pub fn net_of(&self, id: ItemId) -> Option<String> {
        self.world.get(id)?.net().as_deref().map(str::to_string)
    }
}

/// A line as the IR tracks it becomes (`layer_name` is its layer's): a polyline for each run of straight segments, an arc track for each arc the
/// line still has ([`Line::pieces`]). On `net` when given, else the line's own.
pub fn line_to_tracks(line: &Line, layer_name: &str, net: Option<&str>) -> Vec<eda_model::ir::Track> {
    let net = net.map(str::to_string).unwrap_or_else(|| line.net.as_deref().unwrap_or("").to_string());
    line.pieces()
        .into_iter()
        .map(|piece| match piece {
            LinePiece::Poly(pts) => eda_model::ir::Track { id: String::new(), net: net.clone(), pins: Vec::new(), layer: layer_name.to_string(), width: line.width, pts, arc_mid_offset: None },
            LinePiece::Arc(ArcPts { start, mid, end }) => eda_model::ir::Track::new_arc(net.clone(), layer_name.to_string(), line.width, start, mid, end),
        })
        .collect()
}

/// Net lookup helper for callers that only have a name, not an `ItemId` --
/// e.g. resolving a `Cmd`'s own net field the same way the rest of this
/// crate does.
pub fn net(name: &str) -> Net {
    net_of(name)
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;
    use eda_model::ir::{FootprintInstance, LabelSide, PlacementSection, Provenance, RoutingSection, Side};
    use eda_model::{Net as IrNet, Part, Pin, PinKind};

    fn two_pad_board() -> (Design, ConstraintModel) {
        let part = |r: &str| Part { reference: r.into(), mpn: None, lcsc: None, value: None, package: Some("0603".into()), footprint: Some("0603".into()), pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }], body_nm: None, symbol: None, datasheet: None, edge: None };
        let model = ConstraintModel { parts: vec![part("R1"), part("R2")], nets: vec![IrNet { name: "SIG".into(), pins: vec!["R1.1".into(), "R2.1".into()] }], ..Default::default() };
        let design = Design {
            footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
            schema: eda_model::ir::SCHEMA_VERSION,
            provenance: Provenance { engine_version: "test".into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection { outline: vec![], footprints: vec![FootprintInstance { id: "R1".into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, rot: 0, side: Side::Top, label: LabelSide::Above }, FootprintInstance { id: "R2".into(), at: Point { x: mm(6.0), y: Nm::ZERO }, rot: 0, side: Side::Top, label: LabelSide::Above }], modules: vec![] }),
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
            schema: eda_model::ir::SCHEMA_VERSION,
            provenance: Provenance { engine_version: "test".into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection { outline: vec![], footprints: vec![], modules: vec![] }),
            routing: Some(RoutingSection {
                tracks: vec![eda_model::ir::Track { id: "trkA".into(), net: "SIG".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }], arc_mid_offset: None }],
                vias: vec![eda_model::ir::Via { id: "viaA".into(), net: "GND".into(), at: Point { x: mm(5.0), y: Nm::ZERO }, drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }],
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
        router.drag_start(Point { x: mm(1.0), y: Nm::ZERO }, "F.Cu").expect("must grab the track's free end");
        assert!(router.is_dragging());
        let preview = router.drag_preview(Point { x: mm(1.0), y: mm(2.0) }).expect("preview while dragging");
        assert!(!preview.colliding);
        let commit = router.drag_finish(Point { x: mm(1.0), y: mm(2.0) }).expect("collision-free drag must commit");
        assert!(!router.is_dragging());
        assert_eq!(commit.remove_track_ids, vec!["trkA"]);
        assert_eq!(commit.tracks.len(), 1);
        // `dragCorner45`: the dragged end is at the cursor and the track reaches it by 45-degree legs, not one leg at 63 degrees
        assert_eq!(commit.tracks[0].pts, vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::ZERO, y: mm(1.0) }, Point { x: mm(1.0), y: mm(2.0) }]);
    }

    #[test]
    fn shove_mode_drag_displaces_a_via_and_the_commit_carries_its_real_size() {
        let (design, model) = track_and_via_board();
        let mut router = Router::new(&design, &model);
        router.settings.mode = crate::settings::Mode::Shove;
        router.drag_start(Point { x: mm(1.0), y: Nm::ZERO }, "F.Cu").unwrap();
        // Drag the track's free end straight across the board, through
        // the via's position -- shove must push the via aside.
        let to = Point { x: mm(10.0), y: Nm::ZERO };
        let preview = router.drag_preview(to).expect("preview while dragging");
        assert!(!preview.colliding, "shove must clear the via, not just report a collision");
        let commit = router.drag_finish(to).expect("collision-free shoved drag must commit");
        assert!(commit.remove_via_ids.contains(&"viaA".to_string()));
        let moved_via = commit.vias.iter().find(|v| v.at != Point { x: mm(5.0), y: Nm::ZERO }).expect("the via must have moved");
        assert_eq!(moved_via.diameter, mm(0.6), "a shoved via's real diameter must survive, not the 0 placeholder");
        assert_eq!(moved_via.drill, mm(0.3));
        assert_eq!(moved_via.net, "GND");
    }

    /// `PNS_KICAD_IFACE::syncVia` reads the board via's type (`aVia->GetViaType()`) and `AddItem` writes it back: a blind via a shove moves comes back as
    /// a new via of the commit with its type in `via_kinds`, a through via without an entry.
    #[test]
    fn a_shoved_blind_via_keeps_its_type() {
        let (mut design, model) = track_and_via_board();
        design.drawings = Some(eda_model::ir::DrawingsSection { via_extras: vec![eda_model::via_extra::ViaExtra { id: "viaA".into(), kind: ViaKind::Blind, tokens: vec![] }], ..Default::default() });
        let mut router = Router::new(&design, &model);
        router.settings.mode = crate::settings::Mode::Shove;
        router.drag_start(Point { x: mm(1.0), y: Nm::ZERO }, "F.Cu").unwrap();
        let to = Point { x: mm(10.0), y: Nm::ZERO };
        router.drag_preview(to).expect("preview while dragging");
        let commit = router.drag_finish(to).expect("the shoved drag commits");
        let i = commit.vias.iter().position(|v| v.net == "GND").expect("the via is put back");
        assert_eq!(commit.via_kinds, vec![(i, ViaKind::Blind)]);
        let (design, model) = track_and_via_board();
        let mut router = Router::new(&design, &model);
        router.settings.mode = crate::settings::Mode::Shove;
        router.drag_start(Point { x: mm(1.0), y: Nm::ZERO }, "F.Cu").unwrap();
        router.drag_preview(to);
        assert!(router.drag_finish(to).unwrap().via_kinds.is_empty(), "a through via has no entry");
    }

    /// An imported board has one IR track per segment, so a pushed line stands on several. The commit
    /// removes every one of them and puts one polyline in their place -- not the first one replaced by the
    /// whole line while the rest stay behind as copies.
    #[test]
    fn a_pushed_line_on_two_ir_tracks_is_replaced_by_one_track_in_the_commit() {
        let (mut design, model) = two_pad_board();
        let track = |id: &str, a: Point, b: Point| eda_model::ir::Track { id: id.into(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![a, b], arc_mid_offset: None };
        design.routing.as_mut().unwrap().tracks = vec![track("ta", Point { x: mm(2.5), y: mm(-2.0) }, Point { x: mm(2.5), y: mm(-0.5) }), track("tb", Point { x: mm(2.5), y: mm(-0.5) }, Point { x: mm(2.5), y: mm(2.0) })];
        let mut router = Router::new(&design, &model);
        router.settings.mode = crate::settings::Mode::Shove;
        router.start(Point { x: mm(-0.825), y: Nm::ZERO }, "F.Cu", mm(0.2)).expect("starts on R1.1");
        let preview = router.preview(Point { x: mm(4.5), y: Nm::ZERO }).expect("a route is in progress");
        assert!(!preview.colliding);
        assert_eq!(preview.displaced_lines.iter().filter(|d| d.line.point_count() >= 2).count(), 1, "one new shape");
        let commit = router.finish(Point { x: mm(4.5), y: Nm::ZERO }).expect("the pushed route commits");
        let mut removed = commit.remove_track_ids.clone();
        removed.sort();
        assert_eq!(removed, vec!["ta", "tb"], "both tracks the line stood on are removed");
        let gnd: Vec<_> = commit.tracks.iter().filter(|t| t.net == "GND").collect();
        assert_eq!(gnd.len(), 1, "and one polyline replaces them");
        assert_eq!(gnd[0].width, mm(0.2));
        assert_eq!(gnd[0].pts.first(), Some(&Point { x: mm(2.5), y: mm(-2.0) }));
        assert_eq!(gnd[0].pts.last(), Some(&Point { x: mm(2.5), y: mm(2.0) }));
    }

    /// A SIG track `trkA` (0,0)-(4000,0) on F.Cu, 200 wide, and a SIG via `viaB` at (8000, 3000) to route to.
    fn track_and_a_via_of_the_same_net() -> (Design, ConstraintModel) {
        let (mut design, model) = track_and_via_board();
        let rt = design.routing.as_mut().unwrap();
        rt.tracks = vec![eda_model::ir::Track { id: "trkA".into(), net: "SIG".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(4.0), y: Nm::ZERO }], arc_mid_offset: None }];
        rt.vias = vec![eda_model::ir::Via { id: "viaB".into(), net: "SIG".into(), at: Point { x: mm(8.0), y: mm(3.0) }, drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }];
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
        router.start(Point { x: mm(2.0), y: Nm::ZERO }, "F.Cu", mm(0.2)).expect("the middle of trkA is something to route from");
        assert_eq!(router.current_net().as_deref(), Some("SIG"));
        let via = Point { x: mm(8.0), y: mm(3.0) };
        let pv = router.preview(via).expect("a route");
        assert!(!pv.colliding);
        assert_eq!(pv.head.first(), Some(Point { x: mm(2.0), y: Nm::ZERO }), "the head leaves the middle of the track");
        assert_eq!(pv.head.last(), Some(via));
        assert_eq!(pv.snapped_end, Some(via));
        assert!(matches!(router.fix(via), Some(FixOutcome::Fixed { real_end: true })));
        let commit = router.finish(via).expect("the route commits");
        // the track is replaced by itself with a vertex at the start
        assert_eq!(commit.remove_track_ids, vec!["trkA"]);
        let split = commit.tracks.iter().find(|t| t.pts.first() == Some(&Point { x: Nm::ZERO, y: Nm::ZERO })).expect("the cut track");
        assert_eq!(split.pts, vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(2.0), y: Nm::ZERO }, Point { x: mm(4.0), y: Nm::ZERO }]);
        assert_eq!((split.net.as_str(), split.layer.as_str(), split.width), ("SIG", "F.Cu", mm(0.2)));
        let route = commit.tracks.iter().find(|t| t.pts.first() == Some(&Point { x: mm(2.0), y: Nm::ZERO })).expect("the route");
        assert_eq!(route.pts.last(), Some(&via));
        // and in the board that results, the new route and both halves of the track meet at one joint
        let (after, _) = crate::from_ir::build_node(&committed(&design, &commit), &model);
        let joint = after.joint_at(Point { x: mm(2.0), y: Nm::ZERO }, &net_of("SIG")).expect("a joint where the route starts");
        let segments = joint.links.iter().filter(|&&id| matches!(after.get(id), Some(Item::Segment(_)))).count();
        assert_eq!(segments, 3, "the two halves of the track and the route");
        // three segments of one width meet there with no pad or via: no virtual via (`FixupVirtualVias` never counts `n_seg`)
        assert!(!joint.links.iter().any(|&id| after.get(id).is_some_and(Item::is_virtual)));
    }

    /// A route that starts and places nothing leaves the track whole (`CommitRouting` commits nothing).
    #[test]
    fn a_route_that_places_nothing_does_not_cut_the_track() {
        let (design, model) = track_and_a_via_of_the_same_net();
        let mut router = Router::new(&design, &model);
        router.start(Point { x: mm(2.0), y: Nm::ZERO }, "F.Cu", mm(0.2)).unwrap();
        let commit = router.finish(Point { x: mm(2.0), y: Nm::ZERO }).expect("nothing to refuse");
        assert!(commit.tracks.is_empty() && commit.remove_track_ids.is_empty(), "{commit:?}");
        // cancelled: the router's own world was never cut
        let mut again = Router::new(&design, &model);
        again.start(Point { x: mm(2.0), y: Nm::ZERO }, "F.Cu", mm(0.2)).unwrap();
        again.cancel();
        assert!(again.world.iter().filter(|(_, it)| matches!(it, Item::Segment(_))).count() == 1, "the world still has the one segment");
    }

    /// `snapToItem`: within half the track's width of an end it is the end, not the middle.
    #[test]
    fn near_a_tracks_end_the_route_starts_at_the_end_and_cuts_nothing() {
        let (design, model) = track_and_a_via_of_the_same_net();
        let mut router = Router::new(&design, &model);
        router.start(Point { x: mm(3.95), y: mm(0.02) }, "F.Cu", mm(0.2)).expect("the end");
        let pv = router.preview(Point { x: mm(8.0), y: mm(3.0) }).unwrap();
        assert_eq!(pv.head.first(), Some(Point { x: mm(4.0), y: Nm::ZERO }));
        let commit = router.finish(Point { x: mm(8.0), y: mm(3.0) }).unwrap();
        // not cut: the route goes straight on from the track's end, so the two are one line (`simplifyNewLine`), the end point gone
        assert_eq!(commit.remove_track_ids, vec!["trkA".to_string()], "{commit:?}");
        assert_eq!(commit.tracks.len(), 1, "{commit:?}");
        assert_eq!(commit.tracks[0].pts, vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(5.0), y: Nm::ZERO }, Point { x: mm(8.0), y: mm(3.0) }]);
    }

    /// findings p5 F2-6: `simplifyNewLine` -- a route that continues a track straight on is merged with it, and so is a click in the middle of
    /// a straight run; a route that turns at the track's end leaves the track alone.
    #[test]
    fn a_route_continuing_a_track_straight_is_merged_with_it() {
        let (design, model) = track_and_a_via_of_the_same_net();
        let mut router = Router::new(&design, &model);
        router.settings.auto_posture = false;
        router.start(Point { x: mm(4.0), y: Nm::ZERO }, "F.Cu", mm(0.2)).unwrap();
        router.preview(Point { x: mm(6.0), y: Nm::ZERO });
        router.fix(Point { x: mm(6.0), y: Nm::ZERO });
        router.preview(Point { x: mm(7.0), y: Nm::ZERO });
        let commit = router.finish(Point { x: mm(7.0), y: Nm::ZERO }).unwrap();
        assert_eq!(commit.remove_track_ids, vec!["trkA".to_string()]);
        assert_eq!(commit.tracks.iter().map(|t| t.pts.clone()).collect::<Vec<_>>(), vec![vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(7.0), y: Nm::ZERO }]], "one segment");
        // a turn at the end: nothing merged
        let mut router = Router::new(&design, &model);
        router.start(Point { x: mm(4.0), y: Nm::ZERO }, "F.Cu", mm(0.2)).unwrap();
        router.preview(Point { x: mm(4.0), y: mm(-3.0) });
        let commit = router.finish(Point { x: mm(4.0), y: mm(-3.0) }).unwrap();
        assert!(commit.remove_track_ids.is_empty(), "{commit:?}");
    }

    /// A point on the track that the studio's snapping put there exactly wins over a pad within the generous 500 um; one that was not
    /// snapped near a pad still goes to the pad.
    #[test]
    fn a_start_on_the_tracks_centre_line_beats_a_pad_beside_it_and_an_unsnapped_one_still_snaps() {
        let (mut design, model) = track_and_a_via_of_the_same_net();
        design.routing.as_mut().unwrap().vias.push(eda_model::ir::Via { id: "near".into(), net: "SIG".into(), at: Point { x: mm(2.2), y: mm(0.3) }, drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        let mut router = Router::new(&design, &model);
        router.start(Point { x: mm(2.0), y: Nm::ZERO }, "F.Cu", mm(0.2)).unwrap();
        assert_eq!(router.preview(Point { x: mm(8.0), y: mm(3.0) }).unwrap().head.first(), Some(Point { x: mm(2.0), y: Nm::ZERO }), "on the centre line: the track");
        // 300 um above the track and 100 um from the via: no track under it, the via is within the snap
        router.start(Point { x: mm(2.1), y: mm(0.3) }, "F.Cu", mm(0.2)).unwrap();
        assert_eq!(router.preview(Point { x: mm(8.0), y: mm(3.0) }).unwrap().head.first(), Some(Point { x: mm(2.2), y: mm(0.3) }), "the via's centre");
    }

    /// A route ends in the middle of a same-net track the cursor is on (`snap_to_tracks`), and the track is cut there.
    #[test]
    fn a_route_can_end_in_the_middle_of_a_track() {
        let (design, model) = track_and_a_via_of_the_same_net();
        let mut router = Router::new(&design, &model);
        router.start(Point { x: mm(8.0), y: mm(3.0) }, "B.Cu", mm(0.2)).expect("starts on the via");
        let middle = Point { x: mm(3.0), y: Nm::ZERO };
        let pv = router.preview(middle).unwrap();
        // the via spans both layers, the track is on F.Cu: the route is on B.Cu and cannot end on a F.Cu track
        assert_eq!(pv.snapped_end, None, "a track on another layer is not an end");
        router.start(Point { x: mm(8.0), y: mm(3.0) }, "F.Cu", mm(0.2)).unwrap();
        let pv = router.preview(middle).unwrap();
        assert!(!pv.colliding);
        assert_eq!(pv.snapped_end, Some(middle));
        assert!(pv.snapped_segment.is_some());
        assert_eq!(pv.head.last(), Some(middle));
        assert!(matches!(router.fix(middle), Some(FixOutcome::Fixed { real_end: true })), "the click ends the connection");
        let commit = router.finish(middle).expect("commits");
        assert_eq!(commit.remove_track_ids, vec!["trkA"]);
        let cut = commit.tracks.iter().find(|t| t.pts.first() == Some(&Point { x: Nm::ZERO, y: Nm::ZERO })).expect("the cut track");
        assert_eq!(cut.pts, vec![Point { x: Nm::ZERO, y: Nm::ZERO }, middle, Point { x: mm(4.0), y: Nm::ZERO }]);
        let (after, _) = crate::from_ir::build_node(&committed(&design, &commit), &model);
        assert_eq!(after.joint_at(middle, &net_of("SIG")).map(|j| j.links.iter().filter(|&&id| matches!(after.get(id), Some(Item::Segment(_)))).count()), Some(3));
    }

    /// With "snap to tracks" off the cursor over a track is no end, and nothing is cut.
    #[test]
    fn without_snap_to_tracks_a_route_does_not_end_on_a_track() {
        let (design, model) = track_and_a_via_of_the_same_net();
        let mut router = Router::new(&design, &model);
        router.settings.snap_to_tracks = false;
        router.start(Point { x: mm(8.0), y: mm(3.0) }, "F.Cu", mm(0.2)).unwrap();
        let pv = router.preview(Point { x: mm(3.0), y: Nm::ZERO }).unwrap();
        assert_eq!((pv.snapped_end, pv.snapped_segment), (None, None));
    }

    /// A route that comes in along the track it ends on ends where it first touches it (`Move`: `lastSeg.Collinear( targetSeg ) &&
    /// targetSeg.Overlaps( lastSeg )`), instead of running along it to the cursor.
    #[test]
    fn a_route_that_runs_along_the_track_it_ends_on_ends_where_it_meets_it() {
        let (mut design, model) = track_and_a_via_of_the_same_net();
        design.routing.as_mut().unwrap().vias[0].at = Point { x: mm(8.0), y: Nm::ZERO };
        let mut router = Router::new(&design, &model);
        router.start(Point { x: mm(8.0), y: Nm::ZERO }, "F.Cu", mm(0.2)).unwrap();
        // the cursor is on the track, 1 mm in from its end, with the route coming from the right along the same line
        let pv = router.preview(Point { x: mm(3.0), y: Nm::ZERO }).unwrap();
        assert_eq!(pv.head.last(), Some(Point { x: mm(4.0), y: Nm::ZERO }), "it ends at the track's end, not 1 mm along it: {:?}", pv.head.pts);
        assert_eq!(pv.snapped_end, Some(Point { x: mm(4.0), y: Nm::ZERO }));
    }

    /// `ROUTER::ContinueFromEnd`: from the other pad of the connection -- before anything is fixed, and after the first fix, when
    /// what was fixed is committed and the far end is the other side of it.
    #[test]
    fn route_from_other_end_restarts_at_the_far_pad_and_commits_what_is_fixed() {
        let (design, model) = two_pad_board();
        let (r1, r2) = (Point { x: mm(-0.825), y: Nm::ZERO }, Point { x: mm(5.175), y: Nm::ZERO });
        let mut router = Router::new(&design, &model);
        router.start(r1, "F.Cu", mm(0.2)).unwrap();
        // nothing fixed yet: the connection's other end is R2.1, and the route heads for where the old head ended
        let cursor = Point { x: mm(3.0), y: mm(0.8) };
        let head_end = |router: &Router, at: Point| router.preview(at).unwrap().head.last().unwrap();
        let first = router.continue_from_end(cursor).expect("R2.1 is unconnected");
        assert_eq!((first.from, first.to, first.layer.as_str(), first.net.as_str(), first.width), (r2, head_end(&router, cursor), "F.Cu", "SIG", mm(0.2)));
        assert!(first.commit.tracks.is_empty() && first.commit.remove_track_ids.is_empty(), "nothing was placed, so nothing is committed");
        assert!(router.is_routing(), "the session is untouched: the caller replaces it");
        // fix the first leg: R1.1 is connected to the fixed end now, and the far end is still R2.1
        let fixed_at = Point { x: mm(2.0), y: mm(0.8) };
        assert!(matches!(router.fix(fixed_at), Some(FixOutcome::Fixed { real_end: false })));
        let cursor = Point { x: mm(3.5), y: mm(1.5) };
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
        design.routing.as_mut().unwrap().tracks.push(eda_model::ir::Track { id: "t".into(), net: "SIG".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: mm(-0.825), y: Nm::ZERO }, Point { x: mm(5.175), y: Nm::ZERO }], arc_mid_offset: None });
        let mut router = Router::new(&design, &model);
        router.start(Point { x: mm(-0.825), y: Nm::ZERO }, "F.Cu", mm(0.2)).unwrap();
        let err = router.continue_from_end(Point { x: mm(1.0), y: Nm::ZERO }).expect_err("everything is connected");
        assert!(err.contains("nothing is left"), "{err}");
    }

    /// findings p5 F4-5: a pair started on a pad with a stub takes the stub's width (`inheritTrackWidth`), not the class's.
    #[test]
    fn a_pair_started_on_a_pad_with_a_stub_takes_the_stubs_width() {
        let (mut design, mut model) = two_pad_board();
        model.nets = vec![IrNet { name: "USB_DP".into(), pins: vec!["R1.1".into(), "R2.1".into()] }, IrNet { name: "USB_DN".into(), pins: vec!["R1.2".into(), "R2.2".into()] }];
        design.routing.as_mut().unwrap().tracks.push(eda_model::ir::Track { id: "stub".into(), net: "USB_DP".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.15), pts: vec![Point { x: mm(-0.825), y: Nm::ZERO }, Point { x: mm(-0.825), y: mm(-2.0) }], arc_mid_offset: None });
        let mut router = Router::new(&design, &model);
        router.start_diff_pair(Point { x: mm(-0.825), y: Nm::ZERO }, "F.Cu").expect("R1.1 and R1.2 are a pair");
        assert_eq!(router.diff_pair_preview(Point { x: mm(3.0), y: Nm::ZERO }).expect("preview").width, mm(0.15));
    }

    /// findings p5 F4-6: a pair started where nothing is gets KiCad's own refusal.
    #[test]
    fn a_pair_started_in_the_middle_of_nowhere_is_refused_with_kicads_message() {
        let (design, model) = two_pad_board();
        let mut router = Router::new(&design, &model);
        assert_eq!(router.start_diff_pair(Point { x: mm(50.0), y: mm(50.0) }, "F.Cu"), Err("Cannot start a differential pair in the middle of nowhere.".to_string()));
    }

    /// findings p5 F3-12: a drag keeps the mode it started in (`m_currentMode = Settings().Mode()` in `DRAGGER::Start`).
    #[test]
    fn a_drag_keeps_the_mode_it_started_in() {
        let (design, model) = track_and_via_board();
        let mut router = Router::new(&design, &model);
        router.settings.mode = crate::settings::Mode::MarkObstacles;
        router.drag_start(Point { x: mm(1.0), y: Nm::ZERO }, "F.Cu").expect("the track's end");
        router.settings.mode = crate::settings::Mode::Shove;
        // dragged onto the GND via: marked, not shoved
        let pv = router.drag_preview(Point { x: mm(5.0), y: Nm::ZERO }).unwrap();
        assert!(pv.colliding && pv.displaced_vias.is_empty(), "{pv:?}");
    }

    /// findings p5 F2-9: AttemptFinish routes to the nearest unconnected anchor of the net (`ROUTER::Finish`), not to the cursor.
    #[test]
    fn attempt_finish_routes_to_the_ratsnest_anchor() {
        let (design, model) = two_pad_board();
        let mut router = Router::new(&design, &model);
        router.start(Point { x: mm(-0.825), y: Nm::ZERO }, "F.Cu", mm(0.2)).unwrap();
        let cursor = Point { x: mm(1.0), y: mm(2.0) };
        router.preview(cursor);
        let commit = router.attempt_finish(cursor).expect("R2.1 is free to reach");
        let end = commit.tracks.last().and_then(|t| t.pts.last().copied());
        assert_eq!(end, Some(Point { x: mm(6.0) - mm(0.825), y: Nm::ZERO }), "ends on R2.1: {commit:?}");
        assert!(!router.is_routing());
        // a wall the walk cannot get round: the route goes on, and the cursor goes to where the head got
        let (mut design, model) = two_pad_board();
        design.routing.as_mut().unwrap().tracks.push(eda_model::ir::Track { id: "wall".into(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: mm(2.5), y: mm(-40.0) }, Point { x: mm(2.5), y: mm(40.0) }], arc_mid_offset: None });
        let mut router = Router::new(&design, &model);
        router.start(Point { x: mm(-0.825), y: Nm::ZERO }, "F.Cu", mm(0.2)).unwrap();
        router.preview(cursor);
        let warp = router.attempt_finish(cursor).expect_err("blocked");
        assert!(warp.x < mm(2.5), "short of the wall: {warp:?}");
        assert!(router.is_routing(), "the route goes on");
    }

    /// findings p5 F2-8 / P-2: with nothing under the cursor the route starts in empty space on no net (`m_currentNet =
    /// GetOrphanedNetHandle()`) and commits a track on no net.
    #[test]
    fn start_in_empty_space_routes_an_unconnected_track() {
        let (design, model) = two_pad_board();
        let mut router = Router::new(&design, &model);
        let at = Point { x: mm(50.0), y: mm(50.0) };
        router.start(at, "F.Cu", mm(0.2)).expect("a route in empty space");
        assert!(router.is_routing() && router.current_net().is_none());
        router.preview(Point { x: mm(53.0), y: mm(50.0) });
        let commit = router.finish(Point { x: mm(53.0), y: mm(50.0) }).expect("nothing in the way");
        assert_eq!(commit.tracks.len(), 1);
        assert_eq!((commit.tracks[0].net.as_str(), commit.tracks[0].pts.first().copied()), ("", Some(at)));
    }

    /// A track routed in empty space is on no net, and so is what starts from its end: the two join (KiCad's no-net items share
    /// `OrphanedItem()`), and a later no-net route next to it is no violation of it.
    #[test]
    fn a_route_from_the_end_of_a_no_net_track_joins_it() {
        let (mut design, model) = two_pad_board();
        let mut router = Router::new(&design, &model);
        let (a, b) = (Point { x: mm(50.0), y: mm(50.0) }, Point { x: mm(53.0), y: mm(50.0) });
        router.start(a, "F.Cu", mm(0.2)).unwrap();
        router.preview(b);
        let commit = router.finish(b).unwrap();
        design.routing.as_mut().unwrap().tracks.extend(commit.tracks);
        design.assign_missing_ids();
        let mut router = Router::new(&design, &model);
        router.start(b, "F.Cu", mm(0.2)).expect("the end of the no-net track");
        assert!(router.current_net().is_none());
        let c = Point { x: mm(53.0), y: mm(53.0) };
        let pv = router.preview(c).unwrap();
        assert!(!pv.colliding && pv.head.first() == Some(b), "{pv:?}");
        // a route of no net along it, 100 um away: no collision
        router.start(Point { x: mm(50.0), y: mm(50.3) }, "F.Cu", mm(0.2)).expect("empty space");
        let pv = router.preview(Point { x: mm(53.0), y: mm(50.3) }).unwrap();
        assert!(!pv.colliding && pv.head.pts.len() == 2, "{pv:?}");
    }

    /// findings p5 F2-8 / F3-4: `isStartingPointRoutable` refuses a start whose dot of track collides at the route's width and at the board's
    /// minimum width -- unless violations are allowed (Highlight collisions with "Allow DRC violations").
    #[test]
    fn a_start_point_that_violates_drc_is_refused() {
        let (mut design, model) = track_and_via_board();
        // a GND track ending 300 um from the end of the SIG track: 100 um of copper gap at 200 um widths, under the 200 um clearance
        design.routing.as_mut().unwrap().tracks.push(eda_model::ir::Track { id: "trkG".into(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: mm(1.3), y: Nm::ZERO }, Point { x: mm(1.3), y: mm(3.0) }], arc_mid_offset: None });
        let mut router = Router::new(&design, &model);
        assert_eq!(router.start(Point { x: mm(1.0), y: Nm::ZERO }, "F.Cu", mm(0.2)), Err("The routing start point violates DRC.".to_string()));
        assert!(!router.is_routing());
        router.settings = RoutingSettings { mode: crate::settings::Mode::MarkObstacles, can_violate_drc: true, ..RoutingSettings::default() };
        assert!(router.start(Point { x: mm(1.0), y: Nm::ZERO }, "F.Cu", mm(0.2)).is_ok());
    }

    #[test]
    fn full_session_start_move_fix_finish_produces_a_track() {
        let (design, model) = two_pad_board();
        let mut router = Router::new(&design, &model);
        // R1 pin 1 local pos for a two_pad 0603 footprint is (-825, 0); R2 is at board (6000,0) so its pin 1 is at (6000-825,0)=(5175,0).
        let start_pos = Point { x: mm(-0.825), y: Nm::ZERO };
        router.start(start_pos, "F.Cu", mm(0.2)).expect("must start on R1.1");
        assert!(router.is_routing());
        let pv = router.preview(Point { x: mm(5.175), y: Nm::ZERO }).expect("preview while routing");
        assert!(!pv.colliding);
        let commit = router.finish(Point { x: mm(5.175), y: Nm::ZERO }).expect("must finish onto R2.1");
        assert!(!router.is_routing());
        assert_eq!(commit.tracks.len(), 1);
        assert_eq!(commit.tracks[0].net, "SIG");
        assert_eq!(commit.tracks[0].layer, "F.Cu");
        assert_eq!(commit.tracks[0].pts.first(), Some(&start_pos));
        assert_eq!(commit.tracks[0].pts.last(), Some(&Point { x: mm(5.175), y: Nm::ZERO }));
    }

    /// Track Corner Mode arc 45 (`ROUNDED_45`): the corner of a route is an arc track of the commit, between two polylines.
    #[test]
    fn a_rounded_corner_is_committed_as_an_arc_track() {
        let (design, model) = two_pad_board();
        let mut router = Router::new(&design, &model);
        router.settings.corner_mode = crate::direction45::CornerMode::Rounded45;
        router.start(Point { x: mm(-0.825), y: Nm::ZERO }, "F.Cu", mm(0.2)).unwrap();
        router.fix(Point { x: mm(3.0), y: mm(-2.0) }).expect("a route is in progress");
        let commit = router.finish(Point { x: mm(5.175), y: Nm::ZERO }).expect("must finish");
        let arcs: Vec<_> = commit.tracks.iter().filter(|t| t.arc().is_some()).collect();
        assert_eq!(arcs.len(), 2, "one arc a leg: {:?}", commit.tracks.iter().map(|t| (t.pts.len(), t.arc_mid_offset)).collect::<Vec<_>>());
        // the pieces join end to end
        let ends: Vec<(Point, Point)> = commit.tracks.iter().map(|t| (t.pts[0], *t.pts.last().unwrap())).collect();
        for (a, b) in &ends {
            assert!(ends.iter().filter(|(c, d)| c == b || d == b).count() >= 1 && ends.iter().any(|(c, d)| (c, d) != (a, b) && (c == a || d == a || c == b || d == b)), "{ends:?}");
        }
    }

    /// D1: intermediate fix clicks must not emit vias.
    #[test]
    fn multi_click_route_emits_no_fake_vias() {
        let (design, model) = two_pad_board();
        let mut router = Router::new(&design, &model);
        router.start(Point { x: mm(-0.825), y: Nm::ZERO }, "F.Cu", mm(0.2)).unwrap();
        router.fix(Point { x: mm(2.0), y: Nm::ZERO });
        router.fix(Point { x: mm(2.0), y: mm(1.0) });
        let commit = router.finish(Point { x: mm(5.175), y: Nm::ZERO }).expect("must finish");
        // (the walk round R1.2 ends the first leg going straight down onto the second: the line of the three legs loses that point and is
        // one track, `simplifyNewLine`)
        assert!(!commit.tracks.is_empty(), "{:?}", commit.tracks);
        assert!(!commit.tracks.iter().any(|t| t.pts.contains(&Point { x: mm(2.0), y: Nm::ZERO })), "the straight-on click point is gone: {:?}", commit.tracks);
        assert!(commit.vias.is_empty(), "got vias: {:?}", commit.vias);
    }

    #[test]
    fn cancel_drops_the_session_without_a_commit() {
        let (design, model) = two_pad_board();
        let mut router = Router::new(&design, &model);
        router.start(Point { x: mm(-0.825), y: Nm::ZERO }, "F.Cu", mm(0.2)).unwrap();
        router.cancel();
        assert!(!router.is_routing());
        assert!(router.finish(Point { x: mm(5.175), y: Nm::ZERO }).is_none());
    }

    #[test]
    fn diff_pair_dimensions_change_the_running_pair() {
        let (design, mut model) = two_pad_board();
        model.nets = vec![IrNet { name: "USB_DP".into(), pins: vec!["R1.1".into(), "R2.1".into()] }, IrNet { name: "USB_DN".into(), pins: vec!["R1.2".into(), "R2.2".into()] }];
        let mut router = Router::new(&design, &model);
        assert!(!router.set_diff_pair_dimensions(Some(mm(0.2)), Some(mm(0.3))), "no pair is running yet");
        router.start_diff_pair(Point { x: mm(-0.825), y: Nm::ZERO }, "F.Cu").expect("R1.1 and R1.2 are a pair");
        let at = Point { x: mm(3.0), y: Nm::ZERO };
        let before = router.diff_pair_preview(at).expect("preview");
        // no class: KiCad's Default class sizes, 200 um wide, 250 um apart
        assert_eq!(before.width, mm(0.2), "the net class's own width until the dialog says otherwise");
        let pitch_of = |p: &crate::dp_placer::DiffPairPreview| {
            let (a, b) = (p.head_a.last().unwrap(), p.head_b.last().unwrap());
            (((a.x - b.x).as_f64()).powi(2) + ((a.y - b.y).as_f64()).powi(2)).sqrt()
        };
        // the pair ends a pitch apart (the gateways' offsets are whole micrometres: within a few of it)
        let near = |actual: f64, pitch: i64| actual >= pitch as f64 - 10.0 && actual <= pitch as f64 + 10.0;
        assert!(near(pitch_of(&before), 200_000 + 250_000), "{}", pitch_of(&before));
        assert!(router.set_diff_pair_dimensions(Some(mm(0.2)), Some(mm(0.3))));
        let after = router.diff_pair_preview(at).expect("preview");
        assert_eq!(after.width, mm(0.2));
        assert!(near(pitch_of(&after), 200_000 + 300_000), "the lines sit width + gap apart: {}", pitch_of(&after));
        // Zero or negative values are ignored (the dialog refuses a gap of 0).
        assert!(router.set_diff_pair_dimensions(Some(Nm::ZERO), Some(Nm::new(-5000))));
        assert_eq!(router.diff_pair_preview(at).unwrap().width, mm(0.2));
        // `None` leaves a value as it is.
        router.set_diff_pair_dimensions(None, Some(mm(0.4)));
        assert!(near(pitch_of(&router.diff_pair_preview(at).unwrap()), 200_000 + 400_000));
    }

    #[test]
    fn undo_last_segment_on_a_fresh_session_is_a_safe_no_op() {
        let (design, model) = two_pad_board();
        let mut router = Router::new(&design, &model);
        router.start(Point { x: mm(-0.825), y: Nm::ZERO }, "F.Cu", mm(0.2)).unwrap();
        assert!(!router.undo_last_segment());
    }
}
