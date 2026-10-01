//! Offset-area-vs-closed-form tests for `ClipperOffset`, per the task's
//! stage-1 test requirements.

use eda_clipper2::{area, area_paths, ClipperOffset, EndType, JoinType, Path64, Point64};

fn square(x0: i64, y0: i64, side: i64) -> Path64 {
    vec![Point64::new(x0, y0), Point64::new(x0 + side, y0), Point64::new(x0 + side, y0 + side), Point64::new(x0, y0 + side)]
}

#[test]
fn miter_outward_offset_of_square_is_exact() {
    // mitering a square's 90deg corners outward reproduces another exact
    // axis-aligned square of side L+2d (a standard, exact closed form).
    let side = 1000i64;
    let delta = 100.0;
    let sq = square(0, 0, side);

    let mut co = ClipperOffset::new(10.0, 0.0, false, false);
    co.add_path(&sq, JoinType::Miter, EndType::Polygon);
    let mut solution = Vec::new();
    co.execute(delta, &mut solution);

    assert_eq!(solution.len(), 1);
    let expected = (side as f64 + 2.0 * delta).powi(2);
    let got = area_paths(&solution);
    assert!((got - expected).abs() / expected < 0.001, "got {got}, expected {expected}");
}

#[test]
fn round_outward_offset_of_square_matches_closed_form() {
    // L^2 + 4*L*d (four edge strips) + pi*d^2 (four quarter-circle corners
    // = one full circle), exact except for the round join's own polygonal
    // approximation error -- tightened below 0.1% via `arc_tolerance`.
    let side = 10_000i64;
    let delta = 1_000.0;
    let sq = square(0, 0, side);

    let mut co = ClipperOffset::new(2.0, 1.0, false, false);
    co.add_path(&sq, JoinType::Round, EndType::Polygon);
    let mut solution = Vec::new();
    co.execute(delta, &mut solution);

    let expected = (side as f64).powi(2) + 4.0 * side as f64 * delta + std::f64::consts::PI * delta * delta;
    let got = area_paths(&solution);
    assert!((got - expected).abs() / expected < 0.001, "got {got}, expected {expected}");
}

#[test]
fn inward_offset_shrinks_square() {
    let side = 1000i64;
    let delta = -100.0;
    let sq = square(0, 0, side);

    let mut co = ClipperOffset::new(10.0, 0.0, false, false);
    co.add_path(&sq, JoinType::Miter, EndType::Polygon);
    let mut solution = Vec::new();
    co.execute(delta, &mut solution);

    assert_eq!(solution.len(), 1);
    let expected = (side as f64 + 2.0 * delta).powi(2); // delta is negative
    let got = area_paths(&solution);
    assert!((got - expected).abs() / expected < 0.001, "got {got}, expected {expected}");
}

#[test]
fn inward_offset_larger_than_half_width_collapses_to_empty() {
    let side = 1000i64;
    let sq = square(0, 0, side);

    let mut co = ClipperOffset::new(10.0, 0.0, false, false);
    co.add_path(&sq, JoinType::Miter, EndType::Polygon);
    let mut solution = Vec::new();
    co.execute(-600.0, &mut solution); // more than half of 1000
    assert!(solution.is_empty() || area_paths(&solution).abs() < 1.0);
}

#[test]
fn bevel_and_square_joins_do_not_panic_and_grow_area() {
    let side = 1000i64;
    let sq = square(0, 0, side);
    for jt in [JoinType::Bevel, JoinType::Square] {
        let mut co = ClipperOffset::new(2.0, 0.0, false, false);
        co.add_path(&sq, jt, EndType::Polygon);
        let mut solution = Vec::new();
        co.execute(100.0, &mut solution);
        let got = area_paths(&solution);
        assert!(got > (side * side) as f64, "{jt:?} offset should grow area, got {got}");
    }
}

#[test]
fn open_path_square_end_type_builds_a_capsule() {
    // a single horizontal segment, offset with Round end caps, should trace
    // out a capsule / stadium shape: area = 2*halfLen*2*delta (rectangle)
    // + pi*delta^2 (two half-circle caps = one circle).
    let half_len = 1000i64;
    let delta = 200.0;
    let line = vec![Point64::new(-half_len, 0), Point64::new(half_len, 0)];

    let mut co = ClipperOffset::new(2.0, 0.5, false, false);
    co.add_path(&line, JoinType::Round, EndType::Round);
    let mut solution = Vec::new();
    co.execute(delta, &mut solution);

    let expected = (2.0 * half_len as f64) * (2.0 * delta) + std::f64::consts::PI * delta * delta;
    let got = area_paths(&solution);
    assert!((got - expected).abs() / expected < 0.01, "got {got}, expected {expected}");
}

#[test]
fn zero_delta_is_a_no_op() {
    let sq = square(0, 0, 1000);
    let mut co = ClipperOffset::new(2.0, 0.0, false, false);
    co.add_path(&sq, JoinType::Miter, EndType::Polygon);
    let mut solution = Vec::new();
    co.execute(0.0, &mut solution);
    // delta rounds to "insignificant" (< 0.5) and returns the input as-is
    assert_eq!(solution.len(), 1);
    assert_eq!(area(&solution[0]).abs(), 1_000_000.0);
}
