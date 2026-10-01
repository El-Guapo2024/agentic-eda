// The painter: KiCad's pcb_painter.cpp equivalent. Given a 2D context
// already set up with the view transform (see Canvas.tsx), draws every
// board item this app's model has, in GAL layer order (layers.ts).
// Coordinates throughout are board µm -- the caller's ctx.scale/translate
// does the screen mapping, so this file never touches pixels directly
// except for hairline compensation (view.ts `hairlineUm`) and text size.

import type { BoardState, DrcViolation, FillReport, Part, Pad, RatsnestEdge, Shape, Um } from "../../api/types";
import type { DrawState, ToolId, ViewTransform } from "../../state/store";
import { hairlineUm } from "./view";
import { layerColor, copperColorKey, drawOrder } from "./layers";
import { posture45 } from "./routing";
import { snapPoint } from "./gridHelper";
import { drawStrokeText } from "../text/strokeFont";
import { computeVisibleGridSize, isMajorGridLine, DEFAULT_GRID_STYLE, MAJOR_GRID_LINE_WIDTH_RATIO } from "../../kicad-port/grid";
import { netHighlightColor, hexToRgb, rgbToHex } from "../../kicad-port/netHighlight";
import { offsetRatsnestForPreview } from "../../kicad-port/localRatsnest";
import { formatLength, type LengthUnit } from "../../state/units";

/**
 * pcb_painter.cpp GetColor's net-highlight branch (see kicad-port/
 * netHighlight.ts's header comment for the exact source lines): a
 * copper/connected item on the highlighted net brightens, everything
 * else on a different net darkens, both by the same 0.5 factor. Returns
 * `color` unchanged when no net is highlighted at all, or for an item
 * with no net (`net` null/undefined -- a footprint's silkscreen/
 * courtyard/body, which source's own `conItem` cast would be null for
 * too, so this already matches: only genuinely connected items dim).
 */
function withNetHighlight(color: string, net: string | null | undefined, highlight: string | null): string {
  if (!highlight || !net) return color;
  return rgbToHex(netHighlightColor(hexToRgb(color), net === highlight));
}

export interface PaintOptions {
  selection: Set<string>;
  hot: Set<string>;
  netHighlight: string | null;
  showRatsnest: boolean;
  ratsnestCurved: boolean;
  /** GET /api/ratsnest's edges (crates/connectivity, KiCad's own ratsnest algorithm) -- null while the first fetch hasn't landed yet, in which case nothing is drawn (no client-side fallback computation anymore). */
  ratsnestEdges: RatsnestEdge[] | null;
  layerVisible: Record<string, boolean>;
  layerOpacity: Record<string, number>;
  activeLayer: string | null;
  highContrast: boolean;
  gridUm: number;
  gridVisible: boolean;
  movePreview: { refs: string[]; dxUm: number; dyUm: number; rotateQuarterTurns?: number; flipped?: boolean } | null;
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
  /** GET /api/drc's violations (crates/drc, see DrcDialog.tsx) -- null until the dialog has been opened at least once this session (nothing drawn until then); kept showing after it's closed, like real KiCad's markers persisting until the next DRC run. */
  drcViolations: DrcViolation[] | null;
  /** B/Ctrl+B's last GET /api/fill (zone_filler_tool.cpp) -- null (or a zone simply missing from it) means "no fill computed yet", which always paints as an outline regardless of `zoneDisplayMode`. See state.zoneFill's own doc. */
  zoneFill: FillReport | null;
  /** ZONE_DISPLAY_MODE: how a zone WITH fill data paints. A zone with no fill data yet ignores this and always shows its outline. */
  zoneDisplayMode: "filled" | "outline";
  /** pcbnew.EditorControl.viaSizeInc/Dec's current pick (useActionRunner.ts), for the via tool's ghost -- null until the hotkey's first press, same board-default fallback `Canvas.tsx`'s own via-placement click uses. */
  currentViaPreset: { diameter: number; drill: number } | null;
  /** common.Interactive.measureTool's ruler label, same unit the status bar shows. */
  units: LengthUnit;
  /** pcb_point_editor.cpp's zone corner-drag preview (Canvas.tsx's own local state, only set while a drag is live) -- lets drawZoneHandles show the corner actually moving, not the last-committed outline, while the drag is in progress. */
  zoneCornerPreview: { zoneId: string; outline: [Um, Um][] } | null;
  /** Index into `drcViolations` the dialog's list currently has clicked/focused, drawn with LAYER_DRC_HIGHLIGHTED instead of its own severity color -- null when the dialog hasn't focused one (every marker then just shows its own error/warning color). */
  drcSelected: number | null;
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
  if (preview) {
    ctx.translate(preview.dxUm, preview.dyUm);
    // edit_tool.cpp Rotate()/Flip() during an active Move spin the
    // dragged item in place around its own (already-moving) anchor --
    // see useActionRunner.ts's `tryTransformDuringMove` doc comment for
    // why that's an exact-not-approximate match for a single dragged
    // part. `ctx.rotate` is clockwise-positive in a Y-down canvas,
    // exactly like crates/model/src/footprint.rs's `to_board` (no axis
    // flip in either), so the preview spins the same direction the
    // committed `rotate` Cmd will once it lands.
    if (preview.rotateQuarterTurns || preview.flipped) {
      const [ax, ay] = part.at ?? [(x0 + x1) / 2, (y0 + y1) / 2];
      ctx.translate(ax, ay);
      if (preview.rotateQuarterTurns) ctx.rotate((preview.rotateQuarterTurns * 90 * Math.PI) / 180);
      if (preview.flipped) ctx.scale(-1, 1);
      ctx.translate(-ax, -ay);
    }
  }

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
    const fill = withNetHighlight(layerColor(padCopperKey), pad.net, opts.netHighlight);
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
      ctx.strokeStyle = withNetHighlight(layerColor(key), t.net, opts.netHighlight);
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
      const viaColor = withNetHighlight(layerColor("via"), v.net, opts.netHighlight);
      if (opts.sketchVias) {
        ctx.strokeStyle = viaColor;
        ctx.lineWidth = hairlineUm(view, 1.5);
        ctx.stroke();
      } else {
        ctx.fillStyle = viaColor;
        ctx.fill();
      }
    }
  }
}

/**
 * Zones: outline (KiCad's `ZONE_DISPLAY_MODE::SHOW_ZONE_OUTLINE`, and the
 * state every zone starts in before it's ever been filled) or solid
 * copper (`SHOW_FILLED`, the default once B has computed one) --
 * `pcbnew.ZoneFiller.zoneFillAll`/`zoneUnfillAll` and `pcbnew.Control.
 * zoneDisplayEnable`/`Disable` (see `useActionRunner.ts`) are this app's
 * own entry points for the two axes (has-fill-data × which-way-to-paint-
 * it). The other two real `ZONE_DISPLAY_MODE` values (fracture-borders,
 * triangulation) are developer debug views, not ported.
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
    const copperColor = withNetHighlight(layerColor(key), z.net, opts.netHighlight);
    const fill = opts.zoneFill?.zones.find((f) => f.id === z.id);
    withAlpha(ctx, layerAlpha(opts, z.layer), () => {
      if (opts.zoneDisplayMode === "filled" && fill && fill.fragments.length > 0) {
        // `fragments` are already "Fracture"d (pcb_painter.cpp paints the
        // real ZONE_FILLER output the same way): each is one closed ring,
        // holes slit into the outer boundary -- a plain nonzero-winding
        // fill per fragment is exactly right, no separate even-odd pass.
        ctx.fillStyle = copperColor;
        for (const frag of fill.fragments) {
          if (frag.length < 3) continue;
          ctx.beginPath();
          frag.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
          ctx.closePath();
          ctx.fill();
        }
        if (selected) {
          // Source's selection shadow is a separate highlight layer this
          // app doesn't model; stroking the zone's own (unfractured)
          // outline on top reads the same "this one's selected" cue
          // outline mode already uses, without recoloring the copper.
          ctx.beginPath();
          z.outline.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
          ctx.closePath();
          ctx.strokeStyle = layerColor("selection");
          ctx.lineWidth = hairlineUm(view, 2.5);
          ctx.stroke();
        }
        return;
      }
      ctx.beginPath();
      z.outline.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.closePath();
      ctx.strokeStyle = selected ? layerColor("selection") : copperColor;
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

/**
 * KiCad's own ratsnest (GET /api/ratsnest, crates/connectivity -- real
 * connectivity clustering + Delaunay/Kruskal MST, not this app's earlier
 * client-side per-net MST-over-pad-centers approximation). A net that's
 * fully routed simply has no edges here (the backend only ever reports
 * airwires between still-unconnected clusters), so unlike the old
 * approximation this needs no separate "hide once anything is routed"
 * guard -- each net's ratsnest disappears on its own the moment that net
 * is finished, exactly like real KiCad's.
 */
function drawRatsnest(ctx: CanvasRenderingContext2D, view: ViewTransform, edges: RatsnestEdge[], curved: boolean, netHighlight: string | null) {
  const hair = hairlineUm(view, 1);
  for (const e of edges) {
    const on = netHighlight === e.net;
    ctx.strokeStyle = on ? layerColor("LAYER_SELECTION_SHADOWS") : layerColor("ratsnest");
    ctx.lineWidth = on ? hairlineUm(view, 2.5) : hair;
    const [ax, ay] = e.from;
    const [bx, by] = e.to;
    ctx.beginPath();
    ctx.moveTo(ax, ay);
    if (curved) {
      // pcbnew.Control.ratsnestLineMode ("Curved Ratsnest Lines"): a
      // gentle bow instead of a straight line, bulging perpendicular
      // to the line by a fraction of its length -- KiCad's own curve
      // is a proper spline; this is a single quadratic arc, visually
      // the same "it's a curve, not a wire" cue at this zoom level.
      const mx = (ax + bx) / 2,
        my = (ay + by) / 2;
      const dx = bx - ax,
        dy = by - ay;
      const bulge = 0.06;
      ctx.quadraticCurveTo(mx - dy * bulge, my + dx * bulge, bx, by);
    } else {
      ctx.lineTo(bx, by);
    }
    ctx.stroke();
  }
}

/**
 * graphics_abstraction_layer.h GAL::GetVisibleGridSize() + opengl_gal.cpp
 * DrawGrid(), for the DOTS style (KiCad's own real default --
 * gal_display_options.cpp). Source draws this as two stencil-masked line
 * passes (a full-width horizontal line and a full-height vertical line
 * per grid step, each possibly "major" width, overlapped via the stencil
 * buffer so only their intersections show) -- Canvas2D has no stencil
 * buffer, and reproducing that two-pass trick for a cosmetic dot-size
 * nuance isn't worth an offscreen-canvas compositing detour. This instead
 * draws one dot per intersection directly, sized up only where BOTH axes
 * land on a tick line (opengl_gal.cpp's own SMALL_CROSS style, the one
 * other GRID_STYLE enumerator in source that draws per-intersection,
 * does exactly this `tickX && tickY` combination for ITS major marks) --
 * a close, simple approximation rather than a literal transcription of
 * the DOTS stencil mechanism. The coarsen-when-too-dense threshold
 * (computeVisibleGridSize) is the behaviorally important part and IS
 * ported exactly; see PARITY-pcb.md.
 */
function drawGrid(ctx: CanvasRenderingContext2D, view: ViewTransform, widthPx: number, heightPx: number, gridUm: number) {
  const visible = computeVisibleGridSize(gridUm, view.scale, DEFAULT_GRID_STYLE);
  const stepPx = visible * view.scale;
  if (!(stepPx > 0)) return; // gridUm <= 0 or a degenerate scale -- nothing sane to draw
  const x0 = -view.x / view.scale;
  const y0 = -view.y / view.scale;
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
    const x = i * visible;
    const tickX = isMajorGridLine(i);
    for (let j = firstIndexY; j <= lastIndexY; j++) {
      const y = j * visible;
      ctx.beginPath();
      ctx.arc(x, y, tickX && isMajorGridLine(j) ? majorR : minorR, 0, Math.PI * 2);
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
  // A finished measurement (2 points already fixed) is a static ruler --
  // it stops following the cursor, unlike every other click-to-add-points
  // tool here, which always rubber-bands toward wherever the *next* point
  // would land.
  const frozen = draw.kind === "measure" && pts.length >= 2;
  let rubberEnd: [number, number] | null = null;
  if (cursor && !frozen) {
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

  // pcb_viewer_tools.cpp's measure tool: the straight-line distance (and
  // dx/dy) between the ruler's two ends, labeled at the midpoint, in the
  // status bar's own unit. Live while still dragging the end (rubberEnd),
  // fixed once the measurement is complete.
  if (draw.kind === "measure") {
    const end = frozen ? pts[1]! : rubberEnd;
    if (end) {
      const [x0, y0] = pts[0]!;
      const [x1, y1] = end;
      const dist = Math.hypot(x1 - x0, y1 - y0);
      const label = `${formatLength(dist, opts.units)}  (dx ${formatLength(Math.abs(x1 - x0), opts.units)}, dy ${formatLength(Math.abs(y1 - y0), opts.units)})`;
      drawStrokeText(ctx, label, (x0 + x1) / 2, (y0 + y1) / 2 - hairlineUm(view, 8), { sizeUm: hairlineUm(view, 12), justify: "center", color, thicknessUm: hairlineUm(view, 1.4) });
    }
  }

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
  const d = opts.currentViaPreset?.diameter ?? board.board_rules?.via_diameter ?? 600;
  ctx.save();
  ctx.globalAlpha = 0.6;
  ctx.fillStyle = layerColor("via");
  ctx.beginPath();
  ctx.arc(x, y, d / 2, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();
}

/** Radius, um, of a DRC marker's circle -- not a KiCad constant (real KiCad's MARKER_BASE is a small fixed-pixel icon drawn in screen space, independent of zoom; this app has no screen-space-constant-size drawing primitive, so markers scale with the board like everything else here, sized to read clearly at a normal working zoom). */
const DRC_MARKER_RADIUS_UM = 300;

/**
 * DRC violation markers -- one circle per violation, centered on its
 * first item's position (every violation here has at least one item;
 * `crates/drc` never emits an empty `items[]`), color-coded by severity
 * (or LAYER_DRC_HIGHLIGHTED when it's the dialog's currently-focused
 * one), with a small "!" so a marker reads as "problem here" even before
 * the dialog's list gives it a description. This is a legible, KiCad-
 * colored stand-in for real KiCad's own MARKER_BASE icon shape (a
 * distinctive triangle-ish glyph drawn at a fixed screen size) --
 * drawing that exact polygon wasn't part of this pass's source research,
 * so a plain circle is the honest simplification here, not a guess at
 * the real shape.
 */
function drawDrcMarkers(ctx: CanvasRenderingContext2D, view: ViewTransform, violations: DrcViolation[], selected: number | null) {
  const hair = hairlineUm(view, 1.5);
  violations.forEach((v, i) => {
    const item = v.items[0];
    if (!item) return;
    const [x, y] = item.pos;
    const on = i === selected;
    const color = on ? layerColor("LAYER_DRC_HIGHLIGHTED") : layerColor(v.severity === "error" ? "LAYER_DRC_ERROR" : "LAYER_DRC_WARNING");
    const r = on ? DRC_MARKER_RADIUS_UM * 1.4 : DRC_MARKER_RADIUS_UM;
    ctx.save();
    ctx.fillStyle = color;
    ctx.strokeStyle = color;
    ctx.lineWidth = Math.max(100, hair);
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    ctx.globalAlpha = 0.35;
    ctx.fill();
    ctx.globalAlpha = 1;
    ctx.stroke();
    drawStrokeText(ctx, "!", x, y + r * 0.5, { sizeUm: r * 1.3, justify: "center", color, thicknessUm: r * 0.22 });
    ctx.restore();
  });
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
    // pcb_actions.cpp updateLocalRatsnest's non-router equivalent: redraw
    // the airwires live from a moving footprint's (previewed) position
    // rather than waiting for the move to commit and the backend's next
    // /api/ratsnest poll -- see kicad-port/localRatsnest.ts.
    ratsnest: () => opts.showRatsnest && opts.ratsnestEdges && drawRatsnest(ctx, view, offsetRatsnestForPreview(opts.ratsnestEdges, board, opts.movePreview), opts.ratsnestCurved, opts.netHighlight),
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
  // pcb_point_editor.cpp: a single selected zone's outline corners are
  // draggable handles, shown only in the plain Select tool (same as
  // source only activating the point editor over the selection tool).
  if (opts.activeTool === "select") drawZoneHandles(ctx, view, board, opts);
  // DRC markers last of all -- an overlay above every board layer and
  // the in-progress tool preview, matching real KiCad.
  if (opts.drcViolations) drawDrcMarkers(ctx, view, opts.drcViolations, opts.drcSelected);
}

/**
 * pcb_point_editor.cpp's corner handles for a single selected zone --
 * small squares at each outline vertex, filled solid (selection color) so
 * they read as grabbable. Shows the live drag preview's outline while one
 * of this zone's own corners is being dragged (`opts.zoneCornerPreview`),
 * the committed outline otherwise. Only ever one zone's handles at a time
 * (this app's point editor doesn't support editing several zones'
 * outlines in the same gesture, same as source's own one-item-at-a-time
 * point editor).
 */
function drawZoneHandles(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions) {
  if (opts.selection.size !== 1) return;
  const id = [...opts.selection][0]!;
  const zone = board.routing?.zones.find((z) => z.id === id);
  if (!zone) return;
  const outline = opts.zoneCornerPreview?.zoneId === id ? opts.zoneCornerPreview.outline : zone.outline;
  const r = hairlineUm(view, 4);
  ctx.save();
  ctx.fillStyle = layerColor("selection");
  ctx.strokeStyle = "rgba(0,0,0,0.6)";
  ctx.lineWidth = hairlineUm(view, 1);
  for (const [x, y] of outline) {
    ctx.beginPath();
    ctx.rect(x - r, y - r, r * 2, r * 2);
    ctx.fill();
    ctx.stroke();
  }
  ctx.restore();
}
