// Where the docked panes of the editor frames are (kicad-port/dockLayout.ts holds the rules): a tiny store of its own rather than a branch of the
// board reducer in store.tsx, because the layout is the person's window arrangement, not the document -- it outlives a board reload, is read by the
// frame, the panes and the toolbar actions alike, and is remembered in the browser (wxAUI's perspective in KiCad's settings file).
//
// A plain module-level store read through `useSyncExternalStore`: the handlers of the toolbar actions (`showProperties`, `showHierarchy`,
// `showLayersManager`) are not React and call `toggleDockPane` directly.
import { useSyncExternalStore } from "react";
import { defaultDockLayout, parseDockLayout, setColumnCollapsed, toggleFolded, togglePane, type DockColumnId, type DockLayout, type DockPaneId } from "../kicad-port/dockLayout";

const STORAGE_KEY = "eda-studio.dock-layout.v1";

function storage(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null; // blocked site data, a private window: the layout simply is not remembered
  }
}

function windowWidth(): number {
  return typeof window !== "undefined" ? window.innerWidth : 1600;
}

function load(): DockLayout {
  try {
    const raw = storage()?.getItem(STORAGE_KEY);
    return parseDockLayout(raw ? JSON.parse(raw) : null, windowWidth());
  } catch {
    return defaultDockLayout(windowWidth());
  }
}

let current: DockLayout = load();
const listeners = new Set<() => void>();

function commit(next: DockLayout): void {
  if (next === current) return;
  current = next;
  try {
    storage()?.setItem(STORAGE_KEY, JSON.stringify(next));
  } catch {
    /* quota or blocked: keep the in-memory layout */
  }
  for (const l of [...listeners]) l();
}

export function getDockLayout(): DockLayout {
  return current;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The current layout, re-rendering the caller when it changes. */
export function useDockLayout(): DockLayout {
  return useSyncExternalStore(subscribe, getDockLayout, getDockLayout);
}

/** A pane's toolbar toggle: shows or hides it (showing it unfolds it and opens its column). */
export function toggleDockPane(id: DockPaneId): void {
  commit(togglePane(current, id));
}

/** The caption chevron: roll a pane up to its caption or open it again. */
export function toggleDockPaneFolded(id: DockPaneId): void {
  commit(toggleFolded(current, id));
}

export function setDockColumnCollapsed(column: DockColumnId, collapsed: boolean): void {
  commit(setColumnCollapsed(current, column, collapsed));
}

/** Test and "reset layout" hook: back to the first-run layout of this window. */
export function resetDockLayout(): void {
  commit(defaultDockLayout(windowWidth()));
}
