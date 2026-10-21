// Shared by the "E" hotkey (useActionRunner.ts's
// pcbnew.InteractiveEdit.properties) and Canvas.tsx's double-click, so
// the two can never disagree about which properties view a given item
// opens -- pcb_selection_tool.cpp's Main() double-click handler always
// runs `PCB_ACTIONS::properties`, the same action E invokes.
import type { Dispatch } from "react";
import type { Action, StudioApi } from "../../state/store";
import { openGroupDialog } from "../../state/commonDialogs";
import { openBarcodeDialog, openRefImageDialog, openTextBoxDialog } from "../../state/pcbItemDialogs";
import { barcodeToCmd } from "../../kicad-port/barcodeShapes";
import { padById } from "../../kicad-port/pcbItems";
import { fieldById } from "../../kicad-port/fpFields";
import { tablesOf } from "../../kicad-port/tables";
import { openTablePropertiesFor } from "../../actions/tableActions";

export function openPropertiesFor(id: string, api: StudioApi, dispatch: Dispatch<Action>): void {
  // `EDIT_TOOL::Properties`: table cells open Table Cell Properties (on every selected cell of the table), a table Table Properties.
  if (openTablePropertiesFor(tablesOf(api.getState().board), [...api.getState().selection], id)) return;
  // A tuning pattern (`PCB_TUNING_PATTERN::ShowPropertiesDialog`): the Tuning Pattern dialog, which tunes it again.
  if (api.getState().board?.drawings?.tuning_patterns?.some((p) => p.id === id)) {
    dispatch({ type: "PCBX", patch: { tuneDialogPattern: id } });
    dispatch({ type: "SET_LENGTH_TUNING_DIALOG_OPEN", open: true });
    return;
  }
  if (api.textById(id)) dispatch({ type: "SET_TEXT_DIALOG", dialog: { mode: "edit", id } });
  // A text box's Properties are `DIALOG_TEXTBOX_PROPERTIES` (`PCB_BASE_EDIT_FRAME::ShowTextBoxPropertiesDialog`).
  else if (api.getState().board?.drawings?.text_boxes?.some((t) => t.id === id)) {
    const box = api.getState().board!.drawings!.text_boxes!.find((t) => t.id === id)!;
    openTextBoxDialog({ id, box });
  }
  // A reference image's Properties are `DIALOG_REFERENCE_IMAGE_PROPERTIES` (`PCB_BASE_EDIT_FRAME::ShowReferenceImagePropertiesDialog`).
  else if (api.getState().board?.drawings?.images?.some((i) => i.id === id)) openRefImageDialog({ id });
  // A barcode's are `DIALOG_BARCODE_PROPERTIES` (`PCB_BASE_EDIT_FRAME::ShowBarcodePropertiesDialog`).
  else if (api.getState().board?.drawings?.barcodes?.some((b) => b.id === id)) openBarcodeDialog({ id, barcode: barcodeToCmd(api.getState().board!.drawings!.barcodes!.find((b) => b.id === id)!) });
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
    if (group) openGroupDialog({ id, name: group.name, members: [...group.member_ids], designBlock: group.design_block ?? "" });
  }
}
