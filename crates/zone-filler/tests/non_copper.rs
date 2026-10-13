//! Non-copper zones (`ZONE_FILLER::fillNonCopperZone`) and zones on several layers (`ZONE_FILLER::Fill` over `GetLayerSet()`).

use eda_clipper2::Point64;
use eda_model::ir::{FillMode, PadConnection, Point, Zone};
use eda_shape_poly_set::ShapePolySet;
use eda_zone_filler::shape::Shape;
use eda_zone_filler::spokes::poly_set_contains;
use eda_zone_filler::{fill_board, fill_board_layers, fill_zone, FillInput, FillKeepout, FillKnockout, FillPad, FillTrack};

const MAX_ERROR: i64 = 5;

fn square(x0: i64, y0: i64, x1: i64, y1: i64) -> Vec<Point> {
    vec![Point { x: x0, y: y0 }, Point { x: x1, y: y0 }, Point { x: x1, y: y1 }, Point { x: x0, y: y1 }]
}

fn clearance(_a: Option<&str>, _b: Option<&str>) -> i64 {
    200
}

fn copper(fill: &ShapePolySet, x: i64, y: i64) -> bool {
    poly_set_contains(fill, Point64::new(x, y))
}

/// A 10 x 10 mm non-copper zone on `layer`, 0.254 mm minimum width (the default KiCad draws a non-copper zone with).
fn silk_zone(layer: &str) -> Zone {
    Zone { id: "s".into(), layer: layer.into(), outline: square(0, 0, 10_000, 10_000), min_thickness: 254, clearance: 500, ..Default::default() }
}

#[test]
fn a_non_copper_zone_is_its_outline_with_the_corners_rounded_by_the_minimum_width() {
    let zone = silk_zone("F.SilkS");
    let fill = fill_zone(&zone, "F.SilkS", &FillInput::default(), clearance, MAX_ERROR);
    assert_eq!(fill.outline_count(), 1);
    // The deflate by half the width less 1 um (chamfered corners) and the round re-inflate leave the sides where they were and round each corner by
    // 126 um: the area is the square's, less four corners of (1 - pi/4) r^2.
    let r = 126.0;
    let want = 100_000_000.0 - 4.0 * (1.0 - std::f64::consts::FRAC_PI_4) * r * r;
    assert!((fill.area() - want).abs() < 5_000.0, "{} against {want}", fill.area());
    assert!(copper(&fill, 5_000, 5_000) && copper(&fill, 20, 5_000) && !copper(&fill, -20, 5_000));
}

#[test]
fn a_non_copper_zone_keeps_clear_of_nothing_a_pour_would() {
    // A pad, a track and a via of another net on the same layer name: no clearance, no thermal relief, no connectivity.
    let zone = silk_zone("F.Mask");
    let input = FillInput {
        pads: vec![FillPad { net: Some("GND".into()), layers: vec!["F.Mask".into()], copper: Shape::Rect { x0: 4_000, y0: 4_000, x1: 6_000, y1: 6_000 }, ..Default::default() }],
        tracks: vec![FillTrack { net: Some("VDD".into()), layer: "F.Mask".into(), a: Point64::new(0, 2_000), b: Point64::new(10_000, 2_000), width: 400 }],
        ..Default::default()
    };
    let with_items = fill_zone(&zone, "F.Mask", &input, clearance, MAX_ERROR);
    let bare = fill_zone(&zone, "F.Mask", &FillInput::default(), clearance, MAX_ERROR);
    assert!((with_items.area() - bare.area()).abs() < 1.0, "the items change nothing: {} against {}", with_items.area(), bare.area());
    assert!(copper(&with_items, 5_000, 5_000) && copper(&with_items, 5_000, 2_000));
}

#[test]
fn a_knockout_text_is_cut_out_where_its_glyphs_are_and_only_on_its_own_layer() {
    let zone = silk_zone("F.SilkS");
    // A glyph stroke 0.2 mm wide across the middle of the zone.
    let stroke = Shape::Stadium { a: Point64::new(3_000, 5_000), b: Point64::new(7_000, 5_000), r: 100 };
    let on_layer = FillInput { knockouts: vec![FillKnockout { layer: "F.SilkS".into(), shape: stroke.clone() }], ..Default::default() };
    let fill = fill_zone(&zone, "F.SilkS", &on_layer, clearance, MAX_ERROR);
    assert!(!copper(&fill, 5_000, 5_000), "the glyph is a hole");
    assert!(copper(&fill, 5_000, 5_300) && copper(&fill, 5_000, 4_700), "the copper round it is not touched (no clearance)");
    assert!(copper(&fill, 2_000, 5_000), "beyond the stroke's end");

    // The same text on the back silkscreen does nothing to a front zone.
    let elsewhere = FillInput { knockouts: vec![FillKnockout { layer: "B.SilkS".into(), shape: stroke }], ..Default::default() };
    let fill = fill_zone(&zone, "F.SilkS", &elsewhere, clearance, MAX_ERROR);
    assert!(copper(&fill, 5_000, 5_000));
}

#[test]
fn a_rule_area_that_disallows_copper_pours_cuts_a_non_copper_zone_on_its_layer_by_its_outline() {
    let zone = silk_zone("F.SilkS");
    let keepout = |layer: &str| FillInput { keepouts: vec![FillKeepout { layer: layer.into(), outline: eda_zone_filler::chain_from_ir(&square(4_000, -1_000, 6_000, 11_000)) }], ..Default::default() };
    let cut = fill_zone(&zone, "F.SilkS", &keepout("F.SilkS"), clearance, MAX_ERROR);
    assert!(!copper(&cut, 5_000, 5_000) && copper(&cut, 2_000, 5_000) && copper(&cut, 8_000, 5_000));
    assert_eq!(cut.outline_count(), 2, "the keepout splits the zone in two");
    let other = fill_zone(&zone, "F.SilkS", &keepout("B.SilkS"), clearance, MAX_ERROR);
    assert!(copper(&other, 5_000, 5_000), "a keepout on another layer is not here");
}

#[test]
fn a_zone_with_no_minimum_width_is_its_outline_grown_by_the_epsilon() {
    // `fillNonCopperZone` deflates by `half_min_width - epsilon` even when that is negative, so a width of 0 grows the fill by 1 um; there is no re-inflate.
    let zone = Zone { min_thickness: 0, ..silk_zone("F.Mask") };
    let fill = fill_zone(&zone, "F.Mask", &FillInput::default(), clearance, MAX_ERROR);
    assert!(fill.area() > 100_000_000.0 && fill.area() < 100_000_000.0 + 4.0 * 10_000.0 + 100.0, "{}", fill.area());
    assert!(copper(&fill, 0, 5_000), "the outline's own edge is inside");
}

#[test]
fn a_hatched_non_copper_zone_has_holes_and_no_thermal_rings() {
    let zone = Zone { fill_mode: FillMode::HatchPattern, hatch_thickness: 600, hatch_gap: 1_000, ..silk_zone("F.Fab") };
    let fill = fill_zone(&zone, "F.Fab", &FillInput::default(), clearance, MAX_ERROR);
    let solid = fill_zone(&silk_zone("F.Fab"), "F.Fab", &FillInput::default(), clearance, MAX_ERROR);
    assert!(fill.area() < 0.8 * solid.area(), "the hatch is mostly holes: {} against {}", fill.area(), solid.area());
    assert!(fill.area() > 0.1 * solid.area());
    // The border of the zone stays solid.
    assert!(copper(&fill, 100, 5_000) && copper(&fill, 5_000, 100));
}

#[test]
fn a_zone_on_two_copper_layers_is_filled_on_each_with_what_is_on_that_layer() {
    let zone = Zone { id: "gnd".into(), net: "GND".into(), layer: "F.Cu".into(), layers: vec!["F.Cu".into(), "B.Cu".into()], outline: square(0, 0, 10_000, 10_000), min_thickness: 200, clearance: 200, pad_connection: PadConnection::Full, ..Default::default() };
    // A VDD track across the front layer only.
    let input = FillInput { tracks: vec![FillTrack { net: Some("VDD".into()), layer: "F.Cu".into(), a: Point64::new(-1_000, 5_000), b: Point64::new(11_000, 5_000), width: 400 }], ..Default::default() };
    let fills = fill_board_layers(&[zone], &input, clearance, MAX_ERROR);
    assert_eq!(fills.iter().map(|f| (f.zone, f.layer.as_str())).collect::<Vec<_>>(), vec![(0, "F.Cu"), (0, "B.Cu")], "one fill per layer, in the order of the set");
    let (front, back) = (&fills[0].fill, &fills[1].fill);
    assert!(!copper(front, 5_000, 5_000), "the track's clearance is open on the front");
    assert!(copper(back, 5_000, 5_000), "and not on the back");
    assert!(front.area() < back.area());
}

/// `ZONE_FILLER::Fill` judges "if *all* the polygons are islands, do not remove any" per zone, over its layers: a zone with a pad on one layer loses the polygons of the
/// layer where nothing of its net is (`issue5750`'s zone `B` has no copper on B.Cu in kicad-cli's refill), where a zone with no pad anywhere keeps them all.
#[test]
fn a_zone_connected_on_one_layer_loses_the_layer_where_all_of_its_copper_is_an_island() {
    let zone = Zone { id: "gnd".into(), net: "GND".into(), layer: "F.Cu".into(), layers: vec!["F.Cu".into(), "B.Cu".into()], outline: square(0, 0, 10_000, 10_000), min_thickness: 200, clearance: 200, pad_connection: PadConnection::Full, ..Default::default() };
    let pad = FillPad { net: Some("GND".into()), layers: vec!["F.Cu".into()], copper: Shape::Rect { x0: 4_800, y0: 4_800, x1: 5_200, y1: 5_200 }, ..Default::default() };
    let fills = fill_board_layers(&[zone.clone()], &FillInput { pads: vec![pad], ..Default::default() }, clearance, MAX_ERROR);
    assert!(copper(&fills[0].fill, 2_000, 2_000), "the front is connected");
    assert!(fills[1].fill.is_empty(), "the back has no pad of the net: its one polygon is an island, and the front is not, so it goes: {}", fills[1].fill.area());

    // No pad on either layer: every polygon on every layer is an island, and none is removed.
    let fills = fill_board_layers(&[zone], &FillInput::default(), clearance, MAX_ERROR);
    assert!(copper(&fills[0].fill, 2_000, 2_000) && copper(&fills[1].fill, 2_000, 2_000), "a zone of islands only is kept as it is");
}

#[test]
fn a_copper_zone_and_a_non_copper_zone_in_one_board_fill_do_not_see_each_other() {
    // The pour has the higher priority and another net, and covers the silkscreen zone entirely: it knocks out nothing there, they are on different layers.
    let pour = Zone { id: "p".into(), net: "GND".into(), layer: "F.Cu".into(), outline: square(0, 0, 10_000, 10_000), priority: 5, clearance: 200, min_thickness: 200, pad_connection: PadConnection::Full, ..Default::default() };
    let silk = silk_zone("F.SilkS");
    let fills = fill_board(&[pour, silk], &FillInput::default(), clearance, MAX_ERROR);
    assert!(copper(&fills[0], 5_000, 5_000) && copper(&fills[1], 5_000, 5_000));
    assert!((fills[1].area() - 100_000_000.0).abs() < 100_000.0, "the silkscreen zone is whole: {}", fills[1].area());
}

#[test]
fn two_non_copper_zones_of_one_layer_are_one_shape_where_they_overlap() {
    // Both have no net, so they are "the same net": `BuildSmoothedPoly` merges them, and the overlap is no divot.
    let a = silk_zone("F.SilkS");
    let b = Zone { id: "t".into(), outline: square(8_000, 0, 18_000, 10_000), ..silk_zone("F.SilkS") };
    let fills = fill_board(&[a, b], &FillInput::default(), clearance, MAX_ERROR);
    assert!(copper(&fills[0], 9_000, 5_000), "the first zone's fill includes the area the second shares with it");
    assert!(!copper(&fills[0], 12_000, 5_000), "but stops at its own outline");
    assert!(copper(&fills[1], 12_000, 5_000));
}
