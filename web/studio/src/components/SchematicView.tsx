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
import { isTerminalPoint } from "../kicad-port/schTerminalPoint";
import { existingSingleClickItem } from "../kicad-port/schSingleClick";
import { arcMotion } from "../kicad-port/arcGeom";
import { drawStrokeText } from "./text/strokeFont";
import { ARC_DRAW_MODES } from "../kicad-port/arcGeom";
import { ARC_MODE_ACTIONS, CYCLE_ARC_DRAW_MODE, schArcDrawMode } from "../kicad-port/schArcDraw";
import { eeFindHandle, handlePos, POINT_SIZE_PX, type EeHandle } from "../kicad-port/eePointEdit";
import { schItemPointEditor } from "../kicad-port/schItemPointEdit";
import { paintEditPoints } from "./schematic/schArcEditPaint";
import { measureStrokeText } from "./text/strokeFont";
import { useEffect, useMemo, useRef, useState } from "react";
import type { Schematic } from "../api/types";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { fetchHierarchy } from "../api/schControlClient";
import { placeUnfoldLabel, unfoldEntrySize } from "../kicad-port/schBusUnfold";
import { boundsOfPoints, fitTransform } from "./canvas/view";
import { computeAutoPanDirection, computeAutoPanStep, DEFAULT_VIEW_CONTROL_SETTINGS, dragActionOf, dragZoomView, handleWheel, leftDragOutcome, motionPan } from "../kicad-port/viewControls";
import { wheelInputOf } from "../hooks/canvasWheel";
import { panByWorldDelta } from "../kicad-port/view";
import { useWheelPrefs } from "../actions/useWheelPrefs";
import { useNonPassiveWheel } from "../hooks/useNonPassiveWheel";
import { paintSchematic } from "./schematic/painter";
import { withVariant } from "../kicad-port/designVariants";
import { resolveLibSymbol, symbolBounds as libSymbolBounds } from "./schematic/libSymbol";
import { SchSnap, SCH_PLACING_TOOLS, toolCategory, type SchSnapMods } from "./schematic/schSnap";
import { useGridOverrides } from "../state/gridOverrides";
import { useGridSettings } from "../state/gridSettings";
import { heldCursor } from "../kicad-port/heldCursor";
import { paintSnapOverlay } from "./canvas/snapOverlayPainter";
import { layerColor } from "./canvas/layers";
import { drawPaper, drawSchematicDrawingSheet, pageOf } from "./schematic/drawingSheet";
import { schematicSheetPage } from "./drawingSheet/sheetInfo";
import { paintEditorGrid } from "./canvas/gridPaint";
import { useGridDisplay } from "../state/commonOptions";
import { gridMenuEntries, gridYOf } from "../kicad-port/gridSettings";
import { zoomListFor, zoomMenuEntries } from "../kicad-port/zoomFit";
import { formatLength } from "../state/units";
import { computeClickModifiers, applySingleClickModifier, isCrossingSelection, applyBoxSelectionModifiers, hasModifier } from "../kicad-port/selection";
import { isMac } from "../platform";
import { ClickDragGesture, dragRuleFor } from "../kicad-port/dragThreshold";
import { applyPatch, heldCmd, holdPoint, pickedVertices, previewBody, wirePick } from "../kicad-port/schMove";
import { withSchLockOverride } from "../kicad-port/schLock";
import { paintNetCollision } from "./schematic/netCollisionPainter";
import { paintOperatingPoints } from "./simulator/opPainter";
import { useSimUi } from "../state/simulator";
import { withOpVars } from "../kicad-port/sim/opVars";
import { hasNetCollision } from "../kicad-port/schNetCollision";
import { LINE_MODE_FREE } from "../kicad-port/schLineMode";
import { postMovePreview, postPlacePreview } from "../api/client";
import type { SchPastePreview } from "../kicad-port/schClipboard";
import { importContentsCmd, importNumbering } from "../kicad-port/schImportSheet";
import { adoptProjectAnnotation, rememberedAnnotateSettings } from "../kicad-port/schAnnotate";
import { droppedLeft } from "../kicad-port/sheetDrop";
import { placeNextDropped } from "../actions/schSheetFileActions";
import type { SchMovePatch } from "../api/schEditTypes";
import { wireTail } from "../kicad-port/schLineMode";
import { boxItems } from "./schematic/schItems";
import { pickItem } from "./schematic/schPick";
import { isPinItemId, promoteLonePin } from "../kicad-port/schPins";
import { autostartEvent, type Autostart, type AutostartTool } from "../kicad-port/schAutostart";
import { KICURSOR_CSS } from "../kicad-port/kiCursors";
import { schItemGrid, type SchItemKind } from "../kicad-port/gridOverrides";
import { openSchProperties } from "./schematic/schPropertiesOpen";
import { SchContextMenu } from "./schematic/SchContextMenu";
import { ContextMenu, type MenuEntry } from "./canvas/ContextMenu";
import { ercSeverityShown } from "./schematic/displayOptions";
import { ercMarkerPosition } from "./schematic/ercMarkerPosition";
import { ERC_MARKER_HIT_NM, nearestMarker } from "../kicad-port/markerHit";
import { ercKey, rcMenu } from "../kicad-port/rcItems";
import { ercMenuContext, runMarkerMenu, selectMarkerErc, showErcInDialog, toMenuEntries } from "../actions/checkerOps";
import { askExclusionComment } from "../state/checkerView";
import { summarizeSelection } from "./schematic/schSelectionSummary";
import { schContextMenu } from "../kicad-port/schContextMenu";
import { schSelectable } from "../kicad-port/schSelectionFilter";
import { finishShapeDraw, isSchShapeTool, paintShapePreview, shapeToolClick } from "./schematic/schShapeTools";
import { drawSchTablePreview } from "./schematic/schTablePaint";
import { schDrawTableGrid } from "../kicad-port/tables";
import { cellsBetween, cellUnder, shiftClickCells } from "../kicad-port/tableCells";
import { openDrawnTableDialog } from "../state/tableDialogs";
import { breakPreviewSheet, commitBreak } from "./schematic/schBreakTool";
import { paintPinPreview, placePinClick } from "./schematic/schPinTool";
import type { MenuNode } from "../kicad/types";
import { schematicBounds } from "./schematic/schHit";
import { isExplicitJunctionAllowed, junctionCandidates, type JunctionSchematic } from "../kicad-port/schJunction";
import { sheetSize } from "../kicad-port/schSheet";
import { importSheetClick } from "./schematic/schImportTool";
import { isStale } from "../kicad-port/checkRevision";
import { useSchControlState } from "../state/schControlStore";
import { hitSheet } from "../kicad-port/schControl";
import { netAtClick } from "../actions/schControlActions";
import type { Cmd } from "../api/types";
import { placeSchPaste } from "../actions/schClipboardActions";
import { boxSelectionWithGroups, groupsOf, leafIds, substituteSchSelection } from "../kicad-port/schGroups";
import { groupBounds, sheetBoxOf } from "./schematic/schGroupBounds";
import { isGroup } from "../kicad-port/groupTree";
import { picker } from "../actions/pcbPicker";
import { pickNewMemberAt } from "../actions/schGroupActions";
import { pasteOffset, previewIds, withPaste } from "../kicad-port/schClipboard";
import { cancelSchPaste, useSchPaste } from "../state/schPasteStore";
import { useSchImageHand } from "../state/schImageHand";
import { cancelSchCarry, carryCursor, carryDrawing, noteCarryCursor, noteCarryPainted, useSchCarry } from "../state/schCarryStore";
import { isCarryTool, PREVIEW_AT } from "../kicad-port/schPlaceTool";
import { beginItem, carrySymbol, placeCarried } from "../actions/schPlaceActions";
import { imageSizeNm, SCALE_PPM } from "../kicad-port/refImage";
import { useImageVersion } from "./canvas/imageCache";
import { paintingIn, useColorThemeStamp } from "../state/colorThemeStore";
import "../styles/canvas.css";

/** `m_DefaultTextSize`: the glyph size of a sheet's text until Schematic Setup says another (50 mil), micrometres; Draw Table sizes its columns and rows by it. */
const DEFAULT_TEXT_SIZE_NM = 1_270_000;


type DragState =
  /** `WX_VIEW_CONTROLS` DRAG_PANNING by the middle (1) or right (2) button, as `input.drag_middle` / `drag_right` say; `moved`: the press has become a drag (a right press that never did opens the context menu). */
  | { kind: "pan"; button: 1 | 2; startScreen: [number, number]; startView: [number, number]; moved: boolean }
  /** DRAG_ZOOMING: the view is scaled about the press point (`anchor`, canvas pixels) as the pointer goes up and down (`lastY` is where it was at the last motion). */
  | { kind: "zoom"; button: 1 | 2; anchor: { x: number; y: number }; lastY: number; moved: boolean }
  | { kind: "box"; startWorld: [number, number]; startScreen: [number, number] }
  /** `selectTableCells`: a drag from a cell of `table` selects the cells under it (`SelectCellsBetween`), from the selection it began with (`before`). */
  | { kind: "cells"; table: string; startWorld: [number, number]; before: string[]; modifiers: ReturnType<typeof computeClickModifiers>; moved: boolean }
  /** `SCH_POINT_EDITOR::Main`'s drag of one edit point of the selected graphic (kicad-port/schPointEdit.ts). */
  | { kind: "editpoint"; id: string; handle: EeHandle }
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
  | { kind: "move"; refs: string[]; startWorld: [number, number]; /** `BestDragOrigin`: the point of the held items the drag holds them by. */ origin: [number, number]; moved: boolean; vertices?: Record<string, number[]>; /** What a click (no drag) on the item starts instead of selecting it (`autostartEvent`). */ autostart?: Autostart };

/**
 * Every pin's resolved world-space tip, for the junction analysis (`JUNCTION_HELPERS::AnalyzePoint`) and the wire tool's auto-finish on a pin. Only symbols with real
 * resolved `lib_symbols` graphics contribute a pin -- a symbol still drawn as the generic box (no `lib_id` resolved) has no world-space pin geometry computed on this side
 * yet (`layout.ts`'s box layout is local-space only). Where a click goes is the grid helper's (components/schematic/schSnap.ts), which has the same pins as anchors.
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

function nearestSnapPoint(pts: Array<[number, number]>, xNm: number, yNm: number, thresholdNm: number): [number, number] | null {
  let best: { p: [number, number]; d: number } | null = null;
  for (const p of pts) {
    const d = Math.hypot(p[0] - xNm, p[1] - yNm);
    if (d <= thresholdNm && (!best || d < best.d)) best = { p, d };
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
  // the simulator's operating point is drawn on the sheet: a new one redraws it
  const simUi = useSimUi();
  const simVersion = simUi.version;
  /** The paste being carried (`SCH_EDITOR_CONTROL::Paste` -> the Move tool): the items it would add, until a click drops them or Escape throws them away. */
  const paste = useSchPaste();
  /** The label, text, directive label or power symbol a placing tool holds (state/schCarryStore.ts): it rides the cursor until a click places it. */
  const carry = useSchCarry();
  /** The image Place Image holds (state/schImageHand.ts), and the number of bitmaps that have arrived: the sheet is painted again for either. */
  const imageHand = useSchImageHand();
  const imageVersion = useImageVersion();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  // r6 C6-L1: `SCH_EDIT_FRAME::UpdateHierarchyNavigator` runs `SCH_NAVIGATE_TOOL::CleanHistory` when the hierarchy changes, so Back / Forward
  // never step onto a sheet that was deleted. The list is read again on every design change; the clean runs when its sheet paths differ from the
  // last ones read. (The history's `ResetHistory` on load is the store's initial `[[root]]`.)
  const hierarchyKeyRef = useRef<string | null>(null);
  useEffect(() => {
    let live = true;
    fetchHierarchy().then(
      (list) => {
        if (!live) return;
        const key = list.map((e) => e.path.join("/")).sort().join("\n");
        if (hierarchyKeyRef.current !== null && hierarchyKeyRef.current !== key) dispatch({ type: "CLEAN_SCH_NAV", sheets: list.map((e) => e.path) });
        hierarchyKeyRef.current = key;
      },
      () => {}
    );
    return () => {
      live = false;
    };
  }, [state.version, dispatch]);
  const dragRef = useRef<DragState | null>(null);
  /** Set when a right-button drag that panned or zoomed the view has just ended, so the `contextmenu` event the browser sends after it does not open the menu (a click does). */
  const justPannedRef = useRef(false);
  /** `WX_VIEW_CONTROLS::handleAutoPanning`: the last pointer position (canvas pixels and client), and the sheet and what is going on, as the animation frame reads them. */
  const lastPointerRef = useRef<{ x: number; y: number; clientX: number; clientY: number } | null>(null);
  /** Set by an auto-pan step: the pointer has not moved but the sheet under it has, so the motion is replayed once the view is redrawn (`WX_VIEW_CONTROLS::refreshMouse`). */
  const autoPannedRef = useRef(false);
  /** The pressed button's `BUTTON_STATE` (tool_dispatcher.cpp): whether the press has become a drag. Made at every press. */
  const gestureRef = useRef<ClickDragGesture | null>(null);
  const userMovedRef = useRef(false);
  /** Previous wire-preview elbow, for computeBreakPoint's "maintain current line shape" hint (kicad-port/schLineMode.ts). */
  const elbowRef = useRef<{ mid: readonly [number, number]; end: readonly [number, number] } | null>(null);
  /** The plain-click/drag symbol-drag distinguisher's deferred click (see DragState's own "move" kind doc) -- set only for a click that keeps a multi-selection's whole group for a potential drag, same as Canvas.tsx's own `pendingClickRef`. */
  const pendingClickRef = useRef<string | null>(null);
  /** A press on an item whose drag draws the selection area ("Left button drag:", `leftDragOutcome`): what the press does as a click when it is released without a drag. */
  const areaClickRef = useRef<(() => void) | null>(null);
  /** `SCH_SELECTION_TOOL::m_previous_first_cell`: the cell a Shift+click range of cells starts from (UI-LEFT-2 SS18). */
  const previousFirstCellRef = useRef<string | null>(null);
  /** `m_metaPanStart` of "Pan on mouse movement with key:" (null while the key is up). */
  const motionPanRef = useRef<{ x: number; y: number } | null>(null);
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 });
  const [marquee, setMarquee] = useState<{ x0: number; y0: number; x1: number; y1: number; crossing: boolean } | null>(null);
  /** The tool a click under the pointer would start (`autostartEvent` on motion: `displayWireCursor`, `displayBusCursor`), or null. */
  const [autostartHover, setAutostartHover] = useState<AutostartTool | null>(null);
  /** The right-click menu (`SCH_SELECTION_TOOL`'s context menu): where it is and what it offers for the selection. */
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; nodes: MenuNode[] } | null>(null);
  /** The menu of an ERC marker the last right click was on (kicad-port/rcItems.ts `rcMenu`). */
  const [markerMenu, setMarkerMenu] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  const sch = state.schematic;
  /** What the auto-pan frame loop below reads each frame, kept fresh by every render so the loop is made once. */
  const live = useRef({ state, paste, size: containerSize, marquee });
  live.current = { state, paste, size: containerSize, marquee };
  /** `SCH_SELECTION_TOOL::itemPassesFilter`: an item of a category the selection filter has off, or a locked one without its "Locked items", cannot be picked. */
  const selectable = sch ? schSelectable(sch, state.schSelectionFilter) : () => true;
  const pickable = (id: string | null): string | null => (id && selectable(id) ? id : null);
  const moveMode = state.activeTool === "move";
  const dragMode = state.activeTool === "drag";
  /** `M` and `G`: which `sch_move` verb a committed preview becomes (a plain click-drag is a `sch_drag`, sch_selection_tool.cpp's own default for a plain drag). */
  const armedKind: "sch_move" | "sch_drag" | null = moveMode ? "sch_move" : dragMode ? "sch_drag" : null;
  /** The point editor's drag in progress: the graphic, the handle and where the snapped cursor has it (UI-LEFT S13, UI-LEFT-2 SS3). */
  const [pointDrag, setPointDrag] = useState<{ id: string; handle: EeHandle; to: [number, number] } | null>(null);
  /**
   * `SCH_POINT_EDITOR` on a selection of one item it has a behaviour for -- a drawn graphic (a rectangle, circle, arc, bezier, polyline, text box or rule area;
   * kicad-port/schPointEdit.ts), a sheet, an image, a graphic line or a table cell (kicad-port/schItemPointEdit.ts, UI-LEFT-3 S3-2 / S3-4) -- with the select tool
   * and nothing held: its edit points and lines, the sheet as the drag has it and what the drop sends. A locked item (a cell of a locked table) has none.
   */
  const shapeEditor = useMemo(() => {
    if (!sch || state.activeTool !== "select" || armedKind || state.selection.size !== 1) return null;
    const id = [...state.selection][0]!;
    const locked = sch.locked ?? [];
    if (locked.includes(id) || (sch.tables ?? []).some((t) => id.startsWith(`${t.id}:`) && locked.includes(t.id))) return null;
    return schItemPointEditor(sch, id, pointDrag && pointDrag.id === id ? pointDrag : null, state.prefs.schArcEditMode, measureStrokeText);
  }, [sch, state.activeTool, armedKind, state.selection, pointDrag, state.prefs.schArcEditMode]);

  // The grid helper of the tool in force (kicad-port/schGridHelper.ts, schematic/schSnap.ts), fed with the sheet, the view, the grid list and its overrides.
  const snapRef = useRef<SchSnap | null>(null);
  if (!snapRef.current) snapRef.current = new SchSnap();
  const snap = snapRef.current;
  const gridSettingsList = useGridSettings("schematic");
  const gridList = gridSettingsList.grids;
  const gridOverrides = useGridOverrides("schematic");
  const gridNm = state.schGridNm;
  const gridDisplay = useGridDisplay("schematic");
  snap.update({ sch, scale: state.schematicView.scale, gridNm, grids: gridList, gridYs: gridSettingsList.ys, overrides: gridOverrides });
  // A tool of its own starts a helper of its own: the anchor snapped to and the snap line go with the last one.
  useEffect(() => snap.reset(), [snap, state.activeTool]);
  /** The modifiers of the last pointer event (Ctrl, Cmd on a Mac, turns the grid off; Shift the anchors): the click handlers below snap through `snapToGrid` without an event. */
  const modsRef = useRef<SchSnapMods>({ ctrl: false, shift: false });
  const NO_MODS: SchSnapMods = { ctrl: false, shift: false };

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
    const origin = state.moveOriginNm;
    const cmd = held
      ? heldCmd(
          { mode: held.kind === "sch_drag" ? "drag" : "move", ids: held.refs, vertices: held.vertices, dxNm: held.dxNm, dyNm: held.dyNm, turns: held.turns, holdNm: held.holdNm ?? holdPoint(origin, held.dxNm, held.dyNm) },
          { ortho: state.schLineMode !== LINE_MODE_FREE }
        )
      : null;
    if (!cmd) {
      pump.want = null;
      pump.sent = null;
      setPatch(null);
      return;
    }
    // the preview runs as the drop will (`withSchLockOverride`: locked items held with "Override locks" checked)
    pump.want = JSON.stringify(previewBody(withSchLockOverride(cmd), state.currentSheetPath));
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
  }, [state.movePreview, state.moveOriginNm, state.schLineMode, state.currentSheetPath]);
  useEffect(() => {
    const pump = previewPump.current;
    return () => {
      pump.alive = false;
    };
  }, []);
  // A Break / Slice in progress shows the cut lines as their pieces, the new end at the (grid-snapped) cursor (kicad-port/schBreak.ts).
  const breaking = state.drawState?.kind === "sch_shape" ? state.drawState.brk : undefined;
  const breakSnap = breaking && state.cursorNm ? (snap.lastPoint() ?? snap.align([state.cursorNm.x, state.cursorNm.y], NO_MODS, "wires")) : null;
  const breakCursor: [number, number] | null = breakSnap ? [breakSnap[0], breakSnap[1]] : null;
  const baseDisplaySch: Schematic | null = sch && held && patch ? applyPatch(sch, patch) : sch && breaking && breakCursor ? breakPreviewSheet(sch, breaking, breakCursor) : sch;
  // A paste in progress: what it would add, shifted so its anchor sits on the grid-snapped cursor (`SCH_MOVE_TOOL::Main`'s `delta = m_cursor - *m_anchorPos`) and drawn
  // selected, as KiCad selects the pasted items. The sheet itself is untouched until the click (kicad-port/schClipboard.ts).
  const pasteSnap = paste && state.cursorNm ? snap.align([state.cursorNm.x, state.cursorNm.y], NO_MODS, paste.place ? "graphics" : "connectable") : null;
  const pasteDx = paste && pasteSnap ? pasteOffset(paste.anchor, [pasteSnap[0], pasteSnap[1]])[0] : 0;
  const pasteDy = paste && pasteSnap ? pasteOffset(paste.anchor, [pasteSnap[0], pasteSnap[1]])[1] : 0;
  const pasteShown = paste != null && pasteSnap != null;
  // The item a placing tool holds (`TwoClickPlace`'s `m_view->AddToPreview( item )`, `PlaceSymbol`'s `addSymbol`): drawn by the page at `PREVIEW_AT` from the item and
  // its library drawing, with what the server's drawing of the click's command adds once it is back (kicad-port/schCarryPreview.ts), shifted to the cursor as the
  // tool's grid snaps it (`BestSnapAnchor`), and drawn selected (it is the selection while it rides). Until the pointer moves it is where it was made.
  const carrying = carry != null && carry.tool === state.activeTool && carry.sheet.join("/") === state.currentSheetPath.join("/");
  const carryShown = useMemo(() => (carrying && carry && sch ? carryDrawing(carry, sch) : null), [carrying, carry, sch?.lib_symbols]);
  const carrySnap = carrying ? (state.cursorNm ? (snap.lastPoint() ?? snap.align([state.cursorNm.x, state.cursorNm.y], NO_MODS, toolCategory(state.activeTool))) : carry.at) : null;
  const carryDx = carrySnap ? carrySnap[0] - PREVIEW_AT[0] : 0;
  const carryDy = carrySnap ? carrySnap[1] - PREVIEW_AT[1] : 0;
  // Import Sheet's contents in the hand (`placeSheetContents`: the file's items are read, moved to the cursor and handed to the move tool; UI-LEFT S17): the server's
  // drawing of the very `import_sheet` the click sends, made at `PREVIEW_AT` (the command puts the contents' anchor there), shifted to the grid-snapped cursor
  // and drawn selected. A sheet (Place as sheet) is drawn by its two clicks instead.
  const importPlan = state.activeTool === "sch_import_sheet" && state.schSheetImport && !state.schSheetImport.asSheet ? state.schSheetImport : null;
  const [importGhost, setImportGhost] = useState<{ key: string; preview: SchPastePreview } | null>(null);
  const importKey = importPlan ? JSON.stringify([importPlan, state.currentSheetPath]) : null;
  useEffect(() => {
    if (!importPlan || !importKey) return setImportGhost(null);
    let alive = true;
    void postPlacePreview({ sheet: state.currentSheetPath.join("/"), cmd: importContentsCmd(importPlan, [PREVIEW_AT[0], PREVIEW_AT[1]], importNumbering(rememberedAnnotateSettings())) }).then((preview) => {
      if (alive) setImportGhost(preview ? { key: importKey, preview } : null);
    });
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [importKey, state.version]);
  // Schematic Setup > Annotation, as the schematic has it now: what placing, pasting and Import Sheet number by (`PlaceSymbol`, `Paste` and `ImportSheet` read
  // `SCHEMATIC_SETTINGS::m_Annotate*`; UI-LEFT S14)
  const annotationKey = JSON.stringify(sch?.annotation ?? null);
  useEffect(() => {
    if (sch) adoptProjectAnnotation(sch.annotation);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [annotationKey, !!sch]);
  // A drop of several schematics places each in turn (`DoWithAcceptedFiles`): when one file's placement ends -- placed, or dropped with Esc -- the next is in the hand
  const hadImport = useRef(false);
  useEffect(() => {
    const has = state.schSheetImport != null;
    if (hadImport.current && !has && droppedLeft() > 0) placeNextDropped({ dispatch });
    hadImport.current = has;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.schSheetImport]);
  const importShown = importPlan && importGhost && importGhost.key === importKey && state.cursorNm ? importGhost.preview : null;
  const importSnap = importShown && state.cursorNm ? snap.align([state.cursorNm.x, state.cursorNm.y], NO_MODS, "connectable") : null;
  const displaySch: Schematic | null = useMemo(() => {
    if (baseDisplaySch && paste && pasteShown) return withPaste(baseDisplaySch, paste.preview, pasteDx, pasteDy);
    if (baseDisplaySch && carryShown) return withPaste(baseDisplaySch, carryShown, carryDx, carryDy);
    if (baseDisplaySch && importShown && importSnap) return withPaste(baseDisplaySch, importShown, importSnap[0] - PREVIEW_AT[0], importSnap[1] - PREVIEW_AT[1]);
    return baseDisplaySch;
  }, [baseDisplaySch, paste, pasteShown, pasteDx, pasteDy, carryShown, carryDx, carryDy, importShown, importSnap?.[0], importSnap?.[1]]);
  const pasteSelection = useMemo(
    () => (paste && pasteShown ? new Set(previewIds(paste.preview)) : carryShown ? new Set(previewIds(carryShown)) : importShown ? new Set(previewIds(importShown)) : null),
    [paste, pasteShown, carryShown, importShown]
  );
  // A paste belongs to the sheet it was started on and to the select tool: leaving the sheet, the tab or the tool throws it away.
  useEffect(() => {
    if (paste && (paste.sheet.join("/") !== state.currentSheetPath.join("/") || state.activeTool !== "select")) cancelSchPaste();
  }, [paste, state.currentSheetPath, state.activeTool]);
  useEffect(() => () => void cancelSchPaste(), []);
  // An item in the hand belongs to its tool and its sheet: leaving either (or the tab) throws it away, as `cleanup()` does when the tool ends.
  useEffect(() => {
    if (carry && (carry.tool !== state.activeTool || carry.sheet.join("/") !== state.currentSheetPath.join("/"))) cancelSchCarry();
  }, [carry, state.activeTool, state.currentSheetPath]);
  useEffect(() => () => void cancelSchCarry(), []);
  // A symbol an action hands Place Symbol (`PLACE_SYMBOL_PARAMS{ symbol, .. }`: Place Next Symbol Unit, the Symbol Editor's Add Symbol to Schematic) rides at once
  // (`if( symbol ) { addSymbol( symbol ); .. }`), from the cursor, and the tool ends once it is placed (`placeOneOnly`).
  useEffect(() => {
    const armed = state.armedSymbol;
    if (state.activeTool !== "sch_place_symbol" || !armed) return;
    dispatch({ type: "SET_ARMED_SYMBOL", symbol: null });
    // `WarpMouseCursor( GetMousePosition( false ) )`: where the pointer is
    const at: [number, number] = state.cursorNm ? [state.cursorNm.x, state.cursorNm.y] : (carryCursor() ?? [0, 0]);
    carrySymbol({ api }, { ...armed, oneOnly: true }, at, false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.activeTool, state.armedSymbol]);

  // the schematic editor's colour theme (Preferences > Schematic Editor > Colors): the sheet is painted in it, and again when it changes
  const themeStamp = useColorThemeStamp();
  useEffect(() => paintingIn("schematic", () => {
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
    drawPaper(ctx, page);
    // `GAL::DrawGrid` over the whole canvas (eeschema clips it to nothing), in the Schematic Editor's Display Options > Grid Display style
    if (state.gridVisible) paintEditorGrid(ctx, state.schematicView, width, height, { size: { x: gridNm, y: gridYOf(gridSettingsList, gridNm) }, origin: [0, 0], style: gridDisplay.style, lineWidthPx: gridDisplay.lineWidthPx, minSpacingPx: gridDisplay.minSpacingPx }, layerColor("LAYER_SCHEMATIC_GRID"), undefined, "schematic");
    // The drawing sheet (`SCH_VIEW::DisplaySheet`): the `.kicad_wks` Page Settings chose, else KiCad's default, on this sheet's page
    drawSchematicDrawingSheet(ctx, state.schematicView, schematicSheetPage(sch, state.board?.name), sch.drawing_sheet?.text);
    // `SCH_DRAG_NET_COLLISION_MONITOR`: where the held selection would short two nets or pull a connection apart (the server sends it with the preview)
    const collisionOverlay = held && patch ? patch.net_collision : undefined;
    // `${OP}` in a symbol's field or a label: the last operating point's values (kicad-port/sim/opVars.ts), resolved in what is drawn.
    // the current design variant's flags and field texts (KiCad 25ec2335 `SCH_EDIT_FRAME::SetCurrentVariant` -> `HardRedraw`: the painter reads every symbol through it)
    // the item being edited is drawn as its points have it so far, with what follows it (the point editor changes the items as it goes)
    const shownSch = shapeEditor && pointDrag ? shapeEditor.shown(displaySch) : displaySch;
    paintSchematic(ctx, state.schematicView, withOpVars(withVariant(shownSch, state.schVariant), simUi.frame.circuit, simUi.opPoints), { selection: pasteSelection ?? state.selection, netHighlight: state.netHighlight, ercViolations: state.erc?.violations ?? null, ercStale: isStale(state.ercVersion, state.version), ercSelected: state.ercSelected, lintViolations: state.ercDialogOpen ? (state.lint?.schematic.violations ?? null) : null, lintSelected: state.ercLintSelected, display: schControl.display, enteredGroup: state.schEnteredGroupId });
    // the carried item is on the canvas, in this state of it (the test hook's timing: no round trip before it shows)
    if (carryShown && carry) noteCarryPainted(carry.seq);
    if (collisionOverlay) paintNetCollision(ctx, state.schematicView.scale || 1, collisionOverlay);
    if (shapeEditor) paintEditPoints(ctx, state.schematicView, shapeEditor.edit);
    // The simulator's operating point (View > Show OP Voltages / Currents, components/simulator/opPainter.ts).
    paintOperatingPoints(ctx, state.schematicView.scale || 1, displaySch, layerColor("LAYER_SCHEMATIC_BACKGROUND"));
    // The image in the hand, centred on the grid-snapped cursor (`GRID_GRAPHICS`), outlined as a selected image is.
    if (imageHand && state.cursorNm) {
      const [cx, cy] = snap.align([state.cursorNm.x, state.cursorNm.y], NO_MODS, "graphics");
      const [w, h] = imageSizeNm(imageHand.px, imageHand.ppi, SCALE_PPM);
      const x0 = cx - Math.trunc(w / 2);
      const y0 = cy - Math.trunc(h / 2);
      ctx.drawImage(imageHand.img, x0, y0, w, h);
      ctx.strokeStyle = layerColor("LAYER_SELECTION_SHADOWS");
      ctx.lineWidth = Math.max(150_000, (1 / state.schematicView.scale) * 2);
      ctx.strokeRect(x0, y0, w, h);
    }
    if (state.drawState?.kind === "wire") {
      // sch_line_wire_bus_tool.cpp doDrawSegments + computeBreakPoint: the
      // rubber band from the last click to the cursor is two segments (an
      // elbow) in LINE_MODE_90/45, one straight segment in LINE_MODE_FREE.
      let pts = state.drawState.pts;
      // `C` before its first click: the entry leans toward the cursor and the wire starts at its end (r6 C6-M2, `doDrawSegments`' motion branch)
      let unfoldSize = state.busUnfold?.size;
      if (state.cursorNm) {
        const c = snap.lastPoint() ?? snap.align([state.cursorNm.x, state.cursorNm.y], NO_MODS, state.activeTool === "sch_line" ? "graphics" : "wires");
        const u = state.busUnfold;
        if (u && !u.labelAt && pts.length === 1) {
          unfoldSize = unfoldEntrySize(u.entryAt, [c[0], c[1]], u.size);
          pts = [[u.entryAt[0] + unfoldSize[0], u.entryAt[1] + unfoldSize[1]]];
        }
        const tail = wireTail(pts[pts.length - 1]!, [c[0], c[1]], state.schLineMode, state.schPosture, elbowRef.current);
        elbowRef.current = tail.length === 2 ? { mid: tail[0]!, end: tail[1]! } : null;
        pts = [...pts, ...(tail as [number, number][])];
      }
      // GAPS.md #20: the bus tool shares this exact preview (same
      // `DrawState` kind), colored to match whichever is actually armed.
      ctx.strokeStyle = layerColor(state.activeTool === "bus" ? "LAYER_BUS" : state.activeTool === "sch_line" ? "LAYER_NOTES" : "LAYER_WIRE");
      ctx.lineWidth = Math.max(150_000, (1 / state.schematicView.scale) * 1.5);
      // drawn from a wire's end: the segments are duplicates of it, its colour and width
      const likeStroke = state.drawState.like ? sch?.wires.find((w) => w.id === (state.drawState as { like?: string }).like)?.stroke : undefined;
      if (likeStroke?.color && likeStroke.color.a > 0) ctx.strokeStyle = `rgba(${likeStroke.color.r},${likeStroke.color.g},${likeStroke.color.b},${likeStroke.color.a / 255})`;
      if (likeStroke?.width_nm) ctx.lineWidth = Math.max(likeStroke.width_nm, ctx.lineWidth);
      ctx.beginPath();
      pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.stroke();
      // `C` (Unfold from Bus): the bus entry already sits at its root and the wire runs from its far end -- show the stub too.
      const unfold = state.busUnfold;
      if (unfold) {
        const size = unfoldSize ?? unfold.size;
        ctx.strokeStyle = layerColor("LAYER_BUS");
        ctx.beginPath();
        ctx.moveTo(unfold.entryAt[0], unfold.entryAt[1]);
        ctx.lineTo(unfold.entryAt[0] + size[0], unfold.entryAt[1] + size[1]);
        ctx.stroke();
        // the member's label rides the cursor until the first click puts it down (`m_busUnfold.label->SetPosition( cursorPos )`, its preview; UI-LEFT S19, r6),
        // then stays where it was put: a local label's text on its baseline, running from the anchor the way its spin says (R and Shift+R turn it, UI-LEFT-2 SS18)
        const labelAt = unfold.labelAt ?? (state.cursorNm ? snapToGrid(state.cursorNm.x, state.cursorNm.y) : null);
        if (labelAt) {
          const [lx, ly] = labelAt;
          const sizeNm = 1_270_000;
          const o = Math.round(sizeNm * 0.15);
          const spin = unfold.spin ?? "right";
          const vertical = spin === "up" || spin === "bottom";
          const [dx, dy] = spin === "right" ? [o, -o] : spin === "left" ? [-o, -o] : spin === "up" ? [-o, -o] : [-o, o];
          drawStrokeText(ctx, unfold.net, lx + dx, ly + dy, { sizeNm, justify: spin === "right" || spin === "up" ? "left" : "right", angleRad: vertical ? -Math.PI / 2 : 0, color: layerColor("LAYER_LOCLABEL"), thicknessNm: Math.round(sizeNm / 8) });
        }
      }
    }
    // The shape or rule area being drawn (kicad-port/schShapeEdit.ts, polygonGeom.ts), rubber-banded to the cursor.
    if (state.drawState?.kind === "sch_shape" && state.cursorNm) {
      paintShapePreview(ctx, state.schematicView, state.drawState, shapeSnap(state.cursorNm.x, state.cursorNm.y, true), state.schLineMode, state.units);
      // The sheet pin the next click would drop.
      if (state.drawState.pin) paintPinPreview(ctx, state.schematicView, sch, state.drawState.pin, shapeSnap(state.cursorNm.x, state.cursorNm.y, true));
    }
    // `SCH_DRAWING_TOOLS::DrawTable`: the grid the cursor sizes, from the first corner.
    if (state.drawState?.kind === "table" && state.cursorNm) {
      const origin = state.drawState.origin;
      drawSchTablePreview(ctx, state.schematicView, origin, schDrawTableGrid(origin, shapeSnap(state.cursorNm.x, state.cursorNm.y, true), DEFAULT_TEXT_SIZE_NM, snap.gridSize("graphics")));
    }
    // `S`: the sheet being sized, from its first corner to the (grid-snapped) cursor.
    if (state.drawState?.kind === "sheet" && state.cursorNm) {
      const start = state.drawState.start;
      const c = snap.lastPoint() ?? snap.align([state.cursorNm.x, state.cursorNm.y], NO_MODS, "connectable");
      const [w, h] = sheetSize(start, [c[0], c[1]], snap.gridSize("connectable"));
      ctx.strokeStyle = layerColor("LAYER_SHEET");
      ctx.lineWidth = Math.max(150_000, (1 / state.schematicView.scale) * 1.5);
      ctx.setLineDash([4 / state.schematicView.scale, 3 / state.schematicView.scale]);
      ctx.strokeRect(start[0], start[1], w, h);
      ctx.setLineDash([]);
    }
    // What the grid helper shows: the snap marker and the snap line (`SNAP_INDICATOR`, `CONSTRUCTION_GEOM`).
    const snapOverlay = snap.overlay();
    if (snapOverlay) {
      const k = state.schematicView.scale || 1;
      paintSnapOverlay(ctx, k, [-state.schematicView.x / k, -state.schematicView.y / k, (width - state.schematicView.x) / k, (height - state.schematicView.y) / k], snapOverlay, { marker: "#001966", construction: layerColor("LAYER_SCHEMATIC_ANCHOR"), guideActive: layerColor("LAYER_SCHEMATIC_ANCHOR"), guides: false });
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
  }), [sch, displaySch, shapeEditor, pointDrag, gridDisplay, gridNm, gridSettingsList, state.gridVisible, state.schVariant, held, patch, pasteSelection, state.schLineMode, state.schPosture, state.schematicView, state.selection, state.netHighlight, containerSize, state.board?.name, marquee, state.drawState, state.cursorNm, state.erc, state.ercVersion, state.version, state.ercSelected, state.ercDialogOpen, state.lint, state.ercLintSelected, state.activeTool, schControl.display, state.schEnteredGroupId, imageHand, imageVersion, simVersion, themeStamp]);

  const empty = state.schematicError ?? (!sch ? "Loading schematic…" : null);

  const toWorld = (clientX: number, clientY: number): [number, number] => {
    const rect = containerRef.current!.getBoundingClientRect();
    const sx = clientX - rect.left,
      sy = clientY - rect.top;
    return [(sx - state.schematicView.x) / state.schematicView.scale, (sy - state.schematicView.y) / state.schematicView.scale];
  };

  /**
   * Where a click of the tool in force goes: `EE_GRID_HELPER::BestSnapAnchor` on the tool's grid (kicad-port/schGridHelper.ts) -- the grid of its category (connected items and
   * wires 50 mil, text 10 mil, ... under the grid overrides), or an anchor (a pin, a wire end, a label) with the grid off. Ctrl and Shift are the last event's.
   */
  const snapToGrid = (xNm: number, yNm: number): [number, number] => {
    const p = snap.point(state.activeTool, [xNm, yNm], modsRef.current, toolCategory(state.activeTool));
    return [p[0], p[1]];
  };

  /**
   * `SCH_SELECTION_TOOL::autostartEvent` (kicad-port/schAutostart.ts, UI-LEFT-3 S3-1): what a click of the selection tool on the item `id` at the pointer starts -- a wire or a
   * bus from an unconnected pin, label, sheet pin, bus entry or wire end -- when Preferences > Schematic Editor > Editing Options has "Automatically start wires on
   * unconnected pins" on; the point is `BestSnapAnchor( pos, GetItemGrid( item ) )`.
   */
  const autostartAt = (id: string | null, xNm: number, yNm: number): Autostart | null => {
    // (the selection tool's own: not while a picker -- the simulator's probes, Group Properties' pick -- or a paste holds the pointer)
    if (!sch || !id || !state.prefs.schAutoStartWires || state.activeTool !== "select" || picker.session() || paste) return null;
    const wire = sch.wires.find((w) => w.id === id);
    const kind: SchItemKind = isPinItemId(id) ? "pin" : wire ? (wire.bus ? "bus" : "wire") : sch.bus_entries.some((b) => b.id === id) ? "bus_entry" : sch.labels.some((l) => l.id === id) ? "label" : sch.sheets.some((sh) => (sh.pins ?? []).some((p) => p.id === id)) ? "sheet_pin" : "symbol";
    const pos = snap.point("select", [xNm, yNm], modsRef.current, schItemGrid(kind));
    return autostartEvent(sch, id, [pos[0], pos[1]], { showHiddenPins: schControl.display.showHiddenPins });
  };
  /** `m_toolMgr->ProcessEvent( *autostart )`: the selection is cleared (`DrawSegments`' `selectionClear`) and the wire or bus tool starts at the point, to give way to the
   *  selection tool once the wire is drawn (`quitOnDraw`). */
  const startAutostart = (a: Autostart) => {
    dispatch({ type: "SET_SELECTION", refs: [] });
    dispatch({ type: "SET_SCH_WIRE_PICK", pick: null });
    dispatch({ type: "SET_ACTIVE_TOOL", tool: a.tool });
    dispatch({ type: "SET_DRAW_STATE", draw: { kind: "wire", pts: [a.at], quitOnDraw: true, ...(a.like ? { like: a.like } : {}) } });
    setAutostartHover(null);
  };

  /**
   * The point a shape tool takes for the cursor: the grid of its category (`GRID_GRAPHICS`, `GRID_CONNECTABLE` for a rule area) and the anchors of the grid helper.
   * `live` is the render path: the point the last pointer event put the cursor at, without running the helper again.
   */
  const shapeSnap = (xNm: number, yNm: number, live = false): [number, number] => {
    if (live) {
      const last = snap.lastPoint();
      if (last) return [last[0], last[1]];
      const p = snap.align([xNm, yNm], NO_MODS, toolCategory(state.activeTool));
      return [p[0], p[1]];
    }
    return snapToGrid(xNm, yNm);
  };

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
    // a wire started from a wire's end has that wire's stroke (`startSegments`' `Duplicate`): `add_wire_like` when the wire has one of its own
    const like = state.drawState?.kind === "wire" ? state.drawState.like : undefined;
    const likeStroke = like ? sch?.wires.find((w) => w.id === like)?.stroke : undefined;
    const wire: Cmd = likeStroke ? ({ op: "sch_edit", verb: "add_wire_like", pts: points, bus: state.activeTool === "bus", like: like! } as Cmd) : { op: "add_wire", pts: points, bus: state.activeTool === "bus" };
    const unfold = state.busUnfold;
    if (!unfold) {
      void api.cmd(wire);
      return;
    }
    // the label where the first click was (`m_busUnfold.label_placed`), the wire's second point (r6 C6-M2)
    const at = unfold.labelAt ? { x: unfold.labelAt[0], y: unfold.labelAt[1] } : points[1]!;
    void api.cmdBatch([
      { op: "add_bus_entry", at: { x: unfold.entryAt[0], y: unfold.entryAt[1] }, size: { x: unfold.size[0], y: unfold.size[1] } },
      wire,
      // the label with the spin R gave it while unfolding (`m_busUnfold.label`, `SetSpinStyle`; UI-LEFT-2 SS18)
      { op: "sch_edit", verb: "place_label", net: unfold.net, at, kind: { scope: "local" }, spin: unfold.spin ?? "right" } as Cmd,
    ]);
    // `UnfoldBus` goes on drawing wires (`doDrawSegments( aEvent, commit, LAYER_WIRE, false )`): the wire tool stays armed
    dispatch({ type: "SET_BUS_UNFOLD", unfold: null });
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
    const result = handleWheel(state.schematicView, { width: rect.width, height: rect.height }, wheelInputOf(e, rect), wheelPrefs.settings, wheelPrefs.controller);
    if (result.kind !== "unhandled") dispatch({ type: "SET_SCHEMATIC_VIEW", view: result.view });
  };
  // React's own onWheel is passive, so its `preventDefault()` was ignored (and logged on every tick) while the page scrolled under the zoom.
  useNonPassiveWheel(containerRef, onWheel);

  // `WX_VIEW_CONTROLS::handleAutoPanning` / `onTimer`: while the pointer is in the border of the canvas during an interaction -- a wire, bus, line, shape or sheet being drawn, items
  // held by M, G or a drag, a paste being carried, a box being dragged -- the view pans every frame, faster the further into the border the pointer is. Off unless Preferences >
  // Mouse and Touchpad has "Automatically pan while moving object" on (KiCad's own default, and what the PCB canvas does); one frame loop, reading what it needs from refs.
  useEffect(() => {
    let raf = 0;
    const tick = () => {
      raf = requestAnimationFrame(tick);
      const { state: s, paste: carried, size, marquee: box } = live.current;
      if (!s.prefs.autoPan || s.tab !== "schematic") return;
      const drag = dragRef.current;
      const interactive = drag?.kind === "box" || drag?.kind === "cells" || (drag?.kind === "move" && drag.moved) || s.drawState != null || s.movePreview != null || carried != null;
      const pointer = lastPointerRef.current;
      if (!interactive || !pointer || !(s.schematicView.scale > 0)) return;
      const dir = computeAutoPanDirection(pointer, size, DEFAULT_VIEW_CONTROL_SETTINGS.autoPanMargin);
      const step = computeAutoPanStep(dir, size, s.schematicView.scale, DEFAULT_VIEW_CONTROL_SETTINGS.autoPanMargin, s.prefs.autoPanAcceleration);
      if (!step) return;
      userMovedRef.current = true;
      autoPannedRef.current = true;
      // a box being dragged keeps the corner it started at where it is on the sheet: its screen start moves by what the sheet did
      if (box && drag?.kind === "box") setMarquee({ ...box, x0: box.x0 - step.x * s.schematicView.scale, y0: box.y0 - step.y * s.schematicView.scale });
      dispatch({ type: "SET_SCHEMATIC_VIEW", view: panByWorldDelta(s.schematicView, step.x, step.y) });
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [dispatch]);
  // After a pan step the pointer is over a different place of the sheet: replay its last position so what is held, drawn or boxed follows (`WX_VIEW_CONTROLS::refreshMouse`).
  useEffect(() => {
    if (!autoPannedRef.current) return;
    autoPannedRef.current = false;
    const pointer = lastPointerRef.current;
    const container = containerRef.current;
    if (!pointer || !container) return;
    container.dispatchEvent(new PointerEvent("pointermove", { bubbles: true, clientX: pointer.clientX, clientY: pointer.clientY, pointerId: 1, pointerType: "mouse", buttons: dragRef.current ? 1 : 0 }));
  }, [state.schematicView]);

  /** The ERC marker on the sheet at (x, y) that is drawn (its severity is switched on in the display options), or -1. */
  const ercMarkerAt = (s: NonNullable<typeof sch>, x: number, y: number): number =>
    nearestMarker(
      (state.erc?.violations ?? []).map((v) => (ercSeverityShown(v.severity, schControl.display) ? (ercMarkerPosition(v.location, s)?.at ?? null) : null)),
      x,
      y,
      ERC_MARKER_HIT_NM
    );

  return (
    <div
      ref={containerRef}
      className="pcb-canvas-container"
      // how many items Import Sheet's contents in the hand show, and the point editor's points and side handles (for a browser check; KiCad draws both on the canvas);
      // an arc's points by their roles, as S13's check reads them
      data-import-ghost={importShown ? previewIds(importShown).length : undefined}
      // how many items a paste in the hand shows (Import Graphics' preview is one: `data-paste-kind="import"`)
      data-paste-ghost={paste && pasteShown ? previewIds(paste.preview).length : undefined}
      data-paste-kind={paste ? (paste.place ? "import" : paste.origin) : undefined}
      data-arc-points={shapeEditor?.graphic?.shape.type === "arc" ? shapeEditor.edit.points.map((q, i) => `${["start", "mid", "end", "center"][i]}:${q.pos[0]},${q.pos[1]}`).join(" ") : undefined}
      data-edit-points={shapeEditor ? shapeEditor.edit.points.map((q) => `${q.pos[0]},${q.pos[1]}`).join(" ") : undefined}
      data-edit-lines={shapeEditor ? shapeEditor.edit.lines.flatMap((l, i) => (l.center ? [handlePos(shapeEditor.edit, { kind: "line", index: i }).join(",")] : [])).join(" ") : undefined}
      data-autostart-hover={autostartHover ?? undefined}
      // (`displayWireCursor` / `displayBusCursor` over a point a click would start a wire from: KiCad's own LINE_WIRE / LINE_BUS cursors, kicad-port/kiCursors.ts)
      style={{ cursor: dragRef.current?.kind === "pan" || dragRef.current?.kind === "zoom" ? "grabbing" : held && hasNetCollision(patch?.net_collision) ? "not-allowed" : moveMode || dragMode || dragRef.current?.kind === "move" ? "move" : autostartHover ? KICURSOR_CSS[autostartHover] : "default" }}
      onPointerDown={(e) => {
        if (!sch) return;
        modsRef.current = { ctrl: isMac() ? e.metaKey : e.ctrlKey, shift: e.shiftKey };
        const gesture = new ClickDragGesture(dragRuleFor(isMac()));
        gesture.down(e.clientX, e.clientY, e.timeStamp);
        gestureRef.current = gesture;
        justPannedRef.current = false; // a new press: the last right-drag no longer vetoes the menu
        if (e.button === 1 || e.button === 2) {
          // `WX_VIEW_CONTROLS::onButton`: the middle and the right button pan, zoom or do nothing as Preferences > Mouse and Touchpad say. A right press that does
          // not become a drag is a click, and the context menu opens (`onContextMenu`).
          const action = dragActionOf(e.button, wheelPrefs.settings);
          if (action === "none") return;
          userMovedRef.current = true;
          (e.target as Element).setPointerCapture(e.pointerId);
          if (action === "pan") {
            dragRef.current = { kind: "pan", button: e.button, startScreen: [e.clientX, e.clientY], startView: [state.schematicView.x, state.schematicView.y], moved: false };
          } else {
            const rect = containerRef.current!.getBoundingClientRect();
            dragRef.current = { kind: "zoom", button: e.button, anchor: { x: e.clientX - rect.left, y: e.clientY - rect.top }, lastY: e.clientY, moved: false };
          }
          return;
        }
        if (e.button !== 0) return;
        const [wx, wy] = toWorld(e.clientX, e.clientY);

        // A paste in progress: this click drops it where the cursor holds it (`SCH_MOVE_TOOL`'s click, then `commit.Push( _( "Paste" ) )`).
        if (paste) {
          // Import Graphics' preview is held on the graphics grid (`grid.Align( .., GRID_GRAPHICS )`)
          const at = paste.place ? snap.align([wx, wy], NO_MODS, "graphics") : snapToGrid(wx, wy);
          void placeSchPaste(api, dispatch, [at[0], at[1]]);
          return;
        }

        // PICKER_TOOL::Main: while a pick session runs (actions/pcbPicker.ts: Group Properties' "Click on new member...") a left click answers it -- with the item under it, the outermost
        // group it is in -- and does nothing else.
        if (picker.session()) {
          // A session that places something on the grid (Place Image) is answered with the point the grid holds the cursor at.
          const [px, py] = picker.session()?.snap === "graphics" ? snap.align([wx, wy], NO_MODS, "graphics") : [wx, wy];
          picker.click({ point: { x: px, y: py }, item: () => pickNewMemberAt(sch, wx, wy, state.schematicView.scale, state.schSelectionFilter, state.schEnteredGroupId) });
          return;
        }

        // Highlight Nets (`SCH_EDITOR_CONTROL::HighlightNetCursor`'s picker): a click is `highlightNet` at that point -- the net of the wire, label or power symbol there,
        // or, over nothing, the highlight cleared. The tool stays armed.
        if (state.activeTool === "sch_highlight_net") {
          dispatch({ type: "SET_NET_HIGHLIGHT", net: netAtClick(sch, wx, wy, state.schematicView.scale, state.schGridNm) });
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
          // `BestSnapAnchor` on the tool's grid (wires: `GRID_WIRES`, lines: `GRID_GRAPHICS`): a pin is reached through the grid it sits on, or as an anchor with the grid off.
          const snapped = snapToGrid(wx, wy);
          let draw = state.drawState;
          if (draw?.kind !== "wire") {
            dispatch({ type: "SET_DRAW_STATE", draw: { kind: "wire", pts: [snapped] } });
            return;
          }
          // "First click when unfolding places the label and wire-to-bus entry" (r6 C6-M2): the entry keeps the posture this click gives it, the wire
          // starts at its end, and the member's label is here
          const unfold = state.busUnfold;
          if (unfold && !unfold.labelAt && draw.pts.length === 1) {
            const placed = placeUnfoldLabel(unfold.entryAt, unfold.size, snapped);
            dispatch({ type: "SET_BUS_UNFOLD", unfold: { ...unfold, size: placed.size, labelAt: placed.labelAt } });
            draw = { ...draw, pts: [placed.start] };
          }
          const tail = wireTail(draw.pts[draw.pts.length - 1]!, snapped, state.schLineMode, state.schPosture, elbowRef.current);
          elbowRef.current = null;
          const next = { ...draw, pts: [...draw.pts, ...(tail as [number, number][])] };
          // `SCH_SCREEN::IsTerminalPoint( cursorPos, segment->GetLayer() )`: a pin, junction, label, wire or bus of the segment's layer finishes it (r3 C3-M3)
          const layer = isLine ? "notes" : state.activeTool === "bus" ? "bus" : "wire";
          const terminal = isTerminalPoint(sch, pinSnapPoints(sch), snapped, layer);
          if (terminal && next.pts.length >= 2) {
            commitDrawn(next.pts);
            dispatch({ type: "SET_DRAW_STATE", draw: null });
            // `if( aQuitOnDraw ) { m_frame->PopTool( aTool ); break; }`: a wire the selection tool started gives the selection tool back
            if (draw.quitOnDraw) dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
          } else {
            dispatch({ type: "SET_DRAW_STATE", draw: next });
          }
          return;
        }

        // `SCH_DRAWING_TOOLS::DrawTable`: the first click is the table's first corner, the cursor sizes the grid, the second opens Table Properties; the table is on the sheet only once
        // OK is clicked ("Draw Table"). The tool stays armed for the next one.
        if (state.activeTool === "sch_table") {
          const c = snapToGrid(wx, wy);
          const draw = state.drawState;
          if (draw?.kind !== "table") {
            dispatch({ type: "SET_SELECTION", refs: [] });
            dispatch({ type: "SET_DRAW_STATE", draw: { kind: "table", origin: c } });
            return;
          }
          const grid = schDrawTableGrid(draw.origin, c, DEFAULT_TEXT_SIZE_NM, snap.gridSize("graphics"));
          dispatch({ type: "SET_DRAW_STATE", draw: null });
          openDrawnTableDialog({ layer: "", at: draw.origin, cols: grid.cols, rows: grid.rows, cellW: grid.cellW, cellH: grid.cellH, lineWidth: 0 }, "sch");
          return;
        }

        // Place Label / Global Label / Hierarchical Label / Directive Label / Text and Place Power (`TwoClickPlace`, `PlaceSymbol`, actions/schPlaceActions.ts):
        // with nothing in the hand the click makes the item -- its dialog or chooser opens, or a label on a labelled wire takes that name -- and it then
        // rides the cursor; with an item in the hand the click places it where `BestSnapAnchor` puts the cursor on the tool's grid (labels, directive
        // labels and power symbols on the connectable grid, text on the text grid: a label or power symbol on a pin's end joins it by position, as in
        // KiCad). The tool stays armed.
        if (isCarryTool(state.activeTool)) {
          const at = snapToGrid(wx, wy);
          if (carry && carry.tool === state.activeTool) placeCarried({ api, dispatch }, at);
          else beginItem({ api, dispatch }, state.activeTool, at);
          return;
        }

        // Draw Rectangle / Circle / Arc / Bezier / Text Box / Rule Area (components/schematic/schShapeTools.ts).
        if (isSchShapeTool(state.activeTool)) {
          shapeToolClick({ tool: state.activeTool, draw: state.drawState, sch, lineMode: state.schLineMode, dispatch, api, scale: state.schematicView.scale }, shapeSnap(wx, wy));
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

        // Import Sheet... / Draw Sheet from File: the same two corners, or one click for a file's contents (components/schematic/schImportTool.ts).
        if (state.activeTool === "sch_import_sheet") {
          importSheetClick(state, snapToGrid(wx, wy), snap.gridSize("connectable"), api, dispatch);
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
          dispatch({ type: "SET_SCH_SHEET_PENDING", pending: { at: draw.start, size: sheetSize(draw.start, c, snap.gridSize("connectable")) } });
          return;
        }

        // `Q` (sch_drawing_tools.cpp SingleClickPlace) and the bus entry tool: a click places one where `BestSnapAnchor` puts the cursor (the
        // connectable grid), and the tool stays armed for the next one, as real eeschema's does (Escape, or the hotkey again, leaves it). A click on an item
        // of the tool's kind already there (`screen->GetItem( cursorPos, 0, type )`) places nothing: a no-connect flag is removed instead ("No connects can be
        // "toggled"/removed by clicking on them again", `commit.Push( _( "Remove No Connect Flag" ) )`), a bus entry is left as it is.
        if (state.activeTool === "sch_no_connect") {
          const [sx, sy] = snapToGrid(wx, wy);
          const existing = existingSingleClickItem(sch, "no_connect", [sx, sy]);
          if (existing) void api.cmd({ op: "delete_no_connect", id: existing });
          else api.cmd({ op: "add_no_connect", at: { x: sx, y: sy } });
          return;
        }
        if (state.activeTool === "sch_bus_entry") {
          const [sx, sy] = snapToGrid(wx, wy);
          if (existingSingleClickItem(sch, "bus_entry", [sx, sy])) return;
          api.cmd({ op: "add_bus_entry", at: { x: sx, y: sy }, size: { x: 2_540_000, y: 2_540_000 } });
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
        // `SCH_POINT_EDITOR::Main`: a press on an edit point or a side's middle of the selected graphic (`EDIT_POINTS::FindPoint`, `EDIT_POINT::POINT_SIZE` either way)
        // drags that handle
        if (shapeEditor && !hasModifier(modifiers)) {
          const handle = eeFindHandle(shapeEditor.edit, [wx, wy], POINT_SIZE_PX / (state.schematicView.scale || 1));
          if (handle) {
            const at = handlePos(shapeEditor.edit, handle);
            dragRef.current = { kind: "editpoint", id: shapeEditor.id, handle };
            setPointDrag({ id: shapeEditor.id, handle, to: [at[0], at[1]] });
            return;
          }
        }
        // The item under the pointer (components/schematic/schPick.ts): a pin the pointer is on, a shown field, a symbol, any other placed item (a label, text, power symbol,
        // junction, no-connect, bus entry, sheet or drawn shape -- tight, screen-sized tolerance) and last a wire (`SCH_SELECTION_TOOL::selectPoint`: a click on a wire
        // selects it; the net highlight is a tool of its own). Every kind of item but a pin is picked up and dragged the same way.
        // A left click on an ERC marker selects it: `SCH_SELECTION_TOOL` takes an `SCH_MARKER` like any item ("always selectable") and `SCH_INSPECTION_TOOL::CrossProbe`
        // selects its row in the dialog when that is shown. A pin the pointer is exactly on wins (`GuessSelectionCandidates`: "closest" is a pin or a junction).
        if (!hasModifier(modifiers) && state.activeTool === "select") {
          const markerIndex = ercMarkerAt(sch, wx, wy);
          if (markerIndex >= 0 && !pickItem(sch, wx, wy, state.schematicView.scale, selectable, "only")) {
            selectMarkerErc({ api, dispatch }, markerIndex);
            return;
          }
        }
        const picked0 = pickItem(sch, wx, wy, state.schematicView.scale, selectable, "first");
        // `filterCollectorForHierarchy`: an item of a group is picked as the outermost group that holds it inside the group worked in (a pin and a field as their symbol's); an item outside
        // the group worked in leaves it (`select`: `ExitGroup()`), and so does a click on nothing outside its box (`selectPoint`).
        const pickedSub = picked0 ? substituteSchSelection(sch, [picked0], state.schEnteredGroupId) : null;
        const enteredBox = state.schEnteredGroupId ? groupBounds(sch, state.schEnteredGroupId) : null;
        const outsideEntered = !picked0 && enteredBox != null && (wx < enteredBox.minX || wx > enteredBox.maxX || wy < enteredBox.minY || wy > enteredBox.maxY);
        if (pickedSub?.exits || outsideEntered) dispatch({ type: "SET_SCH_ENTERED_GROUP", id: null });
        const picked = pickedSub ? (pickedSub.ids[0] ?? null) : null;
        const wireId = picked && sch.wires.some((w) => w.id === picked) ? picked : null;
        // `SCH_SELECTION_TOOL::Main`'s `IsDrag( BUT_LEFT )`: unless the one item selected is a table the drag starts in (which moves it), a drag from a table cell -- of a
        // table in no group, or in the group worked in -- selects the cells under it (`selectTableCells`, `SelectCellsBetween`). The press picks nothing: a release with
        // no drag is the click it would have been (kicad-port/tableCells.ts).
        if (state.activeTool === "select" && !e.altKey && (sch.tables ?? []).length > 0) {
          const tables = sch.tables ?? [];
          const under = cellUnder(tables, wx, wy, tolerance);
          const holder = under ? (sch.groups ?? []).find((g) => g.member_ids.includes(under.table.id)) : undefined;
          const one = state.selection.size === 1 ? tables.find((t) => state.selection.has(t.id)) : undefined;
          const inOne = one != null && wx >= one.bbox[0] && wx <= one.bbox[2] && wy >= one.bbox[1] && wy <= one.bbox[3];
          if (under && !inOne && (!holder || holder.id === state.schEnteredGroupId)) {
            areaClickRef.current = () => {
              // Shift+click on a second cell: the cells between it and the first (`m_previous_first_cell`, kicad-port/tableCells.ts `shiftClickCells`; UI-LEFT-2 SS18)
              const range = picked && modifiers.additive ? shiftClickCells(tables, [...state.selection], picked, previousFirstCellRef.current) : null;
              if (range) {
                previousFirstCellRef.current = range.previousFirst;
                dispatch({ type: "SET_NET_HIGHLIGHT", net: null });
                dispatch({ type: "SET_SELECTION", refs: range.refs, raw: true });
                dispatch({ type: "SET_SCH_WIRE_PICK", pick: null });
                return;
              }
              if (picked) {
                dispatch({ type: "SET_NET_HIGHLIGHT", net: null });
                dispatch({ type: "SET_SELECTION", refs: hasModifier(modifiers) ? applySingleClickModifier(state.selection, picked, modifiers) : [picked] });
                dispatch({ type: "SET_SCH_WIRE_PICK", pick: null });
              } else if (!hasModifier(modifiers)) {
                dispatch({ type: "CLEAR_SELECTION" });
              }
            };
            dragRef.current = { kind: "cells", table: under.table.id, startWorld: [wx, wy], before: [...state.selection], modifiers, moved: false };
            return;
          }
        }
        if (picked) {
          dispatch({ type: "SET_NET_HIGHLIGHT", net: null });
          if (hasModifier(modifiers)) {
            // A modified click always applies immediately, never starts a drag -- real sch_selection_tool.cpp routes a modified drag to
            // box-select instead, even one starting on a hit item (see this file's "nothing hit" box-select branch below).
            dispatch({ type: "SET_SELECTION", refs: applySingleClickModifier(state.selection, picked, modifiers) });
            dispatch({ type: "SET_SCH_WIRE_PICK", pick: null });
            return;
          }
          // A pin is selected and acted on (Swap Pins, a net highlight), never picked up: `SCH_PIN_T` is no movable item, so a drag that starts on one moves nothing.
          if (isPinItemId(picked)) {
            // `autostartEvent`: a click on an unconnected pin starts a wire there instead (a drag from it draws the selection area, a pin being no movable item)
            const auto = autostartAt(picked, wx, wy);
            if (auto) {
              areaClickRef.current = () => startAutostart(auto);
              dragRef.current = { kind: "box", startWorld: [wx, wy], startScreen: [e.clientX, e.clientY] };
              return;
            }
            dispatch({ type: "SET_SELECTION", refs: [picked] });
            dispatch({ type: "SET_SCH_WIRE_PICK", pick: null });
            return;
          }
          // `SCH_SELECTION_TOOL::Main`'s `IsDrag( BUT_LEFT )` under "Left button drag:" (`leftDragOutcome`): with "Draw selection rectangle", from an item not selected under
          // "Drag selected objects", or from outside a selection that is not empty under "Drag any object", the drag draws the selection area; the press then selects
          // nothing yet, and a release without a drag is the click.
          if (leftDragOutcome(wheelPrefs.settings.dragLeft, { modifier: false, selectionEmpty: state.selection.size === 0, onItem: true, inSelection: state.selection.has(picked) }) === "area") {
            const wire = wireId ? sch.wires.find((w) => w.id === wireId) : undefined;
            const vertices = wire ? (wirePick(wire, [wx, wy], tolerance) ?? undefined) : undefined;
            const auto = autostartAt(picked, wx, wy);
            areaClickRef.current = () => {
              if (auto) return startAutostart(auto);
              dispatch({ type: "SET_SELECTION", refs: [picked] });
              dispatch({ type: "SET_SCH_WIRE_PICK", pick: wire && vertices ? { id: wire.id, vertices } : null });
            };
            dragRef.current = { kind: "box", startWorld: [wx, wy], startScreen: [e.clientX, e.clientY] };
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
          const dragOrigin = snap.dragOrigin([wx, wy], leafIds(sch, refs));
          const origin: [number, number] = [dragOrigin[0], dragOrigin[1]];
          dispatch({ type: "SET_MOVE_ORIGIN", at: { x: wx, y: wy } });
          dragRef.current = { kind: "move", refs, startWorld: [wx, wy], origin, moved: false, vertices: wire && vertices ? { [wire.id]: vertices } : undefined, autostart: autostartAt(picked, wx, wy) ?? undefined };
          return;
        }

        // Nothing under the cursor: box select, or (no modifier) a plain
        // click that just clears whatever was selected -- same as
        // Canvas.tsx's own "nothing hit" branch.
        dragRef.current = { kind: "box", startWorld: [wx, wy], startScreen: [e.clientX, e.clientY] };
        if (!hasModifier(modifiers)) dispatch({ type: "CLEAR_SELECTION" });
      }}
      onPointerMove={(e) => {
        const bounds = containerRef.current?.getBoundingClientRect();
        if (bounds) lastPointerRef.current = { x: e.clientX - bounds.left, y: e.clientY - bounds.top, clientX: e.clientX, clientY: e.clientY };
        // `WX_VIEW_CONTROLS::onMotion`'s `m_motionPanModifier`: with the key of "Pan on mouse movement with key:" held the sheet follows the pointer; the tools still see the motion.
        const metaPan = motionPan(state.schematicView, wheelPrefs.settings.motionPanModifier, { altKey: e.altKey, ctrlOrCmd: isMac() ? e.metaKey : e.ctrlKey, shiftKey: e.shiftKey }, motionPanRef.current, { x: e.clientX, y: e.clientY });
        motionPanRef.current = metaPan.start;
        if (metaPan.view && state.schematicView.scale > 0) {
          userMovedRef.current = true;
          dispatch({ type: "SET_SCHEMATIC_VIEW", view: metaPan.view });
        }
        if (!sch) return;
        const [wx, wy] = toWorld(e.clientX, e.clientY);
        dispatch({ type: "SET_CURSOR", at: { x: wx, y: wy } });
        const motion = gestureRef.current?.move(e.clientX, e.clientY, e.timeStamp);
        modsRef.current = { ctrl: isMac() ? e.metaKey : e.ctrlKey, shift: e.shiftKey };

        // The snapping of the tool in force follows the cursor (`BestSnapAnchor` runs on every event of a KiCad tool): the marker, the snap line, the point a rubber band goes to.
        // Draw Arc (KiCad 25ec2335's `drawManagedShape`): every motion is `AddPoint( cursorPos, false )` on the graphics grid, which also keeps the direction lock
        if (state.activeTool === "sch_arc" && state.drawState?.kind === "sch_shape" && state.drawState.arc) {
          const [ax, ay] = shapeSnap(wx, wy);
          dispatch({ type: "SET_DRAW_STATE", draw: { ...state.drawState, arc: arcMotion(state.drawState.arc, [ax, ay], false) } });
        }
        if (SCH_PLACING_TOOLS.has(state.activeTool) && !dragRef.current) {
          const p = snapToGrid(wx, wy);
          // where the placing tool's hotkey puts its item down (the tool's own `cursorPos`); only while one is armed
          noteCarryCursor(isCarryTool(state.activeTool) ? p : null);
        } else noteCarryCursor(null);

        if (armedKind && state.selection.size > 0) {
          // `SCH_MOVE_TOOL::doMoveSelection`: the selection is held by its reference point (`BestDragOrigin`: the nearest pin or origin of what is held to where it was picked up)
          // and goes to the cursor snapped with `BestSnapAnchor` on the coarsest grid of the selection, skipping the selection itself.
          // (KiCad warps the pointer to the held point when the move starts; a page may not, so the cursor is that point plus the pointer's travel since: the item stays where it was grabbed.)
          const origin = state.moveOriginNm ?? { x: wx, y: wy };
          const ids = [...state.selection];
          // a group is held as the items below it (`SCH_GROUP::Move`): the reference point and the grid are theirs
          const held = leafIds(sch, ids);
          const [ox, oy] = snap.dragOrigin([origin.x, origin.y], held);
          const [sx, sy] = snap.moveCursor(heldCursor([ox, oy], [origin.x, origin.y], [wx, wy]), modsRef.current, held);
          // R, Shift+R, X and Y pressed since the items were picked up stay on the preview as it follows the cursor
          // (Whole micrometres, as KiCad's integer coordinates are: with the grid off (Ctrl) the cursor is wherever the pointer is, and the verbs take integer deltas.)
          dispatch({ type: "SET_MOVE_PREVIEW", preview: { refs: ids, kind: armedKind, dxNm: Math.round(sx - ox), dyNm: Math.round(sy - oy), holdNm: [sx, sy], vertices: pickedVertices(state), turns: state.movePreview?.turns } });
          return;
        }

        const drag = dragRef.current;
        // `evt->IsMotion()` of the selection tool: over a point a click would start a wire from, the cursor says so (`autostartEvent` with no modifier held)
        if (!drag && state.activeTool === "select" && state.prefs.schAutoStartWires && !picker.session() && !paste) {
          const noMods = !e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey;
          const under = noMods ? pickItem(sch, wx, wy, state.schematicView.scale, selectable, "first") : null;
          const top = under ? (substituteSchSelection(sch, [under], state.schEnteredGroupId).ids[0] ?? under) : null;
          const hover = top ? (autostartAt(top, wx, wy)?.tool ?? null) : null;
          if (hover !== autostartHover) setAutostartHover(hover);
        } else if (autostartHover) setAutostartHover(null);
        if (!drag) return;
        if (drag.kind === "editpoint") {
          // `grid->Align( controls->GetMousePosition(), GRID_GRAPHICS )` with the edited item's own anchors left out
          const p = snap.point("pointEditor", [wx, wy], modsRef.current, "graphics", new Set([drag.id]));
          setPointDrag({ id: drag.id, handle: drag.handle, to: [p[0], p[1]] });
          return;
        }
        if (drag.kind === "pan") {
          userMovedRef.current = true;
          if (motion?.dragging) drag.moved = true;
          const dx = e.clientX - drag.startScreen[0];
          const dy = e.clientY - drag.startScreen[1];
          dispatch({ type: "SET_SCHEMATIC_VIEW", view: { ...state.schematicView, x: drag.startView[0] + dx, y: drag.startView[1] + dy } });
        } else if (drag.kind === "zoom") {
          // `onMotion`'s DRAG_ZOOMING: the view is scaled about the press point by how far the pointer went down the screen since the last motion
          userMovedRef.current = true;
          if (motion?.dragging) drag.moved = true;
          dispatch({ type: "SET_SCHEMATIC_VIEW", view: dragZoomView(state.schematicView, drag.anchor, e.clientY - drag.lastY, state.prefs.zoomSpeed) });
          drag.lastY = e.clientY;
        } else if (drag.kind === "cells") {
          if (!motion?.dragging) return;
          drag.moved = true;
          const table = (sch?.tables ?? []).find((t) => t.id === drag.table);
          if (table) dispatch({ type: "SET_SELECTION", refs: cellsBetween(table, drag.startWorld, [wx, wy], drag.before, drag.modifiers), raw: true });
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
          const [sx, sy] = snap.moveCursor(heldCursor(drag.origin, drag.startWorld, [wx, wy]), modsRef.current, leafIds(sch, drag.refs));
          // `if( m_isSymbolEditor || m_frame->eeconfig()->m_Input.drag_is_move ) RunAction( move ) else RunAction( drag )`: "Mouse drag performs Drag (G) operation"
          dispatch({ type: "SET_MOVE_PREVIEW", preview: { refs: drag.refs, kind: state.prefs.schDragIsMove ? "sch_move" : "sch_drag", dxNm: Math.round(sx - drag.origin[0]), dyNm: Math.round(sy - drag.origin[1]), holdNm: [sx, sy], vertices: state.prefs.schDragIsMove ? undefined : drag.vertices, turns: state.movePreview?.turns } });
        }
      }}
      onPointerLeave={() => {
        // a pointer that has left the canvas no longer pans it (a drag keeps the pointer captured, so this does not fire during one)
        lastPointerRef.current = null;
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        // `WX_VIEW_CONTROLS::onButton`: a right-drag pans (or zooms) rather than opening anything -- the browser's own `contextmenu` event still follows the button-up
        if (justPannedRef.current) {
          justPannedRef.current = false;
          return;
        }
        if (!sch || paste) return;
        // `SCH_SELECTION_TOOL`: a right-click on an item that is not selected selects it first, on empty space clears the selection; then the
        // menu for whatever is selected (kicad-port/schContextMenu.ts).
        const [wx, wy] = toWorld(e.clientX, e.clientY);
        // A right click on an ERC marker (a circle the last run left on the sheet) opens the marker's menu -- Exclude, Exclude all of this type, Change
        // severity, Show in the dialog -- instead of the item menu: an `SCH_MARKER` is an item of its own with its own entries.
        const found = state.erc?.violations ?? [];
        const markerIndex = ercMarkerAt(sch, wx, wy);
        if (markerIndex >= 0) {
          const v = found[markerIndex]!;
          dispatch({ type: "SET_ERC_SELECTED", index: markerIndex });
          const spec = rcMenu(ercMenuContext(v, true));
          setContextMenu(null);
          setMarkerMenu({ x: e.clientX, y: e.clientY, entries: toMenuEntries(spec, (id) => void runMarkerMenu({ api, dispatch }, { domain: "erc", index: markerIndex }, id, askExclusionComment), ercKey(v) !== null) });
          return;
        }
        setMarkerMenu(null);
        // The menu opens at the cursor, which is where the actions it offers (Break, Slice, ...) read their position from.
        dispatch({ type: "SET_CURSOR", at: { x: wx, y: wy } });
        const scale = state.schematicView.scale || 1;
        // An item in the hand: the menu of the selection, which is the item (`TwoClickPlace` / `PlaceSymbol`: `m_menu->ShowContextMenu( m_selectionTool->GetSelection() )`);
        // its entries reach the tool's loop like its keys (actions/schPlaceActions.ts), the Symbol Unit choices included
        if (carryShown && carry) {
          const view = withPaste(sch, carryShown, carryDx, carryDy);
          const carriedId = previewIds(carryShown)[0] ?? (carry.item.kind === "symbol" ? carry.item.ref : null);
          if (carriedId) {
            const other = state.units === "mm" ? "mil" : "mm";
            const std = { zoom: zoomMenuEntries(zoomListFor("schematic"), state.schematicView.scale), grid: gridMenuEntries(gridSettingsList, gridNm, (nm) => formatLength(nm, state.units), (nm) => formatLength(nm, other)) };
            setContextMenu({ x: e.clientX, y: e.clientY, nodes: schContextMenu(summarizeSelection(view, [carriedId], [wx, wy], POINT_SIZE_PX / scale, { allowPinSwaps: false, inGroup: false, unit: carry.item.kind === "symbol" ? carry.item.unit : undefined }), std) });
          }
          return;
        }
        const hit0 = pickItem(sch, wx, wy, scale, selectable, "first");
        const hit = hit0 ? (substituteSchSelection(sch, [hit0], state.schEnteredGroupId).ids[0] ?? hit0) : null;
        let ids = [...state.selection];
        if (hit && !state.selection.has(hit)) ids = [hit];
        else if (!hit && ids.length > 0) ids = [];
        if (ids.length !== state.selection.size || ids.some((id) => !state.selection.has(id))) dispatch({ type: "SET_SELECTION", refs: ids });
        // a reference's placed units are one item: the Symbol Unit submenu is for the one under the pointer (else the first)
        const underPointer = ids.length === 1 ? sch.symbols.filter((x) => x.id === ids[0]).find((x) => { const b = libSymbolBounds(x, sch.lib_symbols); return wx >= b.minX && wx <= b.maxX && wy >= b.minY && wy <= b.maxY; })?.unit : undefined;
        // `AddStandardSubMenus`: the Zoom submenu over the eeschema zoom list, the Grid submenu over the Schematic Editor's grids
        const other = state.units === "mm" ? "mil" : "mm";
        const std = { zoom: zoomMenuEntries(zoomListFor("schematic"), state.schematicView.scale), grid: gridMenuEntries(gridSettingsList, gridNm, (nm) => formatLength(nm, state.units), (nm) => formatLength(nm, other)) };
        const nodes = schContextMenu(summarizeSelection(sch, ids, [wx, wy], POINT_SIZE_PX / scale, { allowPinSwaps: state.prefs.allowUnconstrainedPinSwaps, inGroup: state.schEnteredGroupId != null, unit: underPointer, variant: state.schVariant }), std);
        // KiCad 25ec2335's `EE_GRAPHIC_TOOL` menu while Draw Arc is armed (`inDrawingArc`, order 200): the five modes as check items, then Cycle Arc Drawing Mode
        const arcModes: MenuNode[] =
          state.activeTool === "sch_arc"
            ? [...ARC_DRAW_MODES.map((m): MenuNode => ({ type: "item", action: ARC_MODE_ACTIONS[m].action, label: ARC_MODE_ACTIONS[m].label, checked: m === schArcDrawMode() })), { type: "item", action: CYCLE_ARC_DRAW_MODE, label: "Cycle Arc Drawing Mode" }, { type: "separator" }]
            : [];
        setContextMenu({ x: e.clientX, y: e.clientY, nodes: [...arcModes, ...nodes] });
      }}
      onDoubleClick={(e) => {
        // A double-click on a hierarchical sheet enters it (`SCH_SELECTION_TOOL` -> `SCH_ACTIONS::enterSheet`); on any other item it opens its Properties
        // (`SCH_ACTIONS::properties`): the selection the first click made, else the item under the pointer.
        if (sch && state.activeTool === "select" && state.drawState == null) {
          const [wx, wy] = toWorld(e.clientX, e.clientY);
          // `SCH_EDIT_TOOL::Properties` on an `SCH_MARKER`: `CrossProbe( marker )` -- the ERC dialog comes up with the marker's row selected.
          const markerHit = ercMarkerAt(sch, wx, wy);
          if (markerHit >= 0 && !pickItem(sch, wx, wy, state.schematicView.scale || 1, selectable, "only")) {
            showErcInDialog({ api, dispatch }, markerHit);
            return;
          }
          const sheetId = hitSheet(sch.sheets, wx, wy);
          // (a sheet in a group is picked as the group: the double click enters the group, not the sheet)
          const sheetTop = sheetId ? substituteSchSelection(sch, [sheetId], state.schEnteredGroupId).ids[0] : null;
          if (sheetId && sheetTop === sheetId) {
            void api.navigateToSheet([...state.currentSheetPath, sheetId]);
            return;
          }
          const scale = state.schematicView.scale || 1;
          const under0 = pickItem(sch, wx, wy, scale, selectable, "none");
          const under = under0 ? (substituteSchSelection(sch, [under0], state.schEnteredGroupId).ids[0] ?? under0) : null;
          const ids = promoteLonePin(state.selection.size > 0 ? [...state.selection] : under ? [under] : []);
          // `SCH_SELECTION_TOOL::Main`: "else if( m_selection.GetSize() == 1 && m_selection[0]->Type() == SCH_GROUP_T ) EnterGroup()"
          if (ids.length === 1 && isGroup(groupsOf(sch), ids[0]!)) {
            dispatch({ type: "ENTER_SCH_GROUP", id: ids[0]! });
            return;
          }
          if (openSchProperties(sch, ids, dispatch, [wx, wy])) return;
        }
        const draw = state.drawState;
        // `IsDblClick( BUT_LEFT )` in DrawShape / DrawRuleArea: finish the shape as it stands, at the point that was clicked.
        if (draw?.kind === "sch_shape" && sch) {
          const [wx, wy] = toWorld(e.clientX, e.clientY);
          finishShapeDraw({ tool: state.activeTool, draw, sch, lineMode: state.schLineMode, dispatch, api }, shapeSnap(wx, wy));
          return;
        }
        if (draw?.kind !== "wire") return;
        // `doDrawSegments`: a double-click is a click and then the finish, and End (`finishInteractive`, `cursorDblClick`) finishes the segments as
        // they are -- the one that runs to the cursor included (`finishSegments` keeps the last non-null segment). A real double-click's presses put
        // that point in already; the commit drops the zero-length repeat.
        const [wx, wy] = toWorld(e.clientX, e.clientY);
        const tail = wireTail(draw.pts[draw.pts.length - 1]!, snapToGrid(wx, wy), state.schLineMode, state.schPosture, elbowRef.current);
        elbowRef.current = null;
        commitDrawn([...draw.pts, ...(tail as [number, number][])]);
        dispatch({ type: "SET_DRAW_STATE", draw: null });
        if (draw.quitOnDraw) dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
      }}
      onPointerUp={(e) => {
        gestureRef.current?.up();
        const drag = dragRef.current;
        dragRef.current = null;
        if (!drag) return;
        if (drag.kind === "pan" || drag.kind === "zoom") {
          justPannedRef.current = drag.button === 2 && drag.moved;
          return;
        }
        if (drag.kind === "editpoint") {
          // the drag ends: the item as its points have it and what followed it, one undo step (`commit.Push( _( "Move Point" ) )`); a drag that changed nothing sends nothing
          const cmds = shapeEditor && shapeEditor.id === drag.id && pointDrag ? shapeEditor.cmds : [];
          setPointDrag(null);
          if (cmds.length > 0) void api.cmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
          return;
        }
        if (drag.kind === "move") {
          if (drag.moved && state.movePreview) {
            void api.commitSchHeld(state.movePreview);
          } else if (drag.moved) {
            dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
          } else if (drag.autostart) {
            // a click on an unconnected label, sheet pin, bus entry or wire end (`autostartEvent`): the wire starts there, nothing is selected
            dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
            startAutostart(drag.autostart);
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
        const areaClick = areaClickRef.current;
        areaClickRef.current = null;
        if (drag.kind === "cells") {
          if (!drag.moved && areaClick) areaClick();
          return;
        }
        if (drag.kind === "box" && sch) {
          // a press on an item that would have drawn the area, released without a drag: the click it is
          if (!marquee && areaClick) areaClick();
          if (marquee) {
            const ctrlOrCmd = isMac() ? e.metaKey : e.ctrlKey;
            const modifiers = computeClickModifiers(e.shiftKey, ctrlOrCmd, e.altKey);
            const [x1, y1] = toWorld(e.clientX, e.clientY);
            const [x0, y0] = drag.startWorld;
            const box: [number, number, number, number] = [Math.min(x0, x1), Math.min(y0, y1), Math.max(x0, x1), Math.max(y0, y1)];
            // a window takes a group only when it holds the group's whole box, and a member only through its group; a crossing box takes the group of any member it touches
            const found = boxItems(sch, box, marquee.crossing, (id) => pickable(id) !== null);
            const hits = boxSelectionWithGroups(sch, found, { minX: box[0], minY: box[1], maxX: box[2], maxY: box[3] }, marquee.crossing, state.schEnteredGroupId, sheetBoxOf(sch), (id) => pickable(id) !== null);
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
