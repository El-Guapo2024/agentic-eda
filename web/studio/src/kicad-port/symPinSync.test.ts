import { test } from "node:test";
import assert from "node:assert/strict";
import { changePinLength, defaultSyncMode, imagePinsFor, linkedPinsToMove, pinOccupying, planSyncedEdit, synchronizePins } from "./symPinSync";
import type { LibrarySymbolPin } from "../api/types";

function pin(id: string, number: string, unit: number, over: Partial<LibrarySymbolPin> = {}): LibrarySymbolPin {
  return { id, number, name: "IN", electrical_type: "input", shape: "line", at: { x: -5.08, y: 2.54 }, angle_deg: 0, length_mm: 2.54, unit, body_style: 1, hidden: false, name_size_mm: 1.27, number_size_mm: 1.27, ...over };
}

test("the mode is only live for a symbol with more than one unit, and starts on for such a symbol", () => {
  assert.equal(synchronizePins(true, 1), false);
  assert.equal(synchronizePins(true, 2), true);
  assert.equal(synchronizePins(false, 4), false);
  assert.equal(defaultSyncMode(1), false);
  assert.equal(defaultSyncMode(3), true);
});

test("changePinLength keeps the inner end and moves the connection point (SCH_PIN::ChangeLength)", () => {
  // angle 0: the pin points into the body along +x; inner end at x = -5.08 + 2.54 = -2.54
  const shorter = changePinLength(pin("a", "1", 1), 1.27);
  assert.deepEqual(shorter.at, { x: -3.81, y: 2.54 });
  assert.equal(shorter.length_mm, 1.27);
  assert.ok(Math.abs(shorter.at.x + shorter.length_mm - -2.54) < 1e-9, "the inner end is still at -2.54");
  const down = changePinLength(pin("b", "1", 1, { angle_deg: 270, at: { x: 0, y: 5.08 } }), 5.08);
  assert.deepEqual(down.at, { x: 0, y: 7.62 }, "angle 270: into the body is -y, so a longer pin's tip moves up");
});

test("imagePinsFor: one image per other unit, same place, temporary number with the unit letter; none for a shared pin", () => {
  const images = imagePinsFor(pin("a", "7", 2), 3);
  assert.deepEqual(
    images.map((p) => [p.unit, p.number, p.id]),
    [
      [1, "7-UA", undefined],
      [3, "7-UC", undefined],
    ]
  );
  assert.ok(images.every((p) => p.at.x === -5.08 && p.at.y === 2.54 && p.name === "IN"));
  assert.deepEqual(imagePinsFor(pin("a", "7", 0), 3), []);
});

test("pinOccupying: another pin at the position on the same body style (a style-0 pin counts for every style)", () => {
  const a = pin("a", "1", 1);
  assert.equal(pinOccupying(pin("n", "9", 2), [a])?.id, "a");
  assert.equal(pinOccupying(pin("n", "9", 2, { at: { x: 1, y: 1 } }), [a]), undefined);
  assert.equal(pinOccupying(pin("n", "9", 2, { body_style: 2 }), [a]), undefined, "a style-1 pin does not sit under a style-2 one");
  assert.equal(pinOccupying(pin("n", "9", 2, { body_style: 2 }), [pin("c", "1", 1, { body_style: 0 })])?.id, "c");
  assert.equal(pinOccupying(a, [a]), undefined, "a pin never occupies its own place");
});

test("linkedPinsToMove: one matching pin per other unit travels with the moved pin", () => {
  const cur = pin("a1", "1", 1);
  const pins = [cur, pin("a2", "1-UB", 2), pin("a2b", "x", 2), pin("a3", "1-UC", 3), pin("far", "2", 2, { at: { x: 0, y: 0 } }), pin("other", "9", 3, { name: "OUT" })];
  assert.deepEqual(
    linkedPinsToMove(cur, pins, 3).map((p) => p.id),
    ["a2", "a3"],
    "one pin per unit, same position/orientation/type/name"
  );
  assert.deepEqual(linkedPinsToMove(pin("solo", "1", 1, { at: { x: 9, y: 9 } }), pins, 3), []);
});

test("planSyncedEdit: the matching pins of the other units follow the edit but keep their numbers", () => {
  const original = pin("a1", "1", 1);
  const edited = pin("a1", "1", 1, { name: "CLK", electrical_type: "output", shape: "clock", length_mm: 5.08, at: { x: -7.62, y: 2.54 }, angle_deg: 0, name_size_mm: 1.0, number_size_mm: 0.9 });
  const pins = [original, pin("a2", "1-UB", 2), pin("a3", "1-UC", 3), pin("unrelated", "5", 2, { at: { x: 0, y: 0 } })];
  const plan = planSyncedEdit(original, edited, pins, 3);
  assert.deepEqual(plan.removeIds, []);
  assert.deepEqual(
    plan.updates.map((p) => [p.id, p.number, p.name, p.electrical_type, p.shape, p.length_mm, p.at, p.name_size_mm, p.number_size_mm]),
    [
      ["a2", "1-UB", "CLK", "output", "clock", 5.08, { x: -7.62, y: 2.54 }, 1.0, 0.9],
      ["a3", "1-UC", "CLK", "output", "clock", 5.08, { x: -7.62, y: 2.54 }, 1.0, 0.9],
    ]
  );
});

test("planSyncedEdit: a pin on another body style only takes the properties, not the geometry", () => {
  const original = pin("a1", "1", 1);
  const edited = pin("a1", "1", 1, { name: "Z", length_mm: 5.08, at: { x: -7.62, y: 2.54 } });
  const style2 = pin("b2", "1", 2, { body_style: 2 });
  const plan = planSyncedEdit(original, edited, [original, style2], 2);
  assert.equal(plan.updates.length, 1);
  assert.equal(plan.updates[0]!.name, "Z");
  assert.equal(plan.updates[0]!.length_mm, 2.54, "length and position follow only a pin of the same body style");
  assert.deepEqual(plan.updates[0]!.at, { x: -5.08, y: 2.54 });
});

test("planSyncedEdit: an edit that makes the pin common to all units removes the matching pins of the other units", () => {
  const original = pin("a1", "1", 1);
  const edited = pin("a1", "1", 0);
  const plan = planSyncedEdit(original, edited, [original, pin("a2", "1-UB", 2), pin("a3", "1-UC", 3)], 3);
  assert.deepEqual(plan.removeIds.sort(), ["a2", "a3"]);
  assert.deepEqual(plan.updates, []);
});

test("planSyncedEdit: a pin the edit made different no longer matches (compared against the original)", () => {
  const original = pin("a1", "1", 1);
  const edited = pin("a1", "1", 1, { name: "NEW" });
  const plan = planSyncedEdit(original, edited, [original, pin("a2", "1-UB", 2, { name: "something else" })], 2);
  assert.deepEqual(plan.updates, [], "the other unit's pin had a different name before the edit, so it is not an image");
});
