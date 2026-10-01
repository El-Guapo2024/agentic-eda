//! Boolean ops, inflate/deflate, Fracture/Unfracture and Simplify tests for
//! `ShapePolySet`, per the task's stage-2 requirements.

use eda_clipper2::Point64;
use eda_shape_poly_set::{CornerStrategy, ShapePolySet};

fn square(x0: i64, y0: i64, side: i64) -> Vec<Point64> {
    vec![Point64::new(x0, y0), Point64::new(x0 + side, y0), Point64::new(x0 + side, y0 + side), Point64::new(x0, y0 + side)]
}

#[test]
fn outline_with_hole_area() {
    let mut sps = ShapePolySet::new();
    sps.add_outline(square(0, 0, 20));
    sps.add_hole(square(5, 5, 10), None);
    assert_eq!(sps.area(), 300.0); // 400 - 100
    assert!(sps.has_holes());
}

#[test]
fn boolean_add_merges_overlapping_outlines() {
    let mut a = ShapePolySet::new();
    a.add_outline(square(0, 0, 10));
    let mut b = ShapePolySet::new();
    b.add_outline(square(5, 0, 10));

    a.boolean_add(&b);
    assert_eq!(a.outline_count(), 1);
    assert_eq!(a.area(), 150.0);
}

#[test]
fn boolean_subtract_knocks_out_a_pad() {
    let mut zone = ShapePolySet::new();
    zone.add_outline(square(0, 0, 100));
    let mut pad = ShapePolySet::new();
    pad.add_outline(square(10, 10, 20));

    zone.boolean_subtract(&pad);
    assert_eq!(zone.area(), 10_000.0 - 400.0);
}

#[test]
fn boolean_xor_of_disjoint_shapes_keeps_both() {
    let mut a = ShapePolySet::new();
    a.add_outline(square(0, 0, 10));
    let mut b = ShapePolySet::new();
    b.add_outline(square(100, 0, 10));
    a.boolean_xor(&b);
    assert_eq!(a.outline_count(), 2);
    assert_eq!(a.area(), 200.0);
}

#[test]
fn inflate_miter_grows_square_exactly() {
    let mut sps = ShapePolySet::new();
    sps.add_outline(square(0, 0, 1000));
    sps.inflate(100, CornerStrategy::AllowAcuteCorners, 5, false);
    let expected = 1200.0 * 1200.0;
    assert!((sps.area() - expected).abs() / expected < 0.001);
}

#[test]
fn deflate_shrinks_square_exactly() {
    let mut sps = ShapePolySet::new();
    sps.add_outline(square(0, 0, 1000));
    sps.deflate(100, CornerStrategy::AllowAcuteCorners, 5);
    let expected = 800.0 * 800.0;
    assert!((sps.area() - expected).abs() / expected < 0.001);
}

#[test]
fn inflate_round_matches_closed_form() {
    let side = 10_000i64;
    let delta = 1_000i64;
    let mut sps = ShapePolySet::new();
    sps.add_outline(square(0, 0, side));
    sps.inflate(delta, CornerStrategy::RoundAllCorners, 10, false);

    let expected = (side as f64).powi(2) + 4.0 * side as f64 * delta as f64 + std::f64::consts::PI * (delta as f64).powi(2);
    assert!((sps.area() - expected).abs() / expected < 0.01);
}

#[test]
fn fracture_produces_single_contour_preserving_area() {
    let mut sps = ShapePolySet::new();
    sps.add_outline(square(0, 0, 20));
    sps.add_hole(square(5, 5, 10), None);
    let area_before = sps.area();

    sps.fracture(true);

    assert_eq!(sps.outline_count(), 1);
    assert_eq!(sps.hole_count(0), 0); // fractured into one slitted contour, no separate holes
                                       // Polygon area of a fractured (slitted) outline, taken as a
                                       // simple (non-"outline minus holes") signed polygon, equals
                                       // the original net area: the in-and-back-out slit contributes
                                       // zero net area.
    let fractured_area = eda_clipper2::area(sps.outline(0)).abs();
    assert!((fractured_area - area_before).abs() < 1.0, "fractured {fractured_area} vs original {area_before}");
}

#[test]
fn fracture_is_a_no_op_for_a_single_outline() {
    let mut sps = ShapePolySet::new();
    sps.add_outline(square(0, 0, 20));
    let before = sps.outline(0).clone();
    sps.fracture(false);
    assert_eq!(sps.outline(0), &before);
}

#[test]
fn unfracture_recovers_outline_and_hole() {
    let mut sps = ShapePolySet::new();
    sps.add_outline(square(0, 0, 20));
    sps.add_hole(square(5, 5, 10), None);
    let area_before = sps.area();

    sps.fracture(true);
    assert_eq!(sps.hole_count(0), 0);

    sps.unfracture();
    assert_eq!(sps.outline_count(), 1);
    assert_eq!(sps.hole_count(0), 1);
    assert!((sps.area() - area_before).abs() < 1.0);
}

#[test]
fn simplify_merges_overlapping_polygons() {
    let mut sps = ShapePolySet::new();
    sps.add_outline(square(0, 0, 10));
    sps.add_outline(square(5, 0, 10)); // overlaps the first
    sps.simplify();
    assert_eq!(sps.outline_count(), 1);
    assert_eq!(sps.area(), 150.0);
}

#[test]
fn inflate_with_linked_holes_keeps_hole_linked_through_growth() {
    // `InflateWithLinkedHoles` -> `Unfracture()` -> `unfractureSingle`
    // asserts its input polygon already has exactly one (fractured)
    // contour, same as upstream: this op is meant to run on a zone's own
    // previously-`Fracture()`d fill state, not on freshly-built
    // outline+hole data straight off `AddOutline`/`AddHole`.
    let mut sps = ShapePolySet::new();
    sps.add_outline(square(0, 0, 100));
    sps.add_hole(square(40, 40, 20), None);
    sps.fracture(true);
    let before = sps.area();

    sps.inflate_with_linked_holes(10, CornerStrategy::AllowAcuteCorners, 5);

    // outline grew, hole shrank (both offset the same direction relative to
    // their own winding), net area should have grown versus the original.
    assert!(sps.area() > before);
    assert_eq!(sps.outline_count(), 1);
}
