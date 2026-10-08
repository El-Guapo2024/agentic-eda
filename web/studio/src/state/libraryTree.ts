// What the library tree of the Footprint Editor and the Symbol Editor remembers beyond what it lists: which libraries are open or folded
// (`ExpandAll` / `CollapseAll`), which are pinned (`PinLibrary` / `UnpinLibrary`) and which library rows are selected
// (`LIB_TREE::GetSelectedTreeNodes`). Plain module state with subscribers, like `state/commonOptions.ts`, so the actions registered in the action
// runner and the tree component see the same tree. (Whether the tree is shown is the dock layout's -- `state/dockLayoutStore.ts`, the Libraries column.)
//
// KiCad saves the pinned libraries in the project file (`m_PinnedFootprintLibs`, `m_PinnedSymbolLibs`) and the session settings; the studio has no
// project file to write them to, so they are kept in this browser's localStorage (a per-viewer convenience) and the folds and the library
// selection last as long as the page.
import { useSyncExternalStore } from "react";
import { ALL_COLLAPSED, ALL_EXPANDED, FOLD_AUTO, setGroupOpen, withPinned, type TreeFold } from "../kicad-port/libraryTreeState";

export type TreeKind = "footprint" | "symbol";

export interface LibraryTreeUi {
  fold: TreeFold;
  /** The pinned libraries' nicknames. */
  pinned: ReadonlySet<string>;
  /** The library rows selected (nicknames); a selection of items has none. */
  selectedLibs: readonly string[];
}

const STORAGE_KEY = "eda-studio.library-tree";

interface Saved {
  pinned?: string[];
}

function load(kind: TreeKind): LibraryTreeUi {
  let saved: Saved = {};
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    const all = raw ? (JSON.parse(raw) as Partial<Record<TreeKind, Saved>>) : {};
    saved = all[kind] ?? {};
  } catch {
    // no storage (private window, blocked site data): the defaults
  }
  return { fold: FOLD_AUTO, pinned: new Set(Array.isArray(saved.pinned) ? saved.pinned.filter((x) => typeof x === "string") : []), selectedLibs: [] };
}

const KINDS: readonly TreeKind[] = ["footprint", "symbol"];
const states: Record<TreeKind, LibraryTreeUi> = { footprint: load("footprint"), symbol: load("symbol") };
const listeners = new Set<() => void>();

function save(): void {
  try {
    const out: Partial<Record<TreeKind, Saved>> = {};
    for (const kind of KINDS) out[kind] = { pinned: [...states[kind].pinned] };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(out));
  } catch {
    // ignore: it is a convenience
  }
}

function set(kind: TreeKind, patch: Partial<LibraryTreeUi>, persist = false): void {
  states[kind] = { ...states[kind], ...patch };
  if (persist) save();
  for (const l of listeners) l();
}

export function getLibraryTree(kind: TreeKind): LibraryTreeUi {
  return states[kind];
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useLibraryTree(kind: TreeKind): LibraryTreeUi {
  return useSyncExternalStore(subscribe, () => states[kind], () => states[kind]);
}

/** `ExpandAll()` / `CollapseAll()` of the tree control. */
export function expandAllLibraries(kind: TreeKind): void {
  set(kind, { fold: ALL_EXPANDED });
}

export function collapseAllLibraries(kind: TreeKind): void {
  set(kind, { fold: ALL_COLLAPSED });
}

/** A click on a library's row: it goes open or folded. */
export function setLibraryOpen(kind: TreeKind, lib: string, open: boolean): void {
  set(kind, { fold: setGroupOpen(states[kind].fold, lib, open) });
}

export function selectLibraries(kind: TreeKind, libs: readonly string[]): void {
  const same = libs.length === states[kind].selectedLibs.length && libs.every((l, i) => l === states[kind].selectedLibs[i]);
  if (!same) set(kind, { selectedLibs: libs });
}

/** `LIBRARY_EDITOR_CONTROL::changeSelectedPinStatus`: pin (or unpin) the selected libraries. True when something changed. */
export function pinSelectedLibraries(kind: TreeKind, pin: boolean): boolean {
  const ui = states[kind];
  const pinned = withPinned(ui.pinned, ui.selectedLibs, pin);
  if (pinned === ui.pinned) return false;
  set(kind, { pinned }, true);
  return true;
}
