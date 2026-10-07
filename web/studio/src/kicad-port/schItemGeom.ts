// Geometry of the schematic's drawn items (`SchGraphic`: rectangles, circles, arcs, beziers, polygons, text
// boxes, rule areas, directive labels) for hit-testing, box selection and bounds -- ported from
// `EDA_SHAPE::hitTest` / `SCH_SHAPE::HitTest` / `SCH_TEXTBOX::HitTest`, `EDA_SHAPE::computeArcCenter` and
// `SCH_DIRECTIVE_LABEL::CreateGraphicShape` (eeschema at 8303b2ad). Pure: no DOM, no fonts (a text box's text
// does not take part in hit-testing, only its border, like any unfilled shape).
import { bezierPolyline } from "./bezierPoly";
import type { SchGraphic } from "../api/schEditTypes";

export type P = readonly [number, number];

export interface Box {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

export const pt = (p: { x: number; y: number }): P => [p.x, p.y];

export function boxOfPoints(pts: readonly P[]): Box | null {
  if (pts.length === 0) return null;
  let minX = Infinity,
    minY = Infinity,
    maxX = -Infinity,
    maxY = -Infinity;
  for (const [x, y] of pts) {
    minX = Math.min(minX, x);
    minY = Math.min(minY, y);
    maxX = Math.max(maxX, x);
    maxY = Math.max(maxY, y);
  }
  return { minX, minY, maxX, maxY };
}

export function inflateBox(b: Box, d: number): Box {
  return { minX: b.minX - d, minY: b.minY - d, maxX: b.maxX + d, maxY: b.maxY + d };
}

export function unionBox(a: Box | null, b: Box | null): Box | null {
  if (!a) return b;
  if (!b) return a;
  return { minX: Math.min(a.minX, b.minX), minY: Math.min(a.minY, b.minY), maxX: Math.max(a.maxX, b.maxX), maxY: Math.max(a.maxY, b.maxY) };
}

/** `item` overlaps `box` at all -- the "crossing" marquee rule. */
export function boxesOverlap(item: Box, box: Box): boolean {
  return item.minX < box.maxX && item.maxX > box.minX && item.minY < box.maxY && item.maxY > box.minY;
}

/** `item` sits entirely inside `box` -- the "enclosed" marquee rule. */
export function boxEncloses(box: Box, item: Box): boolean {
  return item.minX >= box.minX && item.maxX <= box.maxX && item.minY >= box.minY && item.maxY <= box.maxY;
}

export function pointInBox(p: P, b: Box): boolean {
  return p[0] >= b.minX && p[0] <= b.maxX && p[1] >= b.minY && p[1] <= b.maxY;
}

export function distPointSegment(p: P, a: P, b: P): number {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const lenSq = dx * dx + dy * dy;
  let t = lenSq === 0 ? 0 : ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / lenSq;
  t = Math.max(0, Math.min(1, t));
  return Math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy));
}

/** Distance from `p` to a polyline (`closed` also counts the closing segment). */
export function distToPolyline(p: P, pts: readonly P[], closed: boolean): number {
  let best = Infinity;
  const n = pts.length;
  for (let i = 0; i + 1 < n; i++) best = Math.min(best, distPointSegment(p, pts[i]!, pts[i + 1]!));
  if (closed && n > 2) best = Math.min(best, distPointSegment(p, pts[n - 1]!, pts[0]!));
  if (n === 1) best = Math.hypot(p[0] - pts[0]![0], p[1] - pts[0]![1]);
  return best;
}

/** Even-odd point-in-polygon. */
export function pointInPolygon(p: P, poly: readonly P[]): boolean {
  let inside = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const [xi, yi] = poly[i]!;
    const [xj, yj] = poly[j]!;
    if (yi > p[1] !== yj > p[1] && p[0] < ((xj - xi) * (p[1] - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

export interface ArcShape {
  center: P;
  radius: number;
  /** Radians, atan2 of the start radius vector. */
  startAngle: number;
  /** Signed sweep, radians: start to end through `mid`. */
  sweep: number;
}

/** The circle through three points as an arc from `start` through `mid` to `end`; null for collinear points. */
export function arcFromThreePoints(start: P, mid: P, end: P): ArcShape | null {
  const [ax, ay] = start;
  const [bx, by] = mid;
  const [cx, cy] = end;
  const d = 2 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
  if (Math.abs(d) < 1e-9) return null;
  const ux = ((ax * ax + ay * ay) * (by - cy) + (bx * bx + by * by) * (cy - ay) + (cx * cx + cy * cy) * (ay - by)) / d;
  const uy = ((ax * ax + ay * ay) * (cx - bx) + (bx * bx + by * by) * (ax - cx) + (cx * cx + cy * cy) * (bx - ax)) / d;
  const center: P = [ux, uy];
  const radius = Math.hypot(ax - ux, ay - uy);
  const aStart = Math.atan2(ay - uy, ax - ux);
  const aMid = Math.atan2(by - uy, bx - ux);
  const aEnd = Math.atan2(cy - uy, cx - ux);
  const norm = (a: number) => ((a % (2 * Math.PI)) + 2 * Math.PI) % (2 * Math.PI);
  // Direction start -> end that passes through mid: positive (increasing angle) when mid lies within the CCW sweep.
  const ccwToMid = norm(aMid - aStart);
  const ccwToEnd = norm(aEnd - aStart);
  const sweep = ccwToMid <= ccwToEnd ? ccwToEnd : ccwToEnd - 2 * Math.PI;
  return { center, radius, startAngle: aStart, sweep };
}

/** Points along an arc, at most `maxStepRad` apart. */
export function arcPoints(arc: ArcShape, maxStepRad = Math.PI / 24): P[] {
  const steps = Math.max(2, Math.ceil(Math.abs(arc.sweep) / maxStepRad));
  const out: P[] = [];
  for (let i = 0; i <= steps; i++) {
    const a = arc.startAngle + (arc.sweep * i) / steps;
    out.push([arc.center[0] + arc.radius * Math.cos(a), arc.center[1] + arc.radius * Math.sin(a)]);
  }
  return out;
}

/** A rectangle's four corners, in drawing order, from its two drawn corners. */
export function rectCorners(a: P, b: P): P[] {
  return [a, [b[0], a[1]], b, [a[0], b[1]]];
}

/** `SCH_DIRECTIVE_LABEL::m_symbolSize` (20 mil) -- um. */
export const DIRECTIVE_SYMBOL_SIZE_UM = 508;

/**
 * `SCH_DIRECTIVE_LABEL::CreateGraphicShape`: the flag's outline points -- the pole along the local +y axis, then
 * the flag's own outline -- rotated by the spin style and moved to `at`. `orientation` is the file angle in
 * degrees: 0 right, 90 up, 180 left, 270 bottom.
 */
export function directiveShape(at: P, orientation: number, shape: "dot" | "round" | "diamond" | "rectangle", pinLength: number): { poleEnd: P; outline: P[]; flagCenter: P; flagRadius: number } {
  const S = DIRECTIVE_SYMBOL_SIZE_UM;
  const L = pinLength;
  // `F_DOT` shrinks the symbol to 0.7, `F_RECTANGLE` to 0.8 (CreateGraphicShape's own `symbolSize` adjustments).
  const symbol = shape === "dot" ? Math.round(S * 0.7) : shape === "rectangle" ? Math.round(S * 0.8) : S;
  let local: P[] = [];
  switch (shape) {
    case "dot":
    case "round":
      local = [
        [0, 0],
        [0, L - symbol],
        [0, L],
        [-S, L],
        [0, L],
        [S, L + symbol],
      ];
      break;
    case "diamond":
      local = [
        [0, 0],
        [0, L - symbol],
        [-2 * S, L],
        [0, L + symbol],
        [2 * S, L],
        [0, L - symbol],
        [0, 0],
      ];
      break;
    case "rectangle":
      local = [
        [0, 0],
        [0, L - symbol],
        [-2 * symbol, L - symbol],
        [-2 * symbol, L + symbol],
        [2 * symbol, L + symbol],
        [2 * symbol, L - symbol],
        [0, L - symbol],
        [0, 0],
      ];
      break;
  }
  const spin = ((Math.round(orientation) % 360) + 360) % 360;
  // RotatePoint(point, angle) in KiCad's y-down frame: (x, y) -> (x cos + y sin, y cos - x sin).
  const rot = (p: P, deg: number): P => {
    const c = Math.cos((deg * Math.PI) / 180);
    const s = Math.sin((deg * Math.PI) / 180);
    return [p[0] * c + p[1] * s, p[1] * c - p[0] * s];
  };
  const turn = spin === 90 ? -90 : spin === 0 ? 180 : spin === 270 ? 90 : 0; // UP, RIGHT, BOTTOM, LEFT
  const place = (p: P): P => {
    const r = Math.abs(turn) === 0 ? p : rot(p, turn);
    return [r[0] + at[0], r[1] + at[1]];
  };
  const outline = local.map(place);
  const flagCenter = place([0, L]);
  return { poleEnd: place([0, L]), outline, flagCenter, flagRadius: shape === "dot" ? symbol : S };
}

function strokeSlack(g: SchGraphic): number {
  return Math.max(g.width_um ?? 0, 152.4) / 2;
}

/** The shape's outline as a polyline: `closed` for shapes that enclose an area. */
export function graphicOutline(g: SchGraphic): { pts: P[]; closed: boolean } {
  const s = g.shape;
  switch (s.type) {
    case "rectangle":
      return { pts: rectCorners(pt(s.start), pt(s.end)), closed: true };
    case "text_box":
      return { pts: rectCorners(pt(s.start), pt(s.end)), closed: true };
    case "circle": {
      const c = pt(s.center);
      const out: P[] = [];
      for (let i = 0; i < 48; i++) out.push([c[0] + s.radius_um * Math.cos((i / 48) * 2 * Math.PI), c[1] + s.radius_um * Math.sin((i / 48) * 2 * Math.PI)]);
      return { pts: out, closed: true };
    }
    case "arc": {
      const arc = arcFromThreePoints(pt(s.start), pt(s.mid), pt(s.end));
      return { pts: arc ? arcPoints(arc) : [pt(s.start), pt(s.mid), pt(s.end)], closed: false };
    }
    case "bezier":
      return { pts: bezierPolyline(pt(s.start), pt(s.c1), pt(s.c2), pt(s.end)), closed: false };
    case "polygon":
    case "rule_area":
      return { pts: s.pts.map(pt), closed: true };
    case "directive": {
      const d = directiveShape(pt(s.at), (s.orientation ?? 0) / 1000, s.shape ?? "round", s.pin_length_um);
      return { pts: d.outline, closed: false };
    }
  }
}

export function graphicBounds(g: SchGraphic): Box {
  const slack = strokeSlack(g);
  const { pts } = graphicOutline(g);
  const b = boxOfPoints(pts) ?? { minX: 0, minY: 0, maxX: 0, maxY: 0 };
  if (g.shape.type === "directive") {
    const d = directiveShape(pt(g.shape.at), (g.shape.orientation ?? 0) / 1000, g.shape.shape ?? "round", g.shape.pin_length_um);
    return inflateBox(unionBox(b, boxOfPoints([d.flagCenter]))!, DIRECTIVE_SYMBOL_SIZE_UM * 2);
  }
  return inflateBox(b, slack);
}

/** `EDA_SHAPE::hitTest( point )`: on the outline within `tol`, or -- for a filled shape -- anywhere inside. */
export function graphicHit(g: SchGraphic, p: P, tol: number): boolean {
  const slack = tol + strokeSlack(g);
  const s = g.shape;
  if (s.type === "directive") {
    const d = directiveShape(pt(s.at), (s.orientation ?? 0) / 1000, s.shape ?? "round", s.pin_length_um);
    if (distToPolyline(p, d.outline, false) <= slack + DIRECTIVE_SYMBOL_SIZE_UM / 2) return true;
    return Math.hypot(p[0] - d.flagCenter[0], p[1] - d.flagCenter[1]) <= d.flagRadius + tol;
  }
  if (s.type === "circle") {
    const dist = Math.hypot(p[0] - s.center.x, p[1] - s.center.y);
    if (Math.abs(dist - s.radius_um) <= slack) return true;
    return (g.fill ?? "none") !== "none" && dist <= s.radius_um;
  }
  const { pts, closed } = graphicOutline(g);
  if (distToPolyline(p, pts, closed) <= slack) return true;
  const filled = (g.fill ?? "none") !== "none";
  return filled && closed && pointInPolygon(p, pts);
}
