// Which columns each library tree shows (`APP_SETTINGS_BASE::LIB_TREE::columns`, `lib_tree.columns` in each program's settings: the schematic editor's for the
// Symbol Chooser, the Symbol Editor's, the PCB editor's for the Footprint Chooser, the Footprint Editor's). KiCad keeps them in its settings files; the studio
// keeps them in this browser's localStorage (a per-viewer convenience, as the pinned libraries are, `state/libraryTree.ts`). Plain module state with
// subscribers, read with `useLibTreeColumns`.
import { useSyncExternalStore } from "react";
import { loadShownColumns, withColumnWidth } from "../kicad-port/libTreeColumns";

/** The four trees with settings of their own. */
export type LibTreeId = "symbol_chooser" | "footprint_chooser" | "symbol_editor" | "footprint_editor";

const STORAGE_KEY = "eda-studio.lib-tree-columns";
const IDS: readonly LibTreeId[] = ["symbol_chooser", "footprint_chooser", "symbol_editor", "footprint_editor"];

function load(): Record<LibTreeId, string[]> {
  let saved: Partial<Record<LibTreeId, unknown>> = {};
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    saved = raw ? (JSON.parse(raw) as Partial<Record<LibTreeId, unknown>>) : {};
  } catch {
    // no storage (private window, blocked site data): the defaults
  }
  const out = {} as Record<LibTreeId, string[]>;
  for (const id of IDS) {
    const v = saved[id];
    out[id] = loadShownColumns(Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : undefined);
  }
  return out;
}

let state = load();
const listeners = new Set<() => void>();

function subscribe(l: () => void): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

// The column widths of each tree (`lib_tree.column_widths`, keyed by header: `SaveSettings` keeps the width of every column it has made; UI-LEFT-2 LL4),
// kept in this browser's localStorage beside the columns.
const WIDTHS_KEY = "eda-studio.lib-tree-column-widths";

function loadWidths(): Record<LibTreeId, Record<string, number>> {
  let saved: Partial<Record<LibTreeId, unknown>> = {};
  try {
    const raw = localStorage.getItem(WIDTHS_KEY);
    saved = raw ? (JSON.parse(raw) as Partial<Record<LibTreeId, unknown>>) : {};
  } catch {
    // no storage: the defaults
  }
  const out = {} as Record<LibTreeId, Record<string, number>>;
  for (const id of IDS) {
    const v = saved[id];
    const kept: Record<string, number> = {};
    if (v && typeof v === "object") for (const [k, w] of Object.entries(v as Record<string, unknown>)) if (typeof w === "number" && Number.isFinite(w) && w > 0) kept[k] = w;
    out[id] = kept;
  }
  return out;
}

let widths = loadWidths();

export function useLibTreeColumnWidths(id: LibTreeId): Readonly<Record<string, number>> {
  return useSyncExternalStore(subscribe, () => widths[id], () => widths[id]);
}

/** A column's header dragged to a new width: kept for the next time the tree is made (`SaveSettings`). */
export function setLibTreeColumnWidth(id: LibTreeId, header: string, width: number): void {
  widths = { ...widths, [id]: withColumnWidth(widths[id], header, width) };
  try {
    localStorage.setItem(WIDTHS_KEY, JSON.stringify(widths));
  } catch {
    // ignore: it is a convenience
  }
  for (const l of listeners) l();
}

export function getLibTreeColumns(id: LibTreeId): readonly string[] {
  return state[id];
}

export function useLibTreeColumns(id: LibTreeId): readonly string[] {
  return useSyncExternalStore(subscribe, () => state[id], () => state[id]);
}

/** `SetShownColumns` + `SaveSettings`: the dialog's OK. */
export function setLibTreeColumns(id: LibTreeId, columns: readonly string[]): void {
  state = { ...state, [id]: [...columns] };
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    // ignore: it is a convenience
  }
  for (const l of listeners) l();
}

// The Select Columns dialog of a tree, opened from its header's menu or by running the action (`ACTIONS::selectLibTreeColumns` in a library editor).
let choosing: LibTreeId | null = null;

export function openSelectColumns(id: LibTreeId): void {
  choosing = id;
  for (const l of listeners) l();
}

export function closeSelectColumns(): void {
  choosing = null;
  for (const l of listeners) l();
}

export function useSelectColumnsOpen(id: LibTreeId): boolean {
  return useSyncExternalStore(subscribe, () => choosing === id, () => choosing === id);
}
