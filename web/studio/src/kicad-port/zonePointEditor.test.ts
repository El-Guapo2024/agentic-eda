import { test } from "node:test";
import assert from "node:assert/strict";
import {
  canRemoveContourCorner,
  findNearestContourCorner,
  findNearestCorner,
  findNearestEdgeInsertionIndex,
  flatVertices,
  globalIndex,
  insertCorner,
  moveContourCorner,
  moveCorner,
  neighbourIndexes,
  relativeIndex,
  removeContourCorner,
  removeCorner,
  totalVertices,
  zoneContours,
  zoneOutlineCmd,
} from "./zonePointEditor";

const square: [number, number][] = [
  [0, 0],
  [10000, 0],
  [10000, 10000],
  [0, 10000],
];

test("findNearestCorner: within tolerance picks the right index", () => {
  assert.equal(findNearestCorner(square, 100, -50, 200), 0);
  assert.equal(findNearestCorner(square, 10050, 10050, 200), 2);
});

test("findNearestCorner: outside tolerance of every corner is null", () => {
  assert.equal(findNearestCorner(square, 5000, 5000, 200), null);
});

test("findNearestEdgeInsertionIndex: a point on the top edge inserts between corners 0 and 1", () => {
  assert.equal(findNearestEdgeInsertionIndex(square, 5000, 10, 200), 1);
});

test("findNearestEdgeInsertionIndex: a point on the wrap-around edge (last -> first) inserts at the end", () => {
  assert.equal(findNearestEdgeInsertionIndex(square, 10, 5000, 200), 4);
});

test("findNearestEdgeInsertionIndex: far from every edge is null", () => {
  assert.equal(findNearestEdgeInsertionIndex(square, 5000, 5000, 200), null);
});

test("insertCorner: splices a new point at the given index without disturbing the others", () => {
  const result = insertCorner(square, 1, 5000, 10);
  assert.deepEqual(result, [
    [0, 0],
    [5000, 10],
    [10000, 0],
    [10000, 10000],
    [0, 10000],
  ]);
});

test("moveCorner: replaces exactly one point", () => {
  const result = moveCorner(square, 2, 9000, 9000);
  assert.deepEqual(result, [
    [0, 0],
    [10000, 0],
    [9000, 9000],
    [0, 10000],
  ]);
});

test("removeCorner: drops exactly one point when 4+ remain", () => {
  const result = removeCorner(square, 1);
  assert.deepEqual(result, [
    [0, 0],
    [10000, 10000],
    [0, 10000],
  ]);
});

test("removeCorner: refuses to go below 3 points", () => {
  const triangle: [number, number][] = [
    [0, 0],
    [1000, 0],
    [0, 1000],
  ];
  assert.equal(removeCorner(triangle, 0), null);
});

// ---------------------------------------------------------------------------------------- holes

// A 10 x 10 square with a square hole of four corners (global 4..7) and a triangular hole (global 8..10).
const hole4: [number, number][] = [
  [2000, 2000],
  [4000, 2000],
  [4000, 4000],
  [2000, 4000],
];
const hole3: [number, number][] = [
  [6000, 6000],
  [8000, 6000],
  [7000, 8000],
];
const holed = [square, hole4, hole3];

test("the global vertex index runs over the outline, then each hole (GetRelativeIndices / GetGlobalIndex)", () => {
  assert.equal(totalVertices(holed), 11);
  assert.deepEqual(relativeIndex(holed, 0), { contour: 0, index: 0 });
  assert.deepEqual(relativeIndex(holed, 3), { contour: 0, index: 3 });
  assert.deepEqual(relativeIndex(holed, 4), { contour: 1, index: 0 });
  assert.deepEqual(relativeIndex(holed, 10), { contour: 2, index: 2 });
  assert.equal(relativeIndex(holed, 11), null);
  assert.equal(relativeIndex(holed, -1), null);
  for (let g = 0; g < 11; g++) {
    const at = relativeIndex(holed, g)!;
    assert.equal(globalIndex(holed, at.contour, at.index), g);
  }
  assert.deepEqual(flatVertices(holed)[5], [4000, 2000]);
  // each contour is closed on itself: the last corner of the first hole is next to its first, not to the next hole's
  assert.deepEqual(neighbourIndexes(holed, 7), { prev: 6, next: 4 });
  assert.deepEqual(neighbourIndexes(holed, 8), { prev: 10, next: 9 });
});

test("zoneContours: the outline then the holes, a degenerate hole left out", () => {
  assert.deepEqual(zoneContours({ outline: square, holes: [hole4, [[0, 0], [1, 1]]] }), [square, hole4]);
  assert.deepEqual(zoneContours({ outline: square }), [square]);
});

test("a hole's corners are handles, and dragging one moves that hole only", () => {
  assert.equal(findNearestContourCorner(holed, 4050, 3950, 200), 6);
  assert.equal(findNearestContourCorner(holed, 5000, 5000, 200), null);
  const moved = moveContourCorner(holed, 6, 4500, 4500);
  assert.deepEqual(moved[0], square);
  assert.deepEqual(moved[1], [
    [2000, 2000],
    [4000, 2000],
    [4500, 4500],
    [2000, 4000],
  ]);
  assert.deepEqual(moved[2], hole3);
  assert.deepEqual(moveContourCorner(holed, 99, 0, 0), holed, "no such vertex: nothing moves");
});

test("Remove Corner: a hole loses a corner while it has more than three, a three-corner hole goes whole, a three-corner outline takes the zone", () => {
  assert.deepEqual(removeContourCorner(holed, 5), { kind: "contours", contours: [square, [[2000, 2000], [4000, 4000], [2000, 4000]], hole3] });
  assert.deepEqual(removeContourCorner(holed, 9), { kind: "contours", contours: [square, hole4] }, "RemoveContour");
  assert.deepEqual(removeContourCorner(holed, 1), { kind: "contours", contours: [[[0, 0], [10000, 10000], [0, 10000]], hole4, hole3] });
  const triangle: [number, number][] = [
    [0, 0],
    [10000, 0],
    [0, 10000],
  ];
  assert.deepEqual(removeContourCorner([triangle, hole3], 0), { kind: "remove_item" }, "commit.Remove( item )");
  assert.equal(removeContourCorner(holed, 11), null);
});

test("CanRemoveCorner: any corner of a hole, a corner of the outline only while more than three remain", () => {
  assert.equal(canRemoveContourCorner(holed, 2), true);
  assert.equal(canRemoveContourCorner(holed, 9), true, "the last corners of a hole can go (the hole goes)");
  const triangle: [number, number][] = [
    [0, 0],
    [10000, 0],
    [0, 10000],
  ];
  assert.equal(canRemoveContourCorner([triangle, hole4], 1), false);
  assert.equal(canRemoveContourCorner([triangle, hole4], 4), true);
  assert.equal(canRemoveContourCorner(holed, 42), false);
});

test("zoneOutlineCmd: the holes ride along when the zone has or had some", () => {
  assert.deepEqual(zoneOutlineCmd("z", [square], false), { op: "set_zone_outline", id: "z", outline: square.map(([x, y]) => ({ x, y })) });
  assert.deepEqual(zoneOutlineCmd("z", [square], true), { op: "set_zone_outline", id: "z", outline: square.map(([x, y]) => ({ x, y })), holes: [] }, "the last hole removed");
  assert.deepEqual(zoneOutlineCmd("z", [square, hole3], true).holes, [hole3.map(([x, y]) => ({ x, y }))]);
});
