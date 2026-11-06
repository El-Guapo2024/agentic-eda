// The right-click menu's submenus of one placed symbol and of one of its pins (eeschema/tools/sch_edit_tool.cpp at 8303b2ad, `SCH_EDIT_TOOL::Init`):
//   * Symbol Unit (`SYMBOL_UNIT_MENU`, at `SCH_CONDITIONS::SingleMultiUnitSymbol`): every unit of the symbol, its display name, " (already placed)" after a unit
//     the reference has placed, the symbol's own unit checked; then, past a separator, "Place unit %s" for each unit not placed (`ID_POPUP_SCH_PLACE_UNIT`);
//   * Pin Function (`ALT_PIN_FUNCTION_MENU`, at `SingleMultiFunctionPin`): the library pin's own name, then each of its alternates, the one the pin took
//     checked (`ID_POPUP_SCH_ALT_PIN_FUNCTION`, `SCH_EDIT_FRAME::SetAltPinFunction`).
// What each entry runs is a studio action (the menus' command ids are no TOOL_ACTIONs) with the unit or the function as its argument.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Cmd, LibPin, Schematic, SchematicSymbol } from "../api/types";
import type { MenuNode } from "../kicad/types";
import { parsePinItemId, referenceOfKey } from "./schPins";
import { drawnUnit, unitCountOf } from "./schUnits";
import { unitLetter } from "./unitLetter";

/** The actions the two submenus' entries run (`ID_POPUP_SCH_SELECT_UNIT`, `ID_POPUP_SCH_PLACE_UNIT`, `ID_POPUP_SCH_ALT_PIN_FUNCTION`): studio actions where KiCad's
 *  entry is a command id of its own. */
export const SELECT_UNIT_ACTION = "studio.SymbolUnit.select";
/** `SCH_ACTIONS::placeNextSymbolUnit` with `PLACE_SYMBOL_UNIT_PARAMS{ symbol, unit }`: the unit asked for. */
export const PLACE_UNIT_ACTION = "eeschema.InteractiveDrawing.placeNextSymbolUnit";
export const PIN_FUNCTION_ACTION = "studio.PinFunction.select";

/** `LIB_SYMBOL::GetUnitDisplayName( aUnit, false )`: the unit's own name (`(unit_name ..)`), else its letter (`LetterSubReference( aUnit, 'A' )`). */
export function unitDisplayName(lib: { unit_names?: Record<string, string> } | undefined, unit: number): string {
  return lib?.unit_names?.[String(unit)] ?? unitLetter(unit);
}

/** The one placed unit the menu is for: of the reference `ref`, the one whose `unit` is given, else the only one. */
function placedUnit(sch: Schematic, ref: string, unit?: number): SchematicSymbol | null {
  const placed = sch.symbols.filter((s) => s.id === ref);
  if (unit !== undefined) return placed.find((s) => s.unit === unit) ?? null;
  return placed.length >= 1 ? placed[0]! : null;
}

/** `SingleMultiUnitSymbol`: one symbol selected whose library symbol has two units or more. */
export function isMultiUnitSymbol(sch: Schematic, ref: string): boolean {
  const s = placedUnit(sch, ref);
  return !!s?.lib_id && unitCountOf(sch.lib_symbols[s.lib_id]) >= 2;
}

/**
 * `SYMBOL_UNIT_MENU::update` for the placed unit `unit` of `ref`. `GetUnplacedUnitsForSymbol` looks over every sheet of the hierarchy; the studio knows the
 * sheet in view's placements, so a unit placed on another sheet only is listed as not placed.
 */
export function symbolUnitMenu(sch: Schematic, ref: string, unit?: number): MenuNode | null {
  const s = placedUnit(sch, ref, unit);
  if (!s?.lib_id) return null;
  const lib = sch.lib_symbols[s.lib_id];
  const n = unitCountOf(lib);
  if (n < 2) return null;
  // the units this sheet's placement draws of the reference it gives the symbol (`GetRef( &sheet )`, `GetUnitSelection( &sheet )`; UI-LEFT S9)
  const shownRef = s.reference ?? s.id;
  const placed = new Set(sch.symbols.filter((x) => (x.reference ?? x.id) === shownRef && x.lib_id === s.lib_id).map(drawnUnit));
  const items: MenuNode[] = [];
  for (let u = 1; u <= n; u++) {
    // `if( missingUnits.count( ii + 1 ) == 0 ) unit_text += _( " (already placed)" );`
    const label = unitDisplayName(lib, u) + (placed.has(u) ? " (already placed)" : "");
    // the argument names the placed unit the menu is for and the unit chosen ("<unit>:<new unit>"): a reference's units are one item of the selection
    items.push({ type: "item", action: SELECT_UNIT_ACTION, arg: `${s.unit}:${u}`, label, checked: u === drawnUnit(s) });
  }
  const missing = Array.from({ length: n }, (_, i) => i + 1).filter((u) => !placed.has(u));
  if (missing.length > 0) {
    items.push({ type: "separator" });
    for (const u of missing) items.push({ type: "item", action: PLACE_UNIT_ACTION, arg: u, label: `Place unit ${unitDisplayName(lib, u)}` });
  }
  return { type: "submenu", label: "Symbol Unit", items };
}

/** What `SCH_EDIT_FRAME::SelectUnit` does when `newUnit` is chosen for the placed unit `unit` of `ref`. */
export type SelectUnit =
  /** `if( unitCount <= 1 || currentUnit == aUnit ) return;` */
  | { do: "nothing" }
  | { do: "cmd"; cmd: Cmd }
  /** "Symbol unit '%s' is already placed (on sheet '%s')": the question, with the two answers' labels. */
  | { do: "ask"; message: string; swapLabel: string; duplicateLabel: string; newUnit: number };

export function selectUnit(sch: Schematic, ref: string, unit: number, newUnit: number, sheetName: string): SelectUnit {
  const s = placedUnit(sch, ref, unit);
  if (!s?.lib_id) return { do: "nothing" };
  const lib = sch.lib_symbols[s.lib_id];
  const n = unitCountOf(lib);
  const target = Math.min(newUnit, n);
  // `currentUnit`: the unit of the sheet in view (`GetUnitSelection( &sheetPath )`); `FindSymbolByRefAndUnit` looks for the reference this sheet gives it
  const current = drawnUnit(s);
  if (n <= 1 || target === current) return { do: "nothing" };
  if (sch.symbols.some((x) => x !== s && (x.reference ?? x.id) === (s.reference ?? s.id) && x.lib_id === s.lib_id && drawnUnit(x) === target)) {
    const targetName = unitDisplayName(lib, target);
    const currName = unitDisplayName(lib, current);
    return {
      do: "ask",
      message: `Symbol unit '${targetName}' is already placed (on sheet '${sheetName || "Root"}')`,
      swapLabel: `Swap '${targetName}' and '${currName}'`,
      duplicateLabel: `Duplicate '${targetName}'`,
      newUnit: target,
    };
  }
  return { do: "cmd", cmd: setSymbolUnitCmd(ref, s.unit, target, false) };
}

/** The `SELECT_UNIT_ACTION` argument: the placed unit and the unit chosen. */
export function parseSelectUnitArg(arg: unknown): { unit: number; newUnit: number } | null {
  const m = /^(\d+):(\d+)$/.exec(String(arg ?? ""));
  return m ? { unit: Number(m[1]), newUnit: Number(m[2]) } : null;
}

export function setSymbolUnitCmd(ref: string, unit: number, newUnit: number, swap: boolean): Cmd {
  return { op: "sch_edit", verb: "set_symbol_unit", id: ref, unit, new_unit: newUnit, ...(swap ? { swap: true } : {}) };
}

/** One pin of a placed symbol with what it can be: its library pin (its base name and alternates) and the alternate it took. */
export interface PinFunctions {
  ref: string;
  unit: number;
  number: string;
  /** `GetLibPin()->GetName()`. */
  base: string;
  alternates: string[];
  /** `SCH_PIN::GetAlt()`: "" for none. */
  alt: string;
}

/** The library pin of `number` a placed symbol draws (its unit's, or a pin common to every unit). */
export function libPinOf(sch: Schematic, s: SchematicSymbol, number: string): LibPin | null {
  const lib = s.lib_id ? sch.lib_symbols[s.lib_id] : undefined;
  return lib?.pins.find((p) => p.number === number && (p.unit === 0 || p.unit === (s.unit || 1)) && (p.body_style === 0 || p.body_style === (s.body_style || 1))) ?? lib?.pins.find((p) => p.number === number) ?? null;
}

/** The pin `pinId` (`pin:<key>:<number>`) of the sheet, with its functions; null when it is no pin of a placed symbol or its library pin has no alternates. */
export function pinFunctionsOf(sch: Schematic, pinId: string): PinFunctions | null {
  const parsed = parsePinItemId(pinId);
  if (!parsed) return null;
  const ref = referenceOfKey(parsed.key);
  const hash = parsed.key.lastIndexOf("#");
  const unit = hash > 0 && parsed.key !== ref ? Number(parsed.key.slice(hash + 1)) : 1;
  const s = placedUnit(sch, ref, unit) ?? placedUnit(sch, ref);
  if (!s) return null;
  const lp = libPinOf(sch, s, parsed.number);
  // `SingleMultiFunctionPin`: `!pin->GetLibPin()->GetAlternates().empty()`
  if (!lp?.alternates?.length) return null;
  const alt = s.pins.find((p) => p.number === parsed.number)?.alt ?? "";
  return { ref, unit: s.unit, number: parsed.number, base: lp.name ?? "", alternates: lp.alternates.map((a) => a.name), alt };
}

/** `ALT_PIN_FUNCTION_MENU::update`. */
export function pinFunctionMenu(f: PinFunctions): MenuNode {
  const items: MenuNode[] = [{ type: "item", action: PIN_FUNCTION_ACTION, arg: f.base, label: f.base, checked: f.alt === "" || f.alt === f.base }];
  for (const name of f.alternates) {
    // "The default pin name is set above, avoid setting it again."
    if (name === f.base) continue;
    items.push({ type: "item", action: PIN_FUNCTION_ACTION, arg: name, label: name, checked: name === f.alt });
  }
  return { type: "submenu", label: "Pin Function", items };
}

/**
 * `SCH_EDIT_FRAME::SetAltPinFunction( pin, aFunction )`: "if( aFunction == aPin->GetName() ) aPin->SetAlt( wxEmptyString ); else aPin->SetAlt( aFunction );" --
 * one pin of one placed unit, one undo step ("Set Pin Function"). `GetName()` is the alternate the pin took, else its own name, so choosing the checked
 * entry again goes back to the pin's own function, and `SetAlt` makes the own name no alternate. Null when nothing changes.
 */
export function setPinFunctionCmd(f: PinFunctions, name: string): Cmd | null {
  const now = f.alt === f.base ? "" : f.alt;
  const shownName = now || f.base;
  const want = name === shownName || name === f.base ? "" : name;
  if (want === now) return null;
  return { op: "sch_edit", verb: "set_pin_alternates", id: f.ref, unit: f.unit, alternates: { [f.number]: want } };
}
