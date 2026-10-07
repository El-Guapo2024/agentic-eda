import { test } from "node:test";
import assert from "node:assert/strict";
import { SMALL_CROSS_SIZE_PX, crossHairModeForAction, crosshairSegments, cursorVisible } from "./crosshair";

test("small cross: 80 px across, centred on the cursor", () => {
  const s = crosshairSegments("small", 100, 50, 800, 600);
  assert.equal(SMALL_CROSS_SIZE_PX, 80);
  assert.deepEqual(s, [
    [60, 50, 140, 50],
    [100, 10, 100, 90],
  ]);
});

test("full cross: spans the whole window through the cursor", () => {
  assert.deepEqual(crosshairSegments("full", 100, 50, 800, 600), [
    [0, 50, 800, 50],
    [100, 0, 100, 600],
  ]);
});

test("45 degree cross: two oversized diagonals through the cursor, one rising and one falling", () => {
  const [a, b] = crosshairSegments("diag45", 100, 50, 800, 600);
  // the cairo code draws from p - d to p + d with d = width + height
  const d = 1400;
  assert.deepEqual(a, [100 - d, 50 - d, 100 + d, 50 + d]);
  assert.deepEqual(b, [100 - d, 50 + d, 100 + d, 50 - d]);
  // both pass through the cursor: midpoint of each segment is the cursor
  for (const s of [a!, b!]) assert.deepEqual([(s[0] + s[2]) / 2, (s[1] + s[3]) / 2], [100, 50]);
  // the slopes are +1 and -1 (45 and 135 degrees)
  assert.equal((a![3] - a![1]) / (a![2] - a![0]), 1);
  assert.equal((b![3] - b![1]) / (b![2] - b![0]), -1);
});

test("the cursor shows when a tool wants it or when it is forced on", () => {
  assert.equal(cursorVisible(true, false), true);
  assert.equal(cursorVisible(false, true), true);
  assert.equal(cursorVisible(false, false), false);
});

test("each Cursor ... Crosshairs action sets its own mode", () => {
  assert.equal(crossHairModeForAction("cursorSmallCrosshairs"), "small");
  assert.equal(crossHairModeForAction("cursorFullCrosshairs"), "full");
  assert.equal(crossHairModeForAction("cursor45Crosshairs"), "diag45");
});
