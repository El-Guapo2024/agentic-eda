//! Tests for Update Schematic from PCB's nets (`plan_net_back_annotation`, KiCad 25ec2335 `BACK_ANNOTATE::applyChangelist`, `applyPinSwaps`,
//! `PlanBackannotateUnitSwaps`, `processNetNameChange`): the report, the verbs, and the nets the drawing traces to once the verbs ran.
//!
//! The sheets are KiCad-style (`imported_from_kicad`: a symbol sits at its library origin). `Test:TWO` has pins 1 and 2 on its left edge, `Test:DUAL` two units
//! of two pins (1, 2 and 3, 4), `power:GND` is a power symbol.

use super::*;
use crate::Board;
use eda_model::ir::{NetLabel, PowerSymbol, Provenance, SymbolInstance, Wire};
use eda_model::symbol::SPoint;
use eda_model::units::{mm, Nm};
use eda_model::{LibPin, LibSymbol};

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

fn p(x: f64, y: f64) -> Point {
    Point { x: mm(x), y: mm(y) }
}

fn sym(id: &str, lib_id: &str, unit: u32, at: Point) -> SymbolInstance {
    SymbolInstance { id: id.into(), at, rot: 0, mirrored: false, mirror_y: false, lib_id: lib_id.into(), unit, value: String::new(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false, no_footprint: false }
}

fn lib_pin(number: &str, y: f64, unit: u32, kind: &str) -> LibPin {
    LibPin { number: number.into(), name: String::new(), electrical_type: kind.into(), shape: "line".into(), at: SPoint::new(0.0, y), angle_deg: 0.0, length_mm: 2.54, unit, hidden: false }
}

fn lib(lib_id: &str, unit_count: u32, pins: Vec<LibPin>, power: bool) -> LibSymbol {
    LibSymbol {
        connectivity: Default::default(),
        lib_id: lib_id.into(),
        graphics: vec![],
        pins,
        power,
        in_bom: true,
        on_board: !power,
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

fn model() -> ConstraintModel {
    let two = lib("Test:TWO", 1, vec![lib_pin("1", 2.54, 1, "passive"), lib_pin("2", -2.54, 1, "passive")], false);
    let dual = lib("Test:DUAL", 2, vec![lib_pin("1", 2.54, 1, "passive"), lib_pin("2", -2.54, 1, "passive"), lib_pin("3", 2.54, 2, "passive"), lib_pin("4", -2.54, 2, "passive")], false);
    let gnd = lib("power:GND", 1, vec![lib_pin("1", 0.0, 1, "power_in")], true);
    ConstraintModel { symbols: vec![two, dual, gnd], ..Default::default() }
}

fn section() -> SchematicSection {
    SchematicSection { imported_from_kicad: true, ..Default::default() }
}

/// The tips of the pins of the sheet: "U1.2" -> point.
fn tips(sch: &SchematicSection, m: &ConstraintModel) -> BTreeMap<String, Point> {
    let scene = Scene::new(sch, m);
    let mut out = BTreeMap::new();
    for (i, s) in sch.symbols.iter().enumerate() {
        for (n, at) in scene.pins_of(i) {
            out.insert(format!("{}.{n}", s.id), at);
        }
    }
    out
}

/// A wire from the pin `pin` of the sheet two grid steps to the left and a label `net` at its end (`None`: no label).
fn wire_out(sch: &mut SchematicSection, m: &ConstraintModel, pin: &str, net: Option<&str>) {
    let at = tips(sch, m)[pin];
    let end = Point { x: at.x - mm(5.08), y: at.y };
    sch.wires.push(Wire { id: String::new(), net: String::new(), pins: vec![], pts: vec![at, end], bus: false });
    if let Some(net) = net {
        sch.labels.push(NetLabel { id: String::new(), net: net.into(), at: end, kind: LabelKind::Local });
    }
}

fn design_of(mut sch: SchematicSection) -> Design {
    sch.assign_missing_ids();
    let mut d = empty_design();
    d.schematic = Some(sch);
    d
}

/// What the drawing traces to, with every pin keyed `REF.PIN` as the studio's tracer keys them.
fn drawn(design: &Design, m: &ConstraintModel) -> Vec<TracedNet> {
    let mut copy = design.schematic.clone().unwrap();
    let pins = tips(&copy, m);
    let mut screens = vec![eda_engine::nets::ScreenIn { path: Vec::new(), sch: &mut copy, file: String::new(), pins }];
    eda_engine::nets::trace_nets_with(&mut screens, &eda_engine::nets::Naming::default())
}

fn net_of(nets: &[TracedNet], pin: &str) -> String {
    nets.iter().find(|t| t.pins.iter().any(|p| p == pin)).map(|t| t.name.clone()).unwrap_or_default()
}

fn nets(list: &[(&str, &[&str])]) -> Vec<Net> {
    list.iter().map(|(n, pins)| Net { name: n.to_string(), pins: pins.iter().map(|p| p.to_string()).collect() }).collect()
}

fn texts(plan: &NetBackAnnotation) -> Vec<(String, String)> {
    plan.lines.iter().map(|l| (l.severity.to_string(), l.text.clone())).collect()
}

/// Runs the verbs of the plan on the design, as the dialog's batch does.
fn apply(design: Design, m: &ConstraintModel, plan: &NetBackAnnotation) -> Design {
    let mut b = Board::new(design, m, mm(0.1), mm(0.3));
    for c in &plan.cmds {
        b.apply(c).unwrap_or_else(|e| panic!("{c:?}: {e:?}"));
    }
    b.design().clone()
}

/// U1 (`Test:TWO`) with a label on each pin: pin 1 on A, pin 2 on B.
fn labelled_two() -> (Design, ConstraintModel) {
    let m = model();
    let mut s = section();
    s.symbols.push(sym("U1", "Test:TWO", 1, p(50.8, 50.8)));
    wire_out(&mut s, &m, "U1.1", Some("A"));
    wire_out(&mut s, &m, "U1.2", Some("B"));
    (design_of(s), m)
}

#[test]
fn a_swapped_pair_with_pin_swaps_off_is_said_and_the_labels_are_renamed() {
    let (d, m) = labelled_two();
    let traced = drawn(&d, &m);
    assert_eq!((net_of(&traced, "U1.1"), net_of(&traced, "U1.2")), ("/A".to_string(), "/B".to_string()));
    // Swap Pad Nets on the board: pad 1 is on /B now, pad 2 on /A
    let board = nets(&[("/A", &["U1.2"]), ("/B", &["U1.1"])]);
    let plan = plan_net_back_annotation(&d, &m, &board, &traced, &BackAnnotateOptions::default());
    assert_eq!(
        texts(&plan),
        [
            ("info".into(), "Would swap U1 pins 1 and 2 to match PCB, but unconstrained pin swaps are disabled in the schematic preferences.".into()),
            ("action".into(), "Change U1 pin 1 net label from 'A' to 'B'.".into()),
            ("action".into(), "Change U1 pin 2 net label from 'B' to 'A'.".into()),
        ]
    );
    assert_eq!(plan.cmds.len(), 2, "{:?}", plan.cmds);
    let after = apply(d, &m, &plan);
    let traced = drawn(&after, &m);
    assert_eq!((net_of(&traced, "U1.1"), net_of(&traced, "U1.2")), ("/B".to_string(), "/A".to_string()), "the pins are on the board's nets");
    // and there is nothing left to do
    let board = nets(&[("/A", &["U1.2"]), ("/B", &["U1.1"])]);
    assert!(plan_net_back_annotation(&after, &m, &board, &traced, &BackAnnotateOptions::default()).lines.is_empty());
}

#[test]
fn with_unconstrained_pin_swaps_allowed_the_pins_trade_places_and_the_labels_stay() {
    let (d, m) = labelled_two();
    let traced = drawn(&d, &m);
    let board = nets(&[("/A", &["U1.2"]), ("/B", &["U1.1"])]);
    let opts = BackAnnotateOptions { allow_pin_swaps: true, ..Default::default() };
    let plan = plan_net_back_annotation(&d, &m, &board, &traced, &opts);
    assert_eq!(texts(&plan), [("action".to_string(), "Swap U1 pins 1 and 2 to match PCB.".to_string())]);
    let after = apply(d, &m, &plan);
    let sch = after.schematic.as_ref().unwrap();
    assert_eq!(sch.labels.iter().map(|l| l.net.as_str()).collect::<Vec<_>>(), ["A", "B"], "the labels keep their text");
    assert_eq!(sch.extras.pin_swaps["U1"].len(), 2);
    let traced = drawn(&after, &m);
    assert_eq!((net_of(&traced, "U1.1"), net_of(&traced, "U1.2")), ("/B".to_string(), "/A".to_string()));
    // "Prefer symbol pin swaps" off: the labels again
    let plan = plan_net_back_annotation(&design_of(labelled_two().0.schematic.unwrap()), &m, &board, &drawn(&labelled_two().0, &m), &BackAnnotateOptions { allow_pin_swaps: true, prefer_pin_swaps: false, ..Default::default() });
    assert_eq!(plan.lines.len(), 2);
    assert!(plan.lines.iter().all(|l| l.text.contains("net label")), "{:?}", plan.lines);
}

#[test]
fn a_pin_with_no_label_gets_one_and_a_power_net_is_not_changed() {
    let m = model();
    let mut s = section();
    s.symbols.push(sym("U1", "Test:TWO", 1, p(50.8, 50.8)));
    // pin 1 on a bare wire, pin 2 on a GND symbol
    wire_out(&mut s, &m, "U1.1", None);
    let tip2 = tips(&s, &m)["U1.2"];
    s.power_symbols.push(PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: tip2, rot: 0, net: "GND".into(), pin: String::new() });
    let d = design_of(s);
    let traced = drawn(&d, &m);
    assert_eq!(net_of(&traced, "U1.2"), "GND");
    let before_u1_1 = net_of(&traced, "U1.1");
    let board = nets(&[("SIG", &["U1.1"]), ("VCC", &["U1.2"])]);
    let plan = plan_net_back_annotation(&d, &m, &board, &traced, &BackAnnotateOptions::default());
    assert_eq!(
        texts(&plan),
        [
            ("action".into(), "Add label 'SIG' to U1 pin 1 net.".into()),
            ("error".into(), "Net GND cannot be changed to VCC because it is driven by a power pin.".into()),
        ],
        "U1.1 was {before_u1_1}"
    );
    let after = apply(d, &m, &plan);
    let sch = after.schematic.as_ref().unwrap();
    let label = sch.labels.iter().find(|l| l.net == "SIG").expect("the new label");
    assert_eq!(label.at, tips(sch, &m)["U1.1"], "on the pin (`driver->GetPosition()`)");
    assert_eq!(sch.extras.label_spins.get(&label.id), Some(&LabelSpin::Left), "it reads away from a pin that runs right (`orientLabel`)");
    assert_eq!(net_of(&drawn(&after, &m), "U1.1"), "/SIG");
}

#[test]
fn a_pad_with_no_pin_is_reported_and_net_names_off_does_nothing() {
    let (d, m) = labelled_two();
    let traced = drawn(&d, &m);
    let board = nets(&[("/A", &["U1.1"]), ("/B", &["U1.2"]), ("GND", &["U1.EP"])]);
    let plan = plan_net_back_annotation(&d, &m, &board, &traced, &BackAnnotateOptions::default());
    assert_eq!(texts(&plan), [("error".to_string(), "Cannot find U1 pin 'EP'.".to_string())]);
    assert!(plan.cmds.is_empty());
    let off = BackAnnotateOptions { net_names: false, ..Default::default() };
    let swapped = nets(&[("/A", &["U1.2"]), ("/B", &["U1.1"])]);
    let plan = plan_net_back_annotation(&d, &m, &swapped, &traced, &off);
    assert!(plan.lines.is_empty() && plan.cmds.is_empty());
}

/// U1 (`Test:DUAL`) placed as its two units, each pin labelled: unit A's 1 and 2 on P and Q, unit B's 3 and 4 on R and S.
fn labelled_dual() -> (Design, ConstraintModel) {
    let m = model();
    let mut s = section();
    s.symbols.push(sym("U1", "Test:DUAL", 1, p(50.8, 50.8)));
    s.symbols.push(sym("U1", "Test:DUAL", 2, p(50.8, 76.2)));
    wire_out(&mut s, &m, "U1.1", Some("P"));
    wire_out(&mut s, &m, "U1.2", Some("Q"));
    wire_out(&mut s, &m, "U1.3", Some("R"));
    wire_out(&mut s, &m, "U1.4", Some("S"));
    (design_of(s), m)
}

#[test]
fn a_gate_swap_on_the_board_swaps_the_units_and_leaves_the_labels() {
    let (d, m) = labelled_dual();
    let traced = drawn(&d, &m);
    assert_eq!(["U1.1", "U1.2", "U1.3", "U1.4"].map(|p| net_of(&traced, p)), ["/P", "/Q", "/R", "/S"]);
    // Swap Gate Nets: the pads of unit A take the nets of unit B, position by position, and the other way round
    let board = nets(&[("/P", &["U1.3"]), ("/Q", &["U1.4"]), ("/R", &["U1.1"]), ("/S", &["U1.2"])]);
    let plan = plan_net_back_annotation(&d, &m, &board, &traced, &BackAnnotateOptions::default());
    assert_eq!(texts(&plan), [("action".to_string(), "Swap U1 unit A with unit B.".to_string())]);
    let after = apply(d, &m, &plan);
    let sch = after.schematic.as_ref().unwrap();
    assert_eq!(sch.symbols.iter().map(|s| (s.at, s.unit)).collect::<Vec<_>>(), [(p(50.8, 50.8), 2), (p(50.8, 76.2), 1)], "the symbol at A's place draws B now");
    let traced = drawn(&after, &m);
    assert_eq!(["U1.1", "U1.2", "U1.3", "U1.4"].map(|p| net_of(&traced, p)), ["/R", "/S", "/P", "/Q"], "every pin is on its pad's net");
    // Unit swaps off: the labels are renamed instead
    let (d, m) = labelled_dual();
    let plan = plan_net_back_annotation(&d, &m, &board, &drawn(&d, &m), &BackAnnotateOptions { prefer_unit_swaps: false, ..Default::default() });
    assert!(plan.lines.iter().any(|l| l.text == "Change U1 pin 1 net label from 'P' to 'R'."), "{:?}", plan.lines);
    assert!(plan.cmds.iter().all(|c| matches!(c, Cmd::OnSheet { cmd, .. } if matches!(**cmd, Cmd::SchEdit(SchCmd::EditLabel { .. })))), "{:?}", plan.cmds);
}

#[test]
fn a_net_the_board_renames_without_a_swap_renames_the_label_once() {
    let (d, m) = labelled_two();
    let traced = drawn(&d, &m);
    let board = nets(&[("/A2", &["U1.1"]), ("/B", &["U1.2"])]);
    let plan = plan_net_back_annotation(&d, &m, &board, &traced, &BackAnnotateOptions::default());
    assert_eq!(texts(&plan), [("action".to_string(), "Change U1 pin 1 net label from 'A' to 'A2'.".to_string())]);
    let after = apply(d, &m, &plan);
    assert_eq!(net_of(&drawn(&after, &m), "U1.1"), "/A2");
}

#[test]
fn the_short_name_is_what_follows_the_last_slash() {
    assert_eq!(short_name("/Sheet/A"), "A");
    assert_eq!(short_name("GND"), "GND");
    assert_eq!(short_name("/Net-(R1-Pad1)"), "Net-(R1-Pad1)");
    let _ = Nm::ZERO;
}

/// UI-LEFT-3 P3-34: the units of one footprint on two sheets swap too (`SetUnit` and `SetUnitSelection( sheet, .. )` on each one's own sheet): unit A of U1 on the
/// root, unit B in the sheet `Sub`, the board's gate swap carried back as one `SetSymbolUnit` on each sheet.
#[test]
fn units_on_two_sheets_are_swapped_each_on_its_own_sheet() {
    let m = model();
    let mut root = section();
    root.symbols.push(sym("U1", "Test:DUAL", 1, p(50.8, 50.8)));
    wire_out(&mut root, &m, "U1.1", Some("P"));
    wire_out(&mut root, &m, "U1.2", Some("Q"));
    root.sheets.push(eda_model::ir::SheetInstance { id: "sh1".into(), name: "Sub".into(), file: "sub.kicad_sch".into(), at: p(150.0, 20.0), size: (mm(30.0), mm(20.0)), pins: vec![], page: "2".into() });
    let mut sub = section();
    sub.symbols.push(sym("U1", "Test:DUAL", 2, p(50.8, 76.2)));
    wire_out(&mut sub, &m, "U1.3", Some("R"));
    wire_out(&mut sub, &m, "U1.4", Some("S"));
    sub.assign_missing_ids();
    let mut d = design_of(root);
    d.sheet_contents = Some([("sub.kicad_sch".to_string(), sub)].into_iter().collect());
    // what the drawing traces to (the sheet's local labels under its path)
    let t = |name: &str, pin: &str| TracedNet { name: name.into(), pins: vec![pin.into()], driven: true };
    let traced = vec![t("/P", "U1.1"), t("/Q", "U1.2"), t("/Sub/R", "U1.3"), t("/Sub/S", "U1.4")];
    let board = nets(&[("/Sub/R", &["U1.1"]), ("/Sub/S", &["U1.2"]), ("/P", &["U1.3"]), ("/Q", &["U1.4"])]);
    let plan = plan_net_back_annotation(&d, &m, &board, &traced, &BackAnnotateOptions::default());
    assert_eq!(texts(&plan), [("action".to_string(), "Swap U1 unit A with unit B.".to_string())]);
    assert_eq!(plan.cmds.len(), 2, "one on each sheet: {:?}", plan.cmds);
    let after = apply(d, &m, &plan);
    assert_eq!(after.schematic.as_ref().unwrap().symbols[0].unit, 2, "the root's U1 draws B");
    assert_eq!(after.sheet_contents.as_ref().unwrap()["sub.kicad_sch"].symbols[0].unit, 1, "the sheet's draws A");
}

/// `GetDriverPriority`: a hidden power input of an ordinary symbol drives its net as a global power pin (`SCH_PIN::IsGlobalPower`'s legacy rule), so the net
/// is not renamed; "Net %s cannot be changed to %s because it is driven by a power pin."
#[test]
fn a_hidden_power_pin_of_an_ordinary_symbol_drives_its_net() {
    let mut m = model();
    let mut vdd = lib_pin("3", 0.0, 1, "power_in");
    vdd.hidden = true;
    m.symbols.push(lib("Test:REG", 1, vec![lib_pin("1", 2.54, 1, "passive"), lib_pin("2", -2.54, 1, "passive"), vdd], false));
    let mut s = section();
    s.symbols.push(sym("U1", "Test:TWO", 1, p(50.8, 50.8)));
    s.symbols.push(sym("U2", "Test:REG", 1, p(101.6, 50.8)));
    // U1.1 wired to U2's hidden power pin 3 (at U2's origin)
    let from = tips(&s, &m)["U1.1"];
    let to = tips(&s, &m)["U2.3"];
    s.wires.push(Wire { id: String::new(), net: String::new(), pins: vec![], pts: vec![from, Point { x: from.x - mm(5.08), y: from.y }, Point { x: from.x - mm(5.08), y: to.y }, to], bus: false });
    let d = design_of(s);
    let traced = vec![TracedNet { name: "VDD".into(), pins: vec!["U1.1".into(), "U2.3".into()], driven: true }];
    let board = nets(&[("VCC", &["U1.1"])]);
    let plan = plan_net_back_annotation(&d, &m, &board, &traced, &BackAnnotateOptions::default());
    assert_eq!(texts(&plan), [("error".to_string(), "Net VDD cannot be changed to VCC because it is driven by a power pin.".to_string())]);
    assert!(plan.cmds.is_empty());
}

/// UI-LEFT-3 P3-14, `applyChangelist`'s "Other fields": the footprint's fields carried to its symbol -- a changed one set, one the symbol lacks added hidden, a user
/// field the footprint lacks deleted, Reference and Value passed over, a field naming a variable left alone.
#[test]
fn the_footprints_fields_are_carried_to_its_symbol() {
    let m = model();
    let mut s = section();
    s.symbols.push(sym("U1", "Test:TWO", 1, p(50.8, 50.8)));
    s.user_fields.insert("U1".into(), [("MPN".to_string(), "X1".to_string()), ("Old".to_string(), "gone".to_string()), ("Rev".to_string(), "${REV}".to_string())].into_iter().collect());
    let d = design_of(s);
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let fields: BTreeMap<String, String> = [("Reference", "U9"), ("Value", "v"), ("Datasheet", "ds.pdf"), ("Description", "a part"), ("MPN", "X2"), ("Vendor", "ACME"), ("Rev", "B")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    b.apply(&Cmd::CarrySymbolFields { id: "U1".into(), fields }).unwrap();
    let sch = b.design().schematic.as_ref().unwrap();
    let user = &sch.user_fields["U1"];
    assert_eq!(user.get("MPN").map(String::as_str), Some("X2"), "changed");
    assert_eq!(user.get("Vendor").map(String::as_str), Some("ACME"), "added");
    assert_eq!(user.get("Rev").map(String::as_str), Some("${REV}"), "a field naming a variable is left alone");
    assert!(!user.contains_key("Old"), "deleted: the footprint has no such field");
    assert_eq!(user.get("Description").map(String::as_str), Some("a part"));
    assert_eq!(sch.symbols[0].datasheet, "ds.pdf");
    assert_eq!(sch.symbols[0].id, "U1", "the Reference is not this command's");
    assert!(!sch.field_layout.contains_key(&eda_model::ir::field_key("U1", 1)), "the added field has no place: hidden at the symbol");
    let err = b.apply(&Cmd::CarrySymbolFields { id: "U7".into(), fields: BTreeMap::new() }).unwrap_err();
    assert_eq!(err[0].check, "ops_unknown_item");
}
