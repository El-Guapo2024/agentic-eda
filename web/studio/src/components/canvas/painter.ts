// The painter: KiCad's pcb_painter.cpp equivalent. Given a 2D context
// already set up with the view transform (see Canvas.tsx), draws every
// board item this app's model has, in GAL layer order (layers.ts).
// Coordinates throughout are board µm -- the caller's ctx.scale/translate
// does the screen mapping, so this file never touches pixels directly
// except for hairline compensation (view.ts `hairlineUm`) and text size.

import type { BoardState, Dimension, DrcViolation, FillReport, Part, Pad, RatsnestEdge, Shape, Um, Zone } from "../../api/types";
import type { DrawState, ToolId, ViewTransform } from "../../state/store";
import { boundsOfPoints, hairlineUm } from "./view";
import { layerColor, copperColorKey, drawOrder } from "./layers";
import { constrainByAngleMode } from "./routing";
import type { AngleSnapMode } from "../../kicad-port/pcbParityState";
import { snapPoint } from "./gridHelper";
import { drawStrokeText } from "../text/strokeFont";
import { computeVisibleGridSize, isMajorGridLine, DEFAULT_GRID_STYLE, MAJOR_GRID_LINE_WIDTH_RATIO } from "../../kicad-port/grid";
import { originMarkerColor } from "../../kicad-port/gridOrigin";
import { netHighlightColor, hexToRgb, rgbToHex } from "../../kicad-port/netHighlight";
import { offsetRatsnestForPreview } from "../../kicad-port/localRatsnest";
import { formatLength, type LengthUnit } from "../../state/units";
import { bezierPolyline } from "../../kicad-port/bezierPoly";
import { drawArcPreview } from "./arcPreview";
import { drawBezierPreview } from "./bezierPreview";
import { BEZIER_MAX_ERROR_UM } from "./itemHitTest";
import { isHighlighted } from "../../kicad-port/boardControl";
import { triangulate, type Triangle } from "../../kicad-port/polyTriangulate";
import { shapeEditPoints } from "../../kicad-port/pcbPointEdit";

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
function withNetHighlight(color: string, net: string | null | undefined, highlight: string | readonly string[] | null): string {
  if (!highlight || !net) return color;
  return rgbToHex(netHighlightColor(hexToRgb(color), isHighlighted(net, highlight)));
}

export interface PaintOptions {
  selection: Set<string>;
  hot: Set<string>;
  /** The highlighted net, or every net of a multi-net highlight (`highlightNetSelection`); null = none. */
  netHighlight: string | readonly string[] | null;
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
  movePreview: { refs: string[]; dxUm: number; dyUm: number; rotateQuarterTurns?: number; flipped?: boolean; perRefOffsetUm?: Record<string, [number, number]> } | null;
  /** The route/zone/drawing tool currently in progress (Canvas.tsx), and the cursor to rubber-band its next point toward -- null cursor (pointer left the canvas, or hasn't moved yet) just skips the rubber-band, still showing the fixed points so far. */
  drawState: DrawState | null;
  cursorUm: { x: number; y: number } | null;
  activeTool: ToolId;
  /** `pcbnew.EditorControl.lineModeNext`'s current mode -- constrains the segment/rect tools' rubber-band, same as their click does (Canvas.tsx). Absent = the pre-existing 45-degree behavior. */
  angleSnapMode?: AngleSnapMode;
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
  /** GET /api/drc's violations (kicad-cli's report, see DrcDialog.tsx) -- null until DRC has run at least once this session (nothing drawn until then); kept showing after the dialog closes, like real KiCad's markers persisting until the next DRC run. */
  drcViolations: DrcViolation[] | null;
  /** The board has moved on since `drcViolations` were computed (kicad-port/checkRevision.ts): the markers are still drawn, dimmed and dashed, until the next DRC run. */
  drcStale?: boolean;
  /** GET /api/lint's PCB findings (crates/lint: our own checks, the ones KiCad does not have) -- only while the DRC dialog is open, since they follow the board live there and nowhere else. Drawn as blue diamonds so they never read as KiCad's circles. */
  lintViolations?: DrcViolation[] | null;
  /** Index into `lintViolations` the dialog's Lint tab has clicked. */
  lintSelected?: number | null;
  /** B/Ctrl+B's last GET /api/fill (zone_filler_tool.cpp) -- null (or a zone simply missing from it) means "no fill computed yet", which always paints as an outline regardless of `zoneDisplayMode`. See state.zoneFill's own doc. */
  zoneFill: FillReport | null;
  /** ZONE_DISPLAY_MODE: how a zone WITH fill data paints. A zone with no fill data yet ignores this and always shows its outline. `fractured` / `triangulated` draw the fill with its fractured ring's edges / the triangles it is cut into. */
  zoneDisplayMode: "filled" | "outline" | "fractured" | "triangulated";
  /** `m_DisplayGraphicsFill` off (`pcbnew.Control.graphicOutlines`): filled graphics drawn as outlines. */
  sketchGraphics?: boolean;
  /** `m_DisplayTextFill` off (`pcbnew.Control.textOutlines`, "Show footprint texts in line mode"): text drawn as thin lines. */
  sketchText?: boolean;
  /** `m_DisplayPadNum` (`pcbnew.Control.showPadNumbers`). */
  showPadNumbers?: boolean;
  /** The drill/place file origin (`pcbnew.EditorControl.drillOrigin`), drawn as KiCad's red circle-and-cross when it is not at (0, 0). */
  auxOrigin?: [number, number] | null;
  /** The point the grid is anchored at (`common.Control.gridSetOrigin`): the grid dots follow it and its marker is drawn when it is not at (0, 0). */
  gridOrigin?: [number, number] | null;
  /** pcbnew.EditorControl.viaSizeInc/Dec's current pick (useActionRunner.ts), for the via tool's ghost -- null until the hotkey's first press, same board-default fallback `Canvas.tsx`'s own via-placement click uses. */
  currentViaPreset: { diameter: number; drill: number } | null;
  /** common.Interactive.measureTool's ruler label, same unit the status bar shows. */
  units: LengthUnit;
  /** pcb_point_editor.cpp's zone corner-drag preview (Canvas.tsx's own local state, only set while a drag is live) -- lets drawZoneHandles show the corner actually moving, not the last-committed outline, while the drag is in progress. */
  zoneCornerPreview: { zoneId: string; outline: [Um, Um][] } | null;
  /** The same for a graphic shape's handle drag (kicad-port/pcbPointEdit.ts): the shape as it would be once dropped, drawn instead of the committed one. */
  shapePointPreview: { id: string; shape: Shape } | null;
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

/** Only once the pad is legible on screen, like the net name (a few pixels of digits are not worth drawing). */
function padNumberLegible(pad: Pad, view: ViewTransform): boolean {
  return Math.min(pad.w, pad.h) * view.scale > 8;
}

/**
 * `PCB_PAINTER::draw( const PAD*, LAYER_PAD_NETNAMES )`'s text geometry: the pad's box is limited to 1.1x its smaller side (a 45-degree
 * pad does not bloat it), the text is turned a quarter when the pad is taller than wide, the size is the box's height (at most
 * `MAX_FONT_SIZE`, 10 mm) and, when the pad shows a net name as well, the two lines share it (`size / 2.5`, the net name
 * `size / 1.4` below the middle, the number `size / 1.7` above).
 */
export function padTextLayout(pad: { w: number; h: number }, withNet: boolean): { size: number; width: number; rotated: boolean; netOffset: number; numberOffset: number } {
  let [w, h] = [pad.w, pad.h];
  const limit = Math.min(pad.w, pad.h) * 1.1;
  if (w > limit && h > limit) [w, h] = [limit, limit];
  let size = h;
  let rotated = false;
  if (w < h * 0.95) {
    rotated = true;
    size = w;
    [w, h] = [h, w];
  }
  size = Math.min(size, 10_000);
  let [netOffset, numberOffset] = [0, 0];
  if (withNet) {
    size = size / 2.5;
    netOffset = size / 1.4;
    numberOffset = size / 1.7;
  }
  return { size, width: w, rotated, netOffset, numberOffset };
}

/** The pad's number, centred (above the middle when a net name shares the pad), bold, as `PCB_PAINTER::draw( const PAD* )` writes it. */
function drawPadNumber(ctx: CanvasRenderingContext2D, pad: Pad, withNet: boolean, color: string) {
  const layout = padTextLayout(pad, withNet);
  // "We use a size for at least 3 chars ... a smaller text size to handle interline, pen size"; the stroked font is 0.9 as wide.
  let tsize = Math.min((1.5 * layout.width) / Math.max([...pad.num].length, 3), layout.size);
  tsize = Math.min(tsize * 0.85, layout.size);
  ctx.save();
  ctx.translate(pad.x, pad.y);
  if (layout.rotated) ctx.rotate(-Math.PI / 2);
  // (x, y) is the baseline: the cap height is about 0.7 of the size, so half of it puts the digits' middle where KiCad centres them.
  drawStrokeText(ctx, pad.num, 0, -layout.numberOffset + tsize * 0.35, { sizeUm: tsize, justify: "center", thicknessUm: (tsize * 0.9) / 6, color });
  ctx.restore();
}

function drawFootprint(ctx: CanvasRenderingContext2D, view: ViewTransform, part: Part, opts: PaintOptions) {
  if (!part.placed || !part.courtyard) return;
  const [x0, y0, x1, y1] = part.courtyard;
  const selected = opts.selection.has(part.ref);
  const isHot = opts.hot.has(part.ref);
  const preview = opts.movePreview?.refs.includes(part.ref) ? opts.movePreview : null;

  ctx.save();
  if (preview) {
    // `perRefOffsetUm`: Pack and Move's per-footprint SpreadFootprints shift (absent for every ordinary move).
    const own = preview.perRefOffsetUm?.[part.ref];
    ctx.translate(preview.dxUm + (own?.[0] ?? 0), preview.dyUm + (own?.[1] ?? 0));
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
    const numbered = opts.showPadNumbers && pad.num !== "" && padNumberLegible(pad, view);
    if (pad.net && pad.w * view.scale > 22 && pad.h * view.scale > 10) {
      ctx.fillStyle = "rgba(0,0,0,0.75)";
      const fontUm = Math.min(pad.w, pad.h) * 0.35;
      ctx.font = `${fontUm}px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      // With the number on too, the two share the pad: the net name drops below the middle (`Y_offset_netname`).
      ctx.fillText(pad.net, pad.x, numbered ? pad.y + padTextLayout(pad, true).netOffset : pad.y, pad.w * 0.9);
    }
    if (numbered) drawPadNumber(ctx, pad, !!pad.net && pad.w * view.scale > 22 && pad.h * view.scale > 10, layerColor("pad_netname"));
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
      drawStrokeText(ctx, part.ref, tx, ty, { sizeUm: fs, justify: align, thicknessUm: opts.sketchText ? hairlineUm(view, 1) : fs / 6, color: layerColor(silkKey) });
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
/** `ZONE::GetIsRuleArea()` keepouts (task item 3) never have fill data and
 * are never copper -- KiCad draws one as a hatched outline labelled with
 * which items it disallows, regardless of the current zone display mode
 * (solid/outline), since there is no "fill" concept for one to toggle.
 * This port's stand-in: a dashed outline in a dedicated color, a light
 * diagonal hatch, and an abbreviated restriction label at the centroid. */
const RULE_AREA_COLOR = "#ff8c00";
/** World-space hatch spacing, µm (0.8 mm) -- a fixed physical pitch, same
 * convention a real cross-hatch fill pattern uses, so it reads as a
 * consistent density at any zoom rather than a fixed screen-pixel count. */
const RULE_AREA_HATCH_PITCH_UM = 800;

function ruleAreaLabel(z: Zone): string {
  const parts: string[] = [];
  if (z.keepout_tracks) parts.push("Tracks");
  if (z.keepout_vias) parts.push("Vias");
  if (z.keepout_pads) parts.push("Pads");
  if (z.keepout_copper_pour) parts.push("Copper");
  if (z.keepout_footprints) parts.push("Fp");
  return parts.length > 0 ? `Keepout: ${parts.join("/")}` : "Rule Area";
}

function drawRuleArea(ctx: CanvasRenderingContext2D, view: ViewTransform, z: Zone, selected: boolean) {
  const color = selected ? layerColor("selection") : RULE_AREA_COLOR;

  ctx.save();
  ctx.beginPath();
  z.outline.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
  ctx.closePath();
  ctx.clip();
  // Diagonal (45-degree) hatch across the outline's bounding box, clipped
  // to the real outline above -- KiCad's own keepout rendering is a
  // proper cross-hatch fill pattern; this is a lighter stand-in with the
  // same intent (visually distinct from solid copper, at a glance).
  const xs = z.outline.map((p) => p[0]);
  const ys = z.outline.map((p) => p[1]);
  const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
  const span = y1 - y0;
  ctx.strokeStyle = color;
  ctx.globalAlpha = 0.35;
  ctx.lineWidth = hairlineUm(view, 1);
  for (let d = x0 - span; d < x1; d += RULE_AREA_HATCH_PITCH_UM) {
    ctx.beginPath();
    ctx.moveTo(d, y0);
    ctx.lineTo(d + span, y1);
    ctx.stroke();
  }
  ctx.restore();

  ctx.beginPath();
  z.outline.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
  ctx.closePath();
  ctx.strokeStyle = color;
  ctx.lineWidth = hairlineUm(view, selected ? 2.5 : 1.5);
  ctx.setLineDash([hairlineUm(view, 6), hairlineUm(view, 4)]);
  ctx.stroke();
  ctx.setLineDash([]);

  const cx = (x0 + x1) / 2;
  const cy = (y0 + y1) / 2;
  drawStrokeText(ctx, ruleAreaLabel(z), cx, cy, { sizeUm: Math.max(hairlineUm(view, 11), 300), justify: "center", color });
}

/** A fill's triangles, cached per fill (`CacheTriangulation`): from each island's outline and holes when the backend sent them (`?polys=1`), else from its fractured ring. */
const triangleCache = new WeakMap<object, Triangle[]>();
function fillTriangles(fill: { fragments: [number, number][][]; polys?: { outline: [number, number][]; holes: [number, number][][] }[] }): Triangle[] {
  const key: object = fill.polys ?? fill.fragments;
  let tris = triangleCache.get(key);
  if (!tris) {
    tris = (fill.polys ?? fill.fragments.map((outline) => ({ outline, holes: [] }))).flatMap((poly) => triangulate(poly));
    triangleCache.set(key, tris);
  }
  return tris;
}

function drawZones(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions, wantLayer: "f_cu" | "b_cu" | "inner") {
  if (!board.routing) return;
  for (const z of board.routing.zones) {
    const key = copperColorKey(z.layer);
    const bucket = key === "f_cu" ? "f_cu" : key === "b_cu" ? "b_cu" : "inner";
    if (bucket !== wantLayer) continue;
    if (opts.layerVisible[z.layer] === false) continue;
    if (z.outline.length < 3) continue;
    const selected = opts.selection.has(z.id);
    if (z.is_rule_area) {
      drawRuleArea(ctx, view, z, selected);
      continue;
    }
    if (z.teardrop) {
      // A teardrop's own outline already *is* its final filled shape
      // (task item 4's generator computes the exact pentagon, no
      // knockout/thermal-relief pipeline applies) -- draw it solid
      // unconditionally, regardless of `zoneDisplayMode`, same as its
      // anchor pad/via is never shown as an "outline only" shape either.
      withAlpha(ctx, layerAlpha(opts, z.layer), () => {
        ctx.beginPath();
        z.outline.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
        ctx.closePath();
        ctx.fillStyle = withNetHighlight(layerColor(key), z.net, opts.netHighlight);
        ctx.fill();
        if (selected) {
          ctx.strokeStyle = layerColor("selection");
          ctx.lineWidth = hairlineUm(view, 2.5);
          ctx.stroke();
        }
      });
      continue;
    }
    const copperColor = withNetHighlight(layerColor(key), z.net, opts.netHighlight);
    const fill = opts.zoneFill?.zones.find((f) => f.id === z.id);
    withAlpha(ctx, layerAlpha(opts, z.layer), () => {
      if (opts.zoneDisplayMode !== "outline" && fill && fill.fragments.length > 0) {
        if (opts.zoneDisplayMode === "filled") {
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
        } else {
          // SHOW_FRACTURE_BORDERS / SHOW_TRIANGULATION: `SetIsFill( false ); SetIsStroke( true ); SetLineWidth( 0 )` -- no fill, the
          // fractured ring's own edges, or the triangles the fill is cut into.
          ctx.strokeStyle = copperColor;
          ctx.lineWidth = hairlineUm(view, 1);
          ctx.beginPath();
          if (opts.zoneDisplayMode === "fractured") {
            for (const frag of fill.fragments) {
              if (frag.length < 3) continue;
              frag.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
              ctx.closePath();
            }
          } else {
            for (const [a, b, c] of fillTriangles(fill)) {
              ctx.moveTo(a[0], a[1]);
              ctx.lineTo(b[0], b[1]);
              ctx.lineTo(c[0], c[1]);
              ctx.closePath();
            }
          }
          ctx.stroke();
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
  // A handle drag in progress shows the shape as it would be dropped, in place of the committed one.
  const shapes = (board.drawings?.shapes ?? []).map((s) => (opts.shapePointPreview?.id === s.id ? opts.shapePointPreview.shape : s));
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
    drawShapeGeometry(ctx, s, !!opts.sketchGraphics);
    ctx.restore();
  }
}

/** `sketch` is `pcbnew.Control.graphicOutlines` ("Sketch Graphic Items", `m_DisplayGraphicsFill` off): a filled shape is drawn as its outline only. */
function drawShapeGeometry(ctx: CanvasRenderingContext2D, s: Shape, sketch = false) {
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
    case "bezier": {
      // `EDA_SHAPE::RebuildBezierToSegmentsPointsList( m_MaxError )`: the curve as KiCad's own flattened polyline (an open curve is never filled).
      bezierPolyline(s.start, s.c1, s.c2, s.end, BEZIER_MAX_ERROR_UM).forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.stroke();
      return;
    }
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
      // Pick whichever sweep direction actually passes through `mid`. `normalizeSweep` is true when the sweep of INCREASING
      // angle (clockwise on this y-down canvas) does, and `ctx.arc`'s last argument is `anticlockwise` -- so its negation.
      ctx.arc(cx, cy, r, a0, a1, !normalizeSweep(a0, aMid, a1));
      ctx.stroke();
      return;
    }
  }
  if (s.filled && !sketch) ctx.fill();
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

/** True if sweeping from `a0` in the direction of INCREASING angle reaches `aMid` before `a1` does -- i.e. whichever winding direction actually visits the arc's own recorded midpoint (on the y-down canvas increasing angle is clockwise, so a `ctx.arc` caller passes the negation as its `anticlockwise` flag). Exported for viewer3d/scene.ts, see circleThrough above. */
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
      // "Sketch Text Items" (`m_DisplayTextFill` off): the text in line mode, one pixel wide.
      thicknessUm: opts.sketchText ? hairlineUm(view, 1) : t.stroke_width,
      justify: t.justify,
      angleRad,
      mirror: t.mirror,
      color,
    });
  }
}

/**
 * Task item 7: every `lines` segment (extension lines, crossbar/leader
 * pieces, arrow barbs, a centre cross -- already fully computed server-
 * side, `eda_connectivity::dimension::compute_dimension_geometry`) plus
 * the formatted `text` at `text_at`/`computed_text_angle`. Unlike
 * `drawTexts`' own `-angle` negation, `computed_text_angle` is already in
 * this app's canvas-native clockwise-positive convention (the same one
 * `rotate_point_about` documents backend-side), so it's used directly,
 * with no sign flip.
 */
function drawDimensions(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions) {
  const dimensions = board.drawings?.dimensions ?? [];
  for (const d of dimensions as Dimension[]) {
    if (opts.layerVisible[d.layer] === false) continue;
    const selected = opts.selection.has(d.id);
    const color = selected ? layerColor("selection") : layerColor(realLayerKey(d.layer));
    ctx.save();
    ctx.strokeStyle = color;
    ctx.lineWidth = Math.max(d.stroke_width, hairlineUm(view, selected ? 2 : 1));
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.beginPath();
    for (const [[ax, ay], [bx, by]] of d.lines) {
      ctx.moveTo(ax, ay);
      ctx.lineTo(bx, by);
    }
    ctx.stroke();
    ctx.restore();

    if (d.text) {
      const angleRad = (d.computed_text_angle / 1000) * (Math.PI / 180);
      drawStrokeText(ctx, d.text, d.text_at[0], d.text_at[1], {
        sizeUm: Math.max(d.text_size_um, hairlineUm(view, 8)),
        thicknessUm: d.stroke_width,
        justify: "center",
        angleRad,
        color,
      });
    }
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
function drawRatsnest(ctx: CanvasRenderingContext2D, view: ViewTransform, edges: RatsnestEdge[], curved: boolean, netHighlight: string | readonly string[] | null) {
  const hair = hairlineUm(view, 1);
  for (const e of edges) {
    const on = isHighlighted(e.net, netHighlight);
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
function drawGrid(ctx: CanvasRenderingContext2D, view: ViewTransform, widthPx: number, heightPx: number, gridUm: number, origin: readonly [number, number] = [0, 0]) {
  const visible = computeVisibleGridSize(gridUm, view.scale, DEFAULT_GRID_STYLE);
  const stepPx = visible * view.scale;
  if (!(stepPx > 0)) return; // gridUm <= 0 or a degenerate scale -- nothing sane to draw
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

/**
 * The grid origin marker (`ORIGIN_VIEWITEM` of `PCB_CONTROL::Reset`): a 16 px circle with an X in the grid colour, darkened on a bright background and
 * brightened on a dark one, at the point the grid is anchored at -- not drawn while that is (0, 0).
 */
export function drawGridOrigin(ctx: CanvasRenderingContext2D, view: ViewTransform, at: readonly [number, number] | null | undefined, background: string): void {
  if (!at || (at[0] === 0 && at[1] === 0)) return;
  const r = hairlineUm(view, 16);
  ctx.save();
  ctx.strokeStyle = originMarkerColor(layerColor("grid"), background);
  ctx.lineWidth = hairlineUm(view, 1);
  ctx.beginPath();
  ctx.arc(at[0], at[1], r, 0, Math.PI * 2);
  ctx.moveTo(at[0] - r, at[1] - r);
  ctx.lineTo(at[0] + r, at[1] + r);
  ctx.moveTo(at[0] - r, at[1] + r);
  ctx.lineTo(at[0] + r, at[1] - r);
  ctx.stroke();
  ctx.restore();
}

function strokeDashedPolyline(ctx: CanvasRenderingContext2D, view: ViewTransform, pts: [number, number][], color: string, width: number, closed: boolean) {
  if (pts.length === 0) return;
  ctx.save();
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = Math.max(width, hairlineUm(view, 1));
  ctx.setLineDash([hairlineUm(view, 4), hairlineUm(view, 3)]);
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.beginPath();
  pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
  if (closed && pts.length >= 2) ctx.closePath();
  ctx.stroke();
  ctx.setLineDash([]);
  for (const [x, y] of pts) {
    ctx.beginPath();
    ctx.arc(x, y, hairlineUm(view, 2.5), 0, Math.PI * 2);
    ctx.fill();
  }
  ctx.restore();
}

/**
 * The route/via/zone/drawing tool currently in progress (Canvas.tsx's
 * state.drawState), rendered as a dashed "not committed yet" preview.
 *
 * The route tool's own preview (`draw.pts`) is the backend's own resolved
 * head (`eda_pns::Router`'s walkaround/shove/mark-obstacles already
 * applied, see kicad-port/routeTool.ts) -- no client-side rubber-band to
 * add on top of it, unlike the plain shape tools below, which still do
 * their own single-segment posture45+grid-snap preview locally. A
 * colliding head draws in KiCad's own violation red; `runs` (earlier legs
 * from before a via/layer switch), a pending `via`, and (shove mode)
 * `displaced` tracks each get their own pass so the whole live state is
 * visible, not just the current leg.
 */
/** A dashed ghost circle at `(x, y)` -- a via about to land somewhere (the
 * route tool's pending via) or one `Mode::Shove` would push there (a
 * displaced via, for either the route or drag tool's live preview). */
function strokeViaGhost(ctx: CanvasRenderingContext2D, view: ViewTransform, x: number, y: number, diameter: number, color: string) {
  ctx.save();
  ctx.strokeStyle = color;
  ctx.lineWidth = hairlineUm(view, 1.5);
  ctx.setLineDash([hairlineUm(view, 3), hairlineUm(view, 2)]);
  ctx.beginPath();
  ctx.arc(x, y, diameter / 2, 0, Math.PI * 2);
  ctx.stroke();
  ctx.setLineDash([]);
  ctx.restore();
}

function drawInProgress(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions) {
  const draw = opts.drawState;
  if (!draw) return;

  if (draw.kind === "route") {
    const violationColor = "#ff3333";
    for (const run of draw.runs ?? []) strokeDashedPolyline(ctx, view, run.pts, layerColor(copperColorKey(run.layer)), draw.width, false);
    strokeDashedPolyline(ctx, view, draw.pts, draw.colliding ? violationColor : layerColor(copperColorKey(draw.layer)), draw.width, false);
    for (const d of draw.displaced ?? []) strokeDashedPolyline(ctx, view, d.pts, "#ffaa33", draw.width, false);
    for (const v of draw.displacedVias ?? []) {
      const d = board.routing?.vias.find((via) => via.id === v.source_via)?.d ?? draw.width;
      strokeViaGhost(ctx, view, v.x, v.y, d, "#ffaa33");
    }
    if (draw.via) strokeViaGhost(ctx, view, draw.via.x, draw.via.y, draw.via.diameter, draw.colliding ? violationColor : layerColor("via"));
    return;
  }

  if (draw.kind === "drag") {
    const violationColor = "#ff3333";
    const color = draw.colliding ? violationColor : layerColor(copperColorKey(draw.layer));
    if (draw.dragKind === "via") {
      const [x, y] = draw.pts[0] ?? [0, 0];
      strokeViaGhost(ctx, view, x, y, draw.viaDiameter ?? 600, color);
      for (const leg of draw.fanout ?? []) strokeDashedPolyline(ctx, view, leg.pts, layerColor(copperColorKey(leg.layer)), leg.width, false);
    } else {
      strokeDashedPolyline(ctx, view, draw.pts, color, draw.width, false);
    }
    for (const d of draw.displaced ?? []) strokeDashedPolyline(ctx, view, d.pts, "#ffaa33", draw.width || 150, false);
    for (const v of draw.displacedVias ?? []) {
      const d = board.routing?.vias.find((via) => via.id === v.source_via)?.d ?? 600;
      strokeViaGhost(ctx, view, v.x, v.y, d, "#ffaa33");
    }
    return;
  }

  if (draw.kind === "diffpair") {
    // No shove/walkaround for a pair in this port (crates/pns/src/
    // diff_pair.rs's own doc comment) -- a collision on *either* line
    // colors *both*, since the pair is conceptually one unit to the user
    // even though each line is drawn/collision-checked independently.
    const violationColor = "#ff3333";
    const color = draw.colliding ? violationColor : layerColor(copperColorKey(draw.layer));
    for (const run of draw.runsA ?? []) strokeDashedPolyline(ctx, view, run.pts, layerColor(copperColorKey(run.layer)), draw.width, false);
    for (const run of draw.runsB ?? []) strokeDashedPolyline(ctx, view, run.pts, layerColor(copperColorKey(run.layer)), draw.width, false);
    strokeDashedPolyline(ctx, view, draw.ptsA, color, draw.width, false);
    strokeDashedPolyline(ctx, view, draw.ptsB, color, draw.width, false);
    return;
  }

  // `drawArc` / `drawOneBezier`: the construction managers' own geometry, not a polyline through the clicks.
  if (draw.kind === "shape" && draw.shapeKind === "arc" && draw.arc) {
    drawArcPreview(ctx, draw.arc, { color: layerColor("selection"), hair: (px) => hairlineUm(view, px), units: opts.units });
    return;
  }
  if (draw.kind === "shape" && draw.shapeKind === "bezier" && draw.bezier) {
    drawBezierPreview(ctx, draw.bezier, { color: layerColor("selection"), hair: (px) => hairlineUm(view, px), units: opts.units }, BEZIER_MAX_ERROR_UM);
    return;
  }

  // `S` (the schematic's sheet tool) and the schematic's shape tools draw their own rubber band in SchematicView.tsx -- never reaches the board painter.
  if (draw.kind === "sheet" || draw.kind === "sch_shape") return;

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
    const usePosture = draw.kind === "shape" && (draw.shapeKind === "segment" || draw.shapeKind === "rect");
    const raw: [number, number] = usePosture ? constrainByAngleMode(opts.angleSnapMode ?? "45", last, [cursor.x, cursor.y]) : [cursor.x, cursor.y];
    rubberEnd = snapPoint(raw[0], raw[1], opts.gridUm);
  }
  const color = layerColor("selection");
  const shown: [number, number][] = rubberEnd ? [...pts, rubberEnd] : pts;
  strokeDashedPolyline(ctx, view, shown, color, hairlineUm(view, 1.5), draw.kind === "zone" && shown.length >= 2);

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
 * kicad-cli never emits an empty `items[]`), color-coded by severity
 * (or LAYER_DRC_HIGHLIGHTED when it's the dialog's currently-focused
 * one), with a small "!" so a marker reads as "problem here" even before
 * the dialog's list gives it a description. This is a legible, KiCad-
 * colored stand-in for real KiCad's own MARKER_BASE icon shape (a
 * distinctive triangle-ish glyph drawn at a fixed screen size) --
 * drawing that exact polygon wasn't part of this pass's source research,
 * so a plain circle is the honest simplification here, not a guess at
 * the real shape.
 */
function drawDrcMarkers(ctx: CanvasRenderingContext2D, view: ViewTransform, violations: DrcViolation[], selected: number | null, stale = false) {
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
    // Out of date: the same marker, dimmed and dashed -- where the problem was, not necessarily where it is now.
    if (stale) ctx.setLineDash([r * 0.35, r * 0.25]);
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    ctx.globalAlpha = stale ? 0.1 : 0.35;
    ctx.fill();
    ctx.globalAlpha = stale ? 0.55 : 1;
    ctx.stroke();
    ctx.setLineDash([]);
    drawStrokeText(ctx, "!", x, y + r * 0.5, { sizeUm: r * 1.3, justify: "center", color, thicknessUm: r * 0.22 });
    ctx.restore();
  });
}

/** Our own lint findings (crates/lint), drawn apart from kicad-cli's DRC circles: a blue diamond around the finding's first item, bigger and filled when the dialog's Lint tab has it selected. */
const LINT_MARKER_COLOR = "#4ea1ff";

function drawLintMarkers(ctx: CanvasRenderingContext2D, view: ViewTransform, findings: DrcViolation[], selected: number | null) {
  const hair = hairlineUm(view, 1.5);
  findings.forEach((v, i) => {
    const item = v.items[0];
    if (!item) return;
    const [x, y] = item.pos;
    const on = i === selected;
    const r = on ? DRC_MARKER_RADIUS_UM * 1.5 : DRC_MARKER_RADIUS_UM * 1.1;
    ctx.save();
    ctx.strokeStyle = LINT_MARKER_COLOR;
    ctx.fillStyle = LINT_MARKER_COLOR;
    ctx.lineWidth = Math.max(100, hair);
    ctx.beginPath();
    ctx.moveTo(x, y - r);
    ctx.lineTo(x + r, y);
    ctx.lineTo(x, y + r);
    ctx.lineTo(x - r, y);
    ctx.closePath();
    ctx.globalAlpha = on ? 0.45 : 0.2;
    ctx.fill();
    ctx.globalAlpha = 1;
    ctx.stroke();
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
    grid: () => opts.gridVisible && drawGrid(ctx, view, widthPx, heightPx, opts.gridUm, opts.gridOrigin ?? [0, 0]),
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
  drawDimensions(ctx, view, board, opts);
  if (opts.auxOrigin) drawAuxOrigin(ctx, view, opts.auxOrigin);
  drawGridOrigin(ctx, view, opts.gridOrigin, layerColor("background"));
  // In-progress route/drag/via/zone/drawing tool preview, on top of everything committed.
  drawInProgress(ctx, view, board, opts);
  if (opts.activeTool === "via") drawViaGhost(ctx, board, opts);
  // pcb_point_editor.cpp: a single selected zone's outline corners are
  // draggable handles, shown only in the plain Select tool (same as
  // source only activating the point editor over the selection tool).
  if (opts.activeTool === "select") drawZoneHandles(ctx, view, board, opts);
  if (opts.activeTool === "select") drawShapeHandles(ctx, view, board, opts);
  // Task item 5: a selected group's own bounding box, same tier as the
  // zone corner handles above (a selection-mode indicator, not board
  // content).
  if (opts.activeTool === "select") drawSelectedGroups(ctx, view, board, opts);
  // DRC markers last of all -- an overlay above every board layer and
  // the in-progress tool preview, matching real KiCad.
  if (opts.drcViolations) drawDrcMarkers(ctx, view, opts.drcViolations, opts.drcSelected, opts.drcStale);
  if (opts.lintViolations) drawLintMarkers(ctx, view, opts.lintViolations, opts.lintSelected ?? null);
}

/**
 * `ORIGIN_VIEWITEM( KIGFX::COLOR4D( 0.8, 0.0, 0.0, 1.0 ), CIRCLE_CROSS )` at the drill/place file origin (`BOARD_EDITOR_CONTROL`'s `m_placeOrigin`):
 * a one-pixel red circle and cross, 16 pixels out, not drawn while the origin is at (0, 0).
 */
function drawAuxOrigin(ctx: CanvasRenderingContext2D, view: ViewTransform, at: [number, number]) {
  if (at[0] === 0 && at[1] === 0) return;
  const r = hairlineUm(view, 16);
  ctx.save();
  ctx.strokeStyle = "rgb(204, 0, 0)";
  ctx.lineWidth = hairlineUm(view, 1);
  ctx.beginPath();
  ctx.arc(at[0], at[1], r, 0, Math.PI * 2);
  ctx.moveTo(at[0] - r, at[1]);
  ctx.lineTo(at[0] + r, at[1]);
  ctx.moveTo(at[0], at[1] - r);
  ctx.lineTo(at[0], at[1] + r);
  ctx.stroke();
  ctx.restore();
}

/** The geometry points of a free-standing graphic, for bounding purposes only (not a faithful outline -- an arc's `mid` stands in for its sweep, a circle's `end` for its radius -- see `drawShapeGeometry` for the real rendering). */
function shapePointsOf(s: Shape): Array<[number, number]> {
  switch (s.kind) {
    case "segment":
    case "rect":
      return [s.start, s.end];
    case "arc":
      return [s.start, s.mid, s.end];
    case "circle":
      return [s.center, s.end];
    case "polygon":
      return s.pts;
    case "bezier":
      return bezierPolyline(s.start, s.c1, s.c2, s.end, BEZIER_MAX_ERROR_UM);
  }
}

/**
 * Task item 5: a selected group draws as a dashed box around the union of
 * its members' geometry -- this model's stand-in for `PCB_GROUP::
 * ViewBBox()` (also just the union of its members' own boxes upstream).
 * Member kinds this model doesn't have yet (schematic symbols/wires) are
 * simply never found by the lookups below and contribute nothing, rather
 * than erroring.
 */
function drawSelectedGroups(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions) {
  const groups = board.drawings?.groups ?? [];
  for (const g of groups) {
    if (!opts.selection.has(g.id)) continue;

    const pts: Array<[number, number]> = [];
    for (const id of g.member_ids) {
      const part = board.parts.find((p) => p.ref === id);
      if (part) {
        if (part.courtyard) pts.push([part.courtyard[0], part.courtyard[1]], [part.courtyard[2], part.courtyard[3]]);
        else if (part.at) pts.push(part.at);
        continue;
      }
      const track = board.routing?.tracks.find((t) => t.id === id);
      if (track) {
        pts.push(...track.pts);
        continue;
      }
      const via = board.routing?.vias.find((v) => v.id === id);
      if (via) {
        pts.push([via.x - via.d / 2, via.y - via.d / 2], [via.x + via.d / 2, via.y + via.d / 2]);
        continue;
      }
      const zone = board.routing?.zones.find((z) => z.id === id);
      if (zone) {
        pts.push(...zone.outline);
        continue;
      }
      const shape = board.drawings?.shapes.find((s) => s.id === id);
      if (shape) {
        pts.push(...shapePointsOf(shape));
        continue;
      }
      const text = board.drawings?.texts.find((t) => t.id === id);
      if (text) pts.push([text.x, text.y]);
    }

    const b = boundsOfPoints(pts);
    if (!b) continue;
    const pad = hairlineUm(view, 12);
    ctx.save();
    ctx.strokeStyle = layerColor("selection");
    ctx.lineWidth = hairlineUm(view, 2);
    ctx.setLineDash([hairlineUm(view, 8), hairlineUm(view, 5)]);
    ctx.strokeRect(b.minX - pad, b.minY - pad, b.maxX - b.minX + pad * 2, b.maxY - b.minY + pad * 2);
    ctx.setLineDash([]);
    ctx.restore();
  }
}

/**
 * pcb_point_editor.cpp's handles for a single selected graphic shape (kicad-port/pcbPointEdit.ts): a square at each point, a smaller
 * diamond at the middle of a polygon's or rectangle's edges; unlocked shapes only. Follows the drag preview while one is live.
 */
function drawShapeHandles(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions) {
  if (opts.selection.size !== 1) return;
  const id = [...opts.selection][0]!;
  if ((board.locked ?? []).includes(id)) return;
  const shape = opts.shapePointPreview?.id === id ? opts.shapePointPreview.shape : board.drawings?.shapes.find((s) => s.id === id);
  if (!shape) return;
  const r = hairlineUm(view, 4);
  ctx.save();
  ctx.fillStyle = layerColor("selection");
  ctx.strokeStyle = "rgba(0,0,0,0.6)";
  ctx.lineWidth = hairlineUm(view, 1);
  for (const p of shapeEditPoints(shape)) {
    const [x, y] = p.pos;
    ctx.beginPath();
    if (p.kind === "corner") {
      ctx.rect(x - r, y - r, r * 2, r * 2);
    } else {
      const d = r * 0.8;
      ctx.moveTo(x, y - d);
      ctx.lineTo(x + d, y);
      ctx.lineTo(x, y + d);
      ctx.lineTo(x - d, y);
      ctx.closePath();
    }
    ctx.fill();
    ctx.stroke();
  }
  ctx.restore();
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
