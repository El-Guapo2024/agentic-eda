// The Pin Table's rules (`eeschema/dialogs/dialog_lib_edit_pin_table.cpp`, `SYMBOL_EDITOR_EDIT_TOOL::PinTable`), pure so the unit tests can pin them:
//
// - The dialog edits copies of the pins (`TransferDataToWindow`: `m_pins.push_back( new SCH_PIN( *pin ) )`); OK replaces the symbol's pins with them
//   (`TransferDataFromWindow`) and the tool pushes one commit, "Edit Pins". Here OK is one batch of the verbs that turn the old pins into the edited
//   ones (`pinTableCommands`), so one Ctrl+Z undoes the whole table. The table acts on its own rows only: no Synchronized Pins Mode
//   (`PIN_INFO_FORMATTER::UpdatePin` never calls `SynchronizePins`).
// - The Unit and Body Style cells are `GRID_CELL_COMBOBOX`es of "ALL" and the symbol's units ("Unit A", or the unit's display name) and of "ALL",
//   "Standard", "Alternate" (`unitChoices`, `bodyStyleChoices`); on a symbol of one unit or one body style the cell says "ALL" and takes nothing
//   (`UpdatePin`: `if( !parent->IsMultiUnit() ) break;`).
// - A length edit is `SCH_PIN::ChangeLength` (the body end stays, the connection point moves); X and Y are `SetPosition` as typed.
// - Y is KiCad's internal Y, which points down: the file's Y negated (`parseSymbolPin`'s `parseXY( true )`; eeschema's `ORIGIN_TRANSFORMS` is the
//   identity), so the cell shows `-at.y` and a typed value is stored negated (`displayY`, `storedY`).
// - Add Pin is `OnAddRow` (`addedPin`).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Cmd, LibrarySymbol, LibrarySymbolPin } from "../api/types";
import { REPEAT_PIN_STEP_MM } from "./symEditItems";
import { unitLetter } from "./unitLetter";
import { angleDegToOrientation } from "./pinOrientation";

/** `UNITS_ALL` / `DEMORGAN_ALL` (symbol_edit_frame.h): `_HKI( "ALL" )`. */
export const ALL = "ALL";

export interface Choice {
  value: number;
  label: string;
}

/** `LIB_SYMBOL::GetUnitDisplayName( unit, true )`: the unit's display name, else "Unit A", "Unit B", ... */
export function unitDisplayName(sym: Pick<LibrarySymbol, "kicad">, unit: number): string {
  const names = (sym.kicad?.unit_names ?? undefined) as Record<string, string> | undefined;
  return names?.[String(unit)] ?? `Unit ${unitLetter(unit)}`;
}

/** The Unit cell's choices: "ALL" (0), then each unit (`for( int i = 1; i <= aSymbol->GetUnitCount(); i++ )`). */
export function unitChoices(sym: Pick<LibrarySymbol, "unit_count" | "kicad">): Choice[] {
  const out: Choice[] = [{ value: 0, label: ALL }];
  for (let u = 1; u <= Math.max(1, sym.unit_count); u++) out.push({ value: u, label: unitDisplayName(sym, u) });
  return out;
}

/** The Body Style cell's choices: "ALL" (0), then "Standard" (1) and "Alternate" (2) for a De Morgan symbol. */
export function bodyStyleChoices(sym: Pick<LibrarySymbol, "has_alternate_body_style">): Choice[] {
  return sym.has_alternate_body_style ? [{ value: 0, label: ALL }, { value: 1, label: "Standard" }, { value: 2, label: "Alternate" }] : [{ value: 0, label: ALL }];
}

/** `IsMultiUnit()` / `IsMultiBodyStyle()`: whether the cell takes a value at all. */
export function unitCellEditable(sym: Pick<LibrarySymbol, "unit_count">): boolean {
  return sym.unit_count > 1;
}
export function bodyStyleCellEditable(sym: Pick<LibrarySymbol, "has_alternate_body_style">): boolean {
  return sym.has_alternate_body_style;
}

/** What the Unit cell shows (`PIN_INFO_FORMATTER::Format`, `COL_UNIT`): "ALL" on a symbol of one unit or for a pin common to all units. */
export function unitCellText(sym: Pick<LibrarySymbol, "unit_count" | "kicad">, pin: Pick<LibrarySymbolPin, "unit">): string {
  return !unitCellEditable(sym) || pin.unit === 0 ? ALL : unitDisplayName(sym, pin.unit);
}

/** The Y a cell shows: KiCad's internal Y, the file's negated. */
export function displayY(fileY: number): number {
  return fileY === 0 ? 0 : -fileY;
}
/** The file Y for a typed cell value. */
export function storedY(shownY: number): number {
  return shownY === 0 ? 0 : -shownY;
}

/**
 * `DIALOG_LIB_EDIT_PIN_TABLE::OnAddRow`: `new SCH_PIN( m_symbol )` -- no number or name, unspecified, a line, pointing right, at the origin, the
 * default length and text sizes (`SYMBOL_EDITOR_SETTINGS::m_Defaults`), unit and body style 0 -- and, when the table has pins, the last row's
 * orientation, type, shape, unit and body style, one `m_Repeat.pin_step` (100 mil) from the last row's place: `pos.y -= step` for a left or right
 * pin, `pos.x += step` for an up or down one (KiCad's y points down, so the file's y grows).
 */
export function addedPin(rows: readonly LibrarySymbolPin[]): LibrarySymbolPin {
  const pin: LibrarySymbolPin = {
    number: "",
    name: "",
    electrical_type: "unspecified",
    shape: "line",
    at: { x: 0, y: 0 },
    angle_deg: 0,
    length_mm: 2.54,
    unit: 0,
    body_style: 0,
    hidden: false,
    name_size_mm: 1.27,
    number_size_mm: 1.27,
  };
  const last = rows[rows.length - 1];
  if (!last) return pin;
  const o = angleDegToOrientation(last.angle_deg);
  const snap = (v: number) => Math.round(v * 1e6) / 1e6;
  const at = o === "left" || o === "right" ? { x: last.at.x, y: snap(last.at.y + REPEAT_PIN_STEP_MM) } : { x: snap(last.at.x + REPEAT_PIN_STEP_MM), y: last.at.y };
  return { ...pin, angle_deg: last.angle_deg, electrical_type: last.electrical_type, shape: last.shape, unit: last.unit, body_style: last.body_style, at };
}

const samePin = (a: LibrarySymbolPin, b: LibrarySymbolPin) => JSON.stringify({ ...a, alternates: a.alternates ?? [] }) === JSON.stringify({ ...b, alternates: b.alternates ?? [] });

/**
 * OK: the verbs that make the symbol's pins the table's rows -- a removed row's pin is deleted, a changed row's pin is edited, a new row (no id) is
 * added -- as one batch, so the whole table is one undo step ("Edit Pins"). Rows the table did not change send nothing.
 */
export function pinTableCommands(libId: string, before: readonly LibrarySymbolPin[], rows: readonly LibrarySymbolPin[]): Cmd[] {
  const kept = new Set(rows.map((r) => r.id).filter((id): id is string => !!id));
  const cmds: Cmd[] = [];
  for (const p of before) if (p.id && !kept.has(p.id)) cmds.push({ op: "delete_symbol_pin", lib_id: libId, id: p.id });
  for (const r of rows) {
    if (!r.id) continue;
    const old = before.find((p) => p.id === r.id);
    if (old && !samePin(old, r)) cmds.push({ op: "edit_symbol_pin", lib_id: libId, id: r.id, pin: r });
  }
  for (const r of rows) if (!r.id) cmds.push({ op: "add_symbol_pin", lib_id: libId, pin: r });
  return cmds;
}
