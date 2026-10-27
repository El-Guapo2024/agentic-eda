import { test } from "node:test";
import assert from "node:assert/strict";
import { initialNavHistory, pushToHistory, goBack, goForward, canGoBack, canGoForward, cleanHistory, resetHistory } from "./navHistory";

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

test("r6 C6-L1: clean_history_drops_paths_no_longer_in_the_hierarchy (CleanHistory)", () => {
  assert.deepEqual(cleanHistory({ entries: [[], ["A"], []], index: 2 }, [[]]), { entries: [[]], index: 0 });
  // kept entries; the index goes to the last one
  assert.deepEqual(cleanHistory({ entries: [[], ["A"], ["B"], []], index: 1 }, [[], ["B"]]), { entries: [[], ["B"], []], index: 2 });
  // an empty hierarchy leaves the history as it is (`wxCHECK( !sheets.empty() )`)
  const h = { entries: [[], ["A"]], index: 1 };
  assert.equal(cleanHistory(h, []), h);
  assert.deepEqual(resetHistory(["A"]), { entries: [["A"]], index: 0 });
});
