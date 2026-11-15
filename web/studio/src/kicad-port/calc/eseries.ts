// The E-series of preferred values (pcb_calculator/eseries.{h,cpp} at KiCad 25ec2335): `ESERIES_VALUES` builds a series' values in the 100..999 decade
// from KiCad's two lookup tables (E24 for E1..E24, E192 for E48..E192), `ESERIES_IN_DECADE` scales them to a decade. Used by the Resistor Calculator
// (kicad-port/calc/resistorSubstitution.ts) and the E-Series memo panel (components/calculator/PanelESeries.tsx).
//
// Pure: no React, no DOM.

/** The `ESERIES` enum (its order matters: the skip tables below depend on it). */
export const E1 = 0;
export const E3 = 1;
export const E6 = 2;
export const E12 = 3;
export const E24 = 4;
export const E48 = 5;
export const E96 = 6;
export const E192 = 7;

/** `ESERIES_VALUES::s_e24table`. */
export const E24_TABLE: readonly number[] = [100, 110, 120, 130, 150, 160, 180, 200, 220, 240, 270, 300, 330, 360, 390, 430, 470, 510, 560, 620, 680, 750, 820, 910];

/** `ESERIES_VALUES::s_e192table`. */
export const E192_TABLE: readonly number[] = [
  100, 101, 102, 104, 105, 106, 107, 109, 110, 111, 113, 114, 115, 117, 118, 120, 121, 123, 124, 126, 127, 129, 130, 132, 133, 135, 137, 138, 140, 142, 143, 145, 147, 149,
  150, 152, 154, 156, 158, 160, 162, 164, 165, 167, 169, 172, 174, 176, 178, 180, 182, 184, 187, 189, 191, 193, 196, 198, 200, 203, 205, 208, 210, 213, 215, 218, 221, 223,
  226, 229, 232, 234, 237, 240, 243, 246, 249, 252, 255, 258, 261, 264, 267, 271, 274, 277, 280, 284, 287, 291, 294, 298, 301, 305, 309, 312, 316, 320, 324, 328, 332, 336,
  340, 344, 348, 352, 357, 361, 365, 370, 374, 379, 383, 388, 392, 397, 402, 407, 412, 417, 422, 427, 432, 437, 442, 448, 453, 459, 464, 470, 475, 481, 487, 493, 499, 505,
  511, 517, 523, 530, 536, 542, 549, 556, 562, 569, 576, 583, 590, 597, 604, 612, 619, 626, 634, 642, 649, 657, 665, 673, 681, 690, 698, 706, 715, 723, 732, 741, 750, 759,
  768, 777, 787, 796, 806, 816, 825, 835, 845, 856, 866, 876, 887, 898, 909, 920, 931, 942, 953, 965, 976, 988,
];

/** `ESERIES_VALUES( aESeries )`: the series' values in the 100..999 decade, as integers. */
export function eseriesValues(series: number): number[] {
  let base = E24_TABLE;
  let skip = 24;
  if (series === E1 || series === E3 || series === E6 || series === E12 || series === E24) {
    const skipTableE124 = [24, 8, 4, 2, 1];
    skip = skipTableE124[series]!;
  } else if (series === E48 || series === E96 || series === E192) {
    base = E192_TABLE;
    skip = 1 << (E192 - series);
  }
  const out: number[] = [];
  for (let i = 0; i < base.length; i += skip) out.push(base[i]!);
  return out;
}

/** `ESERIES_IN_DECADE( aESeries, aDecadeExponent )`: the values normalised to 1..10 and multiplied by 10^exponent. */
export function eseriesInDecade(series: number, decadeExponent: number): number[] {
  const values = eseriesValues(series);
  const decadeBase = values[0]!;
  const multiplier = Math.pow(10, decadeExponent);
  return values.map((v) => (multiplier * v) / decadeBase);
}
