// The Symbol Checker (`eeschema.InspectionTool.checkSymbol`, `SCH_INSPECTION_TOOL::CheckSymbol` -> `CheckLibSymbol`, eeschema/symbol_checker.cpp
// at 8303b2ad): the warnings KiCad gives for a library symbol being edited -- an empty or digit-ended reference prefix, duplicate pins, a power
// symbol that is not one unit with one power pin, a hidden power pin, a pin off the grid, a circle of radius 0 or a rectangle of size 0.
//
// KiCad builds each message as a small HTML string (`<b>` for the key facts, `<br><br>` after it); here a message is its list of parts, so the
// dialog needs no HTML. This is KiCad's own symbol check, run on the symbol in the editor -- not a copy of an ERC or DRC engine.
import type { LibrarySymbol, LibrarySymbolPin } from "../api/types";
import { expandStackedPinNotation } from "./stackedPins";
import { unitLetter } from "./unitLetter";

/** One piece of a message: plain text, or one of the bold facts. */
export interface CheckPart {
  text: string;
  bold?: boolean;
}
/** One warning, as the parts of its text in order. */
export type CheckMessage = CheckPart[];

/** `MessageTextFromValue` for a length in mm: the frame's units. */
export type LengthFormatter = (mm: number) => string;

const IU_PER_MM = 1_000_000; // KiCad's internal unit is the nanometre
const MIL_IU = 25_400;
/** `schIUScale.MilsToIU( 25 )`: the smallest grid a pin may sit on. */
export const MIN_PIN_GRID_IU = 25 * MIL_IU;

const b = (text: string): CheckPart => ({ text, bold: true });
const t = (text: string): CheckPart => ({ text });

/** `wxString::Cmp`. */
function cmp(a: string, c: string): number {
  return a < c ? -1 : a > c ? 1 : 0;
}

/** `sort_by_pin_number`: number, then body style, then unit. */
function comparePins(a: LibrarySymbolPin, c: LibrarySymbolPin): number {
  return cmp(a.number, c.number) || a.body_style - c.body_style || a.unit - c.unit;
}

/** `LIB_SYMBOL::GetBodyStyleDescription( style, true ).Lower()` for the two styles a symbol with a DeMorgan alternate has. */
function bodyStyleName(style: number): string {
  return style === 2 ? "alternate" : style === 1 ? "standard" : "?";
}

/** `LIB_SYMBOL::GetUnitDisplayName( unit, false )`: the unit's letter (the IR keeps no custom unit names). */
function unitName(unit: number): string {
  return unitLetter(unit);
}

interface LogicalPin {
  pin: LibrarySymbolPin;
  number: string;
}

/** `CheckDuplicatePins`. */
export function checkDuplicatePins(symbol: LibrarySymbol, fmt: LengthFormatter): CheckMessage[] {
  const out: CheckMessage[] = [];
  const multiBody = symbol.has_alternate_body_style;
  const logical: LogicalPin[] = [];
  for (const pin of symbol.pins) {
    const { numbers, valid } = expandStackedPinNotation(pin.number);
    if (!valid || numbers.length === 0) logical.push({ pin, number: pin.number });
    else for (const number of numbers) logical.push({ pin, number });
  }
  logical.sort((l, r) => cmp(l.number, r.number) || l.pin.body_style - r.pin.body_style || l.pin.unit - r.pin.unit);

  const shown = (l: LogicalPin) => (l.pin.number === l.number ? l.number : `${l.number} (${l.pin.number})`);
  const quoted = (pin: LibrarySymbolPin) => (pin.name === "" ? "" : ` '${pin.name}'`);
  const at = (pin: LibrarySymbolPin) => `(${fmt(pin.at.x)}, ${fmt(pin.at.y)})`;

  for (let i = 1; i < logical.length; i++) {
    const prev = logical[i - 1]!;
    const next = logical[i]!;
    if (prev.number !== next.number || prev.pin === next.pin) continue;
    // Pins are not duplicated only if they are in different body styles (but body style 0 means common to all of them)
    if (prev.pin.body_style !== 0 && next.pin.body_style !== 0 && prev.pin.body_style !== next.pin.body_style) continue;

    const head: CheckPart[] = [b(`Duplicate pin ${shown(next)}`), t(` ${quoted(next.pin)} at location `), b(at(next.pin)), t(" conflicts with pin ")];
    const eitherUnitless = prev.pin.unit === 0 || next.pin.unit === 0;
    if (multiBody && next.pin.body_style !== 0) {
      // the first message of the C++ passes the previous pin's raw name where the others pass the quoted one
      const prevName = eitherUnitless ? prev.pin.name : quoted(prev.pin);
      out.push([
        ...head,
        t(`${shown(prev)}${prevName} at location `),
        b(at(prev.pin)),
        t(eitherUnitless ? ` in ${bodyStyleName(prev.pin.body_style)} body style.` : ` in units ${unitName(next.pin.unit)} and ${unitName(prev.pin.unit)} of ${bodyStyleName(prev.pin.body_style)} body style.`),
      ]);
    } else {
      out.push([...head, t(`${shown(prev)}${quoted(prev.pin)} at location `), b(at(prev.pin)), t(eitherUnitless ? "." : ` in units ${unitName(next.pin.unit)} and ${unitName(prev.pin.unit)}.`)]);
    }
  }
  return out;
}

/** `CheckLibSymbolGraphics`: a circle with no radius and a rectangle whose two corners are one point. */
export function checkSymbolGraphics(symbol: LibrarySymbol, fmt: LengthFormatter): CheckMessage[] {
  const out: CheckMessage[] = [];
  for (const g of symbol.graphics) {
    if (g.kind === "circle" && g.radius_mm <= 0) {
      out.push([b("Graphic circle has radius = 0"), t(" at location "), b(`(${fmt(g.center.x)}, ${fmt(g.center.y)})`), t(".")]);
    } else if (g.kind === "rectangle" && g.start.x === g.end.x && g.start.y === g.end.y) {
      out.push([b("Graphic rectangle has size 0"), t(" at location "), b(`(${fmt(g.start.x)}, ${fmt(g.start.y)})`), t(".")]);
    }
  }
  return out;
}

/**
 * `CheckLibSymbol`. `gridIu` is the editor's grid in KiCad's internal units (nanometres); a pin must sit on at least a 25 mil grid.
 * `fmt` writes a length the way the frame does (`MessageTextFromValue`). No message at all means the symbol is clean.
 */
export function checkLibSymbol(symbol: LibrarySymbol, gridIu: number, fmt: LengthFormatter): CheckMessage[] {
  const out: CheckMessage[] = [];

  // Reference prefix: "if the symbol is saved in a library, the prefix should not end by a digit or a '?'"
  const referenceBase = symbol.reference_prefix;
  if (referenceBase === "") {
    out.push([b("Warning: reference is empty")]);
  } else {
    const illegalEnd = "0123456789?";
    if (illegalEnd.includes(referenceBase[referenceBase.length - 1]!)) {
      out.push([b("Warning: reference prefix"), t(`\nprefix ending by '${illegalEnd}' can create issues if saved in a symbol library`)]);
    }
  }

  out.push(...checkDuplicatePins(symbol, fmt));

  const pinList = [...symbol.pins].sort(comparePins);
  const clamped = gridIu < MIN_PIN_GRID_IU ? MIN_PIN_GRID_IU : gridIu;
  const multiBody = symbol.has_alternate_body_style;
  const where = (pin: LibrarySymbolPin) => `(${fmt(pin.at.x)}, ${fmt(pin.at.y)})`;

  // A valid power symbol has one unit, no alternate body style and one pin, which is a power input (hidden pins are no longer needed) or a power output (a flag).
  if (symbol.power) {
    if (symbol.unit_count !== 1) out.push([b("A Power Symbol should have only one unit")]);
    if (pinList.length !== 1) out.push([b("A Power Symbol should have only one pin")]);
    const pin = pinList[0];
    if (pin) {
      if (pin.electrical_type !== "power_in" && pin.electrical_type !== "power_out") {
        out.push([b("Suspicious Power Symbol"), t("\nOnly an input or output power pin has meaning")]);
      }
      if (pin.electrical_type === "power_in" && pin.hidden) {
        out.push([b("Suspicious Power Symbol"), t("\nInvisible input power pins are no longer required")]);
      }
    }
  }

  for (const pin of pinList) {
    const pinName = pin.name === "" || pin.name === "~" ? "" : `'${pin.name}'`;
    const unitLetterOf = String.fromCharCode(64 + pin.unit); // 'A' + unit - 1
    // Where the pin is, as the C++ words it: by body style for a symbol with an alternate one ("in" for the hidden-pin note, "of" for the grid
    // one -- the two messages differ there), by unit for a multi-unit symbol, nothing for a plain one.
    const suffix = (bodyWord: "in" | "of"): string => {
      if (multiBody && pin.body_style !== 0) {
        return symbol.unit_count <= 1 ? ` ${bodyWord} ${bodyStyleName(pin.body_style)} body style.` : ` in unit ${unitLetterOf} of ${bodyStyleName(pin.body_style)} body style.`;
      }
      return symbol.unit_count <= 1 ? "." : ` in unit ${unitLetterOf}.`;
    };

    // hidden power pin
    if (!symbol.power && pin.electrical_type === "power_in" && pin.hidden) {
      out.push([
        t("Info: "),
        b(`Hidden power pin ${pin.number}`),
        t(` ${pinName} at location `),
        b(where(pin)),
        t(suffix("in")),
        t("\n(Hidden power pins will drive their pin names on to any connected nets.)"),
      ]);
    }

    // off grid
    const xIu = Math.round(pin.at.x * IU_PER_MM);
    const yIu = Math.round(pin.at.y * IU_PER_MM);
    if (xIu % clamped !== 0 || yIu % clamped !== 0) {
      out.push([b(`Off grid pin ${pin.number}`), t(` ${pinName} at location `), b(where(pin)), t(suffix("of"))]);
    }
  }

  out.push(...checkSymbolGraphics(symbol, fmt));
  return out;
}

/** A message as plain text, for tests and for copying out of the dialog. */
export function messageText(m: CheckMessage): string {
  return m.map((p) => p.text).join("");
}
