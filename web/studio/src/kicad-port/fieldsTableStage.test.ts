import { test } from "node:test";
import assert from "node:assert/strict";
import type { FieldsTableSpec } from "../api/types";
import { bomFmtPresets, matchingBomPreset, emptyChanges, isEditableColumn, isEmptyChanges, specAddColumn, specRemoveColumn, specRenameColumn, stageAddField, stageEdit, stageRemoveField, stageRenameField, validNewFieldName } from "./fieldsTableStage";

const spec = (): FieldsTableSpec => ({
  columns: [
    { name: "Reference", label: "Reference", show: true, group_by: false },
    { name: "Value", label: "Value", show: true, group_by: true },
    { name: "MPN", label: "MPN", show: true, group_by: false },
  ],
  group_symbols: true,
  sort_field: "Reference",
  sort_asc: true,
  filter: "",
});

test("stageEdit replaces an earlier edit of the same cell", () => {
  let c = stageEdit(emptyChanges(), "R1", "Value", "1k");
  c = stageEdit(c, "R1", "Value", "2k");
  c = stageEdit(c, "R2", "Value", "2k");
  assert.deepEqual(c.edits, [
    { id: "R1", field: "Value", value: "2k" },
    { id: "R2", field: "Value", value: "2k" },
  ]);
  assert.equal(isEmptyChanges(c), false);
  assert.equal(isEmptyChanges(emptyChanges()), true);
});

test("a field added then removed in one session leaves no trace", () => {
  let c = stageAddField(emptyChanges(), "Tol");
  c = stageEdit(c, "R1", "Tol", "1%");
  c = stageRemoveField(c, "Tol");
  assert.equal(isEmptyChanges(c), true);
});

test("removing an existing field queues its removal and drops its staged edits", () => {
  let c = stageEdit(emptyChanges(), "R1", "MPN", "x");
  c = stageRemoveField(c, "MPN");
  assert.deepEqual(c.remove_fields, ["MPN"]);
  assert.deepEqual(c.edits, []);
});

test("rename of an existing field collapses chains and follows staged edits", () => {
  let c = stageEdit(emptyChanges(), "R1", "MPN", "x");
  c = stageRenameField(c, "MPN", "Part");
  assert.deepEqual(c.rename_fields, [{ from: "MPN", to: "Part" }]);
  assert.deepEqual(c.edits, [{ id: "R1", field: "Part", value: "x" }]);
  c = stageRenameField(c, "Part", "Part Number");
  assert.deepEqual(c.rename_fields, [{ from: "MPN", to: "Part Number" }], "a -> b -> c is one a -> c");
  c = stageRenameField(c, "Part Number", "MPN");
  assert.deepEqual(c.rename_fields, [], "renaming back to the original is no rename at all");
});

test("renaming then removing removes the ORIGINAL name", () => {
  let c = stageRenameField(emptyChanges(), "MPN", "Part");
  c = stageRemoveField(c, "Part");
  assert.deepEqual(c.remove_fields, ["MPN"]);
  assert.deepEqual(c.rename_fields, []);
});

test("rename of a just-added field renames the addition in place", () => {
  let c = stageAddField(emptyChanges(), "Tol");
  c = stageEdit(c, "R1", "Tol", "1%");
  c = stageRenameField(c, "Tol", "Tolerance");
  assert.deepEqual(c.add_fields, ["Tolerance"]);
  assert.deepEqual(c.edits, [{ id: "R1", field: "Tolerance", value: "1%" }]);
  assert.deepEqual(c.rename_fields, []);
});

test("spec column ops mirror the staged ops", () => {
  let s = specAddColumn(spec(), "Tol");
  assert.equal(s.columns.at(-1)?.name, "Tol");
  assert.equal(specAddColumn(s, "Tol").columns.length, s.columns.length, "no duplicate columns");
  s = specRenameColumn({ ...s, sort_field: "Tol" }, "Tol", "Tolerance");
  assert.equal(s.columns.at(-1)?.label, "Tolerance");
  assert.equal(s.sort_field, "Tolerance");
  s = specRemoveColumn(s, "Tolerance");
  assert.equal(s.sort_field, "Reference", "sorting by a removed column falls back to Reference");
});

test("field name validation and editability", () => {
  const s = spec();
  assert.equal(validNewFieldName(s, "  "), "A field needs a name.");
  assert.match(validNewFieldName(s, "Value") ?? "", /built-in/);
  assert.match(validNewFieldName(s, "${QUANTITY}") ?? "", /built-in/);
  assert.match(validNewFieldName(s, "mpn") ?? "", /already exists/);
  assert.equal(validNewFieldName(s, "Tolerance"), null);
  assert.equal(isEditableColumn("Reference"), false);
  assert.equal(isEditableColumn("${QUANTITY}"), false);
  assert.equal(isEditableColumn("Value"), true);
  assert.equal(isEditableColumn("MPN"), true);
});

test("BOM format presets are KiCad CSV / TSV / Semicolons and are recognised again", () => {
  const [csv, tsv, semi] = bomFmtPresets();
  assert.equal(csv?.field_delimiter, ",");
  assert.equal(csv?.string_delimiter, "\"");
  assert.equal(tsv?.field_delimiter, "\t");
  assert.equal(tsv?.string_delimiter, "");
  assert.equal(semi?.string_delimiter, "'");
  assert.equal(matchingBomPreset({ ...tsv!, name: "whatever" })?.name, "TSV");
  assert.equal(matchingBomPreset({ ...csv!, keep_tabs: true }), null, "a tweaked preset is custom");
});
