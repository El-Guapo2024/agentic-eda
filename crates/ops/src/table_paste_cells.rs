//! Paste into table cells: `TableCmd::PasteCells`, the branch of `PCB_CONTROL::Paste` (`pcbnew/tools/pcb_control.cpp`) and `SCH_EDITOR_CONTROL::Paste`
//! (`eeschema/tools/sch_editor_control.cpp`) taken when the selection holds a table cell and the clipboard a table: the first table of the clipboard
//! (`clipBoard->Drawings()` on a board, `tempScreen->Items()` on a sheet) goes into the selected cells (`EDIT_TABLE_TOOL_BASE::validatePasteIntoSelection` and
//! `pasteCellsIntoSelection`, [`eda_model::table_paste`]), one commit ("Paste Cells"). A selection KiCad refuses is refused with its message (`DisplayError`).

use super::pcb_tables::Scope;
use super::Board;
use eda_model::table::parse_cell_item_id;
use eda_model::table_paste::{cell_block_bounds, validate_paste_into, PasteTarget};
use eda_model::CheckResult;

fn fail(code: &str, subject: &str, message: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(code, subject, message)]
}

impl Board<'_> {
    /// See the module doc. `ids` is the editor's selection; `text` the clipboard's KiCad text (a board's `(kicad_pcb ..)` or a sheet's fragment).
    pub(crate) fn paste_cells_in(&mut self, scope: Scope, ids: &[String], text: &str) -> Result<(), Vec<CheckResult>> {
        let source = match scope {
            Scope::Board => eda_kicad::parse_pcb_clipboard(text).ok().and_then(|c| c.tables.into_iter().find(|t| t.is_well_formed())),
            Scope::Sheet => eda_kicad::parse_clipboard(text).ok().and_then(|f| f.section.extras.tables.into_iter().find(|t| t.is_well_formed())),
        };
        let Some(source) = source else { return Err(fail("ops_no_clipboard_table", "paste", "the clipboard holds no table")) };
        // The table of the first selected cell: `validatePasteIntoSelection` measures the selection against it.
        let first = ids.iter().find_map(|id| parse_cell_item_id(id).map(|(t, _, _)| t.to_string()));
        let Some(table_id) = first else { return Err(fail("ops_bad_paste_cells", "paste", "No cells selected")) };
        let exists = |t: &str| match scope {
            Scope::Board => self.design.drawings.as_ref().is_some_and(|d| d.tables.iter().any(|x| x.id == t)),
            Scope::Sheet => self.schematic().is_ok_and(|s| s.extras.tables.iter().any(|x| x.id == t)),
        };
        let targets: Vec<PasteTarget<'_>> = ids
            .iter()
            .map(|id| match parse_cell_item_id(id) {
                Some((t, row, col)) if exists(t) => PasteTarget::Cell { table: t, row, col },
                _ => PasteTarget::Other,
            })
            .collect();
        let table = self.table_mut_in(scope, &table_id)?;
        let cells = validate_paste_into(table, &targets).map_err(|m| fail("ops_bad_paste_cells", &table_id, m))?;
        let Some((col_min, _, row_min, _, _)) = cell_block_bounds(table, &cells) else { return Err(fail("ops_bad_paste_cells", &table_id, "No cells selected")) };
        if table.paste_cells(row_min, col_min, &source) {
            Ok(())
        } else {
            Err(fail("ops_bad_paste_cells", &table_id, "Failed to paste cells"))
        }
    }
}
