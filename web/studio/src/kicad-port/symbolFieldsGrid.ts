// The Fields grid and General box of Symbol Properties beyond the texts and looks (`DIALOG_SYMBOL_PROPERTIES` and `FIELDS_GRID_TABLE` at 8303b2ad; UI-LEFT-2 SS7):
//   * the grid's columns (`FIELDS_DATA_COL_ORDER`, `GetColLabelValue`): in a schematic frame `getColumnCount()` is `FDC_SCH_EDIT_COUNT`, Name .. Allow Autoplacement;
//     Private (`FDC_PRIVATE`) is the Symbol Editor's only. The dialog shows the first eight (`m_fieldsGrid->ShowHideColumns( "0 1 2 3 4 5 6 7" )`, not kept between
//     opens) and a right-click on the column labels toggles any (`GRID_TRICKS::onGridLabelRightClick`: a check item per column, `doPopupSelection` shows or hides it);
//   * Move Up / Move Down (`OnMoveUp` / `OnMoveDown`): a user field moves, the mandatory rows stay first (`row > m_fields->GetMandatoryRowCount()` to go up,
//     `row >= GetMandatoryRowCount()` to go down); OK gives the fields their order (`SetOrdinal( ordinal++ )`): `reorder_fields`;
//   * the General box's Angle and Mirror (`m_orientationCtrl`: "0", "+90", "-90", "180" for `SYM_ORIENT_0`, `_90`, `_270`, `_180`; `m_mirrorCtrl`: "Not mirrored",
//     "Around X axis", "Around Y axis" for none, `SYM_MIRROR_X`, `SYM_MIRROR_Y`), read from `GetOrientation()` and set absolutely: `set_symbol_orientation`;
//   * after OK, `SCH_EDIT_TOOL::Properties` places an autoplaced symbol's fields again (`autoplace_fields_kept`).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Cmd } from "../api/types";

/** `FIELDS_DATA_COL_ORDER` up to `FDC_SCH_EDIT_COUNT`. */
export type FieldColumn = "name" | "value" | "shown" | "show_name" | "h_align" | "v_align" | "italic" | "bold" | "text_size" | "orientation" | "pos_x" | "pos_y" | "font" | "color" | "allow_autoplace";

/** `FIELDS_GRID_TABLE::GetColLabelValue`, in the columns' order. */
export const FIELD_COLUMNS: ReadonlyArray<{ key: FieldColumn; label: string }> = [
  { key: "name", label: "Name" },
  { key: "value", label: "Value" },
  { key: "shown", label: "Show" },
  { key: "show_name", label: "Show Name" },
  { key: "h_align", label: "H Align" },
  { key: "v_align", label: "V Align" },
  { key: "italic", label: "Italic" },
  { key: "bold", label: "Bold" },
  { key: "text_size", label: "Text Size" },
  { key: "orientation", label: "Orientation" },
  { key: "pos_x", label: "X Position" },
  { key: "pos_y", label: "Y Position" },
  { key: "font", label: "Font" },
  { key: "color", label: "Color" },
  { key: "allow_autoplace", label: "Allow Autoplacement" },
];

/** `m_fieldsGrid->ShowHideColumns( "0 1 2 3 4 5 6 7" )`: the columns the dialog opens with. */
export const DEFAULT_SHOWN_COLUMNS = "0 1 2 3 4 5 6 7";

/** `WX_GRID::ShowHideColumns`: every column hidden, then each number of the list (separated by white space) that is a column shown. */
export function shownColumns(list: string): Set<FieldColumn> {
  const out = new Set<FieldColumn>();
  for (const tok of list.split(/[ \t\r\n]+/)) {
    if (tok === "") continue;
    const n = Number.parseInt(tok, 10);
    if (Number.isInteger(n) && n >= 0 && n < FIELD_COLUMNS.length) out.add(FIELD_COLUMNS[n]!.key);
  }
  return out;
}

/** `WX_GRID::GetShownColumnsAsString`. */
export function shownColumnsText(shown: ReadonlySet<FieldColumn>): string {
  return FIELD_COLUMNS.map((c, i) => (shown.has(c.key) ? String(i) : null))
    .filter((x) => x !== null)
    .join(" ");
}

/** `GRID_TRICKS::doPopupSelection` on a column's check item: a shown column is hidden, a hidden one shown. */
export function toggleColumn(shown: ReadonlySet<FieldColumn>, col: FieldColumn): Set<FieldColumn> {
  const out = new Set(shown);
  if (out.has(col)) out.delete(col);
  else out.add(col);
  return out;
}

/** The menu a right-click on the grid's column labels opens (`onGridLabelRightClick`): each column by its label, checked when it is shown. */
export function columnMenu(shown: ReadonlySet<FieldColumn>): Array<{ key: FieldColumn; label: string; checked: boolean }> {
  return FIELD_COLUMNS.map((c) => ({ ...c, checked: shown.has(c.key) }));
}

/**
 * The mandatory rows of a placed symbol's grid here: Reference, Value, Footprint and Datasheet. **Differs**: KiCad's `GetMandatoryRowCount()` is 5 for a symbol,
 * Description being KiCad 8's fifth mandatory field (`MANDATORY_FIELDS`); the model keeps a symbol's Description as a user field, which is listed and moved with them.
 */
export const MANDATORY_ROWS = 4;

/** `OnMoveUp`'s test (`row > m_fields->GetMandatoryRowCount()`): a user field that has another user field above it goes up. */
export const canMoveUp = (row: number, mandatory = MANDATORY_ROWS): boolean => row > mandatory;

/** `OnMoveDown`'s test (`row >= m_fields->GetMandatoryRowCount()`, and a row below it): a user field that is not the last goes down. */
export const canMoveDown = (row: number, rowCount: number, mandatory = MANDATORY_ROWS): boolean => row >= mandatory && row < rowCount - 1;

/** `std::swap( *( m_fields->begin() + row ), *( m_fields->begin() + row - 1 ) )` on the rows after the mandatory ones: `index` is a row of `rows`, the user rows. */
export function moveRow<T>(rows: readonly T[], index: number, up: boolean): T[] {
  const to = up ? index - 1 : index + 1;
  if (index < 0 || index >= rows.length || to < 0 || to >= rows.length) return [...rows];
  const out = [...rows];
  [out[index], out[to]] = [out[to]!, out[index]!];
  return out;
}

/**
 * OK's order (`SetOrdinal( ordinal++ )` over the rows): `reorder_fields` for the placed unit `key` when the grid's user rows (`now`, the names of the rows kept, in
 * the grid's order) are not in the order the server gives them after the dialog's other edits -- the fields the symbol had (`was`) in their order, then the
 * fields added here, which it lists by name (`eda_engine::fields::symbol_specs_in`). Null when they are.
 */
export function fieldOrderCmd(key: string, was: readonly string[], now: readonly string[]): Cmd | null {
  const expected = [...was.filter((n) => now.includes(n)), ...now.filter((n) => !was.includes(n)).sort()];
  if (expected.length === now.length && expected.every((n, i) => n === now[i])) return null;
  return { op: "sch_edit", verb: "reorder_fields", key, names: [...now] } as Cmd;
}

// ------------------------------------------------------------------------------------------------------------------------------ Angle and Mirror

/** `m_orientationCtrlChoices` with the angle each sets (`SYM_ORIENT_0`, `SYM_ORIENT_90`, `SYM_ORIENT_270`, `SYM_ORIENT_180`: degrees counter-clockwise). */
export const ANGLE_CHOICES: ReadonlyArray<{ label: string; angle: number }> = [
  { label: "0", angle: 0 },
  { label: "+90", angle: 90 },
  { label: "-90", angle: 270 },
  { label: "180", angle: 180 },
];

/** `m_mirrorCtrlChoices`: none, `SYM_MIRROR_X` (the state's `mirror: "x"`), `SYM_MIRROR_Y` (`"y"`). */
export const MIRROR_CHOICES: ReadonlyArray<{ label: string; mirror: "x" | "y" | null }> = [
  { label: "Not mirrored", mirror: null },
  { label: "Around X axis", mirror: "x" },
  { label: "Around Y axis", mirror: "y" },
];

/**
 * `TransferDataToWindow`'s `orientation = GetOrientation() & ~( SYM_MIRROR_X | SYM_MIRROR_Y )` and `mirror = GetOrientation() & ( .. )`: the choices of a symbol turned
 * `rot` degrees and mirrored `mirror` (the state keeps the orientation as `GetOrientation` reads it, at most one mirror). A turn that is not a quarter shows 0.
 */
export function orientationChoice(rot: number, mirror: "x" | "y" | null | undefined): { angle: number; mirror: "x" | "y" | null } {
  const q = ((Math.round(rot / 90) % 4) + 4) % 4;
  return { angle: Math.abs(rot - Math.round(rot / 90) * 90) < 1e-6 ? q * 90 : 0, mirror: mirror === "x" || mirror === "y" ? mirror : null };
}

/** `SetOrientation` from the two choices: `set_symbol_orientation` for the symbol `id` (`unit`) when a choice differs from what it was. Null when neither does. */
export function orientationCmd(id: string, unit: number | undefined, was: { angle: number; mirror: "x" | "y" | null }, now: { angle: number; mirror: "x" | "y" | null }): Cmd | null {
  if (was.angle === now.angle && was.mirror === now.mirror) return null;
  return { op: "sch_edit", verb: "set_symbol_orientation", id, ...(unit !== undefined ? { unit } : {}), angle: now.angle, ...(now.mirror ? { mirror: now.mirror } : {}) } as Cmd;
}

/** After OK (`SCH_EDIT_TOOL::Properties`: `AutoplaceFields( screen, fieldsAutoplaced )` when the fields are autoplaced): the batch's last field step for the item `key`. */
export const autoplaceKeptCmd = (key: string): Cmd => ({ op: "sch_edit", verb: "autoplace_fields_kept", keys: [key] }) as Cmd;
