import { test } from "node:test";
import assert from "node:assert/strict";
import { gridProblem, gridRowsFor, moveRow, newUserField, referencePrefix, sameField, savedFields } from "./symFieldsGrid";
import { MANDATORY_FIELDS } from "./symFields";
import type { LibrarySymbol } from "../api/types";

const sym = (over: Partial<LibrarySymbol> = {}) => ({ lib_id: "Lib:R", reference_prefix: "R", datasheet: "", description: "", fields: [], ...over }) as unknown as LibrarySymbol;

test("the reference prefix is the reference without its trailing digits and question marks", () => {
  assert.deepEqual(["R", "R?", "R12", "IC1?", "#PWR", "12", "?"].map(referencePrefix), ["R", "R", "R", "IC", "#PWR", "", ""]);
});

test("a root symbol's rows are its own fields, none inherited", () => {
  const rows = gridRowsFor(sym({ fields: [{ name: "MPN", text: "X" }] }), null);
  assert.deepEqual(rows.map((r) => r.name), [...MANDATORY_FIELDS, "MPN"]);
  assert.ok(rows.every((r) => !r.inherited && r.parent === null));
  assert.equal(new Set(rows.map((r) => r.key)).size, rows.length, "every row has its own key");
});

test("a derived symbol lists its parent's user fields as inherited, and marks its own copy of one", () => {
  const own = sym({ lib_id: "Lib:R_1", extends: "Lib:R", fields: [{ name: "Tol", text: "5%" }] });
  const parent = sym({ fields: [{ name: "MPN", text: "X" }, { name: "Tol", text: "1%" }] });
  const rows = gridRowsFor(own, parent);
  assert.deepEqual(rows.map((r) => [r.name, r.inherited]), [...MANDATORY_FIELDS.map((m) => [m, false]), ["Tol", true], ["MPN", true]]);
  assert.equal(rows.find((r) => r.name === "Tol")!.text, "5%", "the symbol's own text stays");
  assert.equal(rows.find((r) => r.name === "Tol")!.parent!.text, "1%");
  assert.equal(rows.find((r) => r.name === "MPN")!.text, "X");
});

test("OK leaves out an inherited row that still equals its parent's, and the empty unnamed rows", () => {
  const own = sym({ lib_id: "Lib:R_1", extends: "Lib:R", fields: [{ name: "Tol", text: "5%" }] });
  const parent = sym({ fields: [{ name: "MPN", text: "X" }, { name: "Tol", text: "1%" }] });
  const rows = [...gridRowsFor(own, parent), newUserField(8, 99)];
  rows[rows.length - 1]!.name = "";
  const saved = savedFields(rows).map((f) => f.name);
  assert.deepEqual(saved, [...MANDATORY_FIELDS, "Tol"], "MPN is inherited as it is; Tol is overridden; the empty row goes");
  // editing the inherited text makes it the symbol's own
  const edited = rows.map((r) => (r.name === "MPN" ? { ...r, text: "Y" } : r));
  assert.deepEqual(savedFields(edited).map((f) => f.name), [...MANDATORY_FIELDS, "Tol", "MPN"]);
});

test("a mandatory field is always kept, even when it is empty", () => {
  const rows = gridRowsFor(sym({ reference_prefix: "" }), null);
  assert.equal(savedFields(rows).length, 5);
});

test("Validate: the reference of a root, a name for a field with text, and a name used once", () => {
  const root = gridRowsFor(sym(), null);
  assert.equal(gridProblem(root, true), null);
  assert.equal(gridProblem(root.map((r) => (r.name === "Reference" ? { ...r, text: "12" } : r)), true), "References must start with a letter.");
  assert.equal(gridProblem(root.map((r) => (r.name === "Reference" ? { ...r, text: "" } : r)), false), null, "a derived symbol's reference may be empty: it inherits");
  const named = (name: string, text = "t") => ({ ...newUserField(6, 50 + name.length), name, text });
  assert.equal(gridProblem([...root, named("", "text")], true), "Fields must have a name.");
  assert.equal(gridProblem([...root, named("", "")], true), null, "an empty unnamed row is just dropped");
  assert.equal(gridProblem([...root, named("A"), { ...named("a"), key: 77 }], true), "The name 'a' is already in use.");
  assert.equal(gridProblem([...root, named("value")], true), "The name 'value' is already in use.", "the mandatory names count");
});

test("a new field is Field<row count>, hidden", () => {
  const f = newUserField(7, 3);
  assert.deepEqual([f.name, f.hidden, f.text, f.size_mm, f.inherited], ["Field7", true, "", 1.27, false]);
});

test("rows move one place, never into the mandatory rows", () => {
  const rows = [...gridRowsFor(sym(), null), newUserField(5, 100), newUserField(6, 101)].map((r, i) => (i >= 5 ? { ...r, name: `U${i}` } : r));
  assert.deepEqual(moveRow(rows, 5, -1), rows, "the first user row cannot go up");
  assert.deepEqual(moveRow(rows, 3, 1), rows, "a mandatory row cannot move");
  assert.deepEqual(moveRow(rows, 6, 1), rows, "the last row cannot go down");
  assert.deepEqual(moveRow(rows, 6, -1).map((r) => r.name).slice(5), ["U6", "U5"]);
  assert.deepEqual(moveRow(rows, 5, 1).map((r) => r.name).slice(5), ["U6", "U5"]);
});

test("sameField compares what the grid shows", () => {
  const a = gridRowsFor(sym(), null)[0]!;
  assert.ok(sameField(a, { ...a }));
  assert.ok(!sameField(a, { ...a, hidden: !a.hidden }));
  assert.ok(!sameField(a, { ...a, at: { x: a.at.x + 1, y: a.at.y } }));
});
