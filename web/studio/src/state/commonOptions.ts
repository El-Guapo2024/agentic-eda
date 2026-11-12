// Display and editing options of the editors. KiCad keeps the cursor and the grid display once per editor window (`WINDOW_SETTINGS`: `cursor.*`, `grid.style`,
// `grid.line_width`, `grid.min_spacing` of `APP_SETTINGS_BASE::addParamsForWindow`, in each editor's own settings file -- pcbnew, eeschema, footprint_editor,
// symbol_editor) and the polar coordinates in pcbnew's and the Footprint Editor's (`editing.polar_coords`); `window` holds them per editor here, and the canvases,
// the actions and the Display Options pages read the editor's own (`windowOptionsOf`). The rest is shared by the four canvases. Read with `useCommonOptions()`;
// changed with `setCommonOptions` / `setWindowOptions`. Kept in the browser's local storage, like `Preferences`.
//
//   common/tool/common_tools.cpp  ToggleCursor / CursorSmallCrosshairs / CursorFullCrosshairs / Cursor45Crosshairs /
//                                 ToggleBoundingBoxes
//   common/settings/app_settings.cpp  cursor.always_show_cursor (default true), cursor.cross_hair_mode (default small), grid.style (0 dots),
//                                 grid.line_width (1.0), grid.min_spacing (10)
//   common/dialogs/panel_gal_options.cpp  the Grid Display and Cursor sections of each editor's Display Options page
//   pcbnew/pcbnew_settings.cpp, footprint_editor_settings.cpp  editing.polar_coords (default false)
//   include/tool/selection_tool.h     SELECTION_MODE (the selection tool's rectangle / lasso mode)
import { useMemo, useSyncExternalStore } from "react";
import type { GridDisplay } from "../kicad-port/grid";
import { DEFAULT_WINDOW_OPTIONS, editorWindowOf, gridAxesEnabled, parseWindowOptions, type EditorWindow, type WindowOptions } from "../kicad-port/windowOptions";
import { DEFAULT_COMMON_SETTINGS, hiContrastFactorOf, sanitizeCommonSettings, type CommonSettings } from "../kicad-port/commonSettings";
import { setHiContrastFactor } from "../kicad-port/color4d";

export { DEFAULT_WINDOW_OPTIONS, EDITOR_WINDOWS, editorWindowOf, parseWindowOptions, type EditorWindow, type WindowOptions } from "../kicad-port/windowOptions";

export type SelectionAreaMode = "rect" | "lasso";

export interface CommonOptions {
  /** Each editor's own cursor, grid display and polar coordinates. */
  window: Record<EditorWindow, WindowOptions>;
  /** `COMMON_SETTINGS::m_DoNotShowAgain.zone_fill_warning`: "Don't show again" was clicked on the unfilled-zones warning (`PCB_CONTROL::unfilledZoneCheck`). */
  zoneFillWarningDismissed: boolean;
  /** Preferences > Common's settings the studio honours (kicad-port/commonSettings.ts). */
  common: CommonSettings;
}

export const DEFAULT_COMMON_OPTIONS: CommonOptions = {
  window: { pcb: DEFAULT_WINDOW_OPTIONS, schematic: DEFAULT_WINDOW_OPTIONS, footprint: DEFAULT_WINDOW_OPTIONS, symbol: DEFAULT_WINDOW_OPTIONS },
  zoneFillWarningDismissed: false,
  common: DEFAULT_COMMON_SETTINGS,
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
    const j = JSON.parse(raw) as Partial<Record<keyof CommonOptions | "crossHairMode" | "alwaysShowCursor", unknown>>;
    const d = DEFAULT_COMMON_OPTIONS;
    // a save from before the options were per editor: its one crosshair goes to every editor
    const legacy = parseWindowOptions({ crossHairMode: j.crossHairMode, alwaysShowCursor: j.alwaysShowCursor });
    const w = (j.window && typeof j.window === "object" ? j.window : {}) as Partial<Record<EditorWindow, unknown>>;
    return {
      window: { pcb: parseWindowOptions(w.pcb, legacy), schematic: parseWindowOptions(w.schematic, legacy), footprint: parseWindowOptions(w.footprint, legacy), symbol: parseWindowOptions(w.symbol, legacy) },
      zoneFillWarningDismissed: typeof j.zoneFillWarningDismissed === "boolean" ? j.zoneFillWarningDismissed : d.zoneFillWarningDismissed,
      common: sanitizeCommonSettings(j.common),
    };
  } catch {
    return DEFAULT_COMMON_OPTIONS;
  }
}

let current: CommonOptions = parseCommonOptions(storage()?.getItem(STORAGE_KEY) ?? null);
const listeners = new Set<() => void>();

/**
 * What Preferences > Common changes outside the React tree: the High Contrast factor the painters read (`PCB_RENDER_SETTINGS::LoadColors`), the toolbar icon
 * size (`--toolbar-icon-size`, styles/layout.css) and the tables' striping (`data-grid-striping`, styles/global.css) on the document.
 */
function applyCommonSettings(c: CommonSettings): void {
  setHiContrastFactor(hiContrastFactorOf(c));
  try {
    const root = typeof document !== "undefined" ? document.documentElement : null;
    if (!root) return;
    root.style.setProperty("--toolbar-icon-size", `${c.toolbarIconSize}px`);
    if (c.gridStriping) root.setAttribute("data-grid-striping", "1");
    else root.removeAttribute("data-grid-striping");
  } catch {
    /* no document (tests) */
  }
}
applyCommonSettings(current.common);

export function getCommonOptions(): CommonOptions {
  return current;
}

/** The window options of the editor on `tab`. */
export function windowOptionsOf(opts: CommonOptions, tab: string): WindowOptions {
  return opts.window[editorWindowOf(tab)];
}

/** Changes the window options of one editor (`COMMON_TOOLS`' cursor actions change the frame's own, `GetWindowSettings( cfg )`). */
export function setWindowOptions(tab: string, patch: Partial<WindowOptions>): void {
  const e = editorWindowOf(tab);
  const was = current.window[e];
  const next = { ...was, ...patch };
  if ((Object.keys(next) as (keyof WindowOptions)[]).every((k) => next[k] === was[k])) return;
  setCommonOptions({ window: { ...current.window, [e]: next } });
}

export function setCommonOptions(patch: Partial<CommonOptions>): void {
  const next = { ...current, ...patch };
  if ((Object.keys(next) as (keyof CommonOptions)[]).every((k) => next[k] === current[k])) return;
  current = next;
  if (patch.common) applyCommonSettings(current.common);
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

/** How the editor draws its grid (its Display Options > Grid Display, and its axes: `gridAxesEnabled`), re-rendering the caller when that changes. */
export function useGridDisplay(editor: EditorWindow): GridDisplay {
  const w = useCommonOptions().window[editor];
  const axes = gridAxesEnabled(editor, w);
  return useMemo(() => ({ style: w.gridStyle, lineWidthPx: w.gridLineWidth, minSpacingPx: w.gridMinSpacing, axes }), [w.gridStyle, w.gridLineWidth, w.gridMinSpacing, axes]);
}
