import { test } from "node:test";
import assert from "node:assert/strict";
import {
  MANDATORY_FIELDS,
  allFields,
  bareName,
  defaultField,
  descendantsOf,
  fieldNameProblem,
  fieldRow,
  libraryOf,
  parentCandidates,
  relatedSymbols,
  rootOf,
  shownFieldText,
  treeConditions,
} from "./symFields";
import type { LibrarySymbol } from "../api/types";

const sym = (over: Partial<LibrarySymbol> = {}) => ({ lib_id: "Lib:R", reference_prefix: "R", datasheet: "", description: "", fields: [], ...over }) as unknown as LibrarySymbol;

// The rules are the ones crates/model/src/sym_fields.rs asserts: the editor shows what the server holds.

test("a symbol with no stored fields has the five mandatory ones at their defaults, the value being its name", () => {
  const f = allFields(sym({ reference_prefix: "U" }));
  assert.deepEqual(f.map((x) => x.name), [...MANDATORY_FIELDS]);
  assert.deepEqual([f[0]!.text, f[1]!.text], ["U", "R"]);
  assert.deepEqual(f.map((x) => x.hidden), [false, false, true, true, true]);
  assert.deepEqual([f[0]!.at.y, f[1]!.at.y], [5.08, 2.54]);
});

test("the members that are the symbol's own win over a stored copy of the text; Value and Footprint are the list's", () => {
  const f = allFields(
    sym({
      reference_prefix: "IC",
      datasheet: "http://changed",
      description: "d",
      fields: [
        { name: "Reference", text: "stale", at: { x: 1, y: 2 } },
        { name: "Value", text: "BASE" },
        { name: "Footprint", text: "Pkg:FP", hidden: true },
        { name: "MPN", text: "X-1", at: { x: 0, y: -5 }, bold: true },
      ],
    })
  );
  assert.deepEqual([f[0]!.text, f[3]!.text, f[4]!.text], ["IC", "http://changed", "d"]);
  assert.deepEqual([f[1]!.text, f[2]!.text], ["BASE", "Pkg:FP"]);
  assert.deepEqual(f[0]!.at, { x: 1, y: 2 }, "the stored place stays");
  assert.equal(f.length, 6);
  assert.deepEqual([f[5]!.name, f[5]!.text, f[5]!.bold, f[5]!.size_mm], ["MPN", "X-1", true, 1.27], "a user field follows the mandatory ones, with its defaults put back");
});

test("a field from the wire gets every default back", () => {
  assert.deepEqual(fieldRow({ name: "X" }), { name: "X", text: "", at: { x: 0, y: 0 }, angle_deg: 0, size_mm: 1.27, hidden: false, show_name: false, do_not_autoplace: false, bold: false, italic: false, h_align: "center", v_align: "center" });
  assert.equal(defaultField("Datasheet").hidden, true);
});

test("names split into library and item", () => {
  assert.deepEqual([libraryOf("Lib:R"), bareName("Lib:R"), libraryOf("R"), bareName("R"), bareName("A:B:C")], ["Lib", "R", "", "R", "B:C"]);
});

const derived = { "L:Mid": "L:Base", "L:Leaf": "L:Mid", "L:Other": "L:Base", "L:Alone": "L:Else" };

test("the derived symbols of a symbol are everything below it, parents first; the root is the end of the chain", () => {
  assert.deepEqual(descendantsOf(derived, "L:Base"), ["L:Mid", "L:Leaf", "L:Other"]);
  assert.deepEqual(descendantsOf(derived, "L:Leaf"), []);
  assert.equal(rootOf(derived, "L:Leaf"), "L:Base");
  assert.equal(rootOf(derived, "L:Base"), "L:Base");
  assert.deepEqual(relatedSymbols(derived, "L:Leaf"), ["L:Base", "L:Mid", "L:Leaf", "L:Other"], "the root, then everything derived from it");
  // a cycle in the data stops
  assert.equal(rootOf({ A: "B", B: "A" }, "A"), "B");
  assert.deepEqual(descendantsOf({ A: "B", B: "A" }, "A"), ["B"]);
});

test("the parents offered are the symbols of the library but not the symbol and not the ones derived from it", () => {
  const names = ["L:Base", "L:Mid", "L:Leaf", "L:Other", "L:Alone", "M:Foreign", "L:A10", "L:A2"];
  assert.deepEqual(parentCandidates("L:Mid", names, derived), ["L:A2", "L:A10", "L:Alone", "L:Base", "L:Other"], "not itself, not its leaf, none of another library; numbers sort as numbers");
});

test("the tree offers Flatten for a derived symbol and the related table for a derived one or a base", () => {
  assert.deepEqual(treeConditions(derived, "L:Leaf"), { isDerived: true, hasChildren: false, related: true });
  assert.deepEqual(treeConditions(derived, "L:Base"), { isDerived: false, hasChildren: true, related: true });
  assert.deepEqual(treeConditions(derived, "L:Lonely"), { isDerived: false, hasChildren: false, related: false });
  assert.equal(treeConditions(derived, undefined).related, false);
});

test("a shown name is drawn before the text; a field name is checked like the dialog does", () => {
  assert.equal(shownFieldText({ name: "MPN", text: "X", show_name: true }), "MPN: X");
  assert.equal(shownFieldText({ name: "MPN", text: "X", show_name: false }), "X");
  assert.equal(fieldNameProblem("", []), "Fields must have a name.");
  assert.match(fieldNameProblem("Value", [])!, /already in use/);
  assert.match(fieldNameProblem("MPN", ["MPN"])!, /already in use/);
  assert.match(fieldNameProblem("ki_keywords", [])!, /keywords or footprint filters/);
  assert.equal(fieldNameProblem("Colour", ["MPN"]), null);
});
