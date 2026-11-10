import { test } from "node:test";
import assert from "node:assert/strict";
import { fixtureTable as table } from "./tableFixture";
import { cellsBetween, cellUnder, clipboardHasTable, hasTableCells, onlyCells, shiftClickCells, shiftTable } from "./tableCells";
import { cellColorEditOf, cellDialogValues, fillSwatchOf } from "./tables";
import { previewIds, shiftPreview, withPaste, type SchPastePreview } from "./schClipboard";

const none = { additive: false, subtractive: false, exclusiveOr: false };

test("a drag across cells selects the block it touches, as selectTableCells does", () => {
  const t = table(3, 3); // 10 x 5 mm cells from the origin
  // from inside (0, 0) to inside (1, 1): the four cells it touches
  assert.deepEqual(cellsBetween(t, [2_000_000, 1_000_000], [12_000_000, 6_000_000], [], none), ["tbl_a:0:0", "tbl_a:0:1", "tbl_a:1:0", "tbl_a:1:1"]);
  // the other way round is the same rectangle
  assert.deepEqual(cellsBetween(t, [12_000_000, 6_000_000], [2_000_000, 1_000_000], [], none).length, 4);
  // an edge counts (`BOX2I::Intersects`): a drag to the line between two columns touches both
  assert.deepEqual(cellsBetween(t, [1_000_000, 1_000_000], [10_000_000, 1_000_000], [], none), ["tbl_a:0:0", "tbl_a:0:1"]);
  // with no modifier, a cell selected before and not touched now is let go; the other items of the selection stay
  assert.deepEqual(cellsBetween(t, [1_000_000, 1_000_000], [2_000_000, 2_000_000], ["U1", "tbl_a:2:2"], none), ["U1", "tbl_a:0:0"]);
  // Shift (additive) keeps it; Ctrl (exclusive or) toggles what it touches; Ctrl+Shift (subtractive) takes what it touches away
  assert.deepEqual(cellsBetween(t, [1_000_000, 1_000_000], [2_000_000, 2_000_000], ["tbl_a:2:2"], { ...none, additive: true }), ["tbl_a:2:2", "tbl_a:0:0"]);
  assert.deepEqual(cellsBetween(t, [1_000_000, 1_000_000], [12_000_000, 2_000_000], ["tbl_a:0:0"], { ...none, exclusiveOr: true }), ["tbl_a:0:1"]);
  assert.deepEqual(cellsBetween(t, [1_000_000, 1_000_000], [12_000_000, 2_000_000], ["tbl_a:0:0", "tbl_a:2:2"], { ...none, subtractive: true }), ["tbl_a:2:2"]);
  // a cell another one covers is no item to select
  const merged = table(1, 2);
  merged.cells[0]!.col_span = 2;
  merged.cells[1]!.col_span = 0;
  assert.deepEqual(cellsBetween(merged, [1_000_000, 1_000_000], [15_000_000, 2_000_000], [], none), ["tbl_a:0:0"]);
});

test("a drag selects cells only from a cell, and on the board only when nothing or only cells are selected", () => {
  const t = table(2, 2);
  assert.equal(cellUnder([t], 15_000_000, 7_000_000, 0)?.cell.id, "tbl_a:1:1");
  assert.equal(cellUnder([t], 25_000_000, 7_000_000, 0), null);
  assert.equal(cellUnder([t], 20_400_000, 7_000_000, 500_000)?.cell.id, "tbl_a:1:1", "within the tolerance of the collector");
  assert.equal(onlyCells([], [t]), true);
  assert.equal(onlyCells(["tbl_a:0:0", "tbl_a:1:1"], [t]), true);
  assert.equal(onlyCells(["tbl_a:0:0", "U1"], [t]), false);
  assert.equal(onlyCells(["tbl_a"], [t]), false, "the table itself is not a cell");
  assert.equal(hasTableCells(["U1", "tbl_a:0:1"], [t]), true);
  assert.equal(hasTableCells(["U1", "tbl_a"], [t]), false);
});

test("Paste pastes into cells only when the clipboard has a table among its top-level items", () => {
  const board = '(kicad_pcb (version 20241229) (generator "pcbnew")\n\t(table (column_count 1) (cells (table_cell "a")))\n)';
  assert.equal(clipboardHasTable(board, 1), true);
  assert.equal(clipboardHasTable('(kicad_pcb (version 1) (footprint "X" (table (column_count 1))))', 1), false, "a footprint's own table is not one of the board's drawings");
  assert.equal(clipboardHasTable('(kicad_pcb (version 1) (gr_text "(table (x))" (at 0 0)))', 1), false, "a text that looks like one is text");
  assert.equal(clipboardHasTable('(lib_symbols)\n(table\n\t(column_count 2))', 0), true);
  assert.equal(clipboardHasTable("(text \"hello\")", 0), false);
});

test("a sheet's paste preview carries its tables with the rest", () => {
  const t = table(1, 2);
  t.id = "tbl_new";
  const preview: SchPastePreview = { symbols: [], power_symbols: [], wires: [], labels: [], texts: [], no_connects: [], bus_entries: [], junctions: [], lines: [], graphics: [], lib_symbols: {}, tables: [t] };
  const moved = shiftPreview(preview, 1_000_000, 2_000_000).tables![0]!;
  assert.deepEqual([moved.x, moved.y, moved.bbox[0], moved.cells[1]!.corners[0]], [1_000_000, 2_000_000, t.bbox[0] + 1_000_000, [11_000_000, 2_000_000]]);
  assert.deepEqual(shiftTable(t, 0, 0), t);
  assert.ok(previewIds(preview).includes("tbl_new"));
  const sheet = withPaste({ symbols: [], power_symbols: [], wires: [], labels: [], texts: [], sheets: [], no_connects: [], bus_entries: [], lib_symbols: {} } as never, preview, 0, 0);
  assert.equal(sheet.tables?.length, 1);
});

test("the sheet dialog's colours: a swatch per field, mixed values left alone, the fill off when the swatch is the default", () => {
  const t = table(1, 3);
  t.cells[0]!.color = [200, 0, 0, 255];
  t.cells[1]!.color = [200, 0, 0, 255];
  t.cells[0]!.fill = "color";
  t.cells[0]!.fill_color = [0, 0, 200, 255];
  t.cells[2]!.fill = "background";
  const one = cellDialogValues([t.cells[0]!]);
  assert.deepEqual([one.textColor, one.fillColor], [{ r: 200, g: 0, b: 0, a: 255 }, { r: 0, g: 0, b: 200, a: 255 }]);
  const two = cellDialogValues([t.cells[0]!, t.cells[1]!]);
  assert.deepEqual([two.textColor, two.fillColor], [{ r: 200, g: 0, b: 0, a: 255 }, "mixed"]);
  assert.deepEqual(cellColorEditOf(two), { text_color: { r: 200, g: 0, b: 0, a: 255 } }, "a mixed fill is left as the cells have it");
  // `IsSolidFill() ? GetFillColor() : UNSPECIFIED`: a background fill with no colour of its own shows the default, and OK turns it off, as in KiCad
  assert.equal(fillSwatchOf(t.cells[2]!), null);
  assert.deepEqual(cellColorEditOf(cellDialogValues([t.cells[2]!])), { text_color: { r: 0, g: 0, b: 0, a: 0 }, fill: "none" });
  assert.deepEqual(cellColorEditOf({ textColor: "mixed", fillColor: { r: 1, g: 2, b: 3, a: 255 } }), { fill: "color", fill_color: { r: 1, g: 2, b: 3, a: 255 } });
});

test("Shift+click on a second cell selects the cells between it and the first, as the sheet's selection tool does (UI-LEFT-2 SS18)", () => {
  const t = table(3, 3);
  // one cell selected, Shift+click two down and right: the block between their centres, in the table's order; the start is kept
  const a = shiftClickCells([t], ["tbl_a:0:0"], "tbl_a:2:1", null);
  assert.deepEqual(a, { refs: ["tbl_a:0:0", "tbl_a:0:1", "tbl_a:1:0", "tbl_a:1:1", "tbl_a:2:0", "tbl_a:2:1"], previousFirst: "tbl_a:0:0" });
  // a second Shift+click starts from the same cell (`m_previous_first_cell`), not from the first of the new selection
  assert.deepEqual(shiftClickCells([t], a!.refs, "tbl_a:0:2", a!.previousFirst)?.refs, ["tbl_a:0:0", "tbl_a:0:1", "tbl_a:0:2"]);
  // up and to the left works the same
  assert.deepEqual(shiftClickCells([t], ["tbl_a:2:2"], "tbl_a:1:1", null)?.refs, ["tbl_a:1:1", "tbl_a:1:2", "tbl_a:2:1", "tbl_a:2:2"]);
  // another item among the selection: nothing changes
  assert.deepEqual(shiftClickCells([t], ["tbl_a:0:0", "U1"], "tbl_a:1:1", null)?.refs, ["tbl_a:0:0", "U1"]);
  // the first item no cell, or the click on no cell: not this click
  assert.equal(shiftClickCells([t], ["U1", "tbl_a:0:0"], "tbl_a:1:1", null), null);
  assert.equal(shiftClickCells([t], ["tbl_a:0:0"], "U1", null), null);
  assert.equal(shiftClickCells([t], [], "tbl_a:0:0", null), null);
});
