// The PCB canvas. Owns the <canvas> element, the render loop, and the
// view/selection/edit pointer+keyboard interactions:
//   - wheel zoom about the cursor; middle-drag pan (KiCad keeps left-drag
//     for box select, not pan -- see pcbnew/tools/pcb_selection_tool.cpp,
//     which this session could not read, so the *exact* pan modifier set
//     -- e.g. whether space+left-drag also pans -- is not confirmed; only
//     middle-drag is wired here).
//   - click to select (Shift adds); drag from empty space box-selects,
//     left-to-right = window (fully enclosed), right-to-left = crossing
//     (touching) -- crates/ops/src/view.rs has no notion of this, it's
//     pure frontend geometry against each part's courtyard.
//   - drag a selected part to move it, previewed locally, committed with
//     `move_to` on drop (see state/store.tsx `commitMove`); M starts the
//     same preview without holding the button, click commits, Esc cancels.
//   - R / Shift+R rotate by a quarter turn each way; Delete rips.
import React, { useCallback, useEffect, useRef, useState } from "react";
import type { Part } from "../../api/types";
import { useStudioApi, useStudioDispatch, useStudioState } from "../../state/store";
import { boundsOfPoints, fitTransform, screenToWorld, zoomAbout } from "./view";
import { paintBoard } from "./painter";
import { layerColor } from "./layers";
import { snapPoint } from "./gridHelper";
import "../../styles/canvas.css";

type DragState = { kind: "pan"; startScreen: [number, number]; startView: [number, number] } | { kind: "move"; refs: string[]; startWorld: [number, number]; moved: boolean } | { kind: "box"; startWorld: [number, number]; startScreen: [number, number]; additive: boolean };

function partHit(parts: Part[], xUm: number, yUm: number): Part | null {
  // Topmost = last drawn = iterate back-to-front so an overlapping part on
  // top wins, matching paintBoard's draw order.
  for (let i = parts.length - 1; i >= 0; i--) {
    const p = parts[i]!;
    if (!p.placed || !p.courtyard) continue;
    const [x0, y0, x1, y1] = p.courtyard;
    if (xUm >= x0 && xUm <= x1 && yUm >= y0 && yUm <= y1) return p;
  }
  return null;
}

export function Canvas() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const [marquee, setMarquee] = useState<{ x0: number; y0: number; x1: number; y1: number; crossing: boolean } | null>(null);
  const [moveMode, setMoveMode] = useState(false);
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 });

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
      layerVisible: state.layerVisible,
      layerOpacity: state.layerOpacity,
      activeLayer: state.activeLayer,
      highContrast: state.highContrast,
      gridUm: state.gridUm,
      gridVisible: state.gridVisible,
      movePreview: state.movePreview,
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
  }, [board, state.view, state.selection, state.hot, state.netHighlight, state.showRatsnest, state.layerVisible, state.layerOpacity, state.activeLayer, state.highContrast, state.gridUm, state.gridVisible, state.movePreview, state.cursorUm, state.fullscreenCrosshair, marquee, containerSize]);

  const worldAt = useCallback(
    (e: { clientX: number; clientY: number }): [number, number] => {
      const rect = containerRef.current!.getBoundingClientRect();
      return screenToWorld(state.view, e.clientX - rect.left, e.clientY - rect.top);
    },
    [state.view]
  );

  const onPointerDown = (e: React.PointerEvent) => {
    (e.target as Element).setPointerCapture(e.pointerId);
    const [wx, wy] = worldAt(e);

    if (moveMode) {
      setMoveMode(false);
      if (state.movePreview) api.commitMove(state.movePreview.refs, state.movePreview.dxUm, state.movePreview.dyUm);
      return;
    }
    if (state.armed) {
      const [sx, sy] = snapPoint(wx, wy, board?.snap ?? state.gridUm);
      api.placeArmedAt(sx, sy);
      return;
    }
    if (e.button === 1) {
      dragRef.current = { kind: "pan", startScreen: [e.clientX, e.clientY], startView: [state.view.x, state.view.y] };
      return;
    }
    if (e.button !== 0) return;

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
      dragRef.current = { kind: "move", refs, startWorld: [wx, wy], moved: false };
      return;
    }
    dragRef.current = { kind: "box", startWorld: [wx, wy], startScreen: [e.clientX, e.clientY], additive: e.shiftKey };
    if (!e.shiftKey) dispatch({ type: "CLEAR_SELECTION" });
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const [wx, wy] = worldAt(e);
    dispatch({ type: "SET_CURSOR", at: { x: wx, y: wy } });

    if (moveMode && state.selection.size > 0) {
      const origin = state.moveOriginUm ?? { x: wx, y: wy };
      const [dx, dy] = snapPoint(wx - origin.x, wy - origin.y, board?.snap ?? state.gridUm);
      dispatch({ type: "SET_MOVE_PREVIEW", preview: { refs: [...state.selection], dxUm: dx, dyUm: dy } });
      return;
    }

    const drag = dragRef.current;
    if (!drag) return;
    if (drag.kind === "pan") {
      userMovedRef.current = true;
      dispatch({ type: "SET_VIEW", view: { ...state.view, x: drag.startView[0] + (e.clientX - drag.startScreen[0]), y: drag.startView[1] + (e.clientY - drag.startScreen[1]) } });
    } else if (drag.kind === "move") {
      const [dx, dy] = snapPoint(wx - drag.startWorld[0], wy - drag.startWorld[1], board?.snap ?? state.gridUm);
      if (dx !== 0 || dy !== 0) drag.moved = true;
      dispatch({ type: "SET_MOVE_PREVIEW", preview: drag.moved ? { refs: drag.refs, dxUm: dx, dyUm: dy } : null });
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
    const drag = dragRef.current;
    dragRef.current = null;
    if (!drag) return;
    if (drag.kind === "move") {
      if (drag.moved && state.movePreview) api.commitMove(state.movePreview.refs, state.movePreview.dxUm, state.movePreview.dyUm);
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

  const onWheel = (e: React.WheelEvent) => {
    e.preventDefault();
    userMovedRef.current = true;
    const rect = containerRef.current!.getBoundingClientRect();
    const factor = Math.exp(-e.deltaY * 0.0015);
    dispatch({ type: "SET_VIEW", view: zoomAbout(state.view, e.clientX - rect.left, e.clientY - rect.top, factor) });
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const mod = e.metaKey || e.ctrlKey; // Cmd on macOS, Ctrl elsewhere -- see the task's hotkey-mapping note
    if (mod && e.key.toLowerCase() === "z") {
      e.preventDefault();
      if (e.shiftKey) api.redo();
      else api.undo();
    } else if (e.key === "r" || e.key === "R") api.rotateSelection(e.shiftKey ? 3 : 1);
    else if (e.key === "Delete" || e.key === "Backspace") api.ripSelection();
    else if (e.key === "Escape") {
      setMoveMode(false);
      dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
      dispatch({ type: "CLEAR_SELECTION" });
    } else if ((e.key === "m" || e.key === "M") && state.selection.size > 0) {
      setMoveMode(true);
      dispatch({ type: "SET_MOVE_ORIGIN", at: state.cursorUm });
    } else if (e.key === "`") {
      const ref = [...state.selection][0];
      const part = ref ? api.partByRef(ref) : undefined;
      const net = part?.pads?.[0]?.net ?? null;
      dispatch({ type: "SET_NET_HIGHLIGHT", net: state.netHighlight ? null : net });
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
      onWheel={onWheel}
      onKeyDown={onKeyDown}
      data-armed={state.armed ? "true" : "false"}
    >
      <canvas ref={canvasRef} />
      {!board && <div className="pcb-canvas-empty">{state.boardError ?? "Loading board…"}</div>}
    </div>
  );
}
