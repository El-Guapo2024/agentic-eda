//! KiCad's own router regressions (`qa/data/pcbnew/pns_regressions`) replayed
//! on this router. Each case is a board from the QA corpus, a recorded mouse
//! session (`pns.log`) and the commit KiCad recorded at the end of it.
//!
//! Skipped (prints a line, passes) when the QA corpus is not on this machine:
//! `KICAD_QA_DATA`, default `~/ws/kicad-src-8303b2ad/qa/data`.

use eda_model::ir::{Point, Um};
use eda_pns::from_ir::build_node;
use eda_pns::item::{net_of, Item, ItemId};
use eda_pns::line::Line;
use eda_pns::node::Node;
use std::collections::HashSet;
use eda_pns::line_placer::LinePlacer;
use eda_pns::dragger::{DragKind, DragPreview, Dragger};
use eda_pns::router::{RouteCommit, Router};
use eda_pns::settings::{Mode, OptEffort, RoutingSettings};
use std::path::PathBuf;

fn qa_root() -> Option<PathBuf> {
    let p = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data"));
    p.join("pcbnew/pns_regressions").exists().then_some(p.join("pcbnew/pns_regressions"))
}

fn um(nm: &serde_json::Value) -> Um {
    (nm.as_f64().unwrap() / 1000.0).round() as Um
}

fn pt(v: &serde_json::Value) -> Point {
    Point { x: um(&v["x"]), y: um(&v["y"]) }
}

fn settings_of(dir: &std::path::Path) -> RoutingSettings {
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("pns.settings")).unwrap()).unwrap();
    RoutingSettings {
        mode: match j["mode"].as_i64().unwrap() {
            0 => Mode::MarkObstacles,
            1 => Mode::Shove,
            _ => Mode::Walkaround,
        },
        optimizer_effort: match j["effort"].as_i64().unwrap() {
            0 => OptEffort::Low,
            1 => OptEffort::Medium,
            _ => OptEffort::Full,
        },
        shove_vias: j["shove_vias"].as_bool().unwrap(),
        remove_loops: j["remove_loops"].as_bool().unwrap(),
        smart_pads: j["smart_pads"].as_bool().unwrap(),
        jump_over_obstacles: j["jump_over_obstacles"].as_bool().unwrap(),
        shove_iteration_limit: j["shove_iteration_limit"].as_i64().unwrap() as i32,
        walkaround_iteration_limit: j["walkaround_iteration_limit"].as_i64().unwrap() as i32,
        ..RoutingSettings::default()
    }
}

fn load(root: &std::path::Path, board: &str, case: &str) -> (eda_model::ir::Design, eda_model::ConstraintModel) {
    let text = std::fs::read_to_string(root.join("boards").join(format!("{board}.kicad_pcb"))).unwrap();
    let (mut design, mut model, _) = eda_kicad::import_kicad_pcb(&text).unwrap_or_else(|e| panic!("import: {e:?}"));
    if let Ok(pro) = std::fs::read_to_string(root.join(case).join("pns.kicad_pro")) {
        eda_kicad::merge_project_net_classes(&mut model, &pro);
        eda_kicad::merge_project_design_rules(&mut model, &pro);
    }
    design.assign_missing_ids();
    (design, model)
}

/// Every pair of items the router's own collision test calls a violation.
fn violations(node: &Node, rules: &eda_model::BoardRules) -> HashSet<(String, String)> {
    let name = |id: ItemId| match node.get(id) {
        Some(Item::Segment(s)) => format!("seg {:?}-{:?} {:?}", s.a, s.b, s.net),
        Some(Item::Via(v)) => format!("via {:?} {:?}", v.pos, v.net),
        Some(Item::Solid(s)) => format!("pad {}", s.source),
        None => String::new(),
    };
    let mut out = HashSet::new();
    for (id, item) in node.iter() {
        for o in node.all_colliding(&item.shape(item.layers().start()), item.net(), item.layers(), rules, &[id]) {
            let (a, b) = (name(id), name(o.id));
            out.insert(if a < b { (a, b) } else { (b, a) });
        }
    }
    out
}

/// The collisions of the items `after` has and `before` does not -- what a change newly puts in violation.
fn new_item_violations(before: &Node, after: &Node, rules: &eda_model::BoardRules) -> Vec<String> {
    let mut out = Vec::new();
    for (id, item) in after.iter() {
        if before.contains(id) {
            continue;
        }
        for o in after.all_colliding(&item.shape(item.layers().start()), item.net(), item.layers(), rules, &[id]) {
            out.push(format!("{:?} against {:?}", item.shape(item.layers().start()), after.get(o.id).map(|i| i.shape(i.layers().start()))));
        }
    }
    out
}

/// The board after a session's shoved tracks and vias are committed, as the studio would have it.
fn committed(node: &Node, pv: &eda_pns::line_placer::Preview) -> Node {
    let mut w = node.branch();
    let tracks: HashSet<String> = pv.displaced_lines.iter().filter_map(|d| d.source_track.clone()).collect();
    let doomed: Vec<ItemId> = w.iter().filter(|(_, it)| matches!(it, Item::Segment(s) if s.source_track.as_ref().is_some_and(|(t, _)| tracks.contains(t)))).map(|(id, _)| id).collect();
    for id in doomed {
        w.remove(id);
    }
    for d in &pv.displaced_lines {
        if d.line.point_count() >= 2 {
            w.add_line(&d.line, None, false);
        }
    }
    for dv in &pv.displaced_vias {
        let found = w.iter().find_map(|(id, it)| match it {
            Item::Via(v) if v.source_via.as_deref() == Some(dv.source_via.as_str()) => Some((id, v.clone())),
            _ => None,
        });
        if let Some((id, mut v)) = found {
            w.remove(id);
            v.pos = dv.pos;
            w.add(Item::Via(v));
        }
    }
    w.add_line(&Line::from_points(pv.head.net.clone(), pv.head.layer, pv.head.width, pv.head.pts.clone()), None, false);
    w
}

/// `addedItems` of a recorded commit: `(net, a, b)` per segment, in µm.
fn recorded_segments(log: &serde_json::Value) -> Vec<(String, Point, Point)> {
    log["addedItems"].as_array().unwrap().iter().filter(|it| it["kind"] == "segment").map(|it| (it["net"].as_str().unwrap().to_string(), pt(&it["shape"]["start"]), pt(&it["shape"]["end"]))).collect()
}

fn close(a: Point, b: Point, tol: Um) -> bool {
    (a.x - b.x).abs() <= tol && (a.y - b.y).abs() <= tol
}

fn same_segment(x: &(String, Point, Point), y: &(String, Point, Point), tol: Um) -> bool {
    x.0 == y.0 && ((close(x.1, y.1, tol) && close(x.2, y.2, tol)) || (close(x.1, y.2, tol) && close(x.2, y.1, tol)))
}

/// The design after a finished route's commit, as `Cmd::CommitRoute` applies it.
fn apply_commit(design: &eda_model::ir::Design, commit: &RouteCommit) -> eda_model::ir::Design {
    let mut d = design.clone();
    let rt = d.routing.as_mut().unwrap();
    rt.tracks.retain(|t| !commit.remove_track_ids.contains(&t.id));
    rt.vias.retain(|v| !commit.remove_via_ids.contains(&v.id));
    rt.tracks.extend(commit.tracks.iter().cloned());
    rt.vias.extend(commit.vias.iter().cloned());
    d.assign_missing_ids();
    d
}

fn route_events(root: &std::path::Path, case: &str, board: &str) -> (eda_model::ir::Design, eda_model::ConstraintModel, serde_json::Value, RoutingSettings) {
    let dir = root.join(case);
    let log_file = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "log")).unwrap();
    let log: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(log_file).unwrap()).unwrap();
    let (design, model) = load(root, board, case);
    (design, model, log, settings_of(&dir))
}

/// simple-shove-1: a free-hand track dragged through a busy part of `simple.kicad_pcb` in Shove
/// mode, with no click -- the commit KiCad recorded is the 13 tracks it pushed and the 28 segments
/// they became.
#[test]
fn simple_shove_1_pushes_the_same_13_tracks_and_leaves_the_board_clean() {
    let Some(root) = qa_root() else {
        eprintln!("KiCad QA corpus not found: skipping");
        return;
    };
    let (design, model, log, settings) = route_events(&root, "simple-shove-1", "simple");
    let (node, layers) = build_node(&design, &model);
    let events = log["events"].as_array().unwrap();
    // the replay routes with the Default net class's width (the event's own 155 µm is not what KiCad used)
    let width = model.board.width_of("");
    let layer = layers.index_of("F.Cu").unwrap();
    let placer = LinePlacer::start(&node, pt(&events[0]["position"]), None, net_of("(unconnected)"), layer, width);
    let mut last = None;
    for e in &events[1..] {
        last = Some(placer.preview(&node, &model.board, &settings, pt(&e["position"])));
    }
    let pv = last.unwrap();
    assert!(!pv.colliding);

    // the same tracks are pushed
    let removed = pv.displaced_lines.iter().filter_map(|d| d.source_track.clone()).collect::<std::collections::BTreeSet<_>>().len();
    assert_eq!(removed, log["removedItems"].as_array().unwrap().len(), "KiCad pushed 13 tracks");
    // the pushed lines run where KiCad's do (the optimizer picks different corner cuts in places)
    let ours: Vec<(String, Point, Point)> = pv.displaced_lines.iter().flat_map(|d| d.line.segs().map(|(a, b)| (d.line.net.as_deref().unwrap_or("").to_string(), a, b)).collect::<Vec<_>>()).collect();
    let expected = recorded_segments(&log);
    let hit = expected.iter().filter(|e| ours.iter().any(|o| same_segment(e, o, 3))).count();
    assert!(hit >= 10, "{hit} of {} recorded segments reproduced within 3 um", expected.len());
    // and the board it leaves is clean
    let before = violations(&node, &model.board);
    let after = violations(&committed(&node, &pv), &model.board);
    let new_ones: Vec<_> = after.difference(&before).collect();
    assert!(new_ones.is_empty(), "new violations: {new_ones:#?}");
}

/// backspace1: a route in Shove mode with nine clicks undone by Backspace and the first one redone.
#[test]
fn backspace1_ends_with_the_one_fixed_run_kicad_recorded() {
    let Some(root) = qa_root() else {
        eprintln!("KiCad QA corpus not found: skipping");
        return;
    };
    let (design, model, log, settings) = route_events(&root, "backspace1", "backspace1");
    let mut router = Router::new(&design, &model);
    router.settings = settings;
    let events = log["events"].as_array().unwrap();
    let width = model.board.width_of("GND");
    router.start(pt(&events[0]["position"]), "F.Cu", width).expect("starts on the GND pad");
    let mut last_fix = pt(&events[0]["position"]);
    for e in &events[1..] {
        match e["type"].as_i64().unwrap() {
            3 => {
                router.preview(pt(&e["position"])).expect("a route is in progress");
            }
            2 => {
                last_fix = pt(&e["position"]);
                router.fix(last_fix).expect("a route is in progress");
            }
            6 => {
                router.undo_last_segment();
            }
            t => panic!("unexpected event type {t}"),
        }
    }
    let commit = router.finish(last_fix).expect("the final run does not collide");
    let expected = recorded_segments(&log);
    let ours: Vec<(String, Point, Point)> = commit.tracks.iter().flat_map(|t| t.pts.windows(2).map(|w| (t.net.clone(), w[0], w[1])).collect::<Vec<_>>()).collect();
    assert_eq!(ours.len(), expected.len(), "ours {ours:?} expected {expected:?}");
    for e in &expected {
        assert!(ours.iter().any(|o| same_segment(e, o, 2)), "recorded segment {e:?} missing from {ours:?}");
    }
    assert!(commit.tracks.iter().all(|t| t.width == width));
}

/// issue22749: a route started from the end of a track in Shove mode, pushing the neighbour's track around its head.
#[test]
fn issue22749_pushes_the_same_5_tracks_and_leaves_the_board_clean() {
    let Some(root) = qa_root() else {
        eprintln!("KiCad QA corpus not found: skipping");
        return;
    };
    let (design, model, log, settings) = route_events(&root, "issue22749-shove-weird-drag-track-end", "pic_programmer");
    let (node, _) = build_node(&design, &model);
    let mut router = Router::new(&design, &model);
    router.settings = settings;
    let events = log["events"].as_array().unwrap();
    let start = pt(&events[0]["position"]);
    let net = node.nearest_anchor(start, eda_pns::layer::LayerRange::single(1), 500, None).and_then(|(id, _)| node.get(id).and_then(|i| i.net().clone())).expect("a track end with a net");
    router.start(start, "B.Cu", model.board.width_of(&net)).expect("starts on the track end");
    let mut last = None;
    for e in &events[1..] {
        last = router.preview(pt(&e["position"]));
    }
    let pv = last.unwrap();
    assert!(!pv.colliding);
    let removed = pv.displaced_lines.iter().filter_map(|d| d.source_track.clone()).collect::<std::collections::BTreeSet<_>>().len();
    assert_eq!(removed, log["removedItems"].as_array().unwrap().len(), "KiCad pushed 5 tracks");
    let ours: Vec<(String, Point, Point)> = pv.displaced_lines.iter().flat_map(|d| d.line.segs().map(|(a, b)| (d.line.net.as_deref().unwrap_or("").to_string(), a, b)).collect::<Vec<_>>()).collect();
    let expected = recorded_segments(&log);
    let hit = expected.iter().filter(|e| ours.iter().any(|o| same_segment(e, o, 3))).count();
    assert!(hit >= 4, "{hit} of {} recorded segments reproduced within 3 um", expected.len());
    let before = violations(&node, &model.board);
    let after = violations(&committed(&node, &pv), &model.board);
    let new_ones: Vec<_> = after.difference(&before).collect();
    assert!(new_ones.is_empty(), "new violations: {new_ones:#?}");

    // The same through the commit the studio applies (`Cmd::CommitRoute`): the pushed tracks are
    // replaced by their new shapes, nothing is left behind or doubled, the board stays clean.
    let commit = router.finish(pt(&events.last().unwrap()["position"])).expect("the pushed route can be finished");
    let ids: HashSet<&String> = design.routing.as_ref().unwrap().tracks.iter().map(|t| &t.id).collect();
    assert!(commit.remove_track_ids.iter().all(|id| ids.contains(id)), "every removed track exists: {:?}", commit.remove_track_ids);
    let committed_design = apply_commit(&design, &commit);
    let (after_node, _) = build_node(&committed_design, &model);
    let length_of = |d: &eda_model::ir::Design, net: &str| -> i64 { d.routing.as_ref().unwrap().tracks.iter().filter(|t| t.net == net).map(|t| t.pts.windows(2).map(|w| (((w[1].x - w[0].x) as f64).powi(2) + ((w[1].y - w[0].y) as f64).powi(2)).sqrt() as i64).sum::<i64>()).sum() };
    // the pushed net is still one run from end to end, not the old track plus a copy of it
    assert!(length_of(&committed_design, "/VPP{slash}MCLR") < 2 * length_of(&design, "/VPP{slash}MCLR"), "the pushed track was replaced, not duplicated");
    let after_new = violations(&after_node, &model.board);
    let new_after_commit: Vec<_> = after_new.difference(&before).collect();
    assert!(new_after_commit.is_empty(), "new violations after the commit: {new_after_commit:#?}");
}

/// The board after a drag preview is committed.
fn dragged(node: &Node, d: &Dragger, pv: &DragPreview, to: Point) -> Node {
    let mut w = node.branch();
    let tracks: HashSet<String> = pv.displaced_lines.iter().filter_map(|x| x.source_track.clone()).collect();
    let doomed: Vec<ItemId> = w.iter().filter(|(_, it)| matches!(it, Item::Segment(s) if s.source_track.as_ref().is_some_and(|(t, _)| tracks.contains(t)))).map(|(id, _)| id).collect();
    for id in doomed {
        w.remove(id);
    }
    for x in &pv.displaced_lines {
        if x.line.point_count() >= 2 {
            w.add_line(&x.line, None, false);
        }
    }
    match d.kind {
        DragKind::Corner => {
            w.remove_line_segments(&d.original);
            w.add_line(&Line::from_points(d.net.clone(), d.layer, d.width, pv.pts.clone()), None, false);
        }
        DragKind::Via => {
            if let Some(id) = d.via_id {
                w.remove(id);
            }
            w.add(Item::Via(eda_pns::item::Via { net: d.net.clone(), layers: eda_pns::layer::LayerRange::new(0, 1), pos: to, diameter: d.via_diameter, drill: d.via_drill, source_via: d.source_via.clone(), locked: false }));
            for l in &pv.fanout {
                w.remove_line_segments(l);
                w.add_line(l, None, false);
            }
        }
    }
    for v in &pv.displaced_vias {
        let found = w.iter().find_map(|(id, it)| match it {
            Item::Via(x) if x.source_via.as_deref() == Some(v.source_via.as_str()) => Some((id, x.clone())),
            _ => None,
        });
        if let Some((id, mut x)) = found {
            w.remove(id);
            x.pos = v.pos;
            w.add(Item::Via(x));
        }
    }
    w
}

/// Replays a recorded drag with this port's `Dragger` (which drags the nearest end of a track, where KiCad
/// slides the whole segment): no panic, and whenever the drag is accepted the board it leaves is clean.
fn replay_drag(case: &str, board: &str) -> (usize, usize) {
    let Some(root) = qa_root() else {
        eprintln!("KiCad QA corpus not found: skipping");
        return (0, 0);
    };
    let (design, model, log, settings) = route_events(&root, case, board);
    let (node, layers) = build_node(&design, &model);
    let events = log["events"].as_array().unwrap();
    let start = pt(&events[0]["position"]);
    // the dragged item's own layer (the event's is the router's active one)
    let layer = log["headItems"].as_array().and_then(|h| h.first()).map(|h| h["layers"][0].as_i64().unwrap() as i32).unwrap_or_else(|| events[0]["layer"].as_i64().unwrap() as i32);
    let id = node.item_at(start, eda_pns::layer::LayerRange::single(layer), 300).unwrap_or_else(|| panic!("nothing to drag at {start:?} on {}", layers.name_of(layer)));
    let dragger = Dragger::start_with(&node, start, id, false).expect("a track or via");
    let (mut accepted, mut shoved) = (0, 0);
    for e in &events[1..] {
        let to = pt(&e["position"]);
        let pv = dragger.preview(&node, &model.board, &settings, to);
        if pv.colliding {
            continue;
        }
        accepted += 1;
        if !pv.displaced_lines.is_empty() || !pv.displaced_vias.is_empty() {
            shoved += 1;
        }
        let new_ones = new_item_violations(&node, &dragged(&node, &dragger, &pv, to), &model.board);
        assert!(new_ones.is_empty(), "dragging to {to:?} leaves new violations: {new_ones:#?}");
    }
    (accepted, shoved)
}

#[test]
fn issue23449_dragging_a_lone_via_does_not_crash_and_keeps_the_board_clean() {
    let (accepted, shoved) = replay_drag("issue23449-shove-lone-via-drag-crash", "stickhub-extra-via");
    eprintln!("accepted {accepted} previews, {shoved} of them shoved something");
}

/// `video-v10.kicad_pcb` is 6 MB: a debug build takes about a minute, so it runs with the slow tier.
#[test]
fn simple_drag_shove_singlelayer_keeps_the_board_clean() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped (set EDA_SLOW_TESTS=1): video-v10.kicad_pcb takes about a minute in a debug build");
        return;
    }
    let (accepted, shoved) = replay_drag("simple-drag-shove-singlelayer", "video-v10");
    eprintln!("accepted {accepted} previews, {shoved} of them shoved something");
}

#[test]
fn walk_drag_seg_against_board_edge_keeps_the_board_clean() {
    let (accepted, shoved) = replay_drag("walk_drag_seg_against_board_edge", "ultrasound");
    eprintln!("accepted {accepted} previews, {shoved} of them shoved something");
}
