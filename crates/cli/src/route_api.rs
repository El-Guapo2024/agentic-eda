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
use eda_pns::settings::Mode;
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

/// `POST /api/route/start`: `{x, y, layer, width?, mode?, remove_loops?}`.
/// `mode` is one of `"mark_obstacles" | "walkaround" | "shove"` (default
/// `"walkaround"`, matching KiCad's own default -- see
/// `eda_pns::settings::Mode`); `remove_loops` defaults to `true`
/// (`RoutingSettings::default()`'s own default).
pub fn start(dir: &Path, cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let (_, design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let mut router = Router::new(&design, &model);
    router.settings.mode = mode_of(&req);
    router.settings.remove_loops = req.get("remove_loops").and_then(Value::as_bool).unwrap_or(true);
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

/// `POST /api/route/drag_start`: `{x, y, layer, mode?}`. Builds a fresh
/// `Router` from the board as it stands right now, same as `start` -- a
/// drag reads the live board just as much as a route does. `mode` is the
/// same `Mode` a route session takes (`mode_of`'s own doc comment) --
/// `eda_pns::dragger::Dragger` reuses it exactly as upstream's `DRAGGER`
/// reuses `SHOVE`/`WALKAROUND`; no `remove_loops` here, a route-only
/// concept upstream's own `DRAGGER` never touches either.
pub fn drag_start(dir: &Path, cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let (_, design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let mut router = Router::new(&design, &model);
    router.settings.mode = mode_of(&req);
    let at = point_of(&req);
    let layer = req.get("layer").and_then(Value::as_str).unwrap_or("F.Cu");
    match router.drag_start(at, layer) {
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

/// `POST /api/route/dp_start`: `{x, y, layer}`.
pub fn dp_start(dir: &Path, cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let (_, design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let mut router = Router::new(&design, &model);
    let at = point_of(&req);
    let layer = req.get("layer").and_then(Value::as_str).unwrap_or("F.Cu");
    match router.start_diff_pair(at, layer) {
        Ok(()) => {
            let reply = router.diff_pair_preview(at).map(|p| dp_preview_json(&router, &p)).unwrap_or_else(|| err("internal: started but no preview"));
            *cell.lock().unwrap_or_else(|e| e.into_inner()) = Some(router);
            reply
        }
        Err(message) => err(message),
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
