// Buttons for behavior this app already implements but can't yet reach
// through the data-driven menu/toolbar system, because that system is
// keyed by KiCad's real dotted action names and this session couldn't
// extract those (see actions/useActionRunner.ts). Appended to the main
// toolbar so they read as part of it rather than a second, competing bar;
// each should fold into a real toolbar button/menu item once
// tools/extract-actions.js has run and the matching action name is added
// to the registry -- Route and Inspect > DRC almost certainly correspond
// to real KiCad actions/menu entries once that happens.
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { boundsOfPoints, fitTransform } from "./canvas/view";

export function QuickActions() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();

  const fit = () => {
    const bounds = state.board?.outline ? boundsOfPoints(state.board.outline) : null;
    const el = document.querySelector(".pcb-canvas-container");
    if (!bounds || !el) return;
    const rect = el.getBoundingClientRect();
    dispatch({ type: "SET_VIEW", view: fitTransform(bounds, rect.width, rect.height) });
  };

  return (
    <>
      <div className="toolbar-separator" role="separator" />
      <button className="toolbar-button" title="Undo (Cmd/Ctrl+Z)" onClick={() => api.undo()}>
        <span aria-hidden>↶</span>
      </button>
      <button className="toolbar-button" title="Redo (Cmd/Ctrl+Shift+Z)" onClick={() => api.redo()}>
        <span aria-hidden>↷</span>
      </button>
      <div className="toolbar-separator" role="separator" />
      <button className="toolbar-button" title="Fit board to window" onClick={fit}>
        <span aria-hidden>⇲</span>
      </button>
      <button className="toolbar-button" title={state.board?.job === "running" ? "Routing…" : "Route (freeroute, /api/route)"} disabled={state.board?.job === "running"} onClick={() => api.route()}>
        <span aria-hidden>↯</span>
      </button>
      <button className="toolbar-button" title="Inspect → DRC" onClick={() => dispatch({ type: "SET_DRC_OPEN", open: true })}>
        <span aria-hidden>✓</span>
      </button>
      <div className="toolbar-separator" role="separator" />
      <button className={`toolbar-button${state.tab === "pcb" ? " active" : ""}`} title="PCB Editor" onClick={() => dispatch({ type: "SET_TAB", tab: "pcb" })}>
        <span aria-hidden>⛁</span>
      </button>
      <button className={`toolbar-button${state.tab === "schematic" ? " active" : ""}`} title="Switch to Schematic Editor" onClick={() => dispatch({ type: "SET_TAB", tab: "schematic" })}>
        <span aria-hidden>⧉</span>
      </button>
      <button className={`toolbar-button${state.tab === "3d" ? " active" : ""}`} title="3D Viewer (Alt+3)" onClick={() => dispatch({ type: "SET_TAB", tab: "3d" })}>
        <span aria-hidden>⬢</span>
      </button>
    </>
  );
}
