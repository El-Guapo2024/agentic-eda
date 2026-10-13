//! HTTP handlers for the interactive router (`eda_pns`, gap #7's stage 4):
//! `POST /api/route/{start,move,fix,undo_segment,via,finish,cancel}`.
//!
//! One `RouteCell` (a `Mutex<Option<Router>>`, set up once in
//! `studio::serve` alongside its sibling `Job`/`glb_job` cells) holds the
//! in-progress session across the whole gesture: `start` builds a fresh
//! `eda_pns::Router` from the board as it stands right now and stores it;
//! `move`/`fix`/`undo_segment`/`via` all just borrow it back out of the
//! mutex and forward into its methods, so none of them re-read or
//! re-flatten the board -- the one thing the task's "keep per-request work
//! small" instruction is most worried about. `finish`/`cancel` take the
//! session back out of the cell, ending it either way.
//!
//! Preview state never touches `design.json`: only `finish`'s
//! `Cmd::CommitRoute` (`crates/ops/src/lib.rs`) goes through
//! `board::step`, the same single seam every other board edit uses, so it
//! gets the same undo-stack/activity-log treatment as a hand-typed `Cmd`.

use crate::board;
use eda_model::ir::Point;
use eda_pns::line_placer::{FixOutcome, Preview};
use eda_pns::router::Router;
use eda_pns::settings::{Mode, OptEffort, RoutingSettings};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Mutex;

pub type RouteCell = Mutex<Option<Router>>;

fn body_json(body: &[u8]) -> Value {
    serde_json::from_slice(body).unwrap_or(Value::Null)
}

fn num(v: &Value, key: &str, default: i64) -> i64 {
    v.get(key).and_then(Value::as_i64).unwrap_or(default)
}

fn point_of(v: &Value) -> Point {
    Point { x: num(v, "x", 0), y: num(v, "y", 0) }
}

fn pts_json(pts: &[Point]) -> Value {
    json!(pts.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>())
}

fn err(message: impl Into<String>) -> Value {
    json!({ "ok": false, "message": message.into() })
}

/// `req.mode` ("mark_obstacles" | "walkaround" | "shove"), defaulting to
/// `Mode::Walkaround` when absent or unrecognized -- KiCad's own default,
/// and shared by `start`/`drag_start` so a route session and a drag
/// session read the one `RoutingSettings::Mode` field the same way (see
/// `web/studio/src/components/RouterSettingsDialog.tsx`, the frontend's
/// one place that actually lets a person choose it).
fn mode_of(req: &Value) -> Mode {
    match req.get("mode").and_then(Value::as_str) {
        Some("mark_obstacles") => Mode::MarkObstacles,
        Some("shove") => Mode::Shove,
        _ => Mode::Walkaround,
    }
}

/// The router's `ROUTING_SETTINGS` a request carries: `req.settings`, an object with any of
///
/// - `mode` (`"mark_obstacles" | "walkaround" | "shove"`), `optimizer_effort` (`"low" | "medium" | "full"`),
/// - the Interactive Router Settings dialog's switches `shove_vias`, `jump_over_obstacles`, `remove_loops`, `smart_pads`,
///   `allow_drc_violations` (`CanViolateDRC`), `free_angle_mode`, `fix_all_segments`, `smooth_dragged_segments`, and
///   `snap_to_tracks` (the magnetic setting `checkSnap` hands the router, not a row of the dialog),
/// - the limits `shove_iteration_limit`, `shove_time_limit` (milliseconds), `walkaround_iteration_limit`,
///   `via_force_prop_iteration_limit` and `walkaround_hug_length_threshold`, which KiCad keeps in the settings file only.
///
/// Anything absent is KiCad's default (`RoutingSettings::default()`); `mode` and `remove_loops` also come from the request's
/// own top level, where they used to be the only two. Read field by field: serde_json's `arbitrary_precision` (starlark turns
/// it on) breaks a derived reader on numbers.
fn settings_of(req: &Value) -> RoutingSettings {
    settings_over(req, RoutingSettings::default())
}

/// [`settings_of`] over `base`: a switch or limit the request does not carry keeps `base`'s value (`mode` and `optimizer_effort`
/// are always read, defaulting as `settings_of` does). A live session takes the dialog's OK this way, so what the dialog has no row
/// for -- `snap_to_tracks`, the magnetic setting the studio sent at the start -- stays as it was.
fn settings_over(req: &Value, base: RoutingSettings) -> RoutingSettings {
    let mut s = base;
    let obj = req.get("settings").filter(|v| v.is_object()).unwrap_or(&Value::Null);
    let get = |key: &str| obj.get(key).or_else(|| req.get(key));
    let flag = |key: &str, into: &mut bool| {
        if let Some(b) = get(key).and_then(Value::as_bool) {
            *into = b;
        }
    };
    let limit = |key: &str, into: &mut i32| {
        if let Some(n) = get(key).and_then(Value::as_i64) {
            *into = n.clamp(1, 100_000) as i32;
        }
    };
    s.mode = mode_of(if obj.get("mode").is_some() { obj } else { req });
    s.optimizer_effort = match get("optimizer_effort").and_then(Value::as_str) {
        Some("low") => OptEffort::Low,
        Some("full") => OptEffort::Full,
        _ => OptEffort::Medium,
    };
    flag("shove_vias", &mut s.shove_vias);
    flag("jump_over_obstacles", &mut s.jump_over_obstacles);
    flag("remove_loops", &mut s.remove_loops);
    flag("smart_pads", &mut s.smart_pads);
    flag("allow_drc_violations", &mut s.can_violate_drc);
    flag("free_angle_mode", &mut s.free_angle_mode);
    flag("fix_all_segments", &mut s.fix_all_segments);
    flag("smooth_dragged_segments", &mut s.smooth_dragged_segments);
    flag("snap_to_tracks", &mut s.snap_to_tracks);
    limit("shove_iteration_limit", &mut s.shove_iteration_limit);
    limit("shove_time_limit", &mut s.shove_time_limit_ms);
    limit("walkaround_iteration_limit", &mut s.walkaround_iteration_limit);
    limit("via_force_prop_iteration_limit", &mut s.via_force_prop_iteration_limit);
    if let Some(t) = get("walkaround_hug_length_threshold").and_then(Value::as_f64).filter(|t| t.is_finite()) {
        s.walkaround_hug_length_threshold = t.clamp(0.0, 1000.0);
    }
    s
}

fn preview_json(router: &Router, preview: &Preview) -> Value {
    json!({
        "ok": true,
        "net": router.current_net(),
        "colliding": preview.colliding,
        "layer": router.layer_name(preview.head.layer),
        "head": pts_json(&preview.head.pts),
        "runs": preview.runs.iter().map(|r| json!({ "layer": router.layer_name(r.layer), "pts": pts_json(&r.pts) })).collect::<Vec<_>>(),
        "via": preview.via.map(|(p, diameter, drill)| json!({ "x": p.x, "y": p.y, "diameter": diameter, "drill": drill })),
        "snapped_end": preview.snapped_end.map(|p| json!([p.x, p.y])),
        "displaced": preview.displaced_lines.iter().map(|d| json!({ "source_track": d.source_track, "layer": router.layer_name(d.line.layer), "pts": pts_json(&d.line.pts) })).collect::<Vec<_>>(),
        // `Preview::displaced_vias` existed on the Rust side since stage 3
        // (shove) but was never actually serialized here -- a shove-mode
        // route preview that would push a via out of the way never showed
        // that via moving until the route was actually finished. The
        // board still carries that via's real diameter/drill (it isn't
        // removed until commit), so the frontend looks those up by id
        // itself rather than this needing to repeat them.
        "displaced_vias": preview.displaced_vias.iter().map(|d| json!({ "source_via": d.source_via, "x": d.pos.x, "y": d.pos.y })).collect::<Vec<_>>(),
    })
}

/// `POST /api/route/start`: `{x, y, layer, width?, settings?}`. `settings` is the router's
/// `ROUTING_SETTINGS` ([`settings_of`]); `mode` and `remove_loops` at the top level still work, for a client that predates it.
/// Without any, KiCad's defaults: `walkaround`, `remove_loops` on.
pub fn start(dir: &Path, cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let (_, design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let mut router = Router::new(&design, &model);
    router.settings = settings_of(&req);
    let at = point_of(&req);
    let layer = req.get("layer").and_then(Value::as_str).unwrap_or("F.Cu");
    let width = num(&req, "width", model.board.track_width);
    match router.start(at, layer, width) {
        Ok(()) => {
            let reply = router.preview(at).map(|p| preview_json(&router, &p)).unwrap_or_else(|| err("internal: started but no preview"));
            *cell.lock().unwrap_or_else(|e| e.into_inner()) = Some(router);
            reply
        }
        Err(message) => err(message),
    }
}

/// `POST /api/route/move`: `{x, y, flip_posture?, width?}` -- the `/`
/// posture toggle and `W` width-cycle hotkeys fold in here (see
/// `Router::flip_posture`'s own doc comment on why).
pub fn mv(cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    let Some(router) = guard.as_mut() else { return err("not routing") };
    if req.get("flip_posture").and_then(Value::as_bool).unwrap_or(false) {
        router.flip_posture();
    }
    if let Some(w) = req.get("width").and_then(Value::as_i64) {
        router.set_width(w);
    }
    let at = point_of(&req);
    router.preview(at).map(|p| preview_json(router, &p)).unwrap_or_else(|| err("not routing"))
}

/// `POST /api/route/fix`: `{x, y}`.
pub fn fix(cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    let Some(router) = guard.as_mut() else { return err("not routing") };
    let at = point_of(&req);
    match router.fix(at) {
        Some(FixOutcome::Fixed { real_end }) => {
            let preview = router.preview(at).map(|p| preview_json(router, &p)).unwrap_or_else(|| json!({}));
            json!({ "ok": true, "blocked": false, "real_end": real_end, "preview": preview })
        }
        Some(FixOutcome::Blocked) => json!({ "ok": true, "blocked": true, "message": "still colliding; not fixed" }),
        None => err("not routing"),
    }
}

/// `POST /api/route/undo_segment` (Backspace): no body.
pub fn undo_segment(cell: &RouteCell) -> Value {
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    let Some(router) = guard.as_mut() else { return err("not routing") };
    let popped = router.undo_last_segment();
    json!({ "ok": true, "popped": popped })
}

/// `POST /api/route/via`: `{enabled, diameter?, drill?, to_layer?}` --
/// arms (or disarms) "drop a via at the next fix and continue on
/// `to_layer`", the live via-placement-with-layer-switch gesture.
pub fn via(cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    let Some(router) = guard.as_mut() else { return err("not routing") };
    let enabled = req.get("enabled").and_then(Value::as_bool).unwrap_or(true);
    let diameter = num(&req, "diameter", 600);
    let drill = num(&req, "drill", 300);
    let to_layer = req.get("to_layer").and_then(Value::as_str).and_then(|n| router.layer_index(n));
    router.toggle_via(enabled, diameter, drill, to_layer);
    json!({ "ok": true })
}

/// `POST /api/route/finish`: `{x, y}`. Ends the session either way (a
/// refusal leaves nothing committed, same as a plain `fix` refusal) --
/// matching upstream `CommitRouting`'s "nothing placed is a no-op, but the
/// tool still stops" shape would need the caller to retry `start` rather
/// than silently continuing a half-finished session.
pub fn finish(dir: &Path, cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let mut router = match cell.lock().unwrap_or_else(|e| e.into_inner()).take() {
        Some(r) => r,
        None => return err("not routing"),
    };
    let at = point_of(&req);
    let Some(commit) = router.finish(at) else {
        return json!({ "ok": false, "message": "final segment still collides; route left uncommitted" });
    };
    let cmd = eda_ops::Cmd::CommitRoute { remove_track_ids: commit.remove_track_ids, remove_via_ids: commit.remove_via_ids, tracks: commit.tracks, vias: commit.vias };
    match board::step(dir, cmd, true, "ui") {
        Ok(summary) => {
            // a route restarted from the other end is one undo with the part that was committed when it restarted
            if router.append_undo {
                board::append_to_previous_undo(dir);
            }
            json!({ "ok": true, "message": summary })
        }
        Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
    }
}

/// `POST /api/route/continue_from_end`: `{x, y}` -- `ROUTER_TOOL::routerContinueFromEnd` (Ctrl+E, Route From Other End): what the
/// route has fixed so far is committed (`CommitRouting()`; one `Cmd::CommitRoute`, nothing at all if nothing was fixed), and a new
/// route starts at the far end of the connection and heads for where the old head ended (`ROUTER::ContinueFromEnd`: `StartRouting`,
/// `Move( currentEnd )`). The reply is the new route's preview (as `start`'s), plus `committed` (the board changed) and `from`.
/// A refusal ("nothing is left to connect to") leaves the session as it was.
pub fn continue_from_end(dir: &Path, cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let at = point_of(&req);
    let (continuation, settings, append_undo) = {
        let guard = cell.lock().unwrap_or_else(|e| e.into_inner());
        let Some(router) = guard.as_ref() else { return err("not routing") };
        match router.continue_from_end(at) {
            Ok(c) => (c, router.settings.clone(), router.append_undo),
            Err(message) => return err(message),
        }
    };
    let committed = !continuation.commit.tracks.is_empty() || !continuation.commit.vias.is_empty() || !continuation.commit.remove_track_ids.is_empty();
    if committed {
        let c = continuation.commit;
        let cmd = eda_ops::Cmd::CommitRoute { remove_track_ids: c.remove_track_ids, remove_via_ids: c.remove_via_ids, tracks: c.tracks, vias: c.vias };
        if let Err(e) = board::step(dir, cmd, true, "ui") {
            return err(board::reasons(&e));
        }
        // a chain of restarts is one undo: this commit joins the one before it
        if append_undo {
            board::append_to_previous_undo(dir);
        }
    }
    // the board as it stands now, with what was fixed in it
    let (_, design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let mut router = Router::new(&design, &model);
    router.settings = settings;
    // what the new session commits is one undo step with this commit (`APPEND_UNDO`), and with the ones before it
    router.append_undo = append_undo || committed;
    if let Err(message) = router.start(continuation.from, &continuation.layer, continuation.width) {
        *cell.lock().unwrap_or_else(|e| e.into_inner()) = None;
        return json!({ "ok": false, "committed": committed, "message": message });
    }
    let reply = router.preview(continuation.to).map(|p| preview_json(&router, &p)).unwrap_or_else(|| err("internal: restarted but no preview"));
    *cell.lock().unwrap_or_else(|e| e.into_inner()) = Some(router);
    let mut reply = reply;
    if let Some(obj) = reply.as_object_mut() {
        obj.insert("committed".into(), json!(committed));
        obj.insert("from".into(), json!([continuation.from.x, continuation.from.y]));
    }
    reply
}

/// `POST /api/route/cancel`: no body. Drops whatever session is active --
/// a route (`Router::session`) or a drag (`Router::drag`) alike, since
/// both live on the one `Router` this cell holds -- without touching the
/// board.
pub fn cancel(cell: &RouteCell) -> Value {
    *cell.lock().unwrap_or_else(|e| e.into_inner()) = None;
    json!({ "ok": true })
}

/// `POST /api/route/mode`: `{mode}` (`"mark_obstacles" | "walkaround" | "shove"`) -- `ROUTER_TOOL::ChangeRouterMode` /
/// `CycleRouterMode` (`settings.SetMode( mode )`) applied to the session that is running right now (route, drag or diff pair), so
/// the next `move` already uses it. With no session there is nothing to change (the studio keeps the mode for the next one).
pub fn set_mode(cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    let Some(router) = guard.as_mut() else { return err("not routing") };
    router.settings.mode = mode_of(&req);
    json!({ "ok": true })
}

/// `POST /api/route/settings`: `{settings}` ([`settings_of`]) -- Interactive Router Settings, OK, applied to the session
/// that is running right now (route, drag or diff pair), so the next `move` already uses them, as upstream's dialog does
/// (`ROUTER_TOOL::...` hands the changed `ROUTING_SETTINGS` to the live router). With no session there is nothing to
/// change (the studio keeps the settings for the next one).
pub fn set_settings(cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    let Some(router) = guard.as_mut() else { return err("not routing") };
    router.settings = settings_over(&req, router.settings.clone());
    json!({ "ok": true })
}

// --------------------------------------------------------------- dragging (stage 5)
//
// D on a track segment/corner or via: keeps its connections while moving
// it, pushing anything in the way aside in `shove` mode (`eda_pns::
// dragger::Dragger`). Shares `RouteCell`/`Router` with the route session
// above -- `Router` itself keeps the two mutually exclusive -- rather than
// a second cell, so there is exactly one "what's this board session doing
// right now" slot to reason about.

fn drag_kind_name(kind: Option<eda_pns::dragger::DragKind>) -> &'static str {
    match kind {
        Some(eda_pns::dragger::DragKind::Segment) => "segment",
        Some(eda_pns::dragger::DragKind::Via) => "via",
        _ => "corner",
    }
}

fn drag_preview_json(router: &Router, preview: &eda_pns::dragger::DragPreview) -> Value {
    json!({
        "ok": true,
        // `DM_SEGMENT` (the whole segment slides), `DM_CORNER` (a vertex follows the cursor) or `DM_VIA`
        "drag": drag_kind_name(router.drag_kind()),
        "colliding": preview.colliding,
        // the cursor is somewhere the drag cannot be resolved: `pts` is the last position that could (`DRAGGER::Drag`, "Restore last solution")
        "restored": preview.restored,
        // the first position failed, so the drag only marks obstacles; a violation then needs Ctrl+click (`GetForceMarkObstaclesMode`)
        "forced_mark_obstacles": preview.forced_mark_obstacles,
        "pts": pts_json(&preview.pts),
        "displaced": preview.displaced_lines.iter().map(|d| json!({ "source_track": d.source_track, "layer": router.layer_name(d.line.layer), "pts": pts_json(&d.line.pts) })).collect::<Vec<_>>(),
        // See `preview_json`'s matching comment -- same previously-dropped field.
        "displaced_vias": preview.displaced_vias.iter().map(|d| json!({ "source_via": d.source_via, "x": d.pos.x, "y": d.pos.y })).collect::<Vec<_>>(),
        // `DragKind::Via` only: the attached tracks' own live stretched
        // shape, so the frontend can draw them following the via while
        // the drag is still in progress (see `DragPreview::fanout`'s own
        // doc comment) -- empty for a corner drag.
        "fanout": preview.fanout.iter().map(|l| json!({ "layer": router.layer_name(l.layer), "width": l.width, "pts": pts_json(&l.pts) })).collect::<Vec<_>>(),
    })
}

/// `POST /api/route/drag_start`: `{x, y, layer, settings?}` (`mode` at the top level still works). Builds a fresh
/// `Router` from the board as it stands right now, same as `start` -- a
/// drag reads the live board just as much as a route does. `settings` are the same
/// `ROUTING_SETTINGS` a route session takes ([`settings_of`]) --
/// `eda_pns::dragger::Dragger` reuses it exactly as upstream's `DRAGGER`
/// reuses `SHOVE`/`WALKAROUND`; no `remove_loops` here, a route-only
/// concept upstream's own `DRAGGER` never touches either. `free_angle`
/// (default false) is `PNS::DM_FREE_ANGLE` -- the `G` hotkey
/// (`pcbnew.InteractiveRouter.DragFreeAngle`): the drag then only marks
/// obstacles, whatever `mode` says (`DRAGGER::Drag`).
pub fn drag_start(dir: &Path, cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let (_, design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let mut router = Router::new(&design, &model);
    router.settings = settings_of(&req);
    let at = point_of(&req);
    let layer = req.get("layer").and_then(Value::as_str).unwrap_or("F.Cu");
    let free_angle = req.get("free_angle").and_then(Value::as_bool).unwrap_or(false);
    match router.drag_start_with(at, layer, free_angle) {
        Ok(()) => {
            let mut reply = router.drag_preview(at).map(|p| drag_preview_json(&router, &p)).unwrap_or_else(|| err("internal: started but no preview"));
            // the tracks of the line being dragged: the cursor must not snap onto them (`TOOL_BASE::checkSnap`)
            if let Some(obj) = reply.as_object_mut() {
                obj.insert("line_tracks".into(), json!(router.drag_line_tracks()));
            }
            *cell.lock().unwrap_or_else(|e| e.into_inner()) = Some(router);
            reply
        }
        Err(message) => err(message),
    }
}

/// `POST /api/route/drag_move`: `{x, y}`.
pub fn drag_move(cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    let Some(router) = guard.as_mut() else { return err("not dragging") };
    let at = point_of(&req);
    router.drag_preview(at).map(|p| drag_preview_json(router, &p)).unwrap_or_else(|| err("not dragging"))
}

/// `POST /api/route/drag_finish`: `{x, y, force_commit?}`. `DRAGGER::FixRoute( aForceCommit )`: a position the drag cannot resolve
/// commits the last one it could; a drag that is only marking obstacles (its first position failed) commits a violation with
/// `force_commit` (Ctrl+click) and not otherwise.
pub fn drag_finish(dir: &Path, cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let mut router = match cell.lock().unwrap_or_else(|e| e.into_inner()).take() {
        Some(r) => r,
        None => return err("not dragging"),
    };
    let at = point_of(&req);
    let force_commit = req.get("force_commit").and_then(Value::as_bool).unwrap_or(false);
    let marking = router.drag_forces_mark_obstacles();
    let Some(commit) = router.drag_finish_with(at, force_commit) else {
        let message = if marking { "Track violates DRC. Ctrl+click to commit anyway." } else { "drag target still collides; left uncommitted" };
        return json!({ "ok": false, "message": message });
    };
    let cmd = eda_ops::Cmd::CommitRoute { remove_track_ids: commit.remove_track_ids, remove_via_ids: commit.remove_via_ids, tracks: commit.tracks, vias: commit.vias };
    match board::step(dir, cmd, true, "ui") {
        Ok(summary) => json!({ "ok": true, "message": summary }),
        Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
    }
}

// --------------------------------------------------------------- differential pairs (stage 6)
//
// `6` on a recognized diff-pair net: route two parallel, gap-matched lines
// at once (`eda_pns::diff_pair::DiffPairPlacer`, driven through
// `eda_pns::router::Router`'s `*_diff_pair` methods). Shares `RouteCell`/
// `Router` with the route/drag sessions above -- `Router` itself keeps all
// three mutually exclusive -- and the existing `cancel` above already ends
// a diff-pair session too (it just drops the whole cell, session kind
// notwithstanding).

fn dp_via_json(router: &Router, v: &eda_pns::dp_placer::PreviewVia) -> Value {
    json!({ "net": v.net.as_deref(), "x": v.pos.x, "y": v.pos.y, "diameter": v.diameter, "drill": v.drill, "from_layer": router.layer_name(v.layers.start()), "to_layer": router.layer_name(v.layers.end()) })
}

/// A pair's preview as the studio draws it: `net_a`/`net_b` are the P and N nets (`DpNetPair`: the one named `+` or `P` first);
/// `head_*` the two lines of the head, `runs_*` the legs fixed so far (each with its layer); `vias` the pair of vias the head ends in
/// while `V` is on, `fixed_vias` those of earlier legs; `displaced*` what `Shove` mode would push aside; `colliding` that the pair does
/// not fit as drawn.
fn dp_preview_json(router: &Router, preview: &eda_pns::dp_placer::DiffPairPreview) -> Value {
    let (net_a, net_b) = router.diff_pair_nets().unwrap_or((None, None));
    let layer_name = router.layer_name(preview.layer);
    json!({
        "ok": true,
        "net_a": net_a,
        "net_b": net_b,
        "layer": layer_name,
        "width": preview.width,
        "gap": preview.gap,
        "colliding": preview.colliding,
        "head_a": pts_json(&preview.head_a.pts),
        "head_b": pts_json(&preview.head_b.pts),
        "runs_a": preview.runs_a.iter().map(|r| json!({ "layer": router.layer_name(r.layer), "pts": pts_json(&r.pts) })).collect::<Vec<_>>(),
        "runs_b": preview.runs_b.iter().map(|r| json!({ "layer": router.layer_name(r.layer), "pts": pts_json(&r.pts) })).collect::<Vec<_>>(),
        "snapped_end": preview.snapped_end,
        "placing_via": preview.placing_via,
        "vias": preview.vias.iter().map(|v| dp_via_json(router, v)).collect::<Vec<_>>(),
        "fixed_vias": preview.fixed_vias.iter().map(|v| dp_via_json(router, v)).collect::<Vec<_>>(),
        "displaced": preview.displaced_lines.iter().map(|d| json!({ "source_track": d.source_track, "layer": router.layer_name(d.line.layer), "pts": pts_json(&d.line.pts) })).collect::<Vec<_>>(),
        "displaced_vias": preview.displaced_vias.iter().map(|d| json!({ "source_via": d.source_via, "x": d.pos.x, "y": d.pos.y })).collect::<Vec<_>>(),
    })
}

/// `POST /api/route/dp_start`: `{x, y, layer, width?, gap?, via_gap?, settings?}` -- `width`/`gap`/`via_gap` are the "Differential Pair
/// Dimensions..." dialog's custom values (`BOARD_DESIGN_SETTINGS::UseCustomDiffPairDimensions`); without them the pair takes the net
/// class's. `settings` is the router's ([`settings_of`]): the pair is walked, shoved or only marked per its `mode`. A refusal carries
/// KiCad's own message (`ROUTER::SetFailureReason`).
pub fn dp_start(dir: &Path, cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let (_, design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let mut router = Router::new(&design, &model);
    router.settings = settings_of(&req);
    let at = point_of(&req);
    let layer = req.get("layer").and_then(Value::as_str).unwrap_or("F.Cu");
    match router.start_diff_pair(at, layer) {
        Ok(()) => {
            router.set_diff_pair_sizes(req.get("width").and_then(Value::as_i64), req.get("gap").and_then(Value::as_i64), req.get("via_gap").and_then(Value::as_i64));
            let reply = router.diff_pair_preview(at).map(|p| dp_preview_json(&router, &p)).unwrap_or_else(|| err("internal: started but no preview"));
            *cell.lock().unwrap_or_else(|e| e.into_inner()) = Some(router);
            reply
        }
        Err(message) => err(message),
    }
}

/// `POST /api/route/dp_dims`: `{width?, gap?, via_gap?}` -- `ROUTER_TOOL::DpDimensionsDialog` (`m_router->UpdateSizes`) for the pair being routed now.
pub fn dp_dims(cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    let Some(router) = guard.as_mut() else { return err("not routing a diff pair") };
    if router.set_diff_pair_sizes(req.get("width").and_then(Value::as_i64), req.get("gap").and_then(Value::as_i64), req.get("via_gap").and_then(Value::as_i64)) {
        json!({ "ok": true })
    } else {
        err("not routing a diff pair")
    }
}

/// `POST /api/route/dp_move`: `{x, y, flip_posture?}`.
pub fn dp_move(cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    let Some(router) = guard.as_mut() else { return err("not routing a diff pair") };
    if req.get("flip_posture").and_then(Value::as_bool).unwrap_or(false) {
        router.flip_diff_pair_posture();
    }
    let at = point_of(&req);
    router.diff_pair_preview(at).map(|p| dp_preview_json(router, &p)).unwrap_or_else(|| err("not routing a diff pair"))
}

/// `POST /api/route/dp_via`: `{enabled, diameter?, drill?, to_layer?}` -- `V` for a pair (`ROUTER::ToggleViaPlacement`): the head ends in a
/// pair of vias a `viaGap()` apart, and a fix places them and goes on from them on `to_layer`. No preview here; the next `dp_move` has it.
pub fn dp_via(cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    let Some(router) = guard.as_mut() else { return err("not routing a diff pair") };
    if !router.is_routing_diff_pair() {
        return err("not routing a diff pair");
    }
    let enabled = req.get("enabled").and_then(Value::as_bool).unwrap_or(true);
    let to_layer = req.get("to_layer").and_then(Value::as_str).and_then(|n| router.layer_index(n));
    router.toggle_diff_pair_via(enabled, req.get("diameter").and_then(Value::as_i64), req.get("drill").and_then(Value::as_i64), to_layer);
    json!({ "ok": true })
}

/// `POST /api/route/dp_fix`: `{x, y}`.
pub fn dp_fix(cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    let Some(router) = guard.as_mut() else { return err("not routing a diff pair") };
    let at = point_of(&req);
    match router.fix_diff_pair(at) {
        Some(FixOutcome::Fixed { real_end }) => {
            let preview = router.diff_pair_preview(at).map(|p| dp_preview_json(router, &p)).unwrap_or_else(|| json!({}));
            json!({ "ok": true, "blocked": false, "real_end": real_end, "preview": preview })
        }
        Some(FixOutcome::Blocked) => json!({ "ok": true, "blocked": true, "message": "still colliding; not fixed" }),
        None => err("not routing a diff pair"),
    }
}

/// `POST /api/route/dp_undo_segment` (Backspace): no body.
pub fn dp_undo_segment(cell: &RouteCell) -> Value {
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    let Some(router) = guard.as_mut() else { return err("not routing a diff pair") };
    let popped = router.undo_diff_pair_segment();
    json!({ "ok": true, "popped": popped })
}

/// `POST /api/route/dp_finish`: `{x, y}`.
pub fn dp_finish(dir: &Path, cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let mut router = match cell.lock().unwrap_or_else(|e| e.into_inner()).take() {
        Some(r) => r,
        None => return err("not routing a diff pair"),
    };
    let at = point_of(&req);
    let Some(commit) = router.finish_diff_pair(at) else {
        return json!({ "ok": false, "message": "the pair still collides; left uncommitted" });
    };
    let cmd = eda_ops::Cmd::CommitRoute { remove_track_ids: commit.remove_track_ids, remove_via_ids: commit.remove_via_ids, tracks: commit.tracks, vias: commit.vias };
    match board::step(dir, cmd, true, "ui") {
        Ok(summary) => json!({ "ok": true, "message": summary }),
        Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{Design, PlacementSection, Provenance, RoutingSection, Track, Via};
    use eda_model::ConstraintModel;

    /// A scratch project directory, removed again when the test ends.
    struct Scratch(std::path::PathBuf);
    impl std::ops::Deref for Scratch {
        type Target = std::path::PathBuf;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn scratch(name: &str) -> Scratch {
        let d = std::env::temp_dir().join(format!("eda_cli_route_test_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Scratch(d)
    }

    fn p(x: i64, y: i64) -> Point {
        Point { x, y }
    }

    /// A 20 mm board with a SIG stub ending at (10000, 8000) and a GND via at (10000, 5000), right in the way of SIG going down.
    fn setup(dir: &Path) {
        let stub = Track { id: "sig".into(), net: "SIG".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![p(10_000, 9_000), p(10_000, 8_000)], arc_mid_offset: None };
        let via = Via { id: "v_gnd".into(), net: "GND".into(), at: p(10_000, 5_000), drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() };
        setup_board(dir, vec![stub], vec![via]);
    }

    /// A 20 mm board with these tracks and vias.
    fn setup_board(dir: &Path, tracks: Vec<Track>, vias: Vec<Via>) {
        let model = ConstraintModel { nets: ["SIG", "GND", "USB_DP", "USB_DN"].iter().map(|n| eda_model::Net { name: n.to_string(), pins: vec![] }).collect(), ..ConstraintModel::default() };
        let intent_path = dir.join("intent.yaml");
        std::fs::write(&intent_path, serde_yaml::to_string(&model).unwrap()).unwrap();
        let design = Design {
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "t".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection { outline: vec![p(0, 0), p(20_000, 0), p(20_000, 20_000), p(0, 20_000)], footprints: vec![], modules: vec![] }),
            routing: Some(RoutingSection { tracks, vias, zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }),
            drawings: None,
        };
        board::save(dir, &design).unwrap();
        let meta = board::Meta { intent: intent_path.display().to_string(), snap_um: 100, spacing_um: 300 };
        std::fs::write(dir.join("board.json"), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
    }

    fn body(v: Value) -> Vec<u8> {
        v.to_string().into_bytes()
    }

    fn start_with(dir: &Path, cell: &RouteCell, settings: Value) {
        let reply = start(dir, cell, &body(json!({ "x": 10_000, "y": 8_000, "layer": "F.Cu", "width": 200, "settings": settings })));
        assert_eq!(reply["ok"], json!(true), "{reply}");
    }

    #[test]
    fn a_request_without_settings_is_kicads_defaults() {
        let s = settings_of(&json!({}));
        let d = RoutingSettings::default();
        assert_eq!((s.mode, s.optimizer_effort, s.shove_vias, s.jump_over_obstacles, s.remove_loops, s.smart_pads, s.can_violate_drc, s.free_angle_mode, s.fix_all_segments), (d.mode, d.optimizer_effort, true, false, true, true, false, false, true));
        assert_eq!((s.shove_iteration_limit, s.walkaround_iteration_limit, s.via_force_prop_iteration_limit, s.walkaround_hug_length_threshold), (250, 40, 40, 1.5));
        assert_eq!(s.shove_time_limit_ms, 1000, "ROUTING_SETTINGS: m_shoveTimeLimit = 1000");
        // the two a client sent before there was a `settings` object
        let legacy = settings_of(&json!({ "mode": "shove", "remove_loops": false }));
        assert_eq!((legacy.mode, legacy.remove_loops), (Mode::Shove, false));
    }

    #[test]
    fn every_setting_the_dialog_has_reaches_the_router() {
        let s = settings_of(&json!({ "settings": {
            "mode": "mark_obstacles", "optimizer_effort": "full", "shove_vias": false, "jump_over_obstacles": true, "remove_loops": false,
            "smart_pads": false, "allow_drc_violations": true, "free_angle_mode": true, "fix_all_segments": false,
            "shove_iteration_limit": 12, "shove_time_limit": 350, "walkaround_iteration_limit": 7, "via_force_prop_iteration_limit": 9, "walkaround_hug_length_threshold": 3.25,
        } }));
        assert_eq!((s.mode, s.optimizer_effort), (Mode::MarkObstacles, OptEffort::Full));
        assert_eq!((s.shove_vias, s.jump_over_obstacles, s.remove_loops, s.smart_pads, s.can_violate_drc, s.free_angle_mode, s.fix_all_segments), (false, true, false, false, true, true, false));
        assert_eq!((s.shove_iteration_limit, s.walkaround_iteration_limit, s.via_force_prop_iteration_limit, s.walkaround_hug_length_threshold), (12, 7, 9, 3.25));
        assert_eq!(s.shove_time_limit_ms, 350);
        assert!(s.allow_drc_violations() && s.free_angle(), "both only count in Highlight collisions mode");
        // nonsense is clamped, not trusted
        let wild = settings_of(&json!({ "settings": { "shove_iteration_limit": -5, "via_force_prop_iteration_limit": 0, "walkaround_hug_length_threshold": -1, "shove_time_limit": -40 } }));
        assert_eq!((wild.shove_iteration_limit, wild.via_force_prop_iteration_limit, wild.walkaround_hug_length_threshold), (1, 1, 0.0));
        assert_eq!(wild.shove_time_limit_ms, 1, "a clock below a millisecond is no clock to shove by");
    }

    #[test]
    fn shove_vias_decides_whether_a_via_in_the_way_is_pushed_or_walked_around() {
        let dir = scratch("shove_vias");
        setup(&dir);
        let cell: RouteCell = Mutex::new(None);
        // Shove: the GND via at (10000, 5000) sits right on SIG's way down and is pushed aside.
        start_with(&dir, &cell, json!({ "mode": "shove" }));
        let pushed = mv(&cell, &body(json!({ "x": 10_000, "y": 2_000 })));
        assert_eq!(pushed["colliding"], json!(false), "{pushed}");
        assert_eq!(pushed["displaced_vias"].as_array().unwrap().len(), 1, "the via is shoved: {pushed}");
        assert_eq!(pushed["head"].as_array().unwrap().len(), 2, "and SIG goes straight through where it was: {pushed}");
        // The same with "Shove vias" off: the via is an obstacle like a pad, the head walks around it.
        start_with(&dir, &cell, json!({ "mode": "shove", "shove_vias": false }));
        let hugged = mv(&cell, &body(json!({ "x": 10_000, "y": 2_000 })));
        assert_eq!(hugged["colliding"], json!(false), "{hugged}");
        assert!(hugged["displaced_vias"].as_array().unwrap().is_empty(), "the via stays: {hugged}");
        assert!(hugged["head"].as_array().unwrap().len() > 2, "the head goes around it: {hugged}");
        // Changing the setting under a running session takes effect on the next move, as upstream's dialog does.
        let live = set_settings(&cell, &body(json!({ "settings": { "mode": "shove", "shove_vias": true } })));
        assert_eq!(live["ok"], json!(true));
        let again = mv(&cell, &body(json!({ "x": 10_000, "y": 2_000 })));
        assert_eq!(again["displaced_vias"].as_array().unwrap().len(), 1, "{again}");
    }

    #[test]
    fn free_angle_mode_draws_the_head_at_any_angle_only_in_highlight_collisions_mode() {
        let dir = scratch("free_angle");
        setup(&dir);
        let cell: RouteCell = Mutex::new(None);
        // (3000, -1000) from the stub's end: not a multiple of 45 degrees.
        let target = body(json!({ "x": 13_000, "y": 7_000 }));
        start_with(&dir, &cell, json!({ "mode": "mark_obstacles", "free_angle_mode": true }));
        assert_eq!(mv(&cell, &target)["head"], json!([[10_000, 8_000], [13_000, 7_000]]), "one straight leg");
        start_with(&dir, &cell, json!({ "mode": "mark_obstacles" }));
        assert_eq!(mv(&cell, &target)["head"].as_array().unwrap().len(), 3, "an elbow of 45-degree legs");
        start_with(&dir, &cell, json!({ "mode": "walkaround", "free_angle_mode": true }));
        assert_eq!(mv(&cell, &target)["head"].as_array().unwrap().len(), 3, "the switch only counts when highlighting collisions");
    }

    #[test]
    fn allow_drc_violations_lets_a_colliding_head_be_fixed_in_highlight_collisions_mode() {
        let dir = scratch("drc_violations");
        setup(&dir);
        let cell: RouteCell = Mutex::new(None);
        let onto_the_via = body(json!({ "x": 10_000, "y": 5_000 }));
        start_with(&dir, &cell, json!({ "mode": "mark_obstacles" }));
        let preview = mv(&cell, &onto_the_via);
        assert_eq!(preview["colliding"], json!(true), "{preview}");
        assert_eq!(fix(&cell, &onto_the_via)["blocked"], json!(true), "a colliding head is refused by default");
        start_with(&dir, &cell, json!({ "mode": "mark_obstacles", "allow_drc_violations": true }));
        assert_eq!(fix(&cell, &onto_the_via)["blocked"], json!(false), "and fixed once violations are allowed");
    }

    fn tracks_of(dir: &Path) -> Vec<(String, Vec<Point>)> {
        let (_, design, _) = board::load(dir).unwrap();
        design.routing.unwrap().tracks.iter().map(|t| (t.net.clone(), t.pts.clone())).collect()
    }

    fn track(id: &str, net: &str, pts: &[Point]) -> Track {
        Track { id: id.into(), net: net.into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: pts.to_vec(), arc_mid_offset: None }
    }

    fn pts_of(reply: &Value, key: &str) -> Vec<Point> {
        reply[key].as_array().unwrap_or_else(|| panic!("{key} in {reply}")).iter().map(|v| p(v[0].as_i64().unwrap(), v[1].as_i64().unwrap())).collect()
    }

    /// `DRAGGER::Start`: grabbing the middle of a segment slides it (`DM_SEGMENT`), its neighbours following; the commit replaces the
    /// track with the slid one.
    #[test]
    fn a_drag_grabbed_in_the_middle_of_a_segment_slides_it() {
        let dir = scratch("drag_segment");
        setup_board(&dir, vec![track("u", "SIG", &[p(2_000, 12_000), p(2_000, 10_000), p(12_000, 10_000), p(12_000, 12_000)])], vec![]);
        let cell: RouteCell = Mutex::new(None);
        let started = drag_start(&dir, &cell, &body(json!({ "x": 7_000, "y": 10_000, "layer": "F.Cu" })));
        assert_eq!(started["ok"], json!(true), "{started}");
        assert_eq!(started["drag"], json!("segment"), "the middle of the 10 mm segment: {started}");
        assert_eq!(started["line_tracks"], json!(["u"]), "the track the cursor must not snap onto while it is dragged: {started}");
        let moved = drag_move(&cell, &body(json!({ "x": 7_000, "y": 8_000 })));
        assert_eq!(moved["colliding"], json!(false), "{moved}");
        assert_eq!(moved["restored"], json!(false));
        assert_eq!(pts_of(&moved, "pts"), vec![p(2_000, 12_000), p(2_000, 8_000), p(12_000, 8_000), p(12_000, 12_000)]);
        let done = drag_finish(&dir, &cell, &body(json!({ "x": 7_000, "y": 8_000 })));
        assert_eq!(done["ok"], json!(true), "{done}");
        assert_eq!(tracks_of(&dir), vec![("SIG".to_string(), vec![p(2_000, 12_000), p(2_000, 8_000), p(12_000, 8_000), p(12_000, 12_000)])]);
        // near an end it is the corner instead
        let near_end = drag_start(&dir, &cell, &body(json!({ "x": 2_000, "y": 8_050, "layer": "F.Cu" })));
        assert_eq!(near_end["drag"], json!("corner"), "{near_end}");
    }

    /// `DRAGGER::Drag` / `FixRoute`: the cursor goes somewhere the drag cannot be resolved and the line stays at the last valid position;
    /// letting go there commits that one.
    #[test]
    fn an_illegal_position_shows_the_last_valid_one_and_a_release_commits_it() {
        let dir = scratch("drag_restore");
        setup(&dir);
        let cell: RouteCell = Mutex::new(None);
        // the end of the SIG stub, with the GND via at (10000, 5000) below it
        let started = drag_start(&dir, &cell, &body(json!({ "x": 10_000, "y": 8_000, "layer": "F.Cu" })));
        assert_eq!(started["drag"], json!("corner"), "{started}");
        let valid = drag_move(&cell, &body(json!({ "x": 12_000, "y": 6_500 })));
        assert_eq!((valid["colliding"].clone(), valid["restored"].clone()), (json!(false), json!(false)), "{valid}");
        let on_the_via = drag_move(&cell, &body(json!({ "x": 10_000, "y": 5_000 })));
        assert_eq!(on_the_via["restored"], json!(true), "{on_the_via}");
        assert_eq!(on_the_via["colliding"], json!(false));
        assert_eq!(on_the_via["pts"], valid["pts"], "the line is where the last valid position put it");
        let done = drag_finish(&dir, &cell, &body(json!({ "x": 10_000, "y": 5_000 })));
        assert_eq!(done["ok"], json!(true), "{done}");
        let tracks = tracks_of(&dir);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].1.last(), Some(&p(12_000, 6_500)), "committed at the last valid point: {tracks:?}");
    }

    /// `GetForceMarkObstaclesMode`: a drag whose first position is already a violation only marks obstacles; Ctrl+click commits it.
    #[test]
    fn a_drag_that_starts_in_violation_only_marks_and_ctrl_click_commits_it() {
        let dir = scratch("drag_force");
        // the stub's end sits on the GND via: it is in violation where it starts
        setup_board(&dir, vec![track("sig", "SIG", &[p(10_000, 9_000), p(10_000, 5_200)])], vec![Via { id: "v".into(), net: "GND".into(), at: p(10_000, 5_000), drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }]);
        let cell: RouteCell = Mutex::new(None);
        let started = drag_start(&dir, &cell, &body(json!({ "x": 10_000, "y": 5_200, "layer": "F.Cu" })));
        assert_eq!(started["ok"], json!(true), "{started}");
        assert_eq!((started["forced_mark_obstacles"].clone(), started["colliding"].clone()), (json!(true), json!(true)), "{started}");
        let refused = drag_finish(&dir, &cell, &body(json!({ "x": 10_000, "y": 5_300 })));
        assert_eq!(refused["ok"], json!(false), "{refused}");
        assert!(refused["message"].as_str().unwrap().contains("Ctrl+click"), "{refused}");
        let started = drag_start(&dir, &cell, &body(json!({ "x": 10_000, "y": 5_200, "layer": "F.Cu" })));
        assert_eq!(started["forced_mark_obstacles"], json!(true));
        let forced = drag_finish(&dir, &cell, &body(json!({ "x": 10_000, "y": 5_300, "force_commit": true })));
        assert_eq!(forced["ok"], json!(true), "{forced}");
    }

    /// `ROUTER_TOOL::routerContinueFromEnd`: with a leg fixed, the leg is committed and the route restarts at the far end, heading for
    /// where the old head ended.
    #[test]
    fn route_from_other_end_commits_what_is_fixed_and_restarts_at_the_far_end() {
        let dir = scratch("continue");
        // a SIG stub ending at (10000, 8000) and a SIG via at (10000, 2000) it is to be connected to
        setup_board(&dir, vec![track("sig", "SIG", &[p(10_000, 9_000), p(10_000, 8_000)])], vec![Via { id: "far".into(), net: "SIG".into(), at: p(10_000, 2_000), drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }]);
        let cell: RouteCell = Mutex::new(None);
        start_with(&dir, &cell, json!({}));
        // nothing fixed: nothing is committed, the route restarts at the via
        let first = continue_from_end(&dir, &cell, &body(json!({ "x": 10_000, "y": 6_500 })));
        assert_eq!(first["ok"], json!(true), "{first}");
        assert_eq!(first["committed"], json!(false));
        assert_eq!(first["from"], json!([10_000, 2_000]));
        assert_eq!(pts_of(&first, "head").first(), Some(&p(10_000, 2_000)), "the new head leaves the far end");
        assert_eq!(pts_of(&first, "head").last(), Some(&p(10_000, 6_500)), "and heads for where the old one ended");
        assert_eq!(tracks_of(&dir).len(), 1);
        // fix a leg of a route started again from the stub, then continue from the other end
        start_with(&dir, &cell, json!({}));
        let fixed = fix(&cell, &body(json!({ "x": 12_000, "y": 8_000 })));
        assert_eq!((fixed["ok"].clone(), fixed["blocked"].clone()), (json!(true), json!(false)), "{fixed}");
        let second = continue_from_end(&dir, &cell, &body(json!({ "x": 12_000, "y": 5_000 })));
        assert_eq!(second["ok"], json!(true), "{second}");
        assert_eq!(second["committed"], json!(true), "the fixed leg is on the board now");
        let tracks = tracks_of(&dir);
        assert_eq!(tracks.len(), 2, "{tracks:?}");
        assert!(tracks.iter().any(|(_, pts)| pts.first() == Some(&p(10_000, 8_000)) && pts.last() == Some(&p(12_000, 8_000))), "{tracks:?}");
        assert_eq!(pts_of(&second, "head").first(), Some(&p(10_000, 2_000)));
        // a session is running again, from the far end
        let again = mv(&cell, &body(json!({ "x": 12_000, "y": 5_000 })));
        assert_eq!(again["ok"], json!(true), "{again}");
        // the leg committed when the route restarted and the route finished from the far end are one undo step (`APPEND_UNDO`)
        let finished = finish(&dir, &cell, &body(json!({ "x": 12_000, "y": 8_000 })));
        assert_eq!(finished["ok"], json!(true), "{finished}");
        assert_eq!(tracks_of(&dir).len(), 3, "the stub, the committed leg and the route from the via");
        let undone = board::undo(&dir, "test", Some(eda_ops::Domain::Pcb));
        assert!(undone.is_ok(), "{undone:?}");
        assert_eq!(tracks_of(&dir).len(), 1, "one undo took back both commits: {:?}", tracks_of(&dir));
        // nothing left to connect to is a refusal that leaves the session alone
        let nothing = continue_from_end(&dir, &Mutex::new(None), &body(json!({ "x": 0, "y": 0 })));
        assert_eq!(nothing["ok"], json!(false));
    }

    fn via_at(id: &str, net: &str, x: i64, y: i64) -> Via {
        Via { id: id.into(), net: net.into(), at: p(x, y), drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }
    }

    fn vias_of(dir: &Path) -> Vec<(String, Point)> {
        let (_, design, _) = board::load(dir).unwrap();
        design.routing.unwrap().vias.iter().map(|v| (v.net.clone(), v.at)).collect()
    }

    /// A pair of vias, USB_DP and USB_DN a millimetre apart, to start a pair from.
    fn pair_board(dir: &Path) {
        setup_board(dir, vec![], vec![via_at("dp", "USB_DP", 3_000, 5_000), via_at("dn", "USB_DN", 3_000, 6_000)]);
    }

    /// `DIFF_PAIR_PLACER` through the HTTP calls the studio makes: start on one via of a pair, move, fix, finish. Both nets are committed in
    /// one step, P first in the reply, and one undo takes both back.
    #[test]
    fn a_pair_is_routed_over_http_and_committed_as_one_undo_step() {
        let dir = scratch("dp_http");
        pair_board(&dir);
        let cell: RouteCell = Mutex::new(None);
        let started = dp_start(&dir, &cell, &body(json!({ "x": 3_000, "y": 6_000, "layer": "F.Cu", "settings": { "mode": "walkaround" } })));
        assert_eq!(started["ok"], json!(true), "{started}");
        assert_eq!((started["net_a"].clone(), started["net_b"].clone()), (json!("USB_DP"), json!("USB_DN")), "P is the net named P, whichever via was clicked: {started}");
        let moved = dp_move(&cell, &body(json!({ "x": 12_000, "y": 5_500 })));
        assert_eq!((moved["ok"].clone(), moved["colliding"].clone()), (json!(true), json!(false)), "{moved}");
        let (a, b) = (pts_of(&moved, "head_a"), pts_of(&moved, "head_b"));
        assert_eq!((a.first(), b.first()), (Some(&p(3_000, 5_000)), Some(&p(3_000, 6_000))), "the lines leave the vias' centres");
        let pitch = moved["width"].as_i64().unwrap() + moved["gap"].as_i64().unwrap();
        let (la, lb) = (a.last().unwrap(), b.last().unwrap());
        let apart = (((la.x - lb.x).pow(2) + (la.y - lb.y).pow(2)) as f64).sqrt();
        assert!(apart >= pitch as f64 - 1.0 && apart <= pitch as f64 + 3.0, "they end a pitch ({pitch}) apart: {apart}");
        let fixed = dp_fix(&cell, &body(json!({ "x": 12_000, "y": 5_500 })));
        assert_eq!((fixed["ok"].clone(), fixed["blocked"].clone(), fixed["real_end"].clone()), (json!(true), json!(false), json!(false)), "{fixed}");
        assert_eq!(fixed["preview"]["runs_a"].as_array().unwrap().len(), 1, "one leg fixed: {fixed}");
        let done = dp_finish(&dir, &cell, &body(json!({ "x": 12_000, "y": 5_500 })));
        assert_eq!(done["ok"], json!(true), "{done}");
        let tracks = tracks_of(&dir);
        assert_eq!(tracks.len(), 2, "{tracks:?}");
        assert!(tracks.iter().any(|(net, pts)| net == "USB_DP" && pts.first() == Some(&p(3_000, 5_000))) && tracks.iter().any(|(net, pts)| net == "USB_DN" && pts.first() == Some(&p(3_000, 6_000))), "{tracks:?}");
        let undone = board::undo(&dir, "test", Some(eda_ops::Domain::Pcb));
        assert!(undone.is_ok(), "{undone:?}");
        assert!(tracks_of(&dir).is_empty(), "one undo took both lines back");
    }

    /// `V`: the head ends in a pair of vias, a click places them and the route goes on from them on the other layer; the commit has the
    /// four tracks (two per layer) and the two new vias.
    #[test]
    fn v_places_a_pair_of_vias_and_the_pair_goes_on_on_the_other_layer() {
        let dir = scratch("dp_via");
        pair_board(&dir);
        let cell: RouteCell = Mutex::new(None);
        let started = dp_start(&dir, &cell, &body(json!({ "x": 3_000, "y": 5_000, "layer": "F.Cu" })));
        assert_eq!(started["ok"], json!(true), "{started}");
        assert_eq!(dp_via(&cell, &body(json!({ "enabled": true, "to_layer": "B.Cu" })))["ok"], json!(true));
        let at = body(json!({ "x": 9_000, "y": 5_500 }));
        let moved = dp_move(&cell, &at);
        assert_eq!((moved["placing_via"].clone(), moved["colliding"].clone()), (json!(true), json!(false)), "{moved}");
        let vias = moved["vias"].as_array().unwrap();
        assert_eq!(vias.len(), 2, "{moved}");
        assert_eq!((vias[0]["net"].clone(), vias[1]["net"].clone()), (json!("USB_DP"), json!("USB_DN")));
        assert_eq!((vias[0]["from_layer"].clone(), vias[0]["to_layer"].clone()), (json!("F.Cu"), json!("B.Cu")));
        let (la, lb) = (pts_of(&moved, "head_a").pop().unwrap(), pts_of(&moved, "head_b").pop().unwrap());
        assert_eq!((la, lb), (p(vias[0]["x"].as_i64().unwrap(), vias[0]["y"].as_i64().unwrap()), p(vias[1]["x"].as_i64().unwrap(), vias[1]["y"].as_i64().unwrap())), "each line ends in its via");
        let fixed = dp_fix(&cell, &at);
        assert_eq!((fixed["ok"].clone(), fixed["blocked"].clone()), (json!(true), json!(false)), "{fixed}");
        assert_eq!(fixed["preview"]["layer"], json!("B.Cu"), "the route goes on on the other layer: {fixed}");
        assert_eq!(fixed["preview"]["fixed_vias"].as_array().unwrap().len(), 2);
        assert_eq!(fixed["preview"]["placing_via"], json!(false));
        let on = dp_move(&cell, &body(json!({ "x": 15_000, "y": 5_500 })));
        assert_eq!((on["layer"].clone(), on["colliding"].clone()), (json!("B.Cu"), json!(false)), "{on}");
        assert_eq!(pts_of(&on, "head_a").first().copied(), Some(p(vias[0]["x"].as_i64().unwrap(), vias[0]["y"].as_i64().unwrap())), "from the via");
        let done = dp_finish(&dir, &cell, &body(json!({ "x": 15_000, "y": 5_500 })));
        assert_eq!(done["ok"], json!(true), "{done}");
        let (_, design, _) = board::load(&dir).unwrap();
        let routing = design.routing.unwrap();
        let on_layer = |layer: &str| routing.tracks.iter().filter(|t| t.layer == layer).count();
        assert_eq!((on_layer("F.Cu"), on_layer("B.Cu")), (2, 2), "two lines on each layer: {:?}", routing.tracks);
        assert_eq!(vias_of(&dir).len(), 4, "the two it started from and the two it placed: {:?}", vias_of(&dir));
    }

    /// `FindDpPrimitivePair`'s refusals reach the studio in KiCad's words.
    #[test]
    fn a_pair_cannot_start_from_a_net_that_is_not_half_of_one() {
        let dir = scratch("dp_refused");
        setup_board(&dir, vec![], vec![via_at("g", "GND", 3_000, 5_000)]);
        let cell: RouteCell = Mutex::new(None);
        let refused = dp_start(&dir, &cell, &body(json!({ "x": 3_000, "y": 5_000, "layer": "F.Cu" })));
        assert_eq!(refused["ok"], json!(false));
        assert!(refused["message"].as_str().unwrap().contains("Unable to find complementary differential pair nets"), "{refused}");
        assert!(cell.lock().unwrap().is_none(), "no session is left");
        let nothing = dp_start(&dir, &cell, &body(json!({ "x": 15_000, "y": 15_000, "layer": "F.Cu" })));
        assert_eq!(nothing["ok"], json!(false), "{nothing}");
    }

    /// The settings the dragger and the end-of-route snapping read are in the request's `settings`.
    #[test]
    fn the_drag_and_snap_settings_reach_the_router() {
        let s = settings_of(&json!({ "settings": { "smooth_dragged_segments": false, "snap_to_tracks": false } }));
        assert_eq!((s.smooth_dragged_segments, s.snap_to_tracks), (false, false));
        let d = settings_of(&json!({}));
        assert_eq!((d.smooth_dragged_segments, d.snap_to_tracks), (true, true), "KiCad's defaults");
    }
}
