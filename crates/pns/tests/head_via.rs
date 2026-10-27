//! A single track's head that ends in a via is shoved together with the via (`LINE_PLACER::rhShoveOnly`: `newHead.AppendVia( makeVia( newHead.CLastPoint() ) )`,
//! then `m_shove->AddHeads( newHead )`; `SHOVE::Run` adds the head's via to the node with the head's rank): what the via sits on -- on every layer it spans --
//! is pushed aside, as what the track meets is, and the committed route is clean. A pair's heads were shoved with their vias already (`Shove::run_heads`).

use eda_model::units::mm;
use eda_model::ir::{Design, FootprintInstance, LabelSide, PlacementSection, Point, Provenance, RoutingSection, Side, Track, Nm};
use eda_model::{ConstraintModel, Net as IrNet, Part, Pin, PinKind};
use eda_pns::from_ir::build_node;
use eda_pns::item::ItemId;
use eda_pns::router::{RouteCommit, Router};
use eda_pns::settings::Mode;
use std::collections::HashSet;

fn p(x: Nm, y: Nm) -> Point {
    Point { x, y }
}

fn part(r: &str) -> Part {
    Part { reference: r.into(), mpn: None, lcsc: None, value: None, package: Some("0603".into()), footprint: Some("0603".into()), pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }], body_nm: None, symbol: None, datasheet: None, edge: None }
}

fn track(id: &str, net: &str, layer: &str, pts: &[Point]) -> Track {
    Track { id: id.into(), net: net.into(), pins: vec![], layer: layer.into(), width: mm(0.2), pts: pts.to_vec(), arc_mid_offset: None }
}

/// Two parts far from the action, a SIG stub on F.Cu ending at (-9000, 0), and the tracks given.
fn board(tracks: Vec<Track>) -> (Design, ConstraintModel) {
    let model = ConstraintModel { parts: vec![part("R1"), part("R2")], nets: vec![IrNet { name: "SIG".into(), pins: vec!["R1.1".into(), "R2.1".into()] }], ..Default::default() };
    let mut all = vec![track("t_sig", "SIG", "F.Cu", &[p(mm(-10.0), Nm::ZERO), p(mm(-9.0), Nm::ZERO)])];
    all.extend(tracks);
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
            footprints: vec![FootprintInstance { id: "R1".into(), at: p(mm(-9.0), mm(30.0)), rot: 0, side: Side::Top, label: LabelSide::Above }, FootprintInstance { id: "R2".into(), at: p(mm(9.0), mm(30.0)), rot: 0, side: Side::Top, label: LabelSide::Above }],
            modules: vec![],
        }),
        routing: Some(RoutingSection { tracks: all, vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], diff_pair_presets: vec![], teardrop_settings: Default::default() }),
        drawings: None,
    };
    (design, model)
}

/// The design after a commit, as `Cmd::CommitRoute` applies it.
fn committed(design: &Design, commit: &RouteCommit) -> Design {
    let mut d = design.clone();
    let rt = d.routing.as_mut().unwrap();
    rt.tracks.retain(|t| !commit.remove_track_ids.contains(&t.id));
    rt.vias.retain(|v| !commit.remove_via_ids.contains(&v.id));
    rt.tracks.extend(commit.tracks.iter().cloned());
    rt.vias.extend(commit.vias.iter().cloned());
    d.assign_missing_ids();
    d
}

/// Every pair of items the router's own collision test calls a violation.
fn violations(design: &Design, model: &ConstraintModel) -> HashSet<(ItemId, ItemId)> {
    let (node, _) = build_node(design, model);
    let mut out = HashSet::new();
    for (id, item) in node.iter() {
        for o in node.all_colliding(&item.shape(item.layers().start()), item.net(), item.layers(), &model.board, &[id]) {
            out.insert((id.min(o.id), id.max(o.id)));
        }
    }
    out
}

/// `V` while routing on F.Cu: the route goes on on B.Cu from the via; a GND track on B.Cu runs 50 um inside the via's clearance ring.
#[test]
fn a_route_with_a_via_pushes_the_track_on_the_other_layer_the_via_sits_on() {
    let (design, model) = board(vec![track("t_gnd", "GND", "B.Cu", &[p(mm(-3.0), mm(-4.0)), p(mm(-3.0), mm(4.0))])]);
    // the via at (-3000 - 300 - 100 - 50 - 300, 0) is 50 um short of the clearance ring of the GND track
    let via_at = p(mm(-3.45), Nm::ZERO);
    assert!(violations(&design, &model).is_empty(), "the board starts clean");
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::Shove;
    router.start(p(mm(-9.0), Nm::ZERO), "F.Cu", mm(0.2)).expect("starts on the end of the SIG stub");
    router.toggle_via(true, mm(0.6), mm(0.3), Some(1));
    let pv = router.preview(via_at).expect("a route in progress");
    assert!(!pv.colliding, "{pv:?}");
    assert_eq!(pv.via, Some((via_at, mm(0.6), mm(0.3))), "the via is where the head ends");
    assert_eq!(pv.head.last(), Some(via_at));
    assert_eq!(pv.displaced_lines.len(), 1, "the track under the via is pushed: {:?}", pv.displaced_lines);

    // `V` armed: the click fixes the head with its via and the route goes on on B.Cu; finishing it commits all of it as one
    assert!(router.fix(via_at).is_some());
    let commit = router.finish(p(mm(-3.45), mm(3.0))).expect("the route goes on on the other layer");
    assert_eq!(commit.vias.len(), 1);
    assert_eq!((commit.vias[0].at, commit.vias[0].from_layer.as_str(), commit.vias[0].to_layer.as_str()), (via_at, "F.Cu", "B.Cu"));
    assert!(commit.remove_track_ids.contains(&"t_gnd".to_string()), "{:?}", commit.remove_track_ids);
    assert!(commit.tracks.iter().any(|t| t.layer == "F.Cu") && commit.tracks.iter().any(|t| t.layer == "B.Cu"), "tracks on both layers");
    // r5 R5-2: the shoved GND track's replacements name it as their source (`GetSourceItem()`), the route's own tracks name none
    let gnd: Vec<usize> = (0..commit.tracks.len()).filter(|&i| commit.tracks[i].net == "GND").collect();
    assert!(!gnd.is_empty() && gnd.iter().all(|&i| commit.source_of(i) == Some("t_gnd")), "{:?} {:?}", commit.track_sources, commit.tracks);
    assert!((0..commit.tracks.len()).filter(|&i| commit.tracks[i].net == "SIG").all(|i| commit.source_of(i).is_none_or(|s| s == "t_sig")), "{:?}", commit.track_sources);
    let after = committed(&design, &commit);
    assert!(violations(&after, &model).is_empty(), "the committed board is clean: {:?}", violations(&after, &model));
}

/// The same board in Walk around: the via is pushed off the track instead (`VIA::PushoutForce` with every kind), nothing is displaced.
#[test]
fn the_same_via_in_walkaround_mode_moves_the_via_not_the_track() {
    let (design, model) = board(vec![track("t_gnd", "GND", "B.Cu", &[p(mm(-3.0), mm(-4.0)), p(mm(-3.0), mm(4.0))])]);
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::Walkaround;
    router.start(p(mm(-9.0), Nm::ZERO), "F.Cu", mm(0.2)).unwrap();
    router.toggle_via(true, mm(0.6), mm(0.3), Some(1));
    let pv = router.preview(p(mm(-3.45), Nm::ZERO)).expect("a route in progress");
    assert!(!pv.colliding && pv.displaced_lines.is_empty(), "{pv:?}");
    let (at, _, _) = pv.via.expect("a via");
    assert_ne!(at, p(mm(-3.45), Nm::ZERO), "the via was pushed off the track");
}

/// findings p5 F2-2: in Walk around the via is pushed off the track *before* the head is built to it (`rhWalkBase` / `buildInitialLine`:
/// `BuildInitialTrace( m_p_start, aP + force, .. )`, then `v.SetPos( walkFull.CLastPoint() )`), so the head ends at the via, the next leg
/// starts there, and the commit has the via where the preview drew it -- clear of the GND track, where the cursor's point was 150 um short.
#[test]
fn a_walkaround_via_is_committed_where_it_was_pushed_and_the_route_goes_on_from_it() {
    let (design, model) = board(vec![track("t_gnd", "GND", "B.Cu", &[p(mm(-3.0), mm(-4.0)), p(mm(-3.0), mm(4.0))])]);
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::Walkaround;
    router.start(p(mm(-9.0), Nm::ZERO), "F.Cu", mm(0.2)).unwrap();
    router.toggle_via(true, mm(0.6), mm(0.3), Some(1));
    let pv = router.preview(p(mm(-3.45), Nm::ZERO)).expect("a route in progress");
    let (at, _, _) = pv.via.expect("a via");
    assert!(!pv.colliding && at != p(mm(-3.45), Nm::ZERO), "{pv:?}");
    assert_eq!(pv.head.last(), Some(at), "the head ends at the pushed via");
    assert!(router.fix(p(mm(-3.45), Nm::ZERO)).is_some());
    router.preview(p(mm(-3.45), mm(3.0)));
    let commit = router.finish(p(mm(-3.45), mm(3.0))).expect("the route goes on on B.Cu");
    assert_eq!(commit.vias.len(), 1);
    assert_eq!(commit.vias[0].at, at, "committed where it was pushed to");
    let b = commit.tracks.iter().find(|t| t.layer == "B.Cu").expect("a B.Cu leg");
    assert_eq!(b.pts.first(), Some(&at), "the B.Cu leg starts on the via");
    let after = committed(&design, &commit);
    assert!(violations(&after, &model).is_empty(), "the committed board is clean: {:?}", violations(&after, &model));
}

/// findings p5 F2-1: a via that ends the route is committed (`FixRoute`: `if( pl.EndsWithVia() ) .. m_lastNode->Add( std::move( newVia ) )`),
/// whether it was fixed with a click and the route finished on it, or is still armed when the route is finished, or was placed before
/// any segment (an SMD fan-out: out of the pad, V, click, finish).
#[test]
fn a_via_that_ends_the_route_is_committed() {
    let (design, model) = board(vec![]);
    let start = p(mm(-9.0), Nm::ZERO);
    let end = p(mm(-5.0), Nm::ZERO);
    // fixed with the click, finished on it
    let mut router = Router::new(&design, &model);
    router.start(start, "F.Cu", mm(0.2)).unwrap();
    router.toggle_via(true, mm(0.6), mm(0.3), Some(1));
    router.preview(end);
    assert!(router.fix(end).is_some());
    let commit = router.finish(end).expect("finished on the via");
    assert_eq!(commit.vias.iter().map(|v| v.at).collect::<Vec<_>>(), vec![end], "{commit:?}");
    assert_eq!(commit.tracks.len(), 1);
    // armed, and the route finished with the head ending in it
    let mut router = Router::new(&design, &model);
    router.start(start, "F.Cu", mm(0.2)).unwrap();
    router.toggle_via(true, mm(0.6), mm(0.3), Some(1));
    router.preview(end);
    let commit = router.finish(end).expect("finished with the via armed");
    assert_eq!(commit.vias.iter().map(|v| (v.at, v.from_layer.as_str(), v.to_layer.as_str())).collect::<Vec<_>>(), vec![(end, "F.Cu", "B.Cu")], "{commit:?}");
    // placed at the start of the route, before any segment, and the route goes on on B.Cu
    let mut router = Router::new(&design, &model);
    router.start(start, "F.Cu", mm(0.2)).unwrap();
    router.toggle_via(true, mm(0.6), mm(0.3), Some(1));
    assert!(router.fix(start).is_some());
    router.preview(p(mm(-9.0), mm(3.0)));
    let commit = router.finish(p(mm(-9.0), mm(3.0))).expect("the B.Cu leg");
    assert_eq!(commit.vias.iter().map(|v| v.at).collect::<Vec<_>>(), vec![start], "{commit:?}");
    assert!(commit.tracks.iter().all(|t| t.layer == "B.Cu"));
    // and Backspace takes the via away with the run it ended
    let mut router = Router::new(&design, &model);
    router.start(start, "F.Cu", mm(0.2)).unwrap();
    router.toggle_via(true, mm(0.6), mm(0.3), Some(1));
    router.preview(end);
    assert!(router.fix(end).is_some());
    assert!(router.undo_last_segment());
    // (Backspace arms the via again, `UnfixRoute`; V takes it back)
    router.toggle_via(false, mm(0.6), mm(0.3), None);
    router.preview(p(mm(-7.0), Nm::ZERO));
    let commit = router.finish(p(mm(-7.0), Nm::ZERO)).expect("a plain F.Cu route");
    assert!(commit.vias.is_empty(), "{commit:?}");
    assert!(commit.tracks.iter().all(|t| t.layer == "F.Cu"), "back on the layer the undone run was on: {commit:?}");
}

/// Backspace after a leg that ended in a via takes the leg back with the via armed again (`UnfixRoute`: `m_placingVia =
/// st.pts[0].placingVia`), on the leg's own layer: the next click places the via again and the route goes on on the other layer.
#[test]
fn backspace_takes_a_via_leg_back_with_the_via_armed_again() {
    let (design, model) = board(vec![]);
    let mut router = Router::new(&design, &model);
    router.start(p(mm(-9.0), Nm::ZERO), "F.Cu", mm(0.2)).unwrap();
    router.toggle_via(true, mm(0.6), mm(0.3), Some(1));
    router.preview(p(mm(-5.0), Nm::ZERO));
    assert!(router.fix(p(mm(-5.0), Nm::ZERO)).is_some());
    assert_eq!(router.pending_via_layer(), None, "the click placed it");
    assert!(router.undo_last_segment());
    assert_eq!(router.pending_via_layer(), Some(1), "armed again, to B.Cu");
    let pv = router.preview(p(mm(-5.0), Nm::ZERO)).unwrap();
    assert!(pv.via.is_some() && pv.head.layer == 0, "the head is on F.Cu and ends in the via: {pv:?}");
    assert!(router.fix(p(mm(-5.0), Nm::ZERO)).is_some());
    router.preview(p(mm(-5.0), mm(3.0)));
    let commit = router.finish(p(mm(-5.0), mm(3.0))).unwrap();
    assert_eq!(commit.vias.iter().map(|v| v.at).collect::<Vec<_>>(), vec![p(mm(-5.0), Nm::ZERO)]);
    assert!(commit.tracks.iter().any(|t| t.layer == "B.Cu"));
}

/// r5 R5-2: a dragged track's replacement names the track as its source (`NODE::AssembleLine`: `pl.SetSourceItem( aSeg->GetSourceItem() )`), so the
/// commit gives it the track's mask opening and group.
#[test]
fn a_dragged_tracks_replacement_names_it_as_its_source() {
    let (design, model) = board(vec![track("t_gnd", "GND", "F.Cu", &[p(mm(-3.0), mm(-4.0)), p(mm(-3.0), mm(4.0))])]);
    let mut router = Router::new(&design, &model);
    router.settings.mode = Mode::Shove;
    router.drag_start(p(mm(-3.0), Nm::ZERO), "F.Cu").expect("drags the GND track");
    let commit = router.drag_finish(p(mm(-2.0), Nm::ZERO)).expect("commits");
    assert_eq!(commit.remove_track_ids, vec!["t_gnd".to_string()]);
    assert!(!commit.tracks.is_empty() && (0..commit.tracks.len()).all(|i| commit.source_of(i) == Some("t_gnd")), "{:?}", commit.track_sources);
}
