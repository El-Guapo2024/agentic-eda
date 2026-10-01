// Shared by the "E" hotkey (useActionRunner.ts's
// pcbnew.InteractiveEdit.properties) and Canvas.tsx's double-click, so
// the two can never disagree about which properties view a given item
// opens -- pcb_selection_tool.cpp's Main() double-click handler always
// runs `PCB_ACTIONS::properties`, the same action E invokes.
import type { Dispatch } from "react";
import type { Action, StudioApi } from "../../state/store";

export function openPropertiesFor(id: string, api: StudioApi, dispatch: Dispatch<Action>): void {
  if (api.textById(id)) dispatch({ type: "SET_TEXT_DIALOG", dialog: { mode: "edit", id } });
  else if (api.trackById(id) || api.viaById(id) || api.zoneById(id) || api.shapeById(id)) dispatch({ type: "SET_ITEM_PROPERTIES_ID", id });
  else if (api.partByRef(id)) dispatch({ type: "SET_FOOTPRINT_PROPERTIES_OPEN", open: true });
}
