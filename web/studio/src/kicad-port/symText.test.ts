import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_SYMBOL_TEXT_SIZE_MM, noPrintableChars, textAngleDeg, textSizeError } from "./symText";

test("a text of only blanks has no printable characters", () => {
  assert.equal(noPrintableChars(""), true);
  assert.equal(noPrintableChars("  \t "), true);
  assert.equal(noPrintableChars(" A "), false);
});

test("the text size must stay visible: 0.01 mm to 1000 mm", () => {
  assert.equal(textSizeError(DEFAULT_SYMBOL_TEXT_SIZE_MM), null);
  assert.equal(textSizeError(0.01), null);
  assert.equal(textSizeError(1000), null);
  assert.notEqual(textSizeError(0.009), null);
  assert.notEqual(textSizeError(1000.1), null);
  assert.notEqual(textSizeError(Number.NaN), null);
});

test("the dialog offers horizontal and vertical text only", () => {
  assert.equal(textAngleDeg(false), 0);
  assert.equal(textAngleDeg(true), 90);
});
