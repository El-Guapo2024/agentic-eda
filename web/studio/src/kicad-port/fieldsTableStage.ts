// Staged edits for the Symbol Fields Table dialog
// (eeschema/dialogs/dialog_symbol_fields_table.cpp): KiCad's grid edits a
// private data store (`FIELDS_EDITOR_GRID_DATA_MODEL::m_dataStore`) and only
// `ApplyData`s it to the schematic on OK/Apply. These pure helpers keep that
// staged store as a `{ edits, add_fields, rename_fields, remove_fields }`
// value -- exactly the shape the one batched `set_symbol_fields` verb takes
// (applied remove -> rename -> add -> edits, edits addressed by FINAL field
// names, see crates/ops/src/fields_table.rs `apply_field_changes`) -- so
// "Apply" is one /api/cmd call and one undo step.
import type { BomFmt, FieldsTableSpec, SymbolFieldEdit, SymbolFieldRename } from "../api/types";

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

/** A column's name is valid for a new/renamed user field: non-blank, not a mandatory/generated name, not already a column (case-insensitive, like `FindFieldCaseInsensitive`). */
export function validNewFieldName(spec: FieldsTableSpec, name: string): string | null {
  const t = name.trim();
  if (t === "") return "A field needs a name.";
  if (["Reference", "Value", "Footprint", "Datasheet"].includes(t) || (t.startsWith("${") && t.endsWith("}"))) return `"${t}" is a built-in field.`;
  if (spec.columns.some((c) => c.name.toLowerCase() === t.toLowerCase())) return `A field named "${t}" already exists.`;
  return null;
}

/** The cells of the grid a user can type into: not Reference (`ColIsReference`), not generated (`IsGeneratedField`, e.g. `${QUANTITY}`). */
export function isEditableColumn(name: string): boolean {
  return name !== "Reference" && !(name.startsWith("${") && name.endsWith("}"));
}

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
