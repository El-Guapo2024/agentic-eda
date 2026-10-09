//! `fill_board` (`ZONE_FILLER::Fill`): zones knocked out by each other's copper, islands removed by connectivity, and the
//! iterative refill of the zones below a zone that lost islands.

use eda_clipper2::Point64;
use eda_model::ir::{IslandRemovalMode, PadConnection, Point, Zone};
use eda_shape_poly_set::ShapePolySet;
use eda_zone_filler::shape::Shape;
use eda_zone_filler::spokes::poly_set_contains;
use eda_zone_filler::{fill_board, FillInput, FillPad, FillTrack};

const MAX_ERROR: i64 = 5;

fn square(x0: i64, y0: i64, x1: i64, y1: i64) -> Vec<Point> {
    vec![Point { x: x0, y: y0 }, Point { x: x1, y: y0 }, Point { x: x1, y: y1 }, Point { x: x0, y: y1 }]
}

fn zone(id: &str, net: &str, priority: u32) -> Zone {
    Zone {
        id: id.into(),
        net: net.into(),
        layer: "F.Cu".into(),
        outline: square(0, 0, 10_000, 10_000),
        priority,
        clearance: 200,
        min_thickness: 200,
        pad_connection: PadConnection::Full,
        island_removal_mode: IslandRemovalMode::Always,
        ..Default::default()
    }
}

fn clearance(_a: Option<&str>, _b: Option<&str>) -> i64 {
    200
}

fn copper(fill: &ShapePolySet, x: i64, y: i64) -> bool {
    poly_set_contains(fill, Point64::new(x, y))
}

fn vdd_pad() -> FillPad {
    FillPad { net: Some("VDD".into()), layers: vec!["F.Cu".into()], copper: Shape::Rect { x0: 800, y0: 4_800, x1: 1_200, y1: 5_200 }, ..Default::default() }
}

/// A GND track straight through both zones, which cuts the VDD zone in two halves.
fn gnd_track() -> FillTrack {
    FillTrack { net: Some("GND".into()), layer: "F.Cu".into(), a: Point64::new(5_000, -1_000), b: Point64::new(5_000, 11_000), width: 200 }
}

#[test]
fn a_zone_that_loses_its_islands_gives_the_space_back_to_the_zone_below() {
    let zones = vec![zone("v", "VDD", 1), zone("g", "GND", 0)];
    let input = FillInput { pads: vec![vdd_pad()], tracks: vec![gnd_track()], ..Default::default() };
    let fills = fill_board(&zones, &input, clearance, MAX_ERROR);
    let (vdd, gnd) = (&fills[0], &fills[1]);

    // VDD keeps the half that holds its pad, and drops the one that touches nothing of its net.
    assert!(copper(vdd, 2_500, 5_000), "the half with the pad stays");
    assert!(!copper(vdd, 7_500, 5_000), "the other half is an island and goes");

    // GND is knocked out by VDD's copper on the left, but takes the freed half on the right (refilled from its cache).
    assert!(!copper(gnd, 2_500, 3_000), "VDD's copper (and its clearance) keeps GND out");
    assert!(copper(gnd, 7_500, 3_000), "the freed half goes to GND");
}

#[test]
fn a_higher_priority_zone_knocks_out_the_lower_one_by_its_copper_not_its_outline() {
    // VDD's outline covers everything but, with no pad of its net anywhere, all of its fill is one island: it is kept (all
    // islands: nothing to remove), yet GND below it still is not allowed inside it.
    let zones = vec![zone("v", "VDD", 1), zone("g", "GND", 0)];
    let input = FillInput::default();
    let fills = fill_board(&zones, &input, clearance, MAX_ERROR);
    assert!(copper(&fills[0], 5_000, 5_000), "all-islands zones are left as they are");
    assert!(!copper(&fills[1], 5_000, 5_000), "GND is knocked out by VDD's fill");

    // A VDD zone that is empty after its own knockouts (a keepout over all of it) leaves GND the whole area.
    let mut covered = zone("v", "VDD", 1);
    covered.outline = square(0, 0, 4_000, 4_000);
    let zones = vec![covered, zone("g", "GND", 0)];
    let input = FillInput { keepouts: vec![eda_zone_filler::FillKeepout { layer: "F.Cu".into(), outline: eda_zone_filler::chain_from_ir(&square(-100, -100, 4_100, 4_100)) }], ..Default::default() };
    let fills = fill_board(&zones, &input, clearance, MAX_ERROR);
    // the keepout takes GND's copper under it too, but beyond the keepout GND is complete
    assert!(copper(&fills[1], 7_000, 7_000));
}

#[test]
fn equal_priorities_are_ordered_by_id() {
    // Two overlapping zones of different nets at the same priority: one of them wins, by id (KiCad: by UUID), and the
    // other keeps clear of it. Swapping the ids swaps the winner.
    let make = |a: &str, b: &str| {
        let zones = vec![zone(a, "VDD", 0), zone(b, "GND", 0)];
        let input = FillInput { pads: vec![vdd_pad()], ..Default::default() };
        fill_board(&zones, &input, clearance, MAX_ERROR)
    };
    let first = make("b", "a");
    assert!(copper(&first[0], 5_000, 5_000) && !copper(&first[1], 5_000, 5_000), "the larger id wins");
    let second = make("a", "b");
    assert!(!copper(&second[0], 5_000, 5_000) && copper(&second[1], 5_000, 5_000));
}

#[test]
fn a_fragment_joined_to_a_pad_by_a_track_is_not_an_island() {
    // VDD pad in the left half, a VDD track across the GND wall to the right half: both halves are one cluster.
    let zones = vec![zone("v", "VDD", 1)];
    let bridge = FillTrack { net: Some("VDD".into()), layer: "F.Cu".into(), a: Point64::new(1_000, 9_000), b: Point64::new(9_000, 9_000), width: 200 };
    let input = FillInput { pads: vec![vdd_pad()], tracks: vec![gnd_track(), bridge], ..Default::default() };
    let fills = fill_board(&zones, &input, clearance, MAX_ERROR);
    assert!(copper(&fills[0], 2_500, 5_000));
    assert!(copper(&fills[0], 7_500, 5_000), "the right half touches the VDD track that reaches the pad's half");
    assert!(!copper(&fills[0], 5_000, 5_000), "the wall's own clearance stays open");
}
