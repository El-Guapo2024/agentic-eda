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
// only the heuristics read the strokes. The box here is that of the strokes, in the text's own frame. The strokes are the stroke font's glyphs
// (components/text/strokeFont.ts `strokeTextSegments`) placed where KiCad places them (`FONT::getLinePositions`, `STROKE_FONT::GetTextAsGlyphs`), not where the
// canvas draws them: the canvas centres a text on half its advance and puts its baseline 0.36 of its size below the anchor, KiCad on the advance less
// `INTER_CHAR` and 0.415 of the size less 0.052 of the pen, so the canvas' strokes stand 0.1 of the size along the text and 0.05 of it up from KiCad's. A click
// at a field's anchor is on KiCad's strokes of "U1" and 0.1 mm off the canvas' (UI-LEFT P15, footprint-edit.check.js); a click on a drawn stroke is still
// well within the slop of KiCad's.
//
// Pure: no store, no DOM.
import type { FieldInfo } from "../api/types";
import { measureStrokeText, strokeTextSegments, type StrokeSegment } from "../components/text/strokeFont";
import { fieldAsText } from "./fpFields";

/** `GuessSelectionCandidates`' `MAX_SLOP` (pixels): `maxSlop = KiROUND( MAX_SLOP * aCollector.GetGuide()->OnePixelInIU() )`. */
export const MAX_SLOP_PX = 5;

/** `STROKE_FONT::GetInterline`: `FONT_METRICS`' 1.62 glyph heights, times the legacy factor 0.9583 (fpArtPaint.ts's INTERLINE). */
const INTERLINE = 1.62 * 0.9583;

/** `STROKE_FONT::GetTextAsGlyphs`' `INTER_CHAR`: the bounding box of a line ends this much of the size short of the cursor. */
const INTER_CHAR = 0.2;

/** A text's strokes and its pen, and its box: that of its strokes (the pen's width round them) in its own frame, about `anchor` turned by `angleRad`. */
export interface TextStrokes {
  segs: StrokeSegment[];
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
 * The strokes of a footprint's field or text (a `FpTextInfo` through `artTextAsField`), at its real size, placed as `FONT::getLinePositions` places a stroke
 * text's lines in its own frame -- the turn and the mirror about the anchor (`GetTextAsGlyphs`' `aOrigin`) after:
 *
 *   offset.y = size - 0.052 * pen                                          ("Fudge factors to match 6.0 positioning"), less half of the block's height
 *                                                                          (centred) or all of it (bottom); the block is 1.17 sizes for the first line and an
 *                                                                          interline more for each other
 *   lineOffset.x = pen / 1.52 (left), -lineSize.x / 2 (centred), -( lineSize.x + pen / 1.52 ) (right)
 *                                                                          where `lineSize.x` is the line's advance less `INTER_CHAR` of the size
 *
 * The pen is the text's thickness (at most a quarter of the size, `Clamp_Text_PenSize`), or the default one of its size (`GetPenSizeForNormal`,
 * `GetPenSizeForBold`). The size is the text's height both ways, as the canvas draws it.
 */
export function fieldStrokes(f: FieldInfo): TextStrokes {
  const t = fieldAsText(f);
  const size = f.h;
  const pen = f.thickness > 0 ? Math.min(f.thickness, size / 4) : f.bold ? size / 5 : size / 8;
  const lines = t.content.split("\n");
  // KiCad's counter-clockwise millidegrees; the canvas turns clockwise
  const angleRad = (-t.angle / 1000) * (Math.PI / 180);
  const [cos, sin] = [Math.cos(angleRad), Math.sin(angleRad)];
  const interline = INTERLINE * size;
  const height = 1.17 * size + (lines.length - 1) * interline;
  const top = size - 0.052 * pen - (f.valign === 0 ? height / 2 : f.valign > 0 ? height : 0);
  const mirror = f.mirror ? -1 : 1;
  const segs: StrokeSegment[] = [];
  lines.forEach((line, i) => {
    if (!line) return;
    const lineWidth = measureStrokeText(line, size) - INTER_CHAR * size;
    const lx = t.justify === "left" ? pen / 1.52 : t.justify === "center" ? -lineWidth / 2 : -(lineWidth + pen / 1.52);
    const ly = top + i * interline;
    // the line's start in the text's frame, mirrored about the anchor, turned with the text
    const [u, v] = [mirror * lx, ly];
    segs.push(...strokeTextSegments(line, f.x + u * cos - v * sin, f.y + u * sin + v * cos, { sizeNm: size, justify: "left", angleRad, mirror: f.mirror, italic: f.italic }));
  });
  const frame = { anchor: [f.x, f.y] as const, angleRad };
  let box: [number, number, number, number] | null = null;
  for (const [x0, y0, x1, y1] of segs) {
    for (const [u, v] of [toFrame(frame, x0, y0), toFrame(frame, x1, y1)]) {
      box = box ? [Math.min(box[0], u), Math.min(box[1], v), Math.max(box[2], u), Math.max(box[3], v)] : [u, v, u, v];
    }
  }
  const half = pen / 2;
  return { segs, pen, ...frame, box: box && [box[0] - half, box[1] - half, box[2] + half, box[3] + half] };
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

/** How far `(x, y)` is from the nearest stroke's edge (`SHAPE_SEGMENT::Collide`'s actual distance: 0 on a stroke); Infinity for a text with no strokes. */
export function strokeDistance(s: TextStrokes, x: number, y: number): number {
  let best = Infinity;
  for (const seg of s.segs) best = Math.min(best, distToSegment(x, y, seg));
  return Math.max(0, best - s.pen / 2);
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
