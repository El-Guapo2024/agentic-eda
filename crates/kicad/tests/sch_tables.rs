//! A sheet's tables in the `.kicad_sch` the studio writes and reads (package P16t): `(table ..)` with its `(table_cell ..)`s, as `SCH_IO_KICAD_SEXPR::saveTable` writes
//! it and `parseSchTable` reads it -- the cells' text and looks, the grid, a lock, and a group that names a table by its uuid.
//!
//! The kicad-cli side (a table we write is plotted as KiCad's own, and the table of KiCad's own QA sheet comes back the same) is
//! `crates/kicad-engine/tests/tables_sch.rs`.

use eda_kicad::{export_kicad_sch_mapped, import_kicad_sch, ExportMeta};
use eda_model::ir::{Design, Group, Point, Provenance, SchematicSection, SchematicText, TextJustify, TextVAlign};
use eda_model::table::Table;
use eda_model::ConstraintModel;

const META: ExportMeta<'static> = ExportMeta { date: "2026-10-21", title: "tables" };

fn design_with(sch: SchematicSection) -> Design {
    Design { schema: eda_model::ir::SCHEMA_VERSION, provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] }, schematic: Some(sch), nets: None, placement: None, routing: None, drawings: None, footprint_library: None, sheet_contents: None, bus_aliases: Default::default(), symbol_library: None }
}

/// A 3 x 2 table at (100, 50) mm of 30 x 6 mm cells: a header over two rows, one of the cells merged across, one right-aligned and bold, one with a fill.
fn table() -> Table {
    let mut t = Table::new_sch_grid(Point { x: 100_000, y: 50_000 }, 2, 3, 30_000, 6_000, 1_270);
    t.id = "tbl_given".into();
    for (i, text) in ["Name", "Value", "R1", "10k \"tol\"", "Notes\nsecond line", ""].iter().enumerate() {
        t.cells[i].text = text.to_string();
    }
    t.cells[4].col_span = 2;
    t.cells[5].col_span = 0;
    t.cells[5].row_span = 1;
    t.cells[3].style.h_align = TextJustify::Right;
    t.cells[3].style.bold = true;
    t.cells[1].style.v_align = TextVAlign::Center;
    t.cells[1].fill = eda_model::sch_extras::SchFill::Color;
    t.cells[1].fill_color = Some(eda_model::sch_extras::SchColor { r: 255, g: 240, b: 200, a: 255 });
    t.cells[2].exclude_from_sim = true;
    t.border.stroke.width_nm = 254;
    t.separators.cols = false;
    t
}

fn sheet() -> SchematicSection {
    let mut sch = SchematicSection::default();
    sch.texts.push(SchematicText { id: "txt_note".into(), content: "note".into(), at: Point { x: 10_000, y: 10_000 }, angle: 0, size_nm: 1_270 });
    sch.extras.tables.push(table());
    sch.extras.groups.push(Group { id: "grp_t".into(), name: "tab".into(), member_ids: vec!["tbl_given".into(), "txt_note".into()], design_block: String::new() });
    sch.extras.set_locked("tbl_given", true);
    sch
}

#[test]
fn a_table_is_written_into_the_sheet_and_read_back_with_its_cells_its_lock_and_its_group() {
    let m = ConstraintModel::default();
    let (file, uuids) = export_kicad_sch_mapped(&design_with(sheet()), &m, &META).expect("the sheet exports");
    assert!(file.contains("(table\n") && file.contains("(column_count 2)") && file.contains("(table_cell \"Name\""), "{file}");
    assert!(file.contains("(span 2 1)") && file.contains("(span 0 1)"), "the merged cell and the one it covers");
    assert!(file.contains("(locked yes)"), "the table's lock");
    // the group names the table by its uuid, which maps back to the table
    let table_uuid = uuids.iter().find(|(_, id)| id.as_str() == "tbl_given").map(|(u, _)| u.clone()).expect("the table's uuid is mapped");
    let group_at = file.find("(group \"tab\"").expect("the group is written");
    assert!(file[group_at..].contains(&table_uuid), "the group's members include {table_uuid}");

    let (design, _, _) = import_kicad_sch(&file).expect("the file reads back");
    let back = design.schematic.as_ref().unwrap();
    assert_eq!(back.extras.tables.len(), 1);
    let t = &back.extras.tables[0];
    let want = table();
    assert_eq!((t.col_count(), t.row_count(), t.at, t.angle), (2, 3, want.at, 0));
    assert_eq!((t.col_widths.clone(), t.row_heights.clone()), (want.col_widths.clone(), want.row_heights.clone()));
    assert_eq!(t.texts(), want.texts());
    assert_eq!((t.cells[4].col_span, t.cells[5].col_span, t.cells[5].row_span), (2, 0, 1));
    assert_eq!((t.cells[3].style.h_align, t.cells[3].style.bold, t.cells[1].style.v_align), (TextJustify::Right, true, TextVAlign::Center));
    assert_eq!((t.cells[1].fill, t.cells[1].fill_color), (want.cells[1].fill, want.cells[1].fill_color));
    assert!(t.cells[2].exclude_from_sim && !t.cells[0].exclude_from_sim);
    assert_eq!((t.border.stroke.width_nm, t.separators.cols, t.separators.rows), (254, false, true));
    assert!(!t.id.is_empty(), "an id is given to the table it read");
    assert!(back.extras.is_locked(&t.id), "its lock");
    let group = back.extras.groups.iter().find(|g| g.name == "tab").expect("its group");
    assert!(group.member_ids.contains(&t.id), "the group still holds the table: {:?}", group.member_ids);
    // and writing it again changes nothing
    let again = eda_kicad::export_kicad_sch(&design, &m, &META).unwrap();
    assert_eq!(again.matches("(table_cell").count(), 6);
    assert!(again.contains("(column_widths 30 30)") && again.contains("(row_heights 6 6 6)"));
}

#[test]
fn a_sheet_with_no_table_writes_none() {
    let mut sch = sheet();
    sch.extras.tables.clear();
    sch.extras.groups.clear();
    let file = eda_kicad::export_kicad_sch(&design_with(sch), &ConstraintModel::default(), &META).unwrap();
    assert!(!file.contains("(table"));
}
