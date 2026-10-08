// Replace and Replace All, and the highlight of the matches, for the schematic's Find and Replace -- the actions the dialog's buttons run
// (`ACTIONS::replaceAndFindNext`, `replaceAll`, `updateFind`): eeschema/tools/sch_find_replace_tool.cpp SCH_FIND_REPLACE_TOOL::ReplaceAndFindNext /
// ReplaceAll / UpdateFind, driven by eeschema/dialogs/dialog_sch_find.cpp (`OnReplace`, `OnReplaceAll`, `OnSearchForText`). The matching and the
// replacing are the server's (`POST /api/sch/find`, `replace_text`); the state they read -- the search, the cursor, "selected objects only",
// the direction -- is `state.schFind`, so they work with the dialog closed, like the actions do in KiCad.
import type { Dispatch } from "react";
import { fetchSchFind } from "../../api/client";
import type { Action, StudioApi } from "../../state/store";
import { scopeFromSelection } from "../../kicad-port/schFind";
import { findNextMatch } from "./findNavigation";
import { setFindHighlights } from "../../state/commonTool";

/**
 * `ReplaceAndFindNext`: replace the match the cursor is on (when there is one), then go to the next. Without a current match it is just a
 * Find Next. The cursor continues after where the replaced item was: its own key when it still matches (so it is skipped, as `m_afterItem` is),
 * else the match before it.
 */
export async function replaceAndFindNext(api: StudioApi, dispatch: Dispatch<Action>): Promise<void> {
  const state = api.getState();
  const { search, cursor, backward } = state.schFind;
  if (search.find === "") return;
  if (cursor === null) {
    await findNextMatch(state, dispatch, backward);
    return;
  }
  const before = await fetchSchFind({ ...search, search_and_replace: true });
  const at = before.matches.findIndex((m) => m.key === cursor);
  if (at === -1) {
    await findNextMatch(state, dispatch, backward);
    return;
  }
  const prevKey = at > 0 ? (before.matches[at - 1]?.key ?? null) : null;
  const ok = await api.cmd({ op: "replace_text", search, items: [cursor] });
  if (!ok) return;
  const after = await fetchSchFind({ ...search, search_and_replace: false });
  const next = after.matches.some((m) => m.key === cursor) ? cursor : after.matches.some((m) => m.key === prevKey) ? prevKey : null;
  await findNextMatch(api.getState(), dispatch, backward, next);
}

/** `ReplaceAll`: every match -- or, with "selected objects only", those of the selection -- in one undoable step. */
export async function replaceAll(api: StudioApi, dispatch: Dispatch<Action>): Promise<void> {
  const state = api.getState();
  const { search, selectedOnly } = state.schFind;
  if (search.find === "") return;
  let items: string[] | null = null;
  const scope = scopeFromSelection(state.selection, selectedOnly);
  if (scope) {
    const found = await fetchSchFind({ ...search, search_and_replace: true }, scope);
    items = found.matches.map((m) => m.key);
    if (items.length === 0) {
      dispatch({ type: "SET_SCH_FIND", find: { status: "Nothing to replace in the selection." } });
      return;
    }
  }
  const ok = await api.cmd({ op: "replace_text", search, items });
  dispatch({ type: "SET_SCH_FIND", find: { cursor: null, status: ok ? "Replaced." : "Nothing replaced." } });
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
  const reply = await fetchSchFind({ ...search, search_and_replace: state.schDialog === "replace" }, scopeFromSelection(state.selection, selectedOnly));
  setFindHighlights(reply.ok ? [...new Set(reply.matches.map((m) => m.id))] : []);
}
