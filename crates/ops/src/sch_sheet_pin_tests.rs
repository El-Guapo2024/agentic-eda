//! Tests for the pins of a hierarchical sheet taken one by one: a pin moved along the border of its sheet (`SCH_SHEET_PIN::ConstrainOnEdge`), edited, deleted and
//! swapped with another pin of the same sheet, and the nets of the hierarchy following each of them.
//!
//! The root sheet has a sheet `Sub` (40 x 30 mm at 100, 50 mm) with a pin `IN` on its left border and a pin `OUT` on its right one, each on a wire to a label (`SIG`, `RES`).
//! The sheet's own file has a resistor R1 whose pin 1 is on a wire to a hierarchical label `IN` and pin 2 on one to a label `OUT`: so R1.1 is on SIG and R1.2 on RES.

use super::*;
use crate::sch_edit::SchCmd;
use crate::sch_move::SchMoveCmd;
use crate::sch_scene::Scene;
use eda_model::ir::{LabelKind, LabelShape, NetLabel, Provenance, SheetInstance, SheetPin, SymbolInstance, Wire};
use std::collections::BTreeMap;
use eda_model::units::mm;

const SHEET_AT: Point = Point { x: Nm::new(100_000_000), y: Nm::new(50_000_000) };
const SHEET_SIZE: (Nm, Nm) = (Nm::new(40_000_000), Nm::new(30_000_000));

fn p(x: Nm, y: Nm) -> Point {
    Point { x, y }
}

fn wire(id: &str, pts: &[Point]) -> Wire {
    Wire { id: id.into(), net: "N".into(), pins: Vec::new(), pts: pts.to_vec(), bus: false }
}

fn label(id: &str, net: &str, at: Point, kind: LabelKind) -> NetLabel {
    NetLabel { id: id.into(), net: net.into(), at, kind }
}

fn design() -> Design {
    let mut root = eda_model::ir::SchematicSection { imported_from_kicad: true, ..Default::default() };
    root.sheets.push(SheetInstance {
        id: "sheet_sub".into(),
        name: "Sub".into(),
        file: "sub.kicad_sch".into(),
        at: SHEET_AT,
        size: SHEET_SIZE,
        pins: vec![
            SheetPin { id: "shpin_in".into(), name: "IN".into(), shape: LabelShape::Input, at: p(mm(100.0), mm(60.0)) },
            SheetPin { id: "shpin_out".into(), name: "OUT".into(), shape: LabelShape::Output, at: p(mm(140.0), mm(70.0)) },
        ],
        page: String::new(),
    });
    // SIG comes in on a wire to the left border, RES goes out from the right one
    root.wires.push(wire("wire_in", &[p(mm(80.0), mm(60.0)), p(mm(100.0), mm(60.0))]));
    root.labels.push(label("lbl_sig", "SIG", p(mm(80.0), mm(60.0)), LabelKind::Local));
    root.wires.push(wire("wire_out", &[p(mm(140.0), mm(70.0)), p(mm(160.0), mm(70.0))]));
    root.labels.push(label("lbl_res", "RES", p(mm(160.0), mm(70.0)), LabelKind::Local));
    root.assign_missing_ids();

    let mut child = eda_model::ir::SchematicSection { imported_from_kicad: true, ..Default::default() };
    child.symbols.push(SymbolInstance { id: "R1".into(), at: p(mm(20.32), mm(25.4)), rot: 0, mirrored: false, mirror_y: false, lib_id: "Device:R".into(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false });
    child.wires.push(wire("wire_a", &[p(mm(20.32), mm(21.59)), p(mm(20.32), mm(12.7))]));
    child.labels.push(label("lbl_in", "IN", p(mm(20.32), mm(12.7)), LabelKind::Hierarchical { shape: LabelShape::Input }));
    child.wires.push(wire("wire_b", &[p(mm(20.32), mm(29.21)), p(mm(20.32), mm(38.1))]));
    child.labels.push(label("lbl_out", "OUT", p(mm(20.32), mm(38.1)), LabelKind::Hierarchical { shape: LabelShape::Output }));
    child.assign_missing_ids();

    Design {
        footprint_library: None,
        sheet_contents: Some([("sub.kicad_sch".to_string(), child)].into_iter().collect()),
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: Some(root),
        nets: None,
        routing: None,
        placement: None,
        drawings: None,
    }
}

fn run(d: &Design, cmd: Cmd) -> Result<Design, Vec<eda_model::CheckResult>> {
    let m = ConstraintModel::default();
    let mut b = Board::new(d.clone(), &m, mm(0.1), mm(0.3));
    b.apply(&cmd)?;
    Ok(b.design().clone())
}

fn ok(d: &Design, cmd: Cmd) -> Design {
    run(d, cmd).unwrap_or_else(|e| panic!("{e:?}"))
}

fn root_of(d: &Design) -> &eda_model::ir::SchematicSection {
    d.schematic.as_ref().unwrap()
}

fn pin_at(d: &Design, id: &str) -> Point {
    root_of(d).sheets[0].pins.iter().find(|p| p.id == id).unwrap().at
}

/// The net each pin of R1 is on across the hierarchy: "R1.1" -> the name of the net it is on, when a label names it.
fn nets(d: &Design) -> BTreeMap<String, String> {
    let m = ConstraintModel::default();
    let mut root = d.schematic.clone().unwrap();
    let mut child = d.sheet_contents.as_ref().unwrap()["sub.kicad_sch"].clone();
    let scene = Scene::new(&child, &m);
    let mut child_pins = BTreeMap::new();
    for (i, s) in child.symbols.iter().enumerate() {
        for (n, at) in scene.pins_of(i) {
            child_pins.insert(format!("{}.{}", s.id, n), at);
        }
    }
    let mut screens = vec![
        eda_engine::nets::ScreenIn { path: Vec::new(), sch: &mut root, file: String::new(), pins: BTreeMap::new() },
        eda_engine::nets::ScreenIn { path: Vec::new(), sch: &mut child, file: "sub.kicad_sch".into(), pins: child_pins },
    ];
    let naming = eda_engine::nets::Naming::default();
    let mut out = BTreeMap::new();
    for net in eda_engine::nets::trace_nets_with(&mut screens, &naming) {
        for pin in &net.pins {
            out.insert(pin.clone(), net.name.clone());
        }
    }
    out
}

fn mv(ids: &[&str], dx: Nm, dy: Nm) -> Cmd {
    Cmd::SchMove(SchMoveCmd::Move { ids: ids.iter().map(|s| s.to_string()).collect(), dx, dy, turns: Vec::new(), about: None })
}

fn drag(ids: &[&str], dx: Nm, dy: Nm) -> Cmd {
    Cmd::SchMove(SchMoveCmd::Drag { ids: ids.iter().map(|s| s.to_string()).collect(), vertices: BTreeMap::new(), dx, dy, ortho: true, grid: Nm::ZERO, turns: Vec::new(), about: None })
}

#[test]
fn the_hierarchy_ties_the_pin_to_the_label_of_its_name() {
    // sanity: the fixture is what the tests say it is
    let n = nets(&design());
    assert_eq!((n["R1.1"].as_str(), n["R1.2"].as_str()), ("/SIG", "/RES"), "{n:?}");
}

#[test]
fn a_lone_pin_moves_along_its_border_and_the_net_it_was_wired_to_lets_go() {
    let d = design();
    // 5.08 mm down the left border: still on it, still a pin of this sheet; the wire stays where it was (a move leaves wires), so the pin is off it now
    let after = ok(&d, mv(&["shpin_in"], Nm::ZERO, mm(5.08)));
    assert_eq!(pin_at(&after, "shpin_in"), p(mm(100.0), mm(65.08)));
    assert_eq!(root_of(&after).wires.iter().find(|w| w.id == "wire_in").unwrap().pts, vec![p(mm(80.0), mm(60.0)), p(mm(100.0), mm(60.0))]);
    let n = nets(&after);
    assert_ne!(n["R1.1"], "/SIG", "the pin is off the wire: R1.1 is no longer on SIG ({n:?})");
    assert_eq!(n["R1.2"], "/RES", "the other pin did not move");
    // and the sheet's other pin, the sheet itself and its file are as they were
    assert_eq!(pin_at(&after, "shpin_out"), p(mm(140.0), mm(70.0)));
    assert_eq!(root_of(&after).sheets[0].at, SHEET_AT);
}

#[test]
fn a_pin_pushed_past_the_end_of_its_border_stops_at_the_corner_and_one_dragged_across_goes_to_the_nearest_border() {
    let d = design();
    // far below the bottom-left corner: the pin stays on the left border, at the corner
    let below = ok(&d, mv(&["shpin_in"], Nm::ZERO, mm(90.0)));
    assert_eq!(pin_at(&below, "shpin_in"), p(mm(100.0), mm(80.0)));
    // moved to a point nearer the right border than any other: it changes border (`ConstrainOnEdge( aPos, true )`)
    let across = ok(&d, mv(&["shpin_in"], mm(36.0), mm(5.0)));
    assert_eq!(pin_at(&across, "shpin_in"), p(mm(140.0), mm(65.0)));
    // and then it is on the right border: the pin the writer gives angle 0, KiCad reads it back on that side
    assert_eq!(pin_at(&across, "shpin_out"), p(mm(140.0), mm(70.0)));
}

#[test]
fn dragging_the_pin_takes_the_wire_with_it_and_the_net_stays() {
    let d = design();
    let after = ok(&d, drag(&["shpin_in"], Nm::ZERO, mm(5.08)));
    assert_eq!(pin_at(&after, "shpin_in"), p(mm(100.0), mm(65.08)));
    // the wire's end followed the pin: it still ends on it
    let wire = root_of(&after).wires.iter().find(|w| w.pts.contains(&p(mm(100.0), mm(65.08)))).expect("a wire ends at the pin");
    assert!(wire.pts.contains(&p(mm(80.0), mm(60.0))) || root_of(&after).wires.len() > 2, "{:?}", root_of(&after).wires);
    let n = nets(&after);
    assert_eq!(n["R1.1"], "/SIG", "{n:?}");
}

#[test]
fn deleting_a_pin_cuts_the_net_through_it_and_a_second_delete_is_refused() {
    let d = design();
    let after = ok(&d, Cmd::SchEdit(SchCmd::DeleteSheetPin { id: "shpin_in".into() }));
    let sheet = &root_of(&after).sheets[0];
    assert_eq!(sheet.pins.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["shpin_out"]);
    assert_eq!(root_of(&after).wires.len(), 2, "the wire stays: only the pin goes");
    let n = nets(&after);
    assert_ne!(n["R1.1"], "/SIG");
    assert_eq!(n["R1.2"], "/RES");
    let e = run(&after, Cmd::SchEdit(SchCmd::DeleteSheetPin { id: "shpin_in".into() })).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_sheet_pin");
}

#[test]
fn renaming_a_pin_ties_it_to_the_label_of_the_new_name() {
    let d = design();
    // IN -> OUT: the pin of the left border now stands for the label OUT, which is on R1's other pin (and the pin that was OUT now has the same name: the two sheet pins
    // of one name do not join, KiCad's "promote sheet pins ... if no conflict")
    let renamed = ok(&d, Cmd::SchEdit(SchCmd::EditSheetPin { id: "shpin_in".into(), name: Some("CLK".into()), shape: Some(LabelShape::Bidirectional), at: None, size_nm: None, bold: None, italic: None }));
    let sheet = &root_of(&renamed).sheets[0];
    let pin = sheet.pins.iter().find(|p| p.id == "shpin_in").unwrap();
    assert_eq!((pin.name.as_str(), pin.shape), ("CLK", LabelShape::Bidirectional));
    assert_eq!(pin.at, p(mm(100.0), mm(60.0)), "where it is does not change with its name");
    let n = nets(&renamed);
    assert_ne!(n["R1.1"], "/SIG", "no label of the name CLK in the sheet's file: R1.1 is not on SIG any more ({n:?})");
    // an empty name and a point off the border are refused
    let e = run(&d, Cmd::SchEdit(SchCmd::EditSheetPin { id: "shpin_in".into(), name: Some("  ".into()), shape: None, at: None, size_nm: None, bold: None, italic: None }));
    assert!(e.is_err());
    let e = run(&d, Cmd::SchEdit(SchCmd::EditSheetPin { id: "shpin_in".into(), name: None, shape: None, at: Some(p(mm(110.0), mm(65.0))), size_nm: None, bold: None, italic: None })).unwrap_err();
    assert_eq!(e[0].check, "ops_sheet_pin_off_border");
}

#[test]
fn two_pins_of_one_sheet_trade_places_and_with_them_the_nets_they_reach() {
    let d = design();
    let after = ok(&d, Cmd::SwapSchItems { a: "shpin_in".into(), b: "shpin_out".into() });
    // IN is on the right border now and OUT on the left one (a pin's side is where it sits)
    assert_eq!(pin_at(&after, "shpin_in"), p(mm(140.0), mm(70.0)));
    assert_eq!(pin_at(&after, "shpin_out"), p(mm(100.0), mm(60.0)));
    // the wires stayed: SIG now reaches OUT, so R1.2 is on SIG, and RES reaches IN: R1.1 is on RES
    let n = nets(&after);
    assert_eq!((n["R1.1"].as_str(), n["R1.2"].as_str()), ("/RES", "/SIG"), "{n:?}");
}

#[test]
fn a_sheet_pin_swaps_only_with_another_pin_of_its_sheet() {
    let mut d = design();
    // a second sheet with a pin of its own
    d.schematic.as_mut().unwrap().sheets.push(SheetInstance {
        id: "sheet_other".into(),
        name: "Other".into(),
        file: "other.kicad_sch".into(),
        at: p(mm(100.0), mm(100.0)),
        size: (mm(40.0), mm(30.0)),
        pins: vec![SheetPin { id: "shpin_x".into(), name: "X".into(), shape: LabelShape::Input, at: p(mm(100.0), mm(110.0)) }],
        page: String::new(),
    });
    let e = run(&d, Cmd::SwapSchItems { a: "shpin_in".into(), b: "shpin_x".into() }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_swap");
    assert!(e[0].hint.clone().unwrap_or_default().contains("same sheet"), "{:?}", e[0].hint);
    // a pin and a label
    let e = run(&d, Cmd::SwapSchItems { a: "shpin_in".into(), b: "lbl_sig".into() }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_swap");
    assert!(e[0].hint.clone().unwrap_or_default().contains("another pin"), "{:?}", e[0].hint);
}

// ------------------------------------------------------------------------------------------------------------------------------- the text look

fn look_edit(id: &str, size_nm: Option<Nm>, bold: Option<bool>, italic: Option<bool>) -> Cmd {
    Cmd::SchEdit(SchCmd::EditSheetPin { id: id.into(), name: None, shape: None, at: None, size_nm, bold, italic })
}

#[test]
fn a_pins_text_is_set_in_a_size_bold_and_italic_of_its_own_and_the_default_is_no_entry() {
    let d = design();
    let after = ok(&d, look_edit("shpin_in", Some(mm(2.0)), Some(true), Some(true)));
    let look = root_of(&after).extras.sheet_pin_looks.get("shpin_in").copied().expect("the pin has a look");
    assert_eq!((look.size_nm, look.bold, look.italic), (mm(2.0), true, true));
    assert!(!root_of(&after).extras.sheet_pin_looks.contains_key("shpin_out"), "the other pin is as it was");
    // each member alone leaves the others
    let bolder = ok(&after, look_edit("shpin_in", None, Some(false), None));
    let look = root_of(&bolder).extras.sheet_pin_looks.get("shpin_in").copied().unwrap();
    assert_eq!((look.size_nm, look.bold, look.italic), (mm(2.0), false, true));
    // back to 50 mil, regular, upright: the entry goes
    let plain = ok(&bolder, look_edit("shpin_in", Some(mm(1.27)), None, Some(false)));
    assert!(root_of(&plain).extras.sheet_pin_looks.is_empty(), "{:?}", root_of(&plain).extras.sheet_pin_looks);
}

#[test]
fn a_look_that_is_already_there_is_refused_and_a_size_is_clamped_as_set_text_size_clamps_it() {
    let d = design();
    let e = run(&d, look_edit("shpin_in", Some(mm(1.27)), Some(false), Some(false))).unwrap_err();
    assert_eq!(e[0].check, "ops_sheet_pin_unchanged", "50 mil, regular and upright is what it has");
    // `TEXT_MIN_SIZE_MM` 0.001 and `TEXT_MAX_SIZE_MM` 250
    for (size, kept) in [(0, 1_000), (5000, 5000), (1_000_001_000, 250_000_000)] {
        let after = ok(&d, look_edit("shpin_in", Some(Nm::from_raw_shim(size)), None, None));
        assert_eq!(root_of(&after).extras.sheet_pin_looks["shpin_in"].size_nm, Nm::from_raw_shim(kept), "{size}");
    }
    let styled = ok(&d, look_edit("shpin_in", None, Some(true), None));
    let e = run(&styled, look_edit("shpin_in", None, Some(true), None)).unwrap_err();
    assert_eq!(e[0].check, "ops_sheet_pin_unchanged");
}

#[test]
fn the_look_stays_with_a_pin_that_is_renamed_or_moved_and_goes_with_a_pin_or_a_sheet_that_is_deleted() {
    let d = design();
    let styled = ok(&d, look_edit("shpin_in", Some(mm(2.54)), Some(true), None));
    let renamed = ok(&styled, Cmd::SchEdit(SchCmd::EditSheetPin { id: "shpin_in".into(), name: Some("CLK".into()), shape: None, at: None, size_nm: None, bold: None, italic: None }));
    assert!(root_of(&renamed).extras.sheet_pin_looks.contains_key("shpin_in"), "the pin keeps its id, so its look");
    let moved = ok(&styled, Cmd::SchMove(SchMoveCmd::Move { ids: vec!["shpin_in".into()], dx: Nm::ZERO, dy: mm(3.0), turns: Vec::new(), about: None }));
    assert!(root_of(&moved).extras.sheet_pin_looks.contains_key("shpin_in"));
    let gone = ok(&styled, Cmd::SchEdit(SchCmd::DeleteSheetPin { id: "shpin_in".into() }));
    assert!(root_of(&gone).extras.sheet_pin_looks.is_empty());
    let sheet_gone = ok(&styled, Cmd::SchEdit(SchCmd::DeleteSheet { id: "sheet_sub".into() }));
    assert!(root_of(&sheet_gone).extras.sheet_pin_looks.is_empty());
}
