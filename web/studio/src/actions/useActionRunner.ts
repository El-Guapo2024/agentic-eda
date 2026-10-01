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
import { useStudioApi, useStudioDispatch, useStudioState, type ToolId } from "../state/store";
import { isActionEnabledForTab } from "../kicad-port/actionTabGate";
import { zoomAbout, fitTransform, boundsOfPoints, worldToScreen, panByWorldDelta, screenToWorld } from "../components/canvas/view";
import { commitRoute, dropViaAndSwitchLayer } from "../components/canvas/routing";
import { openPropertiesFor } from "../components/canvas/properties";
import { findNetAtCursor } from "../components/canvas/netAtCursor";
import { expandConnection, type ConnTrack, type ConnVia, type StartPoint } from "../kicad-port/expandConnection";
import { GRID_OPTIONS_UM } from "../components/Toolbar";
import { computeDragAttachment } from "../components/schematic/wireAttachment";

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
      // The Schematic tab's only moveable kind is a symbol -- checked
      // first so a ref that happens to share an id with nothing on the
      // PCB side (every schematic symbol's id IS a part reference, so
      // api.partByRef would also resolve on the Schematic tab) still
      // lands on "symbol"/"symbol_drag", not "part". `activeTool ===
      // "drag"` only matters here before the first pointer-move after
      // arming `G` (state.movePreview still null) -- once a preview
      // exists it already carries its own correct kind.
      const kind = state.movePreview?.kind ?? (state.activeTool === "drag" ? "symbol_drag" : state.tab === "schematic" ? "symbol" : api.viaById(first) ? "via" : api.shapeById(first) ? "shape" : api.textById(first) ? "text" : "part");
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
      m.set(
        "pcbnew.InteractiveEdit.flip",
        pcbOnly(() => {
          if (!tryTransformDuringMove(0, true)) api.flipSelection();
        })
      );
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
    m.set("common.Interactive.cancel", () => dispatch({ type: "ESCAPE" }));

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
    // `G` ("Drag", sch_move_tool.cpp): same arm-then-click-to-drop flow as
    // `M`, except the wire endpoints attached to the selection's own pins
    // (computeDragAttachment -- resolved now, before the symbol moves out
    // from under them) rubber-band along with it instead of being left
    // dangling. See state.dragAttach's own doc and MovePreview's
    // "symbol_drag" kind.
    m.set(
      "eeschema.InteractiveMove.drag",
      schematicOnly(() => {
        const first = [...state.selection][0];
        if (!first || !api.symbolById(first) || !state.schematic) return;
        dispatch({ type: "SET_ACTIVE_TOOL", tool: "drag" });
        dispatch({ type: "SET_MOVE_ORIGIN", at: state.cursorUm });
        dispatch({ type: "SET_DRAG_ATTACH", attach: computeDragAttachment(state.schematic, [...state.selection]) });
      })
    );

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

    // `E`/`U`/`V`/`F` (sch_edit_tool.cpp::Properties/EditField): one
    // shared dialog for all four -- see SymbolPropertiesDialog.tsx's own
    // header comment on why U/V/F don't get source's own separate, far
    // smaller single-field dialog.
    const openSymbolProperties = (field: "reference" | "value" | "footprint" | "datasheet" | null) =>
      schematicOnly(() => {
        const id = [...state.selection][0];
        if (id && api.symbolById(id)) dispatch({ type: "SET_SYMBOL_PROPERTIES", value: { id, field } });
      });
    m.set("eeschema.InteractiveEdit.properties", openSymbolProperties(null));
    m.set("eeschema.InteractiveEdit.symbolProperties", openSymbolProperties(null));
    m.set("eeschema.InteractiveEdit.editReference", openSymbolProperties("reference"));
    m.set("eeschema.InteractiveEdit.editValue", openSymbolProperties("value"));
    m.set("eeschema.InteractiveEdit.editFootprint", openSymbolProperties("footprint"));

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
    // `A`: unlike the others above, this opens the chooser dialog first
    // (real source's own order too, for this one tool -- see
    // SymbolChooserDialog.tsx's header comment) rather than arming a tool
    // directly; confirming a choice there is what arms `sch_place_symbol`.
    m.set("eeschema.InteractiveDrawing.placeSymbol", schematicOnly(() => dispatch({ type: "SET_SYMBOL_CHOOSER_OPEN", open: true })));

    return m;
  }, [api, dispatch, state]);

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
  const run = useCallback((name: string) => registry.get(name)?.(), [registry]);
  return { run, isEnabled };
}
