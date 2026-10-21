// The board editor's table actions: the shared table tool's (tableActions.ts) wired to the board's tables and selection, and Draw Table.
//
//   pcbnew/tools/drawing_tool.cpp          DRAWING_TOOL::DrawTable      pcbnew.InteractiveDrawing.drawTable: arms the tool (Canvas.tsx has its two clicks)
//   pcbnew/tools/pcb_edit_table_tool.cpp   PCB_EDIT_TABLE_TOOL          the structure edits, Edit Table, Export Table to CSV
//   pcbnew/tools/pcb_selection_tool.cpp    SelectColumns / SelectRows / SelectTable
//   pcbnew/tools/pcb_control.cpp           PlaceStackup, PlaceCharacteristics    the tables Place > Add Stackup Table and Add Board Characteristics build from the board
import type { ActionHandler, CommonActionContext } from "./commonActions";
import { registerTableEditorActions, type TableEditor } from "./tableActions";
import { postBoardStats } from "../api/client";
import { activeLayerName } from "../kicad-port/layerPresets";
import { boardNumbersOf, tablesOf } from "../kicad-port/tables";

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
  // `PlaceStackup`: the table of Board Setup's stackup is built on the active layer (`table->SetLayer( m_frame->GetActiveLayer() )`) and carried by the cursor until a click
  // drops it (`placeBoardItems( .., aAnchorAtOrigin )`: the table's first corner is the cursor), one undo step.
  m.set("pcbnew.InteractiveDrawing.placeStackup", () => {
    const st = ctx.api.getState();
    const layer = activeLayerName(st.activeLayer, "F.SilkS");
    void ctx.api.placeNewItems((at) => ({ op: "table", verb: "place_stackup", layer, at, units: st.units }), true);
  });
  // `PlaceCharacteristics`: the same with the board's numbers, which kicad-cli measures (`ComputeBoardStatistics` is the Board Statistics dialog's; nothing here does).
  m.set("pcbnew.InteractiveDrawing.placeCharacteristics", async () => {
    const st = ctx.api.getState();
    const layer = activeLayerName(st.activeLayer, "F.SilkS");
    const stats = await postBoardStats({ exclude_footprints_without_pads: false, subtract_holes_from_board_area: false, subtract_holes_from_copper_areas: false });
    const numbers = boardNumbersOf(stats);
    if (!numbers) {
      ctx.dispatch({ type: "TOAST", message: stats.message ?? "kicad-cli gave no board statistics.", kind: "error" });
      return;
    }
    void ctx.api.placeNewItems((at) => ({ op: "table", verb: "place_characteristics", layer, at, units: st.units, numbers }), true);
  });
  // `DrawTable`: the tool toggles like the other drawing tools (the same action again, or Esc, puts it away).
  m.set("pcbnew.InteractiveDrawing.drawTable", () => {
    const st = ctx.api.getState();
    ctx.dispatch({ type: "SET_DRAW_STATE", draw: null });
    ctx.dispatch({ type: "SET_ACTIVE_TOOL", tool: st.activeTool === "table" ? "select" : "table" });
  });
}
