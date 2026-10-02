import { test } from "node:test";
import assert from "node:assert/strict";
import { nextLineMode, computeBreakPoint, wireTail, LINE_MODE_90, LINE_MODE_45, LINE_MODE_FREE } from "./schLineMode";

const Z = { start: [0, 0] as const, end: [0, 0] as const };

test("nextLineMode cycles free -> 90 -> 45 -> free", () => {
  assert.equal(nextLineMode(0), 1);
  assert.equal(nextLineMode(1), 2);
  assert.equal(nextLineMode(2), 0);
});

test("90 mode: fresh start picks the long axis first", () => {
  assert.deepEqual(computeBreakPoint(Z, Z, [100, 40], LINE_MODE_90, false), [100, 0]);
  assert.deepEqual(computeBreakPoint(Z, Z, [40, 100], LINE_MODE_90, false), [0, 100]);
});

test("90 mode: keeps the existing first-segment shape", () => {
  const seg = { start: [0, 0] as const, end: [0, 50] as const }; // was vertical
  assert.deepEqual(computeBreakPoint(seg, Z, [100, 40], LINE_MODE_90, false), [0, 40]);
});

test("45 mode: diagonal elbow; posture flips which leg is diagonal", () => {
  assert.deepEqual(computeBreakPoint(Z, Z, [100, 40], LINE_MODE_45, false), [60, 0]);
  assert.deepEqual(computeBreakPoint(Z, Z, [100, 40], LINE_MODE_45, true), [40, 40]);
});

test("wireTail: free is one segment; degenerate elbow dropped", () => {
  assert.deepEqual(wireTail([0, 0], [100, 40], LINE_MODE_FREE, false, null), [[100, 40]]);
  assert.deepEqual(wireTail([0, 0], [100, 0], LINE_MODE_90, false, null), [[100, 0]]);
  assert.deepEqual(wireTail([0, 0], [100, 40], LINE_MODE_90, false, null), [[100, 0], [100, 40]]);
});
