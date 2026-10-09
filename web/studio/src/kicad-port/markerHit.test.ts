import { test } from "node:test";
import assert from "node:assert/strict";
import { DRC_MARKER_HIT_UM, nearestMarker } from "./markerHit";

test("a right click finds the nearest marker whose circle holds the point", () => {
  const points = [[0, 0], [1000, 0], null, [1000, 200]] as const;
  assert.equal(nearestMarker(points, 10, 10, DRC_MARKER_HIT_UM), 0);
  assert.equal(nearestMarker(points, 1000, 150, DRC_MARKER_HIT_UM), 3, "two circles hold it: the nearer centre wins");
  assert.equal(nearestMarker(points, 1000, 60, DRC_MARKER_HIT_UM), 1);
});

test("a click outside every circle, or on a marker with no place on the canvas, finds nothing", () => {
  const points = [[0, 0], null] as const;
  assert.equal(nearestMarker(points, 5000, 5000, DRC_MARKER_HIT_UM), -1);
  assert.equal(nearestMarker(points, DRC_MARKER_HIT_UM + 1, 0, DRC_MARKER_HIT_UM), -1, "just outside the circle");
  assert.equal(nearestMarker([], 0, 0, DRC_MARKER_HIT_UM), -1);
});
