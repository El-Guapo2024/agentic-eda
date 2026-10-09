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
import { paintSymbol, mmPointToUm, umPointToMm, toLibPin, IDENTITY, type SymShapeKind } from "./symbolPainter";
import { resolvePin } from "../schematic/transform";
import { snapPoint } from "../canvas/gridHelper";
import { useGridSettings } from "../../state/gridSettings";
import { useGridOverrides } from "../../state/gridOverrides";
import { gridSizeFor, selectionGrid, type GridCategory } from "../../kicad-port/gridOverrides";
import { handleWheel, type WheelInput } from "../../kicad-port/viewControls";
import { useWheelPrefs } from "../../actions/useWheelPrefs";
import { useNonPassiveWheel } from "../../hooks/useNonPassiveWheel";
import { isMac } from "../../platform";
import { ClickDragGesture, dragRuleFor } from "../../kicad-port/dragThreshold";
import { computeClickModifiers, applySingleClickModifier, hasModifier } from "../../kicad-port/selection";
import { distToSegment } from "../canvas/itemHitTest";
import { nextPinNumber } from "../../kicad-port/pinNumbering";
import { orientationToAngleDeg } from "../../kicad-port/pinOrientation";
import { polyBegin, polyContinue, polyFinish } from "../../kicad-port/symPolyDraw";
import { pinOccupying, synchronizePins } from "../../kicad-port/symPinSync";
import { pinShown } from "../../kicad-port/symPinText";
import { askConfirm } from "../library/libraryDialogs";
import { ContextMenu, type MenuEntry } from "../canvas/ContextMenu";
import { useActionRunner } from "../../actions/useActionRunner";
import { stackedPinMenuState } from "../../kicad-port/stackedPins";
import "../../styles/canvas.css";

const SHAPE_TOOL_KIND: Partial<Record<SymToolId, SymShapeKind>> = {
  draw_segment: "segment",
  draw_lines: "lines",
  draw_arc: "arc",
  draw_rect: "rect",
  draw_circle: "circle",
  draw_polygon: "polygon",
};

/** Draw Lines and Draw Polygons are one tool in this KiCad (`SHAPE_T::POLY` for both): points are added by clicks and the shape ends on a double click or Enter. */
const isPolyKind = (k: SymShapeKind) => k === "lines" || k === "polygon";

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

function symbolGraphicFromDraw(kind: SymShapeKind, ptsUm: [number, number][], unit: number, bodyStyle: number): LibrarySymbolGraphic | null {
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
    case "lines":
    case "polygon": {
      // `doDrawShape`: both tools build a `SHAPE_T::POLY` with `m_lastFillStyle`, which is `NO_FILL` -- an unfilled polyline, closed only by ending on its start.
      const done = polyFinish(ptsUm);
      return done ? { kind: "polyline", ...base, pts: done.pts.map(([x, y]) => umPointToMm(x, y)) } : null;
    }
  }
}

// The poly tools have no fixed point count -- left out of this table (same
// convention `FootprintCanvas.tsx`'s own `AUTO_FINISH` uses), so
// placement only ever ends on an explicit Enter/double-click.
const AUTO_FINISH: Partial<Record<SymShapeKind, number>> = { segment: 2, rect: 2, circle: 2, arc: 3 };

/** The grid category a symbol editor tool places on (`EE_GRID_HELPER::GetItemGrid`: a pin on the connectable grid, a shape on the graphics one, text on the text one; the anchor tool aligns to the graphics one). */
function toolCategory(tool: SymToolId): GridCategory {
  if (tool === "pin") return "connectable";
  if (tool === "text") return "text";
  if (tool === "anchor" || tool.startsWith("draw_")) return "graphics";
  return "current";
}

export function SymbolEditorCanvas() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  // Grid overrides (`common.Control.toggleGridOverrides`): each kind of item snaps to the grid its category is overridden to while they are on.
  const gridList = useGridSettings("symbol").grids;
  const gridOverrides = useGridOverrides("symbol");
  const gridOf = (c: GridCategory): number => gridSizeFor(c, state.gridUm, gridList, gridOverrides);
  const toolGrid = (): number => gridOf(toolCategory(state.activeTool));
  /** `GetSelectionGrid`: the coarsest of the grids of what is held (a pin is a connectable item, a graphic is text or a shape). */
  const heldGrid = (ids: readonly string[]): number => {
    const cats = ids.map((id): GridCategory => (api.pinById(id) ? "connectable" : api.graphicById(id)?.kind === "text" ? "text" : "graphics"));
    return gridOf(selectionGrid(cats, gridOf));
  };
  const { run } = useActionRunner();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);
  /** The pressed button's `BUTTON_STATE` (tool_dispatcher.cpp): whether the press has become a drag. Made at every press. */
  const gestureRef = useRef<ClickDragGesture | null>(null);
  /** A right-button press that became a pan-drag: the `contextmenu` event that follows its release must not open the menu (a drag is not a click). */
  const justPannedRef = useRef(false);
  const pinTemplateRef = useRef<Omit<LibrarySymbolPin, "id" | "number" | "at" | "unit" | "body_style">>(DEFAULT_PIN_TEMPLATE);
  /** The number the Pin tool gave its latest pin, per open symbol -- see the pin-tool branch of onPointerDown. */
  const lastPlacedPinRef = useRef<{ libId: string | null; number: string } | null>(null);
  /** The view as of now, for work that finishes after a round trip (the anchor tool re-centres the view once the symbol has moved). */
  const viewRef = useRef(state.view);
  viewRef.current = state.view;
  /** An anchor click whose verb has not come back yet: a double click must not shift the symbol twice. */
  const anchorBusyRef = useRef(false);
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
  const fitRequestRef = useRef(state.fitRequest);
  useEffect(() => {
    // Zoom to Fit (`common.Control.zoomFitScreen`): fit again even though the view was moved by hand.
    if (fitRequestRef.current !== state.fitRequest) {
      fitRequestRef.current = state.fitRequest;
      userMovedRef.current = false;
    }
    if (userMovedRef.current || containerSize.width < 50 || containerSize.height < 50) return;
    if (!sym) {
      // Nothing open: KiCad shows an empty canvas with its grid around the origin. An inch across (the symbol editor's own minimum fit span below).
      if (!state.libId) dispatch({ type: "SET_VIEW", view: fitTransform({ minX: -25400, minY: -25400, maxX: 25400, maxY: 25400 }, containerSize.width, containerSize.height) });
      return;
    }
    const pts: [number, number][] = [];
    for (const p of sym.pins) {
      const rp = resolvePin(toLibPin(p), IDENTITY, [0, 0]);
      pts.push(rp.tip, rp.root);
    }
    for (const g of sym.graphics) pts.push(...graphicPoints(g));
    if (pts.length === 0) pts.push([-3000, -3000], [3000, 3000]);
    const bounds = boundsOfPoints(pts);
    if (!bounds) return;
    // A symbol a few millimetres across would be blown up to fill the canvas (a pin number as tall as the window): show at least 25.4 mm (an inch).
    const MIN_SPAN_UM = 25400;
    const grow = (lo: number, hi: number): [number, number] => (hi - lo >= MIN_SPAN_UM ? [lo, hi] : [(lo + hi) / 2 - MIN_SPAN_UM / 2, (lo + hi) / 2 + MIN_SPAN_UM / 2]);
    const [minX, maxX] = grow(bounds.minX, bounds.maxX);
    const [minY, maxY] = grow(bounds.minY, bounds.maxY);
    dispatch({ type: "SET_VIEW", view: fitTransform({ minX, minY, maxX, maxY }, containerSize.width, containerSize.height) });
    dispatch({ type: "MARK_VIEW_INITIALIZED" });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sym != null, state.libId, state.fitRequest, containerSize, dispatch]);

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
      showElectricalTypes: state.showElectricalTypes,
      showHiddenPins: state.showHiddenPins,
      showPinNumbers: state.showPinNumbers,
      pendingText: state.pendingText,
    });
    ctx.restore();
    ctx.restore();
  }, [sym, state.view, state.selection, state.gridUm, state.gridVisible, state.activeUnit, state.activeBodyStyle, state.drawState, state.cursorUm, state.movePreview, state.showElectricalTypes, state.showHiddenPins, state.showPinNumbers, state.pendingText, containerSize]);

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
        // A hidden pin that is not drawn cannot be picked (`IsShowingHiddenPins`).
        if (p.id && pinShown(p.hidden, state.showHiddenPins) && visible(p.unit, p.body_style) && pinHitDistance(p, wx, wy) <= tol) return { kind: "pin", id: p.id };
      }
      for (let i = sym.graphics.length - 1; i >= 0; i--) {
        const g = sym.graphics[i]!;
        if (g.id && visible(g.unit, g.body_style) && graphicHitDistance(g, wx, wy) <= tol) return { kind: "graphic", id: g.id };
      }
      return null;
    },
    [sym, state.view.scale, state.activeUnit, state.activeBodyStyle, state.showHiddenPins]
  );

  const finishDraw = useCallback(() => {
    const draw = state.drawState;
    if (!draw) return;
    const graphic = symbolGraphicFromDraw(draw.shapeKind, draw.pts, state.activeUnit, state.activeBodyStyle);
    if (graphic) void api.addGraphic(graphic);
    dispatch({ type: "SET_DRAW_STATE", draw: null });
  }, [state.drawState, state.activeUnit, state.activeBodyStyle, api, dispatch]);

  /**
   * `PlacePin`: with Synchronized Pins Mode on, a pin dropped on top of another one (any unit, a body style that matches) asks first --
   * "This position is already occupied by another pin, in unit %d." -- and only a "Place Pin Anyway" goes on.
   */
  const confirmPinPlacement = useCallback(
    async (pin: LibrarySymbolPin): Promise<boolean> => {
      if (!sym || !synchronizePins(state.syncPins, sym.unit_count)) return true;
      const occupant = pinOccupying(pin, sym.pins);
      if (!occupant) return true;
      return askConfirm({
        title: "Confirmation",
        message: `This position is already occupied by another pin, in unit ${occupant.unit}.\n\nDisable the 'Synchronized Pins Mode' option to avoid this message.`,
        okLabel: "Place Pin Anyway",
      });
    },
    [sym, state.syncPins]
  );

  /**
   * `SYMBOL_EDITOR_DRAWING_TOOLS::PlaceAnchor`: the click point becomes the symbol's origin (`symbol->Move( -cursorPos )`) and the view is
   * "refreshed without changing the viewport" -- re-centred by the same amount, so the symbol stays where it is on the screen.
   */
  const placeAnchor = useCallback(
    (sx: number, sy: number) => {
      if (!state.libId || anchorBusyRef.current) return;
      anchorBusyRef.current = true;
      void api
        .cmd({ op: "set_symbol_anchor", lib_id: state.libId, at: umPointToMm(sx, sy) })
        .then((ok) => {
          if (!ok) return;
          const v = viewRef.current;
          dispatch({ type: "SET_VIEW", view: { ...v, x: v.x + sx * v.scale, y: v.y + sy * v.scale } });
        })
        .finally(() => {
          anchorBusyRef.current = false;
        });
    },
    [state.libId, api, dispatch]
  );

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
    justPannedRef.current = false;
    const [wx, wy] = worldAt(e);
    setContextMenu(null);

    if (e.button === 1 || e.button === 2) {
      dragRef.current = { kind: "pan", button: e.button as 1 | 2, startScreen: [e.clientX, e.clientY], startView: [state.view.x, state.view.y] };
      return;
    }
    if (e.button !== 0) return;

    const [sx, sy] = snapPoint(wx, wy, toolGrid());

    if (state.activeTool === "pin" && sym) {
      // The pin just placed may not be in `sym.pins` yet (the document is re-polled after the round trip), so seed from it too:
      // a quick second click must get the NEXT number, as the C++ tool's `m_lastPin` carry does.
      const last = lastPlacedPinRef.current;
      const number = nextPinNumber(last && last.libId === state.libId ? [...sym.pins, { number: last.number }] : sym.pins);
      const pin: LibrarySymbolPin = { ...pinTemplateRef.current, number, unit: state.activeUnit, body_style: state.activeBodyStyle, at: umPointToMm(sx, sy) };
      void confirmPinPlacement(pin).then((go) => {
        if (!go) return;
        lastPlacedPinRef.current = { libId: state.libId, number };
        void api.addPin(pin);
      });
      return;
    }

    const shapeKind = SHAPE_TOOL_KIND[state.activeTool];
    if (shapeKind) {
      const same = state.drawState?.shapeKind === shapeKind;
      if (isPolyKind(shapeKind)) {
        // `doDrawShape`: the first click begins the outline, each later one continues it (`EDA_SHAPE::continueEdit`); a double click or Enter ends it.
        const pts = same ? polyContinue(state.drawState!.pts, [sx, sy]) : polyBegin([sx, sy]);
        dispatch({ type: "SET_DRAW_STATE", draw: { kind: "shape", shapeKind, pts } });
        return;
      }
      const already = same ? state.drawState!.pts : [];
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
      // `TwoClickPlace` (SCH_TEXT_T): the first click asks for the text (DIALOG_TEXT_PROPERTIES), which then follows the cursor; the second click places it.
      if (state.pendingText) {
        void api.addGraphic({ kind: "text", unit: state.activeUnit, body_style: state.activeBodyStyle, text: state.pendingText.text, at: umPointToMm(sx, sy), angle_deg: state.pendingText.angleDeg, size_mm: state.pendingText.sizeMm });
        dispatch({ type: "SET_PENDING_TEXT", pending: null });
      } else {
        dispatch({ type: "SET_TEXT_DIALOG", at: [sx, sy] });
      }
      return;
    }

    if (state.activeTool === "anchor") {
      placeAnchor(sx, sy);
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
        const held = refs.length > 1 ? refs : [hit.id];
        dragRef.current = { kind: "move", refs: held, moveKind: hit.kind, startWorld: [wx, wy] };
        // `GetSelectionGrid`: what is picked up snaps on its own grid.
        const [ox, oy] = snapPoint(wx, wy, heldGrid(held));
        dispatch({ type: "SET_MOVE_ORIGIN", at: { x: ox, y: oy } });
      }
    } else if (!hasModifier(modifiers)) {
      dispatch({ type: "CLEAR_SELECTION" });
    }
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const [wx, wy] = worldAt(e);
    dispatch({ type: "SET_CURSOR", at: { x: wx, y: wy } });
    const motion = gestureRef.current?.move(e.clientX, e.clientY, e.timeStamp);
    const drag = dragRef.current;
    if (!drag) return;
    if (drag.kind === "pan") {
      userMovedRef.current = true;
      if (motion?.dragging) justPannedRef.current = drag.button === 2;
      dispatch({ type: "SET_VIEW", view: { ...state.view, x: drag.startView[0] + (e.clientX - drag.startScreen[0]), y: drag.startView[1] + (e.clientY - drag.startScreen[1]) } });
    } else if (drag.kind === "move") {
      // Nothing is picked up until the press has become a drag (tool_dispatcher.cpp: 8 px, or on macOS a motion after 300 ms held).
      if (!motion?.dragging) return;
      const [sx, sy] = snapPoint(wx, wy, heldGrid(drag.refs));
      const origin = state.moveOriginUm ?? { x: sx, y: sy };
      dispatch({ type: "SET_MOVE_PREVIEW", preview: { refs: drag.refs, kind: drag.moveKind, dxUm: sx - origin.x, dyUm: sy - origin.y } });
    }
  };

  const onPointerUp = () => {
    gestureRef.current?.up();
    const drag = dragRef.current;
    dragRef.current = null;
    if (!drag) return;
    if (drag.kind === "move" && state.movePreview) {
      const { refs, kind, dxUm, dyUm } = state.movePreview;
      dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
      if (dxUm !== 0 || dyUm !== 0) {
        const dxMm = dxUm / 1000;
        const dyMm = -dyUm / 1000;
        if (kind === "pin") {
          // One undo step for the whole drop; in Synchronized Pins Mode the matching pins of the other units travel with them (`SYMBOL_EDITOR_MOVE_TOOL::Main`).
          const moves = refs.flatMap((id) => {
            const p = api.pinById(id);
            return p ? [{ id, x: p.at.x + dxMm, y: p.at.y + dyMm }] : [];
          });
          void api.movePins(moves);
        } else {
          for (const id of refs) void api.moveGraphic(id, dxMm, dyMm);
        }
      }
    }
  };

  const onWheel = (e: WheelEvent) => {
    e.preventDefault();
    userMovedRef.current = true;
    const rect = containerRef.current!.getBoundingClientRect();
    const input: WheelInput = { deltaX: e.deltaX, deltaY: e.deltaY, shiftKey: e.shiftKey, ctrlOrCmd: isMac() ? e.metaKey : e.ctrlKey, altKey: e.altKey, x: e.clientX - rect.left, y: e.clientY - rect.top };
    const result = handleWheel(state.view, { width: rect.width, height: rect.height }, input, wheelPrefs.settings, wheelPrefs.controller);
    if (result.kind !== "unhandled") dispatch({ type: "SET_VIEW", view: result.view });
  };
  useNonPassiveWheel(containerRef, onWheel); // React's onWheel is passive: preventDefault() would be ignored and logged

  const onDoubleClick = (e: React.MouseEvent) => {
    if (state.drawState) {
      finishDraw();
      return;
    }
    if (state.activeTool !== "select") return; // only the select tool opens Properties on a double click
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
    // A right-drag pans and opens nothing: the button went up as a drag (TA_MOUSE_UP), not a click, so no menu.
    if (justPannedRef.current) {
      justPannedRef.current = false;
      return;
    }
    const [wx, wy] = worldAt(e);
    const hit = hitTest(wx, wy);
    if (hit && !state.selection.has(hit.id)) dispatch({ type: "SET_SELECTION", refs: [hit.id] });
    const refs = hit ? (state.selection.has(hit.id) ? [...state.selection] : [hit.id]) : [...state.selection];
    const pinRefs = refs.filter((r) => api.pinById(r));
    const entries: MenuEntry[] = [
      { label: "Rotate CCW (R)", onSelect: () => rotatePinSelection(true), disabled: pinRefs.length === 0 },
      { label: "Pin Properties...", onSelect: () => dispatch({ type: "SET_PIN_PROPERTIES_ID", id: pinRefs[0]! }), disabled: pinRefs.length !== 1 },
      // `SYMBOL_EDITOR_PIN_TOOL::Init`: with a single pin selected (`singlePinCondition`) the menu offers to push its length / sizes to every other pin.
      ...(refs.length === 1 && pinRefs.length === 1
        ? ([
            { label: "Push Pin Length", onSelect: () => void api.pushPinProperty(pinRefs[0]!, "length") },
            { label: "Push Pin Name Size", onSelect: () => void api.pushPinProperty(pinRefs[0]!, "name_size") },
            { label: "Push Pin Number Size", onSelect: () => void api.pushPinProperty(pinRefs[0]!, "number_size") },
          ] as MenuEntry[])
        : []),
      // `SYMBOL_EDITOR_EDIT_TOOL::Init`: Convert Stacked Pins / Explode Stacked Pin appear only when their conditions hold (kicad-port/stackedPins.ts).
      ...(() => {
        const stack = stackedPinMenuState(sym?.pins ?? [], refs);
        return [
          ...(stack.canConvert ? [{ label: "Convert Stacked Pins", onSelect: () => run("eeschema.InteractiveEdit.convertStackedPins") }] : []),
          ...(stack.canExplode ? [{ label: "Explode Stacked Pin", onSelect: () => run("eeschema.InteractiveEdit.explodeStackedPin") }] : []),
        ] as MenuEntry[];
      })(),
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
      onContextMenu={onContextMenu}
    >
      <canvas ref={canvasRef} />
      {!sym && state.libId && <div className="pcb-canvas-empty">{state.error ?? "Loading symbol…"}</div>}
      {/* Nothing open: the empty canvas and its grid, like KiCad's, with one quiet line saying how to open something. */}
      {!sym && !state.libId && <div className="canvas-empty-hint">No symbol loaded -- double-click one in Libraries, or press Ctrl+N for a new one</div>}
      {contextMenu && <ContextMenu x={contextMenu.x} y={contextMenu.y} entries={contextMenu.entries} onClose={() => setContextMenu(null)} />}
    </div>
  );
}
