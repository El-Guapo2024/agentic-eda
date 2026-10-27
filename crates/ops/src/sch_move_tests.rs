//! Tests for `Cmd::SchMove`: Move, Drag, Rotate, Mirror and Align to Grid of every kind of schematic item.
//!
//! The sheet used throughout is a KiCad-style one (`imported_from_kicad`: a symbol sits at its library origin) with `Device:R` resistors,
//! whose pins are 3.81 mm (three grid cells) above and below the origin: R1 at (20.32, 25.4) mm has pin 1 at (20.32, 21.59) and pin 2 at
//! (20.32, 29.21).

use super::*;
use crate::sch_move::{AlignMove, SchMoveCmd};
use crate::sch_scene::{rotate_point, Scene};
use eda_model::ir::{BusEntry, Junction, LabelKind, LabelShape, NetLabel, NoConnect, PowerSymbol, Provenance, SchLine, SchematicText, SheetInstance, SheetPin, SymbolInstance, Wire};
use eda_model::sch_extras::{LabelSpin, SchGraphic, SchGraphicKind};
use std::collections::BTreeMap;
use eda_model::units::mm;

const G: Nm = Nm::new(1_270_000);

fn empty_design() -> Design {
    Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        routing: None,
        placement: None,
        drawings: None,
    }
}

fn p(x: Nm, y: Nm) -> Point {
    Point { x, y }
}

fn sym(id: &str, at: Point) -> SymbolInstance {
    SymbolInstance { id: id.into(), at, rot: 0, mirrored: false, mirror_y: false, lib_id: "Device:R".into(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false }
}

fn wire(id: &str, pts: &[Point]) -> Wire {
    Wire { id: id.into(), net: "N".into(), pins: Vec::new(), pts: pts.to_vec(), bus: false }
}

fn label(id: &str, net: &str, at: Point, kind: LabelKind) -> NetLabel {
    NetLabel { id: id.into(), net: net.into(), at, kind }
}

fn section() -> eda_model::ir::SchematicSection {
    eda_model::ir::SchematicSection { imported_from_kicad: true, ..Default::default() }
}

fn run_on(sch: eda_model::ir::SchematicSection, cmd: SchMoveCmd) -> eda_model::ir::SchematicSection {
    let m = ConstraintModel::default();
    let mut design = empty_design();
    design.schematic = Some(sch);
    let mut b = Board::new(design, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::SchMove(cmd)).unwrap_or_else(|e| panic!("{e:?}"));
    b.design().schematic.clone().unwrap()
}

fn run_err(sch: eda_model::ir::SchematicSection, cmd: SchMoveCmd) -> String {
    let m = ConstraintModel::default();
    let mut design = empty_design();
    design.schematic = Some(sch);
    let mut b = Board::new(design, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::SchMove(cmd)).unwrap_err()[0].check.clone()
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn mv(v: &[&str], dx: Nm, dy: Nm) -> SchMoveCmd {
    SchMoveCmd::Move { ids: ids(v), dx, dy, turns: Vec::new(), about: None }
}

fn drag(v: &[&str], dx: Nm, dy: Nm) -> SchMoveCmd {
    SchMoveCmd::Drag { ids: ids(v), vertices: BTreeMap::new(), dx, dy, ortho: true, grid: Nm::ZERO, turns: Vec::new(), about: None }
}

fn rotate(v: &[&str], ccw: bool) -> SchMoveCmd {
    SchMoveCmd::Rotate { ids: ids(v), vertices: BTreeMap::new(), ccw, about: None, grid: Nm::ZERO }
}

fn mirror(v: &[&str], vertical: bool) -> SchMoveCmd {
    SchMoveCmd::Mirror { ids: ids(v), vertices: BTreeMap::new(), vertical, about: None, grid: Nm::ZERO }
}

fn wire_pts(sch: &eda_model::ir::SchematicSection) -> Vec<Vec<Point>> {
    let mut v: Vec<Vec<Point>> = sch.wires.iter().map(|w| w.pts.clone()).collect();
    v.sort();
    v
}

/// A sheet with every kind of item, none of them touching another.
fn every_kind() -> eda_model::ir::SchematicSection {
    let mut s = section();
    s.symbols.push(sym("R1", p(mm(20.32), mm(25.4))));
    s.wires.push(wire("wire_a", &[p(mm(50.8), mm(12.7)), p(mm(63.5), mm(12.7))]));
    s.labels.push(label("lbl_local", "A", p(mm(76.2), mm(25.4)), LabelKind::Local));
    s.labels.push(label("lbl_global", "B", p(mm(76.2), mm(38.1)), LabelKind::Global { shape: LabelShape::Input }));
    s.labels.push(label("lbl_hier", "C", p(mm(76.2), mm(50.8)), LabelKind::Hierarchical { shape: LabelShape::Output }));
    s.power_symbols.push(PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: p(mm(101.6), mm(25.4)), rot: 0, net: "GND".into(), pin: String::new() });
    s.texts.push(SchematicText { id: "txt_a".into(), content: "hello".into(), at: p(mm(101.6), mm(38.1)), angle: 0, size_nm: mm(1.27) });
    s.no_connects.push(NoConnect { id: "nc_a".into(), at: p(mm(101.6), mm(50.8)), pin: String::new() });
    s.bus_entries.push(BusEntry { id: "bent_a".into(), at: p(mm(127.0), mm(25.4)), size: p(mm(2.54), mm(2.54)) });
    s.junctions.push(Junction { id: "jct_a".into(), at: p(mm(127.0), mm(38.1)) });
    s.lines.push(SchLine { id: "sln_a".into(), pts: vec![p(mm(127.0), mm(50.8)), p(mm(139.7), mm(50.8))], width_nm: Nm::ZERO });
    s.extras.graphics.push(SchGraphic::new(SchGraphicKind::Rectangle { start: p(mm(152.4), mm(12.7)), end: p(mm(165.1), mm(25.4)), corner_radius_nm: Nm::ZERO }));
    s.extras.graphics.push(SchGraphic::new(SchGraphicKind::TextBox { start: p(mm(152.4), mm(38.1)), end: p(mm(177.8), mm(50.8)), text: "note".into(), angle: 0, size_nm: mm(1.27), bold: false, italic: false, h_align: eda_model::sch_extras::SchHAlign::Left, v_align: eda_model::sch_extras::SchVAlign::Top, margin_nm: Nm::ZERO }));
    s.sheets.push(SheetInstance {
        id: "sheet_a".into(),
        name: "Sub".into(),
        file: "sub.kicad_sch".into(),
        at: p(mm(190.5), mm(12.7)),
        size: (mm(25.4), mm(12.7)),
        pins: vec![SheetPin { id: "shpin_a".into(), name: "IN".into(), shape: LabelShape::Input, at: p(mm(190.5), mm(17.78)) }, SheetPin { id: "shpin_b".into(), name: "OUT".into(), shape: LabelShape::Output, at: p(mm(215.9), mm(20.32)) }],
        page: String::new(),
    });
    s.assign_missing_ids();
    s
}

fn graphic_ids(s: &eda_model::ir::SchematicSection) -> Vec<String> {
    s.extras.graphics.iter().map(|g| g.id.clone()).collect()
}

// ------------------------------------------------------------------------------------------------------------------------- move

#[test]
fn move_shifts_every_kind_of_item_by_the_offset_and_nothing_else() {
    let before = every_kind();
    let gids = graphic_ids(&before);
    let mut all = ids(&["R1", "wire_a", "lbl_local", "lbl_global", "lbl_hier", "#PWR01", "txt_a", "nc_a", "bent_a", "jct_a", "sln_a", "sheet_a"]);
    all.extend(gids);
    let after = run_on(before.clone(), SchMoveCmd::Move { ids: all, dx: 2 * G, dy: -G, turns: Vec::new(), about: None });
    let d = |q: Point| p(q.x + 2 * G, q.y - G);

    assert_eq!(after.symbols[0].at, d(before.symbols[0].at));
    assert_eq!(wire_pts(&after), vec![before.wires[0].pts.iter().map(|q| d(*q)).collect::<Vec<_>>()]);
    for (a, b) in after.labels.iter().zip(&before.labels) {
        assert_eq!(a.at, d(b.at), "{}", a.id);
    }
    assert_eq!(after.power_symbols[0].at, d(before.power_symbols[0].at));
    assert_eq!(after.texts[0].at, d(before.texts[0].at));
    assert_eq!(after.no_connects[0].at, d(before.no_connects[0].at));
    assert_eq!(after.bus_entries[0].at, d(before.bus_entries[0].at));
    assert_eq!(after.bus_entries[0].size, before.bus_entries[0].size, "a bus entry keeps its size");
    assert_eq!(after.junctions[0].at, d(before.junctions[0].at));
    assert_eq!(after.lines[0].pts, before.lines[0].pts.iter().map(|q| d(*q)).collect::<Vec<_>>());
    match (&after.extras.graphics[0].shape, &before.extras.graphics[0].shape) {
        (SchGraphicKind::Rectangle { start: a, end: b, .. }, SchGraphicKind::Rectangle { start: c, end: e, .. }) => assert_eq!((*a, *b), (d(*c), d(*e))),
        _ => panic!("rectangle"),
    }
    match (&after.extras.graphics[1].shape, &before.extras.graphics[1].shape) {
        (SchGraphicKind::TextBox { start: a, end: b, text, .. }, SchGraphicKind::TextBox { start: c, end: e, .. }) => {
            assert_eq!((*a, *b), (d(*c), d(*e)));
            assert_eq!(text, "note");
        }
        _ => panic!("text box"),
    }
    let (sa, sb) = (&after.sheets[0], &before.sheets[0]);
    assert_eq!(sa.at, d(sb.at));
    assert_eq!(sa.size, sb.size);
    assert_eq!(sa.pins.iter().map(|q| q.at).collect::<Vec<_>>(), sb.pins.iter().map(|q| d(q.at)).collect::<Vec<_>>(), "a sheet takes its pins along");
}

#[test]
fn move_leaves_a_wire_where_it_is_when_only_the_symbol_moves() {
    let mut s = section();
    s.symbols.push(sym("R1", p(mm(20.32), mm(25.4))));
    // a wire from pin 1 straight up
    s.wires.push(wire("wire_a", &[p(mm(20.32), mm(21.59)), p(mm(20.32), mm(10.16))]));
    let after = run_on(s.clone(), mv(&["R1"], Nm::ZERO, 5 * G));
    assert_eq!(after.symbols[0].at, p(mm(20.32), mm(25.4) + 5 * G));
    assert_eq!(wire_pts(&after), vec![vec![p(mm(20.32), mm(21.59)), p(mm(20.32), mm(10.16))]], "Move does not stretch a wire");
}

#[test]
fn move_of_nothing_or_of_an_unknown_id_is_refused_and_a_locked_item_is_not_moved() {
    let mut s = every_kind();
    assert_eq!(run_err(s.clone(), SchMoveCmd::Move { ids: Vec::new(), dx: G, dy: Nm::ZERO, turns: Vec::new(), about: None }), "ops_nothing_selected");
    assert_eq!(run_err(s.clone(), mv(&["R404"], G, Nm::ZERO)), "ops_unknown_item");
    s.extras.set_locked("R1", true);
    assert_eq!(run_err(s.clone(), mv(&["R1"], G, Nm::ZERO)), "ops_locked");
    let after = run_on(s.clone(), mv(&["R1", "txt_a"], G, Nm::ZERO));
    assert_eq!(after.symbols[0].at, s.symbols[0].at, "the locked symbol stays");
    assert_eq!(after.texts[0].at, p(s.texts[0].at.x + G, s.texts[0].at.y), "the rest of the selection moves");
}

#[test]
fn move_of_a_multi_unit_symbol_takes_every_unit_and_one_unit_can_be_named() {
    let mut s = section();
    let mut u1 = sym("U1", p(mm(10.16), mm(10.16)));
    u1.unit = 1;
    let mut u2 = sym("U1", p(mm(40.64), mm(10.16)));
    u2.unit = 2;
    s.symbols.push(u1);
    s.symbols.push(u2);
    let all = run_on(s.clone(), mv(&["U1"], G, Nm::ZERO));
    assert_eq!(all.symbols.iter().map(|x| x.at.x).collect::<Vec<_>>(), vec![mm(10.16) + G, mm(40.64) + G]);
    let one = run_on(s, mv(&["U1#2"], G, Nm::ZERO));
    assert_eq!(one.symbols.iter().map(|x| x.at.x).collect::<Vec<_>>(), vec![mm(10.16), mm(40.64) + G]);
}

#[test]
fn moving_a_wire_whole_keeps_its_id_and_shape() {
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(Nm::ZERO, Nm::ZERO), p(mm(12.7), Nm::ZERO), p(mm(12.7), mm(12.7))]));
    let after = run_on(s, mv(&["wire_a"], G, G));
    assert_eq!(after.wires.len(), 1);
    assert_eq!(after.wires[0].id, "wire_a");
    assert_eq!(after.wires[0].pts, vec![p(G, G), p(mm(12.7) + G, G), p(mm(12.7) + G, mm(12.7) + G)]);
}

#[test]
fn moving_a_two_pin_part_with_both_pins_on_one_wire_trims_the_wire_between_them() {
    let mut s = section();
    // a horizontal resistor needs both pins on the wire: turn R1 a quarter turn, its pins are then 7.62 mm apart along x
    let mut r = sym("R1", p(mm(20.32), mm(25.4)));
    r.rot = 90_000;
    s.symbols.push(r);
    // pins of R1 turned CCW: pin 1 at origin + (-3810, 0)? (x', y') = (y, -x) of internal (0, -3810) -> (-3810, 0); pin 2 -> (3810, 0)
    s.wires.push(wire("wire_a", &[p(Nm::ZERO, mm(50.8)), p(mm(60.96), mm(50.8))]));
    let after = run_on(s, mv(&["R1"], Nm::ZERO, mm(25.4)));
    // R1 lands with its pins at (16_510, 50_800) and (24_130, 50_800): the wire between them is cut
    let mut got = wire_pts(&after);
    got.sort();
    assert_eq!(got, vec![vec![p(Nm::ZERO, mm(50.8)), p(mm(16.51), mm(50.8))], vec![p(mm(24.13), mm(50.8)), p(mm(60.96), mm(50.8))]]);
}

#[test]
fn moving_a_wire_away_from_a_t_junction_removes_the_junction_dot() {
    let mut s = section();
    // a through wire, a branch ending on it, and an explicit dot at the T
    s.wires.push(wire("wire_a", &[p(Nm::ZERO, Nm::ZERO), p(mm(25.4), Nm::ZERO)]));
    s.wires.push(wire("wire_b", &[p(mm(12.7), Nm::ZERO), p(mm(12.7), mm(12.7))]));
    s.junctions.push(Junction { id: "jct_a".into(), at: p(mm(12.7), Nm::ZERO) });
    let after = run_on(s, mv(&["wire_b"], mm(25.4), Nm::ZERO));
    assert!(after.junctions.is_empty(), "nothing joins at the old T any more: {:?}", after.junctions);
}

// ------------------------------------------------------------------------------------------------------------------------- drag

fn pin_points_of(sch: &eda_model::ir::SchematicSection, id: &str) -> Vec<Point> {
    let m = ConstraintModel::default();
    let scene = Scene::new(sch, &m);
    let i = sch.symbols.iter().position(|s| s.id == id).unwrap();
    scene.pin_tips(i)
}

#[test]
fn drag_of_a_symbol_stretches_the_wire_on_its_pin_and_keeps_it_attached() {
    let mut s = section();
    s.symbols.push(sym("R1", p(mm(20.32), mm(25.4))));
    // a wire from pin 1 (above) straight up to a free end
    s.wires.push(wire("wire_a", &[p(mm(20.32), mm(21.59)), p(mm(20.32), mm(10.16))]));
    // dragged up: the wire is parallel to the move, so it just shortens
    let after = run_on(s.clone(), drag(&["R1"], Nm::ZERO, -2 * G));
    assert_eq!(after.symbols[0].at, p(mm(20.32), mm(25.4) - 2 * G));
    assert!(pin_points_of(&after, "R1").contains(&p(mm(20.32), mm(21.59) - 2 * G)));
    assert_eq!(wire_pts(&after), vec![vec![p(mm(20.32), mm(21.59) - 2 * G), p(mm(20.32), mm(10.16))]]);
}

#[test]
fn drag_sideways_keeps_a_vertical_wire_to_a_fixed_pin_orthogonal_with_two_new_segments() {
    // R1's pin 1 wired straight up to R2's pin 2 (fixed). Dragging R1 sideways must not slant the wire: `orthoLineDrag` moves the loose
    // end of the wire to a point one grid cell from the moving end and joins it to the old one by two new lines.
    let mut s = section();
    s.symbols.push(sym("R1", p(mm(20.32), mm(45.72))));
    s.symbols.push(sym("R2", p(mm(20.32), mm(10.16))));
    // R1 pin 1 at (20_320, 41_910); R2 pin 2 at (20_320, 13_970)
    s.wires.push(wire("wire_a", &[p(mm(20.32), mm(41.91)), p(mm(20.32), mm(13.97))]));
    let after = run_on(s.clone(), drag(&["R1"], 5 * G, Nm::ZERO));
    assert_eq!(after.symbols.iter().find(|x| x.id == "R1").unwrap().at.x, mm(20.32) + 5 * G);
    let r1_pin1 = pin_points_of(&after, "R1")[0];
    assert_eq!(r1_pin1, p(mm(20.32) + 5 * G, mm(41.91)));
    let mut got = wire_pts(&after);
    got.sort();
    let mut want = vec![
        // the wire from the pin, shortened to one grid cell
        vec![p(mm(26.67), mm(41.91)), p(mm(26.67), mm(40.64))],
        // across, and up to R2 along the old wire
        vec![p(mm(26.67), mm(40.64)), p(mm(20.32), mm(40.64))],
        vec![p(mm(20.32), mm(40.64)), p(mm(20.32), mm(13.97))],
    ];
    want.sort();
    assert_eq!(got, want);
    // the left of the symbol: the same, mirrored
    let left = run_on(s, drag(&["R1"], -5 * G, Nm::ZERO));
    let segs: Vec<(Point, Point)> = left.wires.iter().flat_map(|w| w.pts.windows(2).map(|q| (q[0], q[1])).collect::<Vec<_>>()).collect();
    assert!(segs.iter().all(|(a, b)| a.x == b.x || a.y == b.y), "{segs:?}");
    assert!(connected(&segs, p(mm(20.32) - 5 * G, mm(41.91)), p(mm(20.32), mm(13.97))), "{segs:?}");
}

#[test]
fn drag_sideways_slides_the_corner_of_an_elbow_wire_instead_of_adding_bends() {
    // the wire leaves the pin upward, turns right at y = 30_480 and goes on to R2: its second segment runs the way R1 is dragged, so the
    // corner moves along with the pin and no segment is added
    let mut s = section();
    s.symbols.push(sym("R1", p(mm(20.32), mm(45.72))));
    s.symbols.push(sym("R2", p(mm(40.64), mm(8.89))));
    s.wires.push(wire("wire_a", &[p(mm(20.32), mm(41.91)), p(mm(20.32), mm(30.48)), p(mm(40.64), mm(30.48)), p(mm(40.64), mm(12.7))]));
    let after = run_on(s.clone(), drag(&["R1"], 5 * G, Nm::ZERO));
    assert_eq!(wire_pts(&after), vec![vec![p(mm(26.67), mm(41.91)), p(mm(26.67), mm(30.48)), p(mm(40.64), mm(30.48)), p(mm(40.64), mm(12.7))]]);
    // dragged straight down the wire only gets longer
    let down = run_on(s, drag(&["R1"], Nm::ZERO, 3 * G));
    assert_eq!(wire_pts(&down), vec![vec![p(mm(20.32), mm(45.72)), p(mm(20.32), mm(30.48)), p(mm(40.64), mm(30.48)), p(mm(40.64), mm(12.7))]]);
}

#[test]
fn drag_of_a_part_off_the_end_of_a_wire_that_ends_on_a_junction_slides_that_end_along_the_other_wire() {
    // "These are special-cased and get a single line added instead of a 90-degree bend"
    let mut s = section();
    s.symbols.push(sym("R1", p(mm(20.32), mm(45.72))));
    s.wires.push(wire("wire_t", &[p(mm(20.32), mm(41.91)), p(mm(20.32), mm(25.4))]));
    s.wires.push(wire("wire_h", &[p(mm(10.16), mm(25.4)), p(mm(40.64), mm(25.4))]));
    s.junctions.push(Junction { id: "jct_a".into(), at: p(mm(20.32), mm(25.4)) });
    let after = run_on(s, drag(&["R1"], 5 * G, Nm::ZERO));
    let segs: Vec<(Point, Point)> = after.wires.iter().flat_map(|w| w.pts.windows(2).map(|q| (q[0], q[1])).collect::<Vec<_>>()).collect();
    assert!(segs.contains(&(p(mm(26.67), mm(41.91)), p(mm(26.67), mm(25.4)))), "{segs:?}");
    // the dot moved with the T: one at the new meeting point, none at the old
    assert_eq!(after.junctions.iter().map(|j| j.at).collect::<Vec<_>>(), vec![p(mm(26.67), mm(25.4))]);
    // the through wire is whole again
    assert!(connected(&segs, p(mm(10.16), mm(25.4)), p(mm(40.64), mm(25.4))));
}

#[test]
fn drag_of_a_part_whose_pin_is_on_another_pin_leaves_a_wire_between_them() {
    let mut s = section();
    s.symbols.push(sym("R1", p(mm(20.32), mm(25.4))));
    s.symbols.push(sym("R2", p(mm(20.32), mm(33.02))));
    // R1's pin 2 and R2's pin 1 are the same point, (20_320, 29_210), with no wire
    let sideways = run_on(s.clone(), drag(&["R2"], 5 * G, Nm::ZERO));
    assert_eq!(wire_pts(&sideways), vec![vec![p(mm(26.67), mm(29.21)), p(mm(20.32), mm(29.21))]]);
    let down = run_on(s, drag(&["R2"], Nm::ZERO, 4 * G));
    assert_eq!(wire_pts(&down), vec![vec![p(mm(20.32), mm(34.29)), p(mm(20.32), mm(29.21))]]);
}

#[test]
fn labels_on_a_dragged_wire_stay_on_it() {
    let mut s = section();
    s.symbols.push(sym("R1", p(mm(20.32), mm(45.72))));
    s.wires.push(wire("wire_a", &[p(mm(20.32), mm(41.91)), p(mm(20.32), mm(25.4))]));
    s.labels.push(label("lbl_a", "A", p(mm(20.32), mm(25.4)), LabelKind::Local));
    s.labels.push(label("lbl_b", "B", p(mm(20.32), mm(33.02)), LabelKind::Local));
    for (dx, dy) in [(5 * G, Nm::ZERO), (Nm::ZERO, -2 * G), (Nm::ZERO, 2 * G)] {
        let after = run_on(s.clone(), drag(&["R1"], dx, dy));
        let segs: Vec<(Point, Point)> = after.wires.iter().flat_map(|w| w.pts.windows(2).map(|q| (q[0], q[1])).collect::<Vec<_>>()).collect();
        for l in &after.labels {
            assert!(segs.iter().any(|(a, b)| crate::sch_scene::on_segment(*a, *b, l.at)), "{} at {:?} is off every wire {segs:?} after {dx},{dy}", l.id, l.at);
        }
    }
}

/// Is `a` joined to `b` through the segments (end to end)?
fn connected(segs: &[(Point, Point)], a: Point, b: Point) -> bool {
    let mut reach = vec![a];
    let mut grew = true;
    while grew {
        grew = false;
        for (s, e) in segs {
            for (from, to) in [(s, e), (e, s)] {
                if reach.contains(from) && !reach.contains(to) {
                    reach.push(*to);
                    grew = true;
                }
            }
        }
    }
    reach.contains(&b)
}

#[test]
fn drag_of_a_label_on_a_wire_slides_the_label_with_the_wire() {
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(Nm::ZERO, Nm::ZERO), p(mm(25.4), Nm::ZERO)]));
    // a label in the middle of the wire, and a second wire end dragged along
    s.labels.push(label("lbl_a", "A", p(mm(12.7), Nm::ZERO), LabelKind::Local));
    let after = run_on(s, drag(&["wire_a"], Nm::ZERO, 2 * G));
    assert_eq!(wire_pts(&after), vec![vec![p(Nm::ZERO, 2 * G), p(mm(25.4), 2 * G)]]);
    assert_eq!(after.labels[0].at, p(mm(12.7), 2 * G), "the label stays on its wire");
}

#[test]
fn drag_of_a_wire_end_stretches_only_that_end_and_its_neighbour_follows() {
    // two wires meeting at a corner (a bend drawn as two wires); pick the first wire's free end and drag it
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(Nm::ZERO, Nm::ZERO), p(mm(12.7), Nm::ZERO)]));
    s.wires.push(wire("wire_b", &[p(mm(12.7), Nm::ZERO), p(mm(12.7), mm(12.7))]));
    // dragging wire_a's start straight down: the wire is not parallel to the move, so its far end (the corner) goes along,
    // taking wire_b's end with it, which is parallel to the move
    let mut vertices = BTreeMap::new();
    vertices.insert("wire_a".to_string(), vec![0usize]);
    let after = run_on(s, SchMoveCmd::Drag { ids: ids(&["wire_a"]), vertices, dx: Nm::ZERO, dy: 2 * G, ortho: true, grid: Nm::ZERO, turns: Vec::new(), about: None });
    let got = wire_pts(&after);
    assert_eq!(got, vec![vec![p(Nm::ZERO, 2 * G), p(mm(12.7), 2 * G)], vec![p(mm(12.7), 2 * G), p(mm(12.7), mm(12.7))]], "{got:?}");
}

#[test]
fn drag_with_free_angles_just_moves_the_picked_end() {
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(Nm::ZERO, Nm::ZERO), p(mm(12.7), Nm::ZERO)]));
    let mut vertices = BTreeMap::new();
    vertices.insert("wire_a".to_string(), vec![1usize]);
    let after = run_on(s, SchMoveCmd::Drag { ids: ids(&["wire_a"]), vertices, dx: Nm::ZERO, dy: 2 * G, ortho: false, grid: Nm::ZERO, turns: Vec::new(), about: None });
    assert_eq!(wire_pts(&after), vec![vec![p(Nm::ZERO, Nm::ZERO), p(mm(12.7), 2 * G)]]);
}

#[test]
fn drag_of_a_symbol_takes_a_no_connect_on_its_pin_along() {
    let mut s = section();
    s.symbols.push(sym("R1", p(mm(20.32), mm(25.4))));
    s.no_connects.push(NoConnect { id: "nc_a".into(), at: p(mm(20.32), mm(29.21)), pin: String::new() });
    let after = run_on(s, drag(&["R1"], 2 * G, Nm::ZERO));
    assert_eq!(after.no_connects[0].at, p(mm(20.32) + 2 * G, mm(29.21)));
}

#[test]
fn drag_of_a_label_takes_the_wire_on_it_along() {
    // a wire ends on a label; dragging the label drags that end of the wire
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(Nm::ZERO, Nm::ZERO), p(mm(12.7), Nm::ZERO)]));
    s.labels.push(label("lbl_a", "A", p(mm(12.7), Nm::ZERO), LabelKind::Local));
    let after = run_on(s, drag(&["lbl_a"], 2 * G, Nm::ZERO));
    assert_eq!(after.labels[0].at, p(mm(12.7) + 2 * G, Nm::ZERO));
    assert_eq!(wire_pts(&after), vec![vec![p(Nm::ZERO, Nm::ZERO), p(mm(12.7) + 2 * G, Nm::ZERO)]]);
}

#[test]
fn drag_of_a_junction_takes_the_wire_ends_on_it_along() {
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(Nm::ZERO, Nm::ZERO), p(mm(12.7), Nm::ZERO)]));
    s.wires.push(wire("wire_b", &[p(mm(12.7), Nm::ZERO), p(mm(25.4), Nm::ZERO)]));
    s.wires.push(wire("wire_c", &[p(mm(12.7), Nm::ZERO), p(mm(12.7), mm(-12.7))]));
    s.junctions.push(Junction { id: "jct_a".into(), at: p(mm(12.7), Nm::ZERO) });
    let after = run_on(s, drag(&["jct_a"], Nm::ZERO, 2 * G));
    assert_eq!(after.junctions[0].at, p(mm(12.7), 2 * G));
}

#[test]
fn drag_of_a_sheet_keeps_the_wire_on_its_pin_attached_with_right_angles() {
    let mut s = section();
    s.symbols.push(sym("R9", p(mm(25.4), mm(21.59))));
    s.sheets.push(SheetInstance {
        id: "sheet_a".into(),
        name: "Sub".into(),
        file: "sub.kicad_sch".into(),
        at: p(mm(50.8), mm(12.7)),
        size: (mm(25.4), mm(12.7)),
        pins: vec![SheetPin { id: "shpin_a".into(), name: "IN".into(), shape: LabelShape::Input, at: p(mm(50.8), mm(17.78)) }],
        page: String::new(),
    });
    // R9's pin 1 sits at (25_400, 17_780), the sheet pin at (50_800, 17_780)
    s.wires.push(wire("wire_a", &[p(mm(25.4), mm(17.78)), p(mm(50.8), mm(17.78))]));
    let after = run_on(s, drag(&["sheet_a"], Nm::ZERO, 2 * G));
    assert_eq!(after.sheets[0].at, p(mm(50.8), mm(12.7) + 2 * G));
    assert_eq!(after.sheets[0].pins[0].at, p(mm(50.8), mm(17.78) + 2 * G));
    let segs: Vec<(Point, Point)> = after.wires.iter().flat_map(|w| w.pts.windows(2).map(|q| (q[0], q[1])).collect::<Vec<_>>()).collect();
    assert!(connected(&segs, p(mm(25.4), mm(17.78)), p(mm(50.8), mm(17.78) + 2 * G)), "{segs:?}");
    assert!(segs.iter().all(|(a, b)| a.x == b.x || a.y == b.y), "{segs:?}");
}

#[test]
fn drag_of_a_sheet_with_a_free_wire_just_carries_the_wire() {
    // "Original line has no attachments, just move the unselected end"
    let mut s = section();
    s.sheets.push(SheetInstance {
        id: "sheet_a".into(),
        name: "Sub".into(),
        file: "sub.kicad_sch".into(),
        at: p(mm(50.8), mm(12.7)),
        size: (mm(25.4), mm(12.7)),
        pins: vec![SheetPin { id: "shpin_a".into(), name: "IN".into(), shape: LabelShape::Input, at: p(mm(50.8), mm(17.78)) }],
        page: String::new(),
    });
    s.wires.push(wire("wire_a", &[p(mm(25.4), mm(17.78)), p(mm(50.8), mm(17.78))]));
    let after = run_on(s, drag(&["sheet_a"], Nm::ZERO, 2 * G));
    assert_eq!(after.sheets[0].pins[0].at, p(mm(50.8), mm(17.78) + 2 * G));
    assert_eq!(wire_pts(&after), vec![vec![p(mm(25.4), mm(17.78) + 2 * G), p(mm(50.8), mm(17.78) + 2 * G)]]);
}

// ----------------------------------------------------------------------------------------------------------------------- rotate

#[test]
fn a_lone_symbol_turns_about_its_own_anchor() {
    let mut s = section();
    s.symbols.push(sym("R1", p(mm(20.32), mm(25.4))));
    let after = run_on(s.clone(), rotate(&["R1"], true));
    assert_eq!(after.symbols[0].at, p(mm(20.32), mm(25.4)));
    assert_eq!(after.symbols[0].rot, 90_000);
    // the pins swung from above and below to the left and right
    let pins = pin_points_of(&after, "R1");
    assert_eq!(pins, vec![p(mm(20.32) - mm(3.81), mm(25.4)), p(mm(20.32) + mm(3.81), mm(25.4))]);
    let cw = run_on(s, rotate(&["R1"], false));
    assert_eq!(cw.symbols[0].rot, 270_000);
}

#[test]
fn several_items_turn_about_the_half_grid_point_nearest_their_centre() {
    let mut s = section();
    s.symbols.push(sym("R1", p(mm(20.32), mm(25.4))));
    s.symbols.push(sym("R2", p(mm(40.64), mm(25.4))));
    // the two boxes span x 20_320..40_640 (centre 30_480, which is on the half grid: 24 * 1270 / 2)... and y 21_590..29_210 (centre 25_400)
    let after = run_on(s, rotate(&["R1", "R2"], true));
    let c = p(mm(30.48), mm(25.4));
    let want = |at: Point| rotate_point(at, c, true);
    assert_eq!(after.symbols[0].at, want(p(mm(20.32), mm(25.4))));
    assert_eq!(after.symbols[1].at, want(p(mm(40.64), mm(25.4))));
    assert!(after.symbols.iter().all(|x| x.rot == 90_000));
}

#[test]
fn a_lone_wire_turns_about_its_far_end() {
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(Nm::ZERO, Nm::ZERO), p(mm(12.7), Nm::ZERO)]));
    let after = run_on(s, rotate(&["wire_a"], true));
    // both ends turn, the end point (12_700, 0) staying put; the start swings a quarter turn about it, counter-clockwise on the screen
    assert_eq!(wire_pts(&after), vec![vec![p(mm(12.7), mm(12.7)), p(mm(12.7), Nm::ZERO)]]);
}

#[test]
fn a_wire_picked_at_one_end_turns_that_end_about_the_other() {
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(Nm::ZERO, Nm::ZERO), p(mm(12.7), Nm::ZERO)]));
    let mut vertices = BTreeMap::new();
    vertices.insert("wire_a".to_string(), vec![1usize]);
    let after = run_on(s, SchMoveCmd::Rotate { ids: ids(&["wire_a"]), vertices, ccw: true, about: None, grid: Nm::ZERO });
    assert_eq!(wire_pts(&after), vec![vec![p(Nm::ZERO, Nm::ZERO), p(Nm::ZERO, mm(-12.7))]]);
}

#[test]
fn a_label_turned_in_place_changes_its_spin_and_a_second_turn_goes_on_from_there() {
    let mut s = section();
    s.labels.push(label("lbl_a", "A", p(mm(12.7), Nm::ZERO), LabelKind::Local));
    let once = run_on(s.clone(), rotate(&["lbl_a"], true));
    assert_eq!(once.labels[0].at, p(mm(12.7), Nm::ZERO), "a single label turns about its own anchor");
    assert_eq!(once.extras.label_spins.get("lbl_a"), Some(&LabelSpin::Up), "a label nothing is wired to reads to the right; turned CCW it goes up");
    let twice = run_on(once, rotate(&["lbl_a"], true));
    assert_eq!(twice.extras.label_spins.get("lbl_a"), Some(&LabelSpin::Left));
    let back = run_on(twice, rotate(&["lbl_a"], false));
    assert_eq!(back.extras.label_spins.get("lbl_a"), Some(&LabelSpin::Up));
}

#[test]
fn a_label_wired_from_the_left_starts_as_right_and_turns_from_there() {
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(Nm::ZERO, Nm::ZERO), p(mm(12.7), Nm::ZERO)]));
    s.labels.push(label("lbl_a", "A", p(mm(12.7), Nm::ZERO), LabelKind::Global { shape: LabelShape::Input }));
    let cw = run_on(s, rotate(&["lbl_a"], false));
    assert_eq!(cw.extras.label_spins.get("lbl_a"), Some(&LabelSpin::Bottom));
}

#[test]
fn text_turns_in_place_and_a_group_turns_around_its_centre() {
    let mut s = section();
    s.texts.push(SchematicText { id: "txt_a".into(), content: "a".into(), at: p(Nm::ZERO, Nm::ZERO), angle: 0, size_nm: mm(1.27) });
    s.texts.push(SchematicText { id: "txt_b".into(), content: "b".into(), at: p(mm(25.4), Nm::ZERO), angle: 0, size_nm: mm(1.27) });
    let one = run_on(s.clone(), rotate(&["txt_a"], true));
    assert_eq!((one.texts[0].at, one.texts[0].angle), (p(Nm::ZERO, Nm::ZERO), 90_000));
    // two texts: the mean of their places is the centre
    let two = run_on(s, rotate(&["txt_a", "txt_b"], true));
    let c = p(mm(12.7), Nm::ZERO);
    assert_eq!(two.texts[0].at, rotate_point(p(Nm::ZERO, Nm::ZERO), c, true));
    assert_eq!(two.texts[1].at, rotate_point(p(mm(25.4), Nm::ZERO), c, true));
}

#[test]
fn a_junction_a_no_connect_and_a_bus_entry_turn_about_their_own_anchor_or_the_selection_centre() {
    let s = every_kind();
    let one = run_on(s.clone(), rotate(&["bent_a"], true));
    assert_eq!(one.bus_entries[0].at, s.bus_entries[0].at, "a lone bus entry keeps its anchor");
    assert_eq!(one.bus_entries[0].size, p(mm(2.54), mm(-2.54)), "and its size turns as a vector");
    let nc = run_on(s.clone(), rotate(&["nc_a"], true));
    assert_eq!(nc.no_connects[0].at, s.no_connects[0].at);
    let two = run_on(s.clone(), rotate(&["jct_a", "nc_a"], true));
    let c = half_grid_of(p(mm(114.3), mm(44.45)));
    assert_eq!(two.junctions[0].at, rotate_point(s.junctions[0].at, c, true));
}

fn half_grid_of(q: Point) -> Point {
    let h = G.as_f64() / 2.0;
    p(Nm::new(((q.x.as_f64() / h).round() * h) as i64), Nm::new(((q.y.as_f64() / h).round() * h) as i64))
}

#[test]
fn a_sheet_turns_about_the_half_grid_point_nearest_its_centre_with_its_pins() {
    let s = every_kind();
    // 25.4 x 12.7 mm at (190.5, 12.7); its centre (203.2, 19.05) is on the half grid
    let after = run_on(s, rotate(&["sheet_a"], true));
    let a = &after.sheets[0];
    assert_eq!((a.at, a.size), (p(mm(196.85), mm(6.35)), (mm(12.7), mm(25.4))));
    // the pin on the left edge went to the bottom edge and the one on the right edge to the top edge
    assert_eq!(a.pins[0].at, p(mm(201.93), mm(31.75)));
    assert_eq!(a.pins[1].at, p(mm(204.47), mm(6.35)));
}

#[test]
fn a_rectangle_and_a_text_box_turn_about_the_half_grid_point_nearest_their_centre() {
    let s = every_kind();
    let rect = run_on(s.clone(), rotate(&[&s.extras.graphics[0].id], true));
    match rect.extras.graphics[0].shape {
        SchGraphicKind::Rectangle { start, end, .. } => {
            // the 12.7 square about (158_750, 19_050)
            let c = half_grid_of(p(mm(158.75), mm(19.05)));
            assert_eq!(start, rotate_point(p(mm(152.4), mm(12.7)), c, true));
            assert_eq!(end, rotate_point(p(mm(165.1), mm(25.4)), c, true));
        }
        _ => panic!(),
    }
    let tb = run_on(s.clone(), rotate(&[&s.extras.graphics[1].id], true));
    match tb.extras.graphics[1].shape {
        SchGraphicKind::TextBox { angle, .. } => assert_eq!(angle, 90_000, "the text turns with the box"),
        _ => panic!(),
    }
}

#[test]
fn a_power_symbol_turns_a_quarter() {
    let s = every_kind();
    let after = run_on(s, rotate(&["#PWR01"], true));
    assert_eq!(after.power_symbols[0].rot, 90_000);
    assert_eq!(after.power_symbols[0].at, p(mm(101.6), mm(25.4)));
}

// ----------------------------------------------------------------------------------------------------------------------- mirror

#[test]
fn a_lone_symbol_mirrors_in_place() {
    let mut s = section();
    s.symbols.push(sym("R1", p(mm(20.32), mm(25.4))));
    let h = run_on(s.clone(), mirror(&["R1"], false));
    assert_eq!(h.symbols[0].at, p(mm(20.32), mm(25.4)));
    assert!(h.symbols[0].mirrored && !h.symbols[0].mirror_y);
    let v = run_on(s.clone(), mirror(&["R1"], true));
    assert!(!v.symbols[0].mirrored && v.symbols[0].mirror_y);
    assert_eq!(pin_points_of(&v, "R1"), vec![p(mm(20.32), mm(25.4) + mm(3.81)), p(mm(20.32), mm(25.4) - mm(3.81))], "mirrored top to bottom, the pins swap");
}

#[test]
fn mirror_x_then_y_is_a_half_turn_not_one_flag() {
    // KiCad composes the orientations (`SetOrientation` is incremental); the IR's two mirror flags are never both set
    let mut s = section();
    s.symbols.push(sym("R1", p(mm(20.32), mm(25.4))));
    let once = run_on(s, mirror(&["R1"], false));
    let twice = run_on(once, mirror(&["R1"], true));
    assert_eq!((twice.symbols[0].rot, twice.symbols[0].mirrored, twice.symbols[0].mirror_y), (180_000, false, false));
}

#[test]
fn several_items_mirror_about_the_half_grid_point_nearest_the_centre() {
    let mut s = section();
    s.symbols.push(sym("R1", p(mm(20.32), mm(25.4))));
    s.symbols.push(sym("R2", p(mm(45.72), mm(25.4))));
    s.wires.push(wire("wire_a", &[p(mm(20.32), mm(10.16)), p(mm(45.72), mm(10.16))]));
    let after = run_on(s, mirror(&["R1", "R2", "wire_a"], false));
    // centre x = (20_320 + 45_720) / 2 = 33_020 (already a half-grid multiple: 52 * 635)
    assert_eq!(after.symbols[0].at.x, mm(33.02) * 2 - mm(20.32));
    assert_eq!(after.symbols[1].at.x, mm(33.02) * 2 - mm(45.72));
    assert_eq!(wire_pts(&after), vec![vec![p(mm(45.72), mm(10.16)), p(mm(20.32), mm(10.16))]]);
    assert!(after.symbols.iter().all(|x| x.mirrored));
}

#[test]
fn a_label_mirrors_its_spin_not_its_place_when_alone() {
    let mut s = section();
    s.labels.push(label("lbl_a", "A", p(mm(12.7), Nm::ZERO), LabelKind::Local));
    let h = run_on(s.clone(), mirror(&["lbl_a"], false));
    assert_eq!(h.labels[0].at, p(mm(12.7), Nm::ZERO));
    assert_eq!(h.extras.label_spins.get("lbl_a"), Some(&LabelSpin::Left), "right <-> left");
    let v = run_on(s, mirror(&["lbl_a"], true));
    assert_eq!(v.extras.label_spins.get("lbl_a"), Some(&LabelSpin::Right), "top-bottom leaves a horizontal label as it is");
}

#[test]
fn every_kind_mirrors_without_losing_items() {
    let s = every_kind();
    let mut all = ids(&["R1", "wire_a", "lbl_local", "lbl_global", "lbl_hier", "#PWR01", "txt_a", "nc_a", "bent_a", "jct_a", "sln_a", "sheet_a"]);
    all.extend(graphic_ids(&s));
    for vertical in [false, true] {
        let after = run_on(s.clone(), SchMoveCmd::Mirror { ids: all.clone(), vertices: BTreeMap::new(), vertical, about: None, grid: Nm::ZERO });
        assert_eq!(after.symbols.len(), 1);
        assert_eq!(after.wires.len(), 1);
        assert_eq!(after.labels.len(), 3);
        assert_eq!(after.sheets[0].pins.len(), 2);
        // mirroring twice about the same axis puts everything back
        let twice = run_on(after.clone(), SchMoveCmd::Mirror { ids: all.clone(), vertices: BTreeMap::new(), vertical, about: None, grid: Nm::ZERO });
        assert_eq!(twice.sheets[0].at.x.abs() >= Nm::ZERO, true);
    }
}

#[test]
fn a_bus_entry_mirrors_its_size() {
    let s = every_kind();
    let h = run_on(s.clone(), mirror(&["bent_a"], false));
    assert_eq!(h.bus_entries[0].size, p(mm(-2.54), mm(2.54)));
    assert_eq!(h.bus_entries[0].at, s.bus_entries[0].at, "a lone item mirrors about its own anchor");
}

#[test]
fn a_sheet_mirrors_in_place_and_its_pins_change_sides() {
    let s = every_kind();
    let before = s.sheets[0].clone();
    let after = run_on(s, mirror(&["sheet_a"], false));
    let a = &after.sheets[0];
    assert_eq!(a.size, before.size);
    // the pin that was on the left edge is now on the right one
    assert_eq!(a.pins[0].at.x, a.at.x + a.size.0);
    assert_eq!(a.pins[1].at.x, a.at.x);
}

// ------------------------------------------------------------------------------------------------------------ turning while held

#[test]
fn a_symbol_turned_while_it_is_held_turns_about_the_point_it_is_held_at() {
    let mut s = section();
    s.symbols.push(sym("R1", p(mm(20.32), mm(25.4))));
    // picked up, carried 5 cells right, and R pressed with the cursor 3 cells further right
    let cmd = SchMoveCmd::Move { ids: ids(&["R1"]), dx: 5 * G, dy: Nm::ZERO, turns: vec![crate::sch_move::Turn::RotCcw], about: Some(p(mm(30.48), mm(25.4))) };
    let after = run_on(s.clone(), cmd);
    assert_eq!(after.symbols[0].at, rotate_point(p(mm(26.67), mm(25.4)), p(mm(30.48), mm(25.4)), true));
    assert_eq!(after.symbols[0].rot, 90_000);
    // two turns in order: turn, then mirror the held selection
    let two = run_on(s, SchMoveCmd::Move { ids: ids(&["R1"]), dx: Nm::ZERO, dy: Nm::ZERO, turns: vec![crate::sch_move::Turn::RotCcw, crate::sch_move::Turn::MirrorH], about: Some(p(mm(20.32), mm(25.4))) });
    // a quarter turn then a left-right flip is KiCad's "270 degrees, mirrored about x" (`GetOrientation` takes the first of its twelve that matches)
    assert_eq!((two.symbols[0].rot, two.symbols[0].mirrored, two.symbols[0].mirror_y), (270_000, false, true));
}

/// `SCH_EDIT_TOOL::Mirror` never reads the held selection's reference point (only Rotate does): two symbols held with the cursor on the first and
/// mirrored left-right flip about the half-grid point nearest the selection's centre, so they trade places, and not about the cursor.
#[test]
fn a_held_selection_mirrors_about_its_centre_not_the_point_it_is_held_at() {
    let mut s = section();
    s.symbols.push(sym("R1", p(Nm::ZERO, mm(25.4))));
    s.symbols.push(sym("R2", p(mm(25.4), mm(25.4))));
    let cmd = SchMoveCmd::Move { ids: ids(&["R1", "R2"]), dx: Nm::ZERO, dy: Nm::ZERO, turns: vec![crate::sch_move::Turn::MirrorH], about: Some(p(Nm::ZERO, mm(25.4))) };
    let held = run_on(s.clone(), cmd);
    let still = run_on(s, mirror(&["R1", "R2"], false));
    let at = |sec: &eda_model::ir::SchematicSection| sec.symbols.iter().map(|x| (x.id.clone(), x.at)).collect::<Vec<_>>();
    assert_eq!(at(&held), at(&still), "the same mirror whether held or not");
    let r2 = held.symbols.iter().find(|x| x.id == "R2").unwrap();
    assert!(r2.at.x < mm(25.4), "R2 crossed the centre: {:?}", r2.at);
}

#[test]
fn a_drag_turned_while_it_is_held_turns_the_wires_it_brought_along() {
    let mut s = section();
    s.symbols.push(sym("R1", p(mm(20.32), mm(45.72))));
    s.symbols.push(sym("R2", p(mm(20.32), mm(10.16))));
    s.wires.push(wire("wire_a", &[p(mm(20.32), mm(41.91)), p(mm(20.32), mm(13.97))]));
    // R1 dragged straight down (the wire only gets longer), then turned about its own place
    let cmd = SchMoveCmd::Drag { ids: ids(&["R1"]), vertices: BTreeMap::new(), dx: Nm::ZERO, dy: 2 * G, ortho: true, grid: Nm::ZERO, turns: vec![crate::sch_move::Turn::RotCcw], about: Some(p(mm(20.32), mm(45.72) + 2 * G)) };
    let after = run_on(s, cmd);
    let r1 = after.symbols.iter().find(|x| x.id == "R1").unwrap();
    assert_eq!((r1.at, r1.rot), (p(mm(20.32), mm(45.72) + 2 * G), 90_000));
    // the wire's end on R1's pin 1 turned with the symbol: pin 1 was 3 cells above its origin, a quarter turn puts it 3 cells to the left
    let pin1 = pin_points_of(&after, "R1")[0];
    assert_eq!(pin1, p(mm(20.32) - mm(3.81), mm(45.72) + 2 * G));
    assert!(after.wires.iter().any(|w| w.pts.contains(&pin1)), "a wire still ends on the pin: {:?}", after.wires);
}

// ------------------------------------------------------------------------------------------------------------------ align to grid

#[test]
fn align_to_grid_puts_a_symbol_pins_on_the_grid_and_takes_its_wire_along() {
    let mut s = section();
    // R1 is half a grid cell off in x: its pins are off the grid
    s.symbols.push(sym("R1", p(mm(20.32) + mm(0.3), mm(25.4))));
    s.wires.push(wire("wire_a", &[p(mm(20.32) + mm(0.3), mm(21.59)), p(mm(20.32) + mm(0.3), mm(10.16))]));
    let after = run_on(s, SchMoveCmd::AlignToGrid { ids: ids(&["R1"]), grid: Nm::ZERO });
    assert_eq!(after.symbols[0].at.x, mm(20.32));
    assert!(pin_points_of(&after, "R1").iter().all(|q| q.x % G == Nm::ZERO && q.y % G == Nm::ZERO));
    // the end of the wire on the pin goes with it; the far end was not selected and stays
    assert_eq!(wire_pts(&after), vec![vec![p(mm(20.32), mm(21.59)), p(mm(20.32) + mm(0.3), mm(10.16))]]);
}

#[test]
fn align_moves_each_item_by_its_offset_keeping_pins_on_the_grid() {
    let mut s = section();
    s.symbols.push(sym("R1", p(mm(20.32), mm(25.4))));
    s.symbols.push(sym("R2", p(mm(40.64), mm(35.56))));
    // the caller measured boxes and wants R2 up by an off-grid 3.3 mm to line up with R1
    let after = run_on(s, SchMoveCmd::Align { moves: vec![AlignMove { id: "R2".into(), dx: Nm::ZERO, dy: -mm(10.16) - mm(0.33) }], grid: Nm::ZERO });
    let r2 = after.symbols.iter().find(|x| x.id == "R2").unwrap();
    assert_eq!(r2.at.y % G, Nm::ZERO, "the offset is snapped to the grid: {:?}", r2.at);
}

// -------------------------------------------------------------------------------------------------------------------------- nets

/// The nets a sheet traces to: (pins, name when driven), sorted.
fn traced(sch: &eda_model::ir::SchematicSection) -> Vec<(Vec<String>, Option<String>)> {
    let m = ConstraintModel::default();
    let scene = Scene::new(sch, &m);
    let mut pins = BTreeMap::new();
    for (i, s) in sch.symbols.iter().enumerate() {
        for (n, at) in scene.pins_of(i) {
            pins.insert(format!("{}.{}", s.id, n), at);
        }
    }
    let mut copy = sch.clone();
    let mut screens = vec![eda_engine::nets::ScreenIn { path: Vec::new(), sch: &mut copy, file: String::new(), pins }];
    let naming = eda_engine::nets::Naming::default();
    let mut out: Vec<(Vec<String>, Option<String>)> = eda_engine::nets::trace_nets_with(&mut screens, &naming).into_iter().map(|t| (t.pins.clone(), t.driven.then(|| t.name.clone()))).collect();
    out.sort();
    out
}

/// A little circuit: R1 - R2 in series, a label on the wire between them, a power symbol on R2's other pin, a junction with a branch.
fn circuit() -> eda_model::ir::SchematicSection {
    let mut s = section();
    s.symbols.push(sym("R1", p(mm(20.32), mm(25.4))));
    s.symbols.push(sym("R2", p(mm(20.32), mm(45.72))));
    // R1 pin 2 (20_320, 29_210) to R2 pin 1 (20_320, 41_910)
    s.wires.push(wire("wire_a", &[p(mm(20.32), mm(29.21)), p(mm(20.32), mm(41.91))]));
    // a branch off the middle of that wire to a label
    s.wires.push(wire("wire_b", &[p(mm(20.32), mm(35.56)), p(mm(40.64), mm(35.56))]));
    s.junctions.push(Junction { id: "jct_a".into(), at: p(mm(20.32), mm(35.56)) });
    s.labels.push(label("lbl_a", "MID", p(mm(40.64), mm(35.56)), LabelKind::Local));
    // a supply on R1's top pin and a ground on R2's bottom pin
    s.power_symbols.push(PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: p(mm(20.32), mm(49.53)), rot: 0, net: "GND".into(), pin: String::new() });
    s.wires.push(wire("wire_c", &[p(mm(20.32), mm(21.59)), p(mm(20.32), mm(12.7))]));
    s.assign_missing_ids();
    s
}

#[test]
fn nets_do_not_change_when_a_circuit_moves_rigidly_with_its_wires() {
    let s = circuit();
    let before = traced(&s);
    assert!(before.iter().any(|(pins, _)| pins.contains(&"R1.2".to_string()) && pins.contains(&"R2.1".to_string())), "sanity: R1.2 and R2.1 are one net: {before:?}");
    let everything: Vec<&str> = vec!["R1", "R2", "wire_a", "wire_b", "wire_c", "jct_a", "lbl_a", "#PWR01"];
    for cmd in [
        mv(&everything, 5 * G, -3 * G),
        drag(&everything, 5 * G, -3 * G),
        rotate(&everything, true),
        rotate(&everything, false),
        mirror(&everything, false),
        mirror(&everything, true),
    ] {
        let after = run_on(s.clone(), cmd.clone());
        assert_eq!(traced(&after), before, "{cmd:?}");
    }
}

#[test]
fn dragging_a_symbol_keeps_every_net_and_its_pins() {
    let s = circuit();
    let before = traced(&s);
    for dir in [(5 * G, Nm::ZERO), (-5 * G, Nm::ZERO), (Nm::ZERO, 4 * G), (Nm::ZERO, -4 * G), (3 * G, 3 * G)] {
        for who in ["R1", "R2"] {
            let after = run_on(s.clone(), drag(&[who], dir.0, dir.1));
            assert_eq!(traced(&after), before, "dragging {who} by {dir:?}");
        }
    }
}

#[test]
fn dragging_the_label_or_the_branch_keeps_the_nets() {
    let s = circuit();
    let before = traced(&s);
    for who in [vec!["lbl_a"], vec!["wire_b"], vec!["jct_a"], vec!["#PWR01"]] {
        let after = run_on(s.clone(), drag(&who, Nm::ZERO, 2 * G));
        assert_eq!(traced(&after), before, "dragging {who:?}");
    }
}

#[test]
fn a_selection_of_symbols_only_is_layout_and_the_rest_edits_connectivity() {
    assert!(!mv(&["R1", "U2"], G, Nm::ZERO).edits_connectivity());
    assert!(!rotate(&["R1", "txt_a"], true).edits_connectivity());
    assert!(mv(&["R1", "wire_a"], G, Nm::ZERO).edits_connectivity());
    assert!(drag(&["lbl_a"], G, Nm::ZERO).edits_connectivity());
    assert!(mv(&["#PWR01"], G, Nm::ZERO).edits_connectivity());
    assert!(Cmd::SchMove(mv(&["R1"], G, Nm::ZERO)).domain() == Domain::Schematic);
}

#[test]
fn the_verbs_read_back_from_the_json_the_studio_sends() {
    let cmd: Cmd = serde_json::from_str(r#"{"op":"sch_move","verb":"drag","ids":["R1","wire_a"],"dx":2540000,"dy":-1270000,"vertices":{"wire_a":[0]}}"#).unwrap();
    match cmd {
        Cmd::SchMove(SchMoveCmd::Drag { ids, vertices, dx, dy, ortho, grid, .. }) => {
            assert_eq!(ids, vec!["R1", "wire_a"]);
            assert_eq!(vertices["wire_a"], vec![0]);
            assert_eq!((dx, dy, ortho, grid), (mm(2.54), mm(-1.27), true, Nm::ZERO));
        }
        other => panic!("{other:?}"),
    }
    let cmd: Cmd = serde_json::from_str(r#"{"op":"sch_move","verb":"rotate","ids":["R1"]}"#).unwrap();
    assert!(matches!(cmd, Cmd::SchMove(SchMoveCmd::Rotate { ccw: true, about: None, .. })));
    let cmd: Cmd = serde_json::from_str(r#"{"op":"sch_move","verb":"mirror","ids":["R1"],"vertical":true}"#).unwrap();
    assert!(matches!(cmd, Cmd::SchMove(SchMoveCmd::Mirror { vertical: true, .. })));
}

#[test]
fn a_move_on_a_sub_sheet_runs_there_and_leaves_the_root_alone() {
    let m = ConstraintModel::default();
    let mut design = empty_design();
    let mut root = section();
    root.symbols.push(sym("R1", p(mm(20.32), mm(25.4))));
    root.sheets.push(SheetInstance { id: "sheet_a".into(), name: "Sub".into(), file: "sub.kicad_sch".into(), at: p(mm(50.8), mm(12.7)), size: (mm(25.4), mm(12.7)), pins: Vec::new(), page: String::new() });
    let mut child = section();
    child.symbols.push(sym("R9", p(mm(10.16), mm(10.16))));
    design.schematic = Some(root);
    design.sheet_contents = Some([("sub.kicad_sch".to_string(), child)].into_iter().collect());
    let mut b = Board::new(design, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::OnSheet { sheet: "sheet_a".into(), cmd: Box::new(Cmd::SchMove(mv(&["R9"], G, Nm::ZERO))) }).unwrap();
    let d = b.design();
    assert_eq!(d.schematic.as_ref().unwrap().symbols[0].at, p(mm(20.32), mm(25.4)));
    assert_eq!(d.sheet_contents.as_ref().unwrap()["sub.kicad_sch"].symbols[0].at, p(mm(10.16) + G, mm(10.16)));
    // the verb ids are of the sheet in view: R1 is not on it
    let e = b.apply(&Cmd::OnSheet { sheet: "sheet_a".into(), cmd: Box::new(Cmd::SchMove(mv(&["R1"], G, Nm::ZERO))) }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_item");
}

// ------------------------------------------------------------------------------------------------------------------------- images

fn image_section() -> eda_model::ir::SchematicSection {
    let mut s = section();
    let data = eda_model::image_info::base64_encode(&eda_model::image_info::solid_png(60, 40, [3, 3, 3], 254));
    s.extras.graphics.push(SchGraphic::new(SchGraphicKind::Image { at: p(mm(101.6), mm(63.5)), scale_ppm: 1_000_000, data }));
    s.assign_missing_ids();
    s
}

fn image_at(s: &eda_model::ir::SchematicSection) -> Point {
    match &s.extras.graphics[0].shape {
        SchGraphicKind::Image { at, .. } => *at,
        other => panic!("{other:?}"),
    }
}

/// `SCH_BITMAP::Move` shifts the centre; a turn about the image's own centre leaves the centre (the pixels are not turned here: GAPS.md item 24), and a turn about another point
/// takes the centre round it.
#[test]
fn an_image_moves_by_its_centre_and_turns_about_its_own() {
    let s = image_section();
    let id = s.extras.graphics[0].id.clone();
    let moved = run_on(s.clone(), SchMoveCmd::Move { ids: ids(&[&id]), dx: 2 * G, dy: -G, turns: Vec::new(), about: None });
    assert_eq!(image_at(&moved), p(mm(101.6) + 2 * G, mm(63.5) - G));
    let turned = run_on(s.clone(), rotate(&[&id], true));
    assert_eq!(image_at(&turned), p(mm(101.6), mm(63.5)), "about its own centre");
    // the pixels and the scale stay
    assert_eq!(turned.extras.graphics[0].shape, s.extras.graphics[0].shape);
}

// ------------------------------------------------------------------------------------------------------------------- a wire drawn

fn draw(sch: eda_model::ir::SchematicSection, pts: &[Point]) -> eda_model::ir::SchematicSection {
    let m = ConstraintModel::default();
    let mut design = empty_design();
    design.schematic = Some(sch);
    let mut b = Board::new(design, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::AddWire { pts: pts.to_vec(), bus: false }).unwrap_or_else(|e| panic!("{e:?}"));
    b.design().schematic.clone().unwrap()
}

/// `SCH_LINE_WIRE_BUS_TOOL::finishSegments`: the ends of a new wire, and the points of the sheet that lie on it, get the junction
/// `IsExplicitJunctionNeeded` asks for (`AddJunction`, which breaks the wire passing through). A wire ending on another's middle is a T: a dot there.
#[test]
fn a_wire_drawn_to_the_middle_of_another_gets_a_junction_there() {
    let mut s = section();
    s.wires.push(wire("w1", &[p(Nm::ZERO, 10_000 * G), p(12_000 * G, 10_000 * G)]));
    let out = draw(s, &[p(6 * G, 16_000 * G), p(6 * G, 10_000 * G)]);
    assert_eq!(out.junctions.iter().map(|j| j.at).collect::<Vec<_>>(), vec![p(6 * G, 10_000 * G)], "one junction, at the T");
    // The wire passing through is broken at the dot, as `AddJunction` -> `BreakSegments` does: here a wire is a polyline, so it keeps its id with a
    // vertex at the dot (`Scene::finish` joins a wire's pieces that still meet end to end).
    let w1 = out.wires.iter().find(|w| w.id == "w1").unwrap();
    assert_eq!(w1.pts, vec![p(Nm::ZERO, 10_000 * G), p(6 * G, 10_000 * G), p(12_000 * G, 10_000 * G)]);
    assert!(out.wires.iter().any(|w| w.pts == vec![p(6 * G, 16_000 * G), p(6 * G, 10_000 * G)]), "the new wire as drawn");
    // A new wire crossing an old one in the middle of both is not joined (`aBreakCrossings` is for an explicit J only), and one drawn alone gets nothing.
    let mut s = section();
    s.wires.push(wire("w1", &[p(Nm::ZERO, 10_000 * G), p(12_000 * G, 10_000 * G)]));
    let out = draw(s, &[p(6 * G, 4 * G), p(6 * G, 16_000 * G)]);
    assert!(out.junctions.is_empty(), "a crossing is not a junction: {:?}", out.junctions);
    assert!(draw(section(), &[p(Nm::ZERO, Nm::ZERO), p(5 * G, Nm::ZERO)]).junctions.is_empty());
}

/// On a sheet this project drew (a symbol placed by the corner of its box), the editor's pins of a turned or mirrored symbol (`Scene::pins_of`: KiCad's
/// `TRANSFORM` of the orientation) are where the writer bakes them and the tracer finds them (`eda_engine::placed::pin_points_in`), in every one of
/// KiCad's orientations: what the user wires to is what kicad-cli connects.
#[test]
fn the_editor_s_pins_of_a_turned_symbol_are_the_writer_s_in_every_orientation() {
    use eda_model::{Part, Pin, PinKind};
    let part = |r: &str, n: usize| Part { reference: r.into(), mpn: None, lcsc: None, value: Some("x".into()), package: None, footprint: None, symbol: None, datasheet: None, pins: (1..=n).map(|i| Pin { number: i.to_string(), name: None, kind: PinKind::Passive }).collect(), body_nm: None, edge: None };
    let mut m = ConstraintModel { parts: vec![part("R1", 2000), part("U1", 6000)], ..Default::default() };
    m.parts[0].symbol = Some("Device:R".into());
    let mut bad = Vec::new();
    for (id, lib_id) in [("R1", "Device:R"), ("U1", "")] {
        for rot in [0, 90_000, 180_000, 270_000] {
            for (mirrored, mirror_y) in [(false, false), (true, false), (false, true)] {
                let mut s = eda_model::ir::SchematicSection::default();
                let mut x = sym(id, p(mm(50.8), mm(50.8)));
                (x.lib_id, x.rot, x.mirrored, x.mirror_y) = (lib_id.into(), rot, mirrored, mirror_y);
                s.symbols.push(x.clone());
                let scene = Scene::new(&s, &m);
                let mut editor = scene.pins_of(0);
                editor.sort();
                let pt = m.part(id).unwrap();
                let resolved = m.real_symbol_of_instance(&s, &x, pt);
                let mut writer = eda_engine::placed::pin_points_in(&s, &x, pt, resolved.as_ref());
                writer.sort();
                if editor != writer {
                    bad.push(format!("{id} rot {rot} mirrored {mirrored} mirror_y {mirror_y}: editor {editor:?} writer {writer:?}"));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// r2 C-H2 (`SCH_LINE_WIRE_BUS_TOOL::finishSegments`: `CleanUp`, then `TrimWire` between the two connection points of every part that has no more than
/// two): a wire drawn straight through R1's pins (20.32, 21.59) and (20.32, 29.21) is cut between them, so R1 is in series on it, not shorted by it, and
/// each pin is a plain wire end (two exit angles: no junction).
#[test]
fn a_wire_drawn_through_a_resistor_is_cut_between_its_pins() {
    let mut s = section();
    s.symbols.push(sym("R1", p(mm(20.32), mm(25.4))));
    let out = draw(s, &[p(mm(20.32), mm(15.0)), p(mm(20.32), mm(35.0))]);
    assert!(out.junctions.is_empty(), "no junction at a pin that ends a wire: {:?}", out.junctions);
    let mut runs: Vec<Vec<Point>> = out.wires.iter().map(|w| w.pts.clone()).collect();
    runs.sort();
    assert_eq!(runs, vec![vec![p(mm(20.32), mm(15.0)), p(mm(20.32), mm(21.59))], vec![p(mm(20.32), mm(29.21)), p(mm(20.32), mm(35.0))]], "the piece between the pins is gone");
}

/// r2 C-H3 (`SCH_DRAWING_TOOLS::PlaceSymbol`: `TrimOverLappingWires`, `AddJunctionsIfNeeded` on the placing click): a resistor placed on a wire cuts it
/// between its pins.
#[test]
fn placing_a_resistor_on_a_wire_cuts_it_between_the_pins() {
    let mut s = section();
    s.wires.push(wire("w1", &[p(mm(20.32), mm(10.0)), p(mm(20.32), mm(40.0))]));
    let m = ConstraintModel::default();
    let mut design = empty_design();
    design.schematic = Some(s);
    let mut b = Board::new(design, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::AddSymbol { id: "R1".into(), lib_id: "Device:R".into(), at: p(mm(20.32), mm(25.4)), rot_millideg: 0, value: "10k".into(), footprint: String::new(), unit: 1 }).unwrap();
    let out = b.design().schematic.clone().unwrap();
    let mut runs: Vec<Vec<Point>> = out.wires.iter().map(|w| w.pts.clone()).collect();
    runs.sort();
    assert_eq!(runs, vec![vec![p(mm(20.32), mm(10.0)), p(mm(20.32), mm(21.59))], vec![p(mm(20.32), mm(29.21)), p(mm(20.32), mm(40.0))]]);
    assert!(out.junctions.is_empty(), "{:?}", out.junctions);
}
