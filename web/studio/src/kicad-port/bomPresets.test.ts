import { test } from "node:test";
import assert from "node:assert/strict";
import type { FieldsTableSpec } from "../api/types";
import { bomPresetJson, matchingPreset, presetChoices, savePresetOutcome, SCOPE_CHOICES, viewCmd } from "./bomPresets";

const spec = (over: Partial<FieldsTableSpec> = {}): FieldsTableSpec => ({
  columns: [
    { name: "Reference", label: "Reference", show: true, group_by: false },
    { name: "Value", label: "Value", show: true, group_by: true },
    { name: "MPN", label: "MPN", show: false, group_by: false },
  ],
  group_symbols: true,
  sort_field: "Reference",
  sort_asc: true,
  filter: "",
  exclude_dnp: false,
  include_excluded_from_bom: true,
  ...over,
});

test("the scope choices are KiCad's, Selection last (25ec2335)", () => {
  assert.deepEqual(
    SCOPE_CHOICES.map((c) => c.label),
    ["Entire Project", "Current Sheet Only", "Current Sheet and Down", "Selection"]
  );
});

test("presetChoices: the built-in and the project's presets in name order, then the separator and the two actions", () => {
  assert.deepEqual(presetChoices(["Default Editing", "Attributes"], ["Mine", "Assembly"]), ["Assembly", "Attributes", "Default Editing", "Mine", "---", "Save preset...", "Delete preset..."]);
});

test("matchingPreset: syncBomPresetSelection compares the settings and the shown or grouped columns only", () => {
  const presets = [{ name: "Mine", spec: spec({ columns: [...spec().columns.slice(0, 2)] }) }];
  assert.equal(matchingPreset(spec(), presets), "Mine", "a hidden column does not count");
  assert.equal(matchingPreset(spec({ sort_asc: false }), presets), null);
  assert.equal(matchingPreset(spec({ filter: "R*" }), presets), null);
  const relabelled = spec();
  relabelled.columns[1] = { ...relabelled.columns[1]!, label: "Val" };
  assert.equal(matchingPreset(relabelled, presets), null, "a column's label counts");
});

test("a preset is bom_settings.cpp's JSON, and Save preset... refuses a built-in name and asks before replacing one of the project's", () => {
  const j = bomPresetJson(spec(), "Mine");
  assert.deepEqual(Object.keys(j), ["name", "fields_ordered", "sort_field", "sort_asc", "filter_string", "group_symbols", "exclude_dnp", "include_excluded_from_bom"]);
  assert.deepEqual((j.fields_ordered as unknown[])[2], { name: "MPN", label: "MPN", show: false, group_by: false });
  assert.equal(savePresetOutcome("Attributes", ["Attributes"], ["Mine"]), "read_only");
  assert.equal(savePresetOutcome("Mine", ["Attributes"], ["Mine"]), "overwrite");
  assert.equal(savePresetOutcome("New", ["Attributes"], ["Mine"]), "new");
  assert.equal((viewCmd(spec()) as unknown as { op: string }).op, "set_bom_view");
});
