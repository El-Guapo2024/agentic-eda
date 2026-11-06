// The board's items as the polygons `GuessSelectionCandidates`' coverage passes measure (UI-LEFT P22; kicad-port/coverage.ts does the arithmetic):
//
//   a pad              its copper (`PAD::TransformShapeToPolygon` of each unique layer, kicad-port/padHit.ts `padCopperRings`): a circle, an oval (a stadium),
//                      a turned rectangle, a rounded rectangle with its corner arcs, and a trapezoid, a chamfered rectangle or a custom pad by the contours of its
//                      copper (UI-LEFT-2 PP2; `padRing`, the rectangle of its size for all but circles and ovals, is kept as INACTIVE)
//   a hole             `PAD::TransformHoleToPolygon`: the drill's circle, or a slot's stadium
//   a shape            `PCB_SHAPE::TransformShapeToPolygon`: a filled rectangle, circle or polygon its area; a line, an arc, a curve and the outline of an unfilled
//                      one its stroke, the stroke's width round each piece
//   a track            its segments with their width; a via its pad's circle
//   a footprint        `GetBoundingHull`: the convex hull of the points of its drawings, pads and holes -- here its silkscreen, fab and assembly drawings, its own
//                      texts' boxes, its pads, its holes, and the corners of its courtyard's box (the courtyard drawing itself is not in the board's state)
//
// And `FOOTPRINT::GetCoverageArea`'s per-kind rule for the area pass: a track (or arc track) is `width * width`, a line, an arc and a curve are `width * width`,
// so is an unfilled rectangle, circle or polygon ("we don't want to consider a linear shape as being much bigger than another ... just because it happens to be
// really long"); a filled one is its area; a footprint is its hull's area. Pure.
import type { FpGraphic, Pad, Part, Shape } from "../api/types";
import { ARC_LOW_DEF_NM, boundingHull, circleRing, polylineRings, rectRing, ringArea, stadiumRing, type Pt, type Ring } from "./coverage";
import { arcThrough } from "./snapGeom";
import { bezierPolyline } from "./bezierPoly";
import { padCopperRings } from "./padHit";

/** INACTIVE (UI-LEFT-2 PP2): a pad's copper as one polygon, a rounded or chamfered rectangle, a trapezoid and a custom pad standing as the rectangle of their size;
 * the hull and the coverage take `padCopperRings` (kicad-port/padHit.ts), the real outline of each layer's copper. */
export function padRing(pad: Pad): Ring {
  const [w, h] = pad.size ?? [pad.w, pad.h];
  const rot = pad.rot ?? 0;
  if (pad.shape === "circle" || (pad.round && w === h)) return circleRing(pad.x, pad.y, Math.max(w, h) / 2);
  if (pad.shape === "oval" || (pad.round && w !== h)) {
    // the long axis, turned with the pad (clockwise degrees on the y-down board)
    const a = (rot * Math.PI) / 180;
    const half = (Math.max(w, h) - Math.min(w, h)) / 2;
    const [ux, uy] = w >= h ? [Math.cos(a), Math.sin(a)] : [-Math.sin(a), Math.cos(a)];
    return stadiumRing([pad.x - ux * half, pad.y - uy * half], [pad.x + ux * half, pad.y + uy * half], Math.min(w, h));
  }
  if (pad.size) return rectRing(pad.x, pad.y, w, h, rot);
  return rectRing(pad.x, pad.y, pad.w, pad.h, 0);
}

/** `PAD::TransformHoleToPolygon`: the drill (a slot as a stadium along its length), about the pad's position; none for a pad without a hole. */
export function holeRing(pad: Pad): Ring | null {
  const cx = pad.px ?? pad.x;
  const cy = pad.py ?? pad.y;
  if (pad.slot && pad.slot[0] !== pad.slot[1]) {
    const [sw, sh] = pad.slot;
    const a = ((pad.rot ?? 0) * Math.PI) / 180;
    const half = (Math.max(sw, sh) - Math.min(sw, sh)) / 2;
    const [ux, uy] = sw >= sh ? [Math.cos(a), Math.sin(a)] : [-Math.sin(a), Math.cos(a)];
    return stadiumRing([cx - ux * half, cy - uy * half], [cx + ux * half, cy + uy * half], Math.min(sw, sh));
  }
  const d = pad.drill ?? (pad.slot ? pad.slot[0] : 0);
  return d > 0 ? circleRing(cx, cy, d / 2) : null;
}

type AnyShape = Shape | FpGraphic;

/** The points along a shape's outline: a line's two ends, an arc and a curve flattened, a rectangle's corners, a circle's chords, a polygon's points. */
function outlinePoints(s: AnyShape): { pts: Pt[]; closed: boolean } {
  switch (s.kind) {
    case "segment":
      return { pts: [s.start, s.end], closed: false };
    case "arc": {
      const g = arcThrough(s.start, s.mid, s.end);
      if (!g) return { pts: [s.start, s.mid, s.end], closed: false };
      const n = Math.max(2, Math.ceil(Math.abs(g.da) / (Math.PI / 18)));
      const pts: Pt[] = [];
      for (let i = 0; i <= n; i++) {
        const a = g.a0 + (g.da * i) / n;
        pts.push([g.c[0] + g.r * Math.cos(a), g.c[1] + g.r * Math.sin(a)]);
      }
      return { pts, closed: false };
    }
    case "bezier":
      return { pts: bezierPolyline(s.start, s.c1, s.c2, s.end, ARC_LOW_DEF_NM), closed: false };
    case "rect":
      return {
        pts: [
          [s.start[0], s.start[1]],
          [s.end[0], s.start[1]],
          [s.end[0], s.end[1]],
          [s.start[0], s.end[1]],
        ],
        closed: true,
      };
    case "circle":
      return { pts: [...circleRing(s.center[0], s.center[1], Math.hypot(s.end[0] - s.center[0], s.end[1] - s.center[1]))], closed: true };
    case "polygon":
      return { pts: s.pts.map((p) => [p[0], p[1]] as Pt), closed: true };
  }
}

const strokeWidthOf = (s: AnyShape): number => Math.max((s as { stroke_width?: number }).stroke_width ?? (s as { width?: number }).width ?? 0, 0);

/** `PCB_SHAPE::TransformShapeToPolygon( .., 0, ARC_LOW_DEF, ERROR_OUTSIDE )`: a filled closed shape its area (and its stroke), any other its stroke. */
export function shapeRings(s: AnyShape, filled: boolean): Ring[] {
  const { pts, closed } = outlinePoints(s);
  const width = strokeWidthOf(s);
  const out: Ring[] = [];
  if (closed && filled && pts.length >= 3) out.push(pts);
  if (width > 0) out.push(...polylineRings(closed ? [...pts, pts[0]!] : pts, width));
  return out;
}

/** `FOOTPRINT::GetCoverageArea` of a shape: `width * width` for a line, an arc, a curve and an unfilled closed shape; a filled one's polygon area. */
export function shapeCoverageArea(s: AnyShape, filled: boolean): number {
  const width = strokeWidthOf(s);
  if (s.kind === "segment" || s.kind === "arc" || s.kind === "bezier" || !filled) return width * width;
  const { pts } = outlinePoints(s);
  return ringArea(pts);
}

/** A track (a polyline of `width`) as KiCad's `PCB_TRACK`s are: each piece with round ends. */
export function trackRings(pts: readonly (readonly [number, number])[], width: number): Ring[] {
  return polylineRings(pts.map((p) => [p[0], p[1]] as Pt), width);
}

/** A box as a ring, grown by `margin` all round (the collector's accuracy that `TransformTextToPolySet` grows a text by). */
export function boxRing(x0: number, y0: number, x1: number, y1: number, margin = 0): Ring {
  return [
    [x0 - margin, y0 - margin],
    [x1 + margin, y0 - margin],
    [x1 + margin, y1 + margin],
    [x0 - margin, y1 + margin],
  ];
}

/**
 * `FOOTPRINT::GetBoundingHull` of a placed part: the hull of its drawings (`graphics`, filled as the drawing says), its own texts (`textBoxes`, the caller
 * measures them), its pads and holes, and its courtyard's box corners; fields are left out ("We intentionally exclude footprint fields from the bounding hull").
 */
export function footprintHull(part: Part, textBoxes: readonly Ring[]): Ring {
  const rings: Ring[] = [];
  for (const g of part.graphics ?? []) rings.push(...shapeRings(g, (g as { filled?: boolean }).filled === true));
  rings.push(...textBoxes);
  for (const pad of part.pads ?? []) {
    rings.push(...padCopperRings(pad));
    const hole = holeRing(pad);
    if (hole) rings.push(hole);
  }
  if (part.courtyard) {
    const [x0, y0, x1, y1] = part.courtyard;
    rings.push(boxRing(x0, y0, x1, y1));
  }
  const at = part.at ?? [0, 0];
  return boundingHull(rings, [at[0], at[1]]);
}
