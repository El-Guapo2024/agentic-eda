import { test } from "node:test";
import assert from "node:assert/strict";
import { leaderModeOf, zoneClick, zoneFinish, zoneLeader, zoneLoop } from "./pcbDrawZone";

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

test("finishing a zone in the 45-degree mode adds the loop's bend (ZONE_CREATE_HELPER::OnComplete, round 5 A5-5)", () => {
  // (0,0), (10,0), (10,10), then a click at (3,7): from (10,10) the leader runs straight to (6,10) and 45 degrees on to (3,7), both locked in.
  let pts: [number, number][] = [[0, 0]];
  for (const at of [[10, 0], [10, 10], [3, 7]] as [number, number][]) {
    const c = zoneClick(pts, at, "deg45");
    assert.equal(c.finish, false);
    if (!c.finish) pts = c.pts;
  }
  assert.deepEqual(pts, [[0, 0], [10, 0], [10, 10], [6, 10], [3, 7]]);
  // A double-click at (3,7): the closing edge to (0,0) is bent onto 45/90 legs, as the preview's loop shows it.
  const loop = zoneLoop(pts, [3, 7], "deg45");
  assert.ok(loop.length === 3, `the loop has a bend: ${JSON.stringify(loop)}`);
  const out = zoneFinish(pts, [3, 7], "deg45")!;
  // The loop goes 45 degrees to (0,4) and down to (0,0); (3,7) is then on the line (6,10)-(0,4), and `Simplify` drops it.
  assert.deepEqual(out, [[0, 0], [10, 0], [10, 10], [6, 10], [0, 4]]);
  assert.ok(out.some((p) => p[0] === loop[1]![0] && p[1] === loop[1]![1]), "the loop's corner is in the outline");
  // Free mode: the corners as clicked, the straight closing edge.
  assert.deepEqual(zoneFinish(pts, [3, 7], "direct"), pts);
});

test("finishing simplifies the outline: collinear and repeated corners go, the start too when it is on its neighbours' line", () => {
  assert.deepEqual(zoneFinish([[0, 0], [5, 0], [10, 0], [10, 10]], [10, 10], "direct"), [[0, 0], [10, 0], [10, 10]]);
  assert.deepEqual(zoneFinish([[0, 0], [10, 0], [10, 0], [10, 10]], [10, 10], "direct"), [[0, 0], [10, 0], [10, 10]]);
  assert.deepEqual(zoneFinish([[5, 0], [10, 0], [10, 10], [0, 10], [0, 0]], [0, 0], "direct"), [[10, 0], [10, 10], [0, 10], [0, 0]]);
  assert.equal(zoneFinish([[0, 0], [10, 0]], [10, 0], "direct"), null, "fewer than three corners: no zone");
});
