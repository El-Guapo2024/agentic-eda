// Where the name and the number of a pin are written: `PIN_LAYOUT_CACHE::GetPinNameInfo` and `GetPinNumberInfo` of eeschema
// (eeschema/pin_layout_cache.cpp at 8303b2ad), read directly. The Rust side is `eda_model::kicad_geom::{pin_name_rect, pin_number_rect}`,
// which measures the same texts; the two agree on every case the tests list.
//
// A symbol shows its pin names inside the body when its `(pin_names (offset x))` is above zero -- from the pin's inner end, `x` on -- and
// over the pin line when it is zero. A pin's number is written over the line, or under it when the name is over it (to the right of a
// vertical pin rather than the left). `(pin_names (hide yes))` and `(pin_numbers (hide yes))` hide every name or number of the symbol.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).

import type { HAlign, VAlign } from "../../kicad-port/schText";

/** How one symbol's pins show their texts (`LIB_SYMBOL::m_showPinNames`, `m_showPinNumbers`, `m_pinNameOffset`). */
export interface PinTexts {
  namesHidden: boolean;
  numbersHidden: boolean;
  /** Micrometres; names are written inside the body when it is above zero. */
  nameOffsetUm: number;
}

/** What a symbol that does not say gets: both shown, names inside at 20 mils (`DEFAULT_PIN_NAME_OFFSET`). */
export const DEFAULT_PIN_TEXTS: PinTexts = { namesHidden: false, numbersHidden: false, nameOffsetUm: 508 };

/** The size every pin name and number is set in: 50 mils. */
export const PIN_TEXT_SIZE_UM = 1270;
/** `getPinTextOffset() + PIN_TEXT_MARGIN`: the gap between a pin line and its text, 8 mils. */
export const PIN_TEXT_CLEARANCE_UM = 203.2;
/** The thickness of a pin's text, 6 mils. */
export const PIN_TEXT_PEN_UM = 152.4;

/** One text of a pin, where KiCad writes it: the anchor on the sheet (micrometres), whether it runs up (a quarter turn counter-clockwise), and its justification. */
export interface PinTextPlacement {
  text: string;
  at: [number, number];
  vertical: boolean;
  h: HAlign;
  v: VAlign;
}

export interface PinTextPlacements {
  name: PinTextPlacement | null;
  number: PinTextPlacement | null;
}

/** `~` stands for no name. */
export function shownPinName(name: string | null | undefined): string {
  return !name || name === "~" ? "" : name;
}

/**
 * The name and the number of a pin. `tip` is its connection point and `root` the end that meets the body (micrometres on the sheet, y
 * down); the way the pin runs, from the tip toward the body, is read off the two (a pin of no length runs right).
 */
export function pinTextPlacements(pin: { name: string | null; number: string }, tip: [number, number], root: [number, number], texts: PinTexts = DEFAULT_PIN_TEXTS, sizeUm: number = PIN_TEXT_SIZE_UM): PinTextPlacements {
  const length = Math.hypot(root[0] - tip[0], root[1] - tip[1]);
  const dir: [number, number] = length === 0 ? [1, 0] : [(root[0] - tip[0]) / length, (root[1] - tip[1]) / length];
  const horizontal = Math.abs(dir[0]) >= Math.abs(dir[1]);
  const mid: [number, number] = [tip[0] + (dir[0] * length) / 2, tip[1] + (dir[1] * length) / 2];
  const name = shownPinName(pin.name);
  const showName = name !== "" && !texts.namesHidden;
  const showNumber = pin.number !== "" && !texts.numbersHidden;
  const nameInside = texts.nameOffsetUm > 0;
  // the number goes under the line (right of a vertical pin) when the name is over it
  const both = showName && !nameInside;
  const off = PIN_TEXT_CLEARANCE_UM + sizeUm / 2 + PIN_TEXT_PEN_UM;

  let nameAt: PinTextPlacement | null = null;
  if (showName) {
    if (nameInside) {
      const anchor: [number, number] = [root[0] + dir[0] * texts.nameOffsetUm, root[1] + dir[1] * texts.nameOffsetUm];
      // `transformTextForPin`: a pin running right writes left-justified, running left right-justified; a vertical one turns the text a quarter
      // counter-clockwise, left-justified for a pin running up, right-justified for one running down
      nameAt = horizontal ? { text: name, at: anchor, vertical: false, h: dir[0] > 0 ? "left" : "right", v: "center" } : { text: name, at: anchor, vertical: true, h: dir[1] < 0 ? "left" : "right", v: "center" };
    } else if (horizontal) {
      nameAt = { text: name, at: [mid[0], tip[1] - off], vertical: false, h: "center", v: "center" };
    } else {
      nameAt = { text: name, at: [tip[0] - off, mid[1]], vertical: true, h: "center", v: "center" };
    }
  }
  let numberAt: PinTextPlacement | null = null;
  if (showNumber) {
    numberAt = horizontal ? { text: pin.number, at: [mid[0], both ? tip[1] + off : tip[1] - off], vertical: false, h: "center", v: "center" } : { text: pin.number, at: [both ? tip[0] + off : tip[0] - off, mid[1]], vertical: true, h: "center", v: "center" };
  }
  return { name: nameAt, number: numberAt };
}
