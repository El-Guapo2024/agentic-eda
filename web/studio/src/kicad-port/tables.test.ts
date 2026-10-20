import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardTable, BoardTableCell } from "../api/types";
import { blockBounds, canMerge, canUnmerge, cellAddress, cellById, cellHitDistance, cellItemId, csvFileName, escapeCsv, gridColumn, parseCellItemId, polygonDistance, selectColumns, selectRows, selectTables, tableCsv } from "./tables";

/** A `rows` x `cols` table of 10 x 5 mm cells at the origin, as the server sends it. */
function table(rows: number, cols: number, layer = "F.SilkS"): BoardTable {
  const cells: BoardTableCell[] = [];
  for (let row = 0; row < rows; row++) {
    for (let col = 0; col < cols; col++) {
      const x = col * 10_000;
      const y = row * 5_000;
      cells.push({
        id: cellItemId("tbl_a", row, col),
        row,
        col,
        text: `r${row}c${col}`,
        col_span: 1,
        row_span: 1,
        corners: [
          [x, y],
          [x + 10_000, y],
          [x + 10_000, y + 5_000],
          [x, y + 5_000],
        ],
        margins: [1_000, 1_000, 1_000, 1_000],
        h_align: "left",
        v_align: "center",
        size_w: 1_270,
        size_h: 1_270,
        thickness: 0,
        pen: 159,
        bold: false,
        italic: false,
        mirror: false,
        face: null,
        line_spacing: 0,
        color: null,
        lines: [],
      });
    }
  }
  return {
    id: "tbl_a",
    layer,
    x: 0,
    y: 0,
    angle: 0,
    rows,
    cols,
    col_widths: Array(cols).fill(10_000),
    row_heights: Array(rows).fill(5_000),
    border: { external: true, header: true, stroke: { width: 100, style: "solid", color: null } },
    separators: { rows: true, cols: true, stroke: { width: 100, style: "solid", color: null } },
    bbox: [0, 0, cols * 10_000, rows * 5_000],
    borders: [],
    cells,
  };
}

const boardOf = (...tables: BoardTable[]) => ({ drawings: { shapes: [], texts: [], groups: [], dimensions: [], dimension_settings: {} as never, tables } });

test("a cell id names its table, row and column, and only a cell id does", () => {
  assert.equal(cellItemId("tbl_a", 3, 1), "tbl_a:3:1");
  assert.deepEqual(parseCellItemId("tbl_a:3:1"), { table: "tbl_a", row: 3, col: 1 });
  assert.equal(parseCellItemId("tbl_a"), null);
  assert.equal(parseCellItemId("U1"), null);
  const b = boardOf(table(2, 2));
  assert.equal(cellById(b, "tbl_a:1:1")?.cell.text, "r1c1");
  assert.equal(cellById(b, "tbl_a:2:0"), undefined, "no third row");
  assert.equal(cellAddress(2, 1), "B3");
});

test("a point is in a cell, or how far it is from it; a turned cell is a quadrilateral", () => {
  const c = table(1, 1).cells[0]!;
  assert.equal(cellHitDistance(c, 5_000, 2_500), 0);
  assert.equal(cellHitDistance(c, 12_000, 2_500), 2_000);
  assert.equal(Math.round(cellHitDistance(c, 13_000, 9_000)), 5_000);
  // a diamond: (0,-5) (5,0) (0,5) (-5,0)
  const diamond = [
    [0, -5_000],
    [5_000, 0],
    [0, 5_000],
    [-5_000, 0],
  ] as const;
  assert.equal(polygonDistance(diamond, 0, 0), 0);
  assert.equal(polygonDistance(diamond, 4_000, 4_000) > 0, true, "the corner of the bounding box is outside the diamond");
});

test("Select Row(s), Select Column(s) and Select Table do what the selection tool does", () => {
  const b = boardOf(table(3, 3));
  const rows = selectRows(b, ["tbl_a:1:1"]);
  assert.deepEqual(rows.sort(), ["tbl_a:1:0", "tbl_a:1:1", "tbl_a:1:2"]);
  const cols = selectColumns(b, ["tbl_a:0:2"]);
  assert.deepEqual(cols.sort(), ["tbl_a:0:2", "tbl_a:1:2", "tbl_a:2:2"]);
  // the cells already selected stay, the others of their rows join
  assert.deepEqual(selectRows(b, ["tbl_a:0:0", "tbl_a:2:1"]).sort(), ["tbl_a:0:0", "tbl_a:0:1", "tbl_a:0:2", "tbl_a:2:0", "tbl_a:2:1", "tbl_a:2:2"]);
  // the whole table replaces the selection
  assert.deepEqual(selectTables(b, ["tbl_a:0:0", "U1"]), ["tbl_a"]);
  // a cell another one covers is not selected by its row
  const t = table(2, 2);
  t.cells[1]!.col_span = 0;
  t.cells[1]!.row_span = 0;
  assert.deepEqual(selectRows(boardOf(t), ["tbl_a:0:0"]).sort(), ["tbl_a:0:0"]);
});

test("the menu offers Merge Cells for a block of cells and Unmerge Cells for a merged one", () => {
  const t = table(3, 3);
  const pick = (...ids: [number, number][]) => ids.map(([r, c]) => t.cells[r * 3 + c]!);
  assert.equal(canMerge(pick([0, 0])), false, "a single cell");
  assert.equal(canMerge(pick([0, 0], [0, 1])), true);
  assert.equal(canMerge(pick([0, 0], [1, 1])), false, "a diagonal is not a block");
  assert.equal(canMerge(pick([0, 0], [0, 1], [1, 0], [1, 1])), true);
  assert.deepEqual(blockBounds(pick([1, 0], [2, 1])), { colMin: 0, colMax: 2, rowMin: 1, rowMax: 3, exact: false });
  const merged = table(2, 2);
  merged.cells[0]!.col_span = 2;
  assert.equal(canUnmerge(merged.cells), true);
  assert.equal(canUnmerge(t.cells), false);
});

test("Export Table to CSV writes the shown text of each cell, quoting what needs it", () => {
  const t = table(2, 2);
  t.cells[0]!.text = "plain";
  t.cells[1]!.text = "a, b";
  t.cells[2]!.text = 'say "hi"';
  t.cells[3]!.text = "two\nlines";
  t.cells[3]!.shown = "shown\nwrapped";
  assert.equal(tableCsv(t), 'plain,"a, b"\n"say ""hi""","shown\nwrapped"\n');
  assert.equal(escapeCsv("x"), "x");
  assert.equal(escapeCsv(""), "");
  assert.equal(csvFileName(t), "plain.csv");
  assert.equal(csvFileName({ ...t, cells: [{ ...t.cells[0]!, text: "", shown: undefined }] }), "tbl_a.csv");
});

test("a table on a back layer is shown to the Table Properties grid from the other side", () => {
  assert.equal(gridColumn({ cols: 4, layer: "F.SilkS" }, 1), 1);
  assert.equal(gridColumn({ cols: 4, layer: "B.SilkS" }, 1), 2);
});
