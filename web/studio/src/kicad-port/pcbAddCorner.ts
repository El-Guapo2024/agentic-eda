// Add Corner (`Insert`, the point editor's "Create Corner" in the context menu): `PCB_POINT_EDITOR::addCorner` (pcbnew/tools/pcb_point_editor.cpp at 8303b2ad).
//
// The new corner is not the cursor: it is the point of the outline nearest the cursor.
//
//   zone, polygon      the nearest side of the outline (every contour, no distance limit: the side the cursor is nearest to wins), `SEG::NearestPoint( cursorPos )`
//                      on it, inserted before that side's end (`InsertVertex( nextNearestIdx, nearestPoint )`); "Do not add points that have the same coordinates as
//                      ones that already belong to polygon, instead, add a point in the middle of the side";
//   segment            split at the nearest point of the segment: the first half ends there, a copy of it starts there (`Split Segment`);
//   arc                split at the nearest point of the arc, both halves on the same circle and in the same direction (`Split Arc`).
//
// `cursorPos` is `getViewControls()->GetCursorPosition()`, the cursor on the grid, which the caller gives. Pure: no store, no DOM. Unit-tested in pcbAddCorner.test.ts.
import type { BoardState, Cmd, CmdShape, Shape } from "../api/types";
import { shapeToCmd } from "./pcbPointEdit";
import { circumcircle, splitArcAt, type P } from "./trackArc";

export type { P };

/** KiCad's integer division of a sum of two coordinates by two (truncation toward zero). */
const half = (v: number): number => Math.trunc(v / 2);

const same = (a: P, b: P): boolean => a[0] === b[0] && a[1] === b[1];

/** `rescale( t, d, l_squared )`: `t * d / l_squared`, rounded to the nearest integer. */
const rescale = (t: number, d: number, l2: number): number => Math.round((t * d) / l2);

/** `SEG::NearestPoint( aP )`: the nearest point of the segment `a`-`b` to `p`, on whole micrometres. */
export function nearestOnSegment(a: P, b: P, p: P): P {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const l2 = dx * dx + dy * dy;
  if (l2 === 0) return [a[0], a[1]];
  const t = dx * (p[0] - a[0]) + dy * (p[1] - a[1]);
  if (t < 0) return [a[0], a[1]];
  if (t > l2) return [b[0], b[1]];
  return [a[0] + rescale(t, dx, l2), a[1] + rescale(t, dy, l2)];
}

/** `SEG::Distance( aP )`. */
export function distanceToSegment(a: P, b: P, p: P): number {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const l2 = dx * dx + dy * dy;
  if (l2 === 0) return Math.hypot(p[0] - a[0], p[1] - a[1]);
  const t = Math.max(0, Math.min(1, (dx * (p[0] - a[0]) + dy * (p[1] - a[1])) / l2));
  return Math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy));
}

/**
 * A zone's or polygon's outline with the corner added: `ring` (closed, the last side runs back to the first point) with the point of its nearest side to `cursor`
 * inserted at the end of that side -- the middle of the side when that point is already one of its ends. The ring is left alone (null) with fewer than two points.
 */
export function addCornerToRing(ring: readonly P[], cursor: P): P[] | null {
  const n = ring.length;
  if (n < 2) return null;
  let best = -1;
  let bestDist = Infinity;
  for (let i = 0; i < n; i++) {
    const d = distanceToSegment(ring[i]!, ring[(i + 1) % n]!, cursor);
    if (d < bestDist) {
      bestDist = d;
      best = i;
    }
  }
  const origin = ring[best]!;
  const jj = (best + 1) % n;
  const end = ring[jj]!;
  let at = nearestOnSegment(origin, end, cursor);
  if (same(at, origin) || same(at, end)) at = [half(origin[0] + end[0]), half(origin[1] + end[1])];
  const out = ring.map((p): P => [p[0], p[1]]);
  out.splice(jj, 0, at);
  return out;
}

/** `Split Segment`: the segment `start`-`end` cut at the point of it nearest `cursor`, as the two segments it becomes; null when that point is one of its ends (nothing to cut). */
export function splitSegmentAt(start: P, end: P, cursor: P): [{ start: P; end: P }, { start: P; end: P }] | null {
  const at = nearestOnSegment(start, end, cursor);
  if (same(at, start) || same(at, end)) return null;
  return [
    { start: [start[0], start[1]], end: at },
    { start: at, end: [end[0], end[1]] },
  ];
}

/** `SHAPE_ARC::NearestPoint( aP )`: the nearest point of the arc through `start`, `mid`, `end` to `p`: the circle's point in the direction of `p` when it is on the arc, else the nearer end. */
export function nearestOnArc(start: P, mid: P, end: P, p: P): P {
  const c = circumcircle(start, mid, end);
  if (!c) return nearestOnSegment(start, end, p);
  const twoPi = Math.PI * 2;
  const norm = (a: number): number => ((a % twoPi) + twoPi) % twoPi;
  const ang = (q: P): number => Math.atan2(q[1] - c.cy, q[0] - c.cx);
  const a0 = ang(start);
  const toEnd = norm(ang(end) - a0);
  const ccw = norm(ang(mid) - a0) <= toEnd;
  const total = ccw ? toEnd : norm(a0 - ang(end));
  const toP = ccw ? norm(ang(p) - a0) : norm(a0 - ang(p));
  if (toP <= total) {
    const a = ang(p);
    return [Math.round(c.cx + c.r * Math.cos(a)), Math.round(c.cy + c.r * Math.sin(a))];
  }
  const ds = Math.hypot(p[0] - start[0], p[1] - start[1]);
  const de = Math.hypot(p[0] - end[0], p[1] - end[1]);
  return ds <= de ? [start[0], start[1]] : [end[0], end[1]];
}

/** `Split Arc`: the arc cut at the point of it nearest `cursor`; null when that point is one of its ends. */
export function splitArcAtCursor(start: P, mid: P, end: P, cursor: P): [{ start: P; mid: P; end: P }, { start: P; mid: P; end: P }] | null {
  const at = nearestOnArc(start, mid, end, cursor);
  if (same(at, start) || same(at, end)) return null;
  return splitArcAt(start, mid, end, at);
}

/**
 * What `Create Corner` (Add Corner) edits: the one selected item that `PCB_POINT_EDITOR::CanAddCorner` accepts -- a zone, a polygon, a segment or an arc (a rectangle,
 * a circle and a curve have no corner to add).
 */
export type CornerTarget = { kind: "zone"; id: string; ring: P[] } | { kind: "polygon"; id: string; shape: Extract<Shape, { kind: "polygon" }> } | { kind: "segment"; id: string; shape: Extract<Shape, { kind: "segment" }> } | { kind: "arc"; id: string; shape: Extract<Shape, { kind: "arc" }> };

export function cornerTarget(board: BoardState | null, selection: ReadonlySet<string>): CornerTarget | null {
  if (!board || selection.size !== 1) return null;
  const id = [...selection][0]!;
  const zone = board.routing?.zones.find((z) => z.id === id);
  if (zone) return zone.teardrop ? null : { kind: "zone", id, ring: zone.outline.map(([x, y]) => [x, y] as P) };
  const shape = board.drawings?.shapes.find((s) => s.id === id);
  if (shape?.kind === "polygon") return { kind: "polygon", id, shape };
  if (shape?.kind === "segment") return { kind: "segment", id, shape };
  if (shape?.kind === "arc") return { kind: "arc", id, shape };
  return null;
}

/** The commands `addCorner` sends for `target` with the cursor at `cursor` (the grid point), null when nothing is to be done. */
export function addCornerCmds(target: CornerTarget, cursor: P): Cmd[] | null {
  const copy = (shape: Shape, patch: Partial<CmdShape>): CmdShape => ({ ...shapeToCmd(shape), ...patch }) as CmdShape;
  switch (target.kind) {
    case "zone": {
      const ring = addCornerToRing(target.ring, cursor);
      return ring ? [{ op: "set_zone_outline", id: target.id, outline: ring.map(([x, y]) => ({ x, y })) }] : null;
    }
    case "polygon": {
      const ring = addCornerToRing(target.shape.pts, cursor);
      return ring ? [{ op: "replace_shape", id: target.id, shape: copy(target.shape, { pts: ring.map(([x, y]) => ({ x, y })) }) }] : null;
    }
    case "segment": {
      // `Split Segment`: the first half ends at the break point, a copy of it starts there.
      const halves = splitSegmentAt(target.shape.start, target.shape.end, cursor);
      if (!halves) return null;
      const [a, b] = halves;
      return [
        { op: "replace_shape", id: target.id, shape: copy(target.shape, { start: { x: a.start[0], y: a.start[1] }, end: { x: a.end[0], y: a.end[1] } }) },
        { op: "add_shape", shape: copy(target.shape, { start: { x: b.start[0], y: b.start[1] }, end: { x: b.end[0], y: b.end[1] } }) },
      ];
    }
    case "arc": {
      const halves = splitArcAtCursor(target.shape.start, target.shape.mid, target.shape.end, cursor);
      if (!halves) return null;
      const [a, b] = halves;
      const pt = (p: P) => ({ x: p[0], y: p[1] });
      return [
        { op: "replace_shape", id: target.id, shape: copy(target.shape, { start: pt(a.start), mid: pt(a.mid), end: pt(a.end) }) },
        { op: "add_shape", shape: copy(target.shape, { start: pt(b.start), mid: pt(b.mid), end: pt(b.end) }) },
      ];
    }
  }
}

