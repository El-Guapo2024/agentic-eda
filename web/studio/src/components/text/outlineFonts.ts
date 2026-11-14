// The outline (TrueType / OpenType) fonts the canvases draw texts in (UI-LEFT-2 CC1): the face KiCad would load for a text, loaded into the browser.
//
// KiCad loads a text's face with `OUTLINE_FONT::LoadFont` (common/font/outline_font.cpp): `FONTCONFIG::FindFont` (common/font/fontconfig.cpp) matches the face, bold and
// italic to a file -- a substitute when the machine lacks the face, a regular file when it lacks the bold or italic (then faked: `SetFakeBold`, `SetFakeItal`) -- and
// FreeType reads it. The studio asks the server's fontconfig the same (`GET /api/fonts/match`, `crates/cli/src/fonts_api.rs`) and takes the file it found
// (`GET /api/fonts/file`), whatever the browser's machine has: kicad-cli plots with the server's fonts, and the browser may be elsewhere. The file is loaded with the
// `FontFace` API under a family of its own, its vertical metrics and its glyphs' boxes read from the file as FreeType reads them (kicad-port/sfnt.ts), and its runs'
// advances measured by the canvas (`measureText`, shaped by the browser's HarfBuzz, kerning and ligatures on as `hb_shape`'s defaults are). A run's ink is its glyphs'
// boxes at those advances (`measureText`'s own ink is rounded out to whole pixels of the browser's glyph size), or `measureText`'s for a font with no `glyf` table.
//
// A canvas draws synchronously: a face not loaded yet is drawn in the stroke font, and once it has loaded the canvases repaint (`noteAssetArrived`, the same version the
// bitmaps move). A face fontconfig cannot match, or a file the browser cannot read (a Type 1 font), stays the stroke font, as `FONT::GetFont` falls back to it.
// A collection's (`.ttc`) face is the one fontconfig named (`FT_New_Face( .., faceIndex, .. )`): `FontFace` takes no index, so that face is cut out of the collection
// into a font file of its own (kicad-port/sfnt.ts `sfntFaceFile`) before it is loaded (UI-LEFT-3 C3-8). A fake bold's ink is the emboldened outline's (one pixel of
// the 358 to the em up and right, `FT_Outline_Embolden`), and the canvas fills its glyphs' polygons (components/text/kicadTextPaint.ts).
import { isStrokeFace, setFontResolver, STROKE, type KicadFont, type OutlineFace } from "../../kicad-port/kicadText";
import { noteAssetArrived } from "../canvas/imageCache";
import { FAKE_BOLD_EM, readSfnt, sfntFaceFile, sfntGlyphPolys, sfntInk, type GlyphPoly, type SfntFace } from "../../kicad-port/sfnt";

type Entry = { state: "loading" } | { state: "ready"; face: OutlineFace; file: string; style: string } | { state: "failed"; reason: string };

const entries = new Map<string, Entry>();
let serial = 0;

/** The font size runs are measured at: large enough that the browser's rounding is nothing. */
const REF_PX = 1000;

function key(face: string, bold: boolean, italic: boolean): string {
  return `${face}\u0000${bold ? "b" : ""}${italic ? "i" : ""}`;
}

/** The font of a text in `face` (bold, italic as the text is): the stroke font for no face, "KiCad Font" or one not loaded (yet); the outline face once it has loaded. */
export function fontFor(face: string | null | undefined, bold = false, italic = false): KicadFont {
  if (isStrokeFace(face)) return STROKE;
  const k = key(face!, bold, italic);
  let e = entries.get(k);
  if (!e) {
    e = { state: "loading" };
    entries.set(k, e);
    void load(k, face!, bold, italic);
  }
  return e.state === "ready" ? { kind: "outline", face: e.face } : STROKE;
}

setFontResolver(fontFor);

/** What the browser checks read: every face asked for and how it went (`window.__eda` has no font state of its own). */
export function fontStates(): { face: string; bold: boolean; italic: boolean; state: string; file?: string; style?: string; family?: string; fakeBold?: boolean; fakeItalic?: boolean; reason?: string }[] {
  return [...entries.entries()].map(([k, e]) => {
    const [face, flags = ""] = k.split("\u0000");
    const base = { face: face ?? "", bold: flags.includes("b"), italic: flags.includes("i"), state: e.state };
    if (e.state === "ready") return { ...base, file: e.file, style: e.style, family: e.face.family, fakeBold: e.face.fakeBold, fakeItalic: e.face.fakeItalic };
    if (e.state === "failed") return { ...base, reason: e.reason };
    return base;
  });
}

/** Resolves once no face is loading (a browser check waits on it before it reads what was drawn). */
export async function fontsSettled(): Promise<void> {
  for (let i = 0; i < 200 && [...entries.values()].some((e) => e.state === "loading"); i++) await new Promise((r) => setTimeout(r, 50));
}

async function load(k: string, face: string, bold: boolean, italic: boolean): Promise<void> {
  const q = `face=${encodeURIComponent(face)}&bold=${bold ? 1 : 0}&italic=${italic ? 1 : 0}`;
  try {
    const m = (await (await fetch(`/api/fonts/match?${q}`)).json()) as { ok: boolean; found?: boolean; file?: string; index?: number; style?: string; fake_bold?: boolean; fake_italic?: boolean };
    if (!m.ok || !m.found) throw new Error(`Error loading font '${face}'.`);
    const res = await fetch(`/api/fonts/file?${q}`);
    if (!res.ok) throw new Error(`${m.file ?? face}: not a font file the canvas reads`);
    // the face fontconfig named, on its own (a collection's other faces cut away: `FontFace` loads a collection's first)
    const bytes = sfntFaceFile(new Uint8Array(await res.arrayBuffer()), m.index ?? 0);
    const sfnt = bytes && readSfnt(bytes, 0);
    if (!bytes || !sfnt) throw new Error(`${m.file ?? face}: no head and hhea tables`);
    const family = `eda-outline-${++serial}`;
    const ff = new FontFace(family, bytes);
    await ff.load();
    document.fonts.add(ff);
    entries.set(k, { state: "ready", face: measuredFace(family, sfnt, !!m.fake_bold, !!m.fake_italic), file: m.file ?? "", style: m.style ?? "" });
  } catch (e) {
    entries.set(k, { state: "failed", reason: e instanceof Error ? e.message : String(e) });
  }
  noteAssetArrived();
}

let measureCtx: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D | null = null;

function context(): CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D {
  if (!measureCtx) {
    measureCtx = typeof OffscreenCanvas !== "undefined" ? new OffscreenCanvas(4, 4).getContext("2d") : document.createElement("canvas").getContext("2d");
    if (!measureCtx) throw new Error("no 2D canvas to measure text with");
  }
  return measureCtx;
}

/** The canvas set to draw runs of `family` at `REF_PX` (what `paintKicadText` sets too). */
export function setRunFont(ctx: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D, family: string): void {
  ctx.font = `${REF_PX}px "${family}"`;
  ctx.fontKerning = "normal";
  ctx.textAlign = "left";
  ctx.textBaseline = "alphabetic";
}

export const RUN_FONT_PX = REF_PX;

function measuredFace(family: string, sfnt: SfntFace, fakeBold: boolean, fakeItalic: boolean): OutlineFace {
  const cache = new Map<string, TextMetrics>();
  const measure = (text: string): TextMetrics => {
    let t = cache.get(text);
    if (!t) {
      const ctx = context();
      setRunFont(ctx, family);
      t = ctx.measureText(text);
      if (cache.size > 4000) cache.clear();
      cache.set(text, t);
    }
    return t;
  };
  const advance = (text: string): number => (text ? measure(text).width / REF_PX : 0);
  const glyphCache = new Map<string, GlyphPoly[] | undefined>();
  return {
    family,
    unitsPerEm: sfnt.unitsPerEm,
    ascender: sfnt.ascender,
    descender: sfnt.descender,
    height: sfnt.height,
    fakeBold,
    fakeItalic,
    advance,
    ink: (text) => {
      const own = sfntInk(sfnt, text, advance);
      // `FT_Outline_Embolden` moves the right and top sides out by the strength, the left and bottom ones not
      if (own && fakeBold) return [own[0], own[1] - FAKE_BOLD_EM, own[2] + FAKE_BOLD_EM, own[3]] as const;
      if (own !== undefined) return own;
      const t = measure(text);
      const [l, r, a, d] = [-t.actualBoundingBoxLeft, t.actualBoundingBoxRight, t.actualBoundingBoxAscent, t.actualBoundingBoxDescent];
      if (!(r > l) || !(a + d > 0)) return null;
      return [l / REF_PX, -a / REF_PX, r / REF_PX, d / REF_PX];
    },
    // the 3D view's glyphs (`BOARD_ADAPTER::addText`'s triangles), at the browser's advances as the ink is
    glyphs: (text) => {
      if (!glyphCache.has(text)) {
        if (glyphCache.size > 2000) glyphCache.clear();
        glyphCache.set(text, sfntGlyphPolys(sfnt, text, advance, fakeItalic, fakeBold));
      }
      return glyphCache.get(text);
    },
  };
}
