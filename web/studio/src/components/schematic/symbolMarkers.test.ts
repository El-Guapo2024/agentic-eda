import { test } from "node:test";
import assert from "node:assert/strict";
import { bodyBoundsOf, dnpCross, simExclusionMark } from "./symbolMarkers";

test("the body box covers the graphics only", () => {
  const box = bodyBoundsOf([
    { kind: "rectangle", start: [-1_000_000, -2_540_000], end: [1_000_000, 2_540_000], strokeWidthNm: 254_000, fill: "none" },
    { kind: "circle", center: [0, 0], radiusNm: 3_000_000, strokeWidthNm: 254_000, fill: "none" },
  ]);
  assert.deepEqual(box, { minX: -3_000_000, minY: -3_000_000, maxX: 3_000_000, maxY: 3_000_000 });
  assert.equal(bodyBoundsOf([]), null);
});

test("the DNP cross widens the body by 60 % of what the pins add, corner to corner", () => {
  // a body 2 mm x 5 mm with pins sticking out 2.5 mm above and below
  const body = { minX: -1_000_000, minY: -2_500_000, maxX: 1_000_000, maxY: 2_500_000 };
  const all = { minX: -1_000_000, minY: -5_000_000, maxX: 1_000_000, maxY: 5_000_000 };
  // margins: x 0, y 2500 -> x = max(0, 750) = 750, y = max(1500, 225) = 1500
  assert.deepEqual(dnpCross(body, all), [
    [[-1_750_000, -4_000_000], [1_750_000, 4_000_000]],
    [[1_750_000, -4_000_000], [-1_750_000, 4_000_000]],
  ]);
});

test("a symbol whose pins do not stick out still gets a cross over its body", () => {
  const b = { minX: 0, minY: 0, maxX: 4_000_000, maxY: 2_000_000 };
  assert.deepEqual(dnpCross(b, b), [
    [[0, 0], [4_000_000, 2_000_000]],
    [[4_000_000, 0], [0, 2_000_000]],
  ]);
});

test("the simulation mark frames the body and puts its circle past the bottom right corner", () => {
  const m = simExclusionMark({ minX: 0, minY: 0, maxX: 4_000_000, maxY: 2_000_000 });
  assert.equal(m.pen, 635_000);
  assert.deepEqual(m.frame, { minX: -318_000, minY: -318_000, maxX: 4_318_000, maxY: 2_318_000 });
  assert.deepEqual(m.center, [4_318_000 + 1_270_000 + 635_000, 2_318_000 - 1_270_000]);
  assert.equal(m.radius, 1_270_000);
  assert.deepEqual(m.curve[0], [m.center[0] - 1_270_000, m.center[1]]);
  assert.deepEqual(m.curve[3], [m.center[0] + 1_270_000, m.center[1]]);
});
