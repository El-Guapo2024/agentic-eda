import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibrarySymbolGraphic, LibrarySymbolPin } from "../api/types";
import { deletePlan, duplicateItems, maxPinNumber, nearestHalfGrid, pastedItems, repeatPin, rotatePlan } from "./symEditItems";

const pin = (id: string, number: string, x: number, y: number, unit = 1, extra: Partial<LibrarySymbolPin> = {}): LibrarySymbolPin => ({
  id,
  number,
  name: "A",
  electrical_type: "input",
  shape: "line",
  at: { x, y },
  angle_deg: 0,
  length_mm: 2.54,
  unit,
  body_style: 1,
  hidden: false,
  name_size_mm: 1.27,
  number_size_mm: 1.27,
  ...extra,
});
const rect: LibrarySymbolGraphic = { kind: "rectangle", id: "g1", unit: 1, body_style: 1, start: { x: -1, y: -1 }, end: { x: 1, y: 1 }, stroke_mm: 0.254, fill: "none" };

test("DoDelete in sync mode takes the twin of each pin in the other units, one per unit (E4)", () => {
  const pins = [pin("a", "1", 0, 0, 1), pin("b", "1-UB", 0, 0, 2), pin("c", "1-UC", 0, 0, 3), pin("c2", "9", 0, 0, 3), pin("d", "2", 0, 0, 2, { name: "B" })];
  const plan = deletePlan(["a", "g1"], pins, [rect], true, 3);
  assert.deepEqual(plan.pins.sort(), ["a", "b", "c"], "the twins at the same place with the same name, type and body style -- the first per unit; another name stays");
  assert.deepEqual(plan.graphics, ["g1"]);
  assert.deepEqual(deletePlan(["a"], pins, [], false, 3).pins, ["a"], "out of sync mode only the selection");
});

test("Rotate turns several pins about the half-grid centre of the selection, one pin about itself (E5)", () => {
  const p1 = pin("a", "1", 0, 0);
  const p2 = pin("b", "2", 0, -2.54);
  // the box of both pins (their ends at x 2.54): x 0..2.54, y -2.54..0 -> centre (1.27, -1.27), on the 0.635 half grid of 1.27
  assert.deepEqual(nearestHalfGrid({ x: 1.3, y: -1.2 }, 1.27), { x: 1.27, y: -1.27 });
  const [r1, r2] = rotatePlan([p1, p2], 2, true, 1.27);
  assert.deepEqual([r1!.at, r1!.angle_deg], [{ x: 0, y: -2.54 }, 90]);
  assert.deepEqual([r2!.at, r2!.angle_deg], [{ x: 2.54, y: -2.54 }, 90]);
  const [one] = rotatePlan([p2], 1, false, 1.27);
  assert.deepEqual([one!.at, one!.angle_deg], [{ x: 0, y: -2.54 }, 270], "one item turns in place, clockwise");
});

test("RepeatPin: 100 mil down for a horizontal pin, across for a vertical one, name and number incremented (E3)", () => {
  const r = repeatPin(pin("a", "1", 0, 0, 1, { name: "A1" }));
  assert.deepEqual([r.number, r.name, r.at, r.id], ["2", "A2", { x: 0, y: -2.54 }, undefined]);
  const up = repeatPin(pin("a", "B9", 5.08, 0, 1, { name: "CLK", angle_deg: 270 }));
  assert.deepEqual([up.number, up.name, up.at], ["B10", "CLK", { x: 7.62, y: 0 }]);
});

test("Duplicate numbers each copied pin GetMaxPinNumber() + 1, in the order of the old numbers (E3)", () => {
  const pins = [pin("a", "3", 0, 0), pin("b", "1", 0, -2.54), pin("c", "", 0, -5.08), pin("d", "X", 0, -7.62)];
  assert.equal(maxPinNumber(pins), 3);
  const d = duplicateItems(["a", "b", "c", "g1"], pins, [rect]);
  assert.deepEqual(d.pins.map((p) => p.number), ["4", "5", ""], "1 then 3 (StrNumCmp), the unnumbered pin keeps no number");
  assert.equal(d.graphics.length, 1);
  assert.ok(d.pins.every((p) => p.id === undefined) && d.graphics[0]!.id === undefined);
});

test("Paste puts the items on the unit and body style being edited unless common to all", () => {
  const out = pastedItems({ pins: [pin("a", "1", 0, 0, 1), pin("b", "2", 0, 0, 0, { body_style: 0 })], graphics: [rect] }, 3, 2);
  assert.deepEqual(out.pins.map((p) => [p.unit, p.body_style]), [[3, 2], [0, 0]]);
  assert.deepEqual([out.graphics[0]!.unit, out.graphics[0]!.body_style], [3, 2]);
});
