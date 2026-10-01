// KiCad's own "Newstroke" font -- decoder and Canvas2D renderer, ported
// from common/font/stroke_font.cpp (STROKE_FONT::loadNewStrokeFont,
// STROKE_FONT::GetTextAsGlyphs) and the markup handling in
// common/font/font.cpp (wordbreakMarkup) and include/font/font_metrics.h
// (KiCad's own metric constants), against the real source snapshot at
// scratchpad/kicad-src/ for this task. Every glyph shape used here is
// generated data (src/kicad/strokeFont.json, tools/extract-strokefont.js),
// not hand-copied -- see that script's header for the exact codepoint
// ranges kept (Basic Latin, Latin-1 Supplement, Greek and Coptic; this
// app has no use for the source file's other 60000-odd lines of CJK/
// Arabic/Indic/etc glyphs, all in scripts nothing in this data model
// ever produces).
//
// This is every place in the app that draws board/schematic *content*
// text (silkscreen, fab layer, schematic symbols/fields/labels, the
// title block) -- not UI chrome (panels, dialogs, tooltips, the status
// bar), which stays normal CSS/system text throughout.
import strokeFontData from "../../kicad/strokeFont.json";

// ---------------------------------------------------------------------
// Decoder -- stroke_font.cpp's loadNewStrokeFont, ported 1:1.
// ---------------------------------------------------------------------

/** STROKE_FONT_SCALE, stroke_font.cpp. */
const SCALE = 1 / 21;
/** FONT_OFFSET, stroke_font.cpp -- moves the glyph origin to the baseline. */
const FONT_OFFSET = -8;
const R_CODE = "R".charCodeAt(0);

export interface Glyph {
  /** Advance width, in font-design units (already * SCALE; multiply by the caller's font size to get real units). */
  width: number;
  /** One array per pen-down run; each point already has glyphStartX subtracted and FONT_OFFSET applied, exactly as loadNewStrokeFont computes it. */
  strokes: Array<Array<readonly [number, number]>>;
}

const glyphCache = new Map<number, Glyph>();
const glyphs = (strokeFontData as { glyphs: Record<string, string> }).glyphs;

/** Decodes one raw stroke-data string (e.g. "H\\QFSFUGVHWJXNXSWWVYUZS[Q[OZNYMWLSLNMJNHOGQF") into a Glyph. Ported from STROKE_FONT::loadNewStrokeFont's per-glyph loop. */
function decodeGlyph(raw: string): Glyph {
  const c0 = raw.charCodeAt(0);
  const c1 = raw.charCodeAt(1);
  const glyphStartX = (c0 - R_CODE) * SCALE;
  const glyphEndX = (c1 - R_CODE) * SCALE;
  const width = glyphEndX - glyphStartX;

  const strokes: Array<Array<[number, number]>> = [];
  let current: Array<[number, number]> = [];

  for (let i = 2; i + 1 < raw.length; i += 2) {
    const a = raw.charCodeAt(i);
    const b = raw.charCodeAt(i + 1);
    if (a === 32 /* ' ' */ && b === R_CODE) {
      if (current.length > 0) strokes.push(current);
      current = [];
    } else {
      const x = (a - R_CODE) * SCALE - glyphStartX;
      const y = (b - R_CODE + FONT_OFFSET) * SCALE;
      current.push([x, y]);
    }
  }
  if (current.length > 0) strokes.push(current);

  return { width, strokes };
}

/** `?` -- KiCad's own fallback glyph for anything outside the loaded font (GetTextAsGlyphs: `if (dd < 0 || dd >= size) c = '?'`). */
const FALLBACK_CODEPOINT = "?".charCodeAt(0);

export function getGlyph(codepoint: number): Glyph {
  let g = glyphCache.get(codepoint);
  if (g) return g;
  const raw = glyphs[String(codepoint)];
  if (raw === undefined) {
    if (codepoint === FALLBACK_CODEPOINT) {
      // Shouldn't happen ('?' is inside Basic Latin, always extracted),
      // but never recurse infinitely if it somehow did.
      g = { width: 0.6, strokes: [] };
    } else {
      g = getGlyph(FALLBACK_CODEPOINT);
    }
  } else {
    g = decodeGlyph(raw);
  }
  glyphCache.set(codepoint, g);
  return g;
}

/** SPACE's own declared advance width (font-design units) -- GetTextAsGlyphs uses this, not a fixed fraction, to advance the cursor over a space. */
const SPACE_WIDTH = getGlyph(32).width;

// ---------------------------------------------------------------------
// Markup -- ~{overbar}, _{subscript}, ^{superscript}. KiCad's own parser
// (common/markup_parser.cpp) builds a proper tree for word-wrapping
// purposes; this app never wraps board/schematic text (every field here
// is single-line), so this is a simpler flat tokenizer that gets the
// same *visual* result: a stack of active styles, pushed on `X{` and
// popped on the matching `}`, so nested markup (e.g. an overbar around a
// subscript) combines correctly without needing a real parse tree.
// ---------------------------------------------------------------------

export interface TextStyle {
  subscript: boolean;
  superscript: boolean;
  overbar: boolean;
}

interface Token {
  char: string;
  style: TextStyle;
}

const MARKUP_OPEN: Record<string, keyof TextStyle> = { "~": "overbar", _: "subscript", "^": "superscript" };

/** Splits `text` into one token per literal character, each carrying whichever styles are active at that point -- `~{...}`, `_{...}` and `^{...}` are consumed as markup, never drawn as literal characters. */
function tokenize(text: string): Token[] {
  const tokens: Token[] = [];
  const stack: Array<keyof TextStyle> = [];
  const styleNow = (): TextStyle => ({
    subscript: stack.includes("subscript"),
    superscript: stack.includes("superscript"),
    overbar: stack.includes("overbar"),
  });

  for (let i = 0; i < text.length; i++) {
    const c = text[i]!;
    const kind = MARKUP_OPEN[c];
    if (kind && text[i + 1] === "{") {
      stack.push(kind);
      i += 1; // consume the '{'
      continue;
    }
    if (c === "}" && stack.length > 0) {
      stack.pop();
      continue;
    }
    tokens.push({ char: c, style: styleNow() });
  }
  return tokens;
}

// ---------------------------------------------------------------------
// Layout -- STROKE_FONT::GetTextAsGlyphs, ported (single-line only: this
// app never word-wraps board/schematic text).
// ---------------------------------------------------------------------

const SUPER_SUB_SIZE_MULTIPLIER = 0.8;
const SUPER_HEIGHT_OFFSET = 0.35;
const SUB_HEIGHT_OFFSET = 0.15;
/** font_metrics.h METRICS::m_OverbarHeight -- distance above the baseline, as a fraction of glyph size. */
const OVERBAR_HEIGHT = 1.23;

interface PlacedGlyph {
  glyph: Glyph;
  /** Baseline-relative origin for this glyph, in real (world) units. */
  x: number;
  y: number;
  size: number;
  style: TextStyle;
}

interface Layout {
  glyphs: PlacedGlyph[];
  /** Total advance width, real units (cursor end minus start -- matches GetTextAsGlyphs' return value, not the tighter drawMarkup bbox). */
  width: number;
  /** Overbar segments to stroke: one per maximal contiguous overbarred run, [x0, x1, y]. */
  overbars: Array<[number, number, number]>;
}

/** Lays out `text` left-to-right starting at (0,0), baseline at y=0, KiCad's own metrics for sub/superscript sizing and offsets. Reused by both measuring and drawing so they can never disagree. */
function layout(text: string, sizeUm: number): Layout {
  const tokens = tokenize(text);
  const placed: PlacedGlyph[] = [];
  let cursorX = 0;
  const overbars: Array<[number, number, number]> = [];
  let overbarStart: number | null = null;
  let overbarY = 0;

  const flushOverbar = (endX: number) => {
    if (overbarStart !== null) {
      overbars.push([overbarStart, endX, overbarY]);
      overbarStart = null;
    }
  };

  for (const tok of tokens) {
    const scaled = tok.style.subscript || tok.style.superscript ? sizeUm * SUPER_SUB_SIZE_MULTIPLIER : sizeUm;
    const yOffset = tok.style.subscript ? sizeUm * SUB_HEIGHT_OFFSET : tok.style.superscript ? -sizeUm * SUPER_HEIGHT_OFFSET : 0;

    if (tok.style.overbar) {
      if (overbarStart === null) {
        overbarStart = cursorX;
        overbarY = yOffset - scaled * OVERBAR_HEIGHT;
      }
    } else {
      flushOverbar(cursorX);
    }

    if (tok.char === " ") {
      cursorX += scaled * SPACE_WIDTH;
      continue;
    }

    const g = getGlyph(tok.char.codePointAt(0) ?? FALLBACK_CODEPOINT);
    placed.push({ glyph: g, x: cursorX, y: yOffset, size: scaled, style: tok.style });
    cursorX += g.width * scaled;
  }
  flushOverbar(cursorX);

  return { glyphs: placed, width: cursorX, overbars };
}

// ---------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------

export type StrokeJustify = "left" | "center" | "right";

/** Total advance width of `text` set at `sizeUm`, real units -- for callers that need to position text themselves (e.g. right/center-justify against a fixed anchor) before calling drawStrokeText with justify:"left". */
export function measureStrokeText(text: string, sizeUm: number): number {
  return layout(text, sizeUm).width;
}

export interface StrokeTextOptions {
  sizeUm: number;
  /** Stroke width, real units. KiCad's own typical default is ~1/8 of size; callers that carry a real per-item stroke_width (BoardText, Shape, pins) should always pass it explicitly instead of relying on this fallback. */
  thicknessUm?: number;
  justify?: StrokeJustify;
  /** Rotation about (x, y), radians, applied after justify. */
  angleRad?: number;
  /** Horizontal mirror, applied in the text's own (pre-rotation) local frame -- matches this app's other text mirroring (viewer3d/scene.ts, schematic symbols). */
  mirror?: boolean;
  /** font.h's ITALIC_TILT shear, applied the same way stroke_font.cpp's GetTextAsGlyphs does (a per-point x-shear proportional to y, not a font substitution -- stroke fonts have no separate italic glyph set). */
  italic?: boolean;
  color: string;
}

const DEFAULT_THICKNESS_FACTOR = 1 / 8;
/** font.h ITALIC_TILT. */
const ITALIC_TILT = 1 / 8;

/**
 * Draws `text` at (x, y) using KiCad's Newstroke font, stroked (not
 * filled) -- this is a vector font, every glyph is a set of pen strokes,
 * not a filled outline. (x, y) is the text's own baseline-left anchor
 * *before* justify is applied (i.e. pass the same anchor point
 * regardless of justify, same as this app's existing ctx.textAlign
 * convention).
 */
export function drawStrokeText(ctx: CanvasRenderingContext2D, text: string, x: number, y: number, opts: StrokeTextOptions): void {
  if (!text) return;
  const { sizeUm, justify = "left", angleRad = 0, mirror = false, italic = false, color } = opts;
  const thickness = opts.thicknessUm ?? sizeUm * DEFAULT_THICKNESS_FACTOR;
  const { glyphs: placed, width, overbars } = layout(text, sizeUm);
  const startX = justify === "center" ? -width / 2 : justify === "right" ? -width : 0;

  ctx.save();
  ctx.translate(x, y);
  if (angleRad) ctx.rotate(angleRad);
  if (mirror) ctx.scale(-1, 1);
  // x' = x - tilt*y: y is negative "up" in this glyph space (FONT_OFFSET),
  // so this shifts ascenders right of the baseline -- a conventional
  // right-leaning italic, matching stroke_font.cpp's own sign (tilt is
  // added to x the same way there, in the same y-down glyph frame).
  if (italic) ctx.transform(1, 0, -ITALIC_TILT, 1, 0, 0);
  ctx.strokeStyle = color;
  ctx.lineWidth = thickness;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";

  for (const pg of placed) {
    for (const stroke of pg.glyph.strokes) {
      if (stroke.length === 0) continue;
      ctx.beginPath();
      stroke.forEach(([gx, gy], i) => {
        const px = startX + pg.x + gx * pg.size;
        const py = pg.y + gy * pg.size;
        if (i === 0) ctx.moveTo(px, py);
        else ctx.lineTo(px, py);
      });
      ctx.stroke();
    }
  }
  for (const [x0, x1, oy] of overbars) {
    ctx.beginPath();
    ctx.moveTo(startX + x0, oy);
    ctx.lineTo(startX + x1, oy);
    ctx.stroke();
  }
  ctx.restore();
}
