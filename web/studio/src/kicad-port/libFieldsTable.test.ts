import { test } from "node:test";
import assert from "node:assert/strict";
import { addColumn, addFieldProblem, buildRows, expanderColumn, groupValue, INDETERMINATE_STATE, initialColumns, removeFieldQuestion, renameFieldProblem, sortedBy, SYMBOL_NAME, type LibFieldsColumn, type LibFieldsSymbol } from "./libFieldsTable";

test("initialColumns: UpdateFieldList's columns, Reference and Description hidden, the user fields once in any case and sorted (UI-LEFT-3 Y3-5)", () => {
  const cols = initialColumns([[{ name: "MPN" }, { name: "supplier" }], [{ name: "mpn" }, { name: "MPN" }, { name: "Secret", private: true }], [{ name: "Alpha" }]]);
  assert.deepEqual(
    cols.map((c) => [c.name, c.show, c.checkbox]),
    [
      [SYMBOL_NAME, true, false],
      ["Reference", false, false],
      ["Value", true, false],
      ["Footprint", true, false],
      ["Datasheet", true, false],
      ["Description", false, false],
      ["Keywords", true, false],
      ["${EXCLUDE_FROM_BOM}", true, true],
      ["${EXCLUDE_FROM_SIM}", true, true],
      ["${EXCLUDE_FROM_BOARD}", true, true],
      ["Power", true, true],
      ["LocalPower", true, true],
      ["Alpha", true, false],
      ["MPN", true, false],
      ["supplier", true, false],
    ]
  );
  assert.deepEqual(cols.slice(7, 12).map((c) => c.label), ["Exclude From BOM", "Exclude From Simulation", "Exclude From Board", "Power Symbol", "Local Power Symbol"]);
  assert.ok(cols.every((c) => !c.groupBy));
});

test("Add / Rename / Remove Field: KiCad's refusals and question", () => {
  const cols = initialColumns([[{ name: "MPN" }]]);
  assert.equal(addFieldProblem(cols, ""), "Field must have a name.");
  assert.equal(addFieldProblem(cols, "mpn"), "Field name 'mpn' already in use.");
  assert.equal(addFieldProblem(cols, "Supplier"), null);
  assert.equal(addColumn(cols, { name: "VALUE", label: "VALUE", show: true, groupBy: false }).length, cols.length, "a column of the name in any case is not added twice");
  assert.equal(renameFieldProblem(cols, "MPN", "MPN"), null);
  assert.equal(renameFieldProblem(cols, "MPN", "Value"), "Field name 'Value' already exists.");
  assert.equal(renameFieldProblem(cols, "MPN", "mpn"), "Field name 'mpn' already exists.", "GetFieldNameCol finds the column itself in another case");
  assert.equal(renameFieldProblem(cols, "MPN", "PartNo"), null);
  assert.equal(removeFieldQuestion("MPN"), "Are you sure you want to remove the field 'MPN'?");
});

const sym = (id: string, values: Record<string, string>): LibFieldsSymbol => ({ id: `L:${id}`, name: id, value: (c) => (c === SYMBOL_NAME ? id : (values[c] ?? "")) });

test("buildRows: filtered, grouped by the Group By columns, sorted, a group expanded into its symbols", () => {
  const cols: LibFieldsColumn[] = initialColumns([]).map((c) => (c.name === "Footprint" ? { ...c, groupBy: true } : c));
  const syms = [sym("R2", { Value: "10k", Footprint: "R_0603" }), sym("C1", { Value: "100n", Footprint: "C_0603" }), sym("R1", { Value: "1k", Footprint: "R_0603" })];
  const rows = buildRows(syms, cols, { grouping: true, filter: "", sort: { col: SYMBOL_NAME, ascending: true } });
  assert.deepEqual(rows.map((r) => [r.kind, r.ids]), [["single", ["L:C1"]], ["group", ["L:R1", "L:R2"]]]);
  assert.equal(groupValue([syms[2]!, syms[0]!], SYMBOL_NAME), "R1,R2");
  assert.equal(groupValue([syms[2]!, syms[0]!], "Value"), INDETERMINATE_STATE);
  assert.equal(groupValue([syms[2]!, syms[0]!], "Footprint"), "R_0603");
  const open = buildRows(syms, cols, { grouping: true, filter: "", sort: { col: SYMBOL_NAME, ascending: true }, expanded: new Set(["L:R1"]) });
  assert.deepEqual(open.map((r) => [r.kind, r.ids, r.expanded ?? null]), [["single", ["L:C1"], null], ["group", ["L:R1", "L:R2"], true], ["child", ["L:R1"], null], ["child", ["L:R2"], null]]);
  // grouping off, or no Group By column: one row each
  assert.equal(buildRows(syms, cols, { grouping: false, filter: "", sort: { col: SYMBOL_NAME, ascending: true } }).length, 3);
  assert.equal(buildRows(syms, initialColumns([]), { grouping: true, filter: "", sort: { col: SYMBOL_NAME, ascending: true } }).length, 3);
  // a filter is a wildcard over every column; a sort by a column, descending
  // (compared as strings, as `local_cmp` does: "10k" < "1k")
  assert.deepEqual(buildRows(syms, cols, { grouping: false, filter: "R_*", sort: { col: "Value", ascending: false } }).map((r) => r.ids[0]), ["L:R1", "L:R2"]);
  assert.deepEqual(buildRows(syms, cols, { grouping: false, filter: "10*", sort: { col: SYMBOL_NAME, ascending: true } }).map((r) => r.ids[0]), ["L:C1", "L:R2"]);
  assert.deepEqual(sortedBy({ col: SYMBOL_NAME, ascending: true }, SYMBOL_NAME), { col: SYMBOL_NAME, ascending: false });
  assert.deepEqual(sortedBy({ col: SYMBOL_NAME, ascending: false }, "Value"), { col: "Value", ascending: true });
  assert.equal(expanderColumn(cols), SYMBOL_NAME);
  assert.equal(expanderColumn(cols.map((c) => (c.name === SYMBOL_NAME ? { ...c, show: false } : c))), "Value");
});

test("cellLook: GetAttr's stripes, colours and bold; stripeColor; movedColumn; columnWidth; pasteTargets", async () => {
  const { cellLook, stripeColor, movedColumn, columnWidth, pasteTargets } = await import("./libFieldsTable");
  const el = (o: Partial<import("./libFieldsTable").LibFieldsElement>) => ({ original: "", current: "", originallyEmpty: false, currentlyEmpty: false, modified: false, ...o });
  assert.deepEqual(cellLook([el({ original: "x", current: "x" })], false, true), { striped: false, background: "window", bold: false });
  assert.deepEqual(cellLook([el({ original: "x", current: "y", modified: true })], true, true), { striped: false, background: "frame", bold: true });
  assert.deepEqual(cellLook([el({ original: "x", current: "", modified: true })], true, true), { striped: false, background: "rgb(192, 255, 192)", bold: true });
  assert.deepEqual(cellLook([el({ originallyEmpty: true, currentlyEmpty: true })], false, true), { striped: true, background: "window", bold: false }, "a field no symbol has: striped");
  assert.deepEqual(cellLook([el({ original: "x", currentlyEmpty: true, modified: true })], true, true).background, "rgb(220, 180, 180)", "Clear cell of a text");
  assert.equal(cellLook([el({ originallyEmpty: true, currentlyEmpty: true })], false, false).striped, false, "a check box is never striped");
  assert.equal(stripeColor(40), "rgb(220, 180, 180)");
  assert.equal(stripeColor(240), "rgb(100, 10, 10)");
  assert.deepEqual(movedColumn(["a", "b", "c", "d"], 0, 2), ["b", "c", "a", "d"]);
  assert.deepEqual(movedColumn(["a", "b", "c", "d"], 3, 1), ["a", "d", "b", "c"]);
  assert.equal(columnWidth({ Value: 222 }, "Value", 10), 222);
  assert.equal(columnWidth({}, "Value", 10), 100);
  assert.equal(columnWidth({}, "Value", 1000), 1100 / 3);
  assert.deepEqual(pasteTargets([["a", "b"]], { row: 1, col: 1 }, null, 3, 2), [[1, 1, "a"]], "a block from the cursor, cut at the grid's edge");
  assert.deepEqual(pasteTargets([["a"]], { row: 0, col: 0 }, { row0: 0, col0: 0, row1: 1, col1: 1 }, 3, 3), [[0, 0, "a"], [0, 1, "a"], [1, 0, "a"], [1, 1, "a"]], "a selection is filled, the clipboard repeated");
});
