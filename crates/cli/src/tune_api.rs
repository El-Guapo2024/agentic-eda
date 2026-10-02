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
//! Three modes, picked by the request's `mode` (default `"single"`):
//!
//! * `"single"` (key `7`, `PNS::MEANDER_PLACER`): lengthen one track to
//!   `target_length`.
//! * `"diffpair"` (key `8`, `PNS::DP_MEANDER_PLACER`): meander the pair the
//!   track belongs to together until the longer net reaches `target_length`.
//! * `"skew"` (key `9`, `PNS::MEANDER_SKEW_PLACER`): lengthen the track until
//!   its net is `target_skew` (default 0) longer than the complementary net.
//!
//! The geometry of the last two is `eda_pns::dp_tune`; this file adds the
//! board read, the collision check against everything else on the board
//! (the lines being replaced excluded), the JSON, and the commit.
//!
//! Scope (see `eda_pns::meander`'s own doc comment for the full reasoning):
//! only existing tracks that are **straight, single-segment** runs (their IR
//! `pts` has exactly 2 points) can be tuned -- reported back as a
//! `message`, not a panic, for anything else; a pair is tuned only where
//! both of its lines are such a run, side by side.

use crate::board;
use eda_model::ir::{Design, Point, Track, Um};
use eda_pns::dp_tune::{net_length, tune_diff_pair, tune_skew};
use eda_pns::item::Item;
use eda_pns::meander::{generate_meander, polyline_length};
use serde_json::{json, Map, Value};
use std::path::Path;

fn num(v: &Value, key: &str, default: i64) -> i64 {
    v.get(key).and_then(Value::as_i64).unwrap_or(default)
}

fn pts_json(pts: &[Point]) -> Value {
    json!(pts.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>())
}

/// One computed tuning, ready to preview or commit.
struct Tuned {
    /// Every track the tuning replaces and the points that replace it.
    replace: Vec<(Track, Vec<Point>)>,
    /// Whether any of the new lines hits something else on the board.
    colliding: bool,
    /// The reply's mode-specific members (merged into `{"ok": true, ...}`).
    info: Map<String, Value>,
    /// What the dialog headlines as the achieved length (the track's own, the pair's, or the line's net).
    achieved_length: Um,
}

/// Do any of `replace`'s new lines collide with anything on the board other than the tracks they replace (same net
/// excluded -- the convention every other preview in this crate uses)?
fn collides(design: &Design, model: &eda_model::ConstraintModel, replace: &[(Track, Vec<Point>)]) -> Result<bool, String> {
    let (node, layers) = eda_pns::from_ir::build_node(design, model);
    let exclude: Vec<_> = node
        .iter()
        .filter_map(|(id, item)| match item {
            Item::Segment(s) if s.source_track.as_ref().is_some_and(|(t, _)| replace.iter().any(|(r, _)| &r.id == t)) => Some(id),
            _ => None,
        })
        .collect();
    for (track, pts) in replace {
        let layer = layers.index_of(&track.layer).ok_or_else(|| format!("unknown layer {:?}", track.layer))?;
        let net = eda_pns::item::net_of(&track.net);
        let layers_range = eda_pns::layer::LayerRange::single(layer);
        let hit = pts.windows(2).any(|w| {
            let shape = eda_drc::kimath::Shape::Stadium { a: w[0], b: w[1], r: track.width.max(1) / 2 };
            node.first_colliding(&shape, &net, layers_range, &model.board, &exclude).is_some()
        });
        if hit {
            return Ok(true);
        }
    }
    Ok(false)
}

/// A tuning that could not be computed: the sentence to show, plus whatever the reply could already say (a pair's
/// current length and partner net, so the dialog can still offer a sensible default target after "the target must
/// be longer than ...").
struct TuneErr {
    message: String,
    info: Map<String, Value>,
}

/// `{mode?, track_id, amplitude, spacing, target_length, target_skew?, flip?}` -> what the tuning replaces, whether
/// the result collides, and the reply members.
fn compute(dir: &Path, req: &Value) -> Result<Tuned, TuneErr> {
    let mut info = Map::new();
    let fail = |message: String, info: &Map<String, Value>| TuneErr { message, info: info.clone() };
    let (_, design, model) = board::load(dir).map_err(|e| fail(board::reasons(&e), &info))?;
    let track_id = req.get("track_id").and_then(Value::as_str).ok_or_else(|| fail("missing track_id".into(), &info))?;
    let tracks: &[Track] = design.routing.as_ref().map(|r| r.tracks.as_slice()).unwrap_or(&[]);
    let track = tracks.iter().find(|t| t.id == track_id).ok_or_else(|| fail("no such track".into(), &info))?.clone();
    let amplitude = num(req, "amplitude", 200) as Um;
    let spacing = num(req, "spacing", 400) as Um;
    let target_length = num(req, "target_length", 0) as Um;
    let flip = req.get("flip").and_then(Value::as_bool).unwrap_or(false);
    let mode = req.get("mode").and_then(Value::as_str).unwrap_or("single");

    info.insert("mode".into(), json!(mode));
    info.insert("net".into(), json!(track.net));
    info.insert("layer".into(), json!(track.layer));
    info.insert("width".into(), json!(track.width));
    // What a pair/skew reply knows even when the tuning itself is refused: the lengths the dialog starts from.
    if let Some(partner_net) = eda_pns::diff_pair::dp_coupled_net_name(&track.net).filter(|_| mode != "single") {
        let (own, coupled) = (net_length(tracks, &track.net), net_length(tracks, &partner_net));
        info.insert("partner_net".into(), json!(partner_net));
        if mode == "diffpair" {
            info.insert("original_length".into(), json!(own.max(coupled)));
        } else {
            info.insert("original_length".into(), json!(own));
            info.insert("partner_length".into(), json!(coupled));
            info.insert("skew_before".into(), json!(own - coupled));
        }
    }

    let (replace, achieved_length) = match mode {
        "single" => {
            if track.pts.len() != 2 {
                return Err(fail("select a straight track with no corners or vias -- this port's length tuner doesn't yet handle a multi-segment run".into(), &info));
            }
            let meander = generate_meander(track.pts[0], track.pts[1], amplitude, spacing, target_length, flip)
                .ok_or_else(|| fail("can't reach that target length -- it may already be at or below the track's own straight length, or the amplitude/spacing leave no room at all on this short a run".into(), &info))?;
            info.insert("original_length".into(), json!(polyline_length(&track.pts)));
            info.insert("pts".into(), pts_json(&meander.pts));
            let achieved = meander.achieved_length;
            (vec![(track.clone(), meander.pts)], achieved)
        }
        "diffpair" => {
            let tune = tune_diff_pair(tracks, track_id, amplitude, spacing, target_length, flip).map_err(|m| fail(m, &info))?;
            let partner = tracks.iter().find(|t| t.id == tune.partner_id).ok_or_else(|| fail("the partner track vanished".into(), &info))?.clone();
            info.insert("partner_id".into(), json!(tune.partner_id));
            info.insert("pitch".into(), json!(tune.pitch));
            info.insert("skew_before".into(), json!(tune.skew_before()));
            info.insert("skew_after".into(), json!(tune.skew_after()));
            info.insert("pts".into(), pts_json(&tune.a_pts));
            info.insert("partner_pts".into(), pts_json(&tune.b_pts));
            let achieved = tune.pair_length_after();
            (vec![(track.clone(), tune.a_pts), (partner, tune.b_pts)], achieved)
        }
        "skew" => {
            let target_skew = num(req, "target_skew", 0) as Um;
            let tune = tune_skew(tracks, track_id, amplitude, spacing, target_skew, flip).map_err(|m| fail(m, &info))?;
            info.insert("skew_after".into(), json!(tune.skew_after()));
            info.insert("pts".into(), pts_json(&tune.pts));
            let achieved = tune.own_after;
            (vec![(track.clone(), tune.pts)], achieved)
        }
        other => return Err(fail(format!("unknown tuning mode {other:?} (expected single, diffpair or skew)"), &info)),
    };
    let colliding = collides(&design, &model, &replace).map_err(|m| fail(m, &info))?;
    Ok(Tuned { replace, colliding, info, achieved_length })
}

fn err_reply(e: TuneErr) -> Value {
    let mut reply = e.info;
    reply.insert("ok".into(), json!(false));
    reply.insert("message".into(), json!(e.message));
    Value::Object(reply)
}

/// `POST /api/tune_length/preview`: read-only, never touches `design.json`.
pub fn preview(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    match compute(dir, &req) {
        Ok(t) => {
            let mut reply = t.info;
            reply.insert("ok".into(), json!(true));
            reply.insert("achieved_length".into(), json!(t.achieved_length));
            reply.insert("colliding".into(), json!(t.colliding));
            Value::Object(reply)
        }
        Err(e) => err_reply(e),
    }
}

/// `POST /api/tune_length/apply`: same body as `preview`; commits through
/// `Cmd::CommitRoute` (`crates/ops`) -- the exact same undo-step/
/// collision-replacement shape a finished route or drag already uses, so
/// this needed no new `Cmd` variant at all, just another caller of the
/// existing one (a pair's two tracks are one commit, one undo step).
/// Refuses (without committing) if the result still collides, same as
/// every other interactive-router finish in this crate.
pub fn apply(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let tuned = match compute(dir, &req) {
        Ok(v) => v,
        Err(e) => return err_reply(e),
    };
    if tuned.colliding {
        return json!({ "ok": false, "message": "the tuned shape collides with something else on the board; adjust amplitude/spacing or target length" });
    }
    let remove_track_ids = tuned.replace.iter().map(|(t, _)| t.id.clone()).collect();
    let tracks = tuned
        .replace
        .iter()
        .map(|(t, pts)| Track { id: String::new(), net: t.net.clone(), pins: t.pins.clone(), layer: t.layer.clone(), width: t.width, pts: pts.clone(), arc_mid_offset: None })
        .collect();
    let cmd = eda_ops::Cmd::CommitRoute { remove_track_ids, remove_via_ids: vec![], tracks, vias: vec![] };
    match board::step(dir, cmd, true, "ui") {
        Ok(summary) => json!({ "ok": true, "message": summary, "achieved_length": tuned.achieved_length }),
        Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{FootprintInstance, PlacementSection, Provenance, Side};
    use eda_model::{ConstraintModel, Footprint, Net, Pad, PadKind, PadShape, Part, Pin, PinKind};
    use eda_ops::Cmd;

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
        let d = std::env::temp_dir().join(format!("eda_cli_tune_test_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Scratch(d)
    }

    /// Two parts 20 mm apart, each with a USB_P pad above a USB_N pad (1 mm pitch), and a 20 mm P and N track joining them.
    fn setup_pair(dir: &Path) {
        let pad = |n: &str, y: i64| Pad { opposite_side: false, number: n.into(), at: (0, y), size: (400, 400), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None };
        let fp = Footprint { name: "PAIR2".into(), pads: vec![pad("1", -500), pad("2", 500)], courtyard: Some((1000, 2000)), model: None, courtyard_outlines: vec![] };
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
        let model = ConstraintModel {
            parts: vec![part("U1"), part("U2")],
            nets: vec![Net { name: "USB_P".into(), pins: vec!["U1.1".into(), "U2.1".into()] }, Net { name: "USB_N".into(), pins: vec!["U1.2".into(), "U2.2".into()] }],
            footprints: vec![fp],
            ..Default::default()
        };
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
                outline: vec![Point { x: 0, y: 0 }, Point { x: 40_000, y: 0 }, Point { x: 40_000, y: 20_000 }, Point { x: 0, y: 20_000 }],
                footprints: vec![
                    FootprintInstance { id: "U1".into(), at: Point { x: 5_000, y: 10_000 }, rot: 0, side: Side::Top, label: Default::default() },
                    FootprintInstance { id: "U2".into(), at: Point { x: 25_000, y: 10_000 }, rot: 0, side: Side::Top, label: Default::default() },
                ],
                modules: vec![],
            }),
            routing: None,
            drawings: None,
        };
        board::save(dir, &design).unwrap();
        let meta = board::Meta { intent: intent_path.display().to_string(), snap_um: 100, spacing_um: 300 };
        std::fs::write(dir.join("board.json"), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
        for (net, y) in [("USB_P", 9_500), ("USB_N", 10_500)] {
            let cmd = Cmd::AddTrack { net: net.into(), layer: "F.Cu".into(), width: 250, pts: vec![Point { x: 5_000, y }, Point { x: 25_000, y }] };
            board::step(dir, cmd, false, "test").unwrap();
        }
    }

    fn track_id(dir: &Path, net: &str) -> String {
        let (_, design, _) = board::load(dir).unwrap();
        design.routing.unwrap().tracks.iter().find(|t| t.net == net && t.pts.len() == 2).map(|t| t.id.clone()).expect("a straight track on that net")
    }

    fn body(v: Value) -> Vec<u8> {
        v.to_string().into_bytes()
    }

    fn net_len(dir: &Path, net: &str) -> Um {
        let (_, design, _) = board::load(dir).unwrap();
        eda_pns::dp_tune::net_length(&design.routing.unwrap().tracks, net)
    }

    #[test]
    fn diff_pair_preview_reports_the_pairs_length_and_never_touches_the_board() {
        let dir = scratch("dp_preview");
        setup_pair(&dir);
        let p = track_id(&dir, "USB_P");
        let reply = preview(&dir, &body(json!({"mode": "diffpair", "track_id": p, "amplitude": 600, "spacing": 1200, "target_length": 23_000})));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert_eq!(reply["mode"], "diffpair");
        assert_eq!(reply["net"], "USB_P");
        assert_eq!(reply["partner_net"], "USB_N");
        assert_eq!(reply["pitch"], json!(1000));
        assert_eq!(reply["original_length"], json!(20_000));
        assert!((reply["achieved_length"].as_i64().unwrap() - 23_000).abs() <= 3, "{reply}");
        assert_eq!(reply["colliding"], json!(false), "{reply}");
        assert!(reply["pts"].as_array().unwrap().len() > 10 && reply["partner_pts"].as_array().unwrap().len() > 10);
        assert_eq!(net_len(&dir, "USB_P"), 20_000, "preview must not change the board");
    }

    #[test]
    fn diff_pair_apply_replaces_both_tracks_in_one_undo_step() {
        let dir = scratch("dp_apply");
        setup_pair(&dir);
        let p = track_id(&dir, "USB_P");
        let reply = apply(&dir, &body(json!({"mode": "diffpair", "track_id": p, "amplitude": 600, "spacing": 1200, "target_length": 23_000})));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        let (np, nn) = (net_len(&dir, "USB_P"), net_len(&dir, "USB_N"));
        assert!((np.max(nn) - 23_000).abs() <= 3, "P {np} N {nn}");
        assert!((np - nn).abs() < 700, "the two lines were tuned together: P {np} N {nn}");
        let (_, design, _) = board::load(&dir).unwrap();
        let tracks = design.routing.unwrap().tracks;
        assert_eq!(tracks.len(), 2, "each original track was replaced by exactly one tuned track");
        // the ends still sit on the pads
        let p_track = tracks.iter().find(|t| t.net == "USB_P").unwrap();
        assert_eq!((p_track.pts.first(), p_track.pts.last()), (Some(&Point { x: 5_000, y: 9_500 }), Some(&Point { x: 25_000, y: 9_500 })));

        board::undo(&dir, "test", None).expect("one undo reverts the whole pair");
        assert_eq!((net_len(&dir, "USB_P"), net_len(&dir, "USB_N")), (20_000, 20_000));
    }

    #[test]
    fn skew_apply_lengthens_the_selected_line_until_the_nets_match() {
        let dir = scratch("skew_apply");
        setup_pair(&dir);
        // 4 mm more copper on USB_N elsewhere: P is now 4 mm short
        board::step(&dir, Cmd::AddTrack { net: "USB_N".into(), layer: "F.Cu".into(), width: 250, pts: vec![Point { x: 2_000, y: 2_000 }, Point { x: 6_000, y: 2_000 }] }, false, "test").unwrap();
        let p = track_id(&dir, "USB_P");
        // (a bump taller than half the 1 mm pitch would run into USB_N -- the collision check says so)
        let tall = preview(&dir, &body(json!({"mode": "skew", "track_id": p, "amplitude": 900, "spacing": 1200})));
        assert_eq!(tall["colliding"], json!(true), "a 900 um bump toward the partner must be flagged: {tall}");
        let pre = preview(&dir, &body(json!({"mode": "skew", "track_id": p, "amplitude": 300, "spacing": 600})));
        assert_eq!(pre["ok"], json!(true), "{pre}");
        assert_eq!(pre["colliding"], json!(false), "{pre}");
        assert_eq!(pre["skew_before"], json!(-4_000));
        assert!(pre["skew_after"].as_i64().unwrap().abs() <= 3, "{pre}");
        assert_eq!(pre["partner_length"], json!(24_000));

        let reply = apply(&dir, &body(json!({"mode": "skew", "track_id": p, "amplitude": 300, "spacing": 600})));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert!((net_len(&dir, "USB_P") - net_len(&dir, "USB_N")).abs() <= 3);
    }

    #[test]
    fn a_refused_pair_tuning_still_reports_the_lengths_the_dialog_needs_for_a_default_target() {
        let dir = scratch("dp_refused");
        setup_pair(&dir);
        let p = track_id(&dir, "USB_P");
        let reply = preview(&dir, &body(json!({"mode": "diffpair", "track_id": p, "amplitude": 600, "spacing": 1200, "target_length": 0})));
        assert_eq!(reply["ok"], json!(false));
        assert!(reply["message"].as_str().unwrap().contains("must be longer"), "{reply}");
        assert_eq!(reply["original_length"], json!(20_000));
        assert_eq!(reply["partner_net"], "USB_N");
        let skew = preview(&dir, &body(json!({"mode": "skew", "track_id": p, "amplitude": 600, "spacing": 1200})));
        assert_eq!(skew["ok"], json!(false));
        assert_eq!((skew["original_length"].clone(), skew["partner_length"].clone(), skew["skew_before"].clone()), (json!(20_000), json!(20_000), json!(0)));
    }

    #[test]
    fn single_mode_is_unchanged_and_modes_report_their_own_errors() {
        let dir = scratch("single_and_errors");
        setup_pair(&dir);
        let p = track_id(&dir, "USB_P");
        let single = preview(&dir, &body(json!({"track_id": p, "amplitude": 400, "spacing": 800, "target_length": 22_000})));
        assert_eq!(single["ok"], json!(true), "{single}");
        assert_eq!(single["mode"], "single");
        assert_eq!(single["original_length"], json!(20_000));
        assert!(single.get("partner_net").is_none());
        // skew on the longer-or-equal line has nothing to add
        let skew = preview(&dir, &body(json!({"mode": "skew", "track_id": p, "amplitude": 400, "spacing": 800})));
        assert_eq!(skew["ok"], json!(false));
        assert!(skew["message"].as_str().unwrap().contains("only lengthens the selected line"), "{skew}");
        // unknown mode / unknown track
        assert_eq!(preview(&dir, &body(json!({"mode": "bogus", "track_id": p})))["ok"], json!(false));
        assert_eq!(preview(&dir, &body(json!({"mode": "diffpair", "track_id": "nope", "target_length": 30_000})))["message"], "no such track");
    }
}
