// Pure helpers behind the "act on what KiCad acts on" fixes: wrap-around list
// stepping (grid / track width / via size), per-item "next larger / smaller
// preset" (TrackWidthInc/Dec, ViaSizeInc/Dec with a track/via-only selection),
// rotation about a shared pivot, and Select All (full selection filter + layer
// visibility, pcb_selection_tool.cpp SelectAll).
import type { BoardState } from "../api/types";
import { collectBoxSelection, type SelectionFilter } from "../components/canvas/selectionCandidates";

/**
 * common_tools.cpp GridNext/GridPrev and board_editor_control.cpp TrackWidth and ViaSize Inc/Dec:
 * `idx++; if (idx >= size) idx = 0;` / `idx--; if (idx < 0) idx = size - 1;`.
 * A current value that is not in the list (`cur < 0`) steps from "before the
 * first" going up and "after the last" going down, like the source's index.
 */
export function wrapStep(cur: number, len: number, dir: 1 | -1): number {
  if (len <= 0) return -1;
  if (cur < 0) return dir > 0 ? 0 : len - 1;
  const next = cur + dir;
  if (next >= len) return 0;
  if (next < 0) return len - 1;
  return next;
}

/** TrackWidthInc / ViaSizeInc per-item rule: the first preset (in list order) strictly larger than `cur`, else null (item untouched). */
export function nextLargerPreset<T>(list: readonly T[], size: (t: T) => number, cur: number): T | null {
  for (let i = 0; i < list.length; i++) if (size(list[i]!) > cur) return list[i]!;
  return null;
}

/** TrackWidthDec / ViaSizeDec per-item rule: the last preset (scanning from the end) strictly smaller than `cur`, else null. */
export function nextSmallerPreset<T>(list: readonly T[], size: (t: T) => number, cur: number): T | null {
  for (let i = list.length - 1; i >= 0; i--) if (size(list[i]!) < cur) return list[i]!;
  return null;
}

/** Rotate (x, y) about (cx, cy) by `quarterTurns` * 90 degrees, mathematical (counter-clockwise for +y up) direction, exact integer arithmetic. */
export function rotateQuarter(x: number, y: number, cx: number, cy: number, quarterTurns: number): { x: number; y: number } {
  const q = ((quarterTurns % 4) + 4) % 4;
  const dx = x - cx,
    dy = y - cy;
  switch (q) {
    case 1:
      return { x: cx - dy, y: cy + dx };
    case 2:
      return { x: cx - dx, y: cy - dy };
    case 3:
      return { x: cx + dy, y: cy - dx };
    default:
      return { x, y };
  }
}

/** Mirror x about the vertical line through cx (`flip_x`) -- used for a multi-item flip / mirror. */
export const mirrorCoord = (v: number, c: number): number => 2 * c - v;

/**
 * pcb_selection_tool.cpp SelectAll: every item that passes the FULL selection
 * filter (incl. locked items) and layer visibility -- the same predicate a
 * box select applies, over an unbounded box -- ADDED to the current selection.
 */
export function selectAllIds(board: BoardState, filter: SelectionFilter, layerVisible: Record<string, boolean>, activeLayer: string | null, highContrast: boolean, current: Iterable<string> = []): string[] {
  const hits = collectBoxSelection(board, [-Infinity, -Infinity, Infinity, Infinity], true, filter, layerVisible, activeLayer, highContrast);
  const out = new Set<string>(current);
  for (const h of hits) out.add(h.id);
  return [...out];
}
