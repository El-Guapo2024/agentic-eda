// The board editor's table actions: the shared table tool's (tableActions.ts) wired to the board's tables and selection, and Draw Table.
//
//   pcbnew/tools/drawing_tool.cpp          DRAWING_TOOL::DrawTable      pcbnew.InteractiveDrawing.drawTable: arms the tool (Canvas.tsx has its two clicks)
//   pcbnew/tools/pcb_edit_table_tool.cpp   PCB_EDIT_TABLE_TOOL          the structure edits, Edit Table, Export Table to CSV
//   pcbnew/tools/pcb_selection_tool.cpp    SelectColumns / SelectRows / SelectTable
import type { ActionHandler, CommonActionContext } from "./commonActions";
import { registerTableEditorActions, type TableEditor } from "./tableActions";
import { tablesOf } from "../kicad-port/tables";

/** The board's tables, selection and verbs as the table actions use them. */
export function boardTableEditor(ctx: CommonActionContext): TableEditor {
  return {
    tables: () => tablesOf(ctx.api.getState().board),
    selection: () => [...ctx.api.getState().selection],
    setSelection: (ids) => {
      const a = ctx.getAdapter();
      if (a) a.setSelection(ids);
      else ctx.dispatch({ type: "SET_SELECTION", refs: ids });
    },
    hoverCellId: () => {
      const a = ctx.getAdapter();
      const at = a?.cursor;
      return (at && a?.candidatesAt(at.x, at.y).find((c) => c.kind === "tablecell")?.id) || null;
    },
    send: (verb) => ctx.api.cmd({ op: "table", ...verb }),
  };
}

export function registerPcbTableActions(m: Map<string, ActionHandler>, ctx: CommonActionContext): void {
  if (ctx.tab !== "pcb") return;
  registerTableEditorActions(m, boardTableEditor(ctx));
  // `DrawTable`: the tool toggles like the other drawing tools (the same action again, or Esc, puts it away).
  m.set("pcbnew.InteractiveDrawing.drawTable", () => {
    const st = ctx.api.getState();
    ctx.dispatch({ type: "SET_DRAW_STATE", draw: null });
    ctx.dispatch({ type: "SET_ACTIVE_TOOL", tool: st.activeTool === "table" ? "select" : "table" });
  });
}
