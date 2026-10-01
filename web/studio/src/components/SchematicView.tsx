// The Schematic Editor tab. Was an <img> of GET /api/schematic.svg
// (one baked image); now a real Canvas2D KiCad-style renderer over GET
// /api/schematic's structured data (see schematic/painter.ts), so
// selection/zoom/pan/net-highlight work the same way the PCB canvas's do
// -- and so PCB <-> Schematic cross-probing (clicking U1 here highlights
// it there, and back) is just both views reading the same
// state.selection/state.netHighlight, not two separate selection models.
// Read-only for now: no schematic edit commands exist yet.
import { useEffect, useRef, useState } from "react";
import type { Schematic } from "../api/types";
import { useStudioDispatch, useStudioState } from "../state/store";
import { boundsOfPoints, fitTransform, zoomAbout } from "./canvas/view";
import { paintSchematic, symbolBounds } from "./schematic/painter";
import { GRID } from "./schematic/layout";
import { layerColor } from "./canvas/layers";
import { drawPageAndFrame, drawZoneReferences, drawTitleBlock, drawGridDots, PAGE_WIDTH_UM, PAGE_HEIGHT_UM } from "./schematic/drawingSheet";
import "../styles/canvas.css";

type DragState = { kind: "pan"; startScreen: [number, number]; startView: [number, number] };

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

function hitWireNet(sch: NonNullable<ReturnType<typeof useStudioState>["schematic"]>, xUm: number, yUm: number, thresholdUm: number): string | null {
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

export function SchematicView() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const userMovedRef = useRef(false);
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 });
  const sch = state.schematic;

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

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !sch || containerSize.width === 0) return;
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
    // Title/date/rev come from the real GET /api/schematic title_block
    // once a board has one; file name/sheet path have no such field (they
    // come from the project/sheet-hierarchy machinery, not the title
    // block's own content) and stay synthesized from the board name.
    const tb = sch.title_block;
    drawTitleBlock(ctx, state.schematicView, {
      title: tb?.title || state.board?.name || "untitled",
      date: tb?.date ?? new Date().toISOString().slice(0, 10),
      rev: tb?.rev ?? "",
      company: tb?.company,
      fileName: `${state.board?.name || "schematic"}.kicad_sch`,
      sheetPath: "/",
    });
    paintSchematic(ctx, state.schematicView, sch, { selection: state.selection, netHighlight: state.netHighlight });
    ctx.restore();
    ctx.restore();
  }, [sch, state.schematicView, state.selection, state.netHighlight, containerSize, state.board?.name]);

  // The container+canvas below must always render, loading/error or not:
  // an early return here would swap in a different DOM subtree with no
  // container div at all, and the ResizeObserver effect above (empty
  // deps, runs once on mount) would find `containerRef.current` null on
  // whichever render happened to be first and never attach to the real
  // container once data arrives -- containerSize would then stay {0,0}
  // forever. The loading/error text is an overlay instead.
  const empty = state.schematicError ?? (!sch ? "Loading schematic…" : null);

  const toWorld = (clientX: number, clientY: number): [number, number] => {
    const rect = containerRef.current!.getBoundingClientRect();
    const sx = clientX - rect.left,
      sy = clientY - rect.top;
    return [(sx - state.schematicView.x) / state.schematicView.scale, (sy - state.schematicView.y) / state.schematicView.scale];
  };

  return (
    <div
      ref={containerRef}
      className="pcb-canvas-container"
      style={{ cursor: dragRef.current ? "grabbing" : "default" }}
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
        const symId = hitSymbol(sch, wx, wy);
        if (symId) {
          dispatch({ type: "SET_SELECTION", refs: [symId] });
          dispatch({ type: "SET_NET_HIGHLIGHT", net: null });
          return;
        }
        const thresholdUm = 400 / state.schematicView.scale;
        const net = hitWireNet(sch, wx, wy, thresholdUm);
        if (net) {
          dispatch({ type: "SET_NET_HIGHLIGHT", net: state.netHighlight === net ? null : net });
        } else {
          dispatch({ type: "CLEAR_SELECTION" });
          dispatch({ type: "SET_NET_HIGHLIGHT", net: null });
        }
      }}
      onPointerMove={(e) => {
        const d = dragRef.current;
        if (!d) return;
        const dx = e.clientX - d.startScreen[0];
        const dy = e.clientY - d.startScreen[1];
        dispatch({ type: "SET_SCHEMATIC_VIEW", view: { ...state.schematicView, x: d.startView[0] + dx, y: d.startView[1] + dy } });
      }}
      onPointerUp={() => (dragRef.current = null)}
    >
      <canvas ref={canvasRef} />
      {empty && <div className="pcb-canvas-empty">{empty}</div>}
    </div>
  );
}
