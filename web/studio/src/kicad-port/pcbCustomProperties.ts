// The Properties panel's "Custom Properties" (KiCad 25ec2335): every board item has a map of names to texts (`EDA_ITEM::m_customProperties`, common/eda_item.cpp), shown as
// rows of their own group (`EDA_CUSTOM_PROPERTY`, `SetGroup( "Custom Properties" )`) after the item's other properties (`GetDynamicProperties`), with the group always
// there for a selection because its button adds one (common/widgets/properties_panel.cpp `rebuildProperties`: "Always show the Custom Properties group, even when it has
// no members, because it has the add button"). pcbnew/widgets/pcb_properties_panel.cpp:
//
//   addBlankCustomProperty      "Add Custom Property": `Property%d`, the first number no selected item uses, with an empty text on each item; then its name is edited
//   onPropertyLabelEdited /     a name edited: kept when it is not empty and no property of the front item has it (`isKeyNameInUse`), else put back -- for a new one,
//   onNewItemLeftBlank          the new property removed (`removeCustomProperty`)
//   onKeyRenamed                "Rename Property": the items that have the old name take the new one, their text kept
//   removeCustomProperty        "Remove Custom Property", from the context menu of a row
//
// A row is the one name every selected item has (`GetCommonDynamicProperties`), its text the one they share or `<...>`. Each change is one `set_custom_property` verb, one
// undo step. The design keeps the properties by item id (`DrawingsSection::custom_properties`, the state's `custom_properties`). Pure: no React, no DOM.
import type { BoardState, Cmd } from "../api/types";
import type { GridGroup, GridRow } from "./propertyGrid";

/** The group's name and caption (`_HKI( "Custom Properties" )`). */
export const CUSTOM_PROPERTIES = "Custom Properties";

/** The item kinds whose custom properties the design keeps: everything the board lists by an id of its own (a footprint's field and its own texts and a table's cells
 *  are not, here). */
export function holdsCustomProperties(board: BoardState, type: string, id: string): boolean {
  if (type === "PCB_FIELD" || type === "PCB_TABLECELL") return false;
  if (type === "PCB_TEXT") return (board.drawings?.texts ?? []).some((t) => t.id === id);
  return true;
}

const propsOf = (board: BoardState, id: string): Readonly<Record<string, string>> => board.custom_properties?.[id] ?? {};

/** The "Custom Properties" group of the selection: the names every item has, in name order (`std::map`), each with the text they share or null; `null` when an item
 *  of the selection keeps none (a field, a table cell). */
export function customPropertyGroup(board: BoardState, items: readonly { type: string; id: string }[]): GridGroup | null {
  if (items.length === 0 || !items.every((i) => holdsCustomProperties(board, i.type, i.id))) return null;
  const front = propsOf(board, items[0]!.id);
  const names = Object.keys(front)
    .filter((name) => items.every((i) => name in propsOf(board, i.id)))
    .sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
  const rows: GridRow[] = names.map((name) => {
    const texts = new Set(items.map((i) => propsOf(board, i.id)[name]));
    return { name, kind: "string", display: "default", value: texts.size === 1 ? front[name]! : null, writable: true, choices: null };
  });
  return { name: CUSTOM_PROPERTIES, caption: CUSTOM_PROPERTIES, rows, custom: true };
}

/** `addBlankCustomProperty`'s name: `Property%d`, the first `n` none of the selected items has. */
export function newCustomPropertyName(board: BoardState, ids: readonly string[]): string {
  for (let n = 0; ; n++) {
    const name = `Property${n}`;
    if (!ids.some((id) => name in propsOf(board, id))) return name;
  }
}

/** `isKeyNameInUse`: a property of the front item has the name -- a row of the grid (its class's properties, its fields) or one of its custom properties. */
export function keyNameInUse(board: BoardState, frontId: string, gridRowNames: readonly string[], name: string): boolean {
  return gridRowNames.includes(name) || name in propsOf(board, frontId);
}

/** The text of a row typed in: `EDA_CUSTOM_PROPERTY::setter` on every item. */
export function setCustomPropertyCmd(ids: readonly string[], name: string, text: string): Cmd {
  return { op: "set_custom_property", ids: [...ids], name, value: text };
}

/** "Add Custom Property": the name with an empty text on every item. */
export function addCustomPropertyCmd(ids: readonly string[], name: string): Cmd {
  return { op: "set_custom_property", ids: [...ids], name, value: "" };
}

/** "Remove Custom Property". */
export function removeCustomPropertyCmd(ids: readonly string[], name: string): Cmd {
  return { op: "set_custom_property", ids: [...ids], name };
}

/** "Rename Property" (`onKeyRenamed`): the items that have `from` take `to`, their text kept. */
export function renameCustomPropertyCmd(ids: readonly string[], from: string, to: string): Cmd {
  return { op: "set_custom_property", ids: [...ids], name: from, rename_to: to };
}

/**
 * What a name edited in the label of a row comes to (`PROPERTIES_PANEL::onPropertyLabelEdited`): for the property just added (`pending`), a name that is empty or in use
 * removes it (`onNewItemLeftBlank`), another renames it; for an existing one, an empty or used name puts the label back (nothing sent), another renames it.
 */
export function labelEdited(board: BoardState, ids: readonly string[], gridRowNames: readonly string[], oldName: string, newName: string, pending: boolean): Cmd | null {
  const name = newName.trim() === "" ? "" : newName;
  // (the property's own name is in use too: the new one typed back as it was is removed, an existing one keeps its label)
  const refused = name === "" || keyNameInUse(board, ids[0] ?? "", gridRowNames, name);
  if (pending) return refused ? removeCustomPropertyCmd(ids, oldName) : renameCustomPropertyCmd(ids, oldName, name);
  return refused ? null : renameCustomPropertyCmd(ids, oldName, name);
}
