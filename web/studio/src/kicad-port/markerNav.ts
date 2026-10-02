// Find Next Marker: the markers-only branch of SCH_FIND_REPLACE_TOOL::FindNext
// (eeschema/tools/sch_find_replace_tool.cpp, entered via
// ACTIONS::findNextMarker, which sets `data.markersOnly = true`).
//
// nextMatch() there sorts the screen's items by position (x, then y, ties
// broken by UUID), walks past the previously-found item (`m_afterItem`) and
// returns the next SCH_MARKER. Falling off the end shows "Reached end of
// sheet. Find again to wrap around to the start." and clears the cursor so
// the next press wraps. This is that walk over an already-resolved list of
// marker positions (the ERC report's violations, positioned by
// components/schematic/ercMarkerPosition.ts).

export interface MarkerPos {
  /** Stable identity of the marker (stands in for the item pointer / UUID). */
  key: string;
  x: number;
  y: number;
}

/** nextMatch()'s std::sort: ascending x, then ascending y, then key (the "deterministic sort" tiebreak). */
export function sortMarkers(markers: readonly MarkerPos[]): MarkerPos[] {
  return [...markers].sort((a, b) => a.x - b.x || a.y - b.y || (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
}

export interface NextMarkerResult {
  marker: MarkerPos | null;
  /** The cursor (`m_afterItem`) to keep: the found marker's key, or null once the end is reached so the next call wraps. */
  cursor: string | null;
  /** True when no marker follows the cursor (source's "Reached end..." branch). */
  endReached: boolean;
}

/** Walk past `afterKey` (null = from the start) and return the next marker in sorted order. A cursor naming a marker that no longer exists restarts from the beginning. */
export function nextMarker(markers: readonly MarkerPos[], afterKey: string | null, reversed = false): NextMarkerResult {
  const sorted = sortMarkers(markers);
  const walk = reversed ? sorted.reverse() : sorted;
  let start = 0;
  if (afterKey !== null) {
    const at = walk.findIndex((m) => m.key === afterKey);
    start = at === -1 ? 0 : at + 1;
  }
  const marker = walk[start] ?? null;
  return marker ? { marker, cursor: marker.key, endReached: false } : { marker: null, cursor: null, endReached: true };
}
