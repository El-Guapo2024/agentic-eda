// The board editor's colour themes (KiCad 8303b2ad): the theme registry of `SETTINGS_MANAGER` (common/settings/settings_manager.cpp), the built-in
// themes of `COLOR_SETTINGS::CreateBuiltinColorSettings` (common/settings/color_settings.cpp), and what Preferences > PCB Editor > Colors does with
// them (`PANEL_COLOR_SETTINGS`, common/dialogs/panel_color_settings.cpp; `PANEL_PCBNEW_COLOR_SETTINGS`, pcbnew/dialogs/panel_pcbnew_color_settings.cpp).
//
//   * The themes: "KiCad Default" (`_builtin_default`, the colours of `s_defaultTheme`) and "KiCad Classic" (`_builtin_classic`, `s_classicTheme`),
//     both read-only (`m_writeFile = false`), and the person's own (KiCad: one `colors/<name>.json` each in the user's configuration folder). The
//     editor's `m_ColorTheme` defaults to `DEFAULT_THEME` ("user"), and `SETTINGS_MANAGER::GetColorSettings( "user" )` without a `user.json`
//     makes that theme on the spot as an editable copy of the default one -- so a first run edits "KiCad Default" (the copy) and lists the
//     built-in "KiCad Default (read-only)" beside it.
//   * A theme holds the board namespace of a theme file (`SaveColorSettings( cs, "board" )`, the `CLR( "board.*", .. )` params): the copper,
//     technical and user layers and the board's GAL layers (background, grid, ratsnest, DRC markers, net names, ...). Colours are `#rrggbbaa`
//     under the layer ids src/kicad/colors.json uses (`F_Cu`, `LAYER_PCB_BACKGROUND`).
//   * The Colors page: the theme list (`createThemeList`: the themes by name, "(read-only)" after a built-in one, then "---" and "New Theme..."),
//     a swatch per layer in KiCad's order (`createSwatches`), a swatch's menu (`ShowColorContextMenu`: Copy color, Paste color, Revert to saved
//     color), New Theme... (`OnThemeChanged`: "New theme name:", "Add Color Theme", "Theme already exists!"; the new theme starts with the
//     current theme's colours), and Reset to Defaults (`ResetPanel`: every colour back to KiCad's default, nothing on a read-only theme).
//
// Pure: no store, no DOM, no JSON import (the data comes in as `ThemeData`; state/colorThemeStore.ts reads the JSON files). Unit-tested in colorTheme.test.ts.

/** The filenames of the built-in themes (`COLOR_SETTINGS::COLOR_BUILTIN_DEFAULT`, `COLOR_BUILTIN_CLASSIC`) and of the editor's default theme (`DEFAULT_THEME`). */
export const BUILTIN_DEFAULT = "_builtin_default";
export const BUILTIN_CLASSIC = "_builtin_classic";
export const DEFAULT_THEME = "user";

/** What the themes are made of: KiCad's default colours (colors.json, `s_defaultTheme`), the Classic theme's (color_themes.json) and the board namespace's layers. */
export interface ThemeData {
  /** `s_defaultTheme`: layer id -> `#rrggbbaa` (every layer of colors.json; only `boardKeys` are a PCB theme's). */
  defaults: Readonly<Record<string, string>>;
  /** `s_classicTheme`'s board layers. */
  classic: Readonly<Record<string, string>>;
  /** The layers of the board namespace (`CLR( "board.*", .. )`), the colours a PCB theme holds. */
  boardKeys: readonly string[];
}

/** One theme of the registry (`COLOR_SETTINGS`). `colors` may leave layers out: those are KiCad's default colour (see `themeColor`). */
export interface ColorTheme {
  /** `GetFilename()`: the file's name without `.json` (a built-in's `_builtin_*`). */
  filename: string;
  /** `GetName()`: `meta.name`, what the list shows. */
  name: string;
  readOnly: boolean;
  colors: Record<string, string>;
}

/** What the person's store keeps: their own themes (KiCad's `colors/*.json`) and the board editor's theme (`PCBNEW_SETTINGS::m_ColorTheme`). */
export interface StoredThemes {
  pcbTheme: string;
  themes: Array<{ filename: string; name: string; colors: Record<string, string> }>;
}

export const EMPTY_STORED_THEMES: StoredThemes = { pcbTheme: DEFAULT_THEME, themes: [] };

/** `CreateBuiltinColorSettings`: "KiCad Default" from the params' defaults, "KiCad Classic" from `s_classicTheme`; neither is written (read-only). */
export function builtinThemes(data: ThemeData): ColorTheme[] {
  const def: Record<string, string> = {};
  for (const k of data.boardKeys) if (data.defaults[k]) def[k] = data.defaults[k]!;
  return [
    { filename: BUILTIN_DEFAULT, name: "KiCad Default", readOnly: true, colors: def },
    { filename: BUILTIN_CLASSIC, name: "KiCad Classic", readOnly: true, colors: { ...data.classic } },
  ];
}

/**
 * Every theme the list offers (`m_color_settings`): the built-ins, the person's, and the editor's default "user" theme, which
 * `GetColorSettings( DEFAULT_THEME )` makes as an editable copy of "KiCad Default" (its name too) while no `user.json` exists.
 */
export function themeRegistry(data: ThemeData, stored: StoredThemes): ColorTheme[] {
  const out = builtinThemes(data);
  for (const t of stored.themes) {
    if (t.filename === BUILTIN_DEFAULT || t.filename === BUILTIN_CLASSIC) continue;
    out.push({ filename: t.filename, name: t.name, readOnly: false, colors: { ...t.colors } });
  }
  if (!out.some((t) => t.filename === DEFAULT_THEME)) out.push({ filename: DEFAULT_THEME, name: "KiCad Default", readOnly: false, colors: {} });
  return out;
}

/**
 * `SETTINGS_MANAGER::GetColorSettings( aName )`: the theme of that filename, else of that display name (any case: "Maybe it's the display name"),
 * else the "user" theme (a theme that cannot be loaded becomes a copy of the default saved as `DEFAULT_THEME`; `PANEL_PCBNEW_COLOR_SETTINGS`'
 * "Saved theme doesn't exist?  Reset to default").
 */
export function findTheme(registry: readonly ColorTheme[], name: string): ColorTheme {
  return (
    registry.find((t) => t.filename === name) ??
    registry.find((t) => t.name.toLowerCase() === name.toLowerCase()) ??
    registry.find((t) => t.filename === DEFAULT_THEME) ??
    registry[0]!
  );
}

/**
 * `GetColorSettingsList`: the themes sorted by name (`a->GetName() < b->GetName()`, a case-sensitive compare). Themes of one name keep the
 * registry's filename order (a `std::map`: `_builtin_classic`, `_builtin_default`, `user`, ...); KiCad's `std::sort` does not promise an order
 * for them.
 */
export function sortedThemes(registry: readonly ColorTheme[]): ColorTheme[] {
  const byFile = [...registry].sort((a, b) => (a.filename < b.filename ? -1 : a.filename > b.filename ? 1 : 0));
  return byFile.map((t, i) => ({ t, i })).sort((a, b) => (a.t.name < b.t.name ? -1 : a.t.name > b.t.name ? 1 : a.i - b.i)).map((x) => x.t);
}

/** `GetSettingsDropdownName`: the name, and " (read-only)" after a theme that cannot be edited. */
export function dropdownName(theme: ColorTheme): string {
  return theme.readOnly ? `${theme.name} (read-only)` : theme.name;
}

/** The entries of the theme list (`createThemeList`): the themes, then the "---" separator and "New Theme...". */
export type ThemeListEntry = { kind: "theme"; filename: string; label: string } | { kind: "separator"; label: "---" } | { kind: "new"; label: "New Theme..." };

export function themeListEntries(registry: readonly ColorTheme[]): ThemeListEntry[] {
  return [...sortedThemes(registry).map((t): ThemeListEntry => ({ kind: "theme", filename: t.filename, label: dropdownName(t) })), { kind: "separator", label: "---" }, { kind: "new", label: "New Theme..." }];
}

/** `COLOR_SETTINGS::GetColor`, with KiCad's default colour where the theme has none (KiCad: `COLOR4D::UNSPECIFIED`, which no built-in board layer is). */
export function themeColor(theme: ColorTheme, key: string, data: ThemeData): string | undefined {
  return theme.colors[key] ?? data.defaults[key];
}

/** `COLOR_SETTINGS::GetDefaultColor`: the param's default, which is `s_defaultTheme`'s colour for every board layer. */
export function defaultColor(key: string, data: ThemeData): string | undefined {
  return data.defaults[key];
}

/** The characters `FOOTPRINT_NAME_VALIDATOR` will not let the "New theme name:" field hold (`wxFILTER_EXCLUDE_CHAR_LIST`: they cannot be typed). */
export const THEME_NAME_EXCLUDED = "%$<>\t\n\r\"\\/:";

/** The text with the excluded characters dropped, as the validated field keeps it. */
export function sanitizeThemeName(text: string): string {
  return [...text].filter((c) => !THEME_NAME_EXCLUDED.includes(c)).join("");
}

/**
 * New Theme... (`OnThemeChanged`): the theme `<name>.json` in the themes folder, named `name`, editable, with the current theme's colour for every
 * layer of the page (`for( int layer : m_validLayers ) newSettings->SetColor( layer, m_currentSettings->GetColor( layer ) )`). The message when
 * `fn.Exists()` is "Theme already exists!"; a name is taken when a theme of the person's has that filename, or it is the default "user" theme's.
 */
export function addTheme(stored: StoredThemes, name: string, current: ColorTheme, pageKeys: readonly string[], data: ThemeData): { stored: StoredThemes; error?: string } {
  if (stored.themes.some((t) => t.filename === name) || name === DEFAULT_THEME) return { stored, error: "Theme already exists!" };
  const colors: Record<string, string> = {};
  for (const k of pageKeys) {
    const c = themeColor(current, k, data);
    if (c) colors[k] = c;
  }
  return { stored: { ...stored, themes: [...stored.themes, { filename: name, name, colors }] } };
}

/**
 * A colour of an editable theme (`updateColor`, `OnColorSwatchChanged`: `SetColor` then `SaveColorSettings`). A read-only theme is not changed.
 * The "user" theme is written the first time it changes (its `user.json`).
 */
export function setThemeColors(stored: StoredThemes, registry: readonly ColorTheme[], filename: string, colors: Readonly<Record<string, string>>): StoredThemes {
  const theme = registry.find((t) => t.filename === filename);
  if (!theme || theme.readOnly) return stored;
  const have = stored.themes.find((t) => t.filename === filename);
  if (!have) return { ...stored, themes: [...stored.themes, { filename, name: theme.name, colors: { ...theme.colors, ...colors } }] };
  return { ...stored, themes: stored.themes.map((t) => (t.filename === filename ? { ...t, colors: { ...t.colors, ...colors } } : t)) };
}

/** `PANEL_COLOR_SETTINGS::ResetPanel`: every colour of the page back to `GetDefaultColor`; a read-only theme is left as it is. */
export function resetThemeColors(theme: ColorTheme, pageKeys: readonly string[], data: ThemeData): ColorTheme {
  if (theme.readOnly) return theme;
  const colors = { ...theme.colors };
  for (const k of pageKeys) {
    const d = defaultColor(k, data);
    if (d) colors[k] = d;
  }
  return { ...theme, colors };
}

/** `ShowColorContextMenu`: Copy color always; Paste color once a colour was copied and the theme can be edited; Revert to saved color when the page's colour is not the saved one. */
export function swatchMenu(readOnly: boolean, copied: string | null, current: string | undefined, saved: string | undefined): Array<"copy" | "paste" | "revert"> {
  const out: Array<"copy" | "paste" | "revert"> = ["copy"];
  if (!readOnly && copied !== null) out.push("paste");
  if (!readOnly && current !== saved) out.push("revert");
  return out;
}

export const SWATCH_MENU_LABELS: Record<"copy" | "paste" | "revert", string> = { copy: "Copy color", paste: "Paste color", revert: "Revert to saved color" };

/**
 * The board editor's GAL layers on the page (`m_validLayers`): `GAL_LAYER_ID_START` .. `GAL_LAYER_ID_BITMASK_END` but `g_excludedLayers`, then the
 * layers the constructor adds, each with `LayerName()`'s text. They are listed sorted by that text after the board layers (`createSwatches`).
 */
export const PCB_GAL_COLOR_ROWS: ReadonlyArray<{ key: string; label: string }> = [
  // GAL_LAYER_ID_START + 5 .. + 29, without the excluded ones (vias, footprints, values, references, tracks, footprint text, the deprecated slots,
  // plated holes, the general-purpose overlay and images)
  { key: "LAYER_NON_PLATEDHOLES", label: "Non-plated holes" },
  { key: "LAYER_ANCHOR", label: "Anchors" },
  { key: "LAYER_RATSNEST", label: "Ratsnest" },
  { key: "LAYER_GRID", label: "Grid" },
  { key: "LAYER_GRID_AXES", label: "Grid axes" },
  { key: "LAYER_VIA_HOLES", label: "Via holes" },
  { key: "LAYER_DRC_ERROR", label: "DRC errors" },
  { key: "LAYER_DRAWINGSHEET", label: "Drawing sheet" },
  { key: "LAYER_SELECT_OVERLAY", label: "Selection highlight" },
  { key: "LAYER_PCB_BACKGROUND", label: "Background" },
  { key: "LAYER_CURSOR", label: "Cursor" },
  { key: "LAYER_AUX_ITEMS", label: "Helper items" },
  // "These layers are not in GAL_LAYER_ID_START ... GAL_LAYER_ID_BITMASK_END"
  { key: "LAYER_LOCKED_ITEM_SHADOW", label: "Locked item shadow" },
  { key: "LAYER_CONFLICTS_SHADOW", label: "Courtyard collision shadow" },
  { key: "LAYER_BOARD_OUTLINE_AREA", label: "Board outline area" },
  { key: "LAYER_PAGE_LIMITS", label: "Page limits" },
  { key: "LAYER_DRC_WARNING", label: "DRC warnings" },
  { key: "LAYER_DRC_EXCLUSION", label: "DRC exclusions" },
  { key: "LAYER_DRC_HIGHLIGHTED", label: "DRC highlighted" },
  { key: "NETNAMES_LAYER_ID_START", label: "Track net names" },
  { key: "LAYER_PAD_NETNAMES", label: "Pad net names" },
];

/** `LSET::TechAndUserUIOrder` of `LSET::AllNonCuMask()`: the 18 technical layers, then User.1 .. User.45. Board file names. */
const TECH_LAYERS = ["F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", "Edge.Cuts", "Margin", "F.CrtYd", "B.CrtYd", "F.Fab", "B.Fab"];

/** A board layer's colour key: the file name with `_` for `.` (`F.SilkS` -> `F_SilkS`, colors.json's ids). */
export function boardLayerColorKey(file: string): string {
  return file.replace(/\./g, "_");
}

/**
 * The page's rows in KiCad's order (`createSwatches`): the copper layers F.Cu, In1.Cu .. In30.Cu, B.Cu (`LAYER_RANGE( F_Cu, B_Cu, MAX_CU_LAYERS )`,
 * all of them, whatever the board has), the technical and user layers (`TechAndUserUIOrder`), then the GAL layers sorted by name (a case-sensitive
 * compare, `LayerName( a ) < LayerName( b )`). A board layer is labelled `m_board->GetLayerName( layer )` (`layerName`: its own name, else
 * `LayerName()`'s).
 */
export function pcbColorPageRows(layerName: (file: string) => string): Array<{ key: string; label: string }> {
  const copper = ["F.Cu", ...Array.from({ length: 30 }, (_, i) => `In${i + 1}.Cu`), "B.Cu"];
  const user = Array.from({ length: 45 }, (_, i) => `User.${i + 1}`);
  const board = [...copper, ...TECH_LAYERS, ...user].map((f) => ({ key: boardLayerColorKey(f), label: layerName(f) }));
  const gal = [...PCB_GAL_COLOR_ROWS].sort((a, b) => (a.label < b.label ? -1 : a.label > b.label ? 1 : 0));
  return [...board, ...gal];
}

/** The keys of `pcbColorPageRows` (`m_validLayers`), what New Theme... copies and Reset to Defaults resets. */
export function pcbColorPageKeys(): string[] {
  return pcbColorPageRows((f) => f).map((r) => r.key);
}

/** The person's themes as stored (state/colorThemeStore.ts' text): anything malformed is dropped, an unknown editor theme stays (findTheme falls back). */
export function parseStoredThemes(raw: string | null): StoredThemes {
  if (!raw) return EMPTY_STORED_THEMES;
  try {
    const j = JSON.parse(raw) as { pcbTheme?: unknown; themes?: unknown };
    const themes: StoredThemes["themes"] = [];
    if (Array.isArray(j.themes)) {
      for (const t of j.themes as Array<Record<string, unknown>>) {
        if (!t || typeof t.filename !== "string" || t.filename === "" || typeof t.name !== "string") continue;
        if (themes.some((x) => x.filename === t.filename)) continue;
        const colors: Record<string, string> = {};
        if (t.colors && typeof t.colors === "object") for (const [k, v] of Object.entries(t.colors as Record<string, unknown>)) if (typeof v === "string" && /^#[0-9a-f]{8}$/i.test(v)) colors[k] = v.toLowerCase();
        themes.push({ filename: t.filename, name: t.name, colors });
      }
    }
    return { pcbTheme: typeof j.pcbTheme === "string" && j.pcbTheme !== "" ? j.pcbTheme : DEFAULT_THEME, themes };
  } catch {
    return EMPTY_STORED_THEMES;
  }
}

export function serializeStoredThemes(s: StoredThemes): string {
  return JSON.stringify(s);
}
