// Preferences > Schematic Editor / Symbol Editor / Footprint Editor > Colors (`PANEL_EESCHEMA_COLOR_SETTINGS`, `PANEL_SYM_COLOR_SETTINGS`,
// `PANEL_FP_EDITOR_COLOR_SETTINGS` at KiCad 8303b2ad). STUB: being ported on task-v2-prefs.
import type { PreferencesPageHandle } from "./preferencesPage";

export type ColorsPageEditor = "schematic" | "symbol" | "footprint";

export function useEditorColorsPage(_editor: ColorsPageEditor, _open: boolean, shown: boolean): PreferencesPageHandle {
  return { element: shown ? <div data-testid="colors-page-stub">Colors: not yet ported.</div> : null, commit: () => {}, cancel: () => {} };
}
