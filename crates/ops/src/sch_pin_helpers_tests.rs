//! Tests for Pin Helpers (`SchCmd::PinHelper`; `sch_pin_helpers.rs`): a wire stub, a no-connect and a local, a hierarchical and a global label on pins and sheet pins.
//!
//! The sheet is a KiCad-style one (`imported_from_kicad`: a symbol sits at its library origin) with `Test:FOUR`, whose four pins run each way from their connection points:
//! 1 at the left edge running right, 2 at the right edge running left, 3 at the bottom running up, 4 at the top running down.

use super::*;
use crate::sch_edit::SchCmd;
use crate::sch_pin_helpers::PinHelper;
use eda_model::ir::{LabelKind, LabelShape, Provenance, SchematicSection, SheetInstance, SheetPin, SymbolInstance, Wire};
use eda_model::sch_extras::LabelSpin;
use eda_model::symbol::SPoint;
use eda_model::{LibPin, LibSymbol};

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

fn lib_pin(number: &str, name: &str, etype: &str, x: f64, y: f64, angle: f64) -> LibPin {
    LibPin { number: number.into(), name: name.into(), electrical_type: etype.into(), shape: "line".into(), at: SPoint::new(x, y), angle_deg: angle, length_mm: 2.54, unit: 1 }
}

fn lib(lib_id: &str, pins: Vec<LibPin>) -> LibSymbol {
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
        unit_count: 1,
        pin_names_hidden: false,
        pin_numbers_hidden: false,
        pin_name_offset_mm: 0.508,
        alternate: None,
    }
}

/// `Test:FOUR` (library frame, mm, y up): pin 1 `IN` (input) at the left edge running right, 2 `OUT` (output) at the right edge running left, 3 `IO` (bidirectional) at the bottom
/// running up, 4 with no name (`~`, power input) at the top running down. `Test:KINDS` has one pin of each of the other types, all running right.
fn model() -> ConstraintModel {
    let four = lib(
        "Test:FOUR",
        vec![
            lib_pin("1", "IN", "input", -5.08, 0.0, 0.0),
            lib_pin("2", "OUT", "output", 5.08, 0.0, 180.0),
            lib_pin("3", "IO", "bidirectional", 0.0, -5.08, 90.0),
            lib_pin("4", "~", "power_in", 0.0, 5.08, 270.0),
        ],
    );
    let kinds = lib(
        "Test:KINDS",
        vec![
            lib_pin("1", "T", "tri_state", 0.0, 0.0, 0.0),
            lib_pin("2", "U", "unspecified", 0.0, -2.54, 0.0),
            lib_pin("3", "P", "passive", 0.0, -5.08, 0.0),
            lib_pin("4", "OC", "open_collector", 0.0, -7.62, 0.0),
        ],
    );
    ConstraintModel { symbols: vec![four, kinds], ..Default::default() }
}

fn symbol(id: &str, lib_id: &str, at: Point, rot: u32, mirrored: bool) -> SymbolInstance {
    SymbolInstance {
        id: id.into(),
        at,
        rot,
        mirrored,
        mirror_y: false,
        lib_id: lib_id.into(),
        unit: 1,
        value: String::new(),
        footprint: String::new(),
        datasheet: String::new(),
        dnp: false,
        exclude_from_bom: false,
        exclude_from_board: false,
        exclude_from_sim: false,
    }
}

/// U1 (`Test:FOUR`) at (50.8, 50.8) mm, turned `rot` and mirrored or not.
fn sheet(rot: u32, mirrored: bool) -> SchematicSection {
    SchematicSection { imported_from_kicad: true, symbols: vec![symbol("U1", "Test:FOUR", p(50_800, 50_800), rot, mirrored)], ..Default::default() }
}

fn run(sch: SchematicSection, cmd: SchCmd) -> Result<SchematicSection, Vec<CheckResult>> {
    let m = model();
    let mut design = empty_design();
    design.schematic = Some(sch);
    let mut b = Board::new(design, &m, 100, 300);
    b.apply(&Cmd::SchEdit(cmd))?;
    Ok(b.design().schematic.clone().unwrap())
}

fn helper(what: PinHelper, pins: &[&str], grid: Um) -> SchCmd {
    SchCmd::PinHelper { what, pins: pins.iter().map(|s| s.to_string()).collect(), grid }
}

fn ok(sch: SchematicSection, what: PinHelper, pins: &[&str]) -> SchematicSection {
    run(sch, helper(what, pins, 0)).unwrap_or_else(|e| panic!("{e:?}"))
}

/// Where pin `number` of U1 is on the sheet.
fn pin_at(sch: &SchematicSection, number: &str) -> Point {
    let m = model();
    let scene = crate::sch_scene::Scene::new(sch, &m);
    scene.pins_of(0).into_iter().find(|(n, _)| n == number).map(|(_, at)| at).unwrap_or_else(|| panic!("no pin {number}"))
}

/// The wire that starts at `from`: its other end.
fn stub_from(sch: &SchematicSection, from: Point) -> Point {
    let w = sch.wires.iter().find(|w| w.pts.first() == Some(&from) || w.pts.last() == Some(&from)).unwrap_or_else(|| panic!("no wire at {from:?}: {:?}", sch.wires));
    if w.pts.first() == Some(&from) {
        *w.pts.last().unwrap()
    } else {
        w.pts[0]
    }
}

fn label_at<'a>(sch: &'a SchematicSection, at: Point) -> &'a eda_model::ir::NetLabel {
    sch.labels.iter().find(|l| l.at == at).unwrap_or_else(|| panic!("no label at {at:?}: {:?}", sch.labels))
}

// ------------------------------------------------------------------------------------------------------------------------------- wire

#[test]
fn a_wire_helper_draws_a_stub_one_grid_step_out_of_each_pin_along_its_line() {
    let start = sheet(0, false);
    let (p1, p2, p3, p4) = (pin_at(&start, "1"), pin_at(&start, "2"), pin_at(&start, "3"), pin_at(&start, "4"));
    // pin 1 runs right (the body is to its right): the stub goes left; pin 2 runs left: right; pin 3 runs up: down; pin 4 runs down: up
    let after = ok(start, PinHelper::Wire, &["pin:U1:1", "pin:U1:2", "pin:U1:3", "pin:U1:4"]);
    assert_eq!(after.wires.len(), 4);
    assert_eq!(stub_from(&after, p1), p(p1.x - 1_270, p1.y));
    assert_eq!(stub_from(&after, p2), p(p2.x + 1_270, p2.y));
    assert_eq!(stub_from(&after, p3), p(p3.x, p3.y + 1_270));
    assert_eq!(stub_from(&after, p4), p(p4.x, p4.y - 1_270));
}

#[test]
fn a_stub_goes_out_of_a_turned_and_mirrored_symbol_the_way_the_pin_runs_there() {
    // `pinOrientation`: the library pin's orientation turned and mirrored by the symbol's (the mirror after the turn, `SetOrientation`)
    for (rot, mirrored, pin, step) in [
        (90_000, false, "1", (0, 1_270)),   // pin 1 runs right; a quarter turn counter-clockwise takes it up, so the stub goes down
        (180_000, false, "1", (1_270, 0)),  // runs left: the stub goes right
        (270_000, false, "1", (0, -1_270)), // runs down: the stub goes up
        (0, true, "1", (1_270, 0)),         // mirrored: runs left
        (0, true, "2", (-1_270, 0)),        // pin 2 ran left, now runs right
        (90_000, true, "3", (-1_270, 0)),   // pin 3 runs up; the quarter turn takes it left and the mirror right, so the stub goes left
    ] {
        let start = sheet(rot, mirrored);
        let tip = pin_at(&start, pin);
        let after = ok(start, PinHelper::Wire, &[&format!("pin:U1:{pin}")]);
        let end = stub_from(&after, tip);
        assert_eq!((end.x - tip.x, end.y - tip.y), step, "rot {rot} mirrored {mirrored} pin {pin}");
    }
}

#[test]
fn the_stub_takes_the_wire_grid_the_command_names() {
    let start = sheet(0, false);
    let tip = pin_at(&start, "1");
    let after = run(start, helper(PinHelper::Wire, &["pin:U1:1"], 2_540)).unwrap();
    assert_eq!(stub_from(&after, tip), p(tip.x - 2_540, tip.y));
}

#[test]
fn a_pin_with_a_wire_on_it_gets_the_junction_its_stub_needs() {
    let mut start = sheet(0, false);
    let tip = pin_at(&start, "1");
    // a wire already runs off pin 1 downwards
    start.wires.push(Wire { id: "w1".into(), net: "N".into(), pins: vec![], pts: vec![tip, p(tip.x, tip.y + 5_080)], bus: false });
    let after = ok(start, PinHelper::Wire, &["pin:U1:1"]);
    assert!(after.junctions.iter().any(|j| j.at == tip), "the pin, the old wire and the stub meet there: {:?}", after.junctions);
}

// ----------------------------------------------------------------------------------------------------------------------------- no connect

#[test]
fn a_no_connect_helper_flags_each_pin_once() {
    let start = sheet(0, false);
    let (p1, p3) = (pin_at(&start, "1"), pin_at(&start, "3"));
    let after = ok(start, PinHelper::NoConnect, &["pin:U1:1", "pin:U1:3"]);
    let mut at: Vec<Point> = after.no_connects.iter().map(|n| n.at).collect();
    at.sort_by_key(|q| (q.x, q.y));
    let mut want = vec![p1, p3];
    want.sort_by_key(|q| (q.x, q.y));
    assert_eq!(at, want);
    // a pin that has one already does not get a second (`CleanUp`: "two no-connects at one point become one"), and a command that changes nothing is refused, so it is no undo step
    let e = run(after.clone(), helper(PinHelper::NoConnect, &["pin:U1:1"], 0)).unwrap_err();
    assert_eq!(e[0].check, "ops_pin_helper_unchanged");
    // one pin that has it and one that has not: the one is done
    let more = ok(after, PinHelper::NoConnect, &["pin:U1:1", "pin:U1:2"]);
    assert_eq!(more.no_connects.len(), 3);
}

#[test]
fn a_stub_that_lies_along_a_wire_that_is_there_changes_nothing_and_is_refused() {
    let mut start = sheet(0, false);
    let tip = pin_at(&start, "1");
    // the wire of pin 1 already leaves it to the left: the stub is a part of it
    start.wires.push(Wire { id: "w1".into(), net: "N".into(), pins: vec![], pts: vec![tip, p(tip.x - 5_080, tip.y)], bus: false });
    let e = run(start, helper(PinHelper::Wire, &["pin:U1:1"], 0)).unwrap_err();
    assert_eq!(e[0].check, "ops_pin_helper_unchanged");
}

// ---------------------------------------------------------------------------------------------------------------------------------- labels

#[test]
fn a_label_helper_labels_each_pin_with_its_name_and_reads_away_from_it() {
    let start = sheet(0, false);
    let (p1, p2, p3, p4) = (pin_at(&start, "1"), pin_at(&start, "2"), pin_at(&start, "3"), pin_at(&start, "4"));
    let after = ok(start, PinHelper::NetLabel, &["pin:U1:1", "pin:U1:2", "pin:U1:3", "pin:U1:4"]);
    assert_eq!(after.labels.len(), 4);
    let spin = |l: &eda_model::ir::NetLabel| after.extras.label_spins.get(&l.id).copied();
    let l1 = label_at(&after, p1);
    assert_eq!((l1.net.as_str(), l1.kind.clone(), spin(l1)), ("IN", LabelKind::Local, Some(LabelSpin::Left)), "pin 1 runs right: the label reads left");
    let l2 = label_at(&after, p2);
    assert_eq!((l2.net.as_str(), spin(l2)), ("OUT", Some(LabelSpin::Right)));
    let l3 = label_at(&after, p3);
    assert_eq!((l3.net.as_str(), spin(l3)), ("IO", Some(LabelSpin::Bottom)), "pin 3 runs up: the label reads down");
    let l4 = label_at(&after, p4);
    assert_eq!((l4.net.as_str(), spin(l4)), ("U1_4", Some(LabelSpin::Up)), "a pin with no name is `<reference>_<number>`; it runs down: the label reads up");
}

#[test]
fn a_hierarchical_or_global_label_takes_the_shape_of_the_pins_type() {
    let sch = SchematicSection { imported_from_kicad: true, symbols: vec![symbol("U2", "Test:KINDS", p(50_800, 50_800), 0, false)], ..Default::default() };
    let shape_of = |sch: &SchematicSection, name: &str| match &sch.labels.iter().find(|l| l.net == name).unwrap().kind {
        LabelKind::Hierarchical { shape } | LabelKind::Global { shape } => *shape,
        LabelKind::Local => panic!("a local label has no shape"),
    };
    let hier = ok(sch.clone(), PinHelper::HierLabel, &["pin:U2:1", "pin:U2:2", "pin:U2:3", "pin:U2:4"]);
    assert!(hier.labels.iter().all(|l| matches!(l.kind, LabelKind::Hierarchical { .. })));
    assert_eq!(shape_of(&hier, "T"), LabelShape::TriState);
    assert_eq!(shape_of(&hier, "U"), LabelShape::Passive, "an unspecified pin: KiCad's unspecified shape");
    assert_eq!(shape_of(&hier, "P"), LabelShape::Input, "every other type is an input");
    assert_eq!(shape_of(&hier, "OC"), LabelShape::Input);
    let global = ok(sch, PinHelper::GlobalLabel, &["pin:U2:1"]);
    assert_eq!(shape_of(&global, "T"), LabelShape::TriState);
    assert!(matches!(global.labels[0].kind, LabelKind::Global { .. }));
    // the library's other types: input, output and bidirectional are their own
    let four = ok(sheet(0, false), PinHelper::GlobalLabel, &["pin:U1:1", "pin:U1:2", "pin:U1:3"]);
    assert_eq!((shape_of(&four, "IN"), shape_of(&four, "OUT"), shape_of(&four, "IO")), (LabelShape::Input, LabelShape::Output, LabelShape::Bidirectional));
}

#[test]
fn a_label_reads_away_from_a_turned_pin() {
    let start = sheet(90_000, false);
    let tip = pin_at(&start, "1");
    let after = ok(start, PinHelper::NetLabel, &["pin:U1:1"]);
    let l = label_at(&after, tip);
    // pin 1 runs right unturned; turned a quarter it runs up: the label reads down
    assert_eq!(after.extras.label_spins.get(&l.id), Some(&LabelSpin::Bottom));
}

#[test]
fn a_swapped_pin_is_labelled_where_it_sits_now_running_the_way_the_pin_it_swapped_with_does() {
    // pins 1 (`IN`, input, left edge, runs right) and 2 (`OUT`, output, right edge, runs left) trade places (`SwapPinGeometry`: position, orientation and length)
    let mut start = sheet(0, false);
    let (was1, was2) = (pin_at(&start, "1"), pin_at(&start, "2"));
    start.extras.swap_pin_places("U1", "1", "2");
    assert_eq!((pin_at(&start, "1"), pin_at(&start, "2")), (was2, was1), "pin 1 sits at the right edge now");
    let after = ok(start, PinHelper::GlobalLabel, &["pin:U1:1"]);
    let l = label_at(&after, was2);
    // it keeps its own name and type, and reads away from the edge it is on now: pin 2's pin runs left, so the label reads right
    assert_eq!(l.net, "IN");
    assert_eq!(l.kind, LabelKind::Global { shape: LabelShape::Input });
    assert_eq!(after.extras.label_spins.get(&l.id), Some(&LabelSpin::Right));
}

// ----------------------------------------------------------------------------------------------------------------------------- sheet pins

fn sheet_with_pins() -> SchematicSection {
    let at = p(100_000, 50_000);
    let pin = |id: &str, name: &str, shape: LabelShape, x: Um, y: Um| SheetPin { id: id.into(), name: name.into(), shape, at: p(x, y) };
    SchematicSection {
        imported_from_kicad: true,
        sheets: vec![SheetInstance {
            id: "sheet_a".into(),
            name: "Sub".into(),
            file: "sub.kicad_sch".into(),
            at,
            size: (40_000, 30_000),
            pins: vec![
                pin("shpin_l", "CLK", LabelShape::Input, 100_000, 60_000),
                pin("shpin_r", "DATA", LabelShape::Output, 140_000, 60_000),
                pin("shpin_t", "EN", LabelShape::Passive, 110_000, 50_000),
                pin("shpin_b", "IO", LabelShape::Bidirectional, 110_000, 80_000),
            ],
            page: String::new(),
        }],
        ..Default::default()
    }
}

#[test]
fn the_helpers_work_on_a_sheet_pin_which_runs_into_the_sheet_from_its_side() {
    // a stub goes out of the sheet: left of a pin on the left side, right of one on the right, up from the top and down from the bottom
    let after = ok(sheet_with_pins(), PinHelper::Wire, &["shpin_l", "shpin_r", "shpin_t", "shpin_b"]);
    assert_eq!(stub_from(&after, p(100_000, 60_000)), p(98_730, 60_000));
    assert_eq!(stub_from(&after, p(140_000, 60_000)), p(141_270, 60_000));
    assert_eq!(stub_from(&after, p(110_000, 50_000)), p(110_000, 48_730));
    assert_eq!(stub_from(&after, p(110_000, 80_000)), p(110_000, 81_270));
    // a label says the pin's name, reads out of the sheet, and has the pin's shape (a passive pin is an input: `PT_PASSIVE` is no label shape)
    let labelled = ok(sheet_with_pins(), PinHelper::GlobalLabel, &["shpin_l", "shpin_r", "shpin_t", "shpin_b"]);
    let kind = |name: &str| labelled.labels.iter().find(|l| l.net == name).map(|l| (l.kind.clone(), labelled.extras.label_spins.get(&l.id).copied())).unwrap();
    assert_eq!(kind("CLK"), (LabelKind::Global { shape: LabelShape::Input }, Some(LabelSpin::Left)));
    assert_eq!(kind("DATA"), (LabelKind::Global { shape: LabelShape::Output }, Some(LabelSpin::Right)));
    // on the top side the pin runs down into the sheet (`PIN_DOWN`): the label reads up out of it; on the bottom side the other way
    assert_eq!(kind("EN"), (LabelKind::Global { shape: LabelShape::Input }, Some(LabelSpin::Up)));
    assert_eq!(kind("IO"), (LabelKind::Global { shape: LabelShape::Bidirectional }, Some(LabelSpin::Bottom)));
    let flagged = ok(sheet_with_pins(), PinHelper::NoConnect, &["shpin_l"]);
    assert_eq!(flagged.no_connects.len(), 1);
    assert_eq!(flagged.no_connects[0].at, p(100_000, 60_000));
}

// ---------------------------------------------------------------------------------------------------------------------------- the refusals

#[test]
fn a_selection_with_no_pin_in_it_is_refused_and_other_items_are_passed_over() {
    let start = sheet(0, false);
    let e = run(start.clone(), helper(PinHelper::NoConnect, &["U1", "wire_x"], 0)).unwrap_err();
    assert_eq!(e[0].check, "ops_nothing_selected");
    let e = run(start.clone(), helper(PinHelper::Wire, &[], 0)).unwrap_err();
    assert_eq!(e[0].check, "ops_nothing_selected");
    let e = run(start.clone(), helper(PinHelper::Wire, &["pin:U1:9"], 0)).unwrap_err();
    assert_eq!(e[0].check, "ops_nothing_selected", "a pin the symbol does not have is no pin");
    // a pin among other things: the pin is done, the rest is left
    let after = ok(start, PinHelper::NoConnect, &["U1", "pin:U1:1"]);
    assert_eq!(after.no_connects.len(), 1);
}

#[test]
fn the_verb_is_one_json_command() {
    let cmd = helper(PinHelper::HierLabel, &["pin:U1:1"], 0);
    let json = serde_json::to_value(Cmd::SchEdit(cmd)).unwrap();
    assert_eq!(json, serde_json::json!({ "op": "sch_edit", "verb": "pin_helper", "what": "hier_label", "pins": ["pin:U1:1"], "grid": 0 }));
    let back: Cmd = serde_json::from_value(serde_json::json!({ "op": "sch_edit", "verb": "pin_helper", "what": "no_connect", "pins": ["pin:U1:2"] })).unwrap();
    assert!(matches!(back, Cmd::SchEdit(SchCmd::PinHelper { what: PinHelper::NoConnect, grid: 0, .. })));
}
