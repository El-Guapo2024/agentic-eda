// The shared text actions: Left Justify, Center Justify and Right Justify. EDIT_TOOL runs them in the PCB Editor and the Footprint Editor
// (pcbnew/tools/edit_tool.cpp, `Go( &EDIT_TOOL::JustifyText, ACTIONS::leftJustify.MakeEvent() )` ...); the verbs are in
// kicad-port/justifyText.ts. `registerCommonActions` (commonActions.ts) calls `registerTextActions` once while the registry is built.
import type { TextJustify } from "../api/types";
import { boardJustifyCmds, footprintJustifyCmds } from "../kicad-port/justifyText";
import type { EditorAdapter } from "./editorAdapter";
import type { ActionHandler, CommonActionContext } from "./commonActions";

/** `RequestSelection()`: the selection, or -- with nothing selected -- the item under the cursor. */
function requestedIds(adapter: EditorAdapter): string[] {
  if (adapter.selection.size > 0) return [...adapter.selection];
  if (!adapter.cursor) return [];
  const hit = adapter.candidatesAt(adapter.cursor.x, adapter.cursor.y)[0];
  return hit ? [hit.id] : [];
}

export function registerTextActions(m: Map<string, ActionHandler>, ctx: CommonActionContext): void {
  if (ctx.tab !== "pcb" && ctx.tab !== "footprint") return;

  // ACTIONS::leftJustify / centerJustify / rightJustify -- EDIT_TOOL::JustifyText: every selected text and text box gets the horizontal justification, as
  // one undo step; items that are not text are left alone and a locked one is skipped (on the board: `FilterCollectorForLockedItems` is the PCB Editor's only).
  const justify =
    (which: TextJustify): ActionHandler =>
    () => {
      const adapter = ctx.getAdapter();
      if (!adapter) return;
      // `if( isRouterActive() ) { wxBell(); return 0; }`
      if (adapter.tab === "pcb" && (ctx.state.drawState?.kind === "route" || ctx.state.drawState?.kind === "diffpair")) return;
      const ids = requestedIds(adapter);
      if (ids.length === 0) return;
      void (async () => {
        if (adapter.tab === "pcb") {
          const board = ctx.api.getState().board;
          if (!board) return;
          // a locked item is left out silently: JustifyText does not call `ReportFilteredLockedItems` (task-v10-handlers: the studio said "Item locked.")
          const { cmds } = boardJustifyCmds(board, ids, which);
          if (cmds.length > 0) await ctx.api.cmdBatch(cmds);
        } else if (adapter.tab === "footprint") {
          const fp = ctx.fpApi.getState();
          if (!fp.name || !fp.footprint) return;
          const cmds = footprintJustifyCmds(fp.name, fp.footprint.texts, ids, which);
          if (cmds.length > 0) await ctx.fpApi.cmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
        }
      })();
    };
  m.set("common.Control.leftJustify", justify("left"));
  m.set("common.Control.centerJustify", justify("center"));
  m.set("common.Control.rightJustify", justify("right"));
}
