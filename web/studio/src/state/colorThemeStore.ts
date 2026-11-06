// The person's colour themes and each editor's choice of one (kicad-port/colorTheme.ts holds the rules), read by `layerColor`
// (components/canvas/layers.ts) for every colour of the board's and the schematic's namespaces, so each painter draws in its editor's theme:
// the board pcbnew's (`PCBNEW_SETTINGS::m_ColorTheme`), the Footprint Editor its own (`FOOTPRINT_EDIT_FRAME::GetColorSettings`), the schematic
// eeschema's (`SCH_BASE_FRAME::GetColorSettings`), the Symbol Editor its own or the schematic editor's (`SYMBOL_EDIT_FRAME::GetColorSettings`,
// `m_UseEeschemaColorSettings`).
//
// Which editor is drawing: a canvas paints inside `paintingIn( editor, .. )` (the schematic, the Symbol Editor and the Footprint Editor do; the
// board needs not, it is the default). Outside one, a colour of the board namespace is pcbnew's theme's and a colour of the schematic namespace
// KiCad's default -- so the board's drawing of a schematic layer (its selection, `LAYER_SELECTION_SHADOWS`) never takes the schematic's theme, and
// the schematic's never the board's.
//
// Where they are kept: in the browser's local storage, as the studio keeps the person's other preferences (`state/commonOptions.ts`,
// the Preferences dialog's `prefs`, the dock layout). KiCad keeps a theme in the user's configuration folder (`colors/<name>.json`,
// `SETTINGS_MANAGER::GetColorSettingsPath`) and each editor's choice in its settings file (`pcbnew.json`, `eeschema.json`, `fpedit.json`,
// `symbol_editor.json`: `appearance.color_theme`) -- per user, never in a project. So a theme is not `appearance.json` (which is the project's
// `.kicad_prl` / `.kicad_pro` part: layers shown, opacities, net and net class colours) and not the design: it does not enter undo, does not
// reload the scene, and follows the person from board to board. A theme file comes in or goes out through the Colors pages (Import Theme...,
// Export Theme...; kicad-port/colorThemeFile.ts).
//
// `preview` is a Colors page's working copy while Preferences is open (KiCad draws it in the page's preview board or sheet,
// `PANEL_PCBNEW_COLOR_SETTINGS::updatePreview`, `PANEL_EESCHEMA_COLOR_SETTINGS::updatePreview`); the studio has no preview and draws it on the
// editor's canvas itself, until OK or Cancel.
import { useSyncExternalStore } from "react";
import colorsData from "../kicad/colors.json";
import themesData from "../kicad/color_themes.json";
import {
  THEME_EDITORS,
  editorNamespace,
  editorThemeName,
  findTheme,
  parseStoredThemes,
  serializeStoredThemes,
  setThemeColors,
  themeRegistry,
  withEditorTheme,
  withSymbolTheme,
  type ColorTheme,
  type StoredThemes,
  type ThemeData,
  type ThemeEditor,
} from "../kicad-port/colorTheme";
import type { ThemeParam } from "../kicad-port/colorThemeFile";

const STORAGE_KEY = "eda-studio.color-themes.v1";

const themesJson = themesData as { classic: Record<string, string>; boardParams: ThemeParam[]; schematicParams: ThemeParam[] };

export const THEME_DATA: ThemeData = {
  defaults: (colorsData as { colors: Record<string, string> }).colors,
  classic: themesJson.classic,
  boardKeys: themesJson.boardParams.map((p) => p.layer),
  schematicKeys: themesJson.schematicParams.map((p) => p.layer),
};

/** The `COLOR_MAP_PARAM`s a theme file holds that the studio reads and writes: the board's and the schematic's. */
export const THEME_PARAMS: readonly ThemeParam[] = [...themesJson.boardParams, ...themesJson.schematicParams];

/** The layers whose colour the PCB and Footprint Editor themes decide (the board namespace, and the PCB page's Selection highlight, which KiCad lists without a param). */
const BOARD_KEYS = new Set<string>([...THEME_DATA.boardKeys, "LAYER_SELECT_OVERLAY"]);
/** The layers whose colour the schematic and Symbol Editor themes decide (the schematic namespace, and the two rows of the schematic page without a param). */
const SCHEMATIC_KEYS = new Set<string>([...(THEME_DATA.schematicKeys ?? []), "LAYER_INTERSHEET_REFS", "LAYER_SHAPES_BACKGROUND"]);

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
  /** Each editor's theme. */
  themes: Record<ThemeEditor, ColorTheme>;
  /** The board's Colors page's working copy while Preferences is open (`previews.pcb`). */
  preview: ColorTheme | null;
  /** Each Colors page's working copy of its editor's theme while Preferences is open, drawn in place of it. */
  previews: Partial<Record<ThemeEditor, ColorTheme>>;
  version: number;
}

function load(): StoredThemes {
  try {
    return parseStoredThemes(storage()?.getItem(STORAGE_KEY) ?? null);
  } catch {
    return parseStoredThemes(null);
  }
}

function make(stored: StoredThemes, previews: Partial<Record<ThemeEditor, ColorTheme>>, version: number): Snapshot {
  const registry = themeRegistry(THEME_DATA, stored);
  const themes = Object.fromEntries(THEME_EDITORS.map((e) => [e, findTheme(registry, editorThemeName(stored, e))])) as Record<ThemeEditor, ColorTheme>;
  return { stored, registry, pcb: themes.pcb, themes, preview: previews.pcb ?? null, previews, version };
}

let current: Snapshot = make(load(), {}, 0);
const listeners = new Set<() => void>();

function commit(stored: StoredThemes, previews: Partial<Record<ThemeEditor, ColorTheme>> = current.previews): void {
  const changedStore = stored !== current.stored;
  current = make(stored, previews, current.version + 1);
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

/** A number that changes whenever a colour an editor is drawn in may have changed (for a canvas's redraw dependencies). */
export function useColorThemeStamp(): number {
  return useColorThemes().version;
}

/** The editor whose canvas is being painted (`paintingIn`), or null: the board's colours are then pcbnew's, the schematic's KiCad's defaults. */
let painting: ThemeEditor | null = null;

/** Runs `fn` as the painting of `editor`'s canvas: `layerColor` gives that editor's theme's colours. */
export function paintingIn<T>(editor: ThemeEditor, fn: () => T): T {
  const was = painting;
  painting = editor;
  try {
    return fn();
  } finally {
    painting = was;
  }
}

/** The theme `editor` draws in now: the Colors page's working copy while it is shown, else the editor's theme. */
export function shownTheme(editor: ThemeEditor): ColorTheme {
  return current.previews[editor] ?? current.themes[editor];
}

/**
 * The colour of a layer (colors.json's id) in the theme of the editor being painted, or undefined when its theme does not decide it (a 3D or Gerber
 * colour, a layer of the other namespace, a colour the theme lacks): `layerColor` then gives KiCad's default.
 */
export function editorThemeColor(key: string): string | undefined {
  const ns = BOARD_KEYS.has(key) ? "board" : SCHEMATIC_KEYS.has(key) ? "schematic" : null;
  if (!ns) return undefined;
  const editor: ThemeEditor | null = painting ? (editorNamespace(painting) === ns ? painting : null) : ns === "board" ? "pcb" : null;
  if (!editor) return undefined;
  return shownTheme(editor).colors[key];
}

/** The board editor's colour of a layer, or undefined when its theme does not decide it. */
export function pcbThemeColor(key: string): string | undefined {
  if (!BOARD_KEYS.has(key)) return undefined;
  return shownTheme("pcb").colors[key];
}

/**
 * `SCH_RENDER_SETTINGS::m_OverrideItemColors` of the schematic being painted (the schematic editor's theme, or the Symbol Editor's): every item in its
 * layer's colour, never its own (`SCH_PAINTER::getRenderColor`).
 */
export function overrideSchItemColors(): boolean {
  const editor: ThemeEditor = painting === "symbol" ? "symbol" : "schematic";
  return shownTheme(editor).overrideSchItemColors ?? false;
}

/** The colours of an editable theme (`SetColor` + `SaveColorSettings`), and its override flag when given; a read-only one is left alone. */
export function setThemeColor(filename: string, colors: Record<string, string>, overrideSchItemColors?: boolean): void {
  const next = setThemeColors(current.stored, current.registry, filename, colors, overrideSchItemColors);
  if (next !== current.stored) commit(next);
}

/** Replaces the whole store (New Theme..., Import Theme...). */
export function replaceStoredThemes(stored: StoredThemes): void {
  commit(stored);
}

/** `PCBNEW_SETTINGS::m_ColorTheme` (the page's `TransferDataFromWindow`). */
export function selectPcbTheme(filename: string): void {
  selectEditorTheme("pcb", filename);
}

/** An editor's `m_ColorTheme` (its Colors page's `TransferDataFromWindow`). */
export function selectEditorTheme(editor: ThemeEditor, filename: string): void {
  const next = withEditorTheme(current.stored, editor, filename);
  if (next !== current.stored) commit(next);
}

/** The Symbol Editor's "Use schematic editor color theme" / "Use theme:" (`PANEL_SYM_COLOR_SETTINGS::TransferDataFromWindow`). */
export function selectSymbolTheme(useSchematicTheme: boolean, filename: string): void {
  commit(withSymbolTheme(current.stored, useSchematicTheme, filename));
}

/** Draws the board in `theme` while the PCB Colors page is up (null: back to the editor's theme). */
export function setThemePreview(theme: ColorTheme | null): void {
  setEditorThemePreview("pcb", theme);
}

/** Draws `editor`'s canvas in `theme` while its Colors page is up (null: back to the editor's theme). */
export function setEditorThemePreview(editor: ThemeEditor, theme: ColorTheme | null): void {
  if ((current.previews[editor] ?? null) === theme) return;
  const previews = { ...current.previews };
  if (theme) previews[editor] = theme;
  else delete previews[editor];
  commit(current.stored, previews);
}

/** For the tests and the scripted hook: forget every theme of the person's and go back to the default theme. */
export function resetColorThemesForTest(): void {
  commit(parseStoredThemes(null), {});
}

/** What the scripted hook reports (`__eda.state().colorTheme`): the board's theme as before, and each editor's. */
export function colorThemeSummary(): {
  pcbTheme: string;
  name: string;
  readOnly: boolean;
  themes: string[];
  preview: boolean;
  editors: Record<ThemeEditor, string>;
  symUseSchTheme: boolean;
  previews: ThemeEditor[];
} {
  return {
    pcbTheme: current.pcb.filename,
    name: current.pcb.name,
    readOnly: current.pcb.readOnly,
    themes: current.registry.map((t) => t.filename),
    preview: current.preview !== null,
    editors: Object.fromEntries(THEME_EDITORS.map((e) => [e, current.themes[e].filename])) as Record<ThemeEditor, string>,
    symUseSchTheme: current.stored.symUseSchTheme !== false,
    previews: THEME_EDITORS.filter((e) => current.previews[e]),
  };
}
