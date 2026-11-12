import { test } from "node:test";
import assert from "node:assert/strict";
import { booleanProcessOrder, zonePriorityCanMove } from "./pcbHandlerRules";

test("the last-added shape goes to the front of a polygon boolean, wherever it was", () => {
  assert.deepEqual(booleanProcessOrder(["a", "b", "c"], "c"), ["c", "b", "a"]);
  assert.deepEqual(booleanProcessOrder(["a", "b", "c"], "b"), ["b", "a", "c"]);
  assert.deepEqual(booleanProcessOrder(["a", "b", "c"], "a"), ["a", "b", "c"]);
  assert.deepEqual(booleanProcessOrder(["a", "b"], undefined), ["a", "b"]);
  assert.deepEqual(booleanProcessOrder(["a", "b"], "zz"), ["a", "b"]);
});

test("a zone moves up only past an overlapping zone above it, down only past one below", () => {
  const zone = { priority: 2 };
  assert.equal(zonePriorityCanMove(zone, [{ priority: 3 }], "top"), true);
  assert.equal(zonePriorityCanMove(zone, [{ priority: 3 }], "raise"), true);
  assert.equal(zonePriorityCanMove(zone, [{ priority: 3 }], "lower"), false);
  assert.equal(zonePriorityCanMove(zone, [{ priority: 1 }], "bottom"), true);
  assert.equal(zonePriorityCanMove(zone, [{ priority: 2 }], "raise"), false);
  assert.equal(zonePriorityCanMove(zone, [], "lower"), false);
});
