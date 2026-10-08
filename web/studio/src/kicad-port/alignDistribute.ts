// Port of pcbnew/tools/align_distribute_tool.cpp (the Align/Distribute
// submenu, no default hotkeys in source either) and libs/kimath/src/
// geometry/distribute.cpp's two gap-math helpers it calls.
//
// Every kind of item aligns and distributes, as in source (`BOARD_ITEM::Move`): placed footprints (by their courtyard, standing in for
// `FOOTPRINT::GetBoundingBox( false )`), tracks, vias, zones, graphics, text, dimensions and groups. A pad selected on its own stands for its
// footprint but aligns by the pad's box (`GetSelections`' `addToList( list, pad, parentFp )`).
//
// Locks are KiCad's two rules (`GetSelections`, `selectTarget`, `DistributeItems`): Align leaves a locked item where it is and moves
// everything else onto IT -- "prefer locked items to unlocked items" -- while Distribute leaves locked items out altogether
// (`FilterCollectorForLockedItems`). Without a locked item the alignment target is the extreme item of the selection unless the cursor sits
// inside one item's box, in which case that item is the target ("secondly, prefer items under the cursor"). Source's mirrored-view swap of Align
// Left/Right is not ported: this app's view never mirrors a PCB.
import type { BoardState, Cmd, Um } from "../api/types";
import { itemBounds, itemKind, padParent } from "./pcbItems";
import { editableSelection, groupOf, isLocked } from "./pcbTransform";

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


// ---------------------------------------------------------------------------
// The tool half: which items, which target, which moves.

/** One item the tool works on: what it moves (`id`), the box it is aligned by, and whether a lock keeps it where it is. */
export interface AlignItem {
  id: string;
  box: Box;
  locked: boolean;
}

/** A move the tool makes: the item `id` by `(dx, dy)`. */
export interface AlignMove {
  id: string;
  dx: number;
  dy: number;
}

type Pt = readonly [number, number];

const contains = (box: Box, p: Pt): boolean => p[0] >= box[0] && p[0] <= box[2] && p[1] >= box[1] && p[1] <= box[3];

/** The sort each Align action gives `GetSelections`' lists: top/left/centres ascending, bottom/right descending (the extreme item first). */
function sortedFor(items: readonly AlignItem[], edge: AlignEdge): AlignItem[] {
  const sign = isMinExtreme(edge) ? 1 : -1;
  return [...items].sort((a, b) => sign * (edgeValue(a.box, edge) - edgeValue(b.box, edge)));
}

/**
 * `ALIGN_DISTRIBUTE_TOOL::selectTarget`: a locked item wins (the one under the cursor, else the first), else the item under the cursor,
 * else the first of the sorted list -- the extreme one.
 */
function targetValue(unlocked: readonly AlignItem[], locked: readonly AlignItem[], edge: AlignEdge, cursor: Pt | null | undefined): number {
  const pool = locked.length > 0 ? locked : unlocked;
  const under = cursor ? pool.find((it) => contains(it.box, cursor)) : undefined;
  return edgeValue((under ?? pool[0]!).box, edge);
}

/** `AlignTop` ... `AlignCenterY`: what moves, and by how much, to put `items` on the target's edge. Nothing moves when everything is locked. */
export function planAlign(items: readonly AlignItem[], edge: AlignEdge, cursor?: Pt | null): AlignMove[] {
  const unlocked = sortedFor(items.filter((it) => !it.locked), edge);
  const locked = sortedFor(items.filter((it) => it.locked), edge);
  if (unlocked.length === 0) return [];
  const target = targetValue(unlocked, locked, edge, cursor);
  const axis = alignAxis(edge);
  const out: AlignMove[] = [];
  for (const it of unlocked) {
    const d = target - edgeValue(it.box, edge);
    if (d !== 0) out.push({ id: it.id, dx: axis === "x" ? d : 0, dy: axis === "y" ? d : 0 });
  }
  return out;
}

/**
 * `DistributeItems`: the items spaced evenly along `axis`, the two at the ends staying where they are. Locked items were already taken out
 * (the caller filters them, `FilterCollectorForLockedItems`); fewer than three items distribute nothing.
 */
export function planDistribute(items: readonly AlignItem[], axis: "x" | "y", mode: "gaps" | "centers"): AlignMove[] {
  if (items.length < 3) return [];
  const start = (b: Box): number => (axis === "x" ? b[0] : b[1]);
  const end = (b: Box): number => (axis === "x" ? b[2] : b[3]);
  const sorted = [...items].sort((a, b) => (mode === "gaps" ? start(a.box) - start(b.box) : (start(a.box) + end(a.box)) / 2 - (start(b.box) + end(b.box)) / 2));
  const deltas = mode === "gaps" ? getDeltasForDistributeByGaps(sorted.map((it) => [start(it.box), end(it.box)] as [number, number])) : getDeltasForDistributeByPoints(sorted.map((it) => (start(it.box) + end(it.box)) / 2));
  const out: AlignMove[] = [];
  for (let i = 1; i < sorted.length - 1; i++) {
    const d = deltas[i]!;
    if (d !== 0) out.push({ id: sorted[i]!.id, dx: axis === "x" ? d : 0, dy: axis === "y" ? d : 0 });
  }
  return out;
}

/**
 * `ALIGN_DISTRIBUTE_TOOL::GetSelections`' list for a selection: every item with its box and lock. A selected pad is listed as its footprint
 * (once, by the first pad's box); an item that belongs to a group that is selected too is left to the group, which moves it.
 */
export function alignItems(board: BoardState, selection: readonly string[]): AlignItem[] {
  const out: AlignItem[] = [];
  const listed = new Set<string>();
  const chosen = new Set(selection);
  for (const raw of selection) {
    const isPad = itemKind(board, raw) === "pad";
    const id = isPad ? padParent(board, raw) : raw;
    if (id == null || listed.has(id) || itemKind(board, id) == null) continue;
    const group = groupOf(board, id);
    if (group != null && chosen.has(group)) continue;
    const box = isPad ? itemBounds(board, raw) : itemBounds(board, id);
    if (!box) continue;
    listed.add(id);
    out.push({ id, box: [box[0], box[1], box[2], box[3]], locked: isLocked(board, id) });
  }
  return out;
}

const moveCmds = (moves: readonly AlignMove[]): Cmd[] => moves.map((m): Cmd => ({ op: "move_items", ids: [m.id], dx: Math.round(m.dx), dy: Math.round(m.dy) }));

/** Align the selection (`cursor` is where the pointer is, which can pick the target): the commands, to be sent as one undo step. */
export function planAlignSelection(board: BoardState, selection: readonly string[], edge: AlignEdge, cursor?: Pt | null): Cmd[] {
  return moveCmds(planAlign(alignItems(board, selection), edge, cursor));
}

/** Distribute the selection: locked items left out, a pad standing for its footprint. */
export function planDistributeSelection(board: BoardState, selection: readonly string[], axis: "x" | "y", mode: "gaps" | "centers"): Cmd[] {
  const { ids } = editableSelection(board, selection);
  return moveCmds(planDistribute(alignItems(board, ids), axis, mode));
}

/** Whether a selection has enough to align (two) or distribute (three) -- the menu conditions `MoreThan( 1 )` / `MoreThan( 2 )`. */
export function alignable(board: BoardState, selection: readonly string[]): number {
  return alignItems(board, selection).length;
}

