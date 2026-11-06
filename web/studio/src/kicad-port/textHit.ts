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
// only the heuristics read the strokes. The box here is that of the strokes, in the text's own frame. The strokes are those the canvas draws (components/text/strokeFont.ts `strokeTextSegments`, placed as components/canvas/painter.ts `drawFields` and
// fpArtPaint.ts `drawArtText` place them), so what is hit is what is seen.
//
// Pure: no store, no DOM.
import type { FieldInfo } from "../api/types";
import { strokeTextSegments, type StrokeSegment } from "../components/text/strokeFont";
import { fieldAsText } from "./fpFields";

/** `GuessSelectionCandidates`' `MAX_SLOP` (pixels): `maxSlop = KiROUND( MAX_SLOP * aCollector.GetGuide()->OnePixelInIU() )`. */
export const MAX_SLOP_PX = 5;

/** `STROKE_FONT::GetInterline`: `FONT_METRICS`' 1.62 glyph heights, times the legacy factor 0.9583 (fpArtPaint.ts's INTERLINE). */
const INTERLINE = 1.62 * 0.9583;

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
 * The strokes of a footprint's field or text (a `FpTextInfo` through `artTextAsField`), at its real size: a line at a time down the text's own axis by the
 * interline, the block placed on the anchor by the vertical justification and the first baseline 0.36 of the size below a line's middle -- where the canvas
 * draws them. The pen is the text's thickness (at most a quarter of the size, `Clamp_Text_PenSize`), or the default one of its size (`GetPenSizeForNormal`,
 * `GetPenSizeForBold`).
 */
export function fieldStrokes(f: FieldInfo): TextStrokes {
  const t = fieldAsText(f);
  const size = f.h;
  const pen = f.thickness > 0 ? Math.min(f.thickness, size / 4) : f.bold ? size / 5 : size / 8;
  const lines = t.content.split("\n");
  // KiCad's counter-clockwise millidegrees; the canvas turns clockwise
  const angleRad = (-t.angle / 1000) * (Math.PI / 180);
  const interline = INTERLINE * size;
  const block = (lines.length - 1) * interline;
  const first = f.valign > 0 ? -block : f.valign < 0 ? 0.72 * size : 0.36 * size - block / 2;
  const segs: StrokeSegment[] = [];
  lines.forEach((line, i) => {
    if (!line) return;
    const down = first + i * interline;
    segs.push(...strokeTextSegments(line, f.x - down * Math.sin(angleRad), f.y + down * Math.cos(angleRad), { sizeNm: size, justify: t.justify, angleRad, mirror: f.mirror, italic: f.italic }));
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
