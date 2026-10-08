// Display and editing options every editor shares -- KiCad keeps each of these once per editor window
// (`WINDOW_SETTINGS`: `cursor.*`, `grid.*`, the render settings), but the four canvases here (board,
// schematic, footprint editor, symbol editor) read the same few switches, so they live in one small
// external store instead of in each editor's own reducer. Read with `useCommonOptions()`; changed with
// `setCommonOptions`. Kept in the browser's local storage, like `Preferences`.
//
//   common/tool/common_tools.cpp  ToggleCursor / CursorSmallCrosshairs / CursorFullCrosshairs / Cursor45Crosshairs /
//                                 ToggleBoundingBoxes
//   common/settings/app_settings.cpp  cursor.always_show_cursor (default true), cursor.cross_hair_mode (default small)
//   include/tool/selection_tool.h     SELECTION_MODE (the selection tool's rectangle / lasso mode)
import { useSyncExternalStore } from "react";
import type { CrossHairMode } from "../kicad-port/crosshair";

export type SelectionAreaMode = "rect" | "lasso";

export interface CommonOptions {
  /** `cursor.cross_hair_mode` -- default `SMALL_CROSS`. */
  crossHairMode: CrossHairMode;
  /** `cursor.always_show_cursor` -- default true. */
  alwaysShowCursor: boolean;
  /** `RENDER_SETTINGS::GetDrawBoundingBoxes` -- default off. */
  drawBoundingBoxes: boolean;
  /** `PCB_SELECTION_TOOL::m_selectionMode` / `SCH_SELECTION_TOOL`'s: which tool a drag on empty space starts. Default rectangle. */
  selectionMode: SelectionAreaMode;
}

export const DEFAULT_COMMON_OPTIONS: CommonOptions = {
  crossHairMode: "small",
  alwaysShowCursor: true,
  drawBoundingBoxes: false,
  selectionMode: "rect",
};

const STORAGE_KEY = "eda-studio.common-options";

function storage(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

/** Reads what a previous session saved; anything missing or of the wrong type keeps its default. */
export function parseCommonOptions(raw: string | null): CommonOptions {
  if (!raw) return DEFAULT_COMMON_OPTIONS;
  try {
    const j = JSON.parse(raw) as Partial<Record<keyof CommonOptions, unknown>>;
    const d = DEFAULT_COMMON_OPTIONS;
    return {
      crossHairMode: j.crossHairMode === "small" || j.crossHairMode === "full" || j.crossHairMode === "diag45" ? j.crossHairMode : d.crossHairMode,
      alwaysShowCursor: typeof j.alwaysShowCursor === "boolean" ? j.alwaysShowCursor : d.alwaysShowCursor,
      drawBoundingBoxes: typeof j.drawBoundingBoxes === "boolean" ? j.drawBoundingBoxes : d.drawBoundingBoxes,
      selectionMode: j.selectionMode === "rect" || j.selectionMode === "lasso" ? j.selectionMode : d.selectionMode,
    };
  } catch {
    return DEFAULT_COMMON_OPTIONS;
  }
}

let current: CommonOptions = parseCommonOptions(storage()?.getItem(STORAGE_KEY) ?? null);
const listeners = new Set<() => void>();

export function getCommonOptions(): CommonOptions {
  return current;
}

export function setCommonOptions(patch: Partial<CommonOptions>): void {
  const next = { ...current, ...patch };
  if ((Object.keys(next) as (keyof CommonOptions)[]).every((k) => next[k] === current[k])) return;
  current = next;
  try {
    storage()?.setItem(STORAGE_KEY, JSON.stringify(current));
  } catch {
    /* private window or blocked site data: the options still apply for this session */
  }
  for (const l of listeners) l();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The shared options, re-rendering the caller whenever one changes. */
export function useCommonOptions(): CommonOptions {
  return useSyncExternalStore(subscribe, getCommonOptions, getCommonOptions);
}
