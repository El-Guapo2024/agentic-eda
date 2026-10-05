import { test } from "node:test";
import assert from "node:assert/strict";
import { polyBegin, polyContinue, polyFinish, polyPreview, type PolyPt } from "./symPolyDraw";

const A: PolyPt = [0, 0];
const B: PolyPt = [1000, 0];
const C: PolyPt = [1000, 1000];
const D: PolyPt = [0, 1000];

test("the first click starts the outline with that one vertex", () => {
  assert.deepEqual(polyBegin(A), [A]);
});

test("each click adds a vertex; a click on the previous vertex adds nothing (no zero-length segments)", () => {
  let pts = polyBegin(A);
  pts = polyContinue(pts, B);
  pts = polyContinue(pts, B); // the second click of a double click lands on the same spot
  pts = polyContinue(pts, C);
  assert.deepEqual(pts, [A, B, C]);
});

test("Enter or a double click leaves the clicked vertices as an open polyline", () => {
  const r = polyFinish([A, B, C, D]);
  assert.deepEqual(r, { pts: [A, B, C, D], closed: false });
});

test("clicking back on the start closes the outline (the last vertex is the first again)", () => {
  const r = polyFinish([A, B, C, A]);
  assert.deepEqual(r, { pts: [A, B, C, A], closed: true });
});

test("two vertices make a plain segment", () => {
  assert.deepEqual(polyFinish([A, B]), { pts: [A, B], closed: false });
});

test("a single point is no shape", () => {
  assert.equal(polyFinish([A]), null);
  assert.equal(polyFinish([]), null);
});

test("the result does not alias the working list", () => {
  const working = [A, B];
  const r = polyFinish(working)!;
  r.pts[0]![0] = 99;
  assert.equal(working[0]![0], 0);
});

test("the preview is the fixed vertices plus the floating one under the cursor", () => {
  assert.deepEqual(polyPreview([A, B], C), [A, B, C]);
  assert.deepEqual(polyPreview([A, B], null), [A, B]);
});
