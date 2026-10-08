import { test } from "node:test";
import assert from "node:assert/strict";
import { appendLassoPoint, applyAreaSelection, boxHitTestPolygon, lassoContained, lassoHits, pointInPoly, segmentMeetsBox, selectionModeForAction, signedArea, type Pt } from "./lasso";
import type { Box } from "./itemBoxes";

// y points down, like the screen: this square goes right, down, left, up -- clockwise on screen.
const CW: Pt[] = [
  [0, 0],
  [100, 0],
  [100, 100],
  [0, 100],
];
const CCW: Pt[] = [...CW].reverse();

test("the lasso actions set the selection mode", () => {
  assert.equal(selectionModeForAction("selectSetLasso"), "lasso");
  assert.equal(selectionModeForAction("selectSetRect"), "rect");
});

test("clockwise on screen is inside mode, counterclockwise is touching, a degenerate lasso touches", () => {
  assert.equal(signedArea(CW), 10_000);
  assert.equal(signedArea(CCW), -10_000);
  assert.equal(lassoContained(CW), true);
  assert.equal(lassoContained(CCW), false);
  assert.equal(lassoContained([[0, 0]]), false);
  assert.equal(lassoContained([
    [0, 0],
    [10, 10],
  ]), false);
});

test("segmentMeetsBox: crossing, inside, touching and missing segments", () => {
  const box: Box = [10, 10, 20, 20];
  assert.equal(segmentMeetsBox([0, 15], [30, 15], box), true, "passes through");
  assert.equal(segmentMeetsBox([12, 12], [18, 18], box), true, "wholly inside");
  assert.equal(segmentMeetsBox([20, 0], [20, 30], box), true, "along the edge");
  assert.equal(segmentMeetsBox([0, 0], [5, 30], box), false, "left of it");
  assert.equal(segmentMeetsBox([0, 25], [30, 25], box), false, "below it");
  assert.equal(segmentMeetsBox([0, 0], [30, 5], box), false, "above it");
  assert.equal(segmentMeetsBox([0, 30], [30, 0], box), true, "diagonal across");
  assert.equal(segmentMeetsBox([0, 30], [5, 20], box), false, "diagonal that stops short");
});

test("pointInPoly", () => {
  assert.equal(pointInPoly([50, 50], CW), true);
  assert.equal(pointInPoly([150, 50], CW), false);
});

test("inside mode: a box wholly inside is hit, one crossing an edge or outside is not", () => {
  assert.equal(boxHitTestPolygon(CW, [10, 10, 40, 40], true), true);
  assert.equal(boxHitTestPolygon(CW, [80, 80, 120, 120], true), false, "crosses the lasso's edge");
  assert.equal(boxHitTestPolygon(CW, [200, 200, 220, 220], true), false, "outside");
  assert.equal(boxHitTestPolygon(CW, [-50, -50, 150, 150], true), false, "the lasso lies inside the box");
});

test("touching mode: crossing, containing and contained boxes are hit, a disjoint one is not", () => {
  assert.equal(boxHitTestPolygon(CCW, [80, 80, 120, 120], false), true, "crosses an edge");
  assert.equal(boxHitTestPolygon(CCW, [10, 10, 40, 40], false), true, "inside");
  assert.equal(boxHitTestPolygon(CCW, [-50, -50, 150, 150], false), true, "the lasso lies inside the box");
  assert.equal(boxHitTestPolygon(CCW, [200, 200, 220, 220], false), false);
  assert.equal(boxHitTestPolygon([[0, 0], [10, 10]], [0, 0, 5, 5], false), false, "fewer than three points select nothing");
});

test("a concave lasso: a box in the notch is outside", () => {
  // a U shape opening upward
  const u: Pt[] = [
    [0, 0],
    [30, 0],
    [30, 70],
    [70, 70],
    [70, 0],
    [100, 0],
    [100, 100],
    [0, 100],
  ];
  assert.equal(boxHitTestPolygon(u, [40, 10, 60, 30], true), false, "in the notch, outside the polygon");
  assert.equal(boxHitTestPolygon(u, [40, 10, 60, 30], false), false);
  assert.equal(boxHitTestPolygon(u, [5, 5, 25, 60], true), true, "in the left arm");
});

test("lassoHits sorts by row then column", () => {
  const boxes = new Map<string, Box>([
    ["b", [60, 10, 70, 20]],
    ["a", [10, 10, 20, 20]],
    ["c", [10, 50, 20, 60]],
    ["out", [300, 300, 310, 310]],
  ]);
  assert.deepEqual(lassoHits(boxes, CW, true), ["a", "b", "c"]);
});

test("applyAreaSelection: a plain drag replaces, shift adds, ctrl+shift removes, exclusive-or toggles", () => {
  assert.deepEqual(applyAreaSelection(["x", "y"], ["a"], "set"), ["a"]);
  assert.deepEqual(applyAreaSelection(["x"], ["a", "x"], "add"), ["x", "a"]);
  assert.deepEqual(applyAreaSelection(["x", "a"], ["a", "q"], "subtract"), ["x"]);
  assert.deepEqual(applyAreaSelection(["x", "a"], ["a", "q"], "toggle"), ["x", "q"]);
});

test("appendLassoPoint ignores samples closer than the minimum distance", () => {
  const p: Pt[] = [[0, 0]];
  assert.equal(appendLassoPoint(p, [1, 1], 3), p);
  assert.deepEqual(appendLassoPoint(p, [10, 0], 3), [
    [0, 0],
    [10, 0],
  ]);
});
