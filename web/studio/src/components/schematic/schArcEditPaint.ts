// The arc point editor's look (`EDIT_POINTS::ViewDraw`, common/tool/edit_points.cpp at 8303b2ad): the indicator lines dashed from the centre to the two ends,
// then a square `EDIT_POINT::POINT_SIZE` (8 px) wide on each edit point, filled with the background and outlined in the selection colour (UI-LEFT S13).
import type { ViewTransform } from "../../state/store";
import type { ArcEditPoints } from "../../kicad-port/schArcEdit";
import { layerColor } from "../canvas/layers";

export function paintArcEditPoints(ctx: CanvasRenderingContext2D, view: ViewTransform, e: ArcEditPoints): void {
  const px = (n: number) => n / (view.scale || 1);
  ctx.save();
  ctx.strokeStyle = layerColor("selection");
  ctx.lineWidth = px(1);
  ctx.setLineDash([px(3), px(3)]);
  ctx.beginPath();
  for (const [a, b] of e.lines) {
    ctx.moveTo(a[0], a[1]);
    ctx.lineTo(b[0], b[1]);
  }
  ctx.stroke();
  ctx.setLineDash([]);
  const half = px(4);
  ctx.fillStyle = layerColor("LAYER_SCHEMATIC_BACKGROUND");
  for (const { pos } of e.points) {
    ctx.fillRect(pos[0] - half, pos[1] - half, 2 * half, 2 * half);
    ctx.strokeRect(pos[0] - half, pos[1] - half, 2 * half, 2 * half);
  }
  ctx.restore();
}
