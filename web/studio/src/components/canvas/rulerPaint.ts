// Draws a `RULER_ITEM` (kicad-port/rulerItem.ts `rulerDrawing`) on a context already in world coordinates: the strokes at their widths, the texts in KiCad's
// stroke font, left or right aligned and centred vertically on their anchor, turned by their angle (counterclockwise on screen, as KiCad's).
import type { RulerDrawing } from "../../kicad-port/rulerItem";
import { drawStrokeText } from "../text/strokeFont";

/** `drawStrokeText` draws on the baseline; KiCad centres a preview's text on its position (`GR_TEXT_V_ALIGN_CENTER`): half the glyph height down, along the text's own up. */
const V_CENTER = 0.5;

export function paintRuler(ctx: CanvasRenderingContext2D, r: RulerDrawing, color: string): void {
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
    drawStrokeText(ctx, t.text, t.at[0] - off * Math.sin(rad), t.at[1] + off * Math.cos(rad), { sizeNm: t.size, thicknessNm: t.stroke, justify: t.halign, angleRad: rad, color });
  }
}
