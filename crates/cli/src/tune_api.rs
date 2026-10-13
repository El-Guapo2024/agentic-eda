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

use crate::board;
use crate::tune_edit::{self, Moved, Pick, Scene, Tuner};
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
    Some(Point { x: p.get("x")?.as_f64()?.round() as i64, y: p.get("y")?.as_f64()?.round() as i64 })
}

fn xy(v: &Value) -> Option<Point> {
    Some(Point { x: v.get("x")?.as_f64()?.round() as i64, y: v.get("y")?.as_f64()?.round() as i64 })
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

/// The cursor's pick for a request: `{x, y, tolerance, layer}`.
fn pick_for(scene: &Scene, req: &Value) -> Option<Pick> {
    let at = xy(req)?;
    let tolerance = req.get("tolerance").and_then(Value::as_f64).unwrap_or(150.0).round() as i64;
    tune_edit::pick(scene, at.x, at.y, tolerance, req.get("layer").and_then(Value::as_str))
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
    let mut reply = json!({ "ok": true, "mode": mode_name(t.pattern.mode), "pattern": pattern_json(&t.pattern), "net": t.net });
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
    let kind = mode_of(&req);
    // the pattern starts at the point of the track nearest the click (`snapToNearestTrack`, the picked item's focus position)
    let tuner = match Tuner::start(scene.clone(), kind, &pk, pk.at, &settings_of(&req)) {
        Ok(t) => t,
        Err(m) => return err(m),
    };
    let stamp = stamp_of(dir);
    let session = Session { tuner, stamp };
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
    match s.tuner.last_end() {
        Some(end) => match s.tuner.mv(end, true) {
            Ok(_) => session_reply(s, s.tuner.last()),
            Err(m) => err(m),
        },
        None => session_reply(s, None),
    }
}

/// `POST /api/tune/fix` `{x?, y?}`: the second click. Puts the pattern on the board (`Cmd::CommitTuning`, one undo step).
pub fn fix(dir: &Path, body: &[u8]) -> Value {
    let req = body_json(body);
    let session = SESSIONS.with(|s| s.borrow_mut().remove(dir));
    let Some(mut session) = session else { return err(NO_SESSION) };
    if stamp_of(dir) != session.stamp {
        return err("The board changed while the pattern was being placed: start it again.");
    }
    if let Some(to) = xy(&req) {
        if session.tuner.last_end() != Some(to) {
            if let Err(m) = session.tuner.mv(to, true) {
                return err(m);
            }
        }
    }
    let cmd = match session.tuner.command() {
        Ok(c) => c,
        Err(m) => {
            // nothing to commit yet: the session goes on
            SESSIONS.with(|s| s.borrow_mut().insert(dir.to_path_buf(), session));
            return err(m);
        }
    };
    let readout = session.tuner.last().map(|m| m.readout.json());
    let p = session.tuner.pattern.clone();
    match board::step(dir, cmd, true, "ui") {
        Ok(summary) => {
            let id = board::load(dir).ok().and_then(|(_, d, _)| tune_edit::patterns_of(&d).into_iter().find(|q| q.mode == p.mode && q.layer == p.layer && q.origin == p.origin).map(|q| q.id));
            json!({ "ok": true, "message": summary, "pattern_id": id, "readout": readout })
        }
        Err(e) => err(board::reasons(&e)),
    }
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
                "members": p.member_ids, "info": p.tuning_info(), "status": p.last_status, "length": p.last_tuning_length as f64 / 1e6,
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
        Point { x, y }
    }

    /// Two parts 20 mm apart, each with a pad above a pad (1 mm pitch): nets `<a>` and `<b>` run from one to the other as 20 mm tracks. `rules` are custom rules.
    fn setup(dir: &Path, nets: [&str; 2], rules: Vec<eda_model::CustomRule>) {
        let straight = |y: i64| vec![p(5_000, y), p(25_000, y)];
        setup_with(dir, nets, rules, vec![(nets[0], straight(9_500)), (nets[1], straight(10_500))]);
    }

    /// [`setup`] with the tracks the caller draws (each its own track).
    fn setup_with(dir: &Path, nets: [&str; 2], rules: Vec<eda_model::CustomRule>, tracks: Vec<(&str, Vec<Point>)>) {
        let pad = |n: &str, y: i64| Pad { opposite_side: false, ext: Default::default(), number: n.into(), at: (0, y), size: (400, 400), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None };
        let fp = Footprint { name: "PAIR2".into(), pads: vec![pad("1", -500), pad("2", 500)], courtyard: Some((1000, 2000)), model: None, courtyard_outlines: vec![], models3d: vec![], art: Default::default() };
        let part = |r: &str| Part {
            reference: r.into(),
            mpn: None,
            lcsc: None,
            value: None,
            package: Some("PAIR2".into()),
            footprint: Some("PAIR2".into()),
            pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }],
            body_um: None,
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
            schema: 1,
            provenance: Provenance { engine_version: "t".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection {
                outline: vec![p(0, 0), p(40_000, 0), p(40_000, 20_000), p(0, 20_000)],
                footprints: vec![
                    FootprintInstance { id: "U1".into(), at: p(5_000, 10_000), rot: 0, side: Side::Top, label: Default::default() },
                    FootprintInstance { id: "U2".into(), at: p(25_000, 10_000), rot: 0, side: Side::Top, label: Default::default() },
                ],
                modules: vec![],
            }),
            routing: None,
            drawings: None,
        };
        board::save(dir, &design).unwrap();
        let meta = board::Meta { intent: intent_path.display().to_string(), snap_um: 100, spacing_um: 300 };
        std::fs::write(dir.join("board.json"), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
        for (net, pts) in tracks {
            let cmd = Cmd::AddTrack { net: net.into(), layer: "F.Cu".into(), width: 250, pts };
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
        let rule = eda_model::CustomRule { name: "sig_length".into(), constraint_type: "length".into(), min: Some(21_900), max: Some(22_100), opt: None, layer: None, severity: None, condition: None };
        setup(&dir, ["SIG", "OTHER"], vec![rule]);
        let before = std::fs::read_to_string(dir.join("design.json")).unwrap();
        let r = hover(&dir, &body(json!({ "x": 15_000, "y": 9_550, "tolerance": 100, "layer": "F.Cu", "mode": "single" })));
        assert_eq!(r["ok"], json!(true), "{r}");
        assert_eq!(r["hit"], json!(true), "{r}");
        assert_eq!(r["pick"]["net"], "SIG");
        assert_eq!(r["readout"]["label"], "current length");
        assert!((r["readout"]["current"].as_f64().unwrap() - 20.0).abs() < 0.01, "{r}");
        // the rule's range, `Opt()` being its minimum
        assert_eq!((r["readout"]["min"].as_f64(), r["readout"]["max"].as_f64(), r["readout"]["opt"].as_f64()), (Some(21.9), Some(22.1), Some(21.9)));
        assert_eq!(r["readout"]["status"], "too_short");
        assert_eq!(r["readout"]["constraint"], "sig_length");
        assert!((r["readout"]["delta"].as_f64().unwrap() + 1.9).abs() < 0.01);
        assert_eq!(r["path"].as_array().unwrap().len(), 2);
        // a pattern that overrides the rules has its own target
        let own = hover(&dir, &body(json!({ "x": 15_000, "y": 9_550, "layer": "F.Cu", "mode": "single", "settings": { "override_custom_rules": true, "target_length": 21_000 } })));
        assert_eq!((own["readout"]["opt"].as_f64(), own["readout"]["constraint"].is_null()), (Some(21.0), true), "{own}");
        // nothing under the cursor
        assert_eq!(hover(&dir, &body(json!({ "x": 15_000, "y": 5_000, "layer": "F.Cu" })))["hit"], json!(false));
        assert_eq!(std::fs::read_to_string(dir.join("design.json")).unwrap(), before, "hovering does not change the board");
    }

    #[test]
    fn a_single_track_is_tuned_to_the_target_as_one_undo_step() {
        let dir = scratch("single");
        setup(&dir, ["SIG", "OTHER"], vec![]);
        let before = design_of(&dir);
        let s = start(&dir, &body(json!({ "x": 8_000, "y": 9_500, "layer": "F.Cu", "mode": "single", "settings": { "target_length": 22_000 } })));
        assert_eq!(s["ok"], json!(true), "{s}");
        assert_eq!(s["origin"], json!({ "x": 8_000, "y": 9_500 }));
        // before the cursor moves there is nothing to put on the board
        assert_eq!(fix(&dir, &body(json!({}))).get("ok"), Some(&json!(false)));
        let start_again = start(&dir, &body(json!({ "x": 8_000, "y": 9_500, "layer": "F.Cu", "mode": "single", "settings": { "target_length": 22_000 } })));
        assert_eq!(start_again["ok"], json!(true));
        let m = mv(&dir, &body(json!({ "x": 22_000, "y": 9_000 })));
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
        assert_eq!(k["pattern"]["max_amplitude"], json!(950), "{k}");
        let k = step(&dir, &body(json!({ "what": "spacing_up" })));
        assert_eq!(k["pattern"]["spacing"], json!(650), "{k}");
        assert!((k["readout"]["current"].as_f64().unwrap() - 22.0).abs() < 0.1, "still tuned: {k}");

        let f = fix(&dir, &body(json!({ "x": 22_000, "y": 9_000 })));
        assert_eq!(f["ok"], json!(true), "{f}");
        let after = design_of(&dir);
        let patterns = tune_edit::patterns_of(&after);
        assert_eq!(patterns.len(), 1);
        let pat = &patterns[0];
        assert_eq!(f["pattern_id"], json!(pat.id));
        assert_eq!((pat.mode, pat.layer.as_str(), pat.origin, pat.end), (TuningMode::Single, "F.Cu", p(8_000, 9_500), p(22_000, 9_500)));
        assert_eq!(pat.base_line, vec![p(8_000, 9_500), p(22_000, 9_500)]);
        assert_eq!((pat.max_amplitude, pat.spacing), (950, 650));
        assert!(pat.member_ids.len() > 10, "the meander's tracks are the members: {}", pat.member_ids.len());
        let rt = after.routing.as_ref().unwrap();
        assert!(pat.member_ids.iter().all(|m| rt.tracks.iter().any(|t| &t.id == m)));
        assert!(rt.tracks.iter().any(|t| t.arc().is_some()), "rounded corners are arc tracks");
        assert!((net_mm(&dir, "SIG") - 22.0).abs() < 0.1, "{}", net_mm(&dir, "SIG"));
        assert_eq!(pat.tuning_info().split(' ').last(), Some("(tuned)"));
        // the other net is untouched
        assert!((net_mm(&dir, "OTHER") - 20.0).abs() < 0.001);

        board::undo(&dir, "test", None).expect("one undo takes the whole pattern back");
        let back = design_of(&dir);
        assert!(tune_edit::patterns_of(&back).is_empty());
        assert_eq!(tracks_of(&back), tracks_of(&before), "the board is as it was");
    }

    #[test]
    fn a_pattern_is_tuned_again_and_removed_and_each_is_one_undo_step() {
        let dir = scratch("retune");
        setup(&dir, ["SIG", "OTHER"], vec![]);
        start(&dir, &body(json!({ "x": 8_000, "y": 9_500, "layer": "F.Cu", "mode": "single", "settings": { "target_length": 22_000 } })));
        mv(&dir, &body(json!({ "x": 22_000, "y": 9_000 })));
        let f = fix(&dir, &body(json!({})));
        assert_eq!(f["ok"], json!(true), "{f}");
        let id = f["pattern_id"].as_str().unwrap().to_string();
        let tuned_once = design_of(&dir);

        // the pattern is listed with its tuning info, and its properties carry the target a rule would give
        let list = patterns(&dir);
        assert_eq!(list["patterns"].as_array().unwrap().len(), 1);
        assert_eq!(list["patterns"][0]["nets"], json!(["SIG"]));
        assert!(list["patterns"][0]["info"].as_str().unwrap().ends_with("(tuned)"), "{list}");
        let props = pattern(&dir, &body(json!({ "id": id })));
        assert_eq!(props["ok"], json!(true), "{props}");
        assert!(props["constraint"].is_null());

        // tuned again, longer: the same pattern, the same id, one step
        let u = update(&dir, &body(json!({ "id": id, "settings": { "target_length": 24_000 } })));
        assert_eq!(u["ok"], json!(true), "{u}");
        let after = design_of(&dir);
        let patterns_after = tune_edit::patterns_of(&after);
        assert_eq!(patterns_after.len(), 1);
        assert_eq!(patterns_after[0].id, id, "a pattern tuned again keeps its id");
        assert_eq!(patterns_after[0].target_length.opt, 24_000);
        assert!((net_mm(&dir, "SIG") - 24.0).abs() < 0.1, "{}", net_mm(&dir, "SIG"));
        board::undo(&dir, "test", None).unwrap();
        assert_eq!(tracks_of(&design_of(&dir)), tracks_of(&tuned_once), "one undo puts the first tuning back");
        assert_eq!(tune_edit::patterns_of(&design_of(&dir)), tune_edit::patterns_of(&tuned_once));

        // regenerated: tuned again as it is
        let g = regenerate(&dir, &body(json!({})));
        assert_eq!(g["ok"], json!(true), "{g}");
        assert_eq!(g["regenerated"], json!(1));
        assert!((net_mm(&dir, "SIG") - 22.0).abs() < 0.1);
        board::undo(&dir, "test", None).unwrap();

        // removed: the track is straight again and the pattern is gone
        let r = remove(&dir, &body(json!({ "id": id })));
        assert_eq!(r["ok"], json!(true), "{r}");
        let gone = design_of(&dir);
        assert!(tune_edit::patterns_of(&gone).is_empty());
        assert!((net_mm(&dir, "SIG") - 20.0).abs() < 0.001, "{}", net_mm(&dir, "SIG"));
        let tracks: Vec<&eda_model::ir::Track> = gone.routing.as_ref().unwrap().tracks.iter().filter(|t| t.net == "SIG").collect();
        assert!(tracks.iter().all(|t| t.pts.iter().all(|q| q.y == 9_500)), "all straight again: {tracks:?}");
        board::undo(&dir, "test", None).unwrap();
        assert_eq!(tune_edit::patterns_of(&design_of(&dir)).len(), 1, "one undo brings the pattern back");
    }

    #[test]
    fn a_differential_pair_is_tuned_together_and_removed_together() {
        let dir = scratch("pair");
        setup(&dir, ["USB_P", "USB_N"], vec![]);
        let s = start(&dir, &body(json!({ "x": 8_000, "y": 9_500, "layer": "F.Cu", "mode": "diffpair", "settings": { "target_length": 26_000 } })));
        assert_eq!(s["ok"], json!(true), "{s}");
        let m = mv(&dir, &body(json!({ "x": 22_000, "y": 9_000 })));
        assert_eq!(m["moved"], json!(true), "{m}");
        assert_eq!(m["readout"]["partner"], "USB_N");
        assert_eq!(m["preview"]["hide"].as_array().unwrap().len(), 2, "both lines are replaced");
        let f = fix(&dir, &body(json!({})));
        assert_eq!(f["ok"], json!(true), "{f}");
        let (p_len, n_len) = (net_mm(&dir, "USB_P"), net_mm(&dir, "USB_N"));
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
        assert!((net_mm(&dir, "USB_P") - 20.0).abs() < 0.001 && (net_mm(&dir, "USB_N") - 20.0).abs() < 0.001);
    }

    #[test]
    fn the_skew_tuner_lengthens_the_clicked_line_until_it_matches_its_partner() {
        let dir = scratch("skew");
        // USB_N takes a 0.7 mm detour on the way: 1.4 mm longer than USB_P, which runs straight
        let n_path = vec![p(5_000, 10_500), p(10_000, 10_500), p(10_000, 11_200), p(12_000, 11_200), p(12_000, 10_500), p(25_000, 10_500)];
        setup_with(&dir, ["USB_P", "USB_N"], vec![], vec![("USB_P", vec![p(5_000, 9_500), p(25_000, 9_500)]), ("USB_N", n_path)]);
        let (p_before, n_before) = (net_mm(&dir, "USB_P"), net_mm(&dir, "USB_N"));
        assert!((n_before - p_before - 1.4).abs() < 0.01, "P {p_before} N {n_before}");
        let h = hover(&dir, &body(json!({ "x": 15_000, "y": 9_500, "layer": "F.Cu", "mode": "skew" })));
        assert_eq!(h["readout"]["label"], "current skew", "{h}");
        assert!((h["readout"]["current"].as_f64().unwrap() + 1.4).abs() < 0.01, "P is 1.4 mm short of N: {h}");
        assert_eq!(h["readout"]["status"], "too_short");
        // the skew target is zero, 0.1 mm either side
        assert_eq!((h["readout"]["min"].as_f64(), h["readout"]["opt"].as_f64(), h["readout"]["max"].as_f64()), (Some(-0.1), Some(0.0), Some(0.1)));
        let s = start(&dir, &body(json!({ "x": 8_000, "y": 9_500, "layer": "F.Cu", "mode": "skew" })));
        assert_eq!(s["ok"], json!(true), "{s}");
        let m = mv(&dir, &body(json!({ "x": 22_000, "y": 9_000 })));
        assert_eq!(m["readout"]["status"], "tuned", "{m}");
        assert!(m["readout"]["current"].as_f64().unwrap().abs() < 0.1, "{m}");
        let f = fix(&dir, &body(json!({})));
        assert_eq!(f["ok"], json!(true), "{f}");
        let (p_after, n_after) = (net_mm(&dir, "USB_P"), net_mm(&dir, "USB_N"));
        assert!((p_after - n_after).abs() < 0.1, "P {p_after} N {n_after}");
        assert!((n_after - n_before).abs() < 0.001, "only the clicked line is lengthened");
        let pat = tune_edit::patterns_of(&design_of(&dir)).remove(0);
        assert_eq!(pat.mode, TuningMode::DiffPairSkew);
        assert!(pat.member_ids.iter().all(|m| design_of(&dir).routing.unwrap().tracks.iter().any(|t| &t.id == m && t.net == "USB_P")), "the members are USB_P's");
        // a skew pattern puts back only its own line
        let r = remove(&dir, &body(json!({ "id": pat.id })));
        assert_eq!(r["ok"], json!(true), "{r}");
        assert!((net_mm(&dir, "USB_P") - p_before).abs() < 0.001);
    }

    #[test]
    fn a_line_of_several_tracks_is_replaced_where_the_pattern_is_and_no_further() {
        let dir = scratch("several");
        // one net, three tracks end to end: 5-10, 10-20 (a polyline with a vertex at 15) and 20-25 mm
        setup_with(
            &dir,
            ["SIG", "OTHER"],
            vec![],
            vec![("SIG", vec![p(5_000, 9_500), p(10_000, 9_500)]), ("SIG", vec![p(10_000, 9_500), p(15_000, 9_500), p(20_000, 9_500)]), ("SIG", vec![p(20_000, 9_500), p(25_000, 9_500)]), ("OTHER", vec![p(5_000, 10_500), p(25_000, 10_500)])],
        );
        let before = design_of(&dir);
        let ids_before: Vec<String> = tracks_of(&before).iter().filter(|t| t.net == "SIG").map(|t| t.id.clone()).collect();
        assert_eq!(ids_before.len(), 3);
        start(&dir, &body(json!({ "x": 12_000, "y": 9_500, "layer": "F.Cu", "mode": "single", "settings": { "target_length": 22_000 } })));
        let m = mv(&dir, &body(json!({ "x": 18_000, "y": 9_000 })));
        assert_eq!(m["readout"]["status"], "tuned", "{m}");
        let f = fix(&dir, &body(json!({})));
        assert_eq!(f["ok"], json!(true), "{f}");
        let after = design_of(&dir);
        let sig_ids: Vec<String> = tracks_of(&after).iter().filter(|t| t.net == "SIG").map(|t| t.id.clone()).collect();
        // the first and the last track were not touched: they are the very same tracks
        assert!(sig_ids.contains(&ids_before[0]) && sig_ids.contains(&ids_before[2]), "{sig_ids:?} {ids_before:?}");
        assert!(!sig_ids.contains(&ids_before[1]), "the middle track is replaced");
        assert!((net_mm(&dir, "SIG") - 22.0).abs() < 0.1);
        let pat = tune_edit::patterns_of(&after).remove(0);
        assert!(pat.member_ids.iter().all(|m| !ids_before.contains(m)));
        board::undo(&dir, "test", None).unwrap();
        assert_eq!(tracks_of(&design_of(&dir)), tracks_of(&before));
    }

    #[test]
    fn a_track_that_cannot_be_tuned_is_refused_with_kicads_message() {
        let dir = scratch("refuse");
        setup(&dir, ["SIG", "OTHER"], vec![]);
        // a pair tuner on a net that is not half of a pair
        let s = start(&dir, &body(json!({ "x": 8_000, "y": 9_500, "layer": "F.Cu", "mode": "diffpair" })));
        assert_eq!(s["ok"], json!(false));
        assert!(s["message"].as_str().unwrap().contains("complementary differential pair"), "{s}");
        let s = start(&dir, &body(json!({ "x": 8_000, "y": 1_000, "layer": "F.Cu", "mode": "single" })));
        assert_eq!(s["message"], "Select a track to tune first.");
        assert_eq!(mv(&dir, &body(json!({ "x": 1, "y": 2 })))["message"], NO_SESSION);
        assert_eq!(cancel(&dir)["had_session"], json!(false));
    }

    #[test]
    fn a_session_on_a_board_that_changed_is_refused() {
        let dir = scratch("stale");
        setup(&dir, ["SIG", "OTHER"], vec![]);
        start(&dir, &body(json!({ "x": 8_000, "y": 9_500, "layer": "F.Cu", "mode": "single", "settings": { "target_length": 22_000 } })));
        mv(&dir, &body(json!({ "x": 22_000, "y": 9_000 })));
        board::step(&dir, Cmd::AddTrack { net: "OTHER".into(), layer: "F.Cu".into(), width: 250, pts: vec![p(2_000, 3_000), p(4_000, 3_000)] }, false, "test").unwrap();
        let f = fix(&dir, &body(json!({})));
        assert_eq!(f["ok"], json!(false));
        assert!(f["message"].as_str().unwrap().contains("changed"), "{f}");
    }
}
