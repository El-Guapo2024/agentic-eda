// The Schematic Editor tab -- a real Canvas2D KiCad-style editor over
// GET /api/schematic's structured data (see schematic/painter.ts), not
// the read-only viewer this used to be (GAPS.md gap #1): selection
// (click + box-select, modifiers ported from kicad-port/selection.ts,
// the same pure logic the PCB canvas's Canvas.tsx already uses) and Move
// (`M`, armed then click-to-drop, live preview) are wired here; Rotate/
// Mirror/Delete dispatch straight through useActionRunner.ts's registry
// the same way the PCB tab's hotkeys do. PCB <-> Schematic cross-probing
// (clicking U1 here highlights it there, and back) is still just both
// views reading the same state.selection/state.netHighlight.
//
// Scope for this pass (see PARITY-sch.md for the full per-action table):
// symbol select/move/rotate('R'/Shift+R)/mirror('X')/delete are wired.
// Drag ('G', wire rubber-banding), wire/label/power-symbol/no-connect
// drawing tools, and a real Properties('E') dialog are not yet -- a
// click still only ever selects a symbol or highlights a wire's net.
import { useEffect, useRef, useState } from "react";
import type { Schematic } from "../api/types";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { boundsOfPoints, fitTransform, zoomAbout } from "./canvas/view";
import { paintSchematic, symbolBounds } from "./schematic/painter";
import { resolveLibSymbol } from "./schematic/libSymbol";
import { GRID } from "./schematic/layout";
import { layerColor } from "./canvas/layers";
import { drawPageAndFrame, drawZoneReferences, drawTitleBlock, drawGridDots, PAGE_WIDTH_UM, PAGE_HEIGHT_UM } from "./schematic/drawingSheet";
import { computeClickModifiers, applySingleClickModifier, isCrossingSelection, applyBoxSelectionModifiers, hasModifier } from "../kicad-port/selection";
import { alignToGrid } from "../kicad-port/gridSnap";
import { isMac } from "../platform";
import "../styles/canvas.css";

type DragState = { kind: "pan"; startScreen: [number, number]; startView: [number, number] } | { kind: "box"; startWorld: [number, number]; startScreen: [number, number] };

/**
 * World-space bounds for the initial fit. KiCad opens a schematic framed
 * on the whole page, not just whatever's drawn on it (an empty sheet
 * still shows the full A4 frame) -- so this is always the page rect,
 * widened to also cover any content that happens to sit outside it
 * (this app doesn't clip/reflow existing symbol positions to the page).
 */
function schematicBounds(sch: Schematic): Array<[number, number]> {
  const pts: Array<[number, number]> = [
    [0, 0],
    [PAGE_WIDTH_UM, PAGE_HEIGHT_UM],
  ];
  for (const s of sch.symbols) {
    const b = symbolBounds(s, sch.lib_symbols);
    pts.push([b.minX, b.minY], [b.maxX, b.maxY]);
  }
  for (const w of sch.wires) pts.push(...w.pts);
  for (const l of sch.labels) pts.push(l.at);
  for (const ps of sch.power_symbols) pts.push(ps.at);
  return pts;
}

function hitSymbol(sch: Schematic, xUm: number, yUm: number): string | null {
  for (let i = sch.symbols.length - 1; i >= 0; i--) {
    const s = sch.symbols[i]!;
    // World-space bbox test -- symbolBounds already accounts for
    // rotation/mirror (and, for a real lib_symbols-resolved symbol, the
    // real graphics' own extent, not just a generic box), so there is no
    // local-space un-rotation to do here.
    const b = symbolBounds(s, sch.lib_symbols);
    if (xUm >= b.minX - 200 && xUm <= b.maxX + 200 && yUm >= b.minY - 200 && yUm <= b.maxY + 200) return s.id;
  }
  return null;
}

/** sch_selection_tool.cpp: a wire is also directly selectable (by id, for Del/move), distinct from the net-highlight click `hitWireNet` below handles when nothing is selectable at that point. Nearest-segment, same threshold convention as the net click. */
function hitWire(sch: Schematic, xUm: number, yUm: number, thresholdUm: number): string | null {
  let best: { id: string; d: number } | null = null;
  for (const w of sch.wires) {
    for (let i = 0; i + 1 < w.pts.length; i++) {
      const [x1, y1] = w.pts[i]!;
      const [x2, y2] = w.pts[i + 1]!;
      const dx = x2 - x1,
        dy = y2 - y1;
      const lenSq = dx * dx + dy * dy || 1;
      let t = ((xUm - x1) * dx + (yUm - y1) * dy) / lenSq;
      t = Math.max(0, Math.min(1, t));
      const px = x1 + t * dx,
        py = y1 + t * dy;
      const d = Math.hypot(xUm - px, yUm - py);
      if (d <= thresholdUm && (!best || d < best.d) && w.id) best = { id: w.id, d };
    }
  }
  return best?.id ?? null;
}

/**
 * Every pin's resolved world-space tip, for the wire tool's "snap to a
 * pin when close" (sch_line_wire_bus_tool.cpp's own grid.BestSnapAnchor
 * on the GRID_CONNECTABLE grid, simplified here to a flat radius check,
 * same scope reduction as `SchematicView.tsx`'s header comment notes for
 * the move tool's own grid-only snap). Only symbols with real resolved
 * `lib_symbols` graphics contribute a pin -- a symbol still drawn as the
 * generic box (no `lib_id` resolved) has no world-space pin geometry
 * computed on this side yet (`layout.ts`'s box layout is local-space
 * only); a wire can still be drawn to one, it just won't snap-assist.
 * See PARITY-sch.md.
 */
function pinSnapPoints(sch: Schematic): Array<[number, number]> {
  const pts: Array<[number, number]> = [];
  for (const s of sch.symbols) {
    const real = resolveLibSymbol(s, sch.lib_symbols);
    if (real) for (const p of real.pins) pts.push(p.tip);
  }
  for (const ps of sch.power_symbols) pts.push(ps.at);
  return pts;
}

function nearestSnapPoint(pts: Array<[number, number]>, xUm: number, yUm: number, thresholdUm: number): [number, number] | null {
  let best: { p: [number, number]; d: number } | null = null;
  for (const p of pts) {
    const d = Math.hypot(p[0] - xUm, p[1] - yUm);
    if (d <= thresholdUm && (!best || d < best.d)) best = { p, d };
  }
  return best?.p ?? null;
}

function hitWireNet(sch: Schematic, xUm: number, yUm: number, thresholdUm: number): string | null {
  let best: { net: string; d: number } | null = null;
  for (const w of sch.wires) {
    for (let i = 0; i + 1 < w.pts.length; i++) {
      const [x1, y1] = w.pts[i]!;
      const [x2, y2] = w.pts[i + 1]!;
      const dx = x2 - x1,
        dy = y2 - y1;
      const lenSq = dx * dx + dy * dy || 1;
      let t = ((xUm - x1) * dx + (yUm - y1) * dy) / lenSq;
      t = Math.max(0, Math.min(1, t));
      const px = x1 + t * dx,
        py = y1 + t * dy;
      const d = Math.hypot(xUm - px, yUm - py);
      if (d <= thresholdUm && (!best || d < best.d)) best = { net: w.net, d };
    }
  }
  return best?.net ?? null;
}

/**
 * pcb_selection_tool.cpp SelectRectArea, narrowed to symbols (the only
 * box-selectable schematic item this pass ports -- wires/labels are a
 * documented gap, see this file's own header comment): "fully enclosed"
 * needs the whole bounding box inside the marquee, "crossing" only needs
 * an overlap.
 */
function collectBoxSelection(sch: Schematic, box: [number, number, number, number], crossing: boolean): string[] {
  const [x0, y0, x1, y1] = box;
  const hits: string[] = [];
  for (const s of sch.symbols) {
    const b = symbolBounds(s, sch.lib_symbols);
    const overlaps = b.minX < x1 && b.maxX > x0 && b.minY < y1 && b.maxY > y0;
    const enclosed = b.minX >= x0 && b.maxX <= x1 && b.minY >= y0 && b.maxY <= y1;
    if (crossing ? overlaps : enclosed) hits.push(s.id);
  }
  return hits;
}

export function SchematicView() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const userMovedRef = useRef(false);
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 });
  const [marquee, setMarquee] = useState<{ x0: number; y0: number; x1: number; y1: number; crossing: boolean } | null>(null);
  const sch = state.schematic;
  const moveMode = state.activeTool === "move";

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

  useEffect(() => {
    if (!sch || userMovedRef.current) return;
    const bounds = boundsOfPoints(schematicBounds(sch));
    if (!bounds || containerSize.width < 50 || containerSize.height < 50) return;
    dispatch({ type: "SET_SCHEMATIC_VIEW", view: fitTransform(bounds, containerSize.width, containerSize.height, 80) });
  }, [sch, containerSize, dispatch]);

  // A live move preview shows as a shifted copy of just the moving
  // symbols -- painter.ts's own paint loop is untouched; this is a
  // display-only substitution, the committed Cmd (api.commitMove) always
  // reads the real position fresh.
  const displaySch: Schematic | null =
    sch && state.movePreview && state.movePreview.kind === "symbol"
      ? {
          ...sch,
          symbols: sch.symbols.map((s) => (state.movePreview!.refs.includes(s.id) ? { ...s, at: [s.at[0] + state.movePreview!.dxUm, s.at[1] + state.movePreview!.dyUm] as [number, number] } : s)),
        }
      : sch;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !sch || !displaySch || containerSize.width === 0) return;
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
    ctx.fillStyle = layerColor("LAYER_SCHEMATIC_BACKGROUND");
    ctx.fillRect(0, 0, width, height);
    ctx.save();
    ctx.translate(state.schematicView.x, state.schematicView.y);
    ctx.scale(state.schematicView.scale || 1, state.schematicView.scale || 1);
    drawPageAndFrame(ctx, state.schematicView);
    drawGridDots(ctx, state.schematicView, width, height, GRID);
    drawZoneReferences(ctx, state.schematicView);
    const tb = sch.title_block;
    drawTitleBlock(ctx, state.schematicView, {
      title: tb?.title || state.board?.name || "untitled",
      date: tb?.date ?? new Date().toISOString().slice(0, 10),
      rev: tb?.rev ?? "",
      company: tb?.company,
      fileName: `${state.board?.name || "schematic"}.kicad_sch`,
      sheetPath: "/",
    });
    paintSchematic(ctx, state.schematicView, displaySch, { selection: state.selection, netHighlight: state.netHighlight });
    if (state.drawState?.kind === "wire") {
      const pts = state.cursorUm ? [...state.drawState.pts, [state.cursorUm.x, state.cursorUm.y] as [number, number]] : state.drawState.pts;
      ctx.strokeStyle = layerColor("LAYER_WIRE");
      ctx.lineWidth = Math.max(150, (1 / state.schematicView.scale) * 1.5);
      ctx.beginPath();
      pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.stroke();
    }
    if (marquee) {
      const x0 = (marquee.x0 - state.schematicView.x) / state.schematicView.scale;
      const y0 = (marquee.y0 - state.schematicView.y) / state.schematicView.scale;
      const x1 = (marquee.x1 - state.schematicView.x) / state.schematicView.scale;
      const y1 = (marquee.y1 - state.schematicView.y) / state.schematicView.scale;
      ctx.strokeStyle = marquee.crossing ? "#4ea1ff" : "#7fe08a";
      ctx.setLineDash([4 / state.schematicView.scale, 3 / state.schematicView.scale]);
      ctx.lineWidth = 1 / state.schematicView.scale;
      ctx.strokeRect(Math.min(x0, x1), Math.min(y0, y1), Math.abs(x1 - x0), Math.abs(y1 - y0));
      ctx.setLineDash([]);
    }
    ctx.restore();
    ctx.restore();
  }, [sch, displaySch, state.schematicView, state.selection, state.netHighlight, containerSize, state.board?.name, marquee, state.drawState, state.cursorUm]);

  const empty = state.schematicError ?? (!sch ? "Loading schematic…" : null);

  const toWorld = (clientX: number, clientY: number): [number, number] => {
    const rect = containerRef.current!.getBoundingClientRect();
    const sx = clientX - rect.left,
      sy = clientY - rect.top;
    return [(sx - state.schematicView.x) / state.schematicView.scale, (sy - state.schematicView.y) / state.schematicView.scale];
  };

  /** `edit_tool_move_fct.cpp`'s grid round-off alone (see kicad-port/gridSnap.ts's header) -- no anchor/pin snap yet, a documented gap (PARITY-sch.md): real eeschema also snaps a move to a nearby pin. */
  const snapToGrid = (xUm: number, yUm: number): [number, number] => {
    const p = alignToGrid({ x: xUm, y: yUm }, GRID, { x: 0, y: 0 }, { ctrlOrCmd: false });
    return [p.x, p.y];
  };

  return (
    <div
      ref={containerRef}
      className="pcb-canvas-container"
      style={{ cursor: dragRef.current?.kind === "pan" ? "grabbing" : moveMode ? "move" : "default" }}
      onWheel={(e) => {
        if (!sch) return;
        e.preventDefault();
        userMovedRef.current = true;
        const rect = containerRef.current!.getBoundingClientRect();
        const px = e.clientX - rect.left,
          py = e.clientY - rect.top;
        dispatch({ type: "SET_SCHEMATIC_VIEW", view: zoomAbout(state.schematicView, px, py, Math.exp(-e.deltaY * 0.0015)) });
      }}
      onPointerDown={(e) => {
        if (!sch) return;
        if (e.button === 1) {
          userMovedRef.current = true;
          (e.target as Element).setPointerCapture(e.pointerId);
          dragRef.current = { kind: "pan", startScreen: [e.clientX, e.clientY], startView: [state.schematicView.x, state.schematicView.y] };
          return;
        }
        if (e.button !== 0) return;
        const [wx, wy] = toWorld(e.clientX, e.clientY);

        if (state.activeTool === "wire") {
          const thresholdUm = 400 / state.schematicView.scale;
          const snapped = nearestSnapPoint(pinSnapPoints(sch), wx, wy, thresholdUm) ?? snapToGrid(wx, wy);
          const draw = state.drawState;
          if (draw?.kind !== "wire") {
            dispatch({ type: "SET_DRAW_STATE", draw: { kind: "wire", pts: [snapped] } });
            return;
          }
          const next = { ...draw, pts: [...draw.pts, snapped] as [number, number][] };
          // sch_screen.cpp IsTerminalPoint, simplified: landing back on a
          // pin auto-finishes the wire, same as a real click on a pin/
          // junction/other wire does in source -- this app only checks
          // the pin case (see this file's header comment for the rest).
          const onPin = pinSnapPoints(sch).some(([px, py]) => px === snapped[0] && py === snapped[1]);
          if (onPin && next.pts.length >= 2) {
            if (next.pts.length >= 2) api.cmd({ op: "add_wire", pts: next.pts.map(([x, y]) => ({ x, y })) });
            dispatch({ type: "SET_DRAW_STATE", draw: null });
          } else {
            dispatch({ type: "SET_DRAW_STATE", draw: next });
          }
          return;
        }

        // `M`-armed move: this click drops whatever is being dragged,
        // same two-step ("arm, then click to commit") flow
        // useActionRunner.ts's pcbnew.InteractiveMove.move already uses --
        // Escape (the ESCAPE reducer case, generic across tabs) cancels it
        // instead, never this handler.
        if (moveMode) {
          dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
          if (state.movePreview) api.commitMove(state.movePreview.refs, state.movePreview.dxUm, state.movePreview.dyUm, "symbol", state.movePreview.rotateQuarterTurns);
          return;
        }

        const ctrlOrCmd = isMac() ? e.metaKey : e.ctrlKey;
        const modifiers = computeClickModifiers(e.shiftKey, ctrlOrCmd, e.altKey);
        const symId = hitSymbol(sch, wx, wy);
        if (symId) {
          const refs = applySingleClickModifier(state.selection, symId, modifiers);
          dispatch({ type: "SET_SELECTION", refs });
          dispatch({ type: "SET_NET_HIGHLIGHT", net: null });
          return;
        }
        const thresholdUm = 400 / state.schematicView.scale;
        const wireId = hitWire(sch, wx, wy, thresholdUm);
        if (wireId && hasModifier(modifiers)) {
          // A modified click on a wire selects it (for Del) rather than
          // only ever toggling the net highlight -- plain click on a wire
          // keeps the existing net-highlight-toggle behavior below, since
          // that is this app's main "what net is this" tool and more
          // useful un-modified than a bare select would be.
          dispatch({ type: "SET_SELECTION", refs: applySingleClickModifier(state.selection, wireId, modifiers) });
          return;
        }
        const net = hitWireNet(sch, wx, wy, thresholdUm);
        if (net) {
          dispatch({ type: "SET_NET_HIGHLIGHT", net: state.netHighlight === net ? null : net });
          return;
        }

        // Nothing under the cursor: box select, or (no modifier) a plain
        // click that just clears whatever was selected -- same as
        // Canvas.tsx's own "nothing hit" branch.
        dragRef.current = { kind: "box", startWorld: [wx, wy], startScreen: [e.clientX, e.clientY] };
        if (!hasModifier(modifiers)) dispatch({ type: "CLEAR_SELECTION" });
      }}
      onPointerMove={(e) => {
        if (!sch) return;
        const [wx, wy] = toWorld(e.clientX, e.clientY);
        dispatch({ type: "SET_CURSOR", at: { x: wx, y: wy } });

        if (moveMode && state.selection.size > 0) {
          const origin = state.moveOriginUm ?? { x: wx, y: wy };
          const [ox, oy] = snapToGrid(origin.x, origin.y);
          const [sx, sy] = snapToGrid(wx, wy);
          const { rotateQuarterTurns } = state.movePreview ?? {};
          dispatch({ type: "SET_MOVE_PREVIEW", preview: { refs: [...state.selection], kind: "symbol", dxUm: sx - ox, dyUm: sy - oy, rotateQuarterTurns } });
          return;
        }

        const drag = dragRef.current;
        if (!drag) return;
        if (drag.kind === "pan") {
          userMovedRef.current = true;
          const dx = e.clientX - drag.startScreen[0];
          const dy = e.clientY - drag.startScreen[1];
          dispatch({ type: "SET_SCHEMATIC_VIEW", view: { ...state.schematicView, x: drag.startView[0] + dx, y: drag.startView[1] + dy } });
        } else if (drag.kind === "box") {
          const rect = containerRef.current!.getBoundingClientRect();
          const x0 = drag.startScreen[0] - rect.left,
            y0 = drag.startScreen[1] - rect.top;
          const x1 = e.clientX - rect.left,
            y1 = e.clientY - rect.top;
          setMarquee({ x0, y0, x1, y1, crossing: isCrossingSelection(x0, x1) });
        }
      }}
      onDoubleClick={() => {
        const draw = state.drawState;
        if (draw?.kind !== "wire") return;
        if (draw.pts.length >= 2) api.cmd({ op: "add_wire", pts: draw.pts.map(([x, y]) => ({ x, y })) });
        dispatch({ type: "SET_DRAW_STATE", draw: null });
      }}
      onPointerUp={(e) => {
        const drag = dragRef.current;
        dragRef.current = null;
        if (!drag) return;
        if (drag.kind === "box" && sch) {
          if (marquee) {
            const ctrlOrCmd = isMac() ? e.metaKey : e.ctrlKey;
            const modifiers = computeClickModifiers(e.shiftKey, ctrlOrCmd, e.altKey);
            const [x1, y1] = toWorld(e.clientX, e.clientY);
            const [x0, y0] = drag.startWorld;
            const box: [number, number, number, number] = [Math.min(x0, x1), Math.min(y0, y1), Math.max(x0, x1), Math.max(y0, y1)];
            const hits = collectBoxSelection(sch, box, marquee.crossing);
            if (hits.length > 0 || hasModifier(modifiers)) {
              dispatch({ type: "SET_SELECTION", refs: applyBoxSelectionModifiers(state.selection, hits, modifiers) });
            }
          }
          setMarquee(null);
        }
      }}
    >
      <canvas ref={canvasRef} />
      {empty && <div className="pcb-canvas-empty">{empty}</div>}
    </div>
  );
}
