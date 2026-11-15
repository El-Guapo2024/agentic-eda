// The Calculator Tools' unit choosers (pcb_calculator/widgets/unit_selector.{h,cpp}) and the scales behind them (pcb_calculator/units_scales.h, the same
// numbers as common/transline_calculations/units_scales.h), KiCad 25ec2335. Each `UNIT_SELECTOR_*` is a wxChoice of unit names whose
// `GetUnitScale()` turns a value typed in the selected unit into the normalized unit (metre, hertz, ohm, radian, volt, watt, m/s, second).
// A panel stores the selection's index (`GetSelection()`), so the lists keep KiCad's order.
//
// Pure: no React, no DOM.

// Inside calculations, units are always meter, Hz, Ohm and Radian.
export const UNIT_KM = 1e3;
export const UNIT_M = 1;
export const UNIT_CM = 1e-2;
export const UNIT_MM = 1e-3;
export const UNIT_MICRON = 1e-6;
export const UNIT_INCH = 1e-2 * 2.54;
export const UNIT_MIL = 1e-5 * 2.54;
export const UNIT_FEET = 0.3048;
/** 1 oz/ft^2 is 34.30 microns nominal, 30.90 minimum (KiCad's comment; the value is 34.40). */
export const UNIT_OZSQFT = 34.4 * UNIT_MICRON;
export const UNIT_GHZ = 1e9;
export const UNIT_MHZ = 1e6;
export const UNIT_KHZ = 1e3;
export const UNIT_DEGREE = Math.PI / 180.0;
export const UNIT_RADIAN = 1.0;
export const UNIT_OHM = 1.0;
export const UNIT_KOHM = 1e3;
export const UNIT_OHM_PER_METER = 1.0;
export const UNIT_OHM_PER_KILOMETER = 1e-3;
export const UNIT_OHM_PER_FEET = 3.28084;
export const UNIT_OHM_PER_1000FEET = 3.28084e-3;
export const UNIT_KILOVOLT = 1e-3;
export const UNIT_VOLT = 1.0;
export const UNIT_MILLIVOLT = 1e3;
export const UNIT_MEGAWATT = 1e-6;
export const UNIT_KILOWATT = 1e-3;
export const UNIT_WATT = 1.0;
export const UNIT_MILLIWATT = 1e3;
export const UNIT_METER_PER_SECOND = 1.0;
export const UNIT_KILOMETER_PER_HOUR = 1 / 3.6;
export const UNIT_FEET_PER_SECOND = 0.3048;
export const UNIT_MILES_PER_HOUR = 1609.34;
export const UNIT_SECOND = 1.0;
export const UNIT_MSECOND = 1e-3;
export const UNIT_USECOND = 1e-6;
export const UNIT_NSECOND = 1e-9;
export const UNIT_PSECOND = 1e-12;

/** One `UNIT_SELECTOR_*`: its choices and their scales, in KiCad's order. */
export interface UnitSelectorDef {
  names: readonly string[];
  scales: readonly number[];
}

const def = (pairs: [string, number][]): UnitSelectorDef => ({ names: pairs.map((p) => p[0]), scales: pairs.map((p) => p[1]) });

/** `UNIT_SELECTOR_LEN`. */
export const UNIT_SELECTOR_LEN = def([["mm", UNIT_MM], ["um", UNIT_MICRON], ["cm", UNIT_CM], ["mil", UNIT_MIL], ["inch", UNIT_INCH]]);
/** `UNIT_SELECTOR_THICKNESS`: lengths and copper weight. */
export const UNIT_SELECTOR_THICKNESS = def([["mm", UNIT_MM], ["µm", UNIT_MICRON], ["cm", UNIT_CM], ["mil", UNIT_MIL], ["inch", UNIT_INCH], ["oz/ft²", UNIT_OZSQFT]]);
/** `UNIT_SELECTOR_FREQUENCY`. */
export const UNIT_SELECTOR_FREQUENCY = def([["GHz", UNIT_GHZ], ["MHz", UNIT_MHZ], ["kHz", UNIT_KHZ], ["Hz", 1.0]]);
/** `UNIT_SELECTOR_ANGLE`. */
export const UNIT_SELECTOR_ANGLE = def([["rad", UNIT_RADIAN], ["deg", UNIT_DEGREE]]);
/** `UNIT_SELECTOR_RESISTOR`. */
export const UNIT_SELECTOR_RESISTOR = def([["Ω", UNIT_OHM], ["kΩ", UNIT_KOHM]]);
/** `UNIT_SELECTOR_LINEAR_RESISTANCE`. */
export const UNIT_SELECTOR_LINEAR_RESISTANCE = def([["Ω/m", UNIT_OHM_PER_METER], ["Ω/km", UNIT_OHM_PER_KILOMETER], ["Ω/ft", UNIT_OHM_PER_FEET], ["Ω/1000ft", UNIT_OHM_PER_1000FEET]]);
/** `UNIT_SELECTOR_LEN_CABLE`. */
export const UNIT_SELECTOR_LEN_CABLE = def([["cm", UNIT_CM], ["m", UNIT_M], ["km", UNIT_KM], ["inch", UNIT_INCH], ["feet", UNIT_FEET]]);
/** `UNIT_SELECTOR_VOLTAGE`. */
export const UNIT_SELECTOR_VOLTAGE = def([["mV", UNIT_MILLIVOLT], ["V", UNIT_VOLT]]);
/** `UNIT_SELECTOR_POWER`. */
export const UNIT_SELECTOR_POWER = def([["mW", UNIT_MILLIWATT], ["W", UNIT_WATT]]);
/** `UNIT_SELECTOR_SPEED`. */
export const UNIT_SELECTOR_SPEED = def([["m/s", UNIT_METER_PER_SECOND], ["ft/s", UNIT_FEET_PER_SECOND], ["km/h", UNIT_KILOMETER_PER_HOUR], ["mi/h", UNIT_MILES_PER_HOUR]]);
/** `UNIT_SELECTOR_TIME`. */
export const UNIT_SELECTOR_TIME = def([["ns", UNIT_NSECOND], ["ps", UNIT_PSECOND]]);

/** `UNIT_SELECTOR::GetUnitScale()`: 1.0 for a selection out of the list (wxNOT_FOUND), as each switch's fall-through. */
export function unitScale(sel: UnitSelectorDef, index: number): number {
  return sel.scales[index] ?? 1.0;
}

/**
 * `wxString::ToDouble` as the panels use it on a text box: the whole text must be a number (leading blanks allowed, as strtod), else it fails and the
 * caller keeps its default (often 0). Both '.' and ',' are taken as the decimal separator, as the panels that call `Replace( ',', '.' )` do; returns
 * null on failure.
 */
export function toDouble(text: string): number | null {
  const t = text.replace(",", ".").replace(/^\s+/, "");
  if (t === "" || !/^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(t)) return null;
  const v = Number(t);
  return Number.isFinite(v) ? v : null;
}

/**
 * params_read_write.cpp `DoubleFromString`: the number at the start of the text, both '.' and ',' as the decimal point, units after it ignored
 * ("4.7 K" is 4.7); NaN when the text does not start with a number. The numeric part is the run of digits, separators, signs and 'e'/'E', which
 * `ToDouble` must read whole (so "1e" or "1-2" is NaN).
 */
export function doubleFromString(text: string): number {
  const buf = text.trim().replace(/,/g, ".");
  let brk = 0;
  while (brk < buf.length && /[0-9.+\-eE]/.test(buf[brk]!)) ++brk;
  if (brk === 0) return Number.NaN;
  return toDouble(buf.slice(0, brk)) ?? Number.NaN;
}
