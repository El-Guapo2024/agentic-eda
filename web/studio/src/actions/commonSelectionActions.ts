// The selection tool's actions, the interactive delete tool and the picker, for every editor (called from `registerCommonActions`).
//
//   common/tool/common_tools.cpp         COMMON_TOOLS::SelectionTool                         ACTIONS::selectionTool
//   common/tool/selection_tool.cpp       SELECTION_TOOL::AddItemToSel / AddItemsToSel / RemoveItemFromSel / RemoveItemsFromSel /
//                                        ReselectItem / SelectionMenu                        selectItem(s), unselectItem(s), reselectItem, selectionMenu
//   pcbnew/tools/pcb_selection_tool.cpp  PCB_SELECTION_TOOL::Main / CursorSelection / ClearSelection / SetSelectPoly / SetSelectRect
//                                                                                            selectionActivate, selectionCursor, selectionClear, selectSetLasso/Rect
//   pcbnew/tools/pcb_control.cpp         PCB_CONTROL::InteractiveDelete                      ACTIONS::deleteTool (eeschema: SCH_TOOL_BASE::InteractiveDelete)
//   common/tool/picker_tool.cpp          PICKER_TOOL::Main                                   ACTIONS::pickerTool / pickerSubTool
//
// The table actions (`selectRows`, `selectColumns`, `selectTable`) are not here: there are no tables to select cells of.
import type { ActionHandler, CommonActionContext } from "./commonActions";
import { isCanvasTab, type PickedItem } from "./editorAdapter";
import { picker } from "./pcbPicker";
import type { PickerSession } from "../kicad-port/pickerHost";
import { cancelLasso, getCommonTool, getLastPointer, setPickerHover, showSelectionMenu } from "../state/commonTool";
import { setCommonOptions } from "../state/commonOptions";
import { selectionModeForAction } from "../kicad-port/lasso";
import { idsOfParameter, reselectIds, selectCursorResult, selectIds, unselectIds } from "../kicad-port/selectionEvents";

function isPickerSession(arg: unknown): arg is PickerSession {
  const s = arg as Partial<PickerSession> | null;
  return !!s && typeof s === "object" && (s.kind === "point" || s.kind === "item") && typeof s.prompt === "string";
}

export function registerSelectionActions(m: Map<string, ActionHandler>, ctx: CommonActionContext): void {
  if (!isCanvasTab(ctx.tab)) return; // the 3D viewer has no selection tool
  const { dispatch, fpDispatch, symDispatch } = ctx;
  const fresh = ctx.getAdapter;
  const toast = (message: string, kind: "error" | "info" = "info") => dispatch({ type: "TOAST", message, kind });

  /**
   * `TA_CANCEL_TOOL`: whatever tool other than the selection tool is running ends -- a picker session, a lasso being drawn, or the
   * editor's own draw / move / route / placement tool -- and the selection stays as it is (Escape's first tier, not its second).
   */
  const cancelActiveTool = (): void => {
    if (picker.cancel() || cancelLasso()) return;
    const a = fresh();
    if (!a || a.toolIdle) return;
    if (a.tab === "footprint") fpDispatch({ type: "ESCAPE" });
    else if (a.tab === "symbol") symDispatch({ type: "ESCAPE" });
    else m.get("common.Interactive.cancel")?.();
  };

  // ACTIONS::selectionTool ("Select item(s)", the arrow of the toolbars) -- COMMON_TOOLS::SelectionTool: "Since selection tools are run
  // permanently underneath the toolStack, this is really just a cancel of whatever other tools might be running."
  m.set("common.InteractiveSelection.selectionTool", cancelActiveTool);
  // ACTIONS::selectionActivate -- SELECTION_TOOL::Main: the selection tool is (again) the one running, which is what ending every other tool means here.
  m.set("common.InteractiveSelection", cancelActiveTool);

  // ACTIONS::selectionClear -- PCB_SELECTION_TOOL::ClearSelection / SCH_SELECTION_TOOL::ClearSelection.
  m.set("common.InteractiveSelection.clear", () => fresh()?.setSelection([]));

  // ACTIONS::selectionCursor -- PCB_SELECTION_TOOL::CursorSelection -> selectCursor( false ): with nothing selected, the item under the
  // cursor is selected (the "hover" selection every edit hotkey relies on); a selection that already holds something is left alone.
  m.set("common.InteractiveSelection.cursor", () => {
    const a = fresh();
    if (!a || !a.cursor) return;
    const next = selectCursorResult([...a.selection], a.candidatesAt(a.cursor.x, a.cursor.y).map((c) => c.id));
    if (next.length !== a.selection.size) a.setSelection(next);
  });

  // ACTIONS::selectItem / selectItems / unselectItem / unselectItems / reselectItem -- the events other tools post with an item or a list of
  // them (`aEvent.Parameter<EDA_ITEM*>()` / `<EDA_ITEMS*>()`): the ids are the event parameter, `run( name, ids )`.
  m.set("common.InteractiveSelection.selectItem", (arg) => {
    const a = fresh();
    if (a) a.setSelection(selectIds([...a.selection], idsOfParameter(arg)));
  });
  m.set("common.InteractiveSelection.selectItems", (arg) => {
    const a = fresh();
    if (a) a.setSelection(selectIds([...a.selection], idsOfParameter(arg)));
  });
  m.set("common.InteractiveSelection.unselectItem", (arg) => {
    const a = fresh();
    if (a) a.setSelection(unselectIds([...a.selection], idsOfParameter(arg)));
  });
  m.set("common.InteractiveSelection.unselectItems", (arg) => {
    const a = fresh();
    if (a) a.setSelection(unselectIds([...a.selection], idsOfParameter(arg)));
  });
  m.set("common.InteractiveSelection.reselectItem", (arg) => {
    const a = fresh();
    if (a) a.setSelection(reselectIds([...a.selection], idsOfParameter(arg)));
  });

  // ACTIONS::selectionMenu -- SELECTION_TOOL::SelectionMenu -> doSelectionMenu: the clarification menu for items a click could mean,
  // "1  ...", "2  ...", then "Select All"; the pick (or all of them) becomes the selection, dismissing it selects nothing.
  m.set("common.InteractiveSelection.selectionMenu", (arg) => {
    const a = fresh();
    const ids = idsOfParameter(arg);
    if (!a || ids.length === 0) return;
    const items = ids.map((id) => ({ id, label: a.describe({ id, kind: id } as PickedItem) || id }));
    showSelectionMenu({
      at: getLastPointer(),
      items,
      onChoose: (chosen) => {
        if (chosen) fresh()?.setSelection(chosen);
      },
    });
  });

  // ACTIONS::selectSetRect / selectSetLasso -- PCB_SELECTION_TOOL::SetSelectRect / SetSelectPoly (and the schematic's): `m_selectionMode =
  // INSIDE_RECTANGLE / INSIDE_LASSO`, then `PostAction( ACTIONS::selectionTool )`. components/CommonToolHost.tsx draws the lasso.
  m.set("common.Interactive.selectSetRect", () => {
    setCommonOptions({ selectionMode: selectionModeForAction("selectSetRect") });
    cancelActiveTool();
  });
  m.set("common.Interactive.selectSetLasso", () => {
    setCommonOptions({ selectionMode: selectionModeForAction("selectSetLasso") });
    cancelActiveTool();
  });

  // ACTIONS::deleteTool ("Interactive Delete Tool", AF_ACTIVATE) -- PCB_CONTROL::InteractiveDelete / SCH_TOOL_BASE::InteractiveDelete:
  // `selectionClear`, then the picker with the REMOVE cursor and no snapping; its motion handler highlights the one item under the
  // pointer (nothing when more than one is left after `GuessSelectionCandidates`), a click deletes that item and the tool goes on until
  // Escape. A locked item cannot be deleted: "Item locked.".
  m.set("common.Interactive.deleteTool", () => {
    const a = fresh();
    if (!a) return;
    // `if( m_isFootprintEditor && !GetFirstFootprint() ) return 0;` -- and the same for an editor with nothing open
    if (a.tab === "pcb" && !ctx.api.getState().board) return;
    if (a.tab === "schematic" && !ctx.api.getState().schematic) return;
    if (a.tab === "footprint" && !ctx.fpApi.getState().footprint) return;
    if (a.tab === "symbol" && !ctx.symApi.getState().symbol) return;
    a.setSelection([]);
    setPickerHover(null);
    picker.start({
      kind: "point",
      prompt: "Delete: click an item to delete it (Esc to stop)",
      cursor: "remove",
      hover: (pt) => {
        const c = fresh()?.candidatesAt(pt.x, pt.y) ?? [];
        return c.length === 1 ? c[0]!.id : null;
      },
      onPoint: () => {
        // `if( m_pickerItem )`: the item the motion handler highlighted
        const id = getCommonTool().hover;
        if (!id) return true;
        setPickerHover(null);
        void fresh()
          ?.deleteIds([id])
          .then((refusal) => {
            if (refusal) toast(refusal, "error");
          });
        return true; // `getNext`: keep deleting
      },
      onFinalize: () => setPickerHover(null),
    });
  });

  // ACTIONS::pickerTool / pickerSubTool -- PICKER_TOOL::Main, started by a tool that has set its click, motion, cancel and finalize handlers
  // first; here the handlers are the event parameter, a `PickerSession` (kicad-port/pickerHost.ts). `pickerTool` is the tool that
  // takes over (it pushes itself on the tool stack, so whatever else was running ends); `pickerSubTool` runs inside another tool -- Move
  // asking for a reference point -- which carries on afterwards. The studio's picker prompt shows either way. The board canvas answers the
  // session; for the other editors the pointer is components/CommonToolHost.tsx's.
  m.set("common.InteractivePicker.pickerTool", (arg) => {
    if (!isPickerSession(arg)) return;
    cancelLasso();
    picker.start(arg);
  });
  m.set("common.InteractivePicker.pickerSubTool", (arg) => {
    if (isPickerSession(arg)) picker.start(arg);
  });
}
