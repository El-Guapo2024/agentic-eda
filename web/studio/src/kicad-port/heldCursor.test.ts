import { test } from "node:test";
import assert from "node:assert/strict";
import { heldCursor } from "./heldCursor";

test("the cursor of a move is the held point moved by the pointer's travel", () => {
  // Grabbed at (5500, 6500) by an item whose held point is (5500, 6750): the pointer 1360 um to the right is the held point 1360 um to the right.
  assert.deepEqual(heldCursor([5500, 6750], [5500, 6500], [6860, 6500]), [6860, 6750]);
  assert.deepEqual(heldCursor([5500, 6750], [5500, 6500], [5500, 6500]), [5500, 6750], "no travel: the held point itself");
  assert.deepEqual(heldCursor([0, 0], [100, 200], [90, 230]), [-10, 30]);
});

test("the item stays under the pointer: held minus cursor is grabbed minus pointer", () => {
  for (const [held, grabbed, pointer] of [
    [[1000, 2000], [1300, 2100], [4000, -500]],
    [[-5, 7], [0, 0], [123.5, -45.25]],
  ] as const) {
    const c = heldCursor(held, grabbed, pointer);
    assert.equal(held[0] - c[0], grabbed[0] - pointer[0]);
    assert.equal(held[1] - c[1], grabbed[1] - pointer[1]);
  }
});
