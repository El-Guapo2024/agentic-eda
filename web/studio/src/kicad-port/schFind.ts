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
  /** The cursor (`m_afterItem`) to remember: the picked match's key, or null after the end so the next call wraps to the start. */
  cursor: string | null;
}

/**
 * `SCH_FIND_REPLACE_TOOL::FindNext`'s selection of the next item.
 * `matches` is already in `nextMatch` order (ascending x, then y); for Find
 * Previous the list is walked reversed (`std::reverse( sorted_items )`).
 * `cursor` is the key of the last visited match (`m_afterItem`); `null`
 * starts from the beginning of the walk (also what the wrap-around timer
 * resets it to). If the cursor's match no longer exists (it was replaced
 * or edited away), the walk restarts from the beginning as well -- the
 * source's `item == aAfter` test never fires then either, so `past_item`
 * stays false... we instead restart, which is what a user expects after a
 * Replace consumed the current match.
 */
export function pickMatch(matches: readonly FindMatch[], cursor: string | null, reversed: boolean): PickResult {
  if (matches.length === 0) return { match: null, endReached: true, cursor: null };
  const walk = reversed ? [...matches].reverse() : matches;
  let start = 0;
  if (cursor !== null) {
    const at = walk.findIndex((m) => m.key === cursor);
    start = at === -1 ? 0 : at + 1;
  }
  const match = walk[start];
  if (!match) return { match: null, endReached: true, cursor: null };
  return { match, endReached: false, cursor: match.key };
}

/** The status line `FindNext` shows when the walk runs out (`msg + " " + "Find again to wrap around to the start."`). */
export function endReachedMessage(): string {
  return "Reached end of schematic. Find again to wrap around to the start.";
}

/** The `findNext` keys the replace verb may be restricted to when "Search only selected objects" is on: owner ids are what selection holds. */
export function scopeFromSelection(selection: ReadonlySet<string>, selectedOnly: boolean): string[] | undefined {
  return selectedOnly ? [...selection] : undefined;
}
