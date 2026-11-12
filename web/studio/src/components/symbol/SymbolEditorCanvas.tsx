// The Symbol Editor's own canvas (eeschema's Symbol Editor). Reuses the
// PCB/Footprint Editor tabs' own ported view/selection/grid-snap logic
// directly (kicad-port/viewControls.ts, gridHelper.ts, selection.ts) --
// "the same canvas with different tools", same precedent
// `FootprintCanvas.tsx`'s own doc comment sets.
//
// Unit convention: every world-space number in this file is this app's
// shared internal space (µm, +y down) -- see `state/symbolEditorStore
// .tsx`'s own doc for why, and `symbolPainter.ts`'s `mmPointToNm`/
// `nmPointToMm` for the one seam back to the symbol's own wire format
// (mm, +y up) that every outgoing `Cmd` needs. Because that conversion
// already happens once, up front, this file's own `screenToWorld`/
// `worldToScreen` calls are completely ordinary -- no second Y-flip
// belongs here.
import { useGridDisplay } from "../../state/commonOptions";
import { gridYOf } from "../../kicad-port/gridSettings";
import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { LibraryFill, LibrarySymbolGraphic, LibrarySymbolPin } from "../../api/types";
import { drawTarget, useSymApi, useSymDispatch, useSymState, type SymToolId } from "../../state/symbolEditorStore";
import { boundsOfPoints, fitTransform, screenToWorld } from "../../kicad-port/view";
import { focusOnLocation } from "../../kicad-port/pinTable";
import { paintSymbol, mmPointToNm, nmPointToMm, toLibPin, IDENTITY, type SymShapeKind } from "./symbolPainter";
import { resolvePin } from "../schematic/transform";
import { useGridSettings } from "../../state/gridSettings";
import { useGridOverrides } from "../../state/gridOverrides";
import { gridSizeFor, selectionGrid, type GridCategory } from "../../kicad-port/gridOverrides";
import { heldCursor } from "../../kicad-port/heldCursor";
import { allFields } from "../../kicad-port/symFields";
import { fieldsOnShow, hitLibField, libFieldName } from "../../kicad-port/symFieldLayout";
import { measureStrokeText } from "../text/strokeFont";
import { SymbolSnap, type SymbolSnapMods } from "./symbolSnap";
import { paintSnapOverlay } from "../canvas/snapOverlayPainter";
import { layerColor } from "../canvas/layers";
import { snapGridNm } from "../canvas/gridHelper";
import { handleWheel, leftDragOutcome } from "../../kicad-port/viewControls";
import { wheelInputOf } from "../../hooks/canvasWheel";
import { useLibraryViewControls } from "../libraryViewControls";
import { SelectionMarquee, type Marquee } from "../SelectionMarquee";
import { rectSelectionHits } from "../../kicad-port/selectRect";
import { symbolItemBoxes } from "../../kicad-port/itemBoxes";
import { useStudioState } from "../../state/store";
import { useWheelPrefs } from "../../actions/useWheelPrefs";
import { useNonPassiveWheel } from "../../hooks/useNonPassiveWheel";
import { isMac } from "../../platform";
import { ClickDragGesture, dragRuleFor } from "../../kicad-port/dragThreshold";
import { computeClickModifiers, applySingleClickModifier, applyBoxSelectionModifiers, hasModifier, isCrossingSelection, type ClickModifiers } from "../../kicad-port/selection";
import { distToSegment } from "../canvas/itemHitTest";
import { symbolGraphicDistance } from "../../kicad-port/libEditorHit";
import { polyBegin, polyContinue, polyFinish } from "../../kicad-port/symPolyDraw";
import { newPinFromClick } from "../../kicad-port/symPinTemplate";
import { pinShown } from "../../kicad-port/symPinText";
import { ContextMenu, type MenuEntry } from "../canvas/ContextMenu";
import { useActionRunner } from "../../actions/useActionRunner";
import { stackedPinMenuState } from "../../kicad-port/stackedPins";
import { newTextBox } from "../../kicad-port/symTextBox";
import { fieldLooksOf, graphicStateOf } from "../../kicad-port/symLooks";
import { useSymFind } from "../../actions/symFindOps";
import { carriedItems } from "../../kicad-port/symCarry";
import { bezierFromClicks, bezierGraphic } from "../../kicad-port/symBezier";
import { addCorner, canAddCorner, eeFindHandle, handlePos, POINT_SIZE_PX, removableCorner, removeCorner, type EeHandle } from "../../kicad-port/eePointEdit";
import { pinsDraggedWithEdge, symArcEditMode, symDrag, symEditShape, symGraphicFrom, symPointEditor } from "../../kicad-port/symPointEdit";
import { paintingIn, useColorThemeStamp } from "../../state/colorThemeStore";
import "../../styles/canvas.css";

const SHAPE_TOOL_KIND: Partial<Record<SymToolId, SymShapeKind>> = {
  draw_segment: "segment",
  draw_lines: "lines",
  draw_arc: "arc",
  draw_rect: "rect",
  draw_circle: "circle",
  draw_polygon: "polygon",
  draw_bezier: "bezier",
  draw_textbox: "textbox",
};

/** Draw Lines and Draw Polygons are one tool in this KiCad (`SHAPE_T::POLY` for both): points are added by clicks and the shape ends on a double click or Enter. */
const isPolyKind = (k: SymShapeKind) => k === "lines" || k === "polygon";

function graphicPoints(g: LibrarySymbolGraphic): [number, number][] {
  const toNm = (p: { x: number; y: number }) => mmPointToNm(p);
  switch (g.kind) {
    case "rectangle":
      return [toNm(g.start), toNm(g.end)];
    case "circle":
      return [toNm(g.center)];
    case "arc":
      return [toNm(g.start), toNm(g.mid), toNm(g.end)];
    case "polyline":
      return g.pts.map(toNm);
    case "text":
      return [toNm(g.at)];
    case "text_box":
      return [toNm(g.start), toNm(g.end)];
    case "bezier":
      return [toNm(g.start), toNm(g.c1), toNm(g.c2), toNm(g.end)];
  }
}

/** `SCH_SHAPE::HitTest` and its kin: the distance from a world point to the graphic as drawn (a bezier's curve, a text box's inside), `kicad-port/libEditorHit.ts`. */
const graphicHitDistance = (g: LibrarySymbolGraphic, wx: number, wy: number): number => symbolGraphicDistance(g, wx, wy, measureStrokeText);

/** Distance from a world point to a pin's own body-to-tip line, resolved the exact same way the painter draws it (`resolvePin` with an identity transform). */
function pinHitDistance(pin: LibrarySymbolPin, wx: number, wy: number): number {
  const rp = resolvePin(toLibPin(pin), IDENTITY, [0, 0]);
  return distToSegment(wx, wy, rp.root[0], rp.root[1], rp.tip[0], rp.tip[1]);
}

type DragState =
  /** `SCH_SELECTION_TOOL::selectMultiple`: the selection rectangle from where the press was; `click` is the item the press was on, selected as a click when the release comes
   * without a drag ("Left button drag:" sent the drag to the rectangle). */
  | { kind: "box"; startWorld: [number, number]; startScreen: [number, number]; modifiers: ClickModifiers; click: string | null }
  | { kind: "move"; refs: string[]; moveKind: "pin" | "graphic" | "field"; startWorld: [number, number]; snapOrigin: [number, number] }
  /** The point editor's drag of one handle (`SCH_POINT_EDITOR::Main`): which shape and which point or side. */
  | { kind: "point"; id: string; handle: EeHandle };

function symbolGraphicFromDraw(kind: SymShapeKind, ptsNm: [number, number][], unit: number, bodyStyle: number, cursorNm: readonly [number, number] | null = null): LibrarySymbolGraphic | null {
  const p = (i: number) => nmPointToMm(ptsNm[i]![0], ptsNm[i]![1]);
  const base = { unit, body_style: bodyStyle, stroke_mm: 0.254, fill: "none" as LibraryFill };
  switch (kind) {
    case "segment":
      return ptsNm.length >= 2 ? { kind: "polyline", ...base, pts: [p(0), p(1)] } : null;
    case "rect":
      return ptsNm.length >= 2 ? { kind: "rectangle", ...base, start: p(0), end: p(1) } : null;
    case "circle":
      return ptsNm.length >= 2 ? { kind: "circle", ...base, center: p(0), radius_mm: Math.hypot(ptsNm[1]![0] - ptsNm[0]![0], ptsNm[1]![1] - ptsNm[0]![1]) / 1e6 } : null;
    case "arc":
      return ptsNm.length >= 3 ? { kind: "arc", ...base, start: p(0), mid: p(1), end: p(2) } : null;
    case "lines":
    case "polygon": {
      // `doDrawShape`: both tools build a `SHAPE_T::POLY` with `m_lastFillStyle`, which is `NO_FILL` -- an unfilled polyline, closed only by ending on its start.
      const done = polyFinish(ptsNm);
      return done ? { kind: "polyline", ...base, pts: done.pts.map(([x, y]) => nmPointToMm(x, y)) } : null;
    }
    case "bezier": {
      // `EndEdit` of the bezier's edit state: the four clicks, or fewer and the cursor (a double click or Enter, kicad-port/symBezier.ts)
      const b = bezierFromClicks(ptsNm, cursorNm ? [cursorNm[0], cursorNm[1]] : null);
      return b ? bezierGraphic(b, unit, bodyStyle, base.stroke_mm) : null;
    }
    case "textbox":
      return null; // a text box is made by its own dialog (`newTextBox`), after the second click
  }
}

// The poly tools have no fixed point count -- left out of this table (same
// convention `FootprintCanvas.tsx`'s own `AUTO_FINISH` uses), so
// placement only ever ends on an explicit Enter/double-click.
const AUTO_FINISH: Partial<Record<SymShapeKind, number>> = { segment: 2, rect: 2, circle: 2, arc: 3, bezier: 4, textbox: 2 };

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
  const gridSettingsList = useGridSettings("symbol");
  const gridList = gridSettingsList.grids;
  // the editor's own Display Options > Grid Display, and the Y of its grid (Edit Grids' "Y:")
  const gridDisplay = useGridDisplay("symbol");
  const gridYNm = gridYOf(gridSettingsList, state.gridNm);
  const gridOverrides = useGridOverrides("symbol");
  const gridOf = (c: GridCategory): number => gridSizeFor(c, state.gridNm, gridList, gridOverrides);
  /** `GetSelectionGrid`: the category of the coarsest grid of what is held (a pin is a connectable item, a graphic is text or a shape). */
  const heldCategory = (ids: readonly string[]): GridCategory => {
    const cats = ids.map((id): GridCategory => (api.pinById(id) ? "connectable" : libFieldName(id) || api.graphicById(id)?.kind === "text" ? "text" : "graphics"));
    return selectionGrid(cats, gridOf);
  };
  const { run } = useActionRunner();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);
  /** The pressed button's `BUTTON_STATE` (tool_dispatcher.cpp): whether the press has become a drag. Made at every press. */
  const gestureRef = useRef<ClickDragGesture | null>(null);
  /** A right-button press that became a pan-drag: the `contextmenu` event that follows its release must not open the menu (a drag is not a click). */
  /** The view as of now, for work that finishes after a round trip (the anchor tool re-centres the view once the symbol has moved). */
  const viewRef = useRef(state.view);
  viewRef.current = state.view;
  /** An anchor click whose verb has not come back yet: a double click must not shift the symbol twice. */
  const anchorBusyRef = useRef(false);
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 });
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  const wheelPrefs = useWheelPrefs();
  const userMovedRef = useRef(false);
  const [marquee, setMarquee] = useState<Marquee | null>(null);
  const studioPrefs = useStudioState().prefs;
  // The middle and right drags, the pan on movement with a key and the auto-pan (`WX_VIEW_CONTROLS`), as the board and the schematic have them.
  const viewCtl = useLibraryViewControls({
    containerRef,
    view: state.view,
    setView: (view) => dispatch({ type: "SET_VIEW", view }),
    settings: wheelPrefs.settings,
    zoomSpeed: studioPrefs.zoomSpeed,
    autoPan: studioPrefs.autoPan,
    autoPanAcceleration: studioPrefs.autoPanAcceleration,
    busy: () => dragRef.current?.kind === "box" || dragRef.current?.kind === "point" || state.movePreview != null || state.drawState != null || state.carry != null,
    shown: true,
    onUserMoved: () => {
      userMovedRef.current = true;
    },
  });

  const sym = state.symbol;
  /** The fields on show: the symbol's own list, those with a text, the hidden ones only while Show Hidden Fields is on (kicad-port/symFieldLayout.ts). */
  const symFind = useSymFind();
  const shownFields = useMemo(() => (sym ? fieldsOnShow(allFields(sym), state.showHiddenFields, state.activeUnit, sym.unit_count) : []), [sym, state.showHiddenFields, state.activeUnit]);

  // The grid helper of the tool in force (kicad-port/schGridHelper.ts, symbolSnap.ts), kept fed with the view, the grid, the overrides and the pins of the unit being edited: a click goes
  // to the grid, or -- with the grid off -- to the end of another pin, like `EE_GRID_HELPER::BestSnapAnchor`.
  const snapRef = useRef<SymbolSnap | null>(null);
  if (!snapRef.current) snapRef.current = new SymbolSnap();
  const snap = snapRef.current;
  const [snapTick, bumpSnap] = useState(0);
  snap.update({ symbol: sym, fields: shownFields, unit: state.activeUnit, bodyStyle: state.activeBodyStyle, showHiddenPins: state.showHiddenPins, scale: state.view.scale, gridNm: state.gridNm, grids: gridList, gridYs: gridSettingsList.ys, overrides: gridOverrides });
  useEffect(() => {
    snap.reset();
    bumpSnap((n) => n + 1);
  }, [snap, state.activeTool]);
  useEffect(() => () => snap.reset(), [snap]);
  const snapMods = (e: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }): SymbolSnapMods => ({ ctrl: isMac() ? e.metaKey : e.ctrlKey, shift: e.shiftKey });
  /** The point a click of the tool in force goes to. */
  const toolPoint = (e: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }, wx: number, wy: number): [number, number] => {
    const p = snap.point(state.activeTool, [wx, wy], snapMods(e), toolCategory(state.activeTool));
    return [p[0], p[1]];
  };
  /** The tools that place something, and anything carried: the grid helper sees every move, so the marker and the snap lines follow the cursor. */
  const placing = state.carry != null || state.activeTool === "pin" || state.activeTool === "text" || state.activeTool === "anchor" || state.activeTool.startsWith("draw_");
  /** The grid what is carried goes to: a pin's the connectable one, a text's the text one, a paste's the coarsest of what it holds (`GetItemGrid`, `GetSelectionGrid`). */
  const carryCategory = (): GridCategory => {
    const c = state.carry;
    if (!c) return toolCategory(state.activeTool);
    if (c.kind === "pin") return "connectable";
    if (c.kind === "text") return "text";
    const cats: GridCategory[] = [...c.pins.map((): GridCategory => "connectable"), ...c.graphics.map((g): GridCategory => (g.kind === "text" ? "text" : "graphics"))];
    return selectionGrid(cats, gridOf);
  };
  /** Where the carried items' anchor goes for the cursor at (`wx`, `wy`): `grid.Align` / `BestSnapAnchor` of the tool, as a click of it would. */
  const carryPoint = (e: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }, wx: number, wy: number): [number, number] => {
    const tool = state.carry?.kind === "pin" ? "pin" : state.carry?.kind === "text" ? "text" : "move";
    const p = snap.point(tool, [wx, wy], snapMods(e), carryCategory());
    return [p[0], p[1]];
  };
  const carried = useMemo(() => (state.carry ? { ...carriedItems(state.carry), looks: state.carry.looks } : null), [state.carry]);
  /** The point editor's drag in progress: the shape, the handle and where the snapped cursor has it. */
  const [pointDrag, setPointDrag] = useState<{ id: string; handle: EeHandle; to: [number, number] } | null>(null);
  /**
   * `SCH_POINT_EDITOR` on a selection of one shape it edits -- a rectangle, circle, arc, bezier, polyline or text box (kicad-port/symPointEdit.ts; UI-LEFT-2 YY1) --
   * with the select tool and nothing carried: its points and lines, and the shape as dragged. Not on a derived symbol (`IsSymbolAlias`: its graphics are its parent's).
   */
  const pointEditor = useMemo(() => {
    if (state.activeTool !== "select" || state.carry || state.selection.size !== 1 || !sym || state.derivedOwn) return null;
    const id = [...state.selection][0]!;
    const g = sym.graphics.find((x) => x.id === id);
    if (!g) return null;
    const ed = symPointEditor(g, pointDrag && pointDrag.id === id ? pointDrag : null, symArcEditMode(), measureStrokeText);
    return ed ? { id, graphic: g, edit: ed.edit, replace: ed.graphic } : null;
  }, [state.activeTool, state.carry, state.selection, sym, state.derivedOwn, pointDrag]);

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
      if (!state.libId) dispatch({ type: "SET_VIEW", view: fitTransform({ minX: -25_400_000, minY: -25_400_000, maxX: 25_400_000, maxY: 25_400_000 }, containerSize.width, containerSize.height) });
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
    const MIN_SPAN_NM = 25_400_000;
    const grow = (lo: number, hi: number): [number, number] => (hi - lo >= MIN_SPAN_NM ? [lo, hi] : [(lo + hi) / 2 - MIN_SPAN_NM / 2, (lo + hi) / 2 + MIN_SPAN_NM / 2]);
    const [minX, maxX] = grow(bounds.minX, bounds.maxX);
    const [minY, maxY] = grow(bounds.minY, bounds.maxY);
    dispatch({ type: "SET_VIEW", view: fitTransform({ minX, minY, maxX, maxY }, containerSize.width, containerSize.height) });
    dispatch({ type: "MARK_VIEW_INITIALIZED" });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sym != null, state.libId, state.fitRequest, containerSize, dispatch]);

  // `FocusOnItem`'s `FocusOnLocation`: the Pin Table's cross-probed pin brought into view when it is off it or near its edge
  useEffect(() => {
    if (!state.brightened || containerSize.width < 50) return;
    const next = focusOnLocation(state.view, state.brightened.atNm, containerSize.width, containerSize.height);
    if (next) dispatch({ type: "SET_VIEW", view: next });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.brightened]);

  useEffect(() => {
    userMovedRef.current = false;
  }, [state.libId]);

  // Render loop, in the Symbol Editor's colour theme (Preferences > Symbol Editor > Colors: its own, or the schematic editor's), redrawn when it changes.
  const themeStamp = useColorThemeStamp();
  useEffect(() => paintingIn("symbol", () => {
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
      // the Pin Table's cross-probed pin (`FocusOnItem`'s `SetBrightened`) and Find's matches (`UpdateFind`'s `BrightenItem`), in `LAYER_BRIGHTENED`
      brightened: new Set([...(state.brightened ? [state.brightened.id] : []), ...symFind.bright]),
      gridNm: state.gridNm,
      gridVisible: state.gridVisible,
      gridYNm,
      gridDisplay,
      activeUnit: state.activeUnit,
      activeBodyStyle: state.activeBodyStyle,
      drawState: state.drawState,
      // the rubber band follows the cursor the grid helper gave (a snapped one), not the pointer
      cursorNm: placing && snap.lastPoint() ? { x: snap.lastPoint()![0], y: snap.lastPoint()![1] } : state.cursorNm,
      movePreview: state.movePreview,
      showElectricalTypes: state.showElectricalTypes,
      showHiddenPins: state.showHiddenPins,
      showPinNumbers: state.showPinNumbers,
      showPinAltIcons: state.showPinAltIcons,
      carried,
      pointEditor,
      fields: shownFields,
      // a graphic's looks kept beside it (UI-LEFT-2 YY2)
      looksOf: (id) => (sym ? graphicStateOf(sym, id) : null),
      // a derived symbol's field looks are its own entry's
      fieldLooksOf: (name) => fieldLooksOf(state.derivedOwn ?? sym!, name),
    });
    // What the grid helper shows: the snap marker, the snap lines, the construction geometry.
    const snapOverlay = snap.overlay();
    if (snapOverlay) {
      const k = state.view.scale || 1;
      paintSnapOverlay(ctx, k, [-state.view.x / k, -state.view.y / k, (width - state.view.x) / k, (height - state.view.y) / k], snapOverlay, { marker: "#4aa3ff", construction: layerColor("anchor"), guideActive: layerColor("anchor"), guides: false });
    }
    ctx.restore();
    ctx.restore();
  }), [gridDisplay, gridYNm, sym, shownFields, state.view, state.selection, state.brightened, symFind.bright, state.gridNm, state.gridVisible, state.activeUnit, state.activeBodyStyle, state.drawState, state.cursorNm, state.movePreview, state.showElectricalTypes, state.showHiddenPins, state.showPinNumbers, state.showPinAltIcons, carried, pointEditor, containerSize, snapTick, themeStamp]);

  const worldAt = useCallback(
    (e: { clientX: number; clientY: number }): [number, number] => {
      const rect = containerRef.current!.getBoundingClientRect();
      return screenToWorld(state.view, e.clientX - rect.left, e.clientY - rect.top);
    },
    [state.view]
  );

  /** Topmost pin/graphic under a world point, pins first (connection points read above body art, same tier `FootprintCanvas.tsx`'s own pad-first hit test uses) -- restricted to whichever unit/body-style is currently being edited, same filter the painter itself applies. */
  const hitTest = useCallback(
    (wx: number, wy: number): { kind: "pin" | "graphic" | "field"; id: string } | null => {
      if (!sym) return null;
      const visible = (unit: number, bodyStyle: number) => (unit === 0 || unit === state.activeUnit) && (bodyStyle === 0 || bodyStyle === state.activeBodyStyle);
      const tol = Math.max(80_000, 6 / state.view.scale);
      for (let i = sym.pins.length - 1; i >= 0; i--) {
        const p = sym.pins[i]!;
        // A hidden pin that is not drawn cannot be picked (`IsShowingHiddenPins`).
        if (p.id && pinShown(p.hidden, state.showHiddenPins) && visible(p.unit, p.body_style) && pinHitDistance(p, wx, wy) <= tol) return { kind: "pin", id: p.id };
      }
      // A field that is on show: its box (`SCH_FIELD::HitTest`); a hidden one is there to pick only while Show Hidden Fields is on, as it is drawn only then.
      const field = hitLibField(shownFields, wx, wy, Math.max(40_000, 3 / state.view.scale), measureStrokeText);
      if (field) return { kind: "field", id: field };
      for (let i = sym.graphics.length - 1; i >= 0; i--) {
        const g = sym.graphics[i]!;
        if (g.id && visible(g.unit, g.body_style) && graphicHitDistance(g, wx, wy) <= tol) return { kind: "graphic", id: g.id };
      }
      return null;
    },
    [sym, shownFields, state.view.scale, state.activeUnit, state.activeBodyStyle, state.showHiddenPins]
  );

  const finishDraw = useCallback(() => {
    const draw = state.drawState;
    if (!draw) return;
    const target = drawTarget(state);
    const graphic = symbolGraphicFromDraw(draw.shapeKind, draw.pts, target.unit, target.bodyStyle, snap.lastPoint() ?? (state.cursorNm ? [state.cursorNm.x, state.cursorNm.y] : null));
    if (graphic) void api.addGraphic(graphic);
    dispatch({ type: "SET_DRAW_STATE", draw: null });
  }, [state.drawState, state.activeUnit, state.activeBodyStyle, state.cursorNm, api, dispatch]);

  /**
   * `SYMBOL_EDITOR_DRAWING_TOOLS::PlaceAnchor`: the click point becomes the symbol's origin (`symbol->Move( -cursorPos )`) and the view is
   * "refreshed without changing the viewport" -- re-centred by the same amount, so the symbol stays where it is on the screen.
   */
  const placeAnchor = useCallback(
    (sx: number, sy: number) => {
      if (!state.libId || anchorBusyRef.current) return;
      anchorBusyRef.current = true;
      void api
        .cmd({ op: "set_symbol_anchor", lib_id: state.libId, at: nmPointToMm(sx, sy) })
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
    const [wx, wy] = worldAt(e);
    setContextMenu(null);

    if (viewCtl.press(e)) return;
    if (e.button !== 0) return;

    if (state.carry) {
      // the click that places what is carried (`TwoClickPlace`'s second click, the move's drop): where it is drawn -- KiCad sets the item's position on motion
      // (`item->SetPosition( cursorPos )`, `moveItem` on `IsMotion()`), and the click places it as it is
      void api.placeCarry();
      return;
    }

    const [sx, sy] = placing ? toolPoint(e, wx, wy) : [wx, wy];

    if (state.activeTool === "pin" && sym) {
      // `SYMBOL_EDITOR_PIN_TOOL::CreatePin`: the click makes the pin (the sticky defaults at that place, no name and no number) and Pin
      // Properties asks for its properties before it is added -- OK adds it, Cancel drops it.
      dispatch({ type: "SET_PENDING_PIN", pin: newPinFromClick(state.pinTemplate, state.activeUnit, state.activeBodyStyle, nmPointToMm(sx, sy)) });
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
        if (shapeKind === "textbox") {
          // `doDrawShape`: the box the two clicks made is shown to `DIALOG_TEXT_PROPERTIES`, which adds it on OK and drops it on Cancel
          const target = drawTarget(state);
          dispatch({ type: "SET_TEXT_BOX_DIALOG", dialog: { id: null, box: newTextBox(pts[0]!, pts[1]!, target.unit, target.bodyStyle, state.lastTextBox) } });
          return;
        }
        const target = drawTarget(state);
        const graphic = symbolGraphicFromDraw(shapeKind, pts, target.unit, target.bodyStyle);
        if (graphic) void api.addGraphic(graphic);
        dispatch({ type: "SET_DRAW_STATE", draw: null });
      } else {
        dispatch({ type: "SET_DRAW_STATE", draw: { kind: "shape", shapeKind, pts } });
      }
      return;
    }

    if (state.activeTool === "text") {
      // `TwoClickPlace` (SCH_TEXT_T): the first click asks for the text (DIALOG_TEXT_PROPERTIES), which then follows the cursor; the second click places it (above).
      dispatch({ type: "SET_TEXT_DIALOG", dialog: { atNm: [sx, sy] } });
      return;
    }

    if (state.activeTool === "anchor") {
      placeAnchor(sx, sy);
      return;
    }

    // Select tool. A handle of the shape the point editor is on is picked first (`EDIT_POINTS::FindPoint`: a point, else a side's middle, `POINT_SIZE` either way):
    // its drag moves that point or side.
    if (pointEditor) {
      const handle = eeFindHandle(pointEditor.edit, [wx, wy], POINT_SIZE_PX / (state.view.scale || 1));
      if (handle) {
        const at = handlePos(pointEditor.edit, handle);
        dragRef.current = { kind: "point", id: pointEditor.id, handle };
        setPointDrag({ id: pointEditor.id, handle, to: [at[0], at[1]] });
        return;
      }
    }
    const hit = hitTest(wx, wy);
    const ctrlOrCmd = isMac() ? e.metaKey : e.ctrlKey;
    const modifiers = computeClickModifiers(e.shiftKey, ctrlOrCmd, e.altKey);
    // `SCH_SELECTION_TOOL::Main`'s `IsDrag( BUT_LEFT )` under "Left button drag:" (`leftDragOutcome`): a drag with a modifier, from empty space, or from an item the
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
        // `BestDragOrigin`: the point of what is picked up that the move holds it by -- the nearest end of a pin to the pointer (else the pointer itself).
        const [ox, oy] = snap.dragOrigin([wx, wy], held, heldCategory(held));
        dragRef.current = { kind: "move", refs: held, moveKind: hit.kind, startWorld: [wx, wy], snapOrigin: [ox, oy] };
        dispatch({ type: "SET_MOVE_ORIGIN", at: { x: ox, y: oy } });
      }
    }
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const [wx, wy] = worldAt(e);
    dispatch({ type: "SET_CURSOR", at: { x: wx, y: wy } });
    const motion = gestureRef.current?.move(e.clientX, e.clientY, e.timeStamp);
    if (viewCtl.move(e, !!motion?.dragging)) return;
    // The tools that place something: the grid helper sees every move, so the marker and the snap lines follow the cursor; what is carried follows it.
    if (state.carry) {
      const [cx, cy] = carryPoint(e, wx, wy);
      dispatch({ type: "SET_CARRY_DELTA", deltaNm: [cx - state.carry.originNm[0], cy - state.carry.originNm[1]] });
    } else if (placing) toolPoint(e, wx, wy);
    const drag = dragRef.current;
    if (!drag) return;
    if (drag.kind === "point") {
      // `BestSnapAnchor( cursor, graphics grid, the edited item )`
      if (!motion?.dragging) return;
      const p = snap.point("pointEditor", [wx, wy], snapMods(e), "graphics", new Set([drag.id]));
      setPointDrag({ id: drag.id, handle: drag.handle, to: [p[0], p[1]] });
      return;
    }
    if (drag.kind === "box") {
      if (!motion?.dragging) return; // a click with a little jitter, not a rectangle
      const rect = containerRef.current!.getBoundingClientRect();
      const x0 = drag.startScreen[0] - rect.left;
      const x1 = e.clientX - rect.left;
      setMarquee({ x0, y0: drag.startScreen[1] - rect.top, x1, y1: e.clientY - rect.top, crossing: isCrossingSelection(x0, x1) });
    } else if (drag.kind === "move") {
      // Nothing is picked up until the press has become a drag (tool_dispatcher.cpp: 8 px, or on macOS a motion after 300 ms held).
      if (!motion?.dragging) return;
      // `BestSnapAnchor( cursor, selection grid, selection )`, the cursor being the held point plus how far the pointer has travelled (kicad-port/heldCursor.ts).
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
      // `selectMultiple`: what the rectangle takes of the unit and body style on show, as its direction says
      const [wx, wy] = worldAt(e);
      const hits = sym ? rectSelectionHits(symbolItemBoxes(sym, state.activeUnit, state.activeBodyStyle), drag.startWorld, [wx, wy]) : [];
      if (hits.length > 0 || hasModifier(drag.modifiers)) dispatch({ type: "SET_SELECTION", refs: applyBoxSelectionModifiers(state.selection, hits, drag.modifiers) });
      else dispatch({ type: "CLEAR_SELECTION" });
      return;
    }
    if (drag.kind === "point") {
      // the drag ends: the shape as its points have it, and the pins a rectangle's side took along (`dragPinsOnEdge`), one undo step (`commit.Push( _( "Move Point" ) )`)
      const g = api.graphicById(drag.id);
      const to = pointDrag && pointDrag.id === drag.id ? pointDrag.to : null;
      setPointDrag(null);
      const moved = g && to ? symDrag(g, drag.handle, to, symArcEditMode(), measureStrokeText) : null;
      if (g && moved) {
        const edited = symGraphicFrom(moved.shape, g);
        const pins = sym ? pinsDraggedWithEdge(g, moved, sym.pins, (p) => resolvePin(toLibPin(p), IDENTITY, [0, 0]).root, studioPrefs.symDragPinsWithEdges) : [];
        if (JSON.stringify(edited) !== JSON.stringify(g) || pins.length > 0) void api.reshapeGraphic(drag.id, edited, pins);
      }
      return;
    }
    if (drag.kind === "move" && state.movePreview) {
      const { refs, kind, dxNm, dyNm } = state.movePreview;
      dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
      if (dxNm !== 0 || dyNm !== 0) {
        const dxMm = dxNm / 1e6;
        const dyMm = -dyNm / 1e6;
        if (kind === "pin") {
          // One undo step for the whole drop; in Synchronized Pins Mode the matching pins of the other units travel with them (`SYMBOL_EDITOR_MOVE_TOOL::Main`).
          const moves = refs.flatMap((id) => {
            const p = api.pinById(id);
            return p ? [{ id, x: p.at.x + dxMm, y: p.at.y + dyMm }] : [];
          });
          void api.movePins(moves);
        } else {
          for (const id of refs) if (!libFieldName(id)) void api.moveGraphic(id, dxMm, dyMm);
        }
        // the fields among what was held (`SCH_FIELD`s move with the rest): one verb for the lot
        const fieldNames = refs.flatMap((id) => {
          const n = libFieldName(id);
          return n ? [n] : [];
        });
        if (fieldNames.length > 0) void api.moveFields(fieldNames, dxMm, dyMm);
      }
    }
  };

  const onWheel = (e: WheelEvent) => {
    e.preventDefault();
    // `SCH_SELECTION_TOOL::Main`: Shift+Alt+wheel increments the primary part (`ACTIONS::INCREMENT{ ±1, 0 }`), Ctrl+Alt+wheel the secondary one -- up is +1
    const ctrl = isMac() ? e.metaKey : e.ctrlKey;
    if (state.activeTool === "select" && !state.carry && e.altKey && (e.shiftKey !== ctrl)) {
      void api.incrementSelection(e.deltaY < 0 || e.deltaX < 0 ? 1 : -1, e.shiftKey ? 0 : 1);
      return;
    }
    userMovedRef.current = true;
    const rect = containerRef.current!.getBoundingClientRect();
    const result = handleWheel(state.view, { width: rect.width, height: rect.height }, wheelInputOf(e, rect), wheelPrefs.settings, wheelPrefs.controller);
    if (result.kind !== "unhandled") dispatch({ type: "SET_VIEW", view: result.view });
  };
  useNonPassiveWheel(containerRef, onWheel); // React's onWheel is passive: preventDefault() would be ignored and logged

  /** `SYMBOL_EDITOR_EDIT_TOOL::Properties` on a graphic of the symbol: a text has Text Properties (`editTextProperties`), a text box Text Box Properties, a rectangle, circle, arc,
   * polygon or curve Shape Properties. */
  const openGraphicProperties = (id: string) => {
    const g = api.graphicById(id);
    if (!g) return;
    if (g.kind === "text") dispatch({ type: "SET_TEXT_DIALOG", dialog: { id } });
    else if (g.kind === "text_box") dispatch({ type: "SET_TEXT_BOX_DIALOG", dialog: { id, box: g } });
    else dispatch({ type: "SET_SHAPE_DIALOG_ID", id });
  };

  const onDoubleClick = (e: React.MouseEvent) => {
    if (state.drawState) {
      finishDraw();
      return;
    }
    if (state.activeTool !== "select") return; // only the select tool opens Properties on a double click
    const [wx, wy] = worldAt(e);
    const hit = hitTest(wx, wy);
    if (hit?.kind === "pin") dispatch({ type: "SET_PIN_PROPERTIES_ID", id: hit.id });
    // a field opens Symbol Properties on its row (`DIALOG_LIB_SYMBOL_PROPERTIES` with the field's row selected)
    else if (hit?.kind === "field") dispatch({ type: "SET_PROPERTIES_OPEN", open: true, field: libFieldName(hit.id) ?? undefined });
    else if (hit?.kind === "graphic") openGraphicProperties(hit.id);
  };

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
    // Delete, R / Shift+R and E are the registered `common.Interactive.delete`, `eeschema.InteractiveEdit.rotateCCW` / `rotateCW` and
    // `eeschema.InteractiveEdit.properties` (actions/editorFrameActions.ts), which the window-level hotkey runs. The canvas keeps Backspace, which the hotkey
    // table binds on a Mac only (`macHotkey`).
    if (e.key === "Backspace" && !/Mac/.test(navigator.platform) && state.selection.size > 0 && state.activeTool === "select") {
      void api.deleteItems([...state.selection]);
    }
  };

  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    // A right-drag pans and opens nothing: the button went up as a drag (TA_MOUSE_UP), not a click, so no menu.
    if (viewCtl.takeJustPanned()) return;
    const [wx, wy] = worldAt(e);
    const hit = hitTest(wx, wy);
    if (hit && !state.selection.has(hit.id)) dispatch({ type: "SET_SELECTION", refs: [hit.id] });
    const refs = hit ? (state.selection.has(hit.id) ? [...state.selection] : [hit.id]) : [...state.selection];
    const pinRefs = refs.filter((r) => api.pinById(r));
    const propertied = refs.filter((r) => !!api.graphicById(r));
    // `SYMBOL_EDITOR_EDIT_TOOL::Init`'s selection tool menu: the four transforms on anything selected (`SCH_CONDITIONS::NotEmpty`), then Properties on one item
    const entries: MenuEntry[] = [
      { label: "Rotate Counterclockwise (R)", onSelect: () => void api.rotateSelection(true), disabled: refs.length === 0 },
      { label: "Rotate Clockwise (Shift+R)", onSelect: () => void api.rotateSelection(false), disabled: refs.length === 0 },
      { label: "Mirror Vertically (Y)", onSelect: () => void api.mirrorSelection("v"), disabled: refs.length === 0 },
      { label: "Mirror Horizontally (X)", onSelect: () => void api.mirrorSelection("h"), disabled: refs.length === 0 },
      { label: "Pin Properties...", onSelect: () => dispatch({ type: "SET_PIN_PROPERTIES_ID", id: pinRefs[0]! }), disabled: pinRefs.length !== 1 },
      // `SCH_EDIT_TOOL::Properties` on the one graphic that is selected: Shape Properties or Text Box Properties
      ...(refs.length === 1 && propertied.length === 1 ? ([{ label: "Properties...", onSelect: () => openGraphicProperties(propertied[0]!) }] as MenuEntry[]) : []),
      // `SCH_POINT_EDITOR::Init`'s rows: Create Corner and Remove Corner on one polyline (`addCornerCondition`: the cursor on its outline; `removeCornerCondition`: the edited
      // point on a vertex), Cycle Arc Editing Mode on one arc (`arcIsEdited`)
      ...(() => {
        const g = refs.length === 1 ? api.graphicById(refs[0]!) : undefined;
        if (!g || state.derivedOwn) return [];
        if (g.kind === "arc") return [{ label: "Cycle Arc Editing Mode", onSelect: () => run("common.Interactive.cycleArcEditMode") }] as MenuEntry[];
        const shape = g.kind === "polyline" ? symEditShape(g, measureStrokeText) : null;
        if (shape?.kind !== "polygon") return [];
        const tol = POINT_SIZE_PX / (state.view.scale || 1);
        const grid = snapGridNm(state.gridNm) || 1;
        // `GetCursorPosition( !DisableGridSnapping() )`: the cursor on the grid (while the editor snaps to it)
        const at: [number, number] = [Math.round(wx / grid) * grid, Math.round(wy / grid) * grid];
        const ed = pointEditor && pointEditor.id === g.id ? pointEditor.edit : null;
        const handle = ed ? eeFindHandle(ed, [wx, wy], tol) : null;
        const edited = ed && handle?.kind === "point" ? handlePos(ed, handle) : null;
        const idx = removableCorner(shape, edited);
        const id = g.id!;
        return [
          ...(canAddCorner(shape, [wx, wy], tol) ? [{ label: "Create Corner", onSelect: () => void api.reshapeGraphic(id, symGraphicFrom(addCorner(shape, at), g), []) }] : []),
          ...(idx >= 0 ? [{ label: "Remove Corner", onSelect: () => { const next = removeCorner(shape, idx); if (next) void api.reshapeGraphic(id, symGraphicFrom(next, g), []); } }] : []),
        ] as MenuEntry[];
      })(),
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
      // `SYMBOL_EDITOR_EDIT_TOOL::Init`'s clipboard rows: Cut, Copy, Paste, Duplicate, then Delete -- each one undo step.
      { label: "Cut", onSelect: () => void api.copySelection(true), disabled: refs.length === 0 },
      { label: "Copy", onSelect: () => void api.copySelection(false), disabled: refs.length === 0 },
      { label: "Paste", onSelect: () => void api.pasteItems(), disabled: !state.copiedItems },
      { label: "Duplicate", onSelect: () => void api.duplicateSelection(), disabled: refs.length === 0 },
      { label: "Delete (Del)", onSelect: () => void api.deleteItems(refs), disabled: refs.length === 0 },
    ];
    setContextMenu({ x: e.clientX, y: e.clientY, entries });
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
      onKeyDown={onKeyDown}
      onContextMenu={onContextMenu}
    >
      <canvas ref={canvasRef} />
      <SelectionMarquee m={marquee} />
      {!sym && state.libId && <div className="pcb-canvas-empty">{state.error ?? "Loading symbol…"}</div>}
      {/* Nothing open: the empty canvas and its grid, like KiCad's, with one quiet line saying how to open something. */}
      {!sym && !state.libId && <div className="canvas-empty-hint">No symbol loaded -- double-click one in Libraries, or press Ctrl+N for a new one</div>}
      {contextMenu && <ContextMenu x={contextMenu.x} y={contextMenu.y} entries={contextMenu.entries} onClose={() => setContextMenu(null)} />}
    </div>
  );
}
