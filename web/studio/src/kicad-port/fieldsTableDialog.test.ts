import { test } from "node:test";
import assert from "node:assert/strict";
import type { BomFmt, FieldsTableSpec } from "../api/types";
import { cellsAsText, closeCmd, columnMenu, fmtPresetChoices, fmtWithSaved, gridCellMenu, groupByRefusal, heldWithSaved, heldWithout, mandatoryRefusal, matchingFmtPreset, saveFmtOutcome, specSetLabel, textAsCells, withPresetSpec } from "./fieldsTableDialog";
import { bomFmtPresets } from "./fieldsTableStage";

const spec = (over: Partial<FieldsTableSpec> = {}): FieldsTableSpec => ({
  columns: [
    { name: "Reference", label: "Reference", show: true, group_by: false },
    { name: "Value", label: "Value", show: true, group_by: true },
    { name: "${QUANTITY}", label: "Qty", show: true, group_by: false },
    { name: "MPN", label: "MPN", show: true, group_by: false },
  ],
  group_symbols: true,
  sort_field: "Reference",
  sort_asc: true,
  filter: "",
  exclude_dnp: false,
  include_excluded_from_bom: true,
  ...over,
});

test("Group By refuses the two generated counts with KiCad's words", () => {
  assert.equal(groupByRefusal("${QUANTITY}", true), "The Quantity column cannot be grouped by.");
  assert.equal(groupByRefusal("${ITEM_NUMBER}", true), "The Item Number column cannot be grouped by.");
  assert.equal(groupByRefusal("${QUANTITY}", false), null, "unticking is no refusal");
  assert.equal(groupByRefusal("Value", true), null);
});

test("the mandatory rows: five of them, refused for Remove and Rename", () => {
  assert.equal(mandatoryRefusal("Description", false), "The first 5 fields are mandatory.");
  assert.equal(mandatoryRefusal("Value", true), "The first 5 fields are mandatory and names cannot be changed.");
  assert.equal(mandatoryRefusal("MPN", false), null);
});

test("a BOM Name names the column", () => {
  assert.equal(specSetLabel(spec(), "MPN", "Part number").columns[3]!.label, "Part number");
});

test("the cell menu: Select Footprint... on the Footprint column, Show Datasheet on the Datasheet column, then GRID_TRICKS' five", () => {
  const labels = (items: ReturnType<typeof gridCellMenu>) => items.map((i) => (i.kind === "separator" ? "---" : i.label.split("\t")[0]));
  assert.deepEqual(labels(gridCellMenu("Footprint", true)), ["Select Footprint...", "---", "Cut", "Copy", "Paste", "Delete", "Select All"]);
  assert.deepEqual(labels(gridCellMenu("Datasheet", true)), ["Show Datasheet", "---", "Cut", "Copy", "Paste", "Delete", "Select All"]);
  const ro = gridCellMenu("Reference", false);
  assert.deepEqual(ro.filter((i) => i.kind === "item" && !i.enabled).map((i) => (i.kind === "item" ? i.id : "")), ["cut", "paste", "delete"], "a read-only selection: Copy and Select All only");
  assert.deepEqual(columnMenu([{ id: "Value", label: "Value", shown: false }]), [{ kind: "item", id: "showhide:Value", label: "Value", enabled: true, checked: false }]);
});

test("cells go to the clipboard as tab-separated lines and come back as rows", () => {
  assert.equal(cellsAsText([["R1", "10k"], ["R2", "1k"]]), "R1\t10k\nR2\t1k");
  assert.deepEqual(textAsCells("a\tb\r\nc\td\n"), [["a", "b"], ["c", "d"]]);
});

test("ApplyBomPreset: the preset's columns first with its labels, the others hidden and ungrouped, an unknown sort field the Reference", () => {
  const preset = spec({ columns: [{ name: "Value", label: "Val", show: true, group_by: false }, { name: "Footprint", label: "Footprint", show: true, group_by: true }], sort_field: "Nope", filter: "R*" });
  const out = withPresetSpec(spec(), preset);
  assert.deepEqual(out.columns.map((c) => [c.name, c.label, c.show, c.group_by]), [
    ["Value", "Val", true, false],
    ["Footprint", "Footprint", true, true],
    ["Reference", "Reference", false, false],
    ["${QUANTITY}", "Qty", false, false],
    ["MPN", "MPN", false, false],
  ]);
  assert.deepEqual([out.sort_field, out.filter], ["Reference", "R*"]);
});

test("the dialog's preset lists: saved in name order, an overwrite replaces, a delete removes", () => {
  let l = heldWithSaved([], "B", spec());
  l = heldWithSaved(l, "A", spec());
  l = heldWithSaved(l, "B", spec({ filter: "C*" }));
  assert.deepEqual(l.map((p) => [p.name, p.spec.filter]), [["A", ""], ["B", "C*"]]);
  assert.deepEqual(heldWithout(l, "A").map((p) => p.name), ["B"]);
});

test("the format box: built-in and the project's by name, the separator, the two actions; the one the format is, a built-in name refused", () => {
  const pipes: BomFmt = { ...bomFmtPresets()[0]!, name: "Pipes", field_delimiter: "|" };
  assert.deepEqual(fmtPresetChoices([pipes]), ["CSV", "Pipes", "Semicolons", "TSV", "---", "Save preset...", "Delete preset..."]);
  assert.equal(matchingFmtPreset({ ...pipes, name: "" }, [pipes]), "Pipes");
  assert.equal(matchingFmtPreset({ ...pipes, field_delimiter: "#" }, [pipes]), null, "the separator");
  assert.equal(saveFmtOutcome("TSV", [pipes]), "read_only");
  assert.equal(saveFmtOutcome("Pipes", [pipes]), "overwrite");
  assert.equal(saveFmtOutcome("New", [pipes]), "new");
  assert.deepEqual(fmtWithSaved([pipes], "Hash", { ...pipes, field_delimiter: "#" }).map((p) => p.name), ["Hash", "Pipes"]);
});

test("savePresetsToSchematic when the dialog closes: only what changed, as one set_bom_view; nothing changed sends nothing", () => {
  const opened = { presets: [], view: spec(), fmtPresets: [], fmt: bomFmtPresets()[0]! };
  assert.equal(closeCmd(opened, opened), null);
  const now = { presets: heldWithSaved([], "Mine", spec()), view: spec({ sort_asc: false }), fmtPresets: [], fmt: bomFmtPresets()[1]! };
  const cmd = closeCmd(opened, now) as unknown as Record<string, unknown>;
  assert.equal(cmd.op, "set_bom_view");
  assert.equal((cmd.presets as Array<{ name: string }>)[0]!.name, "Mine");
  assert.equal((cmd.view as { sort_asc: boolean }).sort_asc, false);
  assert.equal((cmd.fmt_view as { field_delimiter: string }).field_delimiter, "\t");
  assert.equal(cmd.fmt_presets, undefined, "the format presets did not change");
});
