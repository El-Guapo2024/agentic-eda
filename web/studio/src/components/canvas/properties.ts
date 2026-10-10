// Shared by the "E" hotkey (useActionRunner.ts's
// pcbnew.InteractiveEdit.properties) and Canvas.tsx's double-click, so
// the two can never disagree about which properties view a given item
// opens -- pcb_selection_tool.cpp's Main() double-click handler always
// runs `PCB_ACTIONS::properties`, the same action E invokes.
import type { Dispatch } from "react";
import type { Action, StudioApi } from "../../state/store";
import { openGroupDialog } from "../../state/commonDialogs";
import { padById } from "../../kicad-port/pcbItems";
import { fieldById } from "../../kicad-port/fpFields";

export function openPropertiesFor(id: string, api: StudioApi, dispatch: Dispatch<Action>): void {
  if (api.textById(id)) dispatch({ type: "SET_TEXT_DIALOG", dialog: { mode: "edit", id } });
  // A zone gets the full dialog_copper_zones.cpp port (ZoneDialog.tsx),
  // same as "Add Zone" -- not the plain read-only-ish ItemPropertiesDialog
  // every other item here still uses.
  else if (api.zoneById(id)) dispatch({ type: "SET_ZONE_EDIT_ID", id });
  // Task item 7: same reasoning as the zone branch above -- a dimension's
  // field list (kind-specific geometry plus a dozen formatting fields)
  // gets its own dedicated dialog, not the plain ItemPropertiesDialog.
  else if (api.dimensionById(id)) dispatch({ type: "SET_DIMENSION_EDIT_ID", id });
  else if (api.trackById(id) || api.viaById(id) || api.shapeById(id)) dispatch({ type: "SET_ITEM_PROPERTIES_ID", id });
  else if (api.partByRef(id)) dispatch({ type: "SET_FOOTPRINT_PROPERTIES_OPEN", open: true });
  // `PCB_EDIT_FRAME::OnEditItemRequest`: a pad gets Pad Properties (`ShowPadPropertiesDialog`) ...
  else if (api.getState().board && padById(api.getState().board!, id)) dispatch({ type: "SET_BOARD_PAD_PROPERTIES_ID", id });
  // ... and a footprint's field the text properties of that field (`ShowTextPropertiesDialog`), here the Footprint Properties dialog on its footprint,
  // whose grid has the field's every setting.
  else if (api.getState().board && fieldById(api.getState().board!, id)) dispatch({ type: "SET_FOOTPRINT_PROPERTIES_OPEN", open: true });
  // `PCB_GROUP` properties are the group tool's (`ACTIONS::groupProperties`, common.Groups.groupProperties): the name and the member list.
  else {
    const group = api.groupById(id);
    if (group) openGroupDialog({ id, name: group.name, members: [...group.member_ids] });
  }
}
