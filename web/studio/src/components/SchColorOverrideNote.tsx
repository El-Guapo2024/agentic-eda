// The info bar the schematic's item dialogs open with while the editor's colour theme overrides the items' own colours (Preferences > Schematic Editor > Colors,
// "Override individual item colors"): `if( m_frame->GetColorSettings()->GetOverrideSchItemColors() ) m_infoBar->ShowMessage( _( "Note: individual item colors
// overridden in Preferences." ) )` in the constructors of `DIALOG_TEXT_PROPERTIES` (a text box only), `DIALOG_SHAPE_PROPERTIES`, `DIALOG_SHEET_PROPERTIES`,
// `DIALOG_TABLE_PROPERTIES` and `DIALOG_TABLECELL_PROPERTIES` (eeschema/dialogs/ at KiCad 8303b2ad). `m_frame` is the frame the dialog is opened from, so the
// Symbol Editor's text box and shape dialogs read the Symbol Editor's theme. The bar is shown when the dialog is made and stays (no timeout, `ShowMessage`).
import { shownTheme, useColorThemeStamp } from "../state/colorThemeStore";
import type { ThemeEditor } from "../kicad-port/colorTheme";

/** KiCad's message, word for word. */
export const COLOR_OVERRIDE_NOTE = "Note: individual item colors overridden in Preferences.";

/** `COLOR_SETTINGS::GetOverrideSchItemColors()` of the frame's theme (the Colors page's working copy while it is shown). */
export function itemColorsOverridden(editor: Extract<ThemeEditor, "schematic" | "symbol">): boolean {
  return shownTheme(editor).overrideSchItemColors ?? false;
}

/** The dialog's `WX_INFOBAR` with the note, or nothing while the theme keeps the items' own colours. */
export function SchColorOverrideNote({ editor = "schematic" }: { editor?: Extract<ThemeEditor, "schematic" | "symbol"> }) {
  useColorThemeStamp();
  if (!itemColorsOverridden(editor)) return null;
  return (
    <div role="status" data-testid="color-override-note" style={{ padding: "6px 12px", background: "var(--chrome-accent-bg, rgba(74,163,255,0.15))", fontSize: 12 }}>
      {COLOR_OVERRIDE_NOTE}
    </div>
  );
}
