// The in-progress "Draw Arc" overlay: KiCad 25ec2335's `KIGFX::PREVIEW::ARC_ASSISTANT` (kicad-port/arcAssistant.ts, over kicad-port/arcGeom.ts's ARC_GEOM_MANAGER
// port) -- its guides and its cursor strings ("r", "θ", "Δθ", "s") next to the cursor with their drop shadows -- and, for the board's and the Footprint Editor's centre
// mode, the arc as it would be drawn (the schematic and the Symbol Editor paint the arc itself as their preview item). Shared by every editor's painter.
import { ARC_SET_ANGLE, arcSweep, arcUsesExactEndpoints, type ArcGeom } from "../../kicad-port/arcGeom";
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
  /** Draw the centre mode's arc too (the board's and the Footprint Editor's tools); default true. */
  drawArc?: boolean;
  /** The canvas background, for `IsBackgroundDark` (the special-angle colour); a light one when not given. */
  background?: string;
}

/** `ctx` already carries the view transform (world units). */
export function drawArcPreview(ctx: CanvasRenderingContext2D, g: ArcGeom, style: PreviewStyle): void {
  const { color, hair } = style;
  if (style.drawArc !== false && !arcUsesExactEndpoints(g) && g.step === ARC_SET_ANGLE) {
    const sweep = arcSweep(g);
    if (sweep) {
      ctx.save();
      ctx.strokeStyle = color;
      ctx.lineWidth = hair(2);
      ctx.setLineDash([hair(4), hair(3)]);
      const a0 = (sweep.startAngle * Math.PI) / 180;
      ctx.beginPath();
      // `ctx.arc` measures angles clockwise from +x on the y-down board -- the same direction of increasing `atan2` angle the stored arc runs in.
      ctx.arc(g.origin[0], g.origin[1], g.radius, a0, a0 + (sweep.sweep * Math.PI) / 180, false);
      ctx.stroke();
      ctx.restore();
    }
  }
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
