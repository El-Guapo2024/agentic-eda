// The Text tool's dialog rules (`DIALOG_TEXT_PROPERTIES`, eeschema/dialogs/dialog_text_properties.cpp, as `SYMBOL_EDITOR_DRAWING_TOOLS::TwoClickPlace`
// uses it for `eeschema.SymbolDrawing.placeSymbolText`).

/** `cfg->m_Defaults.text_size` = `DEFAULT_TEXT_SIZE` (eeschema/default_values.h): 50 mil. */
export const DEFAULT_SYMBOL_TEXT_SIZE_MM = 1.27;

/** `NoPrintableChars`: the text is empty once its ends are trimmed -- `TwoClickPlace` drops such a text (`dlg.ShowModal() != wxID_OK || NoPrintableChars( text->GetText() )`). */
export function noPrintableChars(text: string): boolean {
  return text.trim() === "";
}

/** `TransferDataFromWindow`: "Don't allow text to disappear" -- `m_textSize.Validate( 0.01, 1000.0, EDA_UNITS::MM )`. */
export function textSizeError(sizeMm: number): string | null {
  if (!Number.isFinite(sizeMm) || sizeMm < 0.01 || sizeMm > 1000) return "Text size must be between 0.01 mm and 1000 mm.";
  return null;
}

/** The dialog's Horizontal / Vertical buttons: `ANGLE_HORIZONTAL` (0) or `ANGLE_VERTICAL` (90). */
export function textAngleDeg(vertical: boolean): number {
  return vertical ? 90 : 0;
}
