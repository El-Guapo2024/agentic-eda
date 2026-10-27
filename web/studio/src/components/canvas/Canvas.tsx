// The PCB canvas. Owns the <canvas> element, the render loop, and the
// view/selection/edit pointer+keyboard interactions:
//   - wheel zoom/pan exactly like wx_view_controls.cpp's onWheel (see
//     kicad-port/viewControls.ts: plain wheel zooms about the cursor,
//     Ctrl+wheel pans horizontally, Shift/Alt+wheel pans vertically);
//     middle- and right-drag both pan (KiCad's own drag_middle/drag_right
//     defaults), edge auto-pan while dragging (off by default, same as
//     KiCad -- state.prefs.autoPan; the wheel assignment and zoom speed are
//     Preferences > Mouse and Touchpad's, see kicad-port/preferences.ts).
//   - click to select (Shift adds); drag from empty space box-selects,
//     left-to-right = window (fully enclosed), right-to-left = crossing
//     (touching) -- crates/ops/src/view.rs has no notion of this, it's
//     pure frontend geometry against each part's courtyard.
//   - drag a selected part to move it, previewed locally, committed with
//     `move_items` on drop (see state/store.tsx `commitMove`); M starts the
//     same preview without holding the button, click commits, Esc cancels.
//   - R / Shift+R rotate by a quarter turn each way; Delete rips.
import { openViaStitchDialog } from "../../state/kicad10Dialogs";
import { newViaStitch } from "../../kicad-port/viaStitchTool";
import { chosenDiffPair, chosenTrackWidthOf } from "../../kicad-port/routerSizes";
import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CmdShape, Part } from "../../api/types";
import { DEFAULT_RULE_AREA_SETTINGS, DEFAULT_ZONE_SETTINGS, useStudioApi, useStudioDispatch, useStudioState, withGroupSubstitution } from "../../state/store";
import { drawStrokeWidthNm, graphicPolygonStyle } from "../../kicad-port/layerDefaults";
import { activeLayerName, layerIsVisible } from "../../kicad-port/layerPresets";
import { carryStart, isLocked } from "../../kicad-port/pcbTransform";
import { itemBounds, padById, padParent } from "../../kicad-port/pcbItems";
import { EXTRA_KINDS, extraItemLabel } from "../../kicad-port/pcbExtraItems";
import { fieldById } from "../../kicad-port/fpFields";
import { isGroup, substituteSelection, topLevelGroup, withinScope } from "../../kicad-port/groupTree";
import type { ToolId } from "../../state/store";
import type { RuleAreaFields, Shape, Zone, ZoneSettingsFields } from "../../api/types";
import { activeEditPoint, cmdShapeToShape, moveShapePoint, shapeEditPoints, shapeToCmd, type EditPoint } from "../../kicad-port/pcbPointEdit";
import { reshapeCmds } from "../../kicad-port/shapeFill";
import { applyCommands } from "../../actions/pcbSweepKit";
import { picker } from "../../actions/pcbPicker";
import { boundsOfPoints, fitTransform, screenToWorld, panByWorldDelta } from "./view";
import { paintBoard } from "./painter";
import { buildPaintAppearance } from "./appearancePaint";
import { netsContext } from "../../kicad-port/appearanceNets";
import { isStale } from "../../kicad-port/checkRevision";
import { layerColor } from "./layers";
import { snapPoint } from "./gridHelper";
import { modificationSnapFor } from "../../state/modificationSnap";
import { PcbSnap, type PointOpts, type SnapMods } from "./pcbSnap";
import { paintSnapOverlay } from "./snapOverlayPainter";
import { useGridOverrides } from "../../state/gridOverrides";
import { useGridSettings } from "../../state/gridSettings";
import { toPcbMagnetic } from "../../kicad-port/preferences";
import { withTrackSnapping } from "../../kicad-port/routerSettings";
import { heldCursor } from "../../kicad-port/heldCursor";
import { shapeSecondPoint } from "../../kicad-port/pcbShapeSnap";
import { SELECT_NET_TITLE, selectNetLabels, snapViaPosition, viaNet, type ViaPlacerInput } from "../../kicad-port/viaPlacer";
import { startInteractiveRoute, fixInteractiveRoute, finishInteractiveRoute } from "./routing";
import { clearRouteQueue } from "./routeQueue";
import { createMoveThrottle, createRequestGuard, drawStateFromPreview } from "../../kicad-port/routeTool";
import { routeMove, routeDragMove, dpMove, fetchState, setRouterEnteredGroup } from "../../api/client";
import { finishInlineDrag } from "./dragging";
import { dragStateFromPreview } from "../../kicad-port/dragTool";
import { startDiffPairRoute, fixDiffPairRoute, finishDiffPairRoute } from "./diffPairRouting";
import { finishEditPattern, fixTune, hoverTune, moveTune, startTune, TuneHandleDrag } from "./tuneRouting";
import { handleAt } from "../../kicad-port/tuneTool";
import { dpStateFromPreview } from "../../kicad-port/dpTool";
import { ContextMenu, type MenuEntry } from "./ContextMenu";
import { handleWheel, computeAutoPanDirection, computeAutoPanStep, DEFAULT_VIEW_CONTROL_SETTINGS, type WheelInput } from "../../kicad-port/viewControls";
import { useWheelPrefs } from "../../actions/useWheelPrefs";
import { useNonPassiveWheel } from "../../hooks/useNonPassiveWheel";
import { isMac } from "../../platform";
import { ClickDragGesture, dragRuleFor } from "../../kicad-port/dragThreshold";
import { computeClickModifiers, isCrossingSelection, applySingleClickModifier, applyBoxSelectionModifiers, hasModifier, type ClickModifiers } from "../../kicad-port/selection";
import { pickSelectionCandidates, collectBoxSelection, type SelectionCandidate, type SelectableKind } from "./selectionCandidates";
import { openPropertiesFor } from "./properties";
import { useActionRunner } from "../../actions/useActionRunner";
import { SchContextMenu } from "../schematic/SchContextMenu";
import { buildPcbMenu } from "./pcbMenuBuilder";
import type { MenuNode } from "../../kicad/types";
import { findNearestCorner, moveCorner } from "../../kicad-port/zonePointEditor";
import { zoneLayers } from "../../kicad-port/zoneLayers";
import { defaultDimensionPayload } from "../../kicad-port/dimensionConvert";
import { cellAddress, cellById, drawTableSession, tableById } from "../../kicad-port/tables";
import { openDrawnTableDialog } from "../../state/tableDialogs";
import { arcClick, arcMotion } from "../../kicad-port/arcGeom";
import { leaderModeOf, zoneClick, zoneFinish } from "../../kicad-port/pcbDrawZone";
import { bezierClick, bezierFinishDouble, bezierMotion } from "../../kicad-port/bezierGeom";
import { arcAngleSnap, arcClickPoints, bezierShape, ptXY } from "./curveTools";
import { displayedRatsnest, flipLocalX, flipPan, highlightedNets, panDeltaX, toggleLocalRatsnestFootprint, toggleLocalRatsnestPad } from "../../kicad-port/boardControl";
import { padAt } from "../../kicad-port/boardControlPick";
import { canExclude, rcMenu } from "../../kicad-port/rcItems";
import { drcMarkerAt, drcMenuContext, runMarkerMenu, selectMarkerDrc, showDrcInDialog, toMenuEntries } from "../../actions/checkerOps";
import { askExclusionComment } from "../../state/checkerView";
import { useImageVersion } from "./imageCache";
import "../../styles/canvas.css";

/** `ZONE_SETTINGS << aSrcZone` (zone_create_helper.cpp createZoneFromExisting): the fill + rule-area settings of an existing zone, as `api.addZone` takes them. */
function zoneSettingsOf(zone: Zone): ZoneSettingsFields & RuleAreaFields {
  const out: Record<string, unknown> = {};
  for (const k of Object.keys({ ...DEFAULT_ZONE_SETTINGS, ...DEFAULT_RULE_AREA_SETTINGS })) out[k] = (zone as unknown as Record<string, unknown>)[k];
  return out as unknown as ZoneSettingsFields & RuleAreaFields;
}

/**
 * What a plain drag of a clicked item picks up. Every kind can be: the drop is one `move_items` (kicad-port/pcbTransform.ts `planCarry`), and a pad
 * stands for its footprint (`FilterCollectorForFreePads`) -- dragging a pad drags the footprint, as in pcbnew.
 */
const DRAGGABLE_KINDS = new Set<SelectableKind>(["part", "track", "via", "zone", "shape", "text", "dimension", "pad", "field", ...EXTRA_KINDS]);

/** The tools whose clicks go through a grid helper: `BestSnapAnchor` on every event, or the router's `snapToItem`. */
const PLACING_TOOLS = new Set<string>(["route", "diffpair", "tune", "drag", "via", "zone", "draw_segment", "draw_arc", "draw_bezier", "draw_rect", "draw_circle", "draw_polygon", "text", "dimension", "table", "measure", "drill_origin", "local_ratsnest", "picker"]);

/** wx_view_controls.cpp onButton: MiddleDown/RightDown both start DRAG_PANNING by default (m_dragMiddle/m_dragRight == MOUSE_DRAG_ACTION::PAN). A plain click (no real movement) of the right button still opens the context menu -- see onContextMenu's `justPanned` check -- same as source's right button also being each platform's native context-menu trigger. */
const PAN_BUTTONS = new Set([1, 2]);
// (A right-button press is a pan-drag rather than a click-to-open-the-context-menu, and a held press stops being a long press, by the dispatcher's rule:
// kicad-port/dragThreshold.ts, tool_dispatcher.cpp -- more than 8 px of travel along an axis, or on macOS a motion after 300 ms held.)

const LONG_PRESS_MS = 500;
// A new graphic starts at the line thickness of its layer's class (Board Setup > Text & Graphics > Defaults, kicad-port/layerDefaults.ts); incWidth / decWidth step it (state.pcbx.drawStrokeStep).
/** How many points finish a given drawing-tool shape by itself, once reached, without waiting for an explicit Enter/double-click -- a plain 2-point line/rect/circle doesn't need a third confirmation the way a polygon does. Arc is start/mid/end (3); zone/polygon/route have no auto-finish (arbitrary length). */
type ShapeToolKind = "segment" | "arc" | "rect" | "circle" | "polygon" | "bezier";

function shapeAutoFinishCount(kind: ShapeToolKind): number | null {
  switch (kind) {
    case "segment":
    case "rect":
    case "circle":
      return 2;
    case "arc": // not point-counted: kicad-port/arcGeom.ts's ARC_GEOM_MANAGER (centre, start, end) decides when the arc is complete
    case "bezier": // kicad-port/bezierGeom.ts's BEZIER_GEOM_MANAGER (four points, chained)
    case "polygon":
      return null;
  }
}

/** `pts` -> the matching `CmdShape` variant for `add_shape`, or null if there aren't enough points yet -- the IR's own point-count floor per kind (a polygon needs 3+, everything else exactly the fixed count `shapeAutoFinishCount` already enforces before this is ever called with too few). */
function shapeFromPoints(kind: ShapeToolKind, pts: [number, number][], layer: string, widthNm: number): CmdShape | null {
  const p = (i: number) => ({ x: pts[i]![0], y: pts[i]![1] });
  switch (kind) {
    case "bezier":
      return null; // built from the construction manager's `BezierCurve`, never from a bare point list
    case "segment":
      return pts.length >= 2 ? { kind: "segment", layer, stroke_width: widthNm, filled: false, start: p(0), end: p(1) } : null;
    case "rect":
      return pts.length >= 2 ? { kind: "rect", layer, stroke_width: widthNm, filled: false, start: p(0), end: p(1) } : null;
    case "circle":
      return pts.length >= 2 ? { kind: "circle", layer, stroke_width: widthNm, filled: false, center: p(0), end: p(1) } : null;
    case "arc":
      return pts.length >= 3 ? { kind: "arc", layer, stroke_width: widthNm, filled: false, start: p(0), mid: p(1), end: p(2) } : null;
    case "polygon":
      // The width and fill of a graphic polygon are `graphicPolygonStyle`'s (the caller passes its width): `commitZone` fills it except on Edge.Cuts and the courtyards.
      return pts.length >= 3 ? { kind: "polygon", layer, stroke_width: widthNm, filled: graphicPolygonStyle(undefined, layer).filled, pts: pts.map((_, i) => p(i)) } : null;
  }
}

const SHAPE_TOOL_KIND: Partial<Record<ToolId, ShapeToolKind>> = {
  draw_segment: "segment",
  draw_arc: "arc",
  draw_bezier: "bezier",
  draw_rect: "rect",
  draw_circle: "circle",
  draw_polygon: "polygon",
};

type DragState =
  | { kind: "pan"; button: 1 | 2; startScreen: [number, number]; startView: [number, number]; moved: boolean }
  | { kind: "move"; refs: string[]; pivotNm?: [number, number]; flipPivotNm?: [number, number]; startWorld: [number, number]; snapOrigin: [number, number]; moved: boolean }
  | { kind: "box"; startWorld: [number, number]; startScreen: [number, number] }
  /** pcb_point_editor.cpp: dragging one corner of the single selected zone's outline. `baseOutline` is a snapshot at drag-start, so every move computes fresh from it (no cumulative drift) -- same "delta from start" shape the move tool's own drag already uses. */
  | { kind: "zoneCorner"; zoneId: string; cornerIndex: number; baseOutline: [number, number][] }
  /** The same for a graphic shape's handle (kicad-port/pcbPointEdit.ts): `base` is the shape at drag-start, every move recomputes from it. */
  | { kind: "shapePoint"; shapeId: string; base: Shape; point: EditPoint }
  /** A selected tuning pattern's edit point (`PCB_TUNING_PATTERN::MakeEditPoints`) being dragged: the pattern is tuned again through an edit session (`genStartEdit` .. `genFinishEdit`). */
  | { kind: "tuneHandle"; drag: TuneHandleDrag };

/** A click/double-click/right-click within this many board µm of a zone corner counts as landing on it -- same generous, zoom-aware tolerance `ANCHOR_SNAP_NM`-adjacent code elsewhere in this file already uses. */
function zoneCornerToleranceNm(viewScale: number): number {
  return Math.max(300_000, 10 / viewScale);
}

/** Every part whose courtyard contains the point, topmost (last drawn) first. */
function partsAt(parts: Part[], xNm: number, yNm: number): Part[] {
  const hits: Part[] = [];
  for (let i = parts.length - 1; i >= 0; i--) {
    const p = parts[i]!;
    if (!p.placed || !p.courtyard) continue;
    const [x0, y0, x1, y1] = p.courtyard;
    if (xNm >= x0 && xNm <= x1 && yNm >= y0 && yNm <= y1) hits.push(p);
  }
  return hits;
}

function partHit(parts: Part[], xNm: number, yNm: number): Part | null {
  return partsAt(parts, xNm, yNm)[0] ?? null;
}

export function Canvas() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const { run } = useActionRunner();
  /** The registry of the latest render: an action run from the canvas (Close Outline, the Enter key) uses what the last render saw. */
  const runRef = useRef(run);
  runRef.current = run;
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);
  /** Ctrl (Cmd on a Mac) at the last pointer move: `drawShape`'s modifier for the rubber band (kicad-port/pcbShapeSnap.ts). */
  const drawCtrlRef = useRef(false);
  /** The pressed button's `BUTTON_STATE` (tool_dispatcher.cpp): whether the press has become a drag (kicad-port/dragThreshold.ts). Made at every press, read by the move, the up and the long press. */
  const gestureRef = useRef<ClickDragGesture | null>(null);
  const [marquee, setMarquee] = useState<{ x0: number; y0: number; x1: number; y1: number; crossing: boolean } | null>(null);
  /** pcb_point_editor.cpp's live corner-drag preview -- local, like `marquee` above, since only this component's own render loop needs it. */
  const [zoneCornerPreview, setZoneCornerPreview] = useState<{ zoneId: string; outline: [number, number][] } | null>(null);
  /** The live preview of a graphic shape's handle drag (the shape as it would be once dropped). */
  const [shapePointPreview, setShapePointPreview] = useState<{ id: string; shape: Shape } | null>(null);
  const moveMode = state.activeTool === "move";
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 });
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  /** The right-click menu of whichever tool owns the click (kicad-port/pcbContextMenu.ts): nested, with the menu bar's own entries. */
  const [pcbMenu, setPcbMenu] = useState<{ x: number; y: number; nodes: MenuNode[] } | null>(null);
  const longPressRef = useRef<{ timer: ReturnType<typeof setTimeout>; startScreen: [number, number] } | null>(null);
  // wx_view_controls.cpp LoadSettings(): the zoom controller is a
  // long-lived object across wheel events (ACCELERATING_ZOOM_CONTROLLER
  // specifically needs to remember the previous tick's timestamp/
  // direction to decide whether to accelerate) -- one instance per
  // mount, picked once for this platform, not reconstructed per wheel
  // event.
  // Preferences > Mouse and Touchpad: the wheel assignment and the zoom controller (rebuilt only when zoom speed/acceleration change).
  const wheelPrefs = useWheelPrefs();
  // Autopan (view_controls.cpp handleAutoPanning/onTimer) needs the
  // latest state/cursor position inside a self-scheduling
  // requestAnimationFrame loop without restarting that loop on every
  // render -- these refs are the loop's "current values" without being
  // render dependencies. lastPointerScreenRef is container-relative,
  // same space as computeAutoPanDirection expects.
  const stateRef = useRef(state);
  stateRef.current = state;
  const containerSizeRef = useRef(containerSize);
  containerSizeRef.current = containerSize;
  const lastPointerScreenRef = useRef<{ x: number; y: number } | null>(null);
  /** Set by onPointerUp when a right-button pan-drag just ended with real movement, so the native `contextmenu` event that follows (a separate, later browser event) knows to suppress the menu instead of opening it -- see onContextMenu. */
  const justPannedRef = useRef(false);
  /**
   * pcb_selection_tool.cpp Main(): a plain click on an item that's
   * already part of a multi-item selection does NOT collapse the
   * selection down to just that one item if a drag follows ("Check if
   * dragging has started within any of selected items bounding box" ->
   * doDrag=true, selectPoint never runs) -- only a genuine click with no
   * movement does. This app decides drag-vs-click only once movement
   * either does or doesn't happen, so the click's own selection-modifier
   * effect is deferred here at pointer-down and applied in onPointerUp
   * ONLY if the drag never actually moved anything.
   */
  const pendingClickRef = useRef<{ id: string; modifiers: ClickModifiers } | null>(null);
  /** Interactive router (gap #7): a mouse-move preview is a real HTTP round
   * trip now (the backend resolves walkaround/shove), so it needs the same
   * "don't flood the server, and never let a slow reply clobber a newer
   * one" treatment every other debounced network call gets. See
   * kicad-port/routeTool.ts's own doc comments on each. */
  const routeMoveThrottleRef = useRef(createMoveThrottle(50));
  const routeMoveGuardRef = useRef(createRequestGuard());

  const board = state.board;
  // The Appearance panel's settings as the painter reads them: opacities, the colour each net gets, the sheet (rebuilt only when they or the board change).
  const nets = useMemo(() => netsContext(board), [board]);
  const paintAppearance = useMemo(() => buildPaintAppearance(state.appearance, nets, board), [state.appearance, nets, board]);

  // The grid helper of the tool in force (kicad-port/pcbGridHelper.ts, pcbSnap.ts), kept fed with the view, the grid list and its overrides and the magnetic settings.
  const snapRef = useRef<PcbSnap | null>(null);
  if (!snapRef.current) snapRef.current = new PcbSnap();
  const snap = snapRef.current;
  const gridList = useGridSettings("pcb").grids;
  const gridOverrides = useGridOverrides("pcb");
  const [, bumpSnap] = useState(0);
  snap.update({ board, scale: state.view.scale, gridNm: state.gridNm, grids: gridList, overrides: gridOverrides, magnetic: toPcbMagnetic(state.prefs, state.magneticAllLayers), layerVisible: state.layerVisible, highContrast: state.highContrast, activeLayer: state.activeLayer, selection: state.selection });
  snap.onChange = () => bumpSnap((n) => n + 1);
  // A tool of its own starts a helper of its own: what the last one remembered (the anchor snapped to, the snap line, the construction geometry) goes with it.
  useEffect(() => {
    snap.reset();
    bumpSnap((n) => n + 1);
  }, [snap, state.activeTool]);
  useEffect(() => () => snap.reset(), [snap]);
  // the router's sessions start with the entered group, which their new items join (r5 R5-2)
  useEffect(() => setRouterEnteredGroup(state.enteredGroupId), [state.enteredGroupId]);

  // The canvas's backing size tracks its container's actual box, not a
  // value computed once at mount -- a side panel opening/closing or the
  // window resizing must resize the canvas too.
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

  // wx_view_controls.cpp handleAutoPanning/onTimer: while the cursor sits
  // in the border near a canvas edge DURING an active drag/draw
  // interaction, pan every frame, accelerating with how far past the
  // border it is. Off entirely unless state.prefs.autoPan (KiCad's own
  // default, see store.tsx; speed from Preferences) -- a persistent rAF loop rather than
  // per-dependency effect restarts, so it reads refs fresh each frame
  // instead of needing to be re-created on every state change.
  useEffect(() => {
    let raf = 0;
    const tick = () => {
      raf = requestAnimationFrame(tick);
      const s = stateRef.current;
      if (!s.prefs.autoPan) return;
      // Only while an interactive tool is actually running -- a box
      // select or move drag in progress, or a route/zone/shape tool
      // mid-click-sequence -- matching source's per-tool SetAutoPan(true)
      // gate (plain idle hover near the edge in the Select tool never
      // autopans in real KiCad either).
      const drag = dragRef.current;
      const interactive = drag?.kind === "box" || drag?.kind === "move" || s.drawState != null;
      const pointer = lastPointerScreenRef.current;
      if (!interactive || !pointer) return;
      const screenSize = containerSizeRef.current;
      const dir = computeAutoPanDirection(pointer, screenSize, DEFAULT_VIEW_CONTROL_SETTINGS.autoPanMargin);
      const step = computeAutoPanStep(dir, screenSize, s.view.scale, DEFAULT_VIEW_CONTROL_SETTINGS.autoPanMargin, s.prefs.autoPanAcceleration);
      if (!step) return;
      dispatch({ type: "SET_VIEW", view: panByWorldDelta(s.view, panDeltaX(s.bcx.boardFlipped, step.x), step.y) });
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [dispatch]);

  // Fit to the container until the user pans/zooms by hand (userMovedRef,
  // set in the wheel/pointer handlers below) -- same rule the old
  // studio.html used. Re-running this on every containerSize change (not
  // just once) matters because the container's true size often isn't
  // known yet on the very first layout pass (panels/toolbars still
  // settling), which previously left the view latched to a fit computed
  // against a too-small box.
  const userMovedRef = useRef(false);
  useEffect(() => {
    if (!board?.outline || userMovedRef.current) return;
    const bounds = boundsOfPoints(board.outline);
    if (!bounds || containerSize.width < 50 || containerSize.height < 50) return;
    dispatch({ type: "SET_VIEW", view: fitTransform(bounds, containerSize.width, containerSize.height) });
    dispatch({ type: "MARK_VIEW_INITIALIZED" });
  }, [board, containerSize, dispatch]);

  // The reference images are bitmaps that arrive after the board does: the canvas paints again when one has.
  const imageVersion = useImageVersion();
  // Render loop: repaint whenever anything visible changes.
  useEffect(() => {
    const canvas = canvasRef.current;
    // Nothing to paint until the first fit has given the view a real scale.
    if (!canvas || !board || containerSize.width === 0 || !(state.view.scale > 0)) return;
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
    ctx.clearRect(0, 0, width, height);
    // The whole viewport is the PCB background color, not just the area
    // inside the board outline -- matches KiCad, where there's no
    // "outside the board" canvas color distinct from the fill.
    ctx.fillStyle = layerColor("background");
    ctx.fillRect(0, 0, width, height);
    ctx.save();
    // pcbnew.Control.flipBoard (`view->SetMirror( m_FlipBoardView )`): the board seen from its other side, mirrored about the canvas middle.
    if (state.bcx.boardFlipped) {
      ctx.translate(width, 0);
      ctx.scale(-1, 1);
    }
    ctx.translate(state.view.x, state.view.y);
    ctx.scale(state.view.scale || 1, state.view.scale || 1);
    paintBoard(ctx, state.view, width, height, board, {
      selection: state.selection,
      hot: state.hot,
      netHighlight: state.bcx.netHighlightMore.length > 0 ? highlightedNets(state.netHighlight, state.bcx.netHighlightMore) : state.netHighlight,
      // The ratsnest lines `RATSNEST_VIEW_ITEM::ViewDraw` would draw: the global switch, hidden nets, the Local Ratsnest tool's pads and the visible-layers mode.
      showRatsnest: state.showRatsnest || state.bcx.localRatsnestPads.length > 0,
      ratsnestCurved: state.ratsnestCurved,
      ratsnestEdges: state.ratsnest
        ? displayedRatsnest(state.ratsnest.edges, {
            showGlobal: state.showRatsnest,
            mode: state.bcx.ratsnestMode,
            hiddenNets: new Set(state.bcx.hiddenRatsnestNets),
            flippedPads: new Set(state.bcx.localRatsnestPads),
            // The "visible layers" ratsnest mode counts the layers on; with the inactive layers hidden, the active one alone (`RATSNEST_VIEW_ITEM::ViewDraw`).
            visibleLayers: new Set(board.layers.flatMap((l, i) => ((state.highContrast && state.appearance.contrastHidden && state.activeLayer ? l === state.activeLayer : state.layerVisible[l] !== false) ? [i] : []))),
          })
        : null,
      tuneHover: state.pcbx.tuneHover,
      drcViolations: state.drc?.violations ?? null,
      drcStale: isStale(state.drcVersion, state.version),
      drcSelected: state.drcSelected,
      lintViolations: state.drcDialogOpen ? (state.lint?.pcb.violations ?? null) : null,
      lintSelected: state.drcLintSelected,
      zoneFill: state.zoneFill,
      zoneDisplayMode: state.zoneDisplayMode,
      currentViaPreset: state.currentViaPreset,
      units: state.units,
      zoneCornerPreview,
      shapePointPreview,
      layerVisible: state.layerVisible,
      layerOpacity: state.layerOpacity,
      activeLayer: state.activeLayer,
      highContrast: state.highContrast,
      gridNm: state.gridNm,
      gridVisible: state.gridVisible,
      movePreview: state.movePreview,
      sketchPads: state.sketchPads,
      sketchTracks: state.sketchTracks,
      sketchVias: state.sketchVias,
      sketchGraphics: state.bcx.sketchGraphics,
      sketchText: state.bcx.sketchText,
      showPadNumbers: state.bcx.showPadNumbers,
      auxOrigin: board.aux_origin ?? null,
      gridOrigin: board.grid_origin ?? null,
      drawState: state.drawState,
      cursorNm: state.cursorNm,
      snappedCursor: PLACING_TOOLS.has(state.activeTool) ? snap.lastPoint() : null,
      activeTool: state.activeTool,
      angleSnapMode: state.pcbx.angleSnapMode,
      drawCtrl: drawCtrlRef.current,
      enteredGroup: state.enteredGroupId,
      appearance: paintAppearance,
    });
    // What the grid helper shows (kicad-port/pcbGridHelper.ts `overlay()`): the snap marker and its icon, the snap guides and snap line, the construction geometry.
    // (The visible world is the same flipped or not: a mirrored screen x is still `width - x`.)
    const snapOverlay = snap.overlay();
    if (snapOverlay) {
      const k = state.view.scale || 1;
      paintSnapOverlay(ctx, k, [-state.view.x / k, -state.view.y / k, (width - state.view.x) / k, (height - state.view.y) / k], snapOverlay, { marker: layerColor("LAYER_AUX_ITEMS"), construction: layerColor("anchor"), guideActive: layerColor("anchor") });
    }
    ctx.restore();

    // Marquee (screen space, on top of everything).
    if (marquee) {
      const x = Math.min(marquee.x0, marquee.x1);
      const y = Math.min(marquee.y0, marquee.y1);
      ctx.fillStyle = marquee.crossing ? "rgba(74,163,255,0.12)" : "rgba(255,216,74,0.12)";
      ctx.strokeStyle = marquee.crossing ? "#4aa3ff" : "#ffd84a";
      ctx.setLineDash(marquee.crossing ? [4, 3] : []);
      ctx.lineWidth = 1;
      ctx.fillRect(x, y, Math.abs(marquee.x1 - marquee.x0), Math.abs(marquee.y1 - marquee.y0));
      ctx.strokeRect(x, y, Math.abs(marquee.x1 - marquee.x0), Math.abs(marquee.y1 - marquee.y0));
      ctx.setLineDash([]);
    }

    // The cursor crosshair is drawn by CommonOverlay (small, full-window or 45 degree; every editor shares it, and it follows the flipped board view).
    ctx.restore();
  }, [board, state.view, state.selection, state.hot, state.netHighlight, state.showRatsnest, state.ratsnestCurved, state.ratsnest, state.drc, state.drcVersion, state.version, state.drcSelected, state.drcDialogOpen, state.lint, state.drcLintSelected, state.layerVisible, state.layerOpacity, state.activeLayer, state.highContrast, state.gridNm, state.gridVisible, state.movePreview, state.cursorNm, state.fullscreenCrosshair, state.sketchPads, state.sketchTracks, state.sketchVias, state.drawState, state.activeTool, state.pcbx.angleSnapMode, state.pcbx.tuneHover, state.zoneFill, state.zoneDisplayMode, state.currentViaPreset, state.units, state.bcx, state.enteredGroupId, paintAppearance, marquee, zoneCornerPreview, shapePointPreview, containerSize, imageVersion]);

  const worldAt = useCallback(
    (e: { clientX: number; clientY: number }): [number, number] => {
      const rect = containerRef.current!.getBoundingClientRect();
      return screenToWorld(state.view, flipLocalX(state.bcx.boardFlipped, rect.width, e.clientX - rect.left), e.clientY - rect.top);
    },
    [state.view, state.bcx.boardFlipped]
  );

  /** The modifiers of an event as the grid helper takes them: Ctrl (Cmd on a Mac) turns the grid off (`TOOL_EVENT::DisableGridSnapping`), Shift the anchor snapping (`MD_SHIFT`). */
  const snapMods = (e: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }): SnapMods => ({ ctrl: isMac() ? e.metaKey : e.ctrlKey, shift: e.shiftKey });

  /**
   * Where the cursor of a placing tool goes: the tool's own `PCB_GRID_HELPER::BestSnapAnchor` call -- drawing_tool.cpp `drawShape` / `drawArc` / `DrawZone` snap on the active
   * layer and the graphics grid, `PlaceText` on the text grid, `DrawDimension` on every layer and the graphics grid, the picker and the ruler on every layer and the current
   * grid; the router's tools use its `snapToItem` instead (`routePoint`). Run on every pointer move (so the marker follows) and again for the press.
   */
  const toolPoint = (tool: string, wx: number, wy: number, e: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }): [number, number] => {
    const mods = snapMods(e);
    const draw = state.drawState;
    // `PlaceTuningPattern` reads `controls->GetMousePosition()`: the pattern follows the pointer, not the grid.
    if (tool === "tune") return [wx, wy];
    if (tool === "route" || tool === "diffpair" || tool === "drag") {
      const started = draw?.kind === "route" || draw?.kind === "diffpair" || draw?.kind === "drag";
      // what the cursor does not snap onto: the line a drag is dragging (`checkSnap`), the item a route started on (`pickSingleItem( .., { m_startItem } )`)
      const skip = draw?.kind === "drag" && draw.lineTracks?.length ? new Set(draw.lineTracks) : draw?.kind === "route" && draw.startItem ? new Set([draw.startItem]) : undefined;
      // `pickSingleItem`'s layer (r5 R5-1): a start takes the active layer's items first and then any layer's (`aLayer` -1, `tl` the top layer); a
      // route's or a pair's end only its current layer's (`updateEndItem`: `layer = m_router->GetCurrentLayer()`, -1 while placing a via)
      const routing = draw?.kind === "route" || draw?.kind === "diffpair" ? draw : null;
      const layer = routing ? (routing.placingVia ? { top: routing.layer } : { top: routing.layer, only: true }) : draw?.kind === "drag" ? undefined : { top: state.activeLayer ?? board?.layers[0] ?? "F.Cu" };
      const p = snap.routePoint([wx, wy], mods, { net: draw?.kind === "route" ? draw.net : null, isEnd: started, skip, layer });
      return [p[0], p[1]];
    }
    const opts: PointOpts = {};
    switch (tool) {
      case "via":
        opts.layers = "all";
        opts.category = "vias";
        break;
      case "text":
        opts.category = "text";
        break;
      case "dimension":
        opts.layers = "all";
        opts.category = "graphics";
        break;
      case "picker":
      case "measure":
      case "drill_origin":
      case "local_ratsnest":
        opts.layers = "all";
        break;
      default:
        opts.category = "graphics"; // the zone and the shape tools
    }
    // `grid.SetSkipPoint( cursorPos )` when a shape starts: it must not snap back onto its own start.
    opts.skipPoint = draw?.kind === "shape" && draw.pts.length > 0 ? draw.pts[0]! : null;
    const p = snap.point(tool, [wx, wy], mods, opts);
    return [p[0], p[1]];
  };

  /** `BestDragOrigin`: the point of the items picked up that the move starts from -- the nearest of their origins and corners to the mouse. */
  const moveOrigin = (wx: number, wy: number, refs: readonly string[]): [number, number] => {
    const p = snap.dragOrigin("move", [wx, wy], refs, state.selectionFilter);
    return [p[0], p[1]];
  };

  /**
   * The cursor of a move: `BestSnapAnchor( cursor, { active layer }, selection grid, selection )`. KiCad warps the pointer to the held point (`BestDragOrigin`) when the move
   * starts, so from then on the cursor is that point plus how far the pointer has travelled; a page may not move the pointer, so the same cursor is computed: `held` + (pointer now
   * - pointer at the start), which keeps the item under the pointer where it was grabbed.
   */
  const moveCursor = (wx: number, wy: number, e: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }, refs: readonly string[], held: readonly [number, number], grabbed: readonly [number, number]): [number, number] => {
    const p = snap.moveCursor(heldCursor(held, grabbed, [wx, wy]), snapMods(e), refs);
    return [p[0], p[1]];
  };

  /** A human-readable label for a disambiguation-menu row. */
  const candidateLabel = useCallback(
    (c: SelectionCandidate): string => {
      switch (c.kind) {
        case "part": {
          const p = api.partByRef(c.id);
          return `${c.id}${p?.value ? ` (${p.value})` : ""} [footprint]`;
        }
        case "track":
          return `Track [${api.trackById(c.id)?.layer ?? c.id}]`;
        case "via":
          return `Via`;
        case "zone":
          return `Zone [${api.zoneById(c.id)?.layer ?? c.id}]`;
        case "shape":
          return `${api.shapeById(c.id)?.kind ?? "Shape"} [${c.layer ?? ""}]`;
        case "text":
          return `Text "${api.textById(c.id)?.content ?? ""}"`;
        case "dimension": {
          const kind = api.dimensionById(c.id)?.kind ?? "dimension";
          return `${kind[0]!.toUpperCase()}${kind.slice(1)} Dimension`;
        }
        case "pad": {
          const hit = board ? padById(board, c.id) : null;
          return hit ? `Pad ${hit.pad.num} of ${hit.part.ref}${hit.pad.net ? ` (${hit.pad.net})` : ""}` : `Pad ${c.id}`;
        }
        case "field": {
          const hit = board ? fieldById(board, c.id) : null;
          return hit ? `${hit.field.name} field of ${hit.part.ref}${hit.field.name === "Reference" ? "" : ` (${hit.field.text})`}` : `Field ${c.id}`;
        }
        // `PCB_TABLE::GetItemDescription`, `PCB_TABLECELL::GetItemDescription`.
        case "table": {
          const t = board ? tableById(board, c.id) : undefined;
          return t ? `${t.cols} column table` : `Table ${c.id}`;
        }
        case "tablecell": {
          const hit = board ? cellById(board, c.id) : undefined;
          return hit ? `Table cell ${cellAddress(hit.cell.row, hit.cell.col)}` : `Table cell ${c.id}`;
        }
        default:
          // The text boxes, images, points and barcodes (`GetItemDescription`).
          return extraItemLabel(board, c.id);
      }
    },
    [api, board]
  );

  /**
   * pcb_selection_tool.cpp doSelectionMenu: where several items overlap
   * (GuessSelectionCandidates couldn't narrow it to one, or Alt/a
   * press-and-hold skipped straight past it), show a picker instead of
   * always taking the topmost -- plus source's own "Select All" entry,
   * which applies the SAME click modifier to every candidate at once
   * rather than just the one eventually clicked.
   */
  const disambiguate = useCallback(
    (candidates: SelectionCandidate[], modifiers: ClickModifiers, screenX: number, screenY: number) => {
      if (candidates.length === 0) return;
      if (candidates.length === 1) {
        dispatch({ type: "SET_SELECTION", refs: applySingleClickModifier(state.selection, candidates[0]!.id, modifiers) });
        return;
      }
      const pick = (c: SelectionCandidate) => dispatch({ type: "SET_SELECTION", refs: applySingleClickModifier(state.selection, c.id, modifiers) });
      const entries: MenuEntry[] = candidates.map((c) => ({ label: candidateLabel(c), onSelect: () => pick(c) }));
      entries.push({
        label: "Select All",
        onSelect: () => dispatch({ type: "SET_SELECTION", refs: applyBoxSelectionModifiers(state.selection, candidates.map((c) => c.id), modifiers) }),
      });
      setContextMenu({ x: screenX, y: screenY, entries });
    },
    [dispatch, state.selection, candidateLabel]
  );

  const clearLongPress = () => {
    if (longPressRef.current) {
      clearTimeout(longPressRef.current.timer);
      longPressRef.current = null;
    }
  };

  /** Route/zone/shape tools all share "click adds a point, Enter/double-click finishes" -- this commits whatever's accumulated in state.drawState, per its kind. Shared with the F ("Attempt Finish") hotkey for the route case specifically (useActionRunner.ts) via routing.ts's finishInteractiveRoute, so the two can never disagree about what finishing a route means. */
  const finishDraw = useCallback(() => {
    const draw = state.drawState;
    // Where the cursor is when a zone or polygon is finished (the leader and loop `SetFinished` sees): the snapped cursor, else the last corner.
    const finishCursor = (pts: readonly [number, number][]): [number, number] => {
      const at = snap.lastPoint() ?? (state.cursorNm ? snapPoint(state.cursorNm.x, state.cursorNm.y, state.gridNm) : pts[pts.length - 1]);
      return at ? [at[0], at[1]] : [0, 0];
    };
    if (!draw) return;
    if (draw.kind === "route") {
      const [x, y] = draw.pts[draw.pts.length - 1] ?? [draw.pts[0]?.[0] ?? 0, draw.pts[0]?.[1] ?? 0];
      void finishInteractiveRoute(x, y, dispatch, api);
    } else if (draw.kind === "diffpair") {
      // a pair that cannot be walked has no head (`tryWalkDp` empties it): finish where the cursor is
      const cursor = state.cursorNm;
      const [x, y] = draw.ptsA[draw.ptsA.length - 1] ?? (cursor ? [cursor.x, cursor.y] : [0, 0]);
      void finishDiffPairRoute(x, y, dispatch, api);
    } else if (draw.kind === "zone") {
      // `ZONE_CREATE_HELPER::OnComplete`: the locked corners, the leader's and loop's corners in the 45/90-degree modes, simplified (round 5 A5-5).
      const outline = zoneFinish(draw.pts, finishCursor(draw.pts), leaderModeOf(state.pcbx.angleSnapMode, drawCtrlRef.current));
      if (outline) {
        // zone_create_helper.cpp commitZone(): a cutout subtracts from its source zone and a
        // "similar" zone copies its settings -- neither shows the properties dialog (only
        // createNewZone() does); a plain draw still goes through ZoneDialog.
        const mode = state.pcbx.zoneDrawMode;
        const source = mode ? api.zoneById(mode.sourceId) : undefined;
        if (mode?.mode === "cutout" && source) {
          void api.cmd({ op: "zone_cutout", id: source.id, cutout: outline.map(([x, y]) => ({ x, y })) }).then(() => dispatch({ type: "SET_SELECTION", refs: [] }));
        } else if (mode?.mode === "similar" && source) {
          void api.addZone(source.net, source.layer, outline, zoneSettingsOf(source), { layers: [...zoneLayers(source)] });
        } else if (mode?.mode === "stitch") {
          // KiCad 10's `ZONE_MODE::STITCH` (zone_create_helper.cpp): a via stitching with the outline, its defaults filled in (`InitializeDefaults`), and its properties dialog before any via is made.
          openViaStitchDialog(null, newViaStitch(outline, board?.board_rules?.via_diameter ?? 0, board?.board_rules?.via_drill ?? 0));
        } else {
          dispatch({ type: "SET_ZONE_PENDING", outline });
        }
      }
      // `endPolygon`: `cleanup(); m_frame->PopTool( aEvent );` whether or not the outline made a zone.
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
      dispatch({ type: "PCBX", patch: { zoneDrawMode: null } });
      dispatch({ type: "SET_DRAW_STATE", draw: null });
    } else if (draw.kind === "shape") {
      // An arc ends only when its three points are in (ARC_GEOM_MANAGER); Enter / a double-click leave it running.
      if (draw.shapeKind === "arc") return;
      if (draw.shapeKind === "bezier") {
        // A double-click: "use the current point for all remaining points", accept, and reset (no chaining).
        const at = snap.lastPoint() ?? (state.cursorNm ? snapPoint(state.cursorNm.x, state.cursorNm.y, state.gridNm) : draw.bezier?.lastPoint);
        const curve = draw.bezier && at ? bezierFinishDouble(draw.bezier, at) : null;
        if (curve) void api.cmd({ op: "add_shape", shape: bezierShape(curve, activeLayerName(state.activeLayer, "F.SilkS"), drawStrokeWidthNm(board?.board_rules?.text_graphics, state.activeLayer, state.pcbx.drawStrokeStep)) });
        dispatch({ type: "SET_DRAW_STATE", draw: null });
        return;
      }
      // A double-click ends a chain of lines with nothing more to add (the segments are on the board already).
      if (draw.shapeKind === "segment") {
        dispatch({ type: "SET_DRAW_STATE", draw: null });
        return;
      }
      // A graphic polygon is `DrawZone`'s too: its outline is `OnComplete`'s (round 5 A5-5).
      const pts = draw.shapeKind === "polygon" ? (zoneFinish(draw.pts, finishCursor(draw.pts), leaderModeOf(state.pcbx.angleSnapMode, drawCtrlRef.current)) ?? []) : draw.pts;
      const layer = activeLayerName(state.activeLayer, "F.SilkS");
      // `DrawZone` never reads `m_stroke`: a graphic polygon is at its layer's own width (round 6 A6-2).
      const widthNm = draw.shapeKind === "polygon" ? graphicPolygonStyle(board?.board_rules?.text_graphics, layer).strokeWidthNm : drawStrokeWidthNm(board?.board_rules?.text_graphics, state.activeLayer, state.pcbx.drawStrokeStep);
      const shape = shapeFromPoints(draw.shapeKind, pts, layer, widthNm);
      if (shape) api.cmd({ op: "add_shape", shape });
      dispatch({ type: "SET_DRAW_STATE", draw: null });
      // A graphic polygon is `DrawZone`'s: finishing it pops the tool (`PopTool`), as a zone does.
      if (draw.shapeKind === "polygon") dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
    }
  }, [state.drawState, state.activeLayer, state.cursorNm, state.gridNm, state.pcbx, board, api, dispatch]);

  // pcbnew.InteractiveDrawing.closeOutline (drawing_tool.cpp: `polyGeomMgr.SetFinished()`) asks for what Enter does: finish the zone/polygon.
  const finishRequestSeen = useRef(state.pcbx.drawFinishRequest);
  useEffect(() => {
    if (state.pcbx.drawFinishRequest === finishRequestSeen.current) return;
    finishRequestSeen.current = state.pcbx.drawFinishRequest;
    finishDraw();
  }, [state.pcbx.drawFinishRequest, finishDraw]);

  const onPointerDown = (e: React.PointerEvent) => {
    // Throws NotFoundError for a synthesized pointer (common.Control.cursorClick's
    // Enter-key click, actions/useActionRunner.ts), which has no real pointer to capture.
    try {
      (e.target as Element).setPointerCapture(e.pointerId);
    } catch {
      /* synthetic pointer */
    }
    const gesture = new ClickDragGesture(dragRuleFor(isMac()));
    gesture.down(e.clientX, e.clientY, e.timeStamp);
    gestureRef.current = gesture;
    justPannedRef.current = false; // a new press: the last right-drag no longer vetoes the menu (the browsers that open it on the press fire it right after this)
    const [wx, wy] = worldAt(e);
    setContextMenu(null);
    setPcbMenu(null);
    if (state.pcbx.menuCursorNm) dispatch({ type: "PCBX", patch: { menuCursorNm: null } });

    // PICKER_TOOL::Main: while a pick session runs (actions/pcbPicker.ts) a left click answers it -- snapped like
    // PCB_GRID_HELPER::BestSnapAnchor, or the item under it for an item session -- and does nothing else.
    if (e.button === 0 && picker.session()) {
      const [px, py] = toolPoint("picker", wx, wy, e);
      picker.click({
        point: { x: px, y: py },
        item: () => {
          if (!board) return null;
          const toleranceNm = Math.max(150_000, 6 / state.view.scale);
          return pickSelectionCandidates(board, wx, wy, toleranceNm, 1 / state.view.scale, state.selectionFilter, state.layerVisible, state.activeLayer, state.highContrast, state.selection, false, false)[0]?.id ?? null;
        },
      });
      return;
    }

    // Route/via/zone/drawing/text tools: a click either starts, extends,
    // or (for via/text) completes one placement -- entirely separate
    // from the select/move flow below, which only applies to the
    // "select"/"move" tools.
    if (board && e.button === 0 && !e.altKey) {
      const [sx, sy] = toolPoint(state.activeTool, wx, wy, e);

      // `pcbnew.EditorControl.drillOrigin`: `DrillOrigin`'s picker click handler -- the drill/place file origin goes here and the tool is done
      // ("drill origin is a one-shot; don't continue with tool").
      if (state.activeTool === "drill_origin") {
        void api.cmd({ op: "set_aux_origin", at: { x: sx, y: sy } });
        dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
        return;
      }

      // `pcbnew.Control.localRatsnestTool`: `BOARD_INSPECTION_TOOL::LocalRatsnestTool`'s click handler. The pad under the cursor, else the
      // footprint, has its ratsnest shown or hidden; a click on neither resets every pad. The tool stays armed until Esc, which resets every pad too (the ESCAPE case in state/store.tsx).
      if (state.activeTool === "local_ratsnest") {
        const pad = padAt(board.parts, wx, wy);
        const part = pad ? board.parts.find((p) => p.ref === pad.ref) : partHit(board.parts, wx, wy);
        const flipped = state.bcx.localRatsnestPads;
        const next = pad ? toggleLocalRatsnestPad(flipped, pad.id) : part ? toggleLocalRatsnestFootprint(flipped, (part.pads ?? []).map((q) => `${part.ref}.${q.num}`), state.showRatsnest) : [];
        dispatch({ type: "BCX", patch: { localRatsnestPads: next } });
        return;
      }

      // `DRAWING_TOOL::DrawVia` (kicad-port/viaPlacer.ts): the via snaps onto the track, pad or graphic under the cursor, takes its net (or the zone fill's,
      // asking when there are several, or none: a free via), and the backend refuses it where it breaks a clearance and splits the track it is on (round 5 B5-1).
      if (state.activeTool === "via") {
        const rules = board.board_rules;
        // pcbnew.EditorControl.viaSizeInc/Dec's current pick (useActionRunner.ts), same board-default fallback the hotkey itself uses until it's ever pressed.
        const viaPreset = state.currentViaPreset ?? { diameter: rules?.via_diameter ?? 600_000, drill: rules?.via_drill ?? 300_000 };
        const copper = (board.layers ?? ["F.Cu", "B.Cu"]).filter((l) => l.endsWith(".Cu"));
        const mods = snapMods(e);
        const magnetic = toPcbMagnetic(state.prefs, state.magneticAllLayers);
        const placer: ViaPlacerInput = {
          board,
          diameter: viaPreset.diameter,
          // `bds.m_CurrentViaType` is always THROUGH: the via spans the board.
          layers: copper,
          magnetic: { tracks: magnetic.tracks, pads: magnetic.pads, graphics: magnetic.graphics },
          snap: !mods.shift,
          alignToSegment: (p, a, b) => snap.alignToSegment("via", p, a, b, mods),
          fills: (state.zoneFill?.zones ?? []).filter((z) => z.filled).map((z) => ({ net: z.net, layer: z.layer, fragments: z.fragments })),
          activeLayer: state.activeLayer ?? "F.Cu",
          highContrast: state.highContrast,
          isLayerVisible: (l) => layerIsVisible(state.layerVisible, l),
        };
        const at = snapViaPosition(placer, [wx, wy], [sx, sy]);
        const place = (net: string, track: string | null) =>
          void api.cmd({
            op: "place_via",
            net,
            x: Math.round(at[0]),
            y: Math.round(at[1]),
            drill: viaPreset.drill,
            diameter: viaPreset.diameter,
            from_layer: copper[0] ?? "F.Cu",
            to_layer: copper[copper.length - 1] ?? "B.Cu",
            split_track: track,
            allow_drc_violations: state.routerSettings.mode === "mark_obstacles" && state.routerSettings.allowDrcViolations,
          });
        const choice = viaNet(placer, at);
        if (choice.kind === "net") place(choice.net, choice.track);
        else {
          // `selectPossibleNetsByPopupMenu`: "The net assignment is ambiguous. Let the user decide." -- a cancelled menu places nothing.
          const labels = selectNetLabels(choice.nets);
          setContextMenu({
            x: e.clientX,
            y: e.clientY,
            entries: [{ label: SELECT_NET_TITLE, onSelect: () => {}, disabled: true }, { label: "", onSelect: () => {}, separator: true }, ...choice.nets.map((net, i) => ({ label: labels[i]!, onSelect: () => place(net, null) }))],
          });
        }
        return;
      }

      if (state.activeTool === "route") {
        const draw = state.drawState;
        if (!draw || draw.kind !== "route") {
          // X-start: `startInteractiveRoute` itself refuses (with a toast)
          // if there's nothing routable under the cursor -- no local
          // pre-check needed, the backend is the single source of truth
          // for "is this a valid start point" (pad/via/track-end -> a
          // real net), same as `isStartingPointRoutable` upstream.
          const layer = state.activeLayer ?? board.layers[0] ?? "F.Cu";
          // pcbnew.EditorControl.trackWidthInc/Dec's current pick (useActionRunner.ts) -- same fallback as the via preset above.
          // `ImportSizes`: with the width list on "use net class" (`UseNetClassTrack()`) no width is sent, and the server takes the start net's
          // (`CT_WIDTH` through the rules: its class, a .kicad_dru rule, a pair's class width). `autoTrackWidth` (`m_UseConnectedTrackWidth`): the
          // server's `inheritTrackWidth` first -- the width of the track the route starts on, or of the one joined to the pad or via whose far end
          // is nearest the cursor -- over either
          // the chosen entry read from the board's list now (`GetCurrentTrackWidth`: `m_TrackWidthList[ m_trackWidthIndex ]`, r5 R5-9)
          const width = chosenTrackWidthOf(board.routing?.track_width_presets ?? [], state.currentTrackWidthIndex, state.pcbx.useCustomTrackVia, state.pcbx.customTrackVia.trackNm);
          clearRouteQueue(); // a route the user starts by hand is no longer RouteSelected's loop
          void startInteractiveRoute(sx, sy, layer, width, withTrackSnapping(state.routerSettings, state.prefs.magneticTracks), dispatch, snap.routePicked(), state.bcx.autoTrackWidth);
          return;
        }
        void fixInteractiveRoute(sx, sy, draw, dispatch, api);
        return;
      }

      // `6` (gap #7 task item 6, pcbnew.InteractiveRouter.DiffPair): same
      // arm-then-click-to-start/fix flow as the route tool above, just
      // through diffPairRouting.ts's own pair of start/fix calls.
      if (state.activeTool === "diffpair") {
        const draw = state.drawState;
        if (!draw || draw.kind !== "diffpair") {
          const layer = state.activeLayer ?? board.layers[0] ?? "F.Cu";
          // `ImportSizes`: the board's custom pair dimensions while `UseCustomDiffPairDimensions()`, else the chosen entry of the pre-defined pairs
          // (`GetCurrentDiffPairWidth/Gap/ViaGap()`, r5 R5-5); the net class's at entry 0
          const custom = state.pcbx.customDiffPair;
          const dims = chosenDiffPair(board.routing?.diff_pair_presets ?? [], state.pcbx.diffPairIndex, state.pcbx.useCustomDiffPair, custom ? { width: custom.widthNm, gap: custom.gapNm, viaGap: custom.viaGapSameAsTrackGap ? custom.gapNm : custom.viaGapNm } : null);
          void startDiffPairRoute(sx, sy, layer, dispatch, dims, withTrackSnapping(state.routerSettings, state.prefs.magneticTracks), state.bcx.autoTrackWidth);
          return;
        }
        void fixDiffPairRoute(sx, sy, draw, dispatch, api);
        return;
      }

      // `7` / `8` / `9` (P5c, `DRAWING_TOOL::PlaceTuningPattern`): the first click starts a tuning pattern on the track under the cursor, the second puts it on the
      // board (or, for a pattern of the board being edited through its edit points, finishes the edit).
      if (state.activeTool === "tune") {
        const draw = state.drawState;
        if (draw?.kind === "tune") void (draw.editing ? finishEditPattern(dispatch, api) : fixTune(state, dispatch, api, wx, wy));
        else void startTune(state, dispatch, wx, wy);
        return;
      }

      // `D` (gap #7 stage 5, pcbnew.InteractiveRouter.Drag45Degree): the
      // drag session itself is started by the hotkey the moment it's
      // pressed (useActionRunner.ts -- there's no separate "click to
      // start" step the way the route tool's `X` has, matching source's
      // own one-shot `InlineDrag` activation), so a click here only ever
      // commits it -- see dragging.ts's own doc on why that's always a
      // finish, never a "fix this leg and keep going".
      if (state.activeTool === "drag") {
        if (state.drawState?.kind === "drag") void finishInlineDrag(sx, sy, dispatch, api, snapMods(e).ctrl);
        else dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
        return;
      }

      // `DRAWING_TOOL::DrawZone` (kicad-port/pcbDrawZone.ts): a click on the first corner closes the outline; the line mode bends the leg in.
      if (state.activeTool === "zone") {
        const draw = state.drawState;
        const click = zoneClick(draw?.kind === "zone" ? draw.pts : [], [sx, sy], leaderModeOf(state.pcbx.angleSnapMode, snapMods(e).ctrl));
        // `ZONE_CREATE_HELPER::OnFirstPoint`: `RunAction( ACTIONS::selectionClear )` (round 5 A5-6).
        if (draw?.kind !== "zone") dispatch({ type: "SET_SELECTION", refs: [] });
        if (click.finish) finishDraw();
        else dispatch({ type: "SET_DRAW_STATE", draw: { kind: "zone", pts: click.pts } });
        return;
      }

      // Task item 7: two clicks (start, end) for whichever of the five
      // kinds `state.nextDimensionKind` names -- see DimensionPropertiesDialog.tsx
      // and PARITY-pcb.md section 18 for why this is a plain two-click
      // commit (defaults filled from Board Setup's DimensionSettings,
      // edited right after) rather than source's own third "set height"
      // click with a live preview.
      if (state.activeTool === "dimension") {
        const draw = state.drawState;
        if (draw?.kind === "dimension" && draw.pts.length === 1) {
          const start = draw.pts[0]!;
          const end: [number, number] = [sx, sy];
          dispatch({ type: "SET_DRAW_STATE", draw: null });
          dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
          const settings = board.drawings?.dimension_settings;
          if (settings) {
            const payload = defaultDimensionPayload(state.nextDimensionKind, start, end, activeLayerName(state.activeLayer, "Dwgs.User"), settings, board.board_rules?.text_graphics);
            void api.addDimension(payload).then((id) => {
              if (id) dispatch({ type: "SET_DIMENSION_EDIT_ID", id });
            });
          }
        } else {
          dispatch({ type: "SET_DRAW_STATE", draw: { kind: "dimension", pts: [[sx, sy]] } });
        }
        return;
      }

      // `DRAWING_TOOL::DrawTable`: the first click is the table's first corner, the cursor sizes its grid, and the second click opens Table Properties on it (OK adds the
      // table as one step, "Draw Table"; Cancel adds nothing). The tool stays armed for the next table.
      if (state.activeTool === "table") {
        const draw = state.drawState;
        if (draw?.kind === "table") {
          const s = drawTableSession(board.board_rules?.text_graphics, state.activeLayer, draw.origin, [sx, sy], state.gridNm);
          dispatch({ type: "SET_DRAW_STATE", draw: null });
          openDrawnTableDialog({ layer: s.layer, at: draw.origin, cols: s.grid.cols, rows: s.grid.rows, cellW: s.grid.cellW, cellH: s.grid.cellH, lineWidth: s.lineWidth });
        } else {
          dispatch({ type: "SET_SELECTION", refs: [] });
          dispatch({ type: "SET_DRAW_STATE", draw: { kind: "table", origin: [sx, sy] } });
        }
        return;
      }

      if (state.activeTool === "measure") {
        // pcb_viewer_tools.cpp's ruler: a 1-point measurement is still
        // being dragged (rubber-banded to the cursor in painter.ts) -- a
        // second click fixes the end point and the ruler stays on screen.
        // Any click after that (2 points already fixed) starts a fresh
        // measurement from scratch, same as clicking the first point.
        const draw = state.drawState;
        if (draw?.kind === "measure" && draw.pts.length === 1) {
          dispatch({ type: "SET_DRAW_STATE", draw: { kind: "measure", pts: [...draw.pts, [sx, sy]] } });
        } else {
          dispatch({ type: "SET_DRAW_STATE", draw: { kind: "measure", pts: [[sx, sy]] } });
        }
        return;
      }

      const shapeKind = SHAPE_TOOL_KIND[state.activeTool];
      // DRAWING_TOOL::drawArc: centre, start, end -- ARC_GEOM_MANAGER decides what each click means and when the arc is done.
      if (shapeKind === "arc") {
        const draw = state.drawState;
        const prev = draw?.kind === "shape" && draw.shapeKind === "arc" ? draw.arc : undefined;
        // `drawArc`: `if( !started ) m_toolMgr->RunAction( ACTIONS::selectionClear );`
        if (!prev) dispatch({ type: "SET_SELECTION", refs: [] });
        const { geom, arc } = arcClick(prev, [sx, sy], arcAngleSnap(state.pcbx.angleSnapMode, e));
        if (arc) {
          const shape: CmdShape = { kind: "arc", layer: activeLayerName(state.activeLayer, "F.SilkS"), stroke_width: drawStrokeWidthNm(board?.board_rules?.text_graphics, state.activeLayer, state.pcbx.drawStrokeStep), filled: false, start: ptXY(arc.start), mid: ptXY(arc.mid), end: ptXY(arc.end) };
          void api.cmd({ op: "add_shape", shape });
        }
        dispatch({ type: "SET_DRAW_STATE", draw: geom ? { kind: "shape", shapeKind: "arc", pts: arcClickPoints(geom), arc: geom } : null });
        return;
      }
      // DRAWING_TOOL::drawOneBezier (via DrawBezier's chaining loop): start, control 1, end, control 2.
      if (shapeKind === "bezier") {
        const draw = state.drawState;
        const prev = draw?.kind === "shape" && draw.shapeKind === "bezier" ? draw.bezier : undefined;
        // `drawOneBezier`: the first click clears the selection.
        if (!prev) dispatch({ type: "SET_SELECTION", refs: [] });
        const { geom, curve } = bezierClick(prev, [sx, sy]);
        if (curve) void api.cmd({ op: "add_shape", shape: bezierShape(curve, activeLayerName(state.activeLayer, "F.SilkS"), drawStrokeWidthNm(board?.board_rules?.text_graphics, state.activeLayer, state.pcbx.drawStrokeStep)) });
        dispatch({ type: "SET_DRAW_STATE", draw: geom ? { kind: "shape", shapeKind: "bezier", pts: [], bezier: geom } : null });
        return;
      }
      // A graphic polygon is `DrawZone`'s (`ZONE_MODE::GRAPHIC_POLYGON`): its clicks are a zone's.
      if (shapeKind === "polygon") {
        const draw = state.drawState;
        const click = zoneClick(draw?.kind === "shape" && draw.shapeKind === "polygon" ? draw.pts : [], [sx, sy], leaderModeOf(state.pcbx.angleSnapMode, snapMods(e).ctrl));
        // `ZONE_CREATE_HELPER::OnFirstPoint` clears the selection (round 5 A5-6).
        if (!(draw?.kind === "shape" && draw.shapeKind === "polygon")) dispatch({ type: "SET_SELECTION", refs: [] });
        if (click.finish) finishDraw();
        else dispatch({ type: "SET_DRAW_STATE", draw: { kind: "shape", shapeKind: "polygon", pts: click.pts } });
        return;
      }
      if (shapeKind) {
        const draw = state.drawState;
        const already = draw?.kind === "shape" && draw.shapeKind === shapeKind ? draw.pts : [];
        // `drawShape`: `if( !started ) { m_toolMgr->RunAction( ACTIONS::selectionClear ); ...`
        if (already.length === 0) dispatch({ type: "SET_SELECTION", refs: [] });
        let point: [number, number] = [sx, sy];
        if (already.length > 0 && (shapeKind === "segment" || shapeKind === "rect" || shapeKind === "circle")) {
          // `drawShape`: the snapped cursor's vector from the first point through `GetVectorSnapped45`/`90` -- a line by the line mode (Ctrl frees it), a
          // rectangle or circle free (Ctrl: 45 degrees, a rectangle square). The cursor is on the grid already, and the snap keeps its x or y (round 5 A5-2/3).
          point = shapeSecondPoint(shapeKind, state.pcbx.angleSnapMode, snapMods(e).ctrl, already[already.length - 1]!, [sx, sy]);
        }
        const pts = [...already, point];
        const finishAt = shapeAutoFinishCount(shapeKind);
        // `DrawLine` loops `while( drawShape( .. ) )`: each segment is committed ("Draw Line") and the next starts at its end, until a click on the same spot
        // (`twoPointMgr.IsEmpty()`), a double-click, or a click on a graphic's anchor that closes a path (`grid.GetSnapped()` a `PCB_SHAPE`).
        if (shapeKind === "segment" && already.length > 0) {
          const start = already[0]!;
          const committed = draw?.kind === "shape" ? (draw.committed ?? []) : [];
          if (point[0] === start[0] && point[1] === start[1]) {
            dispatch({ type: "SET_DRAW_STATE", draw: null });
            return;
          }
          const shape = shapeFromPoints("segment", [start, point], activeLayerName(state.activeLayer, "F.SilkS"), drawStrokeWidthNm(board?.board_rules?.text_graphics, state.activeLayer, state.pcbx.drawStrokeStep));
          if (shape) void api.cmd({ op: "add_shape", shape });
          const snapped = snap.snappedItemId();
          const closesPath = snapped !== null && (board.drawings?.shapes ?? []).some((q) => q.id === snapped);
          dispatch({ type: "SET_DRAW_STATE", draw: closesPath ? null : { kind: "shape", shapeKind: "segment", pts: [point], committed: [...committed, [start, point]] } });
          return;
        }
        if (finishAt !== null && pts.length >= finishAt) {
          const shape = shapeFromPoints(shapeKind, pts, activeLayerName(state.activeLayer, "F.SilkS"), drawStrokeWidthNm(board?.board_rules?.text_graphics, state.activeLayer, state.pcbx.drawStrokeStep));
          if (shape) api.cmd({ op: "add_shape", shape });
          dispatch({ type: "SET_DRAW_STATE", draw: null });
        } else {
          dispatch({ type: "SET_DRAW_STATE", draw: { kind: "shape", shapeKind, pts } });
        }
        return;
      }

      // `DRAWING_TOOL::PlaceText`: the first click clears the selection and opens the dialog on a new text with the layer's defaults; the text then follows the
      // cursor (`drawState` "text", painted by painter.ts), and the next click places it ("Draw Text") and selects it. The tool stays armed (round 5 B5-2).
      if (state.activeTool === "text") {
        const draw = state.drawState;
        if (draw?.kind === "text") {
          const before = new Set((board.drawings?.texts ?? []).map((t) => t.id));
          dispatch({ type: "SET_DRAW_STATE", draw: null });
          void api.cmd({ op: "place_text", text: { ...draw.text, at: { x: sx, y: sy } }, extra: draw.extra }).then(() => {
            // (the board as the backend has it now: the store's catches up on the next render)
            void fetchState().then((now) => {
              const made = (now.drawings?.texts ?? []).find((t) => !before.has(t.id));
              if (made) dispatch({ type: "SET_SELECTION", refs: [made.id] }); // `selectItem, text`
            });
          });
          return;
        }
        if (state.textDialog) return;
        dispatch({ type: "SET_SELECTION", refs: [] }); // `ACTIONS::selectionClear`
        dispatch({ type: "SET_TEXT_DIALOG", dialog: { mode: "add", at: [sx, sy] } });
        return;
      }
    }

    // pcb_point_editor.cpp: with exactly one zone selected and the plain
    // Select tool active, its outline corners are draggable handles --
    // checked before the ordinary click/select/box-select logic below so
    // grabbing one never instead starts a move or a box-select. Uses the
    // unsnapped click point for hit-testing (a handle's board-space size
    // is what the operator sees and aims for) but snaps the drag itself,
    // same as every other click-to-place interaction here.
    if (board && e.button === 0 && !e.altKey && state.activeTool === "select" && state.selection.size === 1) {
      const soleId = [...state.selection][0]!;
      const zone = api.zoneById(soleId);
      // `PCB_POINT_EDITOR::OnSelectionChange`: `if( !item || item->IsLocked() ) return 0;` -- a locked zone (or one in a locked group) has no handles.
      if (zone && !isLocked(board, zone.id)) {
        const idx = findNearestCorner(zone.outline, wx, wy, zoneCornerToleranceNm(state.view.scale));
        if (idx != null) {
          dragRef.current = { kind: "zoneCorner", zoneId: zone.id, cornerIndex: idx, baseOutline: zone.outline };
          setZoneCornerPreview({ zoneId: zone.id, outline: zone.outline });
          return;
        }
      }
      // A selected tuning pattern's four edit points: the start, the end, the width of the meanders and their spacing -- dragging one tunes the pattern again.
      const pattern = board.drawings?.tuning_patterns?.find((p) => p.id === soleId);
      if (pattern?.handles) {
        const h = handleAt(pattern.handles, { x: wx, y: wy }, zoneCornerToleranceNm(state.view.scale));
        if (h != null) {
          dragRef.current = { kind: "tuneHandle", drag: new TuneHandleDrag(dispatch, pattern, h) };
          return;
        }
      }
      // The same for a single selected graphic shape's handles (kicad-port/pcbPointEdit.ts), unless it is locked.
      const shape = api.shapeById(soleId);
      // `if( !item || item->IsLocked() )`: its own lock or a locked group above it
      if (shape && !isLocked(board, soleId)) {
        const hit = activeEditPoint(shapeEditPoints(shape), [wx, wy], zoneCornerToleranceNm(state.view.scale));
        if (hit) {
          dragRef.current = { kind: "shapePoint", shapeId: shape.id, base: shape, point: hit };
          setShapePointPreview(null);
          return;
        }
      }
    }

    if (moveMode) {
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
      if (state.movePreview) api.commitMove(state.movePreview.refs, state.movePreview.dxNm, state.movePreview.dyNm, state.movePreview.kind, state.movePreview.rotateQuarterTurns, state.movePreview.flipped, state.movePreview.perRefOffsetNm, state.movePreview.ops);
      return;
    }
    if (state.armed) {
      const [sx, sy] = snap.align("place", [wx, wy], snapMods(e), "connectable");
      api.placeArmedAt(sx, sy);
      return;
    }
    if (state.armedFootprint) {
      const [sx, sy] = snap.align("place", [wx, wy], snapMods(e), "connectable");
      void api.placeLibraryFootprintAt(sx, sy);
      return;
    }
    if (PAN_BUTTONS.has(e.button)) {
      // wx_view_controls.cpp onButton: MiddleDown/RightDown both start
      // DRAG_PANNING by default. A right-button press might still turn
      // out to be a plain click (-> the context menu, via the browser's
      // own native `contextmenu` event on button-up) rather than a drag;
      // onContextMenu below tells the two apart by whether real movement
      // happened.
      dragRef.current = { kind: "pan", button: e.button as 1 | 2, startScreen: [e.clientX, e.clientY], startView: [state.view.x, state.view.y], moved: false };
      return;
    }
    if (e.button !== 0) return;

    pendingClickRef.current = null;
    const ctrlOrCmd = isMac() ? e.metaKey : e.ctrlKey;
    const modifiers = computeClickModifiers(e.shiftKey, ctrlOrCmd, e.altKey);
    // A left click on a DRC marker selects it: `PCB_SELECTION_TOOL::selectPoint` takes a `PCB_MARKER` like any item, and `DRC_TOOL::CrossProbe` then selects its row in the
    // dialog when that is shown. It is the selection, the items that were selected are not; Alt+click still gets the list of everything under the pointer.
    if (state.activeTool === "select" && !hasModifier(modifiers) && !e.altKey) {
      const markerIndex = drcMarkerAt(state, wx, wy);
      if (markerIndex >= 0) {
        selectMarkerDrc({ api, dispatch }, markerIndex);
        return;
      }
    }
    const toleranceNm = Math.max(150_000, 6 / state.view.scale);
    const onePixelNm = 1 / state.view.scale;
    // pcb_selection_tool.cpp selectPoint: "if( m_enteredGroup && !m_enteredGroup->GetBoundingBox().Contains( aWhere ) ) ExitGroup();" -- a click
    // outside the box of the entered group leaves it, then picks as with no group entered. Inside it, only the group's own members and the groups
    // directly in it can be picked (`FilterCollectorForHierarchy`: "If a group is entered, disallow selections of objects outside the group").
    let scope = state.enteredGroupId;
    if (board && scope) {
      const box = itemBounds(board, scope);
      if (!box || wx < box[0] || wx > box[2] || wy < box[1] || wy > box[3]) {
        dispatch({ type: "SET_ENTERED_GROUP", id: null });
        scope = null;
      }
    }
    const groupsNow = board?.drawings?.groups ?? [];
    const pickable = (id: string): boolean => scope === null || (board != null && withinScope(groupsNow, padParent(board, id) ?? id, scope));
    const runPick = (skipHeuristics: boolean): SelectionCandidate[] =>
      board ? pickSelectionCandidates(board, wx, wy, toleranceNm, onePixelNm, state.selectionFilter, state.layerVisible, state.activeLayer, state.highContrast, state.selection, modifiers.subtractive, skipHeuristics).filter((c) => pickable(c.id)) : [];

    // pcb_selection_tool.cpp SELECTION_TOOL::hasModifier's `m_skip_heuristics`
    // (Alt): go straight to the full candidate list/clarification menu,
    // same as a long-press (disambiguateCursor forces the same thing).
    if (e.altKey) {
      disambiguate(runPick(true), modifiers, e.clientX, e.clientY);
      return;
    }

    // Long-press (ADVANCED_CFG::m_DisambiguationMenuDelay, 500ms default
    // -- LONG_PRESS_MS): if the pointer stays down here without much
    // movement, fire the same disambiguation a moment from now,
    // pre-empting whatever plain click/drag was about to happen.
    const startScreen: [number, number] = [e.clientX, e.clientY];
    clearLongPress();
    longPressRef.current = {
      startScreen,
      timer: setTimeout(() => {
        longPressRef.current = null;
        dragRef.current = null;
        setMarquee(null);
        disambiguate(runPick(true), modifiers, startScreen[0], startScreen[1]);
      }, LONG_PRESS_MS),
    };

    const candidates = runPick(false);

    if (candidates.length > 1) {
      clearLongPress();
      disambiguate(candidates, modifiers, e.clientX, e.clientY);
      return;
    }

    if (candidates.length === 1) {
      clearLongPress();
      const hit = candidates[0]!;

      if (hasModifier(modifiers)) {
        // pcb_selection_tool.cpp Main(): `hasModifier()` alone routes a
        // drag to SelectRectArea/SelectMultiple -- a modifier+drag is
        // ALWAYS a box-select, even one starting right on top of a hit
        // item, never an item move. Defer the click's own effect (a
        // plain Shift/Ctrl+click with no movement still has to toggle/
        // add/remove exactly this one item) until onPointerUp knows
        // whether a real drag happened; a real drag discards this and
        // re-derives the result from the box's actual contents instead
        // (see that handler -- the two converge to the same answer for
        // a single-item "box" anyway, except a literal zero-movement
        // click can't geometrically contain/touch anything, which is
        // exactly why this deferral exists).
        pendingClickRef.current = { id: hit.id, modifiers };
        dragRef.current = { kind: "box", startWorld: [wx, wy], startScreen: [e.clientX, e.clientY] };
        return;
      }

      // pcb_selection_tool.cpp Main(): "Check if dragging has started
      // within any of selected items bounding box" -- a plain click (no
      // modifier) on an item that's already part of a larger selection
      // keeps the WHOLE group for a potential drag, deferring the
      // click's own "collapse to just this one" effect until/unless the
      // drag turns out not to move anything (onPointerUp).
      const keepGroupForDrag = state.selection.has(hit.id) && state.selection.size > 1;
      let refs: string[];
      if (keepGroupForDrag) {
        refs = [...state.selection];
        pendingClickRef.current = { id: hit.id, modifiers };
      } else {
        refs = applySingleClickModifier(state.selection, hit.id, modifiers);
        dispatch({ type: "SET_SELECTION", refs });
      }
      if (DRAGGABLE_KINDS.has(hit.kind) && refs.length > 0 && board) {
        // What the drag picks up: the selection as the click leaves it (a group stands for its members, a pad for its footprint), locked items out
        // (`FilterCollectorForLockedItems`), and the points an R / F pressed on the way acts about.
        const start = carryStart(board, withGroupSubstitution(refs, board.drawings?.groups, state.enteredGroupId), modificationSnapFor(state));
        if (start.refs.length > 0) {
          dragRef.current = { kind: "move", refs: start.refs, pivotNm: start.pivotNm, flipPivotNm: start.flipPivotNm, startWorld: [wx, wy], snapOrigin: moveOrigin(wx, wy, start.refs), moved: false };
        } else if (start.lockedOut) {
          dispatch({ type: "TOAST", message: "Selection contains locked items.", kind: "info" });
        }
      }
      return;
    }

    // Nothing under the cursor: box select, or (no modifier) a plain
    // click that just clears whatever was selected.
    dragRef.current = { kind: "box", startWorld: [wx, wy], startScreen: [e.clientX, e.clientY] };
    if (!hasModifier(modifiers)) dispatch({ type: "CLEAR_SELECTION" });
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const [wx, wy] = worldAt(e);
    drawCtrlRef.current = snapMods(e).ctrl;
    dispatch({ type: "SET_CURSOR", at: { x: wx, y: wy } });
    const containerRectForAutoPan = containerRef.current?.getBoundingClientRect();
    if (containerRectForAutoPan) lastPointerScreenRef.current = { x: e.clientX - containerRectForAutoPan.left, y: e.clientY - containerRectForAutoPan.top };

    // tool_dispatcher.cpp handleMouseButton: a held button becomes a drag after 8 px (or, on macOS, 300 ms) -- not at the first grid step of travel.
    const motion = gestureRef.current?.move(e.clientX, e.clientY, e.timeStamp);
    if (longPressRef.current && motion?.dragging) clearLongPress();

    // The snapping of the tool in force follows the cursor, as `BestSnapAnchor` runs on every event of a KiCad tool: the marker, the snap lines and the construction
    // geometry are the helper's state after this call (and the click that follows takes the same point).
    const tracking = picker.session() != null ? "picker" : state.activeTool;
    const snapped: [number, number] | null = board && !dragRef.current && !moveMode && PLACING_TOOLS.has(tracking) ? toolPoint(tracking, wx, wy, e) : null;

    // Interactive router (gap #7) live preview: every move asks the
    // backend to re-resolve the head toward the cursor (walkaround/shove/
    // mark-obstacles, same as real pcbnew's router_tool.cpp mouse-move
    // handler) -- throttled and guarded against an out-of-order reply the
    // same way the rest of this app's async calls are (see
    // kicad-port/routeTool.ts).
    if (board && state.activeTool === "route" && state.drawState?.kind === "route") {
      const draw = state.drawState;
      if (routeMoveThrottleRef.current.shouldSend(performance.now())) {
        const [sx, sy] = snapped ?? toolPoint("route", wx, wy, e);
        const token = routeMoveGuardRef.current.next();
        routeMove(sx, sy).then((preview) => {
          if (!routeMoveGuardRef.current.isCurrent(token) || !preview.ok) return;
          dispatch({ type: "SET_DRAW_STATE", draw: drawStateFromPreview(draw, preview) });
        });
      }
    }

    // `6`'s own live preview -- same throttle/guard pair as the route tool
    // above (route/drag/diff-pair sessions are mutually exclusive, see
    // crates/pns/src/router.rs).
    if (board && state.activeTool === "diffpair" && state.drawState?.kind === "diffpair") {
      const draw = state.drawState;
      if (routeMoveThrottleRef.current.shouldSend(performance.now())) {
        const [sx, sy] = snapped ?? toolPoint("diffpair", wx, wy, e);
        const token = routeMoveGuardRef.current.next();
        dpMove(sx, sy).then((preview) => {
          if (!routeMoveGuardRef.current.isCurrent(token) || !preview.ok) return;
          dispatch({ type: "SET_DRAW_STATE", draw: dpStateFromPreview(draw, preview) });
        });
      }
    }

    // `7` / `8` / `9`: the status box of the track under the cursor, or -- with a pattern started -- the meander fitted from its start to the cursor (same throttle and
    // guard as the router's previews above).
    if (board && state.activeTool === "tune" && routeMoveThrottleRef.current.shouldSend(performance.now())) {
      const token = routeMoveGuardRef.current.next();
      if (state.drawState?.kind === "tune") void moveTune(state, dispatch, wx, wy, () => routeMoveGuardRef.current.isCurrent(token));
      else void hoverTune(state, dispatch, wx, wy);
    }

    // `D`'s own live preview -- same throttle/guard pair as the route tool
    // above (never both active at once: `Router` keeps a route session and
    // a drag session mutually exclusive, see crates/pns/src/router.rs).
    if (board && state.activeTool === "drag" && state.drawState?.kind === "drag") {
      const draw = state.drawState;
      if (routeMoveThrottleRef.current.shouldSend(performance.now())) {
        const [sx, sy] = snapped ?? toolPoint("drag", wx, wy, e);
        const token = routeMoveGuardRef.current.next();
        routeDragMove(sx, sy).then((preview) => {
          if (!routeMoveGuardRef.current.isCurrent(token) || !preview.ok) return;
          dispatch({ type: "SET_DRAW_STATE", draw: dragStateFromPreview(draw, preview) });
        });
      }
    }

    // `drawArc` / `drawOneBezier`'s motion branch: update the construction manager's geometry (never its step) with the snapped cursor.
    if (board && state.drawState?.kind === "shape" && (state.drawState.arc || state.drawState.bezier)) {
      const draw = state.drawState;
      const [sx, sy] = snapped ?? toolPoint(state.activeTool, wx, wy, e);
      if (draw.arc) dispatch({ type: "SET_DRAW_STATE", draw: { ...draw, arc: arcMotion(draw.arc, [sx, sy], arcAngleSnap(state.pcbx.angleSnapMode, e)) } });
      else if (draw.bezier) dispatch({ type: "SET_DRAW_STATE", draw: { ...draw, bezier: bezierMotion(draw.bezier, [sx, sy]) } });
    }

    if (moveMode && state.selection.size > 0 && board) {
      const origin = state.moveOriginNm ?? { x: wx, y: wy };
      // Pack and Move (`P`) carries footprints, each with its own packed shift; every other move is the one `pcb` kind -- any mix of items, taken in the hand
      // once (the first pointer move) so the points an R / F acts about stay where the selection was picked up.
      const packing = state.movePreview?.perRefOffsetNm != null;
      const held = state.movePreview?.kind === "pcb" ? state.movePreview : null;
      const start = packing || held ? null : carryStart(board, [...state.selection], modificationSnapFor(state));
      if (!packing && !held && start && start.refs.length === 0) {
        if (start.lockedOut) dispatch({ type: "TOAST", message: "Selection contains locked items.", kind: "info" });
        dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
        return;
      }
      const kind = packing ? "part" : "pcb";
      const refs = packing ? [...state.selection] : (held?.refs ?? start!.refs);
      // pcb_grid_helper.cpp BestSnapAnchor applied to both ends: the
      // delta is "where the (snapped) cursor is now" minus "where the
      // (snapped) cursor started" -- not a plain grid-rounded delta --
      // so a drag that starts and ends at the same anchor is exactly
      // zero movement, and dragging onto a *different* nearby anchor
      // (another part's pad, say) lands precisely on it. Only excludes
      // the dragged item's own anchors for a single-item move; a
      // multi-select drag doesn't exclude any member (a reasonable,
      // documented simplification -- see PARITY-pcb.md).
      const [ox, oy] = moveOrigin(origin.x, origin.y, refs);
      const [sx, sy] = moveCursor(wx, wy, e, refs, [ox, oy], [origin.x, origin.y]);
      // Preserve whatever R/Shift+R/F have already accumulated on this
      // same preview (useActionRunner.ts) -- a fresh preview object every
      // pointer-move must not reset the live rotate/flip state.
      // (`perRefOffsetNm` is Pack and Move's per-footprint packed shift -- it rides along until the drop.)
      // The grab point (`BestDragOrigin`): what R turns the carried items about, before the move (kicad-port/pcbCarry.ts `carryPress`).
      const { rotateQuarterTurns, flipped, perRefOffsetNm, ops } = state.movePreview ?? {};
      dispatch({ type: "SET_MOVE_PREVIEW", preview: { refs, kind, dxNm: sx - ox, dyNm: sy - oy, rotateQuarterTurns, flipped, perRefOffsetNm, ops, grabNm: [ox, oy], pivotNm: held?.pivotNm ?? start?.pivotNm, flipPivotNm: held?.flipPivotNm ?? start?.flipPivotNm } });
      return;
    }

    const drag = dragRef.current;
    if (!drag) return;
    if (drag.kind === "pan") {
      userMovedRef.current = true;
      if (motion?.dragging) drag.moved = true;
      dispatch({ type: "SET_VIEW", view: { ...state.view, x: drag.startView[0] + panDeltaX(state.bcx.boardFlipped, e.clientX - drag.startScreen[0]), y: drag.startView[1] + (e.clientY - drag.startScreen[1]) } });
    } else if (drag.kind === "move") {
      // Until the press has become a drag it is a click that may still be released: nothing is picked up.
      if (!motion?.dragging) return;
      drag.moved = true;
      const [sx, sy] = moveCursor(wx, wy, e, drag.refs, drag.snapOrigin, drag.startWorld);
      const dx = sx - drag.snapOrigin[0];
      const dy = sy - drag.snapOrigin[1];
      const { rotateQuarterTurns, flipped, ops } = state.movePreview ?? {};
      dispatch({ type: "SET_MOVE_PREVIEW", preview: { refs: drag.refs, kind: "pcb", dxNm: dx, dyNm: dy, rotateQuarterTurns, flipped, ops, grabNm: [drag.snapOrigin[0], drag.snapOrigin[1]], pivotNm: drag.pivotNm, flipPivotNm: drag.flipPivotNm } });
    } else if (drag.kind === "zoneCorner") {
      if (!motion?.dragging) return; // PCB_POINT_EDITOR::Main starts editing at the drag (`evt->IsDrag`), not at the press
      // `grid.BestSnapAnchor( pos, snapLayers, grid.GetItemGrid( item ), { item } )`: on the zone's layer, not onto the zone itself.
      const p = snap.point("point_edit", [wx, wy], snapMods(e), { layers: new Set((([zone]) => (zone ? zoneLayers(zone) : [state.activeLayer ?? "F.Cu"]))([board?.routing?.zones.find((z) => z.id === drag.zoneId)])), category: "current", skip: new Set([drag.zoneId]) });
      const [sx, sy] = [p[0], p[1]];
      setZoneCornerPreview({ zoneId: drag.zoneId, outline: moveCorner(drag.baseOutline, drag.cornerIndex, sx, sy) });
    } else if (drag.kind === "tuneHandle") {
      if (!motion?.dragging) return;
      drag.drag.move(wx, wy);
    } else if (drag.kind === "shapePoint") {
      if (!motion?.dragging) return;
      const p = snap.point("point_edit", [wx, wy], snapMods(e), { layers: new Set([drag.base.layer]), category: "graphics", skip: new Set([drag.shapeId]) });
      const [sx, sy] = [p[0], p[1]];
      const next = moveShapePoint(drag.base, drag.point, [sx, sy], state.pcbx.arcEditMode);
      setShapePointPreview(next ? { id: drag.shapeId, shape: cmdShapeToShape(next, drag.shapeId) } : null);
    } else if (drag.kind === "box") {
      if (!motion?.dragging) return; // a click with a little jitter, not a rectangle
      const rect = containerRef.current!.getBoundingClientRect();
      const x0 = drag.startScreen[0] - rect.left,
        y0 = drag.startScreen[1] - rect.top;
      const x1 = e.clientX - rect.left,
        y1 = e.clientY - rect.top;
      setMarquee({ x0, y0, x1, y1, crossing: isCrossingSelection(x0, x1) });
    }
  };

  const onPointerUp = (e: React.PointerEvent) => {
    clearLongPress();
    gestureRef.current?.up();
    const drag = dragRef.current;
    dragRef.current = null;
    if (!drag) return;
    if (drag.kind === "pan") {
      justPannedRef.current = drag.button === 2 && drag.moved;
    } else if (drag.kind === "move") {
      const held = state.movePreview;
      if (drag.moved && held) {
        // A drag that ended where it began (and turned and flipped nothing) leaves the board as it was: no undo step.
        if (held.dxNm === 0 && held.dyNm === 0 && !held.rotateQuarterTurns && !held.flipped && !held.ops?.length) dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
        else api.commitMove(held.refs, held.dxNm, held.dyNm, held.kind, held.rotateQuarterTurns, held.flipped, undefined, held.ops);
      } else if (drag.moved) {
        dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
      } else {
        dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
        // No real movement: this was a plain click, not a drag -- apply
        // whatever selection-modifier effect onPointerDown deferred (see
        // pendingClickRef's own doc comment).
        if (pendingClickRef.current) {
          dispatch({ type: "SET_SELECTION", refs: applySingleClickModifier(state.selection, pendingClickRef.current.id, pendingClickRef.current.modifiers) });
        }
      }
      pendingClickRef.current = null;
    } else if (drag.kind === "box" && board) {
      if (!marquee) {
        // Truly zero movement (not even the sub-pixel jitter that
        // normally produces a marquee) -- same deferred-click apply as
        // above, for the "modifier held, landed on exactly one item"
        // case (see onPointerDown's own comment on why this can't just
        // reuse the box-select math: a zero-size box can't geometrically
        // contain or touch anything).
        if (pendingClickRef.current) {
          dispatch({ type: "SET_SELECTION", refs: applySingleClickModifier(state.selection, pendingClickRef.current.id, pendingClickRef.current.modifiers) });
        }
      } else {
        const ctrlOrCmd = isMac() ? e.metaKey : e.ctrlKey;
        const modifiers = computeClickModifiers(e.shiftKey, ctrlOrCmd, e.altKey);
        const crossing = marquee.crossing;
        const [x0, y0] = drag.startWorld;
        const [x1, y1] = screenToWorld(state.view, flipLocalX(state.bcx.boardFlipped, containerSize.width, marquee.x1), marquee.y1);
        const selBox: [number, number, number, number] = [Math.min(x0, x1), Math.min(y0, y1), Math.max(x0, x1), Math.max(y0, y1)];
        const entered = state.enteredGroupId;
        const groupsNow = board.drawings?.groups ?? [];
        const hits = collectBoxSelection(board, selBox, crossing, state.selectionFilter, state.layerVisible, state.activeLayer, state.highContrast).filter((h) => entered === null || withinScope(groupsNow, padParent(board, h.id) ?? h.id, entered));
        if (hits.length > 0 || hasModifier(modifiers)) {
          dispatch({ type: "SET_SELECTION", refs: applyBoxSelectionModifiers(state.selection, hits.map((h) => h.id), modifiers) });
        }
      }
      pendingClickRef.current = null;
      setMarquee(null);
    } else if (drag.kind === "zoneCorner") {
      const orig = drag.baseOutline[drag.cornerIndex]!;
      const final = zoneCornerPreview?.outline[drag.cornerIndex];
      if (final && (final[0] !== orig[0] || final[1] !== orig[1])) {
        api.cmd({ op: "set_zone_outline", id: drag.zoneId, outline: (zoneCornerPreview?.outline ?? drag.baseOutline).map(([x, y]) => ({ x, y })) });
      }
      setZoneCornerPreview(null);
    } else if (drag.kind === "tuneHandle") {
      void drag.drag.finish(api);
    } else if (drag.kind === "shapePoint") {
      // Dropping the handle changes the shape in place (`replace_shape`, as the point editor's `commit.Modify( item )`): it keeps its id, so its group and its hatch; one undo step.
      const preview = shapePointPreview;
      setShapePointPreview(null);
      if (preview && JSON.stringify(shapeToCmd(preview.shape)) !== JSON.stringify(shapeToCmd(drag.base))) {
        const re = reshapeCmds(board, drag.shapeId, shapeToCmd(preview.shape));
        void applyCommands(api, dispatch, re.cmds, re.removed);
      }
    }
  };

  /** wx_view_controls.cpp WX_VIEW_CONTROLS::onWheel, via kicad-port/viewControls.ts's handleWheel -- plain wheel zooms about the cursor (ConstantZoomController/AcceleratingZoomController per platform, not an ad hoc factor), Ctrl+wheel pans horizontally, Shift/Alt+wheel pans vertically, matching KiCad's default scroll_modifier_zoom/scroll_modifier_pan_h settings. */
  const onWheel = (e: WheelEvent) => {
    e.preventDefault();
    userMovedRef.current = true;
    const rect = containerRef.current!.getBoundingClientRect();
    const input: WheelInput = {
      deltaX: e.deltaX,
      deltaY: e.deltaY,
      shiftKey: e.shiftKey,
      ctrlOrCmd: isMac() ? e.metaKey : e.ctrlKey,
      mac: isMac(),
      altKey: e.altKey,
      x: flipLocalX(state.bcx.boardFlipped, rect.width, e.clientX - rect.left),
      y: e.clientY - rect.top,
    };
    const result = handleWheel(state.view, { width: rect.width, height: rect.height }, input, wheelPrefs.settings, wheelPrefs.controller);
    if (result.kind !== "unhandled") dispatch({ type: "SET_VIEW", view: result.kind === "zoom" ? result.view : flipPan(state.bcx.boardFlipped, state.view, result.view) });
  };
  // React's own onWheel is passive, so the `preventDefault()` above would log an error on every tick and let the page scroll under the zoom.
  useNonPassiveWheel(containerRef, onWheel);

  /** KiCad builds this per-selection from whatever tool/edit actions apply (pcb_selection_tool.cpp/edit_tool.cpp) -- ported here as exactly the actions this app implements, everything else the usual disabled "(not ported yet)". */
  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    // wx_view_controls.cpp onButton: a right-drag pans rather than
    // opening anything -- the browser's own native `contextmenu` event
    // still fires on button-up regardless of how far the mouse moved in
    // between, so this is what actually suppresses it for a real drag.
    if (justPannedRef.current) {
      justPannedRef.current = false;
      return;
    }
    if (!board) return;
    const [wx, wy] = worldAt(e);
    // A right click on a DRC marker (a circle the last run left on the board) opens the marker's menu -- Exclude, Exclude all of this type, Change severity,
    // Show in the dialog -- instead of the item menu: a `PCB_MARKER` is an item of its own with its own entries (kicad-port/rcItems.ts `rcMenu`).
    const markerIndex = drcMarkerAt(state, wx, wy);
    if (markerIndex >= 0) {
      const v = state.drc!.violations[markerIndex]!;
      dispatch({ type: "SET_DRC_SELECTED", index: markerIndex });
      const spec = rcMenu(drcMenuContext(state, v, true));
      const entries = toMenuEntries(spec, (id) => void runMarkerMenu({ api, dispatch }, { domain: "drc", tab: "violations", index: markerIndex }, id, askExclusionComment), canExclude(v));
      setPcbMenu(null);
      setContextMenu({ x: e.clientX, y: e.clientY, entries });
      return;
    }
    dispatch({ type: "PCBX", patch: { menuCursorNm: { x: wx, y: wy } } }); // `GetMenuCursorPos()`
    // pcb_selection_tool.cpp Main(): "Right click? if there is any object - show the context menu" -- with an empty selection the item under the pointer is
    // selected first (`selectPoint`, a hover selection); a selection that exists is the one the menu is for, wherever the click is.
    let refs = [...state.selection];
    if (refs.length === 0 && (state.activeTool === "select" || state.activeTool === "move")) {
      const toleranceNm = Math.max(150_000, 6 / state.view.scale);
      const groupsNow = board.drawings?.groups ?? [];
      const top = pickSelectionCandidates(board, wx, wy, toleranceNm, 1 / state.view.scale, state.selectionFilter, state.layerVisible, state.activeLayer, state.highContrast, state.selection, false, false).find(
        (c) => state.enteredGroupId === null || withinScope(groupsNow, padParent(board, c.id) ?? c.id, state.enteredGroupId)
      );
      const id = top?.id ?? partHit(board.parts, wx, wy)?.ref ?? null;
      if (id) {
        refs = substituteSelection(groupsNow, [id], state.enteredGroupId).ids;
        dispatch({ type: "SET_SELECTION", refs: [id] });
      }
    }
    // The menu of whichever tool owns the click -- the router, a drawing tool, a picker, the selection tool: kicad-port/pcbContextMenu.ts, built in components/canvas/pcbMenuBuilder.ts.
    setPcbMenu({ x: e.clientX, y: e.clientY, nodes: buildPcbMenu(state, board, refs, [wx, wy]) });
    setContextMenu(null);
  };

  // Escape/M/rotate/delete/undo/redo/net-highlight-toggle are all
  // handled globally now (actions/useGlobalHotkeys.ts), driven by
  // src/kicad/actions.json's real hotkeys through the same registry the
  // menu bar and toolbars use, reading/writing the store's activeTool
  // and cursorNm. Enter-finishes-the-current-route/zone/shape is the one
  // interaction left here: it's a plain UI convention this app is
  // choosing for its own click-to-add-points tools, not a KiCad dotted
  // action with a real extracted hotkey, so it doesn't belong in that
  // global registry the way a real one does.
  const onCanvasKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && state.drawState) {
      e.preventDefault();
      finishDraw();
    }
    // The Track Corner Mode hotkeys (router_tool.cpp's `ACT_SwitchCornerMode*`, static TOOL_ACTIONs of the router tool): Ctrl+/ the next
    // mode, Ctrl+W 45, Ctrl+Shift+W arc 45, Ctrl+Alt+W 90, Alt+W arc 90 -- in the router tool only (`AS_CONTEXT`), the pair router's too; and
    // the router's other context actions: Ctrl+V microvia, Alt+Shift+V blind via, < / Alt+< pick a layer and place a through / blind via,
    // / Switch Track Posture (`ACT_SwitchPosture`, which takes the key over from the drawing tools' posture keys), Q Custom Track/Via Size.
    // Matched here by the physical key so that a layout's Alt+W character does not hide it. (A browser keeps Ctrl+W for itself; the
    // menu has the same five.)
    if (state.activeTool === "route" || state.activeTool === "diffpair" || state.drawState?.kind === "route" || state.drawState?.kind === "diffpair") {
      const ctrl = e.ctrlKey || e.metaKey;
      const w = e.code === "KeyW";
      const v = e.code === "KeyV";
      // `<` is Shift+Comma on a US keyboard and its own key (IntlBackslash) on an ISO one
      const less = e.key === "<" || e.code === "IntlBackslash" || (e.shiftKey && e.code === "Comma");
      const slash = e.key === "/" || (e.code === "Slash" && !e.shiftKey);
      const action =
        ctrl && slash ? "pcbnew.InteractiveRouter.SwitchRoundingToNext"
        : ctrl && e.altKey && w ? "pcbnew.InteractiveRouter.SwitchRounding90"
        : ctrl && e.shiftKey && w ? "pcbnew.InteractiveRouter.SwitchRoundingArc45"
        : ctrl && w ? "pcbnew.InteractiveRouter.SwitchRounding45"
        : e.altKey && w ? "pcbnew.InteractiveRouter.SwitchRoundingArc90"
        // the router's other context actions (actions/routerViaActions.ts): Ctrl+V is Paste's elsewhere, and Alt+.. may type another character
        : ctrl && v && !e.altKey && !e.shiftKey ? "pcbnew.InteractiveRouter.PlaceMicroVia"
        : !ctrl && e.altKey && e.shiftKey && v ? "pcbnew.InteractiveRouter.PlaceBlindVia"
        : !ctrl && e.altKey && less ? "pcbnew.InteractiveRouter.SelLayerAndPlaceBlindVia"
        : !ctrl && !e.altKey && less ? "pcbnew.InteractiveRouter.SelLayerAndPlaceVia"
        : !ctrl && !e.altKey && slash ? "pcbnew.InteractiveRouter.SwitchPosture"
        : !ctrl && !e.altKey && !e.shiftKey && e.code === "KeyQ" ? "pcbnew.InteractiveRouter.CustomTrackViaSize"
        : null;
      if (action) {
        e.preventDefault();
        e.stopPropagation();
        void run(action);
        return;
      }
    }
  };

  /**
   * pcb_selection_tool.cpp Main()'s IsDblClick handler: if nothing's
   * selected yet, selectPoint() at the click first; a single selected
   * group enters it (task item 5's `common.Interactive.groupEnter`,
   * `EnterGroup()`), anything else runs PCB_ACTIONS::properties -- the
   * same dispatch useActionRunner.ts's "E" hotkey uses (properties.ts).
   */
  const onDoubleClick = (e: React.MouseEvent) => {
    // PICKER_TOOL::Main: "Not currently used, but we don't want to pass them either".
    if (picker.session()) return;
    if (state.drawState) {
      finishDraw();
      return;
    }
    if (!board) return;
    // `PCB_EDIT_FRAME::OnEditItemRequest` on a `PCB_MARKER`: `DRC_TOOL::CrossProbe( marker )` -- the dialog comes up with the marker's row selected.
    const [mx, my] = worldAt(e);
    const markerHit = drcMarkerAt(state, mx, my);
    if (markerHit >= 0) {
      showDrcInDialog({ api, dispatch }, "violations", markerHit);
      return;
    }
    // A double click on a selected zone's edge opens the zone's properties, as on any item: KiCad's point editor has no double-click (`PCB_SELECTION_TOOL::Main`'s
    // `IsDblClick` opens the item's properties); a corner is added with Create Corner (Insert, `pcbnew.PointEditor.addCorner`).
    let refs = [...state.selection];
    if (refs.length === 0) {
      const [wx, wy] = worldAt(e);
      const toleranceNm = Math.max(150_000, 6 / state.view.scale);
      const onePixelNm = 1 / state.view.scale;
      const candidates = pickSelectionCandidates(board, wx, wy, toleranceNm, onePixelNm, state.selectionFilter, state.layerVisible, state.activeLayer, state.highContrast, state.selection, false, false);
      if (candidates.length === 0) return;
      refs = [candidates[0]!.id];
      dispatch({ type: "SET_SELECTION", refs });
    }
    if (refs.length === 1) {
      // `refs[0]` may already be a group's own id (SET_SELECTION's own
      // substitution, state/store.tsx's `withGroupSubstitution`, already
      // ran for anything picked from a pre-existing `state.selection`), or
      // still a raw member id (the empty-selection branch just above,
      // whose dispatch hasn't re-rendered yet) -- checked the same way
      // either case. Re-entering the group already entered falls through
      // to properties instead, same as double-clicking a member while
      // already inside its own group.
      // `m_selection.GetSize() == 1 && m_selection[0]->Type() == PCB_GROUP_T` -> `EnterGroup()` (leaving the group entered, if any, first), else Properties.
      const groups = board.drawings?.groups ?? [];
      const hitGroup = isGroup(groups, refs[0]!) ? refs[0]! : topLevelGroup(groups, refs[0]!, state.enteredGroupId);
      if (hitGroup && hitGroup !== state.enteredGroupId) {
        dispatch({ type: "ENTER_GROUP", id: hitGroup });
        return;
      }
      openPropertiesFor(refs[0]!, api, dispatch);
    }
  };

  return (
    <div
      ref={containerRef}
      className="pcb-canvas-container"
      tabIndex={0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onDoubleClick={onDoubleClick}
      onKeyDown={onCanvasKeyDown}
      onContextMenu={onContextMenu}
      data-armed={state.armed || state.armedFootprint ? "true" : "false"}
    >
      <canvas ref={canvasRef} />
      {!board && <div className="pcb-canvas-empty">{state.boardError ?? "Loading board…"}</div>}
      {contextMenu && <ContextMenu x={contextMenu.x} y={contextMenu.y} entries={contextMenu.entries} onClose={() => setContextMenu(null)} />}
      {pcbMenu && <SchContextMenu x={pcbMenu.x} y={pcbMenu.y} nodes={pcbMenu.nodes} onClose={() => setPcbMenu(null)} />}
    </div>
  );
}
