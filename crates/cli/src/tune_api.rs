//! HTTP handlers for length tuning (`7` / `8` / `9`, gap #7 and package P5c): `POST /api/tune/*`.
//!
//! The tool is KiCad's `DRAWING_TOOL::PlaceTuningPattern` (see `tune_edit.rs`): hover a track and the status box shows its length against the target
//! (`hover`), the first click starts a pattern on the track (`start`), the cursor sets where it ends and the router's placer fits the meander to it
//! (`move`), the keys `1`-`4` step the spacing and the amplitude (`step`), the dialog's settings replace the tool's (`settings`), the second click puts the
//! pattern on the board as one undo step (`fix`) and Escape ends it (`cancel`). A pattern that is on the board is a generator item: `patterns` lists
//! them, `pattern` is one with the target a design rule would give it, `update` tunes it again with new settings, `remove` puts the track back as it was
//! and `regenerate` tunes a number of them again, each of these one undo step.
//!
//! One session at a time, in this thread's memory (the server answers its requests on one thread, and the router's world is not `Send`): `start` builds the
//! world from the board as it is and every `move` after it fits the meander in that world, so nothing re-reads the board between the clicks. The world of
//! the board as `design.json` has it is kept between requests (it is what the hover reads) and built again when the file changes.
//!
//! Lengths are millimetres in the replies' `readout` (`current`, `min`, `opt`, `max`, `delta`) and micrometres everywhere else, as the rest of the studio's
//! API has them.

use eda_model::units::Nm;
use crate::board;
use crate::tune_edit::{self, EditCtx, Moved, Pick, Scene, Tuner};
use eda_model::ir::{Design, Point};
use eda_model::tuning::{TuningMode, TuningPattern};
use eda_ops::Cmd;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

type Stamp = Vec<Option<(SystemTime, u64)>>;

struct Session {
    tuner: Tuner,
    stamp: Stamp,
    /// A pattern of the board being tuned again (`genStartEdit`): what the tuner does is not a new pattern but a change of this one.
    edit: Option<EditCtx>,
}

thread_local! {
    static SESSIONS: RefCell<HashMap<PathBuf, Session>> = RefCell::new(HashMap::new());
    static SCENES: RefCell<HashMap<PathBuf, (Stamp, Rc<Scene>)>> = RefCell::new(HashMap::new());
}

fn body_json(body: &[u8]) -> Value {
    serde_json::from_slice(body).unwrap_or(Value::Null)
}

fn err(message: impl Into<String>) -> Value {
    json!({ "ok": false, "message": message.into() })
}

fn point_of(v: &Value, key: &str) -> Option<Point> {
    let p = v.get(key)?;
    Some(Point { x: Nm::from_raw_shim(p.get("x")?.as_f64()?.round() as i64), y: Nm::from_raw_shim(p.get("y")?.as_f64()?.round() as i64) })
}

fn xy(v: &Value) -> Option<Point> {
    Some(Point { x: Nm::from_raw_shim(v.get("x")?.as_f64()?.round() as i64), y: Nm::from_raw_shim(v.get("y")?.as_f64()?.round() as i64) })
}

fn mode_of(v: &Value) -> TuningMode {
    match v.get("mode").and_then(Value::as_str) {
        Some("diffpair" | "diff_pair") => TuningMode::DiffPair,
        Some("skew" | "diff_pair_skew") => TuningMode::DiffPairSkew,
        _ => TuningMode::Single,
    }
}

fn mode_name(m: TuningMode) -> &'static str {
    match m {
        TuningMode::Single => "single",
        TuningMode::DiffPair => "diffpair",
        TuningMode::DiffPairSkew => "skew",
    }
}

/// What the files of the board look like: the board's design and metadata, and the intent the model is made from.
fn stamp_of(dir: &Path) -> Stamp {
    let one = |p: PathBuf| std::fs::metadata(p).ok().and_then(|m| Some((m.modified().ok()?, m.len())));
    let intent = std::fs::read_to_string(dir.join("board.json")).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()).and_then(|v| v.get("intent").and_then(Value::as_str).map(PathBuf::from));
    vec![one(dir.join("design.json")), one(dir.join("board.json")), intent.and_then(one)]
}

/// The board of `dir` and its world: the one built before when the files have not changed since.
fn scene(dir: &Path) -> Result<Rc<Scene>, String> {
    let stamp = stamp_of(dir);
    if let Some(s) = SCENES.with(|c| c.borrow().get(dir).filter(|(st, _)| *st == stamp && st.iter().all(Option::is_some)).map(|(_, s)| s.clone())) {
        return Ok(s);
    }
    let (_, design, model) = board::load(dir).map_err(|e| board::reasons(&e))?;
    let s = Rc::new(Scene::of(design, model));
    SCENES.with(|c| c.borrow_mut().insert(dir.to_path_buf(), (stamp, s.clone())));
    Ok(s)
}

fn pick_json(pk: &Pick) -> Value {
    json!({ "track_id": pk.track_id, "net": pk.net, "layer": pk.layer, "width": pk.width, "at": { "x": pk.at.x, "y": pk.at.y } })
}

fn pts_json(pts: &[Point]) -> Value {
    Value::Array(pts.iter().map(|p| json!([p.x, p.y])).collect())
}

fn pattern_json(p: &TuningPattern) -> Value {
    serde_json::to_value(p).unwrap_or(Value::Null)
}

/// The pattern's four edit points (`MakeEditPoints`).
pub fn handles_json(p: &TuningPattern) -> Value {
    Value::Array(tune_edit::handle_positions(p).iter().map(|h| json!([h.x, h.y])).collect())
}

/// The cursor's pick for a request: `{x, y, tolerance, layer}`.
fn pick_for(scene: &Scene, req: &Value) -> Option<Pick> {
    let at = xy(req)?;
    let tolerance = req.get("tolerance").and_then(Value::as_f64).unwrap_or(150_000.0).round() as i64;
    tune_edit::pick(scene, at.x, at.y, Nm::from_raw_shim(tolerance), req.get("layer").and_then(Value::as_str))
}

fn settings_of(req: &Value) -> Value {
    req.get("settings").filter(|s| s.is_object()).cloned().unwrap_or(Value::Null)
}

/// `POST /api/tune/hover` `{x, y, tolerance?, layer?, mode, settings?}`: the track under the cursor and the length it starts from. Never touches the board.
pub fn hover(dir: &Path, body: &[u8]) -> Value {
    let req = body_json(body);
    let scene = match scene(dir) {
        Ok(s) => s,
        Err(m) => return err(m),
    };
    let Some(pk) = pick_for(&scene, &req) else { return json!({ "ok": true, "hit": false }) };
    let kind = mode_of(&req);
    match tune_edit::hover(&scene, kind, &pk, &settings_of(&req)) {
        Ok(h) => json!({ "ok": true, "hit": true, "pick": pick_json(&pk), "mode": mode_name(kind), "readout": h.readout.json(), "path": pts_json(&h.path), "pattern": pattern_json(&h.pattern) }),
        Err(m) => json!({ "ok": true, "hit": true, "pick": pick_json(&pk), "mode": mode_name(kind), "message": m }),
    }
}

fn session_reply(s: &Session, moved: Option<&Moved>) -> Value {
    let t = &s.tuner;
    let mut reply = json!({ "ok": true, "mode": mode_name(t.pattern.mode), "pattern": pattern_json(&t.pattern), "net": t.net, "editing": s.edit.is_some() });
    reply["handles"] = handles_json(&t.pattern);
    if let Some(m) = moved {
        reply["moved"] = json!(m.moved && !m.edits.is_empty());
        reply["readout"] = m.readout.json();
        reply["preview"] = tune_edit::preview_json(&t.scene, m);
    }
    reply
}

/// `POST /api/tune/start` `{x, y, tolerance?, layer?, mode, settings?}`: the first click. Starts a session on the track under the cursor (the previous one, if any, ends).
pub fn start(dir: &Path, body: &[u8]) -> Value {
    let req = body_json(body);
    let scene = match scene(dir) {
        Ok(s) => s,
        Err(m) => return err(m),
    };
    let Some(pk) = pick_for(&scene, &req) else { return err("Select a track to tune first.") };
    // `DRAWING_TOOL::PlaceTuningPattern`: "if( dynamic_cast<PCB_TUNING_PATTERN*>( m_pickerItem->GetParentGroup() ) ) ShowInfoBarWarning( ..
    // )" -- a track of another pattern is not tuned again from here (its own pattern is edited instead; findings p5 F4-4)
    if tune_edit::patterns_of(&scene.design).iter().any(|pat| pat.member_ids.contains(&pk.track_id)) {
        return err("Unable to tune segments inside other tuning patterns.");
    }
    let kind = mode_of(&req);
    // the pattern starts at the point of the track nearest the click (`snapToNearestTrack`, the picked item's focus position)
    let tuner = match Tuner::start(scene.clone(), kind, &pk, pk.at, &settings_of(&req)) {
        Ok(t) => t,
        Err(m) => return err(m),
    };
    let stamp = stamp_of(dir);
    let session = Session { tuner, stamp, edit: None };
    let reply = {
        let mut r = session_reply(&session, None);
        r["origin"] = json!({ "x": session.tuner.pattern.origin.x, "y": session.tuner.pattern.origin.y });
        r["pick"] = pick_json(&pk);
        r
    };
    SESSIONS.with(|s| s.borrow_mut().insert(dir.to_path_buf(), session));
    reply
}

fn with_session<R>(dir: &Path, f: impl FnOnce(&mut Session) -> R) -> Option<R> {
    SESSIONS.with(|s| s.borrow_mut().get_mut(dir).map(f))
}

const NO_SESSION: &str = "No tuning pattern is being placed: click a track to start one.";

/// `POST /api/tune/move` `{x, y}`: the cursor moved; the meander is fitted from the start of the pattern to it.
pub fn mv(dir: &Path, body: &[u8]) -> Value {
    let req = body_json(body);
    let Some(to) = xy(&req) else { return err("missing x/y") };
    with_session(dir, |s| match s.tuner.mv(to, true) {
        Ok(_) => session_reply(s, s.tuner.last()),
        Err(m) => err(m),
    })
    .unwrap_or_else(|| err(NO_SESSION))
}

/// `POST /api/tune/step` `{what}` (`amplitude_up`, `amplitude_down`, `spacing_up`, `spacing_down`): the `1`-`4` keys, then the meander is fitted again.
pub fn step(dir: &Path, body: &[u8]) -> Value {
    let req = body_json(body);
    let what = req.get("what").and_then(Value::as_str).unwrap_or("").to_string();
    with_session(dir, |s| {
        if !s.tuner.step(&what) {
            return err(format!("unknown step {what:?}"));
        }
        refit(s)
    })
    .unwrap_or_else(|| err(NO_SESSION))
}

/// `POST /api/tune/settings` `{settings}`: the dialog's settings for the pattern being placed.
pub fn settings(dir: &Path, body: &[u8]) -> Value {
    let req = body_json(body);
    with_session(dir, |s| {
        s.tuner.set_settings(&settings_of(&req));
        refit(s)
    })
    .unwrap_or_else(|| err(NO_SESSION))
}

fn refit(s: &mut Session) -> Value {
    if let Some(ctx) = &s.edit {
        let end = ctx.end;
        return match tune_edit::fit(&mut s.tuner, end) {
            Ok(_) => session_reply(s, s.tuner.last()),
            Err(m) => err(m),
        };
    }
    match s.tuner.last_end() {
        Some(end) => match s.tuner.mv(end, true) {
            Ok(_) => session_reply(s, s.tuner.last()),
            Err(m) => err(m),
        },
        None => session_reply(s, None),
    }
}

/// `POST /api/tune/fix` `{x?, y?}`: the second click. Puts the pattern on the board (`Cmd::CommitTuning`, one undo step). The end of an edit of a pattern that
/// exists (`/api/tune/edit_start`) is this call too.
pub fn fix(dir: &Path, body: &[u8]) -> Value {
    let req = body_json(body);
    let session = SESSIONS.with(|s| s.borrow_mut().remove(dir));
    let Some(mut session) = session else { return err(NO_SESSION) };
    if stamp_of(dir) != session.stamp {
        return err("The board changed while the pattern was being placed: start it again.");
    }
    if session.edit.is_none() {
        if let Some(to) = xy(&req) {
            if session.tuner.last_end() != Some(to) {
                if let Err(m) = session.tuner.mv(to, true) {
                    return err(m);
                }
            }
        }
    }
    let cmd = match (session.tuner.command(), &session.edit) {
        (Ok(c), Some(ctx)) => Cmd::Batch { cmds: vec![ctx.reset.clone(), c] },
        (Ok(c), None) => c,
        (Err(m), _) => {
            // nothing to commit yet: the session goes on
            SESSIONS.with(|s| s.borrow_mut().insert(dir.to_path_buf(), session));
            return err(m);
        }
    };
    let readout = session.tuner.last().map(|m| m.readout.json());
    let p = session.tuner.pattern.clone();
    match board::step(dir, cmd, true, "ui") {
        Ok(summary) => {
            let id = board::load(dir).ok().and_then(|(_, d, _)| tune_edit::patterns_of(&d).into_iter().find(|q| q.mode == p.mode && q.layer == p.layer && (q.origin == p.origin || (!p.id.is_empty() && q.id == p.id))).map(|q| q.id));
            json!({ "ok": true, "message": summary, "pattern_id": id, "readout": readout })
        }
        Err(e) => err(board::reasons(&e)),
    }
}

/// `POST /api/tune/edit_start` `{id, settings?}`: `genStartEdit` -- the pattern's tracks are put back to the baseline in the session's board and the placer is
/// started over it; nothing is changed on the board until `fix`. The reply is the pattern as it fits now, with the four edit points.
pub fn edit_start(dir: &Path, body: &[u8]) -> Value {
    let req = body_json(body);
    let scene = match scene(dir) {
        Ok(s) => s,
        Err(m) => return err(m),
    };
    let id = req.get("id").and_then(Value::as_str).unwrap_or("");
    let p = match find_pattern(&scene.design, id) {
        Ok(p) => p,
        Err(m) => return err(m),
    };
    let mut r = match tune_edit::begin_retune(&scene, &p, &settings_of(&req)) {
        Ok(r) => r,
        Err(m) => return err(m),
    };
    if let Err(m) = r.fit() {
        return err(m);
    }
    let session = Session { tuner: r.tuner, stamp: stamp_of(dir), edit: Some(r.ctx) };
    let reply = session_reply(&session, session.tuner.last());
    SESSIONS.with(|s| s.borrow_mut().insert(dir.to_path_buf(), session));
    reply
}

/// `POST /api/tune/edit_move` `{handle, x, y}` or `{settings}`: `genUpdateEdit` -- an edit point (0 the start, 1 the end, 2 the width of the meanders and the side
/// they start on, 3 their spacing) is at `(x, y)`, or the dialog's settings change; the pattern is fitted again.
pub fn edit_move(dir: &Path, body: &[u8]) -> Value {
    let req = body_json(body);
    with_session(dir, |s| {
        let Some(ctx) = s.edit.as_mut() else { return err("No tuning pattern is being edited.") };
        if let Some(settings) = req.get("settings").filter(|v| v.is_object()) {
            s.tuner.set_settings(settings);
        }
        if let (Some(h), Some(at)) = (req.get("handle").and_then(Value::as_u64), xy(&req)) {
            let mut moved = s.tuner.pattern.clone();
            tune_edit::apply_handle(&mut moved, h as usize, at);
            match h {
                0 => match ctx.tuner_at(moved.origin, &s.tuner.pattern) {
                    Ok(t) => s.tuner = t,
                    Err(m) => return err(m),
                },
                1 => ctx.end = moved.end,
                _ => {
                    s.tuner.pattern.max_amplitude = moved.max_amplitude;
                    s.tuner.pattern.spacing = moved.spacing;
                    s.tuner.pattern.initial_side = moved.initial_side;
                }
            }
        }
        let end = ctx.end;
        match tune_edit::fit(&mut s.tuner, end) {
            Ok(_) => session_reply(s, s.tuner.last()),
            Err(m) => err(m),
        }
    })
    .unwrap_or_else(|| err(NO_SESSION))
}

/// `POST /api/tune/cancel`: Escape; the session ends, nothing was changed.
pub fn cancel(dir: &Path) -> Value {
    let had = SESSIONS.with(|s| s.borrow_mut().remove(dir)).is_some();
    json!({ "ok": true, "had_session": had })
}

fn find_pattern(design: &Design, id: &str) -> Result<TuningPattern, String> {
    tune_edit::patterns_of(design).into_iter().find(|p| p.id == id).ok_or_else(|| format!("no tuning pattern {id:?}"))
}

fn nets_of(design: &Design, p: &TuningPattern) -> Vec<String> {
    let mut nets: Vec<String> = Vec::new();
    if let Some(rt) = &design.routing {
        for m in &p.member_ids {
            if let Some(t) = rt.tracks.iter().find(|t| &t.id == m) {
                if !nets.contains(&t.net) {
                    nets.push(t.net.clone());
                }
            }
        }
    }
    nets
}

/// `POST /api/tune/patterns`: the tuning patterns of the board, as the Generators Manager lists them.
pub fn patterns(dir: &Path) -> Value {
    let scene = match scene(dir) {
        Ok(s) => s,
        Err(m) => return err(m),
    };
    let locked: Vec<&String> = scene.design.drawings.iter().flat_map(|d| d.locked_ids.iter()).collect();
    let list: Vec<Value> = tune_edit::patterns_of(&scene.design)
        .iter()
        .map(|p| {
            json!({
                "id": p.id, "name": "Tuning Pattern", "mode": mode_name(p.mode), "layer": p.layer, "nets": nets_of(&scene.design, p),
                "members": p.member_ids, "info": p.tuning_info(), "status": p.last_status, "length": p.last_tuning_length.raw_shim() as f64 / 1e6,
                "origin": { "x": p.origin.x, "y": p.origin.y }, "end": { "x": p.end.x, "y": p.end.y }, "locked": locked.contains(&&p.id),
            })
        })
        .collect();
    json!({ "ok": true, "patterns": list })
}

/// `POST /api/tune/pattern` `{id}`: a pattern with the target a design rule would give it (`PCB_TUNING_PATTERN::ShowPropertiesDialog`).
pub fn pattern(dir: &Path, body: &[u8]) -> Value {
    let req = body_json(body);
    let scene = match scene(dir) {
        Ok(s) => s,
        Err(m) => return err(m),
    };
    let id = req.get("id").and_then(Value::as_str).unwrap_or("");
    let p = match find_pattern(&scene.design, id) {
        Ok(p) => p,
        Err(m) => return err(m),
    };
    let nets = nets_of(&scene.design, &p);
    let primary = nets.first().cloned().unwrap_or_else(|| p.last_netname.clone());
    let partner = nets.get(1).cloned();
    let constraint = tune_edit::constraint_for(&scene.env.rules, p.mode, &primary, partner.as_deref(), &p.layer);
    json!({
        "ok": true, "pattern": pattern_json(&p), "mode": mode_name(p.mode), "nets": nets, "info": p.tuning_info(),
        "constraint": constraint.map(|c| json!({ "name": c.name, "min": c.range.min, "opt": c.range.opt, "max": c.range.max })),
    })
}

fn board_stepped(dir: &Path, cmds: Vec<Cmd>) -> Result<String, String> {
    let cmd = if cmds.len() == 1 { cmds.into_iter().next().unwrap() } else { Cmd::Batch { cmds } };
    board::step(dir, cmd, true, "ui").map_err(|e| board::reasons(&e))
}

/// `POST /api/tune/update` `{id, settings}`: the pattern tuned again with these settings (`ShowPropertiesDialog`'s OK, `EditStart`, `Update`, `EditFinish`).
pub fn update(dir: &Path, body: &[u8]) -> Value {
    let req = body_json(body);
    let scene = match scene(dir) {
        Ok(s) => s,
        Err(m) => return err(m),
    };
    let id = req.get("id").and_then(Value::as_str).unwrap_or("");
    let p = match find_pattern(&scene.design, id) {
        Ok(p) => p,
        Err(m) => return err(m),
    };
    let mut settings = settings_of(&req);
    for key in ["origin", "end"] {
        if let (Some(pt), Some(obj)) = (point_of(&req, key), settings.as_object_mut()) {
            obj.insert(key.into(), json!({ "x": pt.x, "y": pt.y }));
        }
    }
    if settings.is_null() {
        settings = json!({});
        for key in ["origin", "end"] {
            if let Some(pt) = point_of(&req, key) {
                settings[key] = json!({ "x": pt.x, "y": pt.y });
            }
        }
    }
    match tune_edit::retune_cmds(&scene, &p, &settings).and_then(|cmds| board_stepped(dir, cmds)) {
        Ok(summary) => {
            let after = board::load(dir).ok().and_then(|(_, d, _)| tune_edit::patterns_of(&d).into_iter().find(|q| q.id == p.id));
            json!({ "ok": true, "message": summary, "pattern": after.as_ref().map(pattern_json), "info": after.map(|q| q.tuning_info()) })
        }
        Err(m) => err(m),
    }
}

/// `POST /api/tune/remove` `{id}`: the pattern's tracks go back to the baseline and the pattern is gone (`PCB_TUNING_PATTERN::Remove`).
pub fn remove(dir: &Path, body: &[u8]) -> Value {
    let req = body_json(body);
    let scene = match scene(dir) {
        Ok(s) => s,
        Err(m) => return err(m),
    };
    let id = req.get("id").and_then(Value::as_str).unwrap_or("");
    let p = match find_pattern(&scene.design, id) {
        Ok(p) => p,
        Err(m) => return err(m),
    };
    match tune_edit::reset_cmd(&scene, &p).and_then(|c| board_stepped(dir, vec![c])) {
        Ok(summary) => json!({ "ok": true, "message": summary }),
        Err(m) => err(m),
    }
}

/// `POST /api/tune/regenerate` `{ids?}`: every pattern of the list (all of them without one) tuned again on the board as it is, one after the other, one undo step
/// (`GENERATOR_TOOL::RegenerateAllOfType` / `RegenerateSelected`).
pub fn regenerate(dir: &Path, body: &[u8]) -> Value {
    let req = body_json(body);
    let scene0 = match scene(dir) {
        Ok(s) => s,
        Err(m) => return err(m),
    };
    let all = tune_edit::patterns_of(&scene0.design);
    let wanted: Option<Vec<String>> = req.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect());
    let chosen: Vec<TuningPattern> = all.into_iter().filter(|p| wanted.as_ref().is_none_or(|w| w.contains(&p.id))).collect();
    if chosen.is_empty() {
        return err("There are no tuning patterns to regenerate.");
    }
    let mut cmds: Vec<Cmd> = Vec::new();
    let mut current = Scene::of(scene0.design.clone(), scene0.model.clone());
    let mut failed: Vec<String> = Vec::new();
    for p in &chosen {
        // the pattern as the board has it now (an earlier one in the list may have moved its tracks)
        let Ok(now) = find_pattern(&current.design, &p.id) else { continue };
        match tune_edit::retune_cmds(&current, &now, &Value::Null) {
            Ok(pair) => match tune_edit::apply_all(&current, &pair) {
                Ok(next) => {
                    cmds.extend(pair);
                    current = next;
                }
                Err(m) => failed.push(format!("{}: {m}", p.id)),
            },
            Err(m) => failed.push(format!("{}: {m}", p.id)),
        }
    }
    if cmds.is_empty() {
        return err(format!("No tuning pattern could be regenerated. {}", failed.join(" ")));
    }
    match board_stepped(dir, cmds) {
        Ok(summary) => json!({ "ok": true, "message": summary, "regenerated": chosen.len() - failed.len(), "failed": failed }),
        Err(m) => err(m),
    }
}

#[cfg(test)]
mod tests {
    use eda_model::units::{mm, Nm};
    use super::*;
    use eda_model::ir::{FootprintInstance, PlacementSection, Provenance, Side};
    use eda_model::{ConstraintModel, Footprint, Net, Pad, PadKind, PadShape, Part, Pin, PinKind};

    /// A scratch project directory, removed again when the test ends.
    struct Scratch(PathBuf);
    impl std::ops::Deref for Scratch {
        type Target = PathBuf;
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
        let d = std::env::temp_dir().join(format!("eda_cli_tune_test_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Scratch(d)
    }

    fn p(x: i64, y: i64) -> Point {
        Point { x: Nm::from_raw_shim(x), y: Nm::from_raw_shim(y) }
    }

    /// Two parts 20 mm apart, each with a pad above a pad (1 mm pitch): nets `<a>` and `<b>` run from one to the other as 20 mm tracks. `rules` are custom rules.
    fn setup(dir: &Path, nets: [&str; 2], rules: Vec<eda_model::CustomRule>) {
        let straight = |y: i64| vec![p(5_000_000, y), p(25_000_000, y)];
        setup_with(dir, nets, rules, vec![(nets[0], straight(9_500_000)), (nets[1], straight(10_500_000))]);
    }

    /// [`setup`] with the tracks the caller draws (each its own track).
    fn setup_with(dir: &Path, nets: [&str; 2], rules: Vec<eda_model::CustomRule>, tracks: Vec<(&str, Vec<Point>)>) {
        let pad = |n: &str, y: i64| Pad { opposite_side: false, ext: Default::default(), number: n.into(), at: (Nm::ZERO, Nm::from_raw_shim(y)), size: (mm(0.4), mm(0.4)), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None };
        let fp = Footprint { name: "PAIR2".into(), pads: vec![pad("1", -500_000), pad("2", 500_000)], courtyard: Some((mm(1.0), mm(2.0))), model: None, courtyard_outlines: vec![], models3d: vec![], art: Default::default() };
        let part = |r: &str| Part {
            reference: r.into(),
            mpn: None,
            lcsc: None,
            value: None,
            package: Some("PAIR2".into()),
            footprint: Some("PAIR2".into()),
            pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }],
            body_nm: None,
            symbol: None,
            datasheet: None,
            edge: None,
        };
        let mut model = ConstraintModel {
            parts: vec![part("U1"), part("U2")],
            nets: vec![Net { name: nets[0].into(), pins: vec!["U1.1".into(), "U2.1".into()] }, Net { name: nets[1].into(), pins: vec!["U1.2".into(), "U2.2".into()] }],
            footprints: vec![fp],
            ..Default::default()
        };
        model.board.custom_rules = rules;
        let intent_path = dir.join("intent.yaml");
        std::fs::write(&intent_path, serde_yaml::to_string(&model).unwrap()).unwrap();
        let design = Design {
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
            schema: eda_model::ir::SCHEMA_VERSION,
            provenance: Provenance { engine_version: "t".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection {
                outline: vec![p(0, 0), p(40_000_000, 0), p(40_000_000, 20_000_000), p(0, 20_000_000)],
                footprints: vec![
                    FootprintInstance { id: "U1".into(), at: p(5_000_000, 10_000_000), rot: 0, side: Side::Top, label: Default::default() },
                    FootprintInstance { id: "U2".into(), at: p(25_000_000, 10_000_000), rot: 0, side: Side::Top, label: Default::default() },
                ],
                modules: vec![],
            }),
            routing: None,
            drawings: None,
        };
        board::save(dir, &design).unwrap();
        let meta = board::Meta { intent: intent_path.display().to_string(), snap_nm: 100_000, spacing_nm: 300_000 };
        std::fs::write(dir.join("board.json"), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
        // (the derived schematic labels every net the intent names, so KiCad names them `/SIG`, `/OTHER`, `/USB_P` ...: the root's path and the intent's
        // name, a pair still a pair)
        for (net, pts) in tracks {
            let net = if net.starts_with('/') { net.to_string() } else { format!("/{net}") };
            let cmd = Cmd::AddTrack { net, layer: "F.Cu".into(), width: mm(0.25), pts };
            board::step(dir, cmd, false, "test").unwrap();
        }
    }

    fn body(v: Value) -> Vec<u8> {
        v.to_string().into_bytes()
    }

    fn design_of(dir: &Path) -> Design {
        board::load(dir).unwrap().1
    }

    fn tracks_of(d: &Design) -> Vec<eda_model::ir::Track> {
        d.routing.as_ref().map(|r| r.tracks.clone()).unwrap_or_default()
    }

    /// The length of a net's tuning path, millimetres.
    fn net_mm(dir: &Path, net: &str) -> f64 {
        let (_, design, model) = board::load(dir).unwrap();
        let scene = Scene::of(design, model);
        let id = scene
            .world
            .iter()
            .find_map(|(id, it)| match it {
                eda_pns::item::Item::Segment(s) if s.net.as_deref() == Some(net) => Some(id),
                _ => None,
            })
            .expect("a segment of the net");
        eda_pns::tuning_path::assemble_tuning_path(&scene.world, id).unwrap().length(&scene.world, &scene.env) as f64 / 1e6
    }

    #[test]
    fn hovering_a_track_reads_its_length_against_the_rule_and_places_nothing() {
        let dir = scratch("hover");
        let rule = eda_model::CustomRule { name: "sig_length".into(), constraint_type: "length".into(), min: Some(mm(21.9)), max: Some(mm(22.1)), opt: None, layer: None, severity: None, condition: None };
        setup(&dir, ["SIG", "OTHER"], vec![rule]);
        let before = std::fs::read_to_string(dir.join("design.json")).unwrap();
        let r = hover(&dir, &body(json!({ "x": 15_000_000, "y": 9_550_000, "tolerance": 100_000, "layer": "F.Cu", "mode": "single" })));
        assert_eq!(r["ok"], json!(true), "{r}");
        assert_eq!(r["hit"], json!(true), "{r}");
        assert_eq!(r["pick"]["net"], "/SIG");
        assert_eq!(r["readout"]["label"], "current length");
        assert!((r["readout"]["current"].as_f64().unwrap() - 20.0).abs() < 0.01, "{r}");
        // the rule's range, `Opt()` being its minimum
        assert_eq!((r["readout"]["min"].as_f64(), r["readout"]["max"].as_f64(), r["readout"]["opt"].as_f64()), (Some(21.9), Some(22.1), Some(21.9)));
        assert_eq!(r["readout"]["status"], "too_short");
        assert_eq!(r["readout"]["constraint"], "sig_length");
        assert!((r["readout"]["delta"].as_f64().unwrap() + 1.9).abs() < 0.01);
        assert_eq!(r["path"].as_array().unwrap().len(), 2);
        // a pattern that overrides the rules has its own target
        let own = hover(&dir, &body(json!({ "x": 15_000_000, "y": 9_550_000, "layer": "F.Cu", "mode": "single", "settings": { "override_custom_rules": true, "target_length": 21_000_000 } })));
        assert_eq!((own["readout"]["opt"].as_f64(), own["readout"]["constraint"].is_null()), (Some(21.0), true), "{own}");
        // nothing under the cursor
        assert_eq!(hover(&dir, &body(json!({ "x": 15_000_000, "y": 5_000_000, "layer": "F.Cu" })))["hit"], json!(false));
        assert_eq!(std::fs::read_to_string(dir.join("design.json")).unwrap(), before, "hovering does not change the board");
    }

    #[test]
    fn a_single_track_is_tuned_to_the_target_as_one_undo_step() {
        let dir = scratch("single");
        setup(&dir, ["SIG", "OTHER"], vec![]);
        let before = design_of(&dir);
        let s = start(&dir, &body(json!({ "x": 8_000_000, "y": 9_500_000, "layer": "F.Cu", "mode": "single", "settings": { "target_length": 22_000_000 } })));
        assert_eq!(s["ok"], json!(true), "{s}");
        assert_eq!(s["origin"], json!({ "x": 8_000_000, "y": 9_500_000 }));
        // before the cursor moves there is nothing to put on the board
        assert_eq!(fix(&dir, &body(json!({}))).get("ok"), Some(&json!(false)));
        let start_again = start(&dir, &body(json!({ "x": 8_000_000, "y": 9_500_000, "layer": "F.Cu", "mode": "single", "settings": { "target_length": 22_000_000 } })));
        assert_eq!(start_again["ok"], json!(true));
        let m = mv(&dir, &body(json!({ "x": 22_000_000, "y": 9_000_000 })));
        assert_eq!(m["ok"], json!(true), "{m}");
        assert_eq!(m["moved"], json!(true), "{m}");
        assert_eq!(m["readout"]["status"], "tuned", "{m}");
        assert!((m["readout"]["current"].as_f64().unwrap() - 22.0).abs() < 0.1, "{m}");
        assert!(m["preview"]["pieces"].as_array().unwrap().len() > 10);
        assert_eq!(m["preview"]["hide"].as_array().unwrap().len(), 1, "the track the meander replaces");
        // the cursor was above the track, so the first meander goes on its left (up on the screen: y is down)
        assert_eq!(m["pattern"]["initial_side"], "left");
        // the keys step the amplitude and the spacing, and the meander is fitted again
        let k = step(&dir, &body(json!({ "what": "amplitude_down" })));
        assert_eq!(k["pattern"]["max_amplitude"], json!(950_000), "{k}");
        let k = step(&dir, &body(json!({ "what": "spacing_up" })));
        assert_eq!(k["pattern"]["spacing"], json!(650_000), "{k}");
        assert!((k["readout"]["current"].as_f64().unwrap() - 22.0).abs() < 0.1, "still tuned: {k}");

        let f = fix(&dir, &body(json!({ "x": 22_000_000, "y": 9_000_000 })));
        assert_eq!(f["ok"], json!(true), "{f}");
        let after = design_of(&dir);
        let patterns = tune_edit::patterns_of(&after);
        assert_eq!(patterns.len(), 1);
        let pat = &patterns[0];
        assert_eq!(f["pattern_id"], json!(pat.id));
        assert_eq!((pat.mode, pat.layer.as_str(), pat.origin, pat.end), (TuningMode::Single, "F.Cu", p(8_000_000, 9_500_000), p(22_000_000, 9_500_000)));
        assert_eq!(pat.base_line, vec![p(8_000_000, 9_500_000), p(22_000_000, 9_500_000)]);
        assert_eq!((pat.max_amplitude, pat.spacing), (mm(0.95), mm(0.65)));
        assert!(pat.member_ids.len() > 10, "the meander's tracks are the members: {}", pat.member_ids.len());
        let rt = after.routing.as_ref().unwrap();
        assert!(pat.member_ids.iter().all(|m| rt.tracks.iter().any(|t| &t.id == m)));
        assert!(rt.tracks.iter().any(|t| t.arc().is_some()), "rounded corners are arc tracks");
        assert!((net_mm(&dir, "/SIG") - 22.0).abs() < 0.1, "{}", net_mm(&dir, "/SIG"));
        assert_eq!(pat.tuning_info().split(' ').last(), Some("(tuned)"));
        // the other net is untouched
        assert!((net_mm(&dir, "/OTHER") - 20.0).abs() < 0.001);

        // findings p5 F4-4: a track of the pattern does not start another pattern (`PlaceTuningPattern`'s warning)
        let member = rt.tracks.iter().find(|t| &t.id == pat.member_ids.last().unwrap() && t.arc().is_none() && t.pts.len() >= 2).expect("a straight member");
        let mid = p(((member.pts[0].x + member.pts[1].x) / 2).raw_shim(), ((member.pts[0].y + member.pts[1].y) / 2).raw_shim());
        let again = start(&dir, &body(json!({ "x": mid.x, "y": mid.y, "layer": "F.Cu", "mode": "single", "tolerance": 0 })));
        assert_eq!((again["ok"].clone(), again["message"].clone()), (json!(false), json!("Unable to tune segments inside other tuning patterns.")), "{again}");

        board::undo(&dir, "test", None).expect("one undo takes the whole pattern back");
        let back = design_of(&dir);
        assert!(tune_edit::patterns_of(&back).is_empty());
        assert_eq!(tracks_of(&back), tracks_of(&before), "the board is as it was");
    }

    /// The board's Design Rules > Tuning Profiles: the Default class names a profile of 6.1 ps/mm on F.Cu, with the time domain on or off.
    fn with_profile(dir: &Path, time_domain: bool) {
        use eda_model::setup_pages::{ProfileLayerEntry, ProfileType, TuningProfile, TuningProfiles};
        let intent = dir.join("intent.yaml");
        let mut model: ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(&intent).unwrap()).unwrap();
        let entry = ProfileLayerEntry { signal_layer: "F.Cu".into(), top_reference_layer: None, bottom_reference_layer: None, width_nm: mm(0.2), diff_pair_gap_nm: Nm::ZERO, delay_as_per_mm: 6_100_000 };
        let profile = TuningProfile { name: "FR4".into(), profile_type: ProfileType::Single, target_impedance: 50.0, enable_time_domain_tuning: time_domain, layer_entries: vec![entry], via_propagation_delay_as: 0, via_overrides: vec![] };
        model.board.tuning_profiles = TuningProfiles { profiles: vec![profile], class_profiles: [("Default".to_string(), "FR4".to_string())].into() };
        std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
    }

    fn net_ps(dir: &Path, net: &str) -> f64 {
        let (_, design, model) = board::load(dir).unwrap();
        let scene = Scene::of(design, model);
        let id = scene.world.iter().find_map(|(id, it)| match it {
            eda_pns::item::Item::Segment(s) if s.net.as_deref() == Some(net) => Some(id),
            _ => None,
        });
        let path = eda_pns::tuning_path::assemble_tuning_path(&scene.world, id.expect("a segment of the net")).unwrap();
        path.delay(&scene.world, &scene.env, &scene.env.delay_params(net).expect("a profile")) as f64 / 1e6
    }

    /// The time domain: a net whose class names a tuning profile with the time domain on starts a pattern in picoseconds; the target is a delay, the status box reads
    /// the delay, the pattern is stored as a time domain one, and the net is as slow as asked.
    #[test]
    fn a_track_is_tuned_to_a_delay_in_picoseconds_through_the_tuning_profile_of_its_class() {
        let dir = scratch("delay");
        setup(&dir, ["SIG", "OTHER"], vec![]);
        with_profile(&dir, true);
        // 20 mm at 6.1 ps/mm is 122 ps
        assert!((net_ps(&dir, "/SIG") - 122.0).abs() < 0.01);
        let h = hover(&dir, &body(json!({ "x": 15_000_000, "y": 9_550_000, "tolerance": 100_000, "layer": "F.Cu", "mode": "single" })));
        assert_eq!(h["ok"], json!(true), "{h}");
        assert_eq!(h["readout"]["time_domain"], json!(true), "the class names a profile with the time domain on: {h}");
        assert!((h["readout"]["delay"]["current"].as_f64().unwrap() - 122.0).abs() < 0.01, "{h}");
        assert_eq!(h["readout"]["delay"]["label"], "current delay");
        assert_eq!(h["pattern"]["is_time_domain"], json!(true));

        // 150 ps is 24.59 mm of F.Cu track
        let s = start(&dir, &body(json!({ "x": 8_000_000, "y": 9_500_000, "layer": "F.Cu", "mode": "single", "settings": { "target_delay": 150 } })));
        assert_eq!(s["ok"], json!(true), "{s}");
        let m = mv(&dir, &body(json!({ "x": 22_000_000, "y": 9_000_000 })));
        assert_eq!(m["moved"], json!(true), "{m}");
        assert_eq!(m["readout"]["status"], "tuned", "{m}");
        let d = &m["readout"]["delay"];
        assert!((d["current"].as_f64().unwrap() - 150.0).abs() < 0.2, "{m}");
        assert_eq!((d["min"].as_f64().map(|v| (v * 10.0).round()), d["max"].as_f64().map(|v| (v * 10.0).round())), (Some(1499.0), Some(1501.0)), "0.1 ps either side of the target: {m}");
        let f = fix(&dir, &body(json!({ "x": 22_000_000, "y": 9_000_000 })));
        assert_eq!(f["ok"], json!(true), "{f}");
        let after = design_of(&dir);
        let patterns = tune_edit::patterns_of(&after);
        assert_eq!(patterns.len(), 1);
        let pat = &patterns[0];
        assert!(pat.is_time_domain && pat.target_delay.opt == 150_000_000, "{pat:?}");
        assert!((pat.last_tuning_delay as f64 / 1e6 - 150.0).abs() < 0.2, "{}", pat.last_tuning_delay);
        assert_eq!(pat.tuning_info().split(' ').next_back(), Some("(tuned)"));
        assert!(pat.tuning_info().contains(" ps "), "{}", pat.tuning_info());
        assert!((net_ps(&dir, "/SIG") - 150.0).abs() < 0.2, "{}", net_ps(&dir, "/SIG"));
        // the other net has the same profile, and is untouched
        assert!((net_ps(&dir, "/OTHER") - 122.0).abs() < 0.01);

        board::undo(&dir, "test", None).expect("one undo takes the whole pattern back");
        assert!((net_ps(&dir, "/SIG") - 122.0).abs() < 0.01);
    }

    #[test]
    fn a_profile_without_the_time_domain_leaves_a_pattern_in_length_and_the_dialog_can_ask_for_the_other() {
        let dir = scratch("delay_off");
        setup(&dir, ["SIG", "OTHER"], vec![]);
        with_profile(&dir, false);
        let h = hover(&dir, &body(json!({ "x": 15_000_000, "y": 9_550_000, "tolerance": 100_000, "layer": "F.Cu", "mode": "single" })));
        assert_eq!(h["readout"]["time_domain"], json!(false), "{h}");
        // the dialog's Delay radio makes it a time domain pattern all the same (every delay is zero without the profile's time domain: the target length is the net's own)
        let s = start(&dir, &body(json!({ "x": 8_000_000, "y": 9_500_000, "layer": "F.Cu", "mode": "single", "settings": { "time_domain": true, "target_delay": 150_000 } })));
        assert_eq!(s["ok"], json!(true), "{s}");
        let m = mv(&dir, &body(json!({ "x": 22_000_000, "y": 9_000_000 })));
        assert_eq!(m["readout"]["time_domain"], json!(true));
        assert!(m["readout"]["current"].as_f64().unwrap() - 20.0 < 0.01, "the net was not lengthened: {m}");
    }

    #[test]
    fn a_pattern_is_tuned_again_and_removed_and_each_is_one_undo_step() {
        let dir = scratch("retune");
        setup(&dir, ["SIG", "OTHER"], vec![]);
        start(&dir, &body(json!({ "x": 8_000_000, "y": 9_500_000, "layer": "F.Cu", "mode": "single", "settings": { "target_length": 22_000_000 } })));
        mv(&dir, &body(json!({ "x": 22_000_000, "y": 9_000_000 })));
        let f = fix(&dir, &body(json!({})));
        assert_eq!(f["ok"], json!(true), "{f}");
        let id = f["pattern_id"].as_str().unwrap().to_string();
        let tuned_once = design_of(&dir);

        // the pattern is listed with its tuning info, and its properties carry the target a rule would give
        let list = patterns(&dir);
        assert_eq!(list["patterns"].as_array().unwrap().len(), 1);
        assert_eq!(list["patterns"][0]["nets"], json!(["/SIG"]));
        assert!(list["patterns"][0]["info"].as_str().unwrap().ends_with("(tuned)"), "{list}");
        let props = pattern(&dir, &body(json!({ "id": id })));
        assert_eq!(props["ok"], json!(true), "{props}");
        assert!(props["constraint"].is_null());

        // tuned again, longer: the same pattern, the same id, one step
        let u = update(&dir, &body(json!({ "id": id, "settings": { "target_length": 24_000_000 } })));
        assert_eq!(u["ok"], json!(true), "{u}");
        let after = design_of(&dir);
        let patterns_after = tune_edit::patterns_of(&after);
        assert_eq!(patterns_after.len(), 1);
        assert_eq!(patterns_after[0].id, id, "a pattern tuned again keeps its id");
        assert_eq!(patterns_after[0].target_length.opt, 24_000_000);
        assert!((net_mm(&dir, "/SIG") - 24.0).abs() < 0.1, "{}", net_mm(&dir, "/SIG"));
        board::undo(&dir, "test", None).unwrap();
        assert_eq!(tracks_of(&design_of(&dir)), tracks_of(&tuned_once), "one undo puts the first tuning back");
        assert_eq!(tune_edit::patterns_of(&design_of(&dir)), tune_edit::patterns_of(&tuned_once));

        // regenerated: tuned again as it is
        let g = regenerate(&dir, &body(json!({})));
        assert_eq!(g["ok"], json!(true), "{g}");
        assert_eq!(g["regenerated"], json!(1));
        assert!((net_mm(&dir, "/SIG") - 22.0).abs() < 0.1);
        board::undo(&dir, "test", None).unwrap();

        // removed: the track is straight again and the pattern is gone
        let r = remove(&dir, &body(json!({ "id": id })));
        assert_eq!(r["ok"], json!(true), "{r}");
        let gone = design_of(&dir);
        assert!(tune_edit::patterns_of(&gone).is_empty());
        assert!((net_mm(&dir, "/SIG") - 20.0).abs() < 0.001, "{}", net_mm(&dir, "/SIG"));
        let tracks: Vec<&eda_model::ir::Track> = gone.routing.as_ref().unwrap().tracks.iter().filter(|t| t.net == "/SIG").collect();
        assert!(tracks.iter().all(|t| t.pts.iter().all(|q| q.y == mm(9.5))), "all straight again: {tracks:?}");
        board::undo(&dir, "test", None).unwrap();
        assert_eq!(tune_edit::patterns_of(&design_of(&dir)).len(), 1, "one undo brings the pattern back");
    }

    #[test]
    fn a_pattern_is_edited_through_its_edit_points_and_the_edit_is_one_undo_step() {
        let dir = scratch("edit");
        setup(&dir, ["SIG", "OTHER"], vec![]);
        start(&dir, &body(json!({ "x": 8_000_000, "y": 9_500_000, "layer": "F.Cu", "mode": "single", "settings": { "target_length": 22_000_000 } })));
        mv(&dir, &body(json!({ "x": 22_000_000, "y": 9_000_000 })));
        let id = fix(&dir, &body(json!({})))["pattern_id"].as_str().unwrap().to_string();
        let tuned = design_of(&dir);

        // genStartEdit: the session shows the pattern as it fits now, with its four edit points, and the board is untouched
        let e = edit_start(&dir, &body(json!({ "id": id })));
        assert_eq!(e["ok"], json!(true), "{e}");
        assert_eq!(e["editing"], json!(true));
        assert_eq!(e["handles"].as_array().unwrap().len(), 4);
        assert_eq!(e["handles"][0], json!([8_000_000, 9_500_000]));
        assert_eq!(e["handles"][1], json!([22_000_000, 9_500_000]));
        assert_eq!(tracks_of(&design_of(&dir)), tracks_of(&tuned));
        assert!((e["readout"]["current"].as_f64().unwrap() - 22.0).abs() < 0.1);

        // genUpdateEdit: the end handle dragged back to 18 mm, so the pattern is shorter: the same target, less room
        let m = edit_move(&dir, &body(json!({ "handle": 1, "x": 18_000_000, "y": 9_500_000 })));
        assert_eq!(m["ok"], json!(true), "{m}");
        assert_eq!(m["pattern"]["end"], json!({ "x": 18_000_000, "y": 9_500_000 }), "{m}");
        assert_eq!(m["handles"][1], json!([18_000_000, 9_500_000]));
        // a settings change in the middle of the edit
        let m = edit_move(&dir, &body(json!({ "settings": { "target_length": 21_000_000 } })));
        assert_eq!(m["ok"], json!(true), "{m}");
        assert!((m["readout"]["current"].as_f64().unwrap() - 21.0).abs() < 0.1, "{m}");
        // the amplitude handle: out to 0.8 mm (and the track's half width) on the other side
        let m = edit_move(&dir, &body(json!({ "handle": 2, "x": 8_000_000, "y": 9_500_000 + 925_000 })));
        assert_eq!((m["pattern"]["max_amplitude"].clone(), m["pattern"]["initial_side"].clone()), (json!(800_000), json!("right")), "{m}");

        // genFinishEdit
        let f = fix(&dir, &body(json!({})));
        assert_eq!(f["ok"], json!(true), "{f}");
        assert_eq!(f["pattern_id"], json!(id));
        let after = tune_edit::patterns_of(&design_of(&dir));
        assert_eq!(after.len(), 1);
        assert_eq!((after[0].id.as_str(), after[0].end, after[0].max_amplitude, after[0].target_length.opt), (id.as_str(), p(18_000_000, 9_500_000), mm(0.8), 21_000_000));
        assert!((net_mm(&dir, "/SIG") - 21.0).abs() < 0.1);
        board::undo(&dir, "test", None).unwrap();
        assert_eq!(tracks_of(&design_of(&dir)), tracks_of(&tuned), "the whole edit is one undo step");

        // genCacnelEdit: nothing is written
        edit_start(&dir, &body(json!({ "id": id })));
        edit_move(&dir, &body(json!({ "handle": 1, "x": 15_000_000, "y": 9_500_000 })));
        assert_eq!(cancel(&dir)["had_session"], json!(true));
        assert_eq!(tracks_of(&design_of(&dir)), tracks_of(&tuned));
        assert_eq!(edit_move(&dir, &body(json!({ "handle": 1, "x": 15_000_000, "y": 9_500_000 })))["ok"], json!(false), "the session is over");
    }

    #[test]
    fn a_differential_pair_is_tuned_together_and_removed_together() {
        let dir = scratch("pair");
        setup(&dir, ["USB_P", "USB_N"], vec![]);
        let s = start(&dir, &body(json!({ "x": 8_000_000, "y": 9_500_000, "layer": "F.Cu", "mode": "diffpair", "settings": { "target_length": 26_000_000 } })));
        assert_eq!(s["ok"], json!(true), "{s}");
        let m = mv(&dir, &body(json!({ "x": 22_000_000, "y": 9_000_000 })));
        assert_eq!(m["moved"], json!(true), "{m}");
        assert_eq!(m["readout"]["partner"], "/USB_N");
        assert_eq!(m["preview"]["hide"].as_array().unwrap().len(), 2, "both lines are replaced");
        let f = fix(&dir, &body(json!({})));
        assert_eq!(f["ok"], json!(true), "{f}");
        let (p_len, n_len) = (net_mm(&dir, "/USB_P"), net_mm(&dir, "/USB_N"));
        assert_eq!(m["readout"]["status"], "tuned", "{m}");
        assert!((p_len.max(n_len) - 26.0).abs() < 0.05, "P {p_len} N {n_len}");
        assert!((p_len - n_len).abs() < 0.7, "tuned together: P {p_len} N {n_len}");
        let pat = tune_edit::patterns_of(&design_of(&dir)).remove(0);
        assert_eq!(pat.mode, TuningMode::DiffPair);
        assert_eq!((pat.base_line.len() >= 2, pat.base_line_coupled.len() >= 2), (true, true), "both baselines are kept");
        let nets = nets_of(&design_of(&dir), &pat);
        assert_eq!(nets.len(), 2, "the members are tracks of both nets: {nets:?}");

        let r = remove(&dir, &body(json!({ "id": pat.id })));
        assert_eq!(r["ok"], json!(true), "{r}");
        assert!((net_mm(&dir, "/USB_P") - 20.0).abs() < 0.001 && (net_mm(&dir, "/USB_N") - 20.0).abs() < 0.001);
    }

    #[test]
    fn the_skew_tuner_lengthens_the_clicked_line_until_it_matches_its_partner() {
        let dir = scratch("skew");
        // USB_N takes a 0.7 mm detour on the way: 1.4 mm longer than USB_P, which runs straight
        let n_path = vec![p(5_000_000, 10_500_000), p(10_000_000, 10_500_000), p(10_000_000, 11_200_000), p(12_000_000, 11_200_000), p(12_000_000, 10_500_000), p(25_000_000, 10_500_000)];
        setup_with(&dir, ["USB_P", "USB_N"], vec![], vec![("/USB_P", vec![p(5_000_000, 9_500_000), p(25_000_000, 9_500_000)]), ("/USB_N", n_path)]);
        let (p_before, n_before) = (net_mm(&dir, "/USB_P"), net_mm(&dir, "/USB_N"));
        assert!((n_before - p_before - 1.4).abs() < 0.01, "P {p_before} N {n_before}");
        let h = hover(&dir, &body(json!({ "x": 15_000_000, "y": 9_500_000, "layer": "F.Cu", "mode": "skew" })));
        assert_eq!(h["readout"]["label"], "current skew", "{h}");
        assert!((h["readout"]["current"].as_f64().unwrap() + 1.4).abs() < 0.01, "P is 1.4 mm short of N: {h}");
        assert_eq!(h["readout"]["status"], "too_short");
        // the skew target is zero, 0.1 mm either side
        assert_eq!((h["readout"]["min"].as_f64(), h["readout"]["opt"].as_f64(), h["readout"]["max"].as_f64()), (Some(-0.1), Some(0.0), Some(0.1)));
        let s = start(&dir, &body(json!({ "x": 8_000_000, "y": 9_500_000, "layer": "F.Cu", "mode": "skew" })));
        assert_eq!(s["ok"], json!(true), "{s}");
        let m = mv(&dir, &body(json!({ "x": 22_000_000, "y": 9_000_000 })));
        assert_eq!(m["readout"]["status"], "tuned", "{m}");
        assert!(m["readout"]["current"].as_f64().unwrap().abs() < 0.1, "{m}");
        // findings p5 F4-3: the status box's range is the skew's, around 0 (`statusItem->SetMinMax( m_targetSkew.Min(), Max() )`), not the partner's length
        assert_eq!((m["readout"]["min"].as_f64(), m["readout"]["max"].as_f64()), (Some(-0.1), Some(0.1)), "{m}");
        let f = fix(&dir, &body(json!({})));
        assert_eq!(f["ok"], json!(true), "{f}");
        let (p_after, n_after) = (net_mm(&dir, "/USB_P"), net_mm(&dir, "/USB_N"));
        assert!((p_after - n_after).abs() < 0.1, "P {p_after} N {n_after}");
        assert!((n_after - n_before).abs() < 0.001, "only the clicked line is lengthened");
        let pat = tune_edit::patterns_of(&design_of(&dir)).remove(0);
        assert_eq!(pat.mode, TuningMode::DiffPairSkew);
        assert!(pat.member_ids.iter().all(|m| design_of(&dir).routing.unwrap().tracks.iter().any(|t| &t.id == m && t.net == "/USB_P")), "the members are USB_P's");
        // a skew pattern puts back only its own line
        let r = remove(&dir, &body(json!({ "id": pat.id })));
        assert_eq!(r["ok"], json!(true), "{r}");
        assert!((net_mm(&dir, "/USB_P") - p_before).abs() < 0.001);
    }

    #[test]
    fn a_line_of_several_tracks_is_replaced_where_the_pattern_is_and_no_further() {
        let dir = scratch("several");
        // one net, three tracks end to end: 5-10, 10-20 (a polyline with a vertex at 15) and 20-25 mm
        setup_with(
            &dir,
            ["SIG", "OTHER"],
            vec![],
            vec![("/SIG", vec![p(5_000_000, 9_500_000), p(10_000_000, 9_500_000)]), ("/SIG", vec![p(10_000_000, 9_500_000), p(15_000_000, 9_500_000), p(20_000_000, 9_500_000)]), ("/SIG", vec![p(20_000_000, 9_500_000), p(25_000_000, 9_500_000)]), ("/OTHER", vec![p(5_000_000, 10_500_000), p(25_000_000, 10_500_000)])],
        );
        let before = design_of(&dir);
        let ids_before: Vec<String> = tracks_of(&before).iter().filter(|t| t.net == "/SIG").map(|t| t.id.clone()).collect();
        assert_eq!(ids_before.len(), 3);
        start(&dir, &body(json!({ "x": 12_000_000, "y": 9_500_000, "layer": "F.Cu", "mode": "single", "settings": { "target_length": 22_000_000 } })));
        let m = mv(&dir, &body(json!({ "x": 18_000_000, "y": 9_000_000 })));
        assert_eq!(m["readout"]["status"], "tuned", "{m}");
        let f = fix(&dir, &body(json!({})));
        assert_eq!(f["ok"], json!(true), "{f}");
        let after = design_of(&dir);
        let sig_ids: Vec<String> = tracks_of(&after).iter().filter(|t| t.net == "/SIG").map(|t| t.id.clone()).collect();
        // the first and the last track were not touched: they are the very same tracks
        assert!(sig_ids.contains(&ids_before[0]) && sig_ids.contains(&ids_before[2]), "{sig_ids:?} {ids_before:?}");
        assert!(!sig_ids.contains(&ids_before[1]), "the middle track is replaced");
        assert!((net_mm(&dir, "/SIG") - 22.0).abs() < 0.1);
        let pat = tune_edit::patterns_of(&after).remove(0);
        assert!(pat.member_ids.iter().all(|m| !ids_before.contains(m)));
        board::undo(&dir, "test", None).unwrap();
        assert_eq!(tracks_of(&design_of(&dir)), tracks_of(&before));
    }

    /// A track that is an arc, put on the board as KiCad's `PCB_ARC` (`Track::new_arc`).
    fn add_arc_track(dir: &Path, net: &str, start: Point, mid: Point, end: Point) {
        let (_, mut design, _) = board::load(dir).unwrap();
        let mut t = eda_model::ir::Track::new_arc(net.into(), "F.Cu".into(), mm(0.25), start, mid, end);
        t.id = format!("trk_arc_{net}");
        design.routing.as_mut().expect("the board has tracks").tracks.push(t);
        board::save(dir, &design).unwrap();
    }

    #[test]
    fn a_track_that_is_an_arc_can_be_picked_and_a_pattern_is_tuned_along_it() {
        let dir = scratch("arc");
        // SIG: 7 mm straight, a quarter arc of radius 3 mm turning up, 5 mm straight on; the arc is a track of its own
        let h = (3_000_000.0 * std::f64::consts::FRAC_1_SQRT_2).round() as i64;
        let (arc_start, arc_mid, arc_end) = (p(12_000_000, 9_500_000), p(12_000_000 + h, 6_500_000 + h), p(15_000_000, 6_500_000));
        setup_with(&dir, ["SIG", "OTHER"], vec![], vec![("/SIG", vec![p(5_000_000, 9_500_000), p(12_000_000, 9_500_000)]), ("/SIG", vec![p(15_000_000, 6_500_000), p(15_000_000, 1_500_000)]), ("/OTHER", vec![p(5_000_000, 10_500_000), p(25_000_000, 10_500_000)])]);
        add_arc_track(&dir, "/SIG", arc_start, arc_mid, arc_end);
        let before = design_of(&dir);
        let original = net_mm(&dir, "/SIG");
        let arc_len = 3.0 * std::f64::consts::FRAC_PI_2;
        assert!((original - (7.0 + arc_len + 5.0)).abs() < 0.01, "the arc is measured as an arc: {original}");

        // hovering the arc picks it (its track), the pattern would start at the end of it the cursor is nearer to
        let hov = hover(&dir, &body(json!({ "x": arc_mid.x, "y": arc_mid.y, "tolerance": 100_000, "layer": "F.Cu", "mode": "single" })));
        assert_eq!(hov["hit"], json!(true), "{hov}");
        assert_eq!(hov["pick"]["net"], "/SIG");
        assert!((hov["readout"]["current"].as_f64().unwrap() - original).abs() < 0.01, "{hov}");

        // a pattern from the first straight, through the arc, along the second straight
        let s = start(&dir, &body(json!({ "x": 6_000_000, "y": 9_500_000, "layer": "F.Cu", "mode": "single", "settings": { "target_length": (original * 1e6).round() as i64 + 3_000_000 } })));
        assert_eq!(s["ok"], json!(true), "{s}");
        let m = mv(&dir, &body(json!({ "x": 15_000_000, "y": 3_500_000 })));
        assert_eq!(m["ok"], json!(true), "{m}");
        assert_eq!(m["readout"]["status"], "tuned", "{m}");
        let f = fix(&dir, &body(json!({})));
        assert_eq!(f["ok"], json!(true), "{f}");
        let after = design_of(&dir);
        let rt = after.routing.as_ref().unwrap();
        // the arc track is replaced by the very same arc: the pattern passes through it as it is
        let arcs: Vec<_> = rt.tracks.iter().filter(|t| t.net == "/SIG").filter_map(|t| t.arc()).collect();
        assert!(arcs.contains(&(arc_start, arc_mid, arc_end)), "the arc is written back as the arc it was: {arcs:?}");
        assert!((net_mm(&dir, "/SIG") - (original + 3.0)).abs() < 0.1, "{} vs {}", net_mm(&dir, "/SIG"), original + 3.0);
        board::undo(&dir, "test", None).unwrap();
        assert_eq!(tracks_of(&design_of(&dir)), tracks_of(&before));
    }

    #[test]
    fn a_track_that_cannot_be_tuned_is_refused_with_kicads_message() {
        let dir = scratch("refuse");
        setup(&dir, ["SIG", "OTHER"], vec![]);
        // a pair tuner on a net that is not half of a pair
        let s = start(&dir, &body(json!({ "x": 8_000_000, "y": 9_500_000, "layer": "F.Cu", "mode": "diffpair" })));
        assert_eq!(s["ok"], json!(false));
        assert!(s["message"].as_str().unwrap().contains("complementary differential pair"), "{s}");
        let s = start(&dir, &body(json!({ "x": 8_000_000, "y": 1_000_000, "layer": "F.Cu", "mode": "single" })));
        assert_eq!(s["message"], "Select a track to tune first.");
        assert_eq!(mv(&dir, &body(json!({ "x": 1_000, "y": 2000 })))["message"], NO_SESSION);
        assert_eq!(cancel(&dir)["had_session"], json!(false));
    }

    #[test]
    fn a_session_on_a_board_that_changed_is_refused() {
        let dir = scratch("stale");
        setup(&dir, ["SIG", "OTHER"], vec![]);
        start(&dir, &body(json!({ "x": 8_000_000, "y": 9_500_000, "layer": "F.Cu", "mode": "single", "settings": { "target_length": 22_000_000 } })));
        mv(&dir, &body(json!({ "x": 22_000_000, "y": 9_000_000 })));
        board::step(&dir, Cmd::AddTrack { net: "/OTHER".into(), layer: "F.Cu".into(), width: mm(0.25), pts: vec![p(2_000_000, 3_000_000), p(4_000_000, 3_000_000)] }, false, "test").unwrap();
        let f = fix(&dir, &body(json!({})));
        assert_eq!(f["ok"], json!(false));
        assert!(f["message"].as_str().unwrap().contains("changed"), "{f}");
    }

    /// kicad-cli is the judge of the lengths: a board with a tuned track and a tuned pair loads (the `(generated ..)` items are read), the patterns survive the file, and its
    /// DRC -- `length` and `skew` rules over the nets -- finds nothing wrong with the nets that were tuned to the target the rule asks for, and does find what is wrong with
    /// the same nets before they were tuned (the control: the rule is evaluated). Slow tier: it runs kicad-cli several times.
    #[test]
    fn kicad_cli_finds_the_tuned_nets_within_the_length_and_skew_rules_they_were_tuned_to() {
        if std::env::var_os("EDA_SLOW_TESTS").is_none() {
            eprintln!("skipped (set EDA_SLOW_TESTS=1): runs kicad-cli");
            return;
        }
        if eda_kicad_engine::find_cli().is_none() {
            eprintln!("kicad-cli not found: skipping");
            return;
        }
        let judge = |dir: &Path, rules: &str, kind: &str| -> Vec<String> {
            let (_, design, mut model) = board::load(dir).unwrap();
            model.board.custom_rules_text = Some(format!("(version 1)\n{rules}\n"));
            let report = eda_kicad_engine::drc_scratch(&design, &model).expect("kicad-cli runs the DRC (it loads the board, `(generated ..)` items included)");
            report.violations.iter().filter(|v| v.kind == kind).map(|v| v.description.clone()).collect()
        };

        // a track tuned to 22 mm
        let one = scratch("kicad_single");
        setup(&one, ["SIG", "OTHER"], vec![]);
        let rule = |name: &str, nets: &str, constraint: &str| format!("(rule \"{name}\" (condition \"{nets}\") (constraint {constraint}))");
        let sig = |c: &str| rule("sig", "A.NetName == '/SIG'", c);
        let untuned = judge(&one, &sig("length (min 21.9mm) (max 22.1mm)"), "length_out_of_range");
        assert_eq!(untuned.len(), 1, "the control: the straight 20 mm track is out of the range: {untuned:?}");
        start(&one, &body(json!({ "x": 8_000_000, "y": 9_500_000, "layer": "F.Cu", "mode": "single", "settings": { "target_length": 22_000_000 } })));
        mv(&one, &body(json!({ "x": 22_000_000, "y": 9_000_000 })));
        assert_eq!(fix(&one, &body(json!({})))["ok"], json!(true));
        let tuned = judge(&one, &sig("length (min 21.9mm) (max 22.1mm)"), "length_out_of_range");
        assert!(tuned.is_empty(), "kicad-cli measures the tuned track within 21.9..22.1 mm: {tuned:?}");
        let tighter = judge(&one, &sig("length (min 21.99mm) (max 22.01mm)"), "length_out_of_range");
        assert!(tighter.is_empty(), "and within 10 um of the target: {tighter:?}");

        // the patterns survive the file: written as `(generated ..)`, read back with their tracks
        let (_, design, model) = board::load(&one).unwrap();
        let text = eda_kicad::export_kicad_pcb(&design, &model, &eda_kicad::ExportMeta { date: "2026-01-01", title: "t" }).unwrap();
        assert_eq!(text.matches("\t(generated\n").count(), 1, "one generator is written");
        let (back, _, _) = eda_kicad::import_kicad_pcb(&text).unwrap();
        let (a, b) = (&design.drawings.as_ref().unwrap().tuning_patterns[0], &back.drawings.as_ref().unwrap().tuning_patterns[0]);
        // a polyline of the meander is several segments in the file, so the members come back as that many tracks: as many as the file names
        let written = text.lines().find(|l| l.trim_start().starts_with("(members")).map_or(0, |l| l.matches('"').count() / 2);
        assert!(written >= a.member_ids.len(), "{written} items of the file for {} tracks", a.member_ids.len());
        assert_eq!((b.member_ids.len(), b.origin, b.end, b.mode, &b.layer), (written, a.origin, a.end, a.mode, &a.layer), "the pattern and its members come back");
        assert!((a.target_length.opt - b.target_length.opt).abs() <= 1 && b.max_amplitude == a.max_amplitude && b.spacing == a.spacing);

        // a pair tuned to 26 mm: both nets, and the skew between them
        let pair = scratch("kicad_pair");
        setup(&pair, ["USB_P", "USB_N"], vec![]);
        let usb = |c: &str| rule("usb", "A.NetName == '/USB_P' || A.NetName == '/USB_N'", c);
        start(&pair, &body(json!({ "x": 8_000_000, "y": 9_500_000, "layer": "F.Cu", "mode": "diffpair", "settings": { "target_length": 26_000_000 } })));
        mv(&pair, &body(json!({ "x": 22_000_000, "y": 9_000_000 })));
        assert_eq!(fix(&pair, &body(json!({})))["ok"], json!(true));
        let len = judge(&pair, &usb("length (min 25.9mm) (max 26.1mm)"), "length_out_of_range");
        assert!(len.is_empty(), "both nets of the pair are 26 mm to kicad-cli: {len:?}");
        let skew = judge(&pair, &usb("skew (max 0.3mm) (within_diff_pairs)"), "skew_out_of_range");
        assert!(skew.is_empty(), "and the skew between them is within 0.3 mm: {skew:?}");

        // the skew tuner: USB_N takes a 0.7 mm detour (1.4 mm longer than USB_P), the skew is tuned away
        let sk = scratch("kicad_skew");
        let n_path = vec![p(5_000_000, 10_500_000), p(10_000_000, 10_500_000), p(10_000_000, 11_200_000), p(12_000_000, 11_200_000), p(12_000_000, 10_500_000), p(25_000_000, 10_500_000)];
        setup_with(&sk, ["USB_P", "USB_N"], vec![], vec![("/USB_P", vec![p(5_000_000, 9_500_000), p(25_000_000, 9_500_000)]), ("/USB_N", n_path)]);
        let rule_skew = usb("skew (max 0.15mm) (within_diff_pairs)");
        assert_eq!(judge(&sk, &rule_skew, "skew_out_of_range").len(), 1, "the control: 1.4 mm of skew is flagged, on the shorter net (the longest has none against itself)");
        start(&sk, &body(json!({ "x": 8_000_000, "y": 9_500_000, "layer": "F.Cu", "mode": "skew" })));
        mv(&sk, &body(json!({ "x": 22_000_000, "y": 9_000_000 })));
        assert_eq!(fix(&sk, &body(json!({})))["ok"], json!(true));
        let after = judge(&sk, &rule_skew, "skew_out_of_range");
        assert!(after.is_empty(), "after the skew tuner kicad-cli finds the pair within 0.15 mm: {after:?}");
    }

    /// The time domain against kicad-cli: the Default class names a tuning profile (6.1 ps/mm on F.Cu, time domain on), a `length` rule asks for 150 ps, and the
    /// DRC measures the delay of the nets itself -- the 20 mm track (122 ps) is out of range, the track tuned to 150 ps in picoseconds is within it.
    #[test]
    fn kicad_cli_measures_the_delay_of_a_track_tuned_in_picoseconds_through_its_tuning_profile() {
        if std::env::var_os("EDA_SLOW_TESTS").is_none() {
            eprintln!("skipped (set EDA_SLOW_TESTS=1): runs kicad-cli");
            return;
        }
        if eda_kicad_engine::find_cli().is_none() {
            eprintln!("kicad-cli not found: skipping");
            return;
        }
        let judge = |dir: &Path, rules: &str| -> Vec<String> {
            let (_, design, mut model) = board::load(dir).unwrap();
            model.board.custom_rules_text = Some(format!("(version 1)\n{rules}\n"));
            let report = eda_kicad_engine::drc_scratch(&design, &model).expect("kicad-cli runs the DRC");
            report.violations.iter().filter(|v| v.kind == "length_out_of_range").map(|v| v.description.clone()).collect()
        };
        let dir = scratch("kicad_delay");
        setup(&dir, ["SIG", "OTHER"], vec![]);
        with_profile(&dir, true);
        let rule = |range: &str| format!("(rule \"sig\" (condition \"A.NetName == '/SIG'\") (constraint length {range}))");
        let untuned = judge(&dir, &rule("(min 149.5ps) (max 150.5ps)"));
        assert_eq!(untuned.len(), 1, "the control: the straight 20 mm track takes 122 ps, out of 149.5..150.5: {untuned:?}");
        assert!(untuned[0].contains("ps"), "kicad-cli reports a time: {untuned:?}");
        start(&dir, &body(json!({ "x": 8_000_000, "y": 9_500_000, "layer": "F.Cu", "mode": "single", "settings": { "target_delay": 150 } })));
        mv(&dir, &body(json!({ "x": 22_000_000, "y": 9_000_000 })));
        assert_eq!(fix(&dir, &body(json!({})))["ok"], json!(true));
        let tuned = judge(&dir, &rule("(min 149.5ps) (max 150.5ps)"));
        assert!(tuned.is_empty(), "kicad-cli measures the tuned track within 149.5..150.5 ps: {tuned:?}");
        let tighter = judge(&dir, &rule("(min 149.9ps) (max 150.1ps)"));
        assert!(tighter.is_empty(), "and within 0.1 ps of the target: {tighter:?}");
    }
}
