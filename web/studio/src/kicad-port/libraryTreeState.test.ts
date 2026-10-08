import { test } from "node:test";
import assert from "node:assert/strict";
import { ALL_COLLAPSED, ALL_EXPANDED, PIN_GLYPH, isFolded, libraryLabel, pinMenu, pinnedFirst, toggleFold, withPinned } from "./libraryTreeState";

test("a tree starts with every library open; a click folds one and another click opens it again", () => {
  assert.equal(isFolded(ALL_EXPANDED, "a"), false);
  const folded = toggleFold(ALL_EXPANDED, "a");
  assert.equal(isFolded(folded, "a"), true);
  assert.equal(isFolded(folded, "b"), false);
  assert.equal(isFolded(toggleFold(folded, "a"), "a"), false);
});

test("Collapse All folds every library, also ones that appear later; a click then opens just that one", () => {
  assert.equal(isFolded(ALL_COLLAPSED, "anything"), true);
  const one = toggleFold(ALL_COLLAPSED, "a");
  assert.equal(isFolded(one, "a"), false);
  assert.equal(isFolded(one, "b"), true);
});

test("Expand All after some folds opens them all", () => {
  const some = toggleFold(toggleFold(ALL_EXPANDED, "a"), "b");
  assert.equal(isFolded(some, "a"), true);
  assert.equal(isFolded(ALL_EXPANDED, "a"), false);
});

test("pinned libraries sort first, each group keeping its order", () => {
  const groups = [{ lib: "a" }, { lib: "b" }, { lib: "c" }, { lib: "d" }];
  assert.deepEqual(pinnedFirst(groups, new Set(["c", "a"])).map((g) => g.lib), ["a", "c", "b", "d"]);
  assert.deepEqual(pinnedFirst(groups, new Set()).map((g) => g.lib), ["a", "b", "c", "d"]);
});

test("a pinned library's row starts with the star glyph", () => {
  assert.equal(libraryLabel("lib", new Set(["lib"])), `${PIN_GLYPH}lib`);
  assert.equal(libraryLabel("lib", new Set()), "lib");
  assert.equal(PIN_GLYPH, "☆ ");
});

test("pinning and unpinning touches only the libraries whose status differs", () => {
  const start: ReadonlySet<string> = new Set(["a"]);
  assert.deepEqual([...withPinned(start, ["a", "b"], true)].sort(), ["a", "b"]);
  assert.deepEqual([...withPinned(start, ["a", "b"], false)], []);
  assert.equal(withPinned(start, ["a"], true), start, "nothing changed: the same set comes back");
  assert.equal(withPinned(start, ["z"], false), start);
});

test("the menu offers Pin when none of the selected libraries is pinned, Unpin when all are, neither on a mixed or empty selection", () => {
  const pinned = new Set(["a"]);
  assert.deepEqual(pinMenu(["b"], pinned), { pin: true, unpin: false });
  assert.deepEqual(pinMenu(["a"], pinned), { pin: false, unpin: true });
  assert.deepEqual(pinMenu(["a", "b"], pinned), { pin: false, unpin: false });
  assert.deepEqual(pinMenu([], pinned), { pin: false, unpin: false });
});
