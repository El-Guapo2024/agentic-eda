//! Arc tracks on KiCad's own boards (`qa/data/pcbnew`): the two of its corpus with arcs among their tracks (`issue12609`: 47 arcs, `issue8407`:
//! 40; the router regressions' boards have none). The world holds each as one `ARC_T` item; a route, a shove, a drag of a segment or a corner next
//! to an arc and a drag of an arc itself leave the board with no clearance violation it did not have -- judged on the board as the commit writes it
//! to the IR, so the arcs a commit puts back are arcs (`Track::arc`) in it.
//!
//! Skipped (prints a line, passes) when the QA corpus is not on this machine: `KICAD_QA_DATA`, default `~/ws/kicad-src-8303b2ad/qa/data`.

use eda_model::units::{mm, um};
use eda_model::ir::{Design, Point, Track};
use eda_model::ConstraintModel;
use eda_pns::dragger::{DragKind, Dragger};
use eda_pns::from_ir::build_node;
use eda_pns::item::{net_of, Item, ItemId};
use eda_pns::layer::LayerMap;
use eda_pns::node::Node;
use eda_pns::router::{RouteCommit, Router};
use eda_pns::settings::{Mode, RoutingSettings};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

fn qa_data() -> Option<PathBuf> {
    let p = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data"));
    p.join("pcbnew").exists().then_some(p.join("pcbnew"))
}

fn load(root: &std::path::Path, board: &str) -> (Design, ConstraintModel) {
    let text = std::fs::read_to_string(root.join(format!("{board}.kicad_pcb"))).unwrap();
    let (mut design, model, _) = eda_kicad::import_kicad_pcb(&text).unwrap_or_else(|e| panic!("import {board}: {e:?}"));
    design.assign_missing_ids();
    (design, model)
}

fn arc_tracks(design: &Design) -> Vec<&Track> {
    design.routing.iter().flat_map(|r| r.tracks.iter()).filter(|t| t.arc().is_some()).collect()
}

/// The pairs of items of the world the router's own collision test calls a violation, counted by the nets and kinds of the two (KiCad's boards are not
/// clean to start with, and an item moved by a micrometre is the same violation, not a new one).
fn violations(node: &Node, rules: &eda_model::BoardRules) -> BTreeMap<(String, String), usize> {
    let name = |id: ItemId| match node.get(id) {
        Some(Item::Segment(s)) => format!("seg {:?}", s.net),
        Some(Item::Arc(a)) => format!("arc {:?}", a.net),
        Some(Item::Via(v)) => format!("via {:?}", v.net),
        Some(Item::Solid(s)) => format!("pad {}", s.source),
        None => String::new(),
    };
    let mut seen: HashSet<(ItemId, ItemId)> = HashSet::new();
    let mut out: BTreeMap<(String, String), usize> = BTreeMap::new();
    for (id, item) in node.iter() {
        for o in node.all_colliding(&item.shape(item.layers().start()), item.net(), item.layers(), rules, &[id]) {
            if !seen.insert((id.min(o.id), id.max(o.id))) {
                continue;
            }
            let (a, b) = (name(id), name(o.id));
            *out.entry(if a < b { (a, b) } else { (b, a) }).or_insert(0) += 1;
        }
    }
    out
}

/// The violations `after` has more of than `before`.
fn new_violations(before: &BTreeMap<(String, String), usize>, after: &BTreeMap<(String, String), usize>) -> Vec<((String, String), usize)> {
    after.iter().filter(|(k, n)| **n > before.get(*k).copied().unwrap_or(0)).map(|(k, n)| (k.clone(), *n - before.get(k).copied().unwrap_or(0))).collect()
}

/// The world after a commit, as the IR would have it built again (`from_ir`): the tracks and vias it removes gone, the ones it adds in -- an arc track as one
/// `Arc`, a polyline as its segments. (Rebuilding the whole node from the design takes seconds on these boards; this is the same without that.)
fn world_after(node: &Node, layers: &LayerMap, commit: &RouteCommit) -> Node {
    let mut w = node.branch();
    let doomed: Vec<ItemId> = w
        .iter()
        .filter(|(_, it)| it.source_track_id().is_some_and(|t| commit.remove_track_ids.iter().any(|r| r == t)) || matches!(it, Item::Via(v) if v.source_via.as_ref().is_some_and(|s| commit.remove_via_ids.contains(s))))
        .map(|(id, _)| id)
        .collect();
    for id in doomed {
        w.remove(id);
    }
    for t in &commit.tracks {
        let layer = layers.index_of(&t.layer).expect("a layer of the board");
        let net = net_of(&t.net);
        if let Some((s, m, e)) = t.arc() {
            w.add(Item::Arc(eda_pns::item::Arc::new(net, layer, t.width, eda_pns::arcs::ArcPts::new(s, m, e), None, false)));
        } else {
            for (i, win) in t.pts.windows(2).enumerate() {
                if win[0] != win[1] {
                    w.add(Item::Segment(eda_pns::item::Segment { net: net.clone(), layer, a: win[0], b: win[1], width: t.width, source_track: Some((format!("new_{}", t.pts.len()), i)), locked: false }));
                }
            }
        }
    }
    for v in &commit.vias {
        let range = layers.range_of(&v.from_layer, &v.to_layer);
        w.add(Item::Via(eda_pns::item::Via { net: net_of(&v.net), layers: range, pos: v.at, diameter: v.diameter, drill: v.drill, source_via: None, locked: false, is_virtual: false }));
    }
    w
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

const ALL_BOARDS: [&str; 2] = ["issue12609", "issue8407"];

/// `EDA_SLOW_TESTS=1`: the long sweeps (40 tries a board and mode, lines of any length); otherwise a short one that still touches both boards.
fn slow() -> bool {
    std::env::var_os("EDA_SLOW_TESTS").is_some_and(|v| v != "0")
}

fn tries() -> i64 {
    if slow() {
        40
    } else {
        6
    }
}

/// The longest line (in vertices: an arc is a hundred of them) the quick sweep drags: a debug build walks one of the boards' 260-vertex lines around
/// a hull in seconds, and the long sweep is there for those.
fn longest_line() -> usize {
    if slow() {
        usize::MAX
    } else {
        90
    }
}

/// The boards the sweeps run on: both, or the ones `ARC_BOARDS` (comma separated) names.
fn boards() -> Vec<&'static str> {
    let only = std::env::var("ARC_BOARDS").unwrap_or_default();
    ALL_BOARDS.iter().copied().filter(|b| only.is_empty() || only.split(',').any(|o| o == *b)).collect()
}

/// A dragger's commit as the `RouteCommit` the router turns it into (`Router::drag_finish_with`): the vias a shove moved are put back with their own size.
fn drag_commit(design: &Design, layers: &LayerMap, commit: &eda_pns::dragger::DragCommit) -> RouteCommit {
    let mut out = RouteCommit { remove_track_ids: commit.remove_track_ids.clone(), remove_via_ids: commit.remove_via_ids.clone(), ..RouteCommit::default() };
    for line in &commit.tracks {
        out.tracks.extend(eda_pns::router::line_to_tracks(line, layers.name_of(line.layer), None));
    }
    for (pos, _, _, src) in &commit.vias {
        if let Some(v) = src.as_ref().and_then(|s| design.routing.as_ref().and_then(|r| r.vias.iter().find(|v| &v.id == s))) {
            out.vias.push(eda_model::ir::Via { id: String::new(), net: v.net.clone(), at: *pos, drill: v.drill, diameter: v.diameter, from_layer: v.from_layer.clone(), to_layer: v.to_layer.clone() });
        }
    }
    out
}

#[test]
fn the_world_holds_each_arc_track_of_kicads_boards_as_one_arc_and_a_line_through_it_finds_it_again() {
    let Some(root) = qa_data() else {
        eprintln!("KiCad QA corpus not found: skipping");
        return;
    };
    for board in boards() {
        let (design, model) = load(&root, board);
        let arcs = arc_tracks(&design);
        assert!(arcs.len() > 20, "{board} has arc tracks: {}", arcs.len());
        let (node, _) = build_node(&design, &model);
        let items: Vec<(ItemId, &eda_pns::item::Arc)> = node.iter().filter_map(|(id, it)| if let Item::Arc(a) = it { Some((id, a)) } else { None }).collect();
        // a full circle stays chords, anything else is one item
        let circles = arcs.iter().filter(|t| t.arc().is_some_and(|(s, _, e)| s == e)).count();
        assert_eq!(items.len(), arcs.len() - circles, "{board}: one Arc per arc track");
        for (id, a) in &items {
            let track = arcs.iter().find(|t| Some(t.id.as_str()) == a.source_track.as_deref()).expect("its track");
            assert_eq!(track.arc(), Some((a.start(), a.arc.mid, a.end())));
            let line = node.assemble_line(*id).expect("assembles");
            assert!(line.arc_count() >= 1, "{board}: the line through {:?} has the arc", a.arc);
            assert!(line.segment_ids.iter().filter(|&&s| s == *id).count() >= 1);
        }
        eprintln!("{board}: {} arc items, {} segments", items.len(), node.iter().filter(|(_, it)| matches!(it, Item::Segment(_))).count());
    }
}

/// Routes from random pads to random points up to 12 mm away (half of them to a point near an arc) in Shove mode, on boards with arcs: whatever the router
/// accepts, committed, leaves the board with no clearance violation it did not have.
#[test]
fn random_shove_routes_on_boards_with_arcs_never_leave_a_new_violation() {
    let Some(root) = qa_data() else {
        eprintln!("KiCad QA corpus not found: skipping");
        return;
    };
    for board in boards() {
        let (design, model) = load(&root, board);
        let (node, layers) = build_node(&design, &model);
        let before = violations(&node, &model.board);
        let mut anchors: Vec<(Point, String)> = node
            .iter()
            .filter_map(|(_, it)| match it {
                Item::Solid(s) if s.net.is_some() && s.layers.overlaps_layer(0) && !s.edge => Some((s.pos, s.net.as_deref().unwrap().to_string())),
                _ => None,
            })
            .collect();
        anchors.sort_by_key(|(p, n)| (p.x, p.y, n.clone()));
        let arcs: Vec<(Point, Point, Point)> = arc_tracks(&design).iter().filter_map(|t| t.arc()).collect();
        let arc_ids: HashSet<String> = arc_tracks(&design).iter().map(|t| t.id.clone()).collect();
        let mut router = Router::new(&design, &model);
        router.settings = RoutingSettings { mode: Mode::Shove, smart_pads: false, ..RoutingSettings::default() };
        let mut rng = Rng(0x2545F4914F6CDD1D);
        let (mut accepted, mut touched_arcs, mut kept_arcs) = (0, 0, 0);
        for k in 0..tries() {
            let (start, _) = anchors[rng.below(anchors.len() as i64) as usize].clone();
            let target = if k % 2 == 0 {
                let (s, m, e) = arcs[rng.below(arcs.len() as i64) as usize];
                Point { x: (s.x + m.x + e.x) / 3 + um(1.0) * rng.below(1_500) - mm(0.75), y: (s.y + m.y + e.y) / 3 + um(1.0) * rng.below(1_500) - mm(0.75) }
            } else {
                Point { x: start.x + um(1.0) * rng.below(24_000) - mm(12.0), y: start.y + um(1.0) * rng.below(24_000) - mm(12.0) }
            };
            if router.start(start, "F.Cu", mm(0.25)).is_err() {
                continue;
            }
            let Some(pv) = router.preview(target) else { continue };
            if pv.colliding {
                continue;
            }
            let Some(commit) = router.finish(target) else { continue };
            accepted += 1;
            touched_arcs += usize::from(commit.remove_track_ids.iter().any(|t| arc_ids.contains(t)));
            kept_arcs += commit.tracks.iter().filter(|t| t.arc().is_some()).count();
            let after = violations(&world_after(&node, &layers, &commit), &model.board);
            let new_ones = new_violations(&before, &after);
            assert!(new_ones.is_empty(), "{board} #{k}: {start:?} -> {target:?} leaves {} new violations, e.g. {:?}", new_ones.len(), &new_ones[..new_ones.len().min(2)]);
        }
        eprintln!("{board}: {accepted} of {} routes accepted, {touched_arcs} of them replaced an arc track, {kept_arcs} arc tracks written back", tries());
    }
}

/// Arcs dragged at random (`DM_ARC`), in the three modes: whatever the drag shows as legal and commits leaves no new violation, and what it writes
/// back has the arc in it as an arc.
#[test]
fn random_arc_drags_never_leave_a_new_violation() {
    let Some(root) = qa_data() else {
        eprintln!("KiCad QA corpus not found: skipping");
        return;
    };
    for board in boards() {
        let (design, model) = load(&root, board);
        let (node, layers) = build_node(&design, &model);
        let before = violations(&node, &model.board);
        let mut arcs: Vec<(ItemId, Point)> = node.iter().filter_map(|(id, it)| if let Item::Arc(a) = it { Some((id, a.arc.mid)) } else { None }).collect();
        arcs.sort();
        for mode in [Mode::MarkObstacles, Mode::Walkaround, Mode::Shove] {
            let settings = RoutingSettings { mode, ..RoutingSettings::default() };
            let mut rng = Rng(0xA24BAED4963EE407 ^ mode as u64);
            let (mut grabbed, mut refused, mut shown_legal, mut moved) = (0, 0, 0, 0);
            for k in 0..tries() {
                let (id, at) = arcs[rng.below(arcs.len() as i64) as usize];
                let mut d = match Dragger::start_checked(&node, at, id, false) {
                    Ok(d) => d,
                    Err(_) => {
                        refused += 1;
                        continue;
                    }
                };
                assert_eq!(d.kind, DragKind::Arc);
                grabbed += 1;
                let mut last = at;
                let mut shown = d.drag(&node, &model.board, &settings, at);
                for _ in 0..3 {
                    last = Point { x: at.x + um(1.0) * rng.below(3_000) - mm(1.5), y: at.y + um(1.0) * rng.below(3_000) - mm(1.5) };
                    shown = d.drag(&node, &model.board, &settings, last);
                }
                if shown.colliding || shown.restored {
                    continue;
                }
                shown_legal += 1;
                // the release is where the cursor is
                let Some(commit) = d.finish(&node, &model.board, &settings, last, false) else { continue };
                moved += usize::from(shown.pts != d.original.pts);
                let router_commit = drag_commit(&design, &layers, &commit);
                let after = violations(&world_after(&node, &layers, &router_commit), &model.board);
                let new_ones = new_violations(&before, &after);
                assert!(new_ones.is_empty(), "{board} {mode:?} #{k}: dragging the arc at {at:?} to {last:?} leaves {} new violations, e.g. {:?}", new_ones.len(), &new_ones[..new_ones.len().min(2)]);
                // a shove that walked the dragged line round a pad cut into the arc; otherwise it is written back as an arc
                let kept = eda_pns::line::Line { pts: shown.pts.clone(), arcs: shown.arcs.clone(), ..d.original.clone() }.arc_count();
                assert!(kept == 0 || router_commit.tracks.iter().any(|t| t.arc().is_some()), "{board} {mode:?} #{k}: the dragged arc is written back as an arc");
            }
            eprintln!("{board} {mode:?}: {grabbed} arcs grabbed ({refused} refused), {shown_legal} shown legal, {moved} of them moved");
        }
    }
}

/// Segments slid and corners dragged at random on the tracks of lines that have an arc: no new violation, and the arcs the commit writes back
/// are written as arcs (not 32 or 60 little segments).
#[test]
fn dragging_a_segment_or_corner_of_a_line_with_an_arc_keeps_the_arc() {
    let Some(root) = qa_data() else {
        eprintln!("KiCad QA corpus not found: skipping");
        return;
    };
    for board in boards() {
        let (design, model) = load(&root, board);
        let (node, layers) = build_node(&design, &model);
        let before = violations(&node, &model.board);
        // the straight segments that have an arc on the line they are part of
        let mut segs: Vec<(ItemId, Point, Point)> = node
            .iter()
            .filter_map(|(id, it)| if let Item::Segment(s) = it { Some((id, s.a, s.b)) } else { None })
            .filter(|(id, _, _)| node.assemble_line(*id).is_some_and(|l| l.arc_count() > 0 && l.pts.len() <= longest_line()))
            .collect();
        segs.sort_by_key(|&(id, _, _)| id);
        if segs.is_empty() {
            // the quick sweep leaves out the boards whose every line with an arc is a long one
            assert!(!slow(), "{board} has straight tracks next to arcs");
            eprintln!("{board}: every line with an arc is longer than {} vertices; left to the slow sweep", longest_line());
            continue;
        }
        for mode in [Mode::Walkaround, Mode::Shove] {
            let settings = RoutingSettings { mode, ..RoutingSettings::default() };
            let mut rng = Rng(0x9E3779B97F4A7C15 ^ mode as u64);
            let (mut accepted, mut with_arc, mut with_arc_in_line) = (0, 0, 0);
            for k in 0..tries() {
                let (id, a, b) = segs[rng.below(segs.len() as i64) as usize];
                let at = if k % 2 == 1 { a } else { Point { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 } };
                if std::env::var("ARC_TRACE").is_ok() {
                    eprintln!("{board} {mode:?} #{k}: {id} at {at:?}");
                }
                let Some(mut d) = Dragger::start_with(&node, at, id, false) else { continue };
                if std::env::var("ARC_TRACE").is_ok() {
                    eprintln!("    line of {} points, {} arcs", d.original.pts.len(), d.original.arc_count());
                }
                let mut last = at;
                let mut shown = d.drag(&node, &model.board, &settings, at);
                for _ in 0..3 {
                    last = Point { x: at.x + um(1.0) * rng.below(2_500) - mm(1.25), y: at.y + um(1.0) * rng.below(2_500) - mm(1.25) };
                    if std::env::var("ARC_TRACE").is_ok() {
                        eprintln!("    to {last:?}");
                    }
                    shown = d.drag(&node, &model.board, &settings, last);
                }
                if shown.colliding || shown.restored {
                    continue;
                }
                let Some(commit) = d.finish(&node, &model.board, &settings, last, false) else { continue };
                accepted += 1;
                let router_commit = drag_commit(&design, &layers, &commit);
                with_arc_in_line += usize::from(d.original.arc_count() > 0);
                with_arc += usize::from(router_commit.tracks.iter().any(|t| t.arc().is_some()));
                let after = violations(&world_after(&node, &layers, &router_commit), &model.board);
                let new_ones = new_violations(&before, &after);
                assert!(new_ones.is_empty(), "{board} {mode:?} #{k}: dragging {at:?} ({:?}) to {last:?} leaves {} new violations, e.g. {:?}", d.kind, new_ones.len(), &new_ones[..new_ones.len().min(2)]);
            }
            eprintln!("{board} {mode:?}: {accepted} drags accepted, {with_arc_in_line} on lines with an arc, {with_arc} of them write an arc back");
            assert!(with_arc * 2 >= with_arc_in_line, "{board} {mode:?}: most of the drags keep the arc of their line ({with_arc} of {with_arc_in_line})");
        }
    }
}
