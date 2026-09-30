// Hit-testing for the non-footprint selectable items (item 7: tracks,
// vias, zones, shapes, text) -- Canvas.tsx's existing partHit/partsAt
// cover footprints; this covers everything the model extension added.
// Not pixel-perfect against KiCad's own hit-test geometry (e.g. a real
// stroked-arc hit test, or true point-in-stroke for a thick polyline) --
// reasonable distance/bounds checks, good enough to select something
// that's visibly under the cursor at a normal zoom level.
import type { BoardState, Shape } from "../../api/types";

export type HitKind = "track" | "via" | "zone" | "shape" | "text";
export interface Hit {
  kind: HitKind;
  id: string;
}

function distToSegment(px: number, py: number, ax: number, ay: number, bx: number, by: number): number {
  const dx = bx - ax,
    dy = by - ay;
  const lenSq = dx * dx + dy * dy;
  let t = lenSq > 0 ? ((px - ax) * dx + (py - ay) * dy) / lenSq : 0;
  t = Math.max(0, Math.min(1, t));
  return Math.hypot(px - (ax + t * dx), py - (ay + t * dy));
}

function distToPolyline(px: number, py: number, pts: readonly (readonly [number, number])[]): number {
  let best = Infinity;
  for (let i = 0; i + 1 < pts.length; i++) {
    const [ax, ay] = pts[i]!;
    const [bx, by] = pts[i + 1]!;
    best = Math.min(best, distToSegment(px, py, ax, ay, bx, by));
  }
  return best;
}

function pointInPolygon(px: number, py: number, pts: readonly (readonly [number, number])[]): boolean {
  let inside = false;
  for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) {
    const [xi, yi] = pts[i]!;
    const [xj, yj] = pts[j]!;
    if (yi > py !== yj > py && px < ((xj - xi) * (py - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

function shapeHitDistance(s: Shape, px: number, py: number): number {
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
    // A rough text bounding box: size-tall, ~0.6*size per character wide (matches painter.ts's own font-metric-free approach), centered/left/right per justify.
    const halfH = t.size / 2;
    const w = Math.max(t.content.length, 1) * t.size * 0.6;
    const x0 = t.justify === "left" ? t.x : t.justify === "right" ? t.x - w : t.x - w / 2;
    const d = xUm >= x0 && xUm <= x0 + w && yUm >= t.y - halfH && yUm <= t.y + halfH ? 0 : Math.hypot(xUm - t.x, yUm - t.y) - w / 2;
    if (d <= toleranceUm) hits.push({ kind: "text", id: t.id, d: Math.max(d, 0) });
  }

  hits.sort((a, b) => a.d - b.d);
  return hits;
}
