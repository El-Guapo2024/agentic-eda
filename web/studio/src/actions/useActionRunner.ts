// Bridges KiCad's dotted action names (src/kicad/actions.json, referenced
// by menus.json/toolbars.json) to this app's own implementations.
//
// Now that extraction produces real, verified names (538 actions,
// cross-checked against source for several), the registry below wires
// up the ones this app actually implements. Everything else stays
// disabled with "(not ported yet)" in the menu bar and toolbars, which
// is the correct, honest state for the rest until it's built.
//
// Not wired here despite being implemented: pcbnew.InteractiveMove.move
// ("M") -- its "arm, then click/move to commit" state lives as local
// component state in components/canvas/Canvas.tsx, not this app's
// global store, so a menu/toolbar click can't trigger the same flow
// yet. Its keyboard shortcut still works (Canvas.tsx handles it
// directly); lifting that state so the registry can reach it too is a
// follow-up, not done in this pass.

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
    m.set("common.Control.zoomIn", () => zoomAtCenter(1.5));
    m.set("common.Control.zoomOut", () => zoomAtCenter(1 / 1.5));
    m.set("common.Control.zoomCenter", () => zoomAtCenter(1));
    m.set("common.Control.zoomRedraw", () => zoomAtCenter(1));

    m.set("pcbnew.Control.showLayersManager", () => dispatch({ type: "SET_RIGHT_DOCK_TAB", tab: "appearance" }));
    m.set("common.Control.showProperties", () => {}); // properties panel is always visible in this layout; a no-op is the correct behavior, not a missing feature

    return m;
  }, [api, dispatch, state]);

  const isEnabled = useCallback((name: string) => registry.has(name), [registry]);
  const run = useCallback((name: string) => registry.get(name)?.(), [registry]);
  return { run, isEnabled };
}
