// The point editor's look (`EDIT_POINTS::ViewDraw`, common/tool/edit_points.cpp at 8303b2ad): the indicator lines (an arc's from its centre to its ends, a bezier's
// to its control points), then a square `EDIT_POINT::POINT_SIZE` (8 px) wide on each edit point -- round for a point drawn as a circle (a rectangle's radius point) --
// and a round one on the middle of every line that has a centre point (a rectangle's or a polygon's sides), filled with the background and outlined in the selection
// colour (UI-LEFT S13, UI-LEFT-2 SS3 and YY1). The Symbol Editor paints the same handles (components/symbol/symbolPainter.ts).
import type { ViewTransform } from "../../state/store";
import type { ArcEditPoints } from "../../kicad-port/schArcEdit";
import { lineMid, type EeEditPoints } from "../../kicad-port/eePointEdit";
import { layerColor } from "../canvas/layers";

/** INACTIVE since UI-LEFT-2 SS3: the arc-only editor's painter, replaced by [`paintEditPoints`] for every shape. */
export function paintArcEditPoints(ctx: CanvasRenderingContext2D, view: ViewTransform, e: ArcEditPoints): void {
  paintEditPoints(ctx, view, {
    points: e.points.map((p) => ({ pos: p.pos })),
    lines: e.lines.map((_, i) => ({ a: 3, b: i === 0 ? 0 : 2, center: false, draw: true })),
  });
}

/** Draw the handles of `e` (world coordinates, `view`'s scale), in `fill` / `stroke` (the background and the selection colour by default). */
export function paintEditPoints(ctx: CanvasRenderingContext2D, view: Pick<ViewTransform, "scale">, e: EeEditPoints, colors?: { fill: string; stroke: string }): void {
  const px = (n: number) => n / (view.scale || 1);
  ctx.save();
  ctx.strokeStyle = colors?.stroke ?? layerColor("selection");
  ctx.lineWidth = px(1);
  ctx.setLineDash([px(3), px(3)]);
  ctx.beginPath();
  for (const l of e.lines) {
    if (!l.draw) continue;
    const a = e.points[l.a]!.pos;
    const b = e.points[l.b]!.pos;
    ctx.moveTo(a[0], a[1]);
    ctx.lineTo(b[0], b[1]);
  }
  ctx.stroke();
  ctx.setLineDash([]);
  const half = px(4);
  ctx.fillStyle = colors?.fill ?? layerColor("LAYER_SCHEMATIC_BACKGROUND");
  const square = (x: number, y: number) => {
    ctx.fillRect(x - half, y - half, 2 * half, 2 * half);
    ctx.strokeRect(x - half, y - half, 2 * half, 2 * half);
  };
  const round = (x: number, y: number) => {
    ctx.beginPath();
    ctx.arc(x, y, half, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();
  };
  for (const { pos, circle } of e.points) (circle ? round : square)(pos[0], pos[1]);
  for (const l of e.lines) {
    if (!l.center) continue;
    const m = lineMid(e.points[l.a]!.pos, e.points[l.b]!.pos);
    round(m[0], m[1]);
  }
  ctx.restore();
}
