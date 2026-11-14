// `EDA_BASE_FRAME::ShowPreferences( aStartPage, aStartParentPage )` (common/eda_base_frame.cpp, KiCad 8303b2ad): the page the Preferences dialog opens on when a tool
// asks for one -- `COMMON_TOOLS::GridProperties` ("Edit Grids...") opens it on the editor's Grids page (`ShowPreferences( _( "Grids" ), <the editor's name> )`,
// `PAGED_DIALOG::SetInitialPage`), UI-LEFT-3 C3-5. Without one the dialog opens on the page it was last closed on (`g_lastPage`). Plain module state: the action
// sets it and opens the dialog; components/PreferencesDialog.tsx takes it once as it opens.
import type { GridEditor } from "../kicad-port/gridSettings";

/** A page asked for: an editor's Grids page. */
export type PreferencesStartPage = { grids: GridEditor };

let pending: PreferencesStartPage | null = null;

/** The page the next opening of the dialog shows (`SetInitialPage`). */
export function setPreferencesStartPage(page: PreferencesStartPage): void {
  pending = page;
}

/** The page asked for, once: the dialog takes it as it opens. */
export function takePreferencesStartPage(): PreferencesStartPage | null {
  const p = pending;
  pending = null;
  return p;
}
