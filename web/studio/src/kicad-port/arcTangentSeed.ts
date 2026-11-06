// Port of KiCad 25ec2335's tangent seeds (include/tool/arc_tangent_seed.h, common/tool/arc_tangent_seed.cpp): the start point and departure direction
// `ARC_DRAW_MODE::TANGENT` begins an arc with -- from the end of the arc just drawn (`ArcTangentSeedNear`, chaining) or from the graphic a first click is on
// (`ArcTangentSeedAt`: a segment, arc, bezier or polygon) -- UI-LEFT-2 SS3.
//
// Pure. Internal units (nm).
import { arcCenter, segNearestPoint } from "./pcbGeom";
import { bezierPolyline } from "./bezierPoly";

type P = [number, number];
type V2 = readonly [number, number];

/** `ARC_SEED_PICK_PIXELS`: the screen distance within which a click picks a graphic to start a tangent arc from. */
export const ARC_SEED_PICK_PIXELS = 5;

/** `ARC_TANGENT_SEED`; `directionIsAxis`: only the tangent line is known, not its sign. */
export interface ArcTangentSeed {
  start: P;
  direction: P;
  directionIsAxis: boolean;
}

/** A graphic a seed can start from, as `EDA_SHAPE` has it. */
export type SeedShape =
  | { kind: "segment"; start: V2; end: V2 }
  | { kind: "arc"; start: V2; mid: V2; end: V2 }
  | { kind: "bezier"; start: V2; c1: V2; c2: V2; end: V2 }
  | { kind: "polygon"; pts: readonly V2[]; closed: boolean };

function normalized(v: V2): P | null {
  const l = Math.hypot(v[0], v[1]);
  return l === 0 ? null : [v[0] / l, v[1] / l];
}

const dist = (a: V2, b: V2) => Math.hypot(a[0] - b[0], a[1] - b[1]);
const same = (a: V2, b: V2) => a[0] === b[0] && a[1] === b[1];

/** `nearestEnd`: whichever of the two ends lies nearer `p` (the first on a tie), or null when both are farther than `tolerance`. */
function nearestEnd(first: V2, last: V2, p: V2, tolerance?: number): V2 | null {
  const fd = dist(first, p);
  const ld = dist(last, p);
  const useFirst = fd <= ld;
  if (tolerance !== undefined && (useFirst ? fd : ld) > tolerance) return null;
  return useFirst ? first : last;
}

/** `VECTOR2D::Perpendicular`: `( -y, x )`. */
const perp = (v: V2): P => [-v[1], v[0]];

/** `tangentAtEndpoint`: the outward direction at a segment's or an arc's end `p` (null for another point or shape). */
function tangentAtEndpoint(s: SeedShape, p: V2): P | null {
  if (s.kind !== "segment" && s.kind !== "arc") return null;
  const atStart = same(p, s.start);
  if (!atStart && !same(p, s.end)) return null;
  if (s.kind === "segment") return normalized(atStart ? [s.start[0] - s.end[0], s.start[1] - s.end[1]] : [s.end[0] - s.start[0], s.end[1] - s.start[1]]);
  // "The turn of start->mid->end gives the sweep direction whatever the stored start/end order"
  const turn = (s.mid[0] - s.start[0]) * (s.end[1] - s.mid[1]) - (s.mid[1] - s.start[1]) * (s.end[0] - s.mid[0]);
  if (turn === 0) return null;
  const c = arcCenter({ start: [s.start[0], s.start[1]], mid: [s.mid[0], s.mid[1]], end: [s.end[0], s.end[1]] });
  // "Tangent along increasing polar angle is the radius rotated by a quarter turn"
  let t = perp([p[0] - c[0], p[1] - c[1]]);
  // "Outward is the direction of travel at the end and against it at the start"
  if (turn > 0 === atStart) t = [-t[0], -t[1]];
  return normalized(t);
}

/** `ArcTangentSeedNear`: the seed of the next arc of a chain, from whichever end of `s` lies nearest `near` (the last click on the arc just drawn). */
export function arcTangentSeedNear(s: SeedShape, near: V2): ArcTangentSeed | null {
  if (s.kind !== "segment" && s.kind !== "arc") return null;
  const end = nearestEnd(s.start, s.end, near)!;
  const d = tangentAtEndpoint(s, end);
  return d ? { start: [end[0], end[1]], direction: d, directionIsAxis: false } : null;
}

/** `polylineSeedAt`: a free end of an open chain within `tolerance` gives its outward direction; else the nearest side within `tolerance` gives its axis there. */
function polylineSeedAt(vertices: readonly V2[], closed: boolean, p: V2, tolerance: number): ArcTangentSeed | null {
  const n = vertices.length;
  if (n < 2) return null;
  // "A closed outline has no free ends"
  if (!closed) {
    const end = nearestEnd(vertices[0]!, vertices[n - 1]!, p, tolerance);
    if (end) {
      const inner = end === vertices[0] ? vertices[1]! : vertices[n - 2]!;
      const d = normalized([end[0] - inner[0], end[1] - inner[1]]);
      return d ? { start: [end[0], end[1]], direction: d, directionIsAxis: false } : null;
    }
  }
  const edges = closed ? n : n - 1;
  let best: { a: V2; b: V2 } | null = null;
  let bestDist = tolerance;
  for (let i = 0; i < edges; i++) {
    const a = vertices[i]!;
    const b = vertices[(i + 1) % n]!;
    const q = segNearestPoint({ a: [a[0], a[1]], b: [b[0], b[1]] }, p);
    const d = dist(q, p);
    if (d <= bestDist) {
      best = { a, b };
      bestDist = d;
    }
  }
  if (!best) return null;
  const axis = normalized([best.b[0] - best.a[0], best.b[1] - best.a[1]]);
  if (!axis) return null;
  const q = segNearestPoint({ a: [best.a[0], best.a[1]], b: [best.b[0], best.b[1]] }, p);
  return { start: [Math.round(q[0]), Math.round(q[1])], direction: axis, directionIsAxis: true };
}

/** `SHAPE_ARC::NearestPoint`: on the circle where `p` is within the sweep, else the nearer end. */
function arcNearestPoint(s: Extract<SeedShape, { kind: "arc" }>, p: V2): { point: P; center: P } {
  const c = arcCenter({ start: [s.start[0], s.start[1]], mid: [s.mid[0], s.mid[1]], end: [s.end[0], s.end[1]] });
  const r = dist(c, s.start);
  const ang = (q: V2) => Math.atan2(q[1] - c[1], q[0] - c[0]);
  const turn = (s.mid[0] - s.start[0]) * (s.end[1] - s.mid[1]) - (s.mid[1] - s.start[1]) * (s.end[0] - s.mid[0]);
  const ccw = turn > 0; // +y down: a positive turn sweeps toward increasing atan2
  const norm = (a: number) => ((a % (2 * Math.PI)) + 2 * Math.PI) % (2 * Math.PI);
  const sweep = norm(ccw ? ang(s.end) - ang(s.start) : ang(s.start) - ang(s.end));
  const at = norm(ccw ? ang(p) - ang(s.start) : ang(s.start) - ang(p));
  if (at <= sweep && !(p[0] === c[0] && p[1] === c[1])) {
    const a = ang(p);
    return { point: [Math.round(c[0] + r * Math.cos(a)), Math.round(c[1] + r * Math.sin(a))], center: c };
  }
  const end = dist(s.start, p) <= dist(s.end, p) ? s.start : s.end;
  return { point: [end[0], end[1]], center: c };
}

/**
 * `ArcTangentSeedAt`: the seed for a point on or near a segment, arc, bezier or polygon -- an end within `tolerance` gives the outward direction there and starts at the end;
 * any other point within `tolerance` of the shape gives the tangent axis at the nearest point, and starts there. Null for a point not close enough.
 */
export function arcTangentSeedAt(s: SeedShape, p: V2, tolerance: number): ArcTangentSeed | null {
  switch (s.kind) {
    case "segment":
      return polylineSeedAt([s.start, s.end], false, p, tolerance);
    case "bezier": {
      // "The curve lies inside its control polygon, so a far point skips the costly flattening"
      const xs = [s.start[0], s.c1[0], s.c2[0], s.end[0]];
      const ys = [s.start[1], s.c1[1], s.c2[1], s.end[1]];
      if (p[0] < Math.min(...xs) - tolerance || p[0] > Math.max(...xs) + tolerance || p[1] < Math.min(...ys) - tolerance || p[1] > Math.max(...ys) + tolerance) return null;
      const poly = bezierPolyline(s.start, s.c1, s.c2, s.end, Math.max(1, Math.trunc(tolerance / 10)));
      const seed = polylineSeedAt(poly, false, p, tolerance);
      // "At an endpoint the control polygon gives the exact tangent"
      if (seed && !seed.directionIsAxis) {
        const atStart = same(seed.start, s.start);
        const end = atStart ? s.start : s.end;
        for (const control of atStart ? [s.c1, s.c2, s.end] : [s.c2, s.c1, s.start]) {
          const d = normalized([end[0] - control[0], end[1] - control[1]]);
          if (d) {
            seed.direction = d;
            break;
          }
        }
      }
      return seed;
    }
    case "arc": {
      const end = nearestEnd(s.start, s.end, p, tolerance);
      if (end) {
        const d = tangentAtEndpoint(s, end);
        return d ? { start: [end[0], end[1]], direction: d, directionIsAxis: false } : null;
      }
      const { point, center } = arcNearestPoint(s, p);
      if (dist(point, p) > tolerance) return null;
      const axis = normalized(perp([point[0] - center[0], point[1] - center[1]]));
      return axis ? { start: point, direction: axis, directionIsAxis: true } : null;
    }
    case "polygon":
      return polylineSeedAt(s.pts, s.closed, p, tolerance);
  }
}
