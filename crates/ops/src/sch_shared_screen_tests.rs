//! A screen shown by two sheets (UI-LEFT S9): Annotate numbers every sheet path's references (`SCH_SHEET_LIST::GetSymbols`, `SCH_REFERENCE::Annotate`'s
//! `SetRef( &m_sheetPath, .. )`), a power symbol has a reference per path, a unit edit is the path in view's (`SetUnitSelection( &GetCurrentSheet(), .. )`),
//! and Do not Populate collects the units of the reference the path in view gives the symbol (`SetAttribute`, `CollectOtherUnits`).
use super::sch_annotate_tests::{dual, on, p, placement, section, symbol};
use super::*;
use eda_model::annotate::Algo;
use eda_model::ir::SymbolPathOverride;
use eda_model::sheet_list;
use eda_model::units::mm;

/// The root places the channel screen `ch.kicad_sch` twice: `sa` (page 2) and `sb` (page 3). The screen holds R1 (x = 10 mm) and R2 (x = 20 mm), which
/// the second placement names R3 and R4, and the power symbol #PWR01 (no per-path reference: both paths show #PWR01).
fn channels() -> Design {
    let mut d = super::sch_annotate_tests::design();
    let r = |id: &str, x: Nm| symbol(id, "Device:R", "10k", 1, p(x, Nm::ZERO));
    let mut ch = section(vec![r("R1", mm(10.0)), r("R2", mm(20.0))], vec![]);
    ch.instance_overrides = vec![
        SymbolPathOverride { at: p(mm(10.0), Nm::ZERO), sheet_path: vec!["sb".into()], reference: "R3".into(), unit: 1 },
        SymbolPathOverride { at: p(mm(20.0), Nm::ZERO), sheet_path: vec!["sb".into()], reference: "R4".into(), unit: 1 },
    ];
    ch.power_symbols = vec![PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: p(mm(10.0), mm(10.0)), rot: 0, net: "GND".into(), pin: String::new() }];
    let mut contents = BTreeMap::new();
    contents.insert("ch.kicad_sch".to_string(), ch);
    d.sheet_contents = Some(contents);
    let mut sa = placement("sa", "A", "ch.kicad_sch", "");
    sa.at = p(mm(100.0), Nm::ZERO);
    let mut sb = placement("sb", "B", "ch.kicad_sch", "");
    sb.at = p(mm(200.0), Nm::ZERO);
    d.schematic = Some(section(vec![], vec![sa, sb]));
    d
}

/// The references the sheet at `path` gives the symbols of its screen, sorted.
fn refs_on(d: &Design, path: &[&str]) -> Vec<String> {
    let path: Vec<String> = path.iter().map(|s| s.to_string()).collect();
    let screen = sheet_list::screen_of(d, &path).expect("a sheet at the path");
    let mut r: Vec<String> = screen.symbols.iter().map(|s| sheet_list::reference_under(screen, s, &path)).collect();
    r.sort();
    r
}

fn power_on(d: &Design, path: &[&str]) -> Vec<String> {
    let path: Vec<String> = path.iter().map(|s| s.to_string()).collect();
    let screen = sheet_list::screen_of(d, &path).expect("a sheet at the path");
    let mut r: Vec<String> = screen.power_symbols.iter().map(|ps| sheet_list::power_reference_under(screen, ps, &path)).collect();
    r.sort();
    r
}

fn annotate(reset: bool, scope: AnnotateScope, algo: Algo) -> Cmd {
    Cmd::Annotate { reset_existing: reset, order: AnnotateOrder::XThenY, ids: None, options: AnnotateOptions { scope: Some(scope), algo, ..Default::default() } }
}

/// "Current sheet only" on the second placement numbers that placement's references (`currentSheet.GetSymbols( references, .. )` with the `SCH_SHEET_PATH`
/// in view), not the screen's own -- which are the first placement's. Before, the scope was the screen, so the first placement's were renumbered.
#[test]
fn annotate_on_the_second_placement_numbers_that_placements_references() {
    let m = ConstraintModel::default();
    let mut d = channels();
    // the second placement's references out of order: a reset gives them the first free numbers after the first placement's
    let ch = d.sheet_contents.as_mut().unwrap().get_mut("ch.kicad_sch").unwrap();
    ch.instance_overrides[0].reference = "R9".into();
    ch.instance_overrides[1].reference = "R7".into();
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&on("sb", annotate(true, AnnotateScope::CurrentSheet, Algo::FirstFree))).unwrap();
    assert_eq!(refs_on(b.design(), &["sa"]), ["R1", "R2"], "the first placement is not in the scope");
    assert_eq!(refs_on(b.design(), &["sb"]), ["R3", "R4"], "the second placement is numbered by place from the first free number");
    // the screen's own references (the first placement's) are its symbols' ids still
    let ids: Vec<String> = b.design().sheet_contents.as_ref().unwrap()["ch.kicad_sch"].symbols.iter().map(|s| s.id.clone()).collect();
    assert_eq!(ids, ["R1", "R2"]);
}

/// "Entire schematic" with a reset numbers a reference per path, by sheet number (the page) then place (`SortByXPosition`), with "sheet number x 100" each
/// path from its own page: the first placement R201 R202, the second R301 R302 (before, the second placement kept whatever it had).
#[test]
fn annotate_all_numbers_every_placement_of_a_shared_screen_by_its_page() {
    let m = ConstraintModel::default();
    let mut b = Board::new(channels(), &m, mm(0.1), mm(0.3));
    b.apply(&annotate(true, AnnotateScope::All, Algo::SheetX100)).unwrap();
    assert_eq!(refs_on(b.design(), &["sa"]), ["R201", "R202"]);
    assert_eq!(refs_on(b.design(), &["sb"]), ["R301", "R302"]);
    // and the power symbol: one reference per path (`SYMBOL_FILTER_ALL`), from each page
    assert_eq!(power_on(b.design(), &["sa"]), ["#PWR0201"]);
    assert_eq!(power_on(b.design(), &["sb"]), ["#PWR0301"]);
}

/// The report names each path's reference as KiCad's does (`Updated %s from %s to %s.` with `GetRef( sheet )`).
#[test]
fn the_annotate_report_names_each_paths_reference() {
    let m = ConstraintModel::default();
    let b = Board::new(channels(), &m, mm(0.1), mm(0.3));
    let opts = AnnotateOptions { scope: Some(AnnotateScope::All), algo: Algo::SheetX100, ..Default::default() };
    let report = b.annotate_report("", true, AnnotateOrder::XThenY, None, &opts).unwrap();
    let texts: Vec<&str> = report.messages.iter().map(|m| m.text.as_str()).collect();
    assert!(texts.contains(&"Updated 10k from R3 to R301."), "{texts:?}");
    assert!(texts.contains(&"Updated 10k from R1 to R201."), "{texts:?}");
    assert!(texts.contains(&"Annotation complete."), "{texts:?}");
}

/// A screen whose two paths named its symbols by one id (an old design: no override for the second path) is told apart by a reset of the annotation:
/// the first path keeps the id, the second gets a reference of its own. Without the reset KiCad keeps the duplicates (only a new reference is
/// numbered) and reports them, which the final check does here too.
#[test]
fn annotate_all_gives_a_path_that_shared_the_symbols_id_a_reference_of_its_own() {
    let m = ConstraintModel::default();
    let mut d = channels();
    d.sheet_contents.as_mut().unwrap().get_mut("ch.kicad_sch").unwrap().instance_overrides.clear();
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let keep = AnnotateOptions { scope: Some(AnnotateScope::All), ..Default::default() };
    let report = b.annotate_report("", false, AnnotateOrder::XThenY, None, &keep).unwrap();
    assert!(report.problems.iter().any(|p| p.message.contains("R1")), "the duplicate is reported: {:?}", report.messages);
    b.apply(&annotate(true, AnnotateScope::All, Algo::FirstFree)).unwrap();
    let (a, bb) = (refs_on(b.design(), &["sa"]), refs_on(b.design(), &["sb"]));
    assert_eq!(a, ["R1", "R2"], "the first path keeps the symbols' own references");
    assert_eq!(bb, ["R3", "R4"], "the second path's duplicates are numbered anew");
}

/// A power symbol placed on a screen shown twice: the first path gets the placement's number, every other path a number of its own (`PlaceSymbol`'s
/// `annotate` over `FindAllSheetsForScreen`), so kicad-cli's annotation check finds no duplicate.
#[test]
fn a_power_symbol_placed_on_a_shared_screen_has_a_reference_per_path() {
    let m = ConstraintModel::default();
    let mut b = Board::new(channels(), &m, mm(0.1), mm(0.3));
    let add = Cmd::AddPowerSymbol { lib_id: "power:GND".into(), at: p(mm(30.0), mm(10.0)), rot_millideg: 0, net: "GND".into(), pin: String::new(), annotate: None };
    b.apply(&on("sa", add)).unwrap();
    let (a, bb) = (power_on(b.design(), &["sa"]), power_on(b.design(), &["sb"]));
    assert_eq!(a.len(), 2);
    assert_eq!(bb.len(), 2);
    let mut all: Vec<String> = a.iter().chain(&bb).cloned().collect();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 3, "#PWR01 is on both paths (it was placed before), the new one has a number per path: {a:?} {bb:?}");
    // a power symbol deleted takes its paths' references with it
    let new_id = b.design().sheet_contents.as_ref().unwrap()["ch.kicad_sch"].power_symbols.iter().find(|p| p.id != "#PWR01").unwrap().id.clone();
    b.apply(&on("sa", Cmd::DeletePowerSymbol { id: new_id })).unwrap();
    assert!(b.design().sheet_contents.as_ref().unwrap()["ch.kicad_sch"].extras.power_path_refs.is_empty());
}

/// The channel screen with a two-unit symbol: U1 unit 1 at x = 10 mm and U1 unit 2 at x = 20 mm; the second placement names them U2.
fn dual_channels() -> Design {
    let mut d = channels();
    let ch = d.sheet_contents.as_mut().unwrap().get_mut("ch.kicad_sch").unwrap();
    ch.symbols = vec![symbol("U1", "Test:Dual", "X", 1, p(mm(10.0), Nm::ZERO)), symbol("U1", "Test:Dual", "X", 2, p(mm(20.0), Nm::ZERO))];
    ch.instance_overrides = vec![
        SymbolPathOverride { at: p(mm(10.0), Nm::ZERO), sheet_path: vec!["sb".into()], reference: "U2".into(), unit: 1 },
        SymbolPathOverride { at: p(mm(20.0), Nm::ZERO), sheet_path: vec!["sb".into()], reference: "U2".into(), unit: 2 },
    ];
    ch.power_symbols.clear();
    d.symbol_library = Some(eda_model::ir::SymbolLibrarySection { symbols: vec![dual()], ..Default::default() });
    d
}

fn units_on(d: &Design, path: &[&str]) -> Vec<(f64, u32)> {
    let path: Vec<String> = path.iter().map(|s| s.to_string()).collect();
    let screen = sheet_list::screen_of(d, &path).unwrap();
    screen.symbols.iter().map(|s| (s.at.x.mm(), sheet_list::unit_under(screen, s, &path))).collect()
}

/// Symbol Unit on the second placement changes that placement's unit only (`SetUnitSelection( &sheetPath, aUnit )`): the first keeps its, the swap gives
/// the other unit of the path's reference the unit this one had, and both placements keep their references.
#[test]
fn a_unit_edit_changes_the_placement_in_view_only() {
    let m = ConstraintModel::default();
    let mut b = Board::new(dual_channels(), &m, mm(0.1), mm(0.3));
    let swap = Cmd::SchEdit(crate::sch_edit::SchCmd::SetSymbolUnit { id: "U1".into(), unit: Some(1), new_unit: 2, swap: true });
    b.apply(&on("sb", swap)).unwrap();
    assert_eq!(units_on(b.design(), &["sa"]), [(10.0, 1), (20.0, 2)], "the first placement draws what it drew");
    assert_eq!(units_on(b.design(), &["sb"]), [(10.0, 2), (20.0, 1)], "the second placement's two units are swapped");
    assert_eq!(refs_on(b.design(), &["sa"]), ["U1", "U1"]);
    assert_eq!(refs_on(b.design(), &["sb"]), ["U2", "U2"], "the per-path references survive the edit");
    // without the swap, the unit the path's reference already draws is refused
    let dup = Cmd::SchEdit(crate::sch_edit::SchCmd::SetSymbolUnit { id: "U1".into(), unit: Some(1), new_unit: 1, swap: false });
    assert!(b.apply(&on("sb", dup)).is_err());
    // and swapped back, nothing per path is left
    let back = Cmd::SchEdit(crate::sch_edit::SchCmd::SetSymbolUnit { id: "U1".into(), unit: Some(1), new_unit: 1, swap: true });
    b.apply(&on("sb", back)).unwrap();
    assert!(b.design().sheet_contents.as_ref().unwrap()["ch.kicad_sch"].extras.path_units.is_empty());
}

/// Do not Populate on the second placement: the units collected are those of the reference that path gives the symbol (U2), on whatever sheet they are
/// (`CollectOtherUnits( GetRef( &GetCurrentSheet() ), .. )` over `Hierarchy()`) -- U2's unit placed on the root too -- not the units of the screen's own
/// reference (U1) elsewhere; and the new state is worked out over all of them (`new_state`), so a unit already DNP elsewhere does not turn it off.
#[test]
fn dnp_on_the_second_placement_collects_the_units_of_its_reference() {
    let m = ConstraintModel::default();
    let mut d = dual_channels();
    // a unit of U2 on the root (U2's unit 2 is the second placement's), and a unit of U1 on the root already DNP
    let root = d.schematic.as_mut().unwrap();
    let mut u2 = symbol("U2", "Test:Dual", "X", 2, p(mm(10.0), mm(50.0)));
    u2.dnp = true;
    root.symbols.push(u2);
    root.symbols.push(symbol("U1", "Test:Dual", "X", 2, p(mm(20.0), mm(50.0))));
    // the second placement's U2 draws unit 1 only, the root's U2 unit 2 (and the screen's unit-2 symbol is the second placement's U2 too, so drop it)
    let ch = d.sheet_contents.as_mut().unwrap().get_mut("ch.kicad_sch").unwrap();
    ch.symbols.truncate(1);
    ch.instance_overrides.truncate(1);
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let toggle = Cmd::SchEdit(crate::sch_edit::SchCmd::ToggleSymbolAttribute { ids: vec!["U1".into()], attribute: "dnp".into() });
    b.apply(&on("sb", toggle.clone())).unwrap();
    let d = b.design();
    assert!(d.sheet_contents.as_ref().unwrap()["ch.kicad_sch"].symbols[0].dnp, "the symbol in view (its flag is the shared screen's)");
    let root = d.schematic.as_ref().unwrap();
    assert!(root.symbols.iter().find(|s| s.id == "U2").unwrap().dnp, "U2's unit on the root stays DNP: one of the items lacked it, so the state is set");
    assert!(!root.symbols.iter().find(|s| s.id == "U1").unwrap().dnp, "U1's unit on the root is another part's, as the second placement names it");
    // toggled again: every collected item has it, so it is cleared on all of them
    b.apply(&on("sb", toggle)).unwrap();
    let d = b.design();
    assert!(!d.sheet_contents.as_ref().unwrap()["ch.kicad_sch"].symbols[0].dnp);
    assert!(!d.schematic.as_ref().unwrap().symbols.iter().find(|s| s.id == "U2").unwrap().dnp);
}

/// The Geographical Reannotate of the board renames a part a second placement names (an override) as well as the screen's own (`reference_renames`).
#[test]
fn a_rename_of_a_second_placements_reference_reaches_that_placement() {
    let m = ConstraintModel::default();
    let mut b = Board::new(channels(), &m, mm(0.1), mm(0.3));
    let map: BTreeMap<String, String> = [("R3".to_string(), "R13".to_string()), ("R1".to_string(), "R11".to_string())].into_iter().collect();
    let renames = b.reference_renames(&map);
    b.annotate_apply(&renames, false);
    assert_eq!(refs_on(b.design(), &["sa"]), ["R11", "R2"]);
    assert_eq!(refs_on(b.design(), &["sb"]), ["R13", "R4"]);
}
