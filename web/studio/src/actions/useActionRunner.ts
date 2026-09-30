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
    // real shared binding), and listHotKeys is the one this session is
    // confident about, so it wins; zoomIn/zoomOut stay unmapped rather
    // than guess which of the two dubious hotkeys to keep.
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

    return m;
  }, [api, dispatch, state]);

  const isEnabled = useCallback((name: string) => registry.has(name), [registry]);
  const run = useCallback((name: string) => registry.get(name)?.(), [registry]);
  return { run, isEnabled };
}
