// The selection tool's events: what the actions other tools post to it do to the selection.
//
//   common/tool/selection_tool.cpp  SELECTION_TOOL::AddItemToSel / AddItemsToSel / RemoveItemFromSel / RemoveItemsFromSel /
//                                   ReselectItem (ACTIONS::selectItem, selectItems, unselectItem, unselectItems, reselectItem)
//   pcbnew/tools/pcb_selection_tool.cpp  PCB_SELECTION_TOOL::CursorSelection -> selectCursor( false, filter ) (ACTIONS::selectionCursor)
//
// KiCad passes the items as the event's parameter (an `EDA_ITEM*` or an `EDA_ITEMS*`); here a parameter is an id, a list of ids, or a
// list of anything with an `id` (a picked item).

/** The ids an event parameter names: an id, a list of ids, or a list of `{ id }` records; anything else names nothing. */
export function idsOfParameter(arg: unknown): string[] {
  if (typeof arg === "string") return [arg];
  if (!Array.isArray(arg)) return [];
  const out: string[] = [];
  for (const x of arg) {
    if (typeof x === "string") out.push(x);
    else if (x && typeof x === "object" && typeof (x as { id?: unknown }).id === "string") out.push((x as { id: string }).id);
  }
  return out;
}

/** `select( item )` for each: the ids join the selection (an item already selected stays where it is). */
export function selectIds(current: readonly string[], ids: readonly string[]): string[] {
  const out = [...current];
  for (const id of ids) if (!out.includes(id)) out.push(id);
  return out;
}

/** `unselect( item )` for each. */
export function unselectIds(current: readonly string[], ids: readonly string[]): string[] {
  return current.filter((id) => !ids.includes(id));
}

/** `ReselectItem`: `RemoveItemFromSel( item )` then `AddItemToSel( item )` -- the item ends up selected, last. */
export function reselectIds(current: readonly string[], ids: readonly string[]): string[] {
  return selectIds(unselectIds(current, ids), ids);
}

/**
 * `selectCursor( aForceSelect = false )`: "if( m_selection.Empty() || aForceSelect )" the item under the cursor is selected (as a hover
 * selection); a selection that already holds something is left alone. `candidates` are the items under the cursor, best first.
 */
export function selectCursorResult(current: readonly string[], candidates: readonly string[]): string[] {
  if (current.length > 0) return [...current];
  return candidates.length > 0 ? [candidates[0]!] : [];
}
