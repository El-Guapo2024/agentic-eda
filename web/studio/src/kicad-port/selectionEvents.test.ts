import { test } from "node:test";
import assert from "node:assert/strict";
import { idsOfParameter, reselectIds, selectCursorResult, selectIds, unselectIds } from "./selectionEvents";

test("an event parameter names ids as a string, a list of strings or a list of records with an id", () => {
  assert.deepEqual(idsOfParameter("U1"), ["U1"]);
  assert.deepEqual(idsOfParameter(["a", "b"]), ["a", "b"]);
  assert.deepEqual(idsOfParameter([{ id: "a", kind: "track" }, { id: "b" }, { nope: 1 }, 7, null]), ["a", "b"]);
  assert.deepEqual(idsOfParameter(undefined), []);
  assert.deepEqual(idsOfParameter(42), []);
});

test("selectItem(s) adds without duplicating, unselectItem(s) removes", () => {
  assert.deepEqual(selectIds(["a"], ["b", "a", "c"]), ["a", "b", "c"]);
  assert.deepEqual(unselectIds(["a", "b", "c"], ["b", "x"]), ["a", "c"]);
  assert.deepEqual(unselectIds(["a"], []), ["a"]);
});

test("reselectItem is unselect then select: the item ends up selected, last", () => {
  assert.deepEqual(reselectIds(["a", "b", "c"], ["a"]), ["b", "c", "a"]);
  assert.deepEqual(reselectIds(["b"], ["a"]), ["b", "a"]);
});

test("selectionCursor selects what is under the cursor only when nothing is selected", () => {
  assert.deepEqual(selectCursorResult([], ["x", "y"]), ["x"]);
  assert.deepEqual(selectCursorResult(["s"], ["x"]), ["s"]);
  assert.deepEqual(selectCursorResult([], []), []);
});
