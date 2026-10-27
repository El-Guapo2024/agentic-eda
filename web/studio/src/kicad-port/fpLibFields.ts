// The fields grid of the Footprint Editor's Footprint Properties (`DIALOG_FOOTPRINT_PROPERTIES_FP_EDITOR`, pcbnew/dialogs/dialog_footprint_properties_fp_editor.cpp): the rows it
// starts with (the Reference, the Value and the user fields of the footprint in the library), what OK sends back and what it refuses.
//
// The Reference's text is `REF**` and the Value's the footprint's name until the file says otherwise; their layouts default to `CreateNewFootprint`'s (1 mm text just above and below the origin). The Datasheet
// and the Description are the third and fourth mandatory rows (hidden, empty, at the origin on the fabrication layer until the file says otherwise). A user field
// starts as `OnAddField` makes it: hidden, at the origin, on the fabrication layer, named `Field<n>`. The rules of a row are the board dialog's (kicad-port/fpFields.ts
// `checkFieldRow`) and `Validate`'s for the names: every user field has one, none is a mandatory field's, none repeats another's (compared without case here, as KiCad's grid does).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { FieldLayoutCmd, FootprintField, LibraryFootprint, Nm } from "../api/types";
import { checkFieldRow } from "./fpFields";

export const LIBRARY_REFERENCE_TEXT = "REF**";

/** The names a user field may not take: the mandatory fields' (`MANDATORY_FIELDS`) and the footprint's own keywords and filters. */
const RESERVED = ["reference", "value", "datasheet", "description", "footprint"];

/** One row of the grid: a field with the text it shows and the layout of that text. */
export interface LibFieldRow {
  /** Stable identity while rows are added, removed and moved. */
  key: number;
  name: string;
  text: string;
  layout: FieldLayoutCmd;
}

/** `CreateNewFootprint`'s Reference and Value (`fp_edit::default_library_reference_layout`, `default_library_value_layout`): REF** in the silkscreen's 1 mm / 0.1 mm text half a
 * height above the origin, the name in the fabrication layer's 1 mm / 0.15 mm text at (0, 1 mm). */
export const defaultReferenceLayout = (): FieldLayoutCmd => ({ at: { x: 0, y: -500_000 }, size: [1_000_000, 1_000_000], thickness: 100_000, layer: "F.SilkS", visible: true });
export const defaultValueLayout = (): FieldLayoutCmd => ({ at: { x: 0, y: 1_000_000 }, size: [1_000_000, 1_000_000], thickness: 150_000, layer: "F.Fab", visible: true });
/** `OnAddField`: hidden, at the origin, on the fabrication layer. */
export const newFieldLayout = (): FieldLayoutCmd => ({ at: { x: 0, y: 0 }, size: [1_000_000, 1_000_000], thickness: 150_000, layer: "F.Fab", visible: false });
/** The Datasheet and Description fields of a footprint that has none of its own (`FOOTPRINT::FOOTPRINT`'s `addField( FIELD_T::DATASHEET, F_Fab, false )`): hidden, at the origin,
 * on the fabrication layer, in `EDA_TEXT`'s default 1.27 mm with the automatic pen (`fp_edit::default_library_datasheet_layout`). */
export const defaultInfoFieldLayout = (): FieldLayoutCmd => ({ at: { x: 0, y: 0 }, size: [1_270_000, 1_270_000], thickness: 0, layer: "F.Fab", visible: false });

/** The four mandatory rows come first -- Reference, Value, Datasheet, Description (`PCB_FIELDS_GRID_TABLE::GetMandatoryRowCount`, a footprint's mandatory fields): they are never
 * moved, renamed or deleted. */
export const MANDATORY_ROWS = 4;

/** The rows the grid starts with. A field's `visible` is its Show check box (a layout carries it too, but the footprint's flags are the ones the file is written from). */
export function rowsOfFootprint(fp: Pick<LibraryFootprint, "name" | "reference" | "value" | "reference_visible" | "value_visible" | "fields" | "datasheet" | "description_field">): LibFieldRow[] {
  const reference: FieldLayoutCmd = { ...defaultReferenceLayout(), ...(fp.reference ?? {}), visible: fp.reference_visible !== false };
  const value: FieldLayoutCmd = { ...defaultValueLayout(), ...(fp.value ?? {}), visible: fp.value_visible !== false };
  const info = (l: FieldLayoutCmd | null | undefined): FieldLayoutCmd => ({ ...defaultInfoFieldLayout(), ...(l ?? {}), text: undefined });
  const rows: LibFieldRow[] = [
    { key: 1, name: "Reference", text: fp.reference?.text ?? LIBRARY_REFERENCE_TEXT, layout: { ...reference, text: undefined } },
    { key: 2, name: "Value", text: fp.value?.text ?? fp.name, layout: { ...value, text: undefined } },
    { key: 3, name: "Datasheet", text: fp.datasheet?.text ?? "", layout: info(fp.datasheet) },
    { key: 4, name: "Description", text: fp.description_field?.text ?? "", layout: info(fp.description_field) },
  ];
  (fp.fields ?? []).forEach((f, i) => rows.push({ key: 5 + i, name: f.name, text: f.value, layout: { ...newFieldLayout(), ...(f.layout ?? {}), visible: f.visible !== false } }));
  return rows;
}

/** `GetUserFieldName( ordinal )`: `Field<rows>`, or the next number that is free. */
export function newRowName(rows: readonly LibFieldRow[]): string {
  const taken = new Set(rows.map((r) => r.name.toLowerCase()));
  for (let n = rows.length; ; n++) if (!taken.has(`field${n}`)) return `Field${n}`;
}

/** The first problem of the grid in the dialog's words, or null: each row's size and pen, then the names. `fmt` writes a length in the units on show. */
export function gridProblem(rows: readonly LibFieldRow[], fmt: (um: Nm) => string): string | null {
  const seen = new Set<string>();
  for (let i = 0; i < rows.length; i++) {
    const r = rows[i]!;
    const label = r.name || `Field ${i + 1}`;
    const bad = checkFieldRow(r.name, { size: r.layout.size, thickness: r.layout.thickness }, fmt);
    if (bad) return `${label}: ${bad}`;
    if (i < MANDATORY_ROWS) continue;
    const name = r.name.trim();
    if (name !== r.name) return `The field name "${r.name}" starts or ends with a space.`;
    if (RESERVED.includes(name.toLowerCase())) return `"${name}" is the name of a mandatory field.`;
    if (name.startsWith("ki_")) return `"${name}" is the footprint's own keywords or footprint filters, not a field.`;
    if (seen.has(name.toLowerCase())) return `There are two fields called "${name}".`;
    seen.add(name.toLowerCase());
  }
  return null;
}

/** What OK sends: the Reference's and the Value's layout (with their text when it is not the default), the Show flags, the Datasheet's and the Description's layout with their
 * text, and the user fields. */
export function panelOfRows(
  rows: readonly LibFieldRow[],
  footprintName: string,
): { reference: FieldLayoutCmd; value: FieldLayoutCmd; reference_visible: boolean; value_visible: boolean; datasheet: FieldLayoutCmd; description_field: FieldLayoutCmd; fields: FootprintField[] } {
  const [ref, val, datasheet, description, ...users] = rows;
  const info = (r: LibFieldRow | undefined): FieldLayoutCmd => ({ ...(r?.layout ?? defaultInfoFieldLayout()), text: r?.text ?? "" });
  const own = (r: LibFieldRow | undefined, text: string, fallback: FieldLayoutCmd): FieldLayoutCmd => {
    const l = r?.layout ?? fallback;
    // the layout's `text` is the text shown when it is not the field's own (`REF**`, the name)
    return r && r.text !== text ? { ...l, text: r.text } : { ...l, text: undefined };
  };
  const reference = own(ref, LIBRARY_REFERENCE_TEXT, defaultReferenceLayout());
  const value = own(val, footprintName, defaultValueLayout());
  return {
    reference,
    value,
    reference_visible: ref?.layout.visible !== false,
    value_visible: val?.layout.visible !== false,
    datasheet: info(datasheet),
    description_field: info(description),
    fields: users.map((r) => ({ name: r.name.trim(), value: r.text, visible: r.layout.visible !== false, layout: { ...r.layout, visible: r.layout.visible !== false } })),
  };
}

/** `OnMoveUp` / `OnMoveDown`: a user row swaps with its neighbour, never into the mandatory rows. */
export function moveRow<T>(rows: readonly T[], index: number, dir: -1 | 1): readonly T[] {
  const to = index + dir;
  if (index < MANDATORY_ROWS || to < MANDATORY_ROWS || to >= rows.length) return rows;
  const next = rows.slice();
  [next[index], next[to]] = [next[to]!, next[index]!];
  return next;
}
