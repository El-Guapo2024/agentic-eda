// A free-standing schematic text as KiCad lays it out on its anchor: `SCH_TEXT` (eeschema/sch_text.cpp), `EDA_TEXT::GetTextBox` and `FONT::getLinePositions` at 8303b2ad.
//
// A text is justified about its anchor in its own axes (`EDA_TEXT::GetHorizJustify` / `GetVertJustify`) and runs along the sheet or, at `ANGLE_VERTICAL`, up it (a quarter turn
// counter-clockwise about the anchor). The backend keeps the justification (`SchExtras::text_justify`, left and bottom unless something set it) and sends it with each text
// (`h`, `v`, `vertical`); a text from an older backend has none of them and reads as it always was drawn, left and bottom along the sheet.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { SchematicText } from "../api/types";
import type { Box } from "./schItemGeom";
import { defaultPenNm, type HAlign, type VAlign } from "./schText";

/** How wide a line of text is, micrometres. */
export type Measure = (text: string, sizeNm: number) => number;

/** How a text lies on its anchor. */
export interface TextLie {
  h: HAlign;
  v: VAlign;
  vertical: boolean;
}

/**
 * `EDA_ANGLE::KeepUpright` for a text's angle: [-45, 45], (135, 225] and [315, 360) are along the sheet, the rest up it (a text is never upside down, and is only ever
 * horizontal or vertical).
 */
export function keepUprightIsVertical(angleDeg: number): boolean {
  const a = ((angleDeg % 360) + 360) % 360;
  return !(a <= 45 || a >= 315 || (a > 135 && a <= 225));
}

/** How the text lies on its anchor. */
export function textLie(t: Pick<SchematicText, "angle" | "h" | "v" | "vertical">): TextLie {
  return { h: t.h ?? "left", v: t.v ?? "bottom", vertical: t.vertical ?? keepUprightIsVertical(t.angle) };
}

/** `STROKE_FONT::GetInterline`: the glyph height times `METRICS::m_InterlinePitch` (1.68) and the legacy factor (0.9583), rounded. */
export function interlineNm(sizeNm: number): number {
  return Math.round(sizeNm * 1.68 * 0.9583);
}

/** One line of a text: its string and where its baseline starts, in the text's own axes (x along the text, y down) from the anchor, micrometres. */
export interface TextLine {
  text: string;
  x: number;
  y: number;
}

/**
 * Where each line of the text starts (`FONT::getLinePositions`): the first line's top is `size * 1.17` high with `interline` more for every line after it; the block is
 * shifted up by its whole height for a bottom justification and by half of it for a centre one; each line is justified on its own against the anchor.
 */
export function schTextLines(t: Pick<SchematicText, "content" | "shown" | "size_nm" | "angle" | "h" | "v" | "vertical">, measure: Measure): TextLine[] {
  const lie = textLie(t);
  const size = t.size_nm;
  const pen = defaultPenNm(size);
  const strings = (t.shown ?? t.content).split("\n"); // what `${NAME}` stands for, when the backend says (`shown`)
  const interline = interlineNm(size);
  const height = Math.trunc(size * 1.17) + (strings.length - 1) * interline;
  // the offsets are whole internal units, as the C++ ints are
  const offX = Math.trunc(pen / 1.52);
  let offY = Math.trunc(size - pen * 0.052);
  if (lie.v === "center") offY -= Math.trunc(height / 2);
  else if (lie.v === "bottom") offY -= height;
  return strings.map((text, i) => {
    const advance = Math.trunc(measure(text, size));
    const x = lie.h === "left" ? offX : lie.h === "center" ? -Math.trunc(advance / 2) : -(advance + offX);
    return { text, x, y: offY + i * interline };
  });
}

/**
 * The box the text takes on the sheet (`EDA_TEXT::GetTextBox`): the longest line and the pen's reach, `fudge` taller than the ink, the lines `interline` apart, justified
 * against the anchor; a text that runs up the sheet is turned a quarter counter-clockwise about it.
 */
export function schTextBox(t: Pick<SchematicText, "content" | "shown" | "size_nm" | "angle" | "h" | "v" | "vertical" | "at">, measure: Measure): Box {
  const lie = textLie(t);
  const size = t.size_nm;
  const pen = defaultPenNm(size);
  const k = Math.round(pen * 1.5);
  const lines = (t.shown ?? t.content).split("\n");
  const w = Math.max(...lines.map((l) => measure(l, size)), size * 0.5) + 2 * k;
  const ext = size + 2 * k;
  const fudge = Math.round(ext * 0.17);
  const h = ext + fudge + (lines.length - 1) * interlineNm(size);
  const x0 = lie.h === "left" ? 0 : lie.h === "center" ? -Math.trunc(w / 2) : -w;
  const y0 = lie.v === "top" ? -fudge : lie.v === "center" ? -Math.trunc(h / 2) : -h + fudge;
  const place = ([x, y]: [number, number]): [number, number] => (lie.vertical ? [t.at[0] + y, t.at[1] - x] : [t.at[0] + x, t.at[1] + y]);
  const a = place([x0, y0]);
  const b = place([x0 + w, y0 + h]);
  return { minX: Math.min(a[0], b[0]), minY: Math.min(a[1], b[1]), maxX: Math.max(a[0], b[0]), maxY: Math.max(a[1], b[1]) };
}
