// The idealised geometry the snap helpers work with: zero-width segments, lines, rays, circles, arcs and boxes, their pairwise intersections, the nearest point
// of one to a point, and the key points of circles and ovals. A port of the parts of libs/kimath that pcb_grid_helper.cpp and grid_helper.cpp stand on, commit
// 8303b2ad:
//
//   geometry/intersection.cpp  INTERSECTION_VISITOR       every pair of INTERSECTABLE_GEOM (SEG, LINE, HALF_LINE, CIRCLE, SHAPE_ARC, BOX2I); a box is its four sides
//   geometry/nearest.cpp       GetNearestPoint            the point of a NEARABLE_GEOM nearest to a point
//   geometry/shape_utils.cpp   GetCircleKeyPoints, BoxToSegs, ClipLineToBox, ClipHalfLineToBox
//   geometry/oval.cpp          GetOvalKeyPoints
//   geometry/seg.cpp, circle.cpp, half_line.cpp, shape_arc.cpp  Intersect / IntersectLine / NearestPoint of each
//
// Units are the caller's (the studio's um): KiCad's integer nanometres and their rounding are not reproduced; the tolerances here are relative to the size
// of what is intersected. Angles are radians, the board's y axis points down, as everywhere in the studio.

export type Pt = readonly [number, number];

/** A segment (`SEG`). */
export interface Seg {
  t: "seg";
  a: Pt;
  b: Pt;
}
/** An infinite line through two points (`LINE`). */
export interface Line {
  t: "line";
  a: Pt;
  b: Pt;
}
/** A ray from `a` through `b` (`HALF_LINE{ start, through }`). */
export interface Half {
  t: "half";
  a: Pt;
  b: Pt;
}
/** A circle (`CIRCLE`). */
export interface Circ {
  t: "circle";
  c: Pt;
  r: number;
}
/** An arc (`SHAPE_ARC`): the circle's centre and radius, the angle it starts at and the signed angle it sweeps (positive: increasing atan2 angle). */
export interface ArcG {
  t: "arc";
  c: Pt;
  r: number;
  a0: number;
  da: number;
}
/** An axis-aligned box (`BOX2I`). */
export interface Box {
  t: "box";
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}
export type Geom = Seg | Line | Half | Circ | ArcG | Box;

const TAU = Math.PI * 2;
/** Relative tolerance for "the same point" and "parallel". */
const EPS = 1e-9;

export const seg = (a: Pt, b: Pt): Seg => ({ t: "seg", a, b });
export const line = (a: Pt, b: Pt): Line => ({ t: "line", a, b });
export const half = (a: Pt, b: Pt): Half => ({ t: "half", a, b });
export const circle = (c: Pt, r: number): Circ => ({ t: "circle", c, r });
export const box = (x0: number, y0: number, x1: number, y1: number): Box => ({ t: "box", x0: Math.min(x0, x1), y0: Math.min(y0, y1), x1: Math.max(x0, x1), y1: Math.max(y0, y1) });

export const dist = (p: Pt, q: Pt): number => Math.hypot(p[0] - q[0], p[1] - q[1]);
export const distSq = (p: Pt, q: Pt): number => (p[0] - q[0]) ** 2 + (p[1] - q[1]) ** 2;
const sub = (p: Pt, q: Pt): Pt => [p[0] - q[0], p[1] - q[1]];
const add = (p: Pt, q: Pt): Pt => [p[0] + q[0], p[1] + q[1]];
const mul = (p: Pt, k: number): Pt => [p[0] * k, p[1] * k];
const dot = (p: Pt, q: Pt): number => p[0] * q[0] + p[1] * q[1];
const cross = (p: Pt, q: Pt): number => p[0] * q[1] - p[1] * q[0];
export const samePoint = (p: Pt, q: Pt, tol = 0): boolean => Math.abs(p[0] - q[0]) <= tol && Math.abs(p[1] - q[1]) <= tol;

/** `KIGEOM::BoxToSegs`: the four sides, starting at the top-left and going clockwise on the screen. */
export function boxToSegs(b: Box): [Seg, Seg, Seg, Seg] {
  return [seg([b.x0, b.y0], [b.x1, b.y0]), seg([b.x1, b.y0], [b.x1, b.y1]), seg([b.x1, b.y1], [b.x0, b.y1]), seg([b.x0, b.y1], [b.x0, b.y0])];
}

const normAngle = (a: number): number => {
  let r = a % TAU;
  if (r < 0) r += TAU;
  return r;
};

/** The arc through three points (`SHAPE_ARC( start, mid, end, 0 )`); null for three points on a line. */
export function arcThrough(start: Pt, mid: Pt, end: Pt): ArcG | null {
  const d = 2 * (start[0] * (mid[1] - end[1]) + mid[0] * (end[1] - start[1]) + end[0] * (start[1] - mid[1]));
  if (Math.abs(d) < 1e-12) return null;
  const s2 = start[0] ** 2 + start[1] ** 2;
  const m2 = mid[0] ** 2 + mid[1] ** 2;
  const e2 = end[0] ** 2 + end[1] ** 2;
  const c: Pt = [(s2 * (mid[1] - end[1]) + m2 * (end[1] - start[1]) + e2 * (start[1] - mid[1])) / d, (s2 * (end[0] - mid[0]) + m2 * (start[0] - end[0]) + e2 * (mid[0] - start[0])) / d];
  const r = dist(c, start);
  const a0 = Math.atan2(start[1] - c[1], start[0] - c[0]);
  const aMid = Math.atan2(mid[1] - c[1], mid[0] - c[0]);
  const aEnd = Math.atan2(end[1] - c[1], end[0] - c[0]);
  // The sweep that passes through `mid`: counter-clockwise (positive) when mid is reached before end going that way.
  const toMid = normAngle(aMid - a0);
  const toEnd = normAngle(aEnd - a0);
  const da = toMid <= toEnd ? toEnd : toEnd - TAU;
  return { t: "arc", c, r, a0, da };
}

/** The point of the arc at angle `a`. */
export const arcPoint = (g: ArcG, a: number): Pt => [g.c[0] + g.r * Math.cos(a), g.c[1] + g.r * Math.sin(a)];
export const arcStart = (g: ArcG): Pt => arcPoint(g, g.a0);
export const arcEnd = (g: ArcG): Pt => arcPoint(g, g.a0 + g.da);

/** True when the direction `a` (radians) lies within the arc's sweep. */
export function arcContainsAngle(g: ArcG, a: number): boolean {
  const slack = 1e-9;
  if (g.da >= 0) return normAngle(a - g.a0) <= g.da + slack || normAngle(a - g.a0) >= TAU - slack;
  return normAngle(g.a0 - a) <= -g.da + slack || normAngle(g.a0 - a) >= TAU - slack;
}

const arcContains = (g: ArcG, p: Pt): boolean => arcContainsAngle(g, Math.atan2(p[1] - g.c[1], p[0] - g.c[0]));

// ------------------------------------------------------------------------------------------------------------------------------- primitive pieces

/** The parameter t along `p + t * d` where the lines `p + t d` and `q + u e` cross, or null when they are parallel. */
function lineParams(p: Pt, d: Pt, q: Pt, e: Pt): { t: number; u: number } | null {
  const den = cross(d, e);
  const scale = Math.hypot(d[0], d[1]) * Math.hypot(e[0], e[1]);
  if (scale === 0 || Math.abs(den) <= EPS * scale) return null;
  const qp = sub(q, p);
  return { t: cross(qp, e) / den, u: cross(qp, d) / den };
}

/** `SEG::Intersect( other, false, false )`, endpoints included; parallel (even overlapping) segments do not intersect. */
function segSeg(s: Seg, o: Seg): Pt[] {
  const r = lineParams(s.a, sub(s.b, s.a), o.a, sub(o.b, o.a));
  if (!r) return [];
  const slack = 1e-9;
  if (r.t < -slack || r.t > 1 + slack || r.u < -slack || r.u > 1 + slack) return [];
  return [add(s.a, mul(sub(s.b, s.a), Math.min(1, Math.max(0, r.t))))];
}

/** Where the infinite line through `l` meets the shape given by the parameters of its own points: t along `s`, u along `l`. */
function segLine(s: Seg, l: Line): Pt[] {
  const r = lineParams(s.a, sub(s.b, s.a), l.a, sub(l.b, l.a));
  if (!r || r.t < -1e-9 || r.t > 1 + 1e-9) return [];
  return [add(s.a, mul(sub(s.b, s.a), Math.min(1, Math.max(0, r.t))))];
}

function segHalf(s: Seg, h: Half): Pt[] {
  const r = lineParams(s.a, sub(s.b, s.a), h.a, sub(h.b, h.a));
  if (!r || r.t < -1e-9 || r.t > 1 + 1e-9 || r.u < -1e-9) return [];
  return [add(s.a, mul(sub(s.b, s.a), Math.min(1, Math.max(0, r.t))))];
}

function lineLine(a: Line, b: Line): Pt[] {
  const r = lineParams(a.a, sub(a.b, a.a), b.a, sub(b.b, b.a));
  return r ? [add(a.a, mul(sub(a.b, a.a), r.t))] : [];
}

function lineHalf(l: Line, h: Half): Pt[] {
  const r = lineParams(l.a, sub(l.b, l.a), h.a, sub(h.b, h.a));
  if (!r || r.u < -1e-9) return [];
  return [add(h.a, mul(sub(h.b, h.a), Math.max(0, r.u)))];
}

function halfHalf(h: Half, o: Half): Pt[] {
  const r = lineParams(h.a, sub(h.b, h.a), o.a, sub(o.b, o.a));
  if (!r || r.t < -1e-9 || r.u < -1e-9) return [];
  return [add(h.a, mul(sub(h.b, h.a), Math.max(0, r.t)))];
}

/** The parameters t along `p + t * d` where the infinite line meets the circle (0, 1 or 2 values; a tangent line gives its one touching point). */
function circleLineParams(c: Pt, r: number, p: Pt, d: Pt): number[] {
  const f = sub(p, c);
  const a = dot(d, d);
  if (a === 0) return [];
  const b = 2 * dot(f, d);
  const k = dot(f, f) - r * r;
  let disc = b * b - 4 * a * k;
  const tolerance = EPS * Math.max(1, b * b, 4 * a * Math.abs(k));
  if (disc < -tolerance) return [];
  if (disc < 0) disc = 0;
  const root = Math.sqrt(disc);
  if (root === 0) return [-b / (2 * a)];
  return [(-b - root) / (2 * a), (-b + root) / (2 * a)];
}

function circleLine(c: Circ, a: Pt, b: Pt, min: number, max: number): Pt[] {
  const d = sub(b, a);
  return circleLineParams(c.c, c.r, a, d)
    .filter((t) => t >= min - 1e-9 && t <= max + 1e-9)
    .map((t) => add(a, mul(d, t)));
}

function circleCircle(a: Circ, b: Circ): Pt[] {
  const d = dist(a.c, b.c);
  if (d === 0) return [];
  if (d > a.r + b.r + EPS * Math.max(1, d) || d < Math.abs(a.r - b.r) - EPS * Math.max(1, d)) return [];
  const along = (a.r * a.r - b.r * b.r + d * d) / (2 * d);
  const h2 = a.r * a.r - along * along;
  const dir: Pt = mul(sub(b.c, a.c), 1 / d);
  const mid = add(a.c, mul(dir, along));
  if (h2 <= EPS * Math.max(1, a.r * a.r)) return [mid];
  const h = Math.sqrt(h2);
  const n: Pt = [-dir[1], dir[0]];
  return [add(mid, mul(n, h)), add(mid, mul(n, -h))];
}

// ------------------------------------------------------------------------------------------------------------------------------- the visitor

/** `INTERSECTION_VISITOR`: every point where two pieces of idealised geometry cross. A box is its four sides, so a point on a corner can be listed twice. */
export function intersections(a: Geom, b: Geom): Pt[] {
  if (a.t === "box") return boxToSegs(a).flatMap((s) => intersections(s, b));
  if (b.t === "box") return boxToSegs(b).flatMap((s) => intersections(a, s));
  switch (a.t) {
    case "seg":
      switch (b.t) {
        case "seg":
          return segSeg(a, b);
        case "line":
          return segLine(a, b);
        case "half":
          return segHalf(a, b);
        case "circle":
          return circleLine(b, a.a, a.b, 0, 1);
        case "arc":
          return circleLine(circle(b.c, b.r), a.a, a.b, 0, 1).filter((p) => arcContains(b, p));
      }
      break;
    case "line":
      switch (b.t) {
        case "seg":
          return segLine(b, a);
        case "line":
          return lineLine(a, b);
        case "half":
          return lineHalf(a, b);
        case "circle":
          return circleLine(b, a.a, a.b, -Infinity, Infinity);
        case "arc":
          return circleLine(circle(b.c, b.r), a.a, a.b, -Infinity, Infinity).filter((p) => arcContains(b, p));
      }
      break;
    case "half":
      switch (b.t) {
        case "seg":
          return segHalf(b, a);
        case "line":
          return lineHalf(b, a);
        case "half":
          return halfHalf(a, b);
        case "circle":
          return circleLine(b, a.a, a.b, 0, Infinity);
        case "arc":
          return circleLine(circle(b.c, b.r), a.a, a.b, 0, Infinity).filter((p) => arcContains(b, p));
      }
      break;
    case "circle":
      switch (b.t) {
        case "seg":
          return circleLine(a, b.a, b.b, 0, 1);
        case "line":
          return circleLine(a, b.a, b.b, -Infinity, Infinity);
        case "half":
          return circleLine(a, b.a, b.b, 0, Infinity);
        case "circle":
          return circleCircle(a, b);
        case "arc":
          return circleCircle(a, circle(b.c, b.r)).filter((p) => arcContains(b, p));
      }
      break;
    case "arc":
      switch (b.t) {
        case "seg":
        case "line":
        case "half":
        case "circle":
          return intersections(b, a);
        case "arc":
          return circleCircle(circle(a.c, a.r), circle(b.c, b.r)).filter((p) => arcContains(a, p) && arcContains(b, p));
      }
      break;
  }
  return [];
}

// ------------------------------------------------------------------------------------------------------------------------------- nearest points

function nearestOnSeg(a: Pt, b: Pt, p: Pt, min: number, max: number): Pt {
  const d = sub(b, a);
  const len2 = dot(d, d);
  if (len2 === 0) return a;
  const t = Math.min(max, Math.max(min, dot(sub(p, a), d) / len2));
  return add(a, mul(d, t));
}

/** `GetNearestPoint( NEARABLE_GEOM, point )`. */
export function nearestPoint(g: Geom, p: Pt): Pt {
  switch (g.t) {
    case "seg":
      return nearestOnSeg(g.a, g.b, p, 0, 1);
    case "line":
      return nearestOnSeg(g.a, g.b, p, -Infinity, Infinity);
    case "half":
      return nearestOnSeg(g.a, g.b, p, 0, Infinity);
    case "circle": {
      const d = sub(p, g.c);
      const len = Math.hypot(d[0], d[1]);
      if (len === 0) return [g.c[0] + g.r, g.c[1]];
      return add(g.c, mul(d, g.r / len));
    }
    case "arc": {
      const a = Math.atan2(p[1] - g.c[1], p[0] - g.c[0]);
      if (arcContainsAngle(g, a) && !(p[0] === g.c[0] && p[1] === g.c[1])) return arcPoint(g, a);
      const s = arcStart(g);
      const e = arcEnd(g);
      return distSq(s, p) <= distSq(e, p) ? s : e;
    }
    case "box": {
      let best: Pt = [g.x0, g.y0];
      let bestD = Infinity;
      for (const s of boxToSegs(g)) {
        const n = nearestOnSeg(s.a, s.b, p, 0, 1);
        const d = distSq(n, p);
        if (d <= bestD) {
          bestD = d;
          best = n;
        }
      }
      return best;
    }
  }
}

/** `SHAPE_LINE_CHAIN::NearestPoint( p )`: the point of the polyline (closed or not) nearest to `p`. */
export function polylineNearest(pts: readonly Pt[], closed: boolean, p: Pt): Pt {
  let best: Pt = pts[0] ?? p;
  let bestD = Infinity;
  const n = pts.length;
  const last = closed && n > 2 ? n : n - 1;
  for (let i = 0; i < last; i++) {
    const q = nearestOnSeg(pts[i]!, pts[(i + 1) % n]!, p, 0, 1);
    const d = distSq(q, p);
    if (d < bestD) {
      bestD = d;
      best = q;
    }
  }
  return best;
}

/** The point of the nearest piece of geometry to `p`, or null for none (`GetNearestPoint( std::vector<NEARABLE_GEOM> )`). */
export function nearestOfAny(geoms: readonly Geom[], p: Pt): Pt | null {
  let best: Pt | null = null;
  let bestD = Infinity;
  for (const g of geoms) {
    const n = nearestPoint(g, p);
    const d = distSq(n, p);
    if (best === null || d < bestD) {
      best = n;
      bestD = d;
    }
  }
  return best;
}

/** The distance from `p` to the nearest point of the geometry (`FindSquareDistanceToItem`, unsquared). */
export const distanceTo = (g: Geom, p: Pt): number => dist(nearestPoint(g, p), p);

// ------------------------------------------------------------------------------------------------------------------------------- clipping

/** `KIGEOM::ClipLineToBox`: the part of the infinite line inside the box (Liang-Barsky); null if it misses. */
export function clipLineToBox(l: { a: Pt; b: Pt }, b: Box, ray = false): [Pt, Pt] | null {
  const d = sub(l.b, l.a);
  let t0 = ray ? 0 : -Infinity;
  let t1 = Infinity;
  const clip = (p: number, q: number): boolean => {
    if (p === 0) return q >= 0;
    const r = q / p;
    if (p < 0) {
      if (r > t1) return false;
      if (r > t0) t0 = r;
    } else {
      if (r < t0) return false;
      if (r < t1) t1 = r;
    }
    return true;
  };
  if (!clip(-d[0], l.a[0] - b.x0) || !clip(d[0], b.x1 - l.a[0]) || !clip(-d[1], l.a[1] - b.y0) || !clip(d[1], b.y1 - l.a[1])) return null;
  if (!Number.isFinite(t0) || !Number.isFinite(t1)) return null;
  return [add(l.a, mul(d, t0)), add(l.a, mul(d, t1))];
}

/** `KIGEOM::ClipHalfLineToBox`. */
export const clipHalfLineToBox = (h: Half, b: Box): [Pt, Pt] | null => clipLineToBox(h, b, true);

// ------------------------------------------------------------------------------------------------------------------------------- key points

/** `POINT_TYPE` (include/geometry/point_types.h): what a point means in pure geometric terms. Combine with `|`. */
export const PT = { NONE: 0, CENTER: 1, END: 2, MID: 4, QUADRANT: 8, CORNER: 16, INTERSECTION: 32, ON_ELEMENT: 64 } as const;

export interface TypedPt {
  pt: Pt;
  types: number;
}

/** `KIGEOM::GetCircleKeyPoints`: the four quadrant points and, when asked, the centre. */
export function circleKeyPoints(c: Pt, r: number, includeCenter: boolean): TypedPt[] {
  const out: TypedPt[] = [];
  if (includeCenter) out.push({ pt: c, types: PT.CENTER });
  for (const [x, y] of [
    [0, r],
    [r, 0],
    [0, -r],
    [-r, 0],
  ] as const)
    out.push({ pt: [c[0] + x, c[1] + y], types: PT.QUADRANT });
  return out;
}

/** `KIGEOM::OVAL_KEY_POINT_FLAGS`. */
export const OVAL = { CENTER: 1, CAP_CENTERS: 2, CAP_TIPS: 4, SIDE_ENDS: 8, SIDE_MIDPOINTS: 16, CARDINAL_EXTREMES: 32 } as const;

/** KiCad's `RotatePoint( p, angle )`: counter-clockwise on the screen (y down), i.e. x' = x cos + y sin, y' = y cos - x sin. */
const rotatePoint = (p: Pt, a: number): Pt => [p[0] * Math.cos(a) + p[1] * Math.sin(a), p[1] * Math.cos(a) - p[0] * Math.sin(a)];
const isCardinal = (a: number): boolean => Math.abs(Math.sin(2 * a)) < 1e-9;

/** `EDA_ANGLE::Normalize90`: into -90 .. +90 degrees. */
function normalize90(a: number): number {
  let r = a;
  while (r < -Math.PI / 2 - 1e-12) r += Math.PI;
  while (r > Math.PI / 2 + 1e-12) r -= Math.PI;
  return r;
}

/**
 * `KIGEOM::GetOvalKeyPoints` of the stadium of `width` x `height` at `center`, turned by `orientation` (radians, KiCad's sense: counter-clockwise on the screen): the
 * long axis is the longer side. The C++ takes a `SHAPE_SEGMENT` whose angle (`EDA_ANGLE( B - A )`) runs along the long axis; `BySizeAndCenter` builds it that way.
 */
export function ovalKeyPoints(center: Pt, width: number, height: number, orientation: number, flags: number): TypedPt[] {
  const halfWidth = Math.min(width, height) / 2;
  const halfLen = Math.max(width, height) / 2;
  // The segment is along x for a wide oval and along y for a tall one, then turned by `RotatePoint( segVec, orientation )`; its angle is that vector's atan2.
  const segAngle = (width > height ? 0 : Math.PI / 2) - orientation;
  const rotation = segAngle - Math.PI / 2;
  const cardinal = isCardinal(rotation);
  // Points on a non-rotated oval at the origin, long axis along y.
  const pts: TypedPt[] = [];
  if (flags & OVAL.CENTER) pts.push({ pt: [0, 0], types: PT.CENTER });
  if (flags & OVAL.SIDE_MIDPOINTS) {
    pts.push({ pt: [halfWidth, 0], types: PT.MID });
    pts.push({ pt: [-halfWidth, 0], types: PT.MID });
  }
  if (flags & OVAL.CAP_TIPS) {
    // Square-on, the tips are quadrants.
    const type = cardinal ? PT.QUADRANT : PT.END;
    pts.push({ pt: [0, halfLen], types: type });
    pts.push({ pt: [0, -halfLen], types: type });
  }
  const capCentre = halfLen - halfWidth;
  if (flags & OVAL.CAP_CENTERS) {
    pts.push({ pt: [0, capCentre], types: PT.CENTER });
    pts.push({ pt: [0, -capCentre], types: PT.CENTER });
  }
  if (flags & OVAL.SIDE_ENDS) {
    for (const [x, y] of [
      [halfWidth, capCentre],
      [halfWidth, -capCentre],
      [-halfWidth, capCentre],
      [-halfWidth, -capCentre],
    ] as const)
      pts.push({ pt: [x, y], types: PT.END });
  }
  // The quadrant points of the caps only exist when the oval is turned (square-on they are the tips).
  if (flags & OVAL.CARDINAL_EXTREMES && !cardinal) {
    const radial: Pt = [0, halfWidth];
    let lineRotation = normalize90(rotation);
    const toX = rotatePoint(radial, lineRotation);
    lineRotation = normalize90(lineRotation - Math.PI / 2);
    const toY = rotatePoint(radial, lineRotation);
    pts.push({ pt: [toY[0], capCentre + toY[1]], types: PT.QUADRANT });
    pts.push({ pt: [toX[0], capCentre + toX[1]], types: PT.QUADRANT });
    pts.push({ pt: [-toY[0], -capCentre - toY[1]], types: PT.QUADRANT });
    pts.push({ pt: [-toX[0], -capCentre - toX[1]], types: PT.QUADRANT });
  }
  return pts.map((p) => {
    const turned = rotatePoint(p.pt, -rotation);
    return { pt: [turned[0] + center[0], turned[1] + center[1]] as Pt, types: p.types };
  });
}
