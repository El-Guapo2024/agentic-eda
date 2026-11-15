// The Through-hole Size calculator: the hole size rules (common/pth_hole_size.cpp, include/pth_hole_size.h at KiCad 25ec2335: the IPC-2222B
// standard, `ComputeRecommendedPthHoleSize`) and what `PANEL_PTH_SIZE` (pcb_calculator/calculator_panels/panel_pth_size.cpp) does with them: read
// the lead's size range, find the allowable hole range for the density level, choose the recommended hole on the rounding grid, add the annular
// ring for the pad, and write the report. Sizes are KiCad's PCB internal units (nm) in ints, as in the C++.
//
// Pure: no React, no DOM. The panel is components/calculator/PanelPthSize.tsx.
import { printfG } from "./cformat";
import { doubleFromString } from "./unitSelectors";

export type PthLeadShape = "round" | "square" | "rectangular";
export type PthHoleRounding = "nearest" | "up";

/** `PTH_LEAD_DEF` (nm). */
export interface PthLeadDef {
  shape: PthLeadShape;
  minX: number;
  maxX: number;
  minY: number;
  maxY: number;
}

/** `PTH_HOLE_SIZE_RESULT` (nm). */
export interface PthHoleSizeResult {
  leadMin: number;
  leadMax: number;
  holeMin: number;
  holeMax: number;
}

/** `KiROUND`: half away from zero. */
export function kiRound(v: number): number {
  return v < 0 ? -Math.round(-v) : Math.round(v);
}

/** `pcbIUScale.mmToIU` (1e6 IU per mm) and `IUTomm`. */
export const mmToIU = (mm: number): number => kiRound(mm * 1e6);
export const iuToMm = (iu: number): number => iu / 1e6;

/** `PTH_HOLE_SIZE_STANDARD`. */
export interface PthHoleSizeStandard {
  id: string;
  displayName: string;
  levelCount: number;
  levelName(level: number): string;
  minHoleAddend(level: number): number;
  maxHoleAddend(level: number): number;
  computeHoleSize(level: number, lead: PthLeadDef): PthHoleSizeResult;
}

/** `effectiveLeadSize`: the diameter of a round lead, the diagonal of a square or rectangular one. */
function effectiveLeadSize(lead: PthLeadDef): [number, number] {
  switch (lead.shape) {
    case "round":
      return [lead.minX, lead.maxX];
    case "square":
      return [kiRound(Math.hypot(lead.minX, lead.minX)), kiRound(Math.hypot(lead.maxX, lead.maxX))];
    case "rectangular":
      return [kiRound(Math.hypot(lead.minX, lead.minY)), kiRound(Math.hypot(lead.maxX, lead.maxY))];
  }
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** `IPC2222_HOLE_SIZE_STANDARD`: levels A, B, C; smallest-hole allowances 0.25 / 0.20 / 0.15 mm, largest-hole 0.70 / 0.70 / 0.60 mm. */
const IPC2222B: PthHoleSizeStandard = (() => {
  const MIN_HOLE_ADDED = [mmToIU(0.25), mmToIU(0.2), mmToIU(0.15)];
  const MAX_HOLE_ADDED = [mmToIU(0.7), mmToIU(0.7), mmToIU(0.6)];
  const std: PthHoleSizeStandard = {
    id: "IPC-2222B",
    displayName: "IPC-2222B",
    levelCount: 3,
    levelName: (level) => ["A (General)", "B (Moderate)", "C (High density)"][clamp(level, 0, 2)]!,
    minHoleAddend: (level) => MIN_HOLE_ADDED[clamp(level, 0, 2)]!,
    maxHoleAddend: (level) => MAX_HOLE_ADDED[clamp(level, 0, 2)]!,
    computeHoleSize: (aLevel, lead) => {
      const level = clamp(aLevel, 0, 2);
      const [leadMin, leadMax] = effectiveLeadSize(lead);
      return { leadMin, leadMax, holeMin: leadMax + std.minHoleAddend(level), holeMax: leadMin + std.maxHoleAddend(level) };
    },
  };
  return std;
})();

/** `GetPthHoleSizeStandards`. */
export const PTH_HOLE_SIZE_STANDARDS: readonly PthHoleSizeStandard[] = [IPC2222B];

/** `FindPthHoleSizeStandard`. */
export function findPthHoleSizeStandard(id: string): PthHoleSizeStandard | null {
  return PTH_HOLE_SIZE_STANDARDS.find((s) => s.id === id) ?? null;
}

/** `ComputeRecommendedPthHoleSize`: the range's midpoint on the rounding grid (up or nearest), never below the smallest hole. */
export function computeRecommendedPthHoleSize(range: PthHoleSizeResult, step: number, rounding: PthHoleRounding): number {
  const mid = Math.trunc((range.holeMin + range.holeMax) / 2);
  if (step <= 0) return mid;
  let hole = rounding === "up" ? Math.trunc((mid + step - 1) / step) * step : Math.trunc((mid + Math.trunc(step / 2)) / step) * step;
  if (hole < range.holeMin) hole = Math.trunc((range.holeMin + step - 1) / step) * step;
  return hole;
}

/** `formatMm`: "%.3g" of the size in mm. */
export function formatMm(iu: number): string {
  return printfG(iuToMm(iu), 3);
}

/** `readMm`: the box's mm (DoubleFromString) in IU, or null when it is not a finite number. */
export function readMm(text: string): number | null {
  const mm = doubleFromString(text);
  return Number.isFinite(mm) ? mmToIU(mm) : null;
}

/** The panel's inputs: the boxes' texts and the choices. */
export interface PthInputs {
  shape: PthLeadShape;
  sizeXMin: string;
  sizeXMax: string;
  sizeYMin: string;
  sizeYMax: string;
  annular: string;
  rounding: string;
  roundingChoice: PthHoleRounding;
  standard: number;
  level: number;
}

/** The constructor's values: a typical round component lead. */
export const PTH_DEFAULTS: PthInputs = {
  shape: "round",
  sizeXMin: "0.6",
  sizeXMax: "0.7",
  sizeYMin: "0.25",
  sizeYMax: "0.3",
  annular: "0.4",
  rounding: "0.05",
  roundingChoice: "up",
  standard: 0,
  level: 1,
};

export interface PthOutcome {
  lead: PthLeadDef;
  range: PthHoleSizeResult;
  hole: number;
  pad: number;
  report: string;
}

/** The label of the size X row (`updateLeadShapeUI`). */
export function sizeXLabel(shape: PthLeadShape): string {
  return shape === "round" ? "Diameter:" : shape === "square" ? "Side length:" : "Size X:";
}

/** `pthResultReportText`: label / value rows, the labels padded to the widest. */
export function pthResultReportText(std: PthHoleSizeStandard, level: number, lead: PthLeadDef, annular: number, range: PthHoleSizeResult, hole: number, pad: number): string {
  const f = formatMm;
  const shapeText = lead.shape === "round" ? "Round" : lead.shape === "square" ? "Square" : "Rectangular";
  const sizeText = lead.shape === "rectangular" ? `${f(lead.minX)} – ${f(lead.maxX)} mm x ${f(lead.minY)} – ${f(lead.maxY)} mm` : `${f(lead.minX)} – ${f(lead.maxX)} mm`;
  const rows: string[][] = [
    ["Standard:", std.displayName],
    ["Production level:", std.levelName(level)],
    ["Lead shape:", shapeText],
    ["Lead size:", sizeText],
    ["Effective lead size:", `${f(range.leadMin)} – ${f(range.leadMax)} mm`],
    [""],
    ["Minimum hole size:", `${f(range.holeMin)} mm`],
    ["Maximum hole size:", `${f(range.holeMax)} mm`],
    ["Recommended hole size:", `${f(hole)} mm`],
    ["Hole clearance over max. lead:", `${f(hole - range.leadMax)} mm`],
    ["Annular ring:", `${f(annular)} mm`],
    ["Pad diameter:", `${f(pad)} mm`],
  ];
  const width = Math.max(0, ...rows.filter((r) => r.length > 1).map((r) => r[0]!.length));
  return rows.map((r) => (r.length > 1 ? `${r[0]!.padEnd(width)} ${r[1]}\n` : `${r[0]}\n`)).join("");
}

/** `PANEL_PTH_SIZE::recalculate`: null when an input is incomplete or invalid (the report is emptied, the result boxes keep their last value). */
export function recalculatePth(inp: PthInputs): PthOutcome | null {
  const std = PTH_HOLE_SIZE_STANDARDS[inp.standard];
  if (!std || inp.level < 0) return null;
  const minX = readMm(inp.sizeXMin);
  const maxX = readMm(inp.sizeXMax);
  let valid = minX !== null && maxX !== null && minX > 0 && minX <= maxX;
  let minY = 0;
  let maxY = 0;
  if (inp.shape === "rectangular") {
    const a = readMm(inp.sizeYMin);
    const b = readMm(inp.sizeYMax);
    minY = a ?? 0;
    maxY = b ?? 0;
    valid &&= a !== null && b !== null && a > 0 && a <= b;
  }
  const annular = readMm(inp.annular);
  const step = readMm(inp.rounding);
  valid &&= annular !== null && annular >= 0 && step !== null && step >= 0;
  if (!valid) return null;
  const lead: PthLeadDef = { shape: inp.shape, minX: minX!, maxX: maxX!, minY, maxY };
  const range = std.computeHoleSize(inp.level, lead);
  const hole = computeRecommendedPthHoleSize(range, step!, inp.roundingChoice);
  const pad = hole + 2 * annular!;
  return { lead, range, hole, pad, report: pthResultReportText(std, inp.level, lead, annular!, range, hole, pad) };
}

/** `updateSummaryTable`: per level, the two cells. */
export function pthSummaryCells(std: PthHoleSizeStandard): { level: string; min: string; max: string }[] {
  return Array.from({ length: std.levelCount }, (_, l) => ({
    level: std.levelName(l),
    min: `Maximum lead diameter + ${formatMm(std.minHoleAddend(l))}mm`,
    max: `Minimum lead diameter + ${formatMm(std.maxHoleAddend(l))}mm`,
  }));
}
