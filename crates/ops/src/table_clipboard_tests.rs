//! Tables on the clipboard, in both editors: Copy and Paste of a table and of cells (`CLIPBOARD_IO::SaveSelection`, `SCH_IO_KICAD_SEXPR::Format( SCH_SELECTION* )`,
//! `PCB_CONTROL::Paste`, `SCH_EDITOR_CONTROL::Paste`), Paste Cells (`pasteCellsIntoSelection`), and the sheet's cross-references following a rename.

use super::pcb_transform_tests::{board, model, p};
use super::*;
use eda_model::ir::{Provenance, SymbolInstance};
use eda_model::table::{cell_item_id, Table, TableBorder, TableCell, TableSeparators, TableStroke};
use eda_model::units::mm;

/// A `rows` x `cols` board table of 10 x 5 mm cells at (10, 20) mm on `layer`, each cell's text its row and column (`r0c1`).
fn pcb_table(rows: usize, cols: usize, layer: &str) -> Table {
    let mut cells: Vec<TableCell> = (0..rows * cols).map(|_| TableCell::pcb_new(false)).collect();
    for (i, c) in cells.iter_mut().enumerate() {
        c.text = format!("r{}c{}", i / cols, i % cols);
    }
    Table {
        id: String::new(),
        uuid: String::new(),
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

fn add_pcb(b: &mut Board<'_>, t: Table) -> String {
    b.apply(&Cmd::Table(TableCmd::Add { table: t })).expect("adds");
    b.design().drawings.as_ref().unwrap().tables.last().unwrap().id.clone()
}

fn pcb_tables(b: &Board<'_>) -> Vec<Table> {
    b.design().drawings.as_ref().map(|d| d.tables.clone()).unwrap_or_default()
}

#[test]
fn a_board_table_copied_and_pasted_is_a_second_table_where_the_cursor_put_it() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let t = add_pcb(&mut b, pcb_table(2, 3, "F.SilkS"));
    // copied from its corner, pasted 30 mm to the right
    let text = eda_kicad::export_pcb_clipboard(b.design(), &m, &[t.clone()], p(mm(10.0), mm(20.0))).unwrap();
    b.apply(&Cmd::PasteClipboard { text: text.clone(), at: p(mm(40.0), mm(20.0)) }).unwrap();
    let all = pcb_tables(&b);
    assert_eq!(all.len(), 2);
    let copy = all.iter().find(|x| x.id != t).unwrap();
    assert_eq!((copy.at, copy.texts()), (p(mm(40.0), mm(20.0)), all[0].texts()));
    assert!(copy.uuid.is_empty() && copy.cells.iter().all(|c| c.uuid.is_empty()), "a pasted table has uuids of its own");
    // the cells copied without their table: a table of the block they make, at the first one's corner
    let cells = vec![cell_item_id(&t, 0, 1), cell_item_id(&t, 0, 2), cell_item_id(&t, 1, 1), cell_item_id(&t, 1, 2)];
    let text = eda_kicad::export_pcb_clipboard(b.design(), &m, &cells, p(Nm::ZERO, Nm::ZERO)).unwrap();
    b.apply(&Cmd::PasteClipboard { text, at: p(mm(0.0), mm(50.0)) }).unwrap();
    let block = pcb_tables(&b).into_iter().find(|x| x.at == p(mm(20.0), mm(70.0))).expect("the block lands at the first cell's corner");
    assert_eq!(block.texts(), vec![vec!["r0c1", "r0c2"], vec!["r1c1", "r1c2"]]);
}

#[test]
fn a_group_holding_a_table_is_pasted_with_the_table_in_it() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let t = add_pcb(&mut b, pcb_table(1, 2, "F.SilkS"));
    let u = add_pcb(&mut b, pcb_table(1, 1, "F.SilkS"));
    b.apply(&Cmd::Group { ids: vec![t.clone(), u.clone()] }).unwrap();
    let g = b.design().drawings.as_ref().unwrap().groups[0].id.clone();
    let text = eda_kicad::export_pcb_clipboard(b.design(), &m, &[g], p(Nm::ZERO, Nm::ZERO)).unwrap();
    b.apply(&Cmd::PasteClipboard { text, at: p(mm(0.0), mm(30.0)) }).unwrap();
    let dr = b.design().drawings.as_ref().unwrap();
    assert_eq!(dr.tables.len(), 4);
    assert_eq!(dr.groups.len(), 2);
    let pasted = dr.groups.iter().find(|x| !x.member_ids.contains(&t)).unwrap();
    assert_eq!(pasted.member_ids.len(), 2);
    assert!(pasted.member_ids.iter().all(|m| dr.tables.iter().any(|x| &x.id == m && x.id != t && x.id != u)), "{pasted:?}");
}

#[test]
fn paste_cells_puts_the_copied_table_into_the_selected_cells_and_refuses_what_kicad_refuses() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let target = add_pcb(&mut b, pcb_table(3, 3, "F.SilkS"));
    let mut source = pcb_table(2, 2, "F.SilkS");
    for c in source.cells.iter_mut() {
        c.text = format!("new {}", c.text);
        c.style.bold = true;
    }
    let src = add_pcb(&mut b, source);
    let text = eda_kicad::export_pcb_clipboard(b.design(), &m, &[src.clone()], p(Nm::ZERO, Nm::ZERO)).unwrap();
    let paste = |ids: Vec<String>| Cmd::Table(TableCmd::PasteCells { ids, text: text.clone() });
    // the second row's second cell: the 2 x 2 source lands on (1, 1) .. (2, 2)
    b.apply(&paste(vec![cell_item_id(&target, 1, 1)])).unwrap();
    let t = pcb_tables(&b).into_iter().find(|x| x.id == target).unwrap();
    assert_eq!(t.texts(), vec![vec!["r0c0", "r0c1", "r0c2"], vec!["r1c0", "new r0c0", "new r0c1"], vec!["r2c0", "new r1c0", "new r1c1"]]);
    assert!(t.cell(2, 2).unwrap().style.bold && !t.cell(0, 0).unwrap().style.bold);
    // past the last row and column the table grows
    b.apply(&paste(vec![cell_item_id(&target, 2, 2)])).unwrap();
    let t = pcb_tables(&b).into_iter().find(|x| x.id == target).unwrap();
    assert_eq!((t.row_count(), t.col_count()), (4, 4));
    // KiCad's words for a selection it refuses
    let hint = |r: Result<(), Vec<CheckResult>>| r.unwrap_err()[0].hint.clone().unwrap_or_default();
    assert_eq!(hint(b.apply(&paste(vec![cell_item_id(&target, 0, 0), cell_item_id(&target, 1, 1)]))), "Selected cells must form a contiguous block");
    assert_eq!(hint(b.apply(&paste(vec![cell_item_id(&target, 0, 0), src.clone()]))), "Selection contains non-cell items");
    assert_eq!(hint(b.apply(&paste(vec![cell_item_id(&target, 0, 0), cell_item_id(&src, 0, 0)]))), "Selected cells are from different tables");
    // a clipboard with no table is no paste into cells (the editor pastes it as it is)
    let no_table = Cmd::Table(TableCmd::PasteCells { ids: vec![cell_item_id(&target, 0, 0)], text: "(kicad_pcb (version 20241229) (generator \"x\") (gr_text \"a\" (at 0 0) (layer \"F.SilkS\") (effects (font (size 1 1)))))".into() });
    assert_eq!(b.apply(&no_table).unwrap_err()[0].check, "ops_no_clipboard_table");
    // on the wire
    let wire: Cmd = serde_json::from_value(serde_json::json!({ "op": "table", "verb": "paste_cells", "ids": ["tbl_x:0:0"], "text": "(kicad_pcb)" })).unwrap();
    assert!(matches!(wire, Cmd::Table(TableCmd::PasteCells { .. })));
}

#[test]
fn a_board_table_flipped_top_to_bottom_is_turned_half_round_as_kicad_flips_it() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let t = add_pcb(&mut b, pcb_table(2, 3, "F.SilkS"));
    let before = pcb_tables(&b)[0].bounding_box();
    b.apply(&Cmd::FlipItems { ids: vec![t.clone()], pivot: p(Nm::ZERO, mm(40.0)), direction: FlipDirection::TopBottom }).unwrap();
    let after = pcb_tables(&b).into_iter().find(|x| x.id == t).unwrap();
    assert_eq!((after.layer.as_str(), after.angle), ("B.SilkS", 180_000));
    let bb = after.bounding_box();
    assert_eq!((bb.x0, bb.y0, bb.x1, bb.y1), (before.x0, mm(80.0) - before.y1, before.x1, mm(80.0) - before.y0), "the mirror image's box, and nothing beside it");
    assert!(after.cells.iter().all(|c| c.style.mirror));
}

// ------------------------------------------------------------------------------------------------------------------------------------ the sheet

fn sch_design() -> Design {
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

fn sch_table(rows: usize, cols: usize) -> Table {
    let mut t = Table::new_sch_grid(p(mm(20.32), mm(30.48)), cols, rows, mm(20.32), mm(5.08), mm(1.27));
    for (i, c) in t.cells.iter_mut().enumerate() {
        c.text = format!("r{}c{}", i / cols, i % cols);
    }
    t
}

fn add_sch(b: &mut Board<'_>, t: Table) -> String {
    b.apply(&Cmd::SchTable(TableCmd::Add { table: t })).expect("adds");
    b.design().schematic.as_ref().unwrap().extras.tables.last().unwrap().id.clone()
}

fn sch_tables(b: &Board<'_>) -> Vec<Table> {
    b.design().schematic.as_ref().unwrap().extras.tables.clone()
}

fn sch_copy(b: &Board<'_>, m: &ConstraintModel, ids: &[String]) -> String {
    let sch = b.design().schematic.as_ref().unwrap();
    eda_kicad::write_clipboard(&eda_kicad::CopyInput { design: b.design(), model: m, section: sch, project: "p", sheet_path: &[] }, ids).unwrap().text
}

#[test]
fn a_sheet_table_and_its_cells_are_copied_and_pasted_as_kicads_sheet_does() {
    let m = ConstraintModel::default();
    let mut b = Board::new(sch_design(), &m, mm(0.1), mm(0.3));
    let t = add_sch(&mut b, sch_table(2, 3));
    b.apply(&Cmd::SchTable(TableCmd::Set { id: t.clone(), layer: None, locked: Some(true), patch: Default::default() })).unwrap();
    // the table: one `(table ..)`, which the paste reads back and adds, unlocked, moved by the cursor's shift
    let text = sch_copy(&b, &m, &[t.clone()]);
    assert_eq!(text.matches("(table\n").count(), 1, "{text}");
    let fragment = eda_kicad::parse_clipboard(&text).unwrap();
    assert!(fragment.notes.is_empty(), "{:?}", fragment.notes);
    assert_eq!(fragment.section.extras.tables.len(), 1);
    b.apply(&Cmd::PasteSch { fragment, dx: mm(50.8), dy: Nm::ZERO, mode: Default::default(), numbering: Default::default() }).unwrap();
    let all = sch_tables(&b);
    assert_eq!(all.len(), 2);
    let copy = all.iter().find(|x| x.id != t).unwrap();
    assert_eq!((copy.at, copy.texts()), (p(mm(20.32 + 50.8), mm(30.48)), all[0].texts()));
    assert!(!b.design().schematic.as_ref().unwrap().extras.is_locked(&copy.id), "a paste is never locked");
    // two cells side by side: `saveTable` of the table flagged `SKIP_STRUCT`, a table of those two
    let text = sch_copy(&b, &m, &[cell_item_id(&t, 1, 1), cell_item_id(&t, 1, 2)]);
    let fragment = eda_kicad::parse_clipboard(&text).unwrap();
    assert_eq!(fragment.section.extras.tables[0].texts(), vec![vec!["r1c1", "r1c2"]]);
    // and pasted into the first row's first cells of the copy
    b.apply(&Cmd::SchTable(TableCmd::PasteCells { ids: vec![cell_item_id(&copy.id, 0, 0)], text })).unwrap();
    let copy = sch_tables(&b).into_iter().find(|x| x.id == copy.id).unwrap();
    assert_eq!(copy.texts()[0], vec!["r1c1", "r1c2", "r0c2"]);
}

#[test]
fn a_cross_reference_in_a_sheet_cell_follows_its_symbol_through_a_rename() {
    let m = ConstraintModel::default();
    let mut design = sch_design();
    let sym = |id: &str, x: f64| SymbolInstance { id: id.into(), at: p(mm(x), mm(10.16)), rot: 0, mirrored: false, mirror_y: false, lib_id: "Device:R".into(), unit: 1, value: "10k".into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false };
    let sch = design.schematic.as_mut().unwrap();
    sch.symbols = vec![sym("R1", 10.16), sym("R2", 30.48)];
    sch.texts.push(eda_model::ir::SchematicText { id: "txt_a".into(), content: "see ${R1:VALUE}".into(), at: p(mm(5.08), mm(5.08)), angle: 0, size_nm: mm(1.27) });
    let mut b = Board::new(design, &m, mm(0.1), mm(0.3));
    let mut t = sch_table(1, 2);
    t.cells[0].text = "${R1:VALUE}".into();
    t.cells[1].text = "${R2:VALUE} and ${R10:VALUE}".into();
    let tid = add_sch(&mut b, t);
    b.apply(&Cmd::RenameSymbol { id: "R1".into(), new_id: "R5".into() }).unwrap();
    let sch = b.design().schematic.as_ref().unwrap();
    let t = sch.extras.tables.iter().find(|x| x.id == tid).unwrap();
    assert_eq!(t.texts(), vec![vec!["${R5:VALUE}", "${R2:VALUE} and ${R10:VALUE}"]]);
    assert_eq!(sch.texts[0].content, "see ${R5:VALUE}", "a text follows too");
}
