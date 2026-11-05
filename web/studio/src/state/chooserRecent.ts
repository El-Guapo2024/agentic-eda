// "-- Recently Used --" of the choosers: the last eight symbols / footprints chosen (`s_SymbolHistoryList`, `s_FootprintHistoryList`, `AddSymbolToHistory`
// keeps 8), and Place Power's own list of power symbols (`SCH_DRAWING_TOOLS::m_powerHistoryList`, apart from Place Symbol's `m_symbolHistoryList`).
// KiCad keeps them for the session; here they also survive a reload, in this browser's localStorage (a per-viewer convenience).
import { addRecent, type ChooserItem, type ChooserKind } from "../kicad-port/libChooser";

const KEY = "eda-studio.chooser-recent";

/** Which list: a chooser kind's, or Place Power's. */
export type RecentList = ChooserKind | "power";

function load(): Record<RecentList, ChooserItem[]> {
  try {
    const raw = localStorage.getItem(KEY);
    const parsed = raw ? (JSON.parse(raw) as Partial<Record<RecentList, ChooserItem[]>>) : {};
    const ok = (list: unknown): ChooserItem[] => (Array.isArray(list) ? list.filter((i): i is ChooserItem => !!i && typeof (i as ChooserItem).id === "string" && typeof (i as ChooserItem).name === "string").slice(0, 8) : []);
    return { symbol: ok(parsed.symbol), footprint: ok(parsed.footprint), power: ok(parsed.power) };
  } catch {
    return { symbol: [], footprint: [], power: [] };
  }
}

let lists: Record<RecentList, ChooserItem[]> = load();

/** The recently chosen items of `list`, newest first. The array only changes when something is chosen, so it is safe in a dependency list. */
export function recentItems(list: RecentList): ChooserItem[] {
  return lists[list];
}

export function rememberChosen(list: RecentList, item: ChooserItem): void {
  // The score of the search it was found by means nothing next time.
  const { score: _score, ...plain } = item;
  lists = { ...lists, [list]: addRecent(lists[list], plain) };
  try {
    localStorage.setItem(KEY, JSON.stringify(lists));
  } catch {
    /* no storage: the session's list still works */
  }
}
