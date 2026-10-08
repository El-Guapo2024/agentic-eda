// Bridges KiCad's dotted action names (src/kicad/actions.json, referenced
// by menus.json/toolbars.json) to this app's own implementations.
//
// Now that extraction produces real, verified names (538 actions,
// cross-checked against source for several), the registry below wires
// up the ones this app actually implements. Everything else stays
// disabled with "(not ported yet)" in the menu bar and toolbars, which
// is the correct, honest state for the rest until it's built.
//
// pcbnew.InteractiveMove.move's "arm, then click/move to commit" state
// (state.activeTool) and common.Interactive.cancel's reset both live in
// the global store now (state/store.tsx) -- a menu/toolbar click reaches
// the exact same flow the M/Escape keyboard shortcuts do, and
// Canvas.tsx no longer needs any keydown handling of its own.

import { useCallback, useMemo, useRef } from "react";
import { useStudioApi, useStudioDispatch, useStudioState, type ToolId } from "../state/store";
import { toCmdDimension } from "../kicad-port/dimensionConvert";
import type { Cmd, CmdDimensionKind } from "../api/types";
import { isActionEnabledForTab } from "../kicad-port/actionTabGate";
import { zoomAbout, fitTransform, boundsOfPoints, worldToScreen, panByWorldDelta, screenToWorld } from "../components/canvas/view";
import { finishInteractiveRoute, cancelInteractiveRoute, startInteractiveRoute } from "../components/canvas/routing";
import { clearRouteQueue, startRouteQueue, type QueueOutcome } from "../components/canvas/routeQueue";
import { routeMove, routeToggleVia, routeUndoSegment, dpMove, dpUndoSegment, fetchErc, routeStart, routeFinish, routeCancel, downloadKicadPcb, downloadKicadSchematic } from "../api/client";
import { ercMarkerPosition } from "../components/schematic/ercMarkerPosition";
import { cursorMove, panByGrid, viewCenter, viewCenteredOn, warpViewToInclude, type CursorDir } from "../kicad-port/cursorControl";
import { nextMarker } from "../kicad-port/markerNav";
import { selectionAsText, datasheetTarget } from "../kicad-port/itemText";
import { pickSelectionCandidates } from "../components/canvas/selectionCandidates";
import { snapPoint } from "../components/canvas/gridHelper";
import { findNearestEdgeInsertionIndex, insertCorner } from "../kicad-port/zonePointEditor";
import { grabNearestUnconnectedFootprints, movableItem, otherEndOfStart, resolveToggleLock, routeSelectedAnchors, routeStartLayer, selectUnconnectedFootprints, stepCopperLayer, unrouteSegmentReselect } from "../kicad-port/pcbEditActions";
import { amplitudeStep, nextAngleSnapMode, spacingStep, stepStrokeWidth } from "../kicad-port/pcbParityState";
import { drawStateFromPreview } from "../kicad-port/routeTool";
import { dpStateFromPreview } from "../kicad-port/dpTool";
import { finishDiffPairRoute } from "../components/canvas/diffPairRouting";
import { findDraggableAt, startInlineDrag } from "../components/canvas/dragging";
import { openPropertiesFor } from "../components/canvas/properties";
import { findNetAtCursor } from "../components/canvas/netAtCursor";
import { expandConnection, type ConnTrack, type ConnVia, type StartPoint } from "../kicad-port/expandConnection";
import { pickedVertices } from "../kicad-port/schMove";
import { alignMoves, type SchAlignItem, type SchAlignKind } from "../kicad-port/schAlign";
import type { SchTurn } from "../api/schEditTypes";
import { findNextMatch } from "../components/schematic/findNavigation";
import { resolveLibSymbol } from "../components/schematic/libSymbol";
import { symbolBounds } from "../components/schematic/painter";
import { GRID as SCH_GRID_UM } from "../components/schematic/layout";
import { alignToGrid } from "../kicad-port/gridSnap";
import { netAtPoint } from "../kicad-port/schNetAtPoint";
import { selectConnection, selectNodeAt } from "../kicad-port/schConnection";
import { goBack, goForward } from "../kicad-port/navHistory";
import { nextLineMode, LINE_MODE_FREE, LINE_MODE_90, LINE_MODE_45 } from "../kicad-port/schLineMode";
import { useFpApi, useFpDispatch } from "../state/footprintEditorStore";
import { arcRemoveLastPoint, arcToggleClockwise } from "../kicad-port/arcGeom";
import { bezierRemoveLastPoint } from "../kicad-port/bezierGeom";
import { planPack } from "../kicad-port/packFootprints";
import { netNavigatorItems, stepNetItem, type NavSchematic } from "../kicad-port/netNavigator";
import { repeatCmds } from "../kicad-port/schRepeat";
import { nextReference } from "../kicad-port/nextReference";
import { refDesPrefix } from "../kicad-port/packFootprints";
import { updatePcbMessage } from "../kicad-port/updatePcb";
import { useSymApi, useSymDispatch } from "../state/symbolEditorStore";
import { registerLibraryEditorActions } from "./libraryEditorActions";
import { registerCommonActions, type ActionHandler } from "./commonActions";
import { makeEditorAdapter } from "./editorAdapter";
import { cancelAreaTool, getCommonTool } from "../state/commonTool";
import { commonChecked } from "../kicad-port/commonChecked";
import { useCommonOptions } from "../state/commonOptions";
import { registerEditorFrameActions } from "./editorFrameActions";
import { getDockLayout, setDockColumnCollapsed, toggleDockPane } from "../state/dockLayoutStore";
import { registerSchControlActions, schControlChecked } from "./schControlActions";
import { useSchControlDispatch, useSchControlState } from "../state/schControlStore";
import { arcClickPoints } from "../components/canvas/curveTools";
import { hitBus, hitSymbol, hitWire, schematicBounds } from "../components/schematic/schHit";
import { allItems, hitItems, itemBounds } from "../components/schematic/schItems";
import { deleteCmds } from "../kicad-port/schDelete";
import { withoutLocked } from "../kicad-port/schLock";
import { schSelectable } from "../kicad-port/schSelectionFilter";
import { registerSchEditActions } from "./schEditActions";
import { deleteLastPoint } from "../components/schematic/schShapeTools";
import { openSchProperties } from "../components/schematic/schPropertiesOpen";
import { nextLargerPreset, nextSmallerPreset, selectAllIds, wrapStep } from "../kicad-port/editTargets";
import { registerBoardControlActions } from "./boardControlActions";
import { flipLocalX } from "../kicad-port/boardControl";
import { registerPcbEditSweep } from "./pcbEditSweep";
import { layerPairsOf } from "./pcbRouterSweep";
import { picker } from "./pcbPicker";
import { otherLayerOfPair } from "../kicad-port/layerPairs";

function canvasRect(): DOMRect | null {
  return document.querySelector(".pcb-canvas-container")?.getBoundingClientRect() ?? null;
}

/** `pcbnew.EditorControl.trackWidthInc`/`trackWidthDec`'s ("W"/"Shift+W") preset ladder -- see those two action registrations below for why this is a fixed list rather than a per-board setting. */
const WIDTH_PRESETS_UM = [100, 150, 200, 250, 300, 400, 500, 600, 800, 1000];

/** common/tool/common_tools.cpp doZoomInOut: "Step must be AT LEAST 1.3" -- the exact per-step factor for zoomIn/zoomOut (F1/F2) and zoomInCenter/zoomOutCenter alike (doZoomInOut/doZoomInOutCenter share it). Source then snaps the result to the nearest entry in a separate zoom% preset list before applying it -- not ported (that preset list lives in per-app window settings this project has no equivalent of yet), so this applies the 1.3 factor directly. */
const ZOOM_STEP_FACTOR = 1.3;

export function useActionRunner() {
  const api = useStudioApi();
  const dispatch = useStudioDispatch();
  const state = useStudioState();
  const symApi = useSymApi();
  const symDispatch = useSymDispatch();
  const fpApi = useFpApi();
  const fpDispatch = useFpDispatch();
  const schControl = useSchControlState();
  const schControlDispatch = useSchControlDispatch();
  // The shared tools' toggles (Always Show Crosshairs, Library Tree ...), read by `isChecked`; being subscribed here re-renders a menu or a toolbar when one changes.
  const commonOptions = useCommonOptions();
  /** `m_afterItem` of find-next-marker (SCH_FIND_REPLACE_TOOL): the last ERC marker visited, so the next press continues from it. */
  const markerCursor = useRef<string | null>(null);
  /** The net navigator's own tree selection (`m_netNavigator->GetSelection()`): which item of the highlighted net Tab/Shift+Tab last landed on. */
  const netNavKey = useRef<string | null>(null);

  const registry = useMemo(() => {
    // A handler may take the one parameter an action carries (`TOOL_EVENT::Parameter`), e.g. the sheet path of `NavigateTool.changeSheet`.
    const m = new Map<string, ActionHandler>();
    // Rotate/move/rip/the footprint-properties dialog/the PCB view's own
    // pan-zoom actions all read or write PCB-only state (api.*Selection,
    // state.view, .pcb-canvas-container's rect) -- and that CSS class is
    // shared by the Schematic tab's own container (for style reuse), so
    // canvasRect() would resolve to the wrong canvas there. Read-only for
    // now on the Schematic tab (see SchematicView.tsx's own header
    // comment) means these must be no-ops there, not just "probably
    // harmless" -- a stray R/M/Del/E keypress must never reach a real
    // board command while looking at the schematic.
    const pcbOnly =
      <Args extends unknown[]>(fn: (...args: Args) => void) =>
      (...args: Args) => {
        if (state.tab === "pcb") fn(...args);
      };

    /** Tabs that have their own pan/zoom view in the shared view state (PCB and Schematic). */
    const viewTab =
      <Args extends unknown[]>(fn: (...args: Args) => void) =>
      (...args: Args) => {
        if (state.tab === "pcb" || state.tab === "schematic") fn(...args);
      };
    const curView = () => (state.tab === "schematic" ? state.schematicView : state.view);
    const setView = (view: typeof state.view) => dispatch({ type: state.tab === "schematic" ? "SET_SCHEMATIC_VIEW" : "SET_VIEW", view });

    /**
     * `PCB_SELECTION_TOOL::RequestSelection` / `SCH_SELECTION_TOOL::RequestSelection`: the current
     * selection, or -- when nothing is selected -- the item under the cursor (`selectPoint`; no
     * clarification menu, the best candidate wins). Used by every edit hotkey (R/F/M/Del/E/U/Shift+M...)
     * so hover + key works without a prior click.
     */
    const requestSelection = (): string[] => {
      if (state.selection.size > 0) return [...state.selection];
      if (!state.cursorUm) return [];
      if (state.tab === "schematic") {
        const sch = state.schematic;
        if (!sch) return [];
        // `itemPassesFilter`: an item the selection filter keeps out (a category that is off, a locked item without "Locked items") cannot be picked up by the cursor.
        const selectable = schSelectable(sch, state.schSelectionFilter);
        const pick = (id: string | null | undefined): string | null => (id && selectable(id) ? id : null);
        const sym = pick(hitSymbol(sch, state.cursorUm.x, state.cursorUm.y));
        if (sym) return [sym];
        // Any other placed item (label, text, power symbol, sheet, shape, junction, ...) under the cursor, then a wire.
        const other = hitItems(sch, state.cursorUm.x, state.cursorUm.y, 6 / (state.schematicView.scale || 1)).find((r) => r.kind !== "symbol" && r.kind !== "wire" && pick(r.id));
        if (other) return [other.id];
        const wire = pick(hitWire(sch, state.cursorUm.x, state.cursorUm.y, 400 / (state.schematicView.scale || 1)));
        return wire ? [wire] : [];
      }
      if (state.tab !== "pcb" || !state.board) return [];
      const toleranceUm = Math.max(150, 6 / state.view.scale);
      const cands = pickSelectionCandidates(state.board, state.cursorUm.x, state.cursorUm.y, toleranceUm, 1 / state.view.scale, state.selectionFilter, state.layerVisible, state.activeLayer, state.highContrast, state.selection, false, false);
      return cands[0] ? [cands[0].id] : [];
    };
    /** RequestSelection for dialog/tool hand-offs that read `state.selection`: adopt the hovered item as the selection first. */
    const adoptHovered = (): string[] => {
      const refs = requestSelection();
      if (state.selection.size === 0 && refs.length > 0) dispatch({ type: "SET_SELECTION", refs });
      return refs;
    };
    // The board-control actions (display options, net highlight and ratsnest, zone tools, exports, repair): actions/boardControlActions.ts.
    registerBoardControlActions(m, { state, api, dispatch, pcbOnly, requestSelection });
    /** Delete exactly these refs as ONE undo step (one BOARD_COMMIT::Push in source); locked PCB items are filtered out like `FilterCollectorForLockedItems`. */
    const deleteRefs = (refs: string[]) => {
      const cmds: Cmd[] = [];
      const locked = new Set(state.board?.locked ?? []);
      // `SCH_EDIT_TOOL::DoDelete`: every selectable schematic item, locked ones skipped (kicad-port/schDelete.ts).
      if (state.tab === "schematic" && state.schematic) cmds.push(...deleteCmds(state.schematic, refs, new Set(state.schematic.locked ?? [])));
      for (const id of refs) {
        if (state.tab === "pcb") {
          if (locked.has(id)) continue;
          if (api.trackById(id)) cmds.push({ op: "delete_track", id });
          else if (api.viaById(id)) cmds.push({ op: "delete_via", id });
          else if (api.zoneById(id)) cmds.push({ op: "delete_zone", id });
          else if (api.shapeById(id)) cmds.push({ op: "delete_shape", id });
          else if (api.textById(id)) cmds.push({ op: "delete_text", id });
          else if (api.dimensionById(id)) cmds.push({ op: "delete_dimension", id });
          else if (api.partByRef(id)?.placed) cmds.push({ op: "rip", part: id });
        }
      }
      dispatch({ type: "CLEAR_SELECTION" });
      if (cmds.length) void api.cmdBatch(cmds);
    };

    /**
     * edit_tool.cpp Rotate()/Flip()'s own `m_dragging` branch:
     * during an active Move, R/Shift+R/F act on the live preview instead
     * of committing a separate Rotate/Flip Cmd immediately -- the
     * eventual commit (Canvas.tsx's onPointerUp/onPointerDown "click to
     * place" path, via api.commitMove) applies the accumulated rotation/
     * flip together with the move as one step. Returns false (and does
     * nothing) when no move is active, so the caller falls back to the
     * plain immediate-commit behavior. A mouse-drag that hasn't moved
     * the pointer even once yet (state.movePreview still null,
     * state.activeTool still "select") can't be detected here -- this
     * app's drag state lives in Canvas.tsx's own ref, not the store; see
     * PARITY-pcb.md for this narrow, documented gap.
     */
    const tryTransformDuringMove = (addQuarterTurns: number, toggleFlip: boolean): boolean => {
      const moving = state.activeTool === "move" || state.activeTool === "drag" || state.movePreview != null;
      if (!moving) return false;
      const refs = state.movePreview?.refs ?? [...state.selection];
      if (refs.length === 0) return false;
      const first = refs[0]!;
      const kind = state.movePreview?.kind ?? (api.viaById(first) ? "via" : api.shapeById(first) ? "shape" : api.textById(first) ? "text" : "part");
      const base = state.movePreview ?? { refs, kind, dxUm: 0, dyUm: 0 };
      const rotateQuarterTurns = addQuarterTurns ? (((base.rotateQuarterTurns ?? 0) + addQuarterTurns) % 4 + 4) % 4 : base.rotateQuarterTurns;
      const flipped = toggleFlip ? !base.flipped : base.flipped;
      dispatch({ type: "SET_MOVE_PREVIEW", preview: { ...base, rotateQuarterTurns, flipped } });
      return true;
    };

    /**
     * `SCH_EDIT_TOOL::Rotate` / `Mirror` while a selection is held by `M`, `G` or a click-drag: the turn goes on the held preview (several can
     * follow one another) and is committed with the move as one step, about the point the items are held at. False -- nothing done -- when
     * nothing is held, so the caller turns the selection where it stands.
     */
    const tryHeldTurn = (turn: SchTurn): boolean => {
      const held = state.movePreview != null && (state.movePreview.kind === "sch_move" || state.movePreview.kind === "sch_drag");
      const armed = state.activeTool === "move" || state.activeTool === "drag";
      if (!held && !armed) return false;
      const refs = state.movePreview?.refs ?? [...state.selection];
      if (refs.length === 0) return false;
      const base = held ? state.movePreview! : { refs, kind: state.activeTool === "drag" ? ("sch_drag" as const) : ("sch_move" as const), dxUm: 0, dyUm: 0, vertices: pickedVertices(state) };
      dispatch({ type: "SET_MOVE_PREVIEW", preview: { ...base, turns: [...(base.turns ?? []), turn] } });
      return true;
    };

    m.set(
      "pcbnew.InteractiveEdit.rotateCcw",
      pcbOnly(() => {
        if (!tryTransformDuringMove(1, false)) void api.rotateSelection(1, requestSelection());
      })
    );
    m.set(
      "pcbnew.InteractiveEdit.rotateCw",
      pcbOnly(() => {
        if (!tryTransformDuringMove(3, false)) void api.rotateSelection(3, requestSelection());
      })
    );

    // align_distribute_tool.cpp -- no default hotkey in source either
    // (reached from its own right-click submenu there); this app surfaces
    // them from Canvas.tsx's context menu the same way. Placed footprints
    // only -- see kicad-port/alignDistribute.ts's scope note.
    m.set("pcbnew.AlignAndDistribute.alignTop", pcbOnly(() => api.alignSelection("top")));
    m.set("pcbnew.AlignAndDistribute.alignBottom", pcbOnly(() => api.alignSelection("bottom")));
    m.set("pcbnew.AlignAndDistribute.alignLeft", pcbOnly(() => api.alignSelection("left")));
    m.set("pcbnew.AlignAndDistribute.alignRight", pcbOnly(() => api.alignSelection("right")));
    m.set("pcbnew.AlignAndDistribute.alignCenterX", pcbOnly(() => api.alignSelection("centerX")));
    m.set("pcbnew.AlignAndDistribute.alignCenterY", pcbOnly(() => api.alignSelection("centerY")));
    m.set("pcbnew.AlignAndDistribute.distributeHorizontallyGaps", pcbOnly(() => api.distributeSelection("x", "gaps")));
    m.set("pcbnew.AlignAndDistribute.distributeHorizontallyCenters", pcbOnly(() => api.distributeSelection("x", "centers")));
    m.set("pcbnew.AlignAndDistribute.distributeVerticallyGaps", pcbOnly(() => api.distributeSelection("y", "gaps")));
    m.set("pcbnew.AlignAndDistribute.distributeVerticallyCenters", pcbOnly(() => api.distributeSelection("y", "centers")));

    // common.Interactive.cut (Ctrl+X): trivially "copy then delete" -- the
    // one honorable-mention gap PARITY-pcb.md's hotkey audit named as
    // exactly that. copySelection is synchronous and reads straight off
    // the current board/selection, so there is no race with the delete
    // that follows it.
    // common.Interactive.cut (Ctrl+X) -- edit_tool.cpp copyToClipboard(cut) + DeleteItems(isCut):
    // copies exactly what it then deletes. The clipboard only holds tracks/vias/zones/shapes/text,
    // so footprints and dimensions are neither copied nor deleted (they used to be destroyed
    // un-restorably), and locked items are skipped. Copy happens first, delete is one undo step.
    m.set(
      "common.Interactive.cut",
      pcbOnly(() => {
        const locked = new Set(state.board?.locked ?? []);
        const refs = requestSelection().filter((id) => !locked.has(id) && Boolean(api.trackById(id) || api.viaById(id) || api.zoneById(id) || api.shapeById(id) || api.textById(id)));
        if (refs.length === 0) return;
        api.copySelection(refs);
        deleteRefs(refs);
      })
    );

    // Del: RequestSelection (selection, else the item under the cursor), then ONE commit for all of it.
    m.set("common.Interactive.delete", () => {
      if (state.tab !== "pcb" && state.tab !== "schematic") return;
      // `DrawRuleArea`'s loop: Delete while a rule area is in progress removes its last corner (`deleteLastPoint`) instead of deleting a selection.
      if (state.tab === "schematic" && state.drawState?.kind === "sch_shape" && state.drawState.poly) return deleteLastPoint(state.drawState, dispatch);
      deleteRefs(requestSelection());
    });
    // F is Flip's real KiCad hotkey, but it's also pcbnew.InteractiveRouter.
    // AttemptFinish's while actively routing -- KiCad's own tool stack
    // resolves this by context (which tool currently owns the keyboard),
    // this app's flatter one by only ever registering whichever of the
    // two applies to the current state.drawState, so useGlobalHotkeys'
    // first-enabled-candidate search always lands on the right one.
    if (state.drawState?.kind === "route") {
      const draw = state.drawState;
      const cursor = state.cursorUm ?? { x: draw.pts[draw.pts.length - 1]?.[0] ?? 0, y: draw.pts[draw.pts.length - 1]?.[1] ?? 0 };
      // Re-request the preview at the last known cursor after a backend
      // state change (via armed, a segment undone, width changed) that
      // doesn't itself move the cursor -- same reasoning as Canvas.tsx's
      // own `/` handler.
      const refreshPreview = () => {
        routeMove(cursor.x, cursor.y).then((preview) => {
          if (preview.ok) dispatch({ type: "SET_DRAW_STATE", draw: drawStateFromPreview(draw, preview) });
        });
      };
      m.set(
        "pcbnew.InteractiveRouter.AttemptFinish",
        pcbOnly(() => void finishInteractiveRoute(cursor.x, cursor.y, dispatch, api))
      );
      // V: arm "drop a via here, continue on the other layer" for the next
      // fix (see Router::toggle_via's own doc comment) -- not an immediate
      // commit the way this app's old client-only router made it.
      m.set(
        "pcbnew.Control.layerToggle",
        pcbOnly(() => {
          if (!state.board) return;
          // `layerToggle` (router_tool.cpp): the other layer of the board's layer pair ("Set Layer Pair...", actions/pcbRouterSweep.ts).
          const toLayer = otherLayerOfPair(layerPairsOf(state.board, state.pcbx.layerPairs).current, draw.layer);
          const rules = state.board.board_rules;
          routeToggleVia(!draw.placingVia, rules?.via_diameter ?? 600, rules?.via_drill ?? 300, toLayer).then(() => {
            dispatch({ type: "SET_DRAW_STATE", draw: { ...draw, placingVia: !draw.placingVia, pendingViaLayer: toLayer } });
            refreshPreview();
          });
        })
      );
      m.set(
        "pcbnew.InteractiveRouter.UndoLastSegment",
        pcbOnly(() => {
          routeUndoSegment().then(() => refreshPreview());
        })
      );
      // W / Shift+W: cycle the live track width through a small fixed
      // preset list -- this app has no per-board "preferred track widths"
      // setting the way real pcbnew's dialog does, so the preset is just a
      // reasonable fixed ladder (see WIDTH_PRESETS_UM below).
      const cycleWidth = (dir: 1 | -1) => {
        const i = WIDTH_PRESETS_UM.findIndex((w) => w >= draw.width);
        // board_editor_control.cpp TrackWidthInc/Dec wrap at both ends.
        const next = WIDTH_PRESETS_UM[wrapStep(i, WIDTH_PRESETS_UM.length, dir)]!;
        dispatch({ type: "SET_DRAW_STATE", draw: { ...draw, width: next } });
        routeMove(cursor.x, cursor.y, undefined, next).then((preview) => {
          if (preview.ok) dispatch({ type: "SET_DRAW_STATE", draw: drawStateFromPreview({ ...draw, width: next }, preview) });
        });
      };
      m.set("pcbnew.EditorControl.trackWidthInc", pcbOnly(() => cycleWidth(1)));
      m.set("pcbnew.EditorControl.trackWidthDec", pcbOnly(() => cycleWidth(-1)));
    } else if (state.drawState?.kind === "diffpair") {
      // Same shape as the route branch above, scoped down to what this
      // port's diff pair actually supports (no via/layer-switch, no width
      // cycling -- see crates/pns/src/diff_pair.rs's own doc comment).
      const draw = state.drawState;
      const cursor = state.cursorUm ?? { x: draw.ptsA[draw.ptsA.length - 1]?.[0] ?? 0, y: draw.ptsA[draw.ptsA.length - 1]?.[1] ?? 0 };
      m.set(
        "pcbnew.InteractiveRouter.AttemptFinish",
        pcbOnly(() => void finishDiffPairRoute(cursor.x, cursor.y, dispatch, api))
      );
      m.set(
        "pcbnew.InteractiveRouter.UndoLastSegment",
        pcbOnly(() => {
          dpUndoSegment().then(() =>
            dpMove(cursor.x, cursor.y).then((preview) => {
              if (preview.ok) dispatch({ type: "SET_DRAW_STATE", draw: dpStateFromPreview(draw, preview) });
            })
          );
        })
      );
    } else {
      m.set(
        "pcbnew.InteractiveEdit.flip",
        pcbOnly(() => {
          if (!tryTransformDuringMove(0, true)) void api.flipSelection(requestSelection());
        })
      );
    }
    m.set(
      "pcbnew.InteractiveRouter.SingleTrack",
      pcbOnly(() => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "route" ? "select" : "route" }))
    );
    // `6`: same toggle-arm shape as `X` above.
    m.set(
      "pcbnew.InteractiveRouter.DiffPair",
      pcbOnly(() => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "diffpair" ? "select" : "diffpair" }))
    );
    // `D` (ROUTER_TOOL::InlineDrag): a one-shot action, not a toggle-arm
    // like `X` above -- grab whatever's under the cursor right now (or,
    // failing that, a single already-selected track/via -- see
    // dragging.ts's `findDraggableAt`) and start dragging it immediately.
    // Refuses while another click-to-place session (route/zone/shape/wire/
    // measure) is already mid-flight (`state.drawState`), same as real
    // pcbnew's tool stack never overlapping two interactive placements.
    const startDragAtCursor = (freeAngle: boolean) => () => {
      if (!state.board || !state.cursorUm || state.drawState) return;
      const toleranceUm = Math.max(150, 6 / state.view.scale);
      const onePixelUm = 1 / state.view.scale;
      const hit = findDraggableAt(state.board, state.selection, state.cursorUm.x, state.cursorUm.y, toleranceUm, onePixelUm, state.selectionFilter, state.layerVisible, state.activeLayer, state.highContrast);
      if (!hit) {
        dispatch({ type: "TOAST", message: "Nothing to drag there -- hover a track or via first.", kind: "error" });
        return;
      }
      void startInlineDrag(state.cursorUm.x, state.cursorUm.y, hit, state.board, state.routerSettings.mode, dispatch, freeAngle);
    };
    m.set("pcbnew.InteractiveRouter.Drag45Degree", pcbOnly(startDragAtCursor(false)));
    // `routerInlineDrag` (EDIT_TOOL::invokeInlineRouter -> `RunAction( PCB_ACTIONS::routerInlineDrag, DM_ANY )`): the router's own drag entry point, the one `D` above ends in.
    m.set("pcbnew.InteractiveRouter.InlineDrag", pcbOnly(startDragAtCursor(false)));
    // `G` (EDIT_TOOL::Drag with `dragFreeAngle` -> `invokeInlineRouter( PNS::DM_ANY | PNS::DM_FREE_ANGLE )`,
    // router_tool.cpp InlineDrag -> DRAGGER free-angle mode): the same grab as `D`, but the router only
    // marks obstacles (crates/pns/src/dragger.rs "Free-angle mode"). `CanInlineDrag` refuses footprints for
    // a free-angle drag ("Footprints cannot be dragged freely"), and `findDraggableAt` only ever finds a
    // track or via. A selection hover is the same RequestSelection fallback `D` uses.
    m.set("pcbnew.InteractiveRouter.DragFreeAngle", pcbOnly(startDragAtCursor(true)));
    // `Ctrl+<` (dialog_pns_settings.cpp): mode/remove-redundant-tracks,
    // read fresh by the next `X`/`D` session start -- see
    // `state.routerSettings`'s own doc comment on why this isn't live
    // mid-route the way upstream's dialog is.
    m.set("pcbnew.InteractiveRouter.SettingsDialog", pcbOnly(() => dispatch({ type: "SET_ROUTER_SETTINGS_DIALOG_OPEN", open: true })));
    // `7` (gap #7 task item 4): length tuning -- see
    // components/LengthTuningDialog.tsx's own header comment on why this
    // is a dialog rather than a fourth interactive session. Needs a
    // single straight track selected first (the dialog itself explains
    // this when opened with nothing/the wrong thing selected, same as
    // moveExact's own "only meaningful with something selected" gate).
    // `8` / `9` (`PNS::DP_MEANDER_PLACER` / `PNS::MEANDER_SKEW_PLACER`): the same dialog, in pair / skew mode -- the clicked track picks the pair
    // (`Start( aP, aStartItem )`: "Please select a track whose length you want to tune"), so like `7` they start from the selected -- or, with
    // nothing selected, the hovered -- track.
    const startTuner = (mode: "single" | "diffpair" | "skew") =>
      pcbOnly(() => {
        const refs = requestSelection();
        if (refs.length !== 1 || !api.trackById(refs[0]!)) return;
        if (state.selection.size === 0) dispatch({ type: "SET_SELECTION", refs });
        dispatch({ type: "SET_LENGTH_TUNING_DIALOG_OPEN", open: true, mode });
      });
    m.set("pcbnew.LengthTuner.TuneSingleTrack", startTuner("single"));
    m.set("pcbnew.LengthTuner.TuneDiffPair", startTuner("diffpair"));
    m.set("pcbnew.LengthTuner.TuneDiffPairSkew", startTuner("skew"));
    // `tracks_cleaner.cpp` (task item 1): "Cleanup Tracks & Vias..." --
    // see components/CleanupTracksDialog.tsx. No selection gate (unlike
    // LengthTuner above) -- source's own dialog opens unconditionally and
    // scans the whole board.
    m.set("pcbnew.GlobalEdit.cleanupTracksAndVias", pcbOnly(() => dispatch({ type: "SET_CLEANUP_TRACKS_DIALOG_OPEN", open: true })));
    // `dialog_global_edit_tracks_and_vias.cpp` / `dialog_global_edit_text_
    // and_graphics.cpp` (task item 2) -- see GlobalEditTracksAndViasDialog.tsx
    // / GlobalEditTextAndGraphicsDialog.tsx for scope.
    // `DIALOG_BOARD_STATISTICS` (read-only) / `DIALOG_SWAP_LAYERS` -- see
    // components/BoardStatisticsDialog.tsx / SwapLayersDialog.tsx.
    m.set("pcbnew.InspectionTool.ShowBoardStatistics", pcbOnly(() => dispatch({ type: "SET_BOARD_STATISTICS_DIALOG_OPEN", open: true })));
    m.set("pcbnew.GlobalEdit.swapLayers", pcbOnly(() => dispatch({ type: "SET_SWAP_LAYERS_DIALOG_OPEN", open: true })));
    m.set("pcbnew.GlobalEdit.editTracksAndVias", pcbOnly(() => dispatch({ type: "SET_EDIT_TRACKS_AND_VIAS_DIALOG_OPEN", open: true })));
    m.set("pcbnew.GlobalEdit.editTextAndGraphics", pcbOnly(() => dispatch({ type: "SET_EDIT_TEXT_AND_GRAPHICS_DIALOG_OPEN", open: true })));
    // Ctrl+T (task item 6): `ARRAY_TOOL::CreateArray`'s own
    // `if (selection.Empty()) return 0;` guard -- see CreateArrayDialog.tsx.
    m.set(
      "pcbnew.Array.createArray",
      pcbOnly(() => {
        if (state.selection.size === 0) return;
        dispatch({ type: "SET_CREATE_ARRAY_DIALOG_OPEN", open: true });
      })
    );
    m.set(
      "pcbnew.InteractiveDrawing.via",
      pcbOnly(() => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "via" ? "select" : "via" }))
    );
    // Rule Areas (task item 3): the same outline-drawing path as "Draw
    // Filled Zones" below -- a rule area and a copper-pour zone share one
    // outline tool and one properties dialog in source too
    // (dialog_copper_zones.cpp's single `IsRuleArea()`-branching panel).
    // `state.nextZoneIsRuleArea` is the one bit telling ZoneDialog.tsx
    // which of the two armed it, so a fresh outline's dialog opens with
    // "Rule area" pre-checked only for this entry.
    m.set(
      "pcbnew.InteractiveDrawing.ruleArea",
      pcbOnly(() => {
        dispatch({ type: "SET_NEXT_ZONE_IS_RULE_AREA", value: true });
        dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "zone" ? "select" : "zone" });
      })
    );
    m.set(
      "pcbnew.InteractiveDrawing.zone",
      pcbOnly(() => {
        dispatch({ type: "SET_NEXT_ZONE_IS_RULE_AREA", value: false });
        dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "zone" ? "select" : "zone" });
      })
    );
    // Task item 7: `pcbnew/tools/drawing_tool.cpp`'s `DrawDimension`,
    // scoped to a plain two-click (start, end) placement for every kind
    // -- see DimensionPropertiesDialog.tsx and PARITY-pcb.md section 18
    // for why height/leader-length/orientation are dialog-set afterward
    // rather than a third interactive "set height" click the way source
    // has for Aligned/Orthogonal.
    const armDimension = (kind: CmdDimensionKind["kind"]) => {
      dispatch({ type: "SET_NEXT_DIMENSION_KIND", kind });
      dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "dimension" ? "select" : "dimension" });
    };
    m.set("pcbnew.InteractiveDrawing.alignedDimension", pcbOnly(() => armDimension("aligned")));
    m.set("pcbnew.InteractiveDrawing.orthogonalDimension", pcbOnly(() => armDimension("orthogonal")));
    m.set("pcbnew.InteractiveDrawing.radialDimension", pcbOnly(() => armDimension("radial")));
    m.set("pcbnew.InteractiveDrawing.leader", pcbOnly(() => armDimension("leader")));
    m.set("pcbnew.InteractiveDrawing.centerDimension", pcbOnly(() => armDimension("center")));
    // `GLOBAL_EDIT_TOOL`'s "Switch Dimension Arrows": flips inward/outward
    // on every selected dimension at once, same single-field-toggle shape
    // as `common.Interactive.cut`'s own per-ref loop above.
    m.set(
      "pcbnew.InteractiveDrawing.changeDimensionArrows",
      pcbOnly(() => {
        for (const id of state.selection) {
          const dim = api.dimensionById(id);
          if (!dim) continue;
          const cmdDim = toCmdDimension(dim);
          cmdDim.arrow_direction = cmdDim.arrow_direction === "inward" ? "outward" : "inward";
          api.cmd({ op: "edit_dimension", id, dimension: cmdDim });
        }
      })
    );
    m.set(
      "pcbnew.InteractiveDrawing.line",
      pcbOnly(() => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "draw_segment" ? "select" : "draw_segment" }))
    );
    m.set(
      "pcbnew.InteractiveDrawing.arc",
      pcbOnly(() => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "draw_arc" ? "select" : "draw_arc" }))
    );
    m.set(
      "pcbnew.InteractiveDrawing.rectangle",
      pcbOnly(() => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "draw_rect" ? "select" : "draw_rect" }))
    );
    m.set(
      "pcbnew.InteractiveDrawing.circle",
      pcbOnly(() => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "draw_circle" ? "select" : "draw_circle" }))
    );
    m.set(
      "pcbnew.InteractiveDrawing.graphicPolygon",
      pcbOnly(() => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "draw_polygon" ? "select" : "draw_polygon" }))
    );
    m.set(
      "pcbnew.InteractiveDrawing.text",
      pcbOnly(() => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "text" ? "select" : "text" }))
    );
    // pcb_viewer_tools.cpp's measure tool -- a client-side-only ruler
    // (state.drawState's "measure" kind), same arm/toggle pattern every
    // other click-to-place tool here uses.
    m.set(
      "common.Interactive.measureTool",
      pcbOnly(() => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "measure" ? "select" : "measure" }))
    );
    // Undo/Redo run in the active editor's OWN scope: the Footprint and Symbol editors have their
    // own undo domains (footprint_editor / symbol_editor); only the PCB/Schematic/3D tabs use the board's.
    m.set("common.Interactive.undo", () => void (state.tab === "footprint" ? fpApi.undo() : state.tab === "symbol" ? symApi.undo() : api.undo()));
    m.set("common.Interactive.redo", () => void (state.tab === "footprint" ? fpApi.redo() : state.tab === "symbol" ? symApi.redo() : api.redo()));
    // EDIT_TOOL::Duplicate: the board editor duplicates tracks/vias/zones/shapes/text; the footprint editor
    // pads/graphics/text (state/footprintEditorStore.tsx `duplicateSelection`).
    m.set("common.Interactive.duplicate", () => {
      if (state.tab === "pcb") void api.duplicateSelection();
      else if (state.tab === "footprint") void fpApi.duplicateSelection(false);
    });
    m.set("common.Interactive.copy", pcbOnly(() => api.copySelection()));
    m.set("common.Interactive.paste", pcbOnly(() => api.pasteClipboard()));
    // Task item 5: common/tool/group_tool.cpp (Ctrl+G/Ctrl+Shift+G -- see
    // useGlobalHotkeys.ts's own special-cased binding for why those two
    // hotkeys are hardcoded there instead of read from actions.json).
    m.set("common.Interactive.group", pcbOnly(() => api.groupSelection()));
    m.set("common.Interactive.ungroup", pcbOnly(() => api.ungroupSelection()));
    // `EnterGroup`: only fires for a single selected group, matching
    // source's own `selection.GetSize() == 1 && selection[0]->Type() ==
    // PCB_GROUP_T` guard. `LeaveGroup`: re-selects the group itself,
    // matching `ExitGroup(true /* Select the group */)`.
    m.set(
      "common.Interactive.groupEnter",
      pcbOnly(() => {
        const refs = [...state.selection];
        if (refs.length === 1 && api.groupById(refs[0]!)) dispatch({ type: "SET_ENTERED_GROUP", id: refs[0]! });
      })
    );
    m.set(
      "common.Interactive.groupLeave",
      pcbOnly(() => {
        const leftId = state.enteredGroupId;
        dispatch({ type: "SET_ENTERED_GROUP", id: null });
        if (leftId) dispatch({ type: "SET_SELECTION", refs: [leftId] });
      })
    );
    m.set(
      "pcbnew.InteractiveEdit.moveExact",
      pcbOnly(() => {
        // Only meaningful with something placed/selected to move -- a
        // footprint, or any of item 7's track/via/zone/shape/text (the
        // dialog itself, MoveExactDialog.tsx, resolves which and reads
        // its own position/bbox fresh when it opens).
        if (adoptHovered().length === 0) return;
        dispatch({ type: "SET_MOVE_EXACT_DIALOG_OPEN", open: true });
      })
    );

    m.set(
      "pcbnew.EditorControl.toggleNetHighlight",
      pcbOnly(() => {
        const ref = [...state.selection][0];
        const part = ref ? api.partByRef(ref) : undefined;
        const net = part?.pads?.[0]?.net ?? null;
        dispatch({ type: "SET_NET_HIGHLIGHT", net: state.netHighlight ? null : net });
      })
    );
    // board_inspection_tool.cpp BOARD_INSPECTION_TOOL::highlightNet, the
    // real "`" key (cursor-driven, NOT selection-driven -- a different
    // action, `highlightNetSelection`, is the selection-based one, and
    // this app has no hotkey/menu entry point for it since the plain "`"
    // is what the task names): find the net under the cursor (pads/vias/
    // tracks preferred, zones only as a fallback) and toggle it -- the
    // same net clicked twice clears the highlight, a different one
    // replaces it, nothing under the cursor clears it.
    m.set(
      "pcbnew.EditorControl.highlightNet",
      pcbOnly(() => {
        if (!state.board || !state.cursorUm) return;
        const toleranceUm = 10 / (state.view.scale || 1);
        const net = findNetAtCursor(state.board, state.cursorUm.x, state.cursorUm.y, toleranceUm);
        dispatch({ type: "SET_NET_HIGHLIGHT", net: net && net === state.netHighlight ? null : net });
      })
    );
    // board_inspection_tool.cpp BOARD_INSPECTION_TOOL::ClearHighlight ("~").
    m.set("pcbnew.EditorControl.clearHighlight", pcbOnly(() => dispatch({ type: "SET_NET_HIGHLIGHT", net: null })));

    // pcb_selection_tool.cpp expandConnection ("U" -- Select/Expand
    // Connection): seed points from every currently-selected track/via's
    // own endpoints plus every pad of a selected footprint, each on its
    // own net, and flood outward (kicad-port/expandConnection.ts).
    m.set(
      "pcbnew.InteractiveSelection.SelectConnection",
      pcbOnly(() => {
        const refs = computeConnectionRefs(state.selection);
        if (refs) dispatch({ type: "SET_SELECTION", refs });
      })
    );
    /**
     * The body of `SelectConnection` above, factored out so `deleteFull`
     * (`pcbnew.InteractiveEdit.deleteFull`, whose `REMOVE_FLAGS::ALT` runs
     * `PCB_ACTIONS::selectConnection` before deleting) expands exactly the
     * same way: `selection` plus everything `expandConnection` adds, or null
     * when nothing in it can seed a flood.
     */
    function computeConnectionRefs(selection: ReadonlySet<string>): string[] | null {
      const board = state.board;
      if (!board) return null;
      const tracks: ConnTrack[] = (board.routing?.tracks ?? []).map((t) => ({ id: t.id, net: t.net, start: t.pts[0]!, end: t.pts[t.pts.length - 1]! })).filter((t) => t.start && t.end);
      const vias: ConnVia[] = (board.routing?.vias ?? []).map((v) => ({ id: v.id, net: v.net, at: [v.x, v.y] }));
      const startPoints: StartPoint[] = [];
      const selectedTrackIds: string[] = [];
      const selectedViaIds: string[] = [];
      for (const id of selection) {
        const t = api.trackById(id);
        const v = api.viaById(id);
        const p = api.partByRef(id);
        if (t) {
          selectedTrackIds.push(id);
          startPoints.push({ point: t.pts[0]!, net: t.net }, { point: t.pts[t.pts.length - 1]!, net: t.net });
        } else if (v) {
          selectedViaIds.push(id);
          startPoints.push({ point: [v.x, v.y], net: v.net });
        } else if (p?.placed) {
          for (const pad of p.pads ?? []) if (pad.net) startPoints.push({ point: [pad.x, pad.y], net: pad.net });
        }
      }
      if (startPoints.length === 0) return null;
      const result = expandConnection(tracks, vias, startPoints, { trackIds: selectedTrackIds, viaIds: selectedViaIds });
      const refs = new Set(selection);
      for (const id of result.trackIds) refs.add(id);
      for (const id of result.viaIds) refs.add(id);
      return [...refs];
    }

    const fitToBoard = viewTab(() => {
      const rect = canvasRect();
      if (!rect) return;
      if (state.tab === "schematic") {
        // Schematic: the page frame widened to the content (same bounds as the initial fit).
        const sb = state.schematic ? boundsOfPoints(schematicBounds(state.schematic)) : null;
        if (sb) setView(fitTransform(sb, rect.width, rect.height, 80));
        return;
      }
      const bounds = state.board?.outline ? boundsOfPoints(state.board.outline) : null;
      if (!bounds) return;
      setView(fitTransform(bounds, rect.width, rect.height));
    });
    // common_tools.cpp ZoomFitScreen (ZOOM_FIT_ALL -- the worksheet page,
    // or the edited object if there's no page) vs. ZoomFitObjects
    // (ZOOM_FIT_OBJECTS -- everything on screen): two different fit
    // targets in source. This app has no separate worksheet-page concept
    // to distinguish them, so both fit to the same thing -- the board
    // outline -- same as this already did for zoomFitScreen alone.
    m.set("common.Control.zoomFitScreen", fitToBoard);
    m.set("common.Control.zoomFitObjects", fitToBoard);

    // common_tools.cpp doZoomInOut/doZoomInOutCenter: the *Center variants
    // anchor at the view's center (an unmoving zoom); zoomIn/zoomOut (F1/
    // F2, or Cmd+'+'/Cmd+'-' on macOS -- actions.json's real macHotkey
    // now that tools/lib/actionsParser.js's #if __WXMAC__ parsing is
    // fixed) anchor at the cursor instead ("Zoom In/Out at Cursor",
    // doZoomToPreset's `SetScale(scale, cursorPosition)` when
    // aCenterOnCursor is true). `state.cursorUm` is the last known cursor
    // world position (Canvas.tsx's pointer-move handler); converting it
    // back through the CURRENT view gives back the actual screen pixel it
    // was under, since nothing else can have moved the view in between a
    // pointer-move and this hotkey firing synchronously.
    const zoomAtCenter = viewTab((factor: number) => {
      const rect = canvasRect();
      if (!rect) return;
      setView(zoomAbout(curView(), rect.width / 2, rect.height / 2, factor));
    });
    const zoomAtCursor = viewTab((factor: number) => {
      const rect = canvasRect();
      if (!rect) return;
      const [px, py] = state.cursorUm ? worldToScreen(curView(), state.cursorUm.x, state.cursorUm.y) : [rect.width / 2, rect.height / 2];
      setView(zoomAbout(curView(), px, py, factor));
    });
    m.set("common.Control.zoomInCenter", () => zoomAtCenter(ZOOM_STEP_FACTOR));
    m.set("common.Control.zoomOutCenter", () => zoomAtCenter(1 / ZOOM_STEP_FACTOR));
    m.set("common.Control.zoomIn", () => zoomAtCursor(ZOOM_STEP_FACTOR));
    m.set("common.Control.zoomOut", () => zoomAtCursor(1 / ZOOM_STEP_FACTOR));

    // common_tools.cpp ZoomCenter: `getViewControls()->CenterOnCursor()`
    // -- pans so the cursor's current world point becomes the new view
    // center, AND warps the real pointer to the screen center so it still
    // visually sits over the same spot. This app never moves the real
    // pointer (hard rule, and not something a web page can do anyway), so
    // only the pan half happens: the content jumps to center under a
    // pointer that stays where it was. See PARITY-pcb.md.
    m.set(
      "common.Control.zoomCenter",
      viewTab(() => {
        const rect = canvasRect();
        if (!rect || !state.cursorUm) return;
        const [centerWx, centerWy] = screenToWorld(curView(), rect.width / 2, rect.height / 2);
        setView(panByWorldDelta(curView(), state.cursorUm.x - centerWx, state.cursorUm.y - centerWy));
      })
    );
    // common_tools.cpp ZoomRedraw: `m_frame->HardRedraw()` -- forces a
    // full repaint of possibly-stale cached GAL layers. This app has no
    // such cache (every render reads current state directly), so there is
    // nothing for "redraw" to actually do -- registered as a real no-op
    // rather than left unimplemented, since the action itself is always
    // trivially satisfied here, not missing.
    m.set("common.Control.zoomRedraw", () => {});
    m.set("common.SuiteControl.listHotKeys", () => dispatch({ type: "SET_HOTKEYS_DIALOG_OPEN", open: true }));

    // common_tools.cpp ResetLocalCoords: sets the status bar's dx/dy/dist
    // origin to wherever the cursor currently is (Space). Independent of
    // the move tool, active or not -- see state/store.tsx's
    // localOriginUm doc.
    m.set(
      "common.Control.resetLocalCoords",
      pcbOnly(() => {
        if (state.cursorUm) dispatch({ type: "SET_LOCAL_ORIGIN", at: state.cursorUm });
      })
    );

    // PCB_EDIT_FRAME::ToggleLayersManager: shows or hides the Appearance dock (here its column folds to a handle, kicad-port/dockLayout.ts); showing it picks the Appearance tab.
    m.set("pcbnew.Control.showLayersManager", () => {
      const layout = getDockLayout();
      if (!layout.rightCollapsed && state.rightDockTab === "appearance") {
        setDockColumnCollapsed("right", true);
        return;
      }
      dispatch({ type: "SET_RIGHT_DOCK_TAB", tab: "appearance" });
      setDockColumnCollapsed("right", false);
    });
    // ACTIONS::showProperties (`ToggleProperties`): shows or hides the Properties pane (the board editor's and the schematic's).
    m.set("common.Control.showProperties", () => {
      if (state.tab === "pcb" || state.tab === "schematic") toggleDockPane("properties");
    });

    m.set(
      "pcbnew.InteractiveMove.move",
      pcbOnly(() => {
        const first = requestSelection()[0];
        if (!first) return;
        // Tracks and zones have no move_* Cmd (api/types.ts) -- nothing
        // for M to do for them, same as they're excluded from dragging
        // in Canvas.tsx's onPointerDown.
        if (api.trackById(first) || api.zoneById(first)) return;
        adoptHovered();
        dispatch({ type: "SET_ACTIVE_TOOL", tool: "move" });
        dispatch({ type: "SET_MOVE_ORIGIN", at: state.cursorUm });
      })
    );
    // pcb_selection_tool.cpp's IsCancel() handler (see state/store.tsx's
    // "ESCAPE" reducer case for the full tiered semantics this replaced
    // a plain CLEAR_SELECTION with: an in-progress move/draw/arm cancels
    // itself first *without* touching the selection; only once nothing
    // is running does Escape clear the selection, and only once that's
    // also empty does it clear the net highlight).
    m.set("common.Interactive.cancel", () => {
      // PICKER_TOOL::Main: Escape ends a running pick session (reference point, offset tool, a dialog's "Select ...") and nothing else.
      if (picker.cancel() || cancelAreaTool()) return;
      // Tell the backend's router session to end too (fire-and-forget --
      // see cancelInteractiveRoute's own doc comment) before the ordinary
      // ESCAPE reducer case clears `drawState` locally; otherwise the
      // session would linger server-side until the next `start`/
      // `drag_start` silently replaces it. One backend call covers both
      // kinds (`POST /api/route/cancel` drops whatever's active on the
      // shared `Router`), so route/drag/diff-pair all share this one branch.
      if (state.drawState?.kind === "route" || state.drawState?.kind === "drag" || state.drawState?.kind === "diffpair") cancelInteractiveRoute(dispatch);
      // RouteSelected's loop (`m_cancelled = true` when Escape arrives while `m_inRouteSelected`): the whole run ends and the tool is popped.
      if (clearRouteQueue()) dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
      dispatch({ type: "ESCAPE" });
    });

    m.set(
      "pcbnew.InteractiveEdit.properties",
      pcbOnly(() => {
        // "E" opens whichever properties view actually applies to what's
        // selected -- a real Text gets the full edit_text-backed dialog
        // (item 6's own "E to edit"); everything else this app models
        // (a part, or item 7's track/via/zone/shape) is read-only-ish, so
        // it keeps the existing footprint-properties dialog's pattern.
        // Shared with Canvas.tsx's double-click (properties.ts) so the
        // two can never disagree.
        const ref = requestSelection()[0];
        if (ref) openPropertiesFor(ref, api, dispatch);
      })
    );
    m.set("pcbnew.DRCTool.runDRC", () => dispatch({ type: "SET_DRC_OPEN", open: true }));

    // One window, three tabs (unlike KiCad's separate windows) -- these
    // just jump tabs; App.tsx swaps each tab's own toolbars/menus/panels.
    m.set("pcbnew.EditorControl.showEeschema", () => dispatch({ type: "SET_TAB", tab: "schematic" }));
    m.set("common.Control.show3DViewer", () => dispatch({ type: "SET_TAB", tab: "3d" }));

    // Display-option toggles that were real state but had no menu/
    // hotkey/toolbar entry point yet (only the Appearance panel's own
    // checkboxes reached them) -- wiring the real KiCad action name to
    // the same existing dispatch is what actually surfaces them in the
    // menu bar and the hotkeys list.
    m.set("common.Control.toggleGrid", () => dispatch({ type: "TOGGLE_GRID_VISIBLE" }));
    m.set("pcbnew.Control.showRatsnest", () => dispatch({ type: "TOGGLE_RATSNEST" }));
    m.set("pcbnew.Control.ratsnestLineMode", () => dispatch({ type: "TOGGLE_RATSNEST_CURVED" }));
    // The real action is a 3-state cycle (Normal/Dimmed/Off); this app's
    // high-contrast is a plain on/off, so this simplifies to a toggle
    // rather than inventing a third state painter.ts doesn't implement.
    m.set("common.Control.highContrastModeCycle", () => dispatch({ type: "TOGGLE_HIGH_CONTRAST" }));
    m.set("common.Control.togglePolarCoords", () => dispatch({ type: "TOGGLE_POLAR" }));
    // cursorSmallCrosshairs / cursorFullCrosshairs / cursor45Crosshairs: actions/commonActions.ts (one setting, all four canvases).

    m.set("common.Control.metricUnits", () => dispatch({ type: "SET_UNITS", units: "mm" }));
    m.set("common.Control.imperialUnits", () => dispatch({ type: "SET_UNITS", units: "in" }));
    m.set("common.Control.mils", () => dispatch({ type: "SET_UNITS", units: "mil" }));
    // Real KiCad toggles between its last-used metric/imperial unit; this
    // app has a third (mil), folded into "imperial" for this one action.
    m.set("common.Control.toggleUnits", () => dispatch({ type: "SET_UNITS", units: state.units === "mm" ? "in" : "mm" }));

    m.set("pcbnew.Control.padDisplayMode", () => dispatch({ type: "TOGGLE_SKETCH_PADS" }));
    m.set("pcbnew.Control.trackDisplayMode", () => dispatch({ type: "TOGGLE_SKETCH_TRACKS" }));
    m.set("pcbnew.Control.viaDisplayMode", () => dispatch({ type: "TOGGLE_SKETCH_VIAS" }));

    // pcb_control.cpp LayerNext/LayerPrev ("+"/"-"): step the active
    // layer through the copper stack in UI order, skipping hidden
    // layers, wrapping around, a no-op (source: wxBell()) if every other
    // copper layer is hidden. Source jumps straight to B.Cu/F.Cu when the
    // active layer isn't a copper one at all; this app's activeLayer can
    // be a non-copper layer (or null) via the Appearance panel, so that
    // fallback is reachable here too, not just a defensive branch.
    const cycleActiveLayer = (dir: 1 | -1) => {
      const layers = state.board?.layers ?? [];
      if (layers.length === 0) return;
      const visible = (l: string) => state.layerVisible[l] !== false;
      const cur = state.activeLayer;
      if (cur == null || !layers.includes(cur)) {
        dispatch({ type: "SET_ACTIVE_LAYER", layer: (dir === 1 ? layers[layers.length - 1] : layers[0]) ?? null });
        return;
      }
      const i = layers.indexOf(cur);
      for (let step = 1; step <= layers.length; step++) {
        const j = ((i + dir * step) % layers.length + layers.length) % layers.length;
        if (visible(layers[j]!)) {
          dispatch({ type: "SET_ACTIVE_LAYER", layer: layers[j]! });
          return;
        }
      }
      // every other copper layer is hidden -- source rings the bell and does nothing.
    };
    m.set("pcbnew.Control.layerNext", pcbOnly(() => cycleActiveLayer(1)));
    m.set("pcbnew.Control.layerPrev", pcbOnly(() => cycleActiveLayer(-1)));

    // pcb_control.cpp LayerAlphaInc/Dec ("}"/"{"): the real constants
    // (`#define ALPHA_MIN 0.20` / `ALPHA_MAX 1.00` / `ALPHA_STEP 0.05`),
    // applied to whichever layer is currently active.
    const ALPHA_MIN = 0.2,
      ALPHA_MAX = 1.0,
      ALPHA_STEP = 0.05;
    const stepActiveLayerAlpha = (delta: number) => {
      const layer = state.activeLayer;
      if (!layer) return;
      const cur = state.layerOpacity[layer] ?? 1;
      const next = Math.min(ALPHA_MAX, Math.max(ALPHA_MIN, Math.round((cur + delta) * 100) / 100));
      if (next !== cur) dispatch({ type: "SET_LAYER_OPACITY", layer, opacity: next });
    };
    m.set("pcbnew.Control.layerAlphaInc", pcbOnly(() => stepActiveLayerAlpha(ALPHA_STEP)));
    m.set("pcbnew.Control.layerAlphaDec", pcbOnly(() => stepActiveLayerAlpha(-ALPHA_STEP)));

    // zone_filler_tool.cpp ZoneFillAll/ZoneUnfillAll (B/Ctrl+B): this
    // app's /api/fill is always computed fresh (no per-zone fill cache to
    // mutate), so "fill" is just "go fetch it", and "unfill" is just
    // "stop showing what we fetched" -- see state.zoneFill's own doc.
    // Fill All covers every zone, so it also forgets a draft fill of just some (`bcx.zoneFilled`, see ZoneFiller.zoneFill in boardControlActions.ts).
    m.set(
      "pcbnew.ZoneFiller.zoneFillAll",
      pcbOnly(() => {
        dispatch({ type: "BCX", patch: { zoneFilled: null } });
        void api.fillZones();
      })
    );
    m.set("pcbnew.ZoneFiller.zoneUnfillAll", pcbOnly(() => api.unfillZones()));
    // pcb_control.cpp ZoneDisplayMode: independent of whether a zone HAS
    // fill data at all (above) -- how one that does paints. Source's
    // other two modes (fracture-borders/triangulation) are developer
    // debug views, not ported -- see painter.ts's drawZones doc.
    m.set("pcbnew.Control.zoneDisplayEnable", pcbOnly(() => dispatch({ type: "SET_ZONE_DISPLAY_MODE", mode: "filled" })));
    m.set("pcbnew.Control.zoneDisplayDisable", pcbOnly(() => dispatch({ type: "SET_ZONE_DISPLAY_MODE", mode: "outline" })));
    m.set(
      "pcbnew.Control.zoneDisplayToggle",
      pcbOnly(() => dispatch({ type: "SET_ZONE_DISPLAY_MODE", mode: state.zoneDisplayMode === "filled" ? "outline" : "filled" }))
    );

    // pcbnew.EditorControl.trackWidthInc/Dec (W/Shift+W): BOARD_DESIGN_
    // SETTINGS' real behavior is dual-purpose -- step the board's own
    // "current" width (state.currentTrackWidthUm, read by Canvas.tsx's
    // route tool for the *next* track) AND, if anything is selected,
    // apply the new width to every selected track in the same keypress
    // (so W on an already-drawn track resizes it in place, not just the
    // next one you draw).
    const trackWidthList = (): number[] => {
      const board = state.board;
      const base = board?.board_rules?.track_width ?? 250;
      return [base, ...(board?.routing?.track_width_presets ?? [])];
    };
    // board_editor_control.cpp TrackWidthInc/Dec + ViaSizeInc/Dec. Idle (no tool running) with ONLY
    // tracks/vias selected: each track/via moves to the next larger (smaller) preset above ITS OWN
    // size, one commit, current width untouched. Otherwise the board's current index steps and WRAPS
    // (`if (widthIndex >= size) widthIndex = 0` / `if (< 0) widthIndex = size - 1`).
    const toolIdle = !state.drawState && state.activeTool === "select";
    const selOnlyTracksVias = (): string[] | null => {
      const ids = [...state.selection];
      return ids.length > 0 && ids.every((id) => api.trackById(id) || api.viaById(id)) ? ids : null;
    };
    const cycleTrackWidth = (dir: 1 | -1) => {
      const list = trackWidthList();
      const sel = toolIdle ? selOnlyTracksVias() : null;
      if (sel) {
        const pick = dir > 0 ? nextLargerPreset : nextSmallerPreset;
        const cmds: Cmd[] = [];
        for (const id of sel) {
          const t = api.trackById(id);
          if (!t) continue;
          const w = pick(list, (x) => x, t.width);
          if (w != null) cmds.push({ op: "set_track_width", id, width: w });
        }
        void api.cmdBatch(cmds);
        return;
      }
      const cur = state.currentTrackWidthUm ?? list[0]!;
      const next = list[wrapStep(list.indexOf(cur), list.length, dir)]!;
      dispatch({ type: "SET_CURRENT_TRACK_WIDTH", widthUm: next });
    };
    if (state.drawState?.kind !== "route") {
      m.set("pcbnew.EditorControl.trackWidthInc", pcbOnly(() => cycleTrackWidth(1)));
      m.set("pcbnew.EditorControl.trackWidthDec", pcbOnly(() => cycleTrackWidth(-1)));
    }

    // pcbnew.EditorControl.viaSizeInc/Dec ("\\"/unbound): same rules for `routing.via_presets`.
    const viaPresetList = (): { diameter: number; drill: number }[] => {
      const board = state.board;
      const base = { diameter: board?.board_rules?.via_diameter ?? 600, drill: board?.board_rules?.via_drill ?? 300 };
      return [base, ...(board?.routing?.via_presets ?? [])];
    };
    const sameViaPreset = (a: { diameter: number; drill: number }, b: { diameter: number; drill: number }) => a.diameter === b.diameter && a.drill === b.drill;
    const cycleViaPreset = (dir: 1 | -1) => {
      const list = viaPresetList();
      const sel = toolIdle ? selOnlyTracksVias() : null;
      if (sel) {
        const pick = dir > 0 ? nextLargerPreset : nextSmallerPreset;
        const cmds: Cmd[] = [];
        for (const id of sel) {
          const v = api.viaById(id);
          if (!v) continue;
          const p = pick(list, (x) => x.diameter, v.d);
          if (p) cmds.push({ op: "edit_via", id, diameter: p.diameter, drill: p.drill });
        }
        void api.cmdBatch(cmds);
        return;
      }
      const cur = state.currentViaPreset ?? list[0]!;
      const next = list[wrapStep(list.findIndex((p) => sameViaPreset(p, cur)), list.length, dir)]!;
      dispatch({ type: "SET_CURRENT_VIA_PRESET", preset: next });
    };
    m.set("pcbnew.EditorControl.viaSizeInc", pcbOnly(() => cycleViaPreset(1)));
    m.set("pcbnew.EditorControl.viaSizeDec", pcbOnly(() => cycleViaPreset(-1)));

    // dialog_board_setup.cpp -- see BoardSetupDialog.tsx.
    m.set("pcbnew.EditorControl.boardSetup", pcbOnly(() => dispatch({ type: "SET_BOARD_SETUP_DIALOG_OPEN", open: true })));
    // Task item 4: opens the same Board Setup dialog, landing on its new
    // Teardrops page directly instead of making the user click there.
    m.set(
      "pcbnew.GlobalEdit.editTeardrops",
      pcbOnly(() => {
        dispatch({ type: "SET_BOARD_SETUP_INITIAL_PAGE", page: "teardrops" });
        dispatch({ type: "SET_BOARD_SETUP_DIALOG_OPEN", open: true });
      })
    );

    // File > Fabrication Outputs -- dialog_plot.cpp / dialog_gendrill.cpp /
    // dialog_gen_footprint_position.cpp, see PlotDialog.tsx/
    // GenerateDrillDialog.tsx/FootprintPositionDialog.tsx. `common.Control.plot`
    // is pcbnew's plain "Plot..." menu item, which opens the same dialog
    // generateGerbers does in real KiCad.
    m.set("pcbnew.EditorControl.generateGerbers", pcbOnly(() => dispatch({ type: "SET_PLOT_DIALOG_OPEN", open: true })));
    // `common.Control.plot` (ACTIONS::plot) is shared by both editors' File
    // menus, so it is tab-dispatched: the Gerber Plot dialog on the PCB tab,
    // eeschema's DIALOG_PLOT_SCHEMATIC (PlotSchematicDialog.tsx) on the
    // Schematic tab. The other tabs have no plot.
    m.set("common.Control.plot", () => {
      if (state.tab === "pcb") dispatch({ type: "SET_PLOT_DIALOG_OPEN", open: true });
      else if (state.tab === "schematic") dispatch({ type: "SET_SCH_PLOT_DIALOG_OPEN", open: true });
    });
    // `common.SuiteControl.openPreferences` (ACTIONS::openPreferences, Ctrl+,): the Preferences dialog; the one page the studio can back
    // is Mouse and Touchpad (PreferencesDialog.tsx, kicad-port/preferences.ts) -- every canvas reads what it saves.
    m.set("common.SuiteControl.openPreferences", () => dispatch({ type: "SET_PREFERENCES_DIALOG_OPEN", open: true }));
    // `common.Control.updatePcbFromSchematic` (F8): see kicad-port/updatePcb.ts -- the board is re-derived from the schematic on
    // every schematic edit, so this refetches the design and reports what an update would leave (nothing to apply).
    m.set("common.Control.updatePcbFromSchematic", () => {
      void api.refresh().then(() => {
        const parts = api.getState().board?.parts ?? [];
        dispatch({ type: "TOAST", message: updatePcbMessage(parts), kind: "info" });
      });
    });
    // `common.Control.saveAs` (ACTIONS::saveAs, Ctrl+Shift+S): "Save current document to another location". design.json is the only
    // master, so what is saved is the editor's derived KiCad file(s) -- `.kicad_pcb`, or the `.kicad_sch` (+ one per sub-sheet) --
    // handed to the browser's Save (kicad-port/saveAs.ts). The Footprint Editor's Save As is `commonSuiteActions.ts`'s (a library copy); the Symbol Editor has none (tab gate).
    m.set("common.Control.saveAs", () => {
      const name = state.board?.name ?? "board";
      const saved = (files: string[]) => dispatch({ type: "TOAST", message: `Saved ${files.join(", ")} (derived from design.json).`, kind: "info" });
      const failed = (e: unknown) => dispatch({ type: "TOAST", message: e instanceof Error ? e.message : String(e), kind: "error" });
      if (state.tab === "pcb") downloadKicadPcb(name).then((f) => saved([f])).catch(failed);
      else if (state.tab === "schematic") downloadKicadSchematic(name).then(saved).catch(failed);
    });
    m.set("pcbnew.EditorControl.generateDrillFiles", pcbOnly(() => dispatch({ type: "SET_GENERATE_DRILL_DIALOG_OPEN", open: true })));
    m.set("pcbnew.EditorControl.generatePosFile", pcbOnly(() => dispatch({ type: "SET_FOOTPRINT_POSITION_DIALOG_OPEN", open: true })));

    // Next / Previous Grid, the grid presets and the fast grids work on the editor's grid list (state/gridSettings.ts): actions/commonGridListActions.ts.

    // common.Interactive.search: this app has no KiCad Search panel --
    // the task put Search on the non-KiCad Activity tab instead (see
    // panels/RightDock.tsx), so that's what this jumps to.
    m.set("common.Interactive.search", () => dispatch({ type: "SET_RIGHT_DOCK_TAB", tab: "activity" }));
    m.set("pcbnew.Control.showNetInspector", () => dispatch({ type: "SET_NET_INSPECTOR_OPEN", open: true }));

    // common.Interactive.selectAll/unselectAll: every placed footprint
    // (respecting the footprints selection-filter toggle, same as a box
    // select already does -- tracks/vias/zones/shapes/text have no
    // filter toggle of their own yet, per selectionFilter's own doc in
    // state/store.tsx) plus every track/via/zone/shape/text id. Unselect
    // All only clears the selection (SET_SELECTION, not CLEAR_SELECTION
    // -- it shouldn't also cancel an in-progress tool/drawing the way
    // Escape does).
    m.set(
      "common.Interactive.selectAll",
      viewTab(() => {
        if (state.tab === "schematic") {
          // sch_selection_tool.cpp SelectAll: every selectable item on the sheet (the selection filter's categories, locked ones only with "Locked items" on).
          const sch = state.schematic;
          if (!sch) return;
          const selectable = schSelectable(sch, state.schSelectionFilter);
          dispatch({ type: "SET_SELECTION", refs: [...new Set(allItems(sch).map((r) => r.id))].filter(selectable) });
          return;
        }
        if (!state.board) return;
        // pcb_selection_tool.cpp SelectAll: every item passing the FULL selection filter (incl. locked) and layer visibility, added to the selection.
        dispatch({ type: "SET_SELECTION", refs: selectAllIds(state.board, state.selectionFilter, state.layerVisible, state.activeLayer, state.highContrast, state.selection) });
      })
    );
    m.set("common.Interactive.unselectAll", pcbOnly(() => dispatch({ type: "SET_SELECTION", refs: [] })));

    // ---------------------------------------------------------- eeschema
    //
    // Mirrors the `pcbOnly` guard above -- a stray M/R/X/Del while looking
    // at the PCB tab must never reach a schematic Cmd, same reasoning.
    const schematicOnly =
      <Args extends unknown[]>(fn: (...args: Args) => void) =>
      (...args: Args) => {
        if (state.tab === "schematic") fn(...args);
      };

    // `M` ("Move") and `G` ("Drag"): any selected or hovered item of any kind, locked ones left out (`FilterSelectionForLockedItems`); then the
    // pointer carries them and a click drops them (SchematicView.tsx). `G` stretches the wires, labels, junctions and no-connects attached.
    const armMove = (tool: "move" | "drag") =>
      schematicOnly(() => {
        const refs = withoutLocked(requestSelection(), new Set(state.schematic?.locked ?? []));
        if (refs.length === 0) return;
        adoptHovered();
        dispatch({ type: "SET_ACTIVE_TOOL", tool });
        dispatch({ type: "SET_MOVE_ORIGIN", at: state.cursorUm });
      });
    m.set("eeschema.InteractiveMove.move", armMove("move"));
    m.set("eeschema.InteractiveMove.drag", armMove("drag"));

    // sch_edit_tool.cpp Rotate/Mirror: RequestSelection (hover fallback), every selected item of every kind, one turn point for 2+ items, one undo step.
    // Rotate/Mirror skip locked items (`FilterSelectionForLockedItems`, called first by SCH_EDIT_TOOL::Rotate/Mirror).
    const schLocked = () => new Set(state.schematic?.locked ?? []);
    const schItems = () => withoutLocked(requestSelection(), schLocked());
    const schTurn = (turn: SchTurn) =>
      schematicOnly(() => {
        if (tryHeldTurn(turn)) return;
        void api.transformSchItems(schItems(), turn, pickedVertices(state));
      });
    m.set("eeschema.InteractiveEdit.rotateCCW", schTurn("rot_ccw"));
    m.set("eeschema.InteractiveEdit.rotateCW", schTurn("rot_cw"));
    m.set("eeschema.InteractiveEdit.mirrorH", schTurn("mirror_h"));
    m.set("eeschema.InteractiveEdit.mirrorV", schTurn("mirror_v"));

    // `E`/`U`/`V`/`F` (sch_edit_tool.cpp::Properties/EditField): one
    // shared dialog for all four -- see SymbolPropertiesDialog.tsx's own
    // header comment on why U/V/F don't get source's own separate, far
    // smaller single-field dialog.
    const schSymbols = () => schItems().filter((id) => api.symbolById(id));
    const openSymbolProperties = (field: "reference" | "value" | "footprint" | "datasheet" | null) =>
      schematicOnly(() => {
        const id = schSymbols()[0];
        if (id) dispatch({ type: "SET_SYMBOL_PROPERTIES", value: { id, field } });
      });
    // `E` (`SCH_EDIT_TOOL::Properties`): the dialog of whatever is selected -- a symbol, label, text, sheet, shape, text box, directive label, or wires / buses /
    // bus entries / junctions / graphic lines together (components/schematic/schPropertiesOpen.ts). A locked item has properties too.
    m.set(
      "eeschema.InteractiveEdit.properties",
      schematicOnly(() => {
        if (state.schematic) openSchProperties(state.schematic, requestSelection(), dispatch);
      })
    );
    m.set("eeschema.InteractiveEdit.symbolProperties", openSymbolProperties(null));
    m.set("eeschema.InteractiveEdit.editReference", openSymbolProperties("reference"));
    m.set("eeschema.InteractiveEdit.editValue", openSymbolProperties("value"));
    m.set("eeschema.InteractiveEdit.editFootprint", openSymbolProperties("footprint"));

    m.set("eeschema.InspectionTool.runERC", () => dispatch({ type: "SET_ERC_DIALOG_OPEN", open: true }));

    // `Alt+Backspace`/`Alt+Up` (sch_navigate_tool.cpp::LeaveSheet/Up -- `Up()`
    // itself just calls `LeaveSheet` in source, so both bind the same
    // handler here): pop one level off `state.currentSheetPath`. A no-op
    // at the root, same as source's own `CanGoUp()` guard.
    const leaveSheet = schematicOnly(() => {
      if (state.currentSheetPath.length === 0) return;
      api.navigateToSheet(state.currentSheetPath.slice(0, -1));
    });
    m.set("eeschema.NavigateTool.leaveSheet", leaveSheet);
    m.set("eeschema.NavigateTool.up", leaveSheet);

    // `Ctrl+A`: opens AnnotateDialog.tsx (scope/order/reset options) --
    // the dialog itself issues the real `annotate` Cmd on confirm.
    m.set("eeschema.EditorControl.annotate", schematicOnly(() => dispatch({ type: "SET_ANNOTATE_DIALOG_OPEN", open: true })));
    // File > Export > Netlist... -- DIALOG_EXPORT_NETLIST, see ExportNetlistDialog.tsx.
    m.set("eeschema.EditorControl.exportNetlist", schematicOnly(() => dispatch({ type: "SET_EXPORT_NETLIST_DIALOG_OPEN", open: true })));

    // Symbol Fields Table (`editSymbolFields`), Schematic Setup > ERC pin
    // map (`schematicSetup`) and Find / Find and Replace / Find Next /
    // Find Previous (`common.Interactive.find*`, F3 / Shift+F3): each opens
    // its dialog (SymbolFieldsTableDialog / SchematicSetupDialog /
    // FindReplaceDialog) or, for F3, cycles the shared match cursor
    // (components/schematic/findNavigation.ts). The `common.*` find actions
    // are registered on the Schematic tab only: `isActionEnabledForTab`
    // treats `common.*` as tab-less, and the PCB tab has no schematic search
    // to run, so leaving them unregistered there keeps the PCB menu entry
    // honestly disabled.
    if (state.tab === "schematic") {
      m.set("eeschema.EditorControl.editSymbolFields", () => dispatch({ type: "SET_SCH_DIALOG", dialog: "fields_table" }));
      m.set("eeschema.EditorControl.schematicSetup", () => dispatch({ type: "SET_SCH_DIALOG", dialog: "setup" }));
      m.set("common.Interactive.find", () => dispatch({ type: "SET_SCH_DIALOG", dialog: "find" }));
      m.set("common.Interactive.findAndReplace", () => dispatch({ type: "SET_SCH_DIALOG", dialog: "replace" }));
      m.set("common.Interactive.findNext", () => void findNextMatch(state, dispatch, false));
      m.set("common.Interactive.findPrevious", () => void findNextMatch(state, dispatch, true));
    }

    // `W`: arm/disarm the wire tool -- SchematicView.tsx's own
    // onPointerDown/onDoubleClick own the actual click-to-add-point/
    // finish state machine (same split PCB's route/zone/shape tools use:
    // this registry only ever flips `state.activeTool`).
    m.set(
      "eeschema.InteractiveDrawingLineWireBus.drawWires",
      schematicOnly(() => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "wire" ? "select" : "wire" }))
    );
    // `B` (GAPS.md #20): arm/disarm the bus tool -- shares the exact same
    // click-to-add-point/finish state machine as the wire tool above
    // (`drawState.kind` stays `"wire"` either way; `SchematicView.tsx`
    // reads `state.activeTool === "bus"` at commit time to tag the result
    // `Cmd::AddWire { bus: true }` instead of a plain wire).
    m.set(
      "eeschema.InteractiveDrawingLineWireBus.drawBuses",
      schematicOnly(() => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "bus" ? "select" : "bus" }))
    );
    // Backspace mid-draw: pop the in-progress wire's last point (never a
    // committed-command undo -- see `Cmd::DeleteWire`'s own doc on why
    // this never reaches the backend at all).
    m.set(
      "eeschema.InteractiveDrawingLineWireBus.undoLastSegment",
      schematicOnly(() => {
        const draw = state.drawState;
        if (draw?.kind !== "wire") return;
        dispatch({ type: "SET_DRAW_STATE", draw: draw.pts.length <= 1 ? null : { ...draw, pts: draw.pts.slice(0, -1) } });
      })
    );

    // `L`/Ctrl+`L`/`H`/`P`/`T`/`Q` (sch_drawing_tools.cpp): arm/disarm each
    // placement tool, same toggle shape as the wire tool above --
    // SchematicView.tsx's onPointerDown owns the actual click behavior
    // (pin-snap, open the right pending-dialog state, or for `Q`, commit
    // immediately).
    const toggleSchTool = (tool: Exclude<ToolId, "select">) => schematicOnly(() => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === tool ? "select" : tool }));
    m.set("eeschema.InteractiveDrawing.placeLabel", toggleSchTool("sch_label_local"));
    m.set("eeschema.InteractiveDrawing.placeGlobalLabel", toggleSchTool("sch_label_global"));
    m.set("eeschema.InteractiveDrawing.placeHierarchicalLabel", toggleSchTool("sch_label_hier"));
    m.set("eeschema.InteractiveDrawing.placePowerSymbol", toggleSchTool("sch_power"));
    m.set("eeschema.InteractiveDrawing.placeSchematicText", toggleSchTool("sch_text"));
    m.set("eeschema.InteractiveDrawing.placeNoConnect", toggleSchTool("sch_no_connect"));
    m.set("eeschema.InteractiveDrawing.placeBusWireEntry", toggleSchTool("sch_bus_entry"));
    // `A`: unlike the others above, this opens the chooser dialog first
    // (real source's own order too, for this one tool -- see
    // SymbolChooserDialog.tsx's header comment) rather than arming a tool
    // directly; confirming a choice there is what arms `sch_place_symbol`.
    m.set("eeschema.InteractiveDrawing.placeSymbol", schematicOnly(() => dispatch({ type: "SET_SYMBOL_CHOOSER_OPEN", open: true })));

    // ===================================================================
    // eeschema parity block (UI-ACTIONS.md "eeschema: not handled").
    // Everything between here and `return m` is the schematic-parity port;
    // each registration cites its source function.
    // ===================================================================
    const sch = state.schematic;
    const schConnInput = () => ({
      wires: (sch?.wires ?? []).map((w) => ({ id: w.id, pts: w.pts, bus: w.bus })),
      labels: (sch?.labels ?? []).map((l) => ({ id: l.id, at: l.at })),
      pinPoints: [
        ...(sch?.symbols ?? []).flatMap((s) => resolveLibSymbol(s, sch!.lib_symbols)?.pins.map((p) => p.tip) ?? []),
        ...(sch?.power_symbols ?? []).map((ps) => ps.at),
      ] as [number, number][],
      noConnectPoints: (sch?.no_connects ?? []).map((n) => [n.at[0], n.at[1]] as [number, number]),
    });
    const cursorSnapped = (): [number, number] | null => {
      if (!state.cursorUm) return null;
      const p = alignToGrid({ x: state.cursorUm.x, y: state.cursorUm.y }, SCH_GRID_UM, { x: 0, y: 0 }, { ctrlOrCmd: false });
      return [p.x, p.y];
    };

    // `\`` -- SCH_EDITOR_CONTROL::HighlightNet (sch_editor_control.cpp ->
    // highlightNet(toolMgr, cursorPos)): highlight the net of the
    // connectable item under the cursor; nothing there clears it.
    m.set(
      "eeschema.EditorControl.highlightNet",
      schematicOnly(() => {
        const c = cursorSnapped();
        if (!sch || !c) return;
        const net = netAtPoint(
          sch.wires,
          [...sch.labels.map((l) => ({ net: l.net, at: l.at })), ...sch.power_symbols.map((p) => ({ net: p.net, at: p.at }))],
          c[0],
          c[1],
          400 / state.schematicView.scale
        );
        dispatch({ type: "SET_NET_HIGHLIGHT", net });
      })
    );
    // `~` -- SCH_EDITOR_CONTROL::ClearHighlight: highlightNet(toolMgr, CLEAR).
    m.set("eeschema.EditorControl.clearHighlight", schematicOnly(() => dispatch({ type: "SET_NET_HIGHLIGHT", net: null })));

    // Shift+Space -- SCH_EDITOR_CONTROL::NextLineMode: line_mode =
    // (line_mode + 1) % LINE_MODE_COUNT (kicad-port/schLineMode.ts).
    m.set("eeschema.EditorControl.lineModeNext", schematicOnly(() => dispatch({ type: "SET_SCH_LINE_MODE", mode: nextLineMode(state.schLineMode) })));
    // SCH_EDITOR_CONTROL::ChangeLineMode (lineModeFree / lineMode90 /
    // lineMode45; source's 90-degree action is spelled `lineModeOrthonal`
    // in actions.json).
    m.set("eeschema.EditorControl.lineModeFree", schematicOnly(() => dispatch({ type: "SET_SCH_LINE_MODE", mode: LINE_MODE_FREE })));
    m.set("eeschema.EditorControl.lineModeOrthonal", schematicOnly(() => dispatch({ type: "SET_SCH_LINE_MODE", mode: LINE_MODE_90 })));
    m.set("eeschema.EditorControl.lineMode45", schematicOnly(() => dispatch({ type: "SET_SCH_LINE_MODE", mode: LINE_MODE_45 })));
    // SCH_EDITOR_CONTROL::OnAngleSnapModeChanged: only refreshes the
    // toolbar's selected line-mode button; the toolbar here reads
    // `state.schLineMode` directly, so there is nothing further to do.
    m.set("eeschema.EditorControl.angleSnapModeChanged", schematicOnly(() => {}));

    // `/` -- SCH_LINE_WIRE_BUS_TOOL::doDrawSegments's
    // `switchSegmentPosture` branch: `posture = !posture` while a wire
    // with >= 2 segments is being drawn, then the break point is
    // recomputed (here: the preview recomputes from `state.schPosture`).
    m.set(
      "eeschema.InteractiveDrawingLineWireBus.switchPosture",
      schematicOnly(() => {
        if (state.drawState?.kind !== "wire") return;
        dispatch({ type: "TOGGLE_SCH_POSTURE" });
      })
    );

    // Ctrl+4 -- SCH_SELECTION_TOOL::SelectConnection (staged
    // junction -> pin -> never expansion; kicad-port/schConnection.ts).
    m.set(
      "eeschema.InteractiveSelection.SelectConnection",
      schematicOnly(() => {
        if (!sch || state.selection.size === 0) return;
        dispatch({ type: "SET_SELECTION", refs: selectConnection(schConnInput(), [...state.selection]) });
      })
    );
    // Alt+3 -- SCH_SELECTION_TOOL::SelectNode: SelectPoint(cursor,
    // connectedTypes), the connectable items at the cursor.
    m.set(
      "eeschema.InteractiveSelection.SelectNode",
      schematicOnly(() => {
        const c = cursorSnapped();
        if (!sch || !c) return;
        dispatch({ type: "SET_SELECTION", refs: selectNodeAt(schConnInput(), c) });
      })
    );

    // Alt+Left / Alt+Right -- SCH_NAVIGATE_TOOL::Back / Forward
    // (kicad-port/navHistory.ts): step m_navIndex, clear the selection,
    // and switch to that sheet WITHOUT pushing a new history entry.
    const navStep = (dir: "back" | "forward") =>
      schematicOnly(() => {
        const next = dir === "back" ? goBack(state.schNav) : goForward(state.schNav);
        if (!next) return; // source: wxBell()
        dispatch({ type: "SET_SCH_NAV", nav: next });
        dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
        void api.navigateToSheet(next.entries[next.index]!, false);
      });
    m.set("eeschema.NavigateTool.back", navStep("back"));
    m.set("eeschema.NavigateTool.forward", navStep("forward"));

    // Ctrl+H -- SCH_EDITOR_CONTROL::ShowHierarchy (sch_editor_control.cpp)
    // toggles the Schematic Hierarchy pane (`ToggleSchematicHierarchy`:
    // `PANE_INFO.Show( !IsShown() )`); the pane lives in the left dock
    // column (SchematicDock), which showing it brings back.
    m.set(
      "eeschema.EditorTool.showHierarchy",
      schematicOnly(() => {
        toggleDockPane("hierarchy");
      })
    );

    // Ctrl+E / Ctrl+Shift+E -- SCH_EDITOR_CONTROL::EditWithSymbolEditor
    // (editWithSymbolEditor, and editLibSymbolWithSymbolEditor which
    // routes through the same function): open the selected symbol's
    // library entry in the Symbol Editor. This IR has one symbol library
    // (no separate schematic-local copy), so both open the same entry.
    // An unresolvable lib_id starts a blank symbol named after the
    // reference (`Cmd::OpenSymbolForEdit`'s own precedent).
    const editInSymbolEditor = schematicOnly(() => {
      if (state.selection.size !== 1) return;
      const id = [...state.selection][0]!;
      const sym = sch?.symbols.find((s) => s.id === id);
      if (!sym) return;
      const libId = sym.lib_id && sym.lib_id.trim() ? sym.lib_id : `eda:${sym.id}`;
      dispatch({ type: "SET_TAB", tab: "symbol" });
      void symApi.openSymbol(libId);
    });
    m.set("eeschema.EditorControl.editWithSymbolEditor", editInSymbolEditor);
    m.set("eeschema.EditorControl.editLibSymbolWithSymbolEditor", editInSymbolEditor);

    // SCH_ACTIONS::alignLeft/Right/Top/Bottom/CenterX/CenterY (eeschema/tools/sch_align_tool.cpp): every selected item of every kind lines up
    // with the target's edge (kicad-port/schAlign.ts measures it from the boxes the studio draws and picks by); the backend's `align` verb
    // moves each by its offset, snapped so pins stay on the connection grid and with the wires on each item stretching, as one undo step.
    const alignItems = (kind: SchAlignKind) =>
      schematicOnly(() => {
        if (!sch) return;
        const picked = new Set(requestSelection());
        const locked = new Set(sch.locked ?? []);
        const items: SchAlignItem[] = allItems(sch)
          .filter((r) => picked.has(r.id))
          .flatMap((r) => {
            const b = itemBounds(sch, r);
            return b ? [{ id: r.id, box: [b.minX, b.minY, b.maxX, b.maxY] as const, locked: locked.has(r.id) }] : [];
          });
        const moves = alignMoves(items, kind, state.cursorUm ? [state.cursorUm.x, state.cursorUm.y] : null).filter((mv) => mv.dx !== 0 || mv.dy !== 0);
        if (moves.length > 0) void api.cmd({ op: "sch_move", verb: "align", moves });
      });
    m.set("eeschema.Align.alignLeft", alignItems("left"));
    m.set("eeschema.Align.alignRight", alignItems("right"));
    m.set("eeschema.Align.alignTop", alignItems("top"));
    m.set("eeschema.Align.alignBottom", alignItems("bottom"));
    m.set("eeschema.Align.alignCenterX", alignItems("centerX"));
    m.set("eeschema.Align.alignCenterY", alignItems("centerY"));
    // SCH_MOVE_TOOL::AlignToGrid: each selected item (hovered when nothing is selected) goes to the nearest grid point, by where most of its
    // connection points are, with the wires on it -- `align_to_grid`, one undo step.
    m.set(
      "eeschema.AlignToGrid",
      schematicOnly(() => {
        const ids = schItems();
        if (ids.length > 0) void api.cmd({ op: "sch_move", verb: "align_to_grid", ids });
      })
    );

    // ===================================================================
    // common.* parity block (UI-ACTIONS.md "common: not handled, hotkeyed
    // first"): cursor/pan keys, fast grids, snap mode, zoom-to-area, save/
    // print status, datasheet, copy-as-text, find-next-marker, library
    // search focus. Each registration cites its KiCad source function.
    // Pure math lives in kicad-port/cursorControl.ts, markerNav.ts and
    // itemText.ts (unit tested). saveAs, updatePcbFromSchematic and
    // openPreferences are registered further down (kicad-port/saveAs.ts,
    // updatePcb.ts, preferences.ts). Not ported (no subsystem behind
    // them; tools/ui-parity-missing.json holds the reasons): new, open,
    // toggleGridOverrides, pasteSpecial, cycleArcEditMode.
    // ===================================================================
    const onCanvasTab = state.tab === "pcb" || state.tab === "schematic";
    const canvasOnly =
      <Args extends unknown[]>(fn: (...args: Args) => void) =>
      (...args: Args) => {
        if (onCanvasTab) fn(...args);
      };
    const activeView = () => (state.tab === "schematic" ? state.schematicView : state.view);
    /** `view->IsMirroredX()`: the PCB canvas is showing the board flipped (pcbnew.Control.flipBoard). */
    const flippedView = state.tab === "pcb" && state.bcx.boardFlipped;
    const setActiveView = (view: typeof state.view) => dispatch(state.tab === "schematic" ? { type: "SET_SCHEMATIC_VIEW", view } : { type: "SET_VIEW", view });
    /** The grid CursorControl/PanControl step by: this app's PCB grid, or the schematic's fixed 50 mil (SCH_GRID_UM). */
    const activeGridUm = () => (state.tab === "schematic" ? SCH_GRID_UM : state.gridUm);

    /**
     * Stand-in for `m_toolMgr->ProcessEvent(TC_MOUSE...)` / the warped pointer
     * generating a motion event: re-dispatches a pointer/mouse event on the
     * live canvas at a world position, so every tool that follows the
     * pointer (move preview, route/wire rubber band, click handlers) sees
     * the keyboard cursor exactly as it would a real mouse at that spot.
     * `viewOverride` is the view the position must be projected through when
     * a view change was dispatched in the same tick (it has not rendered yet).
     */
    const emitCanvasPointer = (type: "pointermove" | "pointerdown" | "pointerup" | "dblclick", world: { x: number; y: number }, viewOverride?: typeof state.view) => {
      const el = document.querySelector(".pcb-canvas-container canvas");
      const rect = canvasRect();
      if (!el || !rect) return;
      const [sx, sy] = worldToScreen(viewOverride ?? activeView(), world.x, world.y);
      const init = { bubbles: true, cancelable: true, composed: true, clientX: rect.left + flipLocalX(flippedView, rect.width, sx), clientY: rect.top + sy, button: 0 };
      if (type === "dblclick") el.dispatchEvent(new MouseEvent("dblclick", { ...init, detail: 2 }));
      else el.dispatchEvent(new PointerEvent(type, { ...init, pointerId: 1, pointerType: "mouse", isPrimary: true, buttons: type === "pointerdown" ? 1 : 0 }));
    };

    // common_tools.cpp COMMON_TOOLS::CursorControl (CURSOR_UP/DOWN/LEFT/RIGHT
    // and the *_FAST variants, Ctrl+arrows): step the raw cursor one grid
    // cell (ten when fast), SetCursorPosition( cursor, warpView=true ) --
    // re-centre the view if the cursor left the canvas -- then refreshPreview.
    const moveCursor = (dir: CursorDir, fast: boolean) =>
      canvasOnly(() => {
        const rect = canvasRect();
        const view = activeView();
        if (!rect || !(view.scale > 0)) return;
        const from = state.cursorUm ?? viewCenter(view, rect.width, rect.height);
        const to = cursorMove(from, activeGridUm(), dir, fast, flippedView);
        const nextView = warpViewToInclude(view, rect.width, rect.height, to);
        dispatch({ type: "SET_CURSOR", at: to });
        if (nextView !== view) setActiveView(nextView);
        // PostAction( refreshPreview ): let previews re-evaluate at the new cursor once the view above has rendered.
        window.setTimeout(() => emitCanvasPointer("pointermove", to, nextView), 20);
      });
    m.set("common.Control.cursorUp", moveCursor("up", false));
    m.set("common.Control.cursorDown", moveCursor("down", false));
    m.set("common.Control.cursorLeft", moveCursor("left", false));
    m.set("common.Control.cursorRight", moveCursor("right", false));
    m.set("common.Control.cursorUpFast", moveCursor("up", true));
    m.set("common.Control.cursorDownFast", moveCursor("down", true));
    m.set("common.Control.cursorLeftFast", moveCursor("left", true));
    m.set("common.Control.cursorRightFast", moveCursor("right", true));

    // CursorControl's CURSOR_CLICK / CURSOR_DBL_CLICK: a TC_MOUSE click/double-click
    // event at the cursor position. While a PCB route/zone/shape is being drawn
    // Enter already finishes it (Canvas.tsx's own convention), so the click is
    // left to that handler there.
    m.set(
      "common.Control.cursorClick",
      canvasOnly(() => {
        if (!state.cursorUm || (state.tab === "pcb" && state.drawState)) return;
        emitCanvasPointer("pointerdown", state.cursorUm);
        emitCanvasPointer("pointerup", state.cursorUm);
      })
    );
    const dblClickAtCursor = canvasOnly(() => {
      if (state.cursorUm) emitCanvasPointer("dblclick", state.cursorUm);
    });
    m.set("common.Control.cursorDblClick", dblClickAtCursor);
    // ACTIONS::finishInteractive (End): ends whatever multi-point tool is in
    // progress. The route/zone/shape/wire tools here all finish on double-click
    // (Canvas.tsx / SchematicView.tsx onDoubleClick), so this is that, but only
    // while something is actually being drawn.
    m.set(
      "common.Interactive.finish",
      canvasOnly(() => {
        // End also closes a lasso being drawn (`selectLasso`: `evt->IsAction( &ACTIONS::finishInteractive )`).
        if (state.drawState || getCommonTool().lasso) dblClickAtCursor();
      })
    );

    // common_tools.cpp COMMON_TOOLS::PanControl (Shift+arrows): move the view
    // centre by ten current-grid cells.
    const panView = (dir: CursorDir) =>
      canvasOnly(() => {
        const rect = canvasRect();
        const view = activeView();
        if (!rect || !(view.scale > 0)) return;
        setActiveView(panByGrid(view, rect.width, rect.height, activeGridUm(), dir, flippedView));
      });
    m.set("common.Control.panUp", panView("up"));
    m.set("common.Control.panDown", panView("down"));
    m.set("common.Control.panLeft", panView("left"));
    m.set("common.Control.panRight", panView("right"));

    // The fast grids (common_tools.cpp GridFast1/GridFast2/GridFastCycle) are commonGridListActions.ts's. PCB tab only from here: the board's snap mode.
    if (state.tab === "pcb") {
      // pcb_control.cpp PCB_CONTROL::SnapMode (magneticSnapToggle, Shift+S):
      // `settings.allLayers = !settings.allLayers`; SnapModeFeedback pops up
      // "Object Snapping: Active Layer / All Layers". The setting feeds
      // collectAnchors' layer filter (kicad-port/gridSnap.ts).
      m.set("common.Control.magneticSnapToggle", () => {
        const next = !state.magneticAllLayers;
        dispatch({ type: "SET_MAGNETIC_ALL_LAYERS", value: next });
        dispatch({ type: "TOAST", message: `Object Snapping: ${next ? "All Layers" : "Active Layer"}`, kind: "info" });
      });
    }

    // zoom_tool.cpp ZOOM_TOOL::Main (Ctrl+F5): arm the rubber-band zoom tool;
    // components/ZoomAreaOverlay.tsx owns the drag (selectRegion).
    m.set("common.Control.zoomTool", canvasOnly(() => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "zoom_area" ? "select" : "zoom_area" })));

    // pcbnew/files.cpp SavePcbFile / eeschema/files-io.cpp SaveEEFile report
    // "File '%s' saved." in the status area. Every edit here already went
    // through /api/cmd and was written to design.json, so there is nothing
    // left to flush -- only KiCad's status message is shown (no file dialog).
    m.set(
      "common.Control.save",
      canvasOnly(() => dispatch({ type: "TOAST", message: "File 'design.json' saved.", kind: "info" }))
    );
    // common.Control.print (Ctrl+P): KiCad opens its print dialog for the
    // current sheet/board; here the browser's print of the current view
    // (the @media print rules in styles/layout.css reduce the page to the canvas).
    m.set("common.Control.print", canvasOnly(() => window.print()));

    // sch_inspection_tool.cpp SCH_INSPECTION_TOOL::ShowDatasheet (D): the
    // selected symbol (RequestSelection({SCH_SYMBOL_T}) falls back to the
    // symbol under the cursor); empty or "~" -> "No datasheet defined."
    // (ShowInfoBarError), otherwise open it (GetAssociatedDocument). Schematic
    // tab only -- in pcbnew D is registered by the footprint editor alone,
    // and this app's footprint editor tab has no datasheet field.
    if (state.tab === "schematic") {
      m.set("common.Control.showDatasheet", () => {
        if (!sch) return;
        let symbol = [...state.selection].map((id) => sch.symbols.find((s) => s.id === id)).find((s) => s != null);
        if (!symbol && state.cursorUm) {
          const { x, y } = state.cursorUm;
          symbol = sch.symbols.find((s) => {
            const b = symbolBounds(s, sch.lib_symbols);
            return x >= b.minX && x <= b.maxX && y >= b.minY && y <= b.maxY;
          });
        }
        if (!symbol) return;
        const target = datasheetTarget(symbol.datasheet);
        if (target.kind === "none") dispatch({ type: "TOAST", message: "No datasheet defined.", kind: "error" });
        else if (target.kind === "url") window.open(target.url, "_blank", "noopener,noreferrer");
        else dispatch({ type: "TOAST", message: `Cannot open datasheet '${target.text}': only absolute http(s)/file URLs can be opened from a browser.`, kind: "error" });
      });
    }

    // edit_tool.cpp EDIT_TOOL::copyToClipboardAsText (Ctrl+Shift+C, PCB: text,
    // dimensions) and sch_editor_control.cpp SCH_EDITOR_CONTROL::CopyAsText ->
    // sch_tool_utils.cpp GetSelectedItemsAsText (labels, text): the shown text
    // of each selected item, trimmed, newline-joined, onto the clipboard.
    m.set(
      "common.Interactive.copyAsText",
      canvasOnly(() => {
        const ids = [...state.selection];
        const texts =
          state.tab === "schematic"
            ? ids.map((id) => sch?.labels.find((l) => l.id === id)?.net ?? sch?.texts.find((t) => t.id === id)?.content)
            : ids.map((id) => api.textById(id)?.content ?? api.dimensionById(id)?.text);
        const text = selectionAsText(texts);
        if (text === "") return;
        navigator.clipboard.writeText(text).catch(() => dispatch({ type: "TOAST", message: "Could not write to the clipboard.", kind: "error" }));
      })
    );

    // sch_find_replace_tool.cpp SCH_FIND_REPLACE_TOOL::FindNext with
    // `data.markersOnly = true` (ACTIONS::findNextMarker, Ctrl+Shift+F3): the next
    // ERC marker after the last one visited, in nextMatch()'s x-then-y order;
    // select it and FocusOnLocation. At the end: "Reached end of sheet. Find
    // again to wrap around to the start." (kicad-port/markerNav.ts). Schematic
    // only -- pcbnew has no findNextMarker (its DRC dialog owns nextMarker).
    if (state.tab === "schematic") {
      m.set("common.Interactive.findNextMarker", () => {
        void (async () => {
          let report = state.erc;
          if (!report) {
            try {
              dispatch({ type: "TOAST", message: "Running ERC (kicad-cli, a few seconds)...", kind: "info" });
              report = await fetchErc();
              dispatch({ type: "ERC_OK", erc: report, version: state.version });
            } catch {
              dispatch({ type: "TOAST", message: "Could not run ERC.", kind: "error" });
              return;
            }
          }
          const located = report.violations.flatMap((v, index) => {
            const at = ercMarkerPosition(v.location, sch);
            return at ? [{ key: `${v.check}|${v.location}`, x: at.at[0], y: at.at[1], index, refs: at.refs }] : [];
          });
          const hit = nextMarker(located, markerCursor.current);
          markerCursor.current = hit.cursor;
          if (!hit.marker) {
            dispatch({ type: "TOAST", message: located.length === 0 ? "No markers found." : "Reached end of sheet. Find again to wrap around to the start.", kind: "info" });
            return;
          }
          const found = located.find((l) => l.key === hit.marker!.key)!;
          dispatch({ type: "SET_ERC_SELECTED", index: found.index });
          dispatch({ type: "SET_SELECTION", refs: found.refs });
          dispatch({ type: "SET_HOT", refs: found.refs });
          const rect = canvasRect();
          const view = state.schematicView;
          if (rect && view.scale > 0) dispatch({ type: "SET_SCHEMATIC_VIEW", view: viewCenteredOn(view, rect.width, rect.height, { x: found.x, y: found.y }) });
        })();
      });
    }

    // common/lib_tree / LIB_TREE's ACTIONS::libraryTreeSearch (Ctrl+L): focus
    // the search field of the symbol chooser. Registered only while that
    // dialog is open. (This app has no footprint chooser.)
    if (state.symbolChooserOpen) {
      m.set("common.Control.libraryTreeSearch", () => {
        const el = document.getElementById("library-tree-search") as HTMLInputElement | null;
        el?.focus();
        el?.select();
      });
    }

    // pcbnew parity block -- the "pcbnew: not handled, hotkeyed first" rows
    // of docs/parity/UI-ACTIONS.md. Every handler cites the KiCad source
    // function it ports (pcbnew/tools/*.cpp at commit 8303b2ad); pure logic
    // lives in kicad-port/pcbEditActions.ts + pcbParityState.ts (unit
    // tested), every board edit is an `/api/cmd` verb (so it has undo).
    // Context-sensitive hotkeys (Ctrl++, Backspace, Ctrl+E, Tab, 1-4) are
    // registered only in the state that owns them, which is how this app's
    // flat registry stands in for KiCad's tool stack (see the note on F
    // above): exactly one candidate per key is ever enabled.
    // ===================================================================
    {
      const board = state.board;
      const lockedSet = new Set(board?.locked ?? []);
      const ratsnestEdges = state.ratsnest?.edges ?? [];
      const copperLayers = board?.layers ?? [];
      const routing = state.drawState?.kind === "route";
      const shapeToolActive = state.activeTool === "draw_segment" || state.activeTool === "draw_arc" || state.activeTool === "draw_bezier" || state.activeTool === "draw_rect" || state.activeTool === "draw_circle" || state.activeTool === "draw_polygon";

      // ---- toggleLock / lock / unlock -- board_editor_control.cpp BOARD_EDITOR_CONTROL::modifyLockSelected
      const modifyLock = (mode: "toggle" | "on" | "off") =>
        pcbOnly(() => {
          // A selection that is empty falls back to selectionCursor (the item under the cursor). Free pads are never locked (n/a here: no free pads).
          const ids = requestSelection().filter((id) => Boolean(api.partByRef(id)?.placed || api.trackById(id) || api.viaById(id) || api.zoneById(id) || api.shapeById(id) || api.textById(id)));
          if (ids.length === 0) return;
          const locked = mode === "toggle" ? resolveToggleLock(ids, lockedSet) : mode === "on";
          void api.cmd({ op: "set_locked", ids, locked });
        });
      m.set("pcbnew.EditorControl.toggleLock", modifyLock("toggle"));
      m.set("pcbnew.EditorControl.lock", modifyLock("on"));
      m.set("pcbnew.EditorControl.unlock", modifyLock("off"));

      // ---- deleteFull (Shift+Del) -- edit_tool.cpp EDIT_TOOL::Remove with REMOVE_FLAGS::ALT: "we expand selected track items to their full connection" (RunAction(selectConnection)), then DeleteItems.
      const deleteItems = async (ids: string[]) => {
        dispatch({ type: "SET_SELECTION", refs: [] });
        const trackIds = ids.filter((id) => api.trackById(id));
        const viaIds = ids.filter((id) => api.viaById(id));
        // One Cmd for all copper, so the whole delete is one undo step (as one BOARD_COMMIT::Push in source).
        if (trackIds.length || viaIds.length) await api.cmd({ op: "commit_route", remove_track_ids: trackIds, remove_via_ids: viaIds });
        for (const id of ids) {
          if (api.trackById(id) || api.viaById(id)) continue;
          if (api.zoneById(id)) await api.cmd({ op: "delete_zone", id });
          else if (api.shapeById(id)) await api.cmd({ op: "delete_shape", id });
          else if (api.textById(id)) await api.cmd({ op: "delete_text", id });
          else if (api.dimensionById(id)) await api.cmd({ op: "delete_dimension", id });
          else if (api.partByRef(id)?.placed) await api.cmd({ op: "rip", part: id });
        }
      };
      m.set(
        "pcbnew.InteractiveEdit.deleteFull",
        pcbOnly(() => {
          let ids = requestSelection();
          if (ids.length === 0) return;
          if (ids.some((id) => api.trackById(id) || api.viaById(id))) ids = computeConnectionRefs(new Set(ids)) ?? ids;
          void deleteItems(ids);
        })
      );

      // ---- FindMove (T) -- edit_tool.cpp EDIT_TOOL::GetAndPlace: DIALOG_GET_FOOTPRINT_BY_NAME, then select it and start Move (components/PcbParityDialogs.tsx).
      m.set("pcbnew.InteractiveEdit.FindMove", pcbOnly(() => dispatch({ type: "PCBX", patch: { pcbDialog: "find_move" } })));

      // ---- moveIndividually (Ctrl+M) / skip (Tab) -- edit_tool_move_fct.cpp EDIT_TOOL::doMoveSelection's `moveIndividually` branch:
      // items are picked up one at a time (selection order), each glued to the cursor by its anchor; a click drops it and picks up the next, Tab leaves it where it was.
      const startMoveIndividually = (refs: string[]) => {
        if (!board) return;
        const movable = refs.filter((r) => movableItem(board, r));
        const [first, ...rest] = movable;
        if (!first) return;
        const at = movableItem(board, first)!.at;
        dispatch({ type: "SET_SELECTION", refs: [first] });
        dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
        dispatch({ type: "PCBX", patch: { moveQueue: rest, movingIndividually: true } });
        dispatch({ type: "SET_ACTIVE_TOOL", tool: "move" });
        dispatch({ type: "SET_MOVE_ORIGIN", at: { x: at[0], y: at[1] } });
      };
      m.set("pcbnew.InteractiveMove.moveIndividually", pcbOnly(() => startMoveIndividually(requestSelection())));
      if (state.pcbx.movingIndividually) {
        m.set(
          "pcbnew.InteractiveEdit.skip",
          pcbOnly(() => {
            dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
            api.advanceMoveQueue();
          })
        );
      }

      // ---- SelectUnconnected (O) / GrabUnconnected (Shift+O) -- pcb_selection_tool.cpp PCB_SELECTION_TOOL::selectUnconnected / grabUnconnected
      m.set(
        "pcbnew.InteractiveSelection.SelectUnconnected",
        pcbOnly(() => {
          if (!board) return;
          dispatch({ type: "SET_SELECTION", refs: selectUnconnectedFootprints(board.parts, ratsnestEdges, [...state.selection]) });
        })
      );
      m.set(
        "pcbnew.InteractiveSelection.GrabUnconnected",
        pcbOnly(() => {
          if (!board) return;
          const refs = grabNearestUnconnectedFootprints(board.parts, ratsnestEdges, [...state.selection]);
          dispatch({ type: "SET_SELECTION", refs });
          // `m_toolMgr->RunAction( PCB_ACTIONS::moveIndividually )`
          startMoveIndividually(refs);
        })
      );

      // ---- swap (Alt+S) -- edit_tool_move_fct.cpp EDIT_TOOL::Swap, footprints (needs 2+ selected; poses shift cyclically in selection order).
      m.set(
        "pcbnew.InteractiveEdit.swap",
        pcbOnly(() => {
          const parts = [...state.selection].filter((id) => api.partByRef(id)?.placed);
          if (parts.length < 2) return;
          void api.cmd({ op: "swap_chain", parts });
        })
      );

      // ---- changeTrackLayerNext/Prev (Ctrl++ / Ctrl+-) -- edit_tool.cpp EDIT_TOOL::ChangeTrackLayer: step the active layer (layerNext/layerPrev), then move every selected track onto it.
      // Not while routing, and not while a graphic tool is armed (there Ctrl++/Ctrl+- are incWidth/decWidth).
      if (!routing && !shapeToolActive) {
        const changeTrackLayer = (dir: 1 | -1) =>
          pcbOnly(() => {
            const next = stepCopperLayer(copperLayers, state.layerVisible, state.activeLayer, dir);
            if (!next) return; // newLayer == origLayer: nothing to do
            const ids = requestSelection().filter((id) => api.trackById(id));
            dispatch({ type: "SET_ACTIVE_LAYER", layer: next });
            if (ids.length) void api.cmd({ op: "edit_tracks_and_vias", ids, layer: next });
          });
        m.set("pcbnew.Control.changeTrackLayerNext", changeTrackLayer(1));
        m.set("pcbnew.Control.changeTrackLayerPrev", changeTrackLayer(-1));
      }

      // ---- layerTop / layerInnerN / layerBottom -- pcb_control.cpp PCB_CONTROL::LayerSwitch (PCB_EDIT_FRAME::SwitchLayer): make that copper layer active if the board has it.
      const switchLayer = (layer: string) =>
        pcbOnly(() => {
          if (copperLayers.includes(layer)) dispatch({ type: "SET_ACTIVE_LAYER", layer });
        });
      m.set("pcbnew.Control.layerTop", switchLayer("F.Cu"));
      m.set("pcbnew.Control.layerBottom", switchLayer("B.Cu"));
      m.set("pcbnew.Control.layerInner1", switchLayer("In1.Cu"));
      m.set("pcbnew.Control.layerInner2", switchLayer("In2.Cu"));
      m.set("pcbnew.Control.layerInner3", switchLayer("In3.Cu"));
      m.set("pcbnew.Control.layerInner4", switchLayer("In4.Cu"));
      m.set("pcbnew.Control.layerInner5", switchLayer("In5.Cu"));
      m.set("pcbnew.Control.layerInner6", switchLayer("In6.Cu"));
      m.set("pcbnew.Control.layerInner7", switchLayer("In7.Cu"));
      m.set("pcbnew.Control.layerInner8", switchLayer("In8.Cu"));
      m.set("pcbnew.Control.layerInner9", switchLayer("In9.Cu"));
      m.set("pcbnew.Control.layerInner10", switchLayer("In10.Cu"));
      m.set("pcbnew.Control.layerInner11", switchLayer("In11.Cu"));
      m.set("pcbnew.Control.layerInner12", switchLayer("In12.Cu"));
      m.set("pcbnew.Control.layerInner13", switchLayer("In13.Cu"));
      m.set("pcbnew.Control.layerInner14", switchLayer("In14.Cu"));
      m.set("pcbnew.Control.layerInner15", switchLayer("In15.Cu"));
      m.set("pcbnew.Control.layerInner16", switchLayer("In16.Cu"));
      m.set("pcbnew.Control.layerInner17", switchLayer("In17.Cu"));
      m.set("pcbnew.Control.layerInner18", switchLayer("In18.Cu"));
      m.set("pcbnew.Control.layerInner19", switchLayer("In19.Cu"));
      m.set("pcbnew.Control.layerInner20", switchLayer("In20.Cu"));
      m.set("pcbnew.Control.layerInner21", switchLayer("In21.Cu"));
      m.set("pcbnew.Control.layerInner22", switchLayer("In22.Cu"));
      m.set("pcbnew.Control.layerInner23", switchLayer("In23.Cu"));
      m.set("pcbnew.Control.layerInner24", switchLayer("In24.Cu"));
      m.set("pcbnew.Control.layerInner25", switchLayer("In25.Cu"));
      m.set("pcbnew.Control.layerInner26", switchLayer("In26.Cu"));
      m.set("pcbnew.Control.layerInner27", switchLayer("In27.Cu"));
      m.set("pcbnew.Control.layerInner28", switchLayer("In28.Cu"));
      m.set("pcbnew.Control.layerInner29", switchLayer("In29.Cu"));
      m.set("pcbnew.Control.layerInner30", switchLayer("In30.Cu"));

      // ---- layerPairPresetCycle (Shift+V) -- pcb_control.cpp PCB_CONTROL::CycleLayerPresets: "if( presets.size() < 2 ) return 0;". This model has one layer pair (the outer pair) and no
      // user-defined LAYER_PAIR_SETTINGS presets, so the cycle is always source's early return.
      m.set("pcbnew.Control.layerPairPresetCycle", pcbOnly(() => {}));

      // ---- zoneCutout (Shift+C) / similarZone (Ctrl+Shift+.) -- drawing_tool.cpp DRAWING_TOOL::DrawZone with ZONE_MODE::CUTOUT / SIMILAR (getSourceZoneForAction: a single zone, from
      // the selection or else under the cursor); the outline is then drawn with the ordinary zone tool and Canvas.tsx's finishDraw commits it (zone_cutout / add_zone, no dialog).
      const armSourceZoneDraw = (mode: "cutout" | "similar") =>
        pcbOnly(() => {
          const ids = requestSelection();
          const source = ids.length === 1 ? api.zoneById(ids[0]!) : undefined;
          if (!source) return;
          dispatch({ type: "SET_SELECTION", refs: [source.id] });
          dispatch({ type: "SET_NEXT_ZONE_IS_RULE_AREA", value: false }); // also clears any older zoneDrawMode, so set ours after it
          dispatch({ type: "PCBX", patch: { zoneDrawMode: { mode, sourceId: source.id } } });
          dispatch({ type: "SET_ACTIVE_TOOL", tool: "zone" });
        });
      m.set("pcbnew.InteractiveDrawing.zoneCutout", armSourceZoneDraw("cutout"));
      m.set("pcbnew.InteractiveDrawing.similarZone", armSourceZoneDraw("similar"));

      // ---- positionRelative (Shift+P) -- position_relative_tool.cpp POSITION_RELATIVE_TOOL::PositionRelative -> DIALOG_POSITION_RELATIVE (components/PcbParityDialogs.tsx)
      m.set(
        "pcbnew.PositionRelative.positionRelative",
        pcbOnly(() => {
          const ids = requestSelection();
          if (ids.length === 0) return;
          if (state.selection.size === 0) dispatch({ type: "SET_SELECTION", refs: ids });
          dispatch({ type: "PCBX", patch: { pcbDialog: "position_relative" } });
        })
      );

      // ---- placeFootprint (A) -- board_editor_control.cpp BOARD_EDITOR_CONTROL::PlaceFootprint (components/PcbParityDialogs.tsx: pick an unplaced footprint, click to place)
      m.set("pcbnew.EditorControl.placeFootprint", pcbOnly(() => dispatch({ type: "PCBX", patch: { pcbDialog: "place_footprint" } })));

      // ---- unrouteSegment (Backspace) -- pcb_selection_tool.cpp PCB_SELECTION_TOOL::unrouteSegment: delete the selected tracks/vias, then select what they were connected to
      // so the user can keep backing up. Idle only (while routing/drawing Backspace is UndoLastSegment/deleteLastPoint).
      if (!routing && !state.drawState) {
        m.set(
          "pcbnew.InteractiveSelection.unrouteSegment",
          pcbOnly(() => {
            if (!board) return;
            const ids = [...state.selection].filter((id) => api.trackById(id) || api.viaById(id));
            if (ids.length === 0) return;
            const tracks = board.routing?.tracks ?? [];
            const vias = board.routing?.vias ?? [];
            const reselect = unrouteSegmentReselect(tracks, vias, new Set(ids));
            void api.cmd({ op: "commit_route", remove_track_ids: ids.filter((id) => api.trackById(id)), remove_via_ids: ids.filter((id) => api.viaById(id)) }).then((ok) => {
              if (ok) dispatch({ type: "SET_SELECTION", refs: reselect });
            });
          })
        );
      }

      // ---- deleteLastPoint (Backspace) -- drawing_tool.cpp (DrawZone/DrawSegment/...): `polyGeomMgr.DeleteLastCorner()`; with no corner left the draw is cleaned up.
      const draw = state.drawState;
      if (draw?.kind === "zone" || draw?.kind === "shape") {
        m.set(
          "pcbnew.InteractiveDrawing.deleteLastPoint",
          pcbOnly(() => {
            // drawArc / drawOneBezier: `arcManager.RemoveLastPoint()` / `bezierManager.RemoveLastPoint()` -- the construction manager steps back one point.
            if (draw.kind === "shape" && draw.arc) {
              const g = arcRemoveLastPoint(draw.arc);
              dispatch({ type: "SET_DRAW_STATE", draw: g.step === 0 ? null : { ...draw, arc: g, pts: arcClickPoints(g) } });
              return;
            }
            if (draw.kind === "shape" && draw.bezier) {
              const g = bezierRemoveLastPoint(draw.bezier);
              dispatch({ type: "SET_DRAW_STATE", draw: g.step === 0 ? null : { ...draw, bezier: g } });
              return;
            }
            dispatch({ type: "SET_DRAW_STATE", draw: draw.pts.length <= 1 ? null : { ...draw, pts: draw.pts.slice(0, -1) } });
          })
        );
      }

      // ---- incWidth / decWidth (Ctrl++ / Ctrl+-) -- drawing_tool.cpp DRAWING_TOOL::DrawSegment: `m_stroke.SetWidth( width +/- WIDTH_STEP )` while a graphic tool is armed.
      if (shapeToolActive) {
        m.set("pcbnew.InteractiveDrawing.incWidth", pcbOnly(() => dispatch({ type: "PCBX", patch: { drawStrokeWidthUm: stepStrokeWidth(state.pcbx.drawStrokeWidthUm, 1) } })));
        m.set("pcbnew.InteractiveDrawing.decWidth", pcbOnly(() => dispatch({ type: "PCBX", patch: { drawStrokeWidthUm: stepStrokeWidth(state.pcbx.drawStrokeWidthUm, -1) } })));
      }

      // ---- lineModeNext (Shift+Space) and the explicit modes -- pcb_viewer_tools.cpp PCB_VIEWER_TOOLS::NextLineMode: DIRECT -> DEG45 -> DEG90 -> DIRECT.
      m.set("pcbnew.EditorControl.lineModeNext", pcbOnly(() => dispatch({ type: "PCBX", patch: { angleSnapMode: nextAngleSnapMode(state.pcbx.angleSnapMode) } })));
      m.set("pcbnew.EditorControl.lineModeFree", pcbOnly(() => dispatch({ type: "PCBX", patch: { angleSnapMode: "direct" } })));
      m.set("pcbnew.EditorControl.lineMode45", pcbOnly(() => dispatch({ type: "PCBX", patch: { angleSnapMode: "45" } })));
      m.set("pcbnew.EditorControl.lineModeOrthonal", pcbOnly(() => dispatch({ type: "PCBX", patch: { angleSnapMode: "90" } })));

      // ---- addCorner (Insert; the extractor kept only macOS's F1) -- pcb_point_editor.cpp PCB_POINT_EDITOR::addCorner: a new corner on the single selected zone's outline, at the point of the nearest edge under the cursor.
      if (state.selection.size === 1 && state.cursorUm) {
        const zone = api.zoneById([...state.selection][0]!);
        const at = state.cursorUm;
        if (zone) {
          m.set(
            "pcbnew.PointEditor.addCorner",
            pcbOnly(() => {
              const idx = findNearestEdgeInsertionIndex(zone.outline, at.x, at.y, Math.max(300, 10 / state.view.scale));
              if (idx == null) return;
              const [sx, sy] = snapPoint(at.x, at.y, board?.snap ?? state.gridUm);
              void api.cmd({ op: "set_zone_outline", id: zone.id, outline: insertCorner(zone.outline, idx, sx, sy).map(([x, y]) => ({ x, y })) });
            })
          );
        }
      }

      // ---- EditFpInFpEditor (Ctrl+E) / EditLibFpInFpEditor (Ctrl+Shift+E) -- board_editor_control.cpp BOARD_EDITOR_CONTROL::EditFpInFpEditor / EditLibFpInFpEditor.
      // The Footprint Editor is its own store; useFootprintEditHotkey.ts performs the request. This model's board footprint IS its library footprint (by name), so both open that name.
      // Not while routing (there Ctrl+E is ContinueFromEnd).
      const openFootprintEditor = (lib: boolean) =>
        pcbOnly(() => {
          const ref = requestSelection().find((id) => api.partByRef(id)?.placed);
          const part = ref ? api.partByRef(ref) : undefined;
          if (!part) return;
          if (!part.footprint) {
            dispatch({ type: "TOAST", message: `${part.ref} has no footprint${lib ? "" : "/package"} to open`, kind: "error" });
            return;
          }
          dispatch({ type: "PCBX", patch: { fpEditRequest: part.footprint } });
        });
      if (!routing) m.set("pcbnew.EditorControl.EditFpInFpEditor", openFootprintEditor(false));
      m.set("pcbnew.EditorControl.EditLibFpInFpEditor", openFootprintEditor(true));

      // ---- lengthTuner.{AmplIncrease,AmplDecrease,SpacingIncrease,SpacingDecrease} (3/4/1/2) -- pns_meander_placer_base.cpp MEANDER_PLACER_BASE::AmplitudeStep / SpacingStep,
      // active while the length-tuning dialog (this app's "tuning tool active") is open.
      if (state.lengthTuningDialogOpen) {
        const tuner = state.pcbx.lengthTuner;
        const selTrack = [...state.selection][0] ? api.trackById([...state.selection][0]!) : undefined;
        const trackWidth = selTrack?.width ?? board?.board_rules?.track_width ?? 250;
        const clearance = board?.board_rules?.clearance ?? 0;
        const setTuner = (patch: Partial<typeof tuner>) => dispatch({ type: "PCBX", patch: { lengthTuner: { ...tuner, ...patch } } });
        m.set("pcbnew.lengthTuner.AmplIncrease", pcbOnly(() => setTuner({ amplitudeUm: amplitudeStep(tuner.amplitudeUm, 1) })));
        m.set("pcbnew.lengthTuner.AmplDecrease", pcbOnly(() => setTuner({ amplitudeUm: amplitudeStep(tuner.amplitudeUm, -1) })));
        m.set("pcbnew.lengthTuner.SpacingIncrease", pcbOnly(() => setTuner({ spacingUm: spacingStep(tuner.spacingUm, 1, trackWidth, clearance) })));
        m.set("pcbnew.lengthTuner.SpacingDecrease", pcbOnly(() => setTuner({ spacingUm: spacingStep(tuner.spacingUm, -1, trackWidth, clearance) })));
      }
      // ---- LengthTuner.Settings (Ctrl+L) -- pcb_tuning_pattern.cpp `lengthTunerSettings`: this app's tuner settings live in the length-tuning dialog, so it opens it on a single straight track.
      m.set(
        "pcbnew.LengthTuner.Settings",
        pcbOnly(() => {
          const refs = [...state.selection];
          if (refs.length !== 1 || !api.trackById(refs[0]!)) return;
          dispatch({ type: "SET_LENGTH_TUNING_DIALOG_OPEN", open: true });
        })
      );

      // ---- RouteSelected (Shift+X) / RouteSelectedFromEnd (Shift+E) / Autoroute (Shift+F) -- router_tool.cpp ROUTER_TOOL::RouteSelected. For every ratsnest line leaving the selected
      // footprints' pads (or selected track ends/vias) a route starts at the item's end; `Autoroute` aims it at the line's far end and keeps it only if it reaches it (otherwise the
      // live session is left for the user to finish); the other two hand the connection to the live route tool, from this end / the far end. The connections run one after the other
      // as source's loop does (components/canvas/routeQueue.ts): finishing one -- or `cancelCurrentItem` -- starts the next, Escape ends the run.
      const routeSelected = (variant: "interactive" | "fromEnd" | "auto") =>
        pcbOnly(async () => {
          if (!board || routing) return; // `if( m_router->RoutingInProgress() ) return 0;`
          const selection = [...state.selection];
          if (selection.length === 0) return;
          const anchors = routeSelectedAnchors(board.parts, ratsnestEdges, board.routing?.tracks ?? [], board.routing?.vias ?? [], selection);
          if (anchors.length === 0) {
            dispatch({ type: "TOAST", message: "Nothing to route: the selection has no unrouted connections.", kind: "info" });
            return;
          }
          dispatch({ type: "SET_SELECTION", refs: [] });
          const width = state.currentTrackWidthUm ?? board.board_rules?.track_width ?? 250;
          const settings = state.routerSettings;
          const handOver = async (at: [number, number], layer: string): Promise<QueueOutcome> => {
            dispatch({ type: "SET_ACTIVE_TOOL", tool: "route" });
            return (await startInteractiveRoute(at[0], at[1], layer, width, settings, dispatch)) ? "live" : "done";
          };
          let done = 0;
          const runAnchor = (a: (typeof anchors)[number]) => async (): Promise<QueueOutcome> => {
            const layer = routeStartLayer(a, copperLayers, state.activeLayer);
            if (variant !== "auto") return handOver(variant === "fromEnd" ? a.target : a.at, layer);
            const started = await routeStart(a.at[0], a.at[1], layer, width, settings.mode, settings.removeLoops);
            if (!started.ok) return "done";
            const head = await routeMove(a.target[0], a.target[1]);
            // AttemptFinish: only a head that reaches the far end without colliding completes by itself.
            if (head.ok && !head.colliding && head.snapped_end && (await routeFinish(a.target[0], a.target[1])).ok) {
              done++;
              return "done";
            }
            await routeCancel();
            await api.refresh();
            dispatch({ type: "TOAST", message: `Autorouted ${done} of ${anchors.length} connections; finish the next one by hand.`, kind: "info" });
            return handOver(a.at, layer);
          };
          await startRouteQueue(
            anchors.map(runAnchor),
            dispatch,
            variant === "auto"
              ? () => {
                  void api.refresh();
                  dispatch({ type: "TOAST", message: `Autorouted ${done} of ${anchors.length} connections.`, kind: done === anchors.length ? "info" : "error" });
                }
              : null
          );
        });
      m.set("pcbnew.InteractiveRouter.RouteSelected", routeSelected("interactive"));
      m.set("pcbnew.InteractiveRouter.RouteSelectedFromEnd", routeSelected("fromEnd"));
      m.set("pcbnew.InteractiveRouter.Autoroute", routeSelected("auto"));

      // ---- ContinueFromEnd (Ctrl+E while routing) -- router_tool.cpp `routerContinueFromEnd` -> pns_router.cpp ROUTER::ContinueFromEnd: restart the route from the far end of its
      // ratsnest line. Only before anything was fixed (source's `HasPlacedAnything()` false case: nothing to carry over); the live session is replaced.
      if (routing && state.drawState?.kind === "route") {
        const live = state.drawState;
        m.set(
          "pcbnew.InteractiveRouter.ContinueFromEnd",
          pcbOnly(async () => {
            if ((live.runs?.length ?? 0) > 0 || live.via || live.pts.length === 0) {
              dispatch({ type: "TOAST", message: "Route From Other End: only available before the first segment is fixed.", kind: "error" });
              return;
            }
            const far = otherEndOfStart(live.pts[0]!, live.net, ratsnestEdges);
            if (!far) {
              dispatch({ type: "TOAST", message: "Route From Other End: this route does not start on an unrouted connection.", kind: "error" });
              return;
            }
            cancelInteractiveRoute(dispatch);
            await startInteractiveRoute(far[0], far[1], live.layer, live.width, state.routerSettings, dispatch);
          })
        );
      }
    }

    // ===================================================================
    // The pcbnew rows of the hotkeyed-and-missing sweep (docs/parity/
    // UI-ACTIONS.md): drawing-tool postures and the footprint editor's own
    // actions. Each cites the KiCad function it ports (pcbnew/tools/*.cpp
    // at 8303b2ad); the footprint editor is its own store, so those handlers
    // read it at CALL time through `fpApi.getState()`.
    // ===================================================================
    {
      // `/` (arcPosture) -- DRAWING_TOOL::drawArc: `arcManager.ToggleClockwise()` -- reverse which way round the arc under construction goes (and lock it).
      // The board tab registers it only while an arc is actually being drawn, so `/` stays free for the router's own posture flip the rest of the time.
      if (state.tab === "footprint" || (state.tab === "pcb" && state.drawState?.kind === "shape" && state.drawState.arc)) {
        m.set("pcbnew.InteractiveDrawing.arcPosture", () => {
          if (state.tab === "pcb") {
            const draw = state.drawState;
            if (draw?.kind === "shape" && draw.arc) dispatch({ type: "SET_DRAW_STATE", draw: { ...draw, arc: arcToggleClockwise(draw.arc) } });
          } else {
            const draw = fpApi.getState().drawState;
            if (draw?.arc) fpDispatch({ type: "SET_DRAW_STATE", draw: { ...draw, arc: arcToggleClockwise(draw.arc) } });
          }
        });
      }

      // Ctrl+Shift+B (bezier) -- DRAWING_TOOL::DrawBezier: arm "Draw Bezier Curve" (start, control 1, end, control 2; the curve chains), in either frame.
      m.set("pcbnew.InteractiveDrawing.bezier", () => {
        if (state.tab === "pcb") dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "draw_bezier" ? "select" : "draw_bezier" });
        else if (state.tab === "footprint") fpDispatch({ type: "SET_ACTIVE_TOOL", tool: fpApi.getState().activeTool === "draw_bezier" ? "select" : "draw_bezier" });
      });

      // Ctrl+Shift+N (setAnchor) -- DRAWING_TOOL::SetAnchor: "Make sense only in FP editor". Arms the anchor tool; its single click
      // moves the anchor (`footprint->MoveAnchorPosition`, `Cmd::SetFootprintAnchor`) and the tool is popped.
      m.set("pcbnew.InteractiveDrawing.setAnchor", () => {
        if (state.tab !== "footprint" || !fpApi.getState().name) return;
        const fs = fpApi.getState();
        fpDispatch({ type: "SET_ACTIVE_TOOL", tool: fs.activeTool === "anchor" ? "select" : "anchor" });
      });

      // Ctrl+N (newFootprint) -- FOOTPRINT_EDITOR_CONTROL::NewFootprint -> PCB_BASE_FRAME::CreateNewFootprint: an empty SMD footprint called Untitled (made unique).
      m.set("pcbnew.ModuleEditor.newFootprint", () => {
        if (state.tab === "footprint") void fpApi.newFootprint();
      });

      // Ctrl+Shift+D (duplicateIncrementPads) -- EDIT_TOOL::Duplicate with `increment`: the footprint editor copies the selected pads (taking the next
      // pad numbers), graphics and text and picks the copies up; on the board tab the increment only ever applies to pads, so it is a plain Duplicate.
      m.set("pcbnew.InteractiveEdit.duplicateIncrementPads", () => {
        if (state.tab === "pcb") void api.duplicateSelection();
        else if (state.tab === "footprint") void fpApi.duplicateSelection(true);
      });

      // P (packAndMoveFootprints) -- EDIT_TOOL::PackAndMoveFootprints: the selected footprints (locked ones filtered out; the hovered one when nothing
      // is selected) are packed by `SpreadFootprints` into a compact, reference-ordered block whose top-left is the selection's own top-left, then the
      // Move tool picks the whole block up. The packed layout is a preview until the drop (one `move_to` batch = one undo step); Escape reverts it.
      m.set("pcbnew.InteractiveEdit.packAndMoveFootprints", () => {
        if (state.tab !== "pcb" || !state.board) return;
        // `if( isRouterActive() || m_dragging ) { wxBell(); return 0; }`
        const routing = state.drawState?.kind === "route" || state.drawState?.kind === "drag" || state.drawState?.kind === "diffpair";
        if (routing || state.activeTool === "move" || state.armed) return;
        const plan = planPack(state.board.parts, requestSelection(), new Set(state.board.locked ?? []));
        if (!plan) return;
        dispatch({ type: "SET_SELECTION", refs: plan.refs });
        dispatch({ type: "SET_ACTIVE_TOOL", tool: "move" });
        dispatch({ type: "SET_MOVE_ORIGIN", at: state.cursorUm });
        dispatch({ type: "SET_MOVE_PREVIEW", preview: { refs: plan.refs, kind: "part", dxUm: 0, dyUm: 0, perRefOffsetUm: plan.offsets } });
      });
    }

    // ===================================================================
    // The eeschema rows of the hotkeyed-and-missing sweep (docs/parity/
    // UI-ACTIONS.md): net-item navigation, the schematic drawing/edit tools
    // that were missing, and the Symbol Editor's own actions. Each cites the
    // KiCad function it ports (eeschema/ at 8303b2ad). The Simulation rows,
    // Import Graphics, Place Design Block and Autoplace Fields are recorded
    // in tools/ui-parity-missing.json with the subsystem they need.
    // ===================================================================
    {
      // Tab / Shift+Tab (nextNetItem / previousNetItem) -- SCH_SELECTION_TOOL::SelectNext/SelectPrevious -> SCH_EDIT_FRAME::SelectNextPrevNetNavigatorItem:
      // only with a net highlighted and the selected item on it (`IsBrightened()`); selects the next/previous of the net's items, wrapping.
      const stepNetNavigator = (forward: boolean) => () => {
        if (state.tab !== "schematic" || !state.schematic || !state.netHighlight || state.selection.size === 0) return;
        const s = state.schematic;
        const nav: NavSchematic = {
          pins: s.symbols.flatMap((sym) => (resolveLibSymbol(sym, s.lib_symbols)?.pins ?? []).map((p) => ({ ref: sym.id, number: p.pin.number, name: p.pin.name, tip: p.tip }))),
          wires: s.wires,
          labels: s.labels,
          powerSymbols: s.power_symbols,
          noConnects: s.no_connects,
        };
        const items = netNavigatorItems(nav, state.netHighlight, state.units);
        const first = [...state.selection][0]!;
        if (!items.some((it) => it.owner === first)) return;
        const next = stepNetItem(items, netNavKey.current, first, forward);
        if (!next) return;
        netNavKey.current = next.key;
        dispatch({ type: "SET_SELECTION", refs: [next.owner] });
      };
      m.set("eeschema.EditorControl.nextNetItem", stepNetNavigator(true));
      m.set("eeschema.EditorControl.previousNetItem", stepNetNavigator(false));

      // J (placeJunction) / I (drawLines) / S (drawSheet): arm the tool; SchematicView.tsx's onPointerDown owns the click behaviour.
      m.set("eeschema.InteractiveDrawing.placeJunction", toggleSchTool("sch_junction"));
      m.set("eeschema.InteractiveDrawingLineWireBus.drawLines", toggleSchTool("sch_line"));
      m.set("eeschema.InteractiveDrawing.drawSheet", toggleSchTool("sch_sheet"));

      // C (unfoldBus) -- SCH_LINE_WIRE_BUS_TOOL::UnfoldBus: with the cursor on a bus, pick one of its member nets (`BUS_UNFOLD_MENU`, a dialog here);
      // a bus entry then roots on the bus at the cursor and a wire is drawn from its far end, the member's label going on the wire's end.
      m.set(
        "eeschema.InteractiveDrawingLineWireBus.unfoldBus",
        schematicOnly(() => {
          const c = state.cursorUm;
          if (!sch || !c) return;
          const bus = hitBus(sch, c.x, c.y, 10 / (state.schematicView.scale || 1), cursorSnapped() ?? undefined);
          if (!bus) {
            dispatch({ type: "TOAST", message: "Unfold from Bus: put the cursor on a bus first.", kind: "info" });
            return;
          }
          if (bus.members.length === 0) {
            dispatch({ type: "TOAST", message: `${bus.net} has no member nets to unfold.`, kind: "info" });
            return;
          }
          dispatch({ type: "SET_BUS_UNFOLD_PICKER", picker: { bus: bus.net, entryAt: bus.at, members: bus.members } });
        })
      );

      // Alt+S (swap) -- SCH_EDIT_TOOL::Swap: positions (and, for two instances of one library symbol, orientations) of the selected items are exchanged
      // pairwise in selection order -- a Batch of `swap_sch_items`, one undo step.
      m.set(
        "eeschema.InteractiveEdit.swap",
        schematicOnly(() => {
          if (!sch) return;
          const swappable = (id: string) => Boolean(api.symbolById(id) || sch.labels.some((l) => l.id === id) || sch.texts.some((t) => t.id === id) || sch.power_symbols.some((p) => p.id === id));
          const ids = [...state.selection].filter(swappable);
          if (ids.length < 2) return;
          void api.cmdBatch(ids.slice(0, -1).map((a, i) => ({ op: "swap_sch_items" as const, a, b: ids[i + 1]! })));
        })
      );

      // F1 (Insert off macOS) (repeatDrawItem) -- SCH_EDIT_TOOL::RepeatDrawItem: place another copy of what was last placed (see kicad-port/schRepeat.ts).
      m.set(
        "eeschema.InteractiveEdit.repeatDrawItem",
        schematicOnly(() => {
          if (!sch || state.schRepeat.length === 0) return;
          const cmds = repeatCmds(state.schRepeat, { cursor: cursorSnapped(), nextReference: (id) => nextReference(sch.symbols, refDesPrefix(id), state.board?.parts.map((p) => p.ref)) });
          if (cmds.length > 0) void api.cmdBatch(cmds);
        })
      );

      // P (placeSymbolPin) -- SYMBOL_EDITOR_PIN_TOOL / SYMBOL_EDITOR_DRAWING_TOOLS::PlacePin: arms the Symbol Editor's pin tool (a click places a pin whose number
      // and name take the next value, `IncrementString`); the hotkey again leaves it.
      m.set("eeschema.SymbolDrawing.placeSymbolPin", () => {
        if (state.tab !== "symbol") return;
        symDispatch({ type: "SET_ACTIVE_TOOL", tool: symApi.getState().activeTool === "pin" ? "select" : "pin" });
      });

      // Ctrl+N (newSymbol) -- SYMBOL_EDIT_FRAME::CreateNewSymbol: an empty symbol, "Untitled" made unique, opened in the editor.
      m.set("eeschema.SymbolLibraryControl.newSymbol", () => {
        if (state.tab === "symbol") void symApi.newSymbol();
      });

      // Ctrl+Shift+S (saveLibraryAs) -- SYMBOL_EDIT_FRAME::saveLibrary( ..., aNewFile ): the library as a new `.kicad_sym`, here a download of the
      // whole project library.
      m.set("eeschema.SymbolLibraryControl.saveLibraryAs", () => {
        if (state.tab === "symbol") void symApi.exportLibraryKicadSym();
      });
    }

    // The schematic edit and drawing tools (Lock, Change To, Break, shapes, sheet pins, ...) -- actions/schEditActions.ts. Registered before the library editors'
    // below: both chain on a name they share (drawRectangle, drawCircle, drawArc) so each editor keeps its own tab's handler, in either order.
    registerSchEditActions(m, { state, dispatch, api, symApi, symDispatch, requestSelection, adoptHovered, cursorSnapped });
    // The pcbnew edit-tool rows (router modes, Mirror, Fillet/Chamfer/Dogbone/Extend Lines, polygon booleans, ...): actions/pcbEditSweep.ts.
    registerPcbEditSweep(m, { state, dispatch, api, requestSelection });

    // The two library editors' own actions (pcbnew.ModuleEditor.*, pcbnew.PadTool.*, eeschema.SymbolLibraryControl.*, SymbolDrawing.*, PinEditing.*).
    registerLibraryEditorActions(m, { tab: state.tab, studioDispatch: dispatch, boardParts: (state.board?.parts ?? []).map((p) => ({ ref: p.ref, footprint: p.footprint })), fpApi, fpDispatch, symApi, symDispatch });
    // The shared actions that work in every editor (actions/commonActions.ts). The adapter is built when an action runs: the
    // library editors' stores change without this registry being rebuilt.
    registerCommonActions(m, {
      tab: state.tab,
      getAdapter: () => {
        const studio = api.getState();
        return makeEditorAdapter({ tab: studio.tab, studio, fp: fpApi.getState(), sym: symApi.getState(), dispatch, fpDispatch, symDispatch, api, fpApi, symApi });
      },
      state,
      dispatch,
      api,
      fpApi,
      fpDispatch,
      symApi,
      symDispatch,
    });

    // The frame-wide actions (zoom, grid, save, print, library tree, select) of those two editors, on their own canvas and store, and the unregistering of the toolbar actions they do not support.
    // After the shared actions above: on those two tabs the frame's own handler is the one that stands where both have one.
    registerEditorFrameActions(m, { tab: state.tab, fpApi, fpDispatch, symApi, symDispatch });

    // The schematic editor's control actions (eeschema.EditorControl / NavigateTool / InspectionTool / Interactive.increment*).
    registerSchControlActions(m, { tab: state.tab, state, api, dispatch, requestSelection, symApi, symDispatch, control: schControl, controlDispatch: schControlDispatch });

    return m;
  }, [api, dispatch, state, symApi, symDispatch, fpApi, fpDispatch, schControl, schControlDispatch]);

  // `registry.has(name)` alone used to be the whole check, but the
  // registry holds EVERY action's handler regardless of tab (every
  // `pcbOnly`/`schematicOnly` wrapper above is itself registered
  // unconditionally -- only the function *body* it wraps checks the
  // tab, and only once called). That made `isEnabled` blind to tab at
  // exactly the place useGlobalHotkeys.ts needs it most: several
  // physical keys are double-booked by one `pcbnew.*` and one
  // `eeschema.*` action with the *same* hotkey (R, M, G, X, E, U, V, F
  // all collide this way in the real, extracted hotkey table), and
  // `hotkeyIndex.get(combo)?.find(isEnabled)` there picks the first
  // name `isEnabled` accepts -- so whichever of the pair happened to
  // come first in actions.json's own order permanently shadowed the
  // other, on *every* tab, regardless of which one was actually
  // relevant. `pcbnew.InteractiveEdit.rotateCcw` (R) and
  // `pcbnew.InteractiveMove.move` (M) were already losing that race to
  // their eeschema counterparts before this fix -- real, silent PCB
  // regressions from the schematic port's own earlier sessions, not
  // hypothetical. `isActionEnabledForTab` (kicad-port, unit tested) is
  // the actual fix; MenuBar.tsx/Toolbar.tsx already load an entirely
  // separate, per-tab menu/toolbar tree each (menus.json vs
  // sch_menus.json, same for toolbars), so this changes nothing for
  // them -- they never asked `isEnabled` about an action from the
  // other tab's tree in the first place. HotkeysDialog.tsx is the one
  // visible side effect: it lists every action in one place, so an
  // eeschema action now dims while looking at it from the PCB tab (and
  // vice versa) -- arguably more honest ("usable right now" instead of
  // "usable somewhere"), not a regression.
  const isEnabled = useCallback((name: string) => isActionEnabledForTab(name, state.tab, registry.has(name)), [registry, state.tab]);
  /** `param` is the event parameter KiCad's parameterised actions carry (`zoomPreset`'s entry, `selectItems`' items, `changeSheet`'s path ...). */
  const run = useCallback((name: string, param?: unknown) => registry.get(name)?.(param), [registry]);
  /**
   * The check a menu entry or toolbar button shows for a toggle action (View > Show Hidden Pins, the Net Navigator panel, Edit > Attributes > Do not
   * Populate ...): `true`/`false` for a toggle, `undefined` for an action that has none.
   */
  const isChecked = useCallback(
    (name: string): boolean | undefined => {
      // The shared tools' toggles (Always Show Crosshairs, the crosshair mode, Draw Bounding Boxes, the selection mode, Library Tree), in every editor.
      if (registry.has(name)) {
        const shared = commonChecked(name, commonOptions);
        if (shared !== undefined) return shared;
      }
      if (state.tab !== "schematic" || !registry.has(name)) return undefined;
      const selection = state.selection;
      const sch = state.schematic;
      return schControlChecked(name, {
        control: schControl,
        state,
        requestSelection: () => (selection.size > 0 ? [...selection] : sch && state.cursorUm ? [hitSymbol(sch, state.cursorUm.x, state.cursorUm.y)].filter((id): id is string => !!id) : []),
      });
    },
    [registry, schControl, state, commonOptions]
  );
  /** Every action id the registry has a handler for on this tab (the scripted test hook, `window.__eda`, lists them). */
  const actionNames = useMemo(() => [...registry.keys()], [registry]);
  return { run, isEnabled, isChecked, actionNames };
}
