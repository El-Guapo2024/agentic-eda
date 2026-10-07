// Sheet pins: the rules behind Place Pins from Sheet, Autoplace All Sheet Pins, Cleanup Sheet Pins and the Sync dialog
// (`SCH_SHEET_PIN`, `SCH_SHEET::HasUndefinedPins / CleanupSheet`, `SCH_DRAWING_TOOLS::importHierLabel(s)` / `AutoPlaceAllSheetPins`;
// eeschema/sch_sheet_pin.cpp, sch_sheet.cpp, tools/sch_drawing_tools.cpp at 8303b2ad).
//
// A sheet pin stands for a hierarchical label of the same name inside the sheet's own file; the two are tied by name alone.
import type { LabelShape } from "../api/types";

export type P = readonly [number, number];
export type Side = "top" | "right" | "bottom" | "left";

export interface SheetRect {
  at: P;
  size: P;
}

/** A hierarchical label of the sheet's file -- what a pin of the same name would stand for. */
export interface HierLabel {
  name: string;
  shape: LabelShape | null;
}

export interface PinLike {
  id?: string;
  name: string;
  at?: P;
}

const distToSegment = (p: P, a: P, b: P): number => {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const len2 = dx * dx + dy * dy;
  const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2));
  return Math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy));
};

/**
 * `SCH_SHEET_PIN::ConstrainOnEdge( aPos, true )`: the pin goes on the sheet edge nearest `pos` (`SHAPE_LINE_CHAIN::NearestSegment` over top,
 * right, bottom, left -- the first of equally near edges wins), at the point of that edge level with `pos`, kept between the corners.
 */
export function constrainOnEdge(sheet: SheetRect, pos: P): { at: [number, number]; side: Side } {
  const left = sheet.at[0];
  const top = sheet.at[1];
  const right = left + sheet.size[0];
  const bottom = top + sheet.size[1];
  const edges: Array<[Side, P, P]> = [
    ["top", [left, top], [right, top]],
    ["right", [right, top], [right, bottom]],
    ["bottom", [right, bottom], [left, bottom]],
    ["left", [left, bottom], [left, top]],
  ];
  let best = edges[0]!;
  let bestD = Infinity;
  for (const e of edges) {
    const d = distToSegment(pos, e[1], e[2]);
    if (d < bestD) {
      bestD = d;
      best = e;
    }
  }
  // Whole micrometres (KiCad's coordinates are integers): a position that came from text widths is rounded here.
  const clampX = Math.round(Math.min(Math.max(pos[0], left), right));
  const clampY = Math.round(Math.min(Math.max(pos[1], top), bottom));
  switch (best[0]) {
    case "top":
      return { at: [clampX, top], side: "top" };
    case "bottom":
      return { at: [clampX, bottom], side: "bottom" };
    case "left":
      return { at: [left, clampY], side: "left" };
    case "right":
      return { at: [right, clampY], side: "right" };
  }
}

/** `StrNumCmp( a, b, aIgnoreCase )`: natural string order -- digit runs compare as numbers, other characters one by one (upper-cased when ignoring case). */
export function strNumCmp(a: string, b: string, ignoreCase: boolean): number {
  const isDigit = (c: string | undefined) => c !== undefined && c >= "0" && c <= "9";
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    let c1: string = a[i]!;
    let c2: string = b[j]!;
    if (isDigit(c1) && isDigit(c2)) {
      let n1 = 0;
      let n2 = 0;
      do {
        n1 = n1 * 10 + (a.charCodeAt(i) - 48);
        i++;
      } while (i < a.length && isDigit(a[i]));
      do {
        n2 = n2 * 10 + (b.charCodeAt(j) - 48);
        j++;
      } while (j < b.length && isDigit(b[j]));
      if (n1 < n2) return -1;
      if (n1 > n2) return 1;
      c1 = i < a.length ? a[i]! : "\0";
      c2 = j < b.length ? b[j]! : "\0";
    }
    if (ignoreCase) {
      if (c1 !== c2) {
        const u1 = c1.toUpperCase();
        const u2 = c2.toUpperCase();
        if (u1 !== u2) return u1 < u2 ? -1 : 1;
      }
    } else if (c1 < c2) return -1;
    else if (c1 > c2) return 1;
    if (i < a.length) i++;
    if (j < b.length) j++;
  }
  if (i >= a.length && j < b.length) return -1;
  if (i < a.length && j >= b.length) return 1;
  return 0;
}

/** `SCH_SHEET::HasUndefinedPins`: is some pin without a hierarchical label of exactly its name in the sheet's file? */
export function hasUndefinedPins(pins: readonly PinLike[], labels: readonly HierLabel[]): boolean {
  return pins.some((p) => !labels.some((l) => l.name === p.name));
}

/** `SCH_SHEET::CleanupSheet`: the pins it removes -- those with no label of their name (compared without regard to case, as `CleanupSheet` does). */
export function pinsToCleanUp<T extends PinLike>(pins: readonly T[], labels: readonly HierLabel[]): T[] {
  return pins.filter((p) => !labels.some((l) => l.name.toLowerCase() === p.name.toLowerCase()));
}

/** `importHierLabels`: the file's hierarchical labels the sheet has no pin of that name for, in the file's own order. */
export function unplacedLabels(pins: readonly PinLike[], labels: readonly HierLabel[]): HierLabel[] {
  return labels.filter((l) => !pins.some((p) => p.name === l.name));
}

/** `importHierLabel`: the next label to place -- the first, in natural order, that has no pin yet (null: "No new hierarchical labels found."). */
export function nextLabelToPlace(pins: readonly PinLike[], labels: readonly HierLabel[]): HierLabel | null {
  const sorted = [...labels].sort((a, b) => strNumCmp(a.name, b.name, true));
  return sorted.find((l) => !pins.some((p) => p.name === l.name)) ?? null;
}

/** One line of the Sync Sheet Pins dialog: how a pin and the file's hierarchical labels of its name stand to each other. */
export type SyncRow<Pin extends PinLike & { shape?: LabelShape | null }> =
  | { kind: "ok"; name: string; pin: Pin; label: HierLabel }
  /** The names match but the pin's shape is not the label's. */
  | { kind: "shape"; name: string; pin: Pin; label: HierLabel }
  | { kind: "pin_only"; name: string; pin: Pin }
  | { kind: "label_only"; name: string; label: HierLabel };

/** The rows of the sync dialog for one sheet: its pins in order (matched, reshaped or unreferenced), then the labels that have no pin. */
export function syncRows<Pin extends PinLike & { shape?: LabelShape | null }>(pins: readonly Pin[], labels: readonly HierLabel[]): Array<SyncRow<Pin>> {
  const rows: Array<SyncRow<Pin>> = [];
  for (const pin of pins) {
    const label = labels.find((l) => l.name === pin.name);
    if (!label) rows.push({ kind: "pin_only", name: pin.name, pin });
    else rows.push({ kind: (pin.shape ?? "passive") === (label.shape ?? "passive") ? "ok" : "shape", name: pin.name, pin, label });
  }
  const seen = new Set<string>();
  for (const label of unplacedLabels(pins, labels)) {
    if (seen.has(label.name)) continue; // two labels of one name stand for one pin
    seen.add(label.name);
    rows.push({ kind: "label_only", name: label.name, label });
  }
  return rows;
}

/** A pin's footprint on the sheet: the label's text plus the flag at the border (`text height` either side), as an estimate of `GetBoundingBox`. */
export interface PinExtent {
  width: number;
  height: number;
}

/**
 * `AutoPlaceAllSheetPins`: where each unplaced label's pin goes. The first starts at the sheet's top-left corner (or after the existing pin that
 * sorts last), each next one to the right of the previous, wrapping to the next row when the width runs out; `constrainOnEdge` then puts every one
 * on the border nearest that spot. `extentOf( name )` estimates the bounding box of a pin (text plus flag).
 */
export function autoplacePins(sheet: SheetRect, existing: readonly PinLike[], labels: readonly HierLabel[], extentOf: (name: string) => PinExtent): Array<{ label: HierLabel; at: [number, number] }> {
  const out: Array<{ label: HierLabel; at: [number, number] }> = [];
  const bboxX = sheet.at[0];
  const bboxY = sheet.at[1];
  const bboxW = sheet.size[0];
  const bboxH = sheet.size[1];
  let last: { at: P; extent: PinExtent } | null = null;
  const pending = unplacedLabels(existing, labels);
  for (const label of pending) {
    if (!last) {
      const placed = existing.filter((p): p is PinLike & { at: P } => p.at !== undefined);
      if (placed.length > 0) {
        placed.sort((a, b) => a.at[0] - b.at[0] || a.at[1] - b.at[1]);
        const lastPin = placed[placed.length - 1]!;
        last = { at: lastPin.at, extent: extentOf(lastPin.name) };
      }
    }
    const current = extentOf(label.name);
    let cursor: P = [bboxX, bboxY];
    if (last) {
      const [lx, ly] = last.at;
      if (lx + last.extent.width + current.width <= bboxX + bboxW) cursor = [lx + last.extent.width, ly];
      else if (ly + last.extent.height + current.height <= bboxY + bboxH) cursor = [bboxX, ly + last.extent.height];
    }
    const { at } = constrainOnEdge(sheet, cursor);
    out.push({ label, at });
    last = { at, extent: current };
  }
  return out;
}
