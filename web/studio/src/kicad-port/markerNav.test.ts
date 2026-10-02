import { test } from "node:test";
import assert from "node:assert/strict";
import { nextMarker, sortMarkers } from "./markerNav";

const M = [
  { key: "c", x: 300, y: 0 },
  { key: "a", x: 100, y: 50 },
  { key: "b", x: 100, y: 10 },
];

test("sortMarkers: x then y then key", () => {
  assert.deepEqual(sortMarkers(M).map((m) => m.key), ["b", "a", "c"]);
});

test("nextMarker walks the sorted list, then reports the end and wraps on the next call", () => {
  const r1 = nextMarker(M, null);
  assert.equal(r1.marker?.key, "b");
  const r2 = nextMarker(M, r1.cursor);
  assert.equal(r2.marker?.key, "a");
  const r3 = nextMarker(M, r2.cursor);
  assert.equal(r3.marker?.key, "c");
  const end = nextMarker(M, r3.cursor);
  assert.equal(end.marker, null);
  assert.equal(end.endReached, true);
  assert.equal(end.cursor, null);
  assert.equal(nextMarker(M, end.cursor).marker?.key, "b");
});

test("nextMarker: no markers -> end reached; stale cursor restarts; reversed walks backwards", () => {
  assert.equal(nextMarker([], null).endReached, true);
  assert.equal(nextMarker(M, "gone").marker?.key, "b");
  assert.equal(nextMarker(M, null, true).marker?.key, "c");
});
