//! The router keeps the clearances the board's rules give (round 2, R-1: `PNS_PCBNEW_RULE_RESOLVER::Clearance` asks the DRC engine, so a pad's
//! local clearance, a custom rule and a net tie count) and keeps out of keepout rule areas (R-2: `syncZone`, `IsKeepout`). Each case routes in
//! Walk around mode as the studio does (`Router`), checks the committed copper here, and -- under `EDA_SLOW_TESTS=1` -- has kicad-cli's DRC of
//! the board with the route on it judge it (`eda_kicad_engine::drc`, the derived `.kicad_pcb`, `.kicad_pro` and `.kicad_dru`): no
//! `clearance` or `items_not_allowed` violation of the new track.

use eda_drc::kimath::Shape;
use eda_model::footprint::{Footprint, Pad, PadKind, PadShape};
use eda_model::ir::{Design, DrawingsSection, FootprintExtra, FootprintInstance, FootprintZoneOverrides, Nm, PadZoneOverride, PlacementSection, Point, Provenance, RoutingSection, Side, Zone};
use eda_model::units::mm;
use eda_model::{ConstraintModel, Net, Part, Pin, PinKind};
use eda_pns::router::{RouteCommit, Router};
use eda_pns::settings::Mode;

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

/// A 30 x 20 mm board: `PAD1` footprints (one 1 mm square SMD pad) at `fps`, on the nets `nets` (`("SIG", ["R1.1", "R3.1"])`), and a two-pad
/// `TIE2` footprint (two 1 mm squares side by side, touching) when asked.
fn board(fps: &[(&str, &str, Point)], nets: &[(&str, &[&str])]) -> (Design, ConstraintModel) {
    let parts = fps.iter().map(|(r, f, _)| part(r, f, match *f { "TIE2" => 2, "MH3" => 0, _ => 1 })).collect();
    let th = Pad { opposite_side: false, ext: Default::default(), number: "1".into(), at: (Nm::ZERO, Nm::ZERO), size: (mm(1.6), mm(1.6)), shape: PadShape::Circle, kind: PadKind::ThroughHole, drill: Some(mm(0.8)), drill_slot: None, rot: 0, roundrect_ratio: None };
    let model = ConstraintModel {
        parts,
        nets: nets.iter().map(|(n, pins)| Net { name: n.to_string(), pins: pins.iter().map(|s| s.to_string()).collect() }).collect(),
        footprints: vec![
            footprint("PAD1", vec![smd("1", (Nm::ZERO, Nm::ZERO))]),
            footprint("TIE2", vec![smd("1", (Nm::ZERO, Nm::ZERO)), smd("2", (mm(1.0), Nm::ZERO))]),
            // a through-hole pad: 1.6 mm round, 0.8 mm hole
            footprint("TH1", vec![th]),
            // a 3 mm mounting hole: a non-plated pad of the hole's size
            footprint("MH3", vec![Pad { opposite_side: false, ext: Default::default(), number: String::new(), at: (Nm::ZERO, Nm::ZERO), size: (mm(3.0), mm(3.0)), shape: PadShape::Circle, kind: PadKind::NonPlatedHole, drill: Some(mm(3.0)), drill_slot: None, rot: 0, roundrect_ratio: None }]),
        ],
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
        placement: Some(PlacementSection { outline: vec![p(0.0, 0.0), p(30.0, 0.0), p(30.0, 20.0), p(0.0, 20.0)], footprints: fps.iter().map(|(r, _, at)| FootprintInstance { id: r.to_string(), at: *at, rot: 0, side: Side::Top, label: Default::default() }).collect(), modules: vec![] }),
        routing: Some(RoutingSection { tracks: vec![], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }),
        drawings: Some(DrawingsSection::default()),
    };
    (design, model)
}

/// Route `from` -> `to` in Walk around mode, as the studio does, and commit.
fn route(design: &Design, model: &ConstraintModel, from: Point, to: Point) -> RouteCommit {
    route_on(design, model, from, to, "F.Cu")
}

/// [`route`] on `layer`.
fn route_on(design: &Design, model: &ConstraintModel, from: Point, to: Point, layer: &str) -> RouteCommit {
    let mut router = Router::new(design, model);
    router.settings.mode = Mode::Walkaround;
    router.start(from, layer, mm(0.2)).expect("starts on the pad");
    let pv = router.preview(to).expect("a session");
    assert!(!pv.colliding, "the route is clear: {:?}", pv.head.pts);
    router.finish(to).expect("commits")
}

fn apply(design: &Design, commit: &RouteCommit) -> Design {
    let mut d = design.clone();
    let rt = d.routing.as_mut().unwrap();
    rt.tracks.retain(|t| !commit.remove_track_ids.contains(&t.id));
    rt.tracks.extend(commit.tracks.iter().cloned());
    rt.vias.extend(commit.vias.iter().cloned());
    d.assign_missing_ids();
    d
}

/// The least gap from the committed tracks' copper to a 1 mm square pad at `c`.
fn gap_to_pad(commit: &RouteCommit, c: Point) -> f64 {
    let pad = Shape::Rect { x0: c.x - mm(0.5), y0: c.y - mm(0.5), x1: c.x + mm(0.5), y1: c.y + mm(0.5) };
    let pad = &pad;
    commit.tracks.iter().flat_map(|t| t.pts.windows(2).map(move |w| Shape::Stadium { a: w[0], b: w[1], r: t.width / 2 }.gap_to(pad))).fold(f64::MAX, f64::min)
}

/// kicad-cli's DRC of the board with the route on it, under `EDA_SLOW_TESTS=1`: the violations of `kinds`. `None` when the slow tier is off.
fn kicad_violations(design: &Design, model: &ConstraintModel, kinds: &[&str]) -> Option<Vec<String>> {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("EDA_SLOW_TESTS not set: kicad-cli's DRC is not asked");
        return None;
    }
    static RUN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let run = RUN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let work = std::env::temp_dir().join(format!("pns-rules-kicad-{}-{run}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let report = eda_kicad_engine::drc(design, model, &work, false).unwrap_or_else(|e| panic!("kicad-cli DRC: {e:?}"));
    let _ = std::fs::remove_dir_all(&work);
    Some(report.violations.iter().filter(|v| kinds.contains(&v.kind.as_str()) && !v.excluded).map(|v| format!("{}: {}", v.kind, v.description)).collect())
}

/// R-1: a 0.6 mm clearance on pad R2.1 (`PAD::GetLocalClearance`, "Local override on %s"), where the net class asks 0.2 mm. The straight way from
/// R1.1 to R3.1 passes 0.4 mm from R2.1's copper: the router goes round it at 0.6 mm, and kicad-cli finds no clearance violation.
#[test]
fn a_pad_local_clearance_holds_a_route_off() {
    let (r1, r2, r3) = (p(5.0, 10.0), p(15.0, 11.0), p(25.0, 10.0));
    let (mut design, model) = board(&[("R1", "PAD1", r1), ("R2", "PAD1", r2), ("R3", "PAD1", r3)], &[("SIG", &["R1.1", "R3.1"]), ("GND", &["R2.1"])]);
    design.drawings.as_mut().unwrap().zone_overrides.push(FootprintZoneOverrides { id: "R2".into(), footprint: None, pads: vec![PadZoneOverride { number: "1".into(), clearance: Some(mm(0.6)), ..Default::default() }] });
    let commit = route(&design, &model, r1, r3);
    let gap = gap_to_pad(&commit, r2);
    assert!(gap >= 599_500.0, "the track keeps {gap} nm from R2.1; its local clearance is 0.6 mm: {:?}", commit.tracks);
    // the net class alone would have let it run straight
    let (plain, plain_model) = board(&[("R1", "PAD1", r1), ("R2", "PAD1", r2), ("R3", "PAD1", r3)], &[("SIG", &["R1.1", "R3.1"]), ("GND", &["R2.1"])]);
    let class_only = route(&plain, &plain_model, r1, r3);
    assert!(gap_to_pad(&class_only, r2) < 599_500.0);
    if let Some(found) = kicad_violations(&apply(&design, &commit), &model, &["clearance"]) {
        assert!(found.is_empty(), "{found:#?}");
        // and kicad-cli does see the pad's clearance: the route the net class alone gives is a violation on this board
        assert!(!kicad_violations(&apply(&design, &class_only), &model, &["clearance"]).unwrap().is_empty());
    }
}

/// R-1: the same, from a footprint's own clearance (`FOOTPRINT::GetLocalClearance`, which a pad without one inherits).
#[test]
fn a_footprint_clearance_holds_a_route_off() {
    let (r1, r2, r3) = (p(5.0, 10.0), p(15.0, 11.0), p(25.0, 10.0));
    let (mut design, model) = board(&[("R1", "PAD1", r1), ("R2", "PAD1", r2), ("R3", "PAD1", r3)], &[("SIG", &["R1.1", "R3.1"]), ("GND", &["R2.1"])]);
    design.drawings.as_mut().unwrap().zone_overrides.push(FootprintZoneOverrides { id: "R2".into(), footprint: Some(eda_model::ir::FootprintZoneFacts { zone_connection: None, clearance: Some(mm(0.6)) }), pads: vec![] });
    let commit = route(&design, &model, r1, r3);
    let gap = gap_to_pad(&commit, r2);
    assert!(gap >= 599_500.0, "{gap}: {:?}", commit.tracks);
    if let Some(found) = kicad_violations(&apply(&design, &commit), &model, &["clearance"]) {
        assert!(found.is_empty(), "{found:#?}");
        let (plain, plain_model) = board(&[("R1", "PAD1", r1), ("R2", "PAD1", r2), ("R3", "PAD1", r3)], &[("SIG", &["R1.1", "R3.1"]), ("GND", &["R2.1"])]);
        let class_only = route(&plain, &plain_model, r1, r3);
        assert!(!kicad_violations(&apply(&design, &class_only), &model, &["clearance"]).unwrap().is_empty(), "kicad-cli sees the footprint's clearance");
    }
}

/// R-1: a custom rule (`.kicad_dru`): `(rule hv (constraint clearance (min 1mm)) (condition "A.NetName == 'HV'"))`. R2.1 is on HV; the
/// route keeps 1 mm from it, and kicad-cli, which reads the same rules, finds nothing.
#[test]
fn a_custom_rule_holds_a_route_off() {
    let (r1, r2, r3) = (p(5.0, 10.0), p(15.0, 11.2), p(25.0, 10.0));
    let (design, mut model) = board(&[("R1", "PAD1", r1), ("R2", "PAD1", r2), ("R3", "PAD1", r3)], &[("SIG", &["R1.1", "R3.1"]), ("HV", &["R2.1"])]);
    model.board.custom_rules_text = Some("(version 1)\n(rule hv\n  (constraint clearance (min 1mm))\n  (condition \"A.NetName == 'HV'\"))\n".into());
    let commit = route(&design, &model, r1, r3);
    let gap = gap_to_pad(&commit, r2);
    assert!(gap >= 999_500.0, "{gap}: {:?}", commit.tracks);
    if let Some(found) = kicad_violations(&apply(&design, &commit), &model, &["clearance"]) {
        assert!(found.is_empty(), "{found:#?}");
        let (_, plain_model) = board(&[("R1", "PAD1", r1), ("R2", "PAD1", r2), ("R3", "PAD1", r3)], &[("SIG", &["R1.1", "R3.1"]), ("HV", &["R2.1"])]);
        let class_only = route(&design, &plain_model, r1, r3);
        assert!(gap_to_pad(&class_only, r2) < 999_500.0);
        assert!(!kicad_violations(&apply(&design, &class_only), &model, &["clearance"]).unwrap().is_empty(), "kicad-cli reads the rule");
    }
}

/// R-1: a net tie. T1 is a `TIE2` net tie (`net_tie_pad_groups "1,2"`) whose pad 1 is on SIG and pad 2 on GND, the two touching at x = 15.5.
/// A SIG track that comes down to pad 1 next to the edge they share runs within 0.2 mm of pad 2, but every point of pad 2 it is near is on pad
/// 1 too (`IsNetTieExclusion`): KiCad's router lets it, and so does its DRC. Without the tie the track is held off pad 2. (The track runs 0.2 mm
/// left of the shared edge: its copper is 0.1 mm from pad 2, inside the 0.2 mm clearance, without touching it.)
#[test]
fn a_net_tie_lets_a_track_touch_the_other_pad() {
    let (t1, r1) = (p(15.0, 10.0), p(15.3, 4.0));
    let (mut design, model) = board(&[("T1", "TIE2", t1), ("R1", "PAD1", r1)], &[("SIG", &["T1.1", "R1.1"]), ("GND", &["T1.2"])]);
    design.drawings.as_mut().unwrap().footprint_extras.push(FootprintExtra { id: "T1".into(), net_tie_pad_groups: vec!["1,2".into()], ..Default::default() });
    // from R1 straight down to pad 1, 0.2 mm left of the pads' shared edge
    let to = p(15.3, 10.0);
    let tied = route(&design, &model, r1, to);
    let pad2 = p(16.0, 10.0);
    let gap = gap_to_pad(&tied, pad2);
    assert!(gap < 150_000.0, "the track runs {gap} nm from pad 2, inside the 0.2 mm clearance the tie waives: {:?}", tied.tracks);
    // the same footprint without the tie: pad 2 is an ordinary GND pad, and the route keeps the clearance from it
    let (plain, plain_model) = board(&[("T1", "TIE2", t1), ("R1", "PAD1", r1)], &[("SIG", &["T1.1", "R1.1"]), ("GND", &["T1.2"])]);
    let mut router = Router::new(&plain, &plain_model);
    router.settings.mode = Mode::Walkaround;
    router.start(r1, "F.Cu", mm(0.2)).unwrap();
    let pv = router.preview(to).unwrap();
    let held = RouteCommit { tracks: vec![eda_model::ir::Track { id: String::new(), net: "SIG".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: pv.head.pts.clone(), arc_mid_offset: None }], ..Default::default() };
    assert!(pv.colliding || gap_to_pad(&held, pad2) >= 199_500.0, "without the tie pad 2 holds the track off: {:?}", pv.head.pts);
    if let Some(found) = kicad_violations(&apply(&design, &tied), &model, &["clearance", "shorting_items"]) {
        assert!(found.is_empty(), "{found:#?}");
        // the same track on the board without the tie is a clearance violation to kicad-cli
        assert!(!kicad_violations(&apply(&plain, &tied), &plain_model, &["clearance"]).unwrap().is_empty());
    }
}

fn rule_area(id: &str, outline: Vec<Point>, tracks: bool, vias: bool) -> Zone {
    Zone { id: id.into(), net: String::new(), layer: "F.Cu".into(), outline, is_rule_area: true, keepout_tracks: tracks, keepout_vias: vias, ..Default::default() }
}

/// R-2: a rule area that keeps out tracks (`syncZone`'s triangles, `IsKeepout`'s exact boundary) across the straight way between two SIG pads:
/// the route goes round it, and kicad-cli finds no `items_not_allowed`. Without the area the route is straight.
#[test]
fn a_keepout_rule_area_blocks_tracks() {
    let (r1, r2) = (p(5.0, 10.0), p(25.0, 10.0));
    let (mut design, model) = board(&[("R1", "PAD1", r1), ("R2", "PAD1", r2)], &[("SIG", &["R1.1", "R2.1"])]);
    let area = vec![p(13.0, 7.0), p(17.0, 7.0), p(17.0, 13.0), p(13.0, 13.0)];
    design.routing.as_mut().unwrap().zones.push(rule_area("ko1", area.clone(), true, false));
    let commit = route(&design, &model, r1, r2);
    let keepout = Shape::Polygon { pts: area };
    for t in &commit.tracks {
        for w in t.pts.windows(2) {
            assert!(Shape::Stadium { a: w[0], b: w[1], r: t.width / 2 }.collides(&keepout, Nm::ZERO).is_none(), "{w:?} is in the keepout: {:?}", commit.tracks);
        }
    }
    let (open, open_model) = board(&[("R1", "PAD1", r1), ("R2", "PAD1", r2)], &[("SIG", &["R1.1", "R2.1"])]);
    let straight = route(&open, &open_model, r1, r2);
    assert!(straight.tracks.iter().all(|t| t.pts.iter().all(|q| q.y == mm(10.0))), "{:?}", straight.tracks);
    if let Some(found) = kicad_violations(&apply(&design, &commit), &model, &["items_not_allowed", "clearance"]) {
        assert!(found.is_empty(), "{found:#?}");
        // the straight route through the area is what kicad-cli would flag
        assert!(!kicad_violations(&apply(&design, &straight), &model, &["items_not_allowed"]).unwrap().is_empty());
    }
}

/// No committed track of `commit` crosses the polygon `area`.
fn assert_outside(commit: &RouteCommit, area: &[Point]) {
    let keepout = Shape::Polygon { pts: area.to_vec() };
    for t in &commit.tracks {
        for w in t.pts.windows(2) {
            assert!(Shape::Stadium { a: w[0], b: w[1], r: t.width / 2 }.collides(&keepout, Nm::ZERO).is_none(), "{w:?} is in the keepout: {:?}", commit.tracks);
        }
    }
}

/// R3-1 (`syncZone` adds the triangles on each copper layer of `GetLayerSet()`): a rule area on F.Cu and B.Cu keeps a route on B.Cu out as
/// well. Two through-hole pads, the area across the straight way between them, the route on B.Cu: it goes round, and kicad-cli finds no
/// `items_not_allowed`; the straight route through it on B.Cu is what kicad-cli flags.
#[test]
fn a_keepout_on_both_copper_layers_blocks_a_route_on_the_back() {
    let (r1, r2) = (p(5.0, 10.0), p(25.0, 10.0));
    let (mut design, model) = board(&[("J1", "TH1", r1), ("J2", "TH1", r2)], &[("SIG", &["J1.1", "J2.1"])]);
    let area = vec![p(13.0, 7.0), p(17.0, 7.0), p(17.0, 13.0), p(13.0, 13.0)];
    let mut zone = rule_area("ko3", area.clone(), true, false);
    zone.set_layer_set(&["F.Cu", "B.Cu"]);
    design.routing.as_mut().unwrap().zones.push(zone);
    let commit = route_on(&design, &model, r1, r2, "B.Cu");
    assert!(commit.tracks.iter().all(|t| t.layer == "B.Cu"), "{:?}", commit.tracks);
    assert_outside(&commit, &area);
    if let Some(found) = kicad_violations(&apply(&design, &commit), &model, &["items_not_allowed", "clearance"]) {
        assert!(found.is_empty(), "{found:#?}");
        let straight = RouteCommit { tracks: vec![eda_model::ir::Track { id: String::new(), net: "SIG".into(), pins: vec![], layer: "B.Cu".into(), width: mm(0.2), pts: vec![r1, r2], arc_mid_offset: None }], ..Default::default() };
        assert!(!kicad_violations(&apply(&design, &straight), &model, &["items_not_allowed"]).unwrap().is_empty(), "kicad-cli keeps B.Cu out too");
    }
}

/// R3-5 (`CacheTriangulation` of the outline with its holes): a 10 x 10 mm keepout of tracks with a 4 x 4 mm hole, and two pads in the hole: the
/// route between them is straight, and kicad-cli finds nothing.
#[test]
fn a_route_may_cross_the_hole_of_a_keepout() {
    let (r1, r2) = (p(13.0, 10.0), p(17.0, 10.0));
    let (mut design, model) = board(&[("R1", "PAD1", r1), ("R2", "PAD1", r2)], &[("SIG", &["R1.1", "R2.1"])]);
    let mut zone = rule_area("ko4", vec![p(10.0, 5.0), p(20.0, 5.0), p(20.0, 15.0), p(10.0, 15.0)], true, false);
    zone.holes = vec![vec![p(12.0, 8.0), p(18.0, 8.0), p(18.0, 12.0), p(12.0, 12.0)]];
    design.routing.as_mut().unwrap().zones.push(zone);
    let commit = route(&design, &model, r1, r2);
    assert!(commit.tracks.iter().all(|t| t.pts.iter().all(|q| q.y == mm(10.0))), "straight across the hole: {:?}", commit.tracks);
    if let Some(found) = kicad_violations(&apply(&design, &commit), &model, &["items_not_allowed", "clearance"]) {
        assert!(found.is_empty(), "{found:#?}");
        // the hole is in the derived board: a track across the area's body, outside the hole, is flagged
        let body = RouteCommit { tracks: vec![eda_model::ir::Track { id: String::new(), net: "SIG".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![p(13.0, 6.0), p(17.0, 6.0)], arc_mid_offset: None }], ..Default::default() };
        assert!(!kicad_violations(&apply(&design, &body), &model, &["items_not_allowed"]).unwrap().is_empty());
    }
}

/// R-2: a rule area that keeps out vias only: a track crosses it as if it were not there, but a via in it collides (`checkKeepout`:
/// `GetDoNotAllowVias() && aOther->Type() == PCB_VIA_T`), and one outside it does not.
#[test]
fn a_vias_only_keepout_lets_tracks_through_but_not_vias() {
    let (r1, r2) = (p(5.0, 10.0), p(25.0, 10.0));
    let (mut design, model) = board(&[("R1", "PAD1", r1), ("R2", "PAD1", r2)], &[("SIG", &["R1.1", "R2.1"])]);
    design.routing.as_mut().unwrap().zones.push(rule_area("ko2", vec![p(13.0, 7.0), p(17.0, 7.0), p(17.0, 13.0), p(13.0, 13.0)], false, true));
    let straight = route(&design, &model, r1, r2);
    assert!(straight.tracks.iter().all(|t| t.pts.iter().all(|q| q.y == mm(10.0))), "the track crosses the area: {:?}", straight.tracks);
    // a via at the end of a head that ends in the area, and one that ends before it
    let via_at = |to: Point| {
        let mut router = Router::new(&design, &model);
        router.settings.mode = Mode::MarkObstacles;
        router.start(r1, "F.Cu", mm(0.2)).unwrap();
        router.toggle_via(true, mm(0.6), mm(0.3), None);
        router.preview(to).unwrap().colliding
    };
    assert!(via_at(p(15.0, 10.0)), "a via in the area collides with it");
    assert!(!via_at(p(10.0, 10.0)), "one outside it does not");
    if let Some(found) = kicad_violations(&apply(&design, &straight), &model, &["items_not_allowed"]) {
        assert!(found.is_empty(), "{found:#?}");
        // a via there is what kicad-cli flags
        let mut with_via = apply(&design, &straight);
        with_via.routing.as_mut().unwrap().vias.push(eda_model::ir::Via { id: String::new(), net: "SIG".into(), at: p(15.0, 10.0), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        with_via.assign_missing_ids();
        assert!(!kicad_violations(&with_via, &model, &["items_not_allowed"]).unwrap().is_empty());
    }
}

/// R-1, hole clearance (`CT_HOLE_CLEARANCE`, which `Clearance` asks of a hole and anything): a 3 mm mounting hole whose copper the net
/// class keeps 0.2 mm from, but whose hole the board keeps 0.25 mm from (`m_HoleClearance`, KiCad's default). The straight way passes
/// 0.22 mm from it: the router goes round at 0.25 mm, and kicad-cli finds no `hole_clearance`.
#[test]
fn a_hole_keeps_the_hole_clearance() {
    let (r1, mh, r3) = (p(5.0, 10.0), p(15.0, 11.82), p(25.0, 10.0));
    let (design, mut model) = board(&[("R1", "PAD1", r1), ("H1", "MH3", mh), ("R3", "PAD1", r3)], &[("SIG", &["R1.1", "R3.1"])]);
    assert_eq!(model.board.hole_clearance_nm, mm(0.25), "KiCad's default hole clearance");
    // the board's constraints written as they are (a board built from an intent writes its class clearance as the hole clearance instead)
    model.board.constraints_explicit = true;
    let commit = route(&design, &model, r1, r3);
    let hole = Shape::Circle { c: mh, r: mm(1.5) };
    let gap = commit.tracks.iter().flat_map(|t| t.pts.windows(2).map(|w| Shape::Stadium { a: w[0], b: w[1], r: t.width / 2 }.gap_to(&hole)).collect::<Vec<_>>()).fold(f64::MAX, f64::min);
    assert!(gap >= 249_500.0, "the track keeps {gap} nm from the hole: {:?}", commit.tracks);
    if let Some(found) = kicad_violations(&apply(&design, &commit), &model, &["hole_clearance", "clearance"]) {
        assert!(found.is_empty(), "{found:#?}");
        // the straight track kicad-cli flags
        let straight = RouteCommit { tracks: vec![eda_model::ir::Track { id: String::new(), net: "SIG".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![r1, r3], arc_mid_offset: None }], ..Default::default() };
        assert!(!kicad_violations(&apply(&design, &straight), &model, &["hole_clearance"]).unwrap().is_empty());
    }
}

/// R-1, hole to hole (`CT_HOLE_TO_HOLE`, whatever the nets: "Hole-to-hole collisions don't have anything to do with nets"): a via next to a via
/// of its own net collides when their holes are nearer than the board's 0.25 mm, and not when they are not; the same via in the same place
/// is its own copy, not a collision (`shouldWeConsiderHoleCollisions`).
#[test]
fn a_via_keeps_the_hole_to_hole_distance_from_a_via_of_its_own_net() {
    let (mut design, model) = board(&[], &[("SIG", &[])]);
    design.routing.as_mut().unwrap().vias.push(eda_model::ir::Via { id: "v1".into(), net: "SIG".into(), at: p(10.0, 10.0), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
    let (node, layers) = eda_pns::from_ir::build_node(&design, &model);
    let via = |x: f64| eda_pns::item::Via { net: eda_pns::item::net_of("SIG"), layers: layers.all(), pos: p(x, 10.0), diameter: mm(0.6), drill: mm(0.3), source_via: None, locked: false, is_virtual: false };
    let hits = |x: f64| node.via_colliding(&via(x), &model.board, &[], eda_pns::node::QueryOpts::default()).len();
    assert_eq!(hits(10.5), 1, "holes 0.2 mm apart");
    assert_eq!(hits(10.56), 0, "holes 0.26 mm apart");
    assert_eq!(hits(10.0), 0, "the via's own copy");
}

/// The rule resolver answers a pair of plain items -- tracks, vias, their holes, pads with no clearance of their own, the board edge -- on a
/// board whose clearance rules are its board setup's and its net classes' without asking the engine (`RuleResolver::clearance`'s plain
/// answer). Held here to the engine's own answer (`engine_clearance`, `EvalRules`) for every pair of a sample of the items of KiCad's QA
/// boards (the router's regression boards, and three boards of net classes, one of them composite), with and without the epsilon.
#[test]
fn the_plain_answer_is_the_engines_on_kicads_boards() {
    let Some(data) = std::env::var_os("KICAD_QA_DATA").map(std::path::PathBuf::from).or_else(|| std::env::var_os("KICAD_SRC_DIR").map(|s| std::path::PathBuf::from(s).join("qa/data"))) else {
        eprintln!("KiCad QA corpus not found: skipping");
        return;
    };
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("EDA_SLOW_TESTS not set: skipping");
        return;
    }
    let pcbnew = data.join("pcbnew");
    let mut boards: Vec<std::path::PathBuf> = std::fs::read_dir(pcbnew.join("pns_regressions/boards")).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "kicad_pcb")).collect();
    boards.sort();
    boards.extend(["issue16566", "issue6879", "multinetclasses_drc"].iter().map(|n| pcbnew.join(format!("{n}.kicad_pcb"))));
    let mut compared = 0usize;
    for path in boards {
        let text = std::fs::read_to_string(&path).unwrap();
        let (mut design, mut model, _) = eda_kicad::import_kicad_pcb(&text).unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
        if let Ok(pro) = std::fs::read_to_string(path.with_extension("kicad_pro")) {
            eda_kicad::merge_project_net_classes(&mut model, &pro);
            eda_kicad::merge_project_design_rules(&mut model, &pro);
        }
        model.board.custom_rules_text = None;
        design.assign_missing_ids();
        let (node, layers) = eda_pns::from_ir::build_node(&design, &model);
        let r = node.rule_resolver().expect("a world from the IR has its rules");
        let items: Vec<&eda_pns::item::Item> = node.iter().map(|(_, it)| it).collect();
        let step = (items.len() / 120).max(1);
        let sample: Vec<&eda_pns::item::Item> = items.iter().step_by(step).copied().collect();
        // and heads of every net of the sample, a track on each layer and a via
        let nets: Vec<eda_pns::item::Net> = sample.iter().map(|it| it.net().clone()).collect();
        let vias: Vec<eda_pns::item::Via> = nets.iter().map(|n| eda_pns::item::Via { net: n.clone(), layers: layers.all(), pos: p(1.0, 1.0), diameter: mm(0.6), drill: mm(0.3), source_via: None, locked: false, is_virtual: false }).collect();
        let mut sides: Vec<eda_pns::node::Side> = sample.iter().map(|it| eda_pns::node::Side::Item(it)).collect();
        for it in &sample {
            if let eda_pns::item::Item::Solid(s) = it {
                if r.pad_hole(s).is_some() {
                    sides.push(eda_pns::node::Side::PadHole(s));
                }
            }
        }
        for (n, v) in nets.iter().zip(&vias).step_by(4) {
            sides.push(eda_pns::node::Side::track(n, 0));
            sides.push(eda_pns::node::Side::track(n, layers.count() as i32 - 1));
            sides.push(eda_pns::node::Side::via(v));
            sides.push(eda_pns::node::Side::via_hole(v));
        }
        for (i, a) in sides.iter().enumerate() {
            for b in sides.iter().skip(i + 1).step_by(3) {
                for eps in [false, true] {
                    assert_eq!(r.clearance(a, b, eps), r.engine_clearance(a, b, eps), "{}: {a:?} / {b:?} (epsilon {eps})", path.display());
                    compared += 1;
                }
            }
        }
        eprintln!("{}: plain {}, {} sides", path.display(), r.is_plain(), sides.len());
    }
    assert!(compared > 10_000, "{compared} pairs compared");
}
