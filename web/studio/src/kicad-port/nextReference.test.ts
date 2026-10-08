import { test } from "node:test";
import assert from "node:assert/strict";
import { nextReference } from "./nextReference";
import type { SchematicSymbol } from "../api/types";

function sym(id: string): SchematicSymbol {
  return { id, lib_id: null, at: [0, 0], rot: 0, mirror: null, unit: 1, body_style: 1, value: null, mpn: null, package: null, footprint: null, datasheet: null, pins: [] };
}

test("nextReference: an empty sheet starts at 1", () => {
  assert.equal(nextReference([], "R"), "R1");
});

test("nextReference: picks one past the highest existing number for that prefix", () => {
  assert.equal(nextReference([sym("R1"), sym("R2"), sym("R4")], "R"), "R5", "gaps are not backfilled -- same as a real Annotate pass never renumbers down");
});

test("nextReference: different prefixes are counted independently", () => {
  const sheet = [sym("R1"), sym("R2"), sym("C1")];
  assert.equal(nextReference(sheet, "R"), "R3");
  assert.equal(nextReference(sheet, "C"), "C2");
  assert.equal(nextReference(sheet, "U"), "U1", "a prefix with nothing placed yet starts at 1");
});

test("nextReference: an id that doesn't match <letters><digits> is ignored, not miscounted", () => {
  assert.equal(nextReference([sym("R?"), sym("R1")], "R"), "R2", "the unannotated placeholder has no number to contribute");
});

test("nextReference: the references of the design's other sheets are taken too", () => {
  const thisSheet = [sym("U1"), sym("C1")];
  assert.equal(nextReference(thisSheet, "R", ["R1", "R2", "R8", "D1"]), "R9", "a resistor placed on the MCU sheet does not reuse one the LED channels sheet has");
  assert.equal(nextReference(thisSheet, "C", ["C12"]), "C13");
  assert.equal(nextReference(thisSheet, "U", []), "U2");
});
