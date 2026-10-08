// What the library tree of the Footprint and Symbol editors shows: the project's own entries and what the board's model already resolves (the "known"
// names the editors have always listed), plus KiCad's installed libraries -- every library as a group, its items once the person has expanded it (or
// searched). KiCad's tree (`LIB_TREE`, common/widgets/lib_tree_model_adapter.cpp) is the same shape: libraries as collapsible groups, items under them, a
// search box that filters across every library. Pure, so the merge, the ordering and the search rules are unit tested without a browser.
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
 * - `filter`: a case-insensitive substring of the whole `Lib:Name`. A group appears when an item matches, or -- while its items are not loaded -- when
 *   its own name does (so a search shows something before the index is ready).
 */
export function buildTreeGroups(known: readonly TreeItem[], installedLibs: readonly InstalledLib[], loaded: ReadonlyMap<string, readonly string[]>, filter: string, cap = SEARCH_RESULT_CAP): { groups: TreeGroup[]; truncated: boolean } {
  const q = filter.trim().toLowerCase();
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
  const matching = [...byName.values()].filter((it) => !q || it.name.toLowerCase().includes(q)).sort((a, b) => cmp(a.name, b.name));
  for (const it of matching) {
    if (q && shown >= cap) {
      truncated = true;
      break;
    }
    const lib = splitLibName(it.name).lib || PROJECT_LIBRARY;
    const g = group(lib);
    g.items.push(it);
    if (it.project) g.hasProject = true;
    shown++;
  }

  let groups = [...groupsByLib.values()];
  if (q) {
    // A library that was never loaded cannot say whether it holds a match, so it shows by its own name; a loaded one shows only if an item matched.
    groups = groups.filter((g) => g.items.length > 0 || (!loaded.has(g.lib) && g.installed && g.lib.toLowerCase().includes(q)));
  }
  groups.sort((a, b) => Number(b.hasProject) - Number(a.hasProject) || cmp(a.lib, b.lib));
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
