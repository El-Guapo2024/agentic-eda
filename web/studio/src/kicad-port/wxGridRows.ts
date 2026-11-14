// A `WX_GRID` in `wxGrid::wxGridSelectRows` mode (common/widgets/wx_grid.cpp at KiCad 8303b2ad), as the Footprint Editor's Footprint Defaults and User Layer Names
// grids are made (`PANEL_FP_EDITOR_FIELD_DEFAULTS`, `PANEL_FP_USER_LAYER_NAMES`: `SetSelectionMode( wxGrid::wxGridSelectRows )`, no row labels): a click on any cell
// selects its whole row and puts the grid cursor there, Ctrl (Cmd) adds a row to the selection or takes it out, Shift selects every row from the cursor's to the
// one clicked; `OnAddRow` puts the cursor on the new row (and edits its cell), `OnDeleteRows` deletes the selected rows -- else the cursor's -- and moves the cursor
// to the row above the lowest one deleted (UI-LEFT-3 F3-6; the rows were picked by a check box).
//
// Pure: components/FpDefaultsPages.tsx holds the state.

export interface GridRows {
  /** The rows selected (`GetSelectedRows`, the selected blocks' rows). */
  selected: ReadonlySet<number>;
  /** The grid cursor's row (`GetGridCursorRow`), -1 for none. */
  cursor: number;
}

export const NO_GRID_ROWS: GridRows = { selected: new Set(), cursor: -1 };

/** A click on a cell of `row` (`wxGridSelectRows`): the row alone selected; with Ctrl the row toggled in the selection; with Shift the rows from the cursor's. */
export function clickGridRow(g: GridRows, row: number, mods: { ctrl?: boolean; shift?: boolean } = {}): GridRows {
  if (mods.shift && g.cursor >= 0) {
    const [a, b] = g.cursor <= row ? [g.cursor, row] : [row, g.cursor];
    const selected = new Set(mods.ctrl ? g.selected : []);
    for (let r = a; r <= b; r++) selected.add(r);
    // (Shift extends the block: the cursor stays where the block began)
    return { selected, cursor: g.cursor };
  }
  if (mods.ctrl) {
    const selected = new Set(g.selected);
    if (selected.has(row)) selected.delete(row);
    else selected.add(row);
    return { selected, cursor: row };
  }
  return { selected: new Set([row]), cursor: row };
}

/** `WX_GRID::OnAddRow`: the row the adder appended (`row`) has the cursor (`SetGridCursor( row, editCol )`); the selection is cleared by the cursor's move. */
export function addedGridRow(row: number): GridRows {
  return { selected: new Set(), cursor: row };
}

/**
 * `WX_GRID::OnDeleteRows`: the rows to delete, highest first (`selectedRows.Sort( second - first )`) -- the selected ones, else the cursor's row; none when there is
 * neither -- and the grid as it is after (`nextSelRow = selectedRows.back() - 1`: the cursor on the row above the lowest deleted when there is one, else where it was,
 * clamped to the rows left; nothing selected).
 */
export function deleteGridRows(g: GridRows, rowCount: number): { rows: number[]; after: GridRows } {
  const rows = [...g.selected].filter((r) => r >= 0 && r < rowCount);
  if (rows.length === 0 && g.cursor >= 0 && g.cursor < rowCount) rows.push(g.cursor);
  if (rows.length === 0) return { rows: [], after: g };
  rows.sort((a, b) => b - a);
  const next = rows[rows.length - 1]! - 1;
  const left = rowCount - rows.length;
  const cursor = next >= 0 ? next : left > 0 ? Math.min(Math.max(g.cursor, 0), left - 1) : -1;
  return { rows, after: { selected: new Set(), cursor } };
}

/** The rows of `items` with `rows` (indices) taken out. */
export function withoutRows<T>(items: readonly T[], rows: readonly number[]): T[] {
  const gone = new Set(rows);
  return items.filter((_, i) => !gone.has(i));
}
