import { test } from "node:test";
import assert from "node:assert/strict";
import { nearestPointOnSegment } from "./schBusUnfold";

test("the entry roots at the bus point nearest the grid-snapped cursor, which is on the grid for a horizontal bus", () => {
  // a horizontal bus along y = 2540 from x = 60960 to 68580; the raw cursor is off-grid, the snapped one (50 mil = 1270) is not
  const a: [number, number] = [60960, 2540];
  const b: [number, number] = [68580, 2540];
  assert.deepEqual(nearestPointOnSegment(a, b, [65380, 3100]), [65380, 2540], "the raw cursor lands off the grid");
  assert.deepEqual(nearestPointOnSegment(a, b, [66040, 3810]), [66040, 2540], "the snapped cursor stays on it");
});

test("a point past either end clamps to that end, and a vertical bus keeps its x", () => {
  assert.deepEqual(nearestPointOnSegment([0, 0], [1000, 0], [5000, 700]), [1000, 0]);
  assert.deepEqual(nearestPointOnSegment([0, 0], [1000, 0], [-5000, 700]), [0, 0]);
  assert.deepEqual(nearestPointOnSegment([1270, 0], [1270, 5080], [3810, 2540]), [1270, 2540]);
});

test("a diagonal bus projects perpendicularly and a zero-length one is its own point", () => {
  assert.deepEqual(nearestPointOnSegment([0, 0], [1000, 1000], [1000, 0]), [500, 500]);
  assert.deepEqual(nearestPointOnSegment([7, 9], [7, 9], [100, 100]), [7, 9]);
});
