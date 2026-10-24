//! KiCad's own router regressions (`qa/data/pcbnew/pns_regressions`) replayed
//! on this router. Each case is a board from the QA corpus, a recorded mouse
//! session (`pns.log`) and the commit KiCad recorded at the end of it.
//!
//! The corpus is `$KICAD_QA_DATA`, else `$KICAD_SRC_DIR/qa/data`. Without it a test prints a line
//! and passes in the fast tier, and **fails** under `EDA_SLOW_TESTS=1`, so the slow gate cannot pass
//! without running the replays.
//!
//! KiCad's own test (`qa/tools/pns/pns_log_file.cpp::PNS_LOG_FILE::COMMIT_STATE::Compare` ->
//! `comparePnsItems`, `sa->Seg() != sb->Seg()`, KiCad 25ec2335) demands every recorded segment
//! exactly, in nm, and so does every replay here: all seven 8303b2ad sessions are reproduced to the
//! nanometre. Each prints how many segments it reproduces exactly and within `TOL`, and the ones it
//! misses.

use eda_model::ir::{Point, Nm};
use eda_pns::from_ir::build_node;
use eda_pns::item::{Item, ItemId};
use eda_pns::line::Line;
use eda_pns::node::Node;
use std::collections::HashSet;
use eda_pns::dragger::{DragKind, DragPreview, Dragger};
use eda_pns::router::{RouteCommit, Router};
use eda_pns::settings::{Mode, OptEffort, RoutingSettings};
use std::path::PathBuf;

fn qa_root() -> Option<PathBuf> {
    let data = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).or_else(|| std::env::var_os("KICAD_SRC_DIR").map(|s| PathBuf::from(s).join("qa/data")));
    let found = data.map(|p| p.join("pcbnew/pns_regressions")).filter(|p| p.exists());
    if found.is_none() && std::env::var_os("EDA_SLOW_TESTS").is_some() {
        panic!("EDA_SLOW_TESTS is set but KiCad's QA corpus is not found: set KICAD_QA_DATA (or KICAD_SRC_DIR) to a KiCad source tree's qa/data");
    }
    found
}

/// How many of the `expected` segments `ours` reproduces exactly (KiCad's compare) and within `TOL`, printed for the report.
fn hits(case: &str, expected: &[(String, Point, Point)], ours: &[(String, Point, Point)]) -> (usize, usize) {
    let count = |tol: Nm| expected.iter().filter(|e| ours.iter().any(|o| same_segment(e, o, tol))).count();
    let (exact, near) = (count(0), count(TOL));
    eprintln!("{case}: {exact} of {} recorded segments exact, {near} within {TOL} nm; ours {} segments", expected.len(), ours.len());
    for e in expected.iter().filter(|e| !ours.iter().any(|o| same_segment(e, o, 0))) {
        eprintln!("MISS {e:?}");
    }
    for o in ours.iter().filter(|o| !expected.iter().any(|e| same_segment(e, o, 0))) {
        eprintln!("OURS {o:?}");
    }
    (exact, near)
}

fn nm(v: &serde_json::Value) -> Nm {
    v.as_i64().unwrap_or_else(|| v.as_f64().unwrap().round() as Nm)
}

fn pt(v: &serde_json::Value) -> Point {
    Point { x: nm(&v["x"]), y: nm(&v["y"]) }
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
        shove_time_limit_ms: j["shove_time_limit"].as_i64().unwrap() as i32,
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
        Some(Item::Arc(a)) => format!("arc {:?}-{:?} {:?}", a.start(), a.end(), a.net),
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

/// `addedItems` of a recorded commit: `(net, a, b)` per segment.
fn recorded_segments(log: &serde_json::Value) -> Vec<(String, Point, Point)> {
    log["addedItems"].as_array().unwrap().iter().filter(|it| it["kind"] == "segment").map(|it| (it["net"].as_str().unwrap().to_string(), pt(&it["shape"]["start"]), pt(&it["shape"]["end"]))).collect()
}

/// The near-miss count printed beside the exact one, to tell a rounding from a different path (3 um on the micrometre grid).
const TOL: Nm = 10;

fn close(a: Point, b: Point, tol: Nm) -> bool {
    (a.x - b.x).abs() <= tol && (a.y - b.y).abs() <= tol
}

fn same_segment(x: &(String, Point, Point), y: &(String, Point, Point), tol: Nm) -> bool {
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
    let _ = layer;
    // a route started in empty space, on no net (`LINE_PLACER::Start`: `GetOrphanedNetHandle()`), as KiCad's recorded head has `net: ""`
    let mut router = Router::new(&design, &model);
    router.settings = settings;
    router.start(pt(&events[0]["position"]), "F.Cu", width).expect("a route starts in empty space");
    assert!(router.current_net().is_none(), "on no net");
    let mut last = None;
    for e in &events[1..] {
        last = router.preview(pt(&e["position"]));
    }
    let pv = last.unwrap();
    assert!(!pv.colliding);

    // the same tracks are pushed
    let removed = pv.displaced_lines.iter().filter_map(|d| d.source_track.clone()).collect::<std::collections::BTreeSet<_>>().len();
    assert_eq!(removed, log["removedItems"].as_array().unwrap().len(), "KiCad pushed 13 tracks");
    // the pushed lines run where KiCad's do (the optimizer picks different corner cuts in places)
    let ours: Vec<(String, Point, Point)> = pv.displaced_lines.iter().flat_map(|d| d.line.segs().map(|(a, b)| (d.line.net.as_deref().unwrap_or("").to_string(), a, b)).collect::<Vec<_>>()).collect();
    let expected = recorded_segments(&log);
    let (exact, hit) = hits("simple-shove-1", &expected, &ours);
    // 12 of the 28 before springback, when every move shoved from the world as it was; 24 at 3 um on the micrometre grid; 5 exact
    // before `Simplify2`, the hulls without their micrometre-era guard and `collideSimple`'s "extra 1"
    assert_eq!(hit, expected.len(), "{hit} of {} recorded segments reproduced within {TOL} nm", expected.len());
    assert_eq!(exact, expected.len(), "KiCad's compare is exact");
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
    let (exact, _) = hits("backspace1", &expected, &ours);
    assert_eq!(ours.len(), expected.len(), "ours {ours:?} expected {expected:?}");
    assert_eq!(exact, expected.len(), "KiCad's compare is exact: ours {ours:?} expected {expected:?}");
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
    let net = node.nearest_anchor(start, eda_pns::layer::LayerRange::single(1), 500_000, None).and_then(|(id, _)| node.get(id).and_then(|i| i.net().clone())).expect("a track end with a net");
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
    let (exact, hit) = hits("issue22749", &expected, &ours);
    // all of them since springback (4 of the 11 before it)
    assert_eq!(hit, expected.len(), "{hit} of {} recorded segments reproduced within {TOL} nm", expected.len());
    assert_eq!(exact, expected.len(), "KiCad's compare is exact");
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
        DragKind::Corner | DragKind::Segment | DragKind::Arc => {
            w.remove_line_segments(&d.original);
            w.add_line(&Line { arcs: pv.arcs.clone(), ..Line::from_points(d.net.clone(), d.layer, d.width, pv.pts.clone()) }, None, false);
        }
        DragKind::Via => {
            if let Some(id) = d.via_id {
                w.remove(id);
            }
            // where the drag left the via: the cursor, or further, if a shove or a pushout moved it on
            let end = pv.pts.first().copied().unwrap_or(to);
            w.add(Item::Via(eda_pns::item::Via { net: d.net.clone(), layers: d.via_layers, pos: end, diameter: d.via_diameter, drill: d.via_drill, source_via: d.source_via.clone(), locked: false, is_virtual: false }));
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

/// Replays a recorded drag with this port's `Dragger`: no panic, and whenever the drag is accepted the board it leaves is clean.
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
    let id = node.item_at(start, eda_pns::layer::LayerRange::single(layer), 300_000).unwrap_or_else(|| panic!("nothing to drag at {start:?} on {}", layers.name_of(layer)));
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

/// The segments of a drag's commit: `(net, a, b)` per leg of every added track.
fn commit_segments(commit: &eda_pns::dragger::DragCommit) -> Vec<(String, Point, Point)> {
    commit.tracks.iter().flat_map(|l| l.pts.windows(2).map(|w| (l.net.as_deref().unwrap_or("").to_string(), w[0], w[1])).collect::<Vec<_>>()).collect()
}

/// walk_drag_seg_against_board_edge: a 1 mm track on `ultrasound` grabbed in the middle of its 73 mm horizontal segment (a `DM_SEGMENT`
/// drag) and pulled up against the board edge in Shove mode. KiCad slides the segment as far as the copper-to-edge clearance allows
/// (the track's centre 1 mm in from the edge: y = 51.0, where it was at 51.2), the two 45-degree legs either side giving way -- the three
/// segments it recorded -- and the cursor going on to y = 42.2 beyond the edge changes nothing, so the release commits that.
#[test]
fn walk_drag_seg_against_board_edge_slides_the_segment_to_where_kicad_stopped_it() {
    let Some(root) = qa_root() else {
        eprintln!("KiCad QA corpus not found: skipping");
        return;
    };
    let (design, model, log, settings) = route_events(&root, "walk_drag_seg_against_board_edge", "ultrasound");
    let (node, layers) = build_node(&design, &model);
    let events = log["events"].as_array().unwrap();
    let start = pt(&events[0]["position"]);
    let layer = log["headItems"].as_array().and_then(|h| h.first()).map(|h| h["layers"][0].as_i64().unwrap() as i32).unwrap();
    let id = node.item_at(start, eda_pns::layer::LayerRange::single(layer), 300_000).unwrap_or_else(|| panic!("nothing to drag at {start:?} on {}", layers.name_of(layer)));
    let mut dragger = Dragger::start_with(&node, start, id, false).expect("a track");
    assert_eq!(dragger.kind, DragKind::Segment, "grabbed in the middle of the segment");
    let mut last_valid = None;
    for e in &events[1..] {
        let pv = dragger.drag(&node, &model.board, &settings, pt(&e["position"]));
        if !pv.restored && !pv.colliding {
            last_valid = Some(pv.pts.clone());
        }
        assert!(!pv.colliding, "the shove walks the dragged segment along the edge, it never collides: {:?}", e["position"]);
    }
    let end = pt(&events.last().unwrap()["position"]);
    let commit = dragger.finish(&node, &model.board, &settings, end, false).expect("the drag commits");
    assert_eq!(commit.tracks.len(), 1, "one dragged line");
    assert_eq!(Some(commit.tracks[0].pts.clone()), last_valid);
    let expected = recorded_segments(&log);
    assert_eq!(expected.len(), 3, "KiCad recorded the segment and its two legs");
    let ours = commit_segments(&commit);
    let (exact, _) = hits("walk_drag_seg_against_board_edge", &expected, &ours);
    assert_eq!(exact, expected.len(), "KiCad's compare is exact");
    assert_eq!(ours.len(), expected.len(), "ours {ours:?}\nexpected {expected:?}");
    for e in &expected {
        assert!(ours.iter().any(|o| same_segment(e, o, TOL)), "recorded segment {e:?} missing from {ours:?}");
    }
    assert_eq!(commit.remove_track_ids.len(), log["removedItems"].as_array().unwrap().len(), "the same three tracks are replaced");
}

#[test]
fn walk_drag_seg_against_board_edge_keeps_the_board_clean() {
    let (accepted, shoved) = replay_drag("walk_drag_seg_against_board_edge", "ultrasound");
    eprintln!("accepted {accepted} previews, {shoved} of them shoved something");
}

/// simple-drag-shove-singlelayer: a 14 mm segment of `video-v10` grabbed in the middle (a `DM_SEGMENT` drag, Shove mode) and slid 0.9 mm
/// across a busy part of the board. KiCad's commit replaced 139 tracks with 134. The same segment is slid, the tracks in its way
/// pushed, and what this port commits reproduces most of what KiCad recorded (the optimizer picks other corner cuts in places).
/// Slow tier: `video-v10.kicad_pcb` is 6 MB.
#[test]
fn simple_drag_shove_singlelayer_reproduces_what_kicad_recorded() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped (set EDA_SLOW_TESTS=1): video-v10.kicad_pcb takes about a minute in a debug build");
        return;
    }
    let Some(root) = qa_root() else {
        eprintln!("KiCad QA corpus not found: skipping");
        return;
    };
    let (design, model, log, settings) = route_events(&root, "simple-drag-shove-singlelayer", "video-v10");
    let (node, _) = build_node(&design, &model);
    let events = log["events"].as_array().unwrap();
    let start = pt(&events[0]["position"]);
    let layer = log["headItems"].as_array().and_then(|h| h.first()).map(|h| h["layers"][0].as_i64().unwrap() as i32).unwrap();
    let id = node.item_at(start, eda_pns::layer::LayerRange::single(layer), 300_000).expect("the dragged track");
    let mut dragger = Dragger::start_with(&node, start, id, false).expect("a track");
    assert_eq!(dragger.kind, DragKind::Segment);
    let mut restored = 0;
    for e in &events[1..] {
        restored += usize::from(dragger.drag(&node, &model.board, &settings, pt(&e["position"])).restored);
    }
    let end = pt(&events.last().unwrap()["position"]);
    let commit = dragger.finish(&node, &model.board, &settings, end, false).expect("the drag commits");
    let expected = recorded_segments(&log);
    let ours = commit_segments(&commit);
    let (exact, hit) = hits("simple-drag-shove-singlelayer", &expected, &ours);
    eprintln!("{} of {} recorded segments reproduced; ours {} segments; {} tracks removed (KiCad {}); {restored} positions restored", hit, expected.len(), ours.len(), commit.remove_track_ids.len(), log["removedItems"].as_array().unwrap().len());
    assert_eq!(hit, expected.len(), "{hit} of {} recorded segments reproduced within {TOL} nm", expected.len());
    assert_eq!(exact, expected.len(), "KiCad's compare is exact");
    assert_eq!(commit.remove_track_ids.len(), log["removedItems"].as_array().unwrap().len(), "the same tracks are replaced");
}

/// walk-with-teardrops (8303b2ad log `pns-no-hug-2.log`, board `pns-no-hug-2.dump`, Walk around, `optimize_dragged_track` off): the
/// B.Cu segment of `/Progger/SWCLK` at x = 59.75 (y 55..60) is grabbed in the middle and pulled left in 17 small moves, 0.48 mm in
/// all. KiCad's recorded commit slides it to x = 59.27, where the 45-degree legs either side shrink it to a 184 um stub, and the jog
/// below it to x = 59.673 is gone: 19 segments. On the micrometre grid this drag committed a 212 mm detour round the inside of the
/// board outline (findings p5 P-1); on the nanometre grid without `Simplify2` the walk left 1 nm kinks along the line (59 segments).
/// Every position keeps the line about as long as it was, and the commit is KiCad's to the nanometre.
#[test]
fn walk_with_teardrops_drag_does_not_detour_round_the_board() {
    let Some(root) = qa_root() else {
        eprintln!("KiCad QA corpus not found: skipping");
        return;
    };
    let dir = root.join("walk-with-teardrops");
    let text = std::fs::read_to_string(dir.join("pns-no-hug-2.dump")).unwrap();
    let (mut design, mut model, _) = eda_kicad::import_kicad_pcb(&text).unwrap_or_else(|e| panic!("import: {e:?}"));
    let pro = std::fs::read_to_string(dir.join("pns-no-hug-2.kicad_pro")).unwrap();
    eda_kicad::merge_project_net_classes(&mut model, &pro);
    eda_kicad::merge_project_design_rules(&mut model, &pro);
    design.assign_missing_ids();
    let log: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("pns-no-hug-2.log")).unwrap()).unwrap();
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("pns-no-hug-2.settings")).unwrap()).unwrap();
    assert_eq!(j["mode"], 2, "Walk around");
    let settings = RoutingSettings { mode: Mode::Walkaround, optimize_entire_dragged_track: j["optimize_dragged_track"].as_bool().unwrap(), walkaround_iteration_limit: j["walkaround_iteration_limit"].as_i64().unwrap() as i32, ..RoutingSettings::default() };
    let (node, layers) = build_node(&design, &model);
    let events = log["events"].as_array().unwrap();
    let start = pt(&events[0]["position"]);
    let layer = layers.index_of("B.Cu").unwrap();
    let id = node.item_at(start, eda_pns::layer::LayerRange::single(layer), 300_000).expect("the SWCLK segment");
    let mut dragger = Dragger::start_with(&node, start, id, false).expect("a track");
    assert_eq!(dragger.kind, DragKind::Segment, "grabbed in the middle of the segment");
    let length = |pts: &[Point]| pts.windows(2).map(|w| (((w[1].x - w[0].x) as f64).powi(2) + ((w[1].y - w[0].y) as f64).powi(2)).sqrt()).sum::<f64>();
    let original = length(&dragger.original.pts);
    for e in &events[1..] {
        let to = pt(&e["position"]);
        let pv = dragger.drag(&node, &model.board, &settings, to);
        assert!(length(&pv.pts) < 2.0 * original, "dragging to {to:?} gives a {:.1} mm line for a {:.1} mm one", length(&pv.pts) / 1e6, original / 1e6);
    }
    let end = pt(&events.last().unwrap()["position"]);
    let commit = dragger.finish(&node, &model.board, &settings, end, false).expect("the drag commits");
    let expected = recorded_segments(&log);
    let ours = commit_segments(&commit);
    let (exact, hit) = hits("walk-with-teardrops", &expected, &ours);
    assert_eq!(hit, expected.len(), "{hit} of {} recorded segments reproduced within {TOL} nm", expected.len());
    assert_eq!(exact, expected.len(), "KiCad's compare is exact");
}

/// xorshift64: the same sequence every run.
struct Rng(u64);

impl Rng {
    fn below(&mut self, m: i64) -> i64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % (m as u64)) as i64
    }
}

/// Routes from random pads to random points up to 12 mm away in Shove mode, on four real boards:
/// whatever the router accepts leaves the board with no clearance violation it did not have.
#[test]
fn random_shove_routes_never_leave_a_new_violation() {
    let Some(root) = qa_root() else {
        eprintln!("KiCad QA corpus not found: skipping");
        return;
    };
    for board in ["simple", "pic_programmer", "backspace1", "dp_test"] {
        let (design, model) = load(&root, board, "simple-shove-1");
        let (node, _) = build_node(&design, &model);
        let mut router = Router::new(&design, &model);
        router.settings = RoutingSettings { mode: Mode::Shove, smart_pads: false, ..RoutingSettings::default() };
        let mut anchors: Vec<(Point, String)> = node
            .iter()
            .filter_map(|(_, it)| match it {
                Item::Solid(s) if s.net.is_some() && s.net != eda_pns::item::not_a_net() && s.layers.overlaps_layer(0) => Some((s.pos, s.net.as_deref().unwrap().to_string())),
                _ => None,
            })
            .collect();
        anchors.sort_by_key(|(p, n)| (p.x, p.y, n.clone()));
        let mut rng = Rng(0x2545F4914F6CDD1D);
        let (mut accepted, mut shoved) = (0, 0);
        for k in 0..50 {
            let (start, _) = anchors[rng.below(anchors.len() as i64) as usize].clone();
            let target = Point { x: start.x + rng.below(24_000) * 1000 - 12_000_000, y: start.y + rng.below(24_000) * 1000 - 12_000_000 };
            router.start(start, "F.Cu", 250_000).expect("starts on a pad");
            let pv = router.preview(target).unwrap();
            if pv.colliding {
                continue;
            }
            accepted += 1;
            shoved += usize::from(!pv.displaced_lines.is_empty() || !pv.displaced_vias.is_empty());
            let new_ones = new_item_violations(&node, &committed(&node, &pv), &model.board);
            assert!(new_ones.is_empty(), "{board} #{k}: {start:?} -> {target:?} leaves {} new violations, e.g. {:?}", new_ones.len(), &new_ones[..new_ones.len().min(2)]);
        }
        eprintln!("{board}: {accepted} of 50 routes accepted, {shoved} of them shoved something");
    }
}

/// The same for dragging a track's end: a drag the router accepts leaves no new violation.
#[test]
fn random_corner_drags_never_leave_a_new_violation() {
    let Some(root) = qa_root() else {
        eprintln!("KiCad QA corpus not found: skipping");
        return;
    };
    for board in ["simple", "pic_programmer"] {
        let (design, model) = load(&root, board, "simple-shove-1");
        let (node, _) = build_node(&design, &model);
        let settings = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let mut tracks: Vec<(eda_pns::item::ItemId, Point)> = node
            .iter()
            .filter_map(|(id, it)| match it {
                Item::Segment(s) => Some((id, Point { x: (s.a.x + s.b.x) / 2, y: (s.a.y + s.b.y) / 2 })),
                _ => None,
            })
            .collect();
        tracks.sort();
        let mut rng = Rng(0x9E3779B97F4A7C15);
        let (mut accepted, mut shoved) = (0, 0);
        for k in 0..50 {
            let (id, at) = tracks[rng.below(tracks.len() as i64) as usize];
            let Some(d) = Dragger::start_with(&node, at, id, false) else { continue };
            let to = Point { x: at.x + rng.below(6_000) * 1000 - 3_000_000, y: at.y + rng.below(6_000) * 1000 - 3_000_000 };
            let pv = d.preview(&node, &model.board, &settings, to);
            if pv.colliding {
                continue;
            }
            accepted += 1;
            shoved += usize::from(!pv.displaced_lines.is_empty() || !pv.displaced_vias.is_empty());
            let new_ones = new_item_violations(&node, &dragged(&node, &d, &pv, to), &model.board);
            assert!(new_ones.is_empty(), "{board} #{k}: dragging {at:?} to {to:?} leaves {} new violations, e.g. {:?}", new_ones.len(), &new_ones[..new_ones.len().min(2)]);
        }
        eprintln!("{board}: {accepted} of 50 drags accepted, {shoved} of them shoved something");
    }
}

/// P5a: segments slid and corners dragged at random, in Walk around and Shove, through the stateful `Dragger::drag` (a position that cannot
/// be resolved shows the last valid one): whatever the drag shows and commits leaves no clearance violation it did not have. Half of the
/// grabs are in the middle of a segment (`DM_SEGMENT`), half near an end (`DM_CORNER`).
#[test]
fn random_segment_and_corner_drags_never_leave_a_new_violation() {
    let Some(root) = qa_root() else {
        eprintln!("KiCad QA corpus not found: skipping");
        return;
    };
    for board in ["simple", "pic_programmer"] {
        let (design, model) = load(&root, board, "simple-shove-1");
        let (node, _) = build_node(&design, &model);
        let mut segs: Vec<(eda_pns::item::ItemId, Point, Point)> = node.iter().filter_map(|(id, it)| if let Item::Segment(s) = it { Some((id, s.a, s.b)) } else { None }).collect();
        segs.sort_by_key(|&(id, _, _)| id);
        for mode in [Mode::Shove, Mode::Walkaround] {
            let settings = RoutingSettings { mode, ..RoutingSettings::default() };
            let mut rng = Rng(0xA24BAED4963EE407);
            let (mut segment_drags, mut corner_drags, mut accepted, mut restored) = (0, 0, 0, 0);
            for k in 0..120 {
                let (id, a, b) = segs[rng.below(segs.len() as i64) as usize];
                let near_end = k % 2 == 1;
                let at = if near_end { a } else { Point { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 } };
                let Some(mut d) = Dragger::start_with(&node, at, id, false) else { continue };
                match d.kind {
                    DragKind::Segment => segment_drags += 1,
                    DragKind::Corner => corner_drags += 1,
                    DragKind::Via | DragKind::Arc => continue,
                }
                // the priming move at the grab point, then a few cursor positions
                let mut shown = d.drag(&node, &model.board, &settings, at);
                for _ in 0..3 {
                    let to = Point { x: at.x + rng.below(5_000) * 1000 - 2_500_000, y: at.y + rng.below(5_000) * 1000 - 2_500_000 };
                    shown = d.drag(&node, &model.board, &settings, to);
                    restored += usize::from(shown.restored);
                }
                if shown.colliding {
                    continue;
                }
                accepted += 1;
                let new_ones = new_item_violations(&node, &dragged(&node, &d, &shown, at), &model.board);
                assert!(new_ones.is_empty(), "{board} {mode:?} #{k}: dragging {at:?} ({:?}) leaves {} new violations, e.g. {:?}", d.kind, new_ones.len(), &new_ones[..new_ones.len().min(2)]);
            }
            eprintln!("{board} {mode:?}: {segment_drags} segment and {corner_drags} corner drags, {accepted} shown legal, {restored} positions restored");
        }
    }
}

/// D7: dragging a via. Before it, the dragged via was never a pusher, so a via dropped onto a track left a violation (6 of 200
/// random drags on `simple`); now it is shoved, or pushed off what it lands on, or the drag says it collides. Whichever mode,
/// a drag the router accepts leaves no new violation, the board outline included.
#[test]
fn random_via_drags_never_leave_a_new_violation() {
    let Some(root) = qa_root() else {
        eprintln!("KiCad QA corpus not found: skipping");
        return;
    };
    for board in ["simple", "pic_programmer"] {
        let (design, model) = load(&root, board, "simple-shove-1");
        let (node, _) = build_node(&design, &model);
        let mut vias: Vec<(eda_pns::item::ItemId, Point)> = node.iter().filter_map(|(id, it)| if let Item::Via(v) = it { Some((id, v.pos)) } else { None }).collect();
        vias.sort();
        for mode in [Mode::Shove, Mode::Walkaround] {
            let settings = RoutingSettings { mode, ..RoutingSettings::default() };
            let mut rng = Rng(0xD1B54A32D192ED03);
            let (mut accepted, mut moved) = (0, 0);
            for k in 0..60 {
                let (id, at) = vias[rng.below(vias.len() as i64) as usize];
                let Some(d) = Dragger::start_with(&node, at, id, false) else { continue };
                let to = Point { x: at.x + rng.below(4_000) * 1000 - 2_000_000, y: at.y + rng.below(4_000) * 1000 - 2_000_000 };
                let pv = d.preview(&node, &model.board, &settings, to);
                if pv.colliding {
                    continue;
                }
                accepted += 1;
                moved += usize::from(!pv.displaced_lines.is_empty() || !pv.displaced_vias.is_empty() || pv.pts[0] != to);
                let new_ones = new_item_violations(&node, &dragged(&node, &d, &pv, to), &model.board);
                assert!(new_ones.is_empty(), "{board} {mode:?} #{k}: dragging the via at {at:?} to {to:?} leaves {} new violations, e.g. {:?}", new_ones.len(), &new_ones[..new_ones.len().min(2)]);
            }
            eprintln!("{board} {mode:?}: {accepted} of 60 via drags accepted, {moved} of them shoved or pushed something");
        }
    }
}
