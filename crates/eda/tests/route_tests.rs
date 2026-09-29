use eda_model::ir::{Design, FootprintInstance, PlacementSection, Point, Provenance, Side};
use eda_model::footprint::{Footprint, Pad, PadKind, PadShape};
use eda_model::{ConstraintModel, Net, Part, Pin, PinKind};
use eda::{route, RouteRules};

fn provenance() -> Provenance {
    Provenance { engine_version: "test".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] }
}

fn part(reference: &str, npins: usize) -> Part {
    Part {
        reference: reference.into(),
        mpn: None,
        value: None,
        package: None,
        footprint: Some(format!("LIN{npins}")),
        pins: (1..=npins)
            .map(|i| Pin { number: i.to_string(), name: None, kind: PinKind::Signal })
            .collect(),
        body_um: None,
        edge: None,
    }
}

/// Test footprint: `n` 500x500 SMD pads on a 1000um pitch along +x from the origin.
fn lin(n: usize) -> Footprint {
    Footprint {
        name: format!("LIN{n}"),
        pads: (0..n)
            .map(|i| Pad { number: (i + 1).to_string(), at: (i as i64 * 1000, 0), size: (500, 500), shape: PadShape::Rect, kind: PadKind::Smd, drill: None })
            .collect(),
        courtyard: None,
    }
}

fn model(parts: Vec<Part>, nets: Vec<Net>) -> ConstraintModel {
    ConstraintModel { parts, nets, footprints: vec![lin(1), lin(2), lin(3), lin_th(1)], ..Default::default() }
}

/// Like `lin`, but `n` round through-hole pads (exist on every copper
/// layer) instead of SMD. Named `LINTH{n}` so it doesn't collide with
/// `lin`'s footprint names.
fn lin_th(n: usize) -> Footprint {
    Footprint {
        name: format!("LINTH{n}"),
        pads: (0..n)
            .map(|i| Pad {
                number: (i + 1).to_string(),
                at: (i as i64 * 1000, 0),
                size: (800, 800),
                shape: PadShape::Circle,
                kind: PadKind::ThroughHole,
                drill: Some(400),
            })
            .collect(),
        courtyard: None,
    }
}

fn part_th(reference: &str, npins: usize) -> Part {
    Part {
        reference: reference.into(),
        mpn: None,
        value: None,
        package: None,
        footprint: Some(format!("LINTH{npins}")),
        pins: (1..=npins)
            .map(|i| Pin { number: i.to_string(), name: None, kind: PinKind::Signal })
            .collect(),
        body_um: None,
        edge: None,
    }
}

fn fp(id: &str, x: i64, y: i64) -> FootprintInstance {
    FootprintInstance { id: id.into(), at: Point { x, y }, rot: 0, side: Side::Top, label: Default::default() }
}

fn fp_side(id: &str, x: i64, y: i64, side: Side) -> FootprintInstance {
    FootprintInstance { id: id.into(), at: Point { x, y }, rot: 0, side, label: Default::default() }
}

fn design(outline: Vec<Point>, footprints: Vec<FootprintInstance>) -> Design {
    Design {
        schema: 1,
        provenance: provenance(),
        schematic: None,
        placement: Some(PlacementSection { outline, footprints, modules: Vec::new() }),
        routing: None,
    }
}

fn rect(w: i64, h: i64) -> Vec<Point> {
    vec![
        Point { x: 0, y: 0 },
        Point { x: w, y: 0 },
        Point { x: w, y: h },
        Point { x: 0, y: h },
    ]
}

// -------------------------------------------------------------- fixture 1: 2 parts, 2 nets
fn fixture_2p2n() -> (Design, ConstraintModel) {
    let d = design(rect(20_000, 10_000), vec![fp("U1", 2_000, 5_000), fp("U2", 15_000, 5_000)]);
    let m = model(vec![part("U1", 2), part("U2", 2)], vec![
            Net { name: "NET1".into(), pins: vec!["U1.1".into(), "U2.1".into()] },
            Net { name: "NET2".into(), pins: vec!["U1.2".into(), "U2.2".into()] },
        ]);
    (d, m)
}

// -------------------------------------------------------------- fixture 2: 4 parts, 6 nets crossing
fn fixture_4p6n() -> (Design, ConstraintModel) {
    let d = design(
        rect(30_000, 30_000),
        vec![
            fp("U1", 3_000, 3_000),
            fp("U2", 25_000, 3_000),
            fp("U3", 3_000, 25_000),
            fp("U4", 25_000, 25_000),
        ],
    );
    let parts = vec![part("U1", 3), part("U2", 3), part("U3", 3), part("U4", 3)];
    let nets = vec![
        Net { name: "N1".into(), pins: vec!["U1.1".into(), "U2.1".into()] },
        Net { name: "N2".into(), pins: vec!["U1.2".into(), "U3.1".into()] },
        Net { name: "N3".into(), pins: vec!["U2.2".into(), "U4.1".into()] },
        Net { name: "N4".into(), pins: vec!["U3.2".into(), "U4.2".into()] },
        Net { name: "N5".into(), pins: vec!["U1.3".into(), "U4.3".into()] },
        Net { name: "N6".into(), pins: vec!["U2.3".into(), "U3.3".into()] },
    ];
    let m = model(parts, nets);
    (d, m)
}

// -------------------------------------------------------------- fixture 3: forces a via
fn fixture_via_crossing() -> (Design, ConstraintModel) {
    // Two footprints whose only route requires hopping over a wall of
    // same-row obstacle pads on layer 0 -> forces a layer change.
    let mut footprints = vec![fp("U1", 1_000, 5_000), fp("U2", 19_000, 5_000)];
    // A dense wall of single-pin obstacle parts across the board at x=10000,
    // each on its own net, blocking every grid row on F.Cu except via use.
    let mut parts = vec![part("U1", 1), part("U2", 1)];
    let mut nets = vec![Net { name: "SIGNAL".into(), pins: vec!["U1.1".into(), "U2.1".into()] }];
    let mut y = 0;
    let mut i = 0;
    while y <= 10_000 {
        let id = format!("W{i}");
        footprints.push(fp(&id, 10_000, y));
        parts.push(part(&id, 1));
        nets.push(Net { name: format!("WALL{i}"), pins: vec![format!("{id}.1")] });
        y += 254;
        i += 1;
    }
    let d = design(rect(20_000, 10_000), footprints);
    let m = model(parts, nets);
    (d, m)
}

// -------------------------------------------------------------- fixture 5: two-pin, both through-hole
fn fixture_2p_through_hole() -> (Design, ConstraintModel) {
    // Both pads are through-hole, so they exist on every layer; an
    // unobstructed path between them should never need a via — there's
    // no obstacle to dodge, and either endpoint can legally start/end on
    // whichever layer the path finds cheapest.
    let d = design(rect(20_000, 10_000), vec![fp("U1", 2_000, 5_000), fp("U2", 15_000, 5_000)]);
    let m = model(vec![part_th("U1", 1), part_th("U2", 1)], vec![Net {
        name: "NET1".into(),
        pins: vec!["U1.1".into(), "U2.1".into()],
    }]);
    (d, m)
}

// -------------------------------------------------------------- fixture 4: deliberately unroutable
fn fixture_unroutable() -> (Design, ConstraintModel) {
    // A wall of paired Top+Bottom obstacle pads at every grid row blocks
    // both F.Cu and B.Cu at x=10000 -> no path (direct or via) exists.
    let mut footprints = vec![fp("U1", 1_000, 5_000), fp("U2", 19_000, 5_000)];
    let mut parts = vec![part("U1", 1), part("U2", 1)];
    let mut nets = vec![Net { name: "SIGNAL".into(), pins: vec!["U1.1".into(), "U2.1".into()] }];
    let mut y = -2_000;
    let mut i = 0;
    while y <= 12_000 {
        let top_id = format!("WT{i}");
        let bot_id = format!("WB{i}");
        footprints.push(fp_side(&top_id, 10_000, y, Side::Top));
        footprints.push(fp_side(&bot_id, 10_000, y, Side::Bottom));
        parts.push(part(&top_id, 1));
        parts.push(part(&bot_id, 1));
        nets.push(Net { name: format!("WALLT{i}"), pins: vec![format!("{top_id}.1")] });
        nets.push(Net { name: format!("WALLB{i}"), pins: vec![format!("{bot_id}.1")] });
        y += 254;
        i += 1;
    }
    let d = design(rect(20_000, 10_000), footprints);
    let m = model(parts, nets);
    (d, m)
}

// -------------------------------------------------------------- fixture 6: pass-through bait
/// Three-pin net with a wall of obstacle pads that forces the path to the
/// far pin P2 along the bottom edge, straight across P3's copper. The
/// track may not use P3 as a stepping stone: it ends in P3 or goes round.
fn fixture_pass_through_bait() -> (Design, ConstraintModel) {
    let mut footprints = vec![fp("P1", 2_000, 5_000), fp("P2", 11_000, 5_000), fp("P3", 6_500, 9_900)];
    let mut parts = vec![part("P1", 1), part("P2", 1), part("P3", 1)];
    let mut nets = vec![Net { name: "N".into(), pins: vec!["P1.1".into(), "P2.1".into(), "P3.1".into()] }];
    let mut y = -500;
    let mut i = 0;
    while y <= 9_200 {
        // Both sides, so a via pair can't duck under the wall.
        for (side, tag) in [(Side::Top, "T"), (Side::Bottom, "B")] {
            let id = format!("W{tag}{i}");
            footprints.push(fp_side(&id, 6_500, y, side));
            parts.push(part(&id, 1));
            nets.push(Net { name: format!("WALL{tag}{i}"), pins: vec![format!("{id}.1")] });
        }
        // 300 µm between 500 µm pads: legal pad-pad clearance, no room
        // for a 200 µm track with 200 µm clearance on each side.
        y += 800;
        i += 1;
    }
    let d = design(rect(20_000, 11_500), footprints);
    let m = model(parts, nets);
    (d, m)
}

#[test]
fn pass_through_bait_never_uses_a_pad_as_a_stepping_stone() {
    let (d, m) = fixture_pass_through_bait();
    let rules = RouteRules::default();
    let out = route(&d, &m, &rules, 1).expect("should route");
    let r = out.routing.as_ref().unwrap();
    // The bait works: the N net does route below the wall, past P3.
    assert!(r.tracks.iter().any(|t| t.net == "N" && t.pts.iter().any(|p| p.y > 9_400)), "expected a path under the wall: {:?}", r.tracks);
    // The real gate (pad-mediated connectivity + workmanship).
    assert_gate_clean(&out, &m);
}

fn assert_within_outline(design: &Design) {
    let outline = &design.placement.as_ref().unwrap().outline;
    let min_x = outline.iter().map(|p| p.x).min().unwrap();
    let max_x = outline.iter().map(|p| p.x).max().unwrap();
    let min_y = outline.iter().map(|p| p.y).min().unwrap();
    let max_y = outline.iter().map(|p| p.y).max().unwrap();
    for t in &design.routing.as_ref().unwrap().tracks {
        for p in &t.pts {
            assert!(p.x >= min_x && p.x <= max_x && p.y >= min_y && p.y <= max_y, "track escapes outline: {p:?}");
        }
    }
}

/// The exact-geometry judge must agree with the router on every result.
fn assert_gate_clean(design: &Design, model: &ConstraintModel) {
    let fails: Vec<_> = eda_gates::check_routing(design, model)
        .into_iter()
        .filter(|c| c.status == eda_model::CheckStatus::Fail)
        .collect();
    assert!(fails.is_empty(), "routing gate failures: {fails:#?}");
}

/// The loop-3 quality gate (detour ratio, unnecessary-via count) must
/// agree with the router on every result, same contract as
/// `assert_gate_clean` for legality.
fn assert_routing_quality_clean(design: &Design, model: &ConstraintModel) {
    let fails: Vec<_> = eda_gates::check_routing_quality(design, model)
        .into_iter()
        .filter(|c| c.status == eda_model::CheckStatus::Fail)
        .collect();
    assert!(fails.is_empty(), "routing quality gate failures: {fails:#?}");
}

#[test]
fn fixture_2p2n_fully_routes() {
    let (d, m) = fixture_2p2n();
    let rules = RouteRules::default();
    let out = route(&d, &m, &rules, 1).expect("should route");
    assert_eq!(out.routing.as_ref().unwrap().tracks.iter().map(|t| &t.net).collect::<std::collections::HashSet<_>>().len(), 2);
    assert_within_outline(&out);
    assert_gate_clean(&out, &m);
    assert_routing_quality_clean(&out, &m);
}

#[test]
fn fixture_4p6n_fully_routes() {
    let (d, m) = fixture_4p6n();
    let rules = RouteRules::default();
    let out = route(&d, &m, &rules, 1).expect("should route");
    assert_within_outline(&out);
    assert_gate_clean(&out, &m);
    assert_routing_quality_clean(&out, &m);
}

#[test]
fn fixture_4p6n_routes_under_1s_release() {
    let (d, m) = fixture_4p6n();
    let rules = RouteRules::default();
    let start = std::time::Instant::now();
    let out = route(&d, &m, &rules, 1).expect("should route");
    let elapsed = start.elapsed();
    assert!(out.routing.is_some());
    // Only meaningful in release; debug builds are much slower, so only
    // hard-assert the bound when optimizations are on. 1 s is the
    // regression guard, not a target: routing takes some 40 ms of it, the
    // post-route optimizer the rest.
    if !cfg!(debug_assertions) {
        assert!(elapsed.as_millis() < 1000, "routing took {elapsed:?}, expected <1s in release");
    }
}

#[test]
fn fixture_via_crossing_uses_a_via() {
    let (d, m) = fixture_via_crossing();
    let rules = RouteRules::default();
    let out = route(&d, &m, &rules, 1).expect("should route");
    let vias = &out.routing.as_ref().unwrap().vias;
    assert!(!vias.iter().filter(|v| v.net == "SIGNAL").collect::<Vec<_>>().is_empty(), "expected SIGNAL to require a via");
    assert_within_outline(&out);
}

#[test]
fn fixture_2p_through_hole_routes_with_no_via() {
    // An all-through-hole two-pin net on a wide-open board has nothing to
    // dodge: the router may start on any layer the pad's copper touches,
    // and should need no via.
    let (d, m) = fixture_2p_through_hole();
    let rules = RouteRules::default();
    let out = route(&d, &m, &rules, 1).expect("should route");
    let vias = &out.routing.as_ref().unwrap().vias;
    assert!(vias.iter().all(|v| v.net != "NET1"), "unobstructed through-hole net should need no via, got {vias:?}");
    assert_within_outline(&out);
    assert_gate_clean(&out, &m);
    assert_routing_quality_clean(&out, &m);
}

#[test]
fn fixture_unroutable_reports_failure_not_panic() {
    let (d, m) = fixture_unroutable();
    let rules = RouteRules::default();
    let err = route(&d, &m, &rules, 1).expect_err("should fail, not panic or loop forever");
    assert!(err.iter().any(|c| c.check == "route_net_unrouted" && c.location.as_deref() == Some("SIGNAL")));
}

#[test]
fn same_seed_is_byte_deterministic() {
    let (d, m) = fixture_4p6n();
    let rules = RouteRules::default();
    let out1 = route(&d, &m, &rules, 42).unwrap();
    let out2 = route(&d, &m, &rules, 42).unwrap();
    assert_eq!(out1.canonical_bytes().unwrap(), out2.canonical_bytes().unwrap());
}

#[test]
fn different_seeds_may_differ_but_both_valid() {
    let (d, m) = fixture_4p6n();
    let rules = RouteRules::default();
    let out1 = route(&d, &m, &rules, 1).unwrap();
    let out2 = route(&d, &m, &rules, 999_999).unwrap();
    // Both must be valid regardless of whether the bytes match.
    assert_gate_clean(&out1, &m);
    assert_gate_clean(&out2, &m);
}

#[test]
fn missing_placement_fails_cleanly() {
    let (mut d, m) = fixture_2p2n();
    d.placement = None;
    let rules = RouteRules::default();
    let err = route(&d, &m, &rules, 1).expect_err("no placement should fail");
    assert!(err.iter().any(|c| c.check == "route_precondition"));
}

// ---- copper pours -------------------------------------------------------

/// Four parts around a board with a real ground net: three signals and a
/// GND that touches every part, which is the shape a plane exists for.
fn fixture_pour() -> (Design, ConstraintModel) {
    let d = design(
        rect(30_000, 30_000),
        vec![
            fp("U1", 3_000, 3_000),
            fp("U2", 25_000, 3_000),
            fp("U3", 3_000, 25_000),
            fp("U4", 25_000, 25_000),
        ],
    );
    let parts = vec![part("U1", 3), part("U2", 3), part("U3", 3), part("U4", 3)];
    let nets = vec![
        Net { name: "N1".into(), pins: vec!["U1.1".into(), "U2.1".into()] },
        Net { name: "N2".into(), pins: vec!["U3.1".into(), "U4.1".into()] },
        Net { name: "N3".into(), pins: vec!["U1.2".into(), "U3.2".into()] },
        Net {
            name: "GND".into(),
            pins: vec!["U1.3".into(), "U2.3".into(), "U3.3".into(), "U4.3".into()],
        },
    ];
    let m = model(parts, nets);
    (d, m)
}

fn poured(net: &str) -> RouteRules {
    let mut r = RouteRules::default();
    r.pours = vec![eda_model::Pour { net: net.to_string(), layer: "B.Cu".to_string() }];
    r
}

#[test]
fn a_poured_net_reaches_every_pad() {
    // `route` succeeding means the pour check passed: every GND pad reaches
    // the plane's body, by the plane or by GND's own copper, once every
    // signal has cut into it.
    let (d, m) = fixture_pour();
    let out = route(&d, &m, &poured("GND"), 1).expect("should route");
    assert!(eda::check_pours(&out, &m, &poured("GND")).is_empty());
}

#[test]
fn a_pour_emits_a_zone_on_its_layer() {
    let (d, m) = fixture_pour();
    let out = route(&d, &m, &poured("GND"), 1).expect("should route");
    let z = &out.routing.as_ref().unwrap().zones;
    assert_eq!(z.len(), 1, "expected exactly one zone");
    assert_eq!(z[0].net, "GND");
    assert_eq!(z[0].layer, "B.Cu");
    assert!(z[0].outline.len() >= 3, "zone outline is not a polygon");
}

#[test]
fn a_stitching_via_never_sits_on_pad_copper() {
    let (d, m) = fixture_pour();
    let out = route(&d, &m, &poured("GND"), 1).expect("should route");
    // Via-in-pad is a fab defect on any net, the via's own included.
    assert_gate_clean(&out, &m);
}

#[test]
fn a_poured_board_still_passes_every_routing_gate() {
    let (d, m) = fixture_pour();
    let out = route(&d, &m, &poured("GND"), 1).expect("should route");
    assert_within_outline(&out);
    assert_gate_clean(&out, &m);
    assert_routing_quality_clean(&out, &m);
}

#[test]
fn a_pour_on_a_net_the_board_does_not_have_is_a_hard_fail() {
    let (d, m) = fixture_pour();
    let (_, fails) = eda::route_partial(&d, &m, &poured("NOT_A_NET"));
    assert!(
        fails.iter().any(|f| f.check == "route_precondition" && f.hint.as_deref().is_some_and(|h| h.contains("NOT_A_NET"))),
        "a pour on a net with no pads must fail, got {:?}",
        fails.iter().map(|f| &f.check).collect::<Vec<_>>()
    );
}

#[test]
fn a_pour_on_a_layer_outside_the_stackup_is_a_hard_fail() {
    let (d, m) = fixture_pour();
    let mut rules = RouteRules::default();
    rules.pours = vec![eda_model::Pour { net: "GND".into(), layer: "In7.Cu".into() }];
    let (_, fails) = eda::route_partial(&d, &m, &rules);
    assert!(
        fails.iter().any(|f| f.check == "route_precondition" && f.hint.as_deref().is_some_and(|h| h.contains("In7.Cu"))),
        "a pour on a layer the board does not have must fail, got {:?}",
        fails.iter().map(|f| &f.check).collect::<Vec<_>>()
    );
}


/// A board whose outline does not start at the coordinate origin, at an
/// offset that is no multiple of anything.
fn fixture_offset_outline() -> (Design, ConstraintModel) {
    let (ox, oy) = (1_325, 700);
    let outline = rect(20_000, 10_000).into_iter().map(|p| Point { x: p.x + ox, y: p.y + oy }).collect();
    let d = design(outline, vec![fp("U1", ox + 2_000, oy + 5_000), fp("U2", ox + 15_000, oy + 5_000)]);
    let m = model(vec![part("U1", 2), part("U2", 2)], vec![
            Net { name: "NET1".into(), pins: vec!["U1.1".into(), "U2.1".into()] },
            Net { name: "NET2".into(), pins: vec!["U1.2".into(), "U2.2".into()] },
        ]);
    (d, m)
}

#[test]
fn a_board_not_at_the_origin_routes_gate_clean() {
    let (d, m) = fixture_offset_outline();
    let rules = RouteRules::default();
    let out = route(&d, &m, &rules, 1).expect("should route");
    assert_gate_clean(&out, &m);
}
