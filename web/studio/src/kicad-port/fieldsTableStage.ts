// Staged edits for the Symbol Fields Table dialog
// (eeschema/dialogs/dialog_symbol_fields_table.cpp): KiCad's grid edits a
// private data store (`FIELDS_EDITOR_GRID_DATA_MODEL::m_dataStore`) and only
// `ApplyData`s it to the schematic on OK/Apply. These pure helpers keep that
// staged store as a `{ edits, add_fields, rename_fields, remove_fields }`
// value -- exactly the shape the one batched `set_symbol_fields` verb takes
// (applied remove -> rename -> add -> edits, edits addressed by FINAL field
// names, see crates/ops/src/fields_table.rs `apply_field_changes`) -- so
// "Apply" is one /api/cmd call and one undo step.
import type { BomFmt, Cmd, FieldsTableSpec, SymbolFieldEdit, SymbolFieldRename } from "../api/types";
import { setSymbolVariantCmd, type VariantFlagKey } from "./designVariants";

export interface StagedChanges {
  edits: SymbolFieldEdit[];
  add_fields: string[];
  rename_fields: SymbolFieldRename[];
  remove_fields: string[];
}

export const emptyChanges = (): StagedChanges => ({ edits: [], add_fields: [], rename_fields: [], remove_fields: [] });

export function isEmptyChanges(c: StagedChanges): boolean {
  return c.edits.length === 0 && c.add_fields.length === 0 && c.rename_fields.length === 0 && c.remove_fields.length === 0;
}

/** `SetValue`: stage `value` for (id, field), replacing any earlier staged edit of the same cell. */
export function stageEdit(c: StagedChanges, id: string, field: string, value: string): StagedChanges {
  const edits = c.edits.filter((e) => !(e.id === id && e.field === field));
  edits.push({ id, field, value });
  return { ...c, edits };
}

/** `AddColumn( name, ..., aAddedByUser = true )`: a new, empty user field. A no-op for a name already staged. */
export function stageAddField(c: StagedChanges, name: string): StagedChanges {
  if (c.add_fields.includes(name)) return c;
  return { ...c, add_fields: [...c.add_fields, name] };
}

/**
 * `RemoveColumn`: a field added in this same session just disappears
 * (with its staged edits); an existing one is queued for removal from
 * every symbol (`ApplyData`'s "no longer tracked -> erase" loop). A
 * previous rename of it is dropped -- the *original* name is what gets
 * removed.
 */
export function stageRemoveField(c: StagedChanges, name: string): StagedChanges {
  if (c.add_fields.includes(name)) {
    return { ...c, add_fields: c.add_fields.filter((f) => f !== name), edits: c.edits.filter((e) => e.field !== name) };
  }
  const rename = c.rename_fields.find((r) => r.to === name);
  const original = rename ? rename.from : name;
  return {
    edits: c.edits.filter((e) => e.field !== name),
    add_fields: c.add_fields,
    rename_fields: c.rename_fields.filter((r) => r.to !== name),
    remove_fields: c.remove_fields.includes(original) ? c.remove_fields : [...c.remove_fields, original],
  };
}

/**
 * `RenameColumn`: a field added this session is just renamed in place; an
 * existing one gets a rename entry, collapsing chains (a -> b then b -> c
 * is one a -> c) and dropping a rename back to the original name. Staged
 * edits follow the new name.
 */
export function stageRenameField(c: StagedChanges, from: string, to: string): StagedChanges {
  if (from === to) return c;
  const edits = c.edits.map((e) => (e.field === from ? { ...e, field: to } : e));
  if (c.add_fields.includes(from)) {
    return { ...c, edits, add_fields: c.add_fields.map((f) => (f === from ? to : f)) };
  }
  const chained = c.rename_fields.find((r) => r.to === from);
  const original = chained ? chained.from : from;
  const others = c.rename_fields.filter((r) => r !== chained);
  return { ...c, edits, rename_fields: original === to ? others : [...others, { from: original, to }] };
}

/** `RenameColumn` / `AddColumn` also change the view: the same op on the column list (a new column starts shown, not grouped). */
export function specAddColumn(spec: FieldsTableSpec, name: string): FieldsTableSpec {
  if (spec.columns.some((c) => c.name === name)) return spec;
  return { ...spec, columns: [...spec.columns, { name, label: name, show: true, group_by: false }] };
}

export function specRemoveColumn(spec: FieldsTableSpec, name: string): FieldsTableSpec {
  return { ...spec, columns: spec.columns.filter((c) => c.name !== name), sort_field: spec.sort_field === name ? "Reference" : spec.sort_field };
}

export function specRenameColumn(spec: FieldsTableSpec, from: string, to: string): FieldsTableSpec {
  return {
    ...spec,
    columns: spec.columns.map((c) => (c.name === from ? { ...c, name: to, label: c.label === from ? to : c.label } : c)),
    sort_field: spec.sort_field === from ? to : spec.sort_field,
  };
}

/**
 * A column's name is valid for a new/renamed user field (`DIALOG_SYMBOL_FIELDS_TABLE::OnAddField`): "Field must have a name." for a blank one, "Field name '%s' already
 * in use." for the name of a column, a mandatory field's among them, compared without regard to case (`CmpNoCase`); a generated name (`${...}`) is no user field.
 */
export function validNewFieldName(spec: FieldsTableSpec, name: string): string | null {
  const t = name.trim();
  if (t === "") return "Field must have a name.";
  if (["reference", "value", "footprint", "datasheet", "description"].includes(t.toLowerCase()) || spec.columns.some((c) => c.name.toLowerCase() === t.toLowerCase())) return `Field name '${t}' already in use.`;
  if (t.startsWith("${") && t.endsWith("}")) return `"${t}" is a built-in field.`;
  return null;
}

/** The attribute columns (`FIELDS_EDITOR_GRID_DATA_MODEL::isAttribute`): a symbol's flags as checkbox cells, "1" or "0". */
export const ATTRIBUTE_COLUMNS = ["${DNP}", "${EXCLUDE_FROM_BOM}", "${EXCLUDE_FROM_BOARD}", "${EXCLUDE_FROM_SIM}"] as const;

export function isAttributeColumn(name: string): boolean {
  return (ATTRIBUTE_COLUMNS as readonly string[]).includes(name);
}

/**
 * The cells of the grid a user can type into: not Reference (`ColIsReference`), not generated (`IsGeneratedField`, e.g. `${QUANTITY}`) -- but an attribute column is a cell to edit
 * (`SetValue` refuses "IsGeneratedField && !ColIsAttribute"), by its checkbox rather than by typing.
 */
export function isEditableColumn(name: string): boolean {
  return name !== "Reference" && (isAttributeColumn(name) || !(name.startsWith("${") && name.endsWith("}")));
}

/**
 * What clicking an attribute cell stages (`GRID_CELL_CHECKBOX_RENDERER` through `GRID_TRICKS`): a checked cell is cleared, an unchecked one set, and a cell that is
 * "-- mixed values --" (a group whose symbols differ) is set for all of them.
 */
export function toggledAttribute(text: string, mixed: boolean): "0" | "1" {
  if (mixed) return "1";
  return text === "1" ? "0" : "1";
}

/** How a checkbox cell stands: checked, unchecked, or indeterminate for a group whose symbols differ. */
export function attributeState(text: string, mixed: boolean): "checked" | "unchecked" | "mixed" {
  if (mixed) return "mixed";
  return text === "1" ? "checked" : "unchecked";
}

// ---- design variants (KiCad 25ec2335 dialog_symbol_fields_table.cpp, include/fields_table_data_model.h, eeschema/symbol_fields_data_model.cpp)

/**
 * `SYMBOL_FIELDS_EDITOR_GRID_DATA_MODEL::fieldSupportsVariants`: every column but the Reference and `${EXCLUDE_FROM_BOARD}` keeps a value per variant; an edit of those
 * two while a variant is shown is an edit of the symbol itself (`fieldVariant` files it under the default).
 */
export function fieldSupportsVariants(name: string): boolean {
  return name !== "Reference" && name !== "${EXCLUDE_FROM_BOARD}";
}

/** One staged cell of a variant (the data model's `FIELD_VALUE_STATE` of that variant): attribute columns as "1" or "0". */
export interface StagedVariantEdit {
  variant: string;
  id: string;
  field: string;
  value: string;
}

/** `SetValue` while `variant` is shown: the cell's staged value for that variant, replacing an earlier one of the same cell. */
export function stageVariantEdit(list: readonly StagedVariantEdit[], variant: string, id: string, field: string, value: string): StagedVariantEdit[] {
  return [...list.filter((e) => !(e.variant === variant && e.id === id && e.field === field)), { variant, id, field, value }];
}

/** `RenameColumn` and `RemoveColumn` act on every variant ("Field existence is shared by every variant"): the staged variant cells follow the name, or go with it. */
export function variantEditsRenamed(list: readonly StagedVariantEdit[], from: string, to: string): StagedVariantEdit[] {
  return list.map((e) => (e.field === from ? { ...e, field: to } : e));
}

export function variantEditsRemoved(list: readonly StagedVariantEdit[], name: string): StagedVariantEdit[] {
  return list.filter((e) => e.field !== name);
}

/** `DeleteStoredVariant` / `RenameStoredVariant`: a variant removed or renamed in the dialog takes its staged cells with it. */
export function deleteStoredVariant(list: readonly StagedVariantEdit[], name: string): StagedVariantEdit[] {
  return list.filter((e) => e.variant !== name);
}

export function renameStoredVariant(list: readonly StagedVariantEdit[], from: string, to: string): StagedVariantEdit[] {
  return list.map((e) => (e.variant === from ? { ...e, variant: to } : e));
}

/** The attribute columns a variant overrides, and the flag each stands for. */
const VARIANT_ATTRIBUTE: Readonly<Record<string, VariantFlagKey>> = {
  "${DNP}": "dnp",
  "${EXCLUDE_FROM_BOM}": "exclude_from_bom",
  "${EXCLUDE_FROM_SIM}": "exclude_from_sim",
  "${EXCLUDE_FROM_POS_FILES}": "exclude_from_pos_files",
};

/**
 * `ApplyData`'s loop over `storedVariants`: each (variant, symbol) of the staged cells becomes one `set_symbol_variant` -- its attribute cells as flags, the rest as
 * field texts (`SetFieldText( .., variantName )`: a text equal to the symbol's own drops the override on the server). In variant name, then reference order.
 */
export function variantApplyCmds(list: readonly StagedVariantEdit[]): Cmd[] {
  const groups = new Map<string, { variant: string; id: string; flags: Partial<Record<VariantFlagKey, boolean>>; fields: Record<string, string> }>();
  for (const e of list) {
    if (!fieldSupportsVariants(e.field)) continue;
    const key = `${e.variant}\u0000${e.id}`;
    let g = groups.get(key);
    if (!g) {
      g = { variant: e.variant, id: e.id, flags: {}, fields: {} };
      groups.set(key, g);
    }
    const flag = VARIANT_ATTRIBUTE[e.field];
    if (flag) g.flags[flag] = e.value === "1";
    else if (!(e.field.startsWith("${") && e.field.endsWith("}"))) g.fields[e.field] = e.value;
  }
  const out: Cmd[] = [];
  for (const g of [...groups.values()].sort((a, b) => (a.variant === b.variant ? (a.id < b.id ? -1 : a.id > b.id ? 1 : 0) : a.variant < b.variant ? -1 : 1))) {
    const cmd = setSymbolVariantCmd(g.variant, [g.id], { flags: g.flags, fields: g.fields });
    if (cmd) out.push(cmd);
  }
  return out;
}

/** `GetAttr`'s highlight colours of a cell whose value in the shown variant is the variant's own (`VARIANT_FIELD_OVERRIDE_LIGHT_YELLOW` / `_DARK_YELLOW`). */
export const VARIANT_OVERRIDE_COLOR = { light: "rgb(255, 255, 200)", dark: "rgb(80, 80, 40)" } as const;

// ---- BOM export format presets (common/settings/bom_settings.cpp)

/** `BOM_FMT_PRESET::BuiltInPresets()`: CSV, TSV, Semicolons -- the field values are KiCad's own. */
export function bomFmtPresets(): BomFmt[] {
  return [
    { name: "CSV", field_delimiter: ",", string_delimiter: '"', ref_delimiter: ",", ref_range_delimiter: "", keep_tabs: false, keep_line_breaks: false },
    { name: "TSV", field_delimiter: "\t", string_delimiter: "", ref_delimiter: ",", ref_range_delimiter: "", keep_tabs: false, keep_line_breaks: false },
    { name: "Semicolons", field_delimiter: ";", string_delimiter: "'", ref_delimiter: ",", ref_range_delimiter: "", keep_tabs: false, keep_line_breaks: false },
  ];
}

/** `syncBomFmtPresetSelection`: the preset whose settings equal `fmt` (ignoring the name), else null = "custom". */
export function matchingBomPreset(fmt: BomFmt): BomFmt | null {
  return (
    bomFmtPresets().find(
      (p) =>
        p.field_delimiter === fmt.field_delimiter &&
        p.string_delimiter === fmt.string_delimiter &&
        p.ref_delimiter === fmt.ref_delimiter &&
        p.ref_range_delimiter === fmt.ref_range_delimiter &&
        p.keep_tabs === fmt.keep_tabs &&
        p.keep_line_breaks === fmt.keep_line_breaks
    ) ?? null
  );
}
