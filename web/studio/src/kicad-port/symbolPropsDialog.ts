// The rules of Symbol Properties (`DIALOG_SYMBOL_PROPERTIES`, eeschema/dialogs/dialog_symbol_properties.cpp at 8303b2ad) beyond the fields' texts:
//   * the Pin Functions tab (`SCH_PIN_TABLE_DATA_MODEL`): every pin of the symbol (`m_symbol->GetRawPins()`, all units), sorted by number, with its base name,
//     the alternate it is assigned (a choice of the base name and the library pin's alternates, read-only for a pin with none), and the electrical type and
//     graphic style it has with it (`SCH_PIN::GetType` / `GetShape`); a column header sorts (`OnPinTableColSort`, `compare`); OK sets the assignments
//     (`SetAlt` on every pin of the number, `SyncOtherUnits`). A symbol of several body styles has the tab disabled;
//   * the General box's Unit (`m_unitChoice`: `GetUnitDisplayName( ii, false )`, disabled for one unit) and Body style choices;
//   * the fields grid's look columns (`FIELDS_GRID_TABLE`): Show Name, H Align, V Align, Italic, Bold, Text Size, Font and Color, sent as the field verbs
//     (`edit_field`, `set_field_looks`) for what changed.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Cmd, LibPin, PinElectricalType, PinShape, Schematic, SchematicSymbol, SchField } from "../api/types";
import { comparePinNumbers } from "./pinNumbersSummary";
import { PIN_SHAPE_OPTIONS, PIN_TYPE_OPTIONS } from "./symPinAlternates";
import { unitCountOf } from "./schUnits";
import { unitDisplayName } from "./schSymbolMenus";

// ------------------------------------------------------------------------------------------------------------------------- Pin Functions

/** `PIN_TABLE_COL_ORDER`. */
export type PinColumn = "number" | "base" | "alt" | "type" | "shape";

/** The tab's column labels (`dialog_symbol_properties_base.cpp`: `m_pinGrid->SetColLabelValue`). */
export const PIN_COLUMN_LABELS: Readonly<Record<PinColumn, string>> = { number: "Pin Number", base: "Base Pin Name", alt: "Alternate Assignment", type: "Electrical Type", shape: "Graphic Style" };

export interface PinRow {
  number: string;
  /** `GetBaseName()`: the library pin's own name. */
  base: string;
  /** The library pin's alternate functions, by name: the choices of the Alternate Assignment cell after the base name; empty: the cell is read-only. */
  alternates: Array<{ name: string; electrical_type: PinElectricalType; shape: PinShape }>;
  /** The alternate assigned, "" for none (`SCH_PIN::GetAlt`). */
  alt: string;
  /** The library pin's own type and shape (the alternate's, when one is assigned, are `typeOf` / `shapeOf`'s). */
  type: PinElectricalType;
  shape: PinShape;
}

/** `PinTypeNames()[ GetType() ]`, `PinShapeNames()[ GetShape() ]`: the names the cells show. */
export const pinTypeName = (t: PinElectricalType): string => PIN_TYPE_OPTIONS.find((o) => o.value === t)?.label ?? t;
export const pinShapeName = (s: PinShape): string => PIN_SHAPE_OPTIONS.find((o) => o.value === s)?.label ?? s;

/** `SCH_PIN::GetType()` with `m_alt`: the alternate's type when one is assigned. */
export function typeOf(r: PinRow): PinElectricalType {
  return (r.alt && r.alternates.find((a) => a.name === r.alt)?.electrical_type) || r.type;
}

export function shapeOf(r: PinRow): PinShape {
  return (r.alt && r.alternates.find((a) => a.name === r.alt)?.shape) || r.shape;
}

/** `SCH_PIN_TABLE_DATA_MODEL::GetValue( aPin, aCol )`: the Alternate Assignment cell is empty for a pin with no alternates, else the alternate or the base name. */
export function pinCell(r: PinRow, col: PinColumn): string {
  switch (col) {
    case "number":
      return r.number;
    case "base":
      return r.base;
    case "alt":
      return r.alternates.length === 0 ? "" : r.alt || r.base;
    case "type":
      return pinTypeName(typeOf(r));
    case "shape":
      return pinShapeName(shapeOf(r));
  }
}

/**
 * The rows of the tab for the placed symbol `s`: the pins of its library symbol in its body style (`UpdatePins`: "Don't filter by unit"), one per pin object, with
 * the alternate each placed unit of the reference on the sheet keeps for it, sorted by number (`SortRows( COL_NUMBER, true )`).
 */
export function pinRows(sch: Schematic, s: SchematicSymbol): PinRow[] {
  const lib = s.lib_id ? sch.lib_symbols[s.lib_id] : undefined;
  if (!lib) return [];
  const style = s.body_style || 1;
  const alts = new Map<string, string>();
  // the unit in question first: its own record wins over another unit's
  for (const u of [s, ...sch.symbols.filter((x) => x.id === s.id && x !== s)]) for (const p of u.pins) if (p.alt && !alts.has(p.number)) alts.set(p.number, p.alt);
  const rows = lib.pins
    .filter((p: LibPin) => p.body_style === 0 || p.body_style === style)
    .map((p: LibPin): PinRow => ({ number: p.number, base: p.name ?? "", alternates: p.alternates ?? [], alt: keptAlt(p, alts.get(p.number)), type: p.electrical_type, shape: p.shape }));
  return sortPinRows(rows, "number", true);
}

/** `SCH_PIN::SetAlt`'s rule: an alternate the library pin has, and not its own name. */
function keptAlt(p: LibPin, alt: string | undefined): string {
  return alt && alt !== p.name && (p.alternates ?? []).some((a) => a.name === alt) ? alt : "";
}

/**
 * `SCH_PIN_TABLE_DATA_MODEL::compare`: by the column's text, the pin number when the two are equal; numbers, names and assignments by `PIN_NUMBERS::Compare`,
 * the type and the style without case. Stable, as `std::sort` with this order is on distinct keys.
 */
export function sortPinRows(rows: readonly PinRow[], col: PinColumn, ascending: boolean): PinRow[] {
  const cmp = (a: PinRow, b: PinRow): number => {
    let c = col;
    let l = pinCell(a, c);
    let r = pinCell(b, c);
    if (l === r) {
      c = "number";
      l = a.number;
      r = b.number;
    }
    const v = c === "type" || c === "shape" ? l.toLowerCase().localeCompare(r.toLowerCase()) : comparePinNumbers(l, r);
    return ascending ? v : -v;
  };
  return [...rows].sort(cmp);
}

/** `OnPinTableCellEdited` / `SetValue( COL_ALT_NAME )`: the base name chosen is no alternate. */
export function assignAlt(r: PinRow, choice: string): PinRow {
  return { ...r, alt: choice === r.base ? "" : choice };
}

/**
 * "Update any assignments" (`TransferDataFromWindow`): the pins whose assignment changed, by number, as `set_pin_alternates` takes them -- every placed unit of the
 * reference on the sheet takes them (`SyncOtherUnits`). Null when nothing changed.
 */
export function pinAlternatesCmd(ref: string, unit: number, was: readonly PinRow[], now: readonly PinRow[]): Cmd | null {
  const before = new Map(was.map((r) => [r.number, r.alt]));
  const changed: Record<string, string> = {};
  for (const r of now) if ((before.get(r.number) ?? "") !== r.alt) changed[r.number] = r.alt;
  if (Object.keys(changed).length === 0) return null;
  return { op: "sch_edit", verb: "set_pin_alternates", id: ref, unit, alternates: changed, other_units: true };
}

/** "Alternate pin assignments are not available for symbols with multiple body styles." (the tab's tooltip when it is disabled). */
export const PIN_TABLE_DISABLED_TIP = "Alternate pin assignments are not available for symbols with multiple body styles.";

// ------------------------------------------------------------------------------------------------------------------------------- Unit

/** `m_unitChoice->Append( m_symbol->GetUnitDisplayName( ii, false ) )` for every unit; empty for a symbol of one unit (the choice is disabled). */
export function unitChoices(sch: Schematic, s: SchematicSymbol): Array<{ unit: number; label: string }> {
  const lib = s.lib_id ? sch.lib_symbols[s.lib_id] : undefined;
  const n = unitCountOf(lib);
  if (n < 2) return [];
  return Array.from({ length: n }, (_, i) => ({ unit: i + 1, label: unitDisplayName(lib, i + 1) }));
}

/**
 * Why the Unit chosen cannot be set (`SetUnitSelection( .., unit_selection ); SetUnit( unit_selection )`): KiCad makes a second placement of that unit when it is
 * on the sheet already, which the model cannot hold -- it is refused here with the reason, and the canvas's Symbol Unit submenu swaps the two. Null when it can.
 */
export function unitChangeError(sch: Schematic, s: SchematicSymbol, newUnit: number): string | null {
  if (newUnit === s.unit) return null;
  if (!sch.symbols.some((x) => x.id === s.id && x.unit === newUnit)) return null;
  const lib = s.lib_id ? sch.lib_symbols[s.lib_id] : undefined;
  return `Unit ${unitDisplayName(lib, newUnit)} of ${s.id} is already placed on this sheet: swap the two units from the symbol's Symbol Unit menu instead.`;
}

// ---------------------------------------------------------------------------------------------------------------------------- Fields

/** The look of one field as the grid edits it (`FIELDS_GRID_TABLE`'s `FDC_SHOW_NAME` .. `FDC_COLOR`). */
export interface FieldLook {
  nameShown: boolean;
  h: SchField["h"];
  v: SchField["v"];
  italic: boolean;
  bold: boolean;
  /** Millimetres, as the cell is typed. */
  sizeMm: number;
  /** `FDC_FONT`: "Default Font", "KiCad Font" or a family. */
  font: string;
  /** `FDC_COLOR`: `#rrggbb`, or "" for the layer's colour. */
  color: string;
}

/** `DEFAULT_FONT_NAME` and `KICAD_FONT_NAME`: the Font column's first two choices, before the machine's families (`fonts.Sort(); fonts.Insert( .., 0 )`). */
export const DEFAULT_FONT_NAME = "Default Font";
export const KICAD_FONT_NAME = "KiCad Font";

export function fontChoices(families: readonly string[]): string[] {
  return [DEFAULT_FONT_NAME, KICAD_FONT_NAME, ...[...families].filter((f) => f !== KICAD_FONT_NAME && f !== DEFAULT_FONT_NAME).sort()];
}

const hex2 = (n: number) => Math.max(0, Math.min(255, Math.round(n))).toString(16).padStart(2, "0");

/** A field's colour as the cell holds it: `#rrggbb`, "" for none. */
export function colorCell(c: SchField["color"]): string {
  return c && c.a > 0 ? `#${hex2(c.r)}${hex2(c.g)}${hex2(c.b)}` : "";
}

/** The colour the cell's text sets: "" is the layer's (every channel 0, `COLOR4D::UNSPECIFIED`). */
export function colorOfCell(text: string): { r: number; g: number; b: number; a: number } {
  const m = /^#?([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(text.trim());
  return m ? { r: parseInt(m[1]!, 16), g: parseInt(m[2]!, 16), b: parseInt(m[3]!, 16), a: 255 } : { r: 0, g: 0, b: 0, a: 0 };
}

export function lookOf(f: SchField): FieldLook {
  return {
    nameShown: !!f.name_shown,
    h: f.h,
    v: f.v,
    italic: !!f.italic,
    bold: !!f.bold,
    sizeMm: (f.size_nm && f.size_nm > 0 ? f.size_nm : 1_270_000) / 1e6,
    font: f.font || DEFAULT_FONT_NAME,
    color: colorCell(f.color),
  };
}

/** `m_textSize.Validate( 0.01, 1000.0, EDA_UNITS::MM )`. */
export const FIELD_TEXT_SIZE_MIN_MM = 0.01;
export const FIELD_TEXT_SIZE_MAX_MM = 1000;

/** The commands a field's look changes are: `edit_field` with what changed of its place-free looks, and `set_field_looks` for its font and colour. */
export function fieldLookCmds(id: string, was: FieldLook, now: FieldLook): Cmd[] {
  const out: Cmd[] = [];
  const edit: Record<string, unknown> = {};
  if (now.nameShown !== was.nameShown) edit.name_shown = now.nameShown;
  if (now.h !== was.h) edit.h = now.h;
  if (now.v !== was.v) edit.v = now.v;
  if (now.italic !== was.italic) edit.italic = now.italic;
  if (now.bold !== was.bold) edit.bold = now.bold;
  if (Math.abs(now.sizeMm - was.sizeMm) > 1e-9) edit.size_nm = Math.round(now.sizeMm * 1e6);
  if (Object.keys(edit).length > 0) out.push({ op: "sch_edit", verb: "edit_field", id, ...edit } as Cmd);
  const fontChanged = now.font !== was.font;
  const colorChanged = now.color !== was.color;
  if (fontChanged || colorChanged) {
    out.push({ op: "sch_edit", verb: "set_field_looks", ids: [id], ...(fontChanged ? { font: now.font } : {}), ...(colorChanged ? { color: colorOfCell(now.color) } : {}) });
  }
  return out;
}

/** Why a look cannot be set: the text size out of range. */
export function fieldLookError(name: string, l: FieldLook): string | null {
  if (!(l.sizeMm >= FIELD_TEXT_SIZE_MIN_MM && l.sizeMm <= FIELD_TEXT_SIZE_MAX_MM)) return `The text size of the ${name} field must be between ${FIELD_TEXT_SIZE_MIN_MM} and ${FIELD_TEXT_SIZE_MAX_MM} mm.`;
  return null;
}
