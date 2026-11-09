// Where the board editor's Net Inspector and Search panes are docked (kicad-port/pcbPaneDock.ts holds the rules): a small store of its own, like the dock layout's
// (state/dockLayoutStore.ts), because it is the person's window arrangement -- KiCad keeps it with the frame's AUI layout and Search's `aui.search_panel_*`
// settings -- not the document. Remembered in the browser; a bottom-dock height kept by the studio before this layout existed is where the bottom dock starts.
import { useSyncExternalStore } from "react";
import { clampDockSize, clampPaneRect, defaultPcbPaneDock, movePane, parsePcbPaneDock, type PaneRect, type PaneSide, type PcbPaneDock, type PcbPaneDockAt, type PcbPaneId } from "../kicad-port/pcbPaneDock";

const STORAGE_KEY = "eda-studio.pcb-pane-dock.v1";
/** The bottom dock's height as BottomDock.tsx kept it before the panes could dock elsewhere. */
const LEGACY_HEIGHT_KEY = "eda-studio.bottom-dock-height.v1";

function storage(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

function load(): PcbPaneDock {
  try {
    const st = storage();
    const legacy = Number(st?.getItem(LEGACY_HEIGHT_KEY));
    const bottom = Number.isFinite(legacy) && legacy >= 90 ? legacy : undefined;
    const raw = st?.getItem(STORAGE_KEY);
    return parsePcbPaneDock(raw ? JSON.parse(raw) : null, bottom);
  } catch {
    return defaultPcbPaneDock();
  }
}

let current: PcbPaneDock = load();
const listeners = new Set<() => void>();

function commit(next: PcbPaneDock): void {
  if (next === current) return;
  current = next;
  try {
    storage()?.setItem(STORAGE_KEY, JSON.stringify(next));
  } catch {
    /* blocked site data: the arrangement still applies for this session */
  }
  for (const l of [...listeners]) l();
}

function windowSize(): { w: number; h: number } {
  return typeof window !== "undefined" ? { w: window.innerWidth, h: window.innerHeight } : { w: 1600, h: 1000 };
}

export function getPcbPaneDock(): PcbPaneDock {
  return current;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function usePcbPaneDock(): PcbPaneDock {
  return useSyncExternalStore(subscribe, getPcbPaneDock, getPcbPaneDock);
}

/** Dock pane `id` on a side or float it (`from`: where its floating window opens, the pane's place on the page). */
export function setPcbPaneDock(id: PcbPaneId, to: PcbPaneDockAt, from?: { x: number; y: number }): void {
  const next = movePane(current, id, to, from);
  if (to === "float") {
    const { w, h } = windowSize();
    next.floating = { ...next.floating, [id]: clampPaneRect(id, next.floating[id], w, h) };
  }
  commit(next);
}

/** A dock's sash dragged: the dock's new depth, for the panes it holds. */
export function setPcbDockSize(side: PaneSide, size: number, panes: readonly PcbPaneId[]): void {
  const { w, h } = windowSize();
  commit({ ...current, size: { ...current.size, [side]: clampDockSize(side, size, panes, w, h) } });
}

/** A floating pane moved by its caption or sized by its corner. */
export function setPcbPaneFloatRect(id: PcbPaneId, r: PaneRect): void {
  const { w, h } = windowSize();
  const rect = clampPaneRect(id, r, w, h);
  const was = current.floating[id];
  if (was.x === rect.x && was.y === rect.y && was.w === rect.w && was.h === rect.h) return;
  commit({ ...current, floating: { ...current.floating, [id]: rect } });
}

/** Test and "reset layout" hook: both panes back under the canvas. */
export function resetPcbPaneDock(): void {
  commit(defaultPcbPaneDock());
}
