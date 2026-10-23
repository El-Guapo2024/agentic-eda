// The grid lists and fast grids of the board editor, the Footprint Editor and the Symbol Editor (`WINDOW_SETTINGS::grid.grids` / `fast_grid_1` / `fast_grid_2`,
// which KiCad keeps in each editor's own settings file): what the "Edit Grids..." dialog edits and the grid boxes, Next / Previous Grid and the fast grid
// hotkeys read. Plain module state with subscribers, kept in the browser's local storage like `state/commonOptions.ts` -- these are the user's preferences,
// not the design's. The rules for them are kicad-port/gridSettings.ts.
import { useSyncExternalStore } from "react";
import { GRID_EDITORS, isDefaultGridSettings, normalizeGridSettings, parseStoredGridSettings, type GridEditor, type GridSettings } from "../kicad-port/gridSettings";

/** The grids in nanometres. `OLD_STORAGE_KEY` held them in micrometres, before the design was in nanometres. */
const STORAGE_KEY = "eda-studio.grid-settings.nm";
const OLD_STORAGE_KEY = "eda-studio.grid-settings";

function storage(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

/** The stored grids, an old micrometre list read x1000 (and moved to the new key, once). */
function readStored(): string | null {
  const s = storage();
  if (!s) return null;
  try {
    const now = s.getItem(STORAGE_KEY);
    if (now !== null) return now;
    const old = s.getItem(OLD_STORAGE_KEY);
    if (old === null) return null;
    const parsed: unknown = JSON.parse(old);
    if (parsed && typeof parsed === "object") {
      for (const v of Object.values(parsed as Record<string, { grids?: unknown }>)) {
        if (v && Array.isArray(v.grids)) v.grids = v.grids.map((g) => (typeof g === "number" ? g * 1000 : g));
      }
    }
    const moved = JSON.stringify(parsed);
    s.setItem(STORAGE_KEY, moved);
    s.removeItem(OLD_STORAGE_KEY);
    return moved;
  } catch {
    return null;
  }
}

let current: Record<GridEditor, GridSettings> = parseStoredGridSettings(readStored());
const listeners = new Set<() => void>();

export function getGridSettings(editor: GridEditor): GridSettings {
  return current[editor];
}

/** Saves one editor's grids; a list that is the editor's default is not kept (so a later change of the default reaches it). */
export function setGridSettings(editor: GridEditor, settings: GridSettings): void {
  const next = normalizeGridSettings(settings);
  const now = current[editor];
  if (next.fast1 === now.fast1 && next.fast2 === now.fast2 && next.grids.length === now.grids.length && next.grids.every((g, i) => g === now.grids[i])) return;
  current = { ...current, [editor]: next };
  try {
    const kept: Partial<Record<GridEditor, GridSettings>> = {};
    for (const e of GRID_EDITORS) if (!isDefaultGridSettings(current[e], e)) kept[e] = current[e];
    storage()?.setItem(STORAGE_KEY, JSON.stringify(kept));
  } catch {
    /* private window or blocked site data: the grids still apply for this session */
  }
  for (const l of listeners) l();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** One editor's grids, re-rendering the caller whenever they change. */
export function useGridSettings(editor: GridEditor): GridSettings {
  return useSyncExternalStore(subscribe, () => current[editor], () => current[editor]);
}
