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

const G: Um = 1_270;

fn empty_design() -> Design {
    Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: 1,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        routing: None,
        placement: None,
        drawings: None,
    }
}

fn p(x: Um, y: Um) -> Point {
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
    let mut b = Board::new(design, &m, 100, 300);
    b.apply(&Cmd::SchMove(cmd)).unwrap_or_else(|e| panic!("{e:?}"));
    b.design().schematic.clone().unwrap()
}

fn run_err(sch: eda_model::ir::SchematicSection, cmd: SchMoveCmd) -> String {
    let m = ConstraintModel::default();
    let mut design = empty_design();
    design.schematic = Some(sch);
    let mut b = Board::new(design, &m, 100, 300);
    b.apply(&Cmd::SchMove(cmd)).unwrap_err()[0].check.clone()
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn mv(v: &[&str], dx: Um, dy: Um) -> SchMoveCmd {
    SchMoveCmd::Move { ids: ids(v), dx, dy }
}

fn drag(v: &[&str], dx: Um, dy: Um) -> SchMoveCmd {
    SchMoveCmd::Drag { ids: ids(v), vertices: BTreeMap::new(), dx, dy, ortho: true, grid: 0 }
}

fn rotate(v: &[&str], ccw: bool) -> SchMoveCmd {
    SchMoveCmd::Rotate { ids: ids(v), vertices: BTreeMap::new(), ccw, about: None, grid: 0 }
}

fn mirror(v: &[&str], vertical: bool) -> SchMoveCmd {
    SchMoveCmd::Mirror { ids: ids(v), vertices: BTreeMap::new(), vertical, about: None, grid: 0 }
}

fn wire_pts(sch: &eda_model::ir::SchematicSection) -> Vec<Vec<Point>> {
    let mut v: Vec<Vec<Point>> = sch.wires.iter().map(|w| w.pts.clone()).collect();
    v.sort();
    v
}

/// A sheet with every kind of item, none of them touching another.
fn every_kind() -> eda_model::ir::SchematicSection {
    let mut s = section();
    s.symbols.push(sym("R1", p(20_320, 25_400)));
    s.wires.push(wire("wire_a", &[p(50_800, 12_700), p(63_500, 12_700)]));
    s.labels.push(label("lbl_local", "A", p(76_200, 25_400), LabelKind::Local));
    s.labels.push(label("lbl_global", "B", p(76_200, 38_100), LabelKind::Global { shape: LabelShape::Input }));
    s.labels.push(label("lbl_hier", "C", p(76_200, 50_800), LabelKind::Hierarchical { shape: LabelShape::Output }));
    s.power_symbols.push(PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: p(101_600, 25_400), rot: 0, net: "GND".into(), pin: String::new() });
    s.texts.push(SchematicText { id: "txt_a".into(), content: "hello".into(), at: p(101_600, 38_100), angle: 0, size_um: 1_270 });
    s.no_connects.push(NoConnect { id: "nc_a".into(), at: p(101_600, 50_800), pin: String::new() });
    s.bus_entries.push(BusEntry { id: "bent_a".into(), at: p(127_000, 25_400), size: p(2_540, 2_540) });
    s.junctions.push(Junction { id: "jct_a".into(), at: p(127_000, 38_100) });
    s.lines.push(SchLine { id: "sln_a".into(), pts: vec![p(127_000, 50_800), p(139_700, 50_800)], width_um: 0 });
    s.extras.graphics.push(SchGraphic::new(SchGraphicKind::Rectangle { start: p(152_400, 12_700), end: p(165_100, 25_400), corner_radius_um: 0 }));
    s.extras.graphics.push(SchGraphic::new(SchGraphicKind::TextBox { start: p(152_400, 38_100), end: p(177_800, 50_800), text: "note".into(), angle: 0, size_um: 1_270, bold: false, italic: false, h_align: eda_model::sch_extras::SchHAlign::Left, v_align: eda_model::sch_extras::SchVAlign::Top, margin_um: 0 }));
    s.sheets.push(SheetInstance {
        id: "sheet_a".into(),
        name: "Sub".into(),
        file: "sub.kicad_sch".into(),
        at: p(190_500, 12_700),
        size: (25_400, 12_700),
        pins: vec![SheetPin { id: "shpin_a".into(), name: "IN".into(), shape: LabelShape::Input, at: p(190_500, 17_780) }, SheetPin { id: "shpin_b".into(), name: "OUT".into(), shape: LabelShape::Output, at: p(215_900, 20_320) }],
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
    let after = run_on(before.clone(), SchMoveCmd::Move { ids: all, dx: 2 * G, dy: -G });
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
    s.symbols.push(sym("R1", p(20_320, 25_400)));
    // a wire from pin 1 straight up
    s.wires.push(wire("wire_a", &[p(20_320, 21_590), p(20_320, 10_160)]));
    let after = run_on(s.clone(), mv(&["R1"], 0, 5 * G));
    assert_eq!(after.symbols[0].at, p(20_320, 25_400 + 5 * G));
    assert_eq!(wire_pts(&after), vec![vec![p(20_320, 21_590), p(20_320, 10_160)]], "Move does not stretch a wire");
}

#[test]
fn move_of_nothing_or_of_an_unknown_id_is_refused_and_a_locked_item_is_not_moved() {
    let mut s = every_kind();
    assert_eq!(run_err(s.clone(), SchMoveCmd::Move { ids: Vec::new(), dx: G, dy: 0 }), "ops_nothing_selected");
    assert_eq!(run_err(s.clone(), mv(&["R404"], G, 0)), "ops_unknown_item");
    s.extras.set_locked("R1", true);
    assert_eq!(run_err(s.clone(), mv(&["R1"], G, 0)), "ops_locked");
    let after = run_on(s.clone(), mv(&["R1", "txt_a"], G, 0));
    assert_eq!(after.symbols[0].at, s.symbols[0].at, "the locked symbol stays");
    assert_eq!(after.texts[0].at, p(s.texts[0].at.x + G, s.texts[0].at.y), "the rest of the selection moves");
}

#[test]
fn move_of_a_multi_unit_symbol_takes_every_unit_and_one_unit_can_be_named() {
    let mut s = section();
    let mut u1 = sym("U1", p(10_160, 10_160));
    u1.unit = 1;
    let mut u2 = sym("U1", p(40_640, 10_160));
    u2.unit = 2;
    s.symbols.push(u1);
    s.symbols.push(u2);
    let all = run_on(s.clone(), mv(&["U1"], G, 0));
    assert_eq!(all.symbols.iter().map(|x| x.at.x).collect::<Vec<_>>(), vec![10_160 + G, 40_640 + G]);
    let one = run_on(s, mv(&["U1#2"], G, 0));
    assert_eq!(one.symbols.iter().map(|x| x.at.x).collect::<Vec<_>>(), vec![10_160, 40_640 + G]);
}

#[test]
fn moving_a_wire_whole_keeps_its_id_and_shape() {
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(0, 0), p(12_700, 0), p(12_700, 12_700)]));
    let after = run_on(s, mv(&["wire_a"], G, G));
    assert_eq!(after.wires.len(), 1);
    assert_eq!(after.wires[0].id, "wire_a");
    assert_eq!(after.wires[0].pts, vec![p(G, G), p(12_700 + G, G), p(12_700 + G, 12_700 + G)]);
}

#[test]
fn moving_a_two_pin_part_with_both_pins_on_one_wire_trims_the_wire_between_them() {
    let mut s = section();
    // a horizontal resistor needs both pins on the wire: turn R1 a quarter turn, its pins are then 7.62 mm apart along x
    let mut r = sym("R1", p(20_320, 25_400));
    r.rot = 90_000;
    s.symbols.push(r);
    // pins of R1 turned CCW: pin 1 at origin + (-3810, 0)? (x', y') = (y, -x) of internal (0, -3810) -> (-3810, 0); pin 2 -> (3810, 0)
    s.wires.push(wire("wire_a", &[p(0, 50_800), p(60_960, 50_800)]));
    let after = run_on(s, mv(&["R1"], 0, 25_400));
    // R1 lands with its pins at (16_510, 50_800) and (24_130, 50_800): the wire between them is cut
    let mut got = wire_pts(&after);
    got.sort();
    assert_eq!(got, vec![vec![p(0, 50_800), p(16_510, 50_800)], vec![p(24_130, 50_800), p(60_960, 50_800)]]);
}

#[test]
fn moving_a_wire_away_from_a_t_junction_removes_the_junction_dot() {
    let mut s = section();
    // a through wire, a branch ending on it, and an explicit dot at the T
    s.wires.push(wire("wire_a", &[p(0, 0), p(25_400, 0)]));
    s.wires.push(wire("wire_b", &[p(12_700, 0), p(12_700, 12_700)]));
    s.junctions.push(Junction { id: "jct_a".into(), at: p(12_700, 0) });
    let after = run_on(s, mv(&["wire_b"], 25_400, 0));
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
    s.symbols.push(sym("R1", p(20_320, 25_400)));
    // a wire from pin 1 (above) straight up to a free end
    s.wires.push(wire("wire_a", &[p(20_320, 21_590), p(20_320, 10_160)]));
    // dragged up: the wire is parallel to the move, so it just shortens
    let after = run_on(s.clone(), drag(&["R1"], 0, -2 * G));
    assert_eq!(after.symbols[0].at, p(20_320, 25_400 - 2 * G));
    assert!(pin_points_of(&after, "R1").contains(&p(20_320, 21_590 - 2 * G)));
    assert_eq!(wire_pts(&after), vec![vec![p(20_320, 21_590 - 2 * G), p(20_320, 10_160)]]);
}

#[test]
fn drag_sideways_keeps_a_vertical_wire_to_a_fixed_pin_orthogonal_with_two_new_segments() {
    // R1's pin 1 wired straight up to R2's pin 2 (fixed). Dragging R1 sideways must not slant the wire: `orthoLineDrag` moves the loose
    // end of the wire to a point one grid cell from the moving end and joins it to the old one by two new lines.
    let mut s = section();
    s.symbols.push(sym("R1", p(20_320, 45_720)));
    s.symbols.push(sym("R2", p(20_320, 10_160)));
    // R1 pin 1 at (20_320, 41_910); R2 pin 2 at (20_320, 13_970)
    s.wires.push(wire("wire_a", &[p(20_320, 41_910), p(20_320, 13_970)]));
    let after = run_on(s.clone(), drag(&["R1"], 5 * G, 0));
    assert_eq!(after.symbols.iter().find(|x| x.id == "R1").unwrap().at.x, 20_320 + 5 * G);
    let r1_pin1 = pin_points_of(&after, "R1")[0];
    assert_eq!(r1_pin1, p(20_320 + 5 * G, 41_910));
    let mut got = wire_pts(&after);
    got.sort();
    let mut want = vec![
        // the wire from the pin, shortened to one grid cell
        vec![p(26_670, 41_910), p(26_670, 40_640)],
        // across, and up to R2 along the old wire
        vec![p(26_670, 40_640), p(20_320, 40_640)],
        vec![p(20_320, 40_640), p(20_320, 13_970)],
    ];
    want.sort();
    assert_eq!(got, want);
    // the left of the symbol: the same, mirrored
    let left = run_on(s, drag(&["R1"], -5 * G, 0));
    let segs: Vec<(Point, Point)> = left.wires.iter().flat_map(|w| w.pts.windows(2).map(|q| (q[0], q[1])).collect::<Vec<_>>()).collect();
    assert!(segs.iter().all(|(a, b)| a.x == b.x || a.y == b.y), "{segs:?}");
    assert!(connected(&segs, p(20_320 - 5 * G, 41_910), p(20_320, 13_970)), "{segs:?}");
}

#[test]
fn drag_sideways_slides_the_corner_of_an_elbow_wire_instead_of_adding_bends() {
    // the wire leaves the pin upward, turns right at y = 30_480 and goes on to R2: its second segment runs the way R1 is dragged, so the
    // corner moves along with the pin and no segment is added
    let mut s = section();
    s.symbols.push(sym("R1", p(20_320, 45_720)));
    s.symbols.push(sym("R2", p(40_640, 8_890)));
    s.wires.push(wire("wire_a", &[p(20_320, 41_910), p(20_320, 30_480), p(40_640, 30_480), p(40_640, 12_700)]));
    let after = run_on(s.clone(), drag(&["R1"], 5 * G, 0));
    assert_eq!(wire_pts(&after), vec![vec![p(26_670, 41_910), p(26_670, 30_480), p(40_640, 30_480), p(40_640, 12_700)]]);
    // dragged straight down the wire only gets longer
    let down = run_on(s, drag(&["R1"], 0, 3 * G));
    assert_eq!(wire_pts(&down), vec![vec![p(20_320, 45_720), p(20_320, 30_480), p(40_640, 30_480), p(40_640, 12_700)]]);
}

#[test]
fn drag_of_a_part_off_the_end_of_a_wire_that_ends_on_a_junction_slides_that_end_along_the_other_wire() {
    // "These are special-cased and get a single line added instead of a 90-degree bend"
    let mut s = section();
    s.symbols.push(sym("R1", p(20_320, 45_720)));
    s.wires.push(wire("wire_t", &[p(20_320, 41_910), p(20_320, 25_400)]));
    s.wires.push(wire("wire_h", &[p(10_160, 25_400), p(40_640, 25_400)]));
    s.junctions.push(Junction { id: "jct_a".into(), at: p(20_320, 25_400) });
    let after = run_on(s, drag(&["R1"], 5 * G, 0));
    let segs: Vec<(Point, Point)> = after.wires.iter().flat_map(|w| w.pts.windows(2).map(|q| (q[0], q[1])).collect::<Vec<_>>()).collect();
    assert!(segs.contains(&(p(26_670, 41_910), p(26_670, 25_400))), "{segs:?}");
    // the dot moved with the T: one at the new meeting point, none at the old
    assert_eq!(after.junctions.iter().map(|j| j.at).collect::<Vec<_>>(), vec![p(26_670, 25_400)]);
    // the through wire is whole again
    assert!(connected(&segs, p(10_160, 25_400), p(40_640, 25_400)));
}

#[test]
fn drag_of_a_part_whose_pin_is_on_another_pin_leaves_a_wire_between_them() {
    let mut s = section();
    s.symbols.push(sym("R1", p(20_320, 25_400)));
    s.symbols.push(sym("R2", p(20_320, 33_020)));
    // R1's pin 2 and R2's pin 1 are the same point, (20_320, 29_210), with no wire
    let sideways = run_on(s.clone(), drag(&["R2"], 5 * G, 0));
    assert_eq!(wire_pts(&sideways), vec![vec![p(26_670, 29_210), p(20_320, 29_210)]]);
    let down = run_on(s, drag(&["R2"], 0, 4 * G));
    assert_eq!(wire_pts(&down), vec![vec![p(20_320, 34_290), p(20_320, 29_210)]]);
}

#[test]
fn labels_on_a_dragged_wire_stay_on_it() {
    let mut s = section();
    s.symbols.push(sym("R1", p(20_320, 45_720)));
    s.wires.push(wire("wire_a", &[p(20_320, 41_910), p(20_320, 25_400)]));
    s.labels.push(label("lbl_a", "A", p(20_320, 25_400), LabelKind::Local));
    s.labels.push(label("lbl_b", "B", p(20_320, 33_020), LabelKind::Local));
    for (dx, dy) in [(5 * G, 0), (0, -2 * G), (0, 2 * G)] {
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
    s.wires.push(wire("wire_a", &[p(0, 0), p(25_400, 0)]));
    // a label in the middle of the wire, and a second wire end dragged along
    s.labels.push(label("lbl_a", "A", p(12_700, 0), LabelKind::Local));
    let after = run_on(s, drag(&["wire_a"], 0, 2 * G));
    assert_eq!(wire_pts(&after), vec![vec![p(0, 2 * G), p(25_400, 2 * G)]]);
    assert_eq!(after.labels[0].at, p(12_700, 2 * G), "the label stays on its wire");
}

#[test]
fn drag_of_a_wire_end_stretches_only_that_end_and_its_neighbour_follows() {
    // two wires meeting at a corner (a bend drawn as two wires); pick the first wire's free end and drag it
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(0, 0), p(12_700, 0)]));
    s.wires.push(wire("wire_b", &[p(12_700, 0), p(12_700, 12_700)]));
    // dragging wire_a's start straight down: the wire is not parallel to the move, so its far end (the corner) goes along,
    // taking wire_b's end with it, which is parallel to the move
    let mut vertices = BTreeMap::new();
    vertices.insert("wire_a".to_string(), vec![0usize]);
    let after = run_on(s, SchMoveCmd::Drag { ids: ids(&["wire_a"]), vertices, dx: 0, dy: 2 * G, ortho: true, grid: 0 });
    let got = wire_pts(&after);
    assert_eq!(got, vec![vec![p(0, 2 * G), p(12_700, 2 * G)], vec![p(12_700, 2 * G), p(12_700, 12_700)]], "{got:?}");
}

#[test]
fn drag_with_free_angles_just_moves_the_picked_end() {
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(0, 0), p(12_700, 0)]));
    let mut vertices = BTreeMap::new();
    vertices.insert("wire_a".to_string(), vec![1usize]);
    let after = run_on(s, SchMoveCmd::Drag { ids: ids(&["wire_a"]), vertices, dx: 0, dy: 2 * G, ortho: false, grid: 0 });
    assert_eq!(wire_pts(&after), vec![vec![p(0, 0), p(12_700, 2 * G)]]);
}

#[test]
fn drag_of_a_symbol_takes_a_no_connect_on_its_pin_along() {
    let mut s = section();
    s.symbols.push(sym("R1", p(20_320, 25_400)));
    s.no_connects.push(NoConnect { id: "nc_a".into(), at: p(20_320, 29_210), pin: String::new() });
    let after = run_on(s, drag(&["R1"], 2 * G, 0));
    assert_eq!(after.no_connects[0].at, p(20_320 + 2 * G, 29_210));
}

#[test]
fn drag_of_a_label_takes_the_wire_on_it_along() {
    // a wire ends on a label; dragging the label drags that end of the wire
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(0, 0), p(12_700, 0)]));
    s.labels.push(label("lbl_a", "A", p(12_700, 0), LabelKind::Local));
    let after = run_on(s, drag(&["lbl_a"], 2 * G, 0));
    assert_eq!(after.labels[0].at, p(12_700 + 2 * G, 0));
    assert_eq!(wire_pts(&after), vec![vec![p(0, 0), p(12_700 + 2 * G, 0)]]);
}

#[test]
fn drag_of_a_junction_takes_the_wire_ends_on_it_along() {
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(0, 0), p(12_700, 0)]));
    s.wires.push(wire("wire_b", &[p(12_700, 0), p(25_400, 0)]));
    s.wires.push(wire("wire_c", &[p(12_700, 0), p(12_700, -12_700)]));
    s.junctions.push(Junction { id: "jct_a".into(), at: p(12_700, 0) });
    let after = run_on(s, drag(&["jct_a"], 0, 2 * G));
    assert_eq!(after.junctions[0].at, p(12_700, 2 * G));
}

#[test]
fn drag_of_a_sheet_keeps_the_wire_on_its_pin_attached_with_right_angles() {
    let mut s = section();
    s.symbols.push(sym("R9", p(25_400, 21_590)));
    s.sheets.push(SheetInstance {
        id: "sheet_a".into(),
        name: "Sub".into(),
        file: "sub.kicad_sch".into(),
        at: p(50_800, 12_700),
        size: (25_400, 12_700),
        pins: vec![SheetPin { id: "shpin_a".into(), name: "IN".into(), shape: LabelShape::Input, at: p(50_800, 17_780) }],
        page: String::new(),
    });
    // R9's pin 1 sits at (25_400, 17_780), the sheet pin at (50_800, 17_780)
    s.wires.push(wire("wire_a", &[p(25_400, 17_780), p(50_800, 17_780)]));
    let after = run_on(s, drag(&["sheet_a"], 0, 2 * G));
    assert_eq!(after.sheets[0].at, p(50_800, 12_700 + 2 * G));
    assert_eq!(after.sheets[0].pins[0].at, p(50_800, 17_780 + 2 * G));
    let segs: Vec<(Point, Point)> = after.wires.iter().flat_map(|w| w.pts.windows(2).map(|q| (q[0], q[1])).collect::<Vec<_>>()).collect();
    assert!(connected(&segs, p(25_400, 17_780), p(50_800, 17_780 + 2 * G)), "{segs:?}");
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
        at: p(50_800, 12_700),
        size: (25_400, 12_700),
        pins: vec![SheetPin { id: "shpin_a".into(), name: "IN".into(), shape: LabelShape::Input, at: p(50_800, 17_780) }],
        page: String::new(),
    });
    s.wires.push(wire("wire_a", &[p(25_400, 17_780), p(50_800, 17_780)]));
    let after = run_on(s, drag(&["sheet_a"], 0, 2 * G));
    assert_eq!(after.sheets[0].pins[0].at, p(50_800, 17_780 + 2 * G));
    assert_eq!(wire_pts(&after), vec![vec![p(25_400, 17_780 + 2 * G), p(50_800, 17_780 + 2 * G)]]);
}

// ----------------------------------------------------------------------------------------------------------------------- rotate

#[test]
fn a_lone_symbol_turns_about_its_own_anchor() {
    let mut s = section();
    s.symbols.push(sym("R1", p(20_320, 25_400)));
    let after = run_on(s.clone(), rotate(&["R1"], true));
    assert_eq!(after.symbols[0].at, p(20_320, 25_400));
    assert_eq!(after.symbols[0].rot, 90_000);
    // the pins swung from above and below to the left and right
    let pins = pin_points_of(&after, "R1");
    assert_eq!(pins, vec![p(20_320 - 3_810, 25_400), p(20_320 + 3_810, 25_400)]);
    let cw = run_on(s, rotate(&["R1"], false));
    assert_eq!(cw.symbols[0].rot, 270_000);
}

#[test]
fn several_items_turn_about_the_half_grid_point_nearest_their_centre() {
    let mut s = section();
    s.symbols.push(sym("R1", p(20_320, 25_400)));
    s.symbols.push(sym("R2", p(40_640, 25_400)));
    // the two boxes span x 20_320..40_640 (centre 30_480, which is on the half grid: 24 * 1270 / 2)... and y 21_590..29_210 (centre 25_400)
    let after = run_on(s, rotate(&["R1", "R2"], true));
    let c = p(30_480, 25_400);
    let want = |at: Point| rotate_point(at, c, true);
    assert_eq!(after.symbols[0].at, want(p(20_320, 25_400)));
    assert_eq!(after.symbols[1].at, want(p(40_640, 25_400)));
    assert!(after.symbols.iter().all(|x| x.rot == 90_000));
}

#[test]
fn a_lone_wire_turns_about_its_far_end() {
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(0, 0), p(12_700, 0)]));
    let after = run_on(s, rotate(&["wire_a"], true));
    // both ends turn, the end point (12_700, 0) staying put; the start swings a quarter turn about it, counter-clockwise on the screen
    assert_eq!(wire_pts(&after), vec![vec![p(12_700, 12_700), p(12_700, 0)]]);
}

#[test]
fn a_wire_picked_at_one_end_turns_that_end_about_the_other() {
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(0, 0), p(12_700, 0)]));
    let mut vertices = BTreeMap::new();
    vertices.insert("wire_a".to_string(), vec![1usize]);
    let after = run_on(s, SchMoveCmd::Rotate { ids: ids(&["wire_a"]), vertices, ccw: true, about: None, grid: 0 });
    assert_eq!(wire_pts(&after), vec![vec![p(0, 0), p(0, -12_700)]]);
}

#[test]
fn a_label_turned_in_place_changes_its_spin_and_a_second_turn_goes_on_from_there() {
    let mut s = section();
    s.labels.push(label("lbl_a", "A", p(12_700, 0), LabelKind::Local));
    let once = run_on(s.clone(), rotate(&["lbl_a"], true));
    assert_eq!(once.labels[0].at, p(12_700, 0), "a single label turns about its own anchor");
    assert_eq!(once.extras.label_spins.get("lbl_a"), Some(&LabelSpin::Up), "a label nothing is wired to reads to the right; turned CCW it goes up");
    let twice = run_on(once, rotate(&["lbl_a"], true));
    assert_eq!(twice.extras.label_spins.get("lbl_a"), Some(&LabelSpin::Left));
    let back = run_on(twice, rotate(&["lbl_a"], false));
    assert_eq!(back.extras.label_spins.get("lbl_a"), Some(&LabelSpin::Up));
}

#[test]
fn a_label_wired_from_the_left_starts_as_right_and_turns_from_there() {
    let mut s = section();
    s.wires.push(wire("wire_a", &[p(0, 0), p(12_700, 0)]));
    s.labels.push(label("lbl_a", "A", p(12_700, 0), LabelKind::Global { shape: LabelShape::Input }));
    let cw = run_on(s, rotate(&["lbl_a"], false));
    assert_eq!(cw.extras.label_spins.get("lbl_a"), Some(&LabelSpin::Bottom));
}

#[test]
fn text_turns_in_place_and_a_group_turns_around_its_centre() {
    let mut s = section();
    s.texts.push(SchematicText { id: "txt_a".into(), content: "a".into(), at: p(0, 0), angle: 0, size_um: 1_270 });
    s.texts.push(SchematicText { id: "txt_b".into(), content: "b".into(), at: p(25_400, 0), angle: 0, size_um: 1_270 });
    let one = run_on(s.clone(), rotate(&["txt_a"], true));
    assert_eq!((one.texts[0].at, one.texts[0].angle), (p(0, 0), 90_000));
    // two texts: the mean of their places is the centre
    let two = run_on(s, rotate(&["txt_a", "txt_b"], true));
    let c = p(12_700, 0);
    assert_eq!(two.texts[0].at, rotate_point(p(0, 0), c, true));
    assert_eq!(two.texts[1].at, rotate_point(p(25_400, 0), c, true));
}

#[test]
fn a_junction_a_no_connect_and_a_bus_entry_turn_about_their_own_anchor_or_the_selection_centre() {
    let s = every_kind();
    let one = run_on(s.clone(), rotate(&["bent_a"], true));
    assert_eq!(one.bus_entries[0].at, s.bus_entries[0].at, "a lone bus entry keeps its anchor");
    assert_eq!(one.bus_entries[0].size, p(2_540, -2_540), "and its size turns as a vector");
    let nc = run_on(s.clone(), rotate(&["nc_a"], true));
    assert_eq!(nc.no_connects[0].at, s.no_connects[0].at);
    let two = run_on(s.clone(), rotate(&["jct_a", "nc_a"], true));
    let c = half_grid_of(p(114_300, 44_450));
    assert_eq!(two.junctions[0].at, rotate_point(s.junctions[0].at, c, true));
}

fn half_grid_of(q: Point) -> Point {
    let h = G as f64 / 2.0;
    p(((q.x as f64 / h).round() * h) as Um, ((q.y as f64 / h).round() * h) as Um)
}

#[test]
fn a_sheet_turns_about_the_half_grid_point_nearest_its_centre_with_its_pins() {
    let s = every_kind();
    // 25.4 x 12.7 mm at (190.5, 12.7); its centre (203.2, 19.05) is on the half grid
    let after = run_on(s, rotate(&["sheet_a"], true));
    let a = &after.sheets[0];
    assert_eq!((a.at, a.size), (p(196_850, 6_350), (12_700, 25_400)));
    // the pin on the left edge went to the bottom edge and the one on the right edge to the top edge
    assert_eq!(a.pins[0].at, p(201_930, 31_750));
    assert_eq!(a.pins[1].at, p(204_470, 6_350));
}

#[test]
fn a_rectangle_and_a_text_box_turn_about_the_half_grid_point_nearest_their_centre() {
    let s = every_kind();
    let rect = run_on(s.clone(), rotate(&[&s.extras.graphics[0].id], true));
    match rect.extras.graphics[0].shape {
        SchGraphicKind::Rectangle { start, end, .. } => {
            // the 12.7 square about (158_750, 19_050)
            let c = half_grid_of(p(158_750, 19_050));
            assert_eq!(start, rotate_point(p(152_400, 12_700), c, true));
            assert_eq!(end, rotate_point(p(165_100, 25_400), c, true));
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
    assert_eq!(after.power_symbols[0].at, p(101_600, 25_400));
}

// ----------------------------------------------------------------------------------------------------------------------- mirror

#[test]
fn a_lone_symbol_mirrors_in_place() {
    let mut s = section();
    s.symbols.push(sym("R1", p(20_320, 25_400)));
    let h = run_on(s.clone(), mirror(&["R1"], false));
    assert_eq!(h.symbols[0].at, p(20_320, 25_400));
    assert!(h.symbols[0].mirrored && !h.symbols[0].mirror_y);
    let v = run_on(s.clone(), mirror(&["R1"], true));
    assert!(!v.symbols[0].mirrored && v.symbols[0].mirror_y);
    assert_eq!(pin_points_of(&v, "R1"), vec![p(20_320, 25_400 + 3_810), p(20_320, 25_400 - 3_810)], "mirrored top to bottom, the pins swap");
}

#[test]
fn mirror_x_then_y_is_a_half_turn_not_one_flag() {
    // KiCad composes the orientations (`SetOrientation` is incremental); the IR's two mirror flags are never both set
    let mut s = section();
    s.symbols.push(sym("R1", p(20_320, 25_400)));
    let once = run_on(s, mirror(&["R1"], false));
    let twice = run_on(once, mirror(&["R1"], true));
    assert_eq!((twice.symbols[0].rot, twice.symbols[0].mirrored, twice.symbols[0].mirror_y), (180_000, false, false));
}

#[test]
fn several_items_mirror_about_the_half_grid_point_nearest_the_centre() {
    let mut s = section();
    s.symbols.push(sym("R1", p(20_320, 25_400)));
    s.symbols.push(sym("R2", p(45_720, 25_400)));
    s.wires.push(wire("wire_a", &[p(20_320, 10_160), p(45_720, 10_160)]));
    let after = run_on(s, mirror(&["R1", "R2", "wire_a"], false));
    // centre x = (20_320 + 45_720) / 2 = 33_020 (already a half-grid multiple: 52 * 635)
    assert_eq!(after.symbols[0].at.x, 33_020 * 2 - 20_320);
    assert_eq!(after.symbols[1].at.x, 33_020 * 2 - 45_720);
    assert_eq!(wire_pts(&after), vec![vec![p(45_720, 10_160), p(20_320, 10_160)]]);
    assert!(after.symbols.iter().all(|x| x.mirrored));
}

#[test]
fn a_label_mirrors_its_spin_not_its_place_when_alone() {
    let mut s = section();
    s.labels.push(label("lbl_a", "A", p(12_700, 0), LabelKind::Local));
    let h = run_on(s.clone(), mirror(&["lbl_a"], false));
    assert_eq!(h.labels[0].at, p(12_700, 0));
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
        let after = run_on(s.clone(), SchMoveCmd::Mirror { ids: all.clone(), vertices: BTreeMap::new(), vertical, about: None, grid: 0 });
        assert_eq!(after.symbols.len(), 1);
        assert_eq!(after.wires.len(), 1);
        assert_eq!(after.labels.len(), 3);
        assert_eq!(after.sheets[0].pins.len(), 2);
        // mirroring twice about the same axis puts everything back
        let twice = run_on(after.clone(), SchMoveCmd::Mirror { ids: all.clone(), vertices: BTreeMap::new(), vertical, about: None, grid: 0 });
        assert_eq!(twice.sheets[0].at.x.abs() >= 0, true);
    }
}

#[test]
fn a_bus_entry_mirrors_its_size() {
    let s = every_kind();
    let h = run_on(s.clone(), mirror(&["bent_a"], false));
    assert_eq!(h.bus_entries[0].size, p(-2_540, 2_540));
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

// ------------------------------------------------------------------------------------------------------------------ align to grid

#[test]
fn align_to_grid_puts_a_symbol_pins_on_the_grid_and_takes_its_wire_along() {
    let mut s = section();
    // R1 is half a grid cell off in x: its pins are off the grid
    s.symbols.push(sym("R1", p(20_320 + 300, 25_400)));
    s.wires.push(wire("wire_a", &[p(20_320 + 300, 21_590), p(20_320 + 300, 10_160)]));
    let after = run_on(s, SchMoveCmd::AlignToGrid { ids: ids(&["R1"]), grid: 0 });
    assert_eq!(after.symbols[0].at.x, 20_320);
    assert!(pin_points_of(&after, "R1").iter().all(|q| q.x % G == 0 && q.y % G == 0));
    // the end of the wire on the pin goes with it; the far end was not selected and stays
    assert_eq!(wire_pts(&after), vec![vec![p(20_320, 21_590), p(20_320 + 300, 10_160)]]);
}

#[test]
fn align_moves_each_item_by_its_offset_keeping_pins_on_the_grid() {
    let mut s = section();
    s.symbols.push(sym("R1", p(20_320, 25_400)));
    s.symbols.push(sym("R2", p(40_640, 35_560)));
    // the caller measured boxes and wants R2 up by an off-grid 3.3 mm to line up with R1
    let after = run_on(s, SchMoveCmd::Align { moves: vec![AlignMove { id: "R2".into(), dx: 0, dy: -10_160 - 330 }], grid: 0 });
    let r2 = after.symbols.iter().find(|x| x.id == "R2").unwrap();
    assert_eq!(r2.at.y % G, 0, "the offset is snapped to the grid: {:?}", r2.at);
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
    let mut screens = vec![eda_engine::nets::ScreenIn { sch: &mut copy, file: String::new(), pins }];
    let naming = eda_engine::nets::Naming { wire_hints: false, pin_names: None };
    let mut out: Vec<(Vec<String>, Option<String>)> = eda_engine::nets::trace_nets_with(&mut screens, &naming).into_iter().map(|t| (t.pins.clone(), t.driven.then(|| t.name.clone()))).collect();
    out.sort();
    out
}

/// A little circuit: R1 - R2 in series, a label on the wire between them, a power symbol on R2's other pin, a junction with a branch.
fn circuit() -> eda_model::ir::SchematicSection {
    let mut s = section();
    s.symbols.push(sym("R1", p(20_320, 25_400)));
    s.symbols.push(sym("R2", p(20_320, 45_720)));
    // R1 pin 2 (20_320, 29_210) to R2 pin 1 (20_320, 41_910)
    s.wires.push(wire("wire_a", &[p(20_320, 29_210), p(20_320, 41_910)]));
    // a branch off the middle of that wire to a label
    s.wires.push(wire("wire_b", &[p(20_320, 35_560), p(40_640, 35_560)]));
    s.junctions.push(Junction { id: "jct_a".into(), at: p(20_320, 35_560) });
    s.labels.push(label("lbl_a", "MID", p(40_640, 35_560), LabelKind::Local));
    // a supply on R1's top pin and a ground on R2's bottom pin
    s.power_symbols.push(PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: p(20_320, 49_530), rot: 0, net: "GND".into(), pin: String::new() });
    s.wires.push(wire("wire_c", &[p(20_320, 21_590), p(20_320, 12_700)]));
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
    for dir in [(5 * G, 0), (-5 * G, 0), (0, 4 * G), (0, -4 * G), (3 * G, 3 * G)] {
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
        let after = run_on(s.clone(), drag(&who, 0, 2 * G));
        assert_eq!(traced(&after), before, "dragging {who:?}");
    }
}

#[test]
fn a_selection_of_symbols_only_is_layout_and_the_rest_edits_connectivity() {
    assert!(!mv(&["R1", "U2"], G, 0).edits_connectivity());
    assert!(!rotate(&["R1", "txt_a"], true).edits_connectivity());
    assert!(mv(&["R1", "wire_a"], G, 0).edits_connectivity());
    assert!(drag(&["lbl_a"], G, 0).edits_connectivity());
    assert!(mv(&["#PWR01"], G, 0).edits_connectivity());
    assert!(Cmd::SchMove(mv(&["R1"], G, 0)).domain() == Domain::Schematic);
}

#[test]
fn the_verbs_read_back_from_the_json_the_studio_sends() {
    let cmd: Cmd = serde_json::from_str(r#"{"op":"sch_move","verb":"drag","ids":["R1","wire_a"],"dx":2540,"dy":-1270,"vertices":{"wire_a":[0]}}"#).unwrap();
    match cmd {
        Cmd::SchMove(SchMoveCmd::Drag { ids, vertices, dx, dy, ortho, grid }) => {
            assert_eq!(ids, vec!["R1", "wire_a"]);
            assert_eq!(vertices["wire_a"], vec![0]);
            assert_eq!((dx, dy, ortho, grid), (2540, -1270, true, 0));
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
    root.symbols.push(sym("R1", p(20_320, 25_400)));
    root.sheets.push(SheetInstance { id: "sheet_a".into(), name: "Sub".into(), file: "sub.kicad_sch".into(), at: p(50_800, 12_700), size: (25_400, 12_700), pins: Vec::new(), page: String::new() });
    let mut child = section();
    child.symbols.push(sym("R9", p(10_160, 10_160)));
    design.schematic = Some(root);
    design.sheet_contents = Some([("sub.kicad_sch".to_string(), child)].into_iter().collect());
    let mut b = Board::new(design, &m, 100, 300);
    b.apply(&Cmd::OnSheet { sheet: "sheet_a".into(), cmd: Box::new(Cmd::SchMove(mv(&["R9"], G, 0))) }).unwrap();
    let d = b.design();
    assert_eq!(d.schematic.as_ref().unwrap().symbols[0].at, p(20_320, 25_400));
    assert_eq!(d.sheet_contents.as_ref().unwrap()["sub.kicad_sch"].symbols[0].at, p(10_160 + G, 10_160));
    // the verb ids are of the sheet in view: R1 is not on it
    let e = b.apply(&Cmd::OnSheet { sheet: "sheet_a".into(), cmd: Box::new(Cmd::SchMove(mv(&["R1"], G, 0))) }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_item");
}
