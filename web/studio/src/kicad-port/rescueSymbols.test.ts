import { test } from "node:test";
import assert from "node:assert/strict";
import { chosenCandidates, instancesTitle, NOTHING_RESCUED, NOTHING_TO_RESCUE, rescueOutcome } from "./rescueSymbols";

test("every candidate starts accepted and the cleared ones are left out, in the list's order", () => {
  assert.deepEqual(chosenCandidates(["Gone:R", "MyParts:R", "X:Y"], new Set()), ["Gone:R", "MyParts:R", "X:Y"]);
  assert.deepEqual(chosenCandidates(["Gone:R", "MyParts:R", "X:Y"], new Set(["MyParts:R"])), ["Gone:R", "X:Y"]);
});

test("RescueProject's two messages: nothing found, nothing kept", () => {
  assert.equal(rescueOutcome(0, 0), NOTHING_TO_RESCUE);
  assert.equal(rescueOutcome(0, 0), "This project has nothing to rescue.");
  assert.equal(rescueOutcome(2, 0), NOTHING_RESCUED);
  assert.equal(rescueOutcome(2, 0), "No symbols were rescued.");
  assert.equal(rescueOutcome(2, 1), null);
});

test("the instance list's title counts the instances", () => {
  assert.equal(instancesTitle(3), "Instances of this symbol (3 items):");
});
