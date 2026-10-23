//! Tests for the net-collision overlay of a held selection (`sch_net_collision.rs`, `SCH_DRAG_NET_COLLISION_MONITOR`): where it marks two nets about to join and where it
//! marks a connection pulled apart, and where it says nothing.

use super::*;
use crate::sch_move::SchMoveCmd;
use crate::sch_net_collision::{CollisionMarker, DisconnectionMarker, NetCollision};
use eda_model::ir::{LabelKind, NetLabel, Provenance, SymbolInstance, Wire};
use std::collections::BTreeMap;

fn design(sch: SchematicSection) -> Design {
    Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: Some(sch),
        nets: None,
        routing: None,
        placement: None,
        drawings: None,
    }
}

fn p(x: Nm, y: Nm) -> Point {
    Point { x, y }
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn wire(id: &str, pts: &[Point]) -> Wire {
    Wire { id: id.into(), net: id.into(), pins: Vec::new(), pts: pts.to_vec(), bus: false }
}

fn label(id: &str, net: &str, at: Point) -> NetLabel {
    NetLabel { id: id.into(), net: net.into(), at, kind: LabelKind::Local }
}

fn section() -> SchematicSection {
    SchematicSection { imported_from_kicad: true, ..Default::default() }
}

/// The overlay a command leaves on a board with this sheet.
fn overlay(sch: SchematicSection, cmd: SchMoveCmd) -> NetCollision {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(sch), &m, 100, 300);
    b.apply(&Cmd::SchMove(cmd)).unwrap_or_else(|e| panic!("{e:?}"));
    b.net_collision().cloned().expect("a move leaves an overlay, empty or not")
}

fn mv(v: &[&str], dx: Nm, dy: Nm) -> SchMoveCmd {
    SchMoveCmd::Move { ids: ids(v), dx, dy, turns: Vec::new(), about: None }
}

fn drag(v: &[&str], dx: Nm, dy: Nm) -> SchMoveCmd {
    SchMoveCmd::Drag { ids: ids(v), vertices: BTreeMap::new(), dx, dy, ortho: true, grid: 0, turns: Vec::new(), about: None }
}

/// A horizontal wire and, 5.08 mm below it, the foot of a vertical one: lifted by 5.08 mm the vertical wire's end lands on the middle of the horizontal one.
fn tee() -> SchematicSection {
    let mut s = section();
    s.wires.push(wire("wire_h", &[p(0, 0), p(20_320, 0)]));
    s.wires.push(wire("wire_v", &[p(10_160, 15_240), p(10_160, 5_080)]));
    s
}

#[test]
fn a_wire_dragged_onto_another_net_is_marked_where_a_junction_would_join_them() {
    let o = overlay(tee(), mv(&["wire_v"], 0, -5_080));
    // 1.5 times the 36 mil junction, at the point where the foot of the vertical wire meets the middle of the horizontal one
    assert_eq!(o.collisions, vec![CollisionMarker { at: p(10_160, 0), radius_nm: 914.4 * 1.5 }]);
    assert!(o.disconnections.is_empty(), "{:?}", o.disconnections);
    assert!(!o.is_empty());
}

#[test]
fn the_same_move_short_of_the_other_wire_marks_nothing() {
    let o = overlay(tee(), mv(&["wire_v"], 0, -2_540));
    assert!(o.is_empty(), "{o:?}");
}

#[test]
fn nets_of_one_name_are_one_net_so_joining_them_is_no_collision() {
    // a label of the same name on each wire: they are one net
    let mut s = tee();
    s.labels.push(label("lbl_a", "SIG", p(0, 0)));
    s.labels.push(label("lbl_b", "SIG", p(10_160, 15_240)));
    let o = overlay(s, mv(&["wire_v"], 0, -5_080));
    assert!(o.collisions.is_empty(), "{o:?}");
    // with different names they are two
    let mut s = tee();
    s.labels.push(label("lbl_a", "SIG_A", p(0, 0)));
    s.labels.push(label("lbl_b", "SIG_B", p(10_160, 15_240)));
    let o = overlay(s, mv(&["wire_v"], 0, -5_080));
    assert_eq!(o.collisions.len(), 1, "{o:?}");
}

#[test]
fn wires_already_joined_elsewhere_are_one_net() {
    // a third wire ties the foot of the vertical one to the horizontal one's far end: one net already, so touching again is no short
    let mut s = tee();
    s.wires.push(wire("wire_tie", &[p(10_160, 15_240), p(20_320, 15_240), p(20_320, 0)]));
    let o = overlay(s, mv(&["wire_v"], 0, -5_080));
    assert!(o.collisions.is_empty(), "{o:?}");
}

#[test]
fn a_drag_is_marked_the_same_way_with_the_wires_it_stretches() {
    // dragging the vertical wire's end by its foot: the lines the drag changed are part of what is previewed
    let o = overlay(tee(), drag(&["wire_v"], 0, -5_080));
    assert_eq!(o.collisions.iter().map(|m| m.at).collect::<Vec<_>>(), vec![p(10_160, 0)], "{o:?}");
}

#[test]
fn two_wire_ends_that_meet_make_no_junction_and_so_no_mark_as_in_kicad() {
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(0, 0), p(10_160, 0)]));
    s.wires.push(wire("wire_b", &[p(20_320, 0), p(20_320, 10_160)]));
    // the second wire's end is carried onto the first one's: a plain end-to-end join, no dot needed, which is all `PreviewJunctions` looks at
    let o = overlay(s, mv(&["wire_b"], -10_160, 0));
    assert!(o.collisions.is_empty(), "{o:?}");
}

#[test]
fn a_wire_moved_away_from_the_one_it_ended_on_is_marked_as_a_connection_pulled_apart() {
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(0, 0), p(10_160, 0)]));
    s.wires.push(wire("wire_b", &[p(10_160, 0), p(10_160, 10_160)]));
    // Move leaves the wires where they are, so the joint comes apart: a circle at each end and the line between them
    let o = overlay(s.clone(), mv(&["wire_b"], 5_080, 0));
    assert_eq!(o.disconnections, vec![DisconnectionMarker { a: p(10_160, 0), b: p(15_240, 0), radius_nm: 152.4 }], "{o:?}");
    assert!(o.collisions.is_empty());
    // a Drag stretches the first wire to follow, so nothing comes apart
    let o = overlay(s.clone(), drag(&["wire_b"], 5_080, 0));
    assert!(o.disconnections.is_empty(), "{o:?}");
    // both wires carried together keep their joint: no mark
    let o = overlay(s.clone(), mv(&["wire_a", "wire_b"], 5_080, 0));
    assert!(o.disconnections.is_empty(), "{o:?}");
    // slid along its own line the second wire no longer reaches the first one's end, and that is a joint pulled apart too
    let o = overlay(s, mv(&["wire_b"], 0, 5_080));
    assert_eq!(o.disconnections.iter().map(|d| (d.a, d.b)).collect::<Vec<_>>(), vec![(p(10_160, 0), p(10_160, 5_080))], "{o:?}");
}

#[test]
fn a_symbol_moved_off_its_wires_is_marked_at_each_pin_it_left() {
    let mut s = section();
    s.symbols.push(SymbolInstance { id: "R1".into(), at: p(20_320, 25_400), rot: 0, mirrored: false, mirror_y: false, lib_id: "Device:R".into(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false });
    s.wires.push(wire("wire_top", &[p(20_320, 21_590), p(20_320, 12_700)]));
    s.wires.push(wire("wire_bottom", &[p(20_320, 29_210), p(20_320, 38_100)]));
    let o = overlay(s.clone(), mv(&["R1"], 10_160, 0));
    let marks: Vec<(Point, Point)> = o.disconnections.iter().map(|d| (d.a, d.b)).collect();
    assert_eq!(marks.len(), 2, "{o:?}");
    assert!(marks.contains(&(p(20_320, 21_590), p(30_480, 21_590))) || marks.contains(&(p(30_480, 21_590), p(20_320, 21_590))), "{marks:?}");
    // dragged, the wires follow and nothing is marked
    let o = overlay(s, drag(&["R1"], 10_160, 0));
    assert!(o.disconnections.is_empty(), "{o:?}");
}

#[test]
fn a_turn_or_a_mirror_leaves_no_overlay_to_draw() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(tee()), &m, 100, 300);
    assert!(b.net_collision().is_none(), "nothing has been applied");
    b.apply(&Cmd::SchMove(SchMoveCmd::Rotate { ids: ids(&["wire_v"]), vertices: BTreeMap::new(), ccw: true, about: None, grid: 0 })).unwrap();
    assert!(b.net_collision().is_some_and(NetCollision::is_empty));
}
