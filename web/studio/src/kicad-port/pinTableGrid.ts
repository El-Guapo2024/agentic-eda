// The Pin Table's grid (`eeschema/dialogs/dialog_lib_edit_pin_table.cpp` at 8303b2ad): its columns, what a cell shows, how rows are made, sorted and
// grouped, and the summary above the grid. Pure, so the unit tests can pin each rule; `components/symbol/PinTableDialog.tsx` draws it.
//
// - The columns are `COL_ORDER`'s fourteen, with `GetPinTableColLabel`'s names; the dialog opens with `ShowHideColumns( "0 1 2 3 4 5 9 10" )`, so
//   Count, Number, Name, Electrical Type, Graphic Style, Orientation, X Position and Y Position are shown and the rest come back from the column
//   header's menu (`GRID_TRICKS::onGridLabelRightClick`).
// - A row is one pin, or with "Group by name" every pin of one name (`RebuildRows`' `findRow`); its pins are sorted by `PIN_NUMBERS::Compare`
//   (`SortPins`) and the rows by the sort column (`SortRows` over `compare`: the Count column to begin with, ties broken by the Number column).
// - A cell is `PIN_TABLE_DATA_MODEL::GetValue`: `PIN_INFO_FORMATTER::Format` of each of the row's pins -- the type, style and orientation by their
//   names (`PinTypeNames()`, `PinShapeNames()`, `PinOrientationNames()`), lengths in the frame's units, Visible as 1 / 0, the unit's display name --
//   joined by ", " in the Number column and "-- mixed values --" (`INDETERMINATE_STATE`) elsewhere when the pins differ.
// - A cell edit is `SetValue`: the value goes to each pin of the row (`UpdatePin`); a grouped row's Number takes a comma-separated list and renumbers its
//   pins, adds a pin for each extra number (a copy of the row's last pin's name, orientation, type, shape and unit, a pin step on) and drops the
//   ones the list no longer names.
// - The filters (`OnFilterCheckBox`, `OnFilterChoice`, `RebuildRows`' `includedBy*`) and the summary (`updateSummary`: "Pin numbers:", "Pin count:",
//   "Duplicate pins:" over every pin of the table, `PIN_NUMBERS`).
// - Export (`PIN_TABLE_EXPORT::ExportData`): every column but Count, the values without units and Visible as True / False.
import type { LibrarySymbol, LibrarySymbolPin, PinElectricalType, PinShape } from "../api/types";
import { comparePinNumbers, PinNumbers } from "./pinNumbersSummary";
import { PIN_SHAPE_OPTIONS, PIN_TYPE_OPTIONS } from "./symPinAlternates";
import { angleDegToOrientation, orientationToAngleDeg, type PinOrientation } from "./pinOrientation";
import { ALL, displayY, storedY, unitDisplayName } from "./pinTable";
import { changePinLength } from "./symPinSync";
import { REPEAT_PIN_STEP_MM } from "./symEditItems";
import { strNumCmp } from "./schSheetPins";
import { unitLetter } from "./unitLetter";
import type { LengthUnit } from "../state/units";
import { bodyStyleCount, bodyStyleDescription, isMultiBodyStyle } from "./symBodyStyles";

/** `COL_ORDER`. */
export const COL = {
  PIN_COUNT: 0,
  NUMBER: 1,
  NAME: 2,
  TYPE: 3,
  SHAPE: 4,
  ORIENTATION: 5,
  NUMBER_SIZE: 6,
  NAME_SIZE: 7,
  LENGTH: 8,
  POSX: 9,
  POSY: 10,
  VISIBLE: 11,
  UNIT: 12,
  BODY_STYLE: 13,
} as const;
export const COL_COUNT = 14;

/** `GetPinTableColLabel`. */
export const COLUMN_LABELS: readonly string[] = [
  "Count",
  "Number",
  "Name",
  "Electrical Type",
  "Graphic Style",
  "Orientation",
  "Number Text Size",
  "Name Text Size",
  "Length",
  "X Position",
  "Y Position",
  "Visible",
  "Unit",
  "Body Style",
];

/** `m_grid->ShowHideColumns( "0 1 2 3 4 5 9 10" )`: what the dialog opens with. */
export const DEFAULT_SHOWN_COLUMNS: readonly number[] = [0, 1, 2, 3, 4, 5, 9, 10];

/** `INDETERMINATE_STATE` (include/widgets/ui_common.h). */
export const INDETERMINATE_STATE = "-- mixed values --";

/** `PinOrientationNames()`: `PIN_ORIENTATION`'s order (right, left, up, down). */
export const ORIENTATION_NAMES: Readonly<Record<PinOrientation, string>> = { right: "Right", left: "Left", up: "Up", down: "Down" };

/** `PinTypeNames()[ type ]`. */
export const pinTypeName = (t: PinElectricalType): string => PIN_TYPE_OPTIONS.find((o) => o.value === t)?.label ?? t;
/** `PinShapeNames()[ shape ]`. */
export const pinShapeName = (s: PinShape): string => PIN_SHAPE_OPTIONS.find((o) => o.value === s)?.label ?? s;

/** One pin of the table: a copy of the symbol's (`TransferDataToWindow`: `new SCH_PIN( *pin )`), named by a key of the dialog's own. */
export interface TablePin {
  key: string;
  pin: LibrarySymbolPin;
}

/** What a cell needs from the symbol and the frame. */
export interface GridContext {
  sym: Pick<LibrarySymbol, "unit_count" | "has_alternate_body_style" | "body_style_names" | "kicad">;
  /** `StringFromValue( v, aIncludeUnits )`: a length in mm in the frame's units (with the unit's name when asked). */
  formatLength: (mm: number, withUnits: boolean) => string;
  /** `ValueFromString`: a typed length in the frame's units, in mm. */
  parseLength: (text: string) => number | null;
}

/** `LIB_SYMBOL::GetUnitDisplayName( unit, false )`: the unit's display name, else its letter. */
function unitShortName(sym: GridContext["sym"], unit: number): string {
  const names = (sym.kicad?.unit_names ?? undefined) as Record<string, string> | undefined;
  return names?.[String(unit)] ?? unitLetter(unit);
}

/** `LIB_SYMBOL::GetBodyStyleDescription( style, true )`: "Standard" and "Alternate" of a De Morgan symbol, a custom style's name. */
function bodyStyleName(sym: GridContext["sym"], style: number): string {
  return bodyStyleDescription(sym, style);
}

/** `PIN_INFO_FORMATTER::Format` of one pin's cell (`aIncludeUnits`, the Visible column as 1 / 0 or True / False). */
export function formatPin(pin: LibrarySymbolPin, col: number, ctx: GridContext, includeUnits = true, trueFalse = false): string {
  switch (col) {
    case COL.NAME:
      return pin.name;
    case COL.NUMBER:
      return pin.number;
    case COL.TYPE:
      return pinTypeName(pin.electrical_type);
    case COL.SHAPE:
      return pinShapeName(pin.shape);
    case COL.ORIENTATION:
      return ORIENTATION_NAMES[angleDegToOrientation(pin.angle_deg)];
    case COL.NUMBER_SIZE:
      return ctx.formatLength(pin.number_size_mm ?? 1.27, includeUnits);
    case COL.NAME_SIZE:
      return ctx.formatLength(pin.name_size_mm ?? 1.27, includeUnits);
    case COL.LENGTH:
      return ctx.formatLength(pin.length_mm, includeUnits);
    case COL.POSX:
      return ctx.formatLength(pin.at.x, includeUnits);
    case COL.POSY:
      // KiCad's internal Y, which points down: the file's negated (r7 E7-S3)
      return ctx.formatLength(displayY(pin.at.y), includeUnits);
    case COL.VISIBLE:
      return trueFalse ? (pin.hidden ? "False" : "True") : pin.hidden ? "0" : "1";
    case COL.UNIT:
      if (ctx.sym.unit_count <= 1 || pin.unit === 0) return ALL;
      return unitDisplayName(ctx.sym, pin.unit);
    case COL.BODY_STYLE:
      if (!isMultiBodyStyle(ctx.sym) || pin.body_style === 0) return ALL;
      return bodyStyleName(ctx.sym, pin.body_style);
    default:
      return "";
  }
}

/** `PIN_TABLE_DATA_MODEL::GetValue( pins, col )`: the Number column joins the row's numbers, the others show one value or `INDETERMINATE_STATE`. */
export function rowValue(pins: readonly LibrarySymbolPin[], col: number, ctx: GridContext): string {
  let fieldValue = "";
  if (pins.length === 0) return fieldValue;
  for (const pin of pins) {
    const val = col === COL.PIN_COUNT ? String(pins.length) : formatPin(pin, col, ctx);
    if (col === COL.NUMBER) {
      if (fieldValue.length) fieldValue += ", ";
      fieldValue += val;
    } else if (!fieldValue.length) {
      fieldValue = val;
    } else if (val !== fieldValue) {
      fieldValue = INDETERMINATE_STATE;
    }
  }
  return fieldValue;
}

/** `UNITS_PROVIDER::ValueFromString` for sorting: the leading number of a cell, 0 when there is none ("-- mixed values --"). */
function sortValue(text: string): number {
  const v = Number.parseFloat(text);
  return Number.isNaN(v) ? 0 : v;
}

/** `PIN_TABLE_DATA_MODEL::compare`: whether `lhs` goes before `rhs` when sorting by `sortCol`. */
export function rowLess(lhs: readonly LibrarySymbolPin[], rhs: readonly LibrarySymbolPin[], sortCol: number, ascending: boolean, ctx: GridContext): boolean {
  let col = sortCol;
  let l = rowValue(lhs, col, ctx);
  let r = rowValue(rhs, col, ctx);
  if (l === r) {
    // "Secondary sort key is always COL_NUMBER"
    col = COL.NUMBER;
    l = rowValue(lhs, col, ctx);
    r = rowValue(rhs, col, ctx);
  }
  const cmp = (a: number, b: number) => (ascending ? a < b : b < a);
  switch (col) {
    case COL.NUMBER:
    case COL.NAME:
      return cmp(comparePinNumbers(l, r), 0);
    case COL.NUMBER_SIZE:
    case COL.NAME_SIZE:
    case COL.LENGTH:
    case COL.POSX:
    case COL.POSY:
      return cmp(sortValue(l), sortValue(r));
    default:
      return cmp(strNumCmp(l, r, false), 0);
  }
}

/** The filters of the right pane: `-1` is off; a unit or body style filter is the choice's index (`m_unitFilter->GetSelection()`). */
export interface RowFilters {
  unit: number;
  bodyStyle: number;
  /** `m_filterBySelection`: only the pins that are the editor's selected ones (`pinIsInEditorSelection`). */
  selection: readonly LibrarySymbolPin[] | null;
}

export const NO_FILTERS: RowFilters = { unit: -1, bodyStyle: -1, selection: null };

/** `RebuildRows`' `pinIsInEditorSelection`: a copy is selected when a selected pin has its number, name, unit and body style. */
function inSelection(pin: LibrarySymbolPin, selected: readonly LibrarySymbolPin[]): boolean {
  return selected.some((s) => s.number === pin.number && s.name === pin.name && s.unit === pin.unit && s.body_style === pin.body_style);
}

/**
 * `PIN_TABLE_DATA_MODEL::RebuildRows( pins, groupByName, false )`: the rows of the pins that pass the filters, a row per pin or per name, each row's pins
 * by number (`SortPins`), the rows by the sort column (`SortRows`). The rows hold the pins' keys.
 *
 * The filters as KiCad has them: a unit filter keeps the pins of that unit and those common to every unit (`GetUnit() == 0`), so its first choice, "ALL"
 * (index 0), keeps only the common ones; a body style filter keeps the pins of exactly that style (index 0, "ALL": only the common ones).
 */
export function rebuildRows(pins: readonly TablePin[], groupByName: boolean, filters: RowFilters, sortCol: number, ascending: boolean, ctx: GridContext): string[][] {
  const rows: TablePin[][] = [];
  for (const tp of pins) {
    const p = tp.pin;
    const byUnit = filters.unit === -1 || p.unit === 0 || p.unit === filters.unit;
    const byStyle = filters.bodyStyle === -1 || p.body_style === filters.bodyStyle;
    const bySelection = filters.selection === null || inSelection(p, filters.selection);
    if (!(byUnit && byStyle && bySelection)) continue;
    const at = groupByName ? rows.findIndex((r) => r[0] !== undefined && r[0].pin.name === p.name) : -1;
    if (at < 0) rows.push([tp]);
    else rows[at]!.push(tp);
  }
  for (const r of rows) r.sort((a, b) => comparePinNumbers(a.pin.number, b.pin.number));
  return sortRows(rows, sortCol, ascending, ctx).map((r) => r.map((tp) => tp.key));
}

/** `SortRows`. */
export function sortRows<T extends TablePin>(rows: readonly T[][], sortCol: number, ascending: boolean, ctx: GridContext): T[][] {
  const pinsOf = (r: readonly T[]) => r.map((tp) => tp.pin);
  return [...rows].sort((a, b) => (rowLess(pinsOf(a), pinsOf(b), sortCol, ascending, ctx) ? -1 : rowLess(pinsOf(b), pinsOf(a), sortCol, ascending, ctx) ? 1 : 0));
}

/** `OnColSort`: a click on the column sorting the rows turns its order round; on another column it sorts ascending. */
export function nextSort(current: { col: number; ascending: boolean } | null, clicked: number): { col: number; ascending: boolean } {
  return current && current.col === clicked ? { col: clicked, ascending: !current.ascending } : { col: clicked, ascending: true };
}

/** `updateSummary`: "Pin numbers:", "Pin count:" and "Duplicate pins:" over every pin of the table (a pin with no number is counted, not listed). */
export function pinTableSummary(pins: readonly TablePin[]): { numbers: string; count: string; duplicates: string } {
  const set = new PinNumbers();
  for (const tp of pins) if (tp.pin.number.length) set.insert(tp.pin.number);
  return { numbers: set.summary(), count: String(pins.length), duplicates: set.duplicatesText() };
}

/**
 * `PIN_INFO_FORMATTER::UpdatePin`: a cell's typed value into one pin. A name that is no choice changes nothing (`Index( aValue, false )`), a unit or body
 * style on a symbol that has only one is ignored, and a length edit keeps the body end (`ChangeLength`).
 */
export function updatePin(pin: LibrarySymbolPin, value: string, col: number, ctx: GridContext): LibrarySymbolPin {
  const ieq = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();
  const len = () => ctx.parseLength(value);
  switch (col) {
    case COL.NUMBER:
      return { ...pin, number: value };
    case COL.NAME:
      return { ...pin, name: value };
    case COL.TYPE: {
      const o = PIN_TYPE_OPTIONS.find((t) => ieq(t.label, value));
      return o ? { ...pin, electrical_type: o.value } : pin;
    }
    case COL.SHAPE: {
      const o = PIN_SHAPE_OPTIONS.find((t) => ieq(t.label, value));
      return o ? { ...pin, shape: o.value } : pin;
    }
    case COL.ORIENTATION: {
      const o = (Object.entries(ORIENTATION_NAMES) as [PinOrientation, string][]).find(([, n]) => ieq(n, value));
      return o ? { ...pin, angle_deg: orientationToAngleDeg(o[0]) } : pin;
    }
    case COL.NUMBER_SIZE: {
      const v = len();
      return v === null ? pin : { ...pin, number_size_mm: v };
    }
    case COL.NAME_SIZE: {
      const v = len();
      return v === null ? pin : { ...pin, name_size_mm: v };
    }
    case COL.LENGTH: {
      const v = len();
      return v === null ? pin : changePinLength(pin, v);
    }
    case COL.POSX: {
      const v = len();
      return v === null ? pin : { ...pin, at: { ...pin.at, x: v } };
    }
    case COL.POSY: {
      const v = len();
      return v === null ? pin : { ...pin, at: { ...pin.at, y: storedY(v) } };
    }
    case COL.VISIBLE: {
      // `boolFromString`: 1 / 0, True / False (any case); anything else is False
      const on = value === "1" || ieq(value, "True");
      return { ...pin, hidden: !on };
    }
    case COL.UNIT: {
      if (ctx.sym.unit_count <= 1) return pin;
      if (ieq(value, ALL)) return { ...pin, unit: 0 };
      for (let i = 1; i <= ctx.sym.unit_count; i++) {
        // `GetUnitDisplayName( i, true )` or `( i, false )`: the unit's name, else "Unit A" -- or the bare letter
        if (value === unitDisplayName(ctx.sym, i) || value === unitShortName(ctx.sym, i)) return { ...pin, unit: i };
      }
      return pin;
    }
    case COL.BODY_STYLE: {
      if (!isMultiBodyStyle(ctx.sym)) return pin;
      if (ieq(value, ALL)) return { ...pin, body_style: 0 };
      // `for( int i = 1; i <= GetBodyStyleCount(); i++ ) if( value == GetBodyStyleDescription( i, true ) )`
      for (let s = 1; s <= bodyStyleCount(ctx.sym); s++) if (value === bodyStyleDescription(ctx.sym, s)) return { ...pin, body_style: s };
      return pin;
    }
    default:
      return pin;
  }
}

/**
 * `SetValue` of a grouped row's Number cell: the list's numbers (comma-separated, each trimmed) renumber the row's pins in order; each extra number is a new
 * pin -- the row's last pin's name, orientation, type, shape and unit, `m_Repeat.pin_step` on from it (down a left or right pin, `pos.y -= step` in
 * KiCad's Y; along an up or down one) -- and the pins the list no longer reaches are removed from the table.
 */
export function renumberGroup(row: readonly TablePin[], value: string, newKey: () => string, stepMm = REPEAT_PIN_STEP_MM): { row: TablePin[]; added: TablePin[]; removed: string[] } {
  // `wxStringTokenizer( value, "," )`: with a delimiter that is no space, `wxTOKEN_DEFAULT` is `wxTOKEN_RET_EMPTY` -- an empty token in the middle is
  // returned, the empty one after a last comma is not; each token is then trimmed (`Trim( true ).Trim( false )`)
  const raw = value === "" ? [] : value.split(",");
  if (raw.length > 0 && raw[raw.length - 1] === "") raw.pop();
  const numbers = raw.map((t) => t.trim());
  const out: TablePin[] = [];
  const added: TablePin[] = [];
  numbers.forEach((n, i) => {
    if (i < row.length) {
      out.push({ key: row[i]!.key, pin: { ...row[i]!.pin, number: n } });
      return;
    }
    const last = out[out.length - 1] ?? row[row.length - 1];
    if (!last) return;
    const lp = last.pin;
    const o = angleDegToOrientation(lp.angle_deg);
    const snap = (v: number) => Math.round(v * 1e6) / 1e6;
    const at = o === "left" || o === "right" ? { x: lp.at.x, y: snap(lp.at.y + stepMm) } : { x: snap(lp.at.x + stepMm), y: lp.at.y };
    const pin: LibrarySymbolPin = {
      number: n,
      name: lp.name,
      electrical_type: lp.electrical_type,
      shape: lp.shape,
      at,
      angle_deg: lp.angle_deg,
      length_mm: 2.54,
      unit: lp.unit,
      body_style: 0,
      hidden: false,
      name_size_mm: 1.27,
      number_size_mm: 1.27,
    };
    const tp = { key: newKey(), pin };
    out.push(tp);
    added.push(tp);
  });
  // `while( pins.size() > i ) RemovePin( pins.back() )`
  const removed = row.slice(Math.min(numbers.length, row.length)).map((tp) => tp.key);
  return { row: out, added, removed };
}

/** `PIN_TABLE_EXPORT::ExportData`: the header row and a row per pin -- every column but Count, the values without their units, Visible as True / False. */
export function exportTable(pins: readonly LibrarySymbolPin[], ctx: GridContext): string[][] {
  const cols = [COL.NUMBER, COL.NAME, COL.TYPE, COL.SHAPE, COL.ORIENTATION, COL.NUMBER_SIZE, COL.NAME_SIZE, COL.LENGTH, COL.POSX, COL.POSY, COL.VISIBLE, COL.UNIT, COL.BODY_STYLE];
  return [cols.map((c) => COLUMN_LABELS[c]!), ...pins.map((p) => cols.map((c) => formatPin(p, c, ctx, false, true)))];
}

/**
 * `UNIT_BINDER` / `ValueFromString` of a typed length, in mm: a number, then an optional unit (mm, um, mil / mils / th, in / "); none is the frame's.
 * **Differs**: KiCad's `NUMERIC_EVALUATOR` also takes arithmetic (`2.54*2`); a text that is no number with a unit is refused (null: the pin is left as it is).
 */
export function parseTypedLength(text: string, units: LengthUnit): number | null {
  const m = /^\s*([-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?)\s*([a-zA-Z"]*)\s*$/.exec(text.replace(",", "."));
  if (!m) return null;
  const v = Number(m[1]);
  const unit = (m[2] ?? "").toLowerCase();
  const perMm: Record<string, number> = { mm: 1, um: 0.001, mil: 0.0254, mils: 0.0254, th: 0.0254, in: 25.4, '"': 25.4 };
  const scale = unit === "" ? (units === "mm" ? 1 : units === "mil" ? 0.0254 : 25.4) : perMm[unit];
  return scale === undefined || !Number.isFinite(v) ? null : v * scale;
}

/** `CSV_WRITER::WriteLines` with its defaults: comma-separated, every cell in double quotes (a quote doubled), "\n" after each line. The file and the
 *  clipboard get the same text (`SaveTabularDataToClipboard` puts the CSV on the clipboard as plain text). */
export function csvText(table: readonly string[][]): string {
  return table.map((r) => r.map((c) => `"${c.replace(/"/g, '""')}"`).join(",") + "\n").join("");
}
