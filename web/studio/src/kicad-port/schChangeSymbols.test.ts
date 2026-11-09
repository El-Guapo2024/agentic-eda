import { test } from "node:test";
import assert from "node:assert/strict";
import { matchOverHierarchy, matchReferences, planChange, referencesOf, refNumber, refPrefix, reprefix, wildMatch, type LibInfo, type SymbolRow } from "./schChangeSymbols";

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

test("Change Symbols over the hierarchy: a match on any sheet path (`GetRef( aInstance )`), each screen's symbols once, the report naming every path's reference (UI-LEFT S9)", () => {
  // a channel screen shown at "a" and "b": R1 there is R1 on a and R5 on b; the root has R2
  const rows = [
    { id: "R2", lib_id: "Device:R", unit: 1, value: "1k", reference: "R2", sheet: "", file: "" },
    { id: "R1", lib_id: "Device:R", unit: 1, value: "10k", reference: "R1", sheet: "a", file: "ch.kicad_sch" },
    { id: "R1", lib_id: "Device:R", unit: 1, value: "10k", reference: "R5", sheet: "b", file: "ch.kicad_sch" },
  ];
  // matched on the second placement's reference: the one symbol of the shared screen, addressed through the path it matched on
  assert.deepEqual(matchOverHierarchy(rows, { by: "reference", reference: "R5", value: "", id: "" }, new Set(), ""), [{ sheet: "b", file: "ch.kicad_sch", refs: ["R1"] }]);
  // everything: the root's and the screen's, the screen once
  assert.deepEqual(matchOverHierarchy(rows, { by: "all", reference: "", value: "", id: "" }, new Set(), ""), [
    { sheet: "", file: "", refs: ["R2"] },
    { sheet: "a", file: "ch.kicad_sch", refs: ["R1"] },
  ]);
  // the selection is the sheet in view's
  assert.deepEqual(matchOverHierarchy(rows, { by: "selection", reference: "", value: "", id: "" }, new Set(["R1"]), "b"), [{ sheet: "b", file: "ch.kicad_sch", refs: ["R1"] }]);
  assert.equal(referencesOf(rows, "ch.kicad_sch", "R1"), "R1 R5");
  const plan = planChange(["R1"], rows.slice(1, 2), { found: true, unitCount: 1, prefix: "R", value: "R" }, { newLibId: "Device:R_Small", updateReference: false, updateValue: false, resetEmpty: false, shown: (r) => referencesOf(rows, "ch.kicad_sch", r) });
  assert.deepEqual(plan.report, ["R1: Device:R -> Device:R_Small"]);
  assert.deepEqual(plan.cmds, [{ op: "sch_edit", verb: "change_symbol", id: "R1", lib_id: "Device:R_Small" }]);
  const missing = planChange(["R1"], rows.slice(1, 2), { found: false, unitCount: 1, prefix: "", value: "" }, { newLibId: "X:Y", updateReference: false, updateValue: false, resetEmpty: false, shown: (r) => referencesOf(rows, "ch.kicad_sch", r) });
  assert.deepEqual(missing.report, ["R1 R5: X:Y *** symbol not found ***"]);
});

// UI-LEFT-2 SS14: the change and the resets are one step (`processSymbols`' one commit)
import { changeStep, resetCmd } from "./schChangeSymbols";

test("the resets of the symbols a change processed go in the change's own batch, allowed to change nothing there", () => {
  const symbols: SymbolRow[] = [{ id: "R1", lib_id: "Device:R", unit: 1, value: "10k" }];
  const plan = planChange(["R1"], symbols, { found: true, unitCount: 1, prefix: "C", value: "C" }, { newLibId: "Device:C", updateReference: true, updateValue: false, resetEmpty: false });
  assert.deepEqual(plan.processed, ["C1"], "named as the change leaves it");
  const step = changeStep([{ sheet: "sa", cmds: plan.cmds, processed: plan.processed }], { visibility: false, effects: false, positions: true }) as unknown as { op: string; sheet: string; cmd: { op: string; cmds: Array<{ op: string; sheet: string; cmd: Record<string, unknown> }> } };
  assert.equal(step.op, "on_sheet");
  assert.equal(step.sheet, "");
  const inner = step.cmd.cmds;
  assert.deepEqual(inner.map((c) => c.cmd.verb ?? c.cmd.op), ["change_symbol", "rename_symbol", "reset_symbol_fields"]);
  assert.deepEqual([inner[2]!.sheet, inner[2]!.cmd.ids, inner[2]!.cmd.allow_unchanged], ["sa", ["C1"], true]);
});

test("no reset option: no reset; nothing to send: null; the resets alone are refused when they change nothing", () => {
  assert.equal(resetCmd(["R1"], { visibility: false, effects: false, positions: false }, true), null);
  assert.equal(changeStep([{ sheet: "", cmds: [], processed: ["R1"] }], { visibility: false, effects: false, positions: false }), null);
  const alone = changeStep([{ sheet: "", cmds: [], processed: ["R1"] }], { visibility: true, effects: false, positions: false }) as unknown as { cmd: { cmds: Array<{ cmd: Record<string, unknown> }> } };
  assert.equal(alone.cmd.cmds[0]!.cmd.allow_unchanged, undefined);
  const update = changeStep([{ sheet: "", cmds: [], processed: ["R1"] }], { visibility: true, effects: false, positions: false }, [{ op: "sch_edit", verb: "update_library_symbols", lib_ids: ["Device:R"] } as never]) as unknown as { cmd: { cmds: Array<Record<string, unknown>> } };
  assert.equal(update.cmd.cmds.length, 2);
  assert.equal((update.cmd.cmds[1]!.cmd as Record<string, unknown>).allow_unchanged, true);
});
