// What the Design Blocks panes (`DESIGN_BLOCK_PANE`, one in the schematic frame and one in the board frame) remember beyond what the libraries list: the item selected in the
// tree, the four check boxes under it, the recently used blocks, the pinned libraries, and a counter that says the libraries or the blocks changed so the panels read them
// again (`RefreshLibs`). Plain module state with subscribers, like `state/libraryTree.ts`, so an action registered in the action runner and the component see the same panel.
//
// KiCad keeps the check boxes in the frame's settings (`design_block_chooser.repeated_placement`, `place_as_sheet`, `place_as_group`, `keep_annotations`: the group on, the
// others off), the recently used blocks as the frame's `m_designBlockHistoryList` (eight, newest first) and the pinned libraries in the session settings and the project
// file. The studio has no settings file to write them to, so they are this browser's (localStorage, a per-viewer convenience); the selection and the counter last as long
// as the page.
import { useSyncExternalStore } from "react";

/** `Lib:Name` for a block, `Lib:` for a library row (`LIB_ID( libName, wxEmptyString )`), null for nothing. */
export type BlockId = string;

export interface PlaceOptions {
  /** "Place repeated copies". */
  repeated: boolean;
  /** "Place as group". */
  asGroup: boolean;
  /** "Place as sheet" (the schematic's only). */
  asSheet: boolean;
  /** "Keep annotations". */
  keepAnnotations: boolean;
}

export interface DesignBlocksUi {
  selected: BlockId | null;
  options: PlaceOptions;
  /** The blocks last placed, newest first, at most eight. */
  history: readonly BlockId[];
  pinned: ReadonlySet<string>;
  /** The search box. */
  search: string;
  /** Bumped when a library table or a library changed. */
  version: number;
}

/** `design_block_chooser.*` defaults of `APP_SETTINGS_BASE` (`app_settings.cpp`). */
export const DEFAULT_OPTIONS: PlaceOptions = { repeated: false, asGroup: true, asSheet: false, keepAnnotations: false };

/** `PANEL_DESIGN_BLOCK_CHOOSER::addDesignBlockToHistory`: no duplicate, newest first; the list is cut to leave seven before the new one (`size() >= 8` pops). */
export const HISTORY_MAX = 8;

const STORAGE_KEY = "eda-studio.design-blocks";

interface Saved {
  options?: Partial<PlaceOptions>;
  history?: string[];
  pinned?: string[];
}

function load(): DesignBlocksUi {
  let saved: Saved = {};
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    saved = raw ? (JSON.parse(raw) as Saved) : {};
  } catch {
    // no storage (private window, blocked site data): the defaults
  }
  const flag = (k: keyof PlaceOptions) => (typeof saved.options?.[k] === "boolean" ? (saved.options[k] as boolean) : DEFAULT_OPTIONS[k]);
  return {
    selected: null,
    options: { repeated: flag("repeated"), asGroup: flag("asGroup"), asSheet: flag("asSheet"), keepAnnotations: flag("keepAnnotations") },
    history: Array.isArray(saved.history) ? saved.history.filter((x): x is string => typeof x === "string").slice(0, HISTORY_MAX) : [],
    pinned: new Set(Array.isArray(saved.pinned) ? saved.pinned.filter((x): x is string => typeof x === "string") : []),
    search: "",
    version: 0,
  };
}

let current: DesignBlocksUi = load();
const listeners = new Set<() => void>();

function save(): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ options: current.options, history: [...current.history], pinned: [...current.pinned] } satisfies Saved));
  } catch {
    // ignore: it is a convenience
  }
}

function set(patch: Partial<DesignBlocksUi>, persist = false): void {
  current = { ...current, ...patch };
  if (persist) save();
  for (const l of [...listeners]) l();
}

export function getDesignBlocksUi(): DesignBlocksUi {
  return current;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useDesignBlocksUi(): DesignBlocksUi {
  return useSyncExternalStore(subscribe, getDesignBlocksUi, getDesignBlocksUi);
}

/** `SelectLibId`. */
export function selectDesignBlock(id: BlockId | null): void {
  if (id !== current.selected) set({ selected: id });
}

/** `AddDesignBlockLibrary`'s `SelectLibId( LIB_ID( libName, "" ) )`: the new library's row is the one selected. */
export function selectDesignBlockLibrary(lib: string): void {
  set({ selected: `${lib}:`, version: current.version + 1 });
}

/** `OnCheckBox`. */
export function setPlaceOptions(patch: Partial<PlaceOptions>): void {
  set({ options: { ...current.options, ...patch } }, true);
}

/** `RefreshLibs`: the libraries or the blocks in them changed. */
export function refreshDesignBlocks(): void {
  set({ version: current.version + 1 });
}

export function setDesignBlockSearch(search: string): void {
  if (search !== current.search) set({ search });
}

/** `addDesignBlockToHistory`: the block just placed goes first, and an older entry for it goes. */
export function addToHistory(id: BlockId): void {
  const next = [id, ...current.history.filter((h) => h !== id)].slice(0, HISTORY_MAX);
  set({ history: next }, true);
}

/** `PinLibrary` / `UnpinLibrary`. */
export function setDesignBlockLibraryPinned(lib: string, pin: boolean): void {
  if (current.pinned.has(lib) === pin) return;
  const pinned = new Set(current.pinned);
  if (pin) pinned.add(lib);
  else pinned.delete(lib);
  set({ pinned }, true);
}

/** Test hook: back to the first-run state. */
export function resetDesignBlocksUi(): void {
  current = { selected: null, options: { ...DEFAULT_OPTIONS }, history: [], pinned: new Set(), search: "", version: 0 };
  for (const l of [...listeners]) l();
}
