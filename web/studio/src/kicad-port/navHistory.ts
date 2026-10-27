// Port of SCH_NAVIGATE_TOOL's sheet history (eeschema/tools/sch_navigate_tool.cpp):
// m_navHistory/m_navIndex, pushToHistory, CanGoBack/CanGoForward, Back/Forward
// (Alt+Left / Alt+Right). A "path" here is a sheet path (root-to-here sheet
// ids), the same thing `state.currentSheetPath` holds. Pure/immutable.

export interface NavHistory {
  entries: string[][];
  index: number;
}

export const initialNavHistory = (): NavHistory => ({ entries: [[]], index: 0 });

const same = (a: string[], b: string[]) => a.length === b.length && a.every((v, i) => v === b[i]);

export const canGoBack = (h: NavHistory) => h.entries.length > 0 && h.index > 0;
export const canGoForward = (h: NavHistory) => h.entries.length > 0 && h.index < h.entries.length - 1;

/** pushToHistory: drop the forward tail, append unless it duplicates the last entry. */
export function pushToHistory(h: NavHistory, path: string[]): NavHistory {
  let entries = h.entries;
  if (canGoForward(h)) entries = entries.slice(0, h.index + 1);
  if (entries.length === 0 || !same(entries[entries.length - 1]!, path)) entries = [...entries, path];
  return { entries, index: entries.length - 1 };
}

/** Back(): index-- (caller then navigates to entries[index]). Returns null when !CanGoBack (source: wxBell). */
export function goBack(h: NavHistory): NavHistory | null {
  return canGoBack(h) ? { ...h, index: h.index - 1 } : null;
}

/** Forward(): index++. */
export function goForward(h: NavHistory): NavHistory | null {
  return canGoForward(h) ? { ...h, index: h.index + 1 } : null;
}

/**
 * CleanHistory (sch_navigate_tool.cpp, run by `SCH_EDIT_FRAME::UpdateHierarchyNavigator` on every hierarchy change): drop each entry that no
 * longer names a sheet path of the schematic (`sheets`, `Schematic().Hierarchy()`), and an entry equal to the one before it ("Don't allow
 * multiple consecutive instances of the same history"); then the index goes to the last entry (the first when one or none is left).
 * (KiCad's `wxCHECK( !sheets.empty() )` returns with the history untouched; so does this.)
 */
export function cleanHistory(h: NavHistory, sheets: string[][]): NavHistory {
  if (sheets.length === 0) return h;
  const entries: string[][] = [];
  for (const e of h.entries) {
    if (!sheets.some((s) => same(s, e))) continue;
    if (entries.length > 0 && same(entries[entries.length - 1]!, e)) continue;
    entries.push(e);
  }
  return { entries, index: entries.length <= 1 ? 0 : entries.length - 1 };
}

/** ResetHistory (sch_navigate_tool.cpp, on load, `files-io.cpp` / `sch_edit_frame.cpp`): one entry, the sheet in view. */
export const resetHistory = (current: string[]): NavHistory => ({ entries: [current], index: 0 });
