// The painter: KiCad's pcb_painter.cpp equivalent. Given a 2D context
// already set up with the view transform (see Canvas.tsx), draws every
// board item this app's model has, in GAL layer order (layers.ts).
// Coordinates throughout are board µm -- the caller's ctx.scale/translate
// does the screen mapping, so this file never touches pixels directly
// except for hairline compensation (view.ts `hairlineUm`) and text size.

import type { BoardState, Dimension, DrcViolation, FieldInfo, FillReport, Part, Pad, RatsnestEdge, Shape, Um, Zone } from "../../api/types";
import type { DrawState, ToolId, ViewTransform } from "../../state/store";
import { hairlineUm } from "./view";
import { layerColor, copperColorKey, drawOrder } from "./layers";
import { constrainByAngleMode } from "./routing";
import type { AngleSnapMode } from "../../kicad-port/pcbParityState";
import { snapPoint } from "./gridHelper";
import { drawStrokeText } from "../text/strokeFont";
import { computeVisibleGridSize, isMajorGridLine, DEFAULT_GRID_STYLE, MAJOR_GRID_LINE_WIDTH_RATIO } from "../../kicad-port/grid";
import { originMarkerColor } from "../../kicad-port/gridOrigin";
import { netHighlightColor, hexToRgb, rgbToHex } from "../../kicad-port/netHighlight";
import { carryRatsnest, offsetRatsnestForPreview } from "../../kicad-port/localRatsnest";
import { carryMatrix, carryPoint, splitCarried, type CarryPreview } from "../../kicad-port/pcbCarry";
import { fieldAsText, fieldShownByObjects } from "../../kicad-port/fpFields";
import { itemBounds, padIds } from "../../kicad-port/pcbItems";
import { ancestors } from "../../kicad-port/groupTree";
import type { LengthUnit } from "../../state/units";
import { measureLabel } from "../../kicad-port/measureRuler";
import { bezierPolyline } from "../../kicad-port/bezierPoly";
import { drawArcPreview } from "./arcPreview";
import { drawBezierPreview } from "./bezierPreview";
import { BEZIER_MAX_ERROR_UM } from "./itemHitTest";
import { isHighlighted } from "../../kicad-port/boardControl";
import { triangulate, type Triangle } from "../../kicad-port/polyTriangulate";
import { shapeEditPoints } from "../../kicad-port/pcbPointEdit";
import { layerIsVisible, layerStateKey, isCopper } from "../../kicad-port/layerPresets";
import { drcMarkerObject } from "../../kicad-port/appearance";
import { copperColor, drawAnchors, drawBoardArea, drawConflicts, drawLockedShadows, drawSheet, footprintShown, on, on as objectOnPaint, opacityOf, ratsnestColor, type PaintAppearance } from "./appearancePaint";

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

/**
 * The colour a copper item is drawn in: its net's (or its net class's) colour when the net colour mode is "All" and it has one, else the layer's `base`;
 * then the net highlight on top (`PCB_RENDER_SETTINGS::GetColor`).
 */
function copperPaint(base: string, net: string | null | undefined, opts: PaintOptions): string {
  const highlighted = opts.netHighlight && net ? isHighlighted(net, opts.netHighlight) : null;
  const own = copperColor(base, net, opts.appearance, highlighted);
  return own !== base ? own : withNetHighlight(base, net, opts.netHighlight);
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
  /**
   * The items in the hand. `kind: "pcb"` is any mix of PCB items (kicad-port/pcbCarry.ts): they are drawn through the one transform the drop would apply.
   * The older kinds carry footprints alone (`perRefOffsetUm` is Pack and Move's own shift for each).
   */
  movePreview: (CarryPreview & { kind?: string; perRefOffsetUm?: Record<string, [number, number]> }) | null;
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
  /** The group being worked in (`PCB_SELECTION_TOOL::m_enteredGroup`): drawn in a frame with its name, and everything outside it dimmed. */
  enteredGroup?: string | null;
  /**
   * What the Appearance panel adds to plain layer visibility (appearancePaint.ts): the opacities, net colours, the hidden inactive-layer mode, the sheet.
   * The visibility of the objects themselves (tracks, pads, DRC errors ...) is in `layerVisible` under `obj:<id>` keys (kicad-port/appearance.ts).
   */
  appearance?: PaintAppearance;
}

/** How much of its colour an item outside the entered group keeps. */
export const OUTSIDE_ENTERED_GROUP_ALPHA = 0.3;

function layerAlpha(opts: PaintOptions, key: string): number {
  // HIGH_CONTRAST_MODE::DIMMED mixes the inactive layers 80 % into the background; HIDDEN draws them not at all.
  if (opts.activeLayer && opts.highContrast && opts.activeLayer !== key) return opts.appearance?.contrastHidden ? 0 : 0.25;
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
    // A pad that says what shape it is (`pad.shape`) is drawn as that: a rectangle has square corners, a rounded rectangle the radius of its ratio
    // (KiCad's 25% when it has none), an oval a stadium. One that does not (an older backend) keeps the rounded box it was always drawn as.
    const shorter = Math.min(pad.w, pad.h);
    const r = pad.shape === "rect" ? 0 : pad.shape === "round_rect" ? shorter * (pad.ratio ?? 0.25) : pad.round ? shorter / 2 : shorter * 0.15;
    ctx.roundRect(pad.x - pad.w / 2, pad.y - pad.h / 2, pad.w, pad.h, r);
  }
}

/** A through-hole pad's hole: a circle of the drill, or the slot of an oblong one, at the pad's position (not the copper's centre, which an offset moves). Null when there is none to draw. */
function pathForHole(ctx: CanvasRenderingContext2D, pad: Pad): boolean {
  const cx = pad.px ?? pad.x;
  const cy = pad.py ?? pad.y;
  ctx.beginPath();
  if (pad.slot) {
    // the slot lies along the pad's longer side (KiCad draws an oblong hole along the axis it is longer in, turned with the pad)
    const [sw, sh] = pad.slot;
    const turned = pad.w < pad.h;
    const [w, h] = turned ? [sh, sw] : [sw, sh];
    ctx.roundRect(cx - w / 2, cy - h / 2, w, h, Math.min(w, h) / 2);
    return true;
  }
  if (pad.drill) {
    ctx.arc(cx, cy, pad.drill / 2, 0, Math.PI * 2);
    return true;
  }
  return false;
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
  // Footprints Front / Footprints Back: everything of a footprint on a hidden side -- courtyard, pads, text -- is not drawn.
  if (!footprintShown(opts.layerVisible, part)) return;
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
    // part. R turns counter-clockwise like KiCad's, and `ctx.rotate`
    // is clockwise-positive in a Y-down canvas, hence the sign.
    if (preview.rotateQuarterTurns || preview.flipped) {
      const [ax, ay] = part.at ?? [(x0 + x1) / 2, (y0 + y1) / 2];
      ctx.translate(ax, ay);
      if (preview.rotateQuarterTurns) ctx.rotate((-preview.rotateQuarterTurns * 90 * Math.PI) / 180);
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
  const padAlpha = opacityOf(opts.appearance, "pads");
  // A surface-mount pad is copper on its footprint's side; a through-hole one is on every copper layer, so it shows while any of them does (`PAD::ViewGetLOD`).
  const sideCopper = part.side === "bottom" ? "B.Cu" : "F.Cu";
  const anyCopper = Object.keys(opts.layerVisible).every((k) => !/\.Cu$/.test(k)) || Object.entries(opts.layerVisible).some(([k, v]) => /\.Cu$/.test(k) && v !== false);
  for (const pad of on(opts.layerVisible, "pads") ? (part.pads ?? []).filter((q) => (q.th ? anyCopper : opts.layerVisible[sideCopper] !== false)) : []) {
    ctx.save();
    ctx.globalAlpha *= padAlpha;
    const fill = copperPaint(layerColor(padCopperKey), pad.net, opts);
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
      // The hole punched through the copper (the board's background shows in it), with the wall around it, when the backend says how big it is.
      if (pathForHole(ctx, pad)) {
        ctx.fillStyle = layerColor("background");
        ctx.fill();
        ctx.strokeStyle = layerColor("pad_th");
        ctx.lineWidth = hairlineUm(view, 1);
        ctx.stroke();
      } else {
        pathForPad(ctx, pad);
        ctx.strokeStyle = layerColor("pad_th");
        ctx.lineWidth = hairlineUm(view, 1);
        ctx.stroke();
      }
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
    ctx.restore();
  }

  // A selected pad (pads are selectable on their own): the selection colour around it.
  if (part.pads?.length && !selected) {
    const ids = padIds(part);
    part.pads.forEach((pad, i) => {
      if (!opts.selection.has(ids[i]!)) return;
      pathForPad(ctx, pad);
      ctx.strokeStyle = layerColor("selection");
      ctx.lineWidth = hairlineUm(view, 2.5);
      ctx.stroke();
    });
  }

  // The Reference, the Value and the user fields are drawn by `drawFields` once the backend says where they are (`part.fields`); an older backend's footprint has
  // only its reference, drawn beside the courtyard the way it always was.
  if (!part.fields) {
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
    // The reference shows when its layer does and both Footprint Text and References are on (`PCB_TEXT::ViewGetLOD`).
    if (opts.layerVisible[silkKey] !== false && on(opts.layerVisible, "footprint_text") && on(opts.layerVisible, "footprint_references")) {
      withAlpha(ctx, layerAlpha(opts, silkKey), () => {
        drawStrokeText(ctx, part.ref, tx, ty, { sizeUm: fs, justify: align, thicknessUm: opts.sketchText ? hairlineUm(view, 1) : fs / 6, color: layerColor(silkKey) });
      });
    }

  }

  ctx.restore();
}

/** A field to draw, with the footprint it is on (none for a field carried on its own, which has no footprint to ask). */
export interface FieldToDraw {
  field: FieldInfo;
  part?: Pick<Part, "ref" | "side">;
}

/** The fields of the placed footprints, each with its footprint. */
export function fieldsToDraw(parts: readonly Part[]): FieldToDraw[] {
  return parts.flatMap((part) => (part.placed ? (part.fields ?? []).map((field) => ({ field, part })) : []));
}

/**
 * A footprint's fields (`PCB_FIELD`: Reference, Value, user fields) as the board has them -- position, size, thickness, layer, angle, justification, mirroring --
 * in KiCad's stroke font. A hidden field is not drawn (it is when the footprint is selected in KiCad, with "Force show fields when footprint selected" on; the
 * Properties panel and the dialog show it). `GetDrawRotation`'s keep-upright rule has already been applied in `fieldAsText`.
 *
 * The Appearance panel's Objects rows apply as `PCB_FIELD::ViewGetLOD` has them: the Reference needs References, the Value Values, every field Footprint Text, and the
 * footprint's side (Footprints Front or Back) must show -- unless the footprint is selected, which shows its fields whatever those rows say ("Force show fields when
 * footprint selected", on by default). The field's own layer must be on in any case.
 */
export function drawFields(ctx: CanvasRenderingContext2D, view: ViewTransform, items: readonly FieldToDraw[], opts: PaintOptions) {
  for (const { field: f, part } of items) {
    const selected = opts.selection.has(f.id);
    if (!f.visible && !selected) continue;
    const key = layerStateKey(f.layer);
    if (!f.text || !layerIsVisible(opts.layerVisible, f.layer)) continue;
    if (part && !fieldShownByObjects(opts.layerVisible, part, f, opts.selection.has(part.ref))) continue;
    const t = fieldAsText(f);
    const color = selected ? layerColor("selection") : layerColor(realLayerKey(f.layer));
    // millideg, KiCad's counter-clockwise; the canvas turns clockwise.
    const angleRad = (-t.angle / 1000) * (Math.PI / 180);
    // The anchor is where the text box is centred (or its top or bottom edge sits); the font draws from the baseline.
    const sizeUm = Math.max(f.h, hairlineUm(view, 8));
    const down = f.valign > 0 ? 0 : f.valign < 0 ? 0.72 * sizeUm : 0.36 * sizeUm;
    const x = f.x - down * Math.sin(angleRad);
    const y = f.y + down * Math.cos(angleRad);
    withAlpha(ctx, layerAlpha(opts, key) * (f.visible ? 1 : 0.5), () => {
      drawStrokeText(ctx, t.content, x, y, { sizeUm, thicknessUm: opts.sketchText ? hairlineUm(view, 1) : Math.max(f.thickness, sizeUm / 20), justify: t.justify, angleRad, mirror: f.mirror, italic: f.italic, color });
    });
  }
}

function drawTracksAndVias(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions, wantLayer: "f_cu" | "b_cu" | "inner") {
  if (!board.routing) return;
  const tracksOn = on(opts.layerVisible, "tracks");
  const trackAlpha = opacityOf(opts.appearance, "tracks");
  for (const t of tracksOn ? board.routing.tracks : []) {
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
    withAlpha(ctx, layerAlpha(opts, t.layer) * 0.92 * trackAlpha, () => {
      ctx.beginPath();
      t.pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.strokeStyle = copperPaint(layerColor(key), t.net, opts);
      ctx.lineWidth = opts.sketchTracks ? hairlineUm(view, 1.5) : Math.max(t.width, hairlineUm(view, 1));
      ctx.lineCap = "round";
      ctx.lineJoin = "round";
      ctx.stroke();
    });
  }
  if (wantLayer === "f_cu" && on(opts.layerVisible, "vias")) {
    const viaAlpha = opacityOf(opts.appearance, "vias");
    for (const v of board.routing.vias) {
      ctx.save();
      ctx.globalAlpha *= viaAlpha;
      ctx.beginPath();
      ctx.arc(v.x, v.y, v.d / 2, 0, Math.PI * 2);
      const viaColor = copperPaint(layerColor("via"), v.net, opts);
      if (opts.sketchVias) {
        ctx.strokeStyle = viaColor;
        ctx.lineWidth = hairlineUm(view, 1.5);
        ctx.stroke();
      } else {
        ctx.fillStyle = viaColor;
        ctx.fill();
      }
      ctx.restore();
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
  if (!board.routing || !on(opts.layerVisible, "zones")) return;
  const zoneAlpha = opacityOf(opts.appearance, "zones");
  const teardropAlpha = opacityOf(opts.appearance, "tracks"); // `IsTeardropArea()` takes the track opacity
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
      withAlpha(ctx, layerAlpha(opts, z.layer) * teardropAlpha, () => {
        ctx.beginPath();
        z.outline.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
        ctx.closePath();
        ctx.fillStyle = copperPaint(layerColor(key), z.net, opts);
        ctx.fill();
        if (selected) {
          ctx.strokeStyle = layerColor("selection");
          ctx.lineWidth = hairlineUm(view, 2.5);
          ctx.stroke();
        }
      });
      continue;
    }
    const zoneColor = copperPaint(layerColor(key), z.net, opts);
    const fill = opts.zoneFill?.zones.find((f) => f.id === z.id);
    withAlpha(ctx, layerAlpha(opts, z.layer) * zoneAlpha, () => {
      if (opts.zoneDisplayMode !== "outline" && fill && fill.fragments.length > 0) {
        if (opts.zoneDisplayMode === "filled") {
          // `fragments` are already "Fracture"d (pcb_painter.cpp paints the
          // real ZONE_FILLER output the same way): each is one closed ring,
          // holes slit into the outer boundary -- a plain nonzero-winding
          // fill per fragment is exactly right, no separate even-odd pass.
          ctx.fillStyle = zoneColor;
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
          ctx.strokeStyle = zoneColor;
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
      ctx.strokeStyle = selected ? layerColor("selection") : zoneColor;
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
    if (!layerIsVisible(opts.layerVisible, s.layer)) continue;
    const selected = opts.selection.has(s.id);
    ctx.save();
    // A filled shape takes the Filled Shapes opacity, any other graphic on copper the track opacity (`PCB_RENDER_SETTINGS::GetColor`).
    ctx.globalAlpha *= s.kind !== "segment" && s.kind !== "arc" && s.kind !== "bezier" && s.filled ? opacityOf(opts.appearance, "shapes") : isCopper(s.layer) ? opacityOf(opts.appearance, "tracks") : 1;
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
    if (!layerIsVisible(opts.layerVisible, t.layer)) continue;
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
    if (!layerIsVisible(opts.layerVisible, d.layer)) continue;
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
function drawRatsnest(ctx: CanvasRenderingContext2D, view: ViewTransform, edges: RatsnestEdge[], curved: boolean, netHighlight: string | readonly string[] | null, colorOf?: (net: string) => string | null) {
  const hair = hairlineUm(view, 1);
  for (const e of edges) {
    const lit = isHighlighted(e.net, netHighlight);
    // A net's own colour (or its net class's) replaces the ratsnest colour unless the net colour mode is "None" (`RATSNEST_VIEW_ITEM::ViewDraw`).
    ctx.strokeStyle = lit ? layerColor("LAYER_SELECTION_SHADOWS") : (colorOf?.(e.net) ?? layerColor("ratsnest"));
    ctx.lineWidth = lit ? hairlineUm(view, 2.5) : hair;
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
      const label = measureLabel([x0, y0], [x1, y1], opts.units);
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
function drawDrcMarkers(ctx: CanvasRenderingContext2D, view: ViewTransform, violations: DrcViolation[], selected: number | null, stale = false, layerVisible: Record<string, boolean> = {}) {
  const hair = hairlineUm(view, 1.5);
  violations.forEach((v, i) => {
    const item = v.items[0];
    if (!item) return;
    const [x, y] = item.pos;
    const on = i === selected;
    // A waived violation (`LAYER_DRC_EXCLUSION`): drawn in the exclusion colour and muted, as KiCad draws an excluded marker -- when DRC Exclusions is on.
    const waived = v.excluded === true;
    // `PCB_MARKER::ViewGetLayers`: a marker is on the layer of its severity (an exclusion on its own), which the Objects tab switches. The one the DRC dialog has selected stays.
    if (!on && !objectOnPaint(layerVisible, drcMarkerObject(v))) return;
    const color = on ? layerColor("LAYER_DRC_HIGHLIGHTED") : layerColor(waived ? "LAYER_DRC_EXCLUSION" : v.severity === "error" ? "LAYER_DRC_ERROR" : "LAYER_DRC_WARNING");
    const r = on ? DRC_MARKER_RADIUS_UM * 1.4 : DRC_MARKER_RADIUS_UM;
    ctx.save();
    ctx.fillStyle = color;
    ctx.strokeStyle = color;
    ctx.lineWidth = Math.max(100, hair);
    // Out of date: the same marker, dimmed and dashed -- where the problem was, not necessarily where it is now.
    if (stale) ctx.setLineDash([r * 0.35, r * 0.25]);
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    ctx.globalAlpha = stale ? 0.1 : waived && !on ? 0.18 : 0.35;
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
export function paintBoard(ctx: CanvasRenderingContext2D, view: ViewTransform, widthPx: number, heightPx: number, fullBoard: BoardState, opts: PaintOptions) {
  // A PCB selection in the hand (kicad-port/pcbCarry.ts): the rest of the board is painted as it is, the carried items afterwards through the carry transform.
  const preview = opts.movePreview?.kind === "pcb" ? opts.movePreview : null;
  const carry = preview ? splitCarried(fullBoard, preview.refs) : null;
  const board = carry ? carry.still : fullBoard;
  // The group being worked in: what is inside it is drawn as it is, everything else dimmed (`m_enteredGroupOverlay` puts the group in front; the dim is this
  // port's own cue that nothing outside it can be picked). The split puts the group's items in `moving`, the rest in `still`.
  const entered = opts.enteredGroup ? splitCarried(board, [opts.enteredGroup]) : null;
  const passes: { b: BoardState; alpha: number }[] = entered ? [{ b: entered.still, alpha: OUTSIDE_ENTERED_GROUP_ALPHA }, { b: entered.moving, alpha: 1 }] : [{ b: board, alpha: 1 }];
  const copper = (b: BoardState): Record<string, () => void> => ({
    b_cu: () => {
      drawZones(ctx, view, b, opts, "b_cu");
      drawTracksAndVias(ctx, view, b, opts, "b_cu");
    },
    in2_cu: () => {
      drawZones(ctx, view, b, opts, "inner");
      drawTracksAndVias(ctx, view, b, opts, "inner");
    },
    in1_cu: () => {},
    f_cu: () => {
      drawZones(ctx, view, b, opts, "f_cu");
      drawTracksAndVias(ctx, view, b, opts, "f_cu");
    },
  });
  const copperPasses = passes.map((pass) => ({ alpha: pass.alpha, layers: copper(pass.b) }));
  const byLayer: Record<string, () => void> = {
    grid: () => opts.gridVisible && drawGrid(ctx, view, widthPx, heightPx, opts.gridUm, opts.gridOrigin ?? [0, 0]),
    // The drawing sheet and the board area shadow lie under everything else, the outline on top of them.
    // When the Edge.Cuts shapes are the outline (arcs, circles, cutouts), they are drawn as the shapes they are, below; the polygon is only their summary.
    background: () => {
      drawSheet(ctx, view, opts);
      drawBoardArea(ctx, board, opts);
      // Edge.Cuts is a layer like the others (the Layers tab and the presets switch it); in high contrast it is neither dimmed away nor hidden, only pushed back
      // -- "Graphics on Edge_Cuts layer are not fully dimmed or hidden because they are useful when working on another layer" (`dim_factor_Edge_Cuts`, at least 0.3).
      if (layerIsVisible(opts.layerVisible, "Edge.Cuts")) withAlpha(ctx, opts.highContrast && opts.activeLayer && opts.activeLayer !== "board_edge" ? 0.3 : 1, () => drawOutline(ctx, view, board.outline_is_shapes ? null : board.outline));
    },
    ...Object.fromEntries(Object.keys(copper(board)).map((key) => [key, () => copperPasses.forEach((pass) => withAlpha(ctx, pass.alpha, () => pass.layers[key]?.()))])),
    // pcb_actions.cpp updateLocalRatsnest's non-router equivalent: redraw
    // the airwires live from a moving footprint's (previewed) position
    // rather than waiting for the move to commit and the backend's next
    // /api/ratsnest poll -- see kicad-port/localRatsnest.ts.
    ratsnest: () => {
      if (!opts.showRatsnest || !opts.ratsnestEdges) return;
      const edges = carry && preview ? carryRatsnest(opts.ratsnestEdges, fullBoard, carry, (pt) => carryPoint(preview, pt)) : offsetRatsnestForPreview(opts.ratsnestEdges, fullBoard, opts.movePreview);
      drawRatsnest(ctx, view, edges, opts.ratsnestCurved, opts.netHighlight, (net) => ratsnestColor(net, opts.appearance));
    },
  };
  for (const key of drawOrder()) byLayer[key]?.();
  for (const pass of passes) {
    withAlpha(ctx, pass.alpha, () => {
      // Footprints (courtyard/pads/silk together, so a part's own layers stay coherent) after copper, before selection/cursor.
      for (const part of pass.b.parts) drawFootprint(ctx, view, part, opts);
      // Their fields (Reference, Value, user fields: `PCB_FIELD`), each on its own layer.
      drawFields(ctx, view, fieldsToDraw(pass.b.parts), opts);
      // Free-standing graphics/text (Place > Line/Arc/.../Text) -- same visual tier as silkscreen, after copper and footprints, before the in-progress tool preview.
      drawShapes(ctx, view, pass.b, opts);
      drawTexts(ctx, view, pass.b, opts);
      drawDimensions(ctx, view, pass.b, opts);
    });
  }
  // The overlays of footprints: locked items' shadow, the courtyards a move puts in conflict, and the anchors.
  drawLockedShadows(ctx, board, opts);
  drawConflicts(ctx, fullBoard, opts.movePreview, opts);
  drawAnchors(ctx, view, board, opts);
  // The carried items, in the same order, through the transform the drop would apply.
  if (carry && preview) {
    const own = { ...opts, movePreview: null };
    const [a, b, c, d, e, f] = carryMatrix(preview);
    ctx.save();
    ctx.transform(a, b, c, d, e, f);
    const moving = copper(carry.moving);
    for (const key of drawOrder()) moving[key]?.();
    for (const part of carry.moving.parts) drawFootprint(ctx, view, part, own);
    drawFields(ctx, view, [...fieldsToDraw(carry.moving.parts), ...carry.fields.map((field) => ({ field }))], own);
    drawShapes(ctx, view, carry.moving, own);
    drawTexts(ctx, view, carry.moving, own);
    drawDimensions(ctx, view, carry.moving, own);
    drawAnchors(ctx, view, carry.moving, own);
    drawSelectedGroups(ctx, view, carry.moving, own);
    ctx.restore();
  }
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
  if (opts.drcViolations) drawDrcMarkers(ctx, view, opts.drcViolations, opts.drcSelected, opts.drcStale, opts.layerVisible);
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

/**
 * `PCB_PAINTER::draw( const PCB_GROUP* )` on `LAYER_ANCHOR`: a group draws an enclosing box when it is selected on its own (its parent group is not)
 * or entered, and then its name -- in a tab above the box, when it fits. The box is the union of its items' (`PCB_GROUP::ViewBBox`), the groups it
 * holds opened. A selected group is the dashed selection box this port has always drawn; the entered one is solid, in the anchor colour, with
 * everything outside it dimmed (see `paintBoard`).
 */
function drawSelectedGroups(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions) {
  const groups = board.drawings?.groups ?? [];
  for (const g of groups) {
    const entered = opts.enteredGroup === g.id;
    const selectedOnItsOwn = opts.selection.has(g.id) && !ancestors(groups, g.id).some((a) => opts.selection.has(a.id));
    if (!entered && !selectedOnItsOwn) continue;
    const box = itemBounds(board, g.id);
    if (!box) continue;
    const pad = hairlineUm(view, 12);
    const [x0, y0, w, h] = [box[0] - pad, box[1] - pad, box[2] - box[0] + pad * 2, box[3] - box[1] + pad * 2];
    ctx.save();
    ctx.strokeStyle = entered ? layerColor("anchor") : layerColor("selection");
    ctx.lineWidth = hairlineUm(view, entered ? 2.5 : 2);
    if (!entered) ctx.setLineDash([hairlineUm(view, 8), hairlineUm(view, 5)]);
    ctx.strokeRect(x0, y0, w, h);
    ctx.setLineDash([]);
    // The name, in KiCad's tab: two lines up and across, the text between them and the box ("Scale by zoom a bit, but not too much").
    const name = (g.name ?? "").trim();
    if (name !== "") {
      const textUm = (hairlineUm(view, 14) + 2 * 304.8) / 3;
      if (name.length * textUm * 0.6 < w) {
        const tab = textUm * 2;
        ctx.beginPath();
        ctx.moveTo(x0, y0);
        ctx.lineTo(x0, y0 - tab);
        ctx.lineTo(x0 + w, y0 - tab);
        ctx.lineTo(x0 + w, y0);
        ctx.stroke();
        ctx.fillStyle = ctx.strokeStyle;
        ctx.font = `italic ${textUm}px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`;
        ctx.textAlign = "center";
        ctx.textBaseline = "alphabetic";
        ctx.fillText(name, x0 + w / 2, y0 - textUm * 0.5);
      }
    }
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
