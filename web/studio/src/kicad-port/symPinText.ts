// What the symbol editor's pin view options draw: the electrical-type text beside each pin (`eeschema.SymbolLibraryControl.showElectricalTypes`
// -> `m_ShowPinsElectricalType`), the pins marked hidden (`showHiddenPins` -> `m_ShowHiddenPins`) and the pin number / name labels. Ports of
// `PIN_LAYOUT_CACHE::GetPinElectricalTypeInfo` + `transformTextForPin` (eeschema/pin_layout_cache.cpp) and of `SCH_PAINTER::draw( SCH_PIN )`
// (eeschema/sch_painter.cpp). All lengths are µm in the canvas' internal space (+y down); a pin is described by its connection point (`tip`,
// KiCad's `GetPosition()`) and `dir`, the unit vector from the pin's body to that point -- the way the pin sticks out.
import type { PinElectricalType } from "../api/types";

/** `TARGET_PIN_RADIUS` (eeschema/sch_pin.h): 15 mil. */
export const TARGET_PIN_RADIUS_UM = 381;

/** `DEFAULT_PINNAME_SIZE` (eeschema/default_values.h): 50 mil -- the name size of a pin that does not set one. */
export const DEFAULT_PIN_NAME_SIZE_MM = 1.27;

/** `DEFAULT_TEXT_OFFSET_RATIO` (eeschema/default_values.h): the symbol editor's render settings keep the default. */
export const DEFAULT_TEXT_OFFSET_RATIO = 0.15;

/** `PIN_LAYOUT_CACHE::getPinTextOffset`: `MilsToIU( KiROUND( 24 * offsetRatio ) )`. */
export function pinTextOffsetUm(ratio = DEFAULT_TEXT_OFFSET_RATIO): number {
  return Math.round(24 * ratio) * 25.4;
}

/** `ElectricalPinTypeGetText` (eeschema/pin_type.cpp, `InitTables`). */
export const ELECTRICAL_TYPE_NAMES: Record<PinElectricalType, string> = {
  input: "Input",
  output: "Output",
  bidirectional: "Bidirectional",
  tri_state: "Tri-state",
  passive: "Passive",
  free: "Free",
  unspecified: "Unspecified",
  power_in: "Power input",
  power_out: "Power output",
  open_collector: "Open collector",
  open_emitter: "Open emitter",
  no_connect: "Unconnected",
};

export interface PinTextLayout {
  text: string;
  sizeUm: number;
  thicknessUm: number;
  /** Where the text is anchored (not yet centred on its line: the painter does that the way it does for every other label). */
  at: [number, number];
  /** `m_Angle == ANGLE_VERTICAL`: the text reads upward instead of left to right. */
  vertical: boolean;
  /** Which end of the text sits on `at`. */
  justify: "left" | "right";
}

/**
 * `GetPinElectricalTypeInfo`: the text is the electrical type's name, `max( nameSize * 3 / 4, 0.7 mm )` high with a pen of an eighth of that,
 * placed beyond the connection point by the text offset, half the pen, `TARGET_PIN_RADIUS` and (for a pin that is not connected -- every pin
 * of a library symbol, `IsDangling()` stays true) half a radius more; right-aligned in the frame of a pin whose body points right, which
 * `transformTextForPin` turns for the other three directions (mirrored alignment for left and down, vertical text for up and down).
 */
export function electricalTypeLayout(pin: { electrical_type: PinElectricalType; name_size_mm: number | null }, tip: [number, number], dir: [number, number], dangling = true): PinTextLayout {
  const sizeUm = Math.max(((pin.name_size_mm ?? DEFAULT_PIN_NAME_SIZE_MM) * 1000 * 3) / 4, 700);
  const thicknessUm = sizeUm / 8;
  let distance = pinTextOffsetUm() + thicknessUm / 2 + TARGET_PIN_RADIUS_UM;
  if (dangling) distance += TARGET_PIN_RADIUS_UM / 2;
  const at: [number, number] = [tip[0] + dir[0] * distance, tip[1] + dir[1] * distance];
  const vertical = dir[0] === 0;
  // The body points away from `dir`: a pin whose body points right (dir.x < 0) keeps the local right alignment, left flips it; up (dir.y > 0, text
  // below the connection point) keeps it, down flips it.
  const justify: "left" | "right" = vertical ? (dir[1] > 0 ? "right" : "left") : dir[0] < 0 ? "right" : "left";
  return { text: ELECTRICAL_TYPE_NAMES[pin.electrical_type], sizeUm, thicknessUm, at, vertical, justify };
}

/** `SCH_PAINTER::draw( SCH_PIN )`: a pin marked hidden is drawn (in the hidden colour) only while "Show Hidden Pins" is on; the same holds for picking it (`IsShowingHiddenPins`). */
export function pinShown(hidden: boolean, showHiddenPins: boolean): boolean {
  return !hidden || showHiddenPins;
}

/** `PIN_LAYOUT_CACHE::GetPinNameInfo` / `GetPinNumberInfo`: a label is drawn only when the symbol shows pin names / numbers (`pin_names_hidden`, `pin_numbers_hidden`) and the pin has one; "Show Pin Numbers" forces the numbers on. */
export function pinLabelsShown(symbol: { pin_names_hidden?: boolean; pin_numbers_hidden?: boolean }, forceNumbers: boolean): { names: boolean; numbers: boolean } {
  return { names: !symbol.pin_names_hidden, numbers: !symbol.pin_numbers_hidden || forceNumbers };
}
