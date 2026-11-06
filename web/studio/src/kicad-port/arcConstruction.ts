// Port of KiCad 25ec2335's arc construction helpers (libs/kimath/include/geometry/arc_construction.h, libs/kimath/src/geometry/arc_construction.cpp): the solvers
// behind the interactive arc drawing modes other than centre-start-end (`ARC_DRAW_MODE::START_END_MID`, `START_END_CENTER`, `TANGENT`, `START_DIR_END`; UI-LEFT-2 SS3).
// "Every solver returns the arc as start/mid/end, the same form the file formats store. The midpoint fixes the sweep direction, so no separate clockwise flag is
// needed. Start and end are always returned exactly as given."
//
// Pure. Coordinates are internal units (nm); `VECTOR2I` results are rounded as `KiROUND` rounds.

export type V2 = readonly [number, number];

/** `KIGEOM::ARC_SOLUTION`: `valid` is false for degenerate input (collinear points, a zero chord ...). */
export interface ArcSolution {
  valid: boolean;
  start: [number, number];
  /** The point on the arc halfway along the sweep. */
  mid: [number, number];
  end: [number, number];
  center: [number, number];
  radius: number;
}

export const INVALID_SOLUTION: ArcSolution = { valid: false, start: [0, 0], mid: [0, 0], end: [0, 0], center: [0, 0], radius: 0 };

/** `KiROUND`: half away from zero. */
function kiRound(v: number): number {
  const r = v < 0 ? -Math.round(-v) : Math.round(v);
  return r === 0 ? 0 : r;
}

/** `crossZ`: the cross product's z (KiCad compensates the rounding with `fma`; doubles here, exact for the integer inputs the tools give). */
function crossZ(ax: number, ay: number, bx: number, by: number): number {
  return ax * by - ay * bx;
}

/** `toIU`: round to internal units, failing when the value does not fit. */
function toIU(x: number, y: number): [number, number] | null {
  const limit = 2_147_483_647 - 1;
  if (!Number.isFinite(x) || !Number.isFinite(y) || Math.abs(x) > limit || Math.abs(y) > limit) return null;
  return [kiRound(x), kiRound(y)];
}

/** `buildOnChord`: `side` is +1 for an arc on the (-dy, dx) side of start to end, `centerT` the centre's offset along that normal. */
function buildOnChord(start: V2, end: V2, side: number, centerT: number, radius: number): ArcSolution {
  const sol: ArcSolution = { ...INVALID_SOLUTION, start: [start[0], start[1]], end: [end[0], end[1]] };
  const cx = end[0] - start[0];
  const cy = end[1] - start[1];
  const len = Math.hypot(cx, cy);
  if (len <= 0 || !Number.isFinite(radius) || radius <= 0) return sol;
  const nx = -cy / len;
  const ny = cx / len;
  const mx = (start[0] + end[0]) * 0.5;
  const my = (start[1] + end[1]) * 0.5;
  const half = len * 0.5;
  const offset = Math.abs(centerT);
  // "radius - offset cancels for shallow arcs, so use the equivalent half^2 / ( radius + offset )"
  const height = side * centerT <= 0 ? (half * half) / (radius + offset) : radius + offset;
  const mid = toIU(mx + nx * (side * height), my + ny * (side * height));
  if (!mid) return sol;
  // "An apex within rounding of the chord cannot be stored as an arc"
  if (crossZ(cx, cy, mid[0] - start[0], mid[1] - start[1]) === 0) return sol;
  return { valid: true, start: sol.start, end: sol.end, mid, center: [mx + nx * centerT, my + ny * centerT], radius };
}

/** `ProjectToChordBisector`: `cursor` projected onto the perpendicular bisector of the chord `start` - `end`. */
export function projectToChordBisector(start: V2, end: V2, cursor: V2): [number, number] {
  const cx = end[0] - start[0];
  const cy = end[1] - start[1];
  const len = Math.hypot(cx, cy);
  if (len <= 0) return [cursor[0], cursor[1]];
  const nx = -cy / len;
  const ny = cx / len;
  const mx = (start[0] + end[0]) * 0.5;
  const my = (start[1] + end[1]) * 0.5;
  const d = (cursor[0] - mx) * nx + (cursor[1] - my) * ny;
  return toIU(mx + nx * d, my + ny * d) ?? [cursor[0], cursor[1]];
}

/** `ArcThroughPoints`: the arc from `start` to `end` through `onArc`. */
export function arcThroughPoints(start: V2, onArc: V2, end: V2): ArcSolution {
  const bx = end[0] - start[0];
  const by = end[1] - start[1];
  const cx = onArc[0] - start[0];
  const cy = onArc[1] - start[1];
  const d = crossZ(bx, by, cx, cy);
  if (d === 0) return INVALID_SOLUTION;
  const bb = bx * bx + by * by;
  const cc = cx * cx + cy * cy;
  // "Circumcenter relative to the start point"
  const rx = (cy * bb - by * cc) / (2 * d);
  const ry = (bx * cc - cx * bb) / (2 * d);
  const len = Math.hypot(bx, by);
  // "The signed distance of the center along the chord normal, measured from the chord midpoint"
  const centerT = (-rx * by + ry * bx) / len;
  return buildOnChord(start, end, d > 0 ? 1 : -1, centerT, Math.hypot(rx, ry));
}

/** `ArcFromStartEndMidDrag`: the arc from `start` to `end` whose midpoint is `cursor` projected onto the chord bisector. Invalid when that lands on the chord. */
export function arcFromStartEndMidDrag(start: V2, end: V2, cursor: V2): ArcSolution {
  const projected = projectToChordBisector(start, end, cursor);
  const sol = arcThroughPoints(start, projected, end);
  // "The circle passes through the projected point exactly, so keep it as the midpoint"
  return sol.valid ? { ...sol, mid: projected } : sol;
}

/**
 * `ArcFromStartEndCenterDrag`: the arc from `start` to `end` centred on `cursor` projected onto the chord bisector; `major` false for the minor arc (up to 180 degrees),
 * true for the major one. A centre on the chord gives a semicircle either way, the flag picking the side.
 */
export function arcFromStartEndCenterDrag(start: V2, end: V2, cursor: V2, major: boolean): ArcSolution {
  const cx = end[0] - start[0];
  const cy = end[1] - start[1];
  const len = Math.hypot(cx, cy);
  if (len <= 0) return INVALID_SOLUTION;
  const nx = -cy / len;
  const ny = cx / len;
  const mx = (start[0] + end[0]) * 0.5;
  const my = (start[1] + end[1]) * 0.5;
  const centerT = (cursor[0] - mx) * nx + (cursor[1] - my) * ny;
  // "The minor arc bulges away from the center, the major arc toward it"
  let side = centerT < 0 ? -1 : 1;
  if (!major) side = -side;
  return buildOnChord(start, end, side, centerT, Math.hypot(len * 0.5, centerT));
}

/** `ArcFromStartTangentEnd`: the arc leaving `start` along `tangent` and ending at `end`. Invalid when `end` is on the tangent line or `tangent` is zero. */
export function arcFromStartTangentEnd(start: V2, tangent: V2, end: V2): ArcSolution {
  const tl = Math.hypot(tangent[0], tangent[1]);
  if (tl <= 0 || !Number.isFinite(tl)) return INVALID_SOLUTION;
  const tx = tangent[0] / tl;
  const ty = tangent[1] / tl;
  const dx = end[0] - start[0];
  const dy = end[1] - start[1];
  const dd = dx * dx + dy * dy;
  const cross = crossZ(dx, dy, tx, ty);
  if (dd <= 0 || cross === 0) return INVALID_SOLUTION;
  // "The center sits on the normal to the tangent at the start, equidistant from start and end"
  const k = -dd / (2 * cross);
  const nx = -ty;
  const ny = tx;
  const len = Math.sqrt(dd);
  const cnx = -dy / len;
  const cny = dx / len;
  return buildOnChord(start, end, cross > 0 ? 1 : -1, k * (nx * cnx + ny * cny), Math.abs(k));
}
