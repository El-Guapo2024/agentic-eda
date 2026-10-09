// Draws what the grid helper shows while a tool runs (kicad-port/pcbGridHelper.ts `SnapOverlay`): the snap marker with the icon of what was snapped to, the dashed snap
// guides through the snap line origin and the snap line itself, the auxiliary axes and the construction geometry. A port of KIGFX::SNAP_INDICATOR::ViewDraw
// (common/preview_items/snap_indicator.cpp), KIGFX::ORIGIN_VIEWITEM::ViewDraw (common/origin_viewitem.cpp) and KIGFX::CONSTRUCTION_GEOM::ViewDraw
// (common/preview_items/construction_geom.cpp), commit 8303b2ad. The same painter draws the schematic's (its colours come from the caller).
//
// Everything is drawn in the world frame of the canvas (the context already carries the view's translate and scale); sizes in pixels are divided by the scale.
import type { SnapOverlay } from "../../kicad-port/pcbGridHelper";
import { PT, clipHalfLineToBox, clipLineToBox, box, type Pt } from "../../kicad-port/snapGeom";

export interface OverlayColors {
  /** `LAYER_AUX_ITEMS` / `LAYER_SCHEMATIC_AUX_ITEMS`: the snap marker and its icon, the auxiliary axes. */
  marker: string;
  /** `LAYER_ANCHOR` / `LAYER_SCHEMATIC_ANCHOR`: the construction geometry and the guides. */
  construction: string;
  /** The guide of the direction in use (`Brightened( 0.2 )` of the construction colour). */
  guideActive: string;
  /** False: the dashed guides are not drawn (the schematic's are white in KiCad -- the default sheet colour -- and so never seen). */
  guides?: boolean;
}

/** The visible part of the canvas in world units, `[x0, y0, x1, y1]`. */
export type Viewport = readonly [number, number, number, number];

function dashed(ctx: CanvasRenderingContext2D, a: Pt, b: Pt, dash: number): void {
  ctx.save();
  ctx.setLineDash([dash, dash]);
  ctx.beginPath();
  ctx.moveTo(a[0], a[1]);
  ctx.lineTo(b[0], b[1]);
  ctx.stroke();
  ctx.restore();
}

function cross(ctx: CanvasRenderingContext2D, p: Pt, half: number): void {
  ctx.beginPath();
  ctx.moveTo(p[0] - half, p[1]);
  ctx.lineTo(p[0] + half, p[1]);
  ctx.moveTo(p[0], p[1] - half);
  ctx.lineTo(p[0], p[1] + half);
  ctx.stroke();
}

/** The icon of what was snapped to (`SNAP_INDICATOR::ViewDraw`: "For now, choose the first type that is set"), in a frame of screen pixels centred on the icon. */
function typeIcon(ctx: CanvasRenderingContext2D, types: number): void {
  const size = 16;
  const node = size / 8;
  const dot = (x: number, y: number) => {
    ctx.beginPath();
    ctx.arc(x, y, node, 0, Math.PI * 2);
    ctx.fill();
  };
  const line = (x0: number, y0: number, x1: number, y1: number) => {
    ctx.beginPath();
    ctx.moveTo(x0, y0);
    ctx.lineTo(x1, y1);
    ctx.stroke();
  };
  if (types & PT.CORNER) {
    // DrawCornerIcon: an L with a dot at its corner.
    const cx = -size / 2 + node;
    const cy = -size / 2 + node;
    line(cx, cy, cx + size - node, cy);
    line(cx, cy, cx, cy + size - node);
    dot(cx, cy);
  } else if (types & PT.END) {
    // DrawLineEndpointIcon: a dot at the start of a line.
    const sx = -(size / 2 - node);
    dot(sx, 0);
    line(sx, 0, sx + size - node, 0);
  } else if (types & PT.MID) {
    // DrawMidpointIcon: a dot in the middle of a line.
    dot(0, 0);
    line(-size / 2, 0, size / 2, 0);
  } else if (types & PT.CENTER) {
    // DrawCentrePointIcon: a ring on a cross.
    ctx.beginPath();
    ctx.arc(0, 0, size / 4, 0, Math.PI * 2);
    ctx.stroke();
    line(-size / 2, 0, size / 2, 0);
    line(0, -size / 2, 0, size / 2);
  } else if (types & PT.QUADRANT) {
    // DrawQuadrantPointIcon: a dot at the top of most of a circle.
    const arcRadius = size - node * 2;
    const qy = -(size / 2 - node);
    dot(0, qy);
    ctx.beginPath();
    ctx.arc(0, qy + arcRadius, arcRadius, ((-160 - 90) * Math.PI) / 180, ((140 - 90) * Math.PI) / 180);
    ctx.stroke();
  } else if (types & PT.INTERSECTION) {
    // DrawIntersectionIcon: a dot on a slightly squashed X.
    dot(0, 0);
    line(-size / 2, -size / 3, size / 2, size / 3);
    line(-size / 2, size / 3, size / 2, -size / 3);
  } else if (types & PT.ON_ELEMENT) {
    // DrawOnElementIcon: a dot off to one side of a line.
    dot(size / 4, 0);
    line(-size / 2, 0, size / 2, 0);
  }
}

/** Paints `overlay` into the context, which carries the view's transform (`scale` pixels per world unit). */
export function paintSnapOverlay(ctx: CanvasRenderingContext2D, scale: number, viewport: Viewport, overlay: SnapOverlay, colors: OverlayColors): void {
  if (!(scale > 0)) return;
  const px = 1 / scale;
  const view = box(viewport[0], viewport[1], viewport[2], viewport[3]);
  ctx.save();
  ctx.lineCap = "butt";
  ctx.fillStyle = colors.construction;

  // The auxiliary axes (`m_viewAxis`, a CROSS 20000 px long): a cross through the point across the whole canvas.
  if (overlay.auxAxis) {
    ctx.strokeStyle = colors.marker;
    ctx.globalAlpha = 0.4;
    ctx.lineWidth = px;
    ctx.beginPath();
    ctx.moveTo(viewport[0], overlay.auxAxis[1]);
    ctx.lineTo(viewport[2], overlay.auxAxis[1]);
    ctx.moveTo(overlay.auxAxis[0], viewport[1]);
    ctx.lineTo(overlay.auxAxis[0], viewport[3]);
    ctx.stroke();
    ctx.globalAlpha = 1;
  }

  // CONSTRUCTION_GEOM::ViewDraw: lines and rays run to the edge of the view.
  const snapLine = overlay.snapLine;
  for (const { drawable, persistent, lineWidth } of overlay.construction) {
    ctx.strokeStyle = colors.construction;
    ctx.globalAlpha = persistent ? 1 : 0.8;
    ctx.lineWidth = Math.max(1, lineWidth) * px;
    switch (drawable.t) {
      case "line": {
        const clipped = clipLineToBox(drawable, view);
        if (clipped) dashedOrSolid(ctx, clipped[0], clipped[1], snapLine, px);
        break;
      }
      case "half": {
        const clipped = clipHalfLineToBox(drawable, view);
        if (clipped) dashedOrSolid(ctx, clipped[0], clipped[1], snapLine, px);
        break;
      }
      case "seg":
        dashedOrSolid(ctx, drawable.a, drawable.b, snapLine, px);
        break;
      case "circle":
        ctx.beginPath();
        ctx.arc(drawable.c[0], drawable.c[1], drawable.r, 0, Math.PI * 2);
        ctx.stroke();
        break;
      case "arc":
        ctx.beginPath();
        ctx.arc(drawable.c[0], drawable.c[1], drawable.r, drawable.a0, drawable.a0 + drawable.da, drawable.da < 0);
        ctx.stroke();
        break;
      case "point":
        cross(ctx, drawable.p, 8 * px);
        break;
      case "box":
        ctx.strokeRect(drawable.x0, drawable.y0, drawable.x1 - drawable.x0, drawable.y1 - drawable.y0);
        break;
    }
  }
  ctx.globalAlpha = 1;

  // The snap guides: dashed lines through the snap line origin, the direction in use highlighted.
  for (const g of colors.guides === false ? [] : overlay.guides) {
    const clipped = clipLineToBox({ a: g.a, b: g.b }, view);
    if (!clipped) continue;
    ctx.strokeStyle = g.active ? colors.guideActive : colors.construction;
    ctx.lineWidth = (g.active ? 2 : 1) * px;
    dashed(ctx, clipped[0], clipped[1], 8 * px);
  }

  // The snap line: from its origin to where the cursor snapped, with a marker at the origin when it is long enough.
  if (snapLine) {
    ctx.strokeStyle = colors.construction;
    ctx.lineWidth = px * 1.5;
    dashed(ctx, snapLine.a, snapLine.b, 12 * px);
    const length = Math.hypot(snapLine.b[0] - snapLine.a[0], snapLine.b[1] - snapLine.a[1]);
    if (length > 8 * px) {
      cross(ctx, snapLine.a, 8 * px);
      ctx.beginPath();
      ctx.arc(snapLine.a[0], snapLine.a[1], 8 * px, 0, Math.PI * 2);
      ctx.stroke();
    }
  }

  // The snap marker: ORIGIN_VIEWITEM's circle and cross, 10 px, and the icon at ( +24, +10 ) px.
  if (overlay.snapPoint) {
    const { pos, types } = overlay.snapPoint;
    ctx.strokeStyle = colors.marker;
    ctx.fillStyle = colors.marker;
    ctx.lineWidth = px;
    ctx.beginPath();
    ctx.arc(pos[0], pos[1], 10 * px, 0, Math.PI * 2);
    ctx.stroke();
    cross(ctx, pos, 10 * px);
    ctx.translate(pos[0] + 24 * px, pos[1] + 10 * px);
    ctx.scale(px, px);
    ctx.lineWidth = 1;
    typeIcon(ctx, types);
  }
  ctx.restore();
}

/** A construction line, not drawn where the snap line already runs along it ("Avoid fighting with the snap line"). */
function dashedOrSolid(ctx: CanvasRenderingContext2D, a: Pt, b: Pt, snapLine: SnapOverlay["snapLine"], px: number): void {
  if (snapLine && Math.hypot(snapLine.b[0] - snapLine.a[0], snapLine.b[1] - snapLine.a[1]) >= 10 * px) {
    // Collinear within one unit of the line's length: the snap line is drawn instead.
    const dx = b[0] - a[0];
    const dy = b[1] - a[1];
    const len = Math.hypot(dx, dy) || 1;
    const off = (p: Pt) => Math.abs(((p[0] - a[0]) * dy - (p[1] - a[1]) * dx) / len);
    if (off(snapLine.a) <= px && off(snapLine.b) <= px) return;
  }
  ctx.beginPath();
  ctx.moveTo(a[0], a[1]);
  ctx.lineTo(b[0], b[1]);
  ctx.stroke();
}
