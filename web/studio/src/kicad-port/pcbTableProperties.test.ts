import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, BoardTable, Cmd } from "../api/types";
import { pcbEdit, pcbFriendlyName, pcbGrid, pcbItemOf, pcbItemsOf, TYPE_NAMES, typeName } from "./pcbProperties";
import { fixtureTable } from "./tableFixture";

const board = (...tables: BoardTable[]): BoardState =>
  ({
    name: "t",
    dir: "/t",
    outline: null,
    layers: ["F.Cu", "B.Cu"],
    snap: 100_000,
    parts: [],
    rules: [],
    routing: null,
    drawings: { shapes: [], texts: [], groups: [], dimensions: [], dimension_settings: {}, tables },
    checks: [],
    locked: [],
    activity: [],
    job: "idle",
  }) as unknown as BoardState;

const names = (b: BoardState, ids: string[]): Array<[string, string[]]> => pcbGrid(b, ids, "mm").groups.map((g) => [g.caption, g.rows.map((r) => r.name)]);
const rowOf = (b: BoardState, ids: string[], name: string) => pcbGrid(b, ids, "mm").groups.flatMap((g) => g.rows).find((r) => r.name === name);
const edit = (b: BoardState, ids: string[], name: string, value: string | number | boolean): Cmd[] => {
  const plan = pcbEdit(b, ids, name, value, "mm");
  assert.equal(plan.ok, true, plan.ok ? "" : plan.error);
  return plan.ok ? plan.cmds : [];
};

test("a table and a cell are items of their own, named as KiCad names them", () => {
  const b = board(fixtureTable(2, 3));
  assert.equal(pcbItemOf(b, "tbl_a")?.type, "PCB_TABLE");
  assert.equal(pcbItemOf(b, "tbl_a:1:2")?.type, "PCB_TABLECELL");
  assert.equal(pcbItemOf(b, "tbl_a:9:9"), null, "a cell the table lacks is no item");
  assert.deepEqual(pcbItemsOf(b, ["tbl_a:0:0", "tbl_a", "nope"]).map((i) => i.id), ["tbl_a:0:0", "tbl_a"]);
  assert.equal(pcbItemOf(b, "tbl_a:1:2")?.cell?.text, "r1c2");
  assert.equal(pcbGrid(b, ["tbl_a"], "mm").caption, "Table");
  assert.equal(pcbGrid(b, ["tbl_a:0:0"], "mm").caption, "Table Cell");
  assert.equal(pcbFriendlyName(pcbItemOf(b, "tbl_a")!), "Table");
  assert.equal(typeName(pcbItemOf(b, "tbl_a:0:0")!), "Table Cell");
  assert.ok(TYPE_NAMES.includes("Table") && TYPE_NAMES.includes("Table Cell"));
});

test("a big selection resolves tables and cells through the index too", () => {
  const b = board(fixtureTable(4, 4));
  const ids = ["tbl_a", ...Array.from({ length: 16_000 }, (_, k) => `tbl_a:${Math.floor(k / 4)}:${k % 4}`)];
  assert.equal(pcbItemsOf(b, ids).length, 17);
  assert.equal(pcbItemsOf(b, ids)[5]!.type, "PCB_TABLECELL");
});

test("a table: the board item's rows, then Start X / Y and the Table Properties group (PCB_TABLE_DESC)", () => {
  const b = board(fixtureTable(2, 3));
  const groups = names(b, ["tbl_a"]);
  assert.deepEqual(groups[0], ["Basic Properties", ["Position X", "Position Y", "Layer", "Locked", "Start X", "Start Y"]]);
  assert.deepEqual(groups[1], [
    "Table Properties",
    ["External Border", "Header Border", "Border Width", "Border Style", "Border Color", "Row Separators", "Cell Separators", "Separators Width", "Separators Style", "Separators Color"],
  ]);
  assert.equal(rowOf(b, ["tbl_a"], "Layer")!.value, "F.SilkS");
  assert.equal(rowOf(b, ["tbl_a"], "External Border")!.value, true);
  assert.equal(rowOf(b, ["tbl_a"], "Border Width")!.value, 100_000);
  assert.equal(rowOf(b, ["tbl_a"], "Border Style")!.value, "solid");
  assert.equal(rowOf(b, ["tbl_a"], "Border Color")!.value, "", "no colour is the layer's own");
});

test("a table's default stroke style shows as solid, as GetBorderStyle does", () => {
  const t = fixtureTable(1, 1);
  t.border.stroke.style = "default";
  const b = board(t);
  assert.equal(rowOf(b, ["tbl_a"], "Border Style")!.value, "solid");
  assert.deepEqual(edit(b, ["tbl_a"], "Border Style", "solid"), [], "the value it already shows");
});

test("the table rows edit one field each, with the verb that sets it", () => {
  const b = board(fixtureTable(2, 3));
  assert.deepEqual(edit(b, ["tbl_a"], "External Border", false), [{ op: "table", verb: "set", id: "tbl_a", patch: { props: { external: false } } }]);
  assert.deepEqual(edit(b, ["tbl_a"], "Header Border", false), [{ op: "table", verb: "set", id: "tbl_a", patch: { props: { header: false } } }]);
  assert.deepEqual(edit(b, ["tbl_a"], "Row Separators", false), [{ op: "table", verb: "set", id: "tbl_a", patch: { props: { rows: false } } }]);
  assert.deepEqual(edit(b, ["tbl_a"], "Cell Separators", false), [{ op: "table", verb: "set", id: "tbl_a", patch: { props: { cols: false } } }]);
  assert.deepEqual(edit(b, ["tbl_a"], "Border Width", 250_000), [{ op: "table", verb: "set", id: "tbl_a", patch: { props: { border_width_nm: 250_000 } } }]);
  assert.deepEqual(edit(b, ["tbl_a"], "Separators Width", 80_000), [{ op: "table", verb: "set", id: "tbl_a", patch: { props: { separators_width_nm: 80_000 } } }]);
  assert.deepEqual(edit(b, ["tbl_a"], "Separators Style", "dash_dot"), [{ op: "table", verb: "set", id: "tbl_a", patch: { props: { separators_style: "dash_dot" } } }]);
  assert.deepEqual(edit(b, ["tbl_a"], "Border Color", "#ff0000"), [{ op: "table", verb: "set", id: "tbl_a", patch: { props: { border_color: { r: 255, g: 0, b: 0, a: 255 } } } }]);
  assert.deepEqual(edit(b, ["tbl_a"], "External Border", true), [], "already on");
  assert.deepEqual(edit(b, ["tbl_a"], "Start X", 5_000_000), [{ op: "table", verb: "set", id: "tbl_a", patch: { at: { x: 5_000_000, y: 0 } } }]);
  assert.deepEqual(edit(b, ["tbl_a"], "Start Y", 7_000_000), [{ op: "table", verb: "set", id: "tbl_a", patch: { at: { x: 0, y: 7_000_000 } } }]);
  assert.deepEqual(edit(b, ["tbl_a"], "Layer", "F.Fab"), [{ op: "table", verb: "set", id: "tbl_a", layer: "F.Fab" }]);
});

test("a table is moved and locked by the board item's rows, with the generic verbs", () => {
  const b = board(fixtureTable(1, 1));
  assert.deepEqual(edit(b, ["tbl_a"], "Position X", 2_000_000), [{ op: "move_items", ids: ["tbl_a"], dx: 2_000_000, dy: 0 }]);
  assert.deepEqual(edit(b, ["tbl_a"], "Locked", true), [{ op: "set_locked", ids: ["tbl_a"], locked: true }]);
});

test("a cell: text, font, thickness, bold, justification, margins and the table's column and row; no position, layer, lock or net", () => {
  const b = board(fixtureTable(2, 3));
  const groups = names(b, ["tbl_a:1:1"]);
  const all = groups.flatMap(([, rows]) => rows);
  for (const gone of ["Position X", "Position Y", "Layer", "Locked", "Net", "Start X", "Orientation"]) assert.ok(!all.includes(gone), gone);
  assert.deepEqual(groups.find(([c]) => c === "Text Properties")![1], ["Text", "Font", "Auto Thickness", "Thickness", "Italic", "Bold", "Mirrored", "Width", "Height", "Horizontal Justification", "Vertical Justification"]);
  assert.deepEqual(groups.find(([c]) => c === "Margins")![1], ["Margin Left", "Margin Top", "Margin Right", "Margin Bottom"]);
  assert.deepEqual(groups.find(([c]) => c === "Table")![1], ["Column Width", "Row Height"]);
  assert.equal(rowOf(b, ["tbl_a:1:1"], "Text")!.value, "r1c1");
  assert.equal(rowOf(b, ["tbl_a:1:1"], "Font")!.value, "KiCad Font");
  assert.equal(rowOf(b, ["tbl_a:1:1"], "Auto Thickness")!.value, true);
  assert.equal(rowOf(b, ["tbl_a:1:1"], "Thickness")!.value, 159_000, "the pen the text is drawn with while the thickness is automatic");
  assert.equal(rowOf(b, ["tbl_a:1:1"], "Horizontal Justification")!.value, "left");
  assert.equal(rowOf(b, ["tbl_a:1:1"], "Margin Left")!.value, 1_000_000);
  assert.equal(rowOf(b, ["tbl_a:1:1"], "Column Width")!.value, 10_000_000);
  assert.equal(rowOf(b, ["tbl_a:1:1"], "Row Height")!.value, 5_000_000);
  assert.equal(rowOf(b, ["tbl_a:1:1"], "Mirrored")!.writable, false, "no verb mirrors a cell on its own");
});

test("a cell's rows send edit_cells for that cell, and a selection of cells sends one for each", () => {
  const b = board(fixtureTable(2, 3));
  const one = (edit_: Record<string, unknown>) => [{ op: "table", verb: "edit_cells", id: "tbl_a", cells: [[1, 2000]], edit: edit_ }];
  assert.deepEqual(edit(b, ["tbl_a:1:2"], "Text", "hello"), one({ text: "hello" }));
  assert.deepEqual(edit(b, ["tbl_a:1:2"], "Bold", true), one({ bold: true }));
  assert.deepEqual(edit(b, ["tbl_a:1:2"], "Italic", true), one({ italic: true }));
  assert.deepEqual(edit(b, ["tbl_a:1:2"], "Font", "Arial"), one({ face: "Arial" }));
  assert.deepEqual(edit(b, ["tbl_a:1:2"], "Font", "KiCad Font"), [], "the stroke font it already has");
  assert.deepEqual(edit(b, ["tbl_a:1:2"], "Width", 1_500_000), one({ width_nm: 1_500_000 }));
  assert.deepEqual(edit(b, ["tbl_a:1:2"], "Height", 1_600_000), one({ height_nm: 1_600_000 }));
  assert.deepEqual(edit(b, ["tbl_a:1:2"], "Thickness", 200_000), one({ thickness_nm: 200_000 }));
  assert.deepEqual(edit(b, ["tbl_a:1:2"], "Auto Thickness", false), one({ thickness_nm: 159_000 }), "off keeps the pen it was drawn with");
  assert.deepEqual(edit(b, ["tbl_a:1:2"], "Horizontal Justification", "right"), one({ h_align: "right" }));
  assert.deepEqual(edit(b, ["tbl_a:1:2"], "Vertical Justification", "top"), one({ v_align: "top" }));
  assert.deepEqual(edit(b, ["tbl_a:1:2"], "Margin Top", 700_000), one({ margins: [null, 700_000, null, null] }));
  assert.deepEqual(edit(b, ["tbl_a:1:2"], "Column Width", 12_000_000), one({ col_width_nm: 12_000_000 }));
  assert.deepEqual(edit(b, ["tbl_a:1:2"], "Row Height", 4_000_000), one({ row_height_nm: 4_000_000 }));
  assert.equal(edit(b, ["tbl_a:0:0", "tbl_a:0:1", "tbl_a:0:2"], "Bold", true).length, 3);
});

test("cells that differ show <...>, and a table with its cells selected shares only what both have", () => {
  const t = fixtureTable(1, 2);
  t.cells[1]!.bold = true;
  const b = board(t);
  assert.equal(rowOf(b, ["tbl_a:0:0", "tbl_a:0:1"], "Bold")!.value, null, "the cells disagree");
  assert.equal(rowOf(b, ["tbl_a:0:0", "tbl_a:0:1"], "Italic")!.value, false);
  const mixed = names(b, ["tbl_a", "tbl_a:0:0"]).flatMap(([, rows]) => rows);
  assert.ok(!mixed.includes("Text") && !mixed.includes("Start X"), "a table and a cell have no property in common but the type");
});
