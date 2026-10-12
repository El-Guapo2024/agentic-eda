// What is under a point of a pad that is not a box, a circle or a stadium: a trapezoid, a chamfered rectangle or a custom pad is, on the studio's state, the contours of its
// copper (`Pad.outline`: micrometres on the board, the outline of each piece and then its holes, filled with the even-odd rule -- `eda_drc::pad_shape::pad_outline`). The pad
// picks and snaps by those, as `PAD::HitTest` asks the effective shape and not the pad's box. Pure: no React, no DOM.

/** Contours of a pad's copper as the state carries them. */
export type Contours = readonly (readonly (readonly [number, number])[])[];

/** The even-odd rule over every contour: a point is on the copper when a ray from it crosses an odd number of edges (a ring's hole is a contour of its own). */
export function outlineContains(contours: Contours, x: number, y: number): boolean {
  let inside = false;
  for (const c of contours) {
    for (let i = 0, j = c.length - 1; i < c.length; j = i++) {
      const [xi, yi] = c[i]!;
      const [xj, yj] = c[j]!;
      if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside;
    }
  }
  return inside;
}

function distToEdge(px: number, py: number, ax: number, ay: number, bx: number, by: number): number {
  const dx = bx - ax;
  const dy = by - ay;
  const len2 = dx * dx + dy * dy;
  const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((px - ax) * dx + (py - ay) * dy) / len2));
  return Math.hypot(px - (ax + t * dx), py - (ay + t * dy));
}

/** How far (um) the point is outside the copper: 0 on it, else the distance to the nearest edge of a contour. */
export function outlineDistance(contours: Contours, x: number, y: number): number {
  if (outlineContains(contours, x, y)) return 0;
  let best = Infinity;
  for (const c of contours) {
    for (let i = 0, j = c.length - 1; i < c.length; j = i++) {
      best = Math.min(best, distToEdge(x, y, c[j]![0], c[j]![1], c[i]![0], c[i]![1]));
    }
  }
  return best;
}
