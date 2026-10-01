// The PCB canvas. Owns the <canvas> element, the render loop, and the
// view/selection/edit pointer+keyboard interactions:
//   - wheel zoom/pan exactly like wx_view_controls.cpp's onWheel (see
//     kicad-port/viewControls.ts: plain wheel zooms about the cursor,
//     Ctrl+wheel pans horizontally, Shift/Alt+wheel pans vertically);
//     middle- and right-drag both pan (KiCad's own drag_middle/drag_right
//     defaults), edge auto-pan while dragging (off by default, same as
//     KiCad -- state.autoPanEnabled).
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
import { useStudioApi, useStudioDispatch, useStudioState } from "../../state/store";
import type { ToolId } from "../../state/store";
import { boundsOfPoints, fitTransform, screenToWorld, panByWorldDelta } from "./view";
import { paintBoard } from "./painter";
import { layerColor } from "./layers";
import { snapPoint } from "./gridHelper";
import { findRouteAnchor, posture45, commitRoute } from "./routing";
import { itemHitsAt } from "./itemHitTest";
import { ContextMenu, type MenuEntry } from "./ContextMenu";
import { handleWheel, computeAutoPanDirection, computeAutoPanStep, DEFAULT_VIEW_CONTROL_SETTINGS, type WheelInput } from "../../kicad-port/viewControls";
import { pickDefaultZoomController, type ZoomController } from "../../kicad-port/zoomController";
import { isMac } from "../../platform";
import "../../styles/canvas.css";

/** wx_view_controls.cpp onButton: MiddleDown/RightDown both start DRAG_PANNING by default (m_dragMiddle/m_dragRight == MOUSE_DRAG_ACTION::PAN). A plain click (no real movement) of the right button still opens the context menu -- see onContextMenu's `justPanned` check -- same as source's right button also being each platform's native context-menu trigger. */
const PAN_BUTTONS = new Set([1, 2]);
/** Screen-px movement past which a right-button press counts as a pan-drag rather than a click-to-open-the-context-menu. */
const PAN_CLICK_TOLERANCE_PX = 4;

const LONG_PRESS_MS = 500;
const LONG_PRESS_MOVE_TOLERANCE_PX = 6;
/** A click within this many board um of a pad/via/track-end counts as landing on it -- generous enough to be usable at a typical zoom without needing pixel-perfect precision, same idea as pcb_grid_helper's own anchor snapping (not ported here, see gridHelper.ts). */
const ANCHOR_SNAP_UM = 500;
/** No per-board "default graphic line width" setting exists (board_rules only covers track/via) -- a plain 0.15mm default, same order of magnitude as KiCad's own out-of-the-box default (0.15-0.2mm silkscreen line width, by version/theme). */
const DEFAULT_STROKE_WIDTH_UM = 150;
/** How many points finish a given drawing-tool shape by itself, once reached, without waiting for an explicit Enter/double-click -- a plain 2-point line/rect/circle doesn't need a third confirmation the way a polygon does. Arc is start/mid/end (3); zone/polygon/route have no auto-finish (arbitrary length). */
function shapeAutoFinishCount(kind: "segment" | "arc" | "rect" | "circle" | "polygon"): number | null {
  switch (kind) {
    case "segment":
    case "rect":
    case "circle":
      return 2;
    case "arc":
      return 3;
    case "polygon":
      return null;
  }
}

/** `pts` -> the matching `CmdShape` variant for `add_shape`, or null if there aren't enough points yet -- the IR's own point-count floor per kind (a polygon needs 3+, everything else exactly the fixed count `shapeAutoFinishCount` already enforces before this is ever called with too few). */
function shapeFromPoints(kind: "segment" | "arc" | "rect" | "circle" | "polygon", pts: [number, number][], layer: string): CmdShape | null {
  const p = (i: number) => ({ x: pts[i]![0], y: pts[i]![1] });
  switch (kind) {
    case "segment":
      return pts.length >= 2 ? { kind: "segment", layer, stroke_width: DEFAULT_STROKE_WIDTH_UM, filled: false, start: p(0), end: p(1) } : null;
    case "rect":
      return pts.length >= 2 ? { kind: "rect", layer, stroke_width: DEFAULT_STROKE_WIDTH_UM, filled: false, start: p(0), end: p(1) } : null;
    case "circle":
      return pts.length >= 2 ? { kind: "circle", layer, stroke_width: DEFAULT_STROKE_WIDTH_UM, filled: false, center: p(0), end: p(1) } : null;
    case "arc":
      return pts.length >= 3 ? { kind: "arc", layer, stroke_width: DEFAULT_STROKE_WIDTH_UM, filled: false, start: p(0), mid: p(1), end: p(2) } : null;
    case "polygon":
      return pts.length >= 3 ? { kind: "polygon", layer, stroke_width: DEFAULT_STROKE_WIDTH_UM, filled: true, pts: pts.map((_, i) => p(i)) } : null;
  }
}

const SHAPE_TOOL_KIND: Partial<Record<ToolId, "segment" | "arc" | "rect" | "circle" | "polygon">> = {
  draw_segment: "segment",
  draw_arc: "arc",
  draw_rect: "rect",
  draw_circle: "circle",
  draw_polygon: "polygon",
};

type DragState =
  | { kind: "pan"; button: 1 | 2; startScreen: [number, number]; startView: [number, number]; moved: boolean }
  | { kind: "move"; refs: string[]; moveKind: "part" | "via" | "shape" | "text"; startWorld: [number, number]; moved: boolean }
  | { kind: "box"; startWorld: [number, number]; startScreen: [number, number]; additive: boolean };

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
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const [marquee, setMarquee] = useState<{ x0: number; y0: number; x1: number; y1: number; crossing: boolean } | null>(null);
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
  const zoomControllerRef = useRef<ZoomController>(pickDefaultZoomController(isMac()));
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
  // border it is. Off entirely unless state.autoPanEnabled (KiCad's own
  // default, see store.tsx) -- a persistent rAF loop rather than
  // per-dependency effect restarts, so it reads refs fresh each frame
  // instead of needing to be re-created on every state change.
  useEffect(() => {
    let raf = 0;
    const tick = () => {
      raf = requestAnimationFrame(tick);
      const s = stateRef.current;
      if (!s.autoPanEnabled) return;
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
      const step = computeAutoPanStep(dir, screenSize, s.view.scale, DEFAULT_VIEW_CONTROL_SETTINGS.autoPanMargin, DEFAULT_VIEW_CONTROL_SETTINGS.autoPanAcceleration);
      if (!step) return;
      dispatch({ type: "SET_VIEW", view: panByWorldDelta(s.view, step.x, step.y) });
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
    if (!canvas || !board || containerSize.width === 0) return;
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
    ctx.translate(state.view.x, state.view.y);
    ctx.scale(state.view.scale || 1, state.view.scale || 1);
    paintBoard(ctx, state.view, width, height, board, {
      selection: state.selection,
      hot: state.hot,
      netHighlight: state.netHighlight,
      showRatsnest: state.showRatsnest,
      ratsnestCurved: state.ratsnestCurved,
      ratsnestEdges: state.ratsnest?.edges ?? null,
      drcViolations: state.drc?.violations ?? null,
      drcSelected: state.drcSelected,
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
      drawState: state.drawState,
      cursorUm: state.cursorUm,
      activeTool: state.activeTool,
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
      const sx = state.cursorUm.x * state.view.scale + state.view.x;
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
  }, [board, state.view, state.selection, state.hot, state.netHighlight, state.showRatsnest, state.ratsnestCurved, state.ratsnest, state.drc, state.drcSelected, state.layerVisible, state.layerOpacity, state.activeLayer, state.highContrast, state.gridUm, state.gridVisible, state.movePreview, state.cursorUm, state.fullscreenCrosshair, state.sketchPads, state.sketchTracks, state.sketchVias, state.drawState, state.activeTool, marquee, containerSize]);

  const worldAt = useCallback(
    (e: { clientX: number; clientY: number }): [number, number] => {
      const rect = containerRef.current!.getBoundingClientRect();
      return screenToWorld(state.view, e.clientX - rect.left, e.clientY - rect.top);
    },
    [state.view]
  );

  /** pcb_selection_tool.cpp: where several items overlap, Alt-click (and, here, a press-and-hold) shows a picker instead of always taking the topmost. */
  const disambiguate = useCallback(
    (candidates: Part[], screenX: number, screenY: number) => {
      if (candidates.length === 0) return;
      if (candidates.length === 1) {
        dispatch({ type: "SET_SELECTION", refs: [candidates[0]!.ref] });
        return;
      }
      setContextMenu({
        x: screenX,
        y: screenY,
        entries: candidates.map((p) => ({
          label: `${p.ref}${p.value ? ` (${p.value})` : ""}`,
          onSelect: () => dispatch({ type: "SET_SELECTION", refs: [p.ref] }),
        })),
      });
    },
    [dispatch]
  );

  const clearLongPress = () => {
    if (longPressRef.current) {
      clearTimeout(longPressRef.current.timer);
      longPressRef.current = null;
    }
  };

  /** Route/zone/shape tools all share "click adds a point, Enter/double-click finishes" -- this commits whatever's accumulated in state.drawState, per its kind. Shared with the F ("Attempt Finish") hotkey for the route case specifically (useActionRunner.ts) via routing.ts's commitRoute, so the two can never disagree about what finishing a route means. */
  const finishDraw = useCallback(() => {
    const draw = state.drawState;
    if (!draw) return;
    if (draw.kind === "route") {
      commitRoute(draw, api.cmd);
      dispatch({ type: "SET_DRAW_STATE", draw: null });
    } else if (draw.kind === "zone") {
      if (draw.pts.length >= 3) {
        dispatch({ type: "SET_ZONE_PENDING", outline: draw.pts });
        dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
      }
      dispatch({ type: "SET_DRAW_STATE", draw: null });
    } else if (draw.kind === "shape") {
      const shape = shapeFromPoints(draw.shapeKind, draw.pts, state.activeLayer ?? "F.SilkS");
      if (shape) api.cmd({ op: "add_shape", shape });
      dispatch({ type: "SET_DRAW_STATE", draw: null });
    }
  }, [state.drawState, state.activeLayer, api, dispatch]);

  const onPointerDown = (e: React.PointerEvent) => {
    (e.target as Element).setPointerCapture(e.pointerId);
    const [wx, wy] = worldAt(e);
    setContextMenu(null);

    // Route/via/zone/drawing/text tools: a click either starts, extends,
    // or (for via/text) completes one placement -- entirely separate
    // from the select/move flow below, which only applies to the
    // "select"/"move" tools.
    if (board && e.button === 0 && !e.altKey) {
      const [sx, sy] = snapPoint(wx, wy, board.snap ?? state.gridUm);

      if (state.activeTool === "via") {
        const anchor = findRouteAnchor(board, sx, sy, ANCHOR_SNAP_UM);
        if (!anchor) {
          dispatch({ type: "TOAST", message: "Click a pad, via, or track end -- a via needs a net.", kind: "error" });
          return;
        }
        const rules = board.board_rules;
        api.cmd({ op: "add_via", net: anchor.net, x: sx, y: sy, drill: rules?.via_drill ?? 300, diameter: rules?.via_diameter ?? 600, from_layer: "F.Cu", to_layer: "B.Cu" });
        return;
      }

      if (state.activeTool === "route") {
        const draw = state.drawState;
        if (!draw || draw.kind !== "route") {
          const anchor = findRouteAnchor(board, sx, sy, ANCHOR_SNAP_UM);
          if (!anchor) {
            dispatch({ type: "TOAST", message: "Start a route from a pad, via, or track end.", kind: "error" });
            return;
          }
          const layer = anchor.layer ?? state.activeLayer ?? board.layers[0] ?? "F.Cu";
          dispatch({ type: "SET_DRAW_STATE", draw: { kind: "route", net: anchor.net, layer, width: board.board_rules?.track_width ?? 250, pts: [anchor.at] } });
          return;
        }
        const last = draw.pts[draw.pts.length - 1]!;
        const constrained = posture45(last, [sx, sy]);
        const [fx, fy] = snapPoint(constrained[0], constrained[1], board.snap ?? state.gridUm);
        dispatch({ type: "SET_DRAW_STATE", draw: { ...draw, pts: [...draw.pts, [fx, fy]] } });
        return;
      }

      if (state.activeTool === "zone") {
        const draw = state.drawState;
        const pts: [number, number][] = draw?.kind === "zone" ? [...draw.pts, [sx, sy]] : [[sx, sy]];
        dispatch({ type: "SET_DRAW_STATE", draw: { kind: "zone", pts } });
        return;
      }

      const shapeKind = SHAPE_TOOL_KIND[state.activeTool];
      if (shapeKind) {
        const draw = state.drawState;
        const already = draw?.kind === "shape" && draw.shapeKind === shapeKind ? draw.pts : [];
        let point: [number, number] = [sx, sy];
        if (already.length > 0 && (shapeKind === "segment" || shapeKind === "rect")) {
          const last = already[already.length - 1]!;
          const constrained = posture45(last, [sx, sy]);
          point = snapPoint(constrained[0], constrained[1], board.snap ?? state.gridUm);
        }
        const pts = [...already, point];
        const finishAt = shapeAutoFinishCount(shapeKind);
        if (finishAt !== null && pts.length >= finishAt) {
          const shape = shapeFromPoints(shapeKind, pts, state.activeLayer ?? "F.SilkS");
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

    if (moveMode) {
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
      if (state.movePreview) api.commitMove(state.movePreview.refs, state.movePreview.dxUm, state.movePreview.dyUm);
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

    if (e.altKey && board && state.selectionFilter.footprints) {
      disambiguate(partsAt(board.parts, wx, wy), e.clientX, e.clientY);
      return;
    }

    // Long-press: if the pointer stays down here without much movement,
    // fire the same disambiguation a moment from now, pre-empting
    // whatever plain click/drag was about to happen.
    const startScreen: [number, number] = [e.clientX, e.clientY];
    clearLongPress();
    longPressRef.current = {
      startScreen,
      timer: setTimeout(() => {
        longPressRef.current = null;
        dragRef.current = null;
        setMarquee(null);
        if (board && state.selectionFilter.footprints) disambiguate(partsAt(board.parts, wx, wy), startScreen[0], startScreen[1]);
      }, LONG_PRESS_MS),
    };

    const hit = board && state.selectionFilter.footprints ? partHit(board.parts, wx, wy) : null;
    if (hit) {
      const additive = e.shiftKey;
      const already = state.selection.has(hit.ref);
      let refs: string[];
      if (additive) {
        dispatch({ type: "TOGGLE_SELECTION", ref: hit.ref });
        refs = already ? [...state.selection].filter((r) => r !== hit.ref) : [...state.selection, hit.ref];
      } else if (already && state.selection.size > 1) {
        refs = [...state.selection]; // keep the group for a drag
      } else {
        dispatch({ type: "SET_SELECTION", refs: [hit.ref] });
        refs = [hit.ref];
      }
      dragRef.current = { kind: "move", refs, moveKind: "part", startWorld: [wx, wy], moved: false };
      return;
    }

    // Tracks/vias/zones/shapes/text (item 7): tried only once no
    // footprint is under the click, matching KiCad's own click priority
    // (a copper/graphic item under a footprint never steals its click).
    if (board) {
      const itemHit = itemHitsAt(board, wx, wy, Math.max(150, 6 / state.view.scale))[0];
      if (itemHit) {
        const additive = e.shiftKey;
        if (additive) dispatch({ type: "TOGGLE_SELECTION", ref: itemHit.id });
        else dispatch({ type: "SET_SELECTION", refs: [itemHit.id] });
        // Tracks/zones have no move_* Cmd (see api/types.ts) -- selectable, not draggable.
        if (itemHit.kind === "via" || itemHit.kind === "shape" || itemHit.kind === "text") {
          dragRef.current = { kind: "move", refs: [itemHit.id], moveKind: itemHit.kind, startWorld: [wx, wy], moved: false };
        }
        return;
      }
    }

    dragRef.current = { kind: "box", startWorld: [wx, wy], startScreen: [e.clientX, e.clientY], additive: e.shiftKey };
    if (!e.shiftKey) dispatch({ type: "CLEAR_SELECTION" });
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

    if (moveMode && state.selection.size > 0) {
      const origin = state.moveOriginUm ?? { x: wx, y: wy };
      const [dx, dy] = snapPoint(wx - origin.x, wy - origin.y, board?.snap ?? state.gridUm);
      const first = [...state.selection][0]!;
      const kind = api.viaById(first) ? "via" : api.shapeById(first) ? "shape" : api.textById(first) ? "text" : "part";
      dispatch({ type: "SET_MOVE_PREVIEW", preview: { refs: [...state.selection], kind, dxUm: dx, dyUm: dy } });
      return;
    }

    const drag = dragRef.current;
    if (!drag) return;
    if (drag.kind === "pan") {
      userMovedRef.current = true;
      if (Math.hypot(e.clientX - drag.startScreen[0], e.clientY - drag.startScreen[1]) > PAN_CLICK_TOLERANCE_PX) drag.moved = true;
      dispatch({ type: "SET_VIEW", view: { ...state.view, x: drag.startView[0] + (e.clientX - drag.startScreen[0]), y: drag.startView[1] + (e.clientY - drag.startScreen[1]) } });
    } else if (drag.kind === "move") {
      const [dx, dy] = snapPoint(wx - drag.startWorld[0], wy - drag.startWorld[1], board?.snap ?? state.gridUm);
      if (dx !== 0 || dy !== 0) drag.moved = true;
      dispatch({ type: "SET_MOVE_PREVIEW", preview: drag.moved ? { refs: drag.refs, kind: drag.moveKind, dxUm: dx, dyUm: dy } : null });
    } else if (drag.kind === "box") {
      const rect = containerRef.current!.getBoundingClientRect();
      const x0 = drag.startScreen[0] - rect.left,
        y0 = drag.startScreen[1] - rect.top;
      const x1 = e.clientX - rect.left,
        y1 = e.clientY - rect.top;
      setMarquee({ x0, y0, x1, y1, crossing: x1 < x0 });
    }
  };

  const onPointerUp = () => {
    clearLongPress();
    const drag = dragRef.current;
    dragRef.current = null;
    if (!drag) return;
    if (drag.kind === "pan") {
      justPannedRef.current = drag.button === 2 && drag.moved;
    } else if (drag.kind === "move") {
      if (drag.moved && state.movePreview) api.commitMove(state.movePreview.refs, state.movePreview.dxUm, state.movePreview.dyUm, state.movePreview.kind);
      else dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
    } else if (drag.kind === "box" && board) {
      const crossing = marquee?.crossing ?? false;
      const [x0, y0] = drag.startWorld;
      const [x1, y1] = marquee ? screenToWorld(state.view, marquee.x1, marquee.y1) : [x0, y0];
      const minX = Math.min(x0, x1),
        maxX = Math.max(x0, x1),
        minY = Math.min(y0, y1),
        maxY = Math.max(y0, y1);
      const hits = state.selectionFilter.footprints
        ? board.parts.filter((p) => {
            if (!p.placed || !p.courtyard) return false;
            const [px0, py0, px1, py1] = p.courtyard;
            return crossing ? px0 <= maxX && px1 >= minX && py0 <= maxY && py1 >= minY : px0 >= minX && px1 <= maxX && py0 >= minY && py1 <= maxY;
          })
        : [];
      if (hits.length > 0) {
        const refs = drag.additive ? [...new Set([...state.selection, ...hits.map((p) => p.ref)])] : hits.map((p) => p.ref);
        dispatch({ type: "SET_SELECTION", refs });
      }
      setMarquee(null);
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
      x: e.clientX - rect.left,
      y: e.clientY - rect.top,
    };
    const result = handleWheel(state.view, { width: rect.width, height: rect.height }, input, DEFAULT_VIEW_CONTROL_SETTINGS, zoomControllerRef.current);
    if (result.kind !== "unhandled") dispatch({ type: "SET_VIEW", view: result.view });
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
      { label: "Delete (Del)", onSelect: () => api.ripSelection(), disabled: refs.length === 0 },
    ];
    if (refs.length === 1) {
      const part = api.partByRef(refs[0]!);
      const net = part?.pads?.[0]?.net ?? null;
      entries.push({ label: state.netHighlight ? "Clear Net Highlight" : "Highlight Net (`)", onSelect: () => dispatch({ type: "SET_NET_HIGHLIGHT", net: state.netHighlight ? null : net }), disabled: !net });
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
  };

  const onDoubleClick = () => {
    if (state.drawState) finishDraw();
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
