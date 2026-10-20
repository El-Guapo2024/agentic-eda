import { test } from "node:test";
import assert from "node:assert/strict";
import { parentsFirst, planImport, planPaste } from "./symImportPlan";
import { ensureUniqueLibId, joinLibName } from "./libraryNames";
import type { LibrarySymbol } from "../api/types";

const s = (name: string, extendsName?: string) => ({ lib_id: name, extends: extendsName ?? null }) as unknown as LibrarySymbol;
const names = (l: LibrarySymbol[]) => l.map((x) => x.lib_id);

test("a derived symbol comes after the symbol it extends, whatever the file's order", () => {
  assert.deepEqual(names(parentsFirst([s("Leaf", "Mid"), s("Other"), s("Mid", "Root"), s("Root")])), ["Root", "Mid", "Leaf", "Other"]);
  assert.deepEqual(names(parentsFirst([s("A"), s("B", "A"), s("C")])), ["A", "B", "C"], "an already ordered list is left alone");
});

test("a cycle or a missing parent does not hang the order", () => {
  assert.deepEqual(names(parentsFirst([s("A", "B"), s("B", "A")])).sort(), ["A", "B"]);
  assert.deepEqual(names(parentsFirst([s("Orphan", "Gone"), s("X")])), ["Orphan", "X"]);
});

test("an import brings along the parent of a derived symbol it was not asked for", () => {
  const file = [s("Root"), s("Mid", "Root"), s("Leaf", "Mid"), s("Alone")];
  assert.deepEqual(names(planImport(file, [file[2]!], new Set())), ["Root", "Mid", "Leaf"]);
  assert.deepEqual(names(planImport(file, [file[2]!], new Set(["Mid"]))), ["Leaf"], "a parent the library has is the link");
  assert.deepEqual(names(planImport(file, [file[3]!, file[1]!], new Set())), ["Root", "Mid", "Alone"].sort((a, b) => ["Root", "Mid", "Alone"].indexOf(a) - ["Root", "Mid", "Alone"].indexOf(b)));
});

test("a paste names every symbol freely and a child follows its parent's new name", () => {
  const taken = ["eda:Root", "eda:Mid"];
  const out = planPaste([s("Mid", "Root"), s("Root")], "eda", taken, ensureUniqueLibId, joinLibName);
  assert.deepEqual(names(out.puts), ["eda:Root_1", "eda:Mid_1"]);
  assert.deepEqual(out.puts.map((x) => x.extends), [null, "Root_1"]);
  assert.deepEqual(out.ids, ["eda:Mid_1", "eda:Root_1"], "ids follow the clipboard's order");
  // the parent is not in the clipboard: the child keeps linking the library's own
  const alone = planPaste([s("Kid", "Root")], "eda", taken, ensureUniqueLibId, joinLibName);
  assert.deepEqual([alone.puts[0]!.lib_id, alone.puts[0]!.extends], ["eda:Kid", "Root"]);
});
