// Hit-testing for the non-footprint selectable items (item 7: tracks,
// vias, zones, shapes, text) -- Canvas.tsx's existing partHit/partsAt
// cover footprints; this covers everything the model extension added.
// Not pixel-perfect against KiCad's own hit-test geometry (e.g. a real
// stroked-arc hit test, or true point-in-stroke for a thick polyline) --
// reasonable distance/bounds checks, good enough to select something
// that's visibly under the cursor at a normal zoom level.
import type { BoardState, BoardText, Shape } from "../../api/types";

export type HitKind = "track" | "via" | "zone" | "shape" | "text";
export interface Hit {
  kind: HitKind;
  id: string;
}

export function distToSegment(px: number, py: number, ax: number, ay: number, bx: number, by: number): number {
  const dx = bx - ax,
    dy = by - ay;
  const lenSq = dx * dx + dy * dy;
  let t = lenSq > 0 ? ((px - ax) * dx + (py - ay) * dy) / lenSq : 0;
  t = Math.max(0, Math.min(1, t));
  return Math.hypot(px - (ax + t * dx), py - (ay + t * dy));
}

export function distToPolyline(px: number, py: number, pts: readonly (readonly [number, number])[]): number {
  let best = Infinity;
  for (let i = 0; i + 1 < pts.length; i++) {
    const [ax, ay] = pts[i]!;
    const [bx, by] = pts[i + 1]!;
    best = Math.min(best, distToSegment(px, py, ax, ay, bx, by));
  }
  return best;
}

export function pointInPolygon(px: number, py: number, pts: readonly (readonly [number, number])[]): boolean {
  let inside = false;
  for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) {
    const [xi, yi] = pts[i]!;
    const [xj, yj] = pts[j]!;
    if (yi > py !== yj > py && px < ((xj - xi) * (py - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

/** Every point a shape's geometry is made of -- the model's own `Shape::points()` (crates/model/src/ir.rs), mirrored here for the bounding-box math a box-select needs (selectionCandidates.ts's `collectBoxSelection`). */
export function shapePoints(s: Shape): Array<readonly [number, number]> {
  switch (s.kind) {
    case "segment":
    case "rect":
      return [s.start, s.end];
    case "arc":
      return [s.start, s.mid, s.end];
    case "circle":
      return [s.center, s.end];
    case "polygon":
      return s.pts;
  }
}

/** Axis-aligned bounding box of a shape's actual extent -- circle is special-cased (its two `points()` are the center and an edge point, not the extent) so a box-select against it is correct, not just against its defining points. */
export function shapeBoundingBox(s: Shape): [number, number, number, number] {
  if (s.kind === "circle") {
    const r = Math.hypot(s.end[0] - s.center[0], s.end[1] - s.center[1]);
    return [s.center[0] - r, s.center[1] - r, s.center[0] + r, s.center[1] + r];
  }
  const pts = shapePoints(s);
  let x0 = Infinity,
    y0 = Infinity,
    x1 = -Infinity,
    y1 = -Infinity;
  for (const [x, y] of pts) {
    x0 = Math.min(x0, x);
    y0 = Math.min(y0, y);
    x1 = Math.max(x1, x);
    y1 = Math.max(y1, y);
  }
  return [x0, y0, x1, y1];
}

/** Unsigned polygon area via the shoelace formula -- used both for a zone/polygon's hit area (`shapeArea`/pcb_selection_tool.cpp's `FOOTPRINT::GetCoverageArea` stand-in, see `selectionCandidates.ts`) and nowhere else yet. */
export function polygonArea(pts: readonly (readonly [number, number])[]): number {
  if (pts.length < 3) return 0;
  let sum = 0;
  for (let i = 0; i < pts.length; i++) {
    const [x0, y0] = pts[i]!;
    const [x1, y1] = pts[(i + 1) % pts.length]!;
    sum += x0 * y1 - x1 * y0;
  }
  return Math.abs(sum) / 2;
}

export function shapeHitDistance(s: Shape, px: number, py: number): number {
  switch (s.kind) {
    case "segment":
      return distToSegment(px, py, s.start[0], s.start[1], s.end[0], s.end[1]);
    case "rect": {
      const pts = [s.start, [s.end[0], s.start[1]], s.end, [s.start[0], s.end[1]]] as const;
      if (s.filled && pointInPolygon(px, py, pts)) return 0;
      return distToPolyline(px, py, [...pts, pts[0]]);
    }
    case "circle": {
      const r = Math.hypot(s.end[0] - s.center[0], s.end[1] - s.center[1]);
      const d = Math.hypot(px - s.center[0], py - s.center[1]);
      if (s.filled && d <= r) return 0;
      return Math.abs(d - r);
    }
    case "polygon": {
      if (s.filled && pointInPolygon(px, py, s.pts)) return 0;
      return distToPolyline(px, py, [...s.pts, s.pts[0]!]);
    }
    case "arc":
      // No stroked-arc distance here (would need the same circumcircle
      // math painter.ts uses just to hit-test) -- start/mid/end give a
      // reasonable stand-in via the two chords, good enough to click an
      // arc that's on screen at a normal zoom.
      return Math.min(distToSegment(px, py, s.start[0], s.start[1], s.mid[0], s.mid[1]), distToSegment(px, py, s.mid[0], s.mid[1], s.end[0], s.end[1]));
  }
}

/** `pcb_selection_tool.cpp`'s per-kind area in `GuessSelectionCandidates`'s `itemsByArea`, approximated for the kinds this app's shapes cover (no Clipper polygon-area here, see that function's own header comment): a stroked segment/arc's thin bounding stripe (length*strokeWidth), a rect/circle/polygon's actual geometric area. */
export function shapeArea(s: Shape): number {
  switch (s.kind) {
    case "segment":
      return Math.hypot(s.end[0] - s.start[0], s.end[1] - s.start[1]) * Math.max(s.stroke_width, 1);
    case "arc": {
      // Two-chord length stand-in (same approximation shapeHitDistance's arc case already uses) -- a real arc-length needs the circumcircle math painter.ts computes just to draw it.
      const len = Math.hypot(s.mid[0] - s.start[0], s.mid[1] - s.start[1]) + Math.hypot(s.end[0] - s.mid[0], s.end[1] - s.mid[1]);
      return len * Math.max(s.stroke_width, 1);
    }
    case "rect":
      return Math.abs(s.end[0] - s.start[0]) * Math.abs(s.end[1] - s.start[1]);
    case "circle": {
      const r = Math.hypot(s.end[0] - s.center[0], s.end[1] - s.center[1]);
      return Math.PI * r * r;
    }
    case "polygon":
      return polygonArea(s.pts);
  }
}

/** A rough text bounding box: size-tall, ~0.6*size per character wide (matches painter.ts's own font-metric-free approach), positioned per justify. Shared by the hit test below and `selectionCandidates.ts`'s area computation. */
export function textBoundingBox(t: BoardText): { x0: number; y0: number; x1: number; y1: number } {
  const halfH = t.size / 2;
  const w = Math.max(t.content.length, 1) * t.size * 0.6;
  const x0 = t.justify === "left" ? t.x : t.justify === "right" ? t.x - w : t.x - w / 2;
  return { x0, y0: t.y - halfH, x1: x0 + w, y1: t.y + halfH };
}

/** Every non-footprint item under (xUm, yUm), nearest first -- Canvas.tsx tries this after partHit() finds nothing, so a footprint under a track/zone still wins (matches KiCad: copper items are behind footprints in the click-priority KiCad's own selection tool uses). */
export function itemHitsAt(board: BoardState, xUm: number, yUm: number, toleranceUm: number): Hit[] {
  const hits: Array<Hit & { d: number }> = [];

  for (const t of board.routing?.tracks ?? []) {
    const d = distToPolyline(xUm, yUm, t.pts) - t.width / 2;
    if (d <= toleranceUm) hits.push({ kind: "track", id: t.id, d: Math.max(d, 0) });
  }
  for (const v of board.routing?.vias ?? []) {
    const d = Math.hypot(xUm - v.x, yUm - v.y) - v.d / 2;
    if (d <= toleranceUm) hits.push({ kind: "via", id: v.id, d: Math.max(d, 0) });
  }
  for (const z of board.routing?.zones ?? []) {
    if (z.outline.length < 3) continue;
    const inside = pointInPolygon(xUm, yUm, z.outline);
    const d = inside ? 0 : distToPolyline(xUm, yUm, [...z.outline, z.outline[0]!]);
    if (d <= toleranceUm) hits.push({ kind: "zone", id: z.id, d });
  }
  for (const s of board.drawings?.shapes ?? []) {
    const d = shapeHitDistance(s, xUm, yUm);
    if (d <= toleranceUm) hits.push({ kind: "shape", id: s.id, d });
  }
  for (const t of board.drawings?.texts ?? []) {
    const { x0, y0, x1, y1 } = textBoundingBox(t);
    const d = xUm >= x0 && xUm <= x1 && yUm >= y0 && yUm <= y1 ? 0 : Math.hypot(xUm - t.x, yUm - t.y) - (x1 - x0) / 2;
    if (d <= toleranceUm) hits.push({ kind: "text", id: t.id, d: Math.max(d, 0) });
  }

  hits.sort((a, b) => a.d - b.d);
  return hits;
}
