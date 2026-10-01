import { test } from "node:test";
import assert from "node:assert/strict";
import { alignAxis, alignDeltas, getDeltasForDistributeByGaps, getDeltasForDistributeByPoints, type Box } from "./alignDistribute";

test("alignAxis: left/right/centerX are x, everything else is y", () => {
  assert.equal(alignAxis("left"), "x");
  assert.equal(alignAxis("right"), "x");
  assert.equal(alignAxis("centerX"), "x");
  assert.equal(alignAxis("top"), "y");
  assert.equal(alignAxis("bottom"), "y");
  assert.equal(alignAxis("centerY"), "y");
});

test("alignDeltas: fewer than 2 boxes is a no-op", () => {
  const boxes: Box[] = [[0, 0, 1000, 1000]];
  assert.deepEqual(alignDeltas(boxes, "top"), [0]);
  assert.deepEqual(alignDeltas([], "top"), []);
});

test("alignDeltas: top targets the smallest (topmost) top edge", () => {
  const boxes: Box[] = [
    [0, 500, 1000, 1500], // top = 500
    [2000, 0, 3000, 1000], // top = 0 (the extreme)
    [4000, 1000, 5000, 2000], // top = 1000
  ];
  assert.deepEqual(alignDeltas(boxes, "top"), [-500, 0, -1000]);
});

test("alignDeltas: bottom targets the largest (bottommost) bottom edge", () => {
  const boxes: Box[] = [
    [0, 0, 1000, 1000], // bottom = 1000
    [2000, 0, 3000, 2000], // bottom = 2000 (the extreme)
  ];
  assert.deepEqual(alignDeltas(boxes, "bottom"), [1000, 0]);
});

test("alignDeltas: left targets the smallest left edge, right targets the largest right edge", () => {
  const boxes: Box[] = [
    [500, 0, 1500, 1000],
    [0, 0, 2000, 1000],
  ];
  assert.deepEqual(alignDeltas(boxes, "left"), [-500, 0]);
  assert.deepEqual(alignDeltas(boxes, "right"), [500, 0]);
});

test("alignDeltas: centerX/centerY target the smallest center, same convention as top/left", () => {
  const boxes: Box[] = [
    [0, 0, 1000, 1000], // center (500, 500) -- the extreme (smallest) on both axes
    [2000, 4000, 2200, 4200], // center (2100, 4100)
  ];
  assert.deepEqual(alignDeltas(boxes, "centerX"), [0, -1600]);
  assert.deepEqual(alignDeltas(boxes, "centerY"), [0, -3600]);
});

test("getDeltasForDistributeByGaps: fewer than 3 items is a no-op", () => {
  assert.deepEqual(getDeltasForDistributeByGaps([]), []);
  assert.deepEqual(
    getDeltasForDistributeByGaps([
      [0, 100],
      [900, 1000],
    ]),
    [0, 0]
  );
});

test("getDeltasForDistributeByGaps: end caps never move; the middle item centers the leftover gap", () => {
  // Three 100-wide items: first at [0,100], last at [900,1000] (fixed),
  // middle starts at [400,500] -- the even-gap position for a 100-wide
  // middle item between them is [450,550], a +50 shift.
  const extents: [number, number][] = [
    [0, 100],
    [400, 500],
    [900, 1000],
  ];
  assert.deepEqual(getDeltasForDistributeByGaps(extents), [0, 50, 0]);
});

test("getDeltasForDistributeByGaps: four items split the remaining gap evenly between the two middle ones", () => {
  // Span from the first item's end (100) to the last item's start (900) is
  // 800, minus the two middle items' own 100+100 width = 600 left over,
  // split into 3 equal gaps of 200 each.
  const extents: [number, number][] = [
    [0, 100],
    [300, 400],
    [700, 800],
    [900, 1000],
  ];
  const deltas = getDeltasForDistributeByGaps(extents);
  assert.equal(deltas[0], 0);
  assert.equal(deltas[3], 0);
  // item 1 (currently at 300) should land at 100+200=300 -- already there.
  assert.equal(deltas[1], 0);
  // item 2 (currently at 700) should land at 300+100(item1 width)+200=600.
  assert.equal(deltas[2], -100);
});

test("getDeltasForDistributeByPoints: fewer than 3 points is a no-op", () => {
  assert.deepEqual(getDeltasForDistributeByPoints([0, 1000]), [0, 0]);
});

test("getDeltasForDistributeByPoints: evenly spaces the middle points between the first and last, end caps fixed", () => {
  // 0, 100, 300, 1000 -- evenly spaced 4 points over [0,1000] land at 0, 333, 667, 1000.
  const deltas = getDeltasForDistributeByPoints([0, 100, 300, 1000]);
  assert.equal(deltas[0], 0);
  assert.equal(deltas[3], 0);
  assert.equal(deltas[1], 233); // 100 -> 333
  assert.equal(deltas[2], 367); // 300 -> 667
});
