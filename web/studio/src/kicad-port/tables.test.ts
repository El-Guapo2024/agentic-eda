import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardStatsReply, BoardTable } from "../api/types";
import { fixtureTable as table } from "./tableFixture";
import { allItemIds } from "../components/canvas/clipboard";
import { extremeCell, rowsOf, colsOf, shiftColsAfterInsert, shiftRowsAfterInsert, autoPenFor, blockBounds, boardNumbersOf, tableCellIds, canMerge, canUnmerge, cellAddress, cellById, cellDialogValues, cellEditOf, cellHitDistance, cellItemId, csvFileName, drawTableBox, drawTableGrid, escapeCsv, gridColumn, parseCellItemId, polygonDistance, selectColumns, selectRows, selectTables, tableCsv, tableDialogValues, tablePatchOf, widthAfterToggle } from "./tables";


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

test("Draw Table sizes its grid from the cursor as the tool's motion handler does", () => {
  // 1.27 mm glyphs, a 1.27 mm grid: a column per 15 glyph widths (19.05 mm), a row per 3 glyph heights (3.81 mm)
  const g = drawTableGrid([0, 0], [60_000, 20_000], [1_270, 1_270], [1_270, 1_270]);
  assert.deepEqual([g.cols, g.rows], [3, 5]);
  // the cells are at least 5 glyphs wide and 3 high, and land on the grid
  assert.equal(g.cellW % 1_270, 0);
  assert.equal(g.cellH % 1_270, 0);
  assert.ok(g.cellW >= 1_270 * 5 && g.cellH >= 1_270 * 3);
  // a cursor up and to the left of the first click is a table of one cell of the smallest size
  const back = drawTableGrid([10_000, 10_000], [0, 0], [1_270, 1_270], [1_270, 1_270]);
  assert.deepEqual(back, { cols: 1, rows: 1, cellW: 6_350, cellH: 3_810 });
  assert.deepEqual(drawTableBox([100, 200], { cols: 2, rows: 3, cellW: 10, cellH: 20 }), [100, 200, 120, 260]);
});

test("the Table Properties dialog starts from the table and sends back a patch in the table's own column order", () => {
  const t = table(2, 3, "B.SilkS");
  t.border.stroke.width = -1;
  t.border.external = false;
  t.border.header = false;
  const v = tableDialogValues(t, false, 150);
  assert.equal(v.borderWidth, 150, "a border the table does not have starts at the layer's line width");
  assert.deepEqual(v.grid[0], ["r0c2", "r0c1", "r0c0"], "a back-layer table is shown from the other side");
  assert.equal(v.rowSeparators && v.colSeparators, true);
  v.grid[0]![0] = "changed";
  v.borderExternal = true;
  v.borderWidth = 200.4;
  const patch = tablePatchOf(t, v);
  assert.deepEqual(patch.texts![0], ["r0c0", "r0c1", "changed"]);
  assert.deepEqual(patch.border, { external: true, header: false, width_um: 200, style: "solid" });
  assert.equal(widthAfterToggle(-1, true, 150), 150);
  assert.equal(widthAfterToggle(300, true, 150), 300);
  assert.equal(widthAfterToggle(-1, false, 150), -1);
});

test("the Table Cell Properties dialog leaves alone what the selected cells disagree on", () => {
  const t = table(1, 3);
  t.cells[1]!.bold = true;
  t.cells[1]!.size_w = 2_000;
  t.cells[2]!.h_align = "right";
  t.cells[2]!.margins = [1_000, 500, 1_000, 1_000];
  const v = cellDialogValues(t.cells);
  assert.equal(v.text, null);
  assert.equal(v.width, null);
  assert.equal(v.height, 1_270);
  assert.equal(v.hAlign, null);
  assert.equal(v.autoThickness, true);
  assert.deepEqual(v.margins, [1_000, null, 1_000, 1_000]);
  const e = cellEditOf(v);
  assert.deepEqual(e, { bold: false, italic: false, face: "", height_um: 1_270, thickness_um: 0, v_align: "center", margins: [1_000, null, 1_000, 1_000] });
  // one cell: everything is set
  const one = cellEditOf(cellDialogValues([t.cells[0]!]));
  assert.equal(one.text, "r0c0");
  assert.equal(one.h_align, "left");
  assert.equal(autoPenFor(1_270, 1_270, false), 159);
  assert.equal(autoPenFor(1_270, 2_000, true), 254);
  assert.equal(autoPenFor(null, 2_000, true), null);
});

test("a row or column put into a table pushes the selected cells it is before one step on", () => {
  assert.deepEqual(shiftRowsAfterInsert(["tbl_a:0:1", "tbl_a:2:0", "tbl_b:2:0", "U1"], "tbl_a", 1), ["tbl_a:0:1", "tbl_a:3:0", "tbl_b:2:0", "U1"]);
  assert.deepEqual(shiftColsAfterInsert(["tbl_a:0:1", "tbl_a:2:0"], "tbl_a", 1), ["tbl_a:0:2", "tbl_a:2:0"]);
});

test("the edge cells of a selection and the rows and columns it covers", () => {
  const t = table(3, 3);
  const pick = (...rc: [number, number][]) => rc.map(([r, c]) => t.cells[r * 3 + c]!);
  const sel = pick([2, 0], [0, 2], [1, 1]);
  assert.equal(extremeCell(sel, "top")!.id, "tbl_a:0:2");
  assert.equal(extremeCell(sel, "bottom")!.id, "tbl_a:2:0");
  assert.equal(extremeCell(sel, "left")!.id, "tbl_a:2:0");
  assert.equal(extremeCell(sel, "right")!.id, "tbl_a:0:2");
  assert.deepEqual(rowsOf(sel), [0, 1, 2]);
  assert.deepEqual(colsOf(pick([0, 2], [1, 2])), [2]);
  assert.equal(extremeCell([], "top"), undefined);
});

test("a table is an item of the board (it is picked up by a Paste or a Duplicate), and every cell id of its is listed so a refresh keeps the selected cells", () => {
  const second = { ...table(1, 1), id: "tbl_b", cells: table(1, 1).cells.map((c) => ({ ...c, id: cellItemId("tbl_b", c.row, c.col) })) };
  const b = boardOf(table(2, 2), second);
  const items = allItemIds({ ...b, parts: [] } as never);
  assert.ok(items.has("tbl_a") && items.has("tbl_b"));
  assert.ok(!items.has("tbl_a:0:0"), "a cell is not an item of its own to paste");
  assert.deepEqual(tableCellIds(b), ["tbl_a:0:0", "tbl_a:0:1", "tbl_a:1:0", "tbl_a:1:1", "tbl_b:0:0"]);
  assert.deepEqual(tableCellIds(null), []);
});

test("the board's numbers for Place Board Characteristics are kicad-cli's statistics in nanometres, and there are none without them", () => {
  const stats = {
    ok: true,
    board: { has_outline: true, width_um: 26385, height_um: 25181, area_um2: 0, front_copper_area_um2: 0, back_copper_area_um2: 0, front_courtyard_area_um2: 0, back_courtyard_area_um2: 0, front_density_pct: 0, back_density_pct: 0, min_track_width_um: 200, min_clearance_um: 203.5, min_drill_um: 300, thickness_um: 1600 },
    pad_properties: [
      { title: "Castellated:", qty: 0 },
      { title: "Press-fit:", qty: 3 },
    ],
  } as BoardStatsReply;
  assert.deepEqual(boardNumbersOf(stats), { thickness_nm: 1_600_000, width_nm: 26_385_000, height_nm: 25_181_000, min_track_nm: 200_000, min_clearance_nm: 203_500, min_drill_nm: 300_000, castellated_pads: 0, press_fit_pads: 3 });
  assert.equal(boardNumbersOf({ ok: false, message: "no kicad-cli" } as BoardStatsReply), null);
  assert.equal(boardNumbersOf({ ok: true } as BoardStatsReply), null);
  assert.equal(boardNumbersOf({ ...stats, pad_properties: undefined })!.press_fit_pads, 0, "no pad property entries is none");
});
