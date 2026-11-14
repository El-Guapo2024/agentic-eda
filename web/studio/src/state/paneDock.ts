// Where each editor's movable panes are docked (kicad-port/auiDock.ts holds the rules): a small store of its own per editor, like the dock layout's
// (state/dockLayoutStore.ts), because it is the person's window arrangement -- KiCad keeps it with each frame's AUI layout (and Search's
// `aui.search_panel_*` settings) -- not the document. Remembered in the browser: the board editor's under `eda-studio.pcb-pane-dock.v1` (a layout of before the
// rows is read too, and a bottom-dock height kept before this layout existed is where the bottom dock starts), the schematic's under
// `eda-studio.sch-pane-dock.v1`.
import { useSyncExternalStore } from "react";
import {
  clampDockSize,
  clampPaneRect,
  dockBack,
  dockOnSide,
  defaultPaneDock,
  floatPane,
  parsePaneDock,
  resizeBetween,
  setRowDepth,
  type DockEditor,
  type MovablePaneId,
  type PaneDock,
  type PaneDockAt,
  type PaneRect,
  type PaneSide,
} from "../kicad-port/auiDock";
import { paneVisible } from "../kicad-port/dockLayout";
import { getDockLayout } from "./dockLayoutStore";

const STORAGE_KEY: Record<DockEditor, string> = { pcb: "eda-studio.pcb-pane-dock.v1", sch: "eda-studio.sch-pane-dock.v1" };
/** The board's bottom dock's height as BottomDock.tsx kept it before the panes could dock elsewhere. */
const LEGACY_HEIGHT_KEY = "eda-studio.bottom-dock-height.v1";

function storage(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

function load(editor: DockEditor): PaneDock {
  try {
    const st = storage();
    const legacy = editor === "pcb" ? Number(st?.getItem(LEGACY_HEIGHT_KEY)) : NaN;
    const bottom = Number.isFinite(legacy) && legacy >= 90 ? legacy : undefined;
    const raw = st?.getItem(STORAGE_KEY[editor]);
    return parsePaneDock(editor, raw ? JSON.parse(raw) : null, bottom);
  } catch {
    return defaultPaneDock(editor);
  }
}

const current: Record<DockEditor, PaneDock> = { pcb: load("pcb"), sch: load("sch") };
const listeners = new Set<() => void>();

function commit(editor: DockEditor, next: PaneDock): void {
  if (next === current[editor]) return;
  current[editor] = next;
  try {
    storage()?.setItem(STORAGE_KEY[editor], JSON.stringify(next));
  } catch {
    /* blocked site data: the arrangement still applies for this session */
  }
  for (const l of [...listeners]) l();
}

function windowSize(): { w: number; h: number } {
  return typeof window !== "undefined" ? { w: window.innerWidth, h: window.innerHeight } : { w: 1600, h: 1000 };
}

/** The panes shown in the dock layout (a hidden pane takes no place in its dock). */
export const paneShown = (id: MovablePaneId): boolean => paneVisible(getDockLayout(), id);

export function getPaneDock(editor: DockEditor): PaneDock {
  return current[editor];
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function usePaneDock(editor: DockEditor): PaneDock {
  return useSyncExternalStore(
    subscribe,
    () => current[editor],
    () => current[editor]
  );
}

/** A whole new layout (a drop `doDrop` worked out). */
export function setPaneDock(editor: DockEditor, next: PaneDock): void {
  commit(editor, next);
}

/** Pane `id` docked on a side (the caption's buttons) or floated (`from`: where its floating window opens, the pane's place on the page). */
export function movePaneTo(editor: DockEditor, id: MovablePaneId, to: PaneDockAt, from?: { x: number; y: number }): void {
  const { w, h } = windowSize();
  const dock = current[editor];
  if (to === "float") {
    const next = floatPane(dock, id, from);
    next.floating = { ...next.floating, [id]: clampPaneRect(id, next.floating[id]!, w, h) };
    commit(editor, next);
  } else if (dock.at[id] === "float" && dock.lastDocked[id] === to) commit(editor, dockBack(editor, dock, id));
  else commit(editor, dockOnSide(editor, dock, id, to, paneShown, { width: w, height: h }));
}

/** `OnLeftDClick` on a floating pane: `Dock()`, back where it was. */
export function dockPaneBack(editor: DockEditor, id: MovablePaneId): void {
  commit(editor, dockBack(editor, current[editor], id));
}

/** A dock's sash dragged: the dock's new depth, for its panes. */
export function setDockDepth(editor: DockEditor, side: PaneSide, size: number, panes: readonly MovablePaneId[]): void {
  const { w, h } = windowSize();
  commit(editor, setRowDepth(current[editor], panes, clampDockSize(side, size, panes, w, h)));
}

/** The sash between two panes of a dock dragged by `delta` px. */
export function resizePanesBetween(editor: DockEditor, side: PaneSide, a: MovablePaneId, b: MovablePaneId, sizeA: number, sizeB: number, delta: number): void {
  commit(editor, resizeBetween(current[editor], side, a, b, sizeA, sizeB, delta));
}

/** A floating pane moved by its caption or sized by its corner. */
export function setPaneFloatRect(editor: DockEditor, id: MovablePaneId, r: PaneRect): void {
  const { w, h } = windowSize();
  const rect = clampPaneRect(id, r, w, h);
  const was = current[editor].floating[id];
  if (was && was.x === rect.x && was.y === rect.y && was.w === rect.w && was.h === rect.h) return;
  commit(editor, { ...current[editor], floating: { ...current[editor].floating, [id]: rect } });
}

/** Test and "reset layout" hook: the editor's panes back under the canvas. */
export function resetPaneDock(editor: DockEditor): void {
  commit(editor, defaultPaneDock(editor));
}
