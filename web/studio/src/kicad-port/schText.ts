// Where the glyphs of one line of schematic text go for an anchor, a justification and an angle: KiCad's own placement of stroke-font
// text (`FONT::Draw`, `getLinePositions`; the Rust side is `kicad_text_segments` in crates/drc/src/stroke_font.rs, read against the
// KiCad source at 8303b2ad).
//
// A text has an anchor and is justified against it ("left" puts the text's start there, "center" its middle) in its own axes; a
// vertical text is the same text turned a quarter counter-clockwise about the anchor. The glyph strokes are drawn from a baseline, so
// the justification becomes an offset of the baseline's start from the anchor.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).

import type { SchField } from "../api/types";

export type { SchField };
export type HAlign = "left" | "center" | "right";
export type VAlign = "top" | "center" | "bottom";

/** `GetPenSizeForNormal`: a text with no thickness of its own is written with a pen an eighth of its size. */
export function defaultPenUm(sizeUm: number): number {
  return Math.round(sizeUm / 8);
}

/**
 * The offset from a text's anchor to the start of its baseline, in the text's own axes (x along the text, y down), micrometres.
 * `advanceUm` is the width of the line (the sum of its glyphs' advances).
 *
 * The 1.17 and the `thickness / 1.52` and `thickness * 0.052` terms are `getLinePositions`' compatibility factors with KiCad 6's
 * text; the offsets are truncated to whole internal units as KiCad does.
 */
export function textOrigin(advanceUm: number, sizeUm: number, thicknessUm: number, h: HAlign, v: VAlign): [number, number] {
  const height = sizeUm * 1.17;
  const offX = thicknessUm / 1.52;
  let offY = sizeUm - thicknessUm * 0.052;
  if (v === "center") offY -= Math.trunc(height / 2);
  else if (v === "bottom") offY -= Math.trunc(height);
  const lineX = h === "left" ? offX : h === "center" ? -advanceUm / 2 : -(advanceUm + offX);
  return [Math.trunc(lineX), Math.trunc(offY)];
}

/** The size every field is set in: 50 mils. */
export const FIELD_SIZE_UM = 1_270;
