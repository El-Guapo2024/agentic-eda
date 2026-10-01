import { test } from "node:test";
import assert from "node:assert/strict";
import { findNearestCorner, findNearestEdgeInsertionIndex, insertCorner, moveCorner, removeCorner } from "./zonePointEditor";

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
