//! The board's table verbs: Draw Table's commit, the Table Properties and Table Cell Properties dialogs, and the edits `EDIT_TABLE_TOOL_BASE` shares between the
//! editors (add a row or a column, delete them, merge and unmerge cells), each one undoable and one undo step.
//!
//! Ported from `pcbnew/tools/pcb_edit_table_tool.cpp` and `include/tool/edit_table_tool_base.h` (the structure edits, which are
//! [`eda_model::table::Table`]'s methods: one copy for the board and the sheet), `pcbnew/dialogs/dialog_table{,cell}_properties.cpp` (the fields of the two dialogs,
//! [`eda_model::table_edit`]) and `pcbnew/tools/drawing_tool.cpp` `DrawTable` (the commit, "Draw Table"). On the wire:
//! `{"op": "table", "verb": "add_row", "id": "tbl_..", "row": 0, "above": false}`.
//!
//! A cell is named by its row and column in the table (`cells: [[row, col], ..]`), the way KiCad's own cell addresses go (`B3`).
//! Moving, turning, flipping, deleting, locking and grouping a table are the generic verbs' (`move_items`, `rotate_items`, `flip_items`, `set_locked`, `group`), which
//! know tables as items of the board (`pcb_transform::transform_table`).

use super::pcb_transform::{flip_layer, side_specific, FlipDirection, Xform};
use super::Board;
use eda_model::table::Table;
use eda_model::table_edit::{edit_cells, patch_table, CellEdit, TablePatch};
use eda_model::CheckResult;
use serde::{Deserialize, Serialize};

/// One table verb -- see the module doc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verb", rename_all = "snake_case")]
pub enum TableCmd {
    /// `BOARD_COMMIT::Add( table )` of Draw Table, Place Board Stackup Table and Place Board Characteristics: a new table. Its id and its uuids are made here, whatever
    /// the table brings; it is refused when it is not a grid (no cell, a cell count that is not rows times columns) or its layer is not one the board has on.
    Add { table: Table },
    /// Delete the table (`DoDelete` of a table, or of every row or every column of it).
    Delete { id: String },
    /// `DIALOG_TABLE_PROPERTIES::TransferDataFromWindow` and the table rows of the Properties panel: the layer, the lock, and the table's own fields.
    Set {
        id: String,
        #[serde(default)]
        layer: Option<String>,
        #[serde(default)]
        locked: Option<bool>,
        #[serde(default)]
        patch: TablePatch,
    },
    /// `DIALOG_TABLECELL_PROPERTIES::TransferDataFromWindow` and the cell rows of the Properties panel, on every cell of `cells`.
    EditCells { id: String, cells: Vec<(usize, usize)>, edit: CellEdit },
    /// Add Row Above / Add Row Below (`doAddRowAbove`, `doAddRowBelow`): a row of copies of row `row`'s cells, above it or below.
    AddRow {
        id: String,
        row: usize,
        #[serde(default)]
        above: bool,
    },
    /// Add Column Before / After.
    AddColumn {
        id: String,
        col: usize,
        #[serde(default)]
        before: bool,
    },
    /// Delete Row(s): every row that has one of `rows` goes; all of them deletes the table.
    DeleteRows { id: String, rows: Vec<usize> },
    /// Delete Column(s).
    DeleteColumns { id: String, cols: Vec<usize> },
    /// Merge Cells: the block the cells span becomes one cell.
    MergeCells { id: String, cells: Vec<(usize, usize)> },
    /// Unmerge Cells.
    UnmergeCells { id: String, cells: Vec<(usize, usize)> },
}

impl TableCmd {
    /// The table the verb is about, for the log and the credit assignment.
    pub fn ids(&self) -> Vec<&str> {
        match self {
            TableCmd::Add { .. } => vec!["table"],
            TableCmd::Delete { id }
            | TableCmd::Set { id, .. }
            | TableCmd::EditCells { id, .. }
            | TableCmd::AddRow { id, .. }
            | TableCmd::AddColumn { id, .. }
            | TableCmd::DeleteRows { id, .. }
            | TableCmd::DeleteColumns { id, .. }
            | TableCmd::MergeCells { id, .. }
            | TableCmd::UnmergeCells { id, .. } => vec![id.as_str()],
        }
    }

    /// The line the activity log shows for the verb.
    pub fn describe(&self) -> String {
        match self {
            TableCmd::Add { table } => format!("table add --layer {} --size {}x{}", table.layer, table.col_count(), table.row_count()),
            TableCmd::Delete { id } => format!("table delete {id}"),
            TableCmd::Set { id, layer, locked, .. } => format!("table set {id}{}{}", layer.as_ref().map(|l| format!(" --layer {l}")).unwrap_or_default(), locked.map(|l| format!(" --locked {l}")).unwrap_or_default()),
            TableCmd::EditCells { id, cells, .. } => format!("table edit-cells {id} ({} cell(s))", cells.len()),
            TableCmd::AddRow { id, row, above } => format!("table add-row {id} --{} {}", if *above { "above" } else { "below" }, row + 1),
            TableCmd::AddColumn { id, col, before } => format!("table add-column {id} --{} {}", if *before { "before" } else { "after" }, col + 1),
            TableCmd::DeleteRows { id, rows } => format!("table delete-rows {id} {}", rows.iter().map(|r| (r + 1).to_string()).collect::<Vec<_>>().join(",")),
            TableCmd::DeleteColumns { id, cols } => format!("table delete-columns {id} {}", cols.iter().map(|c| (c + 1).to_string()).collect::<Vec<_>>().join(",")),
            TableCmd::MergeCells { id, cells } => format!("table merge {id} ({} cell(s))", cells.len()),
            TableCmd::UnmergeCells { id, cells } => format!("table unmerge {id} ({} cell(s))", cells.len()),
        }
    }
}

fn fail(code: &str, subject: &str, message: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(code, subject, message)]
}

/// The tables of a design must never share a uuid: a table made from another (paste, duplicate, a design block) starts again.
pub fn forget_uuids(t: &mut Table) {
    t.id.clear();
    t.uuid.clear();
    for c in &mut t.cells {
        c.uuid.clear();
    }
}

/// `PCB_TABLE::Move`, `Rotate` and `Flip` as `EDIT_TOOL` calls them (the transforms of `pcb_transform`, whose turns run clockwise on the screen where a table's own angle,
/// like every angle of a KiCad file, runs the other way).
pub(crate) fn transform_table(t: &mut Table, x: Xform, copper: usize) {
    match x {
        Xform::Move { dx, dy } => t.translate(dx, dy),
        Xform::Rotate { pivot, angle } => t.rotate(pivot, (-angle).rem_euclid(360_000) as u32),
        Xform::Flip { pivot, dir } => {
            let layer = flip_layer(&t.layer, copper);
            t.flip(pivot, dir == FlipDirection::LeftRight, layer.clone(), side_specific(&layer));
        }
    }
}

impl Board<'_> {
    /// Apply one table verb to the board.
    pub(crate) fn apply_table(&mut self, cmd: &TableCmd) -> Result<(), Vec<CheckResult>> {
        match cmd {
            TableCmd::Add { table } => self.add_table(table.clone()),
            TableCmd::Delete { id } => self.delete_table(id),
            TableCmd::Set { id, layer, locked, patch } => self.set_table(id, layer.as_deref(), *locked, patch),
            TableCmd::EditCells { id, cells, edit } => {
                let t = self.table_mut(id)?;
                edit_cells(t, cells, edit).map_err(|m| fail("ops_bad_table", id, m))
            }
            TableCmd::AddRow { id, row, above } => {
                let t = self.table_mut(id)?;
                if t.add_row(*row, *above) {
                    Ok(())
                } else {
                    Err(fail("ops_bad_table", id, format!("the table has no row {}", row + 1)))
                }
            }
            TableCmd::AddColumn { id, col, before } => {
                let t = self.table_mut(id)?;
                if t.add_column(*col, *before) {
                    Ok(())
                } else {
                    Err(fail("ops_bad_table", id, format!("the table has no column {}", col + 1)))
                }
            }
            TableCmd::DeleteRows { id, rows } => {
                let t = self.table_mut(id)?;
                if rows.is_empty() || rows.iter().any(|r| *r >= t.row_count()) {
                    return Err(fail("ops_bad_table", id, "no such row"));
                }
                // Every row of the table: `commit.Remove( table )`.
                let all = (0..t.row_count()).all(|r| rows.contains(&r));
                if all {
                    return self.delete_table(id);
                }
                if self.table_mut(id)?.delete_rows(rows) {
                    Ok(())
                } else {
                    Err(fail("ops_bad_table", id, "nothing to delete"))
                }
            }
            TableCmd::DeleteColumns { id, cols } => {
                let t = self.table_mut(id)?;
                if cols.is_empty() || cols.iter().any(|c| *c >= t.col_count()) {
                    return Err(fail("ops_bad_table", id, "no such column"));
                }
                let all = (0..t.col_count()).all(|c| cols.contains(&c));
                if all {
                    return self.delete_table(id);
                }
                if self.table_mut(id)?.delete_columns(cols) {
                    Ok(())
                } else {
                    Err(fail("ops_bad_table", id, "nothing to delete"))
                }
            }
            TableCmd::MergeCells { id, cells } => {
                let t = self.table_mut(id)?;
                let Some((_, _, _, _, exact)) = t.block_bounds(cells) else { return Err(fail("ops_bad_table", id, "no cells given")) };
                if cells.len() < 2 || !exact {
                    return Err(fail("ops_bad_table", id, "the selected cells are not a block of two cells or more"));
                }
                if t.merge(cells) {
                    Ok(())
                } else {
                    Err(fail("ops_bad_table", id, "the cells cannot be merged"))
                }
            }
            TableCmd::UnmergeCells { id, cells } => {
                let t = self.table_mut(id)?;
                if !cells.iter().any(|&(r, c)| t.cell(r, c).is_some_and(|x| x.col_span > 1 || x.row_span > 1)) {
                    return Err(fail("ops_bad_table", id, "none of the cells is merged"));
                }
                if t.unmerge(cells) {
                    Ok(())
                } else {
                    Err(fail("ops_bad_table", id, "nothing to unmerge"))
                }
            }
        }
    }

    /// A table is drawn on a technical or user layer the board has on, or on one of its copper layers (a table on copper is copper: KiCad lets it).
    fn table_layer_ok(&self, layer: &str, what: &str) -> Result<(), Vec<CheckResult>> {
        if layer.ends_with(".Cu") {
            self.known_layer(layer)
        } else {
            self.layer_is_on(layer, what)
        }
    }

    fn table_mut(&mut self, id: &str) -> Result<&mut Table, Vec<CheckResult>> {
        let dr = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_table", id, "the board has no tables"))?;
        dr.tables.iter_mut().find(|t| t.id == id).ok_or_else(|| fail("ops_unknown_table", id, "no table with this id"))
    }

    fn add_table(&mut self, mut table: Table) -> Result<(), Vec<CheckResult>> {
        if !table.is_well_formed() {
            return Err(fail("ops_bad_table", "table", "a table needs at least one cell, and as many cells as rows times columns"));
        }
        if table.layer.is_empty() {
            return Err(fail("ops_bad_table", "table", "a table needs a layer"));
        }
        self.table_layer_ok(&table.layer, "table")?;
        forget_uuids(&mut table);
        let dr = self.drawings_mut();
        dr.tables.push(table);
        dr.assign_missing_ids();
        Ok(())
    }

    pub(crate) fn delete_table(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let dr = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_table", id, "the board has no tables"))?;
        let before = dr.tables.len();
        dr.tables.retain(|t| t.id != id);
        if dr.tables.len() == before {
            return Err(fail("ops_unknown_table", id, "no table with this id"));
        }
        Ok(())
    }

    fn set_table(&mut self, id: &str, layer: Option<&str>, locked: Option<bool>, patch: &TablePatch) -> Result<(), Vec<CheckResult>> {
        if let Some(l) = layer {
            if l.is_empty() {
                return Err(fail("ops_bad_table", id, "a table needs a layer"));
            }
            self.table_layer_ok(l, id)?;
        }
        let t = self.table_mut(id)?;
        patch_table(t, patch).map_err(|m| fail("ops_bad_table", id, m))?;
        if let Some(l) = layer {
            t.layer = l.to_string();
        }
        if let Some(lock) = locked {
            let dr = self.drawings_mut();
            let mut set: std::collections::BTreeSet<String> = dr.locked_ids.iter().cloned().collect();
            if lock {
                set.insert(id.to_string());
            } else {
                set.remove(id);
            }
            dr.locked_ids = set.into_iter().collect();
        }
        Ok(())
    }
}
