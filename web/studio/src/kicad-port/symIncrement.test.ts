// SCH_TOOL_BASE::Increment in the Symbol Editor (symIncrement.ts).
import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibrarySymbolGraphic, LibrarySymbolPin } from "../api/types";
import { planSymbolIncrement } from "./symIncrement";

const pin = (number: string, name: string): LibrarySymbolPin => ({ id: "p", number, name, electrical_type: "input", shape: "line", at: { x: 0, y: 0 }, angle_deg: 0, length_mm: 2.54, unit: 1, body_style: 1, hidden: false, name_size_mm: null, number_size_mm: null });
const box = (x0: number, y0: number, x1: number, y1: number) => ({ minX: x0, minY: y0, maxX: x1, maxY: y1 });
const text = (t: string): Extract<LibrarySymbolGraphic, { kind: "text" }> => ({ kind: "text", id: "t", unit: 0, body_style: 1, text: t, at: { x: 0, y: 0 }, angle_deg: 0, size_mm: 1.27 });

test("a pin's number goes up when the cursor is on the number, its name when it is on the name; elsewhere nothing", () => {
  const item = { kind: "pin" as const, pin: pin("7", "D3"), numberBox: box(0, 0, 10, 10), nameBox: box(20, 0, 30, 10) };
  assert.equal(planSymbolIncrement([item], [5, 5], 1, 0)!.pins[0]!.number, "8");
  assert.deepEqual(planSymbolIncrement([item], [25, 5], 1, 0)!.pins.map((p) => [p.number, p.name]), [["7", "D4"]]);
  assert.deepEqual(planSymbolIncrement([item], [15, 5], 1, 0)!.pins, []);
  assert.equal(planSymbolIncrement([item], [25, 5], -1, 0)!.pins[0]!.name, "D2");
});

test("the symbol editor's letters skip I, O, S, Q, X and Z (SetSkipIOSQXZ): a BGA ball H1 goes to J1 on its secondary part", () => {
  const item = { kind: "pin" as const, pin: pin("H1", ""), numberBox: box(0, 0, 10, 10), nameBox: null };
  assert.equal(planSymbolIncrement([item], [1, 1], 1, 1)!.pins[0]!.number, "J1");
});

test("a text's text goes up; a selection of pins and texts does nothing", () => {
  assert.equal(planSymbolIncrement([{ kind: "text", text: text("A9") }], [0, 0], 1, 0)!.texts[0]!.text, "A10");
  assert.equal(planSymbolIncrement([{ kind: "text", text: text("A9") }, { kind: "pin", pin: pin("1", ""), numberBox: null, nameBox: null }], [0, 0], 1, 0), null);
});
