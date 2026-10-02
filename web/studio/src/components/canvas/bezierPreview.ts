// The in-progress "Draw Bezier Curve" overlay (KIGFX::PREVIEW::BEZIER_ASSISTANT +
// the preview `PCB_SHAPE`, driven by kicad-port/bezierGeom.ts's BEZIER_GEOM_MANAGER
// port): the control arms (start -> C1, end -> C2) and the curve as it would be
// committed, flattened with the same BEZIER_POLY port the finished shape uses.
import { BEZIER_SET_CONTROL1, bezierControlC2, type BezierGeom } from "../../kicad-port/bezierGeom";
import { bezierPolyline } from "../../kicad-port/bezierPoly";
import type { PreviewStyle } from "./arcPreview";

/** `maxError` is the flattening tolerance (um). `ctx` already carries the view transform. */
export function drawBezierPreview(ctx: CanvasRenderingContext2D, g: BezierGeom, style: PreviewStyle, maxError: number): void {
  if (g.step < BEZIER_SET_CONTROL1) return;
  const { color, hair } = style;
  const c2 = bezierControlC2(g);
  ctx.save();
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";

  // Control arms: start -> C1 and end -> C2 (the real, reflected one), dashed and thin like an assistant overlay.
  ctx.lineWidth = hair(1);
  ctx.setLineDash([hair(3), hair(3)]);
  ctx.beginPath();
  ctx.moveTo(g.start[0], g.start[1]);
  ctx.lineTo(g.controlC1[0], g.controlC1[1]);
  if (g.end[0] !== g.start[0] || g.end[1] !== g.start[1]) {
    ctx.moveTo(g.end[0], g.end[1]);
    ctx.lineTo(c2[0], c2[1]);
  }
  ctx.stroke();

  // The curve itself, once there is an end to draw it to.
  if (g.end[0] !== g.start[0] || g.end[1] !== g.start[1]) {
    ctx.lineWidth = hair(2);
    ctx.setLineDash([hair(4), hair(3)]);
    ctx.beginPath();
    bezierPolyline(g.start, g.controlC1, c2, g.end, maxError).forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.stroke();
  }

  // Handles: the four control points.
  ctx.setLineDash([]);
  for (const p of [g.start, g.controlC1, g.end, c2]) {
    ctx.beginPath();
    ctx.arc(p[0], p[1], hair(2.5), 0, Math.PI * 2);
    ctx.fill();
  }
  ctx.restore();
}
