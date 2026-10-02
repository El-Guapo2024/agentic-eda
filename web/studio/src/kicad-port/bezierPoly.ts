// Port of BEZIER_POLY::GetPoly for cubic curves (libs/kimath/src/bezier_curves.cpp,
// "Fast, Precise Flattening of Cubic Bezier segments" by Hain et al., with the
// recursive-subdivision fallback): the polyline KiCad draws, plots and
// design-rule-checks for a `PCB_SHAPE` of type BEZIER
// (`EDA_SHAPE::RebuildBezierToSegmentsPointsList( maxError )`).
//
// `crates/model/src/ir.rs` (`bezier_polyline`) carries the same algorithm for the
// backend (DRC, plot, board edge); keep the two in step.

type V = [number, number];

const sub = (a: V, b: V): V => [a[0] - b[0], a[1] - b[1]];
const add = (a: V, b: V): V => [a[0] + b[0], a[1] + b[1]];
const mul = (a: V, s: number): V => [a[0] * s, a[1] * s];
const dot = (a: V, b: V): number => a[0] * b[0] + a[1] * b[1];
const cross = (a: V, b: V): number => a[0] * b[1] - a[1] * b[0];
const norm2 = (a: V): number => a[0] * a[0] + a[1] * a[1];

/** One cubic: the four control points (`BEZIER_POLY::m_ctrlPts`). */
type Cubic = [V, V, V, V];

/** `KiROUND`. */
function kiRound(v: number): number {
  const r = v < 0 ? -Math.round(-v) : Math.round(v);
  return r === 0 ? 0 : r;
}

function isNaNCubic(c: Cubic): boolean {
  return c.some((p) => Number.isNaN(p[0]) || Number.isNaN(p[1]));
}

/** `BEZIER_POLY::isFlat` (4 control points). */
function isFlat(c: Cubic, maxError: number): boolean {
  const delta = sub(c[3], c[0]);
  const d21 = sub(c[1], c[0]);
  const d31 = sub(c[2], c[0]);
  const cross1 = cross(delta, d21);
  const cross2 = cross(delta, d31);
  const invDeltaSq = 1.0 / norm2(delta);
  const d1 = cross1 * cross1 * invDeltaSq;
  const d2 = cross2 * cross2 * invDeltaSq;
  const factor = cross1 * cross2 > 0.0 ? 3.0 / 4.0 : 4.0 / 9.0;
  const f2 = factor * factor;
  const tol = maxError * maxError;
  return d1 * f2 <= tol && d2 * f2 <= tol;
}

/** `BEZIER_POLY::subdivide( aT, left, right )`. */
function subdivide(c: Cubic, t: number): [Cubic, Cubic] {
  const left1 = add(c[0], mul(sub(c[1], c[0]), t));
  const tmp = add(c[1], mul(sub(c[2], c[1]), t));
  const left2 = add(left1, mul(sub(tmp, left1), t));
  const right2 = add(c[2], mul(sub(c[3], c[2]), t));
  const right1 = add(tmp, mul(sub(right2, tmp), t));
  const shared = add(left2, mul(sub(right1, left2), t));
  return [
    [c[0], left1, left2, shared],
    [shared, right1, right2, c[3]],
  ];
}

/** `BEZIER_POLY::recursiveSegmentation`: depth-first halving until each piece is flat; zero-length pieces are dropped. */
function recursiveSegmentation(c: Cubic, out: V[], threshold: number): void {
  const stack: Cubic[] = [c];
  while (stack.length > 0) {
    const bezier = stack[stack.length - 1]!;
    if (bezier[3][0] === bezier[0][0] && bezier[3][1] === bezier[0][1]) {
      stack.pop();
    } else if (isFlat(bezier, threshold)) {
      out.push(bezier[3]);
      stack.pop();
    } else {
      const [left, right] = subdivide(bezier, 0.5);
      stack[stack.length - 1] = right;
      stack.push(left);
    }
  }
}

/** `BEZIER_POLY::numberOfInflectionPoints`. */
function numberOfInflectionPoints(c: Cubic): number {
  const d21 = sub(c[1], c[0]);
  const d32 = sub(c[2], c[1]);
  const d43 = sub(c[3], c[2]);
  const cross1 = cross(d21, d32) * cross(d32, d43);
  const cross2 = cross(d21, d32) * cross(d21, d43);
  if (cross1 < 0.0) return 1;
  if (cross2 > 0.0) return 0;
  const b1 = dot(d21, d32) > 0.0;
  const b2 = dot(d32, d43) > 0.0;
  if (b1 !== b2) return 0;
  return -1; // "These are rare cases where there are potentially 2 or 0 inflection points."
}

/** `BEZIER_POLY::findInflectionPoints`: how many (0-2) and where (t1 <= t2). */
function findInflectionPoints(c: Cubic): { n: number; t1: number; t2: number } {
  const A: V = [-c[0][0] + 3 * c[1][0] - 3 * c[2][0] + c[3][0], -c[0][1] + 3 * c[1][1] - 3 * c[2][1] + c[3][1]];
  const B: V = [3 * c[0][0] - 6 * c[1][0] + 3 * c[2][0], 3 * c[0][1] - 6 * c[1][1] + 3 * c[2][1]];
  const C: V = [-3 * c[0][0] + 3 * c[1][0], -3 * c[0][1] + 3 * c[1][1]];
  const a = 3 * cross(A, B);
  const b = 3 * cross(A, C);
  const cc = cross(B, C);
  const r2 = b * b - 4 * a * cc;
  if (r2 >= 0.0 && a !== 0.0) {
    const r = Math.sqrt(r2);
    let t1 = (-b + r) / (2 * a);
    let t2 = (-b - r) / (2 * a);
    if (t1 > 0.0 && t1 < 1.0 && t2 > 0.0 && t2 < 1.0) {
      if (t1 > t2) [t1, t2] = [t2, t1];
      return { n: t2 - t1 > 0.00001 ? 2 : 1, t1, t2 };
    }
    if (t1 > 0.0 && t1 < 1.0) return { n: 1, t1, t2: 0 };
    if (t2 > 0.0 && t2 < 1.0) return { n: 1, t1: t2, t2: 0 };
  }
  return { n: 0, t1: 0, t2: 0 };
}

/** `BEZIER_POLY::thirdControlPointDeviation`. */
function thirdControlPointDeviation(c: Cubic): number {
  const delta = sub(c[1], c[0]);
  const lenSq = norm2(delta);
  if (lenSq < 1e-6) return 0.0;
  const len = Math.sqrt(lenSq);
  const r = (c[1][1] - c[0][1]) / len;
  const s = (c[0][0] - c[1][0]) / len;
  const u = (c[1][0] * c[0][1] - c[0][0] * c[1][1]) / len;
  return Math.abs(r * c[2][0] + s * c[2][1] + u);
}

/** `BEZIER_POLY::cubicParabolicApprox` (Hain et al.'s formula 2 picks each step's `t`). */
function cubicParabolicApprox(start: Cubic, out: V[], maxError: number): void {
  let c = start;
  for (;;) {
    if (isNaNCubic(c)) break;
    if (isFlat(c, maxError)) {
      out.push(c[3]);
      break;
    }
    const d = thirdControlPointDeviation(c);
    const t = 2 * Math.sqrt(maxError / (3.0 * d));
    if (t > 1.0) {
      // "Case where the t value calculated is invalid, so use recursive subdivision"
      recursiveSegmentation(c, out, maxError);
      break;
    }
    const [b1, b2] = subdivide(c, t);
    if (isFlat(b1, maxError)) out.push(b1[3]);
    else recursiveSegmentation(b1, out, maxError); // "if not then use segment to handle any mathematical errors"
    c = b2;
  }
}

/** `BEZIER_POLY::getCubicPoly`: split at inflection points, parabolic approximation per piece. */
function getCubicPoly(c: Cubic, out: V[], maxError: number): void {
  out.push(c[0]);
  if (numberOfInflectionPoints(c) === 0) {
    cubicParabolicApprox(c, out, maxError);
    return;
  }
  const ip = findInflectionPoints(c);
  if (ip.n === 2) {
    const [sub1, tmp1] = subdivide(c, ip.t1);
    cubicParabolicApprox(sub1, out, maxError);
    const second = findInflectionPoints(tmp1);
    if (second.n === 2 || second.n === 1) {
      const [sub2, sub3] = subdivide(tmp1, second.t1);
      recursiveSegmentation(sub2, out, maxError); // "Use Segment for the second (middle) subsegment"
      cubicParabolicApprox(sub3, out, maxError);
    } else {
      out.push(tmp1[3]);
    }
  } else if (ip.n === 1) {
    const [sub1, sub2] = subdivide(c, ip.t1);
    cubicParabolicApprox(sub1, out, maxError);
    cubicParabolicApprox(sub2, out, maxError);
  } else {
    cubicParabolicApprox(c, out, maxError);
  }
}

/**
 * `BEZIER_POLY( start, c1, c2, end ).GetPoly( out, maxError )`: the integer polyline
 * (start first, end last) within `maxError` (board units -- micrometres here) of the
 * true curve. `maxError <= 0` falls back to 10, as the C++ does.
 */
export function bezierPolyline(start: readonly [number, number], c1: readonly [number, number], c2: readonly [number, number], end: readonly [number, number], maxError = 5): [number, number][] {
  const err = maxError <= 0 ? 10 : maxError;
  const out: V[] = [];
  getCubicPoly([[start[0], start[1]], [c1[0], c1[1]], [c2[0], c2[1]], [end[0], end[1]]], out, err);
  return out.map((p) => [kiRound(p[0]), kiRound(p[1])] as [number, number]);
}

/** A point on the curve at parameter `t` (`BEZIER<>::PointAt`) -- for hit testing and mid-point queries. */
export function bezierPointAt(start: readonly [number, number], c1: readonly [number, number], c2: readonly [number, number], end: readonly [number, number], t: number): [number, number] {
  const omt = 1 - t;
  const a = omt * omt * omt;
  const b = 3 * t * omt * omt;
  const c = 3 * t * t * omt;
  const d = t * t * t;
  return [a * start[0] + b * c1[0] + c * c2[0] + d * end[0], a * start[1] + b * c1[1] + c * c2[1] + d * end[1]];
}
