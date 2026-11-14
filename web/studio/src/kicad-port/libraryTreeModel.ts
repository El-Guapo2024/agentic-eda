// What the library tree of the Footprint and Symbol editors shows: the project's own entries and what the board's model already resolves (the "known"
// names the editors have always listed), plus KiCad's installed libraries -- every library as a group, its items once the person has expanded it (or
// searched). KiCad's tree (`LIB_TREE`, common/widgets/lib_tree_model_adapter.cpp) is the same shape: libraries as collapsible groups, items under them, a
// search box that filters across every library. Pure, so the merge, the ordering and the search rules are unit tested without a browser.
import { itemTerms, parseQuery, scoreItemTerms, searchTerm, type ChooserItem } from "./libChooser";
import { PROJECT_LIBRARY, splitLibName } from "./libraryNames";

export interface TreeItem {
  /** The library key: `Lib:Name`, or a bare name. */
  name: string;
  /** In the project library (editable in place); otherwise it comes from an installed library file, the board's model or the built-in table. */
  project: boolean;
  /** Listed from KiCad's installed libraries (not from the project or the model). */
  installed?: boolean;
}

export interface InstalledLib {
  name: string;
  count?: number;
}

export interface TreeGroup {
  lib: string;
  items: TreeItem[];
  /** KiCad has an installed library of this name (its items load when the group opens). */
  installed: boolean;
  /** The installed library's item count when known without loading it. */
  count?: number;
  /** Holds an entry of the project library: such groups are listed first, and open. */
  hasProject: boolean;
  /** While searching: the best score of its items (`LIB_TREE_NODE_LIBRARY::UpdateScore`), or its own name's while its items are not loaded. */
  score?: number;
}

/** What a search scores an item by (UI-LEFT-3 L3-5 follow-up): the tree's kind, each item's description, keywords and fields (`GET /api/library/entries_all`,
 *  by `Lib:Name`), and the shown columns. */
export interface TreeSearch {
  kind: "symbol" | "footprint";
  details?: ReadonlyMap<string, ChooserItem>;
  shownColumns?: readonly string[];
}

/** The most items a search draws: thousands of matches in a DOM tree help nobody; the search box says to narrow it. */
export const SEARCH_RESULT_CAP = 500;

const cmp = (a: string, b: string) => a.localeCompare(b, undefined, { sensitivity: "base", numeric: true });

/**
 * `known` (project + model + built-in names, flagged), the installed libraries, and the item names of every installed library loaded so far
 * (`loaded`: bare names by library) -> the groups to draw, plus whether the search was cut at `cap` items.
 *
 * - An item known twice (the model resolved `Device:R` and so does the installed library) is one item; it stays "project" if the project has it.
 * - A library with nothing loaded yet is a group with no items (its count, when known, is on the group); expanding it loads them.
 * - Groups: libraries holding a project entry first, then by name, case-insensitively with numbers in order, like the tree; items likewise by item name.
 * - `filter`: KiCad's search (`LIB_TREE_NODE_ITEM::UpdateScore` with `EDA_COMBINED_MATCHER`, kicad-port/libChooser.ts): every word must score on an item's
 *   search terms -- its library, name and `Lib:Name`, its keywords and description, a symbol's footprint and shown fields (`LIB_SYMBOL::GetSearchTerms`,
 *   `FOOTPRINT_INFO::GetSearchTerms`, `RebuildSearchTerms`) --; items are listed best first, then by name (`SortNodes( BEST_MATCH )`), groups by their best
 *   item. A group appears when an item matches, or -- while its items are not loaded -- when its own name does (`LIB_TREE_NODE_LIBRARY`'s name term, weight 8).
 */
export function buildTreeGroups(
  known: readonly TreeItem[],
  installedLibs: readonly InstalledLib[],
  loaded: ReadonlyMap<string, readonly string[]>,
  filter: string,
  cap = SEARCH_RESULT_CAP,
  search: TreeSearch = { kind: "symbol" }
): { groups: TreeGroup[]; truncated: boolean } {
  const matchers = parseQuery(filter);
  const q = matchers.length > 0 ? filter.trim().toLowerCase() : "";
  const byName = new Map<string, TreeItem>();
  for (const it of known) byName.set(it.name, it);
  for (const [lib, names] of loaded) {
    for (const item of names) {
      const name = `${lib}:${item}`;
      if (!byName.has(name)) byName.set(name, { name, project: false, installed: true });
    }
  }

  const installedByName = new Map(installedLibs.map((l) => [l.name, l]));
  const groupsByLib = new Map<string, TreeGroup>();
  const group = (lib: string): TreeGroup => {
    let g = groupsByLib.get(lib);
    if (!g) {
      const inst = installedByName.get(lib);
      g = { lib, items: [], installed: inst != null, count: inst?.count, hasProject: false };
      groupsByLib.set(lib, g);
    }
    return g;
  };
  for (const l of installedLibs) group(l.name);

  let shown = 0;
  let truncated = false;
  const scoreOf = (it: TreeItem) => {
    const { lib, item } = splitLibName(it.name);
    const detail: ChooserItem = search.details?.get(it.name) ?? { id: it.name, lib: lib || PROJECT_LIBRARY, name: item, description: "" };
    return scoreItemTerms(matchers, itemTerms(search.kind, detail, search.shownColumns));
  };
  const scored = [...byName.values()].map((it) => ({ it, score: q ? scoreOf(it) : 0 }));
  const matching = scored.filter((x) => !q || x.score > 0).sort((a, b) => b.score - a.score || cmp(a.it.name, b.it.name));
  for (const { it, score } of matching) {
    if (q && shown >= cap) {
      truncated = true;
      break;
    }
    const lib = splitLibName(it.name).lib || PROJECT_LIBRARY;
    const g = group(lib);
    g.items.push(it);
    if (it.project) g.hasProject = true;
    if (q) g.score = Math.max(g.score ?? 0, score);
    shown++;
  }

  let groups = [...groupsByLib.values()];
  if (q) {
    // A library that was never loaded cannot say whether it holds a match, so it shows by its own name; a loaded one shows only if an item matched.
    for (const g of groups) if (g.items.length === 0 && !loaded.has(g.lib) && g.installed) g.score = scoreItemTerms(matchers, [searchTerm(g.lib, 8)]);
    groups = groups.filter((g) => g.items.length > 0 || (!loaded.has(g.lib) && g.installed && (g.score ?? 0) > 0));
    groups.sort((a, b) => (b.score ?? 0) - (a.score ?? 0) || cmp(a.lib, b.lib));
  } else groups.sort((a, b) => Number(b.hasProject) - Number(a.hasProject) || cmp(a.lib, b.lib));
  return { groups, truncated };
}

/** Whether a group starts open: the ones holding entries the editor already knows (the project's own, the board's), never an installed library nobody asked for. */
export function defaultGroupOpen(g: TreeGroup): boolean {
  return g.items.some((it) => !it.installed);
}

/**
 * Whether the installed items of `g` should be loaded now: it is open (by default or by the person) and KiCad has a library of that name. Libraries that
 * are not installed (the project's `eda`) have nothing to load.
 */
export function needsLoading(g: TreeGroup, open: boolean, loaded: ReadonlyMap<string, readonly string[]>): boolean {
  return open && g.installed && !loaded.has(g.lib);
}
