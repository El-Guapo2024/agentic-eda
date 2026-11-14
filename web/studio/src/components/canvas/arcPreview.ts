// The in-progress "Draw Arc" overlay: KiCad 25ec2335's `KIGFX::PREVIEW::ARC_ASSISTANT` (kicad-port/arcAssistant.ts, over kicad-port/arcGeom.ts's ARC_GEOM_MANAGER
// port) -- its guides and its cursor strings ("r", "θ", "Δθ", "s") next to the cursor with their drop shadows -- in `LAYER_AUX_ITEMS`, and the arc being drawn as
// the preview item it is (`drawManagedShape` / `DRAWING_TOOL::drawShape`: the shape on its layer at its width, `m_view->Update( &preview )`, [`drawArcItemPreview`]).
// Shared by every editor's painter.
import type { ArcGeom } from "../../kicad-port/arcGeom";
import { arcSessionShape } from "../../kicad-port/arcDrawSession";
import { arcAssistant, arcAssistantTexts, specialAngleColour } from "../../kicad-port/arcAssistant";
import { parseColor } from "../../kicad-port/color4d";
import { paintRuler } from "./rulerPaint";
import type { LengthUnit } from "../../state/units";

export interface PreviewStyle {
  /** The assistant's colour (`LAYER_AUX_ITEMS`; the schematic's `LAYER_SCHEMATIC_AUX_ITEMS`). */
  color: string;
  /** `hairlineNm( view, px )`: a width that is `px` screen pixels wide at the current zoom. */
  hair: (px: number) => number;
  units: LengthUnit;
  /** The canvas background, for `IsBackgroundDark` (the special-angle colour); a light one when not given. */
  background?: string;
}

/** `ctx` already carries the view transform (world units). */
export function drawArcPreview(ctx: CanvasRenderingContext2D, g: ArcGeom, style: PreviewStyle): void {
  const { color, hair } = style;
  const scale = 1 / (hair(1) || 1);
  const d = arcAssistant(g, scale, style.units);
  if (!d) return;
  const bg = style.background ? parseColor(style.background) : null;
  const special = specialAngleColour(!!bg && bg.r * 0.299 + bg.g * 0.587 + bg.b * 0.117 < 0.5);
  ctx.save();
  // `DRAW_CONTEXT::m_lineWidth` 1.0: a hairline
  ctx.lineWidth = hair(1);
  for (const l of d.lines) {
    ctx.strokeStyle = l.special ? special : color;
    ctx.globalAlpha = l.deemph ? 0.5 : 1;
    ctx.setLineDash(l.dash ? [l.dash.fill, l.dash.step - l.dash.fill] : []);
    ctx.beginPath();
    ctx.moveTo(l.a[0], l.a[1]);
    ctx.lineTo(l.b[0], l.b[1]);
    ctx.stroke();
  }
  ctx.setLineDash([]);
  for (const c of d.circles) {
    ctx.strokeStyle = color;
    ctx.globalAlpha = c.deemph ? 0.5 : 1;
    ctx.beginPath();
    ctx.arc(c.center[0], c.center[1], c.radius, 0, Math.PI * 2);
    ctx.stroke();
  }
  ctx.restore();
  // `DrawTextNextToCursor` on both of `ViewGetLayers`' layers: the drop shadows, then the strings (the browser checks read them as `window.__edaRuler.arcAssistant`)
  if (d.strings.length > 0) paintRuler(ctx, (shadows) => ({ segs: [], texts: arcAssistantTexts(d, scale, shadows) }), color, "arcAssistant");
}

/**
 * The arc being drawn as the preview item KiCad draws it (`ApplyToShape` on the tool's `PCB_SHAPE` / `SCH_SHAPE`, then the painter): solid, in its layer's `color`, `widthNm`
 * wide (at least a pixel), from start to end through the middle; nothing while the construction has no sweep. (8303b2ad's board tool had no other look either: the
 * dashed arc this file drew before was the studio's own.)
 */
export function drawArcItemPreview(ctx: CanvasRenderingContext2D, g: ArcGeom, color: string, widthNm: number, hair: (px: number) => number): void {
  const a = arcSessionShape(g);
  if (!a) return;
  const [cx, cy] = a.center;
  const ang = (p: readonly [number, number]) => Math.atan2(p[1] - cy, p[0] - cx);
  const s0 = ang(a.start);
  const m0 = ang(a.mid);
  const e0 = ang(a.end);
  const tau = 2 * Math.PI;
  // the way round that passes the middle
  const ccw = (m0 - s0 + 2 * tau) % tau > (e0 - s0 + 2 * tau) % tau;
  ctx.save();
  ctx.strokeStyle = color;
  ctx.lineWidth = Math.max(widthNm, hair(1));
  ctx.lineCap = "round";
  ctx.beginPath();
  ctx.arc(cx, cy, a.radius, s0, e0, ccw);
  ctx.stroke();
  ctx.restore();
}
