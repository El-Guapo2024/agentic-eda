// Layout of a schematic text box's text (`SCH_TEXTBOX`): the default margin, word wrapping to the box's inner
// width and where each wrapped line is anchored. Ported from `SCH_TEXTBOX::GetLegacyTextMargin`,
// `SCH_TEXTBOX::GetDrawPos` and the line breaking `EDA_TEXT::GetTextBox`/`KIFONT::FONT::LinebreakText` do,
// with the stroke font's own interline pitch (`METRICS::m_InterlinePitch`, 1.68). Pure: the caller passes the
// text measuring function (the real one lives next to the stroke font and reads a JSON glyph table).

/** `METRICS::m_InterlinePitch` (include/font/font_metrics.h). */
export const INTERLINE_PITCH = 1.68;
/** The stroke font's cap height as a fraction of the text size (a baseline sits this far below the glyph tops). */
export const CAP_HEIGHT_RATIO = 0.95;

/** `SCH_TEXTBOX::GetLegacyTextMargin` on the notes layer: half the stroke width plus 0.75 of the text height (um). */
export function defaultTextBoxMargin(sizeUm: number, strokeWidthUm: number): number {
  return Math.round(strokeWidthUm / 2) + Math.round(sizeUm * 0.75);
}

export type Measure = (text: string, sizeUm: number) => number;

/** Greedy word wrap of `text` to `maxWidthUm` (explicit newlines are kept; a single word longer than the width stays whole on its own line). */
export function wrapText(text: string, maxWidthUm: number, sizeUm: number, measure: Measure): string[] {
  const out: string[] = [];
  for (const paragraph of text.split("\n")) {
    const words = paragraph.split(" ");
    let line = "";
    for (const word of words) {
      const candidate = line === "" ? word : `${line} ${word}`;
      if (line !== "" && measure(candidate, sizeUm) > maxWidthUm) {
        out.push(line);
        line = word;
      } else {
        line = candidate;
      }
    }
    out.push(line);
  }
  return out;
}

export interface TextBoxLine {
  text: string;
  /** Baseline position along the text direction (`u`) and across it (`v`), um, in the box's local text frame. */
  u: number;
  v: number;
  justify: "left" | "center" | "right";
}

export interface TextBoxLayout {
  /** The box's frame: where the text frame's origin sits on the sheet and whether it is rotated 90 degrees (text reading upward). */
  origin: [number, number];
  vertical: boolean;
  lines: TextBoxLine[];
}

export interface TextBoxSpec {
  start: readonly [number, number];
  end: readonly [number, number];
  text: string;
  /** 0 or 90000 (millidegrees). */
  angle: number;
  sizeUm: number;
  hAlign: "left" | "center" | "right";
  vAlign: "top" | "center" | "bottom";
  marginUm: number;
}

/**
 * Where the lines of a text box go. Horizontal text: the frame's origin is the box's top-left corner, `u` runs
 * right and `v` down. Vertical text (angle 90): the origin is the bottom-left corner, `u` runs up the sheet and
 * `v` right -- the frame is rotated -90 degrees, which is how the painter draws it.
 */
export function layoutTextBox(spec: TextBoxSpec, measure: Measure): TextBoxLayout {
  const left = Math.min(spec.start[0], spec.end[0]);
  const right = Math.max(spec.start[0], spec.end[0]);
  const top = Math.min(spec.start[1], spec.end[1]);
  const bottom = Math.max(spec.start[1], spec.end[1]);
  const vertical = Math.round(spec.angle / 1000) % 180 === 90;
  const along = vertical ? bottom - top : right - left; // the text direction's extent
  const across = vertical ? right - left : bottom - top;
  const m = spec.marginUm;
  const lines = wrapText(spec.text, Math.max(along - 2 * m, 0), spec.sizeUm, measure);
  const pitch = spec.sizeUm * INTERLINE_PITCH;
  const blockHeight = spec.sizeUm * CAP_HEIGHT_RATIO + (lines.length - 1) * pitch;
  // `GetDrawPos`: the first baseline's offset from the top margin is the cap height; the block as a whole aligns per vAlign.
  const v0 = spec.vAlign === "top" ? m : spec.vAlign === "bottom" ? across - m - blockHeight : (across - blockHeight) / 2;
  const u = spec.hAlign === "left" ? m : spec.hAlign === "right" ? along - m : along / 2;
  return {
    origin: vertical ? [left, bottom] : [left, top],
    vertical,
    lines: lines.map((text, i) => ({ text, u, v: v0 + spec.sizeUm * CAP_HEIGHT_RATIO + i * pitch, justify: spec.hAlign })),
  };
}
