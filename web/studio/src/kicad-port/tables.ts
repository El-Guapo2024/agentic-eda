// Tables (`PCB_TABLE`, `PCB_TABLECELL`) in the board editor: finding a table and its cells in the state, what a click, a window or a menu does with them, and the
// CSV that Export Table to CSV writes.
//
// The geometry is not here: the server works out every cell's corners, the border lines and the placed lines of each cell's text (`crates/cli/src/table_json.rs`,
// `eda_model::table`, `eda_model::table_text`), so the canvas draws what kicad-cli plots and nothing computes a table's layout twice.
//
// Ported from pcbnew/tools/pcb_selection_tool.cpp (`SelectColumns`, `SelectRows`, `SelectTable`, `hitTestDistance`, `Selectable`), include/tool/edit_table_tool_base.h (the menu
// conditions `cellBlockSelection` and `mergedCellsSelection`, `getCellBlockBounds`) and pcbnew/tools/pcb_edit_table_tool.cpp (`ExportTableToCSV`).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BoardState, BoardTable, BoardTableCell } from "../api/types";
import { isBackLayer } from "./layerDefaults";

/** The id of a cell as an item of the editor: `<table id>:<row>:<col>` (`eda_model::table::cell_item_id`). */
export function cellItemId(table: string, row: number, col: number): string {
  return `${table}:${row}:${col}`;
}

/** The table, row and column of a cell id, or null for any other id. */
export function parseCellItemId(id: string): { table: string; row: number; col: number } | null {
  const m = /^(.+):(\d+):(\d+)$/.exec(id);
  return m ? { table: m[1]!, row: Number(m[2]), col: Number(m[3]) } : null;
}

export function tablesOf(board: Pick<BoardState, "drawings"> | null | undefined): readonly BoardTable[] {
  return board?.drawings?.tables ?? [];
}

export function tableById(board: Pick<BoardState, "drawings"> | null | undefined, id: string): BoardTable | undefined {
  return tablesOf(board).find((t) => t.id === id);
}

/** The cell a cell id names, with its table. */
export function cellById(board: Pick<BoardState, "drawings"> | null | undefined, id: string): { table: BoardTable; cell: BoardTableCell } | undefined {
  const p = parseCellItemId(id);
  if (!p) return undefined;
  const table = tableById(board, p.table);
  const cell = table?.cells[p.row * (table?.cols ?? 0) + p.col];
  return table && cell && cell.row === p.row && cell.col === p.col ? { table, cell } : undefined;
}

/** `PCB_TABLECELL::GetAddr`: `B3` for the second column, third row. */
export function cellAddress(row: number, col: number): string {
  return `${String.fromCharCode(65 + (col % 26))}${row + 1}`;
}

/** A cell another one covers is not an item (`Selectable`: `GetRowSpan() == 0 || GetColSpan() == 0`). */
export function isSelectableCell(c: BoardTableCell): boolean {
  return c.col_span > 0 && c.row_span > 0;
}

type Pt = readonly [number, number];

/** How far (um) `(x, y)` is outside the quadrilateral `poly` (a turned rectangle): 0 inside. */
export function polygonDistance(poly: readonly Pt[], x: number, y: number): number {
  let sign = 0;
  let inside = true;
  let best = Infinity;
  for (let i = 0; i < poly.length; i++) {
    const a = poly[i]!;
    const b = poly[(i + 1) % poly.length]!;
    const abx = b[0] - a[0];
    const aby = b[1] - a[1];
    const cross = abx * (y - a[1]) - aby * (x - a[0]);
    const s = Math.sign(cross);
    if (s !== 0) {
      if (sign === 0) sign = s;
      else if (sign !== s) inside = false;
    }
    const len2 = abx * abx + aby * aby;
    const t = len2 === 0 ? 0 : Math.min(1, Math.max(0, ((x - a[0]) * abx + (y - a[1]) * aby) / len2));
    best = Math.min(best, Math.hypot(x - (a[0] + t * abx), y - (a[1] + t * aby)));
  }
  return inside ? 0 : best;
}

/** `hitTestDistance` of a table cell: how far the point is from its box (0 inside). */
export function cellHitDistance(c: BoardTableCell, x: number, y: number): number {
  return polygonDistance(c.corners, x, y);
}

/** The area of a cell's box (um squared), for the selection tool's "a small item inside a large one" rule. */
export function cellArea(c: BoardTableCell): number {
  const [a, b, , d] = c.corners;
  return Math.hypot(b[0] - a[0], b[1] - a[1]) * Math.hypot(d[0] - a[0], d[1] - a[1]);
}

/** The box round the first and the last cell (`PCB_TABLE::GetBoundingBox`). */
export function tableBox(t: BoardTable): readonly [number, number, number, number] {
  return t.bbox;
}

/** The cells of a table, a row at a time. */
export function cellAt(t: BoardTable, row: number, col: number): BoardTableCell | undefined {
  return row >= 0 && col >= 0 && col < t.cols ? t.cells[row * t.cols + col] : undefined;
}

// --------------------------------------------------------------------------------------------------------------------------------------------- selections

/** The cells among `ids`, grouped by table. */
export function selectedCells(board: Pick<BoardState, "drawings">, ids: Iterable<string>): { table: BoardTable; cells: BoardTableCell[] }[] {
  const by = new Map<string, { table: BoardTable; cells: BoardTableCell[] }>();
  for (const id of ids) {
    const found = cellById(board, id);
    if (!found) continue;
    const e = by.get(found.table.id) ?? { table: found.table, cells: [] };
    e.cells.push(found.cell);
    by.set(found.table.id, e);
  }
  return [...by.values()];
}

/** `SelectColumns`: the cells of the columns the selected cells are in, the selection kept. */
export function selectColumns(board: Pick<BoardState, "drawings">, selection: readonly string[]): string[] {
  const out = new Set(selection);
  for (const { table, cells } of selectedCells(board, selection)) {
    for (const col of new Set(cells.map((c) => c.col))) {
      for (let row = 0; row < table.rows; row++) {
        const c = cellAt(table, row, col);
        if (c && isSelectableCell(c)) out.add(c.id);
      }
    }
  }
  return [...out];
}

/** `SelectRows`. */
export function selectRows(board: Pick<BoardState, "drawings">, selection: readonly string[]): string[] {
  const out = new Set(selection);
  for (const { table, cells } of selectedCells(board, selection)) {
    for (const row of new Set(cells.map((c) => c.row))) {
      for (let col = 0; col < table.cols; col++) {
        const c = cellAt(table, row, col);
        if (c && isSelectableCell(c)) out.add(c.id);
      }
    }
  }
  return [...out];
}

/** `SelectTable`: the selection is cleared and the tables the cells belong to are what is selected. */
export function selectTables(board: Pick<BoardState, "drawings">, selection: readonly string[]): string[] {
  return selectedCells(board, selection).map(({ table }) => table.id);
}

/**
 * `getCellBlockBounds`: the columns and rows (the maxima exclusive) that `cells` take with their spans, and whether they fill that rectangle exactly -- the condition under
 * which the menu offers Merge Cells (`cellBlockSelection`: two cells or more).
 */
export function blockBounds(cells: readonly BoardTableCell[]): { colMin: number; colMax: number; rowMin: number; rowMax: number; exact: boolean } | null {
  if (cells.length === 0) return null;
  let colMin = Infinity;
  let colMax = 0;
  let rowMin = Infinity;
  let rowMax = 0;
  let area = 0;
  for (const c of cells) {
    colMin = Math.min(colMin, c.col);
    colMax = Math.max(colMax, c.col + c.col_span);
    rowMin = Math.min(rowMin, c.row);
    rowMax = Math.max(rowMax, c.row + c.row_span);
    area += c.col_span * c.row_span;
  }
  return { colMin, colMax, rowMin, rowMax, exact: cells.length === 1 || area === (colMax - colMin) * (rowMax - rowMin) };
}

/** The menu's Merge Cells condition: at least two cells, all of one table, filling a rectangle. */
export function canMerge(cells: readonly BoardTableCell[]): boolean {
  return cells.length >= 2 && (blockBounds(cells)?.exact ?? false);
}

/** The menu's Unmerge Cells condition: one of the cells is merged (`mergedCellsSelection`). */
export function canUnmerge(cells: readonly BoardTableCell[]): boolean {
  return cells.some((c) => c.col_span > 1 || c.row_span > 1);
}

// ------------------------------------------------------------------------------------------------------------------------------------------------- CSV

/** `escapeCSV` of `ExportTableToCSV`: a field with a comma, a quote or a line feed is quoted, its quotes doubled. */
export function escapeCsv(field: string): string {
  if (field.includes(",") || field.includes('"') || field.includes("\n")) return `"${field.replace(/"/g, '""')}"`;
  return field;
}

/**
 * `PCB_EDIT_TABLE_TOOL::ExportTableToCSV`: every row on a line, every cell's shown text (variables resolved, as `GetShownText( false, 0 )` gives it) with commas between,
 * a line feed after each row.
 */
export function tableCsv(t: BoardTable): string {
  let out = "";
  for (let row = 0; row < t.rows; row++) {
    const fields: string[] = [];
    for (let col = 0; col < t.cols; col++) {
      const c = cellAt(t, row, col);
      fields.push(escapeCsv(c ? (c.shown ?? c.text) : ""));
    }
    out += fields.join(",") + "\n";
  }
  return out;
}

/** The file name Export Table to CSV offers: the table's first cell's text or its id, `.csv`. */
export function csvFileName(t: BoardTable): string {
  const first = (t.cells[0]?.shown ?? t.cells[0]?.text ?? "").trim().split("\n")[0] ?? "";
  const safe = first.replace(/[^A-Za-z0-9_-]+/g, "_").replace(/^_+|_+$/g, "");
  return `${safe || t.id}.csv`;
}

/**
 * The table-properties grid shows a table on a back layer from the other side (`IsBackLayer( m_table->GetLayer() )`: `GetCell( row, GetColCount() - 1 - col )`), so
 * its columns run the other way. The index of the cell the grid column `gridCol` stands for.
 */
export function gridColumn(t: Pick<BoardTable, "cols" | "layer">, gridCol: number): number {
  return isBackLayer(t.layer) ? t.cols - 1 - gridCol : gridCol;
}
