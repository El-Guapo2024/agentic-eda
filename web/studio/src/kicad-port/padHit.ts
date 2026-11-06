// A pad as the selection tool measures it (pcbnew/pad.cpp and pcbnew/tools/pcb_selection_tool.cpp at 8303b2ad), UI-LEFT-2 PP2:
//
//   PAD::HitTest( aPosition, aAccuracy )          the effective polygon of each of its layers' copper (`GetEffectivePolygon( l, ERROR_INSIDE )->Contains(
//                                                  aPosition, -1, aAccuracy )`, `Padstack().ForEachUniqueLayer`), or its hole (`GetEffectiveHoleShape()->Collide`)
//   PCB_SELECTION_TOOL::hitTestDistance            `case PCB_PAD_T`: the least distance to `GetEffectiveShape( aLayer )` over the unique layers
//   FOOTPRINT::GetCoverageArea                     `case PCB_PAD_T`: the area of the union of each unique layer's `TransformShapeToPolygon( .., ERROR_OUTSIDE )`
//
// The effective shape is the pad's real outline, not its box: a circle, an oval's stadium along its long axis, a rectangle turned with the pad, a rounded
// rectangle with corners of `GetRoundRectCornerRadius()` (the ratio times the smaller side, `PAD::GetRoundRectCornerRadius`), and a trapezoid, a chamfered
// rectangle or a custom pad the contours of its copper (`Pad.outline`, the DRC's `pad_outline`). So a click in the corner of a rounded pad's box, outside its
// arc, misses it, as in KiCad. The distances here are exact (no chords); the coverage polygons are cut into chords outside the curve (`ERROR_OUTSIDE`,
// `ARC_LOW_DEF`) as `TransformShapeToPolygon` cuts them.
//
// A padstack whose copper differs from layer to layer (`Pad.stack_layers`) is measured on each group of layers too; such a group carries the board box of its
// copper, not its size, so a group turned off the quarter turns is measured by that box (the studio's state has nothing better). Pure: no store, no DOM.
import type { Pad, PadShapeName } from "../api/types";
import { outlineContains, outlineDistance } from "./padOutline";
import { ARC_LOW_DEF_NM, circleRing, circleSegments, intersectionArea, stadiumRing, type Pt, type Ring } from "./coverage";

/** What a pad needs to be measured: its board box and roundness at least (an older backend's pad), its shape, size, turn and contours when the state has them. */
export type PadLike = Pick<Pad, "x" | "y" | "w" | "h" | "round"> & Partial<Pad>;

/** One layer's copper of a pad, in the pad's own frame: its centre on the board, its size un-turned, its turn (clockwise degrees on the y-down board). */
export interface PadCopper {
  x: number;
  y: number;
  shape: PadShapeName | "box";
  w: number;
  h: number;
  rot: number;
  /** A rounded rectangle's corner ratio (`GetRoundRectRadiusRatio`). */
  ratio: number;
  outline: readonly (readonly (readonly [number, number])[])[] | null;
}

/** KiCad's default corner ratio of a rounded rectangle (`PAD::PAD`: `SetRoundRectRadiusRatio( 0.25 )`), for a pad the state gives none. */
const DEFAULT_RR_RATIO = 0.25;

/** The shapes whose copper is the `outline` contours (`eda_drc::pad_shape::pad_outline`). */
const OUTLINED: ReadonlySet<string> = new Set(["trapezoid", "chamfered_rect", "custom"]);

/** The size a group of a padstack had before its turn, from its board box: exact on a quarter turn, null off them. */
function untilted(w: number, h: number, rot: number): [number, number] | null {
  const q = (((rot % 360) + 360) % 360) / 90;
  if (Math.abs(q - Math.round(q)) > 1e-9) return null;
  return Math.round(q) % 2 === 0 ? [w, h] : [h, w];
}

/** Each unique layer's copper of `pad` (`Padstack().ForEachUniqueLayer`): the front, then the padstack's other groups. A pad the state gives no size is its box. */
export function padCoppers(pad: PadLike): PadCopper[] {
  const rot = pad.rot ?? 0;
  const front: PadCopper = pad.size
    ? { x: pad.x, y: pad.y, shape: pad.shape ?? (pad.round ? (pad.size[0] === pad.size[1] ? "circle" : "oval") : "rect"), w: pad.size[0], h: pad.size[1], rot, ratio: pad.ratio ?? DEFAULT_RR_RATIO, outline: pad.outline ?? null }
    : { x: pad.x, y: pad.y, shape: pad.outline?.length ? "custom" : pad.round ? (pad.w === pad.h ? "circle" : "oval") : "box", w: pad.w, h: pad.h, rot: 0, ratio: DEFAULT_RR_RATIO, outline: pad.outline ?? null };
  const out = [front];
  for (const l of pad.stack_layers ?? []) {
    const size = untilted(l.w, l.h, rot);
    out.push(
      size
        ? { x: l.x, y: l.y, shape: l.shape, w: size[0], h: size[1], rot, ratio: l.ratio ?? DEFAULT_RR_RATIO, outline: l.outline ?? null }
        : { x: l.x, y: l.y, shape: l.outline?.length ? "custom" : "box", w: l.w, h: l.h, rot: 0, ratio: DEFAULT_RR_RATIO, outline: l.outline ?? null }
    );
  }
  return out;
}

/** (x, y) in a copper's frame: about its centre, turned back by its rotation. */
function local(c: PadCopper, x: number, y: number): [number, number] {
  const a = (c.rot * Math.PI) / 180;
  const [cos, sin] = [Math.cos(a), Math.sin(a)];
  const [dx, dy] = [x - c.x, y - c.y];
  return [dx * cos + dy * sin, -dx * sin + dy * cos];
}

/** The signed distance to a rectangle of half sides `hw`, `hh` with corners of radius `r` about the origin: negative inside, 0 on its edge. */
function roundedBoxDistance(u: number, v: number, hw: number, hh: number, r: number): number {
  const qx = Math.abs(u) - (hw - r);
  const qy = Math.abs(v) - (hh - r);
  return Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) + Math.min(Math.max(qx, qy), 0) - r;
}

/** How far (x, y) is outside one layer's copper: negative or 0 on it (a polygon's is 0 anywhere on it). */
export function copperDistance(c: PadCopper, x: number, y: number): number {
  if (c.outline?.length && (OUTLINED.has(c.shape) || c.shape === "box")) return outlineDistance(c.outline, x, y);
  const [u, v] = local(c, x, y);
  const hw = c.w / 2;
  const hh = c.h / 2;
  switch (c.shape) {
    case "circle":
      return Math.hypot(u, v) - Math.max(hw, hh);
    case "oval":
      // the two end circles of the stadium lie on the pad's long axis
      return roundedBoxDistance(u, v, hw, hh, Math.min(hw, hh));
    case "round_rect":
      // `GetRoundRectCornerRadius()`: the ratio of the smaller side, at most half of it
      return roundedBoxDistance(u, v, hw, hh, Math.min(Math.max(c.ratio, 0), 0.5) * Math.min(c.w, c.h));
    default:
      // a rectangle, a box, and a polygon shape the state gave no contours for: the rectangle of its size
      return roundedBoxDistance(u, v, hw, hh, 0);
  }
}

/** How far (x, y) is from a pad's hole (`GetEffectiveHoleShape()`: the drill's circle or a slot's stadium about the pad's position); Infinity with none. */
export function holeDistance(pad: PadLike, x: number, y: number): number {
  const cx = pad.px ?? pad.x;
  const cy = pad.py ?? pad.y;
  if (pad.slot && pad.slot[0] > 0 && pad.slot[1] > 0) {
    return copperDistance({ x: cx, y: cy, shape: "oval", w: pad.slot[0], h: pad.slot[1], rot: pad.rot ?? 0, ratio: 0, outline: null }, x, y);
  }
  const d = pad.drill ?? 0;
  return d > 0 ? Math.hypot(x - cx, y - cy) - d / 2 : Infinity;
}

/**
 * `PAD::HitTest` as a distance: how far (x, y) is outside the pad -- the least over its layers' copper and its hole; 0 or less on it. A hit within an
 * accuracy `a` is `padHitDistance( .. ) <= a` (`Contains( aPosition, -1, aAccuracy )`).
 */
export function padHitDistance(pad: PadLike, x: number, y: number): number {
  let best = holeDistance(pad, x, y);
  for (const c of padCoppers(pad)) best = Math.min(best, copperDistance(c, x, y));
  return best;
}

/** A point of a copper's frame on the board. */
function toBoard(c: PadCopper, u: number, v: number): Pt {
  const a = (c.rot * Math.PI) / 180;
  const [cos, sin] = [Math.cos(a), Math.sin(a)];
  return [c.x + u * cos - v * sin, c.y + u * sin + v * cos];
}

/**
 * A rectangle of half sides `hw`, `hh` with corners of radius `r`, each corner arc cut into the chords of `GetArcToSegmentCount( r, aError, FULL_CIRCLE ) / 4`
 * lying outside it (`TransformRoundChamferedRectToPolygon`, `ERROR_OUTSIDE`): the arc's ends, and between them the corners of the polygon whose sides touch it.
 */
function roundedBoxRing(c: PadCopper, hw: number, hh: number, r: number, error = ARC_LOW_DEF_NM): Ring {
  if (r <= 0) return [toBoard(c, -hw, -hh), toBoard(c, hw, -hh), toBoard(c, hw, hh), toBoard(c, -hw, hh)];
  const n = Math.max(1, Math.ceil(circleSegments(r, error) / 4));
  const step = Math.PI / 2 / n;
  const rr = r / Math.cos(step / 2);
  const out: Pt[] = [];
  const corners: [number, number, number][] = [
    [hw - r, hh - r, 0],
    [-(hw - r), hh - r, Math.PI / 2],
    [-(hw - r), -(hh - r), Math.PI],
    [hw - r, -(hh - r), (3 * Math.PI) / 2],
  ];
  for (const [cx, cy, a0] of corners) {
    out.push(toBoard(c, cx + r * Math.cos(a0), cy + r * Math.sin(a0)));
    for (let i = 0; i < n; i++) {
      const t = a0 + (i + 0.5) * step;
      out.push(toBoard(c, cx + rr * Math.cos(t), cy + rr * Math.sin(t)));
    }
    out.push(toBoard(c, cx + r * Math.cos(a0 + Math.PI / 2), cy + r * Math.sin(a0 + Math.PI / 2)));
  }
  return out;
}

/** The outer contours of a polygon pad's copper (a contour inside another is one of its holes, left out: the coverage takes what a piece covers). */
function outerContours(outline: PadCopper["outline"]): Ring[] {
  const rings = (outline ?? []).filter((r) => r.length >= 3);
  return rings.filter((r, i) => !rings.some((o, j) => j !== i && outlineContains([o], r[0]![0], r[0]![1]))) as Ring[];
}

/** One layer's copper as polygons (`PAD::TransformShapeToPolygon( .., ERROR_OUTSIDE )`). */
export function copperRings(c: PadCopper): Ring[] {
  if (c.outline?.length && (OUTLINED.has(c.shape) || c.shape === "box")) return outerContours(c.outline);
  const hw = c.w / 2;
  const hh = c.h / 2;
  switch (c.shape) {
    case "circle":
      return [circleRing(c.x, c.y, Math.max(hw, hh))];
    case "oval": {
      const half = Math.abs(hw - hh);
      const [a, b] = hw >= hh ? [toBoard(c, -half, 0), toBoard(c, half, 0)] : [toBoard(c, 0, -half), toBoard(c, 0, half)];
      return [stadiumRing(a, b, Math.min(c.w, c.h))];
    }
    case "round_rect":
      return [roundedBoxRing(c, hw, hh, Math.min(Math.max(c.ratio, 0), 0.5) * Math.min(c.w, c.h))];
    default:
      return [roundedBoxRing(c, hw, hh, 0)];
  }
}

/** Every unique layer's copper of a pad as polygons (`GetBoundingHull`'s and `CoverageRatio`'s `TransformShapeToPolygon` of each layer). */
export function padCopperRings(pad: PadLike): Ring[] {
  return padCoppers(pad).flatMap(copperRings);
}

/** `FOOTPRINT::GetCoverageArea` of a pad: the area of the union of its layers' copper. */
export function padCoverageArea(pad: PadLike): number {
  return intersectionArea(padCopperRings(pad));
}
