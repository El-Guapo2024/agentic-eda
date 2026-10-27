import { test } from "node:test";
import assert from "node:assert/strict";
import { shapeSecondPoint } from "./pcbShapeSnap";
import { DEFAULT_PCB_PARITY } from "./pcbParityState";

const MM = 1e6;

test("a rectangle is free in every line mode, and Ctrl makes it a square (drawShape, round 5 A5-2)", () => {
  for (const mode of ["direct", "45", "90"] as const) {
    assert.deepEqual(shapeSecondPoint("rect", mode, false, [0, 0], [10 * MM, 3 * MM]), [10 * MM, 3 * MM], mode);
  }
  assert.deepEqual(shapeSecondPoint("rect", "direct", true, [0, 0], [10 * MM, 3 * MM]), [10 * MM, 10 * MM], "only45: never on an axis");
  assert.deepEqual(shapeSecondPoint("rect", "45", true, [0, 0], [-4 * MM, 10 * MM]), [-10 * MM, 10 * MM]);
});

test("a line in the 45-degree mode keeps the cursor's x or y (GetVectorSnapped45, round 5 A5-3)", () => {
  assert.deepEqual(shapeSecondPoint("segment", "45", false, [0, 0], [10 * MM, 4 * MM]), [10 * MM, 0], "more than twice as wide as high: on the x axis");
  assert.deepEqual(shapeSecondPoint("segment", "45", false, [0, 0], [10 * MM, 7 * MM]), [10 * MM, 10 * MM], "else the diagonal, from the larger side");
  assert.deepEqual(shapeSecondPoint("segment", "45", false, [1 * MM, 1 * MM], [3 * MM, 11 * MM]), [1 * MM, 11 * MM], "on the y axis");
  assert.deepEqual(shapeSecondPoint("segment", "90", false, [0, 0], [10 * MM, 7 * MM]), [10 * MM, 0]);
  assert.deepEqual(shapeSecondPoint("segment", "45", true, [0, 0], [10 * MM, 4 * MM]), [10 * MM, 4 * MM], "Ctrl frees the angle");
  assert.deepEqual(shapeSecondPoint("segment", "direct", false, [0, 0], [10 * MM, 4 * MM]), [10 * MM, 4 * MM]);
});

test("a circle is free, and Ctrl puts its radius end on a 45-degree ray", () => {
  assert.deepEqual(shapeSecondPoint("circle", "45", false, [0, 0], [10 * MM, 4 * MM]), [10 * MM, 4 * MM]);
  assert.deepEqual(shapeSecondPoint("circle", "direct", true, [0, 0], [10 * MM, 4 * MM]), [10 * MM, 0]);
});

test("the studio starts in KiCad's free-angle line mode (PCBNEW_SETTINGS m_AngleSnapMode, round 5 A5-4)", () => {
  assert.equal(DEFAULT_PCB_PARITY.angleSnapMode, "direct");
});
