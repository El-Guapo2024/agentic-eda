// Sheet Properties' fields grid for a sheet's user fields (`DIALOG_SHEET_PROPERTIES`'s `FIELDS_GRID_TABLE` at 8303b2ad; UI-LEFT-2 SS15): each row's Name, Value, Show,
// Show Name, H Align, V Align, Italic, Bold, Text Size, Orientation, X Position, Y Position, Font and Color, the columns `FIELDS_GRID_TABLE` has in a schematic frame.
// The dialog shows the first eight (`m_grid->ShowHideColumns( "0 1 2 3 4 5 6 7" )`) and a right-click on the column labels shows or hides the others
// (kicad-port/symbolFieldsGrid.ts). X and Y Position are the field's offset from the sheet's corner (`field_copy.Offset( -m_sheet->GetPosition() )`); a new field
// (`OnAddField`) is "Field<N>", hidden, at the corner, centred, at the Sheetname's angle (`SetTextAngle( m_fields->GetField( FIELD_T::SHEET_NAME )->GetTextAngle() )`).
//
// OK sends the texts, Show, Show Name, Italic, Bold and Text Size as `set_sheet_fields` (`SchExtras::sheet_fields`) and the place, angle, justification, font and
// colour as `set_sheet_field_looks` (`SchExtras::sheet_field_places`, `field_looks`), after it, in the dialog's one step.
// **Differs**: no Allow Autoplacement column (Autoplace Fields places a sheet's Sheetname and Sheetfile, never its user fields, so the cell would change nothing).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Cmd } from "../api/types";
import type { SchColor } from "../api/schEditTypes";
import { FIELD_COLUMNS, type FieldColumn } from "./symbolFieldsGrid";
import { DEFAULT_FONT_NAME, FIELD_TEXT_SIZE_MAX_MM, FIELD_TEXT_SIZE_MIN_MM, type FieldLook } from "./symbolPropsDialog";
import { colorToHex, hexToColor } from "./schProperties";

/** A user field as the scene sends it (`Sheet::user_fields`). */
export interface SheetUserFieldJson {
  name: string;
  text: string;
  visible?: boolean;
  italic?: boolean;
  size_nm?: number;
  bold?: boolean;
  name_shown?: boolean;
}

/** Its place and font as the scene sends them (`Sheet::user_field_looks`, by name). */
export interface SheetUserFieldLookJson {
  dx?: number;
  dy?: number;
  angle?: number;
  h?: "left" | "center" | "right";
  v?: "top" | "center" | "bottom";
  font?: string | null;
  color?: SchColor | null;
}

/** One row of the grid. */
export interface SheetFieldRow {
  name: string;
  text: string;
  visible: boolean;
  look: FieldLook;
}

/** The sheet field grid's columns: `FIELDS_GRID_TABLE`'s, but Allow Autoplacement (see the module doc). */
export const SHEET_FIELD_COLUMNS: ReadonlyArray<{ key: FieldColumn; label: string }> = FIELD_COLUMNS.filter((c) => c.key !== "allow_autoplace");

const DEFAULT_SIZE_NM = 1_270_000;
const mmOf = (nm: number) => Math.round((nm / 1e6) * 1e4) / 1e4;

/** A row from the scene's user field and its look. */
export function sheetFieldRow(f: SheetUserFieldJson, l: SheetUserFieldLookJson | undefined): SheetFieldRow {
  const c = l?.color;
  return {
    name: f.name,
    text: f.text,
    visible: f.visible ?? true,
    look: {
      nameShown: !!f.name_shown,
      h: l?.h ?? "center",
      v: l?.v ?? "center",
      italic: !!f.italic,
      bold: !!f.bold,
      sizeMm: mmOf(f.size_nm && f.size_nm > 0 ? f.size_nm : DEFAULT_SIZE_NM),
      font: l?.font || DEFAULT_FONT_NAME,
      color: c && c.a > 0 ? colorToHex(c) : "",
      vertical: (l?.angle ?? 0) === 90_000,
      xMm: mmOf(l?.dx ?? 0),
      yMm: mmOf(l?.dy ?? 0),
    },
  };
}

/** `OnAddField`: "Field<N>" (`GetUserFieldName( m_fields->size() )`, the two mandatory rows counted), hidden, at the corner, at the Sheetname's angle. */
export function newSheetFieldRow(rows: readonly SheetFieldRow[], sheetNameVertical: boolean): SheetFieldRow {
  return { ...sheetFieldRow({ name: `Field${rows.length + 2}`, text: "", visible: false }, { angle: sheetNameVertical ? 90_000 : 0 }) };
}

/** The Text Size cell's check (`FIELDS_GRID_TABLE`'s validator: 0.01 .. 1000 mm). */
export function sheetFieldError(rows: readonly SheetFieldRow[]): string | null {
  for (const r of rows) {
    const s = r.look.sizeMm;
    if (!(s >= FIELD_TEXT_SIZE_MIN_MM && s <= FIELD_TEXT_SIZE_MAX_MM)) return `The text size of the ${r.name} field must be between ${FIELD_TEXT_SIZE_MIN_MM} and ${FIELD_TEXT_SIZE_MAX_MM} mm.`;
  }
  return null;
}

const nmOf = (mm: number | undefined) => Math.round((mm ?? 0) * 1e6);

/** The `set_sheet_fields` row of a grid row (a size of 50 mil is the default, sent as none). */
function fieldOf(r: SheetFieldRow): SheetUserFieldJson {
  const size = nmOf(r.look.sizeMm);
  return { name: r.name.trim(), text: r.text, visible: r.visible, italic: r.look.italic, size_nm: size === DEFAULT_SIZE_NM ? 0 : size, bold: r.look.bold, name_shown: r.look.nameShown };
}

function lookOf(r: SheetFieldRow) {
  const color = r.look.color ? hexToColor(r.look.color) : null;
  return {
    name: r.name.trim(),
    dx: nmOf(r.look.xMm),
    dy: nmOf(r.look.yMm),
    angle: r.look.vertical ? 90_000 : 0,
    h: r.look.h,
    v: r.look.v,
    font: r.look.font === DEFAULT_FONT_NAME ? "" : r.look.font,
    color: color ?? { r: 0, g: 0, b: 0, a: 0 },
  };
}

const canon = (f: SheetUserFieldJson) => JSON.stringify({ name: f.name, text: f.text, visible: f.visible ?? true, italic: !!f.italic, size_nm: f.size_nm && f.size_nm !== DEFAULT_SIZE_NM ? f.size_nm : 0, bold: !!f.bold, name_shown: !!f.name_shown });

/**
 * OK's commands for the user fields: `set_sheet_fields` when a text, a name, the order, Show, Show Name, Italic, Bold or a size changed, then
 * `set_sheet_field_looks` for every kept field whose place, angle, justification, font or colour is not what the sheet has under that name (a renamed field's
 * look starts from none). A row with neither name nor text is dropped (`TransferDataFromWindow`).
 */
export function sheetFieldCmds(sheetId: string, rows: readonly SheetFieldRow[], was: readonly SheetUserFieldJson[], wasLooks: Readonly<Record<string, SheetUserFieldLookJson>>): Cmd[] {
  const kept = rows.filter((r) => r.name.trim() !== "" || r.text !== "");
  const fields = kept.map(fieldOf);
  const out: Cmd[] = [];
  const fieldsChanged = fields.length !== was.length || fields.some((f, i) => canon(f) !== canon(was[i]!));
  if (fieldsChanged) out.push({ op: "sch_edit", verb: "set_sheet_fields", id: sheetId, fields } as unknown as Cmd);
  const wasByName = new Map(was.map((f) => [f.name, f]));
  const looks = kept
    .map(lookOf)
    .filter((l) => {
      const before = wasByName.has(l.name) ? wasLooks[l.name] : undefined;
      const b = lookOf(sheetFieldRow({ name: l.name, text: "" }, before));
      return JSON.stringify(b) !== JSON.stringify(l);
    });
  if (looks.length > 0) out.push({ op: "sch_edit", verb: "set_sheet_field_looks", id: sheetId, fields: looks } as unknown as Cmd);
  return out;
}
