//! Tables and cells on the clipboard: the table a copy of some cells makes, and the paste of a copied table into selected cells, one copy of the code for the board
//! and the sheet as `EDIT_TABLE_TOOL_BASE` (`include/tool/edit_table_tool_base.h`) is.
//!
//! Ported from:
//!   pcbnew/kicad_clipboard.cpp                         `CLIPBOARD_IO::SaveSelection`'s `deleteUnselectedCells` ([`Table::selected_block`])
//!   eeschema/sch_io/kicad_sexpr/sch_io_kicad_sexpr.cpp `SCH_IO_KICAD_SEXPR::saveTable` of a table flagged `SKIP_STRUCT` (the same cut, for the sheet's clipboard)
//!   include/tool/edit_table_tool_base.h                `getCellBlockBounds`, `validatePasteIntoSelection` ([`validate_paste_into`]) and
//!                                                      `pasteCellsIntoSelection` ([`Table::paste_cells`])
//!   pcbnew/tools/pcb_edit_table_tool.cpp               `PCB_EDIT_TABLE_TOOL::copyCell` (the sheet's `SCH_EDIT_TABLE_TOOL::copyCell` is the same)

use crate::ir::Nm;
use crate::table::{Table, TableCell};
use std::collections::BTreeSet;

/// `getCellBlockBounds`: the block `cells` cover, `(col_min, col_max, row_min, row_max)` with the maxima one past the last column and row a cell spans, and whether
/// the cells fill it (the area their spans add up to is the block's; a single cell always does). `None` when there is no cell.
pub fn cell_block_bounds(t: &Table, cells: &[(usize, usize)]) -> Option<(usize, usize, usize, usize, bool)> {
    if cells.is_empty() {
        return None;
    }
    let (mut col_min, mut col_max, mut row_min, mut row_max) = (usize::MAX, 0usize, usize::MAX, 0usize);
    let mut area = 0usize;
    for &(r, c) in cells {
        let Some(cell) = t.cell(r, c) else { continue };
        let (cs, rs) = (cell.col_span as usize, cell.row_span as usize);
        col_min = col_min.min(c);
        col_max = col_max.max(c + cs);
        row_min = row_min.min(r);
        row_max = row_max.max(r + rs);
        area += cs * rs;
    }
    if col_min == usize::MAX {
        return None;
    }
    let contiguous = cells.len() == 1 || area == (col_max.saturating_sub(col_min)) * (row_max.saturating_sub(row_min));
    Some((col_min, col_max, row_min, row_max, contiguous))
}

/// One item of the selection a paste into cells is asked for: a cell of a table (`<table>:<row>:<col>`), with that table, or anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteTarget<'a> {
    Cell { table: &'a str, row: usize, col: usize },
    Other,
}

/// `validatePasteIntoSelection`: the selection must be cells of one table, none of them merged, that fill a block. `Err` is KiCad's message, word for word, which the
/// editor shows (`DisplayError`) and pastes nothing. `t` is the table the first cell is in.
pub fn validate_paste_into(t: &Table, selection: &[PasteTarget<'_>]) -> Result<Vec<(usize, usize)>, String> {
    if selection.is_empty() {
        return Err("No cells selected".to_string());
    }
    let mut table: Option<&str> = None;
    let mut cells: Vec<(usize, usize)> = Vec::new();
    for item in selection {
        let PasteTarget::Cell { table: tid, row, col } = *item else {
            return Err("Selection contains non-cell items".to_string());
        };
        match table {
            None => table = Some(tid),
            Some(first) if first != tid => return Err("Selected cells are from different tables".to_string()),
            Some(_) => {}
        }
        // "Check for merged cells (block paste-into for now)"
        if t.cell(row, col).is_some_and(|c| c.col_span > 1 || c.row_span > 1) {
            return Err("Cannot paste into merged cells".to_string());
        }
        cells.push((row, col));
    }
    match cell_block_bounds(t, &cells) {
        Some((_, _, _, _, true)) => Ok(cells),
        _ => Err("Selected cells must form a contiguous block".to_string()),
    }
}

impl Table {
    /// `SaveSelection`'s `deleteUnselectedCells` on a clone of the table (and `saveTable` of a table flagged `SKIP_STRUCT` on the sheet): the cells not in `sel` are
    /// deleted (`DeleteMarkedCells`), the rows and columns from the first to the last selected one keep their heights and widths, and the cells left are laid out
    /// `maxCol - minCol + 1` to a row (`SetColCount`) from the first one's corner (`GetPosition` is the first cell's, which `Normalize` lays the rest out from). A
    /// block of cells is copied as that block; a selection that is not one is packed into rows the way KiCad packs it, a short last row dropped (`GetRowCount` is the
    /// cell count divided by the column count). **Differs**: a merged cell's span is cut to the new table (KiCad keeps it, and the copy then spans cells it does not
    /// have).
    pub fn selected_block(&self, sel: &BTreeSet<(usize, usize)>) -> Table {
        let (min_row, max_row) = (sel.iter().map(|c| c.0).min().unwrap_or(0), sel.iter().map(|c| c.0).max().unwrap_or(0));
        let (min_col, max_col) = (sel.iter().map(|c| c.1).min().unwrap_or(0), sel.iter().map(|c| c.1).max().unwrap_or(0));
        let cols = max_col - min_col + 1;
        let kept: Vec<(usize, usize)> = (0..self.cells.len()).map(|i| self.row_col(i)).filter(|rc| sel.contains(rc)).collect();
        let rows = (kept.len() / cols).max(1);
        let first = kept.first().copied().unwrap_or((min_row, min_col));
        let (x, y, _, _) = self.cell_local_rect(first.0, first.1);
        let filler = || if self.layer.is_empty() { TableCell::sch_new(self.cells.first().map_or(Nm::new(1_270_000), |c| c.style.height_nm)) } else { TableCell::pcb_new(false) };
        let mut out = self.clone();
        out.at = self.to_world((x, y));
        out.col_widths = (0..cols).map(|c| self.col_widths.get(min_col + c).copied().unwrap_or(Nm::ZERO)).collect();
        out.row_heights = (0..rows).map(|r| if min_row + r <= max_row { self.row_heights.get(min_row + r).copied().unwrap_or(Nm::ZERO) } else { Nm::ZERO }).collect();
        out.cells = kept
            .iter()
            .take(rows * cols)
            .enumerate()
            .map(|(i, &(r, c))| {
                let mut cell = self.cell(r, c).cloned().unwrap_or_else(filler);
                let (nr, nc) = (i / cols, i % cols);
                cell.col_span = cell.col_span.min((cols - nc) as u32);
                cell.row_span = cell.row_span.min((rows - nr) as u32);
                cell
            })
            .collect();
        while out.cells.len() < rows * cols {
            out.cells.push(filler());
        }
        out
    }

    /// `PCB_EDIT_TABLE_TOOL::copyCell` / `SCH_EDIT_TABLE_TOOL::copyCell` for the rows and columns a paste adds: the source cell's looks, no text, a new uuid.
    /// **Differs**: the copy spans one cell (KiCad's copy constructor keeps the source's span, which on a new row or column spans cells that are not merged into it).
    fn paste_copy(src: &TableCell) -> TableCell {
        let mut c = src.clone();
        c.uuid.clear();
        c.text.clear();
        c.col_span = 1;
        c.row_span = 1;
        c
    }

    /// `pasteCellsIntoSelection( aSel, aSourceTable, aCommit )`: the cells of `source` (a copied table) put into this table from the top-left cell of the selected
    /// block (`row_min`, `col_min`; the caller has validated the selection, [`validate_paste_into`]). Where the source runs past the last row or column, rows and then
    /// columns are added first: each new row a copy of the looks of the source's row that lands there (its cell for each column, its first cell past the source's
    /// columns; KiCad inserts every cell of a row at the row's first index, so they come out right to left) with that row's height, each new column likewise with its
    /// width, the table staying where it was (`SetPosition( pos )`). Then each target cell takes the source cell's text, its text looks (`SetAttributes( *sourceCell,
    /// false )`), and its fill (`SetFillMode`, `SetFillColor`); its margins, uuid and span stay. Returns `false` when there is nothing to paste.
    pub fn paste_cells(&mut self, row_min: usize, col_min: usize, source: &Table) -> bool {
        if !source.is_well_formed() || !self.is_well_formed() || row_min >= self.row_count() || col_min >= self.col_count() {
            return false;
        }
        let (source_rows, source_cols) = (source.row_count(), source.col_count());
        let (paste_end_row, paste_end_col) = (row_min + source_rows, col_min + source_cols);
        if paste_end_row > self.row_count() || paste_end_col > self.col_count() {
            let rows_to_add = paste_end_row.saturating_sub(self.row_count());
            let cols_to_add = paste_end_col.saturating_sub(self.col_count());
            for _ in 0..rows_to_add {
                let insert_row = self.row_count();
                let clipboard_row = insert_row - row_min;
                let sources: Vec<&TableCell> = (0..source_cols).filter_map(|c| source.cell(clipboard_row, c)).collect();
                let cols = self.col_count();
                let at = insert_row * cols;
                for col in 0..cols {
                    let src = if col < source_cols { sources[col] } else { sources[0] };
                    // `InsertCell( insertRow * GetColCount(), cell )`: each one in front of the last
                    self.cells.insert(at, Self::paste_copy(src));
                }
                self.row_heights.push(source.row_heights[clipboard_row]);
            }
            for _ in 0..cols_to_add {
                let insert_col = self.col_count();
                let clipboard_col = insert_col - col_min;
                let row_count = self.row_count();
                let sources: Vec<&TableCell> = (0..source_rows).filter_map(|r| source.cell(r, clipboard_col)).collect();
                // `SetColCount( GetColCount() + 1 )`, then `InsertCell( row * GetColCount() + insertCol, cell )` row after row
                let new_cols = insert_col + 1;
                for row in 0..row_count {
                    let src = if row < source_rows { sources[row] } else { sources[0] };
                    self.cells.insert(row * new_cols + insert_col, Self::paste_copy(src));
                }
                self.col_widths.push(source.col_widths[clipboard_col]);
            }
        }
        for src_row in 0..source_rows {
            for src_col in 0..source_cols {
                let (dest_row, dest_col) = (row_min + src_row, col_min + src_col);
                if dest_row >= self.row_count() || dest_col >= self.col_count() {
                    continue;
                }
                let (Some(src), Some(dst)) = (source.cell(src_row, src_col).cloned(), self.cell_mut(dest_row, dest_col)) else { continue };
                dst.text = src.text;
                dst.style = src.style;
                dst.fill = src.fill;
                dst.fill_color = src.fill_color;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Point;

    fn grid(rows: usize, cols: usize, tag: &str) -> Table {
        let mut t = Table::new_grid("F.SilkS", Point { x: Nm::ZERO, y: Nm::ZERO }, cols, rows, Nm::new(10_000_000), Nm::new(5_000_000), Nm::new(100_000));
        t.id = format!("tbl_{tag}");
        for (i, c) in t.cells.iter_mut().enumerate() {
            c.text = format!("{tag}{i}");
        }
        t
    }

    fn cells(t: &Table, list: &[(usize, usize)]) -> Vec<PasteTarget<'static>> {
        let id: &'static str = Box::leak(t.id.clone().into_boxed_str());
        list.iter().map(|&(row, col)| PasteTarget::Cell { table: id, row, col }).collect()
    }

    #[test]
    fn a_paste_into_cells_is_refused_with_kicads_words() {
        let mut t = grid(3, 3, "t");
        assert_eq!(validate_paste_into(&t, &[]), Err("No cells selected".into()));
        let mut sel = cells(&t, &[(0, 0)]);
        sel.push(PasteTarget::Other);
        assert_eq!(validate_paste_into(&t, &sel), Err("Selection contains non-cell items".into()));
        let mut sel = cells(&t, &[(0, 0)]);
        sel.push(PasteTarget::Cell { table: "tbl_other", row: 0, col: 1 });
        assert_eq!(validate_paste_into(&t, &sel), Err("Selected cells are from different tables".into()));
        assert_eq!(validate_paste_into(&t, &cells(&t, &[(0, 0), (1, 1)])), Err("Selected cells must form a contiguous block".into()));
        assert_eq!(validate_paste_into(&t, &cells(&t, &[(0, 0), (0, 1), (1, 0), (1, 1)])), Ok(vec![(0, 0), (0, 1), (1, 0), (1, 1)]));
        assert!(t.merge(&[(1, 1), (1, 2)]));
        assert_eq!(validate_paste_into(&t, &cells(&t, &[(1, 1)])), Err("Cannot paste into merged cells".into()));
    }

    #[test]
    fn copied_cells_land_from_the_first_selected_one_with_their_looks_and_the_target_keeps_its_margins() {
        let mut t = grid(3, 3, "t");
        let mut s = grid(2, 2, "s");
        s.cells[3].style.bold = true;
        s.cells[3].fill = crate::sch_extras::SchFill::Color;
        s.cells[3].margins = [Nm::new(1); 4];
        let margins = t.cell(2, 2).unwrap().margins;
        assert!(t.paste_cells(1, 1, &s));
        assert_eq!(t.texts(), vec![vec!["t0", "t1", "t2"], vec!["t3", "s0", "s1"], vec!["t6", "s2", "s3"]]);
        let c = t.cell(2, 2).unwrap();
        assert!(c.style.bold && c.fill == crate::sch_extras::SchFill::Color);
        assert_eq!(c.margins, margins, "`pasteCellsIntoSelection` copies no margin");
    }

    #[test]
    fn a_paste_that_runs_past_the_table_adds_rows_then_columns_as_kicad_inserts_them() {
        let mut t = grid(2, 2, "t");
        let mut s = grid(2, 3, "s");
        s.row_heights = vec![Nm::new(6_000_000), Nm::new(7_000_000)];
        s.col_widths = vec![Nm::new(11_000_000), Nm::new(12_000_000), Nm::new(13_000_000)];
        for (i, c) in s.cells.iter_mut().enumerate() {
            c.style.height_nm = Nm::new(1_000_000 + i as i64);
        }
        // the selected cell is the second row's second one: the source's second row goes on a new row, its second and third columns on new columns
        assert!(t.paste_cells(1, 1, &s));
        assert_eq!((t.row_count(), t.col_count()), (3, 4));
        assert_eq!(t.row_heights, vec![Nm::new(5_000_000), Nm::new(5_000_000), Nm::new(7_000_000)]);
        assert_eq!(t.col_widths, vec![Nm::new(10_000_000), Nm::new(10_000_000), Nm::new(12_000_000), Nm::new(13_000_000)]);
        assert_eq!(t.texts(), vec![vec!["t0", "t1", "", ""], vec!["t2", "s0", "s1", "s2"], vec!["", "s3", "s4", "s5"]]);
        // the new row's cell under the first column is a copy of the source row's cell for column 1, inserted right to left (`sources[col]` for col 1 lands at index 0)
        assert_eq!(t.cell(2, 0).unwrap().style.height_nm, Nm::new(1_000_000 + 4));
        assert!(t.is_well_formed());
        assert_eq!(t.at, Point { x: Nm::ZERO, y: Nm::ZERO }, "the table stays where it was");
    }

    #[test]
    fn copied_cells_are_a_table_of_the_block() {
        let t = grid(3, 3, "t");
        let b = t.selected_block(&BTreeSet::from([(1, 1), (1, 2), (2, 1), (2, 2)]));
        assert_eq!(b.texts(), vec![vec!["t4", "t5"], vec!["t7", "t8"]]);
        assert_eq!(b.at, Point { x: Nm::new(10_000_000), y: Nm::new(5_000_000) });
    }
}
