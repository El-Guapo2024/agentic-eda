// Opens the Properties dialog of a schematic selection: `E` (eeschema.InteractiveEdit.properties) and a double-click on an item both come here
// (`SCH_SELECTION_TOOL::Main` posts `SCH_ACTIONS::properties` on a double-click, `SCH_EDIT_TOOL::Properties` picks the dialog).
import type { Dispatch } from "react";
import type { Schematic } from "../../api/types";
import { propertiesTarget } from "../../kicad-port/schProperties";
import type { Action } from "../../state/store";

/** Opens the dialog the selection `ids` calls for; false when it calls for none. */
export function openSchProperties(sch: Schematic, ids: readonly string[], dispatch: Dispatch<Action>): boolean {
  const target = propertiesTarget(sch, ids);
  if (!target) return false;
  switch (target.kind) {
    case "symbol":
      dispatch({ type: "SET_SYMBOL_PROPERTIES", value: { id: target.id, field: null } });
      return true;
    case "label":
      dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "props_label", id: target.id } });
      return true;
    case "text":
      dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "props_text", id: target.id } });
      return true;
    case "sheet":
      dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "props_sheet", id: target.id } });
      return true;
    case "graphic":
      dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "props_graphic", id: target.id } });
      return true;
    case "stroke":
      dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "props_stroke", ids: target.ids } });
      return true;
  }
}
