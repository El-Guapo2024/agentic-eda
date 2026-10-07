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
//     `move_to` on drop (see state/store.tsx `commitMove`); M starts the
//     same preview without holding the button, click commits, Esc cancels.
//   - R / Shift+R rotate by a quarter turn each way; Delete rips.
import React, { useCallback, useEffect, useRef, useState } from "react";
import type { CmdShape, Part } from "../../api/types";
import { DEFAULT_RULE_AREA_SETTINGS, DEFAULT_ZONE_SETTINGS, useStudioApi, useStudioDispatch, useStudioState } from "../../state/store";
import type { ToolId } from "../../state/store";
import type { RuleAreaFields, Zone, ZoneSettingsFields } from "../../api/types";
import { boundsOfPoints, fitTransform, screenToWorld, panByWorldDelta } from "./view";
import { paintBoard } from "./painter";
import { isStale } from "../../kicad-port/checkRevision";
import { layerColor } from "./layers";
import { snapPoint, snapWithAnchors, type GridSnapModifiers } from "./gridHelper";
import { findRouteAnchor, constrainByAngleMode, startInteractiveRoute, fixInteractiveRoute, finishInteractiveRoute } from "./routing";
import { createMoveThrottle, createRequestGuard, drawStateFromPreview } from "../../kicad-port/routeTool";
import { routeMove, routeDragMove, dpMove } from "../../api/client";
import { finishInlineDrag } from "./dragging";
import { dragStateFromPreview } from "../../kicad-port/dragTool";
import { startDiffPairRoute, fixDiffPairRoute, finishDiffPairRoute } from "./diffPairRouting";
import { dpStateFromPreview } from "../../kicad-port/dpTool";
import { ContextMenu, type MenuEntry } from "./ContextMenu";
import { handleWheel, computeAutoPanDirection, computeAutoPanStep, DEFAULT_VIEW_CONTROL_SETTINGS, type WheelInput } from "../../kicad-port/viewControls";
import { useWheelPrefs } from "../../actions/useWheelPrefs";
import { isMac } from "../../platform";
import { computeClickModifiers, isCrossingSelection, applySingleClickModifier, applyBoxSelectionModifiers, hasModifier, type ClickModifiers } from "../../kicad-port/selection";
import { pickSelectionCandidates, collectBoxSelection, type SelectionCandidate, type SelectableKind } from "./selectionCandidates";
import { openPropertiesFor } from "./properties";
import { useActionRunner } from "../../actions/useActionRunner";
import { findNearestCorner, findNearestEdgeInsertionIndex, insertCorner, moveCorner, removeCorner } from "../../kicad-port/zonePointEditor";
import { defaultDimensionPayload } from "../../kicad-port/dimensionConvert";
import { arcClick, arcMotion } from "../../kicad-port/arcGeom";
import { bezierClick, bezierFinishDouble, bezierMotion } from "../../kicad-port/bezierGeom";
import { arcAngleSnap, arcClickPoints, bezierShape, ptXY } from "./curveTools";
import { connectedTrackWidth, displayedRatsnest, flipLocalX, flipPan, highlightedNets, panDeltaX, toggleLocalRatsnestFootprint, toggleLocalRatsnestPad } from "../../kicad-port/boardControl";
import { padAt } from "../../kicad-port/boardControlPick";
import "../../styles/canvas.css";

/** `ZONE_SETTINGS << aSrcZone` (zone_create_helper.cpp createZoneFromExisting): the fill + rule-area settings of an existing zone, as `api.addZone` takes them. */
function zoneSettingsOf(zone: Zone): ZoneSettingsFields & RuleAreaFields {
  const out: Record<string, unknown> = {};
  for (const k of Object.keys({ ...DEFAULT_ZONE_SETTINGS, ...DEFAULT_RULE_AREA_SETTINGS })) out[k] = (zone as unknown as Record<string, unknown>)[k];
  return out as unknown as ZoneSettingsFields & RuleAreaFields;
}

/** A candidate's own kind determines which Cmd a drag of it would commit through -- tracks/zones have no move_* Cmd (api/types.ts), so they're selectable but never draggable, same as before this session. */
const DRAGGABLE_KINDS = new Set<SelectableKind>(["part", "via", "shape", "text", "dimension"]);

/** wx_view_controls.cpp onButton: MiddleDown/RightDown both start DRAG_PANNING by default (m_dragMiddle/m_dragRight == MOUSE_DRAG_ACTION::PAN). A plain click (no real movement) of the right button still opens the context menu -- see onContextMenu's `justPanned` check -- same as source's right button also being each platform's native context-menu trigger. */
const PAN_BUTTONS = new Set([1, 2]);
/** Screen-px movement past which a right-button press counts as a pan-drag rather than a click-to-open-the-context-menu. */
const PAN_CLICK_TOLERANCE_PX = 4;

const LONG_PRESS_MS = 500;
const LONG_PRESS_MOVE_TOLERANCE_PX = 6;
/** A click within this many board um of a pad/via/track-end counts as landing on it -- generous enough to be usable at a typical zoom without needing pixel-perfect precision, same idea as pcb_grid_helper's own anchor snapping (not ported here, see gridHelper.ts). */
const ANCHOR_SNAP_UM = 500;
/** No per-board "default graphic line width" setting exists (board_rules only covers track/via) -- a plain 0.15mm default, same order of magnitude as KiCad's own out-of-the-box default (0.15-0.2mm silkscreen line width, by version/theme). */
// (DEFAULT_STROKE_WIDTH_UM now lives in kicad-port/pcbParityState.ts -- incWidth/decWidth step state.pcbx.drawStrokeWidthUm from it.)
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
function shapeFromPoints(kind: ShapeToolKind, pts: [number, number][], layer: string, widthUm: number): CmdShape | null {
  const p = (i: number) => ({ x: pts[i]![0], y: pts[i]![1] });
  switch (kind) {
    case "bezier":
      return null; // built from the construction manager's `BezierCurve`, never from a bare point list
    case "segment":
      return pts.length >= 2 ? { kind: "segment", layer, stroke_width: widthUm, filled: false, start: p(0), end: p(1) } : null;
    case "rect":
      return pts.length >= 2 ? { kind: "rect", layer, stroke_width: widthUm, filled: false, start: p(0), end: p(1) } : null;
    case "circle":
      return pts.length >= 2 ? { kind: "circle", layer, stroke_width: widthUm, filled: false, center: p(0), end: p(1) } : null;
    case "arc":
      return pts.length >= 3 ? { kind: "arc", layer, stroke_width: widthUm, filled: false, start: p(0), mid: p(1), end: p(2) } : null;
    case "polygon":
      return pts.length >= 3 ? { kind: "polygon", layer, stroke_width: widthUm, filled: true, pts: pts.map((_, i) => p(i)) } : null;
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
  | { kind: "move"; refs: string[]; moveKind: "part" | "via" | "shape" | "text" | "dimension"; startWorld: [number, number]; snapOrigin: [number, number]; moved: boolean }
  | { kind: "box"; startWorld: [number, number]; startScreen: [number, number] }
  /** pcb_point_editor.cpp: dragging one corner of the single selected zone's outline. `baseOutline` is a snapshot at drag-start, so every move computes fresh from it (no cumulative drift) -- same "delta from start" shape the move tool's own drag already uses. */
  | { kind: "zoneCorner"; zoneId: string; cornerIndex: number; baseOutline: [number, number][] };

/** A click/double-click/right-click within this many board µm of a zone corner counts as landing on it -- same generous, zoom-aware tolerance `ANCHOR_SNAP_UM`-adjacent code elsewhere in this file already uses. */
function zoneCornerToleranceUm(viewScale: number): number {
  return Math.max(300, 10 / viewScale);
}

/** Every part whose courtyard contains the point, topmost (last drawn) first. */
function partsAt(parts: Part[], xUm: number, yUm: number): Part[] {
  const hits: Part[] = [];
  for (let i = parts.length - 1; i >= 0; i--) {
    const p = parts[i]!;
    if (!p.placed || !p.courtyard) continue;
    const [x0, y0, x1, y1] = p.courtyard;
    if (xUm >= x0 && xUm <= x1 && yUm >= y0 && yUm <= y1) hits.push(p);
  }
  return hits;
}

function partHit(parts: Part[], xUm: number, yUm: number): Part | null {
  return partsAt(parts, xUm, yUm)[0] ?? null;
}

export function Canvas() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  // Only for onContextMenu's Delete/Cut entries below, so they resolve a
  // mixed-kind selection (track/via/zone/shape/text/part) the same
  // correct, per-kind way the Del hotkey's common.Interactive.delete
  // already does -- a plain api.ripSelection() call only ever handles
  // footprints, which used to make a right-click Delete on anything else
  // silently do nothing.
  const { run } = useActionRunner();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const [marquee, setMarquee] = useState<{ x0: number; y0: number; x1: number; y1: number; crossing: boolean } | null>(null);
  /** pcb_point_editor.cpp's live corner-drag preview -- local, like `marquee` above, since only this component's own render loop needs it. */
  const [zoneCornerPreview, setZoneCornerPreview] = useState<{ zoneId: string; outline: [number, number][] } | null>(null);
  const moveMode = state.activeTool === "move";
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 });
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
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
            visibleLayers: new Set(board.layers.flatMap((l, i) => (state.layerVisible[l] !== false ? [i] : []))),
          })
        : null,
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
      layerVisible: state.layerVisible,
      layerOpacity: state.layerOpacity,
      activeLayer: state.activeLayer,
      highContrast: state.highContrast,
      gridUm: state.gridUm,
      gridVisible: state.gridVisible,
      movePreview: state.movePreview,
      sketchPads: state.sketchPads,
      sketchTracks: state.sketchTracks,
      sketchVias: state.sketchVias,
      sketchGraphics: state.bcx.sketchGraphics,
      sketchText: state.bcx.sketchText,
      showPadNumbers: state.bcx.showPadNumbers,
      auxOrigin: board.aux_origin ?? null,
      drawState: state.drawState,
      cursorUm: state.cursorUm,
      activeTool: state.activeTool,
      angleSnapMode: state.pcbx.angleSnapMode,
    });
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

    // Crosshair cursor.
    if (state.cursorUm) {
      const sx = flipLocalX(state.bcx.boardFlipped, width, state.cursorUm.x * state.view.scale + state.view.x);
      const sy = state.cursorUm.y * state.view.scale + state.view.y;
      ctx.strokeStyle = "rgba(224,224,224,0.9)";
      ctx.lineWidth = 1;
      const len = state.fullscreenCrosshair ? 100000 : 8;
      ctx.beginPath();
      if (state.fullscreenCrosshair) {
        ctx.moveTo(0, sy);
        ctx.lineTo(width, sy);
        ctx.moveTo(sx, 0);
        ctx.lineTo(sx, height);
      } else {
        ctx.moveTo(sx - len, sy);
        ctx.lineTo(sx + len, sy);
        ctx.moveTo(sx, sy - len);
        ctx.lineTo(sx, sy + len);
      }
      ctx.stroke();
    }
    ctx.restore();
  }, [board, state.view, state.selection, state.hot, state.netHighlight, state.showRatsnest, state.ratsnestCurved, state.ratsnest, state.drc, state.drcVersion, state.version, state.drcSelected, state.drcDialogOpen, state.lint, state.drcLintSelected, state.layerVisible, state.layerOpacity, state.activeLayer, state.highContrast, state.gridUm, state.gridVisible, state.movePreview, state.cursorUm, state.fullscreenCrosshair, state.sketchPads, state.sketchTracks, state.sketchVias, state.drawState, state.activeTool, state.pcbx.angleSnapMode, state.zoneFill, state.zoneDisplayMode, state.currentViaPreset, state.units, state.bcx, marquee, zoneCornerPreview, containerSize]);

  const worldAt = useCallback(
    (e: { clientX: number; clientY: number }): [number, number] => {
      const rect = containerRef.current!.getBoundingClientRect();
      return screenToWorld(state.view, flipLocalX(state.bcx.boardFlipped, rect.width, e.clientX - rect.left), e.clientY - rect.top);
    },
    [state.view, state.bcx.boardFlipped]
  );

  /** tool_event.h/edit_tool_move_fct.cpp's real move-tool modifiers (see kicad-port/gridSnap.ts's header comment): Ctrl (Cmd on macOS) disables grid round-off, Shift disables anchor snapping. */
  const gridSnapModifiers = (e: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }): GridSnapModifiers => ({
    ctrlOrCmd: isMac() ? e.metaKey : e.ctrlKey,
    shiftKey: e.shiftKey,
  });

  /** pcb_grid_helper.cpp BestSnapAnchor, applied to a single reference point -- see gridHelper.ts:snapWithAnchors. Falls back to plain grid snap when there's no board yet (shouldn't happen once a drag is possible, but keeps this total). */
  const snapRef = (wx: number, wy: number, e: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }, excludeOwnerId?: string): [number, number] => {
    if (!board) return snapPoint(wx, wy, state.gridUm);
    const { x, y } = snapWithAnchors(wx, wy, board.snap ?? state.gridUm, state.view.scale, board, gridSnapModifiers(e), excludeOwnerId, { allLayers: state.magneticAllLayers, activeLayer: state.activeLayer });
    return [x, y];
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
      }
    },
    [api]
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
    if (!draw) return;
    if (draw.kind === "route") {
      const [x, y] = draw.pts[draw.pts.length - 1] ?? [draw.pts[0]?.[0] ?? 0, draw.pts[0]?.[1] ?? 0];
      void finishInteractiveRoute(x, y, dispatch, api);
    } else if (draw.kind === "diffpair") {
      const [x, y] = draw.ptsA[draw.ptsA.length - 1] ?? [draw.ptsA[0]?.[0] ?? 0, draw.ptsA[0]?.[1] ?? 0];
      void finishDiffPairRoute(x, y, dispatch, api);
    } else if (draw.kind === "zone") {
      if (draw.pts.length >= 3) {
        // zone_create_helper.cpp commitZone(): a cutout subtracts from its source zone and a
        // "similar" zone copies its settings -- neither shows the properties dialog (only
        // createNewZone() does); a plain draw still goes through ZoneDialog.
        const mode = state.pcbx.zoneDrawMode;
        const source = mode ? api.zoneById(mode.sourceId) : undefined;
        if (mode?.mode === "cutout" && source) {
          void api.cmd({ op: "zone_cutout", id: source.id, cutout: draw.pts.map(([x, y]) => ({ x, y })) }).then(() => dispatch({ type: "SET_SELECTION", refs: [] }));
        } else if (mode?.mode === "similar" && source) {
          void api.addZone(source.net, source.layer, draw.pts, zoneSettingsOf(source));
        } else {
          dispatch({ type: "SET_ZONE_PENDING", outline: draw.pts });
        }
        dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
        dispatch({ type: "PCBX", patch: { zoneDrawMode: null } });
      }
      dispatch({ type: "SET_DRAW_STATE", draw: null });
    } else if (draw.kind === "shape") {
      // An arc ends only when its three points are in (ARC_GEOM_MANAGER); Enter / a double-click leave it running.
      if (draw.shapeKind === "arc") return;
      if (draw.shapeKind === "bezier") {
        // A double-click: "use the current point for all remaining points", accept, and reset (no chaining).
        const at = state.cursorUm ? snapPoint(state.cursorUm.x, state.cursorUm.y, board?.snap ?? state.gridUm) : draw.bezier?.lastPoint;
        const curve = draw.bezier && at ? bezierFinishDouble(draw.bezier, at) : null;
        if (curve) void api.cmd({ op: "add_shape", shape: bezierShape(curve, state.activeLayer ?? "F.SilkS", state.pcbx.drawStrokeWidthUm) });
        dispatch({ type: "SET_DRAW_STATE", draw: null });
        return;
      }
      const shape = shapeFromPoints(draw.shapeKind, draw.pts, state.activeLayer ?? "F.SilkS", state.pcbx.drawStrokeWidthUm);
      if (shape) api.cmd({ op: "add_shape", shape });
      dispatch({ type: "SET_DRAW_STATE", draw: null });
    }
  }, [state.drawState, state.activeLayer, state.cursorUm, state.gridUm, state.pcbx, board, api, dispatch]);

  const onPointerDown = (e: React.PointerEvent) => {
    // The context menu is a child of this container, so a press on one of its entries bubbles up here: it must not close the menu
    // (and clear the selection) before the entry's click arrives -- the menu closes itself when an entry runs or a press lands outside it.
    if ((e.target as Element).closest?.(".menubar-dropdown")) return;
    // Throws NotFoundError for a synthesized pointer (common.Control.cursorClick's
    // Enter-key click, actions/useActionRunner.ts), which has no real pointer to capture.
    try {
      (e.target as Element).setPointerCapture(e.pointerId);
    } catch {
      /* synthetic pointer */
    }
    const [wx, wy] = worldAt(e);
    setContextMenu(null);

    // Route/via/zone/drawing/text tools: a click either starts, extends,
    // or (for via/text) completes one placement -- entirely separate
    // from the select/move flow below, which only applies to the
    // "select"/"move" tools.
    if (board && e.button === 0 && !e.altKey) {
      const [sx, sy] = snapPoint(wx, wy, board.snap ?? state.gridUm);

      // `pcbnew.EditorControl.drillOrigin`: `DrillOrigin`'s picker click handler -- the drill/place file origin goes here and the tool is done
      // ("drill origin is a one-shot; don't continue with tool").
      if (state.activeTool === "drill_origin") {
        void api.cmd({ op: "set_aux_origin", at: { x: sx, y: sy } });
        dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
        return;
      }

      // `pcbnew.Control.localRatsnestTool`: `BOARD_INSPECTION_TOOL::LocalRatsnestTool`'s click handler. The pad under the cursor, else the
      // footprint, has its ratsnest shown or hidden; a click on neither resets every pad. The tool stays armed until Esc.
      if (state.activeTool === "local_ratsnest") {
        const pad = padAt(board.parts, wx, wy);
        const part = pad ? board.parts.find((p) => p.ref === pad.ref) : partHit(board.parts, wx, wy);
        const flipped = state.bcx.localRatsnestPads;
        const next = pad ? toggleLocalRatsnestPad(flipped, pad.id) : part ? toggleLocalRatsnestFootprint(flipped, (part.pads ?? []).map((q) => `${part.ref}.${q.num}`), state.showRatsnest) : [];
        dispatch({ type: "BCX", patch: { localRatsnestPads: next } });
        return;
      }

      if (state.activeTool === "via") {
        const anchor = findRouteAnchor(board, sx, sy, ANCHOR_SNAP_UM);
        if (!anchor) {
          dispatch({ type: "TOAST", message: "Click a pad, via, or track end -- a via needs a net.", kind: "error" });
          return;
        }
        const rules = board.board_rules;
        // pcbnew.EditorControl.viaSizeInc/Dec's current pick (useActionRunner.ts), same board-default fallback the hotkey itself uses until it's ever pressed.
        const viaPreset = state.currentViaPreset ?? { diameter: rules?.via_diameter ?? 600, drill: rules?.via_drill ?? 300 };
        api.cmd({ op: "add_via", net: anchor.net, x: sx, y: sy, drill: viaPreset.drill, diameter: viaPreset.diameter, from_layer: "F.Cu", to_layer: "B.Cu" });
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
          // `autoTrackWidth`: starting from an existing track's end, that track's width wins over the current one.
          const connected = state.bcx.autoTrackWidth ? connectedTrackWidth(findRouteAnchor(board, sx, sy, ANCHOR_SNAP_UM)?.from, board.routing?.tracks ?? []) : null;
          const width = connected ?? state.currentTrackWidthUm ?? board.board_rules?.track_width ?? 250;
          void startInteractiveRoute(sx, sy, layer, width, state.routerSettings, dispatch);
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
          void startDiffPairRoute(sx, sy, layer, dispatch);
          return;
        }
        void fixDiffPairRoute(sx, sy, draw, dispatch, api);
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
        if (state.drawState?.kind === "drag") void finishInlineDrag(sx, sy, dispatch, api);
        else dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
        return;
      }

      if (state.activeTool === "zone") {
        const draw = state.drawState;
        const pts: [number, number][] = draw?.kind === "zone" ? [...draw.pts, [sx, sy]] : [[sx, sy]];
        dispatch({ type: "SET_DRAW_STATE", draw: { kind: "zone", pts } });
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
            const payload = defaultDimensionPayload(state.nextDimensionKind, start, end, state.activeLayer ?? "Dwgs.User", settings);
            void api.addDimension(payload).then((id) => {
              if (id) dispatch({ type: "SET_DIMENSION_EDIT_ID", id });
            });
          }
        } else {
          dispatch({ type: "SET_DRAW_STATE", draw: { kind: "dimension", pts: [[sx, sy]] } });
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
        const { geom, arc } = arcClick(prev, [sx, sy], arcAngleSnap(state.pcbx.angleSnapMode, e));
        if (arc) {
          const shape: CmdShape = { kind: "arc", layer: state.activeLayer ?? "F.SilkS", stroke_width: state.pcbx.drawStrokeWidthUm, filled: false, start: ptXY(arc.start), mid: ptXY(arc.mid), end: ptXY(arc.end) };
          void api.cmd({ op: "add_shape", shape });
        }
        dispatch({ type: "SET_DRAW_STATE", draw: geom ? { kind: "shape", shapeKind: "arc", pts: arcClickPoints(geom), arc: geom } : null });
        return;
      }
      // DRAWING_TOOL::drawOneBezier (via DrawBezier's chaining loop): start, control 1, end, control 2.
      if (shapeKind === "bezier") {
        const draw = state.drawState;
        const prev = draw?.kind === "shape" && draw.shapeKind === "bezier" ? draw.bezier : undefined;
        const { geom, curve } = bezierClick(prev, [sx, sy]);
        if (curve) void api.cmd({ op: "add_shape", shape: bezierShape(curve, state.activeLayer ?? "F.SilkS", state.pcbx.drawStrokeWidthUm) });
        dispatch({ type: "SET_DRAW_STATE", draw: geom ? { kind: "shape", shapeKind: "bezier", pts: [], bezier: geom } : null });
        return;
      }
      if (shapeKind) {
        const draw = state.drawState;
        const already = draw?.kind === "shape" && draw.shapeKind === shapeKind ? draw.pts : [];
        let point: [number, number] = [sx, sy];
        if (already.length > 0 && (shapeKind === "segment" || shapeKind === "rect")) {
          const last = already[already.length - 1]!;
          const constrained = constrainByAngleMode(state.pcbx.angleSnapMode, last, [sx, sy]);
          point = snapPoint(constrained[0], constrained[1], board.snap ?? state.gridUm);
        }
        const pts = [...already, point];
        const finishAt = shapeAutoFinishCount(shapeKind);
        if (finishAt !== null && pts.length >= finishAt) {
          const shape = shapeFromPoints(shapeKind, pts, state.activeLayer ?? "F.SilkS", state.pcbx.drawStrokeWidthUm);
          if (shape) api.cmd({ op: "add_shape", shape });
          dispatch({ type: "SET_DRAW_STATE", draw: null });
        } else {
          dispatch({ type: "SET_DRAW_STATE", draw: { kind: "shape", shapeKind, pts } });
        }
        return;
      }

      if (state.activeTool === "text") {
        dispatch({ type: "SET_TEXT_DIALOG", dialog: { mode: "add", at: [sx, sy] } });
        dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
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
      if (zone) {
        const idx = findNearestCorner(zone.outline, wx, wy, zoneCornerToleranceUm(state.view.scale));
        if (idx != null) {
          dragRef.current = { kind: "zoneCorner", zoneId: zone.id, cornerIndex: idx, baseOutline: zone.outline };
          setZoneCornerPreview({ zoneId: zone.id, outline: zone.outline });
          return;
        }
      }
    }

    if (moveMode) {
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
      if (state.movePreview) api.commitMove(state.movePreview.refs, state.movePreview.dxUm, state.movePreview.dyUm, state.movePreview.kind, state.movePreview.rotateQuarterTurns, state.movePreview.flipped, state.movePreview.perRefOffsetUm);
      return;
    }
    if (state.armed) {
      const [sx, sy] = snapPoint(wx, wy, board?.snap ?? state.gridUm);
      api.placeArmedAt(sx, sy);
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
    const toleranceUm = Math.max(150, 6 / state.view.scale);
    const onePixelUm = 1 / state.view.scale;
    const runPick = (skipHeuristics: boolean): SelectionCandidate[] =>
      board ? pickSelectionCandidates(board, wx, wy, toleranceUm, onePixelUm, state.selectionFilter, state.layerVisible, state.activeLayer, state.highContrast, state.selection, modifiers.subtractive, skipHeuristics) : [];

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
      if (DRAGGABLE_KINDS.has(hit.kind) && refs.length > 0) {
        const soleRef = refs.length === 1 ? refs[0] : undefined;
        dragRef.current = { kind: "move", refs, moveKind: hit.kind as "part" | "via" | "shape" | "text" | "dimension", startWorld: [wx, wy], snapOrigin: snapRef(wx, wy, e, soleRef), moved: false };
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
    dispatch({ type: "SET_CURSOR", at: { x: wx, y: wy } });
    const containerRectForAutoPan = containerRef.current?.getBoundingClientRect();
    if (containerRectForAutoPan) lastPointerScreenRef.current = { x: e.clientX - containerRectForAutoPan.left, y: e.clientY - containerRectForAutoPan.top };

    if (longPressRef.current) {
      const [sx, sy] = longPressRef.current.startScreen;
      if (Math.hypot(e.clientX - sx, e.clientY - sy) > LONG_PRESS_MOVE_TOLERANCE_PX) clearLongPress();
    }

    // Interactive router (gap #7) live preview: every move asks the
    // backend to re-resolve the head toward the cursor (walkaround/shove/
    // mark-obstacles, same as real pcbnew's router_tool.cpp mouse-move
    // handler) -- throttled and guarded against an out-of-order reply the
    // same way the rest of this app's async calls are (see
    // kicad-port/routeTool.ts).
    if (board && state.activeTool === "route" && state.drawState?.kind === "route") {
      const draw = state.drawState;
      if (routeMoveThrottleRef.current.shouldSend(performance.now())) {
        const [sx, sy] = snapPoint(wx, wy, board.snap ?? state.gridUm);
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
        const [sx, sy] = snapPoint(wx, wy, board.snap ?? state.gridUm);
        const token = routeMoveGuardRef.current.next();
        dpMove(sx, sy).then((preview) => {
          if (!routeMoveGuardRef.current.isCurrent(token) || !preview.ok) return;
          dispatch({ type: "SET_DRAW_STATE", draw: dpStateFromPreview(draw, preview) });
        });
      }
    }

    // `D`'s own live preview -- same throttle/guard pair as the route tool
    // above (never both active at once: `Router` keeps a route session and
    // a drag session mutually exclusive, see crates/pns/src/router.rs).
    if (board && state.activeTool === "drag" && state.drawState?.kind === "drag") {
      const draw = state.drawState;
      if (routeMoveThrottleRef.current.shouldSend(performance.now())) {
        const [sx, sy] = snapPoint(wx, wy, board.snap ?? state.gridUm);
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
      const [sx, sy] = snapPoint(wx, wy, board.snap ?? state.gridUm);
      if (draw.arc) dispatch({ type: "SET_DRAW_STATE", draw: { ...draw, arc: arcMotion(draw.arc, [sx, sy], arcAngleSnap(state.pcbx.angleSnapMode, e)) } });
      else if (draw.bezier) dispatch({ type: "SET_DRAW_STATE", draw: { ...draw, bezier: bezierMotion(draw.bezier, [sx, sy]) } });
    }

    if (moveMode && state.selection.size > 0) {
      const origin = state.moveOriginUm ?? { x: wx, y: wy };
      const first = [...state.selection][0]!;
      const kind = api.viaById(first) ? "via" : api.shapeById(first) ? "shape" : api.textById(first) ? "text" : "part";
      // pcb_grid_helper.cpp BestSnapAnchor applied to both ends: the
      // delta is "where the (snapped) cursor is now" minus "where the
      // (snapped) cursor started" -- not a plain grid-rounded delta --
      // so a drag that starts and ends at the same anchor is exactly
      // zero movement, and dragging onto a *different* nearby anchor
      // (another part's pad, say) lands precisely on it. Only excludes
      // the dragged item's own anchors for a single-item move; a
      // multi-select drag doesn't exclude any member (a reasonable,
      // documented simplification -- see PARITY-pcb.md).
      const soleRef = state.selection.size === 1 ? first : undefined;
      const [ox, oy] = snapRef(origin.x, origin.y, e, soleRef);
      const [sx, sy] = snapRef(wx, wy, e, soleRef);
      // Preserve whatever R/Shift+R/F have already accumulated on this
      // same preview (useActionRunner.ts) -- a fresh preview object every
      // pointer-move must not reset the live rotate/flip state.
      // (`perRefOffsetUm` is Pack and Move's per-footprint packed shift -- it rides along until the drop.)
      const { rotateQuarterTurns, flipped, perRefOffsetUm } = state.movePreview ?? {};
      dispatch({ type: "SET_MOVE_PREVIEW", preview: { refs: [...state.selection], kind, dxUm: sx - ox, dyUm: sy - oy, rotateQuarterTurns, flipped, perRefOffsetUm } });
      return;
    }

    const drag = dragRef.current;
    if (!drag) return;
    if (drag.kind === "pan") {
      userMovedRef.current = true;
      if (Math.hypot(e.clientX - drag.startScreen[0], e.clientY - drag.startScreen[1]) > PAN_CLICK_TOLERANCE_PX) drag.moved = true;
      dispatch({ type: "SET_VIEW", view: { ...state.view, x: drag.startView[0] + panDeltaX(state.bcx.boardFlipped, e.clientX - drag.startScreen[0]), y: drag.startView[1] + (e.clientY - drag.startScreen[1]) } });
    } else if (drag.kind === "move") {
      const [sx, sy] = snapRef(wx, wy, e, drag.refs.length === 1 ? drag.refs[0] : undefined);
      const dx = sx - drag.snapOrigin[0];
      const dy = sy - drag.snapOrigin[1];
      if (dx !== 0 || dy !== 0) drag.moved = true;
      const { rotateQuarterTurns, flipped } = state.movePreview ?? {};
      dispatch({ type: "SET_MOVE_PREVIEW", preview: drag.moved ? { refs: drag.refs, kind: drag.moveKind, dxUm: dx, dyUm: dy, rotateQuarterTurns, flipped } : null });
    } else if (drag.kind === "zoneCorner") {
      const [sx, sy] = snapPoint(wx, wy, board?.snap ?? state.gridUm);
      setZoneCornerPreview({ zoneId: drag.zoneId, outline: moveCorner(drag.baseOutline, drag.cornerIndex, sx, sy) });
    } else if (drag.kind === "box") {
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
    const drag = dragRef.current;
    dragRef.current = null;
    if (!drag) return;
    if (drag.kind === "pan") {
      justPannedRef.current = drag.button === 2 && drag.moved;
    } else if (drag.kind === "move") {
      if (drag.moved && state.movePreview) {
        api.commitMove(state.movePreview.refs, state.movePreview.dxUm, state.movePreview.dyUm, state.movePreview.kind, state.movePreview.rotateQuarterTurns, state.movePreview.flipped);
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
        const hits = collectBoxSelection(board, selBox, crossing, state.selectionFilter, state.layerVisible, state.activeLayer, state.highContrast);
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
    }
  };

  /** wx_view_controls.cpp WX_VIEW_CONTROLS::onWheel, via kicad-port/viewControls.ts's handleWheel -- plain wheel zooms about the cursor (ConstantZoomController/AcceleratingZoomController per platform, not an ad hoc factor), Ctrl+wheel pans horizontally, Shift/Alt+wheel pans vertically, matching KiCad's default scroll_modifier_zoom/scroll_modifier_pan_h settings. */
  const onWheel = (e: React.WheelEvent) => {
    e.preventDefault();
    userMovedRef.current = true;
    const rect = containerRef.current!.getBoundingClientRect();
    const input: WheelInput = {
      deltaX: e.deltaX,
      deltaY: e.deltaY,
      shiftKey: e.shiftKey,
      ctrlOrCmd: isMac() ? e.metaKey : e.ctrlKey,
      altKey: e.altKey,
      x: flipLocalX(state.bcx.boardFlipped, rect.width, e.clientX - rect.left),
      y: e.clientY - rect.top,
    };
    const result = handleWheel(state.view, { width: rect.width, height: rect.height }, input, wheelPrefs.settings, wheelPrefs.controller);
    if (result.kind !== "unhandled") dispatch({ type: "SET_VIEW", view: result.kind === "zoom" ? result.view : flipPan(state.bcx.boardFlipped, state.view, result.view) });
  };

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
    const hit = partHit(board.parts, wx, wy);
    if (hit && !state.selection.has(hit.ref)) dispatch({ type: "SET_SELECTION", refs: [hit.ref] });
    const refs = hit ? (state.selection.has(hit.ref) ? [...state.selection] : [hit.ref]) : [...state.selection];
    const placedRefs = refs.filter((r) => api.partByRef(r)?.placed);

    const entries: MenuEntry[] = [
      { label: placedRefs.length > 1 ? `Rotate ${placedRefs.length} Items (R)` : "Rotate Clockwise (Shift+R)", onSelect: () => api.rotateSelection(3), disabled: placedRefs.length === 0 },
      { label: "Rotate Counterclockwise (R)", onSelect: () => api.rotateSelection(1), disabled: placedRefs.length === 0 },
      { label: "Flip Side (F)", onSelect: () => api.flipSelection(), disabled: placedRefs.length === 0 },
      { label: "Move Exactly... (Shift+M)", onSelect: () => dispatch({ type: "SET_MOVE_EXACT_DIALOG_OPEN", open: true }), disabled: placedRefs.length === 0 },
      { label: "Create Array... (Ctrl+T)", onSelect: () => dispatch({ type: "SET_CREATE_ARRAY_DIALOG_OPEN", open: true }), disabled: refs.length === 0 },
      { label: "Copy (Cmd+C)", onSelect: () => api.copySelection(), disabled: refs.length === 0 },
      { label: "Cut (Cmd+X)", onSelect: () => run("common.Interactive.cut"), disabled: refs.length === 0 },
      { label: "Duplicate (Cmd+D)", onSelect: () => api.duplicateSelection(), disabled: refs.length === 0 },
      { label: "Delete (Del)", onSelect: () => run("common.Interactive.delete"), disabled: refs.length === 0 },
    ];
    // align_distribute_tool.cpp's own menu-visibility floors: MoreThan(1)
    // for align, MoreThan(2) for distribute -- see kicad-port/
    // alignDistribute.ts's doc for why this is placed-footprints-only.
    if (placedRefs.length > 1) {
      entries.push(
        { label: "Align Left", onSelect: () => api.alignSelection("left") },
        { label: "Align Right", onSelect: () => api.alignSelection("right") },
        { label: "Align Top", onSelect: () => api.alignSelection("top") },
        { label: "Align Bottom", onSelect: () => api.alignSelection("bottom") },
        { label: "Align Center Horizontally", onSelect: () => api.alignSelection("centerX") },
        { label: "Align Center Vertically", onSelect: () => api.alignSelection("centerY") }
      );
    }
    if (placedRefs.length > 2) {
      entries.push(
        { label: "Distribute Horizontally (Even Gaps)", onSelect: () => api.distributeSelection("x", "gaps") },
        { label: "Distribute Horizontally (By Centers)", onSelect: () => api.distributeSelection("x", "centers") },
        { label: "Distribute Vertically (Even Gaps)", onSelect: () => api.distributeSelection("y", "gaps") },
        { label: "Distribute Vertically (By Centers)", onSelect: () => api.distributeSelection("y", "centers") }
      );
    }
    // Task item 7: `GLOBAL_EDIT_TOOL`'s "Switch Dimension Arrows" is a
    // context-menu-only action in source too (no menus.json/toolbars.json
    // entry in this extraction either).
    if (refs.some((r) => api.dimensionById(r))) {
      entries.push({ label: "Switch Dimension Arrows", onSelect: () => run("pcbnew.InteractiveDrawing.changeDimensionArrows") });
    }
    if (refs.length === 1) {
      const part = api.partByRef(refs[0]!);
      const net = part?.pads?.[0]?.net ?? null;
      entries.push({ label: state.netHighlight ? "Clear Net Highlight" : "Highlight Net (`)", onSelect: () => dispatch({ type: "SET_NET_HIGHLIGHT", net: state.netHighlight ? null : net }), disabled: !net });

      // pcb_point_editor.cpp: right-clicking a corner of the single
      // selected zone offers to delete just that corner, same floor
      // `Cmd::SetZoneOutline` itself enforces (a zone always keeps 3+ points).
      const zone = api.zoneById(refs[0]!);
      if (zone) {
        const idx = findNearestCorner(zone.outline, wx, wy, zoneCornerToleranceUm(state.view.scale));
        if (idx != null) {
          const next = removeCorner(zone.outline, idx);
          entries.push({
            label: "Delete Corner",
            onSelect: () => {
              if (next) api.cmd({ op: "set_zone_outline", id: zone.id, outline: next.map(([x, y]) => ({ x, y })) });
            },
            disabled: !next,
          });
        }
      }
    }
    // board_editor_control.cpp ZONE_CONTEXT_MENU, which the selection tool's menu carries when only zones are selected
    // (`SELECTION_CONDITIONS::OnlyTypes( { PCB_ZONE_T } )`) -- flat here, with its "Zone Priority" submenu's four entries after it.
    if (refs.length > 0 && refs.every((r) => api.zoneById(r))) {
      const one = refs.length === 1;
      entries.push(
        { label: "Draft Fill Selected Zone(s)", onSelect: () => run("pcbnew.ZoneFiller.zoneFill") },
        { label: "Fill All Zones", onSelect: () => run("pcbnew.ZoneFiller.zoneFillAll") },
        { label: "Unfill Selected Zone(s)", onSelect: () => run("pcbnew.ZoneFiller.zoneUnfill") },
        { label: "Unfill All Zones", onSelect: () => run("pcbnew.ZoneFiller.zoneUnfillAll") },
        { label: "Merge Zones", onSelect: () => run("pcbnew.EditorControl.zoneMerge"), disabled: refs.length < 2 },
        { label: "Duplicate Zone onto Layer...", onSelect: () => run("pcbnew.EditorControl.zoneDuplicate"), disabled: !one },
        { label: "Add a Zone Cutout", onSelect: () => run("pcbnew.InteractiveDrawing.zoneCutout"), disabled: !one },
        { label: "Add a Similar Zone", onSelect: () => run("pcbnew.InteractiveDrawing.similarZone"), disabled: !one },
        { label: "Zone Priority: Move to Top", onSelect: () => run("pcbnew.EditorControl.zonePriorityMoveToTop"), disabled: !one },
        { label: "Zone Priority: Raise", onSelect: () => run("pcbnew.EditorControl.zonePriorityRaise"), disabled: !one },
        { label: "Zone Priority: Lower", onSelect: () => run("pcbnew.EditorControl.zonePriorityLower"), disabled: !one },
        { label: "Zone Priority: Move to Bottom", onSelect: () => run("pcbnew.EditorControl.zonePriorityMoveToBottom"), disabled: !one },
        { label: "Zone Manager...", onSelect: () => run("pcbnew.Control.zonesManager") }
      );
    }
    if (!hit && refs.length === 0 && board.outline) {
      const bounds = boundsOfPoints(board.outline);
      const rect = containerRef.current?.getBoundingClientRect();
      if (bounds && rect) {
        entries.push({ label: "Zoom to Fit (Ctrl+Home)", onSelect: () => dispatch({ type: "SET_VIEW", view: fitTransform(bounds, rect.width, rect.height) }) });
      }
    }
    setContextMenu({ x: e.clientX, y: e.clientY, entries });
  };

  // Escape/M/rotate/delete/undo/redo/net-highlight-toggle are all
  // handled globally now (actions/useGlobalHotkeys.ts), driven by
  // src/kicad/actions.json's real hotkeys through the same registry the
  // menu bar and toolbars use, reading/writing the store's activeTool
  // and cursorUm. Enter-finishes-the-current-route/zone/shape is the one
  // interaction left here: it's a plain UI convention this app is
  // choosing for its own click-to-add-points tools, not a KiCad dotted
  // action with a real extracted hotkey, so it doesn't belong in that
  // global registry the way a real one does.
  const onCanvasKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && state.drawState) {
      e.preventDefault();
      finishDraw();
    }
    // `/` flips posture while routing (router_tool.cpp's own hotkey for
    // this -- real pcbnew binds it with no separate registered
    // TOOL_ACTION the way most hotkeys get one, so it's handled directly
    // here rather than through useActionRunner.ts's dotted-action
    // registry, same as this app's own Enter-to-finish convention just
    // above). Re-requests the preview at the last known cursor position so
    // the flip is visible immediately rather than waiting for the next
    // mouse pixel to move.
    if (e.key === "/" && state.drawState?.kind === "route" && state.cursorUm) {
      e.preventDefault();
      const draw = state.drawState;
      const token = routeMoveGuardRef.current.next();
      routeMove(state.cursorUm.x, state.cursorUm.y, true).then((preview) => {
        if (!routeMoveGuardRef.current.isCurrent(token) || !preview.ok) return;
        dispatch({ type: "SET_DRAW_STATE", draw: drawStateFromPreview(draw, preview) });
      });
    }
    // Same `/` posture flip, for the diff-pair tool's own spine posture.
    if (e.key === "/" && state.drawState?.kind === "diffpair" && state.cursorUm) {
      e.preventDefault();
      const draw = state.drawState;
      const token = routeMoveGuardRef.current.next();
      dpMove(state.cursorUm.x, state.cursorUm.y, true).then((preview) => {
        if (!routeMoveGuardRef.current.isCurrent(token) || !preview.ok) return;
        dispatch({ type: "SET_DRAW_STATE", draw: dpStateFromPreview(draw, preview) });
      });
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
    if (state.drawState) {
      finishDraw();
      return;
    }
    if (!board) return;
    // pcb_point_editor.cpp: double-clicking a single selected zone's edge
    // (not one of its corners -- grabbing a corner is onPointerDown's own
    // drag-start check, above) adds a new corner there.
    if (state.activeTool === "select" && state.selection.size === 1) {
      const zone = api.zoneById([...state.selection][0]!);
      if (zone) {
        const [wx, wy] = worldAt(e);
        const toleranceUm = zoneCornerToleranceUm(state.view.scale);
        if (findNearestCorner(zone.outline, wx, wy, toleranceUm) == null) {
          const insertAt = findNearestEdgeInsertionIndex(zone.outline, wx, wy, toleranceUm);
          if (insertAt != null) {
            const [sx, sy] = snapPoint(wx, wy, board.snap ?? state.gridUm);
            const next = insertCorner(zone.outline, insertAt, sx, sy);
            api.cmd({ op: "set_zone_outline", id: zone.id, outline: next.map(([x, y]) => ({ x, y })) });
            return;
          }
        }
      }
    }
    let refs = [...state.selection];
    if (refs.length === 0) {
      const [wx, wy] = worldAt(e);
      const toleranceUm = Math.max(150, 6 / state.view.scale);
      const onePixelUm = 1 / state.view.scale;
      const candidates = pickSelectionCandidates(board, wx, wy, toleranceUm, onePixelUm, state.selectionFilter, state.layerVisible, state.activeLayer, state.highContrast, state.selection, false, false);
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
      const groups = board.drawings?.groups ?? [];
      const hitGroup = groups.find((g) => g.id === refs[0] || g.member_ids.includes(refs[0]!));
      if (hitGroup && hitGroup.id !== state.enteredGroupId) {
        dispatch({ type: "SET_ENTERED_GROUP", id: hitGroup.id });
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
      onWheel={onWheel}
      onContextMenu={onContextMenu}
      data-armed={state.armed ? "true" : "false"}
    >
      <canvas ref={canvasRef} />
      {!board && <div className="pcb-canvas-empty">{state.boardError ?? "Loading board…"}</div>}
      {contextMenu && <ContextMenu x={contextMenu.x} y={contextMenu.y} entries={contextMenu.entries} onClose={() => setContextMenu(null)} />}
    </div>
  );
}
