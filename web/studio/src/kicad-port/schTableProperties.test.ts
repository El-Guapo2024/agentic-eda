import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardTable, Cmd, Schematic } from "../api/types";
import { schEdit, schFriendlyName, schGrid, schItemOf, schItemsOf } from "./schItemProperties";
import { fixtureTable } from "./tableFixture";

// A sheet's tables in the Properties panel: SCH_TABLE_DESC and SCH_TABLECELL_DESC.
const sheet = (...tables: BoardTable[]): Schematic =>
  ({ lib_symbols: {}, symbols: [], power_symbols: [], wires: [], no_connects: [], labels: [], texts: [], title_block: null, bus_entries: [], junctions: [], lines: [], graphics: [], sheets: [], locked: [], tables }) as unknown as Schematic;

const names = (s: Schematic, ids: string[]): Array<[string, string[]]> => schGrid(s, ids, "mm").groups.map((g) => [g.caption, g.rows.map((r) => r.name)]);
const rowOf = (s: Schematic, ids: string[], name: string) => schGrid(s, ids, "mm").groups.flatMap((g) => g.rows).find((r) => r.name === name);
const edit = (s: Schematic, ids: string[], name: string, value: string | number | boolean): Cmd[] => {
  const plan = schEdit(s, ids, name, value, "mm");
  assert.equal(plan.ok, true, plan.ok ? "" : plan.error);
  return plan.ok ? plan.cmds : [];
};

test("a table and a cell are items of the sheet, named as KiCad names them", () => {
  const s = sheet(fixtureTable(2, 3, ""));
  assert.equal(schItemOf(s, "tbl_a")?.type, "SCH_TABLE");
  assert.equal(schItemOf(s, "tbl_a:1:2")?.type, "SCH_TABLECELL");
  assert.equal(schItemOf(s, "tbl_a:9:9"), null, "a cell the table lacks is no item");
  assert.deepEqual(schItemsOf(s, ["tbl_a:0:0", "tbl_a", "nope"]).map((i) => i.id), ["tbl_a:0:0", "tbl_a"]);
  assert.equal(schItemOf(s, "tbl_a:1:2")?.cell?.text, "r1c2");
  assert.equal(schGrid(s, ["tbl_a"], "mm").caption, "Table");
  assert.equal(schGrid(s, ["tbl_a:0:0"], "mm").caption, "Table Cell");
  assert.equal(schFriendlyName(schItemOf(s, "tbl_a")!), "Table");
  assert.equal(schFriendlyName(schItemOf(s, "tbl_a:0:0")!), "Table Cell");
});

test("a table: Locked of the sheet's item, then Start X / Y and the Table Properties group (SCH_TABLE_DESC)", () => {
  const s = sheet(fixtureTable(2, 3, ""));
  const groups = names(s, ["tbl_a"]);
  assert.deepEqual(groups[0]![1].slice(0, 3), ["Locked", "Start X", "Start Y"]);
  assert.deepEqual(groups.find(([g]) => g === "Table Properties")![1], [
    "External Border",
    "Header Border",
    "Border Width",
    "Border Style",
    "Border Color",
    "Row Separators",
    "Cell Separators",
    "Separators Width",
    "Separators Style",
    "Separators Color",
  ]);
});

test("editing a table sends the sheet's table verb, not the board's", () => {
  const s = sheet(fixtureTable(2, 3, ""));
  const [move] = edit(s, ["tbl_a"], "Start X", 12_000);
  assert.equal(move && (move as { op: string }).op, "sch_table");
  assert.deepEqual(move, { op: "sch_table", verb: "set", id: "tbl_a", patch: { at: { x: 12_000, y: 0 } } });
  assert.deepEqual(edit(s, ["tbl_a"], "Row Separators", false), [{ op: "sch_table", verb: "set", id: "tbl_a", patch: { props: { rows: false } } }]);
  // Locked is the sheet item's: `set_locked` of the table's id
  assert.deepEqual(edit(s, ["tbl_a"], "Locked", true), [{ op: "sch_edit", verb: "set_locked", ids: ["tbl_a"], locked: true }]);
});

test("a cell: the rows of a text box and of the text that a cell keeps (SCH_TABLECELL_DESC)", () => {
  const s = sheet(fixtureTable(2, 3, ""));
  const groups = names(s, ["tbl_a:0:1"]);
  assert.deepEqual(groups.find(([g]) => g === "Text Properties")![1], ["Text", "Italic", "Bold", "Horizontal Justification", "Vertical Justification", "Text Size"]);
  assert.deepEqual(groups.find(([g]) => g === "Margins")![1], ["Margin Left", "Margin Top", "Margin Right", "Margin Bottom"]);
  assert.deepEqual(groups.find(([g]) => g === "Table")![1], ["Column Width", "Row Height"]);
  assert.deepEqual(groups.find(([g]) => g === "Cell Properties")![1], ["Background Fill", "Background Fill Color"]);
  const all = groups.flatMap(([, rows]) => rows);
  for (const masked of ["Locked", "Start X", "Width", "Height", "Thickness", "Orientation", "Mirrored", "Visible", "Hyperlink", "Font"]) assert.ok(!all.includes(masked), `${masked} is masked`);
});

test("editing a cell sends one edit_cells of its address, and several cells one batch of them", () => {
  const s = sheet(fixtureTable(2, 3, ""));
  assert.deepEqual(edit(s, ["tbl_a:1:2"], "Text", "hello"), [{ op: "sch_table", verb: "edit_cells", id: "tbl_a", cells: [[1, 2]], edit: { text: "hello" } }]);
  assert.deepEqual(edit(s, ["tbl_a:1:2"], "Text", "r1c2"), [], "no change, no command");
  assert.deepEqual(edit(s, ["tbl_a:0:0"], "Bold", true), [{ op: "sch_table", verb: "edit_cells", id: "tbl_a", cells: [[0, 0]], edit: { bold: true } }]);
  assert.deepEqual(edit(s, ["tbl_a:0:0"], "Margin Top", 500), [{ op: "sch_table", verb: "edit_cells", id: "tbl_a", cells: [[0, 0]], edit: { margins: [null, 500, null, null] } }]);
  assert.deepEqual(edit(s, ["tbl_a:0:1"], "Column Width", 12_000), [{ op: "sch_table", verb: "edit_cells", id: "tbl_a", cells: [[0, 1]], edit: { col_width_nm: 12_000 } }]);
  // `SCH_TEXTBOX::SetSchTextSize`: one number for both dimensions of the glyph
  assert.deepEqual(edit(s, ["tbl_a:0:0"], "Text Size", 1_000_000), [{ op: "sch_table", verb: "edit_cells", id: "tbl_a", cells: [[0, 0]], edit: { width_nm: 1_000_000, height_nm: 1_000_000 } }]);
  const two = edit(s, ["tbl_a:0:0", "tbl_a:0:1"], "Italic", true);
  assert.equal(two.length, 2);
  // the text size must not make the text disappear
  const refused = schEdit(s, ["tbl_a:0:0"], "Text Size", 5, "mm");
  assert.equal(refused.ok, false);
});

test("the background rows: filling takes the colour the cell has, a colour makes the fill, an empty one takes it away", () => {
  const t = fixtureTable(1, 2, "");
  const s = sheet({ ...t, cells: [{ ...t.cells[0]!, fill: "color", fill_color: [255, 240, 200, 255] }, t.cells[1]!] });
  assert.equal(rowOf(s, ["tbl_a:0:0"], "Background Fill")?.value, true);
  assert.equal(rowOf(s, ["tbl_a:0:1"], "Background Fill")?.value, false);
  assert.equal(rowOf(s, ["tbl_a:0:0"], "Background Fill Color")?.value, "#fff0c8");
  assert.deepEqual(edit(s, ["tbl_a:0:1"], "Background Fill", true), [{ op: "sch_table", verb: "edit_cells", id: "tbl_a", cells: [[0, 1]], edit: { fill: "outline" } }]);
  assert.deepEqual(edit(s, ["tbl_a:0:0"], "Background Fill", false), [{ op: "sch_table", verb: "edit_cells", id: "tbl_a", cells: [[0, 0]], edit: { fill: "none" } }]);
  assert.deepEqual(edit(s, ["tbl_a:0:1"], "Background Fill Color", "#00ff00"), [{ op: "sch_table", verb: "edit_cells", id: "tbl_a", cells: [[0, 1]], edit: { fill: "color", fill_color: { r: 0, g: 255, b: 0, a: 255 } } }]);
  assert.deepEqual(edit(s, ["tbl_a:0:0"], "Background Fill Color", ""), [{ op: "sch_table", verb: "edit_cells", id: "tbl_a", cells: [[0, 0]], edit: { fill: "none" } }]);
});
