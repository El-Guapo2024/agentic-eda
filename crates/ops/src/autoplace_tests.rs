//! Place > Auto-Place Footprints (`autoplace.rs`): which footprints each of KiCad's two actions takes (`AUTOPLACE_TOOL::autoplaceSelected`,
//! `autoplaceOffboard`), that every other footprint stays where it is, that a locked one is left out unless Override locks is on, the board-edge refusal in
//! KiCad's words, and the placement itself: clear of the footprints that stay, inside the board, on the grid asked for.

use super::autoplace::*;
use super::{Board, Cmd};
use eda_model::ir::{FootprintInstance, Point, Side};
use eda_model::units::{mm, Nm};
use eda_model::{CheckStatus, ConstraintModel, Net, Part, Pin, PinKind};
use std::collections::BTreeMap;

fn part(r: &str, pkg: &str, pins: usize) -> Part {
    Part {
        reference: r.into(),
        mpn: None,
        lcsc: None,
        value: None,
        package: Some(pkg.into()),
        footprint: None,
        pins: (1..=pins).map(|i| Pin { number: i.to_string(), name: None, kind: PinKind::Passive }).collect(),
        body_nm: None,
        symbol: None,
        datasheet: None,
        edge: None,
    }
}

/// An SOIC with four resistors and a capacitor on its pins.
fn model() -> ConstraintModel {
    let mut parts = vec![part("U1", "SOIC-8", 8), part("C1", "0603", 2)];
    let mut nets = vec![Net { name: "VCC".into(), pins: vec!["U1.8".into(), "C1.1".into()] }, Net { name: "GND".into(), pins: vec!["U1.4".into(), "C1.2".into()] }];
    for i in 1..=4 {
        parts.push(part(&format!("R{i}"), "0603", 2));
        nets.push(Net { name: format!("S{i}"), pins: vec![format!("R{i}.1"), format!("U1.{i}")] });
        nets[0].pins.push(format!("R{i}.2"));
    }
    ConstraintModel { parts, nets, ..Default::default() }
}

fn board(m: &ConstraintModel) -> Board<'_> {
    Board::new(crate::tests::empty_design(), m, mm(0.1), mm(0.3))
}

/// Each footprint's pose: position, turn and side.
type Pose = (Point, u32, Side);

fn poses(b: &Board) -> BTreeMap<String, Pose> {
    b.design().placement.as_ref().unwrap().footprints.iter().map(|f: &FootprintInstance| (f.id.clone(), (f.at, f.rot, f.side))).collect()
}

fn at(x: f64, y: f64) -> Point {
    Point { x: mm(x), y: mm(y) }
}

/// The board with every part down: U1 in the middle, the others in a row under it.
fn placed(m: &ConstraintModel) -> Board<'_> {
    let mut b = board(m);
    b.apply(&Cmd::PlaceAt { part: "U1".into(), x: mm(50.0), y: mm(50.0) }).unwrap();
    for (i, r) in ["R1", "R2", "R3", "R4", "C1"].iter().enumerate() {
        b.apply(&Cmd::PlaceAt { part: (*r).into(), x: mm(20.0 + 6.0 * i as f64), y: mm(80.0) }).unwrap();
    }
    b
}

fn overlap_failures(b: &Board) -> Vec<String> {
    b.checks().into_iter().filter(|c| c.status == CheckStatus::Fail && (c.check == "placement_courtyard_overlap" || c.check == "placement_within_outline")).map(|c| format!("{} @ {:?}", c.check, c.location)).collect()
}

#[test]
fn place_selected_moves_only_the_selected_footprints_on_the_grid_asked_for() {
    let m = model();
    let mut b = placed(&m);
    let before = poses(&b);
    b.apply(&Cmd::Autoplace(AutoplaceCmd { footprints: vec!["R1".into(), "R2".into(), "R1".into()], grid_nm: Some(mm(0.05)), ..Default::default() })).unwrap();
    let after = poses(&b);
    for r in ["U1", "R3", "R4", "C1"] {
        assert_eq!(after[r], before[r], "{r} stays where it was");
    }
    for r in ["R1", "R2"] {
        assert_eq!(after[r].0.x.get() % 50_000, 0, "{r} on the 0.05 mm grid: {:?}", after[r].0);
        assert_eq!(after[r].0.y.get() % 50_000, 0, "{r} on the 0.05 mm grid: {:?}", after[r].0);
        assert_eq!(after[r].2, Side::Top);
    }
    assert_ne!((after["R1"].0, after["R2"].0), (before["R1"].0, before["R2"].0), "the placer moved them");
    assert!(overlap_failures(&b).is_empty(), "{:?}", overlap_failures(&b));
}

#[test]
fn place_off_board_takes_the_footprints_off_the_board_and_the_parts_not_on_it() {
    let m = model();
    let mut b = board(&m);
    b.apply(&Cmd::PlaceAt { part: "U1".into(), x: mm(50.0), y: mm(50.0) }).unwrap();
    b.apply(&Cmd::PlaceAt { part: "R1".into(), x: mm(30.0), y: mm(30.0) }).unwrap();
    b.apply(&Cmd::PlaceAt { part: "R2".into(), x: mm(70.0), y: mm(70.0) }).unwrap();
    // R2 is dragged off the board's edge; R3, R4 and C1 are not on the board at all (KiCad's Update PCB stacks such footprints beside the board)
    b.apply(&Cmd::MoveItems { ids: vec!["R2".into()], dx: mm(40.0), dy: Nm::ZERO }).unwrap();
    let before = poses(&b);
    assert!(!before.contains_key("R3"));
    b.apply(&Cmd::Autoplace(AutoplaceCmd { offboard: true, ..Default::default() })).unwrap();
    let after = poses(&b);
    assert_eq!(after["U1"], before["U1"]);
    assert_eq!(after["R1"], before["R1"], "a footprint on the board is not off-board");
    for r in ["R2", "R3", "R4", "C1"] {
        let p = after.get(r).unwrap_or_else(|| panic!("{r} placed")).0;
        assert!(p.x > Nm::ZERO && p.x < mm(100.0) && p.y > Nm::ZERO && p.y < mm(100.0), "{r} on the board: {p:?}");
        assert_eq!(p.x.get() % 100_000, 0, "{r} on the board's placement step without a grid given");
    }
    assert!(overlap_failures(&b).is_empty(), "{:?}", overlap_failures(&b));
    // everything is on the board now: nothing left to place
    let e = b.apply(&Cmd::Autoplace(AutoplaceCmd { offboard: true, ..Default::default() })).unwrap_err();
    assert_eq!(e[0].check, "ops_autoplace_nothing");
}

#[test]
fn a_locked_footprint_is_left_out_unless_override_locks_is_on() {
    let m = model();
    let mut b = placed(&m);
    b.apply(&Cmd::SetLocked { ids: vec!["R1".into()], locked: true }).unwrap();
    let before = poses(&b);
    let e = b.apply(&Cmd::Autoplace(AutoplaceCmd { footprints: vec!["R1".into()], ..Default::default() })).unwrap_err();
    assert_eq!(e[0].check, "ops_autoplace_nothing", "`if( !frame()->GetOverrideLocks() )` drops it, and nothing is left");
    // with R2 as well only R2 moves
    b.apply(&Cmd::Autoplace(AutoplaceCmd { footprints: vec!["R1".into(), "R2".into()], ..Default::default() })).unwrap();
    assert_eq!(poses(&b)["R1"], before["R1"]);
    // Override locks places it
    b.apply(&Cmd::Autoplace(AutoplaceCmd { footprints: vec!["R1".into()], override_locks: true, ..Default::default() })).unwrap();
    assert_ne!(poses(&b)["R1"].0, before["R1"].0);
}

#[test]
fn a_footprint_on_the_back_is_placed_on_the_back_and_one_that_stays_turned_is_an_obstacle() {
    let m = model();
    let mut b = placed(&m);
    b.apply(&Cmd::Flip { part: "R3".into() }).unwrap();
    // U1 stays, turned 45 degrees: its turned courtyard is what the others keep clear of
    b.apply(&Cmd::RotateItems { ids: vec!["U1".into()], pivot: at(50.0, 50.0), angle_millideg: 45_000 }).unwrap();
    let u1 = poses(&b)["U1"].clone();
    b.apply(&Cmd::Autoplace(AutoplaceCmd { footprints: vec!["R1".into(), "R2".into(), "R3".into(), "R4".into(), "C1".into()], ..Default::default() })).unwrap();
    let after = poses(&b);
    assert_eq!(after["R3"].2, Side::Bottom, "R3 keeps its side");
    assert_eq!(after["U1"], u1, "U1 stays, turned");
    assert!(overlap_failures(&b).is_empty(), "{:?}", overlap_failures(&b));
}

#[test]
fn a_board_without_edges_is_refused_in_kicads_words() {
    let m = model();
    let mut design = crate::tests::empty_design();
    design.placement.as_mut().unwrap().outline.clear();
    let mut b = Board::new(design, &m, mm(0.1), mm(0.3));
    let e = b.apply(&Cmd::Autoplace(AutoplaceCmd { offboard: true, ..Default::default() })).unwrap_err();
    assert_eq!(e[0].check, "ops_autoplace_no_board");
    assert_eq!(e[0].hint.as_deref(), Some(NO_BOARD_EDGES));
    assert_eq!(NO_BOARD_EDGES, "Board edges must be defined on the Edge.Cuts layer.");
}

#[test]
fn the_verb_on_the_wire_and_in_the_log() {
    let c: Cmd = serde_json::from_str(r#"{"op":"autoplace","footprints":["R1","R2"],"grid_nm":50000}"#).unwrap();
    let Cmd::Autoplace(a) = &c else { panic!("{c:?}") };
    assert_eq!((a.footprints.clone(), a.grid_nm, a.offboard, a.override_locks), (vec!["R1".to_string(), "R2".to_string()], Some(mm(0.05)), false, false));
    assert_eq!(a.describe(), "autoplace --selected R1,R2 --grid 0.0500");
    let off: Cmd = serde_json::from_str(r#"{"op":"autoplace","offboard":true}"#).unwrap();
    assert!(off.always_judged() && !off.clears_routing_in(&board(&model())), "always judged, never clears the routing");
    assert_eq!(serde_json::to_value(&off).unwrap(), serde_json::json!({"op": "autoplace", "offboard": true}));
}
