import { test } from "node:test";
import assert from "node:assert/strict";
import { symbolEditorTarget } from "./editWithSymbolEditor";

const symbols = [
  { id: "R1", lib_id: "Device:R" },
  { id: "R2", lib_id: "Device:R" },
  { id: "U9", lib_id: "Gone:Part" },
  { id: "X1", lib_id: "" },
];
const has = (id: string) => id === "Device:R";

test("r6 C6-M3: the hovered symbol when nothing is selected, `selection.Front()` when several are", () => {
  assert.deepEqual(symbolEditorTarget([], symbols, has, "R2"), { kind: "open", id: "R2", libId: "Device:R", unit: 1, bodyStyle: 1 });
  assert.deepEqual(symbolEditorTarget(["R2", "R1"], symbols, has, "R1"), { kind: "open", id: "R2", libId: "Device:R", unit: 1, bodyStyle: 1 });
  // a selection is trimmed to symbols (`RequestSelection( { SCH_SYMBOL_T } )`); the hover counts only with nothing selected
  assert.deepEqual(symbolEditorTarget(["wire_1", "R1"], symbols, has, null), { kind: "open", id: "R1", libId: "Device:R", unit: 1, bodyStyle: 1 });
  assert.deepEqual(symbolEditorTarget(["wire_1"], symbols, has, "R1"), { kind: "none" });
  assert.deepEqual(symbolEditorTarget([], symbols, has, null), { kind: "none" });
});

test("r6 C6-M3: a broken library link is refused (`IsMissingLibSymbol`)", () => {
  assert.deepEqual(symbolEditorTarget(["U9"], symbols, has, null), { kind: "broken", id: "U9" });
  assert.deepEqual(symbolEditorTarget([], symbols, has, "X1"), { kind: "broken", id: "X1" });
});

test("r7 A7-M2: the editor opens the placed symbol's unit and body style (`LoadSymbolFromSchematic`: std::max( 1, .. ))", () => {
  const units = [
    { id: "U1", lib_id: "Device:R", unit: 1, body_style: 1 },
    { id: "U1", lib_id: "Device:R", unit: 3, body_style: 2 },
    { id: "U2", lib_id: "Device:R", unit: 0, body_style: 0 },
  ];
  assert.deepEqual(symbolEditorTarget([], units, has, units[1]!), { kind: "open", id: "U1", libId: "Device:R", unit: 3, bodyStyle: 2 }, "the hovered unit itself");
  assert.deepEqual(symbolEditorTarget(["U1"], units, has, units[1]!), { kind: "open", id: "U1", libId: "Device:R", unit: 3, bodyStyle: 2 }, "the selected reference's unit under the cursor");
  assert.deepEqual(symbolEditorTarget(["U1"], units, has, null), { kind: "open", id: "U1", libId: "Device:R", unit: 1, bodyStyle: 1 }, "else its first unit");
  assert.deepEqual(symbolEditorTarget(["U2"], units, has, null), { kind: "open", id: "U2", libId: "Device:R", unit: 1, bodyStyle: 1 }, "never below 1");
});
