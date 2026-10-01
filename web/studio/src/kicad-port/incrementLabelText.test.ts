import { test } from "node:test";
import assert from "node:assert/strict";
import { incrementLabelText } from "./incrementLabelText";

test("incrementLabelText: a trailing number increments by 1 by default", () => {
  assert.equal(incrementLabelText("DATA0"), "DATA1");
  assert.equal(incrementLabelText("D3"), "D4");
});

test("incrementLabelText: zero-padding width is preserved (IncrementString's own '%0<n>ld' format)", () => {
  assert.equal(incrementLabelText("AD00"), "AD01");
  assert.equal(incrementLabelText("AD09"), "AD10", "carrying past the padded width still keeps at least that many digits");
});

test("incrementLabelText: a suffix after the digits survives untouched", () => {
  assert.equal(incrementLabelText("DATA0_N"), "DATA1_N");
});

test("incrementLabelText: no trailing digits at all is returned completely unchanged, not suffixed with a 1", () => {
  assert.equal(incrementLabelText("RESET"), "RESET");
  assert.equal(incrementLabelText("CLK"), "CLK");
});

test("incrementLabelText: empty string is returned unchanged", () => {
  assert.equal(incrementLabelText(""), "");
});

test("incrementLabelText: a custom delta (e.g. decrementing) is honored", () => {
  assert.equal(incrementLabelText("DATA5", -1), "DATA4");
  assert.equal(incrementLabelText("DATA2", 3), "DATA5");
});

test("incrementLabelText: going below zero leaves the text unchanged rather than clamping to 0", () => {
  assert.equal(incrementLabelText("DATA0", -1), "DATA0");
});

test("incrementLabelText: a purely numeric name increments as a whole number, same zero-padding rule", () => {
  assert.equal(incrementLabelText("007"), "008");
});
