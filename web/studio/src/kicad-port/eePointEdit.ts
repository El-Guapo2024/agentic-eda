// The point editor of the schematic and the Symbol Editor -- `SCH_POINT_EDITOR` (eeschema/tools/sch_point_editor.cpp at KiCad 8303b2ad, the tool both frames run;
// `m_isSymbolEditor` picks the settings) with its behaviours (common/tool/point_editor_behavior.cpp, include/tool/point_editor_behavior.h): the edit points of one
// selected `SCH_SHAPE` -- a rectangle, circle, arc, bezier or polyline -- of an `SCH_TEXTBOX` and of an `SCH_RULE_AREA`, which of them the cursor is on
// (`EDIT_POINTS::FindPoint`) and what dragging one does (`EDIT_POINT::SetPosition` / `EDIT_LINE::SetPosition`, then the behaviour's `UpdateItem`); the corner
// operations (`addCorner`, `removeCorner` and their menu conditions). UI-LEFT-2 SS3 (the schematic) and YY1 (the Symbol Editor).
//
// - `RECTANGLE_POINT_EDIT_BEHAVIOR`: the four corners, the centre (`RECT_CENTER`, moves the rectangle) and the corner radius (`RECT_RADIUS`, drawn round, on the top
//   edge `radius` in from the right), and an `EDIT_LINE` per side whose middle drags that side; a corner keeps its neighbours on its sides and stays 1 mil inside the
//   opposite corner (`PinEditedCorner`). In the Symbol Editor a side's drag takes the pins whose root is on the old side along (`dragPinsOnEdge`, "Keep pins attached
//   when dragging edges", on by default; a corner's drag never does): [`EeDragResult::edge`] says which side moved, the caller moves the pins.
// - `TEXTBOX_POINT_EDIT_BEHAVIOR`: the rectangle's points; only the corners and the sides do anything (the static `UpdateItem`), and the box is kept at least
//   `GetMinSize()` (the text's height and the margins across the text's direction) and 1 mil.
// - `EDA_CIRCLE_POINT_EDIT_BEHAVIOR`: the centre (`SetCenter`: the end stays, so the radius changes) and the end (`SetEnd`).
// - `EDA_ARC_POINT_EDIT_BEHAVIOR`: start, middle, end and centre with indicator lines from the centre, in the arc edit mode the frame keeps (kicad-port/pcbPointEdit.ts `editArc`).
// - `EDA_BEZIER_POINT_EDIT_BEHAVIOR`: start, control 1, control 2, end, with indicator lines start-control 1 and control 2-end; a drag sets that point.
// - `EDA_POLYGON_POINT_EDIT_BEHAVIOR` (`BuildForPolyOutline`): a point per vertex and an `EDIT_LINE` per side, the last vertex back to the first included (an open
//   polyline has that line too); a side's drag moves both its ends by the same amount (`EDIT_LINE::SetPosition`). The schematic's tool applies no constraint
//   (`SCH_POINT_EDITOR::Main` never calls `ApplyConstraint`), so the `EC_PERPLINE` / `EC_CONVERGING` the behaviours attach do nothing here: a rectangle's side moves
//   perpendicular because `UpdateItem` reads only the one coordinate of it.
//
// Differs: `FindPoint` also finds an indicator line by its middle (dragging it changes nothing in KiCad, as no behaviour reads it); here an indicator line is no handle.
//
// Pure: no React, no DOM. Coordinates are the editor's internal nanometres, +y down (the callers convert the Symbol Editor's mm, +y up).
import { arcCenter } from "./pcbGeom";
import { editArc, type ArcEditMode } from "./pcbPointEdit";

export type P = [number, number];

/** One item the point editor works on, as its behaviour sees it. */
export type EeEditShape =
  /** `radius` absent: the item holds no corner radius (a symbol's rectangle here), so there is no `RECT_RADIUS` point. */
  | { kind: "rectangle"; start: P; end: P; radius?: number }
  /** `minSize`: `SCH_TEXTBOX::GetMinSize()` for the box as it is (the caller lays the text out). */
  | { kind: "text_box"; start: P; end: P; minSize: P }
  | { kind: "circle"; center: P; end: P }
  | { kind: "arc"; start: P; mid: P; end: P }
  | { kind: "bezier"; start: P; c1: P; c2: P; end: P }
  /** An `SCH_SHAPE` polyline (`closed` false: the schematic's and a symbol's are open chains) or an `SCH_RULE_AREA` (`ruleArea`, closed). */
  | { kind: "polygon"; pts: P[]; closed?: boolean; ruleArea?: boolean };

/** An `EDIT_POINT`; `circle`: drawn round (`SetDrawCircle`, the rectangle's radius point). */
export interface EePoint {
  pos: P;
  circle?: boolean;
}

/** An `EDIT_LINE` between points `a` and `b`: with `center` its middle is a handle (drawn round); `draw` draws the line itself (an indicator line). */
export interface EeLine {
  a: number;
  b: number;
  center: boolean;
  draw: boolean;
}

export interface EeEditPoints {
  points: EePoint[];
  lines: EeLine[];
}

export type EeHandle = { kind: "point"; index: number } | { kind: "line"; index: number };

/** `RECTANGLE_POINTS`. */
export const RECT_TOPLEFT = 0;
export const RECT_TOPRIGHT = 1;
export const RECT_BOTLEFT = 2;
export const RECT_BOTRIGHT = 3;
export const RECT_CENTER = 4;
export const RECT_RADIUS = 5;
/** `RECTANGLE_LINES`. */
export const RECT_TOP = 0;
export const RECT_RIGHT = 1;
export const RECT_BOT = 2;
export const RECT_LEFT = 3;

/** `schIUScale.MilsToIU( 1 )`. */
export const MIL_NM = 25_400;

/** `EDIT_POINT::POINT_SIZE`: `FindPoint` takes a point within this many pixels of it on either axis (`WithinPoint( aLocation, ToWorld( POINT_SIZE ) )`). */
export const POINT_SIZE_PX = 8;

const line = (a: number, b: number): EeLine => ({ a, b, center: true, draw: false });
const indicator = (a: number, b: number): EeLine => ({ a, b, center: false, draw: true });
const cp = (p: readonly [number, number]): P => [p[0], p[1]];
/** `EDIT_LINE::GetPosition`: `m_origin / 2 + m_end / 2` (integer halves). */
export const lineMid = (a: P, b: P): P => [Math.trunc(a[0] / 2) + Math.trunc(b[0] / 2), Math.trunc(a[1] / 2) + Math.trunc(b[1] / 2)];

/** `SCH_SHAPE::Normalize` of a rectangle: start the top-left corner, end the bottom-right. */
export function normalizeRect(start: P, end: P): { start: P; end: P } {
  return { start: [Math.min(start[0], end[0]), Math.min(start[1], end[1])], end: [Math.max(start[0], end[0]), Math.max(start[1], end[1])] };
}

/** `EDA_SHAPE::getCenter` of an arc kept as three points (`CalcArcCenter`), rounded to the unit. */
export function arcCenterOf(s: { start: P; mid: P; end: P }): P {
  const c = arcCenter({ start: s.start, mid: s.mid, end: s.end });
  return [Math.round(c[0]), Math.round(c[1])];
}

/** `MakePoints` of the item's behaviour (`SCH_POINT_EDITOR::makePointsAndBehavior`). */
export function eeMakePoints(s: EeEditShape): EeEditPoints {
  switch (s.kind) {
    case "rectangle":
    case "text_box": {
      // `RECTANGLE_POINT_EDIT_BEHAVIOR::MakePoints` (the text box's too), on the normalised box
      const { start: tl, end: br } = normalizeRect(s.start, s.end);
      const radius = s.kind === "rectangle" ? s.radius : 0;
      const points: EePoint[] = [{ pos: tl }, { pos: [br[0], tl[1]] }, { pos: [tl[0], br[1]] }, { pos: br }, { pos: lineMid(tl, br) }];
      // `AddPoint( VECTOR2I( botRight.x - aRect.GetCornerRadius(), topLeft.y ) )`, drawn round; the symbol's rectangle holds no radius
      if (s.kind === "text_box" || radius !== undefined) points.push({ pos: [br[0] - (radius ?? 0), tl[1]], circle: true });
      return { points, lines: [line(RECT_TOPLEFT, RECT_TOPRIGHT), line(RECT_TOPRIGHT, RECT_BOTRIGHT), line(RECT_BOTRIGHT, RECT_BOTLEFT), line(RECT_BOTLEFT, RECT_TOPLEFT)] };
    }
    case "circle":
      return { points: [{ pos: cp(s.center) }, { pos: cp(s.end) }], lines: [] };
    case "arc": {
      const c = arcCenterOf(s);
      return { points: [{ pos: cp(s.start) }, { pos: cp(s.mid) }, { pos: cp(s.end) }, { pos: c }], lines: [indicator(3, 0), indicator(3, 2)] };
    }
    case "bezier":
      return { points: [{ pos: cp(s.start) }, { pos: cp(s.c1) }, { pos: cp(s.c2) }, { pos: cp(s.end) }], lines: [indicator(0, 1), indicator(2, 3)] };
    case "polygon": {
      // `BuildForPolyOutline` of one contour: a point per vertex, a line per side, the last one back to the first
      const n = s.pts.length;
      const points = s.pts.map((p) => ({ pos: cp(p) }));
      if (n === 0) return { points, lines: [] };
      const lines: EeLine[] = [];
      for (let i = 0; i < n - 1; i++) lines.push(line(i, i + 1));
      lines.push(line(n - 1, 0));
      return { points, lines };
    }
  }
}

/** Where a handle is: a point's position, or a line's middle. */
export function handlePos(e: EeEditPoints, h: EeHandle): P {
  if (h.kind === "point") return e.points[h.index]!.pos;
  const l = e.lines[h.index]!;
  return lineMid(e.points[l.a]!.pos, e.points[l.b]!.pos);
}

/**
 * `EDIT_POINTS::FindPoint`: the first point (in `MakePoints`' order) whose square of half-side `tolerance` holds `at` (`WithinPoint`'s strict test), else the first
 * line whose middle does; null when none does. An indicator line is no handle (see the header).
 */
export function eeFindHandle(e: EeEditPoints, at: readonly [number, number], tolerance: number): EeHandle | null {
  const within = (p: P) => Math.abs(at[0] - p[0]) < tolerance && Math.abs(at[1] - p[1]) < tolerance;
  const pi = e.points.findIndex((p) => within(p.pos));
  if (pi >= 0) return { kind: "point", index: pi };
  const li = e.lines.findIndex((l) => l.center && within(lineMid(e.points[l.a]!.pos, e.points[l.b]!.pos)));
  return li >= 0 ? { kind: "line", index: li } : null;
}

/** What a drag gives: the item, and -- for a rectangle side's drag -- the side as it was and how far it went (`dragPinsOnEdge`'s `aOldEdges`, `aMoveVecs`). */
export interface EeDragResult {
  shape: EeEditShape;
  edge?: { seg: [P, P]; delta: P };
}

/** The edit points after the edited handle was set to `to` (`EDIT_POINT::SetPosition`, or `EDIT_LINE::SetPosition`: both ends by the same amount). */
export function movedPoints(e: EeEditPoints, h: EeHandle, to: readonly [number, number]): P[] {
  const pts = e.points.map((p) => cp(p.pos));
  if (h.kind === "point") {
    pts[h.index] = [to[0], to[1]];
    return pts;
  }
  const l = e.lines[h.index]!;
  const mid = lineMid(pts[l.a]!, pts[l.b]!);
  const d: P = [to[0] - mid[0], to[1] - mid[1]];
  pts[l.a] = [pts[l.a]![0] + d[0], pts[l.a]![1] + d[1]];
  pts[l.b] = [pts[l.b]![0] + d[0], pts[l.b]![1] + d[1]];
  return pts;
}

/**
 * `RECTANGLE_POINT_EDIT_BEHAVIOR::PinEditedCorner`: the edited corner stays `minW` x `minH` inside the opposite one and pushes its neighbours along; an edited side stays
 * that far from the opposite side.
 */
export function pinEditedCorner(h: EeHandle, minW: number, minH: number, c: { tl: P; tr: P; bl: P; br: P }): void {
  const { tl, tr, bl, br } = c;
  if (h.kind === "point") {
    switch (h.index) {
      case RECT_TOPLEFT:
        tl[0] = Math.min(tl[0], br[0] - minW);
        tl[1] = Math.min(tl[1], br[1] - minH);
        tr[1] = tl[1];
        bl[0] = tl[0];
        return;
      case RECT_TOPRIGHT:
        tr[0] = Math.max(tr[0], bl[0] + minW);
        tr[1] = Math.min(tr[1], bl[1] - minH);
        tl[1] = tr[1];
        br[0] = tr[0];
        return;
      case RECT_BOTLEFT:
        bl[0] = Math.min(bl[0], tr[0] - minW);
        bl[1] = Math.max(bl[1], tr[1] + minH);
        br[1] = bl[1];
        tl[0] = bl[0];
        return;
      case RECT_BOTRIGHT:
        br[0] = Math.max(br[0], tl[0] + minW);
        br[1] = Math.max(br[1], tl[1] + minH);
        bl[1] = br[1];
        tr[0] = br[0];
        return;
    }
    return;
  }
  switch (h.index) {
    case RECT_TOP:
      tl[1] = Math.min(tl[1], br[1] - minH);
      return;
    case RECT_LEFT:
      tl[0] = Math.min(tl[0], br[0] - minW);
      return;
    case RECT_BOT:
      br[1] = Math.max(br[1], tl[1] + minH);
      return;
    case RECT_RIGHT:
      br[0] = Math.max(br[0], tl[0] + minW);
      return;
  }
}

const isCorner = (h: EeHandle) => h.kind === "point" && h.index <= RECT_BOTRIGHT;

/**
 * What dragging handle `h` of `s` to `to` (the cursor, snapped by the caller: `grid->Align( .., GRID_GRAPHICS )` / `BestSnapAnchor`) makes of the item: the behaviour's
 * `UpdateItem` after the handle's `SetPosition`. `arcMode` is the frame's arc edit mode. Null when the drag makes nothing (an arc's ends on one point, a handle that names
 * nothing).
 */
export function eeDrag(s: EeEditShape, h: EeHandle, to: readonly [number, number], arcMode: ArcEditMode): EeDragResult | null {
  const e = eeMakePoints(s);
  if ((h.kind === "point" && !e.points[h.index]) || (h.kind === "line" && !e.lines[h.index]?.center)) return null;
  const pts = movedPoints(e, h, to);
  switch (s.kind) {
    case "rectangle":
    case "text_box": {
      const c = { tl: pts[RECT_TOPLEFT]!, tr: pts[RECT_TOPRIGHT]!, bl: pts[RECT_BOTLEFT]!, br: pts[RECT_BOTRIGHT]! };
      const old = normalizeRect(s.start, s.end);
      if (s.kind === "text_box") {
        // `TEXTBOX_POINT_EDIT_BEHAVIOR::UpdateItem` -> the static `RECTANGLE_POINT_EDIT_BEHAVIOR::UpdateItem( .., minSize )`: corners and sides only
        pinEditedCorner(h, Math.max(MIL_NM, s.minSize[0]), Math.max(MIL_NM, s.minSize[1]), c);
        if (isCorner(h)) return { shape: { ...s, start: c.tl, end: c.br } };
        if (h.kind !== "line") return { shape: s };
        return { shape: { ...s, ...sideMoved(old, h.index, c) } };
      }
      pinEditedCorner(h, MIL_NM, MIL_NM, c);
      // "Corner drags don't update pins."
      if (isCorner(h)) return { shape: { ...s, start: c.tl, end: c.br } };
      if (h.kind === "point" && h.index === RECT_CENTER) {
        // `m_rect.Move( center - oldBox.GetCenter() )`
        const oc = lineMid(old.start, old.end);
        const d: P = [pts[RECT_CENTER]![0] - oc[0], pts[RECT_CENTER]![1] - oc[1]];
        return { shape: { ...s, start: [old.start[0] + d[0], old.start[1] + d[1]], end: [old.end[0] + d[0], old.end[1] + d[1]] } };
      }
      if (h.kind === "point" && h.index === RECT_RADIUS) {
        // the radius point slides along the top side, between the right corner and half the shorter side in from it
        const { tl, br } = c;
        const maxRadius = Math.trunc(Math.min(Math.abs(br[0] - tl[0]), Math.abs(br[1] - tl[1])) / 2);
        const x = Math.min(Math.max(pts[RECT_RADIUS]![0], br[0] - maxRadius), br[0]);
        return { shape: { ...s, start: old.start, end: old.end, radius: br[0] - x } };
      }
      if (h.kind !== "line") return null;
      const moved = sideMoved(old, h.index, c);
      // `KIGEOM::GetSegsInDirection( oldBox, N / W / S / E )` and the move vector of that side
      const [x0, y0] = old.start;
      const [x1, y1] = old.end;
      const edge: { seg: [P, P]; delta: P } =
        h.index === RECT_TOP
          ? { seg: [[x0, y0], [x1, y0]], delta: [0, moved.start[1] - y0] }
          : h.index === RECT_LEFT
            ? { seg: [[x0, y0], [x0, y1]], delta: [moved.start[0] - x0, 0] }
            : h.index === RECT_BOT
              ? { seg: [[x0, y1], [x1, y1]], delta: [0, moved.end[1] - y1] }
              : { seg: [[x1, y0], [x1, y1]], delta: [moved.end[0] - x1, 0] };
      return { shape: { ...s, ...moved }, edge };
    }
    case "circle":
      // `isModified( CIRC_CENTER )`: `SetCenter( center )` (the end stays where it was); else `SetEnd( end )`
      return { shape: h.kind === "point" && h.index === 0 ? { ...s, center: pts[0]! } : { ...s, end: pts[1]! } };
    case "arc": {
      if (h.kind !== "point") return null;
      const role = (["start", "mid", "end", "center"] as const)[h.index]!;
      const a = editArc({ start: s.start, mid: s.mid, end: s.end }, role, [to[0], to[1]], arcMode);
      if (!a) return null;
      const r = (p: readonly [number, number]): P => [Math.round(p[0]), Math.round(p[1])];
      const out = { kind: "arc" as const, start: r(a.start), mid: r(a.mid), end: r(a.end) };
      if (out.start[0] === out.end[0] && out.start[1] === out.end[1]) return null;
      return { shape: out };
    }
    case "bezier":
      return { shape: { ...s, start: pts[0]!, c1: pts[1]!, c2: pts[2]!, end: pts[3]! } };
    case "polygon":
      // `UpdateOutlineFromPoints`: every vertex from its point
      return { shape: { ...s, pts } };
  }
}

/** The static `UpdateItem`'s side branch: `SetStartY( topLeft.y )`, `SetStartX( topLeft.x )`, `SetEndY( botRight.y )`, `SetEndX( botRight.x )`. */
function sideMoved(old: { start: P; end: P }, side: number, c: { tl: P; br: P }): { start: P; end: P } {
  const start = cp(old.start);
  const end = cp(old.end);
  if (side === RECT_TOP) start[1] = c.tl[1];
  else if (side === RECT_LEFT) start[0] = c.tl[0];
  else if (side === RECT_BOT) end[1] = c.br[1];
  else if (side === RECT_RIGHT) end[0] = c.br[0];
  return { start, end };
}

/** `SEG::Contains` of a horizontal or vertical segment (what `GetSegsInDirection` gives): `p` on it, its ends included. */
function onAxisSeg(seg: [P, P], p: P): boolean {
  const [a, b] = seg;
  if (a[1] === b[1]) return p[1] === a[1] && p[0] >= Math.min(a[0], b[0]) && p[0] <= Math.max(a[0], b[0]);
  if (a[0] === b[0]) return p[0] === a[0] && p[1] >= Math.min(a[1], b[1]) && p[1] <= Math.max(a[1], b[1]);
  return false;
}

/**
 * `RECTANGLE_POINT_EDIT_BEHAVIOR::dragPinsOnEdge` (the Symbol Editor's, with "Keep pins attached when dragging edges" on): the pins whose root (`GetPinRoot`) is on the old
 * side, its two ends excluded (`aIncludeEnds = false`), of the rectangle's unit (`GetGraphicalPins( aEdgeUnit, 0 )`: every unit for a shape common to all units). `pins`
 * gives each pin's root; the answer is the ids of the pins that move by `edge.delta`.
 */
export function pinsOnEdge(edge: { seg: [P, P]; delta: P }, pins: readonly { id: string; root: P }[]): string[] {
  if (edge.delta[0] === 0 && edge.delta[1] === 0) return [];
  const [a, b] = edge.seg;
  return pins.filter((p) => onAxisSeg(edge.seg, p.root) && !(p.root[0] === a[0] && p.root[1] === a[1]) && !(p.root[0] === b[0] && p.root[1] === b[1])).map((p) => p.id);
}

// ------------------------------------------------------------------------------------------------------------------- corners

/** `SEG::Distance` from `p` to the segment `a`-`b`. */
function segDistance(a: P, b: P, p: readonly [number, number]): number {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const len2 = dx * dx + dy * dy;
  const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2));
  return Math.hypot(a[0] + t * dx - p[0], a[1] + t * dy - p[1]);
}

/** `SCH_POINT_EDITOR::addCornerCondition`: a polyline or rule area, and the cursor within `threshold` (`ToWorld( POINT_SIZE )`) of its outline (`HitTest`). */
export function canAddCorner(s: EeEditShape, cursor: readonly [number, number], threshold: number): boolean {
  if (s.kind !== "polygon" || s.pts.length === 0) return false;
  const closed = s.closed || s.ruleArea;
  const n = s.pts.length;
  for (let i = 0; i < (closed ? n : n - 1); i++) if (segDistance(s.pts[i]!, s.pts[(i + 1) % n]!, cursor) <= threshold) return true;
  return n === 1 && Math.hypot(s.pts[0]![0] - cursor[0], s.pts[0]![1] - cursor[1]) <= threshold;
}

/**
 * `SCH_POINT_EDITOR::addCorner`: the cursor (on the grid unless it is off) goes in after the start of the side nearest it (`seg.Distance`, the first of equally near ones);
 * an open chain's closing side is not one (`if( !shape->IsClosed() ) numPoints -= 1`).
 */
export function addCorner(s: Extract<EeEditShape, { kind: "polygon" }>, cursor: readonly [number, number]): Extract<EeEditShape, { kind: "polygon" }> {
  const n = s.pts.length;
  const closed = s.closed || s.ruleArea;
  const count = closed ? n : n - 1;
  let best = Number.MAX_SAFE_INTEGER;
  let start = 0;
  for (let i = 0; i < count; i++) {
    const d = Math.trunc(segDistance(s.pts[i]!, s.pts[(i + 1) % n]!, cursor));
    if (d < best) {
      best = d;
      start = i;
    }
  }
  const pts = s.pts.map(cp);
  pts.splice(start + 1, 0, [cursor[0], cursor[1]]);
  return { ...s, pts };
}

/**
 * `SCH_POINT_EDITOR::removeCornerCondition`: a polyline with more than two points or a rule area with more than three, and the edited point (the one under the cursor)
 * on one of its vertices. The index of that vertex, or -1.
 */
export function removableCorner(s: EeEditShape, edited: P | null): number {
  if (s.kind !== "polygon" || !edited) return -1;
  if (s.ruleArea ? s.pts.length <= 3 : s.pts.length <= 2) return -1;
  return s.pts.findIndex((p) => p[0] === edited[0] && p[1] === edited[1]);
}

/** `SCH_POINT_EDITOR::removeCorner`: vertex `idx` goes; when it is the first and the chain repeats it at the end, the repeat becomes the new first vertex. */
export function removeCorner(s: Extract<EeEditShape, { kind: "polygon" }>, idx: number): Extract<EeEditShape, { kind: "polygon" }> | null {
  if (s.ruleArea ? s.pts.length <= 3 : s.pts.length <= 2) return null;
  if (idx < 0 || idx >= s.pts.length) return null;
  const pts = s.pts.map(cp);
  const last = pts.length - 1;
  if (idx === 0 && pts[0]![0] === pts[last]![0] && pts[0]![1] === pts[last]![1]) {
    pts.splice(0, 1);
    pts[last - 1] = cp(pts[0]!);
  } else {
    pts.splice(idx, 1);
  }
  return { ...s, pts };
}
