// Preferences > PCB Editor > Colors: `PANEL_PCBNEW_COLOR_SETTINGS` (pcbnew/dialogs/panel_pcbnew_color_settings.cpp) over `PANEL_COLOR_SETTINGS`
// (common/dialogs/panel_color_settings.cpp), KiCad 8303b2ad: the page every theme-editing Colors page is (components/ThemeColorsPage.tsx), over the
// board editor's theme and the board namespace, its swatches in `createSwatches`' order (`pcbColorPageRows`), each board layer under the board's own
// name for it. The rules are kicad-port/colorTheme.ts'; the themes are the person's (state/colorThemeStore.ts).
import { useMemo } from "react";
import { useStudioState } from "../state/store";
import { pcbColorPageKeys, pcbColorPageRows } from "../kicad-port/colorTheme";
import { layerDisplayName } from "../kicad-port/boardLayers";
import { PcbColorPreview } from "./ColorPreview";
import { useThemeColorsPage, type ThemeColorsPageConfig } from "./ThemeColorsPage";
import type { ReactNode } from "react";

const PAGE_KEYS = pcbColorPageKeys();

export interface PcbColorsPage {
  element: ReactNode;
  /** `ResetPanel`. */
  reset: () => void;
  resetLabel?: string;
  resetTooltip?: string;
  /** OK: `saveCurrentTheme( true )` and `cfg->m_ColorTheme = m_currentSettings->GetFilename()`. */
  commit: () => void;
  /** Cancel: the board goes back to the editor's theme. */
  cancel: () => void;
}

/** The page's state lives with the Preferences dialog (it outlives a switch to another page, as a `wxTreebook` page does). `open`: the dialog is up. */
export function usePcbColorsPage(open: boolean, shown: boolean): PcbColorsPage {
  const state = useStudioState();
  const setup = state.board?.board_rules?.layer_setup;
  // a board layer is labelled `m_board->GetLayerName( layer )` (its own name, else `LayerName()`'s)
  const config = useMemo<ThemeColorsPageConfig>(() => {
    const rows = pcbColorPageRows((file) => layerDisplayName(setup, file));
    return { editor: "pcb", testId: "pcb-colors", rows: () => rows, pageKeys: PAGE_KEYS, backgroundKey: "LAYER_PCB_BACKGROUND", preview: (working) => <PcbColorPreview working={working} /> };
  }, [setup]);
  const page = useThemeColorsPage(config, open, shown);
  return { element: page.element, reset: page.reset!, resetLabel: page.resetLabel, resetTooltip: page.resetTooltip, commit: page.commit, cancel: page.cancel };
}
