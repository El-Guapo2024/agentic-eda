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
import type { BoardStatsReply, BoardTable, BoardTableCell } from "../api/types";
import { isBackLayer, lineThicknessUm, textStyleOf } from "./layerDefaults";
import { activeLayerName } from "./layerPresets";
import type { TableBoardNumbers, TableBorderEdit, TableCellEdit, TableHAlign, TableLineStyleName, TablePatch, TableSeparatorsEdit, TableVAlign } from "../api/tableTypes";

/** The id of a cell as an item of the editor: `<table id>:<row>:<col>` (`eda_model::table::cell_item_id`). */
export function cellItemId(table: string, row: number, col: number): string {
  return `${table}:${row}:${col}`;
}

/** The table, row and column of a cell id, or null for any other id. */
export function parseCellItemId(id: string): { table: string; row: number; col: number } | null {
  const m = /^(.+):(\d+):(\d+)$/.exec(id);
  return m ? { table: m[1]!, row: Number(m[2]), col: Number(m[3]) } : null;
}

/** Anything that holds tables the way the board's state does (and the sheet's: `drawings.tables`). */
export interface HasTables {
  drawings?: { tables?: readonly BoardTable[] } | null;
}

export function tablesOf(board: HasTables | null | undefined): readonly BoardTable[] {
  return board?.drawings?.tables ?? [];
}

/** The id of every cell of every table (`TABLE:ROW:COL`): the items a click on a table picks. */
export function tableCellIds(board: HasTables | null | undefined): string[] {
  return tablesOf(board).flatMap((t) => t.cells.map((c) => c.id));
}

export function tableById(board: HasTables | null | undefined, id: string): BoardTable | undefined {
  return tablesOf(board).find((t) => t.id === id);
}

/** The cell a cell id names, with its table. */
export function cellById(board: HasTables | null | undefined, id: string): { table: BoardTable; cell: BoardTableCell } | undefined {
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
export function selectedCells(board: HasTables, ids: Iterable<string>): { table: BoardTable; cells: BoardTableCell[] }[] {
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
export function selectColumns(board: HasTables, selection: readonly string[]): string[] {
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
export function selectRows(board: HasTables, selection: readonly string[]): string[] {
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
export function selectTables(board: HasTables, selection: readonly string[]): string[] {
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

// ----------------------------------------------------------------------------------------------------------------------------------------- Draw Table

/** The size of the cells and the grid a Draw Table preview makes: how many columns and rows, and each cell's size. */
export interface DrawTableGrid {
  cols: number;
  rows: number;
  cellW: number;
  cellH: number;
}

/** C's `int / int`: the quotient cut toward zero. */
const idiv = (a: number, b: number): number => Math.trunc(a / b);

/** KiCad's `KiROUND`: half away from zero. */
const kiround = (v: number): number => (v < 0 ? -Math.round(-v) : Math.round(v));

/**
 * `DRAWING_TOOL::DrawTable`'s motion handler: from the first click `origin` to the `cursor`, as many columns as hold 15 glyph widths of the layer's text and as many rows as hold 3
 * glyph heights (at least one of each), cells at least 5 glyph widths wide and 3 high, each cell's size rounded to the grid.
 */
export function drawTableGrid(origin: readonly [number, number], cursor: readonly [number, number], fontSize: readonly [number, number], gridSize: readonly [number, number]): DrawTableGrid {
  const requestedX = cursor[0] - origin[0];
  const requestedY = cursor[1] - origin[1];
  const cols = Math.max(1, idiv(requestedX, fontSize[0] * 15));
  const rows = Math.max(1, idiv(requestedY, fontSize[1] * 3));
  let cellW = Math.max(fontSize[0] * 5, idiv(requestedX, cols));
  let cellH = Math.max(fontSize[1] * 3, idiv(requestedY, rows));
  const gx = Math.max(gridSize[0], 1);
  const gy = Math.max(gridSize[1], 1);
  cellW = kiround(cellW / gx) * gx;
  cellH = kiround(cellH / gy) * gy;
  return { cols, rows, cellW, cellH };
}

/** What a Draw Table session works out from the board and the cursor: the layer it draws on, the grid the cursor sizes and the width of the lines the new table starts with. */
export interface DrawTableSession {
  layer: string;
  grid: DrawTableGrid;
  /** `bds.GetLineThickness( layer )`. */
  lineWidth: number;
}

/**
 * `DrawTable`'s numbers for the first corner `origin`, the `cursor` and the editing `gridUm`: the active layer (silkscreen when none is), the layer's text size
 * (`bds.GetTextSize( layer )`, which sizes the cells) and its line width. `textGraphics` is Board Setup's Text & Graphics defaults.
 */
export function drawTableSession(textGraphics: Parameters<typeof textStyleOf>[0], activeLayer: string | null | undefined, origin: readonly [number, number], cursor: readonly [number, number], gridUm: number): DrawTableSession {
  const layer = activeLayerName(activeLayer, "F.SilkS");
  const st = textStyleOf(textGraphics, layer);
  return { layer, grid: drawTableGrid(origin, cursor, [st.widthUm, st.heightUm], [gridUm, gridUm]), lineWidth: lineThicknessUm(textGraphics, layer) };
}

/** The box of the grid a Draw Table preview shows. */
export function drawTableBox(origin: readonly [number, number], g: DrawTableGrid): [number, number, number, number] {
  return [origin[0], origin[1], origin[0] + g.cols * g.cellW, origin[1] + g.rows * g.cellH];
}

// ------------------------------------------------------------------------------------------------------------------------ Table Properties dialog


/** The five line styles of the dialog's combos (`lineTypeNames`); a `default` stroke is shown as the first. */
export const TABLE_LINE_STYLES: readonly { value: TableLineStyleName; label: string }[] = [
  { value: "solid", label: "Solid" },
  { value: "dash", label: "Dashed" },
  { value: "dot", label: "Dotted" },
  { value: "dash_dot", label: "Dash-Dot" },
  { value: "dash_dot_dot", label: "Dash-Dot-Dot" },
];

export function dialogLineStyle(s: TableLineStyleName): TableLineStyleName {
  return s === "default" ? "solid" : s;
}

/** The fields of `DIALOG_TABLE_PROPERTIES` as the dialog holds them. The grid is as the dialog shows it: a table on a back layer has its columns in the other order. */
export interface TableDialogValues {
  layer: string;
  locked: boolean;
  borderExternal: boolean;
  borderHeader: boolean;
  /** um; the field keeps a value while no border is on (the table's own is -1 then), so switching one on has something to start from. */
  borderWidth: number;
  borderStyle: TableLineStyleName;
  rowSeparators: boolean;
  colSeparators: boolean;
  separatorsWidth: number;
  separatorsStyle: TableLineStyleName;
  grid: string[][];
}

/** `TransferDataToWindow`: the dialog's values for `t`. `defaultLineUm` is what a width field starts at when the table has no such stroke (`bds.GetLineThickness( layer )`). */
export function tableDialogValues(t: BoardTable, locked: boolean, defaultLineUm: number): TableDialogValues {
  const grid: string[][] = [];
  for (let row = 0; row < t.rows; row++) {
    const line: string[] = [];
    for (let gcol = 0; gcol < t.cols; gcol++) {
      const c = cellAt(t, row, gridColumn(t, gcol));
      line.push(c && isSelectableCell(c) ? c.text : "");
    }
    grid.push(line);
  }
  const rows = t.separators.rows && t.separators.stroke.width >= 0;
  const cols = t.separators.cols && t.separators.stroke.width >= 0;
  return {
    layer: t.layer,
    locked,
    borderExternal: t.border.external,
    borderHeader: t.border.header,
    borderWidth: t.border.stroke.width >= 0 ? t.border.stroke.width : defaultLineUm,
    borderStyle: dialogLineStyle(t.border.stroke.style),
    rowSeparators: rows,
    colSeparators: cols,
    separatorsWidth: t.separators.stroke.width >= 0 ? t.separators.stroke.width : defaultLineUm,
    separatorsStyle: dialogLineStyle(t.separators.stroke.style),
    grid,
  };
}

/** `onBorderChecked` / `onHeaderChecked`: switching a border or its header on gives a width that was not set the layer's line width (`m_borderWidth.SetValue( defaultLineThickness )`). */
export function widthAfterToggle(current: number, on: boolean, defaultLineUm: number): number {
  return on && current < 0 ? defaultLineUm : current;
}

/** `TransferDataFromWindow`: the patch the values make (the layer and the lock are sent beside it). The grid goes back to the table's own column order. */
export function tablePatchOf(t: Pick<BoardTable, "cols" | "layer">, v: TableDialogValues): TablePatch {
  const border: TableBorderEdit = { external: v.borderExternal, header: v.borderHeader, width_um: Math.round(v.borderWidth), style: v.borderStyle };
  const separators: TableSeparatorsEdit = { rows: v.rowSeparators, cols: v.colSeparators, width_um: Math.round(v.separatorsWidth), style: v.separatorsStyle };
  const texts = v.grid.map((line) => {
    // The grid was laid out for the table's own layer; a layer change in the dialog does not turn the cells round (KiCad reads each cell from the same place it wrote it).
    const out = new Array<string>(t.cols).fill("");
    line.forEach((text, gcol) => {
      out[isBackLayer(t.layer) ? t.cols - 1 - gcol : gcol] = text;
    });
    return out;
  });
  return { border, separators, texts };
}

// ------------------------------------------------------------------------------------------------------------------ Table Cell Properties dialog

/** A field of the cell dialog that several selected cells may disagree on (`INDETERMINATE_STATE`): `null` is "differs", and the dialog leaves it alone. */
export interface CellDialogValues {
  text: string | null;
  bold: boolean;
  italic: boolean;
  face: string | null;
  width: number | null;
  height: number | null;
  /** 0 with `autoThickness` on is KiCad's automatic width; `null` differs. */
  thickness: number | null;
  autoThickness: boolean;
  hAlign: TableHAlign | null;
  vAlign: TableVAlign | null;
  /** left, top, right, bottom. */
  margins: [number | null, number | null, number | null, number | null];
}

/** `GetPenSizeForNormal` / `GetPenSizeForBold`. */
export const penForNormal = (size: number): number => kiround(size / 8);
export const penForBold = (size: number): number => kiround(size / 5);

/** `DIALOG_TABLECELL_PROPERTIES::TransferDataToWindow`: the first cell sets every field, each later cell makes the ones it differs in indeterminate. */
export function cellDialogValues(cells: readonly BoardTableCell[]): CellDialogValues {
  const first = cells[0]!;
  const v: CellDialogValues = {
    text: first.text,
    bold: first.bold,
    italic: first.italic,
    face: first.face ?? "",
    width: first.size_w,
    height: first.size_h,
    thickness: first.thickness,
    autoThickness: first.thickness === 0,
    hAlign: first.h_align,
    vAlign: first.v_align,
    margins: [first.margins[0], first.margins[1], first.margins[2], first.margins[3]],
  };
  for (const c of cells.slice(1)) {
    if (c.text !== v.text) v.text = null;
    if ((c.face ?? "") !== v.face) v.face = null;
    if (c.size_w !== v.width) v.width = null;
    if (c.size_h !== v.height) v.height = null;
    if (c.thickness !== v.thickness) {
      v.thickness = null;
      v.autoThickness = false;
    }
    if (c.h_align !== v.hAlign) v.hAlign = null;
    if (c.v_align !== v.vAlign) v.vAlign = null;
    for (let i = 0; i < 4; i++) if (c.margins[i] !== v.margins[i]) v.margins[i] = null;
  }
  return v;
}

/** `onBoldToggle` / `onTextSize`: the pen the thickness field takes when bold or the size changes while the width is automatic. */
export function autoPenFor(width: number | null, height: number | null, bold: boolean): number | null {
  if (width == null || height == null) return null;
  const size = Math.min(width, height);
  return bold ? penForBold(size) : penForNormal(size);
}

/** `TransferDataFromWindow`: what the dialog sets, leaving out what the cells disagree on. Bold and italic are every cell's from the boxes, as in KiCad. */
export function cellEditOf(v: CellDialogValues): TableCellEdit {
  const e: TableCellEdit = { bold: v.bold, italic: v.italic };
  if (v.text !== null) e.text = v.text;
  if (v.face !== null) e.face = v.face;
  if (v.width !== null) e.width_um = Math.round(v.width);
  if (v.height !== null) e.height_um = Math.round(v.height);
  if (v.autoThickness) e.thickness_um = 0;
  else if (v.thickness !== null) e.thickness_um = Math.round(v.thickness);
  if (v.hAlign !== null) e.h_align = v.hAlign;
  if (v.vAlign !== null) e.v_align = v.vAlign;
  if (v.margins.some((m) => m !== null)) e.margins = v.margins.map((m) => (m === null ? null : Math.round(m))) as [number | null, number | null, number | null, number | null];
  return e;
}

// ------------------------------------------------------------------------------------------------------------------------------ edits keep the selection

/** A cell id's table, row and column as numbers, or null. */
function cellParts(id: string): { table: string; row: number; col: number } | null {
  return parseCellItemId(id);
}

/**
 * A row was put in `table` at index `at` (Add Row Above puts it at the row of the topmost selected cell, Add Row Below one under the bottommost): the cells of the selection from that row
 * on are the same cells, one row further down. (KiCad's selection holds the cells themselves, which the new row only pushes down.)
 */
export function shiftRowsAfterInsert(ids: readonly string[], table: string, at: number): string[] {
  return ids.map((id) => {
    const p = cellParts(id);
    return p && p.table === table && p.row >= at ? cellItemId(table, p.row + 1, p.col) : id;
  });
}

/** The same for a column put in at index `at`. */
export function shiftColsAfterInsert(ids: readonly string[], table: string, at: number): string[] {
  return ids.map((id) => {
    const p = cellParts(id);
    return p && p.table === table && p.col >= at ? cellItemId(table, p.row, p.col + 1) : id;
  });
}

/** The topmost, bottommost, leftmost and rightmost of `cells` (the first of equals): the cell Add Row Above / Below and Add Column Before / After work from. */
export function extremeCell(cells: readonly BoardTableCell[], which: "top" | "bottom" | "left" | "right"): BoardTableCell | undefined {
  let best: BoardTableCell | undefined;
  for (const c of cells) {
    if (!best) best = c;
    else if (which === "top" ? c.row < best.row : which === "bottom" ? c.row > best.row : which === "left" ? c.col < best.col : c.col > best.col) best = c;
  }
  return best;
}

/** The rows the cells are in, sorted. */
export function rowsOf(cells: readonly BoardTableCell[]): number[] {
  return [...new Set(cells.map((c) => c.row))].sort((a, b) => a - b);
}

/** The columns the cells are in, sorted. */
export function colsOf(cells: readonly BoardTableCell[]): number[] {
  return [...new Set(cells.map((c) => c.col))].sort((a, b) => a - b);
}

/**
 * What Place Board Characteristics prints that `ComputeBoardStatistics` measures, from kicad-cli's statistics (`POST /api/board_stats`), in the nanometres of the board file:
 * the thickness, the overall dimensions, the smallest track, clearance and hole, and whether a pad is castellated or press-fit. Null when kicad-cli gave no statistics.
 */
export function boardNumbersOf(stats: BoardStatsReply): TableBoardNumbers | null {
  const b = stats.board;
  if (!stats.ok || !b) return null;
  const nm = (um: number): number => Math.round(um * 1000);
  const count = (title: string): number => (stats.pad_properties ?? []).find((e) => e.title === title)?.qty ?? 0;
  return {
    thickness_nm: nm(b.thickness_um),
    width_nm: nm(b.width_um),
    height_nm: nm(b.height_um),
    min_track_nm: nm(b.min_track_width_um),
    min_clearance_nm: nm(b.min_clearance_um),
    min_drill_nm: nm(b.min_drill_um),
    castellated_pads: count("Castellated:"),
    press_fit_pads: count("Press-fit:"),
  };
}
