import { test } from "node:test";
import assert from "node:assert/strict";
import { chainConflicts, savePlan, savingIntoOwnInheritance } from "./symSaveAsPlan";
import type { LibrarySymbol } from "../api/types";

const sym = (libId: string, extendsId: string | null = null, value?: string) =>
  ({ lib_id: libId, extends: extendsId, published: true, fields: value === undefined ? [] : [{ name: "Value", text: value }] }) as unknown as LibrarySymbol;
const chain = [sym("A:Leaf", "A:Mid", "Leaf"), sym("A:Mid", "A:Root"), sym("A:Root")];
const flat = sym("A:Leaf", null, "Leaf");
const base = { chain, flat, flatten: false, strategy: "overwrite" as const, taken: new Set<string>() };
const brief = (l: LibrarySymbol[]) => l.map((s) => [s.lib_id, s.extends]);

test("saved into its own library under a new name, a derived symbol keeps its parent", () => {
  assert.deepEqual(brief(savePlan({ ...base, newLibId: "A:Copy" })), [["A:Copy", "A:Mid"]]);
});

test("saved into another library it takes its parents along, each linked to its copy", () => {
  assert.deepEqual(brief(savePlan({ ...base, newLibId: "B:Copy" })), [["B:Root", null], ["B:Mid", "B:Root"], ["B:Copy", "B:Mid"]]);
});

test("flattened it is the one flat symbol, in either library", () => {
  assert.deepEqual(brief(savePlan({ ...base, newLibId: "B:Copy", flatten: true })), [["B:Copy", null]]);
  assert.deepEqual(brief(savePlan({ ...base, newLibId: "A:Copy", flatten: true })), [["A:Copy", null]]);
});

test("Rename All gives every copy the first free name_N, the symbol's own included", () => {
  const taken = new Set(["B:Root", "B:Root_1", "B:Mid", "B:Copy"]);
  assert.deepEqual(brief(savePlan({ ...base, newLibId: "B:Copy", strategy: "rename", taken })), [["B:Root_2", null], ["B:Mid_1", "B:Root_2"], ["B:Copy_1", "B:Mid_1"]]);
});

test("a Value that was the symbol's name follows the new name, in every copy", () => {
  const out = savePlan({ ...base, newLibId: "B:Copy" });
  assert.equal(out[2]!.fields![0]!.text, "Copy");
  const other = savePlan({ ...base, chain: [sym("A:Leaf", "A:Mid", "Custom"), ...chain.slice(1)], newLibId: "B:Copy" });
  assert.equal(other[2]!.fields![0]!.text, "Custom", "a Value of its own stays");
});

test("conflicts: only the symbol itself inside one library or when flattened; the whole chain otherwise", () => {
  const taken = new Set(["A:Taken", "B:Root", "B:Taken"]);
  assert.deepEqual(chainConflicts({ chain, newLibId: "A:Taken", flatten: false, taken }), ["Taken"]);
  assert.deepEqual(chainConflicts({ chain, newLibId: "B:Taken", flatten: true, taken }), ["Taken"]);
  assert.deepEqual(chainConflicts({ chain, newLibId: "B:Taken", flatten: false, taken }), ["Taken", "Root"]);
  assert.deepEqual(chainConflicts({ chain, newLibId: "B:Fresh", flatten: false, taken: new Set() }), []);
});

test("saving over a parent, or over a symbol derived from it, is refused", () => {
  const taken = new Set(["A:Leaf", "A:Mid", "A:Root", "A:Kid"]);
  const parentOf = (id: string) => ({ "A:Leaf": "A:Mid", "A:Mid": "A:Root", "A:Kid": "A:Leaf" })[id];
  assert.deepEqual(savingIntoOwnInheritance(chain, "A:Mid", taken, parentOf), { inAncestry: true, inDescendants: false });
  assert.deepEqual(savingIntoOwnInheritance(chain, "A:Kid", taken, parentOf), { inAncestry: false, inDescendants: true });
  assert.deepEqual(savingIntoOwnInheritance(chain, "A:Leaf", taken, parentOf), { inAncestry: false, inDescendants: false }, "over itself is fine");
  assert.deepEqual(savingIntoOwnInheritance(chain, "B:Mid", taken, parentOf), { inAncestry: false, inDescendants: false }, "another library cannot intersect");
});
