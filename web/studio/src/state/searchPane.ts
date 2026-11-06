// The Search panel's state (`SEARCH_PANE`, common/widgets/search_pane.cpp): its settings (`APP_SETTINGS_BASE::SEARCH_PANE` -- Zoom / Pan to Selection, Search
// Hidden Fields, Search Metadata), which KiCad keeps in the editor's settings file and the studio in the person's browser (like state/commonOptions.ts), and what
// it holds for the session: the last query (`m_lastQuery`, one for every tab), the tab that is up (`m_notebook`'s page) and each tab's sort column and order
// (`SEARCH_PANE_LISTVIEW::m_sortCol`, `m_sortAscending`). `focusSearch` is `SEARCH_PANE::FocusSearch`: a counter the panel's search box watches.
import { useSyncExternalStore } from "react";
import { DEFAULT_SEARCH_PANE_SETTINGS, parseSearchPaneSettings, type SearchPaneSettings, type SearchTabId } from "../kicad-port/searchHandlers";
import { getDockLayout, toggleDockPane } from "./dockLayoutStore";

const STORAGE_KEY = "eda-studio.search-pane.v1";

export interface SearchPaneState {
  settings: SearchPaneSettings;
  query: string;
  tab: SearchTabId;
  sort: Partial<Record<SearchTabId, { col: number | null; ascending: boolean }>>;
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

let current: SearchPaneState = { settings: loadSettings(), query: "", tab: "footprints", sort: {}, focus: 0 };
const listeners = new Set<() => void>();

function set(next: SearchPaneState): void {
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

export function getSearchPane(): SearchPaneState {
  return current;
}

export function setSearchPane(patch: Partial<SearchPaneState>): void {
  set({ ...current, ...patch });
}

/** `SEARCH_PANE::FocusSearch`. */
export function focusSearchPane(): void {
  set({ ...current, focus: current.focus + 1 });
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useSearchPane(): SearchPaneState {
  return useSyncExternalStore(subscribe, getSearchPane, getSearchPane);
}

/** `PCB_EDIT_FRAME::ToggleSearch`: the pane shown or hidden; a pane just shown takes the focus and searches (`FocusSearch`, `RefreshSearch`). */
export function toggleSearchPane(): void {
  const wasShown = getDockLayout().shown.search;
  toggleDockPane("search");
  if (!wasShown) focusSearchPane();
}
