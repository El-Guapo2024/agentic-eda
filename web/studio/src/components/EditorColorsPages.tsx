// Preferences > Schematic Editor / Symbol Editor / Footprint Editor > Colors, KiCad 8303b2ad:
//   * Schematic Editor > Colors: `PANEL_EESCHEMA_COLOR_SETTINGS` (eeschema/dialogs/panel_eeschema_color_settings.cpp) -- the page of every themed Colors
//     page (components/ThemeColorsPage.tsx) over the schematic editor's theme (`EESCHEMA_SETTINGS::m_ColorTheme`) and the schematic namespace
//     (`m_colorNamespace = "schematic"`): `SCH_LAYER_ID_START` .. `SCH_LAYER_ID_END` but `g_excludedLayers`, sorted by `LayerName()`, "Axes (symbol editor
//     only)", "Override individual item colors", no layer but the backgrounds saved pure white (kicad-port/colorTheme.ts `schColorPageRows`,
//     `schSaveColors`). The schematic canvas draws the page's colours while it is shown.
//   * Symbol Editor > Colors: `PANEL_SYM_COLOR_SETTINGS` (eeschema/dialogs/panel_sym_color_settings.cpp) -- "Use schematic editor color theme" or "Use
//     theme:" with the themes by name (`GetColorSettingsList`, `GetName()`, no "(read-only)"); picking a theme picks "Use theme:" (`OnThemeChanged`);
//     OK keeps both (`m_UseEeschemaColorSettings`, and `m_ColorTheme` when a theme of its own is used). No swatches, no reset button (the panel is no
//     `RESETTABLE_PANEL`) and no preview.
//   * Footprint Editor > Colors: `PANEL_FP_EDITOR_COLOR_SETTINGS` (pcbnew/dialogs/panel_fp_editor_color_settings.cpp) -- the board namespace over the
//     Footprint Editor's own theme (`FOOTPRINT_EDITOR_SETTINGS::m_ColorTheme`): F.Cu, "Internal Layers", B.Cu and the GAL layers with a colour
//     (`fpColorPageRows`); no override box (`m_optOverrideColors->Hide()`). The Footprint Editor's canvas draws the page's colours while it is shown.
import { useEffect, useState } from "react";
import { THEME_PARAMS, getColorThemes, selectSymbolTheme, useColorThemes } from "../state/colorThemeStore";
import { DEFAULT_THEME, SCH_SAME_AS_BACKGROUND_QUESTION, findTheme, fpColorPageKeys, fpColorPageRows, schColorPageKeys, schColorPageRows, schSameAsBackground, schSaveColors, sortedThemes } from "../kicad-port/colorTheme";
import { useThemeColorsPage, type ThemeColorsPageConfig } from "./ThemeColorsPage";
import type { PreferencesPageHandle } from "./preferencesPage";

export type ColorsPageEditor = "schematic" | "symbol" | "footprint";

/** The schematic layers (`SCH_LAYER_ID`): the `schematic.*` params' and the page's rows (the two rows of no param among them). */
const SCH_ALL_LAYER_KEYS: readonly string[] = [...new Set([...THEME_PARAMS.filter((p) => p.key.startsWith("schematic.")).map((p) => p.layer), ...schColorPageKeys()])];

const SCH_PAGE: ThemeColorsPageConfig = {
  editor: "schematic",
  testId: "sch-colors",
  rows: schColorPageRows,
  pageKeys: schColorPageKeys(),
  backgroundKey: "LAYER_SCHEMATIC_BACKGROUND",
  overrideOption: true,
  saveColors: schSaveColors,
  // `validateSave`: every schematic layer (`SCH_LAYER_ID_START` .. `SCH_LAYER_ID_END`: the page's and the ones it leaves out) against the background
  validateSave: (colorOf) => (schSameAsBackground(colorOf, SCH_ALL_LAYER_KEYS) ? SCH_SAME_AS_BACKGROUND_QUESTION : null),
};

const FP_PAGE: ThemeColorsPageConfig = {
  editor: "footprint",
  testId: "fp-colors",
  rows: () => fpColorPageRows(),
  pageKeys: fpColorPageKeys(),
  backgroundKey: "LAYER_PCB_BACKGROUND",
};

/** One editor's Colors page; `editor` is the same at each call site (the hooks it calls do not change between renders). */
export function useEditorColorsPage(editor: ColorsPageEditor, open: boolean, shown: boolean): PreferencesPageHandle {
  // eslint-disable-next-line react-hooks/rules-of-hooks
  if (editor === "symbol") return useSymbolColorsPage(open, shown);
  // eslint-disable-next-line react-hooks/rules-of-hooks
  const page = useThemeColorsPage(editor === "schematic" ? SCH_PAGE : FP_PAGE, open, shown);
  return { element: page.element, reset: page.reset, resetLabel: page.resetLabel, resetTooltip: page.resetTooltip, commit: page.commit, cancel: page.cancel, confirmOk: page.confirmOk };
}

/** `PANEL_SYM_COLOR_SETTINGS`. */
function useSymbolColorsPage(open: boolean, shown: boolean): PreferencesPageHandle {
  const themes = useColorThemes();
  const initial = () => {
    const s = getColorThemes().stored;
    // `GetColorSettings( cfg->m_ColorTheme )`: the Symbol Editor's own theme, whichever radio is on
    return { useSch: s.symUseSchTheme !== false, theme: findTheme(getColorThemes().registry, s.symTheme ?? DEFAULT_THEME).filename };
  };
  const [draft, setDraft] = useState(initial);
  useEffect(() => {
    if (open) setDraft(initial());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const element = !shown ? null : (
    <div data-testid="sym-colors-page" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <label className="filter-row">
        <input type="radio" name="sym-colors-use" data-testid="sym-colors-use-sch" checked={draft.useSch} onChange={() => setDraft((d) => ({ ...d, useSch: true }))} />
        Use schematic editor color theme
      </label>
      <div className="filter-row" style={{ gap: 8 }}>
        <label className="filter-row" style={{ margin: 0 }}>
          <input type="radio" name="sym-colors-use" data-testid="sym-colors-use-theme" checked={!draft.useSch} onChange={() => setDraft((d) => ({ ...d, useSch: false }))} />
          Use theme:
        </label>
        <select aria-label="Theme" data-testid="sym-colors-theme" value={draft.theme} onChange={(e) => setDraft({ useSch: false, theme: e.target.value })} style={{ minWidth: 200 }}>
          {sortedThemes(themes.registry).map((t) => (
            <option key={t.filename} value={t.filename}>
              {t.name}
            </option>
          ))}
        </select>
      </div>
    </div>
  );

  return {
    element,
    commit: () => {
      const s = getColorThemes().stored;
      if (draft.useSch !== (s.symUseSchTheme !== false) || (!draft.useSch && draft.theme !== (s.symTheme ?? DEFAULT_THEME))) selectSymbolTheme(draft.useSch, draft.theme);
    },
    cancel: () => {},
  };
}
