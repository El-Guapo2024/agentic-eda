//! `fill_zone` tests: knockouts (same/different net, pad connection modes),
//! thermal spokes, and geometric island removal, per the task's stage-3
//! requirements.

use eda_clipper2::Point64;
use eda_model::ir::{IslandRemovalMode, PadConnection, Point, Zone};
use eda_zone_filler::shape::Shape;
use eda_zone_filler::spokes::poly_set_contains as poly_set_contains_pt;
use eda_zone_filler::{fill_zone, FillInput, FillKeepout, FillPad, FillZoneRef};

const MAX_ERROR: i64 = 5;

fn rect_outline(x0: i64, y0: i64, x1: i64, y1: i64) -> Vec<Point> {
    vec![Point { x: x0, y: y0 }, Point { x: x1, y: y0 }, Point { x: x1, y: y1 }, Point { x: x0, y: y1 }]
}

fn test_zone(net: &str, outline: Vec<Point>) -> Zone {
    Zone {
        net: net.into(),
        layer: "F.Cu".into(),
        outline,
        min_thickness: 200,
        thermal_gap: 200,
        thermal_spoke_width: 300,
        clearance: 20, // small, relative to these tests' 100-1000um fixtures
        ..Default::default()
    }
}

fn no_clearance(_a: Option<&str>, _b: Option<&str>) -> i64 {
    20
}

#[test]
fn empty_zone_fills_almost_its_full_area() {
    let zone = test_zone("GND", rect_outline(0, 0, 1000, 1000));
    let input = FillInput::default();
    let fill = fill_zone(&zone, "F.Cu", &input, no_clearance, MAX_ERROR);

    let area = fill.area();
    assert!((area - 1_000_000.0).abs() / 1_000_000.0 < 0.02, "area {area} should be close to 1_000_000");
}

#[test]
fn full_connection_pad_is_not_knocked_out() {
    let zone = Zone { pad_connection: PadConnection::Full, ..test_zone("GND", rect_outline(0, 0, 1000, 1000)) };
    let input = FillInput {
        pads: vec![FillPad { net: Some("GND".into()), layers: vec!["F.Cu".into()], copper: Shape::Rect { x0: 400, y0: 400, x1: 600, y1: 600 }, hole: None, ..Default::default() }],
        ..Default::default()
    };
    let fill = fill_zone(&zone, "F.Cu", &input, no_clearance, MAX_ERROR);

    let area = fill.area();
    assert!((area - 1_000_000.0).abs() / 1_000_000.0 < 0.02, "area {area} should still be close to 1_000_000 (FULL connection => no knockout)");
    assert!(poly_set_contains_pt(&fill, Point64::new(500, 500)), "pad center should be flooded with copper");
}

/// A thermal-relief fixture: a 2 x 2 mm zone with a 0.4 mm square GND pad in the middle, 0.2 mm gap, 0.3 mm spokes.
fn relief_zone() -> Zone {
    Zone { pad_connection: PadConnection::Thermal, ..test_zone("GND", rect_outline(0, 0, 2000, 2000)) }
}

fn gnd_pad() -> FillPad {
    FillPad { net: Some("GND".into()), layers: vec!["F.Cu".into()], copper: Shape::Rect { x0: 800, y0: 800, x1: 1200, y1: 1200 }, hole: None, ..Default::default() }
}

fn fill_with(zone: &Zone, pad: FillPad) -> eda_shape_poly_set::ShapePolySet {
    fill_zone(zone, "F.Cu", &FillInput { pads: vec![pad], ..Default::default() }, no_clearance, MAX_ERROR)
}

fn copper(fill: &eda_shape_poly_set::ShapePolySet, x: i64, y: i64) -> bool {
    poly_set_contains_pt(fill, Point64::new(x, y))
}

#[test]
fn thermal_pad_is_knocked_out_but_spoked() {
    let fill = fill_with(&relief_zone(), gnd_pad());

    let area = fill.area();
    assert!(area > 0.0, "fill should not be empty");
    assert!(area < 3_950_000.0, "thermal relief should remove a meaningful chunk of area, got {area}");

    // the pad's corner and the diagonal of the gap are knocked out...
    assert!(!copper(&fill, 820, 820), "pad corner should be knocked out, not flooded");
    assert!(!copper(&fill, 700, 700), "the gap's diagonal is open");
    // ...but each spoke bridges the gap on its axis.
    for (x, y) in [(1000, 650), (1000, 1350), (650, 1000), (1350, 1000)] {
        assert!(copper(&fill, x, y), "a spoke should cross the gap at {x},{y}");
    }
    assert!(copper(&fill, 100, 100), "the zone body is copper");
}

#[test]
fn a_pad_turned_forty_five_degrees_turns_its_spokes_with_it() {
    use eda_zone_filler::PadGeometry;
    // The same relief, but the pad is turned 45 degrees: its spokes run along the diagonals, so the gap is bridged
    // on the diagonal and open on the axes.
    let pad = FillPad {
        geometry: Some(PadGeometry { center: Point64::new(1000, 1000), size: (400, 400), circular: false, default_spoke_angle_mdeg: 90_000, orientation_mdeg: 45_000 }),
        ..gnd_pad()
    };
    let fill = fill_with(&relief_zone(), pad);
    assert!(copper(&fill, 1000 + 450, 1000 + 450) || copper(&fill, 1000 - 450, 1000 - 450), "a spoke on the diagonal");
    assert!(!copper(&fill, 1000, 650), "the axes are open now");
}

#[test]
fn a_pads_own_spoke_angle_overrides_the_default() {
    let pad = FillPad { spoke_angle_mdeg: Some(45_000), ..gnd_pad() };
    let fill = fill_with(&relief_zone(), pad);
    assert!(!copper(&fill, 1000, 650), "no spoke on the axis at a 45 degree spoke angle");
    assert!(copper(&fill, 1000 + 450, 1000 + 450) || copper(&fill, 1000 - 450, 1000 - 450) || copper(&fill, 1000 + 450, 1000 - 450) || copper(&fill, 1000 - 450, 1000 + 450), "a diagonal spoke");
}

#[test]
fn a_pad_set_to_solid_is_not_relieved_in_a_thermal_zone() {
    let fill = fill_with(&relief_zone(), FillPad { zone_connection: Some(PadConnection::Full), ..gnd_pad() });
    assert!(copper(&fill, 820, 820), "a solid connection floods the pad");
    assert!(copper(&fill, 700, 700), "and leaves no gap");
}

#[test]
fn a_pad_set_to_none_gets_a_clearance_hole_and_no_spokes_in_a_solid_zone() {
    let zone = Zone { pad_connection: PadConnection::Full, ..relief_zone() };
    let fill = fill_with(&zone, FillPad { zone_connection: Some(PadConnection::None), ..gnd_pad() });
    assert!(!copper(&fill, 1000, 1000), "the pad is cleared");
    // only the zone's 20 um clearance is cleared: the diagonal of a thermal gap (open in a relief) is copper here
    assert!(!copper(&fill, 790, 1000), "the clearance hole hugs the pad");
    assert!(copper(&fill, 700, 700), "there is no relief gap round a pad set to none");
    assert!(copper(&fill, 100, 100));
}

#[test]
fn a_footprints_connection_applies_unless_the_pad_sets_its_own() {
    let zone = relief_zone();
    // the footprint says solid, the pad inherits: solid
    let solid = fill_with(&zone, FillPad { footprint_zone_connection: Some(PadConnection::Full), ..gnd_pad() });
    assert!(copper(&solid, 820, 820), "the footprint's solid connection reaches the pad");
    // the pad says thermal, the footprint says solid: the pad wins
    let relieved = fill_with(&zone, FillPad { footprint_zone_connection: Some(PadConnection::Full), zone_connection: Some(PadConnection::Thermal), ..gnd_pad() });
    assert!(!copper(&relieved, 820, 820), "the pad's own thermal relief beats its footprint's");
}

#[test]
fn thru_hole_only_relieves_a_plated_through_hole_pad_and_floods_an_smd_one() {
    let zone = Zone { pad_connection: PadConnection::ThtThermal, ..relief_zone() };
    let tht = fill_with(&zone, FillPad { plated_through_hole: true, ..gnd_pad() });
    assert!(!copper(&tht, 820, 820), "a plated through-hole pad gets a relief");
    let smd = fill_with(&zone, FillPad { plated_through_hole: false, ..gnd_pad() });
    assert!(copper(&smd, 820, 820), "an SMD pad is connected solidly");
}

#[test]
fn a_pads_own_gap_and_spoke_width_override_the_zones() {
    let wide_gap = fill_with(&relief_zone(), FillPad { thermal_gap: Some(400), ..gnd_pad() });
    let normal = fill_with(&relief_zone(), gnd_pad());
    assert!(wide_gap.area() < normal.area() - 100_000.0, "a bigger gap takes more copper: {} vs {}", wide_gap.area(), normal.area());
    assert!(!copper(&wide_gap, 550, 550), "400 um gap: the knockout reaches the diagonal at 550,550");
    assert!(copper(&normal, 550, 550), "a 200 um gap does not");
    assert!(copper(&wide_gap, 1000, 450), "and the spoke still bridges the gap on its axis");
    // A 0.3 mm spoke is 300 wide at x = 1000 +- 150; a 0.2 mm override is 200 wide: 1000 + 120 is copper only on the wide one.
    assert!(copper(&normal, 1130, 650), "a 300 um spoke covers x = 1130");
    let narrow = fill_with(&relief_zone(), FillPad { spoke_width: Some(200), ..gnd_pad() });
    assert!(!copper(&narrow, 1130, 650), "a 200 um spoke does not");
    assert!(copper(&narrow, 1000, 650));
}

#[test]
fn different_net_pad_gets_a_clearance_hole() {
    let zone = test_zone("GND", rect_outline(0, 0, 1000, 1000));
    let input = FillInput {
        pads: vec![FillPad { net: Some("VCC".into()), layers: vec!["F.Cu".into()], copper: Shape::Rect { x0: 400, y0: 400, x1: 600, y1: 600 }, hole: None, ..Default::default() }],
        ..Default::default()
    };
    let fill = fill_zone(&zone, "F.Cu", &input, no_clearance, MAX_ERROR);

    assert!(!poly_set_contains_pt(&fill, Point64::new(500, 500)), "different-net pad should leave a clearance hole");
    // well outside the pad + its 200um clearance, copper should remain.
    assert!(poly_set_contains_pt(&fill, Point64::new(50, 50)), "far corner should still be filled");
    let area = fill.area();
    assert!(area < 950_000.0 && area > 500_000.0, "area {area} should reflect a moderate knockout, not the whole zone");
}

#[test]
fn island_removal_always_drops_the_disconnected_half() {
    // A wide zone cut clean in two by a different-net blocking pad spanning
    // its full height (plus clearance); the right half touches a same-net
    // FULL pad (kept), the left half touches nothing of this net (dropped
    // under ALWAYS, kept under NEVER).
    let zone = Zone { pad_connection: PadConnection::Full, min_thickness: 20, ..test_zone("GND", rect_outline(0, 0, 400, 100)) };
    let blocker = FillPad { net: Some("OTHER".into()), layers: vec!["F.Cu".into()], copper: Shape::Rect { x0: 190, y0: -50, x1: 210, y1: 150 }, hole: None, ..Default::default() };
    let keeper = FillPad { net: Some("GND".into()), layers: vec!["F.Cu".into()], copper: Shape::Rect { x0: 300, y0: 40, x1: 320, y1: 60 }, hole: None, ..Default::default() };

    let input_always = FillInput { pads: vec![blocker.clone(), keeper.clone()], ..Default::default() };
    let fill_always = fill_zone(&zone, "F.Cu", &input_always, no_clearance, MAX_ERROR);

    assert!(poly_set_contains_pt(&fill_always, Point64::new(300, 50)), "right (connected) half should remain");
    assert!(!poly_set_contains_pt(&fill_always, Point64::new(50, 50)), "left (disconnected) island should be removed under ALWAYS");

    let zone_never = Zone { island_removal_mode: IslandRemovalMode::Never, ..zone };
    let input_never = FillInput { pads: vec![blocker, keeper], ..Default::default() };
    let fill_never = fill_zone(&zone_never, "F.Cu", &input_never, no_clearance, MAX_ERROR);

    assert!(poly_set_contains_pt(&fill_never, Point64::new(300, 50)), "right half should remain under NEVER too");
    assert!(poly_set_contains_pt(&fill_never, Point64::new(50, 50)), "left island should survive under NEVER");
}

#[test]
fn higher_priority_same_net_zone_takes_its_area() {
    let zone = Zone { priority: 0, ..test_zone("GND", rect_outline(0, 0, 1000, 1000)) };
    let input = FillInput { other_zones: vec![FillZoneRef { net: Some("GND".into()), layer: "F.Cu".into(), outline: eda_zone_filler::chain_from_ir(&rect_outline(400, 400, 600, 600)), priority: 1, teardrop: false, ..Default::default() }], ..Default::default() };

    let fill = fill_zone(&zone, "F.Cu", &input, no_clearance, MAX_ERROR);
    assert!(!poly_set_contains_pt(&fill, Point64::new(500, 500)), "higher-priority same-net zone should own its area");
    assert!(poly_set_contains_pt(&fill, Point64::new(50, 50)), "the rest of the zone should still be filled");
}

#[test]
fn different_net_zone_gets_a_clearance_gap_not_an_exact_cut() {
    // `buildDifferentNetZoneClearances` only knocks a zone out for an
    // *other*, higher-priority zone (`knockoutZoneClearance`'s own
    // `aKnockout->HigherPriority(aZone)` gate) -- equal priority (the
    // default for both zones here otherwise) knocks out neither.
    let zone = Zone { priority: 0, ..test_zone("GND", rect_outline(0, 0, 1000, 1000)) };
    let input = FillInput { other_zones: vec![FillZoneRef { net: Some("VCC".into()), layer: "F.Cu".into(), outline: eda_zone_filler::chain_from_ir(&rect_outline(400, 400, 600, 600)), priority: 1, teardrop: false, ..Default::default() }], ..Default::default() };

    let fill = fill_zone(&zone, "F.Cu", &input, no_clearance, MAX_ERROR);
    assert!(!poly_set_contains_pt(&fill, Point64::new(500, 500)));
    // the clearance gap means even just outside the other zone's outline is cleared
    assert!(!poly_set_contains_pt(&fill, Point64::new(395, 500)), "clearance gap around the different-net zone should be cleared too");
}

#[test]
fn a_copper_pour_keepout_cuts_a_hole_regardless_of_net_or_priority() {
    // Task item 3: a rule area with "no copper pours" must exclude copper
    // from every zone that overlaps it, same net or not, any priority --
    // unlike `other_zones`, which only knocks out a *lower*-priority zone
    // and only charges a clearance gap for a different net.
    let zone = Zone { priority: 99, ..test_zone("GND", rect_outline(0, 0, 1000, 1000)) };
    let input = FillInput { keepouts: vec![FillKeepout { layer: "F.Cu".into(), outline: eda_zone_filler::chain_from_ir(&rect_outline(400, 400, 600, 600)) }], ..Default::default() };

    let fill = fill_zone(&zone, "F.Cu", &input, no_clearance, MAX_ERROR);
    assert!(!poly_set_contains_pt(&fill, Point64::new(500, 500)), "the keepout's own area must be excluded from the fill");
    assert!(poly_set_contains_pt(&fill, Point64::new(50, 50)), "the rest of the zone should still be filled");
}

#[test]
fn a_keepout_on_a_different_layer_does_not_affect_this_fill() {
    let zone = test_zone("GND", rect_outline(0, 0, 1000, 1000));
    let input = FillInput { keepouts: vec![FillKeepout { layer: "B.Cu".into(), outline: eda_zone_filler::chain_from_ir(&rect_outline(400, 400, 600, 600)) }], ..Default::default() };

    let fill = fill_zone(&zone, "F.Cu", &input, no_clearance, MAX_ERROR);
    assert!(poly_set_contains_pt(&fill, Point64::new(500, 500)), "a B.Cu keepout must not touch an F.Cu fill");
}
