// Label Properties' fields grid (`DIALOG_LABEL_PROPERTIES`' `FIELDS_GRID_TABLE` over the label's fields, eeschema/dialogs/dialog_label_properties.cpp and
// fields_grid_table.cpp at 8303b2ad; UI-LEFT-3 S3-31): every column `FIELDS_GRID_TABLE` has in a schematic frame -- Name, Value, Show, Show Name, H Align,
// V Align, Italic, Bold shown as the dialog opens (`m_grid->ShowHideColumns( "0 1 2 3 4 5 6 7" )`), Text Size, Orientation, X Position, Y Position, Font, Color
// and Allow Autoplacement from the column labels' right-click menu (kicad-port/symbolFieldsGrid.ts). A global label's Sheet References are the first row, a field
// like the others but for its name, its text and its visibility (Schematic Setup's), which the grid does not change.
//
// X and Y Position are the field's offset from the label's anchor (`field_copy.Offset( -m_currentLabel->GetPosition() )`): a label's fields are in the
// sheet's frame. OK (`TransferDataFromWindow`) puts the fields back and, when a field's position, angle or justification changed (`positioningChanged`), takes
// the label's fields out of the autoplaced ones (`SetFieldsAutoplaced( AUTOPLACE_NONE )`): every field keeps its grid place. The commands, in one step:
//   1. `edit_field` of each field whose place changed, by the name it had, before anything else -- the first one leaves every other field where it is, as the
//      grid shows it (the label's spin, set after, does not move a field: `SetSpinStyle`); the Sheet References' looks ride in theirs;
//   2. `edit_label` with the rows' texts, Show, Italic, Bold, Text Size, Show Name and Allow Autoplacement (`LabelField`), the label's own changes;
//   3. `set_field_looks` of each field whose Font or Color changed, by the name it has then, and on a label placed by hand `edit_field` of a new field
//      whose place cells were changed (a new field is at the label's anchor with the angle and justification of the row before it: `OnAddField`).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Cmd, SchLabelField } from "../api/types";
import type { LabelFieldEdit } from "../api/schEditTypes";
import { colorCell, colorOfCell, DEFAULT_FONT_NAME, type FieldLook } from "./symbolPropsDialog";

/** One row of the grid: the stored name it had (`was`, none for a row added here), the Sheet References (`isr`), its text and Show, and its look and place
 *  columns (`look`, `wasLook`: the place columns absent for a row added here, which has no place until OK). */
export interface LabelGridRow {
  key: number;
  was: string | null;
  isr: boolean;
  name: string;
  text: string;
  visible: boolean;
  look: FieldLook;
  wasLook: FieldLook;
}

const LABEL_FIELD_SIZE_NM = 1_270_000;
const mm = (nm: number) => Math.round((nm / 1e6) * 1e4) / 1e4;

/** A field's look and place cells as the grid shows them (`FIELDS_GRID_TABLE::GetValue`), its position from the label's anchor `labelAt`. */
export function labelFieldLook(f: SchLabelField, labelAt: readonly [number, number]): FieldLook {
  return {
    nameShown: !!f.name_shown,
    h: f.h,
    v: f.v ?? "center",
    italic: !!f.italic,
    bold: !!f.bold,
    sizeMm: mm(f.size_nm && f.size_nm > 0 ? f.size_nm : LABEL_FIELD_SIZE_NM),
    font: f.font || DEFAULT_FONT_NAME,
    color: colorCell(f.color),
    vertical: !!f.vertical,
    xMm: mm(f.at[0] - labelAt[0]),
    yMm: mm(f.at[1] - labelAt[1]),
    allowAutoplace: f.allow_autoplace ?? true,
  };
}

/** The grid's rows for the label's fields as the server sends them (the Sheet References first, then its own). */
export function labelGridRows(fields: readonly SchLabelField[], labelAt: readonly [number, number], firstKey = 0): LabelGridRow[] {
  return fields.map((f, i) => {
    const look = labelFieldLook(f, labelAt);
    return { key: firstKey + i, was: f.name, isr: f.intersheet_refs, name: f.name, text: f.text, visible: f.visible, look, wasLook: look };
  });
}

/**
 * A row added by Add Field from `OnAddField`'s new field (kicad-port/schProperties.ts `newLabelField`). `before` is the look of the grid's last row
 * (`newField.SetAttributes( m_fields->at( m_fields->size() - 1 ) )`: its angle and justification) when the label has a place for its fields: the new field
 * is then at (0, 0), the label's anchor once OK offsets it (`field.Offset( m_currentLabel->GetPosition() )`; UI-LEFT-3 S3-17). With no row before it the
 * field is centred and horizontal (`SCH_FIELD`'s defaults); a new label's rows have no place (`before` absent, the X and Y cells empty).
 */
export function newLabelGridRow(key: number, f: LabelFieldEdit, before?: FieldLook | null): LabelGridRow {
  const look: FieldLook = { nameShown: !!f.name_shown, h: "center", v: "center", italic: f.italic, bold: !!f.bold, sizeMm: mm(f.size_nm && f.size_nm > 0 ? f.size_nm : LABEL_FIELD_SIZE_NM), font: DEFAULT_FONT_NAME, color: "", allowAutoplace: true };
  if (before !== undefined) Object.assign(look, { h: before?.h ?? "center", v: before?.v ?? "center", vertical: !!before?.vertical, xMm: 0, yMm: 0 });
  return { key, was: null, isr: false, name: f.name, text: f.text, visible: f.visible, look, wasLook: look };
}

/** The `edit_label` row of a grid row (the Sheet References have none: they are not one of the label's own fields). */
export function labelFieldEditOf(r: LabelGridRow): LabelFieldEdit {
  return {
    name: r.name,
    text: r.text,
    visible: r.visible,
    italic: r.look.italic,
    size_nm: Math.round(r.look.sizeMm * 1e6),
    bold: r.look.bold,
    name_shown: r.look.nameShown,
    ...(r.look.allowAutoplace === false ? { no_autoplace: true } : {}),
  };
}

const fieldId = (label: string, name: string) => `fld:${label}:${name}`;
const nm = (v: number | undefined) => Math.round((v ?? 0) * 1e6);

/** `positioningChanged( a, b )`: the position, the justification or the angle. */
export function positioningChanged(a: FieldLook, b: FieldLook): boolean {
  return nm(a.xMm) !== nm(b.xMm) || nm(a.yMm) !== nm(b.yMm) || a.h !== b.h || a.v !== b.v || !!a.vertical !== !!b.vertical;
}

/** Step 1: `edit_field` of each field whose place changed, by the name it had (and the Sheet References' looks, which no `edit_label` row carries). */
export function labelPlaceCmds(labelId: string, rows: readonly LabelGridRow[]): Cmd[] {
  const out: Cmd[] = [];
  for (const r of rows) {
    if (r.was === null) continue;
    const edit: Record<string, unknown> = {};
    if (positioningChanged(r.look, r.wasLook)) Object.assign(edit, { local_at: { x: nm(r.look.xMm), y: nm(r.look.yMm) }, local_vertical: !!r.look.vertical, h: r.look.h, v: r.look.v });
    if (r.isr) {
      if (r.look.nameShown !== r.wasLook.nameShown) edit.name_shown = r.look.nameShown;
      if (r.look.italic !== r.wasLook.italic) edit.italic = r.look.italic;
      if (r.look.bold !== r.wasLook.bold) edit.bold = r.look.bold;
      if (nm(r.look.sizeMm) !== nm(r.wasLook.sizeMm)) edit.size_nm = nm(r.look.sizeMm);
      if (r.look.allowAutoplace !== r.wasLook.allowAutoplace) edit.allow_autoplace = r.look.allowAutoplace;
    }
    if (Object.keys(edit).length > 0) out.push({ op: "sch_edit", verb: "edit_field", id: fieldId(labelId, r.was), ...edit } as Cmd);
  }
  return out;
}

/**
 * Step 3: the Font and Color of each field (`set_field_looks`), by the name the field has once `edit_label` is done; and, on a label whose fields were placed
 * by hand (`handPlaced`), a new field's place where the grid's X / Y Position, Orientation or H / V Align were changed from those it was added with (the
 * server puts it at the anchor like the field before it otherwise). On an autoplaced label `AutoplaceFields` places every field after OK, a new one too, and
 * `positioningChanged` compares only the fields the label had, so a new row's place cells change nothing there.
 */
export function labelLookCmds(labelId: string, rows: readonly LabelGridRow[], handPlaced = false): Cmd[] {
  const out: Cmd[] = [];
  for (const r of rows) {
    const name = r.isr ? (r.was ?? r.name) : r.name.trim() === "Net Class" ? "Netclass" : r.name.trim();
    if (!name) continue;
    if (r.was === null && handPlaced && positioningChanged(r.look, r.wasLook))
      out.push({ op: "sch_edit", verb: "edit_field", id: fieldId(labelId, name), local_at: { x: nm(r.look.xMm), y: nm(r.look.yMm) }, local_vertical: !!r.look.vertical, h: r.look.h, v: r.look.v } as Cmd);
    const fontChanged = r.look.font !== r.wasLook.font;
    const colorChanged = r.look.color !== r.wasLook.color;
    if (fontChanged || colorChanged) out.push({ op: "sch_edit", verb: "set_field_looks", ids: [fieldId(labelId, name)], ...(fontChanged ? { font: r.look.font } : {}), ...(colorChanged ? { color: colorOfCell(r.look.color) } : {}) });
  }
  return out;
}
