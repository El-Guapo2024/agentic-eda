// The Footprint Editor's own canvas (GAPS.md #8) -- pcbnew's footprint
// editor frame, one footprint drawn in isolation at its own anchor.
// Reuses the PCB tab's ported view/selection/grid-snap logic directly
// (kicad-port/viewControls.ts, gridHelper.ts, selection.ts) -- it is "the
// same canvas with different tools" exactly as the task describes, just a
// much smaller tool set than Canvas.tsx's (no routing/zones/vias, no
// box-select disambiguation menu, no auto-pan) since a footprint has no
// board-scale content to need them for.
import React, { useCallback, useEffect, useRef, useState } from "react";
import type { CmdShape, LibraryPad, PadKind } from "../../api/types";
import { useFpApi, useFpDispatch, useFpState, type FpToolId } from "../../state/footprintEditorStore";
import { boundsOfPoints, fitTransform, screenToWorld } from "../../kicad-port/view";
import { paintFootprint } from "./footprintPainter";
import { snapPoint } from "../canvas/gridHelper";
import { handleWheel, type WheelInput } from "../../kicad-port/viewControls";
import { useWheelPrefs } from "../../actions/useWheelPrefs";
import { useNonPassiveWheel } from "../../hooks/useNonPassiveWheel";
import { useActionRunner } from "../../actions/useActionRunner";
import { isMac } from "../../platform";
import { computeClickModifiers, applySingleClickModifier, hasModifier } from "../../kicad-port/selection";
import { distToSegment } from "../canvas/itemHitTest";
import { nextPadNumber } from "../../kicad-port/padNumbering";
import { padCanHaveNumber } from "../../kicad-port/fpEditActions";
import { newPadFromMaster, settingsOf } from "../../kicad-port/padSettings";
import { enumerateHit, enumerateShownNumber, padsUnderSweep, sweepPoints } from "../../kicad-port/padEnumeration";
import { bezierPolyline } from "../../kicad-port/bezierPoly";
import { arcClick, arcMotion, arcRemoveLastPoint } from "../../kicad-port/arcGeom";
import { bezierClick, bezierFinishDouble, bezierMotion, bezierRemoveLastPoint } from "../../kicad-port/bezierGeom";
import { arcAngleSnap, arcClickPoints, bezierShape, ptXY } from "../canvas/curveTools";

/** The footprint editor's fixed graphic line width (`footprintToShapeArg`'s own 150 um). */
const DRAW_STROKE_WIDTH_UM = 150;
import { ContextMenu, type MenuEntry } from "../canvas/ContextMenu";
import { useFootprintGridOrigin } from "../../state/gridOrigin";
import "../../styles/canvas.css";

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

function padLocalPoint(pad: LibraryPad, wx: number, wy: number): { x: number; y: number } {
  const dx = wx - pad.at.x;
  const dy = wy - pad.at.y;
  const rad = -(pad.rot / 1000) * (Math.PI / 180);
  const cos = Math.cos(rad);
  const sin = Math.sin(rad);
  return { x: dx * cos - dy * sin, y: dx * sin + dy * cos };
}

/** Approximate pad hit test: an ellipse for circle/oval, an axis-aligned (pad-local) box otherwise -- good enough to click a visibly-under-the-cursor pad; not pixel-exact for a chamfered/trapezoid corner (documented simplification, see PARITY-fpedit.md). */
function padContains(pad: LibraryPad, wx: number, wy: number): boolean {
  const local = padLocalPoint(pad, wx, wy);
  const x = local.x - pad.offset.x;
  const y = local.y - pad.offset.y;
  const [w, h] = pad.size;
  const hw = w / 2,
    hh = h / 2;
  if (pad.shape === "circle" || pad.shape === "oval") {
    return (x / hw) ** 2 + (y / hh) ** 2 <= 1;
  }
  return Math.abs(x) <= hw && Math.abs(y) <= hh;
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

function graphicHitDistance(s: CmdShape, wx: number, wy: number): number {
  if (s.kind === "bezier") {
    const poly = shapePoints(s);
    let best = Infinity;
    for (let i = 0; i + 1 < poly.length; i++) best = Math.min(best, distToSegment(wx, wy, poly[i]![0], poly[i]![1], poly[i + 1]![0], poly[i + 1]![1]));
    return best;
  }
  if (s.kind === "circle") {
    const r = Math.hypot(s.end.x - s.center.x, s.end.y - s.center.y);
    return Math.abs(Math.hypot(wx - s.center.x, wy - s.center.y) - r);
  }
  if (s.kind === "rect") {
    const pts: [number, number][] = [
      [s.start.x, s.start.y],
      [s.end.x, s.start.y],
      [s.end.x, s.end.y],
      [s.start.x, s.end.y],
      [s.start.x, s.start.y],
    ];
    let best = Infinity;
    for (let i = 0; i + 1 < pts.length; i++) best = Math.min(best, distToSegment(wx, wy, pts[i]![0], pts[i]![1], pts[i + 1]![0], pts[i + 1]![1]));
    return best;
  }
  const pts = shapePoints(s);
  let best = Infinity;
  for (let i = 0; i + 1 < pts.length; i++) best = Math.min(best, distToSegment(wx, wy, pts[i]![0], pts[i]![1], pts[i + 1]![0], pts[i + 1]![1]));
  if (s.kind === "polygon" && pts.length > 2) best = Math.min(best, distToSegment(wx, wy, pts[pts.length - 1]![0], pts[pts.length - 1]![1], pts[0]![0], pts[0]![1]));
  return best;
}

type DragState =
  | { kind: "pan"; button: 1 | 2; startScreen: [number, number]; startView: [number, number] }
  | { kind: "move"; refs: string[]; moveKind: "pad" | "graphic" | "text"; startWorld: [number, number] };

function footprintToShapeArg(kind: "segment" | "arc" | "rect" | "circle" | "polygon" | "bezier", pts: [number, number][], layer: string): CmdShape | null {
  const p = (i: number) => ({ x: pts[i]![0], y: pts[i]![1] });
  switch (kind) {
    case "bezier":
      return null; // built from the construction manager's `BezierCurve`, never from a bare point list
    case "segment":
      return pts.length >= 2 ? { kind: "segment", layer, stroke_width: 150, filled: false, start: p(0), end: p(1) } : null;
    case "rect":
      return pts.length >= 2 ? { kind: "rect", layer, stroke_width: 150, filled: false, start: p(0), end: p(1) } : null;
    case "circle":
      return pts.length >= 2 ? { kind: "circle", layer, stroke_width: 150, filled: false, center: p(0), end: p(1) } : null;
    case "arc":
      return pts.length >= 3 ? { kind: "arc", layer, stroke_width: 150, filled: false, start: p(0), mid: p(1), end: p(2) } : null;
    case "polygon":
      return pts.length >= 3 ? { kind: "polygon", layer, stroke_width: 150, filled: true, pts: pts.map((_, i) => p(i)) } : null;
  }
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
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);
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

  const fp = state.footprint;

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
      if (!state.name) dispatch({ type: "SET_VIEW", view: fitTransform({ minX: -15000, minY: -15000, maxX: 15000, maxY: 15000 }, containerSize.width, containerSize.height) });
      return;
    }
    const pts: [number, number][] = [];
    for (const p of fp.pads) {
      const [w, h] = p.size;
      pts.push([p.at.x - w, p.at.y - h], [p.at.x + w, p.at.y + h]);
    }
    for (const g of fp.graphics) for (const [x, y] of shapePoints(g)) pts.push([x, y]);
    for (const t of fp.texts) pts.push([t.at.x, t.at.y]);
    if (pts.length === 0) pts.push([-3000, -3000], [3000, 3000]);
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
    // The Renumber Pads tool shows the numbers it has handed out (and highlights those pads, `pad->SetSelected()`) before anything is committed.
    const session = state.enumerate;
    const shown = fp && session ? { ...fp, pads: fp.pads.map((p) => ({ ...p, number: enumerateShownNumber(session, p.id ?? "", p.number) })) } : fp;
    paintFootprint(ctx, state.view, width, height, shown, {
      selection: session ? new Set([...state.selection, ...session.assigned.keys()]) : state.selection,
      gridUm: state.gridUm,
      gridVisible: state.gridVisible,
      gridOrigin: [gridOrigin.x, gridOrigin.y],
      drawState: state.drawState,
      cursorUm: state.cursorUm,
      movePreview: state.movePreview,
    });
    ctx.restore();
    ctx.restore();
  }, [fp, state.view, state.selection, state.gridUm, state.gridVisible, gridOrigin, state.drawState, state.cursorUm, state.movePreview, state.enumerate, containerSize]);

  const worldAt = useCallback(
    (e: { clientX: number; clientY: number }): [number, number] => {
      const rect = containerRef.current!.getBoundingClientRect();
      return screenToWorld(state.view, e.clientX - rect.left, e.clientY - rect.top);
    },
    [state.view]
  );

  /** Topmost pad/graphic/text under a world point, pad first (copper reads above art, same visual tier the painter uses). */
  const hitTest = useCallback(
    (wx: number, wy: number): { kind: "pad" | "graphic" | "text"; id: string } | null => {
      if (!fp) return null;
      for (let i = fp.pads.length - 1; i >= 0; i--) {
        const p = fp.pads[i]!;
        if (p.id && padContains(p, wx, wy)) return { kind: "pad", id: p.id };
      }
      const tol = Math.max(80, 6 / state.view.scale);
      for (let i = fp.texts.length - 1; i >= 0; i--) {
        const t = fp.texts[i]!;
        if (t.id && Math.hypot(wx - t.at.x, wy - t.at.y) <= Math.max(tol, t.size_um / 2)) return { kind: "text", id: t.id };
      }
      for (let i = fp.graphics.length - 1; i >= 0; i--) {
        const g = fp.graphics[i]!;
        if (g.id && graphicHitDistance(g, wx, wy) <= tol) return { kind: "graphic", id: g.id };
      }
      return null;
    },
    [fp, state.view.scale]
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
    for (const pad of hits) next = enumerateHit(next, pad.id!, pad.number, isClick);
    if (next !== session) dispatch({ type: "SET_ENUMERATE", state: next });
  };

  const finishDraw = useCallback(() => {
    const draw = state.drawState;
    if (!draw) return;
    // An arc ends only when its three points are in (ARC_GEOM_MANAGER); Enter / a double-click leave it running.
    if (draw.shapeKind === "arc") return;
    if (draw.shapeKind === "bezier") {
      // A double-click: "use the current point for all remaining points", accept, and reset (no chaining).
      const at = state.cursorUm ? snapPoint(state.cursorUm.x, state.cursorUm.y, state.gridUm) : draw.bezier?.lastPoint;
      const curve = draw.bezier && at ? bezierFinishDouble(draw.bezier, at) : null;
      if (curve) void api.addGraphic(bezierShape(curve, state.activeLayer, DRAW_STROKE_WIDTH_UM));
      dispatch({ type: "SET_DRAW_STATE", draw: null });
      return;
    }
    const shape = footprintToShapeArg(draw.shapeKind, draw.pts, state.activeLayer);
    if (shape) void api.addGraphic(shape);
    dispatch({ type: "SET_DRAW_STATE", draw: null });
  }, [state.drawState, state.activeLayer, state.cursorUm, state.gridUm, api, dispatch]);

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

    // `EDIT_TOOL::doMoveSelection` armed (Duplicate hands its copies straight to it): a click drops them where they are now.
    if (state.activeTool === "move" && state.duplicatePending) {
      const mp = state.movePreview;
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
      void api.commitMove(mp?.refs ?? [...state.selection], mp?.dxUm ?? 0, mp?.dyUm ?? 0);
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
    // (none for a pad that cannot have one), and the pad placed is pushed back into the default pad.
    if (state.activeTool === "pad" && fp) {
      const master = state.defaultPad;
      const number = padCanHaveNumber(master) ? nextPadNumber(fp.pads) : "";
      const pad: LibraryPad = newPadFromMaster(master, number, { x: sx, y: sy });
      void api.addPad(pad);
      dispatch({ type: "SET_DEFAULT_PAD", pad: settingsOf(pad) });
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

    const shapeKind = SHAPE_TOOL_KIND[state.activeTool];
    // DRAWING_TOOL::drawArc: centre, start, end -- ARC_GEOM_MANAGER decides what each click means and when the arc is done.
    if (shapeKind === "arc") {
      const prev = state.drawState?.shapeKind === "arc" ? state.drawState.arc : undefined;
      const { geom, arc } = arcClick(prev, [sx, sy], arcAngleSnap("45", e));
      if (arc) void api.addGraphic({ kind: "arc", layer: state.activeLayer, stroke_width: DRAW_STROKE_WIDTH_UM, filled: false, start: ptXY(arc.start), mid: ptXY(arc.mid), end: ptXY(arc.end) });
      dispatch({ type: "SET_DRAW_STATE", draw: geom ? { kind: "shape", shapeKind: "arc", pts: arcClickPoints(geom), arc: geom } : null });
      return;
    }
    // DRAWING_TOOL::drawOneBezier (via DrawBezier's chaining loop): start, control 1, end, control 2.
    if (shapeKind === "bezier") {
      const prev = state.drawState?.shapeKind === "bezier" ? state.drawState.bezier : undefined;
      const { geom, curve } = bezierClick(prev, [sx, sy]);
      if (curve) void api.addGraphic(bezierShape(curve, state.activeLayer, DRAW_STROKE_WIDTH_UM));
      dispatch({ type: "SET_DRAW_STATE", draw: geom ? { kind: "shape", shapeKind: "bezier", pts: [], bezier: geom } : null });
      return;
    }
    if (shapeKind) {
      const already = state.drawState?.shapeKind === shapeKind ? state.drawState.pts : [];
      const pts: [number, number][] = [...already, [sx, sy]];
      if (pts.length >= (AUTO_FINISH[shapeKind] ?? Infinity)) {
        const shape = footprintToShapeArg(shapeKind, pts, state.activeLayer);
        if (shape) void api.addGraphic(shape);
        dispatch({ type: "SET_DRAW_STATE", draw: null });
      } else {
        dispatch({ type: "SET_DRAW_STATE", draw: { kind: "shape", shapeKind, pts } });
      }
      return;
    }

    if (state.activeTool === "text") {
      void api.addText({ content: "TEXT", at: { x: sx, y: sy }, angle: 0, layer: state.activeLayer, size_um: 1000, stroke_width: 150, justify: "center", mirror: false });
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
    // The Renumber Pads tool: a drag over pads numbers them (`evt->IsDrag( BUT_LEFT )`); every event moves `oldMousePos` on.
    if (state.activeTool === "enumerate" && state.enumerate) {
      if (e.buttons & 1) enumerateAt(wx, wy, false);
      else lastMouseRef.current = { x: wx, y: wy };
      return;
    }
    // `drawArc` / `drawOneBezier`'s motion branch: update the construction manager's geometry (never its step) with the snapped cursor.
    const draw = state.drawState;
    if (draw && (draw.arc || draw.bezier)) {
      const [sx, sy] = snapPoint(wx, wy, state.gridUm);
      if (draw.arc) dispatch({ type: "SET_DRAW_STATE", draw: { ...draw, arc: arcMotion(draw.arc, [sx, sy], arcAngleSnap("45", e)) } });
      else if (draw.bezier) dispatch({ type: "SET_DRAW_STATE", draw: { ...draw, bezier: bezierMotion(draw.bezier, [sx, sy]) } });
    }
    // The armed Move (a Duplicate's pick-up): the selection follows the snapped cursor from where it was picked up.
    if (state.activeTool === "move" && state.duplicatePending && state.selection.size > 0) {
      const [sx, sy] = snapPoint(wx, wy, state.gridUm);
      // No pointer position was known when the copies were picked up (`doMoveSelection`'s `originalCursorPos`): the first move is it.
      if (!state.moveOriginUm) dispatch({ type: "SET_MOVE_ORIGIN", at: { x: sx, y: sy } });
      const origin = state.moveOriginUm ?? { x: sx, y: sy };
      dispatch({ type: "SET_MOVE_PREVIEW", preview: { refs: [...state.selection], dxUm: sx - origin.x, dyUm: sy - origin.y } });
      return;
    }
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
      const { refs, dxUm, dyUm } = state.movePreview;
      void api.commitMove(refs, dxUm, dyUm); // one undo step for the whole drop
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
    // The Renumber Pads tool owns the double click (it finishes the tool); it must not also open a pad's properties.
    if (state.activeTool === "enumerate" || performance.now() < dblSuppressUntil.current) return;
    if (state.drawState) {
      finishDraw();
      return;
    }
    const [wx, wy] = worldAt(e);
    const hit = hitTest(wx, wy);
    if (hit?.kind === "pad") dispatch({ type: "SET_PAD_PROPERTIES_ID", id: hit.id });
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && state.drawState) {
      e.preventDefault();
      finishDraw();
      return;
    }
    if (e.key === "Escape") {
      // `EDIT_TOOL::Duplicate`: a cancelled pick-up `commit.Revert()`s the copies -- the duplicate was the last undo step.
      if (state.duplicatePending) {
        dispatch({ type: "SET_DUPLICATE_PENDING", pending: false });
        void api.undo();
      }
      dispatch({ type: "ESCAPE" });
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
    if ((e.key === "Delete" || e.key === "Backspace") && state.selection.size > 0 && state.activeTool === "select") {
      for (const id of state.selection) {
        if (api.padById(id)) void api.deletePad(id);
        else if (api.graphicById(id)) void api.deleteGraphic(id);
        else if (api.textById(id)) void api.deleteText(id);
      }
      dispatch({ type: "CLEAR_SELECTION" });
      return;
    }
    // R / Shift+R (rotate the selected pads) is the registered action `pcbnew.InteractiveEdit.rotateCcw` / `rotateCw` (actions/editorFrameActions.ts), which
    // the window-level hotkey runs -- and the top toolbar's Rotate buttons too -- so this canvas no longer handles the key itself (it would rotate twice).
    // `pcbnew.InteractiveEdit.properties` ("E"), same hotkey the PCB tab's
    // own `useActionRunner.ts` binds -- single-pad only, same as that
    // action's own single-item scope there.
    if ((e.key === "e" || e.key === "E") && state.activeTool === "select" && state.selection.size === 1) {
      const [only] = state.selection;
      if (only && api.padById(only)) dispatch({ type: "SET_PAD_PROPERTIES_ID", id: only });
    }
  };

  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    const [wx, wy] = worldAt(e);
    const hit = hitTest(wx, wy);
    if (hit && !state.selection.has(hit.id)) dispatch({ type: "SET_SELECTION", refs: [hit.id] });
    const refs = hit ? (state.selection.has(hit.id) ? [...state.selection] : [hit.id]) : [...state.selection];
    const padRefs = refs.filter((r) => api.padById(r));
    /** An entry that runs a registered KiCad action (PAD_TOOL's selection-menu items, `PAD_TOOL::Init`). */
    const act = (name: string, label: string, disabled = false): MenuEntry => ({ label, onSelect: () => run(name), disabled: disabled || !isEnabled(name) });
    const entries: MenuEntry[] = [
      { label: "Rotate 90° (R)", onSelect: () => padRefs.forEach((id) => void api.rotatePad(id, 1)), disabled: padRefs.length === 0 },
      { label: "Duplicate (Ctrl+D)", onSelect: () => void api.duplicateSelection(false), disabled: refs.length === 0 },
      { label: "Duplicate and Increment (Ctrl+Shift+D)", onSelect: () => void api.duplicateSelection(true), disabled: refs.length === 0 },
      { label: "Pad Properties...", onSelect: () => dispatch({ type: "SET_PAD_PROPERTIES_ID", id: padRefs[0]! }), disabled: padRefs.length !== 1 },
      act("pcbnew.ModuleEditor.padTable", "Pad Table..."),
      act("pcbnew.PadTool.enumeratePads", "Renumber Pads...", (fp?.pads.length ?? 0) === 0),
      act("pcbnew.PadTool.CopyPadSettings", "Copy Pad Properties to Default", padRefs.length !== 1),
      act("pcbnew.PadTool.ApplyPadSettings", "Paste Default Pad Properties to Selected", padRefs.length === 0),
      act("pcbnew.PadTool.PushPadSettings", "Push Pad Properties to Other Pads...", padRefs.length !== 1),
      { label: "Delete (Del)", onSelect: () => refs.forEach((id) => (api.padById(id) ? void api.deletePad(id) : api.graphicById(id) ? void api.deleteGraphic(id) : void api.deleteText(id))), disabled: refs.length === 0 },
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
      {!fp && state.name && <div className="pcb-canvas-empty">{state.error ?? "Loading footprint…"}</div>}
      {/* Nothing open: the empty canvas and its grid, like KiCad's, with one quiet line saying how to open something. */}
      {!fp && !state.name && <div className="canvas-empty-hint">No footprint loaded -- double-click one in Libraries, or press Ctrl+N for a new one</div>}
      {contextMenu && <ContextMenu x={contextMenu.x} y={contextMenu.y} entries={contextMenu.entries} onClose={() => setContextMenu(null)} />}
    </div>
  );
}
