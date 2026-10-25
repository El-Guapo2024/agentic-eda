//! A sheet's tables (`(table ..)`, KiCad's `SCH_TABLE` with its `SCH_TABLECELL`s) read and written: `SCH_IO_KICAD_SEXPR_PARSER::parseSchTable`, `parseSchTableCell`
//! and `parseSchTextBoxContent`, and `SCH_IO_KICAD_SEXPR::saveTable` with `saveTextBox`, for [`eda_model::table::Table`].
//!
//! Each cell of the file has its own place, `(at x y angle)` and `(size w h)`: the table's origin is where cell (0, 0) is, the angle is the cells' text angle (a table
//! that stands has the grid turned a quarter about the origin, as `SCH_TABLE::Normalize` lays it out), and the sizes of the columns and the rows are the lists the file
//! keeps. The model keeps the grid and works the cells out; a cell the file puts somewhere else than the grid does is laid out by the grid, as KiCad's own `Normalize`
//! does at the first edit.
//!
//! What the file does not say a sheet takes from KiCad's constructors: a stroke that is not written is the default width, a cell with no `(margins ..)` the legacy
//! margin, a cell with no `(effects ..)` 50 mil text, left and top.

use std::fmt::Write as _;

use eda_model::ir::{Point, SchematicSection, Nm};
use eda_model::sch_extras::{SchColor, SchFill};
use eda_model::table::{Table, TableBorder, TableCell, TableLineStyle, TableSeparators, TableStroke, TableText};

use crate::pcb_table::{effects_str, read_color, read_effects, read_stroke, stroke_str};
use crate::sexpr::{self, Sexpr};
use crate::{duid_for, fmt_mm_f, sch_mm, sch_mm_to_nm, note_uuid, sexpr_str};

/// The width of a stroke the file does not write: `DEFAULT_LINE_WIDTH_MILS` (6 mils), which the constructor of the table gives it.
const DEFAULT_STROKE_NM: Nm = Nm::from_raw_shim(152_400);

/// A table read from a sheet, with what a `(group ..)` and a lock need to know of it.
pub(crate) struct ReadTable {
    pub table: Table,
    pub locked: bool,
    /// The table's `(uuid ..)`, which a `(group ..)` names it by.
    pub uuid: Option<String>,
}

fn um(v: f64) -> Nm {
    sch_mm_to_nm(v)
}

fn yes(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

/// A flag as the file writes it: `(name yes)`, `(name no)`, `(name)` (yes) or the bare word in the item itself.
fn flag(item: &[Sexpr], name: &str) -> Option<bool> {
    if let Some(l) = sexpr::find(item, name) {
        return Some(sexpr::txt(l, 1).is_none_or(|v| v != "no"));
    }
    item.iter().skip(1).any(|c| matches!(c, Sexpr::Atom(a) if a == name)).then_some(true)
}

/// Where one cell is in the file: its start and its end, and the angle of its text.
struct FileCell {
    start: Point,
    end: Point,
    angle: u32,
}

/// One `(table_cell ..)`: the cell and where the file put it.
fn read_cell(item: &[Sexpr]) -> Option<(TableCell, FileCell)> {
    let text = item.get(1).and_then(Sexpr::text)?.to_string();
    let (start, angle_deg) = match sexpr::find(item, "at") {
        Some(a) => (Point { x: um(sexpr::num(a, 1)?), y: um(sexpr::num(a, 2)?) }, sexpr::num(a, 3).unwrap_or(0.0)),
        // The legacy `(start ..)`, which carries no angle.
        None => {
            let s = sexpr::find(item, "start")?;
            (Point { x: um(sexpr::num(s, 1)?), y: um(sexpr::num(s, 2)?) }, 0.0)
        }
    };
    let end = if let Some(e) = sexpr::find(item, "end") {
        Point { x: um(sexpr::num(e, 1)?), y: um(sexpr::num(e, 2)?) }
    } else {
        let s = sexpr::find(item, "size")?;
        Point { x: start.x + um(sexpr::num(s, 1)?), y: start.y + um(sexpr::num(s, 2)?) }
    };
    let angle = ((angle_deg * 1000.0).round() as i64).rem_euclid(360_000) as u32;
    // The text of a text box is left and top until `(effects ..)` says otherwise; `(effects ..)` with no `(justify ..)` is centred both ways.
    let style = if sexpr::find(item, "effects").is_some() { read_effects(item) } else { TableText::sch_default(Nm::from_raw_shim(1_270_000)) };
    let margins = match sexpr::find(item, "margins") {
        Some(m) => [um(sexpr::num(m, 1).unwrap_or(0.0)), um(sexpr::num(m, 2).unwrap_or(0.0)), um(sexpr::num(m, 3).unwrap_or(0.0)), um(sexpr::num(m, 4).unwrap_or(0.0))],
        None => [TableCell::legacy_margin_nm(&style, DEFAULT_STROKE_NM); 4],
    };
    let (col_span, row_span) = match sexpr::find(item, "span") {
        Some(s) => (sexpr::num(s, 1).unwrap_or(1.0).max(0.0) as u32, sexpr::num(s, 2).unwrap_or(1.0).max(0.0) as u32),
        None => (1, 1),
    };
    let fill = sexpr::find(item, "fill");
    let cell = TableCell {
        uuid: crate::import_items::item_uuid(item).unwrap_or_default(),
        text,
        col_span,
        row_span,
        margins,
        style,
        fill: fill.and_then(|f| sexpr::find(f, "type")).and_then(|t| sexpr::txt(t, 1)).map(SchFill::from_token).unwrap_or(SchFill::None),
        fill_color: fill.and_then(|f| sexpr::find(f, "color")).and_then(read_color),
        exclude_from_sim: flag(item, "exclude_from_sim").unwrap_or(false),
    };
    Some((cell, FileCell { start, end, angle }))
}

/// The numbers of `(column_widths ..)` or `(row_heights ..)`.
fn sizes(item: &[Sexpr], name: &str) -> Vec<Nm> {
    sexpr::find(item, name).map(|l| l.iter().skip(1).filter_map(|x| x.text()?.parse::<f64>().ok()).map(um).collect()).unwrap_or_default()
}

/// One `(table ..)` of a sheet; `None` when it has no cell, or its cells do not fill rows of `column_count`.
pub(crate) fn read(item: &[Sexpr]) -> Option<ReadTable> {
    let cols = sexpr::find(item, "column_count").and_then(|c| sexpr::num(c, 1)).map(|c| c as i64).unwrap_or(0);
    let cells_node = sexpr::find(item, "cells")?;
    let mut cells = Vec::new();
    let mut placed = Vec::new();
    for c in cells_node.iter().skip(1).filter_map(Sexpr::as_list) {
        if sexpr::tag(c) != Some("table_cell") {
            return None;
        }
        let (cell, file) = read_cell(c)?;
        cells.push(cell);
        placed.push(file);
    }
    if cols < 1 || cells.is_empty() || cells.len() % cols as usize != 0 {
        return None;
    }
    let cols = cols as usize;
    let rows = cells.len() / cols;
    let angle = placed[0].angle;
    // The sizes of the grid: the lists the file keeps, else the extent of the cells of the first row and the first column along the way the grid runs.
    let stands = angle % 180_000 == 90_000;
    let mut widths = sizes(item, "column_widths");
    if widths.len() != cols {
        widths = (0..cols).map(|c| if stands { (placed[c].end.y - placed[c].start.y).abs() } else { (placed[c].end.x - placed[c].start.x).abs() }).collect();
    }
    let mut heights = sizes(item, "row_heights");
    if heights.len() != rows {
        heights = (0..rows).map(|r| if stands { (placed[r * cols].end.x - placed[r * cols].start.x).abs() } else { (placed[r * cols].end.y - placed[r * cols].start.y).abs() }).collect();
    }
    let default_stroke = TableStroke { width_nm: DEFAULT_STROKE_NM, style: TableLineStyle::Default, color: None };
    let mut border = TableBorder { external: true, header: true, stroke: default_stroke };
    if let Some(b) = sexpr::find(item, "border") {
        border.external = flag(b, "external").unwrap_or(border.external);
        border.header = flag(b, "header").unwrap_or(border.header);
        if let Some(s) = sexpr::find(b, "stroke") {
            border.stroke = read_stroke(s, default_stroke);
        }
    }
    let mut separators = TableSeparators { rows: true, cols: true, stroke: default_stroke };
    if let Some(s) = sexpr::find(item, "separators") {
        separators.rows = flag(s, "rows").unwrap_or(separators.rows);
        separators.cols = flag(s, "cols").unwrap_or(separators.cols);
        if let Some(st) = sexpr::find(s, "stroke") {
            separators.stroke = read_stroke(st, default_stroke);
        }
    }
    let uuid = crate::import_items::item_uuid(item);
    let table = Table {
        id: String::new(),
        uuid: uuid.clone().unwrap_or_default(),
        layer: String::new(),
        at: placed[0].start,
        angle,
        col_widths: widths,
        row_heights: heights,
        cells,
        border,
        separators,
    };
    Some(ReadTable { table, locked: flag(item, "locked").unwrap_or(false), uuid })
}

/// Every `(table ..)` of a sheet, in file order.
pub(crate) fn read_all(root: &[Sexpr]) -> Vec<ReadTable> {
    sexpr::find_all(root, "table").into_iter().filter_map(read).collect()
}

fn angle_str(m: u32) -> String {
    let d = m as f64 / 1000.0;
    if d == d.trunc() {
        format!("{}", d as i64)
    } else {
        format!("{d}")
    }
}

fn alpha_str(a: u8) -> String {
    fmt_mm_f(a as f64 / 255.0)
}

fn fill_str(c: &TableCell) -> String {
    match (c.fill, c.fill_color) {
        (SchFill::Color, Some(SchColor { r, g, b, a })) => format!("(fill (type color) (color {r} {g} {b} {}))", alpha_str(a)),
        (f, _) => format!("(fill (type {}))", f.token()),
    }
}

/// The uuid of cell `i` of `t`.
fn cell_uuid(t: &Table, i: usize) -> String {
    let c = &t.cells[i];
    if c.uuid.is_empty() {
        duid_for(&format!("sch_table:{}:cell:{i}", t.id), &t.id)
    } else {
        note_uuid(&c.uuid, &t.id);
        c.uuid.clone()
    }
}

/// One table as `saveTable` writes it; returns its uuid.
pub(crate) fn write(out: &mut String, t: &Table, locked: bool) -> String {
    let uuid = if t.uuid.is_empty() {
        duid_for(&format!("sch_table:{}", t.id), &t.id)
    } else {
        note_uuid(&t.uuid, &t.id);
        t.uuid.clone()
    };
    let _ = write!(out, "\t(table\n\t\t(column_count {})\n\t\t(border (external {}) (header {})", t.col_count(), yes(t.border.external), yes(t.border.header));
    if t.border.external || t.border.header {
        let _ = write!(out, " {}", stroke_str(&t.border.stroke));
    }
    let _ = write!(out, ")\n\t\t(separators (rows {}) (cols {})", yes(t.separators.rows), yes(t.separators.cols));
    if t.separators.rows || t.separators.cols {
        let _ = write!(out, " {}", stroke_str(&t.separators.stroke));
    }
    let _ = write!(out, ")\n\t\t(column_widths {})\n\t\t(row_heights {})\n\t\t(uuid \"{uuid}\")", t.col_widths.iter().map(|w| sch_mm(*w)).collect::<Vec<_>>().join(" "), t.row_heights.iter().map(|h| sch_mm(*h)).collect::<Vec<_>>().join(" "));
    if locked {
        out.push_str("\n\t\t(locked yes)");
    }
    out.push_str("\n\t\t(cells");
    for i in 0..t.cells.len() {
        let (row, col) = t.row_col(i);
        let c = &t.cells[i];
        let corners = t.cell_corners(row, col);
        let (start, end) = (corners[0], corners[2]);
        let _ = write!(
            out,
            "\n\t\t\t(table_cell {}\n\t\t\t\t(exclude_from_sim {})\n\t\t\t\t(at {} {} {})\n\t\t\t\t(size {} {})\n\t\t\t\t(margins {} {} {} {})\n\t\t\t\t(span {} {})\n\t\t\t\t{}\n\t\t\t\t{}\n\t\t\t\t(uuid \"{}\")\n\t\t\t)",
            sexpr_str(&c.text),
            yes(c.exclude_from_sim),
            sch_mm(start.x),
            sch_mm(start.y),
            angle_str(t.angle),
            sch_mm(end.x - start.x),
            sch_mm(end.y - start.y),
            sch_mm(c.margins[0]),
            sch_mm(c.margins[1]),
            sch_mm(c.margins[2]),
            sch_mm(c.margins[3]),
            c.col_span,
            c.row_span,
            fill_str(c),
            effects_str(&c.style),
            cell_uuid(t, i)
        );
    }
    out.push_str("\n\t\t)\n\t)\n");
    uuid
}

/// Every table of the sheet, after its graphics.
pub(crate) fn write_all(out: &mut String, sch: &SchematicSection) {
    for t in &sch.extras.tables {
        write(out, t, sch.extras.is_locked(&t.id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexpr::parse;

    /// The first two rows of the table KiCad's own QA sheet `qa/data/eeschema/issue23840/BusAndVectors.kicad_sch` has (cells of 26.67 and 21.59 by 2.54 mm).
    const KICAD_TABLE: &str = r#"(kicad_sch (version 20250114) (generator "eeschema")
	(table
		(column_count 2)
		(border
			(external yes)
			(header yes)
			(stroke
				(width 0)
				(type solid)
			)
		)
		(separators
			(rows yes)
			(cols yes)
			(stroke
				(width 0)
				(type solid)
			)
		)
		(column_widths 26.67 21.59)
		(row_heights 2.54 2.54)
		(uuid "df0f6b4f-dec6-4a60-8f35-956cd22f196e")
		(cells
			(table_cell "LED pin (by VectorIndex)"
				(exclude_from_sim no)
				(at 122.555 29.21 0)
				(size 26.67 2.54)
				(margins 0.9525 0.9525 0.9525 0.9525)
				(span 1 1)
				(fill
					(type none)
				)
				(effects
					(font
						(size 1.27 1.27)
					)
					(justify left top)
				)
				(uuid "b8109563-e2fa-4fb6-9110-83137bb1a864")
			)
			(table_cell "Expected net"
				(exclude_from_sim no)
				(at 149.225 29.21 0)
				(size 21.59 2.54)
				(margins 0.9525 0.9525 0.9525 0.9525)
				(span 1 1)
				(fill
					(type none)
				)
				(effects
					(font
						(size 1.27 1.27)
					)
					(justify left top)
				)
				(uuid "4c5687fb-aaf2-4da6-b5a0-b93a8246fe54")
			)
			(table_cell "LEDs0"
				(exclude_from_sim no)
				(at 122.555 31.75 0)
				(size 26.67 2.54)
				(margins 0.9525 0.9525 0.9525 0.9525)
				(span 1 1)
				(fill
					(type none)
				)
				(effects
					(font
						(size 1.27 1.27)
					)
					(justify left top)
				)
				(uuid "22627914-300f-43c1-90e6-f3c87d8fa3af")
			)
			(table_cell "CON_A_1"
				(exclude_from_sim no)
				(at 149.225 31.75 0)
				(size 21.59 2.54)
				(margins 0.9525 0.9525 0.9525 0.9525)
				(span 1 1)
				(fill
					(type none)
				)
				(effects
					(font
						(size 1.27 1.27)
					)
					(justify left top)
				)
				(uuid "c10e8989-6edb-4949-b08f-85b59b08f685")
			)
		)
	)
)"#;

    fn read_first(text: &str) -> ReadTable {
        let tree = parse(text).unwrap();
        read_all(tree.as_list().unwrap()).into_iter().next().expect("a table")
    }

    #[test]
    fn the_table_kicad_wrote_reads_as_a_grid_with_its_cells() {
        let r = read_first(KICAD_TABLE);
        let t = &r.table;
        assert_eq!((t.col_count(), t.row_count(), t.at, t.angle), (2, 2, Point { x: Nm::from_raw_shim(122_555_000), y: Nm::from_raw_shim(29_210_000) }, 0));
        assert_eq!((t.col_widths.clone(), t.row_heights.clone()), (vec![Nm::from_raw_shim(26_670_000), Nm::from_raw_shim(21_590_000)], vec![Nm::from_raw_shim(2_540_000), Nm::from_raw_shim(2_540_000)]));
        assert_eq!(t.texts(), vec![vec!["LED pin (by VectorIndex)", "Expected net"], vec!["LEDs0", "CON_A_1"]]);
        assert_eq!(r.uuid.as_deref(), Some("df0f6b4f-dec6-4a60-8f35-956cd22f196e"));
        assert!(!r.locked);
        assert_eq!((t.border.external, t.border.header, t.border.stroke.width_nm, t.border.stroke.style), (true, true, Nm::ZERO, TableLineStyle::Solid));
        assert_eq!((t.separators.rows, t.separators.cols, t.separators.stroke.width_nm), (true, true, Nm::ZERO));
        let c = &t.cells[0];
        // 1.27 mm text, left and top, the margin 0.9525 mm, no fill, in simulation
        assert_eq!((c.style.height_nm, c.style.width_nm, c.style.h_align, c.style.v_align, c.fill, c.exclude_from_sim), (Nm::from_raw_shim(1_270_000), Nm::from_raw_shim(1_270_000), eda_model::ir::TextJustify::Left, eda_model::ir::TextVAlign::Top, SchFill::None, false));
        assert!(c.margins.iter().all(|m| *m == Nm::from_raw_shim(952_500)), "{:?}", c.margins);
        assert_eq!(c.uuid, "b8109563-e2fa-4fb6-9110-83137bb1a864");
        // the cells are where the file put them
        for (i, (row, col)) in [(0, 0), (0, 1), (1, 0), (1, 1)].into_iter().enumerate() {
            let want = [Point { x: Nm::from_raw_shim(122_555_000), y: Nm::from_raw_shim(29_210_000) }, Point { x: Nm::from_raw_shim(149_225_000), y: Nm::from_raw_shim(29_210_000) }, Point { x: Nm::from_raw_shim(122_555_000), y: Nm::from_raw_shim(31_750_000) }, Point { x: Nm::from_raw_shim(149_225_000), y: Nm::from_raw_shim(31_750_000) }][i];
            assert_eq!(t.cell_corners(row, col)[0], want);
        }
    }

    #[test]
    fn a_table_is_written_as_kicad_reads_it_and_reads_back_the_same() {
        let r = read_first(KICAD_TABLE);
        let mut text = String::from("(kicad_sch (version 20250114) (generator \"eeschema\")\n");
        let uuid = write(&mut text, &r.table, true);
        text.push_str(")\n");
        assert_eq!(uuid, "df0f6b4f-dec6-4a60-8f35-956cd22f196e", "the uuid it had is the uuid it keeps");
        for token in ["(column_count 2)", "(column_widths 26.67 21.59)", "(row_heights 2.54 2.54)", "(locked yes)", "(at 122.555 29.21 0)", "(size 26.67 2.54)", "(span 1 1)", "(fill (type none))", "(justify left top)", "(exclude_from_sim no)"] {
            assert!(text.contains(token), "{token}\n{text}");
        }
        let again = read_first(&text);
        assert!(again.locked);
        assert_eq!(again.table.col_widths, r.table.col_widths);
        assert_eq!(again.table.at, r.table.at);
        assert_eq!(again.table.texts(), r.table.texts());
        assert_eq!(again.table.cells.iter().map(|c| c.uuid.clone()).collect::<Vec<_>>(), r.table.cells.iter().map(|c| c.uuid.clone()).collect::<Vec<_>>());
        assert_eq!(again.table.border, r.table.border);
        assert_eq!(again.table.separators, r.table.separators);
    }

    #[test]
    fn a_table_that_stands_keeps_its_attitude_and_its_cells_places() {
        let mut t = Table::new_sch_grid(Point { x: Nm::from_raw_shim(50_000_000), y: Nm::from_raw_shim(80_000_000) }, 2, 2, Nm::from_raw_shim(20_000_000), Nm::from_raw_shim(5_000_000), Nm::from_raw_shim(1_270_000));
        t.angle = 90_000;
        t.cells[0].text = "a \"quoted\"\nline".into();
        t.cells[1].col_span = 1;
        let mut text = String::from("(kicad_sch\n");
        write(&mut text, &t, false);
        text.push_str(")\n");
        // The first cell of a table that stands is where the origin is, and its end is a quarter turn about it: 5 mm to the right and 20 mm up the sheet.
        assert!(text.contains("(at 50 80 90)") && text.contains("(size 5 -20)"), "{text}");
        let back = read_first(&text).table;
        assert_eq!((back.at, back.angle, back.texts()[0][0].as_str()), (t.at, 90_000, "a \"quoted\"\nline"));
        assert_eq!(back.cell_corners(1, 1), t.cell_corners(1, 1));
    }

    #[test]
    fn a_stroke_the_file_does_not_write_is_the_default_width_and_a_cell_with_no_effects_is_left_and_top() {
        let text = r#"(kicad_sch
	(table (column_count 1) (border (external no) (header no)) (separators (rows no) (cols no))
		(column_widths 10) (row_heights 4)
		(cells (table_cell "x" (at 1 2 0) (size 10 4) (span 1 1))))
)"#;
        let t = read_first(text).table;
        assert_eq!((t.border.stroke.width_nm, t.separators.stroke.width_nm), (Nm::from_raw_shim(152_400), Nm::from_raw_shim(152_400)));
        assert_eq!((t.cells[0].style.h_align, t.cells[0].style.v_align, t.cells[0].style.height_nm), (eda_model::ir::TextJustify::Left, eda_model::ir::TextVAlign::Top, Nm::from_raw_shim(1_270_000)));
        // no margins: the legacy margin of the cell (the stroke's half and three quarters of 1.27 mm)
        assert!(t.cells[0].margins.iter().all(|m| *m == Nm::from_raw_shim(76_200 + 952_500)), "{:?}", t.cells[0].margins);
        // and nothing is written for the strokes of a table that draws none
        let mut out = String::new();
        write(&mut out, &t, false);
        assert!(out.contains("(border (external no) (header no))") && out.contains("(separators (rows no) (cols no))"), "{out}");
    }

    #[test]
    fn a_table_with_no_cells_or_a_ragged_last_row_is_not_read() {
        assert!(read_all(parse("(kicad_sch (table (column_count 2) (cells)))").unwrap().as_list().unwrap()).is_empty());
        let ragged = "(kicad_sch (table (column_count 2) (column_widths 1 1) (row_heights 1) (cells (table_cell \"a\" (at 0 0 0) (size 1 1)))))";
        assert!(read_all(parse(ragged).unwrap().as_list().unwrap()).is_empty());
    }
}
