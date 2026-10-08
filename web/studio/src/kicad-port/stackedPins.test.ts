import { test } from "node:test";
import assert from "node:assert/strict";
import { comparePinNumbers, expandStackedPinNotation, parseAlphaNumericPin, planConvertStackedPins, planExplodeStackedPin, stackedNotation, stackedPinMenuState } from "./stackedPins";
import type { LibrarySymbolPin } from "../api/types";

const pin = (id: string, number: string, x = 0, y = 0, over: Partial<LibrarySymbolPin> = {}): LibrarySymbolPin => ({
  id,
  number,
  name: "VCC",
  electrical_type: "passive",
  shape: "line",
  at: { x, y },
  angle_deg: 0,
  length_mm: 2.54,
  unit: 1,
  body_style: 1,
  hidden: false,
  name_size_mm: null,
  number_size_mm: null,
  ...over,
});

test("a pin number splits into its prefix and trailing number", () => {
  assert.deepEqual(parseAlphaNumericPin("A12"), ["A", 12]);
  assert.deepEqual(parseAlphaNumericPin("12"), ["", 12]);
  assert.deepEqual(parseAlphaNumericPin("VCC"), ["VCC", -1]);
});

test("stacked notation expands lists and ranges, with or without a prefix", () => {
  assert.deepEqual(expandStackedPinNotation("[1-3,5]"), { numbers: ["1", "2", "3", "5"], valid: true });
  assert.deepEqual(expandStackedPinNotation("[A1-A3,B7]"), { numbers: ["A1", "A2", "A3", "B7"], valid: true });
  assert.deepEqual(expandStackedPinNotation("7"), { numbers: ["7"], valid: true });
});

test("a malformed notation is invalid and stands for itself", () => {
  assert.deepEqual(expandStackedPinNotation("[1-3"), { numbers: ["[1-3"], valid: false });
  assert.deepEqual(expandStackedPinNotation("[3-1]"), { numbers: ["[3-1]"], valid: false });
  assert.deepEqual(expandStackedPinNotation("[A1-B3]"), { numbers: ["[A1-B3]"], valid: false });
  assert.deepEqual(expandStackedPinNotation("[]"), { numbers: ["[]"], valid: false });
});

test("numbers sort numerically first, then as strings", () => {
  assert.deepEqual(["10", "B", "2", "A"].sort(comparePinNumbers), ["2", "10", "A", "B"]);
});

test("the notation collapses consecutive runs and keeps the rest", () => {
  assert.equal(stackedNotation(["1", "2", "3"]), "[1-3]");
  assert.equal(stackedNotation(["1", "2"]), "[1,2]");
  assert.equal(stackedNotation(["1", "2", "3", "5"]), "[1-3,5]");
  assert.equal(stackedNotation(["A1", "A2", "A3", "B1"]), "[A1-A3,B1]");
  assert.equal(stackedNotation(["1", "VCC"]), "[1,VCC]");
});

test("convert folds co-located pins into the first one, the others deleted", () => {
  const pins = [pin("p1", "3", 5, 5), pin("p2", "1", 5, 5), pin("p3", "2", 5, 5), pin("p4", "9", 0, 0)];
  const plan = planConvertStackedPins("Lib:U", pins, ["p1"]);
  assert.equal(plan.ok, true);
  if (!plan.ok) return;
  assert.deepEqual(plan.cmds[0], { op: "edit_symbol_pin", lib_id: "Lib:U", id: "p2", pin: { ...pins[1], number: "[1-3]" } });
  assert.deepEqual(plan.cmds.slice(1), [{ op: "delete_symbol_pin", lib_id: "Lib:U", id: "p3" }, { op: "delete_symbol_pin", lib_id: "Lib:U", id: "p1" }]);
});

test("convert needs two pins at one place", () => {
  assert.deepEqual(planConvertStackedPins("L", [pin("a", "1")], ["a"]), { ok: false, message: "At least two pins are needed to convert to stacked pins" });
  assert.deepEqual(planConvertStackedPins("L", [pin("a", "1", 0, 0), pin("b", "2", 5, 0)], ["a", "b"]), { ok: false, message: "All pins must be at the same location" });
});

test("explode makes the original the smallest number and hidden copies of the rest", () => {
  const stacked = pin("p1", "[1-3]", 5, 5, { electrical_type: "power_in", hidden: true });
  const plan = planExplodeStackedPin("Lib:U", [stacked], ["p1"]);
  assert.equal(plan.ok, true);
  if (!plan.ok) return;
  assert.deepEqual(plan.cmds[0], { op: "edit_symbol_pin", lib_id: "Lib:U", id: "p1", pin: { ...stacked, number: "1", hidden: false } });
  const added = plan.cmds.slice(1);
  assert.equal(added.length, 2);
  assert.deepEqual(added.map((c) => (c.op === "add_symbol_pin" ? [c.pin.number, c.pin.hidden, c.pin.electrical_type] : null)), [["2", true, "passive"], ["3", true, "passive"]]);
});

test("explode refuses a pin without valid stacked notation", () => {
  assert.deepEqual(planExplodeStackedPin("L", [pin("a", "7")], ["a"]), { ok: false, message: "Selected pin does not have valid stacked notation" });
  assert.deepEqual(planExplodeStackedPin("L", [pin("a", "7")], []), { ok: false, message: "Select a single pin with stacked notation to explode" });
});

test("the menu offers Convert for pins sharing a place and Explode for one stacked pin", () => {
  const pins = [pin("a", "1", 5, 5), pin("b", "2", 5, 5), pin("c", "3", 0, 0), pin("d", "[4-6]", 9, 9)];
  // Two pins at one place: Convert. One of a co-located pair: Convert too. A lone pin, or pins apart: not.
  assert.deepEqual(stackedPinMenuState(pins, ["a", "b"]), { canConvert: true, canExplode: false });
  assert.deepEqual(stackedPinMenuState(pins, ["a"]), { canConvert: true, canExplode: false });
  assert.deepEqual(stackedPinMenuState(pins, ["c"]), { canConvert: false, canExplode: false });
  assert.deepEqual(stackedPinMenuState(pins, ["a", "c"]), { canConvert: false, canExplode: false });
  // Anything that is not a pin among the selection (a graphic id) rules Convert out.
  assert.deepEqual(stackedPinMenuState(pins, ["a", "b", "sym_graphic"]), { canConvert: false, canExplode: false });
  // Explode needs the single selected pin to carry valid stacked notation.
  assert.deepEqual(stackedPinMenuState(pins, ["d"]), { canConvert: false, canExplode: true });
  assert.deepEqual(stackedPinMenuState(pins, ["d", "a"]), { canConvert: false, canExplode: false });
  assert.deepEqual(stackedPinMenuState(pins, []), { canConvert: false, canExplode: false });
});

test("a pin number without brackets is one pin, and bracketed lists keep their letters", () => {
  assert.deepEqual(expandStackedPinNotation("A12"), { numbers: ["A12"], valid: true });
  assert.deepEqual(expandStackedPinNotation("[A1-A3, B7]"), { numbers: ["A1", "A2", "A3", "B7"], valid: true });
  assert.deepEqual(expandStackedPinNotation("[ 4 ]"), { numbers: ["4"], valid: true });
});

test("a bracket on one end only, a backwards range, mixed letters or an empty list comes back as it was", () => {
  for (const bad of ["[1,2", "1,2]", "[5-3]", "[A1-B3]", "[1-x]", "[]", "[,]"]) {
    assert.deepEqual(expandStackedPinNotation(bad), { numbers: [bad], valid: false }, bad);
  }
});
