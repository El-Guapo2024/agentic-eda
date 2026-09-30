// The painter: KiCad's pcb_painter.cpp equivalent. Given a 2D context
// already set up with the view transform (see Canvas.tsx), draws every
// board item this app's model has, in GAL layer order (layers.ts).
// Coordinates throughout are board µm -- the caller's ctx.scale/translate
// does the screen mapping, so this file never touches pixels directly
// except for hairline compensation (view.ts `hairlineUm`) and text size.

import type { BoardState, Part, Pad } from "../../api/types";
import type { ViewTransform } from "../../state/store";
import { hairlineUm } from "./view";
import { layerColor, copperColorKey, drawOrder } from "./layers";
import { minimumSpanningTree, padPointsByNet } from "./ratsnest";

export interface PaintOptions {
  selection: Set<string>;
  hot: Set<string>;
  netHighlight: string | null;
  showRatsnest: boolean;
  layerVisible: Record<string, boolean>;
  layerOpacity: Record<string, number>;
  activeLayer: string | null;
  highContrast: boolean;
  gridUm: number;
  gridVisible: boolean;
  movePreview: { refs: string[]; dxUm: number; dyUm: number } | null;
}

function layerAlpha(opts: PaintOptions, key: string): number {
  if (opts.activeLayer && opts.highContrast && opts.activeLayer !== key) return 0.25;
  return opts.layerOpacity[key] ?? 1;
}

function withAlpha(ctx: CanvasRenderingContext2D, alpha: number, draw: () => void) {
  if (alpha >= 1) return draw();
  if (alpha <= 0) return;
  const prev = ctx.globalAlpha;
  ctx.globalAlpha = prev * alpha;
  draw();
  ctx.globalAlpha = prev;
}

function pathForPad(ctx: CanvasRenderingContext2D, pad: Pad) {
  const isCircle = pad.round && Math.abs(pad.w - pad.h) < 1;
  ctx.beginPath();
  if (isCircle) {
    ctx.arc(pad.x, pad.y, pad.w / 2, 0, Math.PI * 2);
  } else {
    const r = pad.round ? Math.min(pad.w, pad.h) / 2 : Math.min(pad.w, pad.h) * 0.15;
    ctx.roundRect(pad.x - pad.w / 2, pad.y - pad.h / 2, pad.w, pad.h, r);
  }
}

// The full-viewport background fill happens once in Canvas.tsx, in
// screen space, before the world transform -- here we only draw the
// Edge.Cuts stroke itself, no separate fill (KiCad doesn't shade
// "inside the board" differently from "outside" it).
function drawOutline(ctx: CanvasRenderingContext2D, view: ViewTransform, outline: [number, number][] | null) {
  if (!outline || outline.length < 2) return;
  ctx.beginPath();
  outline.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
  ctx.closePath();
  ctx.strokeStyle = layerColor("board_edge");
  ctx.lineWidth = hairlineUm(view, 1.5);
  ctx.stroke();
}

function drawFootprint(ctx: CanvasRenderingContext2D, view: ViewTransform, part: Part, opts: PaintOptions) {
  if (!part.placed || !part.courtyard) return;
  const [x0, y0, x1, y1] = part.courtyard;
  const selected = opts.selection.has(part.ref);
  const isHot = opts.hot.has(part.ref);
  const preview = opts.movePreview?.refs.includes(part.ref) ? opts.movePreview : null;

  ctx.save();
  if (preview) ctx.translate(preview.dxUm, preview.dyUm);

  // Courtyard.
  const courtyardKey = part.side === "bottom" ? "b_courtyard" : "f_courtyard";
  if (selected || isHot || opts.layerVisible[courtyardKey] !== false) {
    withAlpha(ctx, layerAlpha(opts, courtyardKey), () => {
      ctx.beginPath();
      ctx.rect(x0, y0, x1 - x0, y1 - y0);
      ctx.strokeStyle = selected ? layerColor("selection") : isHot ? "#ef5b5b" : layerColor(courtyardKey);
      ctx.lineWidth = hairlineUm(view, selected || isHot ? 2 : 1);
      if (!selected && !isHot) ctx.setLineDash([hairlineUm(view, 3), hairlineUm(view, 2)]);
      ctx.stroke();
      ctx.setLineDash([]);
    });
  }

  // Pads. pcb_painter.cpp: "Pad and via copper ... take their color from
  // the copper layer" -- NOT the net (that heuristic is netColor(),
  // still used for the ratsnest, which has no layer of its own). A
  // through-hole pad's hole wall uses via's "golden copper" for contrast
  // (pcb_painter.cpp's comment, literally); the hole itself would be
  // background-colored, but this app's Pad has no drill-diameter field
  // to size it, so only the wall stroke is drawn.
  const padCopperKey = part.side === "bottom" ? "b_cu" : "f_cu";
  for (const pad of part.pads ?? []) {
    const highlighted = opts.netHighlight && pad.net === opts.netHighlight;
    const fill = highlighted ? "#ffffff" : layerColor(padCopperKey);
    pathForPad(ctx, pad);
    ctx.fillStyle = fill;
    ctx.fill();
    if (pad.th) {
      ctx.strokeStyle = layerColor("pad_th");
      ctx.lineWidth = hairlineUm(view, 1);
      ctx.stroke();
    }
    // Net name, only once the pad is legible on screen.
    if (pad.net && pad.w * view.scale > 22 && pad.h * view.scale > 10) {
      ctx.fillStyle = "rgba(0,0,0,0.75)";
      const fontUm = Math.min(pad.w, pad.h) * 0.35;
      ctx.font = `${fontUm}px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(pad.net, pad.x, pad.y, pad.w * 0.9);
    }
  }

  // Reference designator.
  const fs = Math.max(500, Math.min(900, Math.min(x1 - x0, y1 - y0) * 0.45));
  const cx = (x0 + x1) / 2,
    cy = (y0 + y1) / 2;
  let tx = cx,
    ty = y0 - fs * 0.3,
    align: CanvasTextAlign = "center";
  if (part.label === "below") ty = y1 + fs * 0.95;
  if (part.label === "left") {
    tx = x0 - fs * 0.3;
    ty = cy + fs * 0.35;
    align = "right";
  }
  if (part.label === "right") {
    tx = x1 + fs * 0.3;
    ty = cy + fs * 0.35;
    align = "left";
  }
  const silkKey = part.side === "bottom" ? "b_silks" : "f_silks";
  if (opts.layerVisible[silkKey] !== false) {
    withAlpha(ctx, layerAlpha(opts, silkKey), () => {
      ctx.fillStyle = layerColor(silkKey);
      ctx.font = `600 ${fs}px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`;
      ctx.textAlign = align;
      ctx.textBaseline = "alphabetic";
      ctx.fillText(part.ref, tx, ty);
    });
  }

  ctx.restore();
}

function drawTracksAndVias(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions, wantLayer: "f_cu" | "b_cu" | "inner") {
  if (!board.routing) return;
  for (const t of board.routing.tracks) {
    const key = copperColorKey(t.layer);
    const bucket = key === "f_cu" ? "f_cu" : key === "b_cu" ? "b_cu" : "inner";
    if (bucket !== wantLayer) continue;
    if (opts.layerVisible[t.layer] === false) continue;
    // Slightly translucent tracks (KiCad's copper isn't fully opaque
    // either), via withAlpha's own scoped save/restore -- NOT
    // `ctx.globalAlpha *=`, which was a real bug: multiplying the
    // *current* alpha in place, every track, with nothing to ever
    // restore it, decayed it toward zero over a route's whole track
    // list (141 tracks * 0.92^141 ~= 1e-6), leaving every draw call
    // AFTER the tracks -- every footprint -- effectively invisible.
    // layerAlpha keyed by the model's own layer name ("F.Cu"), matching
    // how layerVisible/layerOpacity are populated (BOARD_OK in
    // state/store.tsx) -- `key` above is this app's lowercase paint
    // bucket ("f_cu"), a different namespace, only used for layerColor().
    withAlpha(ctx, layerAlpha(opts, t.layer) * 0.92, () => {
      ctx.beginPath();
      t.pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.strokeStyle = layerColor(key);
      ctx.lineWidth = Math.max(t.width, hairlineUm(view, 1));
      ctx.lineCap = "round";
      ctx.lineJoin = "round";
      ctx.stroke();
    });
  }
  if (wantLayer === "f_cu") {
    for (const v of board.routing.vias) {
      ctx.beginPath();
      ctx.arc(v.x, v.y, v.d / 2, 0, Math.PI * 2);
      ctx.fillStyle = layerColor("via");
      ctx.fill();
    }
  }
}

function drawRatsnest(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState) {
  const byNet = padPointsByNet(board.parts);
  ctx.strokeStyle = layerColor("ratsnest");
  ctx.lineWidth = hairlineUm(view, 1);
  for (const points of byNet.values()) {
    for (const [a, b] of minimumSpanningTree(points)) {
      ctx.beginPath();
      ctx.moveTo(a[0], a[1]);
      ctx.lineTo(b[0], b[1]);
      ctx.stroke();
    }
  }
}

function drawGrid(ctx: CanvasRenderingContext2D, view: ViewTransform, widthPx: number, heightPx: number, gridUm: number) {
  const stepPx = gridUm * view.scale;
  if (stepPx < 4) return; // too dense to be useful -- KiCad fades its grid out the same way
  const [x0] = [(-view.x) / view.scale];
  const [y0] = [(-view.y) / view.scale];
  const firstX = Math.floor(x0 / gridUm) * gridUm;
  const firstY = Math.floor(y0 / gridUm) * gridUm;
  const wUm = widthPx / view.scale;
  const hUm = heightPx / view.scale;
  ctx.fillStyle = layerColor("grid");
  const r = hairlineUm(view, stepPx < 8 ? 0.6 : 1);
  for (let x = firstX; x <= x0 + wUm + gridUm; x += gridUm) {
    for (let y = firstY; y <= y0 + hUm + gridUm; y += gridUm) {
      ctx.beginPath();
      ctx.arc(x, y, r, 0, Math.PI * 2);
      ctx.fill();
    }
  }
}

/**
 * Paints the whole board into `ctx`, which must already have `view`
 * applied (ctx.translate/scale) -- see Canvas.tsx. Iterates GAL layers in
 * `drawOrder()` so moving to WebGL later only means replacing the
 * per-layer draw calls, not this ordering.
 */
export function paintBoard(ctx: CanvasRenderingContext2D, view: ViewTransform, widthPx: number, heightPx: number, board: BoardState, opts: PaintOptions) {
  const byLayer: Record<string, () => void> = {
    grid: () => opts.gridVisible && drawGrid(ctx, view, widthPx, heightPx, opts.gridUm),
    background: () => drawOutline(ctx, view, board.outline),
    b_cu: () => drawTracksAndVias(ctx, view, board, opts, "b_cu"),
    in2_cu: () => drawTracksAndVias(ctx, view, board, opts, "inner"),
    in1_cu: () => {},
    f_cu: () => drawTracksAndVias(ctx, view, board, opts, "f_cu"),
    ratsnest: () => opts.showRatsnest && !board.routing && drawRatsnest(ctx, view, board),
  };
  for (const key of drawOrder()) byLayer[key]?.();
  // Footprints (courtyard/pads/silk together, so a part's own layers stay coherent) after copper, before selection/cursor.
  for (const part of board.parts) drawFootprint(ctx, view, part, opts);
}
