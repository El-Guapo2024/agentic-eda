// The one button of the board editor's main toolbar that is the studio's own: "Route" -- run the autorouter over every unrouted connection (POST
// /api/route). KiCad has no such toolbar button (its router is interactive; an autorouter is a plugin), so there is no KiCad action or icon for it; it
// takes KiCad's generic "run" icon and sits after the extracted toolbar. Everything this strip used to hold beyond it -- Undo, Redo, Fit, DRC, the editor
// switches -- is a button of the KiCad toolbar itself (or the tab strip) and was a second, glyph-only copy of it.
import { useStudioApi, useStudioState } from "../state/store";
import { ActionIcon } from "./Toolbar";

export function QuickActions() {
  const state = useStudioState();
  const api = useStudioApi();
  if (state.tab !== "pcb") return null;
  const routing = state.board?.job === "running";
  return (
    <>
      <div className="toolbar-separator" role="separator" />
      <button className="toolbar-button" title={routing ? "Routing…" : "Route: autoroute every unrouted connection"} disabled={routing} onClick={() => api.route()}>
        <ActionIcon iconName="sim_run" />
      </button>
    </>
  );
}
