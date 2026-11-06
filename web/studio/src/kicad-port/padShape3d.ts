// The copper and the solder paste of a pad as KiCad's 3D viewer makes them: the polygon of the pad's real shape on the board, grown by a margin.
//
// Ported from KiCad 8303b2ad:
// - 3d-viewer/3d_canvas/create_3Dgraphic_brd_items.cpp `BOARD_ADAPTER::createPadWithMargin` (which shape builder a margin goes to) and `BOARD_ADAPTER::addPads`;
// - pcbnew/pad.cpp `PAD::buildEffectiveShape` (a pad's effective shapes: circle, segment, rectangle, simple polygon) and `PAD::TransformShapeToPolygon`;
// - libs/kimath/src/convert_basic_shapes_to_polygon.cpp `TransformCircleToPolygon`, `TransformOvalToPolygon`, `CornerListToPolygon`, `CornerListRemoveDuplicates`,
//   `TransformTrapezoidToPolygon`, `TransformRoundChamferedRectToPolygon` (the `ERROR_INSIDE` branches: the 3D viewer asks for no other), and
//   libs/kimath/src/geometry/geometry_utils.cpp `GetArcToSegmentCount`.
//
// The shapes are built in the pad's own frame, about its copper centre and not turned, and carried to the board by the pad's rotation, the footprint's mirror (a bottom-side
// footprint mirrors x) and the footprint's rotation (`eda_model::pad_geom::PadFrame`, the frame `eda_model::footprint::to_board` uses). A custom pad is the polygon the
// backend sends (`Pad.outline`: `eda_drc::pad_shape::pad_outline`, the union of its basic shapes). Lengths are nanometres. Pure: no three.js, no React.
import type { ChamferCorners, Pad, Part } from "../api/types";

export type Pt = [number, number];
export type Contour = Pt[];

/** `ARC_HIGH_DEF`, the board's default `m_MaxError` (0.005 mm): what `aPad->GetMaxError()` returns on a board that does not set another. */
export const MAX_ERROR_NM = 5000;

/** `MIN_SEGCOUNT_FOR_CIRCLE` (geometry_utils.h). */
const MIN_SEGCOUNT_FOR_CIRCLE = 8;

/** `KiROUND`: half away from zero. */
const kiRound = (v: number): number => (v < 0 ? -Math.round(-v) : Math.round(v));

/** `GetArcToSegmentCount( aRadius, aErrorMax, aArcAngle )`. */
export function arcToSegmentCount(radius: number, errorMax: number, arcDeg = 360): number {
  const r = Math.max(1, radius);
  const e = Math.max(1, errorMax);
  const relError = e / r;
  let arcIncrement = (180 / Math.PI) * Math.acos(1 - relError) * 2;
  arcIncrement = Math.min(360 / MIN_SEGCOUNT_FOR_CIRCLE, arcIncrement);
  const segCount = kiRound(Math.abs(arcDeg) / arcIncrement);
  return Math.max(segCount, 2);
}

/** `RotatePoint( x, y, angle )` in KiCad's y-down frame, `angle` in degrees (KiCad's sign: a positive angle turns counter-clockwise on screen). */
function rotatePoint([x, y]: Pt, deg: number): Pt {
  const a = (deg * Math.PI) / 180;
  const s = Math.sin(a);
  const c = Math.cos(a);
  return [y * s + x * c, y * c - x * s];
}

/** `TransformCircleToPolygon( aBuffer, aCenter, aRadius, aError, ERROR_INSIDE, aMinSegCount )`: the vertices on the circle, half a step off the x axis, a multiple of 8 of them. */
export function circlePolygon(center: Pt, radius: number, error: number, minSegCount = 0): Contour {
  let numSegs = Math.max(minSegCount, arcToSegmentCount(radius, error));
  numSegs = Math.floor((numSegs + 7) / 8) * 8;
  const delta = 360 / numSegs;
  const out: Contour = [];
  for (let k = 0; k < numSegs; k++) {
    const [x, y] = rotatePoint([radius, 0], delta / 2 + k * delta);
    out.push([center[0] + x, center[1] + y]);
  }
  return out;
}

/**
 * `TransformOvalToPolygon( aBuffer, aStart, aEnd, aWidth, aError, ERROR_INSIDE, aMinSegCount )`: the stadium of `width` along `start`-`end`, built along the x axis
 * from the end with the larger x, its round ends `width / 2` (`ERROR_INSIDE`: the clamp to the segment's band takes nothing off), then turned and moved.
 */
export function ovalPolygon(start: Pt, end: Pt, width: number, error: number, minSegCount = 0): Contour {
  const radius = Math.trunc(width / 2);
  let numSegs = Math.max(minSegCount, arcToSegmentCount(radius, error));
  numSegs = Math.floor((numSegs + 7) / 8) * 8;
  const delta = 360 / numSegs;
  let endp: Pt = [end[0] - start[0], end[1] - start[1]];
  let startp = start;
  if (endp[0] < 0) {
    endp = [start[0] - end[0], start[1] - end[1]];
    startp = end;
  }
  // `EDA_ANGLE delta_angle( endp )`: the angle of the vector in KiCad's sense (atan2 of y over x, y down).
  const deltaAngle = (Math.atan2(endp[1], endp[0]) * 180) / Math.PI;
  const segLen = Math.hypot(endp[0], endp[1]);
  const poly: Contour = [[segLen, radius]];
  for (let a = delta / 2; a < 180; a += delta) {
    const [x, y] = rotatePoint([0, radius], a);
    poly.push([x + segLen, y]);
  }
  poly.push([segLen, -radius], [0, -radius]);
  for (let a = delta / 2; a < 180; a += delta) poly.push(rotatePoint([0, -radius], a));
  poly.push([0, radius]);
  // `polyshape.Rotate( -delta_angle ); polyshape.Move( startp );`
  return poly.map((p) => {
    const [x, y] = rotatePoint(p, -deltaAngle);
    return [x + startp[0], y + startp[1]];
  });
}

interface RoundedCorner {
  p: Pt;
  r: number;
}

const resize = ([x, y]: Pt, len: number): Pt => {
  const n = Math.hypot(x, y);
  return n === 0 ? [0, 0] : [(x * len) / n, (y * len) / n];
};
/** `VECTOR2I::Perpendicular`: `( -y, x )`. */
const perpendicular = ([x, y]: Pt): Pt => [-y, x];

/** `SEG( a, b ).ReflectPoint( p )`: `p` mirrored in the line through `a` and `b`. */
function reflectPoint(a: Pt, b: Pt, p: Pt): Pt {
  const d: Pt = [b[0] - a[0], b[1] - a[1]];
  const l2 = d[0] * d[0] + d[1] * d[1];
  if (l2 === 0) return p;
  const t = ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2;
  const foot: Pt = [a[0] + t * d[0], a[1] + t * d[1]];
  return [2 * foot[0] - p[0], 2 * foot[1] - p[1]];
}

/**
 * `CornerListToPolygon( outline, aCorners, aInflate, aError, ERROR_INSIDE )`: a convex polygon of corners in clockwise order, each rounded by its radius (and all of
 * them by `inflate`, which also moves the corner out along the sides), the arcs at least 16 segments a turn with the first and the last the same size.
 */
export function cornerListToPolygon(corners: readonly RoundedCorner[], inflate: number, error: number): Contour {
  const out: Contour = [];
  const n = corners.length;
  if (n === 0) return out;
  let incoming: Pt = [corners[0]!.p[0] - corners[n - 1]!.p[0], corners[0]!.p[1] - corners[n - 1]!.p[1]];
  for (let i = 0; i < n; i++) {
    const cur = corners[i]!;
    const next = corners[(i + 1) % n]!;
    const outgoing: Pt = [next.p[0] - cur.p[0], next.p[1] - cur.p[1]];
    if (!(inflate || cur.r)) {
      out.push([cur.p[0], cur.p[1]]);
    } else {
      let cornerPosition: Pt = [cur.p[0], cur.p[1]];
      let radius = cur.r;
      let endAngle: number;
      let tanAngle2: number;
      if ((incoming[0] === 0 && outgoing[1] === 0) || (incoming[1] === 0 && outgoing[0] === 0)) {
        endAngle = 90;
        tanAngle2 = 1;
      } else {
        const cosNum = incoming[0] * outgoing[0] + incoming[1] * outgoing[1];
        const cosDen = Math.hypot(incoming[0], incoming[1]) * Math.hypot(outgoing[0], outgoing[1]);
        const angle = Math.acos(Math.max(-1, Math.min(1, cosNum / cosDen)));
        tanAngle2 = Math.tan((Math.PI - angle) / 2);
        endAngle = (angle * 180) / Math.PI;
      }
      if (inflate && tanAngle2) {
        radius += inflate;
        const along = resize(incoming, inflate / tanAngle2);
        const side = resize(perpendicular(incoming), -inflate);
        cornerPosition = [cornerPosition[0] + along[0] + side[0], cornerPosition[1] + along[1] + side[1]];
      }
      const numSegs = Math.max(16, arcToSegmentCount(radius, error));
      const angDelta = 360 / numSegs;
      let lastSeg = endAngle;
      if (lastSeg > 0) {
        while (lastSeg > angDelta) lastSeg -= angDelta;
      } else {
        while (lastSeg < -angDelta) lastSeg += angDelta;
      }
      let angPos = lastSeg === 0 ? angDelta : (angDelta + lastSeg) / 2;
      const arcTransitionDistance = tanAngle2 > 0 ? radius / tanAngle2 : 0;
      const back = resize(incoming, arcTransitionDistance);
      const arcStart: Pt = [cornerPosition[0] - back[0], cornerPosition[1] - back[1]];
      const toCenter = resize(perpendicular(incoming), radius);
      const arcCenter: Pt = [arcStart[0] + toCenter[0], arcStart[1] + toCenter[1]];
      const arcEnd = reflectPoint(cornerPosition, arcCenter, arcStart);
      const arcStartOrigin: Pt = [arcStart[0] - arcCenter[0], arcStart[1] - arcCenter[1]];
      out.push(arcStart);
      for (; angPos < endAngle; angPos += angDelta) {
        const [x, y] = rotatePoint(arcStartOrigin, -angPos);
        out.push([x + arcCenter[0], y + arcCenter[1]]);
      }
      out.push(arcEnd);
    }
    incoming = outgoing;
  }
  return out;
}

/** `CornerListRemoveDuplicates`: a corner at the place of the one before it goes. */
function removeDuplicates(corners: RoundedCorner[]): RoundedCorner[] {
  const out = corners.slice();
  let prev = out[0]!.p;
  for (let pos = out.length - 1; pos >= 0; pos--) {
    if (out[pos]!.p[0] === prev[0] && out[pos]!.p[1] === prev[1]) out.splice(pos, 1);
    else prev = out[pos]!.p;
  }
  return out;
}

/** `TransformTrapezoidToPolygon( aBuffer, {0,0}, aSize, ANGLE_0, aDeltaX, aDeltaY, aInflate, aError, ERROR_INSIDE )`: `deltaX`, `deltaY` are half the pad's trapezoid delta. */
export function trapezoidPolygon(size: Pt, deltaX: number, deltaY: number, inflate: number, error: number): Contour {
  let sx = Math.trunc(size[0] / 2);
  let sy = Math.trunc(size[1] / 2);
  let dX = deltaX;
  let dY = deltaY;
  let corners: RoundedCorner[] = [];
  let inf = inflate;
  if (inf < 0) {
    if (!dX && !dY) {
      sx = Math.max(1, sx + inf);
      sy = Math.max(1, sy + inf);
    } else if (dX) {
      const slope = dX / sx;
      const yShrink = kiRound((Math.hypot(sx, dX) * inf) / sx);
      sy = Math.max(1, sy + yShrink);
      sx = Math.max(1, sx + inf);
      dX = kiRound(sx * slope);
      if (dX > sy) corners = [{ p: [-sx, -sy - dX], r: 0 }, { p: [kiRound(sy / slope), 0], r: 0 }, { p: [-sx, sy + dX], r: 0 }];
    } else {
      const slope = dY / sy;
      const xShrink = kiRound((Math.hypot(sy, dY) * inf) / sy);
      sx = Math.max(1, sx + xShrink);
      sy = Math.max(1, sy + inf);
      dY = kiRound(sy * slope);
      if (dY > sx) corners = [{ p: [0, -kiRound(sx / slope)], r: 0 }, { p: [sx + dY, sy], r: 0 }, { p: [-sx - dY, sy], r: 0 }];
    }
    inf = 0;
  }
  if (corners.length === 0) {
    corners = [
      { p: [-sx + dY, -sy - dX], r: 0 },
      { p: [sx - dY, -sy + dX], r: 0 },
      { p: [sx + dY, sy - dX], r: 0 },
      { p: [-sx - dY, sy + dX], r: 0 },
    ];
    if (Math.abs(dY) === Math.abs(sx) || Math.abs(dX) === Math.abs(sy)) corners = removeDuplicates(corners);
  }
  return cornerListToPolygon(corners, inf, error);
}

/** `RECT_CHAMFER_POSITIONS` as a mask: top left 1, top right 2, bottom left 4, bottom right 8. */
export function chamferMask(c: ChamferCorners | null | undefined): number {
  if (!c) return 0;
  return (c.top_left ? 1 : 0) | (c.top_right ? 2 : 0) | (c.bottom_left ? 4 : 0) | (c.bottom_right ? 8 : 0);
}

/** `TransformRoundChamferedRectToPolygon( aBuffer, {0,0}, aSize, ANGLE_0, aCornerRadius, aChamferRatio, aChamferCorners, aInflate, aError, ERROR_INSIDE )`. */
export function roundChamferedRectPolygon(size: Pt, cornerRadius: number, chamferRatio: number, chamferCorners: number, inflate: number, error: number): Contour {
  let sx = Math.trunc(size[0] / 2);
  let sy = Math.trunc(size[1] / 2);
  let radius = cornerRadius;
  let inf = inflate;
  let chamferDeduct = 0;
  if (inf < 0) {
    sx = Math.max(1, sx + inf);
    sy = Math.max(1, sy + inf);
    chamferDeduct = inf * (2 - Math.SQRT2);
    radius = Math.max(0, radius + inf);
    inf = 0;
  }
  let corners: RoundedCorner[] = [
    { p: [-sx, -sy], r: radius },
    { p: [sx, -sy], r: radius },
    { p: [sx, sy], r: radius },
    { p: [-sx, sy], r: radius },
  ];
  if (chamferCorners) {
    const shorterSide = Math.min(size[0], size[1]);
    const chamfer = Math.max(0, kiRound(chamferRatio * shorterSide + chamferDeduct));
    // RECT_CHAMFER_TOP_LEFT, _TOP_RIGHT, _BOTTOM_RIGHT, _BOTTOM_LEFT.
    const chamId = [1, 2, 8, 4];
    const sign = [0, 1, -1, 0, 0, -1, 1, 0];
    let count = 0;
    for (let cc = 0, pos = 0; cc < 4; cc++, pos++) {
      if (!(chamferCorners & chamId[cc]!)) continue;
      count++;
      corners[pos]!.r = 0;
      if (chamfer === 0) continue;
      const base = corners[pos]!;
      const a: RoundedCorner = { p: [base.p[0] + sign[(2 * cc) & 7]! * chamfer, base.p[1] + sign[(2 * cc - 2) & 7]! * chamfer], r: 0 };
      const b: RoundedCorner = { p: [base.p[0] + sign[(2 * cc + 1) & 7]! * chamfer, base.p[1] + sign[(2 * cc - 1) & 7]! * chamfer], r: 0 };
      corners.splice(pos, 1, a, b);
      pos++;
    }
    if (count > 1 && 2 * chamfer >= shorterSide) corners = removeDuplicates(corners);
  }
  return cornerListToPolygon(corners, inf, error);
}

/** The shape fields of a pad the builder reads: a pad of the state, or one copper layer of its padstack (`stack_layers`, which carries `size` from a newer backend). */
export interface PadShapeInput {
  shape?: Pad["shape"];
  size?: [number, number] | null;
  w: number;
  h: number;
  round: boolean;
  ratio?: number | null;
  delta?: [number, number] | null;
  chamfer_ratio?: number | null;
  chamfer_corners?: ChamferCorners | null;
  outline?: [number, number][][] | null;
}

/** `PAD::GetRoundRectCornerRadius`: `KiROUND( ratio * min( size.x, size.y ) )`, the ratio clamped to [0, 0.5] (KiCad's 25 % when the pad has none). */
function cornerRadiusOf(pad: PadShapeInput, size: Pt): number {
  const ratio = Math.min(0.5, Math.max(0, pad.ratio ?? 0.25));
  return kiRound(ratio * Math.min(size[0], size[1]));
}

/** The pad's own shape name: what the state says, else what an older backend's `round` and box say. */
function shapeOf(pad: PadShapeInput): NonNullable<Pad["shape"]> {
  if (pad.shape) return pad.shape;
  if (pad.round) return pad.w === pad.h ? "circle" : "oval";
  return "rect";
}

/** `PAD::TransformShapeToPolygon( aBuffer, aLayer, aClearance, aMaxError, ERROR_INSIDE )` of a pad of `size` about its own copper centre, not turned. */
function shapePolygonLocal(pad: PadShapeInput, size: Pt, clearance: number, error: number): Contour | null {
  const shape = shapeOf(pad);
  const dx = Math.trunc(size[0] / 2);
  const dy = Math.trunc(size[1] / 2);
  // "minimal segment count to approximate a circle to create the polygonal pad shape"
  const padMinSegPerCircle = 16;
  switch (shape) {
    case "circle":
    case "oval":
      if (dx === dy || shape === "circle") return circlePolygon([0, 0], dx + clearance, error, padMinSegPerCircle);
      {
        const halfWidth = Math.min(dx, dy);
        const delta: Pt = [dx - halfWidth, dy - halfWidth];
        return ovalPolygon([-delta[0], -delta[1]], delta, (halfWidth + clearance) * 2, error, padMinSegPerCircle);
      }
    case "rect":
    case "trapezoid": {
      const d = shape === "trapezoid" ? (pad.delta ?? [0, 0]) : [0, 0];
      return trapezoidPolygon(size, Math.trunc(d[0]! / 2), Math.trunc(d[1]! / 2), clearance, error);
    }
    case "round_rect":
    case "chamfered_rect": {
      const chamfer = shape === "chamfered_rect";
      return roundChamferedRectPolygon(size, cornerRadiusOf(pad, size), chamfer ? (pad.chamfer_ratio ?? 0.2) : 0, chamfer ? chamferMask(pad.chamfer_corners) : 0, clearance, error);
    }
    case "custom":
      return null;
  }
}

/**
 * The pad's copper (margin `[0, 0]`) or its paste or mask opening (a margin) as `BOARD_ADAPTER::createPadWithMargin` builds it, as contours in the pad's own frame about
 * its copper centre (a custom pad: null, the caller takes the backend's outline). `createPadWithMargin`:
 * - a negative margin, or one that differs between x and y, on any pad but a custom one: "we apply this huge hack and fake a larger pad" -- the pad grown by the margin
 *   (`dummySize = size + clearance + clearance`; nothing when a side is no longer positive) and `TransformShapeToPolygon` of it at no clearance;
 * - otherwise the pad's effective shapes, each grown by the margin (a circle's radius, a segment's width) and the polygons inflated with round corners
 *   (`poly.Inflate( clearance.x, CORNER_STRATEGY::ROUND_ALL_CORNERS, .. )`), which for a rectangle, a trapezoid and a rounded or chamfered rectangle is the corner list
 *   built with that inflate.
 */
export function padShapeLocal(pad: PadShapeInput, margin: Pt = [0, 0], error = MAX_ERROR_NM): Contour[] | null {
  const shape = shapeOf(pad);
  if (shape === "custom") return null;
  const size: Pt = pad.size ? [pad.size[0], pad.size[1]] : [pad.w, pad.h];
  const [cx, cy] = margin;
  if (cx < 0 || cx !== cy) {
    const dummy: Pt = [size[0] + cx + cx, size[1] + cy + cy];
    if (dummy[0] <= 0 || dummy[1] <= 0) return [];
    const poly = shapePolygonLocal(pad, dummy, 0, error);
    return poly ? [poly] : null;
  }
  // `buildEffectiveShape`: a rounded rectangle that is nearly a circle is one ("For roundrect pad very near a circle, use only a circle").
  if (shape === "round_rect") {
    const r = cornerRadiusOf(pad, size);
    const minLen = 100; // pcbIUScale.mmToIU( 0.0001 )
    if (r && Math.trunc(size[0] / 2) - r < minLen && Math.trunc(size[1] / 2) - r < minLen) return [circlePolygon([0, 0], r + cx, error, 16)];
  }
  const poly = shapePolygonLocal(pad, size, cx, error);
  return poly ? [poly] : null;
}

/** Where a pad's frame lies on the board: its copper centre, the pad's own rotation (the state's degrees, clockwise as `eda_model` turns it), the footprint's mirror and rotation. */
export interface PadPlacement {
  x: number;
  y: number;
  padRotDeg: number;
  fpRotDeg: number;
  mirror: boolean;
}

/** `eda_model::pad_geom::PadFrame::to_board` about the copper centre: the pad's rotation, then the footprint's mirror (x), then its rotation, then the move. */
export function toBoard(place: PadPlacement, [x, y]: Pt): Pt {
  const a = (place.padRotDeg * Math.PI) / 180;
  let u = x * Math.cos(a) - y * Math.sin(a);
  const v = x * Math.sin(a) + y * Math.cos(a);
  if (place.mirror) u = -u;
  const f = (place.fpRotDeg * Math.PI) / 180;
  return [place.x + u * Math.cos(f) - v * Math.sin(f), place.y + u * Math.sin(f) + v * Math.cos(f)];
}

/** The placement of `pad` (or of one layer of its padstack, `at` its copper centre) on `part`. A pad from an older backend (no `size`) is drawn from its board box: no turn. */
export function padPlacement(pad: Pick<Pad, "x" | "y" | "rot" | "size">, part: Pick<Part, "rot" | "side">, at?: Pt): PadPlacement {
  const turned = !!pad.size;
  return { x: at?.[0] ?? pad.x, y: at?.[1] ?? pad.y, padRotDeg: turned ? (pad.rot ?? 0) : 0, fpRotDeg: turned ? (part.rot ?? 0) : 0, mirror: turned && part.side === "bottom" };
}

/**
 * The pad's copper, paste or mask opening on the board (nanometres), as contours to fill with the even-odd rule: [`padShapeLocal`] carried by [`toBoard`], or for a custom
 * pad the backend's outline (`Pad.outline`, already on the board; a custom pad's margin is `Inflate`d by KiCad, which the page cannot do: the outline is drawn as it is).
 * Empty when the margin leaves nothing.
 */
export function padContours(pad: PadShapeInput, place: PadPlacement, margin: Pt = [0, 0], error = MAX_ERROR_NM): Contour[] {
  const local = padShapeLocal(pad, margin, error);
  if (local === null) return (pad.outline ?? []).map((c) => c.map(([x, y]) => [x, y] as Pt));
  // A trapezoid, a chamfered rectangle or a rounded one at no margin: the backend's outline is the same polygon (`pad_outline`); the local build keeps every pad on one path.
  return local.map((c) => c.map((p) => toBoard(place, p)));
}

/** The area of a contour (positive when it winds counter-clockwise in a y-up frame), for the tests and for sorting outlines from holes. */
export function contourArea(c: readonly Pt[]): number {
  let s = 0;
  for (let i = 0, j = c.length - 1; i < c.length; j = i++) s += c[j]![0] * c[i]![1] - c[i]![0] * c[j]![1];
  return s / 2;
}

function contains(c: readonly Pt[], [x, y]: Pt): boolean {
  let inside = false;
  for (let i = 0, j = c.length - 1; i < c.length; j = i++) {
    const [xi, yi] = c[i]!;
    const [xj, yj] = c[j]!;
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

/**
 * Contours filled with the even-odd rule (a pad's outline: each piece, then its holes) as outlines with their holes: a contour inside an odd number of the others is a
 * hole of the smallest one that holds it. What a triangulator that takes an outline and its holes needs.
 */
export function nestContours(contours: readonly Contour[]): Array<{ outer: Contour; holes: Contour[] }> {
  const usable = contours.filter((c) => c.length >= 3 && Math.abs(contourArea(c)) > 0);
  const depth = usable.map((c, i) => usable.filter((o, j) => j !== i && contains(o, c[0]!)).length);
  const outers = usable.map((c, i) => ({ c, i })).filter(({ i }) => depth[i]! % 2 === 0);
  const result = outers.map(({ c }) => ({ outer: c, holes: [] as Contour[] }));
  usable.forEach((c, i) => {
    if (depth[i]! % 2 === 0) return;
    // The smallest outline that holds it.
    let best = -1;
    let bestArea = Infinity;
    outers.forEach((o, k) => {
      const area = Math.abs(contourArea(o.c));
      if (contains(o.c, c[0]!) && area < bestArea) {
        best = k;
        bestArea = area;
      }
    });
    if (best >= 0) result[best]!.holes.push(c);
  });
  return result;
}
