// The person's colour themes and the board editor's choice of one (kicad-port/colorTheme.ts holds the rules), read by `layerColor`
// (components/canvas/layers.ts) for every colour of the board's namespace, so the painter draws the board in the chosen theme.
//
// Where they are kept: in the browser's local storage, as the studio keeps the person's other preferences (`state/commonOptions.ts`,
// the Preferences dialog's `prefs`, the dock layout). KiCad keeps a theme in the user's configuration folder (`colors/<name>.json`,
// `SETTINGS_MANAGER::GetColorSettingsPath`) and the editor's choice in `pcbnew.json` (`m_ColorTheme`) -- per user, never in a
// project. So a theme is not `appearance.json` (which is the project's `.kicad_prl` / `.kicad_pro` part: layers shown, opacities, net
// and net class colours) and not the design: it does not enter undo, does not reload the scene, and follows the person from board to
// board.
//
// `preview` is the Colors page's working copy while Preferences is open (KiCad draws it in the page's preview board,
// `PANEL_PCBNEW_COLOR_SETTINGS::updatePreview`); the studio has no preview board and draws it on the board itself, until OK or Cancel.
import { useSyncExternalStore } from "react";
import colorsData from "../kicad/colors.json";
import themesData from "../kicad/color_themes.json";
import { findTheme, parseStoredThemes, serializeStoredThemes, setThemeColors, themeRegistry, type ColorTheme, type StoredThemes, type ThemeData } from "../kicad-port/colorTheme";

const STORAGE_KEY = "eda-studio.color-themes.v1";

export const THEME_DATA: ThemeData = {
  defaults: (colorsData as { colors: Record<string, string> }).colors,
  classic: (themesData as { classic: Record<string, string> }).classic,
  boardKeys: (themesData as { boardParams: Array<{ layer: string }> }).boardParams.map((p) => p.layer),
};

/** The layers whose colour the board editor's theme decides (the board namespace, and the page's Selection highlight, which KiCad lists without a param). */
const BOARD_KEYS = new Set<string>([...THEME_DATA.boardKeys, "LAYER_SELECT_OVERLAY"]);

function storage(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

interface Snapshot {
  stored: StoredThemes;
  registry: ColorTheme[];
  /** The board editor's theme (`GetColorSettings( m_ColorTheme )`). */
  pcb: ColorTheme;
  /** The Colors page's working copy while Preferences is open, drawn in place of `pcb`. */
  preview: ColorTheme | null;
  version: number;
}

function load(): StoredThemes {
  try {
    return parseStoredThemes(storage()?.getItem(STORAGE_KEY) ?? null);
  } catch {
    return parseStoredThemes(null);
  }
}

function make(stored: StoredThemes, preview: ColorTheme | null, version: number): Snapshot {
  const registry = themeRegistry(THEME_DATA, stored);
  return { stored, registry, pcb: findTheme(registry, stored.pcbTheme), preview, version };
}

let current: Snapshot = make(load(), null, 0);
const listeners = new Set<() => void>();

function commit(stored: StoredThemes, preview: ColorTheme | null = current.preview): void {
  const changedStore = stored !== current.stored;
  current = make(stored, preview, current.version + 1);
  if (changedStore) {
    try {
      storage()?.setItem(STORAGE_KEY, serializeStoredThemes(stored));
    } catch {
      /* private window or quota: the themes still apply for this session */
    }
  }
  for (const l of [...listeners]) l();
}

export function getColorThemes(): Snapshot {
  return current;
}

function subscribe(l: () => void): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

/** The themes, re-rendering the caller when they change. */
export function useColorThemes(): Snapshot {
  return useSyncExternalStore(subscribe, getColorThemes, getColorThemes);
}

/** A number that changes whenever a colour the board is drawn in may have changed (for a canvas's redraw dependencies). */
export function useColorThemeStamp(): number {
  return useColorThemes().version;
}

/**
 * The board editor's colour of a layer (colors.json's id), or undefined when the theme does not decide it (a schematic or 3D colour, an id
 * KiCad's board namespace does not have): `layerColor` then gives KiCad's default.
 */
export function pcbThemeColor(key: string): string | undefined {
  if (!BOARD_KEYS.has(key)) return undefined;
  const t = current.preview ?? current.pcb;
  return t.colors[key];
}

/** The colours of an editable theme (`SetColor` + `SaveColorSettings`); a read-only one is left alone. */
export function setThemeColor(filename: string, colors: Record<string, string>): void {
  const next = setThemeColors(current.stored, current.registry, filename, colors);
  if (next !== current.stored) commit(next);
}

/** Replaces the whole store (New Theme..., OK of the Colors page). */
export function replaceStoredThemes(stored: StoredThemes): void {
  commit(stored);
}

/** `PCBNEW_SETTINGS::m_ColorTheme` (the page's `TransferDataFromWindow`). */
export function selectPcbTheme(filename: string): void {
  if (current.stored.pcbTheme === filename) return;
  commit({ ...current.stored, pcbTheme: filename });
}

/** Draws the board in `theme` while the Colors page is up (null: back to the editor's theme). */
export function setThemePreview(theme: ColorTheme | null): void {
  if (theme === current.preview) return;
  commit(current.stored, theme);
}

/** For the tests and the scripted hook: forget every theme of the person's and go back to the default theme. */
export function resetColorThemesForTest(): void {
  commit(parseStoredThemes(null), null);
}

/** What the scripted hook reports (`__eda.state().colorTheme`). */
export function colorThemeSummary(): { pcbTheme: string; name: string; readOnly: boolean; themes: string[]; preview: boolean } {
  return { pcbTheme: current.pcb.filename, name: current.pcb.name, readOnly: current.pcb.readOnly, themes: current.registry.map((t) => t.filename), preview: current.preview !== null };
}
