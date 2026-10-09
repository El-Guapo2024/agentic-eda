// Align Left / Right / Top / Bottom / Center for schematic items -- `SCH_ALIGN_TOOL` (eeschema/tools/sch_align_tool.cpp at 8303b2ad): every
// selected item that is not locked moves so the chosen edge (or centre) of its box lines up with the target's. The target is, as
// `selectTarget` has it, a locked item (the one under the cursor, else the first) when the selection holds any, else the item under the cursor,
// else the first after sorting (the leftmost for Align Left, the topmost for Align Top, the rightmost for Align Right...).
//
// This only measures: it hands back one offset per item, and the backend's `align` verb (crates/ops/src/sch_move.rs) moves them -- snapped to
// the connection grid (`adjustDeltaForGrid`) and with the wires on each item stretching -- as one undo step.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
export type SchAlignKind = "left" | "right" | "top" | "bottom" | "centerX" | "centerY";

export interface SchAlignItem {
  id: string;
  /** `[minX, minY, maxX, maxY]`, sheet µm: what the studio draws and picks the item by. */
  box: readonly [number, number, number, number];
  locked: boolean;
}

export interface SchAlignMove {
  id: string;
  dx: number;
  dy: number;
}

/** `Centre()` of a KiCad box: the corner plus half the size, truncated. */
const centre = (b: SchAlignItem["box"]): [number, number] => [b[0] + Math.trunc((b[2] - b[0]) / 2), b[1] + Math.trunc((b[3] - b[1]) / 2)];

function edge(item: SchAlignItem, kind: SchAlignKind): number {
  const [x0, y0, x1, y1] = item.box;
  switch (kind) {
    case "left":
      return x0;
    case "right":
      return x1;
    case "top":
      return y0;
    case "bottom":
      return y1;
    case "centerX":
      return centre(item.box)[0];
    case "centerY":
      return centre(item.box)[1];
  }
}

/** Align Right and Bottom sort descending, the rest ascending (each action's own comparator). */
const descending = (kind: SchAlignKind) => kind === "right" || kind === "bottom";

const contains = (b: SchAlignItem["box"], p: readonly [number, number]) => p[0] >= b[0] && p[0] <= b[2] && p[1] >= b[1] && p[1] <= b[3];

/** The offsets that align `items`; empty when fewer than two items are given or none of them can move. */
export function alignMoves(items: readonly SchAlignItem[], kind: SchAlignKind, cursor: readonly [number, number] | null): SchAlignMove[] {
  if (items.length < 2) return [];
  const sign = descending(kind) ? -1 : 1;
  const order = (a: SchAlignItem, b: SchAlignItem) => sign * (edge(a, kind) - edge(b, kind));
  const movable = items.filter((i) => !i.locked).sort(order);
  const locked = items.filter((i) => i.locked).sort(order);
  if (movable.length === 0) return [];
  const pool = locked.length > 0 ? locked : movable;
  const target = (cursor ? pool.find((i) => contains(i.box, cursor)) : undefined) ?? pool[0]!;
  const to = edge(target, kind);
  const horizontal = kind === "left" || kind === "right" || kind === "centerX";
  return movable.map((i) => {
    const d = to - edge(i, kind);
    return { id: i.id, dx: horizontal ? d : 0, dy: horizontal ? 0 : d };
  });
}
