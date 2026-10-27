import { test } from "node:test";
import assert from "node:assert/strict";
import { leaderModeOf, zoneClick, zoneLeader } from "./pcbDrawZone";

test("a click on the first corner closes the zone or polygon (NewPointClosesOutline, round 4 C-4)", () => {
  const pts: [number, number][] = [[0, 0], [10, 0], [10, 10]];
  assert.deepEqual(zoneClick(pts, [0, 0], "direct"), { finish: true });
  assert.deepEqual(zoneClick(pts, [0, 10], "direct"), { finish: false, pts: [...pts, [0, 10]] });
  assert.deepEqual(zoneClick([], [5, 5], "deg45"), { finish: false, pts: [[5, 5]] }, "the first corner");
});

test("in the 45-degree line mode a click locks in the leader's bend and end (AddPoint)", () => {
  // From (0,0), the first leg is GetVectorSnapped45: (10,3) goes onto the axis (|x| > 2|y|).
  assert.deepEqual(zoneClick([[0, 0]], [10, 3], "deg45"), { finish: false, pts: [[0, 0], [10, 0]] });
  // From (0,0)-(10,0), a click at (20,4): a straight run then a 45-degree bend into the cursor.
  const r = zoneClick([[0, 0], [10, 0]], [20, 4], "deg45");
  assert.deepEqual(r, { finish: false, pts: [[0, 0], [10, 0], [16, 0], [20, 4]] });
  assert.deepEqual(zoneLeader([[0, 0], [10, 0]], [20, 4], "deg45"), [[10, 0], [16, 0], [20, 4]]);
  assert.deepEqual(zoneClick([[0, 0], [10, 0]], [20, 4], "deg90"), { finish: false, pts: [[0, 0], [10, 0], [20, 0], [20, 4]] });
  assert.deepEqual(zoneClick([[0, 0], [10, 0]], [20, 4], "direct"), { finish: false, pts: [[0, 0], [10, 0], [20, 4]] });
});

test("Ctrl draws free-angle whatever the line mode", () => {
  assert.equal(leaderModeOf("45", true), "direct");
  assert.equal(leaderModeOf("45", false), "deg45");
  assert.equal(leaderModeOf("90", false), "deg90");
  assert.equal(leaderModeOf("direct", false), "direct");
});
