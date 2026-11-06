// Preferences > <editor> > Toolbars (`PANEL_TOOLBAR_CUSTOMIZATION`, common/dialogs/panel_toolbar_customization.cpp at KiCad 8303b2ad). STUB: being ported on
// task-v2-prefs.
import type { PreferencesPageHandle } from "./preferencesPage";

export type ToolbarsPageEditor = "pcb" | "schematic" | "footprint" | "symbol" | "3d";

export function useToolbarsPage(_editor: ToolbarsPageEditor, _open: boolean, shown: boolean): PreferencesPageHandle {
  return { element: shown ? <div data-testid="toolbars-page-stub">Toolbars: not yet ported.</div> : null, commit: () => {}, cancel: () => {} };
}
