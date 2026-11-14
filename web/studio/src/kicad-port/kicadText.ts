// A text laid out as KiCad lays it out, in the stroke font or an outline (TrueType / OpenType) font: the one layout the canvases draw and the hit tests read
// (UI-LEFT-2 PP1, CC1).
//
// Ported from KiCad 8303b2ad (25ec2335, the box's kicad-cli, is the same in these functions but for the super/subscript constants' names):
//   - `common/font/font.cpp`: `FONT::GetFont` (which font a face is), `FONT::getLinePositions` (where each line's baseline starts), `FONT::Draw` and
//     `drawSingleLineText` (each line drawn from there, mirrored and turned about the text's position), `::drawMarkup` (the `~{}` `_{}` `^{}` runs and the overbar),
//     `boundingBoxSingleLine` (a line's extent: the cursor where its last glyph ends), `StringBoundaryLimits`;
//   - `common/font/stroke_font.cpp`: `STROKE_FONT::GetTextAsGlyphs` (each glyph scaled by the size, slanted by `ITALIC_TILT`, its advance `KiROUND`ed, a space's,
//     a tab's column, `INTER_CHAR` taken off the box only), `GetInterline` (the legacy factor 0.9583), `common/font/glyph.cpp::STROKE_GLYPH::Transform`;
//   - `common/font/outline_font.cpp`: `OUTLINE_FONT::GetTextAsGlyphs` (the tab stops), `getTextAsGlyphsUnlocked` (HarfBuzz's advances; the em is
//     `m_outlineFontSizeCompensation` = 1.4 times the size; a sub- or superscript at `m_subscriptSuperscriptSize` = 0.64 of the face size, 0.25 of it down or 0.45 up;
//     the box from the ascender to the descender), `GetInterline` (the face's `height / units_per_EM`, an integer division, times `METRICS::m_InterlinePitch`);
//   - `include/font/font_metrics.h` (`m_InterlinePitch` 1.68, `m_OverbarHeight` 1.23), `include/font/font.h` (`ITALIC_TILT` 1/8), `include/markup_parser.h` (the grammar);
//   - `common/eda_text.cpp`: `EDA_TEXT::GetTextBox` (the box a selection, a hit test and a flipped view read), `GetEffectiveTextPenWidth`; `common/gr_text.cpp`
//     (`GetPenSizeForBold`, `GetPenSizeForNormal`, `ClampTextPenSize`), `include/gr_text.h::GetKnockoutTextMargin`; `pcbnew/pcb_text.cpp::buildBoundingHull` (a knockout's box).
//
// The browser stands in for FreeType and HarfBuzz: an outline face's advances and ink are measured by the canvas (`ctx.measureText`, which shapes with HarfBuzz too, kerning
// and ligatures included) through `OutlineFace`, and its vertical metrics are read from the font file the server's fontconfig picked (`components/text/outlineFonts.ts`).
// Differences (documented in PARITY-pcb section 94): HarfBuzz's per-glyph advances are truncated to whole face units by KiCad (`VECTOR2I cursor`), here the line is measured
// whole (under 0.001 of the size a glyph); FreeType's hinting at 358 pixels to the em is not applied; a fake bold (`FT_Outline_Embolden`, 1/358 of the em) is not drawn;
// a text's URLs are not split from it (`MARKUP::url`; they are drawn the same); an outline glyph is hit by its ink box, not its triangles.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`); the outline font's measurements come in through `OutlineFace`.
import { getGlyph, strokeOverbarHeight } from "../components/text/strokeFont";

export type HJustify = "left" | "center" | "right";
export type VJustify = "top" | "center" | "bottom";
export type P = [number, number];

/** `KICAD_FONT_NAME` (`include/font/kicad_font_name.h`). */
export const KICAD_FONT_NAME = "KiCad Font";
/** eeschema's name for the schematic's default font (`_( "Default Font" )`, `FONT::IsStroke`). */
export const DEFAULT_FONT_NAME = "Default Font";

/** `FONT::GetFont`: no face, or one that starts with "KiCad Font", is the stroke font; `FONT::IsStroke` also takes "Default Font". */
export function isStrokeFace(face: string | null | undefined): boolean {
  return !face || face.startsWith(KICAD_FONT_NAME) || face === DEFAULT_FONT_NAME;
}

/** `ITALIC_TILT` (`include/font/font.h`). */
export const ITALIC_TILT = 1 / 8;
/** `METRICS::m_InterlinePitch` (`include/font/font_metrics.h`); `m_OverbarHeight` (1.23) is the stroke font's current one (`strokeOverbarHeight`: a sheet's
 * Formatting while it is drawn, `SCH_ITEM::GetFontMetrics`). */
const INTERLINE_PITCH = 1.68;
/** `STROKE_FONT::GetInterline`'s `LEGACY_FACTOR`. */
const LEGACY_FACTOR = 0.9583;
/** `STROKE_FONT::GetTextAsGlyphs`' constants. */
const TAB_WIDTH = 4;
const INTER_CHAR = 0.2;
const SUPER_SUB_SIZE_MULTIPLIER = 0.8;
const SUPER_HEIGHT_OFFSET = 0.35;
const SUB_HEIGHT_OFFSET = 0.15;
/** `OUTLINE_FONT`: `m_faceSize` 16 points, `m_charSizeScaler` 64, `m_outlineFontSizeCompensation` 1.4 -- `faceSize()` is the int 1433 -- and
 * `subscriptSize()` = `KiROUND( faceSize() * m_subscriptSuperscriptSize )` = 917; `m_subscriptVerticalOffset` -0.25, `m_superscriptVerticalOffset` 0.45. */
export const OUTLINE_SIZE_COMPENSATION = 1.4;
const FACE_SIZE = Math.trunc(16 * 64 * 1.4);
const SUBSCRIPT_SIZE = Math.round(FACE_SIZE * 0.64);
const SUBSCRIPT_VERTICAL_OFFSET = -0.25;
const SUPERSCRIPT_VERTICAL_OFFSET = 0.45;
/** `OUTLINE_FONT::GetTextAsGlyphs`' `TAB_WIDTH`: four times 0.6 of the size. */
const OUTLINE_TAB_WIDTH = 4 * 0.6;
/** `FT_Matrix` of a fake italic (`m_fakeItal`): "Create a 12 degree slant". */
export const FAKE_ITALIC_ANGLE = (12 * Math.PI) / 180;

/**
 * An outline face as KiCad's `OUTLINE_FONT` sees it: FreeType's `face->ascender`, `descender`, `height` and `units_per_EM` (font units), whether fontconfig gave a
 * face without the bold or the italic asked for (`FF_MISSING_BOLD` / `FF_MISSING_ITAL`: `SetFakeBold` / `SetFakeItal`), and the shaped run's advance and ink at an em of 1
 * (what HarfBuzz's `x_advance`s and the glyph outlines give).
 */
export interface OutlineFace {
  /** The family the face is drawn with (a `FontFace` the browser loaded from the server's file). */
  family: string;
  unitsPerEm: number;
  ascender: number;
  /** Negative below the baseline, as FreeType has it. */
  descender: number;
  height: number;
  fakeBold: boolean;
  fakeItalic: boolean;
  /** The advance of `text` at an em of 1. */
  advance(text: string): number;
  /** The ink of `text` at an em of 1, from its origin on the baseline, y down: `[left, top, right, bottom]`; null for none (a space). */
  ink(text: string): readonly [number, number, number, number] | null;
  /** The polygons of `text`'s glyphs at an em of 1, from its origin on the baseline, y down, a fake italic slanted and a fake bold emboldened (kicad-port/sfnt.ts
   *  `sfntGlyphPolys`: `OUTLINE_FONT::getTextAsGlyphsUnlocked`'s `OUTLINE_GLYPH`s); undefined when the face's outlines are not read. */
  glyphs?(text: string): ReadonlyArray<{ outer: ReadonlyArray<readonly [number, number]>; holes: ReadonlyArray<ReadonlyArray<readonly [number, number]>> }> | undefined;
}

/** The font a text is drawn in: the stroke font, or an outline face. */
export type KicadFont = { kind: "stroke" } | { kind: "outline"; face: OutlineFace };
export const STROKE: KicadFont = { kind: "stroke" };

let resolver: (face: string, bold: boolean, italic: boolean) => KicadFont = () => STROKE;

/** The browser's outline fonts (`components/text/outlineFonts.ts`) answer `resolveFont`; with none (a unit test), every face is the stroke font. */
export function setFontResolver(r: (face: string, bold: boolean, italic: boolean) => KicadFont): void {
  resolver = r;
}

/** `EDA_TEXT::GetDrawFont`: `FONT::GetFont( face, bold, italic )` -- the stroke font, or the outline face once the browser has it. */
export function resolveFont(face: string | null | undefined, bold = false, italic = false): KicadFont {
  return isStrokeFace(face) ? STROKE : resolver(face!, bold, italic);
}

/** What `FONT::Draw` takes of a text: `TEXT_ATTRIBUTES` and the position (`GetTextPos`), in the caller's units (nm on a board and a sheet). */
export interface KicadTextSpec {
  /** The shown text (variables resolved), lines split at `\n`, with KiCad's markup. */
  text: string;
  pos: readonly [number, number];
  /** `GetTextSize()`: the width (x) and the height (y). */
  size: readonly [number, number];
  /** `m_StrokeWidth`: the pen the stroke font is drawn with (`GetEffectiveTextPenWidth`). */
  pen: number;
  halign: HJustify;
  valign: VJustify;
  /** `GetDrawRotation()`, degrees counter-clockwise on the screen (KiCad's sense). */
  angleDeg?: number;
  mirror?: boolean;
  italic?: boolean;
  bold?: boolean;
  /** `m_LineSpacing`, 1 by default. */
  lineSpacing?: number;
  /** `IsMultilineAllowed()` for `GetTextBox` (a `PCB_FIELD` is single-line); the drawing always splits at `\n`. */
  multiline?: boolean;
  /** How many of the caller's units one of KiCad's internal units is: 1 on a board (nm), 100 on a sheet (eeschema's 100 nm). Its ints round to it. */
  iu?: number;
}

// ------------------------------------------------------------------------------------------------------------------------------- pens

/** `GetPenSizeForBold( int )`: a fifth of the size. */
export const penSizeForBold = (size: number): number => kiRound(size / 5);
/** `GetPenSizeForNormal( int )`: an eighth of the size. */
export const penSizeForNormal = (size: number): number => kiRound(size / 8);

/** `ClampTextPenSize( aPenSize, aSize )`: not more than a quarter (`aStrict`: 0.18) of the smaller side. */
export function clampTextPenSize(pen: number, size: readonly [number, number], strict = false): number {
  return Math.min(pen, kiRound(Math.min(Math.abs(size[0]), Math.abs(size[1])) * (strict ? 0.18 : 0.25)));
}

/** `EDA_TEXT::GetEffectiveTextPenWidth( aDefaultPenWidth )`: the text's own thickness, else the default, else the bold or normal pen of the text's width; clamped. */
export function effectiveTextPenWidth(thickness: number, size: readonly [number, number], bold: boolean, defaultPen = 0): number {
  let pen = thickness;
  if (pen <= 1) {
    pen = defaultPen;
    if (bold) pen = penSizeForBold(size[0]);
    else if (pen <= 1) pen = penSizeForNormal(size[0]);
  }
  return clampTextPenSize(pen, size);
}

/** `GetKnockoutTextMargin( aSize, aThickness )`: half the pen, or a ninth of the height. */
export function knockoutTextMargin(size: readonly [number, number], pen: number): number {
  return Math.max(kiRound(pen / 2), kiRound(size[1] / 9));
}

/** `KiROUND`: half away from zero. */
export function kiRound(v: number): number {
  return v < 0 ? -Math.round(-v) : Math.round(v);
}

// ------------------------------------------------------------------------------------------------------------------------------ markup

/** A node of `MARKUP::MARKUP_PARSER`'s tree: a run of text (`anyString`, `anyStringWithinBraces`), or a subscript, superscript or overbar holding more. */
export type MarkupNode = { kind: "text"; text: string } | { kind: "sub" | "sup" | "over"; children: MarkupNode[] };

const MARKUP_KIND: Record<string, "sub" | "sup" | "over"> = { _: "sub", "^": "sup", "~": "over" };
const ESCAPE = /^\{[A-Za-z_][A-Za-z0-9_]*\}/;

/**
 * `MARKUP_PARSER::Parse`: `X{ .. }` (X one of `_`, `^`, `~`) up to its `}` is a node -- runs of text and further markup inside it, an escape `{name}` kept whole -- and
 * a markup with no closing brace is plain text. An empty markup is dropped (`discard_empty`). Outside the braces a `}` is text.
 */
export function parseMarkup(text: string): MarkupNode[] {
  const chars = Array.from(text);
  // `braces< X >` at `i`: the node and where it ends, or null where it does not close.
  const braces = (i: number): [MarkupNode, number] | null => {
    const kind = MARKUP_KIND[chars[i] ?? ""];
    if (!kind || chars[i + 1] !== "{") return null;
    const children: MarkupNode[] = [];
    let run = "";
    let j = i + 2;
    for (;;) {
      if (j >= chars.length) return null;
      if (chars[j] === "}") break;
      const inner = braces(j);
      if (inner) {
        if (run) children.push({ kind: "text", text: run });
        run = "";
        children.push(inner[0]);
        j = inner[1];
        continue;
      }
      const esc = chars[j] === "{" ? ESCAPE.exec(chars.slice(j, j + 202).join("")) : null;
      if (esc) {
        run += esc[0];
        j += Array.from(esc[0]).length;
        continue;
      }
      run += chars[j];
      j++;
    }
    if (run) children.push({ kind: "text", text: run });
    return [{ kind, children }, j + 1];
  };
  const out: MarkupNode[] = [];
  let run = "";
  for (let i = 0; i < chars.length; ) {
    const node = braces(i);
    if (node) {
      if (run) out.push({ kind: "text", text: run });
      run = "";
      if (node[0].kind !== "text" && node[0].children.length > 0) out.push(node[0]);
      i = node[1];
      continue;
    }
    run += chars[i];
    i++;
  }
  if (run) out.push({ kind: "text", text: run });
  return out;
}

/** The text without its markup (what a measure of the plain glyphs reads). */
export function markupPlainText(nodes: readonly MarkupNode[]): string {
  return nodes.map((n) => (n.kind === "text" ? n.text : markupPlainText(n.children))).join("");
}

// ------------------------------------------------------------------------------------------------------------------------------ layout

interface Box {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

/** `BOX2I::Merge`: an uninitialised box takes the other. */
function merge(a: Box | null, b: Box | null): Box | null {
  if (!a) return b;
  if (!b) return a;
  return { x0: Math.min(a.x0, b.x0), y0: Math.min(a.y0, b.y0), x1: Math.max(a.x1, b.x1), y1: Math.max(a.y1, b.y1) };
}

/** A line of the text: its string and where its baseline starts (`aPositions`), in the text's frame -- the caller's units before the mirror and the turn. */
export interface KicadTextLine {
  text: string;
  x: number;
  y: number;
  /** `aExtents[i].x`: the line's advance. */
  extent: number;
}

/** A run of an outline font: its string drawn from `(x, y)` on its baseline at an em of `emX` by `emY` (in the text's frame), slanted when the face is a fake italic. */
export interface OutlineRun {
  text: string;
  x: number;
  y: number;
  emX: number;
  emY: number;
  /** Its advance, the caller's units. */
  advance: number;
}

/** A text laid out: its lines, the stroke font's polylines and the outline font's runs in the text's frame, and how the frame goes on the board. */
export interface KicadTextLayout {
  font: KicadFont;
  lines: KicadTextLine[];
  /** Pen-down polylines of the stroke font's glyphs, and every overbar (of either font), in the text's frame. */
  strokes: P[][];
  /** The outline font's runs (none for the stroke font). */
  runs: OutlineRun[];
  pen: number;
  /** The frame: mirrored about `pos.x`, then turned about `pos` by `angleDeg` (`STROKE_GLYPH::Transform`, `getTextAsGlyphsUnlocked`). */
  pos: P;
  angleDeg: number;
  mirror: boolean;
  /** A fake italic face's slant (`m_fakeItal`): its runs are sheared by 12 degrees about their baselines. */
  fakeItalic: boolean;
}

const SPACE_WIDTH = getGlyph(32).width;

/** The internal unit's rounding: `KiROUND` and the truncation of an int, to whole `iu`. */
function rounding(iu: number) {
  return {
    round: (v: number) => (iu === 1 ? kiRound(v) : kiRound(v / iu) * iu),
    trunc: (v: number) => (iu === 1 ? Math.trunc(v) : Math.trunc(v / iu) * iu),
  };
}

interface RunStyle {
  sub: boolean;
  sup: boolean;
  italic: boolean;
}

/** Where a run ends (`GetTextAsGlyphs`' return), the box it adds (`aBBox`), and what it draws. */
interface RunResult {
  end: P;
  box: Box | null;
}

interface Sink {
  strokes: P[][];
  runs: OutlineRun[];
}

/** `STROKE_FONT::GetTextAsGlyphs` for one run from `position`, its glyphs (when `sink`) in the text's frame. */
function strokeRun(text: string, size: readonly [number, number], position: P, style: RunStyle, iu: number, sink: Sink | null): RunResult {
  const r = rounding(iu);
  const cursor: P = [position[0], position[1]];
  let glyph: P = [size[0], size[1]];
  const tilt = style.italic ? ITALIC_TILT : 0;
  let charCount = 0;
  if (style.sub || style.sup) {
    glyph = [glyph[0] * SUPER_SUB_SIZE_MULTIPLIER, glyph[1] * SUPER_SUB_SIZE_MULTIPLIER];
    if (style.sub) cursor[1] = r.trunc(cursor[1] + glyph[1] * SUB_HEIGHT_OFFSET);
    else cursor[1] = r.trunc(cursor[1] - glyph[1] * SUPER_HEIGHT_OFFSET);
  }
  for (const c of text) {
    if (c === "\t") {
      // "Handle tabs as locked to the next 4th column (in base-widths)."
      charCount = (Math.trunc(charCount / TAB_WIDTH) + 1) * TAB_WIDTH - 1;
      let next = r.trunc(position[0] + size[0] * charCount + size[0] * SPACE_WIDTH);
      while (next <= cursor[0]) {
        charCount += TAB_WIDTH;
        next += size[0] * TAB_WIDTH;
      }
      cursor[0] = next;
    } else if (c === " ") {
      cursor[0] += r.round(glyph[0] * SPACE_WIDTH);
    } else {
      // "Filtering non existing glyphs and non printable chars": `getGlyph` gives '?' for them
      const g = getGlyph(c.codePointAt(0) ?? 63);
      if (sink) {
        for (const stroke of g.strokes) {
          sink.strokes.push(
            stroke.map(([gx, gy]) => {
              const px = gx * glyph[0];
              const py = gy * glyph[1];
              return [(tilt ? px - py * tilt : px) + cursor[0], py + cursor[1]];
            })
          );
        }
      }
      cursor[0] += r.round(g.width * glyph[0]);
    }
    charCount++;
  }
  const endX = cursor[0] - r.round(glyph[0] * INTER_CHAR);
  const endY = r.trunc(cursor[1] - glyph[1]);
  const box = { x0: Math.min(position[0], endX), y0: Math.min(position[1], endY), x1: Math.max(position[0], endX), y1: Math.max(position[1], endY) };
  return { end: [cursor[0], position[1]], box };
}

/** `OUTLINE_FONT::getTextAsGlyphsUnlocked` for one run (no tabs) from `position`. */
function outlineRunNoTabs(face: OutlineFace, text: string, size: readonly [number, number], position: P, style: RunStyle, iu: number, sink: Sink | null): RunResult {
  const r = rounding(iu);
  const supersub = style.sub || style.sup;
  const scaler = supersub ? SUBSCRIPT_SIZE : FACE_SIZE;
  // the glyph's em: the scaler's units (an em of the face at `FT_Set_Char_Size( .., scaler, GLYPH_RESOLUTION )`, decomposed at `GLYPH_SIZE_SCALER`) times `scaleFactor`
  const emX = (scaler / FACE_SIZE) * OUTLINE_SIZE_COMPENSATION * size[0];
  const emY = (scaler / FACE_SIZE) * OUTLINE_SIZE_COMPENSATION * size[1];
  // `pt.y += m_subscriptVerticalOffset * scaler`, in font units (y up), then `pt *= scaleFactor` (y flipped)
  const dy = style.sub ? -SUBSCRIPT_VERTICAL_OFFSET * emY : style.sup ? -SUPERSCRIPT_VERTICAL_OFFSET * emY : 0;
  const advance = face.advance(text) * emX;
  if (sink && text) sink.runs.push({ text, x: position[0], y: position[1] + dy, emX, emY, advance });
  const end: P = [r.trunc(position[0] + advance), position[1]];
  const ascender = Math.abs(face.ascender / face.unitsPerEm) * emY;
  const descender = Math.abs(face.descender / face.unitsPerEm) * emY;
  // `aBBox->Merge( aPosition - VECTOR2I( 0, ascender ) )`, `Merge( aPosition + VECTOR2I( cursor.x * scaleFactor.x, descender ) )`
  const box = { x0: Math.min(position[0], end[0]), y0: r.trunc(position[1] - ascender), x1: Math.max(position[0], end[0]), y1: r.trunc(position[1] + descender) };
  return { end, box };
}

/** `OUTLINE_FONT::GetTextAsGlyphs`: the text cut at its tabs, each tab "locked to the nearest 4th column (in space-widths)" counted from `origin`. */
function outlineRun(face: OutlineFace, text: string, size: readonly [number, number], position: P, origin: P, style: RunStyle, iu: number, sink: Sink | null): RunResult {
  const r = rounding(iu);
  let at: P = [position[0], position[1]];
  let box: Box | null = { x0: position[0], y0: position[1], x1: position[0], y1: position[1] };
  let run = "";
  for (const c of text) {
    if (c === "\t") {
      if (run) {
        const res = outlineRunNoTabs(face, run, size, at, style, iu, sink);
        box = merge(box, res.box);
        at = res.end;
        run = "";
      }
      const tabWidth = r.round(size[0] * OUTLINE_TAB_WIDTH);
      const intrusion = tabWidth ? (at[0] - origin[0]) % tabWidth : 0;
      at = [at[0] + tabWidth - intrusion, at[1]];
    } else run += c;
  }
  if (run) {
    const res = outlineRunNoTabs(face, run, size, at, style, iu, sink);
    box = merge(box, res.box);
    at = res.end;
  }
  return { end: at, box };
}

/** `::drawMarkup`: the nodes from `position`, an overbar over what a `~{}` holds (trimmed by a tenth of the size at each end, 1.23 sizes above its baseline). */
function drawMarkup(nodes: readonly MarkupNode[], font: KicadFont, size: readonly [number, number], position: P, origin: P, style: RunStyle, iu: number, sink: Sink | null, bbox: { box: Box | null }): P {
  let next: P = position;
  for (const node of nodes) {
    if (node.kind === "text") {
      const res = font.kind === "stroke" ? strokeRun(node.text, size, next, style, iu, sink) : outlineRun(font.face, node.text, size, next, origin, style, iu, sink);
      bbox.box = merge(bbox.box, res.box);
      next = res.end;
      continue;
    }
    const inner: RunStyle = { ...style };
    if (node.kind === "sub") inner.sub = true;
    else if (node.kind === "sup") inner.sup = true;
    const start = next;
    next = drawMarkup(node.children, font, size, next, origin, inner, iu, sink, bbox);
    if (node.kind === "over" && sink) {
      const trim = size[0] * 0.1;
      const offset = size[1] * strokeOverbarHeight();
      sink.strokes.push([
        [start[0] + trim, start[1] - offset],
        [next[0] - trim, next[1] - offset],
      ]);
    }
  }
  return next;
}

/** `GetInterline( aGlyphHeight )`: the stroke font's 1.68 heights times the legacy 0.9583; an outline face's 1.68 times its `height / units_per_EM` (integer division). */
export function fontInterline(font: KicadFont, glyphHeight: number): number {
  if (font.kind === "stroke") return glyphHeight * INTERLINE_PITCH * LEGACY_FACTOR;
  const f = font.face;
  const glyphToFontHeight = f.unitsPerEm ? Math.trunc(f.height / f.unitsPerEm) : 1;
  return glyphHeight * glyphToFontHeight * INTERLINE_PITCH;
}

/** `EDA_TEXT::GetInterline`: `KiROUND` of the font's. */
export function textInterline(font: KicadFont, glyphHeight: number, iu = 1): number {
  return rounding(iu).round(fontInterline(font, glyphHeight));
}

/**
 * `FONT::boundingBoxSingleLine`'s extent of one line: where its last glyph leaves the cursor, less where it started. KiCad measures the line at its place with the origin
 * at (0, 0), so an outline font's tab stops count from there.
 */
export function lineAdvance(line: string, font: KicadFont, size: readonly [number, number], italic = false, iu = 1, at: P = [0, 0]): number {
  return drawMarkup(parseMarkup(line), font, size, at, [0, 0], { sub: false, sup: false, italic }, iu, null, { box: null })[0] - at[0];
}

/**
 * `FONT::getLinePositions`: each line `interline` (times the line spacing) below the one before, the block 1.17 sizes high for its first line ("a fudge to match 6.0
 * positioning") and an interline for each other, its first baseline a size below the top, the stroke font's pen fudges ("to match 6.0 positioning": `pen / 1.52` along,
 * `pen * 0.052` up), the block moved up by half or all of its height for a centre or a bottom justification, each line justified on its own.
 */
export function linePositions(spec: KicadTextSpec, font: KicadFont): KicadTextLine[] {
  const iu = spec.iu ?? 1;
  const r = rounding(iu);
  const strings = spec.text.split("\n");
  const interline = r.trunc(fontInterline(font, spec.size[1]) * (spec.lineSpacing ?? 1));
  const [px, py] = spec.pos;
  const extents = strings.map((line, i) => lineAdvance(line, font, spec.size, !!spec.italic, iu, [px, py + i * interline]));
  let height = 0;
  strings.forEach((_, i) => {
    height = i === 0 ? r.trunc(height + spec.size[1] * 1.17) : height + interline;
  });
  let offX = 0;
  let offY = spec.size[1];
  if (font.kind === "stroke") {
    offX = r.trunc(offX + spec.pen / 1.52);
    offY = r.trunc(offY - spec.pen * 0.052);
  }
  if (spec.valign === "center") offY -= r.trunc(height / 2);
  else if (spec.valign === "bottom") offY -= height;
  return strings.map((text, i) => {
    const extent = extents[i]!;
    const x = spec.halign === "left" ? offX : spec.halign === "center" ? -r.trunc(extent / 2) : -(extent + offX);
    return { text, x: px + x, y: py + offY + i * interline, extent };
  });
}

/** `FONT::Draw`: the text's lines placed and their glyphs laid out, in the text's frame. */
export function layoutKicadText(spec: KicadTextSpec, font: KicadFont = STROKE): KicadTextLayout {
  const iu = spec.iu ?? 1;
  const lines = linePositions(spec, font);
  const sink: Sink = { strokes: [], runs: [] };
  const origin: P = [spec.pos[0], spec.pos[1]];
  for (const l of lines) {
    drawMarkup(parseMarkup(l.text), font, spec.size, [l.x, l.y], origin, { sub: false, sup: false, italic: !!spec.italic }, iu, sink, { box: null });
  }
  return {
    font,
    lines,
    strokes: sink.strokes,
    runs: sink.runs,
    pen: spec.pen,
    pos: origin,
    angleDeg: spec.angleDeg ?? 0,
    mirror: !!spec.mirror,
    fakeItalic: font.kind === "outline" && font.face.fakeItalic,
  };
}

/**
 * `FONT::drawSingleLineText` of one line whose baseline starts at `at` and that is mirrored and turned about `at`: a line the server placed (a table cell's,
 * `eda_model::table_text::place_lines_at`, which turns and mirrors each line's start about the cell's `GetDrawPos` -- the same place for its glyphs).
 */
export function layoutLineAt(text: string, at: readonly [number, number], spec: Omit<KicadTextSpec, "text" | "pos" | "halign" | "valign">, font: KicadFont = STROKE): KicadTextLayout {
  const sink: Sink = { strokes: [], runs: [] };
  const origin: P = [at[0], at[1]];
  drawMarkup(parseMarkup(text), font, spec.size, origin, origin, { sub: false, sup: false, italic: !!spec.italic }, spec.iu ?? 1, sink, { box: null });
  return {
    font,
    lines: [{ text, x: at[0], y: at[1], extent: lineAdvance(text, font, spec.size, !!spec.italic, spec.iu ?? 1, origin) }],
    strokes: sink.strokes,
    runs: sink.runs,
    pen: spec.pen,
    pos: origin,
    angleDeg: spec.angleDeg ?? 0,
    mirror: !!spec.mirror,
    fakeItalic: font.kind === "outline" && font.face.fakeItalic,
  };
}

/** The frame of a laid-out text on the board: `(x, y)` in the frame, mirrored about the text's position and turned about it (`RotatePoint`, KiCad's sense). */
export function frameToWorld(l: Pick<KicadTextLayout, "pos" | "angleDeg" | "mirror">, x: number, y: number): P {
  const [ox, oy] = l.pos;
  const fx = l.mirror ? ox - (x - ox) : x;
  const dx = fx - ox;
  const dy = y - oy;
  const a = (l.angleDeg * Math.PI) / 180;
  const [s, c] = [Math.sin(a), Math.cos(a)];
  // `RotatePoint`: x' = y sin + x cos, y' = y cos - x sin
  return [ox + dy * s + dx * c, oy + dy * c - dx * s];
}

/** The inverse of `frameToWorld`. */
export function worldToFrame(l: Pick<KicadTextLayout, "pos" | "angleDeg" | "mirror">, x: number, y: number): P {
  const [ox, oy] = l.pos;
  const dx = x - ox;
  const dy = y - oy;
  const a = (l.angleDeg * Math.PI) / 180;
  const [s, c] = [Math.sin(a), Math.cos(a)];
  const fx = ox + dx * c - dy * s;
  const fy = oy + dx * s + dy * c;
  return [l.mirror ? ox - (fx - ox) : fx, fy];
}

/** One pen stroke on the board: `(x0, y0)` to `(x1, y1)`. */
export type Segment = readonly [number, number, number, number];

/** The stroke font's pen strokes (and the overbars) on the board: `GetEffectiveTextShape`'s `SHAPE_SEGMENT`s, each `layout.pen` wide. */
export function layoutSegments(l: KicadTextLayout): Segment[] {
  const out: Segment[] = [];
  for (const s of l.strokes) {
    for (let i = 1; i < s.length; i++) {
      const a = frameToWorld(l, s[i - 1]![0], s[i - 1]![1]);
      const b = frameToWorld(l, s[i]![0], s[i]![1]);
      out.push([a[0], a[1], b[0], b[1]]);
    }
  }
  return out;
}

/**
 * An outline run's ink, in the text's frame: the face's ink box at the run's em; for a fake italic each glyph's box slanted about its own origin on the baseline
 * (`m_fakeItal`'s `FT_Matrix` turns every glyph's outline as it is loaded) -- the box of the slanted box, a little wider than the slanted glyph where the glyph is
 * narrower at its top (kicad-cli's 'x' of DejaVu Sans: 0.015 of the size); null for a run with none.
 */
export function runInk(l: Pick<KicadTextLayout, "font" | "fakeItalic">, r: OutlineRun): Box | null {
  if (l.font.kind !== "outline") return null;
  const face = l.font.face;
  if (!l.fakeItalic) {
    const ink = face.ink(r.text);
    return ink ? { x0: r.x + ink[0] * r.emX, y0: r.y + ink[1] * r.emY, x1: r.x + ink[2] * r.emX, y1: r.y + ink[3] * r.emY } : null;
  }
  // x' = x cos + (baseline - y) sin: the top leans right
  const [s, c] = [Math.sin(FAKE_ITALIC_ANGLE), Math.cos(FAKE_ITALIC_ANGLE)];
  const chars = Array.from(r.text);
  let box: Box | null = null;
  chars.forEach((ch, i) => {
    const ink = face.ink(ch);
    if (!ink) return;
    const gx = r.x + face.advance(chars.slice(0, i).join("")) * r.emX;
    const [x0, y0, x1, y1] = [ink[0] * r.emX, ink[1] * r.emY, ink[2] * r.emX, ink[3] * r.emY];
    const xs = [x0 * c - y0 * s, x1 * c - y0 * s, x0 * c - y1 * s, x1 * c - y1 * s];
    box = merge(box, { x0: gx + Math.min(...xs), y0: r.y + y0, x1: gx + Math.max(...xs), y1: r.y + y1 });
  });
  return box;
}

/** An outline run's glyph polygons (an outline and its holes each) in the text's frame; null when the face's outlines are not read (the 3D view then draws the run's ink box). */
export function runGlyphs(l: Pick<KicadTextLayout, "font">, r: OutlineRun): Array<{ outer: P[]; holes: P[][] }> | null {
  if (l.font.kind !== "outline" || !l.font.face.glyphs) return null;
  const polys = l.font.face.glyphs(r.text);
  if (!polys) return null;
  const place = (c: ReadonlyArray<readonly [number, number]>): P[] => c.map(([u, v]) => [r.x + u * r.emX, r.y + v * r.emY] as P);
  return polys.map((g) => ({ outer: place(g.outer), holes: g.holes.map(place) }));
}

/**
 * The box of what is drawn, in the text's frame: the strokes grown by half the pen, the outline runs' ink. `buildBoundingHull`'s `poly.BBox()` of the rendered text
 * (`TransformTextToPolySet`) turned back by the text's angle; null for a text that draws nothing.
 */
export function inkBox(l: KicadTextLayout): Box | null {
  let box: Box | null = null;
  const half = l.pen / 2;
  for (const s of l.strokes) for (const [x, y] of s) box = merge(box, { x0: x - half, y0: y - half, x1: x + half, y1: y + half });
  for (const r of l.runs) box = merge(box, runInk(l, r));
  return box;
}

/** A knockout text's box (`buildBoundingHull( &finalPoly, textShape, margin )`), in the text's frame: what is drawn, grown by `GetKnockoutTextMargin`. */
export function knockoutBox(l: KicadTextLayout, size: readonly [number, number]): Box | null {
  const ink = inkBox(l);
  if (!ink) return null;
  const m = knockoutTextMargin(size, l.pen);
  return { x0: ink.x0 - m, y0: ink.y0 - m, x1: ink.x1 + m, y1: ink.y1 + m };
}

/** `FONT::StringBoundaryLimits`: the size of a line's box at the origin, the stroke font's grown by 1.5 pens ("to catch diacriticals, descenders, etc."). */
export function stringBoundaryLimits(text: string, font: KicadFont, size: readonly [number, number], thickness: number, italic: boolean, iu = 1): P {
  const r = rounding(iu);
  const bbox: { box: Box | null } = { box: null };
  drawMarkup(parseMarkup(text), font, size, [0, 0], [0, 0], { sub: false, sup: false, italic }, iu, null, bbox);
  let b = bbox.box ?? { x0: 0, y0: 0, x1: 0, y1: 0 };
  if (font.kind === "stroke") {
    const k = r.round(thickness * 1.5);
    b = { x0: b.x0 - k, y0: b.y0 - k, x1: b.x1 + k, y1: b.y1 + k };
  }
  return [b.x1 - b.x0, b.y1 - b.y0];
}

/**
 * `EDA_TEXT::GetTextBox( aSettings, -1 )`: the box of every line (`StringBoundaryLimits`, the stroke font's 0.17 of it taller), the interline for each line after the first,
 * a sixth more over an overbar, placed on the text's position by the justification (an italic's lean taken off the width, a mirrored left or a right one to the left of it,
 * the stroke font's fudge up at the top or down at the bottom), unturned: `{ x0, y0, x1, y1 }` about `pos` before the text's angle.
 */
export function kicadTextBox(spec: KicadTextSpec, font: KicadFont = STROKE): Box {
  const iu = spec.iu ?? 1;
  const r = rounding(iu);
  const multiline = spec.multiline ?? true;
  const strings = multiline ? spec.text.split("\n") : [spec.text];
  const first = strings[0] ?? "";
  const italic = !!spec.italic;
  let [w, h] = stringBoundaryLimits(first, font, spec.size, spec.pen, italic, iu);
  const fudge = r.round(h * 0.17);
  let textH = h;
  if (font.kind === "stroke") textH += fudge;
  const overbar = first.includes("~{") ? r.trunc(h / 6) : 0;
  if (multiline && strings.length > 1) {
    for (const s of strings.slice(1)) w = Math.max(w, stringBoundaryLimits(s, font, spec.size, spec.pen, italic, iu)[0]);
    // "interline spacing is only *between* lines"
    textH += r.round((strings.length - 1) * fontInterline(font, spec.size[1]));
  }
  textH += overbar;
  let [x, y] = [spec.pos[0], spec.pos[1]];
  const italicOffset = italic ? r.round(spec.size[1] * ITALIC_TILT) : 0;
  if (spec.halign === "left") {
    if (spec.mirror) x -= w - italicOffset;
  } else if (spec.halign === "center") x -= r.trunc((w - italicOffset) / 2);
  else if (!spec.mirror) x -= w - italicOffset;
  if (spec.valign === "top") y -= fudge;
  else if (spec.valign === "center") y -= r.trunc(textH / 2);
  else y = y - textH + fudge;
  return { x0: Math.min(x, x + w), y0: Math.min(y, y + textH), x1: Math.max(x, x + w), y1: Math.max(y, y + textH) };
}

/** The four corners of a box in the text's frame, turned onto the board. */
export function boxCorners(l: Pick<KicadTextLayout, "pos" | "angleDeg">, b: Box): [P, P, P, P] {
  const f = { pos: l.pos, angleDeg: l.angleDeg, mirror: false };
  return [frameToWorld(f, b.x0, b.y0), frameToWorld(f, b.x1, b.y0), frameToWorld(f, b.x1, b.y1), frameToWorld(f, b.x0, b.y1)];
}
