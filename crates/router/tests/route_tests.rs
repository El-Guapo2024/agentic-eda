use eda_model::ir::{Design, FootprintInstance, PlacementSection, Point, Provenance, Side};
use eda_model::footprint::{Footprint, Pad, PadKind, PadShape};
use eda_model::{ConstraintModel, Net, Part, Pin, PinKind};
use eda_router::{route, RouteRules};
use std::collections::HashMap;

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
    ConstraintModel { parts, nets, footprints: vec![lin(1), lin(2), lin(3)], ..Default::default() }
}

fn fp(id: &str, x: i64, y: i64) -> FootprintInstance {
    FootprintInstance { id: id.into(), at: Point { x, y }, rot: 0, side: Side::Top }
}

fn fp_side(id: &str, x: i64, y: i64, side: Side) -> FootprintInstance {
    FootprintInstance { id: id.into(), at: Point { x, y }, rot: 0, side }
}

fn design(outline: Vec<Point>, footprints: Vec<FootprintInstance>) -> Design {
    Design {
        schema: 1,
        provenance: provenance(),
        schematic: None,
        placement: Some(PlacementSection { outline, footprints }),
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

fn assert_on_grid(design: &Design, rules: &RouteRules) {
    let r = &design.routing.as_ref().unwrap();
    for t in &r.tracks {
        for p in &t.pts {
            assert_eq!(p.x % rules.grid, 0, "track point not on grid: {p:?}");
            assert_eq!(p.y % rules.grid, 0, "track point not on grid: {p:?}");
        }
    }
    for v in &r.vias {
        assert_eq!(v.at.x % rules.grid, 0);
        assert_eq!(v.at.y % rules.grid, 0);
    }
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

fn assert_clearance(design: &Design, rules: &RouteRules) {
    // Rebuild occupied cells per net per layer, verify no two different
    // nets occupy cells within the clearance ring of each other.
    let r_cells = ((rules.clearance + rules.grid - 1) / rules.grid).max(0);
    let mut occ: HashMap<(i64, i64, String), String> = HashMap::new();
    for t in &design.routing.as_ref().unwrap().tracks {
        for p in &t.pts {
            occ.insert((p.x.div_euclid(rules.grid), p.y.div_euclid(rules.grid), t.layer.clone()), t.net.clone());
        }
    }
    for ((cx, cy, layer), net) in &occ {
        for dx in -r_cells..=r_cells {
            for dy in -r_cells..=r_cells {
                if dx == 0 && dy == 0 {
                    continue;
                }
                if let Some(other_net) = occ.get(&(cx + dx, cy + dy, layer.clone())) {
                    assert_eq!(other_net, net, "clearance violation between {net} and {other_net} at ({cx},{cy},{layer})");
                }
            }
        }
    }
}

fn assert_connectivity(design: &Design, model: &ConstraintModel) {
    let routing = design.routing.as_ref().unwrap();
    for net in &model.nets {
        if net.pins.len() < 2 {
            continue;
        }
        let net_tracks: Vec<_> = routing.tracks.iter().filter(|t| t.net == net.name).collect();
        if net_tracks.is_empty() {
            continue; // net may have legitimately failed to route
        }
        // Union-find over track endpoints (snapped grid points) + vias.
        let mut points: Vec<Point> = Vec::new();
        let mut edges: Vec<(usize, usize)> = Vec::new();
        let idx = |p: Point, points: &mut Vec<Point>| -> usize {
            if let Some(i) = points.iter().position(|q| *q == p) {
                i
            } else {
                points.push(p);
                points.len() - 1
            }
        };
        for t in &net_tracks {
            for w in t.pts.windows(2) {
                let a = idx(w[0], &mut points);
                let b = idx(w[1], &mut points);
                edges.push((a, b));
            }
        }
        for v in routing.vias.iter().filter(|v| v.net == net.name) {
            let a = idx(v.at, &mut points);
            edges.push((a, a)); // via is a same-point layer bridge; no-op on point graph
        }
        let mut parent: Vec<usize> = (0..points.len()).collect();
        fn find(parent: &mut Vec<usize>, x: usize) -> usize {
            if parent[x] != x {
                parent[x] = find(parent, parent[x]);
            }
            parent[x]
        }
        for (a, b) in edges {
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            parent[ra] = rb;
        }
        // All pad endpoints referenced in track.pins must be in one component.
        let mut pin_points: Vec<usize> = Vec::new();
        for t in &net_tracks {
            if !t.pins.is_empty() {
                if let Some(&first) = t.pts.first() {
                    pin_points.push(idx(first, &mut points));
                }
                if let Some(&last) = t.pts.last() {
                    pin_points.push(idx(last, &mut points));
                }
            }
        }
        assert!(!pin_points.is_empty(), "net {} has tracks but none reference pins", net.name);
        let root = find(&mut parent, pin_points[0]);
        for p in &pin_points {
            assert_eq!(find(&mut parent, *p), root, "net {} is not fully connected", net.name);
        }
    }
}

#[test]
fn fixture_2p2n_fully_routes() {
    let (d, m) = fixture_2p2n();
    let rules = RouteRules::default();
    let out = route(&d, &m, &rules, 1).expect("should route");
    assert_eq!(out.routing.as_ref().unwrap().tracks.iter().map(|t| &t.net).collect::<std::collections::HashSet<_>>().len(), 2);
    assert_on_grid(&out, &rules);
    assert_within_outline(&out);
    assert_clearance(&out, &rules);
    assert_connectivity(&out, &m);
    assert_gate_clean(&out, &m);
}

#[test]
fn fixture_4p6n_fully_routes() {
    let (d, m) = fixture_4p6n();
    let rules = RouteRules::default();
    let out = route(&d, &m, &rules, 1).expect("should route");
    assert_on_grid(&out, &rules);
    assert_within_outline(&out);
    assert_clearance(&out, &rules);
    assert_connectivity(&out, &m);
    assert_gate_clean(&out, &m);
}

#[test]
fn fixture_4p6n_routes_under_100ms_release() {
    let (d, m) = fixture_4p6n();
    let rules = RouteRules::default();
    let start = std::time::Instant::now();
    let out = route(&d, &m, &rules, 1).expect("should route");
    let elapsed = start.elapsed();
    assert!(out.routing.is_some());
    // Only meaningful in release; debug builds are much slower, so only
    // hard-assert the bound when optimizations are on.
    if !cfg!(debug_assertions) {
        assert!(elapsed.as_millis() < 100, "routing took {elapsed:?}, expected <100ms in release");
    }
}

#[test]
fn fixture_via_crossing_uses_a_via() {
    let (d, m) = fixture_via_crossing();
    let rules = RouteRules::default();
    let out = route(&d, &m, &rules, 1).expect("should route");
    let vias = &out.routing.as_ref().unwrap().vias;
    assert!(!vias.iter().filter(|v| v.net == "SIGNAL").collect::<Vec<_>>().is_empty(), "expected SIGNAL to require a via");
    assert_on_grid(&out, &rules);
    assert_within_outline(&out);
    assert_clearance(&out, &rules);
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
    assert_connectivity(&out1, &m);
    assert_connectivity(&out2, &m);
}

#[test]
fn missing_placement_fails_cleanly() {
    let (mut d, m) = fixture_2p2n();
    d.placement = None;
    let rules = RouteRules::default();
    let err = route(&d, &m, &rules, 1).expect_err("no placement should fail");
    assert!(err.iter().any(|c| c.check == "route_precondition"));
}
