// A footprint's texts as the selection tool measures them (pcbnew/tools/pcb_selection_tool.cpp at 8303b2ad): by their pen strokes, not their box.
//
//   PCB_SELECTION_TOOL::hitTestDistance, `case PCB_FIELD_T: case PCB_TEXT_T:`
//       // Add a bit of slop to text-shapes
//       if( text->GetEffectiveTextShape()->Collide( loc, aMaxDistance, &distance ) )
//           distance = std::clamp( distance - ( aMaxDistance / 2 ), 0, distance );
//
// `GetEffectiveTextShape` of a stroke-font text is one `SHAPE_SEGMENT` of the pen's width per stroke (`EDA_TEXT::GetEffectiveTextShape`'s stroke callback), so a
// click is an exact hit within half `MAX_SLOP` pixels of a stroke's edge, a sloppy one up to `MAX_SLOP` pixels from it, and no hit at all further off
// (`distance` stays `INT_MAX`): `GuessSelectionCandidates`' "Prefer exact hits to sloppy ones" then drops the text for the footprint it lies on, whose hull the
// click is inside (`hitTestDistance`'s `PCB_FOOTPRINT_T`: `GetBoundingHull().Collide`, 0 inside). So a click in the hole of a footprint's fab reference picks the
// footprint, and one on the reference's strokes picks the text -- the text being much the smaller (UI-LEFT P15).
//
// The collector itself takes a text by its box (`EDA_TEXT::TextHitTest`: the point turned into the text's frame, `GetTextBox().GetInflated( aAccuracy ).Contains`);
// only the heuristics read the strokes. The box here is that of the strokes, in the text's own frame. The strokes are the painter's: `kicad-port/kicadText.ts` lays the
// text out as `FONT::getLinePositions` and `STROKE_FONT::GetTextAsGlyphs` do (kicad-cli's plot is the proof, `kicadText.test.ts`), and the canvas draws that layout
// (UI-LEFT-2 PP1; before, the canvas stood 0.1 of the size along the text and 0.05 up from KiCad's place, and this file's hit test centred a line on its advance less `INTER_CHAR`,
// which KiCad takes off the box only, so the two differed by 0.1 of the size the other way). A text in an outline font is hit in the box of each run's ink (KiCad's
// `GetEffectiveTextShape` gives the glyphs' triangles).
//
// Pure: no store, no DOM.
import type { FieldInfo } from "../api/types";
import type { StrokeSegment } from "../components/text/strokeFont";
import { frameToWorld, layoutKicadText, layoutSegments, resolveFont, runInk } from "./kicadText";
import { fieldSpec } from "./textSpecs";

/** `GuessSelectionCandidates`' `MAX_SLOP` (pixels): `maxSlop = KiROUND( MAX_SLOP * aCollector.GetGuide()->OnePixelInIU() )`. */
export const MAX_SLOP_PX = 5;

/** A text's strokes and its pen, and its box: that of its strokes (the pen's width round them) in its own frame, about `anchor` turned by `angleRad`. */
export interface TextStrokes {
  segs: StrokeSegment[];
  /** An outline font's runs: the four corners of each one's ink on the board. */
  areas: [number, number][][];
  pen: number;
  anchor: readonly [number, number];
  /** The canvas' (clockwise) turn of the text. */
  angleRad: number;
  /** `[u0, v0, u1, v1]` in the text's frame; null for a text with no strokes. */
  box: readonly [number, number, number, number] | null;
}

/** (x, y) in the frame of a text: about its anchor, turned back by its angle. */
function toFrame(s: Pick<TextStrokes, "anchor" | "angleRad">, x: number, y: number): [number, number] {
  const [cos, sin] = [Math.cos(s.angleRad), Math.sin(s.angleRad)];
  const [dx, dy] = [x - s.anchor[0], y - s.anchor[1]];
  return [dx * cos + dy * sin, -dx * sin + dy * cos];
}

/**
 * The strokes of a footprint's field or text (a `FpTextInfo` through `artTextAsField`), at its real size, as the canvas draws them: `kicad-port/textSpecs.ts::fieldSpec`
 * laid out by `layoutKicadText` (`FONT::getLinePositions`: the first baseline a size less 0.052 pens below the top of the block, the block centred or hung by the
 * vertical justification, a line `pen / 1.52` in from the left, centred on half its advance or ended `pen / 1.52` short of the right), mirrored and turned about the
 * anchor. The pen is `GetEffectiveTextPenWidth`.
 */
export function fieldStrokes(f: FieldInfo): TextStrokes {
  const { spec, face } = fieldSpec(f);
  const l = layoutKicadText(spec, resolveFont(face, spec.bold, spec.italic));
  const segs: StrokeSegment[] = layoutSegments(l);
  const areas: [number, number][][] = [];
  for (const r of l.runs) {
    const ink = runInk(l, r);
    if (ink) areas.push([frameToWorld(l, ink.x0, ink.y0), frameToWorld(l, ink.x1, ink.y0), frameToWorld(l, ink.x1, ink.y1), frameToWorld(l, ink.x0, ink.y1)]);
  }
  // the canvas turns clockwise
  const angleRad = (-(spec.angleDeg ?? 0) * Math.PI) / 180;
  const frame = { anchor: [f.x, f.y] as const, angleRad };
  let box: [number, number, number, number] | null = null;
  const take = (x: number, y: number, grow: number) => {
    const [u, v] = toFrame(frame, x, y);
    box = box ? [Math.min(box[0], u - grow), Math.min(box[1], v - grow), Math.max(box[2], u + grow), Math.max(box[3], v + grow)] : [u - grow, v - grow, u + grow, v + grow];
  };
  const half = l.pen / 2;
  for (const [x0, y0, x1, y1] of segs) {
    take(x0, y0, half);
    take(x1, y1, half);
  }
  for (const a of areas) for (const [x, y] of a) take(x, y, 0);
  return { segs, areas, pen: l.pen, ...frame, box };
}

/** How far `(x, y)` is outside a text's box (0 inside), in its frame: `TextHitTest`'s test with `aAccuracy` the distance allowed. Infinity with no box. */
export function textBoxDistance(s: TextStrokes, x: number, y: number): number {
  if (!s.box) return Infinity;
  const [u, v] = toFrame(s, x, y);
  const [u0, v0, u1, v1] = s.box;
  return Math.hypot(Math.max(u0 - u, 0, u - u1), Math.max(v0 - v, 0, v - v1));
}

/** The area of a text's box (what the area pass weighs it by here; KiCad's `GetCoverageArea` takes its strokes grown by the collector's accuracy). */
export function textBoxArea(s: TextStrokes): number {
  return s.box ? (s.box[2] - s.box[0]) * (s.box[3] - s.box[1]) : 0;
}

function distToSegment(px: number, py: number, [x0, y0, x1, y1]: StrokeSegment): number {
  const dx = x1 - x0;
  const dy = y1 - y0;
  const len2 = dx * dx + dy * dy;
  const t = len2 > 0 ? Math.max(0, Math.min(1, ((px - x0) * dx + (py - y0) * dy) / len2)) : 0;
  return Math.hypot(px - (x0 + t * dx), py - (y0 + t * dy));
}

/** How far `(x, y)` is from an outline run's ink box (0 inside it). */
function areaDistance(a: readonly [number, number][], x: number, y: number): number {
  let inside = false;
  let best = Infinity;
  for (let i = 0, j = a.length - 1; i < a.length; j = i++) {
    const [xi, yi] = a[i]!;
    const [xj, yj] = a[j]!;
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside;
    best = Math.min(best, distToSegment(x, y, [xj, yj, xi, yi]));
  }
  return inside ? 0 : best;
}

/** How far `(x, y)` is from the nearest stroke's edge (`SHAPE_SEGMENT::Collide`'s actual distance: 0 on a stroke) or an outline run's ink; Infinity for a text with neither. */
export function strokeDistance(s: TextStrokes, x: number, y: number): number {
  let best = Infinity;
  for (const seg of s.segs) best = Math.min(best, distToSegment(x, y, seg));
  best = Math.max(0, best - s.pen / 2);
  for (const a of s.areas ?? []) best = Math.min(best, areaDistance(a, x, y));
  return best;
}

/**
 * `hitTestDistance` of a text: within `MAX_SLOP` pixels of a stroke, the distance less half of that, clamped at 0; further off, no hit (Infinity, KiCad's
 * `INT_MAX`, which the sloppiness pass drops for any exact hit).
 */
export function textHitSlop(s: TextStrokes, x: number, y: number, onePixelNm: number): number {
  const maxSlop = Math.round(MAX_SLOP_PX * onePixelNm);
  const d = strokeDistance(s, x, y);
  return d <= maxSlop ? Math.max(0, d - maxSlop / 2) : Infinity;
}
