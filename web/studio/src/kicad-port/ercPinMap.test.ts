import { test } from "node:test";
import assert from "node:assert/strict";
import { PINMAP_TYPE_COUNT, PIN_TYPE_LABELS, levelTooltip, nextLevel, triangleCells } from "./ercPinMap";

test("nextLevel cycles OK -> Warning -> Error -> OK (changeErrorLevel)", () => {
  assert.equal(nextLevel(0), 1);
  assert.equal(nextLevel(1), 2);
  assert.equal(nextLevel(2), 0);
});

test("the panel is an 11-type lower triangle (NC excluded)", () => {
  assert.equal(PINMAP_TYPE_COUNT, 11);
  assert.equal(PIN_TYPE_LABELS.length, 12);
  const m = Array.from({ length: 12 }, () => Array.from({ length: 12 }, () => 0));
  const cells = triangleCells(m);
  assert.equal(cells.length, (11 * 12) / 2);
  assert.ok(cells.every((c) => c.col <= c.row && c.row < 11));
});

test("triangleCells reads the lower triangle and treats junk as OK", () => {
  const m = Array.from({ length: 12 }, () => Array.from({ length: 12 }, () => 0));
  m[1]![1] = 2; // output <-> output
  m[6]![1] = 1;
  m[2]![0] = 7;
  const cells = triangleCells(m);
  assert.equal(cells.find((c) => c.row === 1 && c.col === 1)?.level, 2);
  assert.equal(cells.find((c) => c.row === 6 && c.col === 1)?.level, 1);
  assert.equal(cells.find((c) => c.row === 2 && c.col === 0)?.level, 0);
});

test("tooltips match setDRCMatrixButtonState", () => {
  assert.equal(levelTooltip(0), "No error or warning");
  assert.equal(levelTooltip(1), "Generate warning");
  assert.equal(levelTooltip(2), "Generate error");
});
