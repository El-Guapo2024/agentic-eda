// A text drawn on a canvas where KiCad draws it: `kicad-port/kicadText.ts`'s layout (the stroke font's strokes, an outline face's runs), mirrored and turned about
// the text's position, in the text's colour -- `PCB_PAINTER::draw( PCB_TEXT )` / `strokeText`, `SCH_PAINTER::draw( SCH_TEXT )`, `GAL::DrawGlyphs` (UI-LEFT-2 PP1, CC1).
//
// A knockout text (`PCB_TEXT::GetKnockoutCache`: `buildBoundingHull` of what is drawn, grown by `GetKnockoutTextMargin`, less the text) is its box filled in the text's
// colour with the text drawn in the background's over it (the studio's way: the board's text boxes are drawn so too). The flipped view's rule for a text that is not side
// specific is `PCB_PAINTER::draw( PCB_TEXT )`'s: the mirror toggled and the position moved by the width of `GetTextBox` along the text, unless the anchor stays (a dimension).
//
// The browser checks read what was last drawn through `window.__edaText[tag][id]`: the text, its font, its ink's box, its `GetTextBox` corners and its lines' starts on the
// board, and `on`, a point on its first stroke (a press there is on the text, as a person presses on it).
import { FAKE_ITALIC_ANGLE, frameToWorld, inkBox, kicadTextBox, knockoutBox, layoutKicadText, boxCorners, resolveFont, runInk, type KicadFont, type KicadTextLayout, type KicadTextSpec } from "../../kicad-port/kicadText";
import { RUN_FONT_PX, setRunFont } from "./outlineFonts";
import { strokeOverbarHeight } from "./strokeFont";

export interface PaintTextOptions {
  color: string;
  /** The pen the strokes are drawn with when it is not the text's own (Sketch Text Items' one pixel); never thinner than `minPen`. */
  pen?: number;
  minPen?: number;
  /** A knockout text: its box filled in `color`, the text in this colour (the background's) over it. */
  knockout?: string | null;
  /** The view is flipped (`IsFlippedX`) and the text is not side specific: drawn mirrored the other way, moved so it stays over the same place (`keepAnchor`: not moved). */
  flippedView?: boolean;
  keepAnchor?: boolean;
  /** Where `window.__edaText` keeps it: the canvas and the item. */
  record?: { tag: string; id: string };
}

/** The face of a text, as `EDA_TEXT::GetDrawFont` asks for it: `FONT::GetFont( face, bold, italic )`. */
export function textFont(face: string | null | undefined, spec: Pick<KicadTextSpec, "bold" | "italic">): KicadFont {
  return resolveFont(face, !!spec.bold, !!spec.italic);
}

/**
 * `PCB_PAINTER::draw( PCB_TEXT )` on a flipped view, for a text that is not side specific: `textWidth` is `GetTextBox().GetWidth()` (negative for a right justification,
 * none centred) turned by the draw rotation, the position moved by it (back for a mirrored text) and the mirror toggled.
 */
export function flippedViewSpec(spec: KicadTextSpec, font: KicadFont, keepAnchor = false): KicadTextSpec {
  if (keepAnchor) return { ...spec, mirror: !spec.mirror };
  const b = kicadTextBox(spec, font);
  const w = spec.halign === "right" ? -(b.x1 - b.x0) : spec.halign === "center" ? 0 : b.x1 - b.x0;
  const a = ((spec.angleDeg ?? 0) * Math.PI) / 180;
  const [dx, dy] = [w * Math.cos(a), -w * Math.sin(a)];
  const sign = spec.mirror ? -1 : 1;
  return { ...spec, pos: [spec.pos[0] + sign * dx, spec.pos[1] + sign * dy], mirror: !spec.mirror };
}

type TextRecord = { text: string; font: string; ink: [number, number, number, number] | null; box: [number, number][]; lines: { text: string; x: number; y: number }[]; on: [number, number] | null };

function recorder(): Record<string, Record<string, TextRecord>> | null {
  if (typeof window === "undefined") return null;
  const w = window as unknown as { __edaText?: Record<string, Record<string, TextRecord>> };
  return (w.__edaText ??= {});
}

/** The middle of the first stroke of a layout (or of its first outline run's ink), on the board. */
function onInk(l: KicadTextLayout): [number, number] | null {
  const s = l.strokes.find((x) => x.length >= 2);
  if (s) return frameToWorld(l, (s[0]![0] + s[1]![0]) / 2, (s[0]![1] + s[1]![1]) / 2);
  const r = l.runs[0];
  const ink = r ? runInk(l, r) : null;
  return ink ? frameToWorld(l, (ink.x0 + ink.x1) / 2, (ink.y0 + ink.y1) / 2) : null;
}

/** A canvas starts painting: what it drew before is forgotten (`window.__edaText[tag]`). */
export function beginTextRecord(tag: string): void {
  const r = recorder();
  if (r) r[tag] = {};
}

function record(tag: string, id: string, spec: KicadTextSpec, l: KicadTextLayout): void {
  const r = recorder();
  if (!r) return;
  // worked out when a check reads them, not on every paint
  const entry = {} as TextRecord;
  const lazy = <K extends keyof TextRecord>(key: K, get: () => TextRecord[K]) => Object.defineProperty(entry, key, { enumerable: true, get });
  lazy("text", () => spec.text);
  lazy("font", () => (l.font.kind === "stroke" ? "stroke" : l.font.face.family));
  lazy("ink", () => {
    const ink = inkBox(l);
    if (!ink) return null;
    const pts = [frameToWorld(l, ink.x0, ink.y0), frameToWorld(l, ink.x1, ink.y0), frameToWorld(l, ink.x1, ink.y1), frameToWorld(l, ink.x0, ink.y1)];
    return [Math.min(...pts.map((p) => p[0])), Math.min(...pts.map((p) => p[1])), Math.max(...pts.map((p) => p[0])), Math.max(...pts.map((p) => p[1]))];
  });
  lazy("box", () => boxCorners(l, kicadTextBox(spec, l.font)));
  lazy("lines", () => l.lines.map(({ text, x, y }) => ({ text, x, y })));
  // a point on what is drawn, for a check that presses on the text: the middle of its first stroke, or of its first run's ink
  lazy("on", () => onInk(l));
  (r[tag] ??= {})[id] = entry;
}

/** The layouts of the last paints, by what they were laid out from: a canvas repaints on every mouse move, and its texts are mostly the same. */
const layouts = new Map<string, KicadTextLayout>();
const MAX_LAYOUTS = 4000;

function layoutOf(spec: KicadTextSpec, font: KicadFont): KicadTextLayout {
  const key = `${font.kind === "stroke" ? "" : font.face.family}\u0000${strokeOverbarHeight()}\u0000${JSON.stringify(spec)}`;
  let l = layouts.get(key);
  if (!l) {
    l = layoutKicadText(spec, font);
    if (layouts.size >= MAX_LAYOUTS) layouts.delete(layouts.keys().next().value!);
    layouts.set(key, l);
  }
  return l;
}

/** Draws a laid-out text in its frame (the caller's transform is the board's). */
export function paintLayout(ctx: CanvasRenderingContext2D, l: KicadTextLayout, opts: Pick<PaintTextOptions, "color" | "pen" | "minPen">, knockout?: { box: ReturnType<typeof knockoutBox>; background: string }): void {
  ctx.save();
  ctx.translate(l.pos[0], l.pos[1]);
  if (l.angleDeg) ctx.rotate((-l.angleDeg * Math.PI) / 180);
  if (l.mirror) ctx.scale(-1, 1);
  ctx.translate(-l.pos[0], -l.pos[1]);
  let color = opts.color;
  if (knockout?.box) {
    const b = knockout.box;
    ctx.fillStyle = color;
    ctx.fillRect(b.x0, b.y0, b.x1 - b.x0, b.y1 - b.y0);
    color = knockout.background;
  }
  if (l.strokes.length) {
    ctx.strokeStyle = color;
    ctx.lineWidth = Math.max(opts.pen ?? l.pen, opts.minPen ?? 0);
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.beginPath();
    for (const s of l.strokes) {
      s.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      // a one-point stroke (a dot) still shows as the pen's round end
      if (s.length === 1) ctx.lineTo(s[0]![0], s[0]![1]);
    }
    ctx.stroke();
  }
  if (l.runs.length && l.font.kind === "outline") {
    const family = l.font.face.family;
    ctx.fillStyle = color;
    const face = l.font.face;
    for (const r of l.runs) {
      // a fake italic slants each glyph about its own origin (`m_fakeItal`'s `FT_Matrix`, x' = x cos 12 + y sin 12 with y up, set before each glyph is loaded), the
      // advances HarfBuzz gave the run kept: glyph by glyph, each where the run's shaping puts it
      const pieces = l.fakeItalic ? Array.from(r.text).map((ch, i, all) => ({ text: ch, dx: face.advance(all.slice(0, i).join("")) * r.emX })) : [{ text: r.text, dx: 0 }];
      for (const p of pieces) {
        ctx.save();
        ctx.translate(r.x + p.dx, r.y);
        if (l.fakeItalic) ctx.transform(Math.cos(FAKE_ITALIC_ANGLE), 0, -Math.sin(FAKE_ITALIC_ANGLE), 1, 0, 0);
        ctx.scale(r.emX / RUN_FONT_PX, r.emY / RUN_FONT_PX);
        setRunFont(ctx, family);
        ctx.fillText(p.text, 0, 0);
        ctx.restore();
      }
    }
  }
  ctx.restore();
}

/**
 * Draws `spec` in `face` where KiCad draws it, and returns the layout drawn. A knockout's box is its ink's grown by `GetKnockoutTextMargin`; a flipped view's text that is
 * not side specific is drawn as `flippedViewSpec` puts it.
 */
export function paintKicadText(ctx: CanvasRenderingContext2D, spec: KicadTextSpec, face: string | null | undefined, opts: PaintTextOptions): KicadTextLayout {
  const font = textFont(face, spec);
  const drawn = opts.flippedView ? flippedViewSpec(spec, font, opts.keepAnchor) : spec;
  const l = layoutOf(drawn, font);
  paintLayout(ctx, l, opts, opts.knockout ? { box: knockoutBox(l, spec.size), background: opts.knockout } : undefined);
  if (opts.record) record(opts.record.tag, opts.record.id, spec, opts.flippedView ? layoutOf(spec, font) : l);
  return l;
}
