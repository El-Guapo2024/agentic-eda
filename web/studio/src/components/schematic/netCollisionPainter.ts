// The overlay a held selection draws where it would short two nets or pull a connection apart (`SCH_DRAG_NET_COLLISION_MONITOR::Update`, eeschema/tools/sch_drag_net_collision.cpp
// at 8303b2ad): a filled and stroked circle at each collision, and for each disconnection a circle at each of the two points and the line between them. The colour is the
// theme's `LAYER_DRAG_NET_COLLISION` (red, alpha 0.8); the fill takes a third of its alpha and the stroke all of it, the line is `drag_net_collision_width` pixels wide (4).
//
// The backend sends the markers with the move preview (`SchMovePatch.net_collision`, crates/ops/src/sch_net_collision.rs); this only draws them.
import type { SchNetCollision } from "../../api/schEditTypes";
import { alphaOf, DRAG_NET_COLLISION_WIDTH_PX, overlayAlphas, withAlpha } from "../../kicad-port/schNetCollision";
import { layerColor } from "../canvas/layers";

/** Draw the overlay on a canvas already set to sheet coordinates (micrometres) at `scale` pixels per micrometre. */
export function paintNetCollision(ctx: CanvasRenderingContext2D, scale: number, o: SchNetCollision): void {
  if (o.collisions.length === 0 && o.disconnections.length === 0) return;
  const base = layerColor("LAYER_DRAG_NET_COLLISION");
  const alphas = overlayAlphas(alphaOf(base));
  ctx.save();
  ctx.fillStyle = withAlpha(base, alphas.fill);
  ctx.strokeStyle = withAlpha(base, alphas.stroke);
  // "m_view->ToWorld( lineWidthPixels )"
  ctx.lineWidth = DRAG_NET_COLLISION_WIDTH_PX / (scale || 1);
  const circle = (x: number, y: number, r: number) => {
    ctx.beginPath();
    ctx.arc(x, y, Math.max(r, 1), 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();
  };
  for (const m of o.collisions) circle(m.at[0], m.at[1], m.radius_nm);
  for (const d of o.disconnections) {
    circle(d.a[0], d.a[1], d.radius_nm);
    circle(d.b[0], d.b[1], d.radius_nm);
    ctx.beginPath();
    ctx.moveTo(d.a[0], d.a[1]);
    ctx.lineTo(d.b[0], d.b[1]);
    ctx.stroke();
  }
  ctx.restore();
}
