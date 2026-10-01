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

import { useCallback, useMemo } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { zoomAbout, fitTransform, boundsOfPoints, worldToScreen, panByWorldDelta, screenToWorld } from "../components/canvas/view";
import { finishInteractiveRoute, cancelInteractiveRoute } from "../components/canvas/routing";
import { routeMove, routeToggleVia, routeUndoSegment } from "../api/client";
import { drawStateFromPreview } from "../kicad-port/routeTool";
import { openPropertiesFor } from "../components/canvas/properties";
import { findNetAtCursor } from "../components/canvas/netAtCursor";
import { expandConnection, type ConnTrack, type ConnVia, type StartPoint } from "../kicad-port/expandConnection";
import { GRID_OPTIONS_UM } from "../components/Toolbar";

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

  const registry = useMemo(() => {
    const m = new Map<string, () => void>();
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
      const moving = state.activeTool === "move" || state.movePreview != null;
      if (!moving) return false;
      const refs = state.movePreview?.refs ?? [...state.selection];
      if (refs.length === 0) return false;
      const first = refs[0]!;
      // The Schematic tab's only moveable kind is a symbol -- checked
      // first so a ref that happens to share an id with nothing on the
      // PCB side (every schematic symbol's id IS a part reference, so
      // api.partByRef would also resolve on the Schematic tab) still
      // lands on "symbol", not "part".
      const kind = state.movePreview?.kind ?? (state.tab === "schematic" ? "symbol" : api.viaById(first) ? "via" : api.shapeById(first) ? "shape" : api.textById(first) ? "text" : "part");
      const base = state.movePreview ?? { refs, kind, dxUm: 0, dyUm: 0 };
      const rotateQuarterTurns = addQuarterTurns ? (((base.rotateQuarterTurns ?? 0) + addQuarterTurns) % 4 + 4) % 4 : base.rotateQuarterTurns;
      const flipped = toggleFlip ? !base.flipped : base.flipped;
      dispatch({ type: "SET_MOVE_PREVIEW", preview: { ...base, rotateQuarterTurns, flipped } });
      return true;
    };

    m.set(
      "pcbnew.InteractiveEdit.rotateCcw",
      pcbOnly(() => {
        if (!tryTransformDuringMove(1, false)) api.rotateSelection(1);
      })
    );
    m.set(
      "pcbnew.InteractiveEdit.rotateCw",
      pcbOnly(() => {
        if (!tryTransformDuringMove(3, false)) api.rotateSelection(3);
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
    m.set(
      "common.Interactive.cut",
      pcbOnly(() => {
        api.copySelection();
        m.get("common.Interactive.delete")?.();
      })
    );

    m.set("common.Interactive.delete", () => {
      // One selection can only ever be one kind of thing at a time in
      // practice (Canvas.tsx/SchematicView.tsx's hit-testing always
      // replaces the selection with a single item; shift-click can still
      // mix kinds by accumulating them), so this deletes each ref through
      // whichever Cmd actually matches what it is. Tab-scoped the same
      // way `common.Interactive.undo`/`redo` are now scoped (GAPS.md
      // #15): a schematic ref deleted from the PCB tab, or vice versa,
      // would otherwise be a silent no-op that still cleared the
      // selection -- same bug shape as the undo one, if either tab's
      // branch ran unconditionally.
      const refs = [...state.selection];
      dispatch({ type: "CLEAR_SELECTION" });
      for (const id of refs) {
        if (state.tab === "schematic") {
          if (api.symbolById(id)) api.deleteSymbol(id);
          else if (api.wireById(id)) api.cmd({ op: "delete_wire", id });
        } else if (state.tab === "pcb") {
          if (api.trackById(id)) api.cmd({ op: "delete_track", id });
          else if (api.viaById(id)) api.cmd({ op: "delete_via", id });
          else if (api.zoneById(id)) api.cmd({ op: "delete_zone", id });
          else if (api.shapeById(id)) api.cmd({ op: "delete_shape", id });
          else if (api.textById(id)) api.cmd({ op: "delete_text", id });
          else if (api.partByRef(id)?.placed) api.cmd({ op: "rip", part: id });
        }
      }
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
          const toLayer = state.board.layers.find((l) => l !== draw.layer) ?? draw.layer;
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
        const next = WIDTH_PRESETS_UM[Math.min(WIDTH_PRESETS_UM.length - 1, Math.max(0, (i < 0 ? WIDTH_PRESETS_UM.length - 1 : i) + dir))]!;
        dispatch({ type: "SET_DRAW_STATE", draw: { ...draw, width: next } });
        routeMove(cursor.x, cursor.y, undefined, next).then((preview) => {
          if (preview.ok) dispatch({ type: "SET_DRAW_STATE", draw: drawStateFromPreview({ ...draw, width: next }, preview) });
        });
      };
      m.set("pcbnew.EditorControl.trackWidthInc", pcbOnly(() => cycleWidth(1)));
      m.set("pcbnew.EditorControl.trackWidthDec", pcbOnly(() => cycleWidth(-1)));
    } else {
      m.set(
        "pcbnew.InteractiveEdit.flip",
        pcbOnly(() => {
          if (!tryTransformDuringMove(0, true)) api.flipSelection();
        })
      );
    }
    m.set(
      "pcbnew.InteractiveRouter.SingleTrack",
      pcbOnly(() => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "route" ? "select" : "route" }))
    );
    m.set(
      "pcbnew.InteractiveDrawing.via",
      pcbOnly(() => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "via" ? "select" : "via" }))
    );
    m.set(
      "pcbnew.InteractiveDrawing.zone",
      pcbOnly(() => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "zone" ? "select" : "zone" }))
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
    m.set("common.Interactive.undo", () => api.undo());
    m.set("common.Interactive.redo", () => api.redo());
    m.set("common.Interactive.duplicate", pcbOnly(() => api.duplicateSelection()));
    m.set("common.Interactive.copy", pcbOnly(() => api.copySelection()));
    m.set("common.Interactive.paste", pcbOnly(() => api.pasteClipboard()));
    m.set(
      "pcbnew.InteractiveEdit.moveExact",
      pcbOnly(() => {
        // Only meaningful with something placed/selected to move -- a
        // footprint, or any of item 7's track/via/zone/shape/text (the
        // dialog itself, MoveExactDialog.tsx, resolves which and reads
        // its own position/bbox fresh when it opens).
        if (state.selection.size === 0) return;
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
        const board = state.board;
        if (!board) return;
        const tracks: ConnTrack[] = (board.routing?.tracks ?? []).map((t) => ({ id: t.id, net: t.net, start: t.pts[0]!, end: t.pts[t.pts.length - 1]! })).filter((t) => t.start && t.end);
        const vias: ConnVia[] = (board.routing?.vias ?? []).map((v) => ({ id: v.id, net: v.net, at: [v.x, v.y] }));
        const startPoints: StartPoint[] = [];
        const selectedTrackIds: string[] = [];
        const selectedViaIds: string[] = [];
        for (const id of state.selection) {
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
        if (startPoints.length === 0) return;
        const result = expandConnection(tracks, vias, startPoints, { trackIds: selectedTrackIds, viaIds: selectedViaIds });
        const refs = new Set(state.selection);
        for (const id of result.trackIds) refs.add(id);
        for (const id of result.viaIds) refs.add(id);
        dispatch({ type: "SET_SELECTION", refs: [...refs] });
      })
    );

    const fitToBoard = pcbOnly(() => {
      const rect = canvasRect();
      const bounds = state.board?.outline ? boundsOfPoints(state.board.outline) : null;
      if (!rect || !bounds) return;
      dispatch({ type: "SET_VIEW", view: fitTransform(bounds, rect.width, rect.height) });
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
    const zoomAtCenter = pcbOnly((factor: number) => {
      const rect = canvasRect();
      if (!rect) return;
      dispatch({ type: "SET_VIEW", view: zoomAbout(state.view, rect.width / 2, rect.height / 2, factor) });
    });
    const zoomAtCursor = pcbOnly((factor: number) => {
      const rect = canvasRect();
      if (!rect) return;
      const [px, py] = state.cursorUm ? worldToScreen(state.view, state.cursorUm.x, state.cursorUm.y) : [rect.width / 2, rect.height / 2];
      dispatch({ type: "SET_VIEW", view: zoomAbout(state.view, px, py, factor) });
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
      pcbOnly(() => {
        const rect = canvasRect();
        if (!rect || !state.cursorUm) return;
        const [centerWx, centerWy] = screenToWorld(state.view, rect.width / 2, rect.height / 2);
        dispatch({ type: "SET_VIEW", view: panByWorldDelta(state.view, state.cursorUm.x - centerWx, state.cursorUm.y - centerWy) });
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

    m.set("pcbnew.Control.showLayersManager", () => dispatch({ type: "SET_RIGHT_DOCK_TAB", tab: "appearance" }));
    m.set("common.Control.showProperties", () => {}); // properties panel is always visible in this layout; a no-op is the correct behavior, not a missing feature

    m.set(
      "pcbnew.InteractiveMove.move",
      pcbOnly(() => {
        const first = [...state.selection][0];
        if (!first) return;
        // Tracks and zones have no move_* Cmd (api/types.ts) -- nothing
        // for M to do for them, same as they're excluded from dragging
        // in Canvas.tsx's onPointerDown.
        if (api.trackById(first) || api.zoneById(first)) return;
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
      // Tell the backend's router session to end too (fire-and-forget --
      // see cancelInteractiveRoute's own doc comment) before the ordinary
      // ESCAPE reducer case clears `drawState` locally; otherwise the
      // session would linger server-side until the next `start` silently
      // replaces it.
      if (state.drawState?.kind === "route") cancelInteractiveRoute(dispatch);
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
        const ref = [...state.selection][0];
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
    m.set("common.Control.cursorFullCrosshairs", () => dispatch({ type: "SET_FULLSCREEN_CROSSHAIR", value: true }));
    m.set("common.Control.cursorSmallCrosshairs", () => dispatch({ type: "SET_FULLSCREEN_CROSSHAIR", value: false }));

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
    m.set("pcbnew.ZoneFiller.zoneFillAll", pcbOnly(() => api.fillZones()));
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
    const cycleTrackWidth = (dir: 1 | -1) => {
      const list = trackWidthList();
      const cur = state.currentTrackWidthUm ?? list[0]!;
      const i = list.indexOf(cur);
      // Source's spin control clamps at the list ends rather than
      // wrapping (ADVANCED_CFG has no "wrap" setting for this one).
      const next = list[Math.max(0, Math.min(list.length - 1, (i === -1 ? 0 : i) + dir))]!;
      dispatch({ type: "SET_CURRENT_TRACK_WIDTH", widthUm: next });
      for (const id of state.selection) if (api.trackById(id)) api.cmd({ op: "set_track_width", id, width: next });
    };
    m.set("pcbnew.EditorControl.trackWidthInc", pcbOnly(() => cycleTrackWidth(1)));
    m.set("pcbnew.EditorControl.trackWidthDec", pcbOnly(() => cycleTrackWidth(-1)));

    // pcbnew.EditorControl.viaSizeInc/Dec ("\\"/unbound): same dual-purpose
    // idea as the track-width cycle above, for `routing.via_presets`.
    const viaPresetList = (): { diameter: number; drill: number }[] => {
      const board = state.board;
      const base = { diameter: board?.board_rules?.via_diameter ?? 600, drill: board?.board_rules?.via_drill ?? 300 };
      return [base, ...(board?.routing?.via_presets ?? [])];
    };
    const sameViaPreset = (a: { diameter: number; drill: number }, b: { diameter: number; drill: number }) => a.diameter === b.diameter && a.drill === b.drill;
    const cycleViaPreset = (dir: 1 | -1) => {
      const list = viaPresetList();
      const cur = state.currentViaPreset ?? list[0]!;
      const i = list.findIndex((p) => sameViaPreset(p, cur));
      const next = list[Math.max(0, Math.min(list.length - 1, (i === -1 ? 0 : i) + dir))]!;
      dispatch({ type: "SET_CURRENT_VIA_PRESET", preset: next });
      for (const id of state.selection) if (api.viaById(id)) api.cmd({ op: "edit_via", id, diameter: next.diameter, drill: next.drill });
    };
    m.set("pcbnew.EditorControl.viaSizeInc", pcbOnly(() => cycleViaPreset(1)));
    m.set("pcbnew.EditorControl.viaSizeDec", pcbOnly(() => cycleViaPreset(-1)));

    // dialog_board_setup.cpp -- see BoardSetupDialog.tsx.
    m.set("pcbnew.EditorControl.boardSetup", pcbOnly(() => dispatch({ type: "SET_BOARD_SETUP_DIALOG_OPEN", open: true })));

    const cycleGrid = (dir: 1 | -1) => {
      const i = GRID_OPTIONS_UM.indexOf(state.gridUm);
      const next = GRID_OPTIONS_UM[Math.max(0, Math.min(GRID_OPTIONS_UM.length - 1, (i === -1 ? 0 : i) + dir))]!;
      dispatch({ type: "SET_GRID_UM", um: next });
    };
    m.set("common.Control.gridNext", () => cycleGrid(1));
    m.set("common.Control.gridPrev", () => cycleGrid(-1));

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
      pcbOnly(() => {
        if (!state.board) return;
        const refs: string[] = [];
        if (state.selectionFilter.footprints) for (const p of state.board.parts) if (p.placed) refs.push(p.ref);
        for (const t of state.board.routing?.tracks ?? []) refs.push(t.id);
        for (const v of state.board.routing?.vias ?? []) refs.push(v.id);
        for (const z of state.board.routing?.zones ?? []) refs.push(z.id);
        for (const s of state.board.drawings?.shapes ?? []) refs.push(s.id);
        for (const t of state.board.drawings?.texts ?? []) refs.push(t.id);
        dispatch({ type: "SET_SELECTION", refs });
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

    m.set(
      "eeschema.InteractiveMove.move",
      schematicOnly(() => {
        const first = [...state.selection][0];
        if (!first || !api.symbolById(first)) return;
        dispatch({ type: "SET_ACTIVE_TOOL", tool: "move" });
        dispatch({ type: "SET_MOVE_ORIGIN", at: state.cursorUm });
      })
    );
    // `G` (drag, with wire rubber-banding) isn't ported yet -- see
    // PARITY-sch.md. Bound to plain Move for now (better than a dead key)
    // rather than left silently unregistered.
    m.set("eeschema.InteractiveMove.drag", schematicOnly(() => m.get("eeschema.InteractiveMove.move")?.()));

    m.set(
      "eeschema.InteractiveEdit.rotateCCW",
      schematicOnly(() => {
        if (tryTransformDuringMove(1, false)) return;
        const id = [...state.selection][0];
        if (id && api.symbolById(id)) api.rotateSymbol(id, 1);
      })
    );
    m.set(
      "eeschema.InteractiveEdit.rotateCW",
      schematicOnly(() => {
        if (tryTransformDuringMove(3, false)) return;
        const id = [...state.selection][0];
        if (id && api.symbolById(id)) api.rotateSymbol(id, 3);
      })
    );
    // `Y` (Mirror Vertically) has no IR field to toggle yet -- see
    // `Cmd::MirrorSymbol`'s own doc -- so only `X` is wired.
    m.set(
      "eeschema.InteractiveEdit.mirrorH",
      schematicOnly(() => {
        const id = [...state.selection][0];
        if (id && api.symbolById(id)) api.mirrorSymbol(id);
      })
    );

    m.set("eeschema.InspectionTool.runERC", () => dispatch({ type: "SET_ERC_DIALOG_OPEN", open: true }));

    // `dialog_annotate.cpp`'s own default mode ("Keep existing
    // annotations", not "Reset") -- no dialog yet to offer the reset
    // choice or the sheet/selection scope options, so this always
    // annotates the whole sheet, additively. See `Cmd::Annotate`'s own
    // doc for the numbering scheme (top-to-bottom, per reference prefix).
    m.set(
      "eeschema.EditorControl.annotate",
      schematicOnly(async () => {
        const ok = await api.cmd({ op: "annotate", reset_existing: false });
        dispatch({ type: "TOAST", message: ok ? "Annotated." : "Nothing to annotate.", kind: "info" });
      })
    );

    // `W`: arm/disarm the wire tool -- SchematicView.tsx's own
    // onPointerDown/onDoubleClick own the actual click-to-add-point/
    // finish state machine (same split PCB's route/zone/shape tools use:
    // this registry only ever flips `state.activeTool`).
    m.set(
      "eeschema.InteractiveDrawingLineWireBus.drawWires",
      schematicOnly(() => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "wire" ? "select" : "wire" }))
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

    return m;
  }, [api, dispatch, state]);

  const isEnabled = useCallback((name: string) => registry.has(name), [registry]);
  const run = useCallback((name: string) => registry.get(name)?.(), [registry]);
  return { run, isEnabled };
}
