// Port of pcbnew/tools/pcb_point_editor.cpp's zone-outline editing --
// drag a corner, double-click an edge to add one, delete a corner --
// scoped to what a single selected zone's outline needs. Not ported:
// source's 45/90-degree edge-angle constraint while dragging (Ctrl held),
// its "equal length" guide overlay, and editing anything other than a
// zone's outline (a graphic shape's points have no point editor here
// either, same gap PARITY-pcb.md's Property dialogs section already
// names for Shape Properties).
import type { Um } from "../api/types";

/** Squared distance, point to point -- avoids a sqrt where only comparison against a squared tolerance is needed. */
function dist2(ax: number, ay: number, bx: number, by: number): number {
  const dx = ax - bx;
  const dy = ay - by;
  return dx * dx + dy * dy;
}

/** The outline corner nearest `(x, y)`, if within `toleranceUm`; null otherwise. Ties (shouldn't happen in practice) go to the lowest index. */
export function findNearestCorner(outline: ReadonlyArray<readonly [Um, Um]>, x: Um, y: Um, toleranceUm: number): number | null {
  let best: number | null = null;
  let bestD2 = toleranceUm * toleranceUm;
  for (let i = 0; i < outline.length; i++) {
    const [cx, cy] = outline[i]!;
    const d2 = dist2(x, y, cx, cy);
    if (d2 <= bestD2) {
      best = i;
      bestD2 = d2;
    }
  }
  return best;
}

/** Squared distance from `(x,y)` to the segment `(ax,ay)-(bx,by)`. */
function segDist2(x: number, y: number, ax: number, ay: number, bx: number, by: number): number {
  const vx = bx - ax;
  const vy = by - ay;
  const len2 = vx * vx + vy * vy;
  if (len2 === 0) return dist2(x, y, ax, ay);
  let t = ((x - ax) * vx + (y - ay) * vy) / len2;
  t = Math.max(0, Math.min(1, t));
  return dist2(x, y, ax + t * vx, ay + t * vy);
}

/**
 * Which edge of a closed outline (vertex `i` to vertex `i+1`, wrapping)
 * `(x, y)` is nearest to, if within `toleranceUm` -- the insertion index a
 * new corner there would get (so the new point lands *between* that
 * edge's two existing corners). Null if nothing is close enough, or the
 * point is actually closer to a corner (`findNearestCorner` should be
 * tried first -- a double-click meant to grab a corner should never
 * insert a new one next to it instead).
 */
export function findNearestEdgeInsertionIndex(outline: ReadonlyArray<readonly [Um, Um]>, x: Um, y: Um, toleranceUm: number): number | null {
  if (outline.length < 2) return null;
  let best: number | null = null;
  let bestD2 = toleranceUm * toleranceUm;
  for (let i = 0; i < outline.length; i++) {
    const [ax, ay] = outline[i]!;
    const [bx, by] = outline[(i + 1) % outline.length]!;
    const d2 = segDist2(x, y, ax, ay, bx, by);
    if (d2 <= bestD2) {
      best = i + 1;
      bestD2 = d2;
    }
  }
  return best;
}

/** `outline` with a new corner `(x, y)` inserted at `index` (0..=length). */
export function insertCorner(outline: ReadonlyArray<readonly [Um, Um]>, index: number, x: Um, y: Um): [Um, Um][] {
  const next = outline.slice() as [Um, Um][];
  next.splice(index, 0, [x, y]);
  return next;
}

/** `outline` with corner `index` moved to `(x, y)`. */
export function moveCorner(outline: ReadonlyArray<readonly [Um, Um]>, index: number, x: Um, y: Um): [Um, Um][] {
  const next = outline.slice() as [Um, Um][];
  next[index] = [x, y];
  return next;
}

/** `outline` with corner `index` removed, or null if that would leave fewer than 3 points (a zone's own floor, `Cmd::SetZoneOutline`'s validation). */
export function removeCorner(outline: ReadonlyArray<readonly [Um, Um]>, index: number): [Um, Um][] | null {
  if (outline.length <= 3) return null;
  const next = outline.slice() as [Um, Um][];
  next.splice(index, 1);
  return next;
}
