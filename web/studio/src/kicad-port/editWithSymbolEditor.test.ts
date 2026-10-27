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
  assert.deepEqual(symbolEditorTarget([], symbols, has, "R2"), { kind: "open", id: "R2", libId: "Device:R" });
  assert.deepEqual(symbolEditorTarget(["R2", "R1"], symbols, has, "R1"), { kind: "open", id: "R2", libId: "Device:R" });
  // a selection is trimmed to symbols (`RequestSelection( { SCH_SYMBOL_T } )`); the hover counts only with nothing selected
  assert.deepEqual(symbolEditorTarget(["wire_1", "R1"], symbols, has, null), { kind: "open", id: "R1", libId: "Device:R" });
  assert.deepEqual(symbolEditorTarget(["wire_1"], symbols, has, "R1"), { kind: "none" });
  assert.deepEqual(symbolEditorTarget([], symbols, has, null), { kind: "none" });
});

test("r6 C6-M3: a broken library link is refused (`IsMissingLibSymbol`)", () => {
  assert.deepEqual(symbolEditorTarget(["U9"], symbols, has, null), { kind: "broken", id: "U9" });
  assert.deepEqual(symbolEditorTarget([], symbols, has, "X1"), { kind: "broken", id: "X1" });
});
