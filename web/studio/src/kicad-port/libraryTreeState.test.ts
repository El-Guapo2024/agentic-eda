import { test } from "node:test";
import assert from "node:assert/strict";
import { ALL_COLLAPSED, ALL_EXPANDED, FOLD_AUTO, PIN_GLYPH, isGroupOpen, libraryLabel, pinMenu, pinnedFirst, setGroupOpen, withPinned } from "./libraryTreeState";

const facts = (hasItems: boolean, defaultOpen: boolean) => ({ hasItems, defaultOpen });

test("with nothing chosen a group is open the way the tree decides", () => {
  assert.equal(isGroupOpen(FOLD_AUTO, "project", facts(true, true)), true);
  assert.equal(isGroupOpen(FOLD_AUTO, "installed", facts(false, false)), false);
});

test("a click on a group is a choice that wins over everything", () => {
  const opened = setGroupOpen(FOLD_AUTO, "installed", true);
  assert.equal(isGroupOpen(opened, "installed", facts(false, false)), true);
  assert.equal(isGroupOpen(opened, "other", facts(false, false)), false);
  const closed = setGroupOpen(ALL_EXPANDED, "a", false);
  assert.equal(isGroupOpen(closed, "a", facts(true, true)), false);
  assert.equal(isGroupOpen(closed, "b", facts(true, true)), true);
});

test("Expand All opens every library that has items listed and leaves an unloaded installed one folded", () => {
  assert.equal(isGroupOpen(ALL_EXPANDED, "a", facts(true, false)), true);
  assert.equal(isGroupOpen(ALL_EXPANDED, "unloaded", facts(false, false)), false);
});

test("Collapse All folds every library, also ones that appear later; a click then opens just that one", () => {
  assert.equal(isGroupOpen(ALL_COLLAPSED, "anything", facts(true, true)), false);
  const one = setGroupOpen(ALL_COLLAPSED, "a", true);
  assert.equal(isGroupOpen(one, "a", facts(true, true)), true);
  assert.equal(isGroupOpen(one, "b", facts(true, true)), false);
});

test("setting a group does not change the fold it came from", () => {
  const next = setGroupOpen(FOLD_AUTO, "a", true);
  assert.equal(FOLD_AUTO.explicit.size, 0);
  assert.equal(next.explicit.size, 1);
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
