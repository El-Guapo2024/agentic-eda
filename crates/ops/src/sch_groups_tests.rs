//! Groups on the schematic (`sch_groups.rs`): the five verbs, what a group's tree does under Move, Drag, Rotate, Mirror and Delete, how it stays honest when its
//! members leave or are renamed, and the items made while a group is entered.
//!
//! The sheet is a KiCad-style one (`imported_from_kicad`: a symbol sits at its library origin) with `Device:R` resistors, whose pins are three grid cells above
//! and below the origin: R1 at (20.32, 25.4) mm has pin 1 at (20.32, 21.59) and pin 2 at (20.32, 29.21).

use super::*;
use crate::sch_edit::SchCmd as V;
use crate::sch_move::SchMoveCmd;
use eda_model::ir::{Group, Junction, LabelKind, NetLabel, NoConnect, PowerSymbol, Provenance, SchLine, SchematicSection, SchematicText, SymbolInstance, Wire};
use std::collections::BTreeMap;

const G: Um = 1_270;

fn p(x: Um, y: Um) -> Point {
    Point { x, y }
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn sym(id: &str, at: Point) -> SymbolInstance {
    SymbolInstance { id: id.into(), at, rot: 0, mirrored: false, mirror_y: false, lib_id: "Device:R".into(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false }
}

fn wire(id: &str, pts: &[Point]) -> Wire {
    Wire { id: id.into(), net: "N".into(), pins: Vec::new(), pts: pts.to_vec(), bus: false }
}

/// R1 and R2 side by side, a wire and a label, a text, a no-connect, a note line and a junction: eight items that do not touch (apart from the wire between the
/// resistors' tops), and R3 and a far-away text outside any group.
fn sheet() -> SchematicSection {
    let mut s = SchematicSection { imported_from_kicad: true, ..Default::default() };
    s.symbols.push(sym("R1", p(20_320, 25_400)));
    s.symbols.push(sym("R2", p(40_640, 25_400)));
    s.symbols.push(sym("R3", p(101_600, 25_400)));
    // R1's pin 1 to R2's pin 1 (a wire the group holds), and one from R2's pin 2 away to the right (outside every group)
    s.wires.push(wire("wire_in", &[p(20_320, 21_590), p(20_320, 17_780), p(40_640, 17_780), p(40_640, 21_590)]));
    s.wires.push(wire("wire_out", &[p(40_640, 29_210), p(40_640, 33_020), p(63_500, 33_020)]));
    s.labels.push(NetLabel { id: "lbl_a".into(), net: "A".into(), at: p(63_500, 33_020), kind: LabelKind::Local });
    s.texts.push(SchematicText { id: "txt_a".into(), content: "note".into(), at: p(30_480, 50_800), angle: 0, size_um: 1_270 });
    s.texts.push(SchematicText { id: "txt_far".into(), content: "far".into(), at: p(152_400, 50_800), angle: 0, size_um: 1_270 });
    s.no_connects.push(NoConnect { id: "nc_a".into(), at: p(20_320, 29_210), pin: String::new() });
    s.lines.push(SchLine { id: "sln_a".into(), pts: vec![p(10_160, 63_500), p(50_800, 63_500)], width_um: 0 });
    s.junctions.push(Junction { id: "jct_a".into(), at: p(30_480, 17_780) });
    s.power_symbols.push(PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: p(76_200, 38_100), rot: 0, net: "GND".into(), pin: String::new() });
    s.assign_missing_ids();
    s
}

fn design_with(sch: SchematicSection) -> Design {
    Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: 1,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: Some(sch),
        nets: None,
        routing: None,
        placement: None,
        drawings: None,
    }
}

fn board(m: &ConstraintModel, sch: SchematicSection) -> Board<'_> {
    Board::new(design_with(sch), m, 100, 300)
}

fn sch(b: &Board<'_>) -> SchematicSection {
    b.design().schematic.clone().unwrap()
}

fn run(b: &mut Board<'_>, cmd: V) {
    b.apply(&Cmd::SchEdit(cmd)).unwrap_or_else(|e| panic!("{e:?}"));
}

fn refuse(b: &mut Board<'_>, cmd: V) -> String {
    b.apply(&Cmd::SchEdit(cmd)).unwrap_err().remove(0).check
}

fn members(s: &SchematicSection, id: &str) -> Vec<String> {
    let mut m = s.extras.group(id).map(|g| g.member_ids.clone()).unwrap_or_default();
    m.sort();
    m
}

/// A sheet with `outer = {inner, R3}` and `inner = {R1, R2, wire_in, txt_a}`: what a group of groups does.
fn nested() -> SchematicSection {
    let mut s = sheet();
    s.extras.groups = vec![
        Group { id: "grp_inner".into(), name: "inner".into(), member_ids: ids(&["R1", "R2", "wire_in", "txt_a"]) },
        Group { id: "grp_outer".into(), name: "outer".into(), member_ids: ids(&["grp_inner", "R3"]) },
    ];
    s
}

fn symbol_at(s: &SchematicSection, id: &str) -> Point {
    s.symbols.iter().find(|x| x.id == id).unwrap().at
}

// ------------------------------------------------------------------------------------------------------------------------ the verbs

#[test]
fn group_makes_a_group_of_the_items_and_a_group_of_groups_nests() {
    let m = ConstraintModel::default();
    let mut b = board(&m, sheet());
    run(&mut b, V::Group { ids: ids(&["R1", "R2", "wire_in"]) });
    let s = sch(&b);
    assert_eq!(s.extras.groups.len(), 1);
    let first = s.extras.groups[0].id.clone();
    assert!(first.starts_with("grp_"), "{first}");
    assert_eq!(members(&s, &first), ids(&["R1", "R2", "wire_in"]));

    // a group and an item: the group becomes a member of the new one (`SCH_GROUP_TOOL::Group` adds every selected item, a group included)
    run(&mut b, V::Group { ids: vec![first.clone(), "txt_a".into()] });
    let s = sch(&b);
    assert_eq!(s.extras.groups.len(), 2);
    let outer = s.extras.groups.iter().find(|g| g.id != first).unwrap().id.clone();
    assert_eq!(s.extras.parent_group(&first).map(|g| g.id.clone()), Some(outer.clone()));
    assert_eq!(s.extras.group_leaves(&outer), ids(&["R1", "R2", "wire_in", "txt_a"]), "the tree opens to its items");
    assert_eq!(s.extras.top_group("R2").map(|g| g.id.clone()), Some(outer));
}

#[test]
fn group_refuses_what_cannot_be_grouped_and_changes_nothing() {
    let m = ConstraintModel::default();
    let mut b = board(&m, sheet());
    let before = sch(&b).extras.clone();
    assert_eq!(refuse(&mut b, V::Group { ids: ids(&["R1"]) }), "ops_bad_group", "a group needs at least two items");
    assert_eq!(refuse(&mut b, V::Group { ids: ids(&["R1", "R1"]) }), "ops_bad_group", "the same item twice is one item");
    assert_eq!(refuse(&mut b, V::Group { ids: ids(&["R1", "no_such_item"]) }), "ops_unknown_item");
    // a pin and a field go along with their symbol; a sheet pin goes along with its sheet
    assert_eq!(refuse(&mut b, V::Group { ids: ids(&["R1", "pin:R2:1"]) }), "ops_unknown_item");
    assert_eq!(refuse(&mut b, V::Group { ids: ids(&["R1", "fld:R2:Value"]) }), "ops_unknown_item");
    assert_eq!(sch(&b).extras, before);
}

#[test]
fn an_item_is_in_one_group_at_a_time() {
    let m = ConstraintModel::default();
    let mut b = board(&m, sheet());
    run(&mut b, V::Group { ids: ids(&["R1", "R2", "wire_in"]) });
    let a = sch(&b).extras.groups[0].id.clone();
    run(&mut b, V::Group { ids: ids(&["R1", "txt_a"]) });
    let s = sch(&b);
    // R1 left the first group for the second; the first keeps R2 and the wire
    assert_eq!(members(&s, &a), ids(&["R2", "wire_in"]));
    assert_eq!(s.extras.groups.len(), 2);
    assert_eq!(s.extras.parent_group("R1").map(|g| g.id.clone()).filter(|g| *g != a).is_some(), true);
}

#[test]
fn ungroup_frees_the_members_and_leaves_an_inner_group_standing() {
    let m = ConstraintModel::default();
    let mut b = board(&m, nested());
    run(&mut b, V::Ungroup { ids: ids(&["grp_outer"]) });
    let s = sch(&b);
    assert!(!s.extras.is_group("grp_outer"));
    assert!(s.extras.is_group("grp_inner"), "ungrouping a group frees its members -- also a group among them");
    assert!(s.extras.parent_group("grp_inner").is_none() && s.extras.parent_group("R3").is_none());
    assert_eq!(members(&s, "grp_inner"), ids(&["R1", "R2", "txt_a", "wire_in"]));
    // an id that is no group is skipped; none at all is refused
    assert_eq!(refuse(&mut b, V::Ungroup { ids: ids(&["R1"]) }), "ops_unknown_group");
    run(&mut b, V::Ungroup { ids: ids(&["R1", "grp_inner"]) });
    assert!(sch(&b).extras.groups.is_empty());
}

#[test]
fn add_items_pulls_each_out_of_its_old_group_and_refuses_a_loop() {
    let m = ConstraintModel::default();
    let mut b = board(&m, nested());
    // R3 is in the outer group; Add Items takes it out and into the inner one
    run(&mut b, V::AddToGroup { group_id: "grp_inner".into(), ids: ids(&["R3", "lbl_a"]) });
    let s = sch(&b);
    assert_eq!(members(&s, "grp_inner"), ids(&["R1", "R2", "R3", "lbl_a", "txt_a", "wire_in"]));
    assert!(!s.extras.is_group("grp_outer"), "the outer group is left with the inner one alone, so it dissolves");

    let mut b = board(&m, nested());
    assert_eq!(refuse(&mut b, V::AddToGroup { group_id: "grp_inner".into(), ids: ids(&["grp_outer"]) }), "ops_bad_group", "a group cannot hold the group that holds it");
    assert_eq!(refuse(&mut b, V::AddToGroup { group_id: "grp_inner".into(), ids: ids(&["grp_inner"]) }), "ops_bad_group");
    assert_eq!(refuse(&mut b, V::AddToGroup { group_id: "grp_nope".into(), ids: ids(&["lbl_a"]) }), "ops_unknown_group");
    assert_eq!(refuse(&mut b, V::AddToGroup { group_id: "grp_inner".into(), ids: ids(&["pin:R1:1"]) }), "ops_unknown_item");
}

#[test]
fn remove_items_dissolves_a_group_left_with_one() {
    let m = ConstraintModel::default();
    let mut b = board(&m, nested());
    run(&mut b, V::RemoveFromGroup { ids: ids(&["R3"]) });
    let s = sch(&b);
    // the outer group held the inner one and R3: with R3 gone it holds one thing and dissolves
    assert!(!s.extras.is_group("grp_outer") && s.extras.is_group("grp_inner"));
    assert!(s.extras.parent_group("R3").is_none());
    run(&mut b, V::RemoveFromGroup { ids: ids(&["R1", "R2", "wire_in"]) });
    assert!(sch(&b).extras.groups.is_empty(), "the inner group fell to one member and dissolved with the rest");
    assert_eq!(refuse(&mut b, V::RemoveFromGroup { ids: ids(&["R1"]) }), "ops_not_in_group", "nothing to remove changes nothing");
}

#[test]
fn group_properties_set_the_name_and_the_members() {
    let m = ConstraintModel::default();
    let mut b = board(&m, nested());
    run(&mut b, V::EditGroup { id: "grp_inner".into(), name: "power stage".into(), member_ids: ids(&["R1", "R2", "lbl_a"]) });
    let s = sch(&b);
    assert_eq!(s.extras.group("grp_inner").unwrap().name, "power stage");
    assert_eq!(members(&s, "grp_inner"), ids(&["R1", "R2", "lbl_a"]), "wire_in and txt_a are released");
    assert!(s.extras.parent_group("wire_in").is_none());
    // a member taken from another group leaves it
    run(&mut b, V::EditGroup { id: "grp_inner".into(), name: "power stage".into(), member_ids: ids(&["R1", "R2", "R3"]) });
    assert_eq!(members(&sch(&b), "grp_inner"), ids(&["R1", "R2", "R3"]));
    assert!(!sch(&b).extras.is_group("grp_outer"), "the outer group lost R3 and had only the inner one left");
    let mut b = board(&m, nested());
    assert_eq!(refuse(&mut b, V::EditGroup { id: "grp_inner".into(), name: String::new(), member_ids: ids(&["R1", "grp_outer"]) }), "ops_bad_group");
    assert_eq!(refuse(&mut b, V::EditGroup { id: "grp_nope".into(), name: String::new(), member_ids: ids(&["R1", "R2"]) }), "ops_unknown_group");
    assert_eq!(refuse(&mut b, V::EditGroup { id: "grp_inner".into(), name: String::new(), member_ids: ids(&["R1", "pin:R2:1"]) }), "ops_unknown_item");
}

#[test]
fn the_verbs_round_trip_as_json() {
    let cmd = Cmd::SchEdit(V::AddToGroup { group_id: "grp_a".into(), ids: ids(&["R1"]) });
    let json = serde_json::to_value(&cmd).unwrap();
    assert_eq!(json, serde_json::json!({ "op": "sch_edit", "verb": "add_to_group", "group_id": "grp_a", "ids": ["R1"] }));
    assert_eq!(serde_json::from_value::<Cmd>(json).unwrap(), cmd);
    assert_eq!(serde_json::to_value(Cmd::SchEdit(V::EditGroup { id: "g".into(), name: "n".into(), member_ids: ids(&["a", "b"]) })).unwrap()["verb"], "edit_group");
    assert_eq!(Cmd::SchEdit(V::Group { ids: ids(&["a", "b"]) }).domain(), Domain::Schematic);
    assert!(!Cmd::SchEdit(V::Ungroup { ids: ids(&["a"]) }).edits_connectivity(), "a group is a set of names");
}

// ------------------------------------------------------------------------------------------------------------------- the tree acts as one

fn mv(v: &[&str], dx: Um, dy: Um) -> Cmd {
    Cmd::SchMove(SchMoveCmd::Move { ids: ids(v), dx, dy, turns: Vec::new(), about: None })
}

#[test]
fn a_group_moves_everything_below_it_and_nothing_else() {
    let m = ConstraintModel::default();
    let before = nested();
    let mut b = board(&m, before.clone());
    b.apply(&mv(&["grp_outer"], 2 * G, -G)).unwrap();
    let s = sch(&b);
    let d = |q: Point| p(q.x + 2 * G, q.y - G);
    for r in ["R1", "R2", "R3"] {
        assert_eq!(symbol_at(&s, r), d(symbol_at(&before, r)), "{r} is below the outer group");
    }
    assert_eq!(s.wires.iter().find(|w| w.id == "wire_in").unwrap().pts, before.wires[0].pts.iter().map(|q| d(*q)).collect::<Vec<_>>(), "a wire of the inner group moves whole");
    assert_eq!(s.texts.iter().find(|t| t.id == "txt_a").unwrap().at, d(before.texts[0].at));
    // outside every group: the other wire, the label, the far text
    assert_eq!(s.texts.iter().find(|t| t.id == "txt_far").unwrap().at, before.texts[1].at);
    assert_eq!(s.labels[0].at, before.labels[0].at);
    // and the tree is still the tree
    assert_eq!(s.extras.groups, before.extras.groups);
}

#[test]
fn moving_a_group_is_moving_its_items_one_by_one() {
    let m = ConstraintModel::default();
    let mut by_group = board(&m, nested());
    let mut by_items = board(&m, nested());
    by_group.apply(&mv(&["grp_inner"], 3 * G, 2 * G)).unwrap();
    by_items.apply(&mv(&["R1", "R2", "wire_in", "txt_a"], 3 * G, 2 * G)).unwrap();
    let (a, c) = (sch(&by_group), sch(&by_items));
    let shape = |s: &SchematicSection| serde_json::to_value((&s.symbols, &s.wires, &s.texts, &s.labels, &s.junctions)).unwrap();
    assert_eq!(shape(&a), shape(&c));
}

#[test]
fn dragging_a_group_stretches_nothing_attached_to_it() {
    // R2's pin 2 has a wire that is not in the group. Dragged on its own R2 pulls the wire's end after it (`getConnectedDragItems`); held by the group it
    // moves as a group does (`SCH_GROUP::Move`) and the wire, attached to a group with no connection points, stays where it is.
    let m = ConstraintModel::default();
    let mut alone = board(&m, nested());
    alone.apply(&Cmd::SchMove(SchMoveCmd::Drag { ids: ids(&["R2"]), vertices: BTreeMap::new(), dx: 0, dy: 3 * G, ortho: true, grid: 0, turns: Vec::new(), about: None })).unwrap();
    let pulled = sch(&alone).wires.iter().find(|w| w.id == "wire_out").unwrap().pts.clone();
    assert_ne!(pulled, nested().wires[1].pts, "the wire of a dragged symbol follows it");

    let mut held = board(&m, nested());
    held.apply(&Cmd::SchMove(SchMoveCmd::Drag { ids: ids(&["grp_inner"]), vertices: BTreeMap::new(), dx: 0, dy: 3 * G, ortho: true, grid: 0, turns: Vec::new(), about: None })).unwrap();
    let s = sch(&held);
    assert_eq!(s.wires.iter().find(|w| w.id == "wire_out").unwrap().pts, nested().wires[1].pts, "the wire outside the group stays");
    assert_eq!(symbol_at(&s, "R2"), p(40_640, 25_400 + 3 * G));
    assert_eq!(symbol_at(&s, "R1"), p(20_320, 25_400 + 3 * G));
}

#[test]
fn a_lone_group_turns_on_itself_about_the_half_grid_point_nearest_its_centre() {
    let m = ConstraintModel::default();
    let before = nested();
    let mut b = board(&m, before.clone());
    b.apply(&Cmd::SchMove(SchMoveCmd::Rotate { ids: ids(&["grp_inner"]), vertices: BTreeMap::new(), ccw: true, about: None, grid: 0 })).unwrap();
    let s = sch(&b);
    // R1 and R2 are now turned a quarter (counter-clockwise: 90 degrees) and their places have gone round the same point
    assert_eq!(s.symbols.iter().find(|x| x.id == "R1").unwrap().rot, 90_000);
    assert_eq!(s.symbols.iter().find(|x| x.id == "R2").unwrap().rot, 90_000);
    let (r1, r2) = (symbol_at(&s, "R1"), symbol_at(&s, "R2"));
    // two points 20.32 mm apart on a row are now 20.32 mm apart on a column (R2 above R1: the sheet's y runs down, counter-clockwise takes right to up)
    assert_eq!((r2.x - r1.x, r2.y - r1.y), (0, -20_320));
    // R3 is in the outer group, not the inner one: it stays
    assert_eq!(symbol_at(&s, "R3"), symbol_at(&before, "R3"));
    // the tree is unchanged
    assert_eq!(s.extras.groups, before.extras.groups);
    // four turns bring the group home
    for _ in 0..3 {
        b.apply(&Cmd::SchMove(SchMoveCmd::Rotate { ids: ids(&["grp_inner"]), vertices: BTreeMap::new(), ccw: true, about: None, grid: 0 })).unwrap();
    }
    let home = sch(&b);
    assert_eq!(home.symbols.iter().find(|x| x.id == "R1").unwrap().rot, 0);
}

#[test]
fn a_lone_group_mirrors_about_its_own_centre() {
    let m = ConstraintModel::default();
    let before = nested();
    let mut b = board(&m, before.clone());
    b.apply(&Cmd::SchMove(SchMoveCmd::Mirror { ids: ids(&["grp_inner"]), vertices: BTreeMap::new(), vertical: false, about: None, grid: 0 })).unwrap();
    let s = sch(&b);
    let (r1b, r2b) = (symbol_at(&before, "R1"), symbol_at(&before, "R2"));
    let (r1, r2) = (symbol_at(&s, "R1"), symbol_at(&s, "R2"));
    // left-right: R1 and R2 trade sides about the same axis, the row stays a row
    assert_eq!((r1.y, r2.y), (r1b.y, r2b.y));
    assert!(r1.x > r2.x, "R1 was on the left and is now on the right: {r1:?} {r2:?}");
    assert_eq!(r1.x + r2.x, r1b.x + r2b.x, "about the same axis");
}

#[test]
fn a_group_with_a_locked_item_below_it_is_no_selection() {
    let m = ConstraintModel::default();
    let mut s = nested();
    s.extras.set_locked("txt_a", true);
    let mut b = board(&m, s);
    let err = b.apply(&mv(&["grp_outer"], G, 0)).unwrap_err();
    assert_eq!(err[0].check, "ops_locked", "`FilterSelectionForLockedItems`: a locked descendant keeps the whole group still");
    assert_eq!(symbol_at(&sch(&b), "R1"), p(20_320, 25_400));
    // locking the group locks what is below it
    let mut s = nested();
    s.extras.set_locked("grp_inner", true);
    let mut b = board(&m, s);
    assert!(b.apply(&mv(&["grp_outer"], G, 0)).is_err());
    assert!(b.apply(&mv(&["R1"], G, 0)).is_err(), "R1 is locked through its group");
    assert!(b.apply(&mv(&["R3"], G, 0)).is_ok(), "R3 is in the outer group but not the locked inner one");
}

#[test]
fn the_lock_verb_takes_a_group() {
    let m = ConstraintModel::default();
    let mut b = board(&m, nested());
    run(&mut b, V::SetLocked { ids: ids(&["grp_inner"]), locked: true });
    assert!(sch(&b).extras.is_locked("grp_inner"));
}

// ------------------------------------------------------------------------------------------------------------- the tree stays honest

#[test]
fn deleting_members_takes_them_out_of_their_groups() {
    let m = ConstraintModel::default();
    let mut b = board(&m, nested());
    b.apply(&Cmd::DeleteSchText { id: "txt_a".into() }).unwrap();
    let s = sch(&b);
    assert_eq!(members(&s, "grp_inner"), ids(&["R1", "R2", "wire_in"]));
    b.apply(&Cmd::DeleteWire { id: "wire_in".into() }).unwrap();
    assert_eq!(members(&sch(&b), "grp_inner"), ids(&["R1", "R2"]));
    b.apply(&Cmd::DeleteSymbol { id: "R2".into(), unit: None }).unwrap();
    let s = sch(&b);
    assert!(!s.extras.is_group("grp_inner"), "a group of one dissolves");
    assert!(s.extras.groups.is_empty(), "and the group above it, left with R3 alone, dissolves too");
}

#[test]
fn a_renamed_reference_stays_in_its_group() {
    let m = ConstraintModel::default();
    let mut b = board(&m, nested());
    b.apply(&Cmd::RenameSymbol { id: "R1".into(), new_id: "R10".into() }).unwrap();
    let s = sch(&b);
    assert!(s.symbols.iter().any(|x| x.id == "R10"));
    assert_eq!(members(&s, "grp_inner"), ids(&["R10", "R2", "txt_a", "wire_in"]));
}

#[test]
fn an_item_made_while_a_group_is_entered_joins_it() {
    let m = ConstraintModel::default();
    let mut b = board(&m, nested());
    let add = Cmd::AddSchText { content: "new".into(), at: p(10_160, 76_200), angle_millideg: 0, size_um: 1_270 };
    b.apply(&Cmd::InGroup { group: "grp_inner".into(), cmd: Box::new(add) }).unwrap();
    let s = sch(&b);
    let new_id = s.texts.iter().find(|t| t.content == "new").unwrap().id.clone();
    assert!(members(&s, "grp_inner").contains(&new_id), "{:?}", s.extras.groups);
    assert_eq!(s.extras.parent_group(&new_id).map(|g| g.id.as_str()), Some("grp_inner"));
    // a group that is gone is not joined, and the command still stands
    let add2 = Cmd::AddSchText { content: "second".into(), at: p(10_160, 88_900), angle_millideg: 0, size_um: 1_270 };
    b.apply(&Cmd::InGroup { group: "grp_gone".into(), cmd: Box::new(add2) }).unwrap();
    assert!(sch(&b).texts.iter().any(|t| t.content == "second"));
}

#[test]
fn groups_belong_to_the_sheet_they_are_on() {
    // a group of a sheet in `sheet_contents` is that sheet's own
    let m = ConstraintModel::default();
    let mut root = sheet();
    root.sheets.push(eda_model::ir::SheetInstance { id: "sheet_a".into(), name: "Sub".into(), file: "sub.kicad_sch".into(), at: p(190_500, 12_700), size: (25_400, 12_700), pins: Vec::new(), page: String::new() });
    let mut d = design_with(root);
    let mut child = SchematicSection { imported_from_kicad: true, ..Default::default() };
    child.symbols.push(sym("R5", p(20_320, 25_400)));
    child.symbols.push(sym("R6", p(40_640, 25_400)));
    child.assign_missing_ids();
    d.sheet_contents = Some([("sub.kicad_sch".to_string(), child)].into_iter().collect());
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::OnSheet { sheet: "sheet_a".into(), cmd: Box::new(Cmd::SchEdit(V::Group { ids: ids(&["R5", "R6"]) })) }).unwrap();
    assert_eq!(b.design().sheet_contents.as_ref().unwrap()["sub.kicad_sch"].extras.groups.len(), 1);
    assert!(b.design().schematic.as_ref().unwrap().extras.groups.is_empty(), "the root has none");
    // R1 is not on the sub sheet
    let err = b.apply(&Cmd::OnSheet { sheet: "sheet_a".into(), cmd: Box::new(Cmd::SchEdit(V::Group { ids: ids(&["R5", "R1"]) })) }).unwrap_err();
    assert_eq!(err[0].check, "ops_unknown_item");
    // and a delete on the sub sheet tidies the sub sheet's groups
    b.apply(&Cmd::OnSheet { sheet: "sheet_a".into(), cmd: Box::new(Cmd::DeleteSymbol { id: "R6".into(), unit: None }) }).unwrap();
    assert!(b.design().sheet_contents.as_ref().unwrap()["sub.kicad_sch"].extras.groups.is_empty());
}

// ------------------------------------------------------------------------------------------------------------------- copy and paste

use eda_model::ir::LibrarySymbol;
use eda_model::sch_clipboard::{PasteMode, SchFragment};

/// A fragment in KiCad's frame with `inner = {R1, w1}` held by `outer = {inner, R2, l1}`, the way a copy of a group reads back (ids as the reader gave them).
fn grouped_fragment() -> SchFragment {
    let mut section = SchematicSection { imported_from_kicad: true, ..Default::default() };
    section.symbols.push(sym("R1", p(100_000, 50_000)));
    section.symbols.push(sym("R2", p(120_000, 50_000)));
    section.wires.push(wire("w1", &[p(100_000, 46_190), p(120_000, 46_190)]));
    section.labels.push(NetLabel { id: "l1".into(), net: "SDA".into(), at: p(130_000, 46_190), kind: LabelKind::Local });
    section.texts.push(SchematicText { id: "t1".into(), content: "outside".into(), at: p(100_000, 70_000), angle: 0, size_um: 1_270 });
    section.extras.groups = vec![Group { id: "grp_a".into(), name: "inner".into(), member_ids: ids(&["R1", "w1"]) }, Group { id: "grp_b".into(), name: "outer".into(), member_ids: ids(&["grp_a", "R2", "l1"]) }];
    let lib_symbols = ["Device:R"].iter().filter_map(|id| eda_model::symbol::builtin(id)).map(|s| LibrarySymbol::from_engine_symbol(&s)).collect();
    SchFragment { section, lib_symbols, notes: vec![], sheet_screens: Default::default() }
}

fn paste(f: &SchFragment, dx: Um, dy: Um) -> Cmd {
    Cmd::PasteSch { fragment: f.clone(), dx, dy, mode: PasteMode::Unique }
}

#[test]
fn a_pasted_group_tree_comes_in_whole_with_new_ids() {
    let m = ConstraintModel::default();
    // the sheet already has R1 and R2 (and a group of its own): the pasted resistors are numbered anew
    let mut b = board(&m, nested());
    let before = sch(&b).extras.groups.clone();
    b.apply(&paste(&grouped_fragment(), 0, 20_000)).unwrap();
    let s = sch(&b);
    assert_eq!(s.extras.groups.len(), before.len() + 2, "two groups came in: {:?}", s.extras.groups);
    assert_eq!(&s.extras.groups[..before.len()], &before[..], "the groups of the sheet are as they were");
    let new_groups: Vec<&Group> = s.extras.groups.iter().filter(|g| !before.iter().any(|o| o.id == g.id)).collect();
    let outer = new_groups.iter().find(|g| g.name == "outer").unwrap();
    let inner = new_groups.iter().find(|g| g.name == "inner").unwrap();
    assert!(outer.id.starts_with("grp_") && inner.id.starts_with("grp_"));
    assert!(outer.member_ids.contains(&inner.id), "the inner group is still in the outer one: {outer:?}");
    assert_eq!(s.extras.parent_group(&inner.id).map(|g| g.id.clone()), Some(outer.id.clone()));
    // the pasted symbols took the next free references, and the groups name those
    let leaves = s.extras.group_leaves(&outer.id);
    assert_eq!(leaves.len(), 4, "R, R, the wire and the label: {leaves:?}");
    for leaf in &leaves {
        let known = s.symbols.iter().any(|x| &x.id == leaf) || s.wires.iter().any(|x| &x.id == leaf) || s.labels.iter().any(|x| &x.id == leaf);
        assert!(known, "{leaf} is an item of the sheet");
    }
    assert!(!leaves.contains(&"R1".to_string()) && !leaves.contains(&"R2".to_string()), "not the sheet's own R1 and R2: {leaves:?}");
    // the text was not in a group
    let t = s.texts.iter().find(|t| t.content == "outside").unwrap();
    assert!(s.extras.parent_group(&t.id).is_none());
    // the group that was there is the same group
    assert_eq!(members(&s, "grp_inner"), ids(&["R1", "R2", "txt_a", "wire_in"]));
}

#[test]
fn a_pasted_tree_moves_as_a_tree() {
    // a pasted group is a group like any other: moved by its id, every member goes
    let m = ConstraintModel::default();
    let mut b = Board::new(design_with(SchematicSection { imported_from_kicad: true, ..Default::default() }), &m, 100, 300);
    b.apply(&paste(&grouped_fragment(), 0, 0)).unwrap();
    let before = sch(&b);
    let outer = before.extras.groups.iter().find(|g| g.name == "outer").unwrap().id.clone();
    b.apply(&mv(&[outer.as_str()], 4 * G, 0)).unwrap();
    let after = sch(&b);
    for r in &before.extras.group_leaves(&outer) {
        if let Some(s) = before.symbols.iter().find(|s| &s.id == r) {
            assert_eq!(symbol_at(&after, r), p(s.at.x + 4 * G, s.at.y), "{r}");
        }
    }
    assert_eq!(after.texts.iter().find(|t| t.content == "outside").unwrap().at, before.texts.iter().find(|t| t.content == "outside").unwrap().at, "a text outside the groups stays");
}

#[test]
fn a_paste_into_an_entered_group_joins_it_as_a_member() {
    let m = ConstraintModel::default();
    let mut b = board(&m, nested());
    b.apply(&Cmd::InGroup { group: "grp_outer".into(), cmd: Box::new(paste(&grouped_fragment(), 0, 20_000)) }).unwrap();
    let s = sch(&b);
    // the pasted tree's top group is a member of the entered group; its own members stay in it
    let top = s.extras.groups.iter().find(|g| g.name == "outer" && g.id != "grp_outer").unwrap();
    assert_eq!(s.extras.parent_group(&top.id).map(|g| g.id.as_str()), Some("grp_outer"), "{:?}", s.extras.groups);
    // the pasted text is in no group of the fragment, so it joins the entered one itself
    let t = s.texts.iter().find(|t| t.content == "outside").unwrap();
    assert_eq!(s.extras.parent_group(&t.id).map(|g| g.id.as_str()), Some("grp_outer"));
}

#[test]
fn a_member_that_was_not_pasted_is_left_out_of_the_pasted_group() {
    let m = ConstraintModel::default();
    let mut b = board(&m, SchematicSection { imported_from_kicad: true, ..Default::default() });
    let mut f = grouped_fragment();
    // the outer group also lists an id the fragment does not have, and the inner one a wire that has fewer than two points (not pasted)
    f.section.extras.groups[1].member_ids.push("ghost".into());
    f.section.wires.push(wire("w_null", &[p(0, 0)]));
    f.section.extras.groups[0].member_ids.push("w_null".into());
    b.apply(&paste(&f, 0, 0)).unwrap();
    let s = sch(&b);
    assert_eq!(s.extras.groups.len(), 2);
    for g in &s.extras.groups {
        assert!(!g.member_ids.iter().any(|m| m == "ghost" || m == "w_null"), "{g:?}");
    }
}
