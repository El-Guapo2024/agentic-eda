import { test } from "node:test";
import assert from "node:assert/strict";
import { ATTRIBUTE_ACTIONS, attributeChecked, hitSheet, INCREMENT_PARAMS, planIncrement, nearestTextItem, nextAttributeState } from "./schControl";

test("an attribute toggle sets all when any lacks it, and clears all when every one has it", () => {
  assert.equal(nextAttributeState([{ dnp: true }, { dnp: false }], "dnp"), true);
  assert.equal(nextAttributeState([{ dnp: true }, { dnp: true }], "dnp"), false);
  assert.equal(nextAttributeState([{}], "dnp"), true);
  assert.equal(attributeChecked([{ dnp: true }, { dnp: true }], "dnp"), true);
  assert.equal(attributeChecked([{ dnp: true }, {}], "dnp"), false);
  assert.equal(attributeChecked([], "dnp"), false);
});

test("the four attribute actions flip their own field", () => {
  assert.deepEqual(Object.values(ATTRIBUTE_ACTIONS).sort(), ["dnp", "exclude_from_board", "exclude_from_bom", "exclude_from_sim"]);
});

test("a click inside a sheet's rectangle hits it, the last drawn wins where two overlap", () => {
  const sheets = [
    { id: "a", at: [0, 0] as const, size: [100, 100] as const },
    { id: "b", at: [50, 50] as const, size: [100, 100] as const },
  ];
  assert.equal(hitSheet(sheets, 10, 10), "a");
  assert.equal(hitSheet(sheets, 75, 75), "b");
  assert.equal(hitSheet(sheets, 200, 200), null);
});

test("the nearest label or text within the tolerance is the hovered one", () => {
  const items = [
    { id: "l1", at: [0, 0] as const },
    { id: "l2", at: [500, 0] as const },
  ];
  assert.equal(nearestTextItem(items, 400, 0, 300), "l2");
  assert.equal(nearestTextItem(items, 250, 0, 100), null);
});

test("a selection of one kind changes every text that can move, a mixed selection changes nothing", () => {
  const one = [
    { id: "a", kind: "label:local" as const, text: "D0" },
    { id: "b", kind: "label:local" as const, text: "D9" },
    { id: "c", kind: "label:local" as const, text: "RESET" },
  ];
  assert.deepEqual(planIncrement(one, 1, 0), [
    { id: "a", text: "D1" },
    { id: "b", text: "D10" },
  ]);
  assert.deepEqual(planIncrement(one, -1, 0), [{ id: "b", text: "D8" }], "D0 does not go below zero");
  assert.equal(planIncrement([...one, { id: "t", kind: "text" as const, text: "R1" }], 1, 0), null);
  assert.deepEqual(planIncrement([], 1, 0), []);
});

test("the five actions pass KiCad's delta and index", () => {
  assert.deepEqual(INCREMENT_PARAMS["eeschema.Interactive.incrementSecondary"], { delta: 1, index: 1 });
  assert.deepEqual(INCREMENT_PARAMS["eeschema.Interactive.decrementPrimary"], { delta: -1, index: 0 });
  assert.equal(Object.keys(INCREMENT_PARAMS).length, 5);
});
