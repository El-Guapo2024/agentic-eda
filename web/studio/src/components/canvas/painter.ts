// The painter: KiCad's pcb_painter.cpp equivalent. Given a 2D context
// already set up with the view transform (see Canvas.tsx), draws every
// board item this app's model has, in GAL layer order (layers.ts).
// Coordinates throughout are board µm -- the caller's ctx.scale/translate
// does the screen mapping, so this file never touches pixels directly
// except for hairline compensation (view.ts `hairlineNm`) and text size.

import type { BoardState, Dimension, DrcViolation, FieldInfo, FillReport, Part, Pad, RatsnestEdge, Shape, Nm, Zone } from "../../api/types";
import type { DrawState, ToolId, ViewTransform } from "../../state/store";
import { hairlineNm } from "./view";
import { layerColor, copperColorKey } from "./layers";
import { shapeSecondPoint } from "../../kicad-port/pcbShapeSnap";
import type { AngleSnapMode } from "../../kicad-port/pcbParityState";
import { snapPoint } from "./gridHelper";
import { drawStrokeText, measureStrokeText } from "../text/strokeFont";
import { beginTextRecord, paintKicadText } from "../text/kicadTextPaint";
import { boardTextSpec, dimensionTextSpec, fieldSpec } from "../../kicad-port/textSpecs";
import { keptReadableInFlippedView } from "../../kicad-port/flipView";
import { rulerDrawing } from "../../kicad-port/rulerItem";
import { paintRuler } from "./rulerPaint";
import { computeVisibleGridSize, isMajorGridLine, DEFAULT_GRID_DISPLAY, DEFAULT_GRID_STYLE, MAJOR_GRID_LINE_WIDTH_RATIO, type GridDisplay } from "../../kicad-port/grid";
import { paintEditorGrid } from "./gridPaint";
import { originMarkerColor } from "../../kicad-port/gridOrigin";
import { leaderModeOf, zoneLeader, zoneLoop } from "../../kicad-port/pcbDrawZone";
import { netHighlightColor, hexToRgb, rgbToHex } from "../../kicad-port/netHighlight";
import { carryRatsnest, offsetRatsnestForPreview, ratsnestCross, type MovedEdge } from "../../kicad-port/localRatsnest";
import { edgeCutsDimFactor, hiContrastFactor, cssOf, hiContrastDim, highlightBrighten, highlightDarken, parseColor, ratsnestCurveControl, ratsnestLineColor } from "../../kicad-port/color4d";
import { padContrast, padHoleContrast, viaCopperContrast, viaHoleContrast, type ContrastLook } from "../../kicad-port/hiContrast";
import { galDrawOrder } from "../../kicad-port/galLayerOrder";
import { installColourScopes, memoColours, withColourScope, withFinalColourScope } from "./colourScope";
import { brightenedBoard, brightenedCss } from "../../kicad-port/brightened";
import { carryMatrix, carryPoint, splitCarried, type CarryPreview } from "../../kicad-port/pcbCarry";
import { fieldShownByObjects } from "../../kicad-port/fpFields";
import { itemBounds, padIds } from "../../kicad-port/pcbItems";
import { ancestors } from "../../kicad-port/groupTree";
import type { LengthUnit } from "../../state/units";
import { bezierPolyline } from "../../kicad-port/bezierPoly";
import { endingInks, hasEndings, kicadArc, shortenArc, shortenBezier, shortenSegment } from "../../kicad-port/lineEnding";
import { dimensionEditPoints } from "../../kicad-port/dimensionDraw";
import { DEFAULT_DASH_RATIOS, dashPieces, isDashStyle, strokeArc, strokeSegment, type DashPiece, type DashRatios, type DashSpec } from "../../kicad-port/strokeDash";
import { drawArcPreview } from "./arcPreview";
import { drawBezierPreview } from "./bezierPreview";
import { BEZIER_MAX_ERROR_NM } from "./itemHitTest";
import { isHighlighted } from "../../kicad-port/boardControl";
import { triangulate, type Triangle } from "../../kicad-port/polyTriangulate";
import { shapeEditPoints } from "../../kicad-port/pcbPointEdit";
import { layerIsVisible, layerNameOfKey, layerStateKey, isCopper } from "../../kicad-port/layerPresets";
import { zoneCopperLayers, zoneLayers } from "../../kicad-port/zoneLayers";
import { flatVertices, zoneContours } from "../../kicad-port/zonePointEditor";
import { cachedZoneHatchLines } from "../../kicad-port/zoneHatch";
import { drcMarkerObject } from "../../kicad-port/appearance";
import { drawFootprintArt } from "./fpArtPaint";
import { drawFootprintItems } from "./fpBoardItemsPaint";
import { isAnyFill } from "../../kicad-port/shapeFill";
import { drawPcbItems, drawPcbToolPreviews, drawPoints, drawRefImages } from "./pcbItemsPaint";
import { drawGridItems, gridItemCovers, shownGridItems } from "./gridItemPaint";
import { drawViaStitches } from "./viaStitchPaint";
import { viaMaxDiameter, viaWidthOn } from "../../kicad-port/viaPadstack";
import { drawDrillItems } from "./drillPaint";
import { gridGeometry, type GridGeometry } from "../../kicad-port/gridItem";
import { drawTablePreview, drawTables } from "./tablePaint";
import { drawSelectedPatterns, drawTune, drawTuneHover } from "./tunePaint";
import type { TuneHoverReply } from "../../api/tuneTypes";
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
  // every caller paints a connected item (`conItem`), and one with no net is on net 0 (`isHighlighted`'s `NO_NET`): dimmed unless that is highlighted (r7 C7-5)
  if (!highlight) return color;
  return rgbToHex(netHighlightColor(hexToRgb(color), isHighlighted(net, highlight)));
}

/**
 * The colour a copper item is drawn in: its net's (or its net class's) colour when the net colour mode is "All" and it has one, else the layer's `base`;
 * then the net highlight on top (`PCB_RENDER_SETTINGS::GetColor`).
 */
function copperPaint(base: string, net: string | null | undefined, opts: PaintOptions): string {
  const highlighted = opts.netHighlight ? isHighlighted(net, opts.netHighlight) : null;
  const own = copperColor(base, net, opts.appearance, highlighted);
  return own !== base ? own : withNetHighlight(base, net, opts.netHighlight);
}

export interface PaintOptions {
  /** Flip Board View is on (`m_FlipBoardView`): a text that is not side specific is drawn to read as it did (kicad-port/flipView.ts). */
  viewFlipped?: boolean;
  /** The board's layer types not their default (`layer_setup.types`): a user layer typed front or back is side specific, so its texts are mirrored in the flipped view. */
  layerTypes?: Readonly<Record<string, string>> | null;
  /** Which canvas `window.__edaText` files the texts under (components/text/kicadTextPaint.ts): "pcb" unless said (the Footprint Editor's "fp"). */
  textRecordTag?: string;
  /**
   * Import Graphics' placement preview (`PlaceImportedGraphics`' `m_view->Add( &preview )`, state/importPreview.ts): the shapes and texts the import would put on the board, as
   * `drawings` has them, moved by (dx, dy) to the snapped cursor. Null when no placement runs.
   */
  importPreview?: { shapes: unknown[]; texts: unknown[]; dx: number; dy: number } | null;
  selection: Set<string>;
  hot: Set<string>;
  /** The items the clarification menu's row under the pointer stands for, drawn brightened over the board (`doSelectionMenu`'s BRIGHTENED, kicad-port/brightened.ts). */
  brightened?: readonly string[];
  /** The highlighted net, or every net of a multi-net highlight (`highlightNetSelection`); null = none. */
  netHighlight: string | readonly string[] | null;
  showRatsnest: boolean;
  ratsnestCurved: boolean;
  /** `PCB_DISPLAY_OPTIONS::m_RatsnestThickness` (Preferences > PCB Editor > Editing Options, "Ratsnest line thickness:"), pixels; 0.5 when absent. */
  ratsnestThicknessPx?: number;
  /** `PCBNEW_SETTINGS::m_ShowCourtyardCollisions` ("Show courtyard collisions when moving/dragging"); on when absent. */
  showCourtyardCollisions?: boolean;
  /** GET /api/ratsnest's edges (crates/connectivity, KiCad's own ratsnest algorithm) -- null while the first fetch hasn't landed yet, in which case nothing is drawn (no client-side fallback computation anymore). */
  ratsnestEdges: RatsnestEdge[] | null;
  layerVisible: Record<string, boolean>;
  layerOpacity: Record<string, number>;
  activeLayer: string | null;
  highContrast: boolean;
  gridNm: number;
  gridVisible: boolean;
  /**
   * The items in the hand. `kind: "pcb"` is any mix of PCB items (kicad-port/pcbCarry.ts): they are drawn through the one transform the drop would apply.
   * The older kinds carry footprints alone (`perRefOffsetNm` is Pack and Move's own shift for each).
   */
  movePreview: (CarryPreview & { kind?: string; perRefOffsetNm?: Record<string, [number, number]> }) | null;
  /** The route/zone/drawing tool currently in progress (Canvas.tsx), and the cursor to rubber-band its next point toward -- null cursor (pointer left the canvas, or hasn't moved yet) just skips the rubber-band, still showing the fixed points so far. */
  drawState: DrawState | null;
  /** The length tuner armed, no pattern started: the track under the cursor with its status box (`updateHoverStatus`). */
  tuneHover?: { reply: TuneHoverReply; at: [number, number] } | null;
  cursorNm: { x: number; y: number } | null;
  /** Where the tool in force has put the cursor (the grid helper's last answer: anchor, snap line or grid); the rubber band and the via ghost follow it. Null: the raw cursor is snapped to the grid. */
  snappedCursor?: readonly [number, number] | null;
  activeTool: ToolId;
  /** `pcbnew.EditorControl.lineModeNext`'s current mode -- constrains the segment/rect tools' rubber-band, same as their click does (Canvas.tsx). Absent = the pre-existing 45-degree behavior. */
  angleSnapMode?: AngleSnapMode;
  /** Ctrl (Cmd on a Mac) held at the last pointer move: `drawShape`'s and `DrawZone`'s modifier for the preview. */
  drawCtrl?: boolean;
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
  /** The current grid's Y size when it is not square (Edit Grids' "Y:"). */
  gridYNm?: number;
  /** The board editor's Display Options > Grid Display: the style, thickness and minimum spacing the grid is drawn with. */
  gridDisplay?: GridDisplay;
  /** pcbnew.EditorControl.viaSizeInc/Dec's current pick (useActionRunner.ts), for the via tool's ghost -- null until the hotkey's first press, same board-default fallback `Canvas.tsx`'s own via-placement click uses. */
  currentViaPreset: { diameter: number; drill: number } | null;
  /** Draw Via's preview (`doInteractiveItemPlacement`: the item at the grid point, then `VIA_PLACER::SnapItem`, round 6 C6-7): where the via would land --
   * on the track, the pad or the graphic under the cursor -- as the click will place it (Canvas.tsx, kicad-port/viaPlacer.ts `snapViaPosition`). */
  viaGhostAt?: readonly [number, number] | null;
  /** common.Interactive.measureTool's ruler label, same unit the status bar shows. */
  units: LengthUnit;
  /** `RULER_ITEM( .., invertXAxis, invertYAxis )` of the Measure tool and Interactive Offset: Origins & Axes' axis directions (`m_DisplayInvertXAxis` /
   *  `m_DisplayInvertYAxis`), which flip the signs of the x and y the ruler shows (UI-LEFT-2 PP9). */
  rulerFlip?: { x: boolean; y: boolean };
  /** pcb_point_editor.cpp's zone corner-drag preview (Canvas.tsx's own local state, only set while a drag is live) -- lets drawZoneHandles show the corner actually moving, not the last-committed outline, while the drag is in progress. */
  zoneCornerPreview: { zoneId: string; outline: [Nm, Nm][]; /** The zone's holes as the drag has them (a hole's corner can be the one dragged). */ holes?: [Nm, Nm][][] } | null;
  /** The same for a graphic shape's handle drag (kicad-port/pcbPointEdit.ts): the shape as it would be once dropped, drawn instead of the committed one. */
  shapePointPreview: { id: string; shape: Shape } | null;
  /** The board's Dashed lines ratios (`boardDashRatios`) a dashed graphic is cut with; KiCad's 12 and 3 when absent. */
  dashRatios?: DashRatios;
  /** The dimension the Draw Dimension tool is drawing (`id` null) or the point editor is reshaping (`POST /api/dimension/preview`), drawn in place of the committed one. */
  dimensionPreview?: { id: string | null; dim: Dimension } | null;
  /** Index into `drcViolations` the dialog's list currently has clicked/focused, drawn with LAYER_DRC_HIGHLIGHTED instead of its own severity color -- null when the dialog hasn't focused one (every marker then just shows its own error/warning color). */
  drcSelected: number | null;
  /** The group being worked in (`PCB_SELECTION_TOOL::m_enteredGroup`): drawn in a frame with its name, and everything outside it dimmed. */
  enteredGroup?: string | null;
  /**
   * High Contrast Mode's Dim as the board editor draws it (set by `paintBoard`): an inactive item in its colour mixed into the background (`withLayer`). Without it
   * (the Footprint Editor's painters, which use these helpers too) an inactive layer is drawn at `layerAlpha`'s quarter alpha.
   */
  dimByColour?: boolean;
  /**
   * What the Appearance panel adds to plain layer visibility (appearancePaint.ts): the opacities, net colours, the hidden inactive-layer mode, the sheet.
   * The visibility of the objects themselves (tracks, pads, DRC errors ...) is in `layerVisible` under `obj:<id>` keys (kicad-port/appearance.ts).
   */
  appearance?: PaintAppearance;
  /**
   * The layer pass `paintBoard` is drawing (UI-LEFT-2 PP3: `PCB_DRAW_PANEL_GAL`'s layer order, kicad-port/galLayerOrder.ts): an item is drawn only in the pass of
   * its own layer and kind (`withLayer`). Absent: every item at once (the Footprint Editor's painters, a paint of one footprint).
   */
  galPass?: GalPass | null;
  /** The board's copper layers, front to back (`board.layers`): the layers a through-hole pad flashes on. */
  copperLayers?: readonly string[];
}

/**
 * One layer of the board editor's draw order as a pass of the painter: `kind` is which of KiCad's layers it is -- a board layer's items ("item"), its zones
 * (`ZONE_LAYER_FOR`, "zone"), points (`POINT_LAYER_FOR`, "point"), images (`BITMAP_LAYER_FOR`, "bitmap"), pads' and vias' copper (`PAD_COPPER_LAYER_FOR`,
 * `VIA_COPPER_LAYER_FOR`, "pad", "via"), or a GAL layer of its own ("gal": the holes, the net names, the overlays) -- `layer` the board layer (the GAL layer's
 * name for "gal"), and `key` that layer's state key (`layerStateKey`).
 */
export interface GalPass {
  kind: "item" | "zone" | "point" | "bitmap" | "pad" | "via" | "netnames" | "clearance" | "gal";
  layer: string;
  key: string;
}

/** The pass of one entry of `galDrawOrder`. */
export function galPassOf(gal: string): GalPass {
  const m = /^(zone|point|bitmap|pad|via|netnames|clearance):(.+)$/.exec(gal);
  if (m) return { kind: m[1] as GalPass["kind"], layer: m[2]!, key: layerStateKey(m[2]!) };
  if (gal.startsWith("LAYER_")) return { kind: "gal", layer: gal, key: gal };
  return { kind: "item", layer: gal, key: layerStateKey(gal) };
}

/** Is an item of `kind` on the layer of state key `key` drawn in the pass `opts` is in? Always, outside the layer passes. */
export function inPass(opts: Pick<PaintOptions, "galPass">, key: string, kind: GalPass["kind"] = "item"): boolean {
  const pass = opts.galPass;
  return !pass || (pass.kind === kind && pass.key === key);
}

/** How much of its colour an item outside the entered group keeps. */
export const OUTSIDE_ENTERED_GROUP_ALPHA = 0.3;

/**
 * The alpha of an item on layer `key`: its opacity, or High Contrast Mode's. HIGH_CONTRAST_MODE::DIMMED mixes the inactive layers 80 % into the background
 * (the board editor does that by colour: `withLayer`); here, for the Footprint Editor's painters, a quarter of the alpha stands for it. HIDDEN draws them not at all.
 */
export function layerAlpha(opts: PaintOptions, key: string): number {
  if (opts.activeLayer && opts.highContrast && opts.activeLayer !== key) return opts.appearance?.contrastHidden ? 0 : 0.25;
  return opts.layerOpacity[key] ?? 1;
}

export function withAlpha(ctx: CanvasRenderingContext2D, alpha: number, draw: () => void) {
  if (alpha >= 1) return draw();
  if (alpha <= 0) return;
  const prev = ctx.globalAlpha;
  ctx.globalAlpha = prev * alpha;
  draw();
  ctx.globalAlpha = prev;
}

/** Is an item on layer `key` (a state key, `layerStateKey`) off the active layer while High Contrast Mode is on (`m_hiContrastEnabled`, not `isActive`)? */
export function layerDimmed(opts: Pick<PaintOptions, "activeLayer" | "highContrast">, key: string): boolean {
  return !!opts.activeLayer && opts.highContrast && opts.activeLayer !== key;
}

/** The layer High Contrast Mode is on (`GetPrimaryHighContrastLayer()`) as a KiCad layer name, when the board editor dims by colour; null otherwise. */
function contrastPrimary(opts: PaintOptions): string | null {
  return opts.dimByColour && opts.highContrast && opts.activeLayer ? layerNameOfKey(opts.activeLayer) : null;
}

/**
 * High Contrast Mode's Dim on the board's background (`color.Mix( backgroundColor, factor )`, kicad-port/color4d.ts): the colours of the draw it is applied to.
 * The selection colour is left alone -- a selected item is never dimmed (`!highlighted && !selected`).
 */
function dimColours(factor = hiContrastFactor()): (css: string) => string {
  const background = layerColor("background");
  const selection = layerColor("selection");
  return memoColours(`dim|${factor}|${background}|${selection}`, (css) => (css === selection ? css : hiContrastDim(css, background, factor)));
}

/** Runs `draw` with the look High Contrast Mode gives an item (kicad-port/hiContrast.ts): as it is, dimmed, or not at all. */
function withLook(ctx: CanvasRenderingContext2D, look: ContrastLook, draw: () => void): void {
  if (look === "hide") return;
  if (look === "dim") return withColourScope(ctx, dimColours(), draw);
  draw();
}

/**
 * Runs `draw` for an item on layer `key` (a state key) at `alpha` times the layer's opacity, as High Contrast Mode leaves it. In the board editor
 * (`dimByColour`) an inactive item is drawn in its colours mixed into the background, `PCB_RENDER_SETTINGS::GetColor`'s `color.Mix( backgroundColor,
 * m_hiContrastFactor )`: opaque where it is, as KiCad's (a lowered alpha would show what lies under it); the Hide mode draws it not at all; `exempt` (an item of
 * a highlighted net: `!highlighted && !selected`) is drawn as it is. Elsewhere it is `layerAlpha`'s quarter alpha.
 */
export function withLayer(ctx: CanvasRenderingContext2D, opts: PaintOptions, key: string, alpha: number, draw: () => void, exempt = false, kind: GalPass["kind"] = "item"): void {
  // drawn in its own layer's pass only (`paintBoard`'s layer order, UI-LEFT-2 PP3)
  if (!inPass(opts, key, kind)) return;
  if (!opts.dimByColour) return withAlpha(ctx, layerAlpha(opts, key) * alpha, draw);
  const a = (opts.layerOpacity[key] ?? 1) * alpha;
  if (exempt || !layerDimmed(opts, key)) return withAlpha(ctx, a, draw);
  if (opts.appearance?.contrastHidden) return;
  withColourScope(ctx, dimColours(), () => withAlpha(ctx, a, draw));
}

/**
 * `withLayer` for the items the board editor dims and the Footprint Editor's painters do not (texts, shapes, dimensions, text boxes and barcodes of the board):
 * without `dimByColour` the draw is run as it is. Edge.Cuts is pushed back by `dim_factor_Edge_Cuts`, never hidden ("Graphics on Edge_Cuts layer are not fully
 * dimmed or hidden because they are useful when working on another layer").
 */
export function withBoardLayer(ctx: CanvasRenderingContext2D, opts: PaintOptions, layer: string, draw: () => void, exempt = false, kind: GalPass["kind"] = "item"): void {
  if (!inPass(opts, layerStateKey(layer), kind)) return;
  if (!opts.dimByColour) return draw();
  if (layer === "Edge.Cuts" && !exempt && layerDimmed(opts, layerStateKey(layer))) return withColourScope(ctx, dimColours(edgeCutsDimFactor()), draw);
  withLayer(ctx, opts, layerStateKey(layer), 1, draw, exempt, kind);
}

/**
 * The alpha of a reference image on `key`: "Reference images can't have their color mixed so just reduce the opacity a bit so they show through less"
 * (`color.a *= m_hiContrastFactor`) in the board editor, `layerAlpha` elsewhere.
 */
export function refImageLayerAlpha(opts: PaintOptions, key: string): number {
  if (!opts.dimByColour || !layerDimmed(opts, key)) return layerAlpha(opts, key);
  return opts.appearance?.contrastHidden ? 0 : (opts.layerOpacity[key] ?? 1) * hiContrastFactor();
}

/** The rule that fills `pathForPad`: a trapezoid, chamfered or custom pad is its outline's contours, a ring having a hole. */
function padFillRule(pad: Pad): CanvasFillRule {
  return pad.outline?.length ? "evenodd" : "nonzero";
}

function pathForPad(ctx: CanvasRenderingContext2D, pad: Pad) {
  const isCircle = pad.round && Math.abs(pad.w - pad.h) < 1;
  ctx.beginPath();
  if (pad.outline?.length) {
    // The copper as the DRC measures it (`pad_outline`): a trapezoid's slanted sides, a chamfer's cut corner, a custom pad's basic shapes.
    for (const contour of pad.outline) {
      contour.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.closePath();
    }
  } else if (isCircle) {
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
  ctx.lineWidth = hairlineNm(view, 1.5);
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
export function drawPadNumber(ctx: CanvasRenderingContext2D, pad: Pick<Pad, "x" | "y" | "w" | "h" | "num">, withNet: boolean, color: string, viewFlipped = false) {
  const layout = padTextLayout(pad, withNet);
  // "We use a size for at least 3 chars ... a smaller text size to handle interline, pen size"; the stroked font is 0.9 as wide.
  let tsize = Math.min((1.5 * layout.width) / Math.max([...pad.num].length, 3), layout.size);
  tsize = Math.min(tsize * 0.85, layout.size);
  ctx.save();
  ctx.translate(pad.x, pad.y);
  if (layout.rotated) ctx.rotate(-Math.PI / 2);
  // (x, y) is the baseline: the cap height is about 0.7 of the size, so half of it puts the digits' middle where KiCad centres them.
  // `GAL::BitmapText`: "Prevent text flipping when view is flipped" (`attrs.m_Mirrored = m_globalFlipX`)
  drawStrokeText(ctx, pad.num, 0, -layout.numberOffset + tsize * 0.35, { sizeNm: tsize, justify: "center", thicknessNm: (tsize * 0.9) / 6, color, keepReadable: viewFlipped, readableAnchorKept: true });
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
    // `perRefOffsetNm`: Pack and Move's per-footprint SpreadFootprints shift (absent for every ordinary move).
    const own = preview.perRefOffsetNm?.[part.ref];
    ctx.translate(preview.dxNm + (own?.[0] ?? 0), preview.dyNm + (own?.[1] ?? 0));
    // edit_tool.cpp Rotate()/Flip() during an active Move spin the
    // dragged item in place around its own (already-moving) anchor --
    // see useActionRunner.ts's `tryTransformDuringMove` doc comment for
    // why that's an exact-not-approximate match for a single dragged
    // part. R turns counter-clockwise like KiCad's, and `ctx.rotate`
    // is clockwise-positive in a Y-down canvas, hence the sign.
    if (preview.rotateQuarterTurns || preview.flipped) {
      const [ax, ay] = part.at ?? [(x0 + x1) / 2, (y0 + y1) / 2];
      ctx.translate(ax, ay);
      if (preview.rotateQuarterTurns) ctx.rotate((-preview.rotateQuarterTurns * (preview.rotateStepDeg ?? 90) * Math.PI) / 180);
      if (preview.flipped) ctx.scale(preview.flipDirection === "top_bottom" ? 1 : -1, preview.flipDirection === "top_bottom" ? -1 : 1);
      ctx.translate(-ax, -ay);
    }
  }

  // Courtyard.
  const courtyardKey = part.side === "bottom" ? "b_courtyard" : "f_courtyard";
  if (selected || isHot || opts.layerVisible[courtyardKey] !== false) {
    withLayer(ctx, opts, courtyardKey, 1, () => {
      ctx.beginPath();
      ctx.rect(x0, y0, x1 - x0, y1 - y0);
      ctx.strokeStyle = selected ? layerColor("selection") : isHot ? "#ef5b5b" : layerColor(courtyardKey);
      ctx.lineWidth = hairlineNm(view, selected || isHot ? 2 : 1);
      if (!selected && !isHot) ctx.setLineDash([hairlineNm(view, 3), hairlineNm(view, 2)]);
      ctx.stroke();
      ctx.setLineDash([]);
    });
  }

  // Pads, in the passes of their layers (`drawPads`).
  drawPads(ctx, view, part, opts);

  // The footprint's own silkscreen, fab and assembly drawings and its texts (`part.graphics`, `part.texts`), each on its layer and over the pads.
  drawFootprintArt(ctx, view, part, opts);
  // Its text boxes, barcodes and points (`part.items`, UI-LEFT P17); its images are drawn with the board's, under everything.
  drawFootprintItems(ctx, view, part, opts);

  // The Reference, the Value and the user fields are drawn by `drawFields` once the backend says where they are (`part.fields`); an older backend's footprint has
  // only its reference, drawn beside the courtyard the way it always was.
  if (!part.fields) {
    const fs = Math.max(500_000, Math.min(900_000, Math.min(x1 - x0, y1 - y0) * 0.45));
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
      withLayer(ctx, opts, silkKey, 1, () => {
        drawStrokeText(ctx, part.ref, tx, ty, { sizeNm: fs, justify: align, thicknessNm: opts.sketchText ? hairlineNm(view, 1) : fs / 6, color: layerColor(silkKey) });
      });
    }

  }

  ctx.restore();
}

/** `m_holePlatingThickness` (`BOARD_DESIGN_SETTINGS::GetHolePlatingThickness()`, Board Setup's 0.018 mm, which the state does not send) times
 * `ADVANCED_CFG::m_HoleWallPaintingMultiplier` (1.5): the width a hole wall is painted at. */
const HOLE_WALL_NM = 18_000 * 1.5;

/** The pad net-name layer of a pad (`PAD::ViewGetLayers`): a pad on more than one copper layer -- or a plated one -- `LAYER_PAD_NETNAMES`, an SMD pad its side's. */
function padNamesLayer(pad: Pad, side: Part["side"]): string {
  if (pad.th) return "LAYER_PAD_NETNAMES";
  return side === "bottom" ? "LAYER_PAD_BK_NETNAMES" : "LAYER_PAD_FR_NETNAMES";
}

/**
 * The copper `pad` has on copper layer `layer` (`PAD::FlashLayer`, the padstack's shape there), or null: a padstack pad its group's shape; a through-hole pad
 * every copper layer of the board, but an unplated one only where its copper is bigger than its hole; a surface-mount pad its footprint's side.
 */
function padCopperOn(pad: Pad, layer: string, sideCopper: string, copper: readonly string[]): Pad | null {
  if (Array.isArray(pad.stack_layers)) {
    const g = pad.stack_layers.find((s) => s.layers.includes(layer));
    return g ? { ...pad, x: g.x, y: g.y, w: g.w, h: g.h, round: g.round, shape: g.shape, ratio: g.ratio ?? null, outline: g.outline ?? null } : null;
  }
  if (!pad.th) return layer === sideCopper ? pad : null;
  if (pad.kind === "non_plated_hole") {
    // "NPTH pads have no copper" where the hole covers the pad: a round pad with a round hole at least as big, an oval with an oblong one as big both ways
    const hole = pad.slot ?? (pad.drill ? [pad.drill, pad.drill] : null);
    if (hole && hole[0] >= Math.min(pad.w, pad.h) && hole[1] >= Math.min(pad.w, pad.h) && (pad.round || pad.shape === "circle" || pad.shape === "oval")) return null;
  }
  return copper.includes(layer) || layer === sideCopper ? pad : null;
}

/**
 * A footprint's pads as `PCB_PAINTER::draw( const PAD*, aLayer )` draws them on each of their layers (UI-LEFT-2 PP3), in the pass of that layer:
 * - `PAD_COPPER_LAYER_FOR( layer )`: the pad's copper on that copper layer, in the layer's colour ("Pad and via copper and clearance outlines take their color
 *   from the copper layer") -- a through-hole pad on every copper layer it flashes on, each its own shape (`PADSTACK::ForEachUniqueLayer`), the active layer's
 *   on top (`SetTopLayer`); active in High Contrast Mode on the primary layer only (`m_highContrastLayers.count( aLayer )`);
 * - `LAYER_PAD_HOLEWALLS`: a plated hole's wall, `HOLE_WALL_NM` wide round the hole in the via-hole colour ("Use via 'golden copper' hole color for pad hole walls
 *   for contrast"); `LAYER_PAD_PLATEDHOLES`: the hole in the background's colour; `LAYER_NON_PLATEDHOLES`: an unplated one in its own colour -- over all the
 *   copper; active on a copper layer (`LAYER_PAD_PLATEDHOLES` is high contrast with a copper layer only);
 * - `LAYER_PAD_NETNAMES` / `LAYER_PAD_FR_NETNAMES` / `LAYER_PAD_BK_NETNAMES`: the number and net name, only on an active pad;
 * - the selection's outline of a selected pad (pads are selectable on their own) in `LAYER_SELECT_OVERLAY`.
 * Outside the layer passes (one paint of the footprint) every part at once, its side's copper on top.
 * A pad of a highlighted net is left as it is in High Contrast Mode (`!highlighted`); the net highlight brightens or darkens its copper, walls and holes
 * (`GetColor`: a pad is a connected item of its net). The pad takes the copper's colours, not the footprint's, so the scopes the footprint is drawn in are set aside.
 */
function drawPads(ctx: CanvasRenderingContext2D, view: ViewTransform, part: Part, opts: PaintOptions): void {
  const pass = opts.galPass;
  const wanted = (gal: string) => !pass || (pass.kind === "gal" && pass.layer === gal);
  const padsOn = on(opts.layerVisible, "pads");
  const pads = part.pads ?? [];
  if (pads.length === 0) return;
  const sideCopper = part.side === "bottom" ? "B.Cu" : "F.Cu";
  const copper = opts.copperLayers ?? ["F.Cu", "B.Cu"];
  const padAlpha = opacityOf(opts.appearance, "pads");
  const primary = contrastPrimary(opts);
  const hiddenMode = !!opts.appearance?.contrastHidden;
  const look = (pad: Pad, kind: "copper" | "hole", layer: string): ContrastLook =>
    !primary || isHighlighted(pad.net, opts.netHighlight) ? "active" : kind === "hole" ? padHoleContrast(primary, hiddenMode) : padContrast(primary, { th: false, copperLayer: layer, flashesOnPrimary: false }, hiddenMode);
  const scoped = (draw: () => void) =>
    withColourScope(ctx, null, () => {
      ctx.save();
      ctx.globalAlpha *= padAlpha;
      draw();
      ctx.restore();
    });
  const paintCopper = (shape: Pad, layer: string, net: string | null) => {
    const paint = copperPaint(layerColor(copperColorKey(layer)), net, opts);
    pathForPad(ctx, shape);
    if (opts.sketchPads) {
      ctx.strokeStyle = paint;
      ctx.lineWidth = hairlineNm(view, 1.5);
      ctx.stroke();
    } else {
      ctx.fillStyle = paint;
      ctx.fill(padFillRule(shape));
    }
  };
  // `PAD_COPPER_LAYER_FOR( layer )`
  if (padsOn && (!pass || pass.kind === "pad")) {
    // one paint: every copper layer, back to front with the footprint's side last (`orderedStackLayers` for a padstack)
    const layers = pass ? [pass.layer] : [...copper].reverse().filter((l) => l !== sideCopper).concat(sideCopper);
    for (const layer of layers) {
      if (opts.layerVisible[layer] === false) continue;
      for (const pad of pads) {
        const shape = padCopperOn(pad, layer, sideCopper, copper);
        if (!shape) continue;
        scoped(() => withLook(ctx, look(pad, "copper", layer), () => paintCopper(shape, layer, pad.net)));
      }
    }
  }
  // the holes, over every copper layer
  const anyCopperShown = copper.some((l) => opts.layerVisible[l] !== false);
  if (padsOn && anyCopperShown) {
    for (const pad of pads) {
      if (!pad.th) continue;
      const npth = pad.kind === "non_plated_hole";
      if (!npth && wanted("LAYER_PAD_HOLEWALLS")) {
        // `DrawHoleWall` / `DrawSegment( .., holeSize + 2 * lineWidth )`: a ring the wall's width round the hole (its inner half is under the hole)
        scoped(() =>
          withLook(ctx, look(pad, "hole", ""), () => {
            if (pathForHole(ctx, pad)) {
              ctx.strokeStyle = copperPaint(layerColor("pad_th"), pad.net, opts);
              ctx.lineWidth = 2 * Math.max(HOLE_WALL_NM, hairlineNm(view, 1));
              ctx.stroke();
            } else {
              // no hole size from the backend: the pad's outline, as before
              pathForPad(ctx, pad);
              ctx.strokeStyle = copperPaint(layerColor("pad_th"), pad.net, opts);
              ctx.lineWidth = hairlineNm(view, 1);
              ctx.stroke();
            }
          })
        );
      }
      if (wanted(npth ? "LAYER_NON_PLATEDHOLES" : "LAYER_PAD_PLATEDHOLES")) {
        scoped(() =>
          withLook(ctx, look(pad, "hole", ""), () => {
            if (!pathForHole(ctx, pad)) return;
            ctx.fillStyle = npth ? layerColor("LAYER_NON_PLATEDHOLES") : copperPaint(layerColor("background"), pad.net, opts);
            ctx.fill();
          })
        );
      }
    }
  }
  // the number and the net name, of an active pad only (`IsNetnameLayer( aLayer )`: `color = COLOR4D::CLEAR` off the high-contrast layers)
  if (padsOn) {
    for (const pad of pads) {
      if (!wanted(padNamesLayer(pad, part.side))) continue;
      const shown = pad.th ? copper.some((l) => opts.layerVisible[l] !== false) : opts.layerVisible[sideCopper] !== false;
      if (!shown) continue;
      const nameLayer = pad.th ? (primary && isCopper(primary) ? primary : sideCopper) : sideCopper;
      if (look(pad, "copper", nameLayer) !== "active") continue;
      scoped(() => {
        const numbered = opts.showPadNumbers && pad.num !== "" && padNumberLegible(pad, view);
        if (pad.net && pad.w * view.scale > 22 && pad.h * view.scale > 10) {
          ctx.fillStyle = "rgba(0,0,0,0.75)";
          const fontNm = Math.min(pad.w, pad.h) * 0.35;
          ctx.font = `${fontNm}px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`;
          ctx.textAlign = "center";
          ctx.textBaseline = "middle";
          // With the number on too, the two share the pad: the net name drops below the middle (`Y_offset_netname`).
          ctx.fillText(pad.net, pad.x, numbered ? pad.y + padTextLayout(pad, true).netOffset : pad.y, pad.w * 0.9);
        }
        if (numbered) drawPadNumber(ctx, pad, !!pad.net && pad.w * view.scale > 22 && pad.h * view.scale > 10, layerColor("pad_netname"), opts.viewFlipped);
      });
    }
  }
  // A selected pad: the selection colour around it.
  if (!opts.selection.has(part.ref) && wanted("LAYER_SELECT_OVERLAY")) {
    const ids = padIds(part);
    pads.forEach((pad, i) => {
      if (!opts.selection.has(ids[i]!)) return;
      pathForPad(ctx, pad);
      ctx.strokeStyle = layerColor("selection");
      ctx.lineWidth = hairlineNm(view, 2.5);
      ctx.stroke();
    });
  }
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
 * A footprint's fields (`PCB_FIELD`: Reference, Value, user fields) as the board has them -- position, size, thickness, layer, angle, justification, mirroring,
 * bold, italic, knockout -- in KiCad's stroke font or the field's outline face. A hidden field is not drawn, its footprint selected or not
 * (`PCB_PAINTER::draw( const PCB_FIELD* )` draws only a visible one; the Properties panel and the dialog show it). `GetDrawRotation`'s keep-upright rule is
 * `fieldSpec`'s (`drawAngle`).
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
    const color = selected ? layerColor("selection") : layerColor(realLayerKey(f.layer));
    // `FONT::Draw` from the field's position (kicad-port/textSpecs.ts `fieldSpec`: `GetDrawRotation`, the width and height, the effective pen, the face), as the hit
    // test lays it out (UI-LEFT-2 PP1); a knockout field is its box filled, the text cut out of it (`GetKnockoutCache`).
    const { spec, face } = fieldSpec(f);
    withLayer(ctx, opts, key, f.visible ? 1 : 0.5, () => {
      paintKicadText(ctx, spec, face, {
        color,
        pen: opts.sketchText ? hairlineNm(view, 1) : undefined,
        minPen: hairlineNm(view, 0.5),
        knockout: f.knockout ? layerColor("background") : null,
        flippedView: keptReadableInFlippedView(opts.viewFlipped, f.layer, opts.layerTypes),
        record: { tag: opts.textRecordTag ?? "pcb", id: f.id },
      });
    });
  }
}

/** The tracks on copper layer `onLayer`, in that layer's pass (`PCB_PAINTER::draw( const PCB_TRACK* )`). */
function drawTracks(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions, onLayer: string) {
  if (!board.routing) return;
  const tracksOn = on(opts.layerVisible, "tracks");
  const trackAlpha = opacityOf(opts.appearance, "tracks");
  // the tuner's preview takes the place of the tracks it replaces (`hideRemovedItems`)
  const replaced = opts.drawState?.kind === "tune" && opts.drawState.hide.length > 0 ? new Set(opts.drawState.hide) : null;
  for (const t of tracksOn ? board.routing.tracks : []) {
    if (t.layer !== onLayer || replaced?.has(t.id)) continue;
    if (opts.layerVisible[t.layer] === false) continue;
    const key = copperColorKey(t.layer);
    // Slightly translucent tracks, through withAlpha's own scoped save/restore -- never `ctx.globalAlpha *=` in place, which decayed it over a route's whole
    // track list and left every draw after the tracks invisible. layerAlpha is keyed by the layer's name ("F.Cu"); `key` is the colour key.
    // High Contrast Mode leaves an item of a highlighted net as it is (`!highlighted`).
    withLayer(ctx, opts, t.layer, 0.92 * trackAlpha, () => {
      ctx.beginPath();
      t.pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.strokeStyle = copperPaint(layerColor(key), t.net, opts);
      ctx.lineWidth = opts.sketchTracks ? hairlineNm(view, 1.5) : Math.max(t.width, hairlineNm(view, 1));
      ctx.lineCap = "round";
      ctx.lineJoin = "round";
      ctx.stroke();
    }, isHighlighted(t.net, opts.netHighlight));
  }
}

/** Does the via span copper layer `layer` (`GetLayerSet().test( layer )`), by the board's copper order (front to back)? */
function viaSpans(v: { from: string; to: string }, layer: string, copper: readonly string[]): boolean {
  const a = copper.indexOf(v.from);
  const b = copper.indexOf(v.to);
  const i = copper.indexOf(layer);
  if (a < 0 || b < 0 || i < 0) return v.from === layer || v.to === layer;
  return i >= Math.min(a, b) && i <= Math.max(a, b);
}

/**
 * The vias' copper on copper layer `onLayer` (`VIA_COPPER_LAYER_FOR( layer )`, `PCB_PAINTER::draw( const PCB_VIA*, aLayer )`, UI-LEFT-2 PP3): a ring from the hole
 * to the layer's own diameter (`aVia->GetWidth( currentLayer )`, a padstack's per layer -- kicad-port/viaPadstack.ts) in the layer's colour, on each layer the via
 * spans and flashes on (`FlashLayer`; with `remove_unused_layers` only the ones it keeps); in sketch mode its outline. Active in High Contrast Mode on the primary
 * layer only. The holes are `drawViaHoles`', over every copper layer.
 */
function drawViaCopper(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions, onLayer: string) {
  if (!board.routing || !on(opts.layerVisible, "vias") || opts.layerVisible[onLayer] === false) return;
  const viaAlpha = opacityOf(opts.appearance, "vias");
  const primary = contrastPrimary(opts);
  const copper = opts.copperLayers ?? board.layers.filter(isCopper);
  const hiddenMode = !!opts.appearance?.contrastHidden;
  for (const v of board.routing.vias) {
    if (!viaSpans(v, onLayer, copper) || (v.flashed && !v.flashed.includes(onLayer))) continue;
    const look: ContrastLook = primary && !isHighlighted(v.net, opts.netHighlight) ? viaCopperContrast(primary, onLayer, hiddenMode) : "active";
    withLook(ctx, look, () => {
      ctx.save();
      ctx.globalAlpha *= viaAlpha;
      const colour = copperPaint(layerColor(copperColorKey(onLayer)), v.net, opts);
      const d = viaWidthOn(v, onLayer);
      ctx.beginPath();
      if (opts.sketchVias) {
        ctx.arc(v.x, v.y, d / 2, 0, Math.PI * 2);
        ctx.strokeStyle = colour;
        ctx.lineWidth = hairlineNm(view, 1.5);
        ctx.stroke();
      } else {
        // `SetLineWidth( annular_width ); radius -= annular_width / 2.0; DrawCircle( center, radius )`
        const annular = Math.max(0, (d - v.drill) / 2);
        ctx.arc(v.x, v.y, d / 2 - annular / 2, 0, Math.PI * 2);
        ctx.strokeStyle = colour;
        ctx.lineWidth = annular;
        ctx.stroke();
      }
      ctx.restore();
    });
  }
}

/**
 * The vias' holes (`LAYER_VIA_HOLEWALLS`, `LAYER_VIA_HOLES`), over every copper layer: the wall a disc `HOLE_WALL_NM` wider than the hole -- no wider than the via's
 * top copper -- in the hole-wall colour ("Underpaint the hole so that there aren't artifacts at its edge"); the hole a disc in the via-hole colour, or for a blind,
 * buried or micro via between inner layers its top half in its top layer's colour and its bottom half in its bottom layer's. Active in High Contrast Mode with a
 * copper layer primary (a blind, buried or micro via one it spans).
 */
function drawViaHoles(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions, which: "walls" | "holes") {
  if (!board.routing || !on(opts.layerVisible, "vias")) return;
  const copper = opts.copperLayers ?? board.layers.filter(isCopper);
  if (!copper.some((l) => opts.layerVisible[l] !== false)) return;
  const viaAlpha = opacityOf(opts.appearance, "vias");
  const primary = contrastPrimary(opts);
  const hiddenMode = !!opts.appearance?.contrastHidden;
  for (const v of board.routing.vias) {
    const look: ContrastLook = primary && !isHighlighted(v.net, opts.netHighlight) ? viaHoleContrast(primary, v, copper, hiddenMode) : "active";
    withLook(ctx, look, () => {
      ctx.save();
      ctx.globalAlpha *= viaAlpha;
      if (which === "walls") {
        const radius = Math.min(v.drill / 2 + HOLE_WALL_NM, viaWidthOn(v, v.from) / 2);
        if (radius > v.drill / 2) {
          ctx.beginPath();
          ctx.arc(v.x, v.y, Math.max(radius, v.drill / 2 + hairlineNm(view, 0.5)), 0, Math.PI * 2);
          ctx.fillStyle = copperPaint(layerColor("LAYER_VIA_HOLEWALLS"), v.net, opts);
          ctx.fill();
        }
      } else {
        const r = v.drill / 2;
        const blindBuried = v.type === "blind" || v.type === "buried" || (v.type === "micro" && (v.from !== copper[0] || v.to !== copper[copper.length - 1]));
        if (blindBuried) {
          // `DrawArc( center, radius, 180, 180 )` in the top layer's colour, `DrawArc( center, radius, 0, 180 )` in the bottom's
          ctx.beginPath();
          ctx.moveTo(v.x, v.y);
          ctx.arc(v.x, v.y, r, Math.PI, 2 * Math.PI);
          ctx.closePath();
          ctx.fillStyle = copperPaint(layerColor(copperColorKey(v.from)), v.net, opts);
          ctx.fill();
          ctx.beginPath();
          ctx.moveTo(v.x, v.y);
          ctx.arc(v.x, v.y, r, 0, Math.PI);
          ctx.closePath();
          ctx.fillStyle = copperPaint(layerColor(copperColorKey(v.to)), v.net, opts);
          ctx.fill();
        } else {
          ctx.beginPath();
          ctx.arc(v.x, v.y, r, 0, Math.PI * 2);
          ctx.fillStyle = copperPaint(layerColor("LAYER_VIA_HOLES"), v.net, opts);
          ctx.fill();
        }
      }
      ctx.restore();
    });
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
/** The zone's outline and its holes (`ZONE::Outline()`), each a closed subpath: fill or clip it with the even-odd rule. */
function traceZone(ctx: CanvasRenderingContext2D, z: Zone) {
  for (const ring of [z.outline, ...(z.holes ?? [])]) {
    ring.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.closePath();
  }
}

/**
 * A rule area (`ZONE::GetIsRuleArea()`, a keepout or a placement area) on one of its layers, as `PCB_PAINTER::draw( const ZONE* )` draws any zone there: every
 * contour as a one-pixel polyline in `color` (the layer's) at full opacity and its border hatch (`GetHatchLines()`); it never has a fill. A selected one has
 * the selection's outline on top. (The studio drew one picture for the whole set, in its own orange, dashed, with a label of its keepouts: KiCad draws none.)
 */
export function drawRuleArea(ctx: CanvasRenderingContext2D, view: ViewTransform, z: Zone, selected: boolean, color: string) {
  const ink = parseOpaque(color);
  ctx.beginPath();
  traceZone(ctx, z);
  ctx.strokeStyle = ink;
  ctx.lineWidth = hairlineNm(view, 1);
  ctx.stroke();
  strokeHatchLines(ctx, view, z, ink);
  if (selected) {
    ctx.beginPath();
    traceZone(ctx, z);
    ctx.strokeStyle = layerColor("selection");
    ctx.lineWidth = hairlineNm(view, 2.5);
    ctx.stroke();
  }
}

/** `color.a > 0.0 ? color.WithAlpha( 1.0 ) : color`: a zone's outline is drawn in its layer's colour at full opacity. */
function parseOpaque(color: string): string {
  const c = parseColor(color);
  return c && c.a > 0 ? cssOf({ ...c, a: 1 }) : color;
}

/** `for( const SEG& hatchLine : aZone->GetHatchLines() ) m_gal->DrawLine( hatchLine.A, hatchLine.B )`, at the outline width (`m_outlineWidth`: one pixel). */
function strokeHatchLines(ctx: CanvasRenderingContext2D, view: ViewTransform, z: Zone, color: string) {
  const lines = cachedZoneHatchLines(z);
  if (lines.length === 0) return;
  ctx.beginPath();
  for (const [a, b] of lines) {
    ctx.moveTo(a[0], a[1]);
    ctx.lineTo(b[0], b[1]);
  }
  ctx.strokeStyle = color;
  ctx.lineWidth = hairlineNm(view, 1);
  ctx.stroke();
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

/** The fill the last `GET /api/fill` made of zone `id` on `layer` (a zone on several layers has one under each). */
function fillOf(opts: PaintOptions, id: string, layer: string) {
  return opts.zoneFill?.zones.find((f) => f.id === id && (f.layer === undefined || f.layer === layer));
}

/**
 * One zone on one of its layers, in `color`: its fill (solid, or only the fractured ring's edges or the triangles, per `zoneDisplayMode`) when it has one, then -- whether
 * or not it has one -- its outline and border hatch as `PCB_PAINTER::draw( const ZONE* )` strokes them on the zone's own layer (every contour as a polyline, then
 * `GetHatchLines()`, at the outline width, one pixel, in the layer's colour at full opacity: `color.WithAlpha( 1.0 )`), and the selection's outline on top. A copper
 * pour and a non-copper zone (silkscreen, mask, paste, a user layer) are drawn the same way. `alpha` is the fill's (the layer's and the zones' opacity), `outlineAlpha`
 * the outline's (the layer's).
 */
export function paintZoneOnLayer(ctx: CanvasRenderingContext2D, view: ViewTransform, z: Zone, layerName: string, color: string, alpha: number, selected: boolean, opts: PaintOptions, outlineAlpha = 1) {
  const fill = fillOf(opts, z.id, layerName);
  withAlpha(ctx, alpha, () => {
    if (opts.zoneDisplayMode !== "outline" && fill && fill.fragments.length > 0) {
      if (opts.zoneDisplayMode === "filled") {
        // `fragments` are already "Fracture"d (pcb_painter.cpp paints the
        // real ZONE_FILLER output the same way): each is one closed ring,
        // holes slit into the outer boundary -- a plain nonzero-winding
        // fill per fragment is exactly right, no separate even-odd pass.
        ctx.fillStyle = color;
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
        ctx.strokeStyle = color;
        ctx.lineWidth = hairlineNm(view, 1);
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
    }
  });
  // The outline and its hatch over the fill (the zone's own layer is drawn above its fill layer), whether or not there is a fill; then the selection's.
  // (`allowDrawOutline = GetHatchStyle() != INVISIBLE_BORDER`: only a teardrop KiCad made has that style, and teardrops are drawn by `drawZones` itself.)
  withAlpha(ctx, outlineAlpha, () => {
    ctx.beginPath();
    traceZone(ctx, z);
    ctx.strokeStyle = color;
    ctx.lineWidth = hairlineNm(view, 1);
    ctx.stroke();
    strokeHatchLines(ctx, view, z, color);
  });
  if (selected) {
    // Source's selection shadow is a separate highlight layer this
    // app doesn't model; stroking the zone's own (unfractured)
    // outline on top reads the same "this one's selected" cue,
    // without recoloring the copper.
    withAlpha(ctx, outlineAlpha, () => {
      ctx.beginPath();
      traceZone(ctx, z);
      ctx.strokeStyle = layerColor("selection");
      ctx.lineWidth = hairlineNm(view, 2.5);
      ctx.stroke();
    });
  }
}

/** Which of the painter's three copper passes a copper layer belongs to. */
/** The zones' fills and outlines on copper layer `onLayer`, in its zone pass (`ZONE_LAYER_FOR( layer )`). */
function drawZones(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions, onLayer: string) {
  if (!board.routing || !on(opts.layerVisible, "zones")) return;
  const zoneAlpha = opacityOf(opts.appearance, "zones");
  const teardropAlpha = opacityOf(opts.appearance, "tracks"); // `IsTeardropArea()` takes the track opacity
  for (const z of board.routing.zones) {
    if (z.outline.length < 3) continue;
    const selected = opts.selection.has(z.id);
    // A zone on several layers is drawn on each of its copper layers, in that layer's pass (`ZONE::ViewGetLayers`); the non-copper ones are `drawNonCopperZones`'.
    const copperLayers = zoneCopperLayers(z);
    // High Contrast Mode leaves a zone of a highlighted net as it is (`!highlighted`).
    const lit = isHighlighted(z.net, opts.netHighlight);
    for (const layerName of copperLayers) {
      const key = copperColorKey(layerName);
      if (layerName !== onLayer) continue;
      if (opts.layerVisible[layerName] === false) continue;
      if (z.teardrop) {
        // A teardrop's own outline already *is* its final filled shape
        // (task item 4's generator computes the exact pentagon, no
        // knockout/thermal-relief pipeline applies) -- draw it solid
        // unconditionally, regardless of `zoneDisplayMode`, same as its
        // anchor pad/via is never shown as an "outline only" shape either.
        withLayer(ctx, opts, layerName, teardropAlpha, () => {
          ctx.beginPath();
          traceZone(ctx, z);
          ctx.fillStyle = copperPaint(layerColor(key), z.net, opts);
          ctx.fill("evenodd");
          if (selected) {
            ctx.strokeStyle = layerColor("selection");
            ctx.lineWidth = hairlineNm(view, 2.5);
            ctx.stroke();
          }
        }, lit, "zone");
        continue;
      }
      // A rule area is drawn as any zone is, on each of its layers in the layer's colour: its outline and border hatch (`PCB_PAINTER::draw( const ZONE* )`); it has no fill.
      withLayer(ctx, opts, layerName, 1, () => paintZoneOnLayer(ctx, view, z, layerName, copperPaint(layerColor(key), z.net, opts), zoneAlpha, selected, opts), lit, "zone");
    }
  }
}

/**
 * The zones that are on no copper layer (`fillNonCopperZone`: silkscreen, mask, paste, a user layer) and a rule area drawn only on such layers: each on every
 * one of its layers that is shown, in that layer's colour. They have no net.
 */
function drawNonCopperZones(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions) {
  if (!board.routing || !on(opts.layerVisible, "zones")) return;
  const zoneAlpha = opacityOf(opts.appearance, "zones");
  for (const z of board.routing.zones) {
    if (z.outline.length < 3 || z.teardrop) continue;
    const selected = opts.selection.has(z.id);
    const layers = zoneLayers(z).filter((l) => !isCopper(l) && layerIsVisible(opts.layerVisible, l));
    if (layers.length === 0) continue;
    // Its copper layers are the copper passes' business; a rule area is drawn on each layer as any zone is (its outline and hatch, no fill).
    for (const layerName of layers) withBoardLayer(ctx, opts, layerName, () => paintZoneOnLayer(ctx, view, z, layerName, layerColor(realLayerKey(layerName)), zoneAlpha, selected, opts), false, "zone");
  }
}

/** A Shape/Text's own dotted KiCad layer name ("F.SilkS") -> colors.json's real key ("F_SilkS") -- unlike copperColorKey() (which also lowercases, fine for copper's own always-lowercase-safe names but wrong for e.g. "Edge.Cuts" -> "Edge_Cuts"), these carry a real named layer already and just need the punctuation swapped, case preserved. */
export function realLayerKey(layer: string): string {
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
    // A copper graphic of a net is a connected item: the net highlight brightens it when its net is highlighted (`m_layerColorsHi`), as it darkens the rest.
    const lit = !!s.net && isCopper(s.layer) && isHighlighted(s.net, opts.netHighlight);
    const draw = () => {
      ctx.save();
      // A filled shape takes the Filled Shapes opacity, any other graphic on copper the track opacity (`PCB_RENDER_SETTINGS::GetColor`).
      ctx.globalAlpha *= s.kind !== "segment" && s.kind !== "arc" && s.kind !== "bezier" && isAnyFill(s) ? opacityOf(opts.appearance, "shapes") : isCopper(s.layer) ? opacityOf(opts.appearance, "tracks") : 1;
      ctx.strokeStyle = selected ? layerColor("selection") : layerColor(bucketish);
      ctx.fillStyle = ctx.strokeStyle;
      ctx.lineWidth = Math.max(s.stroke_width, hairlineNm(view, selected ? 2 : 1));
      ctx.lineCap = "round";
      ctx.lineJoin = "round";
      drawShapeGeometry(ctx, s, !!opts.sketchGraphics, shapeDash(s, ctx.lineWidth, opts.dashRatios ?? boardDashRatios(board)));
      ctx.restore();
    };
    withBoardLayer(ctx, opts, s.layer, lit ? () => withColourScope(ctx, null, () => withColourScope(ctx, memoColours("lit", highlightBrighten), draw)) : draw, lit);
  }
}

/**
 * The dashes of a shape whose line style is not solid (`PCB_PAINTER::draw( PCB_SHAPE )`: `STROKE_PARAMS::Stroke( shape, lineStyle, getLineThickness( GetWidth() ),
 * &m_pcbSettings, .. )`), measured by its width -- the drawn width for a hairline (`getLineThickness`: `m_outlineWidth` for 0) -- and the board's Dashed lines
 * ratios (Board Setup > Formatting, `PCB_BASE_FRAME`'s `SetDashLengthRatio`); undefined for a solid one.
 */
export function shapeDash(s: Pick<Shape, "dash" | "stroke_width">, drawnWidth: number, ratios?: DashRatios): DashSpec | undefined {
  if (!isDashStyle(s.dash)) return undefined;
  return { style: s.dash, width: s.stroke_width > 0 ? s.stroke_width : drawnWidth, ratios: ratios ?? DEFAULT_DASH_RATIOS };
}

/** Board Setup > Formatting's Dashed lines ratios of `board` (`dashed_line_dash_ratio`, `dashed_line_gap_ratio`), KiCad's 12 and 3 when it says none. */
export function boardDashRatios(board: Pick<BoardState, "board_rules"> | null | undefined): DashRatios {
  const f = board?.board_rules?.formatting;
  return f ? { dash: f.dash_length_ratio, gap: f.gap_length_ratio } : DEFAULT_DASH_RATIOS;
}

/** A dashed shape's strokes, each a round-capped segment of the pen the caller set (`m_gal->DrawSegment( a, b, thickness )`). */
function strokeDashPieces(ctx: CanvasRenderingContext2D, pieces: DashPiece[]) {
  if (pieces.length === 0) return;
  ctx.save();
  ctx.setLineDash([]);
  ctx.lineCap = "round";
  ctx.beginPath();
  for (const [[ax, ay], [bx, by]] of pieces) {
    ctx.moveTo(ax, ay);
    ctx.lineTo(bx, by);
  }
  ctx.stroke();
  ctx.restore();
}

/**
 * `sketch` is `pcbnew.Control.graphicOutlines` ("Sketch Graphic Items", `m_DisplayGraphicsFill` off): a filled shape is drawn as its outline only. `dash` is a line
 * style that is not solid (`shapeDash`): `PCB_PAINTER::draw( PCB_SHAPE )` then fills a solid-filled shape without its outline (`SetIsStroke( lineStyle == SOLID
 * .. )`) and draws the line as `STROKE_PARAMS::Stroke` cuts it (kicad-port/strokeDash.ts), the hatch over it.
 */
export function drawShapeGeometry(ctx: CanvasRenderingContext2D, s: Shape, sketch = false, dash?: DashSpec) {
  // KiCad 10's line endings: the body stops short of an arrow, a circle or a square, and the endings are drawn after it in the same colour (`PCB_PAINTER::draw( PCB_SHAPE )`).
  if ((s.kind === "segment" || s.kind === "arc" || s.kind === "bezier") && hasEndings(s)) {
    drawShapeWithEndings(ctx, s, dash);
    return;
  }
  if (dash) {
    if (s.filled && !sketch && (s.kind === "rect" || s.kind === "circle" || s.kind === "polygon")) {
      ctx.beginPath();
      traceClosedShape(ctx, s);
      ctx.fill(s.kind === "polygon" && s.holes?.length ? "evenodd" : "nonzero");
    }
    strokeDashPieces(ctx, dashPieces(s, dash, BEZIER_MAX_ERROR_NM));
    drawHatchLines(ctx, s);
    return;
  }
  ctx.beginPath();
  switch (s.kind) {
    case "segment":
      ctx.moveTo(s.start[0], s.start[1]);
      ctx.lineTo(s.end[0], s.end[1]);
      ctx.stroke();
      return;
    case "rect": {
      // A corner radius rounds the corners (`ROUNDRECT`, clamped to half the smaller side as its constructor does).
      const [x0, y0] = [Math.min(s.start[0], s.end[0]), Math.min(s.start[1], s.end[1])];
      const [w, h] = [Math.abs(s.end[0] - s.start[0]), Math.abs(s.end[1] - s.start[1])];
      const r = Math.min(s.corner_radius ?? 0, Math.min(w, h) / 2);
      if (r > 0) ctx.roundRect(x0, y0, w, h, r);
      else ctx.rect(s.start[0], s.start[1], s.end[0] - s.start[0], s.end[1] - s.start[1]);
      break;
    }
    case "circle": {
      const r = Math.hypot(s.end[0] - s.center[0], s.end[1] - s.center[1]);
      ctx.arc(s.center[0], s.center[1], r, 0, Math.PI * 2);
      break;
    }
    case "polygon":
      s.pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.closePath();
      // its holes, each a contour of its own (UI-LEFT-2 PP10), cut out of the fill below
      for (const h of s.holes ?? []) {
        h.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
        ctx.closePath();
      }
      break;
    case "bezier": {
      // `EDA_SHAPE::RebuildBezierToSegmentsPointsList( m_MaxError )`: the curve as KiCad's own flattened polyline (an open curve is never filled).
      bezierPolyline(s.start, s.c1, s.c2, s.end, BEZIER_MAX_ERROR_NM).forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
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
  if (s.filled && !sketch) ctx.fill(s.kind === "polygon" && s.holes?.length ? "evenodd" : "nonzero");
  ctx.stroke();
  drawHatchLines(ctx, s);
}

/** The path of a rectangle (its corners rounded by its radius), a circle or a polygon, as `drawShapeGeometry` traces it. */
function traceClosedShape(ctx: CanvasRenderingContext2D, s: Extract<Shape, { kind: "rect" | "circle" | "polygon" }>) {
  if (s.kind === "rect") {
    const [x0, y0] = [Math.min(s.start[0], s.end[0]), Math.min(s.start[1], s.end[1])];
    const [w, h] = [Math.abs(s.end[0] - s.start[0]), Math.abs(s.end[1] - s.start[1])];
    const r = Math.min(s.corner_radius ?? 0, Math.min(w, h) / 2);
    if (r > 0) ctx.roundRect(x0, y0, w, h, r);
    else ctx.rect(s.start[0], s.start[1], s.end[0] - s.start[0], s.end[1] - s.start[1]);
  } else if (s.kind === "circle") {
    ctx.arc(s.center[0], s.center[1], Math.hypot(s.end[0] - s.center[0], s.end[1] - s.center[1]), 0, Math.PI * 2);
  } else {
    s.pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.closePath();
    for (const h of s.holes ?? []) {
      h.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.closePath();
    }
  }
}

/**
 * A line, an arc or a curve with line endings, as `PCB_PAINTER::draw( PCB_SHAPE )` draws it: `ShortenSegmentForEndings` / `ShortenArcForEndings` / `ShortenedBezierCurve` take off what
 * the endings cover (nothing is drawn of a body they cover whole), then each ending is drawn at its end (`LINE_ENDING::Draw`): filled, and stroked at its own outline width when it has
 * one; an open arrow is a polyline stroked at its outline width or the line's. The pen colour is the one the caller set.
 */
function drawShapeWithEndings(ctx: CanvasRenderingContext2D, s: Extract<Shape, { kind: "segment" | "arc" | "bezier" }>, dash?: DashSpec) {
  const lw = s.stroke_width;
  ctx.beginPath();
  if (dash) {
    // The shortened body takes the line style (`STROKE_PARAMS::Stroke` of its effective shape), the endings stay solid.
    const pieces: DashPiece[] = [];
    if (s.kind === "segment") {
      const body = shortenSegment(s.start, s.end, s.start_ending, s.end_ending, lw);
      if (body) strokeSegment(body[0], body[1], dash.style, dash.width, dash.ratios, pieces);
    } else if (s.kind === "arc") {
      const k = kicadArc(s.start, s.mid, s.end);
      if (k) {
        const r = Math.hypot(k.start[0] - k.centre[0], k.start[1] - k.centre[1]);
        const a0 = (Math.atan2(k.start[1] - k.centre[1], k.start[0] - k.centre[0]) * 180) / Math.PI;
        let a1 = (Math.atan2(k.end[1] - k.centre[1], k.end[0] - k.centre[0]) * 180) / Math.PI;
        if (a1 === a0) a1 = a0 + 360;
        while (a1 < a0) a1 += 360;
        const body = shortenArc(a0, a1 - a0, r, s.start_ending, s.end_ending, lw);
        if (body) {
          const from: [number, number] = [Math.round(k.centre[0] + r * Math.cos((body[0] * Math.PI) / 180)), Math.round(k.centre[1] + r * Math.sin((body[0] * Math.PI) / 180))];
          strokeArc([Math.round(k.centre[0]), Math.round(k.centre[1])], from, body[1], dash.style, dash.width, dash.ratios, pieces);
        }
      }
    } else {
      const body = shortenBezier({ start: s.start, c1: s.c1, c2: s.c2, end: s.end }, s.start_ending, s.end_ending, lw);
      if (body) {
        const pts = bezierPolyline(body.start, body.c1, body.c2, body.end, BEZIER_MAX_ERROR_NM);
        for (let i = 1; i < pts.length; i++) strokeSegment(pts[i - 1]!, pts[i]!, dash.style, dash.width, dash.ratios, pieces);
      }
    }
    strokeDashPieces(ctx, pieces);
  } else if (s.kind === "segment") {
    const body = shortenSegment(s.start, s.end, s.start_ending, s.end_ending, lw);
    if (body) {
      ctx.moveTo(body[0][0], body[0][1]);
      ctx.lineTo(body[1][0], body[1][1]);
      ctx.stroke();
    }
  } else if (s.kind === "arc") {
    const k = kicadArc(s.start, s.mid, s.end);
    if (k) {
      const r = Math.hypot(k.start[0] - k.centre[0], k.start[1] - k.centre[1]);
      const a0 = (Math.atan2(k.start[1] - k.centre[1], k.start[0] - k.centre[0]) * 180) / Math.PI;
      let a1 = (Math.atan2(k.end[1] - k.centre[1], k.end[0] - k.centre[0]) * 180) / Math.PI;
      if (a1 === a0) a1 = a0 + 360;
      while (a1 < a0) a1 += 360;
      const body = shortenArc(a0, a1 - a0, r, s.start_ending, s.end_ending, lw);
      if (body) {
        const rad = Math.PI / 180;
        ctx.arc(k.centre[0], k.centre[1], r, body[0] * rad, (body[0] + body[1]) * rad, body[1] < 0);
        ctx.stroke();
      }
    }
  } else {
    const body = shortenBezier({ start: s.start, c1: s.c1, c2: s.c2, end: s.end }, s.start_ending, s.end_ending, lw);
    if (body) {
      bezierPolyline(body.start, body.c1, body.c2, body.end, BEZIER_MAX_ERROR_NM).forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.stroke();
    }
  }
  // `LINE_ENDING::Draw`: the ending is always solid (only the body takes the line style) and the GAL strokes its polylines and polygon outlines with round joins and caps.
  ctx.save();
  ctx.fillStyle = ctx.strokeStyle;
  ctx.setLineDash([]);
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  for (const ink of endingInks(s)) {
    ctx.beginPath();
    ink.pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    if (ink.fill) {
      ctx.closePath();
      ctx.fill();
      if (ink.stroke > 0) {
        ctx.lineWidth = ink.stroke;
        ctx.stroke();
      }
    } else {
      // `SetLineWidth( m_stroke.GetWidth() > 0 ? m_stroke.GetWidth() : aLineWidth )`: an open arrow's own pen, thinner than the line or not.
      if (ink.stroke > 0) ctx.lineWidth = ink.stroke;
      ctx.stroke();
    }
  }
  ctx.restore();
}

/**
 * The lines of a hatched rectangle, circle or polygon (`PCB_PAINTER::draw( PCB_SHAPE )`: `UpdateHatching`, then `DrawLine` for each of `GetHatchLines()` with the line width the
 * shape's own). The server works the lines out from the board as it is -- which is what `rehatchShapes`, run after every commit, comes to -- and the state carries them; the pen keeps the
 * stroke's colour and has flat ends.
 */
export function drawHatchLines(ctx: CanvasRenderingContext2D, s: Shape) {
  const lines = s.hatch_lines;
  if (!lines || lines.length === 0) return;
  ctx.save();
  ctx.lineCap = "butt";
  // The line width of the shape, and no thinner than the hairline the outline is kept visible with at a low zoom (`drawShapes` set it).
  ctx.lineWidth = Math.max(s.hatch_width ?? s.stroke_width, ctx.lineWidth);
  ctx.beginPath();
  for (const [x1, y1, x2, y2] of lines) {
    ctx.moveTo(x1, y1);
    ctx.lineTo(x2, y2);
  }
  ctx.stroke();
  ctx.restore();
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

/**
 * Free-standing board text (Place > Text): `PCB_PAINTER::draw( const PCB_TEXT* )` -- `FONT::Draw` from the text's position with its attributes (kicad-port/textSpecs.ts
 * `boardTextSpec`: the width apart from the height, the vertical justification, bold's pen, italic, the face), in the stroke font or the outline face (UI-LEFT-2 PP1, CC1);
 * a knockout text is its box filled, the text cut out of it (`GetKnockoutCache`).
 */
function drawTexts(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions) {
  const texts = board.drawings?.texts ?? [];
  for (const t of texts) {
    if (!layerIsVisible(opts.layerVisible, t.layer)) continue;
    const selected = opts.selection.has(t.id);
    const color = selected ? layerColor("selection") : layerColor(realLayerKey(t.layer));
    const { spec, face } = boardTextSpec(t);
    withBoardLayer(ctx, opts, t.layer, () =>
      paintKicadText(ctx, spec, face, {
        color,
        // "Sketch Text Items" (`m_DisplayTextFill` off): the text in line mode, one pixel wide.
        pen: opts.sketchText ? hairlineNm(view, 1) : undefined,
        minPen: hairlineNm(view, 0.5),
        knockout: t.knockout ? layerColor("background") : null,
        flippedView: keptReadableInFlippedView(opts.viewFlipped, t.layer, opts.layerTypes),
        record: { tag: "pcb", id: t.id },
      })
    );
  }
}

/**
 * Task item 7: every `lines` segment (extension lines, crossbar/leader
 * pieces, arrow barbs, a centre cross -- already fully computed server-
 * side, `eda_connectivity::dimension::compute_dimension_geometry`) plus
 * the formatted `text` at `text_at`/`computed_text_angle`. That angle is KiCad's text angle, counter-clockwise on the screen
 * (`PCB_DIM_ALIGNED::updateText`: `FULL_CIRCLE - EDA_ANGLE( crossbarCenter )`, `ANGLE_VERTICAL` for a vertical crossbar), so it is negated
 * for the canvas as `drawTexts` negates a text's: a vertical dimension reads bottom to top (round 3 A-4).
 */
export function drawDimensions(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions) {
  // The tool's or the point editor's preview (`DRAWING_TOOL::DrawDimension`'s `preview`, `PCB_POINT_EDITOR`'s live item) in place of the dimension it is.
  // A dimension the tool is drawing has no id yet and is drawn by `drawInProgress`.
  const preview = opts.dimensionPreview;
  const committed = (board.drawings?.dimensions ?? []) as Dimension[];
  const dimensions = preview?.id ? committed.map((d) => (d.id === preview.id ? { ...preview.dim, id: d.id } : d)) : committed;
  for (const d of dimensions) {
    if (!layerIsVisible(opts.layerVisible, d.layer)) continue;
    const selected = opts.selection.has(d.id);
    const color = selected ? layerColor("selection") : layerColor(realLayerKey(d.layer));
    withBoardLayer(ctx, opts, d.layer, () => drawOneDimension(ctx, view, d, selected, color, opts));
  }
}

/** One dimension: its lines and circles, then its text. */
function drawOneDimension(ctx: CanvasRenderingContext2D, view: ViewTransform, d: Dimension, selected: boolean, color: string, opts: PaintOptions) {
  ctx.save();
  ctx.strokeStyle = color;
  ctx.lineWidth = Math.max(d.stroke_width, hairlineNm(view, selected ? 2 : 1));
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.beginPath();
  for (const [[ax, ay], [bx, by]] of d.lines) {
    ctx.moveTo(ax, ay);
    ctx.lineTo(bx, by);
  }
  // A leader's circle frame (`DIM_TEXT_BORDER::CIRCLE`: a `SHAPE_CIRCLE` of the dimension, stroked at its line width).
  for (const [[cx, cy], r] of d.circles ?? []) {
    ctx.moveTo(cx + r, cy);
    ctx.arc(cx, cy, r, 0, Math.PI * 2);
  }
  ctx.stroke();
  ctx.restore();

  if (d.text) {
    // `PCB_PAINTER::draw( const PCB_DIMENSION_BASE* )`: `strokeText( resolvedText, GetTextPos(), attrs )`, the label's own pen (kicad-port/textSpecs.ts `dimensionTextSpec`)
    const { spec, face } = dimensionTextSpec(d);
    paintKicadText(ctx, spec, face, {
      color,
      pen: opts.sketchText ? hairlineNm(view, 1) : undefined,
      minPen: hairlineNm(view, 0.5),
      // `if( m_gal->IsFlippedX() && !aDimension->IsSideSpecific() ) attrs.m_Mirrored = !attrs.m_Mirrored;`, the text where it is
      flippedView: keptReadableInFlippedView(opts.viewFlipped, d.layer, opts.layerTypes),
      keepAnchor: true,
      record: { tag: "pcb", id: d.id },
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
function drawRatsnest(ctx: CanvasRenderingContext2D, view: ViewTransform, edges: MovedEdge<RatsnestEdge>[], curved: boolean, netHighlight: string | readonly string[] | null, colorOf?: (net: string) => string | null, thicknessPx = 0.5) {
  // `SetLineWidth( m_RatsnestThickness / GetWorldScale() )`: 0.5 px by default, drawn at the GAL's one-pixel minimum (`u_minLinePixelWidth`), every line the same.
  const hair = hairlineNm(view, Math.max(1, thicknessPx));
  const background = layerColor("background");
  const ratsnest = layerColor("ratsnest");
  // `dimStatic = m_data->GetLocalRatsnest().size() > 0 || highlightedNets.size() > 0`: while something moves or a net is highlighted, the static lines fade.
  const highlighting = typeof netHighlight === "string" ? netHighlight !== "" : (netHighlight?.length ?? 0) > 0;
  const dimStatic = highlighting || edges.some((e) => e.dynamic);
  const colours = new Map<string, string>();
  for (const e of edges) {
    const dynamic = !!e.dynamic;
    const lit = !dynamic && isHighlighted(e.net, netHighlight);
    // A net's own colour (or its net class's) replaces the ratsnest colour unless the net colour mode is "None"; then the line's state (kicad-port/color4d.ts).
    const base = colorOf?.(e.net) ?? ratsnest;
    const memo = `${base}|${dynamic}|${lit}`;
    let stroke = colours.get(memo);
    if (stroke === undefined) {
      stroke = ratsnestLineColor(base, background, { dynamic, highlighted: lit, dimStatic });
      colours.set(memo, stroke);
    }
    ctx.strokeStyle = stroke;
    ctx.lineWidth = hair;
    const [ax, ay] = e.from;
    const [bx, by] = e.to;
    ctx.beginPath();
    // `if( source == target )`: a cross, so a connection whose ends coincide still shows (r7 C7-7).
    const cross = ratsnestCross(e.from, e.to);
    if (cross) {
      for (const [[x0, y0], [x1, y1]] of cross) {
        ctx.moveTo(x0, y0);
        ctx.lineTo(x1, y1);
      }
      ctx.stroke();
      continue;
    }
    ctx.moveTo(ax, ay);
    if (curved) {
      // pcbnew.Control.ratsnestLineMode ("Curved Ratsnest Lines"): `DrawCurve( source, center, center, target )`, the dynamic lines' too (r7 C7-8).
      const [cx, cy] = ratsnestCurveControl(e.from, e.to);
      ctx.bezierCurveTo(cx, cy, cx, cy, bx, by);
    } else {
      ctx.lineTo(bx, by);
    }
    ctx.stroke();
  }
}

/**
 * INACTIVE since UI-LEFT C2 (task-u4-common): the grid is `paintEditorGrid` (components/canvas/gridPaint.ts, kicad-port/grid.ts `gridMarks`), DrawGrid's three styles
 * with the thickness and minimum spacing of Display Options and a grid whose X and Y differ. Kept, not called.
 *
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
export function drawGrid(ctx: CanvasRenderingContext2D, view: ViewTransform, widthPx: number, heightPx: number, gridNm: number, origin: readonly [number, number] = [0, 0], covered: readonly GridGeometry[] = []) {
  const visible = computeVisibleGridSize(gridNm, view.scale, DEFAULT_GRID_STYLE);
  const stepPx = visible * view.scale;
  if (!(stepPx > 0)) return; // gridNm <= 0 or a degenerate scale -- nothing sane to draw
  // The grid is anchored at the grid origin (`GAL::SetGridOrigin`): dots at `origin + i * visible`.
  const x0 = -view.x / view.scale - origin[0];
  const y0 = -view.y / view.scale - origin[1];
  const wNm = widthPx / view.scale;
  const hNm = heightPx / view.scale;
  const firstIndexX = Math.floor(x0 / visible) - 1;
  const firstIndexY = Math.floor(y0 / visible) - 1;
  const lastIndexX = Math.ceil((x0 + wNm) / visible) + 1;
  const lastIndexY = Math.ceil((y0 + hNm) / visible) + 1;
  ctx.fillStyle = layerColor("grid");
  const minorR = hairlineNm(view, stepPx < 8 ? 0.6 : 1);
  const majorR = minorR * MAJOR_GRID_LINE_WIDTH_RATIO;
  for (let i = firstIndexX; i <= lastIndexX; i++) {
    const x = origin[0] + i * visible;
    const tickX = isMajorGridLine(i);
    for (let j = firstIndexY; j <= lastIndexY; j++) {
      const y = origin[1] + j * visible;
      // A grid item's coverage hides the editor grid (`drawGridSource`'s wash): see gridItemPaint.ts.
      if (covered.length > 0 && gridItemCovers(covered, x, y)) continue;
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
  const r = hairlineNm(view, 16);
  ctx.save();
  ctx.strokeStyle = originMarkerColor(layerColor("grid"), background);
  ctx.lineWidth = hairlineNm(view, 1);
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
  ctx.lineWidth = Math.max(width, hairlineNm(view, 1));
  ctx.setLineDash([hairlineNm(view, 4), hairlineNm(view, 3)]);
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.beginPath();
  pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
  if (closed && pts.length >= 2) ctx.closePath();
  ctx.stroke();
  ctx.setLineDash([]);
  for (const [x, y] of pts) {
    ctx.beginPath();
    ctx.arc(x, y, hairlineNm(view, 2.5), 0, Math.PI * 2);
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
  ctx.lineWidth = hairlineNm(view, 1.5);
  ctx.setLineDash([hairlineNm(view, 3), hairlineNm(view, 2)]);
  ctx.beginPath();
  ctx.arc(x, y, diameter / 2, 0, Math.PI * 2);
  ctx.stroke();
  ctx.setLineDash([]);
  ctx.restore();
}

function drawInProgress(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions, viewport: readonly [number, number, number, number]) {
  const draw = opts.drawState;
  if (!draw) return;

  if (draw.kind === "tune") {
    drawTune(ctx, view, draw, opts.cursorNm ? [opts.cursorNm.x, opts.cursorNm.y] : null, opts.units);
    return;
  }

  // `PlaceText`: the text the dialog made follows the cursor (the snapped one) until a click places it.
  if (draw.kind === "text") {
    const at = opts.snappedCursor ?? (opts.cursorNm ? snapPoint(opts.cursorNm.x, opts.cursorNm.y, opts.gridNm) : null);
    if (at) {
      const t = draw.text;
      // laid out as it will be once placed (kicad-port/textSpecs.ts `boardTextSpec`), with the looks the dialog gave it
      const { spec } = boardTextSpec({ id: "", content: t.content, x: at[0], y: at[1], angle: t.angle, layer: t.layer, size: t.size_nm, stroke_width: t.stroke_width, justify: t.justify, mirror: t.mirror, width: draw.extra.width_nm ?? null, italic: draw.extra.italic, valign: draw.extra.valign === "center" ? null : draw.extra.valign });
      paintKicadText(ctx, spec, null, { color: layerColor(realLayerKey(t.layer)), minPen: hairlineNm(view, 0.5) });
    }
    return;
  }

  if (draw.kind === "route") {
    const violationColor = "#ff3333";
    for (const run of draw.runs ?? []) strokeDashedPolyline(ctx, view, run.pts, layerColor(copperColorKey(run.layer)), draw.width, false);
    strokeDashedPolyline(ctx, view, draw.pts, draw.colliding ? violationColor : layerColor(copperColorKey(draw.layer)), draw.width, false);
    for (const d of draw.displaced ?? []) strokeDashedPolyline(ctx, view, d.pts, "#ffaa33", draw.width, false);
    for (const v of draw.displacedVias ?? []) {
      const src = board.routing?.vias.find((via) => via.id === v.source_via);
      // a board via's preview at its widest diameter (`ROUTER_PREVIEW_ITEM`: the layer of the `largestDiameter`)
      const d = src ? viaMaxDiameter(src) : draw.width;
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
      // a virtual via (a junction of tracks) is not drawn: `ROUTER_PREVIEW_ITEM` returns for `v->IsVirtual()`
      if (!draw.virtualVia) strokeViaGhost(ctx, view, x, y, draw.viaDiameter ?? 600_000, color);
      for (const leg of draw.fanout ?? []) strokeDashedPolyline(ctx, view, leg.pts, layerColor(copperColorKey(leg.layer)), leg.width, false);
    } else if (draw.dragKind === "component" || draw.dragKind === "multi") {
      // `COMPONENT_DRAGGER`: the lines that follow the pads (the footprints themselves are drawn moved by the move preview); `MULTI_DRAGGER`: the dragged lines
      for (const leg of draw.fanout ?? []) strokeDashedPolyline(ctx, view, leg.pts, draw.colliding ? violationColor : layerColor(copperColorKey(leg.layer)), leg.width, false);
    } else {
      strokeDashedPolyline(ctx, view, draw.pts, color, draw.width, false);
    }
    for (const d of draw.displaced ?? []) strokeDashedPolyline(ctx, view, d.pts, "#ffaa33", draw.width || 150, false);
    for (const v of draw.displacedVias ?? []) {
      const src = board.routing?.vias.find((via) => via.id === v.source_via);
      const d = src ? viaMaxDiameter(src) : 600_000;
      strokeViaGhost(ctx, view, v.x, v.y, d, "#ffaa33");
    }
    return;
  }

  if (draw.kind === "diffpair") {
    // A collision on *either* line colors *both*: the pair is one unit to the user, though each line is checked on its own. The head is
    // already coupled, walked or shoved by the router (crates/pns/src/dp_placer.rs); `displaced` is what Shove would push aside.
    const violationColor = "#ff3333";
    const color = draw.colliding ? violationColor : layerColor(copperColorKey(draw.layer));
    for (const run of draw.runsA ?? []) strokeDashedPolyline(ctx, view, run.pts, layerColor(copperColorKey(run.layer)), draw.width, false);
    for (const run of draw.runsB ?? []) strokeDashedPolyline(ctx, view, run.pts, layerColor(copperColorKey(run.layer)), draw.width, false);
    strokeDashedPolyline(ctx, view, draw.ptsA, color, draw.width, false);
    strokeDashedPolyline(ctx, view, draw.ptsB, color, draw.width, false);
    for (const d of draw.displaced ?? []) strokeDashedPolyline(ctx, view, d.pts, "#ffaa33", draw.width || 150, false);
    for (const v of draw.displacedVias ?? []) {
      const src = board.routing?.vias.find((via) => via.id === v.source_via);
      const d = src ? viaMaxDiameter(src) : 600_000;
      strokeViaGhost(ctx, view, v.x, v.y, d, "#ffaa33");
    }
    for (const v of draw.fixedVias ?? []) strokeViaGhost(ctx, view, v.x, v.y, v.diameter, layerColor("via"));
    for (const v of draw.vias ?? []) strokeViaGhost(ctx, view, v.x, v.y, v.diameter, draw.colliding ? violationColor : layerColor("via"));
    return;
  }

  // `drawArc` / `drawOneBezier`: the construction managers' own geometry, not a polyline through the clicks.
  if (draw.kind === "shape" && draw.shapeKind === "arc" && draw.arc) {
    drawArcPreview(ctx, draw.arc, { color: layerColor("selection"), hair: (px) => hairlineNm(view, px), units: opts.units });
    return;
  }
  if (draw.kind === "shape" && draw.shapeKind === "bezier" && draw.bezier) {
    drawBezierPreview(ctx, draw.bezier, { color: layerColor("selection"), hair: (px) => hairlineNm(view, px), units: opts.units }, BEZIER_MAX_ERROR_NM);
    return;
  }

  // `S` (the schematic's sheet tool) and the schematic's shape tools draw their own rubber band in SchematicView.tsx -- never reaches the board painter.
  if (draw.kind === "sheet" || draw.kind === "sch_shape") return;

  // `DRAWING_TOOL::DrawTable`: the grid the cursor sizes from the first corner.
  // `DrawDimension`'s preview: the dimension as the server works it out for the step the tool is at (`POST /api/dimension/preview`), in its layer's colour.
  if (draw.kind === "dimension" && draw.dim) {
    const p = opts.dimensionPreview;
    if (p && !p.id) drawDimensions(ctx, view, { ...board, drawings: { ...board.drawings!, dimensions: [p.dim] } } as BoardState, { ...opts, dimensionPreview: null, selection: new Set() });
    return;
  }
  if (draw.kind === "table") {
    drawTablePreview(ctx, view, board, draw.origin, opts);
    return;
  }

  const cursor = opts.cursorNm;
  const pts = draw.pts.slice();
  // A finished measurement (2 points already fixed) is a static ruler --
  // it stops following the cursor, unlike every other click-to-add-points
  // tool here, which always rubber-bands toward wherever the *next* point
  // would land.
  const frozen = draw.kind === "measure" && pts.length >= 2;
  let rubberEnd: [number, number] | null = null;
  if (cursor && !frozen) {
    const last = pts[pts.length - 1]!;
    const twoPoint = draw.kind === "shape" && (draw.shapeKind === "segment" || draw.shapeKind === "rect" || draw.shapeKind === "circle") ? draw.shapeKind : null;
    // The snapped cursor (the grid helper's), or the raw cursor put on the grid; then `drawShape`'s angle constraint, which keeps it on the grid.
    const target: [number, number] = opts.snappedCursor ? [opts.snappedCursor[0], opts.snappedCursor[1]] : snapPoint(cursor.x, cursor.y, opts.gridNm);
    rubberEnd = twoPoint ? shapeSecondPoint(twoPoint, opts.angleSnapMode ?? "direct", opts.drawCtrl ?? false, last, target) : target;
  }
  const color = layerColor("selection");
  // `DrawZone`'s leader (a zone, a rule area, a graphic polygon): bent in the 45- and 90-degree line modes, as the click will lock it in (kicad-port/pcbDrawZone.ts).
  const polygonDraw = draw.kind === "zone" || (draw.kind === "shape" && draw.shapeKind === "polygon");
  const leader = polygonDraw && rubberEnd && pts.length > 0 ? zoneLeader(pts, rubberEnd, leaderModeOf(opts.angleSnapMode, opts.drawCtrl ?? false)) : null;
  // `m_loopPts`: in the 45- and 90-degree modes the outline closes from the cursor back to the first corner along a bent path, as finishing will make it
  // (ZONE_CREATE_HELPER::OnComplete, round 5 A5-5); in the free mode a zone's preview closes straight.
  const loop = polygonDraw && rubberEnd && pts.length > 1 ? zoneLoop(pts, rubberEnd, leaderModeOf(opts.angleSnapMode, opts.drawCtrl ?? false)) : [];
  const shown: [number, number][] = leader ? [...pts, ...leader.slice(1), ...loop.slice(1, -1)] : rubberEnd ? [...pts, rubberEnd] : pts;
  // pcb_viewer_tools.cpp's measure tool: `RULER_ITEM` from the first click to the cursor, fixed at the second (kicad-port/rulerItem.ts, components/canvas/rulerPaint.ts)
  if (draw.kind === "measure") {
    const end = frozen ? pts[1]! : rubberEnd;
    // Interactive Offset's ruler (`arrow`): the anchor colour, no ticks, an arrowhead (`POSITION_RELATIVE_TOOL::InteractiveOffset`)
    if (end) paintRuler(ctx, rulerDrawing(pts[0]!, end, { scale: view.scale, viewport }, { units: opts.units, showTicks: !draw.arrow, showEndArrowHead: draw.arrow === true, flipX: opts.rulerFlip?.x, flipY: opts.rulerFlip?.y }, measureStrokeText), layerColor(draw.arrow ? "anchor" : "LAYER_AUX_ITEMS"), "pcb");
    return;
  }
  strokeDashedPolyline(ctx, view, shown, color, hairlineNm(view, 1.5), (draw.kind === "zone" || loop.length > 0) && shown.length >= 2);
}

/** A ghost circle at the snapped cursor for the standalone via tool -- via.ts's placement is a single click, so there's no multi-point drawState to show, just "a via would land here". */
function drawViaGhost(ctx: CanvasRenderingContext2D, board: BoardState, opts: PaintOptions) {
  if (!opts.cursorNm) return;
  const [x, y] = opts.viaGhostAt ?? opts.snappedCursor ?? snapPoint(opts.cursorNm.x, opts.cursorNm.y, opts.gridNm);
  const d = opts.currentViaPreset?.diameter ?? board.board_rules?.via_diameter ?? 600_000;
  ctx.save();
  ctx.globalAlpha = 0.6;
  ctx.fillStyle = layerColor("via");
  ctx.beginPath();
  ctx.arc(x, y, d / 2, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();
}

/** Radius, um, of a DRC marker's circle -- not a KiCad constant (real KiCad's MARKER_BASE is a small fixed-pixel icon drawn in screen space, independent of zoom; this app has no screen-space-constant-size drawing primitive, so markers scale with the board like everything else here, sized to read clearly at a normal working zoom). */
const DRC_MARKER_RADIUS_NM = 300_000;

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
  const hair = hairlineNm(view, 1.5);
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
    const r = on ? DRC_MARKER_RADIUS_NM * 1.4 : DRC_MARKER_RADIUS_NM;
    ctx.save();
    ctx.fillStyle = color;
    ctx.strokeStyle = color;
    ctx.lineWidth = Math.max(100_000, hair);
    // Out of date: the same marker, dimmed and dashed -- where the problem was, not necessarily where it is now.
    if (stale) ctx.setLineDash([r * 0.35, r * 0.25]);
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    ctx.globalAlpha = stale ? 0.1 : waived && !on ? 0.18 : 0.35;
    ctx.fill();
    ctx.globalAlpha = stale ? 0.55 : 1;
    ctx.stroke();
    ctx.setLineDash([]);
    drawStrokeText(ctx, "!", x, y + r * 0.5, { sizeNm: r * 1.3, justify: "center", color, thicknessNm: r * 0.22 });
    ctx.restore();
  });
}

/** Our own lint findings (crates/lint), drawn apart from kicad-cli's DRC circles: a blue diamond around the finding's first item, bigger and filled when the dialog's Lint tab has it selected. */
const LINT_MARKER_COLOR = "#4ea1ff";

function drawLintMarkers(ctx: CanvasRenderingContext2D, view: ViewTransform, findings: DrcViolation[], selected: number | null) {
  const hair = hairlineNm(view, 1.5);
  findings.forEach((v, i) => {
    const item = v.items[0];
    if (!item) return;
    const [x, y] = item.pos;
    const on = i === selected;
    const r = on ? DRC_MARKER_RADIUS_NM * 1.5 : DRC_MARKER_RADIUS_NM * 1.1;
    ctx.save();
    ctx.strokeStyle = LINT_MARKER_COLOR;
    ctx.fillStyle = LINT_MARKER_COLOR;
    ctx.lineWidth = Math.max(100_000, hair);
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

/** The GAL layers of a footprint's pads that are not a copper layer's: the holes, the net names and the selection's outline (`drawPads`). */
const PAD_GAL_LAYERS = new Set(["LAYER_PAD_HOLEWALLS", "LAYER_PAD_PLATEDHOLES", "LAYER_NON_PLATEDHOLES", "LAYER_PAD_NETNAMES", "LAYER_PAD_FR_NETNAMES", "LAYER_PAD_BK_NETNAMES", "LAYER_SELECT_OVERLAY"]);

/** The board layers something of `board` is on, once per board state: the passes of the other layers are skipped (a pass walks every item). */
const usedLayersCache = new WeakMap<object, Set<string>>();
function layersInUse(board: BoardState): Set<string> {
  const cached = usedLayersCache.get(board);
  if (cached) return cached;
  const used = new Set<string>(["Edge.Cuts"]);
  const seen = new WeakSet<object>();
  // every `layer` and `layers` of the items, whatever their kind; arrays of numbers (points) are not walked
  const walk = (v: unknown, depth: number): void => {
    if (depth > 8 || v === null || typeof v !== "object" || seen.has(v as object)) return;
    seen.add(v as object);
    if (Array.isArray(v)) {
      if (v.length > 0 && typeof v[0] !== "object") {
        if (typeof v[0] === "string") for (const x of v) if (typeof x === "string" && /^[A-Za-z0-9]+\.[A-Za-z0-9]+$/.test(x)) used.add(x);
        return;
      }
      for (const x of v) walk(x, depth + 1);
      return;
    }
    for (const [k, x] of Object.entries(v as Record<string, unknown>)) {
      if (k === "layer" && typeof x === "string") used.add(x);
      else if (typeof x === "object") walk(x, depth + 1);
    }
  };
  walk(board.drawings, 0);
  walk(board.routing, 0);
  walk(board.parts, 0);
  // a footprint's courtyard box is on its side's courtyard layer
  for (const part of board.parts) if (part.placed && part.courtyard) used.add(part.side === "bottom" ? "B.CrtYd" : "F.CrtYd");
  usedLayersCache.set(board, used);
  return used;
}

/** Whether a pass can draw anything: a layer nothing is on is skipped; the pads' and vias' copper passes run on the board's copper layers. */
function passHasWork(pass: GalPass, used: ReadonlySet<string>, copper: readonly string[] | undefined): boolean {
  if (pass.kind === "gal") return pass.layer === "LAYER_VIA_HOLES" || pass.layer === "LAYER_VIA_HOLEWALLS" || PAD_GAL_LAYERS.has(pass.layer);
  if (pass.kind === "netnames" || pass.kind === "clearance") return false;
  if (pass.kind === "pad" || pass.kind === "via") return (copper ?? ["F.Cu", "B.Cu"]).includes(pass.layer);
  return used.has(pass.layer);
}

/**
 * One layer's pass over the items of `b` (`opts.galPass`, UI-LEFT-2 PP3): the items on that layer of the kind the pass is -- the zones of a zone pass, the
 * points of a point pass, the images of an image pass, the pads' or vias' copper on that copper layer, the holes and net names of theirs, and in a layer's own
 * pass its tracks, footprints' graphics, texts, fields and courtyards, and the board's graphics, texts, tables, drill marks, dimensions, text boxes and
 * barcodes. What is not copper is drawn in `withUnlit` (the net highlight darkens it); copper paints its own colours. `extraFields` are fields carried on their own.
 */
function paintPass(ctx: CanvasRenderingContext2D, view: ViewTransform, b: BoardState, o: PaintOptions, withUnlit: (draw: () => void) => void, extraFields: readonly FieldInfo[] = []): void {
  const pass = o.galPass!;
  switch (pass.kind) {
    case "bitmap":
      drawRefImages(ctx, view, b, o);
      return;
    case "zone":
      if (isCopper(pass.layer)) drawZones(ctx, view, b, o, pass.layer);
      else withUnlit(() => drawNonCopperZones(ctx, view, b, o));
      return;
    case "point":
      withUnlit(() => {
        drawPoints(ctx, view, b, o);
        for (const part of b.parts) drawFootprint(ctx, view, part, o);
      });
      return;
    case "pad":
      for (const part of b.parts) drawFootprint(ctx, view, part, o);
      return;
    case "via":
      drawViaCopper(ctx, view, b, o, pass.layer);
      return;
    case "gal":
      if (pass.layer === "LAYER_VIA_HOLEWALLS") drawViaHoles(ctx, view, b, o, "walls");
      else if (pass.layer === "LAYER_VIA_HOLES") drawViaHoles(ctx, view, b, o, "holes");
      else if (PAD_GAL_LAYERS.has(pass.layer)) for (const part of b.parts) drawFootprint(ctx, view, part, o);
      return;
    case "item":
      if (isCopper(pass.layer)) drawTracks(ctx, view, b, o, pass.layer);
      withUnlit(() => {
        // the outline's polygon when the Edge.Cuts shapes are not the outline themselves (then they are drawn as the shapes they are)
        if (pass.layer === "Edge.Cuts" && layerIsVisible(o.layerVisible, "Edge.Cuts") && !b.outline_is_shapes) withBoardLayer(ctx, o, "Edge.Cuts", () => drawOutline(ctx, view, b.outline));
        for (const part of b.parts) drawFootprint(ctx, view, part, o);
        drawFields(ctx, view, [...fieldsToDraw(b.parts), ...extraFields.map((field) => ({ field }))], o);
        drawShapes(ctx, view, b, o);
        drawTexts(ctx, view, b, o);
        drawTables(ctx, view, b, o);
        drawDrillItems(ctx, view, b, o.layerVisible, o.selection, pass.layer);
        drawDimensions(ctx, view, b, o);
        drawPcbItems(ctx, view, b, o);
      });
      return;
    default:
      // a copper layer's track net names and clearance outlines: not drawn by the studio
      return;
  }
}

/**
 * Paints the whole board into `ctx`, which must already have `view`
 * applied (ctx.translate/scale) -- see Canvas.tsx. Iterates GAL layers in
 * `drawOrder()` so moving to WebGL later only means replacing the
 * per-layer draw calls, not this ordering.
 */
export function paintBoard(ctx: CanvasRenderingContext2D, view: ViewTransform, widthPx: number, heightPx: number, fullBoard: BoardState, given: PaintOptions) {
  // High Contrast Mode's Dim and the net highlight work on the items' colours, as `PCB_RENDER_SETTINGS::GetColor` does: the colours set on the context go
  // through the scopes the draws open (colourScope.ts).
  installColourScopes(ctx);
  // what the texts of this paint are, for `window.__edaText.pcb` (components/text/kicadTextPaint.ts)
  beginTextRecord("pcb");
  const opts: PaintOptions = { ...given, dimByColour: true };
  // The net highlight darkens every item that is not of a highlighted net (`GetColor`'s single-net highlight branch: an item that is not a connected copper
  // item has net -1, so the silkscreen, the outline and the texts darken too); a selected item keeps its colour (`!selected`). The copper items are
  // `copperPaint`'s; everything else is drawn in this scope.
  const highlighting = typeof opts.netHighlight === "string" ? opts.netHighlight !== "" : (opts.netHighlight?.length ?? 0) > 0;
  const selectionColour = layerColor("selection");
  const unlit = highlighting ? memoColours(`unlit|${selectionColour}`, (css: string) => (css === selectionColour ? css : highlightDarken(css))) : null;
  const withUnlit = (draw: () => void) => (unlit ? withColourScope(ctx, unlit, draw) : draw());
  // The layers in `PCB_DRAW_PANEL_GAL`'s order (`GAL_LAYER_ORDER`, `SetTopLayer` of the active layer, which `SetActiveLayer` calls whatever the contrast mode:
  // kicad-port/galLayerOrder.ts), back to front, each a pass of the painter (UI-LEFT-2 PP3). KiCad's frame always has an active layer; none here stands for F.Cu.
  const order = galDrawOrder(opts.activeLayer ? layerNameOfKey(opts.activeLayer) : "F.Cu");
  opts.copperLayers = fullBoard.layers.filter(isCopper);
  // A PCB selection in the hand (kicad-port/pcbCarry.ts): the rest of the board is painted as it is, the carried items afterwards through the carry transform.
  const preview = opts.movePreview?.kind === "pcb" ? opts.movePreview : null;
  const carry = preview ? splitCarried(fullBoard, preview.refs) : null;
  const board = carry ? carry.still : fullBoard;
  // The group being worked in: what is inside it is drawn as it is, everything else dimmed (`m_enteredGroupOverlay` puts the group in front; the dim is this
  // port's own cue that nothing outside it can be picked). The split puts the group's items in `moving`, the rest in `still`.
  const entered = opts.enteredGroup ? splitCarried(board, [opts.enteredGroup]) : null;
  const passes: { b: BoardState; alpha: number }[] = entered ? [{ b: entered.still, alpha: OUTSIDE_ENTERED_GROUP_ALPHA }, { b: entered.moving, alpha: 1 }] : [{ b: board, alpha: 1 }];
  const used = layersInUse(fullBoard);
  // The editor grid lies under everything (`EDA_DRAW_PANEL_GAL::DoRePaint` draws it before `m_view->Redraw()`); the axes of the board's window
  // (`grid.axes_enabled`) are drawn whether the grid is shown or not. A grid item's coverage hides the editor grid (`drawGridSource`'s wash: gridItemPaint.ts).
  const gridItems = shownGridItems(board, opts.layerVisible);
  {
    const d = opts.gridDisplay ?? DEFAULT_GRID_DISPLAY;
    if (opts.gridVisible || d.axes) {
      const covered = gridItems.map(gridGeometry);
      paintEditorGrid(ctx, view, widthPx, heightPx, { size: { x: opts.gridNm, y: opts.gridYNm ?? opts.gridNm }, origin: opts.gridOrigin ?? [0, 0], style: d.style, lineWidthPx: d.lineWidthPx, minSpacingPx: d.minSpacingPx, visible: opts.gridVisible, axes: d.axes === true }, layerColor("grid"), covered.length > 0 ? (x, y) => gridItemCovers(covered, x, y) : undefined, "pcb", layerColor("grid_axes"));
    }
  }
  // The DRC markers darken with the net highlight as any item of no net does, but the one selected (`LAYER_DRC_HIGHLIGHTED`, returned before the highlight)
  const drcHighlighted = layerColor("LAYER_DRC_HIGHLIGHTED");
  const markerUnlit = highlighting ? memoColours(`unlit-markers|${drcHighlighted}`, (css: string) => (css === drcHighlighted ? css : highlightDarken(css))) : null;
  const special: Record<string, () => void> = {
    LAYER_DRAWINGSHEET: () => drawSheet(ctx, view, opts),
    LAYER_BOARD_OUTLINE_AREA: () => drawBoardArea(ctx, board, opts),
    // KiCad 10's grid items (`LAYER_SUBGRIDS`)
    LAYER_SUBGRIDS: () => {
      const box: [number, number, number, number] = [-view.x / view.scale, -view.y / view.scale, (widthPx - view.x) / view.scale, (heightPx - view.y) / view.scale];
      drawGridItems(ctx, view, board, opts.layerVisible, opts.selection, DEFAULT_GRID_STYLE, box);
    },
    // pcb_actions.cpp updateLocalRatsnest's non-router equivalent: the airwires redrawn live from a moving footprint's previewed position (kicad-port/localRatsnest.ts).
    LAYER_RATSNEST: () => {
      if (!opts.showRatsnest || !opts.ratsnestEdges) return;
      const edges = carry && preview ? carryRatsnest(opts.ratsnestEdges, fullBoard, carry, (pt) => carryPoint(preview, pt)) : offsetRatsnestForPreview(opts.ratsnestEdges, fullBoard, opts.movePreview);
      drawRatsnest(ctx, view, edges, opts.ratsnestCurved, opts.netHighlight, (net) => ratsnestColor(net, opts.appearance), opts.ratsnestThicknessPx);
    },
    LAYER_ANCHOR: () => drawAnchors(ctx, view, board, opts),
    // KiCad 10's via stitching outlines (`LAYER_VIA_STITCHING`)
    LAYER_VIA_STITCHING: () => drawViaStitches(ctx, view, board, opts.layerVisible, opts.selection),
    // `LAYER_LOCKED_ITEM_SHADOW`: its colour is returned before the net highlight (`GetColor`), so it is never darkened
    LAYER_LOCKED_ITEM_SHADOW: () => drawLockedShadows(ctx, board, opts),
    // `LAYER_CONFLICTS_SHADOW`: a footprint's colour there, darkened by the net highlight as an item of no net
    LAYER_CONFLICTS_SHADOW: () => {
      if (opts.showCourtyardCollisions !== false) withUnlit(() => drawConflicts(ctx, fullBoard, opts.movePreview, opts));
    },
    LAYER_DRC_ERROR: () => {
      if (!opts.drcViolations) return;
      const draw = () => drawDrcMarkers(ctx, view, opts.drcViolations!, opts.drcSelected, opts.drcStale, opts.layerVisible);
      if (markerUnlit) withColourScope(ctx, markerUnlit, draw);
      else draw();
    },
  };
  for (const gal of order) {
    const own = special[gal];
    if (own) {
      own();
      continue;
    }
    const pass = galPassOf(gal);
    if (!passHasWork(pass, used, opts.copperLayers)) continue;
    for (const p of passes) withAlpha(ctx, p.alpha, () => paintPass(ctx, view, p.b, { ...opts, galPass: pass }, withUnlit));
  }
  // The carried items, layer by layer as the board's, through the transform the drop would apply.
  if (carry && preview) {
    const own: PaintOptions = { ...opts, movePreview: null };
    const [a, b, c, d, e, f] = carryMatrix(preview);
    ctx.save();
    ctx.transform(a, b, c, d, e, f);
    const movingUsed = layersInUse(carry.moving);
    const carried = { ...carry.moving, parts: carry.moving.parts };
    for (const gal of order) {
      if (gal === "LAYER_SUBGRIDS") drawGridItems(ctx, view, carry.moving, own.layerVisible, own.selection, DEFAULT_GRID_STYLE, [-Infinity, -Infinity, Infinity, Infinity]);
      else if (gal === "LAYER_ANCHOR") drawAnchors(ctx, view, carry.moving, own);
      else if (gal === "LAYER_VIA_STITCHING") drawViaStitches(ctx, view, carry.moving, own.layerVisible, own.selection);
      else if (!special[gal]) {
        const pass = galPassOf(gal);
        if (passHasWork(pass, movingUsed, opts.copperLayers)) paintPass(ctx, view, carried, { ...own, galPass: pass }, withUnlit, carry.fields);
      }
    }
    drawSelectedGroups(ctx, view, carry.moving, own);
    ctx.restore();
  }
  // The items the clarification menu points at (`doSelectionMenu`: `highlight( current, BRIGHTENED, &highlightGroup )`, UI-LEFT-2 PP19): drawn again over the board,
  // every colour they set brightened (`GetColor`: `color.Brightened( m_selectFactor ).WithAlpha( 0.8 )`), as the highlight group is drawn in the overlay.
  if (opts.brightened && opts.brightened.length > 0) {
    const lit = brightenedBoard(fullBoard, opts.brightened);
    const own: PaintOptions = { ...opts, selection: new Set(), hot: new Set(), movePreview: null, netHighlight: null };
    const litUsed = layersInUse(lit.board);
    withFinalColourScope(ctx, memoColours("brightened", brightenedCss), () => {
      for (const gal of order) {
        if (special[gal]) continue;
        const pass = galPassOf(gal);
        if (passHasWork(pass, litUsed, opts.copperLayers)) paintPass(ctx, view, lit.board, { ...own, galPass: pass }, (draw) => draw(), lit.fields);
      }
    });
  }
  if (opts.auxOrigin) drawAuxOrigin(ctx, view, opts.auxOrigin);
  drawGridOrigin(ctx, view, opts.gridOrigin, layerColor("background"));
  // The tools' rubber bands of pcbItemsPaint.ts (the image in the hand ..), on `LAYER_GP_OVERLAY` over every layer (UI-LEFT-2 PP3: the layer passes leave them out).
  drawPcbToolPreviews(ctx, view, opts);
  // In-progress route/drag/via/zone/drawing tool preview, on top of everything committed.
  drawInProgress(ctx, view, board, opts, [-view.x / view.scale, -view.y / view.scale, (widthPx - view.x) / view.scale, (heightPx - view.y) / view.scale]);
  if (opts.activeTool === "tune" && !opts.drawState && opts.tuneHover) drawTuneHover(ctx, view, opts.tuneHover, opts.units);
  if (opts.activeTool === "via") drawViaGhost(ctx, board, opts);
  // Import Graphics' placement: the items that would be placed, where the cursor holds them (nothing of them is on the board yet).
  if (opts.importPreview) {
    const p = opts.importPreview;
    const ghost = { ...board, drawings: { ...(board.drawings ?? {}), shapes: p.shapes, texts: p.texts } } as BoardState;
    ctx.save();
    ctx.translate(p.dx, p.dy);
    drawShapes(ctx, view, ghost, { ...opts, selection: new Set(), shapePointPreview: null });
    drawTexts(ctx, view, ghost, { ...opts, selection: new Set() });
    ctx.restore();
  }
  // pcb_point_editor.cpp: a single selected zone's outline corners are
  // draggable handles, shown only in the plain Select tool (same as
  // source only activating the point editor over the selection tool).
  if (opts.activeTool === "select") drawZoneHandles(ctx, view, board, opts);
  if (opts.activeTool === "select") drawShapeHandles(ctx, view, board, opts);
  if (opts.activeTool === "select") drawDimensionHandles(ctx, view, board, opts);
  // Task item 5: a selected group's own bounding box, same tier as the
  // zone corner handles above (a selection-mode indicator, not board
  // content).
  if (opts.activeTool === "select") drawSelectedGroups(ctx, view, board, opts);
  // A selected tuning pattern shows its baseline, its outline and its four edit points (`PCB_TUNING_PATTERN::ViewDraw`).
  if (opts.activeTool === "select") drawSelectedPatterns(ctx, view, board, opts.selection);
  // the studio's lint findings (crates/lint), over everything
  if (opts.lintViolations) drawLintMarkers(ctx, view, opts.lintViolations, opts.lintSelected ?? null);
}

/**
 * `ORIGIN_VIEWITEM( KIGFX::COLOR4D( 0.8, 0.0, 0.0, 1.0 ), CIRCLE_CROSS )` at the drill/place file origin (`BOARD_EDITOR_CONTROL`'s `m_placeOrigin`):
 * a one-pixel red circle and cross, 16 pixels out, not drawn while the origin is at (0, 0).
 */
function drawAuxOrigin(ctx: CanvasRenderingContext2D, view: ViewTransform, at: [number, number]) {
  if (at[0] === 0 && at[1] === 0) return;
  const r = hairlineNm(view, 16);
  ctx.save();
  ctx.strokeStyle = "rgb(204, 0, 0)";
  ctx.lineWidth = hairlineNm(view, 1);
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
    const pad = hairlineNm(view, 12);
    const [x0, y0, w, h] = [box[0] - pad, box[1] - pad, box[2] - box[0] + pad * 2, box[3] - box[1] + pad * 2];
    ctx.save();
    ctx.strokeStyle = entered ? layerColor("anchor") : layerColor("selection");
    ctx.lineWidth = hairlineNm(view, entered ? 2.5 : 2);
    if (!entered) ctx.setLineDash([hairlineNm(view, 8), hairlineNm(view, 5)]);
    ctx.strokeRect(x0, y0, w, h);
    ctx.setLineDash([]);
    // The name, in KiCad's tab: two lines up and across, the text between them and the box ("Scale by zoom a bit, but not too much").
    const name = (g.name ?? "").trim();
    if (name !== "") {
      const textNm = (hairlineNm(view, 14) + 2 * 304_800) / 3;
      if (name.length * textNm * 0.6 < w) {
        const tab = textNm * 2;
        ctx.beginPath();
        ctx.moveTo(x0, y0);
        ctx.lineTo(x0, y0 - tab);
        ctx.lineTo(x0 + w, y0 - tab);
        ctx.lineTo(x0 + w, y0);
        ctx.stroke();
        ctx.fillStyle = ctx.strokeStyle;
        ctx.font = `italic ${textNm}px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`;
        ctx.textAlign = "center";
        ctx.textBaseline = "alphabetic";
        ctx.fillText(name, x0 + w / 2, y0 - textNm * 0.5);
      }
    }
    ctx.restore();
  }
}

/**
 * pcb_point_editor.cpp's handles for a single selected graphic shape (kicad-port/pcbPointEdit.ts): a square at each point, a smaller
 * diamond at the middle of a polygon's or rectangle's edges; unlocked shapes only. Follows the drag preview while one is live.
 */
/** `PCB_POINT_EDITOR`'s handles of a single selected dimension that is not locked (kicad-port/dimensionDraw.ts `dimensionEditPoints`), at the dimension the drag has. */
function drawDimensionHandles(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions) {
  if (opts.selection.size !== 1) return;
  const id = [...opts.selection][0]!;
  if ((board.locked ?? []).includes(id)) return;
  const d = opts.dimensionPreview?.id === id ? { ...opts.dimensionPreview.dim, id } : (board.drawings?.dimensions as Dimension[] | undefined)?.find((x) => x.id === id);
  if (!d) return;
  const r = hairlineNm(view, 4);
  ctx.save();
  ctx.fillStyle = layerColor("selection");
  ctx.strokeStyle = "rgba(0,0,0,0.6)";
  ctx.lineWidth = hairlineNm(view, 1);
  for (const p of dimensionEditPoints(d)) {
    ctx.beginPath();
    ctx.rect(p.pos[0] - r, p.pos[1] - r, r * 2, r * 2);
    ctx.fill();
    ctx.stroke();
  }
  ctx.restore();
}

function drawShapeHandles(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: PaintOptions) {
  if (opts.selection.size !== 1) return;
  const id = [...opts.selection][0]!;
  if ((board.locked ?? []).includes(id)) return;
  const shape = opts.shapePointPreview?.id === id ? opts.shapePointPreview.shape : board.drawings?.shapes.find((s) => s.id === id);
  if (!shape) return;
  const r = hairlineNm(view, 4);
  ctx.save();
  ctx.fillStyle = layerColor("selection");
  ctx.strokeStyle = "rgba(0,0,0,0.6)";
  ctx.lineWidth = hairlineNm(view, 1);
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
 * small squares at each vertex of its outline and holes, filled solid (selection color) so
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
  // A handle on every corner of every contour, the holes' too (`POLYGON_POINT_EDIT_BEHAVIOR::BuildForPolyOutline` over `CIterateWithHoles`).
  const preview = opts.zoneCornerPreview?.zoneId === id ? opts.zoneCornerPreview : null;
  const corners = preview ? [...preview.outline, ...(preview.holes ?? []).flat()] : flatVertices(zoneContours(zone));
  const r = hairlineNm(view, 4);
  ctx.save();
  ctx.fillStyle = layerColor("selection");
  ctx.strokeStyle = "rgba(0,0,0,0.6)";
  ctx.lineWidth = hairlineNm(view, 1);
  for (const [x, y] of corners) {
    ctx.beginPath();
    ctx.rect(x - r, y - r, r * 2, r * 2);
    ctx.fill();
    ctx.stroke();
  }
  ctx.restore();
}
