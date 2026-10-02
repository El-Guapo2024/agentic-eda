// The in-progress "Draw Arc" overlay (KIGFX::PREVIEW::ARC_ASSISTANT, driven by
// kicad-port/arcGeom.ts's ARC_GEOM_MANAGER port): the radius line(s) of the
// construction plus the arc as it would be drawn, and the radius/angle readout
// ARC_ASSISTANT prints. Shared by the board painter and the footprint editor's
// painter -- both draw the same tool.
import { ARC_SET_START, arcEndRadiusEnd, arcStartRadiusEnd, arcSubtended, arcSweep, type ArcGeom } from "../../kicad-port/arcGeom";
import { drawStrokeText } from "../text/strokeFont";
import { formatLength, type LengthUnit } from "../../state/units";

export interface PreviewStyle {
  color: string;
  /** `hairlineUm( view, px )`: a width that is `px` screen pixels wide at the current zoom. */
  hair: (px: number) => number;
  units: LengthUnit;
}

/** `ctx` already carries the view transform (world micrometres); strokes are dashed, like every other not-yet-committed preview. */
export function drawArcPreview(ctx: CanvasRenderingContext2D, g: ArcGeom, style: PreviewStyle): void {
  if (g.step < ARC_SET_START) return;
  const { color, hair, units } = style;
  const [ox, oy] = g.origin;
  ctx.save();
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = hair(1.5);
  ctx.setLineDash([hair(4), hair(3)]);
  ctx.lineCap = "round";

  const [sx, sy] = arcStartRadiusEnd(g);
  if (g.step === ARC_SET_START) {
    // Centre locked, waiting for the start point: the radius line follows the cursor (`setStart` runs on every motion).
    ctx.beginPath();
    ctx.moveTo(ox, oy);
    ctx.lineTo(sx, sy);
    ctx.stroke();
    ctx.setLineDash([]);
    centreMark(ctx, ox, oy, hair);
    if (g.radius > 0) label(ctx, `R ${formatLength(g.radius, units)}`, (ox + sx) / 2, (oy + sy) / 2 - hair(8), style);
    ctx.restore();
    return;
  }

  // Start locked: both radius lines (the start radius and the end radius at the cursor's angle), then the arc.
  const [ex, ey] = arcEndRadiusEnd(g);
  ctx.beginPath();
  ctx.moveTo(sx, sy);
  ctx.lineTo(ox, oy);
  ctx.lineTo(ex, ey);
  ctx.stroke();
  const sweep = arcSweep(g);
  if (sweep) {
    const a0 = (sweep.startAngle * Math.PI) / 180;
    ctx.beginPath();
    // `ctx.arc` measures angles clockwise from +x on the y-down board -- the same direction of increasing `atan2` angle the stored arc runs in.
    ctx.arc(ox, oy, g.radius, a0, a0 + (sweep.sweep * Math.PI) / 180, false);
    ctx.lineWidth = hair(2);
    ctx.stroke();
  }
  ctx.setLineDash([]);
  centreMark(ctx, ox, oy, hair);
  // ARC_ASSISTANT's readout: the radius, and the angle swept so far.
  label(ctx, `R ${formatLength(g.radius, units)}   ${Math.abs(arcSubtended(g)).toFixed(1)}°`, ox, oy - g.radius - hair(14), style);
  ctx.restore();
}

function centreMark(ctx: CanvasRenderingContext2D, x: number, y: number, hair: (px: number) => number): void {
  ctx.beginPath();
  ctx.arc(x, y, hair(2.5), 0, Math.PI * 2);
  ctx.fill();
}

function label(ctx: CanvasRenderingContext2D, text: string, x: number, y: number, style: PreviewStyle): void {
  drawStrokeText(ctx, text, x, y, { sizeUm: style.hair(12), justify: "center", color: style.color, thicknessUm: style.hair(1.4) });
}
