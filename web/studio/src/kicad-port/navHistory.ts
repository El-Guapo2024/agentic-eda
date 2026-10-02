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
