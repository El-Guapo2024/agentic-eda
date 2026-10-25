//! Tests for Swap Pins, Swap Pin Labels and Swap Unit Labels (`SchCmd::SwapPins`, `SwapPinLabels`, `SwapUnitLabels`; `sch_pins.rs`).
//!
//! The sheet is a KiCad-style one (`imported_from_kicad`: a symbol sits at its library origin) with `Device:R` resistors: R1 at (20.32, 25.4) mm has pin 1 at
//! (20.32, 21.59) and pin 2 at (20.32, 29.21). A three-pin `Test:TRI` and a two-unit `Test:DUAL` are in the model for the cycles and the units.

use super::*;
use crate::sch_edit::SchCmd;
use crate::sch_scene::Scene;
use eda_model::ir::{LabelKind, NetLabel, PowerSymbol, Provenance, SheetInstance, SymbolInstance, Wire};
use eda_model::symbol::SPoint;
use eda_model::{LibPin, LibSymbol};
use std::collections::BTreeMap;
use eda_model::units::mm;

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

fn sym_of(id: &str, lib_id: &str, unit: u32, at: Point) -> SymbolInstance {
    SymbolInstance { id: id.into(), at, rot: 0, mirrored: false, mirror_y: false, lib_id: lib_id.into(), unit, value: String::new(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false }
}

fn res(id: &str, at: Point) -> SymbolInstance {
    sym_of(id, "Device:R", 1, at)
}

fn wire(id: &str, pts: &[Point]) -> Wire {
    Wire { id: id.into(), net: "N".into(), pins: Vec::new(), pts: pts.to_vec(), bus: false }
}

fn label(id: &str, net: &str, at: Point) -> NetLabel {
    NetLabel { id: id.into(), net: net.into(), at, kind: LabelKind::Local }
}

fn section() -> eda_model::ir::SchematicSection {
    eda_model::ir::SchematicSection { imported_from_kicad: true, ..Default::default() }
}

fn lib_pin(number: &str, x: f64, y: f64, angle: f64, unit: u32) -> LibPin {
    LibPin { number: number.into(), name: format!("P{number}"), electrical_type: "passive".into(), shape: "line".into(), at: SPoint::new(x, y), angle_deg: angle, length_mm: 2.54, unit, hidden: false }
}

fn lib(lib_id: &str, unit_count: u32, pins: Vec<LibPin>) -> LibSymbol {
    LibSymbol {
        lib_id: lib_id.into(),
        graphics: vec![],
        pins,
        power: false,
        in_bom: true,
        on_board: true,
        datasheet: String::new(),
        description: String::new(),
        reference_prefix: "U".into(),
        unit_count,
        pin_names_hidden: false,
        pin_numbers_hidden: false,
        pin_name_offset_mm: 0.508,
        alternate: None,
    }
}

/// A model with a three-pin symbol (pins 1, 2, 3 stacked at 2.54 mm pitch on the left edge) and a two-unit one (two pins per unit).
fn model() -> ConstraintModel {
    let tri = lib("Test:TRI", 1, vec![lib_pin("1", 0.0, 2.54, 0.0, 1), lib_pin("2", 0.0, 0.0, 0.0, 1), lib_pin("3", 0.0, -2.54, 0.0, 1)]);
    let dual = lib("Test:DUAL", 2, vec![lib_pin("1", 0.0, 2.54, 0.0, 1), lib_pin("2", 0.0, -2.54, 0.0, 1), lib_pin("3", 0.0, 2.54, 0.0, 2), lib_pin("4", 0.0, -2.54, 0.0, 2)]);
    ConstraintModel { symbols: vec![tri, dual], ..Default::default() }
}

fn run(sch: eda_model::ir::SchematicSection, cmd: SchCmd) -> Result<eda_model::ir::SchematicSection, Vec<eda_model::CheckResult>> {
    let m = model();
    let mut design = empty_design();
    design.schematic = Some(sch);
    let mut b = Board::new(design, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::SchEdit(cmd))?;
    Ok(b.design().schematic.clone().unwrap())
}

fn ok(sch: eda_model::ir::SchematicSection, cmd: SchCmd) -> eda_model::ir::SchematicSection {
    run(sch, cmd).unwrap_or_else(|e| panic!("{e:?}"))
}

fn err(sch: eda_model::ir::SchematicSection, cmd: SchCmd) -> (String, String) {
    let e = run(sch, cmd).unwrap_err();
    (e[0].check.clone(), e[0].hint.clone().unwrap_or_default())
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// Where each pin of the sheet is: "R1.2" -> point.
fn pin_points(sch: &eda_model::ir::SchematicSection) -> BTreeMap<String, Point> {
    let m = model();
    let scene = Scene::new(sch, &m);
    let mut pins = BTreeMap::new();
    for (i, s) in sch.symbols.iter().enumerate() {
        for (n, at) in scene.pins_of(i) {
            pins.insert(format!("{}.{}", eda_model::ir::field_key(&s.id, s.unit), n), at);
        }
    }
    pins
}

/// The nets a sheet traces to: (pins, name when driven), sorted.
fn traced(sch: &eda_model::ir::SchematicSection) -> Vec<(Vec<String>, Option<String>)> {
    let mut copy = sch.clone();
    let mut screens = vec![eda_engine::nets::ScreenIn { path: Vec::new(), sch: &mut copy, file: String::new(), pins: pin_points(sch) }];
    let naming = eda_engine::nets::Naming::default();
    let mut out: Vec<(Vec<String>, Option<String>)> = eda_engine::nets::trace_nets_with(&mut screens, &naming).into_iter().map(|t| (t.pins.clone(), t.driven.then(|| t.name.clone()))).collect();
    out.sort();
    out
}

fn net_of(nets: &[(Vec<String>, Option<String>)], pin: &str) -> Option<String> {
    nets.iter().find(|(pins, _)| pins.iter().any(|x| x == pin)).and_then(|(_, name)| name.clone())
}

/// R1 with a wire and a label on each pin: pin 1 up to "A", pin 2 down to "B".
fn labelled_resistor() -> eda_model::ir::SchematicSection {
    let mut s = section();
    s.symbols.push(res("R1", p(mm(20.32), mm(25.4))));
    s.wires.push(wire("wire_a", &[p(mm(20.32), mm(21.59)), p(mm(20.32), mm(12.7))]));
    s.wires.push(wire("wire_b", &[p(mm(20.32), mm(29.21)), p(mm(20.32), mm(38.1))]));
    s.labels.push(label("lbl_a", "A", p(mm(20.32), mm(12.7))));
    s.labels.push(label("lbl_b", "B", p(mm(20.32), mm(38.1))));
    s.assign_missing_ids();
    s
}

// ------------------------------------------------------------------------------------------------------------------------- Swap Pins

#[test]
fn swap_pins_trades_the_places_of_the_two_pins_and_the_wires_stay() {
    let s = labelled_resistor();
    let before = pin_points(&s);
    assert_eq!(before["R1.1"], p(mm(20.32), mm(21.59)));
    assert_eq!(before["R1.2"], p(mm(20.32), mm(29.21)));
    let nets = traced(&s);
    assert_eq!(net_of(&nets, "R1.1").as_deref(), Some("/A"));
    assert_eq!(net_of(&nets, "R1.2").as_deref(), Some("/B"));

    let after = ok(s.clone(), SchCmd::SwapPins { pins: ids(&["pin:R1:1", "pin:R1:2"]) });
    let now = pin_points(&after);
    // pin 1 took pin 2's place and the other way round; the symbol and the wires did not move
    assert_eq!(now["R1.1"], before["R1.2"]);
    assert_eq!(now["R1.2"], before["R1.1"]);
    assert_eq!(after.symbols[0].at, s.symbols[0].at);
    assert_eq!(after.wires.iter().map(|w| w.pts.clone()).collect::<Vec<_>>(), s.wires.iter().map(|w| w.pts.clone()).collect::<Vec<_>>());
    // so the pins are on each other's nets: the connection graph reads the new places
    let nets = traced(&after);
    assert_eq!(net_of(&nets, "R1.1").as_deref(), Some("/B"), "{nets:?}");
    assert_eq!(net_of(&nets, "R1.2").as_deref(), Some("/A"), "{nets:?}");
    // the swap is an overlay: both entries, nothing else written
    assert_eq!(after.extras.pin_swaps["R1"], BTreeMap::from([("1".to_string(), "2".to_string()), ("2".to_string(), "1".to_string())]));
}

#[test]
fn a_rename_takes_the_swap_with_the_reference_and_a_delete_drops_it() {
    let m = model();
    let mut design = empty_design();
    design.schematic = Some(ok(labelled_resistor(), SchCmd::SwapPins { pins: ids(&["pin:R1:1", "pin:R1:2"]) }));
    let mut b = Board::new(design, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::RenameSymbol { id: "R1".into(), new_id: "R9".into() }).unwrap();
    let s = b.design().schematic.clone().unwrap();
    assert!(s.extras.pin_swaps.contains_key("R9") && !s.extras.pin_swaps.contains_key("R1"), "{:?}", s.extras.pin_swaps);
    // the pins are still where the swap put them: R9's pin 1 is at the place pin 2 has in the library, on the net B
    let nets = traced(&s);
    assert_eq!(net_of(&nets, "R9.1").as_deref(), Some("/B"), "{nets:?}");
    assert_eq!(net_of(&nets, "R9.2").as_deref(), Some("/A"), "{nets:?}");
    // a symbol that is deleted takes its swap with it, so a new R9 is not born with swapped pins
    b.apply(&Cmd::DeleteSymbol { id: "R9".into(), unit: None }).unwrap();
    assert!(b.design().schematic.as_ref().unwrap().extras.pin_swaps.is_empty(), "{:?}", b.design().schematic.as_ref().unwrap().extras.pin_swaps);
}

#[test]
fn swapping_the_same_two_pins_again_puts_them_back_and_leaves_no_entry() {
    let s = labelled_resistor();
    let once = ok(s.clone(), SchCmd::SwapPins { pins: ids(&["pin:R1:1", "pin:R1:2"]) });
    let twice = ok(once, SchCmd::SwapPins { pins: ids(&["pin:R1:2", "pin:R1:1"]) });
    assert!(twice.extras.pin_swaps.is_empty(), "{:?}", twice.extras.pin_swaps);
    assert_eq!(pin_points(&twice), pin_points(&s));
    assert!(twice.extras.is_empty(), "a symbol with every pin where the library has it keeps nothing");
}

#[test]
fn three_pins_swap_in_a_cycle_in_selection_order() {
    let mut s = section();
    s.symbols.push(sym_of("U1", "Test:TRI", 1, p(mm(20.32), mm(25.4))));
    let before = pin_points(&s);
    // pin 1 and 2, then 2 and 3 (`for i in 0..n-1: SwapPinGeometry( sorted[i], sorted[i+1] )`): 1 takes 2's place, 2 takes 3's, 3 takes 1's
    let after = ok(s, SchCmd::SwapPins { pins: ids(&["pin:U1:1", "pin:U1:2", "pin:U1:3"]) });
    let now = pin_points(&after);
    assert_eq!(now["U1.1"], before["U1.2"]);
    assert_eq!(now["U1.2"], before["U1.3"]);
    assert_eq!(now["U1.3"], before["U1.1"]);
}

#[test]
fn pins_are_swapped_in_the_order_they_were_picked() {
    let mut s = section();
    s.symbols.push(sym_of("U1", "Test:TRI", 1, p(mm(20.32), mm(25.4))));
    let before = pin_points(&s);
    let after = ok(s, SchCmd::SwapPins { pins: ids(&["pin:U1:3", "pin:U1:1", "pin:U1:2"]) });
    let now = pin_points(&after);
    // 3 and 1 swap, then 1 (now in 3's place) and 2 swap
    assert_eq!(now["U1.3"], before["U1.1"]);
    assert_eq!(now["U1.1"], before["U1.2"]);
    assert_eq!(now["U1.2"], before["U1.3"]);
}

#[test]
fn a_swap_adds_the_junction_the_moved_pins_need_and_keeps_every_wire() {
    // Three things meet at pin 3's point (the pin and two wires) with no junction dot: `AddJunctionsIfNeeded` puts one there once the sheet is read
    // again after the swap, wherever the pins went.
    let mut s = section();
    s.symbols.push(sym_of("U1", "Test:TRI", 1, p(mm(20.32), mm(25.4))));
    let pins = pin_points(&s);
    let (p2, p3) = (pins["U1.2"], pins["U1.3"]);
    s.wires.push(wire("wire_b", &[p(p3.x - mm(10.16), p3.y), p3]));
    s.wires.push(wire("wire_c", &[p3, p(p3.x, p3.y + mm(5.08))]));
    s.wires.push(wire("wire_d", &[p(p2.x - mm(10.16), p2.y), p2]));
    s.assign_missing_ids();
    assert!(s.junctions.is_empty());
    let after = ok(s.clone(), SchCmd::SwapPins { pins: ids(&["pin:U1:2", "pin:U1:3"]) });
    assert_eq!(after.wires.iter().map(|w| w.pts.clone()).collect::<Vec<_>>(), s.wires.iter().map(|w| w.pts.clone()).collect::<Vec<_>>(), "no wire lost or changed");
    assert_eq!(after.junctions.len(), 1, "{:?}", after.junctions);
    assert_eq!(after.junctions[0].at, p3);
    assert!(!after.junctions[0].id.is_empty());
    let now = pin_points(&after);
    assert_eq!(now["U1.2"], p3);
    assert_eq!(now["U1.3"], p2);
}

#[test]
fn swap_pins_is_refused_with_its_reason() {
    let s = labelled_resistor();
    // fewer than two pins
    let (check, why) = err(s.clone(), SchCmd::SwapPins { pins: ids(&["pin:R1:1"]) });
    assert_eq!(check, "ops_swap_pins_few");
    assert!(why.contains("at least two pins"), "{why}");
    // pins of two symbols
    let mut two = s.clone();
    two.symbols.push(res("R2", p(mm(60.96), mm(25.4))));
    let (check, why) = err(two, SchCmd::SwapPins { pins: ids(&["pin:R1:1", "pin:R2:2"]) });
    assert_eq!(check, "ops_swap_pins_symbols");
    assert!(why.contains("same symbol"), "{why}");
    // a pin the symbol does not have, a symbol the sheet does not have, a text that is no pin, the same pin twice
    assert_eq!(err(s.clone(), SchCmd::SwapPins { pins: ids(&["pin:R1:1", "pin:R1:9"]) }).0, "ops_unknown_pin");
    assert_eq!(err(s.clone(), SchCmd::SwapPins { pins: ids(&["pin:R1:1", "pin:R7:1"]) }).0, "ops_unknown_symbol");
    assert_eq!(err(s.clone(), SchCmd::SwapPins { pins: ids(&["pin:R1:1", "R1"]) }).0, "ops_unknown_pin");
    assert_eq!(err(s.clone(), SchCmd::SwapPins { pins: ids(&["pin:R1:1", "pin:R1:1"]) }).0, "ops_duplicate_pin");
}

#[test]
fn swap_pins_is_refused_for_a_symbol_of_a_sheet_that_is_placed_twice() {
    let m = model();
    let mut child = labelled_resistor();
    child.imported_from_kicad = true;
    let mut root = section();
    for (i, name) in ["Left", "Right"].iter().enumerate() {
        root.sheets.push(SheetInstance { id: format!("sheet_{i}"), name: (*name).into(), file: "sub.kicad_sch".into(), at: p(Nm::from_raw_shim(10_000_000 + i as i64 * 60_000_000), mm(10.0)), size: (mm(40.0), mm(20.0)), pins: vec![], page: String::new() });
    }
    let mut design = empty_design();
    design.schematic = Some(root);
    design.sheet_contents = Some([("sub.kicad_sch".to_string(), child)].into_iter().collect());
    let mut b = Board::new(design, &m, mm(0.1), mm(0.3));
    let cmd = Cmd::OnSheet { sheet: "sheet_0".into(), cmd: Box::new(Cmd::SchEdit(SchCmd::SwapPins { pins: ids(&["pin:R1:1", "pin:R1:2"]) })) };
    let e = b.apply(&cmd).unwrap_err();
    assert_eq!(e[0].check, "ops_swap_pins_shared");
    let why = e[0].hint.clone().unwrap_or_default();
    assert!(why.contains("Pin swaps are disabled for symbols used by multiple sheet instances (Left, Right)"), "{why}");
    // the sheet is untouched
    assert!(b.design().sheet_contents.as_ref().unwrap()["sub.kicad_sch"].extras.pin_swaps.is_empty());
}

// ------------------------------------------------------------------------------------------------------------------- Swap Pin Labels

#[test]
fn swap_pin_labels_trades_the_texts_of_the_labels_on_the_pins() {
    let s = labelled_resistor();
    let after = ok(s.clone(), SchCmd::SwapPinLabels { pins: ids(&["pin:R1:1", "pin:R1:2"]) });
    let text = |sch: &eda_model::ir::SchematicSection, id: &str| sch.labels.iter().find(|l| l.id == id).unwrap().net.clone();
    assert_eq!(text(&after, "lbl_a"), "B");
    assert_eq!(text(&after, "lbl_b"), "A");
    // the labels did not move and nothing else changed
    assert_eq!(after.labels.iter().map(|l| l.at).collect::<Vec<_>>(), s.labels.iter().map(|l| l.at).collect::<Vec<_>>());
    assert_eq!(after.wires.iter().map(|w| w.pts.clone()).collect::<Vec<_>>(), s.wires.iter().map(|w| w.pts.clone()).collect::<Vec<_>>());
    assert_eq!(after.symbols[0].at, s.symbols[0].at);
    // so the pins are on each other's nets
    let nets = traced(&after);
    assert_eq!(net_of(&nets, "R1.1").as_deref(), Some("/B"));
    assert_eq!(net_of(&nets, "R1.2").as_deref(), Some("/A"));
}

#[test]
fn three_pin_labels_cycle_in_selection_order() {
    let mut s = section();
    s.symbols.push(sym_of("U1", "Test:TRI", 1, p(mm(20.32), mm(25.4))));
    let pins = pin_points(&s);
    for (n, net) in [("1", "X"), ("2", "Y"), ("3", "Z")] {
        let at = pins[&format!("U1.{n}")];
        s.wires.push(wire(&format!("wire_{n}"), &[at, p(at.x - mm(10.16), at.y)]));
        s.labels.push(label(&format!("lbl_{n}"), net, p(at.x - mm(10.16), at.y)));
    }
    s.assign_missing_ids();
    let after = ok(s, SchCmd::SwapPinLabels { pins: ids(&["pin:U1:1", "pin:U1:2", "pin:U1:3"]) });
    let text = |id: &str| after.labels.iter().find(|l| l.id == id).unwrap().net.clone();
    // (a, b) then (b, c): the first label gets the second's text, the second the third's, the third the first's
    assert_eq!((text("lbl_1"), text("lbl_2"), text("lbl_3")), ("Y".to_string(), "Z".to_string(), "X".to_string()));
}

#[test]
fn swap_pin_labels_is_refused_unless_each_pin_has_exactly_one_label_and_no_other_pin() {
    let msg = "Each selected pin must have exactly one attached net label and no other pin connections.";
    // a pin with no label
    let mut s = labelled_resistor();
    s.labels.retain(|l| l.id != "lbl_b");
    let (check, why) = err(s, SchCmd::SwapPinLabels { pins: ids(&["pin:R1:1", "pin:R1:2"]) });
    assert_eq!(check, "ops_swap_labels_unattached");
    assert_eq!(why, msg);
    // a connection with two labels
    let mut s = labelled_resistor();
    // (the second label sits on the wire between pin 2 and "B")
    s.labels.push(label("lbl_c", "C", p(mm(20.32), mm(34.0))));
    assert_eq!(err(s, SchCmd::SwapPinLabels { pins: ids(&["pin:R1:1", "pin:R1:2"]) }).1, msg);
    // a connection that reaches another pin: R1.2 wired to R2.1
    let mut s = labelled_resistor();
    s.symbols.push(res("R2", p(mm(20.32), mm(45.72))));
    s.wires.push(wire("wire_x", &[p(mm(20.32), mm(38.1)), p(mm(20.32), mm(41.91))]));
    assert_eq!(err(s, SchCmd::SwapPinLabels { pins: ids(&["pin:R1:1", "pin:R1:2"]) }).1, msg);
    // a power symbol on the connection is a pin too
    let mut s = labelled_resistor();
    s.power_symbols.push(PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: p(mm(20.32), mm(38.1)), rot: 0, net: "GND".into(), pin: String::new() });
    assert_eq!(err(s, SchCmd::SwapPinLabels { pins: ids(&["pin:R1:1", "pin:R1:2"]) }).1, msg);
    // fewer than two pins
    assert_eq!(err(labelled_resistor(), SchCmd::SwapPinLabels { pins: ids(&["pin:R1:1"]) }).0, "ops_swap_labels_few");
}

// ------------------------------------------------------------------------------------------------------------------- Swap Unit Labels

/// U1 as two units side by side, each pin with a wire and a label to its left: unit 1 has "A" (pin 1, higher) and "B", unit 2 has "C" and "D".
fn two_units() -> eda_model::ir::SchematicSection {
    let mut s = section();
    s.symbols.push(sym_of("U1", "Test:DUAL", 1, p(mm(40.64), mm(25.4))));
    s.symbols.push(sym_of("U1", "Test:DUAL", 2, p(mm(101.6), mm(25.4))));
    let pins = pin_points(&s);
    for (key, net) in [("U1.1", "A"), ("U1.2", "B"), ("U1#2.3", "C"), ("U1#2.4", "D")] {
        let at = pins[key];
        let id = key.replace(['.', '#'], "_");
        s.wires.push(wire(&format!("wire_{id}"), &[at, p(at.x - mm(10.16), at.y)]));
        s.labels.push(label(&format!("lbl_{id}"), net, p(at.x - mm(10.16), at.y)));
    }
    s.assign_missing_ids();
    s
}

#[test]
fn swap_unit_labels_passes_the_texts_one_unit_along_pin_by_pin() {
    let s = two_units();
    let after = ok(s, SchCmd::SwapUnitLabels { ids: ids(&["U1"]) });
    let text = |id: &str| after.labels.iter().find(|l| l.id == id).unwrap().net.clone();
    // pins ordered by position (x, then y): the upper pin of each unit is first; the two units trade the labels of the same index
    assert_eq!((text("lbl_U1_1"), text("lbl_U1_2")), ("C".to_string(), "D".to_string()));
    assert_eq!((text("lbl_U1_2_3"), text("lbl_U1_2_4")), ("A".to_string(), "B".to_string()));
}

#[test]
fn swap_unit_labels_is_refused_with_its_reason() {
    let msg = "Each pin of selected units must have exactly one attached net label and no other pin connections.";
    // a pin without a label
    let mut s = two_units();
    s.labels.retain(|l| l.id != "lbl_U1_2_4");
    let (check, why) = err(s, SchCmd::SwapUnitLabels { ids: ids(&["U1"]) });
    assert_eq!(check, "ops_swap_labels_unattached");
    assert_eq!(why, msg);
    // a symbol with one unit, one unit placed, two references, an unknown reference
    let mut one = section();
    one.symbols.push(res("R1", p(mm(20.32), mm(25.4))));
    one.symbols.push(res("R2", p(mm(60.96), mm(25.4))));
    assert_eq!(err(one.clone(), SchCmd::SwapUnitLabels { ids: ids(&["R1"]) }).0, "ops_swap_units");
    assert_eq!(err(one.clone(), SchCmd::SwapUnitLabels { ids: ids(&["R1", "R2"]) }).0, "ops_swap_units");
    assert_eq!(err(one, SchCmd::SwapUnitLabels { ids: ids(&["R9"]) }).0, "ops_unknown_symbol");
    let mut single_unit = section();
    single_unit.symbols.push(sym_of("U1", "Test:DUAL", 1, p(mm(40.64), mm(25.4))));
    let (check, why) = err(single_unit, SchCmd::SwapUnitLabels { ids: ids(&["U1"]) });
    assert_eq!(check, "ops_swap_units");
    assert!(why.contains("only one unit"), "{why}");
}

#[test]
fn the_three_verbs_describe_themselves_and_change_connectivity() {
    for cmd in [SchCmd::SwapPins { pins: ids(&["pin:R1:1", "pin:R1:2"]) }, SchCmd::SwapPinLabels { pins: ids(&["pin:R1:1", "pin:R1:2"]) }, SchCmd::SwapUnitLabels { ids: ids(&["U1"]) }] {
        assert!(cmd.edits_connectivity());
        assert!(cmd.describe().starts_with("schematic swap-"), "{}", cmd.describe());
        assert!(!cmd.ids().is_empty());
        let json = serde_json::to_string(&Cmd::SchEdit(cmd.clone())).unwrap();
        assert!(json.contains("\"op\":\"sch_edit\"") && json.contains("\"verb\":\"swap_"), "{json}");
        let back: Cmd = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Cmd::SchEdit(cmd));
    }
}
