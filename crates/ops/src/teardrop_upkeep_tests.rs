//! UI-LEFT-2 PP6: the teardrop update every board edit runs (`teardrop_upkeep.rs`, KiCad 25ec2335 `BOARD_COMMIT::Push`).

use super::pcb_transform_tests::{board, ids, model, track, via};
use super::*;
use eda_model::ir::TeardropSettings;
use eda_model::units::mm;

/// A board with a via and a track leaving it (GND), Board Setup's teardrops on, and its teardrop made (`Cmd::AddAllTeardrops`).
fn with_teardrop(b: &mut Board<'_>) {
    let rt = b.routing_mut();
    rt.tracks.clear();
    rt.vias.clear();
    rt.vias.push(Via { diameter: mm(0.8), ..via("v1", (mm(10.0), mm(10.0)), "F.Cu", "B.Cu") });
    rt.tracks.push(track("t1", "F.Cu", &[(mm(10.0), mm(10.0)), (mm(15.0), mm(10.0))]));
    b.apply(&Cmd::SetTeardropSettings { settings: TeardropSettings { enabled: true, ..Default::default() } }).unwrap();
    b.apply(&Cmd::AddAllTeardrops).unwrap();
}

fn teardrops(b: &Board<'_>) -> Vec<Zone> {
    b.design().routing.as_ref().unwrap().zones.iter().filter(|z| z.teardrop).cloned().collect()
}

/// A move of the via and its track takes the teardrop along in the same command (one undo step), and a batch of moves updates it once, at its end; Board Setup's
/// page and Edit Teardrops' own commands do not run the update.
#[test]
fn moving_a_track_and_its_via_takes_the_teardrop_along_in_the_same_command() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    with_teardrop(&mut b);
    let before = teardrops(&b);
    assert_eq!(before.len(), 1);
    b.apply(&Cmd::MoveItems { ids: ids(&["t1", "v1"]), dx: mm(2.0), dy: mm(1.0) }).unwrap();
    let after = teardrops(&b);
    assert_eq!(after.len(), 1, "{after:?}");
    let moved: Vec<Point> = before[0].outline.iter().map(|q| Point { x: q.x + mm(2.0), y: q.y + mm(1.0) }).collect();
    assert_eq!(after[0].outline, moved, "the teardrop follows the via and the track");
    assert!(!after[0].id.is_empty(), "the new teardrop has an id");
    // A batch is one commit.
    b.apply(&Cmd::Batch { cmds: vec![Cmd::MoveItems { ids: ids(&["t1"]), dx: Nm::ZERO, dy: mm(0.5) }, Cmd::MoveItems { ids: ids(&["v1"]), dx: Nm::ZERO, dy: mm(0.5) }] }).unwrap();
    let batched = teardrops(&b);
    assert_eq!(batched.len(), 1);
    assert_eq!(batched[0].outline, moved.iter().map(|q| Point { x: q.x, y: q.y + mm(0.5) }).collect::<Vec<_>>());
    // Moving the track alone off its via: no teardrop is left.
    b.apply(&Cmd::MoveItems { ids: ids(&["t1"]), dx: mm(5.0), dy: Nm::ZERO }).unwrap();
    assert!(teardrops(&b).is_empty(), "{:?}", teardrops(&b));
    // Board Setup's page does not touch them (in KiCad it changes no item's parameters).
    b.apply(&Cmd::MoveItems { ids: ids(&["t1"]), dx: mm(-5.0), dy: Nm::ZERO }).unwrap();
    assert_eq!(teardrops(&b).len(), 1);
    b.apply(&Cmd::SetTeardropSettings { settings: TeardropSettings { enabled: true, best_length_ratio: 1.0, ..Default::default() } }).unwrap();
    assert_eq!(teardrops(&b)[0].outline, batched[0].outline, "Board Setup's page rebuilt nothing");
}

/// A via's own parameters set in Track & Via Properties make its teardrop at once (`Cmd::SetTeardropParams` is a commit of the via), and a board with no teardrop
/// and nothing that could have one is not looked at.
#[test]
fn turning_a_vias_teardrops_on_makes_its_teardrop() {
    use eda_model::teardrop_params::TeardropParams;
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let rt = b.routing_mut();
    rt.tracks.clear();
    rt.vias.clear();
    rt.vias.push(Via { diameter: mm(0.8), ..via("v1", (mm(10.0), mm(10.0)), "F.Cu", "B.Cu") });
    rt.tracks.push(track("t1", "F.Cu", &[(mm(10.0), mm(10.0)), (mm(15.0), mm(10.0))]));
    assert!(eda_connectivity::teardrop::snapshot(b.design(), &m).is_none(), "nothing on the board can have a teardrop");
    b.apply(&Cmd::SetTeardropParams { ids: ids(&["v1"]), params: Some(TeardropParams { enabled: true, ..Default::default() }) }).unwrap();
    assert_eq!(teardrops(&b).len(), 1);
    b.apply(&Cmd::SetTeardropParams { ids: ids(&["v1"]), params: Some(TeardropParams::default()) }).unwrap();
    assert!(teardrops(&b).is_empty(), "turned off, the via's teardrop goes");
}
