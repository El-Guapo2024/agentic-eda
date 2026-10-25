//! Differential pairs on KiCad's own pair board, `qa/data/pcbnew/pns_regressions/boards/dp_test.kicad_pcb`: a connector (J1) with a
//! D+/D- pair routed out of it to x = 118.95 mm, and a micro-BGA (U1) with the two pads' neck-down stubs that pair has to meet.
//! The corpus has no recorded session on it (no `pns.log` uses a pair), so these are not replays: they route the pair the way the
//! studio does and check what KiCad's router promises of a pair -- it starts from a pair of tracks or of pads, meets the coupled item,
//! keeps the gap and the clearance, and leaves the board as clean as it found it.
//!
//! Skipped (prints a line, passes) when the QA corpus is not on this machine: `KICAD_QA_DATA`, default `~/ws/kicad-src-8303b2ad/qa/data`.

use eda_model::units::mm;
use eda_drc::kimath::Shape;
use eda_model::ir::{Design, Point, Track, Nm};
use eda_model::{ConstraintModel, NetClass};
use eda_pns::from_ir::build_node;
use eda_pns::item::{Item, ItemId};
use eda_pns::line_placer::FixOutcome;
use eda_pns::node::Node;
use eda_pns::router::{RouteCommit, Router};
use eda_pns::settings::{Mode, RoutingSettings};
use std::collections::HashSet;
use std::path::PathBuf;

fn corpus() -> Option<PathBuf> {
    let root = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data"));
    let file = root.join("pcbnew/pns_regressions/boards/dp_test.kicad_pcb");
    file.exists().then_some(file)
}

/// `dp_test.kicad_pcb`, with the pair's net class a KiCad project would give it (0.2 mm tracks, 0.25 mm gap).
fn dp_test() -> Option<(Design, ConstraintModel)> {
    let text = std::fs::read_to_string(corpus()?).ok()?;
    let (mut design, mut model, _) = eda_kicad::import_kicad_pcb(&text).unwrap_or_else(|e| panic!("import: {e:?}"));
    model.board.net_classes.push(NetClass { name: "usb".into(), nets: vec!["/D+".into(), "/D-".into()], track_width: None, clearance: None, via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: Some(mm(0.2)), diff_pair_gap: Some(mm(0.25)), diff_pair_via_gap: Some(mm(0.25)), priority: 0 });
    design.assign_missing_ids();
    Some((design, model))
}

fn p(x: Nm, y: Nm) -> Point {
    Point { x, y }
}

fn settings(mode: Mode) -> RoutingSettings {
    RoutingSettings { mode, ..RoutingSettings::default() }
}

/// Every pair of items the router's own collision test calls a violation (as `qa_regressions` does).
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

/// The design after a finished route's commit, as `Cmd::CommitRoute` applies it.
fn apply_commit(design: &Design, commit: &RouteCommit) -> Design {
    let mut d = design.clone();
    let rt = d.routing.as_mut().unwrap();
    rt.tracks.retain(|t| !commit.remove_track_ids.contains(&t.id));
    rt.vias.retain(|v| !commit.remove_via_ids.contains(&v.id));
    rt.tracks.extend(commit.tracks.iter().cloned());
    rt.vias.extend(commit.vias.iter().cloned());
    d.assign_missing_ids();
    d
}

/// A commit applied to the router's world directly (without building it again from the IR): the removed tracks and vias go, the new
/// ones come in.
fn commit_into_node(node: &Node, layers: &eda_pns::layer::LayerMap, commit: &RouteCommit) -> Node {
    let mut w = node.branch();
    let doomed: Vec<ItemId> = w
        .iter()
        .filter(|(_, it)| match it {
            Item::Segment(s) => s.source_track.as_ref().is_some_and(|(t, _)| commit.remove_track_ids.contains(t)),
            Item::Via(v) => v.source_via.as_ref().is_some_and(|id| commit.remove_via_ids.contains(id)),
            _ => false,
        })
        .map(|(id, _)| id)
        .collect();
    for id in doomed {
        w.remove(id);
    }
    for t in &commit.tracks {
        let line = eda_pns::line::Line::from_points(eda_pns::item::net_of(&t.net), layers.index_of(&t.layer).unwrap(), t.width, t.pts.clone());
        w.add_line(&line, None, false);
    }
    for v in &commit.vias {
        w.add(Item::Via(eda_pns::item::Via { net: eda_pns::item::net_of(&v.net), layers: layers.range_of(&v.from_layer, &v.to_layer), pos: v.at, diameter: v.diameter, drill: v.drill, source_via: None, locked: false, is_virtual: false }));
    }
    w
}

/// The least gap between two polylines of the given widths.
fn gap_between(a: &[Point], wa: Nm, b: &[Point], wb: Nm) -> f64 {
    let mut best = f64::MAX;
    for sa in a.windows(2) {
        for sb in b.windows(2) {
            best = best.min(Shape::Stadium { a: sa[0], b: sa[1], r: wa / 2 }.gap_to(&Shape::Stadium { a: sb[0], b: sb[1], r: wb / 2 }));
        }
    }
    best
}

/// The dangling ends of the J1 side's pair, and of the U1 side's stubs (0.075 mm wide), that it has to meet.
const J1_SIDE: (Point, Point) = (Point { x: Nm::new(118_950_000), y: Nm::new(95_075_000) }, Point { x: Nm::new(118_950_000), y: Nm::new(94_625_000) });
const U1_STUBS: (Point, Point) = (Point { x: Nm::new(118_050_000), y: Nm::new(95_300_000) }, Point { x: Nm::new(118_050_000), y: Nm::new(94_750_000) });

fn track_of<'a>(commit: &'a RouteCommit, net: &str) -> &'a Track {
    commit.tracks.iter().find(|t| t.net == net).unwrap_or_else(|| panic!("no track on {net} in {commit:?}"))
}

/// Starting from the J1 side's pair of tracks, the pair meets the two stubs in every mode, ends exactly on their ends, and the
/// board it leaves is as clean as the one it found.
#[test]
fn a_pair_continues_from_a_pair_of_tracks_and_meets_the_two_stubs_in_every_mode() {
    let Some((design, model)) = dp_test() else {
        eprintln!("KiCad QA corpus not found: skipping");
        return;
    };
    let (node, _) = build_node(&design, &model);
    let before = violations(&node, &model.board);
    for mode in [Mode::MarkObstacles, Mode::Walkaround, Mode::Shove] {
        let mut router = Router::new(&design, &model);
        router.settings = settings(mode);
        // the cursor on either end: the pair starts from the ends of both tracks
        router.start_diff_pair(J1_SIDE.0, "F.Cu").unwrap_or_else(|e| panic!("{mode:?}: {e}"));
        assert_eq!(router.diff_pair_nets(), Some((Some("/D+".into()), Some("/D-".into()))), "P is the net named +");
        let pv = router.diff_pair_preview(U1_STUBS.0).expect("preview");
        assert!(!pv.colliding && pv.snapped_end, "{mode:?}: {pv:?}");
        assert_eq!((pv.head_a.first(), pv.head_a.last()), (Some(J1_SIDE.0), Some(U1_STUBS.0)), "{mode:?}: D+ from its track's end to the stub's");
        assert_eq!((pv.head_b.first(), pv.head_b.last()), (Some(J1_SIDE.1), Some(U1_STUBS.1)), "{mode:?}: D- likewise");
        assert!(matches!(router.fix_diff_pair(U1_STUBS.0), Some(FixOutcome::Fixed { real_end: true })));
        let commit = router.finish_diff_pair(U1_STUBS.0).expect("commits");
        assert_eq!(commit.tracks.len(), 2, "{mode:?}: one track per net: {commit:?}");
        assert!(commit.remove_track_ids.is_empty() && commit.vias.is_empty());
        let (d_plus, d_minus) = (track_of(&commit, "/D+"), track_of(&commit, "/D-"));
        assert_eq!((d_plus.width, d_minus.width), (mm(0.2), mm(0.2)), "the width of the tracks it continues");
        // the two tracks keep the pair's gap where they run side by side, and each net's copper joins its stub
        let (after_design, after_node) = {
            let d = apply_commit(&design, &commit);
            let (n, _) = build_node(&d, &model);
            (d, n)
        };
        let new_ones: Vec<_> = violations(&after_node, &model.board).difference(&before).cloned().collect();
        assert!(new_ones.is_empty(), "{mode:?}: new violations {new_ones:#?}");
        let _ = after_design;
        let straight = |t: &Track| t.pts.windows(2).filter(|w| w[0].y == w[1].y).map(|w| (w[0].x - w[1].x).abs()).max().unwrap_or(Nm::ZERO);
        assert!(straight(d_plus) > mm(0.3) && straight(d_minus) > mm(0.3), "{mode:?}: a run side by side: {commit:?}");
    }
}

/// `ROUTER::ContinueFromEnd` restarts the placer of the same mode: Route From Other End for a pair commits what the pair has fixed, and a new pair
/// starts at the far end of the connection and heads for where the old head ended. The far end here is the U1 side's two stubs, a neck-down where KiCad
/// refuses to start a pair at the configured gap unless violations are allowed (`isStartingPointRoutable`): Highlight collisions with "Allow DRC
/// violations" starts there; Walk around and Shove refuse, and say so, after the fixed legs were committed.
#[test]
fn route_from_other_end_restarts_the_pair_at_the_far_end_and_commits_what_was_fixed() {
    let Some((design, model)) = dp_test() else {
        eprintln!("KiCad QA corpus not found: skipping");
        return;
    };
    let (node, _) = build_node(&design, &model);
    let before = violations(&node, &model.board);
    for mode in [Mode::MarkObstacles, Mode::Walkaround] {
        let settings = RoutingSettings { mode, can_violate_drc: true, ..RoutingSettings::default() };
        for fix_first in [false, true] {
            let mut router = Router::new(&design, &model);
            router.settings = settings.clone();
            router.start_diff_pair(J1_SIDE.0, "F.Cu").unwrap_or_else(|e| panic!("{mode:?}: {e}"));
            // a head drawn from the J1 side's tracks towards the stubs, short of them
            let mid = p(mm(118.7), mm(95.0));
            let pv = router.diff_pair_preview(mid).expect("preview");
            assert!(!pv.colliding && !pv.head_a.pts.is_empty(), "{mode:?}: {pv:?}");
            let mut to = mid;
            if fix_first {
                assert!(matches!(router.fix_diff_pair(mid), Some(FixOutcome::Fixed { real_end: false })), "{mode:?}");
                to = p(mm(118.5), mm(95.0));
                let next_leg = router.diff_pair_preview(to).expect("the next leg");
                assert!(!next_leg.head_a.pts.is_empty(), "{mode:?}: {next_leg:?}");
            }
            let c = router.continue_diff_pair_from_end(to).unwrap_or_else(|e| panic!("{mode:?} fix_first={fix_first}: {e}"));
            assert_eq!(c.net, "/D+", "the P net");
            assert_eq!(c.from, U1_STUBS.0, "{mode:?}: the far end of D+ is the U1 side's stub");
            assert_eq!(c.layer, "F.Cu");
            assert_eq!((c.width, c.gap), (mm(0.2), mm(0.25)), "the pair's own sizes go on");
            assert_eq!(c.commit.tracks.is_empty(), !fix_first, "{mode:?}: what was fixed is committed, nothing if nothing was");
            if fix_first {
                assert!(c.commit.tracks.iter().any(|t| t.net == "/D+") && c.commit.tracks.iter().any(|t| t.net == "/D-"), "{:?}", c.commit);
            }
            // the caller commits and starts a new pair there, on the board as it stands now
            let committed = apply_commit(&design, &c.commit);
            let mut next = Router::new(&committed, &model);
            next.settings = settings.clone();
            if mode != Mode::MarkObstacles {
                let refused = next.start_diff_pair(c.from, &c.layer).unwrap_err();
                assert!(refused.contains("gap at the start point does not match"), "{mode:?}: {refused}");
                continue;
            }
            next.start_diff_pair(c.from, &c.layer).unwrap_or_else(|e| panic!("{mode:?}: the new pair starts on the stubs: {e}"));
            let pv = next.diff_pair_preview(c.to).expect("the new pair's preview");
            assert_eq!((pv.head_a.first(), pv.head_b.first()), (Some(U1_STUBS.0), Some(U1_STUBS.1)), "{mode:?}: it leaves the two stubs");
            assert!(!pv.head_a.pts.is_empty(), "{mode:?}: {pv:?}");
            let after_node = build_node(&committed, &model).0;
            let new_ones: Vec<_> = violations(&after_node, &model.board).difference(&before).cloned().collect();
            assert!(new_ones.is_empty(), "{mode:?}: new violations {new_ones:#?}");
        }
    }
    // a pair that is not routing has no other end
    let mut idle = Router::new(&design, &model);
    assert!(idle.continue_diff_pair_from_end(p(Nm::ZERO, Nm::ZERO)).is_err());
}

/// From the pair of pads of J1 (a square pad and a round one on a 1.27 mm pitch, one above the other) to open board on its right: the
/// lines leave the pads from their centres, side by side, at the pair's pitch.
#[test]
fn a_pair_leaves_the_connectors_pad_pair_and_runs_at_the_pitch() {
    let Some((design, model)) = dp_test() else {
        eprintln!("KiCad QA corpus not found: skipping");
        return;
    };
    let (node, _) = build_node(&design, &model);
    let before = violations(&node, &model.board);
    for mode in [Mode::MarkObstacles, Mode::Walkaround, Mode::Shove] {
        let mut router = Router::new(&design, &model);
        router.settings = settings(mode);
        // J1.1 (D+, square) at (124.7, 95.9) and J1.2 (D-, round) at (124.7, 94.63)
        router.start_diff_pair(p(mm(124.7), mm(95.9)), "F.Cu").unwrap_or_else(|e| panic!("{mode:?}: {e}"));
        let at = p(mm(135.0), mm(95.265));
        let pv = router.diff_pair_preview(at).expect("preview");
        assert!(!pv.colliding && !pv.snapped_end, "{mode:?}: {pv:?}");
        assert_eq!((pv.head_a.first(), pv.head_b.first()), (Some(p(mm(124.7), mm(95.9))), Some(p(mm(124.7), mm(94.63)))), "from the pad centres");
        let pitch = pv.width + pv.gap;
        let (a, b) = (pv.head_a.last().unwrap(), pv.head_b.last().unwrap());
        let end = (((a.x - b.x).raw_shim() as f64).powi(2) + ((a.y - b.y).raw_shim() as f64).powi(2)).sqrt();
        assert!(end >= pitch.raw_shim() as f64 - 10.0 && end <= pitch.raw_shim() as f64 + 10.0, "{mode:?}: the pair ends a pitch ({pitch}) apart, not {end}: {a:?} {b:?}");
        let gap = gap_between(&pv.head_a.pts, pv.width, &pv.head_b.pts, pv.width);
        assert!(gap >= pv.gap.raw_shim() as f64 - 100.0, "{mode:?}: the lines come within {gap} of each other");
        // fix it where it is, and finish
        router.fix_diff_pair(at).expect("a pair");
        let commit = router.finish_diff_pair(at).expect("commits");
        let after_node = build_node(&apply_commit(&design, &commit), &model).0;
        let new_ones: Vec<_> = violations(&after_node, &model.board).difference(&before).cloned().collect();
        assert!(new_ones.is_empty(), "{mode:?}: new violations {new_ones:#?}");
    }
}

/// xorshift64: the same sequence every run.
struct Rng(u64);

impl Rng {
    fn below(&mut self, m: i64) -> i64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % m as u64) as i64
    }
}

/// The board with GND tracks laid across the open space for the walk and the shove to deal with: north of the BGA (across the way a
/// pair heading north-west takes), south of it, and east of the J1 connector's pads.
fn dp_test_with_obstacles() -> Option<(Design, ConstraintModel)> {
    let (mut design, model) = dp_test()?;
    let track = |id: &str, pts: &[Point]| Track { id: id.into(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: pts.to_vec(), arc_mid_offset: None };
    let rt = design.routing.as_mut().unwrap();
    rt.tracks.push(track("gnd_north", &[p(mm(109.0), mm(92.5)), p(mm(114.0), mm(91.5)), p(mm(119.0), mm(91.5)), p(mm(121.0), mm(89.5))]));
    rt.tracks.push(track("gnd_south", &[p(mm(109.0), mm(100.5)), p(mm(116.0), mm(100.5)), p(mm(120.0), mm(104.5))]));
    rt.tracks.push(track("gnd_east", &[p(mm(128.0), mm(90.0)), p(mm(128.0), mm(93.0)), p(mm(130.0), mm(95.0)), p(mm(130.0), mm(100.0))]));
    design.assign_missing_ids();
    Some((design, model))
}

/// Routes a pair from `start` to each cursor in `cursors`, in each of `modes`; whatever the router accepts must leave no new violation,
/// keep the pair's lines the clearance from each other and, where a stretch of one runs beside a stretch of the other, the gap.
/// Returns `(tried, accepted, shoved)`.
fn sweep(design: &Design, model: &ConstraintModel, start: Point, modes: &[Mode], cursors: &[Point]) -> (usize, usize, usize) {
    let (node, layers) = build_node(design, model);
    let before = violations(&node, &model.board);
    let mut router = Router::new(design, model);
    let (mut tried, mut accepted, mut shoved) = (0, 0, 0);
    for &mode in modes {
        router.settings = settings(mode);
        for &at in cursors {
            if router.start_diff_pair(start, "F.Cu").is_err() {
                continue;
            }
            tried += 1;
            let Some(pv) = router.diff_pair_preview(at) else { continue };
            if pv.colliding || pv.head_a.pts.len() < 2 {
                continue;
            }
            accepted += 1;
            shoved += usize::from(!pv.displaced_lines.is_empty() || !pv.displaced_vias.is_empty());
            // the lines never come within the clearance of each other, and where they run side by side (a stretch of one has a
            // stretch of the other beside it) they are the gap apart
            let clearance = model.board.clearance.raw_shim() as f64;
            let gap = gap_between(&pv.head_a.pts, pv.width, &pv.head_b.pts, pv.width);
            assert!(gap >= clearance - 500.0, "{mode:?} at {at:?}: the lines come within {gap} of each other (clearance {clearance})\n P {:?}\n N {:?}", pv.head_a.pts, pv.head_b.pts);
            let mut dp = eda_pns::diff_pair::DiffPair { p: pv.head_a.pts.clone(), n: pv.head_b.pts.clone(), width: pv.width, ..Default::default() };
            dp.set_gap(pv.gap);
            for c in dp.coupled_segment_pairs() {
                let g = gap_between(&[c.coupled_p.0, c.coupled_p.1], pv.width, &[c.coupled_n.0, c.coupled_n.1], pv.width);
                assert!(g >= pv.gap.raw_shim() as f64 - 1500.0, "{mode:?} at {at:?}: coupled stretches {c:?} are {g} apart (gap {})\n P {:?}\n N {:?}", pv.gap, pv.head_a.pts, pv.head_b.pts);
            }
            let Some(commit) = router.finish_diff_pair(at) else { panic!("{mode:?} at {at:?}: a pair that fits finishes") };
            let after = commit_into_node(&node, &layers, &commit);
            let new_ones: Vec<_> = violations(&after, &model.board).difference(&before).cloned().collect();
            assert!(new_ones.is_empty(), "{mode:?} at {at:?}: new violations {new_ones:#?}\n P {:?}\n N {:?}", pv.head_a.pts, pv.head_b.pts);
        }
    }
    (tried, accepted, shoved)
}

/// A fixed handful of cursors for each mode, from the J1 pad pair heading east into the open board where GND tracks cross: the pair
/// walks round them or pushes them, and what it commits is clean.
#[test]
fn a_pair_from_the_connector_walks_or_shoves_across_the_open_board_and_leaves_it_clean() {
    let Some((design, model)) = dp_test_with_obstacles() else {
        eprintln!("KiCad QA corpus not found: skipping");
        return;
    };
    let cursors = [p(mm(133.0), mm(95.265)), p(mm(135.0), mm(99.0)), p(mm(134.0), mm(91.0)), p(mm(129.0), mm(94.0)), p(mm(138.0), mm(96.5))];
    let (tried, accepted, shoved) = sweep(&design, &model, p(mm(124.7), mm(95.9)), &[Mode::Walkaround, Mode::Shove, Mode::MarkObstacles], &cursors);
    eprintln!("tried {tried}, accepted {accepted}, shoved something {shoved} times");
    assert!(accepted >= 6, "{accepted} of {tried} accepted");
    assert!(shoved >= 1, "the shove never shoved the GND track across the way");
}

/// Wherever the cursor is, a pair the router says fits is clean and keeps its gap -- from the J1 side's tracks heading west and from the
/// connector's pads heading east, in every mode (slow tier: a dense BGA to weave through, and a debug build).
#[test]
fn whatever_the_cursor_does_an_accepted_pair_leaves_the_board_clean_and_keeps_its_gap() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped (set EDA_SLOW_TESTS=1): about five minutes in a debug build");
        return;
    }
    let Some((design, model)) = dp_test_with_obstacles() else {
        eprintln!("KiCad QA corpus not found: skipping");
        return;
    };
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let mut totals = (0, 0, 0);
    for (start, x0, y0, w, h) in [(J1_SIDE.0, 104_000_000, 86_000_000, 14_000_000, 22_000_000), (p(mm(124.7), mm(95.9)), 125_500_000, 86_000_000, 14_000_000, 22_000_000)] {
        let cursors: Vec<Point> = (0..30).map(|_| p(Nm::from_raw_shim(x0 + rng.below(w / 1000) * 1000), Nm::from_raw_shim(y0 + rng.below(h / 1000) * 1000))).collect();
        let (t, a, s) = sweep(&design, &model, start, &[Mode::Walkaround, Mode::Shove, Mode::MarkObstacles], &cursors);
        totals = (totals.0 + t, totals.1 + a, totals.2 + s);
    }
    eprintln!("tried {}, accepted {}, shoved something {} times", totals.0, totals.1, totals.2);
    assert!(totals.1 > 20, "the test is not testing much: {} accepted", totals.1);
    assert!(totals.2 > 0, "no shove happened: the test is not testing the shove");
}

