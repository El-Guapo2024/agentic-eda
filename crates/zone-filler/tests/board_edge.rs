//! The board outline in the fill: the copper keeps `copper_edge_clearance` from every Edge.Cuts item (`knockoutGraphicClearance`: the curve of an
//! arc, the ring of a circle), the fill is clipped to the outline with its cutouts (`BuildSmoothedPoly( .., m_boardOutline )`) when the outline
//! is well-formed and not when it is not (`m_brdOutlinesValid`), and an island that lies outside the board is dropped.

use eda_clipper2::Point64;
use eda_model::ir::{IslandRemovalMode, PadConnection, Point, Zone};
use eda_model::outline::{circle_contour, Arc};
use eda_shape_poly_set::ShapePolySet;
use eda_zone_filler::spokes::poly_set_contains;
use eda_zone_filler::{fill_board, FillEdge, FillInput};

const MAX_ERROR: i64 = 5;

fn p(x: i64, y: i64) -> Point {
    Point { x, y }
}

fn p64(q: Point) -> Point64 {
    Point64::new(q.x, q.y)
}

fn ring(pts: &[Point]) -> Vec<Point64> {
    pts.iter().map(|&q| p64(q)).collect()
}

/// A GND zone over a 40 x 30 mm board, with room round it so that it is the outline, not the zone, that limits the fill.
fn zone() -> Zone {
    Zone {
        id: "z".into(),
        net: "GND".into(),
        layer: "F.Cu".into(),
        outline: vec![p(-5_000, -5_000), p(45_000, -5_000), p(45_000, 35_000), p(-5_000, 35_000)],
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

/// The outline of the board and what a round hole leaves of it, with the edge items that make them.
fn board_with_a_round_hole(clear: i64) -> FillInput {
    let outer = [p(0, 0), p(40_000, 0), p(40_000, 30_000), p(0, 30_000)];
    let hole = circle_contour(p(20_000, 15_000), p(23_000, 15_000), MAX_ERROR as f64);
    let mut polys = ShapePolySet::from_outline(ring(&outer));
    polys.add_hole(ring(&hole), Some(0));
    FillInput {
        board_outline: Some(polys),
        edge_cuts: vec![FillEdge { pts: ring(&outer), closed: true, filled: false }, FillEdge { pts: ring(&hole), closed: true, filled: false }],
        edge_clearance: clear,
        ..Default::default()
    }
}

#[test]
fn copper_keeps_the_edge_clearance_from_the_ring_of_a_cutout_and_the_outer_edge() {
    let input = board_with_a_round_hole(500);
    let fills = fill_board(&[zone()], &input, clearance, MAX_ERROR);
    let fill = &fills[0];
    // Away from everything there is copper.
    assert!(copper(fill, 5_000, 5_000));
    // Not outside the board, and 0.5 mm in from the outer edge.
    assert!(!copper(fill, -1_000, 15_000));
    assert!(!copper(fill, 300, 15_000), "300 um from the edge is inside the 500 um");
    assert!(copper(fill, 700, 15_000), "700 um from the edge is outside it");
    // The hole has no copper in it, and none within 0.5 mm of its rim (3 mm radius, so out to 3.5 mm).
    assert!(!copper(fill, 20_000, 15_000), "the middle of the hole");
    assert!(!copper(fill, 20_000 + 3_300, 15_000), "300 um outside the rim is inside the clearance");
    assert!(copper(fill, 20_000 + 3_700, 15_000), "700 um outside the rim is clear of it");
    assert!(!copper(fill, 20_000, 15_000 - 3_300));
    assert!(copper(fill, 20_000, 15_000 - 3_700));
}

#[test]
fn an_arc_edge_is_kept_clear_along_its_curve_not_along_its_chord() {
    // The top-right corner of a board is a 6 mm arc: the chord from (34, 0) to (40, 6) cuts it off, and copper may go between the two.
    let r = 6_000.0;
    let centre = p(34_000, 6_000);
    let mid = p(centre.x + (r * std::f64::consts::FRAC_1_SQRT_2).round() as i64, centre.y - (r * std::f64::consts::FRAC_1_SQRT_2).round() as i64);
    let arc = Arc::through(p(34_000, 0), mid, p(40_000, 6_000)).polyline(MAX_ERROR as f64);
    let mut outer: Vec<Point> = vec![p(0, 0)];
    outer.extend(arc.iter().copied());
    outer.extend([p(40_000, 30_000), p(0, 30_000)]);
    let input = FillInput {
        board_outline: Some(ShapePolySet::from_outline(ring(&outer))),
        edge_cuts: vec![
            FillEdge { pts: vec![p64(p(0, 0)), p64(p(34_000, 0))], closed: false, filled: false },
            FillEdge { pts: arc.iter().map(|&q| p64(q)).collect(), closed: false, filled: false },
            FillEdge { pts: vec![p64(p(40_000, 6_000)), p64(p(40_000, 30_000)), p64(p(0, 30_000)), p64(p(0, 0))], closed: false, filled: false },
        ],
        edge_clearance: 500,
        ..Default::default()
    };
    let fills = fill_board(&[zone()], &input, clearance, MAX_ERROR);
    let fill = &fills[0];
    // On the diagonal from the arc's centre, 300 um inside the curve: inside the clearance. 700 um inside: clear.
    let diag = |d: f64| (centre.x + (d * std::f64::consts::FRAC_1_SQRT_2) as i64, centre.y - (d * std::f64::consts::FRAC_1_SQRT_2) as i64);
    let (x, y) = diag(r - 300.0);
    assert!(!copper(fill, x, y), "300 um inside the curve");
    let (x, y) = diag(r - 700.0);
    assert!(copper(fill, x, y), "700 um inside the curve");
    // Outside the curve but inside the chord there is no board, so no copper.
    let (x, y) = diag(r + 300.0);
    assert!(!copper(fill, x, y));
    // The chord's corner region: a point 200 um inside the chord line is copper-free only because of the arc if it is within 500 of the curve.
    // (34 + 6 sin 45... ) a point on the chord's midpoint is 6(1 - cos 45) = 1.76 mm from the curve: copper is allowed there.
    let chord_mid = p((34_000 + 40_000) / 2 - 300, 3_000 + 300);
    assert!(copper(fill, chord_mid.x, chord_mid.y), "{chord_mid:?}: the chord is not the edge");
}

#[test]
fn a_solid_edge_shape_is_knocked_out_inside_as_well() {
    // A solid 4 x 4 mm disc drawn on Edge.Cuts: nothing inside it, nothing within the clearance.
    let disc = circle_contour(p(20_000, 15_000), p(22_000, 15_000), MAX_ERROR as f64);
    let input = FillInput { edge_cuts: vec![FillEdge { pts: ring(&disc), closed: true, filled: true }], edge_clearance: 500, ..Default::default() };
    let fill = &fill_board(&[zone()], &input, clearance, MAX_ERROR)[0];
    assert!(!copper(fill, 20_000, 15_000));
    assert!(!copper(fill, 22_300, 15_000));
    assert!(copper(fill, 22_700, 15_000));
}

#[test]
fn an_outline_that_is_not_well_formed_does_not_clip_the_fill_but_its_edges_still_keep_copper_away() {
    // The edges do not close: KiCad guesses the rectangle round them and `m_brdOutlinesValid` is false, so the fill is not cut to it.
    let guess = ShapePolySet::from_outline(ring(&[p(0, 0), p(10_000, 0), p(10_000, 10_000), p(0, 10_000)]));
    let input = FillInput {
        board_outline: Some(guess),
        board_outline_invalid: true,
        edge_cuts: vec![FillEdge { pts: vec![p64(p(0, 0)), p64(p(10_000, 0))], closed: false, filled: false }],
        edge_clearance: 500,
        ..Default::default()
    };
    // A zone a little bigger than the guess: most of it is inside, so it is not an island of the board, and it spills over.
    let zone = Zone { outline: vec![p(-1_000, -1_000), p(11_000, -1_000), p(11_000, 11_000), p(-1_000, 11_000)], ..zone() };
    let fill = &fill_board(&[zone.clone()], &input, clearance, MAX_ERROR)[0];
    assert!(copper(fill, 10_500, 5_000), "not clipped to the guessed rectangle");
    assert!(!copper(fill, 5_000, 300), "but the edge that is there is kept clear");
    assert!(copper(fill, 5_000, 700));
    // Valid, the same rectangle clips it.
    let valid = FillInput { board_outline_invalid: false, ..input };
    let fill = &fill_board(&[zone], &valid, clearance, MAX_ERROR)[0];
    assert!(!copper(fill, 10_500, 5_000));
    assert!(copper(fill, 5_000, 5_000));
}

#[test]
fn an_island_outside_the_board_is_dropped_whatever_the_outline_is_worth() {
    // Two outlines, the zone reaches both; copper only stays on the board (the second one is 10 mm away, an island of the fill).
    let two = {
        let mut s = ShapePolySet::from_outline(ring(&[p(0, 0), p(10_000, 0), p(10_000, 10_000), p(0, 10_000)]));
        s.add_outline(ring(&[p(20_000, 0), p(30_000, 0), p(30_000, 10_000), p(20_000, 10_000)]));
        s
    };
    let input = FillInput { board_outline: Some(two), ..Default::default() };
    let zone = Zone { island_removal_mode: IslandRemovalMode::Never, ..zone() };
    let fill = &fill_board(&[zone], &input, clearance, MAX_ERROR)[0];
    assert!(copper(fill, 5_000, 5_000) && copper(fill, 25_000, 5_000), "both outlines are board");
    assert!(!copper(fill, 15_000, 5_000), "between them is not");
}
