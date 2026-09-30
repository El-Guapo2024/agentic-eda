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
import { zoomAbout, fitTransform, boundsOfPoints } from "../components/canvas/view";
import { GRID_OPTIONS_UM } from "../components/Toolbar";

function canvasRect(): DOMRect | null {
  return document.querySelector(".pcb-canvas-container")?.getBoundingClientRect() ?? null;
}

export function useActionRunner() {
  const api = useStudioApi();
  const dispatch = useStudioDispatch();
  const state = useStudioState();

  const registry = useMemo(() => {
    const m = new Map<string, () => void>();

    m.set("pcbnew.InteractiveEdit.rotateCcw", () => api.rotateSelection(1));
    m.set("pcbnew.InteractiveEdit.rotateCw", () => api.rotateSelection(3));
    m.set("common.Interactive.delete", () => api.ripSelection());
    m.set("common.Interactive.undo", () => api.undo());
    m.set("common.Interactive.redo", () => api.redo());

    m.set("pcbnew.EditorControl.toggleNetHighlight", () => {
      const ref = [...state.selection][0];
      const part = ref ? api.partByRef(ref) : undefined;
      const net = part?.pads?.[0]?.net ?? null;
      dispatch({ type: "SET_NET_HIGHLIGHT", net: state.netHighlight ? null : net });
    });

    m.set("common.Control.zoomFitScreen", () => {
      const rect = canvasRect();
      const bounds = state.board?.outline ? boundsOfPoints(state.board.outline) : null;
      if (!rect || !bounds) return;
      dispatch({ type: "SET_VIEW", view: fitTransform(bounds, rect.width, rect.height) });
    });
    const zoomAtCenter = (factor: number) => {
      const rect = canvasRect();
      if (!rect) return;
      dispatch({ type: "SET_VIEW", view: zoomAbout(state.view, rect.width / 2, rect.height / 2, factor) });
    };
    m.set("common.Control.zoomInCenter", () => zoomAtCenter(1.5));
    m.set("common.Control.zoomOutCenter", () => zoomAtCenter(1 / 1.5));
    // NOT common.Control.zoomIn/zoomOut here: extraction gave both of
    // those the same "Ctrl+F1"/"Ctrl+F2" hotkeys as zoomInCenter/
    // zoomOutCenter's near-neighbors in source position, which also
    // collides with common.SuiteControl.listHotKeys's real Ctrl+F1 --
    // almost certainly a parser misattribution (regex proximity, not a
    // real shared binding). Leaving them out of the registry is what
    // actually resolves the collision: useGlobalHotkeys tries every
    // action name registered against a given key and fires the first one
    // that isEnabled() -- with zoomIn/zoomOut absent, listHotKeys is the
    // only enabled candidate left for Ctrl+F1, so it wins Ctrl+F1 without
    // this file needing to guess which of the two dubious bindings to
    // keep.
    m.set("common.Control.zoomCenter", () => zoomAtCenter(1));
    m.set("common.Control.zoomRedraw", () => zoomAtCenter(1));
    m.set("common.SuiteControl.listHotKeys", () => dispatch({ type: "SET_HOTKEYS_DIALOG_OPEN", open: true }));

    m.set("pcbnew.Control.showLayersManager", () => dispatch({ type: "SET_RIGHT_DOCK_TAB", tab: "appearance" }));
    m.set("common.Control.showProperties", () => {}); // properties panel is always visible in this layout; a no-op is the correct behavior, not a missing feature

    m.set("pcbnew.InteractiveMove.move", () => {
      if (state.selection.size === 0) return;
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "move" });
      dispatch({ type: "SET_MOVE_ORIGIN", at: state.cursorUm });
    });
    m.set("common.Interactive.cancel", () => {
      dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
      dispatch({ type: "CLEAR_SELECTION" }); // also resets activeTool to "select"
    });

    m.set("pcbnew.InteractiveEdit.properties", () => {
      if (state.selection.size === 0) return;
      dispatch({ type: "SET_FOOTPRINT_PROPERTIES_OPEN", open: true });
    });
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

    return m;
  }, [api, dispatch, state]);

  const isEnabled = useCallback((name: string) => registry.has(name), [registry]);
  const run = useCallback((name: string) => registry.get(name)?.(), [registry]);
  return { run, isEnabled };
}
