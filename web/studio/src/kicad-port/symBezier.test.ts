// The Symbol Editor's bezier tool and its point editor (symBezier.ts), against EDA_SHAPE's edit states and EDA_BEZIER_POINT_EDIT_BEHAVIOR.
import { test } from "node:test";
import assert from "node:assert/strict";
import { bezierDone, bezierEditPoints, bezierFromClicks, bezierGraphic, bezierWithPoint } from "./symBezier";

test("the four clicks of DrawShape( BEZIER ): start; end (the second control point drags with it); control 1; control 2", () => {
  assert.equal(bezierFromClicks([], [1, 1]), null);
  assert.deepEqual(bezierFromClicks([[0, 0]], [9, 9]), { start: [0, 0], c1: [0, 0], c2: [9, 9], end: [9, 9] }, "state 1: the end and control 2 follow the cursor");
  assert.deepEqual(bezierFromClicks([[0, 0], [30, 0]], [5, 20]), { start: [0, 0], c1: [5, 20], c2: [30, 0], end: [30, 0] }, "state 2: control 1 follows");
  assert.deepEqual(bezierFromClicks([[0, 0], [30, 0], [5, 20]], [25, 20]), { start: [0, 0], c1: [5, 20], c2: [25, 20], end: [30, 0] }, "state 3: control 2 follows");
  assert.deepEqual(bezierFromClicks([[0, 0], [30, 0], [5, 20], [25, 20]], [99, 99]), { start: [0, 0], c1: [5, 20], c2: [25, 20], end: [30, 0] });
  assert.equal(bezierDone([[0, 0], [1, 1], [2, 2]]), false);
  assert.equal(bezierDone([[0, 0], [1, 1], [2, 2], [3, 3]]), true);
});

test("the graphic is in the symbol's mm with +y up, unfilled", () => {
  const g = bezierGraphic({ start: [0, 0], c1: [1_270_000, -2_540_000], c2: [2_540_000, -2_540_000], end: [3_810_000, 0] }, 1, 1, 0.254);
  assert.deepEqual(g, { kind: "bezier", unit: 1, body_style: 1, start: { x: 0, y: 0 }, c1: { x: 1.27, y: 2.54 }, c2: { x: 2.54, y: 2.54 }, end: { x: 3.81, y: 0 }, stroke_mm: 0.254, fill: "none" });
});

test("the point editor: four points with the indicator lines start-control 1 and control 2-end; a drag moves one point only", () => {
  const g = bezierGraphic({ start: [0, 0], c1: [1_000_000, -1_000_000], c2: [2_000_000, -1_000_000], end: [3_000_000, 0] }, 1, 1, 0);
  const e = bezierEditPoints(g);
  assert.deepEqual(e.points, [[0, 0], [1_000_000, -1_000_000], [2_000_000, -1_000_000], [3_000_000, 0]]);
  assert.deepEqual(e.lines, [[[0, 0], [1_000_000, -1_000_000]], [[2_000_000, -1_000_000], [3_000_000, 0]]]);
  const moved = bezierWithPoint(g, 1, [500_000, -2_540_000]);
  assert.deepEqual([moved.start, moved.c1, moved.c2, moved.end], [g.start, { x: 0.5, y: 2.54 }, g.c2, g.end]);
  assert.deepEqual(bezierWithPoint(g, 3, [3_810_000, 0]).end, { x: 3.81, y: 0 });
});
