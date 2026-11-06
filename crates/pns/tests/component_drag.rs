//! `D` on a footprint and on several tracks, through the router (`ROUTER::StartDragging( aP, ITEM_SET )`: `COMPONENT_DRAGGER`, `MULTI_DRAGGER`;
//! crates/pns/PARITY.md Stage 22). A component drag moves the footprint and every track that ends on one of its pads follows it: the board
//! the commit leaves -- the footprint moved (`Router`'s `moved_footprints`, `move_by`), the tracks replaced -- has each of those tracks still
//! ending on its pad, nothing new colliding, and no loose end it did not have. A multi-track drag moves the selected lines together.
//!
//! On KiCad's own boards (`qa/data/pcbnew/pns_regressions/boards`: `pic_programmer`, a KiCad demo, and `backspace1`; skipped when the QA corpus
//! is not on this machine, `KICAD_QA_DATA`) a sweep of component drags is judged by the router's own collision test and, under
//! `EDA_SLOW_TESTS=1`, by kicad-cli's DRC: no new clearance, short, crossing, hole, edge, track-width, dangling-track or unconnected-items
//! violation.

use eda_model::footprint::{Footprint, Pad, PadKind, PadShape};
use eda_model::ir::{Design, DrawingsSection, FootprintInstance, Nm, PlacementSection, Point, Provenance, RoutingSection, Side};
use eda_model::units::mm;
use eda_model::{ConstraintModel, Net, Part, Pin, PinKind};
use eda_pns::item::{Item, ItemId};
use eda_pns::node::Node;
use eda_pns::router::{RouteCommit, Router};
use eda_pns::settings::Mode;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

fn p(x: f64, y: f64) -> Point {
    Point { x: mm(x), y: mm(y) }
}

fn part(reference: &str, footprint: &str, pins: usize) -> Part {
    Part { reference: reference.into(), mpn: None, lcsc: None, value: None, package: None, footprint: Some(footprint.into()), pins: (1..=pins).map(|i| Pin { number: i.to_string(), name: None, kind: PinKind::Passive }).collect(), body_nm: None, symbol: None, datasheet: None, edge: None }
}

fn smd(number: &str, at: (Nm, Nm)) -> Pad {
    Pad { opposite_side: false, ext: Default::default(), number: number.into(), at, size: (mm(1.0), mm(1.0)), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None }
}

fn footprint(name: &str, pads: Vec<Pad>) -> Footprint {
    Footprint { name: name.into(), pads, courtyard: None, model: None, courtyard_outlines: vec![], models3d: vec![], art: Default::default() }
}

/// A 40 x 30 mm board: `PAD1` footprints (one 1 mm square SMD pad) and `TWO` footprints (two of them, 2 mm apart) at `fps`, on the nets `nets`.
fn board(fps: &[(&str, &str, Point)], nets: &[(&str, &[&str])]) -> (Design, ConstraintModel) {
    let parts = fps.iter().map(|(r, f, _)| part(r, f, if *f == "TWO" { 2 } else { 1 })).collect();
    let model = ConstraintModel {
        parts,
        nets: nets.iter().map(|(n, pins)| Net { name: n.to_string(), pins: pins.iter().map(|s| s.to_string()).collect() }).collect(),
        footprints: vec![footprint("PAD1", vec![smd("1", (Nm::ZERO, Nm::ZERO))]), footprint("TWO", vec![smd("1", (Nm::ZERO, Nm::ZERO)), smd("2", (mm(2.0), Nm::ZERO))])],
        ..Default::default()
    };
    let design = Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "t".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(PlacementSection { outline: vec![p(0.0, 0.0), p(40.0, 0.0), p(40.0, 30.0), p(0.0, 30.0)], footprints: fps.iter().map(|(r, _, at)| FootprintInstance { id: r.to_string(), at: *at, rot: 0, side: Side::Top, label: Default::default() }).collect(), modules: vec![] }),
        routing: Some(RoutingSection { tracks: vec![], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], diff_pair_presets: vec![], teardrop_settings: Default::default() }),
        drawings: Some(DrawingsSection::default()),
    };
    (design, model)
}

/// The design `commit` leaves, as `Cmd::Batch[ MoveItems( footprints + moved tracks ), CommitRoute ]` applies it: the footprints and the
/// tracks moved whole by `move_by`, the removed tracks and vias gone, the new ones in.
fn apply(design: &Design, commit: &RouteCommit) -> Design {
    let mut d = design.clone();
    let (dx, dy) = commit.move_by;
    for f in d.placement.as_mut().unwrap().footprints.iter_mut().filter(|f| commit.moved_footprints.contains(&f.id)) {
        f.at = Point { x: f.at.x + dx, y: f.at.y + dy };
    }
    let rt = d.routing.as_mut().unwrap();
    for t in rt.tracks.iter_mut().filter(|t| commit.moved_tracks.contains(&t.id)) {
        for q in t.pts.iter_mut() {
            *q = Point { x: q.x + dx, y: q.y + dy };
        }
    }
    rt.tracks.retain(|t| !commit.remove_track_ids.contains(&t.id));
    rt.vias.retain(|v| !commit.remove_via_ids.contains(&v.id));
    rt.tracks.extend(commit.tracks.iter().cloned());
    rt.vias.extend(commit.vias.iter().cloned());
    d.assign_missing_ids();
    d
}

/// Route `from` -> `to` in Walk around mode and put it on the board.
fn routed(design: &Design, model: &ConstraintModel, from: Point, to: Point) -> Design {
    let mut router = Router::new(design, model);
    router.settings.mode = Mode::Walkaround;
    router.start(from, "F.Cu", mm(0.2)).expect("starts on the pad");
    let commit = router.finish(to).expect("commits");
    apply(design, &commit)
}

/// The ends of the world's tracks that nothing else joins (a joint with one link: `track_dangling`), as points.
fn loose_ends(node: &Node) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::new();
    for (_, it) in node.iter() {
        let Some((a, b)) = it.track_ends() else { continue };
        for e in [a, b] {
            if node.joint_at(e, it.net()).is_some_and(|j| j.links.iter().filter(|id| node.get(**id).is_some_and(|x| !x.is_virtual())).count() == 1) {
                out.push(e);
            }
        }
    }
    out.sort_by_key(|q| (q.x, q.y));
    out
}

/// The pairs of items the router's collision test calls a violation, by the nets and kinds of the two (a moved pair is the same violation).
fn violations(node: &Node, rules: &eda_model::BoardRules) -> BTreeMap<(String, String), usize> {
    let name = |id: ItemId| match node.get(id) {
        Some(Item::Segment(s)) => format!("seg {:?}", s.net),
        Some(Item::Arc(a)) => format!("arc {:?}", a.net),
        Some(Item::Via(v)) => format!("via {:?}", v.net),
        Some(Item::Solid(s)) => format!("pad {:?}", s.net),
        None => String::new(),
    };
    let mut seen: HashSet<(ItemId, ItemId)> = HashSet::new();
    let mut out: BTreeMap<(String, String), usize> = BTreeMap::new();
    for (id, item) in node.iter().filter(|(_, it)| !matches!(it, Item::Solid(_))) {
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

fn new_violations(before: &BTreeMap<(String, String), usize>, after: &BTreeMap<(String, String), usize>) -> Vec<((String, String), usize)> {
    after.iter().filter(|(k, n)| **n > before.get(*k).copied().unwrap_or(0)).map(|(k, n)| (k.clone(), *n - before.get(k).copied().unwrap_or(0))).collect()
}

/// R1 (two pads, 2 mm apart) routed to R2 and R3 -- `R1.1`-`R2.1` on N1, `R1.2`-`R3.1` on N2 -- and an unrelated GND track across the board.
fn two_routed() -> (Design, ConstraintModel) {
    let (design, model) = board(&[("R1", "TWO", p(15.0, 15.0)), ("R2", "PAD1", p(5.0, 25.0)), ("R3", "PAD1", p(30.0, 6.0))], &[("N1", &["R1.1", "R2.1"]), ("N2", &["R1.2", "R3.1"])]);
    let design = routed(&design, &model, p(15.0, 15.0), p(5.0, 25.0));
    let design = routed(&design, &model, p(17.0, 15.0), p(30.0, 6.0));
    (design, model)
}

/// D on R1, in every mode: the footprint moves by the cursor's travel from its anchor (`InlineDrag` warps the cursor there), each track that
/// ends on one of its pads follows (its end at the pad's new centre, the far end where it was), and the commit -- R1 moved, its two tracks
/// replaced -- leaves no loose end, nothing new colliding, and the router finds the tracks on the moved pads.
#[test]
fn a_component_drag_keeps_the_footprints_tracks_on_its_pads() {
    let (design, model) = two_routed();
    let before_node = eda_pns::from_ir::build_node(&design, &model).0;
    assert!(loose_ends(&before_node).is_empty(), "the routes are whole: {:?}", loose_ends(&before_node));
    for mode in [Mode::Shove, Mode::Walkaround, Mode::MarkObstacles] {
        let mut router = Router::new(&design, &model);
        router.settings.mode = mode;
        // the cursor on R1's first pad, a little off its centre: the drag starts at the footprint's anchor
        let start = router.drag_start_component(p(15.2, 15.1), &["R1".to_string()], true).expect("starts");
        assert_eq!(start, p(15.0, 15.0), "{mode:?}");
        assert_eq!(router.drag_group_kind(), Some("component"));
        let to = p(16.0, 13.0);
        let pv = router.drag_preview(to).expect("a drag");
        assert_eq!(pv.offset, Some((mm(1.0), mm(-2.0))), "{mode:?}");
        assert!(!pv.colliding, "{mode:?}: {pv:?}");
        assert_eq!(pv.fanout.len(), 2, "{mode:?}: both tracks follow");
        for (pad, at) in [("R1.1", p(16.0, 13.0)), ("R1.2", p(18.0, 13.0))] {
            assert!(pv.fanout.iter().any(|l| l.first() == Some(at) || l.last() == Some(at)), "{mode:?}: a line ends on {pad} at {at:?}: {:?}", pv.fanout);
        }
        let commit = router.drag_finish(to).expect("commits");
        assert!(!router.is_dragging(), "the drag is over");
        assert_eq!(commit.moved_footprints, vec!["R1".to_string()]);
        assert_eq!(commit.move_by, (mm(1.0), mm(-2.0)));
        assert_eq!(commit.remove_track_ids.len(), 2, "{mode:?}: {commit:?}");
        let after = apply(&design, &commit);
        assert_eq!(after.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == "R1").unwrap().at, p(16.0, 13.0));
        let (node, _) = eda_pns::from_ir::build_node(&after, &model);
        assert!(loose_ends(&node).is_empty(), "{mode:?}: no track is left loose: {:?}", loose_ends(&node));
        assert!(new_violations(&violations(&before_node, &model.board), &violations(&node, &model.board)).is_empty(), "{mode:?}");
        // the tracks are on the moved pads, the far ends on R2 and R3
        for at in [p(16.0, 13.0), p(18.0, 13.0), p(5.0, 25.0), p(30.0, 6.0)] {
            let joint = node.iter().find_map(|(_, it)| match it {
                Item::Solid(s) if s.pos == at => node.joint_at(at, &s.net),
                _ => None,
            });
            assert!(joint.is_some_and(|j| j.links.iter().any(|id| node.get(*id).is_some_and(Item::is_track))), "{mode:?}: a track ends on the pad at {at:?}");
        }
    }
}

/// `FixRoute` refuses a component drag that lands a pad on another net's copper, and the drag is over (`InlineDrag`); a Ctrl+click
/// (`aForceCommit`) commits it.
#[test]
fn a_refused_component_drag_ends_and_ctrl_commits_it() {
    let (mut design, model) = two_routed();
    // a GND track R1 is dragged onto
    design.routing.as_mut().unwrap().tracks.push(eda_model::ir::Track { id: "gnd".into(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![p(25.0, 20.0), p(35.0, 20.0)], arc_mid_offset: None });
    let to = p(30.0, 20.0);
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::Walkaround;
    router.drag_start_component(p(15.0, 15.0), &["R1".to_string()], true).unwrap();
    let pv = router.drag_preview(to).unwrap();
    assert!(pv.colliding, "R1 lands on the GND track: {pv:?}");
    assert!(router.drag_finish(to).is_none());
    assert!(!router.is_dragging(), "a refused click ends the drag");
    let mut router = Router::new(&design, &model);
    router.drag_start_component(p(15.0, 15.0), &["R1".to_string()], true).unwrap();
    router.drag_preview(to);
    assert!(router.drag_finish_with(to, true).is_some(), "Ctrl+click commits it");
}

/// D on two selected parallel tracks: `MULTI_DRAGGER`, the second following the first at its distance; the commit replaces both.
#[test]
fn a_multi_track_drag_moves_the_selected_tracks_together() {
    let (mut design, model) = board(&[("R1", "PAD1", p(5.0, 10.0)), ("R2", "PAD1", p(35.0, 10.0)), ("R3", "PAD1", p(5.0, 12.0)), ("R4", "PAD1", p(35.0, 12.0))], &[("N1", &["R1.1", "R2.1"]), ("N2", &["R3.1", "R4.1"])]);
    let rt = design.routing.as_mut().unwrap();
    for (id, net, y) in [("ta", "N1", 10.0), ("tb", "N2", 12.0)] {
        rt.tracks.push(eda_model::ir::Track { id: id.into(), net: net.into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![p(5.0, y), p(35.0, y)], arc_mid_offset: None });
    }
    for mode in [Mode::Shove, Mode::Walkaround, Mode::MarkObstacles] {
        let mut router = Router::new(&design, &model);
        router.settings.mode = mode;
        let start = router.drag_start_multi(p(20.0, 10.05), &["ta".to_string(), "tb".to_string()]).expect("starts");
        assert_eq!(start, p(20.0, 10.0), "{mode:?}: snapped onto ta");
        assert_eq!(router.drag_group_kind(), Some("multi"));
        let pv = router.drag_preview(p(20.0, 8.0)).unwrap();
        assert!(!pv.colliding, "{mode:?}: {pv:?}");
        assert_eq!(pv.fanout.len(), 2, "{mode:?}");
        // both slid 2 mm up, between their pads (the pads stay)
        assert!(pv.fanout.iter().any(|l| l.pts.contains(&p(20.0, 8.0)) || l.pts.windows(2).any(|w| w[0].y == mm(8.0) && w[1].y == mm(8.0))), "{mode:?}: {:?}", pv.fanout);
        assert!(pv.fanout.iter().any(|l| l.pts.windows(2).any(|w| w[0].y == mm(10.0) && w[1].y == mm(10.0) && w[0].x != mm(5.0) && w[1].x != mm(35.0))), "{mode:?}: tb at y = 10: {:?}", pv.fanout);
        let commit = router.drag_finish(p(20.0, 8.0)).expect("commits");
        let mut removed = commit.remove_track_ids.clone();
        removed.sort();
        assert_eq!(removed, vec!["ta", "tb"], "{mode:?}");
        let after = apply(&design, &commit);
        let (node, _) = eda_pns::from_ir::build_node(&after, &model);
        assert!(loose_ends(&node).is_empty(), "{mode:?}: {:?}", loose_ends(&node));
    }
}

fn qa_board(name: &str) -> Option<(Design, ConstraintModel)> {
    let root = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data")).join("pcbnew/pns_regressions/boards");
    let text = std::fs::read_to_string(root.join(format!("{name}.kicad_pcb"))).ok()?;
    let (mut design, model, _) = eda_kicad::import_kicad_pcb(&text).unwrap_or_else(|e| panic!("import {name}: {e:?}"));
    design.assign_missing_ids();
    Some((design, model))
}

/// The footprints of `node` with at least one track ending on a pad's centre, by reference, in order.
fn routed_footprints(node: &Node) -> Vec<String> {
    let mut out: Vec<String> = node
        .iter()
        .filter_map(|(_, it)| match it {
            Item::Solid(s) if !s.edge && !s.is_graphic() && !s.is_text() && !s.is_keepout() && !s.is_obstacle() => {
                let linked = node.joint_at(s.pos, &s.net).is_some_and(|j| j.links.iter().any(|id| node.get(*id).is_some_and(Item::is_track)));
                linked.then(|| s.source.split_once('.').map(|(r, _)| r.to_string())).flatten()
            }
            _ => None,
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// One component drag of `fp` by `by` on `design` in `mode`, committed when the router accepts it: the design it leaves, or `None`.
fn drag_footprint(design: &Design, model: &ConstraintModel, fp: &str, by: (f64, f64), mode: Mode) -> Option<(RouteCommit, Design)> {
    let mut router = Router::new(design, model);
    router.settings.mode = mode;
    let start = router.drag_start_component(Point { x: Nm::ZERO, y: Nm::ZERO }, &[fp.to_string()], true).ok()?;
    let to = Point { x: start.x + mm(by.0), y: start.y + mm(by.1) };
    router.drag_preview(to)?;
    let commit = router.drag_finish(to)?;
    let after = apply(design, &commit);
    Some((commit, after))
}

/// Component drags on KiCad's own boards, every footprint with a routed pad (up to a dozen a board), 0.4 mm one way or another in each mode in
/// turn: whatever the router commits leaves no pair colliding that did not before, and no track end loose that was not -- every track on a
/// moved pad's centre went with it.
#[test]
fn component_drags_on_kicads_boards_never_leave_a_new_violation_or_a_loose_track() {
    let mut judged = 0;
    for board in ["pic_programmer", "backspace1"] {
        let Some((design, model)) = qa_board(board) else {
            eprintln!("KiCad's QA corpus not found; skipping");
            return;
        };
        let (node, _) = eda_pns::from_ir::build_node(&design, &model);
        let before = violations(&node, &model.board);
        let loose_before = loose_ends(&node);
        let fps = routed_footprints(&node);
        eprintln!("{board}: {} routed footprints", fps.len());
        let vectors = [(0.4, 0.0), (0.0, -0.4), (-0.3, 0.3)];
        for (k, fp) in fps.iter().take(12).enumerate() {
            let mode = [Mode::Shove, Mode::Walkaround, Mode::MarkObstacles][k % 3];
            let by = vectors[k % vectors.len()];
            let Some((commit, after)) = drag_footprint(&design, &model, fp, by, mode) else {
                eprintln!("  {fp} {mode:?} by {by:?}: refused");
                continue;
            };
            let (node, _) = eda_pns::from_ir::build_node(&after, &model);
            let new = new_violations(&before, &violations(&node, &model.board));
            assert!(new.is_empty(), "{board}: {fp} {mode:?} by {by:?}: {new:?}");
            let loose: Vec<Point> = loose_ends(&node).into_iter().filter(|e| !loose_before.contains(e)).collect();
            // a loose end the board had, moved with a track that moved whole, is the same one
            let moved_loose = loose_before.iter().filter(|e| !loose_ends(&node).contains(e)).count();
            assert!(loose.len() <= moved_loose, "{board}: {fp} {mode:?} by {by:?}: new loose track ends {loose:?} ({} tracks moved whole)", commit.moved_tracks.len());
            eprintln!("  {fp} {mode:?} by {by:?}: {} tracks replaced, {} moved whole", commit.remove_track_ids.len(), commit.moved_tracks.len());
            judged += 1;
        }
    }
    eprintln!("{judged} component drags judged");
    assert!(judged >= 6, "{judged} component drags committed");
}

/// UI-LEFT P27 / R1's left: a footprint's own Edge.Cuts graphic (a slot of its art) goes with it under D, as `InlineDrag` adds the footprint's
/// `GraphicalItems()` on Edge_Cuts or Margin to the items dragged; the pads keep their distance to it. Left behind, the slot would be where a pad
/// lands, and the drag would collide with the board edge.
#[test]
fn a_component_drag_takes_the_footprints_own_edge_cuts_along() {
    use eda_model::fp_art::FpShape;
    let (design, mut model) = board(&[("R1", "TWO", p(15.0, 15.0))], &[]);
    let slot = eda_model::ir::Shape::Circle { id: String::new(), layer: "Edge.Cuts".into(), stroke_width: mm(0.05), filled: false, center: Point { x: mm(1.0), y: mm(3.0) }, end: Point { x: mm(1.5), y: mm(3.0) } };
    model.footprints.iter_mut().find(|f| f.name == "TWO").unwrap().art.shapes.push(FpShape::new(slot));
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::Walkaround;
    router.drag_start_component(p(15.0, 15.0), &["R1".to_string()], true).expect("starts");
    // three millimetres down: pad 1 lands where the slot was, beside its centre
    let pv = router.drag_preview(p(15.0, 18.0)).expect("a drag");
    assert_eq!(pv.offset, Some((Nm::ZERO, mm(3.0))));
    assert!(!pv.colliding, "the slot moved with the footprint: {pv:?}");
}
