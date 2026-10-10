//! The hierarchy verbs: Find and Replace across the sheets, Import Sheet, the paste of whole sheets.
use super::sch_annotate_tests::{at, design, on};
use super::*;

#[test]
fn replace_all_replaces_in_every_sheet_as_one_command() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, 100, 300);
    let search = search::SearchData { find: "10k".into(), replace: "22k".into(), ..Default::default() };
    let values = |b: &Board<'_>, v: &str| -> usize { b.design().sheet_contents.as_ref().unwrap().values().flat_map(|s| s.symbols.iter()).filter(|s| s.value == v).count() };
    assert_eq!(values(&b, "10k"), 4, "R5, R9 and R? on A, R1 on B");

    // "Search the current sheet only": B's R1 and nothing else
    b.apply(&on("sb", Cmd::ReplaceText { search: search.clone(), items: None, all_sheets: false })).unwrap();
    assert_eq!((values(&b, "10k"), values(&b, "22k")), (3, 1));
    assert_eq!(at(b.design(), "b.kicad_sch", 20).value, "22k");

    // Replace All over the hierarchy reaches the sheets that were not in view
    b.apply(&Cmd::ReplaceText { search: search.clone(), items: None, all_sheets: true }).unwrap();
    assert_eq!((values(&b, "10k"), values(&b, "22k")), (0, 4));
    assert_eq!(at(b.design(), "a.kicad_sch", 30).value, "22k");

    // nothing left to replace is a refusal, on every sheet or on one
    assert!(b.apply(&Cmd::ReplaceText { search: search.clone(), items: None, all_sheets: true }).is_err());
    assert!(b.apply(&on("sa", Cmd::ReplaceText { search, items: None, all_sheets: false })).is_err());
}

#[test]
fn replace_all_over_sheets_renames_references_where_asked_and_keeps_the_focus() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, 100, 300);
    // R1 is on B, R5 and R9 on A: whole-word "R9" -> "R19" renames A's symbol, with references on
    let search = search::SearchData { find: "R9".into(), replace: "R19".into(), mode: search::MatchMode::WholeWord, replace_references: true, ..Default::default() };
    b.apply(&Cmd::ReplaceText { search, items: None, all_sheets: true }).unwrap();
    assert_eq!(at(b.design(), "a.kicad_sch", 10).id, "R19");
    assert!(b.design().schematic.as_ref().unwrap().symbols.is_empty(), "the root sheet has no symbols and was not touched");
}
