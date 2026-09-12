//! Unit checks for the placement quality gates in `eda_gates::pcb`
//! (`check_placement_locality`): each one must fail on a hand-built bad
//! placement and pass once the offending part is moved. Numbers here are
//! the gate thresholds; if a threshold moves these fixtures should too.

use eda_model::ir::{Design, FootprintInstance, PlacementSection, Point, Provenance, Side};
use eda_model::{CheckStatus, ConstraintModel};

const INTENT: &str = r#"
parts:
  - reference: U1
    value: MCU
    package: SOT-223
    pins:
      - { number: "1", name: GND, kind: ground }
      - { number: "2", name: OUT, kind: signal }
      - { number: "3", name: VIN, kind: power }
  - reference: C1
    value: 100n
    package: "0603"
    pins: [{ number: "1", kind: passive }, { number: "2", kind: passive }]
  - reference: J1
    value: HDR
    package: PINHEADER-4
    # A bare `J` refdes is not an edge part (most real J* footprints sit in
    # the interior); the gates key off this flag, so a fixture that is meant
    # to exercise them has to set it.
    edge: true
    pins:
      - { number: "1", name: VIN, kind: power }
      - { number: "2", name: GND, kind: ground }
      - { number: "3", name: A, kind: signal }
      - { number: "4", name: B, kind: signal }
  - reference: R1
    value: 1k
    package: "0603"
    pins: [{ number: "1", kind: passive }, { number: "2", kind: passive }]
  - reference: R2
    value: 1k
    package: "0603"
    pins: [{ number: "1", kind: passive }, { number: "2", kind: passive }]
  - reference: R3
    value: 1k
    package: "0603"
    pins: [{ number: "1", kind: passive }, { number: "2", kind: passive }]
  - reference: R4
    value: 1k
    package: "0603"
    pins: [{ number: "1", kind: passive }, { number: "2", kind: passive }]
nets:
  - { name: VIN, pins: [U1.3, C1.1, J1.1] }
  - { name: GND, pins: [U1.1, C1.2, J1.2] }
  - { name: A, pins: [J1.3, R1.1] }
  - { name: B, pins: [J1.4, R2.1] }
  - { name: X, pins: [R1.2, R2.2] }
  # R3/R4 reach no connector, so net Y is the one pair the compactness
  # gate will actually judge.
  - { name: Y, pins: [R3.1, R4.1] }
"#;

fn square(side: i64) -> Vec<Point> {
    vec![Point { x: 0, y: 0 }, Point { x: side, y: 0 }, Point { x: side, y: side }, Point { x: 0, y: side }]
}

fn fp(id: &str, x: i64, y: i64, rot_deg: u32) -> FootprintInstance {
    FootprintInstance { id: id.into(), at: Point { x, y }, rot: rot_deg * 1000, side: Side::Top, label: Default::default() }
}

fn design(outline: Vec<Point>, fps: Vec<FootprintInstance>) -> Design {
    Design {
        schema: 1,
        provenance: Provenance { engine_version: "test".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        placement: Some(PlacementSection { outline, footprints: fps }),
        routing: None,
    }
}

fn fails(d: &Design, m: &ConstraintModel, check: &str) -> Vec<String> {
    eda_gates::check_placement_locality(d, m)
        .into_iter()
        .filter(|c| c.status == CheckStatus::Fail && c.check == check)
        .map(|c| format!("{} {}", c.location.unwrap_or_default(), c.hint.unwrap_or_default()))
        .collect()
}

fn model() -> ConstraintModel {
    serde_yaml::from_str(INTENT).unwrap()
}

const SIDE: i64 = 24000;

/// A tidy reference layout on a 24 mm board: J1 along the top edge with
/// R1/R2 just below their header pins, U1 lower-centre with C1 beside it.
fn good() -> Vec<FootprintInstance> {
    vec![
        fp("J1", 12000, 1400, 0),
        fp("U1", 12000, 18000, 0),
        fp("C1", 18500, 18000, 0),
        fp("R1", 10000, 5000, 90),
        fp("R2", 14000, 5000, 90),
        fp("R3", 4000, 11000, 0),
        fp("R4", 4000, 13000, 0),
    ]
}

#[test]
fn reference_layout_is_clean() {
    let m = model();
    let d = design(square(SIDE), good());
    let all: Vec<_> = eda_gates::check_placement_locality(&d, &m).into_iter().filter(|c| c.status == CheckStatus::Fail).collect();
    assert!(all.is_empty(), "{all:#?}");
}

#[test]
fn edge_connector_in_the_interior_fails() {
    let m = model();
    let mut fps = good();
    fps[0] = fp("J1", 12000, 8000, 0); // 6.6 mm from the top edge
    let d = design(square(SIDE), fps);
    let f = fails(&d, &m, "placement_edge_connector");
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].starts_with("J1"), "{f:?}");
}

#[test]
fn edge_connector_touching_with_its_short_end_still_fails() {
    let m = model();
    let mut fps = good();
    // Horizontal header whose left end touches the left edge: pins point
    // into the board, not off it.
    fps[0] = fp("J1", 5400, 9000, 0);
    let d = design(square(SIDE), fps.clone());
    assert_eq!(fails(&d, &m, "placement_edge_connector").len(), 1);
    // Rotated so its long side runs along that edge: fine.
    fps[0] = fp("J1", 1400, 9000, 90);
    let d = design(square(SIDE), fps);
    assert!(fails(&d, &m, "placement_edge_connector").is_empty());
}

#[test]
fn off_centre_cluster_fails_board_use() {
    let m = model();
    // The same layout on a 60 mm board: everything in the top-left.
    let d = design(square(60000), good());
    let f = fails(&d, &m, "placement_board_use");
    assert!(!f.is_empty(), "{f:?}");
    assert!(f.iter().any(|s| s.contains("off-centre")) && f.iter().any(|s| s.contains("span only")), "{f:?}");
}

#[test]
fn far_decoupling_cap_fails() {
    let m = model();
    let mut fps = good();
    fps[2] = fp("C1", 22000, 4000, 0); // C1 shares VIN and GND with U1, now ~6 mm away
    let d = design(square(SIDE), fps);
    let f = fails(&d, &m, "placement_decoupling");
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].starts_with("C1/U1"), "{f:?}");
}

#[test]
fn crossing_stubs_fail() {
    let m = model();
    let mut fps = good();
    // Swap R1 and R2 under the header: A and B stubs now cross.
    fps[3] = fp("R1", 14000, 5000, 90);
    fps[4] = fp("R2", 10000, 5000, 90);
    let d = design(square(SIDE), fps);
    let f = fails(&d, &m, "placement_stub_crossings");
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].contains("A×B") || f[0].contains("B×A"), "{f:?}");
}

#[test]
fn spread_small_net_fails_compactness() {
    let m = model();
    let mut fps = good();
    // R3 dragged across the board: net Y (R3-R4) is stretched. Nets A, B
    // and X are not judged — every one of them has a member sitting on a
    // net that reaches the edge connector J1.
    fps[5] = fp("R3", 20000, 11000, 0);
    let d = design(square(SIDE), fps);
    let f = fails(&d, &m, "placement_net_compactness");
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].starts_with("Y "), "{f:?}");
}

