// Port of pcbnew/tools/align_distribute_tool.cpp (the Align/Distribute
// submenu, no default hotkeys in source either) and libs/kimath/src/
// geometry/distribute.cpp's two gap-math helpers it calls.
//
// Scope: this app applies these to placed footprints only (same scope
// rotate/flip already have -- see state/store.tsx's rotateSelection/
// flipSelection), not vias/shapes/text, which source's generic
// BOARD_ITEM::Move would also cover. Also not ported: source's "prefer a
// locked item, else the item under the cursor" override when picking the
// alignment target (`ALIGN_DISTRIBUTE_TOOL::selectTarget`) -- this app has
// no locked-item concept, and reading the cursor's position at
// context-menu-click time adds a second input path for a one-line
// behavioral nuance; the *extreme* item in the selection (topmost/
// bottommost/leftmost/rightmost/the one with the smallest center) is
// always the fallback target in source too once neither override applies,
// so that's what this always uses. Source also special-cases a mirrored
// view (swaps Align Left/Right) -- this app's view never mirrors.
import type { Um } from "../api/types";

export type AlignEdge = "top" | "bottom" | "left" | "right" | "centerX" | "centerY";

/** `[minX, minY, maxX, maxY]`, board µm -- same shape as `Part.courtyard`/`CourtyardBox`. */
export type Box = readonly [Um, Um, Um, Um];

function edgeValue(box: Box, edge: AlignEdge): number {
  const [x0, y0, x1, y1] = box;
  switch (edge) {
    case "top":
      return y0;
    case "bottom":
      return y1;
    case "left":
      return x0;
    case "right":
      return x1;
    case "centerX":
      return (x0 + x1) / 2;
    case "centerY":
      return (y0 + y1) / 2;
  }
}

/** AlignTop/Left/CenterX/CenterY sort ascending and target the front (minimum); AlignBottom/Right sort descending and target the front (maximum) -- align_distribute_tool.cpp's own per-action comparators. */
function isMinExtreme(edge: AlignEdge): boolean {
  return edge === "top" || edge === "left" || edge === "centerX" || edge === "centerY";
}

/** Which axis an edge's alignment delta applies to. */
export function alignAxis(edge: AlignEdge): "x" | "y" {
  return edge === "left" || edge === "right" || edge === "centerX" ? "x" : "y";
}

/**
 * One delta per box, along `alignAxis(edge)`, moving every box's relevant
 * edge/center to match the extreme one in the set -- `AlignTop`/
 * `AlignBottom`/`doAlignLeft`/`doAlignRight`/`AlignCenterX`/`AlignCenterY`'s
 * shared "sort, pick selectTarget's fallback, diff against it" shape.
 * Fewer than 2 boxes: every delta is 0 (source requires `MoreThan(1)` to
 * even show the menu item).
 */
export function alignDeltas(boxes: readonly Box[], edge: AlignEdge): number[] {
  if (boxes.length < 2) return boxes.map(() => 0);
  const values = boxes.map((b) => edgeValue(b, edge));
  const target = isMinExtreme(edge) ? Math.min(...values) : Math.max(...values);
  return values.map((v) => target - v);
}

/**
 * `GetDeltasForDistributeByGaps` (libs/kimath/src/geometry/distribute.cpp)
 * -- `aItemExtents` must already be sorted by `start` (callers: by left
 * edge for a horizontal distribute, by top edge for vertical). The first
 * and last items ("end caps") never move; every item between them is
 * spaced so the *gap* between neighbours is equal. Needs 3+ items (same
 * floor source enforces before distributing at all); fewer returns all
 * zeros.
 */
export function getDeltasForDistributeByGaps(itemExtents: ReadonlyArray<readonly [number, number]>): number[] {
  const deltas = itemExtents.map(() => 0);
  if (itemExtents.length < 3) return deltas;

  const totalSpace = itemExtents[itemExtents.length - 1]![0] - itemExtents[0]![1];
  let totalGap = totalSpace;
  for (let i = 1; i < itemExtents.length - 1; i++) {
    const [start, end] = itemExtents[i]!;
    totalGap -= end - start;
  }
  const perItemGap = totalGap / (itemExtents.length - 1);

  let targetPos = itemExtents[0]![1];
  for (let i = 1; i < itemExtents.length - 1; i++) {
    const [start, end] = itemExtents[i]!;
    // Source keeps the rounding accumulator separate from targetPos
    // (re-multiplying `perItemGap` by `i` every time) specifically so
    // per-step rounding error can't stack across items -- preserved here.
    const accumulatedGaps = i * perItemGap;
    deltas[i] = targetPos - start + Math.round(accumulatedGaps);
    targetPos += end - start;
  }
  return deltas;
}

/**
 * `GetDeltasForDistributeByPoints` -- same end-caps-fixed idea as the gaps
 * version above, but evenly spacing each item's *center point* (or any
 * single representative coordinate) between the first and last instead of
 * equalizing gaps between extents. `aItemPositions` must already be sorted
 * ascending.
 */
export function getDeltasForDistributeByPoints(itemPositions: readonly number[]): number[] {
  const deltas = itemPositions.map(() => 0);
  if (itemPositions.length < 3) return deltas;

  const startPos = itemPositions[0]!;
  const totalGaps = itemPositions[itemPositions.length - 1]! - startPos;
  const itemGap = totalGaps / (itemPositions.length - 1);

  for (let i = 1; i < itemPositions.length - 1; i++) {
    const targetPos = startPos + Math.round(i * itemGap);
    deltas[i] = targetPos - itemPositions[i]!;
  }
  return deltas;
}
