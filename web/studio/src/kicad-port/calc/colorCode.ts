// The Color Code memo (pcb_calculator/calculator_panels/panel_color_code.cpp at KiCad 25ec2335) and the E-Series memo's cell colours
// (panel_eseries_display.{h,cpp}). KiCad's color code panel shows four bitmaps (resources/bitmaps_png/sources/light/color_code_value_and_name.svg,
// color_code_value.svg, color_code_multiplier.svg, color_code_tolerance.svg): one column per band, a row per colour. They are drawn here from the
// same colours and texts, read off those SVGs.
//
// Pure: no React, no DOM.

/** One row of the charts: the band colour's name, its fill (from the SVGs) and whether the text on it is white. */
export interface ColorBand {
  name: string;
  fill: string;
  whiteText: boolean;
  /** The digit of a value band (1st .. 4th), or null for Gold and Silver (empty in the value columns). */
  digit: number | null;
  /** The multiplier column's text ("x 1", "x 10", .. "x 0.01"). */
  multiplier: string;
  /** The tolerance column's text, "" where the bitmap leaves the row empty. */
  tolerance: string;
}

/** The twelve rows, top to bottom, as the bitmaps have them. */
export const COLOR_BANDS: readonly ColorBand[] = [
  { name: "Black", fill: "#0b0000", whiteText: true, digit: 0, multiplier: "x 1", tolerance: "" },
  { name: "Brown", fill: "#643234", whiteText: true, digit: 1, multiplier: "x 10", tolerance: "± 1%" },
  { name: "Red", fill: "#ff0000", whiteText: false, digit: 2, multiplier: "x 100", tolerance: "± 2%" },
  { name: "Orange", fill: "#fc6604", whiteText: false, digit: 3, multiplier: "x 1k", tolerance: "" },
  { name: "Yellow", fill: "#fcfe04", whiteText: false, digit: 4, multiplier: "x 10k", tolerance: "" },
  { name: "Green", fill: "#34ce34", whiteText: false, digit: 5, multiplier: "x 100k", tolerance: "± 0.5%" },
  { name: "Blue", fill: "#6466fc", whiteText: false, digit: 6, multiplier: "x 1M", tolerance: "± 0.25%" },
  { name: "Violet", fill: "#cc66fc", whiteText: false, digit: 7, multiplier: "x 10M", tolerance: "± 0.10%" },
  { name: "Gray", fill: "#949294", whiteText: false, digit: 8, multiplier: "x 100M", tolerance: "± 0.05%" },
  { name: "White", fill: "#ffffff", whiteText: false, digit: 9, multiplier: "x 1G", tolerance: "" },
  { name: "Gold", fill: "#cc9a34", whiteText: false, digit: null, multiplier: "x 0.1", tolerance: "± 5%" },
  { name: "Silver", fill: "#cccecc", whiteText: false, digit: null, multiplier: "x 0.01", tolerance: "± 10%" },
];

/** `m_rbToleranceSelection`'s choices. */
export const COLOR_CODE_TOLERANCES = ["10% / 5%", "<= 2%"] as const;

/** `ToleranceSelection`: the 4th value band is shown for tolerances of 2 % and below (selection 1), else the value has three bands. */
export function showFourthBand(selection: number): boolean {
  return selection !== 0;
}

// ------------------------------------------------------------------------------------------------ the E-Series memo's colours

/** `wxColourBase::AlphaBlend`. */
function alphaBlend(fg: number, bg: number, alpha: number): number {
  const r = bg + alpha * (fg - bg);
  return Math.trunc(Math.min(255, Math.max(0, r)));
}

/** `wxColour::ChangeLightness( ialpha )`: 0 black .. 100 unchanged .. 200 white. */
export function changeLightness(rgb: readonly [number, number, number], ialpha: number): [number, number, number] {
  if (ialpha === 100) return [rgb[0], rgb[1], rgb[2]];
  ialpha = Math.min(200, Math.max(0, ialpha));
  let alpha = (ialpha - 100.0) / 100.0;
  let bg: number;
  if (ialpha > 100) {
    bg = 255;
    alpha = 1.0 - alpha;
  } else {
    bg = 0;
    alpha = 1.0 + alpha;
  }
  return [alphaBlend(rgb[0], bg, alpha), alphaBlend(rgb[1], bg, alpha), alphaBlend(rgb[2], bg, alpha)];
}

/** `wxColour( unsigned long )` reads 0xBBGGRR. */
function fromBGR(v: number): [number, number, number] {
  return [v & 0xff, (v >> 8) & 0xff, (v >> 16) & 0xff];
}

/** `s_cE1BGR` .. `s_cE96BGR`. */
const SERIES_BGR: Record<string, number> = { E1: 0xf0fff0, E3: 0x98fb98, E6: 0xed9564, E12: 0xdda0dd, E24: 0xebce87, E48: 0x23e86b, E96: 0x7aa0ff };
/** `s_altAdjustValue` / `s_darkAdjustValue`. */
const ALT_ADJUST = 125;
const DARK_ADJUST = 78;

/** `recalculateColumnColours`: a series column's two alternating colours ([0] first cell, [1] next), darkened in a dark theme. E1 has one. */
export function eseriesColumnColours(series: "E1" | "E3" | "E6" | "E12" | "E24" | "E48" | "E96", dark: boolean): [string, string] {
  const base = changeLightness(fromBGR(SERIES_BGR[series]!), dark ? DARK_ADJUST : 100);
  const alt = changeLightness(base, ALT_ADJUST);
  const css = (c: number[]) => `rgb(${c[0]}, ${c[1]}, ${c[2]})`;
  return [css(base), css(alt)];
}

/** One cell of the E-Series grids: its text, how many rows it spans, and which of its column's two colours it takes. */
export interface ESeriesCell {
  text: string;
  rowSpan: number;
  alternate: boolean;
}

/**
 * `populateE112Tree` + `recolourE112Tree`: the E1, E3, E6 and E12 columns of 12 rows; each column a list of the cells that start a span (E1 one cell
 * of 12 rows, E3 four rows each, E6 two, E12 one), colours alternating down the column.
 */
export function eseriesGrid112(e12: readonly number[]): ESeriesCell[][] {
  const col = (step: number) => e12.filter((_, i) => i % step === 0).map((v, k) => ({ text: String(v), rowSpan: step, alternate: k % 2 === 1 }));
  return [[{ text: String(e12[0]), rowSpan: 12, alternate: false }], col(4), col(2), col(1)];
}

/**
 * `populateE2496Tree` + `recolourE2496Tree`: four stripes of 24 rows; in each stripe an E24 column (cells of 4 rows), an E48 column (2 rows) and an
 * E96 column (1 row), then a "-" gap column (one cell over the stripe's height) between stripes. Returns the 15 columns' cells, left to right.
 * The alternate colour runs down the whole column across the stripes' cells as KiCad's loop walks the rows of column 0 (it colours each row of
 * every stripe with the same flag).
 */
export function eseriesGrid2496(e24: readonly number[], e96: readonly number[]): (ESeriesCell[] | "gap")[] {
  const stripes = 4;
  const stripeHeight = 96 / stripes;
  const cols: (ESeriesCell[] | "gap")[] = [];
  for (let s = 0; s < stripes; s++) {
    const rowsE24 = e24.slice(s * 6, s * 6 + 6);
    const rowsE96 = e96.slice(s * stripeHeight, (s + 1) * stripeHeight);
    cols.push(rowsE24.map((v, k) => ({ text: String(v), rowSpan: 4, alternate: k % 2 === 1 })));
    cols.push(rowsE96.filter((_, k) => k % 2 === 0).map((v, k) => ({ text: String(v), rowSpan: 2, alternate: k % 2 === 1 })));
    cols.push(rowsE96.map((v, k) => ({ text: String(v), rowSpan: 1, alternate: k % 2 === 1 })));
    if (s < stripes - 1) cols.push("gap");
  }
  return cols;
}
