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
    })
}

/// `POST /api/route/start`: `{x, y, layer, width?, mode?}`. `mode` is one
/// of `"mark_obstacles" | "walkaround" | "shove"` (default `"walkaround"`,
/// matching KiCad's own default -- see `eda_pns::settings::Mode`).
pub fn start(dir: &Path, cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let (_, design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let mut router = Router::new(&design, &model);
    router.settings.mode = match req.get("mode").and_then(Value::as_str) {
        Some("mark_obstacles") => Mode::MarkObstacles,
        Some("shove") => Mode::Shove,
        _ => Mode::Walkaround,
    };
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
    })
}

/// `POST /api/route/drag_start`: `{x, y, layer}`. Builds a fresh `Router`
/// from the board as it stands right now, same as `start` -- a drag reads
/// the live board just as much as a route does.
pub fn drag_start(dir: &Path, cell: &RouteCell, body: &[u8]) -> Value {
    let req = body_json(body);
    let (_, design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let mut router = Router::new(&design, &model);
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
