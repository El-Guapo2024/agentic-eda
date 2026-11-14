// The editors' colour themes (KiCad 8303b2ad): the theme registry of `SETTINGS_MANAGER` (common/settings/settings_manager.cpp), the built-in
// themes of `COLOR_SETTINGS::CreateBuiltinColorSettings` (common/settings/color_settings.cpp), and what the Colors pages of Preferences do with
// them (`PANEL_COLOR_SETTINGS`, common/dialogs/panel_color_settings.cpp; `PANEL_PCBNEW_COLOR_SETTINGS`, pcbnew/dialogs/panel_pcbnew_color_settings.cpp;
// `PANEL_EESCHEMA_COLOR_SETTINGS`, eeschema/dialogs/panel_eeschema_color_settings.cpp; `PANEL_SYM_COLOR_SETTINGS`, eeschema/dialogs/
// panel_sym_color_settings.cpp; `PANEL_FP_EDITOR_COLOR_SETTINGS`, pcbnew/dialogs/panel_fp_editor_color_settings.cpp).
//
//   * The themes: "KiCad Default" (`_builtin_default`, the colours of `s_defaultTheme`) and "KiCad Classic" (`_builtin_classic`, `s_classicTheme`),
//     both read-only (`m_writeFile = false`), and the person's own (KiCad: one `colors/<name>.json` each in the user's configuration folder). The
//     editor's `m_ColorTheme` defaults to `DEFAULT_THEME` ("user"), and `SETTINGS_MANAGER::GetColorSettings( "user" )` without a `user.json`
//     makes that theme on the spot as an editable copy of the default one -- so a first run edits "KiCad Default" (the copy) and lists the
//     built-in "KiCad Default (read-only)" beside it.
//   * A theme holds every namespace of a theme file, as one `COLOR_SETTINGS` does: the board's (the `CLR( "board.*", .. )` params: the copper,
//     technical and user layers and the board's GAL layers -- background, grid, ratsnest, DRC markers, net names, ...) and the schematic's
//     (`CLR( "schematic.*", .. )`: wires, buses, labels, pins, the symbol body, ...; and `schematic.override_item_colors`). Colours are
//     `#rrggbbaa` under the layer ids src/kicad/colors.json uses (`F_Cu`, `LAYER_PCB_BACKGROUND`, `LAYER_WIRE`). A theme read from a KiCad file
//     keeps the file's other entries (`extra`: its 3D viewer and Gerber viewer colours, keys of older versions) so that writing it gives them back
//     (kicad-port/colorThemeFile.ts).
//   * Each editor picks its own theme from the one list: pcbnew's, eeschema's, the Footprint Editor's (`m_ColorTheme` of each editor's settings)
//     and the Symbol Editor's, which by default is the schematic editor's (`SYMBOL_EDITOR_SETTINGS::m_UseEeschemaColorSettings`, default on:
//     `SYMBOL_EDIT_FRAME::GetColorSettings`). The PCB and Footprint Editor pages edit the board namespace, the schematic page the schematic one
//     (`m_colorNamespace`).
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

/** What the themes are made of: KiCad's default colours (colors.json, `s_defaultTheme`), the Classic theme's (color_themes.json) and the namespaces' layers. */
export interface ThemeData {
  /** `s_defaultTheme`: layer id -> `#rrggbbaa` (every layer of colors.json). */
  defaults: Readonly<Record<string, string>>;
  /** `s_classicTheme`'s board and schematic layers. */
  classic: Readonly<Record<string, string>>;
  /** The layers of the board namespace (`CLR( "board.*", .. )`), the colours the PCB and Footprint Editor themes decide. */
  boardKeys: readonly string[];
  /** The layers of the schematic namespace (`CLR( "schematic.*", .. )`), the colours the schematic and Symbol Editor themes decide. */
  schematicKeys?: readonly string[];
}

/** The editors that each pick a theme (`m_ColorTheme` of pcbnew, the Footprint Editor, eeschema and the Symbol Editor). */
export type ThemeEditor = "pcb" | "footprint" | "schematic" | "symbol";
export const THEME_EDITORS: readonly ThemeEditor[] = ["pcb", "footprint", "schematic", "symbol"];

/** The namespace an editor draws from: the board's for the PCB and Footprint Editors, the schematic's for the schematic and Symbol Editors. */
export function editorNamespace(editor: ThemeEditor): "board" | "schematic" {
  return editor === "pcb" || editor === "footprint" ? "board" : "schematic";
}

/** One theme of the registry (`COLOR_SETTINGS`). `colors` may leave layers out: those are KiCad's default colour (see `themeColor`). */
export interface ColorTheme {
  /** `GetFilename()`: the file's name without `.json` (a built-in's `_builtin_*`). */
  filename: string;
  /** `GetName()`: `meta.name`, what the list shows. */
  name: string;
  readOnly: boolean;
  colors: Record<string, string>;
  /** `schematic.override_item_colors` (`GetOverrideSchItemColors`, default off): the schematic draws every item in its layer's colour, never its own. */
  overrideSchItemColors?: boolean;
  /** A theme read from a KiCad file: the file's entries that are not a colour of the board or schematic namespace (kicad-port/colorThemeFile.ts). */
  extra?: Record<string, unknown>;
}

/** One of the person's themes as the store keeps it (KiCad's `colors/<filename>.json`). */
export interface StoredTheme {
  filename: string;
  name: string;
  colors: Record<string, string>;
  overrideSchItemColors?: boolean;
  extra?: Record<string, unknown>;
}

/**
 * What the person's store keeps: their own themes (KiCad's `colors/*.json`) and each editor's choice -- the board editor's (`PCBNEW_SETTINGS::m_ColorTheme`),
 * the schematic editor's (`EESCHEMA_SETTINGS::m_ColorTheme`), the Footprint Editor's (`FOOTPRINT_EDITOR_SETTINGS::m_ColorTheme`) and the Symbol Editor's
 * (`SYMBOL_EDITOR_SETTINGS::m_ColorTheme`, used only when `m_UseEeschemaColorSettings` is off). The last four are absent from a store written before they
 * existed: `editorThemeName` gives their defaults.
 */
export interface StoredThemes {
  pcbTheme: string;
  schTheme?: string;
  fpTheme?: string;
  symTheme?: string;
  /** `m_UseEeschemaColorSettings` ("Use schematic editor color theme"), default on. */
  symUseSchTheme?: boolean;
  themes: StoredTheme[];
}

export const EMPTY_STORED_THEMES: StoredThemes = { pcbTheme: DEFAULT_THEME, themes: [] };

/**
 * The filename of an editor's theme as stored: the editor's own choice, the defaults where the store has none (every editor's `DEFAULT_THEME`, as
 * pcbnew's here), and for the Symbol Editor the schematic editor's while it uses the schematic editor's theme (`SYMBOL_EDIT_FRAME::GetColorSettings`).
 */
export function editorThemeName(stored: StoredThemes, editor: ThemeEditor): string {
  if (editor === "pcb") return stored.pcbTheme;
  if (editor === "footprint") return stored.fpTheme ?? DEFAULT_THEME;
  if (editor === "symbol" && stored.symUseSchTheme === false) return stored.symTheme ?? DEFAULT_THEME;
  return stored.schTheme ?? DEFAULT_THEME;
}

/** The store with an editor's theme changed (each page's `TransferDataFromWindow`: `cfg->m_ColorTheme = m_currentSettings->GetFilename()`). The Symbol Editor's is its own `m_ColorTheme`. */
export function withEditorTheme(stored: StoredThemes, editor: ThemeEditor, filename: string): StoredThemes {
  if (editor === "pcb") return stored.pcbTheme === filename ? stored : { ...stored, pcbTheme: filename };
  const key = editor === "footprint" ? "fpTheme" : editor === "symbol" ? "symTheme" : "schTheme";
  return stored[key] === filename ? stored : { ...stored, [key]: filename };
}

/** `PANEL_SYM_COLOR_SETTINGS::TransferDataFromWindow`: "Use schematic editor color theme", or "Use theme:" and the theme chosen (`m_ColorTheme` is changed only then). */
export function withSymbolTheme(stored: StoredThemes, useSchematicTheme: boolean, filename: string): StoredThemes {
  const next: StoredThemes = { ...stored, symUseSchTheme: useSchematicTheme };
  if (!useSchematicTheme) next.symTheme = filename;
  return next;
}

/** `CreateBuiltinColorSettings`: "KiCad Default" from the params' defaults, "KiCad Classic" from `s_classicTheme`; neither is written (read-only). */
export function builtinThemes(data: ThemeData): ColorTheme[] {
  const def: Record<string, string> = {};
  for (const k of [...data.boardKeys, ...(data.schematicKeys ?? [])]) if (data.defaults[k]) def[k] = data.defaults[k]!;
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
    const theme: ColorTheme = { filename: t.filename, name: t.name, readOnly: false, colors: { ...t.colors } };
    if (t.overrideSchItemColors !== undefined) theme.overrideSchItemColors = t.overrideSchItemColors;
    if (t.extra !== undefined) theme.extra = t.extra;
    out.push(theme);
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

/**
 * `COLOR_SETTINGS::GetDefaultColor`'s fallback for a layer of no param: `s_userColors[ aLayer % s_userColors.size() ]` (common/settings/builtin_color_themes.h: (89, 148,
 * 220), (180, 219, 210), (216, 200, 82), (194, 194, 194)). The two schematic page rows of no `schematic.*` param in 8303b2ad are `LAYER_INTERSHEET_REFS` (1107) and
 * `LAYER_SHAPES_BACKGROUND` (1127) of include/layer_ids.h (`SCH_LAYER_ID_START` = `GAL_LAYER_ID_END` = 1096), both `% 4 == 3`: light grey (UI-LEFT-3 C3-12).
 */
const USER_COLOR_DEFAULTS: Readonly<Record<string, string>> = {
  LAYER_INTERSHEET_REFS: "#c2c2c2ff",
  LAYER_SHAPES_BACKGROUND: "#c2c2c2ff",
};

/** `COLOR_SETTINGS::GetDefaultColor`: the param's default, which is `s_defaultTheme`'s colour for every board layer; a layer of no param its `s_userColors` entry. */
export function defaultColor(key: string, data: ThemeData): string | undefined {
  return data.defaults[key] ?? USER_COLOR_DEFAULTS[key];
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
  // (a new theme's `m_overrideSchItemColors` is its param's default, off: `m_optOverrideColors->SetValue( newSettings->GetOverrideSchItemColors() )`)
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
export function setThemeColors(stored: StoredThemes, registry: readonly ColorTheme[], filename: string, colors: Readonly<Record<string, string>>, overrideSchItemColors?: boolean): StoredThemes {
  const theme = registry.find((t) => t.filename === filename);
  if (!theme || theme.readOnly) return stored;
  const have = stored.themes.find((t) => t.filename === filename);
  const withOverride = (t: StoredTheme): StoredTheme => (overrideSchItemColors === undefined ? t : { ...t, overrideSchItemColors });
  if (!have) {
    const fresh: StoredTheme = { filename, name: theme.name, colors: { ...theme.colors, ...colors } };
    if (theme.overrideSchItemColors !== undefined) fresh.overrideSchItemColors = theme.overrideSchItemColors;
    if (theme.extra !== undefined) fresh.extra = theme.extra;
    return { ...stored, themes: [...stored.themes, withOverride(fresh)] };
  }
  return { ...stored, themes: stored.themes.map((t) => (t.filename === filename ? withOverride({ ...t, colors: { ...t.colors, ...colors } }) : t)) };
}

/**
 * Themes read from files (Import Theme...): each is the person's under its filename, replacing a theme of theirs of that filename; the built-ins and
 * the editors' default "user" theme are replaced too only by a file of that name (KiCad would read `user.json` as "user"). A built-in's filename is not
 * taken: the theme comes in as "<filename>_imported".
 */
export function importThemes(stored: StoredThemes, themes: readonly StoredTheme[]): { stored: StoredThemes; filenames: string[] } {
  let list = [...stored.themes];
  const filenames: string[] = [];
  for (const t of themes) {
    const filename = t.filename === BUILTIN_DEFAULT || t.filename === BUILTIN_CLASSIC ? `${t.filename}_imported` : t.filename;
    list = [...list.filter((x) => x.filename !== filename), { ...t, filename }];
    filenames.push(filename);
  }
  return { stored: { ...stored, themes: list }, filenames };
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

/**
 * The schematic layers of the schematic editor's Colors page, `SCH_LAYER_ID_START` .. `SCH_LAYER_ID_END` in the enum's order (include/layer_ids.h) with
 * `LayerName()`'s text (common/layer_id.cpp), but `g_excludedLayers` (`LAYER_NOTES_BACKGROUND`, `LAYER_DANGLING`, `LAYER_NET_COLOR_HIGHLIGHT`,
 * `LAYER_GROUP`). Two have no `schematic.*` param in 8303b2ad and so no colour in a theme (`GetColor` is `UNSPECIFIED`, and `SaveColorSettings` writes
 * nothing for them): Sheet references and Shape fills.
 */
export const SCH_COLOR_LAYERS: ReadonlyArray<{ key: string; label: string }> = [
  { key: "LAYER_WIRE", label: "Wires" },
  { key: "LAYER_BUS", label: "Buses" },
  { key: "LAYER_JUNCTION", label: "Junctions" },
  { key: "LAYER_LOCLABEL", label: "Labels" },
  { key: "LAYER_GLOBLABEL", label: "Global labels" },
  { key: "LAYER_HIERLABEL", label: "Hierarchical labels" },
  { key: "LAYER_PINNUM", label: "Pin numbers" },
  { key: "LAYER_PINNAM", label: "Pin names" },
  { key: "LAYER_REFERENCEPART", label: "Symbol references" },
  { key: "LAYER_VALUEPART", label: "Symbol values" },
  { key: "LAYER_FIELDS", label: "Symbol fields" },
  { key: "LAYER_INTERSHEET_REFS", label: "Sheet references" },
  { key: "LAYER_NETCLASS_REFS", label: "Net class references" },
  { key: "LAYER_RULE_AREAS", label: "Rule areas" },
  { key: "LAYER_DEVICE", label: "Symbol body outlines" },
  { key: "LAYER_NOTES", label: "Schematic text && graphics" },
  { key: "LAYER_PRIVATE_NOTES", label: "Symbol private text && graphics" },
  { key: "LAYER_PIN", label: "Pins" },
  { key: "LAYER_SHEET", label: "Sheet borders" },
  { key: "LAYER_SHEETNAME", label: "Sheet names" },
  { key: "LAYER_SHEETFILENAME", label: "Sheet file names" },
  { key: "LAYER_SHEETFIELDS", label: "Sheet fields" },
  { key: "LAYER_SHEETLABEL", label: "Sheet pins" },
  { key: "LAYER_NOCONNECT", label: "No-connect symbols" },
  { key: "LAYER_DNP_MARKER", label: "DNP markers" },
  { key: "LAYER_ERC_WARN", label: "ERC warnings" },
  { key: "LAYER_ERC_ERR", label: "ERC errors" },
  { key: "LAYER_ERC_EXCLUSION", label: "ERC exclusions" },
  { key: "LAYER_EXCLUDED_FROM_SIM", label: "Excluded-from-simulation markers" },
  { key: "LAYER_SHAPES_BACKGROUND", label: "Shape fills" },
  { key: "LAYER_DEVICE_BACKGROUND", label: "Symbol body fills" },
  { key: "LAYER_SHEET_BACKGROUND", label: "Sheet backgrounds" },
  { key: "LAYER_SCHEMATIC_GRID", label: "Grid" },
  { key: "LAYER_SCHEMATIC_GRID_AXES", label: "Axes" },
  { key: "LAYER_SCHEMATIC_BACKGROUND", label: "Background" },
  { key: "LAYER_SCHEMATIC_CURSOR", label: "Cursor" },
  { key: "LAYER_HOVERED", label: "Hovered items" },
  { key: "LAYER_BRIGHTENED", label: "Highlighted items" },
  { key: "LAYER_HIDDEN", label: "Hidden items" },
  { key: "LAYER_DRAG_NET_COLLISION", label: "Drag net collisions" },
  { key: "LAYER_SELECTION_SHADOWS", label: "Selection highlight" },
  { key: "LAYER_SCHEMATIC_DRAWINGSHEET", label: "Drawing sheet" },
  { key: "LAYER_SCHEMATIC_PAGE_LIMITS", label: "Page limits" },
  { key: "LAYER_BUS_JUNCTION", label: "Bus junctions" },
  { key: "LAYER_SCHEMATIC_AUX_ITEMS", label: "Helper items" },
  { key: "LAYER_SCHEMATIC_ANCHOR", label: "Anchors" },
  { key: "LAYER_OP_VOLTAGES", label: "Operating point voltages" },
  { key: "LAYER_OP_CURRENTS", label: "Operating point currents" },
];

/**
 * The schematic Colors page's rows (`PANEL_EESCHEMA_COLOR_SETTINGS::createSwatches`): `SCH_COLOR_LAYERS` sorted by `LayerName()` (a case-sensitive compare,
 * `&&` as written), shown with a mnemonic's `&&` as `&`, "Axes" with " (symbol editor only)" after it. Sheet borders and Sheet backgrounds are shown only while
 * the theme overrides the items' own colours (`updateAllowedSwatches`: "If the theme is not overriding individual item colors then don't show them").
 */
export function schColorPageRows(overrideItemColors: boolean): Array<{ key: string; label: string }> {
  const sorted = [...SCH_COLOR_LAYERS].sort((a, b) => (a.label < b.label ? -1 : a.label > b.label ? 1 : 0));
  return sorted
    .filter((r) => overrideItemColors || (r.key !== "LAYER_SHEET" && r.key !== "LAYER_SHEET_BACKGROUND"))
    .map((r) => ({ key: r.key, label: r.key === "LAYER_SCHEMATIC_GRID_AXES" ? `${r.label} (symbol editor only)` : r.label.replace(/&&/g, "&") }));
}

/** The schematic page's `m_validLayers`: every row, shown or not (what New Theme... copies, Reset to Defaults resets and a save writes). */
export function schColorPageKeys(): string[] {
  return SCH_COLOR_LAYERS.map((r) => r.key);
}

/** `#ffffffff`: `COLOR4D::WHITE`. */
const WHITE = "#ffffffff";

/**
 * `PANEL_EESCHEMA_COLOR_SETTINGS::saveCurrentTheme`: "Do not allow non-background layers to be completely white. This ensures the BW printing recognizes that
 * the colors should be printed black" -- a white layer but the background and the sheet background is saved `Darken( 0.01 )`, (252, 252, 252) at its alpha.
 */
export function schSaveColors(colors: Readonly<Record<string, string>>): Record<string, string> {
  const out = { ...colors };
  for (const k of schColorPageKeys()) {
    if (out[k] !== WHITE || k === "LAYER_SCHEMATIC_BACKGROUND" || k === "LAYER_SHEET_BACKGROUND") continue;
    // r * ( 1.0 - 0.01 ) = 0.99, written `0.99 * 255 + 0.5` truncated (`COLOR4D::ToColour`): 252
    out[k] = "#fcfcfcff";
  }
  return out;
}

/** `PANEL_EESCHEMA_COLOR_SETTINGS::validateSave`'s question, word for word ("Warning", Yes / No). */
export const SCH_SAME_AS_BACKGROUND_QUESTION = "Some items have the same color as the background\nand they will not be seen on the screen.  Are you\nsure you want to use these colors?";

/**
 * `PANEL_EESCHEMA_COLOR_SETTINGS::validateSave` (asked by OK's `saveCurrentTheme( true )`, after the no-white rule): whether a schematic layer -- any of
 * `SCH_LAYER_ID_START` .. `SCH_LAYER_ID_END` (`keys`), but the background and the sheet background -- has exactly the background's colour (`COLOR4D ==`, the alpha
 * too). A layer of no colour (`UNSPECIFIED`) is never the background's (UI-LEFT-3 C3-12).
 */
export function schSameAsBackground(colorOf: (key: string) => string | undefined, keys: readonly string[]): boolean {
  const bg = colorOf("LAYER_SCHEMATIC_BACKGROUND")?.toLowerCase();
  if (!bg) return false;
  return keys.some((k) => k !== "LAYER_SCHEMATIC_BACKGROUND" && k !== "LAYER_SHEET_BACKGROUND" && colorOf(k)?.toLowerCase() === bg);
}

/**
 * The Footprint Editor's Colors page (`PANEL_FP_EDITOR_COLOR_SETTINGS`): F.Cu, "Internal Layers" (In1.Cu's colour) and B.Cu, then the board's GAL layers
 * (`GAL_LAYER_ID_START` .. `GAL_LAYER_ID_END`) but the vias' and the holes' (`LAYER_VIAS`, `LAYER_VIA_HOLES`, `LAYER_VIA_HOLEWALLS`, `LAYER_PAD_PLATEDHOLES`,
 * `LAYER_PAD_HOLEWALLS`) that have a colour in the theme (`GetColor( i ) != COLOR4D::UNSPECIFIED`: the `board.*` params of those layers), sorted by
 * `LayerName()` (`createSwatches`). The net names' layers are not GAL layers (`NETNAMES_LAYER_ID_START` .. ), Selection highlight has no param.
 */
export function fpColorPageRows(): Array<{ key: string; label: string }> {
  const gal = [...PCB_GAL_COLOR_ROWS.filter((r) => !["LAYER_VIA_HOLES", "LAYER_SELECT_OVERLAY", "NETNAMES_LAYER_ID_START", "LAYER_PAD_NETNAMES"].includes(r.key)), { key: "LAYER_POINTS", label: "Points" }].sort((a, b) =>
    a.label < b.label ? -1 : a.label > b.label ? 1 : 0
  );
  return [{ key: "F_Cu", label: "F.Cu" }, { key: "In1_Cu", label: "Internal Layers" }, { key: "B_Cu", label: "B.Cu" }, ...gal];
}

/** The Footprint Editor page's `m_validLayers`. */
export function fpColorPageKeys(): string[] {
  return fpColorPageRows().map((r) => r.key);
}

/** The person's themes as stored (state/colorThemeStore.ts' text): anything malformed is dropped, an unknown editor theme stays (findTheme falls back). */
export function parseStoredThemes(raw: string | null): StoredThemes {
  if (!raw) return EMPTY_STORED_THEMES;
  try {
    const j = JSON.parse(raw) as { pcbTheme?: unknown; schTheme?: unknown; fpTheme?: unknown; symTheme?: unknown; symUseSchTheme?: unknown; themes?: unknown };
    const themes: StoredThemes["themes"] = [];
    if (Array.isArray(j.themes)) {
      for (const t of j.themes as Array<Record<string, unknown>>) {
        if (!t || typeof t.filename !== "string" || t.filename === "" || typeof t.name !== "string") continue;
        if (themes.some((x) => x.filename === t.filename)) continue;
        const colors: Record<string, string> = {};
        if (t.colors && typeof t.colors === "object") for (const [k, v] of Object.entries(t.colors as Record<string, unknown>)) if (typeof v === "string" && /^#[0-9a-f]{8}$/i.test(v)) colors[k] = v.toLowerCase();
        const theme: StoredTheme = { filename: t.filename, name: t.name, colors };
        if (typeof t.overrideSchItemColors === "boolean") theme.overrideSchItemColors = t.overrideSchItemColors;
        if (t.extra && typeof t.extra === "object" && !Array.isArray(t.extra)) theme.extra = t.extra as Record<string, unknown>;
        themes.push(theme);
      }
    }
    const out: StoredThemes = { pcbTheme: typeof j.pcbTheme === "string" && j.pcbTheme !== "" ? j.pcbTheme : DEFAULT_THEME, themes };
    // the other editors' choices, kept only when the text has them (a store from before them reads as it was written)
    for (const k of ["schTheme", "fpTheme", "symTheme"] as const) if (typeof j[k] === "string" && j[k] !== "") out[k] = j[k] as string;
    if (typeof j.symUseSchTheme === "boolean") out.symUseSchTheme = j.symUseSchTheme;
    return out;
  } catch {
    return EMPTY_STORED_THEMES;
  }
}

export function serializeStoredThemes(s: StoredThemes): string {
  return JSON.stringify(s);
}
