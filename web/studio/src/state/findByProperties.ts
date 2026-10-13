// The board editor's Find by Properties dialog (`PCB_EDIT_FRAME::m_findByPropertiesDialog`, `DIALOG_FIND_BY_PROPERTIES`): which tab is up, the rows of the From Selection grid with
// their Match modes, the query and its status, and the options. Like KiCad's it is created once and only hidden, modeless, and it keeps its rows when the selection is empty
// (`rebuildPropertyGrid` returns before it clears `m_propertyRows`), so Select Matching can be pressed again after it cleared the selection. As state/pcbFind.ts it is plain
// module state with subscribers. What the rows and the query mean is kicad-port/findByProperties.ts; what the buttons do is actions/findByPropertiesActions.ts.
import { useSyncExternalStore } from "react";
import type { FindRow } from "../kicad-port/findByProperties";

export interface FindByPropertiesState {
  open: boolean;
  /** Bumped each time the dialog is asked to show, so it takes the focus again. */
  shown: number;
  /** `m_notebook->GetSelection()`: "From Selection" (0) or "Query" (1). */
  tab: "selection" | "query";
  /** `m_statusLabel`: "No items selected", the friendly name of the one item, "N objects selected", then "N items matched". */
  status: string;
  rows: FindRow[];
  /** `m_selectedTypes`: the classes of the selection the rows were built for. */
  types: string[];
  /** The selection and the board revision the rows were built for: they are built again when it changes (`OnSelectionChanged`). */
  builtFor: string | null;
  /** `m_queryEditor`. */
  query: string;
  /** `m_queryStatusLabel`. */
  queryStatus: string;
  /** The text of the `Expression Error` box a failed compile opens, null when none is up. */
  errorBox: string | null;
  /** `m_recentQueries` (`pcbnew.find_by_properties.recent_queries`, ten at most). */
  recent: string[];
  /** "Deselect non-matching items" (on by default) and "Zoom to fit selected items" (off). */
  deselect: boolean;
  zoom: boolean;
}

const RECENT_KEY = "eda.findByProperties.recent";

function loadRecent(): string[] {
  try {
    const raw = typeof localStorage === "undefined" ? null : localStorage.getItem(RECENT_KEY);
    const list: unknown = raw ? JSON.parse(raw) : [];
    return Array.isArray(list) ? list.filter((q): q is string => typeof q === "string").slice(0, 10) : [];
  } catch {
    return [];
  }
}

function saveRecent(list: readonly string[]): void {
  try {
    if (typeof localStorage !== "undefined") localStorage.setItem(RECENT_KEY, JSON.stringify(list));
  } catch {
    // a viewer without storage keeps the list for the session only
  }
}

const initial = (): FindByPropertiesState => ({
  open: false,
  shown: 0,
  tab: "selection",
  status: "No items selected",
  rows: [],
  types: [],
  builtFor: null,
  query: "",
  queryStatus: "",
  errorBox: null,
  recent: loadRecent(),
  deselect: true,
  zoom: false,
});

let current: FindByPropertiesState = initial();
const listeners = new Set<() => void>();

export function getFindByProperties(): FindByPropertiesState {
  return current;
}

export function setFindByProperties(patch: Partial<FindByPropertiesState>): void {
  current = { ...current, ...patch };
  if (patch.recent) saveRecent(current.recent);
  for (const l of listeners) l();
}

/** Back to a fresh dialog (tests, and a new design). */
export function resetFindByProperties(): void {
  current = initial();
  for (const l of listeners) l();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useFindByProperties(): FindByPropertiesState {
  return useSyncExternalStore(subscribe, () => current, () => current);
}
