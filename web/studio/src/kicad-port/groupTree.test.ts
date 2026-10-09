import { test } from "node:test";
import assert from "node:assert/strict";
import { ancestors, expandGroups, groupAndDescendants, groupHolds, groupLeaves, isGroup, parentGroup, substituteSelection, topLevelGroup, withinScope, type GroupLike } from "./groupTree";

// outer = { inner, c }, inner = { a, b }, other = { d, e }
const groups: GroupLike[] = [
  { id: "outer", member_ids: ["inner", "c"] },
  { id: "inner", member_ids: ["a", "b"] },
  { id: "other", member_ids: ["d", "e"] },
];

test("parents, ancestors and leaves read the tree", () => {
  assert.equal(parentGroup(groups, "a")?.id, "inner");
  assert.equal(parentGroup(groups, "inner")?.id, "outer");
  assert.equal(parentGroup(groups, "outer"), undefined);
  assert.deepEqual(ancestors(groups, "a").map((g) => g.id), ["inner", "outer"]);
  assert.deepEqual(groupLeaves(groups, "outer"), ["a", "b", "c"]);
  assert.deepEqual(groupLeaves(groups, "inner"), ["a", "b"]);
  assert.deepEqual(groupLeaves(groups, "a"), []);
  assert.ok(isGroup(groups, "inner") && !isGroup(groups, "a"));
});

test("expanding replaces each group by its items once, in order", () => {
  assert.deepEqual(expandGroups(groups, ["x", "outer", "a", "other"]), ["x", "a", "b", "c", "d", "e"]);
});

test("a group holds itself and what is below it", () => {
  assert.ok(groupHolds(groups, "outer", "inner") && groupHolds(groups, "outer", "outer") && !groupHolds(groups, "inner", "outer"));
  assert.deepEqual(groupAndDescendants(groups, "outer"), ["outer", "inner"]);
});

test("a loop in a hand-made file ends the walk", () => {
  const loop: GroupLike[] = [
    { id: "x", member_ids: ["a", "y"] },
    { id: "y", member_ids: ["b", "x"] },
  ];
  assert.deepEqual(groupLeaves(loop, "x"), ["a", "b"]);
  assert.ok(ancestors(loop, "a").length <= 2);
});

test("a click on a member selects the outermost group when nothing is entered (TopLevelGroup)", () => {
  assert.equal(topLevelGroup(groups, "a"), "outer");
  assert.equal(topLevelGroup(groups, "inner"), "outer");
  assert.equal(topLevelGroup(groups, "d"), "other");
  assert.equal(topLevelGroup(groups, "zzz"), null);
  assert.equal(topLevelGroup(groups, "outer"), null);
});

test("with a group entered, a click selects the group directly inside it, or the item when it sits in it", () => {
  assert.equal(topLevelGroup(groups, "a", "outer"), "inner", "a is inside inner, which is directly inside outer");
  assert.equal(topLevelGroup(groups, "c", "outer"), null, "c sits directly in outer: it is picked itself");
  assert.equal(topLevelGroup(groups, "a", "inner"), null);
  assert.equal(topLevelGroup(groups, "d", "outer"), "other", "outside the entered group: the outermost group that holds it");
});

test("only what is inside the entered group can be picked (WithinScope)", () => {
  assert.ok(withinScope(groups, "c", "outer") && withinScope(groups, "inner", "outer") && withinScope(groups, "a", "outer"));
  assert.ok(withinScope(groups, "a", "inner"));
  assert.ok(!withinScope(groups, "c", "inner"), "c is in outer, not in inner");
  assert.ok(!withinScope(groups, "d", "outer") && !withinScope(groups, "other", "outer"));
  assert.ok(!withinScope(groups, "outer", "outer"), "the entered group is not one of its own members");
  assert.ok(!withinScope(groups, "free", "outer"), "an item in no group is outside every entered group");
  assert.ok(withinScope(groups, "anything", null), "nothing entered: everything can be picked");
});

test("selecting substitutes the group for its members, and leaves the entered group when something outside it is chosen", () => {
  assert.deepEqual(substituteSelection(groups, ["a"], null), { ids: ["outer"], exits: false });
  assert.deepEqual(substituteSelection(groups, ["a", "b", "c"], null), { ids: ["outer"], exits: false }, "one group for all its members");
  assert.deepEqual(substituteSelection(groups, ["a", "c", "free"], "outer"), { ids: ["outer", "free"], exits: true });
  assert.deepEqual(substituteSelection(groups, ["a", "c"], "outer"), { ids: ["inner", "c"], exits: false });
  assert.deepEqual(substituteSelection(groups, ["free"], null), { ids: ["free"], exits: false });
  assert.deepEqual(substituteSelection([], ["a"], "outer"), { ids: ["a"], exits: false }, "no groups: nothing to do");
});
