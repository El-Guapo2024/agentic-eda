import { test } from "node:test";
import assert from "node:assert/strict";
import { bridgeHoles, insidePolygonWithHoles } from "./polyHoles";

const sq = (x0: number, y0: number, x1: number, y1: number): [number, number][] => [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];

test("PP10: a polygon's hole is bridged into one ring the backend takes apart, and is outside the polygon", () => {
  const outline = sq(0, 0, 10, 10);
  const hole = sq(3, 3, 6, 6);
  const ring = bridgeHoles(outline, [hole]);
  // the outline's 4 corners, the bridge out to the hole, its 4 corners and back to where it started, and the bridge back
  assert.equal(ring.length, 4 + 5 + 1);
  // the bridge's two sides run opposite ways over the same points (what `unfractureSingle` pairs)
  const i = ring.findIndex((p) => p[0] === 3 && p[1] === 3);
  assert.deepEqual(ring[i - 1], ring[i + 5]);
  assert.deepEqual(ring[i], ring[i + 4]);
  assert.deepEqual(bridgeHoles(outline, undefined), outline);
  assert.equal(insidePolygonWithHoles(1, 1, outline, [hole]), true);
  assert.equal(insidePolygonWithHoles(4, 4, outline, [hole]), false, "in the hole");
  assert.equal(insidePolygonWithHoles(11, 4, outline, [hole]), false);
});
