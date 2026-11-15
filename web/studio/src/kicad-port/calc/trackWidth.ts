// The Track Width panel's arithmetic (pcb_calculator/calculator_panels/panel_track_width.cpp over common/track_width_calculations.cpp, KiCad
// 25ec2335): the Brooks & Adam fit dT = K * I^a * W^b * Th^c to the IPC-2152 data, external tracks with one set of coefficients, internal ones with
// the set of the nearest copper weight. One of current, external width and internal width is the controlling value (`TW_MASTER_*`); the other two
// follow from it, then the areas, resistances, voltage drops and power losses of both tracks (`TWDisplayValues`).
//
// Pure: no React, no DOM.
import { UNIT_MIL } from "./unitSelectors";

interface BaCoefficients {
  K: number;
  a: number;
  b: number;
  c: number;
}

const EXTERNAL_COEFFS: BaCoefficients = { K: 215.3, a: 2.0, b: -1.15, c: -1.0 };
const INTERNAL_HALF_OZ_COEFFS: BaCoefficients = { K: 120.0, a: 2.0, b: -1.1, c: -1.52 };
const INTERNAL_1OZ_COEFFS: BaCoefficients = { K: 200.0, a: 1.9, b: -1.1, c: -1.52 };
const INTERNAL_2OZ_COEFFS: BaCoefficients = { K: 300.0, a: 2.0, b: -1.15, c: -1.52 };
const INTERNAL_3OZ_COEFFS: BaCoefficients = { K: 262.5, a: 1.9, b: -1.15, c: -1.52 };

// Nominal copper weight thicknesses in mils (1 oz/ft^2 is about 1.378 mil).
const HALF_OZ_MILS = 0.689;
const ONE_OZ_MILS = 1.378;
const TWO_OZ_MILS = 2.756;
const THREE_OZ_MILS = 4.134;

/** track_width_calculations.cpp `coefficients`. */
function coefficients(aUseInternalLayer: boolean, aThicknessMils: number): BaCoefficients {
  if (!aUseInternalLayer) return EXTERNAL_COEFFS;
  if (aThicknessMils < (HALF_OZ_MILS + ONE_OZ_MILS) / 2.0) return INTERNAL_HALF_OZ_COEFFS;
  if (aThicknessMils < (ONE_OZ_MILS + TWO_OZ_MILS) / 2.0) return INTERNAL_1OZ_COEFFS;
  if (aThicknessMils < (TWO_OZ_MILS + THREE_OZ_MILS) / 2.0) return INTERNAL_2OZ_COEFFS;
  return INTERNAL_3OZ_COEFFS;
}

/** `TRACK_WIDTH_CALCULATIONS::CurrentFromWidth`: amperes from mils. */
export function currentFromWidth(aWidthMils: number, aThicknessMils: number, aDeltaT_C: number, aUseInternalLayer: boolean): number {
  const k = coefficients(aUseInternalLayer, aThicknessMils);
  const denom = k.K * Math.pow(aWidthMils, k.b) * Math.pow(aThicknessMils, k.c);
  return Math.pow(aDeltaT_C / denom, 1.0 / k.a);
}

/** `TRACK_WIDTH_CALCULATIONS::WidthFromCurrent`: mils from amperes. */
export function widthFromCurrent(aCurrentA: number, aThicknessMils: number, aDeltaT_C: number, aUseInternalLayer: boolean): number {
  const k = coefficients(aUseInternalLayer, aThicknessMils);
  const denom = k.K * Math.pow(aCurrentA, k.a) * Math.pow(aThicknessMils, k.c);
  return Math.pow(aDeltaT_C / denom, 1.0 / k.b);
}

/** `PANEL_TRACK_WIDTH::TWCalculateWidth`: metres. */
export function twCalculateWidth(aCurrent: number, aThickness: number, aDeltaT_C: number, aUseInternalLayer: boolean): number {
  return widthFromCurrent(aCurrent, aThickness / UNIT_MIL, aDeltaT_C, aUseInternalLayer) * UNIT_MIL;
}

/** `PANEL_TRACK_WIDTH::TWCalculateCurrent`. */
export function twCalculateCurrent(aWidth: number, aThickness: number, aDeltaT_C: number, aUseInternalLayer: boolean): number {
  return currentFromWidth(aWidth / UNIT_MIL, aThickness / UNIT_MIL, aDeltaT_C, aUseInternalLayer);
}

/** `TW_MASTER_CURRENT` / `TW_MASTER_EXT_WIDTH` / `TW_MASTER_INT_WIDTH`. */
export type TwMode = "current" | "ext_width" | "int_width";

/** The values the three `OnTWCalculateFrom*` handlers read (already `std::abs`-ed and normalized to A, m, deg C). */
export interface TwInputs {
  current: number;
  extWidth: number;
  intWidth: number;
  extThickness: number;
  intThickness: number;
  deltaT_C: number;
  /** ohm*m. */
  rho: number;
  /** m. */
  trackLen: number;
}

export interface TwResults {
  current: number;
  extWidth: number;
  intWidth: number;
  extResistance: number;
  intResistance: number;
  extV: number;
  intV: number;
  extLoss: number;
  intLoss: number;
}

/** `OnTWCalculateFromCurrent` / `FromExtWidth` / `FromIntWidth` and the arithmetic of `TWDisplayValues` (areas excepted: they need the display units). */
export function calculateTrackWidth(mode: TwMode, p: TwInputs): TwResults {
  let current = p.current;
  let extWidth = p.extWidth;
  let intWidth = p.intWidth;
  if (mode === "current") {
    extWidth = twCalculateWidth(current, p.extThickness, p.deltaT_C, false);
    intWidth = twCalculateWidth(current, p.intThickness, p.deltaT_C, true);
  } else if (mode === "ext_width") {
    current = twCalculateCurrent(extWidth, p.extThickness, p.deltaT_C, false);
    intWidth = twCalculateWidth(current, p.intThickness, p.deltaT_C, true);
  } else {
    current = twCalculateCurrent(intWidth, p.intThickness, p.deltaT_C, true);
    extWidth = twCalculateWidth(current, p.extThickness, p.deltaT_C, false);
  }
  const extResistance = (p.rho * p.trackLen) / (extWidth * p.extThickness);
  const intResistance = (p.rho * p.trackLen) / (intWidth * p.intThickness);
  const extV = extResistance * current;
  const intV = intResistance * current;
  return { current, extWidth, intWidth, extResistance, intResistance, extV, intV, extLoss: extV * current, intLoss: intV * current };
}
