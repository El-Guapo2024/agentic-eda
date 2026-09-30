// The painter: KiCad's pcb_painter.cpp equivalent. Given a 2D context
// already set up with the view transform (see Canvas.tsx), draws every
// board item this app's model has, in GAL layer order (layers.ts).
// Coordinates throughout are board µm -- the caller's ctx.scale/translate
// does the screen mapping, so this file never touches pixels directly
// except for hairline compensation (view.ts `hairlineUm`) and text size.

import type { BoardState, Part, Pad, Shape } from "../../api/types";
import type { DrawState, ToolId, ViewTransform } from "../../state/store";
import { hairlineUm } from "./view";
import { layerColor, copperColorKey, drawOrder } from "./layers";
import { minimumSpanningTree, padPointsByNet } from "./ratsnest";
import { posture45 } from "./routing";
import { snapPoint } from "./gridHelper";
import { drawStrokeText } from "../text/strokeFont";

export interface PaintOptions {
  selection: Set<string>;
  hot: Set<string>;
  netHighlight: string | null;
  showRatsnest: boolean;
  ratsnestCurved: boolean;
  layerVisible: Record<string, boolean>;
  layerOpacity: Record<string, number>;
  activeLayer: string | null;
  highContrast: boolean;
  gridUm: number;
  gridVisible: boolean;
  movePreview: { refs: string[]; dxUm: number; dyUm: number } | null;
  /** The route/zone/drawing tool currently in progress (Canvas.tsx), and the cursor to rubber-band its next point toward -- null cursor (pointer left the canvas, or hasn't moved yet) just skips the rubber-band, still showing the fixed points so far. */
  drawState: DrawState | null;
  cursorUm: { x: number; y: number } | null;
  activeTool: ToolId;
  /**
   * pcbnew.Control.pad/track/viaDisplayMode ("Sketch Pads/Tracks/Vias"):
   * outline instead of filled. KiCad draws a true unfilled outline (two
   * parallel edges for a track, a ring for a via/pad); this simplifies
   * to a thin stroke on the same centerline/outline -- distinguishable
   * from the filled look without offset-polygon geometry for every
   * track segment join.
   */
  sketchPads: boolean;
  sketchTracks: boolean;
  sketchVias: boolean;
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
    if (opts.sketchPads) {
      ctx.strokeStyle = fill;
      ctx.lineWidth = hairlineUm(view, 1.5);
      ctx.stroke();
    } else {
      ctx.fillStyle = fill;
      ctx.fill();
    }
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
      drawStrokeText(ctx, part.ref, tx, ty, { sizeUm: fs, justify: align, thicknessUm: fs / 6, color: layerColor(silkKey) });
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
      ctx.lineWidth = opts.sketchTracks ? hairlineUm(view, 1.5) : Math.max(t.width, hairlineUm(view, 1));
      ctx.lineCap = "round";
      ctx.lineJoin = "round";
      ctx.stroke();
    });
  }
  if (wantLayer === "f_cu") {
    for (const v of board.routing.vias) {
      ctx.beginPath();
      ctx.arc(v.x, v.y, v.d / 2, 0, Math.PI * 2);
      if (opts.sketchVias) {
        ctx.strokeStyle = layerColor("via");
        ctx.lineWidth = hairlineUm(view, 1.5);
        ctx.stroke();
      } else {
        ctx.fillStyle = layerColor("via");
        ctx.fill();
      }
    }
  }
}

/**
 * Zones as outlines only -- fill isn't computed anywhere in this model
 * (no polygon-clipping/thermal-relief engine exists), so drawing a solid
 * copper-colored fill would show area that isn't actually guaranteed
 * copper. KiCad has the same "outline display mode" for exactly this
 * situation (a zone whose fill is stale/not yet run).
 */
function drawZones(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions, wantLayer: "f_cu" | "b_cu" | "inner") {
  if (!board.routing) return;
  for (const z of board.routing.zones) {
    const key = copperColorKey(z.layer);
    const bucket = key === "f_cu" ? "f_cu" : key === "b_cu" ? "b_cu" : "inner";
    if (bucket !== wantLayer) continue;
    if (opts.layerVisible[z.layer] === false) continue;
    if (z.outline.length < 3) continue;
    const selected = opts.selection.has(z.id);
    withAlpha(ctx, layerAlpha(opts, z.layer), () => {
      ctx.beginPath();
      z.outline.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.closePath();
      ctx.strokeStyle = selected ? layerColor("selection") : layerColor(key);
      ctx.lineWidth = hairlineUm(view, selected ? 2.5 : 1.5);
      ctx.setLineDash([hairlineUm(view, 5), hairlineUm(view, 3)]);
      ctx.stroke();
      ctx.setLineDash([]);
    });
  }
}

/** A Shape/Text's own dotted KiCad layer name ("F.SilkS") -> colors.json's real key ("F_SilkS") -- unlike copperColorKey() (which also lowercases, fine for copper's own always-lowercase-safe names but wrong for e.g. "Edge.Cuts" -> "Edge_Cuts"), these carry a real named layer already and just need the punctuation swapped, case preserved. */
function realLayerKey(layer: string): string {
  return layer.replace(/\./g, "_");
}

/** Free-standing board graphics (Place > Line/Arc/Rectangle/Circle/Polygon) -- everything crates/model/src/ir.rs's `Shape` enum can hold, each drawn in its own layer's real color (silkscreen, fab, etc., not just copper). */
function drawShapes(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions) {
  const shapes = board.drawings?.shapes ?? [];
  for (const s of shapes) {
    const bucketish = realLayerKey(s.layer);
    if (opts.layerVisible[s.layer] === false) continue;
    const selected = opts.selection.has(s.id);
    ctx.save();
    ctx.strokeStyle = selected ? layerColor("selection") : layerColor(bucketish);
    ctx.fillStyle = ctx.strokeStyle;
    ctx.lineWidth = Math.max(s.stroke_width, hairlineUm(view, selected ? 2 : 1));
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    drawShapeGeometry(ctx, s);
    ctx.restore();
  }
}

function drawShapeGeometry(ctx: CanvasRenderingContext2D, s: Shape) {
  ctx.beginPath();
  switch (s.kind) {
    case "segment":
      ctx.moveTo(s.start[0], s.start[1]);
      ctx.lineTo(s.end[0], s.end[1]);
      ctx.stroke();
      return;
    case "rect":
      ctx.rect(s.start[0], s.start[1], s.end[0] - s.start[0], s.end[1] - s.start[1]);
      break;
    case "circle": {
      const r = Math.hypot(s.end[0] - s.center[0], s.end[1] - s.center[1]);
      ctx.arc(s.center[0], s.center[1], r, 0, Math.PI * 2);
      break;
    }
    case "polygon":
      s.pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.closePath();
      break;
    case "arc": {
      // Three points (start/mid/end) on the arc -- the circumcircle
      // through them gives center+radius, then the start/end angles.
      const circumcenter = circleThrough(s.start, s.mid, s.end);
      if (!circumcenter) {
        ctx.moveTo(s.start[0], s.start[1]);
        ctx.lineTo(s.end[0], s.end[1]); // degenerate (collinear) points: a straight line is a reasonable fallback, not a crash
        ctx.stroke();
        return;
      }
      const [cx, cy, r] = circumcenter;
      const a0 = Math.atan2(s.start[1] - cy, s.start[0] - cx);
      const aMid = Math.atan2(s.mid[1] - cy, s.mid[0] - cx);
      const a1 = Math.atan2(s.end[1] - cy, s.end[0] - cx);
      // Pick whichever sweep direction (CW vs CCW) actually passes through `mid`.
      const ccw = normalizeSweep(a0, aMid, a1);
      ctx.arc(cx, cy, r, a0, a1, ccw);
      ctx.stroke();
      return;
    }
  }
  if (s.filled) ctx.fill();
  ctx.stroke();
}

/** Circumcenter + radius of the circle through three points, or null if they're (nearly) collinear. Exported: viewer3d/scene.ts reuses this to sample the same true arc geometry for the 3D silk stand-in, instead of re-deriving it. */
export function circleThrough(a: [number, number], b: [number, number], c: [number, number]): [number, number, number] | null {
  const d = 2 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
  if (Math.abs(d) < 1e-9) return null;
  const a2 = a[0] * a[0] + a[1] * a[1];
  const b2 = b[0] * b[0] + b[1] * b[1];
  const c2 = c[0] * c[0] + c[1] * c[1];
  const ux = (a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d;
  const uy = (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d;
  return [ux, uy, Math.hypot(a[0] - ux, a[1] - uy)];
}

/** True (draw counter-clockwise) if sweeping CCW from `a0` reaches `aMid` before `a1` does -- i.e. whichever winding direction actually visits the arc's own recorded midpoint. Exported for viewer3d/scene.ts, see circleThrough above. */
export function normalizeSweep(a0: number, aMid: number, a1: number): boolean {
  const twoPi = Math.PI * 2;
  const fwd = (x: number) => ((x % twoPi) + twoPi) % twoPi; // 0..2pi, CCW-positive
  const ccwSpan = fwd(a1 - a0); // CCW distance a0 -> a1
  const ccwMidSpan = fwd(aMid - a0); // CCW distance a0 -> aMid
  return ccwMidSpan <= ccwSpan; // mid falls within the CCW sweep from a0 to a1
}

/** Free-standing board text (Place > Text), KiCad's real Newstroke font (strokeFont.ts). */
function drawTexts(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions) {
  const texts = board.drawings?.texts ?? [];
  for (const t of texts) {
    if (opts.layerVisible[t.layer] === false) continue;
    const selected = opts.selection.has(t.id);
    const color = selected ? layerColor("selection") : layerColor(realLayerKey(t.layer));
    // millideg -> rad; canvas Y grows downward, so negate for KiCad's CCW-positive convention (matches painter's other rotations)
    const angleRad = (-t.angle / 1000) * (Math.PI / 180);
    drawStrokeText(ctx, t.content, t.x, t.y, {
      sizeUm: Math.max(t.size, hairlineUm(view, 8)),
      thicknessUm: t.stroke_width,
      justify: t.justify,
      angleRad,
      mirror: t.mirror,
      color,
    });
  }
}

function drawRatsnest(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, curved: boolean) {
  const byNet = padPointsByNet(board.parts);
  ctx.strokeStyle = layerColor("ratsnest");
  ctx.lineWidth = hairlineUm(view, 1);
  for (const points of byNet.values()) {
    for (const [a, b] of minimumSpanningTree(points)) {
      ctx.beginPath();
      ctx.moveTo(a[0], a[1]);
      if (curved) {
        // pcbnew.Control.ratsnestLineMode ("Curved Ratsnest Lines"): a
        // gentle bow instead of a straight line, bulging perpendicular
        // to the line by a fraction of its length -- KiCad's own curve
        // is a proper spline; this is a single quadratic arc, visually
        // the same "it's a curve, not a wire" cue at this zoom level.
        const mx = (a[0] + b[0]) / 2,
          my = (a[1] + b[1]) / 2;
        const dx = b[0] - a[0],
          dy = b[1] - a[1];
        const bulge = 0.06;
        ctx.quadraticCurveTo(mx - dy * bulge, my + dx * bulge, b[0], b[1]);
      } else {
        ctx.lineTo(b[0], b[1]);
      }
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
 * The route/via/zone/drawing tool currently in progress (Canvas.tsx's
 * state.drawState), rendered as a dashed "not committed yet" preview:
 * the fixed points so far plus a rubber-band to wherever the next click
 * would actually land -- the exact same posture45+grid-snap Canvas.tsx's
 * own click handler applies, so the preview never lies about where a
 * click will go.
 */
function drawInProgress(ctx: CanvasRenderingContext2D, view: ViewTransform, opts: PaintOptions) {
  const draw = opts.drawState;
  if (!draw) return;
  const cursor = opts.cursorUm;
  const pts = draw.pts.slice();
  let rubberEnd: [number, number] | null = null;
  if (cursor) {
    const last = pts[pts.length - 1]!;
    const usePosture = draw.kind === "route" || (draw.kind === "shape" && (draw.shapeKind === "segment" || draw.shapeKind === "rect"));
    const raw: [number, number] = usePosture ? posture45(last, [cursor.x, cursor.y]) : [cursor.x, cursor.y];
    rubberEnd = snapPoint(raw[0], raw[1], opts.gridUm);
  }

  const color = draw.kind === "route" ? layerColor(copperColorKey(draw.layer)) : layerColor("selection");
  ctx.save();
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = draw.kind === "route" ? Math.max(draw.width, hairlineUm(view, 1)) : hairlineUm(view, 1.5);
  ctx.setLineDash([hairlineUm(view, 4), hairlineUm(view, 3)]);
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.beginPath();
  pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
  if (rubberEnd) ctx.lineTo(rubberEnd[0], rubberEnd[1]);
  if (draw.kind === "zone" && pts.length >= 2) ctx.closePath(); // preview the closing edge back to the start
  ctx.stroke();
  ctx.setLineDash([]);

  // A small dot on every fixed point so far, so the operator can see
  // exactly where each click landed (especially useful once several
  // route segments or zone corners are down).
  for (const [x, y] of pts) {
    ctx.beginPath();
    ctx.arc(x, y, hairlineUm(view, 2.5), 0, Math.PI * 2);
    ctx.fill();
  }
  ctx.restore();
}

/** A ghost circle at the snapped cursor for the standalone via tool -- via.ts's placement is a single click, so there's no multi-point drawState to show, just "a via would land here". */
function drawViaGhost(ctx: CanvasRenderingContext2D, board: BoardState, opts: PaintOptions) {
  if (!opts.cursorUm) return;
  const [x, y] = snapPoint(opts.cursorUm.x, opts.cursorUm.y, opts.gridUm);
  const d = board.board_rules?.via_diameter ?? 600;
  ctx.save();
  ctx.globalAlpha = 0.6;
  ctx.fillStyle = layerColor("via");
  ctx.beginPath();
  ctx.arc(x, y, d / 2, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();
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
    b_cu: () => {
      drawZones(ctx, view, board, opts, "b_cu");
      drawTracksAndVias(ctx, view, board, opts, "b_cu");
    },
    in2_cu: () => {
      drawZones(ctx, view, board, opts, "inner");
      drawTracksAndVias(ctx, view, board, opts, "inner");
    },
    in1_cu: () => {},
    f_cu: () => {
      drawZones(ctx, view, board, opts, "f_cu");
      drawTracksAndVias(ctx, view, board, opts, "f_cu");
    },
    ratsnest: () => opts.showRatsnest && !board.routing && drawRatsnest(ctx, view, board, opts.ratsnestCurved),
  };
  for (const key of drawOrder()) byLayer[key]?.();
  // Footprints (courtyard/pads/silk together, so a part's own layers stay coherent) after copper, before selection/cursor.
  for (const part of board.parts) drawFootprint(ctx, view, part, opts);
  // Free-standing graphics/text (Place > Line/Arc/.../Text) -- same visual tier as silkscreen, after copper and footprints, before the in-progress tool preview.
  drawShapes(ctx, view, board, opts);
  drawTexts(ctx, view, board, opts);
  // In-progress route/via/zone/drawing tool preview, on top of everything committed.
  drawInProgress(ctx, view, opts);
  if (opts.activeTool === "via") drawViaGhost(ctx, board, opts);
}
