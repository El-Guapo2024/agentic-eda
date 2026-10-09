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
///   `allow_drc_violations` (`CanViolateDRC`), `free_angle_mode`, `fix_all_segments`,
/// - the limits `shove_iteration_limit`, `walkaround_iteration_limit`, `via_force_prop_iteration_limit` and
///   `walkaround_hug_length_threshold`, which KiCad keeps in the settings file only.
///
/// Anything absent is KiCad's default (`RoutingSettings::default()`); `mode` and `remove_loops` also come from the request's
/// own top level, where they used to be the only two. Read field by field: serde_json's `arbitrary_precision` (starlark turns
/// it on) breaks a derived reader on numbers.
fn settings_of(req: &Value) -> RoutingSettings {
    let mut s = RoutingSettings::default();
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
    limit("shove_iteration_limit", &mut s.shove_iteration_limit);
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
        Ok(summary) => json!({ "ok": true, "message": summary }),
        Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
    }
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
    router.settings = settings_of(&req);
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

fn drag_preview_json(router: &Router, preview: &eda_pns::dragger::DragPreview) -> Value {
    json!({
        "ok": true,
        "colliding": preview.colliding,
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
            let reply = router.drag_preview(at).map(|p| drag_preview_json(&router, &p)).unwrap_or_else(|| err("internal: started but no preview"));
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

/// `POST /api/route/drag_finish`: `{x, y}`.
pub fn drag_finish(dir: &Path, cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let mut router = match cell.lock().unwrap_or_else(|e| e.into_inner()).take() {
        Some(r) => r,
        None => return err("not dragging"),
    };
    let at = point_of(&req);
    let Some(commit) = router.drag_finish(at) else {
        return json!({ "ok": false, "message": "drag target still collides; left uncommitted" });
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

fn dp_preview_json(router: &Router, preview: &eda_pns::diff_pair::DiffPairPreview) -> Value {
    let (net_a, net_b) = router.diff_pair_nets().unwrap_or((None, None));
    let layer_name = router.layer_name(preview.layer);
    json!({
        "ok": true,
        "net_a": net_a,
        "net_b": net_b,
        "layer": layer_name,
        "width": preview.width,
        "colliding": preview.colliding,
        "head_a": pts_json(&preview.head_a.pts),
        "head_b": pts_json(&preview.head_b.pts),
        "runs_a": preview.runs_a.iter().map(|r| json!({ "layer": layer_name, "pts": pts_json(&r.pts) })).collect::<Vec<_>>(),
        "runs_b": preview.runs_b.iter().map(|r| json!({ "layer": layer_name, "pts": pts_json(&r.pts) })).collect::<Vec<_>>(),
        "snapped_end": preview.snapped_end,
    })
}

/// `POST /api/route/dp_start`: `{x, y, layer, width?, gap?}` -- `width`/`gap` are the "Differential Pair Dimensions..." dialog's
/// custom values (`BOARD_DESIGN_SETTINGS::UseCustomDiffPairDimensions`); without them the pair takes the board rules' own.
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
            router.set_diff_pair_dimensions(req.get("width").and_then(Value::as_i64), req.get("gap").and_then(Value::as_i64));
            let reply = router.diff_pair_preview(at).map(|p| dp_preview_json(&router, &p)).unwrap_or_else(|| err("internal: started but no preview"));
            *cell.lock().unwrap_or_else(|e| e.into_inner()) = Some(router);
            reply
        }
        Err(message) => err(message),
    }
}

/// `POST /api/route/dp_dims`: `{width?, gap?}` -- `ROUTER_TOOL::DpDimensionsDialog` (`m_router->UpdateSizes`) for the pair being routed now.
pub fn dp_dims(cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    let Some(router) = guard.as_mut() else { return err("not routing a diff pair") };
    if router.set_diff_pair_dimensions(req.get("width").and_then(Value::as_i64), req.get("gap").and_then(Value::as_i64)) {
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
        return json!({ "ok": false, "message": "final segment still collides; diff pair left uncommitted" });
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
        let model = ConstraintModel::default();
        let intent_path = dir.join("intent.yaml");
        std::fs::write(&intent_path, serde_yaml::to_string(&model).unwrap()).unwrap();
        let stub = Track { id: "sig".into(), net: "SIG".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![p(10_000, 9_000), p(10_000, 8_000)], arc_mid_offset: None };
        let via = Via { id: "v_gnd".into(), net: "GND".into(), at: p(10_000, 5_000), drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() };
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
            routing: Some(RoutingSection { tracks: vec![stub], vias: vec![via], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }),
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
        // the two a client sent before there was a `settings` object
        let legacy = settings_of(&json!({ "mode": "shove", "remove_loops": false }));
        assert_eq!((legacy.mode, legacy.remove_loops), (Mode::Shove, false));
    }

    #[test]
    fn every_setting_the_dialog_has_reaches_the_router() {
        let s = settings_of(&json!({ "settings": {
            "mode": "mark_obstacles", "optimizer_effort": "full", "shove_vias": false, "jump_over_obstacles": true, "remove_loops": false,
            "smart_pads": false, "allow_drc_violations": true, "free_angle_mode": true, "fix_all_segments": false,
            "shove_iteration_limit": 12, "walkaround_iteration_limit": 7, "via_force_prop_iteration_limit": 9, "walkaround_hug_length_threshold": 3.25,
        } }));
        assert_eq!((s.mode, s.optimizer_effort), (Mode::MarkObstacles, OptEffort::Full));
        assert_eq!((s.shove_vias, s.jump_over_obstacles, s.remove_loops, s.smart_pads, s.can_violate_drc, s.free_angle_mode, s.fix_all_segments), (false, true, false, false, true, true, false));
        assert_eq!((s.shove_iteration_limit, s.walkaround_iteration_limit, s.via_force_prop_iteration_limit, s.walkaround_hug_length_threshold), (12, 7, 9, 3.25));
        assert!(s.allow_drc_violations() && s.free_angle(), "both only count in Highlight collisions mode");
        // nonsense is clamped, not trusted
        let wild = settings_of(&json!({ "settings": { "shove_iteration_limit": -5, "via_force_prop_iteration_limit": 0, "walkaround_hug_length_threshold": -1 } }));
        assert_eq!((wild.shove_iteration_limit, wild.via_force_prop_iteration_limit, wild.walkaround_hug_length_threshold), (1, 1, 0.0));
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
}
