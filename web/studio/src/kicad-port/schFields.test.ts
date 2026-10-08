import { test } from "node:test";
import assert from "node:assert/strict";
import { fieldAnchors, isVerticalTwoPin } from "./schFields";

const standing = [
  { tip: [0, -3_810] as [number, number], root: [0, -2_540] as [number, number] },
  { tip: [0, 3_810] as [number, number], root: [0, 2_540] as [number, number] },
];

test("a symbol of two pins that stand up is a vertical two-pin", () => {
  assert.equal(isVerticalTwoPin(standing), true);
  assert.equal(isVerticalTwoPin([{ tip: [-2_540, 0], root: [-1_270, 0] }, { tip: [2_540, 0], root: [1_270, 0] }]), false, "a diode lying down is not");
  assert.equal(isVerticalTwoPin([...standing, standing[0]!]), false, "three pins are not");
});

test("a standing resistor keeps its text beside the body, ref above the middle line and value below it", () => {
  const a = fieldAnchors({ minX: -1_016, minY: -3_810, maxX: 1_016, maxY: 3_810 }, true);
  assert.equal(a.justify, "left");
  assert.deepEqual(a.ref, [1_816, -300]);
  assert.deepEqual(a.value, [1_816, 1_500]);
  assert.ok(a.footprint[1] > a.value[1], "the footprint sits under the value");
});

test("any other symbol keeps its text above and below, centred", () => {
  const a = fieldAnchors({ minX: 0, minY: 0, maxX: 10_160, maxY: 7_620 }, false);
  assert.equal(a.justify, "center");
  assert.deepEqual(a.ref, [5_080, -400]);
  assert.deepEqual(a.value, [5_080, 9_420]);
});
