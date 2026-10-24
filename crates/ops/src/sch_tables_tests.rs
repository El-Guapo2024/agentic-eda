//! A sheet's tables: the table verbs through `Cmd::SchTable` (the same edits the board's tables get), and what the sheet's own verbs do with a table -- move, turn, mirror,
//! lock, group and delete it.

use super::*;
use crate::sch_move::SchMoveCmd;
use eda_model::ir::{Provenance, SchematicText};
use eda_model::table::Table;
use eda_model::table_edit::{CellEdit, TablePatch};

fn p(x: Nm, y: Nm) -> Point {
    Point { x, y }
}

fn design() -> Design {
    Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: Some(eda_model::ir::SchematicSection { imported_from_kicad: true, ..Default::default() }),
        nets: None,
        routing: None,
        placement: None,
        drawings: None,
    }
}

fn table(rows: usize, cols: usize) -> Table {
    let mut t = Table::new_sch_grid(p(20_000_000, 30_000_000), cols, rows, 20_000_000, 5_000_000, 1_270_000);
    for (i, c) in t.cells.iter_mut().enumerate() {
        c.text = format!("r{}c{}", i / cols, i % cols);
    }
    t
}

fn on_sheet<R>(b: &Board<'_>, f: impl FnOnce(&eda_model::ir::SchematicSection) -> R) -> R {
    f(b.design().schematic.as_ref().unwrap())
}

fn add(b: &mut Board<'_>, t: Table) -> String {
    b.apply(&Cmd::SchTable(TableCmd::Add { table: t })).expect("adds");
    on_sheet(b, |s| s.extras.tables.last().unwrap().id.clone())
}

#[test]
fn a_table_drawn_on_a_sheet_is_a_grid_of_sheet_cells_with_what_its_dialog_set() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, 100_000, 300_000);
    let patch = TablePatch { texts: Some(vec![vec!["a".into(), "b".into()]]), ..TablePatch::default() };
    b.apply(&Cmd::SchTable(TableCmd::Draw { layer: String::new(), at: p(10_160_000, 20_320_000), cols: 2, rows: 1, cell_w_nm: 25_400_000, cell_h_nm: 5_080_000, line_width_nm: 0, locked: true, patch })).unwrap();
    let (t, locked) = on_sheet(&b, |s| (s.extras.tables[0].clone(), s.extras.is_locked(&s.extras.tables[0].id)));
    assert!(t.id.starts_with("tbl_"), "{}", t.id);
    assert_eq!((t.col_count(), t.row_count(), t.at, t.layer.as_str()), (2, 1, p(10_160_000, 20_320_000), ""));
    assert_eq!(t.texts(), vec![vec!["a", "b"]]);
    // `SCH_TABLE( 0 )` and `SCH_TABLECELL`: a stroke of the default width, left and top 50 mil text, the margin of the constructor
    assert_eq!((t.border.stroke.width_nm, t.separators.stroke.width_nm), (0, 0));
    let c = t.cell(0, 0).unwrap();
    assert_eq!((c.style.height_nm, c.style.h_align, c.style.v_align, c.margins), (1_270_000, eda_model::ir::TextJustify::Left, eda_model::ir::TextVAlign::Top, [952_500; 4]));
    assert!(locked);
    assert!(b.design().drawings.is_none(), "nothing went to the board");
    // refused: no size, the board's tables only
    let bad = Cmd::SchTable(TableCmd::Draw { layer: String::new(), at: p(0, 0), cols: 0, rows: 1, cell_w_nm: 1, cell_h_nm: 1, line_width_nm: 0, locked: false, patch: TablePatch::default() });
    assert!(b.apply(&bad).is_err());
    let place = Cmd::SchTable(TableCmd::PlaceStackup { layer: "F.SilkS".into(), at: p(0, 0), units: None });
    assert!(b.apply(&place).is_err(), "a sheet has no stackup");
}

#[test]
fn the_structure_edits_and_the_dialogs_work_on_a_sheets_table_as_on_a_boards() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, 100_000, 300_000);
    let id = add(&mut b, table(2, 2));
    let get = |b: &Board<'_>| on_sheet(b, |s| s.extras.tables[0].clone());
    b.apply(&Cmd::SchTable(TableCmd::AddRow { id: id.clone(), row: 1, above: false })).unwrap();
    b.apply(&Cmd::SchTable(TableCmd::AddColumn { id: id.clone(), col: 0, before: true })).unwrap();
    assert_eq!((get(&b).row_count(), get(&b).col_count()), (3, 3));
    b.apply(&Cmd::SchTable(TableCmd::DeleteColumns { id: id.clone(), cols: vec![0] })).unwrap();
    b.apply(&Cmd::SchTable(TableCmd::MergeCells { id: id.clone(), cells: vec![(0, 0), (0, 1)] })).unwrap();
    let t = get(&b);
    assert_eq!((t.cell(0, 0).unwrap().col_span, t.cell(0, 1).unwrap().col_span), (2, 0));
    b.apply(&Cmd::SchTable(TableCmd::UnmergeCells { id: id.clone(), cells: vec![(0, 0)] })).unwrap();
    let edit = CellEdit { text: Some("hello".into()), bold: Some(true), margins: [Some(500_000), None, None, None], ..CellEdit::default() };
    b.apply(&Cmd::SchTable(TableCmd::EditCells { id: id.clone(), cells: vec![(2, 1)], edit })).unwrap();
    let c = get(&b).cell(2, 1).unwrap().clone();
    assert_eq!((c.text.as_str(), c.style.bold, c.margins[0]), ("hello", true, 500_000));
    // Table Properties: the border and the lock; a layer is no property of a sheet's table
    let patch = TablePatch { border: Some(eda_model::table_edit::BorderEdit { external: false, header: false, width_nm: 300_000, style: Default::default() }), ..TablePatch::default() };
    b.apply(&Cmd::SchTable(TableCmd::Set { id: id.clone(), layer: None, locked: Some(true), patch })).unwrap();
    assert_eq!(get(&b).border.stroke.width_nm, -1, "no border is a stroke that is not drawn");
    assert!(on_sheet(&b, |s| s.extras.is_locked(&id)));
    assert!(b.apply(&Cmd::SchTable(TableCmd::Set { id: id.clone(), layer: Some("F.SilkS".into()), locked: None, patch: TablePatch::default() })).is_err());
    // deleting every row deletes the table, and the lock goes with it
    b.apply(&Cmd::SchTable(TableCmd::DeleteRows { id: id.clone(), rows: vec![0, 1, 2] })).unwrap();
    assert!(on_sheet(&b, |s| s.extras.tables.is_empty() && !s.extras.is_locked(&id)));
    // an unknown table is refused
    assert!(b.apply(&Cmd::SchTable(TableCmd::Delete { id })).is_err());
}

#[test]
fn a_table_moves_with_the_sheets_move_verb_and_turns_between_its_two_attitudes() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, 100_000, 300_000);
    let id = add(&mut b, table(2, 2));
    b.apply(&Cmd::SchMove(SchMoveCmd::Move { ids: vec![id.clone()], dx: 2_540_000, dy: -1_270_000, turns: vec![], about: None })).unwrap();
    assert_eq!(on_sheet(&b, |s| s.extras.tables[0].at), p(22_540_000, 28_730_000));
    // R turns it a quarter about the half-grid point nearest the centre of its box (`SCH_EDIT_TOOL::Rotate`: it is not connectable): the box is 40 x 10 mm from (22.54,
    // 28.73), so the centre is (42.54, 33.73) and the half grid (0.635 mm) puts the pivot at (42.545, 33.655); the first cell goes with the turn and the table stands
    b.apply(&Cmd::SchMove(SchMoveCmd::Rotate { ids: vec![id.clone()], vertices: Default::default(), ccw: true, about: None, grid: 0 })).unwrap();
    let t = on_sheet(&b, |s| s.extras.tables[0].clone());
    assert_eq!((t.angle, t.at), (90_000, p(37_620_000, 53_660_000)));
    // the second quarter lays it down again (the two attitudes of `SCH_TABLE::Rotate`)
    b.apply(&Cmd::SchMove(SchMoveCmd::Rotate { ids: vec![id.clone()], vertices: Default::default(), ccw: true, about: None, grid: 0 })).unwrap();
    assert_eq!(on_sheet(&b, |s| s.extras.tables[0].angle), 0);
    // X and Y do nothing to a table (`SCH_TABLE::MirrorHorizontally` is empty), but the verb is not refused
    let before = on_sheet(&b, |s| s.extras.tables[0].clone());
    b.apply(&Cmd::SchMove(SchMoveCmd::Mirror { ids: vec![id.clone()], vertices: Default::default(), vertical: false, about: None, grid: 0 })).unwrap();
    assert_eq!(on_sheet(&b, |s| s.extras.tables[0].clone()), before);
}

#[test]
fn a_selected_cell_stands_for_its_table_in_move_and_rotate() {
    // `RequestSelection( .., aPromoteCellSelections = true )` in `SCH_MOVE_TOOL::Main` and `SCH_EDIT_TOOL::Rotate`: two cells of one table move the table once
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, 100_000, 300_000);
    let id = add(&mut b, table(2, 2));
    let cells = vec![eda_model::table::cell_item_id(&id, 0, 1), eda_model::table::cell_item_id(&id, 1, 1)];
    b.apply(&Cmd::SchMove(SchMoveCmd::Move { ids: cells.clone(), dx: 2_540_000, dy: 0, turns: vec![], about: None })).unwrap();
    assert_eq!(on_sheet(&b, |s| s.extras.tables[0].at), p(22_540_000, 30_000_000), "moved once, not once per cell");
    b.apply(&Cmd::SchMove(SchMoveCmd::Rotate { ids: cells, vertices: Default::default(), ccw: true, about: None, grid: 0 })).unwrap();
    assert_eq!(on_sheet(&b, |s| s.extras.tables[0].angle), 90_000, "a quarter turn, not two");
    // a cell id of a table the sheet does not have is no item
    let missing = eda_model::table::cell_item_id("tbl_zz", 0, 0);
    assert!(b.apply(&Cmd::SchMove(SchMoveCmd::Move { ids: vec![missing], dx: 1_270_000, dy: 0, turns: vec![], about: None })).is_err());
}

#[test]
fn a_locked_table_does_not_move_and_a_table_can_be_locked_and_grouped_like_any_item() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, 100_000, 300_000);
    let id = add(&mut b, table(1, 2));
    b.schematic_mut().unwrap().texts.push(SchematicText { id: "txt_a".into(), content: "a".into(), at: p(0, 0), angle: 0, size_nm: 1_270_000 });
    b.apply(&Cmd::SchEdit(sch_edit::SchCmd::SetLocked { ids: vec![id.clone()], locked: true })).unwrap();
    assert!(on_sheet(&b, |s| s.extras.is_locked(&id)));
    assert!(b.apply(&Cmd::SchMove(SchMoveCmd::Move { ids: vec![id.clone()], dx: 1_000_000, dy: 0, turns: vec![], about: None })).is_err(), "a locked item is left out, and nothing is left");
    b.apply(&Cmd::SchEdit(sch_edit::SchCmd::SetLocked { ids: vec![id.clone()], locked: false })).unwrap();
    // grouped with a text, and the group moves the table too
    b.apply(&Cmd::SchEdit(sch_edit::SchCmd::Group { ids: vec![id.clone(), "txt_a".into()] })).unwrap();
    let group = on_sheet(&b, |s| s.extras.groups[0].id.clone());
    b.apply(&Cmd::SchMove(SchMoveCmd::Move { ids: vec![group], dx: 5_000_000, dy: 0, turns: vec![], about: None })).unwrap();
    assert_eq!(on_sheet(&b, |s| (s.extras.tables[0].at, s.texts[0].at)), (p(25_000_000, 30_000_000), p(5_000_000, 0)));
    // deleting the table takes it out of the group, which keeps the text (`SCH_COMMIT::Push` dissolves nothing)
    b.apply(&Cmd::SchTable(TableCmd::Delete { id })).unwrap();
    assert_eq!(on_sheet(&b, |s| s.extras.groups.iter().map(|g| g.member_ids.clone()).collect::<Vec<_>>()), vec![vec!["txt_a".to_string()]]);
}

#[test]
fn the_sch_table_verb_reads_from_and_writes_to_the_json_the_studio_sends() {
    let cmd: Cmd = serde_json::from_value(serde_json::json!({ "op": "sch_table", "verb": "add_row", "id": "tbl_x", "row": 0, "above": true })).unwrap();
    assert_eq!(cmd, Cmd::SchTable(TableCmd::AddRow { id: "tbl_x".into(), row: 0, above: true }));
    assert_eq!(serde_json::to_value(&cmd).unwrap(), serde_json::json!({ "op": "sch_table", "verb": "add_row", "id": "tbl_x", "row": 0, "above": true }));
    assert_eq!(cmd.domain(), Domain::Schematic);
    assert!(!cmd.edits_connectivity());
}

#[test]
fn a_sheet_cells_background_is_set_by_the_cell_edit_the_panels_background_rows_send() {
    // `SCH_TABLECELL_DESC`'s "Background Fill" (EDA_SHAPE::SetFillMode) and "Background Fill Color" (SetFillColor), over the wire the studio uses
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, 100_000, 300_000);
    let id = add(&mut b, table(1, 2));
    let cmd: Cmd = serde_json::from_value(serde_json::json!({
        "op": "sch_table", "verb": "edit_cells", "id": id, "cells": [[0, 1]], "edit": { "fill": "color", "fill_color": { "r": 255, "g": 240, "b": 200, "a": 255 } }
    }))
    .unwrap();
    b.apply(&cmd).unwrap();
    let (a, c) = on_sheet(&b, |s| (s.extras.tables[0].cells[0].clone(), s.extras.tables[0].cells[1].clone()));
    assert_eq!((a.fill, a.fill_color), (eda_model::sch_extras::SchFill::None, None), "the other cell is left alone");
    assert_eq!((c.fill, c.fill_color), (eda_model::sch_extras::SchFill::Color, Some(eda_model::sch_extras::SchColor { r: 255, g: 240, b: 200, a: 255 })));
    // and the background goes again
    b.apply(&Cmd::SchTable(TableCmd::EditCells { id, cells: vec![(0, 1)], edit: CellEdit { fill: Some(eda_model::sch_extras::SchFill::None), ..Default::default() } })).unwrap();
    assert_eq!(on_sheet(&b, |s| s.extras.tables[0].cells[1].fill), eda_model::sch_extras::SchFill::None);
}
