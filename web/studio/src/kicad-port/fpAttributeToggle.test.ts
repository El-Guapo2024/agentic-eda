import { test } from "node:test";
import assert from "node:assert/strict";
import type { Part } from "../api/types";
import { fpAttributeChecked, fpAttributeIn, toggleFpAttributeCmds } from "./fpAttributeToggle";

const attrs = (over: Partial<NonNullable<Part["attrs"]>> = {}): NonNullable<Part["attrs"]> => ({ kind: "smd", board_only: false, exclude_from_pos_files: false, exclude_from_bom: false, dnp: false, allow_missing_courtyard: false, custom: false, exclude_from_sim: false, ...over });
const part = (ref: string, a: Partial<NonNullable<Part["attrs"]>> = {}, variant?: Partial<NonNullable<Part["variant"]>>): Part =>
  ({ ref, placed: true, attrs: attrs(a), ...(variant ? { variant: { dnp: false, exclude_from_bom: false, exclude_from_pos_files: false, exclude_from_sim: false, fields: {}, ...variant } } : {}) }) as unknown as Part;

test("ToggleFootprintAttribute in the default variant: on when any footprint lacks it, the footprint's own BOM / position-files attribute, the symbol's simulation flag", () => {
  const r1 = part("R1", { exclude_from_bom: true });
  const r2 = part("R2");
  const cmds = toggleFpAttributeCmds([r1, r2], "bom", "");
  // R2 lacks it, so every footprint gets it; R1 has it already and is left as it is
  assert.equal(cmds.length, 1);
  assert.deepEqual(cmds[0], { op: "edit_board_footprint", part: "R2", attrs: { kind: "smd", board_only: false, exclude_from_pos_files: false, exclude_from_bom: true, dnp: false, allow_missing_courtyard: false } });
  // both have it: off for both
  const off = toggleFpAttributeCmds([part("R1", { exclude_from_pos_files: true }), part("R2", { exclude_from_pos_files: true })], "pos", "");
  assert.deepEqual(off.map((c) => (c.op === "edit_board_footprint" ? [c.part, c.attrs?.exclude_from_pos_files] : null)), [["R1", false], ["R2", false]]);
  assert.deepEqual(toggleFpAttributeCmds([r1, r2], "sim", ""), [{ op: "set_symbol_attrs", ids: ["R1", "R2"], exclude_from_sim: true }]);
  // a footprint with no symbol has no simulation flag in the studio
  assert.deepEqual(toggleFpAttributeCmds([part("MH1", { board_only: true })], "sim", ""), []);
  assert.deepEqual(toggleFpAttributeCmds([], "bom", ""), []);
});

test("in a variant: the variant's attribute (GetExcludedFromBOMForVariant), set on the symbol's record of the variant", () => {
  const r1 = part("R1", { exclude_from_bom: true }, { exclude_from_bom: false });
  const r2 = part("R2", {}, { exclude_from_bom: true });
  assert.equal(fpAttributeIn(r1, "bom", "Lite"), false, "the variant's, not the footprint's own");
  assert.equal(fpAttributeIn(r1, "bom", ""), true);
  assert.deepEqual(toggleFpAttributeCmds([r1, r2], "bom", "Lite"), [{ op: "set_symbol_variant", variant: "Lite", ids: ["R1", "R2"], exclude_from_bom: true }]);
  assert.deepEqual(toggleFpAttributeCmds([part("R3", {}, { exclude_from_sim: true })], "sim", "Lite"), [{ op: "set_symbol_variant", variant: "Lite", ids: ["R3"], exclude_from_sim: false }]);
  // a footprint with no symbol keeps its own attribute
  const mh = part("MH1", { board_only: true });
  assert.deepEqual(toggleFpAttributeCmds([mh], "pos", "Lite").map((c) => c.op), ["edit_board_footprint"]);
});

test("the check mark (excludeFromBOMCond): every selected footprint has the attribute, none selected is unchecked", () => {
  assert.equal(fpAttributeChecked([part("R1", { exclude_from_bom: true }), part("R2", { exclude_from_bom: true })], "bom", ""), true);
  assert.equal(fpAttributeChecked([part("R1", { exclude_from_bom: true }), part("R2")], "bom", ""), false);
  assert.equal(fpAttributeChecked([], "bom", ""), false);
  assert.equal(fpAttributeChecked([part("R1", {}, { exclude_from_pos_files: true })], "pos", "Lite"), true);
});
