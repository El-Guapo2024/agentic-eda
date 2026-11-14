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
