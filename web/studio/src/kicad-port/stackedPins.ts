// Stacked pins in the Symbol Editor -- `SYMBOL_EDITOR_EDIT_TOOL::ConvertStackedPins` / `ExplodeStackedPin` with `ExpandStackedPinNotation`
// (eeschema/tools/symbol_editor_edit_tool.cpp, common/string_utils.cpp at 8303b2ad). Several pins at one spot are drawn as one pin whose number reads
// like `[1-3,5]`; Convert folds co-located pins into that notation, Explode unfolds it back into one pin per number.
import type { Cmd, LibrarySymbolPin } from "../api/types";

/** `ParseAlphaNumericPin`: a pin number as (prefix, trailing number), the number -1 when it has none. */
export function parseAlphaNumericPin(num: string): [string, number] {
  const m = /^(.*?)([0-9]+)$/.exec(num);
  if (!m) return [num, -1];
  return [m[1]!, Number(m[2])];
}

/** `ExpandStackedPinNotation`: the numbers a pin number stands for (`[1-3,5]` -> 1 2 3 5), and whether the notation is valid (a bad one stands for itself). */
export function expandStackedPinNotation(name: string): { numbers: string[]; valid: boolean } {
  const hasOpen = name.includes("[");
  const hasClose = name.includes("]");
  if (hasOpen || hasClose) {
    if (!name.startsWith("[") || !name.endsWith("]")) return { numbers: [name], valid: false };
  }
  if (!name.startsWith("[") || !name.endsWith("]")) return { numbers: [name], valid: true };

  const inner = name.slice(1, -1);
  const out: string[] = [];
  for (const raw of inner.split(",")) {
    const part = raw.trim();
    if (part === "") continue;
    const dash = part.indexOf("-");
    if (dash >= 0) {
      const [startPrefix, startVal] = parseAlphaNumericPin(part.slice(0, dash).trim());
      const [endPrefix, endVal] = parseAlphaNumericPin(part.slice(dash + 1).trim());
      if (startPrefix !== endPrefix || startVal === -1 || endVal === -1 || startVal > endVal) return { numbers: [name], valid: false };
      for (let i = startVal; i <= endVal; i++) out.push(`${startPrefix}${i}`);
    } else {
      out.push(part);
    }
  }
  return out.length === 0 ? { numbers: [name], valid: false } : { numbers: out, valid: true };
}

const isNumeric = (s: string) => /^[+-]?[0-9]+$/.test(s);

/** The order both tools sort pin numbers in: purely numeric ones first, ascending; the rest by plain string order. */
export function comparePinNumbers(a: string, b: string): number {
  const na = isNumeric(a);
  const nb = isNumeric(b);
  if (na && nb) return Number(a) - Number(b);
  if (na !== nb) return na ? -1 : 1;
  return a < b ? -1 : a > b ? 1 : 0;
}

/** The notation `ConvertStackedPins` builds: runs of consecutive numbers with a common prefix collapse to `P1-P3` (two stay `P1,P2`), other numbers follow as they are. */
export function stackedNotation(numbers: readonly string[]): string {
  const groups = new Map<string, number[]>();
  const other: string[] = [];
  for (const n of numbers) {
    if (n === "") {
      other.push("(empty)");
      continue;
    }
    const [prefix, value] = parseAlphaNumericPin(n);
    if (value >= 0) groups.set(prefix, [...(groups.get(prefix) ?? []), value]);
    else other.push(n);
  }
  let result = "";
  // `std::map` visits the prefixes in string order.
  for (const prefix of [...groups.keys()].sort()) {
    if (result !== "") result += ",";
    const values = groups.get(prefix)!.sort((a, b) => a - b);
    let i = 0;
    while (i < values.length) {
      if (i > 0) result += ",";
      const start = values[i]!;
      let end = start;
      while (i + 1 < values.length && values[i + 1] === values[i]! + 1) {
        i++;
        end = values[i]!;
      }
      if (end > start + 1) result += `${prefix}${start}-${prefix}${end}`;
      else if (end === start + 1) result += `${prefix}${start},${prefix}${end}`;
      else result += `${prefix}${start}`;
      i++;
    }
  }
  for (const n of other) result += (result !== "" ? "," : "") + n;
  return `[${result}]`;
}

export type StackPlan = { ok: true; cmds: Cmd[]; description: string } | { ok: false; message: string };

const samePlace = (a: LibrarySymbolPin, b: LibrarySymbolPin) => a.at.x === b.at.x && a.at.y === b.at.y;

/**
 * The right-click menu's conditions (`canConvertStackedPins`, `canExplodeStackedPin` in `SYMBOL_EDITOR_EDIT_TOOL::Init`, symbol_editor_edit_tool.cpp at 8303b2ad):
 * Convert is offered when two or more pins and nothing else are selected, all at one place, or when the one selected pin shares its place with another pin;
 * Explode when the one selected item is a pin whose number is valid stacked notation of more than one number. `selected` holds every selected id, pins or not.
 */
export function stackedPinMenuState(pins: readonly LibrarySymbolPin[], selected: readonly string[]): { canConvert: boolean; canExplode: boolean } {
  const chosen = selected.map((id) => pins.find((p) => p.id === id));
  const first = chosen[0];
  let canConvert = false;
  if (chosen.length >= 2) canConvert = chosen.every((p) => p !== undefined && samePlace(p, chosen[0]!));
  else if (first) canConvert = pins.filter((p) => samePlace(p, first)).length >= 2;
  let canExplode = false;
  if (chosen.length === 1 && first) {
    const { numbers, valid } = expandStackedPinNotation(first.number);
    canExplode = valid && numbers.length > 1;
  }
  return { canConvert, canExplode };
}

/**
 * `ConvertStackedPins`: the selected pins (or, with one selected, every pin of the symbol at its place) become one pin -- the first by number -- numbered with
 * the stacked notation; the others are deleted. `pins` are all the pins of the open symbol, `selected` the ids selected.
 */
export function planConvertStackedPins(libId: string, pins: readonly LibrarySymbolPin[], selected: readonly string[]): StackPlan {
  const chosen = selected.map((id) => pins.find((p) => p.id === id)).filter((p): p is LibrarySymbolPin => p !== undefined);
  const toConvert = chosen.length === 1 ? pins.filter((p) => samePlace(p, chosen[0]!)) : chosen;
  if (toConvert.length < 2) return { ok: false, message: "At least two pins are needed to convert to stacked pins" };
  if (toConvert.some((p) => !samePlace(p, toConvert[0]!))) return { ok: false, message: "All pins must be at the same location" };
  const sorted = [...toConvert].sort((a, b) => comparePinNumbers(a.number, b.number));
  const notation = stackedNotation(sorted.map((p) => p.number));
  const master = sorted[0]!;
  const cmds: Cmd[] = [{ op: "edit_symbol_pin", lib_id: libId, id: master.id!, pin: { ...master, number: notation } }, ...sorted.slice(1).map((p): Cmd => ({ op: "delete_symbol_pin", lib_id: libId, id: p.id! }))];
  return { ok: true, cmds, description: `Convert ${sorted.length} Stacked Pins to '${notation}'` };
}

/**
 * `ExplodeStackedPin`: the one selected pin with stacked notation becomes one pin per number -- the original takes the smallest and is shown, each other is a
 * hidden copy of it (a hidden power-input copy is demoted to passive: "hidden power input pins act as global labels").
 */
export function planExplodeStackedPin(libId: string, pins: readonly LibrarySymbolPin[], selected: readonly string[]): StackPlan {
  const pin = selected.length === 1 ? pins.find((p) => p.id === selected[0]) : undefined;
  if (!pin) return { ok: false, message: "Select a single pin with stacked notation to explode" };
  const { numbers, valid } = expandStackedPinNotation(pin.number);
  if (!valid || numbers.length <= 1) return { ok: false, message: "Selected pin does not have valid stacked notation" };
  const sorted = [...numbers].sort((a, b) => {
    if (isNumeric(a) && isNumeric(b)) return Number(a) - Number(b);
    return a < b ? -1 : a > b ? 1 : 0;
  });
  const cmds: Cmd[] = [{ op: "edit_symbol_pin", lib_id: libId, id: pin.id!, pin: { ...pin, number: sorted[0]!, hidden: false } }];
  for (const number of sorted.slice(1)) {
    cmds.push({ op: "add_symbol_pin", lib_id: libId, pin: { ...pin, id: undefined, number, hidden: true, electrical_type: pin.electrical_type === "power_in" ? "passive" : pin.electrical_type } });
  }
  return { ok: true, cmds, description: "Explode Stacked Pin" };
}
