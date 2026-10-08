import { test } from "node:test";
import assert from "node:assert/strict";
import { nextMarkerIndex, prevMarkerIndex } from "./checkerNav";

test("Next Marker: the first when none is selected, then each one, nothing after the last", () => {
  assert.equal(nextMarkerIndex(3, null), 0);
  assert.equal(nextMarkerIndex(3, 0), 1);
  assert.equal(nextMarkerIndex(3, 1), 2);
  assert.equal(nextMarkerIndex(3, 2), null);
  assert.equal(nextMarkerIndex(0, null), null);
  assert.equal(nextMarkerIndex(3, 9), 0, "a selection that is no longer in the list counts as none");
});

test("Previous Marker: the last when none is selected, then each one back, nothing before the first", () => {
  assert.equal(prevMarkerIndex(3, null), 2);
  assert.equal(prevMarkerIndex(3, 2), 1);
  assert.equal(prevMarkerIndex(3, 1), 0);
  assert.equal(prevMarkerIndex(3, 0), null);
  assert.equal(prevMarkerIndex(0, null), null);
  assert.equal(prevMarkerIndex(3, -1), 2);
});
