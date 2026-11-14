// The schematic editor's Search pane state (`SCH_SEARCH_PANE`, UI-LEFT-2 SS10), the board editor's state/searchPane.ts for the other frame: KiCad keeps a
// pane per frame and the settings in each editor's own settings file (`EESCHEMA_SETTINGS::m_SearchPane`), so the schematic's are kept apart
// (`eda-studio.sch-search-pane.v1` in the person's browser). The query, the tab that is up and each tab's sort and columns last for the session.
import { useSyncExternalStore } from "react";
import { DEFAULT_SEARCH_PANE_SETTINGS, parseSearchPaneSettings, type SearchPaneSettings } from "../kicad-port/searchHandlers";
import type { SchSearchTabId } from "../kicad-port/schSearchHandlers";
import { getDockLayout, toggleDockPane } from "./dockLayoutStore";

const STORAGE_KEY = "eda-studio.sch-search-pane.v1";

export interface SchSearchPaneState {
  settings: SearchPaneSettings;
  query: string;
  tab: SchSearchTabId;
  sort: Partial<Record<SchSearchTabId, { col: number | null; ascending: boolean }>>;
  columns: Partial<Record<SchSearchTabId, { order?: number[]; widths?: number[] }>>;
  focus: number;
}

function storage(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

function loadSettings(): SearchPaneSettings {
  try {
    const raw = storage()?.getItem(STORAGE_KEY);
    return parseSearchPaneSettings(raw ? JSON.parse(raw) : null);
  } catch {
    return DEFAULT_SEARCH_PANE_SETTINGS;
  }
}

let current: SchSearchPaneState = { settings: loadSettings(), query: "", tab: "symbols", sort: {}, columns: {}, focus: 0 };
const listeners = new Set<() => void>();

function set(next: SchSearchPaneState): void {
  const saved = next.settings !== current.settings;
  current = next;
  if (saved) {
    try {
      storage()?.setItem(STORAGE_KEY, JSON.stringify(current.settings));
    } catch {
      /* blocked site data: the settings still apply for this session */
    }
  }
  for (const l of [...listeners]) l();
}

export function getSchSearchPane(): SchSearchPaneState {
  return current;
}

export function setSchSearchPane(patch: Partial<SchSearchPaneState>): void {
  set({ ...current, ...patch });
}

/** `SCH_EDIT_FRAME::FocusSearch` -> `SEARCH_PANE::FocusSearch`. */
export function focusSchSearchPane(): void {
  set({ ...current, focus: current.focus + 1 });
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useSchSearchPane(): SchSearchPaneState {
  return useSyncExternalStore(subscribe, getSchSearchPane, getSchSearchPane);
}

/** `SCH_EDIT_FRAME::ToggleSearch`: the pane shown or hidden; a pane just shown takes the focus and searches (`FocusSearch`, `RefreshSearch`). */
export function toggleSchSearchPane(): void {
  const wasShown = getDockLayout().shown.schSearch;
  toggleDockPane("schSearch");
  if (!wasShown) focusSchSearchPane();
}
