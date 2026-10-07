// The geometry primitives the pcbnew shape-modification routines stand on
// (fillet / chamfer / dogbone / extend lines, heal shapes, outsets, the
// point editor's corner edits), ported from KiCad's libs/kimath at
// 8303b2ad: `SEG` (seg.cpp), `SHAPE_ARC`'s fillet constructor and 3-point
// maths (shape_arc.cpp), `CIRCLE::Intersect` (circle.cpp), `HALF_LINE`
// (half_line.cpp), `KIGEOM::GetSharedEndpoint`/`GetOtherEnd`
// (shape_utils.cpp) and `ComputeChamferPoints` / `ComputeDogbone`
// (corner_operations.cpp).
//
// Units: every function here works in KiCad's own internal unit (IU, 1 nm)
// so KiCad's integer tolerances (`SHAPE::MIN_PRECISION_IU` = 4, the dogbone
// epsilon 8, `SEG::Contains`' squared distance 3) keep their meaning. The
// design IR is in micrometres: callers scale in with `toIU` and back out
// with `fromIU` (one rounding, at the end).
//
// Differences from the C++, all numerical rather than behavioural:
//  * 64-bit integer cross products become doubles (a 1 m board's nm
//    products stay below 2^63 but not 2^53; the relative error is ~1e-16,
//    far under every threshold used here).
//  * `CalcArcCenter`'s slope-based centre is the closed-form circumcentre.

/** `[x, y]` in IU. */
export type V = [number, number];
export interface Seg {
  a: V;
  b: V;
}

/** KiCad's IU is 1 nm; the design IR is 1 um. */
export const IU_PER_UM = 1000;
/** `SHAPE::MIN_PRECISION_IU`. */
export const MIN_PRECISION_IU = 4;

export const toIU = (p: readonly [number, number]): V => [p[0] * IU_PER_UM, p[1] * IU_PER_UM];
export const fromIU = (p: readonly [number, number]): V => [Math.round(p[0] / IU_PER_UM), Math.round(p[1] / IU_PER_UM)];

/** `KiROUND`: round half away from zero. */
export function kiRound(v: number): number {
  return v < 0 ? Math.ceil(v - 0.5) : Math.floor(v + 0.5);
}

/** `rescale( numerator, value, denominator )`: `numerator * value / denominator` rounded to nearest (math/util.cpp). */
export function rescale(numerator: number, value: number, denominator: number): number {
  return kiRound((numerator * value) / denominator);
}

export const eq = (p: readonly [number, number], q: readonly [number, number]): boolean => p[0] === q[0] && p[1] === q[1];
export const vadd = (p: readonly [number, number], q: readonly [number, number]): V => [p[0] + q[0], p[1] + q[1]];
export const vsub = (p: readonly [number, number], q: readonly [number, number]): V => [p[0] - q[0], p[1] - q[1]];
export const vneg = (p: readonly [number, number]): V => [-p[0], -p[1]];
export const dot = (p: readonly [number, number], q: readonly [number, number]): number => p[0] * q[0] + p[1] * q[1];
export const cross = (p: readonly [number, number], q: readonly [number, number]): number => p[0] * q[1] - p[1] * q[0];

/** `VECTOR2I::EuclideanNorm` (vector2d.h): rounded to nearest, exact on the axes, `|x| * sqrt(2)` on a 45 degree line. */
export function norm(v: readonly [number, number]): number {
  const [x, y] = v;
  if (Math.abs(x) === Math.abs(y)) return kiRound(Math.abs(x) * Math.SQRT2);
  if (x === 0) return Math.abs(y);
  if (y === 0) return Math.abs(x);
  return kiRound(Math.hypot(x, y));
}

export function squaredNorm(v: readonly [number, number]): number {
  return v[0] * v[0] + v[1] * v[1];
}

/** `VECTOR2I::Resize` (vector2d.h). */
export function resize(v: readonly [number, number], newLength: number): V {
  const [x, y] = v;
  if (x === 0 && y === 0) return [0, 0];
  let nx: number;
  let ny: number;
  if (Math.abs(x) === Math.abs(y)) {
    nx = ny = Math.abs(newLength) * Math.SQRT1_2;
  } else {
    const xSq = x * x;
    const ySq = y * y;
    const lSq = xSq + ySq;
    const newSq = newLength * newLength;
    nx = Math.sqrt(kiRound((newSq * xSq) / lSq));
    ny = Math.sqrt(kiRound((newSq * ySq) / lSq));
  }
  const s = newLength < 0 ? -1 : newLength > 0 ? 1 : 0;
  const px = x < 0 ? -kiRound(nx) : kiRound(nx);
  const py = y < 0 ? -kiRound(ny) : kiRound(ny);
  return [px * s + 0, py * s + 0];
}

/** `EDA_ANGLE( VECTOR2D )` in degrees: the negative x axis is -180, the zero vector 0. */
export function angleOf(v: readonly [number, number]): number {
  const [x, y] = v;
  if (x === 0 && y === 0) return 0;
  if (y === 0) return x >= 0 ? 0 : -180;
  if (x === 0) return y >= 0 ? 90 : -90;
  return (Math.atan2(y, x) * 180) / Math.PI;
}

/** `EDA_ANGLE::Normalize180`: into (-180, 180]. */
export function normalize180(a: number): number {
  let v = a;
  while (v <= -180) v += 360;
  while (v > 180) v -= 360;
  return v;
}

/** `EDA_ANGLE::Normalize`: into [0, 360). */
export function normalize360(a: number): number {
  let v = a;
  while (v < -0) v += 360;
  while (v >= 360) v -= 360;
  return v;
}

/** `RotatePoint( pt, centre, angle )` (trigo.cpp): positive = counter-clockwise on screen (y grows downward). */
export function rotatePoint(p: readonly [number, number], centre: readonly [number, number], angleDeg: number): V {
  const x = p[0] - centre[0];
  const y = p[1] - centre[1];
  const a = normalize360(angleDeg);
  let rx: number;
  let ry: number;
  if (a === 0) {
    rx = x;
    ry = y;
  } else if (a === 90) {
    rx = y;
    ry = -x;
  } else if (a === 180) {
    rx = -x;
    ry = -y;
  } else if (a === 270) {
    rx = -y;
    ry = x;
  } else {
    const rad = (a * Math.PI) / 180;
    const s = Math.sin(rad);
    const c = Math.cos(rad);
    rx = kiRound(y * s + x * c);
    ry = kiRound(y * c - x * s);
  }
  return [rx + centre[0], ry + centre[1]];
}

// ------------------------------------------------------------------ SEG

export const segLength = (s: Seg): number => norm(vsub(s.b, s.a));

/** `SEG::SquaredDistance( P )`. */
export function segSquaredDistance(s: Seg, p: readonly [number, number]): number {
  return squaredNorm(vsub(segNearestPoint(s, p), p));
}

/** `SEG::Contains`: within a squared distance of 3 (about 1.7 IU). */
export function segContains(s: Seg, p: readonly [number, number]): boolean {
  return segSquaredDistance(s, p) <= 3;
}

/** `SEG::NearestPoint( VECTOR2I )`. */
export function segNearestPoint(s: Seg, p: readonly [number, number]): V {
  const d = vsub(s.b, s.a);
  const lSq = squaredNorm(d);
  if (lSq === 0) return [s.a[0], s.a[1]];
  const pa = vsub(p, s.a);
  const t = dot(d, pa);
  if (t < 0) return [s.a[0], s.a[1]];
  if (t > lSq) return [s.b[0], s.b[1]];
  return [s.a[0] + rescale(t, d[0], lSq), s.a[1] + rescale(t, d[1], lSq)];
}

/** `SEG::LineProject`: the foot of the perpendicular from `p` on the infinite line through the segment. */
export function segLineProject(s: Seg, p: readonly [number, number]): V {
  const d = vsub(s.b, s.a);
  const lSq = squaredNorm(d);
  if (lSq === 0) return [s.a[0], s.a[1]];
  const t = dot(d, vsub(p, s.a));
  return [s.a[0] + rescale(t, d[0], lSq), s.a[1] + rescale(t, d[1], lSq)];
}

/** `SEG::Angle`: the absolute angle between the two lines, in [0, 180] degrees. */
export function segAngle(s: Seg, o: Seg): number {
  const thisAngle = normalize180(angleOf(vsub(s.a, s.b)));
  const otherAngle = normalize180(angleOf(vsub(o.a, o.b)));
  return Math.abs(normalize180(thisAngle - otherAngle));
}

/** `EDA_ANGLE::IsHorizontal` on `SEG::Angle`'s result: parallel (0 or 180 degrees exactly). */
export function segsParallel(s: Seg, o: Seg): boolean {
  const a = segAngle(s, o);
  return a === 0 || a === 180;
}

/** `SEG::checkCollinearOverlap`: do two collinear segments overlap (on the longer axis), and where (the midpoint of the overlap)? */
function checkCollinearOverlap(s: Seg, o: Seg, useXAxis: boolean, ignoreEndpoints: boolean): V | null {
  const i = useXAxis ? 0 : 1;
  const j = useXAxis ? 1 : 0;
  const seg1Start = s.a[i];
  const seg1End = s.b[i];
  const seg2Start = o.a[i];
  const seg2End = o.b[i];
  const coord1Start = s.a[j];
  const coord1End = s.b[j];
  const seg1Min = Math.min(seg1Start, seg1End);
  const seg1Max = Math.max(seg1Start, seg1End);
  const seg2Min = Math.min(seg2Start, seg2End);
  const seg2Max = Math.max(seg2Start, seg2End);
  if (!(seg1Max >= seg2Min && seg2Max >= seg1Min)) return null;
  const overlapStart = Math.max(seg1Min, seg2Min);
  const overlapEnd = Math.min(seg1Max, seg2Max);
  if (ignoreEndpoints && overlapStart === overlapEnd) {
    const touchesA = overlapStart === seg1Min || overlapStart === seg1Max;
    const touchesB = overlapStart === seg2Min || overlapStart === seg2Max;
    if (touchesA && touchesB) return null;
  }
  const proj = Math.trunc((overlapStart + overlapEnd) / 2);
  const other = seg1End !== seg1Start ? coord1Start + rescale(proj - seg1Start, coord1End - coord1Start, seg1End - seg1Start) : coord1Start;
  return useXAxis ? [proj, other] : [other, proj];
}

/** `SEG::intersects`/`Intersect( aSeg, aIgnoreEndpoints, aLines )`: the crossing point, or null. `lines` intersects the infinite lines. */
export function segIntersect(s: Seg, o: Seg, ignoreEndpoints = false, lines = false): V | null {
  if (!lines) {
    if (Math.max(s.a[0], s.b[0]) < Math.min(o.a[0], o.b[0]) || Math.max(o.a[0], o.b[0]) < Math.min(s.a[0], s.b[0])) return null;
    if (Math.max(s.a[1], s.b[1]) < Math.min(o.a[1], o.b[1]) || Math.max(o.a[1], o.b[1]) < Math.min(s.a[1], s.b[1])) return null;
  }
  const dir1 = vsub(s.b, s.a);
  const dir2 = vsub(o.b, o.a);
  const offset = vsub(o.a, s.a);
  const determinant = cross(dir2, dir1);

  if (determinant === 0) {
    if (cross(dir1, offset) !== 0) return null; // parallel, not collinear
    if (lines) {
      if (eq(o.a, o.b)) return [o.a[0], o.a[1]];
      if (eq(s.a, s.b)) return [s.a[0], s.a[1]];
      return [Math.trunc((s.a[0] + o.a[0]) / 2), Math.trunc((s.a[1] + o.a[1]) / 2)];
    }
    return checkCollinearOverlap(s, o, Math.abs(dir1[0]) >= Math.abs(dir1[1]), ignoreEndpoints);
  }

  const param2 = cross(dir2, offset);
  const param1 = cross(dir1, offset);
  if (!lines) {
    if (determinant > 0) {
      if (param1 < 0 || param1 > determinant || param2 < 0 || param2 > determinant) return null;
    } else if (param1 > 0 || param1 < determinant || param2 > 0 || param2 < determinant) {
      return null;
    }
    if (ignoreEndpoints && (param1 === 0 || param1 === determinant) && (param2 === 0 || param2 === determinant)) return null;
  }
  return [o.a[0] + rescale(param1, dir2[0], determinant), o.a[1] + rescale(param1, dir2[1], determinant)];
}

/** `SEG::Intersects`. */
export const segIntersects = (s: Seg, o: Seg): boolean => segIntersect(s, o) !== null;

/** `SEG::IntersectLines` = `Intersect( other, false, true )`. */
export const segIntersectLines = (s: Seg, o: Seg): V | null => segIntersect(s, o, false, true);

/** `SEG::mutualDistanceSquared` + `ApproxCollinear( other, 1 )`. */
export function segApproxCollinear(s: Seg, o: Seg, threshold = 1): boolean {
  const ex = vsub(s.b, s.a);
  const lenSq = squaredNorm(ex);
  if (lenSq === 0) return false;
  const lenSqOther = squaredNorm(vsub(o.b, o.a));
  if (lenSqOther === 0) return false;
  const ca = cross(ex, vsub(o.a, s.a));
  const cb = cross(ex, vsub(o.b, s.a));
  const d1 = (ca * ca) / lenSq;
  const d2 = (cb * cb) / lenSq;
  return Math.abs(d1) <= threshold * threshold && Math.abs(d2) <= threshold * threshold;
}

// -------------------------------------------------------------- helpers

/** `KIGEOM::GetSharedEndpoint( A, B )`. */
export function sharedEndpoint(a: Seg, b: Seg): V | null {
  if (eq(a.a, b.a) || eq(a.a, b.b)) return a.a;
  if (eq(a.b, b.a) || eq(a.b, b.b)) return a.b;
  return null;
}

/** `KIGEOM::GetOtherEnd( seg, point )`. */
export function otherEnd(s: Seg, p: readonly [number, number]): V {
  return eq(s.a, p) ? s.b : s.a;
}

/** `VectorsInSameQuadrant` (half_line.cpp). */
export function vectorsInSameQuadrant(a: readonly [number, number], b: readonly [number, number]): boolean {
  return a[0] >= 0 === b[0] >= 0 && a[1] >= 0 === b[1] >= 0;
}

/** `HALF_LINE::Intersect( SEG )`: the ray from `start` through `through` meets the segment. */
export function halfLineIntersect(start: V, through: V, s: Seg): V | null {
  const hit = segIntersect(s, { a: start, b: through }, false, true);
  if (!hit) return null;
  if (!vectorsInSameQuadrant(vsub(through, start), vsub(hit, start))) return null;
  if (!segContains(s, hit)) return null;
  return hit;
}

/** `KIGEOM::PointsAreInSameDirection( A, B, from )`: within 90 degrees seen from `from`. */
export function pointsAreInSameDirection(pa: V, pb: V, from: V): boolean {
  return dot(vsub(pb, from), vsub(pa, from)) > 0;
}

/** `CIRCLE::IntersectLine` / `CIRCLE::Intersect( SEG )` (circle.cpp). */
export function circleIntersectSeg(center: V, radius: number, s: Seg): V[] {
  const out: V[] = [];
  const m = segLineProject(s, center);
  const omDist = norm(vsub(m, center));
  if (omDist > radius + MIN_PRECISION_IU) return out;
  let hits: V[];
  if (omDist <= radius + MIN_PRECISION_IU && omDist >= radius - MIN_PRECISION_IU) {
    hits = [m];
  } else {
    const mTo1 = Math.trunc(Math.sqrt(radius * radius - omDist * omDist));
    const v1 = resize(vsub(s.b, s.a), mTo1);
    hits = [vadd(v1, m), vadd(vneg(v1), m)];
  }
  for (const h of hits) if (segContains(s, h)) out.push(h);
  return out;
}

// ------------------------------------------------------------ SHAPE_ARC

export interface Arc {
  start: V;
  mid: V;
  end: V;
}

/** `CalcArcCenter( start, mid, end )`: the circumcentre (the centroid when the three points are clustered; the chord midpoint when collinear). */
export function arcCenter(a: Arc): [number, number] {
  const [sx, sy] = a.start;
  const [mx, my] = a.mid;
  const [ex, ey] = a.end;
  const minX = Math.min(sx, mx, ex);
  const maxX = Math.max(sx, mx, ex);
  const minY = Math.min(sy, my, ey);
  const maxY = Math.max(sy, my, ey);
  if (maxX - minX < 5 && maxY - minY < 5) return [(sx + mx + ex) / 3, (sy + my + ey) / 3];
  if (eq(a.start, a.end)) return [(sx + mx) / 2, (sy + my) / 2];
  const d = 2 * (sx * (my - ey) + mx * (ey - sy) + ex * (sy - my));
  if (Math.abs(d) < 1e-9) return [(sx + ex) / 2, (sy + ey) / 2];
  const sa = sx * sx + sy * sy;
  const ma = mx * mx + my * my;
  const ea = ex * ex + ey * ey;
  return [(sa * (my - ey) + ma * (ey - sy) + ea * (sy - my)) / d, (sa * (ex - mx) + ma * (sx - ex) + ea * (mx - sx)) / d];
}

export function arcRadius(a: Arc): number {
  const c = arcCenter(a);
  return Math.hypot(a.start[0] - c[0], a.start[1] - c[1]);
}

/** `SHAPE_ARC::IsCCW`: start, mid, end wind counter-clockwise in KiCad's convention (cross of end-mid and start-mid > 0). */
export function arcIsCCW(a: Arc): boolean {
  return cross(vsub(a.end, a.mid), vsub(a.start, a.mid)) > 0;
}

/** `SHAPE_ARC::GetCentralAngle`: signed degrees swept from start to end through mid; a full circle when start == end. */
export function arcCentralAngle(a: Arc): number {
  if (eq(a.start, a.end)) return 360;
  const c = arcCenter(a);
  let angle = angleOf([a.end[0] - c[0], a.end[1] - c[1]]) - angleOf([a.start[0] - c[0], a.start[1] - c[1]]);
  if (arcIsCCW(a)) {
    if (angle < 0) angle += 360;
  } else if (angle > 0) {
    angle -= 360;
  }
  return angle;
}

/** `SHAPE_ARC::Reversed`. */
export const arcReversed = (a: Arc): Arc => ({ start: a.end, mid: a.mid, end: a.start });

/** `SHAPE_ARC( centre, start, centralAngle )` (shape_arc.cpp): mid and end are the start rotated by -angle/2 and -angle about the centre. */
export function arcFromCenterAngle(center: V, start: V, centralAngleDeg: number): Arc {
  return {
    start: [start[0], start[1]],
    mid: rotatePointD(start, center, -centralAngleDeg / 2),
    end: rotatePointD(start, center, -centralAngleDeg),
  };
}

/** `RotatePoint( VECTOR2D )` followed by `KiROUND`. */
function rotatePointD(p: V, centre: V, angleDeg: number): V {
  const x = p[0] - centre[0];
  const y = p[1] - centre[1];
  const a = normalize360(angleDeg);
  const rad = (a * Math.PI) / 180;
  const s = a === 0 || a === 180 ? 0 : a === 90 ? 1 : a === 270 ? -1 : Math.sin(rad);
  const c = a === 0 ? 1 : a === 180 ? -1 : a === 90 || a === 270 ? 0 : Math.cos(rad);
  return [kiRound(y * s + x * c + centre[0]), kiRound(y * c - x * s + centre[1])];
}

/**
 * `SHAPE_ARC( SEG A, SEG B, radius )` (shape_arc.cpp): the arc of `radius`
 * tangent to both lines at the corner where they meet (the lines are
 * extended: `Intersect( other, true, true )`). Returns null when they do not
 * meet or either is zero length (the C++ asserts in debug).
 */
export function arcFromFillet(segA: Seg, segB: Seg, radius: number): Arc | null {
  const p = segIntersect(segA, segB, true, true);
  if (!p || segLength(segA) === 0 || segLength(segB) === 0) return null;
  let pToA = vsub(segA.b, p);
  let pToB = vsub(segB.b, p);
  if (norm(pToA) === 0) pToA = vsub(segA.a, p);
  if (norm(pToB) === 0) pToB = vsub(segB.a, p);
  const pToAangle = angleOf(pToA);
  const pToBangle = angleOf(pToB);
  const alpha = normalize180(pToAangle - pToBangle);
  const distPC = radius / Math.abs(Math.sin((alpha * Math.PI) / 180 / 2));
  const angPC = pToAangle - alpha / 2;
  const rad = (normalize360(angPC) * Math.PI) / 180;
  const cx = p[0] + kiRound(distPC * Math.cos(rad));
  const cy = p[1] + kiRound(distPC * Math.sin(rad));
  const centre: V = [cx, cy];
  const start = segLineProject(segA, centre);
  const end = segLineProject(segB, centre);
  const startAngle = angleOf(vsub(start, centre));
  const endAngle = angleOf(vsub(end, centre));
  const midRot = normalize180(startAngle - endAngle) / 2;
  const mid = rotatePoint(start, centre, midRot);
  return { start, mid, end };
}

// ----------------------------------------------- corner_operations.cpp

export interface ChamferResult {
  chamfer: Seg;
  updatedA: Seg | null;
  updatedB: Seg | null;
}

/** `ComputeChamferPoints( segA, segB, params )`. Null when nothing can be chamfered. */
export function computeChamferPoints(segA: Seg, segB: Seg, setbackA: number, setbackB: number): ChamferResult | null {
  if (setbackA === 0 && setbackB === 0) return null;
  if (segLength(segA) < setbackA || segLength(segB) < setbackB) return null;
  const corner = sharedEndpoint(segA, segB);
  if (!corner) return null;
  const aEnd = otherEnd(segA, corner);
  const bEnd = otherEnd(segB, corner);
  const setA = vadd(corner, resize(vsub(aEnd, corner), setbackA));
  const setB = vadd(corner, resize(vsub(bEnd, corner), setbackB));
  const chamfer: Seg = { a: setA, b: setB };
  return {
    chamfer,
    updatedA: eq(aEnd, chamfer.a) ? null : { a: aEnd, b: chamfer.a },
    updatedB: eq(bEnd, chamfer.b) ? null : { a: bEnd, b: chamfer.b },
  };
}

/** `GetBisectorOfCornerSegments` (corner_operations.cpp, anonymous namespace). */
function bisectorOfCorner(segA: Seg, segB: Seg, length: number): Seg {
  const corner = sharedEndpoint(segA, segB)!;
  const pointingAway = (s: Seg, p: V): V => {
    const distA = norm(vsub(s.a, p));
    const distB = norm(vsub(s.b, p));
    return distA < distB ? vsub(s.b, s.a) : vsub(s.a, s.b);
  };
  const maxLen = Math.max(segLength(segA), segLength(segB));
  const aOut = resize(pointingAway(segA, corner), maxLen);
  const bOut = resize(pointingAway(segB, corner), maxLen);
  const bisector = vadd(aOut, bOut);
  return { a: corner, b: vadd(corner, resize(bisector, length)) };
}

export interface DogboneResult {
  arc: Arc;
  updatedA: Seg | null;
  updatedB: Seg | null;
  smallArcMouth: boolean;
}

/** `ComputeDogbone( segA, segB, radius, addSlots )`. */
export function computeDogbone(segA: Seg, segB: Seg, radius: number, addSlots: boolean): DogboneResult | null {
  const corner = sharedEndpoint(segA, segB);
  if (!corner || segsParallel(segA, segB)) return null;
  const bisector = bisectorOfCorner(segA, segB, radius);
  const centre = bisector.b;
  const epsilon = 8;
  const pointNotOnCorner = (hits: V[]): V | null => {
    for (const pt of hits) if (norm(vsub(corner, pt)) > epsilon) return pt;
    return null;
  };
  const ptOnA = pointNotOnCorner(circleIntersectSeg(centre, radius, segA));
  const ptOnB = pointNotOnCorner(circleIntersectSeg(centre, radius, segB));
  if (!ptOnA || !ptOnB) return null;

  let arc: Arc = { start: ptOnA, mid: corner, end: ptOnB };
  const otherA = otherEnd(segA, corner);
  const otherB = otherEnd(segB, corner);
  const smallArcMouth = Math.abs(arcCentralAngle(arc)) > 180 + 1e-3;

  if (!smallArcMouth || !addSlots) {
    return {
      arc,
      updatedA: eq(otherA, ptOnA) ? null : { a: otherA, b: ptOnA },
      updatedB: eq(otherB, ptOnB) ? null : { a: otherB, b: ptOnB },
      smallArcMouth,
    };
  }

  // A small mouth: pull the arc back to 180 degrees and glue the bisector to its ends.
  let slot: Arc = { start: rotatePoint(corner, centre, 90), mid: corner, end: rotatePoint(corner, centre, -90) };
  if (!pointsAreInSameDirection(slot.start, arc.start, centre)) slot = { start: slot.end, mid: slot.mid, end: slot.start };
  const ext = vsub(centre, corner);
  const extA = halfLineIntersect(slot.start, vadd(slot.start, ext), segA);
  const extB = halfLineIntersect(slot.end, vadd(slot.end, ext), segB);
  if (!extA || !extB) return null;
  arc = slot;
  return {
    arc,
    updatedA: eq(otherA, extA) ? null : { a: otherA, b: extA },
    updatedB: eq(otherB, extB) ? null : { a: otherB, b: extB },
    smallArcMouth,
  };
}

/** `GetClampedCoords( pt, padding )` (common): clamp to the int range minus `padding` so a far intersection stays representable. */
export function clampedCoords(p: V, padding: number): V {
  const max = 2147483647 - padding;
  const min = -2147483648 + padding;
  return [Math.min(max, Math.max(min, p[0])), Math.min(max, Math.max(min, p[1]))];
}
