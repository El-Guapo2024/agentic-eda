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
use crate::line_placer::{FixOutcome, LinePlacer, Preview};
use crate::node::Node;
use crate::settings::RoutingSettings;
use eda_model::ir::{Design, Point, Um};
use eda_model::ConstraintModel;

/// `ANCHOR_SNAP_UM`-equivalent used for picking a *start* item -- see
/// `line_placer::SNAP_UM` for the same radius used at the route's end.
const START_SNAP_UM: Um = 500;

struct Session {
    placer: LinePlacer,
    /// Set by `toggle_via`: the layer the *next* fix should switch to
    /// (dropping a via there first) -- Router-level orchestration, not
    /// `LinePlacer`'s own concern (it only knows "append a via to the
    /// head," not which layer comes next).
    pending_via_layer: Option<i32>,
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
}

impl Router {
    pub fn new(design: &Design, model: &ConstraintModel) -> Self {
        let (world, layers) = crate::from_ir::build_node(design, model);
        Router { world, layers, rules: model.board.clone(), settings: RoutingSettings::default(), session: None, drag: None, diff: None }
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

    /// `ROUTER::StartRouting`: pick a starting net from whatever's at `at`
    /// (a pad/via/track end within [`START_SNAP_UM`]) -- this port requires
    /// starting on a connectable item (a real net), unlike upstream, which
    /// also allows starting from bare space onto an orphaned net; starting
    /// mid-air has no obvious net to resolve to in this project's model and
    /// isn't a mode the task asked for. Replaces any route already in
    /// progress (matching `StopRouting()` + fresh `StartRouting()`, not an
    /// error).
    pub fn start(&mut self, at: Point, layer_name: &str, width: Um) -> Result<(), String> {
        let layer = self.layer_index(layer_name).ok_or_else(|| format!("unknown layer {layer_name:?}"))?;
        let hit = self.world.nearest_anchor(at, LayerRange::single(layer), START_SNAP_UM, None);
        let Some((start_item, start_pos)) = hit else {
            return Err("no pad, via, or track end there to route from".into());
        };
        let Some(net) = self.world.get(start_item).map(|i| i.net().clone()).filter(|n| n.is_some()) else {
            return Err("that item has no net to route".into());
        };
        self.drag = None;
        self.diff = None;
        self.session = Some(Session { placer: LinePlacer::start(&self.world, start_pos, Some(start_item), net, layer, width), pending_via_layer: None });
        Ok(())
    }

    fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    /// `ROUTER::Move`/`movePlacing`: the live preview. `None` if no route
    /// is in progress.
    pub fn preview(&self, at: Point) -> Option<Preview> {
        self.session().map(|s| s.placer.preview(&self.world, &self.rules, &self.settings, at))
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
        if let Some(to_layer) = s.pending_via_layer.take() {
            let (diameter, drill) = (s.placer.via_diameter, s.placer.via_drill);
            s.placer.switch_layer(&self.world, &self.rules, &self.settings, at, to_layer, diameter, drill);
            return Some(FixOutcome::Fixed { real_end: false });
        }
        Some(s.placer.fix(&self.world, &self.rules, &self.settings, at))
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
        let runs = self.session.as_mut()?.placer.finish(&self.world, &self.rules, &self.settings, at)?;
        let commit = self.build_commit(&net_name, runs, layer, width);
        self.session = None;
        Some(commit)
    }

    fn build_commit(&self, net: &Net, runs: Vec<Line>, _layer: i32, _width: Um) -> RouteCommit {
        let mut commit = RouteCommit::default();
        let net_name = net.as_deref().unwrap_or("").to_string();
        for run in &runs {
            if run.point_count() < 2 {
                continue;
            }
            commit.tracks.push(eda_model::ir::Track { id: String::new(), net: net_name.clone(), pins: Vec::new(), layer: self.layer_name(run.layer).to_string(), width: run.width, pts: run.pts.clone() });
        }
        // Vias this session placed sit at the shared endpoint between two
        // consecutive runs (see `LinePlacer::switch_layer`'s doc comment);
        // reconstruct them from the run boundaries rather than threading a
        // separate via list through `LinePlacer`, since a run's own first/
        // last point already says exactly where one must be.
        if let Some(s) = self.session() {
            for w in runs.windows(2) {
                if w[0].last() == w[1].first() {
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
            for (track_id, line) in s.placer.displaced_tracks() {
                commit.remove_track_ids.push(track_id.to_string());
                if line.point_count() >= 2 {
                    commit.tracks.push(eda_model::ir::Track { id: String::new(), net: self.net_name_of(line), pins: Vec::new(), layer: self.layer_name(line.layer).to_string(), width: line.width, pts: line.pts.clone() });
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
        let layer = self.layer_index(layer_name).ok_or_else(|| format!("unknown layer {layer_name:?}"))?;
        let item_id = self.world.item_at(at, LayerRange::single(layer), START_SNAP_UM).ok_or("nothing to drag there")?;
        let dragger = crate::dragger::Dragger::start(&self.world, at, item_id).ok_or("that item can't be dragged (only track segments/corners and vias can)")?;
        self.session = None;
        self.diff = None;
        self.drag = Some(dragger);
        Ok(())
    }

    /// `ROUTER::Move`/`moveDragging`: the live drag preview.
    pub fn drag_preview(&self, at: Point) -> Option<crate::dragger::DragPreview> {
        self.drag.as_ref().map(|d| d.preview(&self.world, &self.rules, &self.settings, at))
    }

    pub fn drag_cancel(&mut self) {
        self.drag = None;
    }

    /// `DRAGGER::FixRoute`: commit the drag at `at`, converting its
    /// `DragCommit` into the same [`RouteCommit`] shape a finished route
    /// produces (one `Cmd::CommitRoute` seam for both -- see that type's
    /// own doc comment). `None` if still colliding and the mode refuses
    /// to commit it (the drag session stays open either way, so the
    /// caller can keep adjusting or explicitly cancel).
    pub fn drag_finish(&mut self, at: Point) -> Option<RouteCommit> {
        let drag_commit = self.drag.as_ref()?.finish(&self.world, &self.rules, &self.settings, at)?;
        let mut commit = RouteCommit { remove_track_ids: drag_commit.remove_track_ids, remove_via_ids: drag_commit.remove_via_ids, ..RouteCommit::default() };
        for line in &drag_commit.tracks {
            if line.point_count() < 2 {
                continue;
            }
            commit.tracks.push(eda_model::ir::Track { id: String::new(), net: line.net.as_deref().unwrap_or("").to_string(), pins: Vec::new(), layer: self.layer_name(line.layer).to_string(), width: line.width, pts: line.pts.clone() });
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
                commit.tracks.push(eda_model::ir::Track { id: String::new(), net: net_name.clone(), pins: Vec::new(), layer: layer_name.clone(), width, pts: run.pts.clone() });
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
            footprint_library: None, sheet_contents: None, bus_aliases: vec![],
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
            footprint_library: None, sheet_contents: None, bus_aliases: vec![],
            schema: 1,
            provenance: Provenance { engine_version: "test".into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection { outline: vec![], footprints: vec![], modules: vec![] }),
            routing: Some(RoutingSection {
                tracks: vec![eda_model::ir::Track { id: "trkA".into(), net: "SIG".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }] }],
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
        assert_eq!(commit.tracks[0].pts, vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 2000 }]);
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
    fn undo_last_segment_on_a_fresh_session_is_a_safe_no_op() {
        let (design, model) = two_pad_board();
        let mut router = Router::new(&design, &model);
        router.start(Point { x: -825, y: 0 }, "F.Cu", 200).unwrap();
        assert!(!router.undo_last_segment());
    }
}
