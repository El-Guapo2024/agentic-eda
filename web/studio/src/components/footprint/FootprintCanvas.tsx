// The Footprint Editor's own canvas (GAPS.md #8) -- pcbnew's footprint
// editor frame, one footprint drawn in isolation at its own anchor.
// Reuses the PCB tab's ported view/selection/grid-snap logic directly
// (kicad-port/viewControls.ts, gridHelper.ts, selection.ts) -- it is "the
// same canvas with different tools" exactly as the task describes, just a
// much smaller tool set than Canvas.tsx's (no routing/zones/vias, no
// box-select disambiguation menu, no auto-pan) since a footprint has no
// board-scale content to need them for.
import { useGridDisplay } from "../../state/commonOptions";
import { gridYOf } from "../../kicad-port/gridSettings";
import React, { useCallback, useEffect, useRef, useState } from "react";
import type { CmdShape, Dimension, LibraryPad, PadKind } from "../../api/types";
import { FP_ITEM_TOOLS, useFpApi, useFpDispatch, useFpState, type FpToolId } from "../../state/footprintEditorStore";
import { boundsOfPoints, fitTransform, screenToWorld } from "../../kicad-port/view";
import { paintFootprint } from "./footprintPainter";
import { useFootprintChecker } from "../../state/footprintChecker";
import { handleWheel, leftDragOutcome } from "../../kicad-port/viewControls";
import { wheelInputOf } from "../../hooks/canvasWheel";
import { useLibraryViewControls } from "../libraryViewControls";
import { SelectionMarquee, type Marquee } from "../SelectionMarquee";
import { rectSelectionHits } from "../../kicad-port/selectRect";
import { footprintItemBoxes } from "../../kicad-port/itemBoxes";
import { useWheelPrefs } from "../../actions/useWheelPrefs";
import { useNonPassiveWheel } from "../../hooks/useNonPassiveWheel";
import { useActionRunner } from "../../actions/useActionRunner";
import { picker } from "../../actions/pcbPicker";
import { isMac } from "../../platform";
import { ClickDragGesture, dragRuleFor } from "../../kicad-port/dragThreshold";
import { computeClickModifiers, applySingleClickModifier, applyBoxSelectionModifiers, hasModifier, isCrossingSelection, type ClickModifiers } from "../../kicad-port/selection";
import { nextPadNumber } from "../../kicad-port/padNumbering";
import { padCanHaveNumber } from "../../kicad-port/fpEditActions";
import { padEditMenu, padToolsBlocked } from "../../kicad-port/padEditMode";
import { newPadFromMaster, settingsOf } from "../../kicad-port/padSettings";
import { enumerateHit, enumerateLastNumber, enumerateShownNumber, padsUnderSweep, sweepPoints } from "../../kicad-port/padEnumeration";
import { padContains } from "../../kicad-port/libEditorHit";
import { padBox } from "../../kicad-port/itemBoxes";
import { bezierPolyline } from "../../kicad-port/bezierPoly";
import { arcClick, arcMotion, arcRemoveLastPoint } from "../../kicad-port/arcGeom";
import { bezierClick, bezierFinishDouble, bezierMotion, bezierRemoveLastPoint } from "../../kicad-port/bezierGeom";
import { arcAngleSnap, arcClickPoints, bezierShape, ptXY } from "../canvas/curveTools";
import { fpShape, fpStrokeWidthNm, type FpPointShapeKind } from "../../kicad-port/fpDrawDefaults";
import { getFpGraphicsDefaults } from "../../state/fpPrefs";
import { FOOTPRINT_EDITOR_MAGNETIC } from "../../kicad-port/fpSnapScene";
import { fpNewText } from "../../kicad-port/fpTextTool";
import { flipCarriedPad } from "../../kicad-port/fpPadPlacer";
import { useStudioState } from "../../state/store";
import { ContextMenu, type MenuEntry } from "../canvas/ContextMenu";
import { useFootprintGridOrigin } from "../../state/gridOrigin";
import { useGridSettings } from "../../state/gridSettings";
import { useGridOverrides } from "../../state/gridOverrides";
import { gridSizeFor, selectionGrid, type GridCategory } from "../../kicad-port/gridOverrides";
import { heldCursor } from "../../kicad-port/heldCursor";
import { FootprintSnap, type FootprintPointOpts, type FootprintSnapMods } from "./footprintSnap";
import { paintSnapOverlay } from "../canvas/snapOverlayPainter";
import { layerColor } from "../canvas/layers";
import { fpPointAt, fpPointDrag, fpPointEditTarget } from "../../kicad-port/fpPointEdit";
import { shapeEditPoints, type EditPoint } from "../../kicad-port/pcbPointEdit";
import type { Shape } from "../../api/types";
import { fpEditTarget, fpFields, fpGroupMembers, fpHitTest, fpItemBounds, fpItemKind, fpItemLocked, fpSelectionTarget, fpCanMirror, type FpEditTarget } from "../../kicad-port/fpItems";
import { fpPassesFilter } from "../../kicad-port/fpAppearance";
import { shapeSecondPoint } from "../../kicad-port/pcbShapeSnap";
import { leaderModeOf, zoneClick, zoneFinish, zoneLeader } from "../../kicad-port/pcbDrawZone";
import { isGroup } from "../../kicad-port/groupTree";
import { fpItemToolClick, fpItemToolFinish, fpItemToolMotion, fpItemToolSnap, paintFpItemToolPreview } from "./fpItemTools";
import { paintingIn, useColorThemeStamp } from "../../state/colorThemeStore";
import "../../styles/canvas.css";
import { planAddToGroup, planRemoveFromGroup } from "../../kicad-port/groupEdit";
import { hairlineNm } from "../../kicad-port/view";
import { fetchDimensionPreview } from "../../api/dimensionPreview";

const SHAPE_TOOL_KIND: Partial<Record<FpToolId, "segment" | "arc" | "rect" | "circle" | "polygon" | "bezier">> = {
  draw_segment: "segment",
  draw_arc: "arc",
  draw_bezier: "bezier",
  draw_rect: "rect",
  draw_circle: "circle",
  draw_polygon: "polygon",
};

/** Layers a through-hole pad's own default should occupy -- not reachable by the Pad tool directly (it always starts from the SMD master above, same as source), but `padTemplateForKind` keeps this ready for the Pad Properties dialog's own type-preset buttons (step 3) to reuse. */
export function padLayersForKind(kind: PadKind): string[] {
  switch (kind) {
    case "through_hole":
      return ["*.Cu", "*.Mask"];
    case "non_plated_hole":
      return ["*.Cu", "*.Mask"];
    case "smd":
    default:
      return ["F.Cu", "F.Paste", "F.Mask"];
  }
}

function shapePoints(s: CmdShape): [number, number][] {
  switch (s.kind) {
    case "segment":
    case "rect":
      return [
        [s.start.x, s.start.y],
        [s.end.x, s.end.y],
      ];
    case "circle":
      return [
        [s.center.x, s.center.y],
        [s.end.x, s.end.y],
      ];
    case "arc":
      return [
        [s.start.x, s.start.y],
        [s.mid.x, s.mid.y],
        [s.end.x, s.end.y],
      ];
    case "polygon":
      return s.pts.map((p) => [p.x, p.y]);
    case "bezier":
      return bezierPolyline([s.start.x, s.start.y], [s.c1.x, s.c1.y], [s.c2.x, s.c2.y], [s.end.x, s.end.y], 5);
  }
}

/**
 * Where the rubber band of the drawing tool in force ends with the cursor at `cursor`, under the footprint editor's line mode (`FOOTPRINT_EDITOR_SETTINGS::m_AngleSnapMode`):
 * a line's second point is `drawShape`'s (`GetVectorSnapped45` / `90`, kicad-port/pcbShapeSnap.ts), a polygon's leader `DrawZone`'s (`POLYGON_GEOM_MANAGER`, kicad-port/pcbDrawZone.ts).
 * Ctrl frees a line and constrains a rectangle or a circle, as on the board; the canvas reads it at the last pointer event.
 */
let lineModeCtrl = false;
function rubberEndFor(mode: "direct" | "45" | "90", draw: { shapeKind: string; pts: [number, number][] }, cursor: [number, number]): { end: [number, number]; leader?: [number, number][] } {
  const last = draw.pts[draw.pts.length - 1];
  if (!last) return { end: cursor };
  if (draw.shapeKind === "segment" || draw.shapeKind === "rect" || draw.shapeKind === "circle") return { end: shapeSecondPoint(draw.shapeKind, mode, lineModeCtrl, last, cursor) };
  if (draw.shapeKind === "polygon") {
    const leader = zoneLeader(draw.pts, cursor, leaderModeOf(mode, lineModeCtrl));
    return { end: leader[leader.length - 1] ?? cursor, leader };
  }
  return { end: cursor };
}

type DragState =
  | { kind: "move"; refs: string[]; moveKind: string; startWorld: [number, number]; snapOrigin: [number, number] }
  /** `PCB_SELECTION_TOOL::SelectRectArea`: the selection rectangle from where the press was; `click` is the item the press was on, selected as a click when the release comes
   * without a drag ("Left button drag:" sent the drag to the rectangle). */
  | { kind: "box"; startWorld: [number, number]; startScreen: [number, number]; modifiers: ClickModifiers; click: string | null }
  /** `PCB_POINT_EDITOR` on the one selected graphic (kicad-port/fpPointEdit.ts): the edit point pressed, the shape as it was at the press (UI-LEFT-2 FF7). */
  | { kind: "point"; id: string; base: Shape; point: EditPoint };

/** The shape a point-counted tool commits: the layer class's width and, for a polygon, KiCad's fill rule (kicad-port/fpDrawDefaults.ts). */
function footprintToShapeArg(kind: "segment" | "arc" | "rect" | "circle" | "polygon" | "bezier", pts: [number, number][], layer: string): CmdShape | null {
  return kind === "bezier" ? null : fpShape(kind as FpPointShapeKind, pts, layer, getFpGraphicsDefaults()); // a Bezier is built from the construction manager's `BezierCurve`, never from a bare point list
}

// `polygon` has no fixed point count (like the PCB tab's own AddShape
// polygon tool) -- left out of this table entirely, so `pts.length >=
// AUTO_FINISH[shapeKind]` (`undefined` for a missing key) is always
// false and placement only ever ends on an explicit Enter/double-click
// (`finishDraw`), same convention zones/polygons use everywhere else in
// this app.
// (An arc and a Bezier are not point-counted either: kicad-port/arcGeom.ts / bezierGeom.ts decide when they are complete.)
const AUTO_FINISH: Partial<Record<"segment" | "rect" | "circle" | "arc" | "polygon" | "bezier", number>> = { segment: 2, rect: 2, circle: 2 };

export function FootprintCanvas() {
  const state = useFpState();
  const dispatch = useFpDispatch();
  const api = useFpApi();
  const gridOrigin = useFootprintGridOrigin();
  // Grid overrides (`common.Control.toggleGridOverrides`): a pad on the connected-items grid, a shape on the graphics grid, text on the text grid -- `GetItemGrid`.
  const gridSettingsList = useGridSettings("footprint");
  const gridList = gridSettingsList.grids;
  // the editor's own Display Options > Grid Display, and the Y of its grid (Edit Grids' "Y:")
  const gridDisplay = useGridDisplay("footprint");
  const gridYNm = gridYOf(gridSettingsList, state.gridNm);
  const gridOverrides = useGridOverrides("footprint");
  const gridOf = (c: GridCategory): number => gridSizeFor(c, state.gridNm, gridList, gridOverrides);
  /** `GetSelectionGrid`: the category of the coarsest grid of what is held. */
  const heldCategory = (ids: readonly string[]): GridCategory => {
    // `GetItemGrid`: a pad on the connected-items grid, a text or a field on the text grid, every other item on the graphics grid.
    const f = state.footprint;
    const cats = ids.map((id): GridCategory => {
      const kind = f ? fpItemKind(f, id) : null;
      return kind === "pad" ? "connectable" : kind === "text" || kind === "field" ? "text" : "graphics";
    });
    return selectionGrid(cats, gridOf);
  };
  // The grid helper of the tool in force (kicad-port/pcbGridHelper.ts, footprintSnap.ts), kept fed with the view, the grid, the overrides and the open footprint's items: a click goes
  // to the grid, or to an anchor of another item -- a pad's centre and outline, a graphic's end points, middle and centre, the footprint's origin -- like `PCB_GRID_HELPER::BestSnapAnchor`.
  const snapRef = useRef<FootprintSnap | null>(null);
  if (!snapRef.current) snapRef.current = new FootprintSnap();
  const snap = snapRef.current;
  const [snapTick, bumpSnap] = useState(0);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);
  /** The pressed button's `BUTTON_STATE` (tool_dispatcher.cpp): whether the press has become a drag. Made at every press. */
  const gestureRef = useRef<ClickDragGesture | null>(null);
  const { run, isEnabled } = useActionRunner();
  /** `EnumeratePads`' `oldMousePos`: where the mouse was at the last event, so a fast move can be swept for pads in between. */
  const lastMouseRef = useRef<{ x: number; y: number } | null>(null);
  /** The previous press of the Renumber Pads tool: a second press at the same place right after is a double click (finish), not another click. */
  const lastDownRef = useRef<{ t: number; x: number; y: number } | null>(null);
  const dblSuppressUntil = useRef(0);
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 });
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  const wheelPrefs = useWheelPrefs();
  const userMovedRef = useRef(false);
  const [marquee, setMarquee] = useState<Marquee | null>(null);
  const studioPrefs = useStudioState().prefs;
  /** The graphic as the point editor's drag has it so far (drawn in place of the graphic until the drop commits it). */
  const [pointPreview, setPointPreview] = useState<CmdShape | null>(null);
  // The dimension Draw Dimension holds as the server works it out (`POST /api/dimension/preview`, as the board's tool asks): the preview is the dimension itself, its
  // arrows and text included (`DrawDimension`'s `m_view->AddToPreview`, UI-LEFT-2 FF8). Only the latest request's reply is kept; it goes with the tool's dimension.
  const [dimPreview, setDimPreview] = useState<Dimension | null>(null);
  const dimPreviewSeq = useRef(0);
  const dimHeld = state.itemDraw?.tool === "dimension" ? state.itemDraw.draw.dim : null;
  const dimHeldKey = dimHeld ? JSON.stringify(dimHeld) : "";
  useEffect(() => {
    const seq = ++dimPreviewSeq.current;
    if (!dimHeld) {
      setDimPreview(null);
      return;
    }
    void fetchDimensionPreview(dimHeld).then((d) => {
      if (seq === dimPreviewSeq.current) setDimPreview(d);
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dimHeldKey]);
  // The middle and right drags, the pan on movement with a key and the auto-pan (`WX_VIEW_CONTROLS`), as the board and the schematic have them.
  const viewCtl = useLibraryViewControls({
    containerRef,
    view: state.view,
    setView: (view) => dispatch({ type: "SET_VIEW", view }),
    settings: wheelPrefs.settings,
    zoomSpeed: studioPrefs.zoomSpeed,
    autoPan: studioPrefs.autoPan,
    autoPanAcceleration: studioPrefs.autoPanAcceleration,
    busy: () => dragRef.current?.kind === "box" || state.movePreview != null || state.drawState != null || state.itemDraw != null || state.carriedText != null,
    shown: true,
    onUserMoved: () => {
      userMovedRef.current = true;
    },
  });

  const fp = state.footprint;
  // Preferences > Footprint Editor > Editing Options' Magnetic Points (`m_MagneticItems.pads` CAPTURE_ALWAYS or NO_EFFECT, `.graphics`), UI-LEFT F8
  const magnetic = { ...FOOTPRINT_EDITOR_MAGNETIC, pads: studioPrefs.fpMagneticPads ? ("always" as const) : ("never" as const), graphics: studioPrefs.fpMagneticGraphics };
  snap.update({ footprint: fp, scale: state.view.scale, gridNm: state.gridNm, grids: gridList, gridYs: gridSettingsList.ys, overrides: gridOverrides, origin: [gridOrigin.x, gridOrigin.y], highContrast: state.highContrast, activeLayer: state.activeLayer, magnetic });
  snap.onChange = () => bumpSnap((n) => n + 1);
  // A tool of its own starts a helper of its own: what the last one remembered (the anchor snapped to, the snap line, the construction geometry) goes with it.
  useEffect(() => {
    snap.reset();
    bumpSnap((n) => n + 1);
  }, [snap, state.activeTool]);
  useEffect(() => () => snap.reset(), [snap]);
  const snapMods = (e: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }): FootprintSnapMods => ({ ctrl: isMac() ? e.metaKey : e.ctrlKey, shift: e.shiftKey });
  /** What each placing tool asks `BestSnapAnchor` for: the shape and text tools the active layer and their grid; the pad and anchor tools (and the pickers) every layer. */
  const toolOpts = (tool: string): FootprintPointOpts => {
    // the tools of the dimensions, rule areas, text boxes ... (components/footprint/fpItemTools.ts): each one's `BestSnapAnchor` layers and grid
    const item = fpItemToolSnap(tool as FpToolId);
    if (item) return item;
    if (tool.startsWith("draw_")) return { category: "graphics" };
    if (tool === "text") return { category: "text" };
    return { category: "current", layers: "all" };
  };
  /** The point a click of the tool in force goes to. */
  const toolPoint = (e: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }, wx: number, wy: number, tool: string = state.activeTool): [number, number] => {
    const p = snap.point(tool, [wx, wy], snapMods(e), toolOpts(tool));
    return [p[0], p[1]];
  };
  /** The pad the Pad tool carries, at `(x, y)` (`PAD_PLACER::CreateItem`): the default pad, or the one R / F turned or flipped, with the next number. */
  const carriedPadAt = (x: number, y: number): LibraryPad => {
    const base = state.carriedPad;
    const number = padCanHaveNumber(base ?? state.defaultPad) ? nextPadNumber(fp?.pads ?? [], state.lastPadNumber) : "";
    return base ? { ...base, number, at: { x, y } } : newPadFromMaster(state.defaultPad, number, { x, y });
  };
  const flipDirection = useStudioState().prefs.flipDirection;
  /** The tools whose clicks go through the grid helper: the marker and the rubber band follow them on every move. */
  const placing = state.activeTool === "pad" || state.activeTool === "text" || state.activeTool === "anchor" || state.activeTool.startsWith("draw_") || FP_ITEM_TOOLS.has(state.activeTool);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const ro = new ResizeObserver((entries) => {
      const box = entries[0]?.contentRect;
      if (box) setContainerSize({ width: box.width, height: box.height });
    });
    ro.observe(container);
    return () => ro.disconnect();
  }, []);

  // Fit to content once, the same "until the user pans/zooms by hand" rule
  // Canvas.tsx's own board-outline fit uses -- here the "outline" is every
  // pad's bounding box (plus a fallback box for a brand-new, empty
  // footprint, so the view still initializes to something sane).
  // (`fitName` is the footprint this view was last fitted for. Switching straight from one open footprint to another -- New
  // Footprint while one is open -- never passes through `fp == null`, so `fp != null` alone would not re-run this.)
  const fitNameRef = useRef<string | null>(null);
  const fitRequestRef = useRef(state.fitRequest);
  useEffect(() => {
    if (fitNameRef.current !== state.name) {
      fitNameRef.current = state.name;
      userMovedRef.current = false;
    }
    // Zoom to Fit (`common.Control.zoomFitScreen`): fit again even though the view was moved by hand.
    if (fitRequestRef.current !== state.fitRequest) {
      fitRequestRef.current = state.fitRequest;
      userMovedRef.current = false;
    }
    if (userMovedRef.current || containerSize.width < 50 || containerSize.height < 50) return;
    if (!fp) {
      // Nothing open: KiCad shows an empty canvas with its grid around the origin. 30 mm across, so the grid dots are readable at the default grid.
      if (!state.name) dispatch({ type: "SET_VIEW", view: fitTransform({ minX: -15_000_000, minY: -15_000_000, maxX: 15_000_000, maxY: 15_000_000 }, containerSize.width, containerSize.height) });
      return;
    }
    const pts: [number, number][] = [];
    for (const p of fp.pads) {
      const [w, h] = p.size;
      pts.push([p.at.x - w, p.at.y - h], [p.at.x + w, p.at.y + h]);
      // A custom pad reaches as far as its basic shapes.
      if (p.shape === "custom") {
        const [x0, y0, x1, y1] = padBox(p);
        pts.push([x0, y0], [x1, y1]);
      }
    }
    for (const g of fp.graphics) for (const [x, y] of shapePoints(g)) pts.push([x, y]);
    // a text by its box, not its anchor (`GetDocumentExtents` is the items' bounding boxes): a new footprint's REF** and value sit one above the other at
    // x 0, and their anchors alone made a box 0 wide -- Zoom to Fit then zoomed in without end (UI-LEFT-2 FF10's click-through)
    for (const t of fp.texts) {
      const h = t.size_nm || 1_000_000;
      const half = (Math.max(1, (t.content ?? "").length) * h * 0.9) / 2;
      pts.push([t.at.x - half, t.at.y - h / 2], [t.at.x + half, t.at.y + h / 2]);
    }
    if (pts.length === 0) pts.push([-3_000_000, -3_000_000], [3_000_000, 3_000_000]);
    const bounds = boundsOfPoints(pts);
    if (!bounds) return;
    dispatch({ type: "SET_VIEW", view: fitTransform(bounds, containerSize.width, containerSize.height) });
    dispatch({ type: "MARK_VIEW_INITIALIZED" });
    // Only once per footprint open, not on every pad edit -- see
    // state.name's own SET_NAME reset of viewInitialized.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fp != null, state.name, state.fitRequest, containerSize, dispatch]);

  useEffect(() => {
    userMovedRef.current = false;
  }, [state.name]);

  // The Footprint Checker's markers of this footprint (the board's `PCB_MARKER`s: they stay after the dialog is closed).
  const checker = useFootprintChecker();
  const checkMarkers = checker.name === state.name && checker.markers.length > 0 ? checker.markers.map((m, i) => ({ at: m.at as [number, number], severity: m.severity, selected: checker.selected === i })) : undefined;

  // Render loop, in the Footprint Editor's own colour theme (Preferences > Footprint Editor > Colors), redrawn when it changes.
  const themeStamp = useColorThemeStamp();
  useEffect(() => paintingIn("footprint", () => {
    const canvas = canvasRef.current;
    if (!canvas || containerSize.width === 0) return;
    const dpr = window.devicePixelRatio || 1;
    const { width, height } = containerSize;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.save();
    ctx.scale(dpr, dpr);
    // the Footprint Editor's theme's background (`LAYER_PCB_BACKGROUND` of `FOOTPRINT_EDIT_FRAME::GetColorSettings`), which High Contrast Mode mixes into (UI-LEFT-2 FF5)
    ctx.fillStyle = layerColor("background");
    ctx.fillRect(0, 0, width, height);
    ctx.save();
    ctx.translate(state.view.x, state.view.y);
    ctx.scale(state.view.scale || 1, state.view.scale || 1);
    // The Renumber Pads tool shows the numbers it has handed out (and highlights those pads, `pad->SetSelected()`) before anything is committed.
    const session = state.enumerate;
    let shown = fp && session ? { ...fp, pads: fp.pads.map((p) => ({ ...p, number: enumerateShownNumber(session, p.id ?? "", p.number) })) } : fp;
    // What a placing tool carries, drawn at the snapped cursor (`doInteractiveItemPlacement`'s preview, PlaceText's moving text): not items yet, so no id to pick.
    const carriedAt = snap.lastPoint() ?? (state.cursorNm ? [state.cursorNm.x, state.cursorNm.y] : null);
    if (shown && carriedAt && state.activeTool === "pad") shown = { ...shown, pads: [...shown.pads, { ...carriedPadAt(carriedAt[0], carriedAt[1]), id: undefined }] };
    if (shown && carriedAt && state.carriedText) shown = { ...shown, texts: [...shown.texts, { ...state.carriedText, id: undefined, at: { x: carriedAt[0], y: carriedAt[1] } }] };
    // The fields (`PCB_FIELD`s) and the dimensions, zones, text boxes ... of the footprint (kicad-port/fpItems.ts), drawn by the board's painters.
    if (shown && pointPreview) shown = { ...shown, graphics: shown.graphics.map((g) => (g.id === pointPreview.id ? pointPreview : g)) };
    const painted = shown && fp ? { ...shown, fieldsToDraw: fpFields(fp), display: fp.display } : shown;
    // the rubber band follows the cursor the grid helper gave (a snapped one), not the pointer -- and the line mode's constraint (`drawShape`, `DrawZone`)
    const rubber = placing && snap.lastPoint() ? { x: snap.lastPoint()![0], y: snap.lastPoint()![1] } : state.cursorNm;
    const draw = state.drawState;
    const constrained = draw && rubber && !draw.arc && !draw.bezier ? rubberEndFor(state.lineMode, draw, [rubber.x, rubber.y]) : null;
    paintFootprint(ctx, state.view, width, height, painted, {
      selection: session ? new Set([...state.selection, ...session.assigned.keys()]) : state.selection,
      gridNm: state.gridNm,
      gridVisible: state.gridVisible && state.layerVisible["obj:grid"] !== false,
      gridOrigin: [gridOrigin.x, gridOrigin.y],
      gridYNm,
      gridDisplay,
      drawState: draw && constrained?.leader ? { ...draw, leader: constrained.leader } : draw,
      cursorNm: constrained ? { x: constrained.end[0], y: constrained.end[1] } : rubber,
      movePreview: state.movePreview,
      highContrast: state.highContrast,
      contrastHidden: state.contrastHidden,
      activeLayer: state.activeLayer,
      markers: checkMarkers,
      sketchPads: state.sketchPads,
      sketchGraphics: state.sketchGraphics,
      sketchText: state.sketchText,
      layerVisible: state.layerVisible,
      layerOpacity: state.layerOpacity,
    });
    // The group being worked in (`m_enteredGroupOverlay`, `PCB_PAINTER::draw( const PCB_GROUP* )` of an entered group): its box, solid, in the anchor colour (UI-LEFT-2 FF8)
    const enteredBox = fp && state.enteredGroupId ? fpItemBounds(fp, state.enteredGroupId) : null;
    if (enteredBox) {
      const pad = hairlineNm(state.view, 12);
      ctx.save();
      ctx.strokeStyle = layerColor("anchor");
      ctx.lineWidth = hairlineNm(state.view, 2.5);
      ctx.strokeRect(enteredBox[0] - pad, enteredBox[1] - pad, enteredBox[2] - enteredBox[0] + 2 * pad, enteredBox[3] - enteredBox[1] + 2 * pad);
      ctx.restore();
    }
    // the point editor's edit points of the one selected graphic (`EDIT_POINTS::ViewDraw`), as the board draws a shape's handles
    const editTarget = shown && state.activeTool === "select" ? fpPointEditTarget(shown.graphics, state.selection) : null;
    if (editTarget) paintFpEditPoints(ctx, state.view.scale || 1, editTarget);
    // What the grid helper shows: the snap marker and its icon, the snap lines, the construction geometry.
    // what the dimension, table and reference image tools hold (components/footprint/fpItemTools.ts)
    paintFpItemToolPreview(ctx, state.view, state, rubber ? [rubber.x, rubber.y] : null, state.itemDraw?.tool === "dimension" ? dimPreview : null);
    const snapOverlay = snap.overlay();
    if (snapOverlay) {
      const k = state.view.scale || 1;
      paintSnapOverlay(ctx, k, [-state.view.x / k, -state.view.y / k, (width - state.view.x) / k, (height - state.view.y) / k], snapOverlay, { marker: layerColor("LAYER_AUX_ITEMS"), construction: layerColor("anchor"), guideActive: layerColor("anchor") });
    }
    ctx.restore();
    ctx.restore();
    // (`checkMarkers` is made afresh each render: it is its two inputs the loop follows)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }), [gridDisplay, gridYNm, fp, state.view, state.selection, state.gridNm, state.gridVisible, gridOrigin, state.drawState, state.cursorNm, state.movePreview, state.enumerate, state.highContrast, state.activeLayer, containerSize, checker.markers, checker.selected, checker.name, state.name, snapTick, state.activeTool, state.carriedPad, state.carriedText, state.defaultPad, state.lastPadNumber, state.sketchPads, state.sketchGraphics, state.sketchText, state.layerVisible, state.layerOpacity, state.lineMode, state.itemDraw, state.contrastHidden, pointPreview, themeStamp, dimPreview, state.enteredGroupId]);

  const worldAt = useCallback(
    (e: { clientX: number; clientY: number }): [number, number] => {
      const rect = containerRef.current!.getBoundingClientRect();
      return screenToWorld(state.view, e.clientX - rect.left, e.clientY - rect.top);
    },
    [state.view]
  );

  /**
   * Topmost item under a world point (kicad-port/fpItems.ts `fpHitTest`: pads, then fields and texts, the text boxes and the other items, dimensions, graphics, zones), and what a
   * click on it selects: the outermost group that holds it (`PCB_SELECTION_TOOL`'s group promotion; this editor enters no group).
   */
  const hitTest = useCallback(
    (wx: number, wy: number, entered?: string | null): { kind: string; id: string; item: string } | null => {
      if (!fp) return null;
      const tol = Math.max(80_000, 6 / state.view.scale);
      // what is hidden is not picked (`Selectable`), nor what the Selection Filter pane leaves out (`itemPassesFilter`, UI-LEFT-2 FF5)
      const hit = fpHitTest(fp, wx, wy, tol, { layerVisible: state.layerVisible, layerOpacity: state.layerOpacity, highContrast: state.highContrast, activeLayer: state.activeLayer, filter: studioPrefs.fpSelectionFilter });
      if (!hit) return null;
      const id = fpSelectionTarget(fp, hit.id, entered === undefined ? state.enteredGroupId : entered);
      if (id === null) return null; // outside the group being worked in (`WithinScope`)
      return { kind: id === hit.id ? hit.kind : "group", id, item: hit.id };
    },
    [fp, state.view.scale, state.layerVisible, state.layerOpacity, state.highContrast, state.activeLayer, state.enteredGroupId, studioPrefs.fpSelectionFilter]
  );

  /** One mouse event of the Renumber Pads tool: the numberable pads under the swept path take (or, on a click, give back) their number. */
  const enumerateAt = (wx: number, wy: number, isClick: boolean) => {
    const st = api.getState();
    const session = st.enumerate;
    const f = st.footprint;
    if (!session || !f) return;
    const points = sweepPoints(lastMouseRef.current, { x: wx, y: wy }); // "wxWidgets deliver mouse move events not frequently enough"
    lastMouseRef.current = { x: wx, y: wy };
    const hits = padsUnderSweep(
      f.pads.filter((p) => p.id && padCanHaveNumber(p)), // `pad->CanHaveNumber() && checkVisibility( pad )`
      points,
      (pad, x, y) => padContains(pad, x, y)
    );
    let next = session;
    let last: string | null = null;
    for (const pad of hits) {
      const after = enumerateHit(next, pad.id!, pad.number, isClick);
      last = enumerateLastNumber(next, after, pad.id!) ?? last; // `SetLastPadNumber`, as the tool runs
      next = after;
    }
    if (next !== session) dispatch({ type: "SET_ENUMERATE", state: next });
    if (last !== null) dispatch({ type: "SET_LAST_PAD_NUMBER", number: last });
  };

  const finishDraw = useCallback(() => {
    const draw = state.drawState;
    if (!draw) return;
    // A rule area's outline closes as a polygon's does; the text box's two corners take nothing from Enter (components/footprint/fpItemTools.ts).
    if (FP_ITEM_TOOLS.has(state.activeTool)) {
      fpItemToolFinish({ state, dispatch, api }, snap.lastPoint() ?? (state.cursorNm ? [state.cursorNm.x, state.cursorNm.y] : null), lineModeCtrl);
      return;
    }
    // An arc ends only when its three points are in (ARC_GEOM_MANAGER); Enter / a double-click leave it running.
    if (draw.shapeKind === "arc") return;
    if (draw.shapeKind === "bezier") {
      // A double-click: "use the current point for all remaining points", accept, and reset (no chaining).
      const at = snap.lastPoint() ?? draw.bezier?.lastPoint;
      const curve = draw.bezier && at ? bezierFinishDouble(draw.bezier, at) : null;
      if (curve) void api.addGraphic(bezierShape(curve, state.activeLayer, fpStrokeWidthNm(state.activeLayer, getFpGraphicsDefaults())));
      dispatch({ type: "SET_DRAW_STATE", draw: null });
      return;
    }
    // a polygon ends as `ZONE_CREATE_HELPER::OnComplete` makes it: the locked corners and the leader's (kicad-port/pcbDrawZone.ts `zoneFinish`)
    const at = snap.lastPoint() ?? (state.cursorNm ? ([state.cursorNm.x, state.cursorNm.y] as [number, number]) : null);
    const pts = draw.shapeKind === "polygon" && at ? (zoneFinish(draw.pts, at, leaderModeOf(state.lineMode, lineModeCtrl)) ?? draw.pts) : draw.pts;
    const shape = footprintToShapeArg(draw.shapeKind, pts, state.activeLayer);
    if (shape) void api.addGraphic(shape);
    dispatch({ type: "SET_DRAW_STATE", draw: null });
  }, [state.drawState, state.activeLayer, state.cursorNm, state.gridNm, state.lineMode, api, dispatch]);

  const onPointerDown = (e: React.PointerEvent) => {
    // Throws for a synthesized pointer (the Enter-key click of common.Control.cursorClick), which has no real pointer to capture.
    try {
      (e.target as Element).setPointerCapture(e.pointerId);
    } catch {
      /* synthetic pointer */
    }
    // tool_dispatcher.cpp's BUTTON_STATE for this press (kicad-port/dragThreshold.ts): is it a click or has it become a drag?
    const gesture = new ClickDragGesture(dragRuleFor(isMac()));
    gesture.down(e.clientX, e.clientY, e.timeStamp);
    gestureRef.current = gesture;
    const [wx, wy] = worldAt(e);
    setContextMenu(null);

    if (viewCtl.press(e)) return;
    if (e.button !== 0) return;

    const [sx, sy] = placing ? toolPoint(e, wx, wy) : [wx, wy];

    // PICKER_TOOL::Main: while a pick session runs (actions/pcbPicker.ts: Create Array's "Select Point..." / "Select Item...") a left click answers it, snapped to the
    // grid, or the item under it for an item session, and does nothing else.
    if (picker.session()) {
      const [px, py] = toolPoint(e, wx, wy, "picker");
      picker.click({ point: { x: px, y: py }, item: () => hitTest(wx, wy)?.id ?? null });
      return;
    }

    // `EDIT_TOOL::doMoveSelection` armed (Duplicate hands its copies straight to it): a click drops them where they are now.
    if (state.activeTool === "move" && state.duplicatePending) {
      const mp = state.movePreview;
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
      void api.commitMove(mp?.refs ?? [...state.selection], mp?.dxNm ?? 0, mp?.dyNm ?? 0);
      return;
    }

    // `PAD_TOOL::EnumeratePads`: every pad the cursor meets takes the next number; a double click finishes.
    if (state.activeTool === "enumerate" && state.enumerate && fp) {
      const now = performance.now();
      const last = lastDownRef.current;
      lastDownRef.current = { t: now, x: e.clientX, y: e.clientY };
      if (last && now - last.t < 350 && Math.hypot(e.clientX - last.x, e.clientY - last.y) < 6) {
        dblSuppressUntil.current = now + 600; // the dblclick event that follows must not open a pad's properties
        void api.finishEnumerate(true); // `evt->IsDblClick( BUT_LEFT )` -> `commit.Push( "Renumber Pads" )`
        return;
      }
      enumerateAt(wx, wy, true);
      return;
    }

    // `PAD_TOOL::PlacePad` (`PAD_PLACER::CreateItem` / `PlaceItem`): the new pad is the default pad (`m_Pad_Master`) with the next number
    // (none for a pad that cannot have one) -- as R / F turned or flipped it while it was carried (`carriedPad`) -- and the pad placed is pushed back into the default pad.
    if (state.activeTool === "pad" && fp) {
      const pad: LibraryPad = carriedPadAt(sx, sy);
      void api.addPad(pad);
      dispatch({ type: "SET_CARRIED_PAD", pad: null }); // `IPO_SINGLE_CLICK`: the next pad is made afresh from the default pad, which now has this one's settings
      dispatch({ type: "SET_DEFAULT_PAD", pad: settingsOf(pad) });
      const number = pad.number;
      if (number) dispatch({ type: "SET_LAST_PAD_NUMBER", number }); // `PAD_PLACER::CreateItem`: `m_padTool->SetLastPadNumber( padNumber )`
      return;
    }

    // DRAWING_TOOL::SetAnchor: one click, `footprint->MoveAnchorPosition( footprint->GetPosition() - cursorPos )`
    // (every pad/graphic/text shifts so the clicked point becomes the origin), then the tool is popped.
    if (state.activeTool === "anchor") {
      void api.setAnchor({ x: sx, y: sy });
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
      return;
    }

    // The dimensions, rule areas, text boxes, points, tables, barcodes and reference images (components/footprint/fpItemTools.ts).
    if (FP_ITEM_TOOLS.has(state.activeTool)) {
      fpItemToolClick({ state, dispatch, api }, [sx, sy], isMac() ? e.metaKey : e.ctrlKey);
      return;
    }

    const shapeKind = SHAPE_TOOL_KIND[state.activeTool];
    // DRAWING_TOOL::drawArc: centre, start, end -- ARC_GEOM_MANAGER decides what each click means and when the arc is done.
    if (shapeKind === "arc") {
      const prev = state.drawState?.shapeKind === "arc" ? state.drawState.arc : undefined;
      const { geom, arc } = arcClick(prev, [sx, sy], arcAngleSnap(state.lineMode, e));
      if (arc) void api.addGraphic({ kind: "arc", layer: state.activeLayer, stroke_width: fpStrokeWidthNm(state.activeLayer, getFpGraphicsDefaults()), filled: false, start: ptXY(arc.start), mid: ptXY(arc.mid), end: ptXY(arc.end) });
      dispatch({ type: "SET_DRAW_STATE", draw: geom ? { kind: "shape", shapeKind: "arc", pts: arcClickPoints(geom), arc: geom } : null });
      return;
    }
    // DRAWING_TOOL::drawOneBezier (via DrawBezier's chaining loop): start, control 1, end, control 2.
    if (shapeKind === "bezier") {
      const prev = state.drawState?.shapeKind === "bezier" ? state.drawState.bezier : undefined;
      const { geom, curve } = bezierClick(prev, [sx, sy]);
      if (curve) void api.addGraphic(bezierShape(curve, state.activeLayer, fpStrokeWidthNm(state.activeLayer, getFpGraphicsDefaults())));
      dispatch({ type: "SET_DRAW_STATE", draw: geom ? { kind: "shape", shapeKind: "bezier", pts: [], bezier: geom } : null });
      return;
    }
    if (shapeKind === "polygon") {
      // `DRAWING_TOOL::DrawZone` for a graphic polygon (`ZONE_MODE::GRAPHIC_POLYGON`): `POLYGON_GEOM_MANAGER` in the line mode -- a click on the first corner closes the outline,
      // in the 45- and 90-degree modes a click locks in the leader's bend and its end.
      const already = state.drawState?.shapeKind === "polygon" ? state.drawState.pts : [];
      const mode = leaderModeOf(state.lineMode, isMac() ? e.metaKey : e.ctrlKey);
      const click = zoneClick(already, [sx, sy], mode);
      if (click.finish) {
        const outline = zoneFinish(already, [sx, sy], mode);
        const shape = outline ? footprintToShapeArg("polygon", outline, state.activeLayer) : null;
        if (shape) void api.addGraphic(shape);
        dispatch({ type: "SET_DRAW_STATE", draw: null });
      } else {
        dispatch({ type: "SET_DRAW_STATE", draw: { kind: "shape", shapeKind: "polygon", pts: click.pts } });
      }
      return;
    }
    if (shapeKind) {
      const already = state.drawState?.shapeKind === shapeKind ? state.drawState.pts : [];
      // `drawShape`'s second point under the line mode (a line) or Ctrl (a rectangle, a circle): `GetVectorSnapped45` / `GetVectorSnapped90`
      const point: [number, number] = already.length > 0 && (shapeKind === "segment" || shapeKind === "rect" || shapeKind === "circle") ? shapeSecondPoint(shapeKind, state.lineMode, isMac() ? e.metaKey : e.ctrlKey, already[already.length - 1]!, [sx, sy]) : [sx, sy];
      const pts: [number, number][] = [...already, point];
      if (pts.length >= (AUTO_FINISH[shapeKind] ?? Infinity)) {
        const shape = footprintToShapeArg(shapeKind, pts, state.activeLayer);
        if (shape) void api.addGraphic(shape);
        dispatch({ type: "SET_DRAW_STATE", draw: null });
      } else {
        dispatch({ type: "SET_DRAW_STATE", draw: { kind: "shape", shapeKind, pts } });
      }
      return;
    }

    // `DRAWING_TOOL::PlaceText` (kicad-port/fpTextTool.ts): the first click makes a text with the layer's defaults and opens the dialog on it; the text then follows the
    // cursor, and the next click commits it ("Draw Text") and selects it. The tool stays armed.
    if (state.activeTool === "text") {
      if (state.textDialog) return;
      if (state.carriedText) {
        const text = { ...state.carriedText, at: { x: sx, y: sy } };
        dispatch({ type: "SET_CARRIED_TEXT", text: null });
        void api.addText(text).then((id) => {
          if (id) dispatch({ type: "SET_SELECTION", refs: [id] }); // `selectItem, text`
        });
        return;
      }
      dispatch({ type: "SET_SELECTION", refs: [] }); // `ACTIONS::selectionClear`
      dispatch({ type: "SET_TEXT_DIALOG", text: fpNewText(state.activeLayer, { x: sx, y: sy }, getFpGraphicsDefaults()) });
      return;
    }

    // Select tool.
    // `PCB_POINT_EDITOR`: a press on an edit point of the one selected graphic drags that point (`EDIT_POINTS::FindPoint`; the drag starts at `IsDrag( BUT_LEFT )`).
    const pointTarget = fp && state.activeTool === "select" ? fpPointEditTarget(fp.graphics, state.selection) : null;
    const pressedPoint = pointTarget ? fpPointAt(pointTarget, [wx, wy], Math.max(100_000, 10 / (state.view.scale || 1))) : null;
    if (pointTarget && pressedPoint) {
      dragRef.current = { kind: "point", id: pointTarget.id, base: pointTarget, point: pressedPoint };
      setPointPreview(null);
      return;
    }
    // `selectPoint`: "if( m_enteredGroup && !m_enteredGroup->GetBoundingBox().Contains( aWhere ) ) ExitGroup();" -- a click outside the group being worked in leaves it
    const groupBox = fp && state.enteredGroupId ? fpItemBounds(fp, state.enteredGroupId) : null;
    const leaving = !!state.enteredGroupId && (!groupBox || wx < groupBox[0] || wx > groupBox[2] || wy < groupBox[1] || wy > groupBox[3]);
    if (leaving) dispatch({ type: "EXIT_GROUP", selectGroup: false });
    const hit = hitTest(wx, wy, leaving ? null : undefined);
    const ctrlOrCmd = isMac() ? e.metaKey : e.ctrlKey;
    const modifiers = computeClickModifiers(e.shiftKey, ctrlOrCmd, e.altKey);
    // `PCB_SELECTION_TOOL::Main`'s `IsDrag( BUT_LEFT )` under "Left button drag:" (`leftDragOutcome`): a drag with a modifier, from empty space, or from an item the
    // setting does not let the drag pick up draws the selection rectangle; the press then changes nothing yet -- a release without a drag is the click.
    const outcome = leftDragOutcome(wheelPrefs.settings.dragLeft, { modifier: hasModifier(modifiers), selectionEmpty: state.selection.size === 0, onItem: hit != null, inSelection: hit != null && state.selection.has(hit.id) });
    if (!hit || outcome === "area") {
      dragRef.current = { kind: "box", startWorld: [wx, wy], startScreen: [e.clientX, e.clientY], modifiers, click: hit?.id ?? null };
      return;
    }
    if (hit) {
      const refs = applySingleClickModifier(state.selection, hit.id, modifiers);
      dispatch({ type: "SET_SELECTION", refs });
      if (refs.includes(hit.id)) {
        const held = refs.length > 1 ? refs : [hit.id];
        // `BestDragOrigin`: the point of what is picked up that the move holds it by -- the nearest origin or corner of the items to the pointer (else the pointer itself).
        const [ox, oy] = snap.dragOrigin([wx, wy], held);
        dragRef.current = { kind: "move", refs: held, moveKind: hit.kind, startWorld: [wx, wy], snapOrigin: [ox, oy] };
        dispatch({ type: "SET_MOVE_ORIGIN", at: { x: ox, y: oy } });
      }
    }
  };

  const onPointerMove = (e: React.PointerEvent) => {
    lineModeCtrl = isMac() ? e.metaKey : e.ctrlKey;
    const [wx, wy] = worldAt(e);
    dispatch({ type: "SET_CURSOR", at: { x: wx, y: wy } });
    const motion = gestureRef.current?.move(e.clientX, e.clientY, e.timeStamp);
    if (viewCtl.move(e, !!motion?.dragging)) return;
    // The Renumber Pads tool: a drag over pads numbers them (`evt->IsDrag( BUT_LEFT )`); every event moves `oldMousePos` on.
    if (state.activeTool === "enumerate" && state.enumerate) {
      if (e.buttons & 1) enumerateAt(wx, wy, false);
      else lastMouseRef.current = { x: wx, y: wy };
      return;
    }
    // `drawArc` / `drawOneBezier`'s motion branch: update the construction manager's geometry (never its step) with the snapped cursor.
    const draw = state.drawState;
    // The tools that place something: the grid helper sees every move, so the marker, the snap lines and the rubber band follow the cursor.
    const cursor = placing ? toolPoint(e, wx, wy) : null;
    if (cursor && state.itemDraw) fpItemToolMotion({ state, dispatch, api }, cursor, isMac() ? e.metaKey : e.ctrlKey);
    if (draw && (draw.arc || draw.bezier) && cursor) {
      const [sx, sy] = cursor;
      if (draw.arc) dispatch({ type: "SET_DRAW_STATE", draw: { ...draw, arc: arcMotion(draw.arc, [sx, sy], arcAngleSnap(state.lineMode, e)) } });
      else if (draw.bezier) dispatch({ type: "SET_DRAW_STATE", draw: { ...draw, bezier: bezierMotion(draw.bezier, [sx, sy]) } });
    }
    // The armed Move (a Duplicate's pick-up): the selection follows the snapped cursor from where it was picked up.
    if (state.activeTool === "move" && state.duplicatePending && state.selection.size > 0) {
      const held = [...state.selection];
      const [sx, sy] = (([x, y]) => [x, y] as [number, number])(snap.moveCursor([wx, wy], snapMods(e), held, heldCategory(held)));
      // No pointer position was known when the copies were picked up (`doMoveSelection`'s `originalCursorPos`): the first move is it.
      if (!state.moveOriginNm) dispatch({ type: "SET_MOVE_ORIGIN", at: { x: sx, y: sy } });
      const origin = state.moveOriginNm ?? { x: sx, y: sy };
      dispatch({ type: "SET_MOVE_PREVIEW", preview: { refs: [...state.selection], dxNm: sx - origin.x, dyNm: sy - origin.y } });
      return;
    }
    const drag = dragRef.current;
    if (!drag) return;
    if (drag.kind === "box") {
      if (!motion?.dragging) return; // a click with a little jitter, not a rectangle
      const rect = containerRef.current!.getBoundingClientRect();
      const x0 = drag.startScreen[0] - rect.left;
      const x1 = e.clientX - rect.left;
      setMarquee({ x0, y0: drag.startScreen[1] - rect.top, x1, y1: e.clientY - rect.top, crossing: isCrossingSelection(x0, x1) });
    } else if (drag.kind === "point") {
      if (!motion?.dragging) return;
      // the point on the graphics grid or an anchor of another item (`BestSnapAnchor( cursor, layers, GRID_GRAPHICS, item )`), moved in the Footprint Editor's arc mode
      const p = snap.point("point_edit", [wx, wy], snapMods(e), { layers: new Set([drag.base.layer]), category: "graphics", skip: new Set([drag.id]) });
      setPointPreview(fpPointDrag(drag.base, drag.point, [p[0], p[1]], studioPrefs.fpArcEditMode));
    } else if (drag.kind === "move") {
      // Nothing is picked up until the press has become a drag (tool_dispatcher.cpp: 8 px, or on macOS a motion after 300 ms held).
      if (!motion?.dragging) return;
      // `BestSnapAnchor( cursor, { active layer }, selection grid, selection )`, the cursor being the held point plus how far the pointer has travelled (kicad-port/heldCursor.ts).
      const c = snap.moveCursor(heldCursor(drag.snapOrigin, drag.startWorld, [wx, wy]), snapMods(e), drag.refs, heldCategory(drag.refs));
      const [sx, sy] = [c[0], c[1]];
      const origin = state.moveOriginNm ?? { x: sx, y: sy };
      dispatch({ type: "SET_MOVE_PREVIEW", preview: { refs: drag.refs, kind: drag.moveKind, dxNm: sx - origin.x, dyNm: sy - origin.y } });
    }
  };

  const onPointerUp = (e: React.PointerEvent) => {
    gestureRef.current?.up();
    viewCtl.release();
    const drag = dragRef.current;
    dragRef.current = null;
    if (!drag) return;
    if (drag.kind === "box") {
      const box = marquee;
      setMarquee(null);
      if (!box) {
        // no drag: the click -- on an item it selects it (as the modifiers say), on nothing it clears the selection unless a modifier is held
        if (drag.click) dispatch({ type: "SET_SELECTION", refs: applySingleClickModifier(state.selection, drag.click, drag.modifiers) });
        else if (!hasModifier(drag.modifiers)) dispatch({ type: "CLEAR_SELECTION" });
        return;
      }
      // `SelectRectArea` -> `SelectMultiple`: what the rectangle takes (as its direction says), each as the item a click on it would select (a group's member as the group)
      const [wx, wy] = worldAt(e);
      // `SelectMultiple` takes what `itemPassesFilter( item, true )` lets through: the Selection Filter's rows, a locked item (a pad too) only with Locked items on
      const filter = studioPrefs.fpSelectionFilter;
      const boxed = fp ? rectSelectionHits(footprintItemBoxes(fp), drag.startWorld, [wx, wy]).filter((id) => {
        const kind = fpItemKind(fp, id);
        return !kind || kind === "group" || fpPassesFilter(filter, kind, fpItemLocked(fp, id), kind === "zone" && !!(fp.items?.zones ?? []).find((z) => z.id === id)?.is_rule_area, true);
      }) : [];
      const hits = fp ? [...new Set(boxed.map((id) => fpSelectionTarget(fp, id, state.enteredGroupId)).filter((id): id is string => id !== null))] : [];
      if (hits.length > 0 || hasModifier(drag.modifiers)) dispatch({ type: "SET_SELECTION", refs: applyBoxSelectionModifiers(state.selection, hits, drag.modifiers) });
      else dispatch({ type: "CLEAR_SELECTION" });
      return;
    }
    if (drag.kind === "point") {
      // the drop: `commit.Modify( item )` -- `fp_item set_graphic`, one undo step, the shape's looks kept by id
      const next = pointPreview;
      setPointPreview(null);
      const was = fp?.graphics.find((g) => g.id === drag.id);
      if (next && was && JSON.stringify(next) !== JSON.stringify(was)) void api.setGraphic(drag.id, next);
      return;
    }
    if (drag.kind === "move" && state.movePreview) {
      const { refs, dxNm, dyNm } = state.movePreview;
      if (dxNm === 0 && dyNm === 0) dispatch({ type: "SET_MOVE_PREVIEW", preview: null }); // dragged back to where it began: nothing to commit
      else void api.commitMove(refs, dxNm, dyNm); // one undo step for the whole drop
    }
  };

  const onWheel = (e: WheelEvent) => {
    e.preventDefault();
    userMovedRef.current = true;
    const rect = containerRef.current!.getBoundingClientRect();
    const result = handleWheel(state.view, { width: rect.width, height: rect.height }, wheelInputOf(e, rect), wheelPrefs.settings, wheelPrefs.controller);
    if (result.kind !== "unhandled") dispatch({ type: "SET_VIEW", view: result.view });
  };
  useNonPassiveWheel(containerRef, onWheel); // React's onWheel is passive: preventDefault() would be ignored and logged

  const onDoubleClick = (e: React.MouseEvent) => {
    // The Renumber Pads tool owns the double click (it finishes the tool); it must not also open a pad's properties.
    if (state.activeTool === "enumerate" || performance.now() < dblSuppressUntil.current) return;
    // A tool of the dimensions, rule areas ... owns the double click: its clicks were the two presses; a rule area's outline closes. No item's dialog opens.
    if (FP_ITEM_TOOLS.has(state.activeTool)) {
      if (state.drawState) finishDraw();
      return;
    }
    if (state.drawState) {
      finishDraw();
      return;
    }
    const [wx, wy] = worldAt(e);
    const hit = hitTest(wx, wy);
    // `PCB_SELECTION_TOOL::Main`'s double click: on a group, `EnterGroup` (its members selected, each picked one by one from then on, UI-LEFT-2 FF8); else
    // `EDIT_TOOL::Properties` -> `FOOTPRINT_EDIT_FRAME::OnEditItemRequest`, the item's own dialog.
    if (hit && fp && hit.kind === "group") {
      dispatch({ type: "ENTER_GROUP", id: hit.id, members: fpGroupMembers(fp, hit.id) });
      return;
    }
    if (hit && fp) openProperties(fpEditTarget(fp, [hit.id]));
  };

  /** `FOOTPRINT_EDIT_FRAME::OnEditItemRequest`: the dialog of the item (kicad-port/fpItems.ts `fpEditTarget`). */
  const openProperties = (target: FpEditTarget) => {
    if (!target) return;
    switch (target.dialog) {
      case "pad":
        return dispatch({ type: "SET_PAD_PROPERTIES_ID", id: target.id });
      case "field":
      case "text":
        return dispatch({ type: "SET_TEXT_EDIT_ID", id: target.id });
      case "shape":
        return dispatch({ type: "SET_SHAPE_DIALOG_ID", id: target.id });
      case "footprint":
        return dispatch({ type: "SET_FOOTPRINT_PROPERTIES_OPEN", open: true });
      default:
        return dispatch({ type: "SET_ITEM_DIALOG", dialog: { id: target.id, kind: target.dialog } });
    }
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && state.drawState) {
      e.preventDefault();
      finishDraw();
      return;
    }
    if (e.key === "Escape") {
      // `EDIT_TOOL::Duplicate`: a cancelled pick-up `commit.Revert()`s the copies -- the duplicate was the last undo step.
      if (state.duplicatePending) void api.cancelPickUp();
      dispatch({ type: "ESCAPE" });
      return;
    }
    // F with the Pad tool armed: `PCB_ACTIONS::flip` on the carried pad (`IPO_FLIP`: `newItem->Flip( newItem->GetPosition(), m_FlipDirection )`). (R / Shift+R are the
    // registered rotate actions, which turn the carried pad the same way: actions/editorFrameActions.ts.)
    if ((e.key === "f" || e.key === "F") && !e.ctrlKey && !e.metaKey && !e.altKey && state.activeTool === "pad") {
      e.preventDefault();
      const flipped = flipCarriedPad(carriedPadAt(0, 0), flipDirection);
      if (flipped) dispatch({ type: "SET_CARRIED_PAD", pad: flipped });
      else dispatch({ type: "TOAST", message: "A pad with its own shapes on other copper layers (a padstack) cannot be flipped here yet.", kind: "info" });
      return;
    }
    // Backspace mid-draw -> `deleteLastPoint`: the construction manager steps back one point.
    if (e.key === "Backspace" && state.drawState && (state.drawState.arc || state.drawState.bezier)) {
      e.preventDefault();
      const draw = state.drawState;
      if (draw.arc) {
        const g = arcRemoveLastPoint(draw.arc);
        dispatch({ type: "SET_DRAW_STATE", draw: g.step === 0 ? null : { ...draw, arc: g, pts: arcClickPoints(g) } });
      } else if (draw.bezier) {
        const g = bezierRemoveLastPoint(draw.bezier);
        dispatch({ type: "SET_DRAW_STATE", draw: g.step === 0 ? null : { ...draw, bezier: g } });
      }
      return;
    }
    // Delete is the registered `common.Interactive.delete` (actions/editorFrameActions.ts), which the window-level hotkey runs; the canvas keeps Backspace, which
    // the hotkey table binds on a Mac only (`macHotkey`), so off a Mac it would otherwise do nothing.
    const macDelete = /Mac/.test(navigator.platform);
    if (e.key === "Backspace" && !macDelete && state.selection.size > 0 && state.activeTool === "select") {
      void api.deleteItems([...state.selection]);
      return;
    }
    // R / Shift+R (rotate the selected pads) is the registered action `pcbnew.InteractiveEdit.rotateCcw` / `rotateCw` (actions/editorFrameActions.ts), which
    // the window-level hotkey runs -- and the top toolbar's Rotate buttons too -- so this canvas no longer handles the key itself (it would rotate twice).
    // E is the registered `pcbnew.InteractiveEdit.properties` (actions/editorFrameActions.ts: `EDIT_TOOL::Properties`), which the window-level hotkey runs.
  };

  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    // A right-drag pans and opens nothing: the button went up as a drag (TA_MOUSE_UP), not a click, so no menu.
    if (viewCtl.takeJustPanned()) return;
    const [wx, wy] = worldAt(e);
    const hit = hitTest(wx, wy);
    if (hit && !state.selection.has(hit.id)) dispatch({ type: "SET_SELECTION", refs: [hit.id] });
    const refs = hit ? (state.selection.has(hit.id) ? [...state.selection] : [hit.id]) : [...state.selection];
    const padRefs = refs.filter((r) => api.padById(r));
    /** An entry that runs a registered KiCad action (PAD_TOOL's selection-menu items, `PAD_TOOL::Init`). */
    const act = (name: string, label: string, disabled = false): MenuEntry => ({ label, onSelect: () => run(name), disabled: disabled || !isEnabled(name) });
    const anyGroup = !!fp && refs.some((r) => isGroup(fp.items?.groups ?? [], r));
    const entries: MenuEntry[] = [
      // `PCB_SELECTION_TOOL::Init`: Enter Group with one group selected (`groupEnterCondition`), Leave Group inside one (`inGroupCondition`)
      ...(fp && refs.length === 1 && isGroup(fp.items?.groups ?? [], refs[0]!) ? [act("common.Interactive.groupEnter", "Enter Group")] : []),
      ...(state.enteredGroupId ? [act("common.Interactive.groupLeave", "Leave Group")] : []),
      // `EDIT_TOOL::Init`'s transform submenu: Rotate Counterclockwise / Clockwise, Mirror Horizontally / Vertically (`canMirror`)
      act("pcbnew.InteractiveEdit.rotateCcw", "Rotate Counterclockwise (R)", refs.length === 0),
      act("pcbnew.InteractiveEdit.rotateCw", "Rotate Clockwise (Shift+R)", refs.length === 0),
      act("pcbnew.InteractiveEdit.mirrorHoriontally", "Mirror Horizontally", !fp || !fpCanMirror(fp, refs)),
      act("pcbnew.InteractiveEdit.mirrorVertically", "Mirror Vertically", !fp || !fpCanMirror(fp, refs)),
      // `EDIT_TOOL::Init`: `menu.AddItem( PCB_ACTIONS::swap, SELECTION_CONDITIONS::MoreThan( 1 ) )`
      act("pcbnew.InteractiveEdit.swap", "Swap (Alt+S)", refs.length < 2),
      act("pcbnew.InteractiveEdit.properties", "Properties... (E)", refs.length > 1),
      // `PCB_GROUP_TOOL`'s Grouping menu: Group (two or more items), Ungroup (a group among them)
      act("common.Interactive.group", "Group Items", refs.length < 2),
      act("common.Interactive.ungroup", "Ungroup Items", !anyGroup),
      // `GROUP_CONTEXT_MENU::update`: Add Items with one group and an ungrouped item, Remove Items with a member
      act("common.Interactive.addToGroup", "Add Items", !fp || !planAddToGroup(refs, fp.items?.groups ?? [])),
      act("common.Interactive.removeFromGroup", "Remove Items", !fp || planRemoveFromGroup(refs, fp.items?.groups ?? []).length === 0),
      // `EDIT_TOOL::Init`'s clipboard rows (UI-LEFT-2 FF1)
      act("common.Interactive.cut", "Cut (Ctrl+X)", refs.length === 0),
      act("common.Interactive.copy", "Copy (Ctrl+C)", refs.length === 0),
      act("common.Interactive.paste", "Paste (Ctrl+V)"),
      { label: "Duplicate (Ctrl+D)", onSelect: () => void api.duplicateSelection(false), disabled: refs.length === 0 },
      { label: "Duplicate and Increment (Ctrl+Shift+D)", onSelect: () => void api.duplicateSelection(true), disabled: refs.length === 0 },
      act("pcbnew.Array.createArray", "Create Array... (Ctrl+T)", refs.length === 0),
      { label: "Pad Properties...", onSelect: () => dispatch({ type: "SET_PAD_PROPERTIES_ID", id: padRefs[0]! }), disabled: padRefs.length !== 1 },
      // `PAD_TOOL::Init`: the pad edit entries of the selection menu -- Finish Pad Edit while a pad is edited, Edit Pad as Graphic Shapes for one selected pad.
      act("pcbnew.ModuleEditor.padTable", "Pad Table...", padToolsBlocked(fp)),
      act("pcbnew.PadTool.enumeratePads", "Renumber Pads...", (fp?.pads.length ?? 0) === 0 || padToolsBlocked(fp)),
      ...(padEditMenu(fp, new Set(refs)).recombine ? [act("pcbnew.PadTool.recombinePad", "Finish Pad Edit (Ctrl+E)")] : []),
      ...(padEditMenu(fp, new Set(refs)).explode ? [act("pcbnew.PadTool.explodePad", "Edit Pad as Graphic Shapes (Ctrl+E)")] : []),
      act("pcbnew.PadTool.CopyPadSettings", "Copy Pad Properties to Default", padRefs.length !== 1),
      act("pcbnew.PadTool.ApplyPadSettings", "Paste Default Pad Properties to Selected", padRefs.length === 0),
      act("pcbnew.PadTool.PushPadSettings", "Push Pad Properties to Other Pads...", padRefs.length !== 1),
      { label: "Delete (Del)", onSelect: () => void api.deleteItems(refs), disabled: refs.length === 0 },
    ];
    setContextMenu({ x: e.clientX, y: e.clientY, entries });
  };

  return (
    <div
      ref={containerRef}
      className="pcb-canvas-container"
      data-fp-canvas=""
      tabIndex={0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onDoubleClick={onDoubleClick}
      onKeyDown={onKeyDown}
      onContextMenu={onContextMenu}
    >
      <canvas ref={canvasRef} />
      <SelectionMarquee m={marquee} />
      {!fp && state.name && <div className="pcb-canvas-empty">{state.error ?? "Loading footprint…"}</div>}
      {/* Nothing open: the empty canvas and its grid, like KiCad's, with one quiet line saying how to open something. */}
      {!fp && !state.name && <div className="canvas-empty-hint">No footprint loaded -- double-click one in Libraries, or press Ctrl+N for a new one</div>}
      {contextMenu && <ContextMenu x={contextMenu.x} y={contextMenu.y} entries={contextMenu.entries} onClose={() => setContextMenu(null)} />}
    </div>
  );
}

/** `EDIT_POINTS::ViewDraw` of a graphic's edit points, as the board draws them (`drawShapeHandles`): a square at a corner, a diamond at a middle; in world units. */
function paintFpEditPoints(ctx: CanvasRenderingContext2D, scale: number, shape: Shape): void {
  const r = 4 / scale;
  ctx.save();
  ctx.fillStyle = layerColor("selection");
  ctx.strokeStyle = "rgba(0,0,0,0.6)";
  ctx.lineWidth = 1 / scale;
  for (const p of shapeEditPoints(shape)) {
    const [x, y] = p.pos;
    ctx.beginPath();
    if (p.kind === "corner") ctx.rect(x - r, y - r, r * 2, r * 2);
    else {
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
