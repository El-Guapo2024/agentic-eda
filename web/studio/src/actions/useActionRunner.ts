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
import { commitRoute, dropViaAndSwitchLayer } from "../components/canvas/routing";
import { GRID_OPTIONS_UM } from "../components/Toolbar";

function canvasRect(): DOMRect | null {
  return document.querySelector(".pcb-canvas-container")?.getBoundingClientRect() ?? null;
}

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

    m.set("pcbnew.InteractiveEdit.rotateCcw", pcbOnly(() => api.rotateSelection(1)));
    m.set("pcbnew.InteractiveEdit.rotateCw", pcbOnly(() => api.rotateSelection(3)));
    m.set(
      "common.Interactive.delete",
      pcbOnly(() => {
        // One selection can only ever be one kind of thing at a time in
        // practice (Canvas.tsx's hit-testing always replaces the
        // selection with a single item; shift-click can still mix kinds
        // by accumulating them), so this deletes each ref through
        // whichever Cmd actually matches what it is, rather than
        // assuming "selection" always means "footprints" the way
        // ripSelection alone did before tracks/vias/zones/shapes/text
        // existed to select at all.
        const refs = [...state.selection];
        dispatch({ type: "CLEAR_SELECTION" });
        for (const id of refs) {
          if (api.trackById(id)) api.cmd({ op: "delete_track", id });
          else if (api.viaById(id)) api.cmd({ op: "delete_via", id });
          else if (api.zoneById(id)) api.cmd({ op: "delete_zone", id });
          else if (api.shapeById(id)) api.cmd({ op: "delete_shape", id });
          else if (api.textById(id)) api.cmd({ op: "delete_text", id });
          else if (api.partByRef(id)?.placed) api.cmd({ op: "rip", part: id });
        }
      })
    );
    // F is Flip's real KiCad hotkey, but it's also pcbnew.InteractiveRouter.
    // AttemptFinish's while actively routing -- KiCad's own tool stack
    // resolves this by context (which tool currently owns the keyboard),
    // this app's flatter one by only ever registering whichever of the
    // two applies to the current state.drawState, so useGlobalHotkeys'
    // first-enabled-candidate search always lands on the right one.
    if (state.drawState?.kind === "route") {
      m.set(
        "pcbnew.InteractiveRouter.AttemptFinish",
        pcbOnly(() => {
          const draw = state.drawState;
          if (draw?.kind === "route") {
            commitRoute(draw, api.cmd);
            dispatch({ type: "SET_DRAW_STATE", draw: null });
          }
        })
      );
    } else {
      m.set("pcbnew.InteractiveEdit.flip", pcbOnly(() => api.flipSelection()));
    }
    m.set(
      "pcbnew.Control.layerToggle",
      pcbOnly(() => {
        const draw = state.drawState;
        if (draw?.kind !== "route" || !state.board) return;
        dropViaAndSwitchLayer(draw, state.board, api.cmd).then((next) => dispatch({ type: "SET_DRAW_STATE", draw: next }));
      })
    );
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
    m.set("common.Interactive.undo", () => api.undo());
    m.set("common.Interactive.redo", () => api.redo());

    m.set(
      "pcbnew.EditorControl.toggleNetHighlight",
      pcbOnly(() => {
        const ref = [...state.selection][0];
        const part = ref ? api.partByRef(ref) : undefined;
        const net = part?.pads?.[0]?.net ?? null;
        dispatch({ type: "SET_NET_HIGHLIGHT", net: state.netHighlight ? null : net });
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
    m.set("common.Interactive.cancel", () => {
      dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
      dispatch({ type: "CLEAR_SELECTION" }); // also resets activeTool to "select"
    });

    m.set(
      "pcbnew.InteractiveEdit.properties",
      pcbOnly(() => {
        const ref = [...state.selection][0];
        if (!ref) return;
        // "E" opens whichever properties view actually applies to what's
        // selected -- a real Text gets the full edit_text-backed dialog
        // (item 6's own "E to edit"); everything else this app models
        // (a part, or item 7's track/via/zone/shape) is read-only-ish, so
        // it keeps the existing footprint-properties dialog's pattern.
        if (api.textById(ref)) dispatch({ type: "SET_TEXT_DIALOG", dialog: { mode: "edit", id: ref } });
        else if (api.trackById(ref) || api.viaById(ref) || api.zoneById(ref) || api.shapeById(ref)) dispatch({ type: "SET_ITEM_PROPERTIES_ID", id: ref });
        else dispatch({ type: "SET_FOOTPRINT_PROPERTIES_OPEN", open: true });
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

    return m;
  }, [api, dispatch, state]);

  const isEnabled = useCallback((name: string) => registry.has(name), [registry]);
  const run = useCallback((name: string) => registry.get(name)?.(), [registry]);
  return { run, isEnabled };
}
