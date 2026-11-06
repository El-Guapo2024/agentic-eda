// Opens the Properties dialog of a schematic selection: `E` (eeschema.InteractiveEdit.properties) and a double-click on an item both come here
// (`SCH_SELECTION_TOOL::Main` posts `SCH_ACTIONS::properties` on a double-click, `SCH_EDIT_TOOL::Properties` picks the dialog).
import type { Dispatch } from "react";
import type { Schematic } from "../../api/types";
import { propertiesTarget } from "../../kicad-port/schProperties";
import { findField, isFieldId } from "../../kicad-port/schFieldEdit";
import type { Action } from "../../state/store";
import { groupsOf } from "../../kicad-port/schGroups";
import { openGroupDialog } from "../../state/commonDialogs";
import { openTablePropertiesFor } from "../../actions/tableActions";
import { symbolBounds } from "./libSymbol";

/** Opens the dialog the selection `ids` calls for; false when it calls for none. `at` (the cursor) names the placed unit of a reference whose units are
 *  one item of the selection: Symbol Properties opens on the unit under it (KiCad's dialog is the one `SCH_SYMBOL`'s), else on the first placed one. */
export function openSchProperties(sch: Schematic, ids: readonly string[], dispatch: Dispatch<Action>, at?: readonly [number, number] | null): boolean {
  // a group has the dialog of the group tool (`SCH_EDIT_TOOL::Properties`: `case SCH_GROUP_T: RunAction( ACTIONS::groupProperties, group )`)
  const group = ids.length === 1 ? groupsOf(sch).find((g) => g.id === ids[0]) : undefined;
  if (group) {
    openGroupDialog({ id: group.id, name: group.name, members: [...group.member_ids], designBlock: group.design_block ?? "" });
    return true;
  }
  // a table has Table Properties, table cells Table Cell Properties (`SCH_EDIT_TOOL::Properties`: `case SCH_TABLE_T: EditTable`; `case SCH_TABLECELL_T`)
  const tables = sch.tables ?? [];
  if (tables.length > 0 && ids.length > 0 && openTablePropertiesFor(tables, ids, ids[0]!, "sch")) return true;
  // a field picked on its own has the dialog of the field (`SCH_EDIT_TOOL::Properties`: `case SCH_FIELD_T: EditProperties`)
  if (ids.length === 1 && isFieldId(ids[0]!) && findField(sch, ids[0]!)) {
    dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "props_field", id: ids[0]! } });
    return true;
  }
  const target = propertiesTarget(sch, ids);
  if (!target) return false;
  switch (target.kind) {
    case "symbol": {
      const under = at ? sch.symbols.filter((x) => x.id === target.id).find((x) => { const b = symbolBounds(x, sch.lib_symbols); return at[0] >= b.minX && at[0] <= b.maxX && at[1] >= b.minY && at[1] <= b.maxY; }) : undefined;
      dispatch({ type: "SET_SYMBOL_PROPERTIES", value: { id: target.id, field: null, ...(under ? { unit: under.unit } : {}) } });
      return true;
    }
    case "power":
      dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "props_power", id: target.id } });
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
    case "sheet_pin":
      dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "props_sheet_pin", id: target.id } });
      return true;
    case "graphic":
      dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "props_graphic", id: target.id } });
      return true;
    case "stroke":
      dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "props_stroke", ids: target.ids } });
      return true;
  }
}
