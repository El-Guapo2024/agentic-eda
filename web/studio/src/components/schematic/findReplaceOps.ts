// Replace and Replace All, and the highlight of the matches, for the schematic's Find and Replace -- the actions the dialog's buttons run
// (`ACTIONS::replaceAndFindNext`, `replaceAll`, `updateFind`): eeschema/tools/sch_find_replace_tool.cpp SCH_FIND_REPLACE_TOOL::ReplaceAndFindNext /
// ReplaceAll / UpdateFind, driven by eeschema/dialogs/dialog_sch_find.cpp (`OnReplace`, `OnReplaceAll`, `OnSearchForText`). The matching and the
// replacing are the server's (`POST /api/sch/find`, `replace_text`); the state they read -- the search, the cursor, "selected objects only",
// the direction -- is `state.schFind`, so they work with the dialog closed, like the actions do in KiCad.
import type { Dispatch } from "react";
import { fetchSchFind } from "../../api/client";
import type { Action, StudioApi } from "../../state/store";
import { currentMatch, matchId, scopeFromSelection, searchesAllSheets } from "../../kicad-port/schFind";
import { samePath } from "../../kicad-port/sheetPages";
import { findNextMatch } from "./findNavigation";
import { setFindHighlights } from "../../state/commonTool";

/**
 * `ReplaceAndFindNext`: replace the current match (`getCurrentMatch`: the selected item when it matches, or with "Search the current selection
 * only" the match the Find cursor is on), then Find Next. With no current match it does nothing (`if( item && HasMatch() )`). The cursor
 * continues after where the replaced item was: its own key when it still matches (so it is skipped, as `m_afterItem` is), else the match
 * before it. The match is on the sheet in view; the replace is addressed to that sheet.
 */
export async function replaceAndFindNext(api: StudioApi, dispatch: Dispatch<Action>): Promise<void> {
  const state = api.getState();
  const { search, cursor, backward, currentSheetOnly, selectedOnly } = state.schFind;
  if (search.find === "") {
    dispatch({ type: "SET_SCH_DIALOG", dialog: "find" });
    return;
  }
  const allSheets = searchesAllSheets(currentSheetOnly, selectedOnly);
  const scope = scopeFromSelection(state.selection, selectedOnly);
  const before = await fetchSchFind({ ...search, search_and_replace: true }, scope, state.currentSheetPath, allSheets);
  // the selected item is on the sheet in view; so is the cursor's match (selected-only searches the sheet in view)
  const here = before.matches.filter((m) => !m.sheet || samePath(m.sheet, state.currentSheetPath));
  const current = currentMatch(here, cursor, [...state.selection], selectedOnly);
  if (!current) return;
  const at = before.matches.findIndex((m) => matchId(m) === matchId(current));
  const prevId = at > 0 ? matchId(before.matches[at - 1]!) : null;
  const ok = await api.cmd({ op: "replace_text", search, items: [current.key] });
  if (!ok) return;
  const now = api.getState();
  const after = await fetchSchFind({ ...search, search_and_replace: false }, scopeFromSelection(now.selection, selectedOnly), now.currentSheetPath, allSheets);
  // `m_afterItem` is where the last Find was; when the replaced item was it and no longer matches, go on from the match before it
  const from = cursor ?? matchId(current);
  const next = after.matches.some((m) => matchId(m) === from) ? from : after.matches.some((m) => matchId(m) === prevId) ? prevId : null;
  await findNextMatch(api, dispatch, backward, next);
}

/**
 * `ReplaceAll`: every match -- or, with "selected objects only", those of the selection -- in one undoable step. Over every sheet unless
 * "Search the current sheet only" is on. Nothing to replace pushes nothing and says nothing (`if( !commit.Empty() )`).
 */
export async function replaceAll(api: StudioApi, dispatch: Dispatch<Action>): Promise<void> {
  const state = api.getState();
  const { search, selectedOnly, currentSheetOnly } = state.schFind;
  if (search.find === "") return;
  const allSheets = searchesAllSheets(currentSheetOnly, selectedOnly);
  const scope = scopeFromSelection(state.selection, selectedOnly);
  const found = await fetchSchFind({ ...search, search_and_replace: true }, scope, state.currentSheetPath, allSheets);
  if (!found.ok || found.matches.length === 0) return;
  const items = scope ? found.matches.map((m) => m.key) : null;
  const ok = await api.cmd({ op: "replace_text", search, items, all_sheets: allSheets });
  if (ok) dispatch({ type: "SET_SCH_FIND", find: { cursor: null, status: "" } });
}

/**
 * `UpdateFind`: while the Find dialog is open every match of the search text is brightened (and shown, `SetForceVisible`), those that no longer
 * match are put back -- also when the dialog closes, which clears them. The studio's overlay draws the brightened items' boxes.
 */
export async function updateFind(api: StudioApi): Promise<void> {
  const state = api.getState();
  const { search, selectedOnly } = state.schFind;
  const open = state.schDialog === "find" || state.schDialog === "replace";
  if (!open || search.find === "") {
    setFindHighlights([]);
    return;
  }
  // `searchAndReplace` is never set by KiCad's dialog (only read from and written to the config, default false), so the replace dialog
  // brightens References and pins as Find does
  const reply = await fetchSchFind({ ...search, search_and_replace: false }, scopeFromSelection(state.selection, selectedOnly), state.currentSheetPath);
  setFindHighlights(reply.ok ? [...new Set(reply.matches.map((m) => m.id))] : []);
}
