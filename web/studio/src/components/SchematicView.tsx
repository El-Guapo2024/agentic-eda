// The Schematic Editor tab -- a real Canvas2D KiCad-style editor over
// GET /api/schematic's structured data (see schematic/painter.ts), not
// the read-only viewer this used to be (GAPS.md gap #1): selection
// (click + box-select, modifiers ported from kicad-port/selection.ts,
// the same pure logic the PCB canvas's Canvas.tsx already uses), Move
// (`M`, armed then click-to-drop, live preview) and Drag (`G`, same flow
// plus the wires, labels, junctions and no-connects attached stretching
// along), plus a plain click-and-drag on any item (no hotkey needed first,
// same click-vs-drag "moved" distinguisher Canvas.tsx's identical PCB-side
// gesture already uses), which defaults to Drag, same as real eeschema's
// own default for a plain drag. Every kind of item moves, drags, turns and
// mirrors through the one `sch_move` verb (kicad-port/schMove.ts); the live
// preview is that same verb applied in memory on the server (`POST
// /api/sch/move_preview`), so what is shown while an item is held is what
// the drop commits. A plain click on a wire selects it, as in KiCad; the
// net highlight has its own tool. PCB <-> Schematic cross-probing
// (clicking U1 here highlights it there, and back) is still just both
// views reading the same state.selection/state.netHighlight.
//
// Scope: see PARITY-sch.md for the full per-action table.
import { useEffect, useMemo, useRef, useState } from "react";
import type { LabelScope, Schematic } from "../api/types";
import { useStudioApi, useStudioDispatch, useStudioState, type ToolId } from "../state/store";
import { boundsOfPoints, fitTransform } from "./canvas/view";
import { handleWheel, type WheelInput } from "../kicad-port/viewControls";
import { useWheelPrefs } from "../actions/useWheelPrefs";
import { useNonPassiveWheel } from "../hooks/useNonPassiveWheel";
import { paintSchematic } from "./schematic/painter";
import { resolveLibSymbol } from "./schematic/libSymbol";
import { GRID } from "./schematic/layout";
import { layerColor } from "./canvas/layers";
import { drawPageAndFrame, drawZoneReferences, drawTitleBlock, drawGridDots, pageOf } from "./schematic/drawingSheet";
import { computeClickModifiers, applySingleClickModifier, isCrossingSelection, applyBoxSelectionModifiers, hasModifier } from "../kicad-port/selection";
import { alignToGrid } from "../kicad-port/gridSnap";
import { isMac } from "../platform";
import { ClickDragGesture, dragRuleFor } from "../kicad-port/dragThreshold";
import { applyPatch, heldCmd, holdPoint, pickedVertices, previewBody, wirePick } from "../kicad-port/schMove";
import { LINE_MODE_FREE } from "../kicad-port/schLineMode";
import { postMovePreview } from "../api/client";
import type { SchMovePatch } from "../api/schEditTypes";
import { nextReference } from "../kicad-port/nextReference";
import { wireTail } from "../kicad-port/schLineMode";
import { boxItems, hitItems } from "./schematic/schItems";
import { openSchProperties } from "./schematic/schPropertiesOpen";
import { SchContextMenu } from "./schematic/SchContextMenu";
import { ContextMenu, type MenuEntry } from "./canvas/ContextMenu";
import { ercSeverityShown } from "./schematic/displayOptions";
import { ercMarkerPosition } from "./schematic/ercMarkerPosition";
import { ERC_MARKER_HIT_UM, nearestMarker } from "../kicad-port/markerHit";
import { ercKey, rcMenu } from "../kicad-port/rcItems";
import { ercSettingSeverity, ercTitle, isPinMapCheck, runMarkerMenu, toMenuEntries } from "../actions/checkerOps";
import { askExclusionComment } from "../state/checkerView";
import { summarizeSelection } from "./schematic/schSelectionSummary";
import { schContextMenu } from "../kicad-port/schContextMenu";
import { schSelectable } from "../kicad-port/schSelectionFilter";
import { finishShapeDraw, isSchShapeTool, paintShapePreview, shapeToolClick } from "./schematic/schShapeTools";
import { breakPreviewSheet, commitBreak } from "./schematic/schBreakTool";
import { paintPinPreview, placePinClick } from "./schematic/schPinTool";
import type { MenuNode } from "../kicad/types";
import { hitSymbol, hitWire, schematicBounds } from "./schematic/schHit";
import { hitField } from "../kicad-port/schFieldEdit";
import { measureStrokeText } from "./text/strokeFont";
import { isExplicitJunctionAllowed, junctionCandidates, type JunctionSchematic } from "../kicad-port/schJunction";
import { sheetSize } from "../kicad-port/schSheet";
import { isStale } from "../kicad-port/checkRevision";
import { useSchControlState } from "../state/schControlStore";
import { hitSheet } from "../kicad-port/schControl";
import { netAtClick } from "../actions/schControlActions";
import type { Cmd } from "../api/types";
import { placeSchPaste } from "../actions/schClipboardActions";
import { pasteOffset, previewIds, withPaste } from "../kicad-port/schClipboard";
import { cancelSchPaste, useSchPaste } from "../state/schPasteStore";
import "../styles/canvas.css";

type DragState =
  | { kind: "pan"; startScreen: [number, number]; startView: [number, number] }
  | { kind: "box"; startWorld: [number, number]; startScreen: [number, number] }
  /**
   * A plain click-and-drag on any item, started without `M`/`G` armed
   * first -- sch_selection_tool.cpp Main()'s own `IsDrag(BUT_LEFT)`
   * handler (see this file's onPointerDown for the full port): KiCad's
   * real default for a plain drag on a movable item is "Drag" (rubber-
   * band), not "Move", so that's what this defaults to as well, same as
   * `G` itself arms. `moved` is tool_dispatcher.cpp's click-versus-drag
   * rule (kicad-port/dragThreshold.ts): false until the press has become
   * a drag -- more than 8 px of travel along an axis, or on macOS a
   * motion after 300 ms held -- so a click-release with a grid step of
   * jitter still falls through to an ordinary select instead of
   * committing a drag.
   */
  | { kind: "move"; refs: string[]; startWorld: [number, number]; moved: boolean; vertices?: Record<string, number[]> };

/** Which `add_label` scope each of the three label tools commits -- `L`/Ctrl+`L`/`H`, see useActionRunner.ts's own `eeschema.InteractiveDrawing.place*Label` bindings for how each one arms its tool id. */
const LABEL_TOOL_SCOPE: Partial<Record<ToolId, LabelScope>> = {
  sch_label_local: "local",
  sch_label_global: "global",
  sch_label_hier: "hierarchical",
};

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

/** The junction analysis's view of the sheet (`JUNCTION_HELPERS::AnalyzePoint`'s inputs): wire/bus polylines, pin tips (and power symbols), labels, bus entries. */
function junctionModel(sch: Schematic): JunctionSchematic {
  return {
    wires: sch.wires.map((w) => ({ bus: w.bus, pts: w.pts })),
    pinTips: pinSnapPoints(sch),
    labels: sch.labels.map((l) => l.at),
    busEntries: sch.bus_entries.map((be) => ({ a: be.at, b: [be.at[0] + be.size[0], be.at[1] + be.size[1]] as [number, number] })),
  };
}

function nearestSnapPoint(pts: Array<[number, number]>, xUm: number, yUm: number, thresholdUm: number): [number, number] | null {
  let best: { p: [number, number]; d: number } | null = null;
  for (const p of pts) {
    const d = Math.hypot(p[0] - xUm, p[1] - yUm);
    if (d <= thresholdUm && (!best || d < best.d)) best = { p, d };
  }
  return best?.p ?? null;
}

// collectBoxSelection moved to ./schematic/boxSelection.ts this session
// (symbols + wires now, previously symbols only -- see that file's own
// doc) so it's unit-testable like this app's other pure geometry modules.

export function SchematicView() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const wheelPrefs = useWheelPrefs();
  const schControl = useSchControlState();
  /** The paste being carried (`SCH_EDITOR_CONTROL::Paste` -> the Move tool): the items it would add, until a click drops them or Escape throws them away. */
  const paste = useSchPaste();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);
  /** The pressed button's `BUTTON_STATE` (tool_dispatcher.cpp): whether the press has become a drag. Made at every press. */
  const gestureRef = useRef<ClickDragGesture | null>(null);
  const userMovedRef = useRef(false);
  /** Previous wire-preview elbow, for computeBreakPoint's "maintain current line shape" hint (kicad-port/schLineMode.ts). */
  const elbowRef = useRef<{ mid: readonly [number, number]; end: readonly [number, number] } | null>(null);
  /** The plain-click/drag symbol-drag distinguisher's deferred click (see DragState's own "move" kind doc) -- set only for a click that keeps a multi-selection's whole group for a potential drag, same as Canvas.tsx's own `pendingClickRef`. */
  const pendingClickRef = useRef<string | null>(null);
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 });
  const [marquee, setMarquee] = useState<{ x0: number; y0: number; x1: number; y1: number; crossing: boolean } | null>(null);
  /** The right-click menu (`SCH_SELECTION_TOOL`'s context menu): where it is and what it offers for the selection. */
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; nodes: MenuNode[] } | null>(null);
  /** The menu of an ERC marker the last right click was on (kicad-port/rcItems.ts `rcMenu`). */
  const [markerMenu, setMarkerMenu] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  const sch = state.schematic;
  /** `SCH_SELECTION_TOOL::itemPassesFilter`: an item of a category the selection filter has off, or a locked one without its "Locked items", cannot be picked. */
  const selectable = sch ? schSelectable(sch, state.schSelectionFilter) : () => true;
  const pickable = (id: string | null): string | null => (id && selectable(id) ? id : null);
  const moveMode = state.activeTool === "move";
  const dragMode = state.activeTool === "drag";
  /** `M` and `G`: which `sch_move` verb a committed preview becomes (a plain click-drag is a `sch_drag`, sch_selection_tool.cpp's own default for a plain drag). */
  const armedKind: "sch_move" | "sch_drag" | null = moveMode ? "sch_move" : dragMode ? "sch_drag" : null;

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

  /** The last view this auto-fit itself set: a different live view means a hotkey/menu zoom or pan changed it, which counts as the user moving the view (no re-fit under them). */
  const autoViewRef = useRef<typeof state.schematicView | null>(null);
  useEffect(() => {
    if (autoViewRef.current && state.schematicView !== autoViewRef.current && state.schematicView.scale !== 0) userMovedRef.current = true;
    if (!sch || userMovedRef.current) return;
    const bounds = boundsOfPoints(schematicBounds(sch));
    if (!bounds || containerSize.width < 50 || containerSize.height < 50) return;
    const view = fitTransform(bounds, containerSize.width, containerSize.height, 80);
    autoViewRef.current = view;
    dispatch({ type: "SET_SCHEMATIC_VIEW", view });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sch, containerSize, dispatch]);

  // A held selection (M, G or a click-drag) is shown where the drop would put it: the same commands the drop sends, applied in memory by the
  // server (`POST /api/sch/move_preview`), laid over the sheet. One request is in flight at a time; when it returns, the newest offset is sent.
  const [patch, setPatch] = useState<SchMovePatch | null>(null);
  const previewPump = useRef<{ busy: boolean; want: string | null; sent: string | null; alive: boolean }>({ busy: false, want: null, sent: null, alive: true });
  const held = state.movePreview != null && (state.movePreview.kind === "sch_move" || state.movePreview.kind === "sch_drag") ? state.movePreview : null;
  useEffect(() => {
    const pump = previewPump.current;
    pump.alive = true;
    const origin = state.moveOriginUm;
    const cmd = held
      ? heldCmd(
          { mode: held.kind === "sch_drag" ? "drag" : "move", ids: held.refs, vertices: held.vertices, dxUm: held.dxUm, dyUm: held.dyUm, turns: held.turns, holdUm: holdPoint(origin, held.dxUm, held.dyUm) },
          { ortho: state.schLineMode !== LINE_MODE_FREE }
        )
      : null;
    if (!cmd) {
      pump.want = null;
      pump.sent = null;
      setPatch(null);
      return;
    }
    pump.want = JSON.stringify(previewBody(cmd, state.currentSheetPath));
    if (pump.busy) return;
    pump.busy = true;
    void (async () => {
      try {
        while (pump.alive && pump.want && pump.want !== pump.sent) {
          const body = pump.want;
          pump.sent = body;
          const next = await postMovePreview(JSON.parse(body));
          if (pump.alive && pump.want === body) setPatch(next);
        }
      } finally {
        pump.busy = false;
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.movePreview, state.moveOriginUm, state.schLineMode, state.currentSheetPath]);
  useEffect(() => {
    const pump = previewPump.current;
    return () => {
      pump.alive = false;
    };
  }, []);
  // A Break / Slice in progress shows the cut lines as their pieces, the new end at the (grid-snapped) cursor (kicad-port/schBreak.ts).
  const breaking = state.drawState?.kind === "sch_shape" ? state.drawState.brk : undefined;
  const breakSnap = breaking && state.cursorUm ? alignToGrid({ x: state.cursorUm.x, y: state.cursorUm.y }, GRID, { x: 0, y: 0 }, { ctrlOrCmd: false }) : null;
  const breakCursor: [number, number] | null = breakSnap ? [breakSnap.x, breakSnap.y] : null;
  const baseDisplaySch: Schematic | null = sch && held && patch ? applyPatch(sch, patch) : sch && breaking && breakCursor ? breakPreviewSheet(sch, breaking, breakCursor) : sch;
  // A paste in progress: what it would add, shifted so its anchor sits on the grid-snapped cursor (`SCH_MOVE_TOOL::Main`'s `delta = m_cursor - *m_anchorPos`) and drawn
  // selected, as KiCad selects the pasted items. The sheet itself is untouched until the click (kicad-port/schClipboard.ts).
  const pasteSnap = paste && state.cursorUm ? alignToGrid({ x: state.cursorUm.x, y: state.cursorUm.y }, GRID, { x: 0, y: 0 }, { ctrlOrCmd: false }) : null;
  const pasteDx = paste && pasteSnap ? pasteOffset(paste.anchor, [pasteSnap.x, pasteSnap.y])[0] : 0;
  const pasteDy = paste && pasteSnap ? pasteOffset(paste.anchor, [pasteSnap.x, pasteSnap.y])[1] : 0;
  const pasteShown = paste != null && pasteSnap != null;
  const displaySch: Schematic | null = useMemo(() => (baseDisplaySch && paste && pasteShown ? withPaste(baseDisplaySch, paste.preview, pasteDx, pasteDy) : baseDisplaySch), [baseDisplaySch, paste, pasteShown, pasteDx, pasteDy]);
  const pasteSelection = useMemo(() => (paste && pasteShown ? new Set(previewIds(paste.preview)) : null), [paste, pasteShown]);
  // A paste belongs to the sheet it was started on and to the select tool: leaving the sheet, the tab or the tool throws it away.
  useEffect(() => {
    if (paste && (paste.sheet.join("/") !== state.currentSheetPath.join("/") || state.activeTool !== "select")) cancelSchPaste();
  }, [paste, state.currentSheetPath, state.activeTool]);
  useEffect(() => () => void cancelSchPaste(), []);

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
    // The paper this sheet is laid out for (A4 unless the sheet says otherwise), and where in the hierarchy it is.
    const page = pageOf(sch.paper);
    drawPageAndFrame(ctx, state.schematicView, page);
    drawGridDots(ctx, state.schematicView, width, height, GRID, page);
    drawZoneReferences(ctx, state.schematicView, page);
    const tb = sch.title_block;
    const crumbs = sch.sheet_path ?? [];
    drawTitleBlock(
      ctx,
      state.schematicView,
      {
        title: tb?.title || state.board?.name || "untitled",
        date: tb?.date ?? new Date().toISOString().slice(0, 10),
        rev: tb?.rev ?? "",
        company: tb?.company,
        comments: tb?.comments,
        fileName: sch.file || `${state.board?.name || "schematic"}.kicad_sch`,
        sheetPath: crumbs.length === 0 ? "/" : `/${crumbs.map((c) => c.name).join("/")}/`,
      },
      page
    );
    paintSchematic(ctx, state.schematicView, displaySch, { selection: pasteSelection ?? state.selection, netHighlight: state.netHighlight, ercViolations: state.erc?.violations ?? null, ercStale: isStale(state.ercVersion, state.version), ercSelected: state.ercSelected, lintViolations: state.ercDialogOpen ? (state.lint?.schematic.violations ?? null) : null, lintSelected: state.ercLintSelected, display: schControl.display });
    if (state.drawState?.kind === "wire") {
      // sch_line_wire_bus_tool.cpp doDrawSegments + computeBreakPoint: the
      // rubber band from the last click to the cursor is two segments (an
      // elbow) in LINE_MODE_90/45, one straight segment in LINE_MODE_FREE.
      let pts = state.drawState.pts;
      if (state.cursorUm) {
        const c = alignToGrid({ x: state.cursorUm.x, y: state.cursorUm.y }, GRID, { x: 0, y: 0 }, { ctrlOrCmd: false });
        const tail = wireTail(pts[pts.length - 1]!, [c.x, c.y], state.schLineMode, state.schPosture, elbowRef.current);
        elbowRef.current = tail.length === 2 ? { mid: tail[0]!, end: tail[1]! } : null;
        pts = [...pts, ...(tail as [number, number][])];
      }
      // GAPS.md #20: the bus tool shares this exact preview (same
      // `DrawState` kind), colored to match whichever is actually armed.
      ctx.strokeStyle = layerColor(state.activeTool === "bus" ? "LAYER_BUS" : state.activeTool === "sch_line" ? "LAYER_NOTES" : "LAYER_WIRE");
      ctx.lineWidth = Math.max(150, (1 / state.schematicView.scale) * 1.5);
      ctx.beginPath();
      pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.stroke();
      // `C` (Unfold from Bus): the bus entry already sits at its root and the wire runs from its far end -- show the stub too.
      const unfold = state.busUnfold;
      if (unfold) {
        ctx.strokeStyle = layerColor("LAYER_BUS");
        ctx.beginPath();
        ctx.moveTo(unfold.entryAt[0], unfold.entryAt[1]);
        ctx.lineTo(unfold.entryAt[0] + unfold.size[0], unfold.entryAt[1] + unfold.size[1]);
        ctx.stroke();
      }
    }
    // The shape or rule area being drawn (kicad-port/schShapeEdit.ts, polygonGeom.ts), rubber-banded to the cursor.
    if (state.drawState?.kind === "sch_shape" && state.cursorUm) {
      paintShapePreview(ctx, state.schematicView, state.drawState, shapeSnap(state.cursorUm.x, state.cursorUm.y), state.schLineMode);
      // The sheet pin the next click would drop.
      if (state.drawState.pin) paintPinPreview(ctx, state.schematicView, sch, state.drawState.pin, shapeSnap(state.cursorUm.x, state.cursorUm.y));
    }
    // `S`: the sheet being sized, from its first corner to the (grid-snapped) cursor.
    if (state.drawState?.kind === "sheet" && state.cursorUm) {
      const start = state.drawState.start;
      const c = alignToGrid({ x: state.cursorUm.x, y: state.cursorUm.y }, GRID, { x: 0, y: 0 }, { ctrlOrCmd: false });
      const [w, h] = sheetSize(start, [c.x, c.y], GRID);
      ctx.strokeStyle = layerColor("LAYER_SHEET");
      ctx.lineWidth = Math.max(150, (1 / state.schematicView.scale) * 1.5);
      ctx.setLineDash([4 / state.schematicView.scale, 3 / state.schematicView.scale]);
      ctx.strokeRect(start[0], start[1], w, h);
      ctx.setLineDash([]);
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
  }, [sch, displaySch, pasteSelection, state.schLineMode, state.schPosture, state.schematicView, state.selection, state.netHighlight, containerSize, state.board?.name, marquee, state.drawState, state.cursorUm, state.erc, state.ercVersion, state.version, state.ercSelected, state.ercDialogOpen, state.lint, state.ercLintSelected, state.activeTool, schControl.display]);

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

  /** The point a shape tool takes for the cursor: the grid (`GRID_GRAPHICS`), or for a rule area also a pin it is near (`GRID_CONNECTABLE`). */
  const shapeSnap = (xUm: number, yUm: number): [number, number] =>
    (state.activeTool === "sch_rule_area" && sch ? nearestSnapPoint(pinSnapPoints(sch), xUm, yUm, 400 / state.schematicView.scale) : null) ?? snapToGrid(xUm, yUm);

  /**
   * Commit a finished click-to-add-point polyline: a wire or bus (`add_wire`), or -- with the Draw Lines tool -- a graphic line
   * (`add_sch_line`). A wire drawn out of an "Unfold from Bus" (`C`, SCH_LINE_WIRE_BUS_TOOL::UnfoldBus) goes in with its bus entry
   * and the member's label at the far end as ONE undo step, like the C++'s single commit.
   */
  const commitDrawn = (rawPts: readonly [number, number][]) => {
    // A double-click lands its point twice (once per pointer-down); `SCH_LINE_WIRE_BUS_TOOL::finishSegments` drops the resulting
    // zero-length segments (`IsNull()`), so collapse consecutive duplicates before committing.
    const pts = rawPts.filter((p, i) => i === 0 || p[0] !== rawPts[i - 1]![0] || p[1] !== rawPts[i - 1]![1]);
    if (pts.length < 2) return;
    const points = pts.map(([x, y]) => ({ x, y }));
    if (state.activeTool === "sch_line") {
      void api.cmd({ op: "add_sch_line", pts: points });
      return;
    }
    const wire: Cmd = { op: "add_wire", pts: points, bus: state.activeTool === "bus" };
    const unfold = state.busUnfold;
    if (!unfold) {
      void api.cmd(wire);
      return;
    }
    const end = points[points.length - 1]!;
    void api.cmdBatch([
      { op: "add_bus_entry", at: { x: unfold.entryAt[0], y: unfold.entryAt[1] }, size: { x: unfold.size[0], y: unfold.size[1] } },
      wire,
      { op: "add_label", net: unfold.net, at: end, kind: { scope: "local" } },
    ]);
    dispatch({ type: "SET_BUS_UNFOLD", unfold: null });
    dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
  };

  const onWheel = (e: WheelEvent) => {
    // The canvas's context menu is a child of this container; a wheel over it is the menu's (React's own `onWheel={keep}` on the menu cannot stop a native listener).
    if ((e.target as Element | null)?.closest?.(".menubar-dropdown")) return;
    // Before the first fit the view has no scale yet (0): panning by 1/scale would make it NaN.
    if (!sch || !(state.schematicView.scale > 0)) return;
    e.preventDefault();
    userMovedRef.current = true;
    // wx_view_controls.cpp onWheel via handleWheel, the same as the PCB and library canvases: the wheel gestures and zoom speed
    // are Preferences > Mouse and Touchpad's.
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
    const result = handleWheel(state.schematicView, { width: rect.width, height: rect.height }, input, wheelPrefs.settings, wheelPrefs.controller);
    if (result.kind !== "unhandled") dispatch({ type: "SET_SCHEMATIC_VIEW", view: result.view });
  };
  // React's own onWheel is passive, so its `preventDefault()` was ignored (and logged on every tick) while the page scrolled under the zoom.
  useNonPassiveWheel(containerRef, onWheel);

  return (
    <div
      ref={containerRef}
      className="pcb-canvas-container"
      style={{ cursor: dragRef.current?.kind === "pan" ? "grabbing" : moveMode || dragMode || dragRef.current?.kind === "move" ? "move" : "default" }}
      onPointerDown={(e) => {
        if (!sch) return;
        const gesture = new ClickDragGesture(dragRuleFor(isMac()));
        gesture.down(e.clientX, e.clientY, e.timeStamp);
        gestureRef.current = gesture;
        if (e.button === 1) {
          userMovedRef.current = true;
          (e.target as Element).setPointerCapture(e.pointerId);
          dragRef.current = { kind: "pan", startScreen: [e.clientX, e.clientY], startView: [state.schematicView.x, state.schematicView.y] };
          return;
        }
        if (e.button !== 0) return;
        const [wx, wy] = toWorld(e.clientX, e.clientY);

        // A paste in progress: this click drops it where the cursor holds it (`SCH_MOVE_TOOL`'s click, then `commit.Push( _( "Paste" ) )`).
        if (paste) {
          void placeSchPaste(api, dispatch, snapToGrid(wx, wy));
          return;
        }

        // Highlight Nets (`SCH_EDITOR_CONTROL::HighlightNetCursor`'s picker): a click is `highlightNet` at that point -- the net of the wire, label or power symbol there,
        // or, over nothing, the highlight cleared. The tool stays armed.
        if (state.activeTool === "sch_highlight_net") {
          dispatch({ type: "SET_NET_HIGHLIGHT", net: netAtClick(sch, wx, wy, state.schematicView.scale) });
          return;
        }

        // Place Pins from Sheet: pick the sheet, then drop each pin on its border.
        if (state.activeTool === "sch_sheet_pin") {
          const placement = state.drawState?.kind === "sch_shape" && state.drawState.pin ? state.drawState.pin : { sheetId: null, queue: [] };
          void placePinClick({ sch, placement, path: state.currentSheetPath, dispatch, api }, snapToGrid(wx, wy), 6 / state.schematicView.scale);
          return;
        }

        // Break / Slice: the click that drops the new end of the cut wire.
        if (state.activeTool === "sch_break" && state.drawState?.kind === "sch_shape" && state.drawState.brk) {
          commitBreak(state.drawState.brk, snapToGrid(wx, wy), dispatch, api);
          return;
        }

        // `I` (Draw Lines) shares this exact click-to-add-point state machine with the wire and bus tools -- it differs only in
        // what it commits (a graphic notes-layer line) and in connecting to nothing: no pin snap, no auto-finish on a pin
        // (`GRID_GRAPHICS` instead of `GRID_WIRES` in `SCH_LINE_WIRE_BUS_TOOL::DrawSegments`).
        if (state.activeTool === "wire" || state.activeTool === "bus" || state.activeTool === "sch_line") {
          const isLine = state.activeTool === "sch_line";
          const thresholdUm = 400 / state.schematicView.scale;
          const snapped = (isLine ? null : nearestSnapPoint(pinSnapPoints(sch), wx, wy, thresholdUm)) ?? snapToGrid(wx, wy);
          const draw = state.drawState;
          if (draw?.kind !== "wire") {
            dispatch({ type: "SET_DRAW_STATE", draw: { kind: "wire", pts: [snapped] } });
            return;
          }
          const tail = wireTail(draw.pts[draw.pts.length - 1]!, snapped, state.schLineMode, state.schPosture, elbowRef.current);
          elbowRef.current = null;
          const next = { ...draw, pts: [...draw.pts, ...(tail as [number, number][])] };
          // sch_screen.cpp IsTerminalPoint, simplified: landing back on a
          // pin auto-finishes the wire, same as a real click on a pin/
          // junction/other wire does in source -- this app only checks
          // the pin case (see this file's header comment for the rest).
          const onPin = !isLine && pinSnapPoints(sch).some(([px, py]) => px === snapped[0] && py === snapped[1]);
          if (onPin && next.pts.length >= 2) {
            commitDrawn(next.pts);
            dispatch({ type: "SET_DRAW_STATE", draw: null });
          } else {
            dispatch({ type: "SET_DRAW_STATE", draw: next });
          }
          return;
        }

        // Draw Rectangle / Circle / Arc / Bezier / Text Box / Rule Area and Place Directive Label (components/schematic/schShapeTools.ts).
        if (isSchShapeTool(state.activeTool)) {
          shapeToolClick({ tool: state.activeTool, draw: state.drawState, sch, lineMode: state.schLineMode, dispatch, api }, shapeSnap(wx, wy));
          return;
        }

        // `J` (SCH_DRAWING_TOOLS::SingleClickPlace, SCH_JUNCTION_T): snaps to a wire vertex, a pin or a crossing; refuses a point where
        // fewer than three directions meet ("Junction location contains no joinable wires and/or pins."); a click on an existing
        // junction does nothing. The tool stays armed for the next one.
        if (state.activeTool === "sch_junction") {
          const model = junctionModel(sch);
          const snapped = nearestSnapPoint(junctionCandidates(model) as Array<[number, number]>, wx, wy, 12 / state.schematicView.scale) ?? snapToGrid(wx, wy);
          if ((sch.junctions ?? []).some((j) => j.at[0] === snapped[0] && j.at[1] === snapped[1])) return;
          if (!isExplicitJunctionAllowed(model, snapped)) {
            dispatch({ type: "TOAST", message: "Junction location contains no joinable wires and/or pins.", kind: "error" });
            return;
          }
          void api.cmd({ op: "add_junction", at: { x: snapped[0], y: snapped[1] } });
          return;
        }

        // `S` (SCH_DRAWING_TOOLS::DrawSheet): the first click is the sheet's top-left corner, the second sizes it (`sizeSheet`) and opens
        // the properties dialog (SheetDialog.tsx).
        if (state.activeTool === "sch_sheet") {
          const c = snapToGrid(wx, wy);
          const draw = state.drawState;
          if (draw?.kind !== "sheet") {
            dispatch({ type: "SET_DRAW_STATE", draw: { kind: "sheet", start: c } });
            return;
          }
          dispatch({ type: "SET_DRAW_STATE", draw: null });
          dispatch({ type: "SET_SCH_SHEET_PENDING", pending: { at: draw.start, size: sheetSize(draw.start, c, GRID) } });
          return;
        }

        // `L`/Ctrl+`L`/`H`/`P`/`T`/`Q` (sch_drawing_tools.cpp SingleClickPlace/
        // TwoClickPlace): every one of these tools stays armed after a
        // click (same as the wire tool above) for chained placement --
        // real eeschema does too (Escape, or the hotkey again, is how you
        // leave the tool). `L`/`P` pin-snap the same way the wire tool
        // does (a label commonly tags a wire/pin; a power symbol's own
        // `pin` field needs to land on a real pin to resolve at all, see
        // `Cmd::AddPowerSymbol`'s doc) -- `T` is free text, plain grid
        // snap only.
        const labelScope = LABEL_TOOL_SCOPE[state.activeTool];
        if (labelScope) {
          const snapped = nearestSnapPoint(pinSnapPoints(sch), wx, wy, 400 / state.schematicView.scale) ?? snapToGrid(wx, wy);
          dispatch({ type: "SET_SCH_LABEL_PENDING", pending: { at: snapped, scope: labelScope } });
          return;
        }
        if (state.activeTool === "sch_power") {
          const snapped = nearestSnapPoint(pinSnapPoints(sch), wx, wy, 400 / state.schematicView.scale) ?? snapToGrid(wx, wy);
          dispatch({ type: "SET_SCH_POWER_PENDING", pending: { at: snapped } });
          return;
        }
        if (state.activeTool === "sch_text") {
          dispatch({ type: "SET_SCH_TEXT_PENDING", pending: { at: snapToGrid(wx, wy) } });
          return;
        }
        if (state.activeTool === "sch_no_connect") {
          const [sx, sy] = nearestSnapPoint(pinSnapPoints(sch), wx, wy, 400 / state.schematicView.scale) ?? snapToGrid(wx, wy);
          api.cmd({ op: "add_no_connect", at: { x: sx, y: sy } });
          return;
        }
        if (state.activeTool === "sch_bus_entry") {
          const [sx, sy] = snapToGrid(wx, wy);
          api.cmd({ op: "add_bus_entry", at: { x: sx, y: sy }, size: { x: 2_540, y: 2_540 } });
          return;
        }
        // `A`: SymbolChooserDialog already picked the symbol (state.
        // armedSymbol); this click only decides where. `id` is a real,
        // already-numbered reference assigned right now, not a "U?"
        // placeholder -- see nextReference.ts's own doc for why. `value`
        // defaults to the symbol's own bare name (e.g. "Device:R" ->
        // "R"), matching what a real library's own default Value usually
        // is for a part this simple.
        if (state.activeTool === "sch_place_symbol" && state.armedSymbol) {
          const { libId, referencePrefix, unit, ref } = state.armedSymbol;
          const id = ref ?? nextReference(sch.symbols, referencePrefix || "U", state.board?.parts.map((p) => p.ref));
          const value = state.armedSymbol.value ?? (libId.includes(":") ? libId.slice(libId.indexOf(":") + 1) : libId);
          const [sx, sy] = snapToGrid(wx, wy);
          api.cmd({ op: "add_symbol", id, lib_id: libId, at: { x: sx, y: sy }, rot_millideg: 0, value, footprint: state.armedSymbol.footprint ?? "", unit });
          // `PlaceNextSymbolUnit` hands the tool one symbol -- "place that and get out of the placement tool" (`placeOneOnly`).
          if (ref) {
            dispatch({ type: "SET_ARMED_SYMBOL", symbol: null });
            dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
          }
          return;
        }

        // `M`/`G`-armed move/drag: this click drops whatever is being held, same two-step ("arm, then click to commit") flow
        // useActionRunner.ts's pcbnew.InteractiveMove.move already uses -- Escape (the ESCAPE reducer case, generic across
        // tabs) cancels it instead, never this handler. A drop with the pointer never moved and nothing turned sends nothing.
        if (armedKind) {
          dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
          if (state.movePreview) void api.commitSchHeld(state.movePreview);
          return;
        }

        const ctrlOrCmd = isMac() ? e.metaKey : e.ctrlKey;
        const modifiers = computeClickModifiers(e.shiftKey, ctrlOrCmd, e.altKey);
        const tolerance = 6 / (state.schematicView.scale || 1);
        // The item under the pointer, topmost first: a symbol, any other placed item (a label, text, power symbol, junction, no-connect, bus entry,
        // sheet or drawn shape -- tight, screen-sized tolerance) and last a wire (`SCH_SELECTION_TOOL::selectPoint`: a click on a wire selects it;
        // the net highlight is a tool of its own). Every kind of item is picked up and dragged the same way.
        // A shown field (the Reference, the Value, ...) is picked before the symbol whose box it may sit over: its own text box is what a click hits.
        const fieldId = pickable(hitField(sch, wx, wy, tolerance / 2, measureStrokeText));
        const symId = fieldId ? null : pickable(hitSymbol(sch, wx, wy));
        const placedItem = fieldId || symId ? null : (hitItems(sch, wx, wy, tolerance).find((r) => r.kind !== "symbol" && r.kind !== "wire" && pickable(r.id))?.id ?? null);
        const wireId = fieldId || symId || placedItem ? null : pickable(hitWire(sch, wx, wy, tolerance));
        const picked = fieldId ?? symId ?? placedItem ?? wireId;
        if (picked) {
          dispatch({ type: "SET_NET_HIGHLIGHT", net: null });
          if (hasModifier(modifiers)) {
            // A modified click always applies immediately, never starts a drag -- real sch_selection_tool.cpp routes a modified drag to
            // box-select instead, even one starting on a hit item (see this file's "nothing hit" box-select branch below).
            dispatch({ type: "SET_SELECTION", refs: applySingleClickModifier(state.selection, picked, modifiers) });
            dispatch({ type: "SET_SCH_WIRE_PICK", pick: null });
            return;
          }
          // sch_selection_tool.cpp Main(): a plain click-and-drag starting on an item that's already part of a larger selection keeps the
          // WHOLE group for the drag, deferring the "collapse to just this one" effect to onPointerUp in case no real drag happens;
          // otherwise (re)select just this one item immediately. Either way, arm a potential drag -- source's own default for a plain drag
          // on a movable item is SCH_ACTIONS::drag (wires stretch), the same default `G` itself arms, not Move.
          const keepGroupForDrag = state.selection.has(picked) && state.selection.size > 1;
          let refs: string[];
          if (keepGroupForDrag) {
            refs = [...state.selection];
            pendingClickRef.current = picked;
          } else {
            refs = [picked];
            dispatch({ type: "SET_SELECTION", refs });
          }
          // `selectPoint`'s STARTPOINT / ENDPOINT: a click near the end of a wire picks that end only, so the drag stretches it.
          const wire = wireId ? sch.wires.find((w) => w.id === wireId) : undefined;
          const vertices = wire && refs.length === 1 ? (wirePick(wire, [wx, wy], tolerance) ?? undefined) : undefined;
          dispatch({ type: "SET_SCH_WIRE_PICK", pick: wire && vertices && refs.length === 1 ? { id: wire.id, vertices } : null });
          dispatch({ type: "SET_MOVE_ORIGIN", at: { x: wx, y: wy } });
          dragRef.current = { kind: "move", refs, startWorld: [wx, wy], moved: false, vertices: wire && vertices ? { [wire.id]: vertices } : undefined };
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
        const motion = gestureRef.current?.move(e.clientX, e.clientY, e.timeStamp);

        if (armedKind && state.selection.size > 0) {
          const origin = state.moveOriginUm ?? { x: wx, y: wy };
          const [ox, oy] = snapToGrid(origin.x, origin.y);
          const [sx, sy] = snapToGrid(wx, wy);
          // R, Shift+R, X and Y pressed since the items were picked up stay on the preview as it follows the cursor
          dispatch({ type: "SET_MOVE_PREVIEW", preview: { refs: [...state.selection], kind: armedKind, dxUm: sx - ox, dyUm: sy - oy, vertices: pickedVertices(state), turns: state.movePreview?.turns } });
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
          if (!motion?.dragging) return; // a click with a little jitter, not a rectangle
          const rect = containerRef.current!.getBoundingClientRect();
          const x0 = drag.startScreen[0] - rect.left,
            y0 = drag.startScreen[1] - rect.top;
          const x1 = e.clientX - rect.left,
            y1 = e.clientY - rect.top;
          setMarquee({ x0, y0, x1, y1, crossing: isCrossingSelection(x0, x1) });
        } else if (drag.kind === "move") {
          // The click-vs-drag distinguisher is tool_dispatcher.cpp's (kicad-port/dragThreshold.ts): until the press has become a drag -- more than 8 px
          // of travel along an axis, or on macOS a motion after 300 ms held -- nothing is picked up, so the release is still a plain click.
          if (!motion?.dragging) return;
          drag.moved = true;
          const [sx, sy] = snapToGrid(wx, wy);
          const [ox, oy] = snapToGrid(drag.startWorld[0], drag.startWorld[1]);
          dispatch({ type: "SET_MOVE_PREVIEW", preview: { refs: drag.refs, kind: "sch_drag", dxUm: sx - ox, dyUm: sy - oy, vertices: drag.vertices, turns: state.movePreview?.turns } });
        }
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        if (!sch || paste) return;
        // `SCH_SELECTION_TOOL`: a right-click on an item that is not selected selects it first, on empty space clears the selection; then the
        // menu for whatever is selected (kicad-port/schContextMenu.ts).
        const [wx, wy] = toWorld(e.clientX, e.clientY);
        // A right click on an ERC marker (a circle the last run left on the sheet) opens the marker's menu -- Exclude, Exclude all of this type, Change
        // severity, Show in the dialog -- instead of the item menu: an `SCH_MARKER` is an item of its own with its own entries.
        const found = state.erc?.violations ?? [];
        const markerIndex = nearestMarker(
          found.map((v) => (ercSeverityShown(v.severity, schControl.display) ? (ercMarkerPosition(v.location, sch)?.at ?? null) : null)),
          wx,
          wy,
          ERC_MARKER_HIT_UM
        );
        if (markerIndex >= 0) {
          const v = found[markerIndex]!;
          dispatch({ type: "SET_ERC_SELECTED", index: markerIndex });
          const spec = rcMenu({ domain: "erc", title: ercTitle(v.check), excluded: v.severity === "excluded", severity: ercSettingSeverity(v), pinMap: isPinMapCheck(v.check), onCanvas: true, comments: false });
          setContextMenu(null);
          setMarkerMenu({ x: e.clientX, y: e.clientY, entries: toMenuEntries(spec, (id) => void runMarkerMenu({ api, dispatch }, { domain: "erc", index: markerIndex }, id, askExclusionComment), ercKey(v) !== null) });
          return;
        }
        setMarkerMenu(null);
        // The menu opens at the cursor, which is where the actions it offers (Break, Slice, ...) read their position from.
        dispatch({ type: "SET_CURSOR", at: { x: wx, y: wy } });
        const scale = state.schematicView.scale || 1;
        const hit = pickable(hitField(sch, wx, wy, 3 / scale, measureStrokeText)) ?? pickable(hitSymbol(sch, wx, wy)) ?? hitItems(sch, wx, wy, 6 / scale).find((r) => r.kind !== "symbol" && r.kind !== "wire" && pickable(r.id))?.id ?? pickable(hitWire(sch, wx, wy, 400 / scale));
        let ids = [...state.selection];
        if (hit && !state.selection.has(hit)) ids = [hit];
        else if (!hit && ids.length > 0) ids = [];
        if (ids.length !== state.selection.size || ids.some((id) => !state.selection.has(id))) dispatch({ type: "SET_SELECTION", refs: ids });
        setContextMenu({ x: e.clientX, y: e.clientY, nodes: schContextMenu(summarizeSelection(sch, ids, [wx, wy], 10 / scale)) });
      }}
      onDoubleClick={(e) => {
        // A double-click on a hierarchical sheet enters it (`SCH_SELECTION_TOOL` -> `SCH_ACTIONS::enterSheet`); on any other item it opens its Properties
        // (`SCH_ACTIONS::properties`): the selection the first click made, else the item under the pointer.
        if (sch && state.activeTool === "select" && state.drawState == null) {
          const [wx, wy] = toWorld(e.clientX, e.clientY);
          const sheetId = hitSheet(sch.sheets, wx, wy);
          if (sheetId) {
            void api.navigateToSheet([...state.currentSheetPath, sheetId]);
            return;
          }
          const scale = state.schematicView.scale || 1;
          const under = pickable(hitField(sch, wx, wy, 3 / scale, measureStrokeText)) ?? pickable(hitSymbol(sch, wx, wy)) ?? hitItems(sch, wx, wy, 6 / scale).find((r) => r.kind !== "symbol" && r.kind !== "wire" && pickable(r.id))?.id ?? pickable(hitWire(sch, wx, wy, 400 / scale));
          const ids = state.selection.size > 0 ? [...state.selection] : under ? [under] : [];
          if (openSchProperties(sch, ids, dispatch)) return;
        }
        const draw = state.drawState;
        // `IsDblClick( BUT_LEFT )` in DrawShape / DrawRuleArea: finish the shape as it stands, at the point that was clicked.
        if (draw?.kind === "sch_shape" && sch) {
          const [wx, wy] = toWorld(e.clientX, e.clientY);
          finishShapeDraw({ tool: state.activeTool, draw, sch, lineMode: state.schLineMode, dispatch, api }, shapeSnap(wx, wy));
          return;
        }
        if (draw?.kind !== "wire") return;
        commitDrawn(draw.pts);
        dispatch({ type: "SET_DRAW_STATE", draw: null });
      }}
      onPointerUp={(e) => {
        gestureRef.current?.up();
        const drag = dragRef.current;
        dragRef.current = null;
        if (!drag) return;
        if (drag.kind === "move") {
          if (drag.moved && state.movePreview) {
            void api.commitSchHeld(state.movePreview);
          } else if (drag.moved) {
            dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
          } else {
            // No real movement: a plain click, not a drag -- apply
            // whatever selection effect onPointerDown deferred (see
            // pendingClickRef's own doc comment) for the "clicked one
            // member of a multi-selection" case; a fresh single-symbol
            // selection was already applied immediately on pointerDown,
            // so there's nothing left to do for that case here.
            dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
            if (pendingClickRef.current) dispatch({ type: "SET_SELECTION", refs: [pendingClickRef.current] });
          }
          pendingClickRef.current = null;
          return;
        }
        if (drag.kind === "box" && sch) {
          if (marquee) {
            const ctrlOrCmd = isMac() ? e.metaKey : e.ctrlKey;
            const modifiers = computeClickModifiers(e.shiftKey, ctrlOrCmd, e.altKey);
            const [x1, y1] = toWorld(e.clientX, e.clientY);
            const [x0, y0] = drag.startWorld;
            const box: [number, number, number, number] = [Math.min(x0, x1), Math.min(y0, y1), Math.max(x0, x1), Math.max(y0, y1)];
            const hits = boxItems(sch, box, marquee.crossing).filter((id) => pickable(id));
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
      {contextMenu && <SchContextMenu x={contextMenu.x} y={contextMenu.y} nodes={contextMenu.nodes} onClose={() => setContextMenu(null)} />}
      {markerMenu && <ContextMenu x={markerMenu.x} y={markerMenu.y} entries={markerMenu.entries} onClose={() => setMarkerMenu(null)} />}
    </div>
  );
}
