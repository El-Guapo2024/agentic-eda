// The net-collision overlay of a held selection, the parts that are not drawing: when there is anything to show (`SCH_DRAG_NET_COLLISION_MONITOR::Update` returns true and
// `AdjustCursor` turns the cursor into `KICURSOR::WARNING`) and the colours it is drawn in (`COLOR4D::WithAlpha` of the theme's `LAYER_DRAG_NET_COLLISION`: the fill takes a
// third of its alpha and the stroke all of it). components/schematic/netCollisionPainter.ts draws it.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { SchNetCollision } from "../api/schEditTypes";

/** `eeschema_settings.cpp`: `selection.drag_net_collision_width`, pixels. */
export const DRAG_NET_COLLISION_WIDTH_PX = 4;

/** Is there anything for the overlay to draw (`m_hasCollision`)? */
export function hasNetCollision(o: SchNetCollision | null | undefined): o is SchNetCollision {
  return !!o && (o.collisions.length > 0 || o.disconnections.length > 0);
}

/** `rgba( ... )` of a `#rrggbb` or `#rrggbbaa` colour with its alpha replaced by `alpha` (`COLOR4D::WithAlpha`). */
export function withAlpha(color: string, alpha: number): string {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})?$/i.exec(color.trim());
  if (!m) return color;
  return `rgba(${parseInt(m[1]!, 16)}, ${parseInt(m[2]!, 16)}, ${parseInt(m[3]!, 16)}, ${alpha})`;
}

/** The alpha of a `#rrggbb[aa]` colour in 0 to 1 (1 when it has none). */
export function alphaOf(color: string): number {
  const m = /^#[0-9a-f]{6}([0-9a-f]{2})$/i.exec(color.trim());
  return m ? parseInt(m[1]!, 16) / 255 : 1;
}

/** The two alphas of the overlay: "fillAlpha = clamp( baseAlpha * 0.35, 0.05, 1.0 )" and "strokeAlpha = clamp( baseAlpha, 0.05, 1.0 )", the base alpha 1 when the colour has none (0). */
export function overlayAlphas(base: number): { fill: number; stroke: number } {
  const a = base <= 0 ? 1 : base;
  const clamp = (v: number) => Math.min(1, Math.max(0.05, v));
  return { fill: clamp(a * 0.35), stroke: clamp(a) };
}
