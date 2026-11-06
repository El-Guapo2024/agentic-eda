// A text as `GuessSelectionCandidates`' area and coverage passes measure it (pcbnew/footprint.cpp and pcbnew/pcb_text.cpp at 8303b2ad), UI-LEFT-2 PP2:
//
//   FOOTPRINT::GetCoverageArea( text )      `text->TransformTextToPolySet( poly, textMargin, ARC_LOW_DEF, ERROR_INSIDE )`: the area of its strokes, each pen
//                                           stroke grown by the collector's accuracy (`textMargin = aCollector.GetGuide()->Accuracy()`, five pixels)
//   FOOTPRINT::CoverageRatio                a footprint's own texts and shown fields: `TransformFPShapesToPolySet( .., textMargin, ARC_LOW_DEF, ERROR_OUTSIDE,
//                                           true, .. )` -- their strokes grown by the accuracy and the error (`aClearance += aMaxError` for ERROR_OUTSIDE);
//                                           any other text the click found: `item->TransformShapeToPolygon( .., 0, ARC_LOW_DEF, ERROR_OUTSIDE )`, which is
//                                           `buildBoundingHull` -- the box of its strokes in its own frame, turned with it
//   FOOTPRINT::GetBoundingHull              a footprint's own texts: `TransformShapeToPolygon` as above, the box of their strokes
//
// The strokes are kicad-port/textHit.ts's (`fieldStrokes`: the stroke font's glyphs where `FONT::getLinePositions` puts them); a stroke is a segment of the
// pen's width with round ends (`TransformOvalToPolygon`). The union of the strokes is measured exactly by kicad-port/coverage.ts' sweep. A board text
// (`PCB_TEXT` of the board) is measured the same way as a footprint's: `boardTextAsField` hands it to `fieldStrokes`. Pure: no store, no DOM.
import type { BoardText, FieldInfo } from "../api/types";
import { ARC_LOW_DEF_NM, intersectionArea, stadiumRing, type Pt, type Ring } from "./coverage";
import type { TextStrokes } from "./textHit";

/** A board text as the field `fieldStrokes` measures (`PCB_TEXT`: the same `EDA_TEXT` as a footprint's text; never kept upright, no field name). */
export function boardTextAsField(t: BoardText): FieldInfo {
  return {
    id: t.id,
    name: "",
    text: t.content,
    shown: t.shown,
    x: t.x,
    y: t.y,
    angle: t.angle,
    w: t.width ?? t.size,
    h: t.size,
    // an automatic pen (`auto_thickness`) is the default one of its size, as an absent `(thickness ..)` reads
    thickness: t.auto_thickness ? 0 : t.stroke_width,
    layer: t.layer,
    visible: true,
    halign: t.justify === "left" ? -1 : t.justify === "right" ? 1 : 0,
    valign: t.valign === "top" ? -1 : t.valign === "bottom" ? 1 : 0,
    mirror: t.mirror,
    bold: t.bold === true,
    italic: t.italic === true,
    upright: false,
    knockout: t.knockout === true,
    lx: 0,
    ly: 0,
    langle: 0,
    custom: false,
    face: t.face ?? undefined,
  };
}

/**
 * The strokes of a text as polygons, each grown by `margin` (`TransformOvalToPolygon` of the pen's width, then `Inflate( aClearance )`); an outline font's runs as the
 * boxes of their ink, grown likewise (KiCad's glyphs are their triangles: task-v1-text, UI-LEFT-2 CC1).
 */
export function strokeRings(s: TextStrokes, margin = 0, error = ARC_LOW_DEF_NM): Ring[] {
  const width = s.pen + 2 * margin;
  const strokes = width <= 0 ? [] : s.segs.map(([x0, y0, x1, y1]) => stadiumRing([x0, y0], [x1, y1], width, error));
  return [...strokes, ...(s.areas ?? []).map((q) => grownQuad(q, margin))];
}

/** A run's ink box (its corners in order) grown by `m` along both of its sides. */
function grownQuad(q: readonly (readonly [number, number])[], m: number): Ring {
  const [a, b, , d] = q as [readonly [number, number], readonly [number, number], readonly [number, number], readonly [number, number]];
  const unit = (p: readonly [number, number], r: readonly [number, number]): [number, number] => {
    const len = Math.hypot(r[0] - p[0], r[1] - p[1]) || 1;
    return [(r[0] - p[0]) / len, (r[1] - p[1]) / len];
  };
  const [u, v] = [unit(a, b), unit(a, d)];
  const signs: [number, number][] = [[-1, -1], [1, -1], [1, 1], [-1, 1]];
  return q.map((p, i) => [p[0] + m * (signs[i]![0] * u[0] + signs[i]![1] * v[0]), p[1] + m * (signs[i]![0] * u[1] + signs[i]![1] * v[1])] as Pt);
}

/** `GetCoverageArea` of a text: the area its strokes, grown by the collector's accuracy, cover (0 for a text with no strokes). */
export function textCoverageArea(s: TextStrokes, accuracyNm: number): number {
  return intersectionArea(strokeRings(s, accuracyNm));
}

/** `PCB_TEXT::buildBoundingHull`: the box of the strokes in the text's frame, grown by `margin`, its corners turned back onto the board; none with no strokes. */
export function strokesBoxRing(s: TextStrokes, margin = 0): Ring | null {
  if (!s.box) return null;
  const [u0, v0, u1, v1] = s.box;
  const [cos, sin] = [Math.cos(s.angleRad), Math.sin(s.angleRad)];
  const at = (u: number, v: number): Pt => [s.anchor[0] + u * cos - v * sin, s.anchor[1] + u * sin + v * cos];
  return [at(u0 - margin, v0 - margin), at(u1 + margin, v0 - margin), at(u1 + margin, v1 + margin), at(u0 - margin, v1 + margin)];
}
