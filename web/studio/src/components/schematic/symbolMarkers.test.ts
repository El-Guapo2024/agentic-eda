import { test } from "node:test";
import assert from "node:assert/strict";
import { bodyBoundsOf, dnpCross, simExclusionMark } from "./symbolMarkers";

test("the body box covers the graphics only", () => {
  const box = bodyBoundsOf([
    { kind: "rectangle", start: [-1000, -2540], end: [1000, 2540], strokeWidthUm: 254, fill: "none" },
    { kind: "circle", center: [0, 0], radiusUm: 3000, strokeWidthUm: 254, fill: "none" },
  ]);
  assert.deepEqual(box, { minX: -3000, minY: -3000, maxX: 3000, maxY: 3000 });
  assert.equal(bodyBoundsOf([]), null);
});

test("the DNP cross widens the body by 60 % of what the pins add, corner to corner", () => {
  // a body 2 mm x 5 mm with pins sticking out 2.5 mm above and below
  const body = { minX: -1000, minY: -2500, maxX: 1000, maxY: 2500 };
  const all = { minX: -1000, minY: -5000, maxX: 1000, maxY: 5000 };
  // margins: x 0, y 2500 -> x = max(0, 750) = 750, y = max(1500, 225) = 1500
  assert.deepEqual(dnpCross(body, all), [
    [[-1750, -4000], [1750, 4000]],
    [[1750, -4000], [-1750, 4000]],
  ]);
});

test("a symbol whose pins do not stick out still gets a cross over its body", () => {
  const b = { minX: 0, minY: 0, maxX: 4000, maxY: 2000 };
  assert.deepEqual(dnpCross(b, b), [
    [[0, 0], [4000, 2000]],
    [[4000, 0], [0, 2000]],
  ]);
});

test("the simulation mark frames the body and puts its circle past the bottom right corner", () => {
  const m = simExclusionMark({ minX: 0, minY: 0, maxX: 4000, maxY: 2000 });
  assert.equal(m.pen, 635);
  assert.deepEqual(m.frame, { minX: -318, minY: -318, maxX: 4318, maxY: 2318 });
  assert.deepEqual(m.center, [4318 + 1270 + 635, 2318 - 1270]);
  assert.equal(m.radius, 1270);
  assert.deepEqual(m.curve[0], [m.center[0] - 1270, m.center[1]]);
  assert.deepEqual(m.curve[3], [m.center[0] + 1270, m.center[1]]);
});
