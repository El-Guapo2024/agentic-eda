use eda_model::ir::{Design, Point, Provenance};
use eda_model::{CheckStatus, ConstraintModel, Net, Part, Pin, PinKind, PlacementRule};
use eda_place::{hpwl, place, PlaceOptions};

fn part(reference: &str, package: &str, npins: usize) -> Part {
    Part {
        reference: reference.into(),
        mpn: None,
        lcsc: None,
        value: None,
        package: Some(package.into()),
        footprint: None,
        pins: (1..=npins).map(|i| Pin { number: i.to_string(), name: None, kind: PinKind::Signal }).collect(),
        body_um: None, symbol: None, datasheet: None,
        edge: None,
    }
}

fn design() -> Design {
    Design {
        footprint_library: None, sheet_contents: None,
        schema: 1,
        provenance: Provenance { engine_version: "test".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None, nets: None,
        placement: None,
        routing: None,
        drawings: None,
    }
}

/// An LDO-ish board: regulator + in/out caps + 8 pull-ups + a header.
fn ldo_model() -> ConstraintModel {
    // SOT-223 is a 3-net part in the corrected builtin footprint: the tab
    // (VOUT here) and pin 2 are the same physical pad (KiCad numbers the
    // tab as pin 2), so there is no separate "pin 4".
    let mut parts = vec![part("U1", "SOT-223", 3), part("C1", "0805", 2), part("C2", "0805", 2), part("J1", "PINHEADER-4", 4), part("J2", "PINHEADER-8", 8)];
    let mut nets = vec![
        Net { name: "VIN".into(), pins: vec!["U1.3".into(), "C1.1".into(), "J1.1".into()] },
        Net { name: "GND".into(), pins: vec!["U1.1".into(), "C1.2".into(), "C2.2".into(), "J1.2".into()] },
        Net { name: "VOUT".into(), pins: vec!["U1.2".into(), "C2.1".into(), "J1.3".into()] },
    ];
    for i in 1..=8 {
        parts.push(part(&format!("R{i}"), "0603", 2));
        nets.push(Net { name: format!("SIG{i}"), pins: vec![format!("R{i}.1"), format!("J2.{i}")] });
        nets[2].pins.push(format!("R{i}.2"));
    }
    ConstraintModel {
        parts,
        nets,
        placement_rules: vec![
            PlacementRule::Proximity { a: "U1".into(), b: "C1".into(), max_mm: 5.0, reason: None },
            PlacementRule::Proximity { a: "U1".into(), b: "C2".into(), max_mm: 5.0, reason: None },
        ],
        ..Default::default()
    }
}

fn gate_fails(d: &Design, m: &ConstraintModel) -> Vec<eda_model::CheckResult> {
    eda_gates::check_placement(d, m).into_iter().filter(|c| c.status == CheckStatus::Fail).collect()
}

#[test]
fn ldo_board_places_gate_clean() {
    let m = ldo_model();
    let out = place(&design(), &m, &PlaceOptions::default()).expect("places");
    let fails = gate_fails(&out, &m);
    assert!(fails.is_empty(), "{fails:#?}");
    assert_eq!(out.placement.as_ref().unwrap().footprints.len(), m.parts.len());
}

#[test]
fn respects_explicit_outline() {
    let m = ldo_model();
    let outline = vec![Point { x: 0, y: 0 }, Point { x: 32_000, y: 0 }, Point { x: 32_000, y: 20_000 }, Point { x: 0, y: 20_000 }];
    let opts = PlaceOptions { outline: Some(outline.clone()), ..Default::default() };
    let out = place(&design(), &m, &opts).expect("places");
    assert_eq!(out.placement.as_ref().unwrap().outline, outline);
    assert!(gate_fails(&out, &m).is_empty());
}

#[test]
fn same_seed_is_byte_deterministic() {
    let m = ldo_model();
    let a = place(&design(), &m, &PlaceOptions::default()).unwrap();
    let b = place(&design(), &m, &PlaceOptions::default()).unwrap();
    assert_eq!(a.canonical_bytes().unwrap(), b.canonical_bytes().unwrap());
}

#[test]
fn annealing_beats_initial_layout() {
    let m = ldo_model();
    let lazy = place(&design(), &m, &PlaceOptions { moves_per_part: 0, ..Default::default() }).unwrap();
    let tuned = place(&design(), &m, &PlaceOptions::default()).unwrap();
    let (a, b) = (hpwl(&lazy, &m).unwrap(), hpwl(&tuned, &m).unwrap());
    assert!(b < a, "annealed HPWL {b} should beat greedy {a}");
}

#[test]
fn missing_footprint_fails_loudly() {
    let mut m = ldo_model();
    m.parts.push(part("X1", "NO-SUCH-PACKAGE", 3));
    let err = place(&design(), &m, &PlaceOptions::default()).unwrap_err();
    assert!(err.iter().any(|c| c.check == "place_precondition" && c.location.as_deref() == Some("X1")));
}

#[test]
fn too_small_board_fails_not_panics() {
    let m = ldo_model();
    let outline = vec![Point { x: 0, y: 0 }, Point { x: 4_000, y: 0 }, Point { x: 4_000, y: 4_000 }, Point { x: 0, y: 4_000 }];
    let err = place(&design(), &m, &PlaceOptions { outline: Some(outline), moves_per_part: 200, ..Default::default() }).unwrap_err();
    assert!(err.iter().any(|c| c.check == "place_legalize"));
}

#[test]
fn placement_then_routing_end_to_end() {
    let m = ldo_model();
    let placed = place(&design(), &m, &PlaceOptions::default()).expect("places");
    let routed = route(&placed, &m, &m.board, 1).expect("routes");
    let fails: Vec<_> = eda_gates::check_routing(&routed, &m).into_iter().filter(|c| c.status == CheckStatus::Fail).collect();
    assert!(fails.is_empty(), "{fails:#?}");
}

/// The design routed by the FreeRouting port, or what it left unrouted.
fn route(design: &eda_model::ir::Design, model: &eda_model::ConstraintModel, rules: &eda_model::BoardRules, _seed: u64) -> Result<eda_model::ir::Design, Vec<String>> {
    let routed = eda_freeroute::design::route_design(design, model, rules, 20, 10).map_err(|e| vec![e])?;
    if !routed.unrouted.is_empty() {
        return Err(routed.unrouted);
    }
    let mut out = design.clone();
    out.routing = Some(routed.routing);
    Ok(out)
}
