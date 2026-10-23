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

// ------------------------------------------------------------------------------------------------ whole sheets: paste and Import Sheet

use super::sch_annotate_tests::{placement, section, symbol};
use crate::sch_edit::SchCmd;
use eda_model::ir::{LibrarySymbol, Point, SchematicSection, SheetPin, Um};
use eda_model::sch_clipboard::{PasteMode, SchFragment};
use eda_model::sheet_file::SheetFile;

fn pt(x: Um, y: Um) -> Point {
    Point { x, y }
}

fn lib(ids: &[&str]) -> Vec<LibrarySymbol> {
    ids.iter().map(|id| LibrarySymbol::from_engine_symbol(&eda_model::symbol::builtin(id).expect("a built-in symbol"))).collect()
}

/// root -> Power (power.kicad_sch: R1, R2, C1) -> Filter (filter.kicad_sch: R3).
fn power_design() -> Design {
    let mut d = super::sch_annotate_tests::design();
    let r = |id: &str, x: Um| symbol(id, "Device:R", "10k", 1, pt(x, 0));
    let contents: BTreeMap<String, SchematicSection> = [
        ("power.kicad_sch".to_string(), section(vec![r("R1", 10), r("R2", 20), symbol("C1", "Device:C", "100n", 1, pt(30, 0))], vec![placement("sf", "Filter", "filter.kicad_sch", "")])),
        ("filter.kicad_sch".to_string(), section(vec![r("R3", 10)], vec![])),
    ]
    .into_iter()
    .collect();
    d.sheet_contents = Some(contents);
    let mut power = placement("sp", "Power", "power.kicad_sch", "");
    power.at = pt(100_000, 50_000);
    power.pins = vec![SheetPin { id: "p1".into(), name: "VIN".into(), shape: Default::default(), at: pt(100_000, 60_000) }];
    d.schematic.as_mut().unwrap().sheets = vec![power];
    d
}

/// The clipboard fragment of the Power sheet: the sheet symbol and the screens it shows.
fn power_fragment(d: &Design) -> SchFragment {
    let root = d.schematic.as_ref().unwrap();
    let screens = d.sheet_contents.clone().unwrap();
    let section = SchematicSection { sheets: root.sheets.clone(), imported_from_kicad: true, ..Default::default() };
    SchFragment { section, lib_symbols: lib(&["Device:R", "Device:C"]), notes: vec![], sheet_screens: screens }
}

fn sheet_names(d: &Design) -> Vec<String> {
    d.schematic.as_ref().unwrap().sheets.iter().map(|s| s.name.clone()).collect()
}

fn refs_in(d: &Design, file: &str) -> Vec<String> {
    let mut r: Vec<String> = d.sheet_contents.as_ref().unwrap()[file].symbols.iter().map(|s| s.id.clone()).collect();
    r.sort();
    r
}

#[test]
fn a_pasted_sheet_is_a_copy_with_a_new_name_a_file_of_its_own_and_new_references() {
    let m = ConstraintModel::default();
    let d = power_design();
    let fragment = power_fragment(&d);
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::PasteSch { fragment: fragment.clone(), dx: 60_000, dy: 0, mode: PasteMode::Unique }).unwrap();
    let d = b.design();

    // the name counts up, the file is new, the sheet symbol moved with its pin
    assert_eq!(sheet_names(d), vec!["Power", "Power1"]);
    let copy = d.schematic.as_ref().unwrap().sheets.iter().find(|s| s.name == "Power1").unwrap();
    assert_eq!(copy.file, "power1.kicad_sch");
    assert_eq!(copy.at, pt(160_000, 50_000));
    assert_eq!(copy.pins.len(), 1);
    assert_eq!((copy.pins[0].name.as_str(), copy.pins[0].at), ("VIN", pt(160_000, 60_000)));
    assert!(!copy.id.is_empty() && copy.id != "sp");

    // a copy of the screen it shows, and of the one inside that, each under a file of its own
    let files: Vec<&String> = d.sheet_contents.as_ref().unwrap().keys().collect();
    assert!(files.contains(&&"power1.kicad_sch".to_string()) && files.contains(&&"filter_2.kicad_sch".to_string()), "{files:?}");
    let nested = &d.sheet_contents.as_ref().unwrap()["power1.kicad_sch"].sheets;
    assert_eq!((nested[0].name.as_str(), nested[0].file.as_str()), ("Filter", "filter_2.kicad_sch"), "the sheet inside shows the copy of its screen");
    assert_ne!(nested[0].id, "sf", "a sheet is its own id everywhere in the design");

    // the originals are untouched; the copies have references nothing else holds
    assert_eq!(refs_in(d, "power.kicad_sch"), vec!["C1", "R1", "R2"]);
    assert_eq!(refs_in(d, "filter.kicad_sch"), vec!["R3"]);
    assert_eq!(refs_in(d, "power1.kicad_sch"), vec!["C2", "R4", "R5"]);
    assert_eq!(refs_in(d, "filter_2.kicad_sch"), vec!["R6"]);

    // one more paste: Power2 (a counting name: Power1 is taken, so the next number)
    let before = d.clone();
    b.apply(&Cmd::PasteSch { fragment, dx: 0, dy: 90_000, mode: PasteMode::Unique }).unwrap();
    assert_eq!(sheet_names(b.design()), vec!["Power", "Power1", "Power2"]);
    assert_eq!(before.sheet_contents.as_ref().unwrap().len() + 2, b.design().sheet_contents.as_ref().unwrap().len());
}

/// A pasted sheet's pins keep their text's size, bold and italic (`SCH_SHEET_PIN` is an `EDA_TEXT`, copied whole with its sheet).
#[test]
fn a_pasted_sheet_s_pins_keep_their_text_look() {
    let m = ConstraintModel::default();
    let d = power_design();
    let mut fragment = power_fragment(&d);
    let pin_id = fragment.section.sheets[0].pins[0].id.clone();
    assert!(!pin_id.is_empty());
    let look = eda_model::sch_extras::LabelLook { size_um: 2_540, bold: true, ..Default::default() };
    fragment.section.extras.sheet_pin_looks.insert(pin_id, look);
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::PasteSch { fragment, dx: 60_000, dy: 0, mode: PasteMode::Unique }).unwrap();
    let sch = b.design().schematic.as_ref().unwrap();
    let copy = sch.sheets.iter().find(|s| s.name == "Power1").unwrap();
    assert_eq!(sch.extras.sheet_pin_looks.get(&copy.pins[0].id), Some(&look), "{:?}", sch.extras.sheet_pin_looks);
}

#[test]
fn a_cut_and_paste_of_a_sheet_keeps_its_name_its_file_and_its_numbers() {
    let m = ConstraintModel::default();
    let d = power_design();
    let fragment = power_fragment(&d);
    let mut b = Board::new(d, &m, 100, 300);
    // Cut: the copy was taken, then the sheet deleted. Its screens stay (the files are still there) but no sheet shows them: they hold no references
    b.apply(&Cmd::SchEdit(SchCmd::DeleteSheet { id: "sp".into() })).unwrap();
    assert!(b.design().schematic.as_ref().unwrap().sheets.is_empty());
    assert!(b.design().sheet_contents.as_ref().unwrap().contains_key("power.kicad_sch"));
    assert!(eda_model::sheet_list::reachable_files(b.design()).is_empty());
    assert!(b.used_references().iter().all(|u| u.reference.starts_with('#')), "the references of a screen no sheet shows are free");
    b.apply(&Cmd::PasteSch { fragment, dx: 0, dy: 0, mode: PasteMode::Unique }).unwrap();
    let d = b.design();
    assert_eq!(sheet_names(d), vec!["Power"]);
    assert_eq!(d.schematic.as_ref().unwrap().sheets[0].file, "power.kicad_sch");
    assert_eq!(refs_in(d, "power.kicad_sch"), vec!["C1", "R1", "R2"], "nothing else held the numbers");
    assert_eq!(refs_in(d, "filter.kicad_sch"), vec!["R3"]);
    assert_eq!(d.sheet_contents.as_ref().unwrap().len(), 2, "the sheet came back to the files it had: nothing is left over");
}

#[test]
fn a_sheet_whose_screen_the_clipboard_lacks_is_pasted_from_the_design_else_empty() {
    let m = ConstraintModel::default();
    let d = power_design();
    let mut fragment = power_fragment(&d);
    fragment.sheet_screens.clear();
    // the design has the screen under that name: it is what the sheet shows
    let mut b = Board::new(d.clone(), &m, 100, 300);
    b.apply(&Cmd::PasteSch { fragment: fragment.clone(), dx: 0, dy: 90_000, mode: PasteMode::Unique }).unwrap();
    assert_eq!(refs_in(b.design(), "power1.kicad_sch"), vec!["C2", "R4", "R5"]);
    // a design that has none: an empty sheet (`InitSheet`)
    let mut other = power_design();
    other.sheet_contents = None;
    other.schematic.as_mut().unwrap().sheets.clear();
    let mut b = Board::new(other, &m, 100, 300);
    b.apply(&Cmd::PasteSch { fragment, dx: 0, dy: 0, mode: PasteMode::Unique }).unwrap();
    assert_eq!(sheet_names(b.design()), vec!["Power"]);
    assert!(b.design().sheet_contents.as_ref().unwrap()["power.kicad_sch"].symbols.is_empty());
}

/// A file: R1, R2 and C1 on its screen, a sheet Sub inside it with R7.
fn a_file() -> SheetFile {
    let r = |id: &str, x: Um| symbol(id, "Device:R", "4k7", 1, pt(x, 0));
    let mut root = section(vec![r("R1", 10), r("R2", 20), symbol("C1", "Device:C", "1u", 1, pt(30, 0))], vec![placement("sub", "Sub", "sub.kicad_sch", "")]);
    root.imported_from_kicad = true;
    let mut sub = section(vec![r("R7", 10)], vec![]);
    sub.imported_from_kicad = true;
    SheetFile { root, screens: [("sub.kicad_sch".to_string(), sub)].into_iter().collect(), lib_symbols: lib(&["Device:R", "Device:C"]), notes: vec![] }
}

fn import(content: Option<SheetFile>, as_sheet: bool, keep: bool) -> Cmd {
    Cmd::ImportSheet {
        source: "/tmp/amp.kicad_sch".into(),
        content,
        as_sheet,
        name: "Amp".into(),
        file: "amp".into(),
        at: pt(20_000, 20_000),
        size: (40_000, 20_000),
        keep_annotations: keep,
        order: AnnotateOrder::XThenY,
        options: AnnotateOptions::default(),
        as_group: false,
        group_name: String::new(),
        block: String::new(),
    }
}

#[test]
fn import_sheet_puts_a_copy_of_the_file_in_as_a_sheet_and_numbers_its_symbols_anew() {
    let m = ConstraintModel::default();
    let mut b = Board::new(power_design(), &m, 100, 300);
    b.apply(&import(Some(a_file()), true, false)).unwrap();
    let d = b.design();
    let amp = d.schematic.as_ref().unwrap().sheets.iter().find(|s| s.name == "Amp").unwrap();
    assert_eq!((amp.file.as_str(), amp.at, amp.size, amp.pins.len()), ("amp.kicad_sch", pt(20_000, 20_000), (40_000, 20_000), 0));
    // the design holds R1..R3 and C1 already: the file's symbols are numbered round them, in X order
    assert_eq!(refs_in(d, "amp.kicad_sch"), vec!["C2", "R4", "R5"]);
    // the sheet inside the file came along, with a file of its own, and its symbol is numbered too
    let inner = &d.sheet_contents.as_ref().unwrap()["amp.kicad_sch"].sheets[0];
    assert_eq!((inner.name.as_str(), inner.file.as_str()), ("Sub", "sub.kicad_sch"));
    assert_eq!(refs_in(d, "sub.kicad_sch"), vec!["R6"]);
    // the library symbols came with it: the design draws the file's symbols
    assert!(d.sheet_contents.as_ref().unwrap()["amp.kicad_sch"].symbols.iter().all(|s| !s.lib_id.is_empty()));
    // the file's screen is in this design's frame now: a symbol is placed by the corner of its box, not by its library origin
    let amp = &d.sheet_contents.as_ref().unwrap()["amp.kicad_sch"];
    assert!(!amp.imported_from_kicad && !d.sheet_contents.as_ref().unwrap()["sub.kicad_sch"].imported_from_kicad);
    let moved = amp.symbols.iter().filter(|s| s.lib_id.starts_with("Device:")).any(|s| s.at != pt(s.at.x, 0) || s.at.x % 10 != 0);
    assert!(moved, "the symbols were at their library origins (y = 0) and are at the corner of their boxes now: {:?}", amp.symbols.iter().map(|s| s.at).collect::<Vec<_>>());
    // the same name again is refused, and so is a file that was never read
    assert_eq!(b.apply(&import(Some(a_file()), true, false)).unwrap_err()[0].check, "ops_sheet_name_taken");
    assert_eq!(b.apply(&import(None, true, false)).unwrap_err()[0].check, "ops_sheet_file_not_read");
}

#[test]
fn import_sheet_with_keep_annotations_numbers_only_what_the_design_holds() {
    let m = ConstraintModel::default();
    let mut file = a_file();
    // R9 is free in the design, R1 is not
    file.root.symbols.push(symbol("R9", "Device:R", "4k7", 1, pt(40, 0)));
    let mut b = Board::new(power_design(), &m, 100, 300);
    b.apply(&import(Some(file), true, true)).unwrap();
    let refs = refs_in(b.design(), "amp.kicad_sch");
    assert!(refs.contains(&"R9".to_string()), "a reference nothing holds is kept: {refs:?}");
    assert!(refs.contains(&"C2".to_string()) && !refs.contains(&"R1".to_string()) && !refs.contains(&"R2".to_string()), "the taken ones are numbered anew: {refs:?}");
    assert!(b.design().sheet_contents.as_ref().unwrap().values().flat_map(|s| s.symbols.iter()).filter(|s| s.id == "R9").count() == 1);
}

#[test]
fn import_sheet_contents_goes_on_the_sheet_in_view() {
    let m = ConstraintModel::default();
    let mut b = Board::new(power_design(), &m, 100, 300);
    b.apply(&on("sp", import(Some(a_file()), false, false))).unwrap();
    let d = b.design();
    // on the Power sheet: the file's symbols, numbered round the design's, and its sheet Sub as a sheet of Power
    assert_eq!(refs_in(d, "power.kicad_sch"), vec!["C1", "C2", "R1", "R2", "R4", "R5"]);
    let names: Vec<&str> = d.sheet_contents.as_ref().unwrap()["power.kicad_sch"].sheets.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["Filter", "Sub"]);
}

#[test]
fn import_sheet_contents_are_numbered_afresh_by_the_annotate_options_unless_annotations_are_kept() {
    let m = ConstraintModel::default();
    let mut file = a_file();
    // R9 is free in the design: a paste would keep it, `placeSheetContents` numbers it again (reset = true)
    file.root.symbols.push(symbol("R9", "Device:R", "4k7", 1, pt(40, 0)));
    let mut b = Board::new(power_design(), &m, 100, 300);
    b.apply(&on("sp", import(Some(file.clone()), false, false))).unwrap();
    let mut refs = refs_in(b.design(), "power.kicad_sch");
    refs.retain(|r| !["C1", "R1", "R2"].contains(&r.as_str()));
    assert_eq!(refs, vec!["C2", "R4", "R5", "R6"], "the first free numbers, in X order: R1 -> R4, R2 -> R5, R9 -> R6");

    // "First free after sheet number X 100": the Power sheet is page 2
    let mut x100 = import(Some(file.clone()), false, false);
    if let Cmd::ImportSheet { options, .. } = &mut x100 {
        options.algo = eda_model::annotate::Algo::SheetX100;
    }
    let mut b = Board::new(power_design(), &m, 100, 300);
    b.apply(&on("sp", x100)).unwrap();
    let mut refs = refs_in(b.design(), "power.kicad_sch");
    refs.retain(|r| !["C1", "R1", "R2"].contains(&r.as_str()));
    assert_eq!(refs, vec!["C201", "R201", "R202", "R203"]);

    // Keep annotations: nothing is numbered again but what the design holds
    let mut b = Board::new(power_design(), &m, 100, 300);
    b.apply(&on("sp", import(Some(file), false, true))).unwrap();
    let refs = refs_in(b.design(), "power.kicad_sch");
    assert!(refs.contains(&"R9".to_string()), "{refs:?}");
}

#[test]
fn the_checks_of_a_sheet_file_are_loadsheetfromfile_s() {
    let m = ConstraintModel::default();
    let b = Board::new(power_design(), &m, 100, 300);
    // duplicate references are said, not refused
    let found = b.check_sheet_file(&a_file(), "amp", false);
    assert!(found.iter().any(|f| f.severity == "info" && f.text.starts_with("3 reference(s) of the sheet are used in this schematic already (R1, R2, C1")), "{found:?}");
    assert!(!found.iter().any(|f| f.severity == "error"));
    // a file that places a parent of itself
    let mut cyclic = a_file();
    cyclic.screens.get_mut("sub.kicad_sch").unwrap().sheets.push(placement("back", "Back", "sub.kicad_sch", ""));
    let errors: Vec<_> = b.check_sheet_file(&cyclic, "amp", false).into_iter().filter(|f| f.severity == "error").collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].text.contains("itself as a parent"));
    let mut bb = Board::new(power_design(), &m, 100, 300);
    assert_eq!(bb.apply(&import(Some(cyclic), true, false)).unwrap_err()[0].check, "ops_sheet_file");
    // symbols that name no library (a legacy link), and the reader's own remarks
    let mut legacy = a_file();
    legacy.root.symbols.iter_mut().for_each(|s| s.lib_id = s.lib_id.rsplit(':').next().unwrap_or("R").to_string());
    legacy.root.power_symbols.iter_mut().for_each(|s| s.lib_id = s.lib_id.rsplit(':').next().unwrap_or("GND").to_string());
    legacy.screens.values_mut().flat_map(|sc| sc.symbols.iter_mut()).for_each(|s| s.lib_id = s.lib_id.rsplit(':').next().unwrap_or("R").to_string());
    legacy.notes.push("2 symbol(s) had no library symbol in the file and were left out".into());
    let found = b.check_sheet_file(&legacy, "amp", false);
    assert!(found.iter().any(|f| f.severity == "error" && f.text == "The schematic 'amp' has not had its symbol library links remapped to the symbol library table.  The project this schematic belongs to must first be remapped before it can be imported into the current project."), "{found:?}");
    // one symbol with a library link is enough (`HasNoFullyDefinedLibIds` returns false at the first)
    let mut mixed = a_file();
    mixed.root.symbols[0].lib_id = "R".into();
    assert!(b.check_sheet_file(&mixed, "amp", false).iter().all(|f| !f.text.contains("symbol library links")));
    assert!(found.iter().any(|f| f.severity == "warning" && f.text.starts_with("2 symbol(s) had no library")));
    assert!(b.check_sheet_file(&a_file(), "amp", false).iter().all(|f| f.severity != "warning"));
}

#[test]
fn sheet_names_count_up_until_free() {
    let taken: BTreeSet<String> = ["Power", "Power1", "Power2", "Amp 7"].iter().map(|s| s.to_string()).collect();
    assert_eq!(unique_sheet_name(&taken, "Power"), "Power3");
    assert_eq!(unique_sheet_name(&taken, "Power1"), "Power3");
    assert_eq!(unique_sheet_name(&taken, "Amp 7"), "Amp 8");
    assert_eq!(unique_sheet_name(&taken, "Free"), "Free");
    assert_eq!(unique_sheet_name(&BTreeSet::from(["7".to_string()]), "7"), "8");
}

// ------------------------------------------------------------------------------------------------ design blocks (Import Sheet with a block)

/// `a_file()` placed as the schematic of the design block `Power:Buck`: the file is the block's `Buck.kicad_sch`, grouped as the pane's "Place as group" says.
fn block_import(content: Option<SheetFile>, as_sheet: bool, group_name: &str, as_group: bool) -> Cmd {
    let mut c = import(content, as_sheet, false);
    if let Cmd::ImportSheet { as_group: g, group_name: n, block, source, .. } = &mut c {
        *g = as_group;
        *n = group_name.to_string();
        *block = "Power:Buck".to_string();
        *source = "/lib/Power.kicad_blocks/Buck.kicad_block/Buck.kicad_sch".to_string();
    }
    c
}

fn groups_of(d: &Design, file: &str) -> Vec<eda_model::ir::Group> {
    d.sheet_contents.as_ref().unwrap()[file].extras.groups.clone()
}

#[test]
fn a_design_block_placed_as_contents_is_one_linked_group_whose_members_follow_the_numbering() {
    let m = ConstraintModel::default();
    let mut b = Board::new(power_design(), &m, 100, 300);
    b.apply(&on("sp", block_import(Some(a_file()), false, "", true))).unwrap();
    let d = b.design();
    let groups = groups_of(d, "power.kicad_sch");
    assert_eq!(groups.len(), 1, "{groups:?}");
    // named after the block (`designBlock->GetLibId().GetLibItemName()`) and linked to it
    assert_eq!((groups[0].name.as_str(), groups[0].design_block.as_str()), ("Buck", "Power:Buck"));
    // the members are the new symbols under the references the numbering gave them, and the sheet that came along; the design's own R1, R2 and C1 are not in it
    let sec = &d.sheet_contents.as_ref().unwrap()["power.kicad_sch"];
    let mut members = groups[0].member_ids.clone();
    members.sort();
    let mut want = vec!["C2".to_string(), "R4".to_string(), "R5".to_string()];
    want.extend(sec.sheets.iter().filter(|s| s.name == "Sub").map(|s| s.id.clone()));
    want.sort();
    assert_eq!(members, want);
    assert!(!members.contains(&"R1".to_string()));
    // "Place repeated copies" numbers the group's name (the caller passes it)
    b.apply(&on("sp", block_import(Some(a_file()), false, "Buck2", true))).unwrap();
    let names: Vec<String> = groups_of(b.design(), "power.kicad_sch").iter().map(|g| g.name.clone()).collect();
    assert_eq!(names, vec!["Buck", "Buck2"]);
    assert!(groups_of(b.design(), "power.kicad_sch").iter().all(|g| g.design_block == "Power:Buck"));
}

#[test]
fn a_file_placed_as_contents_is_grouped_only_when_asked_and_with_no_link() {
    let m = ConstraintModel::default();
    let mut plain = import(Some(a_file()), false, false);
    if let Cmd::ImportSheet { as_group, source, .. } = &mut plain {
        *as_group = true;
        *source = "/tmp/amp.kicad_sch".to_string();
    }
    let mut b = Board::new(power_design(), &m, 100, 300);
    b.apply(&on("sp", plain)).unwrap();
    let groups = groups_of(b.design(), "power.kicad_sch");
    assert_eq!(groups.len(), 1);
    assert_eq!((groups[0].name.as_str(), groups[0].design_block.as_str()), ("amp", ""), "named after the file, linked to nothing");
    // not asked: no group, however the block is linked
    let mut b = Board::new(power_design(), &m, 100, 300);
    b.apply(&on("sp", block_import(Some(a_file()), false, "", false))).unwrap();
    assert!(groups_of(b.design(), "power.kicad_sch").is_empty());
}

#[test]
fn a_design_block_placed_as_a_sheet_goes_in_a_linked_group_of_its_own_and_a_plain_file_does_not() {
    let m = ConstraintModel::default();
    let mut b = Board::new(power_design(), &m, 100, 300);
    b.apply(&block_import(Some(a_file()), true, "", true)).unwrap();
    let groups = b.design().schematic.as_ref().unwrap().extras.groups.clone();
    assert_eq!(groups.len(), 1, "{groups:?}");
    let amp = b.design().schematic.as_ref().unwrap().sheets.iter().find(|s| s.name == "Amp").unwrap();
    assert_eq!((groups[0].name.as_str(), groups[0].design_block.as_str(), groups[0].member_ids.clone()), ("Buck", "Power:Buck", vec![amp.id.clone()]), "the sheet alone, in a group of one");
    // a group of one that is linked survives the other edits of the groups
    b.apply(&Cmd::SchEdit(SchCmd::Group { ids: vec!["sp".into(), "nothing".into()] })).unwrap_err();
    assert_eq!(b.design().schematic.as_ref().unwrap().extras.groups.len(), 1);
    // the same file as a plain sheet is not grouped (`DrawSheet` groups a design block's sheet only)
    let mut plain = import(Some(a_file()), true, false);
    if let Cmd::ImportSheet { as_group, .. } = &mut plain {
        *as_group = true;
    }
    let mut b = Board::new(power_design(), &m, 100, 300);
    b.apply(&plain).unwrap();
    assert!(b.design().schematic.as_ref().unwrap().extras.groups.is_empty());
    // a block id that is not a Lib:Name is refused, whole
    let mut bad = block_import(Some(a_file()), true, "", true);
    if let Cmd::ImportSheet { block, .. } = &mut bad {
        *block = "nonsense".to_string();
    }
    let mut b = Board::new(power_design(), &m, 100, 300);
    assert_eq!(b.apply(&bad).unwrap_err()[0].check, "ops_bad_design_block");
    assert!(b.design().schematic.as_ref().unwrap().sheets.iter().all(|s| s.name != "Amp"), "nothing was placed");
}

#[test]
fn linking_the_selection_to_a_block_groups_it_or_links_the_group_and_group_properties_edits_the_link() {
    let m = ConstraintModel::default();
    let mut b = Board::new(power_design(), &m, 100, 300);
    let link = |ids: &[&str], block: &str| on("sp", Cmd::SchEdit(SchCmd::LinkDesignBlock { ids: ids.iter().map(|s| s.to_string()).collect(), block: block.into() }));
    // a selection that is not a group: a group named after the block, linked, even for one item
    b.apply(&link(&["R1"], "Mine:Divider")).unwrap();
    let g = groups_of(b.design(), "power.kicad_sch");
    assert_eq!((g[0].name.as_str(), g[0].design_block.as_str(), g[0].member_ids.clone()), ("Divider", "Mine:Divider", vec!["R1".to_string()]));
    // a lone group with no link takes it and keeps its name
    b.apply(&on("sp", Cmd::SchEdit(SchCmd::Group { ids: vec!["R2".into(), "C1".into()] }))).unwrap();
    let plain = groups_of(b.design(), "power.kicad_sch").into_iter().find(|g| g.design_block.is_empty()).unwrap();
    b.apply(&link(&[&plain.id], "Mine:Pair")).unwrap();
    let g = groups_of(b.design(), "power.kicad_sch").into_iter().find(|g| g.id == plain.id).unwrap();
    assert_eq!((g.name.as_str(), g.design_block.as_str()), ("", "Mine:Pair"));
    // Group Properties' "Library link:" box: change, keep, clear, refuse
    let edit = |design_block: Option<&str>| on("sp", Cmd::SchEdit(SchCmd::EditGroup { id: plain.id.clone(), name: "pair".into(), member_ids: g.member_ids.clone(), design_block: design_block.map(str::to_string) }));
    b.apply(&edit(Some("Mine:Other"))).unwrap();
    let link_now = |b: &Board<'_>| groups_of(b.design(), "power.kicad_sch").into_iter().find(|x| x.id == plain.id).unwrap().design_block;
    assert_eq!(link_now(&b), "Mine:Other");
    b.apply(&edit(None)).unwrap();
    assert_eq!(link_now(&b), "Mine:Other");
    let refused = b.apply(&edit(Some("broken"))).unwrap_err();
    assert_eq!((refused[0].check.as_str(), refused[0].location.clone().unwrap_or_default()), ("ops_bad_group", plain.id.clone()));
    b.apply(&edit(Some(""))).unwrap();
    assert_eq!(link_now(&b), "");
    // an id that is no item of the sheet, and a link that is no id
    assert_eq!(b.apply(&link(&["R9"], "Mine:X")).unwrap_err()[0].check, "ops_unknown_item");
    assert!(b.apply(&link(&["R1"], "no colon")).is_err());
}
