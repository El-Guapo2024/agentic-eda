//! The board's table verbs: one test per verb, and the generic verbs (move, turn, flip, lock, group) on a table.

use super::pcb_transform_tests::{board, id, model, p};
use super::*;
use eda_model::table::{Table, TableBorder, TableCell, TableSeparators, TableStroke};
use eda_model::table_edit::{CellEdit, TablePatch};
use eda_model::units::mm;

/// A `rows` x `cols` table of 10 x 5 mm cells at (10, 20) mm on `layer`, each cell's text its row and column (`r0c1`).
fn table(rows: usize, cols: usize, layer: &str) -> Table {
    let mut cells: Vec<TableCell> = (0..rows * cols).map(|_| TableCell::pcb_new(false)).collect();
    for (i, c) in cells.iter_mut().enumerate() {
        c.text = format!("r{}c{}", i / cols, i % cols);
    }
    Table {
        id: String::new(),
        uuid: "stale-uuid".into(),
        layer: layer.into(),
        at: p(mm(10.0), mm(20.0)),
        angle: 0,
        col_widths: vec![mm(10.0); cols],
        row_heights: vec![mm(5.0); rows],
        cells,
        border: TableBorder { external: true, header: true, stroke: TableStroke::solid(mm(0.1)) },
        separators: TableSeparators { rows: true, cols: true, stroke: TableStroke::solid(mm(0.1)) },
    }
}

fn add(b: &mut Board<'_>, t: Table) -> String {
    b.apply(&Cmd::Table(TableCmd::Add { table: t })).expect("adds");
    b.design().drawings.as_ref().unwrap().tables.last().unwrap().id.clone()
}

fn get<'a>(b: &'a Board<'_>, tid: &str) -> &'a Table {
    b.design().drawings.as_ref().unwrap().tables.iter().find(|t| t.id == tid).unwrap_or_else(|| panic!("no table {tid}"))
}

fn tables(b: &Board<'_>) -> usize {
    b.design().drawings.as_ref().unwrap().tables.len()
}

#[test]
fn a_table_added_gets_an_id_of_its_own_and_no_uuid_it_brought() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let a = add(&mut b, table(2, 2, "F.SilkS"));
    assert!(a.starts_with("tbl_"), "{a}");
    assert!(get(&b, &a).uuid.is_empty(), "a table made from another starts its uuids again");
    // the same table added twice is two tables with two ids
    let again = add(&mut b, table(2, 2, "F.SilkS"));
    assert_ne!(a, again);
    assert_eq!(tables(&b), 2);
    // refused: a layer the board does not have on, no layer, not a grid
    assert!(b.apply(&Cmd::Table(TableCmd::Add { table: table(1, 1, "In7.Cu") })).is_err());
    assert!(b.apply(&Cmd::Table(TableCmd::Add { table: table(1, 1, "") })).is_err());
    let mut broken = table(2, 2, "F.SilkS");
    broken.cells.pop();
    assert!(b.apply(&Cmd::Table(TableCmd::Add { table: broken })).is_err());
    assert_eq!(tables(&b), 2, "a refused verb changes nothing");
}

#[test]
fn rows_and_columns_are_added_and_deleted_and_deleting_them_all_deletes_the_table() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let t = add(&mut b, table(2, 2, "F.SilkS"));
    b.apply(&Cmd::Table(TableCmd::AddRow { id: t.clone(), row: 0, above: false })).unwrap();
    assert_eq!((get(&b, &t).row_count(), get(&b, &t).col_count()), (3, 2));
    b.apply(&Cmd::Table(TableCmd::AddRow { id: t.clone(), row: 0, above: true })).unwrap();
    b.apply(&Cmd::Table(TableCmd::AddColumn { id: t.clone(), col: 1, before: true })).unwrap();
    b.apply(&Cmd::Table(TableCmd::AddColumn { id: t.clone(), col: 2, before: false })).unwrap();
    let g = get(&b, &t);
    assert_eq!((g.row_count(), g.col_count()), (4, 4));
    assert!(g.is_well_formed());
    b.apply(&Cmd::Table(TableCmd::DeleteRows { id: t.clone(), rows: vec![0, 1] })).unwrap();
    b.apply(&Cmd::Table(TableCmd::DeleteColumns { id: t.clone(), cols: vec![1, 2, 3] })).unwrap();
    let g = get(&b, &t);
    assert_eq!((g.row_count(), g.col_count()), (2, 1));
    assert!(g.is_well_formed());
    // no such row or column
    assert!(b.apply(&Cmd::Table(TableCmd::AddRow { id: t.clone(), row: 9, above: true })).is_err());
    assert!(b.apply(&Cmd::Table(TableCmd::DeleteColumns { id: t.clone(), cols: vec![5] })).is_err());
    assert!(b.apply(&Cmd::Table(TableCmd::DeleteRows { id: t.clone(), rows: vec![] })).is_err());
    // every row: the table goes
    b.apply(&Cmd::Table(TableCmd::DeleteRows { id: t.clone(), rows: vec![0, 1] })).unwrap();
    assert_eq!(tables(&b), 0);
    let t2 = add(&mut b, table(2, 3, "F.SilkS"));
    b.apply(&Cmd::Table(TableCmd::DeleteColumns { id: t2, cols: vec![0, 1, 2] })).unwrap();
    assert_eq!(tables(&b), 0);
}

#[test]
fn cells_are_merged_into_the_top_left_one_and_unmerged_again() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let t = add(&mut b, table(3, 3, "F.SilkS"));
    b.apply(&Cmd::Table(TableCmd::MergeCells { id: t.clone(), cells: vec![(0, 0), (0, 1), (1, 0), (1, 1)] })).unwrap();
    let g = get(&b, &t);
    assert_eq!((g.cell(0, 0).unwrap().col_span, g.cell(0, 0).unwrap().row_span), (2, 2));
    assert_eq!(g.cell(0, 0).unwrap().text, "r0c0\nr0c1\nr1c0\nr1c1");
    assert!(g.cell(1, 1).unwrap().is_covered());
    // a diagonal is not a block, a single cell is not worth merging
    assert!(b.apply(&Cmd::Table(TableCmd::MergeCells { id: t.clone(), cells: vec![(2, 0), (2, 2)] })).is_err());
    assert!(b.apply(&Cmd::Table(TableCmd::MergeCells { id: t.clone(), cells: vec![(2, 0)] })).is_err());
    // nothing merged among these
    assert!(b.apply(&Cmd::Table(TableCmd::UnmergeCells { id: t.clone(), cells: vec![(2, 2)] })).is_err());
    b.apply(&Cmd::Table(TableCmd::UnmergeCells { id: t.clone(), cells: vec![(0, 0)] })).unwrap();
    assert!(get(&b, &t).cells.iter().all(|c| c.col_span == 1 && c.row_span == 1));
}

#[test]
fn the_dialogs_edit_the_cells_the_border_the_layer_and_the_lock() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let t = add(&mut b, table(2, 2, "F.SilkS"));
    b.apply(&Cmd::Table(TableCmd::EditCells { id: t.clone(), cells: vec![(0, 0), (0, 1)], edit: CellEdit { text: Some("Header".into()), bold: Some(true), h_align: Some(eda_model::ir::TextJustify::Center), ..CellEdit::default() } })).unwrap();
    let g = get(&b, &t);
    assert!(g.cell(0, 0).unwrap().style.bold && g.cell(0, 1).unwrap().style.bold && !g.cell(1, 0).unwrap().style.bold);
    assert_eq!(g.cell(0, 1).unwrap().text, "Header");
    // Table Properties: layer, lock, the grid and the borders in one step
    let patch = TablePatch {
        texts: Some(vec![vec!["a".into(), "b".into()], vec!["c".into(), "d".into()]]),
        border: Some(eda_model::table_edit::BorderEdit { external: false, header: true, width_nm: mm(0.2), style: eda_model::table::TableLineStyle::Dash }),
        ..TablePatch::default()
    };
    b.apply(&Cmd::Table(TableCmd::Set { id: t.clone(), layer: Some("F.Fab".into()), locked: Some(true), patch })).unwrap();
    let g = get(&b, &t);
    assert_eq!(g.layer, "F.Fab");
    assert_eq!(g.texts(), vec![vec!["a", "b"], vec!["c", "d"]]);
    assert!(!g.border.external && g.border.header && g.border.stroke.width_nm == mm(0.2));
    assert!(b.design().drawings.as_ref().unwrap().locked_ids.contains(&t), "locked");
    b.apply(&Cmd::Table(TableCmd::Set { id: t.clone(), layer: None, locked: Some(false), patch: TablePatch::default() })).unwrap();
    assert!(!b.design().drawings.as_ref().unwrap().locked_ids.contains(&t), "unlocked");
    // a layer the board does not have on, an unknown table
    assert!(b.apply(&Cmd::Table(TableCmd::Set { id: t.clone(), layer: Some("In9.Cu".into()), locked: None, patch: TablePatch::default() })).is_err());
    assert!(b.apply(&Cmd::Table(TableCmd::Delete { id: id("tbl_nope") })).is_err());
    b.apply(&Cmd::Table(TableCmd::Delete { id: t })).unwrap();
    assert_eq!(tables(&b), 0);
}

#[test]
fn a_table_moves_turns_and_flips_with_the_generic_verbs_and_is_lockable_and_groupable() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let t = add(&mut b, table(2, 3, "F.SilkS"));
    let before = get(&b, &t).clone();
    b.apply(&Cmd::MoveItems { ids: vec![t.clone()], dx: mm(5.0), dy: mm(-2.0) }).unwrap();
    assert_eq!(get(&b, &t).at, p(mm(15.0), mm(18.0)));
    // a quarter turn clockwise about the table's own corner: the table's angle runs the other way (KiCad's), so it reads 270 degrees
    b.apply(&Cmd::RotateItems { ids: vec![t.clone()], pivot: get(&b, &t).at, angle_millideg: 90_000 }).unwrap();
    assert_eq!(get(&b, &t).angle, 270_000);
    b.apply(&Cmd::RotateItems { ids: vec![t.clone()], pivot: get(&b, &t).at, angle_millideg: -90_000 }).unwrap();
    assert_eq!(get(&b, &t).angle, 0);
    // flipped to the back: the layer, the columns and the text's mirror follow
    let bx = get(&b, &t).bounding_box();
    b.apply(&Cmd::FlipItems { ids: vec![t.clone()], pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    let g = get(&b, &t);
    assert_eq!(g.layer, "B.SilkS");
    assert_eq!(g.texts()[0], vec!["r0c2", "r0c1", "r0c0"]);
    assert!(g.cells.iter().all(|c| c.style.mirror));
    let fb = g.bounding_box();
    assert_eq!((fb.x0, fb.x1), (-bx.x1, -bx.x0));
    // lock and group it like any other item
    b.apply(&Cmd::SetLocked { ids: vec![t.clone()], locked: true }).unwrap();
    assert!(b.design().drawings.as_ref().unwrap().locked_ids.contains(&t));
    let other = add(&mut b, table(1, 1, "F.SilkS"));
    b.apply(&Cmd::Group { ids: vec![t.clone(), other.clone()] }).unwrap();
    assert_eq!(b.design().drawings.as_ref().unwrap().groups.len(), 1);
    // deleting one of them takes it out of the group, which stays with the other (`BOARD_COMMIT::Push` dissolves nothing)
    b.apply(&Cmd::Table(TableCmd::Delete { id: other })).unwrap();
    let groups = &b.design().drawings.as_ref().unwrap().groups;
    assert_eq!((groups.len(), groups[0].member_ids.clone()), (1, vec![t.clone()]));
    let _ = before;
}

#[test]
fn draw_table_makes_a_grid_with_kicads_cell_defaults_and_what_its_dialog_set() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let patch = TablePatch { texts: Some(vec![vec!["a".into(), "b".into(), "c".into()], vec!["d".into(), "e".into(), "f".into()]]), ..TablePatch::default() };
    b.apply(&Cmd::Table(TableCmd::Draw { layer: "F.SilkS".into(), at: p(mm(5.0), mm(6.0)), cols: 3, rows: 2, cell_w_nm: mm(12.0), cell_h_nm: mm(4.0), line_width_nm: mm(0.15), locked: true, patch })).unwrap();
    let t = b.design().drawings.as_ref().unwrap().tables[0].clone();
    assert_eq!((t.col_count(), t.row_count(), t.at), (3, 2, p(mm(5.0), mm(6.0))));
    assert_eq!(t.col_widths, vec![mm(12.0); 3]);
    assert_eq!(t.texts()[1], vec!["d", "e", "f"]);
    // the cells are `new PCB_TABLECELL`: 50 mil text, automatic thickness, left and centred, a margin of the legacy formula, not mirrored on a front layer
    let c = t.cell(0, 0).unwrap();
    assert_eq!((c.style.height_nm, c.style.thickness_nm, c.margins, c.style.mirror), (mm(1.27), Nm::ZERO, [Nm::new(1_002_500); 4], false));
    assert_eq!((t.border.stroke.width_nm, t.separators.stroke.width_nm), (mm(0.15), mm(0.15)));
    assert!(b.design().drawings.as_ref().unwrap().locked_ids.contains(&t.id));
    // on a back layer the cells are mirrored
    b.apply(&Cmd::Table(TableCmd::Draw { layer: "B.SilkS".into(), at: p(Nm::ZERO, Nm::ZERO), cols: 1, rows: 1, cell_w_nm: mm(5.0), cell_h_nm: mm(5.0), line_width_nm: mm(0.1), locked: false, patch: TablePatch::default() })).unwrap();
    assert!(b.design().drawings.as_ref().unwrap().tables.iter().any(|t| t.layer == "B.SilkS" && t.cells[0].style.mirror));
    // refused: no size, no columns
    assert!(b.apply(&Cmd::Table(TableCmd::Draw { layer: "F.SilkS".into(), at: p(Nm::ZERO, Nm::ZERO), cols: 0, rows: 1, cell_w_nm: mm(5.0), cell_h_nm: mm(5.0), line_width_nm: mm(0.1), locked: false, patch: TablePatch::default() })).is_err());
    assert!(b.apply(&Cmd::Table(TableCmd::Draw { layer: "F.SilkS".into(), at: p(Nm::ZERO, Nm::ZERO), cols: 1, rows: 1, cell_w_nm: Nm::ZERO, cell_h_nm: mm(5.0), line_width_nm: mm(0.1), locked: false, patch: TablePatch::default() })).is_err());
}

fn numbers() -> crate::pcb_table_builders::BoardNumbers {
    crate::pcb_table_builders::BoardNumbers { thickness_nm: mm(1.6), width_nm: mm(80.0), height_nm: mm(60.0), min_track_nm: mm(0.15), min_clearance_nm: mm(0.18), min_drill_nm: mm(0.3), castellated_pads: 2, press_fit_pads: 0 }
}

#[test]
fn place_stackup_table_builds_the_boards_stackup_at_the_point_in_the_users_units() {
    let m = model(&["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::Table(TableCmd::PlaceStackup { layer: "F.SilkS".into(), at: p(mm(10.0), mm(20.0)), units: Some("mil".into()) })).unwrap();
    let t = b.design().drawings.as_ref().unwrap().tables[0].clone();
    assert!(!t.id.is_empty(), "the table has an id of its own");
    assert_eq!((t.col_count(), t.layer.as_str(), t.at), (7, "F.SilkS", p(mm(10.0), mm(20.0))));
    // a header row and a row for each of the 13 layers of a four layer board
    assert_eq!(t.row_count(), 14);
    assert_eq!(t.cell(4, 0).unwrap().text, "F.Cu");
    assert_eq!(t.cell(4, 3).unwrap().text, "1.37795 mils", "0.035 mm in the user's units");
    assert_eq!(t.cell(5, 0).unwrap().text, "Dielectric 1");
    // what the board setup says wins over the default: a thicker front copper shows
    let mut settings = eda_model::rules::StackupSettings::new(4, mm(1.6));
    settings.stackup.layers.iter_mut().find(|l| l.name == "F.Cu").unwrap().thickness_mm = Some(0.07);
    settings.stackup.copper_finish = Some("ENIG".into());
    b.apply(&Cmd::SetStackup { settings }).unwrap();
    b.apply(&Cmd::Table(TableCmd::PlaceStackup { layer: "Dwgs.User".into(), at: p(Nm::ZERO, Nm::ZERO), units: None })).unwrap();
    let second = b.design().drawings.as_ref().unwrap().tables[1].clone();
    assert_eq!(second.cell(4, 3).unwrap().text, "0.07 mm");
    assert_ne!(second.id, t.id);
    // a layer the board does not have is refused
    assert!(b.apply(&Cmd::Table(TableCmd::PlaceStackup { layer: "In9.Cu".into(), at: p(Nm::ZERO, Nm::ZERO), units: None })).is_err());
}

#[test]
fn place_board_characteristics_builds_the_table_from_the_stackup_and_the_numbers_it_is_given() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let mut settings = eda_model::rules::StackupSettings::new(2, mm(1.6));
    settings.stackup.copper_finish = Some("HASL".into());
    settings.stackup.edge_plating = true;
    b.apply(&Cmd::SetStackup { settings }).unwrap();
    b.apply(&Cmd::Table(TableCmd::PlaceCharacteristics { layer: "Dwgs.User".into(), at: p(mm(-5.0), mm(7.0)), units: None, numbers: numbers() })).unwrap();
    let t = b.design().drawings.as_ref().unwrap().tables[0].clone();
    assert_eq!((t.col_count(), t.row_count(), t.at, t.layer.as_str()), (4, 7, p(mm(-5.0), mm(7.0)), "Dwgs.User"));
    let row = |r: usize| (0..4).map(|c| t.cell(r, c).unwrap().text.clone()).collect::<Vec<_>>();
    assert_eq!(row(1), ["Copper layer count: ", "2", "Board thickness: ", "1.6000 mm"]);
    assert_eq!(row(2)[1], "80.0000 mm x 60.0000 mm");
    assert_eq!(row(3), ["Min track/spacing: ", "0.1500 mm / 0.1800 mm", "Min hole diameter: ", "0.3000 mm"]);
    assert_eq!(row(4), ["Copper finish: ", "HASL", "Impedance control: ", "No"]);
    assert_eq!(row(5), ["Castellated pads: ", "Yes", "Press-fit pads: ", "No"]);
    assert_eq!(row(6), ["Plated board edge: ", "Yes", "Edge card connectors: ", "No"]);
    assert!(!t.border.external && !t.separators.cols);
}

#[test]
fn the_place_verbs_read_from_and_write_to_the_json_the_studio_sends() {
    let cmd: Cmd = serde_json::from_value(serde_json::json!({ "op": "table", "verb": "place_stackup", "layer": "F.SilkS", "at": { "x": 100_000, "y": 200_000 }, "units": "in" })).unwrap();
    assert_eq!(cmd, Cmd::Table(TableCmd::PlaceStackup { layer: "F.SilkS".into(), at: p(mm(0.1), mm(0.2)), units: Some("in".into()) }));
    let text = r#"{"op":"table","verb":"place_characteristics","layer":"Dwgs.User","at":{"x":0,"y":0},"numbers":{"thickness_nm":1600000,"width_nm":1,"height_nm":2,"min_track_nm":3,"min_clearance_nm":4,"min_drill_nm":5,"castellated_pads":1}}"#;
    let value: serde_json::Value = serde_json::from_str(text).unwrap();
    let cmd: Cmd = serde_json::from_value(value).unwrap();
    assert!(matches!(&cmd, Cmd::Table(TableCmd::PlaceCharacteristics { units: None, numbers, .. }) if numbers.castellated_pads == 1 && numbers.press_fit_pads == 0 && numbers.min_drill_nm == Nm::new(5)));
}

#[test]
fn the_verbs_read_from_and_write_to_the_json_the_studio_sends() {
    let cmd = Cmd::Table(TableCmd::AddRow { id: "tbl_x".into(), row: 2, above: true });
    let value = serde_json::to_value(&cmd).unwrap();
    assert_eq!(value, serde_json::json!({ "op": "table", "verb": "add_row", "id": "tbl_x", "row": 2, "above": true }));
    let back: Cmd = serde_json::from_value(value).unwrap();
    assert_eq!(back, cmd);
    // the flags default
    let minimal: Cmd = serde_json::from_value(serde_json::json!({ "op": "table", "verb": "add_column", "id": "tbl_x", "col": 0 })).unwrap();
    assert_eq!(minimal, Cmd::Table(TableCmd::AddColumn { id: "tbl_x".into(), col: 0, before: false }));
    // a whole table travels in a command: the numbers are integers, so it reads from a parsed body too
    let add = Cmd::Table(TableCmd::Add { table: table(2, 2, "F.SilkS") });
    let text = serde_json::to_string(&add).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let back: Cmd = serde_json::from_value(value).unwrap();
    assert_eq!(back, add);
    let edit: Cmd = serde_json::from_value(serde_json::json!({ "op": "table", "verb": "edit_cells", "id": "tbl_x", "cells": [[0, 1], [1, 1]], "edit": { "text": "x", "margins": [null, 500_000, null, null] } })).unwrap();
    assert!(matches!(&edit, Cmd::Table(TableCmd::EditCells { cells, edit, .. }) if cells == &vec![(0, 1), (1, 1)] && edit.margins[1] == Some(mm(0.5))));
}
