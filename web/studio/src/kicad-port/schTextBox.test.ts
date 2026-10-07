import { test } from "node:test";
import assert from "node:assert/strict";
import { defaultTextBoxMargin, layoutTextBox, wrapText } from "./schTextBox";

/** Every character is `size` wide: easy arithmetic. */
const measure = (text: string, size: number) => text.length * size;

test("the default margin is half the stroke plus three quarters of the text height", () => {
  assert.equal(defaultTextBoxMargin(1_270, 0), 953);
  assert.equal(defaultTextBoxMargin(1_270, 200), 1_053);
});

test("wrapping breaks at spaces and keeps explicit newlines", () => {
  assert.deepEqual(wrapText("aa bb cc", 5, 1, measure), ["aa bb", "cc"]);
  assert.deepEqual(wrapText("aa\nbb", 100, 1, measure), ["aa", "bb"]);
  assert.deepEqual(wrapText("abcdefgh ij", 4, 1, measure), ["abcdefgh", "ij"], "an over-long word stays whole");
});

test("horizontal text: the first baseline sits a cap height below the top margin, lines one interline pitch apart", () => {
  const l = layoutTextBox({ start: [0, 0], end: [10_000, 5_000], text: "one\ntwo", angle: 0, sizeUm: 1_000, hAlign: "left", vAlign: "top", marginUm: 500 }, measure);
  assert.equal(l.vertical, false);
  assert.deepEqual(l.origin, [0, 0]);
  assert.equal(l.lines.length, 2);
  assert.equal(l.lines[0]!.u, 500);
  assert.equal(Math.round(l.lines[0]!.v), 500 + 950);
  assert.equal(Math.round(l.lines[1]!.v - l.lines[0]!.v), 1_680);
});

test("alignment: right aligns to the right margin, bottom to the bottom margin", () => {
  const l = layoutTextBox({ start: [0, 0], end: [10_000, 5_000], text: "x", angle: 0, sizeUm: 1_000, hAlign: "right", vAlign: "bottom", marginUm: 500 }, measure);
  assert.equal(l.lines[0]!.u, 9_500);
  assert.equal(l.lines[0]!.justify, "right");
  assert.equal(Math.round(l.lines[0]!.v), 5_000 - 500, "the baseline is the bottom margin's line");
});

test("vertical text lays its frame out from the bottom-left corner, along the box's height", () => {
  const l = layoutTextBox({ start: [0, 0], end: [4_000, 10_000], text: "abcdefghijklmnopqrstuvwxyz", angle: 90_000, sizeUm: 1_000, hAlign: "left", vAlign: "top", marginUm: 500 }, measure);
  assert.equal(l.vertical, true);
  assert.deepEqual(l.origin, [0, 10_000]);
  // 10 mm of height minus two 0.5 mm margins: 9 characters per line at 1 mm each, but the word is one long token.
  assert.equal(l.lines[0]!.u, 500);
});
