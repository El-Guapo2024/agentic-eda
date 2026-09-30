// The view transform: screen pixels <-> board micrometres. Kept as a
// tiny plain-object + functions "GAL-like layer" rather than a class, so
// swapping the Canvas2D painter for a WebGL one later only touches
// painter.ts, not this module or the state shape (ViewTransform lives in
// state/store.tsx since the reducer owns it).

import type { ViewTransform } from "../../state/store";

export function worldToScreen(v: ViewTransform, xUm: number, yUm: number): [number, number] {
  return [xUm * v.scale + v.x, yUm * v.scale + v.y];
}

export function screenToWorld(v: ViewTransform, sx: number, sy: number): [number, number] {
  return [(sx - v.x) / v.scale, (sy - v.y) / v.scale];
}

/** A hairline width in µm that renders as `px` screen pixels at the current zoom -- Canvas2D's stand-in for SVG's `vector-effect: non-scaling-stroke`. */
export function hairlineUm(v: ViewTransform, px = 1): number {
  return v.scale > 0 ? px / v.scale : px;
}

export interface Bounds {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

export function boundsOfPoints(points: Array<[number, number]>): Bounds | null {
  if (points.length === 0) return null;
  let minX = Infinity,
    minY = Infinity,
    maxX = -Infinity,
    maxY = -Infinity;
  for (const [x, y] of points) {
    if (x < minX) minX = x;
    if (y < minY) minY = y;
    if (x > maxX) maxX = x;
    if (y > maxY) maxY = y;
  }
  return { minX, minY, maxX, maxY };
}

/** A view transform that fits `bounds` inside `width`x`height` screen pixels, with `padPx` margin. */
export function fitTransform(bounds: Bounds, width: number, height: number, padPx = 30): ViewTransform {
  const w = Math.max(bounds.maxX - bounds.minX, 1);
  const h = Math.max(bounds.maxY - bounds.minY, 1);
  const scale = Math.min((width - 2 * padPx) / w, (height - 2 * padPx) / h);
  return {
    scale,
    x: (width - w * scale) / 2 - bounds.minX * scale,
    y: (height - h * scale) / 2 - bounds.minY * scale,
  };
}

/** Zoom by `factor` about screen point (px, py), clamped to a sane range. */
export function zoomAbout(v: ViewTransform, px: number, py: number, factor: number): ViewTransform {
  const scale = Math.max(1e-5, Math.min(50, v.scale * factor));
  return {
    scale,
    x: px - (px - v.x) * (scale / v.scale),
    y: py - (py - v.y) * (scale / v.scale),
  };
}
