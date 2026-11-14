import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState } from "../api/types";
import { CUSTOM_PROPERTIES, customPropertyGroup, labelEdited, newCustomPropertyName } from "./pcbCustomProperties";
import { pcbGrid } from "./pcbProperties";

function board(partial: Partial<BoardState>): BoardState {
  return { name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts: [], rules: [], routing: null, drawings: null, checks: [], activity: [], job: "idle", ...partial } as BoardState;
}

test("the Custom Properties group: the names every selected item has, in name order, the text they share or <...>", () => {
  const b = board({ custom_properties: { s1: { Supplier: "ACME", MPN: "X-1", Only: "1" }, s2: { Supplier: "ACME", MPN: "X-2" } } });
  const g = customPropertyGroup(b, [
    { type: "PCB_SHAPE", id: "s1" },
    { type: "PCB_SHAPE", id: "s2" },
  ])!;
  assert.equal(g.caption, CUSTOM_PROPERTIES);
  assert.ok(g.custom);
  assert.deepEqual(
    g.rows.map((r) => [r.name, r.value]),
    [
      ["MPN", null],
      ["Supplier", "ACME"],
    ],
  );
  // shown with no row too (the add button), and not for a footprint's field or a table cell
  assert.deepEqual(customPropertyGroup(b, [{ type: "PCB_VIA", id: "v1" }])!.rows, []);
  assert.equal(customPropertyGroup(b, [{ type: "PCB_FIELD", id: "U1:Value" }]), null);
});

test("Add Custom Property's name is the first Property%d no selected item has", () => {
  const b = board({ custom_properties: { a: { Property0: "" }, b: { Property1: "x" } } });
  assert.equal(newCustomPropertyName(b, ["a", "b"]), "Property2");
  assert.equal(newCustomPropertyName(b, ["c"]), "Property0");
});

test("a name edited: the new property left blank or given a name in use is removed; an old one keeps its label then; another name renames", () => {
  const b = board({ custom_properties: { a: { Property0: "", Supplier: "ACME" } } });
  const rows = ["Position X", "Locked"];
  assert.deepEqual(labelEdited(b, ["a"], rows, "Property0", "", true), { op: "set_custom_property", ids: ["a"], name: "Property0" }, "`onNewItemLeftBlank`");
  assert.deepEqual(labelEdited(b, ["a"], rows, "Property0", "Locked", true), { op: "set_custom_property", ids: ["a"], name: "Property0" }, "a class property's name is in use");
  assert.deepEqual(labelEdited(b, ["a"], rows, "Property0", "Property0", true), { op: "set_custom_property", ids: ["a"], name: "Property0" }, "its own name is in use too");
  assert.deepEqual(labelEdited(b, ["a"], rows, "Property0", "Note", true), { op: "set_custom_property", ids: ["a"], name: "Property0", rename_to: "Note" });
  assert.equal(labelEdited(b, ["a"], rows, "Supplier", "", false), null, "an old one keeps its label");
  assert.equal(labelEdited(b, ["a"], rows, "Supplier", "Property0", false), null);
  assert.deepEqual(labelEdited(b, ["a"], rows, "Supplier", "Vendor", false), { op: "set_custom_property", ids: ["a"], name: "Supplier", rename_to: "Vendor" });
});

test("the board's grid ends with the Custom Properties group, rows or none", () => {
  const b = board({
    drawings: { shapes: [{ id: "s1", kind: "segment", layer: "F.SilkS", stroke_width: 150, filled: false, start: [0, 0], end: [1000, 0] }], texts: [], groups: [], dimensions: [], dimension_settings: {} } as unknown as BoardState["drawings"],
    custom_properties: { s1: { Supplier: "ACME" } },
  });
  const model = pcbGrid(b, ["s1"], "mm");
  const last = model.groups[model.groups.length - 1]!;
  assert.equal(last.caption, "Custom Properties");
  assert.deepEqual(last.rows.map((r) => r.name), ["Supplier"]);
});
