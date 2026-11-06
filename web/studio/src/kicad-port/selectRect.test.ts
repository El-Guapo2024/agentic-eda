import { test } from "node:test";
import assert from "node:assert/strict";
import { rectPolygon, rectSelectionHits } from "./selectRect";
import type { Box } from "./itemBoxes";

const boxes = new Map<string, Box>([
  ["inside", [10, 10, 20, 20]],
  ["straddles", [25, 10, 45, 20]],
  ["outside", [100, 100, 110, 110]],
]);

test("a rectangle dragged left to right takes what lies wholly inside it (INSIDE_RECTANGLE)", () => {
  assert.deepEqual(rectSelectionHits(boxes, [0, 0], [30, 30]), ["inside"]);
});

test("a rectangle dragged right to left takes what it touches (TOUCHING_RECTANGLE)", () => {
  assert.deepEqual(rectSelectionHits(boxes, [30, 30], [0, 0]).sort(), ["inside", "straddles"]);
  assert.deepEqual(rectSelectionHits(boxes, [30, 0], [0, 30]).sort(), ["inside", "straddles"], "the vertical direction does not matter");
});

test("rectPolygon closes the rectangle through its four corners", () => {
  assert.deepEqual(rectPolygon([0, 0], [5, 7]), [
    [0, 0],
    [5, 0],
    [5, 7],
    [0, 7],
  ]);
});
