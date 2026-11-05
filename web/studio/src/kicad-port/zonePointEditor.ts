// Port of pcbnew/tools/pcb_point_editor.cpp's zone-outline editing --
// drag a corner, double-click an edge to add one, delete a corner --
// scoped to what a single selected zone's outline needs. Not ported:
// source's 45/90-degree edge-angle constraint while dragging (Ctrl held),
// its "equal length" guide overlay, and editing anything other than a
// zone's outline (a graphic shape's points have no point editor here
// either, same gap PARITY-pcb.md's Property dialogs section already
// names for Shape Properties). A zone with holes has handles on every
// contour (the holes section at the end of this file).
import type { Nm } from "../api/types";

/** Squared distance, point to point -- avoids a sqrt where only comparison against a squared tolerance is needed. */
function dist2(ax: number, ay: number, bx: number, by: number): number {
  const dx = ax - bx;
  const dy = ay - by;
  return dx * dx + dy * dy;
}

/** The outline corner nearest `(x, y)`, if within `toleranceNm`; null otherwise. Ties (shouldn't happen in practice) go to the lowest index. */
export function findNearestCorner(outline: ReadonlyArray<readonly [Nm, Nm]>, x: Nm, y: Nm, toleranceNm: number): number | null {
  let best: number | null = null;
  let bestD2 = toleranceNm * toleranceNm;
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
 * `(x, y)` is nearest to, if within `toleranceNm` -- the insertion index a
 * new corner there would get (so the new point lands *between* that
 * edge's two existing corners). Null if nothing is close enough, or the
 * point is actually closer to a corner (`findNearestCorner` should be
 * tried first -- a double-click meant to grab a corner should never
 * insert a new one next to it instead).
 */
export function findNearestEdgeInsertionIndex(outline: ReadonlyArray<readonly [Nm, Nm]>, x: Nm, y: Nm, toleranceNm: number): number | null {
  if (outline.length < 2) return null;
  let best: number | null = null;
  let bestD2 = toleranceNm * toleranceNm;
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
export function insertCorner(outline: ReadonlyArray<readonly [Nm, Nm]>, index: number, x: Nm, y: Nm): [Nm, Nm][] {
  const next = outline.slice() as [Nm, Nm][];
  next.splice(index, 0, [x, y]);
  return next;
}

/** `outline` with corner `index` moved to `(x, y)`. */
export function moveCorner(outline: ReadonlyArray<readonly [Nm, Nm]>, index: number, x: Nm, y: Nm): [Nm, Nm][] {
  const next = outline.slice() as [Nm, Nm][];
  next[index] = [x, y];
  return next;
}

/** `outline` with corner `index` removed, or null if that would leave fewer than 3 points (a zone's own floor, `Cmd::SetZoneOutline`'s validation). */
export function removeCorner(outline: ReadonlyArray<readonly [Nm, Nm]>, index: number): [Nm, Nm][] | null {
  if (outline.length <= 3) return null;
  const next = outline.slice() as [Nm, Nm][];
  next.splice(index, 1);
  return next;
}

// ---------------------------------------------------------------------------------------- holes
//
// A zone's outline is one polygon with holes (`ZONE::Outline()`: a `SHAPE_POLY_SET` of one polygon, contour 0 the outline, the later contours its holes;
// `eda_model::ir::Zone::holes`). KiCad's point editor numbers the corners of all of its contours one after the other (`SHAPE_POLY_SET::CIterateWithHoles`, the
// "global" vertex index of `CVertex( int )`, `GetRelativeIndices` / `GetGlobalIndex`): the outline's vertices first, then the first hole's, and so on. The
// helpers below work on that numbering, so an edit point `v<i>` / `m<i>` (kicad-port/pcbPointEdit.ts) names a corner or a side of any contour.

/** The contours of a zone in `SHAPE_POLY_SET` order: the outline, then each hole (a hole of fewer than three corners is not one: the server drops it). */
export function zoneContours(zone: { outline: ReadonlyArray<readonly [Nm, Nm]>; holes?: ReadonlyArray<ReadonlyArray<readonly [Nm, Nm]>> }): [Nm, Nm][][] {
  return [zone.outline, ...(zone.holes ?? []).filter((h) => h.length >= 3)].map((ring) => ring.map((p) => [p[0], p[1]] as [Nm, Nm]));
}

/** `SHAPE_POLY_SET::TotalVertices`. */
export function totalVertices(contours: ReadonlyArray<ReadonlyArray<unknown>>): number {
  return contours.reduce((n, c) => n + c.length, 0);
}

/** `SHAPE_POLY_SET::GetRelativeIndices`: the contour and the index within it of global vertex `global`; null when there is no such vertex. */
export function relativeIndex(contours: ReadonlyArray<ReadonlyArray<unknown>>, global: number): { contour: number; index: number } | null {
  if (!Number.isInteger(global) || global < 0) return null;
  let left = global;
  for (let c = 0; c < contours.length; c++) {
    const n = contours[c]!.length;
    if (left < n) return { contour: c, index: left };
    left -= n;
  }
  return null;
}

/** `SHAPE_POLY_SET::GetGlobalIndex`: the global vertex index of vertex `index` of contour `contour`. */
export function globalIndex(contours: ReadonlyArray<ReadonlyArray<unknown>>, contour: number, index: number): number {
  let g = index;
  for (let c = 0; c < contour; c++) g += contours[c]!.length;
  return g;
}

/** `CIterateWithHoles`: every vertex of every contour, in global order. */
export function flatVertices(contours: ReadonlyArray<ReadonlyArray<readonly [Nm, Nm]>>): [Nm, Nm][] {
  return contours.flatMap((c) => c.map((p) => [p[0], p[1]] as [Nm, Nm]));
}

/** `GetNeighbourIndexes`: the global indices of the vertices before and after `global` in its own contour (each contour is closed). */
export function neighbourIndexes(contours: ReadonlyArray<ReadonlyArray<unknown>>, global: number): { prev: number; next: number } | null {
  const at = relativeIndex(contours, global);
  if (!at) return null;
  const n = contours[at.contour]!.length;
  if (n < 2) return null;
  return { prev: globalIndex(contours, at.contour, (at.index + n - 1) % n), next: globalIndex(contours, at.contour, (at.index + 1) % n) };
}

/** [`findNearestCorner`] over every contour (`EDIT_POINTS::FindPoint` on the point editor's points, which `BuildForPolyOutline` makes for the holes too): a global index. */
export function findNearestContourCorner(contours: ReadonlyArray<ReadonlyArray<readonly [Nm, Nm]>>, x: Nm, y: Nm, toleranceNm: number): number | null {
  return findNearestCorner(flatVertices(contours), x, y, toleranceNm);
}

/** `SHAPE_POLY_SET::SetVertex( aGlobalIndex, aPos )`: the contours with global vertex `global` at `(x, y)`; unchanged when there is no such vertex. */
export function moveContourCorner(contours: ReadonlyArray<ReadonlyArray<readonly [Nm, Nm]>>, global: number, x: Nm, y: Nm): [Nm, Nm][][] {
  const at = relativeIndex(contours, global);
  return contours.map((c, i) => (at && i === at.contour ? moveCorner(c, at.index, x, y) : c.map((p) => [p[0], p[1]] as [Nm, Nm])));
}

/**
 * `PCB_POINT_EDITOR::CanRemoveCorner`: the corner is a vertex (`findVertex`) and removing it leaves the polygon whole -- the outline (contour 0) keeps at least
 * three corners, while any corner of a hole can go ("The first condition allows one to remove all corners from holes (when there are only 2 vertices left, a hole
 * is removed)").
 */
export function canRemoveContourCorner(contours: ReadonlyArray<ReadonlyArray<unknown>>, global: number): boolean {
  const at = relativeIndex(contours, global);
  if (!at) return false;
  return !(at.contour === 0 && contours[0]!.length <= 3);
}

/** What `removeCorner` does: the contours that are left, or the item itself goes (`commit.Remove( item )`, an outline of three corners or fewer). */
export type RemoveCornerResult = { kind: "contours"; contours: [Nm, Nm][][] } | { kind: "remove_item" };

/**
 * `PCB_POINT_EDITOR::removeCorner`: "the usual case: remove just the corner when there are >3 vertices"; otherwise "either remove a hole or the polygon when there are
 * <= 3 corners" (`RemoveContour( m_contour )` for a hole, `commit.Remove( item )` for the outline). Null when there is no vertex `global`.
 */
export function removeContourCorner(contours: ReadonlyArray<ReadonlyArray<readonly [Nm, Nm]>>, global: number): RemoveCornerResult | null {
  const at = relativeIndex(contours, global);
  if (!at) return null;
  const copy = contours.map((c) => c.map((p) => [p[0], p[1]] as [Nm, Nm]));
  if (copy[at.contour]!.length > 3) {
    copy[at.contour]!.splice(at.index, 1);
    return { kind: "contours", contours: copy };
  }
  if (at.contour > 0) {
    copy.splice(at.contour, 1);
    return { kind: "contours", contours: copy };
  }
  return { kind: "remove_item" };
}

/**
 * The `set_zone_outline` of `contours` (the outline, then the holes) for zone `id`: the holes ride along when the zone has some or had some before the edit
 * (`hadHoles`: a hole removed, or the last one), else the command keeps the plain shape it always had (the server keeps the holes a command does not send).
 */
export function zoneOutlineCmd(id: string, contours: ReadonlyArray<ReadonlyArray<readonly [Nm, Nm]>>, hadHoles: boolean): { op: "set_zone_outline"; id: string; outline: { x: Nm; y: Nm }[]; holes?: { x: Nm; y: Nm }[][] } {
  const xy = (ring: ReadonlyArray<readonly [Nm, Nm]>) => ring.map(([x, y]) => ({ x, y }));
  const holes = contours.slice(1);
  return { op: "set_zone_outline", id, outline: xy(contours[0] ?? []), ...(hadHoles || holes.length > 0 ? { holes: holes.map(xy) } : {}) };
}
