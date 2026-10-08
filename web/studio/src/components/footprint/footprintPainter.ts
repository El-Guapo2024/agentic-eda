// The Footprint Editor's own painter (GAPS.md #8) -- pcbnew's
// pcb_painter.cpp as it draws one footprint in isolation (no board
// outline, no other footprints, origin at the footprint's own anchor).
//
// Deliberately a separate small file rather than extending components/
// canvas/painter.ts's `paintBoard`: that file's `Shape`/`Pad` types use
// the *board display* wire shape (`[x,y]` coordinate pairs -- see
// api/types.ts's own doc on why `BoardState`'s `Shape`/`Pad` differ from
// the Cmd-side `CmdShape`/`LibraryPad`), whereas `GET /api/footprint`
// returns the IR's native shape directly (`{x,y}` objects) -- reusing
// `paintBoard`'s per-shape drawing would need an adapter at every call
// site for no real sharing of logic, only of a few primitives. Those few
// (the grid, hairline width compensation, circumcircle-through-3-points
// for an arc, KiCad's own stroke font) *are* reused, imported directly.
import type { ChamferCorners, CmdShape, CmdText, LibraryPad, Um } from "../../api/types";
import type { ViewTransform } from "../../kicad-port/view";
import { hairlineUm } from "../../kicad-port/view";
import { layerColor } from "../canvas/layers";
import { circleThrough, drawGridOrigin, normalizeSweep } from "../canvas/painter";
import { drawArcPreview } from "../canvas/arcPreview";
import { drawBezierPreview } from "../canvas/bezierPreview";
import { BEZIER_MAX_ERROR_UM } from "../canvas/itemHitTest";
import { bezierPolyline } from "../../kicad-port/bezierPoly";
import type { ArcGeom } from "../../kicad-port/arcGeom";
import type { BezierGeom } from "../../kicad-port/bezierGeom";
import { drawStrokeText } from "../text/strokeFont";
import { computeVisibleGridSize, isMajorGridLine, DEFAULT_GRID_STYLE, MAJOR_GRID_LINE_WIDTH_RATIO } from "../../kicad-port/grid";

/** Footprint-local layer name ("F.SilkS") -> colors.json's real key, same punctuation swap `painter.ts`'s own `realLayerKey` uses. */
function realLayerKey(layer: string): string {
  return layer.replace(/\./g, "_");
}

function drawGrid(ctx: CanvasRenderingContext2D, view: ViewTransform, widthPx: number, heightPx: number, gridUm: number, origin: readonly [number, number] = [0, 0]) {
  const visible = computeVisibleGridSize(gridUm, view.scale, DEFAULT_GRID_STYLE);
  const stepPx = visible * view.scale;
  if (!(stepPx > 0)) return;
  // The grid is anchored at the grid origin (`GAL::SetGridOrigin`): dots at `origin + i * visible`.
  const x0 = -view.x / view.scale - origin[0];
  const y0 = -view.y / view.scale - origin[1];
  const wUm = widthPx / view.scale;
  const hUm = heightPx / view.scale;
  const firstIndexX = Math.floor(x0 / visible) - 1;
  const firstIndexY = Math.floor(y0 / visible) - 1;
  const lastIndexX = Math.ceil((x0 + wUm) / visible) + 1;
  const lastIndexY = Math.ceil((y0 + hUm) / visible) + 1;
  ctx.fillStyle = layerColor("grid");
  const minorR = hairlineUm(view, stepPx < 8 ? 0.6 : 1);
  const majorR = minorR * MAJOR_GRID_LINE_WIDTH_RATIO;
  for (let i = firstIndexX; i <= lastIndexX; i++) {
    const x = origin[0] + i * visible;
    const tickX = isMajorGridLine(i);
    for (let j = firstIndexY; j <= lastIndexY; j++) {
      const y = origin[1] + j * visible;
      ctx.beginPath();
      ctx.arc(x, y, tickX && isMajorGridLine(j) ? majorR : minorR, 0, Math.PI * 2);
      ctx.fill();
    }
  }
}

/** The anchor marker at (0,0) -- `footprint_edit_frame.cpp`'s own small crosshair at the footprint's local origin (distinct from the grid origin, which is this same point here -- the footprint editor has no separate "move the grid" concept). */
function drawAnchor(ctx: CanvasRenderingContext2D, view: ViewTransform) {
  const r = hairlineUm(view, 10);
  ctx.save();
  ctx.strokeStyle = layerColor("anchor");
  ctx.lineWidth = hairlineUm(view, 1.5);
  ctx.beginPath();
  ctx.moveTo(-r, 0);
  ctx.lineTo(r, 0);
  ctx.moveTo(0, -r);
  ctx.lineTo(0, r);
  ctx.stroke();
  ctx.restore();
}

/** Builds a rounded-rectangle-with-some-corners-chamfered-instead path, centered at the origin (caller translates/rotates first). `chamfer` is the cut length along each edge from the corner. */
function chamferedRectPath(ctx: CanvasRenderingContext2D, w: Um, h: Um, chamfer: number, corners: ChamferCorners) {
  const hw = w / 2;
  const hh = h / 2;
  const c = Math.max(0, Math.min(chamfer, Math.min(w, h) / 2));
  ctx.beginPath();
  // Clockwise from top-left, cutting a straight chamfer instead of a
  // square corner wherever that corner's flag is set.
  if (corners.top_left) {
    ctx.moveTo(-hw + c, -hh);
  } else {
    ctx.moveTo(-hw, -hh);
  }
  if (corners.top_right) {
    ctx.lineTo(hw - c, -hh);
    ctx.lineTo(hw, -hh + c);
  } else {
    ctx.lineTo(hw, -hh);
  }
  if (corners.bottom_right) {
    ctx.lineTo(hw, hh - c);
    ctx.lineTo(hw - c, hh);
  } else {
    ctx.lineTo(hw, hh);
  }
  if (corners.bottom_left) {
    ctx.lineTo(-hw + c, hh);
    ctx.lineTo(-hw, hh - c);
  } else {
    ctx.lineTo(-hw, hh);
  }
  if (corners.top_left) {
    ctx.lineTo(-hw, -hh + c);
  } else {
    ctx.lineTo(-hw, -hh);
  }
  ctx.closePath();
}

/** A trapezoid centered at the origin: `delta` widens one pair of opposite edges and narrows the other by the same amount, along whichever axis is non-zero -- a reasonable, documented approximation of `PAD_SHAPE::TRAPEZOID` (see PARITY-fpedit.md) rather than a byte-exact port of source's own corner formula. */
function trapezoidPath(ctx: CanvasRenderingContext2D, w: Um, h: Um, delta: [Um, Um]) {
  const hw = w / 2;
  const hh = h / 2;
  const [dx, dy] = delta;
  ctx.beginPath();
  if (dx !== 0) {
    const d = dx / 2;
    ctx.moveTo(-hw - d, -hh);
    ctx.lineTo(hw + d, -hh);
    ctx.lineTo(hw - d, hh);
    ctx.lineTo(-hw + d, hh);
  } else {
    const d = dy / 2;
    ctx.moveTo(-hw, -hh - d);
    ctx.lineTo(hw, -hh + d);
    ctx.lineTo(hw, hh - d);
    ctx.lineTo(-hw, hh + d);
  }
  ctx.closePath();
}

/** The pad's own copper outline as a path, pad-local (origin at the pad's own center, unrotated -- caller translates/rotates to the pad's actual pose first). */
function tracePadPath(ctx: CanvasRenderingContext2D, pad: LibraryPad) {
  const [w, h] = pad.size;
  switch (pad.shape) {
    case "circle":
      ctx.beginPath();
      ctx.arc(0, 0, Math.min(w, h) / 2, 0, Math.PI * 2);
      return;
    case "oval":
      // A stadium: ctx.roundRect with radius = half the shorter side gives
      // exactly two semicircular ends plus straight sides, same shape
      // `PAD_SHAPE::OVAL` draws.
      ctx.beginPath();
      ctx.roundRect(-w / 2, -h / 2, w, h, Math.min(w, h) / 2);
      return;
    case "round_rect":
      ctx.beginPath();
      ctx.roundRect(-w / 2, -h / 2, w, h, (pad.roundrect_ratio ?? 0.25) * Math.min(w, h));
      return;
    case "trapezoid":
      trapezoidPath(ctx, w, h, pad.trapezoid_delta ?? [0, 0]);
      return;
    case "chamfered_rect":
      chamferedRectPath(ctx, w, h, (pad.chamfer_ratio ?? 0.2) * Math.min(w, h), pad.chamfer_corners);
      return;
    case "rect":
    default:
      ctx.beginPath();
      ctx.rect(-w / 2, -h / 2, w, h);
  }
}

function drawPad(ctx: CanvasRenderingContext2D, view: ViewTransform, pad: LibraryPad, opts: { selected: boolean }) {
  // `PAD::SetOffset`: the copper shape sits at `at + rotate(offset)`
  // (offset is defined in the pad's own, pre-rotation local frame, so it
  // turns with the pad) while the drill stays exactly at `at` -- the two
  // nested `save/translate` scopes below give the copper its own origin
  // without moving the drill drawn afterward in the outer (un-offset) one.
  ctx.save();
  ctx.translate(pad.at.x, pad.at.y);
  ctx.rotate((pad.rot / 1000) * (Math.PI / 180));

  ctx.save();
  ctx.translate(pad.offset.x, pad.offset.y);
  tracePadPath(ctx, pad);
  ctx.fillStyle = layerColor("f_cu");
  ctx.fill();
  if (opts.selected) {
    ctx.strokeStyle = layerColor("selection");
    ctx.lineWidth = hairlineUm(view, 2.5);
    ctx.stroke();
  }
  ctx.restore();

  if (pad.kind !== "smd") {
    // A through-hole/non-plated pad's drill, same "golden copper" hole-wall convention painter.ts's own board pads use.
    const drillR = pad.drill_slot ? Math.min(pad.drill_slot[0], pad.drill_slot[1]) / 2 : (pad.drill ?? 0) / 2;
    if (drillR > 0) {
      ctx.beginPath();
      if (pad.drill_slot) {
        const [dw, dh] = pad.drill_slot;
        ctx.roundRect(-dw / 2, -dh / 2, dw, dh, Math.min(dw, dh) / 2);
      } else {
        ctx.arc(0, 0, drillR, 0, Math.PI * 2);
      }
      ctx.fillStyle = layerColor("background");
      ctx.fill();
      ctx.strokeStyle = layerColor("pad_th");
      ctx.lineWidth = hairlineUm(view, 1);
      ctx.stroke();
    }
  }
  ctx.restore();

  // Pad number, centered, once legible.
  if (pad.size[0] * view.scale > 22 && pad.size[1] * view.scale > 10) {
    const fontUm = Math.min(pad.size[0], pad.size[1]) * 0.35;
    drawStrokeText(ctx, pad.number, pad.at.x, pad.at.y + fontUm * 0.35, { sizeUm: fontUm, justify: "center", thicknessUm: fontUm / 6, color: "rgba(0,0,0,0.75)" });
  }
}

function drawShapeGeometry(ctx: CanvasRenderingContext2D, s: CmdShape) {
  ctx.beginPath();
  switch (s.kind) {
    case "segment":
      ctx.moveTo(s.start.x, s.start.y);
      ctx.lineTo(s.end.x, s.end.y);
      ctx.stroke();
      return;
    case "rect":
      ctx.rect(s.start.x, s.start.y, s.end.x - s.start.x, s.end.y - s.start.y);
      break;
    case "circle": {
      const r = Math.hypot(s.end.x - s.center.x, s.end.y - s.center.y);
      ctx.arc(s.center.x, s.center.y, r, 0, Math.PI * 2);
      break;
    }
    case "polygon":
      s.pts.forEach((p, i) => (i === 0 ? ctx.moveTo(p.x, p.y) : ctx.lineTo(p.x, p.y)));
      ctx.closePath();
      break;
    case "bezier":
      // `BEZIER_POLY::GetPoly` at the board's max error (an open curve is never filled).
      bezierPolyline([s.start.x, s.start.y], [s.c1.x, s.c1.y], [s.c2.x, s.c2.y], [s.end.x, s.end.y], BEZIER_MAX_ERROR_UM).forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.stroke();
      return;
    case "arc": {
      const a = circleThrough([s.start.x, s.start.y], [s.mid.x, s.mid.y], [s.end.x, s.end.y]);
      if (!a) {
        ctx.moveTo(s.start.x, s.start.y);
        ctx.lineTo(s.end.x, s.end.y);
        ctx.stroke();
        return;
      }
      const [cx, cy, r] = a;
      const a0 = Math.atan2(s.start.y - cy, s.start.x - cx);
      const aMid = Math.atan2(s.mid.y - cy, s.mid.x - cx);
      const a1 = Math.atan2(s.end.y - cy, s.end.x - cx);
      ctx.arc(cx, cy, r, a0, a1, !normalizeSweep(a0, aMid, a1)); // see painter.ts: `normalizeSweep` is true for the INCREASING-angle sweep, `anticlockwise` is its negation
      ctx.stroke();
      return;
    }
  }
  if (s.filled) ctx.fill();
  ctx.stroke();
}

function drawGraphic(ctx: CanvasRenderingContext2D, view: ViewTransform, s: CmdShape, selected: boolean) {
  ctx.save();
  const color = selected ? layerColor("selection") : layerColor(realLayerKey(s.layer));
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = Math.max(s.stroke_width, hairlineUm(view, selected ? 2 : 1));
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  drawShapeGeometry(ctx, s);
  ctx.restore();
}

function drawText(ctx: CanvasRenderingContext2D, view: ViewTransform, t: CmdText, selected: boolean) {
  const color = selected ? layerColor("selection") : layerColor(realLayerKey(t.layer));
  const angleRad = (-t.angle / 1000) * (Math.PI / 180);
  drawStrokeText(ctx, t.content, t.at.x, t.at.y, { sizeUm: Math.max(t.size_um, hairlineUm(view, 8)), thicknessUm: t.stroke_width, justify: t.justify, angleRad, mirror: t.mirror, color });
}

/** Rubber-band preview for the graphics tool currently in progress (click-to-add-points, same convention the PCB tab's own `drawInProgress` uses -- including drawing it in the selection color regardless of target layer). */
function drawInProgress(ctx: CanvasRenderingContext2D, view: ViewTransform, draw: FpPaintDrawState | null, cursorUm: { x: number; y: number } | null) {
  if (!draw) return;
  // `drawArc` / `drawOneBezier`: the construction managers' own geometry, not a polyline through the clicks.
  const style = { color: layerColor("selection"), hair: (px: number) => hairlineUm(view, px), units: "mm" as const };
  if (draw.shapeKind === "arc" && draw.arc) {
    drawArcPreview(ctx, draw.arc, style);
    return;
  }
  if (draw.shapeKind === "bezier" && draw.bezier) {
    drawBezierPreview(ctx, draw.bezier, style, BEZIER_MAX_ERROR_UM);
    return;
  }
  const pts = draw.pts.slice();
  if (cursorUm) pts.push([cursorUm.x, cursorUm.y]);
  if (pts.length < 2) return;
  ctx.save();
  ctx.strokeStyle = layerColor("selection");
  ctx.lineWidth = hairlineUm(view, 1.5);
  ctx.setLineDash([hairlineUm(view, 4), hairlineUm(view, 3)]);
  if (draw.shapeKind === "circle") {
    const [cx, cy] = pts[0]!;
    const [ex, ey] = pts[pts.length - 1]!;
    ctx.beginPath();
    ctx.arc(cx, cy, Math.hypot(ex - cx, ey - cy), 0, Math.PI * 2);
    ctx.stroke();
  } else if (draw.shapeKind === "rect") {
    const [x0, y0] = pts[0]!;
    const [x1, y1] = pts[pts.length - 1]!;
    ctx.strokeRect(Math.min(x0, x1), Math.min(y0, y1), Math.abs(x1 - x0), Math.abs(y1 - y0));
  } else {
    ctx.beginPath();
    pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.stroke();
  }
  ctx.setLineDash([]);
  ctx.restore();
}

export interface FpPaintOptions {
  selection: Set<string>;
  gridUm: number;
  gridVisible: boolean;
  /** The point the grid is anchored at (`common.Control.gridSetOrigin` in this editor): the dots follow it and its marker is drawn when it is not at (0, 0). */
  gridOrigin?: [number, number] | null;
  drawState: FpPaintDrawState | null;
  cursorUm: { x: number; y: number } | null;
  movePreview: { refs: string[]; dxUm: number; dyUm: number } | null;
  /** High Contrast Mode (`common.Control.highContrastMode`): what is not on `activeLayer` is drawn dimmed. */
  highContrast?: boolean;
  activeLayer?: string;
}

/** What High Contrast Mode leaves of an item off the active layer (the board painter's `layerAlpha` uses the same 0.25). */
const DIMMED_ALPHA = 0.25;

/**
 * `PCB_PAINTER`'s `m_contrastModeDisplay == DIMMED`: with High Contrast Mode on, an item not on the active layer is dimmed. A pad is copper (and mask and paste)
 * and this editor's layer box offers only silkscreen, fab and courtyard, so its pads are always off the active layer.
 */
function contrastAlpha(opts: FpPaintOptions, layer: string | null): number {
  if (!opts.highContrast || !opts.activeLayer) return 1;
  return layer === opts.activeLayer ? 1 : DIMMED_ALPHA;
}

/** The slice of `FpDrawState` the painter reads. */
export interface FpPaintDrawState {
  shapeKind: "segment" | "arc" | "rect" | "circle" | "polygon" | "bezier";
  pts: [Um, Um][];
  arc?: ArcGeom;
  bezier?: BezierGeom;
}

export function paintFootprint(
  ctx: CanvasRenderingContext2D,
  view: ViewTransform,
  widthPx: number,
  heightPx: number,
  footprint: { pads: LibraryPad[]; graphics: CmdShape[]; texts: CmdText[] } | null,
  opts: FpPaintOptions
) {
  if (opts.gridVisible) drawGrid(ctx, view, widthPx, heightPx, opts.gridUm, opts.gridOrigin ?? [0, 0]);
  drawGridOrigin(ctx, view, opts.gridOrigin, "#1a1a1a"); // the canvas is filled with this colour (FootprintCanvas.tsx)
  if (!footprint) {
    drawAnchor(ctx, view);
    return;
  }

  const moved = (id: string | undefined) => (id && opts.movePreview?.refs.includes(id) ? opts.movePreview : null);

  // Graphics first (silkscreen/fab/courtyard art), then pads on top, same
  // "copper reads above art" ordering the PCB tab's own painter uses.
  for (const g of footprint.graphics) {
    const mv = moved(g.id);
    ctx.save();
    ctx.globalAlpha *= contrastAlpha(opts, g.layer);
    if (mv) ctx.translate(mv.dxUm, mv.dyUm);
    drawGraphic(ctx, view, g, opts.selection.has(g.id ?? ""));
    ctx.restore();
  }
  for (const t of footprint.texts) {
    const mv = moved(t.id);
    ctx.save();
    ctx.globalAlpha *= contrastAlpha(opts, t.layer);
    if (mv) ctx.translate(mv.dxUm, mv.dyUm);
    drawText(ctx, view, t, opts.selection.has(t.id ?? ""));
    ctx.restore();
  }
  for (const p of footprint.pads) {
    const mv = moved(p.id);
    ctx.save();
    ctx.globalAlpha *= contrastAlpha(opts, null);
    if (mv) ctx.translate(mv.dxUm, mv.dyUm);
    drawPad(ctx, view, p, { selected: opts.selection.has(p.id ?? "") });
    ctx.restore();
  }

  drawAnchor(ctx, view);
  drawInProgress(ctx, view, opts.drawState, opts.cursorUm);
}
