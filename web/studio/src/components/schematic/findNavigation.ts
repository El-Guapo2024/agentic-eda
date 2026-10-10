// Find Next / Find Previous for the schematic -- SCH_FIND_REPLACE_TOOL::FindNext
// (eeschema/tools/sch_find_replace_tool.cpp). Shared by FindReplaceDialog.tsx
// and the F3 / Shift+F3 actions (useActionRunner.ts) so both cycle the same
// cursor (`m_afterItem`, here `state.schFind.cursor`).
//
// What FindNext does with a hit, and what this does:
//   * `ClearSelection(); AddItemToSel( item )`  -> SET_SELECTION [owner id]
//     (a symbol field / pin hit selects its symbol, a label/text hit
//     selects that item -- the same ids `state.selection` holds elsewhere).
//   * `BrightenItem` + `SetForceVisible`         -> SET_HOT (the app's
//     existing "highlighted" channel, same one ErcDialog's jumpTo uses).
//   * `FocusOnLocation( item->GetBoundingBox().GetCenter() )` -> re-center
//     the schematic view on the hit, keeping the current zoom (frame it
//     tight only when the view was never initialized) -- same container
//     lookup ErcDialog's jumpTo uses.
//   * the item is on another sheet (the search went over every sheet, `searchAllSheets`) -> `RunAction( changeSheet, &sheet )` first, then the
//     selection and the view as above.
//   * no item left -> "Reached end of schematic. Find again to wrap around
//     to the start." and the cursor resets so the next call wraps.
import type { Dispatch } from "react";
import { fetchSchFind } from "../../api/client";
import type { FindMatch } from "../../api/types";
import { fitTransform } from "../canvas/view";
import { endReachedMessage, pickMatch, searchesAllSheets } from "../../kicad-port/schFind";
import { samePath } from "../../kicad-port/sheetPages";
import type { Action, StudioApi } from "../../state/store";

/** Go to the sheet of a match and select + pan to it (the `FindNext` success path). */
export async function visitMatch(api: StudioApi, dispatch: Dispatch<Action>, match: FindMatch): Promise<void> {
  // `editFrame->GetToolManager()->RunAction<SCH_SHEET_PATH*>( SCH_ACTIONS::changeSheet, &sheet )`
  if (match.sheet && !samePath(match.sheet, api.getState().currentSheetPath)) await api.navigateToSheet(match.sheet);
  dispatch({ type: "SET_SELECTION", refs: [match.id] });
  dispatch({ type: "SET_HOT", refs: [match.id] });
  const container = document.querySelector(".pcb-canvas-container");
  const rect = container?.getBoundingClientRect();
  if (rect && rect.width >= 50 && rect.height >= 50) {
    const [x, y] = match.at;
    const view = api.getState().schematicView;
    if (view.scale > 0) {
      dispatch({ type: "SET_SCHEMATIC_VIEW", view: { scale: view.scale, x: rect.width / 2 - x * view.scale, y: rect.height / 2 - y * view.scale } });
    } else {
      const padUm = 2_000;
      dispatch({ type: "SET_SCHEMATIC_VIEW", view: fitTransform({ minX: x - padUm, minY: y - padUm, maxX: x + padUm, maxY: y + padUm }, rect.width, rect.height, 60) });
    }
  }
}

/**
 * `FindNext` / `FindPrevious` (`reversed`). With no search text it opens the
 * Find dialog instead (`return FindAndReplace( ACTIONS::find.MakeEvent() )`).
 * `cursorOverride` lets Replace continue from a cursor it just fixed up
 * (see FindReplaceDialog's `replaceCurrent`) instead of the stale one in
 * `state`. Over every sheet unless "Search the current sheet only" is on (or the search is of the selection); a Find with no cursor starts on
 * the sheet in view, and the sheets before it only come after the wrap. Returns the match visited, or null.
 */
export async function findNextMatch(api: StudioApi, dispatch: Dispatch<Action>, reversed: boolean, cursorOverride?: string | null): Promise<FindMatch | null> {
  const state = api.getState();
  const { search } = state.schFind;
  const cursor = cursorOverride === undefined ? state.schFind.cursor : cursorOverride;
  if (search.find === "") {
    dispatch({ type: "SET_SCH_DIALOG", dialog: "find" });
    return null;
  }
  const allSheets = searchesAllSheets(state.schFind.currentSheetOnly, state.schFind.selectedOnly);
  const reply = await fetchSchFind({ ...search, search_and_replace: false }, undefined, state.currentSheetPath, allSheets);
  if (!reply.ok) {
    dispatch({ type: "SET_SCH_FIND", find: { status: reply.message ?? "Find failed." } });
    return null;
  }
  const picked = pickMatch(reply.matches, cursor, reversed, allSheets ? (reply.current_rank ?? 0) : undefined);
  if (picked.match) {
    dispatch({ type: "SET_SCH_FIND", find: { cursor: picked.cursor, status: `${reply.matches.length} match${reply.matches.length === 1 ? "" : "es"}` } });
    await visitMatch(api, dispatch, picked.match);
    return picked.match;
  }
  const message = reply.matches.length === 0 ? "No match found." : endReachedMessage(allSheets);
  dispatch({ type: "SET_SCH_FIND", find: { cursor: picked.cursor, status: message } });
  dispatch({ type: "TOAST", message, kind: "info" });
  return null;
}
