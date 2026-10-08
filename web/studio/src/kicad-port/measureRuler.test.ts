import { test } from "node:test";
import assert from "node:assert/strict";
import { measureClick, measureLabel } from "./measureRuler";

test("the first click starts a ruler, the second fixes its end, the third starts the next", () => {
  const a = measureClick([], [0, 0]);
  assert.deepEqual(a, [[0, 0]]);
  const b = measureClick(a, [3000, 4000]);
  assert.deepEqual(b, [[0, 0], [3000, 4000]]);
  assert.deepEqual(measureClick(b, [10, 10]), [[10, 10]]);
});

test("the label has the distance and the extent in x and y, in the unit asked for", () => {
  assert.equal(measureLabel([0, 0], [3000, 4000], "mm"), "5.000 mm  (dx 3.000 mm, dy 4.000 mm)");
  assert.equal(measureLabel([3000, 4000], [0, 0], "mm"), "5.000 mm  (dx 3.000 mm, dy 4.000 mm)", "the same either way round");
  assert.equal(measureLabel([0, 0], [25400, 0], "in"), "1.0000 in  (dx 1.0000 in, dy 0.0000 in)");
});
