// The polygon arithmetic of `GuessSelectionCandidates`' coverage passes (pcbnew/tools/pcb_selection_tool.cpp at 8303b2ad), UI-LEFT P22:
//
//   FOOTPRINT::GetCoverageArea( aItem, aCollector )   the area an item is compared by ("If the user clicked on a small item within a much larger one ...")
//   FOOTPRINT::CoverageRatio( aCollector )            how much of a footprint's bounding hull its own pads and texts and the other items under the click cover;
//                                                     a footprint more than 70 % covered is kept for the clarification menu (`maxCoverRatio = 0.70`)
//   FOOTPRINT::GetBoundingHull()                      the convex hull (`BuildConvexHull`, Andrew's monotone chain) of its drawings, pads, holes and zones
//
// KiCad computes these with Clipper (`SHAPE_POLY_SET::BooleanIntersection`, `Area()`). The browser has no polygon clipper, so the one quantity they need -- the
// area of the region inside a union of polygons and inside another (non-zero rule, as `SHAPE_POLY_SET`'s booleans fill) -- is computed here exactly by a
// sweep: between two successive y's at which a vertex lies or two edges cross, every edge is a straight piece across the slab, so the covered length is
// linear in y and the slab's area is its height times the covered length at its middle. Curves are polygons, as KiCad's are: a circle or an arc is cut
// into chords no further than `ARC_LOW_DEF` (0.02 mm) from it, on the outside (`ERROR_OUTSIDE`).
//
// Pure: no store, no DOM. Unit-tested in coverage.test.ts.

export type Pt = readonly [number, number];
export type Ring = readonly Pt[];

/** `ARC_LOW_DEF = pcbIUScale.mmToIU( ARC_LOW_DEF_MM )`, `ARC_LOW_DEF_MM = 0.02`. */
export const ARC_LOW_DEF_NM = 20_000;

/** `GuessSelectionCandidates`' `maxCoverRatio`: "if a footprint is completely covered with other features then there's no way to select it". */
export const MAX_COVER_RATIO = 0.7;

/** The (absolute) area of one ring, by the shoelace formula. */
export function ringArea(ring: Ring): number {
  if (ring.length < 3) return 0;
  let sum = 0;
  for (let i = 0; i < ring.length; i++) {
    const [x0, y0] = ring[i]!;
    const [x1, y1] = ring[(i + 1) % ring.length]!;
    sum += x0 * y1 - x1 * y0;
  }
  return Math.abs(sum) / 2;
}

interface Edge {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
  /** +1 going down (y grows), -1 going up: the winding it adds to the left of it. */
  dir: number;
  set: 0 | 1;
}

/** Twice the signed area of a ring (positive one way round, negative the other). */
function signedArea2(ring: Ring): number {
  let sum = 0;
  for (let i = 0; i < ring.length; i++) {
    const [x0, y0] = ring[i]!;
    const [x1, y1] = ring[(i + 1) % ring.length]!;
    sum += x0 * y1 - x1 * y0;
  }
  return sum;
}

/**
 * The edges of `rings`, each ring taken as a filled outline whichever way round it was given: KiCad's `TransformShapeToPolygon` outlines all run the same way, so
 * their non-zero union is the union of what they cover; a ring here may come from the board's own points, so it is turned to run that way too. (No ring
 * here is a hole: an unfilled shape is its stroke's outline.)
 */
function edgesOf(rings: readonly Ring[], set: 0 | 1, out: Edge[]): void {
  for (const ring of rings) {
    const n = ring.length;
    if (n < 3) continue;
    const turn = signedArea2(ring) < 0 ? -1 : 1;
    for (let i = 0; i < n; i++) {
      const [ax, ay] = ring[i]!;
      const [bx, by] = ring[(i + 1) % n]!;
      if (ay === by) continue;
      out.push(ay < by ? { x0: ax, y0: ay, x1: bx, y1: by, dir: turn, set } : { x0: bx, y0: by, x1: ax, y1: ay, dir: -turn, set });
    }
  }
}

const xAt = (e: Edge, y: number): number => e.x0 + ((e.x1 - e.x0) * (y - e.y0)) / (e.y1 - e.y0);

/** The x intervals where the non-zero winding of `crossings` (x, dir) is not zero. */
function intervals(crossings: [number, number][]): [number, number][] {
  crossings.sort((a, b) => a[0] - b[0]);
  const out: [number, number][] = [];
  let winding = 0;
  let start = 0;
  for (const [x, dir] of crossings) {
    const was = winding;
    winding += dir;
    if (was === 0 && winding !== 0) start = x;
    else if (was !== 0 && winding === 0 && x > start) out.push([start, x]);
  }
  return out;
}

function overlapLength(a: [number, number][], b: [number, number][]): number {
  let i = 0;
  let j = 0;
  let len = 0;
  while (i < a.length && j < b.length) {
    const lo = Math.max(a[i]![0], b[j]![0]);
    const hi = Math.min(a[i]![1], b[j]![1]);
    if (hi > lo) len += hi - lo;
    if (a[i]![1] < b[j]![1]) i++;
    else j++;
  }
  return len;
}

/**
 * The area inside (non-zero) one of `subjects` and inside one of `clips`: `SHAPE_POLY_SET coveredRegion; coveredRegion.BooleanIntersection( clip ); Area()`.
 * With `clips` absent, the area of the union of `subjects`.
 */
export function intersectionArea(subjects: readonly Ring[], clips?: readonly Ring[]): number {
  const edges: Edge[] = [];
  edgesOf(subjects, 0, edges);
  const clipped = clips !== undefined;
  if (clipped) edgesOf(clips, 1, edges);
  if (edges.length === 0) return 0;
  const ys = new Set<number>();
  for (const e of edges) {
    ys.add(e.y0);
    ys.add(e.y1);
  }
  // where two edges cross inside both their spans
  const byY = [...edges].sort((a, b) => a.y0 - b.y0);
  for (let i = 0; i < byY.length; i++) {
    const a = byY[i]!;
    for (let j = i + 1; j < byY.length; j++) {
      const b = byY[j]!;
      if (b.y0 >= a.y1) break;
      const lo = Math.max(a.y0, b.y0);
      const hi = Math.min(a.y1, b.y1);
      if (hi <= lo) continue;
      const d0 = xAt(a, lo) - xAt(b, lo);
      const d1 = xAt(a, hi) - xAt(b, hi);
      if ((d0 < 0 && d1 > 0) || (d0 > 0 && d1 < 0)) ys.add(lo + ((hi - lo) * d0) / (d0 - d1));
    }
  }
  const levels = [...ys].sort((a, b) => a - b);
  let area = 0;
  for (let k = 0; k + 1 < levels.length; k++) {
    const ya = levels[k]!;
    const yb = levels[k + 1]!;
    if (yb <= ya) continue;
    const ym = (ya + yb) / 2;
    const subj: [number, number][] = [];
    const clip: [number, number][] = [];
    for (const e of edges) {
      if (e.y0 <= ym && e.y1 > ym) (e.set === 0 ? subj : clip).push([xAt(e, ym), e.dir]);
    }
    const s = intervals(subj);
    if (s.length === 0) continue;
    const len = clipped ? overlapLength(s, intervals(clip)) : s.reduce((acc, [a, b]) => acc + (b - a), 0);
    area += len * (yb - ya);
  }
  return area;
}

/** `GetArcToSegmentCount( aRadius, aErrorMax, FULL_CIRCLE )`: the chords of a full circle that keep within `error` of it (at least 8). */
export function circleSegments(radius: number, error = ARC_LOW_DEF_NM): number {
  if (radius <= error) return 8;
  const n = Math.ceil(Math.PI / Math.acos(1 - error / radius));
  return Math.max(8, n);
}

/** A circle as a polygon, its chords outside the circle (`ERROR_OUTSIDE`: the polygon's corners on a slightly larger circle). */
export function circleRing(cx: number, cy: number, radius: number, error = ARC_LOW_DEF_NM): Ring {
  if (radius <= 0) return [];
  const n = circleSegments(radius, error);
  const r = radius / Math.cos(Math.PI / n);
  const out: Pt[] = [];
  for (let i = 0; i < n; i++) {
    const a = (2 * Math.PI * i) / n;
    out.push([cx + r * Math.cos(a), cy + r * Math.sin(a)]);
  }
  return out;
}

/** A `w` x `h` rectangle about (cx, cy), turned by `deg` (clockwise on the board's y-down screen, as a pad's rotation is). */
export function rectRing(cx: number, cy: number, w: number, h: number, deg = 0): Ring {
  const a = (deg * Math.PI) / 180;
  const c = Math.cos(a);
  const s = Math.sin(a);
  const corners: Pt[] = [
    [-w / 2, -h / 2],
    [w / 2, -h / 2],
    [w / 2, h / 2],
    [-w / 2, h / 2],
  ];
  return corners.map(([x, y]) => [cx + x * c - y * s, cy + x * s + y * c] as Pt);
}

/** A segment of `width` with round ends (`TransformOvalToPolygon`): the two sides and a half circle at each end. */
export function stadiumRing(a: Pt, b: Pt, width: number, error = ARC_LOW_DEF_NM, inside = false): Ring {
  const r = width / 2;
  if (r <= 0) return [];
  const len = Math.hypot(b[0] - a[0], b[1] - a[1]);
  if (len === 0 && !inside) return circleRing(a[0], a[1], r, error);
  const ang = Math.atan2(b[1] - a[1], b[0] - a[0]);
  const n = Math.max(4, Math.ceil(circleSegments(r, error) / 2));
  // `inside` (`ERROR_INSIDE`): every vertex on the arc, the chords inside it; else (`ERROR_OUTSIDE`) the corners between are outside it
  const rr = inside ? r : r / Math.cos(Math.PI / (2 * n));
  const out: Pt[] = [];
  // the end at b, from one side round to the other, then the end at a; the straight sides at the half width, the corners between outside the arc
  for (let i = 0; i <= n; i++) {
    const t = ang - Math.PI / 2 + (Math.PI * i) / n;
    const d = i === 0 || i === n ? r : rr;
    out.push([b[0] + d * Math.cos(t), b[1] + d * Math.sin(t)]);
  }
  for (let i = 0; i <= n; i++) {
    const t = ang + Math.PI / 2 + (Math.PI * i) / n;
    const d = i === 0 || i === n ? r : rr;
    out.push([a[0] + d * Math.cos(t), a[1] + d * Math.sin(t)]);
  }
  return out;
}

/** A polyline of `width` as one stadium per piece. */
export function polylineRings(pts: readonly Pt[], width: number): Ring[] {
  const out: Ring[] = [];
  for (let i = 0; i + 1 < pts.length; i++) out.push(stadiumRing(pts[i]!, pts[i + 1]!, width));
  if (pts.length === 1) out.push(circleRing(pts[0]![0], pts[0]![1], width / 2));
  return out;
}

/** `BuildConvexHull`: Andrew's monotone chain over the points sorted by x then y, the last point (the first again) dropped. */
export function convexHull(points: readonly Pt[]): Pt[] {
  if (points.length < 2) return [];
  const poly = [...points].sort((p, q) => p[0] - q[0] || p[1] - q[1]);
  const cross = (o: Pt, a: Pt, b: Pt) => (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
  const out: Pt[] = [];
  for (const p of poly) {
    while (out.length >= 2 && cross(out[out.length - 2]!, out[out.length - 1]!, p) <= 0) out.pop();
    out.push(p);
  }
  const t = out.length + 1;
  for (let i = poly.length - 2; i >= 0; i--) {
    const p = poly[i]!;
    while (out.length >= t && cross(out[out.length - 2]!, out[out.length - 1]!, p) <= 0) out.pop();
    out.push(p);
  }
  if (out.length > 1 && out[0]![0] === out[out.length - 1]![0] && out[0]![1] === out[out.length - 1]![1]) out.pop();
  return out;
}

/**
 * `FOOTPRINT::GetBoundingHull`: the convex hull of the points of the footprint's drawings and pads (and holes); with fewer than three points, "a small dummy
 * rectangular outline around the anchor" of 1 mm each way.
 */
export function boundingHull(rings: readonly Ring[], anchor: Pt): Ring {
  const pts: Pt[] = [];
  for (const r of rings) for (const p of r) pts.push(p);
  if (pts.length < 3) {
    const half = 1_000_000;
    return [
      [anchor[0] - half, anchor[1] - half],
      [anchor[0] + half, anchor[1] - half],
      [anchor[0] + half, anchor[1] + half],
      [anchor[0] - half, anchor[1] + half],
    ];
  }
  return convexHull(pts);
}

/**
 * `FOOTPRINT::CoverageRatio`: the part of the footprint's hull that `covered` (its pads, its texts grown by the collector's accuracy, and the other items under
 * the click -- texts, shapes, tracks, vias, other footprints' hulls) covers, from 0 to 1; a hull of no area counts as covered (1, "this will result in the
 * disambiguate dialog").
 */
export function coverageRatio(hull: Ring, covered: readonly Ring[]): number {
  const footprintArea = ringArea(hull);
  if (footprintArea === 0) return 1;
  const ratio = intersectionArea(covered, [hull]) / footprintArea;
  if (ratio < 0) return 1;
  return Math.min(ratio, 1);
}
