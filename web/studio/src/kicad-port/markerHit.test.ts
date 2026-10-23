import { test } from "node:test";
import assert from "node:assert/strict";
import { DRC_MARKER_HIT_NM, nearestMarker } from "./markerHit";

test("a right click finds the nearest marker whose circle holds the point", () => {
  const points = [[0, 0], [1_000_000, 0], null, [1_000_000, 200_000]] as const;
  assert.equal(nearestMarker(points, 10_000, 10_000, DRC_MARKER_HIT_NM), 0);
  assert.equal(nearestMarker(points, 1_000_000, 150_000, DRC_MARKER_HIT_NM), 3000, "two circles hold it: the nearer centre wins");
  assert.equal(nearestMarker(points, 1_000_000, 60_000, DRC_MARKER_HIT_NM), 1);
});

test("a click outside every circle, or on a marker with no place on the canvas, finds nothing", () => {
  const points = [[0, 0], null] as const;
  assert.equal(nearestMarker(points, 5_000_000, 5_000_000, DRC_MARKER_HIT_NM), -1);
  assert.equal(nearestMarker(points, DRC_MARKER_HIT_NM + 1, 0, DRC_MARKER_HIT_NM), -1, "just outside the circle");
  assert.equal(nearestMarker([], 0, 0, DRC_MARKER_HIT_NM), -1);
});
