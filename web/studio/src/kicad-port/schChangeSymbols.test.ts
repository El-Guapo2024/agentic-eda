import { test } from "node:test";
import assert from "node:assert/strict";
import { matchReferences, planChange, refNumber, refPrefix, reprefix, wildMatch, type LibInfo, type SymbolRow } from "./schChangeSymbols";

const sym = (id: string, lib_id: string, unit = 1, value: string | null = null): SymbolRow => ({ id, lib_id, unit, value });
const symbols = [sym("R1", "Device:R", 1, "10k"), sym("R2", "Device:R", 1, "4k7"), sym("U1", "Device:Q_Dual", 1), sym("U1", "Device:Q_Dual", 2), sym("C1", "Device:C", 1, "100n")];

test("wildcards: * is any run, ? any one character, case ignored unless asked", () => {
  assert.equal(wildMatch("R*", "r12"), true);
  assert.equal(wildMatch("R?", "R12"), false);
  assert.equal(wildMatch("R??", "R12"), true);
  assert.equal(wildMatch("*k7", "4K7"), true);
  assert.equal(wildMatch("r*", "R1", true), false);
  assert.equal(wildMatch("", ""), true);
  assert.equal(wildMatch("a*b*c", "aXXbYYc"), true);
  assert.equal(wildMatch("a*b*c", "aXXbYY"), false);
});

test("a request matches by selection, everything, reference pattern, value pattern or library id; each reference once", () => {
  const sel = new Set(["R2", "U1"]);
  assert.deepEqual(matchReferences(symbols, { by: "selection", reference: "", value: "", id: "" }, sel), ["R2", "U1"]);
  assert.deepEqual(matchReferences(symbols, { by: "all", reference: "", value: "", id: "" }, sel), ["R1", "R2", "U1", "C1"]);
  assert.deepEqual(matchReferences(symbols, { by: "reference", reference: "R*", value: "", id: "" }, sel), ["R1", "R2"]);
  assert.deepEqual(matchReferences(symbols, { by: "value", reference: "", value: "*n", id: "" }, sel), ["C1"]);
  assert.deepEqual(matchReferences(symbols, { by: "id", reference: "", value: "", id: "Device:Q_Dual" }, sel), ["U1"]);
});

test("reference prefix and number follow GetRefDesPrefix / GetRefDesNumber", () => {
  assert.equal(refPrefix("R12"), "R");
  assert.equal(refPrefix("#PWR03"), "#PWR");
  assert.equal(refPrefix("U?"), "U");
  assert.equal(refNumber("R12"), 12);
  assert.equal(refNumber("R?"), -1);
  assert.equal(reprefix("R12", "C"), "C12");
  assert.equal(reprefix("R?", "C"), "C?");
});

const lib = (over: Partial<LibInfo> = {}): LibInfo => ({ found: true, unitCount: 1, prefix: "C", value: "C", ...over });

test("a change swaps the library symbol and leaves the fields unless asked", () => {
  const plan = planChange(["R1"], symbols, lib(), { newLibId: "Device:C", updateReference: false, updateValue: false, resetEmpty: false });
  assert.deepEqual(plan.cmds, [{ op: "sch_edit", verb: "change_symbol", id: "R1", lib_id: "Device:C" }]);
  assert.equal(plan.changed, 1);
});

test("updating the reference and value resets them from the library symbol, the rename last", () => {
  const plan = planChange(["R1"], symbols, lib(), { newLibId: "Device:C", updateReference: true, updateValue: true, resetEmpty: false });
  assert.deepEqual(plan.cmds, [
    { op: "sch_edit", verb: "change_symbol", id: "R1", lib_id: "Device:C" },
    { op: "edit_symbol_fields", id: "R1", value: "C" },
    { op: "rename_symbol", id: "R1", new_id: "C1" },
  ]);
});

test("a symbol the library does not have, or with too few units, is reported and skipped", () => {
  const missing = planChange(["R1"], symbols, lib({ found: false }), { newLibId: "Nowhere:X", updateReference: false, updateValue: false, resetEmpty: false });
  assert.deepEqual(missing.cmds, []);
  assert.match(missing.report[0]!, /symbol not found/);
  const few = planChange(["U1", "R1"], symbols, lib({ unitCount: 1 }), { newLibId: "Device:Q", updateReference: false, updateValue: false, resetEmpty: false });
  assert.match(few.report[0]!, /too few units/);
  assert.deepEqual(few.cmds, [{ op: "sch_edit", verb: "change_symbol", id: "R1", lib_id: "Device:Q" }], "the other symbol is still changed");
});

test("an empty library field only resets with 'reset fields if empty'", () => {
  const noPrefix = lib({ prefix: "", value: "" });
  const off = planChange(["R1"], symbols, noPrefix, { newLibId: "Device:C", updateReference: true, updateValue: true, resetEmpty: false });
  assert.deepEqual(off.cmds, [{ op: "sch_edit", verb: "change_symbol", id: "R1", lib_id: "Device:C" }]);
  const on = planChange(["R1"], symbols, noPrefix, { newLibId: "Device:C", updateReference: true, updateValue: true, resetEmpty: true });
  assert.ok(on.cmds.some((c) => c.op === "edit_symbol_fields"));
});

test("a symbol already on the new library symbol with nothing to update is left alone", () => {
  const plan = planChange(["C1"], symbols, lib(), { newLibId: "Device:C", updateReference: false, updateValue: false, resetEmpty: false });
  assert.deepEqual(plan.cmds, []);
  assert.equal(plan.changed, 0);
});
