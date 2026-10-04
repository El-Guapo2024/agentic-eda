// The Symbol Editor's own canvas (eeschema's Symbol Editor). Reuses the
// PCB/Footprint Editor tabs' own ported view/selection/grid-snap logic
// directly (kicad-port/viewControls.ts, gridHelper.ts, selection.ts) --
// "the same canvas with different tools", same precedent
// `FootprintCanvas.tsx`'s own doc comment sets.
//
// Unit convention: every world-space number in this file is this app's
// shared internal space (µm, +y down) -- see `state/symbolEditorStore
// .tsx`'s own doc for why, and `symbolPainter.ts`'s `mmPointToUm`/
// `umPointToMm` for the one seam back to the symbol's own wire format
// (mm, +y up) that every outgoing `Cmd` needs. Because that conversion
// already happens once, up front, this file's own `screenToWorld`/
// `worldToScreen` calls are completely ordinary -- no second Y-flip
// belongs here.
import React, { useCallback, useEffect, useRef, useState } from "react";
import type { LibraryFill, LibrarySymbolGraphic, LibrarySymbolPin } from "../../api/types";
import { useSymApi, useSymDispatch, useSymState, type SymToolId } from "../../state/symbolEditorStore";
import { boundsOfPoints, fitTransform, screenToWorld } from "../../kicad-port/view";
import { paintSymbol, mmPointToUm, umPointToMm, toLibPin, IDENTITY } from "./symbolPainter";
import { resolvePin } from "../schematic/transform";
import { snapPoint } from "../canvas/gridHelper";
import { handleWheel, type WheelInput } from "../../kicad-port/viewControls";
import { useWheelPrefs } from "../../actions/useWheelPrefs";
import { isMac } from "../../platform";
import { computeClickModifiers, applySingleClickModifier, hasModifier } from "../../kicad-port/selection";
import { distToSegment } from "../canvas/itemHitTest";
import { nextPinNumber } from "../../kicad-port/pinNumbering";
import { orientationToAngleDeg } from "../../kicad-port/pinOrientation";
import { ContextMenu, type MenuEntry } from "../canvas/ContextMenu";
import "../../styles/canvas.css";

const SHAPE_TOOL_KIND: Partial<Record<SymToolId, "segment" | "arc" | "rect" | "circle" | "polygon">> = {
  draw_segment: "segment",
  draw_arc: "arc",
  draw_rect: "rect",
  draw_circle: "circle",
  draw_polygon: "polygon",
};

/** `SYMBOL_EDITOR_PIN_TOOL`'s own sticky defaults (`g_LastPinType`/`g_LastPinShape`/`g_LastPinOrient`, `SYMBOL_EDITOR_SETTINGS::m_Defaults`), confirmed directly from source: input/line/right (180deg, see pinOrientation.ts), 100 mil (2.54mm) length, 50 mil (1.27mm) name/number text. Reused here as this editor's own "last placed pin" memory the same way `FootprintCanvas.tsx`'s `DEFAULT_PAD_TEMPLATE` stands in for `m_Pad_Master`. */
const DEFAULT_PIN_TEMPLATE: Omit<LibrarySymbolPin, "id" | "number" | "at" | "unit" | "body_style"> = {
  name: "",
  electrical_type: "input",
  shape: "line",
  angle_deg: orientationToAngleDeg("right"),
  length_mm: 2.54,
  hidden: false,
  name_size_mm: 1.27,
  number_size_mm: 1.27,
};

function graphicPoints(g: LibrarySymbolGraphic): [number, number][] {
  const toUm = (p: { x: number; y: number }) => mmPointToUm(p);
  switch (g.kind) {
    case "rectangle":
      return [toUm(g.start), toUm(g.end)];
    case "circle":
      return [toUm(g.center)];
    case "arc":
      return [toUm(g.start), toUm(g.mid), toUm(g.end)];
    case "polyline":
      return g.pts.map(toUm);
    case "text":
      return [toUm(g.at)];
  }
}

function graphicHitDistance(g: LibrarySymbolGraphic, wx: number, wy: number): number {
  if (g.kind === "circle") {
    const [cx, cy] = mmPointToUm(g.center);
    const r = g.radius_mm * 1000;
    return Math.abs(Math.hypot(wx - cx, wy - cy) - r);
  }
  if (g.kind === "text") {
    const [tx, ty] = mmPointToUm(g.at);
    return Math.hypot(wx - tx, wy - ty);
  }
  if (g.kind === "rectangle") {
    const [x0, y0] = mmPointToUm(g.start);
    const [x1, y1] = mmPointToUm(g.end);
    const pts: [number, number][] = [
      [x0, y0],
      [x1, y0],
      [x1, y1],
      [x0, y1],
      [x0, y0],
    ];
    let best = Infinity;
    for (let i = 0; i + 1 < pts.length; i++) best = Math.min(best, distToSegment(wx, wy, pts[i]![0], pts[i]![1], pts[i + 1]![0], pts[i + 1]![1]));
    return best;
  }
  const pts = graphicPoints(g);
  let best = Infinity;
  for (let i = 0; i + 1 < pts.length; i++) best = Math.min(best, distToSegment(wx, wy, pts[i]![0], pts[i]![1], pts[i + 1]![0], pts[i + 1]![1]));
  if (g.kind === "polyline" && pts.length > 2) best = Math.min(best, distToSegment(wx, wy, pts[pts.length - 1]![0], pts[pts.length - 1]![1], pts[0]![0], pts[0]![1]));
  return best;
}

/** Distance from a world point to a pin's own body-to-tip line, resolved the exact same way the painter draws it (`resolvePin` with an identity transform). */
function pinHitDistance(pin: LibrarySymbolPin, wx: number, wy: number): number {
  const rp = resolvePin(toLibPin(pin), IDENTITY, [0, 0]);
  return distToSegment(wx, wy, rp.root[0], rp.root[1], rp.tip[0], rp.tip[1]);
}

type DragState = { kind: "pan"; button: 1 | 2; startScreen: [number, number]; startView: [number, number] } | { kind: "move"; refs: string[]; moveKind: "pin" | "graphic"; startWorld: [number, number] };

function symbolGraphicFromDraw(kind: "segment" | "arc" | "rect" | "circle" | "polygon", ptsUm: [number, number][], unit: number, bodyStyle: number): LibrarySymbolGraphic | null {
  const p = (i: number) => umPointToMm(ptsUm[i]![0], ptsUm[i]![1]);
  const base = { unit, body_style: bodyStyle, stroke_mm: 0.254, fill: "none" as LibraryFill };
  switch (kind) {
    case "segment":
      return ptsUm.length >= 2 ? { kind: "polyline", ...base, pts: [p(0), p(1)] } : null;
    case "rect":
      return ptsUm.length >= 2 ? { kind: "rectangle", ...base, start: p(0), end: p(1) } : null;
    case "circle":
      return ptsUm.length >= 2 ? { kind: "circle", ...base, center: p(0), radius_mm: Math.hypot(ptsUm[1]![0] - ptsUm[0]![0], ptsUm[1]![1] - ptsUm[0]![1]) / 1000 } : null;
    case "arc":
      return ptsUm.length >= 3 ? { kind: "arc", ...base, start: p(0), mid: p(1), end: p(2) } : null;
    case "polygon":
      return ptsUm.length >= 3 ? { kind: "polyline", ...base, pts: ptsUm.map((_, i) => p(i)), fill: "background" } : null;
  }
}

// `polygon` has no fixed point count -- left out of this table (same
// convention `FootprintCanvas.tsx`'s own `AUTO_FINISH` uses), so
// placement only ever ends on an explicit Enter/double-click.
const AUTO_FINISH: Partial<Record<"segment" | "rect" | "circle" | "arc" | "polygon", number>> = { segment: 2, rect: 2, circle: 2, arc: 3 };

export function SymbolEditorCanvas() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const pinTemplateRef = useRef<Omit<LibrarySymbolPin, "id" | "number" | "at" | "unit" | "body_style">>(DEFAULT_PIN_TEMPLATE);
  /** The number the Pin tool gave its latest pin, per open symbol -- see the pin-tool branch of onPointerDown. */
  const lastPlacedPinRef = useRef<{ libId: string | null; number: string } | null>(null);
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 });
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  const wheelPrefs = useWheelPrefs();
  const userMovedRef = useRef(false);

  const sym = state.symbol;

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

  // Fit to content once, same "until the user pans/zooms by hand" rule
  // `FootprintCanvas.tsx`'s own board-outline fit uses.
  useEffect(() => {
    if (!sym || userMovedRef.current || containerSize.width < 50 || containerSize.height < 50) return;
    const pts: [number, number][] = [];
    for (const p of sym.pins) {
      const rp = resolvePin(toLibPin(p), IDENTITY, [0, 0]);
      pts.push(rp.tip, rp.root);
    }
    for (const g of sym.graphics) pts.push(...graphicPoints(g));
    if (pts.length === 0) pts.push([-3000, -3000], [3000, 3000]);
    const bounds = boundsOfPoints(pts);
    if (!bounds) return;
    dispatch({ type: "SET_VIEW", view: fitTransform(bounds, containerSize.width, containerSize.height) });
    dispatch({ type: "MARK_VIEW_INITIALIZED" });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sym != null, containerSize, dispatch]);

  useEffect(() => {
    userMovedRef.current = false;
  }, [state.libId]);

  // Render loop.
  useEffect(() => {
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
    ctx.fillStyle = "#1a1a1a";
    ctx.fillRect(0, 0, width, height);
    ctx.save();
    ctx.translate(state.view.x, state.view.y);
    ctx.scale(state.view.scale || 1, state.view.scale || 1);
    paintSymbol(ctx, state.view, width, height, sym, {
      selection: state.selection,
      gridUm: state.gridUm,
      gridVisible: state.gridVisible,
      activeUnit: state.activeUnit,
      activeBodyStyle: state.activeBodyStyle,
      drawState: state.drawState,
      cursorUm: state.cursorUm,
      movePreview: state.movePreview,
    });
    ctx.restore();
    ctx.restore();
  }, [sym, state.view, state.selection, state.gridUm, state.gridVisible, state.activeUnit, state.activeBodyStyle, state.drawState, state.cursorUm, state.movePreview, containerSize]);

  const worldAt = useCallback(
    (e: { clientX: number; clientY: number }): [number, number] => {
      const rect = containerRef.current!.getBoundingClientRect();
      return screenToWorld(state.view, e.clientX - rect.left, e.clientY - rect.top);
    },
    [state.view]
  );

  /** Topmost pin/graphic under a world point, pins first (connection points read above body art, same tier `FootprintCanvas.tsx`'s own pad-first hit test uses) -- restricted to whichever unit/body-style is currently being edited, same filter the painter itself applies. */
  const hitTest = useCallback(
    (wx: number, wy: number): { kind: "pin" | "graphic"; id: string } | null => {
      if (!sym) return null;
      const visible = (unit: number, bodyStyle: number) => (unit === 0 || unit === state.activeUnit) && (bodyStyle === 0 || bodyStyle === state.activeBodyStyle);
      const tol = Math.max(80, 6 / state.view.scale);
      for (let i = sym.pins.length - 1; i >= 0; i--) {
        const p = sym.pins[i]!;
        if (p.id && visible(p.unit, p.body_style) && pinHitDistance(p, wx, wy) <= tol) return { kind: "pin", id: p.id };
      }
      for (let i = sym.graphics.length - 1; i >= 0; i--) {
        const g = sym.graphics[i]!;
        if (g.id && visible(g.unit, g.body_style) && graphicHitDistance(g, wx, wy) <= tol) return { kind: "graphic", id: g.id };
      }
      return null;
    },
    [sym, state.view.scale, state.activeUnit, state.activeBodyStyle]
  );

  const finishDraw = useCallback(() => {
    const draw = state.drawState;
    if (!draw) return;
    const graphic = symbolGraphicFromDraw(draw.shapeKind, draw.pts, state.activeUnit, state.activeBodyStyle);
    if (graphic) void api.addGraphic(graphic);
    dispatch({ type: "SET_DRAW_STATE", draw: null });
  }, [state.drawState, state.activeUnit, state.activeBodyStyle, api, dispatch]);

  const onPointerDown = (e: React.PointerEvent) => {
    (e.target as Element).setPointerCapture(e.pointerId);
    const [wx, wy] = worldAt(e);
    setContextMenu(null);

    if (e.button === 1 || e.button === 2) {
      dragRef.current = { kind: "pan", button: e.button as 1 | 2, startScreen: [e.clientX, e.clientY], startView: [state.view.x, state.view.y] };
      return;
    }
    if (e.button !== 0) return;

    const [sx, sy] = snapPoint(wx, wy, state.gridUm);

    if (state.activeTool === "pin" && sym) {
      // The pin just placed may not be in `sym.pins` yet (the document is re-polled after the round trip), so seed from it too:
      // a quick second click must get the NEXT number, as the C++ tool's `m_lastPin` carry does.
      const last = lastPlacedPinRef.current;
      const number = nextPinNumber(last && last.libId === state.libId ? [...sym.pins, { number: last.number }] : sym.pins);
      lastPlacedPinRef.current = { libId: state.libId, number };
      const pin: LibrarySymbolPin = { ...pinTemplateRef.current, number, unit: state.activeUnit, body_style: state.activeBodyStyle, at: umPointToMm(sx, sy) };
      void api.addPin(pin);
      return;
    }

    const shapeKind = SHAPE_TOOL_KIND[state.activeTool];
    if (shapeKind) {
      const already = state.drawState?.shapeKind === shapeKind ? state.drawState.pts : [];
      const pts: [number, number][] = [...already, [sx, sy]];
      if (pts.length >= (AUTO_FINISH[shapeKind] ?? Infinity)) {
        const graphic = symbolGraphicFromDraw(shapeKind, pts, state.activeUnit, state.activeBodyStyle);
        if (graphic) void api.addGraphic(graphic);
        dispatch({ type: "SET_DRAW_STATE", draw: null });
      } else {
        dispatch({ type: "SET_DRAW_STATE", draw: { kind: "shape", shapeKind, pts } });
      }
      return;
    }

    if (state.activeTool === "text") {
      void api.addGraphic({ kind: "text", unit: state.activeUnit, body_style: state.activeBodyStyle, text: "TEXT", at: umPointToMm(sx, sy), angle_deg: 0, size_mm: 1.27 });
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
      return;
    }

    // Select tool.
    const hit = hitTest(wx, wy);
    const ctrlOrCmd = isMac() ? e.metaKey : e.ctrlKey;
    const modifiers = computeClickModifiers(e.shiftKey, ctrlOrCmd, e.altKey);
    if (hit) {
      const refs = applySingleClickModifier(state.selection, hit.id, modifiers);
      dispatch({ type: "SET_SELECTION", refs });
      if (refs.includes(hit.id)) {
        dragRef.current = { kind: "move", refs: refs.length > 1 ? refs : [hit.id], moveKind: hit.kind, startWorld: [wx, wy] };
        dispatch({ type: "SET_MOVE_ORIGIN", at: { x: sx, y: sy } });
      }
    } else if (!hasModifier(modifiers)) {
      dispatch({ type: "CLEAR_SELECTION" });
    }
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const [wx, wy] = worldAt(e);
    dispatch({ type: "SET_CURSOR", at: { x: wx, y: wy } });
    const drag = dragRef.current;
    if (!drag) return;
    if (drag.kind === "pan") {
      userMovedRef.current = true;
      dispatch({ type: "SET_VIEW", view: { ...state.view, x: drag.startView[0] + (e.clientX - drag.startScreen[0]), y: drag.startView[1] + (e.clientY - drag.startScreen[1]) } });
    } else if (drag.kind === "move") {
      const [sx, sy] = snapPoint(wx, wy, state.gridUm);
      const origin = state.moveOriginUm ?? { x: sx, y: sy };
      dispatch({ type: "SET_MOVE_PREVIEW", preview: { refs: drag.refs, kind: drag.moveKind, dxUm: sx - origin.x, dyUm: sy - origin.y } });
    }
  };

  const onPointerUp = () => {
    const drag = dragRef.current;
    dragRef.current = null;
    if (!drag) return;
    if (drag.kind === "move" && state.movePreview) {
      const { refs, kind, dxUm, dyUm } = state.movePreview;
      dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
      if (dxUm !== 0 || dyUm !== 0) {
        const dxMm = dxUm / 1000;
        const dyMm = -dyUm / 1000;
        for (const id of refs) {
          if (kind === "pin") {
            const p = api.pinById(id);
            if (p) void api.movePin(id, p.at.x + dxMm, p.at.y + dyMm);
          } else {
            void api.moveGraphic(id, dxMm, dyMm);
          }
        }
      }
    }
  };

  const onWheel = (e: React.WheelEvent) => {
    e.preventDefault();
    userMovedRef.current = true;
    const rect = containerRef.current!.getBoundingClientRect();
    const input: WheelInput = { deltaX: e.deltaX, deltaY: e.deltaY, shiftKey: e.shiftKey, ctrlOrCmd: isMac() ? e.metaKey : e.ctrlKey, altKey: e.altKey, x: e.clientX - rect.left, y: e.clientY - rect.top };
    const result = handleWheel(state.view, { width: rect.width, height: rect.height }, input, wheelPrefs.settings, wheelPrefs.controller);
    if (result.kind !== "unhandled") dispatch({ type: "SET_VIEW", view: result.view });
  };

  const onDoubleClick = (e: React.MouseEvent) => {
    if (state.drawState) {
      finishDraw();
      return;
    }
    const [wx, wy] = worldAt(e);
    const hit = hitTest(wx, wy);
    if (hit?.kind === "pin") dispatch({ type: "SET_PIN_PROPERTIES_ID", id: hit.id });
  };

  const rotatePinSelection = useCallback(
    (ccw: boolean) => {
      for (const id of state.selection) {
        const p = api.pinById(id);
        if (!p) continue;
        const delta = ccw ? 90 : -90;
        const angle = ((p.angle_deg + delta) % 360 + 360) % 360;
        void api.editPin(id, { ...p, angle_deg: angle });
      }
    },
    [state.selection, api]
  );

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && state.drawState) {
      e.preventDefault();
      finishDraw();
      return;
    }
    if (e.key === "Escape") {
      dispatch({ type: "ESCAPE" });
      return;
    }
    if ((e.key === "Delete" || e.key === "Backspace") && state.selection.size > 0 && state.activeTool === "select") {
      for (const id of state.selection) {
        if (api.pinById(id)) void api.deletePin(id);
        else if (api.graphicById(id)) void api.deleteGraphic(id);
      }
      dispatch({ type: "CLEAR_SELECTION" });
      return;
    }
    if ((e.key === "r" || e.key === "R") && state.activeTool === "select" && state.selection.size > 0) {
      rotatePinSelection(!e.shiftKey);
      return;
    }
    if ((e.key === "e" || e.key === "E") && state.activeTool === "select" && state.selection.size === 1) {
      const [only] = state.selection;
      if (only && api.pinById(only)) dispatch({ type: "SET_PIN_PROPERTIES_ID", id: only });
    }
  };

  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    const [wx, wy] = worldAt(e);
    const hit = hitTest(wx, wy);
    if (hit && !state.selection.has(hit.id)) dispatch({ type: "SET_SELECTION", refs: [hit.id] });
    const refs = hit ? (state.selection.has(hit.id) ? [...state.selection] : [hit.id]) : [...state.selection];
    const pinRefs = refs.filter((r) => api.pinById(r));
    const entries: MenuEntry[] = [
      { label: "Rotate CCW (R)", onSelect: () => rotatePinSelection(true), disabled: pinRefs.length === 0 },
      { label: "Pin Properties...", onSelect: () => dispatch({ type: "SET_PIN_PROPERTIES_ID", id: pinRefs[0]! }), disabled: pinRefs.length !== 1 },
      { label: "Delete (Del)", onSelect: () => refs.forEach((id) => (api.pinById(id) ? void api.deletePin(id) : void api.deleteGraphic(id))), disabled: refs.length === 0 },
    ];
    setContextMenu({ x: e.clientX, y: e.clientY, entries });
  };

  // Remember the last-placed pin's own template, mirroring
  // `FootprintCanvas.tsx`'s own `m_Pad_Master` push-back comment.
  useEffect(() => {
    if (state.selection.size !== 1) return;
    const [only] = state.selection;
    const p = sym?.pins.find((pin) => pin.id === only);
    if (p) {
      pinTemplateRef.current = { name: p.name, electrical_type: p.electrical_type, shape: p.shape, angle_deg: p.angle_deg, length_mm: p.length_mm, hidden: p.hidden, name_size_mm: p.name_size_mm, number_size_mm: p.number_size_mm };
    }
  }, [state.selection, sym]);

  return (
    <div
      ref={containerRef}
      className="pcb-canvas-container"
      tabIndex={0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onDoubleClick={onDoubleClick}
      onKeyDown={onKeyDown}
      onWheel={onWheel}
      onContextMenu={onContextMenu}
    >
      <canvas ref={canvasRef} />
      {!sym && <div className="pcb-canvas-empty">{state.error ?? (state.libId ? "Loading symbol…" : "Open a symbol to begin")}</div>}
      {contextMenu && <ContextMenu x={contextMenu.x} y={contextMenu.y} entries={contextMenu.entries} onClose={() => setContextMenu(null)} />}
    </div>
  );
}
