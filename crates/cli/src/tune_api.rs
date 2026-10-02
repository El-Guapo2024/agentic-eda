//! HTTP handlers for length tuning (gap #7 task item 4, keys `7`/`8`/`9`):
//! `POST /api/tune_length/{preview,apply}`.
//!
//! Unlike `route_api.rs`'s route/drag/diff-pair sessions, this is entirely
//! **stateless** -- no `RouteCell`, no in-progress session at all. See
//! `eda_pns::meander`'s own doc comment for why: this port exposes length
//! tuning as a one-shot "lengthen this track to a target length" dialog-
//! driven computation (`web/studio/src/components/LengthTuningDialog.tsx`)
//! rather than a third live mouse-driven interactive session, so every
//! call here reads `design.json` fresh (same as every other `/api/*`
//! endpoint outside the route sessions) and there is nothing to hold
//! between `preview` and `apply` beyond what the request body itself
//! carries.
//!
//! Scope (see `eda_pns::meander`'s own doc comment for the full reasoning):
//! only an existing track that is a **straight, single-segment** run (its
//! IR `pts` has exactly 2 points) can be tuned -- reported back as a
//! `message`, not a panic, for anything else. No diff-pair/skew tuning
//! (keys `8`/`9`) yet -- single-track only (`7`).

use crate::board;
use eda_model::ir::{Point, Track, Um};
use eda_pns::item::Item;
use eda_pns::meander::generate_meander;
use serde_json::{json, Value};
use std::path::Path;

fn err(message: impl Into<String>) -> Value {
    json!({ "ok": false, "message": message.into() })
}

fn num(v: &Value, key: &str, default: i64) -> i64 {
    v.get(key).and_then(Value::as_i64).unwrap_or(default)
}

/// `{track_id, amplitude, spacing, target_length, flip?}` -> the source
/// track (validated straight/single-segment), the generated meander, and
/// whether it collides with anything else already on the board (same
/// net excluded, this track's own original segments excluded -- the same
/// convention every other preview in this crate uses).
fn compute(dir: &Path, req: &Value) -> Result<(Track, eda_pns::meander::MeanderResult, bool), String> {
    let (_, design, model) = board::load(dir).map_err(|e| board::reasons(&e))?;
    let track_id = req.get("track_id").and_then(Value::as_str).ok_or("missing track_id")?;
    let track = design.routing.as_ref().and_then(|r| r.tracks.iter().find(|t| t.id == track_id)).ok_or("no such track")?.clone();
    if track.pts.len() != 2 {
        return Err("select a straight track with no corners or vias -- this port's length tuner doesn't yet handle a multi-segment run".into());
    }
    let amplitude = num(req, "amplitude", 200) as Um;
    let spacing = num(req, "spacing", 400) as Um;
    let target_length = num(req, "target_length", 0) as Um;
    let flip = req.get("flip").and_then(Value::as_bool).unwrap_or(false);
    let meander = generate_meander(track.pts[0], track.pts[1], amplitude, spacing, target_length, flip)
        .ok_or("can't reach that target length -- it may already be at or below the track's own straight length, or the amplitude/spacing leave no room at all on this short a run")?;

    let (node, layers) = eda_pns::from_ir::build_node(&design, &model);
    let layer = layers.index_of(&track.layer).ok_or_else(|| format!("unknown layer {:?}", track.layer))?;
    let exclude: Vec<_> = node
        .iter()
        .filter_map(|(id, item)| match item {
            Item::Segment(s) if s.source_track.as_ref().map(|(t, _)| t.as_str()) == Some(track_id) => Some(id),
            _ => None,
        })
        .collect();
    let net = eda_pns::item::net_of(&track.net);
    let layers_range = eda_pns::layer::LayerRange::single(layer);
    let colliding = meander.pts.windows(2).any(|w| {
        let shape = eda_drc::kimath::Shape::Stadium { a: w[0], b: w[1], r: track.width.max(1) / 2 };
        node.first_colliding(&shape, &net, layers_range, &model.board, &exclude).is_some()
    });
    Ok((track, meander, colliding))
}

fn pts_json(pts: &[Point]) -> Value {
    json!(pts.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>())
}

/// `POST /api/tune_length/preview`: read-only, never touches `design.json`.
pub fn preview(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    match compute(dir, &req) {
        Ok((track, meander, colliding)) => json!({
            "ok": true,
            "net": track.net,
            "layer": track.layer,
            "width": track.width,
            "original_length": eda_pns::meander::polyline_length(&track.pts),
            "achieved_length": meander.achieved_length,
            "pts": pts_json(&meander.pts),
            "colliding": colliding,
        }),
        Err(message) => err(message),
    }
}

/// `POST /api/tune_length/apply`: same body as `preview`; commits through
/// `Cmd::CommitRoute` (`crates/ops`) -- the exact same undo-step/
/// collision-replacement shape a finished route or drag already uses, so
/// this needed no new `Cmd` variant at all, just another caller of the
/// existing one. Refuses (without committing) if the result still
/// collides, same as every other interactive-router finish in this crate.
pub fn apply(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let (track, meander, colliding) = match compute(dir, &req) {
        Ok(v) => v,
        Err(message) => return err(message),
    };
    if colliding {
        return json!({ "ok": false, "message": "the tuned shape collides with something else on the board; adjust amplitude/spacing or target length" });
    }
    let new_track = Track { id: String::new(), net: track.net.clone(), pins: track.pins.clone(), layer: track.layer.clone(), width: track.width, pts: meander.pts, arc_mid_offset: None };
    let cmd = eda_ops::Cmd::CommitRoute { remove_track_ids: vec![track.id.clone()], remove_via_ids: vec![], tracks: vec![new_track], vias: vec![] };
    match board::step(dir, cmd, true, "ui") {
        Ok(summary) => json!({ "ok": true, "message": summary, "achieved_length": meander.achieved_length }),
        Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
    }
}
