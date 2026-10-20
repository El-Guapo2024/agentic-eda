// The fields grid of Symbol Properties (`DIALOG_LIB_SYMBOL_PROPERTIES` with `FIELDS_GRID_TABLE`, eeschema/dialogs/dialog_lib_symbol_properties.cpp): the rows it starts with,
// what OK writes back and what it refuses.
//
// * The rows are the symbol's own fields (`CopyFields`), the five mandatory ones first. A derived symbol also lists the non-mandatory fields of its parents, greyed and
//   "inherited" (`addInheritedFields`: a field of the same name becomes the symbol's own copy marked inherited, any other is appended).
// * OK saves the rows in order (`TransferDataFromWindow`): an inherited row that still equals its parent's is left out (the child inherits it), a non-mandatory row with no name
//   and no text is dropped, and a mandatory field is always kept -- an empty one just does not override the parent's (`LIB_SYMBOL::Flatten`).
// * Validate: a root symbol's Reference needs a prefix ("References must start with a letter."), a field with text needs a name ("Fields must have a name."), and field
//   names are not repeated (compared without case: "The name '%s' is already in use.").
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { LibrarySymbol } from "../api/types";
import { MANDATORY_FIELDS, allFields, isMandatoryField, type FieldRow } from "./symFields";

/** One row of the grid. */
export interface GridRow extends FieldRow {
  /** Stable identity while rows are added, removed and moved. */
  key: number;
  /** The row stands for a field a parent has (it is the parent's copy until it is edited). */
  inherited: boolean;
  /** The parent's field this row inherits; `null` for the symbol's own fields. */
  parent: FieldRow | null;
}

/** `GetRefDesPrefix`: the reference without its trailing digits and question marks. */
export function referencePrefix(text: string): string {
  return text.replace(/[?\d]+$/u, "");
}

/** Two fields that say the same thing (`SCH_FIELD::operator==` as far as the grid shows it: name, text, look and place). */
export function sameField(a: FieldRow, b: FieldRow): boolean {
  return (
    a.name === b.name &&
    a.text === b.text &&
    a.at.x === b.at.x &&
    a.at.y === b.at.y &&
    a.angle_deg === b.angle_deg &&
    a.size_mm === b.size_mm &&
    a.hidden === b.hidden &&
    a.show_name === b.show_name &&
    a.do_not_autoplace === b.do_not_autoplace &&
    a.bold === b.bold &&
    a.italic === b.italic &&
    a.h_align === b.h_align &&
    a.v_align === b.v_align
  );
}

/** The rows the grid starts with. `parentFlat` is the derived symbol's parent with everything it inherits (`null` for a root symbol). */
export function gridRowsFor(own: LibrarySymbol, parentFlat: LibrarySymbol | null): GridRow[] {
  let key = 1;
  const rows: GridRow[] = allFields(own).map((f) => ({ ...f, key: key++, inherited: false, parent: null }));
  if (!parentFlat) return rows;
  for (const pf of allFields(parentFlat)) {
    if (isMandatoryField(pf.name)) continue; // "Don't inherit mandatory fields"
    const i = rows.findIndex((r) => !isMandatoryField(r.name) && r.name === pf.name);
    if (i >= 0) {
      rows[i] = { ...rows[i]!, inherited: true, parent: pf }; // `SetFieldInherited`
    } else {
      rows.push({ ...pf, key: key++, inherited: true, parent: pf }); // `AddInheritedField`
    }
  }
  return rows;
}

/** What OK keeps of the rows, in order, as the symbol's own fields. */
export function savedFields(rows: readonly GridRow[]): FieldRow[] {
  const out: FieldRow[] = [];
  for (const r of rows) {
    const { key: _key, inherited, parent, ...field } = r;
    void _key;
    if (inherited && parent && sameField(field, parent)) continue; // "Skip inherited fields"
    if (!isMandatoryField(field.name) && field.text === "" && field.name === "") continue; // an empty row of the grid
    out.push(field);
  }
  return out;
}

/** `Validate` and the name check of `OnGridCellChanging`: the first problem, or null. `isRoot` is whether the symbol has no parent. */
export function gridProblem(rows: readonly GridRow[], isRoot: boolean): string | null {
  if (isRoot) {
    const ref = rows.find((r) => r.name === "Reference");
    if (!ref || referencePrefix(ref.text) === "") return "References must start with a letter.";
  }
  const seen = new Set<string>();
  for (const r of rows) {
    if (isMandatoryField(r.name)) continue;
    if (r.name.trim() === "" && r.text !== "") return "Fields must have a name.";
    if (r.name === "") continue;
    const k = r.name.toLowerCase();
    if (seen.has(k) || MANDATORY_FIELDS.some((m) => m.toLowerCase() === k)) return `The name '${r.name}' is already in use.`;
    seen.add(k);
  }
  return null;
}

/** `OnAddField`: `GetUserFieldName( m_fields->size() )` -- "Field" and the number of rows there are -- hidden, at the default text size. */
export function newUserField(rowCount: number, key: number, sizeMm = 1.27): GridRow {
  return {
    key,
    inherited: false,
    parent: null,
    name: `Field${rowCount}`,
    text: "",
    at: { x: 0, y: 0 },
    angle_deg: 0,
    size_mm: sizeMm,
    hidden: true,
    show_name: false,
    do_not_autoplace: false,
    bold: false,
    italic: false,
    h_align: "center",
    v_align: "center",
  };
}

/** `OnMoveUp` / `OnMoveDown`: a row swaps with its neighbour, but never into the mandatory rows. Returns the new rows, or the same ones when the move is not allowed. */
export function moveRow(rows: readonly GridRow[], index: number, dir: -1 | 1): readonly GridRow[] {
  const mandatory = MANDATORY_FIELDS.length;
  if (dir < 0 ? index <= mandatory : index < mandatory) return rows; // `row > mandatoryCount` for up, `row >= mandatoryCount` for down
  const to = index + dir;
  if (to < mandatory || to >= rows.length) return rows;
  const next = rows.slice();
  [next[index], next[to]] = [next[to]!, next[index]!];
  return next;
}
