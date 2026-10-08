import { test } from "node:test";
import assert from "node:assert/strict";
import { existsMessage, saveAsDefaults, saveAsError, saveAsTarget, savedFootprintId, savedMessage, typedFootprintName } from "./saveFootprintAs";

test("Save As works on the tree's selection, else the loaded footprint, and a library row is Save Library As", () => {
  assert.deepEqual(saveAsTarget(null, "R_0603", []), { kind: "footprint", name: "R_0603", loaded: true });
  assert.deepEqual(saveAsTarget("R_0603", "R_0603", []), { kind: "footprint", name: "R_0603", loaded: true });
  assert.deepEqual(saveAsTarget("C_0805", "R_0603", []), { kind: "footprint", name: "C_0805", loaded: false }, "a selected footprint other than the loaded one");
  assert.deepEqual(saveAsTarget("C_0805", null, []), { kind: "footprint", name: "C_0805", loaded: false });
  assert.deepEqual(saveAsTarget(null, "R_0603", ["Resistor_SMD"]), { kind: "library", lib: "Resistor_SMD" });
  assert.deepEqual(saveAsTarget("C_0805", "R_0603", ["Resistor_SMD"]), { kind: "footprint", name: "C_0805", loaded: false }, "a footprint selected wins over a library row");
  assert.deepEqual(saveAsTarget(null, null, []), { kind: "none" });
});

test("the typed name is trimmed and the project library's footprints stay bare", () => {
  assert.equal(typedFootprintName("  R_0603 \t"), "R_0603");
  assert.equal(savedFootprintId("eda", " R_new "), "R_new");
  assert.equal(savedFootprintId("Resistor_SMD", "R_new"), "Resistor_SMD:R_new");
});

test("the dialog's validator: a library, a name, a legal name", () => {
  assert.equal(saveAsError("", "R"), "A library must be specified.");
  assert.equal(saveAsError("eda", "   "), "Footprint must have a name.");
  assert.match(saveAsError("eda", "R:1")!, /cannot contain/);
  assert.match(saveAsError("Bad\\Lib", "R")!, /library nickname/i);
  assert.equal(saveAsError("eda", "R_0603"), null);
  assert.equal(saveAsError("Resistor_SMD", "R_0603"), null);
});

test("the messages are KiCad's", () => {
  assert.equal(existsMessage("Resistor_SMD", " R_0603 "), "Footprint R_0603 already exists in Resistor_SMD.");
  assert.equal(savedMessage("eda", "R_new", false), "Footprint 'R_new' added to 'eda'");
  assert.equal(savedMessage("eda", "R_new", true), "Footprint 'R_new' replaced in 'eda'");
});

test("the dialog starts on the footprint's own library and name", () => {
  assert.deepEqual(saveAsDefaults("Resistor_SMD:R_0603"), { lib: "Resistor_SMD", item: "R_0603" });
  assert.deepEqual(saveAsDefaults("R_0603"), { lib: "eda", item: "R_0603" });
});
