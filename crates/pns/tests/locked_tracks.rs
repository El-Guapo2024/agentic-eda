//! Locked tracks (`MK_LOCKED`, `PNS_KICAD_IFACE_BASE::syncTrack` / `syncArc` / `syncVia`): the world reads the board's locks, a locked track, arc or via is
//! neither shoved (`pns_shove.cpp` treats it as a solid: `onCollidingSegment` / `onCollidingArc`: `HasLockedSegments` -> `SH_TRY_WALK`; `pushOrShoveVia`:
//! `aVia->IsLocked()`) nor dragged (`ROUTER_TOOL::performDragging`'s "The selected item is locked."), and a dragged line stops in front of one. And the
//! redundant-segment rule of `NODE::Add` (`findRedundantSegment`): a segment the board already has is not added a second time.

use eda_model::ir::{Design, DrawingsSection, FootprintInstance, LabelSide, PlacementSection, Point, Provenance, RoutingSection, Side, Track, Nm, Via};
use eda_model::{ConstraintModel, Net as IrNet, Part, Pin, PinKind};
use eda_pns::arcs::ArcPts;
use eda_pns::item::Item;
use eda_pns::router::{RouteCommit, Router};
use eda_pns::settings::Mode;

fn p(x: Nm, y: Nm) -> Point {
    Point { x, y }
}

fn part(r: &str) -> Part {
    Part { reference: r.into(), mpn: None, lcsc: None, value: None, package: Some("0603".into()), footprint: Some("0603".into()), pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }], body_nm: None, symbol: None, datasheet: None, edge: None }
}

/// Two resistors far from the action (so no pad is in the way), SIG on R1.1 and R2.1, the given tracks and vias, and `locked` ids locked.
fn board(tracks: Vec<Track>, vias: Vec<Via>, locked: &[&str]) -> (Design, ConstraintModel) {
    let model = ConstraintModel { parts: vec![part("R1"), part("R2")], nets: vec![IrNet { name: "SIG".into(), pins: vec!["R1.1".into(), "R2.1".into()] }], ..Default::default() };
    let design = Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "test".into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(PlacementSection {
            outline: vec![],
            footprints: vec![FootprintInstance { id: "R1".into(), at: p(-9000, 30_000), rot: 0, side: Side::Top, label: LabelSide::Above }, FootprintInstance { id: "R2".into(), at: p(9000, 30_000), rot: 0, side: Side::Top, label: LabelSide::Above }],
            modules: vec![],
        }),
        routing: Some(RoutingSection { tracks, vias, zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }),
        drawings: (!locked.is_empty()).then(|| DrawingsSection { locked_ids: locked.iter().map(|s| s.to_string()).collect(), ..Default::default() }),
    };
    (design, model)
}

fn straight(id: &str, net: &str, pts: &[Point]) -> Track {
    Track { id: id.into(), net: net.into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: pts.to_vec(), arc_mid_offset: None }
}

fn arc_track(id: &str, net: &str, a: ArcPts) -> Track {
    let mut t = Track::new_arc(net.into(), "F.Cu".into(), 200, a.start, a.mid, a.end);
    t.id = id.into();
    t
}

/// SIG starts on a stub whose end is (-9000, 0): the head runs along y = 0 to (9000, 0).
fn sig_stub() -> Track {
    straight("t_sig", "SIG", &[p(-10_000, 0), p(-9000, 0)])
}

fn shove_route(design: &Design, model: &ConstraintModel) -> (eda_pns::line_placer::Preview, Option<RouteCommit>) {
    let mut router = Router::new(design, model);
    router.settings.mode = Mode::Shove;
    router.start(p(-9000, 0), "F.Cu", 200).expect("starts on the end of the SIG stub");
    let preview = router.preview(p(9000, 0)).expect("a route in progress");
    let commit = router.finish(p(9000, 0));
    (preview, commit)
}

#[test]
fn the_world_reads_the_locks_of_the_board_tracks_arcs_vias_and_the_tracks_of_a_tuning_pattern() {
    let arc = ArcPts::new(p(0, 5000), p(1000, 4000), p(2000, 5000));
    let via = Via { id: "via_a".into(), net: "GND".into(), at: p(5000, 5000), drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() };
    let tracks = vec![straight("t_locked", "GND", &[p(0, 0), p(1000, 0)]), straight("t_free", "GND", &[p(0, 1000), p(1000, 1000)]), arc_track("t_arc", "GND", arc), straight("t_member", "GND", &[p(0, 2000), p(1000, 2000)])];
    let (mut design, model) = board(tracks, vec![via, Via { id: "via_b".into(), net: "GND".into(), at: p(6000, 5000), drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }], &["t_locked", "t_arc", "via_a"]);
    // the member of a pattern is locked for the router, as a `PCB_GENERATOR`'s members are
    let mut pattern: eda_model::tuning::TuningPattern = serde_json::from_value(serde_json::json!({
        "id": "tun_1", "mode": "single", "layer": "F.Cu", "origin": {"x": 0, "y": 2000}, "end": {"x": 1000, "y": 2000}, "initial_side": "left",
        "min_amplitude": 200, "max_amplitude": 1000, "spacing": 600, "corner_radius_percent": 100, "rounded": true, "single_sided": false,
        "override_custom_rules": false, "target_length": {"min": 0, "opt": 0, "max": 0}, "target_skew": {"min": 0, "opt": 0, "max": 0},
        "base_line": [], "base_line_coupled": [], "member_ids": ["t_member"]
    }))
    .unwrap_or_else(|e| panic!("a pattern: {e}"));
    pattern.member_ids = vec!["t_member".into()];
    design.drawings.as_mut().unwrap().tuning_patterns.push(pattern);
    let (node, _) = eda_pns::from_ir::build_node(&design, &model);
    let locked_of = |track: &str| node.iter().filter(|(_, it)| it.source_track_id() == Some(track)).map(|(_, it)| it.is_locked()).collect::<Vec<_>>();
    assert_eq!(locked_of("t_locked"), vec![true]);
    assert_eq!(locked_of("t_free"), vec![false]);
    assert_eq!(locked_of("t_arc"), vec![true], "an arc track is locked as a track is");
    assert_eq!(locked_of("t_member"), vec![true], "the tracks of a tuning pattern are locked for the router");
    let vias: Vec<(String, bool)> = node.iter().filter_map(|(_, it)| if let Item::Via(v) = it { Some((v.source_via.clone().unwrap(), v.locked)) } else { None }).collect();
    assert!(vias.contains(&("via_a".to_string(), true)) && vias.contains(&("via_b".to_string(), false)), "{vias:?}");
}

/// A GND track beside the SIG head's way (y = 300: 100 um short of the clearance), long enough to be pushed aside; with `lock` it is locked.
fn parallel_gnd(lock: bool) -> (Design, ConstraintModel) {
    board(vec![sig_stub(), straight("t_gnd", "GND", &[p(-6000, 300), p(6000, 300)])], vec![], if lock { &["t_gnd"] } else { &[] })
}

#[test]
fn a_track_beside_the_route_is_pushed_aside_and_the_same_track_locked_is_walked_round_instead() {
    let (design, model) = parallel_gnd(false);
    let (preview, commit) = shove_route(&design, &model);
    assert!(!preview.colliding);
    assert!(!preview.displaced_lines.is_empty(), "the unlocked track is shoved");
    let commit = commit.expect("commits");
    assert!(commit.remove_track_ids.contains(&"t_gnd".to_string()));

    let (design, model) = parallel_gnd(true);
    let (preview, commit) = shove_route(&design, &model);
    assert!(!preview.colliding, "the route goes round it");
    assert!(preview.displaced_lines.is_empty(), "a locked track is not shoved: {:?}", preview.displaced_lines.iter().map(|d| d.source_track.clone()).collect::<Vec<_>>());
    assert!(preview.head.pts.len() > 2, "the head leaves its straight line to go round the locked track: {:?}", preview.head.pts);
    let commit = commit.expect("commits");
    assert!(!commit.remove_track_ids.contains(&"t_gnd".to_string()), "and the locked track is not replaced");
}

#[test]
fn a_locked_arc_track_is_walked_round_not_shoved() {
    // an arc dipping to y = -100 between two stubs, as in arc_tracks.rs
    let arc = ArcPts::new(p(-3000, 876), p(0, -100), p(3000, 876));
    let tracks = |locked: bool| {
        let _ = locked;
        vec![sig_stub(), straight("t_l", "GND", &[p(-3000, 3000), arc.start]), arc_track("t_arc", "GND", arc), straight("t_r", "GND", &[arc.end, p(3000, 3000)])]
    };
    let (design, model) = board(tracks(false), vec![], &[]);
    let (preview, _) = shove_route(&design, &model);
    assert!(!preview.displaced_lines.is_empty(), "unlocked, the arc's line is pushed");
    let (design, model) = board(tracks(true), vec![], &["t_arc"]);
    let (preview, commit) = shove_route(&design, &model);
    assert!(!preview.colliding);
    assert!(preview.displaced_lines.is_empty(), "locked, it is not");
    assert!(!commit.expect("commits").remove_track_ids.contains(&"t_arc".to_string()));
}

#[test]
fn a_locked_via_is_walked_round_not_pushed() {
    let via = |_: bool| Via { id: "via_g".into(), net: "GND".into(), at: p(0, 350), drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() };
    let (design, model) = board(vec![sig_stub()], vec![via(false)], &[]);
    let (preview, _) = shove_route(&design, &model);
    assert!(!preview.displaced_vias.is_empty(), "unlocked, the via is pushed aside");
    let (design, model) = board(vec![sig_stub()], vec![via(true)], &["via_g"]);
    let (preview, commit) = shove_route(&design, &model);
    assert!(!preview.colliding);
    assert!(preview.displaced_vias.is_empty(), "locked, it stays");
    assert!(commit.expect("commits").remove_via_ids.is_empty());
}

#[test]
fn a_locked_track_cannot_be_dragged_unless_the_person_says_drag_anyway_and_then_only_it_is_dragged() {
    let (design, model) = board(vec![straight("t_a", "GND", &[p(0, 0), p(4000, 0)]), straight("t_b", "GND", &[p(4000, 0), p(4000, 4000)]), straight("t_c", "GND", &[p(4000, 4000), p(8000, 4000)])], vec![], &["t_b"]);
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::MarkObstacles;
    // the locked one
    let err = router.drag_start(p(4000, 2000), "F.Cu").unwrap_err();
    assert_eq!(err, eda_pns::dragger::LOCKED_MESSAGE);
    // "Drag Anyway": it is dragged, and the line it is part of stops in front of the locked tracks next to it -- here it is the only one that is locked, so it stands alone
    router.drag_start_locked(p(4000, 2000), "F.Cu", false, true).expect("Drag Anyway");
    let preview = router.drag_preview(p(5000, 2000)).unwrap();
    assert!(!preview.colliding);
    let commit = router.drag_finish(p(5000, 2000)).expect("commits");
    assert!(commit.remove_track_ids.contains(&"t_b".to_string()));
    // the unlocked track next to a locked one is dragged alone: the line it is part of stops at the locked track
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::MarkObstacles;
    router.drag_start(p(2000, 0), "F.Cu").expect("t_a is not locked");
    let commit = {
        let _ = router.drag_preview(p(2000, -1000)).unwrap();
        router.drag_finish(p(2000, -1000)).expect("commits")
    };
    assert_eq!(commit.remove_track_ids, vec!["t_a".to_string()], "the locked t_b is not part of the dragged line");
}

#[test]
fn a_route_started_in_the_middle_of_a_locked_track_cuts_it_in_two_locked_halves() {
    let (design, model) = board(vec![straight("t_gnd", "SIG", &[p(-4000, 0), p(4000, 0)])], vec![], &["t_gnd"]);
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::MarkObstacles;
    router.start(p(1000, 0), "F.Cu", 200).expect("starts on the track");
    let commit = router.finish(p(1000, 5000)).expect("commits");
    assert!(commit.remove_track_ids.contains(&"t_gnd".to_string()));
    // the track is cut in two halves (one polyline with a vertex at the start) -- locked
    let cut: Vec<usize> = commit.tracks.iter().enumerate().filter(|(_, t)| t.pts.contains(&p(1000, 0)) && t.pts.first() == Some(&p(-4000, 0))).map(|(i, _)| i).collect();
    assert!(!cut.is_empty() && cut.iter().all(|i| commit.lock_tracks.contains(i)), "the cut track keeps its lock: {:?} {:?}", commit.lock_tracks, commit.tracks.iter().map(|t| t.pts.len()).collect::<Vec<_>>());
    assert!(commit.lock_tracks.iter().all(|&i| i < commit.tracks.len()));
}

#[test]
fn a_segment_the_board_already_has_is_not_added_again() {
    // SIG runs from (-9000, 0) along an existing SIG track (-6000, 0)..(0, 0), which the route covers exactly
    let existing = straight("t_old", "SIG", &[p(-6000, 0), p(0, 0)]);
    let (design, model) = board(vec![straight("t_sig", "SIG", &[p(-10_000, 0), p(-9000, 0)]), existing], vec![], &[]);
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::MarkObstacles;
    router.start(p(-9000, 0), "F.Cu", 200).unwrap();
    // fix at (-6000, 0), then along the old track to its end and on
    router.fix(p(-6000, 0)).expect("fix");
    router.fix(p(0, 0)).expect("fix");
    let commit = router.finish(p(3000, 0)).expect("commits");
    let covers_old = commit.tracks.iter().any(|t| t.pts.windows(2).any(|w| (w[0] == p(-6000, 0) && w[1] == p(0, 0)) || (w[0] == p(0, 0) && w[1] == p(-6000, 0))));
    assert!(!covers_old, "the stretch the board already has is not a track of the commit: {:?}", commit.tracks.iter().map(|t| t.pts.clone()).collect::<Vec<_>>());
    assert!(commit.tracks.iter().any(|t| t.pts.first() == Some(&p(-9000, 0)) && t.pts.last() == Some(&p(-6000, 0))), "what comes before it is");
    assert!(commit.tracks.iter().any(|t| t.pts.first() == Some(&p(0, 0)) && t.pts.last() == Some(&p(3000, 0))), "and what comes after it");
}
