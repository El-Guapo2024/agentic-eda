// Preferences > Footprint Editor > Footprint Defaults and > User Layer Names (`PANEL_FP_EDITOR_FIELD_DEFAULTS`, `PANEL_FP_USER_LAYER_NAMES` at KiCad 8303b2ad).
// STUB: being ported on task-v2-prefs.
import type { PreferencesPageHandle } from "./preferencesPage";

export function useFpFieldDefaultsPage(_open: boolean, shown: boolean): PreferencesPageHandle {
  return { element: shown ? <div data-testid="fp-defaults-page-stub">Footprint Defaults: not yet ported.</div> : null, commit: () => {}, cancel: () => {} };
}

export function useFpLayerNamesPage(_open: boolean, shown: boolean): PreferencesPageHandle {
  return { element: shown ? <div data-testid="fp-layer-names-page-stub">User Layer Names: not yet ported.</div> : null, commit: () => {}, cancel: () => {} };
}
