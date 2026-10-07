import { test } from "node:test";
import assert from "node:assert/strict";
import { expandStackedPinNotation } from "./stackedPins";

test("a pin number without brackets is one pin", () => {
  assert.deepEqual(expandStackedPinNotation("7"), { numbers: ["7"], valid: true });
  assert.deepEqual(expandStackedPinNotation("A12"), { numbers: ["A12"], valid: true });
});

test("a bracketed list and ranges expand in order, with the letters kept", () => {
  assert.deepEqual(expandStackedPinNotation("[1,2,5-7]"), { numbers: ["1", "2", "5", "6", "7"], valid: true });
  assert.deepEqual(expandStackedPinNotation("[A1-A3, B7]"), { numbers: ["A1", "A2", "A3", "B7"], valid: true });
  assert.deepEqual(expandStackedPinNotation("[ 4 ]"), { numbers: ["4"], valid: true });
});

test("a bracket on one end only, a backwards range, mixed letters or an empty list is invalid and comes back as it was", () => {
  for (const bad of ["[1,2", "1,2]", "[5-3]", "[A1-B3]", "[1-x]", "[]", "[,]"]) {
    assert.deepEqual(expandStackedPinNotation(bad), { numbers: [bad], valid: false }, bad);
  }
});
