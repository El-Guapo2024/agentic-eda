// KiCad 10's line endings: the arrowhead, open arrowhead, circle or square at the start or the end of a line, an arc or a curve (file version 20260818).
//
// Ported from KiCad 25ec2335 (the commit the box's kicad-cli nightly is built from; the 8303b2ad source the rest of the port reads predates line endings):
//   common/line_ending.cpp   LINE_ENDING::GetEffectiveSize, GetShortenDepth, GetCurveOrientationDepth, GetShapes, Draw, StyleToChoiceIndex
//   common/eda_shape.cpp     EDA_SHAPE::GetEndingTangents, GetLineEndingEndpoints, ShortenSegmentForEndings, ShortenArcForEndings, ShortenedBezierCurve,
//                            bezierSpeedAt, bezierLength, findBezierTAtLength; SetArcGeometry's winding (m_endsSwapped)
//   libs/kimath/include/bezier_curves.h   BEZIER::PointAt, Split, SubCurve
//   pcbnew/pcb_painter.cpp   PCB_PAINTER::draw( PCB_SHAPE ): the body drawn shortened, the endings drawn at the shape's ends in the shape's colour
//
// Units are the canvas's (micrometres); KiCad's are nanometres, so KiROUND's rounding lands on a finer grid there -- the shapes are the same to the pixel.
import { kiRound } from "./pcbGeom";

export type LineEndingStyle = "none" | "arrow" | "circle" | "square" | "arrow_open";

/** One `LINE_ENDING` as the state carries it: 0 for a size is automatic. */
export interface LineEnding {
  style: LineEndingStyle;
  length: number;
  width: number;
  stroke_width: number;
}

type P = readonly [number, number];
type V = [number, number];

/** `LINE_ENDING::DEFAULT_RATIO_LENGTH` and `DEFAULT_RATIO_WIDTH`. */
export const DEFAULT_RATIO_LENGTH = 5.0;
export const DEFAULT_RATIO_WIDTH = 5.0;

/** `LINE_ENDING::s_defaultChoiceOrder` with the labels `DIALOG_SHAPE_PROPERTIES::createLineEndingControls` gives them (None, Arrow, Open Arrow, Circle, Square). */
export const LINE_ENDING_CHOICES: { label: string; value: LineEndingStyle }[] = [
  { label: "None", value: "none" },
  { label: "Arrow", value: "arrow" },
  { label: "Open Arrow", value: "arrow_open" },
  { label: "Circle", value: "circle" },
  { label: "Square", value: "square" },
];

/** The dialog's help line under the ending controls (`DIALOG_SHAPE_PROPERTIES::TransferDataToWindow`). */
export const LINE_ENDING_HELP = `Set line ending sizes to 0 for automatic (${DEFAULT_RATIO_LENGTH}× line width).`;

export const NO_ENDING: LineEnding = { style: "none", length: 0, width: 0, stroke_width: 0 };

/** `LINE_ENDING::GetEffectiveSize`: [along the line, across it]; a size of 0 takes the other one, both 0 the ratio times the line width. */
export function effectiveSize(e: LineEnding, lineWidth: number): V {
  const autoLen = kiRound(lineWidth * DEFAULT_RATIO_LENGTH);
  const autoWid = kiRound(lineWidth * DEFAULT_RATIO_WIDTH);
  const len = e.length > 0 ? e.length : e.width > 0 ? e.width : autoLen;
  const wid = e.width > 0 ? e.width : e.length > 0 ? e.length : autoWid;
  return [len, wid];
}

/** `LINE_ENDING::GetShortenDepth`: how far the line stops short of its end -- an arrow's length, half a circle's or a square's, nothing for an open arrow; less half the outline. */
export function shortenDepth(e: LineEnding | undefined, lineWidth: number): number {
  if (!e || e.style === "none") return 0;
  const sz = effectiveSize(e, lineWidth);
  let raw = e.style === "arrow" ? sz[0] : e.style === "arrow_open" ? 0 : Math.trunc(sz[0] / 2);
  if (e.stroke_width > 0) raw = Math.max(0, raw - Math.trunc(e.stroke_width / 2));
  return raw;
}

/** `LINE_ENDING::GetCurveOrientationDepth`. */
export function curveOrientationDepth(e: LineEnding, lineWidth: number): number {
  if (e.style === "arrow_open") return effectiveSize(e, lineWidth)[0];
  return shortenDepth(e, lineWidth);
}

/** `RotatePoint( pt, -aTangent )` (trigo.cpp): `aTangent` in degrees as `EDA_ANGLE( VECTOR2 )` gives it in KiCad's y-down frame. */
function rotateByTangent(p: P, tangentDeg: number): V {
  const a = -tangentDeg;
  const norm = ((a % 360) + 360) % 360;
  if (norm === 0) return [p[0], p[1]];
  if (norm === 90) return [p[1], -p[0]];
  if (norm === 180) return [-p[0], -p[1]];
  if (norm === 270) return [-p[1], p[0]];
  const r = (norm * Math.PI) / 180;
  const s = Math.sin(r);
  const c = Math.cos(r);
  return [p[1] * s + p[0] * c, p[1] * c - p[0] * s];
}

/**
 * `LINE_ENDING::GetShapes`: the ending's outline at `point`, `tangentDeg` pointing out of the line. An arrow's tip is at the point (a closed triangle), an open arrow's vertex
 * (an open three-point polyline), a circle and a square are centred on it. Empty for no ending.
 */
export function endingShape(e: LineEnding, point: P, tangentDeg: number, lineWidth: number): V[] {
  if (e.style === "none") return [];
  const [sizeX, sizeY] = effectiveSize(e, lineWidth);
  const pts: V[] = [];
  switch (e.style) {
    case "arrow": {
      const half = Math.trunc(sizeY / 2);
      pts.push([0, 0], [-sizeX, half], [-sizeX, -half], [0, 0]);
      break;
    }
    case "arrow_open": {
      const half = Math.trunc(sizeY / 2);
      pts.push([-sizeX, -half], [0, 0], [-sizeX, half]);
      break;
    }
    case "circle": {
      const hx = Math.trunc(sizeX / 2);
      const hy = Math.trunc(sizeY / 2);
      const nSeg = 32;
      for (let i = 0; i <= nSeg; i++) {
        const a = (2 * Math.PI * i) / nSeg;
        pts.push([kiRound(hx * Math.cos(a)), kiRound(hy * Math.sin(a))]);
      }
      break;
    }
    case "square": {
      const hx = Math.trunc(sizeX / 2);
      const hy = Math.trunc(sizeY / 2);
      pts.push([hx, hy], [hx, -hy], [-hx, -hy], [-hx, hy], [hx, hy]);
      break;
    }
  }
  return pts.map((p) => {
    const r = rotateByTangent(p, tangentDeg);
    return [r[0] + point[0], r[1] + point[1]] as V;
  });
}

/** `EDA_ANGLE( VECTOR2D )` in degrees: `atan2( y, x )` of KiCad's y-down frame. */
export function angleOf(dx: number, dy: number): number {
  if (dx === 0 && dy === 0) return 0;
  return (Math.atan2(dy, dx) * 180) / Math.PI;
}

// ---------------------------------------------------------------------------------------------------------------------------- the curve

export interface Bezier {
  start: P;
  c1: P;
  c2: P;
  end: P;
}

/** `BEZIER::PointAt`. */
export function bezierAt(b: Bezier, t: number): V {
  const t2 = t * t;
  const t3 = t2 * t;
  const m = 1 - t;
  const m2 = m * m;
  const m3 = m2 * m;
  return [m3 * b.start[0] + 3 * t * m2 * b.c1[0] + 3 * t2 * m * b.c2[0] + t3 * b.end[0], m3 * b.start[1] + 3 * t * m2 * b.c1[1] + 3 * t2 * m * b.c2[1] + t3 * b.end[1]];
}

const lerp = (a: P, b: P, t: number): V => [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];

/** `BEZIER::Split`: de Casteljau at `t`. */
function split(b: Bezier, t: number): [Bezier, Bezier] {
  const startC1 = lerp(b.start, b.c1, t);
  const c1C2 = lerp(b.c1, b.c2, t);
  const c2End = lerp(b.c2, b.end, t);
  const leftC2 = lerp(startC1, c1C2, t);
  const rightC1 = lerp(c1C2, c2End, t);
  const shared = lerp(leftC2, rightC1, t);
  return [
    { start: b.start, c1: startC1, c2: leftC2, end: shared },
    { start: shared, c1: rightC1, c2: c2End, end: b.end },
  ];
}

/** `BEZIER::SubCurve`. */
export function subCurve(b: Bezier, t0in: number, t1in: number): Bezier {
  const t0 = Math.min(1, Math.max(0, t0in));
  const t1 = Math.min(1, Math.max(0, t1in));
  if (t1 <= t0) {
    const p = bezierAt(b, t0);
    return { start: p, c1: p, c2: p, end: p };
  }
  if (t0 === 0 && t1 === 1) return b;
  if (t0 === 0) return split(b, t1)[0];
  if (t1 === 1) return split(b, t0)[1];
  const left = split(b, t1)[0];
  return split(left, t0 / t1)[1];
}

function bezierSpeedAt(b: Bezier, t: number): number {
  const m = 1 - t;
  const dx = 3 * m * m * (b.c1[0] - b.start[0]) + 6 * m * t * (b.c2[0] - b.c1[0]) + 3 * t * t * (b.end[0] - b.c2[0]);
  const dy = 3 * m * m * (b.c1[1] - b.start[1]) + 6 * m * t * (b.c2[1] - b.c1[1]) + 3 * t * t * (b.end[1] - b.c2[1]);
  return Math.hypot(dx, dy);
}

const GL_NODES = [0.095012509837637, 0.281603550779259, 0.458016777657227, 0.6178762444026438, 0.755404408355003, 0.8656312023878318, 0.9445750230732326, 0.9894009349916499];
const GL_WEIGHTS = [0.1894506104550685, 0.1826034150449236, 0.1691565193950025, 0.1495959888165767, 0.1246289712555339, 0.0951585116824928, 0.0622535239386479, 0.0271524594117541];

/** `bezierLength`: 16-point Gauss-Legendre over [t0, t1]. */
export function bezierLength(b: Bezier, t0: number, t1: number): number {
  if (t1 <= t0) return 0;
  const half = (t1 - t0) / 2;
  const centre = (t0 + t1) / 2;
  let len = 0;
  GL_NODES.forEach((node, i) => {
    const off = half * node;
    len += (GL_WEIGHTS[i] ?? 0) * (bezierSpeedAt(b, centre - off) + bezierSpeedAt(b, centre + off));
  });
  return half * len;
}

/** `findBezierTAtLength`: 24 halvings. */
export function bezierTAtLength(b: Bezier, target: number, total: number): number {
  if (target <= 0) return 0;
  if (target >= total) return 1;
  let low = 0;
  let high = 1;
  for (let i = 0; i < 24; i++) {
    const mid = (low + high) / 2;
    if (bezierLength(b, 0, mid) < target) low = mid;
    else high = mid;
  }
  return (low + high) / 2;
}

// ---------------------------------------------------------------------------------------------------------------------------- the shape

/** The shapes that take endings, as the canvas has them. */
export type OpenShape =
  | { kind: "segment"; start: P; end: P; stroke_width: number; start_ending?: LineEnding; end_ending?: LineEnding }
  | { kind: "arc"; start: P; mid: P; end: P; stroke_width: number; start_ending?: LineEnding; end_ending?: LineEnding }
  | { kind: "bezier"; start: P; c1: P; c2: P; end: P; stroke_width: number; start_ending?: LineEnding; end_ending?: LineEnding };

export function hasEndings(s: { start_ending?: LineEnding; end_ending?: LineEnding }): boolean {
  return (!!s.start_ending && s.start_ending.style !== "none") || (!!s.end_ending && s.end_ending.style !== "none");
}

/** The centre of the circle through three points, `CalcArcCenter`; null when they are in a line. */
function arcCentre(a: P, b: P, c: P): V | null {
  const d = 2 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
  if (Math.abs(d) < 1e-9) return null;
  const a2 = a[0] * a[0] + a[1] * a[1];
  const b2 = b[0] * b[0] + b[1] * b[1];
  const c2 = c[0] * c[0] + c[1] * c[1];
  return [(a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d, (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d];
}

/** `CalcArcAngles`: the start angle and an end angle at or past it (degrees, KiCad's y-down sense: an increasing angle turns clockwise on the screen). */
function arcAngles(start: P, end: P, centre: P): [number, number] {
  const a0 = angleOf(start[0] - centre[0], start[1] - centre[1]);
  let a1 = angleOf(end[0] - centre[0], end[1] - centre[1]);
  if (a1 === a0) a1 = a0 + 360;
  while (a1 < a0) a1 += 360;
  return [a0, a1];
}

/**
 * An arc as KiCad holds it once read: `EDA_SHAPE::SetArcGeometry` keeps the ends in the order whose sweep of increasing angle passes through `mid`, and swaps them
 * (`m_endsSwapped`) when the file's order winds the other way -- while the endings stay where they were set, so the file's start ending is drawn at the end KiCad calls
 * its start. Returns the start, the end and the centre in KiCad's order.
 */
export function kicadArc(start: P, mid: P, end: P): { start: P; end: P; centre: V } | null {
  const centre = arcCentre(start, mid, end);
  if (!centre) return null;
  const [a0, a1] = arcAngles(start, end, centre);
  const r = Math.hypot(start[0] - centre[0], start[1] - centre[1]);
  const am = ((a0 + a1) / 2) * (Math.PI / 180);
  const newMid: V = [centre[0] + r * Math.cos(am), centre[1] + r * Math.sin(am)];
  const dist = Math.hypot(newMid[0] - mid[0], newMid[1] - mid[1]);
  const dist2 = Math.hypot(newMid[0] - centre[0], newMid[1] - centre[1]);
  return dist > dist2 ? { start: end, end: start, centre } : { start, end, centre };
}

/** `EDA_SHAPE::ShortenSegmentForEndings`: the visible line, or null when the endings take all of it. */
export function shortenSegment(start: P, end: P, se: LineEnding | undefined, ee: LineEnding | undefined, lineWidth: number): [V, V] | null {
  const dx = end[0] - start[0];
  const dy = end[1] - start[1];
  const len = Math.hypot(dx, dy);
  const s: V = [start[0], start[1]];
  const e: V = [end[0], end[1]];
  if (len === 0) return [s, e];
  const sd = shortenDepth(se, lineWidth);
  const ed = shortenDepth(ee, lineWidth);
  const total = Math.max(0, sd) + Math.max(0, ed);
  if (total <= 0) return [s, e];
  if (total >= len) return null;
  const ux = dx / len;
  const uy = dy / len;
  if (sd > 0) {
    s[0] += kiRound(ux * sd);
    s[1] += kiRound(uy * sd);
  }
  if (ed > 0) {
    e[0] -= kiRound(ux * ed);
    e[1] -= kiRound(uy * ed);
  }
  return [s, e];
}

/** `EDA_SHAPE::ShortenArcForEndings` on KiCad's arc: the start angle and the sweep (degrees) left, or null when nothing is. */
export function shortenArc(startDeg: number, sweepDeg: number, radius: number, se: LineEnding | undefined, ee: LineEnding | undefined, lineWidth: number): [number, number] | null {
  if (radius <= 0) return [startDeg, sweepDeg];
  const sd = shortenDepth(se, lineWidth);
  const ed = shortenDepth(ee, lineWidth);
  const total = Math.max(0, sd) + Math.max(0, ed);
  if (total <= 0) return [startDeg, sweepDeg];
  const arcLen = Math.abs(radius * ((sweepDeg * Math.PI) / 180));
  if (total >= arcLen) return null;
  const startOff = ((Math.max(0, sd) / radius) * 180) / Math.PI;
  const totalOff = ((total / radius) * 180) / Math.PI;
  return sweepDeg > 0 ? [startDeg + startOff, sweepDeg - totalOff] : [startDeg - startOff, sweepDeg + totalOff];
}

/** `EDA_SHAPE::ShortenedBezierCurve`: the curve left once the endings have taken their part, or null when they take all of it. */
export function shortenBezier(b: Bezier, se: LineEnding | undefined, ee: LineEnding | undefined, lineWidth: number): Bezier | null {
  const sd = shortenDepth(se, lineWidth);
  const ed = shortenDepth(ee, lineWidth);
  if (sd <= 0 && ed <= 0) return b;
  const total = bezierLength(b, 0, 1);
  if (total <= 0 || sd + ed >= total) return null;
  const t0 = bezierTAtLength(b, sd, total);
  const t1 = bezierTAtLength(b, total - ed, total);
  if (t1 <= t0) return null;
  return subCurve(b, t0, t1);
}

/** Where the endings go and which way they point: `EDA_SHAPE::GetLineEndingEndpoints` and `GetEndingTangents` (degrees). */
export function endingPlacement(s: OpenShape): { start: V; end: V; startTangent: number; endTangent: number } | null {
  const lw = s.stroke_width;
  switch (s.kind) {
    case "segment": {
      const line = angleOf(s.end[0] - s.start[0], s.end[1] - s.start[1]);
      return { start: [s.start[0], s.start[1]], end: [s.end[0], s.end[1]], startTangent: line + 180, endTangent: line };
    }
    case "arc": {
      const k = kicadArc(s.start, s.mid, s.end);
      if (!k) return null;
      // An arc's `m_ellipse` angles are both 0, so `GetEndingTangents` always takes the branch that turns the start back by 90 degrees and the end on by 90.
      const sa = angleOf(k.start[0] - k.centre[0], k.start[1] - k.centre[1]);
      const ea = angleOf(k.end[0] - k.centre[0], k.end[1] - k.centre[1]);
      return { start: [k.start[0], k.start[1]], end: [k.end[0], k.end[1]], startTangent: sa - 90, endTangent: ea + 90 };
    }
    case "bezier": {
      const b: Bezier = { start: s.start, c1: s.c1, c2: s.c2, end: s.end };
      const shortened = shortenBezier(b, s.start_ending, s.end_ending, lw);
      const fallbackStart = (): number => (s.start[0] !== s.c1[0] || s.start[1] !== s.c1[1] ? angleOf(s.start[0] - s.c1[0], s.start[1] - s.c1[1]) : 0);
      const fallbackEnd = (): number => (s.end[0] !== s.c2[0] || s.end[1] !== s.c2[1] ? angleOf(s.end[0] - s.c2[0], s.end[1] - s.c2[1]) : 0);
      let startTangent: number;
      let endTangent: number;
      if (shortened) {
        const sdx = s.start[0] - shortened.start[0];
        const sdy = s.start[1] - shortened.start[1];
        startTangent = Math.hypot(sdx, sdy) > 0 ? angleOf(sdx, sdy) : fallbackStart();
        const edx = s.end[0] - shortened.end[0];
        const edy = s.end[1] - shortened.end[1];
        endTangent = Math.hypot(edx, edy) > 0 ? angleOf(edx, edy) : fallbackEnd();
      } else {
        startTangent = fallbackStart();
        endTangent = fallbackEnd();
      }
      // An open arrow does not shorten the curve but still reaches back along it: it is turned to the point of the curve that far from its end.
      let total: number | undefined;
      const flex = (e: LineEnding | undefined, atStart: boolean): number | null => {
        if (!e || e.style !== "arrow_open") return null;
        const depth = curveOrientationDepth(e, lw);
        if (depth <= 0) return null;
        total ??= bezierLength(b, 0, 1);
        if (total <= 0) return null;
        let dist = Math.min(depth, total);
        if (!atStart) dist = total - dist;
        const sample = bezierAt(b, bezierTAtLength(b, dist, total));
        const ep = atStart ? s.start : s.end;
        const dx = ep[0] - sample[0];
        const dy = ep[1] - sample[1];
        return Math.hypot(dx, dy) > 0 ? angleOf(dx, dy) : null;
      };
      startTangent = flex(s.start_ending, true) ?? startTangent;
      endTangent = flex(s.end_ending, false) ?? endTangent;
      return { start: [s.start[0], s.start[1]], end: [s.end[0], s.end[1]], startTangent, endTangent };
    }
  }
}

/** One ending ready to draw: `LINE_ENDING::Draw` fills a closed shape (and strokes it at its outline width when it has one) and strokes an open arrow at its outline width or the line's. */
export interface EndingInk {
  pts: V[];
  fill: boolean;
  /** The pen of the outline, 0 for none. */
  stroke: number;
}

/** Both endings of `s`, as `PCB_PAINTER::draw( PCB_SHAPE )` draws them after the body. */
export function endingInks(s: OpenShape): EndingInk[] {
  if (!hasEndings(s)) return [];
  const at = endingPlacement(s);
  if (!at) return [];
  const lw = s.stroke_width;
  const out: EndingInk[] = [];
  for (const [e, p, t] of [
    [s.start_ending, at.start, at.startTangent],
    [s.end_ending, at.end, at.endTangent],
  ] as [LineEnding | undefined, V, number][]) {
    if (!e || e.style === "none") continue;
    const pts = endingShape(e, p, t, lw);
    if (pts.length === 0) continue;
    if (e.style === "arrow_open") out.push({ pts, fill: false, stroke: e.stroke_width > 0 ? e.stroke_width : lw });
    else out.push({ pts, fill: true, stroke: e.stroke_width > 0 ? e.stroke_width : 0 });
  }
  return out;
}
