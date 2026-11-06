// The Symbol Fields Table's scope and view presets (UI-LEFT S11; KiCad 8303b2ad eeschema/dialogs/dialog_symbol_fields_table.cpp: `OnScope`, `setScope`,
// `rebuildBomPresetsWidget`, `syncBomPresetSelection`, `onBomPresetChanged`, `OnClose`; common/settings/bom_settings.cpp; KiCad 25ec2335 adds the "Selection"
// scope). The table's rows are the server's (`POST /api/sch/fields_table` with `scope`); the presets are the project's `schematic.bom_presets`, saved and deleted
// with `set_bom_view` (crates/ops/src/sch_setup.rs).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Cmd, FieldsTableSpec } from "../api/types";

export type FieldsTableScope = "all" | "sheet" | "sheet_and_down" | "selection";

/** `m_scope`'s choices (`dialog_symbol_fields_table_base.cpp`; "Selection" is KiCad 25ec2335's `SCOPE_SELECTION`). */
export const SCOPE_CHOICES: ReadonlyArray<{ value: FieldsTableScope; label: string }> = [
  { value: "all", label: "Entire Project" },
  { value: "sheet", label: "Current Sheet Only" },
  { value: "sheet_and_down", label: "Current Sheet and Down" },
  { value: "selection", label: "Selection" },
];

/** The preset box's last three entries (`rebuildBomPresetsWidget`): a separator, then the two actions. */
export const PRESET_SEPARATOR = "---";
export const SAVE_PRESET = "Save preset...";
export const DELETE_PRESET = "Delete preset...";

/** The preset box (`rebuildBomPresetsWidget`): every preset by name, the built-in and the project's together in name order (`std::map<wxString, BOM_PRESET>`),
 *  then "---", "Save preset...", "Delete preset...". */
export function presetChoices(builtIn: readonly string[], user: readonly string[]): string[] {
  const names = [...new Set([...builtIn, ...user])].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
  return [...names, PRESET_SEPARATOR, SAVE_PRESET, DELETE_PRESET];
}

type PresetLike = { name: string; spec: FieldsTableSpec };

/**
 * `syncBomPresetSelection`: the preset the view is, if any -- the same sort order, filter, grouping and the two filters, the same sort column, and the same columns
 * among those shown or grouped (name, label, shown, grouped, in order). Null: the box shows the separator.
 */
export function matchingPreset(spec: FieldsTableSpec, presets: readonly PresetLike[]): string | null {
  const shownOrGrouped = (s: FieldsTableSpec) => s.columns.filter((c) => c.show || c.group_by).map((c) => [c.name, c.label || c.name, c.show, c.group_by]);
  const same = (p: FieldsTableSpec) =>
    p.sort_asc === spec.sort_asc &&
    (p.filter ?? "") === (spec.filter ?? "") &&
    p.group_symbols === spec.group_symbols &&
    !!p.exclude_dnp === !!spec.exclude_dnp &&
    !!p.include_excluded_from_bom === !!spec.include_excluded_from_bom &&
    p.sort_field === spec.sort_field &&
    JSON.stringify(shownOrGrouped(p)) === JSON.stringify(shownOrGrouped(spec));
  const sorted = [...presets].sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  return sorted.find((p) => same(p.spec))?.name ?? null;
}

/** The view as a `BOM_PRESET` named `name`, as the project stores one (`bom_settings.cpp`'s `to_json`). */
export function bomPresetJson(spec: FieldsTableSpec, name: string): Record<string, unknown> {
  return {
    name,
    fields_ordered: spec.columns.map((c) => ({ name: c.name, label: c.label || c.name, show: c.show, group_by: c.group_by })),
    sort_field: spec.sort_field,
    sort_asc: spec.sort_asc,
    filter_string: spec.filter ?? "",
    group_symbols: spec.group_symbols,
    exclude_dnp: !!spec.exclude_dnp,
    include_excluded_from_bom: !!spec.include_excluded_from_bom,
  };
}

/** What "Save preset..." does with a name (`onBomPresetChanged`): refuse a built-in one, ask before replacing one of the project's, else add it. */
export function savePresetOutcome(name: string, builtIn: readonly string[], user: readonly string[]): "read_only" | "overwrite" | "new" {
  if (builtIn.includes(name)) return "read_only";
  return user.includes(name) ? "overwrite" : "new";
}

/** `wxMessageBox( _( "Default presets cannot be modified.\nPlease use a different name." ), _( "Error" ) )`. */
export const READ_ONLY_PRESET = "Default presets cannot be modified.\nPlease use a different name.";

/** The commands of "Save preset..." and "Delete preset...", and of the view the table closes on (`savePresetsToSchematic`'s `m_BomSettings`). */
export const savePresetCmd = (spec: FieldsTableSpec, name: string): Cmd => ({ op: "set_bom_view", save: bomPresetJson(spec, name) }) as unknown as Cmd;
export const deletePresetCmd = (name: string): Cmd => ({ op: "set_bom_view", delete: name }) as unknown as Cmd;
export const viewCmd = (spec: FieldsTableSpec): Cmd => ({ op: "set_bom_view", view: bomPresetJson(spec, "") }) as unknown as Cmd;

/** `HandleUnsavedChanges( this, _( "Save changes?" ), .. )` (common/confirm.cpp `UnsavedChangesDialog`): the title, the message, the extended text, the buttons. */
export const UNSAVED_CHANGES = {
  title: "Save Changes?",
  message: "Save changes?",
  detail: "If you don't save, all your changes will be permanently lost.",
  save: "Save",
  discard: "Discard Changes",
  cancel: "Cancel",
} as const;
