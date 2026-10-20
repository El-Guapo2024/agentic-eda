// What the Design Blocks panes (`DESIGN_BLOCK_PANE`, one in the schematic frame and one in the board frame) remember beyond what the libraries list: the item selected in the
// tree, the four check boxes under it, the recently used blocks, the pinned libraries, and a counter that says the libraries or the blocks changed so the panels read them
// again (`RefreshLibs`). Plain module state with subscribers, like `state/libraryTree.ts`, so an action registered in the action runner and the component see the same panel.
//
// KiCad keeps the check boxes in the frame's settings (`design_block_chooser.repeated_placement`, `place_as_sheet`, `place_as_group`, `keep_annotations`: the group on, the
// others off), the recently used blocks as the frame's `m_designBlockHistoryList` (seven, newest first) and the pinned libraries in the session settings and the project
// file. The studio has no settings file to write them to, so they are this browser's (localStorage, a per-viewer convenience); the selection and the counter last as long
// as the page.
import { useSyncExternalStore } from "react";
import { fetchBlocks, type BlocksReply } from "../api/designBlockClient";
import type { PlaceOptions } from "../kicad-port/designBlocks";

export type { PlaceOptions };

/** `Lib:Name` for a block, `Lib:` for a library row (`LIB_ID( libName, wxEmptyString )`), null for nothing. */
export type BlockId = string;

/** The two editors that have a Design Blocks pane. */
export type BlockEditor = "schematic" | "pcb";


export interface DesignBlocksUi {
  selected: BlockId | null;
  options: PlaceOptions;
  /** The blocks last placed, newest first, at most seven. */
  history: readonly BlockId[];
  pinned: ReadonlySet<string>;
  /** The search box. */
  search: string;
  /** Bumped when a library table or a library changed. */
  version: number;
  /** The pane is shown, per editor (`aui.design_blocks_show`, off to begin with). */
  shown: Record<BlockEditor, boolean>;
  /** The libraries and their blocks as the server last listed them; null before the first answer. */
  reply: BlocksReply | null;
  loading: boolean;
  /** Why the last listing failed. */
  error: string | null;
}

/** `design_block_chooser.*` defaults of `APP_SETTINGS_BASE` (`app_settings.cpp`). */
export const DEFAULT_OPTIONS: PlaceOptions = { repeated: false, asGroup: true, asSheet: false, keepAnnotations: false };

/** `PANEL_DESIGN_BLOCK_CHOOSER::addDesignBlockToHistory`: no duplicate, newest first, then `while( size() >= 8 ) pop_back()`: seven blocks stay. */
export const HISTORY_MAX = 7;

const STORAGE_KEY = "eda-studio.design-blocks";

interface Saved {
  options?: Partial<PlaceOptions>;
  history?: string[];
  pinned?: string[];
  shown?: Partial<Record<BlockEditor, boolean>>;
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
    shown: { schematic: saved.shown?.schematic === true, pcb: saved.shown?.pcb === true },
    reply: null,
    loading: false,
    error: null,
  };
}

let current: DesignBlocksUi = load();
const listeners = new Set<() => void>();

function save(): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ options: current.options, history: [...current.history], pinned: [...current.pinned], shown: current.shown } satisfies Saved));
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

/** `AddDesignBlockLibrary`'s `SelectLibId( LIB_ID( libName, "" ) )`: the new library's row is the one selected, once the listing has it. */
export function selectDesignBlockLibrary(lib: string): void {
  set({ selected: `${lib}:` });
  void refreshDesignBlocks();
}

/** `OnCheckBox`. */
export function setPlaceOptions(patch: Partial<PlaceOptions>): void {
  set({ options: { ...current.options, ...patch } }, true);
}

let inflight: Promise<BlocksReply | null> | null = null;

/** The libraries and their blocks, read from the server (once at a time). */
function fetchListing(): Promise<BlocksReply | null> {
  if (inflight) return inflight;
  set({ loading: true });
  inflight = fetchBlocks()
    .then(
      (reply) => {
        set({ reply, error: reply.ok ? null : (reply.message ?? "The design block libraries could not be read."), loading: false });
        return reply;
      },
      (e: unknown) => {
        set({ error: e instanceof Error ? e.message : String(e), loading: false });
        return null;
      }
    )
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

/** The listing the panel shows, read when there is none yet. */
export function ensureDesignBlocks(): Promise<BlocksReply | null> {
  return current.reply ? Promise.resolve(current.reply) : fetchListing();
}

/** `RefreshLibs`: the libraries or the blocks in them changed -- the listing is read again. */
export function refreshDesignBlocks(): Promise<BlocksReply | null> {
  set({ version: current.version + 1 });
  return fetchListing();
}

/** `showDesignBlockPanel` (`PANE_INFO.Show( !IsShown() )`): the pane of an editor shows or hides. */
export function setDesignBlockPaneShown(editor: BlockEditor, shown: boolean): void {
  if (current.shown[editor] === shown) return;
  set({ shown: { ...current.shown, [editor]: shown } }, true);
}

export function toggleDesignBlockPane(editor: BlockEditor): void {
  setDesignBlockPaneShown(editor, !current.shown[editor]);
}

/** The check mark of View > Design Blocks (`designBlockCond`: `m_auimgr.GetPane( DesignBlocksPaneName() ).IsShown()`); undefined for any other action. */
export function designBlockChecked(name: string): boolean | undefined {
  if (name === "eeschema.SchDesignBlockControl.showDesignBlockPanel") return current.shown.schematic;
  if (name === "pcbnew.PcbDesignBlockControl.showDesignBlockPanel") return current.shown.pcb;
  return undefined;
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
  current = { selected: null, options: { ...DEFAULT_OPTIONS }, history: [], pinned: new Set(), search: "", version: 0, shown: { schematic: false, pcb: false }, reply: null, loading: false, error: null };
  for (const l of [...listeners]) l();
}
