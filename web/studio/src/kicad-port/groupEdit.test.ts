import { test } from "node:test";
import assert from "node:assert/strict";
import { addMember, newMemberFromPick, planAddToGroup, planRemoveFromGroup, removeMemberAt } from "./groupEdit";

const groups = [
  { id: "g1", member_ids: ["a", "b"] },
  { id: "g2", member_ids: ["c", "d"] },
];

test("Add Items needs exactly one group and at least one ungrouped item in the selection", () => {
  assert.deepEqual(planAddToGroup(["g1", "x", "y"], groups), { groupId: "g1", ids: ["x", "y"] });
  assert.equal(planAddToGroup(["g1", "g2", "x"], groups), null, "two groups: refused");
  assert.equal(planAddToGroup(["g1"], groups), null, "nothing to add");
  assert.equal(planAddToGroup(["x", "y"], groups), null, "no group");
  assert.deepEqual(planAddToGroup(["g1", "x", "c"], groups), { groupId: "g1", ids: ["x"] }, "an item already in a group is not offered");
});

test("Remove Items takes the selected members out of their groups", () => {
  assert.deepEqual(planRemoveFromGroup(["a", "x", "d"], groups), ["a", "d"]);
  assert.deepEqual(planRemoveFromGroup(["x"], groups), []);
  assert.deepEqual(planRemoveFromGroup(["g1"], groups), [], "a whole group is not a member");
});

test("the member list of the dialog: no duplicates, never the group itself, rows can be removed", () => {
  assert.deepEqual(addMember(["a", "b"], "z", "g1"), ["a", "b", "z"]);
  assert.deepEqual(addMember(["a", "b"], "a", "g1"), ["a", "b"]);
  assert.deepEqual(addMember(["a", "b"], "g1", "g1"), ["a", "b"]);
  assert.deepEqual(removeMemberAt(["a", "b", "c"], 1), ["a", "c"]);
  assert.deepEqual(removeMemberAt(["a"], 5), ["a"]);
});

test("a picked group is a new member unless it would hold itself", () => {
  assert.equal(newMemberFromPick("g2", groups, "g1"), "g2", "groups nest: another group is a fine member");
  assert.equal(newMemberFromPick("x", groups, "g1"), "x");
  assert.equal(newMemberFromPick("g1", groups, "g1"), null, "the group itself");
  const nested = [...groups, { id: "top", member_ids: ["g1", "z"] }];
  assert.equal(newMemberFromPick("top", nested, "g1"), null, "a group that holds it");
  assert.equal(newMemberFromPick("g1", nested, "top"), "g1", "a group below it is fine");
});
