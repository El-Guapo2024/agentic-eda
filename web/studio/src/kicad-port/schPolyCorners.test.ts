import { test } from "node:test";
import assert from "node:assert/strict";
import { addCorner, canAddCorner, canRemoveCorner, cornerAt, nearestEdge, removeCorner, type P } from "./schPolyCorners";

const square: P[] = [[0, 0], [10_000, 0], [10_000, 10_000], [0, 10_000]];

test("the nearest edge includes the closing one", () => {
  assert.equal(nearestEdge(square, [5_000, 100]).index, 0);
  assert.equal(nearestEdge(square, [9_900, 5_000]).index, 1);
  assert.equal(nearestEdge(square, [5_000, 9_900]).index, 2);
  assert.equal(nearestEdge(square, [100, 5_000]).index, 3, "the edge from the last corner back to the first");
});

test("a corner can be created only with the cursor on the outline", () => {
  assert.equal(canAddCorner(square, [5_000, 50], 100), true);
  assert.equal(canAddCorner(square, [5_000, 5_000], 100), false);
});

test("the new corner goes in after the start of the nearest edge", () => {
  assert.deepEqual(addCorner(square, [5_000, 0]), [[0, 0], [5_000, 0], [10_000, 0], [10_000, 10_000], [0, 10_000]]);
  assert.deepEqual(addCorner(square, [0, 5_000]), [[0, 0], [10_000, 0], [10_000, 10_000], [0, 10_000], [0, 5_000]], "on the closing edge it is appended");
});

test("the corner under the cursor is the nearest one within the tolerance", () => {
  assert.equal(cornerAt(square, [10_050, 20], 100), 1);
  assert.equal(cornerAt(square, [5_000, 5_000], 100), -1);
});

test("a rule area keeps three corners, a polygon shape two", () => {
  const five: P[] = [...square, [5_000, 12_000]];
  assert.deepEqual(removeCorner("rule_area", five, [5_000, 12_000], 100), square);
  assert.equal(canRemoveCorner("rule_area", square.slice(0, 3), [0, 0], 100), false);
  assert.equal(canRemoveCorner("polygon", square.slice(0, 3), [0, 0], 100), true);
  assert.equal(removeCorner("rule_area", square.slice(0, 3), [0, 0], 100), null);
  assert.equal(removeCorner("rule_area", five, [500, 500], 100), null, "no corner under the cursor");
});
