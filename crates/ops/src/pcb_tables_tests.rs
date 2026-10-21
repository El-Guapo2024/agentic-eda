//! The board's table verbs: one test per verb, and the generic verbs (move, turn, flip, lock, group) on a table.

use super::pcb_transform_tests::{board, id, model, p};
use super::*;
use eda_model::table::{Table, TableBorder, TableCell, TableSeparators, TableStroke};
use eda_model::table_edit::{CellEdit, TablePatch};

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
        at: p(10_000, 20_000),
        angle: 0,
        col_widths: vec![10_000; cols],
        row_heights: vec![5_000; rows],
        cells,
        border: TableBorder { external: true, header: true, stroke: TableStroke::solid(100) },
        separators: TableSeparators { rows: true, cols: true, stroke: TableStroke::solid(100) },
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
        border: Some(eda_model::table_edit::BorderEdit { external: false, header: true, width_um: 200, style: eda_model::table::TableLineStyle::Dash }),
        ..TablePatch::default()
    };
    b.apply(&Cmd::Table(TableCmd::Set { id: t.clone(), layer: Some("F.Fab".into()), locked: Some(true), patch })).unwrap();
    let g = get(&b, &t);
    assert_eq!(g.layer, "F.Fab");
    assert_eq!(g.texts(), vec![vec!["a", "b"], vec!["c", "d"]]);
    assert!(!g.border.external && g.border.header && g.border.stroke.width_um == 200);
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
    b.apply(&Cmd::MoveItems { ids: vec![t.clone()], dx: 5_000, dy: -2_000 }).unwrap();
    assert_eq!(get(&b, &t).at, p(15_000, 18_000));
    // a quarter turn clockwise about the table's own corner: the table's angle runs the other way (KiCad's), so it reads 270 degrees
    b.apply(&Cmd::RotateItems { ids: vec![t.clone()], pivot: get(&b, &t).at, angle_millideg: 90_000 }).unwrap();
    assert_eq!(get(&b, &t).angle, 270_000);
    b.apply(&Cmd::RotateItems { ids: vec![t.clone()], pivot: get(&b, &t).at, angle_millideg: -90_000 }).unwrap();
    assert_eq!(get(&b, &t).angle, 0);
    // flipped to the back: the layer, the columns and the text's mirror follow
    let bx = get(&b, &t).bounding_box();
    b.apply(&Cmd::FlipItems { ids: vec![t.clone()], pivot: p(0, 0), direction: FlipDirection::LeftRight }).unwrap();
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
    // deleting one of them takes it out of the group, which then has too few members to stay
    b.apply(&Cmd::Table(TableCmd::Delete { id: other })).unwrap();
    assert!(b.design().drawings.as_ref().unwrap().groups.is_empty());
    let _ = before;
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
    let edit: Cmd = serde_json::from_value(serde_json::json!({ "op": "table", "verb": "edit_cells", "id": "tbl_x", "cells": [[0, 1], [1, 1]], "edit": { "text": "x", "margins": [null, 500, null, null] } })).unwrap();
    assert!(matches!(&edit, Cmd::Table(TableCmd::EditCells { cells, edit, .. }) if cells == &vec![(0, 1), (1, 1)] && edit.margins[1] == Some(500)));
}
