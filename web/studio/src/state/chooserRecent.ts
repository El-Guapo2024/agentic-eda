// "-- Recently Used --" of the choosers: the last eight symbols / footprints chosen (`s_SymbolHistoryList`, `s_FootprintHistoryList`, `AddSymbolToHistory`
// keeps 8). KiCad keeps them for the session; here they also survive a reload, in this browser's localStorage (a per-viewer convenience).
import { addRecent, type ChooserItem, type ChooserKind } from "../kicad-port/libChooser";

const KEY = "eda-studio.chooser-recent";

function load(): Record<ChooserKind, ChooserItem[]> {
  try {
    const raw = localStorage.getItem(KEY);
    const parsed = raw ? (JSON.parse(raw) as Partial<Record<ChooserKind, ChooserItem[]>>) : {};
    const ok = (list: unknown): ChooserItem[] => (Array.isArray(list) ? list.filter((i): i is ChooserItem => !!i && typeof (i as ChooserItem).id === "string" && typeof (i as ChooserItem).name === "string").slice(0, 8) : []);
    return { symbol: ok(parsed.symbol), footprint: ok(parsed.footprint) };
  } catch {
    return { symbol: [], footprint: [] };
  }
}

let lists: Record<ChooserKind, ChooserItem[]> = load();

/** The recently chosen items of `kind`, newest first. The array only changes when something is chosen, so it is safe in a dependency list. */
export function recentItems(kind: ChooserKind): ChooserItem[] {
  return lists[kind];
}

export function rememberChosen(kind: ChooserKind, item: ChooserItem): void {
  // The score of the search it was found by means nothing next time.
  const { score: _score, ...plain } = item;
  lists = { ...lists, [kind]: addRecent(lists[kind], plain) };
  try {
    localStorage.setItem(KEY, JSON.stringify(lists));
  } catch {
    /* no storage: the session's list still works */
  }
}
