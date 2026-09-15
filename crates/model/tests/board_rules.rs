//! A board that cannot exist must be rejected at the door.
//!
//! Each of these used to be absorbed downstream -- `grid_um.max(1)`,
//! `layers.first().unwrap_or("F.Cu")` -- which meant the reader picked a
//! number nobody wrote down and every gate afterwards measured the board
//! against that invention and reported the answer as fact.

use eda_model::ir::Point;
use eda_model::{BoardRules, CheckStatus, Pour};

fn failing_fields(b: &BoardRules) -> Vec<String> {
    b.validate()
        .into_iter()
        .filter(|c| c.status == CheckStatus::Fail)
        .map(|c| c.location.unwrap_or_default())
        .collect()
}

#[test]
fn the_default_board_is_valid() {
    assert!(BoardRules::default().validate().is_empty());
}

#[test]
fn a_zero_grid_is_rejected() {
    // It is the divisor for every cell index on the board.
    let mut b = BoardRules::default();
    b.grid = 0;
    assert!(failing_fields(&b).contains(&"board.grid".to_string()));
}

#[test]
fn an_empty_stackup_is_rejected() {
    let mut b = BoardRules::default();
    b.layers.clear();
    assert!(failing_fields(&b).contains(&"board.layers".to_string()));
}

#[test]
fn a_repeated_copper_layer_is_rejected() {
    // Layer order is the stackup; a name twice makes "outer" ambiguous.
    let mut b = BoardRules::default();
    b.layers = vec!["F.Cu".into(), "B.Cu".into(), "F.Cu".into()];
    assert!(failing_fields(&b).contains(&"board.layers".to_string()));
}

#[test]
fn a_via_with_no_annular_ring_is_rejected() {
    // Drill at least as wide as the pad leaves nothing for the plating to
    // land on -- a hole where a connection was supposed to be.
    let mut b = BoardRules::default();
    b.via_drill = b.via_diameter;
    assert!(failing_fields(&b).contains(&"board.via_drill".to_string()));
}

#[test]
fn a_two_point_outline_is_rejected() {
    let mut b = BoardRules::default();
    b.outline = Some(vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }]);
    assert!(failing_fields(&b).contains(&"board.outline".to_string()));
}

#[test]
fn a_pour_on_a_layer_outside_the_stackup_is_rejected() {
    let mut b = BoardRules::default();
    b.pours = vec![Pour { net: "GND".into(), layer: "In7.Cu".into() }];
    assert!(failing_fields(&b).contains(&"board.pours".to_string()));
}

#[test]
fn every_problem_is_reported_not_just_the_first() {
    // An agent fixing one value per run learns nothing a single list would
    // not have told it in one.
    let mut b = BoardRules::default();
    b.grid = 0;
    b.track_width = 0;
    b.layers.clear();
    let f = failing_fields(&b);
    assert!(f.len() >= 3, "expected every bad field, got {f:?}");
}

#[test]
fn the_default_tuning_is_valid() {
    // Every check below has to be a statement about a *bad* tuning. If the
    // shipped defaults tripped one, the check would be describing normal
    // operation and no board would load at all.
    assert!(failing_fields(&BoardRules::default()).is_empty());
}

#[test]
fn a_free_via_is_rejected() {
    // At zero cost the router sprinkles vias instead of routing.
    let mut b = BoardRules::default();
    b.tuning.via_cost_cells = 0;
    assert!(failing_fields(&b).contains(&"board.tuning.via_cost_cells".to_string()));
}

#[test]
fn a_present_cost_that_never_grows_is_rejected() {
    // PathFinder converges *because* sharing a cell gets dearer each pass.
    // At a multiplier of 1.0 the negotiation is an infinite loop that the
    // wall budget happens to interrupt.
    let mut b = BoardRules::default();
    b.tuning.nc_pres_fac_mult = 1.0;
    assert!(failing_fields(&b).contains(&"board.tuning.nc_pres_fac_mult".to_string()));
}

#[test]
fn a_zero_iteration_cap_is_rejected() {
    let mut b = BoardRules::default();
    b.tuning.nc_max_iters = 0;
    assert!(failing_fields(&b).contains(&"board.tuning.nc_max_iters".to_string()));
}

#[test]
fn a_zero_expansion_cap_is_rejected() {
    // Every A* search would give up before its first step, and the router
    // would report the board unroutable.
    let mut b = BoardRules::default();
    b.tuning.seq_max_expansions = 0;
    assert!(failing_fields(&b).contains(&"board.tuning.seq_max_expansions".to_string()));
}

#[test]
fn a_router_that_does_not_exist_is_rejected() {
    let mut b = BoardRules::default();
    b.tuning.router = "astar".into();
    assert!(failing_fields(&b).contains(&"board.tuning.router".to_string()));
}

#[test]
fn a_stitch_reach_of_zero_is_rejected() {
    // No via site is ever legal, so every poured pad reports unreachable.
    let mut b = BoardRules::default();
    b.tuning.pour_stitch_reach_um = 0;
    assert!(failing_fields(&b).contains(&"board.tuning.pour_stitch_reach_um".to_string()));
}
