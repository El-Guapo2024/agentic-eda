// Port of the point editor behind pcbnew's corner actions (pcbnew/tools/pcb_point_editor.cpp and
// common/tool/point_editor_behavior.cpp at 8303b2ad): the edit points of a graphic shape, a polygon or
// a zone outline, which of them is "active" under the pointer, and what moving one to a place does --
// the segment / circle / rectangle / curve / polygon behaviours and the three arc editing modes
// (`ARC_EDIT_MODE`), plus the polygon corner operations (Remove Corner, Chamfer Corner).
//
// The studio drags zone corners itself (Canvas.tsx, kicad-port/zonePointEditor.ts); this module is what
// Move Corner To..., Move Midpoint To..., Edit Corners..., the arc modes and the shape handles use.
//
// Units: micrometres, except the arc maths which runs in nm like KiCad (`toIU` / `fromIU`).

import type { CmdShape, Shape } from "../api/types";
import { arcCenter, arcReversed, computeChamferPoints, eq, fromIU, kiRound, norm, resize, rotatePoint, segIntersect, segNearestPoint, toIU, vadd, vsub, angleOf, type Arc, type V } from "./pcbGeom";

type P = [number, number];

// ------------------------------------------------------------------------------ edit points

/** One handle: `corner` is an `EDIT_POINT` (a vertex, an end, a centre ...), `midpoint` an `EDIT_LINE` (the middle of a polygon or rectangle edge). */
export interface EditPoint {
  /** Stable name within the item: "start", "end", "mid", "center", "c1", "c2", "tl", "tr", "br", "bl", "rect-center", "v<i>", "m<i>" (the edge from vertex i to i+1), "edge-top" ... */
  id: string;
  kind: "corner" | "midpoint";
  pos: P;
}

const MIN_RECT_SIDE_UM = 26; // `Mils2IU( 1 )` (25.4 um) as whole micrometres

/** The edit points of a polygon ring: its vertices, then the midpoint of each edge (`POLYGON_POINT_EDIT_BEHAVIOR::BuildForPolyOutline`). */
export function ringEditPoints(ring: readonly P[]): EditPoint[] {
  const out: EditPoint[] = ring.map((p, i) => ({ id: `v${i}`, kind: "corner" as const, pos: [p[0], p[1]] as P }));
  for (let i = 0; i < ring.length; i++) {
    const a = ring[i]!;
    const b = ring[(i + 1) % ring.length]!;
    out.push({ id: `m${i}`, kind: "midpoint", pos: [Math.trunc((a[0] + b[0]) / 2), Math.trunc((a[1] + b[1]) / 2)] });
  }
  return out;
}

/** The edit points of a graphic shape (`makePoints`, per shape type). */
export function shapeEditPoints(s: Shape): EditPoint[] {
  const c = (id: string, p: readonly [number, number]): EditPoint => ({ id, kind: "corner", pos: [p[0], p[1]] });
  switch (s.kind) {
    case "segment":
      return [c("start", s.start), c("end", s.end)];
    case "circle":
      return [c("center", s.center), c("end", s.end)];
    case "bezier":
      return [c("start", s.start), c("c1", s.c1), c("c2", s.c2), c("end", s.end)];
    case "polygon":
      return ringEditPoints(s.pts);
    case "arc": {
      const centre = fromIU(arcCenterOf(s));
      return [c("start", s.start), c("mid", s.mid), c("end", s.end), c("center", centre)];
    }
    case "rect": {
      const x0 = Math.min(s.start[0], s.end[0]);
      const x1 = Math.max(s.start[0], s.end[0]);
      const y0 = Math.min(s.start[1], s.end[1]);
      const y1 = Math.max(s.start[1], s.end[1]);
      const mid = (a: number, b: number) => Math.trunc((a + b) / 2);
      return [
        c("tl", [x0, y0]),
        c("tr", [x1, y0]),
        c("br", [x1, y1]),
        c("bl", [x0, y1]),
        c("rect-center", [mid(x0, x1), mid(y0, y1)]),
        { id: "edge-top", kind: "midpoint", pos: [mid(x0, x1), y0] },
        { id: "edge-right", kind: "midpoint", pos: [x1, mid(y0, y1)] },
        { id: "edge-bottom", kind: "midpoint", pos: [mid(x0, x1), y1] },
        { id: "edge-left", kind: "midpoint", pos: [x0, mid(y0, y1)] },
      ];
    }
  }
}

function arcCenterOf(s: Extract<Shape, { kind: "arc" }>): V {
  const c = arcCenter({ start: toIU(s.start), mid: toIU(s.mid), end: toIU(s.end) });
  return [kiRound(c[0]), kiRound(c[1])];
}

/**
 * The handle under `at` (`EDIT_POINTS::FindPoint`: the points first, then the lines), within `tolUm`; the nearest one when several are.
 */
export function activeEditPoint(points: readonly EditPoint[], at: readonly [number, number], tolUm: number): EditPoint | null {
  const nearest = (kind: EditPoint["kind"]): EditPoint | null => {
    let best: EditPoint | null = null;
    let bestD = tolUm * tolUm;
    for (const p of points) {
      if (p.kind !== kind) continue;
      const d = (p.pos[0] - at[0]) ** 2 + (p.pos[1] - at[1]) ** 2;
      if (d <= bestD) {
        best = p;
        bestD = d;
      }
    }
    return best;
  };
  return nearest("corner") ?? nearest("midpoint");
}

// ---------------------------------------------------------------------------------- rings

/** `EDIT_POINT::SetPosition` on a polygon vertex, or `EDIT_LINE::SetPosition` on an edge (both its ends move by the same amount, so the midpoint lands on `to`). */
export function moveRingPoint(ring: readonly P[], point: EditPoint, to: readonly [number, number]): P[] | null {
  const out = ring.map((p) => [p[0], p[1]] as P);
  if (point.id.startsWith("v")) {
    const i = Number(point.id.slice(1));
    if (!out[i]) return null;
    out[i] = [to[0], to[1]];
    return out;
  }
  if (point.id.startsWith("m")) {
    const i = Number(point.id.slice(1));
    const j = (i + 1) % out.length;
    if (!out[i] || !out[j]) return null;
    const dx = to[0] - point.pos[0];
    const dy = to[1] - point.pos[1];
    out[i] = [out[i]![0] + dx, out[i]![1] + dy];
    out[j] = [out[j]![0] + dx, out[j]![1] + dy];
    return out;
  }
  return null;
}

/** `PCB_POINT_EDITOR::CanRemoveCorner` for the outline's own contour: a vertex can go while more than three remain. */
export function canRemoveCorner(ring: readonly P[]): boolean {
  return ring.length > 3;
}

/** `removeCorner`: the ring without vertex `index`; null when it would degenerate. */
export function removeCorner(ring: readonly P[], index: number): P[] | null {
  if (!canRemoveCorner(ring) || index < 0 || index >= ring.length) return null;
  return ring.filter((_, i) => i !== index).map((p) => [p[0], p[1]] as P);
}

/** The vertex nearest `at` (`chamferCorner`'s own search over the contour's vertices). */
export function nearestVertex(ring: readonly P[], at: readonly [number, number]): number {
  let best = 0;
  let bestD = Infinity;
  ring.forEach((p, i) => {
    const d = Math.hypot(p[0] - at[0], p[1] - at[1]);
    if (d < bestD) {
      bestD = d;
      best = i;
    }
  });
  return best;
}

/**
 * `chamferCorner`: vertex `index` is replaced by the two ends of a 45-degree-style chamfer, "a plausible setback that won't consume a whole
 * edge": 5 mm, but at most a quarter of either neighbouring edge. Null when the corner cannot be chamfered (collinear edges).
 */
export function chamferCorner(ring: readonly P[], index: number): P[] | null {
  const n = ring.length;
  if (n < 3 || index < 0 || index >= n) return null;
  const prev = toIU(ring[(index + n - 1) % n]!);
  const here = toIU(ring[index]!);
  const next = toIU(ring[(index + 1) % n]!);
  const segA = { a: prev, b: here };
  const segB = { a: next, b: here };
  const lenA = norm(vsub(here, prev));
  const lenB = norm(vsub(here, next));
  let setback = 5_000_000; // mmToIU( 5 )
  setback = Math.min(setback, Math.trunc(lenA * 0.25));
  setback = Math.min(setback, Math.trunc(lenB * 0.25));
  const res = computeChamferPoints(segA, segB, setback, setback);
  if (!res || !res.updatedA || !res.updatedB) return null;
  // The two chamfer ends are the new corners: along edge A first, then along edge B.
  const out = ring.map((p) => [p[0], p[1]] as P);
  out.splice(index, 1, fromIU(res.updatedA.b), fromIU(res.updatedB.b));
  return out;
}

// ------------------------------------------------------------------------------ arc editing

/** `ARC_EDIT_MODE`. */
export type ArcEditMode = "keep_center_adjust_angle_radius" | "keep_center_ends_adjust_angle" | "keep_endpoints_or_start_direction";

/** `PCBNEW_SETTINGS::m_ArcEditMode`'s default. */
export const DEFAULT_ARC_EDIT_MODE: ArcEditMode = "keep_center_adjust_angle_radius";

/** `IncrementArcEditMode`. */
export function incrementArcEditMode(mode: ArcEditMode): ArcEditMode {
  switch (mode) {
    case "keep_center_adjust_angle_radius":
      return "keep_center_ends_adjust_angle";
    case "keep_center_ends_adjust_angle":
      return "keep_endpoints_or_start_direction";
    default:
      return "keep_center_adjust_angle_radius";
  }
}

/** The action labels (`ACTIONS::pointEditorArcKeep*`) for the mode, for the confirmation toast. */
export const ARC_EDIT_MODE_LABEL: Record<ArcEditMode, string> = {
  keep_center_adjust_angle_radius: "Keep Arc Center, Adjust Radius",
  keep_center_ends_adjust_angle: "Keep Arc Radius and Center, adjust angle",
  keep_endpoints_or_start_direction: "Keep Arc Endpoints or Direction of Starting Point",
};

/** `EDA_SHAPE` arc: a centre and two ends, swept from `start` to `end` by increasing angle (`CalcArcAngles`). */
interface EdaArc {
  center: V;
  start: V;
  end: V;
}

/** `EDA_SHAPE::CalcArcAngles` + `GetArcAngle`: degrees swept from start to end, a full turn for a ring. */
function sweepOf(a: EdaArc): number {
  const a0 = angleOf(vsub(a.start, a.center));
  let a1 = angleOf(vsub(a.end, a.center));
  if (a1 === a0) a1 = a0 + 360;
  while (a1 < a0) a1 += 360;
  return a1 - a0;
}

/** `EDA_SHAPE::GetArcMid`: the start turned by half the sweep. */
function edaMid(a: EdaArc): V {
  return rotatePoint(a.start, a.center, -sweepOf(a) / 2);
}

/** `EDA_SHAPE::SetArcGeometry`: the ends are swapped when the three points wind the other way. */
function edaFromThree(arc: Arc): { arc: EdaArc; swapped: boolean } {
  const c = arcCenter(arc);
  const center: V = [kiRound(c[0]), kiRound(c[1])];
  const e: EdaArc = { center, start: arc.start, end: arc.end };
  const mid = edaMid(e);
  const dist = (mid[0] - arc.mid[0]) ** 2 + (mid[1] - arc.mid[1]) ** 2;
  const dist2 = (mid[0] - center[0]) ** 2 + (mid[1] - center[1]) ** 2;
  if (dist > dist2) return { arc: { center, start: arc.end, end: arc.start }, swapped: true };
  return { arc: e, swapped: false };
}

function edaToThree(a: EdaArc, swapped: boolean): Arc {
  const mid = edaMid(a);
  return swapped ? { start: a.end, mid, end: a.start } : { start: a.start, mid, end: a.end };
}

const radiusOf = (a: EdaArc): number => norm(vsub(a.start, a.center));

/** `editArcEndpointKeepTangent`: move one end, keeping the tangent at the other (the centre slides along the radius of the fixed end). */
function editArcEndpointKeepTangent(arc: EdaArc, center: V, start: V, mid: V, end: V): EdaArc {
  let p1: V;
  let p2: V;
  let movingStart: boolean;
  if (!eq(start, arc.start)) {
    p1 = end;
    p2 = start;
    movingStart = true;
  } else if (!eq(end, arc.end)) {
    p1 = start;
    p2 = end;
    movingStart = false;
  } else {
    return arc;
  }
  const p3 = mid;
  let v1: V = [p1[0] - center[0], p1[1] - center[1]];
  let v2: V = [p2[0] - center[0], p2[1] - center[1]];
  const v3: V = [p3[0] - center[0], p3[1] - center[1]];
  // A point cannot be both the centre and on the arc.
  const n1 = Math.hypot(v1[0], v1[1]);
  const n2 = Math.hypot(v2[0], v2[1]);
  if (n1 === 0 || n2 === 0) return arc;
  const u1: V = [v1[0] / n1, v1[1] / n1];
  const d = u1[0] * v3[0] + u1[1] * v3[1];
  let u2: V = [v3[0] - d * u1[0], v3[1] - d * u1[1]];
  const nu2 = Math.hypot(u2[0], u2[1]);
  u2 = [u2[0] / nu2, u2[1] / nu2];
  // [u1, u2] is a base centred on the circle: u1 towards the fixed point, u2 towards the mid point.
  const det = u1[0] * u2[1] - u2[0] * u1[1];
  if (det === 0 || !Number.isFinite(det)) return arc;
  v1 = [(v1[0] * u2[1] - v1[1] * u2[0]) / det, (-v1[0] * u1[1] + v1[1] * u1[0]) / det];
  v2 = [(v2[0] * u2[1] - v2[1] * u2[0]) / det, (-v2[0] * u1[1] + v2[1] * u1[0]) / det];
  const R = Math.hypot(v1[0], v1[1]);
  if (v2[0] === R) return arc; // straight line, do nothing
  let transformCircle = false;
  if (v2[0] > R) {
    // Invert the curvature: the same equation, mirrored.
    transformCircle = true;
    v2 = [2 * R - v2[0], v2[1]];
  }
  // The tangent is kept: ||C' p2|| = ||C' p1|| gives delta = ( R^2 - p2.x^2 - p2.y^2 ) / ( 2 * p2.x - 2 * R ).
  const delta = (R * R - v2[0] * v2[0] - v2[1] * v2[1]) / (2 * v2[0] - 2 * R);
  let valid = true;
  if (Math.abs(v2[1] / (R - v2[0])) > DRAW_ARC_CENTER_MAX_ANGLE) valid = false; // "limit the radius, so nothing overflows later"
  if (!Number.isFinite(delta)) valid = false;
  const v4: V = !transformCircle ? [-delta, 0] : [2 * R + delta, 0];
  const nx = v4[0] * u1[0] + v4[1] * u2[0];
  const ny = v4[0] * u1[1] + v4[1] * u2[1];
  const newCenter: V = [kiRound(nx + center[0]), kiRound(ny + center[1])];
  if (!valid) return arc;
  return movingStart ? { center: newCenter, start, end: arc.end } : { center: newCenter, start: arc.start, end };
}

/** `ADVANCED_CFG::m_DrawArcCenterMaxAngle`. */
const DRAW_ARC_CENTER_MAX_ANGLE = 50;

/** `editArcCenterKeepEndpoints`: the centre can only sit on the perpendicular bisector of the chord; snap to it (or to where the axes through the cursor meet it). */
function editArcCenterKeepEndpoints(arc: EdaArc, center: V, start: V, end: V): EdaArc {
  const snapEpsilonSq = 4;
  const m: V = [Math.trunc(start[0] / 2) + Math.trunc(end[0] / 2), Math.trunc(start[1] / 2) + Math.trunc(end[1] / 2)];
  const perp = resize([-(end[1] - start[1]), end[0] - start[0]], 1073741823); // INT_MAX / 2
  const legal = { a: vsub(m, perp), b: vadd(m, perp) };
  const tests = [
    { a: center, b: [center[0] + 1, center[1]] as V },
    { a: center, b: [center[0], center[1] + 1] as V },
  ];
  const candidates: V[] = [legal.a, legal.b];
  for (const t of tests) {
    const hit = segIntersect(legal, t, false, true);
    if (hit && segDistSq(legal, hit) <= snapEpsilonSq) candidates.push(hit);
  }
  let nearest: V | null = null;
  let minD = Infinity;
  for (const p of candidates) {
    const d = (p[0] - center[0]) ** 2 + (p[1] - center[1]) ** 2;
    if (d < minD - snapEpsilonSq) {
      minD = d;
      nearest = p;
    }
  }
  return nearest ? { ...arc, center: nearest } : arc;
}

function segDistSq(s: { a: V; b: V }, p: V): number {
  const n = segNearestPoint(s, p);
  return (n[0] - p[0]) ** 2 + (n[1] - p[1]) ** 2;
}

/** `editArcEndpointKeepCenter`: move one end around the centre; the other end follows to the same radius. */
function editArcEndpointKeepCenter(arc: EdaArc, center: V, start: V, end: V): EdaArc {
  const minRadius = 25_400; // Mils2IU( 1 )
  let movingStart: boolean;
  let p1: V;
  let p2: V;
  let prevP1: V;
  if (!eq(start, arc.start)) {
    prevP1 = arc.start;
    p1 = start;
    p2 = end;
    movingStart = true;
  } else {
    prevP1 = arc.end;
    p1 = end;
    p2 = start;
    movingStart = false;
  }
  p1 = vsub(p1, center);
  p2 = vsub(p2, center);
  if (p1[0] === 0 && p1[1] === 0) p1 = vsub(prevP1, center);
  if (p2[0] === 0 && p2[1] === 0) p2 = [1, 0];
  let radius = Math.hypot(p1[0], p1[1]);
  if (radius < minRadius) radius = minRadius;
  p1 = vadd(center, resize(p1, kiRound(radius)));
  p2 = vadd(center, resize(p2, kiRound(radius)));
  return movingStart ? { center, start: p1, end: p2 } : { center, start: p2, end: p1 };
}

/** `editArcEndpointKeepCenterAndRadius`: move one end around the circle; the radius is kept. */
function editArcEndpointKeepCenterAndRadius(arc: EdaArc, center: V, start: V, end: V): EdaArc {
  const radius = radiusOf(arc);
  if (!eq(start, arc.start)) return { ...arc, start: vadd(center, resize(vsub(start, center), radius)) };
  return { ...arc, end: vadd(center, resize(vsub(end, center), radius)) };
}

/** `editArcMidKeepCenter`: the radius becomes the cursor's distance from the centre; both ends are moved onto it. */
function editArcMidKeepCenter(arc: EdaArc, center: V, start: V, end: V, cursor: V): EdaArc {
  const minRadius = 25_400;
  let radius = Math.hypot(cursor[0] - center[0], cursor[1] - center[1]);
  if (radius < minRadius) radius = minRadius;
  const r = kiRound(radius);
  return { ...arc, start: vadd(resize(vsub(start, center), r), center), end: vadd(resize(vsub(end, center), r), center) };
}

/** `editArcMidKeepEndpoints`: the mid point slides along the ray from the chord's middle through the old mid point ("we do not allow arc inflection"). Returns the arc as three points. */
function editArcMidKeepEndpoints(arc: EdaArc, start: V, end: V, cursor: V): Arc {
  const m: V = [Math.trunc((start[0] + end[0]) / 2), Math.trunc((start[1] + end[1]) / 2)];
  const justOff = Math.trunc(norm(vsub(start, end)) / 100);
  const v = vsub(edaMid(arc), m);
  const legal = { a: vadd(m, resize(v, justOff)), b: vadd(m, resize(v, 1073741823)) };
  const mid = segNearestPoint(legal, cursor);
  return { start, mid, end };
}

/**
 * What moving the arc handle `role` to `to` does (`EDA_ARC_POINT_EDIT_BEHAVIOR::UpdateItem`), as a three-point arc in nm. The typed position
 * stands in for the pointer wherever the C++ reads the cursor. Null when the move is not possible.
 */
export function editArc(arc: Arc, role: "start" | "mid" | "end" | "center", to: V, mode: ArcEditMode): Arc | null {
  const { arc: e, swapped } = edaFromThree(arc);
  const internal = (r: typeof role): typeof role => (r === "start" ? (swapped ? "end" : "start") : r === "end" ? (swapped ? "start" : "end") : r);
  const pts = { center: e.center, mid: edaMid(e), start: e.start, end: e.end };
  pts[internal(role)] = to;
  if (role === "center") {
    if (mode === "keep_endpoints_or_start_direction") return edaToThree(editArcCenterKeepEndpoints(e, pts.center, pts.start, pts.end), swapped);
    // Both the other modes just move the arc.
    const d = vsub(to, e.center);
    return edaToThree({ center: to, start: vadd(e.start, d), end: vadd(e.end, d) }, swapped);
  }
  if (role === "mid") {
    if (mode === "keep_endpoints_or_start_direction") {
      const a = editArcMidKeepEndpoints(e, pts.start, pts.end, to);
      return swapped ? arcReversed(a) : a;
    }
    return edaToThree(editArcMidKeepCenter(e, pts.center, pts.start, pts.end, to), swapped);
  }
  switch (mode) {
    case "keep_center_adjust_angle_radius":
      return edaToThree(editArcEndpointKeepCenter(e, pts.center, pts.start, pts.end), swapped);
    case "keep_center_ends_adjust_angle":
      return edaToThree(editArcEndpointKeepCenterAndRadius(e, pts.center, pts.start, pts.end), swapped);
    case "keep_endpoints_or_start_direction":
      return edaToThree(editArcEndpointKeepTangent(e, pts.center, pts.start, pts.mid, pts.end), swapped);
  }
}

// ----------------------------------------------------------------------------------- shapes

const cp = (p: readonly [number, number]) => ({ x: p[0], y: p[1] });

/**
 * What moving the edit point `point` of `shape` to `to` gives (`UpdateItem` of the shape's behaviour), as the shape `add_shape` takes; null when it is
 * not possible (a rectangle cannot shrink below 1 mil, an arc handle that makes no arc).
 */
export function moveShapePoint(shape: Shape, point: EditPoint, to: readonly [number, number], mode: ArcEditMode = DEFAULT_ARC_EDIT_MODE): CmdShape | null {
  const common = { layer: shape.layer, stroke_width: shape.stroke_width, filled: shape.filled };
  const t: P = [to[0], to[1]];
  switch (shape.kind) {
    case "segment": {
      if (point.id === "start") return { ...common, kind: "segment", start: cp(t), end: cp(shape.end) };
      if (point.id === "end") return { ...common, kind: "segment", start: cp(shape.start), end: cp(t) };
      return null;
    }
    case "circle": {
      // `SetCenter` moves only the centre; `SetEnd` the point on the circumference.
      if (point.id === "center") return { ...common, kind: "circle", center: cp(t), end: cp(shape.end) };
      if (point.id === "end") return { ...common, kind: "circle", center: cp(shape.center), end: cp(t) };
      return null;
    }
    case "bezier": {
      const next = { start: shape.start, c1: shape.c1, c2: shape.c2, end: shape.end } as Record<string, [number, number]>;
      if (!(point.id in next)) return null;
      next[point.id] = t;
      return { ...common, kind: "bezier", start: cp(next.start!), c1: cp(next.c1!), c2: cp(next.c2!), end: cp(next.end!) };
    }
    case "polygon": {
      const ring = moveRingPoint(shape.pts, point, to);
      return ring ? { ...common, kind: "polygon", pts: ring.map(cp) } : null;
    }
    case "rect":
      return moveRectPoint(shape, point, t);
    case "arc": {
      const role = point.id as "start" | "mid" | "end" | "center";
      if (role !== "start" && role !== "mid" && role !== "end" && role !== "center") return null;
      const a = editArc({ start: toIU(shape.start), mid: toIU(shape.mid), end: toIU(shape.end) }, role, toIU(t), mode);
      if (!a) return null;
      const [s, m, e] = [fromIU(a.start), fromIU(a.mid), fromIU(a.end)];
      if (eq(s, e)) return null;
      return { ...common, kind: "arc", start: cp(s), mid: cp(m), end: cp(e) };
    }
  }
}

/** `RECTANGLE_POINT_EDIT_BEHAVIOR::UpdateItem` + `PinEditedCorner`: a corner keeps its adjacent corners on its lines and stays 1 mil inside the opposite one; an edge moves one side; the centre moves the rectangle. */
function moveRectPoint(shape: Extract<Shape, { kind: "rect" }>, point: EditPoint, to: P): CmdShape | null {
  let x0 = Math.min(shape.start[0], shape.end[0]);
  let x1 = Math.max(shape.start[0], shape.end[0]);
  let y0 = Math.min(shape.start[1], shape.end[1]);
  let y1 = Math.max(shape.start[1], shape.end[1]);
  const min = MIN_RECT_SIDE_UM;
  switch (point.id) {
    case "tl":
      x0 = Math.min(to[0], x1 - min);
      y0 = Math.min(to[1], y1 - min);
      break;
    case "tr":
      x1 = Math.max(to[0], x0 + min);
      y0 = Math.min(to[1], y1 - min);
      break;
    case "bl":
      x0 = Math.min(to[0], x1 - min);
      y1 = Math.max(to[1], y0 + min);
      break;
    case "br":
      x1 = Math.max(to[0], x0 + min);
      y1 = Math.max(to[1], y0 + min);
      break;
    case "edge-top":
      y0 = Math.min(to[1], y1 - min);
      break;
    case "edge-bottom":
      y1 = Math.max(to[1], y0 + min);
      break;
    case "edge-left":
      x0 = Math.min(to[0], x1 - min);
      break;
    case "edge-right":
      x1 = Math.max(to[0], x0 + min);
      break;
    case "rect-center": {
      const dx = to[0] - Math.trunc((x0 + x1) / 2);
      const dy = to[1] - Math.trunc((y0 + y1) / 2);
      x0 += dx;
      x1 += dx;
      y0 += dy;
      y1 += dy;
      break;
    }
    default:
      return null;
  }
  return { kind: "rect", layer: shape.layer, stroke_width: shape.stroke_width, filled: shape.filled, start: { x: x0, y: y0 }, end: { x: x1, y: y1 } };
}

// ------------------------------------------------------------------------------ conversions

/** A shape `add_shape` takes, as the display shape (tuple points) the canvas draws: the live preview of a handle drag. */
export function cmdShapeToShape(c: CmdShape, id: string): Shape {
  const common = { id, layer: c.layer, stroke_width: c.stroke_width, filled: c.filled };
  const t = (p: { x: number; y: number }): [number, number] => [p.x, p.y];
  switch (c.kind) {
    case "segment":
      return { ...common, kind: "segment", start: t(c.start), end: t(c.end) };
    case "rect":
      return { ...common, kind: "rect", start: t(c.start), end: t(c.end) };
    case "arc":
      return { ...common, kind: "arc", start: t(c.start), mid: t(c.mid), end: t(c.end) };
    case "circle":
      return { ...common, kind: "circle", center: t(c.center), end: t(c.end) };
    case "polygon":
      return { ...common, kind: "polygon", pts: c.pts.map(t) };
    case "bezier":
      return { ...common, kind: "bezier", start: t(c.start), c1: t(c.c1), c2: t(c.c2), end: t(c.end) };
  }
}

/** The display shape as `add_shape` takes it (the backend assigns the id). */
export function shapeToCmd(s: Shape): CmdShape {
  const common = { layer: s.layer, stroke_width: s.stroke_width, filled: s.filled };
  switch (s.kind) {
    case "segment":
      return { ...common, kind: "segment", start: cp(s.start), end: cp(s.end) };
    case "rect":
      return { ...common, kind: "rect", start: cp(s.start), end: cp(s.end) };
    case "arc":
      return { ...common, kind: "arc", start: cp(s.start), mid: cp(s.mid), end: cp(s.end) };
    case "circle":
      return { ...common, kind: "circle", center: cp(s.center), end: cp(s.end) };
    case "polygon":
      return { ...common, kind: "polygon", pts: s.pts.map(cp) };
    case "bezier":
      return { ...common, kind: "bezier", start: cp(s.start), c1: cp(s.c1), c2: cp(s.c2), end: cp(s.end) };
  }
}
