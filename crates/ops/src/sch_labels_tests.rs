//! Tests for auto-rotate on placement and for the justification of a free text (`sch_labels.rs`, the text part of `sch_move.rs` and `sch_props.rs`):
//! `SCH_EDIT_FRAME::AutoRotateItem`, `SCH_SCREEN::GetLabelOrientationForPoint`, `GetPinSpinStyle`, and `SCH_TEXT::Rotate90` / `MirrorSpinStyle`.
//!
//! The sheet is a KiCad-style one (`imported_from_kicad`: a symbol sits at its library origin): `Device:R` R1 at (20.32, 25.4) mm has pin 1 at (20.32, 21.59)
//! and pin 2 at (20.32, 29.21), the top pin pointing down towards the body.

use super::*;
use crate::sch_edit::SchCmd;
use crate::sch_move::SchMoveCmd;
use eda_model::ir::{BusEntry, LabelShape, NetLabel, PowerSymbol, Provenance, SchLine, SchematicText, SymbolInstance, TextJustify, TextVAlign, Wire};
use eda_model::sch_extras::{LabelLook, LabelSpin, SchTextJustify};
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

fn section() -> SchematicSection {
    SchematicSection { imported_from_kicad: true, ..Default::default() }
}

fn wire(id: &str, pts: &[Point]) -> Wire {
    Wire { id: id.into(), net: "N".into(), pins: Vec::new(), pts: pts.to_vec(), bus: false }
}

fn bus(id: &str, pts: &[Point]) -> Wire {
    Wire { id: id.into(), net: String::new(), pins: Vec::new(), pts: pts.to_vec(), bus: true }
}

fn res(id: &str, at: Point, rot: u32, mirrored: bool, mirror_y: bool) -> SymbolInstance {
    SymbolInstance { id: id.into(), at, rot, mirrored, mirror_y, lib_id: "Device:R".into(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false }
}

fn global(id: &str, at: Point) -> NetLabel {
    NetLabel { id: id.into(), net: "G".into(), at, kind: LabelKind::Global { shape: LabelShape::Input } }
}

fn local(id: &str, at: Point) -> NetLabel {
    NetLabel { id: id.into(), net: "L".into(), at, kind: LabelKind::Local }
}

fn board(sch: SchematicSection, m: &ConstraintModel) -> Board<'_> {
    Board::new(design(sch), m, 100_000, 300_000)
}

fn sch_of<'b>(b: &'b Board) -> &'b SchematicSection {
    b.design().schematic.as_ref().unwrap()
}

fn err(b: &mut Board, cmd: SchCmd) -> String {
    b.apply(&Cmd::SchEdit(cmd)).unwrap_err()[0].check.clone()
}

fn ok(b: &mut Board, cmd: SchCmd) {
    b.apply(&Cmd::SchEdit(cmd)).unwrap_or_else(|e| panic!("{e:?}"));
}

fn place(net: &str, at: Point, kind: LabelKind, auto: Option<bool>, spin: Option<LabelSpin>) -> SchCmd {
    SchCmd::PlaceLabel { net: net.into(), at, kind, spin, size_nm: None, bold: None, italic: None, auto_rotate: auto }
}

fn glob(shape: LabelShape) -> LabelKind {
    LabelKind::Global { shape }
}

/// The spin the scene gives a label at `at` that has `default`.
fn spin_at(sch: &SchematicSection, at: Point, default: LabelSpin) -> LabelSpin {
    let m = ConstraintModel::default();
    crate::sch_scene::Scene::new(sch, &m).label_orientation_for_point(at, default)
}

// ----------------------------------------------------------------------------------------------------------------- GetLabelOrientationForPoint

#[test]
fn a_label_at_the_end_of_a_wire_reads_on_along_it_and_at_the_start_the_other_way() {
    let mut s = section();
    s.wires.push(wire("wire_h", &[p(0, 0), p(10_160_000, 0)]));
    s.wires.push(wire("wire_v", &[p(30_480_000, 0), p(30_480_000, 10_160_000)]));
    // a horizontal wire drawn left to right: the end continues it to the right, the start to the left
    assert_eq!(spin_at(&s, p(10_160_000, 0), LabelSpin::Up), LabelSpin::Right);
    assert_eq!(spin_at(&s, p(0, 0), LabelSpin::Up), LabelSpin::Left);
    // a vertical one drawn downwards: the end hangs below it, the start goes up
    assert_eq!(spin_at(&s, p(30_480_000, 10_160_000), LabelSpin::Up), LabelSpin::Bottom);
    assert_eq!(spin_at(&s, p(30_480_000, 0), LabelSpin::Right), LabelSpin::Up);
    // the same wires drawn the other way give the same pictures: it is the end the label is at that counts
    let mut r = section();
    r.wires.push(wire("wire_h", &[p(10_160_000, 0), p(0, 0)]));
    r.wires.push(wire("wire_v", &[p(30_480_000, 10_160_000), p(30_480_000, 0)]));
    assert_eq!(spin_at(&r, p(10_160_000, 0), LabelSpin::Up), LabelSpin::Right);
    assert_eq!(spin_at(&r, p(0, 0), LabelSpin::Up), LabelSpin::Left);
    assert_eq!(spin_at(&r, p(30_480_000, 10_160_000), LabelSpin::Up), LabelSpin::Bottom);
    assert_eq!(spin_at(&r, p(30_480_000, 0), LabelSpin::Right), LabelSpin::Up);
}

#[test]
fn a_label_in_the_middle_of_a_wire_gets_what_the_c_code_gives_it() {
    let mut s = section();
    s.wires.push(wire("wire_h", &[p(0, 0), p(10_160_000, 0)]));
    // "ret = line->GetEndPoint() == aPosition ? RIGHT : LEFT": not the end, so LEFT
    assert_eq!(spin_at(&s, p(5_080_000, 0), LabelSpin::Up), LabelSpin::Left);
}

#[test]
fn a_label_nothing_overlaps_keeps_the_spin_it_has() {
    let mut s = section();
    s.wires.push(wire("wire_h", &[p(0, 0), p(10_160_000, 0)]));
    assert_eq!(spin_at(&s, p(50_800_000, 50_800_000), LabelSpin::Bottom), LabelSpin::Bottom);
    assert_eq!(spin_at(&s, p(50_800_000, 50_800_000), LabelSpin::Left), LabelSpin::Left);
}

#[test]
fn a_diagonal_line_counts_as_horizontal_up_to_45_degrees_and_as_vertical_beyond() {
    let mut s = section();
    // exactly 45 degrees down-right: "-45 < angle <= 45" is horizontal
    s.lines.push(SchLine { id: "line_a".into(), pts: vec![p(0, 0), p(5_080_000, 5_080_000)], width_nm: 0 });
    // steeper: vertical
    s.lines.push(SchLine { id: "line_b".into(), pts: vec![p(20_320_000, 0), p(22_860_000, 5_080_000)], width_nm: 0 });
    // exactly 45 degrees up-right is -45, which is not above -45: vertical
    s.lines.push(SchLine { id: "line_c".into(), pts: vec![p(40_640_000, 5_080_000), p(45_720_000, 0)], width_nm: 0 });
    assert_eq!(spin_at(&s, p(5_080_000, 5_080_000), LabelSpin::Up), LabelSpin::Right);
    assert_eq!(spin_at(&s, p(22_860_000, 5_080_000), LabelSpin::Up), LabelSpin::Bottom);
    assert_eq!(spin_at(&s, p(45_720_000, 0), LabelSpin::Up), LabelSpin::Up, "ends at the top of an upward line");
}

/// `Items().Overlapping( aPosition )` asks the R-tree of bounding boxes: a point inside a diagonal line's box counts though it is not on the line, and so does a
/// point beside a wire by less than half its pen; one further off does not.
#[test]
fn a_line_counts_wherever_its_bounding_box_holds_the_point() {
    let mut s = section();
    s.lines.push(SchLine { id: "line_d".into(), pts: vec![p(0, 0), p(10_000_000, 10_000_000)], width_nm: 0 });
    // 45 degrees: horizontal; not the end: LEFT
    assert_eq!(spin_at(&s, p(5_000_000, 2_000_000), LabelSpin::Right), LabelSpin::Left);
    let mut w = section();
    w.wires.push(wire("wire_h", &[p(0, 0), p(10_160_000, 0)]));
    assert_eq!(spin_at(&w, p(5_080_000, 70_000), LabelSpin::Up), LabelSpin::Left, "within half of the wire's 6 mil");
    assert_eq!(spin_at(&w, p(5_080_000, 200_000), LabelSpin::Up), LabelSpin::Up, "beyond it");
}

/// The way a pin points away from the other pin of its resistor: the spin `GetPinSpinStyle` asks for, whatever the symbol's orientation.
#[test]
fn a_label_on_a_pin_reads_away_from_the_symbol_in_every_orientation() {
    let m = ConstraintModel::default();
    for rot in [0u32, 90_000, 180_000, 270_000] {
        for (mirrored, mirror_y) in [(false, false), (true, false), (false, true)] {
            let mut s = section();
            s.symbols.push(res("R1", p(40_000_000, 40_000_000), rot, mirrored, mirror_y));
            let scene = crate::sch_scene::Scene::new(&s, &m);
            let pins = scene.pins_of(0);
            let (a, b) = (pins[0].1, pins[1].1);
            let away = |from: Point, to: Point| -> LabelSpin {
                let (dx, dy) = (to.x - from.x, to.y - from.y);
                if dx.abs() > dy.abs() {
                    if dx > 0 { LabelSpin::Right } else { LabelSpin::Left }
                } else if dy > 0 {
                    LabelSpin::Bottom
                } else {
                    LabelSpin::Up
                }
            };
            // a pin's label goes the way from the other pin to it
            assert_eq!(scene.label_orientation_for_point(a, LabelSpin::Right), away(b, a), "rot {rot} mirrored {mirrored} mirror_y {mirror_y} pin 1");
            assert_eq!(scene.label_orientation_for_point(b, LabelSpin::Right), away(a, b), "rot {rot} mirrored {mirrored} mirror_y {mirror_y} pin 2");
        }
    }
}

#[test]
fn the_top_pin_of_an_unturned_resistor_asks_for_up_and_the_bottom_pin_for_bottom() {
    let mut s = section();
    s.symbols.push(res("R1", p(20_320_000, 25_400_000), 0, false, false));
    assert_eq!(spin_at(&s, p(20_320_000, 21_590_000), LabelSpin::Right), LabelSpin::Up);
    assert_eq!(spin_at(&s, p(20_320_000, 29_210_000), LabelSpin::Right), LabelSpin::Bottom);
}

#[test]
fn a_pin_wins_over_a_wire_that_ends_on_it_because_symbols_come_after_lines() {
    let mut s = section();
    s.symbols.push(res("R1", p(20_320_000, 25_400_000), 0, false, false));
    // a wire that comes into the top pin from the left would ask for Right; the pin asks for Up and is later in the walk
    s.wires.push(wire("wire_a", &[p(10_160_000, 21_590_000), p(20_320_000, 21_590_000)]));
    assert_eq!(spin_at(&s, p(20_320_000, 21_590_000), LabelSpin::Left), LabelSpin::Up);
}

#[test]
fn a_power_symbol_asks_for_the_spin_of_its_pin_too() {
    let mut s = section();
    // power:GND hangs down from its pin, which is at the symbol's origin and points down into the body
    s.power_symbols.push(PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: p(20_320_000, 20_320_000), rot: 0, net: "GND".into(), pin: String::new() });
    assert_eq!(spin_at(&s, p(20_320_000, 20_320_000), LabelSpin::Right), LabelSpin::Up);
}

#[test]
fn a_label_at_a_bus_entry_goes_to_the_side_the_entry_is_on() {
    let mut s = section();
    // a vertical bus drawn downwards, an entry off it to the lower right
    s.wires.push(bus("bus_a", &[p(0, 0), p(0, 20_320_000)]));
    s.bus_entries.push(BusEntry { id: "be_a".into(), at: p(0, 5_080_000), size: p(2_540_000, 2_540_000) });
    assert_eq!(spin_at(&s, p(2_540_000, 7_620_000), LabelSpin::Up), LabelSpin::Right);
    // an entry off it to the lower left
    s.bus_entries.push(BusEntry { id: "be_b".into(), at: p(0, 12_700_000), size: p(-2_540_000, 2_540_000) });
    assert_eq!(spin_at(&s, p(-2_540_000, 15_240_000), LabelSpin::Up), LabelSpin::Left);
    // a horizontal bus drawn to the right: an entry above it asks for Up, below it for Bottom
    let mut h = section();
    h.wires.push(bus("bus_h", &[p(0, 0), p(20_320_000, 0)]));
    h.bus_entries.push(BusEntry { id: "be_c".into(), at: p(5_080_000, 0), size: p(2_540_000, -2_540_000) });
    h.bus_entries.push(BusEntry { id: "be_d".into(), at: p(12_700_000, 0), size: p(2_540_000, 2_540_000) });
    assert_eq!(spin_at(&h, p(7_620_000, -2_540_000), LabelSpin::Right), LabelSpin::Up);
    assert_eq!(spin_at(&h, p(15_240_000, 2_540_000), LabelSpin::Right), LabelSpin::Bottom);
    // "bus->Angle().AsDegrees() == 90.0": a vertical bus drawn upwards is -90 and changes nothing
    let mut up = section();
    up.wires.push(bus("bus_u", &[p(0, 20_320_000), p(0, 0)]));
    up.bus_entries.push(BusEntry { id: "be_e".into(), at: p(0, 5_080_000), size: p(2_540_000, 2_540_000) });
    assert_eq!(spin_at(&up, p(2_540_000, 7_620_000), LabelSpin::Up), LabelSpin::Up);
}

// -------------------------------------------------------------------------------------------------------------------------------- Place Label

#[test]
fn place_label_with_auto_takes_the_spin_of_the_wire_it_is_put_on() {
    let m = ConstraintModel::default();
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(0, 0), p(10_160_000, 0)]));
    let mut b = board(s, &m);
    // the dialog's spin is Up, but "Auto" is on: the wire decides
    ok(&mut b, place("CLK", p(10_160_000, 0), glob(LabelShape::Output), Some(true), Some(LabelSpin::Up)));
    let sch = sch_of(&b);
    let l = &sch.labels[0];
    assert!(!l.id.is_empty() && l.net == "CLK" && l.at == p(10_160_000, 0));
    assert_eq!(sch.extras.label_spins.get(&l.id), Some(&LabelSpin::Right));
    assert!(sch.extras.label_looks[&l.id].auto_rotate);
}

#[test]
fn place_label_without_auto_keeps_the_spin_the_dialog_gave() {
    let m = ConstraintModel::default();
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(0, 0), p(10_160_000, 0)]));
    let mut b = board(s, &m);
    ok(&mut b, place("CLK", p(10_160_000, 0), glob(LabelShape::Output), Some(false), Some(LabelSpin::Up)));
    let sch = sch_of(&b);
    let id = sch.labels[0].id.clone();
    assert_eq!(sch.extras.label_spins.get(&id), Some(&LabelSpin::Up));
    assert!(!sch.extras.label_looks.contains_key(&id), "the flag is off: nothing to keep");
    // with no spin given the label is RIGHT (`m_lastTextOrientation( SPIN_STYLE::RIGHT )`), not read off the wire it starts (which would be LEFT), and keeps it
    ok(&mut b, place("D0", p(0, 0), LabelKind::Local, None, None));
    let sch = sch_of(&b);
    let id = sch.labels[1].id.clone();
    assert_eq!(sch.extras.label_spins.get(&id), Some(&LabelSpin::Right));
    assert!(!sch.extras.label_looks.contains_key(&id));
}

/// A KiCad label never changes its spin unless it is a global or hierarchical label with Auto on: a wire that ends at it, moved to run the other way,
/// leaves it as it was put down.
#[test]
fn a_placed_label_keeps_its_spin_when_the_wire_it_ends_is_turned_round() {
    let m = ConstraintModel::default();
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(0, 0), p(10_160_000, 0)]));
    let mut b = board(s, &m);
    ok(&mut b, place("D0", p(10_160_000, 0), LabelKind::Local, None, None));
    let id = sch_of(&b).labels[0].id.clone();
    let mut sch = sch_of(&b).clone();
    sch.wires[0].pts = vec![p(20_320_000, 0), p(10_160_000, 0)];
    let scene = crate::sch_scene::Scene::new(&sch, &m);
    assert_eq!(crate::sch_move::effective_spin(&scene, 0), LabelSpin::Right, "{id}");
}

#[test]
fn place_label_with_auto_on_a_pin_reads_away_from_the_symbol() {
    let m = ConstraintModel::default();
    let mut s = section();
    s.symbols.push(res("R1", p(20_320_000, 25_400_000), 0, false, false));
    let mut b = board(s, &m);
    ok(&mut b, place("TOP", p(20_320_000, 21_590_000), LabelKind::Hierarchical { shape: LabelShape::Input }, Some(true), None));
    ok(&mut b, place("BOT", p(20_320_000, 29_210_000), glob(LabelShape::Input), Some(true), None));
    let sch = sch_of(&b);
    assert_eq!(sch.extras.label_spins[&sch.labels[0].id], LabelSpin::Up);
    assert_eq!(sch.extras.label_spins[&sch.labels[1].id], LabelSpin::Bottom);
}

#[test]
fn place_label_applies_the_look_and_refuses_what_the_dialog_would() {
    let m = ConstraintModel::default();
    let mut b = board(section(), &m);
    ok(&mut b, SchCmd::PlaceLabel { net: "VBUS".into(), at: p(0, 0), kind: LabelKind::Local, spin: Some(LabelSpin::Left), size_nm: Some(2_000_000), bold: Some(true), italic: None, auto_rotate: None });
    let sch = sch_of(&b);
    let look = sch.extras.label_looks[&sch.labels[0].id];
    assert_eq!((look.size_nm, look.bold, look.italic, look.auto_rotate), (2_000_000, true, false, false));
    assert_eq!(sch.extras.label_spins[&sch.labels[0].id], LabelSpin::Left);
    // an empty name, a text too small and "Auto" on a plain label are refused, and nothing is placed
    assert_eq!(err(&mut b, place("  ", p(0, 0), LabelKind::Local, None, None)), "ops_bad_label");
    assert_eq!(err(&mut b, SchCmd::PlaceLabel { net: "X".into(), at: p(0, 0), kind: LabelKind::Local, spin: None, size_nm: Some(5000), bold: None, italic: None, auto_rotate: None }), "ops_bad_text_size");
    assert_eq!(err(&mut b, place("X", p(0, 0), LabelKind::Local, Some(true), None)), "ops_label_no_auto_rotate");
    assert_eq!(sch_of(&b).labels.len(), 1);
}

// ------------------------------------------------------------------------------------------------------------------------- Label Properties

fn edit_label(id: &str, spin: Option<LabelSpin>, auto: Option<bool>) -> SchCmd {
    SchCmd::EditLabel { id: id.into(), text: None, shape: None, spin, size_nm: None, bold: None, italic: None, auto_rotate: auto, fields: None }
}

#[test]
fn label_properties_with_auto_on_rotate_the_label_and_ignore_the_spin_radio() {
    let m = ConstraintModel::default();
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(0, 0), p(10_160_000, 0)]));
    s.labels.push(global("lbl_g", p(10_160_000, 0)));
    s.extras.label_spins.insert("lbl_g".into(), LabelSpin::Up);
    let mut b = board(s, &m);
    ok(&mut b, edit_label("lbl_g", Some(LabelSpin::Bottom), Some(true)));
    let sch = sch_of(&b);
    assert_eq!(sch.extras.label_spins["lbl_g"], LabelSpin::Right, "the wire decides, not the radio button");
    assert!(sch.extras.label_looks["lbl_g"].auto_rotate);
    // with the check off the spin chosen is applied
    ok(&mut b, edit_label("lbl_g", Some(LabelSpin::Bottom), Some(false)));
    let sch = sch_of(&b);
    assert_eq!(sch.extras.label_spins["lbl_g"], LabelSpin::Bottom);
    assert!(!sch.extras.label_looks.contains_key("lbl_g"), "the default look keeps no entry");
}

#[test]
fn label_properties_refuse_auto_on_a_plain_label_and_an_edit_that_changes_nothing() {
    let m = ConstraintModel::default();
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(0, 0), p(10_160_000, 0)]));
    s.labels.push(local("lbl_l", p(10_160_000, 0)));
    s.labels.push(global("lbl_g", p(10_160_000, 0)));
    s.extras.label_spins.insert("lbl_g".into(), LabelSpin::Right);
    let mut b = board(s, &m);
    assert_eq!(err(&mut b, edit_label("lbl_l", None, Some(true))), "ops_label_no_auto_rotate");
    // the label is already as the wire would have it: turning "Auto" on changes the flag only, turning it on again changes nothing
    ok(&mut b, edit_label("lbl_g", None, Some(true)));
    assert_eq!(err(&mut b, edit_label("lbl_g", None, Some(true))), "ops_label_unchanged");
    // "Auto" off on a plain label is the dialog's own state: nothing to do
    assert_eq!(err(&mut b, edit_label("lbl_l", None, Some(false))), "ops_label_unchanged");
}

// ------------------------------------------------------------------------------------------------------------------------------ Move and Drag

fn mv(v: &[&str], dx: Nm, dy: Nm) -> SchMoveCmd {
    SchMoveCmd::Move { ids: ids(v), dx, dy, turns: Vec::new(), about: None }
}

#[test]
fn moving_an_auto_label_onto_another_wire_gives_it_that_wire_s_spin() {
    let m = ConstraintModel::default();
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(0, 0), p(10_160_000, 0)]));
    s.wires.push(wire("wire_b", &[p(30_480_000, 0), p(40_640_000, 0)]));
    s.labels.push(global("lbl_auto", p(10_160_000, 0)));
    s.labels.push(global("lbl_plain", p(10_160_000, 0)));
    s.extras.label_spins.insert("lbl_auto".into(), LabelSpin::Right);
    s.extras.label_spins.insert("lbl_plain".into(), LabelSpin::Right);
    s.extras.label_looks.insert("lbl_auto".into(), LabelLook { auto_rotate: true, ..Default::default() });
    let mut b = board(s, &m);
    b.apply(&Cmd::SchMove(mv(&["lbl_auto", "lbl_plain"], 20_320_000, 0))).unwrap();
    let sch = sch_of(&b);
    // both are now at the start of the second wire; only the one with "Auto" on turns to leave it on the left
    assert_eq!(sch.labels.iter().map(|l| l.at).collect::<Vec<_>>(), vec![p(30_480_000, 0), p(30_480_000, 0)]);
    assert_eq!(sch.extras.label_spins["lbl_auto"], LabelSpin::Left);
    assert_eq!(sch.extras.label_spins["lbl_plain"], LabelSpin::Right);
}

#[test]
fn dragging_labels_along_their_wire_gives_the_auto_one_the_spin_of_the_wire_again() {
    let m = ConstraintModel::default();
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(0, 0), p(10_160_000, 0)]));
    // a global label on the wire's end with "Auto" on, and a plain one beside it, both drawn pointing up
    s.labels.push(global("lbl_auto", p(10_160_000, 0)));
    s.labels.push(global("lbl_plain", p(10_160_000, 0)));
    s.extras.label_spins.insert("lbl_auto".into(), LabelSpin::Up);
    s.extras.label_spins.insert("lbl_plain".into(), LabelSpin::Up);
    s.extras.label_looks.insert("lbl_auto".into(), LabelLook { auto_rotate: true, ..Default::default() });
    let mut b = board(s, &m);
    // the labels are dragged along the wire and the wire's end follows them
    b.apply(&Cmd::SchMove(SchMoveCmd::Drag { ids: ids(&["lbl_auto", "lbl_plain"]), vertices: BTreeMap::new(), dx: 5_080_000, dy: 0, ortho: true, grid: 0, turns: Vec::new(), about: None })).unwrap();
    let sch = sch_of(&b);
    assert_eq!(sch.labels.iter().map(|l| l.at).collect::<Vec<_>>(), vec![p(15_240_000, 0), p(15_240_000, 0)], "{:?}", sch.wires);
    // "Auto" has it read on from the end of the wire again; the plain label keeps what it had
    assert_eq!(sch.extras.label_spins["lbl_auto"], LabelSpin::Right);
    assert_eq!(sch.extras.label_spins["lbl_plain"], LabelSpin::Up);
}

// --------------------------------------------------------------------------------------------------------------------------------- free text

fn text_sheet(angle: Millideg) -> SchematicSection {
    let mut s = section();
    s.texts.push(SchematicText { id: "txt_a".into(), content: "hello".into(), at: p(0, 0), angle, size_nm: 1_270_000 });
    s
}

fn justify_of(b: &Board, id: &str) -> SchTextJustify {
    sch_of(b).extras.text_justify_of(id)
}

fn j(h: TextJustify, v: TextVAlign) -> SchTextJustify {
    SchTextJustify { h, v }
}

fn rot(id: &str, ccw: bool) -> SchMoveCmd {
    SchMoveCmd::Rotate { ids: ids(&[id]), vertices: BTreeMap::new(), ccw, about: None, grid: 0 }
}

fn mir(id: &str, vertical: bool) -> SchMoveCmd {
    SchMoveCmd::Mirror { ids: ids(&[id]), vertices: BTreeMap::new(), vertical, about: None, grid: 0 }
}

#[test]
fn place_text_justifies_it_centred_unless_the_dialog_said_otherwise() {
    let m = ConstraintModel::default();
    let mut b = board(section(), &m);
    ok(&mut b, SchCmd::PlaceText { content: "centred".into(), at: p(0, 0), angle: 0, size_nm: 1_270_000, h: None, v: None });
    ok(&mut b, SchCmd::PlaceText { content: "left".into(), at: p(0, 5_080_000), angle: 90_000, size_nm: 1_270_000, h: Some(TextJustify::Left), v: Some(TextVAlign::Bottom) });
    ok(&mut b, SchCmd::PlaceText { content: "right".into(), at: p(0, 10_160_000), angle: 0, size_nm: 1_270_000, h: Some(TextJustify::Right), v: Some(TextVAlign::Top) });
    let sch = sch_of(&b);
    let id = |c: &str| sch.texts.iter().find(|t| t.content == c).unwrap().id.clone();
    assert!(sch.texts.iter().all(|t| !t.id.is_empty()));
    assert_eq!(sch.extras.text_justify_of(&id("centred")), SchTextJustify::FILE_DEFAULT);
    // left and bottom is the default and keeps no entry
    assert!(!sch.extras.text_justify.contains_key(&id("left")));
    assert_eq!(sch.extras.text_justify_of(&id("right")), j(TextJustify::Right, TextVAlign::Top));
    assert_eq!(sch.texts.iter().find(|t| t.content == "left").unwrap().angle, 90_000);
    // a slanted angle is made upright
    ok(&mut b, SchCmd::PlaceText { content: "slanted".into(), at: p(0, 0), angle: 80_000, size_nm: 1_270_000, h: None, v: None });
    assert_eq!(sch_of(&b).texts.last().unwrap().angle, 90_000);
}

#[test]
fn place_text_refuses_an_empty_text_and_a_size_the_dialog_would() {
    let m = ConstraintModel::default();
    let mut b = board(section(), &m);
    assert_eq!(err(&mut b, SchCmd::PlaceText { content: " ".into(), at: p(0, 0), angle: 0, size_nm: 1_270_000, h: None, v: None }), "ops_bad_text");
    assert_eq!(err(&mut b, SchCmd::PlaceText { content: "x".into(), at: p(0, 0), angle: 0, size_nm: 5000, h: None, v: None }), "ops_bad_text_size");
    assert!(sch_of(&b).texts.is_empty());
}

#[test]
fn text_properties_set_the_justification_and_nothing_else_with_it() {
    let m = ConstraintModel::default();
    let mut b = board(text_sheet(0), &m);
    ok(&mut b, SchCmd::EditText { id: "txt_a".into(), text: None, size_nm: None, angle: None, h: Some(TextJustify::Right), v: None });
    assert_eq!(justify_of(&b, "txt_a"), j(TextJustify::Right, TextVAlign::Bottom));
    ok(&mut b, SchCmd::EditText { id: "txt_a".into(), text: None, size_nm: None, angle: None, h: None, v: Some(TextVAlign::Center) });
    assert_eq!(justify_of(&b, "txt_a"), j(TextJustify::Right, TextVAlign::Center));
    // the same again is no edit; back to the default drops the entry
    assert_eq!(err(&mut b, SchCmd::EditText { id: "txt_a".into(), text: None, size_nm: None, angle: None, h: Some(TextJustify::Right), v: Some(TextVAlign::Center) }), "ops_text_unchanged");
    ok(&mut b, SchCmd::EditText { id: "txt_a".into(), text: None, size_nm: None, angle: None, h: Some(TextJustify::Left), v: Some(TextVAlign::Bottom) });
    assert!(sch_of(&b).extras.text_justify.is_empty());
    // the angle is made upright
    ok(&mut b, SchCmd::EditText { id: "txt_a".into(), text: None, size_nm: None, angle: Some(270_000), h: None, v: None });
    assert_eq!(sch_of(&b).texts[0].angle, 90_000);
}

#[test]
fn a_lone_text_turned_counter_clockwise_runs_up_the_sheet_and_back_with_the_other_end_on_its_anchor() {
    let m = ConstraintModel::default();
    let mut b = board(text_sheet(0), &m);
    // "textItem->Rotate90( clockwise )", clockwise false: along -> up, no flip
    b.apply(&Cmd::SchMove(rot("txt_a", true))).unwrap();
    assert_eq!((sch_of(&b).texts[0].angle, justify_of(&b, "txt_a")), (90_000, SchTextJustify::default()));
    // up -> along, counter-clockwise again: the justification flips, the text now ends at the anchor
    b.apply(&Cmd::SchMove(rot("txt_a", true))).unwrap();
    assert_eq!((sch_of(&b).texts[0].angle, justify_of(&b, "txt_a")), (0, j(TextJustify::Right, TextVAlign::Bottom)));
    // four quarter turns come back to where it was and drop the entry
    b.apply(&Cmd::SchMove(rot("txt_a", true))).unwrap();
    b.apply(&Cmd::SchMove(rot("txt_a", true))).unwrap();
    assert_eq!((sch_of(&b).texts[0].angle, justify_of(&b, "txt_a")), (0, SchTextJustify::default()));
    assert!(sch_of(&b).extras.text_justify.is_empty());
    assert_eq!(sch_of(&b).texts[0].at, p(0, 0), "a lone text turns about its own anchor");
}

#[test]
fn a_lone_text_turned_clockwise_flips_the_justification_when_it_starts_to_run_up() {
    let m = ConstraintModel::default();
    let mut b = board(text_sheet(0), &m);
    b.apply(&Cmd::SchMove(rot("txt_a", false))).unwrap();
    assert_eq!((sch_of(&b).texts[0].angle, justify_of(&b, "txt_a")), (90_000, j(TextJustify::Right, TextVAlign::Bottom)));
    b.apply(&Cmd::SchMove(rot("txt_a", false))).unwrap();
    assert_eq!((sch_of(&b).texts[0].angle, justify_of(&b, "txt_a")), (0, j(TextJustify::Right, TextVAlign::Bottom)), "a half turn either way ends the same");
}

#[test]
fn mirroring_a_lone_text_flips_its_justification_only_along_the_text() {
    let m = ConstraintModel::default();
    let mut b = board(text_sheet(0), &m);
    // X (left-right) on a horizontal text: "MirrorSpinStyle( !vertical )" flips it, the anchor stays
    b.apply(&Cmd::SchMove(mir("txt_a", false))).unwrap();
    assert_eq!((sch_of(&b).texts[0].at, justify_of(&b, "txt_a")), (p(0, 0), j(TextJustify::Right, TextVAlign::Bottom)));
    // Y (top-bottom) on a horizontal text leaves the justification as it is
    b.apply(&Cmd::SchMove(mir("txt_a", true))).unwrap();
    assert_eq!(justify_of(&b, "txt_a"), j(TextJustify::Right, TextVAlign::Bottom));
    // a vertical text flips with Y and not with X
    let mut v = board(text_sheet(90_000), &m);
    v.apply(&Cmd::SchMove(mir("txt_a", false))).unwrap();
    assert!(v.design().schematic.as_ref().unwrap().extras.text_justify.is_empty());
    v.apply(&Cmd::SchMove(mir("txt_a", true))).unwrap();
    assert_eq!(justify_of(&v, "txt_a"), j(TextJustify::Right, TextVAlign::Bottom));
}

#[test]
fn texts_in_a_selection_of_several_items_turn_and_mirror_about_the_selection() {
    let m = ConstraintModel::default();
    let mut s = text_sheet(0);
    s.texts.push(SchematicText { id: "txt_b".into(), content: "b".into(), at: p(10_160_000, 0), angle: 0, size_nm: 1_270_000 });
    let mut b = board(s, &m);
    // mirrored left-right about the middle of the two (`MirrorHorizontally`): the anchors swap sides and each text's justification flips
    b.apply(&Cmd::SchMove(SchMoveCmd::Mirror { ids: ids(&["txt_a", "txt_b"]), vertices: BTreeMap::new(), vertical: false, about: Some(p(5_080_000, 0)), grid: 0 })).unwrap();
    let sch = sch_of(&b);
    assert_eq!(sch.texts.iter().map(|t| (t.id.as_str(), t.at.x)).collect::<Vec<_>>(), vec![("txt_a", 10_160_000), ("txt_b", 0)]);
    assert_eq!(sch.extras.text_justify_of("txt_a"), j(TextJustify::Right, TextVAlign::Bottom));
    assert_eq!(sch.extras.text_justify_of("txt_b"), j(TextJustify::Right, TextVAlign::Bottom));
    // turned counter-clockwise about the same point (`SCH_TEXT::Rotate`): "Rotate90( false )" for each
    let mut c = board(text_sheet(0), &m);
    c.apply(&Cmd::SchMove(SchMoveCmd::Rotate { ids: ids(&["txt_a"]), vertices: BTreeMap::new(), ccw: true, about: Some(p(0, 5_080_000)), grid: 0 })).unwrap();
    assert_eq!(sch_of(&c).texts[0].angle, 90_000);
}

#[test]
fn deleting_a_text_takes_its_justification_with_it() {
    let m = ConstraintModel::default();
    let mut b = board(text_sheet(0), &m);
    ok(&mut b, SchCmd::EditText { id: "txt_a".into(), text: None, size_nm: None, angle: None, h: Some(TextJustify::Center), v: None });
    assert!(!sch_of(&b).extras.text_justify.is_empty());
    b.apply(&Cmd::DeleteSchText { id: "txt_a".into() }).unwrap();
    assert!(sch_of(&b).extras.text_justify.is_empty() && sch_of(&b).texts.is_empty());
}

#[test]
fn an_overlay_from_before_the_feature_reads_as_it_always_drew() {
    // design.json written before text could be justified has no entry for the text: left and bottom, as the studio drew it
    let s: SchematicSection = serde_json::from_str(r#"{"symbols":[],"wires":[],"labels":[],"texts":[{"id":"txt_a","content":"old","at":{"x":0,"y":0},"size_nm":1270}]}"#).unwrap();
    assert_eq!(s.extras.text_justify_of("txt_a"), SchTextJustify::default());
    assert_eq!(s.texts[0].angle, 0);
}
