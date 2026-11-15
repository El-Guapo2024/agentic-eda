// The Galvanic Corrosion memo (pcb_calculator/calculator_panels/panel_galvanic_corrosion.cpp at KiCad 25ec2335): the table of electrochemical
// potential differences between metals and finishes, each cell coloured by sign and size against the threshold voltage typed (`fillTable`), with the
// rows and columns named by chemical symbol or by name. The UI is components/calculator/PanelGalvanicCorrosion.tsx.
//
// Pure: no React, no DOM.
import { formatFixed0, kiRound } from "./cformat";

/** `CORROSION_TABLE_ENTRY`: the potential in volts (KiCad's table: copper is 0, more negative is more noble). */
export interface CorrosionTableEntry {
  name: string;
  symbol: string;
  potential: number;
}

const e = (name: string, symbol: string, potential: number): CorrosionTableEntry => ({ name, symbol, potential });

/** `m_entries`, in the constructor's order. */
export const CORROSION_ENTRIES: readonly CorrosionTableEntry[] = [
  e("Rhodium", "Rh", -0.6),
  e("Platinum", "Pt", -0.57),
  e("Palladium", "Pd", -0.5),
  e("Gold", "Au", -0.44),
  e("Stainless steel 316L", "X2CrNiMo17-12-2", -0.35),
  e("Inconel", "Inconel", -0.35),
  e("Indium", "In", -0.34),
  e("Titanium, passive", "Ti", -0.32),
  e("Stainless steel 18-9", "X8CrNiS18-9", -0.32),
  e("Silver", "Ag", -0.22),
  e("Mercury", "Hg", -0.22),
  e("ENEPIG (Ni/Pd/Au)", "ENEPIG", -0.18),
  e("ENIG (Ni/Au)", "ENIG", -0.15),
  e("Nickel", "Ni", -0.14),
  e("Carbon (Graphitic)", "C", -0.1),
  e("Copper", "Cu", 0.0),
  e("Copper-Aluminium", "CuAl10", 0.03),
  e("Brass", "CuZn39Pb", 0.08),
  e("Beryllium copper", "CuBe2", 0.15),
  e("Lead-free Solder", "SAC305", 0.15),
  e("Phosphor bronze", "CuSnP", 0.15),
  e("Bronze", "CuSn12", 0.2),
  e("Tin-Nickel (Sn/Ni)", "SnNi", 0.21),
  e("Solder 63/37 (Eutectic)", "Sn63Pb37", 0.21),
  e("Solder 60/40 (Leaded)", "Sn60Pb40", 0.22),
  e("Tin", "Sn", 0.23),
  e("Lead", "Pb", 0.27),
  e("2xxx series Al alloy", "AlCu4Mg", 0.37),
  e("Cast iron", "Fe-C-Si", 0.38),
  e("Carbon steel", "Fe–C", 0.43),
  e("Aluminium, chromated", "Al-Chromate", 0.5),
  e("Aluminium, pure, passive", "Al", 0.52),
  e("Cadmium", "Cd", 0.53),
  e("Iron", "Fe", 0.535),
  e("Chrome, passive", "Cr", 0.63),
  e("Zinc", "Zn", 0.83),
  e("Steel, zinc-plated", "Fe-Zn", 0.83),
  e("Manganese", "Mn", 0.9),
  e("Magnesium", "Mg", 1.38),
];

export type Rgb = readonly [number, number, number];

/** `getContrastingTextColour`: ITU-R BT.709 luminance below 140 gets white text, else black. */
export function contrastingTextColour(bg: Rgb): Rgb {
  const luminance = 0.2126 * bg[0] + 0.7152 * bg[1] + 0.0722 * bg[2];
  return luminance < 140.0 ? [255, 255, 255] : [0, 0, 0];
}

export interface CorrosionCell {
  /** `%.0f` of the difference in mV. */
  text: string;
  bg: Rgb;
  fg: Rgb;
}

/** A row's (and column's) label: the symbol, or the name, whichever is asked for (the other when it is empty). */
export function corrosionLabel(entry: CorrosionTableEntry, symbols: boolean): string {
  return symbols ? (entry.symbol.length > 0 ? entry.symbol : entry.name) : entry.name.length > 0 ? entry.name : entry.symbol;
}

/**
 * `fillTable`'s cell for row A and column B: the difference `A - B` in mV; light blue for the same potential, within the threshold `color_ok`
 * (122, 166, 194), above it a cold hue when the row is anodic (positive) and a warm one when it is cathodic, darker as the difference grows.
 * `threshold` is the threshold voltage in mV (`DoubleFromString` of the box: NaN compares false, so every cell counts as within it).
 */
export function corrosionCell(a: CorrosionTableEntry, b: CorrosionTableEntry, threshold: number): CorrosionCell {
  const diff = a.potential - b.potential;
  const diffTemp = kiRound(Math.abs(diff * 99));
  let bg: Rgb;
  if (Math.abs(diff) === 0) bg = [193, 231, 255];
  else if (kiRound(Math.abs(diff * 1000)) > threshold) bg = diff > 0 ? [226 - diffTemp, 226 - diffTemp, 246 - diffTemp] : [255 - diffTemp, 222 - diffTemp, 199 - diffTemp];
  else bg = [122, 166, 194];
  return { text: formatFixed0(diff * 1000), bg, fg: contrastingTextColour(bg) };
}

/** The whole table, row by row. */
export function corrosionTable(threshold: number): CorrosionCell[][] {
  return CORROSION_ENTRIES.map((a) => CORROSION_ENTRIES.map((b) => corrosionCell(a, b, threshold)));
}

/** galvanic_corrosion_help.md, verbatim. */
export const GALVANIC_CORROSION_HELP =
  "This table shows the difference in electrochemical potential between various metals and alloys. Galvanic corrosion affects different metals in contact and under certain conditions.<br>\n" +
  "The anode of an electrochemical pair gets oxidized and eaten away, while the cathode gets dissolved metals plated onto it but stays protected.<br>\n" +
  "A positive number indicates that the row is anodic (-) and the column is cathodic (+), cold and warm coloring hues also indicate rows' potential.<br>\n" +
  "EN 50310 suggests a voltage difference below 300mV. Known practices make use of a third interface metal in between the main pair(ie the ENIG surface finish).<br>\n" +
  "Selected cells shown with the default system's coloring choice after a table refill.\n";
