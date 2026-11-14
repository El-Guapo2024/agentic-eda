// Draws a `RULER_ITEM` (kicad-port/rulerItem.ts `rulerDrawing`) on a context already in world coordinates: first the shadow layer's drawing (`getShadowLayer`, the
// strokes and texts wider in `GetShadowColor` of their colour), then the drawing layer's -- the strokes at their widths, the texts in KiCad's stroke font, left or
// right aligned and centred vertically on their anchor, turned by their angle (counterclockwise on screen, as KiCad's). The browser checks read the last ruler of
// each canvas through `window.__edaRuler[tag]` ({ texts, segs, color, shadow }).
import { shadowColorOf, type RulerDrawing } from "../../kicad-port/rulerItem";
import { parseColor } from "../../kicad-port/color4d";
import { drawStrokeText } from "../text/strokeFont";

/** `drawStrokeText` draws on the baseline; KiCad centres a preview's text on its position (`GR_TEXT_V_ALIGN_CENTER`): half the glyph height down, along the text's own up. */
const V_CENTER = 0.5;

/** `GetShadowColor` of a CSS colour (white for one that does not parse). */
function shadowOf(css: string): string {
  const c = parseColor(css);
  return c ? shadowColorOf(c) : "#ffffff";
}

function paintPass(ctx: CanvasRenderingContext2D, r: RulerDrawing, color: string, textColor: string): void {
  ctx.save();
  ctx.strokeStyle = color;
  ctx.lineCap = "round";
  for (const s of r.segs) {
    ctx.lineWidth = s.width;
    ctx.beginPath();
    ctx.moveTo(s.a[0], s.a[1]);
    ctx.lineTo(s.b[0], s.b[1]);
    ctx.stroke();
  }
  ctx.restore();
  for (const t of r.texts) {
    const rad = (-t.angleDeg * Math.PI) / 180; // the canvas turns clockwise
    const off = t.size * V_CENTER;
    drawStrokeText(ctx, t.text, t.at[0] - off * Math.sin(rad), t.at[1] + off * Math.cos(rad), { sizeNm: t.size, thicknessNm: t.stroke, justify: t.halign, angleRad: rad, color: textColor });
  }
}

/**
 * `RULER_ITEM::ViewDraw` on both its layers: `make( true )` is the shadow layer's drawing, `make( false )` the drawing layer's. `color` is the strokes' (`m_color`, else
 * `LAYER_AUX_ITEMS`), `textColor` the cursor strings' (`DrawTextNextToCursor` always takes `LAYER_AUX_ITEMS`); each shadow is `GetShadowColor` of its own colour.
 */
export function paintRuler(ctx: CanvasRenderingContext2D, make: (shadows: boolean) => RulerDrawing, color: string, tag?: string, textColor: string = color): void {
  const shadow = make(true);
  const r = make(false);
  if (tag && typeof window !== "undefined") {
    const w = window as unknown as { __edaRuler?: Record<string, unknown> };
    w.__edaRuler = { ...(w.__edaRuler ?? {}), [tag]: { texts: r.texts.map((t) => t.text), segs: r.segs.length, color, shadow: { segs: shadow.segs.length, texts: shadow.texts.length, color: shadowOf(color), width: shadow.segs[0]?.width, drawnWidth: r.segs[0]?.width } } };
  }
  // the cursor strings are `textColor`, the tick labels the strokes' colour (`font->Draw` with the GAL's stroke colour)
  const cursorStrings = (d: RulerDrawing) => d.texts.filter((t) => t.text.includes(":"));
  const tickLabels = (d: RulerDrawing) => d.texts.filter((t) => !t.text.includes(":"));
  paintPass(ctx, { segs: shadow.segs, texts: tickLabels(shadow) }, shadowOf(color), shadowOf(color));
  paintPass(ctx, { segs: [], texts: cursorStrings(shadow) }, shadowOf(textColor), shadowOf(textColor));
  paintPass(ctx, { segs: r.segs, texts: tickLabels(r) }, color, color);
  paintPass(ctx, { segs: [], texts: cursorStrings(r) }, textColor, textColor);
}
