// How a schematic rectangle, circle, arc, Bezier curve or text box takes shape while it is drawn --
// `SCH_DRAWING_TOOLS::DrawShape` driving `EDA_SHAPE::beginEdit / continueEdit / calcEdit`
// (eeschema/tools/sch_drawing_tools.cpp, common/eda_shape.cpp at 8303b2ad).
//
//   rectangle, text box   click one corner, click the opposite one
//   circle                click the centre, click a point on the circle
//   arc                   click the start, click the end: the arc between them subtends 90 degrees, turning clockwise
//                         (`calcEdit` edit state 1: radius = chord / sqrt 2); the point editor adjusts it afterwards
//   Bezier curve          click the start, the end, control point 1, control point 2 (edit states 1..3)
//
// There is no 3-click arc here: that is the board editor's `ARC_GEOM_MANAGER` (kicad-port/arcGeom.ts), not this tool.
import type { SchGraphicShape } from "../api/schEditTypes";

export type P = readonly [number, number];
export type ShapeToolKind = "rectangle" | "circle" | "arc" | "bezier" | "text_box";

/** The shape under construction: `m_start`, `m_end`, `m_arcCenter`, `m_bezierC1`, `m_bezierC2` and `m_editState`. */
export interface ShapeEdit {
  kind: ShapeToolKind;
  start: P;
  end: P;
  center: P;
  c1: P;
  c2: P;
  state: number;
}

/** `beginEdit`: the first click. */
export function beginEdit(kind: ShapeToolKind, at: P): ShapeEdit {
  return { kind, start: at, end: at, center: at, c1: at, c2: at, state: kind === "arc" || kind === "bezier" ? 1 : 0 };
}

/** `continueEdit`: a further click. `more` is true when the shape wants more points, false when this click finishes it. */
export function continueEdit(e: ShapeEdit): { edit: ShapeEdit; more: boolean } {
  if (e.kind !== "bezier") return { edit: e, more: false };
  if (e.state === 3) return { edit: e, more: false };
  return { edit: { ...e, state: e.state + 1 }, more: true };
}

const kiRound = (v: number): number => (v < 0 ? -Math.round(-v) : Math.round(v));

/** `EDA_SHAPE::GetArcAngle` as degrees: the sweep from the start radius to the end radius, in (0, 360], growing with `atan2` (clockwise on a Y-down sheet). */
function arcSweepDeg(start: P, end: P, center: P): number {
  const a0 = Math.atan2(start[1] - center[1], start[0] - center[0]);
  let a1 = Math.atan2(end[1] - center[1], end[0] - center[0]);
  if (a1 === a0) a1 = a0 + 2 * Math.PI; // "ring, not null"
  while (a1 < a0) a1 += 2 * Math.PI;
  return ((a1 - a0) * 180) / Math.PI;
}

/** `calcEdit` for an arc in edit state 1: the end follows the cursor and the centre is chosen so the arc subtends 90 degrees. */
function calcArc(e: ShapeEdit, pos: P): ShapeEdit {
  const end = pos;
  const radius = Math.hypot(e.start[0] - end[0], e.start[1] - end[1]) * Math.SQRT1_2;
  const l = Math.hypot(e.start[0] - end[0], e.start[1] - end[1]);
  const m: P = [(e.start[0] + end[0]) / 2, (e.start[1] + end[1]) / 2];
  const sqRadDiff = radius * radius - (l * l) / 4;
  let d: P = [0, 0];
  if (l > 0 && sqRadDiff >= 0) d = [(Math.sqrt(sqRadDiff) * (e.start[1] - end[1])) / l, (Math.sqrt(sqRadDiff) * (end[0] - e.start[0])) / l];
  const c1: P = [kiRound(m[0] + d[0]), kiRound(m[1] + d[1])];
  const c2: P = [kiRound(m[0] - d[0]), kiRound(m[1] - d[1])];
  // "Keep arc clockwise while drawing i.e. arc angle = 90 deg": it can be 90 or 270 depending on the centre choice.
  const center = arcSweepDeg(e.start, end, c1) > 180 ? c2 : c1;
  return { ...e, end, center };
}

/** `calcEdit`: the cursor moved to `pos`. */
export function calcEdit(e: ShapeEdit, pos: P): ShapeEdit {
  switch (e.kind) {
    case "rectangle":
    case "circle":
    case "text_box":
      return { ...e, end: pos };
    case "bezier":
      switch (e.state) {
        case 1:
          return { ...e, c2: pos, end: pos };
        case 2:
          return { ...e, c1: pos };
        case 3:
          return { ...e, c2: pos };
        default:
          return { ...e, start: pos, end: pos, c1: pos, c2: pos };
      }
    case "arc":
      return calcArc(e, pos);
  }
}

/** `GetArcMid`: the start radius turned half the sweep about the centre. */
export function arcMid(start: P, end: P, center: P): P {
  const a0 = Math.atan2(start[1] - center[1], start[0] - center[0]);
  const half = (arcSweepDeg(start, end, center) * Math.PI) / 360;
  const r = Math.hypot(start[0] - center[0], start[1] - center[1]);
  return [kiRound(center[0] + r * Math.cos(a0 + half)), kiRound(center[1] + r * Math.sin(a0 + half))];
}

const xy = (p: P) => ({ x: p[0], y: p[1] });

/** The shape the edit stands for as the verbs take it (a circle's radius is `GetRadius`: the centre-to-end distance, never below 1); null while it is degenerate. */
export function toShape(e: ShapeEdit): SchGraphicShape | null {
  switch (e.kind) {
    case "rectangle":
      return e.start[0] === e.end[0] && e.start[1] === e.end[1] ? null : { type: "rectangle", start: xy(e.start), end: xy(e.end) };
    case "circle": {
      const radius = Math.max(1, kiRound(Math.hypot(e.start[0] - e.end[0], e.start[1] - e.end[1])));
      return e.start[0] === e.end[0] && e.start[1] === e.end[1] ? null : { type: "circle", center: xy(e.start), radius_um: radius };
    }
    case "arc":
      return e.start[0] === e.end[0] && e.start[1] === e.end[1] ? null : { type: "arc", start: xy(e.start), mid: xy(arcMid(e.start, e.end, e.center)), end: xy(e.end) };
    case "bezier":
      return e.start[0] === e.end[0] && e.start[1] === e.end[1] && e.c1[0] === e.start[0] && e.c1[1] === e.start[1] ? null : { type: "bezier", start: xy(e.start), c1: xy(e.c1), c2: xy(e.c2), end: xy(e.end) };
    case "text_box":
      return null; // a text box needs its text first (SchShapeDialogs)
  }
}
