// Schematic Find / Find and Replace -- the pure, UI-free parts of
// eeschema/tools/sch_find_replace_tool.cpp (FindNext / ReplaceAndFindNext)
// and eeschema/dialogs/dialog_sch_find.cpp's option handling. The actual
// matching (EDA_ITEM::Matches/Replace) lives server-side
// (crates/ops/src/search.rs, POST /api/sch/find); this module only decides
// *which* match to visit next, exactly like `FindNext` does with its
// `m_afterItem` cursor and "Reached end of schematic. Find again to wrap
// around to the start." status.
import type { FindMatch, SchSearchData } from "../api/types";

/** `EDA_SEARCH_DATA`'s defaults (`EDA_SEARCH_DATA()` ctor: everything off, plain match). */
export function defaultSearch(): SchSearchData {
  return {
    find: "",
    replace: "",
    match_case: false,
    mode: "plain",
    search_hidden_fields: false,
    search_pins: false,
    search_net_names: false,
    replace_references: false,
    search_and_replace: false,
  };
}

export interface PickResult {
  /** The match to select + pan to, or null when the end of the list was reached. */
  match: FindMatch | null;
  /** True when the end (or, reversed, the start) was reached without a match -- KiCad shows "Reached end of schematic" and wraps on the *next* Find. */
  endReached: boolean;
  /** The cursor (`m_afterItem`) to remember: the picked match's id, or after the end one that makes the next call wrap to the start. */
  cursor: string | null;
}

/**
 * The cursor that makes the next Find start over from the first sheet (`m_wrapAroundTimer` running: `afterSheet = nullptr; m_afterItem = nullptr`).
 * Only used with the sheet in view known (`currentRank`): a Find with no cursor starts *there*, and a wrap has to start at the very beginning.
 */
export const WRAP_CURSOR = "\u0000wrap";

/** What names a match in the cursor: its `uid` (sheet and key) when the search went over the sheets, else its key. */
export function matchId(m: FindMatch): string {
  return m.uid ?? m.key;
}

/** True when a Find / Replace goes over every sheet: unless "Search the current sheet only" is on, or the search is of the selection (`FindNext`'s `searchAllSheets`). */
export function searchesAllSheets(currentSheetOnly: boolean, selectedOnly: boolean): boolean {
  return !currentSheetOnly && !selectedOnly;
}

/**
 * `SCH_FIND_REPLACE_TOOL::FindNext`'s selection of the next item.
 * `matches` is already in `nextMatch` order (the sheets in page order, each one's items ascending x, then y); for Find
 * Previous the list is walked reversed (`std::reverse( sorted_items )`, `std::reverse( paths )`).
 * `cursor` is the id of the last visited match (`m_afterItem`); `null` starts the walk: on the sheet in view when `currentRank` is given
 * (the sheets before it only come after a wrap), else at the beginning. If the cursor's match no longer exists (it was replaced
 * or edited away), the walk restarts from the beginning as well -- the
 * source's `item == aAfter` test never fires then either, so `past_item`
 * stays false... we instead restart, which is what a user expects after a
 * Replace consumed the current match.
 */
export function pickMatch(matches: readonly FindMatch[], cursor: string | null, reversed: boolean, currentRank?: number): PickResult {
  // After the end the next call starts over; with the sheet in view known that needs a cursor of its own (null starts on the sheet in view).
  const afterEnd = currentRank === undefined ? null : WRAP_CURSOR;
  if (matches.length === 0) return { match: null, endReached: true, cursor: null };
  const walk = reversed ? [...matches].reverse() : matches;
  let start = 0;
  if (cursor === null && currentRank !== undefined) {
    // sheets run in page order (reversed: back from the last); the walk begins on the sheet in view
    const first = walk.findIndex((m) => (reversed ? (m.sheet_rank ?? 0) <= currentRank : (m.sheet_rank ?? 0) >= currentRank));
    start = first === -1 ? walk.length : first;
  } else if (cursor !== null && cursor !== WRAP_CURSOR) {
    const at = walk.findIndex((m) => matchId(m) === cursor);
    start = at === -1 ? 0 : at + 1;
  }
  const match = walk[start];
  if (!match) return { match: null, endReached: true, cursor: afterEnd };
  return { match, endReached: false, cursor: matchId(match) };
}

/** The status line `FindNext` shows when the walk runs out (`msg + " " + "Find again to wrap around to the start."`): the schematic's end, or the sheet's with "current sheet only". */
export function endReachedMessage(allSheets = true): string {
  return `${allSheets ? "Reached end of schematic." : "Reached end of sheet."} Find again to wrap around to the start.`;
}

/** The `findNext` keys the replace verb may be restricted to when "Search only selected objects" is on: owner ids are what selection holds. */
export function scopeFromSelection(selection: ReadonlySet<string>, selectedOnly: boolean): string[] | undefined {
  return selectedOnly ? [...selection] : undefined;
}

/** `m_wrapAroundTimer.StartOnce( 4000 )`: after "Reached end of ...", only a Find within this many milliseconds wraps around to the start. */
export const WRAP_WINDOW_MS = 4000;

/**
 * The cursor a Find starts from, given when the end was last reported (`wrapArmedAt`, ms; null when it was not): within {@link WRAP_WINDOW_MS}
 * the wrap timer is running (`afterSheet = nullptr; m_afterItem = nullptr`), so the walk starts over -- at the first sheet when the search
 * goes over the sheets ({@link WRAP_CURSOR}), else at the start of the sheet. Later, the walk goes on from `cursor` as before and reports
 * the end again.
 */
export function findStartCursor(cursor: string | null, wrapArmedAt: number | null, now: number, allSheets: boolean): string | null {
  if (wrapArmedAt !== null && now - wrapArmedAt < WRAP_WINDOW_MS) return allSheets ? WRAP_CURSOR : null;
  return cursor;
}

/**
 * `SCH_FIND_REPLACE_TOOL::getCurrentMatch` + `HasMatch`: what Replace replaces. With "Search the current selection only" it is the match
 * the Find cursor is on (`m_afterItem`); otherwise it is the selected item (`m_selectionTool->GetSelection().Front()`), when it matches.
 * The selection holds owner ids (a symbol for its fields), so the match the cursor is on is taken when it belongs to the selected item,
 * else the item's first match. Null when there is none: Replace then does nothing (`if( item && HasMatch() )`).
 */
export function currentMatch(matches: readonly FindMatch[], cursor: string | null, selection: readonly string[], selectedOnly: boolean): FindMatch | null {
  if (selectedOnly) return matches.find((m) => matchId(m) === cursor) ?? null;
  const front = selection[0];
  if (front === undefined) return null;
  const onCursor = matches.find((m) => matchId(m) === cursor);
  if (onCursor && onCursor.id === front) return onCursor;
  return matches.find((m) => m.id === front) ?? null;
}
