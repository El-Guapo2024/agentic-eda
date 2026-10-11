import { test } from "node:test";
import assert from "node:assert/strict";
import { outlineContains, outlineDistance, type Contours } from "./padOutline";

// A ring pad as the state carries it: a 10 mm square with a 4 mm square hole (a contour of its own), and a 1 mm square anchor inside the hole.
const ring: Contours = [
  [[-5000, -5000], [5000, -5000], [5000, 5000], [-5000, 5000]],
  [[-2000, -2000], [-2000, 2000], [2000, 2000], [2000, -2000]],
  [[-500, -500], [500, -500], [500, 500], [-500, 500]],
];

test("a point is on a ring pad's copper in the band, on the anchor, and not in the hole between them or outside", () => {
  assert.equal(outlineContains(ring, 3500, 0), true, "the band");
  assert.equal(outlineContains(ring, 0, 0), true, "the anchor in the hole");
  assert.equal(outlineContains(ring, 1500, 0), false, "the hole");
  assert.equal(outlineContains(ring, 6000, 0), false, "outside");
});

test("the distance to the copper is 0 on it and the distance to the nearest edge off it, the hole's edge included", () => {
  assert.equal(outlineDistance(ring, 3500, 0), 0);
  assert.equal(outlineDistance(ring, 7000, 0), 2000, "outside, 2 mm from the right side");
  assert.equal(outlineDistance(ring, 1500, 0), 500, "in the hole, 0.5 mm from the anchor and from the band");
  assert.equal(Math.round(outlineDistance(ring, 6000, 6000)), Math.round(Math.hypot(1000, 1000)), "off a corner");
});
