import { test } from "node:test";
import assert from "node:assert/strict";
import { accelIndex, clarificationRows, MENU_LIMIT } from "./clarificationMenu";

test("doSelectionMenu: the first nine rows numbered ('&N  desc\\tN', the number left out of the text on macOS), the rest not, then Select All on A", () => {
  const descs = Array.from({ length: 11 }, (_, i) => `Item ${i + 1}`);
  const rows = clarificationRows(descs, { mac: false, more: false });
  assert.deepEqual(rows[0], { kind: "item", index: 0, label: "1  Item 1", accel: "1" });
  assert.deepEqual(rows[8], { kind: "item", index: 8, label: "9  Item 9", accel: "9" });
  assert.deepEqual(rows[9], { kind: "item", index: 9, label: "Item 10" });
  assert.deepEqual(rows[11], { kind: "all", label: "Select All", accel: "A" });
  assert.equal(rows.length, 12, "no Show More Choices without items left out");
  assert.deepEqual(clarificationRows(["Pad 1"], { mac: true, more: false })[0], { kind: "item", index: 0, label: "Pad 1", accel: "1" });
});

test("doSelectionMenu: Show More Choices... (M) when the guess left items out; at most a hundred rows", () => {
  const rows = clarificationRows(["a", "b"], { mac: false, more: true });
  assert.deepEqual(rows[rows.length - 1], { kind: "more", label: "Show More Choices...", accel: "M" });
  assert.equal(clarificationRows(Array.from({ length: 150 }, () => "x"), { mac: false, more: false }).filter((r) => r.kind === "item").length, MENU_LIMIT);
});

test("accelIndex: a row's number chooses it, A and M in either case; a separator or a disabled entry never", () => {
  const entries = [{ accel: "1" }, { accel: "2", disabled: true }, { separator: true }, { accel: "A" }, {}];
  assert.equal(accelIndex(entries, "1"), 0);
  assert.equal(accelIndex(entries, "2"), -1);
  assert.equal(accelIndex(entries, "a"), 3);
  assert.equal(accelIndex(entries, "A"), 3);
  assert.equal(accelIndex(entries, "Escape"), -1);
  assert.equal(accelIndex(entries, "3"), -1);
});
