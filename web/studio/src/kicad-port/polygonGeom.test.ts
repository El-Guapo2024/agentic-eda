import { test } from "node:test";
import assert from "node:assert/strict";
import { addPoint, build45DegLeader, build90DegLeader, deleteLastCorner, finalOutline, isPolygonInProgress, newPointClosesOutline, newPolyGeom, setCursorPosition, simplifyClosed, snapVector45, withMode, type Pt } from "./polygonGeom";

/** Click the corners in order the way the tool does: the cursor moves to the point (laying out the leader), then the click locks it in. */
function click(g: ReturnType<typeof newPolyGeom>, pts: Pt[]) {
  let geom = g;
  for (const p of pts) {
    geom = setCursorPosition(geom, p);
    geom = addPoint(geom, p);
  }
  return geom;
}

test("snapVector45 puts a vector on the nearest axis or diagonal", () => {
  assert.deepEqual(snapVector45([100, 30]), [100, 0]);
  assert.deepEqual(snapVector45([30, -100]), [0, -100]);
  assert.deepEqual(snapVector45([100, 80]), [100, 100]);
  assert.deepEqual(snapVector45([-60, 100]), [-100, 100]);
  assert.deepEqual(snapVector45([100, 30], true), [100, 100]);
});

test("the first leader runs from the corner to the cursor snapped onto a 45-degree direction", () => {
  assert.deepEqual(build45DegLeader([100, 30], [[0, 0]]), [[0, 0], [100, 0]]);
  assert.deepEqual(build45DegLeader([100, 80], [[0, 0]]), [[0, 0], [100, 100]]);
});

test("after a horizontal edge the leader bends so both segments stay on 45-degree multiples", () => {
  // From (100,0), having come along the x axis, to a cursor at (160, 40): a 45-degree run then the axis (or the reverse), ending at the cursor.
  const leader = build45DegLeader([160, 40], [[0, 0], [100, 0]]);
  assert.equal(leader.length, 3);
  assert.deepEqual(leader[0], [100, 0]);
  assert.deepEqual(leader[2], [160, 40]);
  for (let i = 0; i < 2; i++) {
    const dx = leader[i + 1]![0] - leader[i]![0];
    const dy = leader[i + 1]![1] - leader[i]![1];
    assert.ok(dx === 0 || dy === 0 || Math.abs(dx) === Math.abs(dy), `segment ${i} is on a 45-degree multiple`);
  }
});

test("the 90-degree leader is one segment on an axis, otherwise a horizontal then a vertical one", () => {
  assert.deepEqual(build90DegLeader([50, 0], [[0, 0]]), [[0, 0], [50, 0]]);
  assert.deepEqual(build90DegLeader([50, 70], [[0, 0]]), [[0, 0], [50, 0], [50, 70]]);
});

test("direct mode locks in exactly the clicked corners", () => {
  const g = click(newPolyGeom("direct"), [[0, 0], [100, 0], [100, 100], [0, 100]]);
  assert.deepEqual(g.locked, [[0, 0], [100, 0], [100, 100], [0, 100]]);
  assert.deepEqual(finalOutline(g), [[0, 0], [100, 0], [100, 100], [0, 100]]);
});

test("a polygon in progress is one with a locked corner; a click on the first corner closes it", () => {
  let g = newPolyGeom("direct");
  assert.equal(isPolygonInProgress(g), false);
  g = click(g, [[0, 0], [100, 0], [100, 100]]);
  assert.equal(isPolygonInProgress(g), true);
  assert.equal(newPointClosesOutline(g, [0, 0]), true);
  assert.equal(newPointClosesOutline(g, [1, 0]), false);
});

test("fewer than three corners scrap the rule area", () => {
  assert.equal(finalOutline(click(newPolyGeom("direct"), [[0, 0], [100, 0]])), null);
});

test("collinear corners are simplified away", () => {
  const g = click(newPolyGeom("direct"), [[0, 0], [50, 0], [100, 0], [100, 100], [0, 100]]);
  assert.deepEqual(finalOutline(g), [[0, 0], [100, 0], [100, 100], [0, 100]]);
  assert.deepEqual(simplifyClosed([[0, 0], [10, 0], [10, 10], [0, 10]], 1), [[0, 0], [10, 0], [10, 10], [0, 10]]);
});

test("a start corner that lies on the line between its neighbours is dropped too", () => {
  // The corners start in the middle of the top edge: (50,0) -> (100,0) -> (100,100) -> (0,100) -> (0,0), closing back to (50,0).
  const g = click(newPolyGeom("direct"), [[50, 0], [100, 0], [100, 100], [0, 100], [0, 0]]);
  assert.deepEqual(finalOutline(g), [[100, 0], [100, 100], [0, 100], [0, 0]]);
});

test("a 45-degree outline keeps the bends the preview showed, including the closing path", () => {
  let g = newPolyGeom("deg45");
  g = click(g, [[0, 0], [100, 0], [100, 100]]);
  // The cursor sits at (0, 100): the closing path back to (0,0) is straight, the leader from (100,100) goes straight along the axis.
  g = setCursorPosition(g, [0, 100]);
  const outline = finalOutline(g)!;
  assert.deepEqual(outline, [[0, 0], [100, 0], [100, 100], [0, 100]]);
});

test("delete last corner steps back one corner, then runs out", () => {
  let g = click(newPolyGeom("direct"), [[0, 0], [100, 0], [100, 100]]);
  let r = deleteLastCorner(g);
  assert.deepEqual(r.last, [100, 100]);
  assert.deepEqual(r.geom.locked, [[0, 0], [100, 0]]);
  r = deleteLastCorner(r.geom);
  r = deleteLastCorner(r.geom);
  assert.equal(isPolygonInProgress(r.geom), false);
  assert.equal(deleteLastCorner(r.geom).last, null);
});

test("changing the leader mode re-lays the next cursor position out in the new mode", () => {
  const g = click(newPolyGeom("direct"), [[0, 0]]);
  const m = setCursorPosition(withMode(g, "deg45"), [100, 30]);
  assert.deepEqual(m.leader, [[0, 0], [100, 0]]);
});
