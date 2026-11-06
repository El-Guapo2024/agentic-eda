// Preferences > Hotkeys (`PANEL_HOTKEYS_EDITOR`, common/dialogs/panel_hotkeys_editor.cpp at KiCad 8303b2ad). STUB: being ported on task-v2-prefs.
import type { PreferencesPageHandle } from "./preferencesPage";

export function useHotkeysPage(_open: boolean, shown: boolean): PreferencesPageHandle {
  return { element: shown ? <div data-testid="hotkeys-page-stub">Hotkeys: not yet ported.</div> : null, commit: () => {}, cancel: () => {} };
}
