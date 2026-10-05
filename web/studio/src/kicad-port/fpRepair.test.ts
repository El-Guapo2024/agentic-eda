import { test } from "node:test";
import assert from "node:assert/strict";
import { duplicateIdCount, repairMessage } from "./fpRepair";

const fp = (pads: string[], graphics: string[], texts: string[]) => ({
  pads: pads.map((id) => ({ id }) as never),
  graphics: graphics.map((id) => ({ id }) as never),
  texts: texts.map((id) => ({ id }) as never),
});

test("duplicateIdCount counts every later holder of an id, across pads, graphics and text", () => {
  assert.equal(duplicateIdCount(fp(["a", "b"], ["c"], ["d"])), 0);
  assert.equal(duplicateIdCount(fp(["a", "a"], [], [])), 1);
  assert.equal(duplicateIdCount(fp(["a", "b"], ["a", "b"], ["a"])), 3);
  assert.equal(duplicateIdCount(fp(["a", "a", "a"], [], [])), 2, "three holders = two duplicates");
});

test("items without an id are not counted (the backend gives them one)", () => {
  assert.equal(duplicateIdCount(fp(["", ""], [], [])), 0);
});

test("repairMessage: the two answers RepairFootprint gives", () => {
  assert.deepEqual(repairMessage(0), { title: "No footprint problems found.", details: "" });
  assert.deepEqual(repairMessage(2), { title: "2 potential problems repaired.", details: "2 duplicate IDs replaced." });
});
