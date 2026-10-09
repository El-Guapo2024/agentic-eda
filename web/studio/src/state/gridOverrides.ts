// The grid overrides of each editor (`GRID_SETTINGS::overrides_enabled`, `override_connected` ... `override_graphics_idx`, which KiCad keeps in each editor's
// settings file next to the grid list): the switch Ctrl+Shift+G / the toolbar button flips and the five category grids the Grids page edits. Plain module state
// with subscribers, kept in the browser's local storage like `state/gridSettings.ts` -- these are the user's preferences, not the design's. The rules for them are
// kicad-port/gridOverrides.ts; the snapping that reads them is components/canvas/pcbSnap.ts (board) and components/schematic/schSnap.ts (schematic).
import { useSyncExternalStore } from "react";
import { GRID_EDITORS, SCHEMATIC_FAMILY, type GridEditor } from "../kicad-port/gridSettings";
import { defaultGridOverrides, parseGridOverrides, sameOverrides, type GridOverrides, type OverrideFamily } from "../kicad-port/gridOverrides";
import { getGridSettings } from "./gridSettings";

const STORAGE_KEY = "eda-studio.grid-overrides";

function storage(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

/** Which defaults an editor's overrides start from. */
export const overrideFamilyOf = (editor: GridEditor): OverrideFamily => (SCHEMATIC_FAMILY.includes(editor) ? "schematic" : "pcb");

function parseStored(text: string | null): Record<GridEditor, GridOverrides> {
  let saved: Record<string, unknown> | null = null;
  try {
    saved = text ? (JSON.parse(text) as Record<string, unknown>) : null;
  } catch {
    saved = null;
  }
  const read = (e: GridEditor) => parseGridOverrides(saved?.[e], overrideFamilyOf(e), getGridSettings(e).grids.length);
  return { pcb: read("pcb"), footprint: read("footprint"), symbol: read("symbol"), schematic: read("schematic") };
}

let current: Record<GridEditor, GridOverrides> = parseStored(storage()?.getItem(STORAGE_KEY) ?? null);
const listeners = new Set<() => void>();

export function getGridOverrides(editor: GridEditor): GridOverrides {
  return current[editor];
}

/** Saves one editor's overrides; the defaults are not kept (so a later change of them reaches the editor). */
export function setGridOverrides(editor: GridEditor, overrides: GridOverrides): void {
  if (sameOverrides(overrides, current[editor])) return;
  current = { ...current, [editor]: overrides };
  try {
    const kept: Partial<Record<GridEditor, GridOverrides>> = {};
    for (const e of GRID_EDITORS) if (!sameOverrides(current[e], defaultGridOverrides(overrideFamilyOf(e)))) kept[e] = current[e];
    storage()?.setItem(STORAGE_KEY, JSON.stringify(kept));
  } catch {
    /* private window or blocked site data: the overrides still apply for this session */
  }
  for (const l of listeners) l();
}

/** `COMMON_TOOLS::ToggleGridOverrides`: `SetGridOverrides( !IsGridOverridden() )`. */
export function toggleGridOverrides(editor: GridEditor): boolean {
  const next = { ...current[editor], enabled: !current[editor].enabled };
  setGridOverrides(editor, next);
  return next.enabled;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** One editor's overrides, re-rendering the caller whenever they change. */
export function useGridOverrides(editor: GridEditor): GridOverrides {
  return useSyncExternalStore(subscribe, () => current[editor], () => current[editor]);
}
