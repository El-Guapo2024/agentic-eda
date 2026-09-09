//! Regression tests for the grid's occupancy/clearance bookkeeping.
//!
//! The scenario reconstructed here is the one that produced illegal copper
//! on `examples/ladder/l2_sensor_hub.yaml`: nets `3V3` and `IMU_AD0` ended
//! up 381 µm apart (3 cells at a 127 µm grid) where track-to-track needs
//! 508 µm (4 cells). The cell holding `IMU_AD0`'s copper was one of its own
//! pad cells that its escape track also ran over — `Grid::set` deliberately
//! keeps `kind == Pad` there, and the required separation used to be
//! derived from that `kind`, so the cell advertised the pad rasterisation's
//! half-cell extent (63 µm) instead of the track's real 100 µm.

use eda_model::ir::Point;
use eda_router::grid::{Grid, Occ};

/// The l2_sensor_hub numbers: 127 µm grid, 200 µm clearance, 200 µm
/// tracks, 600 µm vias, two layers.
fn hub_grid() -> Grid {
    let outline = vec![
        Point { x: 0, y: 0 },
        Point { x: 40000, y: 0 },
        Point { x: 40000, y: 40000 },
        Point { x: 0, y: 40000 },
    ];
    Grid::with_widths(outline, 127, 200, 200, 600, 2)
}

/// Sanity: with nothing but a plain track of another net in the cell, the
/// grid already demanded the right 4-cell separation. This is the baseline
/// the pad case has to match.
#[test]
fn foreign_track_needs_four_cells_from_a_track() {
    let mut g = hub_grid();
    let (cx, cy) = (100i64, 100i64);
    g.set(cx, cy, 0, "IMU_AD0", Occ::Track);
    assert!(!g.passable(cx + 3, cy, 0, "3V3"), "3 cells (381 um) is short of the 508 um a track pair needs");
    assert!(g.passable(cx + 4, cy, 0, "3V3"), "4 cells (508 um) is legal");
}

/// The bug: a net's own escape track running over one of its own pad cells
/// left the cell modelled as pad-sized copper, so a foreign track was
/// allowed one cell too close.
#[test]
fn track_over_own_pad_still_claims_track_clearance() {
    let mut g = hub_grid();
    let (cx, cy) = (100i64, 100i64);
    // Pad copper first (step 3 of `route_partial` rasterises every pad),
    // then the net's own escape track over the same cell.
    g.set(cx, cy, 0, "IMU_AD0", Occ::Pad);
    g.set(cx, cy, 0, "IMU_AD0", Occ::Track);
    assert!(
        !g.passable(cx + 3, cy, 0, "3V3"),
        "a track of another net 3 cells (381 um) away leaves a 181 um gap: the cell holds 200 um-wide track copper, not just pad copper"
    );
    assert!(g.passable(cx + 4, cy, 0, "3V3"), "4 cells (508 um) is legal");
}

/// The pad must survive rip-up as an obstacle, but the inflation the
/// ripped track caused must not: otherwise the neighbourhood stays
/// over-blocked forever and nets that used to route stop routing.
#[test]
fn ripup_shrinks_the_cell_back_to_pad_extent() {
    let mut g = hub_grid();
    let (cx, cy) = (100i64, 100i64);
    g.set(cx, cy, 0, "IMU_AD0", Occ::Pad);
    g.set(cx, cy, 0, "IMU_AD0", Occ::Track);
    assert!(!g.passable(cx + 3, cy, 0, "3V3"));

    g.clear_net("IMU_AD0");
    // Pad copper is still there (clearance + 100 + 63 = 363 um -> 3 cells),
    // so 2 cells stays illegal and 3 is legal again.
    assert!(!g.passable(cx + 2, cy, 0, "3V3"), "the pad itself must survive rip-up as an obstacle");
    assert!(g.passable(cx + 3, cy, 0, "3V3"), "the ripped track's extra copper must not outlive the track");
}

/// A via written over a track cell of the same net (and the reverse order)
/// must always leave the cell claiming the wider via copper.
#[test]
fn via_and_track_on_one_cell_claim_via_clearance() {
    for order in [[Occ::Track, Occ::Via], [Occ::Via, Occ::Track]] {
        let mut g = hub_grid();
        let (cx, cy) = (100i64, 100i64);
        for k in order {
            g.set(cx, cy, 0, "IMU_AD0", k);
        }
        // clearance 200 + track half 100 + via half 300 = 600 um -> 5 cells.
        assert!(!g.passable(cx + 4, cy, 0, "3V3"), "order {order:?}: 4 cells is short of a track-to-via pair");
        assert!(g.passable(cx + 5, cy, 0, "3V3"), "order {order:?}: 5 cells is legal");
    }
}
