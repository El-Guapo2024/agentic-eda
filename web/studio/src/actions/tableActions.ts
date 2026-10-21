// The table actions of KiCad's shared table tool, for every editor that has tables: Add Row Above / Below, Add Column Before / After, Delete Row(s) / Column(s), Merge Cells,
// Unmerge Cells, Edit Table, Export Table to CSV (`common.TableEditor.*` and `pcbnew.TableEditor.editTable`), and the selection tool's Select Column(s), Select Row(s) and Select Table
// (`common.InteractiveSelection.*`).
//
//   include/tool/edit_table_tool_base.h   EDIT_TABLE_TOOL_BASE::doAddRowAbove / doAddRowBelow / doAddColumnBefore / doAddColumnAfter / doDeleteRows / doDeleteColumns /
//                                         doMergeCells / doUnmergeCells    the structure edits: ONE copy of the logic for the board's tables and the sheet's, which is
//                                         what this file is -- an editor gives it a `TableEditor` (its tables, its selection, how to send a verb) and the same handlers run
//   pcbnew/tools/pcb_edit_table_tool.cpp  EditTable, ExportTableToCSV (eeschema/tools/sch_edit_table_tool.cpp is the same code)
//   pcbnew/tools/pcb_selection_tool.cpp   SelectColumns, SelectRows, SelectTable
//
// The structure edits themselves are the server's (`eda_model::table::Table`, one copy for both editors); a handler here picks what KiCad's does -- the cell the edit works from, the
// rows or columns the selected cells are in -- and sends one verb, so each is one undo step.
import type { ActionHandler } from "./commonActions";
import type { BoardTable, BoardTableCell } from "../api/types";
import type { TableCellAddr, TableCmdWire } from "../api/tableTypes";
import { saveTextFile } from "../api/libraryClient";
import { openTableCellDialog, openTableDialog, type TableScope } from "../state/tableDialogs";
import { canMerge, colsOf, csvFileName, extremeCell, rowsOf, selectColumns, selectRows, selectTables, selectedCells, shiftColsAfterInsert, shiftRowsAfterInsert, tableCsv } from "../kicad-port/tables";

/** What an editor gives the table actions. */
export interface TableEditor {
  /** The editor's tables: the board's or the sheet's (which the dialogs it opens are for). */
  readonly scope: TableScope;
  tables(): readonly BoardTable[];
  selection(): readonly string[];
  setSelection(ids: string[]): void;
  /** The id of the cell under the pointer, for the actions run with nothing selected (`RequestSelection`'s hover selection), or null. */
  hoverCellId(): string | null;
  /** Sends one table verb; resolves to whether the server accepted it. */
  send(verb: TableCmdWire): Promise<boolean>;
}

/** `getTableCellSelection()`: the selected cells (those of the cell under the pointer when nothing is selected), grouped by table; the table of the first one is the table. */
function cellSelection(ed: TableEditor): { table: BoardTable; cells: BoardTableCell[]; hover: boolean }[] {
  const host = { drawings: { tables: ed.tables() } };
  let groups = selectedCells(host, ed.selection());
  let hover = false;
  if (groups.length === 0) {
    const h = ed.hoverCellId();
    if (h) {
      groups = selectedCells(host, [h]);
      hover = groups.length > 0;
    }
  }
  return groups.map((g) => ({ ...g, hover }));
}

const addr = (c: BoardTableCell): TableCellAddr => [c.row, c.col];

/** Registers the table actions of an editor. */
export function registerTableEditorActions(m: Map<string, ActionHandler>, ed: TableEditor): void {
  /** The cells of the selection (all tables), and the one table the edit is about: that of the first cell (`selection[0]->GetParent()`). */
  const current = () => {
    const groups = cellSelection(ed);
    const first = groups[0];
    if (!first) return null;
    const tableOfCell = new Map<string, BoardTable>();
    for (const g of groups) for (const c of g.cells) tableOfCell.set(c.id, g.table);
    return { table: first.table, cells: first.cells, all: groups.flatMap((g) => g.cells), tableOfCell, hover: first.hover };
  };

  // `doAddRowAbove`: from the topmost selected cell's row; `doAddRowBelow`: from the bottommost's.
  const addRow = (above: boolean): ActionHandler => () => {
    const cur = current();
    if (!cur) return;
    const from = extremeCell(cur.all, above ? "top" : "bottom");
    const table = from ? cur.tableOfCell.get(from.id) : undefined;
    if (!from || !table) return;
    void ed.send({ verb: "add_row", id: table.id, row: from.row, above }).then((ok) => {
      if (ok && !cur.hover) ed.setSelection(shiftRowsAfterInsert(ed.selection(), table.id, above ? from.row : from.row + 1));
    });
  };
  m.set("common.TableEditor.addRowAbove", addRow(true));
  m.set("common.TableEditor.addRowBelow", addRow(false));

  // `doAddColumnBefore`: from the leftmost selected cell's column; `doAddColumnAfter`: from the rightmost's.
  const addColumn = (before: boolean): ActionHandler => () => {
    const cur = current();
    if (!cur) return;
    const from = extremeCell(cur.all, before ? "left" : "right");
    const table = from ? cur.tableOfCell.get(from.id) : undefined;
    if (!from || !table) return;
    void ed.send({ verb: "add_column", id: table.id, col: from.col, before }).then((ok) => {
      if (ok && !cur.hover) ed.setSelection(shiftColsAfterInsert(ed.selection(), table.id, before ? from.col : from.col + 1));
    });
  };
  m.set("common.TableEditor.addColBefore", addColumn(true));
  m.set("common.TableEditor.addColAfter", addColumn(false));

  // `doDeleteRows` / `doDeleteColumns`: every row (column) that has a selected cell, of the first cell's table; the selection is cleared first, and the table goes when every row
  // (column) of it does.
  m.set("common.TableEditor.deleteRows", () => {
    const cur = current();
    if (!cur) return;
    ed.setSelection([]);
    void ed.send({ verb: "delete_rows", id: cur.table.id, rows: rowsOf(cur.cells) });
  });
  m.set("common.TableEditor.deleteColumns", () => {
    const cur = current();
    if (!cur) return;
    ed.setSelection([]);
    void ed.send({ verb: "delete_columns", id: cur.table.id, cols: colsOf(cur.cells) });
  });

  // `doMergeCells`: the block of selected cells (the menu offers it for two cells or more that fill a rectangle) becomes its top-left cell, which is what stays selected.
  m.set("common.TableEditor.mergeCells", () => {
    const cur = current();
    if (!cur || !canMerge(cur.cells)) return;
    const topLeft = [...cur.cells].sort((a, b) => a.row - b.row || a.col - b.col)[0]!;
    void ed.send({ verb: "merge_cells", id: cur.table.id, cells: cur.cells.map(addr) }).then((ok) => {
      if (ok) ed.setSelection([topLeft.id]);
    });
  });
  // `doUnmergeCells`.
  m.set("common.TableEditor.unmergeCell", () => {
    const cur = current();
    if (!cur) return;
    void ed.send({ verb: "unmerge_cells", id: cur.table.id, cells: cur.cells.map(addr) });
  });

  // `EditTable`: the table all the selected cells belong to (cells of two tables do nothing); Table Properties.
  m.set("pcbnew.TableEditor.editTable", () => {
    const groups = cellSelection(ed);
    if (groups.length !== 1) return;
    openTableDialog(groups[0]!.table.id, ed.scope);
  });

  // `ExportTableToCSV`: the shown text of every cell, to a file.
  m.set("common.TableEditor.exportTableCSV", () => {
    const groups = cellSelection(ed);
    if (groups.length !== 1) return;
    const t = groups[0]!.table;
    saveTextFile(tableCsv(t), csvFileName(t), "text/csv");
  });

  // `PCB_SELECTION_TOOL::SelectColumns` / `SelectRows` / `SelectTable`.
  m.set("common.InteractiveSelection.SelectColumns", () => ed.setSelection(selectColumns({ drawings: { tables: ed.tables() } }, ed.selection())));
  m.set("common.InteractiveSelection.Rows", () => ed.setSelection(selectRows({ drawings: { tables: ed.tables() } }, ed.selection())));
  m.set("common.InteractiveSelection.SelectTable", () => ed.setSelection(selectTables({ drawings: { tables: ed.tables() } }, ed.selection())));
}

/**
 * `EDIT_TOOL::Properties` for what the selection holds: table cells only (`OnlyTypes( { PCB_TABLECELL_T } )`) open Table Cell Properties on all of them -- those of the clicked cell's
 * table -- and a table, Table Properties (`OnEditItemRequest`). Returns whether `id` was one of them.
 */
export function openTablePropertiesFor(tables: readonly BoardTable[], selection: readonly string[], id: string, scope: TableScope = "pcb"): boolean {
  const host = { drawings: { tables } };
  const table = tables.find((t) => t.id === id);
  if (table) {
    openTableDialog(table.id, scope);
    return true;
  }
  const own = selectedCells(host, [id])[0];
  if (!own) return false;
  const same = selectedCells(host, selection).find((g) => g.table.id === own.table.id);
  const cells = same && selection.includes(id) ? same.cells : own.cells;
  openTableCellDialog(own.table.id, cells.map(addr), scope);
  return true;
}

