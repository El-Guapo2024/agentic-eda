// The Symbol Fields Table's rules of UI-LEFT-2 SS13 (KiCad 8303b2ad eeschema/dialogs/dialog_symbol_fields_table.cpp; common/grid_tricks.cpp;
// common/settings/bom_settings.cpp; eeschema/fields_data_model.cpp/.h):
//
//   * the field list's columns, "Field", "BOM Name", "Include", "Group By" (`VIEW_CONTROLS_GRID_DATA_MODEL::GetColLabelValue`), the BOM Name an edited label that
//     names the grid's column and the export's header (`OnViewControlsCellChanged`'s LABEL_COLUMN: `SetColLabelValue`);
//   * the Group By refusals ("The Quantity column cannot be grouped by.", "The Item Number column cannot be grouped by.");
//   * the grid's right-button menus (`FIELDS_EDITOR_GRID_TRICKS::showPopupMenu`: "Select Footprint..." on the Footprint column, "Show Datasheet" on the Datasheet
//     column, then `GRID_TRICKS::showPopupMenu`'s Cut / Copy / Paste / Delete / Select All; `onGridLabelRightClick`: a check item per column, shown or not);
//   * `ApplyBomPreset` of a preset the dialog holds (a project's one saved in this session is not on the server yet);
//   * the presets and formats kept in the dialog and written when it closes (`~DIALOG_SYMBOL_FIELDS_TABLE` -> `savePresetsToSchematic`): the view presets, the
//     format presets, the view and the format it closes on, as one `set_bom_view`.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BomFmt, Cmd, FieldsTableSpec } from "../api/types";
import { bomPresetJson, DELETE_PRESET, PRESET_SEPARATOR, SAVE_PRESET } from "./bomPresets";
import { bomFmtPresets } from "./fieldsTableStage";

/** The field list's column labels (`VIEW_CONTROLS_GRID_DATA_MODEL( true )`: for the BOM). */
export const FIELD_LIST_COLUMNS = ["Field", "BOM Name", "Include", "Group By"] as const;

/** `FIELDS_EDITOR_GRID_DATA_MODEL::QUANTITY_VARIABLE` and `ITEM_NUMBER_VARIABLE`. */
export const QUANTITY = "${QUANTITY}";
export const ITEM_NUMBER = "${ITEM_NUMBER}";

/** `OnViewControlsCellChanged`'s GROUP_BY_COLUMN: a refusal (`DisplayError`) for the two generated counts, else null. */
export function groupByRefusal(name: string, groupBy: boolean): string | null {
  if (!groupBy) return null;
  if (name === QUANTITY) return "The Quantity column cannot be grouped by.";
  if (name === ITEM_NUMBER) return "The Item Number column cannot be grouped by.";
  return null;
}

/** The five mandatory fields (`MANDATORY_FIELDS`): the first rows of the list, not renamed or removed. */
export const MANDATORY_FIELDS = ["Reference", "Value", "Footprint", "Datasheet", "Description"] as const;

/** `OnRemoveField` / `OnRenameField` on a mandatory row: "The first %d fields are mandatory." / "... mandatory and names cannot be changed.", else null. */
export function mandatoryRefusal(name: string, rename: boolean): string | null {
  if (!(MANDATORY_FIELDS as readonly string[]).includes(name)) return null;
  return rename ? `The first ${MANDATORY_FIELDS.length} fields are mandatory and names cannot be changed.` : `The first ${MANDATORY_FIELDS.length} fields are mandatory.`;
}

/** The BOM Name of `name` set to `label` (`SetColLabelValue`): the grid's header and the export's. */
export function specSetLabel(spec: FieldsTableSpec, name: string, label: string): FieldsTableSpec {
  return { ...spec, columns: spec.columns.map((c) => (c.name === name ? { ...c, label } : c)) };
}

/** One entry of a right-button menu: a command with its enabled state, a check item, or a separator. */
export type MenuItem = { kind: "item"; id: string; label: string; enabled: boolean; checked?: boolean } | { kind: "separator" };

/**
 * The grid cell's menu (`FIELDS_EDITOR_GRID_TRICKS::showPopupMenu` then `GRID_TRICKS::showPopupMenu`): "Select Footprint..." and a separator on the Footprint
 * column, "Show Datasheet" and a separator on the Datasheet column, then "Cut", "Copy", "Paste", "Delete" (the grid allows multi-cell edits: `m_multiCellEditEnabled`),
 * "Select All"; Cut, Paste and Delete enabled when a cell of the selection can be written (`anyCellsWritable`).
 */
export function gridCellMenu(column: string, anyWritable: boolean): MenuItem[] {
  const out: MenuItem[] = [];
  if (column === "Footprint") out.push({ kind: "item", id: "select_footprint", label: "Select Footprint...", enabled: true }, { kind: "separator" });
  else if (column === "Datasheet") out.push({ kind: "item", id: "show_datasheet", label: "Show Datasheet", enabled: true }, { kind: "separator" });
  out.push(
    { kind: "item", id: "cut", label: "Cut\tCtrl+X", enabled: anyWritable },
    { kind: "item", id: "copy", label: "Copy\tCtrl+C", enabled: true },
    { kind: "item", id: "paste", label: "Paste\tCtrl+V", enabled: anyWritable },
    { kind: "item", id: "delete", label: "Delete\tDel", enabled: anyWritable },
    { kind: "item", id: "select_all", label: "Select All\tCtrl+A", enabled: true }
  );
  return out;
}

/** `GRID_TRICKS::onGridLabelRightClick`: a check item per column, in the grid's order, by its label, checked when it is shown. */
export function columnMenu(columns: ReadonlyArray<{ id: string; label: string; shown: boolean }>): MenuItem[] {
  return columns.map((c) => ({ kind: "item", id: `showhide:${c.id}`, label: c.label, enabled: true, checked: c.shown }));
}

/** `GRID_TRICKS::doPopupSelection`'s copy: the cells of the selection, a row per line, the cells of a row tab-separated. */
export function cellsAsText(rows: readonly (readonly string[])[]): string {
  return rows.map((r) => r.join("\t")).join("\n");
}

/** `paste_text`: the clipboard's text as rows of cells (lines, then tabs), a trailing line end dropped. */
export function textAsCells(text: string): string[][] {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  if (lines.length > 1 && lines[lines.length - 1] === "") lines.pop();
  return lines.map((l) => l.split("\t"));
}

/**
 * `FIELDS_EDITOR_GRID_DATA_MODEL::ApplyBomPreset` on the view `view`: every column hidden and not grouped, then the preset's (each once, in its order, with its
 * label, show and group by) first, the others after; its grouping, sort (the Reference when its column is not there), filter and the two filters. A preset
 * column the view lacks is added (`AddColumn`).
 */
export function withPresetSpec(view: FieldsTableSpec, preset: FieldsTableSpec): FieldsTableSpec {
  const seen = new Set<string>();
  const columns: FieldsTableSpec["columns"] = [];
  for (const c of preset.columns) {
    if (!c.name || seen.has(c.name)) continue;
    seen.add(c.name);
    columns.push({ name: c.name, label: c.label || c.name, show: c.show, group_by: c.group_by });
  }
  for (const c of view.columns) if (!seen.has(c.name)) columns.push({ ...c, show: false, group_by: false });
  const sort = columns.some((c) => c.name === preset.sort_field) ? preset.sort_field : "Reference";
  return { ...view, columns, group_symbols: preset.group_symbols, sort_field: sort, sort_asc: preset.sort_asc, filter: preset.filter ?? "", exclude_dnp: !!preset.exclude_dnp, include_excluded_from_bom: !!preset.include_excluded_from_bom };
}

/** A project preset the dialog holds: its name and view. */
export interface HeldPreset {
  name: string;
  spec: FieldsTableSpec;
}

/** "Save preset..." into the dialog's list (`m_bomPresets[name] = m_dataModel->GetBomSettings()`): a new one added, one of the same name replaced; the list in name
 *  order (`std::map`). */
export function heldWithSaved(list: readonly HeldPreset[], name: string, spec: FieldsTableSpec): HeldPreset[] {
  return [...list.filter((p) => p.name !== name), { name, spec }].sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
}

/** "Delete preset..." from the dialog's list (`m_bomPresets.erase( presetName )`). */
export const heldWithout = <T extends { name: string }>(list: readonly T[], name: string): T[] => list.filter((p) => p.name !== name);

// ---------------------------------------------------------------------------------------------------------------- the export's format presets

/** The format preset box (`rebuildBomFmtPresetsWidget`): the built-in (CSV, TSV, Semicolons) and the project's, by name (`std::map`), then "---", "Save preset...",
 *  "Delete preset...". */
export function fmtPresetChoices(user: readonly BomFmt[]): string[] {
  const names = [...new Set([...bomFmtPresets().map((p) => p.name), ...user.map((p) => p.name)])].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
  return [...names, PRESET_SEPARATOR, SAVE_PRESET, DELETE_PRESET];
}

/** `syncBomFmtPresetSelection`: the first preset, by name, whose delimiters and flags are `fmt`'s (its name aside), else null (the separator). */
export function matchingFmtPreset(fmt: BomFmt, user: readonly BomFmt[]): string | null {
  const all = [...bomFmtPresets(), ...user].sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  const same = (p: BomFmt) =>
    p.field_delimiter === fmt.field_delimiter && p.string_delimiter === fmt.string_delimiter && p.ref_delimiter === fmt.ref_delimiter && p.ref_range_delimiter === fmt.ref_range_delimiter && p.keep_tabs === fmt.keep_tabs && p.keep_line_breaks === fmt.keep_line_breaks;
  return all.find(same)?.name ?? null;
}

/** What "Save preset..." of the format box does with a name: a built-in name is refused ("Default presets cannot be modified.\nPlease use a different name."), one of
 *  the project's asks "Overwrite existing preset?", any other is new. */
export function saveFmtOutcome(name: string, user: readonly BomFmt[]): "read_only" | "overwrite" | "new" {
  if (bomFmtPresets().some((p) => p.name === name)) return "read_only";
  return user.some((p) => p.name === name) ? "overwrite" : "new";
}

/** The format saved under `name` in the dialog's list (`m_bomFmtPresets[name] = GetCurrentBomFmtSettings()`), in name order. */
export function fmtWithSaved(list: readonly BomFmt[], name: string, fmt: BomFmt): BomFmt[] {
  return [...list.filter((p) => p.name !== name), { ...fmt, name }].sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
}

/** `BOM_FMT_PRESET`'s JSON (`to_json`). */
export const fmtJson = (f: BomFmt, name = f.name): Record<string, unknown> => ({
  name,
  field_delimiter: f.field_delimiter,
  string_delimiter: f.string_delimiter,
  ref_delimiter: f.ref_delimiter,
  ref_range_delimiter: f.ref_range_delimiter,
  keep_tabs: f.keep_tabs,
  keep_line_breaks: f.keep_line_breaks,
});

// ---------------------------------------------------------------------------------------------------------------- what the dialog keeps when it closes

/** What the project had when the dialog opened, and what the dialog holds as it closes. */
export interface PresetState {
  presets: readonly HeldPreset[];
  view: FieldsTableSpec | null;
  fmtPresets: readonly BomFmt[];
  fmt: BomFmt | null;
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/**
 * `savePresetsToSchematic` (the dialog's destructor): the view presets and the format presets the dialog holds when either list differs from the project's, the
 * view and the format it closes on when they differ from what it opened on -- one `set_bom_view`, or null when nothing changed. **Differs**: a view or a format the
 * table opened on and closes on is not written again (KiCad compares with `m_BomSettings`, which a project without one has as a default the dialog never shows).
 */
export function closeCmd(opened: PresetState, now: PresetState): Cmd | null {
  const cmd: Record<string, unknown> = { op: "set_bom_view" };
  const presetJson = (l: readonly HeldPreset[]) => l.map((p) => bomPresetJson(p.spec, p.name));
  if (!same(presetJson(opened.presets), presetJson(now.presets))) cmd.presets = presetJson(now.presets);
  if (now.view && (!opened.view || !same(bomPresetJson(opened.view, ""), bomPresetJson(now.view, "")))) cmd.view = bomPresetJson(now.view, "");
  if (!same(opened.fmtPresets.map((f) => fmtJson(f)), now.fmtPresets.map((f) => fmtJson(f)))) cmd.fmt_presets = now.fmtPresets.map((f) => fmtJson(f));
  if (now.fmt && (!opened.fmt || !same(fmtJson(opened.fmt, ""), fmtJson(now.fmt, "")))) cmd.fmt_view = fmtJson(now.fmt, "");
  return Object.keys(cmd).length > 1 ? (cmd as unknown as Cmd) : null;
}
