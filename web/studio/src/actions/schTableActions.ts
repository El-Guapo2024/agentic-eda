// The sheet's table actions: the shared table tool's (tableActions.ts -- Add Row Above / Below, Add Column Before / After, Delete Row(s) / Column(s), Merge / Unmerge Cells, Edit Table,
// Export Table to CSV, Select Column(s) / Row(s) / Table) wired to the sheet's tables, selection and verbs. KiCad's `SCH_EDIT_TABLE_TOOL` and `PCB_EDIT_TABLE_TOOL` are one class of
// `EDIT_TABLE_TOOL_BASE`: here too the editors share the one set of handlers, and only the editor they are given differs. Draw Table (`SCH_DRAWING_TOOLS::DrawTable`) is armed from
// `schEditActions.ts` with the sheet's other drawing tools; the two clicks are SchematicView.tsx's.
import type { ActionHandler, CommonActionContext } from "./commonActions";
import { registerTableEditorActions, type TableEditor } from "./tableActions";

/** The sheet's tables, selection and verbs as the table actions use them. */
export function sheetTableEditor(ctx: CommonActionContext): TableEditor {
  return {
    scope: "sch",
    tables: () => ctx.api.getState().schematic?.tables ?? [],
    selection: () => [...ctx.api.getState().selection],
    setSelection: (ids) => ctx.dispatch({ type: "SET_SELECTION", refs: ids }),
    hoverCellId: () => {
      const a = ctx.getAdapter();
      const at = a?.cursor;
      return (at && a?.candidatesAt(at.x, at.y).find((c) => c.kind === "tablecell")?.id) || null;
    },
    send: (verb) => ctx.api.cmd({ op: "sch_table", ...verb }),
  };
}

export function registerSchTableActions(m: Map<string, ActionHandler>, ctx: CommonActionContext): void {
  if (ctx.tab !== "schematic") return;
  registerTableEditorActions(m, sheetTableEditor(ctx));
}
