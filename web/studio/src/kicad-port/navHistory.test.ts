import { test } from "node:test";
import assert from "node:assert/strict";
import { initialNavHistory, pushToHistory, goBack, goForward, canGoBack, canGoForward } from "./navHistory";

test("push, back, forward", () => {
  let h = initialNavHistory();
  assert.ok(!canGoBack(h) && !canGoForward(h));
  h = pushToHistory(h, ["a"]);
  h = pushToHistory(h, ["a", "b"]);
  assert.equal(h.index, 2);
  h = goBack(h)!;
  assert.deepEqual(h.entries[h.index], ["a"]);
  assert.ok(canGoForward(h));
  h = goForward(h)!;
  assert.deepEqual(h.entries[h.index], ["a", "b"]);
  assert.equal(goForward(h), null);
});

test("pushing after going back drops the forward tail; duplicates not repeated", () => {
  let h = initialNavHistory();
  h = pushToHistory(h, ["a"]);
  h = pushToHistory(h, ["a", "b"]);
  h = goBack(goBack(h)!)!;
  h = pushToHistory(h, ["c"]);
  assert.deepEqual(h.entries, [[], ["c"]]);
  h = pushToHistory(h, ["c"]);
  assert.equal(h.entries.length, 2);
  assert.equal(goBack(initialNavHistory()), null);
});
