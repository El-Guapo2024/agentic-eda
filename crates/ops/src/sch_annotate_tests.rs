//! `Cmd::Annotate` over a hierarchy: scope, order, numbering algorithm, reset or keep, multi-unit symbols, the reports and the checks.
use super::*;
use eda_model::ir::{Provenance, SheetInstance};
use eda_model::units::mm;

pub(crate) fn p(x: Nm, y: Nm) -> Point {
    Point { x, y }
}

pub(crate) fn symbol(id: &str, lib_id: &str, value: &str, unit: u32, at: Point) -> SymbolInstance {
    SymbolInstance { id: id.into(), at, rot: 0, mirrored: false, mirror_y: false, lib_id: lib_id.into(), unit, value: value.into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false }
}

pub(crate) fn placement(id: &str, name: &str, file: &str, page: &str) -> SheetInstance {
    SheetInstance { id: id.into(), name: name.into(), file: file.into(), at: p(Nm::ZERO, Nm::ZERO), size: (mm(50.0), mm(30.0)), pins: vec![], page: page.into() }
}

pub(crate) fn section(symbols: Vec<SymbolInstance>, sheets: Vec<SheetInstance>) -> SchematicSection {
    SchematicSection { symbols, sheets, ..Default::default() }
}

/// A two-unit symbol the way a library has it: pins 1 and 2 on unit 1, pins 3 and 4 on unit 2.
fn dual() -> LibrarySymbol {
    use eda_model::ir::LibrarySymbolPin;
    let pin = |number: &str, unit: u32, y: f64| LibrarySymbolPin { id: String::new(), number: number.into(), name: String::new(), electrical_type: "passive".into(), shape: "line".into(), at: eda_model::symbol::SPoint { x: 0.0, y }, angle_deg: 270.0, length_mm: 2.54, unit, body_style: 1, hidden: false, name_size_mm: None, number_size_mm: None, alternates: Vec::new() };
    let mut s = LibrarySymbol { lib_id: "Test:Dual".into(), reference_prefix: "U".into(), unit_count: 2, published: true, pins: vec![pin("1", 1, 2.54), pin("2", 1, -2.54), pin("3", 2, 2.54), pin("4", 2, -2.54)], ..LibrarySymbol::default() };
    s.assign_missing_ids();
    s
}

/// root -> A (a.kicad_sch) -> C (c.kicad_sch), root -> B (b.kicad_sch).
///
/// A: R5 at x=30, R9 at x=10, R? at x=50.  B: R1 at x=20, C? at x=0.  C: R? at x=0.
pub(crate) fn design() -> Design {
    let r = |id: &str, x: Nm| symbol(id, "Device:R", "10k", 1, p(x, Nm::ZERO));
    let mut contents = BTreeMap::new();
    contents.insert("a.kicad_sch".to_string(), section(vec![r("R5", mm(0.03)), r("R9", mm(0.01)), r("R?", mm(0.05))], vec![placement("sc", "C", "c.kicad_sch", "")]));
    contents.insert("b.kicad_sch".to_string(), section(vec![r("R1", mm(0.02)), symbol("C?", "Device:C", "100n", 1, p(Nm::ZERO, Nm::ZERO))], vec![]));
    contents.insert("c.kicad_sch".to_string(), section(vec![symbol("R?", "Device:R", "1k", 1, p(Nm::ZERO, Nm::ZERO))], vec![]));
    Design {
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: Some(section(vec![], vec![placement("sa", "A", "a.kicad_sch", ""), placement("sb", "B", "b.kicad_sch", "")])),
        nets: None,
        placement: None,
        routing: None,
        drawings: None,
        footprint_library: None,
        sheet_contents: Some(contents),
        bus_aliases: vec![],
        symbol_library: None,
    }
}

fn annotate(reset: bool, order: AnnotateOrder, ids: Option<Vec<String>>, options: AnnotateOptions) -> Cmd {
    Cmd::Annotate { reset_existing: reset, order, ids, options }
}

fn all(algo: eda_model::annotate::Algo, start: i64) -> AnnotateOptions {
    AnnotateOptions { scope: Some(AnnotateScope::All), algo, start_number: start, ..Default::default() }
}

pub(crate) fn on(sheet: &str, cmd: Cmd) -> Cmd {
    Cmd::OnSheet { sheet: sheet.into(), cmd: Box::new(cmd) }
}

pub(crate) fn at<'a>(d: &'a Design, file: &str, x: Nm) -> &'a SymbolInstance {
    d.sheet_contents.as_ref().unwrap()[file].symbols.iter().find(|s| s.at.x == x).unwrap_or_else(|| panic!("no symbol at x={x} on {file}"))
}

use eda_model::annotate::Algo;

#[test]
fn a_new_reference_takes_the_first_free_number_over_every_sheet() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    b.apply(&annotate(false, AnnotateOrder::YThenX, None, all(Algo::FirstFree, 0))).unwrap();
    let d = b.design();
    // R1 is on B, R5 and R9 on A: A's R? is the first free R, then C's, and the capacitor is the first C
    assert_eq!(at(d, "a.kicad_sch", mm(0.05)).id, "R2");
    assert_eq!(at(d, "c.kicad_sch", Nm::ZERO).id, "R3");
    assert_eq!(at(d, "b.kicad_sch", Nm::ZERO).id, "C1");
    assert_eq!(at(d, "a.kicad_sch", mm(0.03)).id, "R5", "an annotated reference is kept");
}

#[test]
fn reset_numbers_the_entire_schematic_by_sheet_then_position() {
    let m = ConstraintModel::default();
    let order = |o: AnnotateOrder, algo: Algo, start: i64| {
        let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
        b.apply(&annotate(true, o, None, all(algo, start))).unwrap();
        b.into_design()
    };
    // the sheets count in the order they are walked: A is page 2, C (under A) page 3, B page 4
    let d = order(AnnotateOrder::XThenY, Algo::FirstFree, 0);
    assert_eq!(
        [at(&d, "a.kicad_sch", mm(0.01)).id.as_str(), at(&d, "a.kicad_sch", mm(0.03)).id.as_str(), at(&d, "a.kicad_sch", mm(0.05)).id.as_str(), at(&d, "c.kicad_sch", Nm::ZERO).id.as_str(), at(&d, "b.kicad_sch", mm(0.02)).id.as_str()],
        ["R1", "R2", "R3", "R4", "R5"]
    );
    assert_eq!(at(&d, "b.kicad_sch", Nm::ZERO).id, "C1");
    let d = order(AnnotateOrder::XThenY, Algo::SheetX100, 0);
    assert_eq!(
        [at(&d, "a.kicad_sch", mm(0.01)).id.as_str(), at(&d, "a.kicad_sch", mm(0.03)).id.as_str(), at(&d, "a.kicad_sch", mm(0.05)).id.as_str(), at(&d, "c.kicad_sch", Nm::ZERO).id.as_str(), at(&d, "b.kicad_sch", mm(0.02)).id.as_str()],
        ["R201", "R202", "R203", "R301", "R401"],
        "sheet number x 100 plus one"
    );
    assert_eq!(at(&d, "b.kicad_sch", Nm::ZERO).id, "C401");
    let d = order(AnnotateOrder::XThenY, Algo::SheetX1000, 0);
    assert_eq!(at(&d, "b.kicad_sch", mm(0.02)).id, "R4001");
    let d = order(AnnotateOrder::XThenY, Algo::FirstFree, 100);
    assert_eq!(at(&d, "a.kicad_sch", mm(0.01)).id, "R101", "first free number after the start number");
    // the other order: a column of one position goes top to bottom (every symbol here is on one row, so X decides either way)
    let d = order(AnnotateOrder::YThenX, Algo::FirstFree, 0);
    assert_eq!(at(&d, "a.kicad_sch", mm(0.01)).id, "R1");
}

#[test]
fn a_sheet_numbered_by_the_user_counts_by_that_page() {
    let m = ConstraintModel::default();
    let mut d = design();
    // page "7" for A: B (page 4 in the walk) now comes first
    d.schematic.as_mut().unwrap().sheets[0].page = "7".into();
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&annotate(true, AnnotateOrder::XThenY, None, all(Algo::SheetX100, 0))).unwrap();
    let d = b.design();
    assert_eq!(at(d, "a.kicad_sch", mm(0.01)).id, "R701");
    assert_eq!(at(d, "b.kicad_sch", mm(0.02)).id, "R401");
    // with first-free numbering the sheets take their numbers in page order
    let mut d = design();
    d.schematic.as_mut().unwrap().sheets[0].page = "7".into();
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&annotate(true, AnnotateOrder::XThenY, None, all(Algo::FirstFree, 0))).unwrap();
    let d = b.design();
    assert_eq!((at(d, "c.kicad_sch", Nm::ZERO).id.as_str(), at(d, "b.kicad_sch", mm(0.02)).id.as_str(), at(d, "a.kicad_sch", mm(0.01)).id.as_str()), ("R1", "R2", "R3"), "page 3, page 4, then page 7");
}

#[test]
fn the_current_sheet_scope_leaves_the_other_sheets_alone_and_recursive_takes_the_ones_under_it() {
    let m = ConstraintModel::default();
    let sheet_only = AnnotateOptions { scope: Some(AnnotateScope::CurrentSheet), ..Default::default() };
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    b.apply(&on("sa", annotate(false, AnnotateOrder::YThenX, None, sheet_only.clone()))).unwrap();
    let d = b.design();
    assert_eq!(at(d, "a.kicad_sch", mm(0.05)).id, "R2");
    assert_eq!(at(d, "c.kicad_sch", Nm::ZERO).id, "R?", "C is under A but not recursive");
    assert_eq!(at(d, "b.kicad_sch", Nm::ZERO).id, "C?", "B is not on the sheet");

    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    b.apply(&on("sa", annotate(false, AnnotateOrder::YThenX, None, AnnotateOptions { recursive: true, ..sheet_only }))).unwrap();
    let d = b.design();
    assert_eq!((at(d, "a.kicad_sch", mm(0.05)).id.as_str(), at(d, "c.kicad_sch", Nm::ZERO).id.as_str()), ("R2", "R3"));
    assert_eq!(at(d, "b.kicad_sch", Nm::ZERO).id, "C?");
}

#[test]
fn the_selection_scope_takes_the_selected_symbols_and_the_sheets_selected_when_recursive() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    // R5 on A is reset: it takes the first free R, 2 (B's R1 holds 1, A's R9 stays)
    let one = AnnotateOptions { scope: Some(AnnotateScope::Selection), ..Default::default() };
    b.apply(&on("sa", annotate(true, AnnotateOrder::YThenX, Some(vec!["R5".into()]), one.clone()))).unwrap();
    let d = b.design();
    assert_eq!(at(d, "a.kicad_sch", mm(0.03)).id, "R2");
    assert_eq!((at(d, "a.kicad_sch", mm(0.01)).id.as_str(), at(d, "a.kicad_sch", mm(0.05)).id.as_str()), ("R9", "R?"), "the rest of the sheet is not in the scope");

    // selecting the sheet symbol C with recursive takes its sheet
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    let with_sheet = AnnotateOptions { recursive: true, sheets: vec!["sc".into()], ..one };
    b.apply(&on("sa", annotate(false, AnnotateOrder::YThenX, Some(vec![]), with_sheet))).unwrap();
    let d = b.design();
    assert_eq!(at(d, "c.kicad_sch", Nm::ZERO).id, "R2");
    assert_eq!(at(d, "a.kicad_sch", mm(0.05)).id, "R?");
}

#[test]
fn what_is_keyed_by_the_reference_follows_the_new_one() {
    let m = ConstraintModel::default();
    let mut d = design();
    {
        let a = d.sheet_contents.as_mut().unwrap().get_mut("a.kicad_sch").unwrap();
        a.wires.push(Wire { id: String::new(), net: "N".into(), pins: vec!["R9.1".into(), "R5.2".into()], pts: vec![p(Nm::ZERO, Nm::ZERO), p(mm(0.01), Nm::ZERO)], bus: false });
        a.user_fields.insert("R9".into(), [("MPN".to_string(), "X".to_string())].into());
        a.field_layout.insert("R9".into(), vec![eda_model::ir::FieldPlacement::at_origin("Reference")]);
        a.extras.locked.push("R9".into());
        // the places Swap Pins gave the pins of R9 are R9's wherever it is named
        a.extras.pin_swaps.insert("R9".into(), [("1".to_string(), "2".to_string()), ("2".to_string(), "1".to_string())].into());
    }
    d.nets = Some(vec![eda_model::Net { name: "N".into(), pins: vec!["R9.1".into(), "R5.2".into()] }]);
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    // reset A's sheet by position: R9 (x=10) is the first, R5 (x=30) the second
    let opts = AnnotateOptions { scope: Some(AnnotateScope::CurrentSheet), ..Default::default() };
    b.apply(&on("sa", annotate(true, AnnotateOrder::XThenY, None, opts))).unwrap();
    let d = b.design();
    let a = &d.sheet_contents.as_ref().unwrap()["a.kicad_sch"];
    assert_eq!((at(d, "a.kicad_sch", mm(0.01)).id.as_str(), at(d, "a.kicad_sch", mm(0.03)).id.as_str()), ("R2", "R3"), "R1 is B's");
    // R9 became R2 and R5 R3 at once -- no pin ever pointed at a name another symbol was taking
    assert_eq!(a.wires[0].pins, vec!["R2.1".to_string(), "R3.2".to_string()]);
    assert!(a.user_fields.contains_key("R2") && !a.user_fields.contains_key("R9"));
    assert!(a.field_layout.contains_key("R2") && !a.field_layout.contains_key("R9"));
    assert_eq!(a.extras.locked, vec!["R2".to_string()]);
    assert!(a.extras.pin_swaps.contains_key("R2") && !a.extras.pin_swaps.contains_key("R9"), "{:?}", a.extras.pin_swaps);
    assert_eq!(d.nets.as_ref().unwrap()[0].pins, vec!["R2.1".to_string(), "R3.2".to_string()], "the net list keeps its nets through the renumbering");
}

fn two_chips() -> Design {
    let u = |id: &str, unit: u32, y: Nm| symbol(id, "Test:Dual", "TL072", unit, p(Nm::ZERO, y));
    let mut d = design();
    d.symbol_library = Some(eda_model::ir::SymbolLibrarySection { symbols: vec![dual()], ..Default::default() });
    let a = d.sheet_contents.as_mut().unwrap().get_mut("a.kicad_sch").unwrap();
    a.symbols = vec![u("U5", 1, Nm::ZERO), u("U3", 2, mm(10.0)), u("U5", 2, mm(20.0)), u("U3", 1, mm(30.0))];
    a.wires.push(Wire { id: String::new(), net: "N".into(), pins: vec!["U3.1".into(), "U3.3".into()], pts: vec![p(Nm::ZERO, Nm::ZERO), p(mm(10.0), Nm::ZERO)], bus: false });
    d
}

#[test]
fn the_units_of_a_multi_unit_symbol_keep_their_reference_between_them() {
    let m = ConstraintModel::default();
    let mut b = Board::new(two_chips(), &m, mm(0.1), mm(0.3));
    let opts = AnnotateOptions { scope: Some(AnnotateScope::CurrentSheet), ..Default::default() };
    b.apply(&on("sa", annotate(true, AnnotateOrder::YThenX, None, opts))).unwrap();
    let a = &b.design().sheet_contents.as_ref().unwrap()["a.kicad_sch"];
    let by_y = |y: Nm, unit: u32| a.symbols.iter().find(|s| s.at.y == y && s.unit == unit).unwrap().id.clone();
    // U5's first unit comes first on the sheet and the part keeps both its units: U5 -> U1; U3 -> U2
    assert_eq!((by_y(Nm::ZERO, 1), by_y(mm(20.0), 2)), ("U1".to_string(), "U1".to_string()));
    assert_eq!((by_y(mm(10.0), 2), by_y(mm(30.0), 1)), ("U2".to_string(), "U2".to_string()));
    assert_eq!(a.wires[0].pins, vec!["U2.1".to_string(), "U2.3".to_string()]);
}

#[test]
fn regroup_units_numbers_the_units_by_where_they_are_placed() {
    let m = ConstraintModel::default();
    let mut b = Board::new(two_chips(), &m, mm(0.1), mm(0.3));
    let opts = AnnotateOptions { scope: Some(AnnotateScope::CurrentSheet), regroup_units: true, ..Default::default() };
    b.apply(&on("sa", annotate(true, AnnotateOrder::YThenX, None, opts))).unwrap();
    let a = &b.design().sheet_contents.as_ref().unwrap()["a.kicad_sch"];
    let by_y = |y: Nm| a.symbols.iter().find(|s| s.at.y == y).unwrap();
    // in placement order: unit 1 takes U1, the next unit (a unit 2) joins U1 where its unit is free, then both are U2's
    assert_eq!((by_y(Nm::ZERO).id.as_str(), by_y(Nm::ZERO).unit), ("U1", 1));
    assert_eq!((by_y(mm(10.0)).id.as_str(), by_y(mm(10.0)).unit), ("U1", 2));
    assert_eq!((by_y(mm(20.0)).id.as_str(), by_y(mm(20.0)).unit), ("U2", 2));
    assert_eq!((by_y(mm(30.0)).id.as_str(), by_y(mm(30.0)).unit), ("U2", 1));
    // the pins of a unit that went to another part follow it, each to the reference of its own unit
    assert_eq!(a.wires[0].pins, vec!["U2.1".to_string(), "U1.3".to_string()]);
}

#[test]
fn the_report_says_what_kicad_says_and_changes_nothing() {
    let m = ConstraintModel::default();
    let b = Board::new(design(), &m, mm(0.1), mm(0.3));
    let report = b.annotate_report("", false, AnnotateOrder::YThenX, None, &all(Algo::FirstFree, 0)).unwrap();
    let texts: Vec<&str> = report.messages.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(report.changed, 3);
    assert!(texts.contains(&"Annotated 10k as R2."), "{texts:?}");
    assert!(texts.contains(&"Annotated 100n as C1."), "{texts:?}");
    assert_eq!(texts.last(), Some(&"Annotation complete."));
    assert!(report.problems.is_empty());
    let sch = b.design().sheet_contents.as_ref().unwrap();
    assert!(sch["a.kicad_sch"].symbols.iter().any(|s| s.id == "R?"), "a dry run leaves the board alone");

    // a reset says "Updated" with the reference it had
    let report = b.annotate_report("", true, AnnotateOrder::XThenY, None, &all(Algo::FirstFree, 0)).unwrap();
    assert!(report.messages.iter().any(|m| m.text == "Updated 10k from R9 to R1."), "{:?}", report.messages);
}

#[test]
fn the_final_check_names_a_unit_the_library_symbol_does_not_have() {
    let m = ConstraintModel::default();
    let mut d = two_chips();
    // unit 3 of a two-unit part
    d.sheet_contents.as_mut().unwrap().get_mut("a.kicad_sch").unwrap().symbols.push(symbol("U7", "Test:Dual", "TL072", 3, p(Nm::ZERO, mm(50.0))));
    let b = Board::new(d, &m, mm(0.1), mm(0.3));
    let report = b.annotate_report("sa", false, AnnotateOrder::YThenX, None, &AnnotateOptions { scope: Some(AnnotateScope::CurrentSheet), ..Default::default() }).unwrap();
    let errors: Vec<&str> = report.messages.iter().filter(|m| m.severity == "error").map(|m| m.text.as_str()).collect();
    assert_eq!(errors, vec!["Error: symbol U7C (unit 3) exceeds units defined (2)"]);
    assert!(!report.messages.iter().any(|m| m.text == "Annotation complete."));
}

/// `CheckAnnotate` in the Selection scope looks at the selected symbols as the numbering left them (`getInferredSymbols`: the same objects), not at the references
/// they had: a selected unit 3 of a two-unit part, numbered again, is still found and named by its new reference.
#[test]
fn the_final_check_of_a_selection_follows_the_symbols_it_renumbered() {
    let m = ConstraintModel::default();
    let mut d = two_chips();
    d.sheet_contents.as_mut().unwrap().get_mut("a.kicad_sch").unwrap().symbols.push(symbol("U7", "Test:Dual", "TL072", 3, p(Nm::ZERO, mm(50.0))));
    let b = Board::new(d, &m, mm(0.1), mm(0.3));
    let opts = AnnotateOptions { scope: Some(AnnotateScope::Selection), ..Default::default() };
    let report = b.annotate_report("sa", true, AnnotateOrder::YThenX, Some(&["U7".to_string()]), &opts).unwrap();
    assert!(report.changed > 0, "U7 was numbered again: {:?}", report.messages);
    let errors: Vec<&str> = report.messages.iter().filter(|m| m.severity == "error").map(|m| m.text.as_str()).collect();
    assert_eq!(errors.len(), 1, "{:?}", report.messages);
    assert!(errors[0].ends_with("(unit 3) exceeds units defined (2)") && !errors[0].contains("U7"), "{errors:?}");
    assert!(!report.messages.iter().any(|m| m.text == "Annotation complete."));
}

#[test]
fn power_symbols_are_numbered_with_the_schematic_and_a_leading_zero() {
    let m = ConstraintModel::default();
    let mut d = design();
    let power = |id: &str| eda_model::ir::PowerSymbol { id: id.into(), lib_id: "power:GND".into(), at: p(Nm::ZERO, mm(0.09)), rot: 0, net: "GND".into(), pin: String::new() };
    d.sheet_contents.as_mut().unwrap().get_mut("a.kicad_sch").unwrap().power_symbols.push(power("#PWR01"));
    d.sheet_contents.as_mut().unwrap().get_mut("b.kicad_sch").unwrap().power_symbols.push(power("#PWR01"));
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&annotate(true, AnnotateOrder::YThenX, None, all(Algo::FirstFree, 0))).unwrap();
    let sch = b.design().sheet_contents.as_ref().unwrap();
    let ids: Vec<&str> = ["a.kicad_sch", "b.kicad_sch"].iter().map(|f| sch[*f].power_symbols[0].id.as_str()).collect();
    assert_eq!(ids, vec!["#PWR01", "#PWR02"], "no two share an id any more, and the sheet in page order numbers first");
}

#[test]
fn the_command_reads_as_it_did_before_the_dialog_had_a_scope() {
    let old = serde_json::json!({ "op": "annotate", "reset_existing": true, "order": "x_then_y", "ids": ["R1"] });
    let cmd: Cmd = serde_json::from_value(old).unwrap();
    assert!(matches!(&cmd, Cmd::Annotate { reset_existing: true, order: AnnotateOrder::XThenY, ids: Some(i), options } if i == &vec!["R1".to_string()] && *options == AnnotateOptions::default()));
    let new = serde_json::json!({ "op": "annotate", "reset_existing": false, "options": { "scope": "all", "algo": "sheet_x100", "start_number": 5, "recursive": true, "regroup_units": true, "sheets": ["s1"] } });
    let cmd: Cmd = serde_json::from_value(new).unwrap();
    let Cmd::Annotate { options, .. } = cmd else { panic!() };
    assert_eq!((options.scope, options.algo, options.start_number, options.recursive, options.regroup_units, options.sheets.len()), (Some(AnnotateScope::All), Algo::SheetX100, 5, true, true, 1));
}
