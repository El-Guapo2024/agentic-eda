import { test } from "node:test";
import assert from "node:assert/strict";
import { renamedFieldLooksCmds } from "./symLooks";
import { gridRowsFor, renamedFields, savedFields } from "./symFieldsGrid";
import type { LibrarySymbol } from "../api/types";

test("a renamed field takes its looks along: the grid knows each row's old name, and the new name is given the old one's looks (UI-LEFT-3 Y3-3)", () => {
  const color = { r: 10, g: 20, b: 30, a: 255 };
  const sym = {
    lib_id: "L:S",
    reference_prefix: "U",
    description: "",
    datasheet: "",
    fields: [{ name: "MPN", text: "X", at: { x: 0, y: 0 } }, { name: "Plain", text: "Y", at: { x: 0, y: 0 } }],
    kicad: { field_looks: { MPN: { font: "DejaVu Sans", color }, Plain: {} } },
  } as unknown as LibrarySymbol;
  const rows = gridRowsFor(sym, null);
  assert.ok(rows.every((r) => r.was === r.name), "each row starts with the name it has");
  const edited = rows.map((r) => (r.name === "MPN" ? { ...r, name: "PartNumber" } : r.name === "Plain" ? { ...r, name: "Other" } : r));
  const renames = renamedFields(edited);
  assert.deepEqual(renames, [{ from: "MPN", to: "PartNumber" }, { from: "Plain", to: "Other" }]);
  assert.ok(savedFields(edited).every((f) => !("was" in f)), "the old name is not written");
  assert.deepEqual(renamedFieldLooksCmds("L:S", sym, renames), [{ op: "set_symbol_field_looks", lib_id: "L:S", name: "PartNumber", looks: { font: "DejaVu Sans", color } }], "a field of default looks gives nothing");
  assert.deepEqual(renamedFields([...edited, { ...rows[0]!, key: 99, name: "New", was: undefined }]).length, 2, "a row added in the dialog is no rename");
});
