import { test } from "node:test";
import assert from "node:assert/strict";
import { addedGridRow, clickGridRow, deleteGridRows, NO_GRID_ROWS, withoutRows } from "./wxGridRows";

test("wxGridSelectRows: a click selects the row alone, Ctrl toggles a row, Shift selects from the cursor's row", () => {
  let g = clickGridRow(NO_GRID_ROWS, 2);
  assert.deepEqual([[...g.selected], g.cursor], [[2], 2]);
  g = clickGridRow(g, 4, { ctrl: true });
  assert.deepEqual([[...g.selected].sort(), g.cursor], [[2, 4], 4]);
  g = clickGridRow(g, 2, { ctrl: true });
  assert.deepEqual([...g.selected], [4]);
  g = clickGridRow(clickGridRow(NO_GRID_ROWS, 1), 3, { shift: true });
  assert.deepEqual([[...g.selected].sort(), g.cursor], [[1, 2, 3], 1]);
  g = clickGridRow(g, 0);
  assert.deepEqual([...g.selected], [0], "a plain click selects one row again");
});

test("WX_GRID::OnDeleteRows: the selected rows (highest first), else the cursor's; the cursor to the row above the lowest deleted", () => {
  const items = ["a", "b", "c", "d"];
  let d = deleteGridRows({ selected: new Set([1, 3]), cursor: 3 }, items.length);
  assert.deepEqual(d.rows, [3, 1]);
  assert.deepEqual(withoutRows(items, d.rows), ["a", "c"]);
  assert.deepEqual([[...d.after.selected], d.after.cursor], [[], 0]);
  d = deleteGridRows({ selected: new Set(), cursor: 2 }, items.length);
  assert.deepEqual(d.rows, [2], "no selection: the cursor's row");
  d = deleteGridRows({ selected: new Set([0]), cursor: 0 }, items.length);
  assert.equal(d.after.cursor, 0, "the first row deleted: the cursor stays on the first row left");
  assert.deepEqual(deleteGridRows(NO_GRID_ROWS, 3).rows, [], "nothing selected and no cursor: nothing goes");
  assert.equal(deleteGridRows({ selected: new Set([0]), cursor: 0 }, 1).after.cursor, -1, "the last row gone: no cursor");
});

test("WX_GRID::OnAddRow: the new row has the cursor", () => {
  assert.deepEqual([[...addedGridRow(5).selected], addedGridRow(5).cursor], [[], 5]);
});
